//! React OrderItemEditor parity for a selected reception product row.
//! The dialog only stages employee intent. Catalog search is Rust-backed;
//! the native manual reducer validates the actual item and recalculates price.

use crate::ui_layout::geometry::UiWidthPolicyExt as _;
use crate::ui_layout::geometry::SharedAllocationExt as _;
use std::sync::Arc;
use eframe::egui::{self,Context,Vec2};
use serde_json::Value;
use crate::{
    gateway::BandoGateway,
    product_manager::ProductManager,
    product_images::ProductImageCache,
    theme::{self, ButtonRole},
    typography,
};

fn field<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
// The reception item editor is a product-inspection dialog, not a compact
// search result row. Scale the image with the Kakao-safe work area while
// keeping room for the shared title/header, messages and action footer.
const ITEM_DETAIL_PHOTO_MAX:f32=300.0;
const ITEM_DETAIL_PHOTO_MIN:f32=176.0;
const ITEM_DETAIL_PANEL_GAP:f32=theme::SPACE_12;
const ITEM_DETAIL_STACK_WIDTH:f32=510.0;
fn item_detail_photo_side(available_width:f32,work_height:f32)->f32{
    (available_width*0.42)
        .min(ITEM_DETAIL_PHOTO_MAX)
        .min((work_height-240.0).max(ITEM_DETAIL_PHOTO_MIN))
        .max(ITEM_DETAIL_PHOTO_MIN)
}

/// Single product-click policy shared by both reception worksheets.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ProductClickIntent{None,OpenDetail,SelectAssignmentTarget}

pub fn product_click_intent(
    clicked:bool,editable:bool,assignment_requested:bool
)->ProductClickIntent{
    if !clicked || !editable{ProductClickIntent::None}
    else if assignment_requested{ProductClickIntent::SelectAssignmentTarget}
    else{ProductClickIntent::OpenDetail}
}

