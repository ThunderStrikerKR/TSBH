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
        let fallback=include_str!("../status_notice/mod.rs").split("fn reception_review_notice(")
            .nth(1).unwrap().split("\n}").next().unwrap();
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


#[test]
fn shared_parallel_cards_fit_current_content_and_shrink_without_cached_height(){
    use super::{parallel_frames,CardHeightPolicy};
    let ctx=egui::Context::default();
    for (left,right) in [(240.0,100.0),(40.0,80.0),(120.0,20.0)]{
        for policy in [CardHeightPolicy::MatchTallest,CardHeightPolicy::FitContent]{
            let mut rects=Vec::new();
            let mut calls=[0;2];
            let _=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,700.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.spacing_mut().item_spacing.y=0.0;
                    rects=parallel_frames(ui,&[180.0,220.0],12.0,Frame::NONE,
                        policy,|index,body|{
                            calls[index]+=1;
                            body.allocate_exact_size(Vec2::new(100.0,[left,right][index]),Sense::hover());
                        });
                    assert!((ui.cursor().top()-rects[0].bottom().max(rects[1].bottom())).abs()<1.0);
                });
            });
            assert_eq!(calls,[1,1]);
            assert_eq!(rects.len(),2);
            assert!((rects[0].top()-rects[1].top()).abs()<1.0);
            assert!((rects[1].left()-rects[0].right()-12.0).abs()<1.0);
            for (index,natural) in [left,right].into_iter().enumerate(){
                let expected=if policy==CardHeightPolicy::MatchTallest{left.max(right)}else{natural};
                assert!((rects[index].height()-expected).abs()<1.0,"{policy:?}: {rects:?}");
            }
        }
    }
    assert_eq!(CardHeightPolicy::default(),CardHeightPolicy::FitContent);
    assert_eq!(CardHeightPolicy::FillAvailable.resolve(40.0,80.0,160.0),160.0);
    assert_eq!(CardHeightPolicy::FillAvailable.resolve(200.0,80.0,160.0),200.0);
    assert_eq!(CardHeightPolicy::FillAvailable.resolve(40.0,80.0,f32::INFINITY),40.0);
}

#[test]
fn card_height_policy_preserves_natural_growth_and_only_explicitly_expands(){
    let ctx=egui::Context::default();
    for (policy,target,expected) in [
        (CardHeightPolicy::FitContent,240.0,40.0),
        (CardHeightPolicy::MatchTallest,120.0,120.0),
        (CardHeightPolicy::FillAvailable,180.0,180.0),
        (CardHeightPolicy::FillAvailable,20.0,40.0),
    ]{
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.spacing_mut().item_spacing.y=0.0;
                let measured=ui.scope(|body|{
                    body.allocate_exact_size(Vec2::new(100.0,40.0),Sense::hover());
                    apply_height_policy(body,policy,target);
                });
                assert!((measured.response.rect.height()-expected).abs()<1.0);
            });
        });
    }
}

#[test]
fn screen_card_height_reservations_use_the_common_policy(){
    let item=include_str!("../item_editor.rs");
    assert!(!item.contains("details.set_min_height("));
    assert!(item.contains("crate::card::CardHeightPolicy::MatchTallest,photo_side"));
    let messages=include_str!("../automatic_message_manager.rs");
    assert!(messages.contains("crate::card::management_card_min_height_for_insets("));
    let cards=include_str!("mod.rs");
    assert!(cards.contains("apply_height_policy(ui,CardHeightPolicy::FillAvailable,min_inner_height)"));
}

#[test]
fn card_width_policies_preserve_natural_and_reserved_widths(){
    use crate::ui_primitives::{UiWidthPolicyExt,CardWidthPolicy};
    use super::{inner_width_for_insets,fixed_and_remaining_columns};
    let ctx=egui::Context::default();
    for (policy,target,expected) in [
        (CardWidthPolicy::FitContent,240.0,40.0),
        (CardWidthPolicy::FillAvailable,180.0,180.0),
        (CardWidthPolicy::FillAvailable,20.0,40.0),
        (CardWidthPolicy::Fixed,160.0,160.0),
        (CardWidthPolicy::Bounded,140.0,140.0),
        (CardWidthPolicy::UpperLimit,140.0,40.0),
    ]{
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let measured=ui.scope(|body|{
                    body.apply_shared_width(policy,target);
                    body.allocate_exact_size(Vec2::new(40.0,30.0),Sense::hover());
                });
                assert!((measured.response.rect.width()-expected).abs()<1.0,
                    "{policy:?}: {:?}",measured.response.rect);
            });
        });
    }
    assert_eq!(CardWidthPolicy::default(),CardWidthPolicy::FitContent);
    assert_eq!(inner_width_for_insets(400.0,12.0,12.0,1.0,14.0,1.0),360.0);
    assert_eq!(inner_width_for_insets(10.0,12.0,12.0,1.0,0.0,1.0),1.0);
    assert_eq!(fixed_and_remaining_columns(880.0,346.0,12.0,310.0),Some((346.0,522.0)));
    assert_eq!(fixed_and_remaining_columns(667.0,346.0,12.0,310.0),None);
}



