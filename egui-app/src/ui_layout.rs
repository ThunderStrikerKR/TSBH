//! Shared UI layout allocations, reserved geometry and dock rendering.
//! Business state and persisted window/split sizes remain in layout.rs.
use eframe::egui::{self,Align,Color32,Layout,Rect,Sense,Ui,UiBuilder,Vec2};
use crate::{theme,typography};
use crate::ui_primitives::{text_exceeds_rect,truncated_label_for_explicit_tooltip,truncated_label,overflow_tooltip};
use crate::scrollbar::{scroll_area,ScrollAxes};
use crate::card::docked_section_body_height;
use crate::table::management_table_data_width;

pub(crate) mod geometry;
pub(crate) mod pixel_paint;
use geometry::{aligned_rect,AlignedUiBuilderExt,SharedAllocationExt,SharedUiClipExt,
    set_fixed_height,reserve_min_height,CardWidthPolicy,UiWidthPolicyExt};

#[cfg(test)]
mod logical_alignment_tests{
    use super::*;
    use egui::emath::GuiRounding as _;
    use super::geometry::{SharedPainterClipExt,reserve_min_size};

    #[test]
    fn shared_size_constraints_preserve_native_layout_semantics(){
        use super::geometry::apply_width_policy;
        let ctx=crate::ui_test_support::test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(600.0,500.0));
        let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|root|{
                let bounds=Rect::from_min_size(egui::pos2(20.0,20.0),Vec2::new(300.0,200.0));
                for (index,target) in [43.01_f32,81.19,123.0].into_iter().enumerate(){
                    let child=|root:&mut Ui,key|root.new_child(UiBuilder::new()
                        .id_salt((index,key)).aligned_max_rect(bounds).layout(Layout::top_down(Align::Min)));
                    let mut fixed=child(root,"fixed");
                    let mut reference=child(root,"native fixed");
                    set_fixed_height(&mut fixed,target);
                    reference.set_height(target.round_ui());
                    assert_eq!(fixed.min_rect(),reference.min_rect());
                    assert_eq!(fixed.max_rect(),reference.max_rect());
                    assert_eq!(fixed.min_rect().height(),target.round_ui());
                    let mut minimum=child(root,"minimum");
                    let mut reference=child(root,"native minimum");
                    reserve_min_height(&mut minimum,target);
                    reference.set_min_height(target.round_ui());
                    assert_eq!(minimum.min_rect(),reference.min_rect());
                    assert_eq!(minimum.max_rect(),reference.max_rect());
                    let mut sized=child(root,"min size");
                    let mut reference=child(root,"native min size");
                    reserve_min_size(&mut sized,Vec2::new(target+10.0,target));
                    reference.set_min_size(Vec2::new((target+10.0).round_ui(),target.round_ui()));
                    assert_eq!(sized.min_rect(),reference.min_rect());
                    assert_eq!(sized.max_rect(),reference.max_rect());
                    let mut width=child(root,"width");
                    let mut reference=child(root,"native width");
                    apply_width_policy(&mut width,CardWidthPolicy::Fixed,target);
                    reference.set_width(target.round_ui());
                    assert_eq!(width.min_rect(),reference.min_rect());
                    assert_eq!(width.max_rect(),reference.max_rect());
                    let mut bounded=child(root,"bounded width");
                    let mut old_bounded=child(root,"previous bounded width");
                    apply_width_policy(&mut bounded,CardWidthPolicy::Bounded,target);
                    old_bounded.set_width(target.round_ui());
                    old_bounded.set_max_width(target.round_ui());
                    assert_eq!(bounded.min_rect(),old_bounded.min_rect());
                    assert_eq!(bounded.max_rect(),old_bounded.max_rect());
                    assert_eq!(bounded.cursor(),old_bounded.cursor());
                }
            });
        });
    }

    #[test]
    fn shared_allocation_preserves_native_rects_responses_and_cursor(){
        let ctx=crate::ui_test_support::test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(640.0,480.0));
        let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|root|{
                let bounds=Rect::from_min_size(egui::pos2(32.0,32.0),Vec2::new(540.0,380.0));
                let size=Vec2::new(83.019,24.113);
                let expected=size.round_ui();
                for (index,layout) in [
                    Layout::top_down(Align::Min),
                    Layout::left_to_right(Align::Center),
                    Layout::right_to_left(Align::Center),
                    Layout::top_down(Align::Center).with_cross_justify(true),
                ].into_iter().enumerate(){
                    let mut actual=root.new_child(UiBuilder::new().id_salt((index,"allocation actual"))
                        .aligned_max_rect(bounds).layout(layout));
                    let mut native=root.new_child(UiBuilder::new().id_salt((index,"allocation native"))
                        .aligned_max_rect(bounds).layout(layout));
                    let (a,ar)=actual.shared_allocate_exact_size(size,Sense::click());
                    let (b,br)=native.allocate_exact_size(expected,Sense::click());
                    assert_eq!(a,b);
                    assert_eq!(ar.rect,br.rect);
                    assert_eq!(actual.cursor(),native.cursor());
                    let (_,a)=actual.shared_allocate_space(size);
                    let (_,b)=native.allocate_space(expected);
                    assert_eq!(a,b);
                    assert_eq!(actual.cursor(),native.cursor());
                    let a=actual.shared_allocate_response(size,Sense::hover());
                    let b=native.allocate_response(expected,Sense::hover());
                    assert_eq!(a.rect,b.rect);
                    assert_eq!(actual.cursor(),native.cursor());
                    let draw=|ui:&mut Ui|ui.label("allocation").rect;
                    let a=actual.shared_allocate_ui_with_layout(size,Layout::top_down(Align::Min),draw);
                    let b=native.allocate_ui_with_layout(expected,Layout::top_down(Align::Min),draw);
                    assert_eq!(a.inner,b.inner);
                    assert_eq!(a.response.rect,b.response.rect);
                    let a=actual.shared_add_sized(size,egui::Button::new("button"));
                    let b=native.add_sized(expected,egui::Button::new("button"));
                    assert_eq!(a.rect,b.rect);
                    assert_eq!(actual.cursor(),native.cursor());
                    assert_eq!(actual.min_rect(),native.min_rect());
                }
            });
        });
    }

    #[test]
    fn shared_clip_preserves_parent_bounds_and_layout(){
        let ctx=crate::ui_test_support::test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(640.0,480.0));
        let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|root|{
                let parent=Rect::from_min_max(egui::pos2(40.013,50.019),egui::pos2(230.17,210.23));
                let requested=Rect::from_min_max(egui::pos2(10.01,80.03),egui::pos2(180.19,270.04));
                let bounds=Rect::from_min_max(egui::pos2(20.0,20.0),egui::pos2(300.0,300.0));
                let mut child=root.new_child(UiBuilder::new().id_salt("shared clip test")
                    .aligned_max_rect(bounds).layout(Layout::top_down(Align::Min)));
                child.use_owner_pane_clip(parent);
                let cursor=child.cursor();
                let min=child.min_rect();
                let max=child.max_rect();
                let expected=parent.intersect(requested);
                let painter=child.painter().with_shared_clip(requested);
                assert_eq!(painter.clip_rect(),expected);
                assert_eq!(child.clip_rect(),parent);
                child.restrict_shared_clip(requested);
                assert_eq!(child.clip_rect(),expected);
                child.restrict_shared_clip(bounds);
                assert_eq!(child.clip_rect(),expected,"child cannot widen inherited clip");
                let outside=Rect::from_min_max(egui::pos2(310.0,310.0),egui::pos2(340.0,340.0));
                child.restrict_shared_clip(outside);
                assert!(!child.clip_rect().is_positive(),"disjoint clipping must not expose any content");
                assert_eq!(child.cursor(),cursor);
                assert_eq!(child.min_rect(),min);
                assert_eq!(child.max_rect(),max);
            });
        });
    }

    #[test]
    fn adjacent_fractional_rects_share_rounded_edges_without_drift(){
        for scale in [1.0_f32,1.25,1.5,1.75,2.0]{
            let origin=egui::pos2(7.13/scale,11.17/scale);
            let outer=Rect::from_min_size(origin,Vec2::new(615.23/scale,801.19/scale));
            let edge=outer.top()+outer.height()*0.425;
            let top=aligned_rect(Rect::from_min_max(outer.min,egui::pos2(outer.right(),edge)));
            let bottom=aligned_rect(Rect::from_min_max(egui::pos2(outer.left(),edge),outer.max));
            assert_eq!(top.bottom(),bottom.top());
            assert_eq!(top.union(bottom),aligned_rect(outer));
            for rect in [top,bottom]{
                assert_eq!(rect,rect.round_ui());
                assert_eq!(rect,aligned_rect(rect));
            }
        }
    }
}

