//! Shared table renderer: surface, header, row/viewport geometry and sort hit testing.
//! Screens retain column data, stable IDs and business sort state.
use eframe::egui::{self,Color32,Rect,Stroke,Ui,Vec2};
use crate::{theme,typography};
use eframe::egui::Sense;

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
    let mut scroll=management_fixed_table_scroll_area(ui,viewport.width);
    if let Some(rail)=viewport.rail{scroll=scroll.scroll_bar_rect(rail);}
    scroll.id_salt(id).max_height(viewport.height).auto_shrink([false,false])
}
pub(crate) fn fixed_rows<R>(
    ui:&mut Ui,viewport:Viewport,id:impl std::hash::Hash,
    row_height:f32,count:usize,draw:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
)->egui::containers::scroll_area::ScrollAreaOutput<R>{
    let scroll=viewport_scroll(ui,viewport,id);
    virtual_fixed_rows(scroll,ui,viewport.width,row_height,count,draw)
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
    management_register_list_scroll_area(ui,height).id_salt(id)
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
            crate::components::paint_shipment_detail_surface(row,block,theme::PANEL,false);
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
pub fn management_register_list_scroll_area(ui:&Ui,height:f32)->egui::ScrollArea{
    crate::components::scroll_area(crate::components::ScrollAxes::Vertical)
        .scroll_bar_rect(management_table_scroll_track_rect(ui,height))
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
pub fn virtual_fixed_rows<R>(
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
        let clip=body.clip_rect();
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
        crate::components::general_screen_geometry_tests::record("row viewport",output.inner_rect);
        // Retain the measured content extent so transient failures can
        // distinguish native scrollbar reservation from card/layout drift.
        crate::components::general_screen_geometry_tests::record("row content extent",
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
pub fn management_fixed_table_scroll_area(ui:&Ui,content_width:f32)->egui::ScrollArea{
    use egui::containers::scroll_area::ScrollBarVisibility;
    let data_width=(ui.available_width()-ui.spacing().scroll.allocated_width()).max(1.0);
    let horizontal=content_width>data_width+0.01;
    crate::components::scroll_area(if horizontal{crate::components::ScrollAxes::Both}else{crate::components::ScrollAxes::Vertical}).scroll_bar_visibility(if horizontal{
        ScrollBarVisibility::AlwaysVisible
    }else{
        ScrollBarVisibility::VisibleWhenNeeded
    })
}

/// One shared 7pt rail extension for EVERY management table, regardless of
/// its page, outer frame or footer. Each caller owns ONLY the viewport height
/// (and reserves its footer separately); the scrollbar offset is not a
/// per-page layout policy. The native 3pt bottom inset leaves 4pt beyond rows.
pub fn management_table_scroll_track_rect(
    ui:&Ui,scroll_height:f32
)->Rect {
    let track=management_table_scroll_track_geometry(
        ui.available_rect_before_wrap(),scroll_height
    );
    #[cfg(test)]
    crate::components::general_screen_geometry_tests::record("scroll rail",track);
    track
}

/// The only place that computes top/bottom of a management table rail.
/// The rail starts exactly below its fixed header (0pt gap); 7pt of common
/// extension minus 3pt optical shortening puts its end 4pt past the body.
/// The row viewport, data columns and any reserved footer stay unchanged.
pub(crate) fn management_table_scroll_track_geometry(
    bounds:Rect,scroll_height:f32
)->Rect {
    debug_assert!(scroll_height.is_finite()&&scroll_height>=0.0);
    let rows_bottom=(bounds.top()+scroll_height).min(bounds.bottom());
    let track_bottom=(rows_bottom+theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION
        -theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET)
        .max(bounds.top()+1.0);
    let track_top=(bounds.top()+theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET)
        .min(track_bottom-1.0);
    Rect::from_min_max(
        egui::pos2(bounds.left(),track_top),
        egui::pos2(bounds.right(),track_bottom)
    )
}

/// One measured vertical transition for every fixed-header management table.
///
/// Call with `item_spacing.y == 0` *from the toolbar allocation onward*.
/// This owns the symmetric 12pt toolbar-to-divider breathing room,
/// 1pt divider, 4pt after it, and 36pt sticky header. Screens must
/// not add a separate `add_space` or rely on egui's preceding item spacing.
#[derive(Clone,Copy,Debug)]
pub struct ManagementTableHeader {
    pub header_rect:Rect,
    pub header_content_rect:Rect,
    pub viewport_width:f32,
    pub scrollbar_gutter:f32,
    pub table_width:f32,
}

/// Standard 25pt transition for a toolbar that touches the card cursor.
/// Existing inset panels can consume part of the 12pt pre-divider margin.
pub fn management_sticky_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,theme::SECTION_DIVIDER_GAP
    )
}

/// Pure SSOT conversion: the frame's existing bottom inset is part of the
/// 12pt pre-divider whitespace, not space to add on top of it.
#[cfg(test)]
pub(crate) fn management_remaining_predivider_gap(consumed_bottom_inset:f32)->f32{
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
    let (_transition,_rule)=crate::components::section_divider_with_top_gap(ui,top_gap);
    #[cfg(test)]
    crate::components::general_screen_geometry_tests::record("toolbar transition",_transition);
    allocate_management_table_header(ui,viewport_width,min_table_width)
}

/// Bare-header variant for nested or independent lists. The data body can
/// exceed the viewport, but the header itself must remain inside its clip.
pub fn management_list_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    let viewport_width=ui.available_rect_before_wrap()
        .intersect(ui.clip_rect()).width().max(1.0);
    allocate_management_table_header(ui,viewport_width,min_table_width)
}

/// Both fixed-header variants share the allocation, data width and rail.
pub(crate) fn allocate_management_table_header(
    ui:&mut Ui,viewport_width:f32,min_table_width:f32
)->ManagementTableHeader{
    let scrollbar_gutter=ui.spacing().scroll.allocated_width();
    allocate_management_table_header_with_gutter(ui,viewport_width,min_table_width,scrollbar_gutter)
}

/// Nested tables scroll with their enclosing pane, so no second native
/// scrollbar gutter belongs to the header or the detail rows.
pub fn management_inline_list_table_header(ui:&mut Ui)->ManagementTableHeader{
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
    crate::components::general_screen_geometry_tests::record("table header",header_rect);
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
pub fn management_sticky_table_header_without_toolbar(
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
pub fn management_master_register_header(
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

