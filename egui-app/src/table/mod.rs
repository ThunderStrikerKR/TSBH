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

fn finish(ui:&Ui,header:Rect,body:Rect){
    let bounds=outline_bounds(header,body);
    if bounds.width()>0.0 && bounds.height()>0.0{
        ui.painter().rect_stroke(bounds,0.0,
            Stroke::new(theme::FRAME_STROKE_WIDTH,theme::BORDER),
            egui::StrokeKind::Inside);
    }
}

/// Inline tables share the parent's scroll; stop at the current row cursor,
/// not the parent's minimum/maximum rectangle or the following freight form.
fn finish_inline(ui:&Ui,header:Rect){
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


/// The existing header/viewport policies are mutually exclusive, not separate
/// table designs. Only this entry point owns perimeter finalization.
pub(crate) enum Header<'a>{
    Sticky(f32),
    List(f32),
    WithoutToolbar(f32),
    AvailableWidth(f32),
    Register{primary:&'a str,action:&'a str,action_width:f32},
}

pub(crate) fn show<R>(
    ui:&mut Ui,header:Header<'_>,
    draw:impl FnOnce(&mut Ui,crate::components::ManagementTableHeader)->(Rect,R),
)->R{
    use crate::components as c;
    let layout=match header{
        Header::Sticky(width)=>c::management_sticky_table_header(ui,width),
        Header::List(width)=>c::management_list_table_header(ui,width),
        Header::WithoutToolbar(width)=>c::management_sticky_table_header_without_toolbar(ui,width),
        Header::AvailableWidth(width)=>{
            let viewport=ui.available_width().max(1.0);
            c::allocate_management_table_header(ui,viewport,width)
        },
        Header::Register{primary,action,action_width}=>
            c::management_master_register_header(ui,primary,action,action_width),
    };
    // Draw directly on the existing UI; do not allocate a scope or alter
    // persistent widget IDs, the scroll salt, or keyboard focus.
    let (body,result)=draw(ui,layout);
    finish(ui,layout.header_rect,body);
    result
}

pub(crate) enum InlineHeader{
    Reception{editable:bool},
    Shipment,
}
pub(crate) fn inline<R>(
    ui:&mut Ui,header:InlineHeader,draw:impl FnOnce(&mut Ui)->R
)->R{
    use crate::components as c;
    let rect=match header{
        InlineHeader::Reception{editable:true}=>c::reception_editable_items_header(ui),
        InlineHeader::Reception{editable:false}=>c::reception_items_header(ui),
        InlineHeader::Shipment=>c::workflow_shipment_header(ui),
    };
    let result=draw(ui);
    finish_inline(ui,rect);
    result
}

