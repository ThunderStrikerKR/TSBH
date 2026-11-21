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
    Basis,
}
impl FreightTab {
    const ALL: [Self; 4] = [
        Self::Groups, Self::Rates, Self::History, Self::Basis
    ];
    const LABELS: [&'static str; 4] = [
        "배송 그룹 편성", "배송 그룹 관리",
        "조합 / 기준 이력", "운임 기준 관리"
    ];
}

// Sort only meaningful text columns. No sorting control on selection or
// the per-product action column; default keeps unassigned groups first.
#[derive(Clone,Copy,PartialEq,Eq)]
enum FreightSortColumn { Product, ColorOption, Category, ShippingGroup }

#[derive(Clone,Copy,PartialEq,Eq)]
enum FreightSortDirection { Ascending, Descending }

#[derive(Clone,Copy,PartialEq,Eq)]
struct FreightSort {
    column: FreightSortColumn,
    direction: FreightSortDirection,
}

impl FreightSort {
    fn column_at(index:usize)->Option<FreightSortColumn>{
        match index {
            1=>Some(FreightSortColumn::Product),
            2=>Some(FreightSortColumn::ColorOption),
            3=>Some(FreightSortColumn::Category),
            4=>Some(FreightSortColumn::ShippingGroup),
            _=>None,
        }
    }

    /// One header click advances: default -> ascending -> descending ->
    /// default. Clicking a different column begins with ascending.
    fn next(previous:Option<Self>,column:FreightSortColumn)->Option<Self>{
        match previous {
            Some(Self{column:active,direction:FreightSortDirection::Ascending})
                if active==column=>Some(Self{
                    column,direction:FreightSortDirection::Descending
                }),
            Some(Self{column:active,direction:FreightSortDirection::Descending})
                if active==column=>None,
            _=>Some(Self{column,direction:FreightSortDirection::Ascending}),
        }
    }
}

// Group management reuses the assignment table's 3-step sort interaction.
// Sorting changes presentation only, not the stored order of shipping groups.
#[derive(Clone,Copy,PartialEq,Eq)]
enum RateSortColumn { Group, Bundle }

#[derive(Clone,Copy,PartialEq,Eq)]
struct RateSort {
    column:RateSortColumn,
    direction:FreightSortDirection,
}
impl RateSort {
    fn column_at(index:usize)->Option<RateSortColumn>{
        match index {
            0=>Some(RateSortColumn::Group),
            1=>Some(RateSortColumn::Bundle),
            _=>None,
        }
    }
    fn next(previous:Option<Self>,column:RateSortColumn)->Option<Self>{
        match previous {
            Some(Self{column:active,direction:FreightSortDirection::Ascending})
                if active==column=>Some(Self{
                    column,direction:FreightSortDirection::Descending
                }),
            Some(Self{column:active,direction:FreightSortDirection::Descending})
                if active==column=>None,
            _=>Some(Self{column,direction:FreightSortDirection::Ascending}),
        }
    }
}

// History sort is visual only. Reset returns to newest-first order and the
// original group-version order remains untouched.
#[derive(Clone,Copy,PartialEq,Eq)]
enum HistorySortColumn { Combination, RecordedAt, Charge, Actual, Basis }
#[derive(Clone,Copy,PartialEq,Eq)]
struct HistorySort {
    column:HistorySortColumn,
    direction:FreightSortDirection,
}
impl HistorySort {
    fn column_at(index:usize)->Option<HistorySortColumn>{
        match index {
            0=>Some(HistorySortColumn::Combination),
            1=>Some(HistorySortColumn::RecordedAt),
            2=>Some(HistorySortColumn::Charge),
            3=>Some(HistorySortColumn::Actual),
            4=>Some(HistorySortColumn::Basis),
            _=>None,
        }
    }
    fn next(previous:Option<Self>,column:HistorySortColumn)->Option<Self>{
        match previous {
            Some(Self{column:active,direction:FreightSortDirection::Ascending})
                if active==column=>Some(Self{
                    column,direction:FreightSortDirection::Descending
                }),
            Some(Self{column:active,direction:FreightSortDirection::Descending})
                if active==column=>None,
            _=>Some(Self{column,direction:FreightSortDirection::Ascending}),
        }
    }
}

