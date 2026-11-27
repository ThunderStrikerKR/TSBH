//! Low-level UI primitives independent of composite screens and controls.
use eframe::egui::{self,Align,Color32,Layout,Rect,Response,Sense,Stroke,Ui,UiBuilder,Vec2};

use crate::{theme,typography};

/// The lowest-level explicit vertical gap rule, also used by card layout.
pub(crate) fn apply_explicit_vertical_spacing(ui:&mut Ui){
    ui.spacing_mut().item_spacing.y=0.0;
}


 
/// Shared state caption for content-sized lists and menus with no reserved body.
/// Fixed viewports use management_body_state_overlay instead.
pub fn management_body_state_inline(ui:&mut Ui,caption:&str){
    ui.label(typography::caption(caption));
}

/// Draw an error within an existing body rectangle without advancing its
/// layout cursor. The full translated text is also available on hover when
/// the visible rectangle is too short to contain the wrapped text.
pub fn text_exceeds_rect(size:Vec2,rect:Rect)->bool{
    size.x>rect.width()+0.5 || size.y>rect.height()+0.5
}

/// One eligibility rule for all explicit overflow tooltips.
fn overflow_tooltip_needed(overflow:bool,full:&str)->bool{
    overflow&&!full.trim().is_empty()
}

/// Labels with only native elision feedback use the native owner; labels
/// with explicit full text/overflow feedback MUST turn the native owner off.
/// Both paths keep identical label sizing, painting and interaction.
pub fn truncated_label(text:impl Into<egui::WidgetText>)->egui::Label{
    egui::Label::new(text).truncate()
}
pub fn label_for_explicit_tooltip(text:impl Into<egui::WidgetText>)->egui::Label{
    egui::Label::new(text).show_tooltip_when_elided(false)
}
pub fn truncated_label_for_explicit_tooltip(text:impl Into<egui::WidgetText>)->egui::Label{
    label_for_explicit_tooltip(text).truncate()
}

/// Attach a tooltip to an existing interactive widget without changing its sense.
pub fn overflow_tooltip(response:Response,overflow:bool,full:&str)->Response{
    if overflow_tooltip_needed(overflow,full)
        && response.rect.width()>0.0 && response.rect.height()>0.0{
        if crate::overlay::tooltip_allowed(&response){response.on_hover_text(full)}else{response}
    }else{response}
}

/// Deliberately shortened text is also overflow, even if its ellipsis fits.
pub fn shortened_text_tooltip(response:Response,visible:&str,full:&str)->Response{
    overflow_tooltip(response,visible!=full,full)
}

/// Only create a hover widget for a nonempty, visible overflowing region.
/// Existing click responses should instead use overflow_tooltip.
pub fn overflow_region_tooltip(
    ui:&Ui,rect:Rect,id:egui::Id,overflow:bool,full:&str
){
    if !overflow_tooltip_needed(overflow,full){return;}
    let visible=rect.intersect(ui.clip_rect());
    if visible.width()<=0.0||visible.height()<=0.0{return;}
    overflow_tooltip(ui.interact(visible,id,Sense::hover()),overflow,full);
}

/// Compare the actual positioned text rectangle, including native elision.
pub fn positioned_text_overflow(text:Rect,visible:Rect,elided:bool)->bool{
    elided || !visible.expand(0.5).contains_rect(text)
}

#[derive(Clone, Copy)]
pub enum WorkflowIcon {
    Collapsed,
    Expanded,
    PreviousStage,
    Trash,
}

