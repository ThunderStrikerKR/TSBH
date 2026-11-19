//! Single source of visual tokens for the egui migration.
//! Based on src/ui/design-system.json and docs/egui-ui-design-contract.md.
//! Keep screen-specific hard-coded colors and geometry out of view code.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Response, Stroke, Ui, Vec2,
};
use crate::typography;

pub const CANVAS: Color32 = Color32::from_rgb(16, 16, 16);
pub const HEADER_BG: Color32 = Color32::from_rgb(27, 27, 27);
pub const PANEL: Color32 = Color32::from_rgb(32, 32, 32);
pub const INPUT: Color32 = Color32::from_rgb(41, 41, 41);
pub const RAISED: Color32 = Color32::from_rgb(48, 48, 48);
pub const BORDER: Color32 = Color32::from_rgb(80, 80, 80);
pub const DIVIDER: Color32 = Color32::from_rgb(63, 63, 63);
pub const TEXT: Color32 = Color32::from_rgb(237, 237, 237);
pub const SECONDARY: Color32 = Color32::from_rgb(185, 185, 185);
pub const MUTED: Color32 = Color32::from_rgb(153, 153, 153);
pub const SELECTION: Color32 = Color32::from_rgb(112, 180, 255);
pub const SELECTED_ORDER: Color32 = Color32::from_rgb(242, 186, 100);
pub const SUCCESS: Color32 = Color32::from_rgb(169, 218, 181);
pub const DANGER: Color32 = Color32::from_rgb(255, 180, 180);
pub const BRIGHT: Color32 = Color32::WHITE;
pub const BRIGHT_TEXT: Color32 = Color32::from_rgb(23, 23, 23);
pub const SPACE_4: f32 = 4.0;
pub const SPACE_8: f32 = 8.0;
pub const SPACE_12: f32 = 12.0;
pub const SPACE_16: f32 = 16.0;
pub const SPACE_24: f32 = 24.0;
pub const SPACING: f32 = SPACE_8;
pub const HEADER_PADDING_X: i8 = 16;
pub const BRAND_ICON_SIZE: f32 = 38.0;
// Optical alignment of the visible red glyph within the PNG canvas.
// Keep text and navigation on the original header centerline.
pub const BRAND_ICON_OPTICAL_CENTER_Y: f32 = 2.0;
// A small positive gap keeps the wordmark visually separate from the logo.
// This affects only the brand slot, not navigation or header geometry.
pub const BRAND_WORDMARK_GAP: f32 = 3.0;
pub const HEADER_DRAG_STRIP: f32 = 8.0;
pub const SECTION_PADDING: f32 = SPACE_16;
pub const NESTED_PADDING: f32 = SPACE_12;
pub const SECTION_HEADER_HEIGHT: f32 = 38.0;
pub const SECTION_CONTENT_GAP: f32 = SPACE_8;
pub const CARD_PADDING: f32 = SPACE_12;
pub const FIELD_LABEL_WIDTH: f32 = 68.0;
pub const PANEL_GAP: f32 = 6.0;
// Splitter hit target stays 6pt; only the painted line is 1pt (2pt on hover).
pub const DOCK_SPLITTER_LINE: f32 = 1.0;
pub const DOCK_SPLITTER_HOVER_LINE: f32 = 2.0;
pub const SOFT_DIVIDER_WIDTH: f32 = 1.0;
pub const FRAME_STROKE_WIDTH: f32 = 1.0;
pub const CONTROL_RADIUS: u8 = 6;
pub const PANEL_RADIUS: u8 = 9;
pub const INPUT_HEIGHT: f32 = 36.0;
pub const ACTION_PRIMARY_HEIGHT: f32 = 36.0;
pub const ACTION_STANDARD_HEIGHT: f32 = 34.0;
pub const ACTION_EMPHASIZED_HEIGHT: f32 = 34.0;
pub const ACTION_QUIET_HEIGHT: f32 = 32.0;
// Reserve space for the single-row order total at the bottom of its pane.
pub const SUMMARY_STRIP_HEIGHT: f32 = 58.0;
pub const HEADER_HEIGHT: f32 = 62.0;
pub const NAV_HEIGHT: f32 = 38.0;
// Original React manual workspace: max 1760, viewport side gap 36,
// source-to-sheet columns 34:66 (.manual-workspace .manual-columns).
pub const MANUAL_DIALOG_MAX_WIDTH: f32 = 1760.0;
pub const MANUAL_DIALOG_SIDE_GAP: f32 = 36.0;
pub const MANUAL_DIALOG_VERTICAL_GAP: f32 = 44.0;
pub const MANUAL_SOURCE_SHARE: f32 = 0.34;
pub const MANUAL_SOURCE_MIN_WIDTH: f32 = 300.0;
pub const MANUAL_SHEET_MIN_WIDTH: f32 = 500.0;
pub const MANUAL_COLUMN_GAP: f32 = SPACE_12;
pub const MANUAL_SEARCH_DIALOG_WIDTH: f32 = MODAL_DEFAULT_MAX_WIDTH;
pub const MANUAL_SEARCH_RESULTS_HEIGHT: f32 = 420.0;
pub const MANUAL_INPUT_WIDTH: f32 = 250.0;
// Shared native modal/table roles. Manual source and spreadsheet-like rows
// inherit these values instead of locally painted one-off geometry.
pub const MODAL_PANEL_PADDING_X: f32 = 18.0;
pub const MODAL_PANEL_PADDING_Y: f32 = 16.0;
pub const MODAL_DECISION_MAX_WIDTH: f32 = 560.0;
pub const MODAL_DEFAULT_MAX_WIDTH: f32 = 880.0;
pub const MODAL_VIEWPORT_GUTTER: f32 = 18.0;
pub const MANUAL_TABLE_MIN_WIDTH: f32 = 1180.0;
pub const MANUAL_TABLE_HEADER_HEIGHT: f32 = 36.0;
pub const MANUAL_TABLE_ROW_HEIGHT: f32 = 42.0;
pub const MANUAL_TABLE_DELIVERY_COL: f32 = 82.0;
pub const MANUAL_TABLE_NAME_COL: f32 = 150.0;
pub const MANUAL_TABLE_ADDRESS_COL: f32 = 360.0;
pub const MANUAL_TABLE_PHONE_COL: f32 = 160.0;
pub const MANUAL_TABLE_NOTE_COL: f32 = 260.0;
pub const MANUAL_TABLE_PRODUCT_COL: f32 = 780.0;
pub const MANUAL_TABLE_QUANTITY_COL: f32 = 96.0;
pub const MANUAL_TABLE_ACTION_COL: f32 = 120.0;
pub const MANUAL_SOURCE_PENDING_BG: Color32 = Color32::from_rgb(63, 63, 63);
pub const MANUAL_SOURCE_ASSIGNED_BG: Color32 = Color32::from_rgb(42, 60, 74);

