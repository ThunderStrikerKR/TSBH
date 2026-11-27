#![cfg(test)]
//! Regression tests owned by the dropdown component.

use eframe::egui::{self,Rect,Vec2};

use crate::{theme,typography};


use crate::composite_reception::{freight_actions_need_second_row,reception_freight_row_controls};




use crate::ui_test_support::{test_context,assert_layout_rects_invariant};
use crate::popup::*;
// Composite widgets import primitive renderers from their owning modules.


use crate::dropdown::reception_cod_dropdown;


use crate::input::{FieldKind,money_field_with_unit};


use crate::dropdown::{ManagementSearchChoice,management_dropdown,management_dropdown_option,management_dropdown_compact_option};
use crate::dropdown::{management_dropdown_option_palette,management_search_choice_matches};
// Shared table primitives used by reception and other composite widgets.

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
                            Some(3000),Some(false),Some(false),visible,true,true,true,true,false);
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
        let source=include_str!("../dropdown/mod.rs");
        let block=source.split("fn management_searchable_dropdown<").nth(1).unwrap()
            .split("/// All menu options").next().unwrap();
        for required in [
            "management_dropdown_trigger(",
            "configure_option_menu(menu)",
            "management_search_field(menu,&mut search,input_width,hint)",
            "management_dropdown_option(",
            "crate::overlay::show_menu(&trigger,None,|menu|{",
            "configure_option_rows(list)",
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
        let screens:[(&str,&str,usize);5]=[
            ("alias_manager",include_str!("../alias_manager.rs"),3),
            ("client_manager",include_str!("../client_manager.rs"),4),
            ("freight_manager",include_str!("../freight_manager.rs"),5),
            // Two product brand selectors now use this shared searchable
            // variant; four ordinary product selectors retain the original.
            ("product_manager",include_str!("../product_manager.rs"),4),
            ("main",include_str!("../main.rs"),1),
        ];
        assert!(include_str!("../manual_workspace.rs").contains(".show_selection_modal("));
        // Both category selectors moved to the shared hierarchical
        // component; the six remaining standard product selectors still use
        // the original management-dropdown master.
        let products=include_str!("../product_manager.rs");
        assert_eq!(
            products.matches("category_dropdown::category_dropdown(").count(),2
        );
        assert_eq!(
            products.matches("crate::dropdown::management_searchable_dropdown(").count(),2
        );
        assert!(include_str!("../category_manager.rs")
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
        assert_eq!(typography::data_value_font(typography::DataValueKind::Money,typography::DataValueContext::Form),
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
        assert_eq!(crate::dropdown::MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT,320.0);
    }

    #[test]
    fn shared_select_options_have_real_vertical_padding_and_compact_width(){
        let ctx=egui::Context::default();
        let mut rects=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.scope(|choices|{
                    choices.spacing_mut().interact_size.y=
                        crate::dropdown::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
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
            assert_eq!(rect.height(),crate::dropdown::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
        }
        assert!(crate::dropdown::MANAGEMENT_DROPDOWN_OPTION_HEIGHT>=28.0);
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
                        crate::dropdown::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
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
        assert_eq!(normal.height(),crate::dropdown::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
        assert_eq!(compact.height(),crate::dropdown::MANAGEMENT_DROPDOWN_OPTION_HEIGHT);
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
        let source=include_str!("../dropdown/mod.rs");
        let inner=source.find("fn management_dropdown_option_inner<T:PartialEq>").unwrap();
        let first=source.find("fn management_dropdown_option<T:PartialEq>").unwrap();
        let end=source.len();
        assert_eq!(source[inner..first].matches("dropdown_option_selected(ui.ctx())").count(),1);
        assert_eq!(source[first..end].matches("management_dropdown_option_inner(").count(),4);
        assert!(!source[first..end].contains("dropdown_option_selected(ui.ctx())"));
        let popup=source.find("fn management_dropdown_inner(").unwrap();
        assert!(source[popup..inner].contains("crate::overlay::show_menu("));
        assert!(include_str!("../overlay.rs").contains("s.popup_trigger=Some(origin)"));
    }

    #[test]
    fn selected_and_hovered_options_have_distinct_scoped_colors(){
        assert_eq!(theme::MANAGEMENT_DROPDOWN_SELECTED_BG,theme::ORANGE_EMPHASIS_BG);
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
                    assert_eq!(popup.visuals().selection.stroke.color,theme::ORANGE_EMPHASIS_TEXT);
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
mod shared_dropdown_rendered_geometry_tests{
    use crate::theme;
    use eframe::egui::{self,Rect};
    use crate::ui_test_support::{test_context,assert_geometry_value,assert_rect_geometry,
        assert_painted_caption,painted_text_rects,assert_rect_inside};
    #[test]
    fn selected_option_paints_shared_orange_and_hover_remains_blue(){
        fn has_fill(shape:&egui::Shape,rect:Rect,fill:egui::Color32)->bool{
            match shape{
                egui::Shape::Rect(s)=>s.fill==fill&&s.rect.contains(rect.center()),
                egui::Shape::Vec(shapes)=>shapes.iter().any(|s|has_fill(s,rect,fill)),
                _=>false,
            }
        }
        let ctx=test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(800.0,500.0));
        let mut current="a";
        let mut selected=Rect::NOTHING;
        let mut other=Rect::NOTHING;
        for phase in 0..4{
            let events=if phase==0{vec![]}else{vec![egui::Event::PointerMoved(other.center())]};
            let output=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    crate::dropdown::management_dropdown_option_palette(ui);
                    selected=crate::dropdown::management_dropdown_option(ui,&mut current,"a","선택 브랜드").rect;
                    other=crate::dropdown::management_dropdown_option(ui,&mut current,"b","다른 브랜드").rect;
                });
            });
            if phase==3{
                assert!(output.shapes.iter().any(|s|has_fill(&s.shape,selected,theme::ORANGE_EMPHASIS_BG)),
                    "selected option must actually paint orange");
                assert!(output.shapes.iter().any(|s|has_fill(&s.shape,other,theme::MANAGEMENT_DROPDOWN_HOVER_BG)),
                    "unselected hovered option must actually paint blue");
                assert_eq!(current,"a");
            }
        }
    }
    #[test]
    fn freight_group_filter_and_bulk_selection_paint_same_orange(){
        fn check<T:Clone+PartialEq>(selected_value:T,other_value:T,selected_label:&str,other_label:&str){
            fn fill_at(shape:&egui::Shape,point:egui::Pos2,fill:egui::Color32)->bool{
                match shape{
                    egui::Shape::Rect(rect)=>rect.fill==fill&&rect.rect.contains(point),
                    egui::Shape::Vec(shapes)=>shapes.iter().any(|s|fill_at(s,point,fill)),
                    _=>false,
                }
            }
            let ctx=test_context();
            let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(800.0,500.0));
            let mut current=selected_value.clone();
            let mut selected=Rect::NOTHING;
            let mut other=Rect::NOTHING;
            for phase in 0..4{
                let events=if phase==0{vec![]}else{vec![egui::Event::PointerMoved(other.center())]};
                let output=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        crate::dropdown::management_dropdown_option_palette(ui);
                        selected=crate::dropdown::management_dropdown_option(ui,&mut current,
                            selected_value.clone(),selected_label).rect;
                        other=crate::dropdown::management_dropdown_option(ui,&mut current,
                            other_value.clone(),other_label).rect;
                    });
                });
                if phase==3{
                    assert!(output.shapes.iter().any(|s|fill_at(&s.shape,selected.center(),theme::ORANGE_EMPHASIS_BG)));
                    assert!(output.shapes.iter().any(|s|fill_at(&s.shape,other.center(),theme::MANAGEMENT_DROPDOWN_HOVER_BG)));
                    assert!(current==selected_value);
                }
            }
        }
        check("none".to_owned(),"group-1".to_owned(),"그룹 미지정만","배송그룹 01");
        check(Some("group-1".to_owned()),Some(String::new()),"배송그룹 01","미지정으로 변경");
    }
    #[test]
    fn dropdown_caption_clip_excludes_arrow_and_state_does_not_resize_trigger(){
        let ctx=test_context();
        for width in [180.0,90.0,180.0]{
            let mut baseline:Option<(Rect,Rect)>=None;
            for (label,enabled) in [("선불",true),("아주 긴 상품 분류 이름입니다",true),("선불",false)]{
                let mut actual=None;
                let output=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(800.0,500.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        ui.spacing_mut().interact_size.y=theme::ACTION_STANDARD_HEIGHT;
                        let response=crate::dropdown::management_dropdown_trigger(ui,label,width,enabled,None);
                        let next=ui.allocate_exact_size(egui::vec2(60.0,20.0),egui::Sense::hover()).0;
                        actual=Some((response.rect,next));
                        assert_eq!(response.enabled(),enabled);
                    });
                });
                let (rect,next)=actual.expect("dropdown response");
                assert_geometry_value("dropdown","width",rect.width(),width,1.0);
                assert_geometry_value("dropdown","height",rect.height(),theme::ACTION_STANDARD_HEIGHT,1.0);
                let (ink,clip)=painted_text_rects(&output,label).into_iter()
                    .find(|(_,clip)|clip.intersect(rect).is_positive())
                    .expect("painted dropdown label");
                assert_rect_inside("dropdown caption clip",clip,rect,1.0);
                assert_geometry_value("dropdown","left caption inset",clip.left()-rect.left(),theme::SPACE_12,1.0);
                assert!(clip.right()<=rect.right()-30.0+1.0,"caption clip enters arrow slot");
                assert!(ink.intersect(clip).is_positive());
                if label=="선불"{assert_painted_caption(&output,label,rect);}
                if let Some((old,following))=baseline{
                    assert_rect_geometry("dropdown state",rect,old,0.5);
                    assert_rect_geometry("following widget",next,following,0.5);
                }else{baseline=Some((rect,next));}
            }
        }
    }
}
