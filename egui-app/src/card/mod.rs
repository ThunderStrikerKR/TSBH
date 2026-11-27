//! Shared card surfaces, measured layout and reusable card content profiles.
use crate::ui_layout::geometry::SharedUiClipExt as _;
use crate::ui_layout::geometry::SharedPainterClipExt as _;
use crate::ui_layout::geometry::SharedAllocationExt as _;
use crate::ui_layout::geometry::AlignedUiBuilderExt as _;
use eframe::egui::{self,Align,Color32,Frame,Layout,Rect,Stroke,Ui,UiBuilder,Vec2};
use crate::{theme,typography};
use crate::theme::{
    bordered_surface, role_surface_frame, surface_palette, ReviewTone, SurfaceRole,
    VisualState, CONTROL_RADIUS, FRAME_STROKE_WIDTH, FREIGHT_SUMMARY_PADDING_X,
    FREIGHT_SUMMARY_PADDING_Y, MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING,
    ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS, PANEL_RADIUS, RECEPTION_PANE_PADDING_Y,
    SECTION_PADDING, SECTION_RADIUS, SPACE_12, SPACE_4, SPACE_8, WORKFLOW_CARD_RADIUS,
    WORKFLOW_DETAIL_ROW_BORDER, WORKFLOW_DETAIL_ROW_PADDING_X, WORKFLOW_SELECTED_ROW_BORDER,
};
use crate::scrollbar::{scroll_area,ScrollAxes};
#[cfg(test)]
use crate::ui_test_support::record;
/// Height policy for reusable card groups. Content fitting is the default;
/// growing to a sibling or viewport is always an explicit choice.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(crate) enum CardHeightPolicy {
    #[default]
    FitContent,
    MatchTallest,
    FillAvailable,
}
impl CardHeightPolicy {
    pub(crate) fn resolve(self,natural:f32,tallest:f32,available:f32)->f32{
        match self {
            Self::FitContent=>natural,
            Self::MatchTallest=>natural.max(tallest),
            Self::FillAvailable if available.is_finite()=>natural.max(available),
            Self::FillAvailable=>natural,
        }
    }
}

/// Single owner for vertical expansion of a card or its aligned content.
/// The target is a minimum, not a maximum: long content must never clip.
/// FitContent does not reserve unused vertical space.
pub(crate) fn apply_height_policy(ui:&mut Ui,policy:CardHeightPolicy,target:f32){
    let used=ui.min_rect();
    let natural=used.height();
    let height=policy.resolve(natural,target,target);
    if height>natural{
        // egui 0.33 Placer::set_min_height measures from the next widget
        // position, not the start of the used region. After rendering,
        // passing a total height there adds another full-height tail.
        // Expand to the absolute bottom instead; this never moves or
        // reruns the controls, and repeated applications are idempotent.
        let target=crate::ui_layout::geometry::aligned_rect(Rect::from_min_max(used.min,egui::pos2(used.right(),used.top()+height)));
        ui.expand_to_include_y(target.bottom());
    }
}

use crate::ui_layout::geometry::{CardWidthPolicy,UiWidthPolicyExt};

/// Width arithmetic for the outer card, its horizontal frame insets and
/// optional scrollbar reservation. Preserve caller-approved minimum widths.
pub(crate) fn inner_width_for_insets(
    outer:f32,left:f32,right:f32,stroke:f32,reserve:f32,minimum:f32,
)->f32{
    (outer-left-right-2.0*stroke-reserve).max(minimum)
}
pub(crate) fn remaining_column_width(total:f32,occupied:f32,gap:f32)->f32{
    total-occupied-gap
}
pub(crate) fn fixed_and_remaining_columns(
    total:f32,fixed:f32,gap:f32,remaining_min:f32,
)->Option<(f32,f32)>{
    let remaining=remaining_column_width(total,fixed,gap);
    if remaining<remaining_min{return None;}
    Some((fixed,remaining))
}

