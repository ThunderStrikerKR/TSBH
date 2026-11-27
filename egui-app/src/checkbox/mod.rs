//! Shared checkbox rendering and fixed list/table placement; callers own selection.
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
pub(crate) fn management_table_checkbox_rect(
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
    let clip=cell.intersect(vertical_area).intersect(ui.clip_rect());
    let mut builder=egui::UiBuilder::new()
        .id_salt(id)
        .max_rect(rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center));
    if !enabled {
        builder=builder.disabled();
    }
    ui.scope_builder(builder,|control|{
        control.set_clip_rect(clip);
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
        let widgets=&mut ui.visuals_mut().widgets;
        for visual in [&mut widgets.inactive,&mut widgets.hovered,&mut widgets.active,&mut widgets.open]{
            visual.bg_fill=theme::CHECKBOX_CHECKED_BG;
            visual.weak_bg_fill=theme::CHECKBOX_CHECKED_BG;
            visual.bg_stroke=egui::Stroke::new(theme::FRAME_STROKE_WIDTH,theme::SELECTED_ORDER);
            visual.fg_stroke=egui::Stroke::new(theme::CHECKBOX_CHECKED_STROKE_WIDTH,theme::CHECKBOX_CHECKED_MARK);
        }
    }
    let response=ui.add_enabled(enabled,egui::Checkbox::new(checked,label));
    ui.set_style(original_style);
    if response.changed(){ui.ctx().request_repaint();}
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("checkbox",response.rect,ui.clip_rect(),None);
    crate::floating_notice::remember_response(ui.ctx(),&response);
    response
}

/// One full-width, fixed-height checkbox-list row with stable identity.
/// The row layout is shared while the caller retains domain-specific updates.
pub(crate) fn checkbox_list_row(
    ui:&mut Ui,id:impl std::hash::Hash,
    checked:&mut bool,label:&str,enabled:bool,height:f32
)->Response{
    ui.allocate_ui_with_layout(
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
