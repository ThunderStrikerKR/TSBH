//! Shared native button rendering and hover/focus policy.
use eframe::egui::{self,Color32,CornerRadius,Rect,Response,Sense,Stroke,Ui,Vec2};
use crate::{theme,typography};
use crate::theme::*;
use crate::ui_primitives::{paint_workflow_icon,WorkflowIcon,ManagementChevronDirection,paint_management_chevron,paint_management_chevron_with_color,paint_chevron_at_angle_scaled};

/// Register an actual interaction, then paint using the shared primitive.
fn paint_button_hover_corners(ui:&Ui,response:&Response,corners:egui::CornerRadius){
    crate::floating_notice::remember_response(ui.ctx(),response);
    crate::ui_primitives::paint_action_hover_corners(ui,response,corners);
}

/// Existing buttons keep their uniform corner radius. Segmented controls can
/// use paint_button_hover_corners without altering other components.
pub(crate) fn paint_button_hover(ui:&Ui,response:&Response,radius:f32){
    let radius=egui::CornerRadius::same(radius.round().clamp(0.0,255.0) as u8);
    paint_button_hover_corners(ui,response,radius);
}

/// Fixed-rectangle remove action for compact rows.
pub(crate) fn remove_at(ui:&Ui,hit:Rect,id:egui::Id,enabled:bool)->bool{
    remove_at_response(ui,hit,id,enabled).clicked()
}

/// The response form lets tag editors reuse the same vector glyph and hit
/// geometry as compact table rows, without a font baseline or nested button.
pub(crate) fn remove_at_response(ui:&Ui,hit:Rect,id:egui::Id,enabled:bool)->Response{
    let enabled=enabled && ui.is_enabled();
    let response=ui.interact(hit,id,if enabled{Sense::click()}else{Sense::hover()});
    let ink=theme::icon_ink(theme::IconRole::Secondary,theme::VisualState::enabled(enabled));
    if enabled{paint_button_hover(ui,&response,theme::CONTROL_RADIUS as f32);}
    let center=hit.center();
    let stroke=Stroke::new(1.5_f32,ink);
    for sign in [-1.0_f32,1.0]{
        ui.painter().with_clip_rect(hit.intersect(ui.clip_rect())).line_segment([
            center+egui::vec2(-4.0,-4.0*sign),
            center+egui::vec2(4.0,4.0*sign)
        ],stroke);
    }
    response
}

/// Shared disclosure chevron, overflow tooltip and hover policy.
pub(crate) fn disclosure_at(
    ui:&Ui,hit:Rect,id:egui::Id,center:egui::Pos2,arrow_clip:Option<Rect>,
    open:bool,overflow:bool,full:&str,hover_radius:f32
)->Response{
    let painter=ui.painter().with_clip_rect(arrow_clip.unwrap_or(ui.clip_rect()));
    paint_management_chevron(&painter,center,if open{
        ManagementChevronDirection::Down
    }else{ManagementChevronDirection::Right});
    let response=ui.interact(hit,id,Sense::click());
    let response=crate::ui_primitives::overflow_tooltip(response,overflow,full);
    paint_button_hover(ui,&response,hover_radius);
    response
}

pub(crate) fn action(ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool) -> Response {
    action_with_min_width(ui, label, role, enabled, 0.0)
}

/// Reuse the exact same action design, filling only the column assigned by a
/// shared button-group component. The surrounding pane geometry is untouched.
pub(crate) fn action_full_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool,
) -> Response {
    let width = ui.available_width();
    action_with_min_width(ui, label, role, enabled, width)
}

/// Shared fixed-minimum-width action, for select triggers that must match
/// the 34pt management controls rather than egui's shorter default ComboBox.
pub(crate) fn action_with_min_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool, min_width: f32,
) -> Response {
    action_with_palette(ui, label, role, enabled, min_width, false)
}

/// Actions embedded in colored review cards retain their original role geometry
/// and hover behavior, but use the shared white action palette when enabled.
pub(crate) fn review_action(ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool) -> Response {
    action_with_palette(ui, label, role, enabled, 0.0, true)
}

/// One visual role/state contract for standard widgets and fixed-rect actions.
/// Geometry and interaction remain owned by the respective components.
pub(crate) fn action_style(
    role:ButtonRole,enabled:bool,review_white:bool
)->(f32,Color32,Color32,Stroke){
    theme::button_visual(role,theme::VisualState::enabled(enabled),review_white)
}

