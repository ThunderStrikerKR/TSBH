//! Full customer chooser for one unconfirmed reception.
//! Native Rust owns match/order decisions; egui owns only the transient
//! list, text entry and selection. Neither operational DB nor order history
//! is modified by searching.

use std::{sync::{Arc,mpsc::{self,Receiver}}, thread};
use eframe::egui::{self, Context};
use serde_json::{json,Value};
use crate::{gateway::{BandoGateway,ReadOperation}, theme::{self,ButtonRole}, typography, components};

pub enum Choice {
    Selected(String),
    Closed,
}

type Directory = Result<Vec<Value>,String>;
type Search = Result<Vec<String>,String>;

pub struct ClientChooser {
    pub order_id: String,
    query: String,
    clients: Vec<Value>,
    results: Vec<String>,
    loaded: bool,
    loading: Receiver<Directory>,
    search_result: Option<Receiver<(u64,Search)>>,
    generation: u64,
    searching: bool,
    error: Option<String>,
    selecting: Option<String>,
}

fn text<'a>(row:&'a Value,key:&str)->&'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn active(row:&Value)->bool {
    row.get("deletedAt").is_none_or(|deleted|
        deleted.is_null() || deleted.as_str()==Some("")
    ) && row.get("active")!=Some(&Value::Bool(false))
}