/// Semantic layout profiles. Screen modules choose a content role, never
/// a numeric spacing override. Preserve the reviewed spacing for each role.
#[derive(Clone,Copy)]
pub(crate) enum SpacingRole{
    Toolbar,ToolbarInput,CompactActions,SummaryStrip,FormColumns,
    AccountFields,ReportMetrics,ReportMetricPair,CompactChooser,SplitWorkspace,
    ExplicitVertical,ExplicitHorizontal,DocumentBody,WorkflowDetail,TagChips,KakaoIconMode,
}
pub(crate) fn apply_spacing_role(ui:&mut Ui,role:SpacingRole){
    if matches!(role,SpacingRole::ExplicitVertical|SpacingRole::DocumentBody){
        crate::ui_primitives::apply_explicit_vertical_spacing(ui);
        return;
    }
    let spacing=ui.spacing_mut();
    match role{
        SpacingRole::Toolbar=>spacing.item_spacing.x=theme::SPACE_8,
        SpacingRole::ToolbarInput=>{
            spacing.item_spacing.x=theme::SPACE_8;
            spacing.interact_size.y=theme::INPUT_HEIGHT;
        }
        SpacingRole::CompactActions=>spacing.item_spacing.x=theme::SPACE_4,
        SpacingRole::SummaryStrip|SpacingRole::TagChips=>spacing.item_spacing=Vec2::splat(theme::SPACE_8),
        SpacingRole::FormColumns|SpacingRole::SplitWorkspace=>spacing.item_spacing.x=theme::SPACE_12,
        SpacingRole::AccountFields=>spacing.item_spacing.x=theme::SPACE_8,
        SpacingRole::ReportMetrics=>spacing.item_spacing=Vec2::new(theme::SPACE_12,theme::SPACE_8),
        SpacingRole::ReportMetricPair=>spacing.item_spacing.x=theme::SPACE_4,
        SpacingRole::CompactChooser=>spacing.item_spacing.y=theme::SPACE_4,
        SpacingRole::ExplicitVertical|SpacingRole::DocumentBody=>unreachable!(),
        SpacingRole::ExplicitHorizontal=>spacing.item_spacing.x=0.0,
        SpacingRole::KakaoIconMode=>spacing.item_spacing.x=theme::KAKAO_ICON_MODE_GAP,
        SpacingRole::WorkflowDetail=>spacing.item_spacing.y=theme::SPACING,
    }
}