#[derive(Clone, Copy)]
pub enum ButtonRole {
    Primary,
    Standard,
    Emphasized,
    Quiet,
    Danger,
    Navigation,
    ActiveNavigation,
}

pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = CANVAS;
    style.visuals.window_fill = PANEL;
    style.visuals.faint_bg_color = RAISED;
    style.visuals.extreme_bg_color = INPUT;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.widgets.noninteractive.bg_fill = PANEL;
    style.visuals.widgets.inactive.bg_fill = RAISED;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 53, 53);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(61, 61, 61);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.selection.bg_fill = SELECTION.linear_multiply(0.3);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, SELECTION);
    style.spacing.item_spacing = Vec2::new(SPACING, SPACING);
    // Per-role horizontal padding is applied by action() below.
    style.spacing.button_padding = Vec2::new(SPACE_12, SPACE_8);
    ctx.set_style(style);
}

pub fn modal_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH,BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(egui::Margin {
            left: MODAL_PANEL_PADDING_X as i8,
            right: MODAL_PANEL_PADDING_X as i8,
            top: MODAL_PANEL_PADDING_Y as i8,
            bottom: MODAL_PANEL_PADDING_Y as i8,
        })
}

pub fn modal_nested_frame() -> Frame {
    Frame::new()
        .fill(INPUT)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH,DIVIDER))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(SPACE_8)
}

pub fn section_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH, BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(SECTION_PADDING)
}

/// Only for docked reception work panes. Neighboring panel edges belong to
/// the splitter, so there is no individual perimeter stroke or rounding.
/// Regular RecordCard, input, button, modal and independent section styles
/// retain their original rounded corners.
pub fn docked_pane_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(0))
        .inner_margin(SECTION_PADDING)
}

pub fn nested_frame() -> Frame {
    Frame::new()
        .fill(INPUT)
        .stroke(Stroke::new(1.0_f32, DIVIDER))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(NESTED_PADDING)
}

pub fn action(ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool) -> Response {
    action_with_min_width(ui, label, role, enabled, 0.0)
}