/// Render each card once in an independent top-aligned, content-sized UI.
/// Finalize the shared frame only after all natural heights are known.
/// A zero-height soft layout bound prevents the scroll viewport or sibling
/// from becoming an accidental minimum; widgets may grow it as needed.
/// No previous-frame cache or second execution of interactive contents.
pub(crate) fn parallel_frames(
    ui:&mut Ui,widths:&[f32],gap:f32,frame:Frame,policy:CardHeightPolicy,
    mut contents:impl FnMut(usize,&mut Ui),
)->Vec<Rect>{
    if widths.is_empty(){return Vec::new();}
    let origin=ui.available_rect_before_wrap().min;
    let available=ui.available_height();
    let mut x=origin.x;
    let mut prepared=Vec::with_capacity(widths.len());
    for (index,&width) in widths.iter().enumerate(){
        let width=width.max(1.0);
        let rect=crate::ui_layout::geometry::aligned_rect(Rect::from_min_size(egui::pos2(x,origin.y),Vec2::new(width,0.0)));
        let mut column=ui.new_child(UiBuilder::new()
            .id_salt(("parallel-card",index))
            .aligned_max_rect(rect)
            .layout(Layout::top_down(Align::Min)));
        column.spacing_mut().item_spacing.y=0.0;
        column.apply_shared_width(CardWidthPolicy::Fixed,rect.width());
        let mut card=frame.clone().begin(&mut column);
        let inner_width=card.content_ui.available_width().max(1.0);
        card.content_ui.apply_shared_width(CardWidthPolicy::FillAvailable,inner_width);
        contents(index,&mut card.content_ui);
        let natural=card.content_ui.min_rect().height();
        prepared.push((column,card,natural));
        x+=width+gap;
    }
    let tallest=prepared.iter().map(|(_,_,height)|*height).fold(0.0_f32,f32::max);
    // The frame includes margins and stroke; the policy resolves body height.
    let mut rects=Vec::with_capacity(prepared.len());
    let mut bounds=Rect::NOTHING;
    for (mut column,mut card,natural) in prepared{
        let overhead=card.frame.total_margin().sum().y;
        let target=policy.resolve(natural,tallest,(available-overhead).max(0.0));
        apply_height_policy(&mut card.content_ui,policy,target);
        let rect=card.end(&mut column).rect;
        bounds=bounds.union(rect);
        rects.push(rect);
    }
    ui.advance_cursor_after_rect(bounds);
    rects
}

fn workflow_group_card_frame(highlighted:bool)->Frame{
    role_surface_frame(SurfaceRole::WorkflowCard,if highlighted{VisualState::Selected}else{VisualState::Normal},WORKFLOW_CARD_RADIUS)
        .inner_margin(0)
}
pub(crate) fn workflow_shipment_row_frame(fill:Color32,chosen:bool)->Frame{
    // Keep the existing stroke width and inner margin in every state.
    bordered_surface(fill,if chosen{WORKFLOW_SELECTED_ROW_BORDER}else{WORKFLOW_DETAIL_ROW_BORDER},ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS as u8)
        .inner_margin(egui::Margin::symmetric(
            WORKFLOW_DETAIL_ROW_PADDING_X as i8,SPACE_4 as i8
        ))
}
pub(crate) fn settings_help_surface_frame()->Frame{
    role_surface_frame(SurfaceRole::Input,VisualState::Normal,CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(SPACE_12 as i8,SPACE_12 as i8))
}

/// Compact ancestor-path surface in the category explorer. Its stronger
/// BORDER outline is intentional; do not replace it with the softer modal
/// nested-frame stroke.
pub(crate) fn category_ancestor_frame()->Frame{
    role_surface_frame(SurfaceRole::Input,VisualState::Normal,CONTROL_RADIUS)
        .inner_margin(SPACE_8)
}

pub(crate) fn section_frame() -> Frame {
    role_surface_frame(SurfaceRole::Section,VisualState::Normal,SECTION_RADIUS).inner_margin(SECTION_PADDING)
}

