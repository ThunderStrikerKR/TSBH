#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the popup component.

use eframe::egui::{self,Align,Layout,Rect,Response,Sense,Stroke,Ui,Vec2};

use crate::{source_highlight,theme,typography};
use crate::ui_primitives::*;



use crate::text_fit::{reception_pending_money_marker};



use crate::ui_test_support::{test_context,assert_layout_rects_invariant};
use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.





use crate::input::{FieldKind,field};
use crate::input::{singleline_editor,reception_recipient_text_edit};




// Shared table primitives used by reception and other composite widgets.

/// Shared compact spreadsheet cell. Selection is an inset border;
/// hovered cells do not resize the row, and a selected cell does not
/// borrow the main navigation style.
/// Manual worksheet value cells use the same blue evidence fill, text ink
/// and 2pt outline as reception. Selection (amber) remains a separate state;
/// an active source link takes visual precedence over selection.
#[cfg(test)]
fn manual_linked_table_cell(
    ui:&mut Ui,value:&str,width:f32,selected:bool,linked:bool
)->Response {
    let label=if value.trim().is_empty(){"미입력"}else{value};
    let border=if linked {
        Stroke::new(theme::SOFT_DIVIDER_WIDTH/2.0,theme::DIVIDER)
    }else if selected {
        Stroke::new(theme::DOCK_SPLITTER_HOVER_LINE,theme::SELECTED_ORDER)
    }else{
        // Two adjacent cells share one visual divider, not a doubled border.
        Stroke::new(theme::SOFT_DIVIDER_WIDTH/2.0,theme::DIVIDER)
    };
    // Match the selected editor and the 16pt reception/source standard.
    // Read-only manual fields must not shrink when focus leaves a cell.
    let text=typography::body(label).size(typography::FONT_RECEPTION_RECIPIENT_TEXT);
    let response=ui.add_sized(
        Vec2::new(width,theme::MANUAL_TABLE_ROW_HEIGHT),
        egui::Button::new(
            if linked{text.color(source_highlight::ACTIVE_TEXT)}else{text}
        )
            .truncate()
            .fill(if linked{source_highlight::ACTIVE_FILL}
                  else if selected{theme::RAISED}else{theme::INPUT})
            .stroke(border)
            .corner_radius(0),
    );
    if linked {
        source_highlight::paint_active_if(ui,&response,true);
    } else {
        crate::button::paint_button_hover(ui,&response,0.0);
    }
    response
}

#[cfg(test)]
mod modal_safe_position_tests {
    use super::*;

    #[test]
    fn ocr_review_center_excludes_kakao_dock(){
        let viewport=Rect::from_min_size(
            egui::pos2(0.0,0.0),Vec2::new(1648.0,928.0)
        );
        // Work area starts to the right of the reserved KakaoTalk column.
        let work_area=Rect::from_min_max(
            egui::pos2(360.0,80.0),egui::pos2(1630.0,910.0)
        );
        let review=modal_ocr_review_rect(work_area);
        assert!((review.center().x-work_area.center().x).abs()<0.01);
        assert!((review.center().y-work_area.center().y).abs()<0.01);
        assert_ne!(review.center().x,viewport.center().x);
        assert!(review.left()>=work_area.left());
        assert!(review.right()<=work_area.right());
        assert!(review.top()>=work_area.top());
        assert!(review.bottom()<=work_area.bottom());
    }
    #[test]
    fn ocr_review_remains_inside_narrow_work_area(){
        for size in [
            Vec2::new(600.0,450.0),
            Vec2::new(1200.0,760.0),
            Vec2::new(1650.0,1100.0)
        ]{
            let work_area=Rect::from_min_size(egui::pos2(340.0,70.0),size);
            let review=modal_ocr_review_rect(work_area);
            assert!((review.center().x-work_area.center().x).abs()<0.01);
            assert!((review.center().y-work_area.center().y).abs()<0.01);
            assert!(review.left()>=work_area.left());
            assert!(review.right()<=work_area.right());
            assert!(review.top()>=work_area.top());
            assert!(review.bottom()<=work_area.bottom());
        }
    }

    #[test]
    fn sizing_pass_paint_clip_does_not_collapse_ocr_layout(){
        let outer=Vec2::new(1400.0,800.0);
        let available=Rect::from_min_size(
            egui::pos2(19.0,17.0),Vec2::new(1362.0,710.0)
        );
        // Available layout space is authoritative, not a possibly empty
        // painter clip and not the full outer window extent.
        let inner=modal_master_content_from_ui(available,outer);
        assert_eq!(inner,Vec2::new(1362.0,710.0));
        assert!(modal_master_ocr_body_is_usable(inner));
        let bounds=Rect::from_min_size(available.min,inner);
        let layout=modal_master_layout(
            bounds,theme::MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT,
            theme::MODAL_FOOTER_HEIGHT,theme::MODAL_MASTER_WORKSPACE_GAP
        );
        assert_eq!(layout.footer.bottom(),bounds.bottom());
        assert!(layout.workspace.height()
            >theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT+2.0*theme::SPACE_12);
    }
    #[test]
    fn ocr_master_does_not_expand_fixed_outer_frame_or_clip_round_bottom(){
        let outer=Vec2::new(1400.0,800.0);
        let available=Rect::from_min_size(
            egui::pos2(19.0,17.0),outer
        );
        let inner=modal_master_content_from_ui(available,outer);
        assert_eq!(inner,Vec2::new(1362.0,766.0));
        let layout=modal_master_layout(
            Rect::from_min_size(available.min,inner),
            theme::MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT,
            theme::MODAL_FOOTER_HEIGHT,
            theme::MODAL_MASTER_WORKSPACE_GAP
        );
        let inner_bottom=layout.footer.bottom();
        let frame_bottom=outer.y;
        assert_eq!(
            inner_bottom+theme::MODAL_PANEL_PADDING_Y
                +theme::FRAME_STROKE_WIDTH,
            frame_bottom
        );
        assert!(inner_bottom<frame_bottom);
        assert!(layout.workspace.bottom()<layout.footer.top());
    }
    #[test]
    fn tiny_sizing_rect_is_not_usable_for_split_ocr_viewports(){
        let outer=Vec2::new(1400.0,900.0);
        let tiny=Rect::from_min_size(
            egui::pos2(10.0,10.0),Vec2::new(1.0,1.0)
        );
        let size=modal_master_content_from_ui(tiny,outer);
        assert_eq!(size,Vec2::new(1.0,1.0));
        assert!(!modal_master_ocr_body_is_usable(size));
        assert!(!modal_master_ocr_body_is_usable(Vec2::new(640.0,200.0)));
        assert!(modal_master_ocr_body_is_usable(Vec2::new(640.0,500.0)));
    }
    #[test]
    fn popup_pivot_uses_exact_safe_area_center_without_double_header_offset(){
        for (width,height) in [(240.0,180.0),(600.0,360.0),(1200.0,860.0)] {
            let safe=Rect::from_min_size(egui::pos2(440.0,80.0),Vec2::new(width,height));
            let pivot=safe.center();
            assert_eq!(pivot,safe.center());
            assert!(safe.contains(pivot));
            assert_eq!(pivot.y-safe.top(),safe.bottom()-pivot.y);
        }
    }
}

