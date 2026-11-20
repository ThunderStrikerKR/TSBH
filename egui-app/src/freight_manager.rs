//! Native freight management read-side. All amounts, active-rule versions and
//! combination validity come from the existing Tauri Rust business state and
//! rules::shipping_manager_view. No preview DB or hard-coded rate is treated as
//! operational freight. Write actions are a separate migration step.
use crate::{
    category_manager::ProductCategoryPaths,
    components,
    gateway::{BandoGateway, ReadOperation},
    management_queries,
    settings_help, theme, typography,
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
/// Shared geometry for header and item rows. Each label/control uses this
/// exact column rectangle; no intrinsic egui Grid sizing can shift a column.
fn freight_assignment_cells(
    origin: egui::Pos2, widths: &[f32;8], height:f32
) -> [egui::Rect;8] {
    let mut x=origin.x;
    std::array::from_fn(|index|{
        let cell=egui::Rect::from_min_size(
            egui::pos2(x,origin.y),egui::vec2(widths[index],height)
        );
        x+=widths[index]+theme::FREIGHT_ASSIGNMENT_COL_GAP;
        cell
    })
}

/// One non-wrapping line anchored 6pt from the shared column origin. Keep
/// the full content on hover without changing the cell's layout footprint.
fn freight_assignment_text(
    ui:&mut Ui,rect:egui::Rect,value:&str,full_text:&str,
    emphasis:bool,secondary:bool,id:impl std::hash::Hash
) {
    let clipped=rect.shrink2(egui::vec2(6.0,0.0));
    let (galley,color)=if secondary {
        (typography::client_table_metadata_galley(ui,value,theme::MUTED),theme::MUTED)
    }else{
        (typography::client_table_cell_galley(ui,value,emphasis),theme::TEXT)
    };
    ui.painter().with_clip_rect(clipped).galley(
        egui::pos2(clipped.left(),clipped.center().y-galley.size().y/2.0),galley,color
    );
    if !value.is_empty(){
        let hover_id=ui.make_persistent_id(("freight-cell",id));
        ui.interact(clipped,hover_id,egui::Sense::hover())
            .on_hover_text(full_text);
    }
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
fn model_label(name:&str)->&str {
    let name=name.trim_start();
    let name=name.strip_prefix("[가상]").unwrap_or(name).trim_start();
    let strip=name.get(..2).is_some_and(|prefix|prefix.eq_ignore_ascii_case("BD"));
    if strip {
        let rest=&name[2..];
        if rest.starts_with(char::is_whitespace) ||
            rest.starts_with(|c:char|c.is_ascii_digit()) {
            return rest.trim_start();
        }
    }
    name
}
fn color_label(color:&str)->String {
    match color.trim().to_lowercase().as_str() {
        "gold"=>"골드","rose gold"=>"로즈골드","cosmo black"=>"코스모블랙",
        "black"|"검정"|"블랙"=>"블랙","gray"|"grey"|"회색"|"그레이"=>"그레이",
        "brown"|"브라운"=>"브라운","white"|"흰색"|"화이트"=>"화이트",
        "clear"|"클리어"=>"클리어","frost"|"frosted"|"프로스트"=>"프로스트",
        "silver"=>"실버","blue"=>"블루","red"=>"레드","색상 미표기"=>"",
        _=>color.trim()
    }.to_owned()
}
fn variant_colors(product:&Value)->String {
    let mut labels=Vec::new();
    for variant in rows(&product["variants"]){
        let value=color_label(text(variant,"color"));
        if !value.is_empty() && !labels.contains(&value){labels.push(value);}
    }
    labels.join(" / ")
}
fn modified_date(value:&Value)->&str {
    let timestamp=value.as_str().unwrap_or("");
    if timestamp.is_empty(){"-"}else{timestamp.split('T').next().unwrap_or("-")}
}
fn registered_fee(rule:&Value)->bool {
    rule["perItem"].as_i64().is_some() ||
        rule["fees"].as_object().is_some_and(|fees|fees.values().any(Value::is_number))
}

fn as_fee(value: &Value) -> String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(|| "미등록".to_owned())
}
fn group_name<'a>(groups: &'a [Value], id: &str) -> &'a str {
    groups.iter().find(|g| active(g) && text(g, "id") == id)
        .map(|g| text(g, "name")).unwrap_or("그룹 미지정")
}

/// The read-only tariff summary is derived from the group's effective Rust
/// record, never independently recomputed at the product level.
fn group_bundle_summary(rule: &Value) -> String {
    if rule["maxConfirmed"] == true {
        rule["maxBundle"].as_i64()
            .map(|n| format!("{n}개"))
            .unwrap_or_else(|| "확인 필요".to_owned())
    } else {
        "확인 필요".to_owned()
    }
}
fn group_fee_summary(rule: &Value) -> String {
    let known=(1..=6).filter_map(|n|
        rule["fees"].get(n.to_string()).and_then(Value::as_i64)
            .map(|amount|format!("{n}개 {}",components::display_won(amount)))
    ).collect::<Vec<_>>();
    if !known.is_empty() {
        known.join(" · ")
    } else if let Some(legacy)=rule["perItem"].as_i64() {
        format!("기존 개당 기준 {}",components::display_won(legacy))
    } else {
        "수량별 배송비 미등록".to_owned()
    }
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
    product: bool,
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
            add_count:None,error:None,ask_discard:false,
            product:false
        }
    }
    fn new_product(id:&str,name:&str,rule:&Value)->Self {
        let mut editor=Self::new(id,name,rule);
        editor.product=true;
        editor
    }
    fn dirty(&self)->bool{
        if self.original["maxBundle"].as_i64()!=self.max_bundle
            || text(&self.original,"notes")!=self.notes {return true;}
        let old=(1..=6).filter_map(|count|{
            self.original["fees"].get(count.to_string()).and_then(Value::as_i64)
                .map(|value|(count,Some(value)))
        }).collect::<BTreeMap<_,_>>();
        let new=self.fees.iter().map(|(key,value)|{
            (*key,components::parse_money_input(value))
        }).collect::<BTreeMap<_,_>>();
        old!=new
    }
    fn write_action(&self)->Result<(String,Value),String>{
        let rule=self.draft()?;
        let operation=if self.product {"save_product_rule"}else{"save_group_rule"};
        let data=if self.product {
            json!({"productId":self.id,"rule":rule})
        }else{
            json!({"groupId":self.id,"rule":rule})
        };
        Ok((operation.to_owned(),data))
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
                let amount=components::parse_money_input(raw)
                    .ok_or("올바른 운임을 입력하세요.")?;
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
    bulk_group: Option<String>,
    bulk_feedback: Option<String>,
    new_name: String,
    rename_target: Option<(String,String)>,
    delete_target: Option<String>,
    rate_editor: Option<RateEditor>,
    basis_target: Option<Option<String>>,
    exception_target: Option<(String,String)>,
    product_reset_target: Option<String>,
    requested_tab: Option<FreightTab>,
    saving_for_tab: bool,
    receiver: Option<Receiver<Result<(Value, Value), String>>>,
    state: Option<Value>,
    projection: Option<Value>,
    error: Option<String>,
    search: String,
    group_filter: String,
    only_missing: bool,
}
impl FreightManager {
    fn fetch(gateway: Arc<dyn BandoGateway>,ctx:egui::Context)
        -> Receiver<Result<(Value,Value),String>> {
        let (sender,receiver)=mpsc::channel();
        std::thread::spawn(move||{
            let outcome=management_queries::load_freight(gateway.as_ref());
            let _=sender.send(outcome);
            ctx.request_repaint();
        });
        receiver
    }
    pub fn refresh(&mut self,ctx:egui::Context) {
        if self.receiver.is_none() && !self.busy() && !self.dirty() {
            self.error=None;
            self.notice=None;
            self.receiver=Some(Self::fetch(Arc::clone(&self.gateway),ctx));
        }
    }
    fn refresh_after_conflict(&mut self,ctx:egui::Context) {
        // Keep local drafts while loading the authoritative newer revision.
        // Never reissue the failed write automatically.
        if self.receiver.is_none() {
            self.receiver=Some(Self::fetch(Arc::clone(&self.gateway),ctx));
        }
    }
    pub fn new(gateway: Arc<dyn BandoGateway>, ctx: egui::Context) -> Self {
        let receiver=Self::fetch(Arc::clone(&gateway),ctx);
        Self {
            tab: FreightTab::Groups,
            gateway,
            write_receiver: None,
            write_action: None,
            notice: None,
            pending: BTreeMap::new(),
            selected: BTreeSet::new(),
            bulk_group: None,
            bulk_feedback: None,
            new_name: String::new(),
            rename_target: None,
            delete_target: None,
            rate_editor: None,
            basis_target: None,
            exception_target: None,
            product_reset_target: None,
            requested_tab: None,
            saving_for_tab: false,
            receiver: Some(receiver),
            state: None,
            projection: None,
            error: None,
            search: String::new(),
            group_filter: String::new(),
            only_missing:false,
        }
    }

