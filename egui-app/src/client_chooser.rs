//! Full customer chooser for one unconfirmed reception.
//! Native Rust owns match/order decisions; egui owns only the transient
//! list, text entry and selection. Neither operational DB nor order history
//! is modified by searching.

use std::{collections::HashMap,sync::{Arc,mpsc::{self,Receiver}}, thread};
use eframe::egui::{self, Context};
use serde_json::{json,Value};
use crate::{gateway::{BandoGateway,ReadOperation},
    theme, typography, components,
    floating_notice::{self,Tone}};

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
    focus_search: bool,
    sort: Option<(usize,bool)>,
}

fn text<'a>(row:&'a Value,key:&str)->&'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn active(row:&Value)->bool {
    row.get("deletedAt").is_none_or(|deleted|
        deleted.is_null() || deleted.as_str()==Some("")
    ) && row.get("active")!=Some(&Value::Bool(false))
}

/// The matcher still receives its indexed aliases/search tags. Enrich ONLY
/// displayed fields from the existing authoritative client master, never
/// replacing the search index or writing a customer record.
fn enrich_display_fields(index:&mut [Value],master:&Value){
    let by_id:HashMap<&str,&Value>=master.get("clients")
        .and_then(Value::as_array).into_iter().flatten()
        .filter_map(|client|{
            client.get("id").and_then(Value::as_str).map(|id|(id,client))
        }).collect();
    for client in index{
        let id=text(client,"id").to_owned();
        if let Some(full)=by_id.get(id.as_str()){
            for key in [
                "name","address","phone","mobile","tier","payment",
                "defaultApplyVat","shippingFeeExcluded","active","deletedAt"
            ]{
                if let Some(value)=full.get(key){client[key]=value.clone();}
            }
        }
    }
}
fn sort_value<'a>(client:&'a Value,column:usize)->&'a str{
    match column{
        0=>text(client,"name"),
        1=>text(client,"address"),
        2=>{
            let phone=text(client,"phone");
            if phone.trim().is_empty(){text(client,"mobile")}else{phone}
        },
        _=>components::client_tier_label(text(client,"tier"))
    }
}
/// Unsorted search results retain native Rust relevance order. Explicit
/// sorting is a presentation-only operation, not a matcher/pricing decision.
fn visible_clients<'a>(
    clients:&'a [Value],results:&[String],query:&str,
    searching:bool,sort:Option<(usize,bool)>
)->Vec<&'a Value>{
    if searching{return Vec::new();}
    let mut visible:Vec<&Value>=if query.trim().is_empty(){
        clients.iter().filter(|client|active(client)).collect()
    }else{
        let by_id:HashMap<&str,&Value>=clients.iter()
            .filter(|client|active(client))
            .map(|client|(text(client,"id"),client)).collect();
        results.iter().filter_map(|id|by_id.get(id.as_str()).copied())
            .collect()
    };
    if let Some((column,descending))=sort{
        visible.sort_by(|left,right|{
            let a=sort_value(left,column);
            let b=sort_value(right,column);
            let cmp=a.to_lowercase().cmp(&b.to_lowercase())
                .then_with(||text(left,"id").cmp(text(right,"id")));
            if descending{cmp.reverse()}else{cmp}
        });
    }
    visible
}

