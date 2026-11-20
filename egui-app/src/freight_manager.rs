//! Native freight management read-side. All amounts, active-rule versions and
//! combination validity come from the existing Tauri Rust business state and
//! rules::shipping_manager_view. No preview DB or hard-coded rate is treated as
//! operational freight. Write actions are a separate migration step.
use crate::{
    components,
    gateway::{BandoGateway, ReadOperation},
    theme, typography,
};
use eframe::egui::{self, Ui};
use std::collections::{BTreeMap, BTreeSet};
use serde_json::{json, Value};
use std::sync::{
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FreightTab {
    Groups,
    Rates,
    History,
}
impl FreightTab {
    const ALL: [Self; 3] = [Self::Groups, Self::Rates, Self::History];
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn active(row: &Value) -> bool {
    row.get("deletedAt").is_none_or(Value::is_null)
}
fn registered(row: &Value) -> bool {
    active(row)
        && row.get("active") != Some(&Value::Bool(false))
        && row.get("supersededBy").is_none_or(Value::is_null)
        && text(row, "catalog") != "DEMO"
        && !rows(&row["variants"]).iter().any(|v| text(v, "source") == "DEMO")
}
fn as_fee(value: &Value) -> String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(|| "미등록".to_owned())
}
fn group_name<'a>(groups: &'a [Value], id: &str) -> &'a str {
    groups.iter().find(|g| active(g) && text(g, "id") == id)
        .map(|g| text(g, "name")).unwrap_or("그룹 미지정")
}


struct RateEditor {
    id: String,
    name: String,
    original: Value,
    max_bundle: Option<i64>,
    fees: BTreeMap<i64, String>,
    notes: String,
    add_count: Option<i64>,
    error: Option<String>,
    ask_discard: bool,
}
impl RateEditor {
    fn new(id:&str,name:&str,rule:&Value)->Self {
        let fees=(1..=6).filter_map(|count|{
            rule["fees"].get(count.to_string()).and_then(Value::as_i64)
                .map(|value|(count,value.to_string()))
        }).collect();
        Self{
            id:id.to_owned(),name:name.to_owned(),original:rule.clone(),
            max_bundle:rule["maxBundle"].as_i64(),
            fees,notes:text(rule,"notes").to_owned(),
            add_count:None,error:None,ask_discard:false
        }
    }
    fn dirty(&self)->bool{
        if self.original["maxBundle"].as_i64()!=self.max_bundle
            || text(&self.original,"notes")!=self.notes {return true;}
        let old=(1..=6).filter_map(|count|{
            self.original["fees"].get(count.to_string()).and_then(Value::as_i64)
                .map(|value|(count,Some(value)))
        }).collect::<BTreeMap<_,_>>();
        let new=self.fees.iter().map(|(key,value)|{
            (*key,value.trim().parse::<i64>().ok())
        }).collect::<BTreeMap<_,_>>();
        old!=new
    }
    fn draft(&self)->Result<Value,String>{
        let mut rule=self.original.clone();
        rule["maxBundle"]=self.max_bundle.map_or(Value::Null,|v|json!(v));
        rule["notes"]=json!(self.notes);
        let mut fees=rule["fees"].as_object().cloned().unwrap_or_default();
        for count in 1..=6 {fees.remove(&count.to_string());}
        for (count,amount) in &self.fees {
            let raw=amount.trim();
            let value=if raw.is_empty() {
                Value::Null
            }else{
                let amount=raw.parse::<i64>()
                    .map_err(|_|"올바른 운임을 입력하세요.".to_owned())?;
                if !(0..=9_007_199_254_740_991).contains(&amount){
                    return Err("올바른 운임을 입력하세요.".into());
                }
                json!(amount)
            };
            fees.insert(count.to_string(),value);
        }
        rule["fees"]=Value::Object(fees);
        Ok(rule)
    }
}

