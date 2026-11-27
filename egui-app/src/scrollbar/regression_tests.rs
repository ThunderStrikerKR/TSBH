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


#[cfg(test)]
mod shared_scrollbar_rendered_geometry_tests{
    use eframe::egui::{self,Rect,Vec2,Sense};
    use crate::{scrollbar,theme};
    use crate::ui_test_support::{test_context,assert_geometry_value,assert_rect_geometry,assert_rect_inside};

    #[test]
    fn native_vertical_rail_reserves_role_width_and_preserves_viewport_on_scroll(){
        for role in [scrollbar::ManagementScrollRole::Standard,scrollbar::ManagementScrollRole::Table]{
            let ctx=test_context();
            let mut baseline:Option<(Rect,Rect)>=None;
            for offset in [0.0,120.0,300.0,0.0]{
                let mut actual=None;
                let mut final_output=None;
                // Native scroll animations settle without changing production style.
                for _ in 0..8{
                    final_output=Some(ctx.run(egui::RawInput{
                        screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(800.0,600.0))),
                        time:Some(ctx.input(|i|i.time)+0.05),predicted_dt:0.05,
                        ..Default::default()
                    },|ctx|{
                        egui::CentralPanel::default().show(ctx,|ui|{
                            scrollbar::apply_management_scroll_role(ui,role);
                            let width=ui.available_width();
                            let style=ui.spacing().scroll;
                            let scroll=scrollbar::scroll_area(scrollbar::ScrollAxes::Vertical)
                                .id_salt("native-rail-geometry")
                                .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible)
                                .vertical_scroll_offset(offset).max_height(200.0).auto_shrink([false,false])
                                .show(ui,|body|{
                                    let rect=body.allocate_exact_size(Vec2::new(100.0,800.0),Sense::hover()).0;
                                    body.painter().rect_filled(rect,0.0,egui::Color32::from_rgb(73,147,211));
                                    rect
                                });
                            let next=ui.allocate_exact_size(Vec2::new(60.0,20.0),Sense::hover()).0;
                            actual=Some((scroll.inner_rect,scroll.inner,scroll.state.offset.y,next,width,style));
                        });
                    }));
                }
                let (viewport,content,actual_offset,next,width,style)=actual.expect("native scroll geometry");
                assert_geometry_value("scrollbar","width reserved",width-viewport.width(),style.allocated_width(),1.0);
                assert_geometry_value("scrollbar","viewport height",viewport.height(),200.0,1.0);
                assert_geometry_value("scrollbar","offset",actual_offset,offset,1.0);
                assert_geometry_value("scrollbar","content translation",viewport.top()-content.top(),actual_offset,1.0);
                if let Some((old,old_next))=baseline{
                    assert_rect_geometry("scroll viewport",viewport,old,0.5);
                    assert_rect_geometry("after scroll viewport",next,old_next,0.5);
                }else{baseline=Some((viewport,next));}
                // Inspect the actual native paint commands, not the requested style.
                let output=final_output.expect("native scroll paint");
                let mut rail_rects=Vec::new();
                fn collect(shape:&egui::Shape,viewport:Rect,out:&mut Vec<Rect>){
                    match shape{
                        egui::Shape::Rect(r) if r.rect.left()>=viewport.right()-1.0
                            && r.rect.width()>0.0&&r.rect.width()<=20.0
                            && r.rect.height()>=40.0=>out.push(r.rect),
                        egui::Shape::Vec(shapes)=>for s in shapes{collect(s,viewport,out);},
                        _=>{}
                    }
                }
                for shape in &output.shapes{collect(&shape.shape,viewport,&mut rail_rects);}
                let rail_bounds=Rect::from_min_max(
                    egui::pos2(viewport.right(),viewport.top()),
                    egui::pos2(viewport.right()+style.allocated_width(),viewport.bottom()));
                assert!(!rail_rects.is_empty(),"missing native scrollbar paint: {viewport:?}");
                for rect in rail_rects{assert_rect_inside("native scrollbar paint",rect,rail_bounds,2.0);}
            }
        }
    }
}
