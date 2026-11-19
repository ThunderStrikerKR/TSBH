//! BANDO HUB egui — first native visual implementation.
//! UI preview only: uses existing read-only Rust business decisions and
//! a separate egui-preview.sqlite3 for source continuity. It NEVER opens the
//! production Tauri database or docks KakaoTalk. Existing React/Tauri stays.

mod theme;
mod layout;
mod components;
mod reception_service;
mod gateway;
mod http_gateway;
mod preview_store;
mod source_autosave;

use eframe::egui::{self, Align, Frame, Layout, RichText, Rect, Sense, Ui, UiBuilder, Vec2};
use theme::ButtonRole;
use std::sync::{mpsc::{self, Receiver}, Arc};
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
    source_editing: bool,
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
            source_editing: false,
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
        if let Some(worker) = &self.source_autosave {
            if let Err(error) = worker.queue(self.raw_source.clone()) {
                self.source_save_error = Some(error);
            }
        } else {
            self.source_save_error = Some("격리 원문 저장소가 초기화되지 않았습니다.".to_owned());
        }
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
                    self.notifications_open = false;
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
        let (sender, receiver) = mpsc::channel();
        self.analysis_receiver = Some(receiver);
        std::thread::spawn(move || {
            let result = reception_service::analyze_raw(gateway.as_ref(), &source);
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
                        if let Some(texture) = &self.brand_texture {
                            bar.add(egui::Image::new((
                                texture.id(),
                                Vec2::splat(theme::BRAND_ICON_SIZE),
                            )));
                        }
                        bar.label(
                            RichText::new("BANDO HUB")
                                .size(theme::FONT_BRAND).strong().color(theme::TEXT)
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
                            if theme::action(actions, "×", ButtonRole::Quiet, true)
                                .on_hover_text("닫기").clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if theme::action(actions, "□", ButtonRole::Quiet, true)
                                .on_hover_text("창 최대화 또는 복원").clicked()
                            {
                                ctx.send_viewport_cmd(
                                    egui::ViewportCommand::Maximized(!maximized)
                                );
                            }
                            if theme::action(actions, "−", ButtonRole::Quiet, true)
                                .on_hover_text("최소화").clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                            if theme::action(
                                actions, "알림", ButtonRole::Standard,
                                self.source_save_error.is_some(),
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

        if self.notifications_open && self.source_save_error.is_some() {
            egui::Window::new("알림")
                .id(egui::Id::new("native_ui_preview_notifications"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if let Some(error) = &self.source_save_error {
                        ui.colored_label(theme::DANGER, error);
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
                    header.label(theme::section_heading(title));
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
                                components::paint_dock_splitter(
                                    ui, splitter, true,
                                    response.hovered() || response.dragged(),
                                );
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
        Self::section_with_header(ui, "발주 원문", height, |ui| {
            theme::action(ui, "+ 새 주문", ButtonRole::Emphasized, false);
        }, |ui| {
            if self.source_editing || !self.raw_source.is_empty() {
                let editor_height = (ui.available_height() - theme::SECTION_CONTENT_GAP).max(120.0);
                let editor = ui.add_sized(
                    Vec2::new(ui.available_width(), editor_height),
                    egui::TextEdit::multiline(&mut self.raw_source)
                        .font(egui::TextStyle::Body),
                );
                if editor.changed() {
                    self.clear_source_analysis();
                    self.queue_source_save();
                    if !self.raw_source.trim().is_empty() {
                        self.analysis_due_at = Some(Instant::now() + Duration::from_millis(250));
                        ui.ctx().request_repaint_after(Duration::from_millis(260));
                    }
                }
            } else {
                ui.add_space(theme::SPACE_24);
                ui.label(theme::subsection_heading("클립보드 접수"));
                ui.label(theme::caption("복사한 텍스트와 이미지를 자동으로 구분해 인식합니다."));
                ui.horizontal(|ui| {
                    theme::action(ui, "대리발송", ButtonRole::Emphasized, false);
                    theme::action(ui, "대리점 납품", ButtonRole::Emphasized, false);
                });
                ui.label(theme::caption("이미지 캡처는 Ctrl + Shift + S 또는 Print Screen으로 복사하세요."));
                components::soft_divider(ui);
                ui.add_space(theme::SPACE_8);
                ui.label(theme::subsection_heading("다른 방법으로 접수"));
                ui.horizontal(|ui| {
                    if theme::action(ui, "직접 입력", ButtonRole::Emphasized, true).clicked() {
                        begin_input = true;
                    }
                    theme::action(ui, "캡처 범위 지정", ButtonRole::Emphasized, false);
                });
                components::soft_divider(ui);
                ui.add_space(theme::SPACE_8);
                ui.label(theme::subsection_heading("폴더 자동 인식"));
                ui.label(theme::caption("설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요."));
            }
        });
        if begin_input {
            self.source_editing = true;
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
            let bottom_controls =
                theme::ACTION_STANDARD_HEIGHT + theme::SPACE_8;
            let field_height = (ui.available_height() - bottom_controls).max(48.0);
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
            ui.horizontal(|ui| {
                theme::action(ui, "그대로 복사", ButtonRole::Standard, false);
                theme::action(ui, "인사말 포함 복사", ButtonRole::Standard, false);
            });
        });
    }

    fn details_and_freight(&mut self, ui: &mut Ui) {
        let full_height = ui.available_height();
        // The center-line between the second and third columns is shared.
        // Independent 27% + 22% measurements used to put the lower third
        // column divider 1–2 physical pixels above the source/message divider.
        let (shared_upper_height, _) = self.layout.vertical_heights(full_height);
        let upper_budget = (shared_upper_height - layout::SPLITTER).max(0.0);
        let review_height = if upper_budget >= 330.0 {
            (full_height * 0.27).clamp(150.0, upper_budget - 180.0)
        } else {
            // Keep the boundary aligned on smaller windows even when the
            // preferred per-section minimum heights cannot both be met.
            upper_budget * (150.0 / 330.0)
        };
        let recipient_height = (upper_budget - review_height).max(0.0);
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
                ui.colored_label(theme::DANGER, error);
            } else if let Some(analysis) = &self.analysis {
                let unidentified = analysis.get("unassigned")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len).unwrap_or(0);
                let candidates = analysis.get("candidates")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len).unwrap_or(0);
                if unidentified > 0 || candidates > 0 {
                    ui.colored_label(theme::SELECTED_ORDER,
                        format!("미분류 {unidentified}건 · 확인 후보 {candidates}건"));
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
                    ui.colored_label(theme::SELECTED_ORDER,
                        format!("상품 확인 {issue_count}건"));
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
                                            ui.label(theme::body(
                                                &reception_service::item_title(item)
                                            ));
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(theme::caption(
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
                                                    ui.colored_label(
                                                        theme::SELECTED_ORDER, message
                                                    );
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
        // Same section frame, header inset and divider as the other panes.
        // Do not replace an empty order list with invented placeholder text.
        let height = ui.available_height();
        Self::section(ui, "처리 중인 주문", height, |_| {});
    }

    fn other_page(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(theme::SPACE_16))
            .show(ctx, |ui| {
                ui.label(RichText::new(self.page.label()).size(theme::FONT_PAGE_HEADING).strong());
            });
    }
}

impl eframe::App for BandoEgui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_source_analysis();
        self.poll_source_saves();
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
            let brand_texture = cc.egui_ctx.load_texture(
                "bando-original-brand",
                brand_image,
                egui::TextureOptions::LINEAR,
            );
            let autosave = source_autosave::SourceAutosave::new(
                Arc::clone(&backend), restored.revision, cc.egui_ctx.clone()
            );
            let source_editing = !restored.raw.is_empty();
            if source_editing { cc.egui_ctx.request_repaint_after(Duration::from_millis(260)); }
            Ok(Box::new(BandoEgui {
                brand_texture: Some(brand_texture),
                gateway: backend,
                raw_source: restored.raw,
                source_editing,
                source_autosave: Some(autosave),
                analysis_due_at: if source_editing {
                    Some(Instant::now() + Duration::from_millis(250))
                } else { None },
                ..BandoEgui::default()
            }))
        }),
    )
}
