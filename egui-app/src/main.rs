//! BANDO HUB egui — first native visual implementation.
//! UI preview only: it does not open production SQLite, call Rust business
//! commands, or dock KakaoTalk. Existing React + Tauri remains untouched.

mod theme;
mod layout;
mod components;
mod native_engine;

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, Rect, Sense, Stroke, Ui, UiBuilder, Vec2};
use theme::{ButtonRole, PANEL_GAP};
use std::sync::mpsc::{self, Receiver};

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
    page: Page,
    sample: bool,
    notifications_open: bool,
    order_expanded: [bool; 2],
    layout: layout::LayoutState,
    dragging_split: Option<HorizontalSplitDrag>,
    dragging_vertical: Option<VerticalSplitDrag>,
    raw_source: String,
    source_editing: bool,
    analysis: Option<serde_json::Value>,
    analysis_error: Option<String>,
    selected_recipient: usize,
    analysis_request: u64,
    analysis_pending: bool,
    analysis_receiver: Option<Receiver<(u64, Result<serde_json::Value, String>)>>,
}

impl Default for BandoEgui {
    fn default() -> Self {
        Self {
            page: Page::Reception,
            sample: false,
            notifications_open: false,
            order_expanded: [true, false],
            layout: layout::LayoutState::load(),
            dragging_split: None,
            dragging_vertical: None,
            raw_source: String::new(),
            source_editing: false,
            analysis: None,
            analysis_error: None,
            selected_recipient: 0,
            analysis_request: 0,
            analysis_pending: false,
            analysis_receiver: None,
        }
    }
}

impl BandoEgui {
    // Run parsing outside the egui frame loop. Any changed source invalidates
    // the previous result and its request sequence, so stale replies are ignored.
    fn start_source_analysis(&mut self, context: egui::Context) {
        self.analysis_request = self.analysis_request.wrapping_add(1);
        let request = self.analysis_request;
        let source = self.raw_source.clone();
        self.analysis = None;
        self.analysis_error = None;
        self.analysis_pending = true;
        let (sender, receiver) = mpsc::channel();
        self.analysis_receiver = Some(receiver);
        std::thread::spawn(move || {
            let result = native_engine::analyze_raw(&source);
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

    fn top_bar(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        egui::TopBottomPanel::top("global_navigation")
            .exact_height(theme::HEADER_HEIGHT)
            .frame(Frame::new().fill(Color32::from_rgb(27, 27, 27)).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    // Frameless window: the brand acts as the title-bar drag
                    // region. Buttons/tabs remain fully interactive.
                    let brand = ui.add(
                        egui::Label::new(
                            RichText::new("BANDO HUB").size(20.0).strong().color(theme::TEXT)
                        ).sense(Sense::click_and_drag())
                    );
                    if brand.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if brand.double_clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    let caption = ui.add(
                        egui::Label::new(
                            RichText::new("/ UI 프리뷰").size(11.0).color(theme::MUTED)
                        ).sense(Sense::click_and_drag())
                    );
                    if caption.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    ui.add_space(12.0);
                    for page in Page::ALL {
                        let role = if self.page == page {
                            ButtonRole::ActiveNavigation
                        } else {
                            ButtonRole::Navigation
                        };
                        if theme::action(ui, page.label(), role, true).clicked() {
                            self.page = page;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::action(ui, "×", ButtonRole::Quiet, true)
                            .on_hover_text("닫기").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if theme::action(
                            ui, if maximized { "❐" } else { "□" },
                            ButtonRole::Quiet, true
                        ).on_hover_text(if maximized { "이전 크기" } else { "최대화" }).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }
                        if theme::action(ui, "−", ButtonRole::Quiet, true)
                            .on_hover_text("최소화").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        if theme::action(ui, "알림", ButtonRole::Standard, true).clicked() {
                            self.notifications_open = !self.notifications_open;
                        }
                        ui.label(
                            RichText::new("TEST / 격리 UI · DB 미연결")
                                .size(11.0).color(theme::SELECTED_ORDER)
                        );
                    });
                });
            });

