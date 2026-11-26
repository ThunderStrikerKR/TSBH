//! Shared reception forms, item and freight rows, actions and totals.
use eframe::egui::{self,Align,Layout,Rect,Response,Sense,Stroke,Ui,Vec2};
use crate::{theme,typography,source_highlight};
use crate::ui_primitives::{text_exceeds_rect,overflow_tooltip};
use crate::input::{reception_recipient_text_edit,allocate_singleline_field,reception_recipient_value_hit,money_edit_text,reception_freight_money_readonly,money_unit_left,RECEPTION_MONEY_PENDING,RECEPTION_MONEY_UNIT_GAP};
use crate::table::{management_table_data_width,reception_item_columns,reception_item_text_clip,paint_management_table_row_rule};
use crate::scrollbar::{scroll_area,ScrollAxes};
use crate::popup::{show_decision_window,decision_content};
use crate::ui_layout::fixed_status_slot;
use crate::checkbox::checkbox;
use crate::dropdown::reception_cod_dropdown;
use crate::display_format::display_money_number;
use crate::text_fit::{cached_reception_product,reception_compact_quantity,reception_pending_money_marker};

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
        // Pending monetary markers are not digits; preserve the approved optical correction.
        let y_adjust=if index==2 && reception_pending_money_marker(label){
            -typography::DATA_NUMERIC_GLYPH_OFFSET_Y
        }else{0.0};
        item_overflow[index]=crate::table::paint_editable_cell_text(
            ui,value_clip,label,text,font,ink,right,y_adjust);
    }
    // Preserve the five-source-field highlighting contract: these three
    // hover responses still map to the exact Rust product/quantity/price keys.
    // Use a distinct id for each column in each row. next_auto_id()
    // alone would not necessarily advance for manually painted cells.
    let row_id = (rect.top().to_bits(), rect.left().to_bits());
    let product=crate::table::editable_cell_hit(ui,cells[0],
        ui.make_persistent_id(("item-cell",row_id,0)),editable,
        egui::CursorIcon::PointingHand,item_overflow[0],name);
    let quantity=crate::table::editable_cell_hit(ui,cells[1],
        ui.make_persistent_id(("item-cell",row_id,1)),editable,
        egui::CursorIcon::Text,item_overflow[1],quantity);
    let amount=crate::table::editable_cell_hit(ui,cells[2],
        ui.make_persistent_id(("item-cell",row_id,2)),editable,
        egui::CursorIcon::Text,item_overflow[2],amount);
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
        let action=crate::button::remove_at(ui,hit,ui.make_persistent_id(("item-remove",row_id)),enabled);
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

pub(crate) fn freight_actions_need_second_row(available_width:f32)->bool{
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