impl ClientChooser {
    pub fn new(order_id: String, gateway: Arc<dyn BandoGateway>, ctx: Context) -> Self {
        // New popup sessions must not inherit stale error deduplication from
        // the previously closed chooser.
        floating_notice::sync(
            &ctx,"reception.client_chooser.error",
            "reception.client_chooser",Tone::Error,None
        );
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = gateway.load_catalog().and_then(|state| {
                let clients = state.get("clients").and_then(Value::as_array)
                    .ok_or("거래처 목록을 가져오지 못했습니다.")?;
                // Retain ONLY the customer index. Product and other catalog
                // sections are discarded as soon as the loading job ends.
                let mut clients:Vec<Value>=clients.iter().filter(|c|active(c))
                    .cloned().collect();
                // The name-only matcher index intentionally omits pricing
                // tiers. Load read-only display metadata on the worker, not
                // every UI frame; retain matcher fields when unavailable.
                if let Ok(master)=gateway.load_client_master(){
                    enrich_display_fields(&mut clients,&master);
                    clients.retain(active);
                }
                Ok(clients)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        Self {
            order_id, query:String::new(),
            clients:Vec::new(),results:Vec::new(),loaded:false,
            loading:receiver,search_result:None,generation:0,
            searching:false,error:None,
            focus_search:true,sort:None,
        }
    }

    pub fn contains(&self, id:&str)->bool {
        self.loaded && self.clients.iter().any(|c| text(c,"id")==id && active(c))
    }
    // A verified row click emits Selected once; the owning reception closes
    // this modal immediately. Native calculation never owns dialog lifetime.

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
        // The 1200pt modal fits the 900pt shared client grid plus the popup
        // padding and scrollbar. Only a physically narrower Kakao-safe work
        // area may require horizontal overflow; never extend the modal beyond
        // the usable window.
        let gutter=2.0*theme::MODAL_VIEWPORT_GUTTER;
        let dialog_size=egui::vec2(
            theme::CLIENT_CHOOSER_DIALOG_WIDTH
                .min((work_area.width()-gutter).max(1.0)),
            theme::CLIENT_CHOOSER_DIALOG_HEIGHT
                .min((work_area.height()-gutter).max(1.0))
        );
        components::modal_window("거래처 찾기","egui-reception-client-chooser",work_area)
            .fixed_size(dialog_size)
            .show(ctx,|ui|{
                components::modal_flow_workspace(ui,|master|{
                    if master.header("거래처 찾기","닫기",true){is_open=false;}
                    master.framed_region(|workspace|{
                    components::modal_master_body_section(
                        workspace,"거래처 검색",|search_panel|{
                            let response=search_panel.add_enabled_ui(true,|field|{
                                let width=field.available_width();
                                components::management_search_field(
                                    field,&mut self.query,width,
                                    "거래처명 / 검색 태그 / 주소 / 전화번호 검색"
                                )
                            }).inner;
                            if self.focus_search{
                                response.request_focus();
                                self.focus_search=false;
                            }
                            if response.changed()&&self.loaded{
                                self.search(Arc::clone(&gateway),ctx.clone());
                            }
                        }
                    );
                    workspace.add_space(theme::SPACE_16);
                    components::modal_master_body_section(
                        workspace,"검색 결과",|results_panel|{
                            results_panel.spacing_mut().scroll=
                                theme::management_table_scroll_style();
                            results_panel.spacing_mut().item_spacing.y=0.0;
                            // Errors belong to the shared floating toast,
                            // not an inline row that shifts the pinned table
                            // header or steals space from the scroll viewport.
                            floating_notice::section(
                                ctx,"reception.client_chooser",
                                results_panel.available_rect_before_wrap()
                            );
                            floating_notice::sync(
                                ctx,"reception.client_chooser.error",
                                "reception.client_chooser",
                                Tone::Error,self.error.as_deref()
                            );
                             let clients=visible_clients(
                                &self.clients,&self.results,&self.query,
                                self.searching,self.sort
                            );
                            let loading=!self.loaded||self.searching;
                            let count_label=if loading{
                                "검색 중".to_owned()
                            }else{
                                format!("{}개 거래처",clients.len())
                            };
                            results_panel.label(typography::caption(&count_label));
                            let viewport_width=results_panel.available_width().max(1.0);
                            let gutter=results_panel.spacing().scroll.allocated_width();
                            let table_width=(viewport_width-gutter)
                                .max(theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH);
                            // Fixed 36pt header is independent from the 42pt
                            // scrolling rows, exactly as in Client Management.
                            let (header_rect,_)=results_panel.allocate_exact_size(
                                egui::vec2(
                                    viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT
                                ),egui::Sense::hover()
                            );
                            let header_content=egui::Rect::from_min_max(
                                header_rect.min,
                                egui::pos2(
                                    (header_rect.right()-gutter)
                                        .max(header_rect.left()+1.0),
                                    header_rect.bottom()
                                )
                            );
                            let height=(results_panel.available_height()-theme::SPACE_8)
                                .max(1.0)
                                .min(theme::CLIENT_CHOOSER_RESULTS_MAX_HEIGHT);
                            let scroll_track=components::management_table_scroll_track_rect(
                                results_panel,height
                            );
                            let mut first_row_left=None;
                            let output=components::virtual_fixed_rows(
                                egui::ScrollArea::both()
                                    .id_salt("egui-all-clients-scroll")
                                    .scroll_bar_rect(scroll_track)
                                    .scroll_bar_visibility(
                                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                                    )
                                    .max_height(height)
                                    .auto_shrink([false,false]),
                                results_panel,table_width,
                                theme::MANAGEMENT_CLIENT_ROW_HEIGHT,
                                clients.len().max(1),
                                |table,visible_rows|{
                                    table.set_min_width(table_width);
                                    table.spacing_mut().item_spacing.y=0.0;
                                    let first=visible_rows.start;
                                    let origin=table.max_rect().min;
                                    let clip=table.clip_rect();
                                    for index in visible_rows {
                                        let rect=egui::Rect::from_min_size(
                                            egui::pos2(origin.x,origin.y+
                                                (index-first) as f32*
                                                theme::MANAGEMENT_CLIENT_ROW_HEIGHT),
                                            egui::vec2(
                                                table_width,theme::MANAGEMENT_CLIENT_ROW_HEIGHT
                                            )
                                        );
                                        first_row_left.get_or_insert(rect.left());
                                        table.scope_builder(
                                            egui::UiBuilder::new()
                                                .id_salt(("client-chooser-row",index))
                                                .max_rect(rect)
                                                .layout(egui::Layout::top_down(egui::Align::Min)),
                                            |row_ui|{
                                                row_ui.set_clip_rect(clip);
                                                if let Some(client)=clients.get(index){
                                                    let client_id=text(client,"id");
                                                    let data=components::ClientDisplaySummary::from_client(client);
                                                    let phone=data.primary_phone();
                                                    let shown_phone=if phone.trim().is_empty(){
                                                        "연락처 미등록"
                                                    }else{phone};
                                                    let selected=current_id==Some(client_id);
                                                    let response=components::management_row_striped(
                                                        row_ui,client_id,[
                                                            (&data.name,""),
                                                            (&data.address,""),
                                                            (shown_phone,data.secondary_mobile()),
                                                            (components::client_tier_label(&data.tier),
                                                                &data.conditions),
                                                        ],Some(&data.tier),index
                                                    ).on_hover_text(if selected {
                                                        "현재 연결된 거래처"
                                                    }else{"클릭하여 거래처 선택"});
                                                    if selected {
                                                        row_ui.painter().with_clip_rect(clip)
                                                            .rect_stroke(
                                                                response.rect.shrink(1.0),0.0,
                                                                egui::Stroke::new(
                                                                    theme::SOFT_DIVIDER_WIDTH,
                                                                    theme::SELECTION
                                                                ),egui::StrokeKind::Inside
                                                            );
                                                    }
                                                    if !selected&&response.clicked(){
                                                        chosen=Some(client_id.to_owned());
                                                    }
                                                } else {
                                                    row_ui.add_space(theme::SPACE_12);
                                                    row_ui.horizontal(|empty|{
                                                        empty.add_space(theme::SPACE_12);
                                                        empty.label(typography::caption(
                                                            if !self.loaded {
                                                                "거래처 명단을 불러오는 중…"
                                                            } else if self.searching {
                                                                "검색 중…"
                                                            } else if self.clients.is_empty(){
                                                                "등록된 거래처가 없습니다."
                                                            } else {
                                                                "검색 결과가 없습니다."
                                                            }
                                                        ));
                                                    });
                                                }
                                            }
                                        );
                                    }
                                }
                            );
                            let header_left=first_row_left.unwrap_or(
                                header_rect.left()-output.state.offset.x
                            );
                            if let Some(column)=components::management_client_header(
                                results_panel,header_rect,header_content,
                                header_left,table_width,self.sort
                            ){
                                self.sort=match self.sort {
                                    Some((old,false)) if old==column=>Some((column,true)),
                                    Some((old,true)) if old==column=>None,
                                    _=>Some((column,false)),
                                };
                                ctx.request_repaint();
                            }
                        }
                    );
                    });
                });
            });
        if let Some(id)=chosen {
            if self.contains(&id){
                floating_notice::sync(
                    ctx,"reception.client_chooser.error",
                    "reception.client_chooser",Tone::Error,None
                );
                return Some(Choice::Selected(id));
            }
        }
        if !is_open{
            floating_notice::sync(
                ctx,"reception.client_chooser.error",
                "reception.client_chooser",Tone::Error,None
            );
            return Some(Choice::Closed);
        }
        None
    }
}

#[cfg(test)]
mod client_chooser_display_tests{
    use super::*;
    use serde_json::json;

