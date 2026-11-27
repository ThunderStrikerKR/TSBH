#![cfg(test)]
//! Cross-module UI regression contracts.

use eframe::egui::{self,Align,Layout,Rect,Sense,Ui,Vec2};

use crate::{theme,typography};
use crate::ui_primitives::*;
use crate::status_notice::{management_body_state_overlay,body_error_overlay};

use crate::composite_reception::{reception_item_row,reception_freight_row_controls,reception_item_origin_action_editable};

use crate::ui_layout::{fixed_row,fixed_result_viewport,fixed_status_slot,management_page_title_top_gap,management_top_content,reception_goods_viewport,reception_docked_goods_scroll};


use crate::ui_test_support::{test_context,production_source_for_contract,assert_layout_rects_invariant};
use crate::checkbox::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.
use crate::button::{management_tabs,reception_item_header_action};






use crate::dropdown::{management_dropdown,management_status_dropdown,management_dropdown_compact,management_dropdown_option,management_status_dropdown_option,management_dropdown_compact_option};


// Shared table primitives used by reception and other composite widgets.
use crate::table::{reception_items_header};

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
            include_str!("product_manager.rs"),include_str!("model_group_manager.rs")]{
            assert!(source.contains("crate::status_notice::management_body_state_overlay("));
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
        assert!(rendered.contains("crate::status_notice::management_body_state_overlay("));
        assert!(!rendered.contains("let empty_reserve=if self.products.is_empty()"));
        assert!(!rendered.contains("management_list_strip_frame().show(panel,|empty|"));
        let settlement=include_str!("daily_settlement_manager.rs");
        assert!(settlement.contains("let values=["));
        assert!(settlement.contains("for (name,value) in ["));
        let model_groups=include_str!("model_group_manager.rs");
        assert!(!model_groups.contains("if self.dirty(){\n                                crate::ui_primitives::soft_divider(right);"));
        let aliases=include_str!("alias_manager.rs");
        assert!(aliases.contains("fn alias_diagnostic("));
        assert!(aliases.contains("let diagnostic=alias_diagnostic(item,&warnings);"));
        assert!(aliases.contains("crate::table::registry_table("));
        assert!(!aliases.contains("hit.on_hover_text(detail);"));
        let settings=include_str!("settings_view.rs");
        assert!(settings.contains("if conflict{"));
        assert!(settings.contains("heading.with_layout(Layout::right_to_left(Align::Center)"));
        assert!(!settings.contains("if conflict && crate::ui_layout::management_top_content"));
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
        assert!(products.contains("crate::status_notice::management_body_state_overlay("));
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
        assert!(model_groups.contains("workspace.header(title,\"닫기\""));

        let aliases=include_str!("alias_manager.rs");
        assert!(aliases.contains("fn alias_diagnostic("));
        assert!(aliases.contains("let diagnostic=alias_diagnostic(item,&warnings);"));
        assert!(aliases.contains("crate::table::registry_table("));
        assert!(aliases.contains("Role::Status,false,(id,3)"));

        let main=include_str!("main.rs");
        assert!(main.contains("crate::composite_client::reception_client_summary_card_editable("));
        assert!(main.contains("let client_card_height = if has_source"));
        assert!(main.contains("fn apply_manual_event("));
        assert!(main.contains("row.edit_history.push("));
        assert!(main.contains("row.manual_model=Some("));
    }
}