/// Full-height master table cards keep the same 12pt interior clearance on
/// all four sides. Row rendering ends above the bottom inset; the scrollbar
/// rail is independent and remains visually centered in that inset rather
/// than touching the outer border. Only the two single-table masters use it.
pub(crate) fn management_master_table_frame()->Frame{
    section_frame().inner_margin(egui::Margin{
        left:SECTION_PADDING as i8,
        right:SECTION_PADDING as i8,
        top:SECTION_PADDING as i8,
        bottom:MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING as i8,
    })
}

/// Overview card keeps the shared frame identity with proportional padding.
/// Only the freight KPI's vertical inset differs from standard section cards.
pub(crate) fn freight_summary_frame() -> Frame {
    role_surface_frame(SurfaceRole::Summary,VisualState::Normal,PANEL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            FREIGHT_SUMMARY_PADDING_X as i8, FREIGHT_SUMMARY_PADDING_Y as i8
        ))
}

pub(crate) fn review_issue_frame(tone: ReviewTone) -> Frame {
    let (background, border, _, _) = tone.palette();
    bordered_surface(background,border,CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            SPACE_8 as i8, SPACE_8 as i8
        ))
}

/// Fill the available row width rather than sizing the notice to its text.
/// The frame owns its insets; the parent owns scrolling and clipping.
pub(crate) fn review_issue_card(ui: &mut Ui, tone: ReviewTone, add_contents: impl FnOnce(&mut Ui)) {
    review_issue_frame(tone).show(ui, |content| {
        let width=content.available_width().max(0.0);
        content.apply_shared_width(CardWidthPolicy::FillAvailable,width);
        add_contents(content);
    });
}

/// Unbordered preview surface for generated or copy-ready text. Keep the
/// INPUT fill and 12pt inset shared instead of rebuilding a local Frame.
pub(crate) fn preview_surface_frame()->Frame{
    Frame::new().fill(surface_palette(SurfaceRole::Input,VisualState::Normal).fill).inner_margin(SPACE_12 as i8)
}

/// Reusable absolute-layout panel with a fixed title band and an
/// independently scrolling body. The caller supplies approved geometry and
/// stable IDs; clipping, frame paint, title placement and scroll ownership
/// remain identical across detail dashboards.
pub(crate) fn fixed_scroll_panel(
    ui:&mut Ui,
    rect:Rect,
    title:&str,
    heading_id:impl std::hash::Hash,
    scroll_id:impl std::hash::Hash,
    inset:Vec2,
    title_height:f32,
    title_to_body_gap:f32,
    body:impl FnOnce(&mut Ui),
){
    let rect=crate::ui_layout::geometry::aligned_rect(rect);
    if rect.width()<2.0||rect.height()<2.0{return;}
    ui.painter().rect(
        rect,theme::CONTROL_RADIUS as f32,theme::surface_palette(theme::SurfaceRole::Input,theme::VisualState::Normal).fill,
        Stroke::new(theme::FRAME_STROKE_WIDTH,theme::border_ink(theme::BorderRole::Divider)),
        egui::StrokeKind::Inside
    );
    let inner=rect.shrink2(inset);
    let parent_clip=ui.clip_rect();
    let title_rect=Rect::from_min_size(
        inner.min,Vec2::new(
            inner.width(),(title_height-title_to_body_gap).max(0.0)
        )
    );
    let scroll_rect=Rect::from_min_max(
        egui::pos2(inner.left(),inner.top()+title_height),inner.max
    );
    let title_rect=crate::ui_layout::geometry::aligned_rect(title_rect);
    let scroll_rect=crate::ui_layout::geometry::aligned_rect(scroll_rect);
    ui.scope_builder(UiBuilder::new()
        .id_salt(heading_id)
        .aligned_max_rect(title_rect)
        .layout(Layout::top_down(Align::Min)),|heading|{
            heading.restrict_shared_clip(title_rect.intersect(parent_clip));
            heading.label(typography::section_heading(title));
        });
    ui.scope_builder(UiBuilder::new()
        .id_salt(scroll_id)
        .aligned_max_rect(scroll_rect)
        .layout(Layout::top_down(Align::Min)),|content|{
            content.restrict_shared_clip(scroll_rect.intersect(parent_clip));
            content.apply_shared_width(CardWidthPolicy::Fixed,scroll_rect.width());
            content.spacing_mut().item_spacing.y=0.0;
            scroll_area(ScrollAxes::Vertical)
                .id_salt("fixed-scroll-panel")
                .max_height(scroll_rect.height().max(1.0))
                .auto_shrink([false,false])
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                )
                .show(content,|scroll|{
                    scroll.spacing_mut().item_spacing.y=theme::SPACE_4;
                    body(scroll);
                });
        });
}