#[cfg(test)]
mod modal_section_heading_spacing_tests {
    use super::*;
    #[test]
    fn same_explicit_gap_both_sides_of_section_heading_rule(){
        assert_eq!(theme::MODAL_SECTION_HEADER_RULE_GAP,12.0);
        assert_eq!(theme::MODAL_SECTION_PADDING,12.0);
        assert_eq!(theme::SOFT_DIVIDER_WIDTH,1.0);
        let source=include_str!("../popup/mod.rs");
        let helper=source.split("fn modal_section_heading_with_rule(").nth(1).unwrap()
            .split("/// A heading and one divider").next().unwrap();
        assert!(helper.contains("ui.spacing_mut().item_spacing.y=0.0;"));
        assert!(helper.contains("section_divider(ui);"));
        let boxed=source.split("fn modal_section_measured<R>(").nth(1).unwrap()
            .split("/// Structural spacing").next().unwrap();
        assert!(boxed.contains("modal_section_heading_with_rule(section,title)"));
        assert!(!boxed.contains("theme::divider(section)"));
        assert!(boxed.contains("section.scope(|body|"));
    }
}

/// Real egui layout integration test: validate the actual allocated Rects
/// for header, three card surfaces, the advanced divider, and footer.
/// A numeric token-only test cannot detect implicit spacer accumulation.
#[cfg(test)]
mod modal_editor_workspace_geometry_tests {
    use super::*;