#[cfg(test)]
mod shared_screen_component_contract_tests {
    use crate::ui_test_support::production_source_for_contract as production_source;

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
                let mut count=0usize;
                let mut pointer=egui::pos2(20.0,20.0);
                // Real pointer and elapsed hover time exercise the overlay
                // ownership guard, unlike forcing every popup visible.
                for frame in 0..16{
                    let output=ctx.run(egui::RawInput{
                        screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(800.0,500.0))),
                        time:Some(frame as f64*0.1),predicted_dt:0.1,
                        events:vec![egui::Event::PointerMoved(pointer)],
                        ..Default::default()
                    },|ctx|{
                        egui::CentralPanel::default().show(ctx,|ui|{
                            let label=if explicit{
                                truncated_label_for_explicit_tooltip(typography::body(full))
                            }else{truncated_label(typography::body(full))};
                            let response=ui.add_sized(Vec2::new(width,32.0),label);
                            pointer=response.rect.center();
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
                    ui,"운임",Some(3000),Some(false),Some(false),true,editable,editable,editable,true,false
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
mod shared_tag_chip_contract_tests {
    #[test]
    fn client_search_tags_use_shared_chip_and_icon_action(){
        let source=include_str!("client_manager.rs");
        let editor=source.split("fn search_tag_editor(").nth(1).unwrap()
            .split("fn editor_fields(").next().unwrap();
        assert!(editor.contains("crate::ui_layout::tag_block_editor("));
        assert!(!editor.contains("take_delimited("));
        let brand=include_str!("brand_manager.rs");
        assert!(brand.contains("crate::ui_layout::tag_block_editor("));
        assert!(!brand.contains("쉼표 구분"));
        let layout=include_str!("ui_layout.rs");
        let shared=layout.split("fn tag_block_editor(").nth(1).unwrap()
            .split("\n}").next().unwrap();
        assert!(shared.contains("crate::popup::removable_tag_chip("));
        assert!(shared.contains("crate::button::action(entry,\"추가\""));
        assert!(!shared.contains("split("));
        assert!(!editor.contains("egui::Frame::new()"));
        assert!(!editor.contains("egui::Button::new("));
    }
}

#[cfg(test)]
mod shared_general_screen_audit_tests {
    use super::*;

    // Ownership is per primitive, not a blanket exemption for all
    // component files. Specialised renderers may compose shared controls.
    const PRIMITIVES:&[(&str,&[&str])]=&[
        ("Button::new(",&["button/mod.rs","dropdown/mod.rs"]),
        ("Frame::new(",&["theme.rs","card/mod.rs"]),
        ("TextEdit::singleline(",&["input/mod.rs"]),
        ("TextEdit::multiline(",&["input/mod.rs"]),
        ("Checkbox::new(",&["checkbox/mod.rs"]),
        ("ComboBox::",&["dropdown/mod.rs"]),
        (".button(",&[]),(".checkbox(",&[]),
        (".text_edit_singleline(",&[]),(".text_edit_multiline(",&[]),
        (".collapsing(",&[]),("CollapsingHeader::new(",&["button/mod.rs"]),
        ("Window::new(",&["overlay.rs"]),
        ("ScrollArea::vertical(",&["scrollbar/mod.rs"]),
        ("ScrollArea::horizontal(",&["scrollbar/mod.rs"]),
        ("ScrollArea::both(",&["scrollbar/mod.rs"]),
        ("ScrollStyle::solid(",&["scrollbar/mod.rs"]),
        ("ScrollStyle::floating(",&["scrollbar/mod.rs"]),
        ("Slider::new(",&["input/mod.rs"]),("Grid::new(",&["navigation.rs"]),
    ];
    fn primitive_violations(name:&str,source:&str)->Vec<String>{
        let mut violations=Vec::new();
        for (index,line) in source.lines().enumerate(){
            let code=line.trim_start();
            if code.starts_with("//")||code.starts_with("/*")||code.starts_with('*'){continue;}
            let code=code.split("//").next().unwrap_or("");
            let compact=code.split_whitespace().collect::<String>();
            for &(token,owners) in PRIMITIVES{
                if owners.contains(&name){continue;}
                if let Some(pos)=compact.find(token){
                    if !compact[..pos].contains('"'){
                        violations.push(format!("{name}:{}: {token}",index+1));
                    }
                }
            }
        }
        violations
    }
    fn scan(root:&std::path::Path,path:&std::path::Path,violations:&mut Vec<String>){
        for entry in std::fs::read_dir(path).expect("read egui source directory"){
            let p=entry.expect("source entry").path();
            if p.is_dir(){scan(root,&p,violations);continue;}
            if p.extension().and_then(|v|v.to_str())!=Some("rs"){continue;}
            let name=p.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/");
            if name.ends_with("regression_tests.rs")||name=="ui_regressions.rs"||name=="ui_test_support.rs"{continue;}
            let source=std::fs::read_to_string(&p).expect("read source");
            if source.starts_with("#![cfg(test)]"){continue;}
            let source=crate::ui_test_support::production_source_for_contract(&source);
            violations.extend(primitive_violations(&name,&source));
        }
    }
    #[test]
    fn primitive_ownership_rejects_bypasses_but_allows_implementations(){
        assert!(primitive_violations("button/mod.rs","egui::Button::new(label);").is_empty());
        assert_eq!(primitive_violations("button/mod.rs","egui::TextEdit::singleline(value);").len(),1);
        assert_eq!(primitive_violations("review_panel.rs","ui.collapsing(label, |ui| {});").len(),1);
        assert_eq!(primitive_violations("settings_view.rs","egui::ScrollArea::vertical();").len(),1);
        assert!(primitive_violations("review_panel.rs","crate::button::collapsing_header(label);").is_empty());
    }
    #[test]
    fn new_and_existing_screen_modules_use_shared_controls_and_surfaces(){
        let mut violations=Vec::new();
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        scan(&root,&root,&mut violations);
        assert!(violations.is_empty(),"general screen UI bypass:\n{}",violations.join("\n"));
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
        assert_eq!(theme::RECEPTION_PANE_PADDING_Y,8.0);
        assert_eq!(theme::SECTION_PADDING,12.0);
        assert_eq!(theme::SOURCE_DOCUMENT_TEXT_MARGIN,4);
        assert!(theme::SOURCE_DOCUMENT_TEXT_MARGIN as f32 >=
            theme::SOURCE_BLOCK_HORIZONTAL_PADDING+theme::LINKED_PREVIEW_BORDER_WIDTH);
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
        crate::ui_test_support::enable_unaligned_diagnostics(ctx);
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
            crate::ui_test_support::begin_component_geometry();
            let output=ctx.run(egui::RawInput{
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
            let components=crate::ui_test_support::finish_component_geometry(Rect::from_min_size(egui::Pos2::ZERO,screen));
            crate::ui_test_support::assert_no_unaligned(&output,&format!("{} {screen:?} frame {frame}",std::thread::current().name().unwrap_or("management page")));
            if frame==11&&next.iter().any(|(role,_)|*role=="top content"){
                assert!(!components.is_empty(),"actual management page did not reach a shared component renderer");
                if next.iter().any(|(role,_)|*role=="table perimeter"){
                    crate::ui_test_support::assert_component_roles(&components,&["table"]);
                }
            }
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
    /// The production page entrypoints are shared with the existing geometry
    /// fixtures; sweep the actual hit rectangles without clicking or changing data.
    fn audit_highlight_page(name:&str,tabs:usize){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for tab in 0..tabs{
                let ctx=test_context();
                let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
                let label=format!("{name}/{tab} {screen:?}");
                let audit=|page:&mut dyn FnMut(&mut Ui)|{
                    crate::ui_test_support::sweep_highlight_geometry(&ctx,Rect::from_min_size(egui::Pos2::ZERO,screen),&label,|ctx|{
                        egui::CentralPanel::default().show(ctx,|root|{
                            let mut host=root.new_child(egui::UiBuilder::new()
                                .id_salt("actual-highlight-page").max_rect(work).layout(Layout::top_down(Align::Min)));
                            host.set_clip_rect(work);host.set_width(work.width());
                            page(&mut host);
                            record("page content",host.min_rect());
                        });
                    });
                };
                match name{
                    "freight"=>{let mut p=crate::freight_manager::FreightManager::geometry_fixture(gateway,ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui));},
                    "history"=>{let mut p=crate::order_history_manager::OrderHistoryManager::geometry_fixture(gateway,ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui));},
                    "products"=>{let mut p=crate::product_manager::ProductManager::geometry_fixture(gateway,ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui));},
                    "clients"=>{let mut p=crate::client_manager::ClientManager::geometry_fixture(Arc::clone(&gateway),ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui,Arc::clone(&gateway)));},
                    "excel"=>{let mut p=crate::excel_manager::ExcelManager::geometry_fixture(gateway,ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui));},
                    "settings"=>{let mut p=crate::settings_view::SettingsView::geometry_fixture(Arc::clone(&gateway),ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui,Arc::clone(&gateway)));},
                    "settlement"=>{let mut p=crate::daily_settlement_manager::DailySettlementManager::geometry_fixture(gateway,ctx.clone(),tab,true);audit(&mut |ui|p.show_page(ui));},
                    _=>panic!("unregistered highlight screen {name}"),
                }
            }
        }
    }
    #[test] fn highlight_invariance_freight(){audit_highlight_page("freight",4);}
    #[test] fn highlight_invariance_history(){audit_highlight_page("history",1);}
    #[test] fn highlight_invariance_products(){audit_highlight_page("products",5);}
    #[test] fn highlight_invariance_clients(){audit_highlight_page("clients",3);}
    #[test] fn highlight_invariance_excel(){audit_highlight_page("excel",3);}
    #[test] fn highlight_invariance_settings(){audit_highlight_page("settings",4);}
    #[test] fn highlight_invariance_settlement(){audit_highlight_page("settlement",1);}
    #[test]
    fn highlight_invariance_reception(){
        for (screen,_) in data_geometry_dimensions(){
            let ctx=test_context();
            let mut app=crate::BandoEgui::with_layout(crate::layout::LayoutState::default());
            app.gateway=Arc::new(GeometryGateway);
            app.raw_source="합성 발주원문\n검증 수취인 및 상품".into();
            app.analysis=Some(crate::reception_service::ReceptionAnalysis{
                recognition:serde_json::json!({"recipients":[
                    {"id":"geometry-r1","name":"검증 수취인","address":"합성 주소","mobile":"010-0000-0000","phone":"","note":"","freightCode":"010",
                    "items":[{"id":"geometry-i1","name":"검증 상품","quantity":1}]}
                ]}),
                reference:vec![],client_match:Value::Null,client_candidates:vec![],
                matched_client_name:None,chosen_client_name:None,directory_count:0,linked_spans:vec![]
            });
            crate::ui_test_support::sweep_highlight_geometry(&ctx,Rect::from_min_size(egui::Pos2::ZERO,screen),
                &format!("reception {screen:?}"),|ctx|app.reception(ctx));
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
    fn actual_product_chooser_uses_modal_body_not_management_page_anchor(){
        for (width,height) in [(1120.0,760.0),(840.0,500.0)]{
            let ctx=test_context();
            let gateway:Arc<dyn BandoGateway>=Arc::new(GeometryGateway);
            let mut picker=crate::product_manager::ProductManager::geometry_fixture(
                gateway,ctx.clone(),0,true);
            let work=Rect::from_min_size(egui::pos2(120.0,60.0),Vec2::new(width,height));
            let screen=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width+160.0,height+100.0));
            crate::ui_test_support::enable_unaligned_diagnostics(&ctx);
            let mut measured=Vec::new();
            for frame in 0..12{
                TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
                let output=ctx.run(egui::RawInput{
                    screen_rect:Some(screen),time:Some(frame as f64*0.05),
                    predicted_dt:0.05,..Default::default()
                },|ctx|{
                    let _=picker.show_selection_modal(ctx,work,"geometry-product-chooser");
                });
                measured=TRACE.with(|trace|trace.borrow_mut().take().unwrap());
                crate::ui_test_support::assert_no_unaligned(&output,&format!("product chooser {width}x{height} frame {frame}"));
            }
            assert!(!measured.iter().any(|(kind,_)|*kind=="top content"),
                "modal must not render the management title or KPI strip");
            let body=first(&measured,"product picker body");
            let card=first(&measured,"master card");
            let rows=first(&measured,"row viewport");
            assert!((body.top()-card.top()).abs()<=1.0,
                "management page anchor leaked into popup: {body:?} {card:?}");
            assert!(card.bottom()<=body.bottom()+2.0,
                "popup list card exceeded its body budget: {card:?} {body:?}");
            assert!(rows.is_positive() && rows.bottom()<=card.bottom()+1.0,
                "popup rows exceeded the card: {rows:?} {card:?}");
        }
    }