/// Compute keys before comparing: sorting large tables must not rebuild
/// category paths or variant labels on each comparison.
fn freight_sort_key(
    product:&Value,column:FreightSortColumn,groups:&[Value],
    category_paths:&ProductCategoryPaths
)->(String,String){
    let value=match column {
        FreightSortColumn::Product=>{
            let model=model_label(text(product,"name"));
            let subtitle=text(product,"subtitle").trim();
            if subtitle.is_empty()||subtitle==text(product,"name"){
                model.to_owned()
            }else{format!("{model} / {subtitle}")}
        },
        FreightSortColumn::ColorOption=>variant_colors(product),
        FreightSortColumn::Category=>category_paths.for_product(product).to_owned(),
        FreightSortColumn::ShippingGroup=>{
            let assigned=text(product,"shippingGroupId");
            if assigned.is_empty(){"그룹 미지정".to_owned()}
            else{group_name(groups,assigned).to_owned()}
        },
    };
    (value.to_lowercase(),text(product,"id").to_owned())
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
/// Shared geometry for header and item rows. Each label/control uses this
/// exact column rectangle; no intrinsic egui Grid sizing can shift a column.
fn freight_table_cells<const N:usize>(
    origin: egui::Pos2, widths: &[f32;N], height:f32
) -> [egui::Rect;N] {
    let mut x=origin.x;
    std::array::from_fn(|index|{
        let cell=egui::Rect::from_min_size(
            egui::pos2(x,origin.y),egui::vec2(widths[index],height)
        );
        x+=widths[index]+theme::FREIGHT_ASSIGNMENT_COL_GAP;
        cell
    })
}

/// These table cells use the shared 14pt semantic regular-data roles.
/// Only headings and explicit final results retain strong emphasis.

/// One non-wrapping line anchored 6pt from the shared column origin. Keep
/// the full content on hover without changing the cell's layout footprint.
fn freight_assignment_text(
    ui:&mut Ui,rect:egui::Rect,value:&str,full_text:&str,
    role:typography::ManagementTableCellRole,id:impl std::hash::Hash
) {
    let clipped=rect.shrink2(egui::vec2(theme::MANAGEMENT_TABLE_TEXT_LEADING_INSET,0.0));
    let painter=ui.painter().with_clip_rect(clipped);
    match role {
        typography::ManagementTableCellRole::Category=>{
            // Show ancestry as muted context with a readable, regular leaf.
            // Keep the original category text intact on hover.
            let (prefix,leaf)=match value.rfind('>'){
                Some(index) if !value[index+1..].trim().is_empty() =>
                    value.split_at(index+1),
                _=>("",value),
            };
            let prefix_galley=typography::management_table_cell_galley(
                ui,prefix,typography::ManagementTableCellRole::Muted
            );
            let leaf_galley=typography::management_table_cell_galley(
                ui,leaf,typography::ManagementTableCellRole::Category
            );
            let y=clipped.center().y-prefix_galley.size().y.max(
                leaf_galley.size().y
            )/2.0;
            painter.galley(
                egui::pos2(clipped.left(),y),
                prefix_galley.clone(),theme::MUTED
            );
            painter.galley(
                egui::pos2(clipped.left()+prefix_galley.size().x,y),
                leaf_galley,theme::SECONDARY
            );
        }
        other=>{
            let galley=typography::management_table_cell_galley(ui,value,other);
            let color=typography::management_table_cell_color(other);
            painter.galley(
                egui::pos2(
                    clipped.left(),
                    clipped.center().y-galley.size().y/2.0
                ),galley,color
            );
        }
    }
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
fn as_fee(value: &Value) -> String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(|| "미등록".to_owned())
}
 // Text, numeric, status and metadata share one fixed-height cell geometry.
fn freight_rate_cell(
    ui:&mut Ui,rect:egui::Rect,value:&str,align:egui::Align2,
    role:typography::ManagementTableCellRole,id:impl std::hash::Hash
){
    let cell=rect.shrink2(egui::vec2(6.0,0.0));
    let color=typography::management_table_cell_color(role);
    let galley=typography::management_table_cell_galley(ui,value,role);
    let x=if align==egui::Align2::RIGHT_CENTER {
        cell.right()-galley.size().x
    }else if align==egui::Align2::CENTER_CENTER {
        cell.center().x-galley.size().x/2.0
    }else{
        cell.left()
    };
    ui.painter().with_clip_rect(cell).galley(
        egui::pos2(x,cell.center().y-galley.size().y/2.0),galley,color
    );
    if !value.is_empty(){
        ui.interact(cell,ui.make_persistent_id(("freight-rate-cell",id)),
            egui::Sense::hover()).on_hover_text(value);
    }
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
    create_group_open: bool,
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
    sort: Option<FreightSort>,
    rate_sort: Option<RateSort>,
    rate_search: String,
    history_sort: Option<HistorySort>,
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
            create_group_open: false,
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
            sort: None,
            rate_sort: None,
            rate_search: String::new(),
            history_sort: None,
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
        self.create_group_open || self.rename_target.is_some() ||
            self.delete_target.is_some() || self.basis_target.is_some() ||
            self.exception_target.is_some() ||
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
        self.create_group_open=false;
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
                            "create_group"=>{
                                self.new_name.clear();
                                self.create_group_open=false;
                            },
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
        crate::floating_notice::dispatch_pointer(&ctx,"freight.action");
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
        let page_origin_y=ui.cursor().top();
        // Non-floating, always legible scrollbar geometry for every freight
        // management tab. This is scoped to the freight page, not global nav.
        ui.spacing_mut().scroll=theme::management_scroll_style();
        self.poll(ui.ctx().clone());
        crate::floating_notice::sync(
            ui.ctx(),"freight.result","freight.action",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"freight.error","freight.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        if self.saving_for_tab && !self.busy() && self.dirty() &&
            !self.save_dirty(ui.ctx().clone()) {
            self.saving_for_tab=false;
        }
        // The workspace shell already provides 6pt above this page.
        // Add only the missing amount: heading top is 16pt below the
        // global navigation, not 16pt ON TOP of the shell's existing 6pt.
        components::management_page_title_top_gap(ui);
        freight_top_content(ui,|top|{
            top.horizontal(|heading|{
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
        });
        // Progressive vertical rhythm: 8pt heading -> tab,
        // 12pt tab -> KPI, 16pt KPI -> main content card.
        // This absorbs the new title-top padding without moving the
        // shared primary-card border below its 59th-round position.
        ui.add_space(theme::SPACE_8);
        let selected_tab=freight_top_content(ui,|top|
            components::management_tabs(top, &[
                (FreightTab::LABELS[0], self.tab == FreightTab::Groups, true),
                (FreightTab::LABELS[1], self.tab == FreightTab::Rates, true),
                (FreightTab::LABELS[2], self.tab == FreightTab::History, true),
                (FreightTab::LABELS[3], self.tab == FreightTab::Basis, true),
            ])
        );
        if let Some(index) = selected_tab {
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
        // KPI cards follow the same 16pt content guide as the page heading
        // and sub-tabs. Their individual 228pt widths and 8pt gaps stay intact.
        // On narrow panes the four complete cards still wrap, never compress.
        freight_top_content(ui,|top|{
            top.horizontal_wrapped(|strip|{
                strip.spacing_mut().item_spacing=egui::vec2(
                    theme::SPACE_8,theme::SPACE_8
                );
                summary(strip,"등록 상품",products.len());
                summary(strip,"그룹 지정 완료",assigned);
                summary(strip,"그룹 미지정",products.len()-assigned);
                summary(strip,"배송 그룹",groups.len());
            });
        });
        // One 16pt boundary between the stats strip and the next section.
        // show_groups() must not add a second top margin on top of this.
        ui.add_space(theme::SPACE_16);
        // Keep the approved Freight Assignment as the layout reference,
        // including when the four KPIs wrap in narrower workspaces.
        components::align_management_primary_card(ui,page_origin_y);
        let enabled=self.gateway.shipping_write_enabled()&&self.write_receiver.is_none();
        let mut action:Option<(String,Value)>=None;
        // The assignment tab owns its own scrollable *rows*. Do not wrap
        // the heading in a second page-wide vertical scroll, which previously
        // pushed the column headings offscreen with the product list.
        if self.tab == FreightTab::Groups {
            Self::show_groups(
                ui, &groups, &products, &category_paths,
                &mut self.search, &mut self.group_filter, &mut self.sort,
                &mut self.pending, &mut self.selected,
                &mut self.bulk_group, &mut self.bulk_feedback,
                &mut self.rate_editor, &mut self.product_reset_target,
                &mut self.error, &self.gateway, &projection["productViews"],
                enabled, &mut action
            );
        }else{
            match self.tab {
                // Management's table scrolls its rows only. A page-wide outer
                // scroll used to move the header away with the old duplicate list.
                FreightTab::Rates => Self::show_rates(
                    ui,&groups,&products,projection,
                    &mut self.create_group_open,&mut self.rename_target,
                    &mut self.delete_target,&mut self.rate_editor,
                    &mut self.basis_target,&mut self.rate_sort,
                    &mut self.rate_search,&mut self.error,enabled,&mut action
                ),
                FreightTab::History => {
                    egui::ScrollArea::vertical()
                        .id_salt(("freight-management",self.tab as u8))
                        .auto_shrink([false,false])
                        .show(ui,|scroll|Self::show_history(
                            scroll,shipping,projection,
                            &mut self.exception_target,&mut self.history_sort,
                            enabled
                        ));
                }
                FreightTab::Basis => Self::show_basis_management(
                    ui,shipping,enabled,&mut self.basis_target
                ),
                FreightTab::Groups => {}
            }
        }
        if self.tab==FreightTab::Groups || self.tab==FreightTab::Rates {
            // Group rename/delete confirmations belong to the management
            // tab, while individual product reset stays on assignment.
            let area=ui.max_rect();
            let ctx=ui.ctx();
            let mut close_create=false;
            let mut close_rename=false;
            let mut close_delete=false;
            let mut close_product_reset=false;
            if self.tab==FreightTab::Rates && self.create_group_open {
                components::modal_backdrop(ctx,"freight-create-backdrop",false);
                components::decision_window(
                    "배송 그룹 추가","freight-create",area
                ).show(ctx,|modal|{
                    if components::modal_header(
                        modal,"배송 그룹 추가","취소",enabled
                    ){close_create=true;}
                    modal.label(typography::field_label("배송 그룹명"));
                    modal.add_enabled_ui(enabled,|field|{
                        components::flexible_text_field(
                            field,&mut self.new_name,
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"그룹 이름 입력"
                        );
                    });
                    modal.add_space(theme::SPACE_12);
                    modal.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |controls|{
                            if theme::action(
                                controls,"배송 그룹 추가",theme::ButtonRole::Primary,
                                enabled&&!self.new_name.trim().is_empty()
                            ).clicked(){
                                action=Some(("create_group".into(),json!({
                                    "name":self.new_name.trim()
                                })));
                            }
                        }
                    );
                });
            }else if let Some((id,name))=self.rename_target.as_mut(){
                components::modal_backdrop(ctx,"freight-rename-backdrop",false);
                components::decision_window("배송 그룹명","freight-rename",area)
                    .show(ctx,|modal|{
                        if components::modal_header(modal,"배송 그룹명","취소",enabled){
                            close_rename=true;
                        }
                        components::field(modal,name,components::FieldKind::Text,enabled);
                        modal.add_space(theme::SPACE_12);
                        modal.with_layout(egui::Layout::right_to_left(egui::Align::Center),|controls|{
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
                        if components::modal_header(
                            modal,"배송 그룹 삭제","취소",enabled
                        ){close_delete=true;}
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
                        modal.with_layout(egui::Layout::right_to_left(egui::Align::Center),|controls|{
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
                    if components::modal_header(
                        dialog,"운임 기준 변경","취소",enabled
                    ){close_product_reset=true;}
                    dialog.label(typography::body(
                        "이 상품의 새 운임 기준을 시작하고 이전 자동 추천을 중단할까요?"
                    ));
                    dialog.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
                        if theme::action(bar,"기준 변경",
                            theme::ButtonRole::Danger,enabled).clicked(){
                            action=Some(("reset_product_freight_rule".into(),
                                json!({"productId":pid})));
                        }
                    });
                });
            }
            if close_create{
                self.create_group_open=false;
                self.new_name.clear();
            }
            if close_rename{self.rename_target=None;}
            if close_delete{self.delete_target=None;}
            if close_product_reset{self.product_reset_target=None;}
        }
        if self.rate_editor.is_some() || self.basis_target.is_some() ||
            self.tab==FreightTab::Rates {
            let area=ui.max_rect();
            let ctx=ui.ctx();
            let mut close_editor=false;
            let mut close_basis=false;
            if let Some(editor)=self.rate_editor.as_mut(){
                components::modal_backdrop(ctx,"freight-rate-backdrop",false);
                components::modal_window(if editor.product{"기본 운임 설정"}else{"배송 그룹 운임 수정"},"freight-rate-modal",area)
                    .resizable(false).show(ctx,|modal|{
                        let title=if editor.product{
                            "기본 운임 설정"
                        }else{"배송 그룹 운임 수정"};
                        if components::modal_header(
                            modal,title,"취소",enabled
                        ){
                            if editor.dirty(){editor.ask_discard=true;}
                            else{close_editor=true;}
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("freight-rate-editor-scroll")
                            .max_height(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
                            .show(modal,|body|{
                                Self::rate_dialog(
                                    body,editor,enabled,&mut action
                                );
                            });
                    });
                if editor.ask_discard {
                    components::modal_backdrop(ctx,"freight-discard-backdrop",true);
                    components::decision_window(
                        "저장하지 않은 운임","freight-discard-confirm",area
                    ).show(ctx,|dialog|{
                        if components::modal_header(
                            dialog,"저장하지 않은 운임","취소",enabled
                        ){editor.ask_discard=false;}
                        dialog.label(typography::body(
                            "운임 수정사항을 저장하지 않고 닫을까요?"
                        ));
                        dialog.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
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
                    if components::modal_header(
                        dialog,"운임 기준 변경","취소",enabled
                    ){close_basis=true;}
                    let warning=match name {
                        None=>"전체 운임 기준을 변경하면 이전 운임의 자동 추천을 중단합니다. 새 운임을 다시 확인해 등록해야 합니다.",
                        Some(_)=>"선택한 그룹의 이전 운임 추천을 중단하고 새 운임 기준으로 변경합니다."
                    };
                    dialog.label(typography::body(warning));
                    dialog.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
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
                    if components::modal_header(
                        dialog,"새로 확인된 청구 운임 (원)","취소",enabled
                    ){close=true;}
                    components::money_field(dialog,amount,enabled);
                    dialog.add_space(theme::SPACE_12);
                    dialog.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
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
                cancel=components::modal_header(
                    dialog,"저장하지 않은 변경사항","취소",enabled
                );
                dialog.label(typography::body(
                    "운임 관리에서 수정한 내용이 있습니다. 변경사항을 저장할까요?"
                ));

                dialog.add_space(theme::SPACE_12);
                dialog.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
                    if theme::action(bar,"저장하고 이동",
                        theme::ButtonRole::Primary,can_save).clicked(){save=true;}
                    if theme::action(bar,"저장 안 함",
                        theme::ButtonRole::Standard,enabled).clicked(){discard=true;}
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
        crate::floating_notice::sync(
            ui.ctx(),"freight.bulk.feedback","freight.action",
            crate::floating_notice::Tone::Info,self.bulk_feedback.as_deref()
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn show_groups(
        ui: &mut Ui, groups: &[Value], products: &[&Value],
        category_paths:&ProductCategoryPaths,
        search: &mut String, group_filter: &mut String,
        sort:&mut Option<FreightSort>,
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
        // Full-height SINGLE table: preserve 16pt padding on all four sides.
        // The data viewport AND the scrollbar rail stop above the same
        // bottom inset. Neither may reach the card's outer border.
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        theme::management_master_table_frame().show(ui, |section| {
            components::fill_management_primary_card(section,card_min_height);
            // Assignment is one continuous zebra table. The standard solid
            // scrollbar reserves 4pt of margin on EACH side of its rail,
            // creating a dark detached strip after the "설정" column.
            // Keep the 10pt thumb width but dock its rail flush with the
            // table rows. The 6pt inset on the last action cell still
            // separates clickable buttons from the scroll hit target.
            section.spacing_mut().scroll=theme::management_table_scroll_style();
            // Both Freight and Order History now reserve the toolbar, divider
            // and header with one common zero-implicit-gap layout path.
            let previous_table_spacing_y=section.spacing().item_spacing.y;
            section.spacing_mut().item_spacing.y=0.0;
            // The parent page title and active "배송 그룹 편성" tab already
            // establish context. Start directly with the search/bulk toolbar:
            // do not leave an empty title row or its implicit vertical gap.
            // Reserve a single toolbar rectangle. Left filters and right
            // bulk controls receive independent fixed-height child UIs.
            // Mixing left-to-right and right-to-left layouts in ONE row
            // previously advanced the second group's vertical origin.
            let toolbar_width=section.available_width();
            let (toolbar_rect,_) = section.allocate_exact_size(
                egui::vec2(toolbar_width,theme::INPUT_HEIGHT),
                egui::Sense::hover()
            );
            // Dual-master CONFLICT-001=C: align the rightmost toolbar
            // control with the CARD'S INNER RIGHT EDGE, like Order History.
            // Do not subtract the table-only 10pt scrollbar gutter here.
            // Keep the same 16pt separation between left and right tools.
            let toolbar_content_width=toolbar_rect.width().max(0.0);
            let bulk_width=500.0_f32.min(toolbar_content_width);
            let left_width=(toolbar_content_width-bulk_width-theme::SPACE_16).max(0.0);
            let left_rect=egui::Rect::from_min_size(
                toolbar_rect.left_top(),
                egui::vec2(left_width,toolbar_rect.height())
            );
            let right_rect=components::management_toolbar_trailing_rect(
                toolbar_rect,bulk_width,0.0,toolbar_rect.height()
            );
            // The right controls (including optional help) own the full
            // trailing slot; only the TABLE body/header reserve scroll space.
            let right_controls_rect=right_rect;
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
                    // Search stays at 36pt; the shared 34pt dropdown trigger
                    // is vertically centered within the same 36pt toolbar.
                    tools.spacing_mut().interact_size.y=theme::INPUT_HEIGHT;
                components::flexible_text_field(
                    tools,search,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                    "모델명 / 상품명 / 카테고리 검색"
                );
                let before_filter=group_filter.clone();
                components::management_dropdown(
                    tools,
                    if group_filter.is_empty() {
                        "전체 상품"
                    }else if group_filter.as_str()=="none" {
                        "그룹 미지정"
                    }else{group_name(groups,group_filter)},
                    theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH,true,"상품 표시 범위 선택",|menu|{
                        components::management_dropdown_option(
                            menu,group_filter,String::new(),"전체 상품"
                        );
                        components::management_dropdown_option(
                            menu,group_filter,"none".into(),"그룹 미지정만"
                        );
                        for group in groups {
                            components::management_dropdown_option(
                                menu,group_filter,text(group,"id").into(),
                                text(group,"name")
                            );
                        }
                    }
                );
                if before_filter!=*group_filter {
                    selected.clear();
                    *bulk_feedback=None;
                }
                // The filter menu already contains "그룹 미지정만".
                // A second checkbox is redundant and can contradict a chosen group.

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
                }).collect();
                // Apply the chosen column to the entire filtered set BEFORE
                // virtualizing; every page of the scroll area has one order.
                // Cached keys avoid repeated category traversal during sort.
                if let Some(active)=*sort {
                    filtered.sort_by_cached_key(|p|
                        freight_sort_key(p,active.column,groups,category_paths)
                    );
                    if active.direction==FreightSortDirection::Descending {
                        filtered.reverse();
                    }
                } else if needle.is_empty() && group_filter.is_empty() {
                    // Original default: group-unassigned products first.
                    // Use committed membership to avoid shifting rows mid-edit.
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
                .max_rect(right_controls_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center));
            if !enabled {
                bulk_builder=bulk_builder.disabled();
            }
            section.scope_builder(bulk_builder,|bulk|{
                    // Clip to the trailing card-inner slot, not the
                    // independent table scrollbar gutter.
                    bulk.set_clip_rect(right_rect);
                    bulk.set_width(right_controls_rect.width());
                    bulk.set_height(right_controls_rect.height());
                    bulk.spacing_mut().item_spacing.x=theme::SPACE_8;
                    // Bulk selection uses the same 34pt split-chevron
                    // trigger as the filter and every product-row selector.
                    bulk.spacing_mut().interact_size.y=theme::INPUT_HEIGHT;
                        settings_help::icon(bulk,"배송 그룹 일괄 지정");
                        if theme::action(bulk,"선택 상품에 적용",
                            theme::ButtonRole::Standard,
                            enabled&&!selected.is_empty()&&bulk_group.is_some()
                        ).clicked(){
                            if let Some(target)=bulk_group.as_deref(){
                                let (mut newly,mut moved,mut removed,mut unchanged)=(0,0,0,0);
                                let mut assignments=serde_json::Map::new();
                                for product in products.iter().copied()
                                    .filter(|product|selected.contains(text(product,"id"))) {
                                    let id=text(product,"id");
                                    let original=text(product,"shippingGroupId");
                                    if original==target {
                                        pending.remove(id);
                                        unchanged+=1;
                                    }else{
                                        pending.insert(id.to_owned(),target.to_owned());
                                        assignments.insert(id.to_owned(),json!(target));
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
                                // Bulk Apply IS the save action. No second
                                // '그룹 지정 저장' confirmation is required.
                                if !assignments.is_empty(){
                                    *action=Some(("assign_products".into(),json!({
                                        "assignments":assignments
                                    })));
                                }
                            }
                        }
                        components::management_dropdown(
                            bulk,
                            match bulk_group.as_deref(){
                                None=>"배송 그룹 선택",
                                Some("")=>"미지정으로 변경",
                                Some(id)=>group_name(groups,id),
                            },
                            theme::FREIGHT_BULK_GROUP_WIDTH,
                            enabled,"일괄 적용할 배송 그룹 선택",|menu|{
                                components::management_dropdown_option(
                                    menu,bulk_group,Some(String::new()),"미지정으로 변경"
                                );
                                for group in groups {
                                    components::management_dropdown_option(
                                        menu,bulk_group,Some(text(group,"id").to_owned()),
                                        text(group,"name")
                                    );
                                }
                            }
                        );
                        bulk.label(typography::field_label(&format!(
                            "{}개 선택",selected.len()
                        )));
                }
            );
            if *bulk_group!=previous_group {
                *bulk_feedback=None;
            }
            // Shared transition: 16pt after the 36pt toolbar, 1pt divider,
            // 4pt before the full-width sticky header, on the same cursor.
            let table_header=components::management_sticky_table_header(
                section,theme::FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH
            );
            let viewport_width=table_header.viewport_width;
            let scrollbar_gutter=table_header.scrollbar_gutter;
            let table_width=table_header.table_width;
            let header_rect=table_header.header_rect;
            let header_content_rect=table_header.header_content_rect;
            // Read the approved 6-column widths and 64/36 extra-width
            // allocation from the single master specification in theme.rs.
            let widths=theme::FREIGHT_ASSIGNMENT_MASTER_TABLE.widths_for(table_width);
            let titles=[
                "",components::ManagementTableColumnLabel::ProductName.text(),
                "색상/옵션","카테고리",
                "배송 그룹","설정"
            ];
            // Preserve the row viewport and card's full 16pt lower inset.
            // The rail connects to the header and extends 4pt past rows,
            // stopping 12pt short of the outer card border.
            let scroll_max_height=components::management_single_list_scroll_height(
                section,card_min_height,0.0
            );
            let scroll_track=components::management_table_scroll_track_rect(
                section,scroll_max_height
            );
            let mut first_row_left=None;
            let scroll_output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .scroll_bar_rect(scroll_track)
                    .id_salt("freight-assignment-rows")
                    // The solid track stays visible without hovering and
                    // takes its own space rather than covering row controls.
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                    )
                    .max_height(scroll_max_height)
                    .auto_shrink([false,false]),
                section,table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT,
                filtered.len(),
                |table,visible_rows|{
                    table.set_min_width(table_width);
                    table.spacing_mut().item_spacing.y=0.0;
                    theme::apply_table_row_colors(table);
                    // Rows outside the viewport keep their selections and
                    // pending assignments, but create no widget this frame.
                    // Absolute positioning is deliberate: nested egui
                    // scope_builder controls advance their PARENT cursor.
                    // When "기본 운임 설정" is absent, their measured
                    // footprints differ, so sequential row allocation
                    // compressed rows and left a large blank scroll area.
                    // A fixed rectangle based on the stable data index
                    // makes every row exactly 42pt, regardless of widgets.
                    let first_visible_row=visible_rows.start;
                    let row_origin=table.max_rect().min;
                    for row_index in visible_rows{
                        let product_id=text(filtered[row_index],"id");
                        let row_rect=egui::Rect::from_min_size(
                            egui::pos2(
                                row_origin.x,
                                row_origin.y+
                                    (row_index-first_visible_row) as f32
                                        *theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                            ),
                            egui::vec2(
                                table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                            )
                        );
                        first_row_left.get_or_insert(row_rect.left());
                        table.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(("freight-fixed-row",product_id))
                                .max_rect(row_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |row|{
                                // Reserve one full logical row before painting.
                                // Descendant controls are anchored to cells,
                                // never used to determine the next row's y.
                                let (_reserved,_)=row.allocate_exact_size(
                                    egui::vec2(
                                        table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                                    ),
                                    egui::Sense::hover()
                                );
                        let product=filtered[row_index];
                        let id=text(product,"id");
                        let committed=text(product,"shippingGroupId");
                        let mut checked=selected.contains(id);
                        let cells=freight_table_cells(
                            row_rect.min,&widths,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                        );
                        let background=if checked {theme::WORKFLOW_SELECTED_BG}
                            else if row_index%2==0 {theme::PANEL}
                            else {theme::TABLE_ROW_ALT_BG};
                        row.painter().rect_filled(row_rect,0.0,background);
                        if checked {
                            row.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    row_rect.min,
                                    egui::vec2(theme::WORKFLOW_SELECTED_STRIPE,row_rect.height())
                                ),0.0,theme::SELECTION
                            );
                        }

                        // Shared centered checkbox: same placement as Excel
                        // regardless of each table's first-column width.
                        if components::management_table_checkbox(
                            row,cells[0],cells[0],&mut checked,enabled,
                            ("freight-checkbox",id)
                        ).changed(){
                            if checked {selected.insert(id.to_owned());}
                            else {selected.remove(id);}
                            *bulk_feedback=None;
                        }

                        let model=model_label(text(product,"name"));
                        let subtitle=text(product,"subtitle").trim();
                        let product_title=if subtitle.is_empty()
                            || subtitle==text(product,"name") {
                            model.to_owned()
                        }else{format!("{model} / {subtitle}")};
                        freight_assignment_text(
                            row,cells[1],&product_title,&product_title,
                            typography::ManagementTableCellRole::ProductName,(id,1)
                        );
                        // The narrow third column is color/option. The wide
                        // fourth column is the full canonical category path.
                        let colors=variant_colors(product);
                        freight_assignment_text(
                            row,cells[2],&colors,&colors,typography::ManagementTableCellRole::Option,(id,2)
                        );
                        let category=category_paths.for_product(product);
                        let category=if category.trim().is_empty(){
                            "분류 미지정"
                        }else{category};
                        let visible_category=typography::fit_category_path(
                            row,category,
                            (cells[3].width()-theme::SPACE_12).max(0.0),
                            typography::CategoryPathFont::FreightTableCell
                        );
                        freight_assignment_text(
                            row,cells[3],&visible_category,category,
                            typography::ManagementTableCellRole::Category,(id,3)
                        );

                        // The group selector's own child UI shares the same
                        // left edge and vertical center as the painted cell.
                        let mut current=pending.get(id).cloned()
                            .unwrap_or_else(||committed.into());
                        let before_manual_edit=current.clone();
                        // A 34pt shared dropdown is centered exactly within
                        // the 42pt product row (4pt above and below).
                        // Its width is owned by the shipping-group cell;
                        // the popup does not change any table column width.
                        let selector_rect=egui::Rect::from_min_size(
                            egui::pos2(
                                cells[4].left()+theme::SPACE_4,
                                cells[4].top()
                                    +(theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                                        -theme::ACTION_STANDARD_HEIGHT)/2.0
                            ),
                            egui::vec2(
                                cells[4].width()-theme::SPACE_8,
                                theme::ACTION_STANDARD_HEIGHT
                            )
                        );
                        let selector_clip=cells[4].intersect(row.clip_rect());
                        let mut selector_builder=egui::UiBuilder::new()
                            .id_salt(("freight-selector",id))
                            .max_rect(selector_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Min));
                        if !enabled {
                            selector_builder=selector_builder.disabled();
                        }
                        row.scope_builder(selector_builder,|control|{
                                control.set_clip_rect(selector_clip);
                                control.set_width(selector_rect.width());
                                components::management_dropdown(
                                    control,
                                    if current.is_empty() {
                                        "그룹 선택"
                                    }else{group_name(groups,&current)},
                                    selector_rect.width(),enabled,
                                    "상품의 배송 그룹 선택",|menu|{
                                        components::management_dropdown_option(
                                            menu,&mut current,String::new(),"그룹 선택"
                                        );
                                        for group in groups {
                                            components::management_dropdown_option(
                                                menu,&mut current,text(group,"id").into(),
                                                text(group,"name")
                                            );
                                        }
                                    }
                                );
                            }
                        );
                        if current!=before_manual_edit {
                            *bulk_feedback=None;
                            if current!=committed {
                                // A per-product change saves immediately.
                                // Retain the optimistic value only until Rust
                                // confirms success or reports a failure.
                                pending.insert(id.to_owned(),current.clone());
                                let mut assignment=serde_json::Map::new();
                                assignment.insert(id.to_owned(),json!(current));
                                *action=Some(("assign_products".into(),json!({
                                    "assignments":assignment
                                })));
                            }else{
                                pending.remove(id);
                            }
                        }

                        let changed=pending.contains_key(id);
                        let effective=product_views.get(id)
                            .filter(|entry|entry.is_object());
                        let rule=effective.map(|entry|&entry["rule"])
                            .unwrap_or(&Value::Null);
                        let ready=effective.is_some();
                        // In the 144pt Settings cell, keep equal 12pt
                        // left/right insets around the complete action.
                        // The button stays within this 120pt slot; no
                        // table/card edge or neighboring column moves.
                        let actions_rect=cells[5].shrink2(
                            egui::vec2(theme::SPACE_12,0.0)
                        );
                        let actions_clip=actions_rect.intersect(row.clip_rect());
                        // Center the single "기본 운임 설정" action within
                        // the 120pt inset slot, preserving its 34pt height
                        // inside the 42pt row. Only the slot is justified.
                        // Preserve the original two-button layout if the
                        // legacy "기준 변경" action is also present.
                        let action_layout=if rule.is_object(){
                            egui::Layout::right_to_left(egui::Align::Center)
                        }else{
                            egui::Layout::left_to_right(egui::Align::Center)
                                .with_main_justify(true)
                        };
                        row.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(("freight-actions",id))
                                .max_rect(actions_rect)
                                .layout(action_layout),
                            |actions|{
                                actions.set_clip_rect(
                                    actions_clip
                                );
                                actions.set_width(actions_rect.width());
                                actions.spacing_mut().item_spacing.x=theme::SPACE_8;
                                let editable=enabled&&!changed&&ready;
                                // Only an unassigned product can edit its
                                // own base freight. Once a group is selected,
                                // freight belongs to that shipping group and
                                // the setting cell intentionally stays empty.
                                // Use current (including pending autosave) so
                                // the button hides immediately on selection.
                                if current.is_empty(){
                                    // Keep the 34pt button target and full
                                    // label, but treat this repeated action as
                                    // secondary to selecting a shipping group.
                                    if theme::action(actions,"기본 운임 설정",
                                        theme::ButtonRole::TableSecondary,editable)
                                        .on_hover_text("그룹 미지정 상품의 기본 운임 설정")
                                        .clicked(){
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
                        for index in 0..5 {
                            let edge=cells[index].right()
                                +theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
                            row.painter().line_segment(
                                [egui::pos2(edge,row_rect.top()),
                                 egui::pos2(edge,row_rect.bottom())],
                                egui::Stroke::new(
                                    theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                                )
                            );
                        }
                        row.painter().line_segment(
                            [row_rect.left_bottom(),row_rect.right_bottom()],
                            egui::Stroke::new(
                                theme::SOFT_DIVIDER_WIDTH,
                                theme::DIVIDER.linear_multiply(0.70)
                            )
                        );
                            }
                        );
                    }
                    if filtered.is_empty(){
                        table.label(typography::caption(
                            if products.is_empty(){"등록된 상품이 없습니다."}
                            else{"검색 조건에 맞는 상품이 없습니다."}
                        ));
                    }
                });
            // Limit the zero-gap setting to the header+rows block.
            section.spacing_mut().item_spacing.y=previous_table_spacing_y;
            // Header and body use freight_table_cells() AND the body
            // scroll origin, so no egui padding / Grid sizing can drift.
            let header_left=first_row_left.unwrap_or(
                header_rect.left()-scroll_output.state.offset.x
            );
            let header_cells=freight_table_cells(
                egui::pos2(header_left,header_rect.top()),
                &widths,header_rect.height()
            );
            // Paint the header background through the complete table viewport,
            // including the scrollbar rail. Only text/checkbox interaction
            // stays inside the column content rect, not the scrollbar.
            let header_painter=section.painter().with_clip_rect(header_rect);
            header_painter.rect_filled(header_rect,0.0,theme::RAISED);
            // The 2pt rule is INSIDE the 36pt header. Center every heading,
            // sort arrow and select-all checkbox in the remaining 34pt,
            // never in the full 36pt including its visible bottom border.
            let header_outline_height=theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT;
            let header_text_rect=egui::Rect::from_min_max(
                header_content_rect.min,
                egui::pos2(header_content_rect.right(),
                    header_content_rect.bottom()-header_outline_height),
            );
            let header_text_center_y=header_text_rect.center().y;
            for (i,title) in titles.iter().enumerate(){
                if !title.is_empty(){
                    let region=header_cells[i];
                    let label=typography::freight_table_header_galley(section,title);
                    let indicator=FreightSort::column_at(i).map(|column|{
                        // Always display a sort affordance, including before
                        // any column is selected. Only one column is active.
                        let current=sort.filter(|value|value.column==column);
                        let (glyph,color)=match current.map(|state|state.direction){
                            Some(FreightSortDirection::Ascending)=>("▲",theme::SELECTION),
                            Some(FreightSortDirection::Descending)=>("▼",theme::SELECTION),
                            None=>("↕",theme::MUTED),
                        };
                        let visible=region.intersect(header_text_rect);
                        if visible.width()>0.0 && visible.height()>0.0 {
                            let next=FreightSort::next(*sort,column);
                            let tooltip=match next.map(|value|value.direction){
                                Some(FreightSortDirection::Ascending)=>"오름차순 정렬",
                                Some(FreightSortDirection::Descending)=>"내림차순 정렬",
                                None=>"기본 순서로 복귀",
                            };
                            let response=section.interact(
                                visible,section.make_persistent_id(("freight-sort-column",i)),
                                egui::Sense::click()
                            ).on_hover_text(tooltip);
                            if response.clicked(){
                                *sort=next;
                                // Header appears after virtual rows in this
                                // layout; apply the new order next frame.
                                section.ctx().request_repaint();
                            }
                            if response.hovered(){
                                header_painter.rect_stroke(
                                    visible.shrink(1.0),2.0,
                                    egui::Stroke::new(1.0,theme::BORDER),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                        (typography::freight_table_header_galley(section,glyph),color)
                    });
                    let gap=if indicator.is_some(){theme::SPACE_4}else{0.0};
                    let arrow_width=indicator.as_ref().map_or(0.0,|(g,_)|g.size().x);
                    let total_width=label.size().x+gap+arrow_width;
                    // Text columns share the same 6pt leading inset as
                    // freight_assignment_text() in each body cell. The
                    // group selector and action columns remain centered.
                    let left=if matches!(i,1|2|3) {
                        region.left()+theme::MANAGEMENT_TABLE_TEXT_LEADING_INSET
                    }else{
                        region.center().x-total_width/2.0
                    };
                    let ink=header_painter.with_clip_rect(header_text_rect.intersect(region));
                    ink.galley(
                        egui::pos2(
                            left,header_text_center_y-label.size().y/2.0
                        ),
                        label.clone(),theme::SECONDARY
                    );
                    if let Some((arrow,color))=indicator{
                        ink.galley(
                            egui::pos2(
                                left+label.size().x+gap,
                                header_text_center_y-arrow.size().y/2.0
                            ),
                            arrow,color
                        );
                    }
                }
                if i<5 {
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
            // Paint one solid 2pt rule entirely INSIDE the header rectangle.
            // A 1pt stroke centered exactly on bottom() was half-clipped by
            // the header clip, then visually lost against the next dark row.
            // A filled band stays visible at 100%/125% DPI without moving
            // the fixed header, introducing a layout gap or extra row height.
            header_painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(
                        header_rect.left(),
                        header_rect.bottom()-header_outline_height
                    ),
                    header_rect.right_bottom(),
                ),
                0.0,
                theme::TABLE_HEADER_OUTLINE,
            );

            // This is the single "select all" control for the currently
            // filtered product list (not only the virtualized visible rows).
            // It replaces the redundant checkbox in the bulk-action toolbar.
            let mut all_filtered=!filtered.is_empty()
                && filtered.iter().all(|p|selected.contains(text(p,"id")));
            if components::management_table_checkbox(
                section,header_cells[0],header_text_rect,
                &mut all_filtered,enabled&&!filtered.is_empty(),
                "freight-select-all-filtered"
            ).on_hover_text("현재 검색·필터 결과 전체 선택 / 해제")
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
            // Assignment is persisted immediately from the product selector
            // or the single bulk Apply button above. Do not render a second
            // save footer, a redundant 'reset drafts' action, or guidance text.
        });
    }

    /// Rate management has only the search field on the left and
    /// group creation on the right. The destructive global basis command
    /// is intentionally isolated in the fourth freight-management tab.
    fn rate_toolbar(
        section:&mut Ui,search:&mut String,enabled:bool
    )->(egui::Response,egui::Rect,Option<egui::Response>){
        let width=section.available_width().max(1.0);
        let (toolbar_rect,_)=section.allocate_exact_size(
            egui::vec2(width,theme::INPUT_HEIGHT),egui::Sense::hover()
        );
        let create=section.scope_builder(
            egui::UiBuilder::new()
                .id_salt("freight-rate-toolbar-actions")
                .max_rect(toolbar_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |tools|{
                tools.set_width(toolbar_rect.width());
                tools.set_height(toolbar_rect.height());
                theme::action(
                    tools,"배송 그룹 추가",theme::ButtonRole::Primary,enabled
                )
            }
        ).inner;
        let room=(create.rect.left()-theme::SPACE_16-toolbar_rect.left())
            .max(0.0);
        let search_width=room.min(theme::MANAGEMENT_SEARCH_MIN_WIDTH);
        let search_response=if search_width>=60.0{
            let search_rect=egui::Rect::from_min_size(
                toolbar_rect.min,egui::vec2(search_width,theme::INPUT_HEIGHT)
            );
            Some(section.scope_builder(
                egui::UiBuilder::new()
                    .id_salt("freight-rate-toolbar-search")
                    .max_rect(search_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |field|{
                    field.set_clip_rect(search_rect);
                    field.set_width(search_rect.width());
                    components::flexible_text_field(
                        field,search,search_width,"배송 그룹 검색"
                    )
                }
            ).inner)
        }else{None};
        (create,toolbar_rect,search_response)
    }

    fn filtered_rate_groups<'a>(
        groups:&'a [Value],query:&str
    )->Vec<&'a Value>{
        let needle=query.trim().to_lowercase();
        groups.iter().filter(|group|
            needle.is_empty() ||
            text(group,"name").to_lowercase().contains(&needle)
        ).collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn show_rates(
        ui: &mut Ui, groups: &[Value], products: &[&Value], projection: &Value,
        create_group_open: &mut bool,
        rename_target: &mut Option<(String,String)>,
        delete_target: &mut Option<String>,
        editor: &mut Option<RateEditor>,
        basis: &mut Option<Option<String>>,
        sort: &mut Option<RateSort>,search:&mut String,
        error: &mut Option<String>,enabled:bool,
        action: &mut Option<(String,Value)>,
    ) {
        let group_rules=&projection["editableRules"];
        // A single group table replaces the duplicate group cards + audit grid.
        // Its actions still use the original Rust write paths and modal editors.
        let card_min_height=components::management_primary_card_min_inner_height(ui,theme::SECTION_PADDING);
        theme::section_frame().show(ui, |section| {
            components::fill_management_primary_card(section,card_min_height);
            section.spacing_mut().scroll=theme::management_table_scroll_style();
            // Search and group creation share one fixed 36pt toolbar.
            // The global rate-basis action is now on the dedicated Basis tab.
            let old_gap=section.spacing().item_spacing.y;
            section.spacing_mut().item_spacing.y=0.0;
            let (create,_toolbar,_search)=
                Self::rate_toolbar(section,search,enabled);
            if create.clicked(){*create_group_open=true;}
            // Shared master: exactly 16pt + 1pt divider + 4pt before
            // the fixed 36pt rate-table header, irrespective of modal state.
            let rate_layout=components::management_sticky_table_header(
                section,theme::FREIGHT_RATE_TABLE_MIN_WIDTH
            );
            let table_width=rate_layout.table_width;
            let widths=theme::management_weighted_columns(
                theme::FREIGHT_RATE_COL_WIDTHS,
                [0.0,0.0,1.0,0.0],
                table_width,theme::FREIGHT_ASSIGNMENT_COL_GAP
            );
            let titles=[
                "배송 그룹","최대 묶음 수량","수량별 배송비","관리"
            ];
            let mut ordered=Self::filtered_rate_groups(groups,search);
            if let Some(active)=*sort {
                ordered.sort_by_cached_key(|group|{
                    let key=match active.column {
                        RateSortColumn::Group=>text(group,"name").to_lowercase(),
                        RateSortColumn::Bundle=>group["current"]["maxBundle"]
                            .as_i64().filter(|_|group["current"]["maxConfirmed"]==true)
                            .map(|n|format!("{n:012}")).unwrap_or_default(),
                    };
                    (key,text(group,"id").to_owned())
                });
                if active.direction==FreightSortDirection::Descending{
                    ordered.reverse();
                }
            }
            let header_rect=rate_layout.header_rect;
            // No footer allocation for empty/search-miss states; their
            // persistent D-type message is painted in the data viewport.
            let scroll_max_height=components::management_single_list_scroll_height(
                section,card_min_height,0.0
            );
            // Footer reservation determines viewport height, not rail offset.
            // Every management table uses the same 7pt extension.
            let scroll_track=components::management_table_scroll_track_rect(
                section,scroll_max_height
            );
            let mut first_row_left=None;
            let scroll_output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .scroll_bar_rect(scroll_track)
                    .id_salt("freight-group-rates-rows")
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                    )
                    .max_height(scroll_max_height)
                    .auto_shrink([false,false]),
                section,table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT,
                ordered.len(),|table,visible_rows|{
                    table.set_min_width(table_width);
                    table.spacing_mut().item_spacing.y=0.0;
                    let origin=table.max_rect().min;
                    let first_index=visible_rows.start;
                    for index in visible_rows {
                        let group=ordered[index];
                        let id=text(group,"id");
                        let row_rect=egui::Rect::from_min_size(
                            egui::pos2(
                                origin.x,
                                origin.y+(index-first_index) as f32
                                    *theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                            ),
                            egui::vec2(
                                table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                            )
                        );
                        first_row_left.get_or_insert(row_rect.left());
                        table.scope_builder(
                            egui::UiBuilder::new()
                                .id_salt(("freight-rate-row",id))
                                .max_rect(row_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |row|{
                                let _=row.allocate_exact_size(
                                    egui::vec2(
                                        table_width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                                    ),
                                    egui::Sense::hover()
                                );
                                let cells=freight_table_cells(
                                    row_rect.min,&widths,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                                );
                                row.painter().rect_filled(
                                    row_rect,0.0,
                                    if index%2==0 {theme::PANEL}
                                    else {theme::TABLE_ROW_ALT_BG}
                                );
                                let count=products.iter().filter(|p|
                                    text(p,"shippingGroupId")==id
                                ).count();
                                let identity=format!("{} · {count}개 상품",text(group,"name"));
                                freight_rate_cell(
                                    row,cells[0],&identity,egui::Align2::LEFT_CENTER,
                                    typography::ManagementTableCellRole::Text,(id,0)
                                );
                                let current=&group["current"];
                                freight_rate_cell(
                                    row,cells[1],&group_bundle_summary(current),
                                    egui::Align2::RIGHT_CENTER,typography::ManagementTableCellRole::Quantity,(id,1)
                                );
                                let fees=group_fee_summary(current);
                                freight_rate_cell(
                                    row,cells[2],&fees,egui::Align2::LEFT_CENTER,
                                    typography::ManagementTableCellRole::Money,(id,2)
                                );
                                // Group operations are one compact trigger.
                                // The popup is an independent overlay and never
                                // changes the virtualized row or scrollbar height.
                                let rect=cells[3].shrink2(egui::vec2(6.0,0.0));
                                row.scope_builder(
                                    egui::UiBuilder::new()
                                        .id_salt(("freight-rate-actions",id))
                                        .max_rect(rect)
                                        .layout(egui::Layout::left_to_right(
                                            egui::Align::Center
                                        ).with_main_align(egui::Align::Center)),
                                    |buttons|{
                                        buttons.set_clip_rect(
                                            cells[3].intersect(row_rect)
                                        );
                                        buttons.set_width(rect.width());
                                        buttons.set_height(rect.height());
                                        let trigger=theme::action(
                                            buttons,"관리",
                                            theme::ButtonRole::TableSecondary,enabled
                                        ).on_hover_text("운임 수정 · 기준 변경 · 이름 수정 · 삭제");
                                        egui::Popup::menu(&trigger)
                                            .close_behavior(
                                                egui::PopupCloseBehavior::CloseOnClickOutside
                                            )
                                            .show(|menu|{
                                                menu.set_min_width(148.0);
                                                menu.spacing_mut().item_spacing.y=
                                                    theme::SPACE_4;
                                                if theme::action_full_width(
                                                    menu,"운임 수정",
                                                    theme::ButtonRole::TableSecondary,enabled
                                                ).clicked(){
                                                    match group_rules.get(id)
                                                        .filter(|rule|rule.is_object()){
                                                        Some(rule)=>*editor=Some(
                                                            RateEditor::new(
                                                                id,text(group,"name"),rule
                                                            )
                                                        ),
                                                        None=>*error=Some(
                                                            "그룹 운임 기준을 읽지 못했습니다. 새로고침 후 확인하세요.".into()
                                                        ),
                                                    }
                                                    menu.close();
                                                }
                                                if theme::action_full_width(
                                                    menu,"기준 변경",
                                                    theme::ButtonRole::TableSecondary,enabled
                                                ).clicked(){
                                                    *basis=Some(Some(id.to_owned()));
                                                    menu.close();
                                                }
                                                if theme::action_full_width(
                                                    menu,"이름 수정",
                                                    theme::ButtonRole::TableSecondary,enabled
                                                ).clicked(){
                                                    *rename_target=Some((
                                                        id.to_owned(),text(group,"name").to_owned()
                                                    ));
                                                    menu.close();
                                                }
                                                if theme::action_full_width(
                                                    menu,"삭제",
                                                    theme::ButtonRole::Danger,enabled
                                                ).clicked(){
                                                    *delete_target=Some(id.to_owned());
                                                    menu.close();
                                                }
                                            });
                                    }
                                );
                                for col in 0..3{
                                    let x=cells[col].right()
                                        +theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
                                    row.painter().line_segment(
                                        [egui::pos2(x,row_rect.top()),
                                         egui::pos2(x,row_rect.bottom())],
                                        egui::Stroke::new(
                                            theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                                        )
                                    );
                                }
                                row.painter().line_segment(
                                    [row_rect.left_bottom(),row_rect.right_bottom()],
                                    egui::Stroke::new(
                                        theme::SOFT_DIVIDER_WIDTH,
                                        theme::DIVIDER.linear_multiply(0.70)
                                    )
                                );
                            }
                        );
                    }
                }
            );
            section.spacing_mut().item_spacing.y=old_gap;
            let left=first_row_left.unwrap_or(
                header_rect.left()-scroll_output.state.offset.x
            );
            let header_cells=freight_table_cells(
                egui::pos2(left,header_rect.top()),&widths,header_rect.height()
            );
            let painter=section.painter().with_clip_rect(header_rect);
            painter.rect_filled(header_rect,0.0,theme::RAISED);
            for (index,title) in titles.iter().enumerate(){
                let cell=header_cells[index];
                let galley=typography::freight_table_header_galley(section,title);
                let sort_column=RateSort::column_at(index);
                let indicator=sort_column.map(|column|{
                    let selected=sort.filter(|current|current.column==column);
                    let (label,color)=match selected.map(|value|value.direction){
                        Some(FreightSortDirection::Ascending)=>("▲",theme::SELECTION),
                        Some(FreightSortDirection::Descending)=>("▼",theme::SELECTION),
                        None=>("↕",theme::MUTED),
                    };
                    let hit=cell.intersect(header_rect);
                    if hit.width()>0.0 && hit.height()>0.0{
                        let next=RateSort::next(*sort,column);
                        let tooltip=match next.map(|value|value.direction){
                            Some(FreightSortDirection::Ascending)=>"오름차순 정렬",
                            Some(FreightSortDirection::Descending)=>"내림차순 정렬",
                            None=>"기본 순서로 복귀",
                        };
                        if section.interact(
                            hit,section.make_persistent_id(("freight-rate-sort",index)),
                            egui::Sense::click()
                        ).on_hover_text(tooltip).clicked(){
                            *sort=next;
                            section.ctx().request_repaint();
                        }
                    }
                    (typography::freight_table_header_galley(section,label),color)
                });
                let gap=if indicator.is_some(){theme::SPACE_4}else{0.0};
                let arrow_width=indicator.as_ref().map_or(0.0,|(g,_)|g.size().x);
                let total=galley.size().x+gap+arrow_width;
                let x=match index{
                    1=>cell.right()-6.0-total,
                    3=>cell.center().x-total/2.0,
                    _=>cell.left()+6.0,
                };
                let ink=painter.with_clip_rect(cell.intersect(header_rect));
                ink.galley(
                    egui::pos2(x,cell.center().y-galley.size().y/2.0),
                    galley,theme::SECONDARY
                );
                if let Some((arrow,color))=indicator{
                    ink.galley(
                        egui::pos2(x+total-arrow.size().x,
                            cell.center().y-arrow.size().y/2.0),
                        arrow,color
                    );
                }
                if index<3{
                    let x=cell.right()+theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
                    painter.line_segment(
                        [egui::pos2(x,header_rect.top()),
                         egui::pos2(x,header_rect.bottom())],
                        egui::Stroke::new(
                            theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                        )
                    );
                }
            }
            painter.line_segment(
                [header_rect.left_bottom(),header_rect.right_bottom()],
                egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
            );
            if ordered.is_empty(){
                let caption=if groups.is_empty(){
                    "등록된 배송 그룹이 없습니다."
                }else{
                    "검색 조건에 맞는 배송 그룹이 없습니다."
                };
                // Keep the master header and scrollbar geometry invariant:
                // empty/search states do not allocate a row below the table.
                let available=egui::Rect::from_min_size(
                    scroll_track.min,egui::vec2(
                        (rate_layout.viewport_width-rate_layout.scrollbar_gutter)
                            .min(scroll_track.width()).max(1.0),
                        scroll_max_height
                    )
                ).intersect(section.clip_rect());
                if available.width()>0.0 && available.height()>0.0 {
                    section.painter().with_clip_rect(available).text(
                        available.center(),egui::Align2::CENTER_CENTER,caption,
                        egui::FontId::proportional(typography::FONT_METADATA),
                        theme::MUTED
                    );
                }
            }
        });
    }

    fn rate_dialog(
        ui:&mut Ui,editor:&mut RateEditor,enabled:bool,
        event:&mut Option<(String,Value)>,
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
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

    /// Global rate-basis changes are intentionally NOT available on the
    /// routine group-edit toolbar. This page exposes scope and consequences
    /// before letting the user open the existing confirmation dialog.
    fn show_basis_management(
        ui:&mut Ui,shipping:&Value,enabled:bool,
        basis:&mut Option<Option<String>>,
    ){
        let card_min_height=components::management_primary_card_min_inner_height(
            ui,theme::SECTION_PADDING
        );
        theme::section_frame().show(ui,|section|{
            components::fill_management_primary_card(section,card_min_height);
            section.label(typography::section_heading("전체 운임 기준"));
            section.add_space(theme::SPACE_12);
            let epoch=shipping["epoch"].as_i64().unwrap_or(1);
            section.label(typography::body(&format!("현재 운임 기준: {epoch}")));
            section.add_space(theme::SPACE_16);
            section.label(typography::body(
                "전체 운임 기준을 변경하면 기존 운임의 자동 추천을 중단합니다. \
                 변경 후 그룹별·상품별 운임을 다시 확인하여 등록해야 합니다."
            ));
            section.add_space(theme::SPACE_16);
            // A dedicated, explicitly labeled destructive action. It still
            // opens freight-basis-confirm and does not mutate on click.
            section.horizontal(|row|{
                row.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),|right|{
                        if theme::action(
                            right,"전체 운임 기준 변경",
                            theme::ButtonRole::Danger,enabled
                        ).clicked(){
                            *basis=Some(None);
                        }
                    }
                );
            });
        });
    }

    fn show_history(
        ui: &mut Ui, shipping: &Value, projection: &Value,
        exception: &mut Option<(String,String)>,
        sort:&mut Option<HistorySort>,enabled:bool,
    ) {
        // Both sections reuse the assignment reference's fixed header, 42pt
        // non-wrapping rows and reserved solid scrollbar. No fee computation
        // or history mutation is performed by this view.
        theme::section_frame().show(ui, |section| {
            // The first history table has no redundant title. Its fixed
            // header must start at the same vertical position as the two
            // freight master tabs: 36pt toolbar-height guide + shared 21pt
            // divider transition. Zero implicit spacing prevents drift.
            let prior_gap=section.spacing().item_spacing.y;
            section.spacing_mut().item_spacing.y=0.0;
            let (history_toolbar_guide,_)=section.allocate_exact_size(
                egui::vec2(section.available_width(),theme::INPUT_HEIGHT),
                egui::Sense::hover()
            );
            debug_assert!((history_toolbar_guide.height()-theme::INPUT_HEIGHT).abs()<0.01);
            let combinations=rows(&shipping["combinations"]);
            // Keep React's original newest-first order unless the user
            // explicitly sorts a meaningful column.
            let mut ordered:Vec<&Value>=combinations.iter().rev().collect();
            if let Some(active_sort)=*sort{
                ordered.sort_by_cached_key(|record|{
                    let key=match active_sort.column{
                        HistorySortColumn::Combination=>
                            text(record,"description").to_lowercase(),
                        HistorySortColumn::RecordedAt=>
                            text(record,"at").to_owned(),
                        HistorySortColumn::Charge=>record["charge"].as_i64()
                            .map(|n|format!("{n:020}")).unwrap_or_default(),
                        HistorySortColumn::Actual=>record["actual"].as_i64()
                            .map(|n|format!("{n:020}")).unwrap_or_default(),
                        HistorySortColumn::Basis=>match projection["currentCombinations"]
                            .get(text(record,"id")).and_then(Value::as_bool){
                                Some(true)=>"2",Some(false)=>"1",None=>"0"
                            }.to_owned(),
                    };
                    (key,text(record,"id").to_owned())
                });
                if active_sort.direction==FreightSortDirection::Descending{
                    ordered.reverse();
                }
            }
            freight_history_table(
                section,"freight-verified-combinations",
                ["조합","등록일 / 검증","청구 운임","실제 운임","기준","수정"],
                theme::FREIGHT_COMBINATION_HISTORY_COL_WIDTHS,
                [0.65,0.35,0.0,0.0,0.0,0.0],ordered.len(),Some(sort),
                true,
                if combinations.is_empty(){
                    Some("실제 배송 완료 후 검증한 운임 조합이 없습니다.")
                }else{None},
                |row,cells,index|{
                    let record=ordered[index];
                    let id=text(record,"id");
                    let at=text(record,"at");
                    let verified_at=text(record,"verifiedAt");
                    let recorded_date=at.split('T').next()
                        .filter(|date|!date.is_empty()).unwrap_or("날짜 미확인");
                    let samples=record["sampleCount"].as_u64().unwrap_or(1);
                    let verification=if verified_at.is_empty(){
                        format!("{recorded_date} · 사용자 등록")
                    }else{
                        format!("{recorded_date} · 실배송 검증 {samples}회")
                    };
                    let verification_detail=if verified_at.is_empty(){
                        format!("등록: {at} / 사용자 등록 기준")
                    }else{
                        format!("등록: {at} / 검증: {verified_at} / 실배송 {samples}회")
                    };
                    history_cell(
                        row,cells[0],text(record,"description"),
                        text(record,"description"),egui::Align2::LEFT_CENTER,
                        typography::ManagementTableCellRole::Text,(id,0)
                    );
                    history_cell(
                        row,cells[1],&verification,&verification_detail,
                        egui::Align2::LEFT_CENTER,typography::ManagementTableCellRole::Description,(id,1)
                    );
                    for (column,key) in [(2,"charge"),(3,"actual")]{
                        let fee=as_fee(&record[key]);
                        history_cell(
                            row,cells[column],&fee,&fee,
                            egui::Align2::RIGHT_CENTER,typography::ManagementTableCellRole::Money,(id,column)
                        );
                    }
                    let validity=projection["currentCombinations"]
                        .get(id).and_then(Value::as_bool);
                    let status=match validity{
                        Some(true)=>"현재 유효",
                        Some(false)=>"과거 이력",
                        None=>"운임 기준 조회 중",
                    };
                    history_cell(
                        row,cells[4],status,status,
                        egui::Align2::CENTER_CENTER,typography::ManagementTableCellRole::Status,(id,4)
                    );
                    let action_rect=cells[5].shrink2(egui::vec2(theme::SPACE_8,0.0));
                    let history_row_clip=row.clip_rect();
                    row.scope_builder(
                        egui::UiBuilder::new()
                            .id_salt(("freight-history-change",id))
                            .max_rect(action_rect)
                            .layout(egui::Layout::left_to_right(
                                egui::Align::Center
                            ).with_main_align(egui::Align::Center)),
                        |buttons|{
                            buttons.set_clip_rect(cells[5].intersect(history_row_clip));
                            buttons.set_width(action_rect.width());
                            if theme::action(
                                buttons,"예외 운임 변경",
                                theme::ButtonRole::TableSecondary,enabled
                            ).clicked(){
                                // This is the existing revise_combination
                                // modal and Rust mutation path, unchanged.
                                *exception=Some((
                                    id.to_owned(),
                                    record["charge"].as_i64()
                                        .map(|n|n.to_string()).unwrap_or_default()
                                ));
                            }
                        }
                    );
                }
            );
            section.spacing_mut().item_spacing.y=prior_gap;
        });
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui, |section| {
            section.label(typography::section_heading("이전 그룹 운임 기준"));
            components::soft_divider(section);
            // Preserve the existing group-by-group expand/collapse behavior.
            // Expanded histories become one-row-per-version fixed tables,
            // rather than unaligned long sentences.
            for group in rows(&shipping["groups"]).iter().filter(|g|active(g)){
                let group_id=text(group,"id");
                let history=rows(&group["history"]);
                egui::CollapsingHeader::new(format!(
                    "{} / {}개 기준",text(group,"name"),history.len()
                )).id_salt(("freight-history",group_id))
                    .icon(components::paint_collapsing_header_icon)
                    .show(section,|body|{
                        if history.is_empty(){
                            body.label(typography::caption("이전 기준이 없습니다."));
                            return;
                        }
                        let table_id=format!("freight-history-group-{group_id}");
                        freight_history_table(
                            body,&table_id,
                            ["기준 일자","최대 묶음 수량","수량별 배송비",
                             "기존 개당 기준","사용자 참고 메모","버전"],
                            theme::FREIGHT_GROUP_HISTORY_COL_WIDTHS,
                            [0.10,0.0,0.55,0.0,0.35,0.0],history.len(),None,
                            false,None,
                            |row,cells,index|{
                                let old=&history[index];
                                let at=text(old,"at");
                                let display_date=at.split('T').next()
                                    .filter(|date|!date.is_empty())
                                    .unwrap_or("날짜 미확인");
                                history_cell(
                                    row,cells[0],display_date,at,
                                    egui::Align2::LEFT_CENTER,typography::ManagementTableCellRole::Text,
                                    (group_id,index,0)
                                );
                                let bundle=group_bundle_summary(old);
                                history_cell(
                                    row,cells[1],&bundle,&bundle,
                                    egui::Align2::RIGHT_CENTER,typography::ManagementTableCellRole::Quantity,
                                    (group_id,index,1)
                                );
                                let fees=group_history_fee_summary(old);
                                history_cell(
                                    row,cells[2],&fees,&fees,
                                    egui::Align2::LEFT_CENTER,typography::ManagementTableCellRole::Money,
                                    (group_id,index,2)
                                );
                                let each=as_fee(&old["perItem"]);
                                history_cell(
                                    row,cells[3],&each,&each,
                                    egui::Align2::RIGHT_CENTER,typography::ManagementTableCellRole::Money,
                                    (group_id,index,3)
                                );
                                let note=text(old,"notes");
                                history_cell(
                                    row,cells[4],if note.is_empty(){"-"}else{note},
                                    note,egui::Align2::LEFT_CENTER,typography::ManagementTableCellRole::Description,(group_id,index,4)
                                );
                                let version=match old.get("version"){
                                    Some(Value::String(value))=>value.to_owned(),
                                    Some(value) if !value.is_null()=>value.to_string(),
                                    _=>"-".to_owned(),
                                };
                                history_cell(
                                    row,cells[5],&version,&version,
                                    egui::Align2::CENTER_CENTER,typography::ManagementTableCellRole::Status,(group_id,index,5)
                                );
                            }
                        );
                    });
            }
        });
    }
}


fn group_history_fee_summary(rule:&Value)->String{
    // A historic null means "unconfirmed", not "missing". Zero is a
    // confirmed amount and must remain visible in the archived record.
    let fees=(1..=6).filter_map(|count|{
        rule["fees"].get(count.to_string()).map(|fee|
            format!("{count}개 {}",if fee.is_null(){
                "미확정".to_owned()
            }else{as_fee(fee)})
        )
    }).collect::<Vec<_>>();
    if fees.is_empty(){"운임 미등록".to_owned()}else{fees.join(" · ")}
}

/// Present a full archived value on hover even when the visible 14pt cell
/// is clipped. This is only a view: no fee/status business decision is added.
fn history_cell(
    ui:&mut Ui,rect:egui::Rect,visible:&str,full:&str,
    align:egui::Align2,role:typography::ManagementTableCellRole,
    id:impl std::hash::Hash
){
    let cell=rect.shrink2(egui::vec2(6.0,0.0));
    let clip=cell.intersect(ui.clip_rect());
    if clip.width()<=0.0 || clip.height()<=0.0{return;}
    let color=typography::management_table_cell_color(role);
    let galley=typography::management_table_cell_galley(ui,visible,role);
    let x=if align==egui::Align2::RIGHT_CENTER{
        cell.right()-galley.size().x
    }else if align==egui::Align2::CENTER_CENTER{
        cell.center().x-galley.size().x/2.0
    }else{cell.left()};
    ui.painter().with_clip_rect(clip).galley(
        egui::pos2(x,cell.center().y-galley.size().y/2.0),galley,color
    );
    if !full.is_empty(){
        ui.interact(
            clip,ui.make_persistent_id(("freight-history-cell",id)),
            egui::Sense::hover()
        ).on_hover_text(full);
    }
}

/// Common freight history table geometry, shared between combination history
/// and expanded historical group versions. The fixed header is outside the
/// virtual row scroll, with the same column origins and gutter reservation.
#[allow(clippy::too_many_arguments)]
fn freight_history_table(
    ui:&mut Ui,id:&str,titles:[&str;6],bases:[f32;6],shares:[f32;6],
    total_rows:usize,mut sort:Option<&mut Option<HistorySort>>,
    follows_master_toolbar:bool,empty_message:Option<&str>,
    mut render_row:impl FnMut(&mut Ui,&[egui::Rect;6],usize),
){
    ui.spacing_mut().scroll=theme::management_table_scroll_style();
    let old_spacing=ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y=0.0;
    let minimum=bases.iter().sum::<f32>()
        +5.0*theme::FREIGHT_ASSIGNMENT_COL_GAP;
    let layout=if follows_master_toolbar {
        // Only the first/history overview has the approved toolbar guide.
        // Expanded group-version tables already have their own section
        // heading and must not receive an unrelated 21pt strip.
        components::management_sticky_table_header(ui,minimum)
    }else{
        components::management_list_table_header(ui,minimum)
    };
    let width=layout.table_width;
    let widths=theme::management_weighted_columns(
        bases,shares,width,theme::FREIGHT_ASSIGNMENT_COL_GAP
    );
    let header_rect=layout.header_rect;
    let scroll_max_height=ui.ctx().content_rect().height()
        *theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION;
    // Multi-section history: never extend a rail through the next card.
    let scroll_track=components::management_table_scroll_track_rect(
        ui,scroll_max_height
    );
    let mut first_row_left=None;
    let scroll=components::virtual_fixed_rows(
        egui::ScrollArea::both()
            .scroll_bar_rect(scroll_track)
            .id_salt(id)
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
            )
            .max_height(scroll_max_height)
            .auto_shrink([false,false]),
        ui,width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT,total_rows,
        |table,visible_rows|{
            table.set_min_width(width);
            table.spacing_mut().item_spacing.y=0.0;
            let first=visible_rows.start;
            let origin=table.max_rect().min;
            for index in visible_rows {
                let row_rect=egui::Rect::from_min_size(
                    egui::pos2(
                        origin.x,
                        origin.y+(index-first) as f32
                            *theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                    ),
                    egui::vec2(width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT)
                );
                first_row_left.get_or_insert(row_rect.left());
                table.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt((id,index))
                        .max_rect(row_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |row|{
                        let _=row.allocate_exact_size(
                            egui::vec2(width,theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT),
                            egui::Sense::hover()
                        );
                        let cells=freight_table_cells(
                            row_rect.min,&widths,
                            theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT
                        );
                        row.painter().rect_filled(
                            row_rect,0.0,if index%2==0{
                                theme::PANEL
                            }else{theme::TABLE_ROW_ALT_BG}
                        );
                        render_row(row,&cells,index);
                        for column in 0..5{
                            let boundary=cells[column].right()
                                +theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
                            row.painter().line_segment(
                                [egui::pos2(boundary,row_rect.top()),
                                 egui::pos2(boundary,row_rect.bottom())],
                                egui::Stroke::new(
                                    theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                                )
                            );
                        }
                        row.painter().line_segment(
                            [row_rect.left_bottom(),row_rect.right_bottom()],
                            egui::Stroke::new(
                                theme::SOFT_DIVIDER_WIDTH,
                                theme::DIVIDER.linear_multiply(0.70)
                            )
                        );
                    }
                );
            }
        }
    );
    ui.spacing_mut().item_spacing.y=old_spacing;
    let left=first_row_left.unwrap_or(
        header_rect.left()-scroll.state.offset.x
    );
    let columns=freight_table_cells(
        egui::pos2(left,header_rect.top()),&widths,header_rect.height()
    );
    let painter=ui.painter().with_clip_rect(header_rect);
    painter.rect_filled(header_rect,0.0,theme::RAISED);
    for (index,title) in titles.iter().enumerate(){
        let cell=columns[index];
        let label=typography::freight_table_header_galley(ui,title);
        let mut indicator=None;
        if let (Some(slot),Some(column))=(
            sort.as_mut(),HistorySort::column_at(index)
        ){
            let current=**slot;
            let (glyph,color)=match current
                .filter(|value|value.column==column)
                .map(|value|value.direction){
                    Some(FreightSortDirection::Ascending)=>
                        ("▲",theme::SELECTION),
                    Some(FreightSortDirection::Descending)=>
                        ("▼",theme::SELECTION),
                    None=>("↕",theme::MUTED),
                };
            let hit=cell.intersect(header_rect);
            if hit.width()>0.0 && hit.height()>0.0{
                let next=HistorySort::next(current,column);
                let tooltip=match next.map(|value|value.direction){
                    Some(FreightSortDirection::Ascending)=>"오름차순 정렬",
                    Some(FreightSortDirection::Descending)=>"내림차순 정렬",
                    None=>"기본 순서로 복귀",
                };
                if ui.interact(
                    hit,ui.make_persistent_id((id,"history-sort",index)),
                    egui::Sense::click()
                ).on_hover_text(tooltip).clicked(){
                    **slot=next;
                    ui.ctx().request_repaint();
                }
            }
            indicator=Some((
                typography::freight_table_header_galley(ui,glyph),color
            ));
        }
        let gap=if indicator.is_some(){theme::SPACE_4}else{0.0};
        let arrow=indicator.as_ref().map_or(0.0,|(galley,_)|galley.size().x);
        let combined=label.size().x+gap+arrow;
        let x=match index{
            2|3=>cell.right()-6.0-combined,
            4|5=>cell.center().x-combined/2.0,
            _=>cell.left()+6.0,
        };
        let ink=painter.with_clip_rect(cell.intersect(header_rect));
        ink.galley(
            egui::pos2(x,cell.center().y-label.size().y/2.0),
            label,theme::SECONDARY
        );
        if let Some((arrow_galley,color))=indicator{
            ink.galley(
                egui::pos2(x+combined-arrow_galley.size().x,
                    cell.center().y-arrow_galley.size().y/2.0),
                arrow_galley,color
            );
        }
        if index<5{
            let boundary=cell.right()+theme::FREIGHT_ASSIGNMENT_COL_GAP/2.0;
            painter.line_segment(
                [egui::pos2(boundary,header_rect.top()),
                 egui::pos2(boundary,header_rect.bottom())],
                egui::Stroke::new(
                    theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                )
            );
        }
    }
    painter.line_segment(
        [header_rect.left_bottom(),header_rect.right_bottom()],
        egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    if total_rows==0 {
        if let Some(message)=empty_message {
            // D-type state paints in the body, with no extra title/footer
            // allocation that could raise or lower the fixed header.
            let body=egui::Rect::from_min_size(
                scroll_track.min,egui::vec2(
                    (layout.viewport_width-layout.scrollbar_gutter)
                        .min(scroll_track.width()).max(0.0),
                    scroll_max_height
                )
            ).intersect(ui.clip_rect());
            if body.width()>0.0&&body.height()>0.0 {
                ui.painter().with_clip_rect(body).text(
                    body.center(),egui::Align2::CENTER_CENTER,message,
                    egui::FontId::proportional(typography::FONT_METADATA),
                    theme::MUTED,
                );
            }
        }
    }
}

/// Only Freight management currently trials the 16pt page-title inset.
/// The heading, all three sub-tabs and four KPI cards share the same left
/// guide as the content INSIDE the 16pt section frame. This intentionally
/// leaves the section frame's OUTER border at the shared 59th-round anchor.
/// Keeping the inset local avoids moving Reception or other management pages.
fn freight_top_content<R>(ui:&mut Ui,add:impl FnOnce(&mut Ui)->R)->R{
    components::management_top_content(ui,add)
}

/// Render one of four independent KPI cards. Visual hierarchy is deliberately
/// numeric-first: 28pt value, then a muted 13pt label, both centered.
/// The card itself stays fixed at 228pt wide, with proportional internal spacing.
fn summary(ui: &mut Ui, label: &str, count: usize) {
    let card_width=theme::FREIGHT_SUMMARY_CARD_WIDTH;
    ui.allocate_ui_with_layout(
        egui::vec2(card_width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |slot|{
            slot.set_width(card_width);
            theme::freight_summary_frame().show(slot,|cell|{
                // These two lines and their gap define the frame's exact
                // content height; disable egui's implicit vertical item gap.
                cell.spacing_mut().item_spacing.y=0.0;
                let content_width=card_width-2.0*theme::FREIGHT_SUMMARY_PADDING_X
                    -2.0*theme::FRAME_STROKE_WIDTH;
                let (number_row,_)=cell.allocate_exact_size(
                    egui::vec2(
                        content_width,theme::FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT
                    ),
                    egui::Sense::hover()
                );
                cell.add_space(theme::FREIGHT_SUMMARY_TEXT_GAP);
                let (label_row,_)=cell.allocate_exact_size(
                    egui::vec2(
                        content_width,theme::FREIGHT_SUMMARY_LABEL_LINE_HEIGHT
                    ),
                    egui::Sense::hover()
                );
                let number=typography::freight_summary_large_number_galley(
                    cell,&count.to_string()
                );
                let caption=typography::freight_summary_small_label_galley(
                    cell,label
                );
                let painter=cell.painter();
                painter.galley(
                    egui::pos2(
                        number_row.center().x-number.size().x/2.0,
                        number_row.center().y-number.size().y/2.0
                    ),
                    number,theme::TEXT
                );
                painter.galley(
                    egui::pos2(
                        label_row.center().x-caption.size().x/2.0,
                        label_row.center().y-caption.size().y/2.0
                    ),
                    caption,theme::SECONDARY
                );
            });
        }
    );
}


#[cfg(test)]
mod rate_toolbar_tests{
    use super::*;

    #[test]
    fn four_tabs_preserve_order_and_isolate_global_basis_action(){
        assert_eq!(FreightTab::ALL.len(),4);
        assert!(matches!(FreightTab::ALL[0],FreightTab::Groups));
        assert!(matches!(FreightTab::ALL[1],FreightTab::Rates));
        assert!(matches!(FreightTab::ALL[2],FreightTab::History));
        assert!(matches!(FreightTab::ALL[3],FreightTab::Basis));
        assert_eq!(FreightTab::LABELS,[
            "배송 그룹 편성","배송 그룹 관리",
            "조합 / 기준 이력","운임 기준 관리"
        ]);
    }

    #[test]
    fn search_and_group_add_share_a_centered_master_toolbar(){
        let ctx=egui::Context::default();
        typography::install(&ctx);
        let mut input=egui::RawInput::default();
        input.screen_rect=Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,egui::vec2(1160.0,800.0)
        ));
        let mut measured=None;
        let _=ctx.run(input,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let mut search=String::new();
                let (add,toolbar,field)=
                    FreightManager::rate_toolbar(ui,&mut search,true);
                measured=Some((add.rect,toolbar,field.map(|r|r.rect)));
            });
        });
        let (add,toolbar,search)=measured.expect("rate toolbar rendered");
        let search=search.expect("search must be visible in desktop layout");
        assert!((toolbar.height()-theme::INPUT_HEIGHT).abs()<0.01);
        assert!((search.left()-toolbar.left()).abs()<0.01);
        assert!((search.center().y-toolbar.center().y).abs()<0.01);
        assert!((add.center().y-toolbar.center().y).abs()<0.01);
        assert!(search.right()+theme::SPACE_16<=add.left()+0.01,
            "search must not overlap group addition");
        assert!((add.right()-toolbar.right()).abs()<0.01);
    }

    #[test]
    fn rate_summary_keeps_all_six_quantity_prices_in_one_string(){
        let rule=serde_json::json!({
            "fees":{"1":3000,"2":4500,"3":6000,
                    "4":7500,"5":9000,"6":10500}
        });
        let summary=group_fee_summary(&rule);
        for n in 1..=6 {
            assert!(summary.contains(&format!("{n}개")),
                "missing quantity {n}: {summary}");
        }
        assert_eq!(summary.matches(" · ").count(),5);
    }

    #[test]
    fn four_column_rate_table_gives_long_tariffs_priority(){
        let base=theme::FREIGHT_RATE_COL_WIDTHS;
        let gap=theme::FREIGHT_ASSIGNMENT_COL_GAP;
        assert_eq!(base.len(),4);
        assert!((base.iter().sum::<f32>()+3.0*gap-
            theme::FREIGHT_RATE_TABLE_MIN_WIDTH).abs()<0.01);
        assert!(base[2]>=740.0,"all six tariff amounts need a wide column");
        assert!(base[3]<=148.0,"one Manage trigger must not reserve four buttons");
        let wide=theme::management_weighted_columns(
            base,[0.0,0.0,1.0,0.0],
            theme::FREIGHT_RATE_TABLE_MIN_WIDTH+120.0,gap
        );
        assert!((wide[2]-base[2]-120.0).abs()<0.01);
        assert!((wide[3]-base[3]).abs()<0.01);
    }

    #[test]
    fn search_filters_only_group_names_case_insensitively(){
        let groups=vec![
            serde_json::json!({"name":"[TEST] 표준 배송 그룹"}),
            serde_json::json!({"name":"Express Group"}),
            serde_json::json!({"name":"이동형 그룹"})
        ];
        let find=|query:&str|FreightManager::filtered_rate_groups(&groups,query).len();
        assert_eq!(find(""),3);
        assert_eq!(find("  "),3);
        assert_eq!(find("배송"),1);
        assert_eq!(find("  group "),1);
        assert_eq!(find("이동형"),1);
        assert_eq!(find("존재하지 않는 이름"),0);
    }
}
