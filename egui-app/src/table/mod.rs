//! Shared table renderer: surface, header, row/viewport geometry and sort hit testing.
//! Screens retain column data, stable IDs and business sort state.
use eframe::egui::{self,Color32,Rect,Stroke,Ui,Vec2};
use crate::card::UiWidthPolicyExt;
use crate::{theme,typography};
use eframe::egui::{Sense,Align};
use crate::ui_primitives::{overflow_region_tooltip,text_exceeds_rect,positioned_text_overflow};

/// Shared editable-cell text placement, clipping and overflow measurement.
pub(crate) fn paint_editable_cell_text(
    ui:&Ui,clip:Rect,label:&str,full:&str,font:egui::FontId,ink:Color32,
    right:bool,y_adjust:f32
)->bool{
    let galley=ui.painter().layout_no_wrap(label.to_owned(),font,ink);
    let overflow=label!=full || text_exceeds_rect(galley.size(),clip.intersect(ui.clip_rect()));
    let mut origin=reception_item_value_origin(clip,galley.size(),right);
    origin.y+=y_adjust;
    ui.painter().with_clip_rect(clip).galley(origin,galley,ink);
    overflow
}

/// Shared fixed-cell interaction and semantic cursor.
pub(crate) fn editable_cell_hit(
    ui:&Ui,rect:Rect,id:egui::Id,editable:bool,cursor:egui::CursorIcon,
    overflow:bool,full:&str
)->egui::Response{
    let response=ui.interact(rect,id,if editable{Sense::click()}else{Sense::hover()});
    let response=crate::ui_primitives::overflow_tooltip(response,overflow,full);
    if editable{response.on_hover_cursor(cursor)}else{response}
}

