#![cfg(test)]
#![allow(unused_imports)]
//! Regression tests owned by the table component.

use eframe::egui::{self,Align,Layout,Rect,Sense,Vec2};

use crate::{theme,typography};
use crate::ui_primitives::*;

use crate::table::ManagementTableColumnLabel;

use crate::text_fit::{reception_ellipsized_product,reception_compact_quantity};
use crate::ui_layout::{management_toolbar_trailing_rect};


use crate::ui_test_support::{test_context,production_source_for_contract};
use crate::checkbox::*;
use crate::scrollbar::*;
use crate::popup::*;
use crate::card::*;
// Composite widgets import primitive renderers from their owning modules.





use crate::input::{reception_recipient_inline_cell_editor,reception_inline_cell_editor,reception_freight_inline_cell_editor};
use crate::input::{reception_recipient_text_edit,reception_freight_money_readonly};
use crate::input::{RECEPTION_FREIGHT_MONEY_WIDTH};



// Shared table primitives used by reception and other composite widgets.
use crate::table::{paint_table_galley,management_client_column_widths,management_client_column_edges,reception_item_columns,reception_item_text_clip,reception_item_value_origin,management_table_container_style,management_table_row_body};
use crate::table::{management_readonly_table_content_width,management_readonly_table_row_advance};
use crate::table::{management_table_data_width};
use crate::table::{virtual_fixed_rows,management_fixed_table_scroll_area,management_table_scroll_track_geometry,management_remaining_predivider_gap};

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

#[cfg(test)]
#[test]
fn management_table_outline_keeps_header_width_and_viewport_bottom(){
    let header=Rect::from_min_max(egui::pos2(20.0,40.0),egui::pos2(420.0,76.0));
    let body=Rect::from_min_max(egui::pos2(20.0,76.0),egui::pos2(410.0,300.0));
    assert_eq!(crate::table::outline_bounds(header,body),
        Rect::from_min_max(header.min,egui::pos2(420.0,300.0)));
    assert_eq!(crate::table::outline_bounds(header,header),header);
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
    fn read_only_table_row_advance_includes_divider_inside_compact_row(){
        assert_eq!(
            management_readonly_table_row_advance(),
            theme::MANAGEMENT_COMPACT_ROW_HEIGHT
        );
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
                let original=reception_freight_money_readonly(ui,&mut readonly,false);
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
                    ui,cell,&mut text,false,None
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
mod shared_ui_base_113_contract_tests {
    use super::*;
    #[test]
    fn managed_table_screens_use_shared_scroll_constructor(){
        for (name,source) in [
            ("product",include_str!("../product_manager.rs")),
            ("client",include_str!("../client_manager.rs")),
            ("client chooser",include_str!("../client_chooser.rs")),
            ("excel",include_str!("../excel_manager.rs")),
            ("settlement",include_str!("../daily_settlement_manager.rs")),
            ("history",include_str!("../order_history_manager.rs")),
        ]{
            let production=production_source_for_contract(source);
            assert!(["crate::table::fixed_rows(","crate::table::variable_rows(",
                "crate::table::fixed_rows_snapshot(","crate::table::variable_rows_snapshot("]
                .iter().any(|call|production.contains(*call)),"{name}");
            assert!(!production.contains("egui::ScrollArea::both()"),"{name}: local table scroll");
        }
    }
    #[test]
    fn all_live_table_surfaces_use_common_container(){
        for (name,source,entry) in [
            ("client chooser",include_str!("../client_chooser.rs"),"crate::table::show("),
            ("client",include_str!("../client_manager.rs"),"crate::table::show("),
            ("product",include_str!("../product_manager.rs"),"crate::table::show("),
            ("history",include_str!("../order_history_manager.rs"),"crate::table::show("),
            ("settlement",include_str!("../daily_settlement_manager.rs"),"crate::table::show("),
            ("excel",include_str!("../excel_manager.rs"),"crate::table::show("),
            ("brand",include_str!("../brand_manager.rs"),"crate::table::registry_table("),
            ("alias",include_str!("../alias_manager.rs"),"crate::table::registry_table("),
            ("model",include_str!("../model_group_manager.rs"),"crate::table::registry_table("),
            ("freight",include_str!("../freight_manager.rs"),"crate::table::show("),
            ("workflow",include_str!("../workflow_board_ui.rs"),"crate::table::inline("),
            ("reception",include_str!("../main.rs"),"crate::table::inline("),
            ("manual",include_str!("../manual_workspace.rs"),"crate::table::inline("),
        ]{
            assert!(source.contains(entry),"{name}: missing common table container");
            assert!(!source.contains("::finish_reception_items_table("),"{name}: local table finalization");
        }
    }
    #[test]
    fn registry_tables_share_fixed_column_geometry(){
        let shared=include_str!("mod.rs");
        let registry=shared.split("pub(crate) fn registry_table<").nth(1).unwrap()
            .split("pub(crate) enum InlineHeader").next().unwrap();
        for required in ["crate::table::show(","crate::table::header_columns(",
            "crate::table::fixed_rows(","crate::table::fixed_row(",
            "crate::table::paint_management_text_cell("]{
            assert!(registry.contains(required),"{required}");
        }
        for source in [include_str!("../brand_manager.rs"),include_str!("../alias_manager.rs"),
            include_str!("../model_group_manager.rs")]{
            let production=production_source_for_contract(source);
            assert!(production.contains("crate::table::registry_table("));
            assert!(!production.contains("crate::table::register_rows("));
        }
    }
    #[test]
    fn standard_text_cells_and_history_header_use_shared_painters(){
        for source in [include_str!("../product_manager.rs"),include_str!("../excel_manager.rs"),
            include_str!("../daily_settlement_manager.rs")]{
            assert!(production_source_for_contract(source).contains("crate::table::paint_management_text_cell("));
        }
        for source in [include_str!("../order_history_manager.rs"),include_str!("../excel_manager.rs"),
            include_str!("../daily_settlement_manager.rs")]{
            let production=production_source_for_contract(source);
            assert!(production.contains("crate::table::header_columns("));
            assert!(!production.contains("components::management_master_header_finish("));
        }
        let history=production_source_for_contract(include_str!("../order_history_manager.rs"));
        assert!(history.contains("crate::table::paint_table_galley("));
    }
}
