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
    group_from_product: bool,
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
            product:false,group_from_product:false
        }
    }
    fn new_product(id:&str,name:&str,rule:&Value)->Self {
        let mut editor=Self::new(id,name,rule);
        editor.product=true;
        editor
    }
    fn group_from_product(id:&str,name:&str,rule:&Value)->Self {
        let mut editor=Self::new(id,name,rule);
        editor.group_from_product=true;
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
    product_reset_target: Option<String>,
    copy_target: Option<(String,Vec<String>)>,
    copy_template: String,
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
            let outcome=(|| {
                let state=gateway.load_business_state()?;
                let shipping=state.get("shipping")
                    .filter(Value::is_object)
                    .ok_or("운영 DB에서 운임 기준을 찾을 수 없습니다.")?;
                let mut projection=gateway.execute(
                    ReadOperation::ShippingManagerView,
                    json!({"state":state,"shipping":shipping})
                )?;
                let product_view=gateway.execute(
                    ReadOperation::ProductFreightView,
                    json!({"action":"product_freight_view","state":{
                        "products":state["products"],"shipping":shipping
                    }})
                )?;
                projection["productViews"]=product_view["views"].clone();
                Ok((state,projection))
            })();
            let _=sender.send(outcome);
            ctx.request_repaint();
        });
        receiver
    }
    pub fn refresh(&mut self,ctx:egui::Context) {
        if self.receiver.is_none() && !self.busy() && !self.dirty() {
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
            bulk_group: String::new(),
            new_name: String::new(),
            rename_target: None,
            delete_target: None,
            rate_editor: None,
            basis_target: None,
            exception_target: None,
            product_reset_target: None,
            copy_target: None,
            copy_template: String::new(),
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
    pub fn can_save_pending(&self)->bool{
        self.gateway.shipping_write_enabled() && !self.busy() &&
            self.rate_editor.is_none() && self.new_name.trim().is_empty() &&
            !self.pending_changes().is_empty()
    }
    pub fn has_modal(&self)->bool{
        self.rate_editor.is_some() || self.rename_target.is_some()
            || self.delete_target.is_some() || self.basis_target.is_some()
            || self.exception_target.is_some() || self.product_reset_target.is_some()
            || self.copy_target.is_some() || self.requested_tab.is_some()
    }
    pub fn discard_drafts(&mut self){
        self.pending.clear();
        self.selected.clear();
        self.new_name.clear();
        self.rate_editor=None;
        self.rename_target=None;
        self.delete_target=None;
        self.basis_target=None;
        self.exception_target=None;
        self.product_reset_target=None;
        self.copy_target=None;
        self.requested_tab=None;
        self.saving_for_tab=false;
    }
    pub fn save_pending(&mut self, ctx:egui::Context)->bool{
        if self.busy() || self.rate_editor.is_some() || !self.new_name.trim().is_empty(){
            return false;
        }
        let changes=self.pending_changes();
        if changes.is_empty(){return true;}
        self.write("assign_products".into(),json!({"assignments":changes}),ctx);
        self.busy()
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
                            "copy_group_rule"=>"선택 상품에 확인된 공통 운임 기준을 복사했습니다.",
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
                            "save_group_rule"|"save_product_rule"=>self.rate_editor=None,
                            "reset_product_freight_rule"=>self.product_reset_target=None,
                            "copy_group_rule"=>{
                                self.copy_target=None;
                                self.selected.clear();
                            },
                            "change_basis"=>self.basis_target=None,
                            "revise_combination"=>self.exception_target=None,
                            _=>{}
                        }
                        if self.saving_for_tab{
                            if !self.dirty(){
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
        ui.label(typography::page_heading("운임 관리"));
        ui.add_space(theme::SPACE_12);
        if let Some(index) = components::management_tabs(ui, &[
            ("배송 그룹 편성", self.tab == FreightTab::Groups, true),
            ("그룹 운임표", self.tab == FreightTab::Rates, true),
            ("조합 / 기준 이력", self.tab == FreightTab::History, true),
        ]) {
            if !self.has_modal() && !self.busy() {
                let next=FreightTab::ALL[index];
                if self.tab!=next {
                    if self.dirty() { self.requested_tab=Some(next); }
                    else { self.tab=next; }
                }
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
            bar.label(typography::caption(&format!(
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
                    &mut self.only_missing, &projection["editableRules"],
                    &mut self.pending, &mut self.selected,
                    &mut self.bulk_group, &mut self.new_name,
                    &mut self.rename_target, &mut self.delete_target,
                    &mut self.rate_editor, &mut self.product_reset_target,
                    &mut self.copy_template, &mut self.copy_target,
                    &mut self.error, &self.gateway, &projection["productViews"],
                    enabled, &mut action
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
            let mut close_product_reset=false;
            let mut close_copy=false;
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
            }else if let Some((group_id,ids))=self.copy_target.as_ref(){
                let group=group_name(&groups,group_id);
                components::modal_backdrop(ctx,"freight-copy-backdrop",false);
                components::decision_window(
                    "상품별 운임 기준 복사","freight-copy-confirm",area
                ).show(ctx,|dialog|{
                    dialog.label(typography::body(&format!(
                        "선택 상품의 개별 운임표에 {group} 기준을 복사할까요? 동급 그룹 묶음은 별도로 지정합니다."
                    )));
                    dialog.horizontal(|bar|{
                        if theme::action(bar,"취소",
                            theme::ButtonRole::Standard,enabled).clicked(){
                            close_copy=true;
                        }
                        if theme::action(bar,"상품별 기준으로 복사",
                            theme::ButtonRole::Primary,enabled).clicked(){
                            action=Some(("copy_group_rule".into(),json!({
                                "groupId":group_id,"productIds":ids
                            })));
                        }
                    });
                });
            }
            if close_rename{self.rename_target=None;}
            if close_delete{self.delete_target=None;}
            if close_product_reset{self.product_reset_target=None;}
            if close_copy{self.copy_target=None;}
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
        if let Some(next)=self.requested_tab {
            let mut cancel=false;
            let mut discard=false;
            let mut save=false;
            let can_save=enabled&&self.new_name.trim().is_empty()
                && !self.pending_changes().is_empty();
            let ctx=ui.ctx();
            components::modal_backdrop(ctx,"freight-tab-leave-backdrop",false);
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
                let changes=self.pending_changes();
                if !changes.is_empty(){
                    self.saving_for_tab=true;
                    action=Some(("assign_products".into(),json!({
                        "assignments":changes
                    })));
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
        search: &mut String, group_filter: &mut String,
        only_missing:&mut bool, group_rules:&Value,
        pending: &mut BTreeMap<String,String>, selected: &mut BTreeSet<String>,
        bulk_group: &mut String, new_name: &mut String,
        rename_target: &mut Option<(String,String)>,
        delete_target: &mut Option<String>,
        rate_editor: &mut Option<RateEditor>,
        reset_target: &mut Option<String>,
        copy_template: &mut String,
        copy_target: &mut Option<(String,Vec<String>)>,
        error: &mut Option<String>,
        gateway: &Arc<dyn BandoGateway>,
        product_views: &Value, enabled:bool,
        action:&mut Option<(String,Value)>,
    ) {
        theme::section_frame().show(ui, |section| {
            section.horizontal(|tools|{
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
            components::soft_divider(section);
            for group in groups {
                section.horizontal(|line|{
                    line.label(typography::body(text(group,"name")));
                    let count=products.iter()
                        .filter(|p|text(p,"shippingGroupId")==text(group,"id"))
                        .count();
                    line.label(typography::caption(&format!("{count}개 상품")));
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
                let before_filter=group_filter.clone();
                egui::ComboBox::from_id_salt("freight-group-filter")
                    .selected_text(if group_filter.is_empty() {
                        "전체 상품"
                    } else if group_filter.as_str() == "none" {
                        "그룹 미지정"
                    } else { group_name(groups,group_filter) })
                    .show_ui(tools, |combo| {
                        combo.selectable_value(group_filter,String::new(),"전체 상품");
                        combo.selectable_value(group_filter,"none".into(),"그룹 미지정만");
                        for group in groups {
                            combo.selectable_value(
                                group_filter,text(group,"id").into(),text(group,"name")
                            );
                        }
                    });
                if before_filter!=*group_filter {selected.clear();}
                tools.checkbox(only_missing,"개별 기준 / 그룹 미지정만");
            });
            components::soft_divider(section);
            let needle=search.to_lowercase().split_whitespace().collect::<String>();
            let filtered:Vec<&Value>=products.iter().copied().filter(|p|{
                (needle.is_empty()||[text(p,"code"),text(p,"name"),
                    text(p,"subtitle"),text(p,"category")]
                    .iter().any(|s|s.to_lowercase().split_whitespace()
                        .collect::<String>().contains(&needle)))
                && (group_filter.is_empty()
                    || (group_filter.as_str()=="none" && !groups.iter().any(|g|
                        text(g,"id")==text(p,"shippingGroupId")))
                    || text(p,"shippingGroupId")==group_filter.as_str())
                && (!*only_missing || product_views.get(text(p,"id"))
                    .is_some_and(|view|view["rule"].is_null()))
            }).collect();
            section.horizontal_wrapped(|tools|{
                let mut all=!filtered.is_empty()
                    && filtered.iter().all(|p|selected.contains(text(p,"id")));
                tools.add_enabled_ui(enabled,|control|{
                    if control.checkbox(&mut all,"").changed(){
                        selected.clear();
                        if all {
                            selected.extend(filtered.iter()
                                .map(|p|text(p,"id").to_owned()));
                        }
                    }
                });
                tools.label(typography::field_label(&format!(
                    "{}개 선택",selected.len()
                )));
                tools.add_enabled_ui(enabled,|control|{
                    egui::ComboBox::from_id_salt("freight-bulk-assign")
                        .selected_text(if bulk_group.is_empty(){
                            "미지정으로 변경"
                        }else{group_name(groups,bulk_group)})
                        .show_ui(control,|combo|{
                            combo.selectable_value(bulk_group,String::new(),"미지정으로 변경");
                            for group in groups{
                                combo.selectable_value(
                                    bulk_group,text(group,"id").into(),text(group,"name")
                                );
                            }
                        });
                });
                if theme::action(tools,"선택 상품에 적용",
                    theme::ButtonRole::Standard,enabled&&!selected.is_empty()
                ).clicked(){
                    for id in selected.iter(){
                        pending.insert(id.clone(),bulk_group.clone());
                    }
                }
            });
            section.horizontal_wrapped(|tools|{
                let mut available=groups.iter().any(|g|text(g,"id")==copy_template.as_str());
                if !available {copy_template.clear();available=false;}
                tools.add_enabled_ui(enabled,|control|{
                    egui::ComboBox::from_id_salt("freight-copy-group-tariff")
                        .selected_text(if available{group_name(groups,copy_template)}
                            else{"확인된 공통 기준 선택"})
                        .show_ui(control,|choice|{
                            for group in groups {
                                choice.selectable_value(
                                    copy_template,text(group,"id").to_owned(),
                                    text(group,"name")
                                );
                            }
                        });
                });
                let can_copy=enabled&&!copy_template.is_empty()&&!selected.is_empty()
                    && selected.iter().all(|id|
                        products.iter().any(|p|text(p,"id")==id.as_str()));
                if theme::action(tools,"상품별 기준으로 복사",
                    theme::ButtonRole::Standard,can_copy).clicked(){
                    *copy_target=Some((
                        copy_template.clone(),selected.iter().cloned().collect()
                    ));
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
                            grid.label(typography::table_header("색상 / 옵션"));
                            grid.label(typography::table_header("배송 그룹"));
                            grid.label(typography::table_header("최대 묶음 수량"));
                            grid.label(typography::table_header("운임 등록 상태"));
                            grid.label(typography::table_header("합포장"));
                            grid.label(typography::table_header("최근 수정"));
                            grid.label(typography::table_header("지정 상태"));
                            grid.label(typography::table_header("설정"));
                            grid.end_row();
                            for product in &filtered{
                                let id=text(product,"id");
                                let committed=text(product,"shippingGroupId");
                                let mut checked=selected.contains(id);
                                grid.add_enabled_ui(enabled,|control|{
                                    if control.checkbox(&mut checked,"").changed(){
                                        if checked{selected.insert(id.into());}
                                        else{selected.remove(id);}
                                    }
                                });
                                grid.vertical(|cell|{
                                    cell.label(typography::table_cell(
                                        model_label(text(product,"name"))
                                    ));
                                    let description=text(product,"subtitle");
                                    if !description.is_empty(){
                                        cell.label(typography::caption(description));
                                    }
                                });
                                grid.label(typography::table_cell(
                                    if text(product,"category").is_empty(){
                                        "분류 미지정"
                                    }else{text(product,"category")}
                                ));
                                grid.label(typography::table_cell(&variant_colors(product)));
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
                                let effective=product_views.get(id)
                                    .filter(|entry|entry.is_object());
                                let rule=effective.map(|entry|&entry["rule"])
                                    .unwrap_or(&Value::Null);
                                let ready=effective.is_some();
                                let confirmed=rule["maxConfirmed"]==true;
                                let count=rule["maxBundle"].as_i64();
                                grid.label(typography::table_cell(
                                    if !ready{"확인 중"}
                                    else if confirmed {
                                        match count{
                                            Some(1)=>"1개",
                                            Some(2)=>"2개",
                                            Some(3)=>"3개",
                                            Some(4)=>"4개",
                                            Some(5)=>"5개",
                                            Some(6)=>"6개",
                                            _=>"확인 필요"
                                        }
                                    }else{"확인 필요"}
                                ));
                                grid.label(typography::table_cell(
                                    if !ready{"확인 중"}
                                    else if registered_fee(rule){"등록"}else{"미등록"}
                                ));
                                grid.label(typography::table_cell(
                                    if !ready{"확인 중"}
                                    else if !confirmed || count.is_none(){"확인 필요"}
                                    else if count==Some(1){"개별 포장"}
                                    else{"합포장 가능"}
                                ));
                                grid.label(typography::table_cell(
                                    modified_date(&rule["at"])
                                ));
                                grid.label(typography::table_cell(
                                    if changed{"변경 예정"}
                                    else if committed.is_empty(){"미지정"}
                                    else{"지정 완료"}
                                ));
                                grid.horizontal_wrapped(|cell|{
                                    let editable=enabled&&!changed&&ready;
                                    if let Some(group)=groups.iter().find(|g|
                                        text(g,"id")==committed){
                                        if theme::action(cell,"그룹 기준 편집",
                                            theme::ButtonRole::Standard,editable).clicked(){
                                            let group_rule=group_rules.get(committed)
                                                .unwrap_or(&group["current"]);
                                            *rate_editor=Some(RateEditor::group_from_product(
                                                committed,text(group,"name"),group_rule
                                            ));
                                        }
                                    }else if theme::action(cell,"기본 운임 설정",
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
                                    if rule.is_object() && theme::action(cell,"기준 변경",
                                        theme::ButtonRole::Standard,editable).clicked(){
                                        *reset_target=Some(id.to_owned());
                                    }
                                });
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
                actions.label(typography::caption(&format!(
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
        theme::section_frame().show(ui,|section|{
            egui::ScrollArea::horizontal()
                .id_salt("freight-group-rates-table")
                .auto_shrink([false,false])
                .show(section,|table|{
                    table.set_min_width(theme::MANAGEMENT_TABLE_MIN_WIDTH);
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
        ui.label(typography::subsection_heading(&format!("{} 운임",editor.name)));
        if editor.group_from_product {
            ui.label(typography::status(
                "이 기준을 변경하면 같은 동급 그룹에 지정한 모든 모델에 적용됩니다.",
                theme::SELECTED_ORDER
            ));
        }
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|line|{
            line.label(typography::field_label("최대 합포장 수량"));
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
                        *event=Some((
                            if editor.product {"save_product_rule"} else {"save_group_rule"}.into(),
                            if editor.product {
                                json!({"productId":editor.id,"rule":rule})
                            } else {
                                json!({"groupId":editor.id,"rule":rule})
                            }
                        ));
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
                    row.label(typography::quantity(&format!(
                        "청구 {}",as_fee(&record["charge"])
                    )));
                    row.label(typography::quantity(&format!(
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
                            body.label(typography::body(&format!(
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
        cell.label(typography::summary_value(&count.to_string()));
    });
}