fn header_begin(ui:&Ui,header:Rect,content:Rect)->Rect{
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

fn outline_bounds(header:Rect,body:Rect)->Rect{
    Rect::from_min_max(header.min,
        egui::pos2(header.right(),body.bottom().max(header.bottom())))
}

fn finish(ui:&Ui,header:Rect,body:Rect){
    paint_frame(ui,outline_bounds(header,body));
}

fn paint_frame(ui:&Ui,bounds:Rect){
    if bounds.width()>0.0 && bounds.height()>0.0{
        ui.painter().with_clip_rect(bounds.intersect(ui.clip_rect())).rect_stroke(
            bounds,0.0,Stroke::new(theme::FRAME_STROKE_WIDTH,theme::BORDER),
            egui::StrokeKind::Inside);
    }
}

/// The measured native viewport is the single source of truth for the
/// frame and the fixed separator. The right gutter is never a data column.
#[derive(Clone,Copy,Debug)]
struct TableGeometry{
    frame:Rect,
    separator:Option<Rect>,
}

impl TableGeometry{
    fn new(layout:ManagementTableHeader,body:Rect,clip:Rect)->Self{
        let header=layout.header_rect;
        let mut frame=outline_bounds(header,body);
        // A horizontal native rail lives below inner_rect, inside the frame.
        // Both rails are reserved by management_fixed_table_scroll_area.
        if layout.table_width>layout.header_content_rect.width()+0.01{
            frame.max.y=(frame.bottom()+layout.scrollbar_gutter).min(clip.bottom())
                .max(header.bottom());
        }
        let x=layout.header_content_rect.right();
        let half=theme::FRAME_STROKE_WIDTH*0.5;
        let separator=(layout.scrollbar_gutter>0.0&&x>frame.left()+half&&x<frame.right()-half)
            .then(||Rect::from_min_max(
                egui::pos2(x-half,header.top()+theme::FRAME_STROKE_WIDTH),
                egui::pos2(x+half,body.bottom().max(header.bottom()))
            ));
        Self{frame,separator}
    }
    fn paint(self,ui:&Ui){
        if let Some(rect)=self.separator{
            let clip=self.frame.intersect(ui.clip_rect());
            ui.painter().with_clip_rect(clip).rect_filled(rect,0.0,theme::BORDER);
            #[cfg(test)]
            crate::ui_test_support::record("table scrollbar separator",rect);
        }
        paint_frame(ui,self.frame);
        #[cfg(test)]
        crate::ui_test_support::record("table perimeter",self.frame);
        #[cfg(test)]
        crate::ui_test_support::record_component_geometry("table",self.frame,ui.clip_rect(),None);
    }
}

/// ScrollArea supplies viewport coordinates relative to its content origin.
/// Intersect with the actual physical viewport, not egui's expanded clip
/// margin: overscan rows must never paint past the table frame.
fn strict_viewport_clip(body:&Ui,viewport:Rect)->Rect{
    viewport.translate(body.max_rect().min.to_vec2()).intersect(body.clip_rect())
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
pub(crate) fn restore_management_table_spacing(ui:&mut Ui,previous:f32){
    ui.spacing_mut().item_spacing.y=previous;
}

pub(crate) fn management_table_container_style(ui:&mut Ui)->f32{
    let previous_gap=ui.spacing().item_spacing.y;
    ui.spacing_mut().scroll=crate::scrollbar::management_table_scroll_style();
    ui.spacing_mut().item_spacing.y=0.0;
    previous_gap
}

/// Shared fixed-column geometry for headers and data rows. The existing
/// table policy supplies widths, height and gap.
pub(crate) fn management_table_cells<const N:usize>(
    origin:egui::Pos2,widths:&[f32;N],height:f32,gap:f32
)->[Rect;N]{
    let mut x=origin.x;
    std::array::from_fn(|index|{
        let rect=Rect::from_min_size(egui::pos2(x,origin.y),Vec2::new(widths[index],height));
        x+=widths[index]+gap;
        rect
    })
}

pub(crate) fn management_table_row_fill(index:usize)->Color32{
    if index%2==0{theme::PANEL}else{theme::TABLE_ROW_ALT_BG}
}

pub(crate) fn paint_management_table_row_background(painter:&egui::Painter,rect:Rect,index:usize){
    painter.rect_filled(rect,0.0,management_table_row_fill(index));
}

/// Interactive table rows retain the standard alternating background when idle.
/// The hover surface is owned by the table, not by an individual screen.
pub(crate) fn paint_management_interactive_row_background(painter:&egui::Painter,rect:Rect,index:usize,hovered:bool){
    if hovered{painter.rect_filled(rect,0.0,theme::INPUT);}
    else{paint_management_table_row_background(painter,rect,index);}
}

/// All table rows use the same filled 1pt rule inside the bottom edge.
/// Do not center a stroke on the boundary: clipping would halve its width.
pub(crate) fn paint_management_table_row_rule(painter:&egui::Painter,rect:Rect){
    let top=(rect.bottom()-theme::SOFT_DIVIDER_WIDTH).max(rect.top());
    theme::paint_soft_rule(painter,Rect::from_min_max(
        egui::pos2(rect.left(),top),rect.right_bottom()
    ));
}

/// Fixed and variable row viewports share the minimum-width and zero-gap
/// rules. Retain the caller's existing width and height calculations.
pub(crate) fn management_table_row_body(ui:&mut Ui,width:f32){
    ui.apply_shared_width(crate::card::CardWidthPolicy::FillAvailable,width);
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
/// shared `header_finish` function.
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
pub(crate) fn management_table_column_rules(ui:&Ui,region:Rect,cells:&[Rect],gap:f32){
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
    draw:impl FnOnce(&mut Ui,ManagementTableHeader)->(Rect,R),
)->R{
    let layout=match header{
        Header::Sticky(width)=>management_sticky_table_header(ui,width),
        Header::List(width)=>management_list_table_header(ui,width),
        Header::WithoutToolbar(width)=>management_sticky_table_header_without_toolbar(ui,width),
        Header::AvailableWidth(width)=>{
            let viewport=ui.available_width().max(1.0);
            allocate_management_table_header(ui,viewport,width)
        },
        Header::Register{primary,action,action_width}=>
            management_master_register_header(ui,primary,action,action_width),
    };
    // Draw directly on the existing UI; do not allocate a scope or alter
    // persistent widget IDs, the scroll salt, or keyboard focus.
    let (body,result)=draw(ui,layout);
    TableGeometry::new(layout,body,ui.clip_rect()).paint(ui);
    result
}

/// Registry column minima plus the number of leading text columns that
/// share surplus space. Numeric/action columns keep their fixed geometry.
pub(crate) struct RegistryColumns<const N:usize>{
    base:[f32;N],
    stretch:usize,
}
impl<const N:usize> From<[f32;N]> for RegistryColumns<N>{
    fn from(base:[f32;N])->Self{Self{base,stretch:1}}
}
impl<const N:usize> RegistryColumns<N>{
    pub(crate) fn balanced_text(base:[f32;N],stretch:usize)->Self{
        Self{base,stretch:stretch.clamp(1,N)}
    }
    fn widths(&self,available:f32,gap:f32)->[f32;N]{
        let mut widths=self.base;
        let minimum=widths.iter().sum::<f32>()+N.saturating_sub(1) as f32*gap;
        let extra=(available-minimum).max(0.0)/self.stretch as f32;
        for width in widths.iter_mut().take(self.stretch){*width+=extra;}
        widths
    }
}

/// Single-line registry tables share the 42pt compact management row policy.
/// Callers already lay out their toolbar divider or section heading; use a
/// bare header here, never the history-only missing-toolbar reservation.
pub(crate) fn registry_table<const N:usize>(
    ui:&mut Ui,id:impl std::hash::Hash,labels:[&str;N],columns:impl Into<RegistryColumns<N>>,
    height:f32,count:usize,selected:impl Fn(usize)->bool,
    mut draw:impl FnMut(&mut Ui,[Rect;N],usize,Rect),
)->Rect{
    let gap=theme::SPACE_8;
    let columns=columns.into();
    let minimum=columns.base.iter().sum::<f32>()+N.saturating_sub(1) as f32*gap;
    let spacing=crate::table::management_table_container_style(ui);
    let inner=crate::table::show(ui,crate::table::Header::List(minimum),|ui,layout|{
        let widths=columns.widths(layout.table_width,gap);
        let mut row_left=None;
        let viewport_height=height.min(ui.available_height()).max(1.0);
        let output=crate::table::fixed_rows(ui,crate::table::Viewport{
            width:layout.table_width,height:viewport_height,rail:None
        },id,theme::MANAGEMENT_COMPACT_ROW_HEIGHT,count,|body,visible|{
            let origin=body.max_rect().min;
            let first=visible.start;
            for index in visible{
                let rect=crate::table::fixed_row_rect(origin,index,first,layout.table_width,
                    theme::MANAGEMENT_COMPACT_ROW_HEIGHT);
                row_left.get_or_insert(rect.left());
                crate::table::fixed_row(body,index,rect,index,&widths,gap,
                    crate::table::RowTone::SelectedIf(selected(index)),|row,cells|draw(row,cells,index,rect));
            }
        });
        let cells=crate::table::management_table_cells(
            egui::pos2(row_left.unwrap_or(layout.header_rect.left()-output.state.offset.x),
                layout.header_rect.top()),&widths,layout.header_rect.height(),gap);
        crate::table::header_columns(ui,layout.header_rect,layout.header_content_rect,&cells,gap,|ui,area|{
            for (i,cell) in cells.iter().enumerate(){
                crate::table::heading(ui,*cell,area,cell.intersect(area),
                    if i==0{crate::table::HeadingAlign::Left(theme::SPACE_8)}
                    else{crate::table::HeadingAlign::Center{min_left:None}},
                    typography::management_table_header_galley(ui,labels[i]),None);
            }
        });
        (output.inner_rect,output.inner_rect)
    });
    ui.spacing_mut().item_spacing.y=spacing;
    inner
}
pub(crate) fn registry_cell(ui:&Ui,rect:Rect,value:&str,role:typography::ManagementTableCellRole,
    center:bool,id:impl std::hash::Hash){
    crate::table::paint_management_text_cell(ui,rect.shrink2(Vec2::new(theme::SPACE_8,0.0)),
        value,value,role,if center{egui::Align2::CENTER_CENTER}else{egui::Align2::LEFT_CENTER},
        id,false);
}
pub(crate) fn registry_actions<R>(ui:&mut Ui,cell:Rect,draw:impl FnOnce(&mut Ui)->R)->R{
    ui.scope_builder(egui::UiBuilder::new().max_rect(cell.shrink2(Vec2::new(theme::MANAGEMENT_TABLE_ACTION_INSET,0.0)))
        .layout(egui::Layout::right_to_left(Align::Center)),|actions|{
            actions.set_clip_rect(actions.clip_rect().intersect(cell));
            draw(actions)
        }).inner
}

pub(crate) enum InlineHeader{
    Reception{editable:bool},
    Shipment,
}
pub(crate) fn inline<R>(
    ui:&mut Ui,header:InlineHeader,draw:impl FnOnce(&mut Ui)->R
)->R{
    let rect=match header{
        InlineHeader::Reception{editable}=>reception_items_header(ui,editable),
        InlineHeader::Shipment=>workflow_shipment_header(ui),
    };
    let result=draw(ui);
    finish_inline(ui,rect);
    result
}

/// Body policies preserve rail behavior and existing scroll IDs.
#[derive(Clone,Copy)]
pub(crate) struct Viewport {
    pub(crate) width:f32,
    pub(crate) height:f32,
    pub(crate) rail:Option<Rect>,
}
fn viewport_scroll(ui:&Ui,viewport:Viewport,id:impl std::hash::Hash)->egui::ScrollArea{
    let scroll=management_fixed_table_scroll_area(ui,viewport.width);
    // Native scrollbars are measured against the actual inner viewport.
    // A caller's requested rail is only a layout/trace hint, not an override
    // of the native track (especially when both axes are active).
    let _=viewport.rail;
    scroll.id_salt(id).max_height(viewport.height).auto_shrink([false,false])
}
pub(crate) fn fixed_rows<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,
    row_height:f32,count:usize,draw:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    fixed_rows_snapshot(ui,viewport,id,row_height,count,true,None,draw)
}
pub(crate) fn fixed_rows_snapshot<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,
    row_height:f32,count:usize,ready:bool,keys:Option<&dyn Fn(usize)->Option<egui::Id>>,
    draw:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    let rows=keys.map(|keys|crate::scrollbar::ScrollRows{count,key:keys,
        geometry:crate::scrollbar::RowGeometry::Fixed(row_height)});
    let scroll=viewport_scroll(ui,viewport,&id);
    let (scroll,capture)=crate::scrollbar::ScrollCapture::begin(ui,&id,scroll,ready,rows);
    let output=virtual_fixed_rows(scroll,ui,viewport.width,row_height,count,draw);
    capture.finish(ui,&output,rows);
    output
}
pub(crate) fn variable_rows<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,rows_id:impl std::hash::Hash,
    offsets:&[f32],draw:impl FnOnce(&mut Ui,egui::Pos2,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    variable_rows_snapshot(ui,viewport,id,rows_id,offsets,true,None,draw)
}
pub(crate) fn variable_rows_snapshot<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,rows_id:impl std::hash::Hash,
    offsets:&[f32],ready:bool,keys:Option<&dyn Fn(usize)->Option<egui::Id>>,
    draw:impl FnOnce(&mut Ui,egui::Pos2,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    debug_assert!(!offsets.is_empty());
    let count=offsets.len().saturating_sub(1);
    let rows=keys.map(|keys|crate::scrollbar::ScrollRows{count,key:keys,
        geometry:crate::scrollbar::RowGeometry::Variable(offsets)});
    let scroll=viewport_scroll(ui,viewport,&id);
    let (scroll,capture)=crate::scrollbar::ScrollCapture::begin(ui,&id,scroll,ready,rows);
    let output=scroll.show_viewport(ui,|body,visible|{
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
        let clip=strict_viewport_clip(body,visible);
        body.scope_builder(egui::UiBuilder::new().id_salt(rows_id).max_rect(visible_rect),|visible|{
            visible.spacing_mut().item_spacing.y=0.0;
            visible.set_clip_rect(clip);
            visible.skip_ahead_auto_ids(first);
            draw(visible,origin,first..last)
        }).inner
    });
    capture.finish(ui,&output,rows);
    output
}
pub(crate) fn register_rows<R>(
    ui:&mut Ui,height:f32,width:f32,id:impl std::hash::Hash,
    draw:impl FnOnce(&mut Ui)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    let scroll=management_register_list_scroll_area(ui,height).id_salt(&id);
    let (scroll,capture)=crate::scrollbar::ScrollCapture::begin(ui,&id,scroll,true,None);
    let output=scroll.show_viewport(ui,|body,viewport|{
            let clip=strict_viewport_clip(body,viewport);
            body.set_clip_rect(clip);
            body.apply_shared_width(crate::card::CardWidthPolicy::FillAvailable,width);
            draw(body)
        });
    capture.finish(ui,&output,None);
    output
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
        row.set_clip_rect(row.clip_rect().intersect(rect));
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
    let text=master_header_begin(ui,header,content);
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
    if response.hovered(){management_master_header_sort_hover(ui,hit);}
    response.clicked()
}

/// Expanded history rows use the same surface, with the nested shipment card
/// as an explicit detail profile. Keep the original paint order: row rule,
/// contents, column rules, then tree connectors.
pub(crate) fn history_row<const N:usize>(
    ui:&mut Ui,id:impl std::hash::Hash,rect:Rect,index:usize,
    widths:&[f32;N],gap:f32,detail:Option<Rect>,
    draw:impl FnOnce(&mut Ui),overlay:impl FnOnce(&mut Ui),
){
    row_scope(ui,id,rect,|row|{
        row.allocate_exact_size(rect.size(),egui::Sense::hover());
        if let Some(block)=detail{
            row.painter().rect_filled(rect,0.0,theme::CANVAS);
            crate::card::paint_shipment_detail_surface(row,block,theme::PANEL,false);
        }else{
            paint_management_table_row_background(row.painter(),rect,index);
            paint_management_table_row_rule(row.painter(),rect);
        }
        draw(row);
        if detail.is_none(){
            let cells=management_table_cells(rect.min,widths,rect.height(),gap);
            management_table_column_rules(row,rect,&cells,gap);
        }
        overlay(row);
    });
}

/// Identical sort-hover outline for all freight management list headers.
/// The caller supplies the column intersected with the 34pt text area.
fn management_master_header_sort_hover(ui:&Ui,hit:Rect){
    if hit.width()>0.0 && hit.height()>0.0 {
        let painter=ui.painter().with_clip_rect(hit);
        painter.rect_filled(
            hit,2.0,theme::TABLE_SORT_HOVER_TINT
        );
        painter.rect_stroke(
            hit.shrink(1.0),2.0,
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,
                theme::SELECTION.linear_multiply(0.82)),
            egui::StrokeKind::Inside
        );
    }
}


