//! Visual projection of authoritative Rust recognition source spans.
//! The source recognizer reports UTF-16 offsets for TypeScript parity;
//! egui TextEdit and LayoutJob require UTF-8 byte ranges / Unicode char cursors.
//! Never match repeated names by guessing text position or infer ownership.

use eframe::egui::{self, Color32, FontId};
use serde_json::Value;
use std::{collections::HashSet,time::{Duration, Instant}};
use crate::theme;

// Source-document linkage: 50ms enter / 150ms release.
// This is distinct from the 250ms working-order foreground preview.
pub const HOVER_ENTER: Duration = Duration::from_millis(50);
pub const HOVER_EXIT: Duration = Duration::from_millis(150);
const MAX_SPANS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkKey {
    pub recipient_id: String,
    pub target_id: String,
    pub field: String,
}

#[derive(Clone, Debug)]
pub struct LinkedSpan {
    pub start: usize, // UTF-8 byte
    pub end: usize,
    pub start_char: usize, // egui character cursor
    pub end_char: usize,
    pub key: LinkKey,
    /// Manual-classification source block ID; ordinary recognition spans
    /// leave this empty. The visual component still owns hit testing.
    pub source_id: Option<String>,
}

#[derive(Default)]
pub struct HoverState {
    /// An evidence key is meaningful only inside its source order.
    order_id: Option<String>,
    pending: Option<LinkKey>,
    pending_at: Option<Instant>,
    pub active: Option<LinkKey>,
    release_at: Option<Instant>,
    pub scroll_to: Option<LinkKey>,
}

impl HoverState {
    pub fn clear(&mut self) { *self=Self::default(); }

    /// One order-scoped hover state is shared by source text, recipient
    /// inputs, item cells and workflow cards, including read-only previews.
    /// A stale hover from another receipt cannot select an identically
    /// numbered recipient on the new receipt.
    pub fn update_for(&mut self, order_id:&str, incoming:Option<LinkKey>,
                      ctx:&egui::Context) {
        if self.order_id.as_deref()!=Some(order_id) {
            self.clear();
            self.order_id=Some(order_id.to_owned());
        }
        self.update(incoming,ctx);
    }

    pub fn active_for(&self, order_id:&str)->Option<&LinkKey>{
        (self.order_id.as_deref()==Some(order_id))
            .then_some(self.active.as_ref()).flatten()
    }

    pub fn scoped_active(&self)->Option<(&str,&LinkKey)> {
        self.order_id.as_deref().zip(self.active.as_ref())
    }

    /// The caller passes exactly one hover target per frame. A hover on an
    /// unrelated field cancels any pending entry to prevent stale focus.
    pub fn update(&mut self, incoming: Option<LinkKey>, ctx:&egui::Context) {
        let now=Instant::now();
        if incoming==self.active {
            self.pending=None;
            self.pending_at=None;
            self.release_at=None;
            return;
        }
        if let Some(key)=incoming {
            self.release_at=None;
            if self.pending.as_ref()!=Some(&key) {
                self.pending=Some(key);
                self.pending_at=Some(now);
            } else if self.pending_at.is_some_and(|time|
                now.duration_since(time)>=HOVER_ENTER
            ) {
                self.active=Some(key.clone());
                self.scroll_to=Some(key);
                self.pending=None;
                self.pending_at=None;
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(HOVER_ENTER);
            }
        } else {
            self.pending=None;
            self.pending_at=None;
            if self.active.is_some() {
                let expire=*self.release_at.get_or_insert(now+HOVER_EXIT);
                if now>=expire {
                    self.active=None;
                    self.release_at=None;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(HOVER_EXIT);
                }
            }
        }
    }
}

fn text<'a>(value:&'a Value,key:&str)->&'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Return byte and char index only on exact UTF-16 boundaries. This rejects
/// offsets that cut a supplementary-plane emoji or non-ASCII character.
pub(crate) fn boundary(raw:&str, units:usize)->Option<(usize,usize)> {
    let mut used=0;
    let mut chars=0;
    for (byte,ch) in raw.char_indices() {
        if used==units {return Some((byte,chars));}
        used+=ch.len_utf16();
        chars+=1;
        if used>units {return None;}
    }
    (used==units).then_some((raw.len(),chars))
}
fn add(raw:&str, owner:&str, span:&Value, out:&mut Vec<LinkedSpan>) {
    if out.len()>=MAX_SPANS {return;}
    let field=text(span,"field");
    let target=text(span,"targetId");
    if owner.is_empty() || target.is_empty() || field.is_empty() {return;}
    let (Some(start),Some(end))=(
        span.get("start").and_then(Value::as_u64),
        span.get("end").and_then(Value::as_u64),
    ) else {return};
    let (Some((start_byte,start_char)),Some((end_byte,end_char)))=(
        boundary(raw,start as usize),
        boundary(raw,end as usize),
    ) else {return};
    if end_byte<=start_byte {return;}
    if let Some(exact)=span.get("text").and_then(Value::as_str) {
        if &raw[start_byte..end_byte]!=exact{return;}
    }
    out.push(LinkedSpan {
        start:start_byte,end:end_byte,start_char,end_char,
        key:LinkKey {
            recipient_id:owner.into(),target_id:target.into(),field:field.into(),
        },
        source_id:None,
    });
}


// Match React's visual projection without changing Rust evidence or ownership.
// Every expansion remains anchored to an existing, exact native source span.
fn display_bounds(raw:&str, span:&mut LinkedSpan, start:usize, end:usize) {
    if start>=end || !raw.is_char_boundary(start) || !raw.is_char_boundary(end) { return; }
    span.start=start;
    span.end=end;
    span.start_char=raw[..start].chars().count();
    span.end_char=span.start_char+raw[start..end].chars().count();
}

fn overlaps_another_field(
    spans:&[LinkedSpan], model:&LinkedSpan, start:usize, end:usize
)->bool {
    spans.iter().any(|s|
        s.key!=model.key && s.start<end && start<s.end
    )
}

fn product_display_range(
    raw:&str, expression:&str, model:&LinkedSpan, spans:&[LinkedSpan]
)->Option<(usize,usize)> {
    if expression.trim().is_empty() || model.key.field!="product" { return None; }
    let line_start=raw[..model.start].rfind('\n').map_or(0,|i|i+1);
    let line_end=raw[model.end..].find('\n')
        .map_or(raw.len(),|i|model.end+i);
    if expression.contains('\n') { return None; }
    // The original React code allows only an exact, same-line occurrence
    // containing the already recognized model code, never a fuzzy match.
    let found=raw[line_start..line_end].match_indices(expression)
        .map(|(relative,_)|line_start+relative)
        .find(|&start|start<=model.start && model.end<=start+expression.len())?;
    let expression_end=found+expression.len();
    let quantity=spans.iter().find(|s|
        s.key.recipient_id==model.key.recipient_id &&
        s.key.target_id==model.key.target_id &&
        s.key.field=="quantity" &&
        s.start>=model.end && s.start<expression_end
    );
    // Without explicit quantity evidence do not silently absorb "4개".
    if quantity.is_none() {
        static QUANTITY:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
        let pattern=QUANTITY.get_or_init(||
            regex::Regex::new(r"(?i)(?:[0-9]+|한|두|세|네)\s*(?:개|대|ea|pcs)")
                .expect("static quantity display pattern")
        );
        if pattern.is_match(expression) { return None; }
    }
    let mut start=found;
    let mut end=quantity.map_or(expression_end,|s|s.start);
    while start<end {
        let ch=raw[start..end].chars().next()?;
        if !ch.is_whitespace() { break; }
        start+=ch.len_utf8();
    }
    while start<end {
        let ch=raw[start..end].chars().next_back()?;
        if !ch.is_whitespace() { break; }
        end-=ch.len_utf8();
    }
    if start>model.start || end<model.end ||
       overlaps_another_field(spans,model,start,end) ||
       (start==model.start && end==model.end) { return None; }
    Some((start,end))
}

