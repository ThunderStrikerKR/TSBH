//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, Sense, Stroke, Ui, UiBuilder, Vec2};
use chrono::{Datelike, Duration as Days, Months, NaiveDate};
use crate::{source_highlight, theme, typography};
use crate::ui_primitives::*;
use crate::display_format::{display_won,display_money_number};
use crate::card::paint_shipment_detail_surface;
#[cfg(test)]
use crate::ui_test_support::test_context;
use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.
#[cfg(test)]
use crate::button::{management_tabs,reception_item_header_action};
use crate::input::reception_recipient_value_hit;
use crate::dropdown::reception_cod_dropdown;
use crate::button::{date_navigation_segment_button,date_navigation_button_surface,DateNavigationSegmentGlyph};
#[cfg(test)]
use crate::button::{management_date_segment_corners,ManagementDateSegmentEdge};
use crate::input::{FieldKind,compact_text_field,field,parse_money_input,money_field,money_edit_text,money_field_with_unit,flexible_text_field,management_date_text_field,multiline_text_field,multiline_text_field_sized,reception_source_text_edit,management_search_field,reception_recipient_inline_cell_editor,reception_inline_cell_editor,reception_freight_inline_cell_editor};
use crate::input::{singleline_editor,allocate_singleline_field,management_date_text_field_impl,reception_recipient_text_edit,reception_text_editor_at_click,reception_money_inline_cell_editor,reception_freight_money_readonly,RECEPTION_MONEY_UNIT_GAP,RECEPTION_MONEY_PENDING,money_unit_left};
#[cfg(test)]
use crate::input::{formatted_money_input,money_cursor_after_format,money_unit_margin,reception_money_font,RECEPTION_FREIGHT_MONEY_WIDTH};
use crate::dropdown::{ManagementSearchChoice,management_dropdown,management_dropdown_full_label,management_status_dropdown,management_dropdown_compact,management_dropdown_values,management_searchable_dropdown,management_dropdown_option,management_dropdown_option_values,management_status_dropdown_option,management_dropdown_compact_option};
use crate::dropdown::{management_dropdown_trigger,management_dropdown_trigger_full_label};
#[cfg(test)]
use crate::dropdown::{management_dropdown_option_palette,management_search_choice_matches};
// Shared table primitives used by reception and other composite widgets.
use crate::table::{
    paint_table_galley,
    shipment_detail_cells,
    shipment_detail_product_galley,
    shipment_detail_block_height,
    paint_shipment_detail_products,
    workflow_shipment_columns,
    management_client_column_widths,
    management_client_column_edges,
    reception_item_columns,
    reception_item_text_clip,
    reception_items_header,
    reception_item_value_origin,
    management_table_container_style,management_table_cells,
    management_table_row_fill,paint_management_table_row_background,
    paint_management_table_row_rule,management_table_row_body,
    management_table_column_rules,
};
#[cfg(test)]
use crate::table::{management_table_scroll_track_rect,management_readonly_table_content_width,management_readonly_table_row_advance};
use crate::table::{management_master_column_rules,management_table_data_width,management_inline_list_table_header};
#[cfg(test)]
use crate::table::{virtual_fixed_rows,management_fixed_table_scroll_area,management_sticky_table_header,management_table_scroll_track_geometry,management_remaining_predivider_gap};

#[cfg(test)]
mod management_table_container_style_tests{
    use super::*;
    #[test]
    fn narrow_fixed_table_reserves_horizontal_rail_on_every_frame(){
        for (width,content_width) in [(450.0,700.0),(450.0,440.0)]{
            let ctx=test_context();
            let mut baseline:Option<Rect>=None;
            for count in [0,0,1,1,65,65,0,0]{
                let _=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,700.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|root|{
                        root.allocate_ui_with_layout(Vec2::new(width,240.0),
                            Layout::top_down(Align::Min),|ui|{
                            ui.set_max_width(width);
                            management_table_container_style(ui);
                            let scroll=management_fixed_table_scroll_area(ui,content_width)
                                .id_salt("fixed-management-rail-test")
                                .max_height(240.0).auto_shrink([false,false]);
                            let output=virtual_fixed_rows(scroll,ui,content_width,42.0,count,
                                |rows,visible|{
                                    for _ in visible{
                                        rows.allocate_exact_size(Vec2::new(content_width,42.0),Sense::hover());
                                    }
                                });
                            assert!(output.inner_rect.is_positive());
                            if let Some(rect)=baseline{
                                assert!((rect.top()-output.inner_rect.top()).abs()<=0.5);
                                assert!((rect.bottom()-output.inner_rect.bottom()).abs()<=0.5,
                                    "row count {count}: {rect:?} -> {:?}",output.inner_rect);
                            }else{baseline=Some(output.inner_rect);}
                            if count==0{assert_eq!(output.content_size.y,0.0);}
                        });
                    });
                });
            }
        }
    }

    #[test]
    fn container_and_row_body_preserve_reference_metrics(){
        let ctx=test_context();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.spacing_mut().item_spacing.y=7.0;
                ui.spacing_mut().item_spacing.x=13.0;
                let old=management_table_container_style(ui);
                let expected=crate::scrollbar::management_table_scroll_style();
                assert_eq!(old,7.0);
                assert_eq!(ui.spacing().item_spacing.y,0.0);
                assert_eq!(ui.spacing().item_spacing.x,13.0);
                assert_eq!(ui.spacing().scroll.bar_width,expected.bar_width);
                assert_eq!(ui.spacing().scroll.allocated_width(),expected.allocated_width());
                assert_eq!(ui.spacing().scroll.bar_inner_margin,expected.bar_inner_margin);
                assert_eq!(ui.spacing().scroll.bar_outer_margin,expected.bar_outer_margin);
                ui.spacing_mut().item_spacing.y=old;
                assert_eq!(ui.spacing().item_spacing.y,7.0);
                management_table_row_body(ui,420.0);
                assert!(ui.min_rect().width()>=420.0);
                assert_eq!(ui.spacing().item_spacing.y,0.0);
            });
        });
    }
}


/// Zero-layout feedback for fixed management table/scroll viewports.
///
/// Pending, empty and no-result text must never create an egui row, change a
/// header's Y coordinate, consume scrollbar height or resize the enclosing
/// card. Call this ONLY after the caller has reserved a stable body rectangle.
/// Operational errors and decisions continue through FloatingNotice/modals.
pub fn management_body_state_overlay(
    ui:&Ui,viewport:Rect,caption:Option<&str>
){
    let Some(caption)=caption else {return};
    let visible=viewport.intersect(ui.clip_rect());
    if visible.width()<=0.0 || visible.height()<=0.0{return;}
    ui.painter().with_clip_rect(visible).text(
        visible.center(),egui::Align2::CENTER_CENTER,caption,
        egui::FontId::proportional(typography::FONT_METADATA),
        theme::MUTED
    );
}



pub fn body_error_overlay(ui:&Ui,viewport:Rect,context:&str,error:&str){
    let visible=viewport.intersect(ui.clip_rect());
    if visible.width()<=0.0||visible.height()<=0.0{return;}
    let display=crate::user_error::display(context,error);
    let painter=ui.painter().with_clip_rect(visible);
    let galley=painter.layout(display.as_ref().to_owned(),
        egui::FontId::proportional(typography::FONT_METADATA),
        theme::DANGER,(visible.width()-2.0*theme::SPACE_8).max(1.0));
    let pos=egui::pos2(
        visible.center().x-galley.size().x/2.0,
        (visible.center().y-galley.size().y/2.0).max(visible.top())
    );
    let clipped=text_exceeds_rect(galley.size(),visible);
    painter.galley(pos,galley,theme::DANGER);
    overflow_region_tooltip(ui,visible,ui.id().with(("body-error-overlay",context)),
        clipped,display.as_ref());
}

/// Allocate a row with an immutable parent footprint. The child can
/// render optional labels, validation messages or actions without changing
/// the next sibling's Rect when its contents appear or disappear.
pub fn fixed_row<R>(
    ui:&mut Ui,size:Vec2,layout:Layout,
    add_contents:impl FnOnce(&mut Ui)->R,
)->(Rect,R){
    let width=size.x.min(ui.available_width()).max(0.0);
    let height=size.y.max(0.0);
    // Allocate the parent footprint exactly once. allocate_ui_with_layout
    // negotiates its response rect with the child's minimum content size,
    // so a 20pt label could silently expand an intended 18pt status slot.
    let (rect,_response)=ui.allocate_exact_size(
        Vec2::new(width,height),Sense::hover()
    );
    // Render in a detached child Ui: its content can grow internally, but
    // it cannot advance the parent's cursor or move the next sibling.
    let mut row=ui.new_child(
        UiBuilder::new().max_rect(rect).layout(layout)
    );
    row.set_clip_rect(rect.intersect(ui.clip_rect()));
    row.spacing_mut().item_spacing.y=0.0;
    let result=add_contents(&mut row);
    (rect,result)
}

/// A result list owns one bounded viewport regardless of its content.
pub fn fixed_result_viewport<R>(
    ui:&mut Ui,id:&str,height:f32,render:impl FnOnce(&mut Ui)->R
)->(Rect,R){
    let width=ui.available_width();
    fixed_row(ui,Vec2::new(width,height),Layout::top_down(Align::Min),|pane|{
        pane.set_min_height(height);
        scroll_area(ScrollAxes::Vertical).id_salt(id)
            .max_height(height).auto_shrink([false,false])
            .show(pane,render).inner
    })
}

#[cfg(test)]
mod result_viewport_invariance_tests{
    use super::*;
    #[test]
    fn empty_pending_and_populated_lists_keep_following_controls_fixed(){
        for width in [480.0,800.0,1200.0]{
            let mut baseline:Option<[(&str,Rect);2]>=None;
            for count in [0,1,30]{
                let ctx=test_context();
                let _=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,800.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let (slot,())=fixed_result_viewport(ui,"results",180.0,|list|{
                            for _ in 0..count{list.label("상품 검색 결과");}
                        });
                        let next=crate::button::action(ui,"다음 조작부",theme::ButtonRole::Standard,true);
                        let current=[("results",slot),("next",next.rect)];
                        if let Some(previous)=baseline.as_ref(){
                            assert_layout_rects_invariant(previous,&current);
                        }else{baseline=Some(current);}
                    });
                });
            }
        }
    }
}

/// One-line status slot for state-dependent text. It always reserves the
/// same width and height; long messages are clipped visually and remain
/// available in the hover tooltip. Use a fixed-height row for action controls.
pub fn fixed_status_slot(
    ui:&mut Ui,width:f32,height:f32,display:Option<&str>,
    tooltip:Option<&str>,color:Color32,
)->Rect{
    let width=width.min(ui.available_width()).max(0.0);
    let (rect,())=fixed_row(
        ui,Vec2::new(width,height),Layout::left_to_right(Align::Center),
        |slot|{
            if let Some(text)=display.filter(|text|!text.trim().is_empty()){
                let display=if color==theme::DANGER{
                    crate::user_error::display("status",text)
                }else{std::borrow::Cow::Borrowed(text)};
                let natural=slot.painter().layout_no_wrap(display.as_ref().to_owned(),
                    egui::FontId::proportional(typography::FONT_METADATA),color);
                let overflow=text_exceeds_rect(natural.size(),slot.available_rect_before_wrap().intersect(slot.clip_rect()));
                let label=typography::status(display.as_ref(),color);
                let response=slot.add(if tooltip.is_some(){
                    truncated_label_for_explicit_tooltip(label)
                }else{truncated_label(label)});
                if let Some(tooltip)=tooltip{
                    let detail=if color==theme::DANGER{
                        crate::user_error::display("status",tooltip)
                    }else{std::borrow::Cow::Borrowed(tooltip)};
                    overflow_tooltip(response,overflow,detail.as_ref());
                }
            }
        }
    );
    rect
}

#[cfg(test)]
mod fixed_layout_slot_contract_tests {
    use super::*;

    #[test]
    fn optional_status_keeps_its_rect_and_the_next_control_fixed(){
        let mut measured=Vec::new();
        for message in [
            None,
            Some("상태 안내문이 표시됩니다."),
            Some("수량 확인 필요: 123건 / 저장 중인 변경사항과 검증 오류가 있어 적용할 수 없습니다."),
            Some("SQLite: database is locked"),
        ]{
            let ctx=test_context();
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(520.0);
                    let status_width=ui.available_width();
                    let expected_gap=ui.spacing().item_spacing.y;
                    let slot=fixed_status_slot(
                        ui,status_width,typography::METADATA_LINE_HEIGHT,
                        message,message,theme::DANGER
                    );
                    let (next,_)=ui.allocate_exact_size(
                        Vec2::new(120.0,theme::ACTION_STANDARD_HEIGHT),Sense::hover()
                    );
                    measured.push((slot,next,next.top()-slot.bottom(),expected_gap));
                });
            });
        }
        for current in measured.iter().skip(1){
            assert_eq!(measured[0].0,current.0,
                "status text must not alter the allocated status Rect");
            assert_eq!(measured[0].1,current.1,
                "status text must not move the following control");
            assert!((current.2-current.3).abs()<0.5,
                "status row must consume exactly one allocation");
        }
        assert!((measured[0].0.height()-typography::METADATA_LINE_HEIGHT).abs()<0.5);
        assert!((measured[0].2-measured[0].3).abs()<0.5,
            "the fixed status row must consume exactly one parent allocation");
    }

    #[test]
    fn body_errors_preserve_reserved_viewport_and_following_control_geometry(){
        for width in [300.0,520.0,800.0]{
            let ctx=test_context();
            let mut baseline:Option<[(&str,Rect);2]>=None;
            for message in [
                None,
                Some("배송 운임을 확인하세요."),
                Some("SQLite: database is locked"),
                Some("배송 운임을 확인하세요.\nSQLite: no such table: core_orders\n상품 수량을 확인하세요."),
                None,
            ]{
                let _=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,480.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let size=Vec2::new(ui.available_width(),typography::METADATA_LINE_HEIGHT);
                        let (body,_)=ui.allocate_exact_size(size,Sense::hover());
                        let cursor=ui.cursor();
                        let min_rect=ui.min_rect();
                        if let Some(error)=message{
                            body_error_overlay(ui,body,"reception.review",error);
                        }
                        assert_eq!(ui.cursor(),cursor,"error overlay must not advance the parent cursor");
                        assert_eq!(ui.min_rect(),min_rect,"error overlay must not enlarge the parent");
                        let (next,_)=ui.allocate_exact_size(
                            Vec2::new(120.0,theme::ACTION_STANDARD_HEIGHT),Sense::hover()
                        );
                        let current=[("reserved body",body),("following control",next)];
                        if let Some(previous)=baseline.as_ref(){
                            assert_layout_rects_invariant(previous,&current);
                        }else{
                            baseline=Some(current);
                        }
                    });
                });
            }
            assert!(baseline.is_some());
        }
    }

    #[test]
    fn optional_action_keeps_its_row_and_the_next_control_fixed(){
        let mut measured=Vec::new();
        for visible in [false,true]{
            let ctx=test_context();
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(520.0);
                    let row_width=ui.available_width();
                    let expected_gap=ui.spacing().item_spacing.y;
                    let (row,_)=fixed_row(
                        ui,Vec2::new(row_width,theme::ACTION_STANDARD_HEIGHT),
                        Layout::left_to_right(Align::Center),|contents|{
                            if visible{crate::button::action(
                                contents,"설정",theme::ButtonRole::Standard,true
                            );}
                        }
                    );
                    let (next,_)=ui.allocate_exact_size(
                        Vec2::new(120.0,theme::ACTION_STANDARD_HEIGHT),Sense::hover()
                    );
                    measured.push((row,next,next.top()-row.bottom(),expected_gap));
                });
            });
        }
        assert_eq!(measured[0].0,measured[1].0);
        assert_eq!(measured[0].1,measured[1].1);
        assert!((measured[0].0.height()-theme::ACTION_STANDARD_HEIGHT).abs()<0.5);
        assert!((measured[0].2-measured[0].3).abs()<0.5,
            "the fixed action row must consume exactly one parent allocation");
    }
}

#[cfg(test)]
mod management_async_layout_contract_tests {
    use super::*;

    #[test]
    fn loading_and_empty_overlays_do_not_advance_parent_cursor(){
        let ctx=egui::Context::default();
        let mut checked=false;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(480.0);
                let body=ui.available_rect_before_wrap();
                let cursor=ui.cursor();
                for caption in [
                    None,Some("자료 조회 중"),Some("검색 결과가 없습니다."),
                    Some("완료"),None
                ]{
                    management_body_state_overlay(
                        ui,Rect::from_min_size(body.min,Vec2::new(300.0,100.0)),
                        caption
                    );
                    assert_eq!(ui.cursor(),cursor,
                        "asynchronous state must not allocate a layout row");
                }
                checked=true;
            });
        });
        assert!(checked);
    }

    #[test]
    fn fixed_list_states_use_shared_overlay(){
        for source in [include_str!("order_history_manager.rs"),include_str!("excel_manager.rs"),
            include_str!("client_manager.rs"),include_str!("client_chooser.rs"),
            include_str!("manual_product_search.rs"),include_str!("model_group_manager.rs")]{
            assert!(source.contains("components::management_body_state_overlay("));
        }
        let history=include_str!("order_history_manager.rs");
        assert!(!history.contains("let galley=typography::management_control_text_galley("));
    }

    #[test]
    fn pending_and_loaded_kpis_consume_identical_height(){
        let mut measured=Vec::new();
        for count in [None,Some(27)]{
            let ctx=egui::Context::default();
            // The production app installs its named emphasis font family
            // before drawing KPI cards. The geometry test must do the same.
            crate::typography::install(&ctx);
            let mut consumed=None;
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(600.0);
                    let start=ui.cursor().top();
                    if let Some(count)=count{
                        management_summary_card(ui,"등록 상품",count);
                    }else{
                        management_summary_card_value(ui,"등록 상품","—");
                    }
                    consumed=Some(ui.cursor().top()-start);
                });
            });
            measured.push(consumed.expect("KPI was laid out"));
        }
        assert_eq!(measured[0],measured[1],
            "placeholder KPI must use exact production card geometry");
    }

    #[test]
    fn pending_and_loaded_kpi_rows_keep_geometry_at_reference_and_narrow_sizes(){
        fn measure(screen:Vec2,pending:bool)->f32{
            let ctx=egui::Context::default();
            crate::typography::install(&ctx);
            let mut consumed=None;
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(Rect::from_min_size(egui::Pos2::ZERO,screen));
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width((screen.x-32.0).max(1.0));
                    let start=ui.cursor().top();
                    ui.horizontal_wrapped(|strip|{
                        strip.spacing_mut().item_spacing=Vec2::new(
                            theme::SPACE_8,theme::SPACE_8
                        );
                        for label in ["등록 상품","모델군","별칭","브랜드"]{
                            if pending{
                                management_summary_card_value(strip,label,"—");
                            }else{
                                management_summary_card(strip,label,27);
                            }
                        }
                    });
                    consumed=Some(ui.cursor().top()-start);
                });
            });
            consumed.expect("KPI row was laid out")
        }
        for screen in [
            Vec2::new(1920.0,1080.0),
            Vec2::new(1366.0,768.0),
            Vec2::new(900.0,650.0)
        ]{
            let pending=measure(screen,true);
            let loaded=measure(screen,false);
            assert!((pending-loaded).abs()<=0.5,
                "KPI row height changed at {screen:?}: pending={pending}, loaded={loaded}");
        }
    }

    #[test]
    fn pending_card_helper_matches_shared_master_shell_at_reference_and_narrow_sizes(){
        fn measure(screen:Vec2,use_pending_helper:bool)->(f32,f32,f32,Rect,f32){
            let ctx=egui::Context::default();
            crate::typography::install(&ctx);
            let mut measured=None;
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(Rect::from_min_size(egui::Pos2::ZERO,screen));
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width((screen.x-32.0).max(1.0));
                    let page_origin_y=ui.cursor().top();
                    management_page_title_top_gap(ui);
                    management_top_content(ui,|top|{
                        top.label(crate::typography::page_heading("상품 관리"));
                    });
                    let title_bottom=ui.cursor().top();
                    ui.add_space(theme::SPACE_8);
                    management_top_content(ui,|top|{
                        management_tabs(top,&[
                            ("상품 목록",true,true),
                            ("카테고리 관리",false,true),
                            ("모델군 관리",false,true),
                            ("별칭 관리",false,true),
                            ("브랜드 관리",false,true)
                        ]);
                    });
                    let tabs_bottom=ui.cursor().top();
                    ui.add_space(theme::SPACE_12);
                    management_top_content(ui,|top|{
                        top.horizontal_wrapped(|strip|{
                            strip.spacing_mut().item_spacing=Vec2::new(
                                theme::SPACE_8,theme::SPACE_8
                            );
                            for label in ["등록 상품","모델군","별칭","브랜드"]{
                                if use_pending_helper{
                                    management_summary_card_value(strip,label,"—");
                                }else{
                                    management_summary_card(strip,label,27);
                                }
                            }
                        });
                    });
                    let kpi_bottom=ui.cursor().top();
                    ui.add_space(theme::SPACE_12);
                    align_management_primary_card(ui,page_origin_y);
                    let card=if use_pending_helper{
                        management_pending_primary_card(ui)
                    }else{
                        let min_height=management_master_table_card_min_inner_height(ui);
                        crate::card::management_master_table_frame().show(ui,|panel|{
                            fill_management_primary_card(panel,min_height);
                        }).response.rect
                    };
                    measured=Some((
                        title_bottom,tabs_bottom,kpi_bottom,card,ui.cursor().top()
                    ));
                });
            });
            measured.expect("primary card was laid out")
        }
        let near=|a:f32,b:f32|(a-b).abs()<=0.5;
        for screen in [
            Vec2::new(1920.0,1080.0),
            Vec2::new(1366.0,768.0),
            Vec2::new(900.0,650.0)
        ]{
            let helper=measure(screen,true);
            let direct=measure(screen,false);
            assert!(near(helper.0,direct.0),"title slot moved at {screen:?}");
            assert!(near(helper.1,direct.1),"tab slot moved at {screen:?}");
            assert!(near(helper.2,direct.2),"KPI slot moved at {screen:?}");
            for (helper_edge,direct_edge) in [
                (helper.3.left(),direct.3.left()),
                (helper.3.top(),direct.3.top()),
                (helper.3.right(),direct.3.right()),
                (helper.3.bottom(),direct.3.bottom()),
                (helper.4,direct.4)
            ]{
                assert!(near(helper_edge,direct_edge),
                    "pending helper Rect differs from shared master shell at {screen:?}: helper={:?}, direct={:?}",
                    helper.3,direct.3);
            }
        }
    }

    #[test]
    fn management_async_renderers_keep_table_slots_invariant(){
        let products=include_str!("product_manager.rs");
        // Source-level assertions inspect the production renderer ONLY:
        // separate test strings may intentionally mention prohibited patterns.
        let rendered=products.split("pub fn show_page(").nth(1).unwrap()
            .split("    fn product_price_candidate(").next().unwrap();
        assert!(rendered.contains("components::management_body_state_overlay("));
        assert!(!rendered.contains("let empty_reserve=if self.products.is_empty()"));
        assert!(!rendered.contains("management_list_strip_frame().show(panel,|empty|"));
        let settlement=include_str!("daily_settlement_manager.rs");
        assert!(settlement.contains("let values=["));
        assert!(settlement.contains("for (name,value) in ["));
        let model_groups=include_str!("model_group_manager.rs");
        assert!(!model_groups.contains("if self.dirty(){\n                                crate::ui_primitives::soft_divider(right);"));
        let aliases=include_str!("alias_manager.rs");
        assert!(aliases.contains("fn alias_diagnostic_slot("));
        assert!(aliases.contains("alias_diagnostic_slot(info,item,&warnings);"));
        assert!(aliases.contains("components::fixed_status_slot("));
        assert!(!aliases.contains("hit.on_hover_text(detail);"));
        let settings=include_str!("settings_view.rs");
        assert!(settings.contains("if conflict{"));
        assert!(settings.contains("heading.with_layout(Layout::right_to_left(Align::Center)"));
        assert!(!settings.contains("if conflict && components::management_top_content"));
    }
}

#[cfg(test)]
mod management_async_behavior_regression_tests {
    #[test]
    fn pending_states_preserve_actions_and_error_channels(){
        let freight=include_str!("freight_manager.rs");
        assert!(freight.contains("crate::card::management_pending_primary_card(ui);"));
        assert!(freight.contains("floating_notice::sync("));
        assert!(freight.contains("gateway.mutate_shipping("));

        let products=include_str!("product_manager.rs");
        assert!(products.contains("components::management_body_state_overlay("));
        assert!(products.contains("self.start_write(\"set_price\""));
        assert!(products.contains("self.start_write(\"upsert_brand\""));
        assert!(products.contains("self.start_write(\"save_alias\""));
        assert!(products.contains("crate::card::management_pending_primary_card(ui);"));

        let settings=include_str!("settings_view.rs");
        assert!(settings.contains("if !self.loaded || self.source.is_null(){"));
        assert!(settings.contains("crate::card::management_pending_primary_card(ui);"));
        assert!(settings.contains("self.save_bar(ui,gateway);"));

        let messages=include_str!("automatic_message_manager.rs");
        assert!(messages.contains("if !self.loaded{"));
        assert!(messages.contains("crate::card::management_pending_primary_card(ui);"));
        assert!(messages.contains("self.start_save("));
        assert!(messages.contains("pub fn show_confirmation("));

        let excel=include_str!("product_excel_ui.rs");
        let menu=excel.split("pub fn show_menu(").nth(1).unwrap()
            .split("pub fn show_review(").next().unwrap();
        assert!(menu.contains("let enabled=!self.busy()&&allowed;"));
        // Product and client workbooks share the same asynchronous menu.
        // Verify the actual dispatch and selected workbook kind, not the
        // retired single-argument product-only signature.
        assert_eq!(menu.matches("let client=self.client;").count(),2);
        assert!(menu.contains("self.start(menu.ctx(),move||export_file(native.as_ref(),client));"));
        assert!(menu.contains("self.start(menu.ctx(),move||preview_file(native.as_ref(),client));"));
    }

    #[test]
    fn async_placeholders_keep_slots_and_manual_edit_evidence(){
        let settlement=include_str!("daily_settlement_manager.rs");
        let values=settlement.split("let values=[").nth(1).unwrap()
            .split("];").next().unwrap();
        assert_eq!(values.matches("unwrap_or_else(||\"—\"").count(),4);
        assert!(settlement.contains(".auto_shrink([false,false])"));
        assert!(!settlement.contains("변경한 결산 날짜를 조회하고 있습니다."));
        assert!(!settlement.contains("결산 자료 확인 중"));

        let model_groups=include_str!("model_group_manager.rs");
        assert!(model_groups.contains("writable&&!busy&&self.dirty()"));
        assert!(model_groups.contains("crate::button::action(footer,\"닫기\""));

        let aliases=include_str!("alias_manager.rs");
        assert!(aliases.contains("fn alias_diagnostic_slot("));
        assert!(aliases.contains("alias_diagnostic_slot(info,item,&warnings);"));
        assert!(aliases.contains("components::fixed_status_slot("));
        assert!(aliases.contains("detail.as_deref(),detail.as_deref(),theme::MUTED"));

        let main=include_str!("main.rs");
        assert!(main.contains("components::reception_client_summary_card_editable("));
        assert!(main.contains("let client_card_height = if has_source"));
        assert!(main.contains("fn apply_manual_event("));
        assert!(main.contains("row.edit_history.push("));
        assert!(main.contains("row.manual_model=Some("));
    }
}

/// Source-contract helper for the repository's top-level test-module style.
/// Preserve production code both before and after each test module; inline
/// geometry probes are not module boundaries. This is a lexical convention,
/// not a Rust parser: a top-level module must close with an unindented brace.
#[cfg(test)]
pub(crate) fn production_source_for_contract(file:&str)->String {
    let mut lines=file.split_inclusive('\n').peekable();
    let mut result=String::with_capacity(file.len());
    while let Some(line)=lines.next(){
        if line.trim_end()=="#[cfg(test)]" &&
            lines.peek().is_some_and(|next|next.starts_with("mod ")){
            let declaration=lines.next().unwrap();
            assert!(declaration.trim_end().ends_with('{'),
                "unsupported source-contract test module declaration: {declaration}");
            assert!(lines.by_ref().any(|line|line.trim_end()=="}"),
                "unterminated source-contract test module: {declaration}");
            continue;
        }
        result.push_str(line);
    }
    result
}

#[cfg(test)]
mod shared_screen_component_contract_tests {
    use super::production_source_for_contract as production_source;

    #[test]
    fn inline_geometry_probe_does_not_truncate_production_source(){
        let source="fn screen(){\n    #[cfg(test)]\n    record_geometry();\n    crate::input::multiline_text_field();\n}\n#[cfg(test)]\nmod tests {\n    fn test_only(){}\n}";
        let production=production_source(source);
        assert!(production.contains("crate::input::multiline_text_field("));
        assert!(!production.contains("fn test_only("));
    }

    #[test]
    fn production_after_multiple_test_modules_remains_visible(){
        let source="fn before(){}\n#[cfg(test)]\nmod first {\n    fn test_one(){}\n}\nfn between(){}\n#[cfg(test)]\nmod second {\n    fn test_two(){}\n}\nfn after(){}\n";
        let production=production_source(source);
        for name in ["fn before(","fn between(","fn after("]{assert!(production.contains(name));}
        assert!(!production.contains("fn test_one("));
        assert!(!production.contains("fn test_two("));
    }

    #[test]
    fn standard_screens_do_not_rebuild_shared_controls(){
        let standard_controls=[
            "Button::new(",
            "Frame::new(",
            "TextEdit::singleline(",
            "TextEdit::multiline(",
            "Checkbox::new(",
            "ComboBox::",
            ".checkbox("
        ];
        let cases=[
            ("floating_notice.rs",include_str!("floating_notice.rs")),
            ("reception_service.rs",include_str!("reception_service.rs")),
            ("review_panel.rs",include_str!("review_panel.rs")),
            ("gateway.rs",include_str!("gateway.rs")),
            ("http_gateway.rs",include_str!("http_gateway.rs")),
            ("preview_store.rs",include_str!("preview_store.rs")),
            ("source_autosave.rs",include_str!("source_autosave.rs")),
            ("clipboard_intake.rs",include_str!("clipboard_intake.rs")),
            ("ocr_learning.rs",include_str!("ocr_learning.rs")),
            ("client_directory.rs",include_str!("client_directory.rs")),
            ("client_master.rs",include_str!("client_master.rs")),
            ("client_manager.rs",include_str!("client_manager.rs")),
            ("client_search_tags.rs",include_str!("client_search_tags.rs")),
            ("automatic_message_manager.rs",include_str!("automatic_message_manager.rs")),
            ("category_manager.rs",include_str!("category_manager.rs")),
            ("category_dropdown.rs",include_str!("category_dropdown.rs")),
            ("alias_manager.rs",include_str!("alias_manager.rs")),
            ("latest_job.rs",include_str!("latest_job.rs")),
            ("brand_manager.rs",include_str!("brand_manager.rs")),
            ("model_group_manager.rs",include_str!("model_group_manager.rs")),
            ("product_manager.rs",include_str!("product_manager.rs")),
            ("product_excel.rs",include_str!("product_excel.rs")),
            ("product_excel_ui.rs",include_str!("product_excel_ui.rs")),
            ("client_chooser.rs",include_str!("client_chooser.rs")),
            ("business_state.rs",include_str!("business_state.rs")),
            ("native_order_change.rs",include_str!("native_order_change.rs")),
            ("pricing_basis.rs",include_str!("pricing_basis.rs")),
            ("reception_jobs.rs",include_str!("reception_jobs.rs")),
            ("source_highlight.rs",include_str!("source_highlight.rs")),
            ("workflow_board.rs",include_str!("workflow_board.rs")),
            ("manual_workspace.rs",include_str!("manual_workspace.rs")),
            ("catalog_query_bridge.rs",include_str!("catalog_query_bridge.rs")),
            ("manual_product_search.rs",include_str!("manual_product_search.rs")),
            ("item_editor.rs",include_str!("item_editor.rs")),
            ("product_images.rs",include_str!("product_images.rs")),
            ("manual_source_view.rs",include_str!("manual_source_view.rs")),
            ("kakao_context.rs",include_str!("kakao_context.rs")),
            ("kakao_dock.rs",include_str!("kakao_dock.rs")),
            ("settings_store.rs",include_str!("settings_store.rs")),
            ("settings_view.rs",include_str!("settings_view.rs")),
            ("freight_manager.rs",include_str!("freight_manager.rs")),
            ("order_history_manager.rs",include_str!("order_history_manager.rs")),
            ("order_history_detail.rs",include_str!("order_history_detail.rs")),
            ("order_history_memo.rs",include_str!("order_history_memo.rs")),
            ("daily_settlement_manager.rs",include_str!("daily_settlement_manager.rs")),
            ("excel_manager.rs",include_str!("excel_manager.rs")),
            ("excel_service.rs",include_str!("excel_service.rs")),
            ("management_queries.rs",include_str!("management_queries.rs")),
        ];
        for (name,file) in cases {
            let source=production_source(file);
            for token in &standard_controls {
                assert!(!source.contains(*token),
                    "{name} bypasses the shared UI layer with {token}");
            }
        }

        // These two files own intentionally unique frame surfaces, but must
        // still route ordinary controls through the shared component layer.
        let special=[
            ("workflow_board_ui.rs",include_str!("workflow_board_ui.rs")),
            ("settings_help.rs",include_str!("settings_help.rs")),
        ];
        let ordinary_controls=[
            "Button::new(","TextEdit::singleline(","TextEdit::multiline(",
            "Checkbox::new(","ComboBox::",".checkbox("
        ];
        for (name,file) in special {
            let source=production_source(file);
            for token in &ordinary_controls {
                assert!(!source.contains(*token),
                    "{name} bypasses the shared UI layer with {token}");
            }
        }

        // main.rs keeps custom selectable source-document rendering and
        // native window chrome; its standard checkboxes are not exceptions.
        let main=include_str!("main.rs");
        assert!(!main.contains("Checkbox::new("));
        assert!(!main.contains(".checkbox("));
    }

