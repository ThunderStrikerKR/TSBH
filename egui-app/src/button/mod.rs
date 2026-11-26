//! Shared native button rendering and hover/focus policy.
use eframe::egui::{self,Color32,CornerRadius,Response,Stroke,Ui,Vec2};
use crate::{theme,typography};
use crate::theme::*;

pub(crate) fn button_hover_palette(hovered:bool,focused:bool)->(Option<Color32>,Option<Stroke>){
    let fill=hovered.then_some(BUTTON_HOVER_TINT);
    let border=if focused{Some(Stroke::new(SOFT_DIVIDER_WIDTH,SELECTION))}
        else if hovered{Some(Stroke::new(SOFT_DIVIDER_WIDTH,SELECTION.linear_multiply(0.82)))}
        else{None};
    (fill,border)
}

/// Paint-only hover emphasis for every actionable rectangular control,
/// including buttons with explicit fills that bypass egui's hovered fill.
//// No layout, font metrics, border rect or hit area changes on hover.
/// Hover paint must use the SAME corner geometry as its button.
/// A rectangular highlight inside a rounded date group leaves blue diagonal
/// slivers and gets clipped by the group's outer border.
pub fn paint_button_hover_corners(
    ui:&Ui,response:&Response,corners:egui::CornerRadius
){
    crate::floating_notice::remember_response(ui.ctx(),response);
    if !response.hovered() && !response.has_focus(){return;}
    let rect=response.rect.shrink(0.5);
    if rect.width()<=1.0 || rect.height()<=1.0{return;}
    let paint=ui.painter().with_clip_rect(
        ui.clip_rect().intersect(response.rect)
    );
    let (fill,border)=button_hover_palette(response.hovered(),response.has_focus());
    if let Some(fill)=fill{
        paint.rect_filled(rect,corners,fill);
    }
    if let Some(stroke)=border{
        paint.rect_stroke(rect,corners,stroke,egui::StrokeKind::Inside);
    }
}

/// Existing buttons keep their uniform corner radius. Segmented controls can
/// use paint_button_hover_corners without altering other components.
pub fn paint_button_hover(ui:&Ui,response:&Response,radius:f32){
    let radius=egui::CornerRadius::same(radius.round().clamp(0.0,255.0) as u8);
    paint_button_hover_corners(ui,response,radius);
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

/// Shared fixed-minimum-width action, for select triggers that must match
/// the 34pt management controls rather than egui's shorter default ComboBox.
pub fn action_with_min_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool, min_width: f32,
) -> Response {
    action_with_palette(ui, label, role, enabled, min_width, false)
}

/// Actions embedded in colored review cards retain their original role geometry
/// and hover behavior, but use the shared white action palette when enabled.
pub fn review_action(ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool) -> Response {
    action_with_palette(ui, label, role, enabled, 0.0, true)
}

