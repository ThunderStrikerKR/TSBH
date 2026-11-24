//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, Sense, Stroke, Ui, UiBuilder, Vec2};
use chrono::{Datelike, Duration as Days, Months, NaiveDate};
use crate::{source_highlight, theme, typography};

#[cfg(test)]
pub(crate) fn test_context()->egui::Context {
    let ctx=egui::Context::default();
    theme::install(&ctx);
    typography::install(&ctx);
    ctx
}

pub const LABEL_WIDTH: f32 = theme::FIELD_LABEL_WIDTH;
pub const TABLE_ROW_HEIGHT: f32 = theme::MANAGEMENT_COMPACT_ROW_HEIGHT;
/// Render only the rows intersecting a scroll viewport, while reserving the
/// full logical height and width of the list. This works with BOTH-axis
/// scrolling, unlike a paginated list, and preserves the backing row indices.
///
/// Each caller must render each row at exactly `row_height` with no vertical
/// spacing; widgets/edits must be keyed by stable entity IDs rather than by
/// which row happened to be visible in the current frame.
pub fn virtual_fixed_rows<R>(
    scroll:egui::ScrollArea,
    ui:&mut Ui,
    content_width:f32,
    row_height:f32,
    total_rows:usize,
    add_visible:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
) -> egui::containers::scroll_area::ScrollAreaOutput<R> {
    debug_assert!(row_height>0.0 && row_height.is_finite());
    let output=scroll.show_viewport(ui,move |body,viewport|{
        body.spacing_mut().item_spacing.y=0.0;
        body.set_min_width(content_width);
        body.set_min_height(row_height*total_rows as f32);
        let first=((viewport.top()/row_height).floor().max(0.0) as usize)
            .saturating_sub(1).min(total_rows);
        let last=((viewport.bottom()/row_height).ceil().max(0.0) as usize)
            .saturating_add(1).min(total_rows);
        let top=body.max_rect().top()+first as f32*row_height;
        let visible_rect=egui::Rect::from_min_size(
            egui::pos2(body.max_rect().left(),top),
            egui::vec2(content_width,(last-first) as f32*row_height),
        );
        let clip=body.clip_rect();
        body.scope_builder(
            egui::UiBuilder::new()
                .id_salt("virtual-fixed-rows")
                .max_rect(visible_rect),
            |visible|{
                visible.spacing_mut().item_spacing.y=0.0;
                visible.set_clip_rect(clip);
                visible.skip_ahead_auto_ids(first);
                add_visible(visible,first..last)
            }
        ).inner
    });
    #[cfg(test)]
    general_screen_geometry_tests::record("row viewport",output.inner_rect);
    output
}

pub const TABLE_HEADER_HEIGHT: f32 = theme::MANAGEMENT_TABLE_HEADER_HEIGHT;

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
        egui::ScrollArea::vertical().id_salt(id)
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
                        let next=theme::action(ui,"다음 조작부",theme::ButtonRole::Standard,true);
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
                let response=slot.add(
                    egui::Label::new(typography::status(text,color)).truncate()
                );
                if let Some(tooltip)=tooltip{
                    response.on_hover_text(tooltip);
                }
            }
        }
    );
    rect
}

/// Reserve a management page's approved primary-card footprint before its
/// asynchronous data source is ready. Its real content can replace this
/// placeholder without moving the top edge of the card or lower page slots.
pub fn management_pending_primary_card(ui:&mut Ui)->Rect{
    let min_height=management_master_table_card_min_inner_height(ui);
    measured_management_master_card(ui,|panel|{
        fill_management_primary_card(panel,min_height);
    }).response.rect
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
                            if visible{theme::action(
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
                    ui.add_space(theme::SPACE_16);
                    align_management_primary_card(ui,page_origin_y);
                    let card=if use_pending_helper{
                        management_pending_primary_card(ui)
                    }else{
                        let min_height=management_master_table_card_min_inner_height(ui);
                        theme::management_master_table_frame().show(ui,|panel|{
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
        assert!(!model_groups.contains("if self.dirty(){\n                                components::soft_divider(right);"));
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
        assert!(freight.contains("components::management_pending_primary_card(ui);"));
        assert!(freight.contains("floating_notice::sync("));
        assert!(freight.contains("gateway.mutate_shipping("));

        let products=include_str!("product_manager.rs");
        assert!(products.contains("components::management_body_state_overlay("));
        assert!(products.contains("self.start_write(\"set_price\""));
        assert!(products.contains("self.start_write(\"upsert_brand\""));
        assert!(products.contains("self.start_write(\"save_alias\""));
        assert!(products.contains("components::management_pending_primary_card(ui);"));

        let settings=include_str!("settings_view.rs");
        assert!(settings.contains("if !self.loaded || self.source.is_null(){"));
        assert!(settings.contains("components::management_pending_primary_card(ui);"));
        assert!(settings.contains("self.save_bar(ui,gateway);"));

        let messages=include_str!("automatic_message_manager.rs");
        assert!(messages.contains("if !self.loaded{"));
        assert!(messages.contains("components::management_pending_primary_card(ui);"));
        assert!(messages.contains("self.start_save("));
        assert!(messages.contains("pub fn show_confirmation("));

        let excel=include_str!("product_excel_ui.rs");
        let menu=excel.split("pub fn show_menu(").nth(1).unwrap()
            .split("pub fn show_review(").next().unwrap();
        assert!(menu.contains("let enabled=!self.busy()&&allowed;"));
        assert!(menu.contains("export_file(native.as_ref())"));
        assert!(menu.contains("preview_file(native.as_ref())"));
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
        assert!(model_groups.contains("theme::action(footer,\"닫기\""));

        let aliases=include_str!("alias_manager.rs");
        assert!(aliases.contains("fn alias_diagnostic_slot("));
        assert!(aliases.contains("alias_diagnostic_slot(info,item,&warnings);"));
        assert!(aliases.contains("components::fixed_status_slot("));
        assert!(aliases.contains("detail.as_deref(),detail.as_deref(),theme::MUTED"));

        let main=include_str!("main.rs");
        assert!(main.contains("components::reception_client_summary_card("));
        assert!(main.contains("let client_card_height = if has_source"));
        assert!(main.contains("fn apply_manual_event("));
        assert!(main.contains("row.edit_history.push("));
        assert!(main.contains("row.manual_model=Some("));
    }
}

#[cfg(test)]
mod shared_screen_component_contract_tests {
    fn production_source(file:&str)->String {
        file.split("#[cfg(test)]").next().unwrap_or(file).to_owned()
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
            ("developer_ui_samples.rs",include_str!("developer_ui_samples.rs")),
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
        assert!(excel.contains("components::management_readonly_table("));
        assert!(excel.contains("components::checkbox("));

        let vat=include_str!("main.rs")
            .split("fn show_vat_settings(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(vat.contains("components::checkbox("));
        assert!(vat.contains("components::checkbox_list_row("));
        assert!(vat.contains("theme::modal_nested_frame()"));

        let messages=production_source(include_str!("automatic_message_manager.rs"));
        assert!(messages.contains("components::multiline_text_field("));

        let history=production_source(include_str!("order_history_manager.rs"));
        assert!(history.contains("components::management_date_text_field("));
        assert!(history.contains("components::multiline_text_field("));
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

/**
 * Shared management-table checkbox placement.
 *
 * `cell` is the complete first-column rectangle (44pt Freight, 48pt Excel,
 * or another approved width). `vertical_area` is the 42pt data-row rect,
 * or the fixed header's text area excluding its inner 2pt bottom outline.
 * The checkbox always follows Freight Assignment's centered 36pt/34pt
 * hit-box policy rather than a caller-specific `shrink2` or `Ui::put`.
 */
fn management_table_checkbox_rect(
    cell:Rect,vertical_area:Rect,icon_width:f32,icon_spacing:f32
)->Rect{
    debug_assert!(icon_width>=0.0&&icon_spacing>=0.0);
    egui::Rect::from_center_size(
        egui::pos2(cell.center().x,vertical_area.center().y),
        egui::vec2(
            icon_width+icon_spacing,
            vertical_area.height().min(theme::INPUT_HEIGHT)
        )
    )
}

/// Render either a fixed-header select-all checkbox or a body-row checkbox
/// with identical centering, clipping, stable identity and enabled semantics.
/// Keep selection business rules at the call site.
pub fn management_table_checkbox(
    ui:&mut Ui,cell:Rect,vertical_area:Rect,
    checked:&mut bool,enabled:bool,id:impl std::hash::Hash
)->Response {
    let rect=management_table_checkbox_rect(
        cell,vertical_area,ui.spacing().icon_width,ui.spacing().icon_spacing
    );
    let clip=cell.intersect(vertical_area).intersect(ui.clip_rect());
    let mut builder=egui::UiBuilder::new()
        .id_salt(id)
        .max_rect(rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center));
    if !enabled {
        builder=builder.disabled();
    }
    ui.scope_builder(builder,|control|{
        control.set_clip_rect(clip);
        control.checkbox(checked,"")
    }).inner
}

#[derive(Clone, Copy)]
pub enum WorkflowIcon {
    Collapsed,
    Expanded,
    PreviousStage,
    Trash,
}

/// Neutral secondary actions inside a workflow card share one compact
/// Standard-size control. Keep their existing disabled functionality.
#[derive(Clone, Copy)]
pub enum WorkflowAuxiliaryAction {
    Manage,
    Delete,
    PreviousStage,
}

/// A single painted/interactive frame for the three grey workflow controls.
/// The two glyphs and the text label are drawn only after the same disabled
/// egui Button has reserved the same 34pt height, 6pt radius and 1pt border.
pub fn workflow_auxiliary_button(ui: &mut Ui, action: WorkflowAuxiliaryAction) -> Response {
    let width = match action {
        WorkflowAuxiliaryAction::Manage => theme::WORKFLOW_MANAGE_WIDTH,
        WorkflowAuxiliaryAction::Delete => theme::WORKFLOW_TRASH_WIDTH,
        WorkflowAuxiliaryAction::PreviousStage => theme::WORKFLOW_UNDO_WIDTH,
    };
    let response = ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.add_enabled(
            false,
            egui::Button::new("")
                .min_size(Vec2::new(width, theme::WORKFLOW_AUX_CONTROL_HEIGHT))
                // These controls are intentionally disabled; use the
                // exact disabled Standard/TableSecondary master appearance.
                .fill(theme::DISABLED_ACTION_BG)
                .stroke(Stroke::new(
                    theme::FRAME_STROKE_WIDTH,theme::DISABLED_ACTION_BORDER
                ))
                .corner_radius(egui::CornerRadius::same(5)),
        )
    }).inner;

    match action {
        WorkflowAuxiliaryAction::Manage => {
            ui.painter().text(
                response.rect.center(),
                egui::Align2::CENTER_CENTER,
                "관리",
                egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
                theme::DISABLED_ACTION_TEXT,
            );
        }
        WorkflowAuxiliaryAction::Delete => paint_workflow_icon(
            ui.painter(), response.rect.center(), WorkflowIcon::Trash,
            theme::DISABLED_ACTION_TEXT,
        ),
        WorkflowAuxiliaryAction::PreviousStage => paint_workflow_icon(
            ui.painter(), response.rect.center(), WorkflowIcon::PreviousStage,
            theme::DISABLED_ACTION_TEXT,
        ),
    }
    response
}

/// Draw workflow glyphs as stable vectors; the UI font may not contain
/// Unicode chevrons, trash or curved-arrow symbols on Windows.
pub fn paint_workflow_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    icon: WorkflowIcon,
    ink: Color32,
) {
    let stroke = Stroke::new(1.55, ink);
    let p = |x: f32, y: f32| egui::pos2(center.x + x, center.y + y);
    match icon {
        WorkflowIcon::Collapsed => {
            paint_chevron_at_angle(painter,center,0.0,0.0,ink);
        }
        WorkflowIcon::Expanded => {
            paint_chevron_at_angle(
                painter,center,std::f32::consts::FRAC_PI_2,1.0,ink
            );
        }
        WorkflowIcon::PreviousStage => {
            painter.line_segment([p(-5.3, -1.6), p(-1.5, -5.2)], stroke);
            painter.line_segment([p(-5.3, -1.6), p(-1.0, 1.8)], stroke);
            painter.add(egui::Shape::line(
                vec![p(-4.4,-1.6), p(1.2,-1.6), p(4.2,1.0),p(4.2,5.0)],
                stroke
            ));
        }
        WorkflowIcon::Trash => {
            for (a,b) in [
                (p(-5.0,-4.1),p(5.0,-4.1)),
                (p(-2.9,-6.9),p(2.9,-6.9)),
                (p(-4.3,-1.5),p(-3.4,6.2)),
                (p(4.3,-1.5),p(3.4,6.2)),
                (p(-3.4,6.2),p(3.4,6.2)),
                (p(-1.3,-1.2),p(-1.3,3.9)),
                (p(1.3,-1.2),p(1.3,3.9))
            ] {painter.line_segment([a,b],stroke);}
        }
    }
}

/// A compact KakaoTalk identity mark, used only as a visual menu prefix.
/// The native Kakao HWND and order-matching logic remain unchanged.
pub fn kakao_menu_icon(ui:&mut Ui) -> Response {
    let side=theme::KAKAO_MENU_ICON_SIZE;
    let (rect,response)=ui.allocate_exact_size(Vec2::splat(side),Sense::hover());
    let painter=ui.painter();
    painter.rect_filled(rect,4.0,theme::KAKAO_ICON_YELLOW);
    let bubble=Rect::from_min_max(
        egui::pos2(rect.left()+3.6,rect.top()+4.7),
        egui::pos2(rect.right()-3.6,rect.bottom()-5.8),
    );
    painter.rect_filled(bubble,5.0,theme::KAKAO_ICON_INK);
    painter.add(egui::Shape::convex_polygon(vec![
        egui::pos2(rect.left()+6.0,rect.bottom()-6.5),
        egui::pos2(rect.left()+5.3,rect.bottom()-3.0),
        egui::pos2(rect.left()+9.2,rect.bottom()-6.3),
    ],theme::KAKAO_ICON_INK,Stroke::NONE));
    for dx in [-2.8,0.0,2.8] {
        painter.circle_filled(
            egui::pos2(rect.center().x+dx,rect.center().y-0.8),
            0.75,theme::KAKAO_ICON_YELLOW
        );
    }
    response.on_hover_text("카카오톡")
}

/// Shared checkbox control. The caller owns the checked state and its
/// business transition; enabled state, widget construction and future visual
/// policy are centralized here.
pub fn checkbox(
    ui:&mut Ui,checked:&mut bool,label:&str,enabled:bool
)->Response{
    ui.add_enabled(enabled,egui::Checkbox::new(checked,label))
}

/// One full-width, fixed-height checkbox-list row with stable identity.
/// The row layout is shared while the caller retains domain-specific updates.
pub fn checkbox_list_row(
    ui:&mut Ui,id:impl std::hash::Hash,
    checked:&mut bool,label:&str,enabled:bool,height:f32
)->Response{
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(),height),
        Layout::left_to_right(Align::Center),
        |row|{
            row.push_id(id,|cell|{
                checkbox(cell,checked,label,enabled)
            }).inner
        }
    ).inner
}

/// Shared compact icon-only action. Its 28pt hit target follows the
/// management date-navigation control; glyph, tooltip and enabled state are
/// supplied by the caller while hover paint stays in the common layer.
pub fn icon_action_button(
    ui:&mut Ui,label:&str,tooltip:&str,enabled:bool
)->Response{
    let response=ui.scope(|button|{
        button.spacing_mut().button_padding=Vec2::ZERO;
        button.add_enabled(
            enabled,
            egui::Button::new(label)
                .frame(false)
                .min_size(Vec2::splat(theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH))
        )
    }).inner;
    if enabled{
        theme::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    response.on_hover_text(tooltip)
}

/// Removable search-tag chip. The modal nested-frame master owns its fill,
/// outline and radius; only the chip's approved compact vertical inset differs.
pub fn removable_tag_chip(ui:&mut Ui,tag:&str)->Response{
    let visible=if tag.chars().count()>22{
        format!("{}…",tag.chars().take(22).collect::<String>())
    }else{tag.to_owned()};
    theme::modal_nested_frame()
        .inner_margin(egui::Margin::symmetric(8,2))
        .show(ui,|chip|{
            chip.horizontal(|line|{
                line.spacing_mut().item_spacing.x=theme::SPACE_4;
                line.label(typography::body(&visible))
                    .on_hover_text(tag);
                icon_action_button(
                    line,"×",&format!("검색 태그 '{tag}' 삭제"),true
                )
            }).inner
        }).inner
}

/// Keep paired reception actions at their content widths. Available pane
/// space stays empty rather than stretching two buttons to the pane edges.
/// The row wraps only if Korean labels cannot fit without clipping.
pub fn paired_actions(
    ui: &mut Ui,
    left: (&str, theme::ButtonRole, bool),
    right: (&str, theme::ButtonRole, bool),
) -> (Response, Response) {
    ui.horizontal_wrapped(|ui| {
        (
            theme::action(ui, left.0, left.1, left.2),
            theme::action(ui, right.0, right.1, right.2),
        )
    }).inner
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
        rect,radius,Stroke::new(1.0,theme::BORDER),
        egui::StrokeKind::Inside
    );
    // Recessed upper rim and light-catching front face.
    painter.rect_filled(face,radius,theme::RECEPTION_KEYCAP_FACE);
    painter.rect_stroke(
        face,radius,Stroke::new(1.0,theme::RECEPTION_KEYCAP_EDGE),
        egui::StrokeKind::Inside
    );
    painter.line_segment(
        [
            egui::pos2(face.left()+5.0,face.top()+1.5),
            egui::pos2(face.right()-5.0,face.top()+1.5)
        ],
        Stroke::new(1.0,theme::RECEPTION_KEYCAP_HIGHLIGHT)
    );
    painter.line_segment(
        [
            egui::pos2(rect.left()+4.0,rect.bottom()-1.7),
            egui::pos2(rect.right()-4.0,rect.bottom()-1.7)
        ],
        Stroke::new(1.0,theme::RECEPTION_KEYCAP_BOTTOM_HIGHLIGHT)
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

/// The actual content space in a fixed-height docked section. Reserve
/// exactly the existing top/bottom frame padding, title, divider and gaps.
/// Section widgets must not impose independent minimums that can spill
/// across the source/message splitter.
pub fn docked_section_body_height(section_height: f32) -> f32 {
    // Must match main.rs::section_with_header and docked_pane_frame.
    // Both sides of the section divider use the same shared 8pt clearance.
    // Keep the fixed pane rectangles unchanged and subtract both gaps once.
    let chrome = theme::RECEPTION_PANE_PADDING_Y * 2.0
        + theme::SECTION_HEADER_HEIGHT
        + theme::SECTION_CONTENT_GAP * 2.0
        + theme::SOFT_DIVIDER_WIDTH;
    (section_height - chrome).max(0.0)
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKind {
    Text,
    Telephone,
    Quantity,
    Money,
    Address,
}

impl FieldKind {
    pub fn preferred_width(self) -> f32 {
        match self {
            Self::Text => 180.0,
            Self::Telephone => 142.0,
            Self::Quantity => 68.0,
            Self::Money => 112.0,
            Self::Address => 320.0,
        }
    }
}

/// A dialog's pivot belongs to the Kakao-safe work rect itself.
/// Do NOT subtract the top-navigation height a second time: callers have
/// already removed the 62pt header and the viewport gutters.
fn modal_pivot_pos(work_area:Rect)->egui::Pos2{
    work_area.center()
}

/// OCR review uses the same KakaoTalk-excluded coordinate space as every
/// other modal. Its outer frame is sized and centered entirely inside the
/// approved work area, never the full viewport hidden behind the Kakao dock.
pub fn modal_ocr_review_rect(work_area:Rect)->Rect{
    let width=(work_area.width()*0.94).clamp(560.0,1480.0)
        .min(work_area.width().max(1.0));
    let height=(work_area.height()*0.88).clamp(450.0,960.0)
        .min(work_area.height().max(1.0));
    Rect::from_center_size(
        work_area.center(),Vec2::new(width,height)
    )
}

/// Size a fixed OCR master inside the common modal frame, not the entire
/// Window::fixed_size rectangle. Both frame margins and the border occupy
/// outer space. Using the outer height for content caused the window to grow
/// past its fixed bounds and clip the rounded bottom edge. Avoid the paint
/// clip for sizing: it may be empty during the first egui layout pass.
pub fn modal_master_content_from_ui(available:Rect,outer:Vec2)->Vec2{
    let frame_x=theme::MODAL_PANEL_PADDING_X
        +theme::FRAME_STROKE_WIDTH;
    let frame_y=theme::MODAL_PANEL_PADDING_Y
        +theme::FRAME_STROKE_WIDTH;
    let max_inner=Vec2::new(
        (outer.x-2.0*frame_x).max(0.0),
        (outer.y-2.0*frame_y).max(0.0)
    );
    Vec2::new(
        available.width().max(0.0).min(max_inner.x),
        available.height().max(0.0).min(max_inner.y)
    )
}

/// The OCR master is painted in a fixed overlay child. Its parent
/// reserves exactly the content rect once; independently positioned header,
/// image, editor and footer must NOT expand the enclosing Window's min_rect.
/// Using scope_builder directly on that parent caused the window to outgrow
/// fixed_size and lose its lower rounded frame to viewport clipping.
pub fn modal_master_overlay_ui(parent:&mut Ui,bounds:Rect,id:&'static str)->Ui{
    let inherited_clip=parent.clip_rect();
    let mut overlay=parent.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(bounds)
            .layout(Layout::top_down(Align::Min))
    );
    overlay.set_clip_rect(bounds.intersect(inherited_clip));
    overlay
}

/// A split OCR review needs room for fixed header, footer, toolbar, inset
/// and real image/text viewports. If the window is only in a sizing pass,
/// skip child construction instead of passing a negative size into egui.
pub fn modal_master_ocr_body_is_usable(body:Vec2)->bool{
    const EXTRA_PANE_HEIGHT:f32=2.0*theme::SPACE_12+4.0;
    let required_height=
        theme::MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT
        +theme::MODAL_FOOTER_HEIGHT
        +2.0*theme::MODAL_MASTER_WORKSPACE_GAP
        +theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT
        +EXTRA_PANE_HEIGHT;
    body.x>=240.0 && body.y>=required_height
}
/// Native modal geometry belongs here, not to individual screens.
/// `work_area` excludes the externally foregrounded KakaoTalk dock.
pub fn modal_window(
    title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    egui::Window::new(typography::modal_title(title))
        .title_bar(false)
        .id(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .fixed_pos(modal_pivot_pos(work_area))
        // Keep both the pivot and the actual window within the SAME safe
        // rectangle. A second header offset pushed tall dialogs upward,
        // where their top edge could be clipped by the viewport.
        .constrain_to(work_area)
        .max_size(work_area.size())
        // All app dialogs are fixed at the approved Kakao-safe center.
        // Scrolling inside a dialog is allowed; dragging and edge-resizing
        // the dialog itself are not. Individual callers must not override it.
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .frame(theme::modal_frame())
}

/// Backward-compatible title entry point. All ordinary dialogs now use
/// the approved OCR master header: one PANEL surface, one divider, title
/// LEFT and Cancel/Close RIGHT. Compact dialogs omit the second subtitle
/// row; they do not inherit the large OCR dialog's 84pt header.
pub fn modal_header(ui:&mut Ui,title:&str,close_label:&str,enabled:bool)->bool{
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_HEADER_HEIGHT),
        Sense::hover()
    );
    let close=modal_master_header_in_rect(ui,rect,title,"",close_label,enabled);
    // Keep the first body line clear of the header's bottom divider. The
    // compact dialog has no separate scrolling chrome or subtitle row.
    ui.add_space(theme::SPACE_8);
    close
}

/// Header for modal workspaces that use exact structural spacing.
/// The compact base header already allocates 8pt after its border; top it
/// up to the shared 16pt block gap, without changing existing popups.
pub fn modal_header_with_workspace_gap(
    ui:&mut Ui,title:&str,close_label:&str,enabled:bool
)->bool{
    let close=modal_header(ui,title,close_label,enabled);
    ui.add_space((theme::MODAL_MASTER_WORKSPACE_GAP-theme::SPACE_8).max(0.0));
    close
}

/// Runtime-measured rects for the common editable modal stack.
/// These are actual egui allocation results, not inferred pixel offsets.
#[derive(Clone,Debug,Default)]
pub struct ModalEditorBodyGeometry {
    pub cards:Vec<Rect>,
    pub divider:Option<Rect>,
    pub advanced_header:Option<Rect>,
}

#[derive(Clone,Debug,Default)]
pub struct ModalEditorGeometry {
    pub header:Option<Rect>,
    pub header_toolbar:Option<Rect>,
    pub body:Option<Rect>,
    pub viewport:Option<Rect>,
    pub scroll_offset_y:f32,
    pub footer:Option<Rect>,
    pub content:ModalEditorBodyGeometry,
}
impl ModalEditorGeometry {
    /// Actual boundary distances measured from allocated screen rects.
    /// The advanced divider contains the intentional 16pt on both sides.
    pub fn gaps(&self)->Vec<(&'static str,f32)>{
        let mut out=Vec::new();
        if let (Some(header),Some(card))=(self.header,self.content.cards.first()){
            out.push(("header_to_card",card.top()-header.bottom()));
        }
        if self.content.cards.len()==3{
            let cards=&self.content.cards;
            if (cards[0].top()-cards[1].top()).abs()<1.0
                && (cards[1].top()-cards[2].top()).abs()<1.0
            {
                out.push(("card_gap_1",cards[1].left()-cards[0].right()));
                out.push(("card_gap_2",cards[2].left()-cards[1].right()));
            }
        }
        if let Some(rule)=self.content.divider{
            if !self.content.cards.is_empty(){
                let bottom=self.content.cards.iter()
                    .map(Rect::bottom).fold(f32::NEG_INFINITY,f32::max);
                out.push(("card_to_rule",rule.top()
                    +theme::MODAL_MASTER_WORKSPACE_GAP-bottom));
            }
            if let Some(advanced)=self.content.advanced_header{
                out.push(("rule_to_advanced",advanced.top()
                    -(rule.top()+theme::MODAL_MASTER_WORKSPACE_GAP
                    +theme::SOFT_DIVIDER_WIDTH)));
            }
        }
        if let (Some(toolbar),Some(body))=(self.header_toolbar,self.body){
            out.push(("toolbar_to_body",body.top()
                -toolbar.bottom()-theme::SOFT_DIVIDER_WIDTH));
        }
        if let (Some(body),Some(footer))=(self.body,self.footer){
            out.push(("body_to_footer",footer.top()-body.bottom()));
        }
        out
    }
    pub fn invalid_gaps(&self,one_pixel_tolerance:f32)->Vec<(&'static str,f32)>{
        self.gaps().into_iter()
            .filter(|(_,actual)|{
                (*actual-theme::MODAL_MASTER_WORKSPACE_GAP).abs()
                    >one_pixel_tolerance.max(0.5)
            }).collect()
    }
}

/// One owner for the popup's title, bounded scrolling workspace, and
/// fixed action footer. Individual editors supply only their contents.
/// All structural gaps are allocated ONCE at the owning level.
pub struct ModalEditorWorkspace<'a> {
    ui:&'a mut Ui,
    geometry:ModalEditorGeometry,
}
impl ModalEditorWorkspace<'_> {
    pub fn header(&mut self,title:&str,close_label:&str,enabled:bool)->bool{
        let (header,_)=self.ui.allocate_exact_size(
            Vec2::new(self.ui.available_width(),theme::MODAL_HEADER_HEIGHT),
            Sense::hover()
        );
        self.geometry.header=Some(header);
        modal_master_header_in_rect(self.ui,header,title,"",close_label,enabled)
    }

    /// Header variant for workspaces with a persistent status/action toolbar.
    /// The toolbar becomes part of the header surface, so the body starts
    /// exactly one master gap below its single bottom divider.
    pub fn header_with_toolbar<R>(
        &mut self,title:&str,close_label:&str,enabled:bool,
        toolbar:impl FnOnce(&mut Ui)->R,
    )->(bool,R){
        let header_height=theme::MODAL_HEADER_HEIGHT
            +theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT
            +theme::SOFT_DIVIDER_WIDTH;
        let (header,_)=self.ui.allocate_exact_size(
            Vec2::new(self.ui.available_width(),header_height),
            Sense::hover()
        );
        self.geometry.header=Some(header);
        let toolbar_rect=modal_master_header_toolbar_rect(
            header,self.ui.ctx().pixels_per_point().max(1.0)
        );
        self.geometry.header_toolbar=Some(toolbar_rect);
        modal_master_header_with_toolbar_in_rect(
            self.ui,header,title,close_label,enabled,toolbar
        )
    }

    pub fn disable(&mut self){self.ui.disable();}

    fn before_body(&mut self){
        let top=self.ui.cursor().top();
        let target=self.geometry.header.map(|r|r.bottom()).unwrap_or(top)
            +theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
    }

    /// Fixed dashboards and search/list workspaces keep their own inner scroll
    /// regions. Only the master owns the outer inset and structural gaps.
    pub fn body_region<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        self.before_body();
        let inset=theme::MODAL_MASTER_CONTENT_INSET;
        let width=(self.ui.available_width()-2.0*inset).max(1.0);
        let body=Frame::NONE.inner_margin(egui::Margin::symmetric(inset as i8,0))
            .show(self.ui,|content|{
                content.set_width(width);
                content.spacing_mut().item_spacing.y=theme::SPACING;
                contents(content)
            });
        self.geometry.body=Some(body.response.rect);
        body.inner
    }

    /// A viewer that already has the master framed body uses its own
    /// 16pt inset exactly once. Do not wrap it in a second body_region.
    pub fn framed_region<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        self.before_body();
        let region=self.ui.scope(|host|modal_master_body(host,contents));
        self.geometry.body=Some(region.response.rect);
        region.inner
    }

    /// Ordinary single-pane forms use one bounded body scrollbar.
    pub fn scroll_region<R>(&mut self,id:&str,max_height:f32,contents:impl FnOnce(&mut Ui)->R)->R{
        self.body_region(|host|{
            egui::ScrollArea::vertical().id_salt(id)
                .max_height(max_height.max(1.0))
                .auto_shrink([false,true])
                .show(host,contents).inner
        })
    }

    fn before_footer(&mut self){
        let top=self.ui.cursor().top();
        let target=self.geometry.body.map(|r|r.bottom()).unwrap_or(top)
            +theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
    }

    pub fn footer_single<R>(&mut self,actions:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
        self.before_footer();
        let (response,rect)=modal_action_row_measured(self.ui,actions);
        self.geometry.footer=Some(rect);
        response
    }

    pub fn body<R>(
        &mut self,max_height:f32,
        contents:impl FnOnce(&mut Ui)->(R,ModalEditorBodyGeometry),
    )->R{
        // Use the *allocated header rect*, not an assumed cursor position.
        // egui can leave the flow cursor inside the reserved header widget
        // area (11pt in the previous Windows Rect audit). Adding 16pt to
        // that cursor produced only 5pt between header and first card.
        let target_top=self.geometry.header
            .map(|rect|rect.bottom()+theme::MODAL_MASTER_WORKSPACE_GAP)
            .unwrap_or(self.ui.cursor().top()+theme::MODAL_MASTER_WORKSPACE_GAP);
        let cursor_top=self.ui.cursor().top();
        self.ui.add_space((target_top-cursor_top).max(0.0));
        let result=self.ui.scope(|host|{
            let scroll=egui::ScrollArea::vertical()
                .id_salt("modal-editor-workspace-body")
                .max_height(max_height)
                .auto_shrink([false,true])
                .show(host,|scroller|{
                    Frame::NONE.inner_margin(egui::Margin::symmetric(
                        theme::MODAL_MASTER_CONTENT_INSET as i8,0
                    )).show(scroller,|body|{
                        modal_master_controlled_spacing(body);
                        contents(body)
                    }).inner
                });
            (scroll.inner,scroll.inner_rect,scroll.state.offset.y)
        });
        self.geometry.body=Some(result.response.rect);
        let ((value,measured),viewport,scroll_offset)=result.inner;
        self.geometry.viewport=Some(viewport);
        self.geometry.scroll_offset_y=scroll_offset;
        self.geometry.content=measured;
        value
    }

    pub fn footer(
        &mut self,
        leading:impl FnOnce(&mut Ui),
        trailing:impl FnOnce(&mut Ui),
    ){
        self.before_footer();
        self.geometry.footer=Some(modal_action_row_split_measured(self.ui,leading,trailing));
    }
}