    fn audit_unaligned_management(name:&str,tabs:usize){
        for (screen,left) in data_geometry_dimensions(){
            let work=Rect::from_min_max(egui::pos2(left,80.0),egui::pos2(screen.x-18.0,screen.y-18.0));
            for tab in 0..tabs{
                for loaded in [false,true]{
                    let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||screen_trace(name,tab,loaded,screen,work)));
                    if let Err(error)=result{
                        eprintln!("UNALIGNED_AUDIT_CASE {name}/{tab} loaded={loaded} screen={screen:?}");
                        std::panic::resume_unwind(error);
                    }
                }
            }
        }
    }
    #[test] fn unaligned_freight(){audit_unaligned_management("freight",4);}
    #[test] fn unaligned_history(){audit_unaligned_management("history",1);}
    #[test] fn unaligned_products(){audit_unaligned_management("products",5);}
    #[test] fn unaligned_clients(){audit_unaligned_management("clients",3);}
    #[test] fn unaligned_excel(){audit_unaligned_management("excel",3);}
    #[test] fn unaligned_settings(){audit_unaligned_management("settings",4);}
    #[test] fn unaligned_settlement(){audit_unaligned_management("settlement",1);}
    #[test]
    fn unaligned_reception_initial_and_populated_frames(){
        for (screen,_) in data_geometry_dimensions(){
            for scale in [1.0_f32,1.25,1.5,1.75,2.0]{
                for populated in [false,true]{
                    let ctx=test_context();
                    ctx.set_pixels_per_point(scale);
                    crate::ui_test_support::enable_unaligned_diagnostics(&ctx);
                    let mut app=crate::BandoEgui::with_layout(crate::layout::LayoutState::default());
                    app.gateway=Arc::new(GeometryGateway);
                    if populated{
                        app.raw_source="합성 발주원문\n검증 수취인 및 상품".into();
                        app.analysis=Some(crate::reception_service::ReceptionAnalysis{
                            recognition:serde_json::json!({"recipients":[
                                {"id":"geometry-r1","name":"검증 수취인","address":"합성 주소","mobile":"010-0000-0000","phone":"","note":"","freightCode":"010",
                                "items":[{"id":"geometry-i1","name":"검증 상품","quantity":1}]}
                            ]}),
                            reference:vec![],client_match:Value::Null,client_candidates:vec![],
                            matched_client_name:None,chosen_client_name:None,directory_count:0,linked_spans:vec![]
                        });
                    }
                    let rect=Rect::from_min_size(egui::Pos2::ZERO,screen);
                    for frame in 0..12{
                        let output=ctx.run(egui::RawInput{
                            screen_rect:Some(rect),time:Some(frame as f64*0.05),
                            predicted_dt:0.05,..Default::default()
                        },|ctx|app.reception(ctx));
                        crate::ui_test_support::assert_no_unaligned(&output,&format!(
                            "reception {screen:?} scale={scale} populated={populated} frame={frame}"));
                    }
                }
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
                    crate::ui_test_support::begin_component_geometry();
                    let _=ctx.run(egui::RawInput{
                        screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,screen)),
                        ..Default::default()
                    },|ctx|app.reception(ctx));
                    let components=crate::ui_test_support::finish_component_geometry(Rect::from_min_size(egui::Pos2::ZERO,screen));
                    crate::ui_test_support::assert_component_roles(&components,&["card"]);
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
        assert_visible_data_rows(name,&populated_pending,64);
        compare_data_state_geometry(name,"populated refresh pending",&populated,&populated_pending,true);
        finish(Err("합성 재조회 실패: 데이터 로딩 이후 오류".into()));
        let error=draw(page);
        assert!(failed(page),"{name}: refresh failure was not consumed");
        assert_visible_data_rows(name,&error,64);
        compare_data_state_geometry(name,"refresh error",&populated,&error,true);
        let finish=begin(page);
        let retry=draw(page);
        assert!(!failed(page),"{name}: populated retry must clear failure");
        assert_visible_data_rows(name,&retry,64);
        compare_data_state_geometry(name,"populated retry pending",&populated,&retry,true);
        // The shared refresh fixture models an interrupted worker by
        // dropping its completion, consistently across management tables.
        drop(finish);
        let disconnected=draw(page);
        assert!(failed(page),"{name}: interrupted read was not consumed");
        assert_visible_data_rows(name,&disconnected,64);
        compare_data_state_geometry(name,"disconnected read",&populated,&disconnected,true);
        let finish=begin(page);
        let reconnecting=draw(page);
        assert!(!failed(page),"{name}: reconnect must clear the old error");
        assert_visible_data_rows(name,&reconnecting,64);
        compare_data_state_geometry(name,"reconnect pending",&populated,&reconnecting,true);
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
                    crate::ui_test_support::begin_component_geometry();
                    let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,screen)),
                        ..Default::default()},|ctx|app.reception(ctx));
                    let components=crate::ui_test_support::finish_component_geometry(Rect::from_min_size(egui::Pos2::ZERO,screen));
                    crate::ui_test_support::assert_component_roles(&components,&["card"]);
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
                assert!((recipient_card.height()-theme::RECEPTION_RECIPIENT_SECTION_HEIGHT).abs()<=0.5,
                    "recipient card height drifted: {recipient_card:?}");
                assert!((recipient_card.bottom()-note.bottom()-theme::RECEPTION_PANE_PADDING_Y).abs()<=0.5,
                    "recipient note has excess gap or is clipped: {note:?} in {recipient_card:?}");
                let review=first(&first_recipient,"review card");
                let items=first(&first_recipient,"items card");
                assert!((review.height()-items.height()).abs()<=0.5,
                    "variable card height mismatch: review={review:?}, items={items:?}");
                assert!((total_card.height()-theme::RECEPTION_ORDER_TOTAL_SECTION_HEIGHT).abs()<=0.5);
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
    fn reception_document_viewport_reserves_equal_border_and_padding(){
        let rect=Rect::from_min_size(egui::pos2(12.0,24.0),Vec2::new(200.0,100.0));
        let inner=crate::card::reception_document_inner_rect(rect).unwrap();
        assert_eq!(inner,rect.shrink(theme::SPACE_4+theme::FRAME_STROKE_WIDTH));
        assert_eq!(inner.left()-rect.left(),5.0);
        assert_eq!(inner.top()-rect.top(),5.0);
        assert_eq!(rect.right()-inner.right(),5.0);
        assert_eq!(rect.bottom()-inner.bottom(),5.0);
        assert!(crate::card::reception_document_inner_rect(
            Rect::from_min_size(rect.min,Vec2::new(200.0,10.0))).is_none());
        let source=include_str!("main.rs");
        assert_eq!(source.matches("crate::card::reception_document_viewport(").count(),5);
        assert!(!source.contains("crate::card::source_document_frame()"));
        assert!(!source.contains("crate::card::paint_source_document_surface("));
        let manual=include_str!("manual_workspace.rs");
        let manual_render=manual.split("mod manual_assignment_gesture_tests{").next().unwrap();
        assert_eq!(manual_render.matches("crate::card::reception_document_viewport(").count(),1);
        assert!(!manual_render.contains("crate::card::source_document_frame()"));
        assert!(!manual_render.contains("crate::card::paint_source_document_surface("));
    }

    #[test]
    fn reception_section_divider_keeps_8pt_to_visible_document_frame(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let trace=render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"발주 원문",320.0,|_|{},|body|{
                let (rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),90.0),Sense::hover());
                let _=crate::card::reception_document_viewport(body,rect,
                    "regression-document","regression-document-scroll",|_|{});
                record("visible document frame",rect);
            });
        });
        let header=first(&trace,"source header");
        let divider=first(&trace,"source divider");
        let document=first(&trace,"visible document frame");
        let card=first(&trace,"source card");
        let header_top=header.top()-card.top();
        let top_gap=divider.top()-header.bottom();
        let bottom_gap=document.top()-divider.bottom();
        assert!((header_top-8.0).abs()<=0.5,
            "card-to-header clearance drifted: {header_top:.2}pt");
        assert!((top_gap-8.0).abs()<=0.5,
            "header-row-to-divider clearance drifted: {top_gap:.2}pt");
        assert!((bottom_gap-8.0).abs()<=0.5,
            "divider-to-visible-document-frame clearance drifted: {bottom_gap:.2}pt");
        let expected_chrome=theme::RECEPTION_PANE_PADDING_Y
            +theme::RECEPTION_PANE_PADDING_Y
            +theme::SECTION_HEADER_HEIGHT
            +17.0;
        assert!((docked_section_body_height(320.0)-(320.0-expected_chrome)).abs()<=0.5,
            "fixed-height body calculator does not reserve both divider gaps");
    }

    #[test]
    fn reception_card_uses_equal_eight_point_vertical_insets(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let trace=render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"발주 원문",280.0,|_|{},|body|{
                body.spacing_mut().item_spacing.y=0.0;
                let (rect,_)=body.allocate_exact_size(
                    Vec2::new(body.available_width(),docked_section_body_height(280.0)),Sense::hover());
                record("full reception body",rect);
            });
        });
        let card=first(&trace,"source card");
        let header=first(&trace,"source header");
        let body=first(&trace,"full reception body");
        assert!((header.top()-card.top()-8.0).abs()<=0.5);
        assert!((card.bottom()-body.bottom()-8.0).abs()<=0.5,
            "card bottom inset drifted: card={card:?}, body={body:?}");
        assert!((card.height()-280.0).abs()<=0.5);
    }

    #[test]
    fn reception_client_and_footer_share_visible_divider_clearance(){
        let ctx=test_context();
        let screen=Vec2::new(900.0,700.0);
        let work=Rect::from_min_max(egui::pos2(24.0,24.0),egui::pos2(876.0,676.0));
        let trace=render(&ctx,screen,work,|ui|{
            reception_section_with_header(ui,"발주 원문",280.0,|_|{},|body|{
                body.spacing_mut().item_spacing.y=0.0;
                let (rect,_)=body.allocate_exact_size(Vec2::new(body.available_width(),150.0),Sense::hover());
                let client=crate::ui_layout::reception_source_client_rect(rect);
                record("visible client card",client);
            });
            reception_section_with_header(ui,"복사용 자동 메시지",280.0,|_|{},|body|{
                body.spacing_mut().item_spacing.y=0.0;
                let (rect,_)=body.allocate_exact_size(Vec2::new(body.available_width(),90.0),Sense::hover());
                record("message canvas",rect);
                let _=crate::card::reception_document_viewport(body,rect,"footer-test","footer-scroll",|_|{});
                crate::ui_layout::reception_message_footer_gap(body);
                let (actions,_)=body.allocate_exact_size(Vec2::new(body.available_width(),theme::ACTION_STANDARD_HEIGHT),Sense::hover());
                record("message actions",actions);
            });
        });
        let divider=first(&trace,"source divider");
        let client=first(&trace,"visible client card");
        let canvas=first(&trace,"message canvas");
        let actions=first(&trace,"message actions");
        assert!((client.top()-divider.bottom()-theme::RECEPTION_SECTION_DIVIDER_GAP).abs()<0.5);
        assert!((actions.top()-canvas.bottom()-theme::RECEPTION_MESSAGE_ACTION_GAP).abs()<0.5);
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
            ("automatic_message_manager.rs",1), // fixed footer
            ("order_history_detail.rs",1), // repeated payment records
            ("freight_manager.rs",1), // compact context menu
            ("excel_manager.rs",1), // repeated reply rows
            ("workflow_board_ui.rs",2), // compact expanded card boundaries
            ("category_dropdown.rs",2), // compact menu
            ("navigation.rs",1), // compact calendar header/grid boundary
        ];
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations=Vec::new();
        for entry in std::fs::read_dir(root).unwrap(){
            let path=entry.unwrap().path();
            if path.extension().and_then(|s|s.to_str())!=Some("rs"){continue;}
            let name=path.file_name().unwrap().to_str().unwrap();
            if name=="theme.rs"{continue;}
            let source=std::fs::read_to_string(&path).unwrap();
            if source.starts_with("#![cfg(test)]"){continue;}
            let production=production_source_for_contract(&source);
            let count=production.lines().filter(|line|!line.trim_start().starts_with("//"))
                .filter(|line|!(name=="ui_primitives.rs"&&line.trim_start().starts_with("pub fn soft_divider(")))
                .map(|line|line.matches("soft_divider(").count()).sum::<usize>();
            let limit=allowed.iter().find(|(file,_)|*file==name).map(|(_,n)|*n).unwrap_or(0);
            if count>limit{violations.push(format!("{name}: {count} compact dividers, reviewed maximum {limit}"));}
            for (line_no,line) in production.lines().enumerate(){
                if line.trim_start().starts_with("//"){continue;}
                if (line.contains("theme::soft_rule(")
                    && !(name=="ui_primitives.rs"&&line.trim()=="theme::soft_rule(ui);"))
                    ||line.contains("theme::divider(")||line.contains(".separator("){
                    violations.push(format!("{name}:{}: raw divider bypass",line_no+1));
                }
            }
        }
        assert!(violations.is_empty(),"structural divider bypass:\n{}",violations.join("\n"));
    }
}