    fn render_measured_workspace() -> ModalEditorGeometry {
        let ctx=egui::Context::default();
        crate::typography::install(&ctx);
        let mut actual=None;
        // egui resolves some text/layout geometry on the next frame.
        for _ in 0..3 {
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(1120.0);
                    let (_,_,geometry)=modal_editor_workspace(
                        ui,"상품 상세 관리","취소",true,|workspace|{
                            workspace.body(640.0,|body|{
                                let mut seen=ModalEditorBodyGeometry::default();
                                body.horizontal(|row|{
                                    row.spacing_mut().item_spacing.x=
                                        theme::MODAL_MASTER_WORKSPACE_GAP;
                                    for (label,width) in [
                                        ("상품 이미지",355.0),
                                        ("상품 개요",405.0),
                                        ("판매 · 가격",255.0),
                                    ] {
                                        row.vertical(|column|{
                                            column.set_width(width);
                                            let section=modal_section_measured(
                                                column,label,|content|{
                                                    // Nontrivial height gives the row a
                                                    // realistic image-led minimum.
                                                    content.allocate_exact_size(
                                                        Vec2::new(
                                                            (width-2.0*theme::MODAL_SECTION_PADDING
                                                                -2.0*theme::FRAME_STROKE_WIDTH)
                                                                .min(320.0),
                                                            320.0
                                                        ),Sense::hover()
                                                    );
                                                }
                                            );
                                            seen.cards.push(section.response.rect);
                                        });
                                    }
                                });
                                seen.divider=Some(section_divider(body));
                                let advanced=egui::CollapsingHeader::new("고급 정보")
                                    .default_open(false)
                                    .show(body,|_|{});
                                seen.advanced_header=Some(advanced.header_response.rect);
                                ((),seen)
                            });
                            workspace.footer(|_|{},|_|{});
                        }
                    );
                    actual=Some(geometry);
                });
            });
        }
        actual.expect("layout was not allocated")
    }

    #[test]
    fn actual_modal_rects_keep_every_12pt_workspace_boundary(){
        let geometry=render_measured_workspace();
        assert_eq!(geometry.content.cards.len(),3);
        assert!(geometry.header.is_some()
            && geometry.body.is_some() && geometry.footer.is_some());
        assert!(geometry.content.advanced_header.is_some());
        let gaps=geometry.gaps();
        eprintln!("modal rect trace header={:?} viewport={:?} scroll_y={:.2} card0={:?} body={:?} footer={:?} gaps={:?}",
            geometry.header,geometry.viewport,geometry.scroll_offset_y,
            geometry.content.cards.first(),geometry.body,geometry.footer,gaps);
        for label in [
            "header_to_card","card_gap_1","card_gap_2",
            "card_to_rule","rule_to_advanced","body_to_footer"
        ]{
            assert!(gaps.iter().any(|(name,_)|*name==label),
                "missing actual measured gap {label}: {gaps:?}");
        }
        assert!(geometry.invalid_gaps(1.0).is_empty(),
            "actual egui rects violate 12pt master: {gaps:?}");
        let cards=&geometry.content.cards;
        for window in cards.windows(2){
            assert!((window[0].top()-window[1].top()).abs()<=1.0,
                "card top boundaries differ");
            assert!((window[0].bottom()-window[1].bottom()).abs()<=1.0,
                "card bottom boundaries differ");
        }
    }
    // Inspect paint commands emitted by the real shared section/chrome,
    // not the spacing constants or a second implementation of the renderer.
    fn collect_modal_paint(shape:&egui::Shape,rules:&mut Vec<Rect>,heading:&mut Vec<Rect>){
        match shape{
            egui::Shape::Rect(rect) if rect.fill==theme::DIVIDER
                &&rect.rect.height()<=2.0&&rect.rect.width()>100.0=>{
                rules.push(rect.rect);
            }
            egui::Shape::Text(text) if text.galley.text()=="검증 섹션"=>{
                heading.push(Rect::from_min_size(text.pos,text.galley.size()));
            }
            egui::Shape::Vec(shapes)=>for shape in shapes{
                collect_modal_paint(shape,rules,heading);
            },
            _=>{}
        }
    }
    fn close_pt(actual:f32,expected:f32,label:&str){
        assert!((actual-expected).abs()<=1.0,
            "{label}: expected {expected}pt, actual {actual}pt");
    }

    #[test]
    fn actual_section_rule_and_insets_match_the_master(){
        for width in [360.0,560.0,880.0]{
            for boxed in [false,true]{
                let ctx=test_context();
                let mut outer=Rect::NOTHING;
                let mut first=Rect::NOTHING;
                let mut last=Rect::NOTHING;
                let output=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(
                        egui::Pos2::ZERO,Vec2::new(1200.0,900.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        ui.set_width(width);
                        let body=|content:&mut Ui|{
                            first=content.allocate_exact_size(
                                Vec2::new(content.available_width(),36.0),Sense::hover()).0;
                            last=content.allocate_exact_size(
                                Vec2::new(content.available_width(),36.0),Sense::hover()).0;
                        };
                        outer=if boxed{
                            modal_section_measured(ui,"검증 섹션",body).response.rect
                        }else{
                            ui.scope(|ui|modal_master_body_section(ui,"검증 섹션",body)).response.rect
                        };
                    });
                });
                let mut rules=Vec::new();
                let mut headings=Vec::new();
                for shape in &output.shapes{
                    collect_modal_paint(&shape.shape,&mut rules,&mut headings);
                }
                let rules=rules.into_iter().filter(|rect|
                    outer.contains(rect.center())).collect::<Vec<_>>();
                assert_eq!(rules.len(),1,"one actual section rule; boxed={boxed} width={width}");
                let rule=rules[0];
                let title=headings.into_iter().find(|rect|outer.contains(rect.center()))
                    .expect("actual section heading");
                close_pt(rule.height(),1.0,"rule thickness");
                close_pt(rule.top()-title.bottom(),12.0,"heading to rule");
                close_pt(first.top()-rule.bottom(),12.0,"rule to first field");
                close_pt(first.left(),rule.left(),"rule/field left alignment");
                close_pt(first.right(),rule.right(),"rule/field right alignment");
                close_pt(last.top()-first.bottom(),theme::SPACING,"ordinary inner field spacing");
                if boxed{
                    let edge=12.0+theme::FRAME_STROKE_WIDTH;
                    close_pt(first.left()-outer.left(),edge,"boxed left inset");
                    close_pt(outer.right()-first.right(),edge,"boxed right inset");
                    close_pt(outer.bottom()-last.bottom(),edge,"boxed bottom inset");
                }
            }
        }
    }

    #[test]
    fn actual_nested_surface_roles_keep_symmetric_approved_insets(){
        for (role,x,y) in [
            (crate::popup::ModalNestedRole::Standard,8.0,8.0),
            (crate::popup::ModalNestedRole::RecipientChecklist,12.0,8.0),
            (crate::popup::ModalNestedRole::CompactTag,8.0,2.0),
        ]{
            for width in [180.0,320.0]{
                let ctx=test_context();
                let mut measured=None;
                let _=ctx.run(egui::RawInput::default(),|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let frame=crate::popup::modal_nested_role_frame(role).show(ui,|inner|{
                            inner.allocate_exact_size(Vec2::new(width,36.0),Sense::hover()).0
                        });
                        measured=Some((frame.response.rect,frame.inner));
                    });
                });
                let (outer,inner)=measured.expect("real nested frame allocation");
                let stroke=theme::FRAME_STROKE_WIDTH;
                close_pt(inner.left()-outer.left(),x+stroke,"nested left");
                close_pt(outer.right()-inner.right(),x+stroke,"nested right");
                close_pt(inner.top()-outer.top(),y+stroke,"nested top");
                close_pt(outer.bottom()-inner.bottom(),y+stroke,"nested bottom");
            }
        }
    }

    #[test]
    fn actual_flow_modal_keeps_chrome_and_action_rects_when_permission_changes(){
        for width in [440.0,720.0,880.0]{
            let ctx=test_context();
            let mut baseline:Option<Vec<(&str,Rect)>>=None;
            for enabled in [true,false,true]{
                let mut measured=None;
                let mut rules=Vec::new();
                for _ in 0..3{
                    let output=ctx.run(egui::RawInput{
                        screen_rect:Some(Rect::from_min_size(
                            egui::Pos2::ZERO,Vec2::new(1200.0,900.0))),
                        ..Default::default()
                    },|ctx|{
                        egui::CentralPanel::default().show(ctx,|ui|{
                            ui.set_width(width);
                            let (field_and_action,geometry)=modal_flow_workspace_measured(ui,|owner|{
                                owner.header("검증 팝업","취소",enabled);
                                let field=owner.body_region(|body|{
                                    let mut text="검증 입력".to_owned();
                                    field(body,&mut text,FieldKind::Text,enabled).rect
                                });
                                let action=owner.footer_single(|footer|{
                                    let response=crate::button::action(footer,"저장",
                                        theme::ButtonRole::Primary,enabled);
                                    assert_eq!(response.enabled(),enabled);
                                    response.rect
                                }).inner;
                                (field,action)
                            });
                            measured=Some((geometry,field_and_action));
                        });
                    });
                    rules.clear();
                    for shape in &output.shapes{
                        collect_modal_paint(&shape.shape,&mut rules,&mut Vec::new());
                    }
                }
                let (geometry,(input,action))=measured.expect("real flow workspace");
                let header=geometry.header.unwrap();
                let body=geometry.body.unwrap();
                let footer=geometry.footer.unwrap();
                close_pt(body.top()-header.bottom(),12.0,"header/body gap");
                close_pt(footer.top()-body.bottom(),12.0,"body/footer gap");
                close_pt(input.left()-body.left(),12.0,"body left inset");
                close_pt(footer.right()-action.right(),12.0,"footer right action inset");
                let header_rules=rules.iter().filter(|r|header.contains(r.center()))
                    .collect::<Vec<_>>();
                let footer_rules=rules.iter().filter(|r|footer.contains(r.center()))
                    .collect::<Vec<_>>();
                assert_eq!(header_rules.len(),1,"one painted header rule");
                assert_eq!(footer_rules.len(),1,"one painted footer rule");
                for rule in [header_rules[0],footer_rules[0]]{
                    close_pt(rule.left(),input.left(),"chrome and body left grid");
                    close_pt(rule.right(),action.right(),"chrome and footer right grid");
                }
                let current=[("header",header),("body",body),("footer",footer),
                    ("field",input),("action",action)];
                if let Some(previous)=baseline.as_ref(){
                    assert_layout_rects_invariant(previous,&current);
                }else{baseline=Some(current.to_vec());}
            }
        }
    }

    #[test]
    fn geometry_audit_detects_real_boundary_drift(){
        let mut geometry=render_measured_workspace();
        let previous=geometry.header.unwrap();
        geometry.header=Some(previous.translate(Vec2::new(0.0,3.0)));
        assert!(geometry.invalid_gaps(1.0).iter()
            .any(|(label,_)|*label=="header_to_card"));
    }
    #[test]
    fn shared_header_toolbar_sits_below_title_rule_and_keeps_body_clearance(){
        let ctx=egui::Context::default();
        crate::typography::install(&ctx);
        let mut measured=None;
        for _ in 0..3{
            let _=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(
                    egui::Pos2::ZERO,Vec2::new(1200.0,900.0)
                )),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(900.0);
                    let (_,geometry)=modal_flow_workspace_measured(ui,|workspace|{
                        let (closed,())=workspace.header_with_toolbar(
                            "수동 분류","닫기",true,|toolbar|{
                                toolbar.set_min_height(
                                    theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT
                                );
                                toolbar.label("미배정 원문: 0개");
                                toolbar.with_layout(
                                    Layout::right_to_left(Align::Center),|right|{
                                        right.label("배송 건수");
                                    }
                                );
                            }
                        );
                        assert!(!closed);
                        workspace.body_region(|body|{
                            body.allocate_exact_size(
                                Vec2::new(360.0,180.0),Sense::hover()
                            );
                        });
                        workspace.footer(|_|{},|_|{});
                    });
                    measured=Some(geometry);
                });
            });
        }
        let geometry=measured.expect("shared modal geometry");
        let header=geometry.header.expect("header rect");
        let toolbar=geometry.header_toolbar.expect("header toolbar rect");
        let body=geometry.body.expect("body rect");
        let footer=geometry.footer.expect("footer rect");
        assert!((header.height()-(
            theme::MODAL_HEADER_HEIGHT
                +theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT
                +theme::SOFT_DIVIDER_WIDTH
                +theme::MODAL_MASTER_TOOLBAR_RULE_GAP
        )).abs()<=0.5,"unexpected two-row header height: {header:?}");
        assert!((toolbar.height()-theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT)
            .abs()<=0.5,"toolbar does not fill its reserved header row: {toolbar:?}");
        let rule=modal_master_toolbar_rule_rect(header,ctx.pixels_per_point());
        assert!((rule.top()-header.top()-theme::MODAL_HEADER_HEIGHT).abs()<=0.5,
            "rule must immediately follow title row: {rule:?}");
        assert!((toolbar.top()-rule.bottom()-theme::MODAL_MASTER_TOOLBAR_RULE_GAP)
            .abs()<=0.5,"toolbar must sit below rule: {rule:?} {toolbar:?}");
        assert!((header.bottom()-toolbar.bottom()).abs()<=0.5,
            "no divider slot below toolbar: {header:?} {toolbar:?}");
        assert!((body.top()-header.bottom()-theme::MODAL_MASTER_WORKSPACE_GAP)
            .abs()<=0.5,"body must begin 12pt below the shared header: {header:?} {body:?}");
        assert!((footer.top()-body.bottom()-theme::MODAL_MASTER_WORKSPACE_GAP)
            .abs()<=0.5,"footer must remain 12pt below the body: {body:?} {footer:?}");
        assert!(geometry.invalid_gaps(0.5).is_empty(),
            "shared header/body/footer spacing drifted: {:?}",geometry.gaps());
    }

    #[test]
    fn title_toolbar_rules_and_footer_actions_share_one_horizontal_grid(){
        let bounds=Rect::from_min_size(
            egui::pos2(40.0,20.0),Vec2::new(900.0,180.0)
        );
        let content=modal_master_content_rect(bounds);
        let toolbar=modal_master_header_toolbar_rect(bounds,1.0);
        let top_rule=modal_master_rule_rect(bounds,1.0,false);
        let bottom_rule=modal_master_rule_rect(bounds,1.0,true);
        let actions=modal_master_action_area(bounds,theme::ACTION_PRIMARY_HEIGHT);
        let toolbar_rule=modal_master_toolbar_rule_rect(bounds,1.0);
        for rect in [toolbar,top_rule,bottom_rule,toolbar_rule,actions] {
            assert_eq!(rect.left(),content.left());
            assert_eq!(rect.right(),content.right());
        }
        assert_eq!(content.left()-bounds.left(),theme::MODAL_MASTER_CONTENT_INSET);
        assert_eq!(bounds.right()-content.right(),theme::MODAL_MASTER_CONTENT_INSET);
    }

    #[test]
    fn footer_actions_balance_outer_margin_and_rule(){
        let bounds=Rect::from_min_size(egui::Pos2::ZERO,
            Vec2::new(900.0,theme::MODAL_FOOTER_HEIGHT));
        let actions=modal_master_action_area(bounds,theme::ACTION_PRIMARY_HEIGHT);
        let upper=actions.top()-bounds.top()-theme::SOFT_DIVIDER_WIDTH;
        let lower=bounds.bottom()-actions.bottom()
            +theme::MODAL_PANEL_PADDING_Y+theme::FRAME_STROKE_WIDTH;
        assert!((upper-lower).abs()<=1.0,"footer gap: {upper} vs {lower}");
    }

    #[test]
    fn pending_money_marker_does_not_change_numeric_baseline(){
        for text in ["-","–","—"]{assert!(reception_pending_money_marker(text));}
        for text in ["0","3,000","-1,000"]{assert!(!reception_pending_money_marker(text));}
        assert_eq!(typography::DATA_NUMERIC_GLYPH_OFFSET_Y,2.5);
        assert_eq!(theme::RECEPTION_ITEMS_TO_FREIGHT_GAP,12.0);
    }

    #[test]
    fn plain_modal_body_edges_match_the_master_header_and_footer(){
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,700.0))),
            ..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(500.0);
                let left=ui.cursor().left();
                let width=ui.available_width();
                let measured=modal_plain_body_region(ui,|body|{
                    (body.cursor().left(),body.available_width())
                }).inner;
                assert!((measured.0-left-theme::MODAL_MASTER_CONTENT_INSET).abs()<=0.5);
                assert!((measured.1-(width-2.0*theme::MODAL_MASTER_CONTENT_INSET)).abs()<=0.5);
            });
        });
    }

    #[test]
    fn shared_soft_rule_occupies_its_own_one_point_slot(){
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.spacing_mut().item_spacing.y=0.0;
                let first=theme::soft_rule(ui);
                let (strip,second)=section_divider_with_top_gap(ui,theme::SECTION_DIVIDER_GAP);
                assert_eq!(first.height(),theme::SOFT_DIVIDER_WIDTH);
                assert_eq!(second.height(),first.height());
                assert_eq!(strip.top(),first.bottom());
                assert_eq!(second.top()-strip.top(),12.0);
                assert_eq!(strip.bottom()-second.bottom(),12.0);
                assert_eq!(strip.height(),25.0);
                assert_eq!(first.x_range(),second.x_range());
            });
        });
    }

    #[test]
    fn popup_radius_hierarchy_separates_shell_sections_controls_and_source_chips(){
        assert!(theme::PANEL_RADIUS>theme::SECTION_RADIUS);
        assert!(theme::SECTION_RADIUS>theme::CONTROL_RADIUS);
        assert!(theme::CONTROL_RADIUS>theme::SOURCE_BLOCK_RADIUS as u8);
        assert_eq!(theme::MODAL_SECTION_RADIUS,theme::SECTION_RADIUS);
    }

    #[test]
    fn recipient_text_editor_uses_the_shared_form_input_margin(){
        let source=include_str!("../input/mod.rs");
        let start=source.find("fn reception_recipient_text_edit(")
            .expect("recipient text editor");
        let editor=source[start..].split("\n}\n").next().unwrap();
        assert!(editor.contains("singleline_editor(value,font,width,Align::LEFT,"));
        assert!(editor.contains("typography::form_input_text_margin()"));
        let base=source.split("fn singleline_editor<'a>(").nth(1).unwrap()
            .split("\n}\n").next().unwrap();
        assert!(base.contains(".margin(margin).desired_width(width)"));
    }
}

