//! BANDO HUB egui — first native visual implementation.
//! UI preview only: it does not open production SQLite, call Rust business
//! commands, or dock KakaoTalk. Existing React + Tauri remains untouched.

mod theme;

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, Stroke, Ui, Vec2};
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
}

impl Default for BandoEgui {
    fn default() -> Self {
        Self {
            page: Page::Reception,
            sample: false,
            notifications_open: false,
            order_expanded: [true, false],
        }
    }
}

impl BandoEgui {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("global_navigation")
            .exact_height(theme::HEADER_HEIGHT)
            .frame(Frame::new().fill(Color32::from_rgb(27, 27, 27)).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new("BANDO HUB").size(20.0).strong().color(theme::TEXT));
                    ui.label(RichText::new("/ UI 프리뷰").size(11.0).color(theme::MUTED));
                    ui.add_space(16.0);
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
                        ui.label(RichText::new("테스트 전용 · DB 미연결").size(12.0).color(theme::SELECTED_ORDER));
                        if theme::action(ui, "알림", ButtonRole::Standard, true).clicked() {
                            self.notifications_open = !self.notifications_open;
                        }
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

    fn reception(&mut self, ctx: &egui::Context) {
        // Dedicated resizable left / right panes. No fake business actions.
        egui::SidePanel::left("kakao_pane")
            .resizable(true)
            .default_width(335.0)
            .width_range(250.0..=470.0)
            .frame(Frame::new().fill(theme::PANEL).stroke(Stroke::new(1.0, theme::BORDER)).inner_margin(10.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.44).max(20.0));
                    theme::action(ui, "카카오톡 연결", ButtonRole::Emphasized, false)
                        .on_disabled_hover_text("카카오톡 Windows 도킹은 아직 이 프리뷰에 연결되지 않았습니다.");
                });
            });

        egui::SidePanel::right("orders_pane")
            .resizable(true)
            .default_width(360.0)
            .width_range(310.0..=550.0)
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(6.0))
            .show(ctx, |ui| {
                self.orders_pane(ui);
            });

        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::CANVAS).inner_margin(6.0))
            .show(ctx, |ui| {
                // A common 2-column grid, maintaining the four logical areas
                // of the React screen; draggable inner split follows in phase 2.
                ui.columns(2, |columns| {
                    self.source_and_message(&mut columns[0]);
                    self.details_and_freight(&mut columns[1]);
                });
            });
    }

    fn source_and_message(&mut self, ui: &mut Ui) {
        let full_height = ui.available_height();
        let source_height = (full_height * 0.50 - PANEL_GAP).max(280.0);
        Self::section_with_header(ui, "발주 원문", source_height, |ui| {
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
        ui.add_space(PANEL_GAP);
        let message_height = (ui.available_height() - PANEL_GAP).max(220.0);
        Self::section_with_header(ui, "복사용 자동 메시지", message_height, |ui| {
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
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1150.0, 720.0])
            .with_title("BANDO HUB · egui UI Preview"),
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
