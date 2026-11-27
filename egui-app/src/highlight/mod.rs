//! Single paint policy for interaction highlights. Components own geometry and
//! hit testing; this module owns colors, strokes, paint order and clip safety.
use eframe::egui::{self,Color32,CornerRadius,Painter,Rect,Response,Stroke,Ui};
use crate::theme;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Role{
    Hover,LightHover,LinkedPreview,Selected,RequiredReview,RequiredInput,Focus,DropTarget,
    DropHovered,ReviewTint,RowHover,ChooserSelected,
}
#[derive(Clone,Copy)]
pub(crate) struct Palette{
    pub fill:Option<Color32>,pub stroke:Option<Stroke>,pub ink:Option<Color32>,
}
pub(crate) fn palette(role:Role)->Palette{
    use Role::*;
    let (fill,stroke,ink)=match role{
        Hover=>(Some(theme::BUTTON_HOVER_TINT),Some(Stroke::new(theme::BUTTON_HOVER_BORDER_WIDTH,theme::SELECTION)),None),
        LightHover=>(Some(theme::BUTTON_HOVER_LIGHT_BG),Some(Stroke::new(theme::BUTTON_HOVER_BORDER_WIDTH,theme::BUTTON_HOVER_LIGHT_BORDER)),Some(theme::BRIGHT_TEXT)),
        LinkedPreview=>(Some(theme::LINKED_PREVIEW_BG),Some(Stroke::new(theme::LINKED_PREVIEW_BORDER_WIDTH,theme::LINKED_PREVIEW_BORDER)),Some(theme::LINKED_PREVIEW_TEXT)),
        Selected=>(Some(theme::ORANGE_EMPHASIS_BG),Some(Stroke::new(theme::SELECTION_STROKE_WIDTH,theme::SELECTED_ORDER)),Some(theme::ORANGE_EMPHASIS_TEXT)),
        RequiredReview=>(Some(theme::ONE_TOUCH_BG),Some(Stroke::new(theme::ONE_TOUCH_BORDER_WIDTH,theme::ONE_TOUCH_BORDER)),Some(theme::ONE_TOUCH_TEXT)),
        RequiredInput=>(Some(theme::REQUIRED_INPUT_TINT),Some(Stroke::new(theme::REQUIRED_INPUT_STROKE_WIDTH,theme::REQUIRED_INPUT_BORDER)),None),
        Focus=>(None,Some(Stroke::new(theme::BUTTON_HOVER_BORDER_WIDTH,theme::SELECTION)),None),
        DropTarget=>(Some(theme::MANUAL_ASSIGNMENT_TINT),Some(Stroke::new(theme::MANUAL_ASSIGNMENT_STROKE_WIDTH,theme::SELECTION)),None),
        DropHovered=>(Some(theme::BUTTON_HOVER_TINT),Some(Stroke::new(theme::BUTTON_HOVER_BORDER_WIDTH,theme::SELECTION)),None),
        ReviewTint=>(Some(theme::ONE_TOUCH_TINT),Some(Stroke::new(theme::ONE_TOUCH_BORDER_WIDTH,theme::ONE_TOUCH_BORDER)),None),
        // Continuous table rows do not introduce a second border over grid rules.
        RowHover=>(Some(theme::BUTTON_HOVER_BG),None,None),
        ChooserSelected=>(Some(theme::SELECTION.linear_multiply(0.22)),Some(Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SELECTION)),None),
    };
    Palette{fill,stroke,ink}
}
pub(crate) fn fill_shape(rect:Rect,radius:impl Into<CornerRadius>,fill:Color32)->egui::Shape{
    egui::Shape::rect_filled(rect,radius,fill)
}
pub(crate) fn outline_shape(rect:Rect,radius:impl Into<CornerRadius>,stroke:Stroke)->egui::Shape{
    egui::Shape::rect_stroke(rect,radius,stroke,egui::StrokeKind::Inside)
}
pub(crate) fn painter(ui:&Ui,rect:Rect)->Option<Painter>{
    (rect.is_positive()&&rect.intersects(ui.clip_rect())).then(||ui.painter().clone())
}
pub(crate) fn paint_parts(painter:&Painter,rect:Rect,radius:impl Into<CornerRadius>,fill:Option<Color32>,stroke:Option<Stroke>){
    if !rect.is_positive()||!rect.intersects(painter.clip_rect()){return;}
    let radius=radius.into();
    if let Some(fill)=fill{painter.add(fill_shape(rect,radius,fill));}
    if let Some(stroke)=stroke{painter.add(outline_shape(rect,radius,stroke));}
}
pub(crate) fn paint(ui:&Ui,rect:Rect,radius:impl Into<CornerRadius>,role:Role){
    let p=palette(role);
    paint_parts(ui.painter(),rect,radius,p.fill,p.stroke);
}
pub(crate) fn paint_fill(painter:&Painter,rect:Rect,radius:impl Into<CornerRadius>,role:Role){
    paint_parts(painter,rect,radius,palette(role).fill,None);
}
pub(crate) fn paint_outline(painter:&Painter,rect:Rect,radius:impl Into<CornerRadius>,role:Role){
    paint_parts(painter,rect,radius,None,palette(role).stroke);
}
/// Preserve the card's existing status stripe geometry, using the same
/// semantic border color as the associated selection or linked preview.
pub(crate) fn paint_stripe(painter:&Painter,rect:Rect,role:Role){
    if let Some(stroke)=palette(role).stroke{
        paint_parts(painter,rect,0.0,Some(stroke.color),None);
    }
}
pub(crate) fn button_hover_palette(hovered:bool,focused:bool)->(Option<Color32>,Option<Stroke>){
    (if hovered{palette(Role::Hover).fill}else{None},
        if hovered||focused{palette(Role::Focus).stroke}else{None})
}
pub(crate) fn action_hover(ui:&Ui,response:&Response,radius:CornerRadius,light:bool){
    if !response.hovered()&&!response.has_focus(){return;}
    let (fill,stroke)=if light{(None,palette(Role::LightHover).stroke)}
        else{button_hover_palette(response.hovered(),response.has_focus())};
    paint_parts(ui.painter(),response.rect,radius,fill,stroke);
}
/// Native widgets retain hit testing, layout and text; only their scoped
/// visual state is configured here. Light button fill is behind dark text.
pub(crate) fn light_button(ui:&mut Ui,rest:Color32){
    let fill=palette(Role::LightHover).fill.unwrap_or(rest);
    let widgets=&mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill=rest;
    widgets.hovered.weak_bg_fill=fill;
    widgets.active.weak_bg_fill=fill;
    widgets.open.weak_bg_fill=fill;
}
pub(crate) fn dropdown_options(ui:&mut Ui){
    let visuals=ui.visuals_mut();
    let selected=palette(Role::Selected);
    let hover=palette(Role::Hover);
    visuals.selection.bg_fill=selected.fill.unwrap_or(theme::MANAGEMENT_DROPDOWN_SELECTED_BG);
    visuals.selection.stroke=Stroke::new(theme::SELECTION_STROKE_WIDTH,selected.ink.unwrap_or(theme::TEXT));
    // Opaque blue for native option backgrounds, not a tint over text.
    let fill=theme::MANAGEMENT_DROPDOWN_HOVER_BG;
    visuals.widgets.hovered.bg_fill=fill;
    visuals.widgets.hovered.weak_bg_fill=fill;
    visuals.widgets.hovered.bg_stroke=hover.stroke.unwrap_or(Stroke::NONE);
    visuals.widgets.hovered.fg_stroke=Stroke::new(1.5,theme::TEXT);
    visuals.widgets.active.bg_fill=fill;
    visuals.widgets.active.weak_bg_fill=fill;
}
pub(crate) fn checked_checkbox(ui:&mut Ui){
    let widgets=&mut ui.visuals_mut().widgets;
    for visual in [&mut widgets.inactive,&mut widgets.hovered,&mut widgets.active,&mut widgets.open]{
        visual.bg_fill=theme::CHECKBOX_CHECKED_BG;
        visual.weak_bg_fill=theme::CHECKBOX_CHECKED_BG;
        visual.bg_stroke=palette(Role::Selected).stroke.unwrap_or(Stroke::NONE);
        visual.fg_stroke=Stroke::new(theme::CHECKBOX_CHECKED_STROKE_WIDTH,theme::CHECKBOX_CHECKED_MARK);
    }
    // Keep the orange checked fill and check mark; blue outline is hover only.
    widgets.hovered.bg_stroke=palette(Role::Hover).stroke.unwrap_or(Stroke::NONE);
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn semantic_highlight_roles_keep_distinct_state_colors(){
        assert_eq!(palette(Role::Hover).stroke.unwrap().width,2.0);
        assert_eq!(palette(Role::LightHover).fill,Some(theme::BUTTON_HOVER_LIGHT_BG));
        assert_eq!(palette(Role::LightHover).ink,Some(theme::BRIGHT_TEXT));
        assert_eq!(palette(Role::DropHovered).fill,palette(Role::Hover).fill);
        assert_eq!(palette(Role::Selected).fill,Some(theme::ORANGE_EMPHASIS_BG));
        assert_eq!(palette(Role::RequiredInput).stroke.unwrap().color,theme::ERROR_BORDER);
        assert_ne!(palette(Role::LinkedPreview).fill,palette(Role::RequiredReview).fill);
    }
    #[test]
    fn common_painter_preserves_parent_clip(){
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let r=Rect::from_min_size(ui.min_rect().min,egui::Vec2::splat(16.0));
                assert_eq!(painter(ui,r).unwrap().clip_rect(),ui.clip_rect());
                assert!(painter(ui,Rect::NOTHING).is_none());
            });
        });
    }
}