    fn pending_changes(&self)->serde_json::Map<String,Value>{
        let mut updates=serde_json::Map::new();
        let Some(state)=self.state.as_ref() else {return updates;};
        let Some(products)=state["products"].as_array() else {return updates;};
        for (id,group) in &self.pending {
            if let Some(product)=products.iter().find(|p|text(p,"id")==id.as_str()){
                if text(product,"shippingGroupId")!=group.as_str(){
                    updates.insert(id.clone(),json!(group));
                }
            }
        }
        updates
    }
    pub fn dirty(&self)->bool{
        !self.pending_changes().is_empty() || !self.new_name.trim().is_empty() ||
            self.rate_editor.as_ref().is_some_and(|editor|editor.dirty())
    }
    pub fn busy(&self)->bool{self.write_receiver.is_some()}
    pub fn can_save_dirty(&self)->bool{
        self.gateway.shipping_write_enabled() && !self.busy() &&
            self.new_name.trim().is_empty() &&
            (self.rate_editor.as_ref().is_some_and(|editor|editor.dirty()) ||
                !self.pending_changes().is_empty())
    }
    fn has_other_modal(&self)->bool{
        self.rename_target.is_some() || self.delete_target.is_some() ||
            self.basis_target.is_some() || self.exception_target.is_some() ||
            self.product_reset_target.is_some()
    }
    pub fn blocks_navigation(&self)->bool{
        self.busy() || self.has_other_modal() || self.requested_tab.is_some()
    }
    pub fn has_rate_modal(&self)->bool{self.rate_editor.is_some()}
    pub fn last_error(&self)->Option<&str>{self.error.as_deref()}

    pub fn discard_drafts(&mut self){
        self.pending.clear();
        self.selected.clear();
        self.bulk_group=None;
        self.bulk_feedback=None;
        self.new_name.clear();
        self.rate_editor=None;
        self.rename_target=None;
        self.delete_target=None;
        self.basis_target=None;
        self.exception_target=None;
        self.product_reset_target=None;
        self.requested_tab=None;
        self.saving_for_tab=false;
    }
    pub fn save_pending(&mut self, ctx:egui::Context)->bool{
        if self.busy() ||
            self.rate_editor.as_ref().is_some_and(|editor|editor.dirty()) ||
            !self.new_name.trim().is_empty(){
            return false;
        }
        let changes=self.pending_changes();
        if changes.is_empty(){return true;}
        self.write("assign_products".into(),json!({"assignments":changes}),ctx);
        self.busy()
    }
    pub fn save_dirty(&mut self,ctx:egui::Context)->bool{
        if !self.can_save_dirty(){return false;}
        if let Some(editor)=self.rate_editor.as_mut().filter(|editor|editor.dirty()){
            match editor.write_action(){
                Ok((action,payload))=>{
                    self.write(action,payload,ctx);
                    self.busy()
                }
                Err(message)=>{
                    editor.error=Some(message.clone());
                    self.error=Some(message);
                    false
                }
            }
        }else{
            self.save_pending(ctx)
        }
    }