/// Field-like actions (calendar and dropdown) share the disabled action
/// palette but retain the approved enabled input surface and semantic ink.
pub(crate) fn field_action_palette(enabled:bool,enabled_ink:Color32)
    ->(Color32,Color32,Stroke){
    theme::field_visual(theme::VisualState::enabled(enabled),enabled_ink)
}

fn action_with_palette(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool, min_width: f32,
    review_white: bool,
) -> Response {
    let effective_enabled=enabled && ui.is_enabled();
    let (height,fill,color,stroke)=action_style(role,effective_enabled,review_white);
    let light_surface=effective_enabled&&fill==theme::BRIGHT;
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
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => crate::highlight::NAVIGATION_RADIUS,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => crate::highlight::COMPACT_RADIUS,
        _ => crate::highlight::CONTROL_RADIUS,
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
        let mut button = egui::Button::new(
            typography::action_label(label, font_size, important, color)
        )
            .corner_radius(CornerRadius::same(radius))
            .min_size(Vec2::new(min_width, height));
        if effective_enabled{
            crate::highlight::action_widget(ui,fill,stroke,light_surface);
        }else{button=button.fill(fill).stroke(stroke);}
        ui.add_enabled(enabled, button)
    }).inner;
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("button",response.rect,ui.clip_rect(),Some(height));
    if effective_enabled {
        crate::floating_notice::remember_response(ui.ctx(),&response);
    }
    response
}

/// Frameless Windows controls share a single 32pt hit area and precise
/// vector marks. Unicode "-", "□" and "×" had different font baselines.
#[derive(Clone, Copy)]
pub(crate) enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

