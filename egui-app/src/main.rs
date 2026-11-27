//! BANDO HUB egui — first native visual implementation.
//! UI preview only: uses existing read-only Rust business decisions and
//! a separate egui-preview.sqlite3 for source continuity. Existing React/Tauri
//! stays untouched. A production client master may be read ONLY by explicit
//! read-only opt-in; KakaoTalk is placed by a separate reversible Win32 dock.

mod theme;
mod typography;
mod layout;
mod ui_layout;
mod navigation;
mod composite_workflow;
mod composite_client;
mod composite_reception;
mod text_fit;
mod window_state;
mod launch_profile;
#[cfg(test)]
mod ui_regressions;
mod category_tree;
mod status_notice;
mod ui_primitives;
mod display_format;
mod ui_tokens;
#[cfg(test)]
mod ui_test_support;
mod table;
mod button;
mod checkbox;
mod scrollbar;
mod popup;
mod card;
mod input;
mod dropdown;
mod overlay;
mod cell_navigation;
mod floating_notice;
mod user_error;
mod reception_service;
mod review_panel;
mod gateway;
mod http_gateway;
mod preview_store;
mod source_autosave;
mod clipboard_intake;
mod ocr_learning;
mod client_directory;
mod client_master;
mod client_manager;
mod client_search_tags;
mod automatic_message_manager;
mod category_manager;
mod category_dropdown;
mod alias_manager;
mod latest_job;
mod screen_refresh;
mod view_state;
mod brand_manager;
mod model_group_manager;
mod product_manager;
mod product_option_display;
mod product_excel;
mod native_workbooks;
mod factory_reply_inbox;
mod notification_center;
mod product_excel_ui;
mod client_chooser;
mod business_state;
mod native_order_change;
mod pricing_basis;
mod reception_jobs;
mod source_highlight;
mod workflow_board;
mod workflow_board_ui;
mod one_touch;
mod manual_workspace;
mod catalog_query_bridge;
mod item_editor;
mod product_images;
mod manual_source_view;
mod kakao_context;
mod kakao_dock;
mod settings_store;
mod settings_view;
mod settings_help;
mod freight_manager;
mod order_history_manager;
mod order_history_detail;
mod order_history_memo;
mod daily_settlement_manager;
mod excel_manager;
mod excel_service;
mod management_queries;

use eframe::egui::{self, Align, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use crate::card::UiWidthPolicyExt;
use theme::ButtonRole;
use button::WindowControl;
use cell_navigation::{CellNavigationCell, CellNavigationKey};
use clipboard_intake::{ClipboardImage, ClipboardMonitor, ClipboardPayload, OrderType};
use preview_store::{OcrFeedback, WorkingReception, WorkingSource};
use kakao_context::{ChatWindow, KakaoTracker};
use client_chooser::{ClientChooser, Choice};
use reception_jobs::{Admission as JobAdmission,Kind as ReceptionJob};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
    Arc,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Reception,
    History,
    Excel,
    Settlement,
    Products,
    Freight,
    Clients,
    Settings,
}

impl Page {
    const ALL: [Page; 8] = [
        Page::Reception,
        Page::History,
        Page::Excel,
        Page::Settlement,
        Page::Products,
        Page::Freight,
        Page::Clients,
        Page::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Reception => "접수",
            Page::History => "주문 내역",
            Page::Excel => "엑셀 관리",
            Page::Settlement => "일일 결산",
            Page::Products => "상품 관리",
            Page::Freight => "운임 관리",
            Page::Clients => "거래처 관리",
            Page::Settings => "설정",
        }
    }
}

// Keep OCR image in memory until an employee approves recognized text.
enum ClipboardReceipt {
    Text { text:String, sequence:Option<u32> },
    Ocr { text:String, image:ClipboardImage, sequence:Option<u32> },
}

// Region capture temporarily hides the HUB so the native Windows overlay
// captures the actual desktop, not our reception panel. Restoration is RAII:
// Esc, an OCR cancel, timeout, and errors all return the HUB to the front.
struct RegionCaptureRestore(egui::Context);
impl Drop for RegionCaptureRestore {
    fn drop(&mut self){
        self.0.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.0.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.0.request_repaint();
    }
}
struct OcrReview {
    order_type: OrderType,
    original_text: String,
    edited_text: String,
    texture: egui::TextureHandle,
    width: usize,
    height: usize,
    zoom: f32,
    chat: Option<ChatWindow>,
    clipboard_sequence: Option<u32>,
    /// Controls applying learned analysis to this review, NOT learning.
    /// Approved original/corrected OCR pairs are recorded regardless of this.
    learned_analysis_enabled: bool,
    model_receiver: Option<Receiver<Result<ocr_learning::Suggestion,String>>>,
    model_suggestion: Option<String>,
    model_error: Option<String>,
}
struct PendingReplace {
    text: String,
    order_type: OrderType,
    chat: Option<ChatWindow>,
    input_method: &'static str,
    clipboard_sequence: Option<u32>,
    // Present only for manually approved OCR. Kept through the second
    // replacement confirmation and never captured on cancel.
    ocr_original: Option<String>,
    ocr_learning_used: bool,
    ocr_model_suggestion: Option<String>,
}
// Do not clear a copied source until its exact order ID and raw text are
// acknowledged by the serialized Rust autosave worker.
struct PendingClipboardClear {
    order_id:String,
    raw:String,
    order_type:String,
    sequence:Option<u32>,
}

/// A drag always measures displacement from the initial pane widths.
#[derive(Clone, Copy)]
struct HorizontalSplitDrag {
    index: usize,
    initial_widths: [f32; layout::PANEL_COUNT],
}

#[derive(Clone, Copy)]
struct VerticalSplitDrag {
    initial_source_height: f32,
    available_height: f32,
}

struct PendingClientChange {
    order_id: String,
    client_id: String,
    source_raw: String,
    order_type: String,
    // A changed Kakao conversation invalidates the pending dealer decision,
    // even when the pasted text itself has not changed.
    chat_handle: Option<String>,
    chat_title: Option<String>,
    // A later inline/manual edit must never be overwritten by an older quote.
    manual_model: Option<serde_json::Value>,
    working_version:Option<u64>,
    price_snapshot:Option<serde_json::Value>,
    // A master-driven reprice keeps an AUTO decision AUTO.
    reprice_selection: Option<String>,
    receiver: Receiver<Result<native_order_change::ChangeResult, String>>,
}

struct PendingPricingBasisCheck {
    order_id:String,
    source_raw:String,
    order_type:String,
    client_id:String,
    quote:serde_json::Value,
    read:screen_refresh::ReadRefresh<bool>,
}

// An automatic match is only advisory until the same native Rust order
// mutation/reprice pipeline approves it. Keep it separate from the employee's
// manual change, which must always take precedence.
struct PendingAutomaticClient {
    request: u64,
    order_id: String,
    source_raw: String,
    order_type: String,
    chat_handle: Option<String>,
    chat_title: Option<String>,
    client_id: String,
    working_version:Option<u64>,
    price_snapshot:Option<serde_json::Value>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<native_order_change::ChangeResult, String>>,
}

// Discarding an advisory auto-match always requests cooperative cancellation.
// Dropped egui receivers alone cannot stop the Rust computation in progress.
impl Drop for PendingAutomaticClient {
    fn drop(&mut self){
        self.cancel.store(true,Ordering::Release);
    }
}

fn workflow_write_admission(revision:Option<u64>,writing:bool,
    refresh_required:bool,failed:bool)->bool{
    screen_refresh::write_admitted(revision.is_some(),writing,refresh_required,failed)
}

struct PendingWorkflowAction {
    group_id: String,
    action: String,
    notice_origin:Option<floating_notice::Anchor>,
    receiver: Receiver<Result<serde_json::Value,String>>,
}

/// Local editor for a confirmed workflow shipment. It is intentionally
/// separate from InlineReceptionEditor: confirmed orders must be saved by
/// the workflow reducer, never by the temporary reception workspace.
struct WorkflowFreightEditor {
    group_id:String,
    shipment_id:String,
    origin:String,
    original:String,
    text:String,
    focus_next:bool,
    click_at:Option<egui::Pos2>,
}

#[derive(Clone)]
enum WorkflowDialog {
    ManageGroup(String),
    ManagePending(String),
    DeleteGroup(String),
    DeletePending(String),
    PaymentNotice(String),
}

#[derive(Clone,Debug,PartialEq,Eq)]
struct InlineTarget {
    recipient_id:String,
    item_id:Option<String>,
    field:&'static str,
}
#[derive(Clone)]
struct InlineNavigationEntry {
    target:InlineTarget,
    cell:CellNavigationCell,
}
// Build navigation metadata only after a keyboard movement request.
fn inline_navigation_entries(recipient:&serde_json::Value,editable:bool)->Vec<InlineNavigationEntry>{
    let rid=reception_service::text(recipient,"id");
    let mut entries=Vec::new();
    let mut push=|item:Option<&str>,field:&'static str,row:usize,column:usize,order:usize,enabled:bool|{
        let target=InlineTarget{recipient_id:rid.to_owned(),item_id:item.map(str::to_owned),field};
        entries.push(InlineNavigationEntry{cell:CellNavigationCell{
            id:inline_target_key(&target),row,column,order,enabled
        },target});
    };
    for (order,field) in cell_navigation::RECIPIENT_FIELDS.into_iter().enumerate(){
        let (row,column)=cell_navigation::recipient_visual_position(field);
        push(None,field,row,column,order,editable);
    }
    let items=recipient["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for (index,item) in items.iter().enumerate(){
        let id=reception_service::text(item,"id");
        push(Some(id),"quantity",index+1,1,5+index*2,editable);
        push(Some(id),"unitPrice",index+1,2,6+index*2,editable);
    }
    push(None,"shippingCharge",items.len()+1,2,5+items.len()*2,
        editable && recipient["freightCode"].as_str()!=Some("020"));
    entries
}
// Confirmed rows reuse the worksheet grid; each dispatch origin has its own freight cell.
fn confirmed_freight_origins(ship:&workflow_board::ShipmentRow)->Vec<String>{
    let mut origins=Vec::<String>::new();
    if let Some(items)=ship.recipient["items"].as_array(){
        for item in items{
            let origin=reception_service::text(item,"origin");
            if ["FACTORY","STORE"].contains(&origin) &&
                !origins.iter().any(|saved|saved.as_str()==origin){
                origins.push(origin.to_owned());
            }
        }
    }
    origins
}
fn confirmed_freight_target(ship:&workflow_board::ShipmentRow,origin:&str)->InlineTarget{
    InlineTarget{recipient_id:ship.recipient_id.clone(),item_id:Some(origin.to_owned()),field:"shippingCharge"}
}
fn confirmed_navigation_entries(ship:&workflow_board::ShipmentRow,editable:bool,freight_editable:bool)
    ->Vec<InlineNavigationEntry>{
    let mut entries=inline_navigation_entries(&ship.recipient,editable);
    entries.retain(|entry|entry.target.field!="shippingCharge");
    for entry in &mut entries{
        entry.target.recipient_id=ship.recipient_id.clone();
        entry.cell.id=inline_target_key(&entry.target);
    }
    let count=ship.recipient["items"].as_array().map_or(0,Vec::len);
    for (index,origin) in confirmed_freight_origins(ship).iter().enumerate(){
        let target=confirmed_freight_target(ship,origin);
        entries.push(InlineNavigationEntry{cell:CellNavigationCell{
            id:inline_target_key(&target),row:count+1+index,column:2,
            order:5+count*2+index,enabled:freight_editable
        },target});
    }
    entries
}
fn resolve_inline_navigation(entries:&[InlineNavigationEntry],current:&InlineTarget,key:CellNavigationKey)
    ->Option<InlineTarget>{
    let cells=entries.iter().filter(|entry|entry.target.recipient_id==current.recipient_id)
        .map(|entry|cell_navigation::ReceptionNavigationCell{
            cell:&entry.cell,shipment:0,field:entry.target.field,has_item:entry.target.item_id.is_some()
        }).collect::<Vec<_>>();
    let id=cell_navigation::resolve_reception_navigation(&cells,&inline_target_key(current),key,false)?;
    entries.iter().find(|entry|entry.cell.id==id).map(|entry|entry.target.clone())
}
fn confirmed_editor_slot(shipment_id:&str,target:&InlineTarget)->egui::Id{
    egui::Id::new(("confirmed-inline-grid",shipment_id,inline_target_key(target)))
}
fn inline_target_key(target:&InlineTarget)->String{
    format!("{}:{}:{}",target.recipient_id,
        target.item_id.as_deref().unwrap_or(""),target.field)
}
/// Stable keyboard slot for a reception cell across egui frames.
fn inline_editor_slot(target:&InlineTarget)->egui::Id{
    egui::Id::new(("reception-inline-grid",inline_target_key(target)))
}
#[cfg(test)]
#[test]
fn reception_arrow_navigation_preserves_section_local_geometry(){
    let recipient=serde_json::json!({"id":"r1","items":[{"id":"i1"},{"id":"i2"}],"freightCode":"010"});
    let entries=inline_navigation_entries(&recipient,true);
    let target=|item:Option<&str>,field:&'static str|InlineTarget{
        recipient_id:"r1".into(),item_id:item.map(str::to_owned),field
    };
    let phone=target(None,"phone");
    assert_eq!(resolve_inline_navigation(&entries,&phone,CellNavigationKey::Up),Some(target(None,"address")));
    assert_eq!(resolve_inline_navigation(&entries,&phone,CellNavigationKey::Down),Some(target(None,"note")));
    assert_eq!(resolve_inline_navigation(&entries,&phone,CellNavigationKey::Left),Some(target(None,"mobile")));
    assert_eq!(resolve_inline_navigation(&entries,&target(None,"note"),CellNavigationKey::Right),None);
    assert_eq!(resolve_inline_navigation(&entries,&target(Some("i1"),"quantity"),CellNavigationKey::Right),Some(target(Some("i1"),"unitPrice")));
}

#[cfg(test)]
#[test]
fn reception_navigation_crosses_recipient_item_boundary_in_both_directions(){
    let recipient=serde_json::json!({"id":"r1","items":[{"id":"i1"},{"id":"i2"}],"freightCode":"010"});
    let entries=inline_navigation_entries(&recipient,true);
    let target=|item:Option<&str>,field:&'static str|InlineTarget{
        recipient_id:"r1".into(),item_id:item.map(str::to_owned),field
    };
    let note=target(None,"note");
    let qty=target(Some("i1"),"quantity");
    let price=target(Some("i1"),"unitPrice");
    assert_eq!(resolve_inline_navigation(&entries,&note,CellNavigationKey::Down),Some(qty.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&note,CellNavigationKey::EnterDown),Some(qty.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&qty,CellNavigationKey::EnterUp),Some(note.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&target(Some("i2"),"quantity"),CellNavigationKey::Down),Some(target(None,"shippingCharge")));
    assert_eq!(resolve_inline_navigation(&entries,&qty,CellNavigationKey::Up),Some(note.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&price,CellNavigationKey::Up),Some(note.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&note,CellNavigationKey::Next),Some(qty.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&qty,CellNavigationKey::Previous),Some(note.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&note,CellNavigationKey::Right),None);
    assert_eq!(resolve_inline_navigation(&entries,&qty,CellNavigationKey::Left),None);
    assert_eq!(resolve_inline_navigation(&entries,&target(Some("i2"),"quantity"),CellNavigationKey::Up),Some(qty));
    let empty=inline_navigation_entries(&serde_json::json!({"id":"r1","items":[],"freightCode":"010"}),true);
    let freight=target(None,"shippingCharge");
    assert_eq!(resolve_inline_navigation(&empty,&note,CellNavigationKey::Down),Some(freight.clone()));
    assert_eq!(resolve_inline_navigation(&empty,&freight,CellNavigationKey::Up),Some(note.clone()));
    let collect=inline_navigation_entries(&serde_json::json!({"id":"r1","items":[],"freightCode":"020"}),true);
    assert_eq!(resolve_inline_navigation(&collect,&note,CellNavigationKey::Down),None);
}

#[cfg(test)]
#[test]
fn confirmed_navigation_uses_shared_geometry_and_skips_ineligible_freight(){
    let ship=workflow_board::ShipmentRow{
        id:"s1".into(),order_id:"o1".into(),recipient_id:"r1".into(),
        transferred:false,recipient_name:String::new(),products:vec![],
        completed_step:Some(1),paid:None,source_raw:String::new(),
        recipient:serde_json::json!({"id":"r1","items":[
            {"id":"i1","origin":"FACTORY"},{"id":"i2","origin":"STORE"}]}),
        remote:None,display_charges:Default::default(),freight_editing_blocked:false,freight_exempt:false,
        collect:false,shipment_count:1,order_total:None
    };
    let entries=confirmed_navigation_entries(&ship,true,true);
    let phone=InlineTarget{recipient_id:"r1".into(),item_id:None,field:"phone"};
    assert_eq!(resolve_inline_navigation(&entries,&phone,CellNavigationKey::Up).unwrap().field,"address");
    let note=InlineTarget{recipient_id:"r1".into(),item_id:None,field:"note"};
    let first_qty=InlineTarget{recipient_id:"r1".into(),item_id:Some("i1".into()),field:"quantity"};
    let first_price=InlineTarget{recipient_id:"r1".into(),item_id:Some("i1".into()),field:"unitPrice"};
    assert_eq!(resolve_inline_navigation(&entries,&note,CellNavigationKey::Down),Some(first_qty.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&first_qty,CellNavigationKey::Up),Some(note.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&first_price,CellNavigationKey::Up),Some(note));
    let price=InlineTarget{recipient_id:"r1".into(),item_id:Some("i2".into()),field:"unitPrice"};
    let factory=confirmed_freight_target(&ship,"FACTORY");
    let store=confirmed_freight_target(&ship,"STORE");
    assert_eq!(resolve_inline_navigation(&entries,&price,CellNavigationKey::Down),Some(factory.clone()));
    assert_eq!(resolve_inline_navigation(&entries,&factory,CellNavigationKey::Down),Some(store));
    let collect=confirmed_navigation_entries(&ship,true,false);
    assert_eq!(resolve_inline_navigation(&collect,&price,CellNavigationKey::Next),None);
    assert_eq!(resolve_inline_navigation(&collect,&price,CellNavigationKey::Down),None);
    let locked=confirmed_navigation_entries(&ship,false,false);
    assert_eq!(resolve_inline_navigation(&locked,&phone,CellNavigationKey::Up),None);
}

#[derive(Clone)]
struct ConfirmedEditTarget{
    selection:workflow_board_ui::Selection,
    revision:u64,
    recipient_id:String,
}
struct ConfirmedCellEditor{
    target:ConfirmedEditTarget,
    cell:InlineReceptionEditor,
}

struct InlineReceptionEditor {
    source_id:String,
    target:InlineTarget,
    original:String,
    text:String,
    focus_next:bool,
    /// Saved pointer-up coordinate for first-click native caret placement.
    click_at:Option<egui::Pos2>,
}
struct PendingRemoteRecheck{
    order_id:String,
    source_raw:String,
    order_type:String,
    recipients:Vec<(String,String)>,
    cancel:Arc<AtomicBool>,
    receiver:Receiver<Vec<(String,String,Result<serde_json::Value,String>)>>,
}
impl Drop for PendingRemoteRecheck{
    fn drop(&mut self){ self.cancel.store(true,Ordering::Release); }
}
struct PendingInlineChange {
    order_id:String,
    raw:String,
    target:InlineTarget,
    /// Only a real text cell can be restored after a failed write.
    editor:Option<InlineReceptionEditor>,
    /// A client choice made during a delivery command is replayed after its result.
    next_client_review:Option<review_panel::Action>,
    /// Captured from the SAME click that blurs the previous field.
    /// Never discard this intent merely because native saving is async.
    next:Option<(InlineTarget,Option<egui::Pos2>)>,
    receiver:Receiver<Result<manual_workspace::Event,String>>,
}
impl PendingInlineChange{
    fn is_delivery_command(&self)->bool{
        self.editor.is_none() && matches!(self.target.field,"remoteDelivery"|"freightCode"|"dispatchOrigin")
    }
}

/// One scope and rate update for the entire order.
#[derive(Clone,Copy,PartialEq,Eq,Debug)]
enum VatScope { None, All, Partial }
struct VatDialog {
    order_id:String,
    source_raw:String,
    recipients:std::collections::HashSet<String>,
    rate_text:String,
    error:Option<String>
}
fn vat_rate_text(bps:i64)->String{
    if bps%100==0{format!("{}",bps/100)}
    else if bps%10==0{format!("{}.{}",bps/100,(bps%100)/10)}
    else{format!("{}.{:02}",bps/100,bps%100)}
}
fn parse_vat_rate(text:&str)->Result<i64,String>{
    let trimmed=text.trim().trim_end_matches('%').trim();
    let parts=trimmed.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len()>2 || parts[0].is_empty() ||
       !parts[0].chars().all(|c|c.is_ascii_digit()){
        return Err("세율은 0%~100% 범위에서 입력하세요.".into());
    }
    let whole=parts[0].parse::<i64>()
        .map_err(|_|"세율을 확인하세요.")?;
    let fraction=if let Some(decimals)=parts.get(1){
        if decimals.is_empty() || decimals.len()>2 ||
            !decimals.chars().all(|c|c.is_ascii_digit()){
            return Err("세율은 소수점 둘째 자리까지만 입력하세요.".into());
        }
        format!("{decimals:0<2}").parse::<i64>()
            .map_err(|_|"세율을 확인하세요.")?
    }else{0};
    whole.checked_mul(100).and_then(|x|x.checked_add(fraction))
        .filter(|bps|(0..=10_000).contains(bps))
        .ok_or("세율은 0%~100% 범위에서 입력하세요.".into())
}
impl VatDialog {
    fn new(order_id:String,source_raw:String,rows:&[serde_json::Value])->Self{
        let recipients=rows.iter().filter(|row|
            gateway::rules::vat_applies(&serde_json::Value::Null,row)
        ).filter_map(|row|row["id"].as_str().map(str::to_owned))
            .collect::<std::collections::HashSet<_>>();
        let bps=rows.iter().filter_map(|row|row["vatRateBps"].as_i64())
            .find(|bps|(0..=10_000).contains(bps)).unwrap_or(1000);
        Self{order_id,source_raw,recipients,
            rate_text:vat_rate_text(bps),error:None}
    }
    /// The global checkbox state is derived from recipients, never stored
    /// as an independent mode that can disagree with the individual rows.
    fn scope(&self,rows:&[serde_json::Value])->VatScope {
        let count=rows.iter().filter_map(|row|row["id"].as_str())
            .filter(|id|self.recipients.contains(*id)).count();
        if count==0{VatScope::None}
        else if count==rows.len(){VatScope::All}
        else{VatScope::Partial}
    }
    /// Both top-level checkboxes update the same selection set used by the
    /// always-visible per-recipient checkboxes.
    fn set_all_checked(&mut self,rows:&[serde_json::Value],checked:bool){
        if checked {
            self.recipients=rows.iter().filter_map(|row|
                row["id"].as_str().filter(|id|!id.is_empty())
            ).map(str::to_owned).collect();
        }else{
            self.recipients.clear();
        }
        self.error=None;
    }
    /// Individual selection is only committed by the modal's Apply action.
    fn set_recipient_checked(&mut self,id:&str,checked:bool){
        if checked{self.recipients.insert(id.to_owned());}
        else{self.recipients.remove(id);}
        self.error=None;
    }
    fn request(&self,rows:&[serde_json::Value])
        ->Result<manual_workspace::InlineRequest,String>
    {
        if rows.is_empty(){return Err("부가세 적용 배송건이 없습니다.".into());}
        let rate=parse_vat_rate(&self.rate_text)?;
        let scope=match self.scope(rows){
            VatScope::None=>"none",
            VatScope::All=>"all",
            VatScope::Partial=>"partial"
        };
        let selected=if scope=="partial" {
            rows.iter().filter_map(|row|{
                let id=row["id"].as_str()?;
                self.recipients.contains(id).then_some(id.to_owned())
            }).collect::<Vec<_>>()
        }else{Vec::new()};
        Ok(manual_workspace::InlineRequest{
            recipient_id:String::new(),item_id:None,
            field:"vatSettings".into(),value:serde_json::json!({
                "scope":scope,"recipientIds":selected,"rateBps":rate
            })
        })
    }
}
struct PendingVatChange {
    order_id:String,
    source_raw:String,
    receiver:Receiver<Result<manual_workspace::Event,String>>
}

/// Resolve the value that is CURRENT after the preceding native edit,
/// rather than copying a stale value captured before it was committed.
// Recognition IDs are intentionally unscoped; persisted native order IDs
// carry the order prefix for global SQLite uniqueness. Match only the exact
// order-qualified form, never by a loose suffix or product name.
// A historical quote may have only unit prices. Requote it with the new
// shipment-aware native contract before treating it as valid.
// Null amounts are permitted for genuinely unresolved catalog/freight data.
fn complete_reception_quote(quote:&serde_json::Value)->bool{
    quote.get("linePrices").and_then(serde_json::Value::as_array)
        .is_some_and(|lines|lines.iter().all(|line|line.get("amount").is_some()))
    && quote.get("freightLines").is_some_and(serde_json::Value::is_array)
    && quote.get("pendingRecipients").is_some_and(serde_json::Value::is_array)
}

/// First-stage approval requires the exact quote for the current draft, not
/// a settled freight total. Native workflow validation owns stage blockers.
fn confirmable_reception_quote(
    source:&WorkingReception,quote:&serde_json::Value
)->bool{
    let Some(client_id)=source.client_id.as_deref()
        .filter(|id|!id.trim().is_empty()) else{return false;};
    let source_ready=if source.raw.trim().is_empty(){
        source.manual_model.as_ref().is_some_and(|model|
            model.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT") &&
            model.get("sourceRaw").and_then(serde_json::Value::as_str)==Some("")
        )
    }else{
        source.analysis_raw.as_deref()==Some(source.raw.as_str()) &&
            source.analysis_evidence.is_some()
    };
    source_ready && source.order_type!="RETAIL" &&
        quote.get("orderId").and_then(serde_json::Value::as_str)==Some(source.id.as_str()) &&
        quote.get("sourceRaw").and_then(serde_json::Value::as_str)==Some(source.raw.as_str()) &&
        quote.get("orderType").and_then(serde_json::Value::as_str)==Some(source.order_type.as_str()) &&
        quote.get("clientId").and_then(serde_json::Value::as_str)==Some(client_id) &&
        quote.get("authorityRevision").and_then(serde_json::Value::as_u64).is_some() &&
        quote.get("linePrices").and_then(serde_json::Value::as_array).is_some() &&
        quote.get("freightLines").and_then(serde_json::Value::as_array).is_some() &&
        quote.get("pendingRecipients").and_then(serde_json::Value::as_array).is_some()
}

fn quote_id_matches(
    quote:&serde_json::Value,quoted_id:Option<&str>,source_id:&str,kind:&str
)->bool{
    let Some(quoted_id)=quoted_id else{return false};
    if quoted_id==source_id{return true;}
    if !quoted_id.starts_with(kind) && !source_id.starts_with(kind){return false;}
    let Some(order_id)=quote.get("orderId").and_then(serde_json::Value::as_str)
        .filter(|id|!id.is_empty()) else{return false;};
    let canonical=|id:&str| if kind=="recognition:item:" {
        gateway::rules::scope_reception_edit_ids(order_id,"",Some(id)).1.unwrap_or_default()
    }else{
        gateway::rules::scope_reception_edit_ids(order_id,id,None).0
    };
    canonical(quoted_id)==canonical(source_id)
}

fn quoted_item_line<'a>(
    quote:&'a serde_json::Value,item_id:&str
)->Option<&'a serde_json::Value>{
    quote.get("linePrices")?.as_array()?.iter().find(|line|
        quote_id_matches(
            quote,line["itemId"].as_str(),item_id,"recognition:item:"
        )
    )
}

fn quoted_freight_line<'a>(
    quote:&'a serde_json::Value,recipient_id:&str,origin:Option<&str>
)->Option<&'a serde_json::Value>{
    // Native reception_merge defaults an absent dispatch origin to FACTORY.
    // Preserve that exact rule rather than inventing a freight charge here.
    let origin=origin.filter(|s|!s.is_empty()).unwrap_or("FACTORY");
    quote.get("freightLines")?.as_array()?.iter().find(|line|
        quote_id_matches(
            quote,line["recipientId"].as_str(),recipient_id,
            "recognition:recipient:"
        ) && line["origin"].as_str()==Some(origin)
    )
}

// Native quote display index. Keep the first matching row, including legacy
// unscoped recognition IDs, using the same identity rule as the editor.
#[derive(Default)]
struct QuoteLineIndex{
    exact:HashMap<(String,String),usize>,
    canonical:HashMap<(String,String),usize>,
    unscoped:HashMap<(String,String),usize>,
}
fn canonical_quote_id(order:&str,id:&str,kind:&str)->String{
    if kind=="recognition:item:"{
        gateway::rules::scope_reception_edit_ids(order,"",Some(id)).1.unwrap_or_default()
    }else{gateway::rules::scope_reception_edit_ids(order,id,None).0}
}
impl QuoteLineIndex{
    fn new(rows:&serde_json::Value,order:&str,field:&str,kind:&str,freight:bool)->Self{
        let mut result=Self::default();
        for (index,line) in rows.as_array().into_iter().flatten().enumerate(){
            let Some(id)=line[field].as_str() else{continue;};
            let origin=if freight{let Some(origin)=line["origin"].as_str() else{continue;};origin}else{""};
            result.exact.entry((id.to_owned(),origin.to_owned())).or_insert(index);
            if !order.is_empty(){
                let canonical=canonical_quote_id(order,id,kind);
                result.canonical.entry((canonical.clone(),origin.to_owned())).or_insert(index);
                if id.starts_with(kind){
                    result.unscoped.entry((canonical,origin.to_owned())).or_insert(index);
                }
            }
        }
        result
    }
    fn lookup(&self,order:&str,id:&str,kind:&str,origin:&str)->Option<usize>{
        let exact=self.exact.get(&(id.to_owned(),origin.to_owned())).copied();
        if order.is_empty(){return exact;}
        let aliases=if id.starts_with(kind){&self.canonical}else{&self.unscoped};
        let alternate=aliases.get(&(canonical_quote_id(order,id,kind),origin.to_owned())).copied();
        match (exact,alternate){
            (Some(a),Some(b))=>Some(a.min(b)),
            (a,b)=>a.or(b),
        }
    }
}
pub(crate) struct QuoteDisplayIndex{
    order:String,items:serde_json::Value,freight:serde_json::Value,
    item_index:QuoteLineIndex,freight_index:QuoteLineIndex,
}
impl QuoteDisplayIndex{
    fn new(quote:&serde_json::Value)->Self{
        let order=quote["orderId"].as_str().unwrap_or("").to_owned();
        let items=quote["linePrices"].clone();
        let freight=quote["freightLines"].clone();
        Self{item_index:QuoteLineIndex::new(&items,&order,"itemId","recognition:item:",false),
            freight_index:QuoteLineIndex::new(&freight,&order,"recipientId","recognition:recipient:",true),
            order,items,freight}
    }
    pub(crate) fn item(&self,id:&str)->Option<&serde_json::Value>{
        self.items.as_array()?.get(self.item_index.lookup(&self.order,id,"recognition:item:","")?)
    }
    fn freight(&self,id:&str,origin:Option<&str>)->Option<&serde_json::Value>{
        let origin=origin.filter(|s|!s.is_empty()).unwrap_or("FACTORY");
        self.freight.as_array()?.get(self.freight_index.lookup(&self.order,id,"recognition:recipient:",origin)?)
    }
}
thread_local!{
    static QUOTE_DISPLAY_CACHE:std::cell::RefCell<Vec<Arc<QuoteDisplayIndex>>>=
        const{std::cell::RefCell::new(Vec::new())};
}
pub(crate) fn indexed_quote(quote:&serde_json::Value)->Arc<QuoteDisplayIndex>{
    QUOTE_DISPLAY_CACHE.with(|cache|{
        let mut cache=cache.borrow_mut();
        let result=if let Some(index)=cache.iter().position(|entry|
            entry.order==quote["orderId"].as_str().unwrap_or("") &&
            entry.items==quote["linePrices"] && entry.freight==quote["freightLines"]){
            cache.remove(index)
        }else{Arc::new(QuoteDisplayIndex::new(quote))};
        if cache.len()>=3{cache.remove(0);}
        cache.push(result.clone());result
    })
}
fn clear_quote_display_cache(){QUOTE_DISPLAY_CACHE.with(|cache|cache.borrow_mut().clear());}
pub(crate) fn indexed_freight_exempt(quote:Option<&QuoteDisplayIndex>,recipient:&serde_json::Value)->bool{
    quote.and_then(|q|q.freight(recipient["id"].as_str()?,recipient["dispatchOrigin"].as_str()))
        .is_some_and(|line|line["billingReason"].as_str()==Some("EXEMPT"))
}
fn indexed_billable_freight(quote:Option<&QuoteDisplayIndex>,recipient:&serde_json::Value)->Option<i64>{
    if recipient["freightCode"].as_str()==Some("020"){return Some(0);}
    let line=quote?.freight(recipient["id"].as_str()?,recipient["dispatchOrigin"].as_str())?;
    if let Some(billable)=line.get("billableCharge"){billable.as_i64()}
    else{line["appliedCharge"].as_i64()}
}

/// Display the charged freight, not the retained parcel tariff.
/// Legacy prepaid snapshots may contain only appliedCharge.
fn quoted_billable_freight(quote:Option<&serde_json::Value>,recipient:&serde_json::Value)->Option<i64>{
    if recipient["freightCode"].as_str()==Some("020"){return Some(0);}
    let line=quoted_freight_line(quote?,recipient["id"].as_str()?,recipient["dispatchOrigin"].as_str())?;
    if let Some(billable)=line.get("billableCharge"){billable.as_i64()}
    else{line["appliedCharge"].as_i64()}
}

#[cfg(test)]
mod quote_identity_tests {
    use super::*;
    #[test]
    fn cod_freight_displays_zero_and_prepaid_restores_native_charge(){
        let mut recipient=serde_json::json!({"id":"r","dispatchOrigin":"FACTORY","freightCode":"010"});
        let quote=serde_json::json!({"freightLines":[{"recipientId":"r","origin":"FACTORY",
            "appliedCharge":8000,"billableCharge":8000}]});
        assert_eq!(quoted_billable_freight(Some(&quote),&recipient),Some(8000));
        recipient["freightCode"]=serde_json::json!("020");
        assert_eq!(quoted_billable_freight(Some(&quote),&recipient),Some(0));
        assert_eq!(quoted_billable_freight(None,&recipient),Some(0));
        recipient["freightCode"]=serde_json::json!("010");
        assert_eq!(quoted_billable_freight(Some(&quote),&recipient),Some(8000));
        assert_eq!(quoted_billable_freight(None,&recipient),None);
        assert_eq!(quote["freightLines"][0]["appliedCharge"],8000);
    }

    #[test]
    fn on_demand_navigation_preserves_grid_order_and_cod_disabled_freight(){
        let recipient=serde_json::json!({"id":"r","freightCode":"020","items":[{"id":"i1"},{"id":"i2"}]});
        let entries=inline_navigation_entries(&recipient,true);
        assert_eq!(entries.len(),10);
        assert_eq!(entries[2].target.field,"mobile");
        assert_eq!((entries[3].cell.row,entries[3].cell.column),(2,1));
        assert_eq!((entries[5].cell.row,entries[5].cell.column),(1,1));
        assert_eq!(entries[8].target.field,"unitPrice");
        assert_eq!(entries[9].target.field,"shippingCharge");
        assert!(!entries[9].cell.enabled);
        assert!(inline_navigation_entries(&recipient,false).iter().all(|e|!e.cell.enabled));
    }

    #[test]
    fn indexed_quote_matches_legacy_first_row_and_invalidates_changed_amount(){
        let mut quote=serde_json::json!({"orderId":"o","linePrices":[
            {"itemId":"o:recognition:item:1:0","amount":10},
            {"itemId":"recognition:item:1:0","amount":20},
            {"itemId":"manual","amount":30}
        ],"freightLines":[
            {"recipientId":"o:recognition:recipient:0","origin":"FACTORY","appliedCharge":3000},
            {"recipientId":"recognition:recipient:0","origin":"FACTORY","appliedCharge":4000},
            {"recipientId":"recognition:recipient:0","origin":"STORE","billableCharge":2500}
        ]});
        let index=indexed_quote(&quote);
        assert!(Arc::ptr_eq(&index,&indexed_quote(&quote)));
        for id in ["recognition:item:1:0","o:recognition:item:1:0","other:recognition:item:1:0","manual","missing"]{
            assert_eq!(index.item(id),quoted_item_line(&quote,id));
        }
        for id in ["recognition:recipient:0","o:recognition:recipient:0","other:recognition:recipient:0"]{
            for origin in [None,Some(""),Some("FACTORY"),Some("STORE")]{
                assert_eq!(index.freight(id,origin),quoted_freight_line(&quote,id,origin));
            }
        }
        let recipient=serde_json::json!({"id":"recognition:recipient:0","freightCode":"020"});
        assert_eq!(indexed_billable_freight(Some(&index),&recipient),Some(0));
        quote["linePrices"][0]["amount"]=serde_json::json!(99);
        let changed=indexed_quote(&quote);
        assert!(!Arc::ptr_eq(&index,&changed));
        assert_eq!(changed.item("recognition:item:1:0").unwrap()["amount"],99);
    }

    #[test]
    fn native_scoped_quote_matches_unscoped_recognition_ids(){
        let quote=serde_json::json!({
            "orderId":"order-42",
            "linePrices":[
                {"itemId":"order-42:recognition:item:8:0",
                 "unitPrice":59000,"amount":236000},
                {"itemId":"manual-item","unitPrice":12000,"amount":12000}
            ],
            "freightLines":[
                {"recipientId":"order-42:recognition:recipient:0",
                 "origin":"FACTORY","appliedCharge":3000}
            ]
        });
        assert_eq!(quoted_item_line(&quote,"recognition:item:8:0")
            .and_then(|line|line["amount"].as_i64()),Some(236000));
        assert_eq!(quoted_item_line(&quote,"manual-item")
            .and_then(|line|line["unitPrice"].as_i64()),Some(12000));
        assert_eq!(quoted_freight_line(&quote,"recognition:recipient:0",None)
            .and_then(|line|line["appliedCharge"].as_i64()),Some(3000));
        assert!(complete_reception_quote(&serde_json::json!({
            "linePrices":[{"itemId":"x","amount":null}],
            "freightLines":[],"pendingRecipients":[{"recipientId":"r","freight":true}]
        })));
        assert!(!complete_reception_quote(&serde_json::json!({
            "linePrices":[{"itemId":"x","unitPrice":1000}]
        })));
        let legacy=serde_json::json!({"orderId":"order-42",
            "linePrices":[{"itemId":"recognition:item:8:0","amount":236000}]});
        assert!(quoted_item_line(&legacy,"order-42:recognition:item:8:0").is_some());
        assert!(quoted_item_line(&legacy,"another-order:recognition:item:8:0").is_none());
        assert!(quoted_item_line(&quote,"recognition:item:9:0").is_none());
        assert!(quoted_freight_line(&quote,"recognition:recipient:1",None).is_none());
        assert!(!quote_id_matches(&quote,
            Some("another-order:recognition:item:8:0"),
            "recognition:item:8:0","recognition:item:"));
    }
}

fn current_inline_value(
    recognition:&serde_json::Value,
    quote:Option<&serde_json::Value>,
    target:&InlineTarget
)->Option<String>{
    let recipient=recognition.get("recipients")?.as_array()?.iter().find(|row|
        row["id"].as_str()==Some(target.recipient_id.as_str())
    )?;
    if let Some(item_id)=target.item_id.as_deref(){
        let item=recipient.get("items")?.as_array()?.iter().find(|row|
            row["id"].as_str()==Some(item_id)
        )?;
        return match target.field {
            "product"=>Some(reception_service::item_title(item)),
            "quantity"=>Some(item["quantity"].as_i64().map(|v|v.to_string())
                .unwrap_or_default()),
            "unitPrice"=>Some(item.get("manualUnitPrice")
                .or_else(||item.get("unitPrice"))
                .and_then(serde_json::Value::as_i64)
                .or_else(||quote.and_then(|value|quoted_item_line(value,item_id))
                    .and_then(|line|line["unitPrice"].as_i64()))
                .map(|v|v.to_string()).unwrap_or_default()),
            _=>None
        };
    }
    if target.field=="shippingCharge"{
        return Some(quoted_billable_freight(quote,recipient)
            .map(|v|v.to_string()).unwrap_or_default());
    }
    recipient[target.field].as_str().map(str::to_owned)
}

struct PendingWorkingSync {
    order_id: String,
    source_raw: String,
    order_type: String,
    client_id: String,
    authority_revision: Option<u64>,
    submitted_manual: Option<serde_json::Value>,
    receiver: Receiver<Result<serde_json::Value,String>>,
}

fn kakao_safe_modal_work_area(
    viewport:Rect,layout:&layout::LayoutState
)->Rect{
    // Match workspace_shell_frame exactly: the reception/management canvas
    // has NO left inset and a single PANEL_GAP inset on the right.
    let content_left=viewport.left();
    let content_right=(viewport.right()-theme::PANEL_GAP)
        .max(content_left+1.0);
    let content_width=(content_right-content_left)
        .max(layout::LayoutState::minimum_width());
    let kakao_edge=content_left+layout.widths(content_width)[0]
        +layout::SPLITTER;
    let gutter=theme::MODAL_VIEWPORT_GUTTER;
    let left=(kakao_edge+gutter).min(content_right-1.0);
    let right=(content_right-gutter).max(left+1.0).min(content_right);
    let top=(viewport.top()+theme::HEADER_HEIGHT+theme::PANEL_GAP+gutter)
        .min(viewport.bottom()-1.0);
    let bottom=(viewport.bottom()-theme::PANEL_GAP-gutter)
        .max(top+1.0).min(viewport.bottom());
    Rect::from_min_max(egui::pos2(left,top),egui::pos2(right,bottom))
}

// Only exact saved analysis inputs may reuse the hovered review projection.
// Do not store a second WorkingReception or its edit/audit history.
struct PendingReviewBasis {
    analysis_raw:Option<String>,evidence:Option<serde_json::Value>,view:Option<serde_json::Value>,
    manual:Option<serde_json::Value>,client_match:Option<serde_json::Value>,
    client_id:Option<String>,order_type:String,client_selection:Option<String>,
}
impl PendingReviewBasis {
    fn new(draft:&WorkingReception)->Self{
        Self{analysis_raw:draft.analysis_raw.clone(),evidence:draft.analysis_evidence.clone(),
            view:draft.analysis_view.clone(),manual:draft.manual_model.clone(),
            client_match:draft.client_match.clone(),client_id:draft.client_id.clone(),
            order_type:draft.order_type.clone(),client_selection:draft.client_selection.clone()}
    }
    fn matches(&self,draft:&WorkingReception)->bool{
        self.analysis_raw==draft.analysis_raw&&self.evidence==draft.analysis_evidence&&
        self.view==draft.analysis_view&&self.manual==draft.manual_model&&
        self.client_match==draft.client_match&&self.client_id==draft.client_id&&
        self.order_type==draft.order_type&&self.client_selection==draft.client_selection
    }
}
// Read-only render projection; do not duplicate receipt audit history.
struct PendingDisplay {
    id:String,raw:String,recognition:serde_json::Value,
    pricing_preview:Option<serde_json::Value>,spans:Vec<source_highlight::LinkedSpan>,
    identity_input:Option<serde_json::Value>,
    review_basis:PendingReviewBasis,
    review_analysis:Option<reception_service::ReceptionAnalysis>,
}
struct WorkflowSourceDisplay{
    board:Arc<workflow_board::BoardSnapshot>,order_id:String,raw:String,
    spans:Arc<[source_highlight::LinkedSpan]>,
}
struct BandoEgui {
    brand_texture: Option<egui::TextureHandle>,
    gateway: Arc<dyn gateway::BandoGateway>,
    page: Page,
    navigation_snapshots: view_state::NavigationSnapshots<8>,
    layout: layout::LayoutState,
    window_state: window_state::Tracker,
    dragging_split: Option<HorizontalSplitDrag>,
    dragging_vertical: Option<VerticalSplitDrag>,
    raw_source: String,
    drafts: Vec<WorkingReception>,
    active_id: Option<String>,
    source_editing: bool,
    // Only the explicit "직접 입력" intake route may modify the source
    // document. Imported or reopened source documents remain read-only.
    direct_source_entry: bool,
    order_type: OrderType,
    clipboard_monitor: ClipboardMonitor,
    kakao_tracker: KakaoTracker,
    kakao_dock: kakao_dock::KakaoDock,
    kakao_dock_error: Option<String>,
    clipboard_chat: Option<ChatWindow>,
    clipboard_busy: bool,
    ocr_processing: bool,
    // Thumbnail is delivered as soon as clipboard/capture decoding finishes,
    // before the slower PaddleOCR worker returns its recognition result.
    ocr_preview_receiver: Option<Receiver<ClipboardImage>>,
    ocr_processing_preview: Option<(egui::TextureHandle,usize,usize)>,
    clipboard_cancel: Arc<AtomicBool>,
    clipboard_receiver: Option<Receiver<(OrderType, Result<ClipboardReceipt, String>)>>,
    clipboard_error: Option<String>,
    ocr_review: Option<OcrReview>,
    discard_ocr_confirm: bool,
    pending_replace: Option<PendingReplace>,
    pending_clipboard_clear: Option<PendingClipboardClear>,
    pending_draft_delete: Option<(String,bool)>,
    source_autosave: Option<source_autosave::SourceAutosave>,
    source_save_error: Option<String>,
    client_chooser: Option<ClientChooser>,
    client_manager: Option<client_manager::ClientManager>,
    product_manager: Option<product_manager::ProductManager>,
    settings_view: Option<settings_view::SettingsView>,
    // Read the SAVED image-inbox directory, not the unsaved Settings draft.
    // Loading is background-only; the reception frame never opens SQLite.
    inbox_folder_path: Option<String>,
    inbox_folder_loaded: bool,
    inbox_folder_error: Option<String>,
    inbox_folder_open_error: Option<String>,
    // One reception-wide revision check owns metadata invalidation.
    reception_metadata_read:screen_refresh::ReadRefresh<(Option<u64>,Option<gateway::ReceptionMetadata>)>,
    reception_metadata_last_probe:Option<Instant>,
    reception_metadata_revision:Option<u64>,
    reception_metadata_ready:bool,
    freight_manager: Option<freight_manager::FreightManager>,
    order_history_manager: Option<order_history_manager::OrderHistoryManager>,
    daily_settlement_manager: Option<daily_settlement_manager::DailySettlementManager>,
    excel_manager: Option<excel_manager::ExcelManager>,
    factory_reply_inbox:factory_reply_inbox::Inbox,
    notification_center:notification_center::Center,
    freight_pending_page: Option<Page>,
    freight_pending_save: bool,
    product_pending_page: Option<Page>,
    client_pending_page: Option<Page>,
    settings_pending_page: Option<Page>,
    settings_pending_save: bool,
    manual_workspace: Option<manual_workspace::ManualWorkspace>,
    item_editor:Option<item_editor::ItemEditor>,
    product_picker:Option<(String,String,String,product_manager::ProductManager)>,
    inline_editor:Option<InlineReceptionEditor>,
    inline_validation_focus:Option<(InlineTarget,String)>,
    guided_inline_target:Option<InlineTarget>,
    pending_input_guidance:Option<(String,String,Option<String>,Option<floating_notice::Anchor>)>,
    inline_change:Option<PendingInlineChange>,
    vat_dialog:Option<VatDialog>,
    vat_change:Option<PendingVatChange>,
    remote_recheck:Option<PendingRemoteRecheck>,
    remote_recheck_due:bool,
    edit_history_open: bool,
    client_change: Option<PendingClientChange>,
    auto_client_change: Option<PendingAutomaticClient>,
    pricing_basis_check: Option<PendingPricingBasisCheck>,
    working_sync: Option<PendingWorkingSync>,
    working_sync_error: Option<String>,
    analysis: Option<reception_service::ReceptionAnalysis>,
    review_panel: review_panel::ReviewPanel,
    pending_review_panel:review_panel::ReviewPanel,
    analysis_error: Option<String>,
    selected_recipient: usize,
    requested_recipient_focus:Option<(String,String)>,
    source_hover: Option<source_highlight::LinkKey>,
    link_hover: source_highlight::HoverState,
    workflow_snapshot: Option<Arc<workflow_board::BoardSnapshot>>,
    pending_display:Option<Arc<PendingDisplay>>,
    workflow_source_display:Option<WorkflowSourceDisplay>,
    workflow_error: Option<String>,
    // Refresh failures belong to the board, not to whichever group is visible.
    workflow_read_error: Option<String>,
    workflow_read: screen_refresh::ReadRefresh<Option<workflow_board::BoardSnapshot>>,
    /// A committed write must publish its new revision before another
    /// operational write; an ordinary periodic read is advisory.
    workflow_last_fetch: Option<Instant>,
    workflow_ui: workflow_board_ui::BoardUi,
    workflow_action: Option<PendingWorkflowAction>,
    workflow_freight_editor:Option<WorkflowFreightEditor>,
    confirmed_cell:Option<ConfirmedCellEditor>,
    submitted_confirmed_cell:Option<ConfirmedCellEditor>,
    confirmed_next_cell:Option<(workflow_board_ui::Selection,InlineTarget,Option<egui::Pos2>,bool)>,
    confirmed_modal_target:Option<ConfirmedEditTarget>,
    workflow_freight_next_selection:Option<workflow_board_ui::Selection>,
    submitted_workflow_freight:Option<WorkflowFreightEditor>,
    workflow_stage_after_freight:Option<(String,usize,bool)>,
    workflow_freight_guidance:Option<String>,
    /// Workflow approval requested while this same draft's inline cell is
    /// committing. Resume only after the native edit succeeds.
    pending_draft_confirmation:Option<String>,
    // SQLite committed; keep the old card until the verified board is ready.
    committed_draft_handoff:Option<String>,
    // Explicit stage click is independent of transient pointer preview.
    pending_stage_display:Option<workflow_board_ui::PendingTarget>,
    // React's original stage safety rule: repeated stage taps within 2s
    // must not silently advance the same group twice.
    workflow_stage_deadlines: HashMap<String, Instant>,
    workflow_dialog: Option<WorkflowDialog>,
    workflow_manage_target: String,
    workflow_manage_ack: bool,
    // Names remain the sole input for pending workflow cards. Reception also
    // keeps an ephemeral, READ-ONLY presentation index from the real client
    // master; no operational client data is persisted in preview SQLite.
    client_names: HashMap<String,String>,
    client_summaries: HashMap<String,crate::composite_client::ClientDisplaySummary>,

    analysis_request: u64,
    analysis_pending: bool,
    analysis_due_at: Option<Instant>,
    analysis_running_key: Option<(String,String,String)>,
    analysis_receiver: Option<Receiver<(u64, Result<reception_service::ReceptionAnalysis, String>)>>,
}

impl Default for BandoEgui {
    fn default() -> Self {
        Self::with_layout(layout::LayoutState::load())
    }
}

impl BandoEgui {
    fn with_layout(layout:layout::LayoutState)->Self{
        Self {
            brand_texture: None,
            gateway: Arc::new(gateway::LocalGateway),
            page: Page::Reception,
            navigation_snapshots:view_state::NavigationSnapshots::new(Page::Reception as usize),
            layout,
            window_state: window_state::Tracker::default(),
            dragging_split: None,
            dragging_vertical: None,
            raw_source: String::new(),
            drafts: Vec::new(),
            active_id: None,
            source_editing: false,
            direct_source_entry: false,
            order_type: OrderType::Agency,
            clipboard_monitor: ClipboardMonitor::default(),
            kakao_tracker: KakaoTracker::default(),
            kakao_dock: kakao_dock::KakaoDock::default(),
            kakao_dock_error: None,
            clipboard_chat: None,
            clipboard_busy: false,
            ocr_processing: false,
            ocr_preview_receiver: None,
            ocr_processing_preview: None,
            clipboard_cancel: Arc::new(AtomicBool::new(false)),
            clipboard_receiver: None,
            clipboard_error: None,
            ocr_review: None,
            discard_ocr_confirm: false,
            pending_replace: None,
            pending_clipboard_clear: None,
            pending_draft_delete: None,
            source_autosave: None,
            source_save_error: None,
            client_chooser: None,
            client_manager: None,
            product_manager: None,
            settings_view: None,
            inbox_folder_path: None,
            inbox_folder_loaded: false,
            inbox_folder_error: None,
            inbox_folder_open_error: None,
            reception_metadata_read:Default::default(),
            reception_metadata_last_probe:None,
            reception_metadata_revision:None,
            reception_metadata_ready:false,
            freight_manager: None,
            order_history_manager: None,
            daily_settlement_manager: None,
            excel_manager: None,
            factory_reply_inbox:factory_reply_inbox::Inbox::default(),
            notification_center:Default::default(),
            freight_pending_page: None,
            freight_pending_save: false,
            product_pending_page: None,
            client_pending_page: None,
            settings_pending_page: None,
            settings_pending_save: false,
            manual_workspace: None,
            item_editor:None,
            product_picker:None,
            inline_editor:None,
            inline_validation_focus:None,
            guided_inline_target:None,
            pending_input_guidance:None,
            inline_change:None,
            vat_dialog:None,
            vat_change:None,
            remote_recheck:None,
            remote_recheck_due:false,
            edit_history_open: false,
            client_change: None,
            auto_client_change: None,
            pricing_basis_check: None,
            working_sync: None,
            working_sync_error: None,
            analysis: None,
            review_panel: review_panel::ReviewPanel::default(),
            pending_review_panel:review_panel::ReviewPanel::default(),
            analysis_error: None,
            selected_recipient: 0,
            requested_recipient_focus:None,
            source_hover: None,
            link_hover: source_highlight::HoverState::default(),
            workflow_snapshot: None,
            pending_display:None,
            workflow_source_display:None,
            workflow_error: None,
            workflow_read_error: None,
            workflow_read:Default::default(),
            workflow_last_fetch: None,
            workflow_ui: workflow_board_ui::BoardUi::default(),
            workflow_action: None,
            workflow_freight_editor:None,
            confirmed_cell:None,
            submitted_confirmed_cell:None,
            confirmed_next_cell:None,
            confirmed_modal_target:None,
            workflow_freight_next_selection:None,
            submitted_workflow_freight:None,
            workflow_stage_after_freight:None,
            workflow_freight_guidance:None,
            pending_draft_confirmation:None,
            committed_draft_handoff:None,
            pending_stage_display:None,
            workflow_stage_deadlines: HashMap::new(),
            workflow_dialog: None,
            workflow_manage_target: String::new(),
            workflow_manage_ack: false,
            client_names: HashMap::new(),
            client_summaries: HashMap::new(),

            analysis_request: 0,
            analysis_pending: false,
            analysis_due_at: None,
            analysis_running_key: None,
            analysis_receiver: None,
        }
    }
}

impl BandoEgui {
    /// Single admission snapshot for native workers, source input/review
    /// and operational group mutations. The CAS-serialized autosave sidecar
    /// deliberately remains independent of user-request admission.
    fn active_reception_jobs(&self,request:ReceptionJob)->Vec<ReceptionJob>{
        let mut active=Vec::with_capacity(21);
        if self.client_change.is_some(){active.push(ReceptionJob::ManualClient);}
        if self.auto_client_change.is_some(){active.push(ReceptionJob::AutomaticClient);}
        if self.pricing_basis_check.is_some(){active.push(ReceptionJob::PricingBasis);}
        if self.inline_change.is_some(){active.push(ReceptionJob::InlineEdit);}
        if self.vat_change.is_some(){active.push(ReceptionJob::VatEdit);}
        if self.working_sync.is_some(){active.push(ReceptionJob::WorkingSync);}
        if self.manual_workspace.is_some(){active.push(ReceptionJob::ManualWorkspace);}
        if self.item_editor.is_some() || self.product_picker.is_some(){active.push(ReceptionJob::ItemDialog);}
        // A changed cell is uncommitted employee input. A clean, unchanged
        // editor may be dismissed by navigation or an approved draft delete;
        // other actions keep their existing edit-focus protection.
        if request!=ReceptionJob::InlineEdit &&
            self.inline_editor.as_ref().is_some_and(|editor|
                editor.text!=editor.original ||
                !matches!(request,
                    ReceptionJob::NavigateReception|ReceptionJob::DeleteReception)
            ){
            active.push(ReceptionJob::InlineEditor);
        }
        if self.confirmed_cell.is_some(){active.push(ReceptionJob::InlineEditor);}
        if self.analysis_pending {active.push(ReceptionJob::Analysis);}
        if self.remote_recheck.is_some(){active.push(ReceptionJob::RemoteRecheck);}
        if self.clipboard_busy{active.push(ReceptionJob::SourceIntake);}
        if self.ocr_review.is_some() || self.pending_replace.is_some(){
            active.push(ReceptionJob::SourceReview);
        }
        if self.direct_source_entry{active.push(ReceptionJob::SourceEntry);}
        if let Some(action)=self.workflow_action.as_ref(){
            active.push(if action.action=="set_shipment_freight"{
                ReceptionJob::WorkflowFreightWrite
            }else{ReceptionJob::WorkflowAction});
        }
        if self.client_chooser.is_some(){active.push(ReceptionJob::ClientChooser);}
        if self.vat_dialog.is_some(){active.push(ReceptionJob::VatDialog);}
        if self.workflow_dialog.is_some(){active.push(ReceptionJob::WorkflowDialog);}
        active
    }

    fn confirming_order(&self)->Option<&str>{
        self.committed_draft_handoff.as_deref()
            .or_else(||self.workflow_action.as_ref()
                .filter(|a|a.action=="confirm_reception").map(|a|a.group_id.as_str()))
            .and_then(|id|id.strip_prefix("group:"))
    }

    fn pending_stage_target(&self)->Option<&workflow_board_ui::PendingTarget>{
        self.pending_stage_display.as_ref().filter(|target|
            self.confirming_order()==Some(target.order_id.as_str()) ||
            self.pending_draft_confirmation.as_deref()==Some(target.order_id.as_str()))
    }

    fn active_confirmation_locked(&self)->bool{
        self.confirming_order().is_some_and(|id|self.active_id.as_deref()==Some(id))
    }

    fn active_delete_locked(&self)->bool{
        self.pending_draft_delete.as_ref().is_some_and(|(id,_)|
            self.active_id.as_deref()==Some(id.as_str()))
    }

    fn reception_job_admission(
        &self,request:ReceptionJob,same_request:bool
    )->JobAdmission{
        if self.pending_draft_delete.is_some() &&
            (request==ReceptionJob::DeleteReception ||
                (self.active_delete_locked() && request!=ReceptionJob::NavigateReception)){
            return JobAdmission::Conflict(ReceptionJob::DeleteReception);
        }
        // Keep this exact draft immutable while its native confirmation or
        // mandatory projection is pending; unrelated drafts retain admission.
        if self.active_confirmation_locked() &&
            !matches!(request,ReceptionJob::NavigateReception){
            return JobAdmission::Conflict(ReceptionJob::WorkflowAction);
        }
        reception_jobs::admit(
            request,&self.active_reception_jobs(request),same_request
        )
    }

    fn reception_job_conflict(&mut self,request:ReceptionJob,
                              blocker:ReceptionJob){
        self.source_save_error=Some(format!(
            "{} 중에는 {} 작업을 시작할 수 없습니다.",
            blocker.label(),request.label()
        ));
    }

    /// Shared metadata clock: one native read-only revision probe for the
    /// board, saved folder path and client presentation cache. Unchanged
    /// revisions never hydrate product/order or client rows.
    fn poll_reception_metadata(&mut self,ctx:&egui::Context){
        if self.page!=Page::Reception{
            // In-flight reads belong to the old visit. On re-entry compare
            // the persisted revision immediately; adapters without a native
            // revision re-read once per visit.
            self.reception_metadata_read.cancel();
            self.reception_metadata_last_probe=None;
            if self.reception_metadata_revision.is_none(){
                self.reception_metadata_ready=false;
            }
            return;
        }
        if let Some(result)=self.reception_metadata_read.poll(){
            match result{
                Ok((revision,metadata))=>{
                    self.reception_metadata_read.verified();
                    self.reception_metadata_revision=revision;
                    self.reception_metadata_ready=true;
                    if let Some(metadata)=metadata{
                        match metadata.inbox{
                            Ok(path)=>{
                                if self.inbox_folder_path!=path{
                                    self.inbox_folder_path=path;
                                    self.inbox_folder_open_error=None;
                                }
                                self.inbox_folder_loaded=true;
                                self.inbox_folder_error=None;
                            }
                            Err(error)=>{
                                self.inbox_folder_error=Some(error);
                                self.inbox_folder_loaded=true;
                            }
                        }
                        if let Ok(clients)=metadata.clients{
                            let mut names=HashMap::new();
                            let mut summaries=HashMap::new();
                            for client in clients{
                                if client.get("deletedAt").is_some_and(|v|!v.is_null()){continue;}
                                let id=client["id"].as_str().unwrap_or("");
                                if id.is_empty(){continue;}
                                let summary=crate::composite_client::ClientDisplaySummary::from_client(&client);
                                if client["active"]!=serde_json::Value::Bool(false) && !summary.name.is_empty(){
                                    names.insert(id.to_owned(),summary.name.clone());
                                }
                                summaries.insert(id.to_owned(),summary);
                            }
                            if self.client_names!=names{self.client_names=names;}
                            if self.client_summaries!=summaries{self.client_summaries=summaries;}
                        }
                    }
                    ctx.request_repaint();
                }
                Err(error)=>{
                    // Retain the last known good caches during a transient
                    // failure. Do not claim a new revision was observed.
                    if !self.inbox_folder_loaded{self.inbox_folder_error=Some(error);}
                }
            }
        }
        if self.reception_metadata_read.pending(){return;}
        // No local DB revision is available in isolated/HTTP adapters.
        if self.reception_metadata_ready && self.reception_metadata_revision.is_none(){return;}
        if self.reception_metadata_last_probe.is_some_and(|at|at.elapsed()<Duration::from_secs(5)){return;}
        self.reception_metadata_last_probe=Some(Instant::now());
        let known=self.reception_metadata_revision;
        let gateway=Arc::clone(&self.gateway);
        self.reception_metadata_read.submit(ctx.clone(),move||gateway.reception_metadata(known));
    }

    // Hidden management screens already refresh on every entry. Do not
    // start full history/settlement reads for pages the user is not viewing.
    fn refresh_after_factory_reply(&mut self,ctx:&egui::Context,corrected:bool,imported:bool){
        if !corrected && !imported{return;}
        // Keep the last known revision as the probe baseline, but reject an
        // in-flight pre-commit result. The native probe detects other edits.
        self.reception_metadata_read.cancel();
        self.reception_metadata_last_probe=None;
        self.workflow_read.cancel();
        self.workflow_last_fetch=None;
        self.workflow_read.require();
        // Correction already projects the verified committed snapshot.
        // Only a separate inbox commit needs another Excel read.
        if imported{
            if let Some(excel)=self.excel_manager.as_mut(){excel.request_reply_refresh();}
        }
        match self.page{
            Page::History=>if let Some(history)=self.order_history_manager.as_mut(){
                history.refresh(ctx.clone());
            },
            Page::Settlement=>if let Some(settlement)=self.daily_settlement_manager.as_mut(){
                settlement.refresh(ctx.clone());
            },
            _=>{}
        }
    }

    fn invalidate_reception_metadata(&mut self){
        self.reception_metadata_read.cancel();
        self.reception_metadata_last_probe=None;
        self.reception_metadata_revision=None;
        self.reception_metadata_ready=false;
    }

    // Run parsing outside the egui frame loop. Any changed source invalidates
    // the previous result and its request sequence, so stale replies are ignored.
    fn queue_source_save(&mut self) {
        self.queue_source_save_with_feedback(None);
    }

    fn source_snapshot(&mut self)->Result<WorkingSource,String>{
        if let Some(active_id) = self.active_id.as_deref() {
            let Some(active) = self.drafts.iter_mut().find(|item| item.id == active_id) else {
                return Err("활성 발주서를 찾을 수 없습니다.".to_owned());
            };
            if source_autosave::update_draft_source(
                active,&self.raw_source,self.order_type.as_str()
            ){
                // In-flight advice belongs to the old raw/kind snapshot.
                self.auto_client_change=None;
            }
        }
        // Capture a screen-only projection alongside the immutable source
        // evidence. Saving unrelated state must not destroy a valid snapshot.
        if let (Some(active_id),Some(analysis))=(
            self.active_id.as_deref(),self.analysis.as_ref()
        ){
            if let Some(row)=self.drafts.iter_mut().find(|row|
                row.id==active_id &&
                row.raw==self.raw_source &&
                row.analysis_raw.as_deref()==Some(self.raw_source.as_str())
            ){
                row.analysis_view=Some(analysis.cached_view());
            }
        }
        Ok(WorkingSource {
            raw: self.raw_source.clone(),
            order_type: self.order_type.as_str().to_owned(),
            revision: 0, // The serialized autosave worker owns the latest revision.
            active_id: self.active_id.clone(),
            drafts: self.drafts.clone(),
        })
    }

    fn queue_source_save_with_feedback(&mut self, feedback:Option<OcrFeedback>) {
        self.source_save_error = None;
        let source=match self.source_snapshot(){
            Ok(source)=>source,
            Err(error)=>{self.source_save_error=Some(error);return;}
        };
        if let Some(worker) = &self.source_autosave {
            if let Err(error) = worker.queue_with_feedback(
                source, feedback.into_iter().collect()
            ) {
                self.source_save_error = Some(error);
            }
        } else {
            self.source_save_error = Some("격리 원문 저장소가 초기화되지 않았습니다.".to_owned());
        }
    }

    /// Direct typing updates one draft in memory and transmits only that
    /// draft's raw/input metadata. All other receipts stay in the worker's
    /// last complete snapshot until a structural edit queues a full Save.
    fn queue_source_text_edit(&mut self){
        self.source_save_error=None;
        let Some(active_id)=self.active_id.as_deref() else{
            self.source_save_error=Some("활성 발주서를 찾을 수 없습니다.".into());
            return;
        };
        let Some(row)=self.drafts.iter_mut().find(|row|row.id==active_id)
        else{
            self.source_save_error=Some("활성 발주서를 찾을 수 없습니다.".into());
            return;
        };
        if source_autosave::update_draft_source(
            row,&self.raw_source,self.order_type.as_str()
        ){
            self.auto_client_change=None;
        }
        let edit=source_autosave::SourceTextEdit{
            order_id:row.id.clone(),
            raw:self.raw_source.clone(),
            order_type:self.order_type.as_str().into(),
            source_input_method:row.source_input_method.clone(),
            last_input_method:row.last_input_method.clone(),
        };
        if let Some(worker)=&self.source_autosave{
            if let Err(error)=worker.queue_source_edit(edit){
                self.source_save_error=Some(error);
            }
        }else{
            self.source_save_error=Some(
                "격리 원문 저장소가 초기화되지 않았습니다.".into()
            );
        }
    }

    /// Rebuild the reception worksheet projection from the saved direct
    /// manual model. This is display data only: it never fabricates OCR,
    /// source evidence, linked spans or an analysis cache.
    fn restore_direct_manual_view(&mut self)->bool{
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|item|item.id==id))
            .cloned() else{return false};
        if !row.raw.trim().is_empty(){return false;}
        if row.manual_model.as_ref().and_then(|model|model["mode"].as_str())!=Some("DIRECT"){
            return false;
        }
        let Some(mut analysis)=reception_service::ReceptionAnalysis::from_working_source(&row)
            else{return false;};
        let count=analysis.recognition["recipients"].as_array().map(Vec::len).unwrap_or(0);
        analysis.chosen_client_name=row.client_id.as_deref()
            .and_then(|id|self.client_names.get(id)).cloned();
        self.analysis=Some(analysis);
        self.analysis_pending=false;
        self.analysis_error=None;
        self.analysis_due_at=None;
        self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
        self.source_hover=None;
        self.link_hover.clear();
        self.remote_recheck_due=true;
        true
    }

    /// Restore exact Rust recognition and its display references from the
    /// source-bound SQLite snapshot. Direct-entry rows instead rebuild from
    /// their employee-authored manual model and never synthesize OCR evidence.
    fn restore_cached_analysis(&mut self)->bool {
        if self.restore_direct_manual_view(){return true;}
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|item|item.id==id))
        else{return false};
        if row.raw!=self.raw_source ||
           row.order_type!=self.order_type.as_str() ||
           row.analysis_raw.as_deref()!=Some(self.raw_source.as_str()) {
            return false;
        }
        let Some(restored)=reception_service::ReceptionAnalysis::from_working_source(row)
            else{return false;};
        self.analysis=Some(restored);
        self.analysis_pending=false;
        self.analysis_error=None;
        self.analysis_due_at=None;
        self.selected_recipient=0;
        true
    }

    /// A cache hit is rendered immediately. Only the selected client's
    /// relevant product/variant and freight masters are compared off-thread.
    /// Unrelated state revisions and contact edits never trigger repricing.
    fn start_pricing_basis_check(&mut self,ctx:&egui::Context){
        if self.reception_job_admission(
            ReceptionJob::PricingBasis,self.pricing_basis_check.is_some()
        )!=JobAdmission::Start{return;}
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
            .cloned()
        else{return};
        let Some(client_id)=row.client_id.clone() else{return};
        if self.gateway.isolated_client_identity_link(){return;}
        let Some(quote)=row.pricing_preview.clone()
            .filter(complete_reception_quote)
        else{
            // A legacy or unpriced reception is not a cache hit. The same
            // dealer is repriced once while preserving AUTO/MANUAL ownership.
            self.apply_client_with_selection(
                &client_id,ctx,row.client_selection.clone()
            );
            return;
        };
        let Some(rows)=quote.pointer("/pricingBasis/orderInputs/recipients")
            .or_else(||row.manual_model.as_ref()
                .and_then(|model|model.pointer("/model/rows")))
            .or_else(||row.analysis_view.as_ref()
                .and_then(|view|view.pointer("/recognition/recipients")))
            .or_else(||row.analysis_evidence.as_ref()
                .and_then(|evidence|evidence.get("recipients")))
            .filter(|rows|rows.is_array()).cloned()
        else{return};
        let gateway=Arc::clone(&self.gateway);
        let old_quote=quote.clone();
        let selected_id=client_id.clone();
        let mut read=screen_refresh::ReadRefresh::default();
        read.submit(ctx.clone(),move||->Result<bool,String>{
                let state=gateway.load_business_state()?;
                let client=state.get("clients").and_then(serde_json::Value::as_array)
                    .and_then(|clients|clients.iter().find(|client|
                        client["id"].as_str()==Some(selected_id.as_str())))
                    .ok_or("저장된 거래처의 가격 기준을 찾지 못했습니다.")?;
                let order=serde_json::json!({"recipients":rows});
                Ok(old_quote.get("pricingBasis")==Some(
                    &pricing_basis::snapshot(&state,client,&order)
                ))
        });
        self.pricing_basis_check=Some(PendingPricingBasisCheck{
            order_id:row.id,source_raw:row.raw,order_type:row.order_type,
            client_id,quote,read
        });
    }

    fn poll_pricing_basis_check(&mut self,ctx:&egui::Context){
        let received=self.pricing_basis_check.as_mut().and_then(|pending|pending.read.poll());
        let Some(received)=received else{
            return;
        };
        let Some(pending)=self.pricing_basis_check.take() else{return};
        let latest=self.drafts.iter().find(|row|row.id==pending.order_id);
        if self.active_id.as_deref()!=Some(pending.order_id.as_str()) ||
           self.raw_source!=pending.source_raw ||
           self.order_type.as_str()!=pending.order_type ||
           latest.is_none_or(|row|
               row.client_id.as_deref()!=Some(pending.client_id.as_str()) ||
               row.pricing_preview.as_ref()!=Some(&pending.quote))
        {return;}
        match received{
            Ok(true)=>{},
            Ok(false)=>{
                if self.client_change.is_some() || self.auto_client_change.is_some() ||
                   self.inline_change.is_some() || self.vat_change.is_some() ||
                   self.working_sync.is_some() || self.manual_workspace.is_some() ||
                   self.inline_editor.is_some(){
                    // Do not race with an employee edit. A later source
                    // selection/reopen will check the same saved basis.
                    return;
                }
                let kind=latest.and_then(|row|row.client_selection.clone());
                self.apply_client_with_selection(&pending.client_id,ctx,kind);
            }
            Err(error)=>{
                self.source_save_error=Some(
                    format!("저장된 견적의 가격 기준 확인 실패: {error}")
                );
            }
        }
    }

    // The workflow and metadata caches share the same observed native revision.
    fn workflow_mutation_ready(&self)->bool{
        workflow_write_admission(
            self.workflow_snapshot.as_ref().and_then(|board|board.revision),
            self.workflow_action.is_some(),self.workflow_read.required(),
            self.workflow_error.is_some()
        )
    }

    fn discard_advisory_workflow_read(&mut self){
        if self.workflow_read.discard_advisory(){
            // A dropped receiver cannot replace the editor's before-image.
            // A fresh poll will run when the editor/action is released.
            self.workflow_last_fetch=None;
        }
    }

    /// Publish the confirmed group and retire the temporary workspace in
    /// the same UI update. A projection error is not a failed DB commit.
    fn finish_confirmed_draft_handoff(&mut self,snapshot:&workflow_board::BoardSnapshot,
        ctx:&egui::Context)->bool{
        let Some(group_id)=self.committed_draft_handoff.as_deref()else{return true;};
        let Some(order_id)=group_id.strip_prefix("group:")else{return false;};
        let Some(group)=snapshot.groups.iter().find(|g|g.id==group_id&&g.step>=1)
            else{return false;};
        let was_active=self.active_id.as_deref()==Some(order_id);
        let clicked=self.pending_stage_target().filter(|target|target.order_id==order_id);
        let preferred=clicked.and_then(|target|target.recipient_id.as_deref())
            .or_else(||if was_active{
                self.analysis.as_ref().and_then(|a|reception_service::recipients(a)
                    .get(self.selected_recipient)).and_then(|r|r["id"].as_str())
            }else{None});
        let target=group.shipments.iter().find(|s|s.order_id==order_id&&
            preferred.is_some_and(|id|s.recipient_id==id))
            .or_else(||group.shipments.iter().find(|s|s.order_id==order_id));
        let Some(target)=target else{return false;};
        let selection=workflow_board_ui::Selection{
            group_id:group.id.clone(),order_id:target.order_id.clone(),shipment_id:target.id.clone()
        };
        let order_id=order_id.to_owned();
        let keep_selected=clicked.is_some() || (was_active&&self.workflow_ui.selection.is_none());
        let was_folded=self.workflow_ui.folded.as_deref()==Some(format!("pending:{order_id}").as_str()) ||
            (clicked.is_some() && (!was_active || self.workflow_ui.selection.is_some()));
        self.drafts.retain(|d|d.id!=order_id);
        self.committed_draft_handoff=None;
        self.pending_stage_display=None;
        self.pending_display=None;
        self.pending_review_panel.clear();
        if was_active{
            self.active_id=None;
            self.raw_source.clear();
            self.source_editing=false;
            self.direct_source_entry=false;
            self.requested_recipient_focus=None;
            self.clear_source_analysis();
        }
        if keep_selected{
            self.workflow_ui.select(selection);
            if was_folded{self.workflow_ui.folded=Some(group.id.clone());}
        }
        self.queue_source_save();
        ctx.request_repaint();
        true
    }

    // Never fall back to an unrelated order when a refreshed projection
    // moves the same shipment to another group (e.g. regrouping).
    fn reconcile_workflow_selection(&mut self,snapshot:&workflow_board::BoardSnapshot){
        let Some(selected)=self.workflow_ui.selection.clone()else{return;};
        if snapshot.groups.iter().any(|group|group.id==selected.group_id &&
            group.shipments.iter().any(|ship|
                ship.id==selected.shipment_id && ship.order_id==selected.order_id)){
            return;
        }
        let old_recipient=self.workflow_snapshot.as_ref().and_then(|board|
            board.groups.iter().find(|group|group.id==selected.group_id))
            .and_then(|group|group.shipments.iter().find(|ship|
                ship.id==selected.shipment_id && ship.order_id==selected.order_id))
            .map(|ship|ship.recipient_id.as_str());
        let candidates=||snapshot.groups.iter().flat_map(|group|
            group.shipments.iter().map(move |ship|(group,ship)))
            .filter(|(_,ship)|ship.order_id==selected.order_id);
        let replacement=candidates().find(|(_,ship)|ship.id==selected.shipment_id)
            .or_else(||{
                let mut matches=candidates().filter(|(_,ship)|
                    old_recipient.is_some_and(|id|ship.recipient_id==id));
                let first=matches.next()?;
                // A split can create several deliveries for one recipient.
                // Without a stable shipment ID, do not guess between them.
                matches.next().is_none().then_some(first)
            })
            .map(|(group,ship)|workflow_board_ui::Selection{
                group_id:group.id.clone(),order_id:ship.order_id.clone(),shipment_id:ship.id.clone()
            });
        let was_folded=self.workflow_ui.folded.as_deref()==Some(selected.group_id.as_str());
        self.confirmed_cell=None;
        if let Some(target)=replacement{
            let group=target.group_id.clone();
            self.workflow_ui.select(target);
            if was_folded{self.workflow_ui.folded=Some(group);}
        }else{
            self.workflow_ui.selection=None;
            self.workflow_ui.folded=None;
            self.workflow_ui.stop_preview();
        }
    }

    fn poll_workflow_board(&mut self,ctx:&egui::Context){
        if let Some(result)=self.workflow_read.poll(){
            match result{
                Ok(None)=>{
                    // An unchanged advisory revision is not a new UI snapshot.
                    // A mandatory refresh still requires a verified projection.
                }
                Ok(Some(snapshot))=>{
                    if !self.finish_confirmed_draft_handoff(&snapshot,ctx){
                        self.workflow_error=Some("주문은 저장됐지만 확정 화면 갱신을 완료하지 못했습니다.".into());
                        self.workflow_read_error=self.workflow_error.clone();
                        self.workflow_read.require();
                        ctx.request_repaint();
                        return;
                    }
                    self.reconcile_workflow_selection(&snapshot);
                    self.workflow_snapshot=Some(Arc::new(snapshot));
                    self.workflow_source_display=None;
                    self.workflow_read.verified();
                    self.workflow_error=None;
                    self.workflow_read_error=None;
                    if let Some((id,step,notice))=self.workflow_stage_after_freight.take(){
                        if self.workflow_snapshot.as_ref().is_some_and(|board|
                            board.groups.iter().any(|group|
                                group.id==id && group.step==step &&
                                group.needs_notice==notice && !group.transferred)){
                            self.advance_workflow_group(&id,ctx);
                        }else{
                            floating_notice::publish(ctx,"reception.workflow",
                                floating_notice::Tone::Validation,
                                "운임 저장 후 업무 단계가 변경됐습니다. 현재 단계를 다시 확인하세요.");
                        }
                    }else if let Some(id)=self.workflow_freight_guidance.take(){
                        if self.page==Page::Reception && self.workflow_freight_editor.is_none(){
                            self.focus_first_missing_workflow_freight(&id,ctx);
                        }
                    }
                    ctx.request_repaint();
                }
                Err(error)=>{
                    self.workflow_freight_guidance=None;
                    self.workflow_error=Some(error);
                    self.workflow_read_error=self.workflow_error.clone();
                    self.workflow_stage_after_freight=None;
                    ctx.request_repaint();
                }
            }
        }
        if self.page!=Page::Reception{
            self.workflow_last_fetch=None;
            return;
        }
        if self.workflow_read.pending() ||
            self.workflow_action.is_some() ||
            ((self.workflow_freight_editor.is_some() || self.confirmed_cell.is_some()
                || self.confirmed_modal_target.is_some()) && self.workflow_error.is_none()) ||
            !std::env::var_os("BANDO_EGUI_BUSINESS_DB")
                .is_some_and(|p|!p.is_empty()) {return;}
        let current=self.workflow_snapshot.as_ref().and_then(|board|board.revision);
        let observed=self.reception_metadata_revision;
        let changed=match (current,observed){
            (Some(current),Some(observed))=>observed!=current,
            (None,_)=>true,
            // An adapter without native change detection retains the old
            // conservative board fallback if explicitly connected.
            (Some(_),None)=>self.reception_metadata_ready,
        };
        if !changed && !self.workflow_read.required() && self.workflow_error.is_none(){return;}
        if !self.workflow_read.required() && self.workflow_last_fetch.is_some_and(|at|
            at.elapsed()<Duration::from_secs(5)){return;}
        // A failed mandatory refresh is also bounded, not retried per frame.
        if self.workflow_error.is_some() && self.workflow_last_fetch.is_some_and(|at|
            at.elapsed()<Duration::from_secs(5)){return;}
        self.workflow_last_fetch=Some(Instant::now());
        let gateway=Arc::clone(&self.gateway);
        self.workflow_read.submit(ctx.clone(),move||workflow_board::load(gateway.as_ref()).map(Some));
    }

    /// Click commits to the real working reception; the target ID is kept
    /// until its asynchronous recognition has restored the same recipient.
    fn focus_pending_recipient(
        &mut self,target:&workflow_board_ui::PendingTarget,ctx:&egui::Context
    ){
        self.select_reception(&target.order_id,ctx);
        if self.active_id.as_deref()!=Some(target.order_id.as_str()){return;}
        self.requested_recipient_focus=target.recipient_id.as_ref()
            .map(|id|(target.order_id.clone(),gateway::rules::scope_reception_edit_ids(
                &target.order_id,id,None).0));
        self.apply_requested_recipient_focus(ctx);
    }
    fn apply_requested_recipient_focus(&mut self,ctx:&egui::Context){
        let Some((order_id,recipient_id))=self.requested_recipient_focus.as_ref()
            else{return};
        if self.active_id.as_deref()!=Some(order_id.as_str()){
            self.requested_recipient_focus=None;
            return;
        }
        let Some(index)=self.analysis.as_ref().and_then(|analysis|
            reception_service::recipients(analysis).iter().position(|row|
                reception_service::text(row,"id")==recipient_id
            )
        )else{return};
        self.selected_recipient=index;
        self.requested_recipient_focus=None;
        ctx.request_repaint();
    }
    /// Hovered source/detail evidence temporarily overrides the board's
    /// preview recipient; neither ever changes the committed clicked target.
    /// All four panes resolve the same order-scoped evidence identity.
    fn visible_recipient_index(&self)->usize{
        let Some(analysis)=self.analysis.as_ref() else{return self.selected_recipient};
        let rows=reception_service::recipients(analysis);
        let linked=self.link_hover.scoped_active()
            .map(|(order,key)|(order,key.recipient_id.as_str()));
        let visible=self.active_id.as_deref().and_then(|order|
            workflow_board_ui::visible_pending_recipient(
                order,self.pending_stage_target().or_else(||self.workflow_ui.pending_preview()),linked
            )
        );
        visible.and_then(|id|rows.iter().position(|row|
            reception_service::text(row,"id")==id
        )).unwrap_or(self.selected_recipient)
    }
    /// Read-only projection of another unsaved receipt. Never replace the
    /// employee's active source, analysis, inline editor or autosave owner.
    fn visible_pending_other(&mut self)->Option<(Arc<PendingDisplay>,usize)>{
        // React precedence: a hovered native stage is above pending-card
        // preview, while the committed selection remains below both.
        let pinned=self.pending_stage_target();
        if pinned.is_none() && self.workflow_ui.active_preview().is_some(){
            self.pending_review_panel.clear();return None;
        }
        let Some(preview)=pinned.or_else(||self.workflow_ui.pending_preview()).cloned()else{
            self.pending_review_panel.clear();return None;
        };
        if self.active_id.as_deref()==Some(preview.order_id.as_str()){
            self.pending_review_panel.clear();return None;
        }
        let draft=self.drafts.iter().find(|row|row.id==preview.order_id)?;
        let previous=self.pending_display.as_ref().map(Arc::clone);
        let display=Self::pending_projection(&mut self.pending_display,draft)?;
        if !previous.as_ref().is_some_and(|old|Arc::ptr_eq(old,&display)){
            self.pending_review_panel.clear();
        }
        let recipients=reception_service::recipients(&display.recognition);
        let linked=self.link_hover.scoped_active()
            .map(|(order,key)|(order,key.recipient_id.as_str()));
        let index=workflow_board_ui::visible_pending_recipient(
            &draft.id,Some(&preview),linked
        ).and_then(|id|recipients.iter().position(|row|
                reception_service::text(row,"id")==id)).unwrap_or(0);
        Some((display,index))
    }

    fn pending_projection(cache:&mut Option<Arc<PendingDisplay>>,draft:&WorkingReception)->Option<Arc<PendingDisplay>>{
        let manual_rows=draft.manual_model.as_ref()
            .and_then(|model|model.pointer("/model/rows")).filter(|rows|rows.is_array());
        let recognized=draft.analysis_view.as_ref().and_then(|view|view.get("recognition"))
            .or(draft.analysis_evidence.as_ref());
        let matches=cache.as_ref().is_some_and(|cached|
            cached.id==draft.id && cached.raw==draft.raw &&
            cached.review_basis.matches(draft) &&
            cached.pricing_preview.as_ref()==draft.pricing_preview.as_ref() &&
            {
                let input=cached.identity_input.as_ref().unwrap_or(&cached.recognition);
                if let Some(rows)=manual_rows{
                    input.as_object().is_some_and(|obj|obj.len()==1) &&
                        input.get("recipients")==Some(rows)
                }else if let Some(recognized)=recognized{
                    input==recognized
                }else{
                    input.as_object().is_some_and(|obj|obj.len()==1) &&
                        input.get("recipients").and_then(|v|v.as_array()).is_some_and(Vec::is_empty)
                }
            });
        if !matches{
            let input=manual_rows.map(|rows|serde_json::json!({"recipients":rows}))
                .or_else(||recognized.cloned())
                .unwrap_or_else(||serde_json::json!({"recipients":[]}));
            let mut recognition=input.clone();
            reception_service::scope_display_recognition(&draft.id,&mut recognition).ok()?;
            let identity_input=(input!=recognition).then_some(input);
            let spans=source_highlight::collect(&draft.raw,&recognition);
            *cache=Some(Arc::new(PendingDisplay{
                id:draft.id.clone(),raw:draft.raw.clone(),recognition,
                pricing_preview:draft.pricing_preview.clone(),spans,identity_input,
                review_basis:PendingReviewBasis::new(draft),
                review_analysis:reception_service::ReceptionAnalysis::from_working_source(draft),
            }));
        }
        cache.as_ref().cloned()
    }

    /// Shared pending-card navigation for the card and its management action.
    /// A blocked selection keeps the current view and lets the caller retain
    /// its decision dialog. Never confirm a dealer or create a group here.
    fn select_pending_workflow_card(&mut self,id:&str,ctx:&egui::Context)->bool{
        self.workflow_freight_guidance=None;
        if self.workflow_freight_editor.as_ref().is_some_and(|editor|
            editor.text.trim()!=editor.original.trim()
        ){
            self.submit_workflow_freight_edit(ctx);
            return false;
        }
        if self.active_id.as_deref()!=Some(id){
            if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
                ReceptionJob::NavigateReception,false
            ){
                self.reception_job_conflict(ReceptionJob::NavigateReception,blocker);
                return false;
            }
        }
        if !self.drafts.iter().any(|draft|draft.id==id){return false;}
        self.workflow_freight_editor=None;
        self.workflow_freight_next_selection=None;
        self.pending_stage_display=None;
        self.workflow_ui.selection=None;
        self.workflow_ui.folded=None;
        self.workflow_ui.stop_preview();
        self.link_hover.clear();
        self.source_hover=None;
        self.select_reception(id,ctx);
        self.active_id.as_deref()==Some(id)
    }

    fn select_reception(&mut self, id: &str, ctx: &egui::Context) {
        if self.active_id.as_deref()==Some(id){return;}
        if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
            ReceptionJob::NavigateReception,false
        ){
            self.reception_job_conflict(ReceptionJob::NavigateReception,blocker);
            return;
        }
        let Some(item) = self.drafts.iter().find(|item| item.id == id).cloned() else {
            self.source_save_error = Some("선택한 발주서를 찾을 수 없습니다.".to_owned());
            return;
        };
        if self.active_id.as_deref() == Some(id) {
            return;
        }
        // Navigation is allowed to abandon a read-only manual dealer quote;
        // dropping its receiver ensures it cannot publish into the next order.
        self.client_change=None;
        self.inline_editor=None;
        self.item_editor=None;
        self.product_picker=None;
        self.active_id = Some(item.id);
        self.requested_recipient_focus=None;
        self.raw_source = item.raw;
        self.order_type = OrderType::parse(&item.order_type).unwrap_or_default();
        // Blank receipts show clipboard/type choices; direct input is explicit.
        self.source_editing = !self.raw_source.is_empty();
        self.direct_source_entry = false;
        self.clear_source_analysis();
        if self.restore_cached_analysis(){
            self.start_pricing_basis_check(ctx);
        }else if !self.raw_source.trim().is_empty() {
            self.analysis_due_at = Some(Instant::now() + Duration::from_millis(250));
            ctx.request_repaint_after(Duration::from_millis(260));
        }
        self.queue_source_save();
    }

    /// Bind only an OS-verified KakaoTalk chat to the ACTIVE unconfirmed
    /// reception. A changed/closed handle is not reused as evidence.
    fn bind_chat_to_active(&mut self, chat: ChatWindow, ctx: &egui::Context) {
        if self.active_delete_locked(){return;}
        // A different chat can imply a different dealer. Preserve an
        // employee-approved manual model rather than silently discarding it.
        if self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|row|row.id==id)
        ).is_some_and(|row|
            row.manual_model.is_some() &&
            (row.chat_handle.as_deref()!=Some(chat.handle.as_str()) ||
             row.chat_title.as_deref()!=Some(chat.title.as_str()))
        ) {
            self.source_save_error=Some(
                "수동 분류한 발주서는 대화창을 변경하기 전에 분류 내역을 확인해야 합니다.".into()
            );
            return;
        }
        if self.kakao_tracker.verified(&chat.handle)
            .is_none_or(|current| current.title != chat.title) {
            return;
        }
        // Associating the chat with a draft must never forge a successful
        // OS dock connection. The actual dock owns connection state.
        let mut changed = false;
        if let Some(id) = self.active_id.as_deref() {
            if let Some(item) = self.drafts.iter_mut().find(|item| item.id == id) {
                if item.chat_handle.as_deref() != Some(chat.handle.as_str()) ||
                   item.chat_title.as_deref() != Some(chat.title.as_str()) {
                    item.chat_handle = Some(chat.handle);
                    item.chat_title = Some(chat.title);
                    item.analysis_raw = None;
                    item.analysis_evidence = None;
                    item.analysis_view = None;
                    item.client_match = None;
                    item.pricing_preview = None;
                    item.manual_model = None;
                    item.working_synced = false;
                    if item.client_selection.as_deref() != Some("MANUAL") {
                        item.client_id = None;
                        item.client_selection = None;
                    }
                    changed = true;
                }
            }
        }
        if changed {
            self.clear_source_analysis();
            if !self.raw_source.trim().is_empty() {
                self.analysis_due_at = Some(Instant::now() + Duration::from_millis(250));
                ctx.request_repaint_after(Duration::from_millis(260));
            }
            self.queue_source_save();
        }
    }

    /// The employee may select ONLY candidates returned by the Rust
    /// matcher, and the choice applies to this isolated preview order only.
    fn select_client_candidate(&mut self, client_id: &str, ctx: &egui::Context) {
        let allowed = self.analysis.as_ref().is_some_and(|analysis|
            analysis.client_candidates.iter().any(|candidate|
                reception_service::text(candidate, "clientId") == client_id
            ));
        if !allowed {
            self.source_save_error = Some("검색 결과에 없는 거래처는 선택할 수 없습니다.".into());
            return;
        }
        self.apply_manual_client(client_id, ctx);
    }

    // All-client choices use one guarded native Rust order-edit path.
    // It computes a full repricing preview, but production state_save and
    // workflow reconciliation are not exposed in this isolated egui app.
    fn apply_manual_client(&mut self, client_id: &str, ctx: &egui::Context) {
        self.apply_client_with_selection(client_id,ctx,None);
    }

    fn apply_client_with_selection(
        &mut self,client_id:&str,ctx:&egui::Context,
        reprice_selection:Option<String>
    ){
        let duplicate=self.client_change.as_ref().is_some_and(|pending|
            pending.order_id==self.active_id.as_deref().unwrap_or("") &&
            pending.client_id==client_id
        );
        match self.reception_job_admission(ReceptionJob::ManualClient,duplicate){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::ManualClient,blocker);
                return;
            }
        }
        let Some(id) = self.active_id.as_deref() else { return; };
        let Some(row) = self.drafts.iter().find(|row| row.id == id).cloned() else { return; };
        if reprice_selection.is_none() &&
           row.client_id.as_deref() == Some(client_id) &&
           row.client_selection.as_deref()==Some("MANUAL") &&
           row.pricing_preview.as_ref().is_some_and(complete_reception_quote) {
            return; // Same dealer with an already complete native quote.
        }
        let direct_input=row.raw.trim().is_empty() &&
            row.manual_model.as_ref().is_some_and(|saved|
                saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
            );
        if !direct_input && (row.analysis_raw.as_deref()!=Some(row.raw.as_str()) ||
            row.analysis_evidence.is_none()) {
            self.source_save_error=Some(
                "현재 발주원문 인식이 끝난 뒤 거래처를 다시 선택하세요.".into()
            );
            return;
        }
        // Employee selection overrides any still-running automatic match.
        // Dropping its receiver prevents a late auto quote from taking over.
        self.auto_client_change=None;
        self.pricing_basis_check=None;
        self.remote_recheck=None;
        self.remote_recheck_due=false;
        let target=client_id.to_owned();
        let order_id=row.id.clone();
        let source_raw=row.raw.clone();
        let order_type=row.order_type.clone();
        let gateway=Arc::clone(&self.gateway);
        let (sender,receiver)=mpsc::channel();
        let context=ctx.clone();
        self.client_change=Some(PendingClientChange {
            order_id,client_id:target.clone(),source_raw,order_type,
            chat_handle:row.chat_handle.clone(),
            chat_title:row.chat_title.clone(),
            manual_model:row.manual_model.clone(),
            working_version:row.working_version,
            price_snapshot:row.pricing_preview.clone(),
            reprice_selection,receiver,
        });
        self.source_save_error=None;
        std::thread::spawn(move || {
            let result=native_order_change::prepare(gateway.as_ref(),&row,&target);
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    fn poll_client_change(&mut self, ctx:&egui::Context) {
        let value=self.client_change.as_ref().map(|p|p.receiver.try_recv());
        let result=match value {
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("주문 변경에 따른 금액 계산이 중단되었습니다.".into())),
            _=>None,
        };
        if let Some(result)=result {
            let Some(pending)=self.client_change.take() else {return;};
            // Another order or edited original may never inherit an old
            // calculation, regardless of asynchronous completion order.
            let current=self.drafts.iter().find(|row|row.id==pending.order_id);
            let stale_reason=if self.active_id.as_deref()!=Some(pending.order_id.as_str()){
                Some("다른 발주서로 이동했습니다. 거래처를 다시 선택하세요.")
            }else if self.raw_source!=pending.source_raw ||
                     self.order_type.as_str()!=pending.order_type ||
                     current.is_none_or(|row|
                         row.raw!=pending.source_raw ||
                         row.order_type!=pending.order_type
                     ){
                Some("발주서 원문 또는 접수 유형이 변경되었습니다. 거래처를 다시 선택하세요.")
            }else if current.is_none_or(|row|
                row.chat_handle!=pending.chat_handle ||
                row.chat_title!=pending.chat_title
            ){
                Some("카카오톡 대화창이 변경되었습니다. 거래처를 다시 선택하세요.")
            }else if current.is_none_or(|row|
                row.manual_model!=pending.manual_model ||
                row.working_version!=pending.working_version ||
                row.pricing_preview!=pending.price_snapshot
            ){
                Some("발주서 수정·운영 저장·견적 상태가 변경되어 이전 계산을 적용하지 않았습니다.")
            }else{None};
            if stale_reason.is_some() {
                // The chooser was dismissed at the original click. An old
                // calculation must never resurrect it or overwrite a newer
                // receipt, source or employee edit.
                return;
            }
            match result {
                Ok(result)=>{
                    // The same native worker already recognized the source
                    // against this dealer before calculating prices. Reuse
                    // that exact projection instead of launching a second
                    // matcher/OCR/quote worker after the successful result.
                    let new_client=result.client_id.clone();
                    let chosen_name=self.client_names.get(&new_client).cloned()
                        .or_else(||self.client_summaries.get(&new_client)
                            .map(|client|client.name.clone()));
                    let direct_input=pending.source_raw.trim().is_empty() &&
                        pending.manual_model.as_ref().is_some_and(|saved|
                            saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
                        );
                    let display_cache=if direct_input{
                        if let Some(analysis)=self.analysis.as_mut(){
                            analysis.chosen_client_name=chosen_name.clone();
                        }
                        None
                    }else if let Some(analysis)=self.analysis.as_mut(){
                        if let Some(recognition)=result.recognition.as_ref(){
                            if let Some(rows)=recognition.get("recipients")
                                .filter(|rows|rows.is_array()){
                                analysis.apply_edited_recipients(
                                    &self.raw_source,rows
                                );
                                analysis.recognition=recognition.clone();
                                analysis.linked_spans=source_highlight::collect(
                                    &self.raw_source,&analysis.recognition
                                );
                            }
                        }
                        analysis.chosen_client_name=chosen_name;
                        Some(analysis.cached_view())
                    }else{None};
                    if let Some(row)=self.drafts.iter_mut().find(|r|
                        r.id==pending.order_id && r.raw==pending.source_raw
                    ) {
                        row.client_id=Some(new_client);
                        row.client_selection=Some(
                            pending.reprice_selection.clone()
                                .unwrap_or_else(||"MANUAL".into())
                        );
                        row.pricing_preview=result.pricing_preview;
                        row.manual_model=result.manual_model;
                        row.working_synced=false;
                        // Preserve the initial analysisEvidence and advisory
                        // clientMatch independently for audit. The newer
                        // dealer-specific view lives in analysisView.
                        row.analysis_view=if direct_input{None}else{display_cache.clone()};
                    }
                    if direct_input{
                        self.restore_direct_manual_view();
                        self.analysis_pending=false;
                        self.analysis_due_at=None;
                        self.analysis_error=None;
                        self.selected_recipient=0;
                        self.source_hover=None;
                        self.link_hover.clear();
                    }else if display_cache.is_some(){
                        self.analysis_pending=false;
                        self.analysis_due_at=None;
                        self.analysis_error=None;
                        self.selected_recipient=0;
                        self.source_hover=None;
                        self.link_hover.clear();
                    }else{
                        // Safe compatibility fallback for incomplete legacy
                        // drafts: the second analysis is strictly exceptional.
                        self.clear_source_analysis();
                        if !self.raw_source.trim().is_empty(){
                            self.analysis_due_at=Some(Instant::now()+Duration::from_millis(250));
                            ctx.request_repaint_after(Duration::from_millis(260));
                        }
                    }
                    self.queue_source_save();
                    self.start_remote_recheck(ctx);
                }
                Err(error)=>{
                    // A failure is a reception-level, non-modal error. Keep
                    // the prior confirmed client/quote and never reopen the
                    // dismissed chooser automatically.
                    let message=format!("거래처 변경 확인 실패: {error}");
                    self.source_save_error=Some(message.clone());
                    floating_notice::publish(
                        ctx,"reception.client_change",
                        floating_notice::Tone::Error,message
                    );
                }
            }
        }
    }

    /// A deliberate employee click is the ONLY way to send an unconfirmed
    /// reception into the Tauri working-order store. Editing or selecting
    /// customers continues to save exclusively to egui-preview.sqlite3.
    #[cfg(test)]
    fn start_working_sync(&mut self, ctx:&egui::Context) {
        match self.reception_job_admission(
            ReceptionJob::WorkingSync,self.working_sync.is_some()
        ){
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.working_sync_error=Some(format!(
                    "{} 중에는 운영 작업본을 저장할 수 없습니다.",blocker.label()
                ));
                return;
            }
            JobAdmission::Start=>{}
        }
        self.working_sync_error=None;
        if !self.gateway.working_commit_enabled() {
            self.working_sync_error=Some(
                "운영 작업본 저장 도우미와 명시적 허용 설정이 필요합니다.".into()
            );
            return;
        }
        if self.working_sync.is_some() || self.client_change.is_some() ||
            self.inline_change.is_some() || self.inline_editor.as_ref()
                .is_some_and(|edit|edit.text!=edit.original) {
            self.working_sync_error=Some("진행 중인 셀 수정을 확정한 후 저장하세요.".into());
            return;
        }
        let Some(source)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row| row.id==id)).cloned()
        else{return};
        let Some(client_id)=source.client_id.clone() else{
            self.working_sync_error=Some("거래처를 먼저 선택하세요.".into());
            return;
        };
        let manual=source.manual_model.clone();
        let authority_revision=if manual.is_none(){
            let Some(quote)=source.pricing_preview.as_ref() else{
                self.working_sync_error=Some(
                    "거래처 변경 창에서 금액을 다시 계산한 뒤 저장하세요.".into()
                );
                return;
            };
            if quote.get("sourceRaw").and_then(serde_json::Value::as_str)
                !=Some(source.raw.as_str()) ||
               quote.get("orderType").and_then(serde_json::Value::as_str)
                !=Some(source.order_type.as_str()) ||
               quote.get("clientId").and_then(serde_json::Value::as_str)
                !=Some(client_id.as_str()) {
                self.working_sync_error=Some("발주서 또는 거래처의 계산 근거가 달라졌습니다.".into());
                return;
            }
            let Some(revision)=quote.get("authorityRevision")
                .and_then(serde_json::Value::as_u64) else{
                self.working_sync_error=Some("최신 업무 정보를 확인할 수 없습니다. 금액을 다시 계산해 주세요.".into());
                return;
            };
            Some(revision)
        }else{None};
        let mut request=serde_json::json!({
            "orderId":&source.id,
            "sourceRaw":&source.raw,
            "orderType":&source.order_type,
            "clientId":&client_id,
            "expectedAuthorityRevision":authority_revision,
            "expectedWorkingVersion":source.working_version,
        });
        if let Some(model)=manual.as_ref(){
            request["manualModel"]=model.clone();
        }
        let (sender,receiver)=mpsc::channel();
        let gateway=Arc::clone(&self.gateway);
        self.working_sync=Some(PendingWorkingSync {
            order_id:source.id,
            source_raw:source.raw,
            order_type:source.order_type,
            client_id,
            authority_revision,
            submitted_manual:manual,
            receiver,
        });
        let context=ctx.clone();
        std::thread::spawn(move || {
            let result=(||->Result<serde_json::Value,String>{
                if request.get("manualModel").is_some(){
                    // A draft without a quote receives a fresh read-only
                    // master revision inside this worker. The native helper
                    // compares it again within its IMMEDIATE transaction.
                    let state=gateway.load_business_state()?;
                    let revision=state.get("revision").and_then(
                        serde_json::Value::as_u64
                    ).ok_or("최신 업무 정보를 확인하지 못했습니다.")?;
                    request["expectedAuthorityRevision"]=serde_json::json!(revision);
                }
                gateway.commit_working_reception(request)
            })();
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    fn poll_working_sync(&mut self) {
        let value=self.working_sync.as_ref().map(|sync|sync.receiver.try_recv());
        let outcome=match value {
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("Tauri 저장 도우미와 연결이 끊어졌습니다.".to_owned())),
            _=>None,
        };
        if let Some(outcome)=outcome {
            let Some(pending)=self.working_sync.take() else{return};
            match outcome {
                Ok(result)=>{
                    let version=result.get("workingVersion")
                        .and_then(serde_json::Value::as_u64);
                    let confirmed_revision=result.get("authorityRevision")
                        .and_then(serde_json::Value::as_u64);
                    let valid=version.is_some_and(|v|v>0) &&
                        result.get("orderId").and_then(serde_json::Value::as_str)
                            ==Some(pending.order_id.as_str()) &&
                        result.get("clientId").and_then(serde_json::Value::as_str)
                            ==Some(pending.client_id.as_str()) &&
                        confirmed_revision.is_some() &&
                        pending.authority_revision.is_none_or(|rev|
                            confirmed_revision==Some(rev)) &&
                        result.get("manualApplied").and_then(serde_json::Value::as_bool)
                            ==Some(pending.submitted_manual.is_some()) &&
                        result.get("linePrices").and_then(serde_json::Value::as_array).is_some();
                    if !valid {
                        self.working_sync_error=Some(
                            "운영 주문이 저장됐을 수 있지만 확인 응답이 일치하지 않습니다. 다시 저장하지 말고 작업본을 확인하세요.".into()
                        );
                        return;
                    }
                    let Some(row)=self.drafts.iter_mut().find(|row|
                        row.id==pending.order_id
                    ) else {
                        self.working_sync_error=Some(
                            "운영 작업본은 저장됐으나 현재 발주서가 목록에 없습니다. 다시 불러오세요.".into()
                        );
                        return;
                    };
                    row.working_version=version;
                    let same_input=row.raw==pending.source_raw &&
                        row.order_type==pending.order_type &&
                        row.client_id.as_deref()==Some(pending.client_id.as_str()) &&
                        row.manual_model==pending.submitted_manual;
                    if same_input {
                        // The Tauri helper's output is the only authoritative
                        // price/freight quote for the employee's manual draft.
                        // The UI never reproduces native calculation rules.
                        row.pricing_preview=Some(serde_json::json!({
                            "sourceRaw":pending.source_raw,
                            "orderId":pending.order_id,
                            "orderType":pending.order_type,
                            "clientId":pending.client_id,
                            "authorityRevision":confirmed_revision,
                            "status":"REVIEW",
                            "shippingFee":result.get("shippingFee"),
                            "shippingVersion":result.get("shippingVersion"),
                            "vatAmount":result.get("vatAmount"),
                            "subtotal":result.get("subtotal"),
                            "total":result.get("total"),
                            "notified":false,
                            "paid":false,
                            "linePrices":result.get("linePrices"),
                            "freightLines":result.get("freightLines"),
                            "pricingBasis":result.get("pricingBasis")
                                .or_else(||row.pricing_preview.as_ref()
                                    .and_then(|quote|quote.get("pricingBasis"))),
                            "pendingRecipients":result.get("pendingRecipients")
                                .filter(|rows|rows.is_array()).cloned()
                                .or_else(||row.pricing_preview.as_ref()
                                    .and_then(|quote|quote.get("pendingRecipients"))
                                    .filter(|rows|rows.is_array()).cloned())
                                .or_else(||result.get("total")
                                    .filter(|total|total.is_number())
                                    .map(|_|serde_json::json!([]))),
                        }));
                    }
                    row.working_synced=same_input && row.pricing_preview.is_some();
                    if !same_input {
                        self.working_sync_error=Some(
                            "운영 작업본은 저장됐지만 이후 수정한 내용은 반영되지 않았습니다.".into()
                        );
                    }
                    self.queue_source_save();
                }
                Err(error)=>{
                    self.working_sync_error=Some(format!("운영 작업본 저장 실패: {error}"));
                }
            }
        }
    }

    /// React OrderItemEditor: product name opens a dedicated modal, never a
    /// TextEdit suggestion popup. Selection is staged until explicit Apply.
    fn open_item_editor(
        &mut self,recipient_id:&str,item_id:&str,ctx:&egui::Context
    ){
        if self.item_editor.is_some(){return;}
        if self.inline_editor.as_ref().is_some_and(|edit|
            edit.text!=edit.original){
            self.source_save_error=Some(
                "현재 셀 편집을 Enter로 확정하거나 Esc로 취소하세요.".into()
            );
            return;
        }
        match self.reception_job_admission(ReceptionJob::InlineEdit,false){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::InlineEdit,blocker);
                return;
            }
        }
        let Some(row)=self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|row|row.id==id)
        )else{return};
        let direct_input=row.raw.trim().is_empty() &&
            row.manual_model.as_ref().is_some_and(|saved|
                saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
            );
        if !direct_input && (row.analysis_raw.as_deref()!=Some(self.raw_source.as_str()) ||
            row.analysis_evidence.is_none()){
            self.source_save_error=Some(
                "발주원문 인식이 완료된 뒤 품목을 수정하세요.".into()
            );
            return;
        }
        let Some(item)=self.analysis.as_ref().and_then(|analysis|
            reception_service::recipients(analysis).iter().find(|recipient|
                reception_service::text(recipient,"id")==recipient_id)
        ).and_then(|recipient|recipient["items"].as_array())
            .and_then(|items|items.iter().find(|item|
                reception_service::text(item,"id")==item_id
            )).cloned()
        else{
            self.source_save_error=Some("선택한 품목이 현재 발주서에 없습니다.".into());
            return;
        };
        let price=row.pricing_preview.as_ref()
            .filter(|quote|quote.get("sourceRaw")
                .and_then(serde_json::Value::as_str)==Some(row.raw.as_str()))
            .and_then(|quote|quoted_item_line(quote,item_id))
            .and_then(|line|line["unitPrice"].as_i64())
            .or_else(||item.get("unitPrice").and_then(serde_json::Value::as_i64));
        self.inline_editor=None;
        let editor=item_editor::ItemEditor::new(
            row.id.clone(),row.raw.clone(),recipient_id.into(),
            item,row.client_id.clone().unwrap_or_default(),price
        );
        self.item_editor=Some(if direct_input{editor.for_manual_draft()}else{editor});
        ctx.request_repaint();
    }

    fn open_item_remove_confirmation(&mut self,recipient_id:&str,item_id:&str,ctx:&egui::Context){
        if self.item_editor.is_some(){return;}
        self.open_item_editor(recipient_id,item_id,ctx);
        if let Some(dialog)=self.item_editor.as_mut(){
            dialog.open_delete_only();
        }
    }

    fn open_product_picker(&mut self,recipient_id:String,ctx:&egui::Context){
        if self.product_picker.is_some() || self.item_editor.is_some()
            || self.inline_editor.is_some()
            || self.reception_job_admission(ReceptionJob::InlineEdit,false)!=JobAdmission::Start{
            return;
        }
        let Some(row)=self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|row|row.id==id)
        )else{return};
        let direct_input=row.raw.trim().is_empty() &&
            row.manual_model.as_ref().is_some_and(|saved|
                saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
            );
        if !direct_input && (row.analysis_raw.as_deref()!=Some(self.raw_source.as_str())
            || row.analysis_evidence.is_none()){return;}
        self.product_picker=Some((row.id.clone(),row.raw.clone(),recipient_id,
            product_manager::ProductManager::new_selection(
                Arc::clone(&self.gateway),ctx.clone(),row.client_id.clone().unwrap_or_default()
            )));
        ctx.request_repaint();
    }

    /// Both the existing product chooser and inline cell masters submit an
    /// identity-bound command. Never use the active temporary draft here.
    fn confirmed_edit_target(&mut self,board:&workflow_board::BoardSnapshot,
        group:&workflow_board::GroupRow,ship:&workflow_board::ShipmentRow)
        ->Option<ConfirmedEditTarget>{
        let revision=board.revision?;
        let selection=workflow_board_ui::Selection{
            group_id:group.id.clone(),order_id:ship.order_id.clone(),shipment_id:ship.id.clone()
        };
        self.workflow_ui.select(selection.clone());
        self.link_hover.clear();
        self.source_hover=None;
        self.discard_advisory_workflow_read();
        Some(ConfirmedEditTarget{selection,revision,recipient_id:ship.recipient_id.clone()})
    }

    fn submit_confirmed_change(&mut self, target:&ConfirmedEditTarget,
        field:&str,item_id:Option<&str>,value:serde_json::Value,ctx:&egui::Context)->bool{
        if self.workflow_snapshot.as_ref().and_then(|b|b.revision)!=Some(target.revision)
            || self.workflow_ui.selection.as_ref()!=Some(&target.selection){
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Validation,
                "주문 상태가 변경됐습니다. 최신 정보를 확인한 뒤 다시 수정하세요.");
            return false;
        }
        self.start_workflow_action_with_args("edit_confirmed_recipient",
            &target.selection.group_id,Some(&target.selection.shipment_id),None,false,
            serde_json::json!({"change":{"recipientId":target.recipient_id,
                "field":field,"itemId":item_id,"value":value}}),ctx);
        self.workflow_action.as_ref().is_some_and(|a|a.action=="edit_confirmed_recipient"
            && a.group_id==target.selection.group_id)
    }

    fn submit_confirmed_cell(&mut self,ctx:&egui::Context){
        let Some(mut editor)=self.confirmed_cell.take()else{return;};
        if editor.cell.text==editor.cell.original{return;}
        let value=match editor.cell.target.field{
            "quantity"=>{
                let parsed=editor.cell.text.trim().parse::<i64>().ok()
                    .filter(|n|(1..=9_007_199_254_740_991).contains(n));
                let Some(n)=parsed else{
                    floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Validation,
                        "수량은 1 이상의 정수로 입력하세요.");
                    editor.cell.focus_next=true;self.confirmed_cell=Some(editor);return;
                };
                serde_json::json!(n.to_string())
            },
            "unitPrice"=>{
                let digits=editor.cell.text.trim().replace(',',"");
                if digits.is_empty(){serde_json::Value::Null}else{
                    let Some(n)=digits.parse::<i64>().ok()
                        .filter(|n|(0..=9_007_199_254_740_991).contains(n))else{
                        floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Validation,
                            "단가는 0 이상의 정수로 입력하세요.");
                        editor.cell.focus_next=true;self.confirmed_cell=Some(editor);return;
                    };
                    serde_json::json!(n)
                }
            },
            _=>serde_json::json!(editor.cell.text)
        };
        if self.submit_confirmed_change(&editor.target,editor.cell.target.field,
            editor.cell.target.item_id.as_deref(),value,ctx){
            self.submitted_confirmed_cell=Some(editor);
        }else{
            editor.cell.focus_next=true;
            self.confirmed_cell=Some(editor);
        }
    }

    /// Preserve the clicked destination before closing the active editor.
    /// Clean switches reuse the current projection; dirty switches wait for
    /// the existing native save and authoritative refreshed projection.
    fn switch_confirmed_editor(&mut self,selection:workflow_board_ui::Selection,
        target:InlineTarget,click_at:Option<egui::Pos2>,guide:bool,ctx:&egui::Context){
        let same=self.confirmed_cell.as_ref().is_some_and(|e|
            e.target.selection==selection && e.cell.target==target)
            || self.workflow_freight_editor.as_ref().is_some_and(|e|
                e.shipment_id==selection.shipment_id && target.field=="shippingCharge"
                && target.item_id.as_deref()==Some(e.origin.as_str()));
        let dirty=self.confirmed_cell.as_ref().is_some_and(|e|e.cell.text!=e.cell.original)
            || self.workflow_freight_editor.as_ref().is_some_and(|e|
                e.text.trim().replace(',',"")!=e.original.trim().replace(',',""));
        use cell_navigation::CellSwitch;
        match cell_navigation::cell_switch(same,dirty){
            CellSwitch::KeepCurrent=>return,
            CellSwitch::OpenNow=>{
                self.confirmed_cell=None;
                self.workflow_freight_editor=None;
                self.workflow_freight_guidance=None;
                self.workflow_ui.select(selection.clone());
                self.confirmed_next_cell=Some((selection,target,click_at,guide));
                self.resume_confirmed_navigation(ctx);
            },
            CellSwitch::CommitThenOpen=>{
                self.confirmed_next_cell=Some((selection,target,click_at,guide));
                if self.confirmed_cell.is_some(){
                    self.submit_confirmed_cell(ctx);
                    if self.confirmed_cell.is_some(){self.confirmed_next_cell=None;}
                }else if !self.submit_workflow_freight_edit(ctx){
                    self.confirmed_next_cell=None;
                }
            }
        }
        ctx.request_repaint();
    }

    // Open the next cell only after the authoritative post-save projection.
    fn resume_confirmed_navigation(&mut self,ctx:&egui::Context){
        if self.confirmed_next_cell.is_none(){return;}
        if self.page!=Page::Reception || self.workflow_error.is_some(){
            self.confirmed_next_cell=None;return;
        }
        if self.workflow_action.is_some() || self.workflow_read.required(){return;}
        let Some((selection,target,click_at,guide))=self.confirmed_next_cell.take()else{return;};
        if !self.workflow_mutation_ready() || !self.gateway.workflow_write_enabled()
            || self.workflow_ui.selection.as_ref()!=Some(&selection)
            || self.confirmed_cell.is_some() || self.workflow_freight_editor.is_some()
            || self.reception_modal_open(){return;}
        let Some(board)=self.workflow_snapshot.clone()else{return;};
        let Some(group)=board.groups.iter().find(|g|g.id==selection.group_id)else{return;};
        let Some(ship)=group.shipments.iter().find(|s|
            s.id==selection.shipment_id && s.order_id==selection.order_id)else{return;};
        let editable=!board.groups.iter().flat_map(|g|g.shipments.iter())
            .any(|s|s.order_id==ship.order_id && s.transferred);
        let freight_editable=!ship.transferred && !ship.freight_editing_blocked && !ship.collect;
        if !confirmed_navigation_entries(ship,editable,freight_editable).iter()
            .any(|entry|entry.cell.enabled && entry.target==target){return;}
        let original=if target.field=="shippingCharge"{
            let Some(origin)=target.item_id.as_deref()else{return;};
            ship.display_charges.get(origin).copied().flatten().map(|n|n.to_string()).unwrap_or_default()
        }else if let Some(item_id)=target.item_id.as_deref(){
            let Some(item)=ship.recipient["items"].as_array().and_then(|items|
                items.iter().find(|item|reception_service::text(item,"id")==item_id))else{return;};
            let value=item[target.field].as_i64();
            if target.field=="unitPrice"{value.map(crate::input::money_edit_text).unwrap_or_default()}
            else{value.map(|n|n.to_string()).unwrap_or_default()}
        }else{
            reception_service::text(&ship.recipient,target.field).to_owned()
        };
        let Some(bound)=self.confirmed_edit_target(&board,group,ship)else{return;};
        if target.field=="shippingCharge"{
            self.workflow_freight_guidance=guide.then(||group.id.clone());
            let Some(origin)=target.item_id else{return;};
            self.workflow_freight_editor=Some(WorkflowFreightEditor{
                group_id:group.id.clone(),shipment_id:ship.id.clone(),origin,
                original:original.clone(),text:original,focus_next:true,click_at
            });
        }else{
            self.confirmed_cell=Some(ConfirmedCellEditor{target:bound,cell:InlineReceptionEditor{
                source_id:ship.order_id.clone(),target,original:original.clone(),text:original,
                focus_next:true,click_at
            }});
        }
        ctx.request_repaint();
    }

    fn show_product_picker(&mut self,ctx:&egui::Context,work_area:Rect){
        if let Some(target)=self.confirmed_modal_target.clone(){
            if let Some((_,_,_,manager))=self.product_picker.as_mut(){
                let (cancelled,chosen)=manager.show_selection_modal(ctx,work_area,"reception-add-product-picker");
                if cancelled{self.product_picker=None;self.confirmed_modal_target=None;}
                else if let Some(product)=chosen{
                    self.product_picker=None;
                    self.confirmed_modal_target=None;
                    self.submit_confirmed_change(&target,"addItem",None,product,ctx);
                }
                return;
            }
        }
        let Some((order_id,raw,recipient_id,manager))=self.product_picker.as_mut()else{return};
        if self.active_id.as_deref()!=Some(order_id.as_str()) || self.raw_source!=*raw{
            self.product_picker=None;
            return;
        }
        let (cancelled,chosen)=manager.show_selection_modal(ctx,work_area,"reception-add-product-picker");
        let recipient_id=recipient_id.clone();
        if cancelled{
            self.product_picker=None;
        }else if let Some(product)=chosen{
            // Close immediately; the existing native worker commits the
            // selected item and recalculates the order as one operation.
            self.product_picker=None;
            self.start_review_inline_action(
                InlineTarget{recipient_id,item_id:None,field:"addItem"},product,ctx
            );
        }
    }

    fn show_item_editor(&mut self,ctx:&egui::Context,work_area:Rect){
        if let Some(target)=self.confirmed_modal_target.clone(){
            if let Some(dialog)=self.item_editor.as_mut(){
                let item_id=dialog.item_id.clone();
                let event=dialog.show(ctx,Arc::clone(&self.gateway),work_area,false);
                if let Some(event)=event{
                    self.item_editor=None;
                    self.confirmed_modal_target=None;
                    match event{
                        item_editor::Event::Closed=>{},
                        item_editor::Event::Apply(product)=>{
                            self.submit_confirmed_change(&target,"product",Some(&item_id),
                                serde_json::json!({"productId":product["id"],"variantId":product["variants"][0]["id"]}),ctx);
                        },
                        item_editor::Event::Remove=>{
                            self.submit_confirmed_change(&target,"removeItem",Some(&item_id),
                                serde_json::Value::Null,ctx);
                        }
                    }
                }
                return;
            }
            if self.product_picker.is_some() || self.client_chooser.is_some() || self.vat_dialog.is_some(){return;}
            self.confirmed_modal_target=None;
        }
        let valid=self.item_editor.as_ref().is_some_and(|dialog|
            self.active_id.as_deref()==Some(dialog.order_id.as_str())
            && self.raw_source==dialog.source_raw
        );
        if !valid{
            self.item_editor=None;
            return;
        }
        let busy=self.inline_change.is_some();
        let event=self.item_editor.as_mut().and_then(|dialog|
            dialog.show(ctx,Arc::clone(&self.gateway),work_area,busy)
        );
        match event{
            Some(item_editor::Event::Closed)=>{
                self.item_editor=None;
            },
            Some(item_editor::Event::Apply(product))=>{
                if let Some(dialog)=self.item_editor.as_ref(){
                    let target=InlineTarget{
                        recipient_id:dialog.recipient_id.clone(),
                        item_id:Some(dialog.item_id.clone()),field:"product"
                    };
                    self.start_review_inline_action(
                        target,serde_json::json!({"product":product}),ctx
                    );
                    if self.inline_change.is_none(){
                        if let Some(error)=self.source_save_error.clone(){
                            if let Some(dialog)=self.item_editor.as_mut(){
                                dialog.report_error(error);
                            }
                        }
                    }else if let Some(dialog)=self.item_editor.as_mut(){
                        dialog.clear_error();
                    }
                }
            },
            Some(item_editor::Event::Remove)=>{
                if let Some(dialog)=self.item_editor.as_ref(){
                    let target=InlineTarget{
                        recipient_id:dialog.recipient_id.clone(),
                        item_id:Some(dialog.item_id.clone()),field:"removeItem"
                    };
                    self.start_review_inline_action(
                        target,serde_json::Value::Null,ctx
                    );
                    if self.inline_change.is_none(){
                        if let Some(error)=self.source_save_error.clone(){
                            if let Some(dialog)=self.item_editor.as_mut(){
                                dialog.report_error(error);
                            }
                        }
                    }else if let Some(dialog)=self.item_editor.as_mut(){
                        dialog.clear_error();
                    }
                }
            },
            None=>{}
        }
    }

    fn open_manual_workspace(&mut self,ctx:&egui::Context) {
        match self.reception_job_admission(
            ReceptionJob::ManualWorkspace,self.manual_workspace.is_some()
        ){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::ManualWorkspace,blocker);
                return;
            }
        }
        if self.client_change.is_some() || self.working_sync.is_some() ||
            self.inline_change.is_some() || self.inline_editor.as_ref()
                .is_some_and(|edit|edit.text!=edit.original) {
            self.source_save_error=Some("셀 입력을 확정한 다음 수동 분류를 여세요.".into());
            return;
        }
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id)).cloned()
            else{return};
        if row.raw.trim().is_empty() ||
           row.analysis_raw.as_deref()!=Some(row.raw.as_str()) ||
           row.analysis_evidence.is_none() {
            self.source_save_error=Some("원문 인식이 완료된 뒤 수동 분류를 시작하세요.".into());
            return;
        }
        // A dealer choice is NOT required for assigning recipient/source
        // evidence. Pricing and business-state reconciliation remain native
        // Rust responsibilities when a dealer is subsequently selected.
        // Do not carry a previous reception-pane hover into the manual
        // worksheet; both views share the same order-scoped state from here.
        self.link_hover.clear();
        self.source_hover=None;
        let mut editor=manual_workspace::ManualWorkspace::new(
            row,Arc::clone(&self.gateway),ctx.clone()
        );
        editor.focus_recipient(self.selected_recipient);
        self.manual_workspace=Some(editor);
    }

    fn open_direct_workspace(&mut self,ctx:&egui::Context){
        match self.reception_job_admission(
            ReceptionJob::ManualWorkspace,self.manual_workspace.is_some()
        ){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::ManualWorkspace,blocker);
                return;
            }
        }
        if self.client_change.is_some() || self.working_sync.is_some() ||
            self.inline_change.is_some() || self.inline_editor.as_ref()
                .is_some_and(|edit|edit.text!=edit.original){
            self.source_save_error=Some("현재 셀 입력을 확정한 다음 직접 입력을 시작하세요.".into());
            return;
        }
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id)).cloned()
            else{return};
        if !row.raw.trim().is_empty(){
            self.source_save_error=Some("발주 원문이 있는 주문은 수동 분류를 사용하세요.".into());
            return;
        }
        self.link_hover.clear();
        self.source_hover=None;
        self.source_editing=false;
        self.direct_source_entry=false;
        let mut editor=manual_workspace::ManualWorkspace::new_direct(
            row,Arc::clone(&self.gateway),ctx.clone()
        );
        editor.focus_recipient(self.selected_recipient);
        self.manual_workspace=Some(editor);
    }

    /// Non-text commands use the same serialized native reducer without
    /// creating a hidden text editor (delivery toggles, issue approvals).
    fn start_review_inline_action(
        &mut self,target:InlineTarget,value:serde_json::Value,ctx:&egui::Context
    ){
        if self.inline_editor.is_some() || self.inline_change.is_some(){
            self.source_save_error=Some(
                "현재 셀 편집을 먼저 확정하거나 취소하세요.".into()
            );
            return;
        }
        match self.reception_job_admission(ReceptionJob::InlineEdit,false){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::InlineEdit,blocker);
                if let Some(error)=self.source_save_error.as_deref(){
                    floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Validation,error);
                }
                return;
            }
        }
        let Some(source)=self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|row|row.id==id)
        ).cloned()else{return};
        let direct_input=source.raw.trim().is_empty() &&
            source.manual_model.as_ref().is_some_and(|saved|
                saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
            );
        if !direct_input && (source.analysis_raw.as_deref()!=Some(source.raw.as_str())
            ||source.analysis_evidence.is_none()){
            self.source_save_error=Some(
                "원문 인식이 완료된 뒤 확인 항목을 처리하세요.".into()
            );
            return;
        }
        self.source_save_error=None;
        let (tx,receiver)=mpsc::channel();
        self.inline_change=Some(PendingInlineChange{
            order_id:source.id.clone(),raw:source.raw.clone(),
            target:target.clone(),editor:None,next_client_review:None,next:None,receiver
        });
        let gateway=Arc::clone(&self.gateway);
        let context=ctx.clone();
        std::thread::spawn(move||{
            let request=manual_workspace::InlineRequest{
                recipient_id:target.recipient_id,
                item_id:target.item_id,
                field:target.field.to_owned(),value
            };
            let outcome=manual_workspace::apply_inline(
                gateway.as_ref(),&source,&request
            );
            let _=tx.send(outcome);
            context.request_repaint();
        });
    }

    /// Only show the validation balloon once the real input widget is
    /// rendered and focused. No pointer coordinates or guessed screen rect.
    fn show_inline_validation_focus(
        guidance:&mut Option<(InlineTarget,String)>,target:&InlineTarget,
        response:&egui::Response,ctx:&egui::Context
    ){
        if !response.has_focus() || !guidance.as_ref().is_some_and(|(t,_)|t==target){return;}
        if let Some((_,message))=guidance.take(){
            Self::publish_validation_focus(response,ctx,"reception.cell",&message);
        }
    }

    fn publish_validation_focus(response:&egui::Response,ctx:&egui::Context,
        channel:&'static str,message:&str){
        response.scroll_to_me(Some(Align::Center));
        floating_notice::action_response(ctx,channel,response);
        floating_notice::publish(ctx,channel,floating_notice::Tone::Validation,message);
    }
    /// Start only after a real failed native confirmation; never auto-approve.
    fn focus_failed_draft_input(&mut self,order_id:&str,error:&str,ctx:&egui::Context,
        origin:Option<floating_notice::Anchor>)->bool{
        if self.page!=Page::Reception || self.inline_editor.is_some(){return false;}
        // Do not change the selected receipt for an unrelated technical error.
        if self.active_id.as_deref()!=Some(order_id){return false;}
        self.workflow_ui.selection=None;
        self.workflow_ui.stop_preview();
        self.pending_input_guidance=Some((order_id.to_owned(),self.raw_source.clone(),Some(error.to_owned()),origin));
        ctx.request_repaint();
        true
    }
    /// Reuse regular review refresh; no additional DB query or UI validation.
    fn poll_input_guidance(&mut self,ctx:&egui::Context){
        let Some((order_id,raw,error,origin))=self.pending_input_guidance.clone()else{return;};
        if self.page!=Page::Reception || self.active_id.as_deref()!=Some(order_id.as_str())
            || self.raw_source!=raw || self.inline_editor.is_some() || self.inline_change.is_some()
            || self.workflow_ui.selection.is_some() || self.manual_workspace.is_some()
            || self.client_chooser.is_some() || self.item_editor.is_some()
            || self.product_picker.is_some() || self.vat_dialog.is_some()
            || self.workflow_dialog.is_some(){
            self.pending_input_guidance=None;return;
        }
        let targets=self.drafts.iter().find(|row|row.id==order_id)
            .zip(self.analysis.as_ref())
            .and_then(|(source,analysis)|self.review_panel.input_targets(source,analysis,error.as_deref()));
        let Some(targets)=targets else{
            if self.review_panel.refresh_failed() || self.analysis.is_none(){
                self.pending_input_guidance=None;
                if let Some(error)=error{
                    floating_notice::publish_from(ctx,"reception.workflow",floating_notice::Tone::Error,error,origin.as_ref());
                }
            }
            return;
        };
        self.pending_input_guidance=None;
        let target=targets.into_iter().find_map(|(rid,item_id,field,message)|{
            let source=self.drafts.iter().find(|r|r.id==order_id)?;
            let analysis=self.analysis.as_ref()?;
            let index=reception_service::recipients(analysis).iter().position(|r|
                reception_service::text(r,"id")==rid)?;
            let target=InlineTarget{recipient_id:rid,item_id,field};
            let original=current_inline_value(&analysis.recognition,
                source.pricing_preview.as_ref(),&target).unwrap_or_default();
            Some((index,target,original,message))
        });
        let Some((index,target,original,message))=target else{
            if let Some(error)=error{
                floating_notice::publish_from(ctx,"reception.workflow",floating_notice::Tone::Error,error,origin.as_ref());
            }
            return;
        };
        self.selected_recipient=index;
        self.workflow_ui.stop_preview();
        self.link_hover.clear();
        self.source_hover=None;
        self.begin_inline_edit(target.clone(),original,None);
        if self.inline_editor.as_ref().is_some_and(|e|e.target==target){
            self.guided_inline_target=Some(target.clone());
            self.inline_validation_focus=Some((target,message));
            ctx.request_repaint();
        }else if let Some(error)=error{
            floating_notice::publish_from(ctx,"reception.workflow",floating_notice::Tone::Error,error,origin.as_ref());
        }
    }

    /// React StaffChecks routing: navigate to the exact recipient/item
    /// rather than opening an unrelated manual screen or creating UI rules.
    fn handle_review_action(
        &mut self,action:review_panel::Action,ctx:&egui::Context
    ){
        if !self.review_panel.ready(){return;}
        let client_action=match &action{
            review_panel::Action::ChooseClient(_)=>true,
            review_panel::Action::Navigate{kind,recipient_id,..}=>recipient_id.is_empty() &&
                (kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_")),
            _=>false
        };
        if client_action{
            if let Some(pending)=self.inline_change.as_mut().filter(|p|p.is_delivery_command()){
                // Keep the click, but never run two writes against the same draft.
                pending.next_client_review=Some(action);
                return;
            }
        }
        match action{
            review_panel::Action::ChooseClient(id)=>{
                self.select_client_candidate(&id,ctx);
                return;
            }
            review_panel::Action::CustomerInquiry{recipient_id,request,cancel}=>{
                self.start_review_inline_action(InlineTarget{recipient_id,item_id:None,
                    field:if cancel{"cancelCustomerInquiry"}else{"requestCustomerInquiry"}},request,ctx);
                return;
            }
            review_panel::Action::ValidationApproval{recipient_id,request,cancel}=>{
                self.start_review_inline_action(InlineTarget{recipient_id,item_id:None,
                    field:if cancel{"cancelValidationApproval"}else{"validationApproval"}},request,ctx);
                return;
            }
            review_panel::Action::ApproveOne{recipient_id,item_id}=>{
                self.start_review_inline_action(
                    InlineTarget{
                        recipient_id,item_id:Some(item_id),field:"approveOne"
                    },serde_json::Value::Null,ctx
                );
                return;
            }
            review_panel::Action::CancelApproval{recipient_id,item_id}=>{
                self.start_review_inline_action(
                    InlineTarget{
                        recipient_id,item_id:Some(item_id),field:"cancelApproval"
                    },serde_json::Value::Null,ctx
                );
                return;
            }
            review_panel::Action::Candidate{recipient_id,field,value}=>{
                let Some(index)=self.analysis.as_ref().and_then(|analysis|
                    reception_service::recipients(analysis).iter().position(|row|
                        reception_service::text(row,"id")==recipient_id)
                )else{return};
                self.selected_recipient=index;
                self.workflow_ui.stop_preview();
                self.link_hover.clear();
                let field=match field.as_str(){
                    "name"=>"name","address"=>"address",
                    "phone"=>"phone","mobile"=>"mobile",_=>return
                };
                // Selecting even the same text confirms the field in Rust.
                self.start_review_inline_action(
                    InlineTarget{recipient_id,item_id:None,field},
                    serde_json::json!(value),ctx
                );
                return;
            }
            review_panel::Action::Navigate{
                kind,recipient_id,item_id,link_key
            }=>{
                if recipient_id.is_empty(){
                    if kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_"){
                        self.open_client_chooser(ctx);
                    }else if kind=="RECIPIENT"{
                        let direct=self.active_id.as_deref()
                            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
                            .is_some_and(|row|row.raw.trim().is_empty() &&
                                row.manual_model.as_ref().is_some_and(|saved|
                                    saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
                                ));
                        if direct{self.open_direct_workspace(ctx);}
                        else{self.open_manual_workspace(ctx);}
                    }
                    return;
                }
                let Some(index)=self.analysis.as_ref().and_then(|analysis|
                    reception_service::recipients(analysis).iter()
                        .position(|row|
                            reception_service::text(row,"id")==recipient_id)
                )else{return};
                self.selected_recipient=index;
                self.workflow_ui.stop_preview();
                self.link_hover.clear();
                self.source_hover=None;
                let field=link_key.as_deref()
                    .and_then(|key|key.rsplit_once(':').map(|(_,field)|field))
                    .unwrap_or(if item_id.is_some(){"product"}else{"name"});
                if kind=="APPLIED"{
                    // Informational auto-matches navigate/highlight the actual
                    // product cell. They never start an unnecessary write.
                    if let Some(item_id)=item_id.as_deref(){
                        let link=source_highlight::key(
                            &recipient_id,item_id,"product"
                        );
                        if let Some(id)=self.active_id.as_deref(){
                            self.link_hover.update_for(id,Some(link.clone()),ctx);
                            self.link_hover.active=Some(link.clone());
                            self.link_hover.scroll_to=Some(link);
                        }
                    }
                    ctx.request_repaint();
                    return;
                }
                let manual=matches!(kind.as_str(),
                    "UNASSIGNED_EVIDENCE"|"ITEM"|"OPTION"|
                    "MODEL"|"MODEL_CHOICE"|"ALIAS_CONFLICT"|
                    "CATEGORY_CONFLICT"|"QUANTITY_LINK"|"MODEL_CONFIRM");
                if manual{
                    let direct=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|row|row.id==id))
                        .is_some_and(|row|row.raw.trim().is_empty() &&
                            row.manual_model.as_ref().is_some_and(|saved|
                                saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
                            ));
                    if direct{self.open_direct_workspace(ctx);}
                    else{self.open_manual_workspace(ctx);}
                    if let Some(editor)=self.manual_workspace.as_mut(){
                        editor.focus_cell(
                            index,item_id,
                            if field=="product"{"product"}else{field}
                        );
                    }
                    return;
                }
                // The InlineTarget field is &'static str. Convert the
                // source's dynamic link suffix into an approved fixed field.
                let edit_field:&'static str=match field{
                    "name"=>"name","address"=>"address",
                    "phone"=>"phone","mobile"=>"mobile",
                    "note"=>"note","quantity"=>"quantity",
                    "unitPrice"|"price"=>"unitPrice",
                    _=>return
                };
                let target=InlineTarget{
                    recipient_id:recipient_id.clone(),item_id,
                    field:edit_field
                };
                let quote=self.active_id.as_deref().and_then(|id|
                    self.drafts.iter().find(|row|row.id==id))
                    .and_then(|row|row.pricing_preview.as_ref());
                if let Some(original)=self.analysis.as_ref()
                    .and_then(|analysis|current_inline_value(
                        &analysis.recognition,quote,&target)){
                    // A click commits the selection immediately; the visual
                    // source link still uses the shared four-panel state.
                    let source_field=if edit_field=="unitPrice"{"price"}else{edit_field};
                    let link=source_highlight::key(
                        &recipient_id,
                        target.item_id.as_deref().unwrap_or(&recipient_id),
                        source_field
                    );
                    if let Some(id)=self.active_id.as_deref(){
                        self.link_hover.update_for(id,Some(link.clone()),ctx);
                        self.link_hover.active=Some(link.clone());
                        self.link_hover.scroll_to=Some(link);
                    }
                    self.switch_inline_edit(target.clone(),original,None,ctx);
                    if self.inline_editor.as_ref().is_some_and(|e|e.target==target){
                        let message=match edit_field{
                            "name"=>"수취인 이름을 확인하세요.",
                            "address"=>"수취인 주소를 확인하세요.",
                            "phone"|"mobile"=>"연락처를 확인하세요.",
                            "quantity"=>"상품 수량을 확인하세요.",
                            "unitPrice"=>"상품 단가를 확인하세요.",
                            _=>"입력 내용을 확인하세요.",
                        };
                        self.inline_validation_focus=Some((target,message.to_owned()));
                    }
                }
            }
        }
        ctx.request_repaint();
    }

    /// The only coordinate owner for every reception modal. KakaoTalk is an
    /// externally owned Windows surface: keep modal bodies outside its area.
    /// Use the exact asymmetric inner rect of workspace_shell_frame rather
    /// than approximating it with equal left/right PANEL_GAP values.
    fn modal_work_area(&self,ctx:&egui::Context)->Rect {
        kakao_safe_modal_work_area(ctx.content_rect(),&self.layout)
    }

    fn show_manual_workspace(&mut self,ctx:&egui::Context,work_area:Rect) {
        if self.manual_workspace.as_ref().is_some_and(|workspace|
            self.active_id.as_deref()!=Some(workspace.order_id.as_str())
        ) {
            // A manual editor may never cross between two reception IDs.
            self.manual_workspace=None;
            self.link_hover.clear();
            self.source_hover=None;
            return;
        }
        // Use the same live source-column calculation as Reception, including
        // any user-resized pane ratio. The manual worksheet must not invent a
        // second source width or percentage-based layout policy.
        let viewport=ctx.content_rect();
        let content_width=(viewport.width()-2.0*theme::PANEL_GAP)
            .max(layout::LayoutState::minimum_width());
        let reception_source_width=self.layout.widths(content_width)[1];
        // The manual worksheet and reception intake share one order-scoped
        // source hover state. Borrow disjoint fields so the overlay can update
        // the same 50ms/150ms highlight contract while it is open.
        let result={
            let editor=self.manual_workspace.as_mut();
            let hover=&mut self.link_hover;
            let source=&mut self.source_hover;
            editor.and_then(|editor|editor.show(
                ctx,work_area,reception_source_width,hover,source
            ))
        };
        match result {
            Some(manual_workspace::Event::Closed)=>{
                self.manual_workspace=None;
                self.link_hover.clear();
                self.source_hover=None;
            }
            Some(event)=>{
                self.manual_workspace=None;
                self.link_hover.clear();
                self.source_hover=None;
                self.apply_manual_event(event,ctx);
            }
            None=>{}
        }
    }

    /// One final, shared application path for BOTH the explicitly opened
    /// manual-classification dialog and edits made inside reception cells.
    /// This is where immutable first recognition, before/after snapshots and
    /// the native (read-only) quote are stored.
    fn apply_manual_event(
        &mut self,event:manual_workspace::Event,ctx:&egui::Context
    ){
        // A successful native cell result supersedes an older validation or
        // admission message. Otherwise the confirmation handoff can mistake
        // that stale error for a failure of this newly committed edit.
        self.source_save_error=None;
        let manual_workspace::Event::Applied{
            order_id,raw,order_type,client_id,direct_input,model,assignment_order,
            order,quote,quote_error
        }=event else{return;};
                if self.active_id.as_deref()!=Some(order_id.as_str()) ||
                   self.raw_source!=raw ||
                   self.order_type.as_str()!=order_type ||
                   self.active_id.as_deref().and_then(|id|
                       self.drafts.iter().find(|row|row.id==id)
                   ).and_then(|row|row.client_id.as_deref()) !=client_id.as_deref() {
                    self.source_save_error=Some(
                        "수동 분류 중 원문·접수 유형·거래처가 변경되어 적용하지 않았습니다.".into()
                    );
                    return;
                }
                if direct_input {
                    if !raw.is_empty() || order.get("raw").and_then(serde_json::Value::as_str)!=Some("") {
                        self.source_save_error=Some("직접 입력 결과에 발주 원문이 포함되어 적용을 중단했습니다.".into());
                        return;
                    }
                    let Some(recips)=order.get("recipients").filter(|v|v.is_array()) else {
                        self.source_save_error=Some("직접 입력한 배송 정보를 확인할 수 없습니다.".into());
                        return;
                    };
                    if let Some(row)=self.drafts.iter_mut().find(|row|row.id==order_id){
                        row.source_input_method=None;
                        row.last_input_method=None;
                        row.analysis_raw=None;
                        row.analysis_evidence=None;
                        row.analysis_view=None;
                        row.client_match=None;
                        row.manual_model=Some(serde_json::json!({
                            "mode":"DIRECT","sourceRaw":"","orderType":order_type,
                            "clientId":client_id,"model":model,
                            "assignmentOrder":assignment_order
                        }));
                        row.pricing_preview=quote.as_ref().and_then(|calculation|{
                            let revision=calculation["authorityRevision"].as_u64()?;
                            let totals=calculation.get("totals")?;
                            let native_order=calculation.get("order")?;
                            Some(serde_json::json!({
                                "sourceRaw":"","orderId":order_id,
                                "orderType":order_type,"clientId":client_id,
                                "authorityRevision":revision,"status":"REVIEW",
                                "shippingFee":native_order.get("shippingFee"),
                                "shippingVersion":native_order.get("shippingVersion"),
                                "vatAmount":totals.get("vat"),
                                "subtotal":totals.get("subtotal"),
                                "total":totals.get("total"),
                                "pendingRecipients":totals.get("pendingRecipients"),
                                "notified":false,"paid":false,
                                "linePrices":calculation.get("linePrices"),
                                "freightLines":calculation.get("freightLines"),
                                "pricingBasis":calculation.get("pricingBasis")
                            }))
                        });
                        row.working_synced=false;
                    }
                    self.source_editing=false;
                    self.direct_source_entry=false;
                    self.clear_source_analysis();
                    self.restore_direct_manual_view();
                    self.working_sync_error=quote_error;
                    self.source_save_error=None;
                    self.queue_source_save();
                    self.start_remote_recheck(ctx);
                    let _=recips;
                    return;
                }
                if let Some(recips)=order.get("recipients").filter(|v|v.is_array()) {
                    if let Some(row)=self.drafts.iter_mut()
                        .find(|row|row.id==order_id) {
                        // The first Rust recognition never becomes an
                        // employee-corrected projection. Keep both snapshots
                        // and every subsequent manual edit independently.
                        let initial=row.analysis_evidence.as_ref()
                            .filter(|v|v.get("recipients").is_some_and(serde_json::Value::is_array));
                        let before=row.manual_model.as_ref()
                            .and_then(|v|v.pointer("/model/rows"))
                            .or_else(||initial.and_then(|v|v.get("recipients")));
                        if let (Some(original),Some(previous))=(initial,before) {
                            if previous!=recips {
                                row.edit_history.push(serde_json::json!({
                                    "id":uuid::Uuid::new_v4().to_string(),
                                    "at":chrono::Utc::now().to_rfc3339(),
                                    "sourceRaw":raw,
                                    "initialAnalysis":original,
                                    "beforeRows":previous,
                                    "afterRows":recips
                                }));
                            }
                        }
                        row.manual_model=Some(serde_json::json!({
                            "sourceRaw":raw,"orderType":order_type,
                            "clientId":client_id,"model":model,
                            "assignmentOrder":assignment_order
                        }));
                        row.pricing_preview=quote.as_ref().and_then(|calculation|{
                            let revision=calculation["authorityRevision"].as_u64()?;
                            let totals=calculation.get("totals")?;
                            let native_order=calculation.get("order")?;
                            Some(serde_json::json!({
                                "sourceRaw":raw,
                                "orderId":order_id,
                                "orderType":order_type,
                                "clientId":client_id,
                                "authorityRevision":revision,
                                "status":"REVIEW",
                                "shippingFee":native_order.get("shippingFee"),
                                "shippingVersion":native_order.get("shippingVersion"),
                                "vatAmount":totals.get("vat"),
                                "subtotal":totals.get("subtotal"),
                                "total":totals.get("total"),
                                "pendingRecipients":totals.get("pendingRecipients"),
                                "notified":false,"paid":false,
                                "linePrices":calculation.get("linePrices"),
                                "freightLines":calculation.get("freightLines"),
                                "pricingBasis":calculation.get("pricingBasis")
                            }))
                        });
                        row.working_synced=false;
                    }
                    if let Some(error)=quote_error {
                        self.working_sync_error=Some(error);
                    }else{
                        self.working_sync_error=None;
                    }
                    if let Some(analysis)=self.analysis.as_mut() {
                        analysis.apply_edited_recipients(&self.raw_source,recips);
                    }
                    self.queue_source_save();
                    self.start_remote_recheck(ctx);
                }else{
                    self.source_save_error=Some(
                        "배송 정보가 없어 수동 분류 결과를 적용하지 않았습니다.".into()
                    );
                }
    }

    /// Resolve each currently displayed recipient in a worker, not per egui
    /// frame. A stale worker is discarded when source or address changes.
    fn start_remote_recheck(&mut self,ctx:&egui::Context){
        let Some(order_id)=self.active_id.clone() else{return};
        let source_raw=self.raw_source.clone();
        let order_type=self.order_type.as_str().to_owned();
        let Some(analysis)=self.analysis.as_ref() else{return};
        let rows=reception_service::recipients(&analysis.recognition).to_vec();
        let recipients=rows.iter().map(|row|(
            reception_service::text(row,"id").to_owned(),
            reception_service::text(row,"address").to_owned()
        )).collect::<Vec<_>>();
        let duplicate=self.remote_recheck.as_ref().is_some_and(|pending|
            pending.order_id==order_id &&
            pending.source_raw==source_raw &&
            pending.order_type==order_type &&
            pending.recipients==recipients
        );
        match self.reception_job_admission(ReceptionJob::RemoteRecheck,duplicate){
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(_)=>{
                // The result is advisory. Never drop the recheck because an
                // employee write or source analysis currently owns the order.
                self.remote_recheck_due=true;
                // The conflicting worker's completion wakes egui, at which
                // point the deferred remote check can be admitted.
                return;
            }
            JobAdmission::Start=>{}
        }
        self.remote_recheck_due=false;
        // Abandon the obsolete address snapshot before starting another.
        // PendingRemoteRecheck::drop signals cooperative cancellation.
        self.remote_recheck=None;
        let cancel=Arc::new(AtomicBool::new(false));
        let cancelled=Arc::clone(&cancel);
        let gateway=Arc::clone(&self.gateway);
        let context=ctx.clone();
        let (tx,receiver)=mpsc::channel();
        self.remote_recheck=Some(PendingRemoteRecheck{
            order_id,source_raw,order_type,recipients,cancel,receiver
        });
        std::thread::spawn(move||{
            let values=rows.into_iter().map_while(|row|{
                if cancelled.load(Ordering::Acquire){return None;}
                let id=reception_service::text(&row,"id").to_owned();
                let address=reception_service::text(&row,"address").to_owned();
                let answer=if address.trim().is_empty(){
                    Ok(serde_json::Value::Null)
                }else{
                    reception_service::remote_decision(gateway.as_ref(),&row)
                };
                Some((id,address,answer))
            }).collect::<Vec<_>>();
            if !cancelled.load(Ordering::Acquire){
                let _=tx.send(values);
                context.request_repaint();
            }
        });
    }
    fn poll_remote_recheck(&mut self,ctx:&egui::Context){
        let outcome=self.remote_recheck.as_ref().map(|pending|
            pending.receiver.try_recv());
        let received=match outcome{
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>Some(Vec::new()),
            _=>None
        };
        let Some(values)=received else{
            return;
        };
        let Some(pending)=self.remote_recheck.take() else{return};
        if self.active_id.as_deref()!=Some(pending.order_id.as_str())
            || self.raw_source!=pending.source_raw
            || self.order_type.as_str()!=pending.order_type{return;}
        let Some(analysis)=self.analysis.as_mut() else{return};
        let current=reception_service::recipients(&analysis.recognition).to_vec();
        for (index,recipient) in current.iter().enumerate(){
            let id=reception_service::text(recipient,"id");
            let address=reception_service::text(recipient,"address");
            let Some((_,_,resolved))=values.iter().find(|(rid,addr,_)|
                rid==id && addr==address
            ) else{continue};
            if index>=analysis.reference.len(){
                analysis.reference.push(reception_service::RecipientReference{
                    remote:None,remote_error:None,item_prices:Vec::new()
                });
            }
            let reference=&mut analysis.reference[index];
            match resolved{
                Ok(value)=>{
                    reference.remote=if value.is_null(){None}else{Some(value.clone())};
                    reference.remote_error=None;
                },
                Err(error)=>{
                    reference.remote=None;
                    reference.remote_error=Some(error.clone());
                }
            }
        }
        analysis.reference.truncate(current.len());
        // Persist the newest OS/Rust remote-area reference into the cached
        // display snapshot; reopening the draft must not recheck unchanged
        // addresses solely to recover the indicator.
        self.queue_source_save();
        ctx.request_repaint();
    }

    fn begin_inline_edit(
        &mut self,target:InlineTarget,original:String,
        click_at:Option<egui::Pos2>
    ){
        // Even entering edit mode is a write intent: an older automatic
        // dealer quote must never land while the employee is typing.
        match self.reception_job_admission(ReceptionJob::InlineEdit,false){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::InlineEdit,blocker);
                return;
            }
        }
        if self.inline_editor.as_ref().is_some_and(|editor|
            Some(editor.source_id.as_str())!=self.active_id.as_deref()){
            self.inline_editor=None;
        }
        if self.inline_editor.as_ref().is_some_and(|editor|editor.target==target){
            return;
        }
        // A different field is never allowed to discard uncommitted typing.
        if self.inline_editor.as_ref().is_some_and(|editor|
            editor.text!=editor.original){
            self.source_save_error=Some(
                "현재 셀 편집을 Enter로 확정하거나 Esc로 취소하세요.".into()
            );
            return;
        }
        // Compare the same display representation before/after editing,
        // so adding separators alone never creates a native write.
        let original=if matches!(target.field,"unitPrice"|"shippingCharge"){
            crate::input::parse_money_input(&original)
                .map(crate::input::money_edit_text).unwrap_or(original)
        }else{original};
        self.inline_editor=Some(InlineReceptionEditor{
            source_id:self.active_id.clone().unwrap_or_default(),
            target,original:original.clone(),text:original,
            focus_next:true,click_at
        });
    }

    /// One click on B commits dirty A, retains B as the next target and
    /// automatically focuses B when Rust returns. Clean A switches immediately.
    fn switch_inline_edit(
        &mut self,target:InlineTarget,original:String,
        click_at:Option<egui::Pos2>,ctx:&egui::Context
    ){
        if self.inline_change.is_some(){return;}
        let transition=cell_navigation::cell_switch(
            self.inline_editor.as_ref().is_some_and(|editor|editor.target==target),
            self.inline_editor.as_ref().is_some_and(|editor|editor.text!=editor.original));
        if transition==cell_navigation::CellSwitch::KeepCurrent{return;}
        if transition==cell_navigation::CellSwitch::CommitThenOpen{
            self.submit_inline_edit(ctx);
            if let Some(pending)=self.inline_change.as_mut(){
                pending.next=Some((target,click_at));
            }
            return;
        }
        self.inline_editor=None;
        self.guided_inline_target=None;
        self.pending_input_guidance=None;
        self.begin_inline_edit(target,original,click_at);
        ctx.request_repaint();
    }

    fn submit_inline_edit(&mut self,ctx:&egui::Context){
        // Errors belong to the previous attempt. Any failure below writes a
        // fresh reason that the caller can surface to the employee.
        self.source_save_error=None;
        match self.reception_job_admission(
            ReceptionJob::InlineEdit,self.inline_change.is_some()
        ){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::InlineEdit,blocker);
                return;
            }
        }
        let Some(editor)=self.inline_editor.take() else{return};
        if editor.text==editor.original{
            self.guided_inline_target=None;
            return;
        }
        let Some(source)=self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|row|row.id==id)
        ).cloned() else{return};
        if editor.source_id!=source.id{
            self.source_save_error=Some(
                "다른 발주서에서 시작한 입력은 현재 주문에 적용되지 않았습니다.".into()
            );
            return;
        }
        let direct_input=source.raw.trim().is_empty() &&
            source.manual_model.as_ref().is_some_and(|saved|
                saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
            );
        if (!direct_input && (source.analysis_raw.as_deref()!=Some(source.raw.as_str()) ||
            source.analysis_evidence.is_none())) ||
            self.working_sync.is_some() || self.client_change.is_some(){
            self.source_save_error=Some("현재 주문은 수정할 수 없습니다.".into());
            self.inline_editor=Some(editor);
            return;
        }
        let field=editor.target.field;
        let value=match field{
            "quantity"|"unitPrice"|"shippingCharge"=>{
                let digits=editor.text.trim().replace(',',"");
                if digits.is_empty() && field!="quantity"{
                    serde_json::Value::Null
                }else{
                    let parsed=digits.parse::<i64>();
                    let valid=parsed.ok().filter(|n|
                        (0..=9_007_199_254_740_991).contains(n) &&
                        (field!="quantity"||*n>0)
                    );
                    let Some(number)=valid else{
                        let message=match field{
                            "quantity"=>"수량은 1 이상의 정수로 입력하세요.",
                            "unitPrice"=>"단가는 0 이상의 정수로 입력하세요.",
                            _=>"운임은 0 이상의 정수로 입력하세요.",
                        };
                        self.source_save_error=Some(message.into());
                        self.inline_validation_focus=Some((editor.target.clone(),message.into()));
                        let mut editor=editor;
                        editor.focus_next=true;
                        self.inline_editor=Some(editor);
                        return;
                    };
                    if field=="quantity"{
                        serde_json::json!(number.to_string())
                    }else{serde_json::json!(number)}
                }
            },
            "remoteDelivery"|"vatOverride"=>{
                match editor.text.as_str(){
                    "true"=>serde_json::json!(true),
                    "false"=>serde_json::json!(false),
                    _=>{
                        self.source_save_error=Some("체크박스 변경값이 올바르지 않습니다.".into());
                        self.inline_editor=Some(editor);
                        return;
                    }
                }
            },
            _=>serde_json::json!(editor.text.clone())
        };
        let request=manual_workspace::InlineRequest{
            recipient_id:editor.target.recipient_id.clone(),
            item_id:editor.target.item_id.clone(),
            field:field.to_string(),value
        };
        let order_id=source.id.clone();
        let raw=source.raw.clone();
        let (tx,receiver)=mpsc::channel();
        let gateway=Arc::clone(&self.gateway);
        self.inline_change=Some(PendingInlineChange{
            order_id,raw,target:editor.target.clone(),editor:Some(editor),next_client_review:None,next:None,receiver
        });
        let context=ctx.clone();
        std::thread::spawn(move||{
            let result=manual_workspace::apply_inline(
                gateway.as_ref(),&source,&request
            );
            let _=tx.send(result);
            context.request_repaint();
        });
    }

    fn poll_inline_change(&mut self,ctx:&egui::Context){
        let complete=self.inline_change.as_ref().map(|pending|
            pending.receiver.try_recv());
        let result=match complete {
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("입력 내용 수정 작업이 중단되었습니다.".into())),
            _=>None
        };
        let Some(result)=result else{
            return;
        };
        let Some(pending)=self.inline_change.take() else{return};
        let confirm_order_id=(self.pending_draft_confirmation.as_deref()==
            Some(pending.order_id.as_str())).then(||pending.order_id.clone());
        let guided=self.guided_inline_target.as_ref()==Some(&pending.target)
            && pending.editor.is_some();
        if guided{self.guided_inline_target=None;}
        let continue_guidance=guided && pending.next.is_none()
            && pending.next_client_review.is_none() && confirm_order_id.is_none();
        if self.active_id.as_deref()!=Some(pending.order_id.as_str()) ||
            self.raw_source!=pending.raw{
            let message=
                "편집 중 다른 발주서로 이동하여 이전 셀 수정 결과를 적용하지 않았습니다.";
            self.source_save_error=Some(message.into());
            if confirm_order_id.is_some(){
                self.pending_draft_confirmation=None;
                self.pending_stage_display=None;
                floating_notice::publish(
                    ctx,"reception.workflow",floating_notice::Tone::Error,
                    format!("셀 저장이 취소되어 주문 확정을 중단했습니다. {message}")
                );
            }
            ctx.request_repaint();
            return;
        }
        match result {
            Ok(event)=>{
                self.source_save_error=None;
                self.apply_manual_event(event,ctx);
                if continue_guidance && self.source_save_error.is_none() && self.page==Page::Reception{
                    self.pending_input_guidance=Some((pending.order_id.clone(),pending.raw.clone(),None,None));
                }
                if self.source_save_error.is_none()
                    && matches!(pending.target.field,"product"|"removeItem")
                    && self.item_editor.as_ref().is_some_and(|dialog|
                        dialog.is_same_target(
                            &pending.order_id,&pending.raw,
                            &pending.target.recipient_id,
                            pending.target.item_id.as_deref().unwrap_or("")
                        )
                    ){
                    // The dialog closes only after the native reducer applied
                    // the selected product/remove result successfully.
                    self.item_editor=None;
                }
                // The prior cell and source must commit successfully before
                // opening the destination. Use the new Rust projection so a
                // quantity edit cannot leave a stale price in the next input.
                if self.source_save_error.is_none() &&
                    self.page==Page::Reception && confirm_order_id.is_none()
                {
                    if let Some((mut target,click_at))=pending.next {
                        // The successful first edit may have upgraded a legacy
                        // display ID. Resolve the queued destination identically.
                        let (rid,iid)=gateway::rules::scope_reception_edit_ids(
                            &pending.order_id,&target.recipient_id,target.item_id.as_deref());
                        target.recipient_id=rid;
                        target.item_id=iid;
                        let displayed_recipient=self.analysis.as_ref()
                            .and_then(|analysis|
                                reception_service::recipients(analysis)
                                    .get(self.selected_recipient))
                            .and_then(|row|row["id"].as_str());
                        if displayed_recipient==Some(target.recipient_id.as_str()){
                            if matches!(target.field,"product"|"removeItem") {
                                if let Some(item_id)=target.item_id.as_deref(){
                                    // One click on the product while quantity
                                    // is being edited commits quantity first,
                                    // then opens React's product modal.
                                    let recipient_id=target.recipient_id.clone();
                                    let item_id=item_id.to_owned();
                                    if target.field=="removeItem"{
                                        self.open_item_remove_confirmation(&recipient_id,&item_id,ctx);
                                    }else{
                                        self.open_item_editor(&recipient_id,&item_id,ctx);
                                    }
                                }
                                return;
                            }
                            let refreshed=self.analysis.as_ref().and_then(|analysis|{
                                let quote=self.active_id.as_deref()
                                    .and_then(|id|self.drafts.iter()
                                        .find(|row|row.id==id))
                                    .and_then(|row|row.pricing_preview.as_ref());
                                current_inline_value(&analysis.recognition,quote,&target)
                            });
                            if let Some(value)=refreshed{
                                self.begin_inline_edit(target,value,click_at);
                            }
                        }
                    }
                }
            },
            Err(error)=>{
                self.source_save_error=Some(format!("셀 수정 실패: {error}"));
                if matches!(pending.target.field,"product"|"removeItem")
                    && self.item_editor.as_ref().is_some_and(|dialog|
                        dialog.is_same_target(
                            &pending.order_id,&pending.raw,
                            &pending.target.recipient_id,
                            pending.target.item_id.as_deref().unwrap_or("")
                        )
                    ){
                    // Do not resurrect a hidden TextEdit when a modal submit
                    // fails. Keep the selected product and its dialog open.
                    if let Some(dialog)=self.item_editor.as_mut(){
                        dialog.report_error(error);
                    }
                }else if let Some(mut editor)=pending.editor{
                    if guided{self.guided_inline_target=Some(editor.target.clone());}
                    let message=if confirm_order_id.is_some(){
                        format!("셀 수정에 실패해 주문 확정을 중단했습니다. {error}")
                    }else{format!("셀 수정 실패: {error}")};
                    self.inline_validation_focus=Some((editor.target.clone(),message));
                    editor.focus_next=true;
                    self.inline_editor=Some(editor);
                }else{
                    // A non-text command has no editor to restore. The last
                    // committed delivery state remains visible and retryable.
                    floating_notice::publish(ctx,"reception.workflow",
                        floating_notice::Tone::Error,format!("변경 저장 실패: {error}"));
                }
            }
        }
        if let Some(order_id)=confirm_order_id{
            let error=self.source_save_error.clone();
            self.pending_draft_confirmation=None;
            if let Some(error)=error{
                // A restored editor owns the same failure; do not publish
                // a competing workflow-button notice.
                if self.inline_validation_focus.is_none(){
                    floating_notice::publish(
                        ctx,"reception.workflow",floating_notice::Tone::Error,
                        format!("셀 수정에 실패해 주문 확정을 중단했습니다. {error}")
                    );
                }
            }else{
                self.start_draft_confirmation(&order_id,ctx);
            }
            if self.pending_stage_target().is_none(){
                self.restore_failed_stage_target(&format!("group:{order_id}"),ctx);
            }
        }else if let Some(action)=pending.next_client_review{
            // The completed delivery command no longer owns the draft. On
            // failure the previous state is intact; client selection is safe.
            self.handle_review_action(action,ctx);
        }
        ctx.request_repaint();
    }

    fn open_vat_settings(&mut self){
        if self.vat_dialog.is_some(){return;}
        match self.reception_job_admission(ReceptionJob::VatEdit,false){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::VatEdit,blocker);
                return;
            }
        }
        let Some(order_id)=self.active_id.clone() else{return};
        let Some(analysis)=self.analysis.as_ref() else{return};
        let rows=reception_service::recipients(analysis);
        if rows.is_empty() || self.vat_change.is_some() ||
            self.inline_change.is_some() || self.inline_editor.is_some(){
            return;
        }
        self.vat_dialog=Some(VatDialog::new(
            order_id,self.raw_source.clone(),rows
        ));
    }

    fn start_vat_change(
        &mut self,request:manual_workspace::InlineRequest,ctx:&egui::Context
    ){
        match self.reception_job_admission(
            ReceptionJob::VatEdit,self.vat_change.is_some()
        ){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::VatEdit,blocker);
                return;
            }
        }
        let Some(source)=self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|row|row.id==id)
        ).cloned() else{return};
        let (tx,receiver)=mpsc::channel();
        let context=ctx.clone();
        let gateway=Arc::clone(&self.gateway);
        self.vat_change=Some(PendingVatChange{
            order_id:source.id.clone(),source_raw:source.raw.clone(),receiver
        });
        std::thread::spawn(move||{
            let result=manual_workspace::apply_inline(
                gateway.as_ref(),&source,&request
            );
            let _=tx.send(result);
            context.request_repaint();
        });
    }

    fn poll_vat_change(&mut self,ctx:&egui::Context){
        let fetched=self.vat_change.as_ref().map(|pending|
            pending.receiver.try_recv());
        let outcome=match fetched{
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("부가세 설정 저장이 중단됐습니다.".into())),
            _=>None
        };
        let Some(outcome)=outcome else{
            return;
        };
        let Some(pending)=self.vat_change.take() else{return};
        if self.active_id.as_deref()!=Some(pending.order_id.as_str()) ||
            self.raw_source!=pending.source_raw{return;}
        match outcome{
            Ok(event)=>{
                self.source_save_error=None;
                self.apply_manual_event(event,ctx);
                if self.source_save_error.is_none(){
                    self.vat_dialog=None;
                }
            }
            Err(error)=>{
                if let Some(dialog)=self.vat_dialog.as_mut(){
                    dialog.error=Some(error);
                }
            }
        }
        ctx.request_repaint();
    }

fn vat_recipient_list_height(count:usize,work_area_height:f32)->f32{
    // Always reserve five checkbox rows, even for one or two recipients.
    // Grow through ten rows; on smaller windows, protect the modal footer
    // by capping the viewport height and allowing the list to scroll.
    let visible_rows=count.clamp(
        theme::RECEPTION_VAT_LIST_MIN_VISIBLE_ROWS,
        theme::RECEPTION_VAT_LIST_VISIBLE_ROWS
    );
    let wanted=visible_rows as f32*theme::RECEPTION_VAT_LIST_ROW_HEIGHT;
    wanted.min((work_area_height-theme::RECEPTION_VAT_LIST_SCREEN_RESERVE)
        .max(theme::RECEPTION_VAT_LIST_ROW_HEIGHT))
}

    fn show_vat_settings(&mut self,ctx:&egui::Context,work_area:Rect){
        let Some(dialog)=self.vat_dialog.as_mut() else{return};
        let confirmed=self.confirmed_modal_target.clone();
        if confirmed.is_none() && (self.active_id.as_deref()!=Some(dialog.order_id.as_str()) ||
            self.raw_source!=dialog.source_raw){
            self.vat_dialog=None;
            return;
        }
        let recipients=if let Some(target)=confirmed.as_ref(){
            self.workflow_snapshot.as_ref().map(|board|{
                let mut seen=std::collections::HashSet::new();
                board.groups.iter().flat_map(|g|g.shipments.iter())
                    .filter(|s|s.order_id==target.selection.order_id && seen.insert(s.recipient_id.clone()))
                    .map(|s|s.recipient.clone()).collect::<Vec<_>>()
            }).unwrap_or_default()
        }else{
            self.analysis.as_ref().map(|analysis|
                reception_service::recipients(analysis).to_vec()).unwrap_or_default()
        };
        let busy=self.vat_change.is_some();
        let mut cancel=false;
        let mut apply=false;
        crate::popup::show_decision_window(ctx,"부가세 설정","egui-order-vat-settings",work_area,|ui|{
            crate::popup::decision_workspace(ui,|decision|{
                cancel=decision.header("부가세 설정","취소",!busy
                );
                decision.body(|ui|{
                    // The tax rate applies to the whole order and precedes
                    // both the bulk and the per-recipient VAT selections.
                    ui.horizontal(|row|{
                        crate::ui_primitives::form_field_label(row,"부가세율");
                        // Three-digit compact field; decimals remain supported by
                        // native VAT parser. The 36pt shared TextEdit centers caret,
                        // selection and glyphs together.
                        crate::input::compact_text_field(
                            row,&mut dialog.rate_text,48.0,!busy
                        );
                        // Let the horizontal row center the natural-height unit
                        // label against the 36pt TextEdit, instead of stretching the
                        // Label itself to 36pt (which top-aligns its glyphs).
                        row.label(typography::body("%"));
                    });
                    ui.add_space(theme::SPACE_12);
                    ui.horizontal_wrapped(|row|{
                        crate::ui_layout::apply_spacing_role(row,crate::ui_layout::SpacingRole::FormColumns);
                        // These are mutually exclusive whole-order shortcuts,
                        // not a separate 3-state mode. Their checked state
                        // always follows the visible recipient checkboxes.
                        let mut none=dialog.scope(&recipients)==VatScope::None;
                        if crate::checkbox::checkbox(
                            row,&mut none,"전체 미적용",!busy
                        ).changed() && none {
                            // Unchecking an active global shortcut is a no-op.
                            // Partial recipient selection leaves both shortcuts off.
                            dialog.set_all_checked(&recipients,false);
                        }
                        let mut all=dialog.scope(&recipients)==VatScope::All;
                        if crate::checkbox::checkbox(
                            row,&mut all,"전체 적용",!busy
                        ).changed() && all {
                            dialog.set_all_checked(&recipients,true);
                        }
                    });
                    ui.add_space(theme::SPACE_12);
                    let list_height=Self::vat_recipient_list_height(
                        recipients.len(),work_area.height()
                    );
                    // The tinted bordered surface marks the exact scrollable region.
                    // Reserve five rows for short orders, expand to ten recipients,
                    // then scroll within the same surface for additional orders.
                    crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::RecipientChecklist)
                        .show(ui,|panel|{
                            crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                                .max_height(list_height)
                                .auto_shrink([false,false])
                                .id_salt("order-vat-select-recipients")
                                .show(panel,|list|{
                                    // The row height already includes all vertical
                                    // breathing room; egui's default item gap would
                                    // otherwise clip the 10th entry and show only ~8.
                                    crate::ui_layout::apply_spacing_role(list,crate::ui_layout::SpacingRole::ExplicitVertical);
                                    for (index,recipient) in recipients.iter().enumerate(){
                                        let id=reception_service::text(recipient,"id");
                                        if id.is_empty(){continue;}
                                        let name=reception_service::text(recipient,"name");
                                        let mut checked=dialog.recipients.contains(id);
                                        let label=format!(
                                            "{} · {}",index+1,
                                            if name.is_empty(){"수취인 확인"}else{name}
                                        );
                                        if crate::checkbox::checkbox_list_row(
                                            list,("order-vat-recipient",id),
                                            &mut checked,&label,!busy,
                                            theme::RECEPTION_VAT_LIST_ROW_HEIGHT
                                        ).changed(){
                                            dialog.set_recipient_checked(id,checked);
                                        }
                                    }
                                });
                        });
                    let error=dialog.error.as_deref();
                    let status_width=ui.available_width();
                    crate::ui_layout::fixed_status_slot(
                        ui,status_width,typography::METADATA_LINE_HEIGHT,
                        error,error,theme::DANGER
                    );
                });
                decision.actions(|buttons|{
                    if crate::button::action(
                        buttons,"적용",ButtonRole::Primary,!busy
                    ).clicked(){apply=true;}
                });
            });
        });
        if cancel && !busy{self.vat_dialog=None;if confirmed.is_some(){self.confirmed_modal_target=None;}return;}
        if apply && !busy{
            let result=self.vat_dialog.as_ref()
                .expect("current VAT dialog").request(&recipients);
            match result{
                Ok(request)=>{
                    if let Some(target)=confirmed.as_ref(){
                        // Release the modal's job admission only for its own submit.
                        let dialog=self.vat_dialog.take();
                        self.confirmed_modal_target=None;
                        if !self.submit_confirmed_change(target,"vatSettings",None,request.value,ctx){
                            self.vat_dialog=dialog;
                            self.confirmed_modal_target=Some(target.clone());
                        }
                    }else{self.start_vat_change(request,ctx);}
                },
                Err(error)=>{
                    if let Some(dialog)=self.vat_dialog.as_mut(){
                        dialog.error=Some(error);
                    }
                }
            }
        }
    }

    fn show_edit_history(&mut self,ctx:&egui::Context,work_area:Rect){
        if !self.edit_history_open{return;}
        let receipt=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id)).cloned();
        let Some(receipt)=receipt else{
            self.edit_history_open=false;
            return;
        };
        let original=receipt.analysis_evidence.as_ref()
            .and_then(|value|value.get("recipients"))
            .cloned().unwrap_or(serde_json::Value::Null);
        let latest=receipt.manual_model.as_ref()
            .and_then(|m|m.pointer("/model/rows"))
            .cloned().unwrap_or_else(||original.clone());
        let mut close=false;
        crate::popup::modal_window(ctx,"최초 분석 및 수정 이력",
            "reception-correction-history",work_area)
            .default_width(work_area.width().min(860.0))
            .show(ctx,|ui|{

                crate::popup::modal_flow_workspace(ui,|workspace|{
                    close=workspace.header("최초 분석 및 수정 이력","닫기",true
                    );
                    workspace.scroll_region("reception-correction-history-body",(work_area.height()*0.6).min(crate::popup::modal_body_height_budget(work_area.height(),false)),|body|{
                                body.label(typography::section_heading("최초 분석"));
                                body.add(egui::Label::new(
                                    typography::body(&serde_json::to_string_pretty(&original)
                                        .unwrap_or_default())
                                ).selectable(true));
                                crate::ui_primitives::section_divider(body);
                                body.label(typography::section_heading("현재 수정 결과"));
                                body.add(egui::Label::new(
                                    typography::body(&serde_json::to_string_pretty(&latest)
                                        .unwrap_or_default())
                                ).selectable(true));
                                crate::ui_primitives::section_divider(body);
                                for (index,event) in receipt.edit_history.iter().enumerate().rev(){
                                    crate::button::collapsing_header(format!(
                                        "수정 {} · {}",index+1,
                                        event["at"].as_str().unwrap_or("")
                                    )).show(body,|history|{
                                        history.label(typography::field_label("수정 전"));
                                        history.add(egui::Label::new(typography::body(
                                            &serde_json::to_string_pretty(&event["beforeRows"])
                                                .unwrap_or_default()
                                        )).selectable(true));
                                        history.label(typography::field_label("수정 후"));
                                        history.add(egui::Label::new(typography::body(
                                            &serde_json::to_string_pretty(&event["afterRows"])
                                                .unwrap_or_default()
                                        )).selectable(true));
                                    });
                                }
                    });
                });
            });
        if close {self.edit_history_open=false;}
    }

    fn open_client_chooser(&mut self, ctx: &egui::Context) {
        if self.client_chooser.is_some(){return;}
        // Do not display a chooser that could submit a conflicting request.
        match self.reception_job_admission(ReceptionJob::ManualClient,false){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::ManualClient,blocker);
                return;
            }
        }
        let Some(order_id) = &self.active_id else { return; };
        self.client_chooser = Some(ClientChooser::new(
            order_id.clone(), Arc::clone(&self.gateway), ctx.clone()
        ));
    }

    /// A verified chooser click owns only the selection intent.
    /// Dismiss its UI synchronously; the guarded Rust worker keeps the
    /// original source/choice snapshot and applies it on completion.
    fn accept_client_chooser_selection(
        &mut self,client_id:&str,verified:bool,ctx:&egui::Context
    ){
        if !verified{return;}
        // A modal must never remain interactive during asynchronous pricing.
        self.client_chooser=None;
        ctx.request_repaint();
        let complete=self.active_id.as_deref()
            .and_then(|active|self.drafts.iter().find(|row|row.id==active))
            .is_some_and(|row|
                row.client_id.as_deref()==Some(client_id) &&
                row.pricing_preview.as_ref().is_some_and(complete_reception_quote)
            );
        if !complete {
            self.apply_manual_client(client_id,ctx);
        }
    }

    fn show_client_chooser(&mut self, ctx: &egui::Context, work_area:Rect) {
        if let Some(target)=self.confirmed_modal_target.clone(){
            if self.client_chooser.is_some(){
                let current=self.workflow_snapshot.as_ref().and_then(|b|b.groups.iter()
                    .find(|g|g.id==target.selection.group_id)).map(|g|g.dealer_id.clone());
                let event=self.client_chooser.as_mut().and_then(|chooser|
                    chooser.show(ctx,Arc::clone(&self.gateway),current.as_deref(),work_area));
                match event{
                    Some(Choice::Selected(id))=>{
                        if self.client_chooser.as_ref().is_some_and(|chooser|chooser.contains(&id)){
                            self.client_chooser=None;
                            self.confirmed_modal_target=None;
                            if current.as_deref()!=Some(id.as_str()){
                                self.submit_confirmed_change(&target,"clientId",None,serde_json::json!(id),ctx);
                            }
                        }
                    },
                    Some(Choice::Closed)=>{self.client_chooser=None;self.confirmed_modal_target=None;},
                    None=>{}
                }
                return;
            }
        }
        // The chooser belongs to one reception, not a global selection.
        if self.client_chooser.as_ref().is_some_and(|chooser|
            self.active_id.as_deref() != Some(chooser.order_id.as_str())
        ) {
            self.client_chooser = None;
            return;
        }
        let current_id = self.active_id.as_deref()
            .and_then(|id| self.drafts.iter().find(|row| row.id == id))
            .and_then(|row| row.client_id.as_deref())
            .map(str::to_owned);
        let event = self.client_chooser.as_mut().and_then(|chooser|
            chooser.show(ctx, Arc::clone(&self.gateway), current_id.as_deref(),work_area)
        );
        match event {
            Some(Choice::Selected(id)) => {
                floating_notice::dispatch_pointer(ctx,"reception.client_change");
                let verified=self.client_chooser.as_ref()
                    .is_some_and(|chooser|chooser.contains(&id));
                self.accept_client_chooser_selection(&id,verified,ctx);
            }
            Some(Choice::Closed) => { self.client_chooser = None; }
            None => {}
        }
    }

    fn new_reception(&mut self, ctx: &egui::Context) {
        if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
            ReceptionJob::NavigateReception,false
        ){
            self.reception_job_conflict(ReceptionJob::NavigateReception,blocker);
            return;
        }
        // Do not clear the visible order on a rejected new-order request.
        if self.workflow_read.required() && self.workflow_error.is_none(){
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Info,
                "주문 상태를 갱신하고 있습니다. 갱신 후 새 주문을 열어 주세요.");
            return;
        }
        if self.drafts.len()>=1000 && !self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|item|item.id==id))
            .is_some_and(|item|item.raw.trim().is_empty() && item.manual_model.is_none()){
            self.source_save_error=Some("미확정 접수 1,000건을 초과할 수 없습니다.".into());
            return;
        }
        self.pending_stage_display=None;
        self.workflow_ui.selection=None;
        self.workflow_ui.folded=None;
        self.workflow_ui.stop_preview();
        self.link_hover.clear();
        self.source_hover=None;
        // An already-open empty receipt is the new order. Do not multiply
        // empty records when the user clicks "+ 새 주문" repeatedly.
        if self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|item| item.id == id)
        ).is_some_and(|item| item.raw.trim().is_empty() && item.manual_model.is_none()) {
            // Clicking '+ 새 주문' on an empty direct-entry draft simply
            // returns to intake mode; it must not strand SourceEntry forever.
            self.direct_source_entry=false;
            self.source_editing = false;
            return;
        }
        use std::sync::atomic::AtomicU64;
        use std::time::{SystemTime, UNIX_EPOCH};
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let epoch = SystemTime::now().duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos()).unwrap_or(0);
        let id = format!("egui-{}-{epoch}-{}", std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed));
        self.drafts.push(WorkingReception {
            id: id.clone(), raw: String::new(),
            order_type: OrderType::Agency.as_str().to_owned(),
            source_input_method: None, last_input_method: None,
            analysis_raw: None, analysis_evidence: None,
            analysis_view:None, client_match: None,
            chat_handle: None, chat_title: None,
            client_id: None, client_selection: None, pricing_preview: None,
            working_version: None, working_synced: false, manual_model: None,
            edit_history: Vec::new(),
        });
        self.select_reception(&id, ctx);
    }

    fn poll_source_saves(&mut self,ctx:&egui::Context) {
        let mut events = Vec::new();
        if let Some(worker) = &self.source_autosave {
            while let Some(status) = worker.try_status() {
                events.push(status);
            }
        }
        for event in events {
            match event {
                source_autosave::SaveStatus::Saved{revision,active_id,raw,order_type}=>{
                    let _=revision;
                    self.source_save_error=None;
                    let saved_import=self.pending_clipboard_clear.as_ref()
                        .is_some_and(|pending|
                            active_id.as_deref()==Some(pending.order_id.as_str())
                            && raw==pending.raw
                            && order_type==pending.order_type
                        );
                    if saved_import{
                        let pending=self.pending_clipboard_clear.take()
                            .expect("checked pending clipboard after save");
                        match clipboard_intake::clear_if_unchanged(pending.sequence){
                            Ok(true)=>{
                                // The just-cleared clipboard must immediately
                                // disable the two reception intake buttons.
                                self.clipboard_monitor.mark_cleared(pending.sequence);
                            }
                            Ok(false)=>{} // The user copied something new.
                            Err(error)=>{self.clipboard_error=Some(error);}
                        }
                    }
                }
                source_autosave::SaveStatus::Failed(error) => {
                    // The order was NOT stored. Keep the clipboard unchanged.
                    self.pending_clipboard_clear=None;
                    self.source_save_error=Some(error);
                }
                source_autosave::SaveStatus::DeleteFinished{id,result}=>{
                    self.finish_pending_draft_delete(&id,result,ctx);
                }
                source_autosave::SaveStatus::FeedbackFailed(error) => {
                    // Source storage succeeded but the separate, private OCR
                    // audit write failed. Never silently report learning saved.
                    self.clipboard_error=Some(format!(
                        "OCR 정정 원본/수정본 보관 실패: {error}"
                    ));
                }
            }
        }
        if self.source_autosave.as_ref().is_none_or(|worker|worker.is_finished()){
            if let Some((id,_))=self.pending_draft_delete.clone(){
                self.finish_pending_draft_delete(&id,
                    Err("자동저장 작업이 중단되었습니다.".into()),ctx);
            }
        }
    }

    fn start_source_analysis(&mut self, context: egui::Context) {
        let key=(
            self.active_id.clone().unwrap_or_default(),
            self.raw_source.clone(),self.order_type.as_str().to_owned()
        );
        // Repeated same-source scheduling must not spawn another OCR/matcher
        // worker. A changed source is still allowed to supersede old evidence.
        if self.analysis_pending && self.analysis_running_key.as_ref()==Some(&key){
            return;
        }
        // A worker about to commit a dealer/price or operational version may
        // never be raced by an automatic source recheck. Recheck after it ends.
        if self.reception_job_admission(
            ReceptionJob::Analysis,false
        )!=JobAdmission::Start{
            self.analysis_due_at=Some(Instant::now()+Duration::from_millis(250));
            context.request_repaint_after(Duration::from_millis(260));
            return;
        }
        self.analysis_running_key=Some(key);
        self.auto_client_change=None;
        self.analysis_request = self.analysis_request.wrapping_add(1);
        let request = self.analysis_request;
        let source = self.raw_source.clone();
        let gateway = Arc::clone(&self.gateway);
        self.analysis = None;
        self.analysis_error = None;
        self.analysis_pending = true;
        let association = self.active_id.as_deref()
            .and_then(|id| self.drafts.iter().find(|item| item.id == id));
        // A persisted HWND is never trusted without a current verified
        // handle+title pair. A reused HWND cannot silently select a dealer.
        let chat_handle = association.and_then(|draft| {
            let handle = draft.chat_handle.as_deref()?;
            let title = draft.chat_title.as_deref()?;
            self.kakao_tracker.verified(handle)
                .filter(|chat| chat.title == title).map(|_| handle.to_owned())
        });
        let (windows, active_chat_handle) = self.kakao_tracker.context(chat_handle.as_deref());
        let manual_client_id = association.filter(|draft|
            draft.client_selection.as_deref() == Some("MANUAL") ||
            draft.manual_model.is_some()
        ).and_then(|draft| draft.client_id.clone());
        let manual_receipt=association.filter(|draft|
            draft.manual_model.is_some()
        ).cloned();
        let (sender, receiver) = mpsc::channel();
        self.analysis_receiver = Some(receiver);
        std::thread::spawn(move || {
            let result=(|| -> Result<reception_service::ReceptionAnalysis,String> {
                let mut analysis=reception_service::analyze_raw(
                    gateway.as_ref(), &source, windows, active_chat_handle, manual_client_id
                )?;
                if let Some(row)=manual_receipt {
                    // Reapply employee-confirmed assignments through Rust,
                    // not a fresh UI parser. The source must still be exact.
                    let mut model=row.manual_model.as_ref()
                        .and_then(|value|value.get("model")).cloned()
                        .ok_or("저장된 수동 분류 결과를 읽을 수 없습니다.")?;
                    gateway::rules::scope_manual_model_ids(&row.id,&mut model)?;
                    let keys=row.manual_model.as_ref()
                        .and_then(|value|value.get("assignmentOrder"))
                        .cloned().unwrap_or_else(||serde_json::json!([]));
                    let order=manual_workspace::native_order(
                        gateway.as_ref(),&row,&analysis.recognition
                    )?;
                    let result=gateway.execute(
                        gateway::ReadOperation::ManualClassification,
                        serde_json::json!({
                            "action":"preview_manual","order":order,
                            "model":model,"assignmentOrder":keys
                        })
                    )?;
                    let recipients=result.get("order")
                        .and_then(|order|order.get("recipients"))
                        .filter(|value|value.is_array())
                        .ok_or("저장된 수동 분류 적용 결과에 배송 정보가 없습니다.")?;
                    analysis.recognition["recipients"]=recipients.clone();
                    analysis.linked_spans=source_highlight::collect(
                        &source,&analysis.recognition
                    );
                    reception_service::refresh_remote_references(
                        gateway.as_ref(),&mut analysis
                    );
                }
                Ok(analysis)
            })();
            let _ = sender.send((request, result));
            context.request_repaint();
        });
    }

    fn clear_source_analysis(&mut self) {
        self.inline_validation_focus=None;
        self.guided_inline_target=None;
        self.pending_input_guidance=None;
        self.auto_client_change=None;
        self.pricing_basis_check=None;
        self.remote_recheck=None;
        self.remote_recheck_due=false;
        self.analysis_request = self.analysis_request.wrapping_add(1);
        self.analysis_receiver = None;
        self.analysis_running_key=None;
        self.analysis_pending = false;
        self.analysis = None;
        self.review_panel.clear();
        self.analysis_error = None;
        self.selected_recipient = 0;
        self.source_hover=None;
        self.link_hover.clear();
        self.analysis_due_at = None;
    }

    /// Run the EXISTING native Tauri change_order_client → merge → reprice
    /// → calculate_price rules after Rust title/raw matching, before marking
    /// an automatic dealer as applied. No UI-side billing or fake totals.
    fn start_auto_client_change(&mut self, client_id:&str, ctx:&egui::Context) {
        let duplicate=self.auto_client_change.as_ref().is_some_and(|pending|
            pending.order_id==self.active_id.as_deref().unwrap_or("") &&
            pending.client_id==client_id &&
            pending.request==self.analysis_request
        );
        if self.reception_job_admission(
            ReceptionJob::AutomaticClient,duplicate
        )!=JobAdmission::Start{return;}
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|draft|draft.id==id))
            .cloned()
        else {return;};
        if row.client_selection.as_deref()==Some("MANUAL") ||
           row.manual_model.is_some() ||
           row.analysis_raw.as_deref()!=Some(row.raw.as_str()) ||
           row.analysis_evidence.is_none() ||
           row.client_match.as_ref().and_then(|match_info|
               match_info.get("clientId")
           ).and_then(serde_json::Value::as_str)!=Some(client_id) {
            return;
        }
        let (sender,receiver)=mpsc::channel();
        let cancel=Arc::new(AtomicBool::new(false));
        self.auto_client_change=Some(PendingAutomaticClient {
            request:self.analysis_request,
            order_id:row.id.clone(),
            source_raw:row.raw.clone(),
            order_type:row.order_type.clone(),
            chat_handle:row.chat_handle.clone(),
            chat_title:row.chat_title.clone(),
            client_id:client_id.to_owned(),
            working_version:row.working_version,
            price_snapshot:row.pricing_preview.clone(),
            cancel:Arc::clone(&cancel),receiver,
        });
        let gateway=Arc::clone(&self.gateway);
        let target=client_id.to_owned();
        let context=ctx.clone();
        std::thread::spawn(move||{
            let result=native_order_change::prepare_cancellable(
                gateway.as_ref(),&row,&target,cancel.as_ref()
            );
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    // A valid unique match can be temporarily blocked by another reception
    // job or restored from a source-bound cache before it was applied.
    // Retry the same native application, not the advisory matcher.
    fn unapplied_auto_client_target(&self)->Option<String>{
        if self.analysis_pending || self.analysis_due_at.is_some() || self.analysis.is_none(){
            return None;
        }
        let row=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))?;
        if row.raw!=self.raw_source || row.order_type!=self.order_type.as_str()
            || row.raw.trim().is_empty() || row.client_id.is_some()
            || row.client_selection.as_deref()==Some("MANUAL") || row.manual_model.is_some()
            || row.analysis_raw.as_deref()!=Some(row.raw.as_str()) || row.analysis_evidence.is_none(){
            return None;
        }
        let matched=row.client_match.as_ref()?;
        if !matches!(matched.get("source").and_then(serde_json::Value::as_str),
            Some("KAKAO_TITLE"|"KAKAO_REMEMBERED"|"RAW_EXPLICIT")){
            return None;
        }
        matched.get("clientId").and_then(serde_json::Value::as_str)
            .filter(|id|!id.trim().is_empty()).map(str::to_owned)
    }

    fn retry_auto_client_change(&mut self,ctx:&egui::Context){
        if self.auto_client_change.is_some(){return;}
        if let Some(target)=self.unapplied_auto_client_target(){
            self.start_auto_client_change(&target,ctx);
            if self.auto_client_change.is_none(){
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    /// Async results are accepted only for the exact recognition request,
    /// working receipt, source, Kakao chat, proposed client and owner state.
    /// A manual selection in progress cancels any outstanding automatic edit.
    fn poll_auto_client_change(&mut self) {
        let received=self.auto_client_change.as_ref()
            .map(|pending|pending.receiver.try_recv());
        let result=match received {
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("거래처 자동 연결이 중단되었습니다.".into())),
            _=>None,
        };
        let Some(result)=result else {
            return;
        };
        let Some(pending)=self.auto_client_change.take() else{return;};
        let latest=self.drafts.iter().find(|row|row.id==pending.order_id);
        let valid=self.analysis_request==pending.request &&
            !self.analysis_pending &&
            self.active_id.as_deref()==Some(pending.order_id.as_str()) &&
            self.raw_source==pending.source_raw &&
            self.order_type.as_str()==pending.order_type &&
            self.client_change.is_none() &&
            latest.is_some_and(|row|
                row.raw==pending.source_raw &&
                row.order_type==pending.order_type &&
                row.chat_handle==pending.chat_handle &&
                row.chat_title==pending.chat_title &&
                row.client_selection.as_deref()!=Some("MANUAL") &&
                row.manual_model.is_none() &&
                row.working_version==pending.working_version &&
                row.pricing_preview==pending.price_snapshot &&
                row.analysis_raw.as_deref()==Some(row.raw.as_str()) &&
                row.analysis_evidence.is_some() &&
                row.client_match.as_ref().and_then(|match_info|
                    match_info.get("clientId")
                ).and_then(serde_json::Value::as_str)==Some(pending.client_id.as_str())
            );
        if !valid {return;}
        match result {
            Ok(result) if result.client_id==pending.client_id => {
                if let Some(row)=self.drafts.iter_mut().find(|row|
                    row.id==pending.order_id
                ){
                    row.client_id=Some(result.client_id);
                    row.client_selection=Some("AUTO".into());
                    row.pricing_preview=result.pricing_preview;
                    row.working_synced=false;
                }
                self.queue_source_save();
            }
            other => {
                // React treats failed automatic association as an unresolved
                // suggestion, not a blocking paste error. Keep recipient
                // recognition and Rust candidates; never show a fake dealer
                // assignment or a fabricated price.
                let detail=match other {
                    Ok(_)=>"거래처 자동 연결 결과가 요청과 다릅니다. 직접 선택하세요.".to_owned(),
                    Err(_)=>"거래처 자동 매칭에 실패했습니다. 직접 선택하세요.".to_owned(),
                };
                let failed=serde_json::json!({
                    "clientId":null,"source":"ERROR","detail":detail
                });
                if let Some(row)=self.drafts.iter_mut().find(|row|
                    row.id==pending.order_id
                ){
                    row.client_id=None;
                    row.client_selection=None;
                    row.pricing_preview=None;
                    row.client_match=Some(failed.clone());
                }
                if let Some(analysis)=self.analysis.as_mut(){
                    analysis.client_match=failed;
                    analysis.matched_client_name=None;
                    analysis.chosen_client_name=None;
                }
                self.queue_source_save();
            }
        }
    }

    fn poll_source_analysis(&mut self,ctx:&egui::Context) {
        let complete = self.analysis_receiver.as_ref()
            .map(|receiver| receiver.try_recv());
        if let Some(Err(mpsc::TryRecvError::Disconnected)) = complete {
            self.analysis_receiver = None;
            self.analysis_running_key=None;
            self.analysis_pending = false;
            self.analysis_error = Some("발주서 분석이 중단되었습니다.".to_owned());
            return;
        }
        if let Some(Ok((request, result))) = complete {
            self.analysis_receiver = None;
            let started=self.analysis_running_key.take();
            let is_current=started.as_ref().is_some_and(|(id,raw,kind)|
                self.active_id.as_deref()==Some(id.as_str()) &&
                self.raw_source==*raw &&
                self.order_type.as_str()==kind.as_str()
            );
            if request != self.analysis_request || !is_current {
                // A changed source/receipt may never inherit an earlier OCR
                // or auto-match response, even if the old worker won the race.
                self.analysis_pending=false;
                if !self.raw_source.trim().is_empty(){
                    self.analysis_due_at=Some(Instant::now());
                    ctx.request_repaint();
                }
                return;
            }
            self.analysis_pending = false;
            match result {
                Ok(mut analysis) => {
                    let original_evidence=analysis.recognition.clone();
                    if let Some(order_id)=self.active_id.as_deref(){
                        if let Err(error)=analysis.scope_display(order_id,&self.raw_source){
                            self.analysis=None;
                            self.analysis_error=Some(error);
                            return;
                        }
                    }
                    // Attach the exact Rust analysis to ONLY the still-active
                    // reception. A switched/new order invalidates this worker.
                    let mut changed_snapshot = false;
                    let mut auto_target:Option<String>=None;
                    let mut requote_manual:Option<String>=None;
                    if let Some(active_id) = self.active_id.as_deref() {
                        if let Some(item) = self.drafts.iter_mut()
                            .find(|item| item.id == active_id) {
                            // An employee-corrected manual model can be
                            // projected onto the new read result. The immutable
                            // first recognition is kept separately for audit.
                            let first_for_source=item.analysis_raw.as_deref()
                                !=Some(self.raw_source.as_str())
                                || item.analysis_evidence.is_none();
                            if first_for_source {
                                item.analysis_evidence=Some(original_evidence);
                            }
                            item.analysis_raw = Some(self.raw_source.clone());
                            item.client_match = Some(analysis.client_match.clone());
                            if item.client_selection.as_deref()==Some("MANUAL") &&
                                !self.gateway.isolated_client_identity_link() &&
                                !item.pricing_preview.as_ref()
                                    .is_some_and(complete_reception_quote) {
                                requote_manual=item.client_id.clone();
                            }
                            if item.client_selection.as_deref() != Some("MANUAL") &&
                               item.manual_model.is_none() {
                                // Do NOT treat an advisory match as a completed
                                // Rust order edit. Existing matching approved
                                // pricing is retained; new matches remain
                                // unassigned until the native quote succeeds.
                                let proposal=analysis.client_match.get("clientId")
                                    .and_then(serde_json::Value::as_str);
                                let same_auto=item.client_selection.as_deref()==Some("AUTO")
                                    && item.client_id.as_deref()==proposal
                                    && (item.pricing_preview.as_ref()
                                        .is_some_and(complete_reception_quote)
                                        || self.gateway.isolated_client_identity_link());
                                if !same_auto {
                                    item.client_id=None;
                                    item.client_selection=None;
                                    item.pricing_preview=None;
                                    item.working_synced=false;
                                    auto_target=proposal.map(str::to_owned);
                                }
                            }
                            changed_snapshot = true;
                        }
                    }
                    self.analysis = Some(analysis);
                    self.analysis_error = None;
                    if changed_snapshot {
                        self.queue_source_save();
                    }
                    self.selected_recipient = 0;
                    self.apply_requested_recipient_focus(ctx);
                    if let Some(target)=auto_target {
                        self.start_auto_client_change(&target,ctx);
                    }
                    // Historical unpriced/manual receipts are recalculated
                    // once after their current recognition snapshot is ready.
                    if let Some(target)=requote_manual {
                        self.apply_manual_client(&target,ctx);
                    }
                }
                Err(error) => {
                    self.analysis = None;
                    self.analysis_error = Some(error);
                }
            }
        }
    }

    fn current_intake_chat(&self)->Option<ChatWindow>{
        if !self.kakao_dock.connected(){return None;}
        // Resolve before minimizing, while the original Kakao focus
        // association is still available; never guess among multiple chats.
        let focused=self.kakao_tracker.recent_focus()
            .and_then(|handle|self.kakao_tracker.verified(handle)).cloned();
        focused.or_else(||{
            let chats=self.kakao_tracker.chats();
            if chats.len()==1{chats.first().cloned()}else{None}
        })
    }

    /// The capture action does NOT depend on ClipboardMonitor::kind().
    /// Select a new desktop rectangle first, then route the captured RGBA
    /// through the same PP-OCRv5 and employee approval flow as pasted images.
    fn begin_region_capture(&mut self, order_type:OrderType,ctx:egui::Context){
        match self.reception_job_admission(
            ReceptionJob::SourceIntake,self.clipboard_busy
        ){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::SourceIntake,blocker);
                return;
            }
        }
        self.clipboard_chat=self.current_intake_chat();
        self.clipboard_busy=true;
        self.ocr_processing=true;
        self.clipboard_error=None;
        self.clipboard_cancel.store(false,Ordering::Release);
        let cancelled=Arc::clone(&self.clipboard_cancel);
        let (tx,rx)=mpsc::channel();
        let (preview_tx,preview_rx)=mpsc::channel();
        self.ocr_preview_receiver=Some(preview_rx);
        self.ocr_processing_preview=None;
        self.clipboard_receiver=Some(rx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        let worker_context=ctx.clone();
        std::thread::spawn(move||{
            let capture={
                let _restore=RegionCaptureRestore(worker_context.clone());
                // The app's minimize command is applied asynchronously.
                // Do not start the Windows snip overlay before it is hidden.
                std::thread::sleep(Duration::from_millis(250));
                clipboard_intake::capture_screen_region(&cancelled)
            };
            let result=capture.and_then(|(image,sequence)|{
                if cancelled.load(Ordering::Acquire){
                    return Err("OCR_CANCELLED".into());
                }
                let _=preview_tx.send(
                    clipboard_intake::preview_thumbnail(&image,640)
                );
                // Let the user see the captured thumbnail immediately;
                // PaddleOCR may continue for significantly longer.
                worker_context.request_repaint();
                let text=clipboard_intake::recognize_image(&image,&cancelled)?;
                Ok(ClipboardReceipt::Ocr{text,image,sequence})
            });
            let _=tx.send((order_type,result));
            worker_context.request_repaint();
        });
    }

    fn begin_clipboard_intake(&mut self, order_type: OrderType, ctx: egui::Context) {
        if !self.clipboard_monitor.kind().has_intake(){return;}
        match self.reception_job_admission(
            ReceptionJob::SourceIntake,self.clipboard_busy
        ){
            JobAdmission::Start=>{},
            JobAdmission::Duplicate=>return,
            JobAdmission::Conflict(blocker)=>{
                self.reception_job_conflict(ReceptionJob::SourceIntake,blocker);
                return;
            }
        }
        self.clipboard_chat=self.current_intake_chat();
        self.clipboard_busy = true;
        self.ocr_processing =
            self.clipboard_monitor.kind() == clipboard_intake::ClipboardKind::Image;
        self.clipboard_error = None;
        self.clipboard_cancel.store(false, Ordering::Release);
        let cancelled = Arc::clone(&self.clipboard_cancel);
        let (tx, rx) = mpsc::channel();
        let (preview_tx,preview_rx)=mpsc::channel();
        self.ocr_preview_receiver=Some(preview_rx);
        self.ocr_processing_preview=None;
        self.clipboard_receiver = Some(rx);
        // Clipboard image decoding and PaddleOCR never block the UI thread.
        std::thread::spawn(move || {
            let result=clipboard_intake::read_with_sequence()
                .and_then(|(value,sequence)|match value{
                    ClipboardPayload::Text(text)=>
                        Ok(ClipboardReceipt::Text{text,sequence}),
                    ClipboardPayload::Image(image)=>{
                        // Send the small preview first; OCR and the approval
                        // workflow continue using the unchanged original.
                        let _=preview_tx.send(
                            clipboard_intake::preview_thumbnail(&image,640)
                        );
                        // The preview has arrived even if native OCR is busy.
                        ctx.request_repaint();
                        let text=clipboard_intake::recognize_image(&image,&cancelled)?;
                        Ok(ClipboardReceipt::Ocr{text,image,sequence})
                    }
                });
            let _ = tx.send((order_type, result));
            ctx.request_repaint();
        });
    }

    fn accept_clipboard_text(&mut self, text: String, order_type: OrderType,
        chat: Option<ChatWindow>, sequence:Option<u32>, ctx: &egui::Context) {
        if text.trim().is_empty() {
            self.clipboard_error = Some("발주 원문이 비어 있습니다.".into());
            return;
        }
        if self.raw_source != text && !self.raw_source.trim().is_empty() {
            // Exact replacement confirmation from the existing React intake.
            self.pending_replace = Some(PendingReplace {
                text, order_type, chat, input_method: "TEXT_PASTE",
                clipboard_sequence:sequence, ocr_original:None,
                ocr_learning_used:false, ocr_model_suggestion:None,
            });
            return;
        }
        self.apply_intake_text(
            text,order_type,chat,ctx,"TEXT_PASTE",false,sequence,
            None,false,None
        );
    }

    fn apply_intake_text(&mut self, text: String, order_type: OrderType,
        chat: Option<ChatWindow>, ctx: &egui::Context,
        input_method: &str, replace_original: bool,
        clipboard_sequence:Option<u32>,ocr_original:Option<String>,
        learning_used:bool,model_suggestion:Option<String>) {
        if text.trim().is_empty() { return; }
        if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
            ReceptionJob::SourceReplace,false
        ){
            self.reception_job_conflict(ReceptionJob::SourceReplace,blocker);
            return;
        }
        if self.active_id.is_none() {
            self.new_reception(ctx);
        }
        let Some(receipt_id)=self.active_id.clone() else {
            self.clipboard_error=Some("OCR 정정 이력을 연결할 접수 건이 없습니다.".into());
            return;
        };
        // Persist the FULL initial recognition and FULL staff-approved text.
        // Corrections are a retrospective corpus, never an auto-rewrite rule.
        let feedback=ocr_original.map(|original_text|OcrFeedback {
            id:uuid::Uuid::new_v4().to_string(),
            order_id:receipt_id,
            // Set from staff-confirmed dealer association by the serialized
            // source-save worker; never infer the dealer from fragmented OCR.
            client_id:None,
            original_text,
            corrected_text:text.clone(),
            // Audit whether learned analysis was enabled. Collection of
            // approved OCR correction pairs is unconditional.
            learning_used,
            model_suggestion,
        });
        let first_content = self.raw_source.trim().is_empty();
        let changed = self.raw_source != text || self.order_type != order_type;
        if changed {
            if let Some(active)=self.active_id.as_deref()
                .and_then(|id|self.drafts.iter_mut().find(|row|row.id==id))
            {
                if first_content || replace_original {
                    active.source_input_method = Some(input_method.to_owned());
                }
                active.last_input_method = Some(input_method.to_owned());
            }
        }
        self.raw_source = text;
        self.order_type = order_type;
        self.source_editing = true;
        self.direct_source_entry = false;
        if let Some(chat) = chat {
            self.bind_chat_to_active(chat, ctx);
        }
        self.clipboard_error = None;
        if changed {
            self.clear_source_analysis();
            self.analysis_due_at = Some(Instant::now() + Duration::from_millis(250));
            ctx.request_repaint_after(Duration::from_millis(260));
        }
        // Pair the audit with the same worker batch as the approved source.
        // A failed source save must not produce an accepted OCR audit.
        self.queue_source_save_with_feedback(feedback);
        // A successful enqueue is not a durable reception. Only clear
        // after source_autosave explicitly acknowledges this exact snapshot.
        self.pending_clipboard_clear=if self.source_save_error.is_none(){
            self.active_id.as_ref().map(|id|PendingClipboardClear{
                order_id:id.clone(),
                raw:self.raw_source.clone(),
                order_type:self.order_type.as_str().to_owned(),
                sequence:clipboard_sequence,
            })
        }else{None};
    }

    fn poll_clipboard_intake(&mut self, ctx: &egui::Context) {
        let preview=self.ocr_preview_receiver.as_ref()
            .map(|rx|rx.try_recv());
        match preview{
            Some(Ok(image))=>{
                self.ocr_preview_receiver=None;
                if self.clipboard_busy && !self.clipboard_cancel.load(Ordering::Acquire){
                    // The actual decoded intake, not an advisory clipboard
                    // format snapshot, owns OCR mode when a copy raced a click.
                    self.ocr_processing=true;
                    let texture=ctx.load_texture(
                        "source-ocr-processing-thumbnail",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width,image.height],&image.rgba
                        ),
                        egui::TextureOptions::LINEAR
                    );
                    self.ocr_processing_preview=Some((
                        texture,image.width,image.height
                    ));
                    ctx.request_repaint();
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.ocr_preview_receiver=None;
            }
            _=>{}
        }
        let next = self.clipboard_receiver.as_ref().map(|rx| rx.try_recv());
        match next {
            Some(Ok((order_type, result))) => {
                self.clipboard_receiver = None;
                self.clipboard_busy = false;
                self.ocr_processing = false;
                self.ocr_preview_receiver=None;
                self.ocr_processing_preview=None;
                let intake_chat = self.clipboard_chat.take();
                if self.clipboard_cancel.swap(false, Ordering::AcqRel) {
                    // User cancellation wins even if a completed OCR result
                    // raced with the Cancel click. Never open the review.
                    return;
                }
                match result {
                    Ok(ClipboardReceipt::Text{text,sequence}) => {
                        self.accept_clipboard_text(
                            text,order_type,intake_chat,sequence,ctx
                        );
                    }
                    Ok(ClipboardReceipt::Ocr{text,image,sequence}) => {
                        if text.trim().is_empty() {
                            self.clipboard_error=Some(
                                "이미지에서 문자를 인식하지 못했습니다.".into()
                            );
                            return;
                        }
                        if text.chars().count()>20_000 {
                            self.clipboard_error=Some(
                                "발주 원문은 최대 20,000자까지 입력할 수 있습니다.".into()
                            );
                            return;
                        }
                        // Wait for manual comparison and approval. No write yet.
                        let texture=ctx.load_texture(
                            "clipboard-ocr-review-preview",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width,image.height],&image.rgba
                            ),
                            egui::TextureOptions::LINEAR
                        );
                        self.ocr_review=Some(OcrReview {
                            order_type,
                            original_text:text.clone(),
                            edited_text:text,
                            texture,width:image.width,height:image.height,
                            zoom:1.0,chat:intake_chat,
                            clipboard_sequence:sequence,
                            learned_analysis_enabled:false,
                            model_receiver:None,
                            model_suggestion:None,
                            model_error:None,
                        });
                        // The decoded texture was uploaded on this UI frame.
                        // Request another frame explicitly so the first OCR
                        // comparison cannot remain a blank, stale surface.
                        ctx.request_repaint();
                    }
                    Err(error) if error == "OCR_CANCELLED" => {}
                    Err(error) => {
                        self.clipboard_error = Some(error);
                        // Existing '알림' button opens details on explicit click;
                        // receiving an error must not spawn a surprise window.
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.clipboard_receiver = None;
                self.clipboard_busy = false;
                self.ocr_processing = false;
                self.ocr_preview_receiver=None;
                self.ocr_processing_preview=None;
                self.clipboard_chat = None;
                if self.clipboard_cancel.swap(false, Ordering::AcqRel) {
                    return;
                }
                self.clipboard_error = Some("클립보드 접수 작업이 중단되었습니다.".into());
                // Keep the approved header notification affordance; no
                // automatic floating alert on failure.
            }
            _ => {}
        }
    }

    /// OCR never changes an order until the employee checks the original
    /// image against the editable recognition result and explicitly applies.
    /// OCR review owns a fixed-size, non-scrolling window. Its image and
    /// editor have separately clipped viewports; even 20,000 characters or
    /// 400% zoom must never move the title, pane borders or Apply button.
    fn show_ocr_review(&mut self, ctx: &egui::Context, work_area:Rect) {
        let mut close=false;
        let mut apply:Option<(
            String,OrderType,Option<ChatWindow>,Option<u32>,String,bool,Option<String>
        )>=None;
        // Never guess dealer identity from fragmented table OCR. If an
        // employee already verified a dealer, use that exact template cohort.
        // Otherwise the experimental learner requires a unique matching
        // layout signature before offering any structural changes.
        let verified_template_client=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
            .filter(|row|row.client_selection.as_deref()==Some("MANUAL"))
            .and_then(|row|row.client_id.clone());
        // Receive optional model suggestions without reading SQLite or
        // running pattern learning on the egui UI thread.
        if let Some(review)=self.ocr_review.as_mut(){
            if let Some(outcome)=review.model_receiver.as_ref().map(|rx|rx.try_recv()){
                match outcome {
                    Ok(Ok(suggestion))=>{
                        if review.learned_analysis_enabled && suggestion.replacements>0
                            && review.edited_text==review.original_text {
                            review.edited_text=suggestion.text.clone();
                            review.model_suggestion=Some(suggestion.text);
                        }
                        review.model_error=None;
                        review.model_receiver=None;
                    }
                    Ok(Err(error))=>{
                        review.model_error=Some(format!(
                            "학습 모델을 읽지 못해 기본 OCR 모드를 유지합니다. {}",
                            user_error::display("reception.ocr_learning",&error)
                        ));
                        review.model_receiver=None;
                    }
                    Err(mpsc::TryRecvError::Disconnected)=>{
                        review.model_error=Some(
                            "학습 모델을 읽지 못해 기본 OCR 모드를 유지합니다.".into()
                        );
                        review.model_receiver=None;
                    }
                    Err(mpsc::TryRecvError::Empty)=>{}
                }
            }
        }
        // The KakaoTalk dock is an externally owned foreground window.
        // Like every other modal, OCR must be centered inside the remaining
        // approved work area and must never overlap the KakaoTalk reserve.
        let dialog_rect=crate::popup::modal_ocr_review_rect(work_area);
        let dialog_size=dialog_rect.size();
        if let Some(review)=self.ocr_review.as_mut() {
            crate::popup::modal_window(ctx,
                "OCR 문자 인식 검토","egui-ocr-review",work_area
            )
                .pivot(egui::Align2::LEFT_TOP)
                .fixed_pos(dialog_rect.min)
                .fixed_size(dialog_size)
                .scroll([false,false])
                .show(ctx,|ui|{
                    floating_notice::section(ctx,"reception.ocr_learning",dialog_rect);
                    // Paint clips are NOT layout measurements. During egui's
                    // window sizing pass an empty clip can collapse the body
                    // to 1pt, causing a -26pt image viewport and a panic.
                    let body_size=crate::popup::modal_master_content_from_ui(
                        ui.available_rect_before_wrap(),dialog_size
                    );
                    if !crate::popup::modal_master_ocr_body_is_usable(body_size){
                        // Do not construct split viewports from tiny sizing
                        // frames. The next layout pass will have valid space.
                        ui.ctx().request_repaint();
                        return;
                    }
                    let (bounds,_)=ui.allocate_exact_size(
                        body_size,Sense::hover()
                    );
                    // The enclosing Window owns only the single allocated
                    // content rect above. Painting the split master inside an
                    // independent child prevents its positioned widgets from
                    // expanding Window::min_rect past the fixed lower edge.
                    let mut master_ui=crate::popup::modal_master_overlay_ui(
                        ui,bounds,"ocr-review-fixed-master"
                    );
                    let master=crate::popup::modal_master_layout(
                        bounds,
                        theme::MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT,
                        theme::MODAL_FOOTER_HEIGHT,
                        theme::MODAL_MASTER_WORKSPACE_GAP
                    );
                    // The OCR master paints all three chrome regions on the
                    // same device-pixel grid. Fractional work-area sizes must
                    // not introduce fuzzy header rules or rounded corners.
                    let ppp=master_ui.ctx().pixels_per_point();
                    let header_rect=crate::popup::modal_master_pixel_aligned_rect(
                        master.header,ppp
                    );
                    let footer_rect=crate::popup::modal_master_pixel_aligned_rect(
                        master.footer,ppp
                    );
                    let compare_rect=crate::popup::modal_master_pixel_aligned_rect(
                        master.workspace,ppp
                    );
                    close=crate::popup::modal_master_header_in_rect(
                        &mut master_ui,header_rect,"OCR 문자 인식 검토",
                        "원본 이미지와 인식된 텍스트를 대조하고 오타를 수정한 뒤 반영하세요.",
                        "취소",true
                    );

                    // Common popup master owns the ONE workspace frame and
                    // both divider lines. The OCR pane ratio is content-specific.
                    let frame=crate::popup::modal_master_workspace_background(
                        &master_ui,compare_rect,theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT,
                        Some(2.0/3.0)
                    );
                    let divider_x=frame.divider_x.expect("OCR has two panes");
                    let toolbar_bottom=frame.toolbar_bottom;
                    let edge=theme::DIVIDER;

                    let inset=theme::MODAL_MASTER_CONTENT_INSET;
                    let left_header=Rect::from_min_size(
                        egui::pos2(compare_rect.left()+inset,
                            compare_rect.top()+theme::SPACE_4),
                        Vec2::new((divider_x-compare_rect.left()-2.0*inset)
                            .max(1.0),theme::ACTION_STANDARD_HEIGHT)
                    );
                    let right_header=Rect::from_min_size(
                        egui::pos2(divider_x+inset,
                            compare_rect.top()+theme::SPACE_4),
                        Vec2::new((compare_rect.right()-divider_x-2.0*inset)
                            .max(1.0),theme::ACTION_STANDARD_HEIGHT)
                    );
                    let left_clip=left_header.intersect(master_ui.clip_rect());
                    master_ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-review-image-toolbar")
                            .max_rect(left_header)
                            .layout(Layout::left_to_right(Align::Center)),
                        |toolbar|{
                            toolbar.set_clip_rect(left_clip);
                            toolbar.apply_shared_width(crate::card::CardWidthPolicy::Fixed,left_header.width());
                            toolbar.label(
                                typography::subsection_heading("원본 이미지")
                            );
                            toolbar.with_layout(
                                Layout::right_to_left(Align::Center),
                                |zoom|{
                                    zoom.add_sized(
                                        Vec2::new(54.0,24.0),
                                        egui::Label::new(
                                            typography::ocr_zoom_label(review.zoom)
                                        )
                                    );
                                    crate::input::ocr_zoom_slider(zoom,&mut review.zoom);
                                }
                            );
                        },
                    );
                    let right_clip=right_header.intersect(master_ui.clip_rect());
                    master_ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-review-text-toolbar")
                            .max_rect(right_header)
                            .layout(Layout::left_to_right(Align::Center)),
                        |toolbar|{
                            toolbar.set_clip_rect(right_clip);
                            toolbar.apply_shared_width(crate::card::CardWidthPolicy::Fixed,right_header.width());
                            // Keep the section heading LEFT and put the
                            // experimental option in a distinct RIGHT-anchored
                            // slot. The checkbox never consumes the heading's
                            // leading edge or gets pushed outside its clip.
                            toolbar.add(crate::ui_primitives::truncated_label(
                                typography::subsection_heading("인식된 텍스트 · 직접 수정")
                            ));
                            let mut changed=false;
                            toolbar.with_layout(
                                Layout::right_to_left(Align::Center),
                                |option|{
                                    let response=crate::checkbox::checkbox(
                                        option,
                                        &mut review.learned_analysis_enabled,
                                        "학습된 분석 사용 (시험 기능)",true
                                    );
                                    floating_notice::clicked(option.ctx(),"reception.ocr_learning",&response);
                                    changed=response.changed();
                                },
                            );
                            if changed {
                                review.model_error=None;
                                if review.learned_analysis_enabled {
                                    let initial=review.original_text.clone();
                                    let template_client=verified_template_client.clone();
                                    let (tx,rx)=mpsc::channel();
                                    review.model_receiver=Some(rx);
                                    let repaint=toolbar.ctx().clone();
                                    std::thread::spawn(move||{
                                        let outcome=(||{
                                            let spellings=preview_store::PreviewStore
                                                ::list_ocr_feedback(1000)?;
                                            // Two independent learned domains:
                                            // (1) repeated OCR mistakes across
                                            // verified staff corrections;
                                            // (2) form layout examples ONLY
                                            // from a manually confirmed client.
                                            // Both corpora continue recording
                                            // regardless of this analysis toggle.
                                            let layouts=if let Some(client_id)=
                                                template_client.as_deref(){
                                                preview_store::PreviewStore
                                                    ::list_ocr_layout_examples_for_client(
                                                        client_id,1000
                                                    )?
                                            }else{
                                                Vec::new()
                                            };
                                            Ok::<_,String>(ocr_learning::suggest_with_layout(
                                                &initial,&spellings,&layouts,
                                                template_client.as_deref()
                                            ))
                                        })();
                                        let _=tx.send(outcome);
                                        repaint.request_repaint();
                                    });
                                }else{
                                    review.model_receiver=None;
                                    if review.model_suggestion.as_ref()
                                        ==Some(&review.edited_text){
                                        review.edited_text=review.original_text.clone();
                                    }
                                    review.model_suggestion=None;
                                }
                            }
                        },
                    );

                    // One contiguous work surface: the dark document mat and
                    // editable text extend directly from the shared toolbar
                    // rule to the bottom outer border. The old 16pt outer
                    // inset and second viewer outline created double gutters
                    // ("creases") around BOTH panes. Keep the 16pt spacing
                    // only INSIDE the document viewer; the TextEdit already
                    // owns its 12pt interior text margin.
                    let viewport_top=toolbar_bottom
                        +theme::SOFT_DIVIDER_WIDTH;
                    let viewport_bottom=compare_rect.bottom()
                        -theme::FRAME_STROKE_WIDTH;
                    let image_view_rect=Rect::from_min_max(
                        egui::pos2(compare_rect.left()
                            +theme::FRAME_STROKE_WIDTH,viewport_top),
                        egui::pos2(divider_x,viewport_bottom)
                    );
                    let text_view_rect=Rect::from_min_max(
                        egui::pos2(divider_x+theme::FRAME_STROKE_WIDTH,viewport_top),
                        egui::pos2(compare_rect.right()
                            -theme::FRAME_STROKE_WIDTH,viewport_bottom)
                    );
                    // Colors identify the two uses of ONE contiguous area:
                    // no separate nested cards, strokes or rounded corners.
                    // Lower pane corners follow the ONE outer 6pt workspace
                    // radius. The 1pt inset belongs to the final stroke,
                    // not a second card border or a square background leak.
                    let inner_radius=theme::CONTROL_RADIUS.saturating_sub(
                        theme::FRAME_STROKE_WIDTH.ceil() as u8
                    );
                    master_ui.painter().rect_filled(
                        image_view_rect,
                        egui::CornerRadius{
                            nw:0,ne:0,sw:inner_radius,se:0
                        },
                        theme::CANVAS
                    );
                    master_ui.painter().rect_filled(
                        text_view_rect,
                        egui::CornerRadius{
                            nw:0,ne:0,sw:0,se:inner_radius
                        },
                        theme::INPUT
                    );

                    // Inset the entire scroll viewport by 12pt. Keeping
                    // the matte OUTSIDE the scrolling area preserves a
                    // visible dark border even when the source is zoomed or
                    // panned to an image edge. No nested card or frame.
                    let image_scroll_rect=image_view_rect.shrink(theme::SPACE_12);
                    let image_clip=image_scroll_rect.intersect(master_ui.clip_rect());
                    master_ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-image-fixed-viewport")
                            .max_rect(image_scroll_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |viewport|{
                            viewport.set_clip_rect(image_clip);
                            viewport.apply_shared_width(crate::card::CardWidthPolicy::Fixed,image_scroll_rect.width());
                            viewport.set_height(image_scroll_rect.height());
                            let image_scroll=crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Both)
                                .id_salt("ocr-original-image-scroll")
                                .max_width(image_scroll_rect.width())
                                .max_height(image_scroll_rect.height())
                                .auto_shrink([false,false])
                                .scroll_source(
                                    egui::scroll_area::ScrollSource::SCROLL_BAR
                                    | egui::scroll_area::ScrollSource::DRAG
                                )
                                .show(viewport,|image_ui|{
                                    // 100% = width fit within the inset mat,
                                    // not 100% of the image's pixel dimensions.
                                    let fit=(image_scroll_rect.width()
                                        /review.width as f32)
                                        .min(1.0).max(0.01);
                                    let scale=fit*review.zoom;
                                    let display=Vec2::new(
                                        review.width as f32*scale,
                                        review.height as f32*scale
                                    );
                                    // Center only when narrower than the
                                    // viewport; larger images retain normal
                                    // independent horizontal scrolling.
                                    let left_pad=((image_scroll_rect.width()
                                        -display.x)/2.0).max(0.0);
                                    image_ui.horizontal(|line|{
                                        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ExplicitHorizontal);
                                        if left_pad>0.0 {
                                            line.add_space(left_pad);
                                        }
                                        let image=line.image((
                                            review.texture.id(),display
                                        ));
                                        // The stroke belongs to the actual
                                        // source boundary and moves with it.
                                        theme::paint_surface_outline(line.painter(),image.rect,0.0,edge);
                                    });
                                });
                            let wheel=viewport.input(|input|{
                                if input.pointer.hover_pos().is_some_and(|pos|
                                    image_scroll.inner_rect.contains(pos)
                                ) {input.raw_scroll_delta.y} else {0.0}
                            });
                            if wheel!=0.0 {
                                review.zoom=(review.zoom
                                    *(wheel*0.0018_f32).exp()).clamp(0.5,4.0);
                                viewport.ctx().request_repaint();
                            }
                        },
                    );
                    let text_clip=text_view_rect.intersect(master_ui.clip_rect());
                    master_ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-text-fixed-viewport")
                            .max_rect(text_view_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |editor|{
                            editor.set_clip_rect(text_clip);
                            editor.apply_shared_width(crate::card::CardWidthPolicy::Fixed,text_view_rect.width());
                            editor.set_height(text_view_rect.height());
                            crate::input::multiline_text_field_sized(
                                editor,
                                &mut review.edited_text,
                                true,
                                text_view_rect.size(),
                                typography::ocr_review_font(),
                                false,
                                egui::Margin::same(theme::SPACE_12 as i8),
                                1
                            );
                        },
                    );

                    // The final master pass sits above both pane fills.
                    // Its flat, pixel-snapped dividers and single rounded
                    // outline repair the tiny overhangs and corner seams.
                    crate::popup::modal_master_workspace_finish(
                        &master_ui,compare_rect,frame
                    );

                    let edited=review.edited_text.trim();
                    let valid=!edited.is_empty() && edited.chars().count()<=20_000;
                    crate::popup::modal_master_footer_in_rect(
                        &mut master_ui,footer_rect,|buttons|{
                            if crate::button::action(
                                buttons,"적용",ButtonRole::Primary,valid
                            ).clicked() {
                                apply=Some((
                                    edited.to_owned(),
                                    review.order_type,
                                    review.chat.clone(),
                                    review.clipboard_sequence,
                                    review.original_text.clone(),
                                    review.learned_analysis_enabled,
                                    review.model_suggestion.clone()
                                ));
                            }
                        }
                    );
                    if !valid {
                        let hint_rect=Rect::from_min_size(
                            egui::pos2(footer_rect.left()+theme::SPACE_12,
                                footer_rect.top()
                                    +(theme::MODAL_FOOTER_HEIGHT-22.0)/2.0),
                            Vec2::new((footer_rect.width()-130.0).max(1.0),22.0)
                        );
                        let clip=hint_rect.intersect(master_ui.clip_rect());
                        master_ui.scope_builder(
                            UiBuilder::new().id_salt("ocr-review-footer-error")
                                .max_rect(hint_rect),
                            |hint|{
                                hint.set_clip_rect(clip);
                                hint.label(typography::ocr_validation_hint(
                                    "인식된 텍스트는 1~20,000자로 수정해 주세요."
                                ));
                            },
                        );
                    }
                });
        }
        if let Some((raw,kind,chat,sequence,original,learned,suggestion))=apply {
            if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
                ReceptionJob::SourceReplace,false
            ){
                // Retain the verified OCR draft on a rejected approval.
                // Losing corrections here forces a complete OCR retry.
                self.reception_job_conflict(ReceptionJob::SourceReplace,blocker);
            }else{
            self.ocr_review=None;
            self.discard_ocr_confirm=false;
            if !self.raw_source.trim().is_empty() && self.raw_source!=raw {
                self.pending_replace=Some(PendingReplace {
                    text:raw,order_type:kind,chat,input_method:"OCR",
                    clipboard_sequence:sequence,
                    ocr_original:Some(original),
                    ocr_learning_used:learned,
                    ocr_model_suggestion:suggestion,
                });
            } else {
                self.apply_intake_text(
                    raw,kind,chat,ctx,"OCR",true,sequence,
                    Some(original),learned,suggestion
                );
            }
            }
        } else if close {
            if self.ocr_review.as_ref().is_some_and(|review|
                review.edited_text!=review.original_text
            ) {
                self.discard_ocr_confirm=true;
            } else {
                self.ocr_review=None;
                self.discard_ocr_confirm=false;
            }
        }
        if self.discard_ocr_confirm {
            crate::popup::modal_backdrop(
                ctx,"ocr-discard-nested-backdrop",true
            );
            match crate::popup::show_discard_confirmation(
                ctx,work_area,"ocr-review-discard-confirm","변경사항 확인",
                "OCR 검토에서 수정한 내용을 버릴까요?","돌아가기","수정 내용 버리기",true
            ){
                crate::popup::ChangeDecision::Cancel=>self.discard_ocr_confirm=false,
                crate::popup::ChangeDecision::Discard=>{
                    self.ocr_review=None;
                    self.discard_ocr_confirm=false;
                }
                _=>{}
            }
        }
    }

    /// Commit an approved original replacement only when safe to mutate.
    /// Failed admission keeps the confirmation and its full OCR audit intact.
    fn approve_pending_replace(&mut self,ctx:&egui::Context)->bool{
        if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
            ReceptionJob::SourceReplace,false
        ){
            self.reception_job_conflict(ReceptionJob::SourceReplace,blocker);
            return false;
        }
        let Some(next)=self.pending_replace.take() else{return false};
        self.apply_intake_text(
            next.text,next.order_type,next.chat,ctx,
            next.input_method,true,next.clipboard_sequence,
            next.ocr_original,next.ocr_learning_used,
            next.ocr_model_suggestion
        );
        true
    }

    fn show_replacement_confirmation(&mut self, ctx: &egui::Context, work_area:Rect) {
        if self.pending_replace.is_none() { return; }
        let mut accept = false;
        let mut cancel = false;
        crate::popup::show_decision_window(ctx,"발주원문 교체 확인","egui-replace-source-confirm",work_area,|ui|{
                crate::popup::decision_workspace(ui,|decision|{
                    cancel=decision.header("발주원문 교체 확인","취소",true
                    );
                    decision.body(|ui|{
                        ui.label("현재 초안과 분석 결과를 새 원문으로 교체할까요?");
                    });
                    decision.actions(|actions|{
                        if crate::button::action(actions, "교체", ButtonRole::Primary, true).clicked() {
                            accept = true;
                        }
                    });
                });
            });
        if accept {
            // On a write conflict, preserve the pending text and confirmation.
            self.approve_pending_replace(ctx);
        } else if cancel {
            self.pending_replace = None;
        }
    }

    fn handle_header_drag(
        ctx: &egui::Context,
        response: &egui::Response,
        maximized: bool,
    ) {
        if response.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if response.double_clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
    }

    fn sync_navigation_snapshot(&mut self,ctx:&egui::Context,now:Instant){
        crate::scrollbar::sweep_snapshots(ctx,now);
        crate::category_dropdown::sweep_popup_states(ctx,now);
        crate::view_state::sweep_focus(ctx,now);
        let previous=self.navigation_snapshots.active();
        if previous!=self.page as usize{
            match Page::ALL[previous]{
                Page::Products=>if let Some(m)=self.product_manager.as_mut(){m.suspend_navigation(now);},
                Page::Freight=>if let Some(m)=self.freight_manager.as_mut(){m.suspend_navigation(now);},
                Page::Excel=>if let Some(m)=self.excel_manager.as_mut(){m.suspend_navigation(now);},
                Page::Clients=>if let Some(m)=self.client_manager.as_mut(){m.suspend_navigation(now);},
                Page::Settings=>if let Some(m)=self.settings_view.as_mut(){m.suspend_navigation(now);},
                _=>{}
            }
        }
        // Re-entering reception is a business refresh boundary independent
        // of presentation snapshot expiry. A retained draft may have been
        // opened before its catalog price or freight tariff was registered.
        if previous!=self.page as usize && self.page==Page::Reception{
            self.start_pricing_basis_check(ctx);
        }
        if !self.navigation_snapshots.visit(self.page as usize,now){return;}
        match self.page{
            Page::Reception=>{
                // Preserve active sources, editor drafts and all pending jobs.
                self.workflow_ui.stop_preview();
                self.workflow_ui.folded=None;
                if self.active_id.is_none()&&self.workflow_action.is_none()
                    &&!self.workflow_read.required()&&self.confirming_order().is_none()
                    &&self.confirmed_cell.is_none()&&self.confirmed_modal_target.is_none()
                    &&self.workflow_freight_editor.is_none(){
                    self.workflow_ui.selection=None;
                    self.selected_recipient=0;
                }
            }
            Page::History=>if let Some(m)=self.order_history_manager.as_mut(){m.reset_navigation(ctx.clone());},
            Page::Excel=>if let Some(m)=self.excel_manager.as_mut(){m.reset_navigation();},
            Page::Settlement=>if let Some(m)=self.daily_settlement_manager.as_mut(){m.reset_navigation(ctx.clone());},
            Page::Products=>if let Some(m)=self.product_manager.as_mut(){m.reset_navigation(ctx.clone());},
            Page::Freight=>if let Some(m)=self.freight_manager.as_mut(){m.reset_navigation();},
            Page::Clients=>if let Some(m)=self.client_manager.as_mut(){m.reset_navigation(Arc::clone(&self.gateway),ctx.clone());},
            Page::Settings=>if let Some(m)=self.settings_view.as_mut(){m.reset_navigation();},
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        egui::TopBottomPanel::top("global_navigation")
            .exact_height(theme::HEADER_HEIGHT)
            .frame(
                theme::navigation_header_frame()
            )
            .show(ctx, |ui| {
                // The former top strip was a normal layout item, so its 8pt
                // height PLUS item spacing pushed everything down. Register
                // the same OS drag target as an overlay instead: it consumes
                // no layout height and cannot overlap the centered controls.
                let header = ui.max_rect();
                let strip = Rect::from_min_size(
                    header.min,
                    Vec2::new(header.width(), theme::HEADER_DRAG_STRIP),
                );
                let top_drag = ui.interact(
                    strip,
                    ui.make_persistent_id("header-empty-top-drag"),
                    Sense::click_and_drag(),
                );
                Self::handle_header_drag(ctx, &top_drag, maximized);

                // Every header child shares this one 62pt-high center line.
                // The logo and label are display-only, with no click, hover
                // styling, drag or double-click action.
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), theme::HEADER_HEIGHT),
                    Layout::left_to_right(Align::Center),
                    |bar| {
                        crate::ui_layout::apply_spacing_role(bar,crate::ui_layout::SpacingRole::Toolbar);
                        // The nested horizontal_centered row used the font
                        // line box for layout, leaving the visible capitals
                        // above the icon's optical center. Allocate ONE fixed
                        // brand slot and paint both elements against its
                        // explicit vertical center; no click/hover/drag area.
                        let wordmark = typography::brand_galley(bar, "BANDO HUB");
                        let icon_and_gap = if self.brand_texture.is_some() {
                            theme::BRAND_ICON_SIZE + theme::BRAND_WORDMARK_GAP
                        } else {
                            0.0
                        };
                        let (_, brand_slot) = bar.allocate_space(Vec2::new(
                            icon_and_gap + wordmark.size().x,
                            theme::HEADER_HEIGHT,
                        ));
                        if let Some(texture) = &self.brand_texture {
                            let icon_rect = Rect::from_center_size(
                                egui::pos2(
                                    brand_slot.left() + theme::BRAND_ICON_SIZE / 2.0,
                                    brand_slot.center().y + theme::BRAND_ICON_OPTICAL_CENTER_Y,
                                ),
                                Vec2::splat(theme::BRAND_ICON_SIZE),
                            );
                            bar.painter().image(
                                texture.id(),
                                icon_rect,
                                Rect::from_min_max(
                                    egui::Pos2::ZERO, egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        bar.painter().galley(
                            egui::pos2(
                                brand_slot.left() + icon_and_gap,
                                brand_slot.center().y - wordmark.size().y / 2.0
                                    + typography::BRAND_OPTICAL_CENTER_Y,
                            ),
                            wordmark,
                            theme::TEXT,
                        );
                        bar.add_space(theme::SPACE_12);
                        for page in Page::ALL {
                            let role = if self.page == page {
                                ButtonRole::ActiveNavigation
                            } else {
                                ButtonRole::Navigation
                            };
                            if crate::button::action(bar, page.label(), role,
                                page==self.page || (self.confirmed_cell.is_none() && self.confirmed_modal_target.is_none())
                            ).clicked() {
                                // Price cells must commit/cancel before leaving,
                                // and a product modal must not be discarded by nav.
                                let product_can_leave=
                                    if self.page==Page::Products && page!=Page::Products {
                                        let allowed=self.product_manager.as_mut()
                                            .is_none_or(|manager|manager.allow_leave());
                                        if !allowed && self.product_manager.as_ref()
                                            .is_some_and(|manager|manager.category_global_leave_in_progress()){
                                            self.product_pending_page=Some(page);
                                        }
                                        allowed
                                    }else{true};
                                // A client editor blocks navigation; a dirty
                                // automatic-message draft opens its existing
                                // save/discard guard instead of vanishing.
                                let client_can_leave=if product_can_leave &&
                                    self.page==Page::Clients && page!=Page::Clients {
                                    let allowed=self.client_manager.as_mut()
                                        .is_none_or(|manager|manager.request_global_leave());
                                    if !allowed{self.client_pending_page=Some(page);}
                                    allowed
                                }else{true};
                                let excel_can_leave=self.page!=Page::Excel||page==Page::Excel||
                                    !self.excel_manager.as_ref().is_some_and(|m|m.blocks_reply_navigation());
                                if product_can_leave && client_can_leave && excel_can_leave &&
                                    !self.client_manager.as_ref()
                                    .is_some_and(|manager|manager.has_modal()) &&
                                    !self.freight_manager.as_ref()
                                    .is_some_and(|manager|manager.blocks_navigation()) &&
                                    self.freight_pending_page.is_none() {
                                    // Same page-leave guard as React SettingsView:
                                    // a modified settings draft is never lost
                                    // by clicking another global navigation tab.
                                    if self.page==Page::Settings && page!=Page::Settings &&
                                        self.settings_view.as_ref().is_some_and(|view|view.dirty()){
                                        self.settings_pending_page=Some(page);
                                        self.settings_pending_save=false;
                                    }else if self.page==Page::Freight && page!=Page::Freight &&
                                        self.freight_manager.as_ref().is_some_and(|manager|manager.dirty()){
                                        self.freight_pending_page=Some(page);
                                        self.freight_pending_save=false;
                                    }else{
                                        // Kakao HWND ownership survives tab changes.
                                        // A return to freight must re-read staff edits made
                                        // while the page was not selected.
                                        if page==Page::Excel && self.page!=Page::Excel {
                                            if let Some(excel)=self.excel_manager.as_mut(){
                                                excel.refresh(ctx.clone());
                                            }
                                        }
                                        if page==Page::Settlement && self.page!=Page::Settlement {
                                            if let Some(settlement)=self.daily_settlement_manager.as_mut(){
                                                settlement.refresh(ctx.clone());
                                            }
                                        }
                                        if page==Page::History && self.page!=Page::History {
                                            if let Some(history)=self.order_history_manager.as_mut(){
                                                history.refresh(ctx.clone());
                                            }
                                        }
                                        if page==Page::Products && self.page!=Page::Products {
                                            if let Some(products)=self.product_manager.as_mut(){
                                                products.refresh(ctx.clone());
                                            }
                                        }
                                        if self.page==Page::Products && page!=Page::Products {
                                            if let Some(products)=self.product_manager.as_mut(){
                                                products.close_detail();
                                            }
                                        }
                                        if page==Page::Freight && self.page!=Page::Freight {
                                            if let Some(manager)=self.freight_manager.as_mut(){
                                                manager.refresh(ctx.clone());
                                            }
                                        }
                                        // Clean navigation preserves the assignment selection.
                                        // Explicit discard still goes through the leave guard.
                                        self.page=page;
                                    }
                                }
                            }
                        }
                        bar.with_layout(Layout::right_to_left(Align::Center), |actions| {
                            crate::ui_layout::apply_spacing_role(actions,crate::ui_layout::SpacingRole::CompactActions);
                            if crate::button::window_control(actions, WindowControl::Close).clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if crate::button::window_control(actions, WindowControl::Maximize).clicked()
                            {
                                ctx.send_viewport_cmd(
                                    egui::ViewportCommand::Maximized(!maximized)
                                );
                            }
                            if crate::button::window_control(actions, WindowControl::Minimize).clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                            actions.add_space(theme::SPACE_8);
                            self.notification_center.header_button(actions);
                            // Unoccupied header space has the same drag and
                            // double-click behavior as the original titlebar.
                            let empty = actions.available_width();
                            if empty > 8.0 {
                                let drag = actions.allocate_response(
                                    Vec2::new(empty, theme::NAV_HEIGHT),
                                    Sense::click_and_drag(),
                                );
                                Self::handle_header_drag(ctx, &drag, maximized);
                            }
                        });
                    },
                );
            });

    }

    fn frameless_resize(&self, ctx: &egui::Context) {
        // Windows borderless windows have no OS titlebar resize grip.
        // Provide 6-point hit regions without covering any interior controls.
        if ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else { return; };
        let bounds = ctx.viewport_rect();
        let edge = 6.0;
        let left = pointer.x <= bounds.left() + edge;
        let right = pointer.x >= bounds.right() - edge;
        let top = pointer.y <= bounds.top() + edge;
        let bottom = pointer.y >= bounds.bottom() - edge;
        use egui::viewport::ResizeDirection as Direction;
        let direction = match (left, right, top, bottom) {
            (true, _, true, _) => Some(Direction::NorthWest),
            (_, true, true, _) => Some(Direction::NorthEast),
            (true, _, _, true) => Some(Direction::SouthWest),
            (_, true, _, true) => Some(Direction::SouthEast),
            (true, _, _, _) => Some(Direction::West),
            (_, true, _, _) => Some(Direction::East),
            (_, _, true, _) => Some(Direction::North),
            (_, _, _, true) => Some(Direction::South),
            _ => None,
        };
        if let Some(direction) = direction {
            let cursor = match direction {
                Direction::NorthWest | Direction::SouthEast => egui::CursorIcon::ResizeNwSe,
                Direction::NorthEast | Direction::SouthWest => egui::CursorIcon::ResizeNeSw,
                Direction::North | Direction::South => egui::CursorIcon::ResizeVertical,
                _ => egui::CursorIcon::ResizeHorizontal,
            };
            ctx.set_cursor_icon(cursor);
            if ctx.input(|input| input.pointer.primary_pressed()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            }
        }
    }

    fn section(
        ui: &mut Ui,
        title: &str,
        minimum_height: f32,
        content: impl FnOnce(&mut Ui),
    ) {
        Self::section_with_header(ui, title, minimum_height, |_| {}, content);
    }

    fn section_with_header(
        ui:&mut Ui,title:&str,minimum_height:f32,
        header_right:impl FnOnce(&mut Ui),content:impl FnOnce(&mut Ui),
    ){
        // Single shared card chrome: Reception and manual classification
        // use identical header, spacing, divider and panel geometry.
        crate::card::reception_section_with_header(
            ui,title,minimum_height,header_right,content
        );
    }

    fn kakao_pane(&mut self, ui: &mut Ui) {
        // React HubShell owns a FULL Kakao slot, with only a one-point
        // border. The former docked_pane_frame() added 16pt on all sides and
        // calculated different Connect and connected HWND regions. Neither
        // is compatible with the exact native rectangle used by React.
        let scale=ui.ctx().pixels_per_point();
        let connected=self.kakao_dock.connected();
        let current_mode=self.kakao_dock.mode();
        let mut region=ui.max_rect();
        let mut connect=false;
        let mut disconnect=false;
        let mut next_mode=None;
        theme::kakao_slot_frame().show(ui,|pane|{
            let slot=pane.max_rect();
            pane.set_min_size(slot.size());
            // The connected HWND begins exactly below the 26pt toolbar.
            // No redundant divider or 1pt reserved strip is left between
            // toolbar and KakaoTalk; Connect and Sync use this same rect.
            region=Rect::from_min_max(
                egui::pos2(
                    slot.left(),
                    (slot.top()+theme::KAKAO_TOOLBAR_HEIGHT).min(slot.bottom())
                ),
                slot.max,
            );
            if connected {
                let toolbar_rect=Rect::from_min_size(
                    slot.min,Vec2::new(slot.width(),theme::KAKAO_TOOLBAR_HEIGHT)
                );
                pane.scope_builder(
                    UiBuilder::new().id_salt("kakao-native-toolbar")
                        .max_rect(toolbar_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |toolbar|{
                        toolbar.set_clip_rect(toolbar_rect);
                        crate::ui_layout::apply_spacing_role(toolbar,crate::ui_layout::SpacingRole::CompactActions);
                        toolbar.add_space(theme::KAKAO_TOOLBAR_PAD_X);
                        // Left: Kakao identity icon + one two-option mode
                        // group. Keep 8pt between the icon and first mode
                        // button AND between the two mode buttons.
                        crate::button::kakao_menu_icon(toolbar);
                        crate::ui_layout::apply_spacing_role(toolbar,crate::ui_layout::SpacingRole::KakaoIconMode);
                        let mode_role=|active|if active{
                            ButtonRole::ActiveKakaoControl
                        }else{ButtonRole::KakaoControl};
                        if crate::button::action(toolbar,"겹쳐 보기",
                            mode_role(current_mode==kakao_dock::DockMode::Together),true
                        ).clicked(){
                            next_mode=Some(kakao_dock::DockMode::Together);
                        }
                        crate::ui_layout::apply_spacing_role(toolbar,crate::ui_layout::SpacingRole::Toolbar);
                        if crate::button::action(toolbar,"나누어 보기",
                            mode_role(current_mode==kakao_dock::DockMode::Split),true
                        ).clicked(){
                            next_mode=Some(kakao_dock::DockMode::Split);
                        }
                        // Right: disconnect is an independent operation, not
                        // a third choice in the display-mode group.
                        toolbar.with_layout(Layout::right_to_left(Align::Center), |right| {
                            right.add_space(theme::KAKAO_TOOLBAR_PAD_X);
                            if crate::button::action(right,"연결 해제",
                                ButtonRole::KakaoControl,true
                            ).clicked(){
                                disconnect=true;
                            }
                        });
                    },
                );
            }else{
                // The unconnected React slot has no toolbar, and its single
                // Connect control stays centered in the actual slot.
                pane.scope_builder(
                    UiBuilder::new().id_salt("kakao-native-connect")
                        .max_rect(slot)
                        .layout(Layout::top_down(Align::Center)),
                    |center|{
                        center.set_clip_rect(slot);
                        center.add_space(
                            ((slot.height()-theme::ACTION_EMPHASIZED_HEIGHT)/2.0)
                                .max(0.0)
                        );
                        if crate::button::action(center,"카카오톡 연결",
                            ButtonRole::Emphasized,true
                        ).clicked(){
                            connect=true;
                        }
                    },
                );
            }
        });
        if disconnect {
            self.kakao_dock_error=None;
            self.kakao_dock.disconnect();
            return;
        }
        if connect {
            self.kakao_dock_error=None;
            // The dock receives the future connected BODY rectangle on the
            // first click, not the larger placeholder rectangle.
            if let Err(reason)=self.kakao_dock.connect(region,scale){
                eprintln!("[BANDO HUB] 카카오톡 연결 실패: {reason}");
                self.kakao_dock_error=Some(reason);
            }
            return;
        }
        if !connected {
            return;
        }
        let result=if let Some(mode)=next_mode {
            self.kakao_dock.set_mode(mode,region,scale)
        }else{
            self.kakao_dock.sync(region,scale)
        };
        if let Err(reason)=result {
            eprintln!("[BANDO HUB] 카카오톡 배치 실패: {reason}");
            self.kakao_dock_error=Some(reason);
        }
    }

    fn reception_modal_open(&self)->bool{
        self.ocr_review.is_some() || self.pending_replace.is_some() ||
            self.client_chooser.is_some() || self.manual_workspace.is_some() ||
            self.edit_history_open || self.workflow_dialog.is_some() ||
            self.vat_dialog.is_some() || self.item_editor.is_some() ||
            self.product_picker.is_some()
    }

    fn reception(&mut self, ctx: &egui::Context) {
        if self.reception_modal_open() || !overlay::background_preview_allowed(ctx){
            self.workflow_ui.stop_preview();
        }
        egui::CentralPanel::default()
            .frame(theme::workspace_shell_frame())
            .show(ctx, |ui| {
                let viewport_width = ui.available_width();
                let content_width = viewport_width.max(layout::LayoutState::minimum_width());
                let content_height = ui.available_height().max(1.0);

                // Exactly four column rectangles are allocated before drawing
                // any business content. Hover/folding a card cannot shift the
                // coordinates of splitters or other panels.
                // The full-width layout needs no ancestor ScrollArea.
                // It used to display a long hover-only horizontal thumb
                // along the bottom even when the four panes fitted exactly.
                let mut draw_columns = |ui: &mut Ui| {
                        let widths = self.layout.widths(content_width);
                        let (_, all) = ui.allocate_space(Vec2::new(content_width, content_height));
                        let mut x = all.left();
                        let mut splitter_visuals =
                            Vec::with_capacity(layout::PANEL_COUNT - 1);
                        for index in 0..layout::PANEL_COUNT {
                            let rect = Rect::from_min_size(
                                egui::pos2(x, all.top()),
                                Vec2::new(widths[index], all.height()),
                            );
                            // Each pane has a stable independent rectangle.
                            let _pane_output=ui.scope_builder(
                                UiBuilder::new()
                                    .id_salt(("reception-pane", index,self.navigation_snapshots.epoch(Page::Reception as usize)))
                                    .max_rect(rect)
                                    .layout(Layout::top_down(Align::Min)),
                                |pane| {
                                    pane.set_clip_rect(rect);
                                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,rect.width());
                                    pane.set_height(rect.height());
                                    match index {
                                        0 => self.kakao_pane(pane),
                                        1 => self.source_and_message(pane),
                                        2 => {
                                            one_touch::set(pane.ctx(),one_touch::Stage::None,false);
                                            self.details_and_freight(pane);
                                            one_touch::set(pane.ctx(),one_touch::Stage::None,false);
                                        },
                                        _ => self.orders_pane(pane),
                                    }
                                },
                            );
                            #[cfg(test)]
                            crate::ui_test_support::record(
                                ["kakao pane","source pane","details pane","orders pane"][index],
                                _pane_output.response.rect
                            );
                            x += widths[index];

                            if index < layout::PANEL_COUNT - 1 {
                                let splitter = Rect::from_min_size(
                                    egui::pos2(x, all.top()),
                                    Vec2::new(layout::SPLITTER, all.height()),
                                );
                                let id = ui.make_persistent_id(("horizontal-splitter", index));
                                let response = ui.interact(splitter, id, Sense::click_and_drag());
                                if response.hovered() || response.dragged() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                                }
                                let highlighted = response.hovered() || response.dragged();
                                crate::ui_layout::paint_dock_splitter(
                                    ui, splitter, true, highlighted,
                                );
                                splitter_visuals.push((splitter, highlighted));
                                if response.drag_started() {
                                    self.dragging_split = Some(HorizontalSplitDrag {
                                        index,
                                        initial_widths: widths,
                                    });
                                }
                                if response.dragged() {
                                    if let Some(drag) = self.dragging_split {
                                        if drag.index == index {
                                            // Absolute displacement from pointer-down.
                                            // Never integrate frame-by-frame deltas
                                            // over already mutated widths.
                                            self.layout.resize_pair(
                                                drag.initial_widths, index,
                                                response.total_drag_delta().unwrap_or_default().x,
                                            );
                                        }
                                    }
                                }
                                if response.drag_stopped()
                                    && self.dragging_split.is_some_and(|drag| drag.index == index)
                                {
                                    self.dragging_split = None;
                                    let _ = self.layout.save();
                                }
                                if response.double_clicked() {
                                    self.layout.reset_pair(widths, index);
                                    let _ = self.layout.save();
                                }
                                x += layout::SPLITTER;
                            }
                        }

                        // One common parent owns the entire splitter drawing
                        // topology. Both endpoints of each horizontal bridge
                        // are connected at the same snapped coordinates.
                        // Input hit areas and resizable pane geometry stay put.
                        if splitter_visuals.len() == layout::PANEL_COUNT - 1 {
                            let shared_upper = self.layout.vertical_heights(all.height()).0;
                            let [review_height, recipient_height, _] =
                                self.layout.detail_heights(all.height());
                            let source_y =
                                all.top() + shared_upper + layout::SPLITTER / 2.0;
                            let review_y =
                                all.top() + review_height + layout::SPLITTER / 2.0;
                            let items_y = all.top() + review_height
                                + layout::SPLITTER + recipient_height
                                + layout::SPLITTER / 2.0;
                            crate::ui_layout::paint_dock_network(
                                ui, &splitter_visuals,
                                &[
                                    (0, 1, source_y),
                                    (1, 2, review_y),
                                    (1, 2, items_y),
                                ],
                            );
                        }
                    };
                if content_width > viewport_width + 0.5 {
                    // Small windows must still be able to reach every pane.
                    crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Horizontal)
                        .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
                        .id_salt("reception_four_columns")
                        .auto_shrink([false, false])
                        .show(ui, &mut draw_columns);
                } else {
                    draw_columns(ui);
                }
            });
    }

    fn source_and_message(&mut self, ui: &mut Ui) {
        let height = ui.available_height().max(1.0);
        let width = ui.available_width();
        let (source_height, message_height) = self.layout.vertical_heights(height);
        let (_, all) = ui.allocate_space(Vec2::new(width, height));
        let source_rect = Rect::from_min_size(all.min, Vec2::new(width, source_height));
        let splitter_rect = Rect::from_min_size(
            egui::pos2(all.left(), source_rect.bottom()),
            Vec2::new(width, layout::SPLITTER),
        );
        let message_rect = Rect::from_min_size(
            egui::pos2(all.left(), splitter_rect.bottom()),
            Vec2::new(width, message_height),
        );

        ui.scope_builder(
            UiBuilder::new().id_salt("source-top").max_rect(source_rect),
            |child| {
                child.set_clip_rect(source_rect);
                child.apply_shared_width(crate::card::CardWidthPolicy::Fixed,source_rect.width());
                child.set_height(source_rect.height());
                self.source_section(child, source_height);
            },
        );

        let response = ui.interact(
            splitter_rect, ui.make_persistent_id("source-message-splitter"),
            Sense::click_and_drag(),
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        crate::ui_layout::paint_dock_splitter(
            ui, splitter_rect, false,
            response.hovered() || response.dragged(),
        );
        if response.drag_started() {
            self.dragging_vertical = Some(VerticalSplitDrag {
                initial_source_height: source_height,
                available_height: height,
            });
        }
        if response.dragged() {
            if let Some(drag) = self.dragging_vertical {
                self.layout.resize_vertical(
                    drag.initial_source_height,
                    response.total_drag_delta().unwrap_or_default().y,
                    drag.available_height,
                );
            }
        }
        if response.drag_stopped() && self.dragging_vertical.is_some() {
            self.dragging_vertical = None;
            let _ = self.layout.save();
        }
        if response.double_clicked() {
            self.layout.reset_vertical();
            let _ = self.layout.save();
        }

        ui.scope_builder(
            UiBuilder::new().id_salt("message-bottom").max_rect(message_rect),
            |child| {
                child.set_clip_rect(message_rect);
                child.apply_shared_width(crate::card::CardWidthPolicy::Fixed,message_rect.width());
                child.set_height(message_rect.height());
                self.message_section(child, message_height);
            },
        );
    }

    /// Selected/hovered workflow content is a transient read-only view.
    /// Never swap it into the editable egui working_source, autosave worker,
    /// or the active employee-selected draft.
    fn visible_workflow_group(&self)->Option<(Arc<workflow_board::BoardSnapshot>,usize,usize)>{
        if self.pending_stage_target().is_some(){return None;}
        let editing=self.workflow_freight_editor.is_some() || self.confirmed_cell.is_some()
            || self.confirmed_modal_target.is_some();
        let stage=if editing{None}else{self.workflow_ui.active_preview()};
        if !editing && stage.is_none() && self.workflow_ui.pending_preview().is_some(){
            return None;
        }
        let board=self.workflow_snapshot.as_ref()?;
        let committed=self.workflow_ui.selection.as_ref();
        // Linked recipient evidence can identify a shipment in ANOTHER
        // workflow group of the same confirmed order. Never match on name.
        let linked=if stage.is_none() && !editing{
            committed.and_then(|selected|
                self.link_hover.active_for(&selected.order_id).and_then(|key|
                    workflow_board::find_recipient_shipment(
                        board,&selected.order_id,&key.recipient_id,
                        Some(&selected.shipment_id)
                    )
                )
            )
        }else{None};
        let group_id=stage
            .or_else(||linked.map(|(group,_)|group.id.as_str()))
            .or_else(||committed.map(|selection|selection.group_id.as_str()))?;
        let group=board.groups.iter().find(|g|g.id==group_id)?;
        let previewed=stage.filter(|active|*active==group.id)
            .and_then(|_|self.workflow_ui.preview_shipment.as_deref())
            .and_then(|id|group.shipments.iter().find(|ship|ship.id==id));
        let linked_ship=linked.filter(|(g,_)|g.id==group.id)
            .map(|(_,ship)|ship);
        let selected=committed.filter(|selection|selection.group_id==group.id)
            .and_then(|selection|group.shipments.iter().find(|ship|
                ship.id==selection.shipment_id &&
                ship.order_id==selection.order_id));
        let ship=previewed.or(linked_ship).or(selected)
            .or_else(||group.shipments.first())?;
        let group_index=board.groups.iter().position(|candidate|std::ptr::eq(candidate,group))?;
        let ship_index=group.shipments.iter().position(|candidate|std::ptr::eq(candidate,ship))?;
        Some((Arc::clone(board),group_index,ship_index))
    }

    fn source_section(&mut self, ui: &mut Ui, height: f32) {
        // React visibleSelection: another pending order may be previewed
        // without replacing the employee's editable reception or autosave.
        if let Some((draft,index))=self.visible_pending_other(){
            let recognition=&draft.recognition;
            self.workflow_ui.note_pending_content_hover(
                &draft.id,ui.ctx().input(|input|input.pointer.hover_pos()
                    .is_some_and(|point|ui.clip_rect().contains(point)))
            );
            let spans=&draft.spans;
            let selected_id=reception_service::recipients(&recognition).get(index)
                .and_then(|row|row["id"].as_str()).map(str::to_owned);
            let active=self.link_hover.active_for(&draft.id).cloned()
                .filter(|key|spans.iter().any(|span|span.key==*key))
                .or_else(||{
                    // A stage preview is not a recipient-row hover. Only a
                    // genuine linked field may override its orange emphasis.
                    if self.workflow_ui.active_pending_stage()==Some(draft.id.as_str()){
                        None
                    }else{
                        selected_id.as_ref().map(|id|source_highlight::key(id,id,"name"))
                    }
                });
            let mut hovered_source=None;
            let mut new_clicked=false;
            let mut clicked_source=None;
            Self::section_with_header(ui,"발주 원문",height,|header|{
                if crate::button::action(header,"+ 새 주문",ButtonRole::Emphasized,true).clicked(){
                    new_clicked=true;
                }
            },|body|{
                crate::ui_layout::apply_spacing_role(body,crate::ui_layout::SpacingRole::DocumentBody);
                let available=crate::card::docked_section_body_height(height)
                    .min(body.available_height().max(0.0));
                let (body_rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),available),Sense::hover()
                );
                let card_rect=crate::ui_layout::reception_source_client_rect(body_rect);
                let linked_id=self.drafts.iter().find(|row|row.id==draft.id)
                    .and_then(|row|row.client_id.as_deref());
                let display=linked_id.and_then(|id|self.client_summaries.get(id));
                let name=linked_id.and_then(|id|self.client_names.get(id).map(String::as_str))
                    .unwrap_or(if linked_id.is_some(){"거래처 확인 중"}else{"거래처 확인 필요"});
                crate::composite_client::reception_client_summary_card_editable(
                    body,card_rect,linked_id.is_some(),display,name,false
                );
                let document_top=(card_rect.bottom()+theme::SPACE_8).min(body_rect.bottom());
                let document_rect=Rect::from_min_max(
                    egui::pos2(body_rect.left(),document_top),body_rect.max
                );
                let _=crate::card::reception_document_viewport(body,document_rect,
                    ("pending-hover-source",draft.id.as_str()),
                    ("pending-hover-source-scroll",draft.id.as_str()),|text_ui|{
                    let mut immutable=draft.raw.as_str();
                    let mut layouter=|layout_ui:&egui::Ui,
                        buffer:&dyn egui::TextBuffer,width:f32|{
                        let job=source_highlight::layout(
                            buffer.as_str(),&spans,
                            selected_id.as_deref(),active.as_ref(),width,
                            typography::reception_source_font(),
                            theme::TEXT
                        );
                        layout_ui.fonts_mut(|fonts|fonts.layout_job(job))
                    };
                    let paint_slot=text_ui.painter().add(egui::Shape::Noop);
                    let display=crate::input::reception_source_text_edit(
                        &mut immutable,text_ui.available_width())
                        .layouter(&mut layouter)
                        .show(text_ui);
                    let (normal,focused)=source_highlight::block_shapes(
                        &draft.raw,&display.galley,display.galley_pos,
                        &spans,selected_id.as_deref(),active.as_ref(),
                        text_ui.clip_rect().intersect(display.text_clip_rect),
                    );
                    text_ui.painter().set(
                        paint_slot,egui::Shape::Vec(normal)
                    );
                    text_ui.painter().add(egui::Shape::Vec(focused));
                    if display.response.hovered(){
                        if let Some(pointer)=text_ui.input(|i|
                            i.pointer.hover_pos()){
                            hovered_source=source_highlight::hovered_key_at_pointer(
                                &draft.raw,&spans,&display.galley,
                                display.galley_pos,pointer
                            );
                            if display.response.clicked(){
                                clicked_source=hovered_source.clone();
                            }
                        }
                    }
                });
            });
            self.source_hover=hovered_source;
            if let Some(clicked)=clicked_source{
                self.workflow_ui.selection=None;
                self.workflow_ui.folded=None;
                self.workflow_ui.stop_preview();
                self.link_hover.clear();
                self.source_hover=None;
                self.focus_pending_recipient(&workflow_board_ui::PendingTarget{
                    order_id:draft.id.clone(),
                    recipient_id:Some(clicked.recipient_id)
                },ui.ctx());
            }
            if new_clicked{
                self.new_reception(ui.ctx());
            }
            return;
        }
        if let Some((board,group_index,ship_index))=self.visible_workflow_group(){
            let group=&board.groups[group_index];
            let shipment=&group.shipments[ship_index];
            // Source and workflow shipment rows share the same authoritative
            // recipient/item spans. Do not identify recipients by name text:
            // duplicate names in one document must remain independent.
            let raw=&shipment.source_raw;
            if !self.workflow_source_display.as_ref().is_some_and(|cached|
                Arc::ptr_eq(&cached.board,&board) && cached.order_id==shipment.order_id &&
                cached.raw==shipment.source_raw){
                let recognition=serde_json::json!({"recipients":
                    board.groups.iter().flat_map(|g|g.shipments.iter())
                    .filter(|row|row.order_id==shipment.order_id && row.source_raw==shipment.source_raw)
                    .map(|row|&row.recipient).collect::<Vec<_>>()});
                self.workflow_source_display=Some(WorkflowSourceDisplay{
                    board:Arc::clone(&board),order_id:shipment.order_id.clone(),
                    raw:shipment.source_raw.clone(),
                    spans:source_highlight::collect(raw,&recognition).into(),
                });
            }
            let spans=self.workflow_source_display.as_ref().unwrap().spans.clone();
            let selected_id=Some(shipment.recipient_id.as_str());
            let active=self.link_hover.active_for(&shipment.order_id).cloned()
                .filter(|key|spans.iter().any(|span|span.key==*key));
            let mut hovered_source=None;
            let mut clicked_source=None;
            let mut new_clicked=false;
            let mut change_client_clicked=false;
            let no_edit=self.confirmed_cell.is_none() && self.confirmed_modal_target.is_none();
            let client_editable=no_edit && self.gateway.workflow_write_enabled()
                && self.workflow_mutation_ready() && !self.reception_modal_open()
                && self.workflow_freight_editor.is_none()
                && !group.dealer_id.is_empty()
                && !board.groups.iter().flat_map(|g|g.shipments.iter()).any(|s|
                    s.order_id==shipment.order_id && (s.transferred || s.completed_step.unwrap_or(0)>=4));
            Self::section_with_header(ui,"발주 원문",height,|header|{
                if crate::button::action(header,"+ 새 주문",ButtonRole::Emphasized,no_edit).clicked(){
                    new_clicked=true;
                }
            },|body|{
                crate::ui_layout::apply_spacing_role(body,crate::ui_layout::SpacingRole::DocumentBody);
                let available=crate::card::docked_section_body_height(height)
                    .min(body.available_height().max(0.0));
                let (body_rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),available),Sense::hover()
                );
                let card_rect=crate::ui_layout::reception_source_client_rect(body_rect);
                let display=self.client_summaries.get(&group.dealer_id);
                floating_notice::section(body.ctx(),"reception.client_change",card_rect);
                change_client_clicked=crate::composite_client::reception_client_summary_card_editable(
                    body,card_rect,!group.dealer_id.is_empty(),display,&group.dealer_name,
                    client_editable
                );
                let document_top=(card_rect.bottom()+theme::SPACE_8).min(body_rect.bottom());
                let document_rect=Rect::from_min_max(
                    egui::pos2(body_rect.left(),document_top),body_rect.max
                );
                let _=crate::card::reception_document_viewport(body,document_rect,
                    ("workflow-source-read-only",shipment.order_id.as_str()),
                    ("workflow-source-lines",shipment.order_id.as_str()),|content|{
                    let mut immutable=raw.as_str();
                    let mut layouter=|layout_ui:&egui::Ui,
                        buffer:&dyn egui::TextBuffer,width:f32|{
                        let job=source_highlight::layout(
                            buffer.as_str(),&spans,
                            selected_id,active.as_ref(),width,
                            typography::reception_source_font(),
                            theme::TEXT
                        );
                        layout_ui.fonts_mut(|fonts|fonts.layout_job(job))
                    };
                    let under=content.painter().add(egui::Shape::Noop);
                    let output=crate::input::reception_source_text_edit(
                        &mut immutable,content.available_width())
                        .layouter(&mut layouter).show(content);
                    let (normal,focused)=source_highlight::block_shapes(
                        raw,&output.galley,output.galley_pos,&spans,
                        selected_id,active.as_ref(),
                        content.clip_rect().intersect(output.text_clip_rect),
                    );
                    content.painter().set(under,egui::Shape::Vec(normal));
                    content.painter().add(egui::Shape::Vec(focused));
                    if output.response.hovered(){
                        if let Some(pointer)=content.input(|i|i.pointer.hover_pos()){
                            hovered_source=source_highlight::hovered_key_at_pointer(
                                raw,&spans,&output.galley,output.galley_pos,pointer
                            );
                            if output.response.clicked(){
                                clicked_source=hovered_source.clone();
                            }
                        }
                    }
                });
            });
            self.source_hover=hovered_source;
            if let Some(clicked)=clicked_source.filter(|_|self.confirmed_cell.is_none() && self.confirmed_modal_target.is_none()){
                // A source token may belong to a recipient in ANOTHER group.
                // Use the same ID-based resolver as hover, not just this card.
                if let Some((target_group,target))=self.workflow_snapshot.as_ref()
                    .and_then(|snapshot|workflow_board::find_recipient_shipment(
                        snapshot,&shipment.order_id,&clicked.recipient_id,
                        Some(&shipment.id)
                    )){
                    self.workflow_ui.select(workflow_board_ui::Selection{
                        group_id:target_group.id.clone(),
                        order_id:target.order_id.clone(),
                        shipment_id:target.id.clone()
                    });
                    self.link_hover.clear();
                    self.source_hover=None;
                }
            }
            if change_client_clicked{
                if let Some(bound)=self.confirmed_edit_target(&board,group,shipment){
                    self.client_chooser=Some(ClientChooser::new(shipment.order_id.clone(),
                        Arc::clone(&self.gateway),ui.ctx().clone()));
                    self.confirmed_modal_target=Some(bound);
                }
            }
            if new_clicked{
                self.new_reception(ui.ctx());
            }
            return;
        }
        let mut begin_input = false;
        let mut begin_new_order = false;
        let mut start_clipboard = None;
        let mut start_region_capture=false;
        let mut open_client_chooser = false;
        let mut open_manual_workspace = false;
        let can_receive_clipboard =
            self.clipboard_monitor.kind().has_intake() && !self.clipboard_busy;
        let ocr_processing = self.ocr_processing;
        let processing_preview=self.ocr_processing_preview.clone();
        let cancel_ocr = Arc::clone(&self.clipboard_cancel);
        let inbox_path = self.inbox_folder_path.clone();
        let inbox_loaded = self.inbox_folder_loaded;
        let inbox_load_error = self.inbox_folder_error.clone();
        let mut inbox_open_request: Option<String> = None;
        let mut inbox_copy_request: Option<String> = None;
        let has_direct_input=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
            .is_some_and(|row|row.raw.trim().is_empty() &&
                row.manual_model.as_ref().is_some_and(|saved|
                    saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
                ));
        Self::section_with_header(ui, "발주 원문", height, |ui| {
            if crate::button::action(ui, "+ 새 주문", ButtonRole::Emphasized, !ocr_processing).clicked() {
                begin_new_order = true;
            }
        }, |ui| {
            // Only the source-body layout owns spacing; the section header
            // and neighboring message pane keep their established tokens.
            crate::ui_layout::apply_spacing_role(ui,crate::ui_layout::SpacingRole::DocumentBody);
            if ocr_processing {
                // Match the React/Electron source-document blocking state:
                // replace the entire source body instead of overlaying a
                // small status line on an interactive editor or intake menu.
                self.source_hover = None;
                let available_body = crate::card::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                let (body_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), available_body),
                    Sense::hover(),
                );
                ui.painter().rect_filled(body_rect, 0.0, theme::CANVAS);
                let clip = body_rect.intersect(ui.clip_rect());
                ui.scope_builder(
                    UiBuilder::new()
                        .id_salt("source-ocr-processing-blocker")
                        .max_rect(body_rect)
                        .layout(Layout::top_down(Align::Center)),
                    |progress| {
                        progress.set_clip_rect(clip);
                        // The original clipboard image is downsampled on
                        // the OCR worker and received independently, before
                        // recognition completes. The employee sees it right
                        // here in the source pane, not in another popup.
                        let image_size=processing_preview.as_ref().map(
                            |(_,width,height)|{
                                let max_w=(body_rect.width()
                                    -2.0*theme::SPACE_12).max(1.0);
                                let max_h=(body_rect.height()
                                    -typography::FONT_OCR_PROGRESS_TITLE*1.4
                                    -theme::ACTION_STANDARD_HEIGHT
                                    -theme::SPACE_12
                                    -theme::SPACE_12
                                    -2.0*theme::SPACE_8).max(1.0);
                                let natural=Vec2::new(
                                    *width as f32,*height as f32
                                );
                                let scale=(max_w/natural.x.max(1.0))
                                    .min(max_h/natural.y.max(1.0))
                                    .min(1.0);
                                natural*scale
                            }
                        );
                        let image_space=image_size.map(|size|
                            size.y+theme::SPACE_12
                        ).unwrap_or(0.0);
                        let content_height=typography::FONT_OCR_PROGRESS_TITLE*1.4
                            +image_space+theme::SPACE_12
                            +theme::ACTION_STANDARD_HEIGHT;
                        progress.add_space(
                            ((body_rect.height()-content_height)/2.0).max(0.0)
                        );
                        // Visual reading order: source image, progress
                        // title, then Cancel. Keep the thumbnail inside the
                        // reception source body with no additional caption.
                        if let (Some((texture,_,_)),Some(size))=
                            (processing_preview.as_ref(),image_size){
                            let (image_rect,_)=progress.allocate_exact_size(
                                size,Sense::hover()
                            );
                            progress.painter().image(
                                texture.id(),image_rect,
                                Rect::from_min_max(
                                    egui::pos2(0.0,0.0),
                                    egui::pos2(1.0,1.0)
                                ),
                                egui::Color32::WHITE
                            );
                            progress.add_space(theme::SPACE_12);
                        }
                        progress.label(
                            typography::ocr_progress_title("이미지 분석중...")
                        );
                        progress.add_space(theme::SPACE_12);
                        if crate::button::action(
                            progress,"취소",ButtonRole::Standard,true
                        ).clicked(){
                            cancel_ocr.store(true,Ordering::Release);
                        }
                    },
                );
            } else if has_direct_input {
                self.source_hover=None;
                let available_body=crate::card::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                let (body_rect,_)=ui.allocate_exact_size(
                    Vec2::new(ui.available_width(),available_body),Sense::hover()
                );
                let gap=theme::SPACE_8;
                let card_rect=crate::ui_layout::reception_source_client_rect(body_rect);
                let active=self.active_id.as_deref()
                    .and_then(|id|self.drafts.iter().find(|row|row.id==id));
                let linked_id=active.and_then(|row|row.client_id.as_deref());
                let display=linked_id.and_then(|id|self.client_summaries.get(id));
                let name=linked_id
                    .and_then(|id|self.client_names.get(id).map(String::as_str))
                    .unwrap_or(if linked_id.is_some(){"거래처 확인 중"}else{"거래처 확인 필요"});
                floating_notice::section(
                    ui.ctx(),"reception.client_change",card_rect
                );
                if crate::composite_client::reception_client_summary_card_editable(
                    ui,card_rect,linked_id.is_some(),display,name,true
                ){
                    open_client_chooser=true;
                }
                let actions_rect=Rect::from_min_size(
                    egui::pos2(
                        body_rect.left(),
                        body_rect.bottom()-gap-theme::ACTION_STANDARD_HEIGHT
                    ),
                    Vec2::new(body_rect.width(),theme::ACTION_STANDARD_HEIGHT)
                );
                ui.scope_builder(
                    UiBuilder::new().id_salt("direct-input-resume-action")
                        .max_rect(actions_rect)
                        .layout(Layout::right_to_left(Align::Center)),
                    |actions|{
                        actions.set_clip_rect(actions_rect);
                        if crate::button::action(actions,"직접 입력 수정",ButtonRole::Emphasized,
                            self.manual_workspace.is_none() && self.client_change.is_none()
                                && self.working_sync.is_none()
                        ).clicked(){
                            begin_input=true;
                        }
                    }
                );
            } else if self.source_editing || !self.raw_source.is_empty() {
                // One allocated body rectangle owns the client card, source
                // editor and manual-classification action. The former sequence
                // of allocations mixed with scope_builder() could advance the
                // parent cursor so the document border landed on the client
                // card's bottom border. Position all three from ONE origin.
                let available_body = crate::card::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                let (body_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), available_body),
                    Sense::hover(),
                );
                let has_source = !self.raw_source.trim().is_empty();
                // The section divider already reserves the leading 8pt.
                // Keep 8pt between the client, document and action.
                // Short panes preserve these gaps and reduce editor height,
                // never overlap adjacent cards or change the dock splitter.
                let gap = theme::SPACE_8;
                let client_card_height = if has_source {
                    crate::ui_layout::reception_source_client_rect(body_rect).height()
                } else { 0.0 };
                if has_source {
                    let card_rect = crate::ui_layout::reception_source_client_rect(body_rect);
                    // Render a compact single-line reference. An advisory
                    // Rust match is not a confirmed dealer until it has passed
                    // the existing native order-client mutation.
                    let active=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|row|row.id==id));
                    let linked_id=active.and_then(|row|row.client_id.as_deref());
                    let display=linked_id.and_then(|id|self.client_summaries.get(id));
                    let name=linked_id
                        .and_then(|id|self.client_names.get(id).map(String::as_str))
                        .or_else(||linked_id.and_then(|id|{
                            self.analysis.as_ref().and_then(|analysis|{
                                if reception_service::text(
                                    &analysis.client_match,"clientId"
                                )==id {
                                    analysis.matched_client_name.as_deref()
                                }else{None}
                            })
                        }))
                        .unwrap_or(if linked_id.is_some(){
                            "거래처 확인 중"
                        }else{"거래처 확인 필요"});
                    floating_notice::section(
                        ui.ctx(),"reception.client_change",card_rect
                    );
                    if crate::composite_client::reception_client_summary_card_editable(
                        ui,card_rect,linked_id.is_some(),display,name,true
                    ){
                        open_client_chooser=true;
                    }
                }
                let editor_top = if has_source {
                    body_rect.top() + client_card_height + gap
                } else {
                    body_rect.top()
                };
                let editor_bottom = if has_source {
                    (body_rect.bottom() - 2.0 * gap - theme::ACTION_STANDARD_HEIGHT)
                        .max(editor_top)
                } else {
                    body_rect.bottom()
                };
                let viewport = Rect::from_min_max(
                    egui::pos2(body_rect.left(), editor_top),
                    egui::pos2(body_rect.right(), editor_bottom),
                );
                let editor_height = viewport.height();
                if editor_height > 0.0 {
                    // The document uses its own reserved rectangle; changing
                    // client labels, recognized text or selection never moves it.
                    // The native recognition worker owns source/recipient
                    // associations. Never infer a position by text search.
                    let spans=self.analysis.as_ref()
                        .filter(|_|self.active_id.as_deref().and_then(|id|
                            self.drafts.iter().find(|row|row.id==id)
                        ).is_some_and(|row|
                            row.analysis_raw.as_deref()==Some(self.raw_source.as_str())
                        ))
                        .map(|analysis|analysis.linked_spans.as_slice())
                        .unwrap_or(&[]);
                    let selected_id=self.analysis.as_ref()
                        .and_then(|analysis|reception_service::recipients(analysis)
                            .get(self.visible_recipient_index()))
                        .and_then(|recipient|recipient.get("id"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    // Use one shared evidence highlight for workflow
                    // row previews and source/detail field hover.
                    let board_link=self.workflow_ui.pending_preview()
                        .filter(|target|self.active_id.as_deref()
                            ==Some(target.order_id.as_str()))
                        .and_then(|target|target.recipient_id.as_deref())
                        .and_then(|id|spans.iter()
                            .find(|span|span.key.recipient_id==id &&
                                span.key.field=="name")
                            .or_else(||spans.iter().find(|span|
                                span.key.recipient_id==id)))
                        .map(|span|span.key.clone());
                    let active_link=self.active_id.as_deref()
                        .and_then(|id|self.link_hover.active_for(id))
                        .cloned().or(board_link);
                    let scroll_link=self.active_id.as_deref()
                        .and_then(|id|self.link_hover.active_for(id))
                        .and_then(|_|self.link_hover.scroll_to.clone());
                    let mut hovered_link=None;
                    let mut clicked_link=None;
                    let mut did_scroll=false;
                    // Resolve the delete guard before the closure borrows raw_source.
                    let source_writable=self.direct_source_entry && !self.active_delete_locked();
                    let editor=crate::card::reception_document_viewport(ui,viewport,
                        "source-editor-fixed-slot",
                        ("source-editor-vertical-scroll",self.active_id.as_deref()),|scroll_ui|{
                        // Keep the source/message splitter fixed. Only
                        // this scroll area grows its internal content.
                        let lines=self.raw_source.lines().count().max(8).min(1000);
                        let mut layouter=|lay_ui:&egui::Ui,
                            buffer:&dyn egui::TextBuffer, width:f32| {
                            let job=source_highlight::layout(
                                buffer.as_str(),&spans,
                                selected_id.as_deref(),active_link.as_ref(),
                                width,typography::reception_source_font(),theme::TEXT,
                            );
                            lay_ui.fonts_mut(|fonts|fonts.layout_job(job))
                        };
                        // Reserve an under-text paint slot before TextEdit draws its
                        // glyphs, caret and selection. Rounded gray source blocks
                        // remain decoration, never input hit targets.
                        let block_slot=scroll_ui.painter().add(egui::Shape::Noop);
                        // Use egui's immutable &str TextBuffer for
                        // imported/reopened source documents. Unlike
                        // interactive(false), this keeps text selection,
                        // copying and linked-span click/hover working.
                        // Only the explicit direct-entry route gets a
                        // mutable String buffer.
                        let mut show_source=|buffer:&mut dyn egui::TextBuffer|{
                            crate::input::reception_source_text_edit(
                                buffer,scroll_ui.available_width())
                                .desired_rows(lines)
                                .layouter(&mut layouter)
                                .show(scroll_ui)
                        };
                        let output=if source_writable {
                            show_source(&mut self.raw_source)
                        }else{
                            let mut read_only=self.raw_source.as_str();
                            show_source(&mut read_only)
                        };
                        let (block_fills,active_outlines)=
                            source_highlight::block_shapes(
                                &self.raw_source,&output.galley,output.galley_pos,
                                &spans,selected_id.as_deref(),active_link.as_ref(),
                                scroll_ui.clip_rect().intersect(output.text_clip_rect),
                            );
                        // Gray fills remain below glyphs; the active outline
                        // stays above neighboring fills and the editor text.
                        scroll_ui.painter().set(
                            block_slot,egui::Shape::Vec(block_fills)
                        );
                        scroll_ui.painter().add(
                            egui::Shape::Vec(active_outlines)
                        );
                        if output.response.hovered() {
                            if let Some(pointer)=scroll_ui.input(|i|i.pointer.hover_pos()) {
                                hovered_link=source_highlight::hovered_key_at_pointer(
                                    &self.raw_source,&spans,&output.galley,
                                    output.galley_pos,pointer
                                );
                                if output.response.clicked() {
                                    clicked_link=hovered_link.clone();
                                }
                            }
                        }
                        if let Some(key)=scroll_link.as_ref() {
                            if let Some(span)=spans.iter().find(|span|span.key==*key) {
                                let cursor=output.galley.pos_from_cursor(
                                    egui::text::CCursor::new(span.start_char)
                                );
                                scroll_ui.scroll_to_rect(
                                    cursor.translate(output.galley_pos.to_vec2()),
                                    Some(Align::Center),
                                );
                                did_scroll=true;
                            }
                        }
                        output.response
                    });
                    self.source_hover=hovered_link;
                    if did_scroll {self.link_hover.scroll_to=None;}
                    if let Some(clicked)=clicked_link {
                        if let Some(analysis)=&self.analysis {
                            if let Some(index)=reception_service::recipients(analysis)
                                .iter().position(|row|
                                    reception_service::text(row,"id")==clicked.recipient_id
                                ) {
                                self.selected_recipient=index;
                                self.workflow_ui.stop_preview();
                                self.link_hover.clear();
                            }
                        }
                    }
                    if self.direct_source_entry && editor.as_ref().is_some_and(|response|response.changed()) {
                        // Ctrl+V into explicit direct entry is text paste,
                        // not manual typing. Clipboard image OCR is recorded
                        // separately by the worker's OCR result branch.
                        let typed_method = if ui.ctx().input(|input|
                            input.events.iter().any(|event|
                                matches!(event, egui::Event::Paste(_))
                            )
                        ) {"TEXT_PASTE"} else {"DIRECT"};
                        if let Some(row)=self.active_id.as_deref().and_then(|id|
                            self.drafts.iter_mut().find(|row|row.id==id)
                        ) {
                            if !self.raw_source.trim().is_empty() {
                                if row.source_input_method.is_none() && row.raw.trim().is_empty() {
                                    row.source_input_method=Some(typed_method.into());
                                }
                                row.last_input_method=Some(typed_method.into());
                            } else {
                                row.source_input_method=None;
                                row.last_input_method=None;
                            }
                        }
                        self.clear_source_analysis();
                        self.queue_source_text_edit();
                        if !self.raw_source.trim().is_empty() {
                            // Analyze only after this input session finishes,
                            // not every 250ms while the user is still typing.
                            self.analysis_due_at=Some(Instant::now());
                        }
                    }
                    // A blur is an explicit end of this editing session.
                    // Empty input returns to intake options; nonempty input
                    // becomes immutable and is analyzed just once afterward.
                    if self.direct_source_entry && !self.active_delete_locked() && editor.as_ref().is_some_and(|response|response.lost_focus()){
                        self.direct_source_entry=false;
                        if self.raw_source.trim().is_empty(){
                            self.source_editing=false;
                            self.analysis_due_at=None;
                        }else{
                            self.analysis_due_at=Some(Instant::now());
                        }
                        ui.ctx().request_repaint();
                    }
                }
                if has_source {
                    let ready = self.active_id.as_deref()
                        .and_then(|id| self.drafts.iter().find(|row| row.id == id))
                        .is_some_and(|row|
                            row.analysis_raw.as_deref()==Some(self.raw_source.as_str()) &&
                            row.analysis_evidence.is_some()
                        );
                    let actions_rect = Rect::from_min_size(
                        egui::pos2(
                            body_rect.left(),
                            body_rect.bottom() - gap - theme::ACTION_STANDARD_HEIGHT,
                        ),
                        Vec2::new(body_rect.width(), theme::ACTION_STANDARD_HEIGHT),
                    );
                    ui.scope_builder(
                        UiBuilder::new()
                            .id_salt("source-manual-action-fixed-slot")
                            .max_rect(actions_rect)
                            .layout(Layout::right_to_left(Align::Center)),
                        |actions| {
                            actions.set_clip_rect(actions_rect);
                            if crate::button::action(actions,"수동 분류",ButtonRole::Standard,
                                ready && self.manual_workspace.is_none() &&
                                self.client_change.is_none() && self.working_sync.is_none()
                            ).clicked() {
                                open_manual_workspace=true;
                            }
                        },
                    );
                }
            } else {
                self.source_hover=None;
                // Narrow panes may need a stacked button row. Scroll ONLY the
                // intake instructions, never the source/message divider.
                let intake_height = crate::card::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                // The two inter-group dividers use the fixed shared geometry.
                // Distribute remaining space above and below the three groups.
                // Existing internal 8pt rhythms and local scrolling remain.
                let clipboard_group_height = typography::SUBSECTION_LINE_HEIGHT
                    + typography::METADATA_LINE_HEIGHT
                    + theme::RECEPTION_SHORTCUT_ROW_HEIGHT
                    + theme::ACTION_EMPHASIZED_HEIGHT
                    + 3.0 * theme::SPACE_8;
                let manual_group_height = typography::SUBSECTION_LINE_HEIGHT
                    + theme::ACTION_EMPHASIZED_HEIGHT + theme::SPACE_8;
                // The configured folder shows a short usage sentence
                // followed by the two standard actions. The saved path
                // remains internal to those actions and is never displayed.
                // Keep the folder group's body footprint identical
                // while the saved path loads, exists or is not configured.
                let folder_body_height=typography::METADATA_LINE_HEIGHT
                    +theme::SPACE_8+theme::ACTION_STANDARD_HEIGHT;
                let folder_group_height = typography::SUBSECTION_LINE_HEIGHT
                    + folder_body_height + theme::SPACE_8
;
                let dividers_height = 2.0 * theme::SECTION_DIVIDER_HEIGHT;
                let balanced_gap = ((intake_height - clipboard_group_height
                    - manual_group_height - folder_group_height - dividers_height)
                    / 2.0).max(theme::SPACE_8);
                crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                    .id_salt("source-intake-instructions")
                    .max_height(intake_height)
                    .auto_shrink([false, false])
                    .show(ui, |intake| {
                        crate::ui_layout::apply_spacing_role(intake,crate::ui_layout::SpacingRole::DocumentBody);
                        intake.add_space(balanced_gap);
                        intake.label(typography::subsection_heading("클립보드 접수"));
                        intake.add_space(theme::SPACE_8);
                        intake.label(typography::caption(
                            "복사한 텍스트와 이미지를 자동으로 구분해 인식합니다."
                        ));
                        intake.add_space(theme::SPACE_8);
                        let (agency, dealer) = crate::button::paired_actions(
                            intake,
                            ("대리발송", ButtonRole::Emphasized, can_receive_clipboard),
                            ("대리점 납품", ButtonRole::Emphasized, can_receive_clipboard),
                        );
                        if agency.clicked() {
                            start_clipboard = Some(OrderType::Agency);
                        } else if dealer.clicked() {
                            start_clipboard = Some(OrderType::Dealer);
                        }
                        intake.add_space(theme::SPACE_8);
                        crate::composite_reception::reception_capture_shortcut_hint(intake);
                        crate::ui_primitives::section_divider(intake);
                        intake.label(typography::subsection_heading("다른 방법으로 접수"));
                        intake.add_space(theme::SPACE_8);
                        let (direct,capture) = crate::button::paired_actions(
                            intake,
                            ("직접 입력", ButtonRole::Emphasized, true),
                            // A new region can be captured even if the clipboard
                            // starts empty. Disable only during an ongoing OCR.
                            ("캡처 범위 지정", ButtonRole::Emphasized,
                                !self.clipboard_busy),
                        );
                        if direct.clicked() {
                            begin_input = true;
                        }
                        if capture.clicked(){
                            start_region_capture=true;
                        }
                        crate::ui_primitives::section_divider(intake);
                        let folder_heading=intake.label(
                            typography::subsection_heading("폴더 자동 인식")
                        );
                        // C-type automatic failure: only surface it while the
                        // corresponding feature is visible in Reception. The
                        // normal configured/idle state remains inline.
                        floating_notice::section(
                            intake.ctx(),"reception.folder",folder_heading.rect
                        );
                        floating_notice::sync(
                            intake.ctx(),"reception.folder.error","reception.folder",
                            floating_notice::Tone::Error,inbox_load_error.as_deref()
                        );
                        let folder_message=if inbox_path.is_some(){
                            Some("발주서 이미지나 엑셀파일을 지정한 폴더에 저장하면 자동 인식합니다.")
                        }else if inbox_load_error.is_some(){
                            None
                        }else if !inbox_loaded{
                            Some("저장된 수신 폴더 설정을 확인하고 있습니다.")
                        }else{
                            Some("설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요.")
                        };
                        let folder_status_width=intake.available_width();
                        crate::ui_layout::fixed_status_slot(
                            intake,folder_status_width,typography::METADATA_LINE_HEIGHT,
                            folder_message,folder_message,theme::MUTED
                        );
                        intake.add_space(theme::SPACE_8);
                        let folder_actions_width=intake.available_width();
                        let _=crate::ui_layout::fixed_row(
                            intake,Vec2::new(folder_actions_width,theme::ACTION_STANDARD_HEIGHT),
                            Layout::left_to_right(Align::Center),|buttons|{
                                if let Some(path)=inbox_path.as_deref(){
                                    crate::ui_layout::apply_spacing_role(buttons,crate::ui_layout::SpacingRole::Toolbar);
                                    if crate::button::action(buttons,"경로 복사",
                                        ButtonRole::Standard,true).clicked(){
                                        inbox_copy_request=Some(path.to_owned());
                                    }
                                    if crate::button::action(buttons,"폴더 열기",
                                        ButtonRole::Standard,true).clicked(){
                                        inbox_open_request=Some(path.to_owned());
                                    }
                                }
                            }
                        );
                        intake.add_space(balanced_gap);
                    });
            }
        });
        // Side effects belong after section rendering, never to the stored
        // settings. A nonexistent folder stays configured but reports why
        // Explorer could not open it. Copy always uses the full saved path.
        if let Some(path) = inbox_copy_request {
            ui.ctx().copy_text(path);
            self.inbox_folder_open_error = None;
        }
        if let Some(path) = inbox_open_request {
            if !std::path::Path::new(&path).is_dir() {
                self.inbox_folder_open_error =
                    Some("지정된 발주 이미지 수신 폴더를 찾을 수 없습니다.".into());
            } else {
                #[cfg(target_os = "windows")]
                {
                    self.inbox_folder_open_error =
                        std::process::Command::new("explorer").arg(&path).spawn()
                            .err().map(|_| "발주 이미지 수신 폴더를 열 수 없습니다.".into());
                }
                #[cfg(not(target_os = "windows"))]
                {
                    self.inbox_folder_open_error =
                        Some("폴더 열기는 Windows 환경에서 사용할 수 있습니다.".into());
                }
            }
        }
        if begin_new_order {
            self.new_reception(ui.ctx());
        }
        if open_client_chooser {
            self.open_client_chooser(ui.ctx());
        }
        if open_manual_workspace {
            self.open_manual_workspace(ui.ctx());
        }
        if begin_input {
            if self.active_id.is_none(){self.new_reception(ui.ctx());}
            if self.active_id.is_some(){
                self.open_direct_workspace(ui.ctx());
            }
        }
        if let Some(kind) = start_clipboard {
            self.begin_clipboard_intake(kind, ui.ctx().clone());
        }
        if start_region_capture{
            self.begin_region_capture(self.order_type,ui.ctx().clone());
        }
    }

    fn message_section(&mut self, ui: &mut Ui, height: f32) {
        if let Some((board,group_index,_ship_index))=self.visible_workflow_group(){
            let group=&board.groups[group_index];
            // React also blocks payment copies with actionable Rust issues.
            // Do not enable controls just because the preview has text.
            let can_copy=self.gateway.workflow_write_enabled()
                && self.workflow_mutation_ready()
                && self.workflow_freight_editor.is_none()
                && self.confirmed_cell.is_none()
                && !group.transferred
                && group.automatic_text.as_deref()
                .is_some_and(|text|!text.trim().is_empty())
                && matches!(group.automatic_phase.as_deref(),
                    Some("request") | Some("payment"))
                && (group.automatic_phase.as_deref()!=Some("payment")
                    ||(group.automatic_payment_issues.is_empty()
                        && group.automatic_signature.as_deref().is_some_and(|value|!value.is_empty())));
            let hover_stage=if self.workflow_ui.active_stage_preview()==Some(group.id.as_str()) && self.workflow_freight_editor.is_none(){one_touch::Stage::from_step(group.step)}else{one_touch::Stage::None};
            let payment_emphasis=can_copy && group.automatic_phase.as_deref()==Some("payment") && matches!(hover_stage,one_touch::Stage::PaymentCopy|one_touch::Stage::PaymentConfirm);
            let mut copy_with_greeting=None;
            // A presentation preference survives frames and hover transitions.
            // A group without a validated official URL cannot add links.
            let link_preference=egui::Id::new("reception-automatic-include-links");
            let include_link=std::cell::Cell::new(ui.ctx().data(|d|
                d.get_temp::<bool>(link_preference).unwrap_or(false)));
            let has_links=group.has_message_links();
            Self::section_with_header(ui,"복사용 자동 메시지",height,|header|{
                let mut checked=include_link.get() && has_links;
                if crate::checkbox::checkbox(header,&mut checked,"링크 포함",has_links).changed(){
                    include_link.set(checked);
                    header.ctx().data_mut(|d|d.insert_temp(link_preference,checked));
                }
            },|body|{
                // All message-body spacing is explicit; default item spacing
                // previously pushed the bottom copy buttons past the pane.
                crate::ui_layout::apply_spacing_role(body,crate::ui_layout::SpacingRole::DocumentBody);
                let button_row_height=if body.available_width()<280.0{
                    theme::ACTION_STANDARD_HEIGHT*2.0+theme::SPACING
                }else{theme::ACTION_STANDARD_HEIGHT};
                let available=crate::card::docked_section_body_height(height)
                    .min(body.available_height().max(0.0));
                let message_height=(available-button_row_height
                    -theme::RECEPTION_MESSAGE_ACTION_GAP).max(0.0);
                // The canvas owns its fixed rectangle even when the message
                // is empty or its content is shorter than the viewport.
                let (message_rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),message_height),Sense::hover()
                );
                let _=crate::card::reception_document_viewport(body,message_rect,
                    "native-workflow-message-canvas",
                    ("native-workflow-message",group.id.as_str()),|content|{
                    if let Some(message)=group.message_text(include_link.get() && has_links){
                        one_touch::payment_message(content,message,
                            group.automatic_signature.as_deref().unwrap_or(""),
                            payment_emphasis);
                    }
                });
                crate::ui_layout::reception_message_footer_gap(body);
                one_touch::set(body.ctx(),if payment_emphasis{hover_stage}else{one_touch::Stage::None},false);
                copy_with_greeting=crate::composite_reception::reception_message_copy_actions(body,can_copy);
                one_touch::set(body.ctx(),one_touch::Stage::None,false);
            });
            if let Some(include_greeting)=copy_with_greeting{
                floating_notice::dispatch_pointer(
                    ui.ctx(),"reception.automatic.copy"
                );
                // Only a successful native clipboard write can create a copy audit.
                // The gateway revalidates the displayed message against the DB revision.
                let result=self.gateway.execute(
                    gateway::ReadOperation::DecorateAutomaticCopy,
                    serde_json::json!({
                        "text":group.message_text(include_link.get() && has_links).unwrap_or(""),
                        "includeGreeting":include_greeting
                    })
                );
                match result {
                    Ok(rendered)=>{
                        if let Some(text)=rendered.get("text")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text|!text.trim().is_empty()){
                            let copied=arboard::Clipboard::new()
                                .and_then(|mut clipboard|clipboard.set_text(text.to_owned()));
                            match copied{
                                Ok(())=>{
                                    let copy_type=if group.automatic_phase.as_deref()==Some("payment"){
                                        "PAYMENT"
                                    }else{"REQUEST"};
                                    self.start_workflow_action_with_args(
                                        "record_message_copy",&group.id,None,None,false,
                                        serde_json::json!({
                                            "copyType":copy_type,
                                            "token":uuid::Uuid::new_v4().to_string(),
                                            "signature":group.automatic_signature,
                                            "includeLinks":include_link.get() && has_links,
                                            "messageText":group.message_text(include_link.get() && has_links)
                                        }),ui.ctx()
                                    );
                                    if self.workflow_action.as_ref().is_none_or(|task|
                                        task.action!="record_message_copy"){
                                        floating_notice::publish(
                                            ui.ctx(),"reception.automatic.copy",
                                            floating_notice::Tone::Error,
                                            "복사는 완료됐지만 업무 이력 저장을 시작하지 못했습니다. 다시 복사하세요."
                                        );
                                    }
                                }
                                Err(error)=>floating_notice::publish(
                                    ui.ctx(),"reception.automatic.copy",
                                    floating_notice::Tone::Error,
                                    format!("클립보드 복사 실패: {error}")
                                )
                            }
                        }else{
                            floating_notice::publish(
                                ui.ctx(),"reception.automatic.copy",
                                floating_notice::Tone::Error,
                                "복사할 메시지가 없습니다."
                            );
                        }
                    }
                    Err(error)=>floating_notice::publish(
                        ui.ctx(),"reception.automatic.copy",
                        floating_notice::Tone::Error,error
                    ),
                }
            }
            return;
        }
        // The intake standby screen has no recognized source or working
        // order. Display only the section chrome, just like the empty review
        // and recipient panes. Once input arrives, restore the existing
        // message canvas and both copy actions without resetting any state.
        let pending_order_visible=self.pending_stage_target()
            .or_else(||self.workflow_ui.pending_preview())
            .is_some_and(|target|self.drafts.iter().any(|draft|draft.id==target.order_id));
        let awaiting_order = !pending_order_visible && self.raw_source.trim().is_empty()
            && self.analysis.is_none();
        // A draft can require an answer BEFORE it can enter workflow.
        // Use the exact source-bound native review result, never the stale
        // message from a hovered or previously selected order.
        let draft_message=if !pending_order_visible && self.inline_change.is_none()
            && !self.analysis_pending && self.inline_editor.is_none(){
            self.active_id.as_deref().and_then(|id|self.drafts.iter().find(|r|r.id==id))
                .zip(self.analysis.as_ref()).and_then(|(source,analysis)|
                    self.review_panel.draft_message(source,analysis)).cloned()
        }else{None};
        let can_copy=draft_message.as_ref().and_then(|result|result.as_ref().ok())
            .is_some_and(|text|!text.trim().is_empty());
        let mut copy_with_greeting=None;
        Self::section_with_header(ui, "복사용 자동 메시지", height, |ui| {
            if !awaiting_order {
                let mut include_link = false;
                crate::checkbox::checkbox(ui,&mut include_link,"링크 포함",false);
            }
        }, |ui| {
            if awaiting_order { return; }
            crate::ui_layout::apply_spacing_role(ui,crate::ui_layout::SpacingRole::DocumentBody);
            // Empty egui Frames shrink to their contents. Allocate the real
            // message rectangle explicitly so it fills the available width
            // instead of appearing as a narrow vertical stripe.
            let button_row_height = if ui.available_width() < 280.0 {
                theme::ACTION_STANDARD_HEIGHT * 2.0 + theme::SPACING
            } else {
                theme::ACTION_STANDARD_HEIGHT
            };
            let bottom_controls = button_row_height + theme::RECEPTION_MESSAGE_ACTION_GAP;
            let available_body = crate::card::docked_section_body_height(height)
                .min(ui.available_height().max(0.0));
            let field_height = (available_body - bottom_controls).max(0.0);
            let (field_rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), field_height),
                Sense::hover(),
            );
            let _=crate::card::reception_document_viewport(ui,field_rect,
                "pending-message-canvas","pending-message-scroll",|content|{
                    match draft_message.as_ref(){
                        Some(Ok(message))=>{one_touch::payment_message(content,message,"",false);},
                        Some(Err(_))=>{content.label(typography::body("문의 메시지를 생성하지 못했습니다. 자동 메시지 설정을 확인하세요."));},
                        None=>{}
                    }
                });
            crate::ui_layout::reception_message_footer_gap(ui);
            copy_with_greeting=crate::composite_reception::reception_message_copy_actions(ui,can_copy);
        });
        if let Some(include_greeting)=copy_with_greeting{
            floating_notice::dispatch_pointer(ui.ctx(),"reception.automatic.copy");
            let result=(||->Result<(),String>{
                let text=draft_message.as_ref().and_then(|r|r.as_ref().ok())
                    .ok_or("복사할 문의 메시지가 없습니다.")?;
                let rendered=self.gateway.execute(gateway::ReadOperation::DecorateAutomaticCopy,
                    serde_json::json!({"text":text,"includeGreeting":include_greeting}))?;
                let text=rendered["text"].as_str().filter(|s|!s.trim().is_empty())
                    .ok_or("복사할 메시지가 없습니다.")?;
                arboard::Clipboard::new().and_then(|mut clipboard|clipboard.set_text(text.to_owned()))
                    .map_err(|error|format!("클립보드 복사 실패: {error}"))
            })();
            if let Err(error)=result{
                floating_notice::publish(ui.ctx(),"reception.automatic.copy",floating_notice::Tone::Error,error);
            }
            // Draft clipboard copy is not a workflow commit or a send receipt.
        }
    }

    fn details_and_freight(&mut self, ui: &mut Ui) {
        let visible_index=self.visible_recipient_index();
        let full_height = ui.available_height();
        // Fixed recipient and total; review and items share the free height.
        // Middle-column gutters are visual only, never draggable.
        let [review_height, recipient_height, _bottom_height] =
            self.layout.detail_heights(full_height);
        let full_width = ui.available_width();
        let (_, bounds) = ui.allocate_space(Vec2::new(full_width, full_height));
        let review_rect = Rect::from_min_size(
            bounds.min, Vec2::new(full_width, review_height),
        );
        let first_sep = Rect::from_min_size(
            egui::pos2(bounds.left(), review_rect.bottom()),
            Vec2::new(full_width, layout::SPLITTER),
        );
        let recipient_rect = Rect::from_min_size(
            egui::pos2(bounds.left(), first_sep.bottom()),
            Vec2::new(full_width, recipient_height),
        );
        let second_sep = Rect::from_min_size(
            egui::pos2(bounds.left(), recipient_rect.bottom()),
            Vec2::new(full_width, layout::SPLITTER),
        );
        let bottom_area=Rect::from_min_size(
            egui::pos2(bounds.left(),second_sep.bottom()),
            Vec2::new(full_width,(bounds.bottom()-second_sep.bottom()).max(0.0))
        );
        let summary_height=theme::RECEPTION_ORDER_TOTAL_SECTION_HEIGHT
            .min(bottom_area.height().max(0.0));
        let summary_gap=theme::RECEPTION_ORDER_TOTAL_SECTION_GAP
            .min((bottom_area.height()-summary_height).max(0.0));
        let items_height=(bottom_area.height()-summary_height-summary_gap).max(0.0);
        let bottom_rect=Rect::from_min_size(
            bottom_area.min,Vec2::new(full_width,items_height)
        );
        let summary_rect=Rect::from_min_size(
            egui::pos2(bounds.left(),bottom_rect.bottom()+summary_gap),
            Vec2::new(full_width,summary_height)
        );

        // Keep the established gutter appearance without resize hit targets.
        for gutter in [first_sep,second_sep] {
            crate::ui_layout::paint_dock_splitter(ui,gutter,false,false);
        }

        if let Some((board,group_index,ship_index))=self.visible_workflow_group(){
            let group=&board.groups[group_index];
            let ship=&group.shipments[ship_index];
            let required=review_panel::RequiredFields::from_issues(&group.issues,&ship.order_id,&ship.recipient_id);
            let hover_stage=if self.workflow_ui.active_stage_preview()==Some(group.id.as_str()) && self.workflow_freight_editor.is_none(){one_touch::Stage::from_step(group.step)}else{one_touch::Stage::None};
            let remote=ship.remote.as_ref().and_then(|r|r["status"].as_str()).is_some_and(|s|matches!(s,"auto"|"review"));
            one_touch::set(ui.ctx(),hover_stage,remote);
            one_touch::prepare_reveal(ui.ctx(),hover_stage,&group.id,&ship.id);
            let linked_active=self.link_hover.active_for(&ship.order_id).cloned();
            let mut hovered_detail:Option<source_highlight::LinkKey>=None;
            let mut workflow_freight_open:Option<(String,String,String,Option<egui::Pos2>,bool)>=None;
            let mut workflow_freight_commit=false;
            let mut workflow_freight_cancel=false;
            let mut workflow_freight_option:Option<(String,serde_json::Value)>=None;
            let mut workflow_origin_change:Option<&'static str>=None;
            let mut confirmed_open:Option<(InlineTarget,String,Option<egui::Pos2>)>=None;
            let mut confirmed_commit=false;
            let mut confirmed_cancel=false;
            let mut confirmed_navigation_request:Option<(InlineTarget,CellNavigationKey)>=None;
            let mut review_edit:Option<(workflow_board_ui::Selection,InlineTarget)>=None;
            let mut review_product:Option<(String,serde_json::Value)>=None;
            let mut confirmed_validation_approval:Option<(String,serde_json::Value,bool)>=None;
            let mut confirmed_customer_inquiry:Option<(String,serde_json::Value,bool)>=None;
            let mut renotice_approval:Option<(serde_json::Value,bool)>=None;
            let mut confirmed_add=false;
            let mut confirmed_vat=false;
            let mut confirmed_item:Option<(serde_json::Value,bool)>=None;
            let confirmed_editable=self.gateway.workflow_write_enabled()
                && self.workflow_mutation_ready()
                && !self.reception_modal_open()
                && !board.groups.iter().flat_map(|g|g.shipments.iter())
                    .any(|other|other.order_id==ship.order_id && other.transferred);
            let confirmed_cell_idle=self.confirmed_cell.is_none();
            let confirmed_controls_idle=confirmed_cell_idle && self.workflow_freight_editor.is_none();
            let origin_editable=confirmed_editable && confirmed_controls_idle;
            // The same preallocated review/recipient/items rectangles are
            // used for real workflow rows. No source replacement, false
            // editing actions or section boundary changes on hover.
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-review")
                    .max_rect(review_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(review_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,review_rect.width());
                    pane.set_height(review_rect.height());
                    Self::section(pane,"확인할 내용",review_height,|ui|{
                        crate::ui_layout::reception_section_scroll(ui,review_height,
                            ("native-workflow-issues",group.id.as_str()),|issues|{
                                for issue in &group.issues{
                                    let Some(label)=workflow_board::workflow_issue_label(issue)else{continue};
                                    // Shared review presentation; original issue identity and
                                    // native blocker messages are preserved.
                                    let recognition=if issue["kind"]=="ADDRESS"||issue["kind"]=="MODEL"{
                                        group.shipments.iter().find(|s|
                                            issue["orderId"].as_str()==Some(s.order_id.as_str()) &&
                                            issue["recipientId"].as_str()==Some(s.recipient_id.as_str()))
                                            .map(|s|serde_json::json!({"recipients":[&s.recipient]})).unwrap_or(serde_json::Value::Null)
                                    }else{serde_json::Value::Null};
                                    let label=review_panel::workflow_review_text(issue,&label,&recognition);
                                    let tone=review_panel::issue_tone(issue);
                                    crate::card::review_issue_card(issues,tone,|content|{
                                        content.add(egui::Label::new(typography::review_title(&label,tone)).wrap());
                                        if issue["kind"]=="NOTICE"{
                                            let enabled=confirmed_editable && confirmed_controls_idle
                                                && !group.transferred && self.workflow_ui.active_preview().is_none();
                                            if let Some(cancel)=review_panel::show_validation_approval_action(content,issue,enabled){
                                                renotice_approval=Some((issue["approvalBasis"].clone(),cancel));
                                            }
                                        }
                                        if let Some(target)=group.shipments.iter().find(|s|
                                            issue["recipientId"].as_str()==Some(s.recipient_id.as_str()) &&
                                            issue["orderId"].as_str()==Some(s.order_id.as_str())){
                                            let enabled=confirmed_editable && confirmed_controls_idle
                                                && !target.transferred && self.workflow_ui.active_preview().is_none();
                                            if issue["severity"]=="ACTION"{
                                                let item_id=issue["itemId"].as_str();
                                                let field=review_panel::notice::confirmed_input_field(issue);
                                                if let Some(field)=field{
                                                    let inline=InlineTarget{recipient_id:target.recipient_id.clone(),
                                                        item_id:item_id.map(str::to_owned),field};
                                                    let editable=confirmed_navigation_entries(target,enabled,false)
                                                        .iter().any(|entry|entry.cell.enabled&&entry.target==inline);
                                                    let caption=review_panel::notice::input_action_caption(field);
                                                    if crate::button::review_action(content,caption,theme::ButtonRole::Standard,editable).clicked(){
                                                        review_edit=Some((workflow_board_ui::Selection{
                                                            group_id:group.id.clone(),order_id:target.order_id.clone(),shipment_id:target.id.clone()
                                                        },inline));
                                                    }
                                                }
                                                if review_panel::notice::confirmed_product_action(issue){
                                                    if let Some(item)=target.recipient["items"].as_array().and_then(|items|
                                                        items.iter().find(|i|i["id"].as_str()==item_id && item_id.is_some())){
                                                        if crate::button::review_action(content,review_panel::notice::PRODUCT_ACTION,theme::ButtonRole::Standard,enabled).clicked(){
                                                            review_product=Some((target.id.clone(),item.clone()));
                                                        }
                                                    }
                                                }
                                            }
                                            if issue["inquiryRequested"]==true{
                                                content.add(egui::Label::new(typography::review_detail(
                                                    review_panel::notice::INQUIRY_WAIT,tone)).wrap());
                                                content.add(egui::Label::new(typography::review_detail(
                                                    review_panel::INQUIRY_GUIDANCE,tone)).wrap());
                                            }
                                            if let Some(cancel)=review_panel::show_inquiry_action(content,issue,enabled){
                                                confirmed_customer_inquiry=Some((target.id.clone(),review_panel::inquiry_request(issue),cancel));
                                            }
                                            if let Some(cancel)=review_panel::show_validation_approval_action(content,issue,enabled){
                                                confirmed_validation_approval=Some((target.id.clone(),
                                                    review_panel::validation_approval_request(issue),cancel));
                                            }
                                        }
                                        if let Some((target,origin))=workflow_board::missing_freight_target(group,issue,&ship.id){
                                            let enabled=self.gateway.workflow_write_enabled()
                                                && self.workflow_mutation_ready()
                                                && self.workflow_freight_editor.is_none()
                                                && self.confirmed_cell.is_none();
                                            if crate::button::review_action(content,review_panel::notice::FREIGHT_ACTION,theme::ButtonRole::Standard,enabled).clicked(){
                                                workflow_freight_open=Some((target.id.clone(),origin,String::new(),None,true));
                                            }
                                        }
                                        if let Some(instruction)=review_panel::issue_instruction(issue){
                                            content.add(egui::Label::new(typography::review_detail(instruction,tone)).wrap());
                                        }
                                        if issue["severity"].as_str()!=Some("INFO"){
                                            if let Some(hint)=review_panel::pricing_registration_hint(
                                                issue["kind"].as_str().unwrap_or("")
                                            ){
                                                content.add(egui::Label::new(typography::review_detail(hint,tone)).wrap());
                                            }
                                        }
                                    });
                                }
                                for message in workflow_board::workflow_review_extra_messages(
                                    group,None
                                ){
                                    crate::status_notice::reception_review_error(
                                        issues,"reception.workflow.message",&message
                                    );
                                }
                            });
                    });
                }
            );
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-recipient")
                    .max_rect(recipient_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(recipient_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,recipient_rect.width());
                    pane.set_height(recipient_rect.height());
                    let mut next_ship=None;
                    let count=group.shipments.len();
                    let index=group.shipments.iter()
                        .position(|item|item.id==ship.id).unwrap_or(0);
                    let name=reception_service::text(&ship.recipient,"name");
                    Self::section_with_header(
                        pane,"수취인별 주문 정보",recipient_height,
                        |header|{
                            next_ship=crate::navigation::reception_recipient_navigator(
                                header,name,index,count,confirmed_cell_idle
                            );
                        },|ui|{
                        crate::ui_layout::reception_section_scroll(ui,recipient_height,
                            ("native-workflow-recipient-readonly",ship.id.as_str()),|contents|{
                                let mut name = reception_service::text(&ship.recipient,"name").to_owned();
                                let mut address = reception_service::text(&ship.recipient,"address").to_owned();
                                let mut mobile = reception_service::text(&ship.recipient,"mobile").to_owned();
                                let mut phone = reception_service::text(&ship.recipient,"phone").to_owned();
                                let mut note = reception_service::text(&ship.recipient,"note").to_owned();
                                let id=ship.recipient_id.as_str();
                                let fields=["name","address","mobile","phone","note"];
                                let highlighted=fields.map(|field|source_highlight::is_active_field(linked_active.as_ref(),id,id,field));
                                let responses=crate::composite_reception::reception_recipient_fields(
                                    contents,&mut name,&mut address,&mut mobile,
                                    &mut phone,&mut note,highlighted
                                );
                                let values=[name,address,mobile,phone,note];
                                for ((response,field),initial) in responses.iter().zip(fields).zip(values){
                                    if required.contains(id,field){crate::input::paint_required_input(contents,response.rect);}
                                    let key=source_highlight::key(id,id,field);
                                    if response.hovered(){hovered_detail=Some(key.clone());}
                                    let target=InlineTarget{recipient_id:id.into(),item_id:None,field};
                                    if confirmed_editable && response.clicked() && !self.confirmed_cell.as_ref().is_some_and(|e|e.cell.target==target){
                                        confirmed_open=Some((target.clone(),initial,response.interact_pointer_pos()));
                                    }
                                    if let Some(editor)=self.confirmed_cell.as_mut().filter(|e|
                                        e.target.selection.shipment_id==ship.id && e.cell.target==target){
                                        let navigation=cell_navigation::consume_worksheet_navigation_before_editor(contents.ctx(),confirmed_editor_slot(&ship.id,&target));
                                        let result=crate::input::reception_recipient_inline_cell_editor(
                                            contents,response.rect,&mut editor.cell.text,editor.cell.focus_next,
                                            editor.cell.click_at.take(),matches!(field,"mobile"|"phone"),
                                            source_highlight::is_active(linked_active.as_ref(),&key));
                                        cell_navigation::remember_worksheet_editor(contents.ctx(),
                                            confirmed_editor_slot(&ship.id,&target),result.id);
                                        editor.cell.focus_next=false;
                                        if overlay::take_cell_escape(&result,false){confirmed_cancel=true;}
                                        else if let Some(key)=navigation{
                                            confirmed_navigation_request=Some((target.clone(),key));
                                        }else if cell_navigation::worksheet_blur_finished(&result,false){confirmed_commit=true;}
                                    }
                                }
                            });
                        }
                    );
                    if let Some(next_index)=next_ship{
                        self.workflow_freight_guidance=None;
                        if let Some(target)=group.shipments.get(next_index){
                            let selection=workflow_board_ui::Selection{
                                group_id:group.id.clone(),
                                order_id:target.order_id.clone(),
                                shipment_id:target.id.clone(),
                            };
                            if self.workflow_freight_editor.as_ref().is_some_and(|editor|
                                editor.text.trim()!=editor.original.trim()
                            ){
                                self.workflow_freight_next_selection=Some(selection);
                                if !self.submit_workflow_freight_edit(pane.ctx()){
                                    self.workflow_freight_next_selection=None;
                                }
                            }else{
                                self.workflow_freight_editor=None;
                                self.workflow_freight_next_selection=None;
                                self.workflow_ui.select(selection);
                            }
                            // Click is authoritative even if the old source
                            // hover is still inside its 150ms release grace.
                            self.link_hover.clear();
                            self.source_hover=None;
                            pane.ctx().request_repaint();
                        }
                    }
                }
            );
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-items")
                    .max_rect(bottom_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(bottom_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,bottom_rect.width());
                    pane.set_height(bottom_rect.height());
                    Self::section_with_header(pane,"품목 및 운임",bottom_rect.height(),
                        |header|{
                            confirmed_add=crate::button::reception_item_header_action(
                                header,false,confirmed_editable && confirmed_controls_idle).clicked();
                        },|ui|{
                        crate::ui_layout::reception_docked_goods_scroll(
                            ui,bottom_rect.height(),("native-workflow-products",ship.id.as_str()),
                            true,|contents|{
                                        crate::table::inline(contents,crate::table::InlineHeader::Reception{editable:true},|contents|{
                                        if let Some(items)=ship.recipient.get("items")
                                            .and_then(serde_json::Value::as_array)
                                        {
                                            for item in items {
                                                let quantity=item.get("quantity")
                                                    .and_then(serde_json::Value::as_i64)
                                                    .filter(|n|*n>0)
                                                    .map(|n|format!("{n}개"))
                                                    .unwrap_or_else(||"—".into());
                                                // Never multiply a price in the UI: only a
                                                // Rust-projected line amount may appear here.
                                                let amount=item.get("lineAmount")
                                                    .or_else(||item.get("amount"))
                                                    .and_then(serde_json::Value::as_i64)
                                                    .map(crate::display_format::display_won)
                                                    .unwrap_or_else(||"—".into());
                                                let item_id=reception_service::text(item,"id");
                                                let fields=["product","quantity","price"];
                                                let highlighted=fields.map(|field|
                                                    source_highlight::is_active_field(linked_active.as_ref(),&ship.recipient_id,item_id,field)
                                                );
                                                let cells=crate::composite_reception::reception_item_row(
                                                    contents,reception_service::item_title_ref(item),
                                                    &quantity,&amount,highlighted,confirmed_editable,true
                                                );
                                                if cells.remove_clicked && confirmed_controls_idle{
                                                    confirmed_item=Some((item.clone(),true));
                                                }
                                                for (response,field) in [
                                                    (&cells.product,"product"),
                                                    (&cells.quantity,"quantity"),
                                                    (&cells.amount,"price")
                                                ] {
                                                    if required.contains(item_id,field){crate::input::paint_required_input(contents,response.rect);}
                                                    let key=source_highlight::key(
                                                        &ship.recipient_id,item_id,field
                                                    );
                                                    if response.hovered(){hovered_detail=Some(key);}
                                                    let target=InlineTarget{recipient_id:ship.recipient_id.clone(),
                                                        item_id:Some(item_id.into()),field:match field{
                                                            "price"=>"unitPrice","quantity"=>"quantity",_=>"product"
                                                        }};
                                                    if confirmed_editable && response.clicked() && !self.confirmed_cell.as_ref().is_some_and(|e|e.cell.target==target){
                                                        if field=="product"{
                                                            if confirmed_controls_idle{confirmed_item=Some((item.clone(),false));}
                                                        }else{
                                                            let original=item.get(target.field).and_then(serde_json::Value::as_i64)
                                                                .map(|n|n.to_string()).unwrap_or_default();
                                                            confirmed_open=Some((target.clone(),original,response.interact_pointer_pos()));
                                                        }
                                                    }
                                                    if let Some(editor)=self.confirmed_cell.as_mut().filter(|e|
                                                        e.target.selection.shipment_id==ship.id && e.cell.target==target){
                                                        let navigation=cell_navigation::consume_worksheet_navigation_before_editor(contents.ctx(),confirmed_editor_slot(&ship.id,&target));
                                                        let (result,step)=crate::input::reception_item_numeric_editor(
                                                            contents,response.rect,&mut editor.cell.text,editor.cell.focus_next,
                                                            editor.cell.click_at.take(),if field=="quantity"{
                                                                crate::input::ReceptionNumericCell::Quantity
                                                            }else{crate::input::ReceptionNumericCell::Money});
                                                        cell_navigation::remember_worksheet_editor(contents.ctx(),
                                                            confirmed_editor_slot(&ship.id,&target),result.id);
                                                        editor.cell.focus_next=false;
                                                        if overlay::take_cell_escape(&result,false){confirmed_cancel=true;}
                                                        else if let Some(key)=navigation{
                                                            confirmed_navigation_request=Some((target.clone(),key));
                                                        }else if crate::input::reception_numeric_edit_finished(&result,step){confirmed_commit=true;}
                                                    }
                                                }
                                            }
                                        }
                                        });
                                        crate::ui_layout::reception_items_to_freight_gap(contents);
                                        // Match React WorkflowFreightRows: one editable
                                        // amount per actual dispatch origin. The displayed
                                        // amount is the Rust-applied value (including the
                                        // locality multiplier), never the saved base charge.
                                        let origins=confirmed_freight_origins(ship);
                                        let remote_origin=if origins.iter().any(|origin|origin.as_str()=="FACTORY"){
                                            Some("FACTORY".to_owned())
                                        }else{origins.first().cloned()};
                                        let remote_checked=ship.remote.as_ref()
                                            .and_then(|decision|decision["checked"].as_bool())
                                            .unwrap_or(false);
                                        let controls_editable=self.gateway.workflow_write_enabled()
                                            && self.workflow_mutation_ready()
                                            && !ship.transferred
                                            && self.workflow_freight_editor.is_none()
                                            && self.confirmed_cell.is_none();
                                        for origin in &origins{
                                            let label=if origins.len()==1{"운임"}
                                                else if origin.as_str()=="FACTORY"{"공장 운임"}
                                                else{"매장 운임"};
                                            let amount=ship.display_charges.get(origin)
                                                .copied().flatten();
                                            let remote=if remote_origin.as_deref()==Some(origin.as_str()){
                                                Some(remote_checked)
                                            }else{None};
                                            let amount_editable=self.gateway.workflow_write_enabled()
                                                && self.workflow_mutation_ready() && !self.reception_modal_open()
                                                && !ship.transferred && !ship.freight_editing_blocked && !ship.collect
                                                && !self.workflow_freight_editor.as_ref().is_some_and(|e|
                                                    e.shipment_id==ship.id && e.origin==*origin);
                                            let freight_actions=crate::composite_reception::reception_freight_row_controls(
                                                contents,label,amount,Some(ship.collect),remote,
                                                origin.as_str()=="FACTORY",amount_editable,
                                                controls_editable,controls_editable,remote.is_some(),ship.freight_exempt
                                            );
                                            if freight_actions.amount_clicked{
                                                workflow_freight_open=Some((
                                                    ship.id.clone(),origin.clone(),
                                                    amount.map(|value|value.to_string()).unwrap_or_default(),
                                                    freight_actions.amount_click_pos,
                                                    workflow_board::missing_freight_origin(group,ship,origin)
                                                ));
                                            }
                                            if freight_actions.collect_clicked{
                                                workflow_freight_option=Some((
                                                    origin.clone(),
                                                    serde_json::json!({
                                                        "freightCode":if ship.collect{"010"}else{"020"}
                                                    })
                                                ));
                                            }
                                            if freight_actions.remote_clicked{
                                                workflow_freight_option=Some((
                                                    origin.clone(),
                                                    serde_json::json!({
                                                        "remoteChecked":!remote_checked
                                                    })
                                                ));
                                            }
                                            if self.workflow_action.is_none() &&
                                                !self.workflow_read.required(){
                                                if let (Some(editor),Some(rect))=(
                                                    self.workflow_freight_editor.as_mut().filter(|editor|
                                                        editor.group_id.as_str()==group.id.as_str() &&
                                                        editor.shipment_id.as_str()==ship.id.as_str() &&
                                                        editor.origin.as_str()==origin.as_str()
                                                    ),
                                                    freight_actions.amount_rect
                                                ){
                                                    let guide_focus=editor.focus_next &&
                                                        self.workflow_freight_guidance.as_deref()==Some(group.id.as_str());
                                                    let target=confirmed_freight_target(ship,origin);
                                                    let navigation=cell_navigation::consume_worksheet_navigation_before_editor(contents.ctx(),confirmed_editor_slot(&ship.id,&target));
                                                    let response=crate::input::reception_freight_inline_cell_editor(
                                                        contents,rect,&mut editor.text,
                                                        editor.focus_next,editor.click_at.take()
                                                    );
                                                    cell_navigation::remember_worksheet_editor(contents.ctx(),
                                                        confirmed_editor_slot(&ship.id,&target),response.id);
                                                    editor.focus_next=false;
                                                    if guide_focus{
                                                        Self::publish_validation_focus(&response,contents.ctx(),"reception.freight",
                                                            "배송 운임을 입력하세요.");
                                                    }
                                                    if overlay::take_cell_escape(&response,false){
                                                        workflow_freight_cancel=true;
                                                    }else if let Some(key)=navigation{
                                                        confirmed_navigation_request=Some((target,key));
                                                    }else if cell_navigation::worksheet_blur_finished(&response,false){
                                                        workflow_freight_commit=true;
                                                    }
                                                }
                                            }
                                            if workflow_board::missing_freight_origin(group,ship,origin){
                                                if let Some(rect)=freight_actions.amount_rect{
                                                    crate::input::paint_required_input(contents,rect);
                                                }
                                            }
                                            contents.add_space(theme::SPACE_8);
                                        }
                                        let origin=ship.recipient.get("dispatchOrigin")
                                            .and_then(serde_json::Value::as_str);
                                        workflow_origin_change=crate::composite_reception::reception_item_origin_action_editable(
                                            contents,origin,origin_editable
                                        ).filter(|selected|Some(*selected)!=origin);
                            }
                        );
                        // Whole-order billing is rendered in its own section.
                        // Only this shipment's goods/transport live here.
                    });
                }
            );
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-order-total")
                    .max_rect(summary_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(summary_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,summary_rect.width());
                    pane.set_height(summary_rect.height());
                    Self::section_with_header(
                        pane,"발주서 합계",summary_rect.height(),
                        |header|{let response=crate::button::action(
                            header,"부가세 설정",ButtonRole::Standard,confirmed_editable && confirmed_controls_idle
                        );confirmed_vat=response.clicked();if one_touch::stage(header)==one_touch::Stage::Review{one_touch::mark(header,response.rect);}},
                        |body|{
                            let mut included=false;
                            let mut excluded=false;
                            for row in board.groups.iter().flat_map(|g|g.shipments.iter())
                                .filter(|row|row.order_id==ship.order_id){
                                if gateway::rules::vat_applies(&serde_json::Value::Null,&row.recipient){included=true;}else{excluded=true;}
                            }
                            let vat=if included && excluded{"부가세 일부 포함"}
                            else if included{"부가세 포함"}else{"부가세 별도"};
                            crate::composite_reception::reception_total_footer(
                                body,ship.order_total,Some(ship.shipment_count),vat
                            );
                        }
                    );
                }
            );
            self.link_hover.update_for(
                &ship.order_id,
                hovered_detail.or_else(||self.source_hover.clone()),ui.ctx()
            );
            if confirmed_cancel{self.confirmed_cell=None;}
            else if workflow_freight_cancel{
                self.workflow_freight_guidance=None;
                self.workflow_freight_editor=None;
                self.workflow_freight_next_selection=None;
            }
            // Destination click wins over the previous editor's blur.
            else if let Some((selection,target))=review_edit{
                self.switch_confirmed_editor(selection,target,None,false,ui.ctx());
            }else if let Some((target_id,item))=review_product{
                if let Some(target_ship)=group.shipments.iter().find(|s|s.id==target_id){
                    if let Some(bound)=self.confirmed_edit_target(&board,group,target_ship){
                        let price=item["unitPrice"].as_i64();
                        self.item_editor=Some(item_editor::ItemEditor::new(
                            target_ship.order_id.clone(),target_ship.source_raw.clone(),
                            target_ship.recipient_id.clone(),item,group.dealer_id.clone(),price));
                        self.confirmed_modal_target=Some(bound);
                        ui.ctx().request_repaint();
                    }
                }
            }else if let Some((target,_original,click_at))=confirmed_open{
                self.switch_confirmed_editor(workflow_board_ui::Selection{
                    group_id:group.id.clone(),order_id:ship.order_id.clone(),shipment_id:ship.id.clone()
                },target,click_at,false,ui.ctx());
            }else if let Some((target_id,origin,_original,click_at,guide))=workflow_freight_open{
                if let Some(target_ship)=group.shipments.iter().find(|s|s.id==target_id){
                    self.switch_confirmed_editor(workflow_board_ui::Selection{
                        group_id:group.id.clone(),order_id:target_ship.order_id.clone(),
                        shipment_id:target_ship.id.clone()
                    },confirmed_freight_target(target_ship,&origin),click_at,guide,ui.ctx());
                }
            }else if let Some((current,key))=confirmed_navigation_request{
                let editable=self.gateway.workflow_write_enabled() && self.workflow_mutation_ready()
                    && !self.reception_modal_open()
                    && !board.groups.iter().flat_map(|g|g.shipments.iter())
                        .any(|other|other.order_id==ship.order_id && other.transferred);
                let freight_editable=self.gateway.workflow_write_enabled() && self.workflow_mutation_ready()
                    && !ship.transferred && !ship.freight_editing_blocked && !ship.collect;
                let entries=confirmed_navigation_entries(ship,editable,freight_editable);
                let next=resolve_inline_navigation(&entries,&current,key);
                if next.is_some() || key.commits_at_boundary(){
                    self.confirmed_next_cell=next.map(|target|(workflow_board_ui::Selection{
                        group_id:group.id.clone(),order_id:ship.order_id.clone(),shipment_id:ship.id.clone()
                    },target,None,false));
                    if current.field=="shippingCharge"{
                        if self.confirmed_next_cell.is_some(){self.workflow_freight_guidance=None;}
                        if !self.submit_workflow_freight_edit(ui.ctx()){self.confirmed_next_cell=None;}
                    }else{
                        self.submit_confirmed_cell(ui.ctx());
                        if self.confirmed_cell.is_some(){self.confirmed_next_cell=None;}
                    }
                    ui.ctx().request_repaint();
                }
            }
            else if confirmed_commit{self.submit_confirmed_cell(ui.ctx());}
            else if let Some((basis,cancel))=renotice_approval{
                self.start_workflow_action_with_args(
                    if cancel{"cancel_renotice_approval"}else{"approve_renotice"},
                    &group.id,None,None,false,serde_json::json!({"approvalBasis":basis}),ui.ctx());
            }
            else if let Some((id,request,cancel))=confirmed_customer_inquiry{
                if let Some(target_ship)=group.shipments.iter().find(|s|s.id==id){
                    if let Some(bound)=self.confirmed_edit_target(&board,group,target_ship){
                        self.submit_confirmed_change(&bound,
                            if cancel{"cancelCustomerInquiry"}else{"requestCustomerInquiry"},None,request,ui.ctx());
                    }
                }
            }
            else if let Some((id,request,cancel))=confirmed_validation_approval{
                if let Some(target_ship)=group.shipments.iter().find(|s|s.id==id){
                    if let Some(bound)=self.confirmed_edit_target(&board,group,target_ship){
                        self.submit_confirmed_change(&bound,
                            if cancel{"cancelValidationApproval"}else{"validationApproval"},None,request,ui.ctx());
                    }
                }
            }
            else if confirmed_vat{
                if let Some(bound)=self.confirmed_edit_target(&board,group,ship){
                    let mut seen=std::collections::HashSet::new();
                    let rows=board.groups.iter().flat_map(|g|g.shipments.iter())
                        .filter(|row|row.order_id==ship.order_id && seen.insert(row.recipient_id.clone()))
                        .map(|row|row.recipient.clone()).collect::<Vec<_>>();
                    self.vat_dialog=Some(VatDialog::new(ship.order_id.clone(),ship.source_raw.clone(),&rows));
                    self.confirmed_modal_target=Some(bound);
                }
            }else if confirmed_add || confirmed_item.is_some(){
                if let Some(bound)=self.confirmed_edit_target(&board,group,ship){
                    if let Some((item,delete))=confirmed_item{
                        let price=item["unitPrice"].as_i64();
                        let mut dialog=item_editor::ItemEditor::new(ship.order_id.clone(),ship.source_raw.clone(),
                            ship.recipient_id.clone(),item,group.dealer_id.clone(),price);
                        if delete{dialog.open_delete_only();}
                        self.item_editor=Some(dialog);
                    }else{
                        self.product_picker=Some((ship.order_id.clone(),ship.source_raw.clone(),ship.recipient_id.clone(),
                            product_manager::ProductManager::new_selection(Arc::clone(&self.gateway),
                                ui.ctx().clone(),group.dealer_id.clone())));
                    }
                    self.confirmed_modal_target=Some(bound);
                    ui.ctx().request_repaint();
                }
            }else if workflow_freight_commit{
                self.submit_workflow_freight_edit(ui.ctx());
            }else if let Some(origin)=workflow_origin_change{
                self.start_workflow_action_with_args(
                    "change_confirmed_dispatch_origin",&group.id,Some(&ship.id),
                    None,false,serde_json::json!({"origin":origin}),ui.ctx()
                );
            }else if let Some((origin,mut extra))=workflow_freight_option{
                floating_notice::dispatch_pointer(ui.ctx(),"reception.freight");
                if !extra.is_object(){extra=serde_json::json!({});}
                extra["origin"]=serde_json::json!(origin);
                self.start_workflow_action_with_args(
                    "set_shipment_freight",&group.id,Some(&ship.id),
                    None,false,extra,ui.ctx()
                );
            }
            return;
        }

        // Same display target as the read-only source pane above. The
        // pending card may point to another order/recipient; do not swap
        // this projection into the employee's active editable draft.
        if let Some((draft,index))=self.visible_pending_other(){
            let hover_stage=if self.workflow_ui.active_pending_stage()==Some(draft.id.as_str()){
                one_touch::Stage::Review
            }else{one_touch::Stage::None};
            one_touch::set(ui.ctx(),hover_stage,false);
            let recognition=&draft.recognition;
            self.workflow_ui.note_pending_content_hover(
                &draft.id,ui.ctx().input(|input|input.pointer.hover_pos()
                    .is_some_and(|point|ui.clip_rect().contains(point)))
            );
            let spans=&draft.spans;
            let linked_active=self.link_hover.active_for(&draft.id).cloned()
                .filter(|key|spans.iter().any(|span|span.key==*key));
            let mut hovered_detail=None;
            let recipients=reception_service::recipients(&recognition);
            let recipient=recipients.get(index);
            let target=recipient.map(|row|reception_service::text(row,"id")).unwrap_or("");
            one_touch::prepare_reveal(ui.ctx(),hover_stage,&draft.id,target);
            let preview=draft.pricing_preview.as_ref()
                .filter(|quote|quote["sourceRaw"].as_str()==Some(draft.raw.as_str()));
            let quote_index=preview.map(indexed_quote);
            let mut next_target=None;
            let preview_source=self.drafts.iter().find(|row|row.id==draft.id);
            self.pending_review_panel.refresh_preview(ui.ctx(),Arc::clone(&self.gateway),
                preview_source,draft.review_analysis.as_ref());
            ui.scope_builder(
                UiBuilder::new().id_salt("pending-hover-review")
                    .max_rect(review_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(review_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,review_rect.width());
                    pane.set_height(review_rect.height());
                    Self::section(pane,"확인할 내용",review_height,|ui|{
                        crate::ui_layout::reception_section_scroll(ui,review_height,
                            ("pending-hover-review-issues",draft.id.as_str()),|issues|{
                                if let (Some(source),Some(analysis))=(preview_source,draft.review_analysis.as_ref()){
                                    self.pending_review_panel.show_preview(issues,source,analysis,index);
                                }else{
                                    crate::status_notice::management_body_state_overlay(
                                        issues,issues.available_rect_before_wrap(),
                                        Some("분석 결과가 준비되면 확인할 내용을 표시합니다.")
                                    );
                                }
                            });
                    });
                }
            );
            ui.scope_builder(
                UiBuilder::new().id_salt("pending-hover-recipient")
                    .max_rect(recipient_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(recipient_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,recipient_rect.width());
                    pane.set_height(recipient_rect.height());
                    Self::section_with_header(
                        pane,"수취인별 주문 정보",recipient_height,
                        |header|{
                            if !recipients.is_empty(){
                                let name=recipient.map(|row|
                                    reception_service::text(row,"name")).unwrap_or("");
                                if let Some(next)=crate::navigation::reception_recipient_navigator(
                                    header,name,index,recipients.len(),true){
                                    next_target=recipients.get(next)
                                        .and_then(|row|row["id"].as_str())
                                        .map(|id|workflow_board_ui::PendingTarget{
                                            order_id:draft.id.clone(),
                                            recipient_id:Some(id.into())
                                        });
                                }
                            }
                        },|ui|{
                            if let Some(row)=recipient{
                                crate::ui_layout::reception_section_scroll(ui,recipient_height,
                                    ("pending-hover-recipient",draft.id.as_str()),|contents|{
                                        let mut name=reception_service::text(row,"name").to_owned();
                                        let mut address=reception_service::text(row,"address").to_owned();
                                        let mut mobile=reception_service::text(row,"mobile").to_owned();
                                        let mut phone=reception_service::text(row,"phone").to_owned();
                                        let mut note=reception_service::text(row,"note").to_owned();
                                        let recipient_id=reception_service::text(row,"id");
                                        let fields=["name","address","mobile","phone","note"];
                                        let active_fields=fields.map(|field|
                                            (hover_stage!=one_touch::Stage::Review && linked_active.is_none() && field=="name") ||
                                            source_highlight::is_active_field(linked_active.as_ref(),recipient_id,recipient_id,field)
                                        );
                                        let responses=crate::composite_reception::reception_recipient_fields(
                                            contents,&mut name,&mut address,&mut mobile,
                                            &mut phone,&mut note,active_fields
                                        );
                                        for (response,field) in responses.iter().zip(fields){
                                            let key=source_highlight::key(recipient_id,recipient_id,field);
                                            if response.hovered(){hovered_detail=Some(key);}
                                        }
                                    });
                            }
                        }
                    );
                }
            );
            ui.scope_builder(
                UiBuilder::new().id_salt("pending-hover-goods")
                    .max_rect(bottom_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(bottom_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,bottom_rect.width());
                    pane.set_height(bottom_rect.height());
                    Self::section_with_header(
                        pane,"품목 및 운임",bottom_rect.height(),
                        |header|{
                            crate::button::reception_item_header_action(header,false,false);
                        },|ui|{
                            crate::ui_layout::reception_docked_goods_scroll(
                                ui,bottom_rect.height(),("pending-hover-products",draft.id.as_str()),
                                false,|contents|{
                                            crate::table::inline(contents,crate::table::InlineHeader::Reception{editable:false},|contents|{
                                            if let Some(row)=recipient{
                                                if let Some(items)=row["items"].as_array(){
                                                    for item in items{
                                                        let item_id=reception_service::text(item,"id");
                                                        let quantity=item["quantity"].as_i64()
                                                            .filter(|n|*n>0)
                                                            .map(|n|format!("{n}개"))
                                                            .unwrap_or_else(||"—".into());
                                                        let amount=quote_index.as_deref()
                                                            .and_then(|quote|quote.item(item_id))
                                                            .and_then(|line|line["amount"].as_i64())
                                                            .map(crate::display_format::display_won)
                                                            .unwrap_or_else(||"—".into());
                                                        let recipient_id=reception_service::text(row,"id");
                                                        let fields=["product","quantity","price"];
                                                        let active_fields=fields.map(|field|
                                                            source_highlight::is_active_field(linked_active.as_ref(),recipient_id,item_id,field)
                                                        );
                                                        let cells=crate::composite_reception::reception_item_row(
                                                            contents,
                                                            reception_service::item_title_ref(item),
                                                            &quantity,&amount,active_fields,false,false
                                                        );
                                                        for (response,field) in [
                                                            (&cells.product,"product"),
                                                            (&cells.quantity,"quantity"),
                                                            (&cells.amount,"price")
                                                        ] {
                                                            let key=source_highlight::key(
                                                                recipient_id,item_id,field
                                                            );
                                                            if response.hovered(){hovered_detail=Some(key);}
                                                        }
                                                    }
                                                }
                                            }
                                            });
                                            if let Some(row)=recipient{
                                                crate::ui_layout::reception_items_to_freight_gap(contents);
                                                let origin=row["dispatchOrigin"].as_str();
                                                let fee=indexed_billable_freight(quote_index.as_deref(),row);
                                                let collect=row["freightCode"].as_str()
                                                    .map(|code|code=="020").unwrap_or(false);
                                                let remote=row["remoteDelivery"]["checked"].as_bool();
                                                let _=crate::composite_reception::reception_freight_row_controls(
                                                    contents,"운임",
                                                    fee,
                                                    Some(collect),remote,
                                                    origin==Some("FACTORY"),false,false,false,true,
                                                    indexed_freight_exempt(quote_index.as_deref(),row)
                                                );
                                                contents.add_space(theme::SPACE_8);
                                                crate::composite_reception::reception_item_origin_action_editable(
                                                    contents,origin,false
                                                );
                                            }
                                }
                            );
                        }
                    );
                }
            );
            ui.scope_builder(
                UiBuilder::new().id_salt("pending-hover-total")
                    .max_rect(summary_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(summary_rect);
                    pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,summary_rect.width());
                    pane.set_height(summary_rect.height());
                    Self::section_with_header(
                        pane,"발주서 합계",summary_rect.height(),
                        |header|{
                            let response=crate::button::action(header,"부가세 설정",ButtonRole::Standard,false);
                            if one_touch::stage(header)==one_touch::Stage::Review{one_touch::mark(header,response.rect);}
                        },|body|{
                            let inclusive=recipients.iter().filter(|r|
                                gateway::rules::vat_applies(&serde_json::Value::Null,r)).count();
                            let vat=if inclusive==recipients.len() && !recipients.is_empty(){
                                "부가세 포함"
                            }else if inclusive>0{"부가세 일부 포함"}
                            else{"부가세 별도"};
                            crate::composite_reception::reception_total_footer(
                                body,preview.and_then(|quote|quote["total"].as_i64()),
                                Some(recipients.len()),vat
                            );
                        }
                    );
                }
            );
            if let Some(target)=next_target{
                self.workflow_ui.stop_preview();
                self.workflow_ui.selection=None;
                self.link_hover.clear();
                self.source_hover=None;
                self.focus_pending_recipient(&target,ui.ctx());
            }else{
                // The read-only projection never changes the working draft.
                // Evidence must exist in this order's authoritative spans.
                let hovered=hovered_detail.or_else(||self.source_hover.clone())
                    .filter(|key|spans.iter().any(|span|span.key==*key));
                self.link_hover.update_for(&draft.id,hovered,ui.ctx());
            }
            return;
        }
        let hover_stage=if self.active_id.as_deref().is_some_and(|id|self.workflow_ui.active_pending_stage()==Some(id)) && self.inline_editor.is_none(){
            one_touch::Stage::Review
        }else{one_touch::Stage::None};
        one_touch::set(ui.ctx(),hover_stage,false);
        let target=self.analysis.as_ref()
            .and_then(|analysis|reception_service::recipients(analysis).get(visible_index))
            .map(|row|reception_service::text(row,"id")).unwrap_or("");
        one_touch::prepare_reveal(ui.ctx(),hover_stage,self.active_id.as_deref().unwrap_or(""),target);
        let required=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
            .zip(self.analysis.as_ref())
            .map(|(source,analysis)|self.review_panel.required_fields(source,analysis,target))
            .unwrap_or_default();
        let mut review_action:Option<review_panel::Action>=None;
        let mut review_header_action:Option<review_panel::Action>=None;
        let review_model_enabled=self.inline_editor.is_none()
            &&self.manual_workspace.is_none()&&self.vat_change.is_none()
            &&self.vat_dialog.is_none()&&self.client_change.is_none()
            &&self.working_sync.is_none()&&self.workflow_ui.pending_preview().is_none()
            &&!self.active_confirmation_locked()&&!self.active_delete_locked()
            &&self.inline_change.is_none();
        let mut hovered_detail: Option<source_highlight::LinkKey>=None;
        let linked_active=self.active_id.as_deref()
            .and_then(|id|self.link_hover.active_for(id)).cloned();
        ui.scope_builder(
            UiBuilder::new().id_salt("details-review").max_rect(review_rect)
                .layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(review_rect);
                pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,review_rect.width());
                pane.set_height(review_rect.height());
                Self::section_with_header(pane, "확인할 내용", review_height, |header| {
                    if let Some(analysis)=&self.analysis {
                        let (shown,action)=self.review_panel.show_model_shortcut(
                            header,analysis,visible_index,review_model_enabled);
                        review_header_action=action;
                        if !shown || header.available_width()>=160.0{
                            self.review_panel.show_header_summary(header,analysis,visible_index);
                        }
                    }
                }, |ui| {
                    crate::ui_layout::reception_section_scroll(ui,review_height,
                        ("reception-review-content-scroll",self.active_id.as_deref()), |ui| {
                    if let Some(analysis)=&self.analysis {
                        let other_ready=self.inline_editor.is_none()
                            &&self.manual_workspace.is_none()
                            &&self.vat_change.is_none()
                            &&self.vat_dialog.is_none()
                            &&self.client_change.is_none()
                            &&self.working_sync.is_none()
                            &&self.workflow_ui.pending_preview().is_none();
                        let other_ready=other_ready && !self.active_confirmation_locked() && !self.active_delete_locked();
                        let can_act=other_ready && self.inline_change.is_none();
                        let client_can_act=other_ready && (self.inline_change.is_none() ||
                            self.inline_change.as_ref().is_some_and(|p|p.is_delivery_command() &&
                                p.next_client_review.is_none()));
                        review_action=self.review_panel.show_with_client_actions(
                            ui,analysis,visible_index,can_act,client_can_act
                        );
                    }
                        });
                });
            },
        );
        if let Some(action)=review_header_action.or(review_action) {
            self.handle_review_action(action,ui.ctx());
        }

        let mut next_recipient = None;
        let mut inline_open:Option<(InlineTarget,String,Option<egui::Pos2>)>=None;
        let mut inline_navigation_request:Option<(InlineTarget,CellNavigationKey)>=None;
        let mut item_popup:Option<(String,String)>=None;
        let mut item_remove_popup:Option<(String,String)>=None;
        let mut inline_commit=false;
        let mut inline_cancel=false;
        let mut inline_command:Option<(InlineTarget,String)>=None;
        let editable=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
            .is_some_and(|row|{
                let direct=row.raw.trim().is_empty() &&
                    row.manual_model.as_ref().is_some_and(|saved|
                        saved.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
                    );
                direct || (row.analysis_raw.as_deref()==Some(self.raw_source.as_str())
                    && row.analysis_evidence.is_some())
            })
            && visible_index==self.selected_recipient
            && !self.active_confirmation_locked()
            && !self.active_delete_locked()
            && self.workflow_ui.pending_preview().is_none()
            && self.manual_workspace.is_none()
            && self.client_change.is_none()
            && self.working_sync.is_none()
            && self.inline_change.is_none()
            && self.vat_change.is_none()
            && self.vat_dialog.is_none();
        let recipient_nav=self.analysis.as_ref()
            .map(|analysis|reception_service::recipients(analysis))
            .filter(|recipients|!recipients.is_empty())
            .map(|recipients|{
                let index=visible_index.min(recipients.len()-1);
                let name=reception_service::text(&recipients[index],"name")
                    .to_owned();
                (name,index,recipients.len())
            });
        ui.scope_builder(
            UiBuilder::new().id_salt("details-recipient")
                .max_rect(recipient_rect).layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(recipient_rect);
                pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,recipient_rect.width());
                pane.set_height(recipient_rect.height());
                Self::section_with_header(
                    pane,"수취인별 주문 정보",recipient_height,
                    |header|{
                        // Actual recipient inputs are directly editable;
                        // separate Edit/History header controls are redundant.
                        if let Some((name,index,total))=recipient_nav.as_ref(){
                            next_recipient=crate::navigation::reception_recipient_navigator(
                                header,name,*index,*total,
                                self.inline_change.is_none()
                                    && self.working_sync.is_none()
                                    && self.manual_workspace.is_none()
                                    && self.product_picker.is_none()
                                    && self.item_editor.is_none()
                            );
                        }
                    },|ui|{
                    if let Some(analysis) = &self.analysis {
                        let recipients = reception_service::recipients(analysis);
                        if let Some(recipient) = recipients.get(visible_index) {
                            let mut name = reception_service::text(recipient, "name").to_owned();
                            let mut address = reception_service::text(recipient, "address").to_owned();
                            let mut mobile = reception_service::text(recipient, "mobile").to_owned();
                            let mut phone = reception_service::text(recipient, "phone").to_owned();
                            let mut note = reception_service::text(recipient, "note").to_owned();
                            let recipient_id = reception_service::text(recipient, "id");
                            // Match the React contact grid and allow only its
                            // content to scroll when the user narrows this pane.
                            crate::ui_layout::reception_section_scroll(ui,recipient_height,
                                ("recognized-recipient-fields",self.active_id.as_deref(),recipient_id), |fields| {
                                    let board_preview=self.workflow_ui.pending_preview()
                                        .is_some_and(|target|
                                            self.active_id.as_deref()
                                                ==Some(target.order_id.as_str()) &&
                                            target.recipient_id.as_deref()==Some(recipient_id));
                                    let active_fields=[
                                        "name","address","mobile","phone","note"
                                    ].map(|field|(board_preview && field=="name") ||
                                        source_highlight::is_active_field(linked_active.as_ref(),recipient_id,recipient_id,field));
                                    let responses = crate::composite_reception::reception_recipient_fields(
                                        fields, &mut name, &mut address, &mut mobile,
                                        &mut phone, &mut note, active_fields,
                                    );
                                    let values=[
                                        name.as_str(),address.as_str(),mobile.as_str(),
                                        phone.as_str(),note.as_str()
                                    ];
                                    for ((response,field),initial) in
                                        responses.into_iter().zip([
                                            "name","address","mobile","phone","note"
                                         ]).zip(values) {
                                        let target=InlineTarget{
                                            recipient_id:recipient_id.to_owned(),
                                            item_id:None,field
                                        };
                                        let is_editing=self.inline_editor.as_ref()
                                            .is_some_and(|edit|edit.target==target);
                                        // The shared recipient field hit target owns
                                        // both reverse source-hover and inline click.
                                        // Do not register a second overlapping click
                                        // response: it can suppress the field hover.
                                        if editable && !is_editing && response.clicked(){
                                            inline_open=Some((
                                                target.clone(),initial.to_owned(),
                                                response.interact_pointer_pos()
                                            ));
                                        }
                                        let key=source_highlight::key(
                                            recipient_id,recipient_id,field
                                        );
                                        if response.hovered(){hovered_detail=Some(key.clone());}
                                        if let Some(editor)=self.inline_editor.as_mut()
                                            .filter(|edit|edit.target==target) {
                                            let navigation=cell_navigation::consume_worksheet_navigation_before_editor(fields.ctx(),inline_editor_slot(&target));
                                            let edit_response=crate::input::reception_recipient_inline_cell_editor(
                                                fields,response.rect,&mut editor.text,
                                                editor.focus_next,
                                                editor.click_at.take(),
                                                matches!(field,"mobile"|"phone"),
                                                source_highlight::is_active(
                                                    linked_active.as_ref(),&key
                                                )
                                            );
                                            cell_navigation::remember_worksheet_editor(
                                                fields.ctx(),inline_editor_slot(&target),
                                                edit_response.id
                                            );
                                            editor.focus_next=false;
                                            Self::show_inline_validation_focus(
                                                &mut self.inline_validation_focus,&target,&edit_response,fields.ctx());

                                            if overlay::take_cell_escape(&edit_response,false){
                                                inline_cancel=true;
                                            }else if let Some(key)=navigation{
                                                inline_navigation_request=Some((target.clone(),key));
                                            }else if cell_navigation::worksheet_blur_finished(&edit_response,false){
                                                inline_commit=true;
                                            }
                                        }
                                        if required.contains(recipient_id,field){
                                            crate::input::paint_required_input(fields,response.rect);
                                        }
                                    }
                                });
                        }
                    }
                });
            },
        );
        if let Some(index) = next_recipient {
            self.selected_recipient = index;
            self.workflow_ui.stop_preview();
            self.link_hover.clear();
            self.source_hover=None;
            if self.inline_editor.is_some(){inline_commit=true;}
        }
        let selected = self.analysis.as_ref().and_then(|analysis| {
            reception_service::recipients(analysis).get(visible_index)
        });

        let remaining = bottom_rect.height();
        ui.scope_builder(
            UiBuilder::new().id_salt("details-items").max_rect(bottom_rect)
                .layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(bottom_rect);
                pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,bottom_rect.width());
                pane.set_height(bottom_rect.height());
                let mut header_add_item=false;
                Self::section_with_header(pane,"품목 및 운임",remaining,
                    |header|{
                        let response=crate::button::reception_item_header_action(
                            header,false,editable && selected.is_some());
                        if selected.is_some_and(|row|required.contains(reception_service::text(row,"id"),"addItem")){
                            crate::input::paint_required_input(header,response.rect);
                        }
                        header_add_item=response.clicked();
                    },|ui| {
                    // In intake standby, do not draw empty table headings,
                    // freight controls, VAT text or a fictitious zero total.
                    // Keep the section frame and divider exactly as-is.
                    if self.raw_source.trim().is_empty()
                        && self.analysis.is_none() {
                        return;
                    }
                    let preview = self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|draft|draft.id==id))
                        .and_then(|draft|draft.pricing_preview.as_ref())
                        .filter(|quote|quote.get("sourceRaw")
                            .and_then(serde_json::Value::as_str)
                            ==Some(self.raw_source.as_str()));
                    let quote_index=preview.map(indexed_quote);
                    crate::ui_layout::reception_docked_goods_scroll(
                        ui,remaining,("recognized-items",self.active_id.clone(),
                            selected.map(|r|reception_service::text(r,"id").to_owned())),true,|items_ui|{
                                    crate::table::inline(items_ui,crate::table::InlineHeader::Reception{editable:true},|items_ui|{
                                    if let Some(recipient)=selected {
                                        let recipient_id=reception_service::text(recipient,"id");
                                        if let Some(items)=recipient.get("items")
                                            .and_then(serde_json::Value::as_array)
                                        {
                                            for item in items {
                                                let item_id=reception_service::text(item,"id");
                                                let quantity=item.get("quantity")
                                                    .and_then(serde_json::Value::as_i64)
                                                    .filter(|n|*n>0)
                                                    .map(|n|format!("{n}개"))
                                                    .unwrap_or_else(||"—".into());
                                                // Native quote owns unit price × quantity.
                                                // The UI only reads its exact computed amount.
                                                let amount=quote_index.as_deref()
                                                    .and_then(|quote|quote.item(item_id))
                                                    .and_then(|line|line.get("amount"))
                                                    .and_then(serde_json::Value::as_i64)
                                                    .map(crate::display_format::display_won)
                                                    .unwrap_or_else(||"—".to_owned());
                                                let board_preview=self.workflow_ui.pending_preview()
                                                    .is_some_and(|target|
                                                        self.active_id.as_deref()
                                                            ==Some(target.order_id.as_str()) &&
                                                        target.recipient_id.as_deref()==Some(recipient_id));
                                                let active_fields=[
                                                    "product","quantity","price"
                                                ].map(|field|(board_preview && field=="product") ||
                                                    source_highlight::is_active_field(linked_active.as_ref(),recipient_id,item_id,field));
                                                let cells=crate::composite_reception::reception_item_row(
                                                    items_ui,reception_service::item_title_ref(item),
                                                    &quantity,&amount,active_fields,editable,true
                                                );
                                                if cells.remove_clicked{
                                                    item_remove_popup=Some((recipient_id.to_owned(),item_id.to_owned()));
                                                }
                                                for (response,field) in [
                                                    (&cells.product,"product"),
                                                    (&cells.quantity,"quantity"),
                                                    (&cells.amount,"price"),
                                                ] {
                                                    let key=source_highlight::key(
                                                        recipient_id,item_id,field
                                                    );
                                                    if response.hovered(){
                                                        hovered_detail=Some(key.clone());
                                                    }
                                                    let target=InlineTarget{
                                                        recipient_id:recipient_id.to_owned(),
                                                        item_id:Some(item_id.to_owned()),
                                                        field:match field {
                                                            "quantity"=>"quantity",
                                                            "price"=>"unitPrice",
                                                            _=>"product"
                                                        }
                                                    };
                                                    let is_editing=self.inline_editor.as_ref()
                                                        .is_some_and(|editor|editor.target==target);
                                                    if editable && !is_editing && response.clicked(){
                                                        if field=="product"{
                                                            if item_editor::product_click_intent(
                                                                response.clicked(),editable&&!is_editing,false
                                                            )==item_editor::ProductClickIntent::OpenDetail{
                                                                item_popup=Some((recipient_id.to_owned(),item_id.to_owned()));
                                                            }
                                                        }else{
                                                            let original=match field{
                                                        "quantity"=>item.get("quantity")
                                                            .and_then(serde_json::Value::as_i64)
                                                            .map(|n|n.to_string()).unwrap_or_default(),
                                                        "price"=>item.get("manualUnitPrice")
                                                            .or_else(||item.get("unitPrice"))
                                                            .and_then(serde_json::Value::as_i64)
                                                            .or_else(||quote_index.as_deref()
                                                                .and_then(|quote|quote.item(item_id))
                                                                .and_then(|line|line["unitPrice"].as_i64()))
                                                            .map(|n|n.to_string()).unwrap_or_default(),
                                                        _=>reception_service::item_title(item)
                                                    };
                                                            inline_open=Some((
                                                                target.clone(),original,
                                                                response.interact_pointer_pos()
                                                            ));
                                                        }
                                                    }
                                                    if let Some(editor)=self.inline_editor.as_mut()
                                                        .filter(|editor|editor.target==target) {
                                                        let navigation=cell_navigation::consume_worksheet_navigation_before_editor(items_ui.ctx(),inline_editor_slot(&target));
                                                        let (result,step_interaction)=crate::input::reception_item_numeric_editor(
                                                            items_ui,response.rect,&mut editor.text,
                                                            editor.focus_next,editor.click_at.take(),
                                                            if field=="quantity"{crate::input::ReceptionNumericCell::Quantity}
                                                            else{crate::input::ReceptionNumericCell::Money}
                                                        );
                                                        cell_navigation::remember_worksheet_editor(
                                                            items_ui.ctx(),inline_editor_slot(&target),
                                                            result.id
                                                        );
                                                        editor.focus_next=false;
                                                        Self::show_inline_validation_focus(
                                                            &mut self.inline_validation_focus,&target,&result,items_ui.ctx());

                                                        if overlay::take_cell_escape(&result,false) {
                                                            inline_cancel=true;
                                                        }else if let Some(key)=navigation{
                                                            inline_navigation_request=Some((target.clone(),key));
                                                        }else if crate::input::reception_numeric_edit_finished(&result,step_interaction){
                                                            inline_commit=true;
                                                        }
                                                    }
                                                    if required.contains(item_id,field){
                                                        crate::input::paint_required_input(items_ui,response.rect);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    });
                                    if let Some(recipient)=selected {
                                        let recipient_id=reception_service::text(recipient,"id");
                                        crate::ui_layout::reception_items_to_freight_gap(items_ui);
                                        // The ORDER total is not a recipient fee.
                                        // Show only the native per-recipient amount.
                                        // Engine result (auto suggestion +
                                        // explicit same-address staff override)
                                        // is authoritative for the CHECKED UI.
                                        // The previous code read only the
                                        // optional manual remoteDelivery field,
                                        // hiding the checkbox for auto zones.
                                        let remote_result=self.analysis.as_ref()
                                            .and_then(|analysis|
                                                analysis.reference.get(visible_index))
                                            .and_then(|reference|reference.remote.as_ref())
                                            .filter(|result|
                                                reception_service::remote_reference_matches(
                                                    recipient,result
                                                )
                                            );
                                        let remote=remote_result.and_then(|result|
                                            result["checked"].as_bool()
                                        );
                                        let origin=recipient.get("dispatchOrigin")
                                            .and_then(serde_json::Value::as_str);
                                        let line_fee=indexed_billable_freight(quote_index.as_deref(),recipient);
                                        let cod=recipient.get("freightCode")
                                            .and_then(serde_json::Value::as_str)
                                            .map(|code|code=="020");
                                        let freight_target=InlineTarget{
                                            recipient_id:recipient_id.to_owned(),
                                            item_id:None,field:"shippingCharge"
                                        };
                                        // Adding products belongs to the section header,
                                        // not to the scrollable freight and item content.
                                        let freight_actions=crate::composite_reception::reception_freight_row_controls(
                                            items_ui,"운임",line_fee,cod,remote,
                                            true,editable,editable,editable,true,
                                            indexed_freight_exempt(quote_index.as_deref(),recipient)
                                        );
                                        if freight_actions.amount_clicked{
                                            inline_open=Some((
                                                freight_target.clone(),
                                                line_fee.map(|n|n.to_string()).unwrap_or_default(),
                                                freight_actions.amount_click_pos
                                            ));
                                        }
                                        if freight_actions.remote_clicked{
                                            inline_command=Some((
                                                InlineTarget{
                                                    recipient_id:recipient_id.to_owned(),
                                                    item_id:None,field:"remoteDelivery"
                                                },
                                                (!remote.unwrap_or(false)).to_string()
                                            ));
                                        }
                                        if freight_actions.collect_clicked{
                                            inline_command=Some((
                                                InlineTarget{
                                                    recipient_id:recipient_id.to_owned(),
                                                    item_id:None,field:"freightCode"
                                                },
                                                if cod.unwrap_or(false){"010"}else{"020"}.to_owned()
                                            ));
                                        }
                                        if let (Some(editor),Some(rect))=(
                                            self.inline_editor.as_mut()
                                                .filter(|edit|edit.target==freight_target && cod!=Some(true)),
                                            freight_actions.amount_rect
                                        ){
                                            let navigation=cell_navigation::consume_worksheet_navigation_before_editor(items_ui.ctx(),inline_editor_slot(&freight_target));
                                            let result=crate::input::reception_freight_inline_cell_editor(
                                                items_ui,rect,&mut editor.text,editor.focus_next,
                                                editor.click_at.take()
                                            );
                                            cell_navigation::remember_worksheet_editor(
                                                items_ui.ctx(),inline_editor_slot(&freight_target),
                                                result.id
                                            );
                                            editor.focus_next=false;
                                            Self::show_inline_validation_focus(
                                                &mut self.inline_validation_focus,&freight_target,&result,items_ui.ctx());

                                            if overlay::take_cell_escape(&result,false){
                                                inline_cancel=true;
                                            }else if let Some(key)=navigation{
                                                inline_navigation_request=Some((freight_target.clone(),key));
                                            }else if cell_navigation::worksheet_blur_finished(&result,false){
                                                inline_commit=true;
                                            }
                                        }
                                        items_ui.add_space(theme::SPACE_8);
                                        if let Some(value)=
                                            crate::composite_reception::reception_item_origin_action_editable(
                                                items_ui,origin,editable
                                            )
                                        {
                                            inline_command=Some((
                                                InlineTarget{
                                                    recipient_id:recipient_id.to_owned(),
                                                    item_id:None,field:"dispatchOrigin"
                                                },value.to_owned()
                                            ));
                                        }
                                    } else {
                                        crate::status_notice::management_body_state_overlay(
                                            items_ui,items_ui.available_rect_before_wrap(),
                                            Some("거래처 선택 후 운임과 청구금액을 확인합니다.")
                                        );
                                    }
                        }
                    );
                    // The goods section does not own the entire-order VAT
                    // policy or total; those are docked in the card below.
                });
                if header_add_item{
                    if let Some(recipient)=selected{
                        inline_command=Some((
                            InlineTarget{
                                recipient_id:reception_service::text(recipient,"id").to_owned(),
                                item_id:None,field:"addItem"
                            },"".to_owned()
                        ));
                    }
                }
            },
        );
        ui.scope_builder(
            UiBuilder::new().id_salt("details-order-total")
                .max_rect(summary_rect).layout(Layout::top_down(Align::Min)),
            |pane|{
                pane.set_clip_rect(summary_rect);
                pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,summary_rect.width());
                pane.set_height(summary_rect.height());
                let mut edit_vat=false;
                Self::section_with_header(pane,"발주서 합계",summary_rect.height(),
                    |header|{
                        let response=crate::button::action(header,"부가세 설정",ButtonRole::Standard,
                            editable && self.analysis.is_some()
                                && self.inline_editor.is_none()
                                && self.vat_change.is_none());
                        if one_touch::stage(header)==one_touch::Stage::Review{one_touch::mark(header,response.rect);}
                        if response.clicked(){edit_vat=true;}
                    },|body|{
                    let preview=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|draft|draft.id==id))
                        .and_then(|draft|draft.pricing_preview.as_ref())
                        .filter(|quote|quote.get("sourceRaw")
                            .and_then(serde_json::Value::as_str)
                            ==Some(self.raw_source.as_str()));
                    let total=preview.and_then(|v|v.get("total"))
                        .and_then(serde_json::Value::as_i64);
                    let recipients=self.analysis.as_ref()
                        .map(|analysis|reception_service::recipients(analysis));
                    let shipment_count=recipients
                        .filter(|rows|!rows.is_empty()).map(|rows|rows.len());
                    let vat_label=recipients.and_then(|rows|{
                        if rows.is_empty(){return None;}
                        let inclusive=rows.iter().filter(|row|
                            gateway::rules::vat_applies(&serde_json::Value::Null,row)
                        ).count();
                        Some(if inclusive==rows.len(){"부가세 포함"}
                            else if inclusive>0{"부가세 일부 포함"}
                            else{"부가세 별도"})
                    }).unwrap_or("—");
                    crate::composite_reception::reception_total_footer(
                        body,total,shipment_count,vat_label
                    );

                    });
                if edit_vat{self.open_vat_settings();}
            }
        );
        // Only authoritative Rust source spans may drive a linked hover.
        // A recognized field with no source evidence must not flash blue as
        // though a corresponding token were found in the original document.
        // Source and recipient/item panes share 50ms enter / 150ms exit;
        // working-order overlay previews deliberately retain 250ms timing.
        let source_matches=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
            .is_some_and(|row|
                row.analysis_raw.as_deref()==Some(self.raw_source.as_str())
            );
        let linked=hovered_detail.or_else(||self.source_hover.clone())
            .filter(|key|source_matches && self.analysis.as_ref()
                .is_some_and(|analysis|analysis.linked_spans.iter()
                    .any(|span|span.key==*key)));
        self.link_hover.update_for(
            self.active_id.as_deref().unwrap_or(""),linked,ui.ctx()
        );
        // React parity: hovering linked source evidence only changes the
        // visible projection; it must not replace the clicked recipient.
        if self.source_hover.is_some() && self.link_hover.active.is_some(){
            self.link_hover.scroll_to=None;
        }
        if let Some((recipient_id,item_id))=item_remove_popup{
            if self.inline_editor.as_ref().is_some_and(|edit|edit.text!=edit.original){
                self.submit_inline_edit(ui.ctx());
                if let Some(pending)=self.inline_change.as_mut(){
                    pending.next=Some((InlineTarget{
                        recipient_id,item_id:Some(item_id),field:"removeItem"
                    },None));
                }
            }else{
                self.open_item_remove_confirmation(&recipient_id,&item_id,ui.ctx());
            }
        }else if let Some((recipient_id,item_id))=item_popup{
            if self.inline_editor.as_ref().is_some_and(|edit|
                edit.text!=edit.original){
                // Respect the first-click handoff used by the other inline
                // cells: commit the current edit before showing the modal.
                self.submit_inline_edit(ui.ctx());
                if let Some(pending)=self.inline_change.as_mut(){
                    pending.next=Some((InlineTarget{
                        recipient_id,
                        item_id:Some(item_id),field:"product"
                    },None));
                }
            }else{
                self.inline_editor=None;
                self.open_item_editor(&recipient_id,&item_id,ui.ctx());
            }
        }else if inline_cancel{
            self.inline_validation_focus=None;
            self.guided_inline_target=None;
            self.pending_input_guidance=None;
            self.inline_editor=None;
        }else if let Some((target,text,click_at))=inline_open {
            // B's click and A's blur happen on the same egui frame.
            // Click intent must win over the A blur, otherwise B needs
            // a second click after the pending native save.
            self.switch_inline_edit(target,text,click_at,ui.ctx());
        }else if let Some((current,key))=inline_navigation_request{
            // Vertical navigation bridges visible sections; Enter also
            // confirms the current value if no eligible destination exists.
            let inline_navigation=self.analysis.as_ref()
                .and_then(|analysis|reception_service::recipients(analysis).get(visible_index))
                .map(|recipient|inline_navigation_entries(recipient,editable)).unwrap_or_default();
            if let Some(next)=resolve_inline_navigation(&inline_navigation,&current,key){
                let quote=self.active_id.as_deref()
                    .and_then(|id|self.drafts.iter().find(|draft|draft.id==id))
                    .and_then(|draft|draft.pricing_preview.as_ref())
                    .filter(|quote|quote["sourceRaw"].as_str()==Some(self.raw_source.as_str()));
                let original=self.analysis.as_ref().and_then(|analysis|
                    current_inline_value(&analysis.recognition,quote,&next)
                ).unwrap_or_default();
                self.switch_inline_edit(next,original,None,ui.ctx());
            }else if key.commits_at_boundary(){
                self.submit_inline_edit(ui.ctx());
            }
        }else if inline_commit{
            self.submit_inline_edit(ui.ctx());
        }else if let Some((target,value))=inline_command{
            if target.field=="addItem"{
                self.open_product_picker(target.recipient_id,ui.ctx());
            }else if self.inline_editor.is_none() && self.inline_change.is_none(){
                let value=if target.field=="remoteDelivery"{
                    serde_json::json!(value=="true")
                }else{
                    serde_json::json!(value)
                };
                self.start_review_inline_action(target,value,ui.ctx());
            }
        }
    }

    /// A first-stage approval is also an explicit request to commit the
    /// currently edited cell. Do not reject the user's action or confirm a
    /// stale quote: submit the cell, wait for its native reducer result, then
    /// continue through start_draft_confirmation().
    fn request_draft_confirmation(&mut self,order_id:&str,ctx:&egui::Context){
        if self.pending_draft_delete.as_ref().is_some_and(|(id,_)|id==order_id){
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Info,
                "발주서 삭제를 저장하고 있습니다.");
            return;
        }
        if self.confirming_order().is_some(){return;}
        // The pending marker is valid only while its matching native cell
        // reducer is still alive. Recover stale markers for ANY order rather
        // than permanently blocking the confirmation button.
        if let Some(pending_id)=self.pending_draft_confirmation.clone(){
            let save_is_running=self.inline_change.as_ref().is_some_and(|pending|
                pending.order_id==pending_id
            );
            if save_is_running{
                let message=if pending_id==order_id{
                    "셀 저장 중입니다. 완료되면 주문 확정을 자동으로 이어갑니다."
                }else{
                    "다른 발주서의 셀 저장이 진행 중입니다."
                };
                floating_notice::publish(
                    ctx,"reception.workflow",floating_notice::Tone::Info,message
                );
                return;
            }
            self.pending_draft_confirmation=None;
        }

        if let Some(pending)=self.inline_change.as_mut(){
            if pending.order_id==order_id{
                pending.next=None;
                self.pending_draft_confirmation=Some(order_id.to_owned());
                floating_notice::publish(
                    ctx,"reception.workflow",floating_notice::Tone::Info,
                    "셀 저장 중입니다. 완료되면 주문 확정을 자동으로 이어갑니다."
                );
                ctx.request_repaint();
                return;
            }
        }
        if self.active_id.as_deref()==Some(order_id) &&
            self.inline_editor.as_ref().is_some_and(|editor|
                editor.source_id==order_id
            ){
            self.pending_draft_confirmation=Some(order_id.to_owned());
            self.submit_inline_edit(ctx);
            if self.inline_change.is_some(){
                floating_notice::publish(
                    ctx,"reception.workflow",floating_notice::Tone::Info,
                    "셀 저장 중입니다. 완료되면 주문 확정을 자동으로 이어갑니다."
                );
                ctx.request_repaint();
                return;
            }
            // Validation/admission failures keep the edit in place. Report
            // the actual reason; never leave the employee with a silent no-op.
            if let Some(error)=self.source_save_error.clone(){
                self.pending_draft_confirmation=None;
                if self.inline_validation_focus.is_none(){
                    floating_notice::publish(
                        ctx,"reception.workflow",floating_notice::Tone::Error,
                        format!("셀 저장에 실패해 주문 확정을 중단했습니다. {error}")
                    );
                }
                ctx.request_repaint();
                return;
            }
            if self.inline_editor.is_some(){
                self.pending_draft_confirmation=None;
                floating_notice::publish(
                    ctx,"reception.workflow",floating_notice::Tone::Error,
                    "현재 셀 입력을 저장하지 못했습니다. 입력 내용을 확인한 뒤 다시 시도하세요."
                );
                ctx.request_repaint();
                return;
            }
            self.pending_draft_confirmation=None;
            self.start_draft_confirmation(order_id,ctx);
            return;
        }
        self.start_draft_confirmation(order_id,ctx);
    }

    /// Convert an exact, freshly repriced egui draft into a real order only
    /// when the employee clicks the first workflow stage. Native reconciliation
    /// overlays the full Rust Order and commits confirmation + workflow in one
    /// revision-checked SQLite transaction.
    fn start_draft_confirmation(&mut self,order_id:&str,ctx:&egui::Context){
        match self.reception_job_admission(ReceptionJob::WorkflowAction,false){
            JobAdmission::Conflict(blocker)=>{
                floating_notice::publish(
                    ctx,"reception.workflow",floating_notice::Tone::Error,
                    format!("{} 중에는 주문을 확정할 수 없습니다.",blocker.label())
                );
                return;
            }
            JobAdmission::Start=>{}
            JobAdmission::Duplicate=>return,
        }
        if !self.gateway.workflow_write_enabled(){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "주문 확정을 허용한 기존 Tauri 업무 DB가 필요합니다."
            );
            return;
        }
        let Some(source)=self.drafts.iter().find(|draft|draft.id==order_id).cloned()
        else{
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "확정할 미확정 발주서를 찾을 수 없습니다."
            );
            return;
        };
        let Some(client_id)=source.client_id.clone()
            .filter(|id|!id.trim().is_empty()) else{
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "거래처를 먼저 선택하세요."
            );
            return;
        };
        let Some(quote)=source.pricing_preview.as_ref() else{
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "거래처의 품목·운임 계산이 완료된 뒤 주문을 확정할 수 있습니다."
            );
            return;
        };
        if !confirmable_reception_quote(&source,quote){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "현재 발주서의 금액을 확인할 수 없습니다. 거래처를 다시 선택해 계산하세요."
            );
            return;
        }
        let Some(expected_revision)=quote.get("authorityRevision")
            .and_then(serde_json::Value::as_u64) else{
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "최신 가격 정보를 확인하지 못했습니다. 다시 계산하세요."
            );
            return;
        };
        let group_id=format!("group:{}",source.id);
        if self.workflow_stage_deadlines.get(&group_id)
            .is_some_and(|deadline|Instant::now()<*deadline){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "주문 확인 직후입니다. 잠시 후 다시 진행하세요."
            );
            return;
        }
        if self.workflow_read.required(){
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Info,
                "저장된 주문 상태를 갱신하고 있습니다. 갱신 후 주문을 확정하세요.");
            return;
        }
        let expected_quote=quote.clone();
        self.discard_advisory_workflow_read();
        let gateway=Arc::clone(&self.gateway);
        let target_id=client_id.clone();
        let worker_group_id=group_id.clone();
        let (sender,receiver)=mpsc::channel();
        let context=ctx.clone();
        self.workflow_action=Some(PendingWorkflowAction{
            group_id:group_id.clone(),
            action:"confirm_reception".into(),
            notice_origin:floating_notice::capture_origin(ctx,"reception.workflow"),
            receiver
        });
        std::thread::spawn(move||{
            let result=(||->Result<serde_json::Value,String>{
                // Re-run the same native dealer recognition and shipment-aware
                // quote at approval time. A changed master revision or changed
                // line/freight result must never confirm the stale screen.
                let prepared=native_order_change::prepare(
                    gateway.as_ref(),&source,&target_id
                )?;
                let fresh_quote=prepared.pricing_preview.as_ref()
                    .ok_or("운영 거래처 기준의 가격 계산 결과가 없습니다.")?;
                if !confirmable_reception_quote(&source,fresh_quote) ||
                    fresh_quote!=&expected_quote ||
                    prepared.client_id!=target_id{
                    return Err("거래처·상품·운임 기준 또는 계산 결과가 변경되었습니다. 거래처를 다시 선택해 최신 금액을 확인하세요.".into());
                }
                let order=prepared.order
                    .ok_or("확정할 주문 정보를 확인할 수 없습니다.")?;
                if order.get("id").and_then(serde_json::Value::as_str)
                        !=Some(source.id.as_str()) ||
                   order.get("raw").and_then(serde_json::Value::as_str)
                        !=Some(source.raw.as_str()) ||
                   order.get("clientId").and_then(serde_json::Value::as_str)
                        !=Some(target_id.as_str()) ||
                   order.get("orderType").and_then(serde_json::Value::as_str)
                        !=Some(source.order_type.as_str()){
                    return Err("최종 주문 데이터가 현재 미확정 발주서와 일치하지 않습니다.".into());
                }
                let args=serde_json::json!({
                    "groupId":worker_group_id,
                    "draft":order
                });
                gateway.mutate_workflow(
                    expected_revision,"advance_group",args
                )
            })();
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    /// Run an approved group mutation off the egui frame. The gateway calls
    /// native Rust rules, writes v2 SQLite under revision CAS and rereads it.
    /// No preview receipt or copied UI state is ever used as a write source.
    fn start_workflow_action(
        &mut self,action:&str,group_id:&str,shipment_id:Option<&str>,
        target_group_id:Option<&str>,ack:bool,ctx:&egui::Context
    ){
        self.start_workflow_action_with_args(
            action,group_id,shipment_id,target_group_id,ack,
            serde_json::Value::Null,ctx
        );
    }

    fn start_workflow_action_with_args(
        &mut self,action:&str,group_id:&str,shipment_id:Option<&str>,
        target_group_id:Option<&str>,ack:bool,extra:serde_json::Value,
        ctx:&egui::Context
    ){
        if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
            ReceptionJob::WorkflowAction,false
        ){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                format!("{} 중에는 주문 업무를 변경할 수 없습니다.",blocker.label())
            );
            return;
        }
        if !self.gateway.workflow_write_enabled(){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "주문 변경을 허용한 기존 Tauri 업무 DB가 필요합니다."
            );
            return;
        }
        if matches!(action,"advance_group"|"confirm_group_payment"|"process_group")
            && self.workflow_stage_deadlines.get(group_id)
                .is_some_and(|deadline|Instant::now()<*deadline){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "단계 변경 직후입니다. 잠시 후 다시 진행하세요."
            );
            return;
        }
        let Some(revision)=self.workflow_snapshot.as_ref()
            .and_then(|snapshot|snapshot.revision) else{
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                "주문 상태를 다시 읽은 후 진행하세요."
            );
            return;
        };
        if !self.workflow_mutation_ready(){
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Validation,
                "저장된 주문 상태를 다시 확인한 뒤 진행하세요.");
            return;
        }
        self.discard_advisory_workflow_read();
        let mut args=serde_json::json!({"groupId":group_id});
        if let Some(shipment_id)=shipment_id{
            args["shipmentId"]=serde_json::json!(shipment_id);
        }
        if let Some(target_group_id)=target_group_id{
            args["targetGroupId"]=serde_json::json!(target_group_id);
        }
        if ack{args["ack"]=serde_json::json!(true);}
        if let Some(values)=extra.as_object(){
            for (key,value) in values{
                args[key.as_str()]=value.clone();
            }
        }
        // A successful admission for an explicit stage/copy/undo action
        // commits the displayed recipient before the asynchronous write.
        if matches!(action,"advance_group"|"confirm_group_payment"|"process_group"|
            "undo_group"|"record_message_copy"){
            self.retain_workflow_stage_target(group_id);
        }
        let gateway=Arc::clone(&self.gateway);
        let (sender,receiver)=mpsc::channel();
        let context=ctx.clone();
        let operation=action.to_owned();
        self.workflow_action=Some(PendingWorkflowAction{
            group_id:group_id.to_owned(),action:operation.clone(),
            notice_origin:floating_notice::capture_origin(ctx,match action{
                "set_shipment_freight"=>"reception.freight",
                "record_message_copy"=>"reception.automatic.copy",
                _=>"reception.workflow",
            }),receiver
        });
        std::thread::spawn(move||{
            let result=gateway.mutate_workflow(revision,&operation,args);
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    fn submit_workflow_freight_edit(&mut self,ctx:&egui::Context)->bool{
        let Some(mut editor)=self.workflow_freight_editor.take() else{return true;};
        floating_notice::inherit_origin(ctx,"reception.cell","reception.freight");
        let digits=editor.text.trim().replace(',',"");
        let original_digits=editor.original.trim().replace(',',"");
        if digits==original_digits{
            self.workflow_freight_guidance=None;
            return true;
        }
        let amount=if digits.is_empty(){
            serde_json::Value::Null
        }else{
            let parsed=digits.parse::<i64>();
            let Some(value)=parsed.ok().filter(|value|(0..=99_999).contains(value)) else{
                floating_notice::publish(
                    ctx,"reception.freight",floating_notice::Tone::Error,
                    "운임은 0~99,999원 사이의 정수로 입력하세요."
                );
                editor.focus_next=true;
                self.workflow_freight_editor=Some(editor);
                self.workflow_freight_next_selection=None;
                return false;
            };
            serde_json::json!(value)
        };
        let was_idle=self.workflow_action.is_none();
        self.start_workflow_action_with_args(
            "set_shipment_freight",&editor.group_id,Some(&editor.shipment_id),
            None,false,serde_json::json!({
                "origin":editor.origin,"appliedCharge":amount
            }),ctx
        );
        let started=was_idle && self.workflow_action.as_ref().is_some_and(|pending|
            pending.action=="set_shipment_freight"
        );
        if started{
            self.submitted_workflow_freight=Some(editor);
        }else{
            editor.focus_next=true;
            self.workflow_freight_editor=Some(editor);
            self.workflow_freight_next_selection=None;
        }
        started
    }

    fn restore_failed_workflow_freight(&mut self){
        if let Some(mut editor)=self.submitted_workflow_freight.take(){
            // The user may have navigated while the independent write ran.
            // Restore the exact failed shipment, not an unrelated visible row.
            if let Some(order_id)=self.workflow_snapshot.as_ref()
                .and_then(|board|board.groups.iter().find(|g|g.id==editor.group_id))
                .and_then(|g|g.shipments.iter().find(|s|s.id==editor.shipment_id))
                .map(|s|s.order_id.clone()){
                self.workflow_ui.select(workflow_board_ui::Selection{
                    group_id:editor.group_id.clone(),order_id,
                    shipment_id:editor.shipment_id.clone()
                });
                self.link_hover.clear();
                self.source_hover=None;
            }
            editor.focus_next=true;
            self.workflow_freight_editor=Some(editor);
        }
    }

    fn poll_workflow_action(&mut self,ctx:&egui::Context){
        let outcome=self.workflow_action.as_ref().map(|task|(
            task.group_id.clone(),task.action.clone(),task.notice_origin.clone(),task.receiver.try_recv()
        ));
        let Some((group_id,action,notice_origin,result))=outcome else{return;};
        match result{
            Ok(Ok(mut saved))=>{
                self.workflow_action=None;
                if action=="edit_confirmed_recipient"{self.submitted_confirmed_cell=None;}
                let scoped_refresh=saved.as_object_mut()
                    .and_then(|root|root.remove("_scopedShipmentRefresh"))
                    .and_then(|flag|flag.as_bool()).unwrap_or(false);
                if action=="set_shipment_freight"{
                    self.submitted_workflow_freight=None;
                    self.workflow_freight_editor=None;
                    if let Some(selection)=self.workflow_freight_next_selection.take(){
                        self.workflow_ui.select(selection);
                    }
                }
                if matches!(action.as_str(),
                    "advance_group"|"confirm_group_payment"|"process_group"|
                    "confirm_reception"
                ){
                    self.workflow_stage_deadlines.insert(
                        group_id.clone(),Instant::now()+Duration::from_secs(2)
                    );
                    ctx.request_repaint_after(Duration::from_secs(2));
                }
                // Keep the previous selection until the verified projection
                // arrives, even for deletion/regrouping. Reconcile by stable ID.
                if action=="confirm_reception"{
                    self.committed_draft_handoff=Some(group_id.clone());
                }
                // A confirmation closes only on its own committed write.
                if (action=="delete_group" && matches!(self.workflow_dialog.as_ref(),
                    Some(WorkflowDialog::DeleteGroup(id)) if id==&group_id)) ||
                    (action=="advance_group" && matches!(self.workflow_dialog.as_ref(),
                    Some(WorkflowDialog::PaymentNotice(id)) if id==&group_id)){
                    self.workflow_dialog=None;
                }
                if matches!(action.as_str(),"detach_shipment"|"regroup_shipment")
                    && matches!(self.workflow_dialog.as_ref(),
                        Some(WorkflowDialog::ManageGroup(id)) if id==&group_id){
                    self.workflow_dialog=None;
                    self.workflow_manage_target.clear();
                    self.workflow_manage_ack=false;
                }
                self.workflow_ui.stop_preview();
                // The gateway already verified the committed DB snapshot.
                // Project it off-frame rather than fetching that DB a third
                // time. Keep dependent actions locked until projection ends.
                // External changes are still detected by periodic polling/CAS.
                self.workflow_read.cancel();
                self.workflow_read.require();
                self.workflow_last_fetch=None;
                self.workflow_error=None;
                self.workflow_read_error=None;
                if saved["revision"].as_u64().is_some() && saved["workflow"].is_object(){
                    let gateway=Arc::clone(&self.gateway);
                    self.workflow_last_fetch=Some(Instant::now());
                    let changed_group=(scoped_refresh &&
                        matches!(action.as_str(),"set_shipment_freight"|"detach_shipment"|"regroup_shipment"))
                        .then(||group_id.clone());
                    let previous=changed_group.as_ref().and_then(|_|self.workflow_snapshot.as_deref().cloned());
                    self.workflow_read.submit_required(ctx.clone(),move||{
                        let result=if let Some(id)=changed_group{
                            workflow_board::after_freight_edit(gateway.as_ref(),saved,previous,&id)
                        }else{
                            workflow_board::from_state(gateway.as_ref(),saved)
                        };
                        result.map(Some)
                    });
                }
                if action=="process_group"{
                    if let Some(excel)=self.excel_manager.as_mut(){
                        excel.refresh(ctx.clone());
                    }
                }
                floating_notice::publish_from(
                    ctx,"reception.workflow",floating_notice::Tone::Success,
                    match action.as_str(){
                        "record_message_copy"=>"클립보드 복사 및 업무 이력 저장 완료.",
                        "advance_group"=>"업무 단계를 진행했습니다.",
                        "confirm_reception"=>"주문 확인을 완료하고 정식 주문으로 확정했습니다.",
                        "confirm_group_payment"=>"입금 확인 단계를 완료했습니다.",
                        "undo_group"=>"이전 단계로 변경했습니다.",
                        "process_group"=>"Excel 관리 대기열로 이관했습니다.",
                        "delete_group"=>"주문 묶음을 삭제했습니다. 업무 이력은 보존됩니다.",
                        "detach_shipment"=>"선택한 배송 건을 분리했습니다.",
                        "regroup_shipment"=>"선택한 배송 건을 다시 묶었습니다.",
                        _=>"주문 변경 내용을 저장했습니다."
                    },notice_origin.as_ref()
                );
                ctx.request_repaint();
            }
            Ok(Err(error))=>{
                self.workflow_action=None;
                if matches!(action.as_str(),"edit_confirmed_recipient"|"set_shipment_freight"){
                    self.confirmed_next_cell=None;
                }
                if action=="edit_confirmed_recipient"{
                    if let Some(mut editor)=self.submitted_confirmed_cell.take(){
                        editor.cell.focus_next=true;
                        self.confirmed_cell=Some(editor);
                    }
                }
                if action=="set_shipment_freight"{
                    self.workflow_freight_next_selection=None;
                    self.workflow_stage_after_freight=None;
                    self.restore_failed_workflow_freight();
                }
                self.workflow_last_fetch=None;
                self.workflow_error=Some(error.clone());
                if action=="confirm_reception"{
                    self.restore_failed_stage_target(&group_id,ctx);
                }
                let routed=action=="confirm_reception" && group_id.strip_prefix("group:")
                    .is_some_and(|order_id|self.focus_failed_draft_input(order_id,&error,ctx,notice_origin.clone()));
                if !routed{
                    floating_notice::publish_from(
                        ctx,"reception.workflow",floating_notice::Tone::Error,error,notice_origin.as_ref()
                    );
                }
            }
            Err(mpsc::TryRecvError::Disconnected)=>{
                self.workflow_action=None;
                if matches!(action.as_str(),"edit_confirmed_recipient"|"set_shipment_freight"){
                    self.confirmed_next_cell=None;
                }
                if action=="edit_confirmed_recipient"{
                    if let Some(mut editor)=self.submitted_confirmed_cell.take(){
                        editor.cell.focus_next=true;
                        self.confirmed_cell=Some(editor);
                    }
                }
                if action=="set_shipment_freight"{
                    self.workflow_freight_next_selection=None;
                    self.workflow_stage_after_freight=None;
                    self.restore_failed_workflow_freight();
                }
                self.workflow_last_fetch=None;
                self.workflow_error=Some("주문 변경 처리 도중 작업이 중단되었습니다.".into());
                if action=="confirm_reception"{
                    self.restore_failed_stage_target(&group_id,ctx);
                }
                floating_notice::publish_from(
                    ctx,"reception.workflow",floating_notice::Tone::Error,
                    "주문 변경 처리 도중 작업이 중단되었습니다.",notice_origin.as_ref()
                );
            }
            Err(mpsc::TryRecvError::Empty)=>{}
        }
    }

    /// A pending receipt is only an isolated unconfirmed workspace record.
    /// Never pretend this deletes an already persisted operational order.
    fn delete_pending_reception(&mut self,id:&str,ctx:&egui::Context)->bool{
        if self.confirming_order()==Some(id){return false;}
        let Some(draft)=self.drafts.iter().find(|row|row.id==id) else{return false;};
        // Older builds could explicitly write a separate egui bridge copy.
        // Never turn that unrelated metadata into a ban on deleting the
        // isolated intake draft; also never pretend to delete the other DB.
        let prior_bridge_copy=draft.working_version.is_some();
        if let JobAdmission::Conflict(blocker)=self.reception_job_admission(
            ReceptionJob::DeleteReception,false
        ){
            floating_notice::publish(
                ctx,"reception.workflow",floating_notice::Tone::Error,
                format!("{} 중에는 발주서를 삭제할 수 없습니다.",blocker.label())
            );
            return false;
        }
        let queued=self.source_snapshot().and_then(|source|{
            self.source_autosave.as_ref()
                .ok_or_else(||"격리 원문 저장소가 초기화되지 않았습니다.".to_owned())?
                .queue_delete(source,id.to_owned())
        });
        if let Err(error)=queued{
            self.source_save_error=Some(error.clone());
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Error,
                format!("발주서 삭제 저장 실패: {error}"));
            return false;
        }
        // The draft remains visible and intact while the worker commits.
        // This also preserves the exact active input on a write failure.
        self.pending_draft_delete=Some((id.to_owned(),prior_bridge_copy));
        self.workflow_dialog=None;
        self.workflow_ui.stop_preview();
        true
    }

    fn finish_pending_draft_delete(&mut self,id:&str,result:Result<(),String>,ctx:&egui::Context){
        if !self.pending_draft_delete.as_ref().is_some_and(|(pending,_)|pending==id){return;}
        let (_,prior_bridge_copy)=self.pending_draft_delete.take().expect("matched deletion");
        if let Err(error)=result{
            self.source_save_error=Some(error.clone());
            floating_notice::publish(ctx,"reception.workflow",floating_notice::Tone::Error,
                format!("발주서 삭제 저장 실패: {error}"));
            return;
        }
        let was_active=self.active_id.as_deref()==Some(id);
        self.drafts.retain(|row|row.id!=id);
        self.pending_display=None;
        self.pending_review_panel.clear();
        if was_active{
            self.client_change=None;
            self.inline_editor=None;
            self.item_editor=None;
            self.product_picker=None;
            self.active_id=None;
            self.raw_source.clear();
            self.source_editing=false;
            self.direct_source_entry=false;
            self.clear_source_analysis();
            if let Some(next)=self.drafts.last().map(|row|row.id.clone()){
                self.select_reception(&next,ctx);
            }
        }
        self.workflow_ui.stop_preview();
        floating_notice::publish(ctx,"reception.workflow",
            if prior_bridge_copy{floating_notice::Tone::Info}else{floating_notice::Tone::Success},
            if prior_bridge_copy{"접수 목록에서 발주서를 삭제했습니다. 이전 저장 기록은 보존됩니다."}
            else{"미확정 발주서를 삭제했습니다."});
        ctx.request_repaint();
    }

    /// Use the common centered/dimmed popup for every card action. The
    /// modal owns no prices or operational state; it only selects validated
    /// IDs and emits native workflow commands after the employee clicks.
    fn show_workflow_dialog(&mut self,ctx:&egui::Context,work_area:Rect){
        let Some(dialog)=self.workflow_dialog.clone() else{return;};
        let mut close=false;
        let mut delete_pending:Option<String>=None;
        let mut select_pending:Option<String>=None;
        let mut command:Option<(
            String,String,Option<String>,Option<String>,bool
        )>=None;
        let busy=self.workflow_action.is_some() || self.workflow_read.required() ||
            self.workflow_error.is_some();
        let writable=self.gateway.workflow_write_enabled();
        let mut target=self.workflow_manage_target.clone();
        let mut ack=self.workflow_manage_ack;
        match dialog{
            WorkflowDialog::ManageGroup(id)=>{
                let group=self.workflow_snapshot.as_ref()
                    .and_then(|board|board.groups.iter().find(|g|g.id==id))
                    .cloned();
                if let Some(group)=group{
                    let targets=self.workflow_snapshot.as_ref()
                        .map(|board|board.groups.iter().filter(|other|
                            other.id!=id && !other.transferred &&
                            !group.source_id.is_empty() &&
                            !group.dealer_id.is_empty() &&
                            other.source_id==group.source_id &&
                            other.dealer_id==group.dealer_id
                        ).cloned().collect::<Vec<_>>()).unwrap_or_default();
                    if !target.is_empty() && !targets.iter().any(|g|g.id==target){
                        target.clear();ack=false;
                    }
                    let applying=self.workflow_action.is_some();
                    crate::popup::show_decision_window(ctx,"배송 건 묶음 관리","egui-workflow-group-manage",work_area,|ui|{
                        crate::popup::decision_workspace(ui,|decision|{
                            close=decision.header("배송 건 묶음 관리","닫기",!applying);
                            decision.body(|ui|{
                                let number=group.source_number.map(|n|format!("{n:03}"))
                                    .unwrap_or_else(||"—".to_owned());
                                ui.label(typography::inline_quantity_amount(
                                    &format!("발주 {number} / {} / 배송 ",group.dealer_name),
                                    group.shipments.len(),"건 / ",group.total,"금액 미확정",false
                                ));
                                crate::ui_primitives::section_divider(ui);
                                if !targets.is_empty(){
                                    let caption=targets.iter().find(|g|g.id==target)
                                        .map(|g|g.management_target_label())
                                        .unwrap_or_else(||typography::inline_body_values(&[
                                            ("같은 최초 발주서의 묶음 선택",None)
                                        ]));
                                    let previous=target.clone();
                                    crate::popup::modal_labeled_row_with_width(ui,"다시 묶을 대상",112.0,|line|{
                                        crate::dropdown::management_dropdown_values(
                                            line,&caption,
                                            line.available_width().max(1.0),!applying,
                                            !targets.iter().any(|g|g.id==target),|list|{
                                                crate::dropdown::management_dropdown_option(
                                                    list,&mut target,String::new(),"대상 선택");
                                                for other in &targets{
                                                    crate::dropdown::management_dropdown_option_values(
                                                        list,&mut target,other.id.clone(),
                                                        &other.management_target_label()
                                                    );
                                                }
                                            })
                                    });
                                    if target!=previous{ack=false;}
                                    crate::checkbox::checkbox(
                                        ui,&mut ack,
                                        "기존 안내액 / 입금 배분 / 변경 합계를 확인했습니다.",
                                        !applying
                                    );
                                }
                                crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                                    .id_salt("workflow-manage-confirmed-shipments")
                                    .max_height((work_area.height()*0.45).max(1.0))
                                    .auto_shrink([false,true])
                                    .show(ui,|list|{
                                        for (index,ship) in group.shipments.iter().enumerate(){
                                            if index>0{crate::ui_primitives::soft_divider(list);}
                                            list.push_id(&ship.id,|list|{
                                            list.label(typography::body(
                                                if ship.recipient_name.is_empty(){
                                                    "수취인 확인"
                                                }else{ship.recipient_name.as_str()}
                                            ));
                                            let step=ship.completed_step.map(|n|format!("{n}/4 완료"))
                                                .unwrap_or_else(||"단계 미확인".to_owned());
                                            list.label(typography::inline_quantity_amount(
                                                &format!("{step} / 상품 "),ship.products.len(),
                                                "종 / 입금 ",ship.paid,"—",true
                                            ));
                                            list.with_layout(Layout::right_to_left(Align::Center),|line|{
                                                if !targets.is_empty() && crate::button::action(
                                                    line,"다시 묶기",ButtonRole::Standard,
                                                    writable&&!busy&&!group.transferred&&!ship.transferred&&!target.is_empty()
                                                ).clicked(){
                                                    command=Some((
                                                        "regroup_shipment".to_owned(),
                                                        group.id.clone(),
                                                        Some(ship.id.clone()),
                                                        Some(target.clone()),ack
                                                    ));
                                                }
                                                if crate::button::action(
                                                    line,"이 배송 건 분리",ButtonRole::Standard,
                                                    writable&&!busy &&
                                                        group.shipments.len()>1 &&
                                                        !group.transferred && !ship.transferred
                                                ).clicked(){
                                                    command=Some((
                                                        "detach_shipment".to_owned(),
                                                        group.id.clone(),
                                                        Some(ship.id.clone()),None,false
                                                    ));
                                                }
                                            });
                                            });
                                        }
                                    });
                                ui.label(typography::caption(
                                    "배송 건 ID, 완료 이력, 입금 배분은 보존됩니다. 같은 최초 발주서와 거래처의 배송 건만 다시 묶을 수 있으며, 단계나 계좌가 다르거나 엑셀 이관된 배송 건은 합칠 수 없습니다."
                                ));
                                // Errors are already published by the native workflow
                                // completion handler using the original action anchor.
                                // Pending/read-only feedback must not grow this dialog.
                                let feedback=if applying{
                                    Some("배송 묶음 변경 내용을 저장하는 중입니다.")
                                }else if !writable{
                                    Some("현재 주문은 조회만 가능합니다.")
                                }else{None};
                                floating_notice::section(ctx,"reception.workflow.manage",ui.min_rect());
                                floating_notice::sync(ctx,"workflow.manage.feedback","reception.workflow.manage",
                                    floating_notice::Tone::Info,feedback);
                            });
                        });
                    });
                }else{close=true;}
            }
            WorkflowDialog::ManagePending(id)=>{
                let draft=self.drafts.iter().find(|row|row.id==id).cloned();
                if let Some(draft)=draft{
                    crate::popup::show_decision_window(ctx,"배송 건 묶음 관리","egui-workflow-pending-manage",work_area,|ui|{
                        crate::popup::decision_workspace(ui,|decision|{
                            close=decision.header("배송 건 묶음 관리","닫기",true
                            );
                            decision.body(|ui|{
                                ui.label(typography::body("거래처와 주문 내용을 확인한 후 주문 확인을 완료하면 배송 건을 분리하거나 다시 묶을 수 있습니다."));
                                crate::ui_primitives::section_divider(ui);
                                let rows=workflow_board_ui::pending_recipients(&draft);
                                if rows.is_empty(){
                                    ui.label(typography::body("수취인 확인"));
                                }
                                crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                                    .id_salt("workflow-manage-pending-recipients")
                                    .max_height((work_area.height()*0.45).max(1.0))
                                    .auto_shrink([false,true])
                                    .show(ui,|list|{
                                        // Pending recipients are single-line information, not
                                        // shipment action records. The shared body spacing is enough.
                                        for recipient in rows{
                                            let name=recipient.get("name")
                                                .and_then(serde_json::Value::as_str)
                                                .filter(|s|!s.is_empty())
                                                .unwrap_or("수취인 확인");
                                            let prefix=format!("{name} / 상품 ");
                                            let count=recipient.get("items")
                                                .and_then(serde_json::Value::as_array)
                                                .map(Vec::len).unwrap_or(0).to_string();
                                            list.label(typography::inline_body_values(&[
                                                (&prefix,None),(&count,Some(typography::DataValueKind::Quantity)),("종",None)
                                            ]));
                                        }
                                    });
                            });
                            decision.actions(|buttons|{
                                if crate::button::action(buttons,"거래처 확인하기",
                                    ButtonRole::Primary,true).clicked(){
                                    select_pending=Some(id.clone());
                                }
                            });
                        });
                    });
                }else{close=true;}
            }
            WorkflowDialog::DeleteGroup(id)=>{
                let group=self.workflow_snapshot.as_ref().and_then(|board|
                    board.groups.iter().find(|g|g.id==id)
                ).cloned();
                if let Some(group)=group{
                    let decision=crate::popup::show_delete_confirmation(
                        ctx,work_area,"egui-workflow-delete-group","주문 묶음 삭제","묶음 삭제",
                        self.workflow_action.is_none(),writable&&!busy&&!group.transferred,
                        crate::popup::DeleteBody::Plain,|ui|{
                            ui.label(format!("{} / 배송 {}건 묶음을 삭제할까요?",
                                group.dealer_name,group.shipments.len()));
                            ui.label("접수 원문과 감사 기록은 보존됩니다.");
                        }
                    );
                    close=decision==crate::popup::ConfirmDecision::Cancel;
                    if decision==crate::popup::ConfirmDecision::Confirm{
                        command=Some(("delete_group".into(),id.clone(),None,None,false));
                    }
                }else{close=true;}
            }
            WorkflowDialog::DeletePending(id)=>{
                if self.drafts.iter().any(|d|d.id==id){
                    let decision=crate::popup::show_delete_confirmation(
                        ctx,work_area,"egui-workflow-delete-pending","미확정 발주서 삭제","발주서 삭제",
                        true,!busy,crate::popup::DeleteBody::Plain,|ui|{
                            ui.label("미확정 발주서를 삭제할까요?");
                        }
                    );
                    close=decision==crate::popup::ConfirmDecision::Cancel;
                    if decision==crate::popup::ConfirmDecision::Confirm{delete_pending=Some(id.clone());}
                }else{close=true;}
            }
            WorkflowDialog::PaymentNotice(id)=>{
                crate::popup::show_decision_window(ctx,"계좌 안내 완료 확인","egui-workflow-payment-confirm",work_area,|ui|{
                    crate::popup::decision_workspace(ui,|decision|{
                        close=decision.header("계좌 안내 완료 확인","돌아가기",self.workflow_action.is_none()
                        );
                        decision.body(|ui|{
                            ui.label(
                                "계좌 안내 메시지를 복사한 기록이 없습니다. 다른 방법으로 안내하셨나요?"
                            );
                        });
                        decision.actions(|buttons|{
                            if crate::button::action(
                                buttons,"전달 완료로 처리",ButtonRole::Primary,
                                writable && !busy
                            ).clicked(){
                                command=Some((
                                    "advance_group".into(),id.clone(),
                                    None,None,true
                                ));
                            }
                        });
                    });
                });
            }
        }
        self.workflow_manage_target=target;
        self.workflow_manage_ack=ack;
        if let Some(id)=select_pending{
            // Navigation admission deliberately rejects open modal dialogs.
            // Release only this approved modal, not the general safety policy.
            let previous=self.workflow_dialog.take();
            if self.select_pending_workflow_card(&id,ctx){
                close=true;
            }else{
                self.workflow_dialog=previous;
                close=false;
            }
        }
        if let Some(id)=delete_pending{
            floating_notice::dispatch_pointer(ctx,"reception.workflow");
            // A conflict is not a dismissal: keep the employee's
            // confirmation context available for a retry.
            close=self.delete_pending_reception(&id,ctx);
        }
        if let Some((action,id,shipment,target,ack))=command{
            floating_notice::dispatch_pointer(ctx,"reception.workflow");
            self.start_workflow_action(
                &action,&id,shipment.as_deref(),target.as_deref(),ack,ctx
            );
            // Keep the approved target and decision on admission/write failure.
            // The matching committed response owns dismissal, not this click.
            close=false;
        }
        if close{self.workflow_dialog=None;}
    }

    /// Route a known native freight blocker to its exact input instead of
    /// attempting a guaranteed-to-fail mutation and locking the whole card.
    fn focus_first_missing_workflow_freight(&mut self,id:&str,ctx:&egui::Context)->bool{
        if !self.gateway.workflow_write_enabled() || !self.workflow_mutation_ready(){return false;}
        let target=self.workflow_snapshot.as_ref()
            .and_then(|board|board.groups.iter().find(|g|g.id==id && g.step==1 && !g.transferred))
            .and_then(workflow_board::first_missing_freight)
            .map(|(ship,origin)|(ship.id.clone(),ship.order_id.clone(),origin));
        let Some((shipment_id,order_id,origin))=target else{return false;};
        self.discard_advisory_workflow_read();
        self.workflow_ui.stop_preview();
        self.workflow_ui.select(workflow_board_ui::Selection{
            group_id:id.to_owned(),order_id,shipment_id:shipment_id.clone()
        });
        self.link_hover.clear();
        self.source_hover=None;
        self.workflow_freight_guidance=Some(id.to_owned());
        self.workflow_freight_editor=Some(WorkflowFreightEditor{
            group_id:id.to_owned(),shipment_id,origin,original:String::new(),
            text:String::new(),focus_next:true,click_at:None
        });
        ctx.request_repaint();
        true
    }

    // A pointer preview is not a selection, but clicking the action is an
    // explicit intent. Preserve its recipient and the base card geometry.
    fn retain_workflow_stage_target(&mut self,id:&str){
        let target=self.visible_workflow_group()
            .filter(|(board,g,_)|board.groups[*g].id==id)
            .map(|(board,g,s)|{
                let ship=&board.groups[g].shipments[s];
                workflow_board_ui::Selection{group_id:id.to_owned(),
                    order_id:ship.order_id.clone(),shipment_id:ship.id.clone()}
            })
            .or_else(||self.workflow_ui.selection.as_ref()
                .filter(|selected|selected.group_id==id).cloned())
            .or_else(||self.workflow_snapshot.as_ref()
                .and_then(|board|board.groups.iter().find(|group|group.id==id))
                .and_then(|group|group.shipments.first())
                .map(|ship|workflow_board_ui::Selection{group_id:id.to_owned(),
                    order_id:ship.order_id.clone(),shipment_id:ship.id.clone()}));
        let Some(target)=target else{return;};
        let keep_folded=self.workflow_ui.selection.as_ref()
            .is_none_or(|selected|selected.group_id!=id)
            || self.workflow_ui.folded.as_deref()==Some(id);
        self.pending_stage_display=None;
        self.workflow_ui.select(target);
        if keep_folded{self.workflow_ui.folded=Some(id.to_owned());}
        self.link_hover.clear();
        self.source_hover=None;
    }

    fn restore_failed_stage_target(&mut self,group_id:&str,ctx:&egui::Context){
        let target=self.pending_stage_display.take().filter(|target|
            group_id.strip_prefix("group:")==Some(target.order_id.as_str()));
        if let Some(target)=target.filter(|_|self.page==Page::Reception){
            if self.select_pending_workflow_card(&target.order_id,ctx){
                self.focus_pending_recipient(&target,ctx);
            }
        }
    }

    fn advance_workflow_group(&mut self,id:&str,ctx:&egui::Context){
        if !self.workflow_mutation_ready(){return;}
        if self.focus_first_missing_workflow_freight(id,ctx){return;}
        let Some(group)=self.workflow_snapshot.as_ref()
            .and_then(|board|board.groups.iter().find(|group|group.id==id))
        else{return;};
        if group.transferred{return;}
        let renotice_approved=group.issues.iter().any(|issue|
            issue["kind"]=="NOTICE" && issue["resolution"]=="USER_APPROVED");
        if (group.step==2 || (group.needs_notice && group.step>=2)) && !group.payment_copied && !renotice_approved{
            self.workflow_dialog=Some(WorkflowDialog::PaymentNotice(id.to_owned()));
        }else{
            let action=if group.step>=4 && !group.needs_notice{"process_group"}
                else if group.step==3 && !group.needs_notice{"confirm_group_payment"}
                else{"advance_group"};
            self.start_workflow_action(action,id,None,None,false,ctx);
        }
    }

    fn orders_pane(&mut self, ui: &mut Ui) {
        let height=ui.available_height();
        let mut event=None;
        Self::section(ui,"처리 중인 주문",height,|ui|{
            floating_notice::section(ui.ctx(),"reception.workflow.read",ui.clip_rect());
            // A continuous order-card list needs the same 10pt solid
            // scrollbar that management lists use, never a floating thumb.
            crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Table);
            crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                )
                .id_salt("working-receptions-list")
                .auto_shrink([false,false])
                .show(ui,|ui|{
                    let active_selected=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|draft|draft.id==id))
                        .and_then(|draft|draft.client_id.as_deref())
                        .zip(self.analysis.as_ref()
                            .and_then(|analysis|analysis.chosen_client_name.as_deref()));
                    let active_recipient=self.analysis.as_ref()
                        .and_then(|analysis|reception_service::recipients(analysis)
                            .get(self.selected_recipient))
                        .and_then(|row|row["id"].as_str());
                    let freight_stage_group=if self.gateway.workflow_write_enabled() &&
                        !self.workflow_read.required() && self.workflow_error.is_none() &&
                        self.workflow_stage_after_freight.is_none(){
                        self.workflow_freight_editor.as_ref()
                            .or(self.submitted_workflow_freight.as_ref())
                            .map(|editor|editor.group_id.as_str())
                    }else{None};
                    let writable=self.gateway.workflow_write_enabled() &&
                        self.workflow_mutation_ready() &&
                        self.workflow_freight_editor.is_none() &&
                        self.confirmed_cell.is_none() && self.confirmed_modal_target.is_none() &&
                        self.pending_draft_confirmation.is_none();
                    let reception_modal_open=self.reception_modal_open();
                    event=workflow_board_ui::show(
                        ui,self.workflow_snapshot.as_deref(),&self.drafts,
                        self.active_id.as_deref(),
                        &self.client_names,active_selected,
                        &mut self.workflow_ui,
                        writable,
                        freight_stage_group,
                        // Hover navigation must never switch the active receipt
                        // while a staff edit, dialog or pending write is open.
                        !reception_modal_open && self.workflow_action.is_none() &&
                        !self.workflow_read.required() && self.pending_draft_confirmation.is_none() &&
                        self.inline_editor.is_none() && self.inline_change.is_none() &&
                            self.manual_workspace.is_none() && self.vat_dialog.is_none() &&
                            self.vat_change.is_none() && self.client_chooser.is_none() &&
                            self.client_change.is_none() && self.working_sync.is_none() &&
                            self.workflow_dialog.is_none() &&
                            self.workflow_freight_editor.is_none() && self.confirmed_cell.is_none() && !self.ocr_processing,
                        active_recipient,
                        self.link_hover.scoped_active().map(|(order,key)|
                            (order,key.recipient_id.as_str()))
                    );
                });
        });
        if self.confirmed_cell.is_some() || self.confirmed_modal_target.is_some(){return;}
        match event{
            Some(workflow_board_ui::Event::ManageGroup(id))=>{
                self.workflow_ui.stop_preview();
                self.workflow_manage_target.clear();
                self.workflow_manage_ack=false;
                self.workflow_dialog=Some(WorkflowDialog::ManageGroup(id));
            }
            Some(workflow_board_ui::Event::DeleteGroup(id))=>{
                self.workflow_ui.stop_preview();
                self.workflow_dialog=Some(WorkflowDialog::DeleteGroup(id));
            }
            Some(workflow_board_ui::Event::AdvanceGroup(id))=>{
                self.retain_workflow_stage_target(&id);
                let editing=self.workflow_freight_editor.as_ref()
                    .or(self.submitted_workflow_freight.as_ref())
                    .is_some_and(|editor|editor.group_id==id);
                if editing{
                    if let Some(group)=self.workflow_snapshot.as_ref()
                        .and_then(|board|board.groups.iter().find(|group|group.id==id)){
                        self.workflow_stage_after_freight=Some(
                            (id.clone(),group.step,group.needs_notice)
                        );
                        if self.workflow_freight_editor.is_some() &&
                            !self.submit_workflow_freight_edit(ui.ctx()){
                            self.workflow_stage_after_freight=None;
                        }else if self.workflow_action.is_none(){
                            self.workflow_stage_after_freight=None;
                            self.advance_workflow_group(&id,ui.ctx());
                        }
                    }
                }else{
                    self.advance_workflow_group(&id,ui.ctx());
                }
            }
            Some(workflow_board_ui::Event::UndoGroup(id))=>{
                self.retain_workflow_stage_target(&id);
                self.start_workflow_action("undo_group",&id,None,None,false,ui.ctx());
            }
            Some(workflow_board_ui::Event::ManagePending(id))=>{
                self.workflow_dialog=Some(WorkflowDialog::ManagePending(id));
            }
            Some(workflow_board_ui::Event::DeletePending(id))=>{
                self.workflow_dialog=Some(WorkflowDialog::DeletePending(id));
            }
            Some(workflow_board_ui::Event::ConfirmDraft(id))=>{
                let recipient_id=self.workflow_ui.pending_preview()
                    .filter(|target|target.order_id==id)
                    .and_then(|target|target.recipient_id.clone())
                    .or_else(||if self.active_id.as_deref()==Some(id.as_str()){
                        self.analysis.as_ref().and_then(|analysis|
                            reception_service::recipients(analysis).get(self.visible_recipient_index()))
                            .and_then(|row|row["id"].as_str()).map(str::to_owned)
                    }else{None});
                self.pending_stage_display=Some(workflow_board_ui::PendingTarget{
                    order_id:id.clone(),recipient_id
                });
                self.workflow_ui.stop_preview();
                self.request_draft_confirmation(&id,ui.ctx());
                if self.pending_stage_target().is_none(){
                    self.restore_failed_stage_target(&format!("group:{id}"),ui.ctx());
                }
            }
            Some(workflow_board_ui::Event::SelectPending(id))=>{
                self.select_pending_workflow_card(&id,ui.ctx());
            }
            Some(workflow_board_ui::Event::SelectPendingRecipient(target))=>{
                self.workflow_freight_guidance=None;
                if self.workflow_freight_editor.as_ref().is_some_and(|editor|
                    editor.text.trim()!=editor.original.trim()
                ){
                    self.submit_workflow_freight_edit(ui.ctx());
                }else{
                    self.workflow_freight_editor=None;
                    self.workflow_freight_next_selection=None;
                    self.pending_stage_display=None;
                    self.workflow_ui.selection=None;
                    self.workflow_ui.folded=None;
                    self.workflow_ui.stop_preview();
                    self.link_hover.clear();
                    self.source_hover=None;
                    self.focus_pending_recipient(&target,ui.ctx());
                }
            }
            Some(workflow_board_ui::Event::SelectShipment(selection))=>{
                self.workflow_freight_guidance=None;
                if self.workflow_freight_editor.as_ref().is_some_and(|editor|
                    editor.text.trim()!=editor.original.trim()
                ){
                    self.workflow_freight_next_selection=Some(selection);
                    if !self.submit_workflow_freight_edit(ui.ctx()){
                        self.workflow_freight_next_selection=None;
                    }
                }else{
                    self.workflow_freight_editor=None;
                    self.workflow_freight_next_selection=None;
                    self.pending_stage_display=None;
                    self.workflow_ui.select(selection);
                }
                self.link_hover.clear();
                self.source_hover=None;
            }
            _=>{}
        }
    }

    fn show_freight_leave_guard(&mut self,ctx:&egui::Context,work_area:Rect){
        let Some(next)=self.freight_pending_page else{return;};
        let Some(manager)=self.freight_manager.as_mut() else{
            self.freight_pending_page=None;
            self.freight_pending_save=false;
            return;
        };
        if self.freight_pending_save{
            if manager.busy(){return;}
            if manager.last_error().is_some(){
                // A failed or revision-conflicted write must NEVER be retried
                // without user review of the new data and explicit action.
                self.freight_pending_save=false;
            }else if !manager.dirty(){
                manager.discard_drafts();
                self.page=next;
                self.freight_pending_page=None;
                self.freight_pending_save=false;
                return;
            }else if manager.save_dirty(ctx.clone()){
                // A rate modal and batch assignments can each have drafts.
                // Continue the user's single "save and leave" instruction
                // through the same Rust snapshot/revision boundary.
                return;
            }else{
                self.freight_pending_save=false;
            }
        }
        if !manager.dirty(){
            manager.discard_drafts();
            self.page=next;
            self.freight_pending_page=None;
            return;
        }
        crate::popup::modal_backdrop(ctx,"freight-global-leave-backdrop",
            manager.has_rate_modal());
        match crate::popup::show_save_confirmation(
            ctx,work_area,"freight-global-leave","저장하지 않은 변경사항",
            "운임 관리에서 수정한 내용이 있습니다. 변경사항을 저장할까요?",
            "저장하고 이동",!manager.busy(),manager.can_save_dirty()
        ){
            crate::popup::ChangeDecision::Cancel=>{
                self.freight_pending_page=None;
                self.freight_pending_save=false;
            }
            crate::popup::ChangeDecision::Save=>{
                if manager.save_dirty(ctx.clone()){self.freight_pending_save=true;}
            }
            crate::popup::ChangeDecision::Discard=>{
                manager.discard_drafts();
                self.page=next;
                self.freight_pending_page=None;
                self.freight_pending_save=false;
            }
            crate::popup::ChangeDecision::Pending=>{}
        }
    }

    fn show_settings_leave_guard(&mut self,ctx:&egui::Context,work_area:Rect){
        let Some(next)=self.settings_pending_page else{return;};
        let Some(view)=self.settings_view.as_mut() else{
            self.settings_pending_page=None;
            return;
        };
        if self.settings_pending_save{
            if view.busy(){return;}
            if !view.dirty(){
                self.page=next;
                self.settings_pending_page=None;
                self.settings_pending_save=false;
                return;
            }
            // A failed save keeps the editor open and the user's draft.
            self.settings_pending_save=false;
        }
        crate::popup::modal_backdrop(ctx,"settings-page-leave-backdrop",false);
        match crate::popup::show_save_confirmation(
            ctx,work_area,"settings-global-leave","저장하지 않은 변경사항",
            "설정에서 수정한 내용이 있습니다. 변경사항을 저장할까요?",
            "저장하고 이동",!view.busy(),!view.busy()&&!view.read_only()
        ){
            crate::popup::ChangeDecision::Cancel=>self.settings_pending_page=None,
            crate::popup::ChangeDecision::Save=>{
                self.settings_pending_save=true;
                view.save(Arc::clone(&self.gateway),ctx.clone());
            }
            crate::popup::ChangeDecision::Discard=>{
                view.discard();
                self.page=next;
                self.settings_pending_page=None;
            }
            crate::popup::ChangeDecision::Pending=>{}
        }
    }

    fn other_page(&mut self, ctx: &egui::Context) {
        if self.page==Page::Excel && self.excel_manager.is_none(){
            self.excel_manager=Some(excel_manager::ExcelManager::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        if self.page==Page::Settlement && self.daily_settlement_manager.is_none(){
            self.daily_settlement_manager=Some(daily_settlement_manager::DailySettlementManager::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        if self.page==Page::History && self.order_history_manager.is_none(){
            self.order_history_manager=Some(order_history_manager::OrderHistoryManager::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        if self.page==Page::Products && self.product_manager.is_none() {
            self.product_manager=Some(product_manager::ProductManager::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        if self.page==Page::Clients && self.client_manager.is_none() {
            self.client_manager=Some(client_manager::ClientManager::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        if self.page==Page::Freight && self.freight_manager.is_none(){
            self.freight_manager=Some(freight_manager::FreightManager::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        if self.page==Page::Settings && self.settings_view.is_none(){
            self.settings_view=Some(settings_view::SettingsView::new(
                Arc::clone(&self.gateway),ctx.clone()
            ));
        }
        // React HubShell owns the Kakao column independently of navigation.
        // A tab change must keep the native HWND dock visible; only the
        // explicit Disconnect button (or app exit) restores external windows.
        egui::CentralPanel::default()
            .frame(theme::workspace_shell_frame())
            .show(ctx, |ui| {
                let viewport_width=ui.available_width();
                let content_width=viewport_width
                    .max(layout::LayoutState::minimum_width());
                let height=ui.available_height().max(1.0);
                let mut draw_management=|host:&mut Ui|{
                        let widths=self.layout.widths(content_width);
                        let (_,all)=host.allocate_space(
                            Vec2::new(content_width,height)
                        );
                        let left=Rect::from_min_size(
                            all.min,Vec2::new(widths[0],height)
                        );
                        host.scope_builder(
                            UiBuilder::new().id_salt("management-kakao-pane")
                                .max_rect(left).layout(Layout::top_down(Align::Min)),
                            |pane|{
                                pane.set_clip_rect(left);
                                pane.apply_shared_width(crate::card::CardWidthPolicy::Fixed,left.width());
                                pane.set_height(left.height());
                                self.kakao_pane(pane);
                            }
                        );
                        let splitter=Rect::from_min_size(
                            egui::pos2(left.right(),all.top()),
                            Vec2::new(layout::SPLITTER,height)
                        );
                        let id=host.make_persistent_id("management-kakao-splitter");
                        let response=host.interact(
                            splitter,id,Sense::click_and_drag()
                        );
                        if response.hovered() || response.dragged(){
                            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                        crate::ui_layout::paint_dock_splitter(
                            host,splitter,true,response.hovered()||response.dragged()
                        );
                        if response.drag_started() {
                            self.dragging_split=Some(HorizontalSplitDrag{
                                index:0,initial_widths:widths,
                            });
                        }
                        if response.dragged() {
                            if let Some(drag)=self.dragging_split {
                                if drag.index==0 {
                                    self.layout.resize_pair(
                                        drag.initial_widths,0,response.total_drag_delta().unwrap_or_default().x
                                    );
                                }
                            }
                        }
                        if response.drag_stopped() &&
                            self.dragging_split.is_some_and(|d|d.index==0) {
                            self.dragging_split=None;
                            let _=self.layout.save();
                        }
                        if response.double_clicked(){
                            self.layout.reset();
                            let _=self.layout.save();
                        }
                        let right=Rect::from_min_max(
                            egui::pos2(splitter.right(),all.top()),all.max
                        );
                        host.scope_builder(
                            UiBuilder::new().id_salt(("management-page",self.page.label(),self.navigation_snapshots.epoch(self.page as usize)))
                                .max_rect(right).layout(Layout::top_down(Align::Min)),
                            |main|{
                                main.set_clip_rect(right);
                                main.apply_shared_width(crate::card::CardWidthPolicy::Fixed,right.width());
                                main.set_height(right.height());
                                // The shared workspace frame already owns the
                                // outer top/right/bottom 6pt gutters. Do not
                                // add a second 16pt page margin: management
                                // cards should end on the same gutter line as
                                // the reception panes. Individual section
                                // frames retain their own content padding.
                                theme::management_page_host_frame().show(main,|page|{
                                    match self.page{
                                        Page::Excel=>{
                                            if let Some(excel)=self.excel_manager.as_mut(){
                                                excel.show_page(page);
                                                if let Some((order_id,order_date))=excel.take_open_order(){
                                                    self.page=Page::History;
                                                    self.sync_navigation_snapshot(ctx,Instant::now());
                                                    if self.order_history_manager.is_none(){
                                                        self.order_history_manager=Some(
                                                            order_history_manager::OrderHistoryManager::new(
                                                                Arc::clone(&self.gateway),ctx.clone()
                                                            )
                                                        );
                                                    }
                                                    if let Some(history)=self.order_history_manager.as_mut(){
                                                        history.open_order(&order_id,&order_date,ctx.clone());
                                                    }
                                                }
                                            }
                                        }
                                        Page::Settlement=>{
                                            if let Some(settlement)=self.daily_settlement_manager.as_mut(){
                                                settlement.show_page(page);
                                            }
                                        }
                                        Page::History=>{
                                            if let Some(history)=self.order_history_manager.as_mut(){
                                                history.show_page(page);
                                            }
                                        }
                                        Page::Products=>{
                                            if let Some(manager)=self.product_manager.as_mut(){
                                                manager.show_page(page);
                                            }
                                        }
                                        Page::Clients=>{
                                            if let Some(manager)=self.client_manager.as_mut(){
                                                manager.show_page(page,Arc::clone(&self.gateway));
                                            }
                                        }
                                        Page::Freight=>{
                                            if let Some(freight)=self.freight_manager.as_mut(){
                                                freight.show_page(page);
                                            }
                                        }
                                        Page::Settings=>{
                                            if let Some(settings)=self.settings_view.as_mut(){
                                                settings.show_page(page,Arc::clone(&self.gateway));
                                            }
                                        }
                                        _=>{page.label(typography::page_heading(self.page.label()));}
                                    }
                                });
                            }
                        );
                    };
                if content_width > viewport_width + 0.5 {
                    crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Horizontal)
                        .id_salt("kakao-management-shell")
                        .auto_shrink([false,false])
                        .show(ui,&mut draw_management);
                }else{
                    draw_management(ui);
                }
            });
    }
}

impl eframe::App for BandoEgui {
    fn on_exit(&mut self,_gl:Option<&eframe::glow::Context>){
        self.window_state.finish();
        if let Err(error)=self.layout.save(){eprintln!("[BANDO HUB] 화면 분할 저장 실패: {error}");}
        // eframe invokes this before the native HUB HWND is destroyed.
        // Kakao must lose its temporary owner BEFORE HUB teardown.
        self.kakao_dock.disconnect();
        self.kakao_tracker.shutdown();
        self.clipboard_monitor.shutdown();
        source_highlight::clear_visual_cache();
            clear_quote_display_cache();
            crate::text_fit::clear_reception_text_fit();
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_started=Instant::now();
        self.window_state.update(ctx);
        overlay::begin_frame(ctx);
        if overlay::begin_page(ctx,self.page.label()){
            self.workflow_ui.stop_preview();
        }
        floating_notice::begin_page(ctx,self.page.label());
        if let Some(page)=self.client_pending_page{
            let approved=self.client_manager.as_mut()
                .is_some_and(|manager|manager.take_global_approved());
            let waiting=self.client_manager.as_ref()
                .is_some_and(|manager|manager.global_leave_in_progress());
            if approved{
                self.page=page;
                self.client_pending_page=None;
            }else if !waiting{
                self.client_pending_page=None;
            }
        }
        if let Some(page)=self.product_pending_page{
            let approved=self.product_manager.as_mut()
                .is_some_and(|manager|manager.take_category_global_leave());
            let waiting=self.product_manager.as_ref()
                .is_some_and(|manager|manager.category_global_leave_in_progress());
            if approved {
                self.page=page;
                self.product_pending_page=None;
            }else if !waiting{
                // User cancelled a pending category leave confirmation.
                self.product_pending_page=None;
            }
        }
        if overlay::begin_page(ctx,self.page.label()){
            floating_notice::begin_page(ctx,self.page.label());
            self.workflow_ui.stop_preview();
        }
        self.poll_source_analysis(ctx);
        self.poll_auto_client_change();
        self.retry_auto_client_change(ctx);
        self.poll_pricing_basis_check(ctx);
        if self.settings_view.as_mut().is_some_and(|view|view.take_restored()){
            // A restored operational DB invalidates every prior projected
            // price/client/order snapshot. Never keep stale native matches.
            self.invalidate_reception_metadata();
            self.notification_center=Default::default();
            self.inbox_folder_path=None;
            self.inbox_folder_loaded=false;
            self.inbox_folder_error=None;
            self.inbox_folder_open_error=None;
            self.client_names.clear();
            self.client_summaries.clear();
            self.workflow_snapshot=None;
            self.pending_display=None;
            self.pending_review_panel.clear();
            self.workflow_source_display=None;
            source_highlight::clear_visual_cache();
            clear_quote_display_cache();
            crate::text_fit::clear_reception_text_fit();
            self.workflow_read.cancel();
            self.workflow_read.verified();
            self.workflow_last_fetch=None;
            self.workflow_ui.selection=None;
            self.workflow_ui.stop_preview();
            self.workflow_action=None;
            self.committed_draft_handoff=None;
            self.pending_stage_display=None;
            self.workflow_ui.clear_card_order();
            self.workflow_stage_deadlines.clear();
            self.workflow_freight_editor=None;
            self.confirmed_cell=None;
            self.submitted_confirmed_cell=None;
            self.confirmed_next_cell=None;
            self.confirmed_modal_target=None;
            self.submitted_workflow_freight=None;
            self.workflow_stage_after_freight=None;
            self.workflow_freight_guidance=None;
            self.workflow_freight_next_selection=None;
            self.workflow_dialog=None;
            self.client_manager=None;
            self.freight_manager=None;
            self.order_history_manager=None;
            self.client_chooser=None;
            self.item_editor=None;
            self.product_picker=None;
            self.working_sync=None;
            self.vat_change=None;
            self.vat_dialog=None;
            self.client_change=None;
            self.auto_client_change=None;
            self.clear_source_analysis();
        }
        // Poll export completion even if the user navigated off the Excel page.
        if let Some(excel)=self.excel_manager.as_mut(){excel.poll(ctx);}
        let corrected=self.excel_manager.as_mut().is_some_and(|excel|excel.take_reply_changed());
        let reply=self.factory_reply_inbox.poll(ctx);
        self.notification_center.poll(ctx,corrected||reply.is_some());
        self.refresh_after_factory_reply(ctx,corrected,
            reply.as_ref().is_some_and(|r|r.files>0));
        if let Some(reply)=reply{
            if reply.files>0{floating_notice::publish(ctx,"excel.reply",floating_notice::Tone::Info,
                format!("공장 회신 {}파일: 반영 {}행, 검토 {}행",reply.files,reply.applied,reply.held));}
            if let Some(error)=reply.error{floating_notice::publish(ctx,"excel.reply",floating_notice::Tone::Error,error);}
        }
        self.poll_workflow_action(ctx);
        self.poll_reception_metadata(ctx);
        self.poll_workflow_board(ctx);
        self.resume_confirmed_navigation(ctx);
        self.poll_client_change(ctx);
        self.poll_working_sync();
        self.poll_inline_change(ctx);
        self.poll_vat_change(ctx);
        self.poll_remote_recheck(ctx);
        if self.remote_recheck_due && self.remote_recheck.is_none() &&
            self.analysis.is_some(){
            self.start_remote_recheck(ctx);
        }
        self.poll_source_saves(ctx);
        self.poll_clipboard_intake(ctx);
        if self.page==Page::Reception &&
            self.client_manager.as_mut().is_some_and(|manager|manager.take_changed()) {
            // An explicit local client change invalidates the metadata cache
            // immediately, including when the current source is empty.
            self.invalidate_reception_metadata();
            if !self.raw_source.trim().is_empty(){
                self.auto_client_change=None;
                self.analysis_due_at=Some(Instant::now());
            }
        }
        if self.page == Page::Reception {
            self.kakao_tracker.refresh(ctx);
            // Native window events wake egui directly. The slower deadline
            // also serves the shared DB revision probe and dock safety check.
            ctx.request_repaint_after(Duration::from_secs(5));
            if !self.source_editing && self.raw_source.is_empty() {
                // Start the native listener lazily; its events wake egui.
                // No 200ms repaint loop or clipboard data read in this frame.
                self.clipboard_monitor.refresh(ctx);
            }
        }
        if !self.direct_source_entry &&
            self.analysis_due_at.is_some_and(|at| Instant::now() >= at) {
            self.analysis_due_at = None;
            if !self.raw_source.trim().is_empty() {
                self.start_source_analysis(ctx.clone());
            }
        }
        // Refresh native staff issues only when the actual source-bound order
        // input changes. The background worker rejects all stale replies.
        if self.page==Page::Reception {
            let active=self.active_id.as_deref()
                .and_then(|id|self.drafts.iter().find(|row|row.id==id));
            if self.unapplied_auto_client_target().is_some(){
                // A confirmed identity proposal is being applied. Do not
                // publish a stale missing-dealer card or authorize progression.
                self.review_panel.clear();
            }else{
                self.review_panel.refresh(
                    ctx,Arc::clone(&self.gateway),active,self.analysis.as_ref()
                );
            }
            self.poll_input_guidance(ctx);
        }else{
            self.pending_input_guidance=None;
            self.guided_inline_target=None;
        }
        self.sync_navigation_snapshot(ctx,Instant::now());
        self.top_bar(ctx);
        self.sync_navigation_snapshot(ctx,Instant::now());
        if overlay::begin_page(ctx,self.page.label()){
            floating_notice::begin_page(ctx,self.page.label());
            self.workflow_ui.stop_preview();
        }
        if self.page == Page::Reception {
            self.reception(ctx);
        } else {
            self.other_page(ctx);
        }
        let work_area=self.modal_work_area(ctx);
        if self.ocr_review.is_some() || self.pending_replace.is_some() ||
            (self.page==Page::Reception &&
                (self.client_chooser.is_some() || self.manual_workspace.is_some() ||
                self.edit_history_open || self.workflow_dialog.is_some()
                 || self.vat_dialog.is_some() || self.item_editor.is_some()
                 || self.product_picker.is_some())) {
            crate::popup::modal_backdrop(
                ctx,"bando-workspace-modal-backdrop",false
            );
        }
        self.show_ocr_review(ctx,work_area);
        self.show_replacement_confirmation(ctx,work_area);
        if self.page == Page::Reception {
            self.show_client_chooser(ctx,work_area);
            self.show_manual_workspace(ctx,work_area);
            self.show_item_editor(ctx,work_area);
            self.show_product_picker(ctx,work_area);
            self.show_vat_settings(ctx,work_area);
            self.show_edit_history(ctx,work_area);
            self.show_workflow_dialog(ctx,work_area);
        } else if self.page==Page::Excel {
            if let Some(excel)=self.excel_manager.as_mut(){excel.show_reply_review(ctx,work_area);}
        } else if self.page==Page::History {
            if let Some(history)=self.order_history_manager.as_mut(){
                history.show_order_detail_modal(ctx,work_area);
                history.show_memo_modal(ctx,work_area);
            }
        } else if self.page==Page::Settings {
            if let Some(settings)=self.settings_view.as_mut(){
                if settings.has_tab_confirmation(){
                    settings.show_tab_confirmation(
                        ctx,work_area,Arc::clone(&self.gateway)
                    );
                }else if settings.has_data_modal(){
                    settings.show_data_modals(
                        ctx,work_area,Arc::clone(&self.gateway)
                    );
                }
            }
            // Only one settings confirmation can be active at a time.
            if !self.settings_view.as_ref()
                .is_some_and(|settings|settings.has_tab_confirmation() ||
                    settings.has_data_modal()){
                self.show_settings_leave_guard(ctx,work_area);
            }
        } else if self.page==Page::Freight {
            self.show_freight_leave_guard(ctx,work_area);
        } else if self.page==Page::Products {
            if let Some(manager)=self.product_manager.as_mut() {
                if manager.has_modal() {
                    manager.show_detail(ctx,work_area);
                }
            }
        } else if self.page==Page::Clients {
            if let Some(manager)=self.client_manager.as_mut() {
                if manager.has_message_confirmation(){
                    manager.show_message_confirmation(
                        ctx,work_area,Arc::clone(&self.gateway)
                    );
                }else if manager.has_excel_review(){
                    manager.show_excel_review(ctx,work_area,Arc::clone(&self.gateway));
                }else if manager.has_modal(){
                    crate::popup::modal_backdrop(
                        ctx,"client-manager-modal-backdrop",false
                    );
                    manager.show_editor(ctx,work_area,Arc::clone(&self.gateway));
                }
            }
        }
        self.notification_center.show(ctx,work_area);
        self.frameless_resize(ctx);
        // Tooltip-order overlay is drawn LAST, on top of the current modal,
        // without allocating space in any card, row or scrollbar viewport.
        floating_notice::sync(ctx,"ocr_learning_error","reception.ocr_learning",
            floating_notice::Tone::Error,self.ocr_review.as_ref().and_then(|review|review.model_error.as_deref()));
        if self.manual_workspace.is_none(){
            floating_notice::sync(ctx,"manual_workspace_error","reception.manual",
                floating_notice::Tone::Error,None);
        }
        floating_notice::sync(ctx,"workflow_board_read_error","reception.workflow.read",
            floating_notice::Tone::Error,
            if self.page==Page::Reception{self.workflow_read_error.as_deref()}else{None});
        self.sync_navigation_snapshot(ctx,Instant::now());
        if overlay::begin_page(ctx,self.page.label()){
            floating_notice::begin_page(ctx,self.page.label());
            self.workflow_ui.stop_preview();
        }
        overlay::finish_frame(ctx);
        floating_notice::show(ctx);
        // Developer console only; never expose performance diagnostics as
        // product notifications or include any order/customer data.
        if cfg!(debug_assertions) && frame_started.elapsed()>Duration::from_millis(100){
            let screen=if self.page==Page::Products{
                self.product_manager.as_ref().map(|p|p.active_tab_name())
                    .unwrap_or(self.page.label())
            }else{self.page.label()};
            eprintln!("[BANDO HUB] slow frame: screen={screen} elapsed_ms={}",
                frame_started.elapsed().as_millis());
        }
    }
}


#[cfg(test)]
mod vat_settings_ui_tests {
    use super::*;
    #[test]
    fn recipient_list_reserves_five_rows_and_scrolls_after_ten(){
        let row=theme::RECEPTION_VAT_LIST_ROW_HEIGHT;
        for count in [0,1,2,3,4,5]{
            assert_eq!(BandoEgui::vat_recipient_list_height(count,900.0),5.0*row);
        }
        assert_eq!(BandoEgui::vat_recipient_list_height(6,900.0),6.0*row);
        assert_eq!(BandoEgui::vat_recipient_list_height(10,900.0),10.0*row);
        assert_eq!(BandoEgui::vat_recipient_list_height(11,900.0),10.0*row);
        assert_eq!(BandoEgui::vat_recipient_list_height(25,900.0),10.0*row);
        // Compact windows must still leave room for header and action row.
        let compact_height=460.0-theme::RECEPTION_VAT_LIST_SCREEN_RESERVE;
        assert_eq!(BandoEgui::vat_recipient_list_height(2,460.0),compact_height);
        assert_eq!(BandoEgui::vat_recipient_list_height(25,460.0),compact_height);
    }
    #[test]
    fn exact_percent_parser_and_formatter(){
        for (input,expected) in [
            ("10",1000),("7.5",750),("0.01",1),
            ("100",10_000),("10.25%",1025)
        ]{
            assert_eq!(parse_vat_rate(input).unwrap(),expected);
            assert_eq!(parse_vat_rate(&vat_rate_text(expected)).unwrap(),expected);
        }
        for bad in ["-1","101","10.123","1e2",""]{
            assert!(parse_vat_rate(bad).is_err());
        }
    }
    #[test]
    fn one_order_wide_request_instead_of_independent_checkbox_edits(){
        let rows=serde_json::json!([
            {"id":"a","vatOverride":true},
            {"id":"b","vatOverride":false}
        ]);
        let mut draft=VatDialog::new(
            "order".into(),"raw".into(),rows.as_array().unwrap()
        );
        let recipients=rows.as_array().unwrap();
        assert_eq!(draft.scope(recipients),VatScope::Partial);
        let request=draft.request(recipients).unwrap();
        assert_eq!(request.field,"vatSettings");
        assert_eq!(request.value["scope"],"partial");
        assert_eq!(request.value["recipientIds"],serde_json::json!(["a"]));
        draft.set_all_checked(recipients,true);
        assert_eq!(draft.scope(recipients),VatScope::All);
        assert_eq!(draft.request(recipients).unwrap().value["scope"],"all");
        draft.set_all_checked(recipients,false);
        assert_eq!(draft.scope(recipients),VatScope::None);
        assert_eq!(draft.request(recipients).unwrap().value["scope"],"none");
        draft.set_recipient_checked("b",true);
        assert_eq!(draft.scope(recipients),VatScope::Partial);
        assert_eq!(draft.request(recipients).unwrap().value["recipientIds"],
            serde_json::json!(["b"]));
    }
    #[test]
    fn global_vat_checkboxes_reflect_each_individual_selection(){
        let rows=serde_json::json!([
            {"id":"a","vatOverride":false},
            {"id":"b","vatOverride":false},
            {"id":"c","vatOverride":false}
        ]);
        let recipients=rows.as_array().unwrap();
        let mut dialog=VatDialog::new("id".into(),"raw".into(),recipients);
        assert_eq!(dialog.scope(recipients),VatScope::None);
        dialog.set_recipient_checked("a",true);
        assert_eq!(dialog.scope(recipients),VatScope::Partial);
        dialog.set_recipient_checked("b",true);
        dialog.set_recipient_checked("c",true);
        assert_eq!(dialog.scope(recipients),VatScope::All);
        dialog.set_recipient_checked("b",false);
        assert_eq!(dialog.scope(recipients),VatScope::Partial);
        dialog.set_all_checked(recipients,false);
        assert_eq!(dialog.scope(recipients),VatScope::None);
    }
    #[test]
    fn partial_vat_checkbox_updates_only_selected_recipient_and_can_toggle_back(){
        let rows=serde_json::json!([
            {"id":"a","name":"김준희","vatOverride":true},
            {"id":"b","name":"표시나","vatOverride":false},
            {"id":"c","name":"추가 수취인","vatOverride":false}
        ]);
        let mut draft=VatDialog::new(
            "order".into(),"raw".into(),rows.as_array().unwrap()
        );
        assert_eq!(draft.scope(rows.as_array().unwrap()),VatScope::Partial);
        draft.set_recipient_checked("a",false);
        draft.set_recipient_checked("b",true);
        assert!(!draft.recipients.contains("a"));
        assert!(draft.recipients.contains("b"));
        assert_eq!(
            draft.request(rows.as_array().unwrap()).unwrap().value["recipientIds"],
            serde_json::json!(["b"])
        );
        draft.set_recipient_checked("a",true);
        draft.set_recipient_checked("b",false);
        assert_eq!(
            draft.request(rows.as_array().unwrap()).unwrap().value["recipientIds"],
            serde_json::json!(["a"])
        );
    }
}
#[cfg(test)]
mod reception_visible_selection_regression_tests{
    use super::*;
    fn two_recipients()->reception_service::ReceptionAnalysis{
        reception_service::ReceptionAnalysis{
            recognition:serde_json::json!({"recipients":[
                {"id":"kim","name":"김준희","items":[{"id":"i1"}]},
                {"id":"pyo","name":"표시나","items":[{"id":"i2"}]}
            ]}),
            reference:vec![],client_match:serde_json::Value::Null,
            client_candidates:vec![],matched_client_name:None,
            chosen_client_name:None,directory_count:0,linked_spans:vec![]
        }
    }
    #[test]
    fn same_order_recipient_click_commits_by_id_after_analysis(){
        let mut app=BandoEgui::default();
        app.active_id=Some("draft".into());
        app.requested_recipient_focus=Some(("draft".into(),"pyo".into()));
        let ctx=egui::Context::default();
        app.apply_requested_recipient_focus(&ctx);
        assert_eq!(app.selected_recipient,0);
        assert!(app.requested_recipient_focus.is_some());
        app.analysis=Some(two_recipients());
        app.apply_requested_recipient_focus(&ctx);
        assert_eq!(app.selected_recipient,1);
        assert!(app.requested_recipient_focus.is_none());
    }
    #[test]
    fn hover_changes_visible_recipient_without_changing_committed_selection(){
        let mut app=BandoEgui::default();
        app.active_id=Some("draft".into());
        app.analysis=Some(two_recipients());
        app.selected_recipient=0;
        let ctx=egui::Context::default();
        let linked=source_highlight::key("pyo","pyo","name");
        app.link_hover.update_for("draft",Some(linked.clone()),&ctx);
        std::thread::sleep(Duration::from_millis(55));
        app.link_hover.update_for("draft",Some(linked),&ctx);
        assert_eq!(app.visible_recipient_index(),1);
        assert_eq!(app.selected_recipient,0);
        app.link_hover.clear();
        assert_eq!(app.visible_recipient_index(),0);
    }
}

#[cfg(test)]
mod inline_single_click_handoff_tests {
    use super::*;

    fn target(field:&'static str)->InlineTarget{
        InlineTarget{recipient_id:"r1".into(),item_id:None,field}
    }

    #[test]
    fn next_cell_is_resolved_from_committed_recipient_state(){
        let latest=serde_json::json!({"recipients":[{
            "id":"r1","name":"수정된 이름","address":"서울시 중구",
            "mobile":"010-1111-2222","phone":"02-333-4444",
            "note":"직접 수정됨"
        }]});
        for (field,expected) in [
            ("name","수정된 이름"),("address","서울시 중구"),
            ("mobile","010-1111-2222"),("phone","02-333-4444"),
            ("note","직접 수정됨")
        ]{
            assert_eq!(
                current_inline_value(&latest,None,&target(field)).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn next_cell_price_comes_from_updated_native_quote(){
        let latest=serde_json::json!({"recipients":[{
            "id":"r1","dispatchOrigin":"FACTORY",
            "items":[{"id":"item-1","name":"709","quantity":4}]
        }]});
        let quote=serde_json::json!({
            "linePrices":[{"itemId":"item-1","unitPrice":12000}],
            "freightLines":[{"recipientId":"r1","origin":"FACTORY",
                "appliedCharge":6000}]
        });
        let quantity=InlineTarget{recipient_id:"r1".into(),
            item_id:Some("item-1".into()),field:"quantity"};
        let price=InlineTarget{field:"unitPrice",..quantity.clone()};
        assert_eq!(current_inline_value(&latest,Some(&quote),&quantity),
            Some("4".into()));
        assert_eq!(current_inline_value(&latest,Some(&quote),&price),
            Some("12000".into()));
        assert_eq!(current_inline_value(
            &latest,Some(&quote),&target("shippingCharge")
        ),Some("6000".into()));
    }

    #[test]
    fn removed_recipient_or_item_is_not_opened_after_save(){
        let latest=serde_json::json!({"recipients":[{"id":"r1","items":[]}]});
        assert!(current_inline_value(&latest,None,&target("name")).is_none());
        let item=InlineTarget{recipient_id:"r1".into(),
            item_id:Some("removed".into()),field:"quantity"};
        assert!(current_inline_value(&latest,None,&item).is_none());
    }
}

fn main() -> eframe::Result<()> {
    // Connect to an existing operating v2 DB before starting workers.
    // Debug builds do not auto-discover it. No DB is created here.
    match business_state::bind_existing_tauri_database() {
        Ok(Some(_)) => {
            if !cfg!(debug_assertions) || business_state::test_launch() {
                if let Err(error)=business_state::migrate_bound_product_colors() {
                    eprintln!("[BANDO HUB] 상품 색상 데이터 정리 실패: {error}");
                    std::process::exit(2);
                }
            }
        },
        Ok(None) => {},
        Err(error) => {
            eprintln!("[BANDO HUB] 기존 Tauri 업무 DB 자동 연결 실패: {error}");
            std::process::exit(2);
        }
    }
    let backend = match gateway::from_environment() {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("[BANDO HUB] 서비스 연결 설정 오류: {error}");
            std::process::exit(2);
        }
    };
    let restored = match backend.load_working_source() {
        Ok(saved) => saved,
        Err(error) => {
            eprintln!("[BANDO HUB] 격리 접수 원문 복원 실패: {error}");
            std::process::exit(2);
        }
    };
    // Both the taskbar icon and the static left wordmark derive from the
    // original Tauri icon, not a re-created/re-designed logo.
    let existing_icon = match eframe::icon_data::from_png_bytes(
        include_bytes!("../../src-tauri/icons/icon.png")
    ) {
        Ok(icon) => icon,
        Err(error) => {
            eprintln!("[BANDO HUB] 원본 로고 파일 오류: {error}");
            std::process::exit(2);
        }
    };
    let brand_image = egui::ColorImage::from_rgba_unmultiplied(
        [existing_icon.width as usize, existing_icon.height as usize],
        &existing_icon.rgba,
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(existing_icon)
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1260.0, 720.0])
            .with_title("BANDO HUB")
            .with_decorations(false)
            .with_resizable(true),
        ..Default::default()
    };
    eframe::run_native(
        "BANDO HUB",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            typography::install(&cc.egui_ctx);
            let brand_texture = cc.egui_ctx.load_texture(
                "bando-original-brand",
                brand_image,
                egui::TextureOptions::LINEAR,
            );
            // Seed the autosave worker once. Keystrokes only transmit the
            // active draft's source delta, never all saved draft projections.
            let autosave = source_autosave::SourceAutosave::new(
                Arc::clone(&backend), restored.clone(), cc.egui_ctx.clone()
            );
            let source_editing = !restored.raw.is_empty();
            let saved_type = match OrderType::parse(&restored.order_type) {
                Ok(kind) => kind,
                Err(error) => {
                    eprintln!("[BANDO HUB] 접수 유형 복원 실패: {error}");
                    std::process::exit(2);
                }
            };
            let mut app=BandoEgui {
                brand_texture: Some(brand_texture),
                gateway: backend,
                raw_source: restored.raw,
                drafts: restored.drafts,
                active_id: restored.active_id,
                source_editing,
                order_type: saved_type,
                source_autosave: Some(autosave),
                ..BandoEgui::default()
            };
            if source_editing {
                if app.restore_cached_analysis(){
                    app.start_pricing_basis_check(&cc.egui_ctx);
                }else{
                    app.analysis_due_at=Some(Instant::now()+Duration::from_millis(250));
                    cc.egui_ctx.request_repaint_after(Duration::from_millis(260));
                }
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod client_chooser_async_handoff_tests {
    use super::*;
    use serde_json::{json,Value};
    use std::sync::{Condvar,Mutex};
    use std::time::Duration;

    // Block the native identity lookup so popup closure can be asserted
    // BEFORE any business worker can possibly return its result.
    struct GatedGateway {
        gate:Arc<(Mutex<bool>,Condvar)>,
        fail:bool,
    }
    impl gateway::BandoGateway for GatedGateway {
        fn label(&self)->&'static str{"gated-client-choice"}
        fn isolated_client_identity_link(&self)->bool{true}
        fn load_catalog(&self)->Result<Value,String>{
            let (lock,ready)=&*self.gate;
            let mut open=lock.lock().unwrap();
            while !*open {
                open=ready.wait(open).unwrap();
            }
            if self.fail{return Err("synthetic dealer lookup error".into());}
            Ok(json!({"clients":[{"id":"new","name":"새 거래처","active":true}]}))
        }
        fn load_working_source(&self)->Result<WorkingSource,String>{
            Err("not used in chooser handoff".into())
        }
        fn save_working_source(&self,_source:&WorkingSource)->Result<u64,String>{
            Err("not used in chooser handoff".into())
        }
        fn execute(&self,_op:gateway::ReadOperation,_value:Value)
            ->Result<Value,String>{
            Err("native price rules are not needed in isolated ID fixture".into())
        }
    }

    fn create_app(gateway:Arc<dyn gateway::BandoGateway>,
                  ctx:&egui::Context)->BandoEgui{
        let mut app=BandoEgui::default();
        app.gateway=Arc::clone(&gateway);
        app.raw_source="첫 발주 원문".into();
        app.order_type=OrderType::Agency;
        app.active_id=Some("draft-1".into());
        app.drafts=vec![WorkingReception{
            id:"draft-1".into(),raw:"첫 발주 원문".into(),
            order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some("첫 발주 원문".into()),
            analysis_evidence:Some(json!({
                "recipients":[],"candidates":[],"unassigned":[]
            })),
            analysis_view:None,client_match:Some(json!({})),
            chat_handle:None,chat_title:None,
            client_id:Some("old".into()),client_selection:Some("MANUAL".into()),
            pricing_preview:None,working_version:None,
            working_synced:false,manual_model:None,edit_history:Vec::new(),
        }];
        app.client_chooser=Some(ClientChooser::new(
            "draft-1".into(),gateway,ctx.clone()
        ));
        app
    }

    fn release(gate:&Arc<(Mutex<bool>,Condvar)>){
        let (lock,ready)=&**gate;
        *lock.lock().unwrap()=true;
        ready.notify_all();
    }

    fn poll_until_done(app:&mut BandoEgui,ctx:&egui::Context){
        for _ in 0..200{
            app.poll_client_change(ctx);
            if app.client_change.is_none(){return;}
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("pending dealer computation never completed");
    }

    #[test]
    fn verified_click_closes_chooser_before_calculation_and_blocks_reopening(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(false),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate:Arc::clone(&gate),fail:false
        });
        let mut app=create_app(adapter,&ctx);
        app.accept_client_chooser_selection("new",false,&ctx);
        assert!(app.client_chooser.is_some(),"unverified result cannot dismiss modal");
        assert!(app.client_change.is_none());
        app.accept_client_chooser_selection("new",true,&ctx);
        assert!(app.client_chooser.is_none(),
            "chooser must close in the click frame, not native completion");
        assert!(app.client_change.is_some(),"Rust request must run independently");
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("old"),
            "an unconfirmed quote cannot replace the selected dealer");
        app.open_client_chooser(&ctx);
        assert!(app.client_chooser.is_none(),
            "a second popup must not open while the first request is pending");
        release(&gate);
        poll_until_done(&mut app,&ctx);
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("new"));
        assert_eq!(app.drafts[0].client_selection.as_deref(),Some("MANUAL"));
        assert!(app.client_chooser.is_none(),"completion must not reopen the modal");
    }

    #[test]
    fn calculation_failure_keeps_old_client_and_reports_without_reopening(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(false),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate:Arc::clone(&gate),fail:true
        });
        let mut app=create_app(adapter,&ctx);
        app.accept_client_chooser_selection("new",true,&ctx);
        assert!(app.client_chooser.is_none());
        assert!(app.client_change.is_some());
        release(&gate);
        poll_until_done(&mut app,&ctx);
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("old"));
        assert_eq!(app.drafts[0].client_selection.as_deref(),Some("MANUAL"));
        assert!(app.client_chooser.is_none());
        assert!(app.source_save_error.as_deref()
            .is_some_and(|message|message.contains("synthetic dealer lookup error")));
    }

    #[test]
    fn repeated_same_dealer_request_is_coalesced_while_native_worker_is_busy(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(false),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate:Arc::clone(&gate),fail:false
        });
        let mut app=create_app(adapter,&ctx);
        // Simulate the real chooser click handoff: the modal is closed
        // synchronously before its native calculation starts.
        app.client_chooser=None;
        app.apply_manual_client("new",&ctx);
        assert_eq!(app.client_change.as_ref().map(|job|job.client_id.as_str()),
            Some("new"));
        app.apply_manual_client("new",&ctx);
        assert!(app.source_save_error.is_none(),
            "identical request should be a no-op, not an error or new worker");
        assert_eq!(app.client_change.as_ref().map(|job|job.client_id.as_str()),
            Some("new"));
        app.start_working_sync(&ctx);
        assert!(app.working_sync.is_none());
        assert!(app.working_sync_error.as_deref()
            .is_some_and(|error|error.contains("거래처 변경")));
        let edit=manual_workspace::InlineRequest{
            recipient_id:"r1".into(),item_id:None,
            field:"vatSettings".into(),value:json!({"scope":"all"})
        };
        app.start_vat_change(edit,&ctx);
        assert!(app.vat_change.is_none());
        app.open_manual_workspace(&ctx);
        assert!(app.manual_workspace.is_none());
        release(&gate);
        poll_until_done(&mut app,&ctx);
    }

    #[test]
    fn working_save_blocks_manual_client_change_in_the_reverse_start_order(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(true),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate,fail:false
        });
        let mut app=create_app(adapter,&ctx);
        let (_tx,receiver)=mpsc::channel();
        app.working_sync=Some(PendingWorkingSync{
            order_id:"draft-1".into(),source_raw:"첫 발주 원문".into(),
            order_type:"AGENCY".into(),client_id:"old".into(),
            authority_revision:Some(1),submitted_manual:None,receiver,
        });
        app.apply_manual_client("new",&ctx);
        assert!(app.client_change.is_none());
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("old"));
        assert!(app.source_save_error.as_deref()
            .is_some_and(|message|message.contains("운영 작업본 저장")));
    }

    #[test]
    fn explicit_client_choice_cancels_superseded_auto_quote(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(false),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate:Arc::clone(&gate),fail:false
        });
        let mut app=create_app(adapter,&ctx);
        app.client_chooser=None;
        let cancelled=Arc::new(AtomicBool::new(false));
        let (_tx,receiver)=mpsc::channel();
        app.auto_client_change=Some(PendingAutomaticClient{
            request:app.analysis_request,order_id:"draft-1".into(),
            source_raw:"첫 발주 원문".into(),order_type:"AGENCY".into(),
            chat_handle:None,chat_title:None,client_id:"old".into(),
            working_version:None,price_snapshot:None,
            cancel:Arc::clone(&cancelled),receiver
        });
        app.apply_manual_client("new",&ctx);
        assert!(app.client_change.is_some());
        assert!(app.auto_client_change.is_none());
        assert!(cancelled.load(Ordering::Acquire),
            "superseded auto quote must not continue with later native operations");
        release(&gate);
        poll_until_done(&mut app,&ctx);
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("new"));
    }

    #[test]
    fn unapplied_unique_auto_match_retries_without_overriding_manual_choice(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(false),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate:Arc::clone(&gate),fail:false
        });
        let mut app=create_app(adapter,&ctx);
        let row=&mut app.drafts[0];
        row.client_id=None;
        row.client_selection=None;
        row.client_match=Some(json!({"source":"KAKAO_TITLE","clientId":"new",
            "clientName":"새 거래처","detail":"검증된 대화 제목"}));
        app.analysis=Some(reception_service::ReceptionAnalysis{
            recognition:json!({"recipients":[]}),reference:vec![],
            client_match:app.drafts[0].client_match.clone().unwrap(),
            client_candidates:vec![],matched_client_name:Some("새 거래처".into()),
            chosen_client_name:Some("새 거래처".into()),directory_count:1,linked_spans:vec![],
        });
        // Chooser blocks the first attempt; the exact match is retained.
        app.retry_auto_client_change(&ctx);
        assert!(app.auto_client_change.is_none());
        app.client_chooser=None;
        app.retry_auto_client_change(&ctx);
        assert!(app.auto_client_change.is_some());
        release(&gate);
        for _ in 0..200{
            app.poll_auto_client_change();
            if app.auto_client_change.is_none(){break;}
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("new"));
        assert_eq!(app.drafts[0].client_selection.as_deref(),Some("AUTO"));
        app.drafts[0].client_id=None;
        app.drafts[0].client_selection=Some("MANUAL".into());
        assert!(app.unapplied_auto_client_target().is_none());
    }

    #[test]
    fn repeated_analysis_request_for_the_same_source_does_not_spawn_twice(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(false),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate:Arc::clone(&gate),fail:false
        });
        let mut app=create_app(adapter,&ctx);
        // Background analysis is scheduled without an unrelated chooser
        // covering reception; a visible chooser intentionally blocks it.
        app.client_chooser=None;
        app.start_source_analysis(ctx.clone());
        assert!(app.analysis_pending);
        let first=app.analysis_request;
        let key=app.analysis_running_key.clone();
        app.start_source_analysis(ctx.clone());
        assert_eq!(app.analysis_request,first);
        assert_eq!(app.analysis_running_key,key);
        release(&gate);
    }

    #[test]
    fn native_client_success_reuses_selected_recognition_without_reanalysis(){
        let ctx=egui::Context::default();
        let gate=Arc::new((Mutex::new(true),Condvar::new()));
        let adapter:Arc<dyn gateway::BandoGateway>=Arc::new(GatedGateway{
            gate,fail:false
        });
        let mut app=create_app(adapter,&ctx);
        let recognition=json!({
            "recipients":[],"candidates":[],"unassigned":[]
        });
        let view=json!({
            "recognition":recognition,"reference":[],
            "clientCandidates":[],"directoryCount":0,
            "matchedClientName":null,"chosenClientName":null
        });
        app.analysis=reception_service::ReceptionAnalysis::from_cached_view(
            &app.raw_source,recognition.clone(),json!({}),&view
        );
        assert!(app.analysis.is_some());
        let (tx,receiver)=mpsc::channel();
        app.client_change=Some(PendingClientChange{
            order_id:"draft-1".into(),client_id:"new".into(),
            source_raw:"첫 발주 원문".into(),order_type:"AGENCY".into(),
            chat_handle:None,chat_title:None,manual_model:None,
            working_version:None,price_snapshot:None,
            reprice_selection:None,receiver,
        });
        tx.send(Ok(native_order_change::ChangeResult{
            client_id:"new".into(),manual_model:None,
            pricing_preview:None,order:None,
            recognition:Some(recognition.clone()),
        })).unwrap();
        app.poll_client_change(&ctx);
        assert!(app.client_change.is_none());
        assert!(app.analysis.is_some());
        assert!(app.analysis_due_at.is_none(),
            "successful native change must not schedule AnalyzeOrder again");
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("new"));
        assert_eq!(app.drafts[0].analysis_evidence.as_ref(),Some(&recognition));
        assert!(app.drafts[0].analysis_view.is_some());
    }
}


#[cfg(test)]
mod reception_workflow_admission_integration_tests {
    use super::*;

    fn draft(id:&str)->WorkingReception {
        WorkingReception {
            id:id.into(),raw:"원문".into(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:None,analysis_evidence:None,analysis_view:None,
            client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,manual_model:None,
            edit_history:Vec::new(),
        }
    }
    fn app()->BandoEgui{
        let mut app=BandoEgui::default();
        app.drafts=vec![draft("draft-1"),draft("draft-2")];
        app.active_id=Some("draft-1".into());
        app.raw_source="원문".into();
        app
    }
    struct DeleteGateway{fail:bool}
    impl gateway::BandoGateway for DeleteGateway{
        fn label(&self)->&'static str{"draft-delete-test"}
        fn load_catalog(&self)->Result<serde_json::Value,String>{Err("fixture".into())}
        fn load_working_source(&self)->Result<WorkingSource,String>{Err("fixture".into())}
        fn save_working_source(&self,source:&WorkingSource)->Result<u64,String>{
            if self.fail{Err("synthetic save failure".into())}else{Ok(source.revision+1)}
        }
        fn execute(&self,_op:gateway::ReadOperation,_request:serde_json::Value)
            ->Result<serde_json::Value,String>{Err("fixture".into())}
    }
    fn enable_delete_worker(app:&mut BandoEgui,ctx:&egui::Context,fail:bool){
        let gateway:Arc<dyn gateway::BandoGateway>=Arc::new(DeleteGateway{fail});
        let mut source=app.source_snapshot().unwrap();
        source.revision=12;
        app.source_autosave=Some(source_autosave::SourceAutosave::new(gateway,source,ctx.clone()));
    }
    fn wait_delete(app:&mut BandoEgui,ctx:&egui::Context){
        for _ in 0..500{
            app.poll_source_saves(ctx);
            if app.pending_draft_delete.is_none(){return;}
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("draft delete worker did not acknowledge");
    }
    #[test]
    fn failed_delete_preserves_active_draft_and_does_not_retry_on_autosave(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.drafts[0].client_id=Some("dealer".into());
        app.drafts[0].pricing_preview=Some(serde_json::json!({"total":104720}));
        app.drafts[0].edit_history.push(serde_json::json!({"id":"audit"}));
        app.selected_recipient=2;
        enable_delete_worker(&mut app,&ctx,true);
        assert!(app.delete_pending_reception("draft-1",&ctx));
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert_eq!(app.drafts.len(),2);
        assert!(!app.delete_pending_reception("draft-2",&ctx));
        app.finish_pending_draft_delete("unrelated",Ok(()),&ctx);
        assert!(app.pending_draft_delete.is_some());
        wait_delete(&mut app,&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert_eq!(app.raw_source,"원문");
        assert_eq!(app.selected_recipient,2);
        assert_eq!(app.drafts[0].client_id.as_deref(),Some("dealer"));
        assert_eq!(app.drafts[0].pricing_preview.as_ref().unwrap()["total"],104720);
        assert_eq!(app.drafts[0].edit_history[0]["id"],"audit");
        assert_eq!(app.drafts.len(),2);
        assert!(app.source_save_error.as_deref().is_some_and(|e|e.contains("synthetic save failure")));
    }
    #[test]
    fn unavailable_delete_worker_leaves_draft_and_confirmation_intact(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.workflow_dialog=Some(WorkflowDialog::DeletePending("draft-1".into()));
        assert!(!app.delete_pending_reception("draft-1",&ctx));
        assert!(app.workflow_dialog.is_some());
        assert_eq!(app.drafts.len(),2);
        assert!(app.pending_draft_delete.is_none());
    }
    #[test]
    fn deleting_inactive_draft_preserves_other_active_edits(){
        let ctx=egui::Context::default();
        let mut app=app();
        enable_delete_worker(&mut app,&ctx,false);
        assert!(app.delete_pending_reception("draft-2",&ctx));
        app.raw_source="다른 발주서 수정".into();
        app.queue_source_text_edit();
        wait_delete(&mut app,&ctx);
        assert_eq!(app.drafts.len(),1);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert_eq!(app.drafts[0].raw,"다른 발주서 수정");
    }

    fn vat_worker(app:&mut BandoEgui){
        let (_tx,rx)=mpsc::channel();
        app.vat_change=Some(PendingVatChange{
            order_id:"draft-1".into(),source_raw:"원문".into(),receiver:rx
        });
    }
    fn inline_worker(app:&mut BandoEgui){
        let (_tx,rx)=mpsc::channel();
        let target=InlineTarget{
            recipient_id:"r1".into(),item_id:None,field:"name"
        };
        app.inline_change=Some(PendingInlineChange{
            order_id:"draft-1".into(),raw:"원문".into(),
            target:target.clone(),
            editor:Some(InlineReceptionEditor{
                source_id:"draft-1".into(),target,
                original:"A".into(),text:"B".into(),
                focus_next:false,click_at:None
            }),
            next_client_review:None,next:None,receiver:rx
        });
    }
    fn remote_analysis()->reception_service::ReceptionAnalysis{
        reception_service::ReceptionAnalysis{
            recognition:serde_json::json!({"recipients":[
                {"id":"r1","address":""}
            ]}),
            reference:Vec::new(),client_match:serde_json::Value::Null,
            client_candidates:Vec::new(),matched_client_name:None,
            chosen_client_name:None,directory_count:0,
            linked_spans:Vec::new(),
        }
    }

    #[test]
    fn pending_display_cache_reuses_exact_projection_and_rejects_edited_receipt(){
        let mut row=draft("synthetic");
        row.analysis_evidence=Some(serde_json::json!({"recipients":[{"id":"r","name":"가상"}]}));
        let mut cache=None;
        let first=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        let same=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        assert!(Arc::ptr_eq(&first,&same));
        // Audit history is deliberately excluded from display identity.
        row.edit_history.push(serde_json::json!({"synthetic":true}));
        assert!(Arc::ptr_eq(&first,&BandoEgui::pending_projection(&mut cache,&row).unwrap()));
        row.analysis_evidence.as_mut().unwrap()["recipients"][0]["name"]=serde_json::json!("수정");
        let edited=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        assert!(!Arc::ptr_eq(&first,&edited));
        assert_eq!(edited.recognition["recipients"][0]["name"],"수정");
        row.pricing_preview=Some(serde_json::json!({"total":3000}));
        let priced=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        assert!(!Arc::ptr_eq(&edited,&priced));
        row.client_selection=Some("MANUAL".into());
        let selected=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        assert!(!Arc::ptr_eq(&priced,&selected));
        assert!(selected.review_analysis.is_none()); // No verified cached view.
    }

    #[test]
    fn pending_review_uses_the_exact_saved_analysis_and_rejects_stale_raw(){
        let mut row=draft("synthetic");
        row.raw="발주 원문".into();
        row.analysis_raw=Some(row.raw.clone());
        let recognition=serde_json::json!({"recipients":[{"id":"r","name":"가상","items":[]}]});
        row.analysis_evidence=Some(recognition.clone());
        row.client_match=Some(serde_json::json!({"source":"MANUAL"}));
        row.analysis_view=Some(serde_json::json!({
            "recognition":recognition,"reference":[{"itemPrices":[]}],
            "clientCandidates":[],"directoryCount":0
        }));
        let mut cache=None;
        let initial=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        let actual=reception_service::ReceptionAnalysis::from_working_source(&row).unwrap();
        assert_eq!(initial.review_analysis.as_ref().unwrap().recognition,actual.recognition);
        row.analysis_raw=Some("이전 발주 원문".into());
        let stale=BandoEgui::pending_projection(&mut cache,&row).unwrap();
        assert!(!Arc::ptr_eq(&initial,&stale));
        assert!(stale.review_analysis.is_none());
    }

    #[test]
    fn pending_inline_and_vat_jobs_both_prevent_deleting_a_draft(){
        let ctx=egui::Context::default();
        let mut app=app();
        inline_worker(&mut app);
        app.delete_pending_reception("draft-1",&ctx);
        assert_eq!(app.drafts.len(),2);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        app.inline_change=None;
        vat_worker(&mut app);
        app.delete_pending_reception("draft-1",&ctx);
        assert_eq!(app.drafts.len(),2);
        assert!(app.vat_change.is_some());
    }

    #[test]
    fn blocked_manual_controls_report_reason_instead_of_ignoring_click(){
        let ctx=egui::Context::default();
        let mut app=app();
        vat_worker(&mut app);
        app.open_client_chooser(&ctx);
        assert!(app.client_chooser.is_none());
        assert!(app.source_save_error.as_deref().is_some_and(|text|
            text.contains("부가세 변경") && text.contains("거래처 변경")
        ));
        app.source_save_error=None;
        app.begin_inline_edit(
            InlineTarget{
                recipient_id:"r1".into(),item_id:None,field:"name"
            },
            "기존값".into(),None
        );
        assert!(app.inline_editor.is_none());
        assert!(app.source_save_error.as_deref().is_some_and(|text|
            text.contains("부가세 변경") && text.contains("셀 수정")
        ));
        app.source_save_error=None;
        app.open_vat_settings();
        assert!(app.vat_dialog.is_none());
        assert!(app.source_save_error.as_deref().is_some_and(|text|
            text.contains("부가세 변경")
        ));
    }

    #[test]
    fn conflicting_delete_leaves_confirmation_open_for_retry(){
        let ctx=egui::Context::default();
        let mut app=app();
        vat_worker(&mut app);
        app.workflow_dialog=Some(WorkflowDialog::DeletePending("draft-1".into()));
        assert!(!app.delete_pending_reception("draft-1",&ctx));
        assert!(app.workflow_dialog.is_some());
        assert_eq!(app.drafts.len(),2);
        app.vat_change=None;
        enable_delete_worker(&mut app,&ctx,false);
        assert!(app.delete_pending_reception("draft-1",&ctx));
        assert!(app.workflow_dialog.is_none());
        assert_eq!(app.drafts.len(),2);
        wait_delete(&mut app,&ctx);
        assert_eq!(app.drafts.len(),1);
    }

    #[test]
    fn legacy_operating_copy_metadata_does_not_block_local_draft_delete(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.drafts[0].working_version=Some(2);
        app.drafts[0].working_synced=true;
        enable_delete_worker(&mut app,&ctx,false);
        app.workflow_dialog=Some(WorkflowDialog::DeletePending("draft-1".into()));
        assert!(app.delete_pending_reception("draft-1",&ctx));
        wait_delete(&mut app,&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-2"));
        assert_eq!(app.drafts.len(),1);
        assert!(app.workflow_dialog.is_none());
    }

    #[test]
    fn approved_delete_closes_its_modal_before_switching_to_next_draft(){
        let ctx=egui::Context::default();
        let mut app=app();
        enable_delete_worker(&mut app,&ctx,false);
        app.workflow_dialog=Some(WorkflowDialog::DeletePending("draft-1".into()));
        app.delete_pending_reception("draft-1",&ctx);
        assert!(app.workflow_dialog.is_none());
        wait_delete(&mut app,&ctx);
        assert_eq!(app.drafts.len(),1);
        assert_eq!(app.active_id.as_deref(),Some("draft-2"));
        assert!(app.workflow_dialog.is_none());
    }

    #[test]
    fn confirm_request_commits_a_dirty_same_order_cell_before_advancing(){
        let ctx=egui::Context::default();
        let mut app=app();
        let raw="원문".to_owned();
        app.drafts[0].analysis_raw=Some(raw.clone());
        app.drafts[0].analysis_evidence=Some(serde_json::json!({
            "recipients":[{"id":"r1","name":"가상 수취인","address":"서울",
                "phone":"","mobile":"","note":"","items":[],"sources":[]}],
            "candidates":[],"unassigned":[]
        }));
        app.inline_editor=Some(InlineReceptionEditor{
            source_id:"draft-1".into(),
            target:InlineTarget{recipient_id:"r1".into(),item_id:None,field:"note"},
            original:String::new(),text:"직원 메모".into(),
            focus_next:false,click_at:None
        });
        app.request_draft_confirmation("draft-1",&ctx);
        assert!(app.inline_editor.is_none(),"dirty editor must be submitted");
        assert!(app.inline_change.is_some(),"native cell save must be in flight");
        assert_eq!(app.pending_draft_confirmation.as_deref(),Some("draft-1"));
    }

    #[test]
    fn confirm_request_waits_for_an_inflight_same_order_edit_and_discards_next_focus(){
        let ctx=egui::Context::default();
        let mut app=app();
        inline_worker(&mut app);
        app.inline_change.as_mut().unwrap().next=Some((
            InlineTarget{recipient_id:"r1".into(),item_id:None,field:"address"},None
        ));
        app.request_draft_confirmation("draft-1",&ctx);
        assert_eq!(app.pending_draft_confirmation.as_deref(),Some("draft-1"));
        assert!(app.inline_change.as_ref().unwrap().next.is_none(),
            "confirmation must not reopen a next-cell editor after commit");
    }

    #[test]
    fn successful_inline_commit_applies_the_cell_before_resuming_confirmation(){
        let ctx=egui::Context::default();
        let mut app=app();
        let initial_source=WorkingSource{
            raw:app.raw_source.clone(),order_type:app.order_type.as_str().into(),
            revision:0,active_id:app.active_id.clone(),drafts:app.drafts.clone()
        };
        app.source_autosave=Some(source_autosave::SourceAutosave::new(
            Arc::clone(&app.gateway),initial_source,ctx.clone()
        ));
        let target=InlineTarget{recipient_id:"r1".into(),item_id:None,field:"note"};
        let (tx,receiver)=mpsc::channel();
        app.inline_change=Some(PendingInlineChange{
            order_id:"draft-1".into(),raw:"원문".into(),target:target.clone(),
            editor:Some(InlineReceptionEditor{
                source_id:"draft-1".into(),target,original:String::new(),
                text:"저장할 메모".into(),focus_next:false,click_at:None
            }),
            next_client_review:None,next:None,receiver
        });
        app.pending_draft_confirmation=Some("draft-1".into());
        // A stale error from an earlier attempt must not block this successful
        // native cell commit from resuming the queued order confirmation.
        app.source_save_error=Some("이전 작업 오류".into());
        tx.send(Ok(manual_workspace::Event::Applied{
            order_id:"draft-1".into(),raw:"원문".into(),order_type:"AGENCY".into(),
            client_id:None,direct_input:false,
            model:serde_json::json!({"rows":[{"id":"r1","note":"저장할 메모","items":[]}],
                "blocks":[],"assignments":{}}),
            assignment_order:Vec::new(),
            order:serde_json::json!({"recipients":[{"id":"r1","note":"저장할 메모","items":[]}]}),
            quote:None,quote_error:None
        })).unwrap();
        app.poll_inline_change(&ctx);
        assert!(app.inline_change.is_none());
        assert!(app.pending_draft_confirmation.is_none());
        assert_eq!(app.drafts[0].manual_model.as_ref().unwrap()
            .pointer("/model/rows/0/note").and_then(serde_json::Value::as_str),
            Some("저장할 메모"));
        assert!(app.source_save_error.is_none());
    }

    #[test]
    fn failed_inline_commit_clears_confirmation_wait_and_keeps_the_error(){
        let ctx=egui::Context::default();
        let mut app=app();
        inline_worker(&mut app);
        let (tx,receiver)=mpsc::channel();
        app.inline_change.as_mut().unwrap().receiver=receiver;
        app.pending_draft_confirmation=Some("draft-1".into());
        tx.send(Err("운영 데이터 저장 실패".into())).unwrap();

        app.poll_inline_change(&ctx);

        assert!(app.inline_change.is_none());
        assert!(app.pending_draft_confirmation.is_none());
        assert!(app.source_save_error.as_deref().is_some_and(|error|
            error.contains("운영 데이터 저장 실패")
        ));
        assert!(app.inline_editor.is_some(),
            "a failed cell write must preserve the employee's editable value");
    }

    #[test]
    fn invalid_confirmed_cell_keeps_editor_and_discards_clicked_destination(){
        let ctx=egui::Context::default();
        let mut app=app();
        let selection=workflow_board_ui::Selection{
            group_id:"g".into(),order_id:"o".into(),shipment_id:"s".into()
        };
        let quantity=InlineTarget{recipient_id:"r1".into(),item_id:Some("i".into()),field:"quantity"};
        app.confirmed_cell=Some(ConfirmedCellEditor{
            target:ConfirmedEditTarget{selection:selection.clone(),revision:1,recipient_id:"r1".into()},
            cell:InlineReceptionEditor{source_id:"o".into(),target:quantity.clone(),
                original:"1".into(),text:"0".into(),focus_next:false,click_at:None}
        });
        app.switch_confirmed_editor(selection,InlineTarget{
            recipient_id:"r1".into(),item_id:None,field:"name"
        },Some(egui::pos2(20.0,20.0)),false,&ctx);
        let editor=app.confirmed_cell.as_ref().unwrap();
        assert_eq!(editor.cell.target,quantity);
        assert_eq!(editor.cell.text,"0");
        assert!(editor.cell.focus_next);
        assert!(app.confirmed_next_cell.is_none());
        assert!(app.workflow_action.is_none());
    }

    #[test]
    fn clean_inline_editor_does_not_block_switch_but_dirty_editor_does(){
        let ctx=egui::Context::default();
        let mut app=app();
        let target=InlineTarget {
            recipient_id:"r1".into(),item_id:None,field:"name"
        };
        app.inline_editor=Some(InlineReceptionEditor{
            source_id:"draft-1".into(),target:target.clone(),
            original:"김준희".into(),text:"김준희".into(),
            focus_next:false,click_at:None,
        });
        assert_eq!(app.reception_job_admission(
            ReceptionJob::NavigateReception,false
        ),JobAdmission::Start);
        app.select_reception("draft-2",&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-2"));
        assert!(app.inline_editor.is_none());
        app.select_reception("draft-1",&ctx);
        app.inline_editor=Some(InlineReceptionEditor{
            source_id:"draft-1".into(),target,
            original:"김준희".into(),text:"수정 중".into(),
            focus_next:false,click_at:None,
        });
        assert_eq!(app.reception_job_admission(
            ReceptionJob::NavigateReception,false
        ),JobAdmission::Conflict(ReceptionJob::InlineEditor));
        app.select_reception("draft-2",&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert_eq!(app.inline_editor.as_ref().unwrap().text,"수정 중");
    }

    #[test]
    fn blank_direct_input_can_be_abandoned_without_stuck_job(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.raw_source.clear();
        app.drafts[0].raw.clear();
        app.source_editing=true;
        app.direct_source_entry=true;
        app.new_reception(&ctx);
        assert!(!app.direct_source_entry);
        assert!(!app.source_editing);
        assert_eq!(app.drafts.len(),2);
        // The same empty draft is reused instead of multiplying blanks.
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
    }

    #[test]
    fn direct_manual_model_restores_reception_view_without_source_analysis(){
        let mut app=app();
        app.raw_source.clear();
        let row=&mut app.drafts[0];
        row.raw.clear();
        row.analysis_raw=None;
        row.analysis_evidence=None;
        row.analysis_view=None;
        row.manual_model=Some(serde_json::json!({
            "mode":"DIRECT","sourceRaw":"","orderType":"AGENCY","clientId":null,
            "assignmentOrder":[],"model":{
                "rows":[{"id":"egui-manual-r1","name":"직접 입력 수취인",
                    "address":"서울특별시 강남구 테헤란로 10",
                    "phone":"","mobile":"","note":"","items":[]}],
                "blocks":[],"assignments":{}
            }
        }));
        assert!(app.restore_cached_analysis());
        let analysis=app.analysis.as_ref().expect("direct worksheet projection");
        let recipients=reception_service::recipients(analysis);
        assert_eq!(recipients.len(),1);
        assert_eq!(recipients[0]["name"],"직접 입력 수취인");
        assert!(analysis.linked_spans.is_empty());
        assert!(app.drafts[0].analysis_evidence.is_none());
        assert!(app.drafts[0].analysis_view.is_none());
    }

    #[test]
    fn saved_direct_entry_is_not_reused_as_the_next_new_reception(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.raw_source.clear();
        app.drafts[0].raw.clear();
        app.drafts[0].manual_model=Some(serde_json::json!({
            "mode":"DIRECT","sourceRaw":"","orderType":"AGENCY","clientId":null,
            "model":{"rows":[{"id":"egui-manual-r1","name":"saved","items":[]}],
                "blocks":[],"assignments":{}},
            "assignmentOrder":[]
        }));
        app.new_reception(&ctx);
        assert_eq!(app.drafts.len(),3);
        assert_ne!(app.active_id.as_deref(),Some("draft-1"));
        assert_eq!(app.drafts[0].manual_model.as_ref().unwrap()["mode"],"DIRECT");
    }

    #[test]
    fn approved_deletion_cancels_pending_advisory_analysis_without_waiting(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.analysis_pending=true;
        app.analysis_running_key=Some((
            "draft-1".into(),"원문".into(),"AGENCY".into()
        ));
        enable_delete_worker(&mut app,&ctx,false);
        app.delete_pending_reception("draft-1",&ctx);
        wait_delete(&mut app,&ctx);
        assert_eq!(app.drafts.len(),1);
        assert_eq!(app.active_id.as_deref(),Some("draft-2"));
        assert!(!app.analysis_pending);
        assert!(app.analysis_running_key.is_none());
    }

    #[test]
    fn direct_source_can_switch_receptions_with_existing_draft_preserved(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.direct_source_entry=true;
        app.source_editing=true;
        app.select_reception("draft-2",&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-2"));
        assert!(!app.direct_source_entry);
        assert_eq!(app.drafts[0].raw,"원문");
    }

    #[test]
    fn replacement_approval_conflict_keeps_corrected_text_for_retry(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.pending_replace=Some(PendingReplace{
            text:"정정된 발주원문".into(),order_type:OrderType::Dealer,
            chat:None,input_method:"OCR",clipboard_sequence:None,
            ocr_original:Some("오인식 원문".into()),
            ocr_learning_used:true,ocr_model_suggestion:None
        });
        vat_worker(&mut app);
        assert!(!app.approve_pending_replace(&ctx));
        assert!(app.pending_replace.is_some());
        assert_eq!(app.raw_source,"원문");
        assert_eq!(app.pending_replace.as_ref().unwrap().text,"정정된 발주원문");
        app.vat_change=None;
        assert!(app.approve_pending_replace(&ctx));
        assert!(app.pending_replace.is_none());
        assert_eq!(app.raw_source,"정정된 발주원문");
    }

    #[test]
    fn running_vat_write_blocks_source_replacement_and_navigation(){
        let ctx=egui::Context::default();
        let mut app=app();
        vat_worker(&mut app);
        app.apply_intake_text(
            "다른 원문".into(),OrderType::Dealer,None,&ctx,
            "TEXT_PASTE",true,None,None,false,None
        );
        assert_eq!(app.raw_source,"원문");
        app.select_reception("draft-2",&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert!(app.source_save_error.as_deref()
            .is_some_and(|s|s.contains("부가세 변경")));
    }

    #[test]
    fn failed_delivery_command_never_restores_a_hidden_editor(){
        let ctx=egui::Context::default();
        for field in ["remoteDelivery","freightCode","dispatchOrigin"]{
            let mut app=app();
            let (tx,receiver)=mpsc::channel();
            app.inline_change=Some(PendingInlineChange{
                order_id:app.active_id.clone().unwrap(),raw:app.raw_source.clone(),
                target:InlineTarget{recipient_id:"r1".into(),item_id:None,field},
                editor:None,next_client_review:None,next:None,receiver
            });
            tx.send(Err("synthetic delivery failure".into())).unwrap();
            app.poll_inline_change(&ctx);
            assert!(app.inline_change.is_none());
            assert!(app.inline_editor.is_none());
            assert!(app.source_save_error.is_some());
            assert_eq!(app.reception_job_admission(ReceptionJob::ManualClient,false),JobAdmission::Start);
        }
    }

    // No filesystem, SQLite or operational gateway access in this fixture.
    struct PricingBasisReadFixture;
    impl gateway::BandoGateway for PricingBasisReadFixture{
        fn label(&self)->&'static str{"pricing-basis-navigation-test"}
        fn load_catalog(&self)->Result<serde_json::Value,String>{Err("fixture".into())}
        fn load_working_source(&self)->Result<WorkingSource,String>{Err("fixture".into())}
        fn save_working_source(&self,_source:&WorkingSource)->Result<u64,String>{Err("fixture".into())}
        fn execute(&self,_op:gateway::ReadOperation,_value:serde_json::Value)
            ->Result<serde_json::Value,String>{Err("fixture".into())}
        fn load_business_state(&self)->Result<serde_json::Value,String>{
            Ok(serde_json::json!({"clients":[{"id":"c","tier":"wholesale"}]}))
        }
    }

    #[test]
    fn reception_metadata_tests_rechecks_quote_on_return_before_snapshot_expiry(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.gateway=Arc::new(PricingBasisReadFixture);
        app.drafts[0].client_id=Some("c".into());
        app.drafts[0].client_selection=Some("MANUAL".into());
        app.drafts[0].pricing_preview=Some(serde_json::json!({
            "linePrices":[],"freightLines":[],"pendingRecipients":[],
            "pricingBasis":{"orderInputs":{"recipients":[]}}
        }));
        let now=Instant::now();
        app.page=Page::Products;
        app.sync_navigation_snapshot(&ctx,now);
        assert!(app.pricing_basis_check.is_none());
        app.page=Page::Reception;
        app.sync_navigation_snapshot(&ctx,now);
        assert!(app.pricing_basis_check.is_some(),
            "retained reception must check its price and freight basis on return");
        assert_eq!(app.navigation_snapshots.epoch(Page::Reception as usize),0);
        // Repeated frame sync must not spawn another check.
        app.pricing_basis_check.take();
        app.sync_navigation_snapshot(&ctx,now);
        assert!(app.pricing_basis_check.is_none());
    }

    #[test]
    fn shared_reception_metadata_keeps_cached_values_on_unchanged_revision(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.page=Page::Reception;
        app.reception_metadata_revision=Some(7);
        app.reception_metadata_ready=true;
        app.reception_metadata_last_probe=Some(Instant::now());
        app.inbox_folder_path=Some("saved folder".into());
        app.inbox_folder_loaded=true;
        app.client_names.insert("c".into(),"거래처".into());
        let finish=app.reception_metadata_read.test_advisory_read();
        finish(Ok((Some(7),None)));
        app.poll_reception_metadata(&ctx);
        assert_eq!(app.inbox_folder_path.as_deref(),Some("saved folder"));
        assert_eq!(app.client_names.get("c").map(String::as_str),Some("거래처"));
        assert!(!app.reception_metadata_read.pending());
        app.invalidate_reception_metadata();
        assert!(app.reception_metadata_revision.is_none());
        assert!(!app.reception_metadata_ready);
    }

    #[test]
    fn unchanged_workflow_revision_keeps_current_snapshot_and_selection(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.workflow_snapshot=Some(Arc::new(workflow_board::BoardSnapshot{revision:Some(7),groups:vec![]}));
        app.workflow_ui.selection=Some(workflow_board_ui::Selection{
            group_id:"g1".into(),order_id:"o1".into(),shipment_id:"s1".into()
        });
        let finish=app.workflow_read.test_advisory_read();
        app.workflow_last_fetch=Some(Instant::now());
        finish(Ok(None));
        app.poll_workflow_board(&ctx);
        assert!(!app.workflow_read.pending());
        assert_eq!(app.workflow_snapshot.as_ref().and_then(|board|board.revision),Some(7));
        assert_eq!(app.workflow_ui.selection.as_ref().map(|s|s.shipment_id.as_str()),Some("s1"));
        assert!(app.workflow_mutation_ready());
    }

    #[test]
    fn factory_reply_refresh_discards_precommit_reads_without_erasing_revision_baseline(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.page=Page::Products;
        app.reception_metadata_revision=Some(7);
        app.reception_metadata_ready=true;
        let finish=app.workflow_read.test_advisory_read();
        app.workflow_last_fetch=Some(Instant::now());
        app.refresh_after_factory_reply(&ctx,false,false);
        assert!(app.workflow_read.pending());
        assert!(!app.workflow_read.required());
        app.refresh_after_factory_reply(&ctx,false,true);
        finish(Ok(None));
        assert!(app.workflow_read.poll().is_none());
        assert!(!app.workflow_read.pending());
        assert!(app.workflow_last_fetch.is_none());
        assert!(app.workflow_read.required());
        assert_eq!(app.reception_metadata_revision,Some(7));
        assert!(app.reception_metadata_ready);
        assert!(app.reception_metadata_last_probe.is_none());
    }

    fn stage_test_board()->workflow_board::BoardSnapshot{
        let ship=workflow_board::ShipmentRow{
            id:"s1".into(),order_id:"draft-1".into(),recipient_id:"r1".into(),
            transferred:false,recipient_name:"synthetic".into(),products:vec![],
            completed_step:Some(1),paid:None,
            source_raw:"원문".into(),recipient:serde_json::json!({}),remote:None,
            display_charges:HashMap::new(),freight_editing_blocked:false,freight_exempt:false,
            collect:false,shipment_count:1,order_total:None
        };
        workflow_board::BoardSnapshot{revision:Some(8),groups:vec![workflow_board::GroupRow{
            id:"group:draft-1".into(),created_at:String::new(),dealer_name:String::new(),
            source_id:String::new(),source_number:None,dealer_id:String::new(),transferred:false,step:1,
            stage_validity:[true,false,false,false],needs_notice:false,total:None,vat_included:0,vat_excluded:0,shipments:vec![ship],
            automatic_text:None,automatic_text_with_links:None,automatic_error:None,automatic_phase:None,automatic_signature:None,
            payment_copied:false,automatic_payment_issues:vec![],issues:vec![]
        }]}
    }

    #[test]
    fn group_delete_confirmation_survives_failed_save_and_closes_on_matching_commit(){
        let mut app=app();
        let ctx=egui::Context::default();
        let id="group:draft-1";
        app.workflow_snapshot=Some(Arc::new(stage_test_board()));
        app.retain_workflow_stage_target(id);
        app.workflow_dialog=Some(WorkflowDialog::DeleteGroup(id.into()));
        let (tx,rx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{
            group_id:id.into(),action:"delete_group".into(),notice_origin:None,receiver:rx
        });
        tx.send(Err("synthetic deletion failure".into())).unwrap();
        app.poll_workflow_action(&ctx);
        assert!(matches!(app.workflow_dialog.as_ref(),Some(WorkflowDialog::DeleteGroup(target)) if target==id));
        assert_eq!(app.workflow_snapshot.as_ref().unwrap().groups.len(),1);
        assert_eq!(app.workflow_ui.selection.as_ref().unwrap().shipment_id,"s1");
        let (tx,rx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{
            group_id:id.into(),action:"delete_group".into(),notice_origin:None,receiver:rx
        });
        tx.send(Ok(serde_json::json!({}))).unwrap();
        app.poll_workflow_action(&ctx);
        assert!(app.workflow_dialog.is_none());
        assert!(app.workflow_read.required());
    }

    #[test]
    fn payment_notice_failure_keeps_confirmation_for_retry(){
        let mut app=app();
        let ctx=egui::Context::default();
        let id="group:draft-1";
        app.workflow_dialog=Some(WorkflowDialog::PaymentNotice(id.into()));
        let (tx,rx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{
            group_id:id.into(),action:"advance_group".into(),notice_origin:None,receiver:rx
        });
        drop(tx);
        app.poll_workflow_action(&ctx);
        assert!(matches!(app.workflow_dialog.as_ref(),Some(WorkflowDialog::PaymentNotice(target)) if target==id));
        let (tx,rx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{
            group_id:id.into(),action:"advance_group".into(),notice_origin:None,receiver:rx
        });
        tx.send(Ok(serde_json::json!({}))).unwrap();
        app.poll_workflow_action(&ctx);
        assert!(app.workflow_dialog.is_none());
    }

    #[test]
    fn explicit_stage_click_retains_group_without_pointer_preview(){
        let mut app=app();
        let mut board=stage_test_board();
        let mut second=board.groups[0].shipments[0].clone();
        second.id="s2".into();
        second.recipient_id="r2".into();
        board.groups[0].shipments.push(second);
        app.workflow_snapshot=Some(Arc::new(board));
        app.workflow_ui.selection=Some(workflow_board_ui::Selection{
            group_id:"group:draft-1".into(),order_id:"draft-1".into(),shipment_id:"s2".into()
        });
        app.workflow_ui.folded=Some("group:draft-1".into());
        app.retain_workflow_stage_target("group:draft-1");
        app.workflow_ui.stop_preview();
        assert_eq!(app.visible_workflow_group().map(|(b,g,s)|b.groups[g].shipments[s].id.clone()),Some("s2".into()));
        assert_eq!(app.workflow_ui.folded.as_deref(),Some("group:draft-1"));
        app.workflow_ui.selection=None;
        app.retain_workflow_stage_target("group:draft-1");
        assert!(app.visible_workflow_group().is_some());
        assert_eq!(app.workflow_ui.folded.as_deref(),Some("group:draft-1"));
    }

    #[test]
    fn refreshed_projection_preserves_same_shipment_across_regrouping(){
        let mut app=app();
        let old=stage_test_board();
        app.workflow_snapshot=Some(Arc::new(old.clone()));
        app.retain_workflow_stage_target("group:draft-1");
        let mut refreshed=old;
        refreshed.groups[0].id="regrouped".into();
        app.reconcile_workflow_selection(&refreshed);
        let selected=app.workflow_ui.selection.as_ref().unwrap();
        assert_eq!(selected.group_id,"regrouped");
        assert_eq!(selected.shipment_id,"s1");
        assert_eq!(app.workflow_ui.folded.as_deref(),Some("regrouped"));
        refreshed.groups[0].shipments[0].order_id="another-order".into();
        app.reconcile_workflow_selection(&refreshed);
        assert!(app.workflow_ui.selection.is_none());
    }

    #[test]
    fn nonactive_draft_stage_click_survives_preview_clear_and_handoff(){
        let mut app=app();
        let ctx=egui::Context::default();
        app.active_id=Some("draft-2".into());
        app.pending_stage_display=Some(workflow_board_ui::PendingTarget{
            order_id:"draft-1".into(),recipient_id:Some("r1".into())
        });
        let (tx,rx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{group_id:"group:draft-1".into(),
            action:"confirm_reception".into(),notice_origin:None,receiver:rx});
        app.workflow_ui.stop_preview();
        assert_eq!(app.pending_stage_target().map(|t|t.order_id.as_str()),Some("draft-1"));
        tx.send(Ok(serde_json::json!({}))).unwrap();
        app.poll_workflow_action(&ctx);
        assert_eq!(app.pending_stage_target().map(|t|t.order_id.as_str()),Some("draft-1"));
        assert!(app.finish_confirmed_draft_handoff(&stage_test_board(),&ctx));
        assert_eq!(app.active_id.as_deref(),Some("draft-2"));
        assert_eq!(app.workflow_ui.selection.as_ref().unwrap().shipment_id,"s1");
        assert_eq!(app.workflow_ui.folded.as_deref(),Some("group:draft-1"));
        assert!(app.pending_stage_target().is_none());
    }

    #[test]
    fn rejected_new_order_keeps_current_workflow_selection(){
        let mut app=app();
        let ctx=egui::Context::default();
        app.workflow_snapshot=Some(Arc::new(stage_test_board()));
        app.retain_workflow_stage_target("group:draft-1");
        app.workflow_read.require();
        app.new_reception(&ctx);
        assert_eq!(app.workflow_ui.selection.as_ref().unwrap().shipment_id,"s1");
        assert_eq!(app.drafts.len(),2);
    }

    #[test]
    fn committed_confirmation_keeps_draft_until_matching_board_is_ready(){
        let mut app=app();
        let ctx=egui::Context::default();
        let (tx,rx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{group_id:"group:draft-1".into(),
            action:"confirm_reception".into(),notice_origin:None,receiver:rx});
        tx.send(Ok(serde_json::json!({}))).unwrap();
        app.poll_workflow_action(&ctx);
        assert_eq!(app.drafts.len(),2);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert_eq!(app.confirming_order(),Some("draft-1"));
        assert!(!app.workflow_mutation_ready());
        assert!(!app.delete_pending_reception("draft-1",&ctx));
        assert!(!app.finish_confirmed_draft_handoff(&workflow_board::BoardSnapshot::default(),&ctx));
        assert_eq!(app.drafts.len(),2);
        let board=stage_test_board();
        assert!(app.finish_confirmed_draft_handoff(&board,&ctx));
        assert_eq!(app.drafts.len(),1);
        assert!(app.active_id.is_none());
        assert_eq!(app.workflow_ui.selection.as_ref().unwrap().shipment_id,"s1");
        assert!(app.confirming_order().is_none());
    }

    #[test]
    fn periodic_workflow_read_is_advisory_but_post_commit_refresh_is_mandatory(){
        let mut app=app();
        app.workflow_snapshot=Some(Arc::new(workflow_board::BoardSnapshot{
            revision:Some(7),groups:vec![]
        }));
        let _finish=app.workflow_read.test_advisory_read();
        assert!(app.workflow_mutation_ready());
        app.discard_advisory_workflow_read();
        assert!(!app.workflow_read.pending());
        let _finish=app.workflow_read.test_advisory_read();
        app.workflow_read.require();
        assert!(!app.workflow_mutation_ready());
        app.discard_advisory_workflow_read();
        assert!(app.workflow_read.pending());
        assert!(!workflow_write_admission(None,false,false,false));
        assert!(!workflow_write_admission(Some(7),true,false,false));
        assert!(!workflow_write_admission(Some(7),false,false,true));
    }

    #[test]
    fn advisory_analysis_allows_unrelated_work_but_employee_write_blocks_capture(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.analysis_pending=true;
        assert_eq!(
            app.reception_job_admission(ReceptionJob::SourceIntake,false),
            JobAdmission::Start
        );
        assert_eq!(
            app.reception_job_admission(ReceptionJob::WorkflowAction,false),
            JobAdmission::Start
        );
        app.analysis_pending=false;
        vat_worker(&mut app);
        app.begin_region_capture(OrderType::Agency,ctx.clone());
        assert!(!app.clipboard_busy);
        assert!(app.clipboard_receiver.is_none());
        app.start_workflow_action(
            "advance_group","group",None,None,false,&ctx
        );
        assert!(app.workflow_action.is_none());
    }

    #[test]
    fn pending_clipboard_or_review_prevents_switching_drafts(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.clipboard_busy=true;
        app.select_reception("draft-2",&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        app.clipboard_busy=false;
        app.pending_replace=Some(PendingReplace{
            text:"새 내용".into(),order_type:OrderType::Agency,
            chat:None,input_method:"TEXT_PASTE",
            clipboard_sequence:None,ocr_original:None,
            ocr_learning_used:false,ocr_model_suggestion:None
        });
        app.select_reception("draft-2",&ctx);
        assert_eq!(app.active_id.as_deref(),Some("draft-1"));
        assert!(app.pending_replace.is_some());
    }

    #[test]
    fn locality_lookup_dedupes_and_cancels_outdated_recipient_snapshot(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.analysis=Some(remote_analysis());
        app.start_remote_recheck(&ctx);
        let first=Arc::clone(&app.remote_recheck.as_ref().unwrap().cancel);
        app.start_remote_recheck(&ctx);
        assert!(Arc::ptr_eq(&first,&app.remote_recheck.as_ref().unwrap().cancel));
        app.analysis.as_mut().unwrap().recognition["recipients"][0]["id"]=
            serde_json::json!("r2");
        app.start_remote_recheck(&ctx);
        assert!(first.load(Ordering::Acquire),
            "a newer recipient snapshot must cancel obsolete remote work");
        assert!(!Arc::ptr_eq(&first,&app.remote_recheck.as_ref().unwrap().cancel));
        assert_eq!(app.remote_recheck.as_ref().unwrap().recipients[0].0,"r2");
    }

    #[test]
    fn every_reception_job_owner_is_visible_to_common_admission(){
        let mut app=app();
        app.analysis_pending=true;
        app.clipboard_busy=true;
        app.direct_source_entry=true;
        app.pending_replace=Some(PendingReplace{
            text:"수정".into(),order_type:OrderType::Dealer,
            chat:None,input_method:"OCR",clipboard_sequence:None,
            ocr_original:None,ocr_learning_used:false,ocr_model_suggestion:None
        });
        let (_rtx,rrx)=mpsc::channel();
        app.remote_recheck=Some(PendingRemoteRecheck{
            order_id:"draft-1".into(),source_raw:"원문".into(),
            order_type:"AGENCY".into(),recipients:Vec::new(),
            cancel:Arc::new(AtomicBool::new(false)),receiver:rrx
        });
        let (_wtx,wrx)=mpsc::channel();
        app.workflow_action=Some(PendingWorkflowAction{
            group_id:"group".into(),action:"advance_group".into(),notice_origin:None,
            receiver:wrx
        });
        app.vat_dialog=Some(VatDialog::new(
            "draft-1".into(),"원문".into(),&[]
        ));
        app.workflow_dialog=Some(WorkflowDialog::ManageGroup("group".into()));
        let active=app.active_reception_jobs(ReceptionJob::DeleteReception);
        for expected in [
            ReceptionJob::Analysis,ReceptionJob::RemoteRecheck,
            ReceptionJob::SourceIntake,ReceptionJob::SourceReview,
            ReceptionJob::SourceEntry,ReceptionJob::WorkflowAction,
            ReceptionJob::VatDialog,ReceptionJob::WorkflowDialog
        ]{
            assert!(active.contains(&expected),"{expected:?} absent from job inventory");
        }
        assert!(matches!(
            app.reception_job_admission(ReceptionJob::DeleteReception,false),
            JobAdmission::Conflict(_)
        ));
    }

    #[test]
    fn changing_only_reception_kind_invalidates_old_analysis_and_quote(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.drafts[0].analysis_raw=Some("원문".into());
        app.drafts[0].analysis_evidence=Some(
            serde_json::json!({"recipients":[]})
        );
        app.drafts[0].pricing_preview=Some(
            serde_json::json!({"totals":{"grandTotal":12000}})
        );
        app.apply_intake_text(
            "원문".into(),OrderType::Dealer,None,&ctx,
            "TEXT_PASTE",true,None,None,false,None
        );
        assert_eq!(app.order_type,OrderType::Dealer);
        assert_eq!(app.drafts[0].order_type,"DEALER");
        assert!(app.drafts[0].analysis_evidence.is_none());
        assert!(app.drafts[0].pricing_preview.is_none());
    }

    #[test]
    fn locality_lookup_retries_after_a_conflicting_write_finishes(){
        let ctx=egui::Context::default();
        let mut app=app();
        app.analysis=Some(remote_analysis());
        vat_worker(&mut app);
        app.start_remote_recheck(&ctx);
        assert!(app.remote_recheck.is_none());
        assert!(app.remote_recheck_due);
        app.vat_change=None;
        app.start_remote_recheck(&ctx);
        assert!(app.remote_recheck.is_some());
        assert!(!app.remote_recheck_due);
    }
}


#[cfg(test)]
mod modal_work_area_geometry_tests{
    use super::*;

    #[test]
    fn kakao_safe_modal_area_matches_workspace_shell_asymmetric_insets(){
        let viewport=Rect::from_min_size(
            egui::pos2(0.0,0.0),Vec2::new(1920.0,1080.0)
        );
        let layout=layout::LayoutState::default();
        let work=kakao_safe_modal_work_area(viewport,&layout);
        let content_left=viewport.left();
        let content_right=viewport.right()-theme::PANEL_GAP;
        let content_width=content_right-content_left;
        let kakao_edge=content_left+layout.widths(content_width)[0]
            +layout::SPLITTER;
        let gutter=theme::MODAL_VIEWPORT_GUTTER;

        assert!((work.left()-(kakao_edge+gutter)).abs()<0.01);
        assert!((work.right()-(content_right-gutter)).abs()<0.01);
        assert!((work.top()-(viewport.top()+theme::HEADER_HEIGHT
            +theme::PANEL_GAP+gutter)).abs()<0.01);
        assert!((work.bottom()-(viewport.bottom()
            -theme::PANEL_GAP-gutter)).abs()<0.01);
        assert!((work.center().x-(kakao_edge+gutter
            +content_right-gutter)/2.0).abs()<0.01);
    }
}

#[cfg(test)]
mod reception_item_editor_integration_tests{
    use super::*;
    fn populated_app()->BandoEgui{
        let mut app=BandoEgui::default();
        app.active_id=Some("o1".into());
        app.raw_source="원문 709 4개".into();
        app.drafts.push(WorkingReception{
            id:"o1".into(),raw:app.raw_source.clone(),
            order_type:"AGENCY".into(),source_input_method:None,
            last_input_method:None,analysis_raw:Some(app.raw_source.clone()),
            analysis_evidence:Some(serde_json::json!({"recipients":[
                {"id":"r1","items":[{"id":"i1","name":"BD 709 / 블랙 / 타공 상판",
                    "productId":"p1","variantId":"v1","quantity":4,
                    "quantityConfirmed":true,"unitPrice":59000}]}
            ]})),analysis_view:None,client_match:None,
            chat_handle:None,chat_title:None,
            client_id:Some("c1".into()),client_selection:Some("MANUAL".into()),
            pricing_preview:None,working_version:None,
            working_synced:false,manual_model:None,edit_history:Vec::new()
        });
        app.analysis=Some(reception_service::ReceptionAnalysis{
            recognition:serde_json::json!({"recipients":[{
                "id":"r1","items":[{
                    "id":"i1","name":"BD 709 / 블랙 / 타공 상판",
                    "productId":"p1","variantId":"v1","quantity":4,
                    "quantityConfirmed":true,"unitPrice":59000
                }]
            }]}),reference:Vec::new(),
            client_match:serde_json::Value::Null,
            client_candidates:Vec::new(),matched_client_name:None,
            chosen_client_name:None,directory_count:0,
            linked_spans:Vec::new()
        });
        app
    }
    #[test]
    fn product_cell_opens_order_scoped_modal_instead_of_inline_text_editor(){
        let ctx=egui::Context::default();
        let mut app=populated_app();
        app.open_item_editor("r1","i1",&ctx);
        let dialog=app.item_editor.as_ref().expect("product detail dialog");
        assert!(dialog.is_same_target("o1","원문 709 4개","r1","i1"));
        assert!(app.inline_editor.is_none(),
            "React product clicks must not open a plain TextEdit");
        assert_eq!(app.reception_job_admission(
            ReceptionJob::NavigateReception,false
        ),JobAdmission::Conflict(ReceptionJob::ItemDialog));
        assert_eq!(app.reception_job_admission(
            ReceptionJob::InlineEdit,false
        ),JobAdmission::Start,
            "the modal must be permitted to submit its native item write");
    }
    #[test]
    fn item_dialog_never_opens_with_uncommitted_edited_quantity(){
        let ctx=egui::Context::default();
        let mut app=populated_app();
        app.inline_editor=Some(InlineReceptionEditor{
            source_id:"o1".into(),
            target:InlineTarget{
                recipient_id:"r1".into(),item_id:Some("i1".into()),
                field:"quantity"
            },
            original:"4".into(),text:"7".into(),
            focus_next:false,click_at:None
        });
        app.open_item_editor("r1","i1",&ctx);
        assert!(app.item_editor.is_none());
        assert_eq!(app.inline_editor.as_ref().unwrap().text,"7");
    }
    #[test]
    fn unknown_product_item_does_not_open_dialog(){
        let ctx=egui::Context::default();
        let mut app=populated_app();
        app.open_item_editor("r1","no-item",&ctx);
        assert!(app.item_editor.is_none());
    }
}
