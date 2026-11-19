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
fn boundary(raw:&str, units:usize)->Option<(usize,usize)> {
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
    spans.sort_by_key(|span|(span.start, span.end,span.key.field.clone()));
    spans.dedup_by(|b,a|
        a.start==b.start && a.end==b.end && a.key==b.key);
    spans
}

pub fn key(recipient:&str,target:&str,field:&str)->LinkKey {
    LinkKey{recipient_id:recipient.into(),target_id:target.into(),field:field.into()}
}

fn same_target(left:&LinkKey,right:&LinkKey)->bool {
    left.recipient_id==right.recipient_id &&
    left.target_id==right.target_id && left.field==right.field
}

/// Layout is strictly visual; no source or business mutations. Active field
/// is blue, other spans within the selected/hovered recipient are muted amber.
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
    // One visible style per character, even for overlapping native spans.
    let mut positions=Vec::<usize>::with_capacity(spans.len()*2+2);
    positions.push(0);
    positions.push(raw.len());
    for span in spans {
        if span.end<=raw.len() && raw.is_char_boundary(span.start) &&
           raw.is_char_boundary(span.end) {
            positions.push(span.start);
            positions.push(span.end);
        }
    }
    positions.sort_unstable();
    positions.dedup();
    for w in positions.windows(2) {
        let (begin,end)=(w[0],w[1]);
        if begin==end {continue;}
        let own=spans.iter().filter(|s|s.start<=begin && s.end>=end);
        let focused=active.and_then(|key|own.clone().find(|span|
            same_target(&span.key,key)));
        let context=selected_recipient
            .map(|id|own.clone().any(|span|span.key.recipient_id==id))
            .unwrap_or(false) ||
            active.is_some_and(|key|own.clone().any(|span|
                span.key.recipient_id==key.recipient_id));
        let background=if focused.is_some() {
            Color32::from_rgb(43,77,112)
        } else if context {
            Color32::from_rgb(69,56,34)
        } else {
            Color32::TRANSPARENT
        };
        let format=egui::TextFormat {
            font_id:font.clone(),color,
            background,..Default::default()
        };
        job.append(&raw[begin..end],0.0,format);
        cursor=end;
    }
    if cursor<raw.len() {
        job.append(&raw[cursor..],0.0,egui::TextFormat {
            font_id:font,color,..Default::default()
        });
    }
    job
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
}
