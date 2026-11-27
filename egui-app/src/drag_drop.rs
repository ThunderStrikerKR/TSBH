//! Shared business drag-and-drop interaction and presentation.
//! Domain payloads, mutation authorization and persistence remain with the
//! owning screen. Window movement and split resizing are not business drops.
use eframe::egui::{self,Context,Id,LayerId,Pos2,Rect,Response,Sense,Ui,Vec2};
use std::{any::Any,sync::Arc};
use crate::{theme,typography};
use crate::ui_layout::geometry::SharedPainterClipExt as _;

pub(crate) fn sense(enabled:bool,draggable:bool)->Sense{
    if !enabled{Sense::hover()}else if draggable{Sense::click_and_drag()}else{Sense::click()}
}
pub(crate) fn cursor(ctx:&Context,allowed:bool){
    ctx.set_cursor_icon(if allowed{egui::CursorIcon::Grabbing}else{egui::CursorIcon::NotAllowed});
}
pub(crate) fn pointer_hits(ui:&Ui,rect:Rect,pointer:Option<Pos2>)->bool{
    ui.is_enabled()&&pointer.is_some_and(|pos|rect.intersect(ui.clip_rect()).contains(pos))
}
/// Per-frame collector for custom drag targets. Repeated observations of
/// the same logical target are harmless; different compatible targets at
/// one pointer position are ambiguous and must not choose by draw order.
pub(crate) struct UniqueDropTarget<T>{
    target:Option<T>,
    ambiguous:bool,
}
impl<T:Eq> UniqueDropTarget<T>{
    pub(crate) fn new()->Self{Self{target:None,ambiguous:false}}
    pub(crate) fn observe(&mut self,target:T){
        if self.ambiguous{return;}
        match self.target.as_ref(){
            None=>self.target=Some(target),
            Some(old) if old==&target=>{},
            Some(_)=>{self.target=None;self.ambiguous=true;}
        }
    }
    pub(crate) fn is_unique(&self)->bool{!self.ambiguous&&self.target.is_some()}
    pub(crate) fn finish(self)->Option<T>{if self.ambiguous{None}else{self.target}}
}
pub(crate) fn paint_target(ui:&Ui,rect:Rect,hovered:bool){
    let rect=rect.intersect(ui.clip_rect());
    if !rect.is_positive(){return;}
    let role=if hovered{crate::highlight::Role::DropHovered}else{crate::highlight::Role::DropTarget};
    crate::highlight::paint(ui,rect,crate::highlight::Geometry::Control,role);
}
pub(crate) fn start_native<T:Any+Send+Sync>(ui:&Ui,response:&Response,enabled:bool,payload:impl FnOnce()->T){
    if enabled&&ui.is_enabled()&&response.enabled()&&response.drag_started()
        &&ui.input(|input|input.pointer.primary_down()){
        response.dnd_set_drag_payload(payload());
    }
    if enabled&&response.dragged(){cursor(ui.ctx(),true);}
}
pub(crate) fn primary_dragged(response:&Response)->bool{
    response.dragged_by(egui::PointerButton::Primary)
}
pub(crate) fn native_payload<T:Any+Send+Sync>(ctx:&Context)->Option<Arc<T>>{
    egui::DragAndDrop::payload::<T>(ctx)
}
pub(crate) fn hover_payload<T:Any+Send+Sync>(response:&Response)->Option<Arc<T>>{
    response.dnd_hover_payload::<T>()
}
pub(crate) fn drop_native<T:Any+Send+Sync>(ui:&Ui,response:&Response,enabled:bool,
    valid:impl Fn(&T)->bool)->Option<Arc<T>>{
    if !enabled||!ui.is_enabled()||!response.enabled(){return None;}
    let drag=hover_payload::<T>(response)?;
    if !valid(&drag){return None;}
    paint_target(ui,response.rect,true);
    // Only the initiating primary-button gesture may commit. The native
    // payload is consumed on release; subsequent targets cannot reuse it.
    if !ui.input(|input|input.pointer.button_released(egui::PointerButton::Primary)){return None;}
    response.dnd_release_payload::<T>().filter(|drag|valid(drag))
}
pub(crate) fn preview_painter(ctx:&Context,owner:LayerId,modal:bool,id:Id)->Option<egui::Painter>{
    if modal{crate::overlay::drag_painter(ctx,owner,id)}
    else{crate::overlay::page_drag_painter(ctx,owner,id)}
}

fn pointer_label_rect(bounds:Rect,pointer:Pos2,size:Vec2)->Rect{
    let offset=theme::SPACE_12;
    let x=if pointer.x+offset+size.x<=bounds.right(){pointer.x+offset}
        else{pointer.x-offset-size.x};
    let y=if pointer.y+offset+size.y<=bounds.bottom(){pointer.y+offset}
        else{pointer.y-offset-size.y};
    Rect::from_min_size(egui::pos2(
        x.clamp(bounds.left(),(bounds.right()-size.x).max(bounds.left())),
        y.clamp(bounds.top(),(bounds.bottom()-size.y).max(bounds.top()))
    ),size)
}