pub enum Event{
    Closed,
    Apply(Value),
    Remove,
}
pub struct ItemEditor{
    pub order_id:String,
    pub source_raw:String,
    pub recipient_id:String,
    pub item_id:String,
    pub client_id:String,
    initial:Value,
    initial_price:Option<i64>,
    manual_draft:bool,
    replacement:Option<Value>,
    search:Option<ProductManager>,
    original_catalog_product:Option<Value>,
    photo_read:crate::screen_refresh::ReadRefresh<Option<Value>>,
    photo_requested:bool,
    photos:ProductImageCache,
    confirm_delete:bool,
    delete_only:bool,
    confirm_discard:bool,
    error:Option<String>,
}
impl ItemEditor{
    pub fn new(
        order_id:String,source_raw:String,recipient_id:String,
        item:Value,client_id:String,price:Option<i64>
    )->Self{
        let item_id=field(&item,"id").to_owned();
        Self{
            order_id,source_raw,recipient_id,item_id,client_id,
            initial:item,initial_price:price,manual_draft:false,replacement:None,
            search:None,original_catalog_product:None,
            photo_read:Default::default(),photo_requested:false,
            photos:ProductImageCache::default(),
            confirm_delete:false,delete_only:false,confirm_discard:false,
            error:None,
        }
    }
    /// The draft may choose a product before quantity confirmation. It still
    /// uses the same independent-variant validation as ordinary reception.
    pub fn for_manual_draft(mut self)->Self{self.manual_draft=true;self}
    fn direct_manual_search(&self)->bool{
        self.manual_draft &&
            (field(&self.initial,"productId").is_empty() || field(&self.initial,"variantId").is_empty())
    }
    pub fn top_window_id(&self)->&'static str{
        if !self.delete_only && (self.direct_manual_search() || self.search.is_some()){
            "native-manual-product-search"
        }
        else if self.confirm_delete || self.delete_only{"reception-item-delete-confirm"}
        else if self.confirm_discard{"reception-item-discard-confirm"}
        else{"reception-item-detail"}
    }
    pub fn escape(&mut self,busy:bool)->Option<Event>{
        if busy{return None;}
        if !self.delete_only && self.direct_manual_search(){
            self.search=None;
            return Some(Event::Closed);
        }
        if self.search.take().is_some(){return None;}
        if self.confirm_delete{self.confirm_delete=false;return self.delete_only.then_some(Event::Closed);}
        if self.confirm_discard{self.confirm_discard=false;return None;}
        self.close()
    }
    pub fn is_same_target(&self,order_id:&str,raw:&str,rid:&str,iid:&str)->bool{
        self.order_id==order_id && self.source_raw==raw &&
            self.recipient_id==rid && self.item_id==iid
    }
    pub fn open_delete_only(&mut self){self.delete_only=true;self.confirm_delete=true;}
    pub fn report_error(&mut self,error:String){self.error=Some(error);}
    pub fn clear_error(&mut self){self.error=None;}
    fn selected(&self)->&Value{
        self.replacement.as_ref().unwrap_or(&self.initial)
    }
    pub fn can_apply(&self)->bool{
        let Some(candidate)=self.replacement.as_ref()else{return false};
        let variants=candidate.get("variants").and_then(Value::as_array);
        let valid_variant=variants.is_some_and(|v|v.len()==1 &&
            v[0].get("id").and_then(Value::as_str).is_some_and(|id|!id.is_empty())
        ) && candidate["requiresOption"]!=true;
        let quantity=self.initial.get("quantity").and_then(Value::as_i64)
            .is_some_and(|v|v>=1 && v<=9_007_199_254_740_991);
        let product_changed=field(candidate,"id")!=field(&self.initial,"productId")
            || variants.and_then(|v|v.first())
                .map(|v|field(v,"id"))
                !=Some(field(&self.initial,"variantId"));
        valid_variant && (quantity || self.manual_draft) && product_changed
    }
    fn close(&mut self)->Option<Event>{
        if self.replacement.is_some(){
            self.confirm_discard=true;
            None
        }else{Some(Event::Closed)}
    }
    fn open_search(&mut self,gateway:Arc<dyn BandoGateway>,ctx:&Context){
        let raw_name=field(&self.initial,"name");
        let query=raw_name.split(" / ").next().unwrap_or(raw_name).to_owned();
        self.search=Some(ProductManager::new_selection_with_query(
            gateway,ctx.clone(),self.client_id.clone(),query
        ));
    }
    /// Existing field names and mutations are retained; each label/value
    /// shares one horizontal row instead of three stacked caption pairs.
    /// Product replacement belongs inside this information column, not in
    /// an orphaned action row under the thumbnail.
    fn detail_information(
        &mut self,ui:&mut egui::Ui,name:&str,category:&str,price:&str,
        gateway:Arc<dyn BandoGateway>,ctx:&Context,
        busy:bool,edited:bool,align_height:Option<f32>
    ){
        let top=ui.cursor().top();
        crate::popup::modal_form_row_spacing(ui);
        crate::popup::modal_labeled_row(ui,"상품명",|row|{
            row.label(typography::subsection_heading(name));
        });
        crate::popup::modal_labeled_row(ui,"카테고리",|row|{
            row.label(typography::body(category));
        });
        crate::popup::modal_labeled_row(ui,"가격",|row|{
            row.label(typography::data_value_text(
                price,typography::DataValueKind::Money,
                typography::DataValueContext::Form
            ).line_height(Some(22.0)));
        });
        if let Some(height)=align_height{
            // Dynamic text may wrap. Only reserve the unused vertical
            // portion, never push the footer below a long product name.
            let used=ui.cursor().top()-top;
            ui.add_space((height-used-theme::ACTION_STANDARD_HEIGHT
                -theme::SPACE_8).max(theme::SPACE_8));
        }else{
            ui.add_space(theme::SPACE_12);
        }
        ui.horizontal(|controls|{
            if crate::button::action(
                controls,"상품 변경",ButtonRole::Standard,!busy
            ).clicked(){
                self.open_search(Arc::clone(&gateway),ctx);
            }
            if crate::button::action(
                controls,"선택 취소",ButtonRole::Standard,edited&&!busy
            ).clicked(){
                self.replacement=None;
                self.error=None;
            }
        });
    }

    fn poll_catalog_photo(&mut self,gateway:Arc<dyn BandoGateway>,ctx:&Context){
        if !self.photo_requested{
            self.photo_requested=true;
            let id=field(&self.initial,"productId").to_owned();
            if !id.is_empty(){
                self.photo_read.submit(ctx.clone(),move||{
                    gateway.load_catalog().map(|catalog|
                        catalog.get("products").and_then(Value::as_array)
                            .and_then(|products|products.iter().find(|p|field(p,"id")==id)).cloned())
                });
            }
        }
        if let Some(Ok(product))=self.photo_read.poll(){
            self.original_catalog_product=product;
            self.photo_read.verified();
        }
    }
    // Both entry points use the same common modal header, footer and action.
    fn show_delete_confirmation(
        &self,ctx:&Context,work_area:egui::Rect,busy:bool
    )->(bool,bool){
        let name=field(&self.initial,"name");
        let name=if name.is_empty(){field(&self.initial,"code")}else{name};
        crate::composite_reception::reception_item_delete_confirmation(
            ctx,work_area,name,self.error.as_deref(),busy
        )
    }

    pub fn show(
        &mut self,ctx:&Context,gateway:Arc<dyn BandoGateway>,
        work_area:egui::Rect,busy:bool
    )->Option<Event>{
        if self.delete_only {
            let (cancel,remove)=self.show_delete_confirmation(ctx,work_area,busy);
            return if cancel{Some(Event::Closed)}else if remove{Some(Event::Remove)}else{None};
        }
        // An unresolved draft has no confirmed catalog detail to inspect.
        // Reuse the common picker and return the same Apply event directly;
        // retain it until the native action succeeds (or the user cancels).
        if self.direct_manual_search(){
            if self.search.is_none(){self.open_search(Arc::clone(&gateway),ctx);}
            let (cancelled,product)={
                let search=self.search.as_mut().expect("manual product picker");
                let (cancelled,chosen)=search.show_selection_modal_with_state(
                    ctx,work_area,"native-manual-product-search",busy,self.error.as_deref()
                );
                (cancelled,chosen.map(|selection|search.selection_product(&selection)))
            };
            if busy{return None;}
            if cancelled{
                self.search=None;
                return Some(Event::Closed);
            }
            if let Some(product)=product{
                self.replacement=product;
                if self.can_apply(){
                    self.error=None;
                    return self.replacement.clone().map(Event::Apply);
                }
                self.replacement=None;
                self.error=Some("선택한 상품 정보를 확인할 수 없습니다.".into());
            }
            return None;
        }
        self.poll_catalog_photo(Arc::clone(&gateway),ctx);
        let mut result=None;
        let selected=self.selected().clone();
        let name=field(&selected,"name");
        let name=if name.is_empty(){field(&selected,"code")}else{name};
        let name=if name.is_empty(){"상품 확인 필요"}else{name};
        let edited=self.replacement.is_some();
        let category=field(&selected,"category");
        let category=if category.is_empty(){
            if edited || !field(&self.initial,"productId").is_empty(){
                "등록 상품"
            }else{"등록 상품 미연결"}
        }else{category};
        let price=if edited {
            "가격 확인 필요".to_owned()
        }else{
            self.initial_price.map(crate::display_format::display_won)
                .unwrap_or_else(||"가격 확인 필요".into())
        };
        let allow_apply=!busy && self.can_apply();
        let invalid_selection=edited && !self.can_apply();
        let mut delete_clicked=false;
        let mut apply_clicked=false;
        let needs_customer=self.initial["modelProposal"]["required"]==true;
        let safe_width=work_area.width()-theme::MANUAL_DIALOG_SIDE_GAP;
        crate::popup::modal_window(ctx,
            "상품 상세 수정","reception-item-detail",work_area
        )
            .default_width(740.0_f32.min(safe_width).max(0.0))
            .constrain_to(work_area)
            .show(ctx,|ui|{

                crate::popup::modal_flow_workspace(ui,|workspace|{
                    if workspace.header("상품 상세 수정","닫기",!busy && self.search.is_none()
                    ){
                        result=self.close();
                    }
                    if self.search.is_some() || self.confirm_delete || self.confirm_discard{
                        workspace.disable();
                    }
                    workspace.scroll_region("reception-item-detail-body",crate::popup::modal_editor_body_max_height(work_area.height()),|ui|{
                        let outer_width=ui.available_width().max(1.0);
                        // The approved nested frame has an 8pt internal inset on
                        // each side. Its content fills the whole modal, not just
                        // the natural 128pt thumbnail + a short text column.
                        let content_width=(outer_width-2.0*theme::SPACE_8
                            -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
                        let photo_side=item_detail_photo_side(
                            content_width,work_area.height()
                        );
                        crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::Standard).show(ui,|body|{
                            body.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::FillAvailable,content_width);
                            // Own the display snapshot so search and selection
                            // actions can mutate ItemEditor during this same frame.
                            let product=self.replacement.as_ref()
                                .or(self.original_catalog_product.as_ref())
                                .unwrap_or(&self.initial).clone();
                            if content_width<ITEM_DETAIL_STACK_WIDTH {
                                // Narrow Kakao-safe workspace: the image still grows
                                // to a useful size; rows and actions stay readable
                                // instead of being squeezed beside the image.
                                let (photo,_)=body.shared_allocate_exact_size(
                                    Vec2::splat(photo_side),egui::Sense::hover()
                                );
                                self.photos.paint(body,&product,photo);
                                body.add_space(theme::SPACE_12);
                                self.detail_information(
                                    body,name,category,&price,Arc::clone(&gateway),
                                    ctx,busy,edited,None
                                );
                            }else{
                                body.horizontal(|line|{
                                    // No implicit spacing around the explicit 16pt
                                    // image-to-details gutter: fill the card exactly.
                                    crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ExplicitHorizontal);
                                    let (photo,_)=line.shared_allocate_exact_size(
                                        Vec2::splat(photo_side),egui::Sense::hover()
                                    );
                                    self.photos.paint(line,&product,photo);
                                    line.add_space(ITEM_DETAIL_PANEL_GAP);
                                    let detail_width=(content_width-photo_side
                                        -ITEM_DETAIL_PANEL_GAP).max(1.0);
                                    line.vertical(|details|{
                                        details.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Fixed,detail_width);
                                        crate::card::apply_height_policy(
                                            details,crate::card::CardHeightPolicy::MatchTallest,photo_side
                                        );
                                        self.detail_information(
                                            details,name,category,&price,
                                            Arc::clone(&gateway),ctx,
                                            busy,edited,Some(photo_side)
                                        );
                                    });
                                });
                            }
                        });
                        // Validation and customer guidance share one fixed
                        // slot. A new error can never move the modal footer.
                        let mut status_messages=Vec::new();
                        if let Some(error)=&self.error{
                            status_messages.push(crate::user_error::display("reception.item",error).into_owned());
                        }
                        if invalid_selection{
                            status_messages.push(
                                if self.manual_draft{
                                    "판매 구성이 하나인 독립 상품만 적용할 수 있습니다."
                                }else{
                                    "수량이 확정되고 판매 구성이 하나인 독립 상품만 적용할 수 있습니다."
                                }.to_owned()
                            );
                        }
                        if needs_customer{
                            status_messages.push("추천 상품 고객 확인 필요".to_owned());
                        }
                        let status_text=status_messages.join(" · ");
                        let status_color=if self.error.is_some()||invalid_selection{
                            theme::message_ink(theme::MessageInkRole::Danger)
                        }else{theme::text_ink(theme::TextRole::Label,theme::TextState::Normal)};
                        let status_width=ui.available_width();
                        crate::ui_layout::fixed_status_slot(
                            ui,status_width,typography::METADATA_LINE_HEIGHT,
                            (!status_text.is_empty()).then_some(status_text.as_str()),
                            (!status_text.is_empty()).then_some(status_text.as_str()),
                            status_color
                        );
                    });
                    workspace.footer(|left|{
                            if crate::button::action(left,"삭제",ButtonRole::Danger,!busy)
                                .clicked(){
                                delete_clicked=true;
                            }
                        },
                        |right|{
                            if crate::button::action(
                                right,"적용",ButtonRole::Primary,allow_apply
                            ).clicked(){
                                apply_clicked=true;
                            }
                        }
                    );
                });
            });
        if delete_clicked{self.confirm_delete=true;}
        if apply_clicked{result=self.replacement.clone().map(Event::Apply);}
        if self.search.is_some() || self.confirm_delete || self.confirm_discard{
            crate::popup::modal_backdrop(ctx,"reception-item-nested-backdrop",true);
        }
        if let Some(search)=self.search.as_mut(){
            let (cancelled,chosen)=search.show_selection_modal(
                ctx,work_area,"native-manual-product-search"
            );
            if cancelled{
                self.search=None;
            }else if let Some(selection)=chosen{
                if let Some(product)=search.selection_product(&selection){
                    self.replacement=Some(product);
                    self.error=None;
                    self.search=None;
                }else{
                    self.error=Some("선택한 상품 정보를 확인할 수 없습니다.".into());
                }
            }
        }
        if self.confirm_delete{
            let (cancel,remove)=self.show_delete_confirmation(ctx,work_area,busy);
            if cancel || remove{self.confirm_delete=false;}
            if remove && !cancel{result=Some(Event::Remove);}
        }
        if self.confirm_discard{
            match crate::popup::show_discard_confirmation(
                ctx,work_area,"reception-item-discard-confirm","변경사항 확인",
                "선택한 상품 변경사항을 버릴까요?","돌아가기","수정 내용 버리기",true
            ){
                crate::popup::ChangeDecision::Cancel=>self.confirm_discard=false,
                crate::popup::ChangeDecision::Discard=>{
                    self.confirm_discard=false;
                    result=Some(Event::Closed);
                }
                _=>{}
            }
        }
        result
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn dialog()->ItemEditor{
        ItemEditor::new(
            "order-1".into(),"원문".into(),"r1".into(),
            json!({"id":"i1","name":"BD 709 / 블랙 / 타공 상판",
                "productId":"p1","variantId":"v1",
                "quantity":4,"quantityConfirmed":true,
                "unitPrice":59000}),"client-1".into(),Some(59000)
        )
    }
    #[test]
    fn actual_item_detail_keeps_rects_during_error_and_busy_changes(){
        use crate::ui_test_support::{test_context,assert_popup_transition_geometry,PopupGeometryGateway};
        let ctx=test_context();
        let work=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1400.0,900.0));
        let gateway:Arc<dyn BandoGateway>=Arc::new(PopupGeometryGateway{writable:true});
        let mut editor=dialog();
        editor.photo_requested=true;
        assert_popup_transition_geometry(&ctx,work,4,|ctx,phase|{
            editor.error=(phase==1).then(||"검증 오류".into());
            editor.show(ctx,Arc::clone(&gateway),work,phase==2);
        });
    }
    #[test]
    fn large_reception_item_photo_uses_full_width_and_safe_height(){
        assert_eq!(item_detail_photo_side(724.0,900.0),300.0);
        assert_eq!(item_detail_photo_side(724.0,500.0),260.0);
        assert!(item_detail_photo_side(480.0,480.0)>=200.0);
        assert_eq!(item_detail_photo_side(800.0,1100.0),300.0);
        assert!(ITEM_DETAIL_STACK_WIDTH>=ITEM_DETAIL_PHOTO_MIN
            +theme::MODAL_FORM_LABEL_WIDTH);
    }
    #[test]
    fn item_detail_modal_has_one_full_width_two_column_card_and_inline_info(){
        let source=include_str!("item_editor.rs");
        let modal=source.split("    pub fn show(").nth(1).unwrap()
            .split("        if delete_clicked{").next().unwrap();
        assert!(modal.contains("body.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::FillAvailable,content_width)"));
        assert!(modal.contains("line.add_space(ITEM_DETAIL_PANEL_GAP)"));
        assert!(modal.contains("Vec2::splat(photo_side)"));
        assert!(modal.contains("self.detail_information("));
        assert!(!modal.contains("Vec2::new(128.0,128.0)"));
        assert!(!modal.contains("ui.horizontal(|controls|"));
        let details=source.split("    fn detail_information(").nth(1).unwrap()
            .split("    fn poll_catalog_photo(").next().unwrap();
        assert!(details.contains("crate::popup::modal_labeled_row(ui"));
        for label in ["상품명","카테고리","가격"]{
            assert!(details.contains(label));
        }
        assert!(details.contains("상품 변경"));
        assert!(modal.contains("crate::popup::modal_flow_workspace("));
        assert!(modal.contains("workspace.scroll_region("));
        assert!(modal.contains("workspace.footer("));
        assert!(!modal.contains("components::modal_action_row_split("));
    }
    #[test]
    fn only_real_product_changes_with_confirmed_quantity_can_apply(){
        let mut editor=dialog();
        assert!(!editor.can_apply());
        editor.replacement=Some(json!({"id":"p2","name":"BD 709",
            "requiresOption":false,"variants":[{"id":"v2"}]}));
        assert!(editor.can_apply());
        editor.replacement=Some(json!({"id":"p1","name":"BD 709",
            "requiresOption":false,"variants":[{"id":"v1"}]}));
        assert!(!editor.can_apply());
        editor.replacement=Some(json!({"id":"p2","name":"BD 709",
            "requiresOption":true,"variants":[{"id":"v2"}]}));
        assert!(!editor.can_apply());
        editor.replacement=Some(json!({"id":"p2","name":"BD 709",
            "requiresOption":false,"variants":[{"id":"v2"},{"id":"v3"}]}));
        assert!(!editor.can_apply());
    }
    #[test]
    fn shared_click_intent_prioritizes_explicit_assignment_and_disabled_state(){
        assert_eq!(product_click_intent(true,true,false),ProductClickIntent::OpenDetail);
        assert_eq!(product_click_intent(true,true,true),ProductClickIntent::SelectAssignmentTarget);
        assert_eq!(product_click_intent(true,false,true),ProductClickIntent::None);
        assert_eq!(product_click_intent(false,true,false),ProductClickIntent::None);
    }
    #[test]
    fn manual_draft_can_choose_independent_product_before_quantity_confirmation(){
        let mut editor=dialog();
        editor.initial["quantity"]=Value::Null;
        editor.initial["quantityConfirmed"]=json!(false);
        editor.replacement=Some(json!({"id":"p2","variants":[{"id":"v2"}]}));
        assert!(!editor.can_apply());
        editor=editor.for_manual_draft();
        assert!(editor.can_apply());
        editor.replacement=Some(json!({"id":"p2","requiresOption":true,"variants":[{"id":"v2"}]}));
        assert!(!editor.can_apply());
    }
    #[test]
    fn unresolved_manual_product_enters_search_without_detail_and_escape_closes(){
        let mut editor=dialog().for_manual_draft();
        assert!(!editor.direct_manual_search());
        assert_eq!(editor.top_window_id(),"reception-item-detail");
        editor.initial["variantId"]=Value::Null;
        assert!(editor.direct_manual_search());
        assert_eq!(editor.top_window_id(),"native-manual-product-search");
        assert!(editor.escape(true).is_none());
        assert!(matches!(editor.escape(false),Some(Event::Closed)));
        editor.initial["productId"]=Value::Null;
        assert!(editor.direct_manual_search());
        editor.open_delete_only();
        assert_eq!(editor.top_window_id(),"reception-item-delete-confirm");
    }
    #[test]
    fn dialog_target_is_scoped_to_order_source_recipient_and_item(){
        let editor=dialog();
        assert!(editor.is_same_target("order-1","원문","r1","i1"));
        assert!(!editor.is_same_target("order-2","원문","r1","i1"));
        assert!(!editor.is_same_target("order-1","다른 원문","r1","i1"));
        assert!(!editor.is_same_target("order-1","원문","r2","i1"));
        assert!(!editor.is_same_target("order-1","원문","r1","i2"));
    }
}
