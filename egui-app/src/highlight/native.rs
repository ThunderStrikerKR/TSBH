//! Native egui visual-state adapter. No theme dependency: the theme passes
//! its semantic tokens at startup, so the UI dependency graph stays acyclic.
use eframe::egui::{Color32,Stroke,Visuals};

pub(crate) struct NativePalette{
    pub hover_fill:Color32,pub hover_outline:Stroke,pub hover_ink:Color32,
    pub active_fill:Color32,pub focus_outline:Stroke,
    pub selection_fill:Color32,pub selection_outline:Stroke,
}
pub(crate) fn native_widgets(visuals:&mut Visuals,p:NativePalette){
    visuals.widgets.hovered.bg_fill=p.hover_fill;
    visuals.widgets.hovered.weak_bg_fill=p.hover_fill;
    visuals.widgets.hovered.bg_stroke=p.hover_outline;
    visuals.widgets.hovered.fg_stroke=Stroke::new(1.5,p.hover_ink);
    visuals.widgets.hovered.expansion=0.0;
    visuals.widgets.active.bg_fill=p.active_fill;
    visuals.widgets.active.bg_stroke=p.focus_outline;
    visuals.selection.bg_fill=p.selection_fill;
    visuals.selection.stroke=p.selection_outline;
}
