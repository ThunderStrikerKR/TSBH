//! Shared native input construction, formatting and exact-cell editing.
use eframe::egui::{self,Align,Layout,Rect,Response,Stroke,Ui,Vec2};
use crate::{source_highlight,theme,typography};
use crate::button::{date_navigation_segment_button,DateNavigationSegmentGlyph};
use crate::ui_primitives::{navigation_segment_child,date_navigation_segmented_surface};
#[cfg(test)]
use crate::ui_test_support::test_context;

/// Placeholder ink is independent of the actual input value and general UI text.
pub(crate) const INPUT_PLACEHOLDER_COLOR:egui::Color32=
    theme::text_ink(theme::TextRole::Value,theme::TextState::Placeholder);

fn input_placeholder(text:&str)->egui::RichText{
    egui::RichText::new(text).color(INPUT_PLACEHOLDER_COLOR)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FieldKind {
    #[cfg(test)]
    Text,
    #[cfg(test)]
    Telephone,
    #[cfg(test)]
    Quantity,
    Money,
    #[cfg(test)]
    Address,
}

impl FieldKind {
    pub(crate) fn preferred_width(self) -> f32 {
        match self {
            #[cfg(test)]
            Self::Text => 180.0,
            #[cfg(test)]
            Self::Telephone => 142.0,
            #[cfg(test)]
            Self::Quantity => 68.0,
            Self::Money => 112.0,
            #[cfg(test)]
            Self::Address => 320.0,
        }
    }
}

/// Shared construction for single-line text and numeric editors. Wrappers
/// retain their semantic font, allocation, focus and business behavior.
pub(crate) fn singleline_editor<'a>(
    value:&'a mut String,font:impl Into<egui::FontSelection>,width:f32,
    align:Align,margin:egui::Margin
)->egui::TextEdit<'a>{
    egui::TextEdit::singleline(value).font(font)
        .horizontal_align(align).vertical_align(Align::Center)
        .margin(margin).desired_width(width)
}

/// Allocate a native single-line input in the current layout. A permission
/// scope is opt-in: direct/read-only slots must not gain a child UI ID or
/// a different cursor advance merely by sharing the allocator.
pub(crate) fn allocate_singleline_field(
    ui:&mut Ui,size:Vec2,edit:egui::TextEdit<'_>,enabled:Option<bool>
)->Response{
    let place=|slot:&mut Ui|slot.add_sized(size,edit);
    let response=match enabled{
        Some(enabled)=>ui.add_enabled_ui(enabled,place).inner,
        None=>place(ui),
    };
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("input",response.rect,ui.clip_rect(),Some(size.y));
    response
}

/// A shared tag editor: text stays in the input until the explicit Add action.
pub(crate) struct TagBlockEdit{
    pub changed:bool,
    pub add:bool,
    pub remove:Option<String>,
}
pub(crate) fn tag_block_editor(ui:&mut Ui,tags:&[String],input:&mut String,hint:&str)->TagBlockEdit{
    let mut remove=None;
    if !tags.is_empty(){
        // Start-align fixed-height chips so the first sibling cannot be
        // repositioned when a later sibling expands the wrapping row.
        ui.scope_builder(egui::UiBuilder::new()
            .layout(Layout::left_to_right(Align::Min).with_main_wrap(true)),|chips|{
            crate::ui_layout::apply_spacing_role(chips,crate::ui_layout::SpacingRole::TagChips);
            for tag in tags{
                if crate::popup::removable_tag_chip(chips,tag).clicked(){
                    remove=Some(tag.clone());
                }
            }
        });
    }
    let (changed,add)=ui.horizontal(|entry|{
        crate::ui_layout::apply_spacing_role(entry,crate::ui_layout::SpacingRole::Toolbar);
        let width=(entry.available_width()-theme::INPUT_HEIGHT*2.5-theme::SPACE_8).max(60.0);
        let response=flexible_text_field(entry,input,width,hint);
        let add=crate::button::action(entry,"추가",theme::ButtonRole::Standard,
            !input.trim().is_empty()).clicked();
        (response.changed(),add)
    }).inner;
    TagBlockEdit{changed,add,remove}
}

/// Standard form width policy; compact rate fields retain their 1pt floor.
fn form_field_width(ui:&Ui,preferred:f32)->f32{
    preferred.min(ui.available_width().max(60.0))
}

