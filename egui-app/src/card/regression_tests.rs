#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the card component.

use eframe::egui::{self,Color32,Frame,Rect,Sense,Vec2};

use crate::{theme};
use crate::ui_primitives::*;
use crate::status_notice::{body_error_overlay};






use crate::ui_test_support::{test_context,assert_layout_rects_invariant};
use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.











// Shared table primitives used by reception and other composite widgets.
use crate::table::{management_table_container_style};
use crate::table::{management_table_scroll_track_rect};

use crate::table::{virtual_fixed_rows,management_sticky_table_header};

#[cfg(test)]
mod shared_review_card_contract_tests {
    #[test]
    fn reception_review_cards_use_shared_surface_frame(){
        let source=include_str!("../review_panel.rs");
        assert_eq!(source.matches("crate::card::review_issue_card(").count(),2);
        assert!(source.contains("crate::status_notice::body_error_overlay("));
        assert!(!source.contains("egui::Frame::new()"));
        let fallback=include_str!("../status_notice/mod.rs").split("pub fn reception_review_status(")
            .nth(1).unwrap().split("\n///").next().unwrap();
        assert!(fallback.contains("crate::card::review_issue_card("));
    }
}

#[cfg(test)]
mod shared_card_scroll_rendering_tests {
    use super::*;

    #[derive(Debug)]
    struct TableFrame{
        card:Rect,toolbar:Rect,header:Rect,header_data:Rect,rail:Rect,
        viewport:Rect,footer:Rect,next:Rect,
        offset:f32,rows:Vec<(usize,Rect)>,
    }

    fn collect_rules(shape:&egui::Shape,out:&mut Vec<(Color32,Rect)>){
        match shape{
            egui::Shape::Rect(rect) if
                (rect.fill==theme::DIVIDER||rect.fill==theme::TABLE_HEADER_OUTLINE)
                &&rect.rect.height()<=2.0&&rect.rect.width()>100.0=>{
                out.push((rect.fill,rect.rect));
            },
            egui::Shape::Vec(shapes)=>for shape in shapes{collect_rules(shape,out);},
            _=>{}
        }
    }