/// Draw workflow glyphs as stable vectors; the UI font may not contain
/// Unicode chevrons, trash or curved-arrow symbols on Windows.
pub fn paint_workflow_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    icon: WorkflowIcon,
    ink: Color32,
) {
    let stroke = Stroke::new(1.55_f32, ink);
    let p = |x: f32, y: f32| egui::pos2(center.x + x, center.y + y);
    match icon {
        WorkflowIcon::Collapsed => {
            paint_chevron_at_angle(painter,center,0.0,0.0,ink);
        }
        WorkflowIcon::Expanded => {
            paint_chevron_at_angle(
                painter,center,std::f32::consts::FRAC_PI_2,1.0,ink
            );
        }
        WorkflowIcon::PreviousStage => {
            painter.line_segment([p(-5.3, -1.6), p(-1.5, -5.2)], stroke);
            painter.line_segment([p(-5.3, -1.6), p(-1.0, 1.8)], stroke);
            painter.add(egui::Shape::line(
                vec![p(-4.4,-1.6), p(1.2,-1.6), p(4.2,1.0),p(4.2,5.0)],
                stroke
            ));
        }
        WorkflowIcon::Trash => {
            for (a,b) in [
                (p(-5.0,-4.1),p(5.0,-4.1)),
                (p(-2.9,-6.9),p(2.9,-6.9)),
                (p(-4.3,-1.5),p(-3.4,6.2)),
                (p(4.3,-1.5),p(3.4,6.2)),
                (p(-3.4,6.2),p(3.4,6.2)),
                (p(-1.3,-1.2),p(-1.3,3.9)),
                (p(1.3,-1.2),p(1.3,3.9))
            ] {painter.line_segment([a,b],stroke);}
        }
    }
}

/// One 10pt / 1.8pt-stroke chevron for selectors, date stepping and
/// expand/collapse affordances. Rotate this same vector for every screen.
/// Draw the TWO diagonal arms as ONE continuous path. Separate
/// line_segment() calls blend anti-aliased coverage twice at their shared
/// corner, making the arrow tip darker and giving inconsistent edge colors
/// on Windows / fractional DPI. A single path has one joined stroke.
#[derive(Clone,Copy)]
pub(crate) enum ManagementChevronDirection { Down, Left, Right }

pub(crate) fn paint_chevron_at_angle(
    painter:&egui::Painter,center:egui::Pos2,angle:f32,optical_raise:f32,
    ink:Color32
){
    paint_chevron_at_angle_scaled(painter,center,angle,optical_raise,ink,1.0);
}

/// Shared chevron geometry, scaled only for constrained half-height segments.
pub(crate) fn paint_chevron_at_angle_scaled(
    painter:&egui::Painter,center:egui::Pos2,angle:f32,optical_raise:f32,
    ink:Color32,scale:f32
){
    let half_span=5.0*scale;
    let depth=2.5*scale;
    let stroke=(1.8*scale).max(1.0);
    let (s,c)=angle.sin_cos();
    let rotated=|x:f32,y:f32|egui::pos2(
        center.x+x*c-y*s,
        center.y+x*s+y*c-optical_raise
    );
    let a=rotated(-depth,-half_span);
    let tip=rotated(depth,0.0);
    let b=rotated(-depth,half_span);
    // One tessellated stroke owns the corner, diagonal edges and caps.
    // Preserve the approved 10pt shape, rotation and 1.8pt thickness.
    painter.add(egui::Shape::line(
        vec![a,tip,b],Stroke::new(stroke,ink)
    ));
}

pub(crate) fn paint_management_chevron_with_color(
    painter:&egui::Painter,center:egui::Pos2,
    direction:ManagementChevronDirection,color:Color32
){
    let (angle,raise)=match direction {
        ManagementChevronDirection::Right=>(0.0,0.0),
        ManagementChevronDirection::Down=>(std::f32::consts::FRAC_PI_2,1.0),
        ManagementChevronDirection::Left=>(std::f32::consts::PI,0.0),
    };
    paint_chevron_at_angle(painter,center,angle,raise,color);
}

pub(crate) fn paint_management_chevron(
    painter:&egui::Painter,center:egui::Pos2,direction:ManagementChevronDirection
){
    paint_management_chevron_with_color(
        painter,center,direction,theme::icon_ink(theme::IconRole::Secondary,theme::VisualState::Normal)
    );
}