/// Fixed-header register lists retain a vertical-only rail and fixed viewport.
/// The caller already installs the management table style and owns the height.
pub(crate) fn management_register_list_scroll_area(ui:&Ui,height:f32)->egui::ScrollArea{
    let _=management_table_scroll_track_rect(ui,height);
    crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
        .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .max_height(height)
        .auto_shrink([false,false])
}

/// Render only the rows intersecting a scroll viewport, while reserving the
/// full logical height and width of the list. This works with BOTH-axis
/// scrolling, unlike a paginated list, and preserves the backing row indices.
///
/// Each caller must render each row at exactly `row_height` with no vertical
/// spacing; widgets/edits must be keyed by stable entity IDs rather than by
/// which row happened to be visible in the current frame.
pub(crate) fn virtual_fixed_rows<R>(
    scroll:egui::ScrollArea,
    ui:&mut Ui,
    content_width:f32,
    row_height:f32,
    total_rows:usize,
    add_visible:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
) -> egui::containers::scroll_area::ScrollAreaOutput<R> {
    debug_assert!(row_height>0.0 && row_height.is_finite());
    let output=scroll.show_viewport(ui,move |body,viewport|{
        management_table_row_body(body,content_width);
        body.set_min_height(row_height*total_rows as f32);
        let first=((viewport.top()/row_height).floor().max(0.0) as usize)
            .saturating_sub(1).min(total_rows);
        let last=((viewport.bottom()/row_height).ceil().max(0.0) as usize)
            .saturating_add(1).min(total_rows);
        let top=body.max_rect().top()+first as f32*row_height;
        let visible_rect=egui::Rect::from_min_size(
            egui::pos2(body.max_rect().left(),top),
            egui::vec2(content_width,(last-first) as f32*row_height),
        );
        let clip=strict_viewport_clip(body,viewport);
        body.scope_builder(
            egui::UiBuilder::new()
                .id_salt("virtual-fixed-rows")
                .max_rect(visible_rect),
            |visible|{
                visible.spacing_mut().item_spacing.y=0.0;
                visible.set_clip_rect(clip);
                visible.skip_ahead_auto_ids(first);
                add_visible(visible,first..last)
            }
        ).inner
    });
    #[cfg(test)]
    {
        crate::ui_test_support::record("row viewport",output.inner_rect);
        // Retain the measured content extent so transient failures can
        // distinguish native scrollbar reservation from card/layout drift.
        crate::ui_test_support::record("row content extent",
            Rect::from_min_size(output.inner_rect.min,output.content_size));
    }
    output
}