    fn draw_table(ctx:&egui::Context,width:f32,count:usize,offset:f32)
        ->(TableFrame,Vec<(Color32,Rect)>){
        let mut measured=None;
        let mut rules=Vec::new();
        // Preserve native scrollbar interpolation. Settle the geometry
        // without changing the renderer's normal animation policy.
        for _ in 0..12{
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(
                    egui::Pos2::ZERO,Vec2::new(1200.0,900.0))),
                time:Some(ctx.input(|i|i.time)+0.05),predicted_dt:0.05,
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|root|{
                    root.set_width(width);
                    let card=measured_management_master_card(root,|body|{
                        management_table_container_style(body);
                        let w=body.available_width();
                        let toolbar=body.allocate_exact_size(
                            Vec2::new(w,theme::INPUT_HEIGHT),Sense::hover()).0;
                        let layout=management_sticky_table_header(body,0.0);
                        let rail=management_table_scroll_track_rect(body,240.0);
                        let scrolled=virtual_fixed_rows(
                            egui::ScrollArea::vertical()
                                .id_salt("shared-card-scroll-regression")
                                .scroll_bar_rect(rail)
                                .vertical_scroll_offset(offset)
                                .max_height(240.0).auto_shrink([false,false]),
                            body,layout.table_width,42.0,count,
                            |visible,indices|{
                                let mut rows=Vec::new();
                                for index in indices{
                                    let row=visible.allocate_exact_size(
                                        Vec2::new(layout.table_width,41.0),Sense::hover()).0;
                                    let divider=theme::soft_rule(visible);
                                    assert!((divider.top()-row.bottom()).abs()<=0.5);
                                    assert!((divider.height()-1.0).abs()<=0.5);
                                    rows.push((index,row.union(divider)));
                                }
                                rows
                            }
                        );
                        crate::table::master_header_begin(body,layout.header_rect,
                            layout.header_content_rect);
                        crate::table::header_finish(body,layout.header_rect);
                        body.add_space(12.0);
                        let footer=body.allocate_exact_size(
                            Vec2::new(w,theme::INPUT_HEIGHT),Sense::hover()).0;
                        (toolbar,layout,rail,scrolled.inner_rect,
                            scrolled.state.offset.y,scrolled.inner,footer)
                    });
                    let (toolbar,layout,rail,viewport,offset,rows,footer)=card.inner;
                    let next=root.allocate_exact_size(Vec2::new(80.0,36.0),Sense::hover()).0;
                    measured=Some(TableFrame{
                        card:card.response.rect,toolbar,header:layout.header_rect,
                        header_data:layout.header_content_rect,rail,viewport,
                        footer,next,offset,rows
                    });
                });
            });
            rules.clear();
            for shape in &output.shapes{collect_rules(&shape.shape,&mut rules);}
        }
        (measured.expect("rendered table frame"),rules)
    }

    fn check_fixed_slots(before:&TableFrame,after:&TableFrame){
        let slots=|f:&TableFrame|[
            ("card",f.card),("toolbar",f.toolbar),("header",f.header),
            ("header data",f.header_data),("rail",f.rail),
            ("footer",f.footer),("following control",f.next),
        ];
        assert_layout_rects_invariant(&slots(before),&slots(after));
        for (edge,a,b) in [("left",before.viewport.left(),after.viewport.left()),
            ("top",before.viewport.top(),after.viewport.top()),
            ("bottom",before.viewport.bottom(),after.viewport.bottom())]{
            assert!((a-b).abs()<=0.5,"viewport {edge}: {a} -> {b}");
        }
    }

    #[test]
    fn shared_master_card_keeps_painted_rules_and_footer_when_rows_overflow(){
        for width in [440.0,880.0]{
            let ctx=test_context();
            let (empty,_)=draw_table(&ctx,width,0,0.0);
            let (single,_)=draw_table(&ctx,width,1,0.0);
            let (many,rules)=draw_table(&ctx,width,64,0.0);
            assert!(empty.rows.is_empty());
            assert_eq!(single.rows.len(),1);
            assert!(many.rows.len()>1&&many.rows.len()<64);
            check_fixed_slots(&empty,&single);
            check_fixed_slots(&empty,&many);
            let edge=12.0+theme::FRAME_STROKE_WIDTH;
            for (actual,expected,label) in [
                (many.toolbar.left()-many.card.left(),edge,"card left"),
                (many.card.right()-many.toolbar.right(),edge,"card right"),
                (many.toolbar.top()-many.card.top(),edge,"card top"),
                (many.card.bottom()-many.footer.bottom(),edge,"card bottom"),
                (many.rail.top()-many.header.bottom(),0.0,"rail starts at header"),
                (many.rail.bottom()-many.viewport.bottom(),0.0,"rail/body bottom"),
                (many.header.height(),36.0,"header height"),
                (many.header.right()-many.header_data.right(),10.0,"data gutter"),
                (many.header_data.right()-many.viewport.right(),0.0,"header/rows edge"),
            ]{
                assert!((actual-expected).abs()<=0.5,
                    "{label}: {actual} != {expected}");
            }
            assert!(many.footer.top()>many.rail.bottom(),"rail overlaps footer");
            let transition=rules.iter().filter(|(color,r)|*color==theme::DIVIDER
                &&r.top()>=many.toolbar.bottom()&&r.bottom()<=many.header.top())
                .map(|(_,r)|*r).collect::<Vec<_>>();
            assert_eq!(transition.len(),1,"one real toolbar divider");
            assert!((transition[0].top()-many.toolbar.bottom()-12.0).abs()<=0.5);
            assert!((many.header.top()-transition[0].bottom()-12.0).abs()<=0.5);
            let outlines=rules.iter().filter(|(color,r)|*color==theme::TABLE_HEADER_OUTLINE
                &&many.header.contains(r.center())).map(|(_,r)|*r).collect::<Vec<_>>();
            assert_eq!(outlines.len(),1,"one painted header outline");
            assert!((outlines[0].height()-2.0).abs()<=0.5);
            assert!((outlines[0].bottom()-many.header.bottom()).abs()<=0.5);
        }
    }

    #[test]
    fn shared_virtual_table_reaches_last_row_without_moving_card_header_or_rail(){
        for width in [440.0,880.0]{
            let ctx=test_context();
            let (top,_)=draw_table(&ctx,width,64,0.0);
            let (middle,_)=draw_table(&ctx,width,64,42.0*25.0);
            let (bottom,_)=draw_table(&ctx,width,64,42.0*64.0);
            assert_eq!(top.rows.first().unwrap().0,0);
            assert!(middle.offset>top.offset&&middle.rows.first().unwrap().0>0);
            assert!(bottom.offset>middle.offset);
            assert_eq!(bottom.rows.last().unwrap().0,63,"last data row is reachable");
            let last=bottom.rows.last().unwrap().1;
            assert!(last.top()<bottom.viewport.bottom()&&last.bottom()<=bottom.viewport.bottom()+1.0,
                "last row is clipped beyond the actual viewport");
            for frame in [&middle,&bottom]{
                check_fixed_slots(&top,frame);
                assert!((frame.viewport.right()-top.viewport.right()).abs()<=0.5);
                assert!(frame.rows.len()<64,"offscreen rows must remain virtual");
                for pair in frame.rows.windows(2){
                    assert_eq!(pair[1].0,pair[0].0+1);
                    assert!((pair[1].1.top()-pair[0].1.bottom()).abs()<=0.5);
                }
            }
            let (returned,_)=draw_table(&ctx,width,64,0.0);
            check_fixed_slots(&top,&returned);
            assert_eq!(returned.rows.first().unwrap().0,0);
            assert!(returned.offset.abs()<=0.5);
        }
    }
}