    fn poll(&mut self,ctx:egui::Context) {
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
                            "save_product_rule"=>"기본 운임을 저장했습니다. 이전 기준은 이력으로 보존합니다.",
                            "reset_product_freight_rule"=>"새 운임 기준을 시작했습니다. 확인된 운임을 다시 입력하세요.",
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
                                self.bulk_group=None;
                                self.bulk_feedback=None;
                            }
                            "save_group_rule"|"save_product_rule"=>self.rate_editor=None,
                            "reset_product_freight_rule"=>self.product_reset_target=None,
                            "change_basis"=>self.basis_target=None,
                            "revise_combination"=>self.exception_target=None,
                            _=>{}
                        }
                        if self.saving_for_tab{
                            if !self.dirty(){
                                self.rate_editor=None;
                                if let Some(target)=self.requested_tab.take(){
                                    self.tab=target;
                                }
                                self.saving_for_tab=false;
                            }
                        }
                    }else{
                        self.error=Some("운임 저장 후 최신 상태를 확인하지 못했습니다.".into());
                    }
                    self.write_receiver=None;
                }
                Ok(Err(error))=>{
                    let conflict=error.contains("다른 작업에서 데이터가 수정되었습니다.");
                    self.error=Some(if conflict {
                        "다른 작업에서 운임 데이터가 수정되었습니다. 최신 값을 다시 불러옵니다. 현재 입력은 유지되므로 변경 내용을 비교한 뒤 저장하세요.".into()
                    }else{error});
                    self.write_receiver=None;
                    self.write_action=None;
                    self.saving_for_tab=false;
                    if conflict {
                        self.refresh_after_conflict(ctx);
                    }
                }
                Err(TryRecvError::Disconnected)=>{
                    self.error=Some("운임 저장 작업이 중단되었습니다. 데이터 재조회를 확인하세요.".into());
                    self.write_receiver=None;
                    self.write_action=None;
                    self.saving_for_tab=false;
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
        self.poll(ui.ctx().clone());
        if self.saving_for_tab && !self.busy() && self.dirty() &&
            !self.save_dirty(ui.ctx().clone()) {
            self.saving_for_tab=false;
        }
        ui.horizontal(|heading|{
            heading.label(typography::page_heading("운임 관리"));
            settings_help::icon(heading,"운임 관리");
            if let Some(state)=self.state.as_ref(){
                heading.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),|right|{
                        right.label(typography::caption(&format!(
                            "운임 기준 {}",
                            state["shipping"]["epoch"].as_i64().unwrap_or(1)
                        )));
                    });
            }
        });
        ui.add_space(theme::SPACE_12);
        if let Some(index) = components::management_tabs(ui, &[
            ("배송 그룹 편성", self.tab == FreightTab::Groups, true),
            ("배송 그룹 관리", self.tab == FreightTab::Rates, true),
            ("조합 / 기준 이력", self.tab == FreightTab::History, true),
        ]) {
            if !self.has_other_modal() && !self.busy() &&
                self.requested_tab.is_none() {
                let next=FreightTab::ALL[index];
                if self.tab!=next {
                    if self.dirty() { self.requested_tab=Some(next); }
                    else {
                        self.rate_editor=None;
                        self.tab=next;
                    }
                }
            }
        }
        ui.add_space(theme::SPACE_12);
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
        let category_paths=ProductCategoryPaths::new(state,&products);
        let assigned = products.iter().filter(|p| {
            let id = text(p, "shippingGroupId");
            !id.is_empty() && groups.iter().any(|g| text(g, "id") == id)
        }).count();
        // Keep all summary cards at their intrinsic width, starting at the
        // left edge. A narrow pane wraps whole cards rather than stretching
        // every card to one quarter of a very wide workspace.
        ui.horizontal_wrapped(|strip|{
            strip.spacing_mut().item_spacing=egui::vec2(
                theme::SPACE_8,theme::SPACE_8
            );
            summary(strip,"등록 상품",products.len());
            summary(strip,"그룹 지정 완료",assigned);
            summary(strip,"그룹 미지정",products.len()-assigned);
            summary(strip,"배송 그룹",groups.len());
        });
        ui.add_space(theme::SPACE_12);
        let enabled=self.gateway.shipping_write_enabled()&&self.write_receiver.is_none();
        let mut action:Option<(String,Value)>=None;
        // The assignment tab owns its own scrollable *rows*. Do not wrap
        // the heading in a second page-wide vertical scroll, which previously
        // pushed the column headings offscreen with the product list.
        if self.tab == FreightTab::Groups {
            Self::show_groups(
                ui, &groups, &products, &category_paths,
                &mut self.search, &mut self.group_filter,
                &mut self.only_missing, &mut self.pending, &mut self.selected,
                &mut self.bulk_group, &mut self.bulk_feedback,
                &mut self.rate_editor, &mut self.product_reset_target,
                &mut self.error, &self.gateway, &projection["productViews"],
                enabled, &mut action
            );
        }else{
            egui::ScrollArea::vertical()
                .id_salt(("freight-management", self.tab as u8))
                .auto_shrink([false,false])
                .show(ui,|scroll|match self.tab {
                    FreightTab::Rates => Self::show_rates(
                        scroll,&groups,&products,projection,
                        &mut self.new_name,&mut self.rename_target,
                        &mut self.delete_target,&mut self.rate_editor,
                        &mut self.basis_target,&mut self.error,enabled,&mut action
                    ),
                    FreightTab::History => Self::show_history(
                        scroll,shipping,projection,&mut self.exception_target,enabled
                    ),
                    FreightTab::Groups => {}
                });
        }
        if self.tab==FreightTab::Groups || self.tab==FreightTab::Rates {
            // Group rename/delete confirmations belong to the management
            // tab, while individual product reset stays on assignment.
            let area=ui.max_rect();
            let ctx=ui.ctx();
            let mut close_rename=false;
            let mut close_delete=false;
            let mut close_product_reset=false;
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
                let affected=products.iter().copied().filter(|p|
                    text(p,"shippingGroupId")==id.as_str()).collect::<Vec<_>>();
                let name=group_name(&groups,id);
                components::modal_backdrop(ctx,"freight-delete-backdrop",false);
                components::decision_window("배송 그룹 삭제","freight-delete",area)
                    .show(ctx,|modal|{
                        modal.label(typography::section_heading(name));
                        if !affected.is_empty(){
                            modal.label(typography::status(
                                "사용 중인 상품이 있습니다. 상품을 다른 그룹으로 옮기거나 미지정으로 변경한 후 삭제해야 합니다.",
                                theme::SELECTED_ORDER
                            ));
                            egui::ScrollArea::vertical()
                                .id_salt("freight-delete-affected-products")
                                .max_height(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
                                .show(modal,|list|{
                                    for product in &affected {
                                        let first=rows(&product["variants"]).first();
                                        let color=first.map(|v|color_label(text(v,"color")))
                                            .unwrap_or_default();
                                        let label=if color.is_empty(){
                                            model_label(text(product,"name")).to_owned()
                                        }else{
                                            format!("{} / {}",model_label(text(product,"name")),color)
                                        };
                                        list.label(typography::table_cell(&label));
                                    }
                                });
                        }else{
                            modal.label(typography::body(
                                "연결된 현재 상품이 없습니다. 과거 운임과 주문 기록은 보존합니다."
                            ));
                        }
                        modal.add_space(theme::SPACE_12);
                        modal.horizontal(|controls|{
                            if theme::action(controls,"취소",theme::ButtonRole::Quiet,enabled)
                                .clicked(){close_delete=true;}
                            if theme::action(controls,if !affected.is_empty(){
                                "상품 연결을 미지정으로 변경하고 삭제"
                            }else{"그룹 삭제"},theme::ButtonRole::Danger,enabled)
                                .clicked(){
                                action=Some(("delete_group".into(),json!({
                                    "groupId":id,"detach":!affected.is_empty()
                                })));
                            }
                        });
                    });
            }else if let Some(pid)=self.product_reset_target.as_ref(){
                components::modal_backdrop(ctx,"freight-reset-backdrop",false);
                components::decision_window(
                    "운임 기준 변경","freight-product-reset",area
                ).show(ctx,|dialog|{
                    dialog.label(typography::body(
                        "이 상품의 새 운임 기준을 시작하고 이전 자동 추천을 중단할까요?"
                    ));
                    dialog.horizontal(|bar|{
                        if theme::action(bar,"취소",
                            theme::ButtonRole::Standard,enabled).clicked(){
                            close_product_reset=true;
                        }
                        if theme::action(bar,"기준 변경",
                            theme::ButtonRole::Danger,enabled).clicked(){
                            action=Some(("reset_product_freight_rule".into(),
                                json!({"productId":pid})));
                        }
                    });
                });
            }
            if close_rename{self.rename_target=None;}
            if close_delete{self.delete_target=None;}
            if close_product_reset{self.product_reset_target=None;}
        }
        if self.rate_editor.is_some() || self.tab==FreightTab::Rates {
            let area=ui.max_rect();
            let ctx=ui.ctx();
            let mut close_editor=false;
            let mut close_basis=false;
            if let Some(editor)=self.rate_editor.as_mut(){
                components::modal_backdrop(ctx,"freight-rate-backdrop",false);
                components::modal_window(if editor.product{"기본 운임 설정"}else{"배송 그룹 운임 수정"},"freight-rate-modal",area)
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
                            if theme::action(bar,"저장하고 닫기",
                                theme::ButtonRole::Primary,enabled&&editor.dirty()).clicked(){
                                match editor.write_action(){
                                    Ok(command)=>{
                                        editor.error=None;
                                        action=Some(command);
                                    }
                                    Err(message)=>{
                                        editor.error=Some(message);
                                        editor.ask_discard=false;
                                    }
                                }
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
                    components::money_field(dialog,amount,enabled);
                    dialog.add_space(theme::SPACE_12);
                    dialog.horizontal(|bar|{
                        if theme::action(bar,"취소",
                            theme::ButtonRole::Standard,enabled).clicked(){
                            close=true;
                        }
                        if theme::action(bar,"예외 운임 변경",
                            theme::ButtonRole::Primary,enabled).clicked(){
                            match components::parse_money_input(amount){
                                Some(value)=>{
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
        if let Some(next)=self.requested_tab {
            let mut cancel=false;
            let mut discard=false;
            let mut save=false;
            let can_save=self.can_save_dirty();
            let ctx=ui.ctx();
            components::modal_backdrop(ctx,"freight-tab-leave-backdrop",
                self.rate_editor.is_some());
            components::decision_window(
                "저장하지 않은 변경사항","freight-tab-leave",ui.max_rect()
            ).show(ctx,|dialog|{
                dialog.label(typography::body(
                    "운임 관리에서 수정한 내용이 있습니다. 변경사항을 저장할까요?"
                ));
                if !self.new_name.trim().is_empty(){
                    dialog.label(typography::caption(
                        "새 배송 그룹을 등록하려면 배송 그룹 추가를 눌러 주세요."
                    ));
                }
                dialog.add_space(theme::SPACE_12);
                dialog.horizontal(|bar|{
                    if theme::action(bar,"취소",theme::ButtonRole::Standard,enabled)
                        .clicked(){cancel=true;}
                    if theme::action(bar,"저장 안 함",
                        theme::ButtonRole::Standard,enabled).clicked(){discard=true;}
                    if theme::action(bar,"저장하고 이동",
                        theme::ButtonRole::Primary,can_save).clicked(){save=true;}
                });
            });
            if cancel{self.requested_tab=None;}
            else if discard{
                self.discard_drafts();
                self.tab=next;
            }else if save{
                if self.save_dirty(ui.ctx().clone()){
                    self.saving_for_tab=true;
                }
            }
        }
        if let Some((kind,data))=action {
            self.write(kind,data,ui.ctx().clone());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_groups(
        ui: &mut Ui, groups: &[Value], products: &[&Value],
        category_paths:&ProductCategoryPaths,
        search: &mut String, group_filter: &mut String,
        only_missing:&mut bool,
        pending: &mut BTreeMap<String,String>, selected: &mut BTreeSet<String>,
        bulk_group: &mut Option<String>,
        bulk_feedback: &mut Option<String>,
        rate_editor: &mut Option<RateEditor>,
        reset_target: &mut Option<String>,
        error: &mut Option<String>,
        gateway: &Arc<dyn BandoGateway>,
        product_views: &Value, enabled:bool,
        action:&mut Option<(String,Value)>,
    ) {
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui, |section| {
            section.horizontal(|title|{
                title.label(typography::section_heading("상품별 배송 그룹 지정"));
                settings_help::icon(title,"상품별 운임");
            });
            // Reserve a single toolbar rectangle. Left filters and right
            // bulk controls receive independent fixed-height child UIs.
            // Mixing left-to-right and right-to-left layouts in ONE row
            // previously advanced the second group's vertical origin.
            let toolbar_width=section.available_width();
            let (toolbar_rect,_) = section.allocate_exact_size(
                egui::vec2(toolbar_width,theme::INPUT_HEIGHT),
                egui::Sense::hover()
            );
            let bulk_width=500.0_f32.min(toolbar_rect.width());
            let left_width=(toolbar_rect.width()-bulk_width-theme::SPACE_16).max(0.0);
            let left_rect=egui::Rect::from_min_size(
                toolbar_rect.left_top(),
                egui::vec2(left_width,toolbar_rect.height())
            );
            let right_rect=egui::Rect::from_min_size(
                egui::pos2(toolbar_rect.right()-bulk_width,toolbar_rect.top()),
                egui::vec2(bulk_width,toolbar_rect.height())
            );
            section.scope_builder(
                egui::UiBuilder::new()
                    .id_salt("freight-assignment-search-toolbar")
                    .max_rect(left_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |tools|{
                    tools.set_clip_rect(left_rect);
                    tools.set_width(left_rect.width());
                    tools.set_height(left_rect.height());
                    tools.spacing_mut().item_spacing.x=theme::SPACE_8;
                    // Match the common 36pt input geometry for both search
                    // and filter dropdowns, rather than default egui height.
                    tools.spacing_mut().interact_size.y=theme::INPUT_HEIGHT;
                components::flexible_text_field(
                    tools,search,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                    "모델명 / 상품명 / 카테고리 검색"
                );
                let before_filter=group_filter.clone();
                egui::ComboBox::from_id_salt("freight-group-filter")
                    .selected_text(if group_filter.is_empty() {
                        "전체 상품"
                    } else if group_filter.as_str()=="none" {
                        "그룹 미지정"
                    } else { group_name(groups,group_filter) })
                    .show_ui(tools,|combo|{
                        combo.selectable_value(group_filter,String::new(),"전체 상품");
                        combo.selectable_value(group_filter,"none".into(),"그룹 미지정만");
                        for group in groups {
                            combo.selectable_value(
                                group_filter,text(group,"id").into(),text(group,"name")
                            );
                        }
                    });
                if before_filter!=*group_filter {
                    selected.clear();
                    *bulk_feedback=None;
                    *only_missing=false;
                }
                tools.checkbox(only_missing,"그룹 미지정 상품만");

                }
            );

                // Apply the current filters before drawing bulk selection.
                // This ensures "전체 선택" always targets the visible rows.
                let needle=search.to_lowercase().split_whitespace().collect::<String>();
                let mut filtered:Vec<&Value>=products.iter().copied().filter(|p|{
                    let grouped=groups.iter().any(|g|
                        text(g,"id")==text(p,"shippingGroupId")
                    );
                    (needle.is_empty()||[text(p,"code"),text(p,"name"),
                        text(p,"subtitle"),category_paths.for_product(p)]
                        .iter().any(|s|s.to_lowercase().split_whitespace()
                            .collect::<String>().contains(&needle)))
                    && (group_filter.is_empty()
                        || (group_filter.as_str()=="none" && !grouped)
                        || text(p,"shippingGroupId")==group_filter.as_str())
                    && (!*only_missing || !grouped)
                }).collect();
                // Sort by committed group membership, never by an unsaved
                // selector value, to keep product rows stable during edits.
                if needle.is_empty() && group_filter.is_empty() && !*only_missing {
                    filtered.sort_by_key(|p|usize::from(groups.iter().any(|g|
                        text(g,"id")==text(p,"shippingGroupId")
                    )));
                }

                // None = no target chosen; Some("") = explicit removal.
                if bulk_group.as_deref().is_some_and(|id|
                    !id.is_empty() && !groups.iter().any(|g|text(g,"id")==id)
                ) {
                    *bulk_group=None;
                    *bulk_feedback=None;
                }
            let previous_group=bulk_group.clone();
            // Disable at the toolbar scope rather than using nested
            // add_enabled_ui() scopes. Those nested scopes introduced a
            // second vertical alignment pass that pushed the combo box
            // and checkbox below the search control's center line.
            let mut bulk_builder=egui::UiBuilder::new()
                .id_salt("freight-assignment-bulk-toolbar")
                .max_rect(right_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center));
            if !enabled {
                bulk_builder=bulk_builder.disabled();
            }
            section.scope_builder(bulk_builder,|bulk|{
                    bulk.set_clip_rect(right_rect);
                    bulk.set_width(right_rect.width());
                    bulk.set_height(right_rect.height());
                    bulk.spacing_mut().item_spacing.x=theme::SPACE_8;
                    // Keep the bulk dropdown at the same 36pt control
                    // height as the search field on the other side.
                    bulk.spacing_mut().interact_size.y=theme::INPUT_HEIGHT;
                        settings_help::icon(bulk,"배송 그룹 일괄 지정");
                        if theme::action(bulk,"선택 상품에 적용",
                            theme::ButtonRole::Standard,
                            enabled&&!selected.is_empty()&&bulk_group.is_some()
                        ).clicked(){
                            if let Some(target)=bulk_group.as_deref(){
                                let (mut newly,mut moved,mut removed,mut unchanged)=(0,0,0,0);
                                for product in products.iter().copied()
                                    .filter(|product|selected.contains(text(product,"id"))) {
                                    let id=text(product,"id");
                                    let original=text(product,"shippingGroupId");
                                    if original==target {
                                        pending.remove(id);
                                        unchanged+=1;
                                    }else{
                                        pending.insert(id.to_owned(),target.to_owned());
                                        if target.is_empty(){removed+=1;}
                                        else if original.is_empty(){newly+=1;}
                                        else{moved+=1;}
                                    }
                                }
                                *bulk_feedback=Some(if target.is_empty(){
                                    format!("그룹 해제 {removed}건 · 변경 없음 {unchanged}건")
                                }else{
                                    format!("신규 지정 {newly}건 · 그룹 변경 {moved}건 · 변경 없음 {unchanged}건")
                                });
                            }
                        }
                        egui::ComboBox::from_id_salt("freight-bulk-assign")
                            .width(theme::FREIGHT_BULK_GROUP_WIDTH)
                            .selected_text(match bulk_group.as_deref(){
                                None=>"배송 그룹 선택",
                                Some("")=>"미지정으로 변경",
                                Some(id)=>group_name(groups,id),
                            })
                            .show_ui(bulk,|combo|{
                                combo.selectable_value(
                                    bulk_group,Some(String::new()),"미지정으로 변경"
                                );
                                for group in groups {
                                    combo.selectable_value(
                                        bulk_group,Some(text(group,"id").to_owned()),
                                        text(group,"name")
                                    );
                                }
                            });
                        bulk.label(typography::field_label(&format!(
                            "{}개 선택",selected.len()
                        )));
                }
            );
            if *bulk_group!=previous_group {
                *bulk_feedback=None;
            }
            components::soft_divider(section);
            // Reserve the header OUTSIDE the scroll area. The table's
            // scroll offset is reflected only in its horizontal coordinates,
            // so wheel scrolling cannot push column titles off-screen.
            let viewport_width=section.available_width().max(1.0);
            let table_width=viewport_width.max(theme::FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH);
            let mut widths=theme::FREIGHT_ASSIGNMENT_COL_WIDTHS;
            let extra=(table_width-theme::FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH).max(0.0);
            // Fixed product and color/option widths; all extra viewport
            // space belongs to the canonical category path (column 4).
            widths[3]+=extra;
            let titles=[
                "","상품명 / 상품설명","색상/옵션","카테고리",
                "배송 그룹","운임 / 묶음 상태","지정 상태","설정"
            ];
            // The header is a fixed sibling above the independently scrolling
            // data area. Both use freight_assignment_cells() for all 8 edges.
            let (header_rect,_)=section.allocate_exact_size(
                egui::vec2(viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
                egui::Sense::hover()
            );
            let mut first_row_left=None;
            let scroll_output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .id_salt("freight-assignment-rows")
                    .max_height(section.ctx().content_rect().height()
                        * theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
                    .auto_shrink([false,false]),
                section,table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT,
                filtered.len(),
                |table,visible_rows|{
                    table.set_min_width(table_width);
                    table.spacing_mut().item_spacing.y=0.0;
                    theme::apply_table_row_colors(table);
                    // Rows outside the viewport keep their selections and
                    // pending assignments, but create no widget this frame.
                    for row_index in visible_rows{
                        let product=filtered[row_index];
                        let id=text(product,"id");
                        let committed=text(product,"shippingGroupId");
                        let mut checked=selected.contains(id);
                        let (row_rect,_)=table.allocate_exact_size(
                            egui::vec2(table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT),
                            egui::Sense::hover()
                        );
                        first_row_left.get_or_insert(row_rect.left());
                        let cells=freight_assignment_cells(
                            row_rect.min,&widths,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                        );
                        let background=if checked {theme::WORKFLOW_SELECTED_BG}
                            else if row_index%2==0 {theme::PANEL}
                            else {theme::TABLE_ROW_ALT_BG};
                        table.painter().rect_filled(row_rect,0.0,background);
                        if checked {
                            table.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    row_rect.min,
                                    egui::vec2(theme::WORKFLOW_SELECTED_STRIPE,row_rect.height())
                                ),0.0,theme::SELECTION
                            );
                        }

                        // A checkbox owns exactly one column rectangle; it
                        // must not be allowed to change subsequent column widths.
                        // Center the checkbox within the FIRST COLUMN,
                        // not 4pt from its left edge. Keep one stable
                        // interactive box centered vertically in the 42pt row.
                        let check_width=table.spacing().icon_width
                            +table.spacing().icon_spacing;
                        let checkbox_rect=egui::Rect::from_center_size(
                            cells[0].center(),
                            egui::vec2(check_width,theme::INPUT_HEIGHT)
                        );
                        let checkbox_clip=cells[0].intersect(table.clip_rect());
                        let mut checkbox_builder=egui::UiBuilder::new()
                            .id_salt(("freight-checkbox",id))
                            .max_rect(checkbox_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Min));
                        if !enabled {
                            checkbox_builder=checkbox_builder.disabled();
                        }
                        table.scope_builder(checkbox_builder,|controls|{
                            controls.set_clip_rect(checkbox_clip);
                            if controls.checkbox(&mut checked,"").changed(){
                                if checked {selected.insert(id.to_owned());}
                                else {selected.remove(id);}
                                *bulk_feedback=None;
                            }
                        });

                        let model=model_label(text(product,"name"));
                        let subtitle=text(product,"subtitle").trim();
                        let product_title=if subtitle.is_empty()
                            || subtitle==text(product,"name") {
                            model.to_owned()
                        }else{format!("{model} / {subtitle}")};
                        freight_assignment_text(
                            table,cells[1],&product_title,&product_title,
                            true,false,(id,1)
                        );
                        // The narrow third column is color/option. The wide
                        // fourth column is the full canonical category path.
                        let colors=variant_colors(product);
                        freight_assignment_text(
                            table,cells[2],&colors,&colors,false,false,(id,2)
                        );
                        let category=category_paths.for_product(product);
                        let category=if category.trim().is_empty(){
                            "분류 미지정"
                        }else{category};
                        let visible_category=typography::fit_category_path(
                            table,category,
                            (cells[3].width()-theme::SPACE_12).max(0.0),
                            typography::CategoryPathFont::TableCell
                        );
                        freight_assignment_text(
                            table,cells[3],&visible_category,category,
                            false,false,(id,3)
                        );

                        // The group selector's own child UI shares the same
                        // left edge and vertical center as the painted cell.
                        let mut current=pending.get(id).cloned()
                            .unwrap_or_else(||committed.into());
                        let before_manual_edit=current.clone();
                        // The dropdown was vertically clipped because an
                        // Align::Center child was centered again by
                        // add_enabled_ui(), leaving its bottom outside the
                        // 42pt row. Reserve one 36pt control slot at y+3,
                        // render from its TOP, and clip only to the full row.
                        // This change does not alter any column width.
                        let selector_rect=egui::Rect::from_min_size(
                            egui::pos2(
                                cells[4].left()+theme::SPACE_4,
                                cells[4].top()
                                    +(theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                                        -theme::INPUT_HEIGHT)/2.0
                            ),
                            egui::vec2(
                                cells[4].width()-theme::SPACE_8,
                                theme::INPUT_HEIGHT
                            )
                        );
                        let selector_clip=cells[4].intersect(table.clip_rect());
                        let mut selector_builder=egui::UiBuilder::new()
                            .id_salt(("freight-selector",id))
                            .max_rect(selector_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Min));
                        if !enabled {
                            selector_builder=selector_builder.disabled();
                        }
                        table.scope_builder(selector_builder,|control|{
                                control.set_clip_rect(selector_clip);
                                control.set_width(selector_rect.width());
                                egui::ComboBox::from_id_salt(("freight-row-group",id))
                                    .width(selector_rect.width())
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .selected_text(if current.is_empty(){
                                        "그룹 선택"
                                    }else{group_name(groups,&current)})
                                    .show_ui(control,|choice|{
                                        choice.selectable_value(
                                            &mut current,String::new(),"그룹 선택"
                                        );
                                        for group in groups {
                                            choice.selectable_value(
                                                &mut current,text(group,"id").into(),
                                                text(group,"name")
                                            );
                                        }
                                    });
                            }
                        );
                        if current!=before_manual_edit {
                            *bulk_feedback=None;
                        }
                        if current!=committed {
                            pending.insert(id.to_owned(),current);
                        }else{pending.remove(id);}

                        let changed=pending.contains_key(id);
                        let effective=product_views.get(id)
                            .filter(|entry|entry.is_object());
                        let rule=effective.map(|entry|&entry["rule"])
                            .unwrap_or(&Value::Null);
                        let ready=effective.is_some();
                        let confirmed=rule["maxConfirmed"]==true;
                        let count=rule["maxBundle"].as_i64();
                        let bundle=if !ready{"확인 중".to_owned()}
                            else if !confirmed{"확인 필요".to_owned()}
                            else{count.map(|n|format!("{n}개"))
                                .unwrap_or_else(||"확인 필요".to_owned())};
                        let freight=if !ready{"확인 중"}
                            else if registered_fee(rule){"등록"}else{"미등록"};
                        let packing=if !ready{"확인 중"}
                            else if !confirmed || count.is_none(){"확인 필요"}
                            else if count==Some(1){"개별 포장"}
                            else{"합포장 가능"};
                        let info=format!("{freight} · {bundle} · {packing}");
                        let full_info=format!(
                            "최대 묶음: {bundle} / 운임: {freight} / 합포장: {packing} / 최근 수정: {}",
                            modified_date(&rule["at"])
                        );
                        freight_assignment_text(
                            table,cells[5],&info,&full_info,false,false,(id,5)
                        );
                        let assigned=if changed{"변경 예정"}
                            else if committed.is_empty(){"미지정"}
                            else{"지정 완료"};
                        freight_assignment_text(
                            table,cells[6],assigned,assigned,false,true,(id,6)
                        );

                        // The action column's buttons and labels cannot expand
                        // their cell, wrap, or shift the other seven columns.
                        let actions_rect=cells[7].shrink2(egui::vec2(6.0,0.0));
                        let actions_clip=actions_rect.intersect(table.clip_rect());
                        table.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(("freight-actions",id))
                                .max_rect(actions_rect)
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |actions|{
                                actions.set_clip_rect(
                                    actions_clip
                                );
                                actions.set_width(actions_rect.width());
                                actions.spacing_mut().item_spacing.x=theme::SPACE_8;
                                let editable=enabled&&!changed&&ready;
                                if groups.iter().any(|g|text(g,"id")==committed){
                                    actions.add(
                                        egui::Label::new(typography::caption(
                                            if changed {"그룹 변경 예정"}
                                            else {"그룹 운임 적용 · 배송 그룹 관리에서 수정"}
                                        )).truncate()
                                    ).on_hover_text(
                                        "그룹 운임은 배송 그룹 관리 탭에서 수정합니다."
                                    );
                                }else{
                                    if theme::action(actions,"기본 운임 설정",
                                        theme::ButtonRole::Standard,editable).clicked(){
                                        let initial=effective
                                            .and_then(|entry|entry.get("editableRule"))
                                            .filter(|v|v.is_object()).cloned()
                                            .or_else(||{
                                                match gateway.execute(
                                                    ReadOperation::DefaultProductRule,
                                                    json!({
                                                        "action":"default_product_rule",
                                                        "versionId":"unsaved",
                                                        "now":chrono::Utc::now().to_rfc3339()
                                                    })
                                                ){
                                                    Ok(value)=>value.get("rule").cloned(),
                                                    Err(message)=>{
                                                        *error=Some(message);
                                                        None
                                                    }
                                                }
                                            });
                                        if let Some(initial)=initial {
                                            *rate_editor=Some(RateEditor::new_product(
                                                id,text(product,"name"),&initial
                                            ));
                                        }
                                    }
                                    if rule.is_object() &&
                                        theme::action(actions,"기준 변경",
                                            theme::ButtonRole::Quiet,editable).clicked(){
                                        *reset_target=Some(id.to_owned());
                                    }
                                }
                            }
                        );
                        // Shared 1pt dividers are drawn at the exact midpoint
                        // of the column gaps. No opacity multiplication:
                        // 0.30 × DIVIDER was darker than the row background,
                        // which made the separators effectively invisible.
                        for index in 0..7 {
                            let edge=cells[index].right()
                                +theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
                            table.painter().line_segment(
                                [egui::pos2(edge,row_rect.top()),
                                 egui::pos2(edge,row_rect.bottom())],
                                egui::Stroke::new(
                                    theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                                )
                            );
                        }
                        table.painter().line_segment(
                            [row_rect.left_bottom(),row_rect.right_bottom()],
                            egui::Stroke::new(
                                theme::SOFT_DIVIDER_WIDTH,
                                theme::DIVIDER.linear_multiply(0.70)
                            )
                        );
                    }
                    if filtered.is_empty(){
                        table.label(typography::caption(
                            if products.is_empty(){"등록된 상품이 없습니다."}
                            else{"검색 조건에 맞는 상품이 없습니다."}
                        ));
                    }
                });
            // Header and body use freight_assignment_cells() AND the body
            // scroll origin, so no egui padding / Grid sizing can drift.
            let header_left=first_row_left.unwrap_or(
                header_rect.left()-scroll_output.state.offset.x
            );
            let header_cells=freight_assignment_cells(
                egui::pos2(header_left,header_rect.top()),
                &widths,header_rect.height()
            );
            let header_painter=section.painter().with_clip_rect(header_rect);
            header_painter.rect_filled(header_rect,0.0,theme::RAISED);
            for (i,title) in titles.iter().enumerate(){
                if !title.is_empty(){
                    let region=header_cells[i];
                    let galley=typography::table_header_galley(section,title);
                    header_painter.with_clip_rect(header_rect.intersect(region)).galley(
                        egui::pos2(
                            region.center().x-galley.size().x/2.0,
                            region.center().y-galley.size().y/2.0
                        ),
                        galley,theme::SECONDARY
                    );
                }
                if i<7 {
                    let edge=header_cells[i].right()
                        +theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
                    header_painter.line_segment(
                        [egui::pos2(edge,header_rect.top()),
                         egui::pos2(edge,header_rect.bottom())],
                        egui::Stroke::new(
                            theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                        )
                    );
                }
            }
            header_painter.line_segment(
                [header_rect.left_bottom(),header_rect.right_bottom()],
                egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
            );

            // This is the single "select all" control for the currently
            // filtered product list (not only the virtualized visible rows).
            // It replaces the redundant checkbox in the bulk-action toolbar.
            let mut all_filtered=!filtered.is_empty()
                && filtered.iter().all(|p|selected.contains(text(p,"id")));
            let header_check_width=section.spacing().icon_width
                +section.spacing().icon_spacing;
            let header_check_rect=egui::Rect::from_center_size(
                header_cells[0].center(),
                egui::vec2(
                    header_check_width,
                    header_cells[0].height().min(theme::INPUT_HEIGHT)
                )
            );
            let mut header_check_builder=egui::UiBuilder::new()
                .id_salt("freight-select-all-filtered")
                .max_rect(header_check_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center));
            if !enabled || filtered.is_empty(){
                header_check_builder=header_check_builder.disabled();
            }
            section.scope_builder(header_check_builder,|header_check|{
                header_check.set_clip_rect(
                    header_cells[0].intersect(header_rect)
                );
                if header_check.checkbox(&mut all_filtered,"")
                    .on_hover_text("현재 검색·필터 결과 전체 선택 / 해제")
                    .changed()
                {
                    let current_ids: BTreeSet<&str>=filtered.iter()
                        .map(|p|text(p,"id")).collect();
                    // Keep selections outside the current filtered list.
                    selected.retain(|id|!current_ids.contains(id.as_str()));
                    if all_filtered{
                        selected.extend(current_ids.into_iter().map(str::to_owned));
                    }
                    *bulk_feedback=None;
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
                // Reuse the existing footer, not another toolbar row or
                // popup, to report the staged result of the last bulk action.
                let summary=if let Some(feedback)=bulk_feedback.as_deref(){
                    format!("{feedback} · 저장 대기 {}건",assignments.len())
                }else if assignments.is_empty(){
                    "등록된 상품을 임의로 분류하지 않습니다. 사용자가 확인한 그룹을 지정해 주세요.".to_owned()
                }else{
                    format!("{}개 상품의 배송 그룹을 변경합니다.",assignments.len())
                };
                actions.label(typography::caption(&summary));
                let save_label=if assignments.is_empty(){
                    "그룹 지정 저장".to_owned()
                }else{
                    format!("그룹 지정 저장 / {}개",assignments.len())
                };
                actions.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),|buttons|{
                        if theme::action(buttons,&save_label,
                            theme::ButtonRole::Primary,enabled&&!assignments.is_empty()
                        ).clicked(){
                            *action=Some(("assign_products".into(),json!({
                                "assignments":assignments
                            })));
                        }
                        if !assignments.is_empty() &&
                            theme::action(buttons,"미저장 변경 초기화",
                                theme::ButtonRole::Standard,enabled).clicked(){
                            pending.clear();
                        }
                    }
                );
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn show_rates(
        ui: &mut Ui, groups: &[Value], products: &[&Value], projection: &Value,
        new_name: &mut String,
        rename_target: &mut Option<(String,String)>,
        delete_target: &mut Option<String>,
        editor: &mut Option<RateEditor>,
        basis: &mut Option<Option<String>>,
        error: &mut Option<String>,enabled:bool,
        action: &mut Option<(String,Value)>,
    ) {
        let group_rules=&projection["editableRules"];
        // Group tariffs are the primary source for all assigned products.
        // Put their editable quantity/rate summary next to the group itself.
        theme::section_frame().show(ui, |section| {
            section.label(typography::section_heading("배송 그룹 관리"));
            section.label(typography::caption(
                "그룹 소속 상품은 공통 묶음포장 수량과 수량별 배송비를 적용합니다."
            ));
            section.add_space(theme::SPACE_12);
            section.horizontal_wrapped(|tools|{
                tools.label(typography::field_label("새 배송 그룹명"));
                tools.add_enabled_ui(enabled,|field|{
                    components::flexible_text_field(
                        field,new_name,theme::MANAGEMENT_SEARCH_MIN_WIDTH,"새 배송 그룹명"
                    );
                });
                if theme::action(tools,"배송 그룹 추가",theme::ButtonRole::Primary,
                    enabled&&!new_name.trim().is_empty()).clicked(){
                    *action=Some(("create_group".into(),json!({
                        "name":new_name.trim()
                    })));
                }
            });
            section.add_space(theme::SPACE_12);
            components::soft_divider(section);
            if groups.is_empty(){
                section.label(typography::caption(
                    "그룹을 등록한 다음 최대 묶음 수량과 수량별 배송비를 설정하세요."
                ));
            }
            for group in groups {
                let id=text(group,"id");
                let count=products.iter()
                    .filter(|p|text(p,"shippingGroupId")==id).count();
                section.add_space(theme::SPACE_8);
                section.horizontal_wrapped(|line|{
                    line.label(typography::body(text(group,"name")));
                    line.label(typography::caption(&format!("{count}개 상품")));
                    if theme::action(
                        line,"묶음 수량·배송비 설정",theme::ButtonRole::Standard,enabled
                    ).clicked(){
                        match group_rules.get(id).filter(|value|value.is_object()){
                            Some(rule)=>*editor=Some(RateEditor::new(
                                id,text(group,"name"),rule
                            )),
                            None=>*error=Some(
                                "그룹 운임 기준을 읽지 못했습니다. 새로고침 후 확인하세요.".into()
                            ),
                        }
                    }
                    if theme::action(line,"이름 수정",theme::ButtonRole::Quiet,enabled)
                        .clicked(){
                        *rename_target=Some((id.into(),text(group,"name").into()));
                    }
                    if theme::action(line,"삭제",theme::ButtonRole::Danger,enabled)
                        .clicked(){
                        *delete_target=Some(id.into());
                    }
                });
                section.horizontal_wrapped(|info|{
                    info.label(typography::field_label("최대 묶음"));
                    info.label(typography::quantity(
                        &group_bundle_summary(&group["current"])
                    ));
                    info.add_space(theme::SPACE_8);
                    info.label(typography::field_label("수량별 배송비"));
                    info.label(typography::caption(
                        &group_fee_summary(&group["current"])
                    ));
                });
                components::soft_divider(section);
            }
        });
        ui.add_space(theme::SPACE_12);
        ui.horizontal(|bar|{
            bar.label(typography::section_heading("그룹 운임표"));
            settings_help::icon(bar,"그룹 운임표");
            if theme::action(bar,"전체 운임 기준 변경",theme::ButtonRole::Danger,enabled)
                .clicked(){*basis=Some(None);}
            settings_help::icon(bar,"전체 운임 기준 변경");
        });
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui,|section|{
            egui::ScrollArea::both()
                .id_salt("freight-group-rates-table")
                .max_height(section.ctx().content_rect().height()
                    * theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
                .auto_shrink([false,false])
                .show(section,|table|{
                    table.set_min_width(theme::MANAGEMENT_TABLE_MIN_WIDTH);
                    theme::apply_table_row_colors(table);
                    egui::Grid::new("freight-group-rates-grid")
                        .striped(true)
                        .spacing([theme::SPACE_24,theme::SPACE_8])
                        .show(table,|grid|{
                            for heading in [
                                "배송 그룹","최대 묶음 수량","운임 등록 상태",
                                "사용자 참고 메모","최근 수정","관리"
                            ] {
                                grid.label(typography::table_header(heading));
                            }
                            grid.end_row();
                            for group in groups {
                                let id=text(group,"id");
                                let name=text(group,"name");
                                // The raw current tariff owns status and confirmation.
                                // editableRules is only for opening a writable dialog:
                                // legacy perItem normalization can make an unconfirmed
                                // raw group look "maxConfirmed = 1" if displayed.
                                let current=&group["current"];
                                let editable=projection["editableRules"].get(id)
                                    .unwrap_or(current);
                                let max=if current["maxConfirmed"]==true {
                                    current["maxBundle"].as_i64()
                                        .map(|n|format!("{n}개"))
                                        .unwrap_or_else(||"확인 필요".into())
                                }else{"확인 필요".into()};
                                let registered=current["perItem"].as_i64().is_some() ||
                                    current["fees"].as_object().is_some_and(|fees|
                                        fees.values().any(Value::is_number));
                                let note=text(current,"notes");
                                let date=text(current,"at").split('T').next().unwrap_or("");
                                grid.label(typography::table_cell(name));
                                grid.label(typography::table_cell(&max));
                                grid.label(typography::table_cell(
                                    if registered{"등록"}else{"미등록"}
                                ));
                                grid.label(typography::caption(
                                    if note.is_empty(){"-"}else{note}
                                ));
                                grid.label(typography::table_cell(
                                    if date.is_empty(){"-"}else{date}
                                ));
                                grid.horizontal_wrapped(|actions|{
                                    if theme::action(actions,"운임 수정",
                                        theme::ButtonRole::Standard,enabled).clicked(){
                                        *editor=Some(RateEditor::new(id,name,editable));
                                    }
                                    if theme::action(actions,"기준 변경",
                                        theme::ButtonRole::Standard,enabled).clicked(){
                                        *basis=Some(Some(id.to_owned()));
                                    }
                                });
                                grid.end_row();
                            }
                        });
                    if groups.is_empty(){
                        table.label(typography::caption("등록된 배송 그룹이 없습니다."));
                    }
                });
        });
    }

    fn rate_dialog(
        ui:&mut Ui,editor:&mut RateEditor,enabled:bool,
        event:&mut Option<(String,Value)>,close:&mut bool,
    ){
        ui.horizontal(|title|{
            title.label(typography::subsection_heading(&format!("{} 운임",editor.name)));
            settings_help::icon(title,if editor.product {
                "상품별 운임"
            }else{
                "배송 그룹 운임 수정"
            });
        });
        if !editor.product {
            ui.label(typography::status(
                "이 기준을 변경하면 같은 동급 그룹에 지정한 모든 모델에 적용됩니다.",
                theme::SELECTED_ORDER
            ));
        }
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|line|{
            line.label(typography::field_label("최대 묶음포장 수량"));
            line.add_enabled_ui(enabled,|input|{
                egui::ComboBox::from_id_salt("freight-rate-max")
                    .selected_text(editor.max_bundle
                        .map(|v|format!("{v}개")).unwrap_or_else(||"미확정".into()))
                    .show_ui(input,|choice|{
                        choice.selectable_value(&mut editor.max_bundle,None,"미확정");
                        for n in 1..=6 {
                            choice.selectable_value(&mut editor.max_bundle,Some(n),format!("{n}개"));
                        }
                    });
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
                line.label(typography::field_label(&format!("{count}개 묶음")));
                components::money_field(line,amount,enabled);
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
                line.add_enabled_ui(enabled,|input|{
                    egui::ComboBox::from_id_salt("freight-rate-add-count")
                        .selected_text(editor.add_count
                            .map(|n|format!("{n}개")).unwrap_or_else(||"묶음 수량 선택".into()))
                        .show_ui(input,|choice|{
                            for count in &available{
                                choice.selectable_value(
                                    &mut editor.add_count,Some(*count),format!("{count}개")
                                );
                            }
                        });
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
        ui.add_enabled_ui(enabled,|input|{
            components::flexible_text_field(
                input,&mut editor.notes,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                "포장할 때 참고할 사항 (선택)"
            );
        });
        ui.add_space(theme::SPACE_12);
        ui.horizontal(|bar|{
            if theme::action(bar,"닫기",theme::ButtonRole::Standard,enabled).clicked(){
                if editor.dirty(){editor.ask_discard=true;}
                else{*close=true;}
            }
            if theme::action(bar,"확인한 운임 저장",theme::ButtonRole::Primary,
                enabled&&editor.dirty()).clicked(){
                match editor.write_action(){
                    Ok(command)=>{
                        editor.error=None;
                        *event=Some(command);
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
                return;
            }
            egui::ScrollArea::both()
                .id_salt("freight-verified-combination-table")
                .max_height(section.ctx().content_rect().height()
                    * theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
                .auto_shrink([false,false])
                .show(section,|table|{
                    table.set_min_width(theme::MANAGEMENT_TABLE_MIN_WIDTH);
                    theme::apply_table_row_colors(table);
                    egui::Grid::new("freight-combination-history-grid")
                        .striped(true)
                        .spacing([theme::SPACE_24,theme::SPACE_8])
                        .show(table,|grid|{
                            for heading in ["조합","청구 운임","실제 운임","기준","수정"] {
                                grid.label(typography::table_header(heading));
                            }
                            grid.end_row();
                            for record in combinations.iter().rev() {
                                let valid=projection["currentCombinations"]
                                    .get(text(record,"id"))
                                    .and_then(Value::as_bool);
                                grid.vertical(|cell|{
                                    cell.label(typography::table_cell(
                                        text(record,"description")
                                    ));
                                    let at=text(record,"at");
                                    let verification=if !text(record,"verifiedAt").is_empty(){
                                        format!("실배송 검증 {}회",
                                            record["sampleCount"].as_u64().unwrap_or(1))
                                    }else{
                                        "사용자 등록 기준".to_owned()
                                    };
                                    cell.label(typography::caption(&format!(
                                        "{} / {}",
                                        if at.is_empty(){"날짜 미확인"}else{at},
                                        verification
                                    )));
                                });
                                grid.label(typography::quantity(&as_fee(&record["charge"])));
                                grid.label(typography::quantity(&as_fee(&record["actual"])));
                                grid.label(typography::table_cell(match valid {
                                    Some(true)=>"현재 유효",
                                    Some(false)=>"과거 이력",
                                    None=>"운임 기준 조회 중"
                                }));
                                if theme::action(grid,"예외 운임 변경",
                                    theme::ButtonRole::Standard,enabled).clicked(){
                                    *exception=Some((
                                        text(record,"id").into(),
                                        record["charge"].as_i64()
                                            .map(|n|n.to_string()).unwrap_or_default()
                                    ));
                                }
                                grid.end_row();
                            }
                        });
                });
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
                            let fees=(1..=6).filter_map(|n|{
                                old["fees"].get(n.to_string()).map(|amount|
                                    format!("{n}개 {}",
                                        if amount.is_null(){"미확정".to_owned()}
                                        else{as_fee(amount)})
                                )
                            }).collect::<Vec<_>>();
                            body.label(typography::body(&format!(
                                "{} / 개당 {} / {}",
                                text(old, "at"),as_fee(&old["perItem"]),
                                if fees.is_empty(){"운임 미등록".to_owned()}
                                else{fees.join(" / ")}
                            )));
                        }
                    });
            }
        });
    }

}

fn summary(ui: &mut Ui, label: &str, count: usize) {
    let card_width=theme::FREIGHT_SUMMARY_CARD_WIDTH;
    ui.allocate_ui_with_layout(
        egui::vec2(card_width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |slot|{
            slot.set_width(card_width);
            theme::section_frame().show(slot,|cell|{
                let content_width=card_width-2.0*theme::SECTION_PADDING
                    -2.0*theme::FRAME_STROKE_WIDTH;
                // The frame margins plus this single 22pt content row
                // preserve the existing 56pt card height. Every glyph is
                // positioned against ONE centerline, not nested rows whose
                // baseline and allocation depend on text length.
                let (line,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,22.0),egui::Sense::hover()
                );
                let caption=typography::summary_card_label_galley(
                    cell,label
                );
                let digits=typography::summary_value_galley(
                    cell,&count.to_string()
                );
                let unit=typography::summary_card_unit_galley(
                    cell,"개"
                );
                // Right-align the number to the unit: "358개" remains
                // adjacent, and the last digit shares the same x-position
                // for 0, 358, or a five-digit quantity.
                let unit_x=line.right()-unit.size().x;
                let digits_x=unit_x-theme::SPACE_4-digits.size().x;
                let painter=cell.painter();
                let y=line.center().y;
                painter.galley(
                    egui::pos2(line.left(),y-caption.size().y/2.0),
                    caption,theme::SECONDARY
                );
                painter.galley(
                    egui::pos2(digits_x,y-digits.size().y/2.0),
                    digits,theme::TEXT
                );
                painter.galley(
                    egui::pos2(unit_x,y-unit.size().y/2.0),
                    unit,theme::SECONDARY
                );
            });
        }
    );
}