/// Fixed-width management tables must reserve horizontal scrollbar space
/// from the first frame whenever the approved columns exceed the available
/// width after the vertical gutter. Otherwise the native scrollbar's
/// previous-frame visibility can change the row viewport height by 10pt.
/// A narrow table reserves both rails deterministically; a fitting table
/// remains vertical-only with its existing conditional scrollbar.
pub(crate) fn management_fixed_table_scroll_area(ui:&Ui,content_width:f32)->egui::ScrollArea{
    use egui::containers::scroll_area::ScrollBarVisibility;
    let data_width=(ui.available_width()-ui.spacing().scroll.allocated_width()).max(1.0);
    let horizontal=content_width>data_width+0.01;
    crate::scrollbar::scroll_area(if horizontal{crate::scrollbar::ScrollAxes::Both}else{crate::scrollbar::ScrollAxes::Vertical}).scroll_bar_visibility(if horizontal{
        ScrollBarVisibility::AlwaysVisible
    }else{
        ScrollBarVisibility::VisibleWhenNeeded
    })
}

/// Requested viewport bounds for management-table layout and test traces.
/// Native scrollbars use their actual viewport, never an extended rail.
pub(crate) fn management_table_scroll_track_rect(
    ui:&Ui,scroll_height:f32
)->Rect {
    let track=management_table_scroll_track_geometry(
        ui.available_rect_before_wrap(),scroll_height
    );
    #[cfg(test)]
    crate::ui_test_support::record("scroll rail",track);
    track
}

/// Requested rail coincides with the body viewport: no independent bottom
/// extension or inset. The native scroll area handles horizontal reservation.
fn management_table_scroll_track_geometry(
    bounds:Rect,scroll_height:f32
)->Rect {
    debug_assert!(scroll_height.is_finite()&&scroll_height>=0.0);
    let rows_bottom=(bounds.top()+scroll_height).min(bounds.bottom());
    let track_bottom=rows_bottom.max(bounds.top());
    let track_top=bounds.top();
    Rect::from_min_max(
        egui::pos2(bounds.left(),track_top),
        egui::pos2(bounds.right(),track_bottom)
    )
}

