//! Shared card surfaces, measured layout and reusable card content profiles.
use eframe::egui::{self,Align,Color32,Frame,Layout,Rect,Response,Sense,Stroke,Ui,UiBuilder,Vec2};
use crate::{theme,typography};
use crate::theme::*;
use crate::scrollbar::{scroll_area,ScrollAxes};
#[cfg(test)]
use crate::components::general_screen_geometry_tests;
pub fn workflow_group_card_frame(highlighted:bool)->Frame{
    bordered_surface(if highlighted{WORKFLOW_SELECTED_BG}else{WORKFLOW_CARD_BG},WORKFLOW_CARD_BORDER,WORKFLOW_CARD_RADIUS)
        .inner_margin(0)
}
pub fn workflow_shipment_row_frame(fill:Color32,chosen:bool)->Frame{
    // Keep the existing stroke width and inner margin in every state.
    bordered_surface(fill,if chosen{WORKFLOW_SELECTED_ROW_BORDER}else{WORKFLOW_DETAIL_ROW_BORDER},ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS as u8)
        .inner_margin(egui::Margin::symmetric(
            WORKFLOW_DETAIL_ROW_PADDING_X as i8,SPACE_4 as i8
        ))
}
pub fn settings_help_surface_frame()->Frame{
    bordered_surface(INPUT,BORDER,CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(SPACE_12 as i8,SPACE_12 as i8))
}

/// Compact ancestor-path surface in the category explorer. Its stronger
/// BORDER outline is intentional; do not replace it with the softer modal
/// nested-frame stroke.
pub fn category_ancestor_frame()->Frame{
    bordered_surface(INPUT,BORDER,CONTROL_RADIUS)
        .inner_margin(SPACE_8)
}

pub fn section_frame() -> Frame {
    bordered_surface(PANEL,CARD_BORDER,SECTION_RADIUS).inner_margin(SECTION_PADDING)
}

/// Full-height master table cards keep the same 12pt interior clearance on
/// all four sides. Row rendering ends above the bottom inset; the scrollbar
/// rail is independent and remains visually centered in that inset rather
/// than touching the outer border. Only the two single-table masters use it.
pub fn management_master_table_frame()->Frame{
    section_frame().inner_margin(egui::Margin{
        left:SECTION_PADDING as i8,
        right:SECTION_PADDING as i8,
        top:SECTION_PADDING as i8,
        bottom:MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING as i8,
    })
}

/// Overview card keeps the shared frame identity with proportional padding.
/// Only the freight KPI's vertical inset differs from standard section cards.
pub fn freight_summary_frame() -> Frame {
    bordered_surface(PANEL,BORDER,PANEL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            FREIGHT_SUMMARY_PADDING_X as i8, FREIGHT_SUMMARY_PADDING_Y as i8
        ))
}

pub fn review_issue_frame(tone: ReviewTone) -> Frame {
    let (background, border, _, _) = tone.palette();
    bordered_surface(background,border,CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            SPACE_8 as i8, SPACE_8 as i8
        ))
}

/// Fill the available row width rather than sizing the notice to its text.
/// The frame owns its insets; the parent owns scrolling and clipping.
pub fn review_issue_card(ui: &mut Ui, tone: ReviewTone, add_contents: impl FnOnce(&mut Ui)) {
    review_issue_frame(tone).show(ui, |content| {
        content.set_min_width(content.available_width().max(0.0));
        add_contents(content);
    });
}

/// Unbordered preview surface for generated or copy-ready text. Keep the
/// INPUT fill and 12pt inset shared instead of rebuilding a local Frame.
pub fn preview_surface_frame()->Frame{
    Frame::new().fill(INPUT).inner_margin(SPACE_12 as i8)
}

/// Border and rounding belong to the reusable source-document surface.
/// The editor's own frame is disabled so its square corners cannot cover
/// the approved rounded rect below the dealer card.
pub fn source_document_frame() -> Frame {
    bordered_surface(INPUT,BORDER,CONTROL_RADIUS)
        .inner_margin(SPACE_4)
}