/// One visual role/state contract for standard widgets and fixed-rect actions.
/// Geometry and interaction remain owned by the respective components.
pub(crate) fn action_style(
    role:ButtonRole,enabled:bool,review_white:bool
)->(f32,Color32,Color32,Stroke){
    let (height, fill, color, stroke): (f32, Color32, Color32, Stroke) = match role {
        ButtonRole::Primary => (ACTION_PRIMARY_HEIGHT, PRIMARY_ACTION_FILL, BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Standard => (ACTION_STANDARD_HEIGHT, RAISED, TEXT, Stroke::new(1.0_f32, BORDER)),
        ButtonRole::TableSecondary => (
            ACTION_STANDARD_HEIGHT,
            PANEL,
            SECONDARY,
            Stroke::new(1.0_f32, DIVIDER),
        ),
        ButtonRole::Emphasized => (ACTION_EMPHASIZED_HEIGHT, BRIGHT, BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Quiet => (ACTION_QUIET_HEIGHT, PANEL, SECONDARY, Stroke::new(FRAME_STROKE_WIDTH,DIVIDER)),
        ButtonRole::Ghost => (ACTION_PRIMARY_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::Danger => (34.0, DANGER_ACTION_BG, ERROR_TEXT, Stroke::new(FRAME_STROKE_WIDTH,DANGER_ACTION_BORDER)),
        ButtonRole::Navigation => (NAV_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveNavigation => (NAV_HEIGHT, ACTIVE_NAVIGATION_BG, TEXT, Stroke::NONE),
        ButtonRole::ManagementTab => (MANAGEMENT_TAB_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveManagementTab => (MANAGEMENT_TAB_HEIGHT, RAISED, TEXT, Stroke::new(1.0_f32,BORDER)),
        ButtonRole::KakaoControl => (KAKAO_CONTROL_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveKakaoControl => (KAKAO_CONTROL_HEIGHT, RAISED, TEXT, Stroke::new(FRAME_STROKE_WIDTH,BORDER)),
    };
    // Standard management actions have an explicit disabled appearance.
    // egui's generic disabled tint alone was too close to the enabled state.
    // Callers resolve the parent scope before selecting the visual state.
    let (fill,color,stroke)=if review_white {
        if enabled {
            (BRIGHT, BRIGHT_TEXT, stroke)
        } else {
            (DISABLED_ACTION_BG, DISABLED_ACTION_TEXT, stroke)
        }
    }else if !enabled && matches!(role,ButtonRole::Ghost) {
        (Color32::TRANSPARENT,DISABLED_ACTION_TEXT,Stroke::NONE)
    }else if !enabled
        && matches!(role,
            ButtonRole::Primary | ButtonRole::Standard | ButtonRole::TableSecondary | ButtonRole::Emphasized | ButtonRole::Quiet | ButtonRole::Danger
        )
    {
        (DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,
         Stroke::new(FRAME_STROKE_WIDTH,DISABLED_ACTION_BORDER))
    }else{(fill,color,stroke)};
    (height,fill,color,stroke)
}

/// Field-like actions (calendar and dropdown) share the disabled action
/// palette but retain the approved enabled input surface and semantic ink.
pub(crate) fn field_action_palette(enabled:bool,enabled_ink:Color32)
    ->(Color32,Color32,Stroke){
    if enabled{
        (INPUT,enabled_ink,Stroke::new(SOFT_DIVIDER_WIDTH,BORDER))
    }else{
        let (_,_,ink,stroke)=action_style(ButtonRole::Standard,false,false);
        (MANAGEMENT_DROPDOWN_DISABLED_BG,ink,stroke)
    }
}

fn action_with_palette(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool, min_width: f32,
    review_white: bool,
) -> Response {
    let effective_enabled=enabled && ui.is_enabled();
    let (height,fill,color,stroke)=action_style(role,effective_enabled,review_white);
    let font_size = match role {
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => typography::FONT_MAIN_NAV,
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab =>
            typography::FONT_MANAGEMENT_TAB,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl =>
            typography::FONT_METADATA,
        ButtonRole::Primary => typography::FONT_PRIMARY_ACTION,
        _ => typography::FONT_STANDARD_ACTION,
    };
    // Scoped spacing prevents one button role from changing the padding of
    // neighboring buttons, fields and navigation controls.
    let padding_x = match role {
        ButtonRole::Primary => 14.0,
        ButtonRole::Quiet => 8.0,
        ButtonRole::Ghost => 12.0,
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => 12.0,
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => 12.0,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => 7.0,
        _ => 12.0,
    };
    let radius = match role {
        ButtonRole::Navigation | ButtonRole::ActiveNavigation |
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => 5,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => 4,
        _ => CONTROL_RADIUS,
    };
    // All light-filled actions (Primary and Emphasized) use regular-weight
    // captions app-wide. Selected tabs and destructive actions retain their
    // distinct typography; size, color, padding and behavior are unchanged.
    let important = matches!(
        role,
        ButtonRole::Danger | ButtonRole::ActiveNavigation |
        ButtonRole::ActiveManagementTab | ButtonRole::ActiveKakaoControl
    );
    let response=ui.scope(|ui| {
        let padding_y = match role {
            ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => 2.0,
            ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => 6.0,
            _ => 6.0,
        };
        ui.spacing_mut().button_padding = Vec2::new(padding_x, padding_y);
        let button = egui::Button::new(
            typography::action_label(label, font_size, important, color)
        )
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(radius))
            .min_size(Vec2::new(min_width, height));
        ui.add_enabled(enabled, button)
    }).inner;
    if effective_enabled {
        paint_button_hover(ui,&response,radius as f32);
    }
    response
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
    // Window chrome uses the same hover outline as every other actionable
    // control, without shifting the 32pt hit rectangle or painted glyph.
    paint_button_hover(ui,&response,4.0);
    let center = response.rect.center();
    let ink = if response.hovered() { TEXT } else { SECONDARY };
    let line = Stroke::new(1.35_f32, ink);
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

/// Shared zero-padding allocation for compact action roles.
fn compact_action_surface(ui:&mut Ui,enabled:bool,button:egui::Button<'_>)->Response{
    ui.scope(|slot|{
        slot.spacing_mut().button_padding=Vec2::ZERO;
        slot.add_enabled(enabled,button)
    }).inner
}

/// Shared compact icon-only action. Its 28pt hit target follows the
/// management date-navigation control; glyph and enabled state are
/// supplied by the caller while hover paint stays in the common layer.
pub fn icon_action_button(
    ui:&mut Ui,label:&str,enabled:bool
)->Response{
    let response=compact_action_surface(ui,enabled,egui::Button::new(label)
        .frame(false)
        .min_size(Vec2::splat(theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH)));
    if enabled{
        theme::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    response
}

/// Native navigation buttons retain their existing optional permission
/// scope. Fixed-rectangle step buttons deliberately use interact instead.
pub(crate) fn navigation_native_button(
    ui:&mut Ui,size:Vec2,button:egui::Button<'_>,enabled:Option<bool>
)->Response{
    let place=|slot:&mut Ui|slot.add_sized(size,button);
    match enabled{
        Some(enabled)=>ui.add_enabled_ui(enabled,place).inner,
        None=>place(ui),
    }
}