pub(crate) fn window_control(ui: &mut Ui, control: WindowControl) -> Response {
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

/// Fixed notification surface: bell and count own disjoint slots in all
/// unread states. The ordinary action renderer owns the frame and hover.
fn notification_button_slots(rect:Rect,count_width:f32)->(Rect,Rect){
    let bell=Rect::from_min_size(rect.min,Vec2::new(ACTION_STANDARD_HEIGHT,rect.height()));
    let count=Rect::from_min_max(
        egui::pos2(bell.right()+SPACE_4,rect.top()),
        egui::pos2((bell.right()+SPACE_4+count_width).min(rect.right()),rect.bottom())
    );
    (bell,count)
}
pub(crate) fn notification_button(ui:&mut Ui,unread:usize)->Response{
    let max_count=typography::client_table_metadata_galley(ui,"99+",ERROR_TEXT);
    let width=ACTION_STANDARD_HEIGHT+max_count.size().x+SPACE_8;
    let response=action_with_min_width(ui,"",ButtonRole::Standard,true,width);
    let (bell,count)=notification_button_slots(response.rect,max_count.size().x);
    let center=bell.center();
    let ink=theme::icon_ink(theme::IconRole::Interactive,theme::VisualState::pointer(true,response.hovered()));
    let stroke=Stroke::new(1.55_f32,ink);
    let p=|x:f32,y:f32|center+egui::vec2(x,y);
    let painter=ui.painter();
    // Native vector bell, independent of platform emoji/font availability.
    painter.add(egui::Shape::line(vec![p(-7.0,4.0),p(-5.0,1.0),p(-5.0,-3.0),
        p(-3.5,-6.0),p(0.0,-7.5),p(3.5,-6.0),p(5.0,-3.0),
        p(5.0,1.0),p(7.0,4.0),p(-7.0,4.0)],stroke));
    painter.add(egui::Shape::line(vec![p(-2.0,6.0),p(0.0,7.5),p(2.0,6.0)],stroke));
    if unread>0{
        let label=if unread>99{"99+".to_owned()}else{unread.to_string()};
        let galley=typography::client_table_metadata_galley(ui,&label,ERROR_TEXT);
        let origin=count.center()-galley.size()/2.0;
        painter.with_clip_rect(count.intersect(ui.clip_rect())).galley(origin,galley,ERROR_TEXT);
    }
    response
}
#[cfg(test)]
mod notification_button_geometry_tests{
    use super::*;
    #[test]
    fn bell_and_maximum_badge_have_disjoint_fixed_slots(){
        let width=26.0;
        let rect=Rect::from_min_size(egui::Pos2::ZERO,
            Vec2::new(ACTION_STANDARD_HEIGHT+width+SPACE_8,ACTION_STANDARD_HEIGHT));
        let (bell,count)=notification_button_slots(rect,width);
        assert!(bell.right()<count.left());
        assert!(rect.contains_rect(bell)&&rect.contains_rect(count));
        assert_eq!(count.width(),width);
    }
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
pub(crate) fn icon_action_button(
    ui:&mut Ui,label:&str,enabled:bool
)->Response{
    let response=compact_action_surface(ui,enabled,egui::Button::new(label)
        .frame(false)
        .min_size(Vec2::splat(theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH)));
    if enabled{
        crate::button::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    response
}

/// Native navigation buttons retain their existing optional permission
/// scope. Fixed-rectangle step buttons deliberately use interact instead.
fn navigation_native_button(
    ui:&mut Ui,size:Vec2,button:egui::Button<'_>,enabled:Option<bool>
)->Response{
    let place=|slot:&mut Ui|slot.add_sized(size,button);
    match enabled{
        Some(enabled)=>ui.add_enabled_ui(enabled,place).inner,
        None=>place(ui),
    }
}


/// Neutral secondary actions inside a workflow card share one compact
/// Standard-size control. Keep their existing disabled functionality.
#[derive(Clone, Copy)]
pub(crate) enum WorkflowAuxiliaryAction {
    Manage,
    Delete,
    PreviousStage,
}

/// Shared label and glyph painting for allocated and fixed-rectangle workflow controls.
fn paint_workflow_auxiliary_content(
    painter:&egui::Painter,center:egui::Pos2,
    action:WorkflowAuxiliaryAction,ink:Color32
){
    match action{
        WorkflowAuxiliaryAction::Manage=>{
            painter.text(center,egui::Align2::CENTER_CENTER,"관리",
                egui::FontId::proportional(typography::FONT_STANDARD_ACTION),ink);
        }
        WorkflowAuxiliaryAction::Delete=>
            paint_workflow_icon(painter,center,WorkflowIcon::Trash,ink),
        WorkflowAuxiliaryAction::PreviousStage=>
            paint_workflow_icon(painter,center,WorkflowIcon::PreviousStage,ink),
    }
}

/// Fixed-size workflow actions share one renderer; callers retain workflow IDs
/// and business transitions. Stage controls keep hover when disabled because
/// they also open the existing order preview.
#[derive(Clone, Copy)]
pub(crate) enum WorkflowFixedAction<'a> {
    Auxiliary(WorkflowAuxiliaryAction),
    Stage(&'a str),
}

pub(crate) fn workflow_fixed_action(
    ui:&mut Ui,id:egui::Id,rect:Rect,
    action:WorkflowFixedAction<'_>,enabled:bool
)->Response{
    let response=ui.interact(
        rect,id,if enabled{Sense::click()}else{Sense::hover()}
    );
    let stage=matches!(action,WorkflowFixedAction::Stage(_));
    let role=if stage{theme::ButtonRole::Emphasized}else{theme::ButtonRole::Standard};
    let (_,fill,ink,stroke)=crate::button::action_style(role,enabled,false);
    let light_surface=enabled&&fill==theme::BRIGHT;
    let radius=crate::highlight::NAVIGATION_RADIUS;
    let rest_stroke=if !stage || !enabled{stroke}else{Stroke::NONE};
    crate::highlight::fixed_action_surface(ui,&response,radius,fill,rest_stroke,light_surface,enabled||stage);
    let painter=ui.painter();
    match action{
        WorkflowFixedAction::Auxiliary(action)=>
            paint_workflow_auxiliary_content(painter,rect.center(),action,ink),
        WorkflowFixedAction::Stage(text)=>{
            let galley=painter.layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(typography::FONT_STANDARD_ACTION),ink
            );
            painter.with_clip_rect(rect.shrink2(Vec2::new(theme::SPACE_8,0.0)))
                .galley(egui::Pos2::new(
                    rect.center().x-galley.size().x/2.0,
                    rect.center().y-galley.size().y/2.0
                ),galley,ink);
        }
    }
    if enabled || stage{
        crate::floating_notice::remember_response(ui.ctx(),&response);
    }
    response
}

/// Common management date group used by Order History and Excel History.
/// The shared component owns the outer 36pt frame, 34pt content clip,
/// asymmetric end caps, divider-safe hover geometry and arrow rendering.
/// Consumers own only the date value, callbacks and calendar popup.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum ManagementDateSegmentEdge{Leading,Middle,Trailing}

fn management_date_segment_corners(
    edge:ManagementDateSegmentEdge
)->egui::CornerRadius{
    let r=theme::CONTROL_RADIUS.saturating_sub(
        theme::FRAME_STROKE_WIDTH as u8
    );
    match edge {
        ManagementDateSegmentEdge::Leading=>egui::CornerRadius{
            nw:r,ne:0,sw:r,se:0
        },
        ManagementDateSegmentEdge::Middle=>egui::CornerRadius::same(0),
        ManagementDateSegmentEdge::Trailing=>egui::CornerRadius{
            nw:0,ne:r,sw:0,se:r
        },
    }
}

/// Keep the date segment's hit target, fill, asymmetric rounding and hover
/// tied to one component. The caller must not paint a second hover layer.
pub(crate) fn management_date_segment(
    ui:&mut Ui,label:&str,width:f32,edge:ManagementDateSegmentEdge
)->Response{
    let corners=management_date_segment_corners(edge);
    let direction=match label {
        "‹"=>Some(ManagementChevronDirection::Left),
        "›"=>Some(ManagementChevronDirection::Right),
        _=>None
    };
    let display=if direction.is_some(){""}else{label};
    let response=navigation_native_button(
        ui,Vec2::new(width,theme::ACTION_STANDARD_HEIGHT),
        egui::Button::new(typography::action_label(
            display,typography::FONT_STANDARD_ACTION,false,theme::TEXT
        ))
            .fill(theme::INPUT)
            .stroke(Stroke::NONE)
            .corner_radius(corners),
        None
    );
    crate::button::paint_button_hover_corners(ui,&response,corners);
    if let Some(direction)=direction{
        let painter=ui.painter().with_clip_rect(response.rect);
        paint_management_chevron(
            &painter,response.rect.center(),direction
        );
    }
    response
}

/// The single date-navigation surface used by calendar arrows and compact
/// count steppers. Geometry, border, fill, radius and hover treatment must not
/// drift between controls that share the date-picker visual language.
#[derive(Clone,Copy)]
pub(crate) enum DateNavigationSegmentGlyph{
    CountStep { increase: bool },
    Chevron(ManagementChevronDirection),
    CompactVerticalChevron { up: bool },
}

/// Count symbols are geometric strokes, not unrelated font glyphs.
/// Both horizontal bars have the same length and the same center line.
fn paint_count_step_symbol(painter:&egui::Painter,center:egui::Pos2,increase:bool,ink:Color32){
    let half=4.0;
    let stroke=Stroke::new(1.8_f32,ink);
    painter.line_segment(
        [center-egui::vec2(half,0.0),center+egui::vec2(half,0.0)],stroke
    );
    if increase{
        painter.line_segment(
            [center-egui::vec2(0.0,half),center+egui::vec2(0.0,half)],stroke
        );
    }
}

/// A borderless segment button; its parent owns the only outer border.
/// The horizontal navigator uses native allocation. The two compact
/// stepper forms share exact-rect hit testing and hover painting.
pub(crate) fn date_navigation_segment_button(
    ui:&mut Ui,width:f32,enabled:bool,
    glyph:DateNavigationSegmentGlyph
)->Response{
    let enabled=enabled && ui.is_enabled();
    if let DateNavigationSegmentGlyph::Chevron(direction)=glyph{
        let height=ui.available_height();
        let fill=theme::surface_palette(theme::SurfaceRole::NavigationSegment,theme::VisualState::enabled(enabled)).fill;
        let response=navigation_native_button(
            ui,Vec2::new(width,height),
            egui::Button::new("").fill(fill).stroke(Stroke::NONE).corner_radius(0),
            Some(enabled)
        );
        let ink=theme::icon_ink(theme::IconRole::Navigation,theme::VisualState::pointer(enabled,response.hovered()));
        if enabled{crate::button::paint_button_hover(ui,&response,0.0);}
        let painter=ui.painter().with_clip_rect(response.rect);
        paint_management_chevron_with_color(&painter,response.rect.center(),direction,ink);
        return response;
    }
    // Preserve the established IDs, exact hit area and separate corner
    // geometry of the horizontal count and half-height vertical steppers.
    let rect=ui.max_rect().intersect(ui.clip_rect());
    let (id,corners)=match glyph{
        DateNavigationSegmentGlyph::CompactVerticalChevron{up}=>(
            "compact-chevron",egui::CornerRadius{
                nw:if up{3}else{0},ne:0,sw:if up{0}else{3},se:0
            }
        ),
        DateNavigationSegmentGlyph::CountStep{increase}=>{
            let radius=theme::CONTROL_RADIUS.saturating_sub(2);
            ("count-step",egui::CornerRadius{
                nw:if increase{0}else{radius},sw:if increase{0}else{radius},
                ne:if increase{radius}else{0},se:if increase{radius}else{0},
            })
        }
        DateNavigationSegmentGlyph::Chevron(_)=>unreachable!("handled above"),
    };
    let response=ui.interact(rect,ui.id().with(id),
        if enabled{Sense::click()}else{Sense::hover()});
    let painter=ui.painter().with_clip_rect(rect);
    let (fill,ink)=match glyph{
        DateNavigationSegmentGlyph::CompactVerticalChevron{..}=>{
            let ink=theme::icon_ink(theme::IconRole::Navigation,theme::VisualState::pointer(enabled,response.hovered()));
            (if enabled{None}else{Some(theme::RECEPTION_RECIPIENT_NAV_DISABLED_BG)},ink)
        }
        DateNavigationSegmentGlyph::CountStep{..}=>{
            let (_,fill,ink,_)=crate::button::action_style(theme::ButtonRole::Standard,enabled,false);
            (Some(fill),ink)
        }
        DateNavigationSegmentGlyph::Chevron(_)=>unreachable!("handled above"),
    };
    if let Some(fill)=fill{painter.rect_filled(rect,corners,fill);}
    if enabled{crate::button::paint_button_hover_corners(ui,&response,corners);}
    match glyph{
        DateNavigationSegmentGlyph::CompactVerticalChevron{up}=>{
            let angle=if up{-std::f32::consts::FRAC_PI_2}else{std::f32::consts::FRAC_PI_2};
            paint_chevron_at_angle_scaled(&painter,rect.center(),angle,0.0,ink,0.72);
        }
        DateNavigationSegmentGlyph::CountStep{increase}=>
            paint_count_step_symbol(&painter,rect.center(),increase,ink),
        DateNavigationSegmentGlyph::Chevron(_)=>unreachable!("handled above"),
    }
    response
}

pub(crate) fn date_navigation_button_surface(
    ui:&mut Ui,enabled:bool
)->(Response,Color32){
    let effective_enabled=enabled && ui.is_enabled();
    let (fill,ink,stroke)=crate::button::field_action_palette(effective_enabled,theme::SECONDARY);
    let response=ui.scope(|ui|{
        let mut button=egui::Button::new("").corner_radius(crate::highlight::CONTROL_RADIUS);
        if effective_enabled{
            crate::highlight::action_widget(ui,fill,stroke,false);
        }else{button=button.fill(fill).stroke(stroke);}
        navigation_native_button(ui,Vec2::splat(theme::CALENDAR_NAV_SIZE),button,Some(effective_enabled))
    }).inner;
    let ink=if effective_enabled && response.hovered(){
        theme::TEXT
    }else{ink};
    if effective_enabled{
        crate::floating_notice::remember_response(ui.ctx(),&response);
    }
    (response,ink)
}


/// Calendar cells use the shared action hover policy without changing grid geometry.
pub(crate) fn calendar_day(grid:&mut Ui,day:&str,width:f32,selected:bool,today:bool)->Response{
    let response=grid.scope(|ui|{
        crate::highlight::calendar_widget(ui,selected,today);
        ui.add_sized(egui::vec2(width,width),
            egui::Button::new(typography::body(day)).corner_radius(crate::highlight::CONTROL_RADIUS))
    }).inner;
    if grid.is_enabled(){crate::floating_notice::remember_response(grid.ctx(),&response);}
    response
}

/// Dispatch-origin toggle; the reception row retains its business selection and highlight.
pub(crate) fn dispatch_origin(right:&mut Ui,label:&str,selected:bool,editable:bool)->Response{
    let effective_enabled=editable&&right.is_enabled();
    let response=right.scope(|ui|{
        let fill=if selected{theme::RAISED}else{theme::PANEL};
        let stroke=Stroke::new(theme::FRAME_STROKE_WIDTH,
            if selected{theme::SELECTION}else{theme::DIVIDER});
        let mut button=egui::Button::new(typography::action_label(
            label,typography::FONT_STANDARD_ACTION,false,
            if selected{theme::TEXT}else{theme::SECONDARY}
        )).min_size(Vec2::new(42.0,theme::ACTION_STANDARD_HEIGHT))
         .corner_radius(crate::highlight::CONTROL_RADIUS);
        if effective_enabled{crate::highlight::action_widget(ui,fill,stroke,false);}
        else{button=button.fill(fill).stroke(stroke);}
        ui.add_enabled(editable,button)
    }).inner;
    if effective_enabled{crate::floating_notice::remember_response(right.ctx(),&response);}
    response
}

#[cfg(test)]
mod shared_action_style_tests{
    use super::*;
    use crate::ui_primitives::button_hover_palette;

    #[test]
    fn blue_pointer_hover_and_keyboard_focus_have_independent_paint(){
        let hover_border=Stroke::new(BUTTON_HOVER_BORDER_WIDTH,SELECTION);
        let focus_border=hover_border;
        assert_ne!(BUTTON_HOVER_LIGHT_BG,BRIGHT);
        assert_eq!(BUTTON_HOVER_LIGHT_BG,Color32::from_rgb(143,197,255));
        assert_eq!(button_hover_palette(false,false),(None,None));
        assert_eq!(button_hover_palette(true,false),(Some(BUTTON_HOVER_TINT),Some(hover_border)));
        assert_eq!(button_hover_palette(false,true),(None,Some(focus_border)));
        assert_eq!(button_hover_palette(true,true),(Some(BUTTON_HOVER_TINT),Some(focus_border)));
        let ctx=egui::Context::default();
        theme::install(&ctx);
        let style=ctx.style();
        assert_eq!(style.visuals.widgets.hovered.bg_fill,BUTTON_HOVER_BG);
        assert_eq!(style.visuals.widgets.hovered.weak_bg_fill,BUTTON_HOVER_BG);
        assert_eq!(style.visuals.widgets.hovered.bg_stroke,
            Stroke::new(BUTTON_HOVER_BORDER_WIDTH,SELECTION));
        assert_eq!(style.visuals.widgets.hovered.expansion,0.0);
    }

    #[test]
    fn fixed_and_native_actions_use_the_same_standard_and_emphasized_palette(){
        let (_,fill,ink,stroke)=action_style(ButtonRole::Standard,true,false);
        assert_eq!((fill,ink,stroke),(RAISED,TEXT,Stroke::new(FRAME_STROKE_WIDTH,BORDER)));
        let (_,fill,ink,stroke)=action_style(ButtonRole::Emphasized,true,false);
        assert_eq!((fill,ink,stroke),(BRIGHT,BRIGHT_TEXT,Stroke::NONE));
        for role in [ButtonRole::Standard,ButtonRole::Emphasized,ButtonRole::TableSecondary]{
            let (_,fill,ink,stroke)=action_style(role,false,false);
            assert_eq!((fill,ink,stroke),(DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,
                Stroke::new(FRAME_STROKE_WIDTH,DISABLED_ACTION_BORDER)));
        }
    }

    #[test]
    fn special_review_and_field_roles_retain_their_own_appearance(){
        let (_,fill,ink,stroke)=action_style(ButtonRole::Standard,true,true);
        assert_eq!((fill,ink,stroke),(BRIGHT,BRIGHT_TEXT,Stroke::new(1.0,BORDER)));
        let (_,fill,ink,stroke)=action_style(ButtonRole::Standard,false,true);
        assert_eq!((fill,ink,stroke),(DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,Stroke::new(1.0,BORDER)));
        let (_,fill,ink,stroke)=action_style(ButtonRole::Ghost,false,false);
        assert_eq!((fill,ink,stroke),(Color32::TRANSPARENT,DISABLED_ACTION_TEXT,Stroke::NONE));
        for ink in [TEXT,SECONDARY]{
            assert_eq!(field_action_palette(true,ink),(INPUT,ink,Stroke::new(SOFT_DIVIDER_WIDTH,BORDER)));
            assert_eq!(field_action_palette(false,ink),(DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,
                Stroke::new(FRAME_STROKE_WIDTH,DISABLED_ACTION_BORDER)));
        }
    }
}

// Shared controls and presentation profiles.
/// A compact KakaoTalk identity mark, used only as a visual menu prefix.
/// The native Kakao HWND and order-matching logic remain unchanged.
pub(crate) fn kakao_menu_icon(ui:&mut Ui) -> Response {
    let side=theme::KAKAO_MENU_ICON_SIZE;
    let (rect,response)=ui.allocate_exact_size(Vec2::splat(side),Sense::hover());
    let painter=ui.painter();
    painter.rect_filled(rect,4.0,theme::KAKAO_ICON_YELLOW);
    let bubble=Rect::from_min_max(
        egui::pos2(rect.left()+3.6,rect.top()+4.7),
        egui::pos2(rect.right()-3.6,rect.bottom()-5.8),
    );
    painter.rect_filled(bubble,5.0,theme::KAKAO_ICON_INK);
    painter.add(egui::Shape::convex_polygon(vec![
        egui::pos2(rect.left()+6.0,rect.bottom()-6.5),
        egui::pos2(rect.left()+5.3,rect.bottom()-3.0),
        egui::pos2(rect.left()+9.2,rect.bottom()-6.3),
    ],theme::KAKAO_ICON_INK,Stroke::NONE));
    for dx in [-2.8,0.0,2.8] {
        painter.circle_filled(
            egui::pos2(rect.center().x+dx,rect.center().y-0.8),
            0.75,theme::KAKAO_ICON_YELLOW
        );
    }
    response
}

/// Keep paired reception actions at their content widths. Available pane
/// space stays empty rather than stretching two buttons to the pane edges.
/// The row wraps only if Korean labels cannot fit without clipping.
pub(crate) fn paired_actions(
    ui: &mut Ui,
    left: (&str, theme::ButtonRole, bool),
    right: (&str, theme::ButtonRole, bool),
) -> (Response, Response) {
    ui.horizontal_wrapped(|ui| {
        (
            action(ui, left.0, left.1, left.2),
            action(ui, right.0, right.1, right.2),
        )
    }).inner
}

/// Shared management tab roles (content width, no full-page stretching).
pub(crate) fn management_tabs(ui:&mut Ui,labels:&[(&str,bool,bool)]) -> Option<usize> {
    let mut pressed=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        for (index,(title,active,enabled)) in labels.iter().enumerate() {
            let role=if *active {theme::ButtonRole::ActiveManagementTab}
                else {theme::ButtonRole::ManagementTab};
            if action(bar,title,role,*enabled).clicked() {
                pressed=Some(index);
            }
        }
    });
    pressed
}

/// The add/rollback modes occupy exactly the same header action slot.
/// Never insert a pending-assignment action between the item and freight rows.
pub(crate) fn reception_item_header_action(ui:&mut Ui,rollback:bool,enabled:bool)->Response{
    action_with_min_width(
        ui,if rollback{"선택 취소"}else{"＋ 상품 추가"},
        theme::ButtonRole::Standard,enabled,112.0
    )
}


/// Native CollapsingHeader keeps its original click/keyboard/state/indent
/// behavior; only its default filled triangle becomes the common line icon.
/// Rotate smoothly with egui's animated openness (0 = right, 1 = down).
pub(crate) fn collapsing_header(title:impl Into<egui::WidgetText>)->egui::CollapsingHeader{
    egui::CollapsingHeader::new(title).icon(paint_collapsing_header_icon)
}


fn paint_collapsing_header_icon(
    ui:&mut Ui,openness:f32,response:&Response
){
    let t=openness.clamp(0.0,1.0);
    crate::ui_primitives::paint_chevron_at_angle(
        ui.painter(),response.rect.center(),
        t*std::f32::consts::FRAC_PI_2,t,theme::SECONDARY
    );
}

#[cfg(test)]
mod regression_tests;
