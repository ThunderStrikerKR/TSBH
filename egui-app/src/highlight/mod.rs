//! Single paint policy for interaction highlights. Components own geometry and
//! hit testing; this module owns colors, strokes, paint order and clip safety.
use eframe::egui::{self,Color32,CornerRadius,Painter,Rect,Response,Stroke,Ui};
use crate::theme;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Role{
    Hover,LightHover,LinkedPreview,Selected,RequiredReview,RequiredInput,Focus,DropTarget,
    DropHovered,ReviewTint,RowHover,ChooserSelected,
}
/// Persistent semantic state takes priority over transient pointer feedback.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(crate) struct States{
    pub required:bool,pub selected:bool,pub linked:bool,pub review:bool,
    pub focused:bool,pub hovered:bool,
}
impl States{
    pub(crate) fn resolve(self)->Option<Role>{
        if self.required{Some(Role::RequiredInput)}
        else if self.selected{Some(Role::Selected)}
        else if self.linked{Some(Role::LinkedPreview)}
        else if self.review{Some(Role::RequiredReview)}
        else if self.focused{Some(Role::Focus)}
        else if self.hovered{Some(Role::Hover)}
        else{None}
    }
}
/// An existing selection retains its established surface palette. A hover
/// never adds a second border or hides a persistent state.
/// Read-only and edit renderers share the same persistent error decision.
/// The editor owns the surface when a required cell is actively edited.
#[derive(Clone,Copy)]
pub(crate) struct FieldHighlights<const N:usize>{
    linked:[bool;N],required:[bool;N],editing:[bool;N],
}
impl<const N:usize> From<[bool;N]> for FieldHighlights<N>{
    fn from(linked:[bool;N])->Self{Self::new(linked,[false;N],[false;N])}
}
impl<const N:usize> FieldHighlights<N>{
    pub(crate) fn new(linked:[bool;N],required:[bool;N],editing:[bool;N])->Self{
        Self{linked,required,editing}
    }
    pub(crate) fn editor_owns(self,index:usize)->bool{self.required[index]&&self.editing[index]}
    pub(crate) fn role(self,index:usize,review:bool)->Option<Role>{
        if self.editor_owns(index){return None;}
        States{required:self.required[index],linked:self.linked[index],review,..Default::default()}.resolve()
    }
}
pub(crate) fn field_radius(role:Role)->CornerRadius{
    CornerRadius::same(if role==Role::LinkedPreview{SOURCE_RADIUS as u8}else{CONTROL_RADIUS})
}
fn composite_opaque(t:Color32,rest:Color32)->Color32{
    let [r,g,b,a]=t.to_array();
    let inv=255-u16::from(a);
    let mix=|src:u8,dst:u8| (u16::from(src)+(u16::from(dst)*inv+127)/255).min(255) as u8;
    Color32::from_rgb(mix(r,rest.r()),mix(g,rest.g()),mix(b,rest.b()))
}
pub(crate) fn field_background(ui:&Ui,rect:Rect,role:Role,opaque:bool){
    let fill=palette(role).fill.map(|t|if opaque{composite_opaque(t,theme::INPUT)}else{t});
    paint_parts(ui.painter(),rect,field_radius(role),fill,None);
}
pub(crate) fn field_outline(ui:&Ui,rect:Rect,role:Role){
    paint_outline(ui.painter(),rect,field_radius(role),role);
}
pub(crate) fn prioritized_stroke(rest:Stroke,states:States)->Stroke{
    states.resolve().and_then(|role|palette(role).stroke).unwrap_or(rest)
}
pub(crate) fn card_stripe_rect(rect:Rect)->Rect{
    // Inset from the structural frame and its rounded corners.
    let inset=theme::SOFT_DIVIDER_WIDTH;
    let ends=f32::from(theme::WORKFLOW_CARD_RADIUS);
    Rect::from_min_max(
        egui::pos2(rect.left()+inset,rect.top()+ends),
        egui::pos2(rect.left()+inset+theme::WORKFLOW_SELECTED_STRIPE,rect.bottom()-ends)
    )
}
pub(crate) fn calendar_widget(ui:&mut Ui,selected:bool,today:bool){
    let widgets=&mut ui.visuals_mut().widgets;
    let chosen_stroke=Stroke::new(theme::SELECTION_STROKE_WIDTH,theme::SELECTION);
    let today_stroke=Stroke::new(theme::FRAME_STROKE_WIDTH,theme::BORDER);
    for (visual,hovered,focused) in [
        (&mut widgets.inactive,false,false),
        (&mut widgets.hovered,true,false),
        (&mut widgets.active,false,true),
        (&mut widgets.open,false,true),
    ]{
        let fill=if selected{theme::WORKFLOW_SELECTED_BG}else{visual.weak_bg_fill};
        let stroke=if selected{chosen_stroke}
            else if today{today_stroke}else{visual.bg_stroke};
        let (fill,stroke)=if selected{(fill,stroke)}
            else{action_surface(fill,stroke,false,hovered,focused)};
        visual.bg_fill=fill;
        visual.weak_bg_fill=fill;
        visual.bg_stroke=stroke;
        visual.expansion=0.0;
    }
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
/// Common highlight geometry preserves each approved surface hierarchy.
/// A surface uses its own corners, never a second independent outline radius.
pub(crate) const VALUE_PADDING:f32=theme::SPACE_4;
pub(crate) const CONTROL_RADIUS:u8=theme::CONTROL_RADIUS;
pub(crate) const NAVIGATION_RADIUS:u8=5;
pub(crate) const COMPACT_RADIUS:u8=4;
pub(crate) const SOURCE_RADIUS:f32=theme::SOURCE_BLOCK_RADIUS;
pub(crate) fn value_rect(ink:Rect,slot:Rect)->Rect{
    ink.expand(VALUE_PADDING).intersect(slot)
}
pub(crate) fn source_block_rect(text_left:f32,text_right:f32,row_top:f32,row_height:f32)->Option<Rect>{
    if !text_left.is_finite() || !text_right.is_finite()
        || !row_top.is_finite() || !row_height.is_finite()
        || text_right<=text_left || row_height<=2.0*theme::SOURCE_BLOCK_ROW_INSET {
        return None;
    }
    let horizontal=theme::SOURCE_BLOCK_HORIZONTAL_PADDING+theme::SOURCE_RECOGNIZED_BORDER_WIDTH;
    Some(Rect::from_min_max(
        egui::pos2(text_left-horizontal,row_top+theme::SOURCE_BLOCK_ROW_INSET),
        egui::pos2(text_right+horizontal,row_top+row_height-theme::SOURCE_BLOCK_ROW_INSET)
    ))
}
/// Resolve a single fill/stroke pair. Focus and hover replace the resting
/// outline; they are never painted over it. Text is rendered afterwards.
pub(crate) fn action_surface(rest:Color32,rest_stroke:Stroke,light:bool,hovered:bool,focused:bool)->(Color32,Stroke){
    let role=if light{Role::LightHover}else{Role::Hover};
    let p=palette(role);
    let fill=if hovered{
        if light{p.fill.unwrap_or(rest)}else{p.fill.map(|t|composite_opaque(t,rest)).unwrap_or(rest)}
    }else{rest};
    let stroke=if hovered||focused{p.stroke.unwrap_or(rest_stroke)}else{rest_stroke};
    (fill,stroke)
}
/// The persistent state is selected before egui chooses its native frame.
pub(crate) struct ActionState{light:bool,persistent:Option<Role>}
impl From<bool> for ActionState{
    fn from(light:bool)->Self{Self{light,persistent:None}}
}
impl From<(bool,Option<Role>)> for ActionState{
    fn from((light,persistent):(bool,Option<Role>))->Self{Self{light,persistent}}
}
pub(crate) fn persistent_surface(rest:Color32,stroke:Stroke,role:Option<Role>)->(Color32,Stroke){
    if let Some(role)=role{
        let p=palette(role);
        (p.fill.map(|t|composite_opaque(t,rest)).unwrap_or(rest),p.stroke.unwrap_or(stroke))
    }else{(rest,stroke)}
}
pub(crate) fn freight_role(required:bool,review:bool)->Option<Role>{
    States{required,review,..Default::default()}.resolve().map(|role|
        if role==Role::RequiredReview{Role::ReviewTint}else{role})
}
/// The native Button owns its single frame; no custom highlight overlay.
pub(crate) fn action_widget(ui:&mut Ui,rest:Color32,stroke:Stroke,state:impl Into<ActionState>){
    let ActionState{light,persistent}=state.into();
    let widgets=&mut ui.visuals_mut().widgets;
    for (visual,hovered,focused) in [
        (&mut widgets.inactive,false,false),
        (&mut widgets.hovered,true,false),
        (&mut widgets.active,false,true),
        (&mut widgets.open,false,true),
    ]{
        let (fill,border)=if persistent.is_some(){persistent_surface(rest,stroke,persistent)}
            else{action_surface(rest,stroke,light,hovered,focused)};
        visual.bg_fill=fill;
        visual.weak_bg_fill=fill;
        visual.bg_stroke=border;
        visual.expansion=0.0;
    }
}
pub(crate) fn fixed_action_surface(ui:&Ui,response:&Response,radius:impl Into<CornerRadius>,rest:Color32,stroke:Stroke,light:bool,interactive:bool){
    let (fill,border)=action_surface(rest,stroke,light,interactive&&response.hovered(),interactive&&response.has_focus());
    paint_parts(ui.painter(),response.rect,radius,Some(fill),Some(border));
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
    fn action_surface_replaces_instead_of_stacking_borders(){
        let rest=theme::BRIGHT;
        let border=Stroke::new(1.0,theme::DIVIDER);
        assert_eq!(action_surface(rest,border,true,false,false),(rest,border));
        assert_eq!(action_surface(rest,border,true,true,false),(theme::BUTTON_HOVER_LIGHT_BG,palette(Role::LightHover).stroke.unwrap()));
        assert_eq!(action_surface(rest,border,true,false,true),(rest,palette(Role::LightHover).stroke.unwrap()));
        assert_eq!(value_rect(Rect::from_min_max(egui::pos2(5.0,5.0),egui::pos2(10.0,10.0)),Rect::from_min_max(egui::pos2(3.0,3.0),egui::pos2(12.0,12.0))),Rect::from_min_max(egui::pos2(3.0,3.0),egui::pos2(12.0,12.0)));
    }
    #[test]
    fn priority_preserves_required_and_persistent_states(){
        let all=States{required:true,selected:true,linked:true,review:true,focused:true,hovered:true};
        assert_eq!(all.resolve(),Some(Role::RequiredInput));
        assert_eq!(States{required:false,..all}.resolve(),Some(Role::Selected));
        assert_eq!(States{required:false,selected:false,..all}.resolve(),Some(Role::LinkedPreview));
        assert_eq!(States{required:false,selected:false,linked:false,..all}.resolve(),Some(Role::RequiredReview));
        assert_eq!(States{focused:true,hovered:true,..States::default()}.resolve(),Some(Role::Focus));
        assert_eq!(States::default().resolve(),None);
        assert_eq!(prioritized_stroke(Stroke::NONE,all).color,theme::ERROR_BORDER);
        let fields=FieldHighlights::new([true,true,false],[true,true,false],[false,true,false]);
        assert_eq!(fields.role(0,true),Some(Role::RequiredInput));
        assert_eq!(fields.role(1,true),None);
        assert!(fields.editor_owns(1));
        assert_eq!(fields.role(2,true),Some(Role::RequiredReview));
        assert_eq!(freight_role(true,true),Some(Role::RequiredInput));
        assert_eq!(freight_role(false,true),Some(Role::ReviewTint));
        assert_eq!(persistent_surface(theme::INPUT,Stroke::NONE,freight_role(true,true)).1.color,theme::ERROR_BORDER);
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