#[cfg(test)]
mod shared_composite_control_contract_tests{
    #[test]
    fn migrated_composite_controls_use_their_shared_owners(){
        let reception=include_str!("composite_reception.rs");
        assert!(reception.contains("crate::button::remove_at("));
        assert_eq!(reception.matches("crate::table::editable_cell_hit(").count(),3);
        assert!(reception.contains("crate::table::paint_editable_cell_text("));
        assert!(!reception.contains("let action=ui.interact(hit,ui.make_persistent_id((\"item-remove\""));
        let models=include_str!("model_group_manager.rs");
        assert!(models.contains("crate::table::registry_table("));
        assert!(models.contains("crate::table::registry_cell("));
        assert!(!models.contains("crate::card::selectable_list_row("));
        assert!(include_str!("workflow_board_ui.rs").contains("crate::button::disclosure_at("));
        assert!(include_str!("order_history_manager.rs").contains("crate::button::disclosure_at("));
        assert!(include_str!("composite_workflow.rs").contains("highlight::Geometry::Shipment"));
    }
}

#[cfg(test)]
mod inline_labeled_form_contract_tests{
    /// Explicitly approved presentation-only field labels. Editable form
    /// captions must go through the shared horizontal row instead.
    #[test]
    fn raw_field_labels_are_limited_to_readonly_presentation(){
        fn scan(root:&std::path::Path,dir:&std::path::Path,violations:&mut Vec<String>){
            for entry in std::fs::read_dir(dir).expect("read egui source directory"){
                let path=entry.expect("source entry").path();
                if path.is_dir(){scan(root,&path,violations);continue;}
                if path.extension().and_then(|v|v.to_str())!=Some("rs"){continue;}
                let name=path.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/");
                if name.ends_with("regression_tests.rs")||name=="ui_regressions.rs"||name=="ui_test_support.rs"{continue;}
                let source=std::fs::read_to_string(&path).unwrap();
                if source.starts_with("#![cfg(test)]"){continue;}
                if !source.contains("field_label("){continue;}
                let production=crate::ui_test_support::production_source_for_contract(&source);
                let mut compact=production.split_whitespace().collect::<String>()
                    .replace(".label(crate::typography::field_label(",".label(typography::field_label(");
                let allowed:&[&str]=match name.as_str(){
                    "main.rs"=>&["history.label(typography::field_label(\"수정전\"));",
                        "history.label(typography::field_label(\"수정후\"));"],
                    "settings_view.rs"=>&["detail.label(typography::field_label(\"정정전작업용원문\"));",
                        "detail.label(typography::field_label(\"정정후작업용원문\"));",
                        "detail.label(typography::field_label(\"최초접수원문\"));"],
                    "settings_help.rs"=>&["ui.label(typography::field_label(\"언제사용하나요?\"));",
                        "ui.label(typography::field_label(\"사용방법\"));",
                        "ui.label(typography::field_label(\"사용하면어떻게되나요?\"));",
                        "ui.label(typography::field_label(\"알아둘점\"));"],
                    "popup/mod.rs"=>&["self.ui.label(typography::field_label(label));self.ui.label(typography::body(value));"],
                    "daily_settlement_manager.rs"=>&["line.label(typography::field_label(label));"],
                    "excel_manager.rs"=>&["ui.label(typography::field_label(&format!(\"미해결회신{count}행\")));",
                        "line.label(typography::field_label(&format!(\"{}/{}행/{}\","],
                    "product_excel_ui.rs"=>&["counts.label(typography::field_label(&format!(\"{title}{value}건\")"],
                    "freight_manager.rs"=>&["bulk.label(typography::field_label(&format!(\"{}개선택\",selected.len()"],
                    "product_manager.rs"=>&["line.label(typography::field_label(\"원\"));"],
                    _=>&[],
                };
                for token in allowed{compact=compact.replacen(token,"",1);}
                if compact.contains(".label(typography::field_label("){
                    violations.push(name);
                }
            }
        }
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations=Vec::new();
        scan(&root,&root,&mut violations);
        assert!(violations.is_empty(),"use the common inline form row instead of raw editable labels: {violations:?}");
    }
    #[test]
    fn shared_text_form_controls_stay_beside_labels_at_different_widths(){
        use eframe::egui;
        for width in [250.0,720.0]{
            let ctx=crate::ui_test_support::test_context();
            let screen=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width,400.0));
            let mut value=String::new();
            crate::ui_test_support::begin_component_geometry();
            let output=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    crate::popup::modal_form_row_spacing(ui);
                    crate::popup::modal_compact_text_field(ui,"브랜드명",&mut value,"");
                    crate::popup::modal_named_text_field(ui,"그룹 이름",&mut value,"",true);
                });
            });
            let trace=crate::ui_test_support::finish_component_geometry(screen);
            let inputs=trace.iter().filter(|s|s.role=="input").map(|s|s.rect).collect::<Vec<_>>();
            assert_eq!(inputs.len(),2);
            for (label,input) in ["브랜드명","그룹 이름"].into_iter().zip(inputs.iter()){
                let ink=crate::ui_test_support::painted_text_rects(&output,label)[0].0;
                assert!(ink.right()<=input.left()+0.5);
                assert!((ink.center().y-input.center().y).abs()<=3.0);
                assert!((input.height()-crate::theme::INPUT_HEIGHT).abs()<0.5);
            }
            assert!((inputs[0].left()-inputs[1].left()).abs()<0.5);
        }
    }
    #[test]
    fn editor_forms_use_common_inline_label_layout(){
        let popup=include_str!("popup/mod.rs");
        for (start,end) in [("fn modal_compact_text_field(","fn modal_named_text_field("),
            ("fn modal_named_text_field(","enum ManagementMenuRole")]{
            let section=popup.split(start).nth(1).unwrap().split(end).next().unwrap();
            assert!(section.contains("modal_labeled_row_with_width("));
            assert!(!section.contains(".label(typography::field_label("));
        }
        for source in [include_str!("brand_manager.rs"),include_str!("alias_manager.rs"),
            include_str!("automatic_message_manager.rs")]{
            let production=crate::ui_test_support::production_source_for_contract(source);
            assert!(!production.contains(".label(typography::field_label("));
        }
        let settings=include_str!("settings_view.rs");
        for (start,end) in [("fn edit_input_width(","fn section("),
            ("fn recommendation_picker(","fn settings_title_row("),
            ("fn scale_selector(","fn show_accounts(")]{
            let section=settings.split(start).nth(1).unwrap().split(end).next().unwrap();
            assert!(section.contains("modal_labeled_row"));
            assert!(!section.contains(".label(typography::field_label("));
        }
    }
}

