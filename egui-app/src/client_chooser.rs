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
    focus_search: bool,
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
            focus_search:true,
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
        // A search workflow needs a real results viewport. Keep the dialog
        // centered and fixed like the OCR master, and shrink only when the
        // available Kakao-safe work area is smaller than the ideal size.
        let gutter=2.0*theme::MODAL_VIEWPORT_GUTTER;
        let dialog_size=egui::vec2(
            theme::CLIENT_CHOOSER_DIALOG_WIDTH
                .min((work_area.width()-gutter).max(1.0)),
            theme::CLIENT_CHOOSER_DIALOG_HEIGHT
                .min((work_area.height()-gutter).max(1.0))
        );
        components::modal_window("거래처 변경","egui-reception-client-chooser",work_area)
            .fixed_size(dialog_size)
            .show(ctx,|ui| {
                if components::modal_header_chrome(ui,"거래처 변경","닫기",!busy){
                    is_open=false;
                }
                ui.add_space(theme::MODAL_MASTER_WORKSPACE_GAP);
                // One OCR-master work surface, with search and results
                // separated by headings rather than two stacked card edges.
                components::modal_master_body(ui,|workspace|{
                components::modal_master_body_section(
                    workspace,"거래처 검색",|search_panel|{
                // Search is the first and largest interactive control.
                // Reuse the 36pt field geometry of every management input.
                let response=search_panel.add_enabled_ui(!busy,|field|{
                    let width=field.available_width();
                    components::flexible_text_field(
                        field,&mut self.query,width,
                        "상호 / 별칭 / 주소 / 전화번호 검색"
                    )
                }).inner;
                if self.focus_search {
                    response.request_focus();
                    self.focus_search=false;
                }
                if response.changed()&&self.loaded {
                    self.search(Arc::clone(&gateway),ctx.clone());
                }
                let currently=self.clients.iter()
                    .find(|c|Some(text(c,"id"))==current_id)
                    .map(|c|text(c,"name")).unwrap_or("미선택");
                let current_label=format!("현재 거래처: {currently}");
                search_panel.add(
                    egui::Label::new(typography::caption(&current_label)).truncate()
                ).on_hover_text(current_label);
                });
                workspace.add_space(theme::SPACE_16);
                components::modal_master_body_section(
                    workspace,"검색 결과",|results_panel|{
                if busy {
                    results_panel.label(typography::caption("거래처 적용 중…"));
                }
                // Native selection errors must remain visible in the chooser.
                // Otherwise a failed Rust repricing looks like a dead button.
                if let Some(error)=self.error.as_deref() {
                    results_panel.label(egui::RichText::new(error).color(theme::DANGER));
                }
                if !self.loaded||self.searching {
                    results_panel.label(typography::caption("검색 중"));
                } else {
                    let ids:Vec<&str>=if self.query.trim().is_empty() {
                        self.clients.iter().map(|c|text(c,"id")).collect()
                    } else {
                        self.results.iter().map(String::as_str).collect()
                    };
                    results_panel.label(typography::caption(&format!("{}개 거래처",ids.len())));
                    // Use the actual space left in the fixed dialog. The
                    // former content-driven 390pt cap let the whole popup
                    // collapse until only three result rows were visible.
                    let results_height=(results_panel.available_height()
                        -theme::SPACE_8)
                        .max(1.0)
                        .min(theme::CLIENT_CHOOSER_RESULTS_MAX_HEIGHT);
                    egui::ScrollArea::vertical()
                        .id_salt("egui-all-clients-scroll")
                        .max_height(results_height)
                        .auto_shrink([false,false])
                        .show(results_panel,|ui|{
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
                                    // Give the business name, address and
                                    // phone their own readable lines. The
                                    // selection stays pinned to the right.
                                    ui.horizontal(|line|{
                                        line.spacing_mut().item_spacing.x=theme::SPACE_8;
                                        let info_width=(line.available_width()
                                            -theme::SPACE_8-88.0).max(1.0);
                                        line.allocate_ui_with_layout(
                                            egui::vec2(info_width,
                                                theme::CLIENT_CHOOSER_ROW_HEIGHT),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |details|{
                                                details.spacing_mut().item_spacing.y=
                                                    theme::SPACE_4;
                                                details.add(egui::Label::new(
                                                    typography::section_heading(name)
                                                ).truncate()).on_hover_text(name);
                                                let visible_address=if address.is_empty(){
                                                    "주소 미등록"
                                                }else{address};
                                                details.add(egui::Label::new(
                                                    typography::body(visible_address)
                                                ).truncate()).on_hover_text(visible_address);
                                                details.add(egui::Label::new(
                                                    typography::caption(
                                                        if phone.is_empty(){"전화 미등록"}
                                                        else{phone}
                                                    )
                                                ).truncate());
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
                });
            });
        if let Some(id)=chosen {
            if self.contains(&id) {return Some(Choice::Selected(id));}
        }
        if !is_open && !busy {return Some(Choice::Closed);}
        None
    }
}