fn note_display_start(raw:&str,span:&LinkedSpan)->Option<usize> {
    if span.key.field!="note" { return None; }
    let line_start=raw[..span.start].rfind('\n').map_or(0,|i|i+1);
    let prefix=&raw[line_start..span.start];
    // Preserve the legacy "메모 / 요청사항 / 배송 메시지" heading projection.
    static HEADING:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let pattern=HEADING.get_or_init(||
        regex::Regex::new(
            r"^\s*\(?(?:배송\s*)?(?:메세지|메시지|요청(?:사항)?|메모)(?:\s*[:：]\s*|\s+)$"
        ).expect("static note heading pattern")
    );
    if !pattern.is_match(prefix) { return None; }
    Some(line_start + prefix.find(|ch:char|!ch.is_whitespace()).unwrap_or(0))
}

/// Apply React's display-only range adjustments to exact Rust recognition
/// evidence. The source offsets, recipient IDs and order data remain native.
fn project_visual_ranges(raw:&str, recognition:&Value, spans:&mut [LinkedSpan]) {
    let Some(recipients)=recognition.get("recipients").and_then(Value::as_array)
        else{return};
    let original=spans.to_vec();
    for recipient in recipients {
        let owner=text(recipient,"id");
        for item in recipient.get("items").and_then(Value::as_array)
            .into_iter().flatten() {
            let item_id=text(item,"id");
            let Some(model)=original.iter().find(|span|
                span.key.recipient_id==owner &&
                span.key.target_id==item_id &&
                span.key.field=="product"
            ) else {continue};
            if let Some((start,end))=product_display_range(
                raw,text(item,"raw"),model,&original
            ) {
                for span in spans.iter_mut().filter(|span|
                    span.key==model.key && span.key.field=="product"
                ) {
                    display_bounds(raw,span,start,end);
                }
            }
        }
    }
    for span in spans.iter_mut().filter(|span|span.key.field=="note") {
        if let Some(start)=note_display_start(raw,span) {
            display_bounds(raw,span,start,span.end);
        }
    }
}

/// The Rust recognition output is the only source of location/recipient
/// association. Preserve it even when two recipients contain the same text.
pub fn collect(raw:&str, recognition:&Value)->Vec<LinkedSpan>{
    let mut spans=Vec::new();
    let Some(recipients)=recognition.get("recipients").and_then(Value::as_array)
        else{return spans};
    for recipient in recipients {
        let owner=text(recipient,"id");
        for span in recipient.get("sources").and_then(Value::as_array)
            .into_iter().flatten() {
            add(raw,owner,span,&mut spans);
        }
        for item in recipient.get("items").and_then(Value::as_array)
            .into_iter().flatten() {
            for span in item.get("sources").and_then(Value::as_array)
                .into_iter().flatten() {
                add(raw,owner,span,&mut spans);
            }
        }
    }
    // Do this once per analysis (including manual reclassification), never per frame.
    project_visual_ranges(raw,recognition,&mut spans);
    spans.sort_by_key(|span|(span.start, span.end,span.key.field.clone()));
    spans.dedup_by(|b,a|
        a.start==b.start && a.end==b.end && a.key==b.key);
    spans
}

/// Build the same linked-span projection for the editable manual worksheet.
/// Assigned blocks follow the employee's current recipient/item/field target;
/// unassigned blocks retain their original Rust-recognized target. No text
/// matching or ownership inference is performed in the UI.
pub fn collect_manual(
    raw:&str,rows:&[Value],blocks:&[Value],assignments:&Value
)->Vec<LinkedSpan>{
    let mut ordered=blocks.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|block|
        block.get("start").and_then(Value::as_u64).unwrap_or(u64::MAX)
    );
    let mut spans=Vec::new();
    let mut last_end=0usize;
    for block in ordered {
        if spans.len()>=MAX_SPANS {break;}
        let id=text(block,"id");
        let field=text(block,"field");
        let (Some(start),Some(end))=(
            block.get("start").and_then(Value::as_u64),
            block.get("end").and_then(Value::as_u64)
        )else{continue};
        let (Some((start_byte,start_char)),Some((end_byte,end_char)))=(
            boundary(raw,start as usize),boundary(raw,end as usize)
        )else{continue};
        if id.is_empty() || field.is_empty() || end_byte<=start_byte ||
            start_byte<last_end || &raw[start_byte..end_byte]!=text(block,"text") {
            continue;
        }
        let assignment=assignments.get(id).filter(|value|value.is_object());
        let (owner,target,linked_field)=if let Some(assignment)=assignment {
            let owner=text(assignment,"recipientId");
            let item=text(assignment,"itemId");
            let linked_field=text(assignment,"field");
            let target=if item.is_empty(){owner}else{item};
            (owner,target,linked_field)
        }else{
            let original_target=text(block,"targetId");
            let row=rows.iter().find(|row|
                text(row,"id")==original_target ||
                row.get("items").and_then(Value::as_array).is_some_and(|items|
                    items.iter().any(|item|text(item,"id")==original_target))
            );
            let owner=row.map(|row|text(row,"id")).unwrap_or(original_target);
            let is_item=row.is_some_and(|row|
                row.get("items").and_then(Value::as_array).is_some_and(|items|
                    items.iter().any(|item|text(item,"id")==original_target))
            );
            let target=if is_item{original_target}else{owner};
            (owner,target,field)
        };
        if owner.is_empty() || target.is_empty() || linked_field.is_empty(){continue;}
        spans.push(LinkedSpan{
            start:start_byte,end:end_byte,start_char,end_char,
            key:LinkKey{
                recipient_id:owner.to_owned(),
                target_id:target.to_owned(),
                field:linked_field.to_owned()
            },
            source_id:Some(id.to_owned())
        });
        last_end=end_byte;
    }
    spans
}

pub fn key(recipient:&str,target:&str,field:&str)->LinkKey {
    LinkKey{recipient_id:recipient.into(),target_id:target.into(),field:field.into()}
}

/// Shared evidence emphasis for source, recipient input values and item cells.
/// All three destinations use the same blue fill, text and four-sided inset
/// border. The source only projects native evidence; never alters ownership.
pub const ACTIVE_FILL:Color32=Color32::from_rgb(23,67,109);       // #17436d
pub const ACTIVE_TEXT:Color32=Color32::from_rgb(234,244,255);   // #eaf4ff
pub const ACTIVE_BORDER:Color32=Color32::from_rgb(117,181,255); // #75b5ff
pub const ACTIVE_RADIUS:f32=theme::SOURCE_BLOCK_RADIUS;
pub const ACTIVE_BORDER_WIDTH:f32=2.0;

pub fn is_active(active:Option<&LinkKey>, key:&LinkKey)->bool {
    active.is_some_and(|selection|same_target(selection,key))
}

/// Fill must be painted before the value text. For TextEdit use its
/// background_color() instead, so the native editor paints behind glyphs.
pub fn paint_active_background(ui:&egui::Ui,rect:egui::Rect) {
    ui.painter().rect_filled(rect,ACTIVE_RADIUS,ACTIVE_FILL);
}