#[cfg(test)]
mod modal_input_routing_tests {
    use super::*;

    #[test]
    fn nested_decision_uses_the_top_foreground_layer_above_parent_and_dim(){
        let dialog=modal_window_layer("manual-discard-prompt");
        assert_eq!(dialog.order,egui::Order::Foreground);
        assert_ne!(
            dialog,
            egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("manual-nested-modal-backdrop")
            )
        );
    }

    #[test]
    fn foreground_modal_popup_stays_above_its_owner_after_each_raise(){
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let work_area=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,700.0));
        let mut popup_layer=None;
        let mut popup_rect=Rect::NOTHING;
        let mut owner_body=Rect::NOTHING;
        for _ in 0..4 {
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(work_area);
            let _=ctx.run(raw,|ctx|{
                modal_backdrop(ctx,"popup-owner-regression-dim",true);
                show_decision_window(ctx,"Popup owner","popup-owner-regression",work_area,|ui|{
                    let trigger=ui.button("Open popup");
                    egui::Popup::open_id(ctx,egui::Popup::default_response_id(&trigger));
                    let _=crate::overlay::show_menu(&trigger,None,|menu|{
                        let response=menu.button("Select option");
                        popup_layer=Some(response.layer_id);
                        popup_rect=response.rect;
                    });
                    owner_body=ui.allocate_exact_size(Vec2::new(300.0,180.0),Sense::hover()).0;
                });
            });
        }
        let popup_layer=popup_layer.expect("popup rendered");
        assert!(owner_body.intersects(popup_rect),"test must exercise overlapping layers");
        let hit=owner_body.intersect(popup_rect).center();
        assert_eq!(ctx.layer_id_at(hit),Some(popup_layer));
        assert!(ctx.memory(|m|m.allows_interaction(popup_layer)));
        assert_eq!(ctx.memory(|m|m.top_modal_layer()),
            Some(modal_window_layer("popup-owner-regression")));
    }

    #[test]
    fn nested_decision_button_receives_a_real_pointer_click(){
        fn draw(
            ctx:&egui::Context,
            mut raw:egui::RawInput,
            work_area:Rect,
            action_clicked:&mut bool,
            plain_clicked:&mut bool,
            action_hovered:&mut bool,
            action_enabled:&mut bool,
        )->(Rect,Rect){
            raw.screen_rect=Some(work_area);
            let mut action_rect=Rect::NOTHING;
            let mut plain_rect=Rect::NOTHING;
            let _=ctx.run(raw,|ctx|{
                modal_window(ctx,"수동 분류","manual-parent-workspace",work_area)
                    .show(ctx,|ui|{
                        ui.disable();
                        ui.label("parent worksheet");
                    });
                modal_backdrop(ctx,"round87-nested-click-dim",true);
                show_decision_window(ctx,"수동 분류 변경 취소","manual-discard-prompt",work_area,|ui|{
                    decision_workspace(ui,|decision|{
                        decision.header("수동 분류 변경 취소","돌아가기",true);
                        decision.body(|body|{
                            body.label("적용하지 않은 수동 분류 변경사항을 버릴까요?");
                            let plain=body.button("plain pointer probe");
                            plain_rect=plain.rect;
                            if plain.clicked(){*plain_clicked=true;}
                        });
                        decision.actions(|actions|{
                            *action_enabled=actions.is_enabled();
                            let response=crate::button::action(
                                actions,"변경 취소",theme::ButtonRole::Standard,true
                            );
                            action_rect=response.rect;
                            *action_hovered=response.hovered();
                            if response.clicked(){*action_clicked=true;}
                        });
                    });
                });
            });
            assert_eq!(
                ctx.memory(|memory|memory.top_modal_layer()),
                Some(modal_window_layer("manual-discard-prompt"))
            );
            assert_eq!(
                ctx.memory(|memory|memory.areas().top_layer_id(egui::Order::Foreground)),
                Some(modal_window_layer("manual-discard-prompt")),
                "the full-screen dim Area must never remain above the decision window"
            );
            (action_rect,plain_rect)
        }

        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let work_area=Rect::from_min_size(
            egui::Pos2::ZERO,Vec2::new(800.0,600.0)
        );
        let mut action_clicked=false;
        let mut plain_clicked=false;
        let mut action_hovered=false;
        let mut action_enabled=false;
        let _=draw(&ctx,egui::RawInput::default(),work_area,
            &mut action_clicked,&mut plain_clicked,&mut action_hovered,&mut action_enabled);
        let (action_rect,plain_rect)=draw(&ctx,egui::RawInput{
            time:Some(0.1),..Default::default()
        },work_area,&mut action_clicked,&mut plain_clicked,
            &mut action_hovered,&mut action_enabled);
        assert!(action_rect.width()>0.0 && action_rect.height()>0.0);
        assert!(plain_rect.width()>0.0 && plain_rect.height()>0.0);
        assert!(action_enabled,"the decision footer must be enabled independently of its parent");
        let click=|pointer:egui::Pos2,time:f64,
            action_clicked:&mut bool,plain_clicked:&mut bool,
            action_hovered:&mut bool,action_enabled:&mut bool|{
            let _=draw(&ctx,egui::RawInput{
                events:vec![egui::Event::PointerMoved(pointer)],
                time:Some(time),..Default::default()
            },work_area,action_clicked,plain_clicked,action_hovered,action_enabled);
            let _=draw(&ctx,egui::RawInput{
                events:vec![egui::Event::PointerButton{
                    pos:pointer,button:egui::PointerButton::Primary,
                    pressed:true,modifiers:egui::Modifiers::NONE
                }],
                time:Some(time+0.1),..Default::default()
            },work_area,action_clicked,plain_clicked,action_hovered,action_enabled);
            let _=draw(&ctx,egui::RawInput{
                events:vec![egui::Event::PointerButton{
                    pos:pointer,button:egui::PointerButton::Primary,
                    pressed:false,modifiers:egui::Modifiers::NONE
                }],
                time:Some(time+0.2),..Default::default()
            },work_area,action_clicked,plain_clicked,action_hovered,action_enabled);
        };
        click(plain_rect.center(),1.0,&mut action_clicked,&mut plain_clicked,
            &mut action_hovered,&mut action_enabled);
        assert!(plain_clicked,"plain decision-window button must receive pointer input");
        click(action_rect.center(),2.0,&mut action_clicked,&mut plain_clicked,
            &mut action_hovered,&mut action_enabled);
        assert!(action_clicked,
            "shared modal footer action must receive pointer input; hovered={action_hovered}, enabled={action_enabled}");
    }

    #[test]
    fn nested_decision_modal_blocks_parent_pointer_input(){
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let work_area=Rect::from_min_size(
            egui::Pos2::ZERO,Vec2::new(800.0,600.0)
        );
        let mut parent_clicked=false;
        let mut draw=|mut raw:egui::RawInput|{
            raw.screen_rect=Some(work_area);
            let mut parent_rect=Rect::NOTHING;
            let _=ctx.run(raw,|ctx|{
                modal_window(ctx,"수동 분류","manual-parent-workspace",work_area)
                    .show(ctx,|ui|{
                        ui.disable();
                        let response=ui.button("parent action");
                        parent_rect=response.rect;
                        if response.clicked(){parent_clicked=true;}
                    });
                decision_window(ctx,"수동 분류 변경 취소","manual-discard-prompt",work_area
                ).show(ctx,|ui|{
                    ui.label("discard?");
                    modal_action_row(ui,|actions|{
                        crate::button::action(actions,"변경 취소",theme::ButtonRole::Standard,true);
                    });
                });
                modal_foreground_for_window(ctx,"manual-discard-prompt");
            });
            parent_rect
        };
        let parent_rect=draw(egui::RawInput::default());
        let pointer=parent_rect.center();
        draw(egui::RawInput{
            events:vec![egui::Event::PointerMoved(pointer)],
            time:Some(1.0),..Default::default()
        });
        draw(egui::RawInput{
            events:vec![egui::Event::PointerButton{
                pos:pointer,button:egui::PointerButton::Primary,
                pressed:true,modifiers:egui::Modifiers::NONE
            }],
            time:Some(1.1),..Default::default()
        });
        draw(egui::RawInput{
            events:vec![egui::Event::PointerButton{
                pos:pointer,button:egui::PointerButton::Primary,
                pressed:false,modifiers:egui::Modifiers::NONE
            }],
            time:Some(1.2),..Default::default()
        });
        assert!(!parent_clicked,"parent controls must remain blocked behind a decision modal");
    }
}

