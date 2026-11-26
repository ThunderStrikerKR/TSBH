//! Shared table surface. Owns chrome only; no widget IDs, focus, layout
//! allocation, pointer capture or business state. All profiles keep their
//! existing row geometry and scroll ownership.
use eframe::egui::{self,Color32,Rect,Stroke,Ui,Vec2};
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

/// Shared management table container policy. The caller owns its layout
/// scope; return the preceding vertical gap for existing explicit restores.
/// Column widths, row heights, table IDs and header positions stay unchanged.
pub fn management_table_container_style(ui:&mut Ui)->f32{
    let previous_gap=ui.spacing().item_spacing.y;
    ui.spacing_mut().scroll=theme::management_table_scroll_style();
    ui.spacing_mut().item_spacing.y=0.0;
    previous_gap
}

/// Shared fixed-column geometry for headers and data rows. The existing
/// table policy supplies widths, height and gap.
pub fn management_table_cells<const N:usize>(
    origin:egui::Pos2,widths:&[f32;N],height:f32,gap:f32
)->[Rect;N]{
    let mut x=origin.x;
    std::array::from_fn(|index|{
        let rect=Rect::from_min_size(egui::pos2(x,origin.y),Vec2::new(widths[index],height));
        x+=widths[index]+gap;
        rect
    })
}

pub fn management_table_row_fill(index:usize)->Color32{
    if index%2==0{theme::PANEL}else{theme::TABLE_ROW_ALT_BG}
}

pub fn paint_management_table_row_background(painter:&egui::Painter,rect:Rect,index:usize){
    painter.rect_filled(rect,0.0,management_table_row_fill(index));
}

/// All table rows use the same filled 1pt rule inside the bottom edge.
/// Do not center a stroke on the boundary: clipping would halve its width.
pub fn paint_management_table_row_rule(painter:&egui::Painter,rect:Rect){
    let top=(rect.bottom()-theme::SOFT_DIVIDER_WIDTH).max(rect.top());
    theme::paint_soft_rule(painter,Rect::from_min_max(
        egui::pos2(rect.left(),top),rect.right_bottom()
    ));
}

/// Fixed and variable row viewports share the minimum-width and zero-gap
/// rules. Retain the caller's existing width and height calculations.
pub fn management_table_row_body(ui:&mut Ui,width:f32){
    ui.set_min_width(width);
    ui.spacing_mut().item_spacing.y=0.0;
}


/// One minimum-width calculation for both sticky master tables and
/// other fixed-header management lists. The gutter supplied here is the
/// egui scrollbar allocation only (CONFLICT-003=A), without any spacer.
pub(crate) fn management_table_data_width(
    viewport_width:f32,scrollbar_gutter:f32,min_table_width:f32
)->f32{
    debug_assert!(scrollbar_gutter>=0.0 && scrollbar_gutter.is_finite());
    debug_assert!(viewport_width.is_finite() && min_table_width.is_finite());
    (viewport_width-scrollbar_gutter).max(min_table_width)
}


/// Shared management-table column separators. Freight Assignment and
/// Client Management both call this painter for header and body instead
/// of leaving vertical rules to independently implemented screen widgets.
///
/// The caller supplies DATA coordinates, not window coordinates. `region`
/// clips the header before its scrollbar gutter and keeps body rules inside
/// each virtualized row. Header bottom 2pt outline remains owned by the
/// existing `management_master_header_finish` function.
pub(crate) fn management_master_column_rules(
    ui:&Ui,region:Rect,boundaries:impl IntoIterator<Item=f32>
){
    let clip=region.intersect(ui.clip_rect());
    if clip.width()<=0.0 || clip.height()<=0.0{return;}
    let painter=ui.painter().with_clip_rect(clip);
    for x in boundaries{
        if x>clip.left() && x<clip.right() {
            let half=theme::SOFT_DIVIDER_WIDTH/2.0;
            theme::paint_soft_rule(&painter,Rect::from_min_max(
                egui::pos2(x-half,region.top()),egui::pos2(x+half,region.bottom())
            ));
        }
    }
}

/// Header and body use the same cell edges and inter-column midpoint.
pub fn management_table_column_rules(ui:&Ui,region:Rect,cells:&[Rect],gap:f32){
    management_master_column_rules(ui,region,
        cells.iter().take(cells.len().saturating_sub(1)).map(|cell|cell.right()+gap/2.0));
}