/// Draw the same 2pt outline whenever the shared visible projection
/// marks a field active. A board-driven recipient preview and a span-driven
/// hover MUST produce exactly the same border, even when no LinkKey has
/// completed its 50ms entrance timer yet.
pub fn paint_active_if(ui:&egui::Ui, response:&egui::Response, highlighted:bool) {
    if highlighted {
        ui.painter().rect_stroke(
            response.rect.shrink(0.5),ACTIVE_RADIUS,
            egui::Stroke::new(ACTIVE_BORDER_WIDTH,ACTIVE_BORDER),
            egui::StrokeKind::Inside,
        );
    }
}

fn same_target(left:&LinkKey,right:&LinkKey)->bool {
    left.recipient_id==right.recipient_id &&
    left.target_id==right.target_id && left.field==right.field
}

/// Hit-test the visible glyph cells, NOT TextEdit's insertion cursor.
/// cursor_from_pos snaps to the next insertion index on the right half of
/// a character, making the final glyph of a source name lose its link.
/// Match the exact Rust evidence ownership, including wrapped/newline rows.
pub fn hovered_span_index_at_pointer(
    raw:&str,spans:&[LinkedSpan],galley:&egui::Galley,
    galley_pos:egui::Pos2,pointer:egui::Pos2
)->Option<usize>{
    let characters:Vec<char>=raw.chars().collect();
    let mut row_start=0usize;
    for (row_index,placed) in galley.rows.iter().enumerate(){
        let top=galley_pos.y+placed.pos.y;
        let glyphs=&placed.row.glyphs;
        if pointer.y>=top && pointer.y<top+placed.row.size.y {
            for (index,glyph) in glyphs.iter().enumerate(){
                if glyph.chr=='\n' {continue;}
                let left=galley_pos.x+placed.pos.x+glyph.pos.x;
                let right=left+glyph.advance_width;
                if pointer.x>=left && pointer.x<right {
                    let char_index=row_start+index;
                    return spans.iter().position(|span|
                        span.start_char<=char_index && char_index<span.end_char
                    );
                }
            }
            return None;
        }
        row_start+=glyphs.len();
        if row_index+1<galley.rows.len() &&
            characters.get(row_start)==Some(&'\n'){
            row_start+=1;
        }
    }
    None
}

pub fn hovered_key_at_pointer(
    raw:&str,spans:&[LinkedSpan],galley:&egui::Galley,
    galley_pos:egui::Pos2,pointer:egui::Pos2
)->Option<LinkKey>{
    hovered_span_index_at_pointer(raw,spans,galley,galley_pos,pointer)
        .and_then(|index|spans.get(index))
        .map(|span|span.key.clone())
}

// The source token palette is intentionally quiet in this dark theme.
// 16pt SUIT stays unchanged. Blocks reserve the inside stroke in addition
// to the approved 1.5pt horizontal and 0.5pt vertical visible clearance.
const SOURCE_BLOCK:Color32=Color32::from_rgb(66,66,66);        // #424242
const SOURCE_CONTEXT:Color32=Color32::from_rgb(73,73,73);      // #494949
const SOURCE_ACTIVE:Color32=ACTIVE_FILL;
const SOURCE_ACTIVE_TEXT:Color32=ACTIVE_TEXT;
const SOURCE_ACTIVE_BORDER:Color32=ACTIVE_BORDER;