/// A direct-entry worksheet has no left document column; retain the
/// approved manual split gap only when both columns are visible.
pub(crate) fn apply_manual_workspace_spacing(ui:&mut Ui,direct_input:bool){
    ui.spacing_mut().item_spacing.x=if direct_input{0.0}else{theme::MANUAL_COLUMN_GAP};
}

/// Allocate a row with an immutable parent footprint. The child can
/// render optional labels, validation messages or actions without changing
/// the next sibling's Rect when its contents appear or disappear.
pub(crate) fn fixed_row<R>(
    ui:&mut Ui,size:Vec2,layout:Layout,
    add_contents:impl FnOnce(&mut Ui)->R,
)->(Rect,R){
    let width=size.x.min(ui.available_width()).max(0.0);
    let height=size.y.max(0.0);
    // Allocate the parent footprint exactly once. allocate_ui_with_layout
    // negotiates its response rect with the child's minimum content size,
    // so a 20pt label could silently expand an intended 18pt status slot.
    let (rect,_response)=ui.shared_allocate_exact_size(
        Vec2::new(width,height),Sense::hover()
    );
    let rect=aligned_rect(rect);
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("fixed layout row",rect,ui.clip_rect(),None);
    // Render in a detached child Ui: its content can grow internally, but
    // it cannot advance the parent's cursor or move the next sibling.
    let mut row=ui.new_child(
        UiBuilder::new().aligned_max_rect(rect).layout(layout)
    );
    row.restrict_shared_clip(rect.intersect(ui.clip_rect()));
    row.spacing_mut().item_spacing.y=0.0;
    let result=add_contents(&mut row);
    (rect,result)
}

