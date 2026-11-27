#![cfg(test)]
//! Regression tests owned by the button component.


use crate::{theme};








// Composite widgets import primitive renderers from their owning modules.




use crate::button::{management_date_segment_corners,ManagementDateSegmentEdge};






// Shared table primitives used by reception and other composite widgets.

#[cfg(test)]
mod shared_ghost_action_contract_tests {
    #[test]
    fn category_explorer_back_uses_shared_icon_action_in_header(){
        let source=include_str!("../category_dropdown.rs");
        let back=source.split("fn category_back_button(").nth(1).unwrap()
            .split("fn motion_ms(").next().unwrap();
        assert!(back.contains("crate::button::icon_action_button("));
        assert!(!back.contains("egui::Button::new("));

        let nav=source.split("fn crumbs_row(").nth(1).unwrap()
            .split("impl ExplorerState").next().unwrap();
        assert!(nav.contains("category_back_button(&mut back,enabled&&!path.is_empty())"));
        assert!(nav.contains("crate::card::category_ancestor_frame()"));
        assert!(!nav.contains("egui::Frame::new()"));
        let tree=source.split("fn explorer_body(").nth(1).unwrap()
            .split("type PopupStates").next().unwrap();
        assert!(!tree.contains("category_back_button("));
        assert!(tree.contains("menu.vertical(|tree_ui|"));
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


#[cfg(test)]
mod shared_button_rendered_geometry_tests{
    use crate::{theme,theme::ButtonRole};
    use eframe::egui::{self,Rect,Vec2,Sense};
    use crate::ui_test_support::{test_context,assert_geometry_value,assert_rect_geometry,assert_painted_caption};
    #[test]
    fn action_roles_keep_caption_and_following_slot_stable_across_states(){
        for (role,height) in [(ButtonRole::Standard,theme::ACTION_STANDARD_HEIGHT),
            (ButtonRole::Primary,theme::ACTION_PRIMARY_HEIGHT),
            (ButtonRole::Danger,theme::ACTION_STANDARD_HEIGHT)]{
            let ctx=test_context();
            let mut baseline:Option<(Rect,Rect)>=None;
            for enabled in [true,true,false,true]{
                let mut actual=None;
                let events=baseline.map_or_else(Vec::new,|(r,_)|vec![egui::Event::PointerMoved(r.center())]);
                let output=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(800.0,500.0))),
                    events,..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let response=crate::button::action_with_min_width(ui,"적용",role,enabled,120.0);
                        let next=ui.allocate_exact_size(Vec2::new(80.0,20.0),Sense::hover()).0;
                        assert_eq!(response.enabled(),enabled);
                        actual=Some((response.rect,next));
                    });
                });
                let (rect,next)=actual.expect("button response");
                assert_geometry_value("button","width",rect.width(),120.0,1.0);
                assert_geometry_value("button","height",rect.height(),height,1.0);
                let ink=assert_painted_caption(&output,"적용",rect);
                assert_geometry_value("button","caption center x",ink.center().x,rect.center().x,1.5);
                assert_geometry_value("button","caption center y",ink.center().y,rect.center().y,2.0);
                if let Some((old,following))=baseline{
                    assert_rect_geometry("button state",rect,old,0.5);
                    assert_rect_geometry("following widget",next,following,0.5);
                }else{baseline=Some((rect,next));}
            }
        }
    }
}
