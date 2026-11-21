//! BANDO HUB egui — first native visual implementation.
//! UI preview only: uses existing read-only Rust business decisions and
//! a separate egui-preview.sqlite3 for source continuity. Existing React/Tauri
//! stays untouched. A production client master may be read ONLY by explicit
//! read-only opt-in; KakaoTalk is placed by a separate reversible Win32 dock.

mod theme;
mod typography;
mod layout;
mod components;
mod floating_notice;
mod reception_service;
mod gateway;
mod http_gateway;
mod preview_store;
mod source_autosave;
mod clipboard_intake;
mod ocr_learning;
mod client_directory;
mod client_master;
mod client_manager;
mod automatic_message_manager;
mod category_manager;
mod alias_manager;
mod latest_job;
mod brand_manager;
mod model_group_manager;
mod product_manager;
mod product_excel;
mod product_excel_ui;
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
mod developer_ui_samples;

use eframe::egui::{self, Align, Frame, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use theme::{ButtonRole, WindowControl};
use clipboard_intake::{ClipboardImage, ClipboardMonitor, ClipboardPayload, OrderType};
use preview_store::{OcrFeedback, WorkingReception, WorkingSource};
use kakao_context::{ChatWindow, KakaoTracker};
use client_chooser::{ClientChooser, Choice};
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
    /// Entirely optional: disabled starts with unmodified OCR output.
    learning_enabled: bool,
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

// Keep deletion bound to the exact unconfirmed receipt and raw text. The
// Rust rule must approve the reset before any egui preview state changes.
struct SourceClearRequest {
    order_id: String,
    source_raw: String,
    order_type: String,
    client_id: Option<String>,
}
struct PendingSourceClear {
    request: SourceClearRequest,
    receiver: Receiver<Result<(), String>>,
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

#[derive(Clone, Copy)]
struct DetailSplitDrag {
    index: usize,
    initial_heights: [f32; 3],
}

struct PendingClientChange {
    order_id: String,
    source_raw: String,
    order_type: String,
    // A changed Kakao conversation invalidates the pending dealer decision,
    // even when the pasted text itself has not changed.
    chat_handle: Option<String>,
    chat_title: Option<String>,
    receiver: Receiver<Result<native_order_change::ChangeResult, String>>,
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
    layout: layout::LayoutState,
    dragging_split: Option<HorizontalSplitDrag>,
    dragging_vertical: Option<VerticalSplitDrag>,
    dragging_detail: Option<DetailSplitDrag>,
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
    pending_clipboard_clear: Option<PendingClipboardClear>,
    source_clear_confirm: Option<SourceClearRequest>,
    source_clear_job: Option<PendingSourceClear>,
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
    inbox_folder_receiver: Option<Receiver<Result<Option<String>, String>>>,
    inbox_folder_last_refresh: Option<Instant>,
    freight_manager: Option<freight_manager::FreightManager>,
    order_history_manager: Option<order_history_manager::OrderHistoryManager>,
    daily_settlement_manager: Option<daily_settlement_manager::DailySettlementManager>,
    excel_manager: Option<excel_manager::ExcelManager>,
    freight_pending_page: Option<Page>,
    freight_pending_save: bool,
    product_pending_page: Option<Page>,
    client_pending_page: Option<Page>,
    settings_pending_page: Option<Page>,
    settings_pending_save: bool,
    manual_workspace: Option<manual_workspace::ManualWorkspace>,
    client_change: Option<PendingClientChange>,
    auto_client_change: Option<PendingAutomaticClient>,
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
    // Transient name-only directory: resolve selected dealer ID in ALL
    // pending cards, not from an advisory match discarded on manual edits.
    client_names: HashMap<String,String>,
    client_names_receiver: Option<Receiver<Result<HashMap<String,String>,String>>>,
    client_names_last_refresh: Option<Instant>,
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
            layout: layout::LayoutState::load(),
            dragging_split: None,
            dragging_vertical: None,
            dragging_detail: None,
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
            pending_clipboard_clear: None,
            source_clear_confirm: None,
            source_clear_job: None,
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
            inbox_folder_receiver: None,
            inbox_folder_last_refresh: None,
            freight_manager: None,
            order_history_manager: None,
            daily_settlement_manager: None,
            excel_manager: None,
            freight_pending_page: None,
            freight_pending_save: false,
            product_pending_page: None,
            client_pending_page: None,
            settings_pending_page: None,
            settings_pending_save: false,
            manual_workspace: None,
            client_change: None,
            auto_client_change: None,
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
            client_names: HashMap::new(),
            client_names_receiver: None,
            client_names_last_refresh: None,
            analysis_request: 0,
            analysis_pending: false,
            analysis_due_at: None,
            analysis_receiver: None,
        }
    }
}

impl BandoEgui {
    /// Project only the saved order-image inbox path into the reception UI.
    /// The Settings page owns editing/saving; this read-only projection runs
    /// asynchronously and refreshes immediately on return from other pages.
    fn poll_inbox_folder(&mut self, ctx: &egui::Context) {
        if self.page != Page::Reception {
            // Discard in-flight old reads so a just-saved setting is freshly
            // fetched as soon as the employee returns to Reception.
            self.inbox_folder_receiver = None;
            self.inbox_folder_last_refresh = None;
            self.inbox_folder_loaded = false;
            self.inbox_folder_path = None;
            self.inbox_folder_error = None;
            self.inbox_folder_open_error = None;
            return;
        }
        if let Some(result) = self.inbox_folder_receiver.as_ref().map(|rx| rx.try_recv()) {
            match result {
                Ok(Ok(path)) => {
                    if self.inbox_folder_path != path {
                        self.inbox_folder_open_error = None;
                    }
                    self.inbox_folder_path = path;
                    self.inbox_folder_loaded = true;
                    self.inbox_folder_error = None;
                    self.inbox_folder_receiver = None;
                }
                Ok(Err(error)) => {
                    self.inbox_folder_path = None;
                    self.inbox_folder_loaded = true;
                    self.inbox_folder_error = Some(error);
                    self.inbox_folder_receiver = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.inbox_folder_path = None;
                    self.inbox_folder_loaded = true;
                    self.inbox_folder_error = Some("발주 이미지 수신 폴더 설정을 읽지 못했습니다.".into());
                    self.inbox_folder_receiver = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.inbox_folder_receiver.is_some() ||
            self.inbox_folder_last_refresh.is_some_and(|time|
                time.elapsed() < Duration::from_secs(15)
            ) {
            return;
        }
        self.inbox_folder_last_refresh = Some(Instant::now());
        let (sender, receiver) = mpsc::channel();
        self.inbox_folder_receiver = Some(receiver);
        let gateway = Arc::clone(&self.gateway);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = gateway.load_settings().and_then(|snapshot| {
                let saved = snapshot.get("settings")
                    .and_then(serde_json::Value::as_object)
                    .ok_or("저장된 폴더 설정 형식을 확인할 수 없습니다.")?;
                Ok(saved.get("orderImageInboxDirectory")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned))
            });
            let _ = sender.send(outcome);
            ctx.request_repaint();
        });
    }

    // Run parsing outside the egui frame loop. Any changed source invalidates
    // the previous result and its request sequence, so stale replies are ignored.
    fn queue_source_save(&mut self) {
        self.queue_source_save_with_feedback(None);
    }