    #[test]
    fn migrated_screens_keep_their_shared_component_entrypoints(){
        let excel=production_source(include_str!("product_excel_ui.rs"));
        assert!(excel.contains("crate::table::management_readonly_table("));
        assert!(excel.contains("crate::checkbox::checkbox("));

        let vat=include_str!("main.rs")
            .split("fn show_vat_settings(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(vat.contains("crate::checkbox::checkbox("));
        assert!(vat.contains("crate::checkbox::checkbox_list_row("));
        assert!(vat.contains("crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::RecipientChecklist)"));
        assert!(!vat.contains(".inner_margin("));

        let messages=production_source(include_str!("automatic_message_manager.rs"));
        assert!(messages.contains("crate::input::multiline_text_field("));

        let history=production_source(include_str!("order_history_manager.rs"));
        assert!(history.contains("crate::input::management_date_text_field("));
        assert!(history.contains("crate::input::multiline_text_field("));
    }
}

/// Semantic labels for management table columns. A screen selects the
/// column role; the shared component owns the displayed header wording.
/// This does not change the underlying product description or column size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagementTableColumnLabel {
    ProductName,
}

impl ManagementTableColumnLabel {
    pub const fn text(self) -> &'static str {
        match self {
            Self::ProductName => "상품명",
        }
    }
}

/// Compact workflow money uses the shared semantic font and one-touch
/// geometry. The allocated price slot, rather than its text clip, bounds
/// the highlight so the currency suffix retains the same 4pt clearance.
pub fn workflow_compact_money(
    ui:&Ui,price:Rect,price_ink:Rect,bottom_y:f32,amount:&str,emphasis:bool
)->bool{
    let color=if emphasis{crate::one_touch::INK}else{theme::TEXT};
    let galley=ui.painter().layout_no_wrap(
        amount.to_owned(),typography::data_value_font(
            typography::DataValueKind::Money,typography::DataValueContext::CompactCard
        ),color
    );
    let pos=egui::pos2(price_ink.right()-galley.size().x,bottom_y-galley.size().y);
    let bounds=Rect::from_min_size(pos,galley.size());
    let highlight=crate::one_touch::value_rect(bounds,price);
    if emphasis{crate::one_touch::background(ui,highlight);}
    let clip=price_ink.intersect(ui.clip_rect());
    let clipped=text_exceeds_rect(galley.size(),clip);
    ui.painter().with_clip_rect(clip).galley(pos,galley,color);
    if emphasis{crate::one_touch::outline(ui,highlight);}
    clipped
}

/// Noninteractive retail marker occupies the existing unused action slot.
pub fn workflow_idle_caption(ui:&Ui,rect:Rect,caption:&str){
    let galley=ui.painter().layout_no_wrap(caption.to_owned(),
        egui::FontId::proportional(typography::FONT_METADATA),theme::SECONDARY);
    let x=rect.right()-galley.size().x;
    let _=paint_table_galley(ui,rect,x,galley,theme::SECONDARY);
}

/// Use the existing semantic numeric/body layout for shipment charges.
fn workflow_freight_text_job(text:&str,ink:Color32)->egui::text::LayoutJob{
    let mut parts=Vec::new();
    for (line_index,line) in text.split('\n').enumerate(){
        if line_index>0{parts.push(("\n",None));}
        for (index,part) in line.split(" / ").enumerate(){
            if index>0{parts.push((" / ",None));}
            if let Some(number)=part.strip_suffix("원"){
                parts.push((number,Some(typography::DataValueKind::Money)));
                parts.push(("원",None));
            }else{parts.push((part,None));}
        }
    }
    let mut job=typography::inline_body_values(&parts);
    for section in &mut job.sections{section.format.color=ink;}
    job
}

/// Same shipment detail surface, multiline product renderer and cell
/// coordinates as Order History. Selection only supplies the palette.
pub fn workflow_shipment_row(
    ui:&mut Ui,name:&str,summary:&str,fill:Color32,chosen:bool,previewed:bool,freight:Option<&str>,freight_emphasis:bool
)->egui::Response{
    let outer=ui.available_width();
    let (widths,gap)=workflow_shipment_columns(outer);
    let preview_ink=if chosen{theme::WORKFLOW_SELECTED_ROW_TEXT}
        else if previewed{source_highlight::ACTIVE_TEXT}else{theme::TEXT};
    let summary_ink=if chosen{theme::TEXT}else{preview_ink};
    let freight_ink=if freight_emphasis{crate::one_touch::INK}else{theme::TEXT};
    let name_galley=shipment_detail_product_galley(ui,widths[0],name,preview_ink);
    let product_galley=shipment_detail_product_galley(ui,widths[1],summary,summary_ink);
    let freight_text=freight.unwrap_or("—");
    let mut freight_job=workflow_freight_text_job(&freight_text.replace(" / ","\n"),freight_ink);
    freight_job.wrap.max_width=(widths[2]-2.0*theme::SPACE_8).max(1.0);
    freight_job.wrap.break_anywhere=true;
    let freight_galley=ui.fonts_mut(|fonts|fonts.layout_job(freight_job));
    let height=shipment_detail_block_height(name_galley.size().y.max(product_galley.size().y)
        .max(freight_galley.size().y));
    let (block,response)=ui.allocate_exact_size(Vec2::new(outer,height),Sense::hover());
    let [name_rect,product_rect,freight_rect]=shipment_detail_cells(block,&widths,gap);
    paint_shipment_detail_surface(ui,block,fill,chosen);
    paint_shipment_detail_products(ui,name_rect,name_galley,preview_ink);
    paint_shipment_detail_products(ui,product_rect,product_galley,summary_ink);
    let x=freight_rect.right()-theme::SPACE_8-freight_galley.size().x;
    let bounds=Rect::from_min_size(
        egui::pos2(x,freight_rect.center().y-freight_galley.size().y/2.0),freight_galley.size());
    let highlight=crate::one_touch::value_rect(bounds,freight_rect);
    if freight_emphasis{crate::one_touch::background(ui,highlight);}
    paint_table_galley(ui,freight_rect,x,freight_galley,freight_ink);
    if freight_emphasis{crate::one_touch::outline(ui,highlight);}
    if previewed && !chosen{
        ui.painter().rect_stroke(block.shrink(0.5),theme::ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS,
            egui::Stroke::new(source_highlight::ACTIVE_BORDER_WIDTH,source_highlight::ACTIVE_BORDER),
            egui::StrokeKind::Inside);
    }
    response
}

#[cfg(test)]
mod workflow_freight_typography_tests{
    use super::*;
    #[test]
    fn nested_shipment_header_uses_master_height_and_shared_detail_columns(){
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(390.0);
                let header=management_inline_list_table_header(ui);
                assert_eq!(header.header_rect.height(),theme::MANAGEMENT_TABLE_HEADER_HEIGHT);
                assert_eq!(header.scrollbar_gutter,0.0);
                assert_eq!(header.header_content_rect,header.header_rect);
                let area=crate::table::master_header_begin(ui,header.header_rect,header.header_content_rect);
                let (widths,gap)=workflow_shipment_columns(header.table_width);
                let headings=shipment_detail_cells(area,&widths,gap);
                let block=Rect::from_min_size(
                    egui::pos2(header.header_rect.left(),header.header_rect.bottom()+8.0),
                    Vec2::new(header.table_width,90.0)
                );
                let body=shipment_detail_cells(block,&widths,gap);
                for (heading,cell) in headings.iter().zip(&body){
                    assert_eq!(heading.left(),cell.left());
                    assert_eq!(heading.right(),cell.right());
                }
                assert!((body[2].right()-(block.right()-theme::SPACE_12)).abs()<0.01);
            });
        });
    }
    #[test]
    fn freight_uses_semantic_money_and_stable_layout_in_both_states(){
        let text="4,000원 / —";
        let regular=workflow_freight_text_job(text,theme::TEXT);
        let emphasized=workflow_freight_text_job(text,crate::one_touch::INK);
        assert_eq!(regular.text,text);
        assert_eq!(regular.sections.len(),emphasized.sections.len());
        let money=typography::data_value_font(typography::DataValueKind::Money,typography::DataValueContext::Form);
        assert_eq!(regular.sections[0].format.font_id,money);
        assert_eq!(regular.sections[0].format.line_height,Some(22.0));
        assert_eq!(regular.sections[1].format.font_id.family,egui::FontFamily::Proportional);
        for (a,b) in regular.sections.iter().zip(&emphasized.sections){
            assert_eq!(a.byte_range,b.byte_range);
            assert_eq!(a.format.font_id,b.format.font_id);
            assert_eq!(a.format.line_height,b.format.line_height);
        }
        let cod=workflow_freight_text_job("착불",theme::TEXT);
        assert_eq!(cod.sections[0].format.font_id.family,egui::FontFamily::Proportional);
    }
}

/// Same state precedence in draft bundles and confirmed orders.
pub fn workflow_shipment_hover(ui:&Ui,response:&Response,chosen:bool,previewed:bool){
    crate::floating_notice::remember_response(ui.ctx(),response);
    if !chosen && !previewed{
        crate::button::paint_button_hover(ui,response,theme::CONTROL_RADIUS as f32);
    }
}

#[cfg(test)]
mod common_action_tooltip_tests{
    use super::*;
    #[test]
    fn only_overflowing_text_qualifies_for_a_tooltip(){
        let rect=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(100.0,20.0));
        assert!(!text_exceeds_rect(Vec2::new(80.0,18.0),rect));
        assert!(!text_exceeds_rect(Vec2::new(100.0,20.0),rect));
        assert!(text_exceeds_rect(Vec2::new(101.0,18.0),rect));
        assert!(text_exceeds_rect(Vec2::new(80.0,21.0),rect));
    }

    #[test]
    fn overflow_tooltip_policy_rejects_blank_and_fitting_text(){
        assert!(!overflow_tooltip_needed(false,"상품명"));
        assert!(!overflow_tooltip_needed(true,""));
        assert!(!overflow_tooltip_needed(true,"  "));
        assert!(overflow_tooltip_needed(true,"상품명 전체"));
    }

    #[test]
    fn positioned_overflow_detects_clipping_and_elision(){
        let visible=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(100.0,20.0));
        let fitting=Rect::from_min_size(egui::pos2(4.0,2.0),Vec2::new(90.0,16.0));
        assert!(!positioned_text_overflow(fitting,visible,false));
        assert!(positioned_text_overflow(fitting,visible,true));
        assert!(positioned_text_overflow(fitting.translate(egui::vec2(10.0,0.0)),visible,false));
    }

    #[test]
    fn truncated_labels_have_exactly_one_tooltip_owner(){
        fn full_text_count(shape:&egui::Shape,full:&str)->usize{
            match shape{
                egui::Shape::Text(text)=>usize::from(!text.galley.elided&&text.galley.text()==full),
                egui::Shape::Vec(shapes)=>shapes.iter().map(|shape|full_text_count(shape,full)).sum(),
                _=>0
            }
        }
        let full="BD 950 microphone stand";
        for explicit in [false,true]{
            for width in [48.0,400.0]{
                let ctx=test_context();
                ctx.memory_mut(|memory|memory.set_everything_is_visible(true));
                let mut count=0usize;
                // The tooltip Area measures its size before the visible pass.
                for _ in 0..3{
                    let output=ctx.run(egui::RawInput{
                        screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(800.0,500.0))),
                        ..Default::default()
                    },|ctx|{
                        egui::CentralPanel::default().show(ctx,|ui|{
                            let label=if explicit{
                                truncated_label_for_explicit_tooltip(typography::body(full))
                            }else{truncated_label(typography::body(full))};
                            let response=ui.add_sized(Vec2::new(width,32.0),label);
                            if explicit{overflow_tooltip(response,width<100.0,full);}
                        });
                    });
                    count=output.shapes.iter().map(|shape|full_text_count(&shape.shape,full)).sum();
                }
                // Narrow: one full tooltip, elided label excluded. Wide:
                // one full label, no tooltip. Neither may draw two copies.
                assert_eq!(count,1,"explicit={explicit}, width={width}");
            }
        }
    }

    #[test]
    fn native_label_elision_uses_actual_width_for_reviewed_text_roles(){
        let ctx=test_context();
        let text="반도스탠드 대형 오케스트라 보면대";
        let roles:[(&str,fn(&str)->egui::RichText);4]=[
            ("modal title",typography::modal_title),
            ("modal description",typography::caption),
            ("table cell",typography::table_cell),
            ("body / shipment / folder / warning",typography::body),
        ];
        let _=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1024.0,600.0))),
            ..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                for (role,style) in roles{
                    for (width,expected_elided) in [(60.0,true),(800.0,false)]{
                        ui.allocate_ui_with_layout(
                            Vec2::new(width,36.0),Layout::left_to_right(Align::Center),|slot|{
                                slot.set_width(width);
                                let (_,galley,_)=truncated_label_for_explicit_tooltip(style(text))
                                    .layout_in_ui(slot);
                                assert_eq!(galley.elided,expected_elided,
                                    "{role}: wrong native elision at width {width}");
                                assert_eq!(galley.text(),text,
                                    "{role}: native tooltip must retain the original string");
                            }
                        );
                    }
                }
            });
        });
    }
}





/// Four distinct panes replace the word "Win". This is a static glyph
/// inside an explanatory keyboard illustration, not a clickable icon.
fn paint_windows_key_glyph(painter:&egui::Painter,face:Rect){
    let size=12.5;
    let gap=1.5;
    let pane=(size-gap)*0.5;
    let left=face.center().x-size*0.5;
    let top=face.center().y-size*0.5;
    for row in 0..2{
        for col in 0..2{
            let x=left+col as f32*(pane+gap);
            let y=top+row as f32*(pane+gap);
            painter.rect_filled(
                Rect::from_min_size(egui::pos2(x,y),Vec2::splat(pane)),
                0.5,theme::TEXT
            );
        }
    }
}

/// Low-angle keycap illustration: the bottom side slopes outward toward
/// the viewer. Distinct top face, tapered side, rim and lip highlight make
/// it three-dimensional without click, hover or pressed button styling.
fn reception_keycap(ui:&mut Ui,key:&str){
    let font=egui::FontId::proportional(typography::FONT_FIELD_LABEL);
    let galley=ui.painter().layout_no_wrap(
        if key=="Win"{String::new()}else{key.to_owned()},
        font,theme::TEXT
    );
    let width=if key=="Win"{30.0}else{
        (galley.size().x+2.0*theme::SPACE_8).ceil().max(26.0)
    };
    let height=theme::RECEPTION_KEYCAP_HEIGHT;
    let depth=theme::RECEPTION_KEYCAP_DEPTH;
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(width,height),Sense::hover()
    );
    let radius=theme::RECEPTION_KEYCAP_RADIUS;
    let face=Rect::from_min_max(
        egui::pos2(rect.left()+2.0,rect.top()+1.0),
        egui::pos2(rect.right()-2.0,rect.bottom()-depth)
    );
    let painter=ui.painter();

    // Silhouette and angled lower wall, visible from slightly below.
    painter.rect_filled(rect,radius,theme::RECEPTION_KEYCAP_SIDE);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(face.left(),face.bottom()-0.5),
            egui::pos2(face.right(),face.bottom()-0.5),
            egui::pos2(rect.right()-1.0,rect.bottom()-1.0),
            egui::pos2(rect.left()+1.0,rect.bottom()-1.0)
        ],
        theme::RECEPTION_KEYCAP_SIDE,
        Stroke::NONE
    ));
    painter.rect_stroke(
        rect,radius,Stroke::new(1.0_f32,theme::BORDER),
        egui::StrokeKind::Inside
    );
    // Recessed upper rim and light-catching front face.
    painter.rect_filled(face,radius,theme::RECEPTION_KEYCAP_FACE);
    painter.rect_stroke(
        face,radius,Stroke::new(1.0_f32,theme::RECEPTION_KEYCAP_EDGE),
        egui::StrokeKind::Inside
    );
    painter.line_segment(
        [
            egui::pos2(face.left()+5.0,face.top()+1.5),
            egui::pos2(face.right()-5.0,face.top()+1.5)
        ],
        Stroke::new(1.0_f32,theme::RECEPTION_KEYCAP_HIGHLIGHT)
    );
    painter.line_segment(
        [
            egui::pos2(rect.left()+4.0,rect.bottom()-1.7),
            egui::pos2(rect.right()-4.0,rect.bottom()-1.7)
        ],
        Stroke::new(1.0_f32,theme::RECEPTION_KEYCAP_BOTTOM_HIGHLIGHT)
    );
    if key=="Win"{
        paint_windows_key_glyph(painter,face);
    }else{
        painter.galley(
            egui::pos2(
                (face.center().x-galley.size().x*0.5).round(),
                (face.center().y-galley.size().y*0.5).round()
            ),
            galley,theme::TEXT
        );
    }
}

/// Reuse the same 4pt key grouping on every reception layout. Unlike plain
/// prose, each shortcut key has its own bordered background and hit-free box.
pub fn reception_capture_shortcut_hint(ui:&mut Ui){
    ui.horizontal_wrapped(|line|{
        line.spacing_mut().item_spacing=egui::vec2(
            theme::SPACE_4,theme::SPACE_4
        );
        line.label(typography::caption("이미지 캡처:"));
        reception_keycap(line,"Win");
        line.label(typography::caption("+"));
        reception_keycap(line,"Shift");
        line.label(typography::caption("+"));
        reception_keycap(line,"S");
        line.label(typography::caption("또는"));
        reception_keycap(line,"Print Screen");
    });
}


/// The fixed goods viewport owns its allocated rectangle. Content may
/// scroll, but an egui child reporting a larger min_rect must not grow
/// the surrounding card, overlap its footer, or shift sibling sections.
/// Reserve the parent's slot once and render the scroll area independently.
pub fn reception_goods_viewport(
    ui:&mut Ui,height:f32,content:impl FnOnce(&mut Ui)
){
    let size=Vec2::new(ui.available_width().max(0.0),height.max(0.0));
    let (rect,_)=ui.allocate_exact_size(size,Sense::hover());
    if !rect.is_positive(){return;}
    let mut child=ui.new_child(UiBuilder::new()
        .id_salt("reception-goods-viewport")
        .max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.set_width(rect.width());
    child.set_height(rect.height());
    child.spacing_mut().item_spacing.y=0.0;
    content(&mut child);
}

/// Docked reception goods own their fixed viewport; the manual worksheet
/// scrolls its complete card stack instead. Both use the same item table
/// spacing and minimum-width rules without introducing another child ID.
pub fn reception_items_body(ui:&mut Ui,width:f32,exact_width:bool){
    if exact_width{ui.set_width(width);}else{ui.set_min_width(width);}
    ui.spacing_mut().item_spacing.y=0.0;
}

pub fn reception_worksheet_items_body(ui:&mut Ui){
    let width=ui.available_width().max(
        theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+theme::RECEPTION_ITEM_REMOVE_WIDTH
    );
    reception_items_body(ui,width,false);
}

/// Preserve the established 12pt worksheet stack gap in one semantic rule.
/// Docked panes retain their independent draggable splitter instead.
pub const RECEPTION_STACKED_CARD_GAP:f32=theme::SPACE_12;

pub fn reception_docked_goods_scroll(
    ui:&mut Ui,section_height:f32,id_salt:impl std::hash::Hash,
    editable_columns:bool,content:impl FnOnce(&mut Ui)
){
    ui.spacing_mut().item_spacing.y=0.0;
    ui.spacing_mut().scroll=crate::scrollbar::management_table_scroll_style();
    let height=docked_section_body_height(section_height)
        .min(ui.available_height().max(0.0));
    reception_goods_viewport(ui,height,|area|{
        let minimum=theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+
            if editable_columns{theme::RECEPTION_ITEM_REMOVE_WIDTH}else{0.0};
        // Reserve the rail even for short lists so overflow never shifts columns.
        let width=management_table_data_width(
            area.available_width(),area.spacing().scroll.allocated_width(),minimum
        );
        scroll_area(ScrollAxes::Both)
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .id_salt(id_salt).max_height(height).auto_shrink([false,false])
            .show(area,|items|{
                reception_items_body(items,width,true);
                content(items);
            });
    });
}

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


/// Clamp the selected recipient and expose navigation availability separately
/// from header visibility. A single recipient still has a visible 1/1 header;
/// only its previous/next controls are disabled.
fn reception_recipient_navigation_state(
    index:usize,total:usize,navigation_enabled:bool
)->Option<(usize,bool,bool)>{
    if total==0{return None;}
    let current=index.min(total-1);
    Some((
        current,
        navigation_enabled && current>0,
        navigation_enabled && current+1<total
    ))
}

#[cfg(test)]
mod reception_recipient_navigation_state_tests{
    use super::*;

    #[test]
    fn one_recipient_keeps_name_and_progress_with_both_arrows_disabled(){
        assert_eq!(
            reception_recipient_navigation_state(0,1,true),
            Some((0,false,false))
        );
    }

    #[test]
    fn multiple_recipients_enable_only_the_available_direction(){
        assert_eq!(
            reception_recipient_navigation_state(0,2,true),
            Some((0,false,true))
        );
        assert_eq!(
            reception_recipient_navigation_state(1,2,true),
            Some((1,true,false))
        );
    }

    #[test]
    fn busy_or_pending_work_disables_only_the_arrows(){
        assert_eq!(
            reception_recipient_navigation_state(0,1,false),
            Some((0,false,false))
        );
        assert_eq!(
            reception_recipient_navigation_state(0,3,false),
            Some((0,false,false))
        );
        assert_eq!(
            reception_recipient_navigation_state(1,3,false),
            Some((1,false,false))
        );
    }

    #[test]
    fn empty_recipient_list_has_no_navigator(){
        assert_eq!(reception_recipient_navigation_state(0,0,true),None);
    }
}

/// Navigate one recipient from the 34pt section HEADER. The name and 1/N
/// progress remain visible for one shipment and while work is pending; only
/// the two directional segments become disabled.
pub fn reception_recipient_navigator(
    ui:&mut Ui,name:&str,index:usize,total:usize,navigation_enabled:bool
)->Option<usize>{
    let (current,previous_enabled,next_enabled)=
        reception_recipient_navigation_state(index,total,navigation_enabled)?;
    let wanted=theme::RECEPTION_RECIPIENT_NAV_WIDTH;
    let width=ui.available_width().min(wanted).max(0.0);
    if width<2.0*theme::RECEPTION_RECIPIENT_NAV_ARROW_WIDTH
        +theme::RECEPTION_RECIPIENT_NAV_COUNT_WIDTH
        +2.0*theme::FRAME_STROKE_WIDTH
    {return None;}
    let height=theme::SECTION_HEADER_HEIGHT;
    let arrow_width=theme::RECEPTION_RECIPIENT_NAV_ARROW_WIDTH;
    let count_width=theme::RECEPTION_RECIPIENT_NAV_COUNT_WIDTH;
    let name_width=(width-2.0*theme::FRAME_STROKE_WIDTH
        -2.0*arrow_width-count_width).max(0.0);
    let (_,selected)=date_navigation_segmented_surface(
        ui,"reception-recipient-nav-header",Vec2::new(width,height),
        |segments,inner|{
            let mut selected=None;
            segments.set_width(inner.width());
            segments.set_height(inner.height());
            segments.spacing_mut().item_spacing=Vec2::ZERO;

            let previous=date_navigation_segment_button(
                segments,arrow_width,previous_enabled,
                DateNavigationSegmentGlyph::Chevron(
                    ManagementChevronDirection::Left
                )
            );
            if previous.clicked(){selected=Some(current-1);}

            let fallback=format!("수취인 {}",current+1);
            let display=if name.trim().is_empty(){fallback.as_str()}else{name};
            let natural_width=segments.painter().layout_no_wrap(
                display.to_owned(),
                egui::FontId::proportional(typography::FONT_BODY),
                theme::TEXT
            ).size().x;
            let content_width=(name_width-2.0*theme::SPACE_8).max(0.0);
            let clipped=natural_width>content_width;
            let label=segments.add_sized(
                Vec2::new(name_width,inner.height()),
                truncated_label_for_explicit_tooltip(typography::body(display))
                    .halign(Align::Center)
                    .sense(if clipped{Sense::click()}else{Sense::hover()})
            );
            let label=overflow_tooltip(label,clipped,display);
            if clipped{
                crate::overlay::show_menu(&label,None,|popup|{
            popup.set_max_width(320.0);
                        popup.add(egui::Label::new(typography::body(display)).wrap());
                    });
            }

            let next=date_navigation_segment_button(
                segments,arrow_width,next_enabled,
                DateNavigationSegmentGlyph::Chevron(
                    ManagementChevronDirection::Right
                )
            );
            if next.clicked(){selected=Some(current+1);}

            segments.add_sized(
                Vec2::new(count_width,inner.height()),
                egui::Label::new(typography::field_label(
                    &format!("{}/{}",current+1,total)
                )).halign(Align::Center)
            );
            paint_navigation_vertical_rules(segments,inner,[
                inner.left()+arrow_width,
                inner.left()+arrow_width+name_width,
                inner.left()+2.0*arrow_width+name_width
            ],theme::DIVIDER);
            selected
        }
    );
    selected
}




/// Draw BEFORE TextEdit renders its glyphs: egui's normal TextEdit background
/// is disabled only for linked fields, so the same fill/radius used by
/// source evidence and item cells remains visible without obscuring text.
fn reception_recipient_linked_background(
    ui:&Ui,width:f32,active:bool
){
    if !active {return;}
    let rect=Rect::from_min_size(
        ui.cursor().min,
        Vec2::new(width,theme::INPUT_HEIGHT)
    );
    source_highlight::paint_active_background(ui,rect);
}


/// Every recipient field shares one 36pt baseline. The two telephone
/// editors occupy the same FULL-WIDTH row: mobile begins at the same x
/// coordinate as the other text inputs, and the landline fills the right.
/// The minimum widths preserve Korean telephone numbers; extra space is
/// distributed to BOTH editors rather than left empty. The two labels and
/// editors are siblings in one horizontal layout (no nested line offsets).
/// Only the five actual input controls are hover/highlight targets.
/// Static Korean field labels remain in their normal theme color.
pub fn reception_recipient_fields(
    ui: &mut Ui,
    name: &mut String,
    address: &mut String,
    mobile: &mut String,
    phone: &mut String,
    note: &mut String,
    active: [bool; 5],
) -> [Response; 5] {

    fn label_cell(ui: &mut Ui, text: &str, width: f32) -> Response {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, theme::INPUT_HEIGHT), Sense::hover(),
        );
        let galley = typography::field_label_galley(ui, text);
        ui.painter().with_clip_rect(rect).galley(
            egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley, theme::SECONDARY,
        );
        response
    }

    fn row(ui: &mut Ui, label: &str, value: &mut String, active: bool,index:usize) -> Response {
        ui.horizontal(|line| {
            line.spacing_mut().item_spacing.x = theme::RECIPIENT_LABEL_GAP;
            label_cell(line, label, theme::RECIPIENT_LABEL_WIDTH);
            let width = line.available_width().max(1.0);
            reception_recipient_linked_background(line,width,active);
            let orange=!active && crate::one_touch::recipient(line,index);
            if orange{crate::one_touch::background(line,Rect::from_min_size(line.cursor().min,Vec2::new(width,theme::INPUT_HEIGHT)));}
            // The response belongs to the input, NOT the combined row.
            // This is read-only while retaining hover and native text layout.
            let edit=reception_recipient_text_edit(value,width,active,false);
            let edit=if orange{edit.frame(false).text_color(crate::one_touch::INK)}else{edit};
            let display=allocate_singleline_field(
                line,Vec2::new(width, theme::INPUT_HEIGHT),edit.interactive(false),None,
            );
            let hit=reception_recipient_value_hit(line,&display);
            source_highlight::paint_active_if(line,&hit,active);
            if orange{crate::one_touch::outline(line,hit.rect);}
            hit
        }).inner
    }

    let responses=ui.scope(|fields| {
        fields.spacing_mut().item_spacing.y = theme::RECIPIENT_ROW_GAP;
        let name_response = row(fields, "이름", name, active[0],0);
        let address_response = row(fields, "주소", address, active[1],1);

        let row_width = fields.available_width()
            .max(theme::RECIPIENT_PHONE_ROW_MIN_WIDTH);
        let input_budget = row_width
            - theme::RECIPIENT_LABEL_WIDTH
            - 2.0 * theme::RECIPIENT_LABEL_GAP
            - theme::RECIPIENT_PHONE_COLUMN_GAP
            - theme::RECIPIENT_PHONE_LABEL_WIDTH;
        // At the reference pane width both fields grow and use all space.
        // At an unusually narrow pane the contact row alone can scroll.
        let mobile_width = (input_budget * theme::RECIPIENT_MOBILE_WIDTH_SHARE)
            .clamp(
                theme::RECIPIENT_MOBILE_INPUT_WIDTH,
                input_budget - theme::RECIPIENT_PHONE_INPUT_WIDTH,
            );
        let phone_width = input_budget - mobile_width;
        let (mobile_response, phone_response) = scroll_area(ScrollAxes::Horizontal)
            .id_salt("recipient-contact-row")
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
            .auto_shrink([false, true])
            .show(fields, |contacts| {
                contacts.set_min_width(row_width);
                contacts.horizontal(|line| {
                    line.spacing_mut().item_spacing.y = 0.0;
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_LABEL_GAP;
                    label_cell(line, "휴대폰", theme::RECIPIENT_LABEL_WIDTH);
                    reception_recipient_linked_background(
                        line,mobile_width,active[2]
                    );
                    let orange=!active[2] && crate::one_touch::recipient(line,2);
                    if orange{crate::one_touch::background(line,Rect::from_min_size(line.cursor().min,Vec2::new(mobile_width,theme::INPUT_HEIGHT)));}
                    let edit=reception_recipient_text_edit(mobile,mobile_width,active[2],true);
                    let edit=if orange{edit.frame(false).text_color(crate::one_touch::INK)}else{edit};
                    let mobile_display=allocate_singleline_field(
                        line,Vec2::new(mobile_width,theme::INPUT_HEIGHT),edit.interactive(false),None,
                    );
                    let mobile_input=reception_recipient_value_hit(line,&mobile_display);
                    source_highlight::paint_active_if(line,&mobile_input,active[2]);
                    if orange{crate::one_touch::outline(line,mobile_input.rect);}
                    // Only the gap between the TWO contact groups is 8pt.
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_PHONE_COLUMN_GAP;
                    label_cell(line, "전화", theme::RECIPIENT_PHONE_LABEL_WIDTH);
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_LABEL_GAP;
                    reception_recipient_linked_background(
                        line,phone_width,active[3]
                    );
                    let orange=!active[3] && crate::one_touch::recipient(line,3);
                    if orange{crate::one_touch::background(line,Rect::from_min_size(line.cursor().min,Vec2::new(phone_width,theme::INPUT_HEIGHT)));}
                    let edit=reception_recipient_text_edit(phone,phone_width,active[3],true);
                    let edit=if orange{edit.frame(false).text_color(crate::one_touch::INK)}else{edit};
                    let phone_display=allocate_singleline_field(
                        line,Vec2::new(phone_width,theme::INPUT_HEIGHT),edit.interactive(false),None,
                    );
                    let phone_input=reception_recipient_value_hit(line,&phone_display);
                    source_highlight::paint_active_if(line,&phone_input,active[3]);
                    if orange{crate::one_touch::outline(line,phone_input.rect);}
                    (mobile_input,phone_input)
                }).inner
            }).inner;
        let note_response = row(fields, "메시지", note, active[4],4);
        [
            name_response, address_response,
            mobile_response, phone_response, note_response,
        ]
    }).inner;
    #[cfg(test)]
    for (name,response) in ["recipient name","recipient address","recipient mobile","recipient phone","recipient note"].into_iter().zip(responses.iter()){
        crate::ui_test_support::record(name,response.rect);
    }
    crate::one_touch::reveal_target(ui,crate::one_touch::Stage::Review,responses[0].rect);
    responses
}



