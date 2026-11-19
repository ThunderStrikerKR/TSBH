//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, Sense, Stroke, Ui, Vec2};
use crate::{theme, typography};

pub const LABEL_WIDTH: f32 = theme::FIELD_LABEL_WIDTH;
pub const TABLE_ROW_HEIGHT: f32 = 42.0;
pub const TABLE_HEADER_HEIGHT: f32 = 36.0;

/// Keep paired reception actions at their content widths. Available pane
/// space stays empty rather than stretching two buttons to the pane edges.
/// The row wraps only if Korean labels cannot fit without clipping.
pub fn paired_actions(
    ui: &mut Ui,
    left: (&str, theme::ButtonRole, bool),
    right: (&str, theme::ButtonRole, bool),
) -> (Response, Response) {
    ui.horizontal_wrapped(|ui| {
        (
            theme::action(ui, left.0, left.1, left.2),
            theme::action(ui, right.0, right.1, right.2),
        )
    }).inner
}


/// The actual content space in a fixed-height docked section. Reserve
/// exactly the existing top/bottom frame padding, title, divider and gaps.
/// Section widgets must not impose independent minimums that can spill
/// across the source/message splitter.
pub fn docked_section_body_height(section_height: f32) -> f32 {
    let chrome = theme::SECTION_PADDING * 2.0
        + theme::SECTION_HEADER_HEIGHT
        + theme::SECTION_CONTENT_GAP
        + theme::SOFT_DIVIDER_WIDTH
        + theme::SPACE_4;
    (section_height - chrome).max(0.0)
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKind {
    Text,
    Telephone,
    Quantity,
    Money,
    Address,
}

impl FieldKind {
    pub fn preferred_width(self) -> f32 {
        match self {
            Self::Text => 180.0,
            Self::Telephone => 142.0,
            Self::Quantity => 68.0,
            Self::Money => 112.0,
            Self::Address => 320.0,
        }
    }
}

/// Native modal geometry belongs here, not to individual screens.
/// `work_area` excludes the externally foregrounded KakaoTalk dock.
pub fn modal_window(
    title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    egui::Window::new(typography::modal_title(title))
        .id(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .fixed_pos(work_area.center())
        .constrain_to(work_area)
        .max_size(work_area.size())
        .collapsible(false)
        .frame(theme::modal_frame())
}

pub fn decision_window(
    title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    modal_window(title,id,work_area)
        .resizable(false)
        .default_width(theme::MODAL_DECISION_MAX_WIDTH.min(work_area.width()))
}

/// A separate foreground layer draws the dim and consumes background input.
/// It is intentionally clipped to the actual workspace, never over KakaoTalk.
/// Render the backdrop before the corresponding modal(s); render a second one
/// before a nested confirmation to keep only the top dialog interactive.
pub fn modal_backdrop(
    ctx:&egui::Context,work_area:egui::Rect,id:&'static str,nested:bool
) {
    let alpha=if nested {
        theme::MODAL_NESTED_BACKDROP_ALPHA
    } else {
        theme::MODAL_BACKDROP_ALPHA
    };
    let area=egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(work_area.min)
        .interactable(true)
        .sense(egui::Sense::click_and_drag());
    // egui's modal focus barrier also blocks keyboard input in the toolbar,
    // while the visual dim and pointer blocker stay clear of the Kakao dock.
    let layer=area.layer();
    ctx.memory_mut(|memory|memory.set_modal_layer(layer));
    area.show(ctx,|ui|{
        let (rect,_)=ui.allocate_exact_size(
            work_area.size(),egui::Sense::click_and_drag()
        );
        ui.painter().rect_filled(
            rect,0.0,egui::Color32::from_black_alpha(alpha)
        );
    });
}

pub fn modal_actions(ui:&mut Ui, cancel:&str, proceed:&str,
    proceed_role:theme::ButtonRole, enabled:bool) -> (bool,bool) {
    let mut cancelled=false;
    let mut submitted=false;
    ui.horizontal(|ui|{
        cancelled=theme::action(ui,cancel,theme::ButtonRole::Quiet,true).clicked();
        submitted=theme::action(ui,proceed,proceed_role,enabled).clicked();
    });
    (cancelled,submitted)
}

/// Uses the same height, font and focus styling for every single-line field.
/// Disabled preview controls cannot edit business data.
pub fn field(ui: &mut Ui, value: &mut String, kind: FieldKind, enabled: bool) -> Response {
    let width = kind.preferred_width().min(ui.available_width().max(60.0));
    let editor_font = match kind {
        FieldKind::Money | FieldKind::Quantity => {
            egui::FontSelection::FontId(typography::numeric_editor_font())
        }
        // Preserve the regular Korean body face for names, addresses,
        // telephone and free-form text. Only explicitly numeric data
        // fields use the Consolas / monospace family.
        _ => egui::FontSelection::Style(egui::TextStyle::Body),
    };
    let field = egui::TextEdit::singleline(value)
        .font(editor_font)
        .vertical_align(Align::Center)
        .margin(egui::Margin::symmetric(8, 2))
        .desired_width(width);
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(Vec2::new(width, theme::INPUT_HEIGHT), field)
    }).inner
}

