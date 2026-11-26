#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the input component.

use eframe::egui::{self,Frame,Rect,Response,Sense,Ui,Vec2};

use crate::{theme,typography};
use crate::ui_primitives::*;


use crate::composite_reception::{reception_recipient_fields,reception_total_footer};


use crate::display_format::{display_won,display_money_number};

use crate::ui_test_support::{test_context,assert_layout_rects_invariant};
use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.





use crate::input::{FieldKind,field,parse_money_input,money_edit_text,flexible_text_field,management_date_text_field,multiline_text_field,multiline_text_field_sized,management_search_field,reception_recipient_inline_cell_editor,reception_inline_cell_editor};
use crate::input::{reception_recipient_text_edit,reception_money_inline_cell_editor,RECEPTION_MONEY_UNIT_GAP,RECEPTION_MONEY_PENDING,money_unit_left};
use crate::input::{formatted_money_input,money_cursor_after_format,money_unit_margin,reception_money_font};



// Shared table primitives used by reception and other composite widgets.

#[cfg(test)]
mod management_date_field_contract_tests {
    #[test]
    fn order_history_date_group_uses_shared_date_text_field(){
        let source=include_str!("../order_history_manager.rs");
        let date=source.split("fn range_date_picker_rtl(").nth(1).unwrap()
            .split("\nfn ").next().unwrap();
        assert!(date.contains("crate::input::management_date_text_field("));
        assert!(!date.contains("egui::TextEdit::singleline(date)"));
    }
}

#[cfg(test)]
mod shared_multiline_field_contract_tests {
    #[test]
    fn automatic_message_and_history_memo_use_shared_multiline_editor(){
        let messages=include_str!("../automatic_message_manager.rs");
        assert_eq!(messages.matches("crate::input::multiline_text_field(").count(),2);
        assert!(!messages.contains("TextEdit::multiline("));
        assert_eq!(messages.matches("crate::card::preview_surface_frame()").count(),2);
        assert!(!messages.contains("egui::Frame::new()"));
        let history=include_str!("../order_history_manager.rs");
        let memo=history.split("pub fn show_memo_modal(").nth(1).unwrap()
            .split("pub fn ").next().unwrap();
        assert!(memo.contains("crate::input::multiline_text_field("));
        assert!(!memo.contains("TextEdit::multiline("));

        let main=include_str!("../main.rs");
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
mod management_search_caret_tests {
    use super::*;

    #[test]
    fn every_management_query_surface_uses_shared_caret_policy(){
        // The list covers actual search/filter controls. Ordinary editing
        // of product names, client tags, dates, addresses, memo text, etc.
        // remains a native click-position TextEdit.
        let surfaces=[
            ("freight",include_str!("../freight_manager.rs"),2),
            ("products",include_str!("../product_manager.rs"),1),
            ("clients",include_str!("../client_manager.rs"),1),
            ("excel",include_str!("../excel_manager.rs"),1),
            ("order history",include_str!("../order_history_manager.rs"),1),
            // The narrow and wide branches are mutually exclusive render
            // paths for the same logical search field.
            ("categories",include_str!("../category_manager.rs"),2),
            ("model groups",include_str!("../model_group_manager.rs"),2),
            ("aliases",include_str!("../alias_manager.rs"),1),
            ("automatic messages",include_str!("../automatic_message_manager.rs"),1),
            ("audit archive",include_str!("../settings_view.rs"),1),
            ("client chooser",include_str!("../client_chooser.rs"),1),
            ("manual product chooser",include_str!("../manual_product_search.rs"),1),
        ];
        for (screen,source,expected) in surfaces {
            let count=source.matches("crate::input::management_search_field(").count();
            assert_eq!(count,expected,"{screen} query did not use the shared field");
        }
        // Search-tag entry is an editor, not a query field.
        let clients=include_str!("../client_manager.rs");
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
                    let response=if numeric{
                        crate::input::reception_item_numeric_editor(
                            ui,cell,&mut editing,true,Some(click),crate::input::ReceptionNumericCell::Money
                        ).0
                    }else{
                        reception_inline_cell_editor(ui,cell,&mut editing,true,Some(click))
                    };
                    let state=egui::TextEdit::load_state(ui.ctx(),response.id)
                        .expect("native caret state");
                    let range=state.cursor.char_range().expect("first caret");
                    assert_eq!(range.primary.index,range.secondary.index);
                    assert!(range.primary.index<=editing.chars().count());
                    selections.push(range.primary.index);
                }
            });
        });
        assert_eq!(selections.len(),2);
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