/// Untyped recognition/workflow notices retain informational coloring.
pub fn reception_review_status(ui:&mut Ui,message:&str){
    let tone=theme::ReviewTone::Info;
    let display=crate::user_error::display("reception.review",message);
    crate::card::review_issue_card(ui,tone,|content|{
        content.add(egui::Label::new(typography::body(display.as_ref())
            .color(tone.text_color())).wrap());
    });
}

/// The message editor/footer uses a 12pt spacer with a 1pt centered divider.
/// The caller subtracts the SAME token before sizing the editable canvas.
pub fn reception_message_footer_gap(ui:&mut Ui){
    let (gap_rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::RECEPTION_MESSAGE_ACTION_GAP),Sense::hover()
    );
    let y=gap_rect.center().y;
    ui.painter().line_segment(
        [egui::pos2(gap_rect.left(),y),egui::pos2(gap_rect.right(),y)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
}

/// One shared action layout for preview and working states. Button clicks
/// return the requested Rust copy variant (false=unchanged, true=greeting);
/// the caller alone owns clipboard writes and workflow-safe enablement.
pub fn reception_message_copy_actions(ui:&mut Ui,enabled:bool)->Option<bool>{
    let mut greeting=None;
    let button=|actions:&mut Ui,label:&str|{
        let response=crate::button::action(actions,label,theme::ButtonRole::Standard,enabled);
        if enabled && crate::one_touch::stage(actions)==crate::one_touch::Stage::PaymentCopy{
            crate::one_touch::mark(actions,response.rect);
        }
        response.clicked()
    };
    if ui.available_width()<280.0 {
        ui.with_layout(Layout::top_down(Align::Max),|actions|{
            actions.spacing_mut().item_spacing.y=theme::SPACE_8;
            if button(actions,"그대로 복사"){greeting=Some(false);}
            if button(actions,"인사말 포함 복사"){greeting=Some(true);}
        });
    }else{
        ui.with_layout(Layout::right_to_left(Align::Center),|actions|{
            actions.spacing_mut().item_spacing.x=theme::SPACE_8;
            if button(actions,"인사말 포함 복사"){greeting=Some(true);}
            if button(actions,"그대로 복사"){greeting=Some(false);}
        });
    }
    greeting
}

/// The 6pt split interaction region is always the workspace (CANVAS)
/// between PANEL cards, including the Kakao reservation boundary.
/// Only hover/drag shows an axis cue; drag targets and saved sizing persist.
pub fn paint_dock_splitter(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
    paint_dock_axis_line(ui, rect, vertical, active);
}

/// Pixel-exact axis line within a gutter rectangle. A 6-*physical*-pixel
/// gutter has a center between two pixels, so its normal line must occupy
/// the middle TWO pixels. A one-pixel line can only sit to one side.
/// Odd-width gutters use a single centered pixel. Hover uses the shared
/// 2pt token, adjusted only when necessary for symmetrical pixel margins.
pub fn paint_dock_axis_line(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    // No permanent 1-2pt splitter bar: section/card strokes are enough.
    // A thin resize cue appears only when the pointer is over its hit zone.
    if !active { return; }
    let scale = ui.ctx().pixels_per_point().max(1.0);
    let (low, high) = if vertical {
        (rect.left(), rect.right())
    } else {
        (rect.top(), rect.bottom())
    };
    let start_px = (low * scale).round() as i32;
    let end_px = (high * scale).round() as i32;
    let span_px = (end_px - start_px).max(1);
    // Honor the 1pt / 2pt design tokens where they can be truly centered.
    // For a 6px gutter, a requested 1px rule must expand to the middle 2px.
    let logical_width = if active {
        theme::DOCK_SPLITTER_HOVER_LINE
    } else {
        theme::DOCK_SPLITTER_LINE
    };
    let mut line_width_px = ((logical_width * scale).round() as i32)
        .clamp(1, span_px);
    if (span_px - line_width_px) % 2 != 0 {
        // Match slot-width parity so left/right margins are always equal.
        line_width_px += if line_width_px < span_px { 1 } else { -1 };
    }
    let line_start = start_px + (span_px - line_width_px) / 2;
    let line_end = line_start + line_width_px;

    let snapped = |n: f32| (n * scale).round() / scale;
    let line = if vertical {
        Rect::from_min_max(
            egui::pos2(line_start as f32 / scale, snapped(rect.top())),
            egui::pos2(line_end as f32 / scale, snapped(rect.bottom())),
        )
    } else {
        Rect::from_min_max(
            egui::pos2(snapped(rect.left()), line_start as f32 / scale),
            egui::pos2(snapped(rect.right()), line_end as f32 / scale),
        )
    };
    ui.painter().rect_filled(
        line, 0.0,
        if active { theme::BORDER } else { theme::DIVIDER },
    );
}

/// Repaint the complete reception splitter topology in ONE parent layer.
/// Bridges and junctions use the SAME measured pane-divider coordinates.
/// Painting only the bridge's left junction left a visible gap where its
/// right endpoint met a vertical bar at fractional DPI scaling.
pub fn paint_dock_network(
    ui: &Ui,
    verticals: &[(Rect, bool)],
    bridges: &[(usize, usize, f32)],
) {
    // Paint each horizontal bridge only between the CENTER AXES of its
    // two vertical boundaries. Full-width junction squares used to extend
    // the horizontal rule past the vertical line and create protruding tips.
    for &(left, right, center_y) in bridges {
        if left >= verticals.len() || right >= verticals.len() { continue; }
        let x_start = verticals[left].0.center().x;
        let x_end = verticals[right].0.center().x;
        let area = Rect::from_min_max(
            egui::pos2(x_start, center_y - crate::layout::SPLITTER / 2.0),
            egui::pos2(x_end, center_y + crate::layout::SPLITTER / 2.0),
        );
        paint_dock_axis_line(ui, area, false, false);
    }
    // Draw the continuous vertical rules last. This seals T joints with
    // snapped pixels without extending the horizontal tips past the axis.
    for &(rect, highlighted) in verticals {
        paint_dock_axis_line(ui, rect, true, highlighted);
    }
}

/// Shared date-picker calendar from the approved Order History screen.
/// Use the SAME popup, month navigation, selected-day style and 7-column
/// grid for all management date selectors; each caller supplies its own ID.
pub fn calendar_month_start(date: NaiveDate)->NaiveDate{
    NaiveDate::from_ymd_opt(date.year(),date.month(),1).unwrap_or(date)
}
pub fn shift_calendar_month(month:NaiveDate,offset:i32)->NaiveDate{
    let first=calendar_month_start(month);
    if offset>=0{
        first.checked_add_months(Months::new(offset as u32))
    }else{
        first.checked_sub_months(Months::new(offset.unsigned_abs()))
    }.unwrap_or(first)
}
pub fn calendar_dates(month:NaiveDate)->Vec<Option<NaiveDate>>{
    let first=calendar_month_start(month);
    let leading=first.weekday().num_days_from_monday() as usize;
    let next=shift_calendar_month(first,1);
    let days=next.pred_opt().map_or(31,|end|end.day() as usize);
    let visible=((leading+days+6)/7)*7;
    (0..visible).map(|index|{
        (index>=leading && index<leading+days)
            .then(||first.checked_add_signed(Days::days((index-leading) as i64)))
            .flatten()
    }).collect()
}
/// One 36pt allocation per date group. All children share the 34pt inset
/// and cannot extend hover painting into the outer rounded frame.
pub fn management_date_group<R>(
    ui:&mut Ui,id:&'static str,width:f32,
    add_contents:impl FnOnce(&mut Ui,Rect)->R
)->R{
    date_navigation_segmented_surface(
        ui,("management-date-group",id),Vec2::new(width,theme::INPUT_HEIGHT),
        add_contents
    ).1
}

fn paint_navigation_vertical_rules(
    ui:&Ui,inner:Rect,positions:impl IntoIterator<Item=f32>,color:Color32
){
    for x in positions{
        ui.painter().line_segment(
            [egui::pos2(x,inner.top()+4.0),egui::pos2(x,inner.bottom()-4.0)],
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,color)
        );
    }
}

fn date_navigation_count_step_state(
    value:&str,current:usize,enabled:bool,minimum:usize,maximum:usize
)->(bool,bool){
    let step_count=value.parse::<usize>().unwrap_or(current);
    (
        enabled && step_count>minimum,
        enabled && step_count<maximum
    )
}

/// The editable delivery count is one segmented date-selector control:
/// minus | centered numeric editor | plus. The parent frame owns one border.
pub struct DateNavigationCountControlResponse{
    pub decrement_clicked:bool,
    pub increment_clicked:bool,
    pub input:Response,
    #[cfg(test)]
    pub rect:Rect,
    #[cfg(test)]
    decrement_rect:Rect,
    #[cfg(test)]
    increment_rect:Rect,
}

pub fn date_navigation_count_control(
    ui:&mut Ui,id_salt:impl std::hash::Hash,value:&mut String,
    current:usize,enabled:bool,minimum:usize,maximum:usize
)->DateNavigationCountControlResponse{
    let enabled=enabled && ui.is_enabled();
    let (decrement_enabled,increment_enabled)=date_navigation_count_step_state(
        value,current,enabled,minimum,maximum
    );
    let arrow_width=theme::CALENDAR_NAV_SIZE;
    let input_width=64.0;
    // The inline date group (not the small popup calendar buttons) is the
    // master: 36pt outer height and 34pt interior.
    let height=theme::INPUT_HEIGHT;
    let width=2.0*arrow_width+input_width+2.0*theme::FRAME_STROKE_WIDTH;
    let (_rect,(decrement,input,increment))=
        date_navigation_segmented_surface(
            ui,id_salt,Vec2::new(width,height),|segments,inner|{
                let x1=inner.left()+arrow_width;
                let x2=inner.right()-arrow_width;
                let left=Rect::from_min_max(inner.min,egui::pos2(x1,inner.bottom()));
                let center=Rect::from_min_max(egui::pos2(x1,inner.top()),egui::pos2(x2,inner.bottom()));
                let right=Rect::from_min_max(egui::pos2(x2,inner.top()),inner.max);
                // Every segment gets its own fixed rectangle and ID. TextEdit
                // margins or a disabled button cannot shift the next segment.
                let mut minus=navigation_segment_child(segments,"count-minus",left,Layout::top_down(Align::Center));
                let decrement=date_navigation_segment_button(
                    &mut minus,arrow_width,
                    decrement_enabled,DateNavigationSegmentGlyph::CountStep{increase:false}
                );
                let mut entry=navigation_segment_child(segments,"count-value",center,Layout::centered_and_justified(egui::Direction::TopDown));
                entry.spacing_mut().interact_size=Vec2::ZERO;
                let hint=format!("{minimum}~{maximum}");
                let input=entry.add_enabled_ui(enabled,|entry|{
                    management_date_text_field_impl(
                        entry,value,center.width(),center.height(),hint.as_str(),Some(3)
                    )
                }).inner;
                let mut plus=navigation_segment_child(segments,"count-plus",right,Layout::top_down(Align::Center));
                let increment=date_navigation_segment_button(
                    &mut plus,arrow_width,
                    increment_enabled,DateNavigationSegmentGlyph::CountStep{increase:true}
                );
                paint_navigation_vertical_rules(segments,inner,[
                    inner.left()+arrow_width,
                    inner.left()+arrow_width+input_width
                ],theme::BORDER);
                (decrement,input,increment)
            }
        );
    DateNavigationCountControlResponse{
        decrement_clicked:decrement.clicked(),input,increment_clicked:increment.clicked(),
        #[cfg(test)]
        rect:_rect,
        #[cfg(test)]
        decrement_rect:decrement.rect,
        #[cfg(test)]
        increment_rect:increment.rect,
    }
}

/// Shared calendar navigation: identical 28pt hit targets, border, radius,
/// and vector chevrons for year/month movement in either direction.
fn calendar_nav_button(
    ui:&mut Ui,direction:ManagementChevronDirection,
    year:bool)->bool{
    let (response,ink)=date_navigation_button_surface(ui,true);
    let painter=ui.painter().with_clip_rect(response.rect);
    if year{
        for offset in [-3.8,3.8]{
            paint_management_chevron_with_color(
                &painter,response.rect.center()+egui::vec2(offset,0.0),
                direction,ink
            );
        }
    }else{
        paint_management_chevron_with_color(
            &painter,response.rect.center(),direction,ink
        );
    }
    response.clicked()
}

/// A single popup for Order History and Excel export history. The header
/// has the exact width of the seven-column date grid, so the month label
/// and four navigation buttons remain aligned at every month length.
pub fn date_calendar(
    trigger:&egui::Response,month:&mut NaiveDate,selected:&str,
    grid_id:&'static str
)->Option<NaiveDate>{
    let mut chosen=None;
    let selected_date=NaiveDate::parse_from_str(selected,"%Y-%m-%d").ok();
    let today=(chrono::Utc::now()+Days::hours(9)).date_naive();
    crate::overlay::show_menu(trigger,None,|calendar|{
            calendar.spacing_mut().item_spacing=egui::vec2(
                theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
            );
            calendar.set_min_width(theme::CALENDAR_GRID_WIDTH);
            calendar.horizontal(|nav|{
                nav.spacing_mut().item_spacing.x=theme::CALENDAR_GRID_GAP;
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,true){
                    *month=shift_calendar_month(*month,-12);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,false){
                    *month=shift_calendar_month(*month,-1);
                }
                let title_width=theme::CALENDAR_GRID_WIDTH
                    -4.0*theme::CALENDAR_NAV_SIZE
                    -4.0*theme::CALENDAR_GRID_GAP;
                nav.add_sized(
                    egui::vec2(title_width,theme::CALENDAR_NAV_SIZE),
                    egui::Label::new(typography::field_label(&format!(
                        "{}년 {}월",month.year(),month.month()
                    ))).halign(Align::Center)
                );
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,false){
                    *month=shift_calendar_month(*month,1);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,true){
                    *month=shift_calendar_month(*month,12);
                }
            });
            calendar.add_space(theme::SPACE_8);
            calendar.separator();
            calendar.add_space(theme::SPACE_4);
            let width=theme::CALENDAR_DAY_SIZE;
            let dates=calendar_dates(*month);
            egui::Grid::new(grid_id)
                .num_columns(7)
                .spacing(egui::vec2(
                    theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
                ))
                .show(calendar,|grid|{
                    for day in ["월","화","수","목","금","토","일"]{
                        grid.add_sized(
                            egui::vec2(width,width),
                            egui::Label::new(typography::caption(day))
                                .halign(Align::Center),
                        );
                    }
                    grid.end_row();
                    for (index,day) in dates.iter().enumerate(){
                        if let Some(date)=day{
                            let response=crate::button::calendar_day(grid,&date.day().to_string(),width,selected_date==Some(*date),*date==today);
                            if response.clicked(){
                                chosen=Some(*date);
                                grid.close();
                            }
                        }else{
                            grid.allocate_space(egui::vec2(width,width));
                        }
                        if index%7==6{grid.end_row();}
                    }
                });
        });
    chosen
}

/// Approved 59th-round Freight page heading: 12pt from the top of the
/// workspace. The workspace shell already supplies a 6pt outer gutter, so
/// only the difference is added. Reception must not call this helper.
pub fn management_page_title_top_gap(ui:&mut Ui) {
    ui.add_space((theme::SPACE_12-theme::PANEL_GAP).max(0.0));
}

/// One horizontal guide for a management heading, its optional tabs,
/// top-right metadata and optional KPI strip. This affects ONLY the top
/// content; primary card outer borders keep the established full width.
pub fn management_top_content<R>(
    ui:&mut Ui, add:impl FnOnce(&mut Ui)->R
)->R{
    let output=Frame::new()
        .inner_margin(egui::Margin::symmetric(theme::SECTION_PADDING as i8,0))
        .show(ui,|inset|{
            inset.set_min_width(inset.available_width());
            add(inset)
        });
    #[cfg(test)]
    crate::ui_test_support::record("top content",output.response.rect);
    output.inner
}

/// C-approved dual-master toolbar alignment contract:
/// right-aligned search/date/bulk controls end at the PRIMARY CARD'S inner
/// right edge. The scrollbar gutter belongs only to table headers and rows.
/// Slot height/vertical stacking may differ by screen, but not its right edge.
/// Returns egui logical-point coordinates; convert to device pixels with
/// Context::pixels_per_point() when reporting runtime measurements.
pub fn management_toolbar_trailing_rect(
    toolbar:Rect,desired_width:f32,top_offset:f32,height:f32
)->Rect {
    debug_assert!(desired_width.is_finite() && desired_width>=0.0);
    debug_assert!(top_offset.is_finite() && top_offset>=0.0);
    debug_assert!(height.is_finite() && height>=0.0);
    debug_assert!(top_offset+height<=toolbar.height()+0.01,
        "trailing controls must stay within the reserved toolbar");
    let width=desired_width.min(toolbar.width()).max(0.0);
    let rect=Rect::from_min_size(
        egui::pos2(toolbar.right()-width,toolbar.top()+top_offset),
        Vec2::new(width,height)
    );
    debug_assert!((rect.right()-toolbar.right()).abs()<0.01,
        "management toolbar trailing edge must match the card's inner right edge");
    rect
}

#[cfg(test)]
#[test]
fn management_table_outline_keeps_header_width_and_viewport_bottom(){
    let header=Rect::from_min_max(egui::pos2(20.0,40.0),egui::pos2(420.0,76.0));
    let body=Rect::from_min_max(egui::pos2(20.0,76.0),egui::pos2(410.0,300.0));
    assert_eq!(crate::table::outline_bounds(header,body),
        Rect::from_min_max(header.min,egui::pos2(420.0,300.0)));
    assert_eq!(crate::table::outline_bounds(header,header),header);
}

/// CategoryTree presentation contract. The caller owns data, selection and
/// expansion; all tree row geometry and painting are centralized here.
pub(crate) struct CategoryTreeRow<'a>{
    pub name:&'a str,
    pub summary:&'a str,
    pub depth:usize,
    /// For each ancestor axis above the immediate parent, whether a later
    /// sibling still needs that vertical rail (root axis is index zero).
    pub ancestor_guides:&'a [bool],
    pub is_last:bool,
    pub root:bool,
    pub has_children:bool,
    pub open:bool,
    pub selected:bool,
}

struct CategoryTreeGeometry{
    row:Rect,
    chevron:Rect,
    folder:Rect,
    name_x:f32,
    depth:usize,
}

impl CategoryTreeGeometry{
    fn new(row:Rect,depth:usize)->Self{
        let chevron=Rect::from_min_size(
            egui::pos2(row.left()+theme::CATALOG_TREE_INSET
                +depth as f32*theme::CATALOG_TREE_INDENT,
                row.center().y-theme::CATALOG_TREE_CHEVRON_SIZE/2.0),
            Vec2::splat(theme::CATALOG_TREE_CHEVRON_SIZE)
        );
        let folder=Rect::from_min_size(
            egui::pos2(chevron.right()+theme::CATALOG_TREE_GAP,
                row.center().y-theme::CATALOG_TREE_FOLDER_SIZE/2.0),
            Vec2::splat(theme::CATALOG_TREE_FOLDER_SIZE)
        );
        Self{row,chevron,folder,name_x:folder.right()+theme::CATALOG_TREE_GAP,depth}
    }
    fn axis(&self,level:usize)->f32{
        self.row.left()+theme::CATALOG_TREE_INSET
            +theme::CATALOG_TREE_CHEVRON_SIZE/2.0
            +level as f32*theme::CATALOG_TREE_INDENT
    }
    fn guides(&self,ancestor_guides:&[bool],is_last:bool,has_children:bool,
        open:bool,mut draw:impl FnMut([egui::Pos2;2])){
        let y=self.row.center().y;
        let top=self.row.top();
        let bottom=self.row.bottom();
        let p=egui::pos2;
        // Older ancestor rails never overlap the immediate parent elbow.
        for (level,&continued) in ancestor_guides.iter().take(self.depth.saturating_sub(1)).enumerate(){
            if continued{
                let x=self.axis(level);
                draw([p(x,top),p(x,bottom)]);
            }
        }
        if self.depth>0{
            let x=self.axis(self.depth-1);
            draw([p(x,top),p(x,if is_last{y}else{bottom})]);
            // Leaves reserve the same chevron column, but the elbow extends
            // through that empty slot to the folder instead of stopping short.
            let target=if has_children{self.chevron.left()}else{self.folder.left()};
            draw([p(x,y),p((target-theme::CATALOG_TREE_TERMINAL_GAP).max(x),y)]);
        }
        if has_children&&open{
            let x=self.axis(self.depth);
            draw([p(x,(self.chevron.bottom()+theme::CATALOG_TREE_TERMINAL_GAP).min(bottom)),p(x,bottom)]);
        }
    }
}

/// Font-independent 20pt vector folder, with a distinct open front flap.
fn paint_category_tree_folder(painter:&egui::Painter,rect:Rect,open:bool,ink:Color32){
    let p=|x:f32,y:f32|egui::pos2(rect.left()+rect.width()*x/20.0,
        rect.top()+rect.height()*y/20.0);
    let stroke=Stroke::new(theme::CATALOG_TREE_FOLDER_STROKE,ink);
    if open{
        painter.add(egui::Shape::line(vec![
            p(2.0,16.5),p(2.0,4.0),p(7.0,4.0),p(9.0,6.0),p(16.0,6.0),p(16.0,8.0)
        ],stroke));
        painter.add(egui::Shape::closed_line(vec![
            p(2.0,16.5),p(5.0,8.5),p(18.5,8.5),p(15.5,16.5)
        ],stroke));
    }else{
        painter.add(egui::Shape::closed_line(vec![
            p(2.0,16.5),p(2.0,4.0),p(7.0,4.0),p(9.0,6.0),
            p(18.0,6.0),p(18.0,16.5)
        ],stroke));
    }
}

pub(crate) fn category_tree_row(ui:&mut Ui,item:CategoryTreeRow<'_>)->Response{
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(1.0),theme::CATALOG_TREE_ROW_HEIGHT),Sense::click()
    );
    let geo=CategoryTreeGeometry::new(rect,item.depth);
    let painter=ui.painter().with_clip_rect(rect);
    let radius=egui::CornerRadius::same(theme::CONTROL_RADIUS);
    if item.selected{
        let highlight=rect.shrink(0.5);
        if highlight.width()>1.0{
            painter.rect_filled(highlight,radius,theme::CATALOG_TREE_SELECTED_BG);
            painter.rect_stroke(highlight,radius,Stroke::new(1.0,theme::CATALOG_TREE_SELECTED_BORDER),
                egui::StrokeKind::Inside);
        }
    }else{
        crate::button::paint_button_hover(ui,&response,theme::CONTROL_RADIUS as f32);
    }
    let guide=Stroke::new(theme::CATALOG_TREE_GUIDE_WIDTH,theme::CATALOG_TREE_GUIDE_COLOR);
    geo.guides(item.ancestor_guides,item.is_last,item.has_children,item.open,
        |segment|{painter.line_segment(segment,guide);});
    if item.has_children{
        paint_management_chevron(&painter,geo.chevron.center(),
            if item.open{ManagementChevronDirection::Down}else{ManagementChevronDirection::Right});
    }
    let ink=if item.selected{theme::CATALOG_TREE_SELECTED_INK}else{theme::CATALOG_TREE_FOLDER_COLOR};
    paint_category_tree_folder(&painter,geo.folder,item.has_children&&item.open,ink);
    // Reserve metadata first and clip both label regions at the shared inset.
    // No full-width row dividers: hierarchy rails own the grouping.
    let right=(rect.right()-theme::CATALOG_TREE_INSET).max(rect.left());
    let name_x=geo.name_x.min(right);
    let summary_galley=typography::client_table_metadata_galley(ui,item.summary,theme::SECONDARY);
    let summary_reserve=if item.summary.is_empty(){0.0}else{theme::CATALOG_TREE_GAP+summary_galley.size().x};
    let name_width=(right-name_x-summary_reserve).max(0.0);
    let font=if item.root{typography::CategoryPathFont::Subsection}else{typography::CategoryPathFont::Body};
    let visible=typography::fit_category_path(ui,item.name,name_width,font);
    let name_galley=typography::category_tree_name_galley(ui,&visible,item.root);
    let name_end=name_x+name_galley.size().x.min(name_width);
    if name_end>name_x{
        painter.with_clip_rect(Rect::from_min_max(egui::pos2(name_x,rect.top()),
            egui::pos2(name_end,rect.bottom()))).galley(
            egui::pos2(name_x,rect.center().y-name_galley.size().y/2.0),
            name_galley,theme::TEXT);
    }
    let summary_x=(name_end+theme::CATALOG_TREE_GAP).min(right);
    if !item.summary.is_empty()&&right>summary_x{
        painter.with_clip_rect(Rect::from_min_max(egui::pos2(summary_x,rect.top()),
            egui::pos2(right,rect.bottom()))).galley(
            egui::pos2(summary_x,rect.center().y-summary_galley.size().y/2.0),
            summary_galley,theme::SECONDARY);
    }
    overflow_tooltip(response,visible!=item.name,item.name)
}

#[cfg(test)]
mod category_tree_geometry_tests{
    use super::*;
    fn row(depth:usize)->CategoryTreeGeometry{
        CategoryTreeGeometry::new(Rect::from_min_size(egui::pos2(0.0,0.0),
            Vec2::new(500.0,theme::CATALOG_TREE_ROW_HEIGHT)),depth)
    }
    #[test]
    fn category_tree_slots_share_depth_and_center(){
        let root=row(0);
        let child=row(1);
        assert_eq!(root.row.height(),42.0);
        assert_eq!(root.folder.size(),Vec2::splat(20.0));
        assert_eq!(root.chevron.size(),Vec2::splat(16.0));
        assert_eq!(child.folder.left()-root.folder.left(),24.0);
        assert_eq!(child.folder.left()-child.chevron.right(),8.0);
        assert_eq!(child.name_x-child.folder.right(),8.0);
        assert_eq!(child.folder.center().y,child.chevron.center().y);
    }
    #[test]
    fn category_tree_leaf_elbow_reaches_folder_and_last_rail_stops(){
        let g=row(2);
        let mut lines=Vec::new();
        g.guides(&[true],true,false,false,|s|lines.push(s));
        assert_eq!(lines.len(),3);
        assert_eq!(lines[0][0].x,g.axis(0));
        assert_eq!(lines[0][1].y,g.row.bottom());
        assert_eq!(lines[1][1].y,g.row.center().y);
        assert_eq!(lines[2][1].x,g.folder.left()-4.0);
        lines.clear();
        g.guides(&[false],false,true,true,|s|lines.push(s));
        assert_eq!(lines.len(),3);
        assert_eq!(lines[0][1].y,g.row.bottom());
        assert_eq!(lines[1][1].x,g.chevron.left()-4.0);
        assert_eq!(lines[2][0].x,g.axis(2));
        assert_eq!(lines[2][1].y,g.row.bottom());
    }
}

/// Native CollapsingHeader keeps its original click/keyboard/state/indent
/// behavior; only its default filled triangle becomes the common line icon.
/// Rotate smoothly with egui's animated openness (0 = right, 1 = down).
pub fn collapsing_header(title:impl Into<egui::WidgetText>)->egui::CollapsingHeader{
    egui::CollapsingHeader::new(title).icon(paint_collapsing_header_icon)
}


fn paint_collapsing_header_icon(
    ui:&mut Ui,openness:f32,response:&Response
){
    let t=openness.clamp(0.0,1.0);
    paint_chevron_at_angle(
        ui.painter(),response.rect.center(),
        t*std::f32::consts::FRAC_PI_2,t,theme::SECONDARY
    );
}


/// Select a row in an editor's left-hand list. This is NOT a table
/// record: it has a 14pt item title and a separate 12pt state caption,
/// following the original 48pt message-template chooser hierarchy.
/// Painting is confined to the reserved fixed-height rectangle, so hover
/// and selected styles never change the following row positions.
pub fn management_chooser_row(
    ui:&mut Ui,title:&str,status:&str,selected:bool
)->Response{
    let width=ui.available_width().max(1.0);
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::MANAGEMENT_CHOOSER_ROW_HEIGHT),Sense::click()
    );
    let background=if selected {
        theme::SELECTION.linear_multiply(0.22)
    }else if response.hovered(){theme::INPUT}else{theme::PANEL};
    let painter=ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect,theme::CONTROL_RADIUS as f32,background);
    if selected{
        painter.rect_stroke(
            rect.shrink(0.5),theme::CONTROL_RADIUS as f32,
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SELECTION),
            egui::StrokeKind::Inside
        );
    }
    let primary=typography::management_table_cell_galley(
        ui,title,typography::ManagementTableCellRole::ClientName
    );
    let secondary=typography::client_table_metadata_galley(
        ui,status,theme::MUTED
    );
    let lines=primary.size().y+secondary.size().y+theme::SPACE_4;
    let x=rect.left()+theme::SPACE_12;
    let y=rect.center().y-lines/2.0;
    let content_rect=Rect::from_min_max(
        egui::pos2(x,rect.top()),egui::pos2(rect.right()-theme::SPACE_12,rect.bottom())
    );
    let ink=painter.with_clip_rect(content_rect);
    ink.galley(egui::pos2(x,y),primary,theme::TEXT);
    ink.galley(
        egui::pos2(x,y+lines-secondary.size().y),
        secondary,theme::MUTED
    );
    if !selected {
        crate::button::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    response
}

/// Read-only presentation of a canonical client. Used by both management
/// rows and the compact Reception card; never modifies billing or matching.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ClientDisplaySummary{
    pub name:String,pub tier:String,pub address:String,
    pub phone:String,pub mobile:String,pub conditions:String,
}
pub fn client_tier_label(tier:&str)->&'static str{
    match tier{
        "wholesale"=>"도매","semi"=>"반도매",
        "retail"=>"소매","special"=>"특수",_=>"미지정"
    }
}
pub fn client_exception_terms(client:&serde_json::Value)->String{
    let mut flags=Vec::new();
    if client["payment"].as_str()==Some("POSTPAID"){flags.push("후불");}
    if client["defaultApplyVat"].as_bool().unwrap_or(false){
        flags.push("부가세 포함");
    }
    if client["shippingFeeExcluded"].as_bool().unwrap_or(false){
        flags.push("운임 청구 제외");
    }
    if client["active"]==serde_json::Value::Bool(false){
        flags.push("거래 중지");
    }
    flags.join(" / ")
}
impl ClientDisplaySummary{
    pub fn from_client(client:&serde_json::Value)->Self{
        let field=|key:&str|client.get(key)
            .and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
        let name=client.get("name").and_then(serde_json::Value::as_str)
            .filter(|s|!s.trim().is_empty())
            .or_else(||client.get("displayName").and_then(serde_json::Value::as_str))
            .unwrap_or("").to_owned();
        Self{name,tier:field("tier"),address:field("address"),
            phone:field("phone"),mobile:field("mobile"),
            conditions:client_exception_terms(client)}
    }
    pub fn primary_phone(&self)->&str{
        if self.phone.trim().is_empty(){&self.mobile}else{&self.phone}
    }
    pub fn secondary_mobile(&self)->&str{
        if self.phone.trim().is_empty() || self.mobile.trim().is_empty()
            || self.phone==self.mobile{""}else{&self.mobile}
    }
}

