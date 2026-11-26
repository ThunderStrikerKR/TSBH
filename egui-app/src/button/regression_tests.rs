#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the button component.

use eframe::egui::{self,Frame};

use crate::{theme};
use crate::ui_primitives::*;








use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.




use crate::button::{management_date_segment_corners,ManagementDateSegmentEdge};






// Shared table primitives used by reception and other composite widgets.

#[cfg(test)]
mod shared_ghost_action_contract_tests {
    #[test]
    fn category_explorer_back_uses_shared_ghost_action(){
        let source=include_str!("../category_dropdown.rs");
        let back=source.split("fn category_back_button(").nth(1).unwrap()
            .split("fn crumbs_row(").next().unwrap();
        assert!(back.contains("theme::ButtonRole::Ghost"));
        assert!(back.contains("crate::button::action_with_min_width("));
        assert!(!back.contains("egui::Button::new("));

        let explorer=source.split("if state.ancestors_open && state.path.len()>1")
            .nth(1).unwrap().split("crate::ui_primitives::soft_divider(menu);")
            .next().unwrap();
        assert!(explorer.contains("crate::card::category_ancestor_frame()"));
        assert!(!explorer.contains("egui::Frame::new()"));
    }
}

#[cfg(test)]
mod management_date_group_tests{
    use super::*;

    #[test]
    fn only_outer_date_segment_corners_are_rounded(){
        let radius=theme::CONTROL_RADIUS-1;
        let leading=management_date_segment_corners(
            ManagementDateSegmentEdge::Leading
        );
        let middle=management_date_segment_corners(
            ManagementDateSegmentEdge::Middle
        );
        let trailing=management_date_segment_corners(
            ManagementDateSegmentEdge::Trailing
        );
        assert_eq!((leading.nw,leading.ne,leading.sw,leading.se),
            (radius,0,radius,0));
        assert_eq!((middle.nw,middle.ne,middle.sw,middle.se),(0,0,0,0));
        assert_eq!((trailing.nw,trailing.ne,trailing.sw,trailing.se),
            (0,radius,0,radius));
    }

    #[test]
    fn date_segment_dimensions_fit_one_common_outer_frame(){
        let segments=theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH*2.0
            +theme::MANAGEMENT_DATE_CALENDAR_BUTTON_WIDTH+106.0;
        let outer=segments+theme::FRAME_STROKE_WIDTH*2.0;
        assert_eq!(outer-segments,2.0*theme::FRAME_STROKE_WIDTH);
        assert_eq!(theme::INPUT_HEIGHT-2.0*theme::FRAME_STROKE_WIDTH,
            theme::ACTION_STANDARD_HEIGHT);
    }
}