#[cfg(test)]
mod shared_composite_layout_contract_tests{
    use super::*;
    #[test]
    fn migrated_screens_route_geometry_through_shared_owners(){
        let client=include_str!("client_manager.rs");
        let category=include_str!("category_manager.rs");
        assert!(client.contains("crate::ui_layout::management_toolbar_pair("));
        assert!(category.contains("crate::ui_layout::management_toolbar_pair("));
        for source in [include_str!("product_manager.rs"),include_str!("freight_manager.rs"),
            include_str!("excel_manager.rs"),include_str!("order_history_manager.rs")]{
            assert!(source.contains("crate::ui_layout::management_toolbar_slot("));
        }
        for source in [include_str!("product_manager.rs"),include_str!("freight_manager.rs"),include_str!("excel_manager.rs")]{
            assert!(source.contains("crate::table::control_cell("));
        }
        assert!(client.contains("crate::popup::modal_form_pair("));
        assert!(include_str!("automatic_message_manager.rs").contains("crate::ui_layout::workspace_pane("));
        assert!(include_str!("manual_workspace.rs").contains("crate::ui_layout::workspace_pane("));
        assert!(include_str!("workflow_board_ui.rs").contains("crate::card::workflow_group_card("));
        assert!(include_str!("excel_manager.rs").contains("crate::navigation::excel_export_date_controls("));
        assert!(!include_str!("excel_manager.rs").contains("order_history_manager::excel_export_date_controls("));
        assert!(include_str!("table/mod.rs").contains("crate::popup::TagChip::new("));
        assert!(include_str!("review_panel.rs").contains("crate::button::collapsing_header("));
    }
    #[test]
    fn toolbar_pair_uses_one_band_or_two_without_overlap(){
        for width in [500.0,1000.0]{
            let ctx=crate::ui_test_support::test_context();
            let screen=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,400.0));
            let mut slots=Vec::new();
            let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    crate::ui_layout::management_toolbar_pair(ui,crate::ui_layout::ToolbarPair{
                        search_id:"test-search",actions_id:"test-actions",
                        narrow_search_id:"test-search-narrow",narrow_actions_id:"test-actions-narrow",
                        stack_width:700.0,search_max:280.0,actions_width:300.0,
                        narrow_actions_first:false,actions_rtl:true,clip:true,
                    },|slot,part|{
                        slots.push((part,slot.max_rect()));
                        slot.allocate_space(Vec2::new(10.0,crate::theme::INPUT_HEIGHT));
                    });
                });
            });
            assert_eq!(slots.len(),2);
            let first=slots[0].1;
            let second=slots[1].1;
            assert!(!first.intersect(second).is_positive());
            assert!((first.height()-theme::INPUT_HEIGHT).abs()<0.5);
            assert!((second.height()-theme::INPUT_HEIGHT).abs()<0.5);
            if width<700.0{
                assert!(second.top()>=first.bottom()+theme::SPACE_8-0.5);
                assert!((first.right()-second.right()).abs()<0.5);
            }else{
                assert!((first.center().y-second.center().y).abs()<0.5);
                assert!(second.left()>=first.right()+theme::SPACE_12-0.5);
            }
        }
    }
    #[test]
    fn compact_date_rows_and_price_cells_keep_approved_geometry(){
        let ctx=crate::ui_test_support::test_context();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let (first,second)=crate::navigation::compact_date_range_rows(ui,390.0,350.0);
                assert!((first.right()-second.right()).abs()<0.5);
                assert!((second.top()-first.bottom()-theme::SPACE_8).abs()<0.5);
                let cell=Rect::from_min_size(egui::pos2(20.0,100.0),Vec2::new(116.0,42.0));
                let (editor,clip)=crate::table::money_editor_rects(cell,ui.clip_rect());
                assert_eq!(editor.width(),112.0);
                assert_eq!(editor.center().y,cell.center().y);
                assert_eq!(clip,cell.intersect(ui.clip_rect()));
            });
        });
    }
}