/// A result list owns one bounded viewport regardless of its content.
pub(crate) fn fixed_result_viewport<R>(
    ui:&mut Ui,id:&str,height:f32,render:impl FnOnce(&mut Ui)->R
)->(Rect,R){
    let width=ui.available_width();
    fixed_row(ui,Vec2::new(width,height),Layout::top_down(Align::Min),|pane|{
        reserve_min_height(pane,height);
        scroll_area(ScrollAxes::Vertical).id_salt(id)
            .max_height(height).auto_shrink([false,false])
            .show(pane,render).inner
    })
}

/// One-line status slot for state-dependent text. It always reserves the
/// same width and height; long messages are clipped visually and remain
/// available in the hover tooltip. Use a fixed-height row for action controls.
pub(crate) fn fixed_status_slot(
    ui:&mut Ui,width:f32,height:f32,display:Option<&str>,
    tooltip:Option<&str>,color:Color32,
)->Rect{
    let width=width.min(ui.available_width()).max(0.0);
    let (rect,())=fixed_row(
        ui,Vec2::new(width,height),Layout::left_to_right(Align::Center),
        |slot|{
            if let Some(text)=display.filter(|text|!text.trim().is_empty()){
                let display=if color==theme::DANGER{
                    crate::user_error::display("status",text)
                }else{std::borrow::Cow::Borrowed(text)};
                let natural=slot.painter().layout_no_wrap(display.as_ref().to_owned(),
                    egui::FontId::proportional(typography::FONT_METADATA),color);
                let overflow=text_exceeds_rect(natural.size(),slot.available_rect_before_wrap().intersect(slot.clip_rect()));
                let label=typography::status(display.as_ref(),color);
                let response=slot.add(if tooltip.is_some(){
                    truncated_label_for_explicit_tooltip(label)
                }else{truncated_label(label)});
                if let Some(tooltip)=tooltip{
                    let detail=if color==theme::DANGER{
                        crate::user_error::display("status",tooltip)
                    }else{std::borrow::Cow::Borrowed(tooltip)};
                    overflow_tooltip(response,overflow,detail.as_ref());
                }
            }
        }
    );
    rect
}

/// The fixed goods viewport owns its allocated rectangle. Content may
/// scroll, but an egui child reporting a larger min_rect must not grow
/// the surrounding card, overlap its footer, or shift sibling sections.
/// Reserve the parent's slot once and render the scroll area independently.
pub(crate) fn reception_goods_viewport(
    ui:&mut Ui,height:f32,content:impl FnOnce(&mut Ui)
){
    if let Some(mut child)=reception_fixed_viewport(ui,height,"reception-goods-viewport"){
        child.spacing_mut().item_spacing.y=0.0;
        content(&mut child);
    }
}