/// The tier palette belongs to the shared design system. Badge geometry is
/// painted inside the client column and cannot expand the row's layout.
fn client_tier_colors(tier:&str)->(Color32,Color32) {
    match tier {
        "wholesale"=>(theme::CLIENT_TIER_WHOLESALE_BG,theme::CLIENT_TIER_WHOLESALE_TEXT),
        "semi"=>(theme::CLIENT_TIER_SEMI_BG,theme::CLIENT_TIER_SEMI_TEXT),
        "retail"=>(theme::CLIENT_TIER_RETAIL_BG,theme::CLIENT_TIER_RETAIL_TEXT),
        "special"=>(theme::CLIENT_TIER_SPECIAL_BG,theme::CLIENT_TIER_SPECIAL_TEXT),
        _=>(theme::RAISED,theme::SECONDARY),
    }
}

/// Shared measured badge: the longest label (반도매) controls width.
/// Both the management table and Reception render through this painter.
fn paint_client_tier_badge(ui:&Ui,cell:Rect,label:&str,tier:&str)->Rect{
    let (fill,ink)=client_tier_colors(tier);
    let galley=typography::client_table_metadata_galley(ui,label,ink);
    let widest=typography::client_table_metadata_galley(ui,"반도매",ink);
    let width=(widest.size().x.max(galley.size().x)+theme::SPACE_8)
        .min(cell.width().max(0.0));
    let height=widest.size().y.max(galley.size().y)+theme::SPACE_4;
    let badge=Rect::from_min_size(
        egui::pos2(cell.left(),cell.center().y-height/2.0),
        Vec2::new(width,height)
    );
    ui.painter().with_clip_rect(cell).rect_filled(
        badge,theme::CONTROL_RADIUS as f32,fill
    );
    paint_table_galley(
        ui,badge,badge.left()+(badge.width()-galley.size().x)/2.0,galley,ink
    );
    badge
}

/// Keep names and the first exceptional condition adjacent on a compact
/// card. The management table's address column may expand; the intake card's
/// name must never use that empty width as a spacer.
fn client_card_name_limit(
    available:f32,name_width:f32,terms_width:f32,has_terms:bool
)->f32{
    let available=available.max(0.0);
    if !has_terms || available<=160.0{return name_width.min(available);}
    let reserved=terms_width.min(available*0.45)+theme::SPACE_8;
    name_width.min((available-reserved).max(0.0))
}

// Preserve the exact existing prefix/ellipsis measurement and reuse it
// across idle frames. The full galley is still supplied by egui.
thread_local!{
    static CLIENT_NAME_FIT:std::cell::RefCell<Vec<(String,u32,u32,std::sync::Arc<str>)>>=
        const{std::cell::RefCell::new(Vec::new())};
}
fn fitted_client_name(ui:&Ui,name:&str,limit:f32)->std::sync::Arc<str>{
    let scale=ui.ctx().pixels_per_point().to_bits();
    CLIENT_NAME_FIT.with(|cache|{
        let mut cache=cache.borrow_mut();
        if let Some(index)=cache.iter().position(|(text,width,dpi,_)|
            text==name && *width==limit.to_bits() && *dpi==scale){
            let entry=cache.remove(index);let shown=entry.3.clone();cache.push(entry);return shown;
        }
        let measure=|text:&str|typography::management_table_cell_galley(
            ui,text,typography::ManagementTableCellRole::ClientName).size().x;
        let mut shown=String::new();
        if measure("…")<=limit{
            for ch in name.chars(){
                let next=format!("{shown}{ch}…");
                if measure(&next)>limit{break;}
                shown.push(ch);
            }
            shown.push('…');
        }
        let shown:std::sync::Arc<str>=shown.into();
        if cache.len()>=64{cache.remove(0);}
        cache.push((name.to_owned(),limit.to_bits(),scale,shown.clone()));shown
    })
}

/// Shared source-card geometry, reserving the same chrome across stages.
pub fn reception_source_client_rect(body:Rect)->Rect{
    let gap=theme::SPACE_8;
    let height=(body.height()-4.0*gap-theme::ACTION_STANDARD_HEIGHT)
        .max(0.0).min(theme::ACTION_STANDARD_HEIGHT+2.0*gap);
    Rect::from_min_size(egui::pos2(body.left(),body.top()+gap),
        Vec2::new(body.width(),height))
}

/// Keep the exact client surface when workflow permissions change.
pub fn reception_client_summary_card_editable(
    ui:&mut Ui,rect:Rect,linked:bool,
    summary:Option<&ClientDisplaySummary>,fallback:&str,editable:bool,
)->bool{
    paint_reception_client_summary_surface(ui,rect);
    if rect.width()<40.0||rect.height()<18.0{return false;}
    let inset=theme::SPACE_12.min(rect.width()/6.0);
    let button_label=if linked{"변경"}else{"거래처 찾기"};
    let button_width=if linked{76.0}else{112.0};
    let has_button=rect.width()>button_width+2.0*inset+48.0
        && rect.height()>=theme::ACTION_STANDARD_HEIGHT;
    let button_x=rect.right()-inset-button_width;
    let text_right=if has_button{
        button_x-theme::SPACE_8
    }else{rect.right()-inset};
    let left=rect.left()+inset;
    // Only one visible line: badge, client name, exceptional terms.
    // Center it vertically alongside the standard change/search action.
    // Clip inside the existing card so short panes cannot overlap the source.
    let row=Rect::from_min_max(
        egui::pos2(left,rect.top()),
        egui::pos2(text_right,rect.bottom())
    ).intersect(rect);
    if row.width()>0.0&&row.height()>0.0{
        let mut x=row.left();
        if let Some(data)=summary.filter(|c|!c.tier.is_empty()){
            let badge=paint_client_tier_badge(
                ui,row,client_tier_label(&data.tier),&data.tier
            );
            x=badge.right()+theme::SPACE_8;
        }
        if x<row.right(){
            let name=summary.map(|data|data.name.as_str())
                .filter(|value|!value.trim().is_empty()).unwrap_or(fallback);
            let terms=summary.map(|data|data.conditions.as_str()).unwrap_or("");
            let available=row.right()-x;
            let terms_galley=typography::client_table_metadata_galley(
                ui,terms,theme::MUTED
            );
            let full=typography::management_table_cell_galley(
                ui,name,typography::ManagementTableCellRole::ClientName
            );
            let name_limit=client_card_name_limit(
                available,full.size().x,terms_galley.size().x,
                !terms.is_empty()
            );
            // Truncate the name only when genuinely necessary. Short names
            // occupy their measured width, not the whole flexible subcell:
            // the next condition starts directly after the visible name.
            let name_overflow=full.size().x>name_limit;
            let shown_galley=if name_overflow{
                let shown=fitted_client_name(ui,name,name_limit);
                typography::management_table_cell_galley(
                    ui,&shown,typography::ManagementTableCellRole::ClientName
                )
            }else{full};
            let name_width=shown_galley.size().x.min(name_limit);
            if name_width>0.0{
                let name_cell=Rect::from_min_max(
                    egui::pos2(x,row.top()),
                    egui::pos2(x+name_width,row.bottom())
                );
                paint_table_galley(ui,name_cell,x,shown_galley,theme::TEXT);
                overflow_region_tooltip(ui,name_cell,
                    ui.make_persistent_id("source-client-name-overflow"),name_overflow,name);
            }
            if !terms.is_empty(){
                let terms_x=x+name_width+theme::SPACE_8;
                if terms_x<row.right(){
                    let terms_cell=Rect::from_min_max(
                        egui::pos2(terms_x,row.top()),row.right_bottom()
                    );
                    let overflow=paint_table_galley(
                        ui,terms_cell,terms_x,terms_galley,theme::MUTED
                    );
                    overflow_region_tooltip(ui,terms_cell,
                        ui.make_persistent_id("source-client-terms-overflow"),overflow,terms);
                }
            }
        }
    }
    if !has_button{return false;}
    let button_rect=Rect::from_min_size(
        egui::pos2(button_x,
            rect.center().y-theme::ACTION_STANDARD_HEIGHT/2.0),
        Vec2::new(button_width,theme::ACTION_STANDARD_HEIGHT)
    );
    let clip=button_rect.intersect(ui.clip_rect());
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt("source-client-card-action")
            .max_rect(button_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |buttons|{
            buttons.set_clip_rect(clip);
            crate::button::action_with_min_width(
                buttons,button_label,theme::ButtonRole::Standard,
                editable,button_width
            ).clicked()
        }
    ).inner
}



/// Four client data columns. The visible index determines the alternating
/// background; the dedicated management_client_header owns header rendering.
pub fn management_row(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);4],tier:&str,visible_index:usize
) -> Response {
    let width=ui.available_width().max(theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH);
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::MANAGEMENT_CLIENT_ROW_HEIGHT),Sense::click()
    );
    let bg=if response.hovered() {theme::INPUT}
        else {management_table_row_fill(visible_index)};
    ui.painter().rect_filled(rect,0.0,bg);
    // Paint the 1pt row rule inside its allocated rect. A centered
    // line_segment on bottom() was half-clipped and looked inconsistent
    // against the master's crisp 2pt header band at fractional DPI.
    paint_management_table_row_rule(ui.painter(),rect);

    let mut x=rect.left();
    let column_widths=management_client_column_widths(rect.width());
    for (i,(primary,secondary)) in columns.iter().enumerate() {
        let col_width=column_widths[i];
        let cell=Rect::from_min_max(
            egui::pos2(x+theme::SPACE_12,rect.top()),
            egui::pos2(x+col_width-theme::SPACE_12,rect.bottom())
        );
        // Every header and value shares the column's leading inset.
        // Only the exact field that exceeds its painted rectangle gets a tooltip.
        if i==3 {
            let badge=paint_client_tier_badge(
                ui,cell,primary,tier
            );
            let (_,ink)=client_tier_colors(tier);
            let badge_size=typography::client_table_metadata_galley(ui,primary,ink).size();
            overflow_region_tooltip(ui,badge,
                ui.make_persistent_id(("management-row-hover",row_id,i,0)),
                text_exceeds_rect(badge_size,badge.intersect(ui.clip_rect())),primary);
            if !secondary.is_empty(){
                let second=Rect::from_min_max(
                    egui::pos2((badge.right()+theme::SPACE_8).min(cell.right()),cell.top()),
                    cell.max
                );
                let galley=typography::client_table_metadata_galley(ui,secondary,theme::MUTED);
                let overflow=paint_table_galley(ui,second,second.left(),galley,theme::MUTED);
                overflow_region_tooltip(ui,second,
                    ui.make_persistent_id(("management-row-hover",row_id,i,1)),overflow,secondary);
            }
        }else{
            // A secondary contact is a separate field, not clipped primary text.
            let visible=if primary.is_empty(){secondary}else{primary};
            if !visible.is_empty(){
                let metadata=primary.is_empty() && i==0;
                let galley=if metadata{
                    typography::client_table_metadata_galley(ui,visible,theme::MUTED)
                }else{
                    typography::management_table_cell_galley(
                        ui,visible,if i==0{
                            typography::ManagementTableCellRole::ClientName
                        }else{
                            typography::ManagementTableCellRole::Text
                        }
                    )
                };
                let overflow=paint_table_galley(
                    ui,cell,cell.left(),galley,
                    if metadata{theme::MUTED}else{theme::TEXT}
                );
                overflow_region_tooltip(ui,cell,
                    ui.make_persistent_id(("management-row-hover",row_id,i,0)),overflow,visible);
            }
        }
        x+=col_width;
    }
    management_master_column_rules(
        ui,rect,
        management_client_column_edges(rect.left(),rect.width())
    );
    crate::button::paint_button_hover(ui,&response,0.0);
    response
}

#[cfg(test)]
mod management_client_column_divider_tests{
    use super::*;
    #[test]
    fn address_gains_all_extra_width(){
        assert_eq!(management_client_column_widths(900.0),
            [172.0,224.0,154.0,350.0]);
        assert_eq!(management_client_column_widths(1200.0),
            [172.0,524.0,154.0,350.0]);
    }
    #[test]
    fn header_and_rows_share_four_column_edges(){
        assert_eq!(management_client_column_edges(0.0,900.0),
            [172.0,396.0,550.0]);
        assert_eq!(management_client_column_edges(-20.0,900.0),
            [152.0,376.0,530.0]);
    }
    #[test]
    fn boundaries_are_ordered_and_inside_table(){
        for width in [theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH,900.0,1200.0]{
            let edges=management_client_column_edges(25.0,width);
            assert!(edges[0]>25.0);
            assert!(edges[1]>edges[0]);
            assert!(edges[2]>edges[1]);
            assert!(edges[2]<25.0+width);
        }
    }
}

#[cfg(test)]
mod client_display_summary_tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn short_client_names_do_not_create_extra_space_before_conditions(){
        assert_eq!(client_card_name_limit(400.0,62.0,94.0,true),62.0);
        assert_eq!(client_card_name_limit(400.0,340.0,94.0,true),298.0);
        assert_eq!(client_card_name_limit(400.0,62.0,94.0,false),62.0);
        assert_eq!(client_card_name_limit(130.0,200.0,94.0,true),130.0);
    }
    #[test]
    fn shared_tier_and_default_conditions(){
        let data=json!({"name":"테스트 상사","tier":"semi",
            "payment":"PREPAID","defaultApplyVat":false,
            "shippingFeeExcluded":false,"active":true,
            "phone":"02-1111-2222","mobile":"010-1111-2222"});
        let summary=ClientDisplaySummary::from_client(&data);
        assert_eq!(client_tier_label(&summary.tier),"반도매");
        assert_eq!(summary.conditions,"");
        assert_eq!(summary.primary_phone(),"02-1111-2222");
        assert_eq!(summary.secondary_mobile(),"010-1111-2222");
    }
    #[test]
    fn exceptional_conditions_are_shared(){
        let data=json!({"payment":"POSTPAID","defaultApplyVat":true,
            "shippingFeeExcluded":true,"active":false});
        assert_eq!(client_exception_terms(&data),
            "후불 / 부가세 포함 / 운임 청구 제외 / 거래 중지");
    }
}


/// Ellipsize only the displayed product label, using the exact 16pt glyph
/// width. This does not alter the Rust item title, matching key or hover text.
/// Unicode char boundaries prevent cutting a Hangul syllable or UTF-8 byte.
// Cache the exact measured truncation, not the galley: the latter still
// uses egui's current font atlas and active highlight ink.
#[derive(Clone)]
struct ReceptionTextFit{ text:String,width:u32,scale:u32,font:egui::FontId,shown:std::sync::Arc<str> }
thread_local!{
    static RECEPTION_TEXT_FIT:std::cell::RefCell<Vec<ReceptionTextFit>>=
        const{std::cell::RefCell::new(Vec::new())};
}
fn cached_reception_product(ui:&Ui,text:&str,width:f32,font:&egui::FontId,color:egui::Color32)->std::sync::Arc<str>{
    let scale=ui.ctx().pixels_per_point().to_bits();
    RECEPTION_TEXT_FIT.with(|cache|{
        let mut cache=cache.borrow_mut();
        if let Some(index)=cache.iter().position(|entry|entry.text==text &&
            entry.width==width.to_bits() && entry.scale==scale && &entry.font==font){
            let entry=cache.remove(index);let shown=entry.shown.clone();cache.push(entry);return shown;
        }
        let shown:std::sync::Arc<str>=reception_ellipsized_product(text,width,|candidate|
            ui.painter().layout_no_wrap(candidate.to_owned(),font.clone(),color).size().x).into();
        if cache.len()>=128{cache.remove(0);}
        cache.push(ReceptionTextFit{text:text.to_owned(),width:width.to_bits(),scale,font:font.clone(),shown:shown.clone()});
        shown
    })
}
pub fn clear_reception_text_fit(){
    RECEPTION_TEXT_FIT.with(|cache|cache.borrow_mut().clear());
    CLIENT_NAME_FIT.with(|cache|cache.borrow_mut().clear());
}
fn reception_ellipsized_product(
    full:&str,max_width:f32,mut measure:impl FnMut(&str)->f32
)->String {
    if max_width<=0.0 {return String::new();}
    if measure(full)<=max_width {return full.to_owned();}
    if measure("…")>max_width {return String::new();}
    let boundaries:Vec<usize>=full.char_indices().map(|(i,_)|i)
        .chain(std::iter::once(full.len())).collect();
    let mut low=0usize;
    let mut high=boundaries.len()-1;
    while low<high {
        let mid=low+(high-low+1)/2;
        let candidate=format!("{}…",&full[..boundaries[mid]]);
        if measure(&candidate)<=max_width {low=mid;}
        else {high=mid-1;}
    }
    format!("{}…",full[..boundaries[low]].trim_end())
}

/// A deliberately compact presentation for exceptional quantities.
/// Backend quantities, pricing and full-value tooltips remain untouched.
fn reception_compact_quantity(full:&str)->&str {
    let count=full.strip_suffix('개')
        .and_then(|digits|digits.parse::<u64>().ok());
    if count.is_some_and(|n|n>=1000) {"999+"} else {full}
}

pub fn reception_item_delete_confirmation(
    ctx:&egui::Context,work_area:Rect,name:&str,error:Option<&str>,busy:bool
)->(bool,bool){
    let mut cancel=false;
    let mut remove=false;
    show_decision_window(ctx,"품목 삭제 확인","reception-item-delete-confirm",work_area,|ui|{
            (cancel,remove)=decision_content(
                ui,"품목 삭제 확인","취소",!busy,
                |body|{
                    body.add(egui::Label::new(typography::body(
                        &format!("{} 품목을 발주서에서 삭제할까요?",name)
                    )).wrap());
                    let width=body.available_width();
                    fixed_status_slot(body,width,typography::METADATA_LINE_HEIGHT,
                        error,error,theme::DANGER);
                },
                |actions|crate::button::action(actions,"삭제",theme::ButtonRole::Danger,!busy).clicked()
            );
        });
    (cancel,remove)
}

pub struct ReceptionItemCells {
    pub product: Response,
    pub quantity: Response,
    pub amount: Response,
    pub remove_clicked: bool,
}




fn reception_pending_money_marker(value:&str)->bool{
    matches!(value.trim(), "-" | "–" | "—")
}

/// Shared item-table to freight-group transition; call once per group,
/// not for every dispatch origin.
pub fn reception_items_to_freight_gap(ui:&mut Ui){
    ui.add_space(theme::RECEPTION_ITEMS_TO_FREIGHT_GAP);
}

/// Render one item row; editability and action-column allocation are independent.
pub fn reception_item_row(
    ui: &mut Ui, name: &str, quantity: &str, amount: &str,
    active: [bool; 3],editable:bool,remove_slot:bool,
) -> ReceptionItemCells {
    let action_width=if remove_slot{theme::RECEPTION_ITEM_REMOVE_WIDTH}else{0.0};
    let width=management_table_data_width(
        ui.available_width(),0.0,theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+action_width
    );
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_ROW_HEIGHT), Sense::hover()
    );
    let columns = reception_item_columns(Rect::from_min_max(
        rect.min,egui::pos2(rect.right()-action_width,rect.bottom())
    ));
    // A 42pt row previously lost 8pt top and bottom, leaving a 26pt
    // highlight rectangle. Give 16pt glyphs a 34pt value surface instead;
    // the 4pt inset keeps both rounded corners and the border inside the row.
    let padded = |index: usize| columns[index].shrink2(
        egui::vec2(theme::SPACE_4, theme::SPACE_4)
    );
    let cells = [padded(0), padded(1), padded(2)];
    let compact_quantity=reception_compact_quantity(quantity);
    // Use explicit column anchors for all three text cells. Ui::put(Label)
    // was moving text toward the center inside the row, while the table
    // header used another alignment calculation.
    // One 16pt proportional font for product names, digits and Korean units.
    // Mono font fallback gave quantity digits and '개' differing visual heights.
    let mut item_overflow=[false;3];
    for (index, (text, color)) in [
        (name, theme::TEXT),
        (quantity, theme::SECONDARY),
        (amount, theme::TEXT),
    ].into_iter().enumerate() {
        let font=match index {
            1=>typography::data_value_font(
                typography::DataValueKind::Quantity,
                typography::DataValueContext::Reception
            ),
            2=>typography::data_value_font(
                typography::DataValueKind::Money,
                typography::DataValueContext::Reception
            ),
            _=>egui::FontId::proportional(typography::FONT_RECEPTION_ITEM_TEXT),
        };
        let cell = cells[index];
        let right = index != 0;
        let row_text=if index==1 {compact_quantity} else {text};
        // Draw the same blue fill as recipient TextEdit, before value glyphs.
        // No header, label, surrounding table row or column divider is filled.
        let orange=!active[index] && crate::one_touch::stage(ui).item(index);
        if active[index] {source_highlight::paint_active_background(ui,cell);}
        else if orange{crate::one_touch::background(ui,cell);}
        // Text must stay clear of the 2pt inset outline. Keeping the
        // value clip and its anchor inside the highlighted cell also stops
        // the last digit / descender from touching a blue edge.
        let value_clip=reception_item_text_clip(
            cell.shrink2(egui::vec2(theme::SPACE_4,0.0)),index
        );
        // Use the same font metrics as Painter::text, not a character-count
        // guess. Only the product column truncates; financial values do not.
        let visible_product=(index==0).then(||cached_reception_product(
            ui,text,(value_clip.width()-0.5).max(0.0),&font,color
        ));
        let ink=if active[index] {source_highlight::ACTIVE_TEXT} else if orange{crate::one_touch::INK}else{color};
        let label=visible_product.as_deref().unwrap_or(row_text);
        let galley=ui.painter().layout_no_wrap(label.to_owned(),font,ink);
        item_overflow[index]=label!=text || text_exceeds_rect(galley.size(),value_clip.intersect(ui.clip_rect()));
        let mut origin=reception_item_value_origin(value_clip,galley.size(),right);
        // Pending monetary markers are not digits; do not inherit the
        // approved 2.5pt downward numeric glyph correction.
        if index==2 && reception_pending_money_marker(label){
            origin.y-=typography::DATA_NUMERIC_GLYPH_OFFSET_Y;
        }
        ui.painter().with_clip_rect(value_clip).galley(origin,galley,ink);
    }
    // Preserve the five-source-field highlighting contract: these three
    // hover responses still map to the exact Rust product/quantity/price keys.
    // Use a distinct id for each column in each row. next_auto_id()
    // alone would not necessarily advance for manually painted cells.
    let row_id = (rect.top().to_bits(), rect.left().to_bits());
    let product = ui.interact(
        cells[0], ui.make_persistent_id(("item-cell", row_id, 0)),
        if editable{Sense::click()}else{Sense::hover()},
    );
    let product=overflow_tooltip(product,item_overflow[0],name);
    let product = if editable { product.on_hover_cursor(egui::CursorIcon::PointingHand) } else { product };
    let quantity_response = ui.interact(
        cells[1], ui.make_persistent_id(("item-cell", row_id, 1)),
        if editable{Sense::click()}else{Sense::hover()},
    );
    let quantity=overflow_tooltip(quantity_response,item_overflow[1],quantity);
    let quantity = if editable { quantity.on_hover_cursor(egui::CursorIcon::Text) } else { quantity };
    let amount_response = ui.interact(
        cells[2], ui.make_persistent_id(("item-cell", row_id, 2)),
        if editable{Sense::click()}else{Sense::hover()},
    );
    let amount=overflow_tooltip(amount_response,item_overflow[2],amount);
    let amount = if editable { amount.on_hover_cursor(egui::CursorIcon::Text) } else { amount };
    paint_management_table_row_rule(ui.painter(),rect);
    // The renderer owns both fill and border. Do not let reception page
    // branches independently paint an outline: pending board previews
    // otherwise receive blue fills without the matching blue stroke.
    for (index,(response,on)) in [(&product,active[0]),(&quantity,active[1]),
        (&amount,active[2])].into_iter().enumerate(){
        source_highlight::paint_active_if(ui,response,on);
        if !on && crate::one_touch::stage(ui).item(index){crate::one_touch::outline(ui,response.rect);}
    }
    let mut remove_clicked=false;
    if remove_slot {
        let hit=Rect::from_center_size(
            egui::pos2(rect.right()-action_width/2.0,rect.center().y),
            egui::vec2(action_width,28.0)
        );
        let enabled=editable && ui.is_enabled();
        let action=ui.interact(hit,ui.make_persistent_id(("item-remove",row_id)),
            if enabled{Sense::click()}else{Sense::hover()});
        let ink=if enabled{theme::SECONDARY}else{theme::DISABLED_ACTION_TEXT};
        if enabled{crate::button::paint_button_hover(ui,&action,theme::CONTROL_RADIUS as f32);}
        let center=hit.center();
        let stroke=Stroke::new(1.5_f32,ink);
        for sign in [-1.0_f32,1.0]{
            ui.painter().line_segment([
                center+egui::vec2(-4.0,-4.0*sign),
                center+egui::vec2(4.0,4.0*sign)
            ],stroke);
        }
        remove_clicked=enabled && action.clicked();
    }
    #[cfg(test)]
    for (name,response) in [("item product",&product),("item quantity",&quantity),("item amount",&amount)]{
        crate::ui_test_support::record(name,response.rect);
    }
    ReceptionItemCells { product, quantity, amount, remove_clicked }
}

/// Presentation-only freight amount supplied by native Rust. The region
/// checkbox is a real controlled egui checkbox whose value comes from the
/// engine's resolved remote-area state; the factory inquiry is a disabled stub.
pub struct ReceptionFreightEdit {
    pub amount_rect:Option<Rect>,
    pub amount_clicked:bool,
    pub amount_click_pos:Option<egui::Pos2>,
    pub remote_clicked:bool,
    pub collect_clicked:bool,
    #[cfg(test)]
    pub remote_rect:Option<Rect>,
}

// Combined intrinsic controls need more width than the fixed 392pt
// product table. Above this threshold they remain in one horizontal
// line; below it the right-hand actions get their own full-width row.
const RECEPTION_FREIGHT_INLINE_MIN_WIDTH:f32=560.0;
fn freight_actions_need_second_row(available_width:f32)->bool{
    available_width<RECEPTION_FREIGHT_INLINE_MIN_WIDTH
}

/// Workflow freight rows have independent permissions: the amount can be
/// blocked for COD/waived shipments while the freight-code and locality
/// controls remain available until the shipment is transferred.
pub fn reception_freight_row_controls(
    ui:&mut Ui,label:&str,amount:Option<i64>,collect:Option<bool>,
    remote:Option<bool>,inquiry_visible:bool,
    amount_editable:bool,collect_editable:bool,remote_editable:bool,
    remote_visible:bool
)->ReceptionFreightEdit{
    let mut clicked=false;
    let mut amount_click_pos=None;
    let mut remote_clicked=false;
    #[cfg(test)]
    let mut remote_rect=None;
    let mut collect_clicked=false;
    let mut amount_rect=None;
    let mut amount_text=amount
        .map(money_edit_text)
        .unwrap_or_else(||RECEPTION_MONEY_PENDING.to_owned());
    // The item grid has a 392pt minimum horizontal scroll extent, but
    // freight has a larger combined intrinsic control width. Never put the
    // left-to-right price/COD group and the right-to-left actions in the
    // same egui horizontal row unless BOTH groups physically fit.
    // Keep the 36pt control geometry; only the row placement responds.
    let stack_actions=freight_actions_need_second_row(ui.available_width());
    let mut draw_price_and_cod=|line:&mut Ui|{
        line.spacing_mut().item_spacing=Vec2::new(theme::SPACE_4,theme::SPACE_4);
        line.allocate_ui_with_layout(
            Vec2::new(168.0,theme::INPUT_HEIGHT),
            Layout::left_to_right(Align::Center),
            |price|{
                price.spacing_mut().item_spacing.x=theme::SPACE_4;
                price.add_space(theme::SPACE_8);
                let (label_rect,_)=price.allocate_exact_size(
                    Vec2::new(40.0,theme::INPUT_HEIGHT),Sense::hover()
                );
                let galley=price.painter().layout_no_wrap(
                    label.to_owned(),
                    egui::FontId::proportional(typography::FONT_RECEPTION_ITEM_TEXT),
                    theme::TEXT,
                );
                price.painter().galley(
                    egui::pos2(
                        label_rect.left(),
                        label_rect.center().y-galley.size().y/2.0
                    ),galley,theme::TEXT
                );
                // Prevent editing without greying out an already entered
                // amount. The disabled parent Ui was dimming valid freight.
                let displayed=reception_freight_money_readonly(price,&mut amount_text);
                amount_rect=Some(displayed.rect);
                if crate::one_touch::stage(price).freight(){crate::one_touch::mark(price,displayed.rect);}
                if amount_editable && collect!=Some(true){
                    let hit=price.interact(
                        displayed.rect,
                        price.make_persistent_id(("edit-applied-freight",label)),
                        Sense::click()
                    );
                    if hit.hovered() {
                        price.ctx().set_cursor_icon(egui::CursorIcon::Text);
                    }
                    if hit.clicked(){
                        clicked=true;
                        amount_click_pos=hit.interact_pointer_pos();
                    }
                }
            }
        );
        // A checkbox/VAT edit temporarily disables interactions while Rust
        // saves. Keep the SAME common dropdown instead of flashing "(선불)".
        let (changed,response)=reception_cod_dropdown(line,collect,collect_editable);
        if crate::one_touch::stage(line).freight(){crate::one_touch::mark(line,response.rect);}
        collect_clicked=changed;
    };
    let mut draw_remote_and_inquiry=|line:&mut Ui|{
        line.spacing_mut().item_spacing.x=theme::SPACE_4;
        // Right edge matches React: disabled inquiry is the rightmost
        // action and the 도서/산간 checkbox is immediately to its left.
        line.with_layout(Layout::right_to_left(Align::Center),|right|{
            // Hidden actions still own the exact same slot as visible ones.
            right.add_visible_ui(inquiry_visible,|slot|{
                crate::button::action(slot,"공장에 운임 문의",theme::ButtonRole::Standard,false);
            });
            if remote_visible{
                let mut checked=remote.unwrap_or(false);
                let response=checkbox(right,&mut checked,"도서/산간",
                    remote_editable && remote.is_some());
                #[cfg(test)]
                {remote_rect=Some(response.rect);}
                if response.changed(){
                    remote_clicked=true;
                }
            }
        });    };
    if stack_actions {
        ui.vertical(|column|{
            column.spacing_mut().item_spacing.y=theme::SPACE_8;
            column.horizontal(|line|draw_price_and_cod(line));
            column.horizontal(|line|draw_remote_and_inquiry(line));
        });
    }else{
        ui.horizontal(|line|{
            draw_price_and_cod(line);
            draw_remote_and_inquiry(line);
        });
    }
    if let Some(rect)=amount_rect{
        crate::one_touch::reveal_target(ui,crate::one_touch::Stage::Freight,rect);
    }
    #[cfg(test)]
    if let Some(rect)=amount_rect{crate::ui_test_support::record("freight amount",rect);}
    ReceptionFreightEdit {
        amount_rect,amount_clicked:clicked,amount_click_pos,
        remote_clicked,collect_clicked,
        #[cfg(test)]
        remote_rect,
    }
}
#[cfg(test)]
mod reception_cod_selector_tests {
    use super::*;

    #[test]
    fn narrow_freight_stacks_controls_instead_of_overlapping_right_to_left(){
        // The item table stays 392pt wide and scrolls horizontally below
        // this; the unrelated freight controls must never overlap it.
        assert!(freight_actions_need_second_row(
            theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH
        ));
        assert!(freight_actions_need_second_row(480.0));
        assert!(freight_actions_need_second_row(559.0));
        assert!(!freight_actions_need_second_row(560.0));
        assert!(!freight_actions_need_second_row(720.0));
    }

    #[test]
    fn factory_inquiry_visibility_does_not_move_remote_checkbox(){
        for width in [392.0,480.0,560.0,800.0]{
            let render=|visible|{
                let ctx=test_context();
                let mut rects=None;
                let _=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width+16.0,600.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        ui.set_width(width);
                        let row=reception_freight_row_controls(ui,"운임",
                            Some(3000),Some(false),Some(false),visible,true,true,true,true);
                        rects=Some([("remote",row.remote_rect.unwrap()),
                            ("amount",row.amount_rect.unwrap())]);
                    });
                });
                rects.unwrap()
            };
            assert_layout_rects_invariant(&render(true),&render(false));
        }
    }

    #[test]
    fn selector_geometry_remains_identical_while_native_edit_is_pending(){
        let ctx=egui::Context::default();
        let mut rects=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(400.0);
                for (editable,collect) in [(true,Some(false)),
                    (false,Some(false)),(true,Some(true)),
                    (false,Some(true)),(false,None)]
                {
                    let (changed,response)=reception_cod_dropdown(
                        ui,collect,editable
                    );
                    assert!(!changed);
                    rects.push(response.rect);
                }
            });
        });
        for rect in &rects {
            assert_eq!(rect.width(),90.0);
            assert_eq!(rect.height(),theme::ACTION_STANDARD_HEIGHT);
        }
    }
}



