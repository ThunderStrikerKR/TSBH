#![cfg(test)]
//! Regression tests owned by the checkbox component.











// Composite widgets import primitive renderers from their owning modules.











// Shared table primitives used by reception and other composite widgets.

#[cfg(test)]
mod shared_checkbox_contract_tests {
    #[test]
    fn vat_dialog_uses_shared_checkbox_and_nested_frame_components(){
        let source=include_str!("../main.rs");
        let dialog=source.split("fn show_vat_settings(").nth(1).unwrap()
            .split("fn show_edit_history(").next().unwrap();
        assert!(dialog.contains("crate::checkbox::checkbox("));
        assert!(dialog.contains("crate::checkbox::checkbox_list_row("));
        assert!(dialog.contains("crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::RecipientChecklist)"));
        assert!(!dialog.contains(".inner_margin("));
        assert!(!dialog.contains("egui::Checkbox::new("));
        assert!(!dialog.contains("egui::Frame::new()"));

        let client=include_str!("../client_manager.rs");
        let boolean=client.split("fn bool_edit(").nth(1).unwrap()
            .split("fn new_id(").next().unwrap();
        assert!(boolean.contains("crate::checkbox::checkbox("));
        assert!(!boolean.contains("ui.checkbox("));

        let main=include_str!("../main.rs");
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

        let category=include_str!("../category_manager.rs");
        assert!(category.contains("crate::checkbox::checkbox("));
        assert!(!category.contains("notice.checkbox("));

        let products=include_str!("../product_manager.rs");
        assert!(products.contains("crate::checkbox::checkbox("));
        assert!(products.contains("&mut self.missing"));
        assert!(!products.contains("filter.checkbox("));
    }
}


#[cfg(test)]
mod shared_checkbox_rendered_geometry_tests{
    use eframe::egui::{self,Rect,Vec2,Sense};
    use crate::{checkbox,theme};
    use crate::ui_test_support::{test_context,assert_rect_geometry,assert_rect_inside,
        assert_painted_caption,assert_geometry_value};

    #[test]
    fn checkbox_list_preserves_hit_caption_and_next_row_across_states(){
        let ctx=test_context();
        let mut baseline:Option<(Rect,Rect,Rect)>=None;
        for (checked,enabled) in [(false,true),(true,true),(true,false),(false,false)]{
            let mut value=checked;
            let mut geometry=None;
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(800.0,500.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    let response=checkbox::checkbox_list_row(ui,"render-check", &mut value,
                        "부가세 적용",enabled,theme::INPUT_HEIGHT);
                    assert_eq!(response.enabled(),enabled);
                    let next=ui.allocate_exact_size(Vec2::new(80.0,20.0),Sense::hover()).0;
                    geometry=Some((response.rect,next));
                });
            });
            let (hit,next)=geometry.expect("checkbox geometry");
            let ink=assert_painted_caption(&output,"부가세 적용",hit);
            assert!(hit.height()<=theme::INPUT_HEIGHT+1.0);
            assert!(next.top()>=hit.bottom()-0.5);
            assert_eq!(value,checked,"drawing must not modify selection");
            if let Some((old_hit,old_ink,old_next))=baseline{
                assert_rect_geometry("checkbox hit",hit,old_hit,0.5);
                assert_rect_geometry("checkbox caption",ink,old_ink,1.0);
                assert_rect_geometry("following row",next,old_next,0.5);
            }else{baseline=Some((hit,ink,next));}
        }
    }

    #[test]
    fn checked_table_checkbox_paints_neutral_fill_without_leaking_style(){
        fn has_fill(shape:&egui::Shape,hit:Rect,fill:egui::Color32)->bool{
            match shape{
                egui::Shape::Rect(r)=>r.fill==fill&&hit.contains(r.rect.center()),
                egui::Shape::Vec(shapes)=>shapes.iter().any(|s|has_fill(s,hit,fill)),
                _=>false,
            }
        }
        let ctx=test_context();
        let mut baseline=None;
        for (width,height) in [(44.0,34.0),(48.0,42.0)]{
            for checked in [false,true,false]{
                let mut value=checked;
                let mut hit=Rect::NOTHING;
                let output=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(800.0,500.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let before=ui.visuals().widgets.inactive.bg_fill;
                        let cell=ui.allocate_exact_size(Vec2::new(width,height),Sense::hover()).0;
                        hit=checkbox::management_table_checkbox(ui,cell,cell,&mut value,true,"contrast-check").rect;
                        assert_eq!(ui.visuals().widgets.inactive.bg_fill,before);
                    });
                });
                if checked{
                    assert!(output.shapes.iter().any(|s|has_fill(&s.shape,hit,theme::checkbox_palette(true,theme::VisualState::Normal).expect("checked palette").fill)));
                }
                assert!(!output.shapes.iter().any(|s|
                    has_fill(&s.shape,hit,theme::ORANGE_EMPHASIS_BG)||
                    has_fill(&s.shape,hit,theme::SELECTED_ORDER)||
                    has_fill(&s.shape,hit,egui::Color32::from_rgb(140,76,16))));
                assert_eq!(value,checked);
                if let Some((old_width,old_height,old_hit))=baseline{
                    if old_width==width&&old_height==height{
                        assert_rect_geometry("checked checkbox",hit,old_hit,0.5);
                    }
                }
                baseline=Some((width,height,hit));
            }
        }
    }

    #[test]
    fn rendered_table_checkbox_stays_in_header_and_body_cells(){
        let ctx=test_context();
        for (width,height) in [(44.0,34.0),(48.0,42.0)]{
            let mut previous=None;
            for (checked,enabled) in [(false,true),(true,true),(true,false)]{
                let mut value=checked;
                let mut actual=None;
                let _=ctx.run(egui::RawInput::default(),|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let cell=ui.allocate_exact_size(Vec2::new(width,height),Sense::hover()).0;
                        let response=checkbox::management_table_checkbox(ui,cell,cell,
                            &mut value,enabled,"checkbox-cell");
                        assert_eq!(response.enabled(),enabled);
                        actual=Some((cell,response.rect));
                    });
                });
                let (cell,hit)=actual.expect("table checkbox geometry");
                assert_rect_inside("table checkbox",hit,cell,1.0);
                assert_geometry_value("table checkbox","vertical center",hit.center().y,cell.center().y,1.0);
                if let Some(old)=previous{assert_rect_geometry("table checkbox state",hit,old,0.5);}
                else{previous=Some(hit);}
                assert_eq!(value,checked);
            }
        }
    }
}
