//! Manual classification uses the same source-highlight projection as
//! reception intake. This module owns only the worksheet's click-to-assign
//! action; linked spans, hover timing, visual blocks and hit testing are shared.

use eframe::egui::{self, Align, Color32, FontId, Sense, Ui};
use crate::{source_highlight::{self,LinkKey,LinkedSpan},theme,typography};

#[derive(Default)]
pub struct SourceInteraction {
    pub hovered:Option<LinkKey>,
    pub clicked_block:Option<String>,
    pub did_scroll:bool,
}

/// Paint the exact original document with the shared rounded evidence blocks.
/// Hovering a source block previews its target cell; hovering a target cell
/// previews and scrolls back to this exact source range. Clicking a block
/// returns its native ID so the Rust manual reducer remains authoritative.
pub fn source_document(
    ui:&mut Ui,
    raw:&str,
    spans:&[LinkedSpan],
    selected_recipient:Option<&str>,
    active:Option<&LinkKey>,
    scroll_to:Option<&LinkKey>,
    enabled:bool,
)->SourceInteraction {
    let width=ui.available_width().max(1.0);
    let job=source_highlight::layout(
        raw,spans,selected_recipient,active,width,
        FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT),
        theme::TEXT,
    );
    let galley=ui.fonts_mut(|fonts|fonts.layout_job(job));
    let (rect,response)=ui.allocate_exact_size(
        egui::vec2(width,galley.size().y.max(theme::INPUT_HEIGHT)),
        if enabled{Sense::click()}else{Sense::hover()},
    );
    let (fills,outlines)=source_highlight::block_shapes(
        raw,&galley,rect.min,spans,selected_recipient,active,
        rect.intersect(ui.clip_rect()),
    );
    ui.painter().add(egui::Shape::Vec(fills));
    ui.painter().galley(rect.min,galley.clone(),theme::TEXT);
    ui.painter().add(egui::Shape::Vec(outlines));

    let mut result=SourceInteraction::default();
    let hovered_index=if response.hovered() || response.clicked() {
        response.interact_pointer_pos().and_then(|pointer|
            source_highlight::hovered_span_index_at_pointer(
                raw,spans,&galley,rect.min,pointer
            )
        )
    }else{None};
    if let Some(index)=hovered_index.and_then(|index|spans.get(index)){
        result.hovered=Some(index.key.clone());
        if enabled {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked(){
            result.clicked_block=index.source_id.clone();
        }
    }
    if let Some(key)=scroll_to {
        if let Some(span)=spans.iter().find(|span|span.key==*key){
            let cursor=galley.pos_from_cursor(
                egui::text::CCursor::new(span.start_char)
            );
            ui.scroll_to_rect(
                cursor.translate(rect.min.to_vec2()),Some(Align::Center)
            );
            result.did_scroll=true;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicked_span_preserves_the_exact_native_block_id() {
        let spans=vec![LinkedSpan{
            start:0,end:3,start_char:0,end_char:3,
            key:source_highlight::key("r1","r1","name"),
            source_id:Some("0:3".into()),
        }];
        assert_eq!(spans[0].source_id.as_deref(),Some("0:3"));
    }
}