/// A single outer frame with zero-gap internal segments. Both the recipient
/// navigator and shipment-count editor use this exact date-selector surface.
pub(crate) fn date_navigation_segmented_surface<R>(
    ui:&mut Ui,id_salt:impl std::hash::Hash,size:Vec2,
    add_segments:impl FnOnce(&mut Ui,Rect)->R
)->(Rect,R){
    date_navigation_segmented_surface_with_state(ui,id_salt,size,None,add_segments,|_|crate::highlight::States::default())
}
/// A single outer outline chosen after the segment interaction is known.
/// The child never draws a second focus outline around the same surface.
pub(crate) fn date_navigation_segmented_surface_with_state<R>(
    ui:&mut Ui,id_salt:impl std::hash::Hash,size:Vec2,base_role:Option<crate::highlight::Role>,
    add_segments:impl FnOnce(&mut Ui,Rect)->R,
    state:impl FnOnce(&R)->crate::highlight::States
)->(Rect,R){
    let (outer,_)=ui.allocate_exact_size(size,Sense::hover());
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("segmented frame",outer,ui.clip_rect(),None);
    let inner=outer.shrink(theme::FRAME_STROKE_WIDTH);
    if let Some(role)=base_role{
        crate::highlight::field_background(ui,outer,role,true);
    }else{
        ui.painter().rect_filled(outer,crate::highlight::Geometry::Control.corners(),theme::surface_palette(theme::SurfaceRole::Input,theme::VisualState::Normal).fill);
    }
    // The parent has already allocated the complete frame. A scoped UI
    // would advance its cursor again using the children's expanded bounds.
    let mut segments=navigation_segment_child(
        ui,id_salt,inner,Layout::left_to_right(Align::Center)
    );
    segments.apply_shared_width(CardWidthPolicy::Fixed,inner.width());
    segments.set_height(inner.height());
    segments.spacing_mut().item_spacing=Vec2::ZERO;
    let content=add_segments(&mut segments,inner);
    let stroke=crate::highlight::prioritized_stroke(
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::border_ink(theme::BorderRole::Standard)),state(&content));
    crate::highlight::paint_parts(ui.painter(),crate::highlight::segment_outline_rect(outer),
        crate::highlight::Geometry::Control,None,Some(stroke));
    (outer,content)
}

/// A fixed segment gets its own child ID and clipped rectangle without
/// advancing the parent layout. The caller retains the appropriate layout.
pub(crate) fn navigation_segment_child(
    parent:&mut Ui,id:impl std::hash::Hash,rect:Rect,layout:Layout
)->Ui{
    let mut child=parent.new_child(UiBuilder::new().id_salt(id)
        .max_rect(rect).layout(layout));
    child.set_clip_rect(rect.intersect(parent.clip_rect()));
    child
}

/// Structural section divider: one 12/1/12pt allocation, separate from row rules.
/// Ordinary vertical layouts already reserve item_spacing on both sides.
/// Count that spacing toward the 12pt clearance, not on top of it.
pub fn section_divider(ui:&mut Ui)->Rect{
    let implicit=ui.spacing().item_spacing.y;
    debug_assert!(implicit>=0.0 && implicit<=theme::SECTION_DIVIDER_GAP);
    let gap=(theme::SECTION_DIVIDER_GAP-implicit).max(0.0);
    allocate_section_divider(ui,gap,gap).0
}

/// Shared renderer also supports a preceding inset already consumed by the
/// toolbar, or a reserved missing-toolbar slot. The bottom gap never varies.
pub(crate) fn section_divider_with_top_gap(ui:&mut Ui,top_gap:f32)->(Rect,Rect){
    debug_assert!(ui.spacing().item_spacing.y.abs()<0.01);
    allocate_section_divider(ui,top_gap,theme::SECTION_DIVIDER_GAP)
}

/// Symmetric section-header clearance, with explicit spacing owned by the caller.
pub(crate) fn section_divider_symmetric(ui:&mut Ui,gap:f32)->(Rect,Rect){
    debug_assert!(ui.spacing().item_spacing.y.abs()<0.01);
    allocate_section_divider(ui,gap,gap)
}

