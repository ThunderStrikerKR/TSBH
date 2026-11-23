//! Native client manager: semantic parity with React ClientManager.
//! Layout uses shared management components; validation and persistence are
//! delegated to the Rust catalog client rules via BandoGateway.
use crate::{automatic_message_manager::AutomaticMessageManager,
    client_search_tags, components, gateway::{BandoGateway,ReadOperation},
    theme::{self,ButtonRole}, typography};
use eframe::egui::{self,Align,Context,Layout,Ui};
use serde_json::{json,Value};
use std::{sync::{mpsc::{self,Receiver},Arc},thread,time::{SystemTime,UNIX_EPOCH}};

fn field<'a>(client:&'a Value,key:&str)->&'a str {
    client.get(key).and_then(Value::as_str).unwrap_or("")
}
fn displayed_phone(client:&Value)->&str {
    let phone=field(client,"phone");
    if phone.trim().is_empty(){field(client,"mobile")}else{phone}
}

const CLIENT_ACCOUNT_EMPTY_LABEL:&str="미지정 (상품군 추천 계좌 사용)";

/// Settings accounts are stored as {id, bank, number, holder}; they do not
/// have a "name" field. Keep only complete, identifiable accounts selectable.
fn client_account_is_complete(account:&Value)->bool{
    !field(account,"id").trim().is_empty()
        && ["bank","number","holder"].iter()
            .all(|key|!field(account,key).trim().is_empty())
}

fn client_account_display(account:&Value)->String{
    format!("{} {} {}",
        field(account,"bank").trim(),
        field(account,"number").trim(),
        field(account,"holder").trim()
    )
}

/// Match React AccountPicker: the unassigned choice is always available,
/// followed by complete accounts labelled with bank, number and holder.
fn client_account_choices(accounts:&[Value])->Vec<(String,String)>{
    let mut choices=vec![(
        String::new(),CLIENT_ACCOUNT_EMPTY_LABEL.to_owned()
    )];
    choices.extend(accounts.iter()
        .filter(|account|client_account_is_complete(account))
        .map(|account|(
            field(account,"id").to_owned(),
            client_account_display(account)
        )));
    choices
}
fn label_edit(ui:&mut Ui,draft:&mut Value,key:&str,label:&str) {
    ui.vertical(|ui|{
        // React client-editor fields use a compact label/control gap;
        // keep the shared body typeface and standard 36pt field height.
        ui.spacing_mut().item_spacing.y=theme::SPACE_4;
        ui.label(typography::field_label(label));
        let mut input=field(draft,key).to_owned();
        if components::flexible_text_field(
            ui,&mut input,ui.available_width(),""
        ).changed() {
            draft[key]=json!(input);
        }
    });
}
fn bool_edit(ui:&mut Ui,draft:&mut Value,key:&str,title:&str) {
    let mut selected=draft.get(key).and_then(Value::as_bool).unwrap_or(false);
    if components::checkbox(ui,&mut selected,title,true).changed(){
        draft[key]=json!(selected);
    }
}
fn new_id()->String {
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|time|time.as_nanos()).unwrap_or(0);
    format!("egui-client-{}-{stamp}",std::process::id())
}
fn empty_client()->Value {
    json!({"id":new_id(),"name":"","shortNames":"",
        "identifiers":"","address":"","phone":"","mobile":"",
        "tier":"wholesale","payment":"PREPAID",
        "preferredAccountId":null,"shippingFeeExcluded":false,
        "defaultApplyVat":false,"active":true,"contact":"",
        "overrides":{}})
}
type AsyncState=Receiver<Result<Value,String>>;
type SearchState=Receiver<(u64,Result<Vec<String>,String>)>;

pub struct ClientManager {
    state:Value,
    ready:bool,
    loading:Option<AsyncState>,
    saving:Option<AsyncState>,
    search_rx:Option<SearchState>,
    generation:u64,
    matches:Vec<String>,
    search:String,
    searching:bool,
    // None = Rust/source order; false = ascending; true = descending.
    sort:Option<(usize,bool)>,
    error:Option<String>,
    changed:bool,
    draft:Option<Value>,
    original:Option<Value>,
    confirm_delete:bool,
    confirm_discard:bool,
    exception_id:String,
    exception_price:String,
    tag_input:String,
    tag_notice:Option<String>,
    show_messages:bool,
    messages:Option<AutomaticMessageManager>,
}