/// Reuse the exact same action design, filling only the column assigned by a
/// shared button-group component. The surrounding pane geometry is untouched.
pub fn action_full_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool,
) -> Response {
    let width = ui.available_width();
    action_with_min_width(ui, label, role, enabled, width)
}

fn action_with_min_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool, min_width: f32,
) -> Response {
    let (height, fill, color, stroke): (f32, Color32, Color32, Stroke) = match role {
        ButtonRole::Primary => (ACTION_PRIMARY_HEIGHT, Color32::from_rgb(238, 238, 238), BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Standard => (ACTION_STANDARD_HEIGHT, RAISED, TEXT, Stroke::new(1.0_f32, BORDER)),
        ButtonRole::Emphasized => (ACTION_EMPHASIZED_HEIGHT, BRIGHT, BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Quiet => (ACTION_QUIET_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::Danger => (34.0, Color32::from_rgb(84, 44, 48), DANGER, Stroke::NONE),
        ButtonRole::Navigation => (NAV_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveNavigation => (NAV_HEIGHT, Color32::from_rgb(41, 72, 92), TEXT, Stroke::NONE),
    };
    let font_size = match role {
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => typography::FONT_MAIN_NAV,
        ButtonRole::Primary => typography::FONT_PRIMARY_ACTION,
        _ => typography::FONT_STANDARD_ACTION,
    };
    // Scoped spacing prevents one button role from changing the padding of
    // neighboring buttons, fields and navigation controls.
    let padding_x = match role {
        ButtonRole::Primary => 14.0,
        ButtonRole::Quiet => 8.0,
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => 12.0,
        _ => 12.0,
    };
    let radius = match role {
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => 5,
        _ => CONTROL_RADIUS,
    };
    let important = matches!(
        role,
        ButtonRole::Primary | ButtonRole::Danger | ButtonRole::ActiveNavigation
    );
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(padding_x, 6.0);
        let button = egui::Button::new(
            typography::action_label(label, font_size, important, color)
        )
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(radius))
            .min_size(Vec2::new(min_width, height));
        ui.add_enabled(enabled, button)
    }).inner
}

/// Frameless Windows controls share a single 32pt hit area and precise
/// vector marks. Unicode "-", "□" and "×" had different font baselines.
#[derive(Clone, Copy)]
pub enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

pub fn window_control(ui: &mut Ui, control: WindowControl) -> Response {
    let response = ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.add(
            egui::Button::new("")
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .min_size(Vec2::splat(ACTION_QUIET_HEIGHT))
        )
    }).inner;
    if response.hovered() {
        ui.painter().rect_filled(response.rect, 4.0, RAISED);
    }
    let center = response.rect.center();
    let ink = if response.hovered() { TEXT } else { SECONDARY };
    let line = Stroke::new(1.35, ink);
    let painter = ui.painter();
    match control {
        WindowControl::Minimize => {
            painter.line_segment(
                [egui::pos2(center.x - 5.0, center.y),
                 egui::pos2(center.x + 5.0, center.y)], line
            );
        }
        WindowControl::Maximize => {
            let left = center.x - 5.0;
            let right = center.x + 5.0;
            let top = center.y - 5.0;
            let bottom = center.y + 5.0;
            for (a, b) in [
                (egui::pos2(left, top), egui::pos2(right, top)),
                (egui::pos2(right, top), egui::pos2(right, bottom)),
                (egui::pos2(right, bottom), egui::pos2(left, bottom)),
                (egui::pos2(left, bottom), egui::pos2(left, top)),
            ] {
                painter.line_segment([a, b], line);
            }
        }
        WindowControl::Close => {
            painter.line_segment(
                [egui::pos2(center.x - 5.0, center.y - 5.0),
                 egui::pos2(center.x + 5.0, center.y + 5.0)], line
            );
            painter.line_segment(
                [egui::pos2(center.x + 5.0, center.y - 5.0),
                 egui::pos2(center.x - 5.0, center.y + 5.0)], line
            );
        }
    }
    response
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), SOFT_DIVIDER_WIDTH),
        egui::Sense::hover(),
    );
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(SOFT_DIVIDER_WIDTH, DIVIDER),
    );
    ui.add_space(4.0);
}

pub fn label_value(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized([FIELD_LABEL_WIDTH, INPUT_HEIGHT], egui::Label::new(typography::field_label(label)));
        ui.label(typography::body(value));
    });
}