/// Same master owner for sequential forms, viewers and existing dashboard
/// bodies. No additional scrollbar is imposed on a split/list workspace.
pub fn modal_flow_workspace<R>(ui:&mut Ui,contents:impl FnOnce(&mut ModalEditorWorkspace<'_>)->R)->R{
    modal_flow_workspace_measured(ui,contents).0
}

fn modal_flow_workspace_measured<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut ModalEditorWorkspace<'_>)->R,
)->(R,ModalEditorGeometry){
    ui.scope(|layout|{
        modal_master_controlled_spacing(layout);
        let mut owner=ModalEditorWorkspace{ui:layout,geometry:ModalEditorGeometry::default()};
        let value=contents(&mut owner);
        (value,owner.geometry)
    }).inner
}

pub fn modal_workspace_footer_reserve()->f32{
    theme::MODAL_FOOTER_HEIGHT+theme::MODAL_MASTER_WORKSPACE_GAP
}

pub fn modal_editor_workspace<R>(
    ui:&mut Ui,title:&str,close_label:&str,enabled:bool,
    contents:impl FnOnce(&mut ModalEditorWorkspace<'_>)->R,
)->(bool,R,ModalEditorGeometry){
    modal_master_controlled_spacing(ui);
    let (header,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_HEADER_HEIGHT),
        Sense::hover()
    );
    let close=modal_master_header_in_rect(
        ui,header,title,"",close_label,enabled
    );
    let mut owner=ModalEditorWorkspace{
        ui,geometry:ModalEditorGeometry{
            header:Some(header),..Default::default()
        },
    };
    let value=contents(&mut owner);
    (close,value,owner.geometry)
}

/// Fixed popup title region with its own background and a single
/// Freight-master DIVIDER at the bottom. This is an opt-in popup v2
/// component during the three-screen rollout: old dialogs keep their
/// approved geometry until explicitly migrated.
/// Compatibility for fixed-position compact headers during migration.
pub fn modal_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,close_label:&str,enabled:bool
)->bool {
    modal_master_header_in_rect(ui,rect,title,"",close_label,enabled)
}

/// Standard 56pt title slot. The returned event is still the original
/// user-supplied cancel/close action; this component changes appearance only.
pub fn modal_header_chrome(
    ui:&mut Ui,title:&str,close_label:&str,enabled:bool
)->bool {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_HEADER_HEIGHT),
        Sense::hover()
    );
    modal_header_in_rect(ui,rect,title,close_label,enabled)
}

/// Footer has a stable height, a subtle background and ONE upper divider.
/// Do not create extra cancel buttons inside it.
/// Backward-compatible master footer; reserve the full 36pt primary
/// button height and render a single PANEL upper rule in every dialog.
pub fn modal_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    modal_master_footer_in_rect(ui,rect,actions)
}

/// Approved OCR modal geometry, now the default structural baseline for
/// future egui dialogs. Text length and local scrollbars never move these
/// three regions. Callers choose the appropriate popup size.
#[derive(Clone,Copy,Debug)]
pub struct ModalMasterLayout {
    pub header:Rect,
    pub workspace:Rect,
    pub footer:Rect,
}

pub fn modal_master_layout(
    bounds:Rect,header_height:f32,footer_height:f32,gap:f32
)->ModalMasterLayout {
    let h=bounds.height().max(0.0);
    let title_h=header_height.max(0.0).min(h);
    let actions_h=footer_height.max(0.0).min((h-title_h).max(0.0));
    let header=Rect::from_min_size(bounds.min,Vec2::new(bounds.width(),title_h));
    let footer=Rect::from_min_size(
        egui::pos2(bounds.left(),bounds.bottom()-actions_h),
        Vec2::new(bounds.width(),actions_h)
    );
    let gap=gap.max(0.0);
    let top=(header.bottom()+gap).min(footer.top());
    let bottom=(footer.top()-gap).max(top);
    let workspace=Rect::from_min_max(
        egui::pos2(bounds.left(),top),
        egui::pos2(bounds.right(),bottom)
    );
    ModalMasterLayout{header,workspace,footer}
}

/// A single continuous PANEL surface; toolbar and split are optional.
/// Returned lines are authoritative layout anchors for clipped contents.
/// NO nested cards, doubled strokes, or independent popup color palette.
#[derive(Clone,Copy,Debug)]
pub struct ModalMasterWorkspace {
    pub toolbar_bottom:f32,
    pub divider_x:Option<f32>,
}

/// Align the master edges to physical pixels before painting the header,
/// rounded workspace, or footer. Layout still owns the original dimensions;
/// every edge moves by at most half a device pixel. All strokes and pane fills
/// must use the SAME aligned rect so anti-aliasing cannot expose a seam.
pub fn modal_master_pixel_aligned_rect(rect:Rect,pixels_per_point:f32)->Rect {
    let scale=pixels_per_point.max(1.0);
    let snap=|v:f32|(v*scale).round()/scale;
    Rect::from_min_max(
        egui::pos2(snap(rect.left()),snap(rect.top())),
        egui::pos2(snap(rect.right()),snap(rect.bottom()))
    )
}

/// Paint only the single shared work-surface background. OCR paints its
/// two content panes after this step, then uses `finish` for crisp borders.
/// This order prevents rectangular pane fills from erasing rounded corners.
pub fn modal_master_workspace_background(
    ui:&Ui,rect:Rect,toolbar_height:f32,split_ratio:Option<f32>
)->ModalMasterWorkspace {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,theme::CONTROL_RADIUS,theme::PANEL);
    let snap=|v:f32|(v*ppp).round()/ppp;
    let toolbar_bottom=snap((rect.top()+toolbar_height.max(0.0)).min(rect.bottom()));
    let divider_x=split_ratio.map(|share|{
        snap(rect.left()+rect.width()*share.clamp(0.0,1.0))
    });
    ModalMasterWorkspace{toolbar_bottom,divider_x}
}

/// Paint separators AFTER pane backgrounds and widgets, then redraw exactly
/// one outer outline as the final layer. Pixel-aligned rectangular hairlines
/// have flat endpoints; intersecting dividers therefore do not protrude into
/// the outer frame or leave doubled anti-aliased corner pixels.
pub fn modal_master_workspace_finish(
    ui:&Ui,rect:Rect,geometry:ModalMasterWorkspace
) {
    let painter=ui.painter();
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    let snap=|v:f32|(v*ppp).round()/ppp;
    let line_width=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    let outer_width=(theme::FRAME_STROKE_WIDTH*ppp).round().max(1.0)/ppp;
    let inset=outer_width.max(line_width);
    let left=snap(rect.left()+inset);
    let right=snap(rect.right()-inset);
    let top=snap(rect.top()+inset);
    let bottom=snap(rect.bottom()-inset);
    if right>left && bottom>top {
        if geometry.toolbar_bottom>rect.top() &&
            geometry.toolbar_bottom<rect.bottom() {
            let y=snap(geometry.toolbar_bottom-line_width/2.0)
                .clamp(top,(bottom-line_width).max(top));
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(left,y),egui::pos2(right,y+line_width)
                ),0.0,theme::DIVIDER
            );
        }
        if let Some(divider_x)=geometry.divider_x {
            let x=snap(divider_x-line_width/2.0)
                .clamp(left,(right-line_width).max(left));
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x,top),egui::pos2(x+line_width,bottom)
                ),0.0,theme::DIVIDER
            );
        }
    }
    painter.rect_stroke(
        rect,theme::CONTROL_RADIUS,
        Stroke::new(outer_width,theme::DIVIDER),
        egui::StrokeKind::Inside
    );
}

/// Keep the one-call API for other dialogs. Screens with separately painted
/// content can opt into background/finish without changing the master style.
pub fn modal_master_workspace_surface(
    ui:&Ui,rect:Rect,toolbar_height:f32,split_ratio:Option<f32>
)->ModalMasterWorkspace {
    let geometry=modal_master_workspace_background(
        ui,rect,toolbar_height,split_ratio
    );
    modal_master_workspace_finish(ui,rect,geometry);
    geometry
}

/// Draw the shared title/close controls inside an already allocated title row.
/// Background and divider ownership stays with the enclosing header component.
fn modal_master_header_content_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool{
    let has_description=!description.trim().is_empty();
    let heading_top=if has_description {
        rect.top()+theme::SPACE_8
    }else{
        rect.top()+((rect.height()-theme::ACTION_STANDARD_HEIGHT)/2.0).max(0.0)
    };
    let heading=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,heading_top),
        Vec2::new((rect.width()-2.0*theme::SPACE_8).max(1.0),
            theme::ACTION_STANDARD_HEIGHT)
    );
    let subtitle=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,heading.bottom()+theme::SPACE_4),
        Vec2::new(heading.width(),20.0)
    );
    let heading_clip=heading.intersect(ui.clip_rect());
    // Reserve the real close-button hitbox first; a long heading may
    // truncate, but must never push the only exit outside the modal.
    let mut close_ui=ui.new_child(
        egui::UiBuilder::new().id_salt("modal-master-heading-close")
            .max_rect(heading)
            .layout(Layout::right_to_left(Align::Center))
    );
    close_ui.set_clip_rect(heading_clip);
    let response=theme::action(&mut close_ui,close_label,theme::ButtonRole::Standard,enabled);
    let close=response.clicked();
    let title_right=(response.rect.left()-theme::SPACE_8).max(heading.left());
    if title_right>heading.left(){
        let title_rect=Rect::from_min_max(heading.min,egui::pos2(title_right,heading.bottom()));
        let mut title_ui=ui.new_child(
            egui::UiBuilder::new().id_salt("modal-master-heading-title")
                .max_rect(title_rect)
                .layout(Layout::left_to_right(Align::Center))
        );
        title_ui.set_clip_rect(title_rect.intersect(heading_clip));
        title_ui.add(egui::Label::new(typography::modal_title(title)).truncate())
            .on_hover_text(title);
    }
    if has_description{
        let subtitle_clip=subtitle.intersect(ui.clip_rect());
        ui.scope_builder(
            egui::UiBuilder::new().id_salt("modal-master-description")
                .max_rect(subtitle),
            |details|{
                details.set_clip_rect(subtitle_clip);
                details.set_width(subtitle.width());
                details.add(egui::Label::new(typography::caption(description)).truncate())
                    .on_hover_text(description);
            },
        );
    }
    close
}

/// Canonical OCR-approved popup header, used for descriptive dialogs:
/// title and top-right Cancel/Close in the first row, optional explanation
/// in the second, ONE bottom divider. Height belongs to modal_master_layout.
/// Use an empty description for dialogs with no explanatory subtitle.
pub fn modal_master_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    // A centered line at rect.bottom() is half clipped and can appear
    // darker at one end. Fill exactly one pixel-aligned INNER rule instead.
    let rule_height=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(rect.left(),rect.bottom()-rule_height),
            rect.right_bottom()
        ),0.0,theme::DIVIDER
    );
    modal_master_header_content_in_rect(
        ui,rect,title,description,close_label,enabled
    )
}

/// Toolbar bounds are inset to the same 16pt horizontal master as the body.
/// Its bottom divider is excluded from this rect and painted by the header.
fn modal_master_header_toolbar_rect(rect:Rect,ppp:f32)->Rect{
    let ppp=ppp.max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    let rule_height=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    Rect::from_min_max(
        egui::pos2(
            rect.left()+theme::MODAL_MASTER_CONTENT_INSET,
            rect.top()+theme::MODAL_HEADER_HEIGHT
        ),
        egui::pos2(
            rect.right()-theme::MODAL_MASTER_CONTENT_INSET,
            rect.bottom()-rule_height
        )
    )
}

/// Common two-row modal header: title/close first, optional workspace toolbar
/// second, and exactly one divider below both rows. The body owns the standard
/// 16pt clearance after that divider.
pub fn modal_master_header_with_toolbar_in_rect<R>(
    ui:&mut Ui,rect:Rect,title:&str,close_label:&str,enabled:bool,
    toolbar:impl FnOnce(&mut Ui)->R,
)->(bool,R){
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    let rule_height=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(rect.left(),rect.bottom()-rule_height),
            rect.right_bottom()
        ),0.0,theme::DIVIDER
    );
    let title_rect=Rect::from_min_size(
        rect.min,Vec2::new(rect.width(),theme::MODAL_HEADER_HEIGHT)
    );
    let close=modal_master_header_content_in_rect(
        ui,title_rect,title,"",close_label,enabled
    );
    let toolbar_rect=modal_master_header_toolbar_rect(rect,ppp);
    let mut toolbar_ui=ui.new_child(
        egui::UiBuilder::new().id_salt("modal-master-header-toolbar")
            .max_rect(toolbar_rect)
            .layout(Layout::left_to_right(Align::Center))
    );
    toolbar_ui.set_clip_rect(toolbar_rect.intersect(ui.clip_rect()));
    let result=toolbar(&mut toolbar_ui);
    (close,result)
}


/// A single physical-pixel aligned footer surface for both fixed OCR
/// dialogs and flow-based confirmations. Owning the rule in one place
/// prevents half-clipped line segments or doubled separator strokes.
fn paint_modal_master_footer_surface(ui:&Ui,rect:Rect) {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    let width=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    let rule_bottom=(rect.top()+width).min(rect.bottom());
    ui.painter().rect_filled(
        Rect::from_min_max(
            rect.left_top(),egui::pos2(rect.right(),rule_bottom)
        ),0.0,theme::DIVIDER
    );
}

/// Canonical quiet modal footer: one upper divider and the final action
/// anchored at the right, never an additional bottom Cancel button.
pub fn modal_master_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    paint_modal_master_footer_surface(ui,rect);
    // Primary actions are 36pt, but standard actions are 34pt. The
    // shared footer must reserve the taller control's ACTUAL hit/paint
    // height; a 34pt clip sliced 2pt off the bottom of "적용".
    let action_height=theme::ACTION_PRIMARY_HEIGHT
        .max(theme::ACTION_STANDARD_HEIGHT);
    let content=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,
            rect.top()+((rect.height()-action_height)/2.0).max(0.0)),
        Vec2::new((rect.width()-2.0*theme::SPACE_8).max(1.0),
            action_height)
    );
    let clip=content.intersect(ui.clip_rect());
    ui.scope_builder(
        egui::UiBuilder::new().id_salt("modal-master-actions")
            .max_rect(content)
            .layout(Layout::right_to_left(Align::Center)),
        |footer|{
            footer.set_clip_rect(clip);
            footer.set_width(content.width());
            actions(footer);
        },
    );
}


/// Retain the pilot API so older callers can migrate without visual churn.
#[allow(dead_code)]
pub fn modal_review_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool {
    modal_master_header_in_rect(ui,rect,title,description,close_label,enabled)
}

/// Retain the pilot API while new screens use the master naming.
#[allow(dead_code)]
pub fn modal_review_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    modal_master_footer_in_rect(ui,rect,actions);
}

/// Flow-layout sibling of modal_master_footer_in_rect for compact
/// confirmations. Shares the OCR master footer appearance and 36pt hitbox
/// while retaining egui's InnerResponse<R>, so existing confirmation state
/// machines (including .inner results) remain unchanged.
pub fn modal_action_row<R>(
    ui:&mut Ui,actions:impl FnOnce(&mut Ui)->R
)->egui::InnerResponse<R> {
    modal_action_row_measured(ui,actions).0
}

fn modal_action_row_measured<R>(
    ui:&mut Ui,actions:impl FnOnce(&mut Ui)->R
)->(egui::InnerResponse<R>,Rect) {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_FOOTER_HEIGHT),
        Sense::hover()
    );
    paint_modal_master_footer_surface(ui,rect);
    let action_height=theme::ACTION_PRIMARY_HEIGHT
        .max(theme::ACTION_STANDARD_HEIGHT);
    let area=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,
            rect.top()+((rect.height()-action_height)/2.0).max(0.0)),
        Vec2::new((rect.width()-2.0*theme::SPACE_8).max(1.0),
            action_height)
    );
    let clip=area.intersect(ui.clip_rect());
    let response=ui.scope_builder(
        egui::UiBuilder::new().id_salt("modal-master-flow-actions")
            .max_rect(area)
            .layout(Layout::right_to_left(Align::Center)),
        |buttons|{
            buttons.set_clip_rect(clip);
            buttons.set_width(area.width());
            actions(buttons)
        }
    );
    (response,rect)
}

/// Maximum scrollable editor body height after reserving the OCR-master
/// header/footer and the native modal's fixed outer margins. Screen modules
/// must not independently subtract arbitrary offsets from the full work area:
/// doing so can push the final 58pt action row outside a short window.
pub fn modal_editor_body_max_height(work_area_height:f32)->f32 {
    modal_body_height_budget(work_area_height,true)
}

/// The same structural budget for every flowing modal. Viewers without
/// a footer do not reserve a phantom action row.
pub fn modal_body_height_budget(work_area_height:f32,has_footer:bool)->f32 {
    let fixed=2.0*theme::MODAL_PANEL_PADDING_Y
        +2.0*theme::FRAME_STROKE_WIDTH
        +theme::MODAL_HEADER_HEIGHT
        +theme::MODAL_MASTER_WORKSPACE_GAP
        +if has_footer {modal_workspace_footer_reserve()} else {0.0};
    (work_area_height-fixed).max(1.0)
        .min(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
}

/// Bound a list after its search toolbar and filters have actually been
/// laid out. The parent owns the outer header and optional footer budget;
/// the caller can choose a preferred list height, never exceed the budget.
pub fn modal_list_max_height(
    body:&Ui,work_area_height:f32,preferred:f32,has_footer:bool
)->f32{
    let consumed=(body.cursor().top()-body.min_rect().top()).max(0.0);
    (modal_body_height_budget(work_area_height,has_footer)-consumed)
        .max(1.0).min(preferred.max(1.0))
}

/// Two-ended footer for an editor with both a contextual/danger action
/// and a final Save/Apply action. The OCR-master surface, rule, height and
/// button hitbox still belong to `modal_action_row`.
///
/// Independent LEFT and RIGHT slots prevent the primary action from being
/// pushed off-screen by a longer destructive label or variable-width status.
pub fn modal_action_row_split(
    ui:&mut Ui,
    leading:impl FnOnce(&mut Ui),
    trailing:impl FnOnce(&mut Ui),
) {
    modal_action_row_split_measured(ui,leading,trailing);
}

fn modal_action_row_split_measured(
    ui:&mut Ui,
    leading:impl FnOnce(&mut Ui),
    trailing:impl FnOnce(&mut Ui),
)->Rect {
    modal_action_row_measured(ui,|footer|{
        footer.columns(2,|columns|{
            columns[0].with_layout(
                Layout::left_to_right(Align::Center),|left|leading(left)
            );
            columns[1].with_layout(
                Layout::right_to_left(Align::Center),|right|trailing(right)
            );
        });
    }).1
}

pub fn modal_footer_chrome(ui:&mut Ui,actions:impl FnOnce(&mut Ui)) {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_FOOTER_HEIGHT),
        Sense::hover()
    );
    modal_footer_in_rect(ui,rect,actions);
}

/// One framed content work surface for flow-based popup bodies. Use one
/// per dialog rather than wrapping every subsection in another card.
pub fn modal_master_body<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut Ui)->R
)->R {
    let inset=theme::MODAL_SECTION_PADDING;
    let width=(ui.available_width()-2.0*inset
        -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER))
        .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
        .inner_margin(inset)
        .show(ui,|body|{
            body.set_min_width(width);
            contents(body)
        }).inner
}

/// Shared modal-section heading geometry. Both margins around its
/// 1pt rule are explicit 12pt, independent of egui's default item spacing.
/// Its content gets a separate scope to prevent an implicit extra 8pt gap.
fn modal_section_heading_with_rule(ui:&mut Ui,title:&str){
    ui.spacing_mut().item_spacing.y=0.0;
    ui.label(typography::section_heading(title));
    ui.add_space(theme::MODAL_SECTION_HEADER_RULE_GAP);
    soft_divider(ui);
    ui.add_space(theme::MODAL_SECTION_HEADER_RULE_GAP);
}

/// A heading and one divider within the modal master work surface.
/// Uses the same heading-rule-content geometry as the boxed modal section.
pub fn modal_master_body_section<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->R {
    ui.scope(|section|{
        modal_section_heading_with_rule(section,title);
        section.scope(|body|{
            body.spacing_mut().item_spacing.y=theme::SPACING;
            contents(body)
        }).inner
    }).inner
}

/// One coherent nested information section, visually related to Freight
/// Shipping-Group Assignment's PANEL/DIVIDER hierarchy. Continuous fields
/// share the same frame rather than each receiving a redundant border.
pub fn modal_section<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->R {
    modal_section_measured(ui,title,contents).inner
}