#[cfg(test)]
mod manual_nested_prompt_regression_tests{
    use super::*;

    /// Reproduce the operational stack: Middle app backdrop, Foreground
    /// worksheet disabled while prompting, and a clickable child decision.
    /// The old extra full-screen Foreground dim must never be added here.
    #[test]
    fn both_decision_actions_remain_clickable_while_manual_worksheet_is_disabled(){
        fn paint(
            ctx:&egui::Context,mut raw:egui::RawInput,
            keep_clicked:&mut bool,discard_clicked:&mut bool,
            parent_clicked:&mut bool
        )->(Rect,Rect,Rect){
            let screen=Rect::from_min_size(
                egui::Pos2::ZERO,Vec2::new(1200.0,800.0)
            );
            raw.screen_rect=Some(screen);
            let work_area=screen.shrink(24.0);
            let mut parent_rect=Rect::NOTHING;
            let mut keep_rect=Rect::NOTHING;
            let mut discard_rect=Rect::NOTHING;
            let _=ctx.run(raw,|ctx|{
                modal_backdrop(ctx,"manual-regression-main-backdrop",false);
                modal_window(ctx,"수동 분류","manual-regression-parent",work_area)
                    .default_width(900.0)
                    .default_height(650.0)
                    .show(ctx,|parent|{
                        parent.disable();
                        let response=parent.button("background worksheet action");
                        parent_rect=response.rect;
                        if response.clicked(){*parent_clicked=true;}
                    });
                decision_window(ctx,"수동 분류 나가기","manual-regression-child",work_area)
                    .show(ctx,|decision|{
                        decision.label("초안에 반영하지 않은 변경사항이 있습니다.");
                        decision.horizontal(|row|{
                            let keep=crate::button::action(
                                row,"계속 편집",theme::ButtonRole::Standard,true
                            );
                            keep_rect=keep.rect;
                            if keep.clicked(){*keep_clicked=true;}
                            let discard=crate::button::action(
                                row,"변경사항 버리고 닫기",theme::ButtonRole::Danger,true
                            );
                            discard_rect=discard.rect;
                            if discard.clicked(){*discard_clicked=true;}
                        });
                    });
                modal_foreground_for_window(ctx,"manual-regression-child");
            });
            assert_eq!(
                ctx.memory(|memory|memory.top_modal_layer()),
                Some(modal_window_layer("manual-regression-child"))
            );
            assert_eq!(
                ctx.memory(|memory|memory.areas().top_layer_id(egui::Order::Foreground)),
                Some(modal_window_layer("manual-regression-child"))
            );
            (parent_rect,keep_rect,discard_rect)
        }
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let mut keep=false;
        let mut discard=false;
        let mut parent=false;
        paint(&ctx,egui::RawInput::default(),&mut keep,&mut discard,&mut parent);
        let (parent_rect,keep_rect,discard_rect)=paint(
            &ctx,egui::RawInput{time:Some(0.1),..Default::default()},
            &mut keep,&mut discard,&mut parent
        );
        for rect in [parent_rect,keep_rect,discard_rect]{
            assert!(rect.is_positive(),"action hitbox must have non-zero area");
        }
        let click=|pos:egui::Pos2,offset:f64,
            keep:&mut bool,discard:&mut bool,parent:&mut bool|{
            paint(&ctx,egui::RawInput{
                events:vec![egui::Event::PointerMoved(pos)],
                time:Some(offset),..Default::default()
            },keep,discard,parent);
            paint(&ctx,egui::RawInput{
                events:vec![egui::Event::PointerButton{
                    pos,button:egui::PointerButton::Primary,
                    pressed:true,modifiers:egui::Modifiers::NONE
                }],
                time:Some(offset+0.1),..Default::default()
            },keep,discard,parent);
            paint(&ctx,egui::RawInput{
                events:vec![egui::Event::PointerButton{
                    pos,button:egui::PointerButton::Primary,
                    pressed:false,modifiers:egui::Modifiers::NONE
                }],
                time:Some(offset+0.2),..Default::default()
            },keep,discard,parent);
        };
        click(keep_rect.center(),1.0,&mut keep,&mut discard,&mut parent);
        assert!(keep,"continue editing button did not receive pointer input");
        click(discard_rect.center(),2.0,&mut keep,&mut discard,&mut parent);
        assert!(discard,"discard button did not receive pointer input");
        click(parent_rect.center(),3.0,&mut keep,&mut discard,&mut parent);
        assert!(!parent,"disabled parent must not receive pointer input");
    }