/// Short body-text input (for rates and percentages): same caret, selection,
/// margins and row height as every form editor. Font remains proportional,
/// so the independent Consolas +2.5pt data-value tweak is not involved.
pub(crate) fn compact_text_field(
    ui:&mut Ui,value:&mut String,width:f32,enabled:bool
)->Response{
    let width=width.min(ui.available_width().max(1.0)).max(1.0);
    let edit=singleline_editor(value,egui::TextStyle::Body,width,Align::LEFT,
        typography::form_input_text_margin());
    allocate_singleline_field(ui,Vec2::new(width,theme::INPUT_HEIGHT),edit,Some(enabled))
}

/// Uses the same height, font and focus styling for every single-line field.
/// Disabled preview controls cannot edit business data.
pub(crate) fn field(ui: &mut Ui, value: &mut String, kind: FieldKind, enabled: bool) -> Response {
    field_with_width(ui,value,kind,enabled,kind.preferred_width())
}

fn field_with_width(ui:&mut Ui,value:&mut String,kind:FieldKind,enabled:bool,preferred:f32)->Response{
    let width=form_field_width(ui,preferred);
    let semantic=match kind {
        FieldKind::Money=>Some(typography::DataValueKind::Money),
        #[cfg(test)]
        FieldKind::Quantity=>Some(typography::DataValueKind::Quantity),
        #[cfg(test)]
        FieldKind::Telephone=>Some(typography::DataValueKind::Phone),
        #[cfg(test)]
        _=>None,
    };
    let editor_font=semantic.map_or(
        egui::FontSelection::Style(egui::TextStyle::Body),
        |role|egui::FontSelection::FontId(
            typography::data_value_font(role,typography::DataValueContext::Form)
        )
    );
    let field=singleline_editor(value,editor_font,width,
        semantic.map_or(Align::LEFT,typography::data_value_align),
        typography::form_input_text_margin());
    allocate_singleline_field(ui,Vec2::new(width,theme::INPUT_HEIGHT),field,Some(enabled))
}