#[cfg(test)]
pub(crate) fn assert_layout_rects_invariant(
    before:&[(&str,Rect)],after:&[(&str,Rect)]
){
    assert_eq!(before.len(),after.len(),"protected layout slot count changed");
    for ((name,a),(other,b)) in before.iter().zip(after){
        assert_eq!(name,other,"protected layout slot identity changed");
        for (edge,x,y) in [
            ("left",a.left(),b.left()),("top",a.top(),b.top()),
            ("right",a.right(),b.right()),("bottom",a.bottom(),b.bottom())
        ]{
            assert!((x-y).abs()<=0.5,
                "layout drift {name}.{edge}: {x} -> {y}");
        }
    }
}

#[cfg(test)]
mod reception_state_layout_invariance_tests{
    use super::*;

    fn render(width:f32,pending:bool,editable:bool)->Vec<(&'static str,Rect)>{
        let ctx=test_context();
        let mut rects=Vec::new();
        let _=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,800.0))),
            ..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let header=reception_item_header_action(ui,pending,editable);
                rects.push(("header action",header.rect));
                reception_items_header(ui,true);
                let cells=reception_item_row(
                    ui,"BD 709-1",if pending{"—"}else{"1개"},"—",[false;3],editable,true
                );
                rects.extend([("product",cells.product.rect),("quantity",cells.quantity.rect),
                    ("amount",cells.amount.rect)]);
                ui.add_space(theme::SPACE_8);
                let freight=reception_freight_row_controls(
                    ui,"운임",Some(3000),Some(false),Some(false),true,editable,editable,editable,true
                );
                rects.push(("freight amount",freight.amount_rect.expect("freight amount slot")));
                ui.add_space(theme::SPACE_8);
                let origin=ui.cursor().top();
                reception_item_origin_action_editable(ui,Some("FACTORY"),editable);
                rects.push(("origin row",Rect::from_min_max(
                    egui::pos2(ui.min_rect().left(),origin),
                    egui::pos2(ui.min_rect().right(),ui.cursor().top())
                )));
                let footer=crate::button::action(ui,"초안에 적용",theme::ButtonRole::Standard,editable);
                rects.push(("following action",footer.rect));
            });
        });
        rects
    }

    #[test]
    fn pending_assignment_and_issue_feedback_do_not_move_reception_controls(){
        for width in [480.0,800.0,1200.0]{
            let base=render(width,false,true);
            for (pending,editable) in [(true,true),(true,false),(false,false)]{
                assert_layout_rects_invariant(&base,&render(width,pending,editable));
            }
        }
    }

    #[test]
    #[should_panic(expected="layout drift")]
    fn invariant_detector_rejects_inserted_row(){
        let a=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(100.0,34.0));
        assert_layout_rects_invariant(&[("freight",a)],
            &[("freight",a.translate(Vec2::new(0.0,24.0)))]);
    }

    #[test]
    fn production_consumers_do_not_reinsert_item_feedback_rows(){
        let manual=include_str!("manual_workspace.rs");
        let production=production_source_for_contract(manual);
        assert!(!production.contains("crate::button::action(items_ui,\"상품 선택 취소\""));
        assert!(production.contains("crate::button::reception_item_header_action("));
        let main=include_str!("main.rs");
        assert!(!main.contains("items_ui.label(typography::status("));
        assert!(!main.contains("components::reception_item_issue_values("));
        assert!(main.contains("self.review_panel.show_with_client_actions("));
    }
}

/// Dispatch origin is a separate right-aligned row below freight.
/// A selected action remains visually active, not disabled-looking.
pub fn reception_item_origin_action_editable(
    ui:&mut Ui,dispatch_origin:Option<&str>,editable:bool
)->Option<&'static str>{
    let mut selection=None;
    ui.horizontal(|bar|{
        bar.with_layout(Layout::right_to_left(Align::Center),|right|{
            let button=|right:&mut Ui,label:&str,selected:bool|{
                let response=crate::button::dispatch_origin(right,label,selected,editable);
                if selected && crate::one_touch::stage(right).freight(){crate::one_touch::mark(right,response.rect);}
                response.clicked()
            };
            if button(right,"매장",dispatch_origin==Some("STORE")){
                selection=Some("STORE");
            }
            if button(right,"공장",dispatch_origin==Some("FACTORY")){
                selection=Some("FACTORY");
            }
            right.label(typography::caption("발송지:"));
        });
    });
    selection
}

/// In the dedicated "발주서 합계" card, show the right-aligned total amount
/// and the second line of shipment count / VAT without a duplicate "총 주문"
/// caption or another border: the containing card owns its header and rule.
pub fn reception_total_footer(
    ui: &mut Ui, total: Option<i64>, count: Option<usize>, vat: &str,
) {
    let (rect, _response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), theme::RECEPTION_TOTAL_HEIGHT),
        Sense::hover(),
    );
    #[cfg(test)]
    crate::ui_test_support::record("order total footer",rect);
    let left = rect.left() + theme::SPACE_8;
    let right = rect.right() - theme::SPACE_8;
    let right_start = (right - 148.0).max(left + 50.0);
    let review=crate::one_touch::stage(ui)==crate::one_touch::Stage::Review;
    let value_color=if review{crate::one_touch::INK}else{theme::TEXT};
    let amount = total.map(display_money_number)
        .unwrap_or_else(||RECEPTION_MONEY_PENDING.to_owned());
    let first_amount = Rect::from_min_max(
        egui::pos2(left-theme::SPACE_4,rect.top()),
        egui::pos2(right+theme::SPACE_4,rect.top()+30.0),
    );
    let font=typography::data_value_font(
        typography::DataValueKind::Money,
        typography::DataValueContext::ReceptionTotal
    );
    let painter=ui.painter().with_clip_rect(first_amount.intersect(ui.clip_rect()));
    let unit=painter.layout_no_wrap("원".to_owned(),font.clone(),value_color);
    let number=painter.layout_no_wrap(amount,font,value_color);
    // Preserve the fixed currency anchor and the shared number/unit gap.
    // The actual text and its measured surface share one vertical center.
    let center_y=first_amount.center().y;
    let unit_left=money_unit_left(right,0.0,unit.size().x);
    let unit_pos=egui::pos2(unit_left,center_y-unit.size().y/2.0);
    let number_pos=egui::pos2(unit_left-RECEPTION_MONEY_UNIT_GAP-number.size().x,
        center_y-number.size().y/2.0);
    let amount_ink=Rect::from_min_size(number_pos,number.size())
        .union(Rect::from_min_size(unit_pos,unit.size()));
    let amount_highlight=crate::one_touch::value_rect(amount_ink,first_amount);
    if review{crate::one_touch::background(ui,amount_highlight);}
    painter.galley(number_pos,number,value_color);
    painter.galley(unit_pos,unit,value_color);
    if review{crate::one_touch::outline(ui,amount_highlight);}

    // A missing shipment count is unknown, not zero. Only the value,
    // including the unit, participates in one-touch emphasis.
    let second_left = Rect::from_min_max(
        egui::pos2(left, rect.top() + 30.0),
        egui::pos2(right_start, rect.bottom()),
    );
    let metadata_font=egui::FontId::proportional(typography::FONT_METADATA);
    let count_painter=ui.painter().with_clip_rect(second_left.intersect(ui.clip_rect()));
    let center_y=second_left.center().y;
    if let Some(n)=count{
        let prefix=count_painter.layout_no_wrap("배송".to_owned(),metadata_font.clone(),theme::SECONDARY);
        let count_color=if review{crate::one_touch::INK}else{theme::SECONDARY};
        let value=count_painter.layout_no_wrap(format!("{n}건"),metadata_font.clone(),count_color);
        let prefix_pos=egui::pos2(left,center_y-prefix.size().y/2.0);
        let value_pos=egui::pos2(left+prefix.size().x+theme::SPACE_8,center_y-value.size().y/2.0);
        let value_ink=Rect::from_min_size(value_pos,value.size());
        let count_highlight=crate::one_touch::value_rect(value_ink,second_left);
        if review{crate::one_touch::background(ui,count_highlight);}
        count_painter.galley(prefix_pos,prefix,theme::SECONDARY);
        count_painter.galley(value_pos,value,count_color);
        if review{crate::one_touch::outline(ui,count_highlight);}
    }else{
        count_painter.text(egui::pos2(left,center_y),egui::Align2::LEFT_CENTER,
            "배송 건수 미확정",metadata_font.clone(),theme::SECONDARY);
    }
    let second_right = Rect::from_min_max(
        egui::pos2(right_start, rect.top() + 30.0),
        egui::pos2(right, rect.bottom()),
    );
    ui.painter().with_clip_rect(second_right.intersect(ui.clip_rect())).text(
        egui::pos2(right, second_right.center().y), egui::Align2::RIGHT_CENTER,
        vat, metadata_font, theme::SECONDARY,
    );
}

#[cfg(test)]
mod management_date_field_contract_tests {
    #[test]
    fn order_history_date_group_uses_shared_date_text_field(){
        let source=include_str!("order_history_manager.rs");
        let date=source.split("fn range_date_picker_rtl(").nth(1).unwrap()
            .split("\nfn ").next().unwrap();
        assert!(date.contains("crate::input::management_date_text_field("));
        assert!(!date.contains("egui::TextEdit::singleline(date)"));
    }
}

#[cfg(test)]
mod fixed_scroll_panel_contract_tests {
    #[test]
    fn order_detail_panes_delegate_frame_title_and_scroller_to_shared_component(){
        let source=include_str!("order_history_detail.rs");
        let section=source.split("fn section_panel(").nth(1).unwrap()
            .split("fn overview(").next().unwrap();
        assert!(section.contains("crate::card::fixed_scroll_panel("));
        assert!(!section.contains("egui::ScrollArea::vertical()"));
        assert!(!section.contains("ui.painter().rect("));
    }
}

#[cfg(test)]
mod shared_detail_divider_contract_tests {
    #[test]
    fn order_history_detail_uses_shared_soft_dividers(){
        let source=include_str!("order_history_detail.rs");
        assert_eq!(source.matches("crate::ui_primitives::soft_divider(").count(),1);
        assert_eq!(source.matches("crate::ui_primitives::section_divider(").count(),2);
        assert!(!source.contains(".separator("));
    }
}

#[cfg(test)]
mod shared_review_card_contract_tests {
    #[test]
    fn reception_review_cards_use_shared_surface_frame(){
        let source=include_str!("review_panel.rs");
        assert_eq!(source.matches("crate::card::review_issue_card(").count(),2);
        assert!(source.contains("crate::components::body_error_overlay("));
        assert!(!source.contains("egui::Frame::new()"));
        let fallback=include_str!("components.rs").split("pub fn reception_review_status(")
            .nth(1).unwrap().split("\n///").next().unwrap();
        assert!(fallback.contains("crate::card::review_issue_card("));
    }
}

#[cfg(test)]
mod shared_multiline_field_contract_tests {
    #[test]
    fn automatic_message_and_history_memo_use_shared_multiline_editor(){
        let messages=include_str!("automatic_message_manager.rs");
        assert_eq!(messages.matches("crate::input::multiline_text_field(").count(),2);
        assert!(!messages.contains("TextEdit::multiline("));
        assert_eq!(messages.matches("crate::card::preview_surface_frame()").count(),2);
        assert!(!messages.contains("egui::Frame::new()"));
        let history=include_str!("order_history_manager.rs");
        let memo=history.split("pub fn show_memo_modal(").nth(1).unwrap()
            .split("pub fn ").next().unwrap();
        assert!(memo.contains("crate::input::multiline_text_field("));
        assert!(!memo.contains("TextEdit::multiline("));

        let main=include_str!("main.rs");
        let ocr=main.split("fn show_ocr_review(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(ocr.contains("crate::input::multiline_text_field_sized("));
        assert!(!ocr.contains("TextEdit::multiline("));
    }
}

#[cfg(test)]
mod sized_multiline_field_geometry_tests {
    use super::*;

    #[test]
    fn sized_multiline_field_keeps_the_requested_viewport(){
        let ctx=egui::Context::default();
        let mut value=String::from("OCR 검토 텍스트");
        let mut actual=Rect::NOTHING;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                actual=multiline_text_field_sized(
                    ui,&mut value,true,Vec2::new(280.0,96.0),
                    egui::FontId::proportional(typography::FONT_OCR_REVIEW_TEXT),
                    false,egui::Margin::same(theme::SPACE_12 as i8),1
                ).rect;
            });
        });
        assert_eq!(actual.size(),Vec2::new(280.0,96.0));
    }
}



#[cfg(test)]
mod shared_selection_interaction_tests {
    use super::*;

    fn raw(ctx:&egui::Context,events:Vec<egui::Event>)->egui::RawInput{
        egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,700.0))),
            time:Some(ctx.input(|i|i.time)+0.05),predicted_dt:0.05,
            events,..Default::default()
        }
    }
    fn click<R>(pos:egui::Pos2,mut draw:impl FnMut(Vec<egui::Event>)->R)->R{
        draw(vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true,false]{
            draw(vec![egui::Event::PointerButton{
                pos,button:egui::PointerButton::Primary,pressed,
                modifiers:egui::Modifiers::NONE
            }]);
        }
        draw(vec![])
    }
    #[derive(Clone,Copy,Debug)]
    enum Selector { Ordinary,Compact,Status }
    struct SelectFrame {
        trigger:Rect,
        next:Rect,
        outside:Rect,
        options:Vec<Rect>,
        enabled:bool,
    }
    fn select_frame(ctx:&egui::Context,kind:Selector,selected:&mut usize,
        enabled:bool,parent_enabled:bool,changes:&mut usize,
        events:Vec<egui::Event>)->SelectFrame{
        let mut frame=None;
        let _=ctx.run(raw(ctx,events),|ctx|{
            crate::floating_notice::begin_page(ctx,"synthetic selectors");
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(480.0);
                let labels=match kind{
                    Selector::Ordinary=>["그룹 A","그룹 B"],
                    Selector::Compact=>["선불","착불"],
                    Selector::Status=>["판매중","품절"],
                };
                let width=if matches!(kind,Selector::Compact){90.0}else{180.0};
                let colors=[theme::TEXT,theme::DANGER];
                let label=labels[*selected];
                let current_color=colors[*selected];
                let mut options=Vec::new();
                let choose=|menu:&mut Ui|{
                    for (i,label) in labels.into_iter().enumerate(){
                        let response=match kind{
                            Selector::Ordinary=>management_dropdown_option(menu,selected,i,label),
                            Selector::Compact=>management_dropdown_compact_option(
                                menu,selected,i,label,width),
                            Selector::Status=>management_status_dropdown_option(
                                menu,selected,i,label,colors[i]),
                        };
                        options.push(response.rect);
                        if response.changed(){*changes+=1;}
                    }
                };
                let trigger=ui.add_enabled_ui(parent_enabled,|ui|match kind{
                    Selector::Ordinary=>management_dropdown(ui,label,width,enabled,choose),
                    Selector::Compact=>management_dropdown_compact(ui,label,width,enabled,choose),
                    Selector::Status=>management_status_dropdown(
                        ui,label,width,enabled,current_color,choose),
                }).inner;
                let next=ui.allocate_exact_size(Vec2::new(80.0,36.0),Sense::hover()).0;
                let outside=Rect::from_min_size(egui::pos2(700.0,500.0),Vec2::splat(20.0));
                frame=Some(SelectFrame{trigger:trigger.rect,next,outside,options,
                    enabled:trigger.enabled()});
            });
        });
        frame.expect("shared dropdown rendered")
    }
    fn same_select_slots(base:&SelectFrame,frame:&SelectFrame){
        assert_layout_rects_invariant(
            &[("trigger",base.trigger),("following row",base.next)],
            &[("trigger",frame.trigger),("following row",frame.next)]
        );
    }
    #[test]
    fn shared_dropdown_click_selection_escape_outside_and_disabled_keep_fixed_geometry(){
        for kind in [Selector::Ordinary,Selector::Compact,Selector::Status]{
            let ctx=test_context();
            let mut selected=0;
            let mut changes=0;
            let mut draw=|enabled,parent,events|select_frame(
                &ctx,kind,&mut selected,enabled,parent,&mut changes,events
            );
            let base=draw(true,true,vec![]);
            assert_eq!(base.trigger.height(),theme::ACTION_STANDARD_HEIGHT);
            assert!(base.options.is_empty());
            let opened=click(base.trigger.center(),|events|draw(true,true,events));
            same_select_slots(&base,&opened);
            assert_eq!(opened.options.len(),2,"{kind:?}: popup never opened");
            for option in &opened.options{
                assert_eq!(option.height(),theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
                if matches!(kind,Selector::Compact){assert_eq!(option.width(),90.0);}
            }
            assert!(opened.options[1].top()>=opened.options[0].bottom());
            let closed=click(opened.options[1].center(),|events|draw(true,true,events));
            assert!(closed.options.is_empty(),"{kind:?}: selecting must close popup");
            same_select_slots(&base,&closed);
            let reopened=click(base.trigger.center(),|events|draw(true,true,events));
            assert_eq!(reopened.options.len(),2);
            let unchanged=click(reopened.options[1].center(),|events|draw(true,true,events));
            assert!(unchanged.options.is_empty());
            let reopened=click(base.trigger.center(),|events|draw(true,true,events));
            assert_eq!(reopened.options.len(),2);
            draw(true,true,vec![egui::Event::Key{
                key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,
                modifiers:egui::Modifiers::NONE
            }]);
            let escaped=draw(true,true,vec![]);
            assert!(escaped.options.is_empty(),"{kind:?}: Escape must close popup");
            same_select_slots(&base,&escaped);
            let reopened=click(base.trigger.center(),|events|draw(true,true,events));
            assert_eq!(reopened.options.len(),2);
            let outside=click(base.outside.center(),|events|draw(true,true,events));
            assert!(outside.options.is_empty(),"{kind:?}: outside click must close popup");
            same_select_slots(&base,&outside);
            for (enabled,parent) in [(false,true),(true,false)]{
                let disabled=draw(enabled,parent,vec![]);
                assert!(!disabled.enabled);
                same_select_slots(&base,&disabled);
                let blocked=click(base.trigger.center(),|events|draw(enabled,parent,events));
                assert!(blocked.options.is_empty(),"{kind:?}: disabled trigger opened");
                same_select_slots(&base,&blocked);
            }
            drop(draw);
            assert_eq!(selected,1,"{kind:?}: selection was not retained");
            assert_eq!(changes,1,"{kind:?}: exactly one value change expected");
        }
    }

    #[derive(Clone,Copy,Debug)]
    enum CheckKind{Ordinary,List,TableHeader,TableRow}
    struct CheckFrame{hit:Rect,next:Rect,id:egui::Id,enabled:bool}
    fn check_frame(ctx:&egui::Context,kind:CheckKind,checked:&mut bool,
        enabled:bool,parent_enabled:bool,changes:&mut usize,
        events:Vec<egui::Event>)->CheckFrame{
        let mut frame=None;
        let _=ctx.run(raw(ctx,events),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(480.0);
                let response=ui.add_enabled_ui(parent_enabled,|ui|match kind{
                    CheckKind::Ordinary=>checkbox(ui,checked,"도서/산간",enabled),
                    CheckKind::List=>checkbox_list_row(ui,"recipient-1",checked,
                        "합성 수취인",enabled,36.0),
                    CheckKind::TableHeader|CheckKind::TableRow=>{
                        let height=if matches!(kind,CheckKind::TableHeader){
                            theme::MANAGEMENT_TABLE_HEADER_HEIGHT
                        }else{42.0};
                        let (cell,_)=ui.allocate_exact_size(Vec2::new(48.0,height),Sense::hover());
                        let area=if matches!(kind,CheckKind::TableHeader){
                            Rect::from_min_max(cell.min,egui::pos2(cell.right(),cell.bottom()-2.0))
                        }else{cell};
                        management_table_checkbox(ui,cell,area,checked,enabled,"select-row")
                    }
                }).inner;
                if response.changed(){*changes+=1;}
                let next=ui.allocate_exact_size(Vec2::new(80.0,36.0),Sense::hover()).0;
                frame=Some(CheckFrame{hit:response.rect,next,id:response.id,enabled:response.enabled()});
            });
        });
        frame.expect("shared checkbox rendered")
    }
    #[test]
    fn shared_checkbox_click_toggles_once_and_disabled_clicks_preserve_value(){
        for kind in [CheckKind::Ordinary,CheckKind::List,CheckKind::TableHeader,CheckKind::TableRow]{
            let ctx=test_context();
            let mut checked=false;
            let mut changes=0;
            let mut draw=|enabled,parent,events|check_frame(
                &ctx,kind,&mut checked,enabled,parent,&mut changes,events
            );
            let base=draw(true,true,vec![]);
            let same=|frame:&CheckFrame|{
                assert_eq!(frame.id,base.id,"{kind:?}: checkbox identity drift");
                assert_layout_rects_invariant(
                    &[("checkbox",base.hit),("following row",base.next)],
                    &[("checkbox",frame.hit),("following row",frame.next)]
                );
            };
            let on=click(base.hit.center(),|events|draw(true,true,events));
            same(&on);
            for (enabled,parent) in [(false,true),(true,false)]{
                let disabled=draw(enabled,parent,vec![]);
                assert!(!disabled.enabled);
                same(&disabled);
                let blocked=click(base.hit.center(),|events|draw(enabled,parent,events));
                same(&blocked);
            }
            drop(draw);
            assert!(checked,"{kind:?}: disabled click modified selection");
            assert_eq!(changes,1,"{kind:?}: expected one change");
            let off=click(base.hit.center(),|events|check_frame(
                &ctx,kind,&mut checked,true,true,&mut changes,events
            ));
            same(&off);
            assert!(!checked,"{kind:?}: second enabled click must uncheck");
            assert_eq!(changes,2,"{kind:?}: exactly two changes expected");
        }
    }
}

#[cfg(test)]
mod shared_checkbox_contract_tests {
    #[test]
    fn vat_dialog_uses_shared_checkbox_and_nested_frame_components(){
        let source=include_str!("main.rs");
        let dialog=source.split("fn show_vat_settings(").nth(1).unwrap()
            .split("fn show_edit_history(").next().unwrap();
        assert!(dialog.contains("crate::checkbox::checkbox("));
        assert!(dialog.contains("crate::checkbox::checkbox_list_row("));
        assert!(dialog.contains("crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::RecipientChecklist)"));
        assert!(!dialog.contains(".inner_margin("));
        assert!(!dialog.contains("egui::Checkbox::new("));
        assert!(!dialog.contains("egui::Frame::new()"));

        let client=include_str!("client_manager.rs");
        let boolean=client.split("fn bool_edit(").nth(1).unwrap()
            .split("fn new_id(").next().unwrap();
        assert!(boolean.contains("crate::checkbox::checkbox("));
        assert!(!boolean.contains("ui.checkbox("));

        let main=include_str!("main.rs");
        let message=main.split("fn message_section(").nth(1).unwrap()
            .split("fn ").next().unwrap();
        assert!(message.contains("crate::checkbox::checkbox("));
        assert!(!message.contains("Checkbox::new("));
        let workflow=main.split("fn show_workflow_dialog(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(workflow.contains("crate::checkbox::checkbox("));
        assert!(!workflow.contains("ui.checkbox("));

        // OCR learning, category transfer consent and product filtering are
        // standard checkboxes too; none may construct a local egui checkbox.
        assert!(main.contains("changed=crate::checkbox::checkbox("));
        assert!(main.contains("&mut review.learned_analysis_enabled"));
        assert!(!main.contains("option.checkbox("));

        let category=include_str!("category_manager.rs");
        assert!(category.contains("crate::checkbox::checkbox("));
        assert!(!category.contains("notice.checkbox("));

        let products=include_str!("product_manager.rs");
        assert!(products.contains("crate::checkbox::checkbox("));
        assert!(products.contains("&mut self.missing"));
        assert!(!products.contains("filter.checkbox("));
    }
}

#[cfg(test)]
mod shared_ghost_action_contract_tests {
    #[test]
    fn category_explorer_back_uses_shared_ghost_action(){
        let source=include_str!("category_dropdown.rs");
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
mod shared_tag_chip_contract_tests {
    #[test]
    fn client_search_tags_use_shared_chip_and_icon_action(){
        let source=include_str!("client_manager.rs");
        let editor=source.split("fn search_tag_editor(").nth(1).unwrap()
            .split("fn editor_fields(").next().unwrap();
        assert!(editor.contains("crate::popup::removable_tag_chip("));
        assert!(!editor.contains("egui::Frame::new()"));
        assert!(!editor.contains("egui::Button::new("));
    }
}

#[cfg(test)]
mod management_readonly_table_contract_tests {
    use super::*;

    #[test]
    fn read_only_table_width_includes_only_inter_column_gaps(){
        assert_eq!(
            management_readonly_table_content_width(&[86.0,52.0,200.0,70.0,300.0]),
            740.0
        );
        assert_eq!(management_readonly_table_content_width(&[120.0]),120.0);
        assert_eq!(management_readonly_table_content_width(&[]),0.0);
    }

    #[test]
    fn read_only_table_row_advance_includes_one_shared_divider(){
        assert_eq!(
            management_readonly_table_row_advance(),
            theme::MANAGEMENT_COMPACT_ROW_HEIGHT+theme::SOFT_DIVIDER_WIDTH
        );
    }
}

// Narrow geometry contract check only, not a business-data regression suite.
#[cfg(test)]
mod management_search_caret_tests {
    use super::*;

    #[test]
    fn every_management_query_surface_uses_shared_caret_policy(){
        // The list covers actual search/filter controls. Ordinary editing
        // of product names, client tags, dates, addresses, memo text, etc.
        // remains a native click-position TextEdit.
        let surfaces=[
            ("freight",include_str!("freight_manager.rs"),2),
            ("products",include_str!("product_manager.rs"),1),
            ("clients",include_str!("client_manager.rs"),1),
            ("excel",include_str!("excel_manager.rs"),1),
            ("order history",include_str!("order_history_manager.rs"),1),
            // The narrow and wide branches are mutually exclusive render
            // paths for the same logical search field.
            ("categories",include_str!("category_manager.rs"),2),
            ("model groups",include_str!("model_group_manager.rs"),2),
            ("aliases",include_str!("alias_manager.rs"),1),
            ("automatic messages",include_str!("automatic_message_manager.rs"),1),
            ("audit archive",include_str!("settings_view.rs"),1),
            ("client chooser",include_str!("client_chooser.rs"),1),
            ("manual product chooser",include_str!("manual_product_search.rs"),1),
        ];
        for (screen,source,expected) in surfaces {
            let count=source.matches("crate::input::management_search_field(").count();
            assert_eq!(count,expected,"{screen} query did not use the shared field");
        }
        // Search-tag entry is an editor, not a query field.
        let clients=include_str!("client_manager.rs");
        assert!(clients.contains("entry,&mut self.tag_input,width"));
        assert!(clients.contains("crate::input::flexible_text_field("));
    }

    #[test]
    fn clicking_existing_query_resumes_at_the_end_not_the_first_character(){
        fn draw(
            ctx:&egui::Context,raw:egui::RawInput,query:&mut String
        )->(Rect,egui::Id){
            let mut measured=None;
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(320.0);
                    let response=management_search_field(
                        ui,query,240.0,"모델명 / 상품명 검색"
                    );
                    measured=Some((response.rect,response.id));
                });
            });
            measured.expect("search input rendered")
        }
        let ctx=egui::Context::default();
        let mut query="709".to_owned();
        let mut input=egui::RawInput::default();
        input.screen_rect=Some(Rect::from_min_size(
            egui::Pos2::ZERO,Vec2::new(360.0,180.0)
        ));
        let (rect,field_id)=draw(&ctx,input,&mut query);
        let pointer=egui::pos2(rect.left()+8.0,rect.center().y);
        draw(&ctx,egui::RawInput{
            events:vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton{
                    pos:pointer,button:egui::PointerButton::Primary,
                    pressed:true,modifiers:egui::Modifiers::NONE
                },
            ],
            ..Default::default()
        },&mut query);
        draw(&ctx,egui::RawInput{
            events:vec![egui::Event::PointerButton{
                pos:pointer,button:egui::PointerButton::Primary,
                pressed:false,modifiers:egui::Modifiers::NONE
            }],
            ..Default::default()
        },&mut query);
        let state=egui::TextEdit::load_state(&ctx,field_id)
            .expect("search edit state after click");
        let range=state.cursor.char_range().expect("search caret after click");
        assert_eq!(range.primary.index,3);
        assert_eq!(range.secondary.index,3);
        assert_eq!(query,"709");
    }
    #[test]
    fn unicode_search_cursor_end_uses_character_index_not_bytes(){
        let ctx=egui::Context::default();
        let mut value="보면대709".to_owned();
        let mut id=None;
        ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                id=Some(management_search_field(
                    ui,&mut value,240.0,"검색"
                ).id);
            });
        });
        let id=id.unwrap();
        let mut state=egui::TextEdit::load_state(&ctx,id).unwrap();
        // The end index is 6 Unicode scalar values, not UTF-8 bytes.
        let end=value.chars().count();
        assert_eq!(end,6);
        state.cursor.set_char_range(Some(
            egui::text::CCursorRange::one(egui::text::CCursor::new(end))
        ));
        state.store(&ctx,id);
        assert_eq!(egui::TextEdit::load_state(&ctx,id).unwrap()
            .cursor.char_range().unwrap().primary.index,6);
    }
}

#[cfg(test)]
mod shared_input_rendering_tests{
    use super::*;
    use typography::{DataValueContext,DataValueKind};