/// Exact outer card rectangle from the same canonical section renderer.
/// Do not estimate it from the inner body or content text size.
pub fn modal_section_measured<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->egui::InnerResponse<R>{
    let inner_width=(ui.available_width()-2.0*theme::MODAL_SECTION_PADDING
        -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
    Frame::new()
        .fill(theme::MODAL_SECTION_BG)
        .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER))
        .corner_radius(egui::CornerRadius::same(theme::MODAL_SECTION_RADIUS))
        .inner_margin(theme::MODAL_SECTION_PADDING)
        .show(ui,|section|{
            section.set_min_width(inner_width);
            modal_section_heading_with_rule(section,title);
            section.scope(|body|{
                body.spacing_mut().item_spacing.y=theme::SPACING;
                contents(body)
            }).inner
        })
}

/// Structural spacing for an explicitly managed modal workspace.
///
/// Eguis default item_spacing is additive to manual add_space() calls.
/// A controlled modal sets vertical spacing to zero, and structural gaps
/// are then allocated exactly once by the master components below.
/// Keep normal 8pt spacing *inside* ordinary modal_section field content.
pub fn modal_master_controlled_spacing(ui:&mut Ui){
    ui.spacing_mut().item_spacing.y=0.0;
}

/// One shared 16pt space / 1pt divider / 16pt space sequence. Call from a
/// modal_master_controlled_spacing surface to prevent implicit extra gaps.
pub fn modal_master_spaced_divider(ui:&mut Ui)->Rect{
    let gap=theme::MODAL_MASTER_WORKSPACE_GAP;
    let thickness=theme::SOFT_DIVIDER_WIDTH;
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),2.0*gap+thickness),Sense::hover()
    );
    let y=rect.top()+gap+thickness/2.0;
    ui.painter().line_segment(
        [egui::pos2(rect.left(),y),egui::pos2(rect.right(),y)],
        Stroke::new(thickness,theme::DIVIDER)
    );
    rect
}

/// Same nested-section surface for absolute-layout dialogs such as OCR.
/// Caller is responsible for fixed content hit areas and clipping.
pub fn modal_section_surface(ui:&Ui,rect:Rect,header_height:f32) {
    let painter=ui.painter();
    painter.rect_filled(
        rect,theme::MODAL_SECTION_RADIUS,theme::MODAL_SECTION_BG
    );
    let band=Rect::from_min_max(
        rect.min+Vec2::splat(1.0),
        egui::pos2(
            (rect.right()-1.0).max(rect.left()+1.0),
            (rect.top()+header_height).min(rect.bottom()).max(rect.top()+1.0)
        )
    );
    painter.rect_filled(
        band,theme::MODAL_SECTION_RADIUS,theme::MODAL_SECTION_HEADER_BG
    );
    let y=(rect.top()+header_height).min(rect.bottom());
    painter.line_segment(
        [egui::pos2(rect.left()+1.0,y),egui::pos2(rect.right()-1.0,y)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    painter.rect_stroke(
        rect,theme::MODAL_SECTION_RADIUS,
        Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER),
        egui::StrokeKind::Inside
    );
}

/// A nested decision remains an interactive Foreground window.
/// Tooltip order is reserved for transient overlays and is not a reliable
/// interaction surface for modal actions in egui 0.33.
pub fn nested_modal_window(
    title:&str,id:&'static str,work_area:egui::Rect
)->egui::Window<'static>{
    modal_window(title,id,work_area).order(egui::Order::Foreground)
}

/// Shared decision layout. The owner allocates header, body and action
/// regions in sequence, so form edits are available to the footer immediately.
/// The caller supplies contents and actions, not outer padding or separators.
pub struct DecisionWorkspace<'a>{
    ui:&'a mut Ui,
    header_bottom:Option<f32>,
    body_bottom:Option<f32>,
}

impl DecisionWorkspace<'_>{
    pub fn header(&mut self,title:&str,cancel_label:&str,enabled:bool)->bool{
        let (rect,_)=self.ui.allocate_exact_size(
            Vec2::new(self.ui.available_width(),theme::MODAL_HEADER_HEIGHT),
            Sense::hover()
        );
        self.header_bottom=Some(rect.bottom());
        modal_master_header_in_rect(self.ui,rect,title,"",cancel_label,enabled)
    }

    pub fn body<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        let top=self.ui.cursor().top();
        let target=self.header_bottom.unwrap_or(top)+theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
        let inset=theme::SPACE_8;
        let width=(self.ui.available_width()-2.0*inset).max(1.0);
        let body=Frame::NONE.inner_margin(egui::Margin::symmetric(inset as i8,0))
            .show(self.ui,|content|{
                content.set_min_width(width);
                content.spacing_mut().item_spacing.y=theme::SPACING;
                contents(content)
            });
        self.body_bottom=Some(body.response.rect.bottom());
        body.inner
    }

    /// A framed decision owns one surface, instead of adding a framed
    /// master inside the already inset plain decision body.
    pub fn framed_body<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        let top=self.ui.cursor().top();
        let target=self.header_bottom.unwrap_or(top)+theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
        let region=self.ui.scope(|host|modal_master_body(host,contents));
        self.body_bottom=Some(region.response.rect.bottom());
        region.inner
    }

    fn before_actions(&mut self){
        let top=self.ui.cursor().top();
        let target=self.body_bottom.unwrap_or(top)+theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
    }

    pub fn actions<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
        self.before_actions();
        modal_action_row(self.ui,contents)
    }

    pub fn actions_split(&mut self,leading:impl FnOnce(&mut Ui),trailing:impl FnOnce(&mut Ui)){
        self.before_actions();
        modal_action_row_split(self.ui,leading,trailing);
    }
}

pub fn decision_workspace<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut DecisionWorkspace<'_>)->R
)->R{
    ui.scope(|layout|{
        modal_master_controlled_spacing(layout);
        contents(&mut DecisionWorkspace{
            ui:layout,header_bottom:None,body_bottom:None,
        })
    }).inner
}

/// Convenience entry point for simple decisions. Uses the same workspace
/// as interactive decision forms; never assembles a second layout.
pub fn decision_content<R>(
    ui:&mut Ui,title:&str,cancel_label:&str,enabled:bool,
    body:impl FnOnce(&mut Ui),actions:impl FnOnce(&mut Ui)->R
)->(bool,R){
    decision_workspace(ui,|decision|{
        let cancelled=decision.header(title,cancel_label,enabled);
        decision.body(body);
        (cancelled,decision.actions(actions).inner)
    })
}

fn decision_window(
    title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    nested_modal_window(title,id,work_area)
        // Decisions are short, blocking actions: show their controls
        // immediately rather than fading an input surface in over the parent.
        .fade_in(false)
        // Confirmation dialogs must size to their content. Enabling an outer
        // ScrollArea here makes egui greedily occupy the available height,
        // leaving a large blank panel below the final action button.
        // Large editors keep their own explicitly bounded body scroll areas.
        .resizable(false)
        .default_width(theme::MODAL_DECISION_MAX_WIDTH.min(work_area.width()))
}

/// Show a decision and establish the real window as the input boundary
/// only after egui has created its layer. This applies to nested and root
/// decisions alike, avoiding a full-screen dim swallowing button input.
pub fn show_decision_window<R>(
    ctx:&egui::Context,title:&str,id:&'static str,work_area:Rect,
    contents:impl FnOnce(&mut Ui)->R,
){
    if decision_window(title,id,work_area).show(ctx,contents).is_some(){
        modal_foreground_for_window(ctx,id);
    }
}

/// Draw the dim over the ENTIRE application viewport, including its header
/// and KakaoTalk reserved pane. Only the actual dialog remains centered in
/// the Kakao-safe work area; do not use that area to clip the backdrop.
/// Render before its dialog; nested confirmations draw one more dim layer.
pub fn modal_backdrop(
    ctx:&egui::Context,id:&'static str,nested:bool
) {
    let viewport=ctx.screen_rect();
    let alpha=if nested {
        theme::MODAL_NESTED_BACKDROP_ALPHA
    } else {
        theme::MODAL_BACKDROP_ALPHA
    };
    let area=egui::Area::new(egui::Id::new(id))
        .order(if nested {egui::Order::Foreground}else{egui::Order::Middle})
        .fixed_pos(viewport.min)
        .movable(false)
        // The backdrop cannot rise above the dialog after an outside click.
        .interactable(false)
        .sense(egui::Sense::hover());
    // The modal layer prevents background input; visual coverage is full-screen.
    let layer=area.layer();
    ctx.memory_mut(|memory|memory.set_modal_layer(layer));
    area.show(ctx,|ui|{
        let (rect,_)=ui.allocate_exact_size(
            viewport.size(),egui::Sense::hover()
        );
        ui.painter().rect_filled(
            rect,0.0,egui::Color32::from_black_alpha(alpha)
        );
    });
}

/// Layer occupied by the interactive nested decision window.
/// It intentionally shares Foreground order with its parent and backdrop;
/// explicit stacking below keeps the dialog above the full-screen dim Area.
pub fn modal_window_layer(id:&'static str)->egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground,egui::Id::new(id))
}

/// A child decision is rendered AFTER its disabled parent worksheet. Raise
/// only the real decision Window layer, and do so after Window::show created
/// it. An extra full-screen Foreground Area can intercept every button when
/// egui resolves previous-frame hit testing; the outer Middle backdrop and
/// disabled parent already block background input.
pub fn modal_foreground_for_window(
    ctx:&egui::Context,window_id:&'static str
){
    let dialog_layer=modal_window_layer(window_id);
    ctx.memory_mut(|memory|{
        memory.areas_mut().move_to_top(dialog_layer);
        memory.set_modal_layer(dialog_layer);
    });
}

/// Render the nested dim first, then restore the decision window to the top
/// of the Foreground stack. The full-screen backdrop itself can be marked
/// topmost by egui when the pointer is pressed anywhere inside its rectangle.
/// Without this explicit reordering, the backdrop can move above the dialog
/// and swallow every button. The modal input boundary is the dialog layer.
pub fn modal_backdrop_for_window(
    ctx:&egui::Context,backdrop_id:&'static str,window_id:&'static str
) {
    let viewport=ctx.screen_rect();
    let area=egui::Area::new(egui::Id::new(backdrop_id))
        .order(egui::Order::Foreground)
        .fixed_pos(viewport.min)
        .movable(false)
        .interactable(false)
        .sense(egui::Sense::hover());
    area.show(ctx,|ui|{
        let (rect,_)=ui.allocate_exact_size(
            viewport.size(),egui::Sense::hover()
        );
        ui.painter().rect_filled(
            rect,0.0,egui::Color32::from_black_alpha(
                theme::MODAL_NESTED_BACKDROP_ALPHA
            )
        );
    });
    let dialog_layer=modal_window_layer(window_id);
    ctx.memory_mut(|memory|{
        memory.areas_mut().move_to_top(dialog_layer);
        memory.set_modal_layer(dialog_layer);
    });
}

/// Reusable absolute-layout panel with a fixed title band and an
/// independently scrolling body. The caller supplies approved geometry and
/// stable IDs; clipping, frame paint, title placement and scroll ownership
/// remain identical across detail dashboards.
pub fn fixed_scroll_panel(
    ui:&mut Ui,
    rect:Rect,
    title:&str,
    heading_id:impl std::hash::Hash,
    scroll_id:impl std::hash::Hash,
    inset:Vec2,
    title_height:f32,
    title_to_body_gap:f32,
    body:impl FnOnce(&mut Ui),
){
    if rect.width()<2.0||rect.height()<2.0{return;}
    ui.painter().rect(
        rect,theme::CONTROL_RADIUS as f32,theme::INPUT,
        Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER),
        egui::StrokeKind::Inside
    );
    let inner=rect.shrink2(inset);
    let parent_clip=ui.clip_rect();
    let title_rect=Rect::from_min_size(
        inner.min,Vec2::new(
            inner.width(),(title_height-title_to_body_gap).max(0.0)
        )
    );
    let scroll_rect=Rect::from_min_max(
        egui::pos2(inner.left(),inner.top()+title_height),inner.max
    );
    ui.scope_builder(UiBuilder::new()
        .id_salt(heading_id)
        .max_rect(title_rect)
        .layout(Layout::top_down(Align::Min)),|heading|{
            heading.set_clip_rect(title_rect.intersect(parent_clip));
            heading.label(typography::section_heading(title));
        });
    ui.scope_builder(UiBuilder::new()
        .id_salt(scroll_id)
        .max_rect(scroll_rect)
        .layout(Layout::top_down(Align::Min)),|content|{
            content.set_clip_rect(scroll_rect.intersect(parent_clip));
            content.set_width(scroll_rect.width());
            content.spacing_mut().item_spacing.y=0.0;
            egui::ScrollArea::vertical()
                .id_salt("fixed-scroll-panel")
                .max_height(scroll_rect.height().max(1.0))
                .auto_shrink([false,false])
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                )
                .show(content,|scroll|{
                    scroll.spacing_mut().item_spacing.y=theme::SPACE_4;
                    body(scroll);
                });
        });
}

/// A 36pt inline form label. Unlike natural-height ui.label(), the text is
/// measured and painted inside the SAME row height as its sibling TextEdit.
/// Optical adjustment comes from the typography layer, not each screen.
pub fn form_field_label_sized(ui:&mut Ui,label:&str,width:f32)->Response {
    let galley=typography::field_label_galley(ui,label);
    let width=width.max(1.0).min(ui.available_width().max(1.0));
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover()
    );
    let pos=egui::pos2(rect.left(),
        rect.top()+typography::form_label_text_y(rect.height(),galley.size().y));
    ui.painter().with_clip_rect(rect).galley(pos,galley,theme::SECONDARY);
    response
}
pub fn form_field_label(ui:&mut Ui,label:&str)->Response {
    let width=typography::field_label_galley(ui,label).size().x;
    form_field_label_sized(ui,label,width)
}

/// Approved inline modal form: label and editor occupy ONE 36pt row.
/// The editor is responsible for its own minimum width and disabled state.
/// Reuse this for product, client and other management dialogs instead of
/// screen-specific vertical label/input stacks.
pub fn modal_labeled_row<R>(
    ui:&mut Ui,label:&str,control:impl FnOnce(&mut Ui)->R
)->R {
    ui.horizontal(|line|{
        line.spacing_mut().item_spacing.x=theme::SPACE_8;
        form_field_label_sized(
            line,label,theme::MODAL_FORM_LABEL_WIDTH
        );
        control(line)
    }).inner
}

/// Short body-text input (for rates and percentages): same caret, selection,
/// margins and row height as every form editor. Font remains proportional,
/// so the independent Consolas +2.5pt data-value tweak is not involved.
pub fn compact_text_field(
    ui:&mut Ui,value:&mut String,width:f32,enabled:bool
)->Response{
    let width=width.min(ui.available_width().max(1.0)).max(1.0);
    let edit=egui::TextEdit::singleline(value)
        .font(egui::TextStyle::Body)
        .vertical_align(Align::Center)
        .margin(typography::form_input_text_margin())
        .desired_width(width);
    ui.add_enabled_ui(enabled,|entry|
        entry.add_sized(Vec2::new(width,theme::INPUT_HEIGHT),edit)
    ).inner
}

/// Uses the same height, font and focus styling for every single-line field.
/// Disabled preview controls cannot edit business data.
pub fn field(ui: &mut Ui, value: &mut String, kind: FieldKind, enabled: bool) -> Response {
    field_with_margin(ui,value,kind,enabled,typography::form_input_text_margin())
}

/// Keep the shared font, focus, validation and fixed 36pt geometry while
/// allowing unit-bearing money fields to reserve part of their inner margin.
fn field_with_margin(
    ui:&mut Ui,value:&mut String,kind:FieldKind,enabled:bool,
    input_margin:egui::Margin
)->Response{
    let width = kind.preferred_width().min(ui.available_width().max(60.0));
    let semantic=match kind {
        FieldKind::Money=>Some(typography::DataValueKind::Money),
        FieldKind::Quantity=>Some(typography::DataValueKind::Quantity),
        FieldKind::Telephone=>Some(typography::DataValueKind::Phone),
        _=>None,
    };
    let editor_font=semantic.map_or(
        egui::FontSelection::Style(egui::TextStyle::Body),
        |role|egui::FontSelection::FontId(
            typography::data_value_font(role,typography::DataValueContext::Form)
        )
    );
    let field = egui::TextEdit::singleline(value)
        .font(editor_font)
        .horizontal_align(semantic.map_or(Align::LEFT,typography::data_value_align))
        .vertical_align(Align::Center)
        .margin(input_margin)
        .desired_width(width);
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(Vec2::new(width, theme::INPUT_HEIGHT), field)
    }).inner
}

/// UI-only money input: keep the 36pt shared money field and semantic font.
/// Thousands separators are visible during typing, not only after blur.
/// The native business command remains responsible for validating saved values.
pub fn parse_money_input(raw:&str)->Option<i64>{
    let digits=raw.trim().replace(',',"");
    if digits.is_empty() || !digits.bytes().all(|b|b.is_ascii_digit()){
        return None;
    }
    digits.parse::<i64>().ok()
        .filter(|value|*value<=9_007_199_254_740_991)
}

/// Format digits without changing their numeric meaning or stripping leading
/// zeros while the employee is still typing. Empty is an unset price.
fn formatted_money_input(raw:&str)->Option<String>{
    let digits=raw.trim().replace(',',"");
    if digits.is_empty(){return Some(String::new());}
    parse_money_input(&digits)?;
    let mut formatted=String::with_capacity(digits.len()+digits.len()/3);
    for (index,digit) in digits.bytes().enumerate(){
        if index>0 && (digits.len()-index)%3==0{
            formatted.push(',');
        }
        formatted.push(digit as char);
    }
    Some(formatted)
}

/// Remap a caret or selection endpoint using the number of digits before it.
/// This works when a separator appears *before* a selection, so inserting
/// digits in the middle does not jump the cursor to the end of the input.
fn money_cursor_after_format(raw:&str,formatted:&str,old_index:usize)->usize{
    let before=raw.chars().take(old_index).filter(char::is_ascii_digit).count();
    if before==0{return 0;}
    let mut seen=0;
    let mut target=formatted.chars().count();
    for (index,c) in formatted.chars().enumerate(){
        if c.is_ascii_digit(){
            seen+=1;
            if seen==before {
                target=index+1;
                break;
            }
        }
    }
    // When the caret was just after an existing separator, keep it there.
    if raw.chars().nth(old_index.saturating_sub(1))==Some(',')
        && formatted.chars().nth(target)==Some(','){
        target+=1;
    }
    target
}
pub fn money_field(ui:&mut Ui,value:&mut String,enabled:bool)->Response {
    money_field_with_margin(ui,value,enabled,typography::form_input_text_margin())
}

fn money_field_with_margin(
    ui:&mut Ui,value:&mut String,enabled:bool,input_margin:egui::Margin
)->Response{
    let before=value.clone();
    let response=field_with_margin(ui,value,FieldKind::Money,enabled,input_margin);
    normalize_money_edit(ui,value,&before,&response);
    response
}

/// One live formatting/caret path for forms and exact-rect worksheet cells.
fn normalize_money_edit(ui:&Ui,value:&mut String,before:&str,response:&Response){
    // Never persist display punctuation as an alternative business value.
    let formatted=formatted_money_input(value);
    let next=formatted.unwrap_or_else(||before.to_owned());
    if *value!=next{
        let raw=value.clone();
        if response.has_focus(){
            if let Some(mut state)=egui::TextEdit::load_state(ui.ctx(),response.id){
                if let Some(mut range)=state.cursor.char_range(){
                    range.primary=egui::text::CCursor::new(
                        money_cursor_after_format(
                            &raw,&next,range.primary.index
                        )
                    );
                    range.secondary=egui::text::CCursor::new(
                        money_cursor_after_format(
                            &raw,&next,range.secondary.index
                        )
                    );
                    state.cursor.set_char_range(Some(range));
                    state.store(ui.ctx(),response.id);
                }
            }
        }
        *value=next;
        ui.ctx().request_repaint();
    }
}

/// Canonical numeric draft text, shared by resting and focused cells.
/// No currency suffix enters the native business value.
pub fn money_edit_text(amount:i64)->String{
    formatted_money_input(&amount.to_string()).unwrap_or_else(||amount.to_string())
}

// Reception money uses a fixed gap, independent of the numeric glyphs.
const RECEPTION_MONEY_UNIT_GAP:f32=theme::SPACE_4;
const RECEPTION_MONEY_PENDING:&str="—";

fn money_unit_left(right:f32,right_inset:f32,unit_width:f32)->f32{
    right-right_inset-unit_width
}

fn money_unit_margin(
    ui:&Ui,unit:&str,font:egui::FontId,right_inset:f32,unit_gap:f32,
    mut margin:egui::Margin
)->(egui::Margin,std::sync::Arc<egui::Galley>){
    let galley=ui.painter().layout_no_wrap(unit.to_owned(),font,theme::TEXT);
    margin.right=(galley.size().x+right_inset+unit_gap).ceil().clamp(0.0,127.0) as i8;
    margin.top=2;
    margin.bottom=2;
    (margin,galley)
}

fn paint_money_unit(ui:&Ui,rect:Rect,galley:std::sync::Arc<egui::Galley>,right_inset:f32){
    let pos=egui::pos2(money_unit_left(rect.right(),right_inset,galley.size().x),
        rect.center().y-galley.size().y/2.0);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(pos,galley,theme::TEXT);
}

/// Shared money widget construction. Both allocated form fields and
/// exact-rectangle worksheet overlays use the same text and suffix geometry.
fn money_text_edit_with_unit<'a>(
    ui:&Ui,value:&'a mut String,font:egui::FontId,width:f32,
    unit:&str,right_inset:f32,unit_gap:f32,margin:egui::Margin
)->(egui::TextEdit<'a>,std::sync::Arc<egui::Galley>){
    let (margin,galley)=money_unit_margin(ui,unit,font.clone(),right_inset,unit_gap,margin);
    let edit=egui::TextEdit::singleline(value).font(font)
        .horizontal_align(Align::RIGHT).vertical_align(Align::Center)
        .margin(margin).desired_width(width);
    (edit,galley)
}

/// Price-table variant of the same shared money editor. The won unit is
/// a non-editable suffix inside the 112pt field, never part of the stored
/// number. Reserve its actual rendered width in the TextEdit's right margin:
/// the caret and selection cannot cover the suffix. Numeric glyphs retain
/// the same 14pt font and live comma formatting as money_field().
///
/// `unit_right_inset` is relative to the field's right edge (6pt in the
/// product grid, whose field ends 2pt before the 8pt read-only money anchor).
pub fn money_field_with_unit(
    ui:&mut Ui,value:&mut String,enabled:bool,
    unit:&str,unit_right_inset:f32
)->Response{
    let before=value.clone();
    let width=FieldKind::Money.preferred_width().min(ui.available_width().max(60.0));
    let (edit,unit_galley)=money_text_edit_with_unit(
        ui,value,typography::numeric_editor_font(),width,unit,unit_right_inset,
        0.0,typography::form_input_text_margin()
    );
    let response=ui.add_enabled_ui(enabled,|ui|{
        ui.add_sized(Vec2::new(width,theme::INPUT_HEIGHT),edit)
    }).inner;
    normalize_money_edit(ui,value,&before,&response);
    paint_money_unit(ui,response.rect,unit_galley,unit_right_inset);
    response
}

/// Shared single-line free-text editor for inline manual cells and search.
/// Uses the same font, internal margin and height as the standard field role.
pub fn flexible_text_field(
    ui:&mut Ui, value:&mut String, width:f32, placeholder:&str
)->Response {
    let width=width.min(ui.available_width().max(60.0));
    let edit=egui::TextEdit::singleline(value)
        .font(egui::TextStyle::Body)
        .vertical_align(Align::Center)
        .margin(typography::form_input_text_margin())
        .hint_text(placeholder)
        .desired_width(width);
    ui.add_sized(Vec2::new(width,theme::INPUT_HEIGHT),edit)
}

/// Shared date text field used inside the management date-group master.
/// The caller owns navigation, parsing and validation; this component owns
/// the centered, frameless editor, proportional date face and caret margin.
pub fn management_date_text_field(
    ui:&mut Ui,value:&mut String,width:f32,height:f32,hint:&str
)->Response{
    management_date_text_field_impl(ui,value,width,height,hint,None)
}

fn management_date_text_field_impl(
    ui:&mut Ui,value:&mut String,width:f32,height:f32,hint:&str,limit:Option<usize>
)->Response{
    let mut edit=egui::TextEdit::singleline(value)
        .font(typography::data_value_font(
            typography::DataValueKind::Date,
            typography::DataValueContext::Form
        ))
        .frame(false)
        .horizontal_align(Align::Center)
        .vertical_align(Align::Center)
        .margin(typography::management_date_text_margin())
        .hint_text(hint);
    if let Some(limit)=limit{edit=edit.char_limit(limit);}
    ui.add_sized(Vec2::new(width,height),edit)
}

/// Shared multiline editor for long-form messages and management memos.
/// Screens provide only the existing width, row count and optional hint; the
/// native TextEdit retains its standard click, selection and keyboard behavior.
pub fn multiline_text_field(
    ui:&mut Ui,value:&mut String,enabled:bool,
    desired_width:f32,desired_rows:usize,hint:Option<&str>
)->Response{
    let mut editor=egui::TextEdit::multiline(value)
        .desired_width(desired_width)
        .desired_rows(desired_rows);
    if let Some(hint)=hint{
        editor=editor.hint_text(hint);
    }
    ui.add_enabled(enabled,editor)
}

/// Shared exact-viewport multiline editor for content-specific text roles.
/// The caller may supply a dedicated font and inset without rebuilding the
/// native TextEdit or bypassing the common input layer.
pub fn multiline_text_field_sized(
    ui:&mut Ui,value:&mut String,enabled:bool,size:Vec2,
    font:egui::FontId,frame:bool,margin:egui::Margin,desired_rows:usize
)->Response{
    let editor=egui::TextEdit::multiline(value)
        .interactive(enabled)
        .font(font)
        .frame(frame)
        .margin(margin)
        .desired_width(size.x)
        .desired_rows(desired_rows);
    ui.add_sized(size,editor)
}

/// Source-document text styling is shared while the caller supplies its
/// custom layout job and immutable or editable buffer.
pub fn reception_source_text_edit<'a>(
    buffer:&'a mut dyn egui::TextBuffer,width:f32
)->egui::TextEdit<'a>{
    egui::TextEdit::multiline(buffer).frame(false)
        .margin(egui::Margin::same(theme::SOURCE_DOCUMENT_TEXT_MARGIN))
        .font(egui::FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT))
        .desired_width(width)
}