/// Real frame/widget allocation and paint-shape tests. These are deliberately
/// named shared_ so the existing targeted CI shared-component suite runs them.
#[cfg(test)]
mod shared_card_rendered_geometry_tests {
    use super::*;
    use crate::ui_test_support::{
        assert_card_content_insets,assert_geometry_value,assert_rect_geometry,card_content_insets
    };

    const TOL:f32=1.0;
    const PROBE:Color32=Color32::from_rgb(73,147,211);

    fn probe(ui:&mut egui::Ui,width:f32,height:f32)->Rect{
        let (rect,_)=ui.allocate_exact_size(Vec2::new(width,height),Sense::hover());
        ui.painter().rect_filled(rect,0.0,PROBE);
        rect
    }

    fn collect_fill(shape:&egui::Shape,fill:Color32,clip:Rect,out:&mut Vec<(Rect,Rect)>){
        match shape{
            egui::Shape::Rect(r) if r.fill==fill=>out.push((r.rect,clip)),
            egui::Shape::Vec(shapes)=>for shape in shapes{collect_fill(shape,fill,clip,out);},
            _=>{}
        }
    }

    fn assert_painted(output:&egui::FullOutput,role:&str,fill:Color32,expected:Rect){
        let mut candidates=Vec::new();
        for shape in &output.shapes{
            collect_fill(&shape.shape,fill,shape.clip_rect,&mut candidates);
        }
        // egui may represent a frame stroke at a half-point inset.
        let tolerance=TOL+theme::FRAME_STROKE_WIDTH/2.0;
        let (paint,clip)=candidates.iter().copied().find(|(r,_)|
            (r.left()-expected.left()).abs()<=tolerance &&
            (r.right()-expected.right()).abs()<=tolerance &&
            (r.top()-expected.top()).abs()<=tolerance &&
            (r.bottom()-expected.bottom()).abs()<=tolerance
        ).unwrap_or_else(||panic!("{role}: no painted frame/widget matching {expected:?}; candidates={candidates:?}"));
        assert_rect_geometry(role,paint,expected,tolerance);
        assert_rect_geometry(role,paint.intersect(clip),paint,tolerance);
    }

