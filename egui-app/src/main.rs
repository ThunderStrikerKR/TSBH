//! BANDO HUB egui — first native visual implementation.
//! UI preview only: it does not open production SQLite, call Rust business
//! commands, or dock KakaoTalk. Existing React + Tauri remains untouched.

mod theme;
mod layout;

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, Rect, Sense, Ui, UiBuilder, Vec2};
use theme::{ButtonRole, PANEL_GAP};

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

struct BandoEgui {
    page: Page,
    sample: bool,
    notifications_open: bool,
    order_expanded: [bool; 2],
    layout: layout::LayoutState,
    dragging_split: Option<usize>,
    split_last_delta: f32,
    dragging_vertical: bool,
    vertical_last_delta: f32,
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
            split_last_delta: 0.0,
            dragging_vertical: false,
            vertical_last_delta: 0.0,
        }
    }
}

impl BandoEgui {
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
        let bounds = ctx.screen_rect();
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
            ui.set_min_width((width - 20.0).max(60.0));
            ui.set_min_height((minimum_height - 20.0).max(48.0));
            ui.horizontal(|ui| {
                ui.label(theme::section_heading(title));
                ui.with_layout(Layout::right_to_left(Align::Center), header_right);
            });
            theme::divider(ui);
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
                                    self.dragging_split = Some(index);
                                    self.split_last_delta = 0.0;
                                }
                                if response.dragged() && self.dragging_split == Some(index) {
                                    let step = response.drag_delta().x - self.split_last_delta;
                                    self.layout.resize_pair(widths, index, step);
                                    self.split_last_delta = response.drag_delta().x;
                                }
                                if response.drag_stopped() && self.dragging_split == Some(index) {
                                    self.dragging_split = None;
                                    self.split_last_delta = 0.0;
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
            self.dragging_vertical = true;
            self.vertical_last_delta = 0.0;
        }
        if response.dragged() && self.dragging_vertical {
            let step = response.drag_delta().y - self.vertical_last_delta;
            self.layout.resize_vertical(source_height, step, height);
            self.vertical_last_delta = response.drag_delta().y;
        }
        if response.drag_stopped() && self.dragging_vertical {
            self.dragging_vertical = false;
            self.vertical_last_delta = 0.0;
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
        Self::section_with_header(ui, "발주 원문", height, |ui| {
            theme::action(ui, "+ 새 주문", ButtonRole::Emphasized, false)
                .on_disabled_hover_text("주문 생성은 Rust 업무 엔진 연결 이후 활성화됩니다.");
        }, |ui| {
            if self.sample {
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
            } else {
                ui.add_space(50.0);
                ui.label(theme::subsection_heading("클립보드 접수"));
                ui.label(theme::caption("복사한 텍스트와 이미지를 자동으로 구분해 인식합니다."));
                ui.horizontal(|ui| {
                    theme::action(ui, "대리발송", ButtonRole::Emphasized, false);
                    theme::action(ui, "대리점 납품", ButtonRole::Emphasized, false);
                });
                ui.label(theme::caption("이미지 캡처는 Ctrl + Shift + S 또는 Print Screen으로 복사하세요."));
                theme::divider(ui);
                ui.label(theme::subsection_heading("다른 방법으로 접수"));
                ui.horizontal(|ui| {
                    theme::action(ui, "직접 입력", ButtonRole::Emphasized, false);
                    theme::action(ui, "캡처 범위 지정", ButtonRole::Emphasized, false);
                });
                theme::divider(ui);
                ui.label(theme::subsection_heading("폴더 자동 인식"));
                ui.label(theme::caption("설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요."));
            }
        });
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
        Self::section_with_header(ui, "확인할 내용", (full_height * 0.27).max(150.0), |ui| {
            ui.label(theme::caption("조치 0건 / 안내 0건"));
        }, |ui| {
            ui.label(theme::caption(if self.sample {
                "샘플 UI에는 실제 Rust 분석 결과가 없습니다."
            } else {
                "필수 확인 사항 없음"
            }));
        });
        ui.add_space(PANEL_GAP);
        Self::section(ui, "수취인별 주문 정보", (full_height * 0.22).max(145.0), |ui| {
            if self.sample {
                theme::label_value(ui, "이름", "샘플수취인");
                theme::label_value(ui, "주소", "샘플시 샘플로 123");
                theme::label_value(ui, "휴대폰", "010-0000-0000");
                theme::label_value(ui, "요청 메시지", "없음");
            } else {
                ui.label(theme::caption("발주 원문을 입력하거나 배송 건을 선택하세요."));
            }
        });
        ui.add_space(PANEL_GAP);
        let remaining = ui.available_height().max(230.0);
        Self::section(ui, "품목 및 운임", remaining, |ui| {
            if self.sample {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("품목").size(13.0).color(theme::SECONDARY));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(theme::caption("금액"));
                        ui.add_space(68.0);
                        ui.label(theme::caption("수량"));
                    });
                });
                theme::divider(ui);
                ui.horizontal(|ui| {
                    ui.label(theme::body("709-1 / 블랙 / 비타공 상판"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(theme::body("59,000원"));
                        ui.add_space(30.0);
                        ui.label(theme::body("1개"));
                    });
                });
                theme::divider(ui);
                ui.horizontal(|ui| {
                    ui.label(theme::body("운임"));
                    ui.label(theme::body("3,000원 (선불)"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        theme::action(ui, "공장에 운임 문의", ButtonRole::Quiet, false);
                    });
                });
                ui.add_space(12.0);
                theme::action(ui, "+ 상품 추가", ButtonRole::Standard, false);
            } else {
                ui.label(theme::caption("거래처 선택 후 운임과 청구금액을 확인합니다."));
            }
            ui.add_space((ui.available_height() - 42.0).max(4.0));
            theme::divider(ui);
            ui.horizontal(|ui| {
                ui.label(theme::caption("총 주문"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(if self.sample { "62,000원" } else { "—" })
                            .size(16.0).strong().color(theme::TEXT),
                    );
                });
            });
            ui.label(theme::caption(if self.sample { "택배 1건 · 부가세 별도" } else { "택배 0건 · 부가세 별도" }));
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
                    theme::section_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let caret = if selected { "▾" } else { "▸" };
                            if theme::action(ui, &format!("{caret} 샘플거래처"), ButtonRole::Quiet, true).clicked() {
                                changed = Some(index);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                theme::action(ui, "묶음 관리", ButtonRole::Standard, false);
                                for stage in (0..4).rev() {
                                    let completed = stage <= if index == 0 { 0 } else { 3 };
                                    let color = if !completed { theme::BORDER } else if index == 0 {
                                        theme::SELECTED_ORDER
                                    } else if stage == 3 {
                                        theme::SELECTED_ORDER
                                    } else {
                                        theme::SUCCESS
                                    };
                                    ui.colored_label(color, "●");
                                }
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
                            ui.label(theme::caption("수취인                 품목                                     운임"));
                            theme::nested_frame().show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(theme::body("샘플수취인"));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(theme::body("3,000원"));
                                    });
                                });
                                ui.label(theme::caption("709-1 / 블랙 / 비타공 상판 × 1"));
                            });
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