#[derive(Clone,Debug)]
struct VisualBlock {
    start:usize,
    end:usize,
    start_char:usize,
    end_char:usize,
    key:LinkKey,
    fill:Color32,
    focused:bool,
    manual_state:Option<ManualBlockState>,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum ManualBlockState { Unassigned, Classified }

// Resolve overlapping evidence once per character interval, so two
// classifiers cannot paint competing fills over the same source text.
fn visual_blocks(
    raw:&str,spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>
)->Vec<VisualBlock> {
    visual_blocks_with_manual_states(raw,spans,selected_recipient,active,None)
}

/// Shared geometry with a manual-only classification palette. When states
/// are omitted (Reception), preserve the original evidence rendering.
fn visual_blocks_with_manual_states(
    raw:&str,spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>,assigned_ids:Option<&HashSet<String>>
)->Vec<VisualBlock> {
    let mut positions=Vec::with_capacity(spans.len()*2+2);
    positions.extend([0,raw.len()]);
    for span in spans {
        if span.start<span.end && span.end<=raw.len() &&
           raw.is_char_boundary(span.start) && raw.is_char_boundary(span.end) {
            positions.extend([span.start,span.end]);
        }
    }
    positions.sort_unstable();
    positions.dedup();
    let mut result:Vec<VisualBlock>=Vec::new();
    let mut char_cursor=0;
    for window in positions.windows(2) {
        let (start,end)=(window[0],window[1]);
        let start_char=char_cursor;
        char_cursor+=raw[start..end].chars().count();
        let mut visible=spans.iter().filter(|s|s.start<=start && s.end>=end);
        let focused=active.and_then(|key|visible.clone().find(|s|
            same_target(&s.key,key)));
        let contextual=visible.clone().find(|s|
            selected_recipient==Some(s.key.recipient_id.as_str()) ||
            active.is_some_and(|key|key.recipient_id==s.key.recipient_id));
        let Some(chosen)=focused.or(contextual).or_else(||visible.next()) else {
            continue;
        };
        let is_focused=focused.is_some();
        let is_context=selected_recipient==Some(chosen.key.recipient_id.as_str()) ||
            active.is_some_and(|key|key.recipient_id==chosen.key.recipient_id);
        let manual_state=assigned_ids.map(|ids| {
            if chosen.source_id.as_ref().is_some_and(|id|ids.contains(id)) {
                ManualBlockState::Classified
            }else{ManualBlockState::Unassigned}
        });
        let fill=if is_focused {SOURCE_ACTIVE}
            else {match manual_state {
                Some(ManualBlockState::Unassigned)=>theme::MANUAL_SOURCE_UNASSIGNED_FILL,
                Some(ManualBlockState::Classified)=>theme::MANUAL_SOURCE_CLASSIFIED_FILL,
                None if is_context=>SOURCE_CONTEXT,
                None=>SOURCE_BLOCK,
            }};
        if let Some(previous)=result.last_mut() {
            if previous.end==start && previous.key==chosen.key &&
               previous.fill==fill && previous.focused==is_focused &&
               previous.manual_state==manual_state {
                previous.end=end;
                previous.end_char=char_cursor;
                continue;
            }
        }
        result.push(VisualBlock {
            start,end,start_char,end_char:char_cursor,
            key:chosen.key.clone(),fill,focused:is_focused,manual_state,
        });
    }
    result
}

/// TextEdit handles selection, cursor, wrapping and source edits. The paint
/// layer below its glyphs owns the rounded visual blocks, not the layout job.
pub fn layout(
    raw:&str,
    spans:&[LinkedSpan],
    selected_recipient:Option<&str>,
    active:Option<&LinkKey>,
    width:f32,
    font:FontId,
    color:Color32,
)->egui::text::LayoutJob {
    layout_with_states(raw,spans,selected_recipient,active,None,width,font,color)
}

/// Used only by the manual source viewer, not by normal Reception.
pub fn layout_manual(
    raw:&str,spans:&[LinkedSpan],
    selected_recipient:Option<&str>,active:Option<&LinkKey>,
    assigned_ids:&HashSet<String>,width:f32,font:FontId,color:Color32
)->egui::text::LayoutJob{
    layout_with_states(
        raw,spans,selected_recipient,active,Some(assigned_ids),width,font,color
    )
}

fn layout_with_states(
    raw:&str,spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>,assigned_ids:Option<&HashSet<String>>,
    width:f32,font:FontId,color:Color32
)->egui::text::LayoutJob {
    let mut job=egui::text::LayoutJob::default();
    job.wrap.max_width=width;
    let mut cursor=0;
    for block in visual_blocks_with_manual_states(
        raw,spans,selected_recipient,active,assigned_ids
    ) {
        if cursor<block.start {
            job.append(&raw[cursor..block.start],0.0,egui::TextFormat {
                font_id:font.clone(),color,..Default::default()
            });
        }
        job.append(&raw[block.start..block.end],0.0,egui::TextFormat {
            font_id:font.clone(),
            color:if block.focused {SOURCE_ACTIVE_TEXT} else {
                match block.manual_state {
                    Some(ManualBlockState::Unassigned)=>theme::MANUAL_SOURCE_UNASSIGNED_TEXT,
                    Some(ManualBlockState::Classified)=>theme::MANUAL_SOURCE_CLASSIFIED_TEXT,
                    None=>color,
                }
            },
            ..Default::default()
        });
        cursor=block.end;
    }
    if cursor<raw.len() {
        job.append(&raw[cursor..],0.0,egui::TextFormat {
            font_id:font,color,..Default::default()
        });
    }
    job
}

/// Build a source chip around glyph ink. Padding measures the EMPTY space
/// between ink and the inner edge of the outline, not the outer rect edge.
/// The 1pt inside stroke must be reserved in addition to this padding.
fn compact_block_rect(
    text_left:f32,text_right:f32,ink_top:f32,ink_bottom:f32
)->Option<egui::Rect> {
    if !text_left.is_finite() || !text_right.is_finite()
        || !ink_top.is_finite() || !ink_bottom.is_finite()
        || text_right<=text_left || ink_bottom<=ink_top {
        return None;
    }
    let horizontal=theme::SOURCE_BLOCK_HORIZONTAL_PADDING
        +theme::SOURCE_RECOGNIZED_BORDER_WIDTH;
    let vertical=theme::SOURCE_BLOCK_VERTICAL_PADDING
        +theme::SOURCE_RECOGNIZED_BORDER_WIDTH;
    let left=text_left-horizontal;
    let right=text_right+horizontal;
    let top=ink_top-vertical;
    let bottom=ink_bottom+vertical;
    (right>left && bottom>top).then(||egui::Rect::from_min_max(
        egui::pos2(left,top),egui::pos2(right,bottom)
    ))
}

/// At boundaries between adjacent classified ranges, trim only their shared
/// edges to leave a 0.5pt gap, preserving full padding on open edges.
fn separate_touching_blocks(mut rect:egui::Rect,left:bool,right:bool)->egui::Rect {
    let trim=theme::SOURCE_BLOCK_HORIZONTAL_PADDING
        +theme::SOURCE_RECOGNIZED_BORDER_WIDTH
        +theme::SOURCE_BLOCK_ADJACENT_GAP*0.5;
    if left {rect.min.x+=trim;}
    if right {rect.max.x-=trim;}
    rect
}

/// Focus adds its extra stroke outside the normal chip so its thicker
/// outline cannot consume the restored ink clearance.
fn source_focused_block_rect(rect:egui::Rect)->egui::Rect{
    rect.expand((ACTIVE_BORDER_WIDTH-theme::SOURCE_RECOGNIZED_BORDER_WIDTH).max(0.0))
}

// Rounded blocks are painted beneath the editor glyphs, but the blue focus
/// border is painted over all blocks. In particular, a later gray token must
/// never hide the active blue field when adjacent token padding overlaps.
/// Glyph positions come from egui; source bytes and character cursors stay
/// untouched, including Korean text and wrapped addresses.
pub fn block_shapes(
    raw:&str,galley:&egui::Galley,galley_pos:egui::Pos2,
    spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>,clip:egui::Rect,
)->(Vec<egui::Shape>,Vec<egui::Shape>) {
    block_shapes_with_states(
        raw,galley,galley_pos,spans,selected_recipient,active,None,clip
    )
}

/// Evidence fill + 1pt outlined state for the manual classification popup.
/// Reception still calls block_shapes() without a manual-state override.
pub fn block_shapes_manual(
    raw:&str,galley:&egui::Galley,galley_pos:egui::Pos2,
    spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>,assigned_ids:&HashSet<String>,clip:egui::Rect,
)->(Vec<egui::Shape>,Vec<egui::Shape>) {
    block_shapes_with_states(
        raw,galley,galley_pos,spans,selected_recipient,active,Some(assigned_ids),clip
    )
}

/// Exact visual style and row segments for one native manual source block.
/// The geometry is shared with the normal source-block painter so a drag ghost
/// can be a true visual copy rather than a newly approximated text chip.
#[derive(Clone, Copy, Debug)]
pub struct ManualSourceBlockStyle {
    pub fill:Color32,
    pub text:Color32,
    pub border:Color32,
    pub border_width:f32,
    pub radius:f32,
}

#[derive(Clone, Debug)]
pub struct ManualSourceBlockVisual {
    pub rects:Vec<egui::Rect>,
    pub style:ManualSourceBlockStyle,
}

pub fn manual_source_block_style(
    span:&LinkedSpan,
    active:Option<&LinkKey>,
    assigned_ids:&HashSet<String>,
)->ManualSourceBlockStyle {
    let focused=active.is_some_and(|key|same_target(&span.key,key));
    if focused {
        ManualSourceBlockStyle {
            fill:SOURCE_ACTIVE,
            text:SOURCE_ACTIVE_TEXT,
            border:SOURCE_ACTIVE_BORDER,
            border_width:ACTIVE_BORDER_WIDTH,
            radius:theme::SOURCE_BLOCK_RADIUS,
        }
    } else if span.source_id.as_ref().is_some_and(|id|assigned_ids.contains(id)) {
        ManualSourceBlockStyle {
            fill:theme::MANUAL_SOURCE_CLASSIFIED_FILL,
            text:theme::MANUAL_SOURCE_CLASSIFIED_TEXT,
            border:theme::MANUAL_SOURCE_CLASSIFIED_BORDER,
            border_width:theme::MANUAL_SOURCE_BLOCK_BORDER_WIDTH,
            radius:theme::SOURCE_BLOCK_RADIUS,
        }
    } else {
        ManualSourceBlockStyle {
            fill:theme::MANUAL_SOURCE_UNASSIGNED_FILL,
            text:theme::MANUAL_SOURCE_UNASSIGNED_TEXT,
            border:theme::MANUAL_SOURCE_UNASSIGNED_BORDER,
            border_width:theme::MANUAL_SOURCE_BLOCK_BORDER_WIDTH,
            radius:theme::SOURCE_BLOCK_RADIUS,
        }
    }
}

/// Return the exact rounded row rectangles for a source ID, including the
/// same adjacent-token spacing used by block_shapes_manual().
pub fn manual_source_block_visual(
    raw:&str,
    galley:&egui::Galley,
    galley_pos:egui::Pos2,
    spans:&[LinkedSpan],
    source_id:&str,
    selected_recipient:Option<&str>,
    active:Option<&LinkKey>,
    assigned_ids:&HashSet<String>,
)->Option<ManualSourceBlockVisual> {
    let target=spans.iter().find(|span|
        span.source_id.as_deref()==Some(source_id)
    )?;
    let blocks=visual_blocks_with_manual_states(
        raw,spans,selected_recipient,active,Some(assigned_ids)
    );
    let target_blocks:Vec<&VisualBlock>=blocks.iter().filter(|block|
        block.key==target.key &&
        block.start_char>=target.start_char &&
        block.end_char<=target.end_char
    ).collect();
    let _first=*target_blocks.first()?;
    let style=manual_source_block_style(target,active,assigned_ids);
    let characters:Vec<char>=raw.chars().collect();
    let mut rects=Vec::new();
    let mut row_start=0usize;
    for (row_index,placed) in galley.rows.iter().enumerate() {
        let glyphs=&placed.row.glyphs;
        let row_end=row_start+glyphs.len();
        let y=galley_pos.y+placed.pos.y;
        for block in &target_blocks {
            let start=block.start_char.max(row_start);
            let end=block.end_char.min(row_end);
            if start>=end {continue;}
            let first_glyph=start-row_start;
            let last_glyph=end-row_start-1;
            if first_glyph>=glyphs.len() || last_glyph>=glyphs.len() {continue;}
            let end_x=if glyphs[last_glyph].chr=='\n' && last_glyph>first_glyph {
                glyphs[last_glyph-1].pos.x+glyphs[last_glyph-1].advance_width
            } else {
                glyphs[last_glyph].pos.x+glyphs[last_glyph].advance_width
            };
            let text_left=galley_pos.x+placed.pos.x+glyphs[first_glyph].pos.x;
            let text_right=galley_pos.x+placed.pos.x+end_x;
            let mut ink_top=f32::INFINITY;
            let mut ink_bottom=f32::NEG_INFINITY;
            for glyph in &glyphs[first_glyph..=last_glyph] {
                if glyph.uv_rect.is_nothing(){continue;}
                let glyph_top=y+glyph.pos.y+glyph.uv_rect.offset.y;
                let glyph_bottom=glyph_top+glyph.uv_rect.size.y;
                ink_top=ink_top.min(glyph_top);
                ink_bottom=ink_bottom.max(glyph_bottom);
            }
            let Some(mut rect)=compact_block_rect(
                text_left,text_right,ink_top,ink_bottom
            ) else {continue};
            if !block.focused {
                let left=blocks.iter().any(|other|
                    other.key!=block.key && other.end_char==start
                );
                let right=blocks.iter().any(|other|
                    other.key!=block.key && other.start_char==end
                );
                rect=separate_touching_blocks(rect,left,right);
                if rect.width()<=0.0 {continue;}
            } else {
                rect=source_focused_block_rect(rect);
            }
            rects.push(rect);
        }
        row_start=row_end;
        if row_index+1<galley.rows.len() &&
            characters.get(row_start)==Some(&'\n') {
            row_start+=1;
        }
    }
    if rects.is_empty(){return None;}
    Some(ManualSourceBlockVisual{rects,style})
}

fn block_shapes_with_states(
    raw:&str,galley:&egui::Galley,galley_pos:egui::Pos2,
    spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>,assigned_ids:Option<&HashSet<String>>,clip:egui::Rect,
)->(Vec<egui::Shape>,Vec<egui::Shape>) {
    let blocks=visual_blocks_with_manual_states(
        raw,spans,selected_recipient,active,assigned_ids
    );
    let characters:Vec<char>=raw.chars().collect();
    let mut normal=Vec::new();
    let mut focused=Vec::new();
    let mut outlines=Vec::new();
    let mut focused_outlines=Vec::new();
    let mut row_start=0usize;
    for (row_index,placed) in galley.rows.iter().enumerate() {
        let glyphs=&placed.row.glyphs;
        let row_end=row_start+glyphs.len();
        let y=galley_pos.y+placed.pos.y;
        let height=placed.row.size.y;
        if y<=clip.bottom()+2.0 && y+height>=clip.top()-2.0 {
            for block in &blocks {
                let start=block.start_char.max(row_start);
                let end=block.end_char.min(row_end);
                if start>=end {continue;}
                let first=start-row_start;
                let last=end-row_start-1;
                if first>=glyphs.len() || last>=glyphs.len() {continue;}
                let end_x=if glyphs[last].chr=='\n' && last>first {
                    glyphs[last-1].pos.x+glyphs[last-1].advance_width
                } else {
                    glyphs[last].pos.x+glyphs[last].advance_width
                };
                let text_left=galley_pos.x+placed.pos.x+glyphs[first].pos.x;
                let text_right=galley_pos.x+placed.pos.x+end_x;
                let mut ink_top=f32::INFINITY;
                let mut ink_bottom=f32::NEG_INFINITY;
                for glyph in &glyphs[first..=last] {
                    if glyph.uv_rect.is_nothing(){continue;}
                    let glyph_top=y+glyph.pos.y+glyph.uv_rect.offset.y;
                    let glyph_bottom=glyph_top+glyph.uv_rect.size.y;
                    ink_top=ink_top.min(glyph_top);
                    ink_bottom=ink_bottom.max(glyph_bottom);
                }
                let Some(mut rect)=compact_block_rect(
                    text_left,text_right,ink_top,ink_bottom
                ) else {continue};
                if !block.focused {
                    let left=blocks.iter().any(|other|
                        other.key!=block.key && other.end_char==start
                    );
                    let right=blocks.iter().any(|other|
                        other.key!=block.key && other.start_char==end
                    );
                    rect=separate_touching_blocks(rect,left,right);
                    if rect.width()<=0.0 {continue;}
                } else {
                    rect=source_focused_block_rect(rect);
                }
                if !rect.intersects(clip) {continue;}
                let radius=theme::SOURCE_BLOCK_RADIUS;
                let background=egui::Shape::rect_filled(rect,radius,block.fill);
                if block.focused {
                    focused.push(background);
                    focused_outlines.push(egui::Shape::rect_stroke(
                        rect,radius,egui::Stroke::new(
                            ACTIVE_BORDER_WIDTH,ACTIVE_BORDER
                        ),
                        egui::StrokeKind::Inside
                    ));
                } else {
                    normal.push(background);
                    // Reception retains its subdued gray border.
                    // Manual pending and completed blocks share rounded
                    // geometry but use independent state-specific colors.
                    let (border,width)=match block.manual_state {
                        Some(ManualBlockState::Unassigned)=>(
                            theme::MANUAL_SOURCE_UNASSIGNED_BORDER,
                            theme::MANUAL_SOURCE_BLOCK_BORDER_WIDTH
                        ),
                        Some(ManualBlockState::Classified)=>(
                            theme::MANUAL_SOURCE_CLASSIFIED_BORDER,
                            theme::MANUAL_SOURCE_BLOCK_BORDER_WIDTH
                        ),
                        None=>(
                            theme::SOURCE_RECOGNIZED_BORDER,
                            theme::SOURCE_RECOGNIZED_BORDER_WIDTH
                        ),
                    };
                    outlines.push(egui::Shape::rect_stroke(
                        rect,radius,egui::Stroke::new(width,border),
                        egui::StrokeKind::Inside
                    ));
                }
            }
        }
        row_start=row_end;
        if row_index+1<galley.rows.len() &&
           characters.get(row_start)==Some(&'\n') {
            row_start+=1;
        }
    }
    // The focus fill MUST paint after gray fills, regardless of token order.
    normal.extend(focused);
    outlines.extend(focused_outlines);
    (normal,outlines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_link_delays_match_approved_ui_behavior() {
        assert_eq!(HOVER_ENTER,Duration::from_millis(50));
        assert_eq!(HOVER_EXIT,Duration::from_millis(150));
    }

    #[test]
    fn linked_hover_never_crosses_order_identity() {
        let ctx=egui::Context::default();
        let mut state=HoverState::default();
        let linked=key("r1","r1","name");
        state.update_for("order-a",Some(linked.clone()),&ctx);
        state.pending_at=Some(Instant::now()-HOVER_ENTER);
        state.update_for("order-a",Some(linked.clone()),&ctx);
        assert_eq!(state.active_for("order-a"),Some(&linked));
        assert!(state.active_for("order-b").is_none());
        state.update_for("order-b",None,&ctx);
        assert!(state.active_for("order-a").is_none());
        assert!(state.scoped_active().is_none());
    }

    #[test]
    fn korean_and_emoji_use_true_utf16_boundaries() {
        let raw="가😀나 가";
        assert_eq!(boundary(raw,1),Some(("가".len(),1)));
        assert_eq!(boundary(raw,2),None); // emoji surrogate split
        assert_eq!(boundary(raw,3),Some(("가😀".len(),2)));
        let evidence=json!({"recipients":[{
            "id":"r1","sources":[
                {"field":"name","targetId":"r1","start":3,"end":4,"text":"나"},
                {"field":"name","targetId":"r1","start":2,"end":4,"text":"😀나"}
            ],"items":[]
        }]});
        let spans=collect(raw,&evidence);
        assert_eq!(spans.len(),1);
        assert_eq!(&raw[spans[0].start..spans[0].end],"나");
        assert_eq!(spans[0].start_char,2);
    }

    #[test]
    fn right_half_of_last_name_glyph_keeps_its_recipient_link(){
        let raw="김준희\n표시나";
        let evidence=json!({"recipients":[
            {"id":"first","sources":[{"field":"name","targetId":"first",
                "start":0,"end":3,"text":"김준희"}]},
            {"id":"second","sources":[{"field":"name","targetId":"second",
                "start":4,"end":7,"text":"표시나"}]}
        ]});
        let spans=collect(raw,&evidence);
        let ctx=egui::Context::default();
        let mut result=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            let job=layout(raw,&spans,None,None,500.0,
                FontId::proportional(16.0),Color32::WHITE);
            let galley=ctx.fonts_mut(|fonts|fonts.layout_job(job));
            assert_eq!(galley.rows.len(),2);
            let placed=&galley.rows[1];
            let glyph=placed.row.glyphs.last().unwrap();
            let pos=egui::pos2(
                placed.pos.x+glyph.pos.x+glyph.advance_width*0.75,
                placed.pos.y+placed.row.size.y*0.5,
            );
            result=hovered_key_at_pointer(raw,&spans,&galley,
                egui::pos2(0.0,0.0),pos);
        });
        assert_eq!(result.unwrap().recipient_id,"second");
    }

