//! One faithful, selectable original document, not a stack of buttons.
//! Rust supplies manual block boundaries (UTF-16). This view only validates
//! them and paints visual ownership; allocation/release remains in the
//! native manual_classification reducer.

use eframe::egui::{self, Color32, FontId, Sense, Ui};
use serde_json::Value;
use crate::{source_highlight,theme,typography};

struct Block<'a> {
    id: &'a str,
    first: usize, // UTF-8 byte
    last: usize,
    start_char: usize,
    end_char: usize,
    assigned: bool,
}
fn str_field<'a>(v:&'a Value,key:&str)->&'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn valid_blocks<'a>(raw:&str, rows:&'a [Value], assignments:&Value)->Vec<Block<'a>> {
    let mut out=Vec::new();
    let mut last_end=0;
    for block in rows {
        let (Some(start),Some(end))=(
            block.get("start").and_then(Value::as_u64),
            block.get("end").and_then(Value::as_u64),
        )else{continue};
        let (Some((first,start_char)),Some((last,end_char)))=(
            source_highlight::boundary(raw,start as usize),
            source_highlight::boundary(raw,end as usize),
        )else{continue};
        if first<last_end || last<=first || last>raw.len()
            || &raw[first..last]!=str_field(block,"text") {
            continue;
        }
        let id=str_field(block,"id");
        if id.is_empty(){continue;}
        out.push(Block {
            id,first,last,start_char,end_char,
            assigned:assignments.get(id).is_some_and(|v|!v.is_null()),
        });
        last_end=last;
    }
    out
}
fn text_format(color:Color32,background:Color32)->egui::TextFormat {
    egui::TextFormat {
        font_id:FontId::proportional(typography::FONT_BODY),
        color,background,..Default::default()
    }
}

/// Preserves all whitespace/newlines including text outside recognized
/// spans; click returns a native block ID and nothing else.
pub fn source_document(
    ui:&mut Ui,
    raw:&str,
    blocks:&[Value],
    assignments:&Value,
    enabled:bool,
)->Option<String> {
    let verified=valid_blocks(raw,blocks,assignments);
    let mut job=egui::text::LayoutJob::default();
    job.wrap.max_width=ui.available_width().max(1.0);
    let mut cursor=0;
    for block in &verified {
        if cursor<block.first {
            job.append(&raw[cursor..block.first],0.0,
                text_format(theme::TEXT,Color32::TRANSPARENT));
        }
        job.append(&raw[block.first..block.last],0.0,text_format(
            theme::TEXT,
            if block.assigned {
                theme::MANUAL_SOURCE_ASSIGNED_BG
            } else {
                theme::MANUAL_SOURCE_PENDING_BG
            },
        ));
        cursor=block.last;
    }
    if cursor<raw.len() {
        job.append(&raw[cursor..],0.0,
            text_format(theme::TEXT,Color32::TRANSPARENT));
    }
    let galley=ui.fonts_mut(|fonts|fonts.layout_job(job));
    let (rect,response)=ui.allocate_exact_size(
        egui::vec2(ui.available_width(),galley.size().y.max(theme::INPUT_HEIGHT)),
        if enabled{Sense::click()}else{Sense::hover()},
    );
    ui.painter().galley(rect.min,galley.clone(),theme::TEXT);
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if enabled && response.clicked() {
        if let Some(point)=response.interact_pointer_pos() {
            let index=galley.cursor_from_pos(point-rect.min).index;
            return verified.iter().find(|b|
                b.start_char<=index && index<b.end_char
            ).map(|b|b.id.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_exact_repeated_source_ranges() {
        let raw="김가상\n김가상";
        let blocks=json!([
            {"id":"a","text":"김가상","start":0,"end":3},
            {"id":"b","text":"김가상","start":4,"end":7}
        ]);
        let spans=valid_blocks(raw,blocks.as_array().unwrap(),&json!({}));
        assert_eq!(spans.len(),2);
        assert_eq!(spans[0].id,"a");
        assert_eq!(spans[1].id,"b");
        assert_eq!(spans[1].start_char,4);
    }
    #[test]
    fn rejects_invalid_emoji_split() {
        let raw="가😀나";
        let blocks=json!([
            {"id":"bad","text":"😀","start":2,"end":3},
            {"id":"ok","text":"나","start":3,"end":4}
        ]);
        let spans=valid_blocks(raw,blocks.as_array().unwrap(),&json!({}));
        assert_eq!(spans.len(),1);
        assert_eq!(spans[0].id,"ok");
    }
}
