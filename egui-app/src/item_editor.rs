//! React OrderItemEditor parity for a selected reception product row.
//! The dialog only stages employee intent. Catalog search is Rust-backed;
//! the native manual reducer validates the actual item and recalculates price.

use std::sync::Arc;
use eframe::egui::{self,Context,Vec2};
use serde_json::Value;
use crate::{
    components,
    gateway::BandoGateway,
    manual_product_search::{Event as SearchEvent, ProductSearch},
    theme::{self, ButtonRole},
    typography,
};

fn field<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
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
    replacement:Option<Value>,
    search:Option<ProductSearch>,
    confirm_delete:bool,
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
            initial:item,initial_price:price,replacement:None,
            search:None,confirm_delete:false,confirm_discard:false,
            error:None,
        }
    }
    pub fn is_same_target(&self,order_id:&str,raw:&str,rid:&str,iid:&str)->bool{
        self.order_id==order_id && self.source_raw==raw &&
            self.recipient_id==rid && self.item_id==iid
    }
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
        valid_variant && quantity && product_changed
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
        self.search=Some(ProductSearch::with_query(
            self.recipient_id.clone(),self.item_id.clone(),
            self.client_id.clone(),query,gateway,ctx.clone()
        ));
    }
    pub fn show(
        &mut self,ctx:&Context,gateway:Arc<dyn BandoGateway>,
        work_area:egui::Rect,busy:bool
    )->Option<Event>{
        let mut result=None;
        let selected=self.selected().clone();
        let name=field(&selected,"name");
        let name=if name.is_empty(){field(&selected,"code")}else{name};
        let name=if name.is_empty(){"상품 확인 필요"}else{name};
        let category=field(&selected,"category");
        let category=if category.is_empty(){
            if edited || !field(&self.initial,"productId").is_empty(){
                "등록 상품"
            }else{"등록 상품 미연결"}
        }else{category};
        let edited=self.replacement.is_some();
        let price=if edited {
            "적용 후 Rust 가격 재계산".to_owned()
        }else{
            self.initial_price.map(components::display_won)
                .unwrap_or_else(||"가격 확인 필요".into())
        };
        let allow_apply=!busy && self.can_apply();
        let invalid_selection=edited && !self.can_apply();
        let mut delete_clicked=false;
        let mut apply_clicked=false;
        let needs_customer=self.initial["modelProposal"]["required"]==true;
        let safe_width=work_area.width()-theme::MANUAL_DIALOG_SIDE_GAP;
        components::modal_window(
            "상품 상세 수정","reception-item-detail",work_area
        )
            .default_width(740.0_f32.min(safe_width).max(0.0))
            .constrain_to(work_area)
            .show(ctx,|ui|{
                if components::modal_header(
                    ui,"상품 상세 수정","닫기",!busy && self.search.is_none()
                ){
                    result=self.close();
                }
                ui.add_space(theme::SPACE_8);
                theme::modal_nested_frame().show(ui,|body|{
                    body.horizontal(|line|{
                        let (photo,_) = line.allocate_exact_size(
                            Vec2::new(128.0,128.0),egui::Sense::hover()
                        );
                        line.painter().rect_filled(
                            photo,theme::CONTROL_RADIUS as f32,theme::INPUT
                        );
                        line.painter().text(
                            photo.center(),egui::Align2::CENTER_CENTER,
                            "상품 사진 없음",egui::FontId::proportional(14.0),
                            theme::SECONDARY
                        );
                        line.add_space(theme::SPACE_16);
                        line.vertical(|details|{
                            details.label(typography::caption("상품명"));
                            details.label(typography::body(name));
                            details.add_space(theme::SPACE_8);
                            details.label(typography::caption("카테고리"));
                            details.label(typography::body(category));
                            details.add_space(theme::SPACE_8);
                            details.label(typography::caption("가격"));
                            details.label(typography::body(&price));
                        });
                    });
                });
                ui.add_space(theme::SPACE_16);
                ui.horizontal(|controls|{
                    if theme::action(
                        controls,"상품 변경",ButtonRole::Standard,!busy
                    ).clicked(){
                        self.open_search(Arc::clone(&gateway),ctx);
                    }
                    if edited && theme::action(
                        controls,"선택 취소",ButtonRole::Quiet,!busy
                    ).clicked(){
                        self.replacement=None;
                        self.error=None;
                    }
                });
                if invalid_selection{
                    ui.label(typography::status(
                        "수량이 확정되고 판매 구성이 하나인 독립 상품만 적용할 수 있습니다.",
                        theme::DANGER
                    ));
                }
                if needs_customer{
                    ui.label(typography::caption(
                        "이 상품은 고객 확인이 필요한 추천 상품입니다."
                    ));
                }
                if let Some(error)=&self.error{
                    ui.label(typography::status(error,theme::DANGER));
                }
                if busy{ui.label(typography::caption("Rust 상품 변경 처리 중"));}
                ui.add_space(theme::SPACE_16);
                components::modal_action_row_split(
                    ui,
                    |left|{
                        if theme::action(left,"삭제",ButtonRole::Danger,!busy)
                            .clicked(){
                            delete_clicked=true;
                        }
                    },
                    |right|{
                        if theme::action(
                            right,"적용",ButtonRole::Primary,allow_apply
                        ).clicked(){
                            apply_clicked=true;
                        }
                    }
                );
            });
        if delete_clicked{self.confirm_delete=true;}
        if apply_clicked{result=self.replacement.clone().map(Event::Apply);}
        if self.search.is_some() || self.confirm_delete || self.confirm_discard{
            components::modal_backdrop(ctx,"reception-item-nested-backdrop",true);
        }
        if let Some(event)=self.search.as_mut().and_then(|search|
            search.show(ctx,Arc::clone(&gateway),work_area)
        ){
            match event{
                SearchEvent::Cancelled=>{},
                SearchEvent::Chosen(product)=>{
                    self.replacement=Some(product);
                    self.error=None;
                }
            }
            self.search=None;
        }
        if self.confirm_delete{
            components::decision_window(
                "품목 삭제 확인","reception-item-delete-confirm",work_area
            ).show(ctx,|ui|{
                if components::modal_header(ui,"품목 삭제 확인","돌아가기",!busy){
                    self.confirm_delete=false;
                }
                ui.label("선택한 품목을 발주서에서 삭제할까요?");
                components::modal_action_row(ui,|actions|{
                    if theme::action(actions,"삭제",ButtonRole::Danger,!busy)
                        .clicked(){
                        self.confirm_delete=false;
                        result=Some(Event::Remove);
                    }
                });
            });
        }
        if self.confirm_discard{
            components::decision_window(
                "변경사항 확인","reception-item-discard-confirm",work_area
            ).show(ctx,|ui|{
                if components::modal_header(ui,"변경사항 확인","돌아가기",true){
                    self.confirm_discard=false;
                }
                ui.label("선택한 상품 변경사항을 버릴까요?");
                components::modal_action_row(ui,|actions|{
                    if theme::action(actions,"수정 내용 버리기",ButtonRole::Danger,true)
                        .clicked(){
                        self.confirm_discard=false;
                        result=Some(Event::Closed);
                    }
                });
            });
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
    fn dialog_target_is_scoped_to_order_source_recipient_and_item(){
        let editor=dialog();
        assert!(editor.is_same_target("order-1","원문","r1","i1"));
        assert!(!editor.is_same_target("order-2","원문","r1","i1"));
        assert!(!editor.is_same_target("order-1","다른 원문","r1","i1"));
        assert!(!editor.is_same_target("order-1","원문","r2","i1"));
        assert!(!editor.is_same_target("order-1","원문","r1","i2"));
    }
}