/// Reserve a management page's approved primary-card footprint before its
/// asynchronous data source is ready. Its real content can replace this
/// placeholder without moving the top edge of the card or lower page slots.
pub(crate) fn management_pending_primary_card(ui:&mut Ui)->Rect{
    let min_height=management_master_table_card_min_inner_height(ui);
    measured_management_master_card(ui,|panel|{
        fill_management_primary_card(panel,min_height);
    }).response.rect
}

/// Shared numeric KPI card used by the two approved management masters
/// and any other page copying their heading / tabs / KPI hierarchy.
/// Keep the 228pt card, 28pt value, 13pt caption and insets in one place.
pub(crate) fn management_summary_card(ui:&mut Ui,label:&str,count:usize){
    management_summary_card_value(ui,label,&count.to_string());
}

/// The same KPI geometry with an explicit display value, including "—"
/// while data is pending. Never report a fabricated zero before a fetch.
pub(crate) fn management_summary_card_value(ui:&mut Ui,label:&str,value:&str){
    management_summary_card_with_state(ui,label,value,theme::TextState::Normal);
}

pub(crate) fn management_summary_card_with_state(ui:&mut Ui,label:&str,value:&str,state:theme::TextState){
    management_summary_card_with_role_and_state(ui,label,value,SummaryCardRole::Count,state);
}