/// One measured vertical transition for every fixed-header management table.
///
/// Call with `item_spacing.y == 0` *from the toolbar allocation onward*.
/// This owns the symmetric 12pt toolbar-to-divider breathing room,
/// 1pt divider, 12pt after it, and 36pt sticky header. Screens must
/// not add a separate `add_space` or rely on egui's preceding item spacing.
#[derive(Clone,Copy,Debug)]
pub(crate) struct ManagementTableHeader {
    pub(crate) header_rect:Rect,
    pub(crate) header_content_rect:Rect,
    pub(crate) viewport_width:f32,
    pub(crate) scrollbar_gutter:f32,
    pub(crate) table_width:f32,
}

/// Standard 25pt transition for a toolbar that touches the card cursor.
/// Existing inset panels can consume part of the 12pt pre-divider margin.
pub(crate) fn management_sticky_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,theme::SECTION_DIVIDER_GAP
    )
}

/// Pure SSOT conversion: the frame's existing bottom inset is part of the
/// 12pt pre-divider whitespace, not space to add on top of it.
#[cfg(test)]
fn management_remaining_predivider_gap(consumed_bottom_inset:f32)->f32{
    debug_assert!(consumed_bottom_inset.is_finite()&&consumed_bottom_inset>=0.0
        && consumed_bottom_inset<=theme::SECTION_DIVIDER_GAP,
        "toolbar bottom inset must fit inside the standard top gap");
    (theme::SECTION_DIVIDER_GAP-consumed_bottom_inset).max(0.0)
}

fn management_sticky_table_header_with_top_gap(
    ui:&mut Ui,min_table_width:f32,top_gap:f32
)->ManagementTableHeader{
    debug_assert!(ui.spacing().item_spacing.y.abs()<0.01,
        "table toolbar and header must use zero implicit vertical spacing");
    let viewport_width=ui.available_width().max(1.0);
    // One allocation for the full toolbar-to-header separator strip.
    let (_transition,_rule)=crate::ui_primitives::section_divider_with_top_gap(ui,top_gap);
    #[cfg(test)]
    crate::ui_test_support::record("toolbar transition",_transition);
    allocate_management_table_header(ui,viewport_width,min_table_width)
}

/// Bare-header variant for nested or independent lists. The data body can
/// exceed the viewport, but the header itself must remain inside its clip.
pub(crate) fn management_list_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    let viewport_width=ui.available_rect_before_wrap()
        .intersect(ui.clip_rect()).width().max(1.0);
    allocate_management_table_header(ui,viewport_width,min_table_width)
}

/// Both fixed-header variants share the allocation, data width and rail.
fn allocate_management_table_header(
    ui:&mut Ui,viewport_width:f32,min_table_width:f32
)->ManagementTableHeader{
    let scrollbar_gutter=ui.spacing().scroll.allocated_width();
    allocate_management_table_header_with_gutter(ui,viewport_width,min_table_width,scrollbar_gutter)
}

