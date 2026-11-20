//! Native client manager: semantic parity with React ClientManager.
//! Layout uses shared management components; validation and persistence are
//! delegated to the Rust catalog client rules via BandoGateway.
use crate::{components, gateway::{BandoGateway,ReadOperation}, theme::{self,ButtonRole}, typography};
use eframe::egui::{self,Align,Context,Layout,Ui};
use serde_json::{json,Value};
use std::{sync::{mpsc::{self,Receiver},Arc},thread,time::{SystemTime,UNIX_EPOCH}};

fn field<'a>(client:&'a Value,key:&str)->&'a str {
    client.get(key).and_then(Value::as_str).unwrap_or("")
}
fn readable_tier(tier:&str)->&'static str {
    match tier {"wholesale"=>"도매","semi"=>"반도매",
        "retail"=>"소매","special"=>"특수",_=>"미지정"}
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
    if ui.checkbox(&mut selected,title).changed(){
        draft[key]=json!(selected);
    }
}
fn new_id()->String {
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|time|time.as_nanos()).unwrap_or(0);
    format!("egui-client-{}-{stamp}",std::process::id())
}
fn empty_client()->Value {
    json!({"id":new_id(),"name":"","displayName":"","shortNames":"",
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
    error:Option<String>,
    changed:bool,
    draft:Option<Value>,
    original:Option<Value>,
    confirm_delete:bool,
    confirm_discard:bool,
    exception_id:String,
    exception_price:String,
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
            searching:false,error:None,changed:false,
            draft:None,original:None,confirm_delete:false,confirm_discard:false,
            exception_id:String::new(),exception_price:String::new(),
        }
    }
    pub fn has_modal(&self)->bool {self.draft.is_some()}
    pub fn take_changed(&mut self)->bool {
        std::mem::take(&mut self.changed)
    }
    fn editable(&self)->bool {
        self.state.is_object() &&
        self.state.get("readOnly")!=Some(&Value::Bool(true))
    }
    fn dirty(&self)->bool {
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
                self.changed=true;
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

    fn list_search(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>,reserve:f32) {
        // The placeholder already states the search purpose; an extra
        // external '검색' label wastes the first column's left alignment.
        let width=(ui.available_width()-reserve).max(60.0)
            .min(theme::MANAGEMENT_CLIENT_SEARCH_MAX_WIDTH);
        if components::flexible_text_field(
            ui,&mut self.search,width,"상호 / 주소 / 전화번호 / 별칭 검색"
        ).changed(){
            self.query(gateway,ui.ctx().clone());
        }
    }

    pub fn show_page(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>) {
        self.poll(Arc::clone(&gateway),ui.ctx());
        ui.label(typography::page_heading("거래처 관리"));
        ui.add_space(theme::SPACE_12);
        components::management_tabs(ui,&[
            ("거래처 목록",true,true),
            ("자동 메시지 편집",false,false),
        ]);
        ui.add_space(theme::SPACE_12);
        // React client-list-panel: ONE bordered surface owns both the
        // searchable toolbar and the scrolling table, with no doubled
        // 16pt section padding around the rows.
        theme::management_list_frame().show(ui,|section|{
            section.spacing_mut().item_spacing.y=0.0;
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(
                    theme::SPACE_12 as i8,theme::SPACE_8 as i8
                ))
                .show(section,|tools|{
                    if tools.available_width() <
                        theme::MANAGEMENT_CLIENT_TOOLBAR_STACK_WIDTH {
                        // Keep the search usable with the Kakao split widened.
                        // Only the toolbar wraps, never the table columns.
                        tools.horizontal(|search|{
                            self.list_search(search,Arc::clone(&gateway),0.0);
                        });
                        tools.add_space(theme::SPACE_8);
                        self.list_toolbar_actions(tools);
                    }else{
                        tools.horizontal(|bar|{
                            self.list_search(bar,Arc::clone(&gateway),
                                theme::MANAGEMENT_TOOLBAR_ACTIONS_WIDTH);
                            self.list_toolbar_actions(bar);
                        });
                    }
                });
            components::soft_divider(section);
            // Do not inject unsolicited success/error text after actions.
            // Retain failure state so an unsuccessful save cannot close the
            // editor or be treated as a committed change.
            if !self.ready {
                theme::management_list_strip_frame().show(section,|strip|{
                    strip.label(typography::caption("거래처 명단을 불러오는 중입니다."));
                });
                return;
            }
            let rows=self.clients().iter()
                .filter(|row|field(row,"deletedAt").is_empty())
                .filter(|row|self.search.trim().is_empty() ||
                    (!self.searching && self.matches.iter()
                    .any(|id|id==field(row,"id"))))
                .cloned().collect::<Vec<_>>();
            if self.searching {
                section.label(typography::caption("거래처 검색 중…"));
            }
            egui::ScrollArea::both()
                .id_salt("client-management-table")
                .auto_shrink([false,false])
                .show(section,|table|{
                    table.set_min_width(theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH);
                    components::management_row(table,"client-management-header",[
                        ("거래처",""),("발송인 정보",""),("거래 조건","")
                    ],true,None);
                    for (index,client) in rows.iter().enumerate() {
                        let name=field(client,"name");
                        let display=field(client,"displayName");
                        let description=if display!=name {display} else {""};
                        let address=field(client,"address");
                        let contacts=[field(client,"phone"),field(client,"mobile")]
                            .into_iter().filter(|s|!s.trim().is_empty())
                            .collect::<Vec<_>>().join(" / ");
                        let contacts=if contacts.is_empty(){
                            "연락처 미등록".to_owned()
                        }else{contacts};
                        let conditions=[
                            if field(client,"payment")=="POSTPAID"{"후불"}else{"선입금"},
                            if client.get("defaultApplyVat").and_then(Value::as_bool)
                                .unwrap_or(false) {"부가세 포함"} else {"부가세 별도"},
                            if client.get("shippingFeeExcluded").and_then(Value::as_bool)
                                .unwrap_or(false) {"운임 청구 제외"} else {""},
                            if client.get("active")==Some(&Value::Bool(false))
                                {"거래 중지"}else{""}
                        ].into_iter().filter(|t|!t.is_empty())
                            .collect::<Vec<_>>().join(" / ");
                        if components::management_row_striped(table,field(client,"id"),[
                            (name,description),(address,&contacts),
                            (readable_tier(field(client,"tier")),&conditions)
                        ],Some(field(client,"tier")),index).clicked() {
                            self.begin(client.clone());
                        }
                    }
                    if rows.is_empty() && !self.searching {
                        table.add_space(theme::SPACE_12);
                        table.horizontal(|empty|{
                            empty.add_space(theme::SPACE_12);
                            empty.label(typography::caption(
                                if self.search.is_empty() {
                                    "등록된 거래처가 없습니다."
                                } else {"검색 결과가 없습니다."}
                            ));
                        });
                    }
                });
        });
    }
    fn editor_fields(&mut self,ui:&mut Ui,draft:&mut Value) {
        ui.label(typography::section_heading("거래처 식별"));
        ui.columns(2,|cols|{
            label_edit(&mut cols[0],draft,"name","거래처명");
            label_edit(&mut cols[1],draft,"displayName","송장 표시 상호");
        });
        ui.vertical(|ui|{
            ui.label(typography::field_label("검색 태그"));
            let mut tags=[
                field(draft,"shortNames"),field(draft,"identifiers")
            ].into_iter().filter(|v|!v.is_empty())
                .collect::<Vec<_>>().join(", ");
            if components::flexible_text_field(
                ui,&mut tags,ui.available_width(),""
            ).changed(){
                draft["shortNames"]=json!(tags);
                draft["identifiers"]=json!("");
            }
        });
        components::soft_divider(ui);
        ui.label(typography::section_heading("발송인 정보"));
        label_edit(ui,draft,"address","거래처 주소");
        ui.columns(2,|cols|{
            label_edit(&mut cols[0],draft,"phone","전화번호");
            label_edit(&mut cols[1],draft,"mobile","휴대폰번호");
        });
        components::soft_divider(ui);
        ui.label(typography::section_heading("거래 조건"));
        ui.columns(2,|cols|{
            cols[0].label(typography::field_label("가격 등급"));
            let mut tier=field(draft,"tier").to_owned();
            egui::ComboBox::from_id_salt("client-manager-tier")
                .width(cols[0].available_width())
                .selected_text(readable_tier(&tier))
                .show_ui(&mut cols[0],|ui|{
                    for (key,label) in [
                        ("wholesale","도매"),("semi","반도매"),
                        ("retail","소매"),("special","특수")
                    ] {
                        ui.selectable_value(&mut tier,key.to_owned(),label);
                    }
                });
            draft["tier"]=json!(tier);
            cols[1].label(typography::field_label("결제 방식"));
            let mut payment=field(draft,"payment").to_owned();
            egui::ComboBox::from_id_salt("client-manager-payment")
                .width(cols[1].available_width())
                .selected_text(if payment=="POSTPAID"{"후불"}else{"선입금"})
                .show_ui(&mut cols[1],|ui|{
                    ui.selectable_value(&mut payment,"PREPAID".into(),"선입금");
                    ui.selectable_value(&mut payment,"POSTPAID".into(),"후불");
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
            egui::ComboBox::from_id_salt("client-manager-account")
                .width(ui.available_width())
                .selected_text(selected_label)
                .show_ui(ui,|ui|{
                    ui.selectable_value(&mut selected,String::new(),"미지정");
                    for account in &accounts {
                        let id=field(account,"id").to_owned();
                        let label=field(account,"name");
                        ui.selectable_value(&mut selected,id,label);
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
                        row.label(typography::body(&format!("{label} · {}원",price)));
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
                egui::ComboBox::from_id_salt("client-manager-exception-variant")
                    .selected_text(variants.iter()
                        .find(|(id,_)|id==&self.exception_id)
                        .map(|(_,label)|label.as_str())
                        .unwrap_or("예외 품목 / 색상 선택"))
                    .show_ui(row,|ui|{
                        for (id,label) in &variants {
                            ui.selectable_value(&mut self.exception_id,id.clone(),label);
                        }
                    });
                row.label(typography::field_label("특수 단가"));
                components::field(
                    row,&mut self.exception_price,components::FieldKind::Money,true
                );
                let amount=self.exception_price.parse::<u64>().ok();
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
            ui.collapsing("상품 별칭 / 변경 이력",|ui|{
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
                let body_height=theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT
                    .min((work_area.height()-theme::SPACE_24*4.0).max(100.0));
                egui::ScrollArea::vertical()
                    .id_salt("native-client-manager-fields")
                    .max_height(body_height)
                    .auto_shrink([false,false])
                    .show(ui,|body|{
                        body.add_enabled_ui(editable && !busy,|form|{
                            self.editor_fields(form,&mut draft);
                        });
                    });
                components::soft_divider(ui);
                ui.horizontal(|footer|{
                    if exists && theme::action(footer,"거래처 삭제",
                        ButtonRole::Danger,editable && !busy).clicked(){
                        delete=true;
                    }
                    footer.with_layout(Layout::right_to_left(Align::Center),|buttons|{
                        let label=if exists {"변경사항 저장"}else{"거래처 등록"};
                        let dirty=self.original.as_ref()
                            .is_some_and(|old|*old!=draft);
                        if theme::action(buttons,label,ButtonRole::Primary,
                            editable && !busy && (!exists || dirty)).clicked(){
                            save=true;
                        }
                    });
                });
            });
        if ctx.input(|input|input.key_pressed(egui::Key::Escape)) {
            if self.confirm_delete || self.confirm_discard {
                self.confirm_delete=false;
                self.confirm_discard=false;
            } else {
                close=true;
            }
        }
        self.draft=Some(draft);
        if save {self.mutate(Arc::clone(&gateway),ctx,"upsert_client");}
        if delete {self.confirm_delete=true;}
        if close {self.request_close();}
        if self.confirm_delete {
            components::modal_backdrop(ctx,"client-manager-delete-backdrop",true);
            components::decision_window(
                "거래처 삭제","client-manager-delete-confirm",work_area
            ).show(ctx,|ui|{
                let cancel=components::modal_header(ui,"거래처 삭제","취소",true);
                ui.label("거래처를 삭제할까요? 기존 주문의 송하인과 금액은 그대로 보존합니다.");
                let approve=theme::action(
                    ui,"거래처 삭제",ButtonRole::Danger,true
                ).clicked();
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
                let approve=theme::action(
                    ui,"변경 취소",ButtonRole::Danger,true
                ).clicked();
                if cancel {self.confirm_discard=false;}
                if approve {
                    self.confirm_discard=false;self.draft=None;self.original=None;
                }
            });
        }
    }
}