#[derive(Clone,Copy,PartialEq,Eq)]
pub(crate) enum SummaryCardRole{Count,Settlement}
/// Count and monetary summaries share geometry, typography and frame.
/// Settlement preserves its wider card and explicit amount clipping.
pub(crate) fn management_summary_card_with_role_and_state(
    ui:&mut Ui,label:&str,value:&str,role:SummaryCardRole,state:theme::TextState
){
    let ink=theme::text_ink(theme::TextRole::Value,state);
    let card_width=match role{
        SummaryCardRole::Count=>theme::FREIGHT_SUMMARY_CARD_WIDTH,
        SummaryCardRole::Settlement=>theme::SETTLEMENT_KPI_CARD_WIDTH,
    };
    ui.shared_allocate_ui_with_layout(
        egui::vec2(card_width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |slot|{
            slot.apply_shared_width(CardWidthPolicy::Fixed,card_width);
            crate::card::freight_summary_frame().show(slot,|cell|{
                cell.spacing_mut().item_spacing.y=0.0;
                let content_width=card_width-2.0*theme::FREIGHT_SUMMARY_PADDING_X
                    -2.0*theme::FRAME_STROKE_WIDTH;
                let (number_row,number_response)=cell.shared_allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                cell.add_space(theme::FREIGHT_SUMMARY_TEXT_GAP);
                let (label_row,_)=cell.shared_allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_LABEL_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                let number=typography::freight_summary_large_number_galley_with_state(
                    cell,value,state
                );
                let caption=typography::freight_summary_small_label_galley(cell,label);
                let number_pos=egui::pos2(
                    number_row.center().x-number.size().x/2.0,
                    number_row.center().y-number.size().y/2.0);
                let label_pos=egui::pos2(
                    label_row.center().x-caption.size().x/2.0,
                    label_row.center().y-caption.size().y/2.0);
                if role==SummaryCardRole::Settlement{
                    let overflow=crate::ui_primitives::text_exceeds_rect(
                        number.size(),number_row.intersect(cell.clip_rect()));
                    cell.painter().with_shared_clip(number_row).galley(number_pos,number,ink);
                    cell.painter().with_shared_clip(label_row).galley(label_pos,caption,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
                    crate::ui_primitives::overflow_tooltip(number_response,overflow,value);
                }else{
                    cell.painter().galley(number_pos,number,ink);
                    cell.painter().galley(label_pos,caption,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
                }
            });
        }
    );
}

/// Capture the actual frame allocation without changing the layout.
pub(crate) fn measured_management_master_card<R>(ui:&mut Ui,body:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
    let output=crate::card::management_master_table_frame().show(ui,body);
    #[cfg(test)]
    record("master card",output.response.rect);
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("card",output.response.rect,ui.clip_rect(),None);
    output
}
pub(crate) fn measured_management_section_card<R>(ui:&mut Ui,body:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
    let output=crate::card::section_frame().show(ui,body);
    #[cfg(test)]
    record("section card",output.response.rect);
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("card",output.response.rect,ui.clip_rect(),None);
    output
}

/// Align the OUTER top border of a management page's principal card with
/// Freight > Shipping Group Assignment. Pages without four KPI cards leave
/// an intentional blank area. This is not table-cell padding or row geometry.
pub(crate) fn align_management_primary_card(ui: &mut Ui, page_origin_y: f32) {
    let target_top = page_origin_y
        + theme::management_primary_card_top_offset(ui.available_width());
    let remaining = target_top - ui.cursor().top();
    if remaining > 0.5 {
        ui.add_space(remaining);
    }
}

/// Expand one primary list card to the bottom of the management workspace
/// when its rows do not already require more room. Nested/multiple-card
/// pages must not use this: their own content needs ordinary scrolling.
pub(crate) fn management_primary_card_min_inner_height(ui: &Ui, vertical_inset: f32) -> f32 {
    management_card_min_height_for_insets(
        ui.available_height(),vertical_inset,vertical_inset
    )
}

/// Same OUTER card height regardless of whether its role uses a symmetric
/// inset or the single-master table's zero bottom inset.
pub(crate) fn management_card_min_height_for_insets(
    available_height:f32,top_inset:f32,bottom_inset:f32
)->f32{
    (available_height-top_inset-bottom_inset
        -2.0*theme::FRAME_STROKE_WIDTH).max(0.0)
}

/// Both master tables preserve 12pt padding on all four inner edges.
/// Compute their inner minimum from the matching frame insets without
/// changing the outer card bounds or the shared row/scroll viewport edge.
pub(crate) fn management_master_table_card_min_inner_height(ui:&Ui)->f32{
    management_card_min_height_for_insets(
        ui.available_height(),theme::SECTION_PADDING,
        theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING
    )
}
pub(crate) fn fill_management_primary_card(ui: &mut Ui, min_inner_height: f32) {
    let width=ui.available_width().max(1.0);
    ui.apply_shared_width(CardWidthPolicy::FillAvailable,width);
    apply_height_policy(ui,CardHeightPolicy::FillAvailable,min_inner_height);
}


/// Return reception panes to the Settings-inspired PANEL card tier.
/// No outer margin: the 6pt splitter hit area is the only reserved gutter.
/// Compensate the 1pt outline in inner margins on both vertical edges.
pub(crate) fn docked_pane_frame() -> Frame {
    section_frame()
        .inner_margin(egui::Margin {
            left: (SECTION_PADDING - FRAME_STROKE_WIDTH) as i8,
            right: (SECTION_PADDING - FRAME_STROKE_WIDTH) as i8,
            top: (RECEPTION_PANE_PADDING_Y - FRAME_STROKE_WIDTH) as i8,
            bottom: (RECEPTION_PANE_PADDING_Y - FRAME_STROKE_WIDTH) as i8,
        })
}


pub(crate) fn paint_shipment_detail_surface(ui:&Ui,block:Rect,fill:Color32,chosen:bool){
    let style=workflow_shipment_row_frame(fill,chosen);
    theme::paint_bordered_surface(ui.painter(),block,
        theme::ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS,style.fill,style.stroke.color);
}

// Shared controls and presentation profiles.
/// The actual content space in a fixed-height docked section. Reserve
/// exactly the existing top/bottom frame padding, title, divider and gaps.
/// Section widgets must not impose independent minimums that can spill
/// across the source/message splitter.
pub(crate) fn docked_section_body_height(section_height: f32) -> f32 {
    (section_height - theme::RECEPTION_SECTION_CHROME_HEIGHT).max(0.0)
}

/// The approved Reception card chrome is shared with the manual worksheet.
/// The same 16pt card heading, symmetric 8pt vertical inset and 8pt divider clearance
/// now render in both screens; business actions stay in their callers.
pub(crate) fn reception_section_with_header(
    ui:&mut Ui,title:&str,minimum_height:f32,
    header_right:impl FnOnce(&mut Ui),content:impl FnOnce(&mut Ui),
){
    let width=ui.available_width();
    let _card=docked_pane_frame().show(ui,|ui|{
        let height_inset=2.0*theme::RECEPTION_PANE_PADDING_Y;
        let inner_width=inner_width_for_insets(width,theme::SECTION_PADDING,
            theme::SECTION_PADDING,0.0,0.0,60.0);
        ui.apply_shared_width(CardWidthPolicy::FillAvailable,inner_width);
        // Docked panes have an explicit viewport budget; zero requests a natural card.
        let policy=if minimum_height>0.0{CardHeightPolicy::FillAvailable}
            else{CardHeightPolicy::FitContent};
        apply_height_policy(ui,policy,(minimum_height-height_inset).max(0.0));
        let body_item_spacing=theme::SPACING;
        ui.spacing_mut().item_spacing.y=0.0;
        let _header=ui.shared_allocate_ui_with_layout(
            Vec2::new(ui.available_width(),theme::SECTION_HEADER_HEIGHT),
            Layout::left_to_right(Align::Center),|header|{
                crate::ui_layout::geometry::reserve_min_height(header,theme::SECTION_HEADER_HEIGHT);
                header.label(typography::section_heading(title));
                header.with_layout(Layout::right_to_left(Align::Center),header_right);
            }
        );
        #[cfg(test)]
        crate::ui_test_support::record(reception_geometry_role(title,true),_header.response.rect);
        let (_transition,_divider)=crate::ui_primitives::section_divider_symmetric(ui,theme::RECEPTION_SECTION_DIVIDER_GAP);
        #[cfg(test)]
        crate::ui_test_support::record("source divider",_divider);
        let _body=ui.scope(|body|{
            body.spacing_mut().item_spacing.y=body_item_spacing;
            content(body);
        });
        #[cfg(test)]
        crate::ui_test_support::record("source body",_body.response.rect);
    });
    #[cfg(test)]
    crate::ui_test_support::record(reception_geometry_role(title,false),_card.response.rect);
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("card",_card.response.rect,ui.clip_rect(),None);
}

#[cfg(test)]
fn reception_geometry_role(title:&str,header:bool)->&'static str{
    match (title,header){
        ("발주 원문",false)=>"source card",("발주 원문",true)=>"source header",
        ("복사용 자동 메시지",false)=>"message card",("복사용 자동 메시지",true)=>"message header",
        ("확인할 내용",false)=>"review card",("확인할 내용",true)=>"review header",
        ("수취인별 주문 정보",false)=>"recipient card",("수취인별 주문 정보",true)=>"recipient header",
        ("품목 및 운임",false)=>"items card",("품목 및 운임",true)=>"items header",
        ("발주서 합계",false)=>"total card",("발주서 합계",true)=>"total header",
        ("처리 중인 주문",false)=>"orders card",("처리 중인 주문",true)=>"orders header",
        (_,false)=>"other reception card",(_,true)=>"other reception header",
    }
}

/// The source editor and automatic-message canvas use the SAME surface
/// tokens. Painting a reserved rect avoids Frame::show shrinking an empty
/// message body and moving the fixed copy-action row.
fn paint_source_document_surface(ui: &Ui, rect: Rect) {
    let palette=theme::surface_palette(theme::SurfaceRole::Input,theme::VisualState::Normal);
    theme::paint_bordered_surface(ui.painter(),rect,theme::CONTROL_RADIUS,palette.fill,palette.border);
}

/// One reserved document surface for draft, confirmed, hover, manual classification and message content.
/// The inside stroke occupies one point; the content starts four points beyond it.
pub(crate) fn reception_document_inner_rect(rect:Rect)->Option<Rect>{
    let inset=theme::SPACE_4+theme::FRAME_STROKE_WIDTH;
    if rect.width()<=2.0*inset || rect.height()<=2.0*inset{return None;}
    Some(rect.shrink(inset))
}

pub(crate) fn reception_document_viewport<R>(
    ui:&mut Ui,rect:Rect,canvas_id:impl std::hash::Hash,
    scroll_id:impl std::hash::Hash,content:impl FnOnce(&mut Ui)->R
)->Option<R>{
    let rect=crate::ui_layout::geometry::aligned_rect(rect);
    if rect.width()<=0.0 || rect.height()<=0.0{return None;}
    paint_source_document_surface(ui,rect);
    let inner=crate::ui_layout::geometry::aligned_rect(reception_document_inner_rect(rect)?);
    let clip=ui.clip_rect().intersect(inner);
    // All callers already reserve the document rectangle. A scope_builder
    // would report the inset child back to the parent and can change the
    // cursor used by the following footer. Keep scroll layout isolated.
    let mut canvas=ui.new_child(
        UiBuilder::new().id_salt(canvas_id).aligned_max_rect(inner)
            .layout(Layout::top_down(Align::Min))
    );
    canvas.restrict_shared_clip(clip);
    canvas.apply_shared_width(CardWidthPolicy::Fixed,inner.width());
    crate::ui_layout::geometry::set_fixed_height(&mut canvas,inner.height());
    Some(scroll_area(ScrollAxes::Vertical).id_salt(scroll_id)
        .max_height(inner.height()).auto_shrink([false,false])
        .show(&mut canvas,content).inner)
}

/// The 70pt client identification band is INFORMATION, not an editable
/// source document. Keep its exact reserved Rect and use the management
/// master's muted inner-row colors to establish a quieter visual tier.
pub(crate) fn paint_reception_client_summary_surface(ui:&Ui,rect:Rect){
    theme::paint_bordered_surface(ui.painter(),rect,theme::CONTROL_RADIUS,
        theme::RECEPTION_CLIENT_SUMMARY_BG,theme::RECEPTION_CLIENT_SUMMARY_BORDER);
}

#[cfg(test)]
mod regression_tests;

pub(crate) fn workflow_group_card<R>(
    ui:&mut Ui,selected:bool,preview:bool,body:impl FnOnce(&mut Ui)->R
)->R{
    let outer=ui.available_width();
    let frame=workflow_group_card_frame(selected||preview);
    let response=frame.show(ui,|card|{
        // The 42pt upper/lower bands and 1pt divider own all vertical
        // spacing. No additional automatic gap between the compact rows.
        crate::ui_primitives::apply_explicit_vertical_spacing(card);
        let width=crate::card::inner_width_for_insets(outer,0.0,0.0,theme::SOFT_DIVIDER_WIDTH,0.0,0.0);
        card.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::FillAvailable,width);
        body(card)
    });
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("workflow card",response.response.rect,ui.clip_rect(),None);
    let state=crate::highlight::States{selected,linked:preview,..Default::default()};
    if let Some(role)=state.resolve(){
        crate::highlight::paint_stripe(ui.painter(),
            crate::highlight::card_stripe_rect(response.response.rect),role);
    }
    response.inner
}

/// Divider inside the existing workflow card band, not an extra frame.
pub(crate) fn paint_workflow_band_divider(ui:&Ui,rect:Rect){
    ui.painter().with_shared_clip(rect.intersect(ui.clip_rect())).rect_filled(rect,0.0,theme::WORKFLOW_CARD_BORDER);
}