// Transient feedback is not document data. Screen-local inline labels must
// not allocate new rows for validation/error/notice signals. Permanent
// labels, explicit decision-dialog bodies and actual review/data rows remain
// outside this lexical check; their geometry requires rendered transitions.
#[cfg(test)]
mod notice_no_reflow_contract_tests {
    use regex::Regex;
    use std::path::Path;

    fn violations(file:&str,source:&str)->Vec<String>{
        let direct=Regex::new(r"\.\s*label\s*\(\s*(?:(?:crate\s*::\s*)?typography\s*::\s*(?:caption|body|status)\s*\(\s*)?&?\s*(?:self\s*\.\s*)?(?:[A-Za-z_][A-Za-z_0-9]*_)?(?:notice|error)\b").unwrap();
        let constructor=Regex::new(r"Label\s*::\s*new\s*\(\s*(?:typography\s*::\s*(?:caption|body|status)\s*\(\s*)?&?\s*(?:self\s*\.\s*)?(?:[A-Za-z_][A-Za-z_0-9]*_)?(?:notice|error)\b").unwrap();
        let input_hint=Regex::new(r"\bif\s+[^{};\n]*(?:alias_input|tag_input|search_tag_input|draft_input)\b[^{};\n]*\{\s*[A-Za-z_][A-Za-z_0-9]*\s*\.\s*label\s*\(").unwrap();
        // Recognize literal feedback and named guidance as well as error variables.
        // Deliberately do not prohibit review_detail: issue cards are business data.
        let transient_caption=Regex::new(r#"(?:\.\s*label|Label\s*::\s*new)\s*\(\s*typography\s*::\s*(?:caption|body|status)\s*\(\s*(?:crate\s*::\s*review_panel\s*::\s*MODEL_RESOLUTION_GUIDANCE|"(?:현재 주문은 조회만 가능합니다|배송 묶음 변경 내용을 저장하는 중입니다)|&?\s*format!\s*\(\s*"이전 대표색상:)"#).unwrap();
        let mut hits=Vec::new();
        for rule in [&direct,&constructor,&input_hint,&transient_caption]{
            for m in rule.find_iter(source){
                let line=source[..m.start()].bytes().filter(|&b|b==b'\n').count()+1;
                let line_start=source[..m.start()].rfind('\n').map_or(0,|i|i+1);
                if source[line_start..m.start()].trim_start().starts_with("//"){continue;}
                hits.push(format!("{file}:{line}: transient notice allocates an inline widget; use floating_notice or a stable overlay"));
            }
        }
        hits.sort();hits.dedup();hits
    }

    fn scan(root:&Path,path:&Path,hits:&mut Vec<String>){
        for entry in std::fs::read_dir(path).expect("read egui production sources"){
            let p=entry.expect("source entry").path();
            if p.is_dir(){scan(root,&p,hits);continue;}
            if p.extension().and_then(|v|v.to_str())!=Some("rs"){continue;}
            let name=p.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/");
            if name.ends_with("regression_tests.rs")||name=="ui_regressions.rs"||name=="ui_test_support.rs"{continue;}
            let source=std::fs::read_to_string(&p).expect("read Rust source");
            if source.starts_with("#![cfg(test)]"){continue;}
            let production=crate::ui_test_support::production_source_for_contract(&source);
            hits.extend(violations(&name,&production));
        }
    }

    #[test]
    fn guard_detects_direct_feedback_and_multiline_input_hint(){
        for bad in [
            "if let Some(notice)=self.alias_notice.as_deref(){body.label(typography::caption(notice));}",
            "if let Some(error)=self.error.as_deref(){body.label(typography::caption(error));}",
            "ui.add(egui::Label::new(typography::body(save_error)));",
            "if !self.alias_input.trim().is_empty(){\n body.label(typography::caption(\"hint\"));\n}",
            "if applying { ui.label(typography::caption(\"배송 묶음 변경 내용을 저장하는 중입니다.\")); }",
            "ui.add(egui::Label::new(typography::caption(crate::review_panel::MODEL_RESOLUTION_GUIDANCE)).wrap());",
            "body.label(typography::caption(&format!(\"이전 대표색상: {}\", color)));",
        ]{assert!(!violations("synthetic.rs",bad).is_empty(),"missed: {bad}");}
        for good in [
            "crate::floating_notice::sync(ctx,signal,source,tone,error);",
            "body.label(typography::field_label(\"검색 별칭\"));",
            "for row in review_data { body.label(typography::body(row)); }",
        ]{assert!(violations("synthetic.rs",good).is_empty(),"false positive: {good}");}
    }

    #[test]
    fn production_screens_reject_direct_inline_transient_notices(){
        let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits=Vec::new();
        scan(&root,&root,&mut hits);
        hits.sort();
        assert!(hits.is_empty(),"UI notice reflow violations:\n{}",hits.join("\n"));
    }
}


// Validate rendered glyph colors, not merely the presence of a color token.
// The literal approved value is deliberately independent of production code.
#[cfg(test)]
mod shared_placeholder_color_contract_tests{
    use super::*;
    use crate::dropdown::{DropdownCaption,ManagementSearchChoice};
    use egui::Color32;
    const APPROVED:Color32=Color32::from_rgb(96,96,96);
    const HINT:&str="Select or enter";
    const VALUE:&str="2026-11-27";

