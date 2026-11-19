//! BANDO HUB egui — first native visual implementation.
//! UI preview only: uses existing read-only Rust business decisions and
//! a separate egui-preview.sqlite3 for source continuity. Existing React/Tauri
//! stays untouched. A production client master may be read ONLY by explicit
//! read-only opt-in; KakaoTalk is placed by a separate reversible Win32 dock.

mod theme;
mod typography;
mod layout;
mod components;
mod reception_service;
mod gateway;
mod http_gateway;
mod preview_store;
mod source_autosave;
mod clipboard_intake;
mod client_directory;
mod client_master;
mod client_manager;
mod client_chooser;
mod business_state;
mod native_order_change;
mod source_highlight;
mod workflow_board;
mod workflow_board_ui;
mod manual_workspace;
mod catalog_query_bridge;
mod manual_product_search;
mod manual_source_view;
mod kakao_context;
mod kakao_dock;

use eframe::egui::{self, Align, Frame, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use theme::{ButtonRole, WindowControl};
use clipboard_intake::{ClipboardImage, ClipboardMonitor, ClipboardPayload, OrderType};
use preview_store::{WorkingReception, WorkingSource};
use kakao_context::{ChatWindow, KakaoTracker};
use client_chooser::{ClientChooser, Choice};
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

// Only accepted clipboard text/OCR text is ever committed to the egui
// working source. Images remain in memory for the existing review workflow.
enum ClipboardReceipt {
    Text(String),
    Ocr { text: String, image: ClipboardImage },
}
struct OcrReview {
    order_type: OrderType,
    original_text: String,
    edited_text: String,
    texture: egui::TextureHandle,
    width: usize,
    height: usize,
    chat: Option<ChatWindow>,
}
struct PendingReplace {
    text: String,
    order_type: OrderType,
    chat: Option<ChatWindow>,
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
    source_raw: String,
    order_type: String,
    receiver: Receiver<Result<native_order_change::ChangeResult, String>>,
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

struct BandoEgui {
    brand_texture: Option<egui::TextureHandle>,
    gateway: Arc<dyn gateway::BandoGateway>,
    page: Page,
    notifications_open: bool,
    layout: layout::LayoutState,
    dragging_split: Option<HorizontalSplitDrag>,
    dragging_vertical: Option<VerticalSplitDrag>,
    raw_source: String,
    drafts: Vec<WorkingReception>,
    active_id: Option<String>,
    source_editing: bool,
    order_type: OrderType,
    clipboard_monitor: ClipboardMonitor,
    kakao_tracker: KakaoTracker,
    kakao_dock: kakao_dock::KakaoDock,
    kakao_dock_error: Option<String>,
    clipboard_chat: Option<ChatWindow>,
    clipboard_busy: bool,
    ocr_processing: bool,
    clipboard_cancel: Arc<AtomicBool>,
    clipboard_receiver: Option<Receiver<(OrderType, Result<ClipboardReceipt, String>)>>,
    clipboard_error: Option<String>,
    ocr_review: Option<OcrReview>,
    discard_ocr_confirm: bool,
    pending_replace: Option<PendingReplace>,
    source_autosave: Option<source_autosave::SourceAutosave>,
    source_save_error: Option<String>,
    client_chooser: Option<ClientChooser>,
    client_manager: Option<client_manager::ClientManager>,
    manual_workspace: Option<manual_workspace::ManualWorkspace>,
    client_change: Option<PendingClientChange>,
    working_sync: Option<PendingWorkingSync>,
    working_sync_error: Option<String>,
    analysis: Option<reception_service::ReceptionAnalysis>,
    analysis_error: Option<String>,
    selected_recipient: usize,
    source_hover: Option<source_highlight::LinkKey>,
    link_hover: source_highlight::HoverState,
    workflow_snapshot: Option<workflow_board::BoardSnapshot>,
    workflow_error: Option<String>,
    workflow_receiver: Option<Receiver<Result<workflow_board::BoardSnapshot,String>>>,
    workflow_last_fetch: Option<Instant>,
    workflow_ui: workflow_board_ui::BoardUi,
    analysis_request: u64,
    analysis_pending: bool,
    analysis_due_at: Option<Instant>,
    analysis_receiver: Option<Receiver<(u64, Result<reception_service::ReceptionAnalysis, String>)>>,
}

impl Default for BandoEgui {
    fn default() -> Self {
        Self {
            brand_texture: None,
            gateway: Arc::new(gateway::LocalGateway),
            page: Page::Reception,
            notifications_open: false,
            layout: layout::LayoutState::load(),
            dragging_split: None,
            dragging_vertical: None,
            raw_source: String::new(),
            drafts: Vec::new(),
            active_id: None,
            source_editing: false,
            order_type: OrderType::Agency,
            clipboard_monitor: ClipboardMonitor::default(),
            kakao_tracker: KakaoTracker::default(),
            kakao_dock: kakao_dock::KakaoDock::default(),
            kakao_dock_error: None,
            clipboard_chat: None,
            clipboard_busy: false,
            ocr_processing: false,
            clipboard_cancel: Arc::new(AtomicBool::new(false)),
            clipboard_receiver: None,
            clipboard_error: None,
            ocr_review: None,
            discard_ocr_confirm: false,
            pending_replace: None,
            source_autosave: None,
            source_save_error: None,
            client_chooser: None,
            client_manager: None,
            manual_workspace: None,
            client_change: None,
            working_sync: None,
            working_sync_error: None,
            analysis: None,
            analysis_error: None,
            selected_recipient: 0,
            source_hover: None,
            link_hover: source_highlight::HoverState::default(),
            workflow_snapshot: None,
            workflow_error: None,
            workflow_receiver: None,
            workflow_last_fetch: None,
            workflow_ui: workflow_board_ui::BoardUi::default(),
            analysis_request: 0,
            analysis_pending: false,
            analysis_due_at: None,
            analysis_receiver: None,
        }
    }
}

impl BandoEgui {
    // Run parsing outside the egui frame loop. Any changed source invalidates
    // the previous result and its request sequence, so stale replies are ignored.
    fn queue_source_save(&mut self) {
        self.source_save_error = None;
        if let Some(active_id) = self.active_id.as_deref() {
            let Some(active) = self.drafts.iter_mut().find(|item| item.id == active_id) else {
                self.source_save_error = Some("활성 발주서를 찾을 수 없습니다.".to_owned());
                return;
            };
            if active.raw != self.raw_source ||
               active.order_type != self.order_type.as_str() {
                active.analysis_raw = None;
                active.analysis_evidence = None;
                active.client_match = None;
                active.pricing_preview = None;
                active.manual_model = None;
                active.working_synced = false;
                // Source changes revoke only automatic dealer assignment.
                // A deliberate employee selection stays with this receipt.
                if active.client_selection.as_deref() == Some("AUTO") {
                    active.client_id = None;
                    active.client_selection = None;
                }
            }
            active.raw.clone_from(&self.raw_source);
            active.order_type = self.order_type.as_str().to_owned();
        }
        let source = WorkingSource {
            raw: self.raw_source.clone(),
            order_type: self.order_type.as_str().to_owned(),
            revision: 0, // The serialized autosave worker owns the latest revision.
            active_id: self.active_id.clone(),
            drafts: self.drafts.clone(),
        };
        if let Some(worker) = &self.source_autosave {
            if let Err(error) = worker.queue(source) {
                self.source_save_error = Some(error);
            }
        } else {
            self.source_save_error = Some("격리 원문 저장소가 초기화되지 않았습니다.".to_owned());
        }
    }

    // Every reception has its own source. Selecting an existing one never
    // replaces the text of a different order or merges their evidence.
    /// A strictly read-only lookup against an explicitly chosen Tauri v2
    /// database. Calculations come from Rust preview_workflow/read_shipments,
    /// never from UI arithmetic or fabricated placeholder workflow groups.
    fn poll_workflow_board(&mut self,ctx:&egui::Context){
        if let Some(receiver)=self.workflow_receiver.as_ref(){
            match receiver.try_recv(){
                Ok(Ok(snapshot))=>{
                    self.workflow_receiver=None;
                    if self.workflow_ui.selection.as_ref().is_some_and(|selected|
                        !snapshot.groups.iter().any(|group|
                            group.id==selected.group_id &&
                            group.shipments.iter().any(|ship|
                                ship.id==selected.shipment_id &&
                                ship.order_id==selected.order_id)))
                    {
                        self.workflow_ui.selection=None;
                        self.workflow_ui.folded=None;
                        self.workflow_ui.stop_preview();
                    }
                    self.workflow_snapshot=Some(snapshot);
                    self.workflow_error=None;
                    ctx.request_repaint();
                }
                Ok(Err(error))=>{
                    self.workflow_receiver=None;
                    self.workflow_error=Some(error);
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.workflow_receiver=None;
                    self.workflow_error=Some(
                        "발주묶음 Rust 조회 작업이 중단되었습니다.".to_owned()
                    );
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if self.page!=Page::Reception || self.workflow_receiver.is_some() ||
            !std::env::var_os("BANDO_EGUI_BUSINESS_DB")
                .is_some_and(|p|!p.is_empty()) {return;}
        if self.workflow_last_fetch.is_some_and(|at|
            at.elapsed()<Duration::from_secs(5)){return;}
        self.workflow_last_fetch=Some(Instant::now());
        let gateway=Arc::clone(&self.gateway);
        let (sender,receiver)=mpsc::channel();
        self.workflow_receiver=Some(receiver);
        let context=ctx.clone();
        std::thread::spawn(move||{
            let result=workflow_board::load(gateway.as_ref());
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    fn select_reception(&mut self, id: &str, ctx: &egui::Context) {
        let Some(item) = self.drafts.iter().find(|item| item.id == id).cloned() else {
            self.source_save_error = Some("선택한 발주서를 찾을 수 없습니다.".to_owned());
            return;
        };
        if self.active_id.as_deref() == Some(id) {
            return;
        }
        self.active_id = Some(item.id);
        self.raw_source = item.raw;
        self.order_type = OrderType::parse(&item.order_type).unwrap_or_default();
        // Blank receipts show clipboard/type choices; direct input is explicit.
        self.source_editing = !self.raw_source.is_empty();
        self.clear_source_analysis();
        if !self.raw_source.trim().is_empty() {
            self.analysis_due_at = Some(Instant::now() + Duration::from_millis(250));
            ctx.request_repaint_after(Duration::from_millis(260));
        }
        self.queue_source_save();
    }

    /// Bind only an OS-verified KakaoTalk chat to the ACTIVE unconfirmed
    /// reception. A changed/closed handle is not reused as evidence.
    fn bind_chat_to_active(&mut self, chat: ChatWindow, ctx: &egui::Context) {
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
            self.source_save_error = Some("Rust 후보 목록에 없는 거래처는 적용할 수 없습니다.".into());
            return;
        }
        self.apply_manual_client(client_id, ctx);
    }

    // All-client choices use one guarded native Rust order-edit path.
    // It computes a full repricing preview, but production state_save and
    // workflow reconciliation are not exposed in this isolated egui app.
    fn apply_manual_client(&mut self, client_id: &str, ctx: &egui::Context) {
        if self.client_change.is_some() {
            self.source_save_error = Some("이전 거래처 변경 계산이 끝난 뒤 다시 선택하세요.".into());
            return;
        }
        let Some(id) = self.active_id.as_deref() else { return; };
        let Some(row) = self.drafts.iter().find(|row| row.id == id).cloned() else { return; };
        if row.client_id.as_deref() == Some(client_id) &&
           row.pricing_preview.is_some() {
            return; // Same dealer with a valid quote: no unnecessary rewrite.
        }
        if row.analysis_raw.as_deref() != Some(row.raw.as_str()) ||
            row.analysis_evidence.is_none() {
            self.source_save_error = Some(
                "현재 발주원문 인식이 끝난 뒤 거래처를 다시 선택하세요.".into()
            );
            return;
        }
        let target=client_id.to_owned();
        let order_id=row.id.clone();
        let source_raw=row.raw.clone();
        let order_type=row.order_type.clone();
        let gateway=Arc::clone(&self.gateway);
        let (sender,receiver)=mpsc::channel();
        self.client_change=Some(PendingClientChange {
            order_id,source_raw,order_type,receiver,
        });
        self.source_save_error=None;
        std::thread::spawn(move || {
            let result=native_order_change::prepare(gateway.as_ref(),&row,&target);
            let _=sender.send(result);
        });
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn poll_client_change(&mut self, ctx:&egui::Context) {
        let value=self.client_change.as_ref().map(|p|p.receiver.try_recv());
        let result=match value {
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("Rust 주문 변경 계산이 중단되었습니다.".into())),
            _=>None,
        };
        if let Some(result)=result {
            let Some(pending)=self.client_change.take() else {return;};
            // Another order or edited original may never inherit an old
            // calculation, regardless of asynchronous completion order.
            if self.active_id.as_deref()!=Some(pending.order_id.as_str()) ||
               self.raw_source!=pending.source_raw ||
               self.order_type.as_str()!=pending.order_type {
                return;
            }
            match result {
                Ok(result)=>{
                    if let Some(row)=self.drafts.iter_mut().find(|r|
                        r.id==pending.order_id && r.raw==pending.source_raw
                    ) {
                        row.client_id=Some(result.client_id);
                        row.client_selection=Some("MANUAL".into());
                        row.pricing_preview=Some(result.pricing_preview);
                        row.manual_model=None;
                        row.working_synced=false;
                        row.analysis_raw=None;
                        row.analysis_evidence=None;
                        row.client_match=None;
                    }
                    self.clear_source_analysis();
                    if !self.raw_source.trim().is_empty() {
                        self.analysis_due_at=Some(Instant::now()+Duration::from_millis(250));
                        ctx.request_repaint_after(Duration::from_millis(260));
                    }
                    self.queue_source_save();
                }
                Err(error)=>{
                    self.source_save_error=Some(format!("거래처 변경 확인 실패: {error}"));
                }
            }
        } else if self.client_change.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// A deliberate employee click is the ONLY way to send an unconfirmed
    /// reception into the Tauri working-order store. Editing or selecting
    /// customers continues to save exclusively to egui-preview.sqlite3.
    fn start_working_sync(&mut self, ctx:&egui::Context) {
        self.working_sync_error=None;
        if !self.gateway.working_commit_enabled() {
            self.working_sync_error=Some(
                "운영 작업본 저장 도우미와 명시적 허용 설정이 필요합니다.".into()
            );
            return;
        }
        if self.working_sync.is_some() || self.client_change.is_some() {
            self.working_sync_error=Some("진행 중인 Rust 작업이 끝난 후 저장하세요.".into());
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
                    "거래처 변경 창에서 Rust 금액을 재계산한 뒤 저장하세요.".into()
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
                self.working_sync_error=Some("Rust 운영 데이터 기준 버전이 누락되었습니다.".into());
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
        std::thread::spawn(move || {
            let result=(||->Result<serde_json::Value,String>{
                if request.get("manualModel").is_some(){
                    // A draft without a quote receives a fresh read-only
                    // master revision inside this worker. The native helper
                    // compares it again within its IMMEDIATE transaction.
                    let state=gateway.load_business_state()?;
                    let revision=state.get("revision").and_then(
                        serde_json::Value::as_u64
                    ).ok_or("운영 DB revision을 확인하지 못했습니다.")?;
                    request["expectedAuthorityRevision"]=serde_json::json!(revision);
                }
                gateway.commit_working_reception(request)
            })();
            let _=sender.send(result);
        });
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn poll_working_sync(&mut self, ctx:&egui::Context) {
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
        } else if self.working_sync.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn open_manual_workspace(&mut self,ctx:&egui::Context) {
        if self.manual_workspace.is_some() {return;}
        if self.client_change.is_some() || self.working_sync.is_some() {
            self.source_save_error=Some("진행 중인 Rust 주문 작업이 끝난 뒤 수동 분류를 여세요.".into());
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
        self.manual_workspace=Some(manual_workspace::ManualWorkspace::new(
            row,Arc::clone(&self.gateway),ctx.clone()
        ));
    }

    /// The only coordinate owner for every reception modal. KakaoTalk is an
    /// externally owned Windows surface: keep modal bodies outside its area.
    fn modal_work_area(&self,ctx:&egui::Context)->Rect {
        let viewport=ctx.screen_rect();
        let gutter=theme::MODAL_VIEWPORT_GUTTER;
        let content_width=(viewport.width()-2.0*theme::PANEL_GAP)
            .max(layout::LayoutState::minimum_width());
        // React HubShell reserves Kakao across every navigation tab.
        let kakao_edge=viewport.left()+theme::PANEL_GAP+
            self.layout.widths(content_width)[0]+layout::SPLITTER;
        let left=(kakao_edge+gutter).min(viewport.right()-1.0);
        let right=(viewport.right()-gutter).max(left+1.0)
            .min(viewport.right());
        let top=(viewport.top()+theme::HEADER_HEIGHT+gutter)
            .min(viewport.bottom()-1.0);
        let bottom=(viewport.bottom()-gutter).max(top+1.0)
            .min(viewport.bottom());
        Rect::from_min_max(egui::pos2(left,top),egui::pos2(right,bottom))
    }

    fn show_manual_workspace(&mut self,ctx:&egui::Context,work_area:Rect) {
        if self.manual_workspace.as_ref().is_some_and(|workspace|
            self.active_id.as_deref()!=Some(workspace.order_id.as_str())
        ) {
            // A manual editor may never cross between two reception IDs.
            self.manual_workspace=None;
            return;
        }
        // Work-area coordinates are shared with every other modal.
        let result=self.manual_workspace.as_mut().and_then(|editor|
            editor.show(ctx,work_area)
        );
        match result {
            Some(manual_workspace::Event::Closed)=>{
                self.manual_workspace=None;
            }
            Some(manual_workspace::Event::Applied {
                order_id,raw,order_type,client_id,model,assignment_order,order
            })=>{
                self.manual_workspace=None;
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
                if let Some(recips)=order.get("recipients").filter(|v|v.is_array()) {
                    if let Some(row)=self.drafts.iter_mut()
                        .find(|row|row.id==order_id) {
                        row.manual_model=Some(serde_json::json!({
                            "sourceRaw":raw,"orderType":order_type,
                            "clientId":client_id,"model":model,
                            "assignmentOrder":assignment_order
                        }));
                        row.pricing_preview=None;
                        row.working_synced=false;
                    }
                    if let Some(analysis)=self.analysis.as_mut() {
                        analysis.recognition["recipients"]=recips.clone();
                        analysis.linked_spans=source_highlight::collect(
                            &self.raw_source,&analysis.recognition
                        );
                    }
                    self.queue_source_save();
                }else{
                    self.source_save_error=Some(
                        "Rust 수동 분류 결과에 배송 정보가 없어 적용하지 않았습니다.".into()
                    );
                }
            }
            None=>{}
        }
    }

    fn open_client_chooser(&mut self, ctx: &egui::Context) {
        let Some(order_id) = &self.active_id else { return; };
        self.client_chooser = Some(ClientChooser::new(
            order_id.clone(), Arc::clone(&self.gateway), ctx.clone()
        ));
    }

    fn show_client_chooser(&mut self, ctx: &egui::Context, work_area:Rect) {
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
                let verified = self.client_chooser.as_ref()
                    .is_some_and(|chooser| chooser.contains(&id));
                self.client_chooser = None;
                // Re-selecting the same dealer is harmless unless raw
                // changes require a fresh native price calculation.
                let refresh=self.active_id.as_deref()
                    .and_then(|active|self.drafts.iter().find(|row|row.id==active))
                    .is_some_and(|row|row.pricing_preview.is_none());
                if verified && (current_id.as_deref()!=Some(id.as_str()) || refresh) {
                    self.apply_manual_client(&id, ctx);
                }
            }
            Some(Choice::Closed) => { self.client_chooser = None; }
            None => {}
        }
    }

    fn new_reception(&mut self, ctx: &egui::Context) {
        // An already-open empty receipt is the new order. Do not multiply
        // empty records when the user clicks "+ 새 주문" repeatedly.
        if self.active_id.as_deref().and_then(|id|
            self.drafts.iter().find(|item| item.id == id)
        ).is_some_and(|item| item.raw.trim().is_empty()) {
            self.source_editing = false;
            return;
        }
        if self.drafts.len() >= 1000 {
            self.source_save_error = Some("미확정 접수 1,000건을 초과할 수 없습니다.".into());
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
            analysis_raw: None, analysis_evidence: None, client_match: None,
            chat_handle: None, chat_title: None,
            client_id: None, client_selection: None, pricing_preview: None,
            working_version: None, working_synced: false, manual_model: None,
        });
        self.select_reception(&id, ctx);
    }

    fn poll_source_saves(&mut self) {
        let mut events = Vec::new();
        if let Some(worker) = &self.source_autosave {
            while let Some(status) = worker.try_status() {
                events.push(status);
            }
        }
        for event in events {
            match event {
                source_autosave::SaveStatus::Saved(revision) => {
                    let _ = revision;
                    self.source_save_error = None;
                    // Saving another field must not silently dismiss an
                    // outstanding clipboard/OCR error notification.
                    if self.clipboard_error.is_none() {
                        self.notifications_open = false;
                    }
                }
                source_autosave::SaveStatus::Failed(error) => {
                    self.source_save_error = Some(error);
                }
            }
        }
    }

    fn start_source_analysis(&mut self, context: egui::Context) {
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
                    let model=row.manual_model.as_ref()
                        .and_then(|value|value.get("model"))
                        .ok_or("저장된 수동 분류 결과를 읽을 수 없습니다.")?;
                    let keys=row.manual_model.as_ref()
                        .and_then(|value|value.get("assignmentOrder"))
                        .cloned().unwrap_or_else(||serde_json::json!([]));
                    let order=manual_workspace::native_order(
                        gateway.as_ref(),&row,&analysis.recognition
                    )?;
                    let result=gateway.execute(
                        gateway::ReadOperation::ManualClassification,
                        serde_json::json!({
                            "action":"apply_manual","order":order,
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
                }
                Ok(analysis)
            })();
            let _ = sender.send((request, result));
            context.request_repaint();
        });
    }

    fn clear_source_analysis(&mut self) {
        self.analysis_request = self.analysis_request.wrapping_add(1);
        self.analysis_receiver = None;
        self.analysis_pending = false;
        self.analysis = None;
        self.analysis_error = None;
        self.selected_recipient = 0;
        self.source_hover=None;
        self.link_hover.clear();
        self.analysis_due_at = None;
    }

    fn poll_source_analysis(&mut self) {
        let complete = self.analysis_receiver.as_ref()
            .map(|receiver| receiver.try_recv());
        if let Some(Err(mpsc::TryRecvError::Disconnected)) = complete {
            self.analysis_receiver = None;
            self.analysis_pending = false;
            self.analysis_error = Some("Rust 인식 작업이 중단되었습니다.".to_owned());
            return;
        }
        if let Some(Ok((request, result))) = complete {
            self.analysis_receiver = None;
            if request != self.analysis_request {
                return;
            }
            self.analysis_pending = false;
            match result {
                Ok(analysis) => {
                    // Attach the exact Rust analysis to ONLY the still-active
                    // reception. A switched/new order invalidates this worker.
                    let mut changed_snapshot = false;
                    if let Some(active_id) = self.active_id.as_deref() {
                        if let Some(item) = self.drafts.iter_mut()
                            .find(|item| item.id == active_id) {
                            item.analysis_raw = Some(self.raw_source.clone());
                            item.analysis_evidence = Some(analysis.recognition.clone());
                            item.client_match = Some(analysis.client_match.clone());
                            if item.client_selection.as_deref() != Some("MANUAL") &&
                               item.manual_model.is_none() {
                                item.client_id = analysis.client_match.get("clientId")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_owned);
                                item.client_selection =
                                    item.client_id.as_ref().map(|_| "AUTO".to_owned());
                            }
                            changed_snapshot = true;
                        }
                    }
                    if changed_snapshot {
                        self.queue_source_save();
                    }
                    self.analysis = Some(analysis);
                    self.analysis_error = None;
                    self.selected_recipient = 0;
                }
                Err(error) => {
                    self.analysis = None;
                    self.analysis_error = Some(error);
                }
            }
        }
    }

    fn begin_clipboard_intake(&mut self, order_type: OrderType, ctx: egui::Context) {
        if self.clipboard_busy || !self.clipboard_monitor.kind().has_intake() {
            return;
        }
        self.clipboard_chat = if self.kakao_dock.connected() {
            // Existing contract: observe the actual KakaoTalk conversation
            // when an order is pasted, not when a user presses Connect.
            // A recently focused, OS-verified chat is the strongest source.
            // When there is exactly one chat, it is unambiguous even if
            // foreground focus has already moved to BANDO HUB.
            let focused = self.kakao_tracker.recent_focus()
                .and_then(|handle| self.kakao_tracker.verified(handle)).cloned();
            focused.or_else(|| {
                let chats=self.kakao_tracker.chats();
                if chats.len()==1 {chats.first().cloned()} else {None}
            })
        } else { None };
        self.clipboard_busy = true;
        self.ocr_processing =
            self.clipboard_monitor.kind() == clipboard_intake::ClipboardKind::Image;
        self.clipboard_error = None;
        self.clipboard_cancel.store(false, Ordering::Release);
        let cancelled = Arc::clone(&self.clipboard_cancel);
        let (tx, rx) = mpsc::channel();
        self.clipboard_receiver = Some(rx);
        // Clipboard image decoding and PaddleOCR never block the UI thread.
        std::thread::spawn(move || {
            let result = clipboard_intake::read().and_then(|value| match value {
                ClipboardPayload::Text(text) => Ok(ClipboardReceipt::Text(text)),
                ClipboardPayload::Image(image) => {
                    let text = clipboard_intake::recognize_image(&image, &cancelled)?;
                    Ok(ClipboardReceipt::Ocr { text, image })
                }
            });
            let _ = tx.send((order_type, result));
            ctx.request_repaint();
        });
    }

    fn accept_clipboard_text(&mut self, text: String, order_type: OrderType,
        chat: Option<ChatWindow>, ctx: &egui::Context) {
        if text.trim().is_empty() {
            self.clipboard_error = Some("발주 원문이 비어 있습니다.".into());
            return;
        }
        if self.raw_source != text && !self.raw_source.trim().is_empty() {
            // Exact replacement confirmation from the existing React intake.
            self.pending_replace = Some(PendingReplace { text, order_type, chat });
            return;
        }
        self.apply_intake_text(text, order_type, chat, ctx);
    }

    fn apply_intake_text(&mut self, text: String, order_type: OrderType,
        chat: Option<ChatWindow>, ctx: &egui::Context) {
        if text.trim().is_empty() { return; }
        if self.active_id.is_none() {
            self.new_reception(ctx);
        }
        let changed = self.raw_source != text;
        self.raw_source = text;
        self.order_type = order_type;
        self.source_editing = true;
        if let Some(chat) = chat {
            self.bind_chat_to_active(chat, ctx);
        }
        self.clipboard_error = None;
        if changed {
            self.clear_source_analysis();
            self.analysis_due_at = Some(Instant::now() + Duration::from_millis(250));
            ctx.request_repaint_after(Duration::from_millis(260));
        }
        // One revision-checked write includes both raw and AGENCY/DEALER.
        self.queue_source_save();
    }

    fn poll_clipboard_intake(&mut self, ctx: &egui::Context) {
        let next = self.clipboard_receiver.as_ref().map(|rx| rx.try_recv());
        match next {
            Some(Ok((order_type, result))) => {
                self.clipboard_receiver = None;
                self.clipboard_busy = false;
                self.ocr_processing = false;
                let intake_chat = self.clipboard_chat.take();
                if self.clipboard_cancel.swap(false, Ordering::AcqRel) {
                    // User cancellation wins even if a completed OCR result
                    // raced with the Cancel click. Never open the review.
                    return;
                }
                match result {
                    Ok(ClipboardReceipt::Text(text)) => {
                        self.accept_clipboard_text(text, order_type, intake_chat, ctx);
                    }
                    Ok(ClipboardReceipt::Ocr { text, image }) => {
                        if text.chars().count() > 20_000 {
                            self.clipboard_error = Some(
                                "발주 원문은 최대 20,000자까지 입력할 수 있습니다.".into()
                            );
                            return;
                        }
                        let texture = ctx.load_texture(
                            "clipboard-ocr-review-preview",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width, image.height], &image.rgba,
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        self.ocr_review = Some(OcrReview {
                            order_type,
                            original_text: text.clone(),
                            edited_text: text,
                            texture,
                            width: image.width,
                            height: image.height,
                            chat: intake_chat,
                        });
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

    /// The same OCR review controls that React exposes: inspect the image,
    /// correct recognized text, append or replace. No OCR result is committed
    /// to SQLite or sent to the Rust business engine before acceptance.
    fn show_ocr_review(&mut self, ctx: &egui::Context, work_area:Rect) {
        let mut close = false;
        let mut apply: Option<(String, OrderType, bool, Option<ChatWindow>)> = None;
        if let Some(review) = self.ocr_review.as_mut() {
            let mut open = true;
            components::modal_window("OCR 문자 인식 검토","egui-ocr-review",work_area)
                .open(&mut open)
                .resizable(true)
                .default_width(620.0)
                .show(ctx, |ui| {
                    ui.label(typography::caption(
                        "접수한 이미지와 인식 문구를 비교하고 오타를 고쳐 주세요."
                    ));
                    ui.label(typography::caption(
                        "확인 전에는 발주 원문이나 주문 데이터베이스가 변경되지 않습니다."
                    ));
                    let width = review.width as f32;
                    let height = review.height as f32;
                    let scale = (ui.available_width().min(560.0) / width)
                        .min(240.0 / height).min(1.0);
                    ui.image((
                        review.texture.id(),
                        Vec2::new(width * scale, height * scale),
                    ));
                    ui.label(typography::field_label("인식된 문구"));
                    ui.add(
                        egui::TextEdit::multiline(&mut review.edited_text)
                            .font(egui::TextStyle::Body)
                            .desired_width(ui.available_width())
                            .desired_rows(9),
                    );
                    ui.horizontal(|ui| {
                        if theme::action(ui, "닫기", ButtonRole::Standard, true).clicked() {
                            close = true;
                        }
                        let can_apply = !review.edited_text.trim().is_empty()
                            && review.edited_text.chars().count() <= 20_000;
                        if theme::action(ui, "원문에 추가", ButtonRole::Standard, can_apply).clicked() {
                            let appended = if self.raw_source.trim().is_empty() {
                                review.edited_text.trim().to_owned()
                            } else {
                                format!("{}\n{}", self.raw_source.trim_end(), review.edited_text.trim())
                            };
                            apply = Some((appended, review.order_type, false, review.chat.clone()));
                        }
                        if theme::action(ui, "원문 교체", ButtonRole::Primary, can_apply).clicked() {
                            apply = Some((
                                review.edited_text.trim().to_owned(),
                                review.order_type, true, review.chat.clone()
                            ));
                        }
                    });
                });
            if !open { close = true; }
        }
        if let Some((raw, kind, replacing, chat)) = apply {
            self.ocr_review = None;
            self.discard_ocr_confirm = false;
            if replacing {
                self.accept_clipboard_text(raw, kind, chat, ctx);
            } else {
                // Append preserves the original React behavior: no redundant
                // replacement confirmation because the old source remains.
                self.apply_intake_text(raw, kind, chat, ctx);
            }
        } else if close {
            if self.ocr_review.as_ref().is_some_and(|review|
                review.edited_text != review.original_text
            ) {
                self.discard_ocr_confirm = true;
            } else {
                self.ocr_review = None;
                self.discard_ocr_confirm = false;
            }
        }

        if self.discard_ocr_confirm {
            components::modal_backdrop(
                ctx,"ocr-discard-nested-backdrop",true
            );
            components::decision_window(
                "변경사항 확인","ocr-review-discard-confirm",work_area
            )
                .show(ctx, |ui| {
                    ui.label("OCR 문자 인식 검토의 수정 내용을 버릴까요?");
                    ui.horizontal(|ui| {
                        if theme::action(ui, "계속 편집", ButtonRole::Standard, true).clicked() {
                            self.discard_ocr_confirm = false;
                        }
                        if theme::action(ui, "변경 취소", ButtonRole::Danger, true).clicked() {
                            self.ocr_review = None;
                            self.discard_ocr_confirm = false;
                        }
                    });
                });
        }
    }

    fn show_replacement_confirmation(&mut self, ctx: &egui::Context, work_area:Rect) {
        if self.pending_replace.is_none() { return; }
        let mut accept = false;
        let mut cancel = false;
        components::decision_window(
            "발주원문 교체 확인","egui-replace-source-confirm",work_area
        )
            .show(ctx, |ui| {
                ui.label("현재 초안과 분석 결과를 새 원문으로 교체할까요?");
                ui.horizontal(|ui| {
                    if theme::action(ui, "취소", ButtonRole::Standard, true).clicked() {
                        cancel = true;
                    }
                    if theme::action(ui, "교체", ButtonRole::Primary, true).clicked() {
                        accept = true;
                    }
                });
            });
        if accept {
            if let Some(next) = self.pending_replace.take() {
                self.apply_intake_text(next.text, next.order_type, next.chat, ctx);
            }
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

    fn top_bar(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        egui::TopBottomPanel::top("global_navigation")
            .exact_height(theme::HEADER_HEIGHT)
            .frame(
                Frame::new().fill(theme::HEADER_BG)
                    .inner_margin(egui::Margin::symmetric(theme::HEADER_PADDING_X, 0))
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
                        bar.spacing_mut().item_spacing.x = theme::SPACE_8;
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
                            if theme::action(bar, page.label(), role, true).clicked() {
                                // A client editor is modal: navigation cannot
                                // dispose of an unsaved client draft.
                                if !self.client_manager.as_ref()
                                    .is_some_and(|manager|manager.has_modal()) {
                                    // React HubShell keeps the Kakao dock
                                    // outside the per-tab content. Navigation
                                    // must NOT disconnect an OS window.
                                    self.page = page;
                                }
                            }
                        }
                        bar.with_layout(Layout::right_to_left(Align::Center), |actions| {
                            actions.spacing_mut().item_spacing.x = theme::SPACE_4;
                            if theme::window_control(actions, WindowControl::Close)
                                .on_hover_text("닫기").clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if theme::window_control(actions, WindowControl::Maximize)
                                .on_hover_text("창 최대화 또는 복원").clicked()
                            {
                                ctx.send_viewport_cmd(
                                    egui::ViewportCommand::Maximized(!maximized)
                                );
                            }
                            if theme::window_control(actions, WindowControl::Minimize)
                                .on_hover_text("최소화").clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                            actions.add_space(theme::SPACE_8);
                            if theme::action(
                                actions, "알림", ButtonRole::Standard,
                                self.source_save_error.is_some() || self.clipboard_error.is_some(),
                            ).clicked() {
                                self.notifications_open = !self.notifications_open;
                            }
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

        if self.notifications_open &&
            (self.source_save_error.is_some() || self.clipboard_error.is_some())
        {
            egui::Window::new("알림")
                .id(egui::Id::new("native_ui_preview_notifications"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if let Some(error) = &self.source_save_error {
                        ui.label(typography::status(error, theme::DANGER));
                    }
                    if let Some(error) = &self.clipboard_error {
                        ui.label(typography::status(error, theme::DANGER));
                    }
                });
        }
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
        ui: &mut Ui,
        title: &str,
        minimum_height: f32,
        header_right: impl FnOnce(&mut Ui),
        content: impl FnOnce(&mut Ui),
    ) {
        let width = ui.available_width();
        theme::docked_pane_frame().show(ui, |ui| {
            let frame_inset = 2.0 * theme::SECTION_PADDING;
            ui.set_min_width((width - frame_inset).max(60.0));
            ui.set_min_height((minimum_height - frame_inset).max(48.0));
            // The section header owns its vertical rhythm. Eguis implicit
            // item spacing must not be added on top of explicit gaps.
            let body_item_spacing = theme::SPACING;
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), theme::SECTION_HEADER_HEIGHT),
                Layout::left_to_right(Align::Center),
                |header| {
                    header.set_min_height(theme::SECTION_HEADER_HEIGHT);
                    header.label(typography::section_heading(title));
                    header.with_layout(Layout::right_to_left(Align::Center), header_right);
                },
            );
            ui.add_space(theme::SECTION_CONTENT_GAP);
            components::soft_divider(ui);
            ui.add_space(theme::SPACE_4);
            ui.scope(|body| {
                body.spacing_mut().item_spacing.y = body_item_spacing;
                content(body);
            });
        });
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
            // Match .kakao-panel-toolbar (34px) and its 1px bottom border.
            // The same target is used on the Connect click and every later
            // sync, so an external HWND never shifts between those frames.
            region=Rect::from_min_max(
                egui::pos2(
                    slot.left(),
                    (slot.top()+theme::KAKAO_TOOLBAR_HEIGHT
                        +theme::SOFT_DIVIDER_WIDTH).min(slot.bottom())
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
                        toolbar.spacing_mut().item_spacing.x=theme::SPACE_4;
                        toolbar.add_space(theme::KAKAO_TOOLBAR_PAD_X);
                        // Explicit KakaoTalk menu prefix: icon and all three
                        // controls form ONE compact left-aligned row.
                        components::kakao_menu_icon(toolbar);
                        let mode_role=|active|if active{
                            ButtonRole::ActiveKakaoControl
                        }else{ButtonRole::KakaoControl};
                        if theme::action(toolbar,"겹쳐 보기",
                            mode_role(current_mode==kakao_dock::DockMode::Together),true
                        ).clicked(){
                            next_mode=Some(kakao_dock::DockMode::Together);
                        }
                        if theme::action(toolbar,"나누어 보기",
                            mode_role(current_mode==kakao_dock::DockMode::Split),true
                        ).clicked(){
                            next_mode=Some(kakao_dock::DockMode::Split);
                        }
                        if theme::action(toolbar,"연결 해제",
                            ButtonRole::KakaoControl,true
                        ).clicked(){
                            disconnect=true;
                        }
                    },
                );
                let line_y=slot.top()+theme::KAKAO_TOOLBAR_HEIGHT;
                pane.painter().line_segment(
                    [egui::pos2(slot.left(),line_y),
                        egui::pos2(slot.right(),line_y)],
                    egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
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
                        if theme::action(center,"카카오톡 연결",
                            ButtonRole::Emphasized,true
                        ).clicked(){
                            connect=true;
                        }
                        if let Some(reason)=&self.kakao_dock_error {
                            center.label(reason);
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

    fn reception(&mut self, ctx: &egui::Context) {
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
                            ui.scope_builder(
                                UiBuilder::new()
                                    .id_salt(("reception-pane", index))
                                    .max_rect(rect)
                                    .layout(Layout::top_down(Align::Min)),
                                |pane| {
                                    pane.set_clip_rect(rect);
                                    pane.set_width(rect.width());
                                    pane.set_height(rect.height());
                                    match index {
                                        0 => self.kakao_pane(pane),
                                        1 => self.source_and_message(pane),
                                        2 => self.details_and_freight(pane),
                                        _ => self.orders_pane(pane),
                                    }
                                },
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
                                components::paint_dock_splitter(
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
                                                response.drag_delta().x,
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
                                    self.layout.reset();
                                    let _ = self.layout.save();
                                }
                                x += layout::SPLITTER;
                            }
                        }

                        // Draw the visible dock rules once in the common
                        // coordinate system. A panel-clipped horizontal line
                        // otherwise stops at the edge of a 6pt vertical
                        // gutter and cannot reach its centered stroke.
                        // The overlays only paint; interaction rectangles,
                        // widths and drag behavior remain unchanged.
                        if splitter_visuals.len() == layout::PANEL_COUNT - 1 {
                            let shared_upper = self.layout.vertical_heights(all.height()).0;
                            let (review_upper, _) =
                                self.layout.detail_upper_heights(all.height());
                            let shared_y =
                                all.top() + shared_upper + layout::SPLITTER / 2.0;
                            let review_y =
                                all.top() + review_upper + layout::SPLITTER / 2.0;
                            for (left, right, center_y) in [
                                (0, 1, shared_y),
                                (1, 2, review_y),
                                (1, 2, shared_y),
                            ] {
                                let line_area = Rect::from_min_max(
                                    egui::pos2(
                                        splitter_visuals[left].0.center().x,
                                        center_y - layout::SPLITTER / 2.0,
                                    ),
                                    egui::pos2(
                                        splitter_visuals[right].0.center().x,
                                        center_y + layout::SPLITTER / 2.0,
                                    ),
                                );
                                components::paint_dock_axis_line(
                                    ui, line_area, false, false,
                                );
                            }
                            // Keep all vertical axes visually continuous at
                            // T-junctions by drawing them over the crossbars.
                            for (splitter, highlighted) in splitter_visuals {
                                components::paint_dock_axis_line(
                                    ui, splitter, true, highlighted,
                                );
                            }
                        }
                    };
                if content_width > viewport_width + 0.5 {
                    // Small windows must still be able to reach every pane.
                    egui::ScrollArea::horizontal()
                        .drag_to_scroll(false)
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
                child.set_width(source_rect.width());
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
        components::paint_dock_splitter(
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
                    response.drag_delta().y,
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
                child.set_width(message_rect.width());
                child.set_height(message_rect.height());
                self.message_section(child, message_height);
            },
        );
    }

    /// Selected/hovered workflow content is a transient read-only view.
    /// Never swap it into the editable egui working_source, autosave worker,
    /// or the active employee-selected draft.
    fn visible_workflow_group(&self)->Option<(
        workflow_board::GroupRow,workflow_board::ShipmentRow
    )>{
        let group_id=self.workflow_ui.active_preview()
            .or_else(||self.workflow_ui.selection.as_ref()
                .map(|selection|selection.group_id.as_str()))?;
        let board=self.workflow_snapshot.as_ref()?;
        let group=board.groups.iter().find(|g|g.id==group_id)?;
        let selected=self.workflow_ui.selection.as_ref()
            .filter(|s|s.group_id==group.id)
            .and_then(|s|group.shipments.iter().find(|ship|
                ship.id==s.shipment_id&&ship.order_id==s.order_id));
        let ship=selected.or_else(||group.shipments.first())?;
        Some((group.clone(),ship.clone()))
    }

    fn source_section(&mut self, ui: &mut Ui, height: f32) {
        if let Some((_group,shipment))=self.visible_workflow_group(){
            let mut new_clicked=false;
            Self::section_with_header(ui,"발주 원문",height,|header|{
                if theme::action(header,"+ 새 주문",ButtonRole::Emphasized,true).clicked(){
                    new_clicked=true;
                }
            },|body|{
                egui::ScrollArea::vertical()
                    .id_salt("workflow-source-read-only")
                    .max_height(components::docked_section_body_height(height))
                    .auto_shrink([false,false])
                    .show(body,|content|{
                        content.add(egui::Label::new(
                            typography::body(&shipment.source_raw)
                        ).selectable(true));
                    });
            });
            if new_clicked{
                self.workflow_ui.selection=None;
                self.workflow_ui.folded=None;
                self.workflow_ui.stop_preview();
                self.new_reception(ui.ctx());
            }
            return;
        }
        let mut begin_input = false;
        let mut begin_new_order = false;
        let mut start_clipboard = None;
        let mut open_client_chooser = false;
        let mut open_manual_workspace = false;
        let can_receive_clipboard =
            self.clipboard_monitor.kind().has_intake() && !self.clipboard_busy;
        let ocr_processing = self.ocr_processing;
        let cancel_ocr = Arc::clone(&self.clipboard_cancel);
        Self::section_with_header(ui, "발주 원문", height, |ui| {
            if theme::action(ui, "+ 새 주문", ButtonRole::Emphasized, true).clicked() {
                begin_new_order = true;
            }
        }, |ui| {
            if self.source_editing || !self.raw_source.is_empty() {
                // The approved React source pane reserves an 82pt dealer
                // band below the title. Its geometry must be allocated BEFORE
                // the editor, so dealer information cannot overlap the raw
                // document or push the fixed source/message splitter.
                let available_body = components::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                let client_band_height = if !self.raw_source.trim().is_empty() {
                    available_body.min(82.0)
                } else { 0.0 };
                if client_band_height > 0.0 {
                    let (band_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), client_band_height),
                        Sense::hover(),
                    );
                    // The header-to-band gap and the band's following item gap
                    // are asymmetric. Shift the card within its reserved
                    // band so the visible gaps above and below match.
                    let card_rect = band_rect
                        .shrink2(egui::vec2(0.0, theme::PANEL_GAP))
                        .translate(egui::vec2(0.0, -theme::SPACE_4));
                    ui.painter().rect(
                        card_rect,
                        theme::CONTROL_RADIUS as f32,
                        theme::INPUT,
                        egui::Stroke::new(1.0, theme::BORDER),
                        egui::StrokeKind::Inside,
                    );
                    let client_name = self.active_id.as_deref()
                        .and_then(|id| self.drafts.iter().find(|item| item.id == id))
                        .and_then(|item| item.client_id.as_deref())
                        .and_then(|id| self.analysis.as_ref().and_then(|analysis| {
                            if reception_service::text(&analysis.client_match, "clientId") == id {
                                analysis.matched_client_name.as_deref()
                            } else {
                                analysis.chosen_client_name.as_deref()
                            }
                        }))
                        .map(str::to_owned);
                    let status = client_name.as_deref()
                        .unwrap_or("거래처 확인 필요");
                    ui.scope_builder(
                        UiBuilder::new()
                            .id_salt("source-current-client-band")
                            .max_rect(card_rect.shrink2(egui::vec2(10.0, 5.0)))
                            .layout(Layout::top_down(Align::Min)),
                        |card| {
                            card.set_clip_rect(card_rect);
                            card.label(typography::caption("현재 거래처"));
                            card.horizontal(|line| {
                                line.label(typography::body(status));
                                line.with_layout(Layout::right_to_left(Align::Center), |right| {
                                    let label = if client_name.is_some() {
                                        "변경"
                                    } else { "거래처 찾기" };
                                    if theme::action(right, label, ButtonRole::Standard, true)
                                        .clicked() {
                                        open_client_chooser = true;
                                    }
                                });
                            });
                        },
                    );
                }
                // The React source actions belong at the BOTTOM of the source
                // pane, not inside the adjacent "확인할 내용" review scroll.
                // Reserve their height before allocating the editor so the
                // fixed source/message splitter cannot be displaced.
                let has_source = !self.raw_source.trim().is_empty();
                let source_actions_height = if has_source {
                    theme::ACTION_STANDARD_HEIGHT + theme::SPACE_8 + theme::SPACING
                } else { 0.0 };
                let editor_height = (available_body - client_band_height
                    - source_actions_height)
                    .min(ui.available_height().max(0.0)).max(0.0);
                if editor_height > 0.0 {
                    let viewport = Rect::from_min_size(
                        ui.available_rect_before_wrap().min,
                        Vec2::new(ui.available_width(), editor_height),
                    );
                    let clip = ui.clip_rect().intersect(viewport);
                    // The native recognition worker owns source/recipient
                    // associations. Never infer a position by text search.
                    let spans=self.analysis.as_ref()
                        .filter(|_|self.active_id.as_deref().and_then(|id|
                            self.drafts.iter().find(|row|row.id==id)
                        ).is_some_and(|row|
                            row.analysis_raw.as_deref()==Some(self.raw_source.as_str())
                        ))
                        .map(|analysis|analysis.linked_spans.clone())
                        .unwrap_or_default();
                    let selected_id=self.analysis.as_ref()
                        .and_then(|analysis|reception_service::recipients(analysis)
                            .get(self.selected_recipient))
                        .and_then(|recipient|recipient.get("id"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    let active_link=self.link_hover.active.clone();
                    let scroll_link=self.link_hover.scroll_to.clone();
                    let mut hovered_link=None;
                    let mut clicked_link=None;
                    let mut did_scroll=false;
                    let editor = ui.scope(|editor_ui| {
                        editor_ui.set_clip_rect(clip);
                        egui::ScrollArea::vertical()
                            .id_salt("source-editor-vertical-scroll")
                            .max_height(editor_height)
                            .auto_shrink([false, false])
                            .show(editor_ui, |scroll_ui| {
                                // Keep the source/message splitter fixed. Only
                                // this scroll area grows its internal content.
                                let lines=self.raw_source.lines().count().max(8).min(1000);
                                let mut layouter=|lay_ui:&egui::Ui,
                                    buffer:&dyn egui::TextBuffer, width:f32| {
                                    let job=source_highlight::layout(
                                        buffer.as_str(),&spans,
                                        selected_id.as_deref(),active_link.as_ref(),
                                        width,egui::FontId::proportional(typography::FONT_BODY),
                                        theme::TEXT,
                                    );
                                    lay_ui.fonts_mut(|fonts|fonts.layout_job(job))
                                };
                                let output=egui::TextEdit::multiline(&mut self.raw_source)
                                    .font(egui::TextStyle::Body)
                                    .desired_rows(lines)
                                    .desired_width(scroll_ui.available_width())
                                    .layouter(&mut layouter)
                                    .show(scroll_ui);
                                if output.response.hovered() {
                                    if let Some(pointer)=scroll_ui.input(|i|i.pointer.hover_pos()) {
                                        let cursor=output.galley.cursor_from_pos(
                                            pointer-output.galley_pos
                                        ).index;
                                        hovered_link=spans.iter().find(|span|
                                            span.start_char<=cursor && cursor<span.end_char
                                        ).map(|span|span.key.clone());
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
                            }).inner
                    }).inner;
                    self.source_hover=hovered_link;
                    if did_scroll {self.link_hover.scroll_to=None;}
                    if let Some(clicked)=clicked_link {
                        if let Some(analysis)=&self.analysis {
                            if let Some(index)=reception_service::recipients(analysis)
                                .iter().position(|row|
                                    reception_service::text(row,"id")==clicked.recipient_id
                                ) {
                                self.selected_recipient=index;
                            }
                        }
                    }
                    if editor.changed() {
                        self.clear_source_analysis();
                        self.queue_source_save();
                        if !self.raw_source.trim().is_empty() {
                            self.analysis_due_at = Some(
                                Instant::now() + Duration::from_millis(250)
                            );
                            ui.ctx().request_repaint_after(Duration::from_millis(260));
                        }
                    }
                }
                if has_source {
                    ui.add_space(theme::SPACE_8);
                    let ready = self.active_id.as_deref()
                        .and_then(|id| self.drafts.iter().find(|row| row.id == id))
                        .is_some_and(|row|
                            row.analysis_raw.as_deref()==Some(self.raw_source.as_str()) &&
                            row.analysis_evidence.is_some()
                        );
                    ui.with_layout(Layout::right_to_left(Align::Center), |actions| {
                        if theme::action(actions,"수동 분류",ButtonRole::Standard,
                            ready && self.manual_workspace.is_none() &&
                            self.client_change.is_none() && self.working_sync.is_none()
                        ).clicked() {
                            open_manual_workspace=true;
                        }
                    });
                }
            } else {
                self.source_hover=None;
                // Narrow panes may need a stacked button row. Scroll ONLY the
                // intake instructions, never the source/message divider.
                let intake_height = components::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                egui::ScrollArea::vertical()
                    .id_salt("source-intake-instructions")
                    .max_height(intake_height)
                    .auto_shrink([false, false])
                    .show(ui, |intake| {
                        intake.add_space(theme::SPACE_24);
                        intake.label(typography::subsection_heading("클립보드 접수"));
                        intake.label(typography::caption(
                            "복사한 텍스트와 이미지를 자동으로 구분해 인식합니다."
                        ));
                        let (agency, dealer) = components::paired_actions(
                            intake,
                            ("대리발송", ButtonRole::Emphasized, can_receive_clipboard),
                            ("대리점 납품", ButtonRole::Emphasized, can_receive_clipboard),
                        );
                        if agency.clicked() {
                            start_clipboard = Some(OrderType::Agency);
                        } else if dealer.clicked() {
                            start_clipboard = Some(OrderType::Dealer);
                        }
                        if ocr_processing {
                            intake.horizontal(|ui| {
                                ui.label(typography::caption("OCR 처리 중…"));
                                if theme::action(
                                    ui, "취소", ButtonRole::Standard, true
                                ).clicked() {
                                    cancel_ocr.store(true, Ordering::Release);
                                }
                            });
                        }
                        intake.label(typography::caption(
                            "이미지 캡처는 Ctrl + Shift + S 또는 Print Screen으로 복사하세요."
                        ));
                        components::soft_divider(intake);
                        intake.add_space(theme::SPACE_8);
                        intake.label(typography::subsection_heading("다른 방법으로 접수"));
                        let (direct, _) = components::paired_actions(
                            intake,
                            ("직접 입력", ButtonRole::Emphasized, true),
                            ("캡처 범위 지정", ButtonRole::Emphasized, false),
                        );
                        if direct.clicked() {
                            begin_input = true;
                        }
                        components::soft_divider(intake);
                        intake.add_space(theme::SPACE_8);
                        intake.label(typography::subsection_heading("폴더 자동 인식"));
                        intake.label(typography::caption(
                            "설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요."
                        ));
                    });
            }
        });
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
            if self.active_id.is_none() {
                self.new_reception(ui.ctx());
            }
            self.source_editing = true;
        }
        if let Some(kind) = start_clipboard {
            self.begin_clipboard_intake(kind, ui.ctx().clone());
        }
    }

    fn message_section(&mut self, ui: &mut Ui, height: f32) {
        if let Some((group,_shipment))=self.visible_workflow_group(){
            Self::section_with_header(ui,"복사용 자동 메시지",height,|header|{
                let mut include_link=false;
                header.add_enabled(false,egui::Checkbox::new(&mut include_link,"링크 포함"));
            },|body|{
                let available=components::docked_section_body_height(height)
                    .min(body.available_height().max(0.0));
                let message_height=(available-theme::ACTION_STANDARD_HEIGHT
                    -theme::SPACE_8).max(0.0);
                egui::ScrollArea::vertical()
                    .id_salt("native-workflow-message")
                    .max_height(message_height)
                    .auto_shrink([false,false])
                    .show(body,|content|{
                        if let Some(message)=group.automatic_text.as_deref(){
                            content.add(egui::Label::new(typography::body(message))
                                .selectable(true));
                        }
                        if let Some(error)=group.automatic_error.as_deref(){
                            content.label(typography::status(error,theme::DANGER));
                        }
                    });
                body.add_space(theme::SPACE_8);
                components::paired_actions(body,
                    ("그대로 복사",ButtonRole::Standard,false),
                    ("인사말 포함 복사",ButtonRole::Standard,false)
                );
            });
            return;
        }
        Self::section_with_header(ui, "복사용 자동 메시지", height, |ui| {
            let mut include_link = false;
            ui.add_enabled(false, egui::Checkbox::new(&mut include_link, "링크 포함"));
        }, |ui| {
            // Empty egui Frames shrink to their contents. Allocate the real
            // message rectangle explicitly so it fills the available width
            // instead of appearing as a narrow vertical stripe.
            let button_row_height = if ui.available_width() < 280.0 {
                theme::ACTION_STANDARD_HEIGHT * 2.0 + theme::SPACING
            } else {
                theme::ACTION_STANDARD_HEIGHT
            };
            let bottom_controls = button_row_height + theme::SPACE_8;
            let available_body = components::docked_section_body_height(height)
                .min(ui.available_height().max(0.0));
            let field_height = (available_body - bottom_controls).max(0.0);
            let (field_rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), field_height),
                Sense::hover(),
            );
            ui.painter().rect_filled(
                field_rect,
                theme::CONTROL_RADIUS as f32,
                theme::INPUT,
            );
            ui.add_space(theme::SPACE_8);
            components::paired_actions(
                ui,
                ("그대로 복사", ButtonRole::Standard, false),
                ("인사말 포함 복사", ButtonRole::Standard, false),
            );
        });
    }

    fn details_and_freight(&mut self, ui: &mut Ui) {
        let full_height = ui.available_height();
        // The center-line between the second and third columns is shared.
        // Independent 27% + 22% measurements used to put the lower third
        // column divider 1–2 physical pixels above the source/message divider.
        let (review_height, recipient_height) =
            self.layout.detail_upper_heights(full_height);
        // Fix the third column to the SAME upper/lower boundary as column
        // two. Section content must never be allowed to move the divider.
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
        let bottom_rect = Rect::from_min_size(
            egui::pos2(bounds.left(), second_sep.bottom()),
            Vec2::new(
                full_width, (bounds.bottom() - second_sep.bottom()).max(0.0)
            ),
        );

        if let Some((group,ship))=self.visible_workflow_group(){
            // The same preallocated review/recipient/items rectangles are
            // used for real workflow rows. No source replacement, false
            // editing actions or section boundary changes on hover.
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-review")
                    .max_rect(review_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(review_rect);
                    pane.set_width(review_rect.width());
                    pane.set_height(review_rect.height());
                    Self::section(pane,"확인할 내용",review_height,|ui|{
                        egui::ScrollArea::vertical()
                            .max_height(components::docked_section_body_height(review_height))
                            .id_salt("native-workflow-issues")
                            .show(ui,|issues|{
                                for issue in &group.issues{
                                    issues.label(typography::status(issue,theme::SELECTED_ORDER));
                                }
                            });
                    });
                }
            );
            components::paint_dock_splitter(ui,first_sep,false,false);
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-recipient")
                    .max_rect(recipient_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(recipient_rect);
                    pane.set_width(recipient_rect.width());
                    pane.set_height(recipient_rect.height());
                    Self::section(pane,"수취인별 주문 정보",recipient_height,|ui|{
                        egui::ScrollArea::vertical()
                            .max_height(components::docked_section_body_height(recipient_height))
                            .id_salt("native-workflow-recipient-readonly")
                            .show(ui,|contents|{
                                for (label,field) in [
                                    ("이름","name"),("주소","address"),
                                    ("휴대전화","mobile"),("전화","phone"),
                                    ("요청 메시지","note")
                                ]{
                                    let value=reception_service::text(&ship.recipient,field);
                                    if value.is_empty() && field!="name"{continue;}
                                    contents.horizontal(|line|{
                                        line.label(typography::field_label(label));
                                        line.label(typography::body(value));
                                    });
                                }
                            });
                    });
                }
            );
            components::paint_dock_splitter(ui,second_sep,false,false);
            ui.scope_builder(
                UiBuilder::new().id_salt("native-workflow-items")
                    .max_rect(bottom_rect).layout(Layout::top_down(Align::Min)),
                |pane|{
                    pane.set_clip_rect(bottom_rect);
                    pane.set_width(bottom_rect.width());
                    pane.set_height(bottom_rect.height());
                    Self::section(pane,"품목 및 운임",bottom_rect.height(),|ui|{
                        let content_height=(components::docked_section_body_height(
                            bottom_rect.height())-theme::SUMMARY_STRIP_HEIGHT).max(0.0);
                        egui::ScrollArea::vertical()
                            .id_salt("native-workflow-products")
                            .max_height(content_height)
                            .show(ui,|contents|{
                                for product in &ship.products{
                                    contents.label(typography::body(product));
                                    components::soft_divider(contents);
                                }
                                contents.horizontal(|contents|{
                                    contents.label(typography::field_label("운임"));
                                    let display=if ship.collect{"착불".to_owned()}
                                        else{ship.freight.map(components::display_won)
                                            .unwrap_or_else(||"—".into())};
                                    contents.label(typography::quantity(&display));
                                });
                            });
                        let amount=group.total.map(components::display_won)
                            .unwrap_or_else(||"—".into());
                        components::summary_strip(ui,"총 주문",&amount,"");
                    });
                }
            );
            return;
        }

        let mut client_choice: Option<String> = None;
        let mut hovered_detail: Option<source_highlight::LinkKey>=None;
        let mut sync_clicked=false;
        let mut refresh_price:Option<String>=None;
        ui.scope_builder(
            UiBuilder::new().id_salt("details-review").max_rect(review_rect)
                .layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(review_rect);
                pane.set_width(review_rect.width());
                pane.set_height(review_rect.height());
                Self::section(pane, "확인할 내용", review_height, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("reception-review-content-scroll")
                        .max_height(components::docked_section_body_height(review_height))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                    if let Some(message)=&self.working_sync_error {
                        ui.label(typography::status(message,theme::DANGER));
                    }
                    if let Some(row)=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|row|row.id==id))
                    {
                        if row.working_version.is_some() {
                            ui.label(typography::caption(if row.working_synced {
                                "운영 미확정 작업본 저장됨 · 주문 확인 완료 전"
                            } else {
                                "운영 작업본 저장 후 변경사항 있음 · 다시 저장 필요"
                            }));
                        }
                        if row.manual_model.is_some() && !row.working_synced {
                            ui.label(typography::caption(
                                "수동 분류 반영 대기 · 운영 미확정 작업본 저장 필요"
                            ));
                        }
                        if self.gateway.working_commit_enabled() {
                            if self.working_sync.is_some() {
                                ui.label(typography::caption("운영 작업본 저장 중"));
                            } else if row.client_id.is_some() && !row.raw.trim().is_empty() {
                                if row.manual_model.is_some() ||
                                   row.pricing_preview.is_some() {
                                    if theme::action(
                                        ui,"운영 작업본 저장",ButtonRole::Standard,
                                        !row.working_synced
                                    ).clicked() {
                                        sync_clicked=true;
                                    }
                                } else if let Some(client_id)=&row.client_id {
                                    if theme::action(
                                        ui,"Rust 금액 다시 계산",ButtonRole::Quiet,true
                                    ).clicked() {
                                        refresh_price=Some(client_id.clone());
                                    }
                                }
                            }
                        }
                    }
                    if let Some(error) = &self.analysis_error {
                        ui.label(typography::status(error, theme::DANGER));
                    } else if let Some(analysis) = &self.analysis {
                        let manually_selected = self.active_id.as_deref()
                            .and_then(|id| self.drafts.iter().find(|row| row.id == id))
                            .is_some_and(|row| row.client_selection.as_deref() == Some("MANUAL"));
                        if analysis.directory_count == 0 {
                            ui.label(typography::status(
                                "거래처 명단 미연결 · 자동 매칭 확인 필요",
                                theme::SELECTED_ORDER,
                            ));
                        } else if manually_selected {
                            if let Some(name) = &analysis.chosen_client_name {
                                ui.label(typography::body(
                                    &format!("거래처 직접 선택 · {name}")
                                ));
                            } else {
                                ui.label(typography::status(
                                    "선택한 거래처를 현재 명단에서 찾지 못했습니다.",
                                    theme::DANGER,
                                ));
                            }
                        } else if let Some(name) = &analysis.matched_client_name {
                            ui.label(typography::body(
                                &format!("거래처 자동 인식 · {name}")
                            ));
                        } else {
                            ui.label(typography::status(
                                "거래처 확인 필요", theme::SELECTED_ORDER
                            ));
                        }
                        if reception_service::text(&analysis.client_match, "source") == "CONFLICT" {
                            ui.label(typography::status(
                                reception_service::text(&analysis.client_match, "detail"),
                                theme::DANGER,
                            ));
                        }
                        if !analysis.client_candidates.is_empty() {
                            ui.label(typography::caption("거래처 후보 선택"));
                            for candidate in &analysis.client_candidates {
                                let id = reception_service::text(candidate, "clientId");
                                let name = reception_service::text(candidate, "name");
                                if !id.is_empty() && !name.is_empty() &&
                                    theme::action(ui, name, ButtonRole::Quiet, true).clicked() {
                                    client_choice = Some(id.to_owned());
                                }
                            }
                        }
                        let unidentified = analysis.get("unassigned")
                            .and_then(serde_json::Value::as_array)
                            .map(Vec::len).unwrap_or(0);
                        let candidates = analysis.get("candidates")
                            .and_then(serde_json::Value::as_array)
                            .map(Vec::len).unwrap_or(0);
                        if unidentified > 0 || candidates > 0 {
                            ui.label(typography::status(
                                &format!("미분류 {unidentified}건 · 확인 후보 {candidates}건"),
                                theme::SELECTED_ORDER,
                            ));
                        }
                        let issue_count: usize = reception_service::recipients(analysis).iter()
                            .flat_map(|recipient| recipient.get("items")
                                .and_then(serde_json::Value::as_array)
                                .into_iter().flatten())
                            .map(|item| item.get("issues")
                                .and_then(serde_json::Value::as_array)
                                .map(Vec::len).unwrap_or(0))
                            .sum();
                        if issue_count > 0 {
                            ui.label(typography::status(
                                &format!("상품 확인 {issue_count}건"),
                                theme::SELECTED_ORDER,
                            ));
                        }
                    }
                        });
                });
            },
        );
        if let Some(client_id) = client_choice {
            self.select_client_candidate(&client_id, ui.ctx());
        }
        if let Some(client_id)=refresh_price {
            self.apply_manual_client(&client_id,ui.ctx());
        }
        if sync_clicked {
            self.start_working_sync(ui.ctx());
        }
        components::paint_dock_splitter(ui, first_sep, false, false);

        let mut next_recipient = None;
        ui.scope_builder(
            UiBuilder::new().id_salt("details-recipient")
                .max_rect(recipient_rect).layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(recipient_rect);
                pane.set_width(recipient_rect.width());
                pane.set_height(recipient_rect.height());
                Self::section(pane, "수취인별 주문 정보", recipient_height, |ui| {
                    if let Some(analysis) = &self.analysis {
                        let recipients = reception_service::recipients(analysis);
                        if recipients.len() > 1 {
                            egui::ScrollArea::horizontal()
                                .id_salt("analyzed-recipients")
                                .drag_to_scroll(false)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for (index, recipient) in recipients.iter().enumerate() {
                                            let name = reception_service::text(recipient, "name");
                                            let label = if name.is_empty() {
                                                format!("수취인 {}", index + 1)
                                            } else {
                                                format!("{} · {}", index + 1, name)
                                            };
                                            let role = if index == self.selected_recipient {
                                                ButtonRole::ActiveNavigation
                                            } else {
                                                ButtonRole::Quiet
                                            };
                                            if theme::action(ui, &label, role, true).clicked() {
                                                next_recipient = Some(index);
                                            }
                                        }
                                    });
                                });
                        }
                        if let Some(recipient) = recipients.get(self.selected_recipient) {
                            ui.spacing_mut().item_spacing.y = theme::SPACE_4;
                            let mut name = reception_service::text(recipient, "name").to_owned();
                            let mut address = reception_service::text(recipient, "address").to_owned();
                            let mut mobile = reception_service::text(recipient, "mobile").to_owned();
                            let mut phone = reception_service::text(recipient, "phone").to_owned();
                            let mut note = reception_service::text(recipient, "note").to_owned();
                            let recipient_id=reception_service::text(recipient,"id");
                            for (label,value,kind,field) in [
                                ("이름",&mut name,components::FieldKind::Text,"name"),
                                ("주소",&mut address,components::FieldKind::Address,"address"),
                                ("휴대전화",&mut mobile,components::FieldKind::Telephone,"mobile"),
                            ] {
                                let response=components::labeled_field(ui,label,value,kind,false);
                                let key=source_highlight::key(recipient_id,recipient_id,field);
                                if response.hovered() {
                                    hovered_detail=Some(key.clone());
                                }
                                source_highlight::paint_active(
                                    ui,&response,&key,self.link_hover.active.as_ref()
                                );
                            }
                            if !phone.trim().is_empty() {
                                let response=components::labeled_field(
                                    ui,"전화",&mut phone,components::FieldKind::Telephone,false
                                );
                                let key=source_highlight::key(recipient_id,recipient_id,"phone");
                                if response.hovered() {hovered_detail=Some(key.clone());}
                                source_highlight::paint_active(
                                    ui,&response,&key,self.link_hover.active.as_ref()
                                );
                            }
                            let response=components::labeled_field(
                                ui,"요청 메시지",&mut note,components::FieldKind::Text,false
                            );
                            let key=source_highlight::key(recipient_id,recipient_id,"note");
                            if response.hovered() {hovered_detail=Some(key.clone());}
                            source_highlight::paint_active(
                                ui,&response,&key,self.link_hover.active.as_ref()
                            );
                        }
                    }
                });
            },
        );
        if let Some(index) = next_recipient {
            self.selected_recipient = index;
        }
        let selected = self.analysis.as_ref().and_then(|analysis| {
            reception_service::recipients(analysis).get(self.selected_recipient)
        });
        components::paint_dock_splitter(ui, second_sep, false, false);

        let remaining = bottom_rect.height();
        ui.scope_builder(
            UiBuilder::new().id_salt("details-items").max_rect(bottom_rect)
                .layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(bottom_rect);
                pane.set_width(bottom_rect.width());
                pane.set_height(bottom_rect.height());
                Self::section(pane, "품목 및 운임", remaining, |ui| {
                    // Reserve the bottom summary from the pane geometry, not from
                    // the current text height. This keeps the summary at the bottom
                    // even when there are no products.
                    let section_chrome = theme::SECTION_PADDING * 2.0
                        + theme::SECTION_HEADER_HEIGHT
                        + theme::SECTION_CONTENT_GAP
                        + theme::SOFT_DIVIDER_WIDTH
                        + theme::SPACE_4;
                    let item_area_height =
                        (remaining - section_chrome - theme::SUMMARY_STRIP_HEIGHT)
                            .max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), item_area_height),
                        Layout::top_down(Align::Min),
                        |items_ui| {
                            items_ui.set_min_height(item_area_height);
                            if let Some(recipient) = selected {
                                if let Some(items) =
                                    recipient.get("items").and_then(serde_json::Value::as_array)
                                {
                                    egui::ScrollArea::vertical()
                                        .id_salt("recognized-items")
                                        .max_height(item_area_height)
                                        .auto_shrink([false, false])
                                        .show(items_ui, |ui| {
                                            for item in items {
                                                let recipient_id=reception_service::text(recipient,"id");
                                                let item_id=reception_service::text(item,"id");
                                                ui.horizontal(|ui| {
                                                    let response=ui.label(typography::body(
                                                        &reception_service::item_title(item)
                                                    ));
                                                    let key=source_highlight::key(
                                                        recipient_id,item_id,"product"
                                                    );
                                                    if response.hovered() {hovered_detail=Some(key.clone());}
                                                    source_highlight::paint_active(
                                                        ui,&response,&key,self.link_hover.active.as_ref()
                                                    );
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            let response=ui.label(typography::quantity(
                                                                &reception_service::item_quantity(item)
                                                            ));
                                                            let key=source_highlight::key(
                                                                recipient_id,item_id,"quantity"
                                                            );
                                                            if response.hovered() {
                                                                hovered_detail=Some(key.clone());
                                                            }
                                                            source_highlight::paint_active(
                                                                ui,&response,&key,self.link_hover.active.as_ref()
                                                            );
                                                        },
                                                    );
                                                });
                                                if let Some(issues) = item.get("issues")
                                                    .and_then(serde_json::Value::as_array)
                                                {
                                                    for issue in issues.iter().take(3) {
                                                        if let Some(message) = issue.as_str() {
                                                            ui.label(typography::status(message, theme::SELECTED_ORDER));
                                                        }
                                                    }
                                                }
                                                components::soft_divider(ui);
                                            }
                                        });
                                }
                            }
                        },
                    );
                    let preview = self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|draft| draft.id==id))
                        .and_then(|draft|draft.pricing_preview.as_ref())
                        .filter(|quote|
                            quote.get("sourceRaw").and_then(serde_json::Value::as_str)
                                ==Some(self.raw_source.as_str())
                        );
                    let total = preview
                        .and_then(|quote|quote.get("total"))
                        .and_then(serde_json::Value::as_i64)
                        .map(components::display_won)
                        .unwrap_or_else(||"—".to_owned());
                    let synced=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|row|row.id==id))
                        .is_some_and(|row|row.working_synced);
                    let note = if synced {
                        "Rust 참고 금액 · 운영 미확정 작업본 저장됨"
                    } else if preview.is_some() {
                        "Rust 재계산 참고 · 운영 작업본 저장 전"
                    } else { "" };
                    components::summary_strip(ui, "총 주문", &total, note);
                });
            },
        );
        // Hovering details highlights the exact Rust source span after 200ms.
        // Leaving the field retains the last link for 250ms so the cursor
        // can cross panel gutters without flicker.
        self.link_hover.update(
            hovered_detail.or_else(||self.source_hover.clone()),ui.ctx()
        );
    }

    fn orders_pane(&mut self, ui: &mut Ui) {
        let height=ui.available_height();
        let mut event=None;
        Self::section(ui,"처리 중인 주문",height,|ui|{
            if let Some(error)=self.workflow_error.as_deref(){
                ui.label(typography::status(error,theme::DANGER));
            }
            egui::ScrollArea::vertical()
                .id_salt("working-receptions-list")
                .auto_shrink([false,false])
                .show(ui,|ui|{
                    event=workflow_board_ui::show(
                        ui,self.workflow_snapshot.as_ref(),&self.drafts,
                        self.active_id.as_deref(),&mut self.workflow_ui
                    );
                });
        });
        match event{
            Some(workflow_board_ui::Event::SelectPending(id))=>{
                self.workflow_ui.selection=None;
                self.workflow_ui.folded=None;
                self.workflow_ui.stop_preview();
                self.select_reception(&id,ui.ctx());
            }
            Some(workflow_board_ui::Event::SelectShipment(selection))=>{
                self.workflow_ui.select(selection);
            }
            _=>{}
        }
    }

    fn other_page(&mut self, ctx: &egui::Context) {
        if self.page==Page::Clients && self.client_manager.is_none() {
            self.client_manager=Some(client_manager::ClientManager::new(
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
                                pane.set_width(left.width());
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
                        components::paint_dock_splitter(
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
                                        drag.initial_widths,0,response.drag_delta().x
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
                            UiBuilder::new().id_salt(("management-page",self.page.label()))
                                .max_rect(right).layout(Layout::top_down(Align::Min)),
                            |main|{
                                main.set_clip_rect(right);
                                main.set_width(right.width());
                                main.set_height(right.height());
                                Frame::new().inner_margin(theme::SPACE_16).show(main,|page|{
                                    if self.page==Page::Clients {
                                        if let Some(manager)=self.client_manager.as_mut() {
                                            manager.show_page(page,Arc::clone(&self.gateway));
                                        }
                                    }else{
                                        page.label(typography::page_heading(self.page.label()));
                                    }
                                });
                            }
                        );
                    };
                if content_width > viewport_width + 0.5 {
                    egui::ScrollArea::horizontal()
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
        // eframe invokes this before the native HUB HWND is destroyed.
        // Kakao must lose its temporary owner BEFORE HUB teardown.
        self.kakao_dock.disconnect();
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_source_analysis();
        self.poll_workflow_board(ctx);
        self.poll_client_change(ctx);
        self.poll_working_sync(ctx);
        self.poll_source_saves();
        self.poll_clipboard_intake(ctx);
        // HWND ownership now handles HUB activation. Only the 350ms
        // geometry/new-conversation refresh remains; no 32ms focus polling.
        if self.kakao_dock.connected() {
            ctx.request_repaint_after(Duration::from_millis(350));
        }
        if self.page==Page::Reception &&
            self.client_manager.as_mut().is_some_and(|manager|manager.take_changed()) &&
            !self.raw_source.trim().is_empty() {
            // A registered/edited/deleted client changes the same native
            // matching index that reception must see on its next frame.
            self.analysis_due_at=Some(Instant::now());
        }
        if self.page == Page::Reception {
            self.kakao_tracker.refresh();
            ctx.request_repaint_after(Duration::from_millis(450));
            if !self.source_editing && self.raw_source.is_empty() {
                self.clipboard_monitor.refresh();
            }
        }
        if self.analysis_due_at.is_some_and(|at| Instant::now() >= at) {
            self.analysis_due_at = None;
            if !self.raw_source.trim().is_empty() {
                self.start_source_analysis(ctx.clone());
            }
        }
        self.top_bar(ctx);
        if self.page == Page::Reception {
            self.reception(ctx);
        } else {
            self.other_page(ctx);
        }
        let work_area=self.modal_work_area(ctx);
        if self.ocr_review.is_some() || self.pending_replace.is_some() ||
            (self.page==Page::Reception &&
                (self.client_chooser.is_some() || self.manual_workspace.is_some())) {
            components::modal_backdrop(
                ctx,"bando-workspace-modal-backdrop",false
            );
        }
        self.show_ocr_review(ctx,work_area);
        self.show_replacement_confirmation(ctx,work_area);
        if self.page == Page::Reception {
            self.show_client_chooser(ctx,work_area);
            self.show_manual_workspace(ctx,work_area);
        } else if self.page==Page::Clients {
            if let Some(manager)=self.client_manager.as_mut() {
                if manager.has_modal() {
                    components::modal_backdrop(
                        ctx,"client-manager-modal-backdrop",false
                    );
                    manager.show_editor(ctx,work_area,Arc::clone(&self.gateway));
                }
            }
        }
        self.frameless_resize(ctx);
    }
}

fn main() -> eframe::Result<()> {
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
            let autosave = source_autosave::SourceAutosave::new(
                Arc::clone(&backend), restored.revision, cc.egui_ctx.clone()
            );
            let source_editing = !restored.raw.is_empty();
            let saved_type = match OrderType::parse(&restored.order_type) {
                Ok(kind) => kind,
                Err(error) => {
                    eprintln!("[BANDO HUB] 접수 유형 복원 실패: {error}");
                    std::process::exit(2);
                }
            };
            if source_editing { cc.egui_ctx.request_repaint_after(Duration::from_millis(260)); }
            Ok(Box::new(BandoEgui {
                brand_texture: Some(brand_texture),
                gateway: backend,
                raw_source: restored.raw,
                drafts: restored.drafts,
                active_id: restored.active_id,
                source_editing,
                order_type: saved_type,
                source_autosave: Some(autosave),
                analysis_due_at: if source_editing {
                    Some(Instant::now() + Duration::from_millis(250))
                } else { None },
                ..BandoEgui::default()
            }))
        }),
    )
}