/// Search inputs are append-first: a normal pointer click to resume an
/// existing filter places the caret after its last Unicode character, even
/// when the click lands over the first character. This does NOT alter ordinary
/// TextEdit or inline editing: double-click selection, dragging, modifier
/// selection and keyboard caret navigation retain their native behavior.
fn management_search_append_click(response:&Response,ui:&Ui,text:&str){
    if !response.clicked_by(egui::PointerButton::Primary)
        || response.double_clicked()
        || response.triple_clicked()
        || response.dragged()
        || ui.input(|input| {
            let modifiers=input.modifiers;
            modifiers.shift || modifiers.ctrl || modifiers.alt || modifiers.mac_cmd
        })
    {
        return;
    }
    if let Some(mut state)=egui::TextEdit::load_state(ui.ctx(),response.id){
        let end=text.chars().count();
        state.cursor.set_char_range(Some(
            egui::text::CCursorRange::one(egui::text::CCursor::new(end))
        ));
        state.store(ui.ctx(),response.id);
        ui.ctx().request_repaint();
    }
}

/// Same shared 36pt search field geometry, but a deliberate end-of-query
/// caret policy instead of TextEdit's pointer-position-based initial caret.
pub fn management_search_field(
    ui:&mut Ui,text:&mut String,width:f32,placeholder:&str
)->Response {
    let response=flexible_text_field(ui,text,width,placeholder);
    management_search_append_click(&response,ui,text);
    response
}

/// Shared compact spreadsheet cell. Selection is an inset border;
/// hovered cells do not resize the row, and a selected cell does not
/// borrow the main navigation style.
/// Manual worksheet value cells use the same blue evidence fill, text ink
/// and 2pt outline as reception. Selection (amber) remains a separate state;
/// an active source link takes visual precedence over selection.
pub fn manual_linked_table_cell(
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
        theme::paint_button_hover(ui,&response,0.0);
    }
    response
}

/// Inline editor variant of the manual worksheet cell. The text editor stays
/// inside the selected cell, while the same linked-evidence fill and selection
/// border used by the read-only cell remain visible.
pub fn manual_linked_table_editor(
    ui:&mut Ui,value:&mut String,width:f32,linked:bool
)->Response{
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(width,theme::MANUAL_TABLE_ROW_HEIGHT),egui::Sense::hover()
    );
    let fill=if linked{source_highlight::ACTIVE_FILL}else{theme::RAISED};
    let stroke=if linked{
        Stroke::new(theme::SOFT_DIVIDER_WIDTH/2.0,theme::DIVIDER)
    }else{
        Stroke::new(theme::DOCK_SPLITTER_HOVER_LINE,theme::SELECTED_ORDER)
    };
    ui.painter().rect(
        rect,0.0,fill,stroke,egui::StrokeKind::Inside
    );
    let inner=rect.shrink2(Vec2::new(5.0,2.0));
    let mut editor_ui=ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(Align::Center))
    );
    editor_ui.set_clip_rect(inner);
    editor_ui.add_sized(
        inner.size(),
        egui::TextEdit::singleline(value)
            .font(egui::FontId::proportional(
                typography::FONT_RECEPTION_RECIPIENT_TEXT
            ))
            .text_color(if linked{source_highlight::ACTIVE_TEXT}else{theme::TEXT})
            .vertical_align(Align::Center)
            .margin(egui::Margin::symmetric(2,0))
            .frame(false)
            .desired_width(inner.width())
            .hint_text("미입력")
    )
}

/// Unlinked shipment/header cells retain their existing selected styling.
pub fn manual_table_cell(ui:&mut Ui,value:&str,width:f32,selected:bool)->Response {
    manual_linked_table_cell(ui,value,width,selected,false)
}
pub fn manual_table_heading(ui:&mut Ui,label:&str,width:f32){
    ui.add_sized(
        Vec2::new(width,theme::MANUAL_TABLE_HEADER_HEIGHT),
        egui::Label::new(typography::field_label(label)),
    );
}

/// A field group's label and editor share the same baseline and label width.
pub fn labeled_field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    kind: FieldKind,
    enabled: bool,
) -> Response {
    ui.horizontal(|ui| {
        let title=form_field_label_sized(ui,label,LABEL_WIDTH);
        // The whole field row (including its label) is a single hover
        // target for exact source linking. No geometry is modified.
        let input=field(ui, value, kind, enabled);
        title.union(input)
    }).inner
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
                segments,arrow_width,"이전 수취인",previous_enabled,
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
                egui::Label::new(typography::body(display))
                    .truncate().halign(Align::Center)
                    .sense(if clipped{Sense::click()}else{Sense::hover()})
            ).on_hover_text(display);
            if clipped{
                egui::Popup::menu(&label)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|popup|{
                        popup.set_max_width(320.0);
                        popup.add(egui::Label::new(typography::body(display)).wrap());
                    });
            }

            let next=date_navigation_segment_button(
                segments,arrow_width,"다음 수취인",next_enabled,
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
            for x in [
                inner.left()+arrow_width,
                inner.left()+arrow_width+name_width,
                inner.left()+2.0*arrow_width+name_width
            ]{
                segments.painter().line_segment(
                    [
                        egui::pos2(x,inner.top()+4.0),
                        egui::pos2(x,inner.bottom()-4.0)
                    ],
                    Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
                );
            }
            selected
        }
    );
    selected
}


/// The approved Reception card chrome is shared with the manual worksheet.
/// The same 16pt card heading, 16pt inset, divider and header-action alignment
/// now render in both screens; business actions stay in their callers.
pub fn reception_section_with_header(
    ui:&mut Ui,title:&str,minimum_height:f32,
    header_right:impl FnOnce(&mut Ui),content:impl FnOnce(&mut Ui),
){
    reception_section_with_header_tooltip(
        ui,title,None,minimum_height,header_right,content
    );
}

/// Identical Reception section chrome with an optional title explanation.
/// The tooltip variant lets specialised panes retain their existing guidance
/// without rebuilding the approved card, divider or content insets.
pub fn reception_section_with_header_tooltip(
    ui:&mut Ui,title:&str,title_tooltip:Option<&str>,minimum_height:f32,
    header_right:impl FnOnce(&mut Ui),content:impl FnOnce(&mut Ui),
){
    let width=ui.available_width();
    let _card=theme::docked_pane_frame().show(ui,|ui|{
        let width_inset=2.0*theme::SECTION_PADDING;
        let height_inset=2.0*theme::RECEPTION_PANE_PADDING_Y;
        ui.set_min_width((width-width_inset).max(60.0));
        ui.set_min_height((minimum_height-height_inset).max(48.0));
        let body_item_spacing=theme::SPACING;
        ui.spacing_mut().item_spacing.y=0.0;
        let _header=ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(),theme::SECTION_HEADER_HEIGHT),
            Layout::left_to_right(Align::Center),|header|{
                header.set_min_height(theme::SECTION_HEADER_HEIGHT);
                let heading=header.label(typography::section_heading(title));
                if let Some(tooltip)=title_tooltip{
                    heading.on_hover_text(tooltip);
                }
                header.with_layout(Layout::right_to_left(Align::Center),header_right);
            }
        );
        #[cfg(test)]
        general_screen_geometry_tests::record(reception_geometry_role(title,true),_header.response.rect);
        ui.add_space(theme::SECTION_CONTENT_GAP);
        let divider=reception_section_divider(ui);
        #[cfg(test)]
        general_screen_geometry_tests::record("source divider",divider);
        ui.add_space(theme::SECTION_CONTENT_GAP);
        let body=ui.scope(|body|{
            body.spacing_mut().item_spacing.y=body_item_spacing;
            content(body);
        });
        #[cfg(test)]
        general_screen_geometry_tests::record("source body",body.response.rect);
    });
    #[cfg(test)]
    general_screen_geometry_tests::record(reception_geometry_role(title,false),_card.response.rect);
}

#[cfg(test)]
fn reception_geometry_role(title:&str,header:bool)->&'static str{
    match (title,header){
        ("발주 원문",false)=>"source card",("발주 원문",true)=>"source header",
        ("복사용 자동 메시지",false)=>"message card",("복사용 자동 메시지",true)=>"message header",
        ("확인할 내용",false)=>"review card",("확인할 내용",true)=>"review header",
        ("수취인별 주문 정보",false)=>"recipient card",("수취인별 주문 정보",true)=>"recipient header",
        ("품목 및 운임",false)=>"items card",("품목 및 운임",true)=>"items header",
        ("발주서 합계",false)=>"total card",("발주서 합계",true)=>"total header",
        ("처리 중인 주문",false)=>"orders card",("처리 중인 주문",true)=>"orders header",
        (_,false)=>"other reception card",(_,true)=>"other reception header",
    }
}

/// The READ and EDIT renderers of recipient fields must use exactly one
/// typography/padding definition. Contact numbers use the same 16pt regular
/// proportional UI family as name, address, message and source text.
fn reception_recipient_text_edit(
    value:&mut String,width:f32,active:bool,phone_number:bool
)->egui::TextEdit<'_>{
    let font=if phone_number {
        typography::data_value_font(
            typography::DataValueKind::Phone,
            typography::DataValueContext::Reception
        )
    }else{
        egui::FontId::proportional(typography::FONT_RECEPTION_RECIPIENT_TEXT)
    };
    let input=egui::TextEdit::singleline(value)
        .font(font)
        .horizontal_align(Align::LEFT)
        .vertical_align(Align::Center)
        .margin(egui::Margin::symmetric(8,2))
        .desired_width(width);
    if active {
        // TextEdit's native background uses a different square/filled
        // geometry. Let the shared evidence surface own the rounded fill
        // and the outer caller own the common 2pt border instead.
        input.frame(false).text_color(source_highlight::ACTIVE_TEXT)
    }else{input}
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

/// Read-only TextEdit does not reliably own the egui hover response.
/// One shared hit target now owns field hover, I-beam cursor and inline-click
/// handoff for both recipient details and read-only workflow views.
fn reception_recipient_value_hit(ui:&mut Ui,display:&Response)->Response{
    let hit=ui.interact(
        display.rect,
        display.id.with("recipient-value-hit"),
        Sense::click()
    );
    if hit.hovered(){
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    hit
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

    fn row(ui: &mut Ui, label: &str, value: &mut String, active: bool) -> Response {
        ui.horizontal(|line| {
            line.spacing_mut().item_spacing.x = theme::RECIPIENT_LABEL_GAP;
            label_cell(line, label, theme::RECIPIENT_LABEL_WIDTH);
            let width = line.available_width().max(1.0);
            reception_recipient_linked_background(line,width,active);
            // The response belongs to the input, NOT the combined row.
            // This is read-only while retaining hover and native text layout.
            let display=line.add_sized(
                Vec2::new(width, theme::INPUT_HEIGHT),
                reception_recipient_text_edit(value, width, active, false).interactive(false),
            );
            let hit=reception_recipient_value_hit(line,&display);
            source_highlight::paint_active_if(line,&hit,active);
            hit
        }).inner
    }

    let responses=ui.scope(|fields| {
        fields.spacing_mut().item_spacing.y = theme::RECIPIENT_ROW_GAP;
        let name_response = row(fields, "이름", name, active[0]);
        let address_response = row(fields, "주소", address, active[1]);

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
        let (mobile_response, phone_response) = egui::ScrollArea::horizontal()
            .id_salt("recipient-contact-row")
            .drag_to_scroll(false)
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
                    let mobile_display = line.add_sized(
                        Vec2::new(mobile_width, theme::INPUT_HEIGHT),
                        reception_recipient_text_edit(mobile, mobile_width, active[2], true).interactive(false),
                    );
                    let mobile_input=reception_recipient_value_hit(line,&mobile_display);
                    source_highlight::paint_active_if(line,&mobile_input,active[2]);
                    // Only the gap between the TWO contact groups is 8pt.
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_PHONE_COLUMN_GAP;
                    label_cell(line, "전화", theme::RECIPIENT_PHONE_LABEL_WIDTH);
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_LABEL_GAP;
                    reception_recipient_linked_background(
                        line,phone_width,active[3]
                    );
                    let phone_display = line.add_sized(
                        Vec2::new(phone_width, theme::INPUT_HEIGHT),
                        reception_recipient_text_edit(phone, phone_width, active[3], true).interactive(false),
                    );
                    let phone_input=reception_recipient_value_hit(line,&phone_display);
                    source_highlight::paint_active_if(line,&phone_input,active[3]);
                    (mobile_input,phone_input)
                }).inner
            }).inner;
        let note_response = row(fields, "메시지", note, active[4]);
        [
            name_response, address_response,
            mobile_response, phone_response, note_response,
        ]
    }).inner;
    #[cfg(test)]
    for (name,response) in ["recipient name","recipient address","recipient mobile","recipient phone","recipient note"].into_iter().zip(responses.iter()){
        general_screen_geometry_tests::record(name,response.rect);
    }
    responses
}

/// Place an inline editor in the same exact frame as Ui::place, while
/// retaining its native galley so the initial pointer position becomes a
/// real character cursor instead of forcing the end of the text.
fn reception_text_editor_at_click(
    ui:&mut Ui,cell:Rect,edit:egui::TextEdit<'_>,
    request_focus:bool,click_at:Option<egui::Pos2>
)->Response{
    let mut placed=ui.new_child(
        egui::UiBuilder::new()
            .max_rect(cell)
            .layout(egui::Layout::centered_and_justified(
                egui::Direction::TopDown
            ))
    );
    let mut output=edit.show(&mut placed);
    if request_focus{
        output.response.request_focus();
        // The product/amount editor can be inset a few points from the
        // clickable read-only cell. Clamp clicks in that border to the
        // nearest valid text position rather than falling back to the end.
        if let Some(pointer)=click_at.filter(|point|
            cell.expand2(egui::vec2(theme::SPACE_4,theme::SPACE_2))
                .contains(*point)
        ){
            let pointer=egui::pos2(
                pointer.x.clamp(cell.left(),cell.right()),
                pointer.y.clamp(cell.top(),cell.bottom())
            );
            // Galley coordinates honor Hangul widths, left/right align,
            // font choice, margins and horizontal text scrolling.
            let cursor=output.galley.cursor_from_pos(
                pointer-output.galley_pos
            );
            output.state.cursor.set_char_range(Some(
                egui::text::CCursorRange::one(cursor)
            ));
            output.state.store(ui.ctx(),output.response.id);
            ui.ctx().request_repaint();
        }
    }
    output.response
}

/// Edit over the original 36pt input rectangle without shrinking it.
/// Ui::place never advances the parent layout cursor (Ui::put does).
/// The readonly control and focused control share the same builder, so the text
/// baseline, inset and phone font do not change when focus is acquired.
pub fn reception_recipient_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>,phone_number:bool,active:bool
)->Response{
    reception_text_editor_at_click(
        ui,cell,
        reception_recipient_text_edit(value,cell.width(),active,phone_number),
        request_focus,click_at
    )
}

/// The source editor and automatic-message canvas use the SAME surface
/// tokens. Painting a reserved rect avoids Frame::show shrinking an empty
/// message body and moving the fixed copy-action row.
pub fn paint_source_document_surface(ui: &Ui, rect: Rect) {
    ui.painter().rect(
        rect,
        theme::CONTROL_RADIUS as f32,
        theme::INPUT,
        Stroke::new(theme::FRAME_STROKE_WIDTH, theme::BORDER),
        egui::StrokeKind::Inside,
    );
}

/// The 70pt client identification band is INFORMATION, not an editable
/// source document. Keep its exact reserved Rect and use the management
/// master's muted inner-row colors to establish a quieter visual tier.
pub fn paint_reception_client_summary_surface(ui:&Ui,rect:Rect){
    ui.painter().rect(
        rect,theme::CONTROL_RADIUS as f32,
        theme::RECEPTION_CLIENT_SUMMARY_BG,
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,
            theme::RECEPTION_CLIENT_SUMMARY_BORDER),
        egui::StrokeKind::Inside,
    );
}

/// Reception section titles are NOT fixed table headers. Keep a soft
/// 1pt DIVIDER under them; the strong 2pt TABLE_HEADER_OUTLINE belongs
/// exclusively to the management/list column headers.
/// Preserve the existing one-point slot and all dock splitter coordinates.
pub fn reception_section_divider(ui:&mut Ui)->Rect{
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::SOFT_DIVIDER_WIDTH),Sense::hover()
    );
    ui.painter().rect_filled(rect,0.0,theme::DIVIDER);
    rect
}

