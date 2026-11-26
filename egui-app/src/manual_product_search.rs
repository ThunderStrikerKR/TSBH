//! Native filtered catalog results: no UI business filtering.
//! Calls the same Rust catalog query used by the React product chooser.

use eframe::egui::{self,Context};
use serde_json::Value;
use std::{sync::{mpsc::{self,Receiver},Arc},thread};
use crate::{gateway::BandoGateway,management_queries::{self,ManualProductRows},theme::{self,ButtonRole},typography,components};

fn field<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
/// Mirror React's categoryFilterLevels presentation. Product inclusion,
/// brand aliases and listing order are still entirely decided by Rust.
fn category_levels(categories:&[Value],selected_id:&str)->Vec<(usize,String,String,Vec<Value>)> {
    let mut ancestors=Vec::<String>::new();
    let mut cursor=selected_id.to_owned();
    let mut visited=std::collections::HashSet::new();
    while !cursor.is_empty() && visited.insert(cursor.clone()) {
        let Some(row)=categories.iter().find(|row|field(row,"id")==cursor)
            else{break};
        ancestors.push(cursor.clone());
        cursor=field(row,"parentId").to_owned();
    }
    ancestors.reverse();
    let mut levels=Vec::new();
    for depth in 0..=ancestors.len() {
        let parent=if depth==0{String::new()}else{ancestors[depth-1].clone()};
        let children=categories.iter().filter(|category|{
            field(category,"parentId")==parent &&
            category.get("active")!=Some(&Value::Bool(false))
        }).cloned().collect::<Vec<_>>();
        if children.is_empty() && depth>0{break;}
        let value=ancestors.get(depth).cloned().unwrap_or_default();
        levels.push((depth,parent,value,children));
    }
    levels
}

