//! Native filtered catalog results: no UI business filtering.
//! Calls the same Rust catalog query used by the React product chooser.

use eframe::egui::{self,Context};
use serde_json::{json,Value};
use std::{sync::{mpsc::{self,Receiver},Arc},thread};
use crate::{gateway::{BandoGateway,ReadOperation},theme::{self,ButtonRole},typography,components};

fn field<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
struct Results{catalog:Value,products:Vec<Value>}
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
    receiver:Option<Receiver<Result<Results,String>>>,
    error:Option<String>,
}
impl ProductSearch{
    pub fn new(recipient_id:String,item_id:String,client_id:String,gateway:Arc<dyn BandoGateway>,ctx:Context)->Self{
        let mut state=Self{recipient_id,item_id,query:String::new(),major:String::new(),
            brand:String::new(),client_id,catalog:None,products:Vec::new(),receiver:None,error:None};
        state.reload(gateway,ctx);
        state
    }
    fn reload(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        self.error=None;
        self.products.clear();
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
                Ok(Results{catalog:state,products})
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
    pub fn show(&mut self,ctx:&Context,gateway:Arc<dyn BandoGateway>)->Option<Event>{
        self.poll();
        let mut open=true;
        let mut chosen=None;
        let mut changed=false;
        egui::Window::new("상품 검색")
            .id(egui::Id::new("native-manual-product-search"))
            .collapsible(false).resizable(true)
            .default_width(theme::MANUAL_SEARCH_DIALOG_WIDTH)
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
                    ui.horizontal(|ui|{
                        egui::ComboBox::from_id_salt("native-product-category")
                            .selected_text(categories.iter().find(|c|field(c,"id")==self.major)
                                .map(|v|field(v,"name")).unwrap_or("전체 분류"))
                            .show_ui(ui,|ui|{
                                if ui.selectable_value(&mut self.major,String::new(),"전체 분류").changed(){changed=true;}
                                for row in &categories{
                                    let id=field(row,"id");
                                    if !id.is_empty() && ui.selectable_value(&mut self.major,id.to_owned(),
                                        field(row,"name")).changed(){changed=true;}
                                }
                            });
                        egui::ComboBox::from_id_salt("native-product-brand")
                            .selected_text(brands.iter().find(|b|field(b,"id")==self.brand)
                                .map(|v|field(v,"name")).unwrap_or("전체 브랜드"))
                            .show_ui(ui,|ui|{
                                if ui.selectable_value(&mut self.brand,String::new(),"전체 브랜드").changed(){changed=true;}
                                for row in &brands{
                                    let id=field(row,"id");
                                    if !id.is_empty() && ui.selectable_value(&mut self.brand,id.to_owned(),
                                        field(row,"name")).changed(){changed=true;}
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
                                if theme::action(ui,name,ButtonRole::Standard,true).clicked(){
                                    chosen=Some(row.clone());
                                }
                                let detail=field(row,"subtitle");
                                let detail=if detail.is_empty(){field(row,"specification")}else{detail};
                                if !detail.is_empty(){ui.label(typography::caption(detail));}
                                ui.separator();
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
