//! Shared table surface. Owns chrome only; no widget IDs, focus, layout
//! allocation, pointer capture or business state. All profiles keep their
//! existing row geometry and scroll ownership.
use eframe::egui::{self,Rect,Stroke,Ui};
use crate::theme;

pub(crate) fn header_begin(ui:&Ui,header:Rect,content:Rect)->Rect{
    let text=Rect::from_min_max(content.min,egui::pos2(
        content.right(),content.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
    ));
    ui.painter().with_clip_rect(header).rect_filled(header,0.0,theme::RAISED);
    text
}

pub(crate) fn header_finish(ui:&Ui,header:Rect){
    ui.painter().with_clip_rect(header).rect_filled(
        Rect::from_min_max(egui::pos2(header.left(),
            header.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT),
            header.right_bottom()),0.0,theme::TABLE_HEADER_OUTLINE);
}

pub(crate) fn outline_bounds(header:Rect,body:Rect)->Rect{
    Rect::from_min_max(header.min,
        egui::pos2(header.right(),body.bottom().max(header.bottom())))
}

pub(crate) fn finish(ui:&Ui,header:Rect,body:Rect){
    let bounds=outline_bounds(header,body);
    if bounds.width()>0.0 && bounds.height()>0.0{
        ui.painter().rect_stroke(bounds,0.0,
            Stroke::new(theme::FRAME_STROKE_WIDTH,theme::BORDER),
            egui::StrokeKind::Inside);
    }
}

/// Inline tables share the parent's scroll; stop at the current row cursor,
/// not the parent's minimum/maximum rectangle or the following freight form.
pub(crate) fn finish_inline(ui:&Ui,header:Rect){
    let bottom=ui.cursor().top().max(header.bottom());
    finish(ui,header,Rect::from_min_max(header.min,
        egui::pos2(header.right(),bottom)));
}