/// Non-interactive cursor-following preview. Reserve the count before fitting
/// the title, so even a long Korean product name cannot hide the quantity.
pub(crate) fn paint_pointer_label(ui:&Ui,name:&str,count:&str){
    let ctx=ui.ctx();
    if !ctx.input(|i|i.pointer.primary_down()){return;}
    let Some(pointer)=ctx.input(|i|i.pointer.hover_pos())else{return;};
    let bounds=ui.clip_rect().intersect(ctx.content_rect());
    if !bounds.is_positive()||!bounds.contains(pointer){return;}
    let Some(painter)=preview_painter(ctx,ui.layer_id(),false,Id::new("business-drag-pointer-label"))else{return;};
    let pad=theme::SPACE_8;
    let count_galley=typography::client_table_metadata_galley(ui,count,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
    let count_w=count_galley.size().x;
    let max_width=bounds.width().min(320.0);
    let name_width=max_width-pad*3.0-count_w;
    if name_width<=0.0{return;}
    let visible=typography::fit_category_path(ui,name,name_width,typography::CategoryPathFont::Body);
    let name_galley=typography::management_control_text_galley(ui,&visible,theme::text_ink(theme::TextRole::Value,theme::TextState::Normal));
    let size=egui::vec2((name_galley.size().x+count_w+pad*3.0).min(max_width),
        name_galley.size().y.max(count_galley.size().y)+pad*2.0);
    if size.y>bounds.height(){return;}
    let rect=pointer_label_rect(bounds,pointer,size);
    let painter=painter.with_shared_clip(bounds);
    painter.rect(rect,theme::CONTROL_RADIUS,theme::surface_palette(theme::SurfaceRole::Button,theme::VisualState::Normal).fill,
        egui::Stroke::new(1.0,theme::border_ink(theme::BorderRole::Standard)),egui::StrokeKind::Inside);
    let name_y=rect.center().y-name_galley.size().y/2.0;
    let count_y=rect.center().y-count_galley.size().y/2.0;
    let count_x=rect.right()-pad-count_w;
    let name_clip=Rect::from_min_max(rect.min+egui::vec2(pad,0.0),
        egui::pos2((count_x-pad).max(rect.left()+pad),rect.bottom()));
    painter.with_shared_clip(name_clip).galley(egui::pos2(rect.left()+pad,name_y),name_galley,theme::text_ink(theme::TextRole::Value,theme::TextState::Normal));
    painter.galley(egui::pos2(count_x,count_y),count_galley,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
    cursor(ctx,true);
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn unique_drop_target_rejects_ambiguous_hits_independently_of_draw_order(){
        for candidates in [[1,2,1],[2,1,2]]{
            let mut targets=UniqueDropTarget::new();
            for candidate in candidates{targets.observe(candidate);}
            assert!(!targets.is_unique());
            assert_eq!(targets.finish(),None);
        }
        let mut targets=UniqueDropTarget::new();
        targets.observe(1);targets.observe(1);
        assert!(targets.is_unique());
        assert_eq!(targets.finish(),Some(1));
        assert_eq!(UniqueDropTarget::<u8>::new().finish(),None);
    }
    #[test]
    fn native_drag_commits_only_once_to_enabled_valid_primary_target(){
        use egui::{Event,Modifiers,PointerButton};
        for (enabled,valid,button,expected) in [
            (true,true,PointerButton::Primary,1),
            (false,true,PointerButton::Primary,0),
            (true,false,PointerButton::Primary,0),
            (true,true,PointerButton::Secondary,0),
        ]{
            let ctx=crate::ui_test_support::test_context();
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(600.0,400.0));
            let mut starts=0;
            let mut drops=0;
            let mut draw=|events:Vec<Event>|{
                let mut rects=None;
                let time=ctx.input(|input|input.time)+0.1;
                let _=ctx.run(egui::RawInput{screen_rect:Some(screen),time:Some(time),events,..Default::default()},|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let (_,source)=ui.allocate_exact_size(Vec2::new(150.0,40.0),sense(true,true));
                        start_native(ui,&source,true,||{starts+=1;17_u32});
                        ui.add_space(60.0);
                        let (_,target)=ui.allocate_exact_size(Vec2::new(150.0,40.0),Sense::click());
                        for _ in 0..2{
                            if let Some(value)=drop_native::<u32>(ui,&target,enabled,|v|valid&&*v==17){
                                assert_eq!(*value,17);
                                drops+=1;
                            }
                        }
                        rects=Some((source.rect,target.rect));
                    });
                });
                rects.unwrap()
            };
            let (source,target)=draw(vec![]);
            draw(vec![Event::PointerMoved(source.center())]);
            draw(vec![Event::PointerButton{pos:source.center(),button,pressed:true,modifiers:Modifiers::NONE}]);
            draw(vec![Event::PointerMoved(source.center()+egui::vec2(12.0,0.0))]);
            draw(vec![Event::PointerMoved(target.center())]);
            draw(vec![Event::PointerMoved(target.center())]);
            draw(vec![Event::PointerButton{pos:target.center(),button,pressed:false,modifiers:Modifiers::NONE}]);
            draw(vec![]);
            draw(vec![]);
            drop(draw);
            assert_eq!(starts,if button==PointerButton::Primary{1}else{0});
            assert_eq!(drops,expected,"enabled={enabled} valid={valid} button={button:?}");
        }
    }
    #[test]
    fn pointer_label_flips_and_stays_inside_work_area(){
        let bounds=Rect::from_min_max(egui::pos2(10.0,20.0),egui::pos2(500.0,300.0));
        let size=egui::vec2(180.0,42.0);
        for pointer in [bounds.min,bounds.max,bounds.right_top(),bounds.left_bottom(),bounds.center()]{
            let rect=pointer_label_rect(bounds,pointer,size);
            assert!(bounds.contains(rect.min)&&bounds.contains(rect.max));
            assert_eq!(rect.size(),size);
        }
    }
}