/// Nested tables scroll with their enclosing pane, so no second native
/// scrollbar gutter belongs to the header or the detail rows.
pub(crate) fn management_inline_list_table_header(ui:&mut Ui)->ManagementTableHeader{
    let width=ui.available_width().max(1.0);
    allocate_management_table_header_with_gutter(ui,width,width,0.0)
}
fn allocate_management_table_header_with_gutter(
    ui:&mut Ui,viewport_width:f32,min_table_width:f32,scrollbar_gutter:f32
)->ManagementTableHeader{
    let table_width=management_table_data_width(
        viewport_width,scrollbar_gutter,min_table_width
    );
    let (header_rect,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
        Sense::hover()
    );
    // The rightmost scrollbar rail is not another table column. Both the
    // sticky header and virtualized body should use the same data width.
    let header_content_rect=Rect::from_min_max(
        header_rect.min,
        egui::pos2(
            (header_rect.right()-scrollbar_gutter)
                .max(header_rect.left()+1.0),
            header_rect.bottom()
        )
    );
    // Fixed-header contract shared by BOTH approved master tables.
    // Validate real allocated Rects, not just intended token values.
    debug_assert!((header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<0.01,
        "management table header must retain the shared 36pt height");
    if header_rect.width()>=scrollbar_gutter+1.0 {
        debug_assert!((header_content_rect.right()+scrollbar_gutter
            -header_rect.right()).abs()<0.01,
            "header's data right edge must exclude the table scrollbar gutter");
    }
    #[cfg(test)]
    crate::ui_test_support::record("table header",header_rect);
    ManagementTableHeader{
        header_rect,header_content_rect,viewport_width,
        scrollbar_gutter,table_width
    }
}

/// A history overview without interactive toolbar controls still has the
/// same fixed-header origin as the assignment master: 36pt toolbar role plus
/// the approved 25pt transition. Reserve those 61pt as ONE separator area,
/// not a phantom interactive toolbar widget with its own implicit spacing.
/// This keeps the first row and the 1pt divider on the master coordinates.
pub(crate) fn management_sticky_table_header_without_toolbar(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader {
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,
        theme::INPUT_HEIGHT+theme::SECTION_DIVIDER_GAP
    )
}

/// Paint a freight master header's background and return its 34pt text area.
/// Shared by assignment, group management and combination history so text,
/// arrow centers and sort hit rects never use the lower 2pt outline.
pub(crate) fn master_header_begin(
    ui:&Ui,header_rect:Rect,header_content_rect:Rect
)->Rect {
    debug_assert!((header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT)
        .abs()<0.01);
    header_begin(ui,header_rect,header_content_rect)
}

/// Plain management-register header for two-column editor/list pages.
/// The same master allocates the 25pt toolbar transition, 36pt fixed header,
/// native scrollbar gutter and inner 2pt bottom rule. Only the row content
/// is allowed to be taller than a compact 42pt product row.
pub(crate) fn management_master_register_header(
    ui:&mut Ui,primary:&str,action:&str,action_width:f32
)->ManagementTableHeader{
    let layout=management_sticky_table_header(ui,0.0);
    let text_rect=master_header_begin(
        ui,layout.header_rect,layout.header_content_rect
    );
    let x=(text_rect.right()-action_width).max(text_rect.left());
    let left=typography::management_table_header_galley(ui,primary);
    let right=typography::management_table_header_galley(ui,action);
    let painter=ui.painter().with_clip_rect(text_rect);
    // Long alias/brand titles must not run under the right-hand
    // management column on a narrow workspace.
    let primary_rect=Rect::from_min_max(
        text_rect.min,egui::pos2(x,text_rect.bottom())
    );
    painter.with_clip_rect(primary_rect).galley(
        egui::pos2(text_rect.left()+theme::SPACE_8,
            text_rect.center().y-left.size().y/2.0),
        left,theme::SECONDARY
    );
    painter.galley(
        egui::pos2((x+(action_width-right.size().x)/2.0)
            .max(x).min(text_rect.right()),
            text_rect.center().y-right.size().y/2.0),
        right,theme::SECONDARY
    );
    management_master_column_rules(ui,text_rect,[x]);
    header_finish(ui,layout.header_rect);
    layout
}


/// Read-only embedded management table with the shared fixed-header
/// geometry, scrollbar gutter and virtual fixed-row viewport. Screens provide
/// only their column labels, width policy and already formatted cell values.
pub(crate) fn management_readonly_table(
    ui:&mut Ui,id:&str,headings:&[&str],rows:&[Vec<String>],max_height:f32,
    right_aligned:&[usize],widths_for_available:impl FnOnce(f32)->Vec<f32>,
){
    assert!(!headings.is_empty(),"a management table needs at least one column");
    let original_scroll=ui.spacing().scroll.clone();
    let original_spacing=management_table_container_style(ui);
    let viewport_width=ui.available_rect_before_wrap()
        .intersect(ui.clip_rect()).width().max(1.0);
    let available_data_width=(viewport_width
        -ui.spacing().scroll.allocated_width()).max(1.0);
    let widths=widths_for_available(available_data_width);
    assert_eq!(headings.len(),widths.len(),
        "management table heading and width counts must match");
    assert!(widths.iter().all(|width|width.is_finite()&&*width>0.0),
        "management table column widths must be finite and positive");
    let minimum_width=management_readonly_table_content_width(&widths);
    crate::table::show(ui,crate::table::Header::List(minimum_width),|ui,layout|{

    let row_height=management_readonly_table_row_advance();
    let mut row_left=None;
    let output=crate::table::fixed_rows(
        ui,crate::table::Viewport{width:layout.table_width,height:max_height,rail:None},
        id,row_height,rows.len(),
        |body,range|{
            let origin=body.max_rect().min;
            let first=range.start;
            for index in range{
                let values=&rows[index];
                debug_assert_eq!(values.len(),widths.len(),
                    "management table row must match its column count");
                let row_rect=fixed_row_rect(origin,index,first,layout.table_width,row_height);
                row_left.get_or_insert(row_rect.left());
                row_scope(body,index,row_rect,|row|{
                    row.set_clip_rect(row.clip_rect().intersect(row_rect));
                    row.allocate_exact_size(row_rect.size(),Sense::hover());
                    paint_management_table_row_background(row.painter(),row_rect,index);
                    let mut x=row_rect.left();
                    let columns=widths.iter().map(|width|{
                        let cell=Rect::from_min_size(egui::pos2(x,row_rect.top()),Vec2::new(*width,row_height));
                        x+=*width+theme::SPACE_8;
                        cell
                    }).collect::<Vec<_>>();
                    for (column,(value,cell)) in values.iter().zip(&columns).enumerate(){
                        paint_management_text_cell(row,cell.shrink2(Vec2::new(theme::SPACE_8,0.0)),
                            value,value,typography::ManagementTableCellRole::Text,
                            if right_aligned.contains(&column){egui::Align2::RIGHT_CENTER}else{egui::Align2::LEFT_CENTER},
                            (index,column),false);
                    }
                    management_table_column_rules(row,row_rect,&columns,theme::SPACE_8);
                    paint_management_table_row_rule(row.painter(),row_rect);
                    #[cfg(test)]
                    crate::ui_test_support::record("readonly data row",row_rect);
                });
            }
        }
    );
    let mut x=row_left.unwrap_or(layout.header_rect.left()-output.state.offset.x);
    let columns=widths.iter().map(|width|{
        let cell=Rect::from_min_size(egui::pos2(x,layout.header_rect.top()),
            Vec2::new(*width,layout.header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT));
        x+=*width+theme::SPACE_8;
        cell
    }).collect::<Vec<_>>();
    crate::table::header_columns(ui,layout.header_rect,layout.header_content_rect,&columns,theme::SPACE_8,|ui,area|{
        for (index,(heading,cell)) in headings.iter().zip(&columns).enumerate(){
            let alignment=if right_aligned.contains(&index){
                crate::table::HeadingAlign::Right{inset:theme::SPACE_8,min_left:None}
            }else{crate::table::HeadingAlign::Left(theme::SPACE_8)};
            let label=typography::management_table_header_galley(ui,heading);
            crate::table::heading(ui,*cell,area,cell.intersect(area),alignment,label,None);
        }
    });
    
            (output.inner_rect,())
        });

    ui.spacing_mut().scroll=original_scroll;
    ui.spacing_mut().item_spacing.y=original_spacing;
}

fn management_readonly_table_content_width(widths:&[f32])->f32{
    widths.iter().sum::<f32>()
        +widths.len().saturating_sub(1) as f32*theme::SPACE_8
}

fn management_readonly_table_row_advance()->f32{
    theme::MANAGEMENT_COMPACT_ROW_HEIGHT
}


// Shared text painters and standard table header/column geometry.
/// Shared clipped single-line management cell. Callers choose semantic role,
/// alignment and the existing column-specific insets; no business interaction
/// or row identity is owned by the painter.
/// For a deliberately shortened displayed value, preserve the original
/// text only when clipping or the explicit shortening flag warrants it.
/// Ordinary cells pass their display text as full text with shortened=false.
pub(crate) fn paint_management_text_cell(
    ui:&Ui,content:Rect,text:&str,full:&str,role:typography::ManagementTableCellRole,
    align:egui::Align2,id:impl std::hash::Hash,shortened:bool
){
    let visible=content.intersect(ui.clip_rect());
    if visible.width()<=0.0||visible.height()<=0.0{return;}
    let color=typography::management_table_cell_color(role);
    let galley=typography::management_table_cell_galley(ui,text,role);
    let x=if align==egui::Align2::RIGHT_CENTER{
        content.right()-galley.size().x
    }else if align==egui::Align2::CENTER_CENTER{
        content.center().x-galley.size().x/2.0
    }else{content.left()};
    let overflow=paint_table_galley(ui,content,x,galley,color);
    overflow_region_tooltip(ui,visible,ui.make_persistent_id(id),
        overflow||shortened,full);
}

/// Shared vertically centered text painting for ordinary and composite cells.
/// The caller owns the horizontal anchor, semantic font and interaction.
pub(crate) fn paint_table_galley(
    ui:&Ui,cell:Rect,x:f32,galley:std::sync::Arc<egui::Galley>,color:Color32
)->bool{
    let visible=cell.intersect(ui.clip_rect());
    let overflow=text_exceeds_rect(galley.size(),visible);
    if visible.width()>0.0&&visible.height()>0.0{
        ui.painter().with_clip_rect(visible).galley(
            egui::pos2(x,cell.center().y-galley.size().y/2.0),galley,color
        );
    }
    overflow
}

/// Shared expanded shipment detail geometry for History and Reception.
pub(crate) fn shipment_detail_content(block:Rect)->Rect{
    block.shrink2(Vec2::new(theme::SPACE_12.min(block.width()/2.0),0.0))
}

pub(crate) fn shipment_detail_cells<const N:usize>(block:Rect,widths:&[f32;N],gap:f32)->[Rect;N]{
    let content=shipment_detail_content(block);
    management_table_cells(content.min,widths,content.height(),gap)
}

pub(crate) fn shipment_detail_product_galley(
    ui:&Ui,cell_width:f32,text:&str,ink:Color32
)->std::sync::Arc<egui::Galley>{
    typography::freight_table_wrapped_galley_with_color(
        ui,text,(cell_width-theme::SPACE_12).max(1.0),ink)
}

pub(crate) fn shipment_detail_block_height(text_height:f32)->f32{
    (text_height+2.0*theme::SPACE_4).ceil().max(theme::ACTION_STANDARD_HEIGHT)
}

pub(crate) fn paint_shipment_detail_products(
    ui:&Ui,cell:Rect,galley:std::sync::Arc<egui::Galley>,ink:Color32
)->bool{
    let x=cell.left()+theme::SPACE_12/2.0;
    let pos=egui::pos2(x,cell.center().y-galley.size().y/2.0);
    let overflow=positioned_text_overflow(Rect::from_min_size(pos,galley.size()),
        cell.intersect(ui.clip_rect()),galley.elided);
    paint_table_galley(ui,cell,x,galley,ink);
    overflow
}

/// Only the responsive widths differ from History: this nested list has
/// no tracking column or management action column.
pub(crate) fn workflow_shipment_columns(outer:f32)->([f32;3],f32){
    let inside=(outer-2.0*theme::SPACE_12).max(0.0);
    let gap=theme::SPACE_8.min(inside*0.04);
    let name=96.0_f32.min(inside*0.25);
    let freight=116.0_f32.min(inside*0.23);
    let product=(inside-name-freight-2.0*gap).max(0.0);
    ([name,product,freight],gap)
}

pub(crate) fn workflow_shipment_header(ui:&mut Ui)->Rect{
    let layout=management_inline_list_table_header(ui);
    let area=master_header_begin(ui,layout.header_rect,layout.header_content_rect);
    let (widths,gap)=workflow_shipment_columns(layout.table_width);
    let cells=shipment_detail_cells(area,&widths,gap);
    for (index,label) in ["수취인","품목","운임"].iter().enumerate(){
        let cell=cells[index];
        let galley=typography::management_table_header_galley(ui,label);
        let x=if index==2{cell.right()-theme::SPACE_8-galley.size().x}
            else{cell.left()+theme::SPACE_12/2.0};
        paint_table_galley(ui,cell,x,galley,theme::SECONDARY);
    }
    management_table_column_rules(ui,area,&cells,gap);
    header_finish(ui,layout.header_rect);
    layout.header_rect
}

/// The three compact columns keep fixed widths; the address gets all spare
/// width. The header, rows and rules share this one layout computation.
pub(crate) fn management_client_column_widths(table_width:f32)->[f32;4]{
    let name=theme::MANAGEMENT_CLIENT_NAME_COLUMN_WIDTH;
    let phone=theme::MANAGEMENT_CLIENT_PHONE_COLUMN_WIDTH;
    let terms=theme::MANAGEMENT_CLIENT_TERMS_COLUMN_WIDTH;
    [name,(table_width-name-phone-terms).max(0.0),phone,terms]
}

pub(crate) fn management_client_column_edges(table_left:f32,table_width:f32)->[f32;3]{
    let [name,address,phone,_]=management_client_column_widths(table_width);
    [
        table_left+name,
        table_left+name+address,
        table_left+name+address+phone
    ]
}

/// Client columns share the EXACT 36pt / 34pt text / 2pt footer band
/// of Freight Assignment. Column widths and 12pt label starts still
/// belong to the client row contract; only the master chrome is reused.
/// The visible data rect excludes the scrollbar, and horizontally scrolled
/// labels move together with the row data rather than with the rail.
pub(crate) fn management_client_header(
    ui:&mut Ui,header:Rect,header_content_rect:Rect,
    table_left:f32,table_width:f32,current_sort:Option<(usize,bool)>
)->Option<usize>{
    let titles=["거래처","주소","전화번호","거래 조건"];

    let text_row=master_header_begin(
        ui,header,header_content_rect
    );
    let column_widths=management_client_column_widths(table_width);
    let mut x=table_left;
    let mut selected=None;
    for (i,title) in titles.iter().enumerate(){
        let col_width=column_widths[i];
        let column=Rect::from_min_max(
            egui::pos2(x,text_row.top()),
            egui::pos2(x+col_width,text_row.bottom())
        );
        let visible=column.intersect(text_row);
        if visible.width()>0.0 {
            if sort_clicked(ui,visible,("client-management-sort",i)){
                selected=Some(i);
            }
            let text_rect=Rect::from_min_max(
                egui::pos2(column.left()+theme::SPACE_12,text_row.top()),
                egui::pos2(column.right()-theme::SPACE_12,text_row.bottom())
            ).intersect(text_row);
            if text_rect.width()>0.0 {
                let title_galley=typography::management_table_header_galley(ui,title);
                let (arrow,color)=match current_sort{
                    Some((col,false)) if col==i=>("▲",theme::SELECTION),
                    Some((col,true)) if col==i=>("▼",theme::SELECTION),
                    _=>("↕",theme::MUTED)
                };
                let arrow_galley=typography::management_table_header_galley(ui,arrow);
                heading(ui,column,text_row,text_rect,
                    HeadingAlign::Left(theme::SPACE_12),title_galley,Some((arrow_galley,color)));
            }
        }
        x+=col_width;
    }
    management_master_column_rules(
        ui,text_row,
        management_client_column_edges(table_left,table_width)
    );
    header_finish(ui,header);
    selected
}

pub(crate) fn reception_item_columns(rect: Rect) -> [Rect; 3] {
    let qty=theme::RECEPTION_ITEMS_QTY_WIDTH;
    let amount=theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH;
    let name_right=rect.right()-qty-amount;
    let qty_right=name_right+qty;
    [
        Rect::from_min_max(rect.min,egui::pos2(name_right,rect.bottom())),
        Rect::from_min_max(egui::pos2(name_right,rect.top()),
            egui::pos2(qty_right,rect.bottom())),
        Rect::from_min_max(egui::pos2(qty_right,rect.top()),rect.max),
    ]
}

/// Product text needs equal breathing room on both sides of the blue
/// outline. Header and rows use the same 4pt extra horizontal inset;
/// quantity/amount alignment and highlight geometry remain untouched.
pub(crate) fn reception_item_text_clip(rect:Rect,index:usize)->Rect {
    if index==0 {
        rect.shrink2(egui::vec2(theme::SPACE_4,0.0))
    }else{rect}
}

/// One shared item header; remove_slot preserves the action-column footprint.
pub(crate) fn reception_items_header(ui:&mut Ui,remove_slot:bool)->Rect{
    let action_width=if remove_slot{theme::RECEPTION_ITEM_REMOVE_WIDTH}else{0.0};
    let width=management_table_data_width(
        ui.available_width(),0.0,theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+action_width
    );
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_HEADER_HEIGHT), Sense::hover()
    );
    // The reception list has its own approved 32pt compact header, but
    // follows the master's raised header surface and INSIDE bottom band.
    // No extra layout height or clipped 1pt edge is introduced.
    let text_rect=header_begin(ui,rect,rect);
    let columns = reception_item_columns(Rect::from_min_max(
        rect.min,egui::pos2(rect.right()-action_width,rect.bottom())
    ));
    for (index, label) in ["품목", "수량", "금액"].iter().enumerate() {
        // Header and value cells use identical text start coordinates.
        let cell = reception_item_text_clip(
            columns[index].shrink2(egui::vec2(theme::SPACE_8, 0.0)),index
        );
        let galley = typography::management_table_header_galley(ui, label);
        // The product header starts at the same LEFT edge as product text;
        // quantity and amount end at the same RIGHT edges as their values.
        let x = if index == 0 {
            cell.left()
        } else {
            cell.right() - galley.size().x
        };
        let text_center_y = text_rect.center().y;
        ui.painter().with_clip_rect(cell).galley(
            egui::pos2(x, text_center_y - galley.size().y / 2.0),
            galley, theme::SECONDARY,
        );
    }
    header_finish(ui,rect);
    rect
}

