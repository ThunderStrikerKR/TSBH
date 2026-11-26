//! Native scroll construction, rail style and common viewport height policy.
use eframe::egui::{self,Ui};
use crate::theme::*;
/// Common native scroll construction. Screen-specific IDs, bounds and shrink
/// behavior are applied by the caller; shared role policies build on this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxes { Vertical, Horizontal, Both }

pub fn scroll_area(axes:ScrollAxes)->egui::ScrollArea{
    match axes {
        ScrollAxes::Vertical=>egui::ScrollArea::vertical(),
        ScrollAxes::Horizontal=>egui::ScrollArea::horizontal(),
        ScrollAxes::Both=>egui::ScrollArea::both(),
    }
}

pub fn management_scroll_style() -> egui::style::ScrollStyle {
    let mut style=egui::style::ScrollStyle::solid();
    style.bar_width=10.0;
    style.handle_min_length=44.0;
    style.bar_inner_margin=SPACE_4;
    style.bar_outer_margin=SPACE_4;
    style.foreground_color=true;
    style
}

/// CONFLICT-003=A: table rails use the same 10pt solid style but WITHOUT
/// independent 4pt inner/outer margins. Non-table scroll areas keep the
/// original management_scroll_style until their role is reviewed.
pub fn management_table_scroll_style()->egui::style::ScrollStyle{
    let mut style=management_scroll_style();
    style.bar_inner_margin=0.0;
    style.bar_outer_margin=0.0;
    style
}

/// CONFLICT-004=A: use the remaining inner height of a SINGLE-list card,
/// not a fixed fraction of the whole application's content rectangle.
/// Measure AFTER toolbars, notices and the pinned header have been laid out.
/// `reserve_below` protects a footer, an empty-state message or other
/// controls that the screen must still render underneath its scroll area.
/// Never use this for multi-table reports or adjacent editor panels.
pub fn management_single_list_scroll_height(
    ui:&Ui,card_min_inner_height:f32,reserve_below:f32
)->f32 {
    management_single_list_scroll_height_geometry(
        ui.min_rect().top()+card_min_inner_height,
        ui.cursor().top(),ui.available_height(),reserve_below
    )
}

/// Pure calculation used by the management list geometry check. A 1pt
/// minimum avoids requesting an invalid/negative scroll area when a split
/// window is shorter than its non-scrolling toolbar and reserved footer.
pub(crate) fn management_single_list_scroll_height_geometry(
    card_bottom:f32,rows_top:f32,available_height:f32,reserve_below:f32
)->f32 {
    let remaining=(card_bottom-rows_top-reserve_below.max(0.0)).max(0.0);
    remaining.min(available_height.max(0.0)).max(1.0)
}