/// UI-only money input: keep the 36pt shared money field and semantic font.
/// Thousands separators are visible during typing, not only after blur.
/// The native business command remains responsible for validating saved values.
pub(crate) fn parse_money_input(raw:&str)->Option<i64>{
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
pub(crate) fn money_field(ui:&mut Ui,value:&mut String,enabled:bool)->Response {
    let before=value.clone();
    let response=field(ui,value,FieldKind::Money,enabled);
    normalize_money_edit(ui,value,&before,&response);
    response
}

/// Explicit-width money field for forms that align unlike control types.
/// Formatting, typography and focus behavior are identical to money_field.
pub(crate) fn money_field_sized(ui:&mut Ui,value:&mut String,width:f32,enabled:bool)->Response{
    let before=value.clone();
    let response=field_with_width(ui,value,FieldKind::Money,enabled,width);
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
pub(crate) fn money_edit_text(amount:i64)->String{
    formatted_money_input(&amount.to_string()).unwrap_or_else(||amount.to_string())
}

// Reception money uses a fixed gap, independent of the numeric glyphs.
pub(crate) const RECEPTION_MONEY_UNIT_GAP:f32=theme::SPACE_4;
pub(crate) const RECEPTION_MONEY_PENDING:&str="—";

pub(crate) fn money_unit_left(right:f32,right_inset:f32,unit_width:f32)->f32{
    right-right_inset-unit_width
}

fn money_unit_margin(
    ui:&Ui,unit:&str,font:egui::FontId,right_inset:f32,unit_gap:f32,
    mut margin:egui::Margin
)->(egui::Margin,std::sync::Arc<egui::Galley>){
    let galley=ui.painter().layout_no_wrap(unit.to_owned(),font,theme::text_ink(theme::TextRole::Value,theme::TextState::Normal));
    margin.right=(galley.size().x+right_inset+unit_gap).ceil().clamp(0.0,127.0) as i8;
    margin.top=2;
    margin.bottom=2;
    (margin,galley)
}

fn paint_money_unit(ui:&Ui,rect:Rect,galley:std::sync::Arc<egui::Galley>,right_inset:f32){
    let pos=egui::pos2(money_unit_left(rect.right(),right_inset,galley.size().x),
        rect.center().y-galley.size().y/2.0);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(pos,galley,theme::text_ink(theme::TextRole::Value,theme::TextState::Normal));
}

/// Shared money widget construction. Both allocated form fields and
/// exact-rectangle worksheet overlays use the same text and suffix geometry.
fn money_text_edit_with_unit<'a>(
    ui:&Ui,value:&'a mut String,font:egui::FontId,width:f32,
    unit:&str,right_inset:f32,unit_gap:f32,margin:egui::Margin
)->(egui::TextEdit<'a>,std::sync::Arc<egui::Galley>){
    let (margin,galley)=money_unit_margin(ui,unit,font.clone(),right_inset,unit_gap,margin);
    let edit=singleline_editor(value,font,width,Align::RIGHT,margin);
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
pub(crate) fn money_field_with_unit(
    ui:&mut Ui,value:&mut String,enabled:bool,
    unit:&str,unit_right_inset:f32
)->Response{
    let before=value.clone();
    let width=form_field_width(ui,FieldKind::Money.preferred_width());
    let (edit,unit_galley)=money_text_edit_with_unit(
        ui,value,typography::numeric_editor_font(),width,unit,unit_right_inset,
        0.0,typography::form_input_text_margin()
    );
    let response=allocate_singleline_field(ui,Vec2::new(width,theme::INPUT_HEIGHT),edit,Some(enabled));
    normalize_money_edit(ui,value,&before,&response);
    paint_money_unit(ui,response.rect,unit_galley,unit_right_inset);
    response
}

/// Shared single-line free-text editor for inline manual cells and search.
/// Uses the same font, internal margin and height as the standard field role.
pub(crate) fn flexible_text_field(
    ui:&mut Ui, value:&mut String, width:f32, placeholder:&str
)->Response {
    let width=form_field_width(ui,width);
    let edit=singleline_editor(value,egui::TextStyle::Body,width,Align::LEFT,
        typography::form_input_text_margin()).hint_text(input_placeholder(placeholder));
    allocate_singleline_field(ui,Vec2::new(width,theme::INPUT_HEIGHT),edit,None)
}

/// Shared date text field used inside the management date-group master.
/// The caller owns navigation, parsing and validation; this component owns
/// the centered, frameless editor, proportional date face and caret margin.
pub(crate) fn management_date_text_field(
    ui:&mut Ui,value:&mut String,width:f32,height:f32,hint:&str
)->Response{
    management_date_text_field_impl(ui,value,width,height,hint,None)
}

pub(crate) fn management_date_text_field_impl(
    ui:&mut Ui,value:&mut String,width:f32,height:f32,hint:&str,limit:Option<usize>
)->Response{
    let mut edit=singleline_editor(value,typography::data_value_font(
            typography::DataValueKind::Date,typography::DataValueContext::Form
        ),width,Align::Center,typography::management_date_text_margin())
        .frame(false).hint_text(input_placeholder(hint));
    if let Some(limit)=limit{edit=edit.char_limit(limit);}
    allocate_singleline_field(ui,Vec2::new(width,height),edit,None)
}

#[cfg(test)]
mod shared_singleline_allocation_tests{
    use super::*;

    #[test]
    fn shared_allocation_preserves_native_id_geometry_and_permission_scope(){
        for permission in [None,Some(true),Some(false)]{
            let ctx=test_context();
            let mut value="12,345".to_owned();
            let mut results=Vec::new();
            for shared in [false,true]{
                let _=ctx.run(egui::RawInput::default(),|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        let size=Vec2::new(112.0,theme::INPUT_HEIGHT);
                        let edit=singleline_editor(&mut value,typography::numeric_editor_font(),
                            size.x,Align::RIGHT,typography::form_input_text_margin());
                        let response=if shared{
                            allocate_singleline_field(ui,size,edit,permission)
                        }else if let Some(enabled)=permission{
                            ui.add_enabled_ui(enabled,|slot|slot.add_sized(size,edit)).inner
                        }else{
                            ui.add_sized(size,edit)
                        };
                        results.push((response.id,response.rect,response.enabled(),
                            ui.available_rect_before_wrap().min));
                    });
                });
            }
            assert_eq!(results[0],results[1]);
            assert_eq!(results[1].1.height(),theme::INPUT_HEIGHT);
            assert_eq!(results[1].2,permission.unwrap_or(true));
            assert_eq!(value,"12,345");
        }
    }
}