fn allocate_section_divider(ui:&mut Ui,top_gap:f32,bottom_gap:f32)->(Rect,Rect){
    let (strip,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),top_gap+theme::SOFT_DIVIDER_WIDTH
            +bottom_gap),Sense::hover()
    );
    let rule=Rect::from_min_size(
        egui::pos2(strip.left(),strip.top()+top_gap),
        Vec2::new(strip.width(),theme::SOFT_DIVIDER_WIDTH)
    );
    theme::paint_soft_rule(ui.painter(),rule);
    (strip,rule)
}

/// A subtle separation between data belonging to the same record.
/// Sections themselves are framed by the caller.
pub fn soft_divider(ui: &mut Ui) {
    theme::soft_rule(ui);
}

/// A 36pt inline form label. Unlike natural-height ui.label(), the text is
/// measured and painted inside the SAME row height as its sibling TextEdit.
/// Optical adjustment comes from the typography layer, not each screen.
pub fn form_field_label_sized(ui:&mut Ui,label:&str,width:f32)->Response {
    let galley=typography::field_label_galley(ui,label);
    let width=width.max(1.0).min(ui.available_width().max(1.0));
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover()
    );
    let pos=egui::pos2(rect.left(),
        rect.top()+typography::form_label_text_y(rect.height(),galley.size().y));
    let overflow=galley.size().x>rect.width()+0.5;
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(pos,galley,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
    overflow_tooltip(response,overflow,label)
}
pub fn form_field_label(ui:&mut Ui,label:&str)->Response {
    let width=typography::field_label_galley(ui,label).size().x;
    form_field_label_sized(ui,label,width)
}

#[cfg(test)]
mod shared_hover_clip_tests{
    use super::*;
    #[test]
    fn highlight_preserves_parent_clip_without_tight_widget_scissor(){
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let widget=Rect::from_min_size(ui.min_rect().min,Vec2::splat(16.0));
                let painter=crate::highlight::painter(ui,widget).unwrap();
                assert_eq!(painter.clip_rect(),ui.clip_rect());
                assert_ne!(painter.clip_rect(),widget);
                assert!(crate::highlight::painter(ui,Rect::NOTHING).is_none());
            });
        });
    }
}

/// Card width is independent of height. Natural cards do not reserve space;
/// full-width cards reserve a minimum, fixed columns retain their allocation,
/// and bounded report cards additionally constrain wrapping before scrolling.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(crate) enum CardWidthPolicy {
    #[default]
    FitContent,
    FillAvailable,
    Fixed,
    Bounded,
    /// Upper bound only: preserve the content's natural minimum width.
    UpperLimit,
}
pub(crate) fn apply_width_policy(ui:&mut Ui,policy:CardWidthPolicy,target:f32){
    if !target.is_finite(){return;}
    let width=target.max(0.0);
    match policy{
        CardWidthPolicy::FitContent=>{},
        CardWidthPolicy::FillAvailable=>ui.set_min_width(width),
        CardWidthPolicy::Fixed=>ui.set_width(width),
        CardWidthPolicy::UpperLimit=>ui.set_max_width(width),
        CardWidthPolicy::Bounded=>{
            ui.set_width(width);
            ui.set_max_width(width);
        }
    }
}

/// Same policy for owned Ui and borrowed Ui receivers.
pub(crate) trait UiWidthPolicyExt {
    fn apply_shared_width(&mut self,policy:CardWidthPolicy,target:f32);
}
impl UiWidthPolicyExt for Ui {
    fn apply_shared_width(&mut self,policy:CardWidthPolicy,target:f32){
        apply_width_policy(self,policy,target);
    }
}


#[cfg(test)]
mod private_api_contract_overflow_tooltip_policy_rejects_blank_and_fitting_text {
    use super::*;

    #[test]
    fn overflow_tooltip_policy_rejects_blank_and_fitting_text(){
        assert!(!overflow_tooltip_needed(false,"상품명"));
        assert!(!overflow_tooltip_needed(true,""));
        assert!(!overflow_tooltip_needed(true,"  "));
        assert!(overflow_tooltip_needed(true,"상품명 전체"));
    }
}
