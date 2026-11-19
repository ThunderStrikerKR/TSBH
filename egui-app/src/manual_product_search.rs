//! Native filtered catalog results: no UI business filtering.
//! Calls the same Rust catalog query used by the React product chooser.

use eframe::egui::{self,Context};
use serde_json::{json,Value};
use std::{sync::{mpsc::{self,Receiver},Arc},thread};
use crate::{gateway::{BandoGateway,ReadOperation},theme::{self,ButtonRole},typography,components};

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

struct Results{catalog:Value,products:Vec<Value>,detail_ids:Vec<String>}
pub enum Event{Cancelled,Chosen(Value)}
pub struct ProductSearch{
    pub recipient_id:String,
    pub item_id:String,
    query:String,
    major:String,
    brand:String,
    client_id:String,
    catalog:Option<Value>,
    products:Vec<Value>,
    detail_ids:Vec<String>,
    receiver:Option<Receiver<Result<Results,String>>>,
    error:Option<String>,
}
impl ProductSearch{
    pub fn new(recipient_id:String,item_id:String,client_id:String,gateway:Arc<dyn BandoGateway>,ctx:Context)->Self{
        let mut state=Self{recipient_id,item_id,query:String::new(),major:String::new(),
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
            let result=(||->Result<Results,String>{
                let state=match catalog{Some(value)=>value,None=>gateway.load_catalog()?};
                let result=gateway.execute(ReadOperation::SearchProductsWithUrls,json!({
                    "state":&state,"query":query,"clientId":client,
                    "major":major,"brand":brand,"excludeDemo":true
                }))?;
                let ids=result.get("ids").and_then(Value::as_array)
                    .ok_or("Rust 상품 검색 결과 형식 오류")?;
                let all=state.get("products").and_then(Value::as_array)
                    .ok_or("상품 목록이 없습니다.")?;
                let mut products=Vec::new();
                for id in ids.iter().take(100){
                    let id=id.as_str().ok_or("Rust 상품 검색 ID 오류")?;
                    let row=all.iter().find(|p|field(p,"id")==id)
                        .ok_or("상품 ID와 목록이 일치하지 않습니다.")?;
                    products.push(row.clone());
                }
                let detail_ids=result.get("detailLinkIds")
                    .and_then(Value::as_array)
                    .into_iter().flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                Ok(Results{catalog:state,products,detail_ids})
            })();
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
        components::modal_window("상품 검색","native-manual-product-search")
            .resizable(true)
            .default_pos(egui::pos2(
                work_area.center().x-
                    (theme::MANUAL_SEARCH_DIALOG_WIDTH/2.0).min(work_area.width()/2.0),
                work_area.center().y-theme::MANUAL_SEARCH_RESULTS_HEIGHT/2.0
            ))
            .default_width(theme::MANUAL_SEARCH_DIALOG_WIDTH
                .min(work_area.width()-theme::MANUAL_DIALOG_SIDE_GAP))
            .constrain_to(work_area)
            .open(&mut open)
            .show(ctx,|ui|{
                let width=ui.available_width();
                if components::flexible_text_field(
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
                            egui::ComboBox::from_id_salt(("manual-product-category",depth))
                                .selected_text(label)
                                .show_ui(ui,|ui|{
                                    let reset=if depth==0{String::new()}else{parent.clone()};
                                    if ui.selectable_label(selected.is_empty(),
                                        if depth==0{"전체 카테고리"}else{"현재 분류 전체"})
                                        .clicked() {
                                        self.major=reset;
                                        changed=true;
                                    }
                                    for row in &children {
                                        let id=field(row,"id");
                                        if !id.is_empty() &&
                                            ui.selectable_label(selected==id,field(row,"name"))
                                                .clicked() {
                                            self.major=id.to_owned();
                                            changed=true;
                                        }
                                    }
                                });
                        }
                        if !self.major.is_empty() &&
                            theme::action(ui,"카테고리 초기화",ButtonRole::Quiet,true).clicked() {
                            self.major.clear();
                            changed=true;
                        }
                        egui::ComboBox::from_id_salt("native-product-brand")
                            .selected_text(brands.iter().find(|b|field(b,"id")==self.brand)
                                .map(|v|field(v,"name")).unwrap_or("전체 브랜드"))
                            .show_ui(ui,|ui|{
                                if ui.selectable_value(&mut self.brand,String::new(),
                                    "전체 브랜드").changed(){changed=true;}
                                for row in &brands {
                                    let id=field(row,"id");
                                    if !id.is_empty() && ui.selectable_value(
                                        &mut self.brand,id.to_owned(),field(row,"name")
                                    ).changed(){changed=true;}
                                }
                            });
                    });
                }
                if let Some(error)=&self.error{
                    ui.label(typography::status(error,theme::DANGER));
                }
                if self.receiver.is_some(){
                    ui.label(typography::caption("상품 검색 중"));
                }else{
                    egui::ScrollArea::vertical()
                        .id_salt("manual-product-search-results")
                        .max_height(theme::MANUAL_SEARCH_RESULTS_HEIGHT)
                        .show(ui,|ui|{
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
                                            if theme::action(actions,"선택",ButtonRole::Standard,true)
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
                                                    if theme::action(actions,"상세보기 ↗",
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
                                components::soft_divider(ui);
                            }
                            if self.products.is_empty()&&self.error.is_none(){
                                ui.label(typography::caption("검색 결과가 없습니다."));
                            }
                        });
                }
            });
        if changed{self.reload(gateway,ctx.clone());}
        if let Some(row)=chosen{return Some(Event::Chosen(row));}
        if !open{return Some(Event::Cancelled);}
        None
    }
}