/// Single native multiline constructor. Each caller keeps its existing
/// sizing, frame, input permissions and layout options.
fn multiline_editor<'a>(
    value:&'a mut dyn egui::TextBuffer,width:f32
)->egui::TextEdit<'a>{
    egui::TextEdit::multiline(value).desired_width(width)
}

/// Shared multiline editor for long-form messages and management memos.
/// Screens provide only the existing width, row count and optional hint; the
/// native TextEdit retains its standard click, selection and keyboard behavior.
pub(crate) fn multiline_text_field(
    ui:&mut Ui,value:&mut String,enabled:bool,
    desired_width:f32,desired_rows:usize,hint:Option<&str>
)->Response{
    let mut editor=multiline_editor(value,desired_width)
        .desired_rows(desired_rows);
    if let Some(hint)=hint{
        editor=editor.hint_text(input_placeholder(hint));
    }
    ui.add_enabled(enabled,editor)
}

/// Shared exact-viewport multiline editor for content-specific text roles.
/// The caller may supply a dedicated font and inset without rebuilding the
/// native TextEdit or bypassing the common input layer.
pub(crate) fn multiline_text_field_sized(
    ui:&mut Ui,value:&mut String,enabled:bool,size:Vec2,
    font:egui::FontId,frame:bool,margin:egui::Margin,desired_rows:usize
)->Response{
    let editor=multiline_editor(value,size.x)
        .interactive(enabled)
        .font(font)
        .frame(frame)
        .margin(margin)
        .desired_rows(desired_rows);
    ui.add_sized(size,editor)
}

/// Source-document text styling is shared while the caller supplies its
/// custom layout job and immutable or editable buffer.
pub(crate) fn reception_source_text_edit<'a>(
    buffer:&'a mut dyn egui::TextBuffer,width:f32
)->egui::TextEdit<'a>{
    multiline_editor(buffer,width).frame(false)
        .margin(egui::Margin::same(theme::SOURCE_DOCUMENT_TEXT_MARGIN))
        .font(egui::FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT))
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
pub(crate) fn management_search_field(
    ui:&mut Ui,text:&mut String,width:f32,placeholder:&str
)->Response {
    let response=flexible_text_field(ui,text,width,placeholder);
    management_search_append_click(&response,ui,text);
    crate::view_state::remember_search_focus(&response);
    response
}

/// The READ and EDIT renderers of recipient fields must use exactly one
/// typography/padding definition. Contact numbers use the same 16pt regular
/// proportional UI family as name, address, message and source text.
pub(crate) fn reception_recipient_text_edit(
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
    let input=singleline_editor(value,font,width,Align::LEFT,
        typography::form_input_text_margin());
    if active {
        // TextEdit's native background uses a different square/filled
        // geometry. Let the shared evidence surface own the rounded fill
        // and the outer caller own the common 2pt border instead.
        input.frame(false).text_color(source_highlight::ACTIVE_TEXT)
    }else{input}
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
            let range=egui::text::CCursorRange::one(cursor);
            output.state.cursor.set_char_range(Some(range));
            // The TextEditOutput still owns its state here. Capture the
            // newly resolved click caret BEFORE store consumes TextEditState.
            output.cursor_range=Some(range);
            crate::floating_notice::text_edit_caret(ui.ctx(),"reception.cell",&output);
            output.state.store(ui.ctx(),output.response.id);
            ui.ctx().request_repaint();
            return output.response;
        }
    }
    crate::floating_notice::text_edit_caret(ui.ctx(),"reception.cell",&output);
    output.response
}

/// Edit over the original 36pt input rectangle without shrinking it.
/// Ui::place never advances the parent layout cursor (Ui::put does).
/// The readonly control and focused control share the same builder, so the text
/// baseline, inset and phone font do not change when focus is acquired.
pub(crate) fn reception_recipient_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>,phone_number:bool,active:bool
)->Response{
    reception_text_editor_at_click(
        ui,cell,
        reception_recipient_text_edit(value,cell.width(),active,phone_number),
        request_focus,click_at
    )
}