/// Untyped recognition/workflow notices retain informational coloring.
pub fn reception_review_status(ui:&mut Ui,message:&str){
    let tone=theme::ReviewTone::Info;
    theme::review_issue_card(ui,tone,|content|{
        content.add(egui::Label::new(typography::body(message)
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
    if ui.available_width()<280.0 {
        ui.with_layout(Layout::top_down(Align::Max),|actions|{
            actions.spacing_mut().item_spacing.y=theme::SPACE_8;
            if theme::action(actions,"그대로 복사",theme::ButtonRole::Standard,enabled)
                .clicked(){greeting=Some(false);}
            if theme::action(actions,"인사말 포함 복사",
                theme::ButtonRole::Standard,enabled).clicked(){greeting=Some(true);}
        });
    }else{
        ui.with_layout(Layout::right_to_left(Align::Center),|actions|{
            actions.spacing_mut().item_spacing.x=theme::SPACE_8;
            if theme::action(actions,"인사말 포함 복사",
                theme::ButtonRole::Standard,enabled).clicked(){greeting=Some(true);}
            if theme::action(actions,"그대로 복사",theme::ButtonRole::Standard,enabled)
                .clicked(){greeting=Some(false);}
        });
    }
    greeting
}

/// A subtle separation between data belonging to the same record.
/// Sections themselves are framed by the caller.
pub fn soft_divider(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, theme::SOFT_DIVIDER_WIDTH), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER),
    );
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
/// Common management date group used by Order History and Excel History.
/// The shared component owns the outer 36pt frame, 34pt content clip,
/// asymmetric end caps, divider-safe hover geometry and arrow rendering.
/// Consumers own only the date value, callbacks and calendar popup.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ManagementDateSegmentEdge{Leading,Middle,Trailing}

pub fn management_date_segment_corners(
    edge:ManagementDateSegmentEdge
)->egui::CornerRadius{
    let r=theme::CONTROL_RADIUS.saturating_sub(
        theme::FRAME_STROKE_WIDTH as u8
    );
    match edge {
        ManagementDateSegmentEdge::Leading=>egui::CornerRadius{
            nw:r,ne:0,sw:r,se:0
        },
        ManagementDateSegmentEdge::Middle=>egui::CornerRadius::same(0),
        ManagementDateSegmentEdge::Trailing=>egui::CornerRadius{
            nw:0,ne:r,sw:0,se:r
        },
    }
}

/// Keep the date segment's hit target, fill, asymmetric rounding and hover
/// tied to one component. The caller must not paint a second hover layer.
pub fn management_date_segment(
    ui:&mut Ui,label:&str,width:f32,edge:ManagementDateSegmentEdge
)->Response{
    let corners=management_date_segment_corners(edge);
    let direction=match label {
        "‹"=>Some(ManagementChevronDirection::Left),
        "›"=>Some(ManagementChevronDirection::Right),
        _=>None
    };
    let display=if direction.is_some(){""}else{label};
    let response=ui.add_sized(
        Vec2::new(width,theme::ACTION_STANDARD_HEIGHT),
        egui::Button::new(typography::action_label(
            display,typography::FONT_STANDARD_ACTION,false,theme::TEXT
        ))
            .fill(theme::INPUT)
            .stroke(Stroke::NONE)
            .corner_radius(corners)
    );
    theme::paint_button_hover_corners(ui,&response,corners);
    if let Some(direction)=direction{
        let painter=ui.painter().with_clip_rect(response.rect);
        paint_management_chevron(
            &painter,response.rect.center(),direction
        );
    }
    response
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

/// The single date-navigation surface used by calendar arrows and compact
/// count steppers. Geometry, border, fill, radius and hover treatment must not
/// drift between controls that share the date-picker visual language.
#[derive(Clone,Copy)]
enum DateNavigationSegmentGlyph<'a>{
    Text(&'a str),
    Chevron(ManagementChevronDirection),
    CompactVerticalChevron { up: bool },
}

/// A single outer frame with zero-gap internal segments. Both the recipient
/// navigator and shipment-count editor use this exact date-selector surface.
fn date_navigation_segmented_surface<R>(
    ui:&mut Ui,id_salt:impl std::hash::Hash,size:Vec2,
    add_segments:impl FnOnce(&mut Ui,Rect)->R
)->(Rect,R){
    let (outer,_)=ui.allocate_exact_size(size,Sense::hover());
    let inner=outer.shrink(theme::FRAME_STROKE_WIDTH);
    ui.painter().rect_filled(
        outer,theme::CONTROL_RADIUS as f32,theme::INPUT
    );
    // The parent has already allocated the complete frame. A scoped UI
    // would advance its cursor again using the children's expanded bounds.
    let mut segments=ui.new_child(
        UiBuilder::new().id_salt(id_salt).max_rect(inner)
            .layout(Layout::left_to_right(Align::Center))
    );
    segments.set_clip_rect(inner.intersect(ui.clip_rect()));
    segments.set_width(inner.width());
    segments.set_height(inner.height());
    segments.spacing_mut().item_spacing=Vec2::ZERO;
    let content=add_segments(&mut segments,inner);
    ui.painter().rect_stroke(
        outer.shrink(0.5),theme::CONTROL_RADIUS as f32,
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER),
        egui::StrokeKind::Inside
    );
    (outer,content)
}

/// Count symbols are geometric strokes, not unrelated font glyphs.
/// Both horizontal bars have the same length and the same center line.
fn paint_count_step_symbol(painter:&egui::Painter,center:egui::Pos2,increase:bool,ink:Color32){
    let half=4.0;
    let stroke=Stroke::new(1.8,ink);
    painter.line_segment(
        [center-egui::vec2(half,0.0),center+egui::vec2(half,0.0)],stroke
    );
    if increase{
        painter.line_segment(
            [center-egui::vec2(0.0,half),center+egui::vec2(0.0,half)],stroke
        );
    }
}

/// Count buttons use the existing action/field contrast, not a local color.
fn count_step_palette(enabled:bool)->(Color32,Color32){
    if enabled{(theme::RAISED,theme::TEXT)}
    else{(theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_TEXT)}
}

/// A borderless segment button; its parent owns the only outer border.
/// Enabled/disabled colors and hover feedback follow the date-selector roles.
fn date_navigation_segment_button(
    ui:&mut Ui,width:f32,tooltip:&str,enabled:bool,
    glyph:DateNavigationSegmentGlyph<'_>
)->Response{
    let enabled=enabled && ui.is_enabled();
    // Half-height stepper segments must not inherit the full-sized Button's
    // minimum height or widget expansion. Use their allocated rectangle.
    if let DateNavigationSegmentGlyph::CompactVerticalChevron{up}=glyph {
        let rect=ui.max_rect().intersect(ui.clip_rect());
        let response=ui.interact(rect,ui.id().with("compact-chevron"),
            if enabled{Sense::click()}else{Sense::hover()});
        let corners=egui::CornerRadius{
            nw:if up{3}else{0},ne:0,
            sw:if up{0}else{3},se:0,
        };
        let painter=ui.painter().with_clip_rect(rect);
        if !enabled{
            painter.rect_filled(rect,corners,theme::RECEPTION_RECIPIENT_NAV_DISABLED_BG);
        }else{
            theme::paint_button_hover_corners(ui,&response,corners);
        }
        let ink=if !enabled{theme::RECEPTION_RECIPIENT_NAV_DISABLED_ICON}
            else if response.hovered(){theme::TEXT}else{theme::SECONDARY};
        let angle=if up{-std::f32::consts::FRAC_PI_2}else{std::f32::consts::FRAC_PI_2};
        paint_chevron_at_angle_scaled(&painter,rect.center(),angle,0.0,ink,0.72);
        return response.on_hover_text(tooltip);
    }
    if let DateNavigationSegmentGlyph::Text(text)=glyph{
        // Compact +/- buttons own an exact segment, not the minimum size of
        // a standard egui Button. This avoids overflow at 28pt outer height.
        let rect=ui.max_rect().intersect(ui.clip_rect());
        let response=ui.interact(rect,ui.id().with("count-step"),
            if enabled{Sense::click()}else{Sense::hover()});
        let radius=theme::CONTROL_RADIUS.saturating_sub(2);
        let left=text=="−";
        let corners=egui::CornerRadius{
            nw:if left{radius}else{0},sw:if left{radius}else{0},
            ne:if left{0}else{radius},se:if left{0}else{radius},
        };
        let painter=ui.painter().with_clip_rect(rect);
        // Keep the editor on INPUT and give its action segments the shared
        // RAISED button face. The existing disabled and blue hover roles
        // remain distinct; neither changes the segment's allocated geometry.
        let (fill,ink)=count_step_palette(enabled);
        painter.rect_filled(rect,corners,fill);
        if enabled{
            theme::paint_button_hover_corners(ui,&response,corners);
        }
        paint_count_step_symbol(&painter,rect.center(),!left,ink);
        return response.on_hover_text(tooltip);
    }
    let height=ui.available_height();
    let (disabled_fill,disabled_ink)=match glyph{
        DateNavigationSegmentGlyph::Chevron(_)|DateNavigationSegmentGlyph::CompactVerticalChevron{..}=>(
            theme::RECEPTION_RECIPIENT_NAV_DISABLED_BG,
            theme::RECEPTION_RECIPIENT_NAV_DISABLED_ICON
        ),
        DateNavigationSegmentGlyph::Text(_)=>(
            theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_TEXT
        )
    };
    let fill=if enabled{theme::INPUT}else{disabled_fill};
    let response=ui.add_enabled_ui(enabled,|button_ui|{
        button_ui.add_sized(
            Vec2::new(width,height),
            egui::Button::new("")
                .fill(fill)
                .stroke(Stroke::NONE)
                .corner_radius(0)
        )
    }).inner;
    let ink=if enabled && response.hovered(){
        theme::TEXT
    }else if enabled{
        theme::SECONDARY
    }else{
        disabled_ink
    };
    // Paint the translucent hover surface before the glyph so the icon/text
    // remains crisp and uses the same z-order as the date-picker arrows.
    if enabled{
        theme::paint_button_hover(ui,&response,0.0);
    }
    let painter=ui.painter().with_clip_rect(response.rect);
    match glyph{
        DateNavigationSegmentGlyph::Text(text)=>{
            painter.text(
                response.rect.center(),egui::Align2::CENTER_CENTER,text,
                egui::FontId::proportional(typography::FONT_STANDARD_ACTION+2.0),
                ink
            );
        }
        DateNavigationSegmentGlyph::Chevron(direction)=>{
            paint_management_chevron_with_color(
                &painter,response.rect.center(),direction,ink
            );
        }
        DateNavigationSegmentGlyph::CompactVerticalChevron{..}=>unreachable!("handled above"),
    }
    response.on_hover_text(tooltip)
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
    let (rect,(decrement,input,increment))=
        date_navigation_segmented_surface(
            ui,id_salt,Vec2::new(width,height),|segments,inner|{
                let x1=inner.left()+arrow_width;
                let x2=inner.right()-arrow_width;
                let left=Rect::from_min_max(inner.min,egui::pos2(x1,inner.bottom()));
                let center=Rect::from_min_max(egui::pos2(x1,inner.top()),egui::pos2(x2,inner.bottom()));
                let right=Rect::from_min_max(egui::pos2(x2,inner.top()),inner.max);
                // Every segment gets its own fixed rectangle and ID. TextEdit
                // margins or a disabled button cannot shift the next segment.
                let mut minus=segments.new_child(UiBuilder::new().id_salt("count-minus")
                    .max_rect(left).layout(Layout::top_down(Align::Center)));
                minus.set_clip_rect(left.intersect(segments.clip_rect()));
                let decrement=date_navigation_segment_button(
                    &mut minus,arrow_width,"배송 건수 1건 줄이기",
                    decrement_enabled,DateNavigationSegmentGlyph::Text("−")
                );
                let mut entry=segments.new_child(UiBuilder::new().id_salt("count-value")
                    .max_rect(center).layout(Layout::centered_and_justified(egui::Direction::TopDown)));
                entry.set_clip_rect(center.intersect(segments.clip_rect()));
                entry.spacing_mut().interact_size=Vec2::ZERO;
                let hint=format!("{minimum}~{maximum}");
                let input=entry.add_enabled_ui(enabled,|entry|{
                    management_date_text_field_impl(
                        entry,value,center.width(),center.height(),hint.as_str(),Some(3)
                    )
                }).inner;
                let mut plus=segments.new_child(UiBuilder::new().id_salt("count-plus")
                    .max_rect(right).layout(Layout::top_down(Align::Center)));
                plus.set_clip_rect(right.intersect(segments.clip_rect()));
                let increment=date_navigation_segment_button(
                    &mut plus,arrow_width,"배송 건수 1건 늘리기",
                    increment_enabled,DateNavigationSegmentGlyph::Text("+")
                );
                for x in [
                    inner.left()+arrow_width,
                    inner.left()+arrow_width+input_width
                ]{
                    segments.painter().line_segment(
                        [
                            egui::pos2(x,inner.top()+4.0),
                            egui::pos2(x,inner.bottom()-4.0)
                        ],
                        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER)
                    );
                }
                (decrement,input,increment)
            }
        );
    DateNavigationCountControlResponse{
        decrement_clicked:decrement.clicked(),input,increment_clicked:increment.clicked(),rect,
        #[cfg(test)]
        decrement_rect:decrement.rect,
        #[cfg(test)]
        increment_rect:increment.rect,
    }
}

fn date_navigation_button_surface(
    ui:&mut Ui,tooltip:&str,enabled:bool
)->(Response,Color32){
    let effective_enabled=enabled && ui.is_enabled();
    let (fill,border,ink)=if effective_enabled{
        (theme::INPUT,theme::BORDER,theme::SECONDARY)
    }else{
        (theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_BORDER,
         theme::DISABLED_ACTION_TEXT)
    };
    let response=ui.add_enabled_ui(effective_enabled,|button_ui|{
        button_ui.add_sized(
            Vec2::splat(theme::CALENDAR_NAV_SIZE),
            egui::Button::new("")
                .fill(fill)
                .stroke(Stroke::new(theme::SOFT_DIVIDER_WIDTH,border))
                .corner_radius(theme::CONTROL_RADIUS)
        )
    }).inner.on_hover_text(tooltip);
    let ink=if effective_enabled && response.hovered(){
        theme::TEXT
    }else{ink};
    if effective_enabled{
        theme::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    (response,ink)
}

/// Shared calendar navigation: identical 28pt hit targets, border, radius,
/// and vector chevrons for year/month movement in either direction.
fn calendar_nav_button(
    ui:&mut Ui,direction:ManagementChevronDirection,
    year:bool,tooltip:&str
)->bool{
    let (response,ink)=date_navigation_button_surface(ui,tooltip,true);
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

/// Compact minus/plus control that deliberately reuses the date-picker's
/// 28pt button surface. Only the centered glyph and tooltip are caller-owned.
pub fn date_navigation_step_button(
    ui:&mut Ui,glyph:&str,tooltip:&str,enabled:bool
)->Response{
    let (response,ink)=date_navigation_button_surface(ui,tooltip,enabled);
    ui.painter().with_clip_rect(response.rect).text(
        response.rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION+2.0),
        ink
    );
    response
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
    egui::Popup::menu(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|calendar|{
            calendar.spacing_mut().item_spacing=egui::vec2(
                theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
            );
            calendar.set_min_width(theme::CALENDAR_GRID_WIDTH);
            calendar.horizontal(|nav|{
                nav.spacing_mut().item_spacing.x=theme::CALENDAR_GRID_GAP;
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,true,"이전 연도"
                ){
                    *month=shift_calendar_month(*month,-12);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,false,"이전 달"
                ){
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
                    nav,ManagementChevronDirection::Right,false,"다음 달"
                ){
                    *month=shift_calendar_month(*month,1);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,true,"다음 연도"
                ){
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
                            let mut button=egui::Button::new(typography::body(
                                &date.day().to_string()
                            )).corner_radius(theme::CONTROL_RADIUS);
                            if selected_date==Some(*date){
                                button=button.fill(theme::WORKFLOW_SELECTED_BG)
                                    .stroke(Stroke::new(1.0,theme::SELECTION));
                            }else if *date==today{
                                button=button.stroke(Stroke::new(1.0,theme::BORDER));
                            }
                            let response=grid.add_sized(
                                egui::vec2(width,width),button
                            );
                            theme::paint_button_hover(
                                grid,&response,theme::CONTROL_RADIUS as f32
                            );
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

/// Shared numeric KPI card used by the two approved management masters
/// and any other page copying their heading / tabs / KPI hierarchy.
/// Keep the 228pt card, 28pt value, 13pt caption and insets in one place.
pub fn management_summary_card(ui:&mut Ui,label:&str,count:usize){
    management_summary_card_value(ui,label,&count.to_string());
}

/// The same KPI geometry with an explicit display value, including "—"
/// while data is pending. Never report a fabricated zero before a fetch.
pub fn management_summary_card_value(ui:&mut Ui,label:&str,value:&str){
    let card_width=theme::FREIGHT_SUMMARY_CARD_WIDTH;
    ui.allocate_ui_with_layout(
        egui::vec2(card_width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |slot|{
            slot.set_width(card_width);
            theme::freight_summary_frame().show(slot,|cell|{
                cell.spacing_mut().item_spacing.y=0.0;
                let content_width=card_width-2.0*theme::FREIGHT_SUMMARY_PADDING_X
                    -2.0*theme::FRAME_STROKE_WIDTH;
                let (number_row,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                cell.add_space(theme::FREIGHT_SUMMARY_TEXT_GAP);
                let (label_row,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_LABEL_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                let number=typography::freight_summary_large_number_galley(
                    cell,value
                );
                let caption=typography::freight_summary_small_label_galley(cell,label);
                cell.painter().galley(
                    egui::pos2(
                        number_row.center().x-number.size().x/2.0,
                        number_row.center().y-number.size().y/2.0
                    ),number,theme::TEXT
                );
                cell.painter().galley(
                    egui::pos2(
                        label_row.center().x-caption.size().x/2.0,
                        label_row.center().y-caption.size().y/2.0
                    ),caption,theme::SECONDARY
                );
            });
        }
    );
}

/// Capture the actual frame allocation without changing the layout.
pub fn measured_management_master_card<R>(ui:&mut Ui,body:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
    let output=theme::management_master_table_frame().show(ui,body);
    #[cfg(test)]
    general_screen_geometry_tests::record("master card",output.response.rect);
    output
}
pub fn measured_management_section_card<R>(ui:&mut Ui,body:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
    let output=theme::section_frame().show(ui,body);
    #[cfg(test)]
    general_screen_geometry_tests::record("section card",output.response.rect);
    output
}

/// Approved 59th-round Freight page heading: 16pt from the top of the
/// workspace. The workspace shell already supplies a 6pt outer gutter, so
/// only the difference is added. Reception must not call this helper.
pub fn management_page_title_top_gap(ui:&mut Ui) {
    ui.add_space((theme::SPACE_16-theme::PANEL_GAP).max(0.0));
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
    general_screen_geometry_tests::record("top content",output.response.rect);
    output.inner
}

/// Align the OUTER top border of a management page's principal card with
/// Freight > Shipping Group Assignment. Pages without four KPI cards leave
/// an intentional blank area. This is not table-cell padding or row geometry.
pub fn align_management_primary_card(ui: &mut Ui, page_origin_y: f32) {
    let target_top = page_origin_y
        + theme::management_primary_card_top_offset(ui.available_width());
    let remaining = target_top - ui.cursor().top();
    if remaining > 0.5 {
        ui.add_space(remaining);
    }
}

/// Expand one primary list card to the bottom of the management workspace
/// when its rows do not already require more room. Nested/multiple-card
/// pages must not use this: their own content needs ordinary scrolling.
pub fn management_primary_card_min_inner_height(ui: &Ui, vertical_inset: f32) -> f32 {
    management_card_min_height_for_insets(
        ui.available_height(),vertical_inset,vertical_inset
    )
}

/// Same OUTER card height regardless of whether its role uses a symmetric
/// inset or the single-master table's zero bottom inset.
fn management_card_min_height_for_insets(
    available_height:f32,top_inset:f32,bottom_inset:f32
)->f32{
    (available_height-top_inset-bottom_inset
        -2.0*theme::FRAME_STROKE_WIDTH).max(0.0)
}

/// Both master tables preserve 16pt padding on all four inner edges.
/// Compute their inner minimum from the matching frame insets without
/// changing the outer card bounds or the shared row/scroll viewport edge.
pub fn management_master_table_card_min_inner_height(ui:&Ui)->f32{
    management_card_min_height_for_insets(
        ui.available_height(),theme::SECTION_PADDING,
        theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING
    )
}
pub fn fill_management_primary_card(ui: &mut Ui, min_inner_height: f32) {
    ui.set_min_width(ui.available_width().max(1.0));
    ui.set_min_height(min_inner_height);
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
fn management_single_list_scroll_height_geometry(
    card_bottom:f32,rows_top:f32,available_height:f32,reserve_below:f32
)->f32 {
    let remaining=(card_bottom-rows_top-reserve_below.max(0.0)).max(0.0);
    remaining.min(available_height.max(0.0)).max(1.0)
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

/// One shared 7pt rail extension for EVERY management table, regardless of
/// its page, outer frame or footer. Each caller owns ONLY the viewport height
/// (and reserves its footer separately); the scrollbar offset is not a
/// per-page layout policy. The native 3pt bottom inset leaves 4pt beyond rows.
pub fn management_table_scroll_track_rect(
    ui:&Ui,scroll_height:f32
)->Rect {
    let track=management_table_scroll_track_geometry(
        ui.available_rect_before_wrap(),scroll_height
    );
    #[cfg(test)]
    general_screen_geometry_tests::record("scroll rail",track);
    track
}

/// The only place that computes top/bottom of a management table rail.
/// The rail starts exactly below its fixed header (0pt gap); 7pt of common
/// extension minus 3pt optical shortening puts its end 4pt past the body.
/// The row viewport, data columns and any reserved footer stay unchanged.
fn management_table_scroll_track_geometry(
    bounds:Rect,scroll_height:f32
)->Rect {
    debug_assert!(scroll_height.is_finite()&&scroll_height>=0.0);
    let rows_bottom=(bounds.top()+scroll_height).min(bounds.bottom());
    let track_bottom=(rows_bottom+theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION
        -theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET)
        .max(bounds.top()+1.0);
    let track_top=(bounds.top()+theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET)
        .min(track_bottom-1.0);
    Rect::from_min_max(
        egui::pos2(bounds.left(),track_top),
        egui::pos2(bounds.right(),track_bottom)
    )
}

/// One measured vertical transition for every fixed-header management table.
///
/// Call with `item_spacing.y == 0` *from the toolbar allocation onward*.
/// This owns the symmetric 16pt toolbar-to-divider breathing room,
/// 1pt divider, 4pt after it, and 36pt sticky header. Screens must
/// not add a separate `add_space` or rely on egui's preceding item spacing.
#[derive(Clone,Copy,Debug)]
pub struct ManagementTableHeader {
    pub header_rect:Rect,
    pub header_content_rect:Rect,
    pub viewport_width:f32,
    pub scrollbar_gutter:f32,
    pub table_width:f32,
}

/// One minimum-width calculation for both sticky master tables and
/// other fixed-header management lists. The gutter supplied here is the
/// egui scrollbar allocation only (CONFLICT-003=A), without any spacer.
fn management_table_data_width(
    viewport_width:f32,scrollbar_gutter:f32,min_table_width:f32
)->f32{
    debug_assert!(scrollbar_gutter>=0.0 && scrollbar_gutter.is_finite());
    debug_assert!(viewport_width.is_finite() && min_table_width.is_finite());
    (viewport_width-scrollbar_gutter).max(min_table_width)
}

/// Standard 21pt transition for a toolbar that touches the card cursor.
/// Existing inset panels can consume part of the 16pt pre-divider margin.
pub fn management_sticky_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP
    )
}

/// CONFLICT-005 (approved): the parent toolbar/strip may already contain an
/// 8pt bottom inner margin. Subtract that existing inset from the 16pt
/// pre-divider breathing room instead of adding another full 21pt strip.
/// Call only for a directly adjacent toolbar and fixed table header.
pub fn management_sticky_table_header_after_inset(
    ui:&mut Ui,min_table_width:f32,consumed_bottom_inset:f32
)->ManagementTableHeader{
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,
        management_remaining_predivider_gap(consumed_bottom_inset)
    )
}

/// Pure SSOT conversion: the frame's existing bottom inset is part of the
/// 16pt pre-divider whitespace, not space to add on top of it.
fn management_remaining_predivider_gap(consumed_bottom_inset:f32)->f32{
    debug_assert!(consumed_bottom_inset.is_finite()&&consumed_bottom_inset>=0.0
        && consumed_bottom_inset<=theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP,
        "toolbar bottom inset must fit inside the standard top gap");
    (theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP-consumed_bottom_inset).max(0.0)
}

fn management_sticky_table_header_with_top_gap(
    ui:&mut Ui,min_table_width:f32,top_gap:f32
)->ManagementTableHeader{
    debug_assert!(ui.spacing().item_spacing.y.abs()<0.01,
        "table toolbar and header must use zero implicit vertical spacing");
    let viewport_width=ui.available_width().max(1.0);
    let scrollbar_gutter=ui.spacing().scroll.allocated_width();
    let table_width=management_table_data_width(
        viewport_width,scrollbar_gutter,min_table_width
    );

    // One allocation for the full toolbar-to-header separator strip.
    // Keeping it in a single Ui layout operation also avoids a fractional
    // pixel difference between manual add_space and automatic item gaps.
    let transition_height=top_gap+theme::SOFT_DIVIDER_WIDTH
        +theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP;
    let (transition,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,transition_height),Sense::hover()
    );
    ui.painter().rect_filled(
        Rect::from_min_size(
            egui::pos2(transition.left(),
                transition.top()+top_gap),
            Vec2::new(viewport_width,theme::SOFT_DIVIDER_WIDTH)
        ),0.0,theme::DIVIDER
    );
    #[cfg(test)]
    general_screen_geometry_tests::record("toolbar transition",transition);
    let (header_rect,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
        Sense::hover()
    );
    // The rightmost scrollbar rail is not another table column. Both the
    // sticky header and virtualized body should use the same data width.
    let header_content_rect=Rect::from_min_max(
        header_rect.min,
        egui::pos2(
            (header_rect.right()-scrollbar_gutter)
                .max(header_rect.left()+1.0),
            header_rect.bottom()
        )
    );
    // Fixed-header contract shared by BOTH approved master tables.
    // Validate real allocated Rects, not just intended token values.
    debug_assert!((header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<0.01,
        "management table header must retain the shared 36pt height");
    if header_rect.width()>=scrollbar_gutter+1.0 {
        debug_assert!((header_content_rect.right()+scrollbar_gutter
            -header_rect.right()).abs()<0.01,
            "header's data right edge must exclude the table scrollbar gutter");
    }
    #[cfg(test)]
    general_screen_geometry_tests::record("table header",header_rect);
    ManagementTableHeader{
        header_rect,header_content_rect,viewport_width,
        scrollbar_gutter,table_width
    }
}

/// CONFLICT-003=A: list-table data width reserves only the native
/// scrollbar allocation. CONFLICT-005: this *bare-header* variant is still
/// required when status messages, nested editor controls, or independent
/// section headings intervene between a toolbar and its table. For adjacent
/// toolbars use management_sticky_table_header[_after_inset] instead.
pub fn management_list_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    // A nested vertical ScrollArea can retain a content width wider than
    // the visible work area when a report table has large minimum columns.
    // The fixed header itself must remain inside the actual clipped viewport;
    // only its data rows may extend and scroll horizontally.
    let viewport_width=ui.available_rect_before_wrap()
        .intersect(ui.clip_rect()).width().max(1.0);
    let scrollbar_gutter=ui.spacing().scroll.allocated_width();
    let table_width=management_table_data_width(
        viewport_width,scrollbar_gutter,min_table_width
    );
    let (header_rect,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
        Sense::hover()
    );
    let header_content_rect=Rect::from_min_max(
        header_rect.min,
        egui::pos2((header_rect.right()-scrollbar_gutter)
            .max(header_rect.left()+1.0),header_rect.bottom())
    );
    debug_assert!(
        (header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<0.01,
        "management list header must use the shared 36pt height"
    );
    #[cfg(test)]
    general_screen_geometry_tests::record("table header",header_rect);
    ManagementTableHeader{
        header_rect,header_content_rect,viewport_width,
        scrollbar_gutter,table_width
    }
}

/// A history overview without interactive toolbar controls still has the
/// same fixed-header origin as the assignment master: 36pt toolbar role plus
/// the approved 21pt transition. Reserve those 57pt as ONE separator area,
/// not a phantom interactive toolbar widget with its own implicit spacing.
/// This keeps the first row and the 1pt divider on the master coordinates.
pub fn management_sticky_table_header_without_toolbar(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader {
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,
        theme::INPUT_HEIGHT+theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP
    )
}

/// Paint a freight master header's background and return its 34pt text area.
/// Shared by assignment, group management and combination history so text,
/// arrow centers and sort hit rects never use the lower 2pt outline.
pub fn management_master_header_begin(
    ui:&Ui,header_rect:Rect,header_content_rect:Rect
)->Rect {
    debug_assert!((header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT)
        .abs()<0.01);
    let text_rect=Rect::from_min_max(
        header_content_rect.min,
        egui::pos2(
            header_content_rect.right(),
            header_content_rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
        )
    );
    debug_assert!((text_rect.height()-
        (theme::MANAGEMENT_TABLE_HEADER_HEIGHT-
            theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT)).abs()<0.01);
    ui.painter().with_clip_rect(header_rect)
        .rect_filled(header_rect,0.0,theme::RAISED);
    text_rect
}

/// Draw the master header's solid 2pt bottom band AFTER labels, separators
/// and hover chrome. A centered 1pt line at bottom() is half clipped.
pub fn management_master_header_finish(ui:&Ui,header_rect:Rect){
    ui.painter().with_clip_rect(header_rect).rect_filled(
        Rect::from_min_max(
            egui::pos2(
                header_rect.left(),
                header_rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
            ),header_rect.right_bottom()
        ),0.0,theme::TABLE_HEADER_OUTLINE
    );
}

/// Identical sort-hover outline for all freight management list headers.
/// The caller supplies the column intersected with the 34pt text area.
pub fn management_master_header_sort_hover(ui:&Ui,hit:Rect){
    if hit.width()>0.0 && hit.height()>0.0 {
        let painter=ui.painter().with_clip_rect(hit);
        painter.rect_filled(
            hit,2.0,egui::Color32::from_rgba_unmultiplied(112,180,255,20)
        );
        painter.rect_stroke(
            hit.shrink(1.0),2.0,
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,
                theme::SELECTION.linear_multiply(0.82)),
            egui::StrokeKind::Inside
        );
    }
}

/// Plain management-register header for two-column editor/list pages.
/// The same master allocates the 21pt toolbar transition, 36pt fixed header,
/// native scrollbar gutter and inner 2pt bottom rule. Only the row content
/// is allowed to be taller than a compact 42pt product row.
pub fn management_master_register_header(
    ui:&mut Ui,primary:&str,action:&str,action_width:f32
)->ManagementTableHeader{
    let layout=management_sticky_table_header(ui,0.0);
    let text_rect=management_master_header_begin(
        ui,layout.header_rect,layout.header_content_rect
    );
    let x=(text_rect.right()-action_width).max(text_rect.left());
    let left=typography::freight_table_header_galley(ui,primary);
    let right=typography::freight_table_header_galley(ui,action);
    let painter=ui.painter().with_clip_rect(text_rect);
    // Long alias/brand titles must not run under the right-hand
    // management column on a narrow workspace.
    let primary_rect=Rect::from_min_max(
        text_rect.min,egui::pos2(x,text_rect.bottom())
    );
    painter.with_clip_rect(primary_rect).galley(
        egui::pos2(text_rect.left()+theme::SPACE_8,
            text_rect.center().y-left.size().y/2.0),
        left,theme::SECONDARY
    );
    painter.galley(
        egui::pos2((x+(action_width-right.size().x)/2.0)
            .max(x).min(text_rect.right()),
            text_rect.center().y-right.size().y/2.0),
        right,theme::SECONDARY
    );
    painter.line_segment(
        [egui::pos2(x,text_rect.top()),egui::pos2(x,text_rect.bottom())],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    management_master_header_finish(ui,layout.header_rect);
    layout
}

/// One 10pt / 1.8pt-stroke chevron for selectors, date stepping and
/// expand/collapse affordances. Rotate this same vector for every screen.
/// Draw the TWO diagonal arms as ONE continuous path. Separate
/// line_segment() calls blend anti-aliased coverage twice at their shared
/// corner, making the arrow tip darker and giving inconsistent edge colors
/// on Windows / fractional DPI. A single path has one joined stroke.
#[derive(Clone,Copy)]
pub(crate) enum ManagementChevronDirection { Down, Left, Right }

fn paint_chevron_at_angle(
    painter:&egui::Painter,center:egui::Pos2,angle:f32,optical_raise:f32,
    ink:Color32
){
    paint_chevron_at_angle_scaled(painter,center,angle,optical_raise,ink,1.0);
}

/// Shared chevron geometry, scaled only for constrained half-height segments.
fn paint_chevron_at_angle_scaled(
    painter:&egui::Painter,center:egui::Pos2,angle:f32,optical_raise:f32,
    ink:Color32,scale:f32
){
    let half_span=5.0*scale;
    let depth=2.5*scale;
    let stroke=(1.8*scale).max(1.0);
    let (s,c)=angle.sin_cos();
    let rotated=|x:f32,y:f32|egui::pos2(
        center.x+x*c-y*s,
        center.y+x*s+y*c-optical_raise
    );
    let a=rotated(-depth,-half_span);
    let tip=rotated(depth,0.0);
    let b=rotated(-depth,half_span);
    // One tessellated stroke owns the corner, diagonal edges and caps.
    // Preserve the approved 10pt shape, rotation and 1.8pt thickness.
    painter.add(egui::Shape::line(
        vec![a,tip,b],Stroke::new(stroke,ink)
    ));
}

fn paint_management_chevron_with_color(
    painter:&egui::Painter,center:egui::Pos2,
    direction:ManagementChevronDirection,color:Color32
){
    let (angle,raise)=match direction {
        ManagementChevronDirection::Right=>(0.0,0.0),
        ManagementChevronDirection::Down=>(std::f32::consts::FRAC_PI_2,1.0),
        ManagementChevronDirection::Left=>(std::f32::consts::PI,0.0),
    };
    paint_chevron_at_angle(painter,center,angle,raise,color);
}

pub(crate) fn paint_management_chevron(
    painter:&egui::Painter,center:egui::Pos2,direction:ManagementChevronDirection
){
    paint_management_chevron_with_color(
        painter,center,direction,theme::SECONDARY
    );
}

/// Native CollapsingHeader keeps its original click/keyboard/state/indent
/// behavior; only its default filled triangle becomes the common line icon.
/// Rotate smoothly with egui's animated openness (0 = right, 1 = down).
pub(crate) fn paint_collapsing_header_icon(
    ui:&mut Ui,openness:f32,response:&Response
){
    let t=openness.clamp(0.0,1.0);
    paint_chevron_at_angle(
        ui.painter(),response.rect.center(),
        t*std::f32::consts::FRAC_PI_2,t,theme::SECONDARY
    );
}

/// Configure the shared SINGLE-SELECT popup only. Never recolor
/// unrelated checkboxes, tables, global widget hover, or order selection.
fn management_dropdown_option_palette(ui:&mut Ui){
    let visuals=ui.visuals_mut();
    // egui::selectable_value uses selection.bg_fill for the current value.
    // Keep that persistent state darker than the pointer's transient hover.
    visuals.selection.bg_fill=theme::MANAGEMENT_DROPDOWN_SELECTED_BG;
    visuals.selection.stroke=Stroke::new(1.0,theme::TEXT);
    visuals.widgets.hovered.bg_fill=theme::MANAGEMENT_DROPDOWN_HOVER_BG;
    visuals.widgets.hovered.weak_bg_fill=theme::MANAGEMENT_DROPDOWN_HOVER_BG;
    visuals.widgets.hovered.bg_stroke=Stroke::new(
        theme::SOFT_DIVIDER_WIDTH,theme::SELECTION.linear_multiply(0.8)
    );
    visuals.widgets.hovered.fg_stroke=Stroke::new(1.5,theme::TEXT);
    visuals.widgets.active.bg_fill=theme::MANAGEMENT_DROPDOWN_HOVER_BG;
    visuals.widgets.active.weak_bg_fill=theme::MANAGEMENT_DROPDOWN_HOVER_BG;
}

/// Reference "접수일 기준" dropdown: 34pt dark trigger, separated
/// right-hand chevron, 1pt outline, 6pt corner radius. This replaces native
/// ComboBox triggers where management screens need the same visual language.
/// The caller retains its existing option values and change handling.
pub fn management_dropdown(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    options:impl FnOnce(&mut Ui)
)->Response {
    management_dropdown_inner(ui,label,width,enabled,tooltip,false,None,options)
}
/// Same management dropdown master, with an optional colored status mark.
/// This only changes the glyph painting; popup placement, hover and row
/// selection remain owned by the shared management dropdown.
pub fn management_status_dropdown(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    status_color:Color32,options:impl FnOnce(&mut Ui)
)->Response {
    management_dropdown_inner(
        ui,label,width,enabled,tooltip,false,Some(status_color),options
    )
}

/// Compact, two-option menus such as 선불 / 착불 never grow wider than their
/// own trigger. Other selectors may expand to fit longer catalog names.
pub fn management_dropdown_compact(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    options:impl FnOnce(&mut Ui)
)->Response {
    management_dropdown_inner(ui,label,width,enabled,tooltip,true,None,options)
}

/// Shared 34pt management trigger; the hierarchical category popup and
/// ordinary selectors must use the SAME paint, chevron and hover geometry.
pub(crate) fn management_dropdown_trigger(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    indicator:Option<Color32>,
)->Response {
    // Keep exact toolbar/cell width without stretching the subsequent action
    // or exceeding a narrow selector slot.
    let width=width.min(ui.available_width()).max(1.0);
    // Parent Ui may be disabled independently of the explicit enabled flag.
    let interactive=enabled && ui.is_enabled();
    let fill=if interactive{theme::INPUT}
        else{theme::MANAGEMENT_DROPDOWN_DISABLED_BG};
    let border=if interactive{theme::BORDER}
        else{theme::DISABLED_ACTION_BORDER};
    let ink=if interactive{theme::TEXT}
        else{theme::DISABLED_ACTION_TEXT};
    let arrow_ink=if interactive{theme::SECONDARY}
        else{theme::DISABLED_ACTION_TEXT};
    let trigger=ui.add_enabled(
        enabled,
        egui::Button::new("")
            .min_size(Vec2::new(width,theme::ACTION_STANDARD_HEIGHT))
            .fill(fill)
            .stroke(Stroke::new(theme::SOFT_DIVIDER_WIDTH,border))
            .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
    );
    let trigger=if tooltip.is_empty(){trigger}
        else{trigger.on_hover_text(tooltip)};
    let rect=trigger.rect;
    let painter=ui.painter().with_clip_rect(rect);
    let arrow_left=rect.right()-30.0;
    let caption=typography::management_control_text_galley(
        ui,label,ink
    );
    // Sales status selectors sit in a narrow price column. Reserve the
    // indicator without clipping the longest label ("판매준비중").
    let caption_x=rect.left()+if indicator.is_some(){
        theme::SPACE_24
    }else{theme::SPACE_12};
    let text_clip=Rect::from_min_max(
        egui::pos2(caption_x,rect.top()),
        egui::pos2((arrow_left-theme::SPACE_4).max(caption_x),rect.bottom())
    );
    if let Some(color)=indicator{
        painter.circle_filled(
            egui::pos2(rect.left()+theme::SPACE_12,rect.center().y),
            3.5,color
        );
    }
    painter.with_clip_rect(text_clip).galley(
        egui::pos2(caption_x,
            rect.center().y-caption.size().y/2.0
                +typography::MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y),
        caption,ink
    );
    painter.line_segment(
        [egui::pos2(arrow_left,rect.top()+5.0),
         egui::pos2(arrow_left,rect.bottom()-5.0)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    // Restore the pre-enlargement dropdown chevron dimensions; the date
    // prev/next controls rotate this same shared shape.
    paint_management_chevron_with_color(
        &painter,egui::pos2(rect.right()-15.0,rect.center().y),
        ManagementChevronDirection::Down,arrow_ink
    );
    if interactive {
        theme::paint_button_hover(
            ui,&trigger,theme::CONTROL_RADIUS as f32
        );
    }
    trigger
}

fn management_dropdown_inner(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    compact:bool,indicator:Option<Color32>,
    options:impl FnOnce(&mut Ui)
)->Response {
    let width=width.min(ui.available_width()).max(1.0);
    let trigger=management_dropdown_trigger(
        ui,label,width,enabled,tooltip,indicator
    );
    let rect=trigger.rect;
    egui::Popup::menu(&trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|menu|{
            // Every selectable row reports its parent trigger, never its
            // popup-row pointer, to the shared action-notice origin.
            crate::floating_notice::dropdown_popup_trigger(menu.ctx(),rect);
            menu.set_min_width(width);
            if compact {menu.set_max_width(width);}
            management_dropdown_option_palette(menu);
            // The popup frame owns its 4pt inset. Use full-height selectable
            // rows, so Hangul glyph ink has space above AND below its center.
            menu.spacing_mut().item_spacing.y=theme::SPACE_4;
            // Horizontal auto-shrink is mandatory: forcing a wide ScrollArea
            // caused 90pt 선불/착불 selectors to open ~600pt-wide menus.
            let scroll=egui::ScrollArea::vertical()
                .max_height(theme::MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT)
                .auto_shrink([true,true]);
            let scroll=if compact {scroll.max_width(width)}else{scroll};
            scroll.show(menu,|choices|{
                choices.spacing_mut().item_spacing.y=theme::SPACE_4;
                choices.spacing_mut().interact_size.y=
                    theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
                if compact{choices.set_width(width);}
                options(choices);
            });
        });
    trigger
}

/// One search-capable choice for a management single-select dropdown.
/// Its stable ID is the value passed to the existing catalog/filter workflow;
/// search_text can additionally contain localized names and registry aliases.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ManagementSearchChoice {
    pub value:String,
    pub label:String,
    pub search_text:String,
}
impl ManagementSearchChoice {
    pub fn new(value:String,label:String,search_text:String)->Self{
        Self{value,label,search_text}
    }
}

fn management_search_choice_matches(
    choice:&ManagementSearchChoice,query:&str
)->bool{
    let query=query.trim().to_lowercase();
    query.is_empty()
        ||choice.label.to_lowercase().contains(&query)
        ||choice.search_text.to_lowercase().contains(&query)
}

/// A searchable variant of the existing single-select master. The trigger,
/// palette, selection semantics, 28pt option rows, click-to-close and toast
/// anchor all remain shared; only a 36pt search field is added above the
/// internally scrolling choices. Search never changes the selected value.
pub fn management_searchable_dropdown(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    current:&mut String,options:&[ManagementSearchChoice],hint:&str,
)->Response{
    let trigger=management_dropdown_trigger(ui,label,width,enabled,tooltip,None);
    let rect=trigger.rect;
    let id=trigger.id.with("management-searchable-dropdown");
    let ctx=ui.ctx().clone();
    let mut search=ctx.data_mut(|data|
        data.get_temp::<String>(id).unwrap_or_default()
    );
    if trigger.clicked(){search.clear();}
    // The search field needs room even on a 148pt toolbar trigger, but
    // long brand names must never stretch the popup or surrounding toolbar.
    let popup_width=width.max(theme::MANAGEMENT_SEARCHABLE_POPUP_MIN_WIDTH)
        .min((ctx.screen_rect().width()-theme::SPACE_24).max(1.0));
    egui::Popup::menu(&trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|menu|{
            crate::floating_notice::dropdown_popup_trigger(menu.ctx(),rect);
            menu.set_min_width(popup_width);
            menu.set_max_width(popup_width);
            menu.set_width(popup_width);
            management_dropdown_option_palette(menu);
            menu.spacing_mut().item_spacing.y=theme::SPACE_4;
            let input_width=menu.available_width().max(1.0);
            management_search_field(menu,&mut search,input_width,hint);
            let visible=options.iter()
                .filter(|choice|management_search_choice_matches(choice,&search))
                .collect::<Vec<_>>();
            egui::ScrollArea::vertical()
                .id_salt("management-searchable-options")
                .max_height(theme::MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT)
                .auto_shrink([false,true])
                .show(menu,|list|{
                    list.spacing_mut().item_spacing.y=theme::SPACE_4;
                    list.spacing_mut().interact_size.y=
                        theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
                    if visible.is_empty(){
                        list.label(typography::caption("검색 결과가 없습니다."));
                    }else{
                        for choice in visible{
                            management_dropdown_option(
                                list,current,choice.value.clone(),&choice.label
                            );
                        }
                    }
                });
        });
    ctx.data_mut(|data|data.insert_temp(id,search));
    trigger
}

/// Paint only the option glyphs using the shared -1pt optical correction.
/// Native widgets still own row sizing, selection fill, hover, focus and clicks.
/// Normal options stay left-aligned; short two-choice menus stay centered.
fn management_dropdown_option_glyph(
    ui:&Ui,response:&Response,label:&str,compact:bool
){
    let galley=ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
        theme::TEXT,
    );
    let row=response.rect;
    let x=if compact{
        row.center().x-galley.size().x*0.5
    }else{
        row.left()+theme::SPACE_8
    };
    ui.painter().with_clip_rect(row.shrink(2.0)).galley(
        egui::pos2(
            x,row.top()+typography::management_dropdown_option_text_y(
                row.height(),galley.size().y
            ),
        ),
        galley,theme::TEXT,
    );
}

/// Long-name options retain native selectable_value sizing and selection
/// semantics. Invisible native text reserves its original width while our
/// shared painter controls the glyph baseline.
pub fn management_dropdown_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str
)->Response {
    let response=ui.selectable_value(
        current,value,egui::RichText::new(label).color(Color32::TRANSPARENT)
    );
    management_dropdown_option_glyph(ui,&response,label,false);
    if response.clicked(){
        crate::floating_notice::dropdown_option_selected(ui.ctx());
        ui.close();
    }
    response
}

/// A colored state row inside the SAME standard management menu.
pub fn management_status_dropdown_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str,color:Color32
)->Response {
    let response=ui.selectable_value(
        current,value,egui::RichText::new(label).color(Color32::TRANSPARENT)
    );
    let row=response.rect;
    let painter=ui.painter().with_clip_rect(row.shrink(2.0));
    painter.circle_filled(
        egui::pos2(row.left()+theme::SPACE_12,row.center().y),
        4.0,color
    );
    let galley=ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
        theme::TEXT
    );
    painter.galley(
        egui::pos2(
            row.left()+theme::SPACE_24,
            row.top()+typography::management_dropdown_option_text_y(
                row.height(),galley.size().y
            )
        ),
        galley,theme::TEXT
    );
    if response.clicked(){
        crate::floating_notice::dropdown_option_selected(ui.ctx());
        ui.close();
    }
    response
}

/// Compact options use the same glyph baseline, but preserve their fixed
/// width and centered caption independently of long-name management menus.
pub fn management_dropdown_compact_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str,width:f32
)->Response {
    let selected=*current==value;
    let mut response=ui.add_sized(
        Vec2::new(width,theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT),
        egui::Button::selectable(
            selected,egui::RichText::new(label).color(Color32::TRANSPARENT)
        )
    );
    management_dropdown_option_glyph(ui,&response,label,true);
    if response.clicked(){
        if *current!=value {
            *current=value;
            response.mark_changed();
        }
        crate::floating_notice::dropdown_option_selected(ui.ctx());
        ui.close();
    }
    response
}

/// Shared management tab roles (content width, no full-page stretching).
pub fn management_tabs(ui:&mut Ui,labels:&[(&str,bool,bool)]) -> Option<usize> {
    let mut pressed=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        for (index,(title,active,enabled)) in labels.iter().enumerate() {
            let role=if *active {theme::ButtonRole::ActiveManagementTab}
                else {theme::ButtonRole::ManagementTab};
            if theme::action(bar,title,role,*enabled).clicked() {
                pressed=Some(index);
            }
        }
    });
    pressed
}

/// Select a row in an editor's left-hand list. This is NOT a table
/// record: it has a 14pt item title and a separate 12pt state caption,
/// following the original 48pt message-template chooser hierarchy.
/// Painting is confined to the reserved fixed-height rectangle, so hover
/// and selected styles never change the following row positions.
pub fn management_chooser_row(
    ui:&mut Ui,id:&str,title:&str,status:&str,selected:bool
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
        theme::paint_button_hover(
            ui,&response,theme::CONTROL_RADIUS as f32
        );
    }
    response.on_hover_text(format!("{title} / {status}"))
}

/// Read-only presentation of a canonical client. Used by both management
/// rows and the compact Reception card; never modifies billing or matching.
#[derive(Clone,Debug)]
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
    pub fn hover_details(&self)->String{
        let mut parts=vec![self.name.clone()];
        if !self.address.trim().is_empty(){parts.push(format!("주소: {}",self.address));}
        if !self.phone.trim().is_empty(){parts.push(format!("전화번호: {}",self.phone));}
        if !self.mobile.trim().is_empty()&&self.mobile!=self.phone{
            parts.push(format!("휴대폰번호: {}",self.mobile));
        }
        if !self.conditions.is_empty(){parts.push(self.conditions.clone());}
        parts.join("\n")
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

/// Paint a measured text line at a literal left boundary. The header and
/// values call the same helper, so layout alignment never depends on egui's
/// Label allocation/halign behavior inside a wide subcell.
fn paint_client_line(
    ui:&Ui,cell:Rect,x:f32,galley:std::sync::Arc<egui::Galley>,color:Color32
) {
    let y=cell.center().y-galley.size().y/2.0;
    ui.painter().with_clip_rect(cell).galley(
        egui::pos2(x,y),galley,color
    );
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
    paint_client_line(
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

/// Single-line Reception client reference. It shares the management badge
/// and exceptional terms but does not reserve a redundant status/header row
/// or use the 900pt-wide management table geometry.
pub fn reception_client_summary_card(
    ui:&mut Ui,rect:Rect,linked:bool,
    summary:Option<&ClientDisplaySummary>,fallback:&str,
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
            let mut shown=name.to_owned();
            if full.size().x>name_limit{
                shown.clear();
                let ellipsis=typography::management_table_cell_galley(
                    ui,"…",typography::ManagementTableCellRole::ClientName
                );
                if ellipsis.size().x<=name_limit{
                    for ch in name.chars(){
                        let next=format!("{shown}{ch}…");
                        let measured=typography::management_table_cell_galley(
                            ui,&next,typography::ManagementTableCellRole::ClientName
                        );
                        if measured.size().x>name_limit{break;}
                        shown.push(ch);
                    }
                    shown.push('…');
                }
            }
            let shown_galley=typography::management_table_cell_galley(
                ui,&shown,typography::ManagementTableCellRole::ClientName
            );
            let name_width=shown_galley.size().x.min(name_limit);
            if name_width>0.0{
                let name_cell=Rect::from_min_max(
                    egui::pos2(x,row.top()),
                    egui::pos2(x+name_width,row.bottom())
                );
                paint_client_line(ui,name_cell,x,shown_galley,theme::TEXT);
            }
            if !terms.is_empty(){
                let terms_x=x+name_width+theme::SPACE_8;
                if terms_x<row.right(){
                    let terms_cell=Rect::from_min_max(
                        egui::pos2(terms_x,row.top()),row.right_bottom()
                    );
                    paint_client_line(
                        ui,terms_cell,terms_x,terms_galley,theme::MUTED
                    );
                }
            }
        }
    }
    // Register a hover surface first; the action rendered afterward keeps
    // its own click target and normal shared hover highlight.
    if let Some(data)=summary{
        ui.interact(
            rect,ui.make_persistent_id("source-client-card-hover"),Sense::hover()
        ).on_hover_text(data.hover_details());
    }else if linked&&!fallback.is_empty(){
        ui.interact(
            rect,ui.make_persistent_id("source-client-card-hover"),Sense::hover()
        ).on_hover_text(fallback);
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
            theme::action_with_min_width(
                buttons,button_label,theme::ButtonRole::Standard,
                true,button_width
            ).clicked()
        }
    ).inner
}

/// Shared management-table column separators. Freight Assignment and
/// Client Management both call this painter for header and body instead
/// of leaving vertical rules to independently implemented screen widgets.
///
/// The caller supplies DATA coordinates, not window coordinates. `region`
/// clips the header before its scrollbar gutter and keeps body rules inside
/// each virtualized row. Header bottom 2pt outline remains owned by the
/// existing `management_master_header_finish` function.
pub fn management_master_column_rules(
    ui:&Ui,region:Rect,boundaries:&[f32]
){
    let clip=region.intersect(ui.clip_rect());
    if clip.width()<=0.0 || clip.height()<=0.0{return;}
    let painter=ui.painter().with_clip_rect(clip);
    for &x in boundaries{
        if x>clip.left() && x<clip.right() {
            painter.line_segment(
                [egui::pos2(x,region.top()),egui::pos2(x,region.bottom())],
                Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
            );
        }
    }
}

/// The three compact columns keep fixed widths; the address gets all spare
/// width. The header, rows and rules share this one layout computation.
fn management_client_column_widths(table_width:f32)->[f32;4]{
    let name=theme::MANAGEMENT_CLIENT_NAME_COLUMN_WIDTH;
    let phone=theme::MANAGEMENT_CLIENT_PHONE_COLUMN_WIDTH;
    let terms=theme::MANAGEMENT_CLIENT_TERMS_COLUMN_WIDTH;
    [name,(table_width-name-phone-terms).max(0.0),phone,terms]
}
fn management_client_column_edges(table_left:f32,table_width:f32)->[f32;3]{
    let [name,address,phone,_]=management_client_column_widths(table_width);
    [
        table_left+name,
        table_left+name+address,
        table_left+name+address+phone
    ]
}

/// Client columns share the EXACT 36pt / 34pt text / 2pt footer band
/// of Freight Assignment. Column widths and 12pt label starts still
/// belong to the client row contract; only the master chrome is reused.
/// The visible data rect excludes the scrollbar, and horizontally scrolled
/// labels move together with the row data rather than with the rail.
pub fn management_client_header(
    ui:&mut Ui,header:Rect,header_content_rect:Rect,
    table_left:f32,table_width:f32,current_sort:Option<(usize,bool)>
)->Option<usize>{
    let titles=["거래처","주소","전화번호","거래 조건"];
    let sort_keys=["거래처명","주소","전화번호","가격 등급"];
    let text_row=management_master_header_begin(
        ui,header,header_content_rect
    );
    let painter=ui.painter().with_clip_rect(text_row);
    let column_widths=management_client_column_widths(table_width);
    let mut x=table_left;
    let mut selected=None;
    for (i,title) in titles.iter().enumerate(){
        let col_width=column_widths[i];
        let column=Rect::from_min_max(
            egui::pos2(x,text_row.top()),
            egui::pos2(x+col_width,text_row.bottom())
        );
        let visible=column.intersect(text_row);
        if visible.width()>0.0 {
            let response=ui.interact(
                visible,ui.make_persistent_id(("client-management-sort",i)),
                Sense::click()
            ).on_hover_text(format!(
                "{} · {}",sort_keys[i],match current_sort{
                    Some((col,false)) if col==i=>"내림차순 정렬",
                    Some((col,true)) if col==i=>"기본 순서로 복귀",
                    _=>"오름차순 정렬"
                }
            ));
            if response.clicked(){selected=Some(i);}
            if response.hovered(){
                management_master_header_sort_hover(ui,visible);
            }
            let text_rect=Rect::from_min_max(
                egui::pos2(column.left()+theme::SPACE_12,text_row.top()),
                egui::pos2(column.right()-theme::SPACE_12,text_row.bottom())
            ).intersect(text_row);
            if text_rect.width()>0.0 {
                let title_galley=typography::management_table_header_galley(ui,title);
                let (arrow,color)=match current_sort{
                    Some((col,false)) if col==i=>("▲",theme::SELECTION),
                    Some((col,true)) if col==i=>("▼",theme::SELECTION),
                    _=>("↕",theme::MUTED)
                };
                let arrow_galley=typography::management_table_header_galley(ui,arrow);
                let title_x=column.left()+theme::SPACE_12;
                let clip=painter.with_clip_rect(text_rect);
                clip.galley(
                    egui::pos2(title_x,
                        text_row.center().y-title_galley.size().y/2.0),
                    title_galley.clone(),theme::SECONDARY
                );
                clip.galley(
                    egui::pos2(title_x+title_galley.size().x+theme::SPACE_4,
                        text_row.center().y-arrow_galley.size().y/2.0),
                    arrow_galley,color
                );
            }
        }
        x+=col_width;
    }
    management_master_column_rules(
        ui,text_row,
        &management_client_column_edges(table_left,table_width)
    );
    management_master_header_finish(ui,header);
    selected
}

/// Four client columns, each a single line. Complete values stay available
/// on hover while address space grows with the table width.
pub fn management_row(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);4],heading:bool,tier:Option<&str>
) -> Response {
    management_row_impl(ui,row_id,columns,heading,tier,None)
}