    fn colors(output:&egui::FullOutput,label:&str)->Vec<Color32>{
        fn visit(shape:&egui::Shape,label:&str,found:&mut Vec<Color32>){
            match shape{
                egui::Shape::Text(text) if text.galley.text()==label=>{
                    for section in &text.galley.job.sections{
                        let ink=section.format.color;
                        found.push(text.override_text_color.unwrap_or(
                            if ink==Color32::PLACEHOLDER{text.fallback_color}else{ink}
                        ));
                    }
                }
                egui::Shape::Vec(shapes)=>for shape in shapes{visit(shape,label,found);},
                _=>{}
            }
        }
        let mut found=Vec::new();
        for shape in &output.shapes{visit(&shape.shape,label,&mut found);}
        found
    }

    fn assert_color(output:&egui::FullOutput,label:&str,expected:Color32){
        let actual=colors(output,label);
        assert!(!actual.is_empty(),"caption not rendered: {label}");
        assert!(actual.iter().all(|ink|*ink==expected),
            "{label}: expected {expected:?}, got {actual:?}");
    }

    fn render(ctx:&egui::Context,pointer:Option<egui::Pos2>,
        mut draw:impl FnMut(&mut Ui)->egui::Response
    )->(egui::FullOutput,egui::Response){
        let mut response=None;
        let output=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(800.0,500.0))),
            events:pointer.into_iter().map(egui::Event::PointerMoved).collect(),
            ..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(440.0);
                response=Some(draw(ui));
            });
        });
        (output,response.expect("control rendered"))
    }

    #[test]
    fn actual_input_hints_keep_approved_ink_when_empty_hovered_and_focused(){
        assert_eq!(crate::input::INPUT_PLACEHOLDER_COLOR,APPROVED);
        for kind in 0..4{
            let ctx=test_context();
            let mut value=String::new();
            let draw=|ui:&mut Ui,value:&mut String|match kind{
                0=>crate::input::flexible_text_field(ui,value,240.0,HINT),
                1=>crate::input::management_search_field(ui,value,240.0,HINT),
                2=>crate::input::management_date_text_field(ui,value,240.0,36.0,HINT),
                _=>crate::input::multiline_text_field(ui,value,true,240.0,2,Some(HINT)),
            };
            let (output,field)=render(&ctx,None,|ui|draw(ui,&mut value));
            assert_color(&output,HINT,APPROVED);
            assert!(value.is_empty(),"placeholder became input data");
            let (output,_)=render(&ctx,Some(field.rect.center()),|ui|draw(ui,&mut value));
            assert_color(&output,HINT,APPROVED);
            ctx.memory_mut(|memory|memory.request_focus(field.id));
            let (output,focused)=render(&ctx,Some(field.rect.center()),|ui|draw(ui,&mut value));
            assert!(focused.has_focus(),"input focus not exercised: {kind}");
            assert_color(&output,HINT,APPROVED);
            value=VALUE.to_owned();
            let (output,_)=render(&ctx,None,|ui|draw(ui,&mut value));
            assert_color(&output,VALUE,theme::TEXT);
            assert!(colors(&output,HINT).is_empty(),"hint survives nonempty input");
            value.clear();
            let (output,_)=render(&ctx,None,|ui|draw(ui,&mut value));
            assert_color(&output,HINT,APPROVED);
            assert!(value.is_empty());
        }
    }

    #[test]
    fn actual_dropdown_caption_ink_distinguishes_selection_and_valid_empty_filters(){
        for kind in 0..4{
            let ctx=test_context();
            let mut selected=String::new();
            let choices=[ManagementSearchChoice::new("id".into(),VALUE.into(),VALUE.into())];
            let draw=|ui:&mut Ui,selected:&mut String,placeholder:bool|{
                let caption=if placeholder{HINT}else{VALUE};
                match kind{
                    0=>crate::dropdown::management_dropdown(ui,
                        DropdownCaption::new(caption,placeholder),240.0,true,|_|{}),
                    1=>crate::dropdown::management_searchable_dropdown(ui,
                        DropdownCaption::new(caption,placeholder),240.0,true,
                        selected,&choices,"Search"),
                    2=>crate::dropdown::management_dropdown_values(ui,
                        &typography::inline_body_values(&[(caption,None)]),
                        240.0,true,placeholder,|_|{}),
                    // The empty category value is deliberately retained. In
                    // filter mode it is a valid "all categories" selection.
                    _=>crate::category_dropdown::category_dropdown(ui,&[],selected,
                        240.0,caption,true,true,placeholder),
                }
            };
            let (output,trigger)=render(&ctx,None,|ui|draw(ui,&mut selected,true));
            assert_color(&output,HINT,APPROVED);
            let (output,_)=render(&ctx,Some(trigger.rect.center()),|ui|draw(ui,&mut selected,true));
            assert_color(&output,HINT,APPROVED);
            if kind!=3{selected="id".to_owned();}
            let (output,_)=render(&ctx,None,|ui|draw(ui,&mut selected,false));
            assert_color(&output,VALUE,theme::TEXT);
            assert!(colors(&output,HINT).is_empty(),"hint survives selection");
            selected.clear();
            let (output,_)=render(&ctx,None,|ui|draw(ui,&mut selected,true));
            assert_color(&output,HINT,APPROVED);
        }
    }

    #[test]
    fn production_input_hints_must_use_shared_placeholder_builder(){
        fn check(name:&str,source:&str)->Vec<String>{
            let hint=regex::Regex::new(r"\.\s*hint_text\s*\(").unwrap();
            let shared=regex::Regex::new(r"^\s*input_placeholder\s*\(").unwrap();
            let mut hits=Vec::new();
            for m in hint.find_iter(source){
                let start=source[..m.start()].rfind('\n').map_or(0,|i|i+1);
                if source[start..m.start()].trim_start().starts_with("//"){continue;}
                if name!="input/mod.rs"||!shared.is_match(&source[m.end()..]){
                    let line=source[..m.start()].bytes().filter(|b|*b==b'\n').count()+1;
                    hits.push(format!("{name}:{line}: use the shared input placeholder builder"));
                }
            }
            hits
        }
        fn scan(root:&std::path::Path,path:&std::path::Path,hits:&mut Vec<String>){
            for entry in std::fs::read_dir(path).expect("read Rust source directory"){
                let p=entry.expect("source entry").path();
                if p.is_dir(){scan(root,&p,hits);continue;}
                if p.extension().and_then(|v|v.to_str())!=Some("rs"){continue;}
                let name=p.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/");
                if name.ends_with("regression_tests.rs")||name=="ui_regressions.rs"||name=="ui_test_support.rs"{continue;}
                let source=std::fs::read_to_string(&p).expect("read source");
                if source.starts_with("#![cfg(test)]"){continue;}
                hits.extend(check(&name,&production_source_for_contract(&source)));
            }
        }
        assert_eq!(check("product_manager.rs","editor.hint_text(\"hint\")").len(),1);
        assert_eq!(check("input/mod.rs","editor.hint_text(hint)").len(),1);
        assert!(check("input/mod.rs","editor.hint_text(input_placeholder(hint))").is_empty());
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits=Vec::new();
        scan(&root,&root,&mut hits);
        assert!(hits.is_empty(),"placeholder ownership violations:\n{}",hits.join("\n"));
    }
}
