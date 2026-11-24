//! Manual classification uses the same source-highlight projection as
//! reception intake. This module owns only the worksheet's click-to-assign
//! action; linked spans, hover timing, visual blocks and hit testing are shared.

use std::collections::HashSet;
use eframe::egui::{self, Align, FontId, Sense, Ui};
use crate::{source_highlight::{self,LinkKey,LinkedSpan},theme,typography};

#[derive(Default)]
pub struct SourceInteraction {
    pub hovered:Option<LinkKey>,
    pub clicked_block:Option<String>,
    pub drag_started_block:Option<String>,
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
    assigned_ids:&HashSet<String>,
    selected_recipient:Option<&str>,
    active:Option<&LinkKey>,
    scroll_to:Option<&LinkKey>,
    enabled:bool,
    selected_source:Option<&str>,
)->SourceInteraction {
    let width=ui.available_width().max(1.0);
    let job=source_highlight::layout_manual(
        raw,spans,selected_recipient,active,assigned_ids,width,
        FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT),
        theme::TEXT,
    );
    let galley=ui.fonts_mut(|fonts|fonts.layout_job(job));
    let (rect,response)=ui.allocate_exact_size(
        egui::vec2(width,galley.size().y.max(theme::INPUT_HEIGHT)),
        if enabled{Sense::click_and_drag()}else{Sense::hover()},
    );
    let (fills,outlines)=source_highlight::block_shapes_manual(
        raw,&galley,rect.min,spans,selected_recipient,active,assigned_ids,
        rect.intersect(ui.clip_rect()),
    );
    ui.painter().add(egui::Shape::Vec(fills));
    ui.painter().galley(rect.min,galley.clone(),theme::TEXT);
    ui.painter().add(egui::Shape::Vec(outlines));
    // Selection is exact by native source ID, even if another span shares its LinkKey.
    if let Some(span)=selected_source.and_then(|id|spans.iter().find(|s|s.source_id.as_deref()==Some(id))){
        let (fill,outline)=source_highlight::block_shapes_manual(
            raw,&galley,rect.min,std::slice::from_ref(span),selected_recipient,
            Some(&span.key),assigned_ids,rect.intersect(ui.clip_rect())
        );
        ui.painter().add(egui::Shape::Vec(fill));
        ui.painter().galley(rect.min,galley.clone(),theme::TEXT);
        ui.painter().add(egui::Shape::Vec(outline));
    }

    let mut result=SourceInteraction::default();
    // egui::Response::interact_pointer_pos() is None for a pure hover:
    // use PointerState::hover_pos when the source text is hovered without a
    // mouse button. Respect the widget's native hovered/clicked gate so a
    // disabled or obscured parent cannot gain input through this fallback.
    let hovered_index=if response.hovered() || response.clicked() {
        response.interact_pointer_pos()
            .or_else(||ui.input(|input|input.pointer.hover_pos()))
            .and_then(|pointer|
                source_highlight::hovered_span_index_at_pointer(
                    raw,spans,&galley,rect.min,pointer
                )
            )
    }else{None};
    if enabled && response.drag_started_by(egui::PointerButton::Primary){
        // Resolve the block at press origin, not the current drag pointer.
        result.drag_started_block=ui.input(|input|input.pointer.press_origin())
            .and_then(|pointer|source_highlight::hovered_span_index_at_pointer(
                raw,spans,&galley,rect.min,pointer
            )).and_then(|index|spans.get(index))
            .and_then(|span|span.source_id.clone());
    }
    if enabled {
        if let Some(index)=hovered_index.and_then(|index|spans.get(index)){
            result.hovered=Some(index.key.clone());
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if response.clicked(){
                result.clicked_block=index.source_id.clone();
            }
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

    fn draw(
        ctx:&egui::Context,
        raw_input:egui::RawInput,
        raw:&str,
        spans:&[LinkedSpan],
        origin:&mut egui::Pos2,
        enabled:bool,
    )->SourceInteraction{
        let mut interaction=SourceInteraction::default();
        let _=ctx.run(raw_input,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(320.0);
                *origin=ui.cursor().min;
                interaction=source_document(
                    ui,raw,spans,&HashSet::new(),None,None,None,enabled,None
                );
            });
        });
        interaction
    }

    #[test]
    fn hover_and_click_share_the_native_source_link_identity(){
        let raw="가나다";
        let key=source_highlight::key("r1","r1","name");
        let spans=vec![LinkedSpan{
            start:0,end:raw.len(),start_char:0,end_char:raw.chars().count(),
            key:key.clone(),source_id:Some("0:9".into()),
        }];
        let ctx=egui::Context::default();
        let screen=egui::Rect::from_min_size(
            egui::Pos2::ZERO,egui::vec2(600.0,400.0)
        );
        let mut origin=egui::Pos2::ZERO;
        let mut initial=egui::RawInput::default();
        initial.screen_rect=Some(screen);
        draw(&ctx,initial,raw,&spans,&mut origin,true);

        let job=source_highlight::layout(
            raw,&spans,None,None,320.0,
            FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT),
            theme::TEXT,
        );
        let galley=ctx.fonts_mut(|fonts|fonts.layout_job(job));
        let glyph=&galley.rows[0].row.glyphs[0];
        let pointer=origin+egui::vec2(
            glyph.pos.x+glyph.advance_width*0.5,
            galley.rows[0].row.size.y*0.5
        );
        // egui 0.33 resolves Area hit-testing using prior-frame geometry.
        // Enter the glyph first, then verify hover on a stable second frame;
        // this models a real pointer staying over the source for >50ms.
        draw(&ctx,egui::RawInput{
            events:vec![egui::Event::PointerMoved(pointer)],
            time:Some(0.1),..Default::default()
        },raw,&spans,&mut origin,true);
        let hovered=draw(&ctx,egui::RawInput{
            events:vec![egui::Event::PointerMoved(pointer)],
            time:Some(0.2),..Default::default()
        },raw,&spans,&mut origin,true);
        assert_eq!(hovered.hovered,Some(key));

        draw(&ctx,egui::RawInput{
            events:vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton{
                    pos:pointer,button:egui::PointerButton::Primary,
                    pressed:true,modifiers:egui::Modifiers::NONE
                }
            ],
            time:Some(0.3),..Default::default()
        },raw,&spans,&mut origin,true);
        let clicked=draw(&ctx,egui::RawInput{
            events:vec![egui::Event::PointerButton{
                pos:pointer,button:egui::PointerButton::Primary,
                pressed:false,modifiers:egui::Modifiers::NONE
            }],
            time:Some(0.4),..Default::default()
        },raw,&spans,&mut origin,true);
        assert_eq!(clicked.clicked_block.as_deref(),Some("0:9"));
    }

    #[test]
    fn busy_source_view_keeps_links_visible_but_blocks_hover_and_click(){
        let raw="가나다";
        let spans=vec![LinkedSpan{
            start:0,end:raw.len(),start_char:0,end_char:raw.chars().count(),
            key:source_highlight::key("r1","r1","name"),
            source_id:Some("0:9".into()),
        }];
        let ctx=egui::Context::default();
        let screen=egui::Rect::from_min_size(
            egui::Pos2::ZERO,egui::vec2(600.0,400.0)
        );
        let mut origin=egui::Pos2::ZERO;
        let mut initial=egui::RawInput::default();
        initial.screen_rect=Some(screen);
        draw(&ctx,initial,raw,&spans,&mut origin,false);
        let job=source_highlight::layout(
            raw,&spans,None,None,320.0,
            FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT),
            theme::TEXT,
        );
        let galley=ctx.fonts_mut(|fonts|fonts.layout_job(job));
        let glyph=&galley.rows[0].row.glyphs[0];
        let pointer=origin+egui::vec2(
            glyph.pos.x+glyph.advance_width*0.5,
            galley.rows[0].row.size.y*0.5
        );
        let interaction=draw(&ctx,egui::RawInput{
            events:vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton{
                    pos:pointer,button:egui::PointerButton::Primary,
                    pressed:true,modifiers:egui::Modifiers::NONE
                },
                egui::Event::PointerButton{
                    pos:pointer,button:egui::PointerButton::Primary,
                    pressed:false,modifiers:egui::Modifiers::NONE
                }
            ],
            ..Default::default()
        },raw,&spans,&mut origin,false);
        assert_eq!(interaction.hovered,None);
        assert_eq!(interaction.clicked_block,None);
    }
}