pub enum Event{Cancelled,Chosen(Value)}
pub struct ProductSearch{
    query:String,
    major:String,
    brand:String,
    client_id:String,
    catalog:Option<Value>,
    products:Vec<Value>,
    detail_ids:Vec<String>,
    receiver:Option<Receiver<Result<ManualProductRows,String>>>,
    error:Option<String>,
}
impl ProductSearch{
    /// Reuse React's initial model-based query in the item-detail dialog.
    /// Rust still decides product inclusion and variant eligibility.
    pub fn with_query(
        client_id:String,query:String,
        gateway:Arc<dyn BandoGateway>,ctx:Context
    )->Self{
        let mut state=Self{query,major:String::new(),
            brand:String::new(),client_id,catalog:None,products:Vec::new(),
            detail_ids:Vec::new(),receiver:None,error:None};
        state.reload(gateway,ctx);
        state
    }
    fn reload(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        self.error=None;
        self.products.clear();
        self.detail_ids.clear();
        let query=self.query.clone();
        let brand=self.brand.clone();
        let major=self.major.clone();
        let client=self.client_id.clone();
        let catalog=self.catalog.clone();
        let (tx,rx)=mpsc::channel();
        self.receiver=Some(rx);
        thread::spawn(move||{
            let result=management_queries::query_manual_products(
                gateway.as_ref(),catalog,&query,&client,&major,&brand,
            );
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self){
        if let Some(receiver)=self.receiver.as_ref(){
            match receiver.try_recv(){
                Ok(Ok(result))=>{
                    self.catalog=Some(result.catalog);
                    self.products=result.products;
                    self.detail_ids=result.detail_ids;
                    self.receiver=None;
                }
                Ok(Err(error))=>{self.receiver=None;self.error=Some(error);}
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.receiver=None;self.error=Some("상품 검색 작업이 중단되었습니다.".into());
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
    }
    pub fn show(&mut self,ctx:&Context,gateway:Arc<dyn BandoGateway>,
        work_area:egui::Rect)->Option<Event>{
        self.poll();
        let mut open=true;
        let mut chosen=None;
        let mut changed=false;
        crate::popup::nested_modal_window(ctx,"상품 검색","native-manual-product-search",work_area)
            
            .default_width(theme::MANUAL_SEARCH_DIALOG_WIDTH
                .min((work_area.width()-theme::MANUAL_DIALOG_SIDE_GAP).max(0.0)))
            .constrain_to(work_area)
            .show(ctx,|ui|{

                crate::popup::modal_flow_workspace(ui,|workspace|{
                    if workspace.header("상품 검색","닫기",true){
                        open=false;
                    }
                    workspace.body_region(|ui|{
                        let width=ui.available_width();
                        if crate::input::management_search_field(
                            ui,&mut self.query,width,
                            "브랜드 / 상품명 / 모델 / 별칭 검색"
                        ).changed(){changed=true;}
                        if let Some(catalog)=&self.catalog{
                            let brands=catalog.get("brands").and_then(Value::as_array)
                                .cloned().unwrap_or_default();
                            let categories=catalog.get("categories").and_then(Value::as_array)
                                .cloned().unwrap_or_default();
                            let levels=category_levels(&categories,&self.major);
                            ui.horizontal_wrapped(|ui|{
                                for (depth,parent,selected,children) in levels {
                                    let label=children.iter().find(|c|field(c,"id")==selected)
                                        .map(|c|field(c,"name"))
                                        .unwrap_or(if depth==0{"전체 카테고리"}else{"현재 분류 전체"});
                                    let width=theme::CATEGORY_PATH_PICKER_WIDTH;
                                    let text_width=width-theme::CATEGORY_PATH_PICKER_TEXT_PADDING;
                                    let visible=typography::fit_category_path(
                                        ui,label,text_width,typography::CategoryPathFont::Body
                                    );
                                    let mut value=selected.clone();
                                    crate::dropdown::management_dropdown_full_label(
                                        ui,&visible,label,width,true,|choices|{
                                            crate::dropdown::management_dropdown_option(
                                                choices,&mut value,String::new(),
                                                if depth==0{"전체 카테고리"}else{"현재 분류 전체"}
                                            );
                                            for row in &children {
                                                let id=field(row,"id");
                                                let full=field(row,"name");
                                                let visible=typography::fit_category_path(
                                                    choices,full,text_width,
                                                    typography::CategoryPathFont::Body
                                                );
                                                if !id.is_empty(){
                                                    let response=crate::dropdown::management_dropdown_option(
                                                        choices,&mut value,id.to_owned(),&visible
                                                    );
                                                    crate::ui_primitives::shortened_text_tooltip(response,&visible,full);
                                                }
                                            }
                                        });
                                    if value!=selected {
                                        self.major=if value.is_empty(){parent}else{value};
                                        changed=true;
                                    }
                                }
                                if !self.major.is_empty() &&
                                    crate::button::action(ui,"카테고리 초기화",ButtonRole::Quiet,true).clicked() {
                                    self.major.clear();
                                    changed=true;
                                }
                                let previous_brand=self.brand.clone();
                                crate::dropdown::management_dropdown(
                                    ui,brands.iter().find(|b|field(b,"id")==self.brand)
                                        .map(|v|field(v,"name")).unwrap_or("전체 브랜드"),
                                    theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH,true,|choices|{
                                        crate::dropdown::management_dropdown_option(
                                            choices,&mut self.brand,String::new(),"전체 브랜드");
                                        for row in &brands {
                                            let id=field(row,"id");
                                            if !id.is_empty(){
                                                crate::dropdown::management_dropdown_option(
                                                    choices,&mut self.brand,id.to_owned(),
                                                    field(row,"name"));
                                            }
                                        }
                                    });
                                if self.brand!=previous_brand {changed=true;}
                            });
                        }
                        // Always allocate one result viewport. The search state
                        // changes inside it, never shrinking or jumping the modal.
                        let results_height=crate::popup::modal_list_max_height(
                            ui,work_area.height(),theme::MANUAL_SEARCH_RESULTS_HEIGHT,false
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(),results_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |pane|{
                                let results_rect=egui::Rect::from_min_size(
                                    pane.cursor().min,egui::vec2(pane.available_width(),results_height));
                                crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                                    .id_salt("manual-product-search-results")
                                    .max_height(results_height)
                                    .show(pane,|ui|{
                                        if self.error.is_some() {
                                            // Error uses the reserved outer result viewport below.
                                        } else if self.receiver.is_some() {
                                            // Pending status is painted in the reserved result viewport.
                                        } else {
                                    for row in &self.products{
                                        let name=field(row,"name");
                                        let name=if name.is_empty(){field(row,"code")}else{name};
                                        let detail=field(row,"subtitle");
                                        let detail=if detail.is_empty(){field(row,"specification")}else{detail};
                                        ui.horizontal(|ui|{
                                            ui.vertical(|info|{
                                                info.label(typography::body(name));
                                                if !detail.is_empty() {
                                                    info.label(typography::caption(detail));
                                                }
                                            });
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |actions|{
                                                    if crate::button::action(actions,"선택",ButtonRole::Standard,true)
                                                        .clicked() {
                                                        chosen=Some(row.clone());
                                                    }
                                                    if self.detail_ids.iter().any(|id|id==field(row,"id")) {
                                                        let url=row.get("variants")
                                                            .and_then(Value::as_array)
                                                            .and_then(|variants|variants.first())
                                                            .and_then(|variant|variant.get("url"))
                                                            .and_then(Value::as_str);
                                                        if let Some(url)=url {
                                                            if crate::button::action(actions,"상세보기 ↗",
                                                                ButtonRole::Quiet,true).clicked() {
                                                                ctx.open_url(egui::OpenUrl::new_tab(
                                                                    url.to_owned()
                                                                ));
                                                            }
                                                        }
                                                    }
                                                }
                                            );
                                        });
                                        crate::ui_primitives::soft_divider(ui);
                                    }

                                        }
                                    });
                                if let Some(error)=self.error.as_deref(){
                                    crate::status_notice::body_error_overlay(pane,results_rect,"reception.product",error);
                                }else{
                                    let caption=if self.receiver.is_some(){Some("상품 검색 중")}
                                        else if self.products.is_empty(){Some("검색 결과가 없습니다.")}
                                        else{None};
                                    crate::status_notice::management_body_state_overlay(pane,results_rect,caption);
                                }
                            },
                        );
                    });
                });
            });
        crate::popup::modal_foreground_for_window(ctx,"native-manual-product-search");
        if changed{self.reload(gateway,ctx.clone());}
        if let Some(row)=chosen{return Some(Event::Chosen(row));}
        if !open{return Some(Event::Cancelled);}
        None
    }
}
