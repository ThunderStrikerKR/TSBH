//! BANDO HUB egui — first native visual implementation.
//! UI preview only: uses existing read-only Rust business decisions and
//! a separate egui-preview.sqlite3 for source continuity. It NEVER opens the
//! production Tauri database or docks KakaoTalk. Existing React/Tauri stays.

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
mod kakao_context;

use eframe::egui::{self, Align, Frame, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use theme::{ButtonRole, WindowControl};
use clipboard_intake::{ClipboardImage, ClipboardMonitor, ClipboardPayload, OrderType};
use preview_store::{WorkingReception, WorkingSource};
use kakao_context::{ChatWindow, KakaoTracker};
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
    kakao_connected: bool,
    kakao_choice: Option<String>,
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
    analysis: Option<reception_service::ReceptionAnalysis>,
    analysis_error: Option<String>,
    selected_recipient: usize,
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
            kakao_connected: false,
            kakao_choice: None,
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
            analysis: None,
            analysis_error: None,
            selected_recipient: 0,
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
            if active.raw != self.raw_source {
                active.analysis_raw = None;
                active.analysis_evidence = None;
                active.client_match = None;
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
            client_id: None, client_selection: None,
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
            draft.client_selection.as_deref() == Some("MANUAL")
        ).and_then(|draft| draft.client_id.clone());
        let (sender, receiver) = mpsc::channel();
        self.analysis_receiver = Some(receiver);
        std::thread::spawn(move || {
            let result = reception_service::analyze_raw(
                gateway.as_ref(), &source, windows, active_chat_handle, manual_client_id
            );
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
                            if item.client_selection.as_deref() != Some("MANUAL") {
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

    fn accept_clipboard_text(&mut self, text: String, order_type: OrderType, ctx: &egui::Context) {
        if text.trim().is_empty() {
            self.clipboard_error = Some("발주 원문이 비어 있습니다.".into());
            return;
        }
        if self.raw_source != text && !self.raw_source.trim().is_empty() {
            // Exact replacement confirmation from the existing React intake.
            self.pending_replace = Some(PendingReplace { text, order_type });
            return;
        }
        self.apply_intake_text(text, order_type, ctx);
    }

    fn apply_intake_text(&mut self, text: String, order_type: OrderType, ctx: &egui::Context) {
        if text.trim().is_empty() { return; }
        if self.active_id.is_none() {
            self.new_reception(ctx);
        }
        let changed = self.raw_source != text;
        self.raw_source = text;
        self.order_type = order_type;
        self.source_editing = true;
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
                if self.clipboard_cancel.swap(false, Ordering::AcqRel) {
                    // User cancellation wins even if a completed OCR result
                    // raced with the Cancel click. Never open the review.
                    return;
                }
                match result {
                    Ok(ClipboardReceipt::Text(text)) => {
                        self.accept_clipboard_text(text, order_type, ctx);
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
                        });
                    }
                    Err(error) if error == "OCR_CANCELLED" => {}
                    Err(error) => {
                        self.clipboard_error = Some(error);
                        self.notifications_open = true;
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.clipboard_receiver = None;
                self.clipboard_busy = false;
                self.ocr_processing = false;
                if self.clipboard_cancel.swap(false, Ordering::AcqRel) {
                    return;
                }
                self.clipboard_error = Some("클립보드 접수 작업이 중단되었습니다.".into());
                self.notifications_open = true;
            }
            _ => {}
        }
    }

    /// The same OCR review controls that React exposes: inspect the image,
    /// correct recognized text, append or replace. No OCR result is committed
    /// to SQLite or sent to the Rust business engine before acceptance.
    fn show_ocr_review(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut apply: Option<(String, OrderType, bool)> = None;
        if let Some(review) = self.ocr_review.as_mut() {
            let mut open = true;
            egui::Window::new("OCR 문자 인식 검토")
                .id(egui::Id::new("egui-ocr-review"))
                .open(&mut open)
                .collapsible(false)
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
                            apply = Some((appended, review.order_type, false));
                        }
                        if theme::action(ui, "원문 교체", ButtonRole::Primary, can_apply).clicked() {
                            apply = Some((
                                review.edited_text.trim().to_owned(),
                                review.order_type, true
                            ));
                        }
                    });
                });
            if !open { close = true; }
        }
        if let Some((raw, kind, replacing)) = apply {
            self.ocr_review = None;
            self.discard_ocr_confirm = false;
            if replacing {
                self.accept_clipboard_text(raw, kind, ctx);
            } else {
                // Append preserves the original React behavior: no redundant
                // replacement confirmation because the old source remains.
                self.apply_intake_text(raw, kind, ctx);
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
            egui::Window::new("변경사항 확인")
                .id(egui::Id::new("ocr-review-discard-confirm"))
                .collapsible(false).resizable(false)
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

    fn show_replacement_confirmation(&mut self, ctx: &egui::Context) {
        if self.pending_replace.is_none() { return; }
        let mut accept = false;
        let mut cancel = false;
        egui::Window::new("발주원문 교체 확인")
            .id(egui::Id::new("egui-replace-source-confirm"))
            .collapsible(false).resizable(false)
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
                self.apply_intake_text(next.text, next.order_type, ctx);
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
                                    brand_slot.center().y,
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
                                self.page = page;
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
        // A pane must occupy the exact rectangle allocated by the 4-column
        // splitter. The previous content-driven Frame ended too early.
        let pane_width = ui.available_width();
        let pane_height = ui.available_height();
        theme::docked_pane_frame().show(ui, |ui| {
            let frame_inset = 2.0 * theme::SECTION_PADDING;
            let inner_width = (pane_width - frame_inset).max(1.0);
            let inner_height = (pane_height - frame_inset).max(1.0);
            ui.set_min_size(Vec2::new(inner_width, inner_height));
            ui.vertical_centered(|ui| {
                ui.add_space(
                    ((inner_height - theme::ACTION_EMPHASIZED_HEIGHT) / 2.0).max(0.0)
                );
                theme::action(ui, "카카오톡 연결", ButtonRole::Emphasized, false);
            });
        });
    }

    fn reception(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(theme::PANEL_GAP))
            .show(ctx, |ui| {
                let viewport_width = ui.available_width();
                let content_width = viewport_width.max(layout::LayoutState::minimum_width());
                let content_height = ui.available_height().max(1.0);

                // Exactly four column rectangles are allocated before drawing
                // any business content. Hover/folding a card cannot shift the
                // coordinates of splitters or other panels.
                egui::ScrollArea::horizontal()
                    // The parent scroll area's touch-style drag handling
                    // must not compete with a draggable pane separator.
                    .drag_to_scroll(false)
                    .id_salt("reception_four_columns")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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
                    });
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

    fn source_section(&mut self, ui: &mut Ui, height: f32) {
        let mut begin_input = false;
        let mut begin_new_order = false;
        let mut start_clipboard = None;
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
                // The source editor receives only the space left after this
                // section's title, divider, gaps and padding. The old
                // .max(120.0) could force a larger control than its pane.
                // Longer originals scroll INSIDE this fixed viewport instead
                // of enlarging the source section into automatic messages.
                let editor_height = components::docked_section_body_height(height)
                    .min(ui.available_height().max(0.0));
                if editor_height > 0.0 {
                    let viewport = Rect::from_min_size(
                        ui.available_rect_before_wrap().min,
                        Vec2::new(ui.available_width(), editor_height),
                    );
                    let clip = ui.clip_rect().intersect(viewport);
                    let editor = ui.scope(|editor_ui| {
                        editor_ui.set_clip_rect(clip);
                        egui::ScrollArea::vertical()
                            .id_salt("source-editor-vertical-scroll")
                            .max_height(editor_height)
                            .auto_shrink([false, false])
                            .show(editor_ui, |scroll_ui| {
                                scroll_ui.add_sized(
                                    Vec2::new(scroll_ui.available_width(), editor_height),
                                    egui::TextEdit::multiline(&mut self.raw_source)
                                        .font(egui::TextStyle::Body),
                                )
                            })
                            .inner
                    }).inner;
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
            } else {
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

        ui.scope_builder(
            UiBuilder::new().id_salt("details-review").max_rect(review_rect)
                .layout(Layout::top_down(Align::Min)),
            |pane| {
                pane.set_clip_rect(review_rect);
                pane.set_width(review_rect.width());
                pane.set_height(review_rect.height());
                Self::section(pane, "확인할 내용", review_height, |ui| {
                    if let Some(error) = &self.analysis_error {
                        ui.label(typography::status(error, theme::DANGER));
                    } else if let Some(analysis) = &self.analysis {
                        if analysis.directory_count == 0 {
                            ui.label(typography::status(
                                "거래처 명단 미연결 · 자동 매칭 확인 필요",
                                theme::SELECTED_ORDER,
                            ));
                        } else if let Some(name) = &analysis.matched_client_name {
                            ui.label(typography::body(
                                &format!("거래처 자동 인식 · {name}")
                            ));
                        } else if reception_service::text(&analysis.client_match, "source") == "CONFLICT" {
                            ui.label(typography::status(
                                reception_service::text(&analysis.client_match, "detail"),
                                theme::DANGER,
                            ));
                        } else {
                            ui.label(typography::status(
                                "거래처 확인 필요", theme::SELECTED_ORDER
                            ));
                            if !analysis.client_candidates.is_empty() {
                                ui.label(typography::caption(&format!(
                                    "거래처 후보 {}건 · 자동 적용하지 않음",
                                    analysis.client_candidates.len()
                                )));
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
            },
        );
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
                            components::labeled_field(ui, "이름", &mut name, components::FieldKind::Text, false);
                            components::labeled_field(ui, "주소", &mut address, components::FieldKind::Address, false);
                            components::labeled_field(ui, "휴대전화", &mut mobile, components::FieldKind::Telephone, false);
                            if !phone.trim().is_empty() {
                                components::labeled_field(ui, "전화", &mut phone, components::FieldKind::Telephone, false);
                            }
                            components::labeled_field(ui, "요청 메시지", &mut note, components::FieldKind::Text, false);
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
                                                ui.horizontal(|ui| {
                                                    ui.label(typography::body(
                                                        &reception_service::item_title(item)
                                                    ));
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            ui.label(typography::quantity(
                                                                &reception_service::item_quantity(item)
                                                            ));
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
                    components::summary_strip(ui, "총 주문", "—", "");
                });
            },
        );
    }

    fn orders_pane(&mut self, ui: &mut Ui) {
        let height = ui.available_height();
        let active = self.active_id.clone();
        let rows: Vec<(String, String)> = self.drafts.iter().rev().map(|item| {
            let kind = if item.order_type == "DEALER" { "대리점 납품" } else { "대리발송" };
            let title = item.raw.lines().find(|line| !line.trim().is_empty())
                .unwrap_or("새 발주서").trim();
            let excerpt: String = title.chars().take(40).collect();
            (item.id.clone(), format!("{kind} · {excerpt}"))
        }).collect();
        let mut select = None;
        Self::section(ui, "처리 중인 주문", height, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("working-receptions-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (id, title) in &rows {
                        let role = if active.as_deref() == Some(id.as_str()) {
                            ButtonRole::ActiveNavigation
                        } else {
                            ButtonRole::Quiet
                        };
                        if theme::action(ui, title, role, true).clicked() {
                            select = Some(id.clone());
                        }
                        components::soft_divider(ui);
                    }
                });
        });
        if let Some(id) = select {
            self.select_reception(&id, ui.ctx());
        }
    }

    fn other_page(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(theme::SPACE_16))
            .show(ctx, |ui| {
                ui.label(typography::page_heading(self.page.label()));
            });
    }
}

impl eframe::App for BandoEgui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_source_analysis();
        self.poll_source_saves();
        self.poll_clipboard_intake(ctx);
        if self.page == Page::Reception && !self.source_editing && self.raw_source.is_empty() {
            self.clipboard_monitor.refresh();
            ctx.request_repaint_after(Duration::from_millis(450));
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
        self.show_ocr_review(ctx);
        self.show_replacement_confirmation(ctx);
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
            .with_min_inner_size([1150.0, 720.0])
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