/// Draw a client record with the shared low-contrast alternate-row surface.
/// The index is the visible row position after filtering, not a persistent
/// customer ID, so the stripes remain consistent during search.
pub fn management_row_striped(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);4],tier:Option<&str>,
    visible_index:usize
) -> Response {
    management_row_impl(ui,row_id,columns,false,tier,Some(visible_index))
}

fn management_row_impl(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);4],heading:bool,
    tier:Option<&str>,visible_index:Option<usize>
) -> Response {
    let width=ui.available_width().max(theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH);
    let height=if heading {theme::MANAGEMENT_TABLE_HEADER_HEIGHT}
        else {theme::MANAGEMENT_CLIENT_ROW_HEIGHT};
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,height),if heading {Sense::hover()} else {Sense::click()}
    );
    let bg=if heading {theme::RAISED}
        else if response.hovered() {theme::INPUT}
        else if visible_index.is_some_and(|index|index%2==1) {
            theme::TABLE_ROW_ALT_BG
        }else{theme::PANEL};
    ui.painter().rect_filled(rect,0.0,bg);
    // Paint the 1pt row rule inside its allocated rect. A centered
    // line_segment on bottom() was half-clipped and looked inconsistent
    // against the master's crisp 2pt header band at fractional DPI.
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(rect.left(),
                rect.bottom()-theme::SOFT_DIVIDER_WIDTH),
            rect.right_bottom()
        ),0.0,theme::DIVIDER
    );

    let mut x=rect.left();
    let column_widths=management_client_column_widths(rect.width());
    for (i,(primary,secondary)) in columns.iter().enumerate() {
        let col_width=column_widths[i];
        let cell=Rect::from_min_max(
            egui::pos2(x+theme::SPACE_12,rect.top()),
            egui::pos2(x+col_width-theme::SPACE_12,rect.bottom())
        );
        // Every header and value shares the column's leading inset.
        if heading {
            paint_client_line(
                ui,cell,cell.left(),typography::table_header_galley(ui,primary),
                theme::SECONDARY
            );
        }else if i==3 {
            let badge=paint_client_tier_badge(
                ui,cell,primary,tier.unwrap_or("")
            );
            if !secondary.is_empty(){
                let second=Rect::from_min_max(
                    egui::pos2((badge.right()+theme::SPACE_8).min(cell.right()),cell.top()),
                    cell.max
                );
                // Show only exceptions, left-aligned directly after the
                // tier badge; no blank payment/VAT slots or centering.
                paint_client_line(
                    ui,second,second.left(),
                    typography::client_table_metadata_galley(
                        ui,secondary,theme::MUTED
                    ),theme::MUTED
                );
            }
        }else{
            // Only the primary value is visible in a name/address/phone cell.
            // Secondary contacts remain available through the hover tooltip.
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
                paint_client_line(
                    ui,cell,cell.left(),galley,
                    if metadata{theme::MUTED}else{theme::TEXT}
                );
            }
        }
        // Keep full, unclipped information accessible without changing the
        // row-wide click-to-edit behavior or inserting extra visible controls.
        if !heading {
            let full=if secondary.is_empty(){primary.to_string()}
                else if primary.is_empty(){secondary.to_string()}
                else{format!("{primary} / {secondary}")};
            if !full.is_empty() {
                let hover_id=ui.make_persistent_id(("management-row-hover",row_id,i));
                ui.interact(cell,hover_id,Sense::hover()).on_hover_text(full);
            }
        }
        x+=col_width;
    }
    management_master_column_rules(
        ui,rect,
        &management_client_column_edges(rect.left(),rect.width())
    );
    if !heading {
        theme::paint_button_hover(ui,&response,0.0);
    }
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

/// Display-only won formatting. No business price or rounding decisions live
/// in the UI layer; amounts are supplied by the Rust business engine.
pub fn display_won(amount: i64) -> String {
    format!("{}원",display_money_number(amount))
}

fn display_money_number(amount:i64)->String{
    let digits = amount.to_string();
    let (sign, positive) = if let Some(text) = digits.strip_prefix('-') {
        ("-", text)
    } else { ("", digits.as_str()) };
    let mut groups = String::new();
    for (index, character) in positive.chars().enumerate() {
        if index > 0 && (positive.len() - index) % 3 == 0 {
            groups.push(',');
        }
        groups.push(character);
    }
    format!("{sign}{groups}")
}

pub fn summary_strip(ui: &mut Ui, label: &str, value: &str, footnote: &str) {
    ui.vertical(|ui| {
        soft_divider(ui);
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|ui| {
            ui.label(typography::field_label(label));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(typography::summary_value(value));
            });
        });
        let footnote_width=ui.available_width();
        fixed_status_slot(
            ui,footnote_width,typography::METADATA_LINE_HEIGHT,
            (!footnote.trim().is_empty()).then_some(footnote),
            (!footnote.trim().is_empty()).then_some(footnote),theme::MUTED
        );
    });
}

/// Geometry shared by recognized working orders and native workflow rows.
/// The name receives all width not reserved for the two narrow numeric columns.
/// Stable grid geometry for both the item header and rows.
/// The product column uses remaining viewport space, but the numeric columns
/// retain exactly 76pt and 136pt regardless of content or window dimensions.
/// Below the minimum, the entire grid scrolls instead of compressing columns.
fn reception_item_table_width(available:f32)->f32 {
    available.max(theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH)
}
/// The vertically scrolling item list reserves a native scrollbar rail.
/// Using the OUTER ScrollArea width for the three columns made the table
/// overflow by exactly that rail width whenever many products were added,
/// causing a spurious horizontal scrollbar even in a 1080p workspace.
/// Keep this rail reserved in both short and long lists so the item/header
/// columns do not jump sideways when vertical overflow first appears.
pub fn reception_items_scroll_content_width(ui:&Ui)->f32 {
    management_table_data_width(
        ui.available_width(),
        ui.spacing().scroll.allocated_width(),
        theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH
    )
}
fn reception_item_columns(rect: Rect) -> [Rect; 3] {
    let qty=theme::RECEPTION_ITEMS_QTY_WIDTH;
    let amount=theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH;
    let name_right=rect.right()-qty-amount;
    let qty_right=name_right+qty;
    [
        Rect::from_min_max(rect.min,egui::pos2(name_right,rect.bottom())),
        Rect::from_min_max(egui::pos2(name_right,rect.top()),
            egui::pos2(qty_right,rect.bottom())),
        Rect::from_min_max(egui::pos2(qty_right,rect.top()),rect.max),
    ]
}