    #[test]
    fn repeated_names_preserve_native_recipient_ownership() {
        let raw="김가상\n김가상";
        let evidence=json!({"recipients":[
            {"id":"r1","sources":[{"field":"name","targetId":"r1",
                "start":0,"end":3,"text":"김가상"}]},
            {"id":"r2","sources":[{"field":"name","targetId":"r2",
                "start":4,"end":7,"text":"김가상"}]}
        ]});
        let spans=collect(raw,&evidence);
        assert_eq!(spans.len(),2);
        assert_ne!(spans[0].key.recipient_id,spans[1].key.recipient_id);
    }
    fn source(raw:&str,value:&str,field:&str,target:&str)->Value {
        let byte=raw.find(value).expect("sample text");
        let start=raw[..byte].encode_utf16().count();
        serde_json::json!({
            "field":field,"targetId":target,"start":start,
            "end":start+value.encode_utf16().count(),"text":value
        })
    }

    #[test]
    fn product_expression_stops_before_native_quantity() {
        let raw="표시나\nBD 709(블랙) 4개\n다음";
        let recognition=json!({"recipients":[{
            "id":"r1","sources":[],
            "items":[{"id":"i1","raw":"BD 709(블랙) 4개",
                "sources":[source(raw,"BD 709","product","i1"),
                           source(raw,"4개","quantity","i1")]}]
        }]});
        let spans=collect(raw,&recognition);
        let model=spans.iter().find(|s|s.key.field=="product").unwrap();
        assert_eq!(&raw[model.start..model.end],"BD 709(블랙)");
        let count=spans.iter().find(|s|s.key.field=="quantity").unwrap();
        assert_eq!(&raw[count.start..count.end],"4개");
    }