pub struct FreightManager {
    tab: FreightTab,
    gateway: Arc<dyn BandoGateway>,
    write_receiver: Option<Receiver<Result<Value,String>>>,
    write_action: Option<String>,
    notice: Option<String>,
    pending: BTreeMap<String,String>,
    selected: BTreeSet<String>,
    bulk_group: String,
    new_name: String,
    rename_target: Option<(String,String)>,
    delete_target: Option<String>,
    rate_editor: Option<RateEditor>,
    basis_target: Option<Option<String>>,
    exception_target: Option<(String,String)>,
    receiver: Option<Receiver<Result<(Value, Value), String>>>,
    state: Option<Value>,
    projection: Option<Value>,
    error: Option<String>,
    search: String,
    group_filter: String,
}
impl FreightManager {
    pub fn new(gateway: Arc<dyn BandoGateway>, ctx: egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let loader=Arc::clone(&gateway);
        std::thread::spawn(move || {
            let outcome = (|| {
                let state = loader.load_business_state()?;
                let shipping = state.get("shipping")
                    .filter(|value| value.is_object())
                    .ok_or("운영 DB에서 운임 기준을 찾을 수 없습니다.")?;
                let projection = loader.execute(
                    ReadOperation::ShippingManagerView,
                    json!({"state":state,"shipping":shipping}),
                )?;
                Ok((state, projection))
            })();
            let _ = sender.send(outcome);
            ctx.request_repaint();
        });
        Self {
            tab: FreightTab::Groups,
            gateway,
            write_receiver: None,
            write_action: None,
            notice: None,
            pending: BTreeMap::new(),
            selected: BTreeSet::new(),
            bulk_group: String::new(),
            new_name: String::new(),
            rename_target: None,
            delete_target: None,
            rate_editor: None,
            basis_target: None,
            exception_target: None,
            receiver: Some(receiver),
            state: None,
            projection: None,
            error: None,
            search: String::new(),
            group_filter: String::new(),
        }
    }

