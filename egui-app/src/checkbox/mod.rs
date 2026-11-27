//! Shared checkbox rendering and fixed list/table placement; callers own selection.
use crate::ui_layout::geometry::SharedUiClipExt as _;
use crate::ui_layout::geometry::SharedAllocationExt as _;
use crate::ui_layout::geometry::AlignedUiBuilderExt as _;
use eframe::egui::{self,Align,Layout,Rect,Response,Ui,Vec2};
use crate::theme;
/**
 * Shared management-table checkbox placement.
 *
 * `cell` is the complete first-column rectangle (44pt Freight, 48pt Excel,
 * or another approved width). `vertical_area` is the 42pt data-row rect,
 * or the fixed header's text area excluding its inner 2pt bottom outline.
 * The checkbox always follows Freight Assignment's centered 36pt/34pt
 * hit-box policy rather than a caller-specific `shrink2` or `Ui::put`.
 */
fn management_table_checkbox_rect(
    cell:Rect,vertical_area:Rect,icon_width:f32,icon_spacing:f32
)->Rect{
    debug_assert!(icon_width>=0.0&&icon_spacing>=0.0);
    egui::Rect::from_center_size(
        egui::pos2(cell.center().x,vertical_area.center().y),
        egui::vec2(
            icon_width+icon_spacing,
            vertical_area.height().min(theme::INPUT_HEIGHT)
        )
    )
}

/// Render either a fixed-header select-all checkbox or a body-row checkbox
/// with identical centering, clipping, stable identity and enabled semantics.
/// Keep selection business rules at the call site.
pub(crate) fn management_table_checkbox(
    ui:&mut Ui,cell:Rect,vertical_area:Rect,
    checked:&mut bool,enabled:bool,id:impl std::hash::Hash
)->Response {
    let rect=management_table_checkbox_rect(
        cell,vertical_area,ui.spacing().icon_width,ui.spacing().icon_spacing
    );
    let rect=crate::ui_layout::geometry::aligned_rect(rect);
    let clip=cell.intersect(vertical_area).intersect(ui.clip_rect());
    let mut builder=egui::UiBuilder::new()
        .id_salt(id)
        .aligned_max_rect(rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center));
    if !enabled {
        builder=builder.disabled();
    }
    ui.scope_builder(builder,|control|{
        control.restrict_shared_clip(clip);
        checkbox(control,checked,"",true)
    }).inner
}

/// Shared checkbox control. The caller owns the checked state and its
/// business transition; enabled state, widget construction and future visual
/// policy are centralized here.
pub(crate) fn checkbox(
    ui:&mut Ui,checked:&mut bool,label:&str,enabled:bool
)->Response{
    // Scope native widget visuals to this control. Keep native hit testing,
    // keyboard handling, text layout and disabled-state dimming unchanged.
    let original_style=ui.style().clone();
    if *checked&&enabled&&ui.is_enabled(){
        crate::highlight::checked_checkbox(ui);
    }
    let response=ui.add_enabled(enabled,egui::Checkbox::new(checked,label));
    ui.set_style(original_style);
    if response.changed(){ui.ctx().request_repaint();}
    #[cfg(test)]
    crate::ui_test_support::record_control_response("checkbox",&response,ui.clip_rect(),None);
    crate::floating_notice::remember_response(ui.ctx(),&response);
    response
}

/// One full-width, fixed-height checkbox-list row with stable identity.
/// The row layout is shared while the caller retains domain-specific updates.
pub(crate) fn checkbox_list_row(
    ui:&mut Ui,id:impl std::hash::Hash,
    checked:&mut bool,label:&str,enabled:bool,height:f32
)->Response{
    ui.shared_allocate_ui_with_layout(
        Vec2::new(ui.available_width(),height),
        Layout::left_to_right(Align::Center),
        |row|{
            row.push_id(id,|cell|{
                checkbox(cell,checked,label,enabled)
            }).inner
        }
    ).inner
}


#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod private_api_contract_table_checkbox_is_centered_for_freight_and_excel_column_widths {
    use super::*;

    #[test]
    fn table_checkbox_is_centered_for_freight_and_excel_column_widths(){
        // The 44pt and 48pt first columns differ in width but NOT their
        // widget-centering rule; no 8pt/4pt shrink offsets are permitted.
        for width in [44.0,48.0] {
            let cell=Rect::from_min_size(
                egui::pos2(100.0,200.0),Vec2::new(width,42.0)
            );
            let rect=management_table_checkbox_rect(cell,cell,18.0,4.0);
            assert_eq!(rect.center(),cell.center());
            assert_eq!(rect.height(),theme::INPUT_HEIGHT);
            assert_eq!(rect.width(),22.0);
            assert_eq!(rect.left()-cell.left(),cell.right()-rect.right());
            assert_eq!(rect.top()-cell.top(),cell.bottom()-rect.bottom());
            assert!(cell.contains(rect.min)&&cell.contains(rect.max));
        }
    }

    #[test]
    fn table_header_checkbox_is_centered_above_inner_two_point_rule(){
        let first_col=Rect::from_min_size(
            egui::pos2(100.0,300.0),Vec2::new(48.0,36.0)
        );
        let text_area=Rect::from_min_max(
            egui::pos2(0.0,300.0),
            egui::pos2(600.0,336.0-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT)
        );
        let rect=management_table_checkbox_rect(first_col,text_area,18.0,4.0);
        assert_eq!(rect.center().x,first_col.center().x);
        assert_eq!(rect.center().y,text_area.center().y);
        assert_eq!(rect.height(),34.0);
        assert_eq!(rect.bottom(),text_area.bottom());
        assert_eq!(rect.left()-first_col.left(),first_col.right()-rect.right());
        assert!(rect.bottom()<=first_col.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT);
    }
}