impl ClientChooser {
    pub fn new(order_id: String, gateway: Arc<dyn BandoGateway>, ctx: Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = gateway.load_catalog().and_then(|state| {
                let clients = state.get("clients").and_then(Value::as_array)
                    .ok_or("거래처 목록을 가져오지 못했습니다.")?;
                // Retain ONLY the customer index. Product and other catalog
                // sections are discarded as soon as the loading job ends.
                Ok(clients.iter().filter(|c|active(c)).cloned().collect())
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        Self {
            order_id, query:String::new(),
            clients:Vec::new(),results:Vec::new(),loaded:false,
            loading:receiver,search_result:None,generation:0,
            searching:false,error:None,selecting:None,
        }
    }

    pub fn contains(&self, id:&str)->bool {
        self.loaded && self.clients.iter().any(|c| text(c,"id")==id && active(c))
    }
    /// Only a successful native result closes the chooser. Keep selection
    /// and search state intact across Rust errors for an immediate retry.
    pub fn begin_selection(&mut self,id:&str) {
        self.selecting=Some(id.to_owned());
        self.error=None;
    }
    pub fn selection_failed(&mut self,message:String) {
        self.selecting=None;
        self.error=Some(message);
    }
    pub fn is_selecting(&self)->bool { self.selecting.is_some() }

    fn search(&mut self, gateway:Arc<dyn BandoGateway>, ctx:Context) {
        self.generation=self.generation.wrapping_add(1);
        self.search_result=None;
        self.error=None;
        self.results.clear();
        if self.query.trim().is_empty() {
            self.searching=false;
            return;
        }
        self.searching=true;
        let generation=self.generation;
        let query=self.query.clone();
        let clients=self.clients.clone();
        let (sender,receiver)=mpsc::channel();
        self.search_result=Some(receiver);
        thread::spawn(move || {
            let result=gateway.execute(ReadOperation::ClientCandidates, json!({
                "state": {"clients":clients}, "query":query, "raw":""
            })).and_then(|value| {
                let ids=value.get("ids").and_then(Value::as_array)
                    .ok_or("거래처 검색 결과가 잘못되었습니다.")?;
                ids.iter().map(|entry|entry.as_str().map(str::to_owned)
                    .ok_or_else(||"거래처 검색 ID가 잘못되었습니다.".to_owned()))
                    .collect()
            });
            let _=sender.send((generation,result));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, gateway:Arc<dyn BandoGateway>, ctx:&Context) {
        if !self.loaded {
            match self.loading.try_recv() {
                Ok(Ok(clients))=>{
                    self.clients=clients;
                    self.loaded=true;
                    if !self.query.trim().is_empty() {self.search(gateway,ctx.clone());}
                }
                Ok(Err(e))=>{
                    self.loaded=true;
                    self.error=Some(e);
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.loaded=true;
                    self.error=Some("거래처 명단 조회가 중단되었습니다.".into());
                }
                _=>{}
            }
        }
        if let Some(rx)=self.search_result.as_ref() {
            match rx.try_recv() {
                Ok((generation, result))=>{
                    if generation==self.generation {
                        self.searching=false;
                        match result {
                            Ok(ids)=>self.results=ids,
                            Err(e)=>self.error=Some(e),
                        }
                    }
                    self.search_result=None;
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.search_result=None;
                    self.searching=false;
                    self.error=Some("거래처 검색이 중단되었습니다.".into());
                }
                _=>{}
            }
        }
    }

    pub fn show(&mut self, ctx:&Context, gateway:Arc<dyn BandoGateway>,
                current_id:Option<&str>, work_area:egui::Rect) -> Option<Choice> {
        self.poll(Arc::clone(&gateway),ctx);
        let mut is_open=true;
        let mut chosen:Option<String>=None;
        let busy=self.selecting.is_some();
        components::modal_window("거래처 변경","egui-reception-client-chooser",work_area)
            
            .default_width(460.0_f32.min(work_area.width()))
            .min_width(340.0_f32.min(work_area.width()))
            .show(ctx,|ui| {
                if components::modal_header(ui,"거래처 변경","닫기",!busy){
                    is_open=false;
                }
                let currently=self.clients.iter()
                    .find(|c|Some(text(c,"id"))==current_id)
                    .map(|c|text(c,"name")).unwrap_or("미선택");
                ui.label(typography::body(&format!("현재 거래처: {currently}")));
                let response=ui.add_enabled(!busy,
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("상호 / 별칭 / 주소 / 전화번호 검색")
                        .desired_width(ui.available_width())
                );
                if response.changed()&&self.loaded {
                    self.search(Arc::clone(&gateway),ctx.clone());
                }
                if busy {
                    ui.label(typography::caption("거래처 적용 중…"));
                }
                // Native selection errors must remain visible in the chooser.
                // Otherwise a failed Rust repricing looks like a dead button.
                if let Some(error)=self.error.as_deref() {
                    ui.label(egui::RichText::new(error).color(theme::DANGER));
                }
                if !self.loaded||self.searching {
                    ui.label(typography::caption("검색 중"));
                } else {
                    let ids:Vec<&str>=if self.query.trim().is_empty() {
                        self.clients.iter().map(|c|text(c,"id")).collect()
                    } else {
                        self.results.iter().map(String::as_str).collect()
                    };
                    ui.label(typography::caption(&format!("{}개 거래처",ids.len())));
                    egui::ScrollArea::vertical()
                        .id_salt("egui-all-clients-scroll")
                        .max_height((work_area.height()-180.0).max(60.0).min(390.0))
                        .show(ui,|ui|{
                            for id in ids {
                                if let Some(row)=self.clients.iter().find(|c|text(c,"id")==id) {
                                    let name=text(row,"name");
                                    let address=text(row,"address");
                                    let phone=if !text(row,"phone").is_empty() {
                                        text(row,"phone")
                                    } else { text(row,"mobile") };
                                    let selected=current_id==Some(id);
                                    // A transparent text-only Quiet action
                                    // looked like static data in the old chooser.
                                    // Keep details as labels and provide a
                                    // visible, standard action on EVERY row.
                                    let info_width=(ui.available_width()-110.0).max(150.0);
                                    ui.horizontal(|line|{
                                        line.allocate_ui_with_layout(
                                            egui::vec2(info_width,
                                                theme::MANAGEMENT_CHOOSER_ROW_HEIGHT),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |details|{
                                                details.spacing_mut().item_spacing.y=
                                                    theme::SPACE_4;
                                                details.add(egui::Label::new(
                                                    typography::body(name)).truncate());
                                                let contact=format!("{} / {}",
                                                    if address.is_empty(){"주소 미등록"}
                                                    else{address},
                                                    if phone.is_empty(){"전화 미등록"}
                                                    else{phone});
                                                details.add(egui::Label::new(
                                                    typography::caption(&contact)).truncate())
                                                    .on_hover_text(contact);
                                            },
                                        );
                                        line.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |actions|{
                                                let label=if selected {"선택됨"}else{"선택"};
                                                if theme::action_with_min_width(
                                                    actions,label,ButtonRole::Standard,
                                                    !busy && !selected,76.0
                                                ).clicked(){
                                                    chosen=Some(id.to_owned());
                                                }
                                            },
                                        );
                                    });
                                    ui.separator();
                                }
                            }
                            if self.clients.is_empty() {
                                ui.label(typography::caption("등록된 거래처가 없습니다."));
                            } else if self.query.trim().len()>0 && self.results.is_empty() &&
                                    !self.searching {
                                ui.label(typography::caption("검색 결과가 없습니다."));
                            }
                        });
                }
            });
        if let Some(id)=chosen {
            if self.contains(&id) {return Some(Choice::Selected(id));}
        }
        if !is_open && !busy {return Some(Choice::Closed);}
        None
    }
}