    #[test]
    fn master_enrichment_keeps_native_aliases_and_tags(){
        let mut index=vec![json!({"id":"c1","name":"기존",
            "shortNames":"약칭","identifiers":"검색태그","active":true})];
        let master=json!({"clients":[{"id":"c1","name":"정식 거래처",
            "tier":"semi","payment":"POSTPAID","address":"서울",
            "defaultApplyVat":true,"active":true}]});
        enrich_display_fields(&mut index,&master);
        assert_eq!(index[0]["name"],"정식 거래처");
        assert_eq!(index[0]["shortNames"],"약칭");
        assert_eq!(index[0]["identifiers"],"검색태그");
        let data=components::ClientDisplaySummary::from_client(&index[0]);
        assert_eq!(data.tier,"semi");
        assert_eq!(data.conditions,"후불 / 부가세 포함");
    }

    #[test]
    fn default_dialog_fits_shared_client_table_without_horizontal_scroll(){
        // The modal master has 12pt padding per side, a 1pt border per side,
        // and a scrollbar gutter. The remaining table viewport exceeds 900pt.
        let required=theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH
            +2.0*theme::MODAL_SECTION_PADDING
            +2.0*theme::FRAME_STROKE_WIDTH
            +2.0*theme::MODAL_VIEWPORT_GUTTER;
        assert!(theme::CLIENT_CHOOSER_DIALOG_WIDTH>=required);
    }
    #[test]
    fn native_result_order_and_explicit_sort_are_separate(){
        let rows=vec![
            json!({"id":"a","name":"가게","active":true}),
            json!({"id":"b","name":"바다","active":true}),
            json!({"id":"c","name":"삭제","active":false})
        ];
        let ids=vec!["b".to_owned(),"a".to_owned(),"c".to_owned()];
        let native=visible_clients(&rows,&ids,"가",false,None);
        assert_eq!(native.iter().map(|row|text(row,"id"))
            .collect::<Vec<_>>(),vec!["b","a"]);
        let sorted=visible_clients(&rows,&ids,"가",false,Some((0,false)));
        assert_eq!(sorted.iter().map(|row|text(row,"id"))
            .collect::<Vec<_>>(),vec!["a","b"]);
    }
}