/// Actual editor rendered IN the already allocated input cell; no dialog,
/// detached form, or second layout row. Focus and response are owned by egui.
pub(crate) fn reception_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>
)->Response{
    let rect=cell.shrink2(egui::vec2(theme::SPACE_4,theme::SPACE_2));
    let font=egui::FontId::proportional(typography::FONT_RECEPTION_RECIPIENT_TEXT);
    reception_text_editor_at_click(
        ui,rect,
        singleline_editor(value,font,rect.width(),
            Align::LEFT,typography::reception_cell_text_margin()),
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
        RECEPTION_MONEY_UNIT_GAP,typography::reception_cell_text_margin()
    );
    let response=reception_text_editor_at_click(
        ui,rect,edit,request_focus,click_at
    );
    normalize_money_edit(ui,value,&before,&response);
    paint_money_unit(ui,response.rect,unit_galley,inset);
    response
}

/// The unit occupies the former external label's width, keeping the
/// overall freight/COD group stable. Resting and focused money controls
/// use identical suffix geometry; only their interaction mode differs.
pub(crate) const RECEPTION_FREIGHT_MONEY_WIDTH:f32=104.0;

pub(crate) fn reception_freight_money_readonly(ui:&mut Ui,value:&mut String,exempt:bool)->Response{
    if exempt{
        // Exemption is a billing status, not a numeric amount to display.
        let mut label="면제".to_owned();
        let edit=singleline_editor(&mut label,reception_money_font(),
            RECEPTION_FREIGHT_MONEY_WIDTH,Align::Center,typography::reception_cell_text_margin());
        return allocate_singleline_field(
            ui,Vec2::new(RECEPTION_FREIGHT_MONEY_WIDTH,theme::INPUT_HEIGHT),
            edit.interactive(false),None
        );
    }
    let inset=theme::SPACE_8;
    let (edit,unit_galley)=money_text_edit_with_unit(
        ui,value,reception_money_font(),RECEPTION_FREIGHT_MONEY_WIDTH,
        "원",inset,RECEPTION_MONEY_UNIT_GAP,typography::reception_cell_text_margin()
    );
    let response=allocate_singleline_field(
        ui,Vec2::new(RECEPTION_FREIGHT_MONEY_WIDTH,theme::INPUT_HEIGHT),
        edit.interactive(false),None
    );
    paint_money_unit(ui,response.rect,unit_galley,inset);
    response
}