impl ClientManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let (tx,rx)=mpsc::channel();
        thread::spawn(move||{
            let result=gateway.load_client_master();
            let _=tx.send(result);
            ctx.request_repaint();
        });
        Self {
            state:Value::Null,ready:false,loading:Some(rx),saving:None,
            search_rx:None,generation:0,matches:Vec::new(),search:String::new(),
            searching:false,sort:None,error:None,changed:false,
            draft:None,original:None,confirm_delete:false,confirm_discard:false,
            exception_id:String::new(),exception_price:String::new(),
            tag_input:String::new(),tag_notice:None,
            show_messages:false,messages:None,
        }
    }
    pub fn has_modal(&self)->bool {
        self.draft.is_some() || self.messages.as_ref()
            .is_some_and(|message|self.show_messages && message.has_confirmation())
    }
    pub fn has_message_confirmation(&self)->bool{
        self.show_messages && self.messages.as_ref()
            .is_some_and(AutomaticMessageManager::has_confirmation)
    }
    pub fn global_leave_in_progress(&self)->bool{
        self.show_messages && self.messages.as_ref()
            .is_some_and(AutomaticMessageManager::global_leave_in_progress)
    }
    pub fn show_message_confirmation(
        &mut self,ctx:&Context,area:egui::Rect,gateway:Arc<dyn BandoGateway>
    ){
        if self.show_messages {
            if let Some(manager)=self.messages.as_mut(){
                manager.show_confirmation(ctx,area,gateway);
            }
        }
    }
    pub fn request_global_leave(&mut self)->bool{
        if !self.show_messages{return true;}
        self.messages.as_mut().is_none_or(|m|m.request_global_leave())
    }
    pub fn take_global_approved(&mut self)->bool{
        self.messages.as_mut().is_some_and(|m|m.take_global_approved())
    }
    pub fn take_changed(&mut self)->bool {
        std::mem::take(&mut self.changed)
    }
    fn editable(&self)->bool {
        self.state.is_object() &&
        self.state.get("readOnly")!=Some(&Value::Bool(true))
    }
    fn dirty(&self)->bool {
        !self.tag_input.trim().is_empty() ||
        self.draft.as_ref().zip(self.original.as_ref())
            .is_some_and(|(draft,old)|draft!=old)
    }
    fn clients(&self)->&[Value] {
        self.state.get("clients").and_then(Value::as_array)
            .map(Vec::as_slice).unwrap_or(&[])
    }
    fn begin(&mut self,client:Value) {
        self.original=Some(client.clone());
        self.draft=Some(client);
        self.error=None;
        self.confirm_delete=false;
        self.confirm_discard=false;
        self.exception_id.clear();
        self.exception_price.clear();
        self.tag_input.clear();
        self.tag_notice=None;
    }
    fn query(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context) {
        self.generation=self.generation.wrapping_add(1);
        self.search_rx=None;
        self.matches.clear();
        if self.search.trim().is_empty() {
            self.searching=false;
            return;
        }
        self.searching=true;
        let generation=self.generation;
        let query=self.search.clone();
        let clients=self.clients().to_vec();
        let (tx,rx)=mpsc::channel();
        self.search_rx=Some(rx);
        thread::spawn(move||{
            let result=gateway.execute(ReadOperation::ClientCandidates,json!({
                "state":{"clients":clients},"query":query,"raw":""
            })).and_then(|value|{
                value.get("ids").and_then(Value::as_array)
                    .ok_or("Rust 거래처 검색 응답이 잘못되었습니다.".into())
                    .and_then(|rows|rows.iter().map(|id|
                        id.as_str().map(str::to_owned)
                          .ok_or_else(||"거래처 검색 ID가 잘못되었습니다.".into())
                    ).collect())
            });
            let _=tx.send((generation,result));
            ctx.request_repaint();
        });
    }
    fn poll(&mut self,gateway:Arc<dyn BandoGateway>,ctx:&Context) {
        let loaded=self.loading.as_ref().map(|rx|rx.try_recv());
        match loaded {
            Some(Ok(Ok(state)))=>{
                self.state=state;self.ready=true;self.loading=None;
                self.error=None;
                self.query(Arc::clone(&gateway),ctx.clone());
            },
            Some(Ok(Err(error)))=>{
                self.ready=true;self.loading=None;self.error=Some(error);
            },
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.ready=true;self.loading=None;
                self.error=Some("거래처 명단을 불러오지 못했습니다.".into());
            },
            _=>{},
        }
        let saved=self.saving.as_ref().map(|rx|rx.try_recv());
        match saved {
            Some(Ok(Ok(state)))=>{
                self.state=state;self.saving=None;
                self.draft=None;self.original=None;self.confirm_delete=false;
                self.confirm_discard=false;self.error=None;
                self.tag_input.clear();self.tag_notice=None;
                self.changed=true;
                crate::floating_notice::publish(
                    ctx,"clients.action",crate::floating_notice::Tone::Success,
                    "거래처 변경사항을 반영했습니다."
                );
                self.query(Arc::clone(&gateway),ctx.clone());
            },
            Some(Ok(Err(error)))=>{
                self.error=Some(error);self.saving=None;
            },
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.error=Some("거래처 저장 연결이 중단되었습니다.".into());
                self.saving=None;
            },
            _=>{},
        }
        let search=self.search_rx.as_ref().map(|rx|rx.try_recv());
        match search {
            Some(Ok((generation,result))) if generation==self.generation=>{
                match result {
                    Ok(ids)=>{self.matches=ids;self.searching=false;},
                    Err(error)=>{self.error=Some(error);self.searching=false;}
                }
                self.search_rx=None;
            },
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.error=Some("Rust 거래처 검색 연결이 중단되었습니다.".into());
                self.searching=false;self.search_rx=None;
            },
            _=>{},
        }
    }
    fn mutate(&mut self,gateway:Arc<dyn BandoGateway>,ctx:&Context,action:&str) {
        if self.saving.is_some() || !self.editable(){return;}
        let Some(client)=self.draft.clone() else {return};
        let Some(revision)=self.state.get("revision").and_then(Value::as_u64)
            else{self.error=Some("거래처 revision을 확인할 수 없습니다.".into());return};
        crate::floating_notice::dispatch_pointer(ctx,"clients.action");
        let (tx,rx)=mpsc::channel();
        self.saving=Some(rx);
        self.error=None;
        let action=action.to_owned();
        let ctx=ctx.clone();
        thread::spawn(move||{
            let result=gateway.mutate_client_master(revision,&action,client);
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn request_close(&mut self) {
        if self.saving.is_some(){return;}
        if self.dirty() {
            self.confirm_discard=true;
        } else {
            self.draft=None;
            self.original=None;
            self.tag_input.clear();
            self.tag_notice=None;
        }
    }
    /// Keep the existing actions in their approved order, anchored right.
    /// Rendering alone is refactored; Excel remains disabled until migrated.
    fn list_toolbar_actions(&mut self,ui:&mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center),|actions|{
            if theme::action(actions,"＋ 거래처 등록",ButtonRole::Primary,
                self.ready && self.editable() && self.saving.is_none()
            ).clicked(){
                self.begin(empty_client());
            }
            theme::action(actions,"Excel 관리",ButtonRole::Standard,false)
                .on_disabled_hover_text("Excel 관리 기능은 이전되지 않았습니다.");
        });
    }

    fn list_search(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>) {
        // The placeholder is sufficient. The toolbar owns a fixed
        // left-hand rectangle, so no guesswork about button widths here.
        let width=ui.available_width().max(60.0)
            .min(theme::MANAGEMENT_CLIENT_SEARCH_MAX_WIDTH);
        if components::management_search_field(
            ui,&mut self.search,width,"상호 / 주소 / 전화번호 / 별칭 검색"
        ).changed(){
            self.query(gateway,ui.ctx().clone());
        }
    }

    pub fn show_page(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>) {
        let page_origin_y=ui.cursor().top();
        self.poll(Arc::clone(&gateway),ui.ctx());
        crate::floating_notice::sync(
            ui.ctx(),"clients.error","clients.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        // The client directory uses the same always-readable 10pt solid
        // scrollbar track as the approved Freight Assignment screen.
        ui.spacing_mut().scroll=theme::management_scroll_style();
        components::management_page_title_top_gap(ui);
        // The four actual client tiers fill the same four KPI slots as
        // Freight > Shipping Group Assignment. These counts intentionally
        // include every non-deleted client, not just filtered search rows.
        let mut tier_counts=[0usize;4];
        let mut filtered_count=0usize;
        for client in self.clients().iter()
            .filter(|client|field(client,"deletedAt").is_empty()){
            match field(client,"tier"){
                "wholesale"=>tier_counts[0]+=1,
                "semi"=>tier_counts[1]+=1,
                "retail"=>tier_counts[2]+=1,
                "special"=>tier_counts[3]+=1,
                _=>{},
            }
            if self.search.trim().is_empty() ||
                (!self.searching && self.matches.iter()
                    .any(|id|id==field(client,"id"))){
                filtered_count+=1;
            }
        }
        let list_status=if !self.ready{
            "불러오는 중".to_owned()
        }else if self.searching{
            "검색 중".to_owned()
        }else{
            format!("{}개",filtered_count)
        };
        components::management_top_content(ui,|top|{
            top.horizontal(|heading|{
                heading.label(typography::page_heading("거래처 관리"));
                if !self.show_messages {
                    heading.with_layout(
                        Layout::right_to_left(Align::Center),|right|{
                            right.label(typography::caption(&list_status));
                        }
                    );
                }
            });
        });
        ui.add_space(theme::SPACE_8);
        // Keep the existing second tab disabled until its native workflow
        // is actually migrated; a visual refresh must not fake availability.
        if let Some(manager)=self.messages.as_mut(){
            manager.poll();
            if manager.take_clients_approved(){self.show_messages=false;}
        }
        let chosen_tab=components::management_top_content(ui,|top|
            components::management_tabs(top,&[
                ("거래처 목록",!self.show_messages,true),
                ("자동 메시지 편집",self.show_messages,true),
            ])
        );
        if let Some(tab)=chosen_tab{
            if tab==1 && !self.show_messages {
                if self.messages.is_none(){
                    self.messages=Some(AutomaticMessageManager::new(
                        Arc::clone(&gateway),ui.ctx().clone()
                    ));
                }
                self.show_messages=true;
            }else if tab==0 && self.show_messages{
                if let Some(manager)=self.messages.as_mut(){
                    manager.request_clients();
                    if manager.take_clients_approved(){self.show_messages=false;}
                }
            }
        }
        if self.show_messages {
            // The automatic-message editor has its own nested tabs, not
            // four client-tier KPIs. Preserve that established workflow.
            ui.add_space(theme::SPACE_16);
            if let Some(manager)=self.messages.as_mut(){
                manager.show(ui,gateway,page_origin_y);
            }
            return;
        }
        // Match Freight's 12pt tabs-to-KPI and 16pt KPI-to-card rhythm.
        ui.add_space(theme::SPACE_12);
        components::management_top_content(ui,|top|{
            top.horizontal_wrapped(|strip|{
                strip.spacing_mut().item_spacing=egui::vec2(
                    theme::SPACE_8,theme::SPACE_8
                );
                for (label,count) in [
                    ("도매",tier_counts[0]),("반도매",tier_counts[1]),
                    ("소매",tier_counts[2]),("특수",tier_counts[3])
                ]{
                    components::management_summary_card(strip,label,count);
                }
            });
        });
        ui.add_space(theme::SPACE_16);

        // A filtered copy keeps Rust's search IDs authoritative.
        // Sorting changes display order only, never the backing master.
        let mut rows=self.clients().iter()
            .filter(|row|field(row,"deletedAt").is_empty())
            .filter(|row|self.search.trim().is_empty() ||
                (!self.searching && self.matches.iter()
                    .any(|id|id==field(row,"id"))))
            .cloned().collect::<Vec<_>>();
        if let Some((column,descending))=self.sort{
            rows.sort_by(|left,right|{
                let a=match column {
                    0=>field(left,"name"),
                    1=>field(left,"address"),
                    2=>displayed_phone(left),
                    _=>components::client_tier_label(field(left,"tier")),
                };
                let b=match column {
                    0=>field(right,"name"),
                    1=>field(right,"address"),
                    2=>displayed_phone(right),
                    _=>components::client_tier_label(field(right,"tier")),
                };
                let compared=a.to_lowercase().cmp(&b.to_lowercase())
                    .then_with(||field(left,"id").cmp(field(right,"id")));
                if descending{compared.reverse()}else{compared}
            });
        }

        // Same FULL master table frame as Freight Assignment: one 16pt
        // inset on every edge, one 36pt toolbar, one 21pt transition and
        // one fixed 36pt header. No extra '등록 거래처' title row/card.
        components::align_management_primary_card(ui,page_origin_y);
        let card_min_height=
            components::management_master_table_card_min_inner_height(ui);
        theme::management_master_table_frame().show(ui,|section|{
            components::fill_management_primary_card(section,card_min_height);
            section.spacing_mut().scroll=theme::management_table_scroll_style();
            section.spacing_mut().item_spacing.y=0.0;
            let toolbar_width=section.available_width().max(1.0);
            if toolbar_width<theme::MANAGEMENT_CLIENT_TOOLBAR_STACK_WIDTH {
                // Only genuinely narrow panes stack their SEARCH and
                // actions. The table and header stay on one fixed grid.
                let (search_rect,_)=section.allocate_exact_size(
                    egui::vec2(toolbar_width,theme::INPUT_HEIGHT),
                    egui::Sense::hover()
                );
                section.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("client-master-search-narrow")
                        .max_rect(search_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |search|{
                        search.set_clip_rect(search_rect);
                        search.set_width(search_rect.width());
                        self.list_search(search,Arc::clone(&gateway));
                    }
                );
                section.add_space(theme::SPACE_8);
                let (actions_rect,_)=section.allocate_exact_size(
                    egui::vec2(toolbar_width,theme::INPUT_HEIGHT),
                    egui::Sense::hover()
                );
                section.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("client-master-actions-narrow")
                        .max_rect(actions_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |actions|{
                        actions.set_clip_rect(actions_rect);
                        actions.set_width(actions_rect.width());
                        self.list_toolbar_actions(actions);
                    }
                );
            }else{
                // Freight's separate absolute slots keep the trailing
                // action at the card's inner edge, NOT the scrollbar edge.
                let (toolbar_rect,_)=section.allocate_exact_size(
                    egui::vec2(toolbar_width,theme::INPUT_HEIGHT),
                    egui::Sense::hover()
                );
                let actions_width=theme::MANAGEMENT_TOOLBAR_ACTIONS_WIDTH
                    .min(toolbar_rect.width());
                let left_width=(toolbar_rect.width()-actions_width
                    -theme::SPACE_16).max(1.0);
                let search_rect=egui::Rect::from_min_size(
                    toolbar_rect.min,
                    egui::vec2(left_width,toolbar_rect.height())
                );
                let action_rect=components::management_toolbar_trailing_rect(
                    toolbar_rect,actions_width,0.0,toolbar_rect.height()
                );
                section.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("client-master-search-toolbar")
                        .max_rect(search_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |search|{
                        search.set_clip_rect(search_rect);
                        search.set_width(search_rect.width());
                        self.list_search(search,Arc::clone(&gateway));
                    }
                );
                section.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("client-master-actions-toolbar")
                        .max_rect(action_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |actions|{
                        actions.set_clip_rect(action_rect);
                        actions.set_width(action_rect.width());
                        self.list_toolbar_actions(actions);
                    }
                );
            }
            let layout=components::management_sticky_table_header(
                section,theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH
            );
            let table_width=layout.table_width;
            let header_rect=layout.header_rect;
            let header_content_rect=layout.header_content_rect;
            // This is the only large list in its card; no footer follows.
            // Measure after the title, toolbar and pinned 36pt header.
            let scroll_max_height=components::management_single_list_scroll_height(
                section,card_min_height,0.0
            );
            let mut chosen=None;
            let mut first_row_left=None;
            let scroll_track=components::management_table_scroll_track_rect(
                section,scroll_max_height
            );
            let scroll_output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .scroll_bar_rect(scroll_track)
                    .id_salt("client-management-table-rows")
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                    )
                    .max_height(scroll_max_height)
                    .auto_shrink([false,false]),
                section,table_width,theme::MANAGEMENT_CLIENT_ROW_HEIGHT,
                rows.len().max(1),
                |table,visible_rows|{
                    table.set_min_width(table_width);
                    table.spacing_mut().item_spacing.y=0.0;
                    let first=visible_rows.start;
                    let origin=table.max_rect().min;
                    let rows_clip=table.clip_rect();
                    for index in visible_rows{
                        let row_rect=egui::Rect::from_min_size(
                            egui::pos2(
                                origin.x,
                                origin.y+(index-first) as f32
                                    *theme::MANAGEMENT_CLIENT_ROW_HEIGHT
                            ),
                            egui::vec2(
                                table_width,theme::MANAGEMENT_CLIENT_ROW_HEIGHT
                            )
                        );
                        first_row_left.get_or_insert(row_rect.left());
                        table.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(("client-management-fixed-row",index))
                                .max_rect(row_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |row|{
                                row.set_clip_rect(rows_clip);
                                if let Some(client)=rows.get(index){
                                    let display=components::ClientDisplaySummary::from_client(client);
                                    let name=display.name.as_str();
                                    let description="";
                                    let address=display.address.as_str();
                                    let phone=display.primary_phone();
                                    let shown_phone=if phone.trim().is_empty(){
                                        "연락처 미등록"
                                    }else{phone};
                                    let extra_mobile=display.secondary_mobile();
                                    let conditions=display.conditions.as_str();
                                    if components::management_row_striped(
                                        row,field(client,"id"),[
                                            (name,description),
                                            (address,""),
                                            (shown_phone,extra_mobile),
                                            (components::client_tier_label(&display.tier),conditions)
                                        ],Some(field(client,"tier")),index
                                    ).clicked(){
                                        chosen=Some(client.clone());
                                    }
                                }else{
                                    let status=if !self.ready{
                                        "거래처 명단을 불러오는 중입니다."
                                    }else if self.searching{
                                        "거래처 검색 중…"
                                    }else if self.search.trim().is_empty(){
                                        "등록된 거래처가 없습니다."
                                    }else{
                                        "검색 결과가 없습니다."
                                    };
                                    row.add_space(theme::SPACE_12);
                                    row.horizontal(|empty|{
                                        empty.add_space(theme::SPACE_12);
                                        empty.label(typography::caption(status));
                                    });
                                }
                            }
                        );
                    }
                }
            );
            let header_left=first_row_left.unwrap_or(
                header_rect.left()-scroll_output.state.offset.x
            );
            if let Some(column)=components::management_client_header(
                section,header_rect,header_content_rect,
                header_left,table_width,self.sort
            ){
                self.sort=match self.sort{
                    Some((old,false)) if old==column=>Some((column,true)),
                    Some((old,true)) if old==column=>None,
                    _=>Some((column,false))
                };
                section.ctx().request_repaint();
            }
            if let Some(client)=chosen{self.begin(client);}
        });
    }
    fn accept_tag_text(&mut self,draft:&mut Value,text:&str){
        let (_,duplicates)=client_search_tags::add(draft,text);
        self.tag_notice=if duplicates>0 {
            Some("중복된 검색 태그는 추가하지 않았습니다.".to_owned())
        }else{None};
    }
    fn search_tag_editor(&mut self,ui:&mut Ui,draft:&mut Value){
        ui.vertical(|panel|{
            panel.spacing_mut().item_spacing.y=theme::SPACE_8;
            panel.label(typography::field_label("검색 태그"));
            let tags=client_search_tags::labels(draft);
            if !tags.is_empty(){
                let mut removed=None;
                panel.horizontal_wrapped(|chips|{
                    chips.spacing_mut().item_spacing=egui::vec2(
                        theme::SPACE_8,theme::SPACE_8
                    );
                    for tag in &tags{
                        if components::removable_tag_chip(chips,tag).clicked(){
                            removed=Some(tag.to_owned());
                        }
                    }
                });
                if let Some(tag)=removed{
                    client_search_tags::remove(draft,&tag);
                    self.tag_notice=None;
                }
            }
            let (changed,submit)=panel.horizontal(|entry|{
                entry.spacing_mut().item_spacing.x=theme::SPACE_8;
                let button_slot=theme::INPUT_HEIGHT*2.5;
                let width=(entry.available_width()-button_slot
                    -theme::SPACE_8).max(60.0);
                let response=components::flexible_text_field(
                    entry,&mut self.tag_input,width,"검색 태그 입력"
                );
                let submit_by_enter=response.has_focus() &&
                    entry.input(|input|input.key_pressed(egui::Key::Enter));
                let submit_by_button=theme::action(
                    entry,"추가",ButtonRole::Standard,
                    !self.tag_input.trim().is_empty()
                ).clicked();
                (response.changed(),submit_by_enter||submit_by_button)
            }).inner;
            if changed {
                self.tag_notice=None;
                if let Some(completed)=client_search_tags::take_delimited(
                    &mut self.tag_input
                ){
                    self.accept_tag_text(draft,&completed);
                }
            }
            if submit {
                let entered=std::mem::take(&mut self.tag_input);
                self.accept_tag_text(draft,&entered);
            }
            if let Some(notice)=self.tag_notice.as_deref(){
                panel.label(typography::caption(notice));
            }
        });
    }
    fn editor_fields(&mut self,ui:&mut Ui,draft:&mut Value) {
        ui.label(typography::section_heading("거래처 식별"));
        label_edit(ui,draft,"name","거래처명");
        self.search_tag_editor(ui,draft);
        components::soft_divider(ui);
        ui.label(typography::section_heading("주소 / 전화번호"));
        label_edit(ui,draft,"address","주소");
        ui.columns(2,|cols|{
            label_edit(&mut cols[0],draft,"phone","전화번호");
            label_edit(&mut cols[1],draft,"mobile","휴대폰번호");
        });
        components::soft_divider(ui);
        ui.label(typography::section_heading("거래 조건"));
        ui.columns(2,|cols|{
            cols[0].label(typography::field_label("가격 등급"));
            let mut tier=field(draft,"tier").to_owned();
            let width=cols[0].available_width();
            components::management_dropdown(
                &mut cols[0],components::client_tier_label(&tier),
                width,true,"가격 등급 선택",|ui|{
                    for (key,label) in [
                        ("wholesale","도매"),("semi","반도매"),
                        ("retail","소매"),("special","특수")
                    ] {
                        components::management_dropdown_option(
                            ui,&mut tier,key.to_owned(),label);
                    }
                });
            draft["tier"]=json!(tier);
            cols[1].label(typography::field_label("결제 방식"));
            let mut payment=field(draft,"payment").to_owned();
            let width=cols[1].available_width();
            components::management_dropdown(
                &mut cols[1],if payment=="POSTPAID"{"후불"}else{"선입금"},
                width,true,"결제 방식 선택",|ui|{
                    components::management_dropdown_option(
                        ui,&mut payment,"PREPAID".into(),"선입금");
                    components::management_dropdown_option(
                        ui,&mut payment,"POSTPAID".into(),"후불");
                });
            draft["payment"]=json!(payment);
        });
        ui.label(typography::field_label("주 사용 입금계좌"));
        let accounts=self.state.pointer("/settings/accounts")
            .and_then(Value::as_array).cloned().unwrap_or_default();
        if accounts.is_empty() {
            ui.label(typography::caption("미지정 (상품군 추천 계좌 사용)"));
        } else {
            let mut selected=field(draft,"preferredAccountId").to_owned();
            let selected_label=accounts.iter()
                .find(|a|field(a,"id")==selected)
                .map(|a|field(a,"name")).unwrap_or("미지정");
            let width=ui.available_width();
            components::management_dropdown(
                ui,selected_label,width,true,"주 사용 입금계좌 선택",|ui|{
                    components::management_dropdown_option(
                        ui,&mut selected,String::new(),"미지정");
                    for account in &accounts {
                        let id=field(account,"id").to_owned();
                        let label=field(account,"name");
                        components::management_dropdown_option(ui,&mut selected,id,label);
                    }
                });
            draft["preferredAccountId"]=if selected.is_empty(){Value::Null}
                else{json!(selected)};
        }
        ui.horizontal_wrapped(|flags|{
            bool_edit(flags,draft,"shippingFeeExcluded","배송비 청구 제외");
            bool_edit(flags,draft,"defaultApplyVat","부가세 포함");
        });
        if field(draft,"tier")=="special" {
            components::soft_divider(ui);
            ui.label(typography::section_heading("특수 / 예외 품목 단가"));
            let mut remove=None;
            if let Some(overrides)=draft.get("overrides").and_then(Value::as_object) {
                for (id,price) in overrides {
                    ui.horizontal(|row|{
                        let label=self.variant_name(id);
                        // A single LayoutJob keeps the approved row/action
                        // spacing while rendering only the monetary tail
                        // with the shared Consolas amount policy.
                        row.add(egui::Label::new(typography::inline_body_money(
                            &format!("{label} · "),&format!("{price}원")
                        )));
                        if theme::action(row,"예외 해제",ButtonRole::Quiet,true)
                            .clicked(){remove=Some(id.clone());}
                    });
                }
            }
            if let Some(id)=remove {
                if let Some(data)=draft.get_mut("overrides").and_then(Value::as_object_mut) {
                    data.remove(&id);
                }
            }
            let mut variants=Vec::new();
            for product in self.state.get("products").and_then(Value::as_array)
                .into_iter().flatten() {
                let product_name=field(product,"name");
                for variant in product.get("variants").and_then(Value::as_array)
                    .into_iter().flatten() {
                    let id=field(variant,"id");
                    if !id.is_empty() {
                        variants.push((id.to_owned(),format!(
                            "{} / {}",product_name,field(variant,"color")
                        )));
                    }
                }
            }
            ui.horizontal(|row|{
                components::management_dropdown(
                    row,variants.iter()
                        .find(|(id,_)|id==&self.exception_id)
                        .map(|(_,label)|label.as_str())
                        .unwrap_or("예외 품목 / 색상 선택"),
                    theme::MANAGEMENT_SEARCH_MIN_WIDTH,true,
                    "특수 단가 적용 품목 선택",|ui|{
                        for (id,label) in &variants {
                            components::management_dropdown_option(
                                ui,&mut self.exception_id,id.clone(),label);
                        }
                    });
                components::form_field_label(row,"특수 단가");
                components::money_field(row,&mut self.exception_price,true);
                // The common editor formats even while focused; the
                // business override still receives an unpunctuated integer.
                let amount=components::parse_money_input(
                    &self.exception_price
                ).and_then(|value|u64::try_from(value).ok());
                if theme::action(row,"예외 추가",ButtonRole::Standard,
                    !self.exception_id.is_empty() && amount.is_some()
                ).clicked(){
                    if let Some(amount)=amount {
                        if !draft["overrides"].is_object(){
                            draft["overrides"]=json!({});
                        }
                        draft["overrides"][self.exception_id.as_str()]=json!(amount);
                        self.exception_id.clear();self.exception_price.clear();
                    }
                }
            });
        }
        let aliases=self.state.get("aliases").and_then(Value::as_array)
            .into_iter().flatten()
            .filter(|a|field(a,"clientId")==field(draft,"id"))
            .cloned().collect::<Vec<_>>();
        let history=draft.get("history").and_then(Value::as_array)
            .cloned().unwrap_or_default();
        if !aliases.is_empty() || !history.is_empty() {
            components::soft_divider(ui);
            egui::CollapsingHeader::new("상품 별칭 / 변경 이력")
                .icon(components::paint_collapsing_header_icon)
                .show(ui,|ui|{
                for alias in &aliases {
                    let product_name=self.state.get("products")
                        .and_then(Value::as_array).into_iter().flatten()
                        .find(|p|field(p,"id")==field(alias,"productId"))
                        .map(|p|field(p,"name")).unwrap_or("과거 품목");
                    ui.label(typography::body(&format!(
                        "{} → {}",field(alias,"text"),product_name
                    )));
                }
                for row in history.iter().rev(){
                    ui.label(typography::caption(&format!(
                        "{} / {}",field(row,"at"),field(row,"field")
                    )));
                }
            });
        }
    }
    fn variant_name(&self,id:&str)->String {
        for product in self.state.get("products").and_then(Value::as_array)
            .into_iter().flatten(){
            if product.get("variants").and_then(Value::as_array)
                .is_some_and(|vs|vs.iter().any(|v|field(v,"id")==id)){
                return field(product,"name").to_owned();
            }
        }
        id.to_owned()
    }
    pub fn show_editor(
        &mut self,ctx:&Context,work_area:egui::Rect,gateway:Arc<dyn BandoGateway>
    ){
        let Some(mut draft)=self.draft.take() else{return;};
        let exists=self.clients().iter()
            .any(|row|field(row,"id")==field(&draft,"id"));
        let title=if exists {"거래처 상세 관리"}else{"거래처 등록"};
        let mut save=false;
        let mut delete=false;
        let mut close=false;
        let editable=self.editable();
        let busy=self.saving.is_some();
        components::modal_window(title,"native-client-manager-editor",work_area)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|ui|{
                if components::modal_header(ui,title,"취소",!busy){
                    close=true;
                }
                // Reserve the native header, footer, gap and outer frame
                // before allocating the only scrollable editor region.
                let body_height=components::modal_editor_body_max_height(
                    work_area.height()
                );
                egui::ScrollArea::vertical()
                    .id_salt("native-client-manager-fields")
                    .max_height(body_height)
                    .auto_shrink([false,false])
                    .show(ui,|body|{
                        body.add_enabled_ui(editable && !busy,|form|{
                            self.editor_fields(form,&mut draft);
                        });
                    });
                ui.add_space(theme::SPACE_12);
                components::modal_action_row_split(
                    ui,
                    |left|{
                        if exists && theme::action(left,"거래처 삭제",
                            ButtonRole::Danger,editable && !busy).clicked(){
                            delete=true;
                        }
                    },
                    |right|{
                        let label=if exists {"변경사항 저장"}else{"거래처 등록"};
                        let dirty=self.original.as_ref()
                            .is_some_and(|old|*old!=draft)
                            || !self.tag_input.trim().is_empty();
                        if theme::action(right,label,ButtonRole::Primary,
                            editable && !busy && (!exists || dirty)).clicked(){
                            save=true;
                        }
                    }
                );
            });
        if ctx.input(|input|input.key_pressed(egui::Key::Escape)) {
            if self.confirm_delete || self.confirm_discard {
                self.confirm_delete=false;
                self.confirm_discard=false;
            } else {
                close=true;
            }
        }
        // Saving with an unfinished tag must never silently drop the input.
        // Resolve it into the same canonical shortNames draft before CAS.
        if save && !self.tag_input.trim().is_empty(){
            let pending=std::mem::take(&mut self.tag_input);
            self.accept_tag_text(&mut draft,&pending);
        }
        self.draft=Some(draft);
        if save && self.dirty(){
            self.mutate(Arc::clone(&gateway),ctx,"upsert_client");
        } else if save && !exists {
            self.mutate(Arc::clone(&gateway),ctx,"upsert_client");
        }
        if delete {self.confirm_delete=true;}
        if close {self.request_close();}
        if self.confirm_delete {
            components::modal_backdrop(ctx,"client-manager-delete-backdrop",true);
            components::decision_window(
                "거래처 삭제","client-manager-delete-confirm",work_area
            ).show(ctx,|ui|{
                let cancel=components::modal_header(ui,"거래처 삭제","취소",true);
                ui.label("거래처를 삭제할까요? 기존 주문의 송하인과 금액은 그대로 보존합니다.");
                let approve=components::modal_action_row(ui,|buttons|{
                        theme::action(
                            buttons,"거래처 삭제",ButtonRole::Danger,true
                        ).clicked()
                    }
                ).inner;
                if cancel {self.confirm_delete=false;}
                if approve {
                    self.confirm_delete=false;
                    self.mutate(Arc::clone(&gateway),ctx,"archive_client");
                }
            });
        }
        if self.confirm_discard {
            components::modal_backdrop(ctx,"client-manager-discard-backdrop",true);
            components::decision_window(
                "변경사항 확인","client-manager-discard-confirm",work_area
            ).show(ctx,|ui|{
                let cancel=components::modal_header(ui,"변경사항 확인","취소",true);
                ui.label("거래처의 수정 내용을 버릴까요?");
                let approve=components::modal_action_row(ui,|buttons|{
                        theme::action(
                            buttons,"변경 취소",ButtonRole::Danger,true
                        ).clicked()
                    }
                ).inner;
                if cancel {self.confirm_discard=false;}
                if approve {
                    self.confirm_discard=false;self.draft=None;self.original=None;
                    self.tag_input.clear();self.tag_notice=None;
                }
            });
        }
    }
}