    #[test]
    fn ambiguous_quantity_without_source_is_not_absorbed() {
        let raw="BD 709(블랙) 4개";
        let recognition=json!({"recipients":[{
            "id":"r1","items":[{"id":"i1","raw":raw,
                "sources":[source(raw,"BD 709","product","i1")]}]
        }]});
        let spans=collect(raw,&recognition);
        assert_eq!(&raw[spans[0].start..spans[0].end],"BD 709");
    }

    #[test]
    fn note_heading_is_one_visual_target() {
        let raw="메모 문 앞에 놓아주세요";
        let recognition=json!({"recipients":[{
            "id":"r1","sources":[source(raw,"문 앞에 놓아주세요","note","r1")],
            "items":[]
        }]});
        let spans=collect(raw,&recognition);
        assert_eq!(&raw[spans[0].start..spans[0].end],raw);
    }

    #[test]
    fn repeated_product_in_other_recipient_is_not_reassigned() {
        let raw="김준희\nBD 709(블랙) 4개\n표시나\nBD 709(블랙) 4개";
        let first=raw.find("BD 709").unwrap();
        let second=raw.rfind("BD 709").unwrap();
        let mk=|byte:usize,id:&str| json!({
            "field":"product","targetId":id,
            "start":raw[..byte].encode_utf16().count(),
            "end":raw[..byte].encode_utf16().count()+6,"text":"BD 709"
        });
        let recognition=json!({"recipients":[
            {"id":"r1","items":[{"id":"i1","raw":"BD 709(블랙) 4개",
                 "sources":[mk(first,"i1"),source(raw,"4개","quantity","i1")]}]},
            {"id":"r2","items":[{"id":"i2","raw":"BD 709(블랙) 4개",
                 "sources":[mk(second,"i2")]}]}
        ]});
        let spans=collect(raw,&recognition);
        let first_product=spans.iter().find(|s|s.key.recipient_id=="r1" && s.key.field=="product").unwrap();
        let second_product=spans.iter().find(|s|s.key.recipient_id=="r2" && s.key.field=="product").unwrap();
        assert!(first_product.start<second_product.start);
        assert!(second_product.start>=second);
    }

    #[test]
    fn gray_source_blocks_keep_a_gap_between_adjacent_fields_and_lines() {
        // Adjacent recognized fields share glyph boundaries. Even then their
        // quiet gray rectangles must never touch or intrude into the next row.
        let a=compact_block_rect(5.0,15.0,10.0,20.0).unwrap();
        let b=compact_block_rect(15.0,25.0,10.0,20.0).unwrap();
        let next=compact_block_rect(5.0,15.0,30.0,40.0).unwrap();
        // Verify clearance at the INNER edge of the actual painted stroke.
        assert_eq!(a.left(),2.5);
        assert_eq!(a.right(),17.5);
        assert_eq!(a.top(),8.5);
        assert_eq!(a.bottom(),21.5);
        let inner=a.shrink(theme::SOURCE_RECOGNIZED_BORDER_WIDTH);
        assert_eq!(5.0-inner.left(),1.5);
        assert_eq!(inner.right()-15.0,1.5);
        assert_eq!(10.0-inner.top(),0.5);
        assert_eq!(inner.bottom()-20.0,0.5);
        let first=separate_touching_blocks(a,false,true);
        let second=separate_touching_blocks(b,true,false);
        assert_eq!(first.right(),14.75);
        assert_eq!(second.left(),15.25);
        assert!((second.left()-first.right()-theme::SOURCE_BLOCK_ADJACENT_GAP).abs()<0.001);
        assert!(a.bottom()<next.top());
        // Focus adds only the extra stroke width; the inner whitespace
        // must remain exactly the same as in the normal chip.
        let active=source_focused_block_rect(a);
        assert_eq!(active.shrink(ACTIVE_BORDER_WIDTH),inner);
        assert_eq!(theme::SOURCE_BLOCK_HORIZONTAL_PADDING,1.5);
        assert_eq!(theme::SOURCE_BLOCK_VERTICAL_PADDING,0.5);
    }