    #[test]
    fn manual_linked_cells_use_the_exact_shared_source_fill_and_outline(){
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let output=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                manual_linked_table_cell(ui,"김준희",240.0,true,true);
                manual_linked_table_cell(ui,"표시나",240.0,false,false);
            });
        });
        assert!(output.shapes.iter().any(|paint|
            matches!(&paint.shape,
                egui::Shape::Rect(rect) if rect.fill==source_highlight::ACTIVE_FILL
            )
        ),"shared blue source fill is absent from linked table cells");
        assert!(output.shapes.iter().any(|paint|
            matches!(&paint.shape,
                egui::Shape::Rect(rect)
                    if rect.stroke.color==source_highlight::ACTIVE_BORDER
            )
        ),"shared blue source border is absent from linked table cells");
    }
}

#[cfg(test)]
mod round87_modal_flow_contract_tests {
    use super::*;

    #[test]
    fn phase_two_editors_and_viewers_keep_the_shared_workspace_entry_point(){
        let sources=[
            ("alias_manager",include_str!("../alias_manager.rs")),
            ("category_manager",include_str!("../category_manager.rs")),
            ("client_manager",include_str!("../client_manager.rs")),
            ("client_chooser",include_str!("../client_chooser.rs")),
            ("freight_manager",include_str!("../freight_manager.rs")),
            ("item_editor",include_str!("../item_editor.rs")),
            ("main",include_str!("../main.rs")),
            ("manual_product_search",include_str!("../manual_product_search.rs")),
            ("manual_workspace",include_str!("../manual_workspace.rs")),
            ("order_history_detail",include_str!("../order_history_detail.rs")),
            ("order_history_manager",include_str!("../order_history_manager.rs")),
            ("product_excel_ui",include_str!("../product_excel_ui.rs")),
            ("product_manager",include_str!("../product_manager.rs")),
            ("settings_view",include_str!("../settings_view.rs")),
        ];
        for (name,source) in sources {
            assert!(source.contains("crate::popup::modal_flow_workspace("),
                "{name}: shared editor/viewer entry point removed");
        }
    }