#[cfg(test)]
mod client_condition_summary_tests{
    use super::*;
    #[test]
    fn normal_client_does_not_repeat_default_conditions(){
        let client=json!({"payment":"PREPAID","defaultApplyVat":false,
            "shippingFeeExcluded":false,"active":true});
        assert_eq!(components::client_exception_terms(&client),"");
    }
    #[test]
    fn exceptional_client_terms_have_no_empty_separators(){
        let postpaid=json!({"payment":"POSTPAID","defaultApplyVat":false,
            "shippingFeeExcluded":false,"active":true});
        assert_eq!(components::client_exception_terms(&postpaid),"후불");
        let vat=json!({"payment":"PREPAID","defaultApplyVat":true,
            "shippingFeeExcluded":false,"active":true});
        assert_eq!(components::client_exception_terms(&vat),"부가세 포함");
        let extra=json!({"payment":"PREPAID","defaultApplyVat":false,
            "shippingFeeExcluded":true,"active":false});
        assert_eq!(components::client_exception_terms(&extra),"운임 청구 제외 / 거래 중지");
        let all=json!({"payment":"POSTPAID","defaultApplyVat":true,
            "shippingFeeExcluded":true,"active":false});
        assert_eq!(components::client_exception_terms(&all),
            "후불 / 부가세 포함 / 운임 청구 제외 / 거래 중지");
    }
}