    #[test]
    fn source_focus_uses_the_same_palette_as_recipient_and_item_highlights(){
        let raw="표시나";
        let recognition=json!({"recipients":[{
            "id":"r1","sources":[source(raw,raw,"name","r1")]
        }]});
        let spans=collect(raw,&recognition);
        let active=key("r1","r1","name");
        let blocks=visual_blocks(raw,&spans,Some("r1"),Some(&active));
        assert_eq!(blocks.len(),1);
        assert!(blocks[0].focused);
        assert_eq!(blocks[0].fill,ACTIVE_FILL);
        assert_eq!(SOURCE_ACTIVE,ACTIVE_FILL);
        assert_eq!(SOURCE_ACTIVE_TEXT,ACTIVE_TEXT);
        assert_eq!(SOURCE_ACTIVE_BORDER,ACTIVE_BORDER);
        assert_eq!(ACTIVE_RADIUS,4.0);
        assert_eq!(ACTIVE_BORDER_WIDTH,2.0);
    }

    #[test]
    fn manual_assignment_state_changes_only_manual_palette_and_ink() {
        assert_eq!(theme::PRIMARY_ACTION_FILL,Color32::from_rgb(238,238,238));
        let gray=((u16::from(theme::PRIMARY_ACTION_FILL.r())*9+5)/10) as u8;
        assert_eq!(theme::MANUAL_SOURCE_UNASSIGNED_FILL,Color32::from_rgb(gray,gray,gray));
        assert_eq!(theme::MANUAL_SOURCE_UNASSIGNED_FILL,Color32::from_rgb(214,214,214));
        assert_eq!(theme::MANUAL_SOURCE_UNASSIGNED_TEXT,theme::BRIGHT_TEXT);
        assert_eq!(theme::MANUAL_SOURCE_UNASSIGNED_BORDER,theme::MANUAL_SOURCE_UNASSIGNED_FILL);
        assert_eq!(theme::SOURCE_BLOCK_RADIUS,4.0);
        assert_eq!(ACTIVE_RADIUS,theme::SOURCE_BLOCK_RADIUS);
        assert_eq!(theme::MANUAL_SOURCE_CLASSIFIED_BORDER,theme::SOURCE_RECOGNIZED_BORDER);
        assert_ne!(theme::MANUAL_SOURCE_UNASSIGNED_BORDER,theme::SOURCE_RECOGNIZED_BORDER);
        assert_ne!(theme::MANUAL_SOURCE_UNASSIGNED_FILL,ACTIVE_FILL);
        assert_ne!(theme::MANUAL_SOURCE_UNASSIGNED_BORDER,ACTIVE_BORDER);
        // Both reading states must remain legible; use the actual palette
        // luminance contrast instead of hard-coding obsolete gray swatches.
        let luminance=|c:Color32|{
            let linear=|v:u8|{let v=f64::from(v)/255.0;
                if v<=0.04045{v/12.92}else{((v+0.055)/1.055).powf(2.4)}};
            0.2126*linear(c.r())+0.7152*linear(c.g())+0.0722*linear(c.b())
        };
        for (ink,fill) in [
            (theme::MANUAL_SOURCE_UNASSIGNED_TEXT,theme::MANUAL_SOURCE_UNASSIGNED_FILL),
            (theme::MANUAL_SOURCE_CLASSIFIED_TEXT,theme::MANUAL_SOURCE_CLASSIFIED_FILL)
        ]{
            let ink_luminance=luminance(ink);
            let fill_luminance=luminance(fill);
            assert!((ink_luminance.max(fill_luminance)+0.05)
                /(ink_luminance.min(fill_luminance)+0.05)>=4.5);
        }
        assert_eq!(theme::MANUAL_SOURCE_BLOCK_BORDER_WIDTH,1.0);
        let raw="팔렬고등학교 30개";
        let rows=json!([{"id":"r1","items":[{"id":"i1"}]}]);
        let blocks=json!([
            {"id":"school","start":0,"end":6,"text":"팔렬고등학교",
                "targetId":"r1","field":"name"},
            {"id":"qty","start":7,"end":10,"text":"30개",
                "targetId":"i1","field":"quantity"}
        ]);
        let assignments=json!({
            "school":{"recipientId":"r1","field":"name"}
        });
        let spans=collect_manual(raw,rows.as_array().unwrap(),
            blocks.as_array().unwrap(),&assignments);
        assert_eq!(spans.len(),2);
        let assigned=HashSet::from([String::from("school")]);
        let manual=visual_blocks_with_manual_states(raw,&spans,None,None,Some(&assigned));
        assert_eq!(manual.len(),2);
        assert_eq!(manual[0].manual_state,Some(ManualBlockState::Classified));
        assert_eq!(manual[0].fill,theme::MANUAL_SOURCE_CLASSIFIED_FILL);
        assert_eq!(manual[1].manual_state,Some(ManualBlockState::Unassigned));
        assert_eq!(manual[1].fill,theme::MANUAL_SOURCE_UNASSIGNED_FILL);
        let reception=visual_blocks(raw,&spans,None,None);
        assert!(reception.iter().all(|b|b.manual_state.is_none()));
        assert!(reception.iter().all(|b|b.fill==SOURCE_BLOCK));
        let active=spans[1].key.clone();
        let highlighted=visual_blocks_with_manual_states(
            raw,&spans,None,Some(&active),Some(&assigned)
        );
        assert_eq!(highlighted[1].fill,ACTIVE_FILL);
        assert!(highlighted[1].focused);

        // The manual status palette stays scoped to the manual viewer;
        // Reception now receives an ordinary 1pt white token border.
        let regular_layout=layout(
            raw,&spans,None,None,360.0,
            FontId::proportional(16.0),Color32::WHITE
        );
        assert!(regular_layout.sections.iter().all(|section|
            section.format.color==Color32::WHITE
        ));
        let manual_layout=layout_manual(
            raw,&spans,None,None,&assigned,360.0,
            FontId::proportional(16.0),Color32::WHITE
        );
        assert!(manual_layout.sections.iter().any(|section|
            section.format.color==theme::MANUAL_SOURCE_CLASSIFIED_TEXT
        ));
        assert!(manual_layout.sections.iter().any(|section|
            section.format.color==theme::MANUAL_SOURCE_UNASSIGNED_TEXT
        ));

        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            let galley=ctx.fonts_mut(|fonts|fonts.layout_job(manual_layout.clone()));
            let area=egui::Rect::from_min_size(
                egui::Pos2::ZERO,egui::vec2(400.0,200.0)
            );
            let (manual_fills,manual_strokes)=block_shapes_manual(
                raw,&galley,egui::Pos2::ZERO,&spans,None,None,&assigned,area
            );
            assert_eq!(manual_strokes.len(),2,
                "each classified/unclassified source block needs its own stroke");
            assert_eq!(manual_fills.len(),2);
            assert!(manual_fills.iter().any(|shape|matches!(shape,
                egui::Shape::Rect(rect) if rect.fill==theme::MANUAL_SOURCE_UNASSIGNED_FILL)));
            assert!(manual_strokes.iter().any(|shape|matches!(shape,
                egui::Shape::Rect(rect) if rect.stroke.color==theme::MANUAL_SOURCE_UNASSIGNED_BORDER)));
            for shape in manual_fills.iter().chain(manual_strokes.iter()){
                assert!(matches!(shape,egui::Shape::Rect(rect)
                    if rect.corner_radius==egui::CornerRadius::same(4)),
                    "manual fill and outline must share the same rounded corners");
            }
            let (receipt_fills,receipt_strokes)=block_shapes(
                raw,&galley,egui::Pos2::ZERO,&spans,None,None,area
            );
            assert_eq!(receipt_strokes.len(),2,
                "Reception recognized tokens need their own outlines");
            assert_eq!(receipt_fills.len(),2);
            for shape in receipt_fills.iter().chain(receipt_strokes.iter()) {
                assert!(matches!(shape,egui::Shape::Rect(rect)
                    if rect.corner_radius==egui::CornerRadius::same(4)),
                    "Reception source blocks must match the manual master radius");
            }
            assert!(receipt_strokes.iter().all(|shape|matches!(
                shape,egui::Shape::Rect(rect)
                    if rect.stroke.color==theme::SOURCE_RECOGNIZED_BORDER
                    && rect.stroke.width==theme::SOURCE_RECOGNIZED_BORDER_WIDTH
            )));
        });
    }

    #[test]
    fn normal_reception_source_tokens_use_white_borders_and_blue_focus_precedes(){
        let raw="270 2개";
        let recognition=json!({"recipients":[{
            "id":"r1","sources":[source(raw,"270","product","r1"),
                source(raw,"2개","quantity","r1")]
        }]});
        let spans=collect(raw,&recognition);
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            let job=layout(raw,&spans,None,None,360.0,
                FontId::proportional(16.0),Color32::WHITE);
            let galley=ctx.fonts_mut(|fonts|fonts.layout_job(job));
            let clip=egui::Rect::from_min_size(
                egui::Pos2::ZERO,egui::vec2(400.0,100.0)
            );
            let (_,white)=block_shapes(raw,&galley,egui::Pos2::ZERO,
                &spans,None,None,clip);
            assert_eq!(white.len(),2);
            assert!(white.iter().all(|shape|matches!(
                shape,egui::Shape::Rect(rect)
                    if rect.stroke.color==theme::SOURCE_RECOGNIZED_BORDER
                    && rect.stroke.width==theme::SOURCE_RECOGNIZED_BORDER_WIDTH
            )));
            let active=spans[0].key.clone();
            let (_,focused)=block_shapes(raw,&galley,egui::Pos2::ZERO,
                &spans,None,Some(&active),clip);
            assert_eq!(focused.len(),2);
            assert!(matches!(focused.last(),
                Some(egui::Shape::Rect(rect))
                    if rect.stroke.color==ACTIVE_BORDER
                    && rect.stroke.width==ACTIVE_BORDER_WIDTH
                    && rect.corner_radius==egui::CornerRadius::same(4)
            ),"focused blue border must retain the common radius and draw last");
        });
    }

    #[test]
    fn quiet_gray_blocks_exist_without_recipient_selection() {
        let raw="표시나";
        let recognition=json!({"recipients":[{
            "id":"r1","sources":[source(raw,raw,"name","r1")]
        }]});
        let spans=collect(raw,&recognition);
        let blocks=visual_blocks(raw,&spans,None,None);
        assert_eq!(blocks.len(),1);
        assert_eq!(blocks[0].fill,SOURCE_BLOCK);
        assert!(!blocks[0].focused);
        let selected=visual_blocks(raw,&spans,Some("r1"),None);
        assert_eq!(selected[0].fill,SOURCE_CONTEXT);
    }

}