pub(crate) fn reception_freight_inline_cell_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>
)->Response{
    reception_money_editor_at_rect(ui,cell,value,request_focus,click_at)
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

pub(crate) struct ReceptionQuantityEditorResponse{
    pub(crate) input:Response,
    pub(crate) step_interaction:bool,
}

fn reception_quantity_editor(
    ui:&mut Ui,cell:Rect,value:&mut String,request_focus:bool,
    click_at:Option<egui::Pos2>,maximum:i64
)->ReceptionQuantityEditorResponse{
    let rect=cell.shrink2(egui::vec2(theme::SPACE_2,theme::SPACE_2));
    let mut placed=navigation_segment_child(
        ui,"reception-quantity-editor",rect,Layout::top_down(Align::Min)
    );
    let font=typography::data_value_font(
        typography::DataValueKind::Quantity,
        typography::DataValueContext::Reception
    );
    let decrease=reception_quantity_step(value,-1);
    let increase=reception_quantity_step(value,1)
        .filter(|next|next.parse::<i64>().is_ok_and(|n|n<=maximum));
    let (_, (input,step_interaction))=date_navigation_segmented_surface(
        &mut placed,"quantity-stepper",rect.size(),|segments,inner|{
            // Left arrow pair; right-aligned value and permanent unit.
            let arrow_width=16.0;
            let unit_width=segments.painter().layout_no_wrap(
                "개".to_owned(),font.clone(),theme::text_ink(theme::TextRole::Value,theme::TextState::Normal)
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
            let mut input_ui=navigation_segment_child(
                segments,"quantity-value",input_rect,Layout::top_down(Align::Min)
            );
            let input=reception_text_editor_at_click(
                &mut input_ui,input_rect,
                singleline_editor(value,font.clone(),input_rect.width(),
                    Align::RIGHT,egui::Margin::ZERO).frame(false),
                request_focus,click_at
            );
            segments.painter().with_clip_rect(unit_rect).text(
                unit_rect.center(),egui::Align2::CENTER_CENTER,"개",font.clone(),theme::text_ink(theme::TextRole::Value,theme::TextState::Normal)
            );
            let mut clicked=false;
            let half=arrow_rect.height()/2.0;
            for (index,next,up) in [
                (0,increase,true),
                (1,decrease,false),
            ]{
                let part=Rect::from_min_size(
                    egui::pos2(arrow_rect.left(),arrow_rect.top()+index as f32*half),
                    egui::vec2(arrow_rect.width(),half)
                );
                let mut button=navigation_segment_child(
                    segments,("quantity-arrow",index),part,Layout::left_to_right(Align::Center)
                );
                button.spacing_mut().item_spacing=Vec2::ZERO;
                // Do not let the standard button's minimum padding enlarge
                // the half-height arrow slots.
                button.spacing_mut().interact_size=Vec2::ZERO;
                button.spacing_mut().button_padding=Vec2::ZERO;
                let response=date_navigation_segment_button(
                    &mut button,arrow_rect.width(),next.is_some(),
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
        crate::highlight::paint_outline(placed.painter(),rect,theme::CONTROL_RADIUS,crate::highlight::Role::Focus);
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

/// Form wrapper for the same numeric stepper used by reception cells.
pub(crate) fn bounded_quantity_field(
    ui:&mut Ui,value:&mut String,enabled:bool,maximum:i64
){
    let previous=value.clone();
    ui.add_enabled_ui(enabled,|input|{
        let (rect,_)=input.allocate_exact_size(
            Vec2::new(112.0,theme::INPUT_HEIGHT),egui::Sense::hover()
        );
        reception_quantity_editor(input,rect,value,false,None,maximum);
    });
    if !value.is_empty() && !value.parse::<i64>().is_ok_and(|n|(1..=maximum).contains(&n)){
        *value=previous;
    }
}

#[derive(Clone,Copy)]
pub(crate) enum ReceptionNumericCell{Quantity,Money}

/// A stepper click may temporarily move the text focus. Both consumers
/// must retain the draft until that click resolves.
pub(crate) fn reception_numeric_edit_finished(response:&Response,step_interaction:bool)->bool{
    crate::cell_navigation::worksheet_blur_finished(response,step_interaction)
}

pub(crate) fn reception_item_numeric_editor(
    ui:&mut Ui,rect:Rect,value:&mut String,focus:bool,
    click_at:Option<egui::Pos2>,kind:ReceptionNumericCell
)->(Response,bool){
    match kind{
        ReceptionNumericCell::Quantity=>{
            let editor=reception_quantity_editor(ui,rect,value,focus,click_at,RECEPTION_QUANTITY_MAX);
            (editor.input,editor.step_interaction)
        }
        ReceptionNumericCell::Money=>(
            reception_money_inline_cell_editor(ui,rect,value,focus,click_at),false
        )
    }
}

/// Persistent required-input accent. Paint on the already allocated cell,
/// after any inline editor and blue/orange hover decoration. No layout space.
pub(crate) fn paint_required_input(ui:&Ui,rect:Rect){
    crate::highlight::paint(ui,rect,theme::CONTROL_RADIUS,crate::highlight::Role::RequiredInput);
}

// Shared controls and presentation profiles.
/// Read-only TextEdit does not reliably own the egui hover response.
/// One shared hit target now owns field hover, I-beam cursor and inline-click
/// handoff for both recipient details and read-only workflow views.
pub(crate) fn reception_recipient_value_hit(ui:&mut Ui,display:&Response)->Response{
    let hit=ui.interact(
        display.rect,
        display.id.with("recipient-value-hit"),
        egui::Sense::click()
    );
    if hit.hovered(){
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    hit
}

/// OCR image zoom uses a fixed control width and a separate formatted label.
/// Keep the native slider's keyboard and drag behavior.
pub(crate) fn ocr_zoom_slider(ui:&mut Ui,zoom:&mut f32)->Response{
    ui.add_sized(
        Vec2::new(118.0,24.0),
        egui::Slider::new(zoom,0.5..=4.0).show_value(false)
    )
}

#[cfg(test)]
mod regression_tests;