    #[derive(Debug)]
    struct PaintedInput{
        rect:Rect,
        fonts:Vec<egui::FontId>,
    }
    fn collect_input(shape:&egui::Shape,value:&str,found:&mut Vec<PaintedInput>){
        match shape{
            egui::Shape::Text(text) if text.galley.text()==value=>{
                found.push(PaintedInput{
                    rect:text.galley.rect.translate(text.pos.to_vec2()),
                    fonts:text.galley.job.sections.iter()
                        .map(|section|section.format.font_id.clone()).collect(),
                });
            }
            egui::Shape::Vec(shapes)=>for shape in shapes{collect_input(shape,value,found);},
            _=>{}
        }
    }
    fn draw_input(ctx:&egui::Context,value:&str,
        mut draw:impl FnMut(&mut Ui)->Response)->(Response,PaintedInput,Rect){
        let mut response=None;
        let mut next=Rect::NOTHING;
        let output=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,700.0))),
            ..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(480.0);
                response=Some(draw(ui));
                next=ui.allocate_exact_size(Vec2::new(80.0,theme::INPUT_HEIGHT),Sense::hover()).0;
            });
        });
        let mut found=Vec::new();
        for shape in &output.shapes{collect_input(&shape.shape,value,&mut found);}
        let response=response.expect("real input response");
        let ink=found.into_iter().find(|ink|response.rect.contains(ink.rect.center()))
            .unwrap_or_else(||panic!("missing rendered input value {value}"));
        (response,ink,next)
    }
    fn check_font(ink:&PaintedInput,font:&egui::FontId){
        assert!(!ink.fonts.is_empty());
        assert!(ink.fonts.iter().all(|actual|actual==font),
            "rendered font runs {:?} differ from {font:?}",ink.fonts);
    }
    fn check_stable(before:&(Response,PaintedInput,Rect),
        after:&(Response,PaintedInput,Rect)){
        assert_layout_rects_invariant(
            &[("input",before.0.rect),("following row",before.2)],
            &[("input",after.0.rect),("following row",after.2)]
        );
        // Galley layout can round to device pixels; the control allocation
        // retains the separate, stricter 0.5pt geometry contract.
        assert!((before.1.rect.min-after.1.rect.min).length()<=1.0,
            "input text moved: {:?} -> {:?}",before.1.rect,after.1.rect);
        assert!((before.1.rect.max-after.1.rect.max).length()<=1.0,
            "input text metrics changed across focus/permission state");
    }
    #[test]
    fn shared_form_fields_keep_rendered_font_alignment_and_geometry_across_focus(){
        for (kind,value,role) in [
            (FieldKind::Text,"BD 709",None),
            (FieldKind::Address,"서울 종로구",None),
            (FieldKind::Telephone,"010-1234-5678",Some(DataValueKind::Phone)),
            (FieldKind::Quantity,"123",Some(DataValueKind::Quantity)),
            (FieldKind::Money,"12,345",Some(DataValueKind::Money)),
        ]{
            let ctx=test_context();
            let mut draft=value.to_owned();
            let mut render=|enabled|draw_input(&ctx,value,|ui|field(ui,&mut draft,kind,enabled));
            let normal=render(true);
            let expected=role.map_or(egui::FontId::proportional(14.0),
                |role|typography::data_value_font(role,DataValueContext::Form));
            assert_eq!(expected.size,14.0);
            check_font(&normal.1,&expected);
            assert_eq!(normal.0.rect.height(),theme::INPUT_HEIGHT);
            let margin=typography::form_input_text_margin();
            let right=matches!(kind,FieldKind::Quantity|FieldKind::Money);
            let inset=if right{normal.0.rect.right()-normal.1.rect.right()}
                else{normal.1.rect.left()-normal.0.rect.left()};
            let expected_inset=f32::from(if right{margin.right}else{margin.left});
            assert!((inset-expected_inset).abs()<=1.0,
                "{kind:?} alignment inset {inset} != {expected_inset}");
            let expected_y=normal.0.rect.center().y+f32::from(margin.top-margin.bottom)/2.0;
            assert!((normal.1.rect.center().y-expected_y).abs()<=1.0,
                "{kind:?} baseline does not honor shared vertical margin");
            ctx.memory_mut(|memory|memory.request_focus(normal.0.id));
            let focused=render(true);
            assert!(focused.0.has_focus());
            check_font(&focused.1,&expected);
            check_stable(&normal,&focused);
            ctx.memory_mut(|memory|memory.surrender_focus(focused.0.id));
            let blurred=render(true);
            assert!(!blurred.0.has_focus());
            check_stable(&normal,&blurred);
            let disabled=render(false);
            assert!(!disabled.0.enabled());
            check_font(&disabled.1,&expected);
            check_stable(&normal,&disabled);
            assert_eq!(draft,value,"focus/permission changes must not edit data");
        }
    }
    #[test]
    fn shared_recipient_editor_preserves_actual_text_when_linked_or_focused(){
        for (phone,value) in [(false,"김수취"),(true,"010-1234-5678")]{
            let ctx=test_context();
            let mut draft=value.to_owned();
            let expected=if phone{
                typography::data_value_font(DataValueKind::Phone,DataValueContext::Reception)
            }else{egui::FontId::proportional(16.0)};
            assert_eq!(expected,egui::FontId::proportional(16.0));
            let normal=draw_input(&ctx,value,|ui|{
                ui.add_sized(Vec2::new(260.0,theme::INPUT_HEIGHT),
                    reception_recipient_text_edit(&mut draft,260.0,false,phone).interactive(false))
            });
            check_font(&normal.1,&expected);
            for active in [false,true]{
                let edited=draw_input(&ctx,value,|ui|{
                    let (cell,_)=ui.allocate_exact_size(
                        Vec2::new(260.0,theme::INPUT_HEIGHT),Sense::hover()
                    );
                    reception_recipient_inline_cell_editor(
                        ui,cell,&mut draft,true,Some(cell.center()),phone,active
                    )
                });
                check_font(&edited.1,&expected);
                check_stable(&normal,&edited);
            }
            assert_eq!(draft,value);
        }
    }
    #[test]
    fn shared_date_editor_renders_proportional_font_and_centered_value(){
        let ctx=test_context();
        let mut date="2026-11-25".to_owned();
        let initial=date.clone();
        let normal=draw_input(&ctx,&initial,|ui|
            management_date_text_field(ui,&mut date,180.0,theme::INPUT_HEIGHT,"날짜")
        );
        check_font(&normal.1,&egui::FontId::proportional(14.0));
        assert!((normal.1.rect.center().x-normal.0.rect.center().x).abs()<=1.0);
        let margin=typography::management_date_text_margin();
        let y=normal.0.rect.center().y+f32::from(margin.top-margin.bottom)/2.0;
        assert!((normal.1.rect.center().y-y).abs()<=1.0);
        ctx.memory_mut(|memory|memory.request_focus(normal.0.id));
        let focused=draw_input(&ctx,&initial,|ui|
            management_date_text_field(ui,&mut date,180.0,theme::INPUT_HEIGHT,"날짜")
        );
        assert!(focused.0.has_focus());
        check_stable(&normal,&focused);
        assert_eq!(date,initial);
    }
}

#[cfg(test)]
mod inline_first_click_caret_tests {
    use super::*;
    #[test]
    fn hangul_and_right_aligned_amount_use_native_galley_cursor(){
        let ctx=egui::Context::default();
        let mut selections=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                for (value,numeric) in [("가나다라마바사",false),("12000",true)]{
                    let (cell,_)=ui.allocate_exact_size(
                        Vec2::new(220.0,theme::INPUT_HEIGHT),Sense::hover()
                    );
                    let click=egui::pos2(
                        if numeric{cell.right()-29.0}else{cell.left()+16.0},
                        cell.center().y
                    );
                    let mut editing=value.to_owned();
                    let response=reception_inline_cell_editor(
                        ui,cell,&mut editing,true,Some(click),numeric
                    );
                    let state=egui::TextEdit::load_state(ui.ctx(),response.id)
                        .expect("native caret state");
                    let range=state.cursor.char_range().expect("first caret");
                    assert_eq!(range.primary.index,range.secondary.index);
                    assert!(range.primary.index<=value.chars().count());
                    selections.push(range.primary.index);
                }
            });
        });
        assert_eq!(selections.len(),2);
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

#[cfg(test)]
mod management_toolbar_geometry_tests {
    use super::*;
    #[test]
    fn all_freight_headers_share_34pt_text_area_and_2pt_bottom_rule(){
        let ctx=egui::Context::default();
        let mut observed=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let header=Rect::from_min_size(
                    egui::pos2(16.0,216.0),Vec2::new(900.0,36.0)
                );
                let content=Rect::from_min_max(
                    header.min,egui::pos2(header.right()-10.0,header.bottom())
                );
                let text=crate::table::master_header_begin(ui,header,content);
                crate::table::header_finish(ui,header);
                observed=Some((header,content,text));
            });
        });
        let (header,content,text)=observed.expect("header geometry");
        assert_eq!(text.top(),header.top());
        assert_eq!(text.bottom(),header.bottom()-2.0);
        assert_eq!(text.height(),34.0);
        assert_eq!(text.right(),content.right());
        assert_eq!(header.center().y-text.center().y,1.0,
            "full-height centering must not be reused");
        assert_eq!(theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT,2.0);
    }

    #[test]
    fn master_table_keeps_symmetric_padding_without_moving_card_border(){
        let outer_height=800.0;
        let regular=management_card_min_height_for_insets(
            outer_height,theme::SECTION_PADDING,theme::SECTION_PADDING
        );
        let master=management_card_min_height_for_insets(
            outer_height,theme::SECTION_PADDING,
            theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING
        );
        assert_eq!(master,regular);
        assert_eq!(theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING,
            theme::SECTION_PADDING);
        let regular_outer=regular+2.0*theme::FRAME_STROKE_WIDTH
            +2.0*theme::SECTION_PADDING;
        let master_outer=master+2.0*theme::FRAME_STROKE_WIDTH
            +theme::SECTION_PADDING
            +theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING;
        assert_eq!(regular_outer,outer_height);
        assert_eq!(master_outer,outer_height);
    }

    #[test]
    fn product_name_column_header_has_one_short_shared_label(){
        // Freight Assignment and Product List must both use this semantic
        // column label; the product-description value stays in the row.
        assert_eq!(ManagementTableColumnLabel::ProductName.text(),"상품명");
    }

    #[test]
    fn table_checkbox_is_centered_for_freight_and_excel_column_widths(){
        // The 44pt and 48pt first columns differ in width but NOT their
        // widget-centering rule; no 8pt/4pt shrink offsets are permitted.
        for width in [44.0,48.0] {
            let cell=Rect::from_min_size(
                egui::pos2(100.0,200.0),Vec2::new(width,42.0)
            );
            let rect=management_table_checkbox_rect(cell,cell,18.0,4.0);
            assert_eq!(rect.center(),cell.center());
            assert_eq!(rect.height(),theme::INPUT_HEIGHT);
            assert_eq!(rect.width(),22.0);
            assert_eq!(rect.left()-cell.left(),cell.right()-rect.right());
            assert_eq!(rect.top()-cell.top(),cell.bottom()-rect.bottom());
            assert!(cell.contains(rect.min)&&cell.contains(rect.max));
        }
    }

    #[test]
    fn recipient_focus_preserves_original_cell_geometry_and_contact_font_token(){
        let ctx=egui::Context::default();
        let mut measured=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                for (phone,value) in [
                    (false,"김준희"),(false,"전라남도 순천시"),
                    (true,"010-4801-4727"),(true,"061-123-4567"),
                    (false,"문 앞에 놓아주세요")
                ]{
                    let mut readonly=value.to_owned();
                    let original=ui.add_sized(
                        Vec2::new(260.0,theme::INPUT_HEIGHT),
                        reception_recipient_text_edit(
                            &mut readonly,260.0,false,phone
                        ).interactive(false)
                    );
                    let before=ui.available_rect_before_wrap().min;
                    let mut editing=value.to_owned();
                    let active=reception_recipient_inline_cell_editor(
                        ui,original.rect,&mut editing,false,None,phone,false
                    );
                    measured.push((original.rect,active.rect,before,
                        ui.available_rect_before_wrap().min));
                }
            });
        });
        for (original,focused,before,after) in measured{
            assert_eq!(focused,original,
                "focused recipient editor must not move or shrink its text box");
            assert_eq!(before,after,
                "focused recipient editor must not add another layout row");
        }
        assert_eq!(
            typography::data_value_font(
                typography::DataValueKind::Phone,
                typography::DataValueContext::Reception
            ),
            egui::FontId::proportional(typography::FONT_RECEPTION_RECIPIENT_TEXT)
        );
    }

    #[test]
    fn freight_focus_preserves_amount_height_and_bottom_inset(){
        let ctx=egui::Context::default();
        let mut measured=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let mut readonly="6,000".to_owned();
                let original=reception_freight_money_readonly(ui,&mut readonly);
                let before=ui.available_rect_before_wrap().min;
                let mut editing="6000".to_owned();
                let active=reception_freight_inline_cell_editor(
                    ui,original.rect,&mut editing,false,None
                );
                measured=Some((original.rect,active.rect,before,
                    ui.available_rect_before_wrap().min));
            });
        });
        let (original,focused,before,after)=measured.expect("freight geometry");
        assert_eq!(original,focused);
        assert_eq!(original.height(),theme::INPUT_HEIGHT);
        assert_eq!(original.width(),RECEPTION_FREIGHT_MONEY_WIDTH);
        assert_eq!(before,after,
            "freight focus cannot consume the following row's vertical padding");
    }

    #[test]
    fn inline_editor_is_in_original_cell_and_never_adds_a_layout_row(){
        let ctx=egui::Context::default();
        let mut measured=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let (cell,_)=ui.allocate_exact_size(
                    Vec2::new(240.0,theme::INPUT_HEIGHT),Sense::click()
                );
                let before=ui.available_rect_before_wrap().min;
                let mut text="바로 편집".to_owned();
                let response=reception_inline_cell_editor(
                    ui,cell,&mut text,false,None,false
                );
                let after=ui.available_rect_before_wrap().min;
                measured=Some((cell,response.rect,before,after));
            });
        });
        let (cell,input,before,after)=measured.expect("inline input layout");
        assert!(cell.contains(input.min) && cell.contains(input.max));
        assert_eq!(before,after,"inline TextEdit must not create a second row");
        assert_eq!(input.height(),cell.height()-2.0*theme::SPACE_2);
    }

    #[test]
    fn reception_value_origins_share_vertical_center_and_right_edge() {
        let cell=Rect::from_min_size(
            egui::pos2(10.0,104.0),Vec2::new(76.0,34.0)
        );
        let quantity=reception_item_value_origin(cell,Vec2::new(42.0,19.0),true);
        let short=reception_item_value_origin(cell,Vec2::new(10.0,19.0),true);
        let product=reception_item_value_origin(cell,Vec2::new(65.0,19.0),false);
        assert_eq!(quantity.x+42.0,cell.right());
        assert_eq!(short.x+10.0,cell.right());
        assert_eq!(product.x,cell.left());
        for pos in [quantity,short,product] {
            assert_eq!(pos.y+19.0/2.0,cell.center().y);
        }
    }

    #[test]
    fn reception_product_text_keeps_eight_point_gaps_on_both_sides() {
        let row=Rect::from_min_size(
            egui::pos2(0.0,0.0),Vec2::new(480.0,42.0)
        );
        let columns=reception_item_columns(row);
        let highlighted=columns[0].shrink2(
            egui::vec2(theme::SPACE_4,theme::SPACE_4)
        );
        let row_text=reception_item_text_clip(
            highlighted.shrink2(egui::vec2(theme::SPACE_4,0.0)),0
        );
        let header_text=reception_item_text_clip(
            columns[0].shrink2(egui::vec2(theme::SPACE_8,0.0)),0
        );
        assert_eq!(row_text.left()-highlighted.left(),theme::SPACE_8);
        assert_eq!(highlighted.right()-row_text.right(),theme::SPACE_8);
        assert_eq!(row_text.left(),header_text.left());
        assert_eq!(row_text.right(),header_text.right());
        assert_eq!(
            reception_item_text_clip(columns[1],1),columns[1]
        );
        assert_eq!(
            reception_item_text_clip(columns[2],2),columns[2]
        );
    }

    #[test]
    fn item_list_reserves_the_scrollbar_before_measuring_three_columns(){
        let gutter=crate::scrollbar::management_table_scroll_style().allocated_width();
        assert!(gutter>0.0);
        // A 1080p desktop can leave ~480-520pt for this docked card.
        // Adding the fifth, tenth or twentieth row must NOT change the
        // declared table width when the vertical scrollbar appears.
        for viewport in [480.0,500.0,520.0,700.0] {
            let content=management_table_data_width(
                viewport,gutter,theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH
            );
            assert_eq!(content,viewport-gutter);
            assert!(content>=theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH);
            let columns=reception_item_columns(
                Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(content,42.0))
            );
            assert_eq!(columns[2].right(),content);
            assert_eq!(columns[1].width(),theme::RECEPTION_ITEMS_QTY_WIDTH);
            assert_eq!(columns[2].width(),theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH);
        }
        // Horizontal scrolling is correct only when there is not enough
        // width for the three approved minimum columns PLUS the rail.
        let narrow=theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+gutter-1.0;
        assert_eq!(
            management_table_data_width(
                narrow,gutter,theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH
            ),theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH
        );
    }

    #[test]
    fn reception_item_columns_keep_numeric_widths_on_resize() {
        // Header and every row use the same reserved width; no data value
        // changes the column geometry.
        for viewport in [220.0,350.0,392.0,520.0,820.0] {
            let width=management_table_data_width(viewport,0.0,theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH);
            let area=Rect::from_min_size(
                egui::pos2(0.0,0.0),Vec2::new(width,42.0)
            );
            let [product,quantity,amount]=reception_item_columns(area);
            assert_eq!(quantity.width(),theme::RECEPTION_ITEMS_QTY_WIDTH);
            assert_eq!(amount.width(),theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH);
            assert!(product.width()>=theme::RECEPTION_ITEMS_PRODUCT_MIN_WIDTH);
            assert_eq!(product.right(),quantity.left());
            assert_eq!(quantity.right(),amount.left());
            assert_eq!(amount.right(),area.right());
        }
        assert_eq!(management_table_data_width(220.0,0.0,theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH),
            theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH);
    }

    #[test]
    fn reception_quantity_displays_up_to_999_then_uses_compact_indicator() {
        assert_eq!(reception_compact_quantity("1개"),"1개");
        assert_eq!(reception_compact_quantity("999개"),"999개");
        assert_eq!(reception_compact_quantity("1000개"),"999+");
        assert_eq!(reception_compact_quantity("12345개"),"999+");
        assert_eq!(reception_compact_quantity("—"),"—");
        assert_eq!(reception_compact_quantity("0개"),"0개");
        let row=Rect::from_min_size(egui::pos2(0.0,0.0),Vec2::new(600.0,42.0));
        let columns=reception_item_columns(row);
        assert_eq!(columns[1].width(),theme::RECEPTION_ITEMS_QTY_WIDTH);
        assert_eq!(columns[2].width(),theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH);
    }

    #[test]
    fn reception_product_ellipsis_respects_width_and_korean_boundaries() {
        let measure=|value:&str|value.chars().count() as f32;
        assert_eq!(reception_ellipsized_product("연주용보면대708-1",5.0,measure),"연주용보…");
        assert_eq!(reception_ellipsized_product("짧은품목",8.0,measure),"짧은품목");
        assert_eq!(reception_ellipsized_product("긴품목",1.0,measure),"…");
        assert_eq!(reception_ellipsized_product("긴품목",0.5,measure),"");
        assert_eq!(reception_ellipsized_product("긴품목",0.0,measure),"");
    }

    #[test]
    fn table_header_checkbox_is_centered_above_inner_two_point_rule(){
        let first_col=Rect::from_min_size(
            egui::pos2(100.0,300.0),Vec2::new(48.0,36.0)
        );
        let text_area=Rect::from_min_max(
            egui::pos2(0.0,300.0),
            egui::pos2(600.0,336.0-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT)
        );
        let rect=management_table_checkbox_rect(first_col,text_area,18.0,4.0);
        assert_eq!(rect.center().x,first_col.center().x);
        assert_eq!(rect.center().y,text_area.center().y);
        assert_eq!(rect.height(),34.0);
        assert_eq!(rect.bottom(),text_area.bottom());
        assert_eq!(rect.left()-first_col.left(),first_col.right()-rect.right());
        assert!(rect.bottom()<=first_col.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT);
    }

    #[test]
    fn management_table_scroll_policy_is_identical_for_every_table(){
        let bounds=Rect::from_min_size(
            egui::pos2(16.0,300.0),Vec2::new(800.0,484.0)
        );
        let rows_bottom=bounds.bottom();
        let rail=management_table_scroll_track_geometry(bounds,bounds.height());
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION,0.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET,0.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET,0.0);
        assert_eq!(rail.top(),bounds.top()); // no gap under the header
        assert_eq!(rail.bottom(),rows_bottom);
        assert_eq!(rail.bottom(),784.0);

        // Neither a list's footer nor a master card's 12pt lower inset
        // changes the shared rail offset. The scroll viewport is unchanged.
        for height in [120.0,400.0,484.0,1200.0] {
            let track=management_table_scroll_track_geometry(bounds,height);
            let viewport_bottom=(bounds.top()+height).min(bounds.bottom());
            assert_eq!(track.bottom(),viewport_bottom);
        }
        let tiny=management_table_scroll_track_geometry(bounds,1.0);
        assert!(tiny.height()>=1.0);
        assert!(tiny.top()>=bounds.top());
    }

    #[test]
    fn list_and_master_table_gutters_reserve_only_native_scrollbar_width() {
        // CONFLICT-003=A. Every role uses the same 10pt allocation;
        // none adds the previous 4pt between data and the rail.
        for (viewport,minimum) in [
            (900.0,900.0),(1100.0,1100.0),(1440.0,942.0),
            (1440.0,1242.0),
        ] {
            let native_gutter=10.0;
            let data_width=management_table_data_width(
                viewport,native_gutter,minimum
            );
            if viewport-native_gutter>=minimum {
                assert!((data_width+native_gutter-viewport).abs()<0.01);
            }else{
                assert!((data_width-minimum).abs()<0.01);
            }
        }
    }

    #[test]
    fn toolbar_to_header_transition_remains_25pt_with_preexisting_inset() {
        let top=theme::SECTION_DIVIDER_GAP;
        let rule=theme::SOFT_DIVIDER_WIDTH;
        let bottom=theme::SECTION_DIVIDER_GAP;
        assert_eq!(top+rule+bottom,25.0);
        let existing_toolbar_inset=theme::SPACE_8;
        let remaining=management_remaining_predivider_gap(existing_toolbar_inset);
        assert_eq!(remaining,4.0);
        assert_eq!(existing_toolbar_inset+remaining+rule+bottom,25.0);
        assert_eq!(management_remaining_predivider_gap(0.0),12.0);
    }

    #[test]
    fn single_list_scroll_height_reserves_footer_and_uses_card_bottom() {
        // 200pt card top + 600pt card height = 800pt bottom.
        // The toolbar and sticky header have already used 60pt.
        let full=management_single_list_scroll_height_geometry(
            800.0,260.0,700.0,0.0
        );
        let with_footer=management_single_list_scroll_height_geometry(
            800.0,260.0,700.0,72.0
        );
        let capped_by_actual_window=management_single_list_scroll_height_geometry(
            800.0,260.0,300.0,0.0
        );
        let very_short=management_single_list_scroll_height_geometry(
            410.0,402.0,20.0,36.0
        );
        assert_eq!(full,540.0);
        assert_eq!(with_footer,468.0);
        assert_eq!(capped_by_actual_window,300.0);
        assert_eq!(very_short,1.0);
    }

    #[test]
    fn trailing_slot_ends_at_card_inner_edge_without_scrollbar_subtraction() {
        for width in [400.0, 704.0, 1280.0] {
            let toolbar=Rect::from_min_size(
                egui::pos2(16.0,216.0),Vec2::new(width,124.0)
            );
            let bulk=management_toolbar_trailing_rect(toolbar,500.0,0.0,36.0);
            let dates=management_toolbar_trailing_rect(toolbar,704.0,44.0,80.0);
            for slot in [bulk,dates] {
                assert!((slot.right()-toolbar.right()).abs()<0.01);
                assert!(slot.left()>=toolbar.left()-0.01);
                assert!(slot.right()<=toolbar.right()+0.01);
            }
        }
    }
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
mod management_dropdown_unification_tests {
    #[test]
    fn searchable_single_select_matches_name_localized_form_and_alias(){
        let choices=[
            super::ManagementSearchChoice::new(
                "a".into(),"Bando".into(),"Bando 반도 별칭보면대".into()
            ),
            super::ManagementSearchChoice::new(
                "b".into(),"K&M".into(),"K&M 케이앤엠".into()
            ),
        ];
        for query in ["bando","BANDO","반도","별칭보면대","  반도  "]{
            assert!(super::management_search_choice_matches(&choices[0],query));
            assert!(!super::management_search_choice_matches(&choices[1],query));
        }
        assert!(super::management_search_choice_matches(&choices[1],"케이앤엠"));
        assert!(super::management_search_choice_matches(&choices[0],""));
        assert!(super::management_search_choice_matches(&choices[1],""));
        assert!(!super::management_search_choice_matches(&choices[0],"없음"));
    }
    #[test]
    fn searchable_dropdown_keeps_shared_trigger_and_one_line_options(){
        let source=include_str!("dropdown/mod.rs");
        let block=source.split("pub fn management_searchable_dropdown(").nth(1).unwrap()
            .split("/// All menu options").next().unwrap();
        for required in [
            "management_dropdown_trigger(",
            "management_dropdown_option_palette(menu)",
            "management_search_field(menu,&mut search,input_width,hint)",
            "management_dropdown_option(",
            "crate::overlay::show_menu(&trigger,None,|menu|{",
            "MANAGEMENT_DROPDOWN_OPTION_HEIGHT",
            "MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT",
        ]{
            assert!(block.contains(required),"{required} missing");
        }
        assert!(!block.contains("ui.vertical(|"));
    }

    use super::*;

    #[test]
    fn all_ordinary_egui_single_selectors_use_the_shared_component() {
        // These screens previously owned their own ComboBox triggers.
        // Keep UI choice styling and popup lifecycle in one component.
        let screens:[(&str,&str,usize);6]=[
            ("alias_manager",include_str!("alias_manager.rs"),3),
            ("client_manager",include_str!("client_manager.rs"),4),
            ("freight_manager",include_str!("freight_manager.rs"),5),
            // Two product brand selectors now use this shared searchable
            // variant; four ordinary product selectors retain the original.
            ("product_manager",include_str!("product_manager.rs"),4),
            ("manual_product_search",include_str!("manual_product_search.rs"),2),
            ("main",include_str!("main.rs"),1),
        ];
        // Both category selectors moved to the shared hierarchical
        // component; the six remaining standard product selectors still use
        // the original management-dropdown master.
        let products=include_str!("product_manager.rs");
        assert_eq!(
            products.matches("category_dropdown::category_dropdown(").count(),2
        );
        assert_eq!(
            products.matches("crate::dropdown::management_searchable_dropdown(").count(),2
        );
        assert!(include_str!("category_manager.rs")
            .contains("category_dropdown::category_dropdown("));
        for (name,source,minimum) in screens {
            assert!(!source.contains("ComboBox::"),
                "{name} still contains a screen-local native ComboBox");
            let shared=source.matches("management_dropdown(").count()
                +source.matches("management_status_dropdown(").count();
            assert!(shared>=minimum,
                "{name} lost a shared single-select call");
        }
    }

    #[test]
    fn modal_form_dropdown_matches_input_height_without_changing_toolbar(){
        let ctx=egui::Context::default();
        crate::typography::install(&ctx);
        let mut toolbar=Rect::NOTHING;
        let mut form=Rect::NOTHING;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                toolbar=management_dropdown(ui,"toolbar",140.0,true,|_|{}).rect;
                modal_labeled_row_with_width(ui,"홈페이지 상품 연결",150.0,|line|{
                    form=management_dropdown(line,"form",140.0,true,|_|{}).rect;
                });
            });
        });
        assert_eq!(toolbar.height(),theme::ACTION_STANDARD_HEIGHT);
        assert_eq!(form.height(),theme::INPUT_HEIGHT);
    }
    #[test]
    fn suffix_money_field_keeps_money_edit_size_and_does_not_write_unit(){
        let ctx=egui::Context::default();
        let mut response_rect=Rect::NOTHING;
        let mut amount="26000".to_owned();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                response_rect=money_field_with_unit(ui,&mut amount,true,"원",6.0).rect;
            });
        });
        assert_eq!(response_rect.height(),theme::INPUT_HEIGHT);
        assert_eq!(response_rect.width(),FieldKind::Money.preferred_width());
        assert_eq!(amount,"26,000");
        assert!(!amount.contains('원'));
        assert_eq!(typography::numeric_editor_font(),
            typography::management_table_money_font());
    }

    #[test]
    fn management_single_select_triggers_keep_the_shared_control_geometry() {
        let ctx=egui::Context::default();
        let mut rects=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(480.0);
                for (label,width,enabled) in [
                    ("접수일 기준",148.0,true),
                    ("계좌 선택",240.0,true),
                    ("비활성",148.0,false),
                ]{
                    let response=management_dropdown(
                        ui,label,width,enabled,|choices|{
                            let mut value=String::from("A");
                            management_dropdown_option(
                                choices,&mut value,String::from("A"),"선택 A");
                            management_dropdown_option(
                                choices,&mut value,String::from("B"),"선택 B");
                        }
                    );
                    rects.push((response.rect,width));
                }
            });
        });
        for (rect,width) in rects {
            assert_eq!(rect.width(),width);
            assert_eq!(rect.height(),theme::ACTION_STANDARD_HEIGHT);
        }
        assert_eq!(theme::MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT,320.0);
    }

    #[test]
    fn shared_select_options_have_real_vertical_padding_and_compact_width(){
        let ctx=egui::Context::default();
        let mut rects=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.scope(|choices|{
                    choices.spacing_mut().interact_size.y=
                        theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
                    let mut selected=false;
                    let first=management_dropdown_compact_option(
                        choices,&mut selected,false,"선불",90.0
                    );
                    let second=management_dropdown_compact_option(
                        choices,&mut selected,true,"착불",90.0
                    );
                    rects.extend([first.rect,second.rect]);
                });
            });
        });
        assert_eq!(rects.len(),2);
        for rect in rects {
            assert_eq!(rect.width(),90.0);
            assert_eq!(rect.height(),theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
        }
        assert!(theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT>=28.0);
    }

    #[test]
    fn long_and_compact_options_share_the_glyph_baseline_without_resizing_rows(){
        let ctx=egui::Context::default();
        let mut normal=Rect::NOTHING;
        let mut compact=Rect::NOTHING;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.scope(|options|{
                    options.spacing_mut().interact_size.y=
                        theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
                    let mut group=String::new();
                    normal=management_dropdown_option(
                        options,&mut group,"test".to_owned(),
                        "[TEST] 배송 그룹 03 · 접이식"
                    ).rect;
                    let mut cod=false;
                    compact=management_dropdown_compact_option(
                        options,&mut cod,false,"선불",90.0
                    ).rect;
                });
            });
        });
        assert_eq!(normal.height(),theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
        assert_eq!(compact.height(),theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
        assert_eq!(compact.width(),90.0);
        assert!(normal.width()>compact.width());
        assert_eq!(
            typography::management_dropdown_option_text_y(28.0,18.0),4.0
        );
    }

    #[test]
    fn all_dropdown_options_share_the_same_selection_notice_bridge(){
        // The common trigger registers the position; all option variants
        // notify the same bridge. No individual screen should capture a
        // popup item's pointer for a result bubble.
        let source=include_str!("dropdown/mod.rs");
        let inner=source.find("fn management_dropdown_option_inner<T:PartialEq>").unwrap();
        let first=source.find("pub fn management_dropdown_option<T:PartialEq>").unwrap();
        let end=source.len();
        assert_eq!(source[inner..first].matches("dropdown_option_selected(ui.ctx())").count(),1);
        assert_eq!(source[first..end].matches("management_dropdown_option_inner(").count(),4);
        assert!(!source[first..end].contains("dropdown_option_selected(ui.ctx())"));
        let popup=source.find("fn management_dropdown_inner(").unwrap();
        assert!(source[popup..inner].contains("crate::overlay::show_menu("));
        assert!(include_str!("overlay.rs").contains("s.popup_trigger=Some(origin)"));
    }

    #[test]
    fn selected_and_hovered_options_have_distinct_scoped_colors(){
        assert_ne!(
            theme::MANAGEMENT_DROPDOWN_SELECTED_BG,
            theme::MANAGEMENT_DROPDOWN_HOVER_BG
        );
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let original_selection=ui.visuals().selection.bg_fill;
                let original_hover=ui.visuals().widgets.hovered.weak_bg_fill;
                ui.scope(|popup|{
                    management_dropdown_option_palette(popup);
                    assert_eq!(popup.visuals().selection.bg_fill,
                        theme::MANAGEMENT_DROPDOWN_SELECTED_BG);
                    assert_eq!(popup.visuals().selection.stroke.color,theme::TEXT);
                    assert_eq!(popup.visuals().widgets.hovered.weak_bg_fill,
                        theme::MANAGEMENT_DROPDOWN_HOVER_BG);
                    assert_eq!(popup.visuals().widgets.hovered.bg_fill,
                        theme::MANAGEMENT_DROPDOWN_HOVER_BG);
                    assert_eq!(popup.visuals().widgets.active.weak_bg_fill,
                        theme::MANAGEMENT_DROPDOWN_HOVER_BG);
                });
                assert_eq!(ui.visuals().selection.bg_fill,original_selection);
                assert_eq!(ui.visuals().widgets.hovered.weak_bg_fill,original_hover);
            });
        });
    }
}

#[cfg(test)]
mod recipient_source_reverse_hover_tests {
    use super::*;

    #[test]
    fn all_five_read_only_recipient_cells_own_hover_and_ibeam_cursor() {
        let ctx=test_context();
        let mut cells=[Rect::NOTHING;5];
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(740.0);
                let (mut name,mut address,mut mobile,mut phone,mut note)=(
                    "김준희".to_owned(),"전라남도 순천시".to_owned(),
                    "010-1111-2222".to_owned(),"061-222-3333".to_owned(),
                    "문 앞에 놓아주세요".to_owned()
                );
                let responses=reception_recipient_fields(ui,&mut name,
                    &mut address,&mut mobile,&mut phone,&mut note,[false;5]);
                cells=responses.map(|response|response.rect);
            });
        });
        for (index,cell) in cells.iter().enumerate() {
            assert!(cell.width()>0.0 && cell.height()>0.0);
            let mut hovered=[false;5];
            let output=ctx.run(egui::RawInput{
                events:vec![egui::Event::PointerMoved(cell.center())],
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(740.0);
                    let (mut name,mut address,mut mobile,mut phone,mut note)=(
                        "김준희".to_owned(),"전라남도 순천시".to_owned(),
                        "010-1111-2222".to_owned(),"061-222-3333".to_owned(),
                        "문 앞에 놓아주세요".to_owned()
                    );
                    hovered=reception_recipient_fields(ui,&mut name,
                        &mut address,&mut mobile,&mut phone,&mut note,[false;5])
                        .map(|response|response.hovered());
                });
            });
            assert!(hovered[index],"recipient field {index} lost hover");
            assert_eq!(output.platform_output.cursor_icon,
                egui::CursorIcon::Text,"recipient field {index} lost I-beam cursor");
        }
    }
}