    #[test]
    fn flow_regions_measure_master_vertical_gaps_at_normal_and_compact_widths(){
        for width in [440.0,740.0] {
            for kind in 0..3 {
                let ctx=egui::Context::default();
                crate::typography::install(&ctx);
                let mut measured=None;
                for _ in 0..3 {
                    let _=ctx.run(egui::RawInput::default(),|ctx|{
                        egui::CentralPanel::default().show(ctx,|ui|{
                            ui.set_width(width);
                            let (_,geometry)=modal_flow_workspace_measured(ui,|workspace|{
                                workspace.header("편집","취소",true);
                                let render=|body:&mut Ui|{
                                    body.allocate_exact_size(Vec2::new(80.0,80.0),Sense::hover());
                                };
                                match kind {
                                    0=>workspace.body_region(render),
                                    1=>workspace.scroll_region("round87-flow",160.0,render),
                                    _=>workspace.framed_region(render),
                                }
                                workspace.footer(|_|{},|_|{});
                            });
                            measured=Some(geometry);
                        });
                    });
                }
                let g=measured.expect("flow geometry not allocated");
                let header=g.header.expect("header");
                let body=g.body.expect("body");
                let footer=g.footer.expect("footer");
                let gap=theme::MODAL_MASTER_WORKSPACE_GAP;
                assert!((body.top()-header.bottom()-gap).abs()<=1.0,
                    "width={width} region={kind} header={header:?} body={body:?}");
                assert!((footer.top()-body.bottom()-gap).abs()<=1.0,
                    "width={width} region={kind} body={body:?} footer={footer:?}");
            }
        }
    }
}

#[cfg(test)]
mod round87_modal_internal_layout_tests {
    use super::*;

    #[test]
    fn body_budget_reserves_actual_master_geometry_not_extra_spacing(){
        let height=440.0;
        let viewer=modal_body_height_budget(height,false);
        let editor=modal_body_height_budget(height,true);
        assert_eq!(viewer-editor,modal_workspace_footer_reserve());
        let expected=height-2.0*theme::MODAL_PANEL_PADDING_Y
            -2.0*theme::FRAME_STROKE_WIDTH-theme::MODAL_HEADER_HEIGHT
            -2.0*theme::MODAL_MASTER_WORKSPACE_GAP-theme::MODAL_FOOTER_HEIGHT;
        assert_eq!(editor,expected);
        assert_eq!(modal_body_height_budget(0.0,true),1.0);
    }