    #[test]
    fn natural_section_frame_shrinks_both_axes_and_preserves_four_insets(){
        let ctx=test_context();
        let pad=theme::SECTION_PADDING+theme::FRAME_STROKE_WIDTH;
        let mut previous:Option<Rect>=None;
        // Same Context and widget position: a smaller frame must not retain
        // either dimension from the preceding larger frame.
        for (viewport,w,h) in [(1000.0,260.0,160.0),(640.0,124.0,36.0),(840.0,190.0,84.0)]{
            let mut geometry=None;
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(viewport,700.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.spacing_mut().item_spacing.y=0.0;
                    let measured=measured_management_section_card(ui,|body|{
                        apply_width_policy(body,CardWidthPolicy::FitContent,400.0);
                        let content=probe(body,w,h);
                        apply_height_policy(body,CardHeightPolicy::FitContent,400.0);
                        content
                    });
                    geometry=Some((measured.response.rect,measured.inner));
                });
            });
            let (card,content)=geometry.expect("natural card geometry");
            assert_card_content_insets("natural card",card,content,[pad;4],TOL);
            assert_geometry_value("natural card","width",card.width(),w+2.0*pad,TOL);
            assert_geometry_value("natural card","height",card.height(),h+2.0*pad,TOL);
            assert_painted(&output,"natural card border",theme::PANEL,card);
            assert_painted(&output,"natural card content",PROBE,content);
            if w==124.0{
                let old=previous.expect("preceding larger card");
                assert!(card.width()<old.width()&&card.height()<old.height(),
                    "card retained previous frame: {old:?} -> {card:?}");
            }
            previous=Some(card);
        }
    }

    #[test]
    fn master_card_fill_policy_reserves_only_the_approved_work_area(){
        let ctx=test_context();
        let left=theme::SECTION_PADDING+theme::FRAME_STROKE_WIDTH;
        let bottom=theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING+theme::FRAME_STROKE_WIDTH;
        for (viewport,budget) in [(920.0,180.0),(640.0,110.0)]{
            let mut geometry=None;
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(viewport,500.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.spacing_mut().item_spacing.y=0.0;
                    let available=ui.available_width();
                    let measured=measured_management_master_card(ui,|body|{
                        fill_management_primary_card(body,budget);
                        let width=body.available_width();
                        probe(body,width,40.0)
                    });
                    geometry=Some((measured.response.rect,measured.inner,available));
                });
            });
            let (card,content,available)=geometry.expect("master card geometry");
            assert_card_content_insets("master fill",card,content,
                [left,left,left,bottom+budget-40.0],TOL);
            assert_geometry_value("master fill","width",card.width(),available,TOL);
            assert_geometry_value("master fill","height",card.height(),budget+left+bottom,TOL);
            assert_painted(&output,"master card border",theme::PANEL,card);
            assert_painted(&output,"master card content",PROBE,content);
        }
    }

    #[test]
    fn parallel_modal_cards_measure_widths_gap_and_only_intended_height_slack(){
        let ctx=test_context();
        let pad=theme::MODAL_SECTION_PADDING+theme::FRAME_STROKE_WIDTH;
        let gap=theme::MODAL_MASTER_WORKSPACE_GAP;
        let mut first_height=None;
        for (viewport,widths,heights,policy) in [
            (1000.0,[346.0,420.0],[170.0,90.0],CardHeightPolicy::MatchTallest),
            (780.0,[250.0,330.0],[50.0,70.0],CardHeightPolicy::MatchTallest),
            (780.0,[250.0,330.0],[50.0,70.0],CardHeightPolicy::FitContent),
        ]{
            let mut cards=Vec::new();
            let mut content=[Rect::NOTHING;2];
            let mut calls=[0;2];
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(viewport,700.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.spacing_mut().item_spacing.y=0.0;
                    let sections=[("상품 이미지",widths[0]),("상품 개요",widths[1])];
                    cards=crate::popup::modal_parallel_sections(ui,&sections,policy,|i,body|{
                        calls[i]+=1;
                        let width=body.available_width();
                        content[i]=probe(body,width,heights[i]);
                    });
                    let end=cards.iter().map(Rect::bottom).fold(f32::NEG_INFINITY,f32::max);
                    assert_geometry_value("parallel modal","next content top",ui.cursor().top(),end,TOL);
                });
            });
            assert_eq!(calls,[1,1],"interactive card contents executed twice");
            assert_eq!(cards.len(),2);
            let header_inset=card_content_insets(cards[0],content[0])[1];
            assert!(header_inset>pad,"modal heading/divider must precede the content");
            assert_geometry_value("parallel modal","top alignment",cards[1].top(),cards[0].top(),TOL);
            assert_geometry_value("parallel modal","card gap",cards[1].left()-cards[0].right(),gap,TOL);
            let tallest=heights[0].max(heights[1]);
            for i in 0..2{
                let slack=if policy==CardHeightPolicy::MatchTallest{tallest-heights[i]}else{0.0};
                assert_geometry_value("parallel modal","allocated width",cards[i].width(),widths[i],TOL);
                assert_card_content_insets("parallel modal",cards[i],content[i],
                    [pad,header_inset,pad,pad+slack],TOL);
                assert_painted(&output,"parallel modal border",theme::MODAL_SECTION_BG,cards[i]);
                assert_painted(&output,"parallel modal content",PROBE,content[i]);
            }
            if policy==CardHeightPolicy::MatchTallest{
                assert_geometry_value("parallel modal","bottom alignment",cards[0].bottom(),cards[1].bottom(),TOL);
                if let Some(old)=first_height{
                    assert!(cards[0].height()<old,"matched card retained obsolete height");
                }else{first_height=Some(cards[0].height());}
            }
        }
    }
}


#[test]
fn shared_card_height_is_absolute_after_multiple_rows_and_idempotent(){
    let ctx=test_context();
    for gap in [0.0,theme::SPACE_8]{
        for (policy,target,expected) in [
            (CardHeightPolicy::FitContent,400.0,350.0+gap),
            (CardHeightPolicy::MatchTallest,400.0,400.0),
            (CardHeightPolicy::FillAvailable,400.0,400.0),
            (CardHeightPolicy::FillAvailable,100.0,350.0+gap),
        ]{
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.add_space(77.0);
                    let measured=ui.scope(|body|{
                        body.spacing_mut().item_spacing.y=gap;
                        let first=body.allocate_exact_size(Vec2::new(100.0,150.0),Sense::hover()).0;
                        let last=body.allocate_exact_size(Vec2::new(100.0,200.0),Sense::hover()).0;
                        assert!((last.bottom()-first.top()-(350.0+gap)).abs()<1.0);
                        for _ in 0..3{
                            apply_height_policy(body,policy,target);
                            assert!((body.min_rect().bottom()-first.top()-expected).abs()<1.0,
                                "absolute height {policy:?}, target {target}: {:?}",body.min_rect());
                        }
                    });
                    assert!((measured.response.rect.height()-expected).abs()<1.0);
                });
            });
        }
    }
}