#[cfg(test)]
mod money_live_format_tests{
    use super::*;
    #[test]
    fn grouped_money_is_immediate_and_keeps_empty_or_zero(){
        for (raw,expected) in [
            ("",""),("0","0"),("100","100"),("1000","1,000"),
            ("1234567","1,234,567"),("0000100","0,000,100"),
            ("9,999","9,999"),("9007199254740991","9,007,199,254,740,991")
        ]{
            assert_eq!(formatted_money_input(raw),Some(expected.into()));
        }
        for invalid in ["-1","1.25","1e3","1 000","abc",
            "9007199254740992","9,999,999,999,999,999,999"]{
            assert_eq!(formatted_money_input(invalid),None);
        }
    }
    #[test]
    fn cursor_remapping_preserves_middle_insertions_and_selection(){
        assert_eq!(money_cursor_after_format("1000","1,000",1),1);
        assert_eq!(money_cursor_after_format("1000","1,000",2),3);
        assert_eq!(money_cursor_after_format("1000","1,000",4),5);
        assert_eq!(money_cursor_after_format("1,000","1,000",2),2);
        assert_eq!(money_cursor_after_format("1234567","1,234,567",4),5);
        let (a,b)=(money_cursor_after_format("1234567","1,234,567",2),
            money_cursor_after_format("1234567","1,234,567",6));
        assert_eq!((a,b),(3,8));
    }
    #[test]
    fn reception_money_cell_formats_without_storing_suffix_or_changing_layout(){
        let ctx=crate::ui_test_support::test_context();
        let mut amount="59000".to_owned();
        let mut observed=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let (cell,_)=ui.allocate_exact_size(Vec2::new(128.0,42.0),Sense::hover());
                let before=ui.available_rect_before_wrap().min;
                let response=reception_money_inline_cell_editor(ui,cell,&mut amount,true,None);
                observed=Some((cell,response.rect,before,ui.available_rect_before_wrap().min));
            });
        });
        let (cell,rect,before,after)=observed.unwrap();
        assert!(cell.contains(rect.min)&&cell.contains(rect.max));
        assert_eq!(before,after);
        assert_eq!(amount,"59,000");
        assert_eq!(money_edit_text(59000),amount);
        assert_eq!(parse_money_input(&amount),Some(59000));
        assert!(!amount.contains('원'));
    }
    #[test]
    fn reception_money_margin_reserves_gap_without_moving_unit(){
        let ctx=test_context();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let (margin,unit)=money_unit_margin(
                    ui,"원",reception_money_font(),theme::SPACE_8,
                    RECEPTION_MONEY_UNIT_GAP,typography::reception_cell_text_margin()
                );
                let unit_left=money_unit_left(104.0,theme::SPACE_8,unit.size().x);
                assert!(unit_left-(104.0-f32::from(margin.right))
                    >=RECEPTION_MONEY_UNIT_GAP);
                assert_eq!(margin.top,2);
                assert_eq!(margin.bottom,2);
            });
        });
    }

    #[test]
    fn reception_total_keeps_won_anchor_and_gap_for_pending_zero_and_amount(){
        fn render(total:Option<i64>,expected:&str)->(egui::Pos2,Rect){
            let ctx=test_context();
            let output=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(400.0);
                    reception_total_footer(ui,total,Some(1),"부가세 별도");
                });
            });
            let mut unit=None;
            let mut amount=None;
            for clipped in &output.shapes{
                if let egui::Shape::Text(shape)=&clipped.shape{
                    if shape.galley.text()=="원"{unit=Some(shape.pos);}
                    if shape.galley.text()==expected{
                        amount=Some(Rect::from_min_size(shape.pos,shape.galley.size()));
                    }
                }
            }
            (unit.expect("won unit must always be painted"),amount.expect("amount must be painted"))
        }
        let (anchor,pending)=render(None,RECEPTION_MONEY_PENDING);
        assert!((anchor.x-pending.right()-RECEPTION_MONEY_UNIT_GAP).abs()<=1.0);
        for (total,expected) in [(0,"0"),(3000,"3,000"),(1234567,"1,234,567")]{
            let (unit,amount)=render(Some(total),expected);
            assert_eq!(unit,anchor);
            assert!((unit.x-amount.right()-RECEPTION_MONEY_UNIT_GAP).abs()<=1.0);
        }
        assert_eq!(display_won(-1234),"-1,234원");
    }

    #[test]
    fn reception_total_review_highlights_follow_value_galleys_without_layout_drift(){
        fn render(total:i64,count:usize,review:bool)->(Vec<(String,Rect)>,Vec<Rect>){
            let ctx=test_context();
            crate::one_touch::set(&ctx,if review{crate::one_touch::Stage::Review}
                else{crate::one_touch::Stage::None},false);
            let output=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(400.0);
                    reception_total_footer(ui,Some(total),Some(count),"부가세 별도");
                });
            });
            let mut text=Vec::new();
            let mut highlights=Vec::new();
            for shape in &output.shapes{
                match &shape.shape{
                    egui::Shape::Text(t)=>text.push((t.galley.text().to_owned(),
                        Rect::from_min_size(t.pos,t.galley.size()))),
                    egui::Shape::Rect(r) if r.stroke.color==theme::ONE_TOUCH_BORDER
                        =>highlights.push(r.rect),
                    _=>{}
                }
            }
            (text,highlights)
        }
        for (total,count) in [(0,1),(3_775_000,12),(123_456_789,100)]{
            let (normal,none)=render(total,count,false);
            let (review,highlights)=render(total,count,true);
            assert_eq!(normal,review,"hover must not move text");
            assert!(none.is_empty());
            assert_eq!(highlights.len(),2);
            let bounds=|value:&str|review.iter().find(|(text,_)|text==value).unwrap().1;
            let number=display_money_number(total);
            let amount=bounds(&number).union(bounds("원"));
            let count_value=bounds(&format!("{count}건"));
            for (ink,frame) in [(amount,highlights[0]),(count_value,highlights[1])]{
                assert!(frame.contains_rect(ink),"highlight must contain the entire value");
                assert!((ink.left()-frame.left()-theme::SPACE_4).abs()<0.5);
                assert!((frame.right()-ink.right()-theme::SPACE_4).abs()<0.5);
                assert!((frame.center().y-ink.center().y).abs()<0.5);
            }
            assert!(highlights[1].left()>bounds("배송").right());
            assert!(!highlights[1].intersects(bounds("부가세 별도")));
        }
    }

    #[test]
    fn grouped_display_roundtrips_through_original_native_price_parser(){
        for raw in ["1000","1234567","0","9007199254740991"]{
            let pretty=formatted_money_input(raw).unwrap();
            assert_eq!(parse_money_input(&pretty),parse_money_input(raw));
        }
    }
}

// Modal section heading geometry is a shared contract, not a product-specific
// offset. Regression tests guard against returning to implicit egui spacing.
#[cfg(test)]
mod modal_section_heading_spacing_tests {
    use super::*;
    #[test]
    fn same_explicit_gap_both_sides_of_section_heading_rule(){
        assert_eq!(theme::MODAL_SECTION_HEADER_RULE_GAP,12.0);
        assert_eq!(theme::MODAL_SECTION_PADDING,12.0);
        assert_eq!(theme::SOFT_DIVIDER_WIDTH,1.0);
        let source=include_str!("popup/mod.rs");
        let helper=source.split("fn modal_section_heading_with_rule(").nth(1).unwrap()
            .split("/// A heading and one divider").next().unwrap();
        assert!(helper.contains("ui.spacing_mut().item_spacing.y=0.0;"));
        assert!(helper.contains("section_divider(ui);"));
        let boxed=source.split("pub fn modal_section_measured<R>(").nth(1).unwrap()
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
        let source=include_str!("input/mod.rs");
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
mod date_navigation_segmented_control_tests{
    use super::*;

    #[test]
    fn shared_navigation_child_retains_id_rect_clip_and_parent_cursor(){
        let ctx=test_context();
        let mut observed=Vec::new();
        for shared in [false,true]{
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    let (rect,_)=ui.allocate_exact_size(Vec2::new(112.0,36.0),Sense::hover());
                    let slot=rect.shrink(1.0);
                    let before=ui.available_rect_before_wrap().min;
                    let child=if shared{
                        navigation_segment_child(ui,"geometry",slot,Layout::top_down(Align::Center))
                    }else{
                        let mut child=ui.new_child(UiBuilder::new().id_salt("geometry")
                            .max_rect(slot).layout(Layout::top_down(Align::Center)));
                        child.set_clip_rect(slot.intersect(ui.clip_rect()));
                        child
                    };
                    observed.push((child.id(),child.max_rect(),child.clip_rect(),before,
                        ui.available_rect_before_wrap().min));
                });
            });
        }
        assert_eq!(observed[0],observed[1]);
        assert_eq!(observed[1].3,observed[1].4);
    }


    #[test]
    fn shipment_count_is_one_outer_frame_with_three_adjacent_segments(){
        let ctx=test_context();
        let mut value="2".to_owned();
        let mut rendered=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                rendered=Some(date_navigation_count_control(
                    ui,"test-shipment-count",&mut value,2,true,1,100
                ));
            });
        });
        let control=rendered.expect("count control must render");
        assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
        assert_eq!(control.input.rect.height(),theme::ACTION_STANDARD_HEIGHT);
        assert_eq!(control.input.rect.width(),64.0);
        let expected_input_left=control.rect.left()
            +theme::FRAME_STROKE_WIDTH+theme::CALENDAR_NAV_SIZE;
        assert!(
            (control.input.rect.left()-expected_input_left).abs()<0.5,
            "numeric editor must sit directly between the two 28pt arrow segments"
        );
        let expected_input_right=control.rect.right()
            -theme::FRAME_STROKE_WIDTH-theme::CALENDAR_NAV_SIZE;
        assert!(
            (control.input.rect.right()-expected_input_right).abs()<0.5,
            "the plus segment must directly follow the numeric editor"
        );
    }

    #[test]
    fn shipment_count_segments_do_not_overflow_in_actual_horizontal_row(){
        for (value,enabled) in [("1",true),("50",true),("100",true),("1",false)]{
            let ctx=test_context();
            let mut value=value.to_owned();
            let current=value.parse().unwrap();
            let mut rendered=None;
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.horizontal(|row|{
                        let label=form_field_label(row,"배송 건수");
                        let control=date_navigation_count_control(
                            row,"count-geometry",&mut value,current,enabled,1,100
                        );
                        let unit=form_field_label(row,"건");
                        rendered=Some((control,label.rect,unit.rect));
                    });
                });
            });
            let (control,label,unit)=rendered.unwrap();
            assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
            assert_eq!(label.height(),control.rect.height());
            assert_eq!(unit.height(),control.rect.height());
            assert!((label.center().y-control.rect.center().y).abs()<0.5);
            assert!((unit.center().y-control.rect.center().y).abs()<0.5);
            let inner=control.rect.shrink(theme::FRAME_STROKE_WIDTH);
            for segment in [control.decrement_rect,control.input.rect,control.increment_rect]{
                assert!(inner.expand(0.5).contains(segment.min)
                    && inner.expand(0.5).contains(segment.max),
                    "segment outside outer frame: {segment:?}, inner={inner:?}");
                assert!((segment.center().y-inner.center().y).abs()<0.6,
                    "segment is vertically off-center");
            }
            assert!((control.decrement_rect.right()-control.input.rect.left()).abs()<0.6);
            assert!((control.input.rect.right()-control.increment_rect.left()).abs()<0.6);
            assert!(unit.left()>=control.rect.right()-0.5,
                "parent cursor must advance exactly once past the control");
        }
    }

    #[test]
    fn count_step_symbols_are_centered_vector_strokes_in_the_rendered_control(){
        let ctx=test_context();
        let mut value="2".to_owned();
        let mut rendered=None;
        let output=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.horizontal(|row|{
                    form_field_label(row,"배송 건수");
                    rendered=Some(date_navigation_count_control(
                        row,"count-symbols",&mut value,2,true,1,100
                    ));
                    form_field_label(row,"건");
                });
            });
        });
        let control=rendered.unwrap();
        let minus=control.decrement_rect.center();
        let plus=control.increment_rect.center();
        let expected=[
            [minus-egui::vec2(4.0,0.0),minus+egui::vec2(4.0,0.0)],
            [plus-egui::vec2(4.0,0.0),plus+egui::vec2(4.0,0.0)],
            [plus-egui::vec2(0.0,4.0),plus+egui::vec2(0.0,4.0)],
        ];
        for endpoints in expected{
            assert!(output.shapes.iter().any(|paint|
                matches!(&paint.shape,egui::Shape::LineSegment{points,stroke}
                    if *points==endpoints && (stroke.width-1.8).abs()<0.01
                        && stroke.color==theme::TEXT)
            ),"missing centered count symbol stroke: {endpoints:?}");
        }
        let source=include_str!("components.rs");
        let count=source.split("pub fn date_navigation_count_control(").nth(1).unwrap()
            .split("/// Shared calendar navigation").next().unwrap();
        assert!(count.contains("management_date_text_field_impl("));
        assert!(!count.contains("numeric_editor_font("));
        assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
    }

    #[test]
    fn count_stepper_palette_distinguishes_actions_from_editor_and_disabled_end_stops(){
        let (_,fill,ink,_)=crate::button::action_style(theme::ButtonRole::Standard,true,false);
        assert_eq!((fill,ink),(theme::RAISED,theme::TEXT));
        let (_,fill,ink,_)=crate::button::action_style(theme::ButtonRole::Standard,false,false);
        assert_eq!((fill,ink),(theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_TEXT));
        assert!(theme::RAISED.r()>theme::INPUT.r());
        assert!(theme::INPUT.r()>theme::DISABLED_ACTION_BG.r());
        assert!(theme::TEXT.r()>theme::DISABLED_ACTION_TEXT.r());
    }

    #[test]
    fn count_stepper_bounds_follow_the_visible_draft_value(){
        assert_eq!(date_navigation_count_step_state("1",2,true,1,100),(false,true));
        assert_eq!(date_navigation_count_step_state("100",2,true,1,100),(true,false));
        assert_eq!(date_navigation_count_step_state("",2,true,1,100),(true,true));
        assert_eq!(date_navigation_count_step_state("0",2,true,1,100),(false,true));
        assert_eq!(date_navigation_count_step_state("101",2,true,1,100),(true,false));
        assert_eq!(date_navigation_count_step_state("2",2,false,1,100),(false,false));
    }

    #[test]
    fn single_recipient_still_allocates_name_and_one_of_one_navigator(){
        let ctx=test_context();
        let mut consumed_height=0.0;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let before=ui.cursor().min.y;
                let selected=reception_recipient_navigator(
                    ui,"권기엽",0,1,false
                );
                assert_eq!(selected,None);
                consumed_height=ui.cursor().min.y-before;
            });
        });
        assert!(
            consumed_height>=theme::SECTION_HEADER_HEIGHT-0.5,
            "the 1/1 recipient navigator must remain allocated even with both arrows disabled"
        );
    }
}

#[cfg(test)]
mod round87_modal_flow_contract_tests {
    use super::*;

