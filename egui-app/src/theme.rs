//! Single source of visual tokens for the egui migration.
//! Based on src/ui/design-system.json and docs/egui-ui-design-contract.md.
//! Keep screen-specific hard-coded colors and geometry out of view code.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Response, RichText, Stroke, Ui, Vec2,
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
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(padding_x, 6.0);
        let button = egui::Button::new(RichText::new(label).size(font_size).color(color))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(radius))
            .min_size(Vec2::new(0.0, height));
        ui.add_enabled(enabled, button)
    }).inner
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