/// Shared single-line free-text editor for inline manual cells and search.
/// Uses the same font, internal margin and height as the standard field role.
pub fn flexible_text_field(
    ui:&mut Ui, value:&mut String, width:f32, placeholder:&str
)->Response {
    let width=width.min(ui.available_width().max(60.0));
    let edit=egui::TextEdit::singleline(value)
        .font(egui::TextStyle::Body)
        .vertical_align(Align::Center)
        .margin(egui::Margin::symmetric(8,2))
        .hint_text(placeholder)
        .desired_width(width);
    ui.add_sized(Vec2::new(width,theme::INPUT_HEIGHT),edit)
}

/// Shared compact spreadsheet cell. Selection is an inset border;
/// hovered cells do not resize the row, and a selected cell does not
/// borrow the main navigation style.
pub fn manual_table_cell(ui:&mut Ui,value:&str,width:f32,selected:bool)->Response {
    let label=if value.trim().is_empty(){"미입력"}else{value};
    let border=if selected {
        Stroke::new(theme::DOCK_SPLITTER_HOVER_LINE,theme::SELECTED_ORDER)
    }else{
        // Two adjacent cells share one visual divider. Never create a
        // double 2pt border on every table column.
        Stroke::new(theme::SOFT_DIVIDER_WIDTH/2.0,theme::DIVIDER)
    };
    ui.add_sized(
        Vec2::new(width,theme::MANUAL_TABLE_ROW_HEIGHT),
        egui::Button::new(typography::table_cell(label))
            .truncate()
            .fill(if selected{theme::RAISED}else{theme::INPUT})
            .stroke(border)
            .corner_radius(0),
    )
}
pub fn manual_table_heading(ui:&mut Ui,label:&str,width:f32){
    ui.add_sized(
        Vec2::new(width,theme::MANUAL_TABLE_HEADER_HEIGHT),
        egui::Label::new(typography::field_label(label)),
    );
}

/// A field group's label and editor share the same baseline and label width.
pub fn labeled_field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    kind: FieldKind,
    enabled: bool,
) -> Response {
    ui.horizontal(|ui| {
        let title=ui.add_sized(
            Vec2::new(LABEL_WIDTH, theme::INPUT_HEIGHT),
            egui::Label::new(typography::field_label(label)),
        );
        // The whole field row (including its label) is a single hover
        // target for exact source linking. No geometry is modified.
        let input=field(ui, value, kind, enabled);
        title.union(input)
    }).inner
}

/// A subtle separation between data belonging to the same record.
/// Sections themselves are framed by the caller.
pub fn soft_divider(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, theme::SOFT_DIVIDER_WIDTH), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER),
    );
}

/// The 6pt interaction gutter is always painted as a single surface.
/// The line is a separate operation so the parent dock can extend horizontal
/// dividers into a neighboring vertical gutter without painting a dark band.
pub fn paint_dock_splitter(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
    paint_dock_axis_line(ui, rect, vertical, active);
}