    #[test]
    fn phase_two_editors_and_viewers_keep_the_shared_workspace_entry_point(){
        let sources=[
            ("alias_manager",include_str!("alias_manager.rs")),
            ("category_manager",include_str!("category_manager.rs")),
            ("client_manager",include_str!("client_manager.rs")),
            ("client_chooser",include_str!("client_chooser.rs")),
            ("freight_manager",include_str!("freight_manager.rs")),
            ("item_editor",include_str!("item_editor.rs")),
            ("main",include_str!("main.rs")),
            ("manual_product_search",include_str!("manual_product_search.rs")),
            ("manual_workspace",include_str!("manual_workspace.rs")),
            ("order_history_detail",include_str!("order_history_detail.rs")),
            ("order_history_manager",include_str!("order_history_manager.rs")),
            ("product_excel_ui",include_str!("product_excel_ui.rs")),
            ("product_manager",include_str!("product_manager.rs")),
            ("settings_view",include_str!("settings_view.rs")),
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
            ("alias",include_str!("alias_manager.rs")),
            ("manual",include_str!("manual_product_search.rs")),
            ("audit",include_str!("settings_view.rs")),
        ] {
            assert!(source.contains("crate::popup::modal_list_max_height("),"{name}");
        }
        let freight=include_str!("freight_manager.rs");
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
                || p.file_name().and_then(|v|v.to_str())==Some("components.rs"){continue;}
            let source=std::fs::read_to_string(&p).expect("read Rust source");
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
mod shared_general_screen_audit_tests {
    use super::*;

    fn scan(path:&std::path::Path,violations:&mut Vec<String>){
        for entry in std::fs::read_dir(path).expect("read egui source directory"){
            let p=entry.expect("source entry").path();
            if p.is_dir(){scan(&p,violations);continue;}
            if p.extension().and_then(|v|v.to_str())!=Some("rs"){continue;}
            let name=p.file_name().and_then(|v|v.to_str()).unwrap_or("");
            if matches!(name,"components.rs"|"theme.rs"){continue;}
            let source=std::fs::read_to_string(&p).expect("read source");
            for (index,line) in source.lines().enumerate(){
                let code=line.trim_start();
                if code.starts_with("//") || code.starts_with("/*") || code.starts_with('*'){continue;}
                // One existing native caret fixture intentionally tests egui
                // selection state, not an operating screen.
                if name=="product_manager.rs"
                    && code=="let response=ui.add(egui::TextEdit::singleline(&mut value));"
                    && source.lines().skip(index.saturating_sub(12)).take(12.min(index)).any(|l|
                        l.contains("fn focused_price_cell_selects_existing_amount_for_direct_replacement(")){
                    continue;
                }
                for token in [
                    "Button::new(","Frame::new(","TextEdit::singleline(",
                    "TextEdit::multiline(","Checkbox::new(","ComboBox::",
                    ".button(",".checkbox(",".text_edit_singleline(",
                    ".text_edit_multiline(","ScrollStyle::solid(","ScrollStyle::floating(",
                ] {
                    if let Some(pos)=code.find(token){
                        if !code[..pos].contains('"'){
                            violations.push(format!("{}:{}: {}",p.display(),index+1,token));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn new_and_existing_screen_modules_use_shared_controls_and_surfaces(){
        let mut violations=Vec::new();
        scan(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),&mut violations);
        assert!(violations.is_empty(),"general screen UI bypass:\n{}",violations.join("\n"));
    }

    #[test]
    fn general_screen_master_geometry_and_scroll_tokens_stay_approved(){
        assert_eq!(theme::SECTION_PADDING,12.0);
        assert_eq!(theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING,12.0);
        assert_eq!(theme::MANAGEMENT_TABLE_HEADER_HEIGHT,36.0);
        assert_eq!(theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT,2.0);
        assert_eq!(theme::SECTION_DIVIDER_GAP,12.0);
        assert_eq!(theme::SECTION_DIVIDER_HEIGHT,25.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION,0.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET,0.0);
        let standard=crate::scrollbar::management_scroll_style();
        let table=crate::scrollbar::management_table_scroll_style();
        assert_eq!(standard.bar_width,10.0);
        assert_eq!(table.bar_width,standard.bar_width);
        assert_eq!(table.handle_min_length,standard.handle_min_length);
        assert_eq!(table.bar_inner_margin,0.0);
        assert_eq!(table.bar_outer_margin,0.0);
    }

    #[test]
    fn all_reception_source_entry_points_share_the_native_editor_configuration(){
        let source=include_str!("main.rs");
        let manual=include_str!("manual_source_view.rs");
        let components=include_str!("input/mod.rs");
        assert_eq!(source.matches("crate::input::reception_source_text_edit(").count(),3);
        assert!(!source.contains("TextEdit::multiline("));
        assert!(components.contains(
            ".margin(egui::Margin::same(theme::SOURCE_DOCUMENT_TEXT_MARGIN))"
        ));
        assert!(manual.contains("theme::SOURCE_DOCUMENT_TEXT_MARGIN"));
        assert_eq!(theme::RECEPTION_PANE_PADDING_Y,theme::SECTION_PADDING);
        assert_eq!(theme::SOURCE_DOCUMENT_TEXT_MARGIN,0);
    }
}

#[cfg(test)]
pub(crate) mod general_screen_geometry_tests {
    use super::*;
    use std::sync::Arc;
    use crate::ui_test_support::{Sample,TRACE,record};
    use crate::gateway::{BandoGateway,ReadOperation};
    use crate::preview_store::WorkingSource;
    use serde_json::Value;
    // No files, network or database are accessed by fixture read workers.
    struct GeometryGateway;
    impl BandoGateway for GeometryGateway{
        fn label(&self)->&'static str{"synthetic UI geometry"}
        fn load_catalog(&self)->Result<Value,String>{Err("isolated fixture".into())}
        fn load_working_source(&self)->Result<WorkingSource,String>{Err("isolated fixture".into())}
        fn save_working_source(&self,_:&WorkingSource)->Result<u64,String>{Err("read only fixture".into())}
        fn execute(&self,_:ReadOperation,_:Value)->Result<Value,String>{Err("isolated fixture".into())}
    }
    fn render(ctx:&egui::Context,screen:Vec2,work:Rect,page:impl FnMut(&mut Ui))->Vec<Sample>{
        render_observed(ctx,screen,work,page,|_,_|{})
    }
    fn render_observed(ctx:&egui::Context,screen:Vec2,work:Rect,
        mut page:impl FnMut(&mut Ui),mut observe:impl FnMut(usize,&[Sample]))->Vec<Sample>{
        let mut measured=Vec::new();
        // egui solid scrollbars interpolate their reserved space. Three
        // frames at 60 Hz captured different fractions (e.g. 8.75/10pt)
        // depending on whether an existing scrollbar was already visible.
        // Advance actual input time, keep the normal production animation,
        // and require the last two measured frames to agree. This is a
        // settled-state check, NOT a relaxation of the 0.5pt tolerance.
        let mut time=ctx.input(|input|input.time);
        for frame in 0..12{
            time+=0.05;
            TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
            let _=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,screen)),
                time:Some(time),predicted_dt:0.05,
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|root|{
                    let mut host=root.new_child(egui::UiBuilder::new()
                        .id_salt("actual-management-page")
                        .max_rect(work).layout(Layout::top_down(Align::Min)));
                    host.set_clip_rect(work);
                    host.set_width(work.width());
                    page(&mut host);
                    record("page content",host.min_rect());
                });
            });
            let next=TRACE.with(|trace|trace.borrow_mut().take().unwrap());
            observe(frame,&next);
            if frame==11{
                let slots=|trace:&[Sample]|trace.iter().copied().filter(|(role,_)|
                    matches!(*role,"top content"|"master card"|"section card"|"table header"|"toolbar transition"|"scroll rail"|"row viewport"|"account viewport")).collect::<Vec<_>>();
                assert_layout_rects_invariant(&slots(&measured),&slots(&next));
            }
            measured=next;
        }
        measured
    }
    fn screen_trace(name:&str,tab:usize,loaded:bool,screen:Vec2,work:Rect)->Vec<Sample>{
        let ctx=test_context();
        let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
        match name{
            "freight"=>{let mut p=crate::freight_manager::FreightManager::geometry_fixture(gateway,ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui))},
            "history"=>{let mut p=crate::order_history_manager::OrderHistoryManager::geometry_fixture(gateway,ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui))},
            "products"=>{let mut p=crate::product_manager::ProductManager::geometry_fixture(gateway,ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui))},
            "clients"=>{let mut p=crate::client_manager::ClientManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui,Arc::clone(&gateway)))},
            "excel"=>{let mut p=crate::excel_manager::ExcelManager::geometry_fixture(gateway,ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui))},
            "settings"=>{let mut p=crate::settings_view::SettingsView::geometry_fixture(Arc::clone(&gateway),ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui,Arc::clone(&gateway)))},
            "settlement"=>{let mut p=crate::daily_settlement_manager::DailySettlementManager::geometry_fixture(gateway,ctx.clone(),tab,loaded);render(&ctx,screen,work,|ui|p.show_page(ui))},
            _=>panic!("unregistered screen {name}")
        }
    }
    fn first(trace:&[Sample],name:&str)->Rect{
        trace.iter().find(|(kind,_)|*kind==name)
            .unwrap_or_else(||panic!("missing actual {name} allocation")).1
    }
    fn verify_rects(name:&str,tab:usize,work:Rect,trace:&[Sample]){
        assert!(trace.iter().any(|(kind,_)|*kind=="top content"),"{name}/{tab}: missing title/top content");
        for (kind,rect) in trace{
            // Content extent measures logical data, not an allocated widget.
            // A truly empty table has zero content height; its actual viewport
            // and every protected layout rectangle must still be positive.
            let valid_size=if *kind=="row content extent"{
                rect.width()>0.0 && rect.height()>=0.0
            }else{rect.is_positive()};
            assert!([rect.left(),rect.top(),rect.right(),rect.bottom()].iter().all(|v|v.is_finite())
                && valid_size,"{name}/{tab}/{kind}: invalid {rect:?}");
            if *kind=="table header"{
                assert!((rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<=0.5,
                    "{name}/{tab}: header height {rect:?}");
            }
            if *kind=="master card" || *kind=="table header"{
                assert!(rect.left()>=work.left()-0.5 && rect.right()<=work.right()+0.5,
                    "{name}/{tab}/{kind}: horizontal overflow {rect:?} outside {work:?}");
            }
        }
    }
    #[test]
    fn actual_management_pages_allocate_valid_geometry_at_three_work_area_sizes(){
        let cases=[("freight",4),("history",1),("products",5),("clients",3),
            ("excel",3),("settings",4),("settlement",1)];
        for (screen,left) in [(Vec2::new(1920.0,1080.0),380.0),
            (Vec2::new(1366.0,768.0),320.0),(Vec2::new(1280.0,720.0),380.0)]{
            let work=Rect::from_min_max(egui::pos2(left,80.0),
                egui::pos2(screen.x-18.0,screen.y-18.0));
            for (name,tabs) in cases{
                for tab in 0..tabs{
                    for loaded in [false,true]{
                        let trace=screen_trace(name,tab,loaded,screen,work);
                        verify_rects(name,tab,work,&trace);
                    }
                }
            }
        }
    }
    #[test]
    fn actual_dual_master_table_header_and_rail_share_geometry(){
        let screen=Vec2::new(1920.0,1080.0);
        let work=Rect::from_min_max(egui::pos2(380.0,80.0),egui::pos2(1902.0,1062.0));
        for name in ["freight","history"]{
            let trace=screen_trace(name,0,true,screen,work);
            let card=first(&trace,"master card");
            let transition=first(&trace,"toolbar transition");
            let header=first(&trace,"table header");
            let rail=first(&trace,"scroll rail");
            let rows=first(&trace,"row viewport");
            let perimeter=first(&trace,"table perimeter");
            let separator=first(&trace,"table scrollbar separator");
            for (a,b) in [(transition.bottom(),header.top()),(header.bottom(),rail.top()),
                (header.bottom(),rows.top()),(rail.bottom(),rows.bottom()),
                (perimeter.bottom(),rows.bottom()),(separator.bottom(),rows.bottom()),
                (separator.center().x,header.right()-10.0),
                (card.left()+theme::SECTION_PADDING+theme::FRAME_STROKE_WIDTH,header.left()),
                (card.right()-theme::SECTION_PADDING-theme::FRAME_STROKE_WIDTH,header.right())]{
                assert!((a-b).abs()<=0.5,"{name}: master boundary drift {a} -> {b}");
            }
            assert!((transition.height()-(theme::SECTION_DIVIDER_GAP
                +theme::SOFT_DIVIDER_WIDTH+theme::SECTION_DIVIDER_GAP)).abs()<=0.5);
        }
    }

    #[test]
    fn actual_reception_four_panes_keep_their_rects_during_pending_analysis(){
        let names=["kakao pane","source pane","details pane","orders pane"];
        for screen in [Vec2::new(1920.0,1080.0),Vec2::new(1366.0,768.0),Vec2::new(1280.0,720.0)]{
            let draw=|pending:bool|{
                let ctx=test_context();
                let mut app=crate::BandoEgui::with_layout(crate::layout::LayoutState::default());
                app.gateway=Arc::new(GeometryGateway);
                app.analysis_pending=pending;
                let mut last=Vec::new();
                for _ in 0..3{
                    TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
                    let _=ctx.run(egui::RawInput{
                        screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,screen)),
                        ..Default::default()
                    },|ctx|app.reception(ctx));
                    last=TRACE.with(|trace|trace.borrow_mut().take().unwrap());
                }
                names.iter().map(|name|(*name,first(&last,name))).collect::<Vec<_>>()
            };
            let base=draw(false);
            let pending=draw(true);
            assert_layout_rects_invariant(&base,&pending);
            for pair in base.windows(2){
                assert!(pair[0].1.is_positive() && pair[1].1.is_positive());
                assert!((pair[1].1.left()-pair[0].1.right()-crate::layout::SPLITTER).abs()<=0.5,
                    "reception pane spacing drift at {screen:?}: {pair:?}");
            }
        }
    }


    // The five production single-table screens must retain their actual
    // fixed allocations while the same page receives new read results.
    // A native scrollbar may occupy horizontal space when records overflow;
    // only the row viewport's right edge is allowed to change for that reason.
    fn compare_data_state_geometry(name:&str,phase:&str,base:&[Sample],next:&[Sample],table_required:bool){
        let fixed=|samples:&[Sample]|samples.iter().copied()
            .filter(|(kind,_)|matches!(*kind,"top content"|"master card")).collect::<Vec<_>>();
        let a=fixed(base);let b=fixed(next);
        assert!(a.iter().any(|(kind,_)|*kind=="master card"),"{name}/{phase}: no master card");
        assert_eq!(a.len(),b.len(),"{name}/{phase}: fixed allocation count changed");
        for ((kind,old),(other,new)) in a.iter().zip(&b){
            assert_eq!(kind,other,"{name}/{phase}: protected slot identity changed");
            for (edge,x,y) in [("left",old.left(),new.left()),("right",old.right(),new.right()),
                ("top",old.top(),new.top()),("bottom",old.bottom(),new.bottom())]{
                assert!((x-y).abs()<=0.5,"{name}/{phase}/{kind}/{edge}: {x} -> {y}");
            }
        }
        if !table_required && !next.iter().any(|(kind,_)|*kind=="table header"){return;}
        for kind in ["toolbar transition","table header","scroll rail"]{
            let old=first(base,kind);let new=first(next,kind);
            for (edge,x,y) in [("left",old.left(),new.left()),("right",old.right(),new.right()),
                ("top",old.top(),new.top()),("bottom",old.bottom(),new.bottom())]{
                assert!((x-y).abs()<=0.5,"{name}/{phase}/{kind}/{edge}: {x} -> {y}");
            }
        }
        let old=first(base,"row viewport");let new=first(next,"row viewport");
        for (edge,x,y) in [("left",old.left(),new.left()),("top",old.top(),new.top()),
            ("bottom",old.bottom(),new.bottom())]{
            assert!((x-y).abs()<=0.5,"{name}/{phase}/row viewport/{edge}: {x} -> {y}");
        }
    }
    fn assert_visible_data_rows(name:&str,trace:&[Sample],expected:usize){
        let rows=trace.iter().filter(|(kind,_)|*kind=="data row")
            .map(|(_,rect)|*rect).collect::<Vec<_>>();
        if expected<=1{
            assert_eq!(rows.len(),expected,"{name}: actual rendered data row count");
        }else{
            assert!(rows.len()>=2 && rows.len()<expected,
                "{name}: expected virtualized populated rows, rendered {} of {expected}",rows.len());
        }
        for row in &rows{
            assert!(row.is_positive(),"{name}: invalid actual row {row:?}");
        }
        for pair in rows.windows(2){
            assert!(pair[1].top()>=pair[0].bottom()-0.5,
                "{name}: actual data rows overlap {pair:?}");
        }
    }
    // Compare the protected allocations on every sampled transition frame,
    // not just the final settled frame. Scrollbar interpolation can change
    // the row viewport's right edge; it cannot move title/header/card edges.
    fn assert_transition_frame_geometry(name:&str,frame:usize,
        settled:&[Sample],current:&[Sample],editor:bool){
        let phase=format!("transient frame {frame}");
        if editor{
            compare_top_slots(name,&phase,settled,current);
            let fixed=|trace:&[Sample]|trace.iter().find(|(role,_)|
                matches!(*role,"master card"|"section card")).map(|(_,r)|*r);
            match (fixed(settled),fixed(current)){
                (Some(a),Some(b))=>assert!((a.left()-b.left()).abs()<=0.5
                    &&(a.top()-b.top()).abs()<=0.5,
                    "{name}/{phase}: primary origin drift {a:?} -> {b:?}"),
                (None,None)=>{},
                _=>panic!("{name}/{phase}: primary card appeared/disappeared"),
            }
        }else{
            let requires_table=settled.iter().any(|(role,_)|*role=="table header");
            assert_eq!(requires_table,current.iter().any(|(role,_)|*role=="table header"),
                "{name}/{phase}: table header appeared/disappeared");
            if requires_table{
                let stable=first(settled,"row viewport");
                let now=first(current,"row viewport");
                if (stable.bottom()-now.bottom()).abs()>0.5{
                    let extent=|trace:&[Sample]|trace.iter()
                        .find(|(role,_)|*role=="row content extent").map(|(_,rect)|rect.size());
                    eprintln!("TRANSIENT_VIEWPORT_DIAGNOSTIC {name}/{phase}: settled={stable:?} current={now:?} settled_content={:?} current_content={:?} settled_rail={:?} current_rail={:?}",
                        extent(settled),extent(current),first(settled,"scroll rail"),first(current,"scroll rail"));
                }
            }
            compare_data_state_geometry(name,&phase,settled,current,requires_table);
        }
    }
    fn render_transition(ctx:&egui::Context,screen:Vec2,work:Rect,
        page:impl FnMut(&mut Ui),name:&str,editor:bool,check:bool)->Vec<Sample>{
        let mut frames=Vec::new();
        let settled=render_observed(ctx,screen,work,page,|frame,trace|{
            if check{frames.push((frame,trace.to_vec()));}
        });
        for (frame,trace) in &frames{
            assert_transition_frame_geometry(name,*frame,&settled,trace,editor);
        }
        settled
    }
    #[test]
    #[should_panic(expected="transient frame 2")]
    fn transient_detector_rejects_one_frame_drift_even_if_final_frame_recovers(){
        let card=Rect::from_min_size(egui::pos2(16.0,100.0),Vec2::new(500.0,400.0));
        let stable=[("master card",card)];
        let shifted=[("master card",card.translate(Vec2::new(0.0,2.0)))];
        assert_transition_frame_geometry("synthetic",1,&stable,&stable,false);
        assert_transition_frame_geometry("synthetic",2,&stable,&shifted,false);
        assert_transition_frame_geometry("synthetic",3,&stable,&stable,false);
    }
    fn exercise_data_transitions<P>(
        name:&str,ctx:&egui::Context,screen:Vec2,work:Rect,page:&mut P,
        mut show:impl FnMut(&mut P,&mut Ui),
        begin:impl Fn(&mut P)->Box<dyn FnOnce(Result<usize,String>)>,
        failed:impl Fn(&P)->bool,
    )->Vec<Sample>{
        let mut established=false;
        let mut draw=|page:&mut P|{
            // First mount has no previous frame; subsequent state changes
            // are measured from the first frame through the settled frame.
            let trace=render_transition(ctx,screen,work,|ui|show(page,ui),name,false,established);
            established=true;
            trace
        };
        // Keep the sender alive across real render frames: this is an
        // actually pending receiver, not just a false loaded boolean.
        let finish=begin(page);
        let pending=draw(page);
        assert_visible_data_rows(name,&pending,0);
        finish(Err("합성 조회 실패: 길이가 긴 오류 메시지의 UI 밀림 검증".into()));
        let initial_error=draw(page);
        assert!(failed(page),"{name}: initial failure was not consumed");
        let finish=begin(page);
        let retry_pending=draw(page);
        assert_visible_data_rows(name,&retry_pending,0);
        assert!(!failed(page),"{name}: retry did not clear the previous error");
        finish(Ok(0));
        let empty=draw(page);
        assert!(!failed(page),"{name}: successful empty response did not clear failure");
        assert_visible_data_rows(name,&empty,0);
        for (phase,trace) in [("initial pending",&pending),("initial error",&initial_error),
            ("initial retry pending",&retry_pending)]{
            compare_data_state_geometry(name,phase,&empty,trace,name!="freight");
        }
        let finish=begin(page);
        finish(Ok(1));
        let single=draw(page);
        assert_visible_data_rows(name,&single,1);
        compare_data_state_geometry(name,"one row",&empty,&single,true);
        let finish=begin(page);
        let refreshing=draw(page);
        compare_data_state_geometry(name,"refresh pending",&empty,&refreshing,true);
        finish(Ok(64));
        let populated=draw(page);
        assert_visible_data_rows(name,&populated,64);
        compare_data_state_geometry(name,"64 rows",&empty,&populated,true);
        let finish=begin(page);
        let populated_pending=draw(page);
        assert!(!failed(page),"{name}: populated refresh must start without an error");
        compare_data_state_geometry(name,"populated refresh pending",&populated,&populated_pending,true);
        finish(Err("합성 재조회 실패: 데이터 로딩 이후 오류".into()));
        let error=draw(page);
        assert!(failed(page),"{name}: refresh failure was not consumed");
        compare_data_state_geometry(name,"refresh error",&empty,&error,true);
        let finish=begin(page);
        let retry=draw(page);
        assert!(!failed(page),"{name}: populated retry must clear failure");
        compare_data_state_geometry(name,"populated retry pending",&empty,&retry,true);
        // LatestJob (products) publishes caught worker panics as an Err
        // rather than exposing an mpsc disconnect to the view. Its staged
        // answer fixture has no sender-drop semantics.
        if name=="products"{
            finish(Err("조회 작업이 중단되었습니다.".into()));
        }else{
            drop(finish);
        }
        let disconnected=draw(page);
        assert!(failed(page),"{name}: interrupted read was not consumed");
        compare_data_state_geometry(name,"disconnected read",&empty,&disconnected,true);
        let finish=begin(page);
        let reconnecting=draw(page);
        assert!(!failed(page),"{name}: reconnect must clear the old error");
        compare_data_state_geometry(name,"reconnect pending",&empty,&reconnecting,true);
        finish(Ok(64));
        let recovered=draw(page);
        assert!(!failed(page),"{name}: retry did not clear failure");
        assert_visible_data_rows(name,&recovered,64);
        compare_data_state_geometry(name,"recovered",&empty,&recovered,true);
        // The scrollbar exception only applies to differing row counts.
        // A fully restored result must also restore the viewport right edge.
        let stable=|trace:&[Sample]|trace.iter().copied().filter(|(role,_)|
            matches!(*role,"top content"|"master card"|"toolbar transition"|"table header"|"scroll rail"|"row viewport")).collect::<Vec<_>>();
        assert_layout_rects_invariant(&stable(&populated),&stable(&recovered));
        println!("GEOMETRY_DATA_TRANSITIONS {name} {screen:?}: pending/error/empty/one/many/disconnected/recovered");
        recovered
    }
    fn data_geometry_dimensions()->[(Vec2,f32);3]{
        [(Vec2::new(1920.0,1080.0),380.0),
         (Vec2::new(1366.0,768.0),320.0),(Vec2::new(1280.0,720.0),380.0)]
    }
    #[test]
    fn actual_freight_table_keeps_fixed_slots_through_populated_read_transitions(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),
                egui::pos2(screen.x-18.0,screen.y-18.0));
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut page=crate::freight_manager::FreightManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),0,false);
            exercise_data_transitions("freight",&ctx,screen,work,&mut page,
                |p,ui|p.show_page(ui),|p|p.geometry_begin_read(),|p|p.geometry_read_failed());
        }
    }

    #[test]
    fn actual_history_table_keeps_fixed_slots_through_populated_read_transitions(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),
                egui::pos2(screen.x-18.0,screen.y-18.0));
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut page=crate::order_history_manager::OrderHistoryManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),0,false);
            let recovered=exercise_data_transitions("history",&ctx,screen,work,&mut page,
                |p,ui|p.show_page(ui),|p|p.geometry_begin_read(),|p|p.geometry_read_failed());
            page.geometry_expand_first();
            let expanded=render(&ctx,screen,work,|ui|page.show_page(ui));
            compare_data_state_geometry("history","expanded order",&recovered,&expanded,true);
            assert_visible_data_rows("history",&expanded,65);
            assert_eq!(expanded.iter().filter(|(kind,_)|*kind=="history detail row").count(),1,
                "expanded recipient detail must actually render");
        }
    }

    #[test]
    fn actual_products_table_keeps_fixed_slots_through_populated_read_transitions(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),
                egui::pos2(screen.x-18.0,screen.y-18.0));
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut page=crate::product_manager::ProductManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),0,false);
            exercise_data_transitions("products",&ctx,screen,work,&mut page,
                |p,ui|p.show_page(ui),|p|p.geometry_begin_read(),|p|p.geometry_read_failed());
        }
    }

    #[test]
    fn actual_clients_table_keeps_fixed_slots_through_populated_read_transitions(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),
                egui::pos2(screen.x-18.0,screen.y-18.0));
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut page=crate::client_manager::ClientManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),0,false);
            exercise_data_transitions("clients",&ctx,screen,work,&mut page,
                |p,ui|p.show_page(ui,Arc::clone(&gateway)),|p|p.geometry_begin_read(),|p|p.geometry_read_failed());
        }
    }

    #[test]
    fn actual_excel_table_keeps_fixed_slots_through_populated_read_transitions(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),
                egui::pos2(screen.x-18.0,screen.y-18.0));
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut page=crate::excel_manager::ExcelManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),0,false);
            exercise_data_transitions("excel",&ctx,screen,work,&mut page,
                |p,ui|p.show_page(ui),|p|p.geometry_begin_read(),|p|p.geometry_read_failed());
        }
    }


    // Multi-card editors deliberately expand their internal data regions.
    // Compare protected top allocations and actual card origins rather than
    // requiring the number of rows or an intentionally elastic list to stay fixed.
    fn compare_top_slots(name:&str,phase:&str,before:&[Sample],after:&[Sample]){
        let fixed=|trace:&[Sample]|trace.iter().copied().filter(|(kind,_)|*kind=="top content").collect::<Vec<_>>();
        let a=fixed(before);let b=fixed(after);
        assert!(!a.is_empty(),"{name}: missing measured top content");
        assert_eq!(a.len(),b.len(),"{name}/{phase}: title/tab slot count");
        assert_layout_rects_invariant(&a,&b);
    }
    fn primary_card(trace:&[Sample])->Rect{
        trace.iter().find(|(kind,_)|matches!(*kind,"master card"|"section card"))
            .expect("missing measured primary card").1
    }
    fn compare_primary_origin(name:&str,phase:&str,before:&[Sample],after:&[Sample]){
        let a=primary_card(before);let b=primary_card(after);
        assert!((a.left()-b.left()).abs()<=0.5 && (a.top()-b.top()).abs()<=0.5,
            "{name}/{phase}: primary card origin drift {a:?} -> {b:?}");
    }
    fn assert_data_kind(name:&str,trace:&[Sample],kind:&str,count:usize){
        let measured=trace.iter().filter(|(role,_)|*role==kind).map(|(_,rect)|*rect).collect::<Vec<_>>();
        assert!(!measured.is_empty(),"{name}: {kind} fixture never reached actual renderer");
        if count==1{assert_eq!(measured.len(),1,"{name}: unexpected one-record row count");}
        for rect in &measured{assert!(rect.is_positive(),"{name}: invalid {kind} {rect:?}");}
        for pair in measured.windows(2){
            assert!(pair[1].top()>=pair[0].bottom()-0.5,"{name}: {kind} overlap {pair:?}");
        }
    }
    fn exercise_editor_transitions<P>(
        name:&str,ctx:&egui::Context,screen:Vec2,work:Rect,page:&mut P,
        mut show:impl FnMut(&mut P,&mut Ui),
        begin:impl Fn(&mut P)->Box<dyn FnOnce(Result<usize,String>)>,
        failed:impl Fn(&P)->bool, data_roles:&[&str],
    )->Vec<Sample>{
        let mut established=false;
        let mut draw=|page:&mut P|{
            // First mount has no previous frame; subsequent state changes
            // are measured from the first frame through the settled frame.
            let trace=render_transition(ctx,screen,work,|ui|show(page,ui),name,true,established);
            established=true;
            trace
        };
        let finish=begin(page);
        let pending=draw(page);
        finish(Err("합성 최초 조회 오류 — 긴 문구에도 제목과 탭이 이동하지 않아야 함".into()));
        let error=draw(page);
        assert!(failed(page),"{name}: initial error not consumed");
        let finish=begin(page);
        let retry_pending=draw(page);
        assert!(!failed(page),"{name}: initial retry did not clear error");
        finish(Ok(0));
        let empty=draw(page);
        assert!(!failed(page),"{name}: empty recovery failed");
        for (phase,trace) in [("pending",&pending),("error",&error),("retry pending",&retry_pending)]{
            compare_top_slots(name,phase,&empty,trace);
        }
        begin(page)(Ok(1));
        let single=draw(page);
        begin(page)(Ok(64));
        let many=draw(page);
        for kind in data_roles{
            assert_data_kind(name,&single,kind,1);
            assert_data_kind(name,&many,kind,64);
        }
        for (phase,trace) in [("single",&single),("many",&many)]{
            compare_top_slots(name,phase,&empty,trace);
            compare_primary_origin(name,phase,&empty,trace);
        }
        let finish=begin(page);
        let refresh=draw(page);
        compare_top_slots(name,"refresh",&many,&refresh);
        compare_primary_origin(name,"refresh",&many,&refresh);
        finish(Err("합성 재조회 오류".into()));
        let failed_trace=draw(page);
        assert!(failed(page),"{name}: refresh error not consumed");
        compare_top_slots(name,"refresh error",&many,&failed_trace);
        compare_primary_origin(name,"refresh error",&many,&failed_trace);
        let finish=begin(page);
        let retry=draw(page);
        assert!(!failed(page),"{name}: refresh retry did not clear error");
        compare_top_slots(name,"retry pending",&many,&retry);
        compare_primary_origin(name,"retry pending",&many,&retry);
        if matches!(name,"categories"|"model groups"|"aliases"|"brands"){
            finish(Err("조회 작업이 중단되었습니다.".into()));
        }else{
            drop(finish);
        }
        let disconnected=draw(page);
        assert!(failed(page),"{name}: interrupted read was not consumed");
        compare_top_slots(name,"disconnected",&many,&disconnected);
        compare_primary_origin(name,"disconnected",&many,&disconnected);
        let finish=begin(page);
        let reconnecting=draw(page);
        assert!(!failed(page),"{name}: reconnect did not clear error");
        compare_top_slots(name,"reconnecting",&many,&reconnecting);
        compare_primary_origin(name,"reconnecting",&many,&reconnecting);
        finish(Ok(64));
        let recovered=draw(page);
        assert!(!failed(page),"{name}: recovery failed");
        compare_top_slots(name,"recovery",&many,&recovered);
        compare_primary_origin(name,"recovery",&many,&recovered);
        let fixed=|trace:&[Sample]|trace.iter().copied().filter(|(kind,_)|
            matches!(*kind,"master card"|"section card"|"table header"|"toolbar transition"|"scroll rail"|"account viewport")).collect::<Vec<_>>();
        let a=fixed(&many);let b=fixed(&recovered);
        assert!(!a.is_empty(),"{name}: no fixed editor allocations");
        assert_layout_rects_invariant(&a,&b);
        println!("GEOMETRY_EDITOR_TRANSITIONS {name} {screen:?}");
        recovered
    }
    #[test]
    fn actual_freight_subtabs_keep_fixed_slots_through_populated_reads(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for tab in 1..4{
                let ctx=test_context();
                let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
                let mut page=crate::freight_manager::FreightManager::geometry_fixture(gateway,ctx.clone(),tab,false);
                if tab==3{
                    exercise_editor_transitions("freight basis",&ctx,screen,work,&mut page,|p,ui|p.show_page(ui),
                        |p|p.geometry_begin_read(),|p|p.geometry_read_failed(),&[]);
                }else{
                    // Initial loading is the approved placeholder for both lists.
                    exercise_data_transitions("freight",&ctx,screen,work,&mut page,|p,ui|p.show_page(ui),
                        |p|p.geometry_begin_read(),|p|p.geometry_read_failed());
                }
            }
        }
    }
    #[test]
    fn actual_catalog_subtabs_render_populated_records_and_recover_from_errors(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for (tab,name) in [(1,"categories"),(2,"model groups"),(3,"aliases"),(4,"brands")]{
                let ctx=test_context();
                let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
                let mut page=crate::product_manager::ProductManager::geometry_fixture(gateway,ctx.clone(),tab,false);
                exercise_editor_transitions(name,&ctx,screen,work,&mut page,|p,ui|p.show_page(ui),
                    |p|p.geometry_begin_read(),|p|p.geometry_read_failed(),&["catalog data row"]);
            }
        }
    }
    #[test]
    fn actual_excel_store_and_archive_tabs_render_populated_records(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for tab in 1..3{
                let ctx=test_context();
                let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
                let mut page=crate::excel_manager::ExcelManager::geometry_fixture(gateway,ctx.clone(),tab,false);
                exercise_data_transitions(if tab==1{"excel store"}else{"excel archive"},&ctx,screen,work,&mut page,
                    |p,ui|p.show_page(ui),|p|p.geometry_begin_read(),|p|p.geometry_read_failed());
            }
        }
    }
    #[test]
    fn actual_automatic_message_tabs_preserve_workspace_during_reads(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for tab in 1..3{
                let ctx=test_context();
                let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
                let mut page=crate::client_manager::ClientManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),tab,false);
                let roles:&[&str]=if tab==2{&["notice product row"]}else{&[]};
                let recovered=exercise_editor_transitions("automatic messages",&ctx,screen,work,&mut page,
                    |p,ui|p.show_page(ui,Arc::clone(&gateway)),|p|p.geometry_begin_message_read(),
                    |p|p.geometry_message_failed(),roles);
                if tab==2{
                    page.geometry_select_notice();
                    let selected=render(&ctx,screen,work,|ui|page.show_page(ui,Arc::clone(&gateway)));
                    compare_top_slots("notice","selected",&recovered,&selected);
                    compare_primary_origin("notice","selected",&recovered,&selected);
                }
            }
        }
    }
    #[test]
    fn actual_settings_tabs_keep_top_slots_during_account_and_path_reads(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for tab in 0..4{
                let ctx=test_context();
                let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
                let mut page=crate::settings_view::SettingsView::geometry_fixture(Arc::clone(&gateway),ctx.clone(),tab,false);
                let recovered=exercise_editor_transitions("settings",&ctx,screen,work,&mut page,
                    |p,ui|p.show_page(ui,Arc::clone(&gateway)),|p|p.geometry_begin_read(),|p|p.geometry_read_failed(),&[]);
                if tab==0{
                    let account=first(&recovered,"account viewport");
                    assert!(account.is_positive(),"settings accounts: missing actual scroll viewport");
                }
            }
        }
    }
    #[test]
    fn actual_settlement_renders_orders_payments_and_error_recovery(){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut page=crate::daily_settlement_manager::DailySettlementManager::geometry_fixture(gateway,ctx.clone(),0,false);
            exercise_editor_transitions("settlement",&ctx,screen,work,&mut page,
                |p,ui|p.show_page(ui),|p|p.geometry_begin_read(),|p|p.geometry_read_failed(),
                &["settlement order row","settlement payment row"]);
        }
    }
    #[test]
    fn actual_reception_sections_and_value_cells_keep_geometry_during_recipient_changes(){
        let card_roles=["source card","source header","message card","message header",
            "review card","review header","recipient card","recipient header",
            "items card","items header","total card","total header","orders card","orders header"];
        let field_roles=["recipient name","recipient address","recipient mobile","recipient phone","recipient note",
            "item product","item quantity","item amount","freight amount"];
        for (screen,_) in data_geometry_dimensions(){
            let ctx=test_context();
            let mut app=crate::BandoEgui::with_layout(crate::layout::LayoutState::default());
            app.gateway=Arc::new(GeometryGateway);
            let draw=|app:&mut crate::BandoEgui|{
                let mut measured=Vec::new();
                for _ in 0..3{
                    TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
                    let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,screen)),
                        ..Default::default()},|ctx|app.reception(ctx));
                    measured=TRACE.with(|trace|trace.borrow_mut().take().unwrap());
                }
                measured
            };
            let standby=draw(&mut app);
            app.raw_source="합성 발주원문\n검증 수취인 및 상품".into();
            app.analysis=Some(crate::reception_service::ReceptionAnalysis{
                recognition:serde_json::json!({"recipients":[
                    {"id":"geometry-r1","name":"짧은 이름","address":"합성 주소","mobile":"010-0000-0000","phone":"","note":"","freightCode":"010","items":[{"id":"geometry-i1","name":"검증 상품","quantity":1}]},
                    {"id":"geometry-r2","name":"긴 합성 수취인 이름과 추가 정보","address":"긴 합성 주소 — 실제 주소 아님","mobile":"010-0000-0000","phone":"02-0000-0000","note":"긴 합성 배송 메시지","freightCode":"020","items":[{"id":"geometry-i2","name":"아주 긴 합성 상품명과 옵션 색상","quantity":64}]}
                ]}),
                reference:vec![],client_match:Value::Null,client_candidates:vec![],
                matched_client_name:None,chosen_client_name:None,directory_count:0,linked_spans:vec![]
            });
            let first_recipient=draw(&mut app);
            let total_card=first(&first_recipient,"total card");
            let footer=first(&first_recipient,"order total footer");
            assert!(total_card.bottom() <= screen.y + 0.5,
                "order total leaves the screen: {total_card:?}");
            assert!(footer.bottom() + theme::RECEPTION_PANE_PADDING_Y <= total_card.bottom() + 0.5,
                "order total footer is clipped: {footer:?} in {total_card:?}");
            if screen.x >= 1500.0 && screen.y >= 900.0 {
                let recipient_card=first(&first_recipient,"recipient card");
                let note=first(&first_recipient,"recipient note");
                assert!(note.bottom() + theme::RECEPTION_PANE_PADDING_Y <= recipient_card.bottom() + 0.5,
                    "recipient note is clipped: {note:?} in {recipient_card:?}");
            }
            app.selected_recipient=1;
            let second_recipient=draw(&mut app);
            app.analysis_pending=true;
            let pending=draw(&mut app);
            let cards=|trace:&[Sample]|card_roles.iter().map(|role|(*role,first(trace,role))).collect::<Vec<_>>();
            let fields=|trace:&[Sample]|field_roles.iter().map(|role|(*role,first(trace,role))).collect::<Vec<_>>();
            assert_layout_rects_invariant(&cards(&standby),&cards(&first_recipient));
            assert_layout_rects_invariant(&cards(&first_recipient),&cards(&second_recipient));
            assert_layout_rects_invariant(&cards(&second_recipient),&cards(&pending));
            assert_layout_rects_invariant(&fields(&first_recipient),&fields(&second_recipient));
            assert_layout_rects_invariant(&fields(&second_recipient),&fields(&pending));
        }
    }
    #[test]
    fn docked_goods_scroll_keeps_card_and_following_slot_fixed(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let draw=|overflow:bool|render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"품목 및 운임",320.0,|_|{},|body|{
                reception_docked_goods_scroll(body,320.0,"goods-regression",true,|items|{
                    assert_eq!(items.spacing().item_spacing.y,0.0);
                    assert!(items.available_width()+0.5>=theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+theme::RECEPTION_ITEM_REMOVE_WIDTH);
                    reception_items_header(items,true);
                    items.allocate_exact_size(Vec2::new(200.0,if overflow{900.0}else{20.0}),Sense::hover());
                });
            });
            let (rect,_)=ui.allocate_exact_size(Vec2::new(100.0,20.0),Sense::hover());
            record("after goods",rect);
        });
        let short=draw(false);
        let long=draw(true);
        assert_layout_rects_invariant(
            &[("items card",first(&short,"items card")),("after goods",first(&short,"after goods"))],
            &[("items card",first(&long,"items card")),("after goods",first(&long,"after goods"))]);
    }

    #[test]
    fn goods_viewport_reserves_fixed_parent_height_even_with_oversized_content(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let draw=|overflow:bool|render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"품목 및 운임",320.0,|_|{},|body|{
                body.spacing_mut().item_spacing.y=0.0;
                reception_goods_viewport(body,docked_section_body_height(320.0),|child|{
                    child.allocate_exact_size(Vec2::new(200.0,if overflow{900.0}else{20.0}),Sense::hover());
                });
            });
            let (rect,_)=ui.allocate_exact_size(Vec2::new(100.0,20.0),Sense::hover());
            record("after goods",rect);
        });
        let short=draw(false);
        let long=draw(true);
        assert_layout_rects_invariant(
            &[("items card",first(&short,"items card")),("after goods",first(&short,"after goods"))],
            &[("items card",first(&long,"items card")),("after goods",first(&long,"after goods"))]);
    }
    #[test]
    fn reception_section_divider_keeps_12pt_to_visible_document_frame(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let trace=render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"발주 원문",320.0,|_|{},|body|{
                let document=crate::card::source_document_frame().show(body,|document|{
                    document.allocate_exact_size(
                        Vec2::new(document.available_width(),80.0),Sense::hover()
                    );
                });
                record("visible document frame",document.response.rect);
            });
        });
        let header=first(&trace,"source header");
        let divider=first(&trace,"source divider");
        let document=first(&trace,"visible document frame");
        let top_gap=divider.top()-header.bottom();
        let bottom_gap=document.top()-divider.bottom();
        assert!((top_gap-12.0).abs()<=0.5,
            "header-row-to-divider clearance drifted: {top_gap:.2}pt");
        assert!((bottom_gap-12.0).abs()<=0.5,
            "divider-to-visible-document-frame clearance drifted: {bottom_gap:.2}pt");
        let expected_chrome=theme::RECEPTION_SECTION_HEADER_TOP
            +theme::RECEPTION_PANE_PADDING_Y
            +theme::SECTION_HEADER_HEIGHT
            +33.0;
        assert!((docked_section_body_height(320.0)-(320.0-expected_chrome)).abs()<=0.5,
            "fixed-height body calculator does not reserve both divider gaps");
    }

    #[test]
    fn actual_master_card_boundary_survives_loading_to_empty_transition(){
        let screen=Vec2::new(1920.0,1080.0);
        let work=Rect::from_min_max(egui::pos2(380.0,80.0),egui::pos2(1902.0,1062.0));
        for name in ["freight","history","products","clients","excel"]{
            let before=screen_trace(name,0,false,screen,work);
            let after=screen_trace(name,0,true,screen,work);
            let a=first(&before,"master card");
            let b=first(&after,"master card");
            assert_layout_rects_invariant(&[(name,a)],&[(name,b)]);
        }
    }
}




#[cfg(test)]
mod structural_divider_contract_tests {
    use super::*;

    #[test]
    fn structural_divider_counts_implicit_spacing_only_once(){
        for spacing in [0.0,8.0,12.0] {
            let ctx=egui::Context::default();
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.spacing_mut().item_spacing.y=spacing;
                    let (before,_)=ui.allocate_exact_size(Vec2::new(100.0,24.0),Sense::hover());
                    let strip=section_divider(ui);
                    let (after,_)=ui.allocate_exact_size(Vec2::new(100.0,24.0),Sense::hover());
                    let rule_top=strip.top()+theme::SECTION_DIVIDER_GAP-spacing;
                    let rule_bottom=rule_top+theme::SOFT_DIVIDER_WIDTH;
                    assert!((rule_top-before.bottom()-12.0).abs()<=0.5);
                    assert!((after.top()-rule_bottom-12.0).abs()<=0.5);
                });
            });
        }
    }

    /// Compact record rows and fixed footer/menu boundaries are not sections.
    /// New raw-divider calls must be reviewed instead of silently adding a
    /// screen-local structural gap. This is a lexical gate, not a Rust parser.
    #[test]
    fn production_has_no_unreviewed_compact_divider_calls(){
        let allowed:&[(&str,usize)]=&[
            ("main.rs",1), // repeated shipment rows
            ("settings_view.rs",2), // repeated account and backup records
            ("alias_manager.rs",2), // repeated list/result rows
            ("brand_manager.rs",1), // repeated list rows
            ("model_group_manager.rs",4), // repeated result/registered rows, fixed footer
            ("automatic_message_manager.rs",1), // fixed footer
            ("order_history_detail.rs",1), // repeated payment records
            ("freight_manager.rs",1), // compact context menu
            ("excel_manager.rs",1), // repeated reply rows
            ("workflow_board_ui.rs",2), // compact expanded card boundaries
            ("manual_product_search.rs",1), // repeated search rows
            ("category_dropdown.rs",2), // compact menu
        ];
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations=Vec::new();
        for entry in std::fs::read_dir(root).unwrap(){
            let path=entry.unwrap().path();
            if path.extension().and_then(|s|s.to_str())!=Some("rs"){continue;}
            let name=path.file_name().unwrap().to_str().unwrap();
            if name=="components.rs"||name=="theme.rs"{continue;}
            let source=std::fs::read_to_string(&path).unwrap();
            let production=production_source_for_contract(&source);
            let count=production.lines().filter(|line|!line.trim_start().starts_with("//"))
                .map(|line|line.matches("soft_divider(").count()).sum::<usize>();
            let limit=allowed.iter().find(|(file,_)|*file==name).map(|(_,n)|*n).unwrap_or(0);
            if count>limit{violations.push(format!("{name}: {count} compact dividers, reviewed maximum {limit}"));}
            for (line_no,line) in production.lines().enumerate(){
                if line.trim_start().starts_with("//"){continue;}
                if line.contains("theme::soft_rule(")||line.contains("theme::divider(")||line.contains(".separator("){
                    violations.push(format!("{name}:{}: raw divider bypass",line_no+1));
                }
            }
        }
        assert!(violations.is_empty(),"structural divider bypass:\n{}",violations.join("\n"));
    }
}

#[cfg(test)]
mod shared_ui_base_113_contract_tests {
    use super::*;
    #[test]
    fn managed_table_screens_use_shared_scroll_constructor(){
        for (name,source) in [
            ("product",include_str!("product_manager.rs")),
            ("client",include_str!("client_manager.rs")),
            ("client chooser",include_str!("client_chooser.rs")),
            ("excel",include_str!("excel_manager.rs")),
            ("settlement",include_str!("daily_settlement_manager.rs")),
            ("history",include_str!("order_history_manager.rs")),
        ]{
            let production=production_source_for_contract(source);
            assert!(production.contains("crate::table::fixed_rows(")||production.contains("crate::table::variable_rows("),"{name}");
            assert!(!production.contains("egui::ScrollArea::both()"),"{name}: local table scroll");
        }
    }
    #[test]
    fn all_live_table_surfaces_use_common_container(){
        for (name,source,entry) in [
            ("client chooser",include_str!("client_chooser.rs"),"crate::table::show("),
            ("client",include_str!("client_manager.rs"),"crate::table::show("),
            ("product",include_str!("product_manager.rs"),"crate::table::show("),
            ("history",include_str!("order_history_manager.rs"),"crate::table::show("),
            ("settlement",include_str!("daily_settlement_manager.rs"),"crate::table::show("),
            ("excel",include_str!("excel_manager.rs"),"crate::table::show("),
            ("brand",include_str!("brand_manager.rs"),"crate::table::show("),
            ("alias",include_str!("alias_manager.rs"),"crate::table::show("),
            ("model",include_str!("model_group_manager.rs"),"crate::table::show("),
            ("freight",include_str!("freight_manager.rs"),"crate::table::show("),
            ("workflow",include_str!("workflow_board_ui.rs"),"crate::table::inline("),
            ("reception",include_str!("main.rs"),"crate::table::inline("),
            ("manual",include_str!("manual_workspace.rs"),"crate::table::inline("),
        ]{
            assert!(source.contains(entry),"{name}: missing common table container");
            assert!(!source.contains("::finish_reception_items_table("),"{name}: local table finalization");
        }
    }
    #[test]
    fn standard_text_cells_and_history_header_use_shared_painters(){
        for source in [include_str!("product_manager.rs"),include_str!("excel_manager.rs"),
            include_str!("daily_settlement_manager.rs")]{
            assert!(production_source_for_contract(source).contains("crate::table::paint_management_text_cell("));
        }
        for source in [include_str!("order_history_manager.rs"),include_str!("excel_manager.rs"),
            include_str!("daily_settlement_manager.rs")]{
            let production=production_source_for_contract(source);
            assert!(production.contains("crate::table::header_columns("));
            assert!(!production.contains("components::management_master_header_finish("));
        }
        let history=production_source_for_contract(include_str!("order_history_manager.rs"));
        assert!(history.contains("crate::table::paint_table_galley("));
    }
}
