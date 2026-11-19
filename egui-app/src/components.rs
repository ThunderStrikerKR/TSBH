//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};
use crate::theme;

pub const LABEL_WIDTH: f32 = theme::FIELD_LABEL_WIDTH;
pub const TABLE_ROW_HEIGHT: f32 = 42.0;
pub const TABLE_HEADER_HEIGHT: f32 = 36.0;

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

/// Uses the same height, font and focus styling for every single-line field.
/// Disabled preview controls cannot edit business data.
pub fn field(ui: &mut Ui, value: &mut String, kind: FieldKind, enabled: bool) -> Response {
    let width = kind.preferred_width().min(ui.available_width().max(60.0));
    let field = egui::TextEdit::singleline(value)
        .font(egui::TextStyle::Body)
        .desired_width(width);
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(Vec2::new(width, theme::INPUT_HEIGHT), field)
    }).inner
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
        ui.add_sized(
            Vec2::new(LABEL_WIDTH, theme::INPUT_HEIGHT),
            egui::Label::new(RichText::new(label).size(theme::FONT_FIELD_LABEL).color(theme::SECONDARY)),
        );
        // egui horizontal layout already supplies the shared item gap.
        // Avoid doubling it and unnecessarily shrinking long address fields.
        field(ui, value, kind, enabled)
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

/// A single coordinate and pixel-snapping rule owns all vertical and
/// horizontal dock lines. Visual thickness is snapped to physical pixels,
/// while the full 6pt hit-area rectangle stays unchanged.
pub fn paint_dock_splitter(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
    let logical_thickness = if active {
        theme::DOCK_SPLITTER_HOVER_LINE
    } else {
        theme::DOCK_SPLITTER_LINE
    };
    let scale = ui.ctx().pixels_per_point().max(1.0);
    let line_px = (logical_thickness * scale).round().max(1.0);
    let line_width = line_px / scale;
    let painted = if vertical {
        let x = ((rect.center().x - 0.5 * line_width) * scale).round() / scale;
        Rect::from_min_size(
            egui::pos2(x, rect.top()),
            Vec2::new(line_width, rect.height()),
        )
    } else {
        let y = ((rect.center().y - 0.5 * line_width) * scale).round() / scale;
        Rect::from_min_size(
            egui::pos2(rect.left(), y),
            Vec2::new(rect.width(), line_width),
        )
    };
    ui.painter().rect_filled(
        painted, 0.0,
        if active { theme::BORDER } else { theme::DIVIDER },
    );
}

pub fn summary_strip(ui: &mut Ui, label: &str, value: &str, footnote: &str) {
    ui.vertical(|ui| {
        soft_divider(ui);
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(theme::FONT_FIELD_LABEL).color(theme::SECONDARY));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(value).size(theme::FONT_SECTION_HEADING).strong().color(theme::TEXT));
            });
        });
        if !footnote.trim().is_empty() {
            ui.label(RichText::new(footnote).size(theme::FONT_METADATA).color(theme::SECONDARY));
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
                            RichText::new(*title).size(theme::FONT_FIELD_LABEL).strong().color(theme::SECONDARY),
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
                egui::Label::new(RichText::new(*value).size(theme::FONT_FIELD_LABEL).color(theme::TEXT)),
            );
        }
    });
    soft_divider(ui);
}