    fn poll(&mut self) {
        if let Some(receiver)=self.receiver.as_ref(){
            match receiver.try_recv() {
                Ok(Ok((state, projection))) => {
                    self.state = Some(state);
                    self.projection = Some(projection);
                    self.receiver = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("운임 기준을 불러오는 작업이 중단되었습니다.".into());
                    self.receiver = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(receiver)=self.write_receiver.as_ref(){
            match receiver.try_recv(){
                Ok(Ok(result))=>{
                    if result["state"].is_object() && result["projection"].is_object(){
                        self.state=Some(result["state"].clone());
                        self.projection=Some(result["projection"].clone());
                        self.error=None;
                        let action=self.write_action.take().unwrap_or_default();
                        self.notice=Some(match action.as_str(){
                            "create_group"=>"새 배송 그룹을 등록했습니다.",
                            "rename_group"=>"배송 그룹명을 변경했습니다.",
                            "delete_group"=>"그룹을 삭제하고 과거 운임 기록을 보존했습니다.",
                            "assign_products"=>"배송 그룹 지정을 저장했습니다.",
                            "save_group_rule"=>"운임표를 저장했습니다. 변경 전 기준은 이력으로 보존됩니다.",
                            "change_basis"=>"운임 기준을 변경했습니다.",
                            "revise_combination"=>"새 예외 운임을 저장하고 이전 값을 이력으로 보존했습니다.",
                            _=>"운임 변경사항을 저장했습니다."
                        }.into());
                        match action.as_str(){
                            "create_group"=>self.new_name.clear(),
                            "rename_group"=>self.rename_target=None,
                            "delete_group"=>self.delete_target=None,
                            "assign_products"=>{
                                self.pending.clear();
                                self.selected.clear();
                            }
                            "save_group_rule"=>self.rate_editor=None,
                            "change_basis"=>self.basis_target=None,
                            "revise_combination"=>self.exception_target=None,
                            _=>{}
                        }
                    }else{
                        self.error=Some("운임 저장 후 최신 상태를 확인하지 못했습니다.".into());
                    }
                    self.write_receiver=None;
                }
                Ok(Err(error))=>{
                    self.error=Some(error);
                    self.write_receiver=None;
                    self.write_action=None;
                }
                Err(TryRecvError::Disconnected)=>{
                    self.error=Some("운임 저장 작업이 중단되었습니다. 데이터 재조회를 확인하세요.".into());
                    self.write_receiver=None;
                    self.write_action=None;
                }
                Err(TryRecvError::Empty)=>{}
            }
        }
    }

    fn write(&mut self, action: String, data: Value, ctx:egui::Context) {
        if self.write_receiver.is_some() || !self.gateway.shipping_write_enabled(){
            return;
        }
        let Some(revision)=self.state.as_ref()
            .and_then(|s|s["revision"].as_u64()) else {
                self.error=Some("운영 DB 리비전을 확인하지 못했습니다.".into());
                return;
            };
        let gateway=Arc::clone(&self.gateway);
        let requested=action.clone();
        let (sender,receiver)=mpsc::channel();
        self.write_action=Some(action);
        self.notice=None;
        self.error=None;
        self.write_receiver=Some(receiver);
        std::thread::spawn(move||{
            let result=gateway.mutate_shipping(revision,&requested,data);
            let _=sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn show_page(&mut self, ui: &mut Ui) {
        self.poll();
        ui.label(typography::page_heading("운임 관리"));
        ui.add_space(theme::SPACE_12);
        if let Some(index) = components::management_tabs(ui, &[
            ("배송 그룹 편성", self.tab == FreightTab::Groups, true),
            ("그룹 운임표", self.tab == FreightTab::Rates, true),
            ("조합 / 기준 이력", self.tab == FreightTab::History, true),
        ]) {
            if self.rate_editor.is_none() && self.rename_target.is_none()
                && self.delete_target.is_none() && self.basis_target.is_none()
                && self.exception_target.is_none() && self.write_receiver.is_none() {
                self.tab = FreightTab::ALL[index];
            }
        }
        ui.add_space(theme::SPACE_12);
        if let Some(error) = &self.error {
            ui.label(typography::status(error, theme::DANGER));
            ui.add_space(theme::SPACE_8);
        }else if let Some(notice)=&self.notice{
            ui.label(typography::status(notice,theme::SUCCESS));
            ui.add_space(theme::SPACE_8);
        }
        let (Some(state), Some(projection)) = (self.state.as_ref(), self.projection.as_ref())
        else {
            if self.receiver.is_some() {
                ui.label(typography::caption("운임 기준을 불러오는 중입니다."));
            }
            return;
        };
        let shipping = &state["shipping"];
        let groups: Vec<Value> = rows(&shipping["groups"])
            .iter().filter(|g| active(g)).cloned().collect();
        let products: Vec<&Value> = rows(&state["products"])
            .iter().filter(|p| registered(p)).collect();
        let assigned = products.iter().filter(|p| {
            let id = text(p, "shippingGroupId");
            !id.is_empty() && groups.iter().any(|g| text(g, "id") == id)
        }).count();
        ui.horizontal_wrapped(|bar| {
            summary(bar, "등록 상품", products.len());
            summary(bar, "그룹 지정 완료", assigned);
            summary(bar, "그룹 미지정", products.len() - assigned);
            summary(bar, "배송 그룹", groups.len());
            bar.label(typography::caption(format!(
                "운임 기준 {}", shipping["epoch"].as_i64().unwrap_or(1)
            )));
        });
        ui.add_space(theme::SPACE_12);
        let enabled=self.gateway.shipping_write_enabled()&&self.write_receiver.is_none();
        let mut action:Option<(String,Value)>=None;
        egui::ScrollArea::vertical()
            .id_salt(("freight-management", self.tab as u8))
            .auto_shrink([false, false])
            .show(ui, |scroll| match self.tab {
                FreightTab::Groups => Self::show_groups(
                    scroll, &groups, &products, &mut self.search, &mut self.group_filter,
                    &mut self.pending, &mut self.selected,
                    &mut self.bulk_group, &mut self.new_name,
                    &mut self.rename_target, &mut self.delete_target, enabled, &mut action
                ),
                FreightTab::Rates => Self::show_rates(
                    scroll,&groups,projection,
                    &mut self.rate_editor,&mut self.basis_target,enabled
                ),
                FreightTab::History => Self::show_history(
                    scroll,shipping,projection,&mut self.exception_target,enabled
                ),
            });
        if self.tab==FreightTab::Groups {
            let area=ui.max_rect();
            let ctx=ui.ctx();
            let mut close_rename=false;
            let mut close_delete=false;
            if let Some((id,name))=self.rename_target.as_mut(){
                components::modal_backdrop(ctx,"freight-rename-backdrop",false);
                components::decision_window("배송 그룹명","freight-rename",area)
                    .show(ctx,|modal|{
                        components::field(modal,name,components::FieldKind::Text,enabled);
                        modal.add_space(theme::SPACE_12);
                        modal.horizontal(|controls|{
                            if theme::action(controls,"취소",theme::ButtonRole::Quiet,enabled)
                                .clicked(){close_rename=true;}
                            if theme::action(controls,"이름 수정",theme::ButtonRole::Primary,
                                enabled&&!name.trim().is_empty()).clicked(){
                                action=Some(("rename_group".into(),json!({
                                    "groupId":id,"name":name.trim()
                                })));
                            }
                        });
                    });
            }else if let Some(id)=self.delete_target.as_ref(){
                let linked=products.iter().filter(|p|text(p,"shippingGroupId")==id).count();
                let name=group_name(&groups,id);
                components::modal_backdrop(ctx,"freight-delete-backdrop",false);
                components::decision_window("배송 그룹 삭제","freight-delete",area)
                    .show(ctx,|modal|{
                        modal.label(typography::body(name));
                        modal.label(typography::body(if linked>0 {
                            "사용 중인 상품을 미지정으로 변경한 후 그룹을 삭제합니다."
                        }else{
                            "연결된 현재 상품이 없습니다. 과거 운임과 주문 기록은 보존합니다."
                        }));
                        modal.add_space(theme::SPACE_12);
                        modal.horizontal(|controls|{
                            if theme::action(controls,"취소",theme::ButtonRole::Quiet,enabled)
                                .clicked(){close_delete=true;}
                            if theme::action(controls,if linked>0{
                                "상품 연결을 미지정으로 변경하고 삭제"
                            }else{"그룹 삭제"},theme::ButtonRole::Danger,enabled)
                                .clicked(){
                                action=Some(("delete_group".into(),json!({
                                    "groupId":id,"detach":linked>0
                                })));
                            }
                        });
                    });
            }
            if close_rename{self.rename_target=None;}
            if close_delete{self.delete_target=None;}
        }
        if self.tab==FreightTab::Rates {
            let area=ui.max_rect();
            let ctx=ui.ctx();
            let mut close_editor=false;
            let mut close_basis=false;
            if let Some(editor)=self.rate_editor.as_mut(){
                components::modal_backdrop(ctx,"freight-rate-backdrop",false);
                components::modal_window("배송 그룹 운임 수정","freight-rate-modal",area)
                    .resizable(false).show(ctx,|modal|{
                        egui::ScrollArea::vertical()
                            .id_salt("freight-rate-editor-scroll")
                            .max_height(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
                            .show(modal,|body|{
                                Self::rate_dialog(
                                    body,editor,enabled,&mut action,&mut close_editor
                                );
                            });
                    });
                if editor.ask_discard {
                    components::modal_backdrop(ctx,"freight-discard-backdrop",true);
                    components::decision_window(
                        "저장하지 않은 운임","freight-discard-confirm",area
                    ).show(ctx,|dialog|{
                        dialog.label(typography::body(
                            "운임 수정사항을 저장하지 않고 닫을까요?"
                        ));
                        dialog.horizontal(|bar|{
                            if theme::action(bar,"계속 수정",
                                theme::ButtonRole::Standard,enabled).clicked(){
                                editor.ask_discard=false;
                            }
                            if theme::action(bar,"저장 안 함",
                                theme::ButtonRole::Danger,enabled).clicked(){
                                close_editor=true;
                            }
                        });
                    });
                }
            }else if let Some(target)=self.basis_target.as_ref(){
                let name=target.as_deref().map(|id|group_name(&groups,id));
                components::modal_backdrop(ctx,"freight-basis-backdrop",false);
                components::decision_window(
                    "운임 기준 변경","freight-basis-confirm",area
                ).show(ctx,|dialog|{
                    let warning=match name {
                        None=>"전체 운임 기준을 변경하면 이전 운임의 자동 추천을 중단합니다. 새 운임을 다시 확인해 등록해야 합니다.",
                        Some(_)=>"선택한 그룹의 이전 운임 추천을 중단하고 새 운임 기준으로 변경합니다."
                    };
                    dialog.label(typography::body(warning));
                    dialog.horizontal(|bar|{
                        if theme::action(bar,"취소",
                            theme::ButtonRole::Standard,enabled).clicked(){
                            close_basis=true;
                        }
                        if theme::action(bar,"기준 변경",
                            theme::ButtonRole::Danger,enabled).clicked(){
                            action=Some(("change_basis".into(),match target{
                                Some(id)=>json!({"groupId":id}),
                                None=>json!({})
                            }));
                        }
                    });
                });
            }
            if close_editor{self.rate_editor=None;}
            if close_basis{self.basis_target=None;}
        }
        if self.tab==FreightTab::History {
            let mut close=false;
            if let Some((id,amount))=self.exception_target.as_mut(){
                let ctx=ui.ctx();
                components::modal_backdrop(ctx,"freight-exception-backdrop",false);
                components::decision_window(
                    "새로 확인된 청구 운임 (원)","freight-exception",ui.max_rect()
                ).show(ctx,|dialog|{
                    components::field(
                        dialog,amount,components::FieldKind::Money,enabled
                    );
                    dialog.add_space(theme::SPACE_12);
                    dialog.horizontal(|bar|{
                        if theme::action(bar,"취소",
                            theme::ButtonRole::Standard,enabled).clicked(){
                            close=true;
                        }
                        if theme::action(bar,"예외 운임 변경",
                            theme::ButtonRole::Primary,enabled).clicked(){
                            match amount.trim().parse::<i64>(){
                                Ok(value) if (0..=9_007_199_254_740_991).contains(&value)=>{
                                    action=Some(("revise_combination".into(),json!({
                                        "combinationId":id,"charge":value
                                    })));
                                }
                                _=>self.error=Some("올바른 운임을 입력하세요.".into())
                            }
                        }
                    });
                });
            }
            if close{self.exception_target=None;}
        }
        if let Some((kind,data))=action {
            self.write(kind,data,ui.ctx().clone());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_groups(
        ui: &mut Ui, groups: &[Value], products: &[&Value],
        search: &mut String, group_filter: &mut String,
        pending: &mut BTreeMap<String,String>, selected: &mut BTreeSet<String>,
        bulk_group: &mut String, new_name: &mut String,
        rename_target: &mut Option<(String,String)>,
        delete_target: &mut Option<String>, enabled:bool,
        action:&mut Option<(String,Value)>,
    ) {
        theme::section_frame().show(ui, |section| {
            section.horizontal(|tools|{
                tools.label(typography::field_label("새 배송 그룹명"));
                components::flexible_text_field(
                    tools,new_name,theme::MANAGEMENT_SEARCH_MIN_WIDTH,"새 배송 그룹명"
                );
                if theme::action(tools,"배송 그룹 추가",theme::ButtonRole::Primary,
                    enabled&&!new_name.trim().is_empty()).clicked(){
                    *action=Some(("create_group".into(),json!({
                        "name":new_name.trim()
                    })));
                }
            });
            components::soft_divider(section);
            for group in groups {
                section.horizontal(|line|{
                    line.label(typography::body(text(group,"name")));
                    let count=products.iter()
                        .filter(|p|text(p,"shippingGroupId")==text(group,"id"))
                        .count();
                    line.label(typography::caption(format!("{count}개 상품")));
                    if theme::action(line,"이름 수정",theme::ButtonRole::Standard,enabled)
                        .clicked() {
                        *rename_target=Some((
                            text(group,"id").into(),text(group,"name").into()
                        ));
                    }
                    if theme::action(line,"삭제",theme::ButtonRole::Danger,enabled)
                        .clicked(){
                        *delete_target=Some(text(group,"id").into());
                    }
                });
            }
        });
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui, |section| {
            section.horizontal_wrapped(|tools| {
                tools.label(typography::field_label("검색"));
                components::flexible_text_field(
                    tools,search,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                    "모델명 / 상품명 / 카테고리 검색"
                );
                egui::ComboBox::from_id_salt("freight-group-filter")
                    .selected_text(if group_filter.is_empty() {
                        "전체 상품"
                    } else if group_filter.as_str() == "none" {
                        "그룹 미지정"
                    } else { group_name(groups,group_filter) })
                    .show_ui(tools, |combo| {
                        combo.selectable_value(group_filter,String::new(),"전체 상품");
                        combo.selectable_value(group_filter,"none".into(),"그룹 미지정");
                        for group in groups {
                            combo.selectable_value(
                                group_filter,text(group,"id").into(),text(group,"name")
                            );
                        }
                    });
            });
            components::soft_divider(section);
            let needle=search.trim().to_lowercase();
            let filtered:Vec<&Value>=products.iter().copied().filter(|p|{
                (needle.is_empty()||[text(p,"code"),text(p,"name"),
                    text(p,"subtitle"),text(p,"category")]
                    .iter().any(|s|s.to_lowercase().contains(&needle)))
                && (group_filter.is_empty()
                    || (group_filter.as_str()=="none" && !groups.iter().any(|g|
                        text(g,"id")==text(p,"shippingGroupId")))
                    || text(p,"shippingGroupId")==group_filter.as_str())
            }).collect();
            section.horizontal_wrapped(|tools|{
                let mut all=!filtered.is_empty()
                    && filtered.iter().all(|p|selected.contains(text(p,"id")));
                if tools.checkbox(&mut all,"").changed(){
                    selected.clear();
                    if all {
                        selected.extend(filtered.iter()
                            .map(|p|text(p,"id").to_owned()));
                    }
                }
                tools.label(typography::field_label(format!(
                    "{}개 선택",selected.len()
                )));
                egui::ComboBox::from_id_salt("freight-bulk-assign")
                    .selected_text(if bulk_group.is_empty(){
                        "미지정으로 변경"
                    }else{group_name(groups,bulk_group)})
                    .show_ui(tools,|combo|{
                        combo.selectable_value(bulk_group,String::new(),"미지정으로 변경");
                        for group in groups{
                            combo.selectable_value(
                                bulk_group,text(group,"id").into(),text(group,"name")
                            );
                        }
                    });
                if theme::action(tools,"선택 상품에 적용",
                    theme::ButtonRole::Standard,enabled&&!selected.is_empty()
                ).clicked(){
                    for id in selected.iter(){
                        pending.insert(id.clone(),bulk_group.clone());
                    }
                }
            });
            components::soft_divider(section);
            egui::ScrollArea::horizontal()
                .id_salt("freight-assignment-rows")
                .auto_shrink([false,false])
                .show(section,|table|{
                    table.set_min_width(theme::MANAGEMENT_TABLE_MIN_WIDTH);
                    egui::Grid::new("freight-group-assignments-grid")
                        .striped(true).spacing([theme::SPACE_24,theme::SPACE_8])
                        .show(table,|grid|{
                            grid.label(typography::table_header("선택"));
                            grid.label(typography::table_header("상품명 / 상품설명"));
                            grid.label(typography::table_header("카테고리"));
                            grid.label(typography::table_header("배송 그룹"));
                            grid.label(typography::table_header("지정 상태"));
                            grid.end_row();
                            for product in &filtered{
                                let id=text(product,"id");
                                let committed=text(product,"shippingGroupId");
                                let mut checked=selected.contains(id);
                                if grid.checkbox(&mut checked,"").changed(){
                                    if checked{selected.insert(id.into());}
                                    else{selected.remove(id);}
                                }
                                grid.vertical(|cell|{
                                    cell.label(typography::table_cell(text(product,"name")));
                                    let description=text(product,"subtitle");
                                    if !description.is_empty(){
                                        cell.label(typography::caption(description));
                                    }
                                });
                                grid.label(typography::table_cell(text(product,"category")));
                                let mut current=pending.get(id).cloned()
                                    .unwrap_or_else(||committed.into());
                                grid.add_enabled_ui(enabled,|control|{
                                    egui::ComboBox::from_id_salt(("freight-row-group",id))
                                        .selected_text(if current.is_empty(){
                                            "그룹 선택"
                                        }else{group_name(groups,&current)})
                                        .show_ui(control,|choice|{
                                            choice.selectable_value(
                                                &mut current,String::new(),"그룹 선택"
                                            );
                                            for group in groups{
                                                choice.selectable_value(
                                                    &mut current,text(group,"id").into(),
                                                    text(group,"name")
                                                );
                                            }
                                        });
                                });
                                if current!=committed {
                                    pending.insert(id.into(),current);
                                }else{pending.remove(id);}
                                let changed=pending.contains_key(id);
                                grid.label(typography::table_cell(
                                    if changed{"변경 예정"}
                                    else if committed.is_empty(){"미지정"}
                                    else{"지정 완료"}
                                ));
                                grid.end_row();
                            }
                        });
                    if filtered.is_empty(){
                        table.label(typography::caption("검색 조건에 맞는 상품이 없습니다."));
                    }
                });
            let mut assignments=serde_json::Map::new();
            for (id,group) in pending.iter(){
                if let Some(product)=products.iter().find(|p|text(p,"id")==id.as_str()){
                    if text(product,"shippingGroupId")!=group.as_str(){
                        assignments.insert(id.clone(),json!(group));
                    }
                }
            }
            components::soft_divider(section);
            section.horizontal(|actions|{
                actions.label(typography::caption(format!(
                    "{}개 상품의 배송 그룹을 변경합니다.",assignments.len()
                )));
                if theme::action(actions,"미저장 변경 초기화",
                    theme::ButtonRole::Standard,enabled&&!assignments.is_empty()
                ).clicked(){pending.clear();}
                if theme::action(actions,"그룹 지정 저장",
                    theme::ButtonRole::Primary,enabled&&!assignments.is_empty()
                ).clicked(){
                    *action=Some(("assign_products".into(),json!({
                        "assignments":assignments
                    })));
                }
            });
        });
    }

    fn show_rates(
        ui: &mut Ui, groups: &[Value], projection: &Value,
        editor: &mut Option<RateEditor>,
        basis: &mut Option<Option<String>>,enabled:bool,
    ) {
        ui.horizontal(|bar|{
            bar.label(typography::section_heading("그룹 운임표"));
            if theme::action(bar,"전체 운임 기준 변경",theme::ButtonRole::Danger,enabled)
                .clicked(){*basis=Some(None);}
        });
        ui.add_space(theme::SPACE_12);
        for group in groups {
            let name = text(group, "name");
            let id = text(group, "id");
            let rule = projection["editableRules"].get(id)
                .unwrap_or(&group["current"]);
            theme::section_frame().show(ui, |section| {
                section.horizontal(|bar|{
                    bar.label(typography::section_heading(name));
                    if theme::action(bar,"운임 수정",
                        theme::ButtonRole::Standard,enabled).clicked(){
                        *editor=Some(RateEditor::new(id,name,rule));
                    }
                    if theme::action(bar,"기준 변경",
                        theme::ButtonRole::Danger,enabled).clicked(){
                        *basis=Some(Some(id.to_owned()));
                    }
                });
                section.add_space(theme::SPACE_8);
                let max = rule["maxBundle"].as_i64();
                let max_label = if rule["maxConfirmed"] == true {
                    max.map(|n| format!("{n}개")).unwrap_or_else(||"확인 필요".into())
                } else {"확인 필요".into()};
                theme::label_value(section, "최대 묶음 수량", &max_label);
                theme::label_value(section, "최근 수정", text(rule, "at"));
                if !text(rule, "notes").is_empty() {
                    theme::label_value(section, "사용자 참고 메모", text(rule, "notes"));
                }
                section.add_space(theme::SPACE_8);
                section.label(typography::subsection_heading("수량별 운임"));
                let fee_rows: Vec<(i64, String)> = (1..=6).filter_map(|count| {
                    rule["fees"].get(count.to_string()).and_then(Value::as_i64)
                        .map(|v| (count, components::display_won(v)))
                }).collect();
                if fee_rows.is_empty() {
                    section.label(typography::caption("등록된 수량별 운임이 없습니다."));
                } else {
                    egui::Grid::new(("freight-rates", id))
                        .striped(true).spacing([theme::SPACE_24, theme::SPACE_8])
                        .show(section, |grid| {
                            for (count, amount) in &fee_rows {
                                grid.label(typography::field_label(format!("{count}개 묶음")));
                                grid.label(typography::quantity(amount));
                                grid.end_row();
                            }
                        });
                }
            });
            ui.add_space(theme::SPACE_12);
        }
        if groups.is_empty() {
            ui.label(typography::caption("등록된 배송 그룹이 없습니다."));
        }
    }

    fn rate_dialog(
        ui:&mut Ui,editor:&mut RateEditor,enabled:bool,
        event:&mut Option<(String,Value)>,close:&mut bool,
    ){
        ui.label(typography::subsection_heading(format!("{} 운임",editor.name)));
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|line|{
            line.label(typography::field_label("최대 합포장 수량"));
            egui::ComboBox::from_id_salt("freight-rate-max")
                .selected_text(editor.max_bundle
                    .map(|v|format!("{v}개")).unwrap_or_else(||"미확정".into()))
                .show_ui(line,|choice|{
                    choice.selectable_value(&mut editor.max_bundle,None,"미확정");
                    for n in 1..=6 {
                        choice.selectable_value(&mut editor.max_bundle,Some(n),format!("{n}개"));
                    }
                });
        });
        if editor.max_bundle.is_none(){
            ui.label(typography::caption("최대 묶음 수량을 먼저 선택하세요."));
        }
        ui.add_space(theme::SPACE_12);
        ui.label(typography::subsection_heading("수량별 운임"));
        ui.label(typography::caption("확인한 묶음 수량의 운임만 추가하세요."));
        let mut removed=None;
        for (count,amount) in editor.fees.iter_mut(){
            ui.horizontal(|line|{
                line.label(typography::field_label(format!("{count}개 묶음")));
                components::field(line,amount,components::FieldKind::Money,enabled);
                if theme::action(line,"삭제",theme::ButtonRole::Standard,enabled).clicked(){
                    removed=Some(*count);
                }
                if editor.max_bundle.is_some_and(|max|*count>max){
                    line.label(typography::status(
                        "최대 묶음 수량 초과",theme::DANGER
                    ));
                }
            });
        }
        if let Some(count)=removed{editor.fees.remove(&count);}
        let available=(1..=6).filter(|n|
            editor.max_bundle.is_some_and(|max|*n<=max)
            && !editor.fees.contains_key(n)
        ).collect::<Vec<_>>();
        if !available.is_empty(){
            ui.horizontal(|line|{
                egui::ComboBox::from_id_salt("freight-rate-add-count")
                    .selected_text(editor.add_count
                        .map(|n|format!("{n}개")).unwrap_or_else(||"묶음 수량 선택".into()))
                    .show_ui(line,|choice|{
                        for count in &available{
                            choice.selectable_value(
                                &mut editor.add_count,Some(*count),format!("{count}개")
                            );
                        }
                    });
                if theme::action(line,"운임 구간 추가",
                    theme::ButtonRole::Standard,enabled&&editor.add_count.is_some()
                ).clicked(){
                    if let Some(n)=editor.add_count.take(){
                        if available.contains(&n){editor.fees.insert(n,String::new());}
                    }
                }
            });
        }
        ui.add_space(theme::SPACE_12);
        ui.label(typography::field_label("사용자 참고 메모"));
        components::flexible_text_field(
            ui,&mut editor.notes,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
            "포장할 때 참고할 사항 (선택)"
        );
        if let Some(error)=&editor.error {
            ui.label(typography::status(error,theme::DANGER));
        }
        ui.add_space(theme::SPACE_12);
        ui.horizontal(|bar|{
            if theme::action(bar,"닫기",theme::ButtonRole::Standard,enabled).clicked(){
                if editor.dirty(){editor.ask_discard=true;}
                else{*close=true;}
            }
            if theme::action(bar,"확인한 운임 저장",theme::ButtonRole::Primary,
                enabled&&editor.dirty()).clicked(){
                match editor.draft(){
                    Ok(rule)=>{
                        editor.error=None;
                        *event=Some(("save_group_rule".into(),json!({
                            "groupId":editor.id,"rule":rule
                        })));
                    }
                    Err(message)=>editor.error=Some(message)
                }
            }
        });
    }

    fn show_history(
        ui: &mut Ui, shipping: &Value, projection: &Value,
        exception: &mut Option<(String,String)>,enabled:bool,
    ) {
        theme::section_frame().show(ui, |section| {
            section.label(typography::section_heading("확인된 배송 조합"));
            components::soft_divider(section);
            let combinations = rows(&shipping["combinations"]);
            if combinations.is_empty() {
                section.label(typography::caption(
                    "실제 배송 완료 후 검증한 운임 조합이 여기에 표시됩니다."
                ));
            }
            for record in combinations.iter().rev() {
                let current = projection["currentCombinations"]
                    .get(text(record, "id")).and_then(Value::as_bool).unwrap_or(false);
                section.horizontal_wrapped(|row|{
                    row.label(typography::table_cell(text(record, "description")));
                    row.label(typography::quantity(format!(
                        "청구 {}",as_fee(&record["charge"])
                    )));
                    row.label(typography::quantity(format!(
                        "실제 {}",as_fee(&record["actual"])
                    )));
                    row.label(typography::caption(
                        if current { "현재 유효" } else { "과거 이력" }
                    ));
                    if theme::action(row,"예외 운임 변경",
                        theme::ButtonRole::Standard,enabled).clicked(){
                        *exception=Some((
                            text(record,"id").into(),
                            record["charge"].as_i64()
                                .map(|n|n.to_string()).unwrap_or_default()
                        ));
                    }
                });
                components::soft_divider(section);
            }
        });
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui, |section| {
            section.label(typography::section_heading("이전 그룹 운임 기준"));
            for group in rows(&shipping["groups"]).iter().filter(|g| active(g)) {
                let history = rows(&group["history"]);
                egui::CollapsingHeader::new(format!(
                    "{} / {}개 기준", text(group, "name"), history.len()
                )).id_salt(("freight-history", text(group, "id")))
                    .show(section, |body| {
                        for old in history {
                            body.label(typography::body(format!(
                                "{} / 개당 {} / {}",
                                text(old, "at"), as_fee(&old["perItem"]),
                                (1..=6).filter_map(|n| {
                                    old["fees"].get(n.to_string())
                                        .and_then(Value::as_i64)
                                        .map(|v| format!("{n}개 {}", components::display_won(v)))
                                }).collect::<Vec<_>>().join(" / "),
                            )));
                        }
                    });
            }
        });
    }

}

fn summary(ui: &mut Ui, label: &str, count: usize) {
    theme::section_frame().show(ui, |cell| {
        cell.label(typography::caption(label));
        cell.label(typography::summary_value(count.to_string()));
    });
}