/// Ellipsize only the displayed product label, using the exact 16pt glyph
/// width. This does not alter the Rust item title, matching key or hover text.
/// Unicode char boundaries prevent cutting a Hangul syllable or UTF-8 byte.
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

/// Actual editor rendered IN the already allocated input cell; no dialog,
/// detached form, or second layout row. Focus and response are owned by egui.
pub fn reception_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>,numeric:bool
)->Response{
    let rect=cell.shrink2(egui::vec2(theme::SPACE_4,theme::SPACE_2));
    let font=if numeric{
        typography::data_value_font(
            typography::DataValueKind::Money,
            typography::DataValueContext::Reception
        )
    }else{
        egui::FontId::proportional(typography::FONT_RECEPTION_RECIPIENT_TEXT)
    };
    reception_text_editor_at_click(
        ui,rect,
        egui::TextEdit::singleline(value)
            .font(font)
            .vertical_align(Align::Center)
            .horizontal_align(if numeric{Align::RIGHT}else{Align::LEFT})
            .margin(egui::Margin::symmetric(8,2))
            .desired_width(rect.width()),
        request_focus,click_at
    )
}

/// Money variant of the exact-rect editor: preserve the native caret,
/// table footprint and shared live separator/suffix behavior.
fn reception_money_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>
)->Response{
    reception_money_editor_at_rect(
        ui,cell.shrink2(egui::vec2(theme::SPACE_4,theme::SPACE_2)),
        value,request_focus,click_at
    )
}

fn reception_money_font()->egui::FontId{
    typography::data_value_font(
        typography::DataValueKind::Money,typography::DataValueContext::Reception
    )
}

fn reception_money_editor_at_rect(
    ui:&mut Ui,rect:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>
)->Response{
    // The preformatted draft is also the comparison baseline in both
    // callers. Normalize raw initial values for direct component reuse.
    if let Some(formatted)=formatted_money_input(value){*value=formatted;}
    let before=value.clone();
    let inset=theme::SPACE_8;
    let (edit,unit_galley)=money_text_edit_with_unit(
        ui,value,reception_money_font(),rect.width(),"원",inset,
        RECEPTION_MONEY_UNIT_GAP,egui::Margin::symmetric(8,2)
    );
    let response=reception_text_editor_at_click(
        ui,rect,edit,request_focus,click_at
    );
    normalize_money_edit(ui,value,&before,&response);
    paint_money_unit(ui,response.rect,unit_galley,inset);
    response
}

/// Quantity uses the shared date-selector frame and segment buttons.
/// Only the numeric draft is editable; the fixed suffix never enters the
/// native quantity parser or changes when the cell gains focus.
const RECEPTION_QUANTITY_MAX:i64=9_007_199_254_740_991;

fn reception_quantity_step(value:&str,delta:i64)->Option<String>{
    let count=value.trim().replace(',',"").parse::<i64>().ok();
    match (count,delta){
        (Some(n),-1) if n>1 && n<=RECEPTION_QUANTITY_MAX=>Some((n-1).to_string()),
        (Some(n),1) if n>=0 && n<RECEPTION_QUANTITY_MAX=>Some((n+1).to_string()),
        (None,1) if value.trim().is_empty()=>Some("1".into()),
        _=>None,
    }
}

pub struct ReceptionQuantityEditorResponse{
    pub input:Response,
    pub step_interaction:bool,
}

fn reception_quantity_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>
)->ReceptionQuantityEditorResponse{
    let rect=cell.shrink2(egui::vec2(theme::SPACE_2,theme::SPACE_2));
    let mut placed=ui.new_child(
        UiBuilder::new().id_salt("reception-quantity-editor")
            .max_rect(rect).layout(Layout::top_down(Align::Min))
    );
    placed.set_clip_rect(rect.intersect(ui.clip_rect()));
    let font=typography::data_value_font(
        typography::DataValueKind::Quantity,
        typography::DataValueContext::Reception
    );
    let decrease=reception_quantity_step(value,-1);
    let increase=reception_quantity_step(value,1);
    let (_, (input,step_interaction))=date_navigation_segmented_surface(
        &mut placed,"quantity-stepper",rect.size(),|segments,inner|{
            // Left arrow pair; right-aligned value and permanent unit.
            let arrow_width=16.0;
            let unit_width=segments.painter().layout_no_wrap(
                "개".to_owned(),font.clone(),theme::TEXT
            ).size().x;
            // Leave clearance for the rounded outer focus stroke.
            let arrow_rect=Rect::from_min_max(
                inner.min+egui::vec2(1.0,1.0),
                egui::pos2(inner.left()+arrow_width,inner.bottom()-1.0)
            );
            let unit_rect=Rect::from_min_max(
                egui::pos2(inner.right()-unit_width-2.0,inner.top()),
                egui::pos2(inner.right()-2.0,inner.bottom())
            );
            let input_rect=Rect::from_min_max(
                egui::pos2(arrow_rect.right()+2.0,inner.top()),
                egui::pos2(unit_rect.left(),inner.bottom()));
            let mut input_ui=segments.new_child(
                UiBuilder::new().id_salt("quantity-value")
                    .max_rect(input_rect).layout(Layout::top_down(Align::Min))
            );
            input_ui.set_clip_rect(input_rect.intersect(segments.clip_rect()));
            let input=reception_text_editor_at_click(
                &mut input_ui,input_rect,
                egui::TextEdit::singleline(value).font(font.clone())
                    .horizontal_align(Align::RIGHT).vertical_align(Align::Center)
                    .margin(egui::Margin::ZERO).frame(false)
                    .desired_width(input_rect.width()),
                request_focus,click_at
            );
            segments.painter().with_clip_rect(unit_rect).text(
                unit_rect.center(),egui::Align2::CENTER_CENTER,"개",font.clone(),theme::TEXT
            );
            let mut clicked=false;
            let half=arrow_rect.height()/2.0;
            for (index,caption,next,up) in [
                (0,"수량 1개 늘리기",increase,true),
                (1,"수량 1개 줄이기",decrease,false),
            ]{
                let part=Rect::from_min_size(
                    egui::pos2(arrow_rect.left(),arrow_rect.top()+index as f32*half),
                    egui::vec2(arrow_rect.width(),half)
                );
                let mut button=segments.new_child(
                    UiBuilder::new().id_salt(("quantity-arrow",index))
                        .max_rect(part).layout(Layout::left_to_right(Align::Center))
                );
                button.set_clip_rect(part.intersect(segments.clip_rect()));
                button.spacing_mut().item_spacing=Vec2::ZERO;
                // Do not let the standard button's minimum padding enlarge
                // the half-height arrow slots.
                button.spacing_mut().interact_size=Vec2::ZERO;
                button.spacing_mut().button_padding=Vec2::ZERO;
                let response=date_navigation_segment_button(
                    &mut button,arrow_rect.width(),caption,next.is_some(),
                    DateNavigationSegmentGlyph::CompactVerticalChevron{up}
                );
                if response.clicked(){
                    if let Some(next)=next{*value=next;clicked=true;}
                }
            }
            let stroke=Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER);
            segments.painter().line_segment(
                [arrow_rect.right_top(),arrow_rect.right_bottom()],stroke
            );
            segments.painter().line_segment(
                [egui::pos2(arrow_rect.left(),arrow_rect.center().y),
                 egui::pos2(arrow_rect.right(),arrow_rect.center().y)],stroke
            );
            // Pointer-down can remove TextEdit focus before the click is
            // released. Keep that draft alive until the arrow action resolves.
            let pressing=segments.input(|input|
                (input.pointer.any_down() || input.pointer.any_released())
                    && input.pointer.interact_pos().is_some_and(|p|arrow_rect.contains(p))
            );
            if clicked || pressing{input.request_focus();}
            (input,clicked||pressing)
        }
    );
    if input.has_focus() || step_interaction{
        placed.painter().rect_stroke(
            rect.shrink(0.5),theme::CONTROL_RADIUS as f32,
            Stroke::new(2.0,theme::SELECTION),egui::StrokeKind::Inside
        );
    }
    ReceptionQuantityEditorResponse{input,step_interaction}
}

#[cfg(test)]
#[test]
fn reception_quantity_step_keeps_unit_out_of_native_draft_and_respects_bounds(){
    assert_eq!(reception_quantity_step("5",1).as_deref(),Some("6"));
    assert_eq!(reception_quantity_step("5",-1).as_deref(),Some("4"));
    assert_eq!(reception_quantity_step("",1).as_deref(),Some("1"));
    assert_eq!(reception_quantity_step("1",-1),None);
    assert_eq!(reception_quantity_step("5개",1),None);
    assert_eq!(reception_quantity_step(&RECEPTION_QUANTITY_MAX.to_string(),1),None);
}

#[derive(Clone,Copy)]
pub enum ReceptionNumericCell{Quantity,Money}

/// A stepper click may temporarily move the text focus. Both consumers
/// must retain the draft until that click resolves.
pub fn reception_numeric_edit_finished(response:&Response,step_interaction:bool)->bool{
    response.lost_focus() && !step_interaction
}

pub fn reception_item_numeric_editor(
    ui:&mut Ui,rect:Rect,value:&mut String,focus:bool,
    click_at:Option<egui::Pos2>,kind:ReceptionNumericCell
)->(Response,bool){
    match kind{
        ReceptionNumericCell::Quantity=>{
            let editor=reception_quantity_editor(ui,rect,value,focus,click_at);
            (editor.input,editor.step_interaction)
        }
        ReceptionNumericCell::Money=>(
            reception_money_inline_cell_editor(ui,rect,value,focus,click_at),false
        )
    }
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
                |actions|theme::action(actions,"삭제",theme::ButtonRole::Danger,!busy).clicked()
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

/// Product text needs equal breathing room on both sides of the blue
/// outline. Header and rows use the same 4pt extra horizontal inset;
/// quantity/amount alignment and highlight geometry remain untouched.
fn reception_item_text_clip(rect:Rect,index:usize)->Rect {
    if index==0 {
        rect.shrink2(egui::vec2(theme::SPACE_4,0.0))
    }else{rect}
}

pub fn reception_editable_items_header(ui:&mut Ui){
    reception_items_header_with_actions(ui,"금액",true);
}

pub fn reception_items_header(ui:&mut Ui){
    reception_items_header_with_actions(ui,"금액",false);
}

// Only these two fixed public headers may be used: read-only or editable.
fn reception_items_header_with_actions(ui:&mut Ui,amount_heading:&str,remove_slot:bool){
    let action_width=if remove_slot{theme::RECEPTION_ITEM_REMOVE_WIDTH}else{0.0};
    let width=ui.available_width().max(theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+action_width);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_HEADER_HEIGHT), Sense::hover()
    );
    // The reception list has its own approved 32pt compact header, but
    // follows the master's raised header surface and INSIDE bottom band.
    // No extra layout height or clipped 1pt edge is introduced.
    ui.painter().rect_filled(rect,0.0,theme::RAISED);
    let columns = reception_item_columns(Rect::from_min_max(
        rect.min,egui::pos2(rect.right()-action_width,rect.bottom())
    ));
    for (index, label) in ["품목", "수량", amount_heading].iter().enumerate() {
        // Header and value cells use identical text start coordinates.
        let cell = reception_item_text_clip(
            columns[index].shrink2(egui::vec2(theme::SPACE_8, 0.0)),index
        );
        let galley = typography::management_table_header_galley(ui, label);
        // The product header starts at the same LEFT edge as product text;
        // quantity and amount end at the same RIGHT edges as their values.
        let x = if index == 0 {
            cell.left()
        } else {
            cell.right() - galley.size().x
        };
        let text_bottom = cell.bottom() - theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT;
        let text_center_y = (cell.top() + text_bottom) / 2.0;
        ui.painter().with_clip_rect(cell).galley(
            egui::pos2(x, text_center_y - galley.size().y / 2.0),
            galley, theme::SECONDARY,
        );
    }
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(
                rect.left(),
                rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
            ),
            rect.right_bottom()
        ),
        0.0,theme::TABLE_HEADER_OUTLINE
    );
}

/// Use the actual laid-out glyph box for all value cells.
/// Product text is left aligned; quantity and amount share a right edge and
/// the exact same vertical midpoint independently of their string length.
fn reception_item_value_origin(cell:Rect,glyphs:Vec2,right:bool)->egui::Pos2 {
    egui::pos2(
        if right {cell.right()-glyphs.x} else {cell.left()},
        cell.center().y-glyphs.y/2.0,
    )
}

pub fn reception_item_row(
    ui: &mut Ui, name: &str, quantity: &str, amount: &str,
    active: [bool; 3],
) -> ReceptionItemCells {
    reception_item_row_with_mode(ui,name,quantity,amount,active,false,false)
}
pub fn reception_item_row_with_remove(
    ui:&mut Ui,name:&str,quantity:&str,amount:&str,active:[bool;3],editable:bool,
)->ReceptionItemCells{
    reception_item_row_with_mode(ui,name,quantity,amount,active,editable,true)
}

fn reception_item_row_with_mode(
    ui: &mut Ui, name: &str, quantity: &str, amount: &str,
    active: [bool; 3],editable:bool,remove_slot:bool,
) -> ReceptionItemCells {
    let action_width=if remove_slot{theme::RECEPTION_ITEM_REMOVE_WIDTH}else{0.0};
    let width=ui.available_width().max(theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH+action_width);
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
        if active[index] {
            source_highlight::paint_active_background(ui,cell);
        }
        // Text must stay clear of the 2pt inset outline. Keeping the
        // value clip and its anchor inside the highlighted cell also stops
        // the last digit / descender from touching a blue edge.
        let value_clip=reception_item_text_clip(
            cell.shrink2(egui::vec2(theme::SPACE_4,0.0)),index
        );
        // Use the same font metrics as Painter::text, not a character-count
        // guess. Only the product column truncates; financial values do not.
        let visible_product=(index==0).then(||reception_ellipsized_product(
            text,(value_clip.width()-0.5).max(0.0),
            |candidate|ui.painter().layout_no_wrap(
                candidate.to_owned(),font.clone(),color
            ).size().x,
        ));
        let ink=if active[index] {source_highlight::ACTIVE_TEXT} else {color};
        let label=visible_product.as_deref().unwrap_or(row_text);
        let galley=ui.painter().layout_no_wrap(label.to_owned(),font,ink);
        let origin=reception_item_value_origin(value_clip,galley.size(),right);
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
    ).on_hover_text(name);
    let product = if editable { product.on_hover_cursor(egui::CursorIcon::PointingHand) } else { product };
    let quantity = ui.interact(
        cells[1], ui.make_persistent_id(("item-cell", row_id, 1)),
        if editable{Sense::click()}else{Sense::hover()},
    ).on_hover_text(quantity);
    let quantity = if editable { quantity.on_hover_cursor(egui::CursorIcon::Text) } else { quantity };
    let amount = ui.interact(
        cells[2], ui.make_persistent_id(("item-cell", row_id, 2)),
        if editable{Sense::click()}else{Sense::hover()},
    ).on_hover_text(amount);
    let amount = if editable { amount.on_hover_cursor(egui::CursorIcon::Text) } else { amount };
    ui.painter().line_segment(
        [egui::pos2(rect.left(), rect.bottom()),
         egui::pos2(rect.right(), rect.bottom())],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER)
    );
    // The renderer owns both fill and border. Do not let reception page
    // branches independently paint an outline: pending board previews
    // otherwise receive blue fills without the matching blue stroke.
    for (response,on) in [(&product,active[0]),(&quantity,active[1]),
        (&amount,active[2])]{
        source_highlight::paint_active_if(ui,response,on);
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
        if enabled{theme::paint_button_hover(ui,&action,theme::CONTROL_RADIUS as f32);}
        let center=hit.center();
        let stroke=Stroke::new(1.5,ink);
        for sign in [-1.0_f32,1.0]{
            ui.painter().line_segment([
                center+egui::vec2(-4.0,-4.0*sign),
                center+egui::vec2(4.0,4.0*sign)
            ],stroke);
        }
        remove_clicked=enabled && action.on_hover_text("품목 삭제").clicked();
    }
    #[cfg(test)]
    for (name,response) in [("item product",&product),("item quantity",&quantity),("item amount",&amount)]{
        general_screen_geometry_tests::record(name,response.rect);
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
}
/// The unit occupies the former external label's width, keeping the
/// overall freight/COD group stable. Resting and focused money controls
/// use identical suffix geometry; only their interaction mode differs.
const RECEPTION_FREIGHT_MONEY_WIDTH:f32=104.0;

fn reception_freight_money_readonly(ui:&mut Ui,value:&mut String)->Response{
    let inset=theme::SPACE_8;
    let (edit,unit_galley)=money_text_edit_with_unit(
        ui,value,reception_money_font(),RECEPTION_FREIGHT_MONEY_WIDTH,
        "원",inset,RECEPTION_MONEY_UNIT_GAP,egui::Margin::symmetric(8,2)
    );
    let response=ui.add_sized(
        Vec2::new(RECEPTION_FREIGHT_MONEY_WIDTH,theme::INPUT_HEIGHT),
        edit.interactive(false)
    );
    paint_money_unit(ui,response.rect,unit_galley,inset);
    response
}

pub fn reception_freight_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>
)->Response{
    reception_money_editor_at_rect(ui,cell,value,request_focus,click_at)
}
pub fn reception_freight_row(
    ui: &mut Ui, label: &str, amount: Option<i64>, collect: Option<bool>,
    remote: Option<bool>, inquiry_visible: bool,
) {
    let _=reception_freight_row_editable(
        ui,label,amount,collect,remote,inquiry_visible,false
    );
}
/// Do not swap the COD selector for a textual caption during a Rust save.
/// The shared dropdown keeps its width, baseline, outline and chevron in
/// BOTH states; only hit-testing is disabled while another edit is pending.
fn reception_cod_dropdown(
    ui:&mut Ui,collect:Option<bool>,editable:bool
)->(bool,Response){
    let mut selected=collect.unwrap_or(false);
    let label=match collect {
        Some(true)=>"착불",
        Some(false)=>"선불",
        None if editable=>"선불",
        None=>"—",
    };
    let response=management_dropdown_compact(ui,label,90.0,editable,"",|menu|{
        management_dropdown_compact_option(menu,&mut selected,false,"선불",90.0);
        management_dropdown_compact_option(menu,&mut selected,true,"착불",90.0);
    });
    (editable && selected!=collect.unwrap_or(false),response)
}

// Combined intrinsic controls need more width than the fixed 392pt
// product table. Above this threshold they remain in one horizontal
// line; below it the right-hand actions get their own full-width row.
const RECEPTION_FREIGHT_INLINE_MIN_WIDTH:f32=560.0;
fn freight_actions_need_second_row(available_width:f32)->bool{
    available_width<RECEPTION_FREIGHT_INLINE_MIN_WIDTH
}

pub fn reception_freight_row_editable(
    ui:&mut Ui,label:&str,amount:Option<i64>,collect:Option<bool>,
    remote:Option<bool>,inquiry_visible:bool,editable:bool
)->ReceptionFreightEdit{
    let mut clicked=false;
    let mut amount_click_pos=None;
    let mut remote_clicked=false;
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
                if editable && collect!=Some(true){
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
        let (changed,_response)=reception_cod_dropdown(line,collect,editable);
        collect_clicked=changed;
    };
    let mut draw_remote_and_inquiry=|line:&mut Ui|{
        line.spacing_mut().item_spacing.x=theme::SPACE_4;
        // Right edge matches React: disabled inquiry is the rightmost
        // action and the 도서/산간 checkbox is immediately to its left.
        line.with_layout(Layout::right_to_left(Align::Center),|right|{
            let _=inquiry_visible;
            theme::action(right,"공장에 운임 문의",theme::ButtonRole::Standard,false);
            let mut checked=remote.unwrap_or(false);
            if right.add_enabled(editable && remote.is_some(),
                egui::Checkbox::new(&mut checked,"도서/산간")
            ).changed(){
                remote_clicked=true;
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
    #[cfg(test)]
    if let Some(rect)=amount_rect{general_screen_geometry_tests::record("freight amount",rect);}
    ReceptionFreightEdit {
        amount_rect,amount_clicked:clicked,amount_click_pos,
        remote_clicked,collect_clicked
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

/// Shared add-product action; the reception title row owns its placement.
pub fn reception_item_add_action_editable(ui:&mut Ui,editable:bool)->bool{
    reception_item_header_action(ui,false,editable).clicked()
}

/// The add/rollback modes occupy exactly the same header action slot.
/// Never insert a pending-assignment action between the item and freight rows.
pub fn reception_item_header_action(ui:&mut Ui,rollback:bool,enabled:bool)->Response{
    theme::action_with_min_width(
        ui,if rollback{"선택 취소"}else{"＋ 상품 추가"},
        theme::ButtonRole::Standard,enabled,112.0
    )
}

/// Persistent validation is attached to an existing cell, not an extra row.
/// The small marker and tooltip consume no parent layout space.
pub fn reception_item_issue_feedback(ui:&Ui,cell:&Response,message:&str){
    if message.trim().is_empty(){return;}
    let rect=cell.rect.intersect(ui.clip_rect());
    if rect.width()>12.0 && rect.height()>12.0{
        ui.painter().with_clip_rect(rect).circle_filled(
            egui::pos2(rect.right()-4.0,rect.top()+4.0),2.5,theme::SELECTED_ORDER
        );
    }
    cell.clone().on_hover_text(message);
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

    fn render(width:f32,pending:bool,issues:bool,editable:bool)->Vec<(&'static str,Rect)>{
        let ctx=test_context();
        let mut rects=Vec::new();
        let _=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,800.0))),
            ..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let header=reception_item_header_action(ui,pending,editable);
                rects.push(("header action",header.rect));
                reception_editable_items_header(ui);
                let cells=reception_item_row_with_remove(
                    ui,"BD 709-1",if pending{"—"}else{"1개"},"—",[false;3],editable
                );
                rects.extend([("product",cells.product.rect),("quantity",cells.quantity.rect),
                    ("amount",cells.amount.rect)]);
                if issues{
                    reception_item_issue_feedback(ui,&cells.product,
                        "수량 확인 필요 / 금액 확인 필요 / 긴 오류 메시지");
                }
                ui.add_space(theme::SPACE_8);
                let freight=reception_freight_row_editable(
                    ui,"운임",Some(3000),Some(false),Some(false),true,editable
                );
                rects.push(("freight amount",freight.amount_rect.expect("freight amount slot")));
                ui.add_space(theme::SPACE_8);
                let origin=ui.cursor().top();
                reception_item_origin_action_editable(ui,Some("FACTORY"),editable);
                rects.push(("origin row",Rect::from_min_max(
                    egui::pos2(ui.min_rect().left(),origin),
                    egui::pos2(ui.min_rect().right(),ui.cursor().top())
                )));
                let footer=theme::action(ui,"초안에 적용",theme::ButtonRole::Standard,editable);
                rects.push(("following action",footer.rect));
            });
        });
        rects
    }

    #[test]
    fn pending_assignment_and_issue_feedback_do_not_move_reception_controls(){
        for width in [480.0,800.0,1200.0]{
            let base=render(width,false,false,true);
            for (pending,issues,editable) in [
                (true,false,true),(true,true,true),(false,true,true),(true,true,false)
            ]{
                assert_layout_rects_invariant(&base,&render(width,pending,issues,editable));
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
        let production=manual.split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("theme::action(items_ui,\"상품 선택 취소\""));
        assert!(production.contains("components::reception_item_header_action("));
        let main=include_str!("main.rs");
        assert!(!main.contains("items_ui.label(typography::status("));
        assert!(main.contains("components::reception_item_issue_feedback("));
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
                let response=right.add_enabled(editable,
                    egui::Button::new(typography::action_label(
                        label,typography::FONT_STANDARD_ACTION,false,
                        if selected{theme::TEXT}else{theme::SECONDARY}
                    ))
                    .min_size(Vec2::new(42.0,theme::ACTION_STANDARD_HEIGHT))
                    .fill(if selected{theme::RAISED}else{theme::PANEL})
                    .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH,
                        if selected{theme::SELECTION}else{theme::DIVIDER}))
                    .corner_radius(theme::CONTROL_RADIUS)
                );
                theme::paint_button_hover(
                    right,&response,theme::CONTROL_RADIUS as f32
                );
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
    note: &str,
) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), theme::RECEPTION_TOTAL_HEIGHT),
        Sense::hover(),
    );
    // Both summary lines occupy fixed vertical centers. The containing
    // order-total card already has the separating header line.
    let left = rect.left() + theme::SPACE_8;
    let right = rect.right() - theme::SPACE_8;
    let right_start = (right - 148.0).max(left + 50.0);
    let amount = total.map(display_money_number)
        .unwrap_or_else(||RECEPTION_MONEY_PENDING.to_owned());
    let first_amount = Rect::from_min_max(
        egui::pos2(right_start, rect.top()), egui::pos2(right, rect.top() + 30.0),
    );
    let font=typography::data_value_font(
        typography::DataValueKind::Money,
        typography::DataValueContext::ReceptionTotal
    );
    // The currency unit is always present and has a fixed right anchor.
    // Pending, zero and calculated amounts only change the numeric galley.
    let painter=ui.painter().with_clip_rect(first_amount.intersect(ui.clip_rect()));
    let unit=painter.layout_no_wrap("원".to_owned(),font.clone(),theme::TEXT);
    let unit_left=money_unit_left(right,0.0,unit.size().x);
    let center_y=rect.top()+17.0;
    painter.text(
        egui::pos2(unit_left-RECEPTION_MONEY_UNIT_GAP,center_y),
        egui::Align2::RIGHT_CENTER,&amount,font,theme::TEXT
    );
    painter.galley(egui::pos2(unit_left,center_y-unit.size().y/2.0),unit,theme::TEXT);
    // A missing shipment count is unknown, not zero.
    let count_text = count.map(|n| format!("배송 {n}건"))
        .unwrap_or_else(|| "배송 건수 미확정".to_owned());
    let second_left = Rect::from_min_max(
        egui::pos2(left, rect.top() + 30.0),
        egui::pos2(right_start, rect.bottom()),
    );
    ui.painter().with_clip_rect(second_left).text(
        egui::pos2(left, rect.top() + 43.0), egui::Align2::LEFT_CENTER,
        count_text, egui::FontId::proportional(typography::FONT_METADATA),
        theme::SECONDARY,
    );
    let second_right = Rect::from_min_max(
        egui::pos2(right_start, rect.top() + 30.0),
        egui::pos2(right, rect.bottom()),
    );
    ui.painter().with_clip_rect(second_right).text(
        egui::pos2(right, rect.top() + 43.0), egui::Align2::RIGHT_CENTER,
        vat, egui::FontId::proportional(typography::FONT_METADATA),
        theme::SECONDARY,
    );
    if !note.trim().is_empty() {
        response.on_hover_text(note);
    }
}