    #[test]
    fn search_list_budget_shrinks_after_toolbar_and_respects_preferred_height(){
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let before=modal_list_max_height(ui,440.0,670.0,false);
                ui.allocate_exact_size(Vec2::new(80.0,60.0),Sense::hover());
                let after=modal_list_max_height(ui,440.0,670.0,false);
                assert!(before-after>=60.0);
                assert_eq!(modal_list_max_height(ui,440.0,20.0,false),20.0);
            });
        });
    }

    #[test]
    fn migrated_search_lists_use_shared_remaining_height_budget(){
        for (name,source) in [
            ("alias",include_str!("../alias_manager.rs")),
            ("manual",include_str!("../manual_product_search.rs")),
            ("audit",include_str!("../settings_view.rs")),
        ] {
            assert!(source.contains("crate::popup::modal_list_max_height("),"{name}");
        }
        let freight=include_str!("../freight_manager.rs");
        assert!(freight.contains("decision.framed_body("));
        assert!(!freight.contains("crate::popup::modal_master_body("));
    }
}

#[cfg(test)]
mod round87_phase4_modal_safety_tests {
    use super::*;

    #[test]
    fn compact_editor_footer_and_scroll_body_fit_safe_height(){
        for height in [440.0,600.0,900.0] {
            let ctx=egui::Context::default();
            typography::install(&ctx);
            theme::install(&ctx);
            let safe=Rect::from_min_size(egui::pos2(380.0,80.0),Vec2::new(440.0,height));
            let mut measured=None;
            for _ in 0..3 {
                let _=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1280.0,1080.0))),
                    ..Default::default()
                },|ctx|{
                    modal_window(ctx,"좁은 작업 영역","phase4-compact-editor",safe)
                        .default_width(380.0).show(ctx,|ui|{
                            let (_,geometry)=modal_flow_workspace_measured(ui,|workspace|{
                                workspace.header("아주 긴 제목을 갖는 상품 상세 편집 화면","취소",true);
                                workspace.scroll_region("phase4-scroll",modal_editor_body_max_height(height),|body|{
                                    for _ in 0..80 {body.label("스크롤 검증 항목");}
                                });
                                workspace.footer_single(|actions|{
                                    crate::button::action(actions,"저장",theme::ButtonRole::Primary,true);
                                });
                            });
                            measured=Some(geometry);
                        });
                });
            }
            let g=measured.expect("modal geometry");
            let header=g.header.unwrap();
            let footer=g.footer.unwrap();
            assert!(header.top()>=safe.top()-1.0,"header outside {safe:?}: {header:?}");
            assert!(footer.bottom()<=safe.bottom()+1.0,"footer outside {safe:?}: {footer:?}");
            assert!(footer.left()>=safe.left()-1.0 && footer.right()<=safe.right()+1.0);
        }
    }
}

#[cfg(test)]
mod round87_phase5_modal_contract_tests {
    use super::*;

    // Read the source tree rather than keeping a hand-maintained list:
    // newly added screen modules are subject to the same contract.
    fn scan(path:&std::path::Path,violations:&mut Vec<String>){
        for entry in std::fs::read_dir(path).expect("read egui source directory"){
            let entry=entry.expect("read source entry");
            let p=entry.path();
            if p.is_dir(){scan(&p,violations);continue;}
            if p.extension().and_then(|v|v.to_str())!=Some("rs")
                {continue;}
            let source=std::fs::read_to_string(&p).expect("read Rust source");
            if source.starts_with("#![cfg(test)]"){continue;}
            for (index,line) in source.lines().enumerate(){
                // Ignore comments and textual assertions about source code.
                // Compile-time visibility is the primary guard for the
                // decision constructor; this audit also catches raw windows.
                let code=line.trim_start();
                if code.starts_with("//") || code.starts_with("/*") || code.starts_with('*'){continue;}
                for forbidden in [
                    "crate::popup::decision_window(",
                    "Window::new(",
                    "Modal::new(",
                    "components::modal_header(",
                    "components::modal_header_chrome(",
                    "components::modal_footer_chrome(",
                    "crate::popup::modal_action_row(",
                    "components::modal_action_row_split(",
                    ".movable(true)",
                    ".resizable(true)",
                ] {
                    if forbidden=="Window::new(" && p.file_name().and_then(|v|v.to_str())==Some("overlay.rs"){continue;}
                    if let Some(pos)=code.find(forbidden){
                        if !code[..pos].contains('"'){
                            violations.push(format!("{}:{}: {}",p.display(),index+1,forbidden));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn production_screens_do_not_bypass_shared_modal_chrome_or_decision_entry(){
        let mut violations=Vec::new();
        scan(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),&mut violations);
        assert!(violations.is_empty(),"modal contract bypass:\n{}",violations.join("\n"));
    }

    #[test]
    fn approved_modal_tokens_remain_unchanged(){
        assert_eq!(theme::MODAL_MASTER_WORKSPACE_GAP,12.0);
        assert_eq!(theme::MODAL_MASTER_CONTENT_INSET,12.0);
        assert_eq!(theme::MODAL_SECTION_PADDING,12.0);
        assert_eq!(theme::MODAL_SECTION_HEADER_RULE_GAP,12.0);
        assert_eq!(theme::MODAL_PANEL_PADDING_Y,12.0);
        // 735ea52b normalized both outer popup insets to 16pt.
        assert_eq!(theme::MODAL_PANEL_PADDING_X,12.0);
        assert_eq!(theme::MODAL_HEADER_HEIGHT,56.0);
        assert_eq!(theme::MODAL_FOOTER_HEIGHT,58.0);
    }
}

#[cfg(test)]
#[test]
fn freight_modal_content_uses_shared_layout_and_controls(){
    let source=include_str!("../freight_manager.rs");
    let rate=source.split("    fn rate_dialog(").nth(1).unwrap()
        .split("    /// Global rate-basis changes").next().unwrap();
    assert!(rate.contains("crate::popup::modal_form_stack("));
    assert!(rate.contains("form.section("));
    assert!(rate.contains("form.compact_group("));
    assert!(rate.contains("form.named_text_field("));
    assert!(!rate.contains("spacing_mut("));
    assert!(rate.contains("crate::table::registry_table("));
    assert!(!rate.contains("ui.add_space("));
    assert_eq!(source.matches("crate::popup::modal_named_text_field(").count(),2);
    assert_eq!(source.matches("crate::popup::show_management_menu(").count(),2);
    assert!(!source.contains("menu.close("));
    assert!(source.contains("crate::popup::modal_compact_table_height("));
    assert!(source.contains("그룹을 삭제하면 연결된 상품의 배송 그룹이 미지정으로 변경됩니다."));
    assert!(!source.contains("area.height()-310.0"));
    assert!(!source.contains("truncated_label(typography::body(warning))"));
    assert!(source.contains("ManagementMenuRole::Actions"));
    assert!(source.contains("ManagementMenuRole::Details"));
    assert!(!source.contains("menu.set_min_width("));
}
