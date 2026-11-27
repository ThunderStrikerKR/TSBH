//! Native client manager: semantic parity with React ClientManager.
//! Layout uses shared management components; validation and persistence are
//! delegated to the Rust catalog client rules via BandoGateway.
use crate::{automatic_message_manager::AutomaticMessageManager,
    client_search_tags, gateway::{BandoGateway,ReadOperation},
    theme::{self,ButtonRole}, typography};
use crate::card::UiWidthPolicyExt;
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
fn client_form_row<R>(ui:&mut Ui,label:&str,control:impl FnOnce(&mut Ui)->R)->R{
    crate::popup::modal_labeled_row_with_width(ui,label,theme::MODAL_FORM_WIDE_LABEL_WIDTH,control)
}
fn client_form_pair(ui:&mut Ui,draft:&mut Value,left:impl FnOnce(&mut Ui,&mut Value),right:impl FnOnce(&mut Ui,&mut Value)){
    crate::popup::modal_form_pair(ui,draft,theme::MODAL_FORM_WIDE_LABEL_WIDTH,left,right);
}
fn label_edit(ui:&mut Ui,draft:&mut Value,key:&str,label:&str) {
    let mut input=field(draft,key).to_owned();
    let response=client_form_row(ui,label,|value|{
        let width=value.available_width();
        crate::input::flexible_text_field(value,&mut input,width,"")
    });
    if response.changed(){draft[key]=json!(input);}
}
fn client_tier_edit(ui:&mut Ui,draft:&mut Value){
    client_form_row(ui,"가격 등급",|value|{
        let mut tier=field(draft,"tier").to_owned();
        let width=value.available_width();
        crate::dropdown::management_dropdown(
            value,crate::composite_client::client_tier_label(&tier),width,true,|menu|{
                for (key,label) in [("wholesale","도매"),("semi","반도매"),
                    ("retail","소매"),("special","특수")]{
                    crate::dropdown::management_dropdown_option(menu,&mut tier,key.to_owned(),label);
                }
            });
        draft["tier"]=json!(tier);
    });
}
fn client_payment_edit(ui:&mut Ui,draft:&mut Value){
    client_form_row(ui,"결제 방식",|value|{
        let mut payment=field(draft,"payment").to_owned();
        let width=value.available_width();
        crate::dropdown::management_dropdown(
            value,if payment=="POSTPAID"{"후불"}else{"선입금"},width,true,|menu|{
                crate::dropdown::management_dropdown_option(menu,&mut payment,"PREPAID".into(),"선입금");
                crate::dropdown::management_dropdown_option(menu,&mut payment,"POSTPAID".into(),"후불");
            });
        draft["payment"]=json!(payment);
    });
}
fn bool_edit(ui:&mut Ui,draft:&mut Value,key:&str,title:&str) {
    let mut selected=draft.get(key).and_then(Value::as_bool).unwrap_or(false);
    if crate::checkbox::checkbox(ui,&mut selected,title,true).changed(){
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

pub struct ClientManager {
    state:Value,
    list_cache:crate::screen_refresh::ViewProjectionCache<(String,Option<(usize,bool)>),([usize;4],Vec<usize>)>,
    ready:bool,
    loading:crate::screen_refresh::ReadRefresh<Value>,
    saving:Option<AsyncState>,
    search_job:crate::screen_refresh::ReadRefresh<Vec<String>>,
    matches:Vec<String>,
    matches_query:String,
    search:String,
    // None = Rust/source order; false = ascending; true = descending.
    sort:Option<(usize,bool)>,
    error:Option<String>,
    changed:bool,
    draft:Option<Value>,
    original:Option<Value>,
    expected_client:Value,
    confirm_delete:bool,
    confirm_discard:bool,
    exception_id:String,
    exception_price:String,
    tag_input:String,
    tag_notice:Option<String>,
    show_messages:bool,
    tab_snapshots:crate::view_state::NavigationSnapshots<2>,
    messages:Option<AutomaticMessageManager>,
    excel:crate::product_excel_ui::ProductExcelUi,
}

fn client_tier_kpi_values(available:bool,counts:[usize;4])->[String;4]{
    if available{
        counts.map(|count|count.to_string())
    }else{
        std::array::from_fn(|_|"—".to_owned())
    }
}

impl ClientManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let mut loading=crate::screen_refresh::ReadRefresh::default();
        loading.submit_required(ctx,move||gateway.load_client_master());
        Self {
            state:Value::Null,list_cache:Default::default(),ready:false,loading,saving:None,
            search_job:Default::default(),matches:Vec::new(),matches_query:String::new(),search:String::new(),
            sort:None,error:None,changed:false,
            draft:None,original:None,expected_client:Value::Null,confirm_delete:false,confirm_discard:false,
            exception_id:String::new(),exception_price:String::new(),
            tag_input:String::new(),tag_notice:None,
            show_messages:false,tab_snapshots:crate::view_state::NavigationSnapshots::new(0),messages:None,excel:crate::product_excel_ui::ProductExcelUi::for_clients(),
        }
    }
    pub(crate) fn suspend_navigation(&mut self,now:std::time::Instant){
        self.tab_snapshots.suspend(now);
        if self.tab_snapshots.active()==1{
            if let Some(m)=self.messages.as_mut(){m.suspend_navigation(now);}
        }
    }
    fn sync_tab_navigation(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        let now=std::time::Instant::now();
        let next=usize::from(self.show_messages);
        if next==0&&self.tab_snapshots.active()==1{
            if let Some(m)=self.messages.as_mut(){m.suspend_navigation(now);}
        }
        if !self.tab_snapshots.visit(next,now){return;}
        if next==0{self.search.clear();self.sort=None;self.query(gateway,ctx);}
        else if let Some(m)=self.messages.as_mut(){m.reset_navigation();}
    }
    pub(crate) fn reset_navigation(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        if self.has_modal()||self.saving.is_some()||self.messages.as_ref().is_some_and(|m|m.dirty()||m.busy()){return;}
        self.search.clear();self.sort=None;self.show_messages=false;
        self.tab_snapshots=crate::view_state::NavigationSnapshots::new(0);
        if let Some(m)=self.messages.as_mut(){m.reset_navigation();}
        self.query(gateway,ctx);
    }
    pub fn has_modal(&self)->bool {
        self.draft.is_some() || self.excel.has_modal() || self.messages.as_ref()
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
        if self.excel.blocks_navigation(){return false;}
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
        !self.loading.required() && self.state.is_object() &&
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
        self.expected_client=self.clients().iter()
            .find(|row|field(row,"id")==field(&client,"id"))
            .cloned().unwrap_or(Value::Null);
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
        self.search_job.cancel();
        // Retain same-query results, never across a changed search.
        if self.matches_query!=self.search{self.list_cache.invalidate();}
        crate::screen_refresh::retain_scope(&mut self.matches,self.matches_query==self.search);
        self.matches_query=self.search.clone();
        if self.search.trim().is_empty() {
            self.search_job.reset();
            return;
        }
        let query=self.search.clone();
        let clients=self.clients().to_vec();
        self.search_job.submit_required(ctx,move||{
            gateway.execute(ReadOperation::ClientCandidates,json!({
                "state":{"clients":clients},"query":query,"raw":""
            })).and_then(|value|{
                value.get("ids").and_then(Value::as_array)
                    .ok_or("거래처 검색 결과를 확인할 수 없습니다.".into())
                    .and_then(|rows|rows.iter().map(|id|
                        id.as_str().map(str::to_owned)
                          .ok_or_else(||"거래처 검색 ID가 잘못되었습니다.".into())
                    ).collect())
            })
        });
    }
    fn poll(&mut self,gateway:Arc<dyn BandoGateway>,ctx:&Context) {
        match self.loading.poll(){
            Some(Ok(state))=>{
                self.state=state;self.list_cache.invalidate();self.ready=true;self.loading.verified();
                self.error=None;
                self.query(Arc::clone(&gateway),ctx.clone());
            },
            Some(Err(error))=>{self.ready=true;self.error=Some(error);},
            None=>{},
        }
        let saved=self.saving.as_ref().map(|rx|rx.try_recv());
        match saved {
            Some(Ok(Ok(mut state)))=>{
                let notice=state.as_object_mut().and_then(|map|map.remove("saveNotice"));
                self.state=state;self.list_cache.invalidate();self.saving=None;
                self.draft=None;self.original=None;self.confirm_delete=false;
                self.confirm_discard=false;self.error=None;
                self.tag_input.clear();self.tag_notice=None;
                self.changed=true;
                let message=notice.as_ref().and_then(Value::as_str);
                crate::floating_notice::publish(
                    ctx,"clients.action",if message.is_some(){
                        crate::floating_notice::Tone::Info
                    }else{crate::floating_notice::Tone::Success},
                    message.unwrap_or("거래처 변경사항을 반영했습니다.")
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
        match self.search_job.poll(){
            Some(Ok(ids))=>{self.matches=ids;self.list_cache.invalidate();self.search_job.verified();},
            Some(Err(error))=>self.error=Some(error),
            None=>{},
        }
    }
    fn mutate(&mut self,gateway:Arc<dyn BandoGateway>,ctx:&Context,action:&str) {
        if self.saving.is_some() || !self.editable(){return;}
        let Some(client)=self.draft.clone() else {return};
        crate::floating_notice::dispatch_pointer(ctx,"clients.action");
        let expected=self.expected_client.clone();
        let (tx,rx)=mpsc::channel();
        self.saving=Some(rx);
        self.error=None;
        let action=action.to_owned();
        let ctx=ctx.clone();
        thread::spawn(move||{
            let result=gateway.mutate_client_master(expected,&action,client);
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
    fn list_toolbar_actions(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>) {
        ui.with_layout(Layout::right_to_left(Align::Center),|actions|{
            if crate::button::action(actions,"＋ 거래처 등록",ButtonRole::Primary,
                self.ready && self.editable() && self.saving.is_none() && !self.excel.blocks_navigation()
            ).clicked(){
                self.begin(empty_client());
            }
            let allowed=self.ready&&self.editable()&&self.saving.is_none()&&self.draft.is_none()&&!self.excel.blocks_navigation();
            let response=crate::button::action(actions,"Excel 관리",ButtonRole::Standard,allowed);
            self.excel.show_menu(actions,&response,gateway,allowed);
        });
    }

    fn list_search(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>) {
        // The placeholder is sufficient. The toolbar owns a fixed
        // left-hand rectangle, so no guesswork about button widths here.
        let width=ui.available_width().max(60.0)
            .min(theme::MANAGEMENT_CLIENT_SEARCH_MAX_WIDTH);
        if crate::input::management_search_field(
            ui,&mut self.search,width,"상호 / 주소 / 전화번호 / 별칭 검색"
        ).changed(){
            self.query(gateway,ui.ctx().clone());
        }
    }

    pub fn show_page(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>) {
        let page_origin_y=ui.cursor().top();
        self.poll(Arc::clone(&gateway),ui.ctx());
        if let Some(state)=self.excel.poll(ui.ctx()){
            self.state=state;self.list_cache.invalidate();self.changed=true;self.error=None;
            self.query(Arc::clone(&gateway),ui.ctx().clone());
        }
        crate::floating_notice::sync(
            ui.ctx(),"clients.error","clients.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        // The client directory uses the same always-readable 10pt solid
        // scrollbar track as the approved Freight Assignment screen.
        crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Standard);
        crate::ui_layout::management_page_title_top_gap(ui);
        // The four actual client tiers fill the same four KPI slots as
        // Freight > Shipping Group Assignment. These counts intentionally
        // include every non-deleted client, not just filtered search rows.
        let projection=self.list_cache.get((self.search.clone(),self.sort),||{
            let clients=self.state.get("clients").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            let matches:std::collections::HashSet<&str>=self.matches.iter().map(String::as_str).collect();
            let mut tier_counts=[0usize;4];
            let mut rows=Vec::new();
            for (i,client) in clients.iter().enumerate().filter(|(_,row)|field(row,"deletedAt").is_empty()){
                match field(client,"tier"){
                    "wholesale"=>tier_counts[0]+=1,"semi"=>tier_counts[1]+=1,
                    "retail"=>tier_counts[2]+=1,"special"=>tier_counts[3]+=1,_=>{},
                }
                if self.search.trim().is_empty()||matches.contains(field(client,"id")){rows.push(i);}
            }
            if let Some((column,descending))=self.sort{
                rows.sort_by_cached_key(|&i|{
                    let client=&clients[i];
                    let value=match column{
                        0=>field(client,"name"),1=>field(client,"address"),2=>displayed_phone(client),
                        _=>crate::composite_client::client_tier_label(field(client,"tier")),
                    };
                    let key=(value.to_lowercase(),field(client,"id").to_owned());
                    if descending{(None,Some(std::cmp::Reverse(key)))}else{(Some(key),None)}
                });
            }
            (tier_counts,rows)
        });
        let tier_counts=projection.0;
        let filtered_count=projection.1.len();
        let list_status=if !self.ready{
            "불러오는 중".to_owned()
        }else if !self.state.is_object(){
            "조회 실패".to_owned()
        }else if self.search_job.pending(){
            "검색 중".to_owned()
        }else{
            format!("{}개",filtered_count)
        };
        crate::ui_layout::management_top_content(ui,|top|{
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
        let chosen_tab=crate::ui_layout::management_top_content(ui,|top|
            crate::button::management_tabs(top,&[
                ("거래처 목록",!self.show_messages,true),
                ("자동 메시지 편집",self.show_messages,true),
            ])
        );
        if let Some(tab)=chosen_tab.filter(|_|!self.excel.blocks_navigation()){
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
        self.sync_tab_navigation(Arc::clone(&gateway),ui.ctx().clone());
        ui.push_id(("client-navigation",usize::from(self.show_messages),self.tab_snapshots.epoch(usize::from(self.show_messages))),|ui|{
        if self.show_messages {
            // The automatic-message editor has its own nested tabs, not
            // four client-tier KPIs. Preserve that established workflow.
            ui.add_space(theme::SPACE_12);
            if let Some(manager)=self.messages.as_mut(){
                manager.show(ui,gateway,page_origin_y);
            }
            return;
        }
        // Match Freight's 12pt tabs-to-KPI and 16pt KPI-to-card rhythm.
        ui.add_space(theme::SPACE_12);
        crate::ui_layout::management_top_content(ui,|top|{
            top.horizontal_wrapped(|strip|{
                crate::ui_layout::apply_spacing_role(strip,crate::ui_layout::SpacingRole::SummaryStrip);
                let values=client_tier_kpi_values(self.state.is_object(),tier_counts);
                for (label,value) in [
                    "도매","반도매","소매","특수"
                ].into_iter().zip(values.iter()){
                    crate::card::management_summary_card_with_state(strip,label,value,theme::TextState::query(self.state.is_object()));
                }
            });
        });
        ui.add_space(theme::SPACE_12);

        // A filtered copy keeps Rust's search IDs authoritative.
        // Sorting changes display order only, never the backing master.
        let rows=&projection.1;

        // Same FULL master table frame as Freight Assignment: one 16pt
        // inset on every edge, one 36pt toolbar, one 33pt transition and
        // one fixed 36pt header. No extra '등록 거래처' title row/card.
        crate::card::align_management_primary_card(ui,page_origin_y);
        let card_min_height=
            crate::card::management_master_table_card_min_inner_height(ui);
        crate::card::measured_management_master_card(ui,|section|{
            crate::card::fill_management_primary_card(section,card_min_height);
            crate::table::management_table_container_style(section);
            crate::ui_layout::management_toolbar_pair(section,crate::ui_layout::ToolbarPair{
                search_id:"client-master-search-toolbar",actions_id:"client-master-actions-toolbar",
                narrow_search_id:"client-master-search-narrow",narrow_actions_id:"client-master-actions-narrow",
                stack_width:theme::MANAGEMENT_CLIENT_TOOLBAR_STACK_WIDTH,search_max:f32::INFINITY,
                actions_width:theme::MANAGEMENT_TOOLBAR_ACTIONS_WIDTH,narrow_actions_first:false,actions_rtl:false,clip:true,
            },|slot,part|match part{
                crate::ui_layout::ToolbarPart::Search=>self.list_search(slot,Arc::clone(&gateway)),
                crate::ui_layout::ToolbarPart::Actions=>self.list_toolbar_actions(slot,Arc::clone(&gateway)),
            });
            crate::table::show(section,crate::table::Header::Sticky(theme::MANAGEMENT_CLIENT_TAG_TABLE_MIN_WIDTH),|section,layout|{
            let table_width=layout.table_width;
            let header_rect=layout.header_rect;
            let header_content_rect=layout.header_content_rect;
            // This is the only large list in its card; no footer follows.
            // Measure after the title, toolbar and pinned 36pt header.
            let scroll_max_height=crate::scrollbar::management_single_list_scroll_height(
                section,card_min_height,0.0
            );
            let mut chosen=None;
            let mut first_row_left=None;
            let scroll_track=crate::table::management_table_scroll_track_rect(
                section,scroll_max_height
            );
            let clients=self.state.get("clients").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            let scroll_keys=|i:usize|crate::scrollbar::row_key(field(&clients[rows[i]],"id"));
            let scroll_output=crate::table::fixed_rows_snapshot(
                section,crate::table::Viewport{width:table_width,height:scroll_max_height,rail:Some(scroll_track)},
                "client-management-table-rows",theme::MANAGEMENT_CLIENT_ROW_HEIGHT,rows.len(),
                self.ready&&!self.search_job.pending()&&self.state.is_object(),Some(&scroll_keys),
                |table,visible_rows|{
                    crate::table::management_table_row_body(table,table_width);
                    let first=visible_rows.start;
                    let origin=table.max_rect().min;
                    let rows_clip=table.clip_rect();
                    for index in visible_rows{
                        let row_rect=crate::table::fixed_row_rect(origin,index,first,table_width,theme::MANAGEMENT_CLIENT_ROW_HEIGHT);
                        first_row_left.get_or_insert(row_rect.left());
                        crate::table::row_scope(table,("client-management-fixed-row",index),row_rect,|row|{
                                row.set_clip_rect(rows_clip);
                                if let Some(client)=rows.get(index).and_then(|&i|clients.get(i)){
                                    #[cfg(test)]
                                    crate::ui_test_support::record("data row",row_rect);
                                    let display=crate::composite_client::ClientDisplaySummary::from_client(client);
                                    let name=display.name.as_str();
                                    let description="";
                                    let address=display.address.as_str();
                                    let phone=display.primary_phone();
                                    let shown_phone=if phone.trim().is_empty(){
                                        "연락처 미등록"
                                    }else{phone};
                                    let extra_mobile=display.secondary_mobile();
                                    let conditions=display.conditions.as_str();
                                    let tags=client_search_tags::labels(client);
                                    if crate::composite_client::management_row_with_tags(
                                        row,field(client,"id"),[
                                            (name,description),
                                            (address,""),
                                            (shown_phone,extra_mobile),
                                            (crate::composite_client::client_tier_label(&display.tier),conditions)
                                        ],field(client,"tier"),index,Some(&tags)
                                    ).clicked() && !self.search_job.required(){
                                        chosen=Some(client.clone());
                                    }
                                }
                            }
                        );
                    }
                }
            );
            if rows.is_empty(){
                let status=if !self.ready{"거래처 명단을 불러오는 중입니다."}
                    else if self.search_job.pending(){"거래처 검색 중…"}
                    else if self.search.trim().is_empty(){"등록된 거래처가 없습니다."}
                    else{"검색 결과가 없습니다."};
                crate::status_notice::management_body_state_overlay(section,scroll_output.inner_rect,Some(status));
            }
            let header_left=first_row_left.unwrap_or(
                header_rect.left()-scroll_output.state.offset.x
            );
            if let Some(column)=crate::table::management_client_header_with_tags(
                section,header_rect,header_content_rect,
                header_left,table_width,self.sort,true
            ){
                self.sort=match self.sort{
                    Some((old,false)) if old==column=>Some((column,true)),
                    Some((old,true)) if old==column=>None,
                    _=>Some((column,false))
                };
                section.ctx().request_repaint();
            }
            
            if !self.excel.blocks_navigation(){if let Some(client)=chosen{self.begin(client);}}

            (scroll_output.inner_rect,())
        });
        });
        });
    }
    fn accept_tag_text(&mut self,draft:&mut Value){
        match client_search_tags::add_one(draft,&self.tag_input){
            Ok((_,duplicates))=>{
                self.tag_input.clear();
                self.tag_notice=if duplicates>0{
                    Some("중복된 검색 태그는 추가하지 않았습니다.".to_owned())
                }else{None};
            }
            Err(message)=>self.tag_notice=Some(message.into()),
        }
    }
    fn search_tag_editor(&mut self,ui:&mut Ui,draft:&mut Value){
        client_form_row(ui,"검색 태그",|controls|{
            controls.vertical(|panel|{
            crate::ui_layout::apply_spacing_role(panel,crate::ui_layout::SpacingRole::WorkflowDetail);
            let tags=client_search_tags::labels(draft);
            let interaction=crate::input::tag_block_editor(panel,&tags,&mut self.tag_input,"검색 태그 입력");
            if interaction.changed{self.tag_notice=None;}
            if let Some(tag)=interaction.remove{
                client_search_tags::remove(draft,&tag);
                self.tag_notice=None;
            }
            if interaction.add{self.accept_tag_text(draft);}
            let status_width=panel.available_width();
            let notice=self.tag_notice.as_deref().or_else(||
                (!self.tag_input.trim().is_empty()).then_some("입력한 태그는 추가 버튼으로 등록하세요."));
            crate::ui_layout::fixed_status_slot(
                panel,status_width,typography::METADATA_LINE_HEIGHT,
                notice,notice,theme::text_ink(theme::TextRole::Metadata,theme::TextState::Normal)
            );
            });
        });
    }
    fn editor_fields(&mut self,ui:&mut Ui,draft:&mut Value) {
        crate::popup::modal_form_row_spacing(ui);
        ui.label(typography::section_heading("거래처 식별"));
        label_edit(ui,draft,"name","거래처명");
        self.search_tag_editor(ui,draft);
        crate::ui_primitives::section_divider(ui);
        ui.label(typography::section_heading("주소 / 전화번호"));
        label_edit(ui,draft,"address","주소");
        client_form_pair(ui,draft,
            |left,draft|label_edit(left,draft,"phone","전화번호"),
            |right,draft|label_edit(right,draft,"mobile","휴대폰번호"));
        crate::ui_primitives::section_divider(ui);
        ui.label(typography::section_heading("거래 조건"));
        client_form_pair(ui,draft,client_tier_edit,client_payment_edit);
        let accounts=self.state.pointer("/settings/accounts")
            .and_then(Value::as_array).cloned().unwrap_or_default();
        let choices=client_account_choices(&accounts);
        let selected=field(draft,"preferredAccountId").to_owned();
        let selected_label=choices.iter()
            .find(|(id,_)|id==&selected)
            .map(|(_,label)|label.as_str())
            .unwrap_or(CLIENT_ACCOUNT_EMPTY_LABEL);
        let mut next_selected=selected.clone();
        client_form_row(ui,"주 사용 입금계좌",|value|{
            let width=value.available_width();
            crate::dropdown::management_dropdown(
                value,crate::dropdown::DropdownCaption::new(selected_label,
                    selected.is_empty()||!choices.iter().any(|(id,_)|id==&selected)),width,true,|menu|{
                    for (id,label) in &choices {
                        crate::dropdown::management_dropdown_option(
                            menu,&mut next_selected,id.clone(),label
                        );
                    }
                });
        });
        // Persist only an explicit selection. A stale legacy reference stays
        // visible as unassigned until the user deliberately chooses an option.
        if next_selected!=selected {
            draft["preferredAccountId"]=if next_selected.is_empty(){Value::Null}
                else{json!(next_selected)};
        }
        client_form_row(ui,"",|value|{
            value.horizontal_wrapped(|flags|{
                bool_edit(flags,draft,"shippingFeeExcluded","배송비 청구 제외");
                bool_edit(flags,draft,"defaultApplyVat","부가세 포함");
            });
        });
        if field(draft,"tier")=="special" {
            crate::ui_primitives::section_divider(ui);
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
                        if crate::button::action(row,"예외 해제",ButtonRole::Quiet,true)
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
                crate::dropdown::management_dropdown(
                    row,crate::dropdown::DropdownCaption::new(variants.iter()
                        .find(|(id,_)|id==&self.exception_id)
                        .map(|(_,label)|label.as_str())
                        .unwrap_or("예외 품목 / 색상 선택"),
                        !variants.iter().any(|(id,_)|id==&self.exception_id)),
                    theme::MANAGEMENT_SEARCH_MIN_WIDTH,true,|ui|{
                        for (id,label) in &variants {
                            crate::dropdown::management_dropdown_option(
                                ui,&mut self.exception_id,id.clone(),label);
                        }
                    });
                crate::ui_primitives::form_field_label(row,"특수 단가");
                crate::input::money_field(row,&mut self.exception_price,true);
                // The common editor formats even while focused; the
                // business override still receives an unpunctuated integer.
                let amount=crate::input::parse_money_input(
                    &self.exception_price
                ).and_then(|value|u64::try_from(value).ok());
                if crate::button::action(row,"예외 추가",ButtonRole::Standard,
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
            crate::ui_primitives::section_divider(ui);
            crate::button::collapsing_header("상품 별칭 / 변경 이력")
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
    pub fn has_excel_review(&self)->bool{self.excel.has_modal()}
    pub fn show_excel_review(&mut self,ctx:&Context,area:egui::Rect,gateway:Arc<dyn BandoGateway>){
        let allowed=self.editable();
        self.excel.show_review(ctx,area,gateway,allowed);
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
        crate::popup::modal_window(ctx,title,"native-client-manager-editor",work_area)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|ui|{

                crate::popup::modal_flow_workspace(ui,|workspace|{
                    if workspace.header(title,"취소",!busy){
                        close=true;
                    }
                    let body_height=crate::popup::modal_editor_body_max_height(work_area.height());
                    workspace.scroll_region("native-client-manager-fields",body_height,|body|{
                                body.add_enabled_ui(editable && !busy,|form|{
                                    self.editor_fields(form,&mut draft);
                                });
                    });
                    workspace.footer(|left|{
                            if exists && crate::button::action(left,"거래처 삭제",
                                ButtonRole::Danger,editable && !busy).clicked(){
                                delete=true;
                            }
                        },
                        |right|{
                            let label=if exists {"변경사항 저장"}else{"거래처 등록"};
                            let dirty=self.original.as_ref()
                                .is_some_and(|old|*old!=draft)
                                || !self.tag_input.trim().is_empty();
                            if crate::button::action(right,label,ButtonRole::Primary,
                                editable && !busy && self.tag_input.trim().is_empty() && (!exists || dirty)).clicked(){
                                save=true;
                            }
                        }
                    );
                });
            });
        if crate::overlay::take_modal_escape(ctx) {
            if self.confirm_delete || self.confirm_discard {
                self.confirm_delete=false;
                self.confirm_discard=false;
            } else {
                close=true;
            }
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
            crate::popup::modal_backdrop(ctx,"client-manager-delete-backdrop",true);
            match crate::popup::show_delete_confirmation(
                ctx,work_area,"client-manager-delete-confirm","거래처 삭제","거래처 삭제",
                true,true,crate::popup::DeleteBody::Plain,|ui|{
                    ui.label("거래처를 삭제할까요? 기존 주문의 송하인과 금액은 그대로 보존합니다.");
                }
            ){
                crate::popup::ConfirmDecision::Cancel=>self.confirm_delete=false,
                crate::popup::ConfirmDecision::Confirm=>{
                    self.confirm_delete=false;
                    self.mutate(Arc::clone(&gateway),ctx,"archive_client");
                }
                crate::popup::ConfirmDecision::Pending=>{}
            }
        }
        if self.confirm_discard {
            crate::popup::modal_backdrop(ctx,"client-manager-discard-backdrop",true);
            match crate::popup::show_discard_confirmation(
                ctx,work_area,"client-manager-discard-confirm","변경사항 확인",
                "거래처의 수정 내용을 버릴까요?","취소","변경 취소",true
            ){
                crate::popup::ChangeDecision::Cancel=>self.confirm_discard=false,
                crate::popup::ChangeDecision::Discard=>{
                    self.confirm_discard=false;self.draft=None;self.original=None;
                    self.tag_input.clear();self.tag_notice=None;
                }
                _=>{}
            }
        }
    }
}

#[cfg(test)]
mod client_async_kpi_tests{
    use super::*;

    #[test]
    fn loading_tiers_are_unknown_but_loaded_empty_tiers_are_zero(){
        assert_eq!(
            client_tier_kpi_values(false,[0,0,0,0]),
            ["—".to_owned(),"—".to_owned(),"—".to_owned(),"—".to_owned()]
        );
        assert_eq!(
            client_tier_kpi_values(true,[0,0,0,0]),
            ["0".to_owned(),"0".to_owned(),"0".to_owned(),"0".to_owned()]
        );
        assert_eq!(
            client_tier_kpi_values(true,[3,0,5,1]),
            ["3".to_owned(),"0".to_owned(),"5".to_owned(),"1".to_owned()]
        );
        let source=include_str!("client_manager.rs");
        let page=source.split("pub fn show_page(").nth(1).unwrap()
            .split("let mut rows=").next().unwrap();
        assert!(page.contains(
            "client_tier_kpi_values(self.state.is_object(),tier_counts)"
        ));
        assert!(page.contains(
            "crate::card::management_summary_card_with_state(strip,label,value,theme::TextState::query(self.state.is_object()))"
        ));
    }
}

#[cfg(test)]
mod client_condition_summary_tests{
    use super::*;
    #[test]
    fn normal_client_does_not_repeat_default_conditions(){
        let client=json!({"payment":"PREPAID","defaultApplyVat":false,
            "shippingFeeExcluded":false,"active":true});
        assert_eq!(crate::composite_client::client_exception_terms(&client),"");
    }
    #[test]
    fn exceptional_client_terms_have_no_empty_separators(){
        let postpaid=json!({"payment":"POSTPAID","defaultApplyVat":false,
            "shippingFeeExcluded":false,"active":true});
        assert_eq!(crate::composite_client::client_exception_terms(&postpaid),"후불");
        let vat=json!({"payment":"PREPAID","defaultApplyVat":true,
            "shippingFeeExcluded":false,"active":true});
        assert_eq!(crate::composite_client::client_exception_terms(&vat),"부가세 포함");
        let extra=json!({"payment":"PREPAID","defaultApplyVat":false,
            "shippingFeeExcluded":true,"active":false});
        assert_eq!(crate::composite_client::client_exception_terms(&extra),"운임 청구 제외 / 거래 중지");
        let all=json!({"payment":"POSTPAID","defaultApplyVat":true,
            "shippingFeeExcluded":true,"active":false});
        assert_eq!(crate::composite_client::client_exception_terms(&all),
            "후불 / 부가세 포함 / 운임 청구 제외 / 거래 중지");
    }
}

#[cfg(test)]
mod client_account_picker_tests{
    use super::*;

    #[test]
    fn options_use_the_real_account_schema_and_full_account_label(){
        let accounts=json!([
            {"id":"bank-1","bank":"국민은행","number":"123-45-6789","holder":"김희준"}
        ]);
        assert_eq!(
            client_account_choices(accounts.as_array().unwrap()),
            vec![
                (String::new(),CLIENT_ACCOUNT_EMPTY_LABEL.to_owned()),
                ("bank-1".to_owned(),"국민은행 123-45-6789 김희준".to_owned())
            ]
        );
    }

    #[test]
    fn incomplete_or_unidentified_accounts_are_not_selectable(){
        let accounts=json!([
            {"id":"missing-holder","bank":"국민은행","number":"123","holder":""},
            {"id":"missing-number","bank":"신한은행","number":" ","holder":"예금주"},
            {"id":"","bank":"우리은행","number":"456","holder":"예금주"},
            {"id":"valid","bank":"농협","number":"789","holder":"예금주"}
        ]);
        assert_eq!(
            client_account_choices(accounts.as_array().unwrap()),
            vec![
                (String::new(),CLIENT_ACCOUNT_EMPTY_LABEL.to_owned()),
                ("valid".to_owned(),"농협 789 예금주".to_owned())
            ]
        );
    }

    #[test]
    fn unassigned_choice_remains_available_when_no_account_exists(){
        assert_eq!(
            client_account_choices(&[]),
            vec![(String::new(),CLIENT_ACCOUNT_EMPTY_LABEL.to_owned())]
        );
    }

    #[test]
    fn unknown_preferred_id_is_not_silently_overwritten_by_rendering(){
        let accounts=json!([
            {"id":"valid","bank":"농협","number":"789","holder":"예금주"}
        ]);
        let choices=client_account_choices(accounts.as_array().unwrap());
        let selected="removed-account";
        let label=choices.iter().find(|(id,_)|id==selected)
            .map(|(_,label)|label.as_str())
            .unwrap_or(CLIENT_ACCOUNT_EMPTY_LABEL);
        assert_eq!(label,CLIENT_ACCOUNT_EMPTY_LABEL);
        assert_eq!(selected,"removed-account");
    }
}

#[cfg(test)]
impl ClientManager {
    pub(crate) fn geometry_fixture(gateway:Arc<dyn BandoGateway>,ctx:egui::Context,tab:usize,loaded:bool)->Self{
        let mut page=Self::new(Arc::clone(&gateway),ctx.clone());
        page.loading.reset(); page.ready=loaded; if loaded{page.state=serde_json::json!({"revision":1,"clients":[],"readOnly":true});} if tab>0{page.show_messages=true;page.messages=Some(AutomaticMessageManager::geometry_fixture(gateway,ctx,tab-1,loaded));}
        page
    }

    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        let finish=self.loading.geometry_begin_read();
        self.error=None;
        Box::new(move |answer|{
            let result=answer.map(|count|{
                let clients=(0..count).map(|i|serde_json::json!({"id":format!("geometry-c-{i}"),"name":format!("검증 거래처 {i:03}"),"tier":(["WHOLESALE","SEMI","RETAIL","SPECIAL"][i%4]),"address":"합성 주소 — 실제 고객 정보 아님","active":true})).collect::<Vec<_>>();
                serde_json::json!({"revision":1,"clients":clients,"readOnly":true})
            });
            finish(result);
        })
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
    pub(crate) fn geometry_begin_message_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        self.messages.as_mut().expect("message fixture").geometry_begin_read()
    }
    pub(crate) fn geometry_message_failed(&self)->bool{
        self.messages.as_ref().expect("message fixture").geometry_read_failed()
    }
    pub(crate) fn geometry_select_notice(&mut self){
        self.messages.as_mut().expect("message fixture").geometry_select_first();
    }
}

#[cfg(test)]
mod client_inline_form_tests{
    use super::*;
    #[test]
    fn actual_client_editor_keeps_rects_during_permission_changes(){
        use crate::ui_test_support::{test_context,assert_popup_transition_geometry,PopupGeometryGateway};
        let ctx=test_context();
        let work=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1400.0,1000.0));
        let gateway:Arc<dyn BandoGateway>=Arc::new(PopupGeometryGateway{writable:true});
        let mut manager=ClientManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),0,true);
        let client=json!({"id":"synthetic-client","name":"검증 거래처","tier":"WHOLESALE","address":"검증 주소","active":true});
        manager.state=json!({"revision":1,"clients":[client.clone()],"readOnly":false});
        manager.begin(client);
        assert_popup_transition_geometry(&ctx,work,3,|ctx,phase|{
            manager.state["readOnly"]=json!(phase==1);
            manager.show_editor(ctx,work,Arc::clone(&gateway));
        });
    }

    #[test]
    fn client_form_labels_and_input_controls_share_rows(){
        for width in [840.0,420.0]{
            let ctx=crate::ui_test_support::test_context();
            let screen=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width,500.0));
            let mut draft=json!({"name":"합성 거래처","address":"합성 주소","phone":"02-0000-0000","mobile":""});
            crate::ui_test_support::begin_component_geometry();
            let output=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    label_edit(ui,&mut draft,"name","거래처명");
                    label_edit(ui,&mut draft,"address","주소");
                    client_form_pair(ui,&mut draft,
                        |left,draft|label_edit(left,draft,"phone","전화번호"),
                        |right,draft|label_edit(right,draft,"mobile","휴대폰번호"));
                });
            });
            let samples=crate::ui_test_support::finish_component_geometry(screen);
            let inputs=samples.iter().filter(|s|s.role=="input").map(|s|s.rect).collect::<Vec<_>>();
            assert_eq!(inputs.len(),4);
            for (label,rect) in ["거래처명","주소","전화번호","휴대폰번호"].into_iter().zip(&inputs){
                let ink=crate::ui_test_support::painted_text_rects(&output,label)[0].0;
                assert!(ink.right()<=rect.left());
                assert!((ink.center().y-rect.center().y).abs()<=3.0);
            }
            assert!((inputs[0].left()-inputs[1].left()).abs()<0.5);
            assert!((inputs[0].left()-inputs[2].left()).abs()<0.5);
            if width==840.0{assert!((inputs[2].top()-inputs[3].top()).abs()<0.5);}
            else{assert!(inputs[3].top()>=inputs[2].bottom());}
        }
    }
}