pub fn record_card(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::PANEL_RADIUS)
        .inner_margin(theme::CARD_PADDING)
        .show(ui, content);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusTone {
    Neutral,
    Active,
    Complete,
    Warning,
    Error,
    Linked,
}
impl StatusTone {
    pub fn color(self) -> Color32 {
        match self {
            Self::Neutral => theme::BORDER,
            Self::Active | Self::Warning => theme::SELECTED_ORDER,
            Self::Complete => theme::SUCCESS,
            Self::Error => theme::DANGER,
            Self::Linked => theme::SELECTION,
        }
    }
}

pub fn status_dots(ui: &mut Ui, tones: &[StatusTone]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for tone in tones {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(12.0, 18.0), egui::Sense::hover(),
            );
            ui.painter().circle_filled(rect.center(), 4.5, tone.color());
        }
    });
}

/// Read-only embedded management table with the shared fixed-header
/// geometry, scrollbar gutter and virtual fixed-row viewport. Screens provide
/// only their column labels, width policy and already formatted cell values.
pub fn management_readonly_table(
    ui:&mut Ui,
    id:&str,
    headings:&[&str],
    rows:&[Vec<String>],
    max_height:f32,
    widths_for_available:impl FnOnce(f32)->Vec<f32>,
){
    assert!(!headings.is_empty(),"a management table needs at least one column");
    let original_scroll=ui.spacing().scroll.clone();
    ui.spacing_mut().scroll=theme::management_table_scroll_style();
    let viewport_width=ui.available_width().max(1.0);
    let available_data_width=(viewport_width
        -ui.spacing().scroll.allocated_width()).max(1.0);
    let widths=widths_for_available(available_data_width);
    assert_eq!(headings.len(),widths.len(),
        "management table heading and width counts must match");
    assert!(widths.iter().all(|width|width.is_finite()&&*width>0.0),
        "management table column widths must be finite and positive");
    let minimum_width=management_readonly_table_content_width(&widths);
    let layout=management_list_table_header(ui,minimum_width);

    // Embedded review tables retain their existing plain header appearance.
    // The shared list-header allocation owns the 36pt height and scrollbar
    // gutter; text uses the same table-header role as the management masters.
    let header_clip=layout.header_content_rect.intersect(ui.clip_rect());
    let painter=ui.painter().with_clip_rect(header_clip);
    let mut x=layout.header_rect.left();
    for (heading,width) in headings.iter().zip(&widths){
        let galley=typography::table_header_galley(ui,heading);
        let y=layout.header_rect.center().y-galley.size().y/2.0;
        if header_clip.width()>0.0&&header_clip.height()>0.0{
            painter.with_clip_rect(egui::Rect::from_min_max(
                egui::pos2(x,layout.header_rect.top()),
                egui::pos2((x+*width).min(layout.header_rect.right()),
                    layout.header_rect.bottom())
            )).galley(egui::pos2(x,y),galley,theme::SECONDARY);
        }
        x+=*width+theme::SPACE_8;
    }

    let row_height=theme::MANAGEMENT_COMPACT_ROW_HEIGHT;
    let row_advance=management_readonly_table_row_advance();
    let scroll=egui::ScrollArea::both()
        .id_salt(id)
        .max_height(max_height)
        .auto_shrink([false,false]);
    let _=virtual_fixed_rows(
        scroll,ui,layout.table_width,row_advance,rows.len(),
        |body,range|{
            for index in range{
                let values=&rows[index];
                debug_assert_eq!(values.len(),widths.len(),
                    "management table row must match its column count");
                body.horizontal(|line|{
                    line.spacing_mut().item_spacing.x=theme::SPACE_8;
                    for (value,width) in values.iter().zip(&widths){
                        line.add_sized(
                            Vec2::new(*width,row_height),
                            egui::Label::new(typography::table_cell(value))
                                .truncate()
                        ).on_hover_text(value);
                    }
                });
                soft_divider(body);
            }
        }
    );
    ui.spacing_mut().scroll=original_scroll;
}

fn management_readonly_table_content_width(widths:&[f32])->f32{
    widths.iter().sum::<f32>()
        +widths.len().saturating_sub(1) as f32*theme::SPACE_8
}

fn management_readonly_table_row_advance()->f32{
    theme::MANAGEMENT_COMPACT_ROW_HEIGHT+theme::SOFT_DIVIDER_WIDTH
}

#[cfg(test)]
mod management_date_field_contract_tests {
    #[test]
    fn order_history_date_group_uses_shared_date_text_field(){
        let source=include_str!("order_history_manager.rs");
        let date=source.split("fn range_date_picker_rtl(").nth(1).unwrap()
            .split("\nfn ").next().unwrap();
        assert!(date.contains("components::management_date_text_field("));
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
        assert!(section.contains("components::fixed_scroll_panel("));
        assert!(!section.contains("egui::ScrollArea::vertical()"));
        assert!(!section.contains("ui.painter().rect("));
    }
}

#[cfg(test)]
mod shared_detail_divider_contract_tests {
    #[test]
    fn order_history_detail_uses_shared_soft_dividers(){
        let source=include_str!("order_history_detail.rs");
        assert_eq!(source.matches("components::soft_divider(").count(),3);
        assert!(!source.contains(".separator("));
    }
}

#[cfg(test)]
mod shared_review_card_contract_tests {
    #[test]
    fn reception_review_cards_use_shared_surface_frame(){
        let source=include_str!("review_panel.rs");
        assert_eq!(source.matches("theme::review_issue_card(").count(),3);
        assert!(!source.contains("egui::Frame::new()"));
        let fallback=include_str!("components.rs").split("pub fn reception_review_status(")
            .nth(1).unwrap().split("\n///").next().unwrap();
        assert!(fallback.contains("theme::review_issue_card("));
    }
}

#[cfg(test)]
mod shared_multiline_field_contract_tests {
    #[test]
    fn automatic_message_and_history_memo_use_shared_multiline_editor(){
        let messages=include_str!("automatic_message_manager.rs");
        assert_eq!(messages.matches("components::multiline_text_field(").count(),2);
        assert!(!messages.contains("TextEdit::multiline("));
        assert_eq!(messages.matches("theme::preview_surface_frame()").count(),2);
        assert!(!messages.contains("egui::Frame::new()"));
        let history=include_str!("order_history_manager.rs");
        let memo=history.split("pub fn show_memo_modal(").nth(1).unwrap()
            .split("pub fn ").next().unwrap();
        assert!(memo.contains("components::multiline_text_field("));
        assert!(!memo.contains("TextEdit::multiline("));

        let main=include_str!("main.rs");
        let ocr=main.split("fn show_ocr_review(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(ocr.contains("components::multiline_text_field_sized("));
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
mod shared_checkbox_contract_tests {
    #[test]
    fn vat_dialog_uses_shared_checkbox_and_nested_frame_components(){
        let source=include_str!("main.rs");
        let dialog=source.split("fn show_vat_settings(").nth(1).unwrap()
            .split("fn show_edit_history(").next().unwrap();
        assert!(dialog.contains("components::checkbox("));
        assert!(dialog.contains("components::checkbox_list_row("));
        assert!(dialog.contains("theme::modal_nested_frame()"));
        assert!(!dialog.contains("egui::Checkbox::new("));
        assert!(!dialog.contains("egui::Frame::new()"));

        let client=include_str!("client_manager.rs");
        let boolean=client.split("fn bool_edit(").nth(1).unwrap()
            .split("fn new_id(").next().unwrap();
        assert!(boolean.contains("components::checkbox("));
        assert!(!boolean.contains("ui.checkbox("));

        let main=include_str!("main.rs");
        let message=main.split("fn message_section(").nth(1).unwrap()
            .split("fn ").next().unwrap();
        assert!(message.contains("components::checkbox("));
        assert!(!message.contains("Checkbox::new("));
        let workflow=main.split("fn show_workflow_dialog(").nth(1).unwrap()
            .split("\n    fn ").next().unwrap();
        assert!(workflow.contains("components::checkbox("));
        assert!(!workflow.contains("ui.checkbox("));

        // OCR learning, category transfer consent and product filtering are
        // standard checkboxes too; none may construct a local egui checkbox.
        assert!(main.contains("changed=components::checkbox("));
        assert!(main.contains("&mut review.learned_analysis_enabled"));
        assert!(!main.contains("option.checkbox("));

        let category=include_str!("category_manager.rs");
        assert!(category.contains("components::checkbox("));
        assert!(!category.contains("notice.checkbox("));

        let products=include_str!("product_manager.rs");
        assert!(products.contains("components::checkbox("));
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
        assert!(back.contains("theme::action_with_min_width("));
        assert!(!back.contains("egui::Button::new("));

        let explorer=source.split("if state.ancestors_open && state.path.len()>3")
            .nth(1).unwrap().split("components::soft_divider(menu);")
            .next().unwrap();
        assert!(explorer.contains("theme::category_ancestor_frame()"));
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
        assert!(editor.contains("components::removable_tag_chip("));
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

pub fn table_header(ui: &mut Ui, columns: &[(&str, f32)]) {
    Frame::new().fill(theme::RAISED).inner_margin(egui::Margin::symmetric(theme::SPACE_8 as i8, theme::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (title, width) in columns {
                    ui.add_sized(
                        Vec2::new(*width, TABLE_HEADER_HEIGHT - 8.0),
                        egui::Label::new(
                            typography::table_header(title),
                        ),
                    );
                }
            });
        });
}

pub fn table_row(ui: &mut Ui, columns: &[(&str, f32)]) {
    ui.horizontal(|ui| {
        for (value, width) in columns {
            ui.add_sized(
                Vec2::new(*width, TABLE_ROW_HEIGHT),
                egui::Label::new(typography::table_cell(value)),
            );
        }
    });
    soft_divider(ui);
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
            let count=source.matches("components::management_search_field(").count();
            assert_eq!(count,expected,"{screen} query did not use the shared field");
        }
        // Search-tag entry is an editor, not a query field.
        let clients=include_str!("client_manager.rs");
        assert!(clients.contains("entry,&mut self.tag_input,width"));
        assert!(clients.contains("components::flexible_text_field("));
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
                let text=management_master_header_begin(ui,header,content);
                management_master_header_finish(ui,header);
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
        let gutter=theme::management_table_scroll_style().allocated_width();
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
            let width=reception_item_table_width(viewport);
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
        assert_eq!(reception_item_table_width(220.0),
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
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION,7.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET,0.0);
        assert_eq!(rail.top(),bounds.top()); // no gap under the header
        assert_eq!(rail.bottom(),rows_bottom+theme::SPACE_4);
        assert_eq!(rail.bottom(),788.0);

        // Neither a list's footer nor a master card's 16pt lower inset
        // changes the shared rail offset. The scroll viewport is unchanged.
        for height in [120.0,400.0,484.0,1200.0] {
            let track=management_table_scroll_track_geometry(bounds,height);
            let viewport_bottom=(bounds.top()+height).min(bounds.bottom());
            assert_eq!(track.bottom(),viewport_bottom+theme::SPACE_4);
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
    fn toolbar_to_header_transition_remains_21pt_with_preexisting_inset() {
        let top=theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP;
        let rule=theme::SOFT_DIVIDER_WIDTH;
        let bottom=theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP;
        assert_eq!(top+rule+bottom,21.0);
        let existing_toolbar_inset=theme::SPACE_8;
        let remaining=management_remaining_predivider_gap(existing_toolbar_inset);
        assert_eq!(remaining,8.0);
        assert_eq!(existing_toolbar_inset+remaining+rule+bottom,21.0);
        assert_eq!(management_remaining_predivider_gap(0.0),16.0);
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
            let pivot=modal_pivot_pos(safe);
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
        let source=include_str!("components.rs");
        let block=source.split("pub fn management_searchable_dropdown(").nth(1).unwrap()
            .split("/// Paint only the option glyphs").next().unwrap();
        for required in [
            "management_dropdown_trigger(",
            "management_dropdown_option_palette(menu)",
            "management_search_field(menu,&mut search,input_width,hint)",
            "management_dropdown_option(",
            "dropdown_popup_trigger(menu.ctx(),rect)",
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
            products.matches("components::management_searchable_dropdown(").count(),2
        );
        assert!(include_str!("category_manager.rs")
            .contains("category_dropdown::category_dropdown("));
        for (name,source,minimum) in screens {
            assert!(!source.contains("ComboBox::"),
                "{name} still contains a screen-local native ComboBox");
            assert!(source.matches("management_dropdown(").count()>=minimum,
                "{name} lost a shared single-select call");
        }
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
                        ui,label,width,enabled,"",|choices|{
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
    fn normal_and_compact_dropdowns_share_the_same_selection_notice_bridge(){
        // The common trigger registers the position; both option variants
        // notify the same bridge. No individual screen should capture a
        // popup item's pointer for a result bubble.
        let source=include_str!("components.rs");
        let first=source.find("pub fn management_dropdown_option<T:PartialEq>").unwrap();
        let second=source.find("pub fn management_dropdown_compact_option<T:PartialEq>").unwrap();
        let end=source[second..].find("pub fn management_tabs(").unwrap()+second;
        assert!(source[first..second].contains("dropdown_option_selected(ui.ctx())"));
        assert!(source[second..end].contains("dropdown_option_selected(ui.ctx())"));
        let popup=source.find("fn management_dropdown_inner(").unwrap();
        assert!(source[popup..first].contains("dropdown_popup_trigger(menu.ctx(),rect)"));
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
        let ctx=crate::components::test_context();
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
                    RECEPTION_MONEY_UNIT_GAP,egui::Margin::symmetric(8,2)
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
                    reception_total_footer(ui,total,Some(1),"부가세 별도","");
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
        assert_eq!(theme::MODAL_SECTION_HEADER_RULE_GAP,16.0);
        assert_eq!(theme::MODAL_SECTION_PADDING,16.0);
        assert_eq!(theme::SOFT_DIVIDER_WIDTH,1.0);
        let source=include_str!("components.rs");
        let helper=source.split("fn modal_section_heading_with_rule(").nth(1).unwrap()
            .split("/// A heading and one divider").next().unwrap();
        assert!(helper.contains("ui.spacing_mut().item_spacing.y=0.0;"));
        assert_eq!(helper.matches("ui.add_space(theme::MODAL_SECTION_HEADER_RULE_GAP)").count(),2);
        assert!(helper.contains("soft_divider(ui);"));
        let boxed=source.split("pub fn modal_section<R>(").nth(1).unwrap()
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
                                seen.divider=Some(modal_master_spaced_divider(body));
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
    fn actual_modal_rects_keep_every_16pt_workspace_boundary(){
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
            "actual egui rects violate 16pt master: {gaps:?}");
        let cards=&geometry.content.cards;
        for window in cards.windows(2){
            assert!((window[0].top()-window[1].top()).abs()<=1.0,
                "card top boundaries differ");
            assert!((window[0].bottom()-window[1].bottom()).abs()<=1.0,
                "card bottom boundaries differ");
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
    fn shared_header_toolbar_keeps_the_rule_and_body_clearance_at_the_16pt_master(){
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
        )).abs()<=0.5,"unexpected two-row header height: {header:?}");
        assert!((toolbar.height()-theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT)
            .abs()<=0.5,"toolbar does not fill its reserved header row: {toolbar:?}");
        assert!((header.bottom()-toolbar.bottom()-theme::SOFT_DIVIDER_WIDTH)
            .abs()<=0.5,"header rule must immediately follow the toolbar: {header:?} {toolbar:?}");
        assert!((body.top()-header.bottom()-theme::MODAL_MASTER_WORKSPACE_GAP)
            .abs()<=0.5,"body must begin 16pt below the shared header: {header:?} {body:?}");
        assert!((footer.top()-body.bottom()-theme::MODAL_MASTER_WORKSPACE_GAP)
            .abs()<=0.5,"footer must remain 16pt below the body: {body:?} {footer:?}");
        assert!(geometry.invalid_gaps(0.5).is_empty(),
            "shared header/body/footer spacing drifted: {:?}",geometry.gaps());
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
                modal_window("수동 분류","manual-parent-workspace",work_area)
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
                            let response=theme::action(
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
                modal_window("수동 분류","manual-parent-workspace",work_area)
                    .show(ctx,|ui|{
                        ui.disable();
                        let response=ui.button("parent action");
                        parent_rect=response.rect;
                        if response.clicked(){parent_clicked=true;}
                    });
                decision_window(
                    "수동 분류 변경 취소","manual-discard-prompt",work_area
                ).show(ctx,|ui|{
                    ui.label("discard?");
                    modal_action_row(ui,|actions|{
                        theme::action(actions,"변경 취소",theme::ButtonRole::Standard,true);
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
                modal_window("수동 분류","manual-regression-parent",work_area)
                    .default_width(900.0)
                    .default_height(650.0)
                    .show(ctx,|parent|{
                        parent.disable();
                        let response=parent.button("background worksheet action");
                        parent_rect=response.rect;
                        if response.clicked(){*parent_clicked=true;}
                    });
                decision_window("수동 분류 나가기","manual-regression-child",work_area)
                    .show(ctx,|decision|{
                        decision.label("초안에 반영하지 않은 변경사항이 있습니다.");
                        decision.horizontal(|row|{
                            let keep=theme::action(
                                row,"계속 편집",theme::ButtonRole::Standard,true
                            );
                            keep_rect=keep.rect;
                            if keep.clicked(){*keep_clicked=true;}
                            let discard=theme::action(
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
            .split("fn date_navigation_button_surface(").next().unwrap();
        assert!(count.contains("management_date_text_field_impl("));
        assert!(!count.contains("numeric_editor_font("));
        assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
    }

    #[test]
    fn count_stepper_palette_distinguishes_actions_from_editor_and_disabled_end_stops(){
        assert_eq!(count_step_palette(true),(theme::RAISED,theme::TEXT));
        assert_eq!(count_step_palette(false),(theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_TEXT));
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
            assert!(source.contains("components::modal_flow_workspace("),
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
            assert!(source.contains("components::modal_list_max_height("),"{name}");
        }
        let freight=include_str!("freight_manager.rs");
        assert!(freight.contains("decision.framed_body("));
        assert!(!freight.contains("components::modal_master_body("));
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
                    modal_window("좁은 작업 영역","phase4-compact-editor",safe)
                        .default_width(380.0).show(ctx,|ui|{
                            let (_,geometry)=modal_flow_workspace_measured(ui,|workspace|{
                                workspace.header("아주 긴 제목을 갖는 상품 상세 편집 화면","취소",true);
                                workspace.scroll_region("phase4-scroll",modal_editor_body_max_height(height),|body|{
                                    for _ in 0..80 {body.label("스크롤 검증 항목");}
                                });
                                workspace.footer_single(|actions|{
                                    theme::action(actions,"저장",theme::ButtonRole::Primary,true);
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
                    "components::decision_window(",
                    "Window::new(",
                    "Modal::new(",
                    "components::modal_header(",
                    "components::modal_header_chrome(",
                    "components::modal_footer_chrome(",
                    "components::modal_action_row(",
                    "components::modal_action_row_split(",
                    ".movable(true)",
                    ".resizable(true)",
                ] {
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
        assert_eq!(theme::MODAL_MASTER_WORKSPACE_GAP,16.0);
        assert_eq!(theme::MODAL_MASTER_CONTENT_INSET,16.0);
        assert_eq!(theme::MODAL_SECTION_PADDING,16.0);
        assert_eq!(theme::MODAL_SECTION_HEADER_RULE_GAP,16.0);
        assert_eq!(theme::MODAL_PANEL_PADDING_Y,16.0);
        assert_eq!(theme::MODAL_PANEL_PADDING_X,18.0);
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
        assert_eq!(theme::SECTION_PADDING,16.0);
        assert_eq!(theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING,16.0);
        assert_eq!(theme::MANAGEMENT_TABLE_HEADER_HEIGHT,36.0);
        assert_eq!(theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT,2.0);
        assert_eq!(theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP,16.0);
        assert_eq!(theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP,4.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION,7.0);
        let standard=theme::management_scroll_style();
        let table=theme::management_table_scroll_style();
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
        let components=include_str!("components.rs");
        assert_eq!(source.matches("components::reception_source_text_edit(").count(),3);
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
    use std::{cell::RefCell,sync::Arc};
    use crate::gateway::{BandoGateway,ReadOperation};
    use crate::preview_store::WorkingSource;
    use serde_json::Value;
    type Sample=(&'static str,Rect);
    thread_local!{
        static TRACE:RefCell<Option<Vec<Sample>>>=RefCell::new(None);
    }
    pub(crate) fn record(name:&'static str,rect:Rect){
        TRACE.with(|trace|{if let Some(samples)=trace.borrow_mut().as_mut(){samples.push((name,rect));}});
    }
    // No files, network or database are accessed by fixture read workers.
    struct GeometryGateway;
    impl BandoGateway for GeometryGateway{
        fn label(&self)->&'static str{"synthetic UI geometry"}
        fn load_catalog(&self)->Result<Value,String>{Err("isolated fixture".into())}
        fn load_working_source(&self)->Result<WorkingSource,String>{Err("isolated fixture".into())}
        fn save_working_source(&self,_:&WorkingSource)->Result<u64,String>{Err("read only fixture".into())}
        fn execute(&self,_:ReadOperation,_:Value)->Result<Value,String>{Err("isolated fixture".into())}
    }
    fn render(ctx:&egui::Context,screen:Vec2,work:Rect,mut page:impl FnMut(&mut Ui))->Vec<Sample>{
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
            assert!([rect.left(),rect.top(),rect.right(),rect.bottom()].iter().all(|v|v.is_finite())
                && rect.is_positive(),"{name}/{tab}/{kind}: invalid {rect:?}");
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
            for (a,b) in [(transition.bottom(),header.top()),(header.bottom(),rail.top()),
                (header.bottom(),rows.top()),(card.left()+theme::SECTION_PADDING+theme::FRAME_STROKE_WIDTH,header.left()),
                (card.right()-theme::SECTION_PADDING-theme::FRAME_STROKE_WIDTH,header.right())]{
                assert!((a-b).abs()<=0.5,"{name}: master boundary drift {a} -> {b}");
            }
            assert!((transition.height()-(theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP
                +theme::SOFT_DIVIDER_WIDTH+theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP)).abs()<=0.5);
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
        for ((kind,old),(_,new)) in a.iter().zip(&b){
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
    fn exercise_data_transitions<P>(
        name:&str,ctx:&egui::Context,screen:Vec2,work:Rect,page:&mut P,
        mut show:impl FnMut(&mut P,&mut Ui),
        begin:impl Fn(&mut P)->Box<dyn FnOnce(Result<usize,String>)>,
        failed:impl Fn(&P)->bool,
    )->Vec<Sample>{
        let mut draw=|page:&mut P|render(ctx,screen,work,|ui|show(page,ui));
        // Keep the sender alive across real render frames: this is an
        // actually pending receiver, not just a false loaded boolean.
        let finish=begin(page);
        let pending=draw(page);
        assert_visible_data_rows(name,&pending,0);
        finish(Err("합성 조회 실패: 길이가 긴 오류 메시지의 UI 밀림 검증".into()));
        let initial_error=draw(page);
        assert!(failed(page),"{name}: initial failure was not consumed");
        let finish=begin(page);
        let _retry_pending=draw(page);
        finish(Ok(0));
        let empty=draw(page);
        assert!(!failed(page),"{name}: successful empty response did not clear failure");
        assert_visible_data_rows(name,&empty,0);
        for (phase,trace) in [("initial pending",&pending),("initial error",&initial_error)]{
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
        finish(Err("합성 재조회 실패: 데이터 로딩 이후 오류".into()));
        let error=draw(page);
        assert!(failed(page),"{name}: refresh failure was not consumed");
        compare_data_state_geometry(name,"refresh error",&empty,&error,true);
        let finish=begin(page);
        finish(Ok(64));
        let recovered=draw(page);
        assert!(!failed(page),"{name}: retry did not clear failure");
        assert_visible_data_rows(name,&recovered,64);
        compare_data_state_geometry(name,"recovered",&empty,&recovered,true);
        println!("GEOMETRY_DATA_TRANSITIONS {name} {screen:?}: pending/error/empty/one/many/recovered");
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
        let mut draw=|p:&mut P|render(ctx,screen,work,|ui|show(p,ui));
        let finish=begin(page);
        let pending=draw(page);
        finish(Err("합성 최초 조회 오류 — 긴 문구에도 제목과 탭이 이동하지 않아야 함".into()));
        let error=draw(page);
        assert!(failed(page),"{name}: initial error not consumed");
        begin(page)(Ok(0));
        let empty=draw(page);
        assert!(!failed(page),"{name}: empty recovery failed");
        for (phase,trace) in [("pending",&pending),("error",&error)]{
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
        begin(page)(Ok(64));
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
    fn reception_section_divider_has_equal_8pt_clearance_on_both_sides(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let trace=render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"발주 원문",320.0,|_|{},|body|{
                body.allocate_exact_size(
                    Vec2::new(body.available_width(),80.0),Sense::hover()
                );
            });
        });
        let header=first(&trace,"source header");
        let divider=first(&trace,"source divider");
        let body=first(&trace,"source body");
        let top_gap=divider.top()-header.bottom();
        let bottom_gap=body.top()-divider.bottom();
        assert!((top_gap-theme::SECTION_CONTENT_GAP).abs()<=0.5,
            "header-to-divider clearance drifted: {top_gap:.2}pt");
        assert!((bottom_gap-theme::SECTION_CONTENT_GAP).abs()<=0.5,
            "divider-to-body clearance drifted: {bottom_gap:.2}pt");
        assert!((top_gap-bottom_gap).abs()<=0.5,
            "divider clearance must be symmetrical: top={top_gap:.2}pt bottom={bottom_gap:.2}pt");
        let expected_chrome=2.0*theme::RECEPTION_PANE_PADDING_Y
            +theme::SECTION_HEADER_HEIGHT
            +2.0*theme::SECTION_CONTENT_GAP
            +theme::SOFT_DIVIDER_WIDTH;
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