/// Use the actual laid-out glyph box for all value cells.
/// Product text is left aligned; quantity and amount share a right edge and
/// the exact same vertical midpoint independently of their string length.
pub(crate) fn reception_item_value_origin(cell:Rect,glyphs:Vec2,right:bool)->egui::Pos2 {
    egui::pos2(
        if right {cell.right()-glyphs.x} else {cell.left()},
        cell.center().y-glyphs.y/2.0,
    )
}

/// Semantic labels for management table columns. A screen selects the
/// column role; the shared component owns the displayed header wording.
/// This does not change the underlying product description or column size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagementTableColumnLabel {
    ProductName,
}

impl ManagementTableColumnLabel {
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::ProductName => "상품명",
        }
    }
}

#[cfg(test)]
#[test]
fn registry_balances_text_columns_without_widening_actions(){
    let columns=RegistryColumns::balanced_text([240.0,220.0,96.0,108.0],2);
    let widths=columns.widths(1200.0,theme::SPACE_8);
    assert_eq!(widths,[496.0,476.0,96.0,108.0]);
    assert_eq!(columns.widths(500.0,theme::SPACE_8),columns.base);
}

#[cfg(test)]
#[test]
fn registry_uses_compact_rows_without_a_phantom_toolbar(){
    let ctx=crate::ui_test_support::test_context();
    let _=ctx.run(egui::RawInput{
        screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,500.0))),
        ..Default::default()
    },|ctx|{
        egui::CentralPanel::default().show(ctx,|ui|{
            let top=ui.cursor().top();
            let mut rows=Vec::new();
            let viewport=registry_table(ui,"registry-compact-geometry",
                ["name","action"],[240.0,100.0],160.0,3,|_|false,
                |_,_,_,rect|rows.push(rect));
            assert_eq!(rows.len(),3);
            assert!((viewport.top()-top-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<=0.5);
            for rect in &rows{
                assert!((rect.height()-theme::PRODUCT_TABLE_ROW_HEIGHT).abs()<=0.5);
            }
            for pair in rows.windows(2){
                assert!((pair[1].top()-pair[0].bottom()).abs()<=0.5);
            }
        });
    });
}

#[cfg(test)]
mod regression_tests;