/// Pixel-exact axis line within a gutter rectangle. A 6-*physical*-pixel
/// gutter has a center between two pixels, so its normal line must occupy
/// the middle TWO pixels. A one-pixel line can only sit to one side.
/// Odd-width gutters use a single centered pixel. Hover uses the shared
/// 2pt token, adjusted only when necessary for symmetrical pixel margins.
pub fn paint_dock_axis_line(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    let scale = ui.ctx().pixels_per_point().max(1.0);
    let (low, high) = if vertical {
        (rect.left(), rect.right())
    } else {
        (rect.top(), rect.bottom())
    };
    let start_px = (low * scale).round() as i32;
    let end_px = (high * scale).round() as i32;
    let span_px = (end_px - start_px).max(1);
    // Honor the 1pt / 2pt design tokens where they can be truly centered.
    // For a 6px gutter, a requested 1px rule must expand to the middle 2px.
    let logical_width = if active {
        theme::DOCK_SPLITTER_HOVER_LINE
    } else {
        theme::DOCK_SPLITTER_LINE
    };
    let mut line_width_px = ((logical_width * scale).round() as i32)
        .clamp(1, span_px);
    if (span_px - line_width_px) % 2 != 0 {
        // Match slot-width parity so left/right margins are always equal.
        line_width_px += if line_width_px < span_px { 1 } else { -1 };
    }
    let line_start = start_px + (span_px - line_width_px) / 2;
    let line_end = line_start + line_width_px;

    let snapped = |n: f32| (n * scale).round() / scale;
    let line = if vertical {
        Rect::from_min_max(
            egui::pos2(line_start as f32 / scale, snapped(rect.top())),
            egui::pos2(line_end as f32 / scale, snapped(rect.bottom())),
        )
    } else {
        Rect::from_min_max(
            egui::pos2(snapped(rect.left()), line_start as f32 / scale),
            egui::pos2(snapped(rect.right()), line_end as f32 / scale),
        )
    };
    ui.painter().rect_filled(
        line, 0.0,
        if active { theme::BORDER } else { theme::DIVIDER },
    );
}

/// Display-only won formatting. No business price or rounding decisions live
/// in the UI layer; amounts are supplied by the Rust business engine.
pub fn display_won(amount: i64) -> String {
    let digits = amount.to_string();
    let (sign, positive) = if let Some(text) = digits.strip_prefix('-') {
        ("-", text)
    } else { ("", digits.as_str()) };
    let mut groups = String::new();
    for (index, character) in positive.chars().enumerate() {
        if index > 0 && (positive.len() - index) % 3 == 0 {
            groups.push(',');
        }
        groups.push(character);
    }
    format!("{sign}{groups}원")
}

pub fn summary_strip(ui: &mut Ui, label: &str, value: &str, footnote: &str) {
    ui.vertical(|ui| {
        soft_divider(ui);
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|ui| {
            ui.label(typography::field_label(label));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(typography::summary_value(value));
            });
        });
        if !footnote.trim().is_empty() {
            ui.label(typography::caption(footnote));
        }
    });
}

pub fn record_card(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::PANEL_RADIUS)
        .inner_margin(theme::CARD_PADDING)
        .show(ui, content);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusTone {
    Neutral,
    Active,
    Complete,
    Warning,
    Error,
    Linked,
}
impl StatusTone {
    pub fn color(self) -> Color32 {
        match self {
            Self::Neutral => theme::BORDER,
            Self::Active | Self::Warning => theme::SELECTED_ORDER,
            Self::Complete => theme::SUCCESS,
            Self::Error => theme::DANGER,
            Self::Linked => theme::SELECTION,
        }
    }
}

pub fn status_dots(ui: &mut Ui, tones: &[StatusTone]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for tone in tones {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(12.0, 18.0), egui::Sense::hover(),
            );
            ui.painter().circle_filled(rect.center(), 4.5, tone.color());
        }
    });
}

pub fn table_header(ui: &mut Ui, columns: &[(&str, f32)]) {
    Frame::new().fill(theme::RAISED).inner_margin(egui::Margin::symmetric(theme::SPACE_8 as i8, theme::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (title, width) in columns {
                    ui.add_sized(
                        Vec2::new(*width, TABLE_HEADER_HEIGHT - 8.0),
                        egui::Label::new(
                            typography::table_header(title),
                        ),
                    );
                }
            });
        });
}

pub fn table_row(ui: &mut Ui, columns: &[(&str, f32)]) {
    ui.horizontal(|ui| {
        for (value, width) in columns {
            ui.add_sized(
                Vec2::new(*width, TABLE_ROW_HEIGHT),
                egui::Label::new(typography::table_cell(value)),
            );
        }
    });
    soft_divider(ui);
}