/// Reusable absolute-layout panel with a fixed title band and an
/// independently scrolling body. The caller supplies approved geometry and
/// stable IDs; clipping, frame paint, title placement and scroll ownership
/// remain identical across detail dashboards.
pub fn fixed_scroll_panel(
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
    if rect.width()<2.0||rect.height()<2.0{return;}
    ui.painter().rect(
        rect,theme::CONTROL_RADIUS as f32,theme::INPUT,
        Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER),
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
    ui.scope_builder(UiBuilder::new()
        .id_salt(heading_id)
        .max_rect(title_rect)
        .layout(Layout::top_down(Align::Min)),|heading|{
            heading.set_clip_rect(title_rect.intersect(parent_clip));
            heading.label(typography::section_heading(title));
        });
    ui.scope_builder(UiBuilder::new()
        .id_salt(scroll_id)
        .max_rect(scroll_rect)
        .layout(Layout::top_down(Align::Min)),|content|{
            content.set_clip_rect(scroll_rect.intersect(parent_clip));
            content.set_width(scroll_rect.width());
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
pub fn management_pending_primary_card(ui:&mut Ui)->Rect{
    let min_height=management_master_table_card_min_inner_height(ui);
    measured_management_master_card(ui,|panel|{
        fill_management_primary_card(panel,min_height);
    }).response.rect
}

/// Shared numeric KPI card used by the two approved management masters
/// and any other page copying their heading / tabs / KPI hierarchy.
/// Keep the 228pt card, 28pt value, 13pt caption and insets in one place.
pub fn management_summary_card(ui:&mut Ui,label:&str,count:usize){
    management_summary_card_value(ui,label,&count.to_string());
}

/// The same KPI geometry with an explicit display value, including "—"
/// while data is pending. Never report a fabricated zero before a fetch.
pub fn management_summary_card_value(ui:&mut Ui,label:&str,value:&str){
    let card_width=theme::FREIGHT_SUMMARY_CARD_WIDTH;
    ui.allocate_ui_with_layout(
        egui::vec2(card_width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |slot|{
            slot.set_width(card_width);
            crate::card::freight_summary_frame().show(slot,|cell|{
                cell.spacing_mut().item_spacing.y=0.0;
                let content_width=card_width-2.0*theme::FREIGHT_SUMMARY_PADDING_X
                    -2.0*theme::FRAME_STROKE_WIDTH;
                let (number_row,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                cell.add_space(theme::FREIGHT_SUMMARY_TEXT_GAP);
                let (label_row,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_LABEL_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                let number=typography::freight_summary_large_number_galley(
                    cell,value
                );
                let caption=typography::freight_summary_small_label_galley(cell,label);
                cell.painter().galley(
                    egui::pos2(
                        number_row.center().x-number.size().x/2.0,
                        number_row.center().y-number.size().y/2.0
                    ),number,theme::TEXT
                );
                cell.painter().galley(
                    egui::pos2(
                        label_row.center().x-caption.size().x/2.0,
                        label_row.center().y-caption.size().y/2.0
                    ),caption,theme::SECONDARY
                );
            });
        }
    );
}

/// Capture the actual frame allocation without changing the layout.
pub fn measured_management_master_card<R>(ui:&mut Ui,body:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
    let output=crate::card::management_master_table_frame().show(ui,body);
    #[cfg(test)]
    general_screen_geometry_tests::record("master card",output.response.rect);
    output
}
pub fn measured_management_section_card<R>(ui:&mut Ui,body:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
    let output=crate::card::section_frame().show(ui,body);
    #[cfg(test)]
    general_screen_geometry_tests::record("section card",output.response.rect);
    output
}

/// Align the OUTER top border of a management page's principal card with
/// Freight > Shipping Group Assignment. Pages without four KPI cards leave
/// an intentional blank area. This is not table-cell padding or row geometry.
pub fn align_management_primary_card(ui: &mut Ui, page_origin_y: f32) {
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
pub fn management_primary_card_min_inner_height(ui: &Ui, vertical_inset: f32) -> f32 {
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
pub fn management_master_table_card_min_inner_height(ui:&Ui)->f32{
    management_card_min_height_for_insets(
        ui.available_height(),theme::SECTION_PADDING,
        theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING
    )
}
pub fn fill_management_primary_card(ui: &mut Ui, min_inner_height: f32) {
    ui.set_min_width(ui.available_width().max(1.0));
    ui.set_min_height(min_inner_height);
}