        if self.notifications_open {
            egui::Window::new("알림")
                .id(egui::Id::new("native_ui_preview_notifications"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(theme::body("현재는 UI 프리뷰입니다. 알림 데이터베이스에 연결되지 않았습니다."));
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
        theme::section_frame().show(ui, |ui| {
            ui.set_min_width((width - 2.0 * theme::SECTION_PADDING).max(60.0));
            ui.set_min_height((minimum_height - 2.0 * theme::SECTION_PADDING).max(48.0));
            // All section headers occupy the same height whether or not
            // they contain an action. Prevent controls from moving when
            // the section switches between empty and populated states.
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), theme::SECTION_HEADER_HEIGHT),
                Layout::left_to_right(Align::Center),
                |header| {
                    header.set_min_height(theme::SECTION_HEADER_HEIGHT);
                    header.label(theme::section_heading(title));
                    header.with_layout(Layout::right_to_left(Align::Center), header_right);
                },
            );
            components::soft_divider(ui);
            ui.add_space(theme::SECTION_CONTENT_GAP);
            content(ui);
        });
    }

    fn kakao_pane(&mut self, ui: &mut Ui) {
        Frame::new()
            .fill(theme::PANEL)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_min_height((ui.available_height() - 20.0).max(100.0));
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.44).max(20.0));
                    theme::action(ui, "카카오톡 연결", ButtonRole::Emphasized, false)
                        .on_disabled_hover_text("카카오톡 Windows 도킹은 아직 UI 프리뷰에 연결되지 않았습니다.");
                });
            });
    }

    fn reception(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(6.0))
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
                                ui.painter().rect_filled(
                                    splitter, 0.0,
                                    if response.hovered() || response.dragged() {
                                        theme::BORDER
                                    } else {
                                        theme::DIVIDER
                                    },
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
        ui.painter().rect_filled(
            splitter_rect, 0.0,
            if response.hovered() || response.dragged() { theme::BORDER } else { theme::DIVIDER },
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
        let mut new_reception = false;
        let mut begin_input = false;
        let mut analyze_requested = false;
        Self::section_with_header(ui, "발주 원문", height, |ui| {
            if theme::action(ui, "+ 새 주문", ButtonRole::Emphasized, true)
                .on_hover_text("프리뷰: 입력한 원문과 분석 결과만 초기화합니다.")
                .clicked() {
                new_reception = true;
            }
        }, |ui| {
            if self.source_editing || !self.raw_source.is_empty() {
                ui.label(theme::caption("직접 입력 · 실제 Rust 인식 엔진 · 기본 상품 시드"));
                let editor_height = (ui.available_height() - 62.0).max(120.0);
                let editor = ui.add_sized(
                    Vec2::new(ui.available_width(), editor_height),
                    egui::TextEdit::multiline(&mut self.raw_source)
                        .font(egui::TextStyle::Body)
                        .hint_text("발주서 원문을 여기에 입력하거나 붙여넣으세요."),
                );
                if editor.changed() {
                    self.clear_source_analysis();
                }
                ui.horizontal(|ui| {
                    if theme::action(
                        ui,
                        if self.analysis_pending { "분석 중…" } else { "원문 분석" },
                        ButtonRole::Emphasized,
                        !self.analysis_pending && !self.raw_source.trim().is_empty(),
                    ).clicked() {
                        analyze_requested = true;
                    }
                    ui.label(theme::caption("기존 Rust 인식 엔진 · 운영 DB 미사용"));
                });
            } else if self.sample {
                theme::nested_frame().show(ui, |ui| {
                    ui.label(RichText::new("현재 거래처").size(12.0).color(theme::SECONDARY));
                    ui.label(RichText::new("샘플거래처").size(14.0).strong());
                });
                ui.add_space(8.0);
                theme::nested_frame().show(ui, |ui| {
                    ui.label(theme::body("반도 709 민판 1개"));
                    ui.label(theme::body("수령자: 샘플수취인 / 010-0000-0000"));
                    ui.label(theme::body("주소: 샘플시 샘플로 123"));
                    ui.add_space(8.0);
                    ui.label(theme::caption("실제 발주서가 아닌 UI 배치 확인용 예시입니다."));
                });
                if theme::action(ui, "실제 원문 입력", ButtonRole::Emphasized, true).clicked() {
                    begin_input = true;
                }
            } else {
                ui.add_space(32.0);
                ui.label(theme::subsection_heading("클립보드 접수"));
                ui.label(theme::caption("복사한 텍스트와 이미지를 자동으로 구분해 인식합니다."));
                ui.horizontal(|ui| {
                    theme::action(ui, "대리발송", ButtonRole::Emphasized, false)
                        .on_disabled_hover_text("클립보드 자동 접수는 아직 연결되지 않았습니다.");
                    theme::action(ui, "대리점 납품", ButtonRole::Emphasized, false);
                });
                ui.label(theme::caption("이미지 캡처는 Ctrl + Shift + S 또는 Print Screen으로 복사하세요."));
                components::soft_divider(ui);
                ui.add_space(8.0);
                ui.label(theme::subsection_heading("다른 방법으로 접수"));
                ui.horizontal(|ui| {
                    if theme::action(ui, "직접 입력", ButtonRole::Emphasized, true).clicked() {
                        begin_input = true;
                    }
                    theme::action(ui, "캡처 범위 지정", ButtonRole::Emphasized, false);
                });
                components::soft_divider(ui);
                ui.add_space(8.0);
                ui.label(theme::subsection_heading("폴더 자동 인식"));
                ui.label(theme::caption("설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요."));
            }
        });
        if new_reception {
            self.raw_source.clear();
            self.clear_source_analysis();
            self.source_editing = false;
        } else if begin_input {
            self.source_editing = true;
        }
        if analyze_requested {
            self.start_source_analysis(ui.ctx().clone());
        }
    }

    fn message_section(&mut self, ui: &mut Ui, height: f32) {
        Self::section_with_header(ui, "복사용 자동 메시지", height, |ui| {
            let mut include_link = false;
            ui.add_enabled(false, egui::Checkbox::new(&mut include_link, "링크 포함"));
        }, |ui| {
            ui.add_space(8.0);
            Frame::new().fill(theme::INPUT).corner_radius(6).inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_min_height((ui.available_height() - 85.0).max(80.0));
                    ui.label(theme::caption("거래처와 주문 정보를 확정하면 Rust가 자동 메시지를 생성합니다."));
                });
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.horizontal(|ui| {
                    theme::action(ui, "그대로 복사", ButtonRole::Standard, false);
                    theme::action(ui, "인사말 포함 복사", ButtonRole::Standard, false);
                });
            });
        });
    }

    fn details_and_freight(&mut self, ui: &mut Ui) {
        let full_height = ui.available_height();
        let has_result = self.analysis.is_some();
        let count = self.analysis.as_ref().map(|analysis| {
            native_engine::recipients(analysis).len()
        }).unwrap_or(0);

        Self::section_with_header(ui, "확인할 내용", (full_height * 0.27).max(150.0), |ui| {
            ui.label(theme::caption(if has_result {
                "Rust 원문 분석 완료"
            } else if self.analysis_pending {
                "Rust 원문 분석 중"
            } else {
                "분석 대기"
            }));
        }, |ui| {
            if let Some(error) = &self.analysis_error {
                ui.colored_label(theme::DANGER, format!("인식 오류: {error}"));
            } else if self.analysis_pending {
                ui.label(theme::body("기존 Rust 인식 엔진이 발주원문을 분석하고 있습니다."));
            } else if let Some(analysis) = &self.analysis {
                ui.label(theme::body(&format!("수취인 {count}건 인식")));
                let unidentified = analysis.get("unassigned")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len).unwrap_or(0);
                let candidates = analysis.get("candidates")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len).unwrap_or(0);
                if unidentified > 0 || candidates > 0 {
                    ui.colored_label(theme::SELECTED_ORDER,
                        format!("미분류 근거 {unidentified}건 · 추가 후보 {candidates}건"));
                }
                let issue_count: usize = native_engine::recipients(analysis).iter()
                    .flat_map(|recipient| recipient.get("items")
                        .and_then(serde_json::Value::as_array)
                        .into_iter().flatten())
                    .map(|item| item.get("issues")
                        .and_then(serde_json::Value::as_array)
                        .map(Vec::len).unwrap_or(0))
                    .sum();
                if issue_count > 0 {
                    ui.colored_label(theme::SELECTED_ORDER,
                        format!("상품 인식 검토 사항 {issue_count}건"));
                }
                ui.label(theme::caption("인식 결과만 표시 · 주문 확정·가격·운임 검증은 아직 미연결"));
            } else if self.sample {
                ui.label(theme::caption("샘플 화면입니다. 발주원문을 입력하면 실제 Rust 분석 결과가 표시됩니다."));
            } else {
                ui.label(theme::caption("발주원문을 입력하고 분석하면 수취인·상품 인식 결과가 여기에 나타납니다."));
            }
        });
        ui.add_space(PANEL_GAP);

        let mut next_recipient = None;
        Self::section(ui, "수취인별 주문 정보", (full_height * 0.22).max(180.0), |ui| {
            if let Some(analysis) = &self.analysis {
                let recipients = native_engine::recipients(analysis);
                if recipients.len() > 1 {
                    egui::ScrollArea::horizontal()
                        .id_salt("analyzed-recipients")
                        .drag_to_scroll(false)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, recipient) in recipients.iter().enumerate() {
                                    let name = native_engine::text(recipient, "name");
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
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let mut name = native_engine::text(recipient, "name").to_owned();
                    let mut address = native_engine::text(recipient, "address").to_owned();
                    let mut mobile = native_engine::text(recipient, "mobile").to_owned();
                    let mut phone = native_engine::text(recipient, "phone").to_owned();
                    let mut note = native_engine::text(recipient, "note").to_owned();
                    components::labeled_field(ui, "이름", &mut name, components::FieldKind::Text, false);
                    components::labeled_field(ui, "주소", &mut address, components::FieldKind::Address, false);
                    components::labeled_field(ui, "휴대전화", &mut mobile, components::FieldKind::Telephone, false);
                    if !phone.trim().is_empty() {
                        components::labeled_field(ui, "전화", &mut phone, components::FieldKind::Telephone, false);
                    }
                    components::labeled_field(ui, "요청 메시지", &mut note, components::FieldKind::Text, false);
                } else {
                    ui.label(theme::caption("인식된 수취인 정보가 없습니다."));
                }
            } else if self.sample {
                let mut name = String::from("샘플수취인");
                let mut address = String::from("샘플시 샘플로 123");
                let mut phone = String::from("010-0000-0000");
                let mut note = String::from("없음");
                ui.spacing_mut().item_spacing.y = 4.0;
                components::labeled_field(ui, "이름", &mut name, components::FieldKind::Text, false);
                components::labeled_field(ui, "주소", &mut address, components::FieldKind::Address, false);
                components::labeled_field(ui, "휴대전화", &mut phone, components::FieldKind::Telephone, false);
                components::labeled_field(ui, "요청 메시지", &mut note, components::FieldKind::Text, false);
            } else {
                ui.label(theme::caption("발주원문을 입력하고 분석하면 인식된 수취인 정보가 표시됩니다."));
            }
        });
        if let Some(index) = next_recipient {
            self.selected_recipient = index;
        }
        // Derive the displayed items AFTER changing the recipient selection.
        // Otherwise the details pane switches immediately but the item pane
        // can display the previous recipient for one frame.
        let selected = self.analysis.as_ref().and_then(|analysis| {
            native_engine::recipients(analysis).get(self.selected_recipient)
        });
        ui.add_space(PANEL_GAP);

        let remaining = ui.available_height().max(230.0);
        Self::section(ui, "품목 및 운임", remaining, |ui| {
            if let Some(recipient) = selected {
                if let Some(items) = recipient.get("items").and_then(serde_json::Value::as_array) {
                    if items.is_empty() {
                        ui.label(theme::caption("인식된 상품이 없습니다. 원문과 상품 등록 정보를 확인하세요."));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("recognized-items")
                        .max_height((ui.available_height() - 115.0).max(90.0))
                        .show(ui, |ui| {
                            for item in items {
                                ui.horizontal(|ui| {
                                    ui.label(theme::body(&native_engine::item_title(item)));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(theme::caption(&native_engine::item_quantity(item)));
                                    });
                                });
                                let issues = item.get("issues").and_then(serde_json::Value::as_array);
                                if let Some(issues) = issues {
                                    for issue in issues.iter().take(3) {
                                        if let Some(message) = issue.as_str() {
                                            ui.colored_label(theme::SELECTED_ORDER, message);
                                        }
                                    }
                                }
                                components::soft_divider(ui);
                            }
                        });
                } else {
                    ui.label(theme::caption("상품 인식 결과가 없습니다."));
                }
                ui.add_space(8.0);
                ui.label(theme::caption("운임·단가 미연결 · 인식 결과는 주문으로 저장되지 않습니다."));
            } else if self.sample {
                ui.label(theme::body("709-1 / 블랙 / 비타공 상판 · 1개"));
                components::soft_divider(ui);
                ui.label(theme::body("운임 3,000원 (선불) · 샘플 표시"));
                ui.label(theme::caption("샘플은 실제 Rust 인식 결과와 구별됩니다."));
            } else {
                ui.label(theme::caption("분석 후 인식된 품목을 확인할 수 있습니다."));
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                components::summary_strip(
                    ui,
                    "총 주문",
                    if self.sample && !has_result { "62,000원 (샘플)" } else { "—" },
                    if has_result {
                        "청구·부가세·운임 계산 미연결"
                    } else if self.sample {
                        "택배 1건 · 샘플 금액"
                    } else {
                        "업무 엔진 계산 연결 전"
                    },
                );
            });
        });
    }

    fn orders_pane(&mut self, ui: &mut Ui) {
        ui.label(theme::section_heading("처리 중인 주문"));
        theme::divider(ui);
        let mut changed = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.sample {
                for index in 0..2 {
                    let selected = self.order_expanded[index];
                    components::record_card(ui, |ui| {
                        ui.horizontal(|ui| {
                            let caret = if selected { "▾" } else { "▸" };
                            if theme::action(ui, &format!("{caret} 샘플거래처"), ButtonRole::Quiet, true).clicked() {
                                changed = Some(index);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                theme::action(ui, "묶음 관리", ButtonRole::Standard, false);
                                let tones = [
                                    if index == 0 { components::StatusTone::Active } else { components::StatusTone::Complete },
                                    if index == 0 { components::StatusTone::Neutral } else { components::StatusTone::Complete },
                                    if index == 0 { components::StatusTone::Neutral } else { components::StatusTone::Complete },
                                    if index == 0 { components::StatusTone::Neutral } else { components::StatusTone::Active },
                                ];
                                components::status_dots(ui, &tones);
                            });
                        });
                        theme::divider(ui);
                        ui.horizontal(|ui| {
                            ui.label(theme::caption("배송 1건"));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                theme::action(
                                    ui,
                                    if index == 0 { "주문 확인 완료" } else { "입금 확인 완료" },
                                    ButtonRole::Primary,
                                    false,
                                );
                                ui.label(RichText::new(if index == 0 { "62,000원" } else { "86,000원" }).size(13.0).strong());
                            });
                        });
                        if selected {
                            theme::divider(ui);
                            components::table_header(ui, &[
                                ("수취인", 90.0), ("품목", 164.0), ("운임", 76.0),
                            ]);
                            components::table_row(ui, &[
                                ("샘플수취인", 90.0),
                                ("709-1 블랙 × 1", 164.0),
                                ("3,000원", 76.0),
                            ]);
                        }
                    });
                    ui.add_space(PANEL_GAP);
                }
                ui.add_space(12.0);
                ui.label(theme::caption("위 주문 2건은 UI 미리보기 전용 가상 데이터입니다."));
            } else {
                ui.label(theme::caption("실제 진행 중인 주문은 Rust/SQLite 연결 후 표시됩니다."));
            }
        });
        if let Some(index) = changed {
            self.order_expanded[index] = !self.order_expanded[index];
        }
    }

    fn other_page(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(16.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(self.page.label()).size(20.0).strong());
                ui.add_space(12.0);
                Self::section(ui, "공통 레이아웃 적용 대기", 180.0, |ui| {
                    ui.label(theme::body("이 화면의 Rust 업무 기능과 관리 UI는 아직 연결되지 않았습니다."));
                    ui.label(theme::caption("기존 React 관리 화면은 삭제하거나 수정하지 않았습니다."));
                });
            });
    }
}

impl eframe::App for BandoEgui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_source_analysis();
        self.top_bar(ctx);
        egui::TopBottomPanel::bottom("preview_mode")
            .exact_height(36.0)
            .frame(Frame::new().fill(theme::PANEL).inner_margin(4.0))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(theme::caption("egui UI 재구축 · 별도 실행 · 실제 주문/DB 미연결"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::action(
                            ui,
                            if self.sample { "빈 화면 보기" } else { "UI 샘플 보기" },
                            ButtonRole::Standard,
                            true,
                        ).clicked() {
                            self.sample = !self.sample;
                        }
                    });
                });
            });
        if self.page == Page::Reception {
            self.reception(ctx);
        } else {
            self.other_page(ctx);
        }
        self.frameless_resize(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1150.0, 720.0])
            .with_title("BANDO HUB · egui UI Preview")
            .with_decorations(false)
            .with_resizable(true),
        ..Default::default()
    };
    eframe::run_native(
        "BANDO HUB · egui UI Preview",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::<BandoEgui>::default())
        }),
    )
}