/// Shared detached viewport geometry. Distinct caller IDs and spacing
/// policies are deliberately retained; a child cannot reflow its parent.
fn reception_fixed_viewport(ui:&mut Ui,height:f32,id:&'static str)->Option<Ui>{
    let size=Vec2::new(ui.available_width().max(0.0),height.max(0.0));
    let (rect,_)=ui.shared_allocate_exact_size(size,Sense::hover());
    let rect=aligned_rect(rect);
    if !rect.is_positive(){return None;}
    let mut child=ui.new_child(UiBuilder::new().id_salt(id)
        .aligned_max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.restrict_shared_clip(rect);
    child.apply_shared_width(CardWidthPolicy::Fixed,rect.width());
    set_fixed_height(&mut child,rect.height());
    Some(child)
}

/// Review and recipient content scroll within the reserved body without
/// enlarging their fixed section frames. The actual remaining height wins.
pub(crate) fn reception_section_scroll(
    ui:&mut Ui,section_height:f32,id_salt:impl std::hash::Hash,
    content:impl FnOnce(&mut Ui),
){
    let height=docked_section_body_height(section_height)
        .min(ui.available_height().max(0.0));
    if let Some(mut child)=reception_fixed_viewport(ui,height,"reception-section-viewport"){
        scroll_area(ScrollAxes::Vertical).id_salt(id_salt)
            .max_height(height).auto_shrink([false,false]).show(&mut child,content);
    }
}

/// Docked reception goods own their fixed viewport; the manual worksheet
/// scrolls its complete card stack instead. Both use the same item table
/// spacing and minimum-width rules without introducing another child ID.
fn reception_items_body(ui:&mut Ui,width:f32,exact_width:bool){
    if exact_width{ui.apply_shared_width(CardWidthPolicy::Fixed,width);}else{ui.apply_shared_width(CardWidthPolicy::FillAvailable,width);}
    ui.spacing_mut().item_spacing.y=0.0;
}

pub(crate) fn reception_worksheet_items_body(ui:&mut Ui){
    let width=ui.available_width().max(
        theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+theme::RECEPTION_ITEM_REMOVE_WIDTH
    );
    reception_items_body(ui,width,false);
}

/// Preserve the established 12pt worksheet stack gap in one semantic rule.
/// Docked panes retain their independent draggable splitter instead.
pub(crate) const RECEPTION_STACKED_CARD_GAP:f32=theme::SPACE_12;

pub(crate) fn reception_docked_goods_scroll(
    ui:&mut Ui,section_height:f32,id_salt:impl std::hash::Hash,
    editable_columns:bool,content:impl FnOnce(&mut Ui)
){
    ui.spacing_mut().item_spacing.y=0.0;
    ui.spacing_mut().scroll=crate::scrollbar::management_table_scroll_style();
    let height=docked_section_body_height(section_height)
        .min(ui.available_height().max(0.0));
    reception_goods_viewport(ui,height,|area|{
        let minimum=theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+
            if editable_columns{theme::RECEPTION_ITEM_REMOVE_WIDTH}else{0.0};
        // Reserve the rail even for short lists so overflow never shifts columns.
        let width=management_table_data_width(
            area.available_width(),area.spacing().scroll.allocated_width(),minimum
        );
        scroll_area(ScrollAxes::Both)
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .id_salt(id_salt).max_height(height).auto_shrink([false,false])
            .show(area,|items|{
                reception_items_body(items,width,true);
                content(items);
            });
    });
}

/// Use the same 8/1/8pt clearance as the reception section header.
pub(crate) fn reception_message_footer_gap(ui:&mut Ui){
    crate::ui_primitives::section_divider_symmetric(ui,theme::RECEPTION_SECTION_DIVIDER_GAP);
}

/// The 6pt split interaction region is always the workspace (CANVAS)
/// between PANEL cards, including the Kakao reservation boundary.
/// Only hover/drag shows an axis cue; drag targets and saved sizing persist.
pub(crate) fn paint_dock_splitter(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    ui.painter().rect_filled(rect, 0.0, theme::surface_palette(theme::SurfaceRole::Workspace,theme::VisualState::Normal).fill);
    paint_dock_axis_line(ui, rect, vertical, active);
}

/// Pixel-exact axis line within a gutter rectangle. A 6-*physical*-pixel
/// gutter has a center between two pixels, so its normal line must occupy
/// the middle TWO pixels. A one-pixel line can only sit to one side.
/// Odd-width gutters use a single centered pixel. Hover uses the shared
/// 2pt token, adjusted only when necessary for symmetrical pixel margins.
fn paint_dock_axis_line(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    // No permanent 1-2pt splitter bar: section/card strokes are enough.
    // A thin resize cue appears only when the pointer is over its hit zone.
    if !active { return; }
    let scale=ui.ctx().pixels_per_point();
    let logical_width=if active {
        theme::DOCK_SPLITTER_HOVER_LINE
    }else{
        theme::DOCK_SPLITTER_LINE
    };
    let line=pixel_paint::centered_paint_axis_rect(rect,vertical,logical_width,scale);
    line.fill(
        ui.painter(), 0.0,
        if active { theme::border_ink(theme::BorderRole::Standard) } else { theme::border_ink(theme::BorderRole::Divider) },
    );
}

/// Repaint the complete reception splitter topology in ONE parent layer.
/// Bridges and junctions use the SAME measured pane-divider coordinates.
/// Painting only the bridge's left junction left a visible gap where its
/// right endpoint met a vertical bar at fractional DPI scaling.
pub(crate) fn paint_dock_network(
    ui: &Ui,
    verticals: &[(Rect, bool)],
    bridges: &[(usize, usize, f32)],
) {
    // Paint each horizontal bridge only between the CENTER AXES of its
    // two vertical boundaries. Full-width junction squares used to extend
    // the horizontal rule past the vertical line and create protruding tips.
    for &(left, right, center_y) in bridges {
        if left >= verticals.len() || right >= verticals.len() { continue; }
        let x_start = verticals[left].0.center().x;
        let x_end = verticals[right].0.center().x;
        let area = Rect::from_min_max(
            egui::pos2(x_start, center_y - crate::ui_tokens::WORKSPACE_SPLITTER_WIDTH / 2.0),
            egui::pos2(x_end, center_y + crate::ui_tokens::WORKSPACE_SPLITTER_WIDTH / 2.0),
        );
        paint_dock_axis_line(ui, area, false, false);
    }
    // Draw the continuous vertical rules last. This seals T joints with
    // snapped pixels without extending the horizontal tips past the axis.
    for &(rect, highlighted) in verticals {
        paint_dock_axis_line(ui, rect, true, highlighted);
    }
}

/// Approved 59th-round Freight page heading: 12pt from the top of the
/// workspace. The workspace shell already supplies a 6pt outer gutter, so
/// only the difference is added. Reception must not call this helper.
pub(crate) fn management_page_title_top_gap(ui:&mut Ui) {
    ui.add_space((theme::SPACE_12-theme::PANEL_GAP).max(0.0));
}

/// One horizontal guide for a management heading, its optional tabs,
/// top-right metadata and optional KPI strip. This affects ONLY the top
/// content; primary card outer borders keep the established full width.
pub(crate) fn management_top_content<R>(
    ui:&mut Ui, add:impl FnOnce(&mut Ui)->R
)->R{
    let output=theme::management_top_content_frame()
        .show(ui,|inset|{
            inset.apply_shared_width(CardWidthPolicy::FillAvailable,inset.available_width());
            add(inset)
        });
    #[cfg(test)]
    crate::ui_test_support::record("top content",output.response.rect);
    output.inner
}

/// C-approved dual-master toolbar alignment contract:
/// right-aligned search/date/bulk controls end at the PRIMARY CARD'S inner
/// right edge. The scrollbar gutter belongs only to table headers and rows.
/// Slot height/vertical stacking may differ by screen, but not its right edge.
/// Returns egui logical-point coordinates; convert to device pixels with
/// Context::pixels_per_point() when reporting runtime measurements.
pub(crate) fn management_toolbar_trailing_rect(
    toolbar:Rect,desired_width:f32,top_offset:f32,height:f32
)->Rect {
    debug_assert!(desired_width.is_finite() && desired_width>=0.0);
    debug_assert!(top_offset.is_finite() && top_offset>=0.0);
    debug_assert!(height.is_finite() && height>=0.0);
    debug_assert!(top_offset+height<=toolbar.height()+0.01,
        "trailing controls must stay within the reserved toolbar");
    let width=desired_width.min(toolbar.width()).max(0.0);
    let rect=Rect::from_min_size(
        egui::pos2(toolbar.right()-width,toolbar.top()+top_offset),
        Vec2::new(width,height)
    );
    debug_assert!((rect.right()-toolbar.right()).abs()<0.01,
        "management toolbar trailing edge must match the card's inner right edge");
    rect
}

/// Shared source-card geometry, reserving the same chrome across stages.
pub(crate) fn reception_source_client_rect(body:Rect)->Rect{
    let gap=theme::SPACE_8;
    // The section divider already owns the leading clearance. Reserve only
    // the card-to-document, document-to-action and final action gaps.
    let height=(body.height()-3.0*gap-theme::ACTION_STANDARD_HEIGHT)
        .max(0.0).min(theme::ACTION_STANDARD_HEIGHT+2.0*gap);
    Rect::from_min_size(body.min,Vec2::new(body.width(),height))
}

/// Shared item-table to freight-group transition; call once per group,
/// not for every dispatch origin.
pub(crate) fn reception_items_to_freight_gap(ui:&mut Ui){
    ui.add_space(theme::RECEPTION_ITEMS_TO_FREIGHT_GAP);
}

/// Fixed toolbar slots own alignment, width and clipping; callers supply controls.
/// Retain existing IDs so editing focus and popup state survive the migration.
pub(crate) fn management_toolbar_slot<R>(
    ui:&mut Ui,id:impl std::hash::Hash,rect:Rect,layout:Layout,
    clip:bool,fixed_height:bool,draw:impl FnOnce(&mut Ui)->R
)->R{
    let rect=aligned_rect(rect);
    ui.scope_builder(UiBuilder::new().id_salt(id).aligned_max_rect(rect).layout(layout),|slot|{
        if clip{slot.restrict_shared_clip(rect.intersect(slot.clip_rect()));}
        slot.apply_shared_width(CardWidthPolicy::Fixed,rect.width());
        if fixed_height{set_fixed_height(slot,rect.height());}
        draw(slot)
    }).inner
}
#[derive(Clone,Copy)]
pub(crate) enum ToolbarPart{Search,Actions}
pub(crate) struct ToolbarPair{
    pub search_id:&'static str,pub actions_id:&'static str,
    pub narrow_search_id:&'static str,pub narrow_actions_id:&'static str,
    pub stack_width:f32,pub search_max:f32,pub actions_width:f32,
    pub narrow_actions_first:bool,pub actions_rtl:bool,pub clip:bool,
}
pub(crate) fn management_toolbar_pair(ui:&mut Ui,policy:ToolbarPair,
    mut draw:impl FnMut(&mut Ui,ToolbarPart)){
    let width=ui.available_width().max(1.0);
    let actions_layout=if policy.actions_rtl{Layout::right_to_left(Align::Center)}
        else{Layout::left_to_right(Align::Center)};
    if width<policy.stack_width{
        let parts=if policy.narrow_actions_first{[ToolbarPart::Actions,ToolbarPart::Search]}
            else{[ToolbarPart::Search,ToolbarPart::Actions]};
        for (index,part) in parts.into_iter().enumerate(){
            if index>0{ui.add_space(theme::SPACE_8);}
            let (rect,_)=ui.shared_allocate_exact_size(Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover());
            let (id,layout)=match part{
                ToolbarPart::Search=>(policy.narrow_search_id,Layout::left_to_right(Align::Center)),
                ToolbarPart::Actions=>(policy.narrow_actions_id,actions_layout.clone()),
            };
            management_toolbar_slot(ui,id,rect,layout,policy.clip,false,|slot|draw(slot,part));
        }
    }else{
        let (outer,_)=ui.shared_allocate_exact_size(Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover());
        let actions=management_toolbar_trailing_rect(outer,policy.actions_width,0.0,outer.height());
        let search_width=policy.search_max.min((actions.left()-outer.left()-theme::SPACE_12).max(1.0));
        let search=Rect::from_min_size(outer.min,Vec2::new(search_width,outer.height()));
        management_toolbar_slot(ui,policy.search_id,search,Layout::left_to_right(Align::Center),policy.clip,false,|slot|draw(slot,ToolbarPart::Search));
        management_toolbar_slot(ui,policy.actions_id,actions,actions_layout,policy.clip,false,|slot|draw(slot,ToolbarPart::Actions));
    }
}

/// Workspace pane allocation is independent of its card contents. Some
/// reception panes intentionally fit their existing inner-width policy.
pub(crate) fn workspace_pane<R>(ui:&mut Ui,width:f32,height:f32,fixed_width:bool,
    draw:impl FnOnce(&mut Ui)->R)->R{
    ui.shared_allocate_ui_with_layout(Vec2::new(width,height),Layout::top_down(Align::Min),|pane|{
        if fixed_width{pane.apply_shared_width(CardWidthPolicy::Fixed,width);}
        draw(pane)
    }).inner
}

/// A shared tag editor: text stays in the input until the explicit Add action.
pub(crate) struct TagBlockEdit{
    pub changed:bool,
    pub add:bool,
    pub remove:Option<String>,
}
pub(crate) fn tag_block_editor(ui:&mut Ui,tags:&[String],input:&mut String,hint:&str)->TagBlockEdit{
    let mut remove=None;
    if !tags.is_empty(){
        // Start-align fixed-height chips so the first sibling cannot be
        // repositioned when a later sibling expands the wrapping row.
        ui.scope_builder(egui::UiBuilder::new()
            .layout(Layout::left_to_right(Align::Min).with_main_wrap(true)),|chips|{
            apply_spacing_role(chips,SpacingRole::TagChips);
            for tag in tags{
                if crate::popup::removable_tag_chip(chips,tag).clicked(){
                    remove=Some(tag.clone());
                }
            }
        });
    }
    let (changed,add)=ui.horizontal(|entry|{
        apply_spacing_role(entry,SpacingRole::Toolbar);
        let width=(entry.available_width()-theme::INPUT_HEIGHT*2.5-theme::SPACE_8).max(60.0);
        let response=crate::input::flexible_text_field(entry,input,width,hint);
        let add=crate::button::action(entry,"추가",theme::ButtonRole::Standard,
            !input.trim().is_empty()).clicked();
        (response.changed(),add)
    }).inner;
    TagBlockEdit{changed,add,remove}
}