/// Body policies preserve rail behavior and existing scroll IDs.
#[derive(Clone,Copy)]
pub(crate) struct Viewport {
    pub width:f32,
    pub height:f32,
    pub rail:Option<Rect>,
}
fn viewport_scroll(ui:&Ui,viewport:Viewport,id:impl std::hash::Hash)->egui::ScrollArea{
    let mut scroll=crate::components::management_fixed_table_scroll_area(ui,viewport.width);
    if let Some(rail)=viewport.rail{scroll=scroll.scroll_bar_rect(rail);}
    scroll.id_salt(id).max_height(viewport.height).auto_shrink([false,false])
}
pub(crate) fn fixed_rows<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,
    row_height:f32,count:usize,draw:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    let scroll=viewport_scroll(ui,viewport,id);
    crate::components::virtual_fixed_rows(scroll,ui,viewport.width,row_height,count,draw)
}
pub(crate) fn variable_rows<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,rows_id:impl std::hash::Hash,
    offsets:&[f32],draw:impl FnOnce(&mut Ui,egui::Pos2,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    debug_assert!(!offsets.is_empty());
    let count=offsets.len().saturating_sub(1);
    viewport_scroll(ui,viewport,id).show_viewport(ui,|body,visible|{
        management_table_row_body(body,viewport.width);
        body.set_min_height(*offsets.last().unwrap_or(&0.0));
        let first=offsets.partition_point(|value|*value<=visible.top())
            .saturating_sub(1).min(count);
        let last=offsets.partition_point(|value|*value<=visible.bottom())
            .saturating_add(1).min(count);
        let origin=body.max_rect().min;
        let visible_rect=Rect::from_min_size(
            egui::pos2(origin.x,origin.y+offsets[first]),
            Vec2::new(viewport.width,(offsets[last]-offsets[first]).max(0.0)));
        let clip=body.clip_rect();
        body.scope_builder(egui::UiBuilder::new().id_salt(rows_id).max_rect(visible_rect),|visible|{
            visible.spacing_mut().item_spacing.y=0.0;
            visible.set_clip_rect(clip);
            visible.skip_ahead_auto_ids(first);
            draw(visible,origin,first..last)
        }).inner
    })
}
pub(crate) fn register_rows<R>(
    ui:&mut Ui,height:f32,width:f32,id:impl std::hash::Hash,
    draw:impl FnOnce(&mut Ui)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    crate::components::management_register_list_scroll_area(ui,height).id_salt(id)
        .show(ui,|body|{body.set_min_width(width);draw(body)})
}
pub(crate) fn fixed_row_rect(origin:egui::Pos2,index:usize,first:usize,width:f32,height:f32)->Rect{
    Rect::from_min_size(egui::pos2(origin.x,origin.y+(index-first) as f32*height),
        Vec2::new(width,height))
}
pub(crate) fn row_scope<R>(
    ui:&mut Ui,id:impl std::hash::Hash,rect:Rect,draw:impl FnOnce(&mut Ui)->R,
)->egui::InnerResponse<R>{
    ui.scope_builder(egui::UiBuilder::new().id_salt(id).max_rect(rect)
        .layout(egui::Layout::top_down(egui::Align::Min)),draw)
}
#[derive(Clone,Copy)]
pub(crate) enum RowTone{Alternating,SelectedIf(bool)}
pub(crate) fn fixed_row<const N:usize>(
    ui:&mut Ui,id:impl std::hash::Hash,rect:Rect,index:usize,
    widths:&[f32;N],gap:f32,tone:RowTone,draw:impl FnOnce(&mut Ui,[Rect;N]),
){
    row_scope(ui,id,rect,|row|{
        row.allocate_exact_size(rect.size(),egui::Sense::hover());
        if matches!(tone,RowTone::SelectedIf(true)){
            row.painter().rect_filled(rect,0.0,theme::WORKFLOW_SELECTED_BG);
            row.painter().rect_filled(Rect::from_min_size(rect.min,
                Vec2::new(theme::WORKFLOW_SELECTED_STRIPE,rect.height())),0.0,theme::SELECTION);
        }else{paint_management_table_row_background(row.painter(),rect,index);}
        let cells=management_table_cells(rect.min,widths,rect.height(),gap);
        draw(row,cells);
        management_table_column_rules(row,rect,&cells,gap);
        paint_management_table_row_rule(row.painter(),rect);
    });
}
/// Only column content and business sort state are supplied by the caller.
pub(crate) fn header_columns<R>(
    ui:&mut Ui,header:Rect,content:Rect,cells:&[Rect],gap:f32,
    draw:impl FnOnce(&mut Ui,Rect)->R,
)->(Rect,R){
    let text=header_begin(ui,header,content);
    let result=draw(ui,text);
    management_table_column_rules(ui,text,cells,gap);
    header_finish(ui,header);
    (text,result)
}

/// Alignment policies retain each approved column's inset and narrow-width clamp.
#[derive(Clone,Copy)]
pub(crate) enum HeadingAlign{
    Left(f32),
    Right{inset:f32,min_left:Option<f32>},
    Center{min_left:Option<f32>},
}
pub(crate) fn heading_alignment(align:egui::Align2,left:f32,right:f32)->HeadingAlign{
    if align==egui::Align2::RIGHT_CENTER{HeadingAlign::Right{inset:right,min_left:None}}
    else if align==egui::Align2::CENTER_CENTER{HeadingAlign::Center{min_left:None}}
    else{HeadingAlign::Left(left)}
}
pub(crate) fn heading(
    ui:&Ui,cell:Rect,area:Rect,clip:Rect,align:HeadingAlign,
    label:std::sync::Arc<egui::Galley>,arrow:Option<(std::sync::Arc<egui::Galley>,Color32)>,
){
    let clip=clip.intersect(area).intersect(ui.clip_rect());
    if clip.width()<=0.0||clip.height()<=0.0{return;}
    let gap=if arrow.is_some(){theme::SPACE_4}else{0.0};
    let total=label.size().x+gap+arrow.as_ref().map_or(0.0,|(g,_)|g.size().x);
    let x=match align{
        HeadingAlign::Left(inset)=>cell.left()+inset,
        HeadingAlign::Right{inset,min_left}=>{
            let x=cell.right()-inset-total;
            min_left.map_or(x,|left|x.max(cell.left()+left))
        },
        HeadingAlign::Center{min_left}=>{
            let x=cell.center().x-total/2.0;
            min_left.map_or(x,|left|x.max(cell.left()+left))
        },
    };
    let label_width=label.size().x;
    let painter=ui.painter().with_clip_rect(clip);
    painter.galley(egui::pos2(x,area.center().y-label.size().y/2.0),label,theme::SECONDARY);
    if let Some((glyph,color))=arrow{
        painter.galley(egui::pos2(x+label_width+gap,area.center().y-glyph.size().y/2.0),glyph,color);
    }
}
pub(crate) fn sort_clicked(ui:&mut Ui,hit:Rect,id:impl std::hash::Hash)->bool{
    if hit.width()<=0.0||hit.height()<=0.0{return false;}
    let response=ui.interact(hit,ui.make_persistent_id(id),egui::Sense::click());
    if response.hovered(){crate::components::management_master_header_sort_hover(ui,hit);}
    response.clicked()
}