#[cfg(test)]
mod manual_span_projection_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manual_source_projection_follows_current_assignment_targets() {
        let raw="김준희\nBD 709 4개";
        let rows=json!([
            {"id":"r1","items":[{"id":"i1"}]},
            {"id":"r2","items":[{"id":"i2"}]}
        ]);
        let blocks=json!([
            {"id":"name","start":0,"end":3,"text":"김준희",
                "targetId":"r1","field":"name"},
            {"id":"product","start":4,"end":10,"text":"BD 709",
                "targetId":"i1","field":"product"},
            {"id":"quantity","start":11,"end":13,"text":"4개",
                "targetId":"i1","field":"quantity"}
        ]);
        let assignments=json!({
            "product":{"recipientId":"r2","itemId":"i2","field":"product"}
        });
        let spans=collect_manual(
            raw,rows.as_array().unwrap(),blocks.as_array().unwrap(),&assignments
        );
        assert_eq!(spans.len(),3);
        assert_eq!(spans[0].source_id.as_deref(),Some("name"));
        assert_eq!(spans[0].key, key("r1","r1","name"));
        assert_eq!(spans[1].source_id.as_deref(),Some("product"));
        assert_eq!(spans[1].key,key("r2","i2","product"));
        assert_eq!(spans[2].source_id.as_deref(),Some("quantity"));
        assert_eq!(spans[2].key,key("r1","i1","quantity"));
    }

    #[test]
    fn manual_projection_rejects_utf16_surrogate_splits_and_stale_text() {
        let raw="가😀나";
        let rows=json!([{"id":"r1","items":[]}]);
        let blocks=json!([
            {"id":"emoji-half","start":1,"end":2,"text":"😀",
                "targetId":"r1","field":"name"},
            {"id":"stale","start":3,"end":4,"text":"다",
                "targetId":"r1","field":"name"},
            {"id":"valid","start":3,"end":4,"text":"나",
                "targetId":"r1","field":"name"}
        ]);
        let spans=collect_manual(
            raw,rows.as_array().unwrap(),blocks.as_array().unwrap(),&json!({})
        );
        assert_eq!(spans.len(),1);
        assert_eq!(spans[0].source_id.as_deref(),Some("valid"));
        assert_eq!(&raw[spans[0].start..spans[0].end],"나");
    }

    #[test]
    fn hovered_manual_span_returns_its_exact_block_identity() {
        let raw="가😀나";
        let rows=json!([{"id":"r1","items":[]}]);
        let blocks=json!([
            {"id":"first","start":0,"end":1,"text":"가",
                "targetId":"r1","field":"name"},
            {"id":"last","start":3,"end":4,"text":"나",
                "targetId":"r1","field":"name"}
        ]);
        let spans=collect_manual(
            raw,rows.as_array().unwrap(),blocks.as_array().unwrap(),&json!({})
        );
        let ctx=egui::Context::default();
        let mut hit=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            let job=layout(
                raw,&spans,None,None,300.0,
                FontId::proportional(16.0),Color32::WHITE
            );
            let galley=ctx.fonts_mut(|fonts|fonts.layout_job(job));
            let glyph=galley.rows[0].row.glyphs.last().unwrap();
            let pointer=egui::pos2(
                glyph.pos.x+glyph.advance_width*0.75,
                galley.rows[0].row.size.y*0.5
            );
            hit=hovered_span_index_at_pointer(
                raw,&spans,&galley,egui::Pos2::ZERO,pointer
            ).and_then(|index|spans.get(index))
                .and_then(|span|span.source_id.clone());
        });
        assert_eq!(hit.as_deref(),Some("last"));
    }
}
