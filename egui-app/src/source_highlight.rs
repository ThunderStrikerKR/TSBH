//! Visual projection of authoritative Rust recognition source spans.
//! The source recognizer reports UTF-16 offsets for TypeScript parity;
//! egui TextEdit and LayoutJob require UTF-8 byte ranges / Unicode char cursors.
//! Never match repeated names by guessing text position or infer ownership.

use eframe::egui::{self, Color32, FontId};
use serde_json::Value;
use std::time::{Duration, Instant};

pub const HOVER_ENTER: Duration = Duration::from_millis(200);
pub const HOVER_EXIT: Duration = Duration::from_millis(250);
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
}

#[derive(Default)]
pub struct HoverState {
    pending: Option<LinkKey>,
    pending_at: Option<Instant>,
    pub active: Option<LinkKey>,
    release_at: Option<Instant>,
    pub scroll_to: Option<LinkKey>,
}

impl HoverState {
    pub fn clear(&mut self) { *self=Self::default(); }

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

pub fn key(recipient:&str,target:&str,field:&str)->LinkKey {
    LinkKey{recipient_id:recipient.into(),target_id:target.into(),field:field.into()}
}

/// Paint-only feedback on the detail field; it never changes layout,
/// capture targets, recipient ownership or input focus.
pub fn paint_active(ui:&egui::Ui, response:&egui::Response,
                    key:&LinkKey, active:Option<&LinkKey>) {
    if active.is_some_and(|selection|same_target(selection,key)) {
        ui.painter().rect_stroke(
            response.rect,3.0,
            egui::Stroke::new(1.5,Color32::from_rgb(112,180,255)),
            egui::StrokeKind::Outside,
        );
    }
}

fn same_target(left:&LinkKey,right:&LinkKey)->bool {
    left.recipient_id==right.recipient_id &&
    left.target_id==right.target_id && left.field==right.field
}

// The React source token palette is intentionally quiet in this dark theme.
// 16pt SUIT stays unchanged; only the recognized text receives 4pt rounded
// blocks (3pt horizontal / 1pt vertical visual padding).
const SOURCE_BLOCK:Color32=Color32::from_rgb(66,66,66);        // #424242
const SOURCE_CONTEXT:Color32=Color32::from_rgb(73,73,73);      // #494949
const SOURCE_ACTIVE:Color32=Color32::from_rgb(23,67,109);      // #17436d
const SOURCE_ACTIVE_TEXT:Color32=Color32::from_rgb(234,244,255);
const SOURCE_ACTIVE_BORDER:Color32=Color32::from_rgb(117,181,255);

#[derive(Clone,Debug)]
struct VisualBlock {
    start:usize,
    end:usize,
    start_char:usize,
    end_char:usize,
    key:LinkKey,
    fill:Color32,
    focused:bool,
}

// Resolve overlapping evidence once per character interval, so two
// classifiers cannot paint competing fills over the same source text.
fn visual_blocks(
    raw:&str,spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>
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
        let fill=if is_focused {SOURCE_ACTIVE}
            else if is_context {SOURCE_CONTEXT}
            else {SOURCE_BLOCK};
        if let Some(previous)=result.last_mut() {
            if previous.end==start && previous.key==chosen.key &&
               previous.fill==fill && previous.focused==is_focused {
                previous.end=end;
                previous.end_char=char_cursor;
                continue;
            }
        }
        result.push(VisualBlock {
            start,end,start_char,end_char:char_cursor,
            key:chosen.key.clone(),fill,focused:is_focused,
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
    let mut job=egui::text::LayoutJob::default();
    job.wrap.max_width=width;
    let mut cursor=0;
    for block in visual_blocks(raw,spans,selected_recipient,active) {
        if cursor<block.start {
            job.append(&raw[cursor..block.start],0.0,egui::TextFormat {
                font_id:font.clone(),color,..Default::default()
            });
        }
        job.append(&raw[block.start..block.end],0.0,egui::TextFormat {
            font_id:font.clone(),
            color:if block.focused {SOURCE_ACTIVE_TEXT} else {color},
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

/// Rounded blocks are painted beneath the editor glyphs, but the blue focus
/// border is painted over all blocks. In particular, a later gray token must
/// never hide the active blue field when adjacent token padding overlaps.
/// Glyph positions come from egui; source bytes and character cursors stay
/// untouched, including Korean text and wrapped addresses.
pub fn block_shapes(
    raw:&str,galley:&egui::Galley,galley_pos:egui::Pos2,
    spans:&[LinkedSpan],selected_recipient:Option<&str>,
    active:Option<&LinkKey>,clip:egui::Rect,
)->(Vec<egui::Shape>,Vec<egui::Shape>) {
    let blocks=visual_blocks(raw,spans,selected_recipient,active);
    let characters:Vec<char>=raw.chars().collect();
    let mut normal=Vec::new();
    let mut focused=Vec::new();
    let mut outlines=Vec::new();
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
                // Active outlines get a small amount of breathing room while
                // normal gray tokens do not intrude into neighbouring fields.
                let hpad=if block.focused {2.0} else {0.5};
                let left=galley_pos.x+placed.pos.x+glyphs[first].pos.x-hpad;
                let right=galley_pos.x+placed.pos.x+end_x+hpad;
                if right<=left {continue;}
                let rect=egui::Rect::from_min_max(
                    egui::pos2(left,y-1.0),
                    egui::pos2(right,y+height+1.0)
                );
                if !rect.intersects(clip) {continue;}
                let background=egui::Shape::rect_filled(rect,4.0,block.fill);
                if block.focused {
                    focused.push(background);
                    outlines.push(egui::Shape::rect_stroke(
                        rect,4.0,egui::Stroke::new(2.0,SOURCE_ACTIVE_BORDER),
                        egui::StrokeKind::Inside
                    ));
                } else {
                    normal.push(background);
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
    (normal,outlines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
