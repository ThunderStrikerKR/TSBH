#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the scrollbar component.

use eframe::egui::{self};


use crate::ui_primitives::*;








use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.











// Shared table primitives used by reception and other composite widgets.

#[cfg(test)]
mod fixed_scroll_panel_contract_tests {
    #[test]
    fn order_detail_panes_delegate_frame_title_and_scroller_to_shared_component(){
        let source=include_str!("../order_history_detail.rs");
        let section=source.split("fn section_panel(").nth(1).unwrap()
            .split("fn overview(").next().unwrap();
        assert!(section.contains("crate::card::fixed_scroll_panel("));
        assert!(!section.contains("egui::ScrollArea::vertical()"));
        assert!(!section.contains("ui.painter().rect("));
    }
}
