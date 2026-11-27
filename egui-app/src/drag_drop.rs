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
pub(crate) fn paint_target(ui:&Ui,rect:Rect,hovered:bool){
    let rect=rect.intersect(ui.clip_rect());
    if !rect.is_positive(){return;}
    let role=if hovered{crate::highlight::Role::DropHovered}else{crate::highlight::Role::DropTarget};
    crate::highlight::paint(ui,rect,crate::highlight::Geometry::Control,role);
}
pub(crate) fn start_native<T:Any+Send+Sync>(ui:&Ui,response:&Response,enabled:bool,payload:impl FnOnce()->T){
    if enabled&&ui.is_enabled()&&response.enabled()&&response.drag_started(){
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
    let count_galley=typography::client_table_metadata_galley(ui,count,theme::SECONDARY);
    let count_w=count_galley.size().x;
    let max_width=bounds.width().min(320.0);
    let name_width=max_width-pad*3.0-count_w;
    if name_width<=0.0{return;}
    let visible=typography::fit_category_path(ui,name,name_width,typography::CategoryPathFont::Body);
    let name_galley=typography::management_control_text_galley(ui,&visible,theme::TEXT);
    let size=egui::vec2((name_galley.size().x+count_w+pad*3.0).min(max_width),
        name_galley.size().y.max(count_galley.size().y)+pad*2.0);
    if size.y>bounds.height(){return;}
    let rect=pointer_label_rect(bounds,pointer,size);
    let painter=painter.with_shared_clip(bounds);
    painter.rect(rect,theme::CONTROL_RADIUS,theme::RAISED,
        egui::Stroke::new(1.0,theme::BORDER),egui::StrokeKind::Inside);
    let name_y=rect.center().y-name_galley.size().y/2.0;
    let count_y=rect.center().y-count_galley.size().y/2.0;
    let count_x=rect.right()-pad-count_w;
    let name_clip=Rect::from_min_max(rect.min+egui::vec2(pad,0.0),
        egui::pos2((count_x-pad).max(rect.left()+pad),rect.bottom()));
    painter.with_shared_clip(name_clip).galley(egui::pos2(rect.left()+pad,name_y),name_galley,theme::TEXT);
    painter.galley(egui::pos2(count_x,count_y),count_galley,theme::SECONDARY);
    cursor(ctx,true);
}

#[cfg(test)]
mod tests{
    use super::*;
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