    fn queue_source_save_with_feedback(&mut self, feedback:Option<OcrFeedback>) {
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
                // Invalidate an unfinished auto-pricing response for an old
                // raw/kind even before a new Rust analysis begins.
                self.auto_client_change = None;
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
            if let Err(error) = worker.queue_with_feedback(
                source, feedback.into_iter().collect()
            ) {
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
    /// Name-only master projection for pending-order headings. The worker
    /// reads the same explicitly configured catalog as client selection.
    /// Never run IO in egui's frame and never persist names in draft SQLite.
    fn poll_client_names(&mut self,ctx:&egui::Context){
        if let Some(result)=self.client_names_receiver.as_ref()
            .map(|receiver|receiver.try_recv()){
            match result{
                Ok(Ok(names))=>{
                    self.client_names=names;
                    self.client_names_receiver=None;
                    ctx.request_repaint();
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected)=>{
                    self.client_names_receiver=None;
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if self.page!=Page::Reception{
            // Re-check the current customer master on return from its editor.
            self.client_names_last_refresh=None;
            return;
        }
        if self.client_names_receiver.is_some() ||
            self.client_names_last_refresh.is_some_and(|time|
                time.elapsed()<Duration::from_secs(60)){return;}
        self.client_names_last_refresh=Some(Instant::now());
        let gateway=Arc::clone(&self.gateway);
        let context=ctx.clone();
        let (sender,receiver)=mpsc::channel();
        self.client_names_receiver=Some(receiver);
        std::thread::spawn(move||{
            let result=gateway.load_catalog().map(|catalog|{
                let mut names=HashMap::new();
                if let Some(clients)=catalog.get("clients")
                    .and_then(serde_json::Value::as_array){
                    for client in clients{
                        if client.get("deletedAt").is_some_and(|v|!v.is_null())
                            || client.get("active")==Some(&serde_json::Value::Bool(false)){
                            continue;
                        }
                        let id=client.get("id")
                            .and_then(serde_json::Value::as_str).unwrap_or("");
                        let name=client.get("name")
                            .and_then(serde_json::Value::as_str)
                            .filter(|name|!name.trim().is_empty())
                            .or_else(||client.get("displayName")
                                .and_then(serde_json::Value::as_str))
                            .unwrap_or("");
                        if !id.is_empty() && !name.trim().is_empty(){
                            names.insert(id.to_owned(),name.to_owned());
                        }
                    }
                }
                names
            });
            let _=sender.send(result);
            context.request_repaint();
        });
    }

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
        // Employee selection overrides any still-running automatic match.
        // Dropping its receiver prevents a late auto quote from taking over.
        self.auto_client_change=None;
        let target=client_id.to_owned();
        let order_id=row.id.clone();
        let source_raw=row.raw.clone();
        let order_type=row.order_type.clone();
        let gateway=Arc::clone(&self.gateway);
        let (sender,receiver)=mpsc::channel();
        self.client_change=Some(PendingClientChange {
            order_id,source_raw,order_type,
            chat_handle:row.chat_handle.clone(),
            chat_title:row.chat_title.clone(),receiver,
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
            let same_chat=self.drafts.iter().find(|row|
                row.id==pending.order_id
            ).is_some_and(|row|
                row.chat_handle==pending.chat_handle &&
                row.chat_title==pending.chat_title &&
                row.manual_model.is_none()
            );
            if self.active_id.as_deref()!=Some(pending.order_id.as_str()) ||
               self.raw_source!=pending.source_raw ||
               self.order_type.as_str()!=pending.order_type ||
               !same_chat {
                if let Some(chooser)=self.client_chooser.as_mut(){
                    if chooser.order_id==pending.order_id && chooser.is_selecting() {
                        chooser.selection_failed(
                            "발주서나 카카오톡 대화창이 변경되었습니다. 거래처를 다시 선택하세요.".into()
                        );
                    }
                }
                return;
            }
            match result {
                Ok(result)=>{
                    if let Some(row)=self.drafts.iter_mut().find(|r|
                        r.id==pending.order_id && r.raw==pending.source_raw
                    ) {
                        row.client_id=Some(result.client_id);
                        row.client_selection=Some("MANUAL".into());
                        row.pricing_preview=result.pricing_preview;
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
                    if self.client_chooser.as_ref().is_some_and(|chooser|
                        chooser.order_id==pending.order_id && chooser.is_selecting()
                    ){
                        self.client_chooser=None;
                    }
                }
                Err(error)=>{
                    let message=format!("거래처 변경 확인 실패: {error}");
                    if let Some(chooser)=self.client_chooser.as_mut()
                        .filter(|chooser|
                            chooser.order_id==pending.order_id && chooser.is_selecting()
                        ){
                        chooser.selection_failed(message);
                    }else{
                        self.source_save_error=Some(message);
                    }
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
                if !verified { return; }
                // React's current-dealer click simply closes the chooser.
                // An unpriced quote is a separate pricing task, not grounds
                // to re-run the client mutation without a different dealer.
                if current_id.as_deref()==Some(id.as_str()) {
                    self.client_chooser=None;
                } else if self.client_change.is_some() {
                    if let Some(chooser)=self.client_chooser.as_mut(){
                        chooser.selection_failed(
                            "이전 거래처 변경이 끝난 뒤 다시 선택하세요.".into()
                        );
                    }
                } else {
                    self.apply_manual_client(&id,ctx);
                    if self.client_change.is_some() {
                        if let Some(chooser)=self.client_chooser.as_mut(){
                            chooser.begin_selection(&id);
                        }
                    } else if let Some(chooser)=self.client_chooser.as_mut(){
                        chooser.selection_failed(self.source_save_error.clone()
                            .unwrap_or_else(||"거래처 변경을 시작할 수 없습니다.".into()));
                    }
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
            source_input_method: None, last_input_method: None,
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
                                self.clipboard_monitor.refresh();
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
                source_autosave::SaveStatus::FeedbackFailed(error) => {
                    // Source storage succeeded but the separate, private OCR
                    // audit write failed. Never silently report learning saved.
                    self.clipboard_error=Some(format!(
                        "OCR 정정 원본/수정본 보관 실패: {error}"
                    ));
                }
            }
        }
    }

    fn start_source_analysis(&mut self, context: egui::Context) {
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
        self.auto_client_change=None;
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

    /// Run the EXISTING native Tauri change_order_client → merge → reprice
    /// → calculate_price rules after Rust title/raw matching, before marking
    /// an automatic dealer as applied. No UI-side billing or fake totals.
    fn start_auto_client_change(&mut self, client_id:&str, ctx:&egui::Context) {
        if self.client_change.is_some() || self.auto_client_change.is_some() {
            return;
        }
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
        self.auto_client_change=Some(PendingAutomaticClient {
            request:self.analysis_request,
            order_id:row.id.clone(),
            source_raw:row.raw.clone(),
            order_type:row.order_type.clone(),
            chat_handle:row.chat_handle.clone(),
            chat_title:row.chat_title.clone(),
            client_id:client_id.to_owned(),receiver,
        });
        let gateway=Arc::clone(&self.gateway);
        let target=client_id.to_owned();
        let context=ctx.clone();
        std::thread::spawn(move||{
            let result=native_order_change::prepare(gateway.as_ref(),&row,&target);
            let _=sender.send(result);
            context.request_repaint();
        });
    }

    /// Async results are accepted only for the exact recognition request,
    /// working receipt, source, Kakao chat, proposed client and owner state.
    /// A manual selection in progress cancels any outstanding automatic edit.
    fn poll_auto_client_change(&mut self,ctx:&egui::Context) {
        let received=self.auto_client_change.as_ref()
            .map(|pending|pending.receiver.try_recv());
        let result=match received {
            Some(Ok(value))=>Some(value),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("Rust 거래처 자동 연결 계산이 중단되었습니다.".into())),
            _=>None,
        };
        let Some(result)=result else {
            if self.auto_client_change.is_some() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
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
                    let mut auto_target:Option<String>=None;
                    if let Some(active_id) = self.active_id.as_deref() {
                        if let Some(item) = self.drafts.iter_mut()
                            .find(|item| item.id == active_id) {
                            item.analysis_raw = Some(self.raw_source.clone());
                            item.analysis_evidence = Some(analysis.recognition.clone());
                            item.client_match = Some(analysis.client_match.clone());
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
                                    && (item.pricing_preview.is_some()
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
                    if changed_snapshot {
                        self.queue_source_save();
                    }
                    self.analysis = Some(analysis);
                    self.analysis_error = None;
                    self.selected_recipient = 0;
                    if let Some(target)=auto_target {
                        self.start_auto_client_change(&target,ctx);
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
        if self.clipboard_busy{return;}
        self.clipboard_chat=self.current_intake_chat();
        self.clipboard_busy=true;
        self.ocr_processing=true;
        self.clipboard_error=None;
        self.clipboard_cancel.store(false,Ordering::Release);
        let cancelled=Arc::clone(&self.clipboard_cancel);
        let (tx,rx)=mpsc::channel();
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
                let text=clipboard_intake::recognize_image(&image,&cancelled)?;
                Ok(ClipboardReceipt::Ocr{text,image,sequence})
            });
            let _=tx.send((order_type,result));
            worker_context.request_repaint();
        });
    }

    fn begin_clipboard_intake(&mut self, order_type: OrderType, ctx: egui::Context) {
        if self.clipboard_busy || !self.clipboard_monitor.kind().has_intake() {
            return;
        }
        self.clipboard_chat=self.current_intake_chat();
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
            let result=clipboard_intake::read_with_sequence()
                .and_then(|(value,sequence)|match value{
                    ClipboardPayload::Text(text)=>
                        Ok(ClipboardReceipt::Text{text,sequence}),
                    ClipboardPayload::Image(image)=>{
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
            learning_used,
            model_suggestion,
        });
        let first_content = self.raw_source.trim().is_empty();
        let changed = self.raw_source != text;
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
                            learning_enabled:false,
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
                        if review.learning_enabled && suggestion.replacements>0
                            && review.edited_text==review.original_text {
                            review.edited_text=suggestion.text.clone();
                            review.model_suggestion=Some(suggestion.text);
                        }
                        review.model_error=None;
                        review.model_receiver=None;
                    }
                    Ok(Err(error))=>{
                        review.model_error=Some(error);
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
        let dialog_width=(work_area.width()*0.94).clamp(560.0,1480.0)
            .min(work_area.width().max(1.0));
        let dialog_height=(work_area.height()*0.88).clamp(450.0,960.0)
            .min(work_area.height().max(1.0));
        if let Some(review)=self.ocr_review.as_mut() {
            // One finite body owns every rect. The preview, editor and footer
            // remain stationary even at 400% zoom or 20,000 characters.
            let body_width=(dialog_width
                -2.0*theme::MODAL_PANEL_PADDING_X
                -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
            let body_height=(dialog_height
                -2.0*theme::MODAL_PANEL_PADDING_Y
                -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
            components::modal_window(
                "OCR 문자 인식 검토","egui-ocr-review",work_area
            )
                .fixed_size(Vec2::new(dialog_width,dialog_height))
                .scroll([false,false])
                .show(ctx,|ui|{
                    let (bounds,_) = ui.allocate_exact_size(
                        Vec2::new(body_width,body_height),Sense::hover()
                    );
                    let master=components::modal_master_layout(
                        bounds,
                        theme::MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT,
                        theme::MODAL_FOOTER_HEIGHT,
                        theme::MODAL_MASTER_WORKSPACE_GAP
                    );
                    let header_rect=master.header;
                    let footer_rect=master.footer;
                    let compare_rect=master.workspace;
                    close=components::modal_master_header_in_rect(
                        ui,header_rect,"OCR 문자 인식 검토",
                        "원본 이미지와 인식된 텍스트를 대조하고 오타를 수정한 뒤 반영하세요.",
                        "취소",true
                    );

                    // Common popup master owns the ONE workspace frame and
                    // both divider lines. The OCR pane ratio is content-specific.
                    let frame=components::modal_master_workspace_background(
                        ui,compare_rect,theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT,
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
                    let left_clip=left_header.intersect(ui.clip_rect());
                    ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-review-image-toolbar")
                            .max_rect(left_header)
                            .layout(Layout::left_to_right(Align::Center)),
                        |toolbar|{
                            toolbar.set_clip_rect(left_clip);
                            toolbar.set_width(left_header.width());
                            toolbar.label(
                                typography::subsection_heading("원본 이미지")
                            );
                            toolbar.with_layout(
                                Layout::right_to_left(Align::Center),
                                |zoom|{
                                    zoom.add_sized(
                                        Vec2::new(54.0,24.0),
                                        egui::Label::new(
                                            egui::RichText::new(format!(
                                                "{:>3.0}%",review.zoom*100.0
                                            )).monospace()
                                        )
                                    );
                                    zoom.add_sized(
                                        Vec2::new(118.0,24.0),
                                        egui::Slider::new(&mut review.zoom,0.5..=4.0)
                                            .show_value(false)
                                    ).on_hover_text(
                                        "원본 이미지 위에서 마우스 휠로 확대·축소"
                                    );
                                }
                            );
                        },
                    );
                    let right_clip=right_header.intersect(ui.clip_rect());
                    ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-review-text-toolbar")
                            .max_rect(right_header)
                            .layout(Layout::left_to_right(Align::Center)),
                        |toolbar|{
                            toolbar.set_clip_rect(right_clip);
                            toolbar.set_width(right_header.width());
                            // Keep the section heading LEFT and put the
                            // experimental option in a distinct RIGHT-anchored
                            // slot. The checkbox never consumes the heading's
                            // leading edge or gets pushed outside its clip.
                            toolbar.add(egui::Label::new(
                                typography::subsection_heading("인식된 텍스트 · 직접 수정")
                            ).truncate());
                            let help=review.model_error.as_deref()
                                .unwrap_or("반복 정정 패턴을 적용하되 최종 판단은 직원이 합니다.");
                            let mut changed=false;
                            toolbar.with_layout(
                                Layout::right_to_left(Align::Center),
                                |option|{
                                    changed=option.checkbox(
                                        &mut review.learning_enabled,
                                        "학습 모델 사용 (시험 기능)"
                                    ).on_hover_text(help).changed();
                                },
                            );
                            if changed {
                                review.model_error=None;
                                if review.learning_enabled {
                                    let initial=review.original_text.clone();
                                    let template_client=verified_template_client.clone();
                                    let (tx,rx)=mpsc::channel();
                                    review.model_receiver=Some(rx);
                                    let repaint=toolbar.ctx().clone();
                                    std::thread::spawn(move||{
                                        let outcome=(||{
                                            let spellings=preview_store::PreviewStore
                                                ::list_ocr_feedback(1000)?;
                                            let layouts=preview_store::PreviewStore
                                                ::list_ocr_layout_examples(1000)?;
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
                    ui.painter().rect_filled(
                        image_view_rect,
                        egui::CornerRadius{
                            nw:0,ne:0,sw:inner_radius,se:0
                        },
                        theme::CANVAS
                    );
                    ui.painter().rect_filled(
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
                    let image_clip=image_scroll_rect.intersect(ui.clip_rect());
                    ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-image-fixed-viewport")
                            .max_rect(image_scroll_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |viewport|{
                            viewport.set_clip_rect(image_clip);
                            viewport.set_width(image_scroll_rect.width());
                            viewport.set_height(image_scroll_rect.height());
                            let image_scroll=egui::ScrollArea::both()
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
                                        line.spacing_mut().item_spacing.x=0.0;
                                        if left_pad>0.0 {
                                            line.add_space(left_pad);
                                        }
                                        let image=line.image((
                                            review.texture.id(),display
                                        ));
                                        // The stroke belongs to the actual
                                        // source boundary and moves with it.
                                        line.painter().rect_stroke(
                                            image.rect,0.0,
                                            egui::Stroke::new(
                                                theme::SOFT_DIVIDER_WIDTH,edge
                                            ),
                                            egui::StrokeKind::Inside
                                        );
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
                    let text_clip=text_view_rect.intersect(ui.clip_rect());
                    ui.scope_builder(
                        UiBuilder::new().id_salt("ocr-text-fixed-viewport")
                            .max_rect(text_view_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |editor|{
                            editor.set_clip_rect(text_clip);
                            editor.set_width(text_view_rect.width());
                            editor.set_height(text_view_rect.height());
                            editor.add_sized(
                                text_view_rect.size(),
                                egui::TextEdit::multiline(&mut review.edited_text)
                                    // The unified workspace paints INPUT;
                                    // remove TextEdit's own second outline.
                                    .frame(false)
                                    // Larger, dedicated OCR-review typography:
                                    // compare misread model codes, numbers and
                                    // addresses without changing other screens.
                                    .font(egui::FontId::proportional(
                                        typography::FONT_OCR_REVIEW_TEXT
                                    ))
                                    .margin(egui::Margin::same(
                                        theme::SPACE_12 as i8
                                    ))
                                    .desired_width(text_view_rect.width())
                                    .desired_rows(1)
                            );
                        },
                    );

                    // The final master pass sits above both pane fills.
                    // Its flat, pixel-snapped dividers and single rounded
                    // outline repair the tiny overhangs and corner seams.
                    components::modal_master_workspace_finish(
                        ui,compare_rect,frame
                    );

                    let edited=review.edited_text.trim();
                    let valid=!edited.is_empty() && edited.chars().count()<=20_000;
                    components::modal_master_footer_in_rect(
                        ui,footer_rect,|buttons|{
                            if theme::action(
                                buttons,"적용",ButtonRole::Primary,valid
                            ).clicked() {
                                apply=Some((
                                    edited.to_owned(),
                                    review.order_type,
                                    review.chat.clone(),
                                    review.clipboard_sequence,
                                    review.original_text.clone(),
                                    review.learning_enabled,
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
                        let clip=hint_rect.intersect(ui.clip_rect());
                        ui.scope_builder(
                            UiBuilder::new().id_salt("ocr-review-footer-error")
                                .max_rect(hint_rect),
                            |hint|{
                                hint.set_clip_rect(clip);
                                hint.label(egui::RichText::new(
                                    "인식된 텍스트는 1~20,000자로 수정해 주세요."
                                ).color(theme::DANGER));
                            },
                        );
                    }
                });
        }
        if let Some((raw,kind,chat,sequence,original,learned,suggestion))=apply {
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
            components::modal_backdrop(
                ctx,"ocr-discard-nested-backdrop",true
            );
            components::decision_window(
                "변경사항 확인","ocr-review-discard-confirm",work_area
            ).show(ctx,|ui|{
                if components::modal_header(
                    ui,"변경사항 확인","돌아가기",true
                ) {
                    self.discard_ocr_confirm=false;
                }
                ui.label("OCR 검토에서 수정한 내용을 버릴까요?");
                components::modal_action_row(ui,|buttons|{
                    if theme::action(
                        buttons,"수정 내용 버리기",ButtonRole::Danger,true
                    ).clicked() {
                        self.ocr_review=None;
                        self.discard_ocr_confirm=false;
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
                cancel=components::modal_header(
                    ui,"발주원문 교체 확인","취소",true
                );
                ui.label("현재 초안과 분석 결과를 새 원문으로 교체할까요?");
                components::modal_action_row(ui,|actions|{
                    if theme::action(actions, "교체", ButtonRole::Primary, true).clicked() {
                        accept = true;
                    }
                });
            });
        if accept {
            if let Some(next) = self.pending_replace.take() {
                // Explicit replacement starts a new original-source route;
                // OCR appended later still preserves this first method.
                self.apply_intake_text(
                    next.text,next.order_type,next.chat,ctx,
                    next.input_method,true,next.clipboard_sequence,
                    next.ocr_original,next.ocr_learning_used,
                    next.ocr_model_suggestion
                );
            }
        } else if cancel {
            self.pending_replace = None;
        }
    }

    // The old React "내용 삭제" operation calls Rust replace_reception,
    // which clears the dealer and classification when the new source is empty.
    fn request_source_clear(&mut self) {
        if self.source_clear_job.is_some() || self.client_change.is_some() ||
           self.working_sync.is_some() || self.manual_workspace.is_some() ||
           self.clipboard_busy || self.ocr_review.is_some() || self.pending_replace.is_some() {
            return;
        }
        let Some(row)=self.active_id.as_deref()
            .and_then(|id|self.drafts.iter().find(|row|row.id==id))
        else {return;};
        if self.raw_source.trim().is_empty() {return;}
        self.source_clear_confirm=Some(SourceClearRequest {
            order_id:row.id.clone(),
            source_raw:self.raw_source.clone(),
            order_type:self.order_type.as_str().to_owned(),
            client_id:row.client_id.clone(),
        });
    }

    fn show_source_clear_confirmation(&mut self,ctx:&egui::Context,work_area:Rect) {
        if self.source_clear_confirm.is_none() {return;}
        let mut accept=false;
        let mut cancel=false;
        components::decision_window(
            "발주원문 삭제 확인","egui-clear-source-confirm",work_area
        ).show(ctx,|ui|{
            cancel=components::modal_header(ui,"발주원문 삭제 확인","취소",true);
            ui.label("현재 미확정 원문과 분석 결과를 비울까요?");
            components::modal_action_row(ui,|actions|{
                if theme::action(actions,"내용 삭제",ButtonRole::Danger,true).clicked(){
                    accept=true;
                }
            });
        });
        if cancel {
            self.source_clear_confirm=None;
        } else if accept {
            let Some(request)=self.source_clear_confirm.take() else {return;};
            let valid=self.active_id.as_deref()==Some(request.order_id.as_str()) &&
                self.raw_source==request.source_raw &&
                self.order_type.as_str()==request.order_type &&
                self.drafts.iter().any(|row|
                    row.id==request.order_id && row.raw==request.source_raw &&
                    row.client_id==request.client_id);
            if !valid {
                self.source_save_error=Some(
                    "발주서 내용이 변경되었습니다. 삭제하려면 다시 확인하세요.".into()
                );
                return;
            }
            let gateway=Arc::clone(&self.gateway);
            let (sender,receiver)=mpsc::channel();
            let context=ctx.clone();
            let order_id=request.order_id.clone();
            let source_raw=request.source_raw.clone();
            let order_type=request.order_type.clone();
            let client_id=request.client_id.clone();
            std::thread::spawn(move||{
                // An isolated preview receipt is projected into the same
                // domain contract as React's nativeReplaceReception.
                let original=serde_json::json!({
                    "id":order_id,"raw":source_raw,"clientId":client_id,
                    "orderType":order_type,
                    "createdAt":chrono::Utc::now().to_rfc3339()
                });
                let result=gateway.execute(gateway::ReadOperation::NewReception,
                    serde_json::json!({
                        "action":"replace_reception","order":original,
                        "raw":"","now":chrono::Utc::now().to_rfc3339()
                    })
                ).and_then(|value|{
                    let cleared=value.get("order").ok_or(
                        "Rust 원문 삭제 결과에 주문 정보가 없습니다.")?;
                    if cleared.get("raw").and_then(serde_json::Value::as_str)!=Some("") ||
                       cleared.get("clientId").and_then(serde_json::Value::as_str)!=Some("") {
                        return Err("Rust 원문 삭제 결과가 올바르지 않습니다.".into());
                    }
                    Ok(())
                });
                let _=sender.send(result);
                context.request_repaint();
            });
            self.source_clear_job=Some(PendingSourceClear{request,receiver});
        }
    }

    fn poll_source_clear(&mut self,ctx:&egui::Context) {
        let received=self.source_clear_job.as_ref()
            .map(|job|job.receiver.try_recv());
        let outcome=match received {
            Some(Ok(outcome))=>Some(outcome),
            Some(Err(mpsc::TryRecvError::Disconnected))=>
                Some(Err("Rust 원문 삭제 요청이 중단되었습니다.".into())),
            _=>None,
        };
        let Some(outcome)=outcome else {
            if self.source_clear_job.is_some() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return;
        };
        let Some(job)=self.source_clear_job.take() else {return;};
        // Ignore late replies when a different receipt or edited source is open.
        let valid=self.active_id.as_deref()==Some(job.request.order_id.as_str()) &&
            self.raw_source==job.request.source_raw &&
            self.order_type.as_str()==job.request.order_type &&
            self.drafts.iter().any(|row|
                row.id==job.request.order_id &&
                row.raw==job.request.source_raw &&
                row.client_id==job.request.client_id);
        if !valid {return;}
        match outcome {
            Ok(())=>{
                self.raw_source.clear();
                self.source_editing=false;
                self.clear_source_analysis();
                if let Some(row)=self.drafts.iter_mut().find(|row|
                    row.id==job.request.order_id
                ){
                    row.client_id=None;
                    row.client_selection=None;
                    row.source_input_method=None;
                    row.last_input_method=None;
                    row.chat_handle=None;
                    row.chat_title=None;
                }
                self.queue_source_save();
                ctx.request_repaint();
            }
            Err(error)=>{
                self.source_save_error=Some(format!("원문 삭제 실패: {error}"));
            }
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
                                if product_can_leave && client_can_leave &&
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
                                        if self.page==Page::Freight && page!=Page::Freight {
                                            if let Some(manager)=self.freight_manager.as_mut(){
                                                manager.discard_drafts();
                                            }
                                        }
                                        self.page=page;
                                    }
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
        ui: &mut Ui,
        title: &str,
        minimum_height: f32,
        header_right: impl FnOnce(&mut Ui),
        content: impl FnOnce(&mut Ui),
    ) {
        let width = ui.available_width();
        theme::docked_pane_frame().show(ui, |ui| {
            let width_inset = 2.0 * theme::SECTION_PADDING;
            let height_inset = 2.0 * theme::RECEPTION_PANE_PADDING_Y;
            ui.set_min_width((width - width_inset).max(60.0));
            ui.set_min_height((minimum_height - height_inset).max(48.0));
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
            components::reception_section_divider(ui);
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
                        toolbar.spacing_mut().item_spacing.x=theme::SPACE_4;
                        toolbar.add_space(theme::KAKAO_TOOLBAR_PAD_X);
                        // Left: Kakao identity icon + one two-option mode
                        // group. Keep 8pt between the icon and first mode
                        // button AND between the two mode buttons.
                        components::kakao_menu_icon(toolbar);
                        toolbar.spacing_mut().item_spacing.x=
                            theme::KAKAO_ICON_MODE_GAP;
                        let mode_role=|active|if active{
                            ButtonRole::ActiveKakaoControl
                        }else{ButtonRole::KakaoControl};
                        if theme::action(toolbar,"겹쳐 보기",
                            mode_role(current_mode==kakao_dock::DockMode::Together),true
                        ).clicked(){
                            next_mode=Some(kakao_dock::DockMode::Together);
                        }
                        toolbar.spacing_mut().item_spacing.x=theme::SPACE_8;
                        if theme::action(toolbar,"나누어 보기",
                            mode_role(current_mode==kakao_dock::DockMode::Split),true
                        ).clicked(){
                            next_mode=Some(kakao_dock::DockMode::Split);
                        }
                        // Right: disconnect is an independent operation, not
                        // a third choice in the display-mode group.
                        toolbar.with_layout(Layout::right_to_left(Align::Center), |right| {
                            right.add_space(theme::KAKAO_TOOLBAR_PAD_X);
                            if theme::action(right,"연결 해제",
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
                        if theme::action(center,"카카오톡 연결",
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

    fn reception(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(theme::reception_workspace_frame())
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
                            components::paint_dock_network(
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
        let previewed=self.workflow_ui.active_preview()
            .filter(|active|*active==group.id)
            .and_then(|_|self.workflow_ui.preview_shipment.as_deref())
            .and_then(|id|group.shipments.iter().find(|ship|ship.id==id));
        let selected=self.workflow_ui.selection.as_ref()
            .filter(|s|s.group_id==group.id)
            .and_then(|s|group.shipments.iter().find(|ship|
                ship.id==s.shipment_id&&ship.order_id==s.order_id));
        let ship=previewed.or(selected).or_else(||group.shipments.first())?;
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
                // Read-only source and generated message are both documents.
                // Reserve the same bordered INPUT canvas and 4pt inner inset;
                // only the source's selectable content differs from a message.
                body.spacing_mut().item_spacing.y=0.0;
                let available=components::docked_section_body_height(height)
                    .min(body.available_height().max(0.0));
                let (document_rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),available),Sense::hover()
                );
                components::paint_source_document_surface(body,document_rect);
                if available>0.0 {
                    let inner=document_rect.shrink(theme::SPACE_4);
                    body.scope_builder(
                        UiBuilder::new()
                            .id_salt(("workflow-source-read-only",shipment.order_id.as_str()))
                            .max_rect(inner)
                            .layout(Layout::top_down(Align::Min)),
                        |canvas|{
                            canvas.set_clip_rect(inner);
                            canvas.set_width(inner.width());
                            canvas.set_height(inner.height());
                            egui::ScrollArea::vertical()
                                .id_salt(("workflow-source-lines",shipment.order_id.as_str()))
                                .max_height(inner.height())
                                .auto_shrink([false,false])
                                .show(canvas,|content|{
                                    content.add(
                                        egui::Label::new(typography::reception_source_text(
                                            &shipment.source_raw
                                        )).selectable(true)
                                    );
                                });
                        }
                    );
                }
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
        let mut start_region_capture=false;
        let mut open_client_chooser = false;
        let mut open_manual_workspace = false;
        let mut request_source_clear = false;
        let can_receive_clipboard =
            self.clipboard_monitor.kind().has_intake() && !self.clipboard_busy;
        let ocr_processing = self.ocr_processing;
        let cancel_ocr = Arc::clone(&self.clipboard_cancel);
        let inbox_path = self.inbox_folder_path.clone();
        let inbox_loaded = self.inbox_folder_loaded;
        let inbox_load_error = self.inbox_folder_error.clone();
        let mut inbox_open_request: Option<String> = None;
        let mut inbox_copy_request: Option<String> = None;
        Self::section_with_header(ui, "발주 원문", height, |ui| {
            if theme::action(ui, "+ 새 주문", ButtonRole::Emphasized, !ocr_processing).clicked() {
                begin_new_order = true;
            }
        }, |ui| {
            // Only the source-body layout owns spacing; the section header
            // and neighboring message pane keep their established tokens.
            ui.spacing_mut().item_spacing.y = 0.0;
            if ocr_processing {
                // Match the React/Electron source-document blocking state:
                // replace the entire source body instead of overlaying a
                // small status line on an interactive editor or intake menu.
                self.source_hover = None;
                let available_body = components::docked_section_body_height(height)
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
                        let content_height = typography::FONT_OCR_PROGRESS_TITLE * 1.4
                            + typography::METADATA_LINE_HEIGHT
                            + theme::ACTION_STANDARD_HEIGHT
                            + theme::SPACE_12 + theme::SPACE_16;
                        progress.add_space(
                            ((body_rect.height() - content_height) / 2.0).max(0.0)
                        );
                        progress.label(
                            egui::RichText::new("OCR 처리 중…")
                                .size(typography::FONT_OCR_PROGRESS_TITLE)
                                .strong()
                                .color(theme::TEXT)
                        );
                        progress.add_space(theme::SPACE_12);
                        progress.label(
                            egui::RichText::new(
                                "선택한 이미지에서 글자를 인식하고 있습니다."
                            ).size(typography::FONT_FIELD_LABEL)
                                .color(theme::SECONDARY)
                        );
                        progress.add_space(theme::SPACE_16);
                        if theme::action(
                            progress, "취소", ButtonRole::Standard, true
                        ).clicked() {
                            cancel_ocr.store(true, Ordering::Release);
                        }
                    },
                );
            } else if self.source_editing || !self.raw_source.is_empty() {
                // One allocated body rectangle owns the client card, source
                // editor and manual-classification action. The former sequence
                // of allocations mixed with scope_builder() could advance the
                // parent cursor so the document border landed on the client
                // card's bottom border. Position all three from ONE origin.
                let available_body = components::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                let (body_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), available_body),
                    Sense::hover(),
                );
                let has_source = !self.raw_source.trim().is_empty();
                // Equal visible 8pt clearances: body top -> client,
                // client -> document, document -> action, action -> bottom.
                // Short panes preserve these gaps and reduce editor height,
                // never overlap adjacent cards or change the dock splitter.
                let gap = theme::SPACE_8;
                let client_card_height = if has_source {
                    (available_body - 4.0 * gap - theme::ACTION_STANDARD_HEIGHT)
                        .max(0.0).min(70.0)
                } else { 0.0 };
                if has_source {
                    let card_rect = Rect::from_min_size(
                        egui::pos2(body_rect.left(), body_rect.top() + gap),
                        Vec2::new(body_rect.width(), client_card_height),
                    );
                    // The current client is supporting metadata, not
                    // another input editor. Shared master surface treatment
                    // changes only paint, never its reserved 70pt Rect.
                    components::paint_reception_client_summary_surface(
                        ui,card_rect
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
                            // The outer source body uses explicit spacing,
                            // but this 2-row card keeps its normal 8pt rhythm.
                            card.spacing_mut().item_spacing.y = theme::SPACING;
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
                let editor_top = if has_source {
                    body_rect.top() + gap + client_card_height + gap
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
                    let editor = ui.scope_builder(
                        UiBuilder::new()
                            .id_salt("source-editor-fixed-slot")
                            .max_rect(viewport)
                            .layout(Layout::top_down(Align::Min)),
                        |editor_ui| {
                            editor_ui.set_clip_rect(clip);
                            editor_ui.set_width(viewport.width());
                            editor_ui.set_height(viewport.height());
                            theme::source_document_frame().show(editor_ui, |document_ui| {
                            egui::ScrollArea::vertical()
                            .id_salt("source-editor-vertical-scroll")
                            // The frame border/padding occupies the outer
                            // editor slot; only the text contents scroll.
                            .max_height((editor_height-theme::SPACE_8
                                -2.0*theme::FRAME_STROKE_WIDTH).max(0.0))
                            .auto_shrink([false, false])
                            .show(document_ui, |scroll_ui| {
                                // Keep the source/message splitter fixed. Only
                                // this scroll area grows its internal content.
                                let lines=self.raw_source.lines().count().max(8).min(1000);
                                let mut layouter=|lay_ui:&egui::Ui,
                                    buffer:&dyn egui::TextBuffer, width:f32| {
                                    let job=source_highlight::layout(
                                        buffer.as_str(),&spans,
                                        selected_id.as_deref(),active_link.as_ref(),
                                        width,egui::FontId::proportional(
                                            typography::FONT_RECEPTION_SOURCE_TEXT
                                        ),theme::TEXT,
                                    );
                                    lay_ui.fonts_mut(|fonts|fonts.layout_job(job))
                                };
                                // Reserve an under-text paint slot before TextEdit draws its
                                // glyphs, caret and selection. Rounded gray source blocks
                                // remain decoration, never input hit targets.
                                let block_slot=scroll_ui.painter().add(egui::Shape::Noop);
                                let output=egui::TextEdit::multiline(&mut self.raw_source)
                                    .frame(false)
                                    .font(egui::FontId::proportional(
                                        typography::FONT_RECEPTION_SOURCE_TEXT
                                    ))
                                    .desired_rows(lines)
                                    .desired_width(scroll_ui.available_width())
                                    .layouter(&mut layouter)
                                    .show(scroll_ui);
                                let block_shapes=source_highlight::block_shapes(
                                    &self.raw_source,&output.galley,output.galley_pos,
                                    &spans,selected_id.as_deref(),active_link.as_ref(),
                                    scroll_ui.clip_rect().intersect(output.text_clip_rect),
                                );
                                scroll_ui.painter().set(
                                    block_slot,egui::Shape::Vec(block_shapes)
                                );
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
                        // Ctrl+V into the editable document is text paste,
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
                            if theme::action(actions,"수동 분류",ButtonRole::Standard,
                                ready && self.manual_workspace.is_none() &&
                                self.client_change.is_none() && self.working_sync.is_none() &&
                                self.source_clear_job.is_none()
                            ).clicked() {
                                open_manual_workspace=true;
                            }
                        },
                    );
                    // Two independent ends of the existing fixed action row;
                    // never move the manual button or the source/editor frame.
                    ui.scope_builder(
                        UiBuilder::new()
                            .id_salt("source-clear-action-fixed-slot")
                            .max_rect(actions_rect)
                            .layout(Layout::left_to_right(Align::Center)),
                        |actions| {
                            actions.set_clip_rect(actions_rect);
                            if theme::action(actions,"내용 삭제",ButtonRole::Standard,
                                actions_rect.width()>=200.0 &&
                                self.source_clear_job.is_none() &&
                                self.manual_workspace.is_none() &&
                                self.client_change.is_none() && self.working_sync.is_none() &&
                                !self.clipboard_busy
                            ).clicked() {
                                request_source_clear=true;
                            }
                        },
                    );
                }
            } else {
                self.source_hover=None;
                // Narrow panes may need a stacked button row. Scroll ONLY the
                // intake instructions, never the source/message divider.
                let intake_height = components::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                // Four identical free-space gaps: above the first group,
                // between the three groups, and below the last group.
                // Existing internal 8pt rhythms stay unchanged. When the
                // pane is short, the minimum gap triggers LOCAL scrolling.
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
                let folder_body_height = if inbox_path.is_some() {
                    typography::METADATA_LINE_HEIGHT
                        + theme::SPACE_8 + theme::ACTION_STANDARD_HEIGHT
                } else {
                    typography::METADATA_LINE_HEIGHT
                };
                let folder_group_height = typography::SUBSECTION_LINE_HEIGHT
                    + folder_body_height + theme::SPACE_8
;
                let dividers_height = 2.0 * theme::SOFT_DIVIDER_WIDTH;
                let balanced_gap = ((intake_height - clipboard_group_height
                    - manual_group_height - folder_group_height - dividers_height)
                    / 4.0).max(theme::SPACE_8);
                egui::ScrollArea::vertical()
                    .id_salt("source-intake-instructions")
                    .max_height(intake_height)
                    .auto_shrink([false, false])
                    .show(ui, |intake| {
                        intake.spacing_mut().item_spacing.y = 0.0;
                        intake.add_space(balanced_gap);
                        intake.label(typography::subsection_heading("클립보드 접수"));
                        intake.add_space(theme::SPACE_8);
                        intake.label(typography::caption(
                            "복사한 텍스트와 이미지를 자동으로 구분해 인식합니다."
                        ));
                        intake.add_space(theme::SPACE_8);
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
                        intake.add_space(theme::SPACE_8);
                        components::reception_capture_shortcut_hint(intake);
                        intake.add_space(balanced_gap / 2.0);
                        components::soft_divider(intake);
                        intake.add_space(balanced_gap / 2.0);
                        intake.label(typography::subsection_heading("다른 방법으로 접수"));
                        intake.add_space(theme::SPACE_8);
                        let (direct,capture) = components::paired_actions(
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
                        intake.add_space(balanced_gap / 2.0);
                        components::soft_divider(intake);
                        intake.add_space(balanced_gap / 2.0);
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
                        intake.add_space(theme::SPACE_8);
                        if let Some(path) = inbox_path.as_deref() {
                            intake.label(typography::caption(
                                "발주서 이미지나 엑셀파일을 지정한 폴더에 저장하면 자동 인식합니다."
                            ));
                            intake.add_space(theme::SPACE_8);
                            intake.horizontal(|buttons| {
                                buttons.spacing_mut().item_spacing.x = theme::SPACE_8;
                                if theme::action(buttons, "경로 복사",
                                    ButtonRole::Standard, true).clicked() {
                                    inbox_copy_request = Some(path.to_owned());
                                }
                                if theme::action(buttons, "폴더 열기",
                                    ButtonRole::Standard, true).clicked() {
                                    inbox_open_request = Some(path.to_owned());
                                }
                            });
                        } else if inbox_load_error.is_some() {
                            // Preserve failure state without extra UI message.
                        } else if !inbox_loaded {
                            intake.label(typography::caption(
                                "저장된 수신 폴더 설정을 확인하고 있습니다."
                            ));
                        } else {
                            intake.label(typography::caption(
                                "설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요."
                            ));
                        }
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
        if request_source_clear {
            self.request_source_clear();
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
        if start_region_capture{
            self.begin_region_capture(self.order_type,ui.ctx().clone());
        }
    }

    fn message_section(&mut self, ui: &mut Ui, height: f32) {
        if let Some((group,_shipment))=self.visible_workflow_group(){
            // React also blocks payment copies with actionable Rust issues.
            // Do not enable controls just because the preview has text.
            let can_copy=group.automatic_text.as_deref()
                .is_some_and(|text|!text.trim().is_empty())
                && matches!(group.automatic_phase.as_deref(),
                    Some("request") | Some("payment"))
                && (group.automatic_phase.as_deref()!=Some("payment")
                    ||group.automatic_payment_issues.is_empty());
            let mut copy_with_greeting=None;
            Self::section_with_header(ui,"복사용 자동 메시지",height,|header|{
                let mut include_link=false;
                header.add_enabled(false,egui::Checkbox::new(&mut include_link,"링크 포함"));
            },|body|{
                // All message-body spacing is explicit; default item spacing
                // previously pushed the bottom copy buttons past the pane.
                body.spacing_mut().item_spacing.y=0.0;
                let button_row_height=if body.available_width()<280.0{
                    theme::ACTION_STANDARD_HEIGHT*2.0+theme::SPACING
                }else{theme::ACTION_STANDARD_HEIGHT};
                let available=components::docked_section_body_height(height)
                    .min(body.available_height().max(0.0));
                let message_height=(available-button_row_height
                    -theme::RECEPTION_MESSAGE_ACTION_GAP).max(0.0);
                // The canvas owns its fixed rectangle even when the message
                // is empty or its content is shorter than the viewport.
                let (message_rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),message_height),Sense::hover()
                );
                components::paint_source_document_surface(body,message_rect);
                if message_height>0.0 {
                    let inner=message_rect.shrink(theme::SPACE_4);
                    body.scope_builder(
                        UiBuilder::new()
                            .id_salt("native-workflow-message-canvas")
                            .max_rect(inner)
                            .layout(Layout::top_down(Align::Min)),
                        |canvas|{
                            canvas.set_clip_rect(inner);
                            canvas.set_width(inner.width());
                            canvas.set_height(inner.height());
                            egui::ScrollArea::vertical()
                                .id_salt(("native-workflow-message",group.id.as_str()))
                                .max_height(inner.height())
                                .auto_shrink([false,false])
                                .show(canvas,|content|{
                                    if let Some(message)=group.automatic_text.as_deref(){
                                        content.add(egui::Label::new(typography::body(message))
                                            .selectable(true));
                                    }
                                });
                        },
                    );
                }
                components::reception_message_footer_gap(body);
                copy_with_greeting=components::reception_message_copy_actions(
                    body,can_copy
                );
            });
            if let Some(include_greeting)=copy_with_greeting{
                floating_notice::dispatch_pointer(
                    ui.ctx(),"reception.automatic.copy"
                );
                // The native Rust output engine owns the optional greeting.
                // Copying is clipboard-only until an authorized workflow
                // audit CAS transaction is exposed to this egui frontend.
                let result=self.gateway.execute(
                    gateway::ReadOperation::DecorateAutomaticCopy,
                    serde_json::json!({
                        "text":group.automatic_text.as_deref().unwrap_or(""),
                        "includeGreeting":include_greeting
                    })
                );
                match result {
                    Ok(rendered)=>{
                        if let Some(text)=rendered.get("text")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text|!text.trim().is_empty()){
                            ui.ctx().copy_text(text.to_owned());
                            floating_notice::publish(
                                ui.ctx(),"reception.automatic.copy",
                                floating_notice::Tone::Success,
                                "클립보드 복사 완료 · 업무 이력은 기록되지 않았습니다."
                            );
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
        let awaiting_order = self.raw_source.trim().is_empty()
            && self.analysis.is_none();
        Self::section_with_header(ui, "복사용 자동 메시지", height, |ui| {
            if !awaiting_order {
                let mut include_link = false;
                ui.add_enabled(false, egui::Checkbox::new(&mut include_link, "링크 포함"));
            }
        }, |ui| {
            if awaiting_order { return; }
            ui.spacing_mut().item_spacing.y=0.0;
            // Empty egui Frames shrink to their contents. Allocate the real
            // message rectangle explicitly so it fills the available width
            // instead of appearing as a narrow vertical stripe.
            let button_row_height = if ui.available_width() < 280.0 {
                theme::ACTION_STANDARD_HEIGHT * 2.0 + theme::SPACING
            } else {
                theme::ACTION_STANDARD_HEIGHT
            };
            let bottom_controls = button_row_height + theme::RECEPTION_MESSAGE_ACTION_GAP;
            let available_body = components::docked_section_body_height(height)
                .min(ui.available_height().max(0.0));
            let field_height = (available_body - bottom_controls).max(0.0);
            let (field_rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), field_height),
                Sense::hover(),
            );
            components::paint_source_document_surface(ui, field_rect);
            components::reception_message_footer_gap(ui);
            let _=components::reception_message_copy_actions(ui,false);
        });
    }

    fn details_and_freight(&mut self, ui: &mut Ui) {
        let full_height = ui.available_height();
        // The review, recipient and items sections own two independent
        // draggable boundaries; adjacent columns no longer dictate their sizes.
        let [review_height, recipient_height, _items_height] =
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
        let bottom_rect = Rect::from_min_size(
            egui::pos2(bounds.left(), second_sep.bottom()),
            Vec2::new(
                full_width, (bounds.bottom() - second_sep.bottom()).max(0.0)
            ),
        );

        // Register both middle-column splitters in the parent UI so they are
        // draggable in both working-order and empty/new-order states.
        let initial_heights = [review_height, recipient_height, bottom_rect.height()];
        for (index, splitter_rect) in [(0, first_sep), (1, second_sep)] {
            let id = ui.make_persistent_id(("detail-splitter", index));
            let response = ui.interact(splitter_rect, id, Sense::click_and_drag());
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            components::paint_dock_splitter(
                ui, splitter_rect, false, response.hovered() || response.dragged(),
            );
            if response.drag_started() {
                self.dragging_detail = Some(DetailSplitDrag {
                    index,
                    initial_heights,
                });
            }
            if response.dragged() {
                if let Some(drag) = self.dragging_detail {
                    if drag.index == index {
                        let delta = response.total_drag_delta().unwrap_or_default().y;
                        self.layout.resize_detail_pair(drag.initial_heights, index, delta);
                    }
                }
            }
            if response.drag_stopped()
                && self.dragging_detail.is_some_and(|drag| drag.index == index)
            {
                self.dragging_detail = None;
                let _ = self.layout.save();
            }
            if response.double_clicked() {
                self.layout.reset_detail_pair(index, full_height);
                let _ = self.layout.save();
            }
        }

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
                            .id_salt(("native-workflow-issues",group.id.as_str()))
                            .show(ui,|issues|{
                                for issue in &group.issues{
                                    components::reception_review_status(issues,issue);
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
                    pane.set_width(recipient_rect.width());
                    pane.set_height(recipient_rect.height());
                    let mut next_ship=None;
                    let count=group.shipments.len();
                    let index=group.shipments.iter()
                        .position(|item|item.id==ship.id).unwrap_or(0);
                    let name=reception_service::text(&ship.recipient,"name");
                    Self::section_with_header(
                        pane,"수취인별 주문 정보",recipient_height,
                        |header|{
                            next_ship=components::reception_recipient_navigator(
                                header,name,index,count
                            );
                        },|ui|{
                        egui::ScrollArea::vertical()
                            .max_height(components::docked_section_body_height(recipient_height))
                            .id_salt(("native-workflow-recipient-readonly",ship.id.as_str()))
                            .show(ui,|contents|{
                                let mut name = reception_service::text(&ship.recipient,"name").to_owned();
                                let mut address = reception_service::text(&ship.recipient,"address").to_owned();
                                let mut mobile = reception_service::text(&ship.recipient,"mobile").to_owned();
                                let mut phone = reception_service::text(&ship.recipient,"phone").to_owned();
                                let mut note = reception_service::text(&ship.recipient,"note").to_owned();
                                components::reception_recipient_fields(
                                    contents, &mut name, &mut address, &mut mobile,
                                    &mut phone, &mut note,
                                );
                            });
                        }
                    );
                    if let Some(next_index)=next_ship{
                        if let Some(target)=group.shipments.get(next_index){
                            self.workflow_ui.select(workflow_board_ui::Selection{
                                group_id:group.id.clone(),
                                order_id:target.order_id.clone(),
                                shipment_id:target.id.clone(),
                            });
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
                    pane.set_width(bottom_rect.width());
                    pane.set_height(bottom_rect.height());
                    Self::section(pane,"품목 및 운임",bottom_rect.height(),|ui|{
                        // Footer slots must consume EXACTLY their reserved
                        // heights, with no hidden egui spacing added between.
                        ui.spacing_mut().item_spacing.y=0.0;
                        // The list inherits the master's 10pt reserved solid
                        // rail; only the interior data viewport can scroll.
                        ui.spacing_mut().scroll=theme::management_table_scroll_style();
                        // React structure: 3-column product grid + freight + actions,
                        // independently scrolling above a fixed VAT and total footer.
                        let available = components::docked_section_body_height(
                            bottom_rect.height()
                        ).min(ui.available_height().max(0.0));
                        let list_height = (available
                            - theme::RECEPTION_VAT_ROW_HEIGHT
                            - theme::RECEPTION_TOTAL_HEIGHT
                            - 2.0 * theme::SPACE_4).max(0.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), list_height),
                            Layout::top_down(Align::Min),
                            |area| {
                                area.set_min_height(list_height);
                                egui::ScrollArea::vertical()
                                    .scroll_bar_visibility(
                                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                                    )
                                    .id_salt(("native-workflow-products",ship.id.as_str()))
                                    .max_height(list_height)
                                    .auto_shrink([false,false])
                                    .show(area,|contents|{
                                        contents.spacing_mut().item_spacing.y=0.0;
                                        components::reception_items_header(contents);
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
                                                    .map(components::display_won)
                                                    .unwrap_or_else(||"—".into());
                                                components::reception_item_row(
                                                    contents,
                                                    &reception_service::item_title(item),
                                                    &quantity, &amount,
                                                );
                                            }
                                        }
                                        contents.add_space(theme::SPACE_8);
                                        let remote=ship.recipient.get("remoteDelivery")
                                            .and_then(|v|v.get("checked"))
                                            .and_then(serde_json::Value::as_bool);
                                        let origin=ship.recipient.get("dispatchOrigin")
                                            .and_then(serde_json::Value::as_str);
                                        components::reception_freight_row(
                                            contents,"운임",
                                            if ship.collect {None} else {ship.freight},
                                            Some(ship.collect),remote,
                                            origin==Some("FACTORY"),
                                        );
                                        contents.add_space(theme::SPACE_8);
                                        components::reception_item_actions(contents,origin);
                                    });
                            }
                        );
                        ui.add_space(theme::SPACE_4);
                        components::reception_vat_row(ui,ship.vat_included);
                        ui.add_space(theme::SPACE_4);
                        let vat_label=match ship.vat_included {
                            Some(true)=>"부가세 포함",
                            Some(false)=>"부가세 별도",
                            None=>"—",
                        };
                        components::reception_total_footer(
                            ui,ship.order_total,Some(ship.shipment_count),
                            vat_label,"",
                        );
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
                    if self.analysis_error.is_some() {
                        // Failed analysis stays internal; do not show stale output.
                    } else if let Some(analysis) = &self.analysis {
                        let selected=self.active_id.as_deref()
                            .and_then(|id|self.drafts.iter().find(|row|row.id==id));
                        let manually_selected=selected.is_some_and(|row|
                            row.client_selection.as_deref()==Some("MANUAL"));
                        // A Rust suggestion is not an applied dealer until
                        // the exact native change/reprice result is accepted.
                        let automatically_selected=selected.is_some_and(|row|
                            row.client_selection.as_deref()==Some("AUTO") &&
                            row.client_id.as_deref()==analysis.client_match.get("clientId")
                                .and_then(serde_json::Value::as_str));
                        let matching_pending=self.auto_client_change.as_ref()
                            .is_some_and(|pending|
                                self.active_id.as_deref()==Some(pending.order_id.as_str()));
                        if analysis.directory_count == 0 {
                            components::reception_review_status(
                                ui,"거래처 명단 미연결 · 자동 매칭 확인 필요"
                            );
                        } else if manually_selected {
                            if let Some(name) = &analysis.chosen_client_name {
                                ui.label(typography::body(
                                    &format!("거래처 직접 선택 · {name}")
                                ));
                            }
                        } else if automatically_selected {
                            if let Some(name)=&analysis.matched_client_name {
                                ui.label(typography::body(
                                    &format!("거래처 자동 인식 · {name}")
                                ));
                            }
                        } else {
                            components::reception_review_status(
                                ui,if matching_pending {"거래처 확인 중"}
                                else {"거래처 확인 필요"}
                            );
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
                            components::reception_review_status(
                                ui,&format!("미분류 {unidentified}건 · 확인 후보 {candidates}건")
                            );
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
                            components::reception_review_status(
                                ui,&format!("상품 확인 {issue_count}건")
                            );
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

        let mut next_recipient = None;
        let recipient_nav=self.analysis.as_ref()
            .map(|analysis|reception_service::recipients(analysis))
            .filter(|recipients|recipients.len()>1)
            .map(|recipients|{
                let index=self.selected_recipient.min(recipients.len()-1);
                let name=reception_service::text(&recipients[index],"name")
                    .to_owned();
                (name,index,recipients.len())
            });
        ui.scope_builder(
            UiBuilder::new().id_salt("details-recipient")
                .max_rect(recipient_rect).layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(recipient_rect);
                pane.set_width(recipient_rect.width());
                pane.set_height(recipient_rect.height());
                Self::section_with_header(
                    pane,"수취인별 주문 정보",recipient_height,
                    |header|{
                        if let Some((name,index,total))=recipient_nav.as_ref(){
                            next_recipient=components::reception_recipient_navigator(
                                header,name,*index,*total
                            );
                        }
                    },|ui|{
                    if let Some(analysis) = &self.analysis {
                        let recipients = reception_service::recipients(analysis);
                        if let Some(recipient) = recipients.get(self.selected_recipient) {
                            let mut name = reception_service::text(recipient, "name").to_owned();
                            let mut address = reception_service::text(recipient, "address").to_owned();
                            let mut mobile = reception_service::text(recipient, "mobile").to_owned();
                            let mut phone = reception_service::text(recipient, "phone").to_owned();
                            let mut note = reception_service::text(recipient, "note").to_owned();
                            let recipient_id = reception_service::text(recipient, "id");
                            // Match the React contact grid and allow only its
                            // content to scroll when the user narrows this pane.
                            egui::ScrollArea::vertical()
                                .id_salt("recognized-recipient-fields")
                                .max_height(components::docked_section_body_height(recipient_height)
                                    .min(ui.available_height().max(0.0)))
                                .auto_shrink([false, false])
                                .show(ui, |fields| {
                                    let responses = components::reception_recipient_fields(
                                        fields, &mut name, &mut address, &mut mobile,
                                        &mut phone, &mut note,
                                    );
                                    for (response, field) in responses.into_iter().zip([
                                        "name", "address", "mobile", "phone", "note",
                                    ]) {
                                        let key = source_highlight::key(
                                            recipient_id, recipient_id, field,
                                        );
                                        if response.hovered() {
                                            hovered_detail = Some(key.clone());
                                        }
                                        source_highlight::paint_active(
                                            fields, &response, &key,
                                            self.link_hover.active.as_ref(),
                                        );
                                    }
                                });
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

        let remaining = bottom_rect.height();
        ui.scope_builder(
            UiBuilder::new().id_salt("details-items").max_rect(bottom_rect)
                .layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(bottom_rect);
                pane.set_width(bottom_rect.width());
                pane.set_height(bottom_rect.height());
                Self::section(pane, "품목 및 운임", remaining, |ui| {
                    // In intake standby, do not draw empty table headings,
                    // freight controls, VAT text or a fictitious zero total.
                    // Keep the section frame and divider exactly as-is.
                    if self.raw_source.trim().is_empty()
                        && self.analysis.is_none() {
                        return;
                    }
                    ui.spacing_mut().item_spacing.y=0.0;
                    ui.spacing_mut().scroll=theme::management_table_scroll_style();
                    let preview = self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|draft|draft.id==id))
                        .and_then(|draft|draft.pricing_preview.as_ref())
                        .filter(|quote|quote.get("sourceRaw")
                            .and_then(serde_json::Value::as_str)
                            ==Some(self.raw_source.as_str()));
                    let available=components::docked_section_body_height(remaining)
                        .min(ui.available_height().max(0.0));
                    let list_height=(available
                        -theme::RECEPTION_TOTAL_HEIGHT
                        -theme::RECEPTION_VAT_ROW_HEIGHT
                        -2.0*theme::SPACE_4).max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(),list_height),
                        Layout::top_down(Align::Min),
                        |list_area|{
                            list_area.set_min_height(list_height);
                            egui::ScrollArea::vertical()
                                .scroll_bar_visibility(
                                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                                )
                                .id_salt("recognized-items")
                                .max_height(list_height)
                                .auto_shrink([false,false])
                                .show(list_area,|items_ui|{
                                    items_ui.spacing_mut().item_spacing.y=0.0;
                                    components::reception_items_header(items_ui);
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
                                                // The egui working-reception acknowledgement
                                                // currently supplies unitPrice, not a computed
                                                // line amount. Do not invent an invoice amount.
                                                let amount=preview.and_then(|quote|
                                                    quote.get("linePrices"))
                                                    .and_then(serde_json::Value::as_array)
                                                    .and_then(|rows|rows.iter().find(|row|
                                                        row.get("itemId")
                                                            .and_then(serde_json::Value::as_str)
                                                            ==Some(item_id)))
                                                    .and_then(|row|row.get("amount"))
                                                    .and_then(serde_json::Value::as_i64)
                                                    .map(components::display_won)
                                                    .unwrap_or_else(||"—".to_owned());
                                                let cells=components::reception_item_row(
                                                    items_ui,
                                                    &reception_service::item_title(item),
                                                    &quantity,&amount,
                                                );
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
                                                    source_highlight::paint_active(
                                                        items_ui,response,&key,
                                                        self.link_hover.active.as_ref(),
                                                    );
                                                }
                                                if let Some(issues)=item.get("issues")
                                                    .and_then(serde_json::Value::as_array)
                                                {
                                                    for issue in issues.iter().take(3){
                                                        if let Some(message)=issue.as_str(){
                                                            items_ui.label(typography::status(
                                                                message,theme::SELECTED_ORDER
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        items_ui.add_space(theme::SPACE_8);
                                        // The saved quote's shippingFee is an ORDER
                                        // total, not a per-recipient applied freight.
                                        // Do not mislabel it as this recipient's fee.
                                        let remote=recipient.get("remoteDelivery")
                                            .and_then(|v|v.get("checked"))
                                            .and_then(serde_json::Value::as_bool);
                                        let origin=recipient.get("dispatchOrigin")
                                            .and_then(serde_json::Value::as_str);
                                        components::reception_freight_row(
                                            items_ui,"운임",None,None,remote,
                                            origin==Some("FACTORY"),
                                        );
                                        items_ui.add_space(theme::SPACE_8);
                                        components::reception_item_actions(items_ui,origin);
                                    } else {
                                        items_ui.add_space(theme::SPACE_8);
                                        items_ui.label(typography::caption(
                                            "거래처 선택 후 운임과 청구금액을 확인합니다."
                                        ));
                                    }
                                });
                        }
                    );
                    ui.add_space(theme::SPACE_4);
                    // A working reception is not a confirmed bill. Its quote
                    // has no authoritative VAT-included flag at this point.
                    components::reception_vat_row(ui,None);
                    ui.add_space(theme::SPACE_4);
                    let synced=self.active_id.as_deref()
                        .and_then(|id|self.drafts.iter().find(|row|row.id==id))
                        .is_some_and(|row|row.working_synced);
                    let note=if synced {
                        "Rust 참고 금액 · 운영 미확정 작업본 저장됨"
                    } else if preview.is_some() {
                        "Rust 재계산 참고 · 운영 작업본 저장 전"
                    } else { "" };
                    let total=preview.and_then(|v|v.get("total"))
                        .and_then(serde_json::Value::as_i64);
                    // The native working preview does not yet project
                    // shipmentCount or vatLabel. Keep them unknown.
                    components::reception_total_footer(ui,total,None,"—",note);
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
            // A continuous order-card list needs the same 10pt solid
            // scrollbar that management lists use, never a floating thumb.
            ui.spacing_mut().scroll=theme::management_table_scroll_style();
            egui::ScrollArea::vertical()
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
                    event=workflow_board_ui::show(
                        ui,self.workflow_snapshot.as_ref(),&self.drafts,
                        self.active_id.as_deref(),
                        &self.client_names,active_selected,
                        &mut self.workflow_ui
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
        components::modal_backdrop(ctx,"freight-global-leave-backdrop",
            manager.has_rate_modal());
        components::decision_window(
            "저장하지 않은 변경사항","freight-global-leave",work_area
        ).show(ctx,|dialog|{
            if components::modal_header(
                dialog,"저장하지 않은 변경사항","취소",!manager.busy()
            ){
                self.freight_pending_page=None;
                self.freight_pending_save=false;
            }
            dialog.label(typography::body(
                "운임 관리에서 수정한 내용이 있습니다. 변경사항을 저장할까요?"
            ));

            dialog.add_space(theme::SPACE_12);
            components::modal_action_row(dialog,|bar|{
                if theme::action(bar,"저장하고 이동",ButtonRole::Primary,
                    manager.can_save_dirty()).clicked(){
                    if manager.save_dirty(ctx.clone()){
                        self.freight_pending_save=true;
                    }
                }
                if theme::action(bar,"저장 안 함",ButtonRole::Standard,
                    !manager.busy()).clicked(){
                    manager.discard_drafts();
                    self.page=next;
                    self.freight_pending_page=None;
                    self.freight_pending_save=false;
                }
            });
        });
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
        components::modal_backdrop(ctx,"settings-page-leave-backdrop",false);
        components::decision_window(
            "저장하지 않은 변경사항","settings-global-leave",work_area
        ).show(ctx,|ui|{
            if components::modal_header(
                ui,"저장하지 않은 변경사항","취소",!view.busy()
            ){self.settings_pending_page=None;}
            ui.label(typography::body(
                "설정에서 수정한 내용이 있습니다. 변경사항을 저장할까요?"
            ));
            ui.add_space(theme::SPACE_12);
            components::modal_action_row(ui,|actions|{
                if theme::action(actions,"저장하고 이동",ButtonRole::Primary,
                    !view.busy()&&!view.read_only()).clicked(){
                    self.settings_pending_save=true;
                    view.save(Arc::clone(&self.gateway),ctx.clone());
                }
                if theme::action(actions,"저장 안 함",ButtonRole::Standard,
                    !view.busy()).clicked(){
                    view.discard();
                    self.page=next;
                    self.settings_pending_page=None;
                }
            });
        });
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
                            UiBuilder::new().id_salt(("management-page",self.page.label()))
                                .max_rect(right).layout(Layout::top_down(Align::Min)),
                            |main|{
                                main.set_clip_rect(right);
                                main.set_width(right.width());
                                main.set_height(right.height());
                                // The shared workspace frame already owns the
                                // outer top/right/bottom 6pt gutters. Do not
                                // add a second 16pt page margin: management
                                // cards should end on the same gutter line as
                                // the reception panes. Individual section
                                // frames retain their own content padding.
                                Frame::new().inner_margin(0).show(main,|page|{
                                    match self.page{
                                        Page::Excel=>{
                                            if let Some(excel)=self.excel_manager.as_mut(){
                                                excel.show_page(page);
                                                if let Some((order_id,order_date))=excel.take_open_order(){
                                                    self.page=Page::History;
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
                                                if excel.take_open_settings(){
                                                    self.page=Page::Settings;
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
        let frame_started=Instant::now();
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
        self.poll_source_analysis(ctx);
        self.poll_auto_client_change(ctx);
        if self.settings_view.as_mut().is_some_and(|view|view.take_restored()){
            // A restored operational DB invalidates every prior projected
            // price/client/order snapshot. Never keep stale native matches.
            self.workflow_snapshot=None;
            self.workflow_last_fetch=None;
            self.workflow_ui.selection=None;
            self.workflow_ui.stop_preview();
            self.client_manager=None;
            self.freight_manager=None;
            self.order_history_manager=None;
            self.client_chooser=None;
            self.working_sync=None;
            self.client_change=None;
            self.auto_client_change=None;
            // Invalidate deletion replies from a different restored DB/catalog.
            self.source_clear_confirm=None;
            self.source_clear_job=None;
            self.clear_source_analysis();
        }
        self.poll_workflow_board(ctx);
        self.poll_client_names(ctx);
        self.poll_client_change(ctx);
        self.poll_working_sync(ctx);
        self.poll_source_clear(ctx);
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
            self.auto_client_change=None;
            self.analysis_due_at=Some(Instant::now());
        }
        if self.page == Page::Reception {
            self.kakao_tracker.refresh();
            ctx.request_repaint_after(Duration::from_millis(450));
            if !self.source_editing && self.raw_source.is_empty() {
                // Clipboard intake actions are visible only in this state.
                // Poll the cheap Win32 change counter promptly so clearing
                // text/image disables both buttons without a stale frame.
                self.clipboard_monitor.refresh();
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if self.analysis_due_at.is_some_and(|at| Instant::now() >= at) {
            self.analysis_due_at = None;
            if !self.raw_source.trim().is_empty() {
                self.start_source_analysis(ctx.clone());
            }
        }
        self.top_bar(ctx);
        self.poll_inbox_folder(ctx);
        if self.page == Page::Reception {
            self.reception(ctx);
        } else {
            self.other_page(ctx);
        }
        let work_area=self.modal_work_area(ctx);
        if self.ocr_review.is_some() || self.pending_replace.is_some() || self.source_clear_confirm.is_some() ||
            (self.page==Page::Reception &&
                (self.client_chooser.is_some() || self.manual_workspace.is_some())) {
            components::modal_backdrop(
                ctx,"bando-workspace-modal-backdrop",false
            );
        }
        self.show_ocr_review(ctx,work_area);
        self.show_replacement_confirmation(ctx,work_area);
        self.show_source_clear_confirmation(ctx,work_area);
        if self.page == Page::Reception {
            self.show_client_chooser(ctx,work_area);
            self.show_manual_workspace(ctx,work_area);
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
                }else if manager.has_modal(){
                    components::modal_backdrop(
                        ctx,"client-manager-modal-backdrop",false
                    );
                    manager.show_editor(ctx,work_area,Arc::clone(&self.gateway));
                }
            }
        }
        self.frameless_resize(ctx);
        // Tooltip-order overlay is drawn LAST, on top of the current modal,
        // without allocating space in any card, row or scrollbar viewport.
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

fn main() -> eframe::Result<()> {
    // Connect to the EXISTING Tauri v2 database before creating any gateway
    // or background worker. Debug uses only the developer-test DB, release
    // uses only the production DB. This never creates an operating DB.
    match business_state::bind_existing_tauri_database() {
        Ok(Some(_)) => {},
        Ok(None) => {},
        Err(error) => {
            eprintln!("[BANDO HUB] 기존 Tauri 업무 DB 자동 연결 실패: {error}");
            std::process::exit(2);
        }
    }
    // Only the explicitly opted-in Windows developer launcher appends
    // synthetic UI-density fixtures to the existing isolated TEST DB.
    // It never touches an operating DB or launches a save-helper process.
    if let Err(error) = developer_ui_samples::seed_if_requested() {
        eprintln!("[BANDO HUB TEST] 가상 UI 데이터 준비 실패: {error}");
        std::process::exit(2);
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
