//! Unified, layout-free transient feedback for BANDO HUB egui.
//! Never use this for empty/loading/dirty states or decisions that require a modal.
//! All coordinates are egui logical points, not OS/device pixels.

use std::{collections::HashMap, time::{Duration, Instant}};
use eframe::egui::{
    self, Context, Id, Pos2, Rect, Response, Sense, Stroke, Vec2,
};
use crate::{theme, typography};

const NOTICE_ID: &str = "bando-shared-floating-notice";
const MAX_WIDTH: f32 = theme::FLOATING_NOTICE_MAX_WIDTH;
const SCREEN_MARGIN: f32 = theme::FLOATING_NOTICE_SCREEN_MARGIN;
const ANCHOR_GAP: f32 = theme::FLOATING_NOTICE_ANCHOR_GAP;
const ANCHOR_VALID_FOR: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone { Success, Info, Validation, Error }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnchorKind { Caret, Action, Section }

#[derive(Clone)]
pub struct Anchor {
    owner:Option<egui::LayerId>,
    at: Pos2,
    below: Pos2,
    page: &'static str,
    page_epoch:u64,
    created: Instant,
    kind: AnchorKind,
}

#[derive(Clone)]
struct Notice {
    owner:Option<egui::LayerId>,
    message: String,
    tone: Tone,
    source: &'static str,
    at: Pos2,
    below: Pos2,
    kind: AnchorKind,
    created: Instant,
}

#[derive(Clone,Copy)]
struct ElementOrigin{rect:Rect,owner:Option<egui::LayerId>}

#[derive(Default, Clone)]
struct State {
    page: &'static str,
    anchors: HashMap<&'static str, Anchor>,
    sections: HashMap<&'static str, Anchor>,
    page_epoch:u64,
    seen: HashMap<&'static str, String>,
    active: Option<Notice>,
    // Transient option-click origin for the currently open shared dropdown.
    // It is never carried to a later frame or a different user action.
    dropdown_popup_trigger: Option<ElementOrigin>,
    dropdown_selected_trigger: Option<ElementOrigin>,
    last_element: Option<ElementOrigin>,
}

fn with_state<R>(ctx: &Context, f: impl FnOnce(&mut State) -> R) -> R {
    let id = Id::new(NOTICE_ID);
    ctx.data_mut(|data| {
        let mut state = data.get_temp::<State>(id).unwrap_or_default();
        let result = f(&mut state);
        data.insert_temp(id, state);
        result
    })
}

/// Call once at the beginning of each frame. Do not carry an anchor from one
/// management page to another; a delayed result must not point to unrelated UI.
pub fn begin_page(ctx: &Context, page: &'static str) {
    with_state(ctx, |state| {
        // One-shot UI dispatch information is valid for the click frame only.
        // An uncommitted filter choice must not relocate a later button notice.
        state.dropdown_popup_trigger = None;
        state.dropdown_selected_trigger = None;
        state.last_element = None;
        if state.page != page {
            state.page = page;
            state.anchors.clear();
            state.sections.clear();
            state.page_epoch=state.page_epoch.wrapping_add(1);
            // Keep per-signal dedupe state when navigating. Managers may retain
            // their last success String; revisiting a page must not replay it.
            state.active = None;
        }
    });
}

#[cfg(test)]
fn record(ctx: &Context, source: &'static str, at: Pos2, kind: AnchorKind) {
    record_element(ctx,source,Rect::from_min_max(at,at),kind);
}
fn record_element(ctx:&Context,source:&'static str,rect:Rect,kind:AnchorKind){
    record_origin(ctx,source,ElementOrigin{rect,owner:crate::overlay::modal_scope(ctx)},kind);
}
fn record_origin(ctx:&Context,source:&'static str,origin:ElementOrigin,kind:AnchorKind){
    let rect=origin.rect;
    with_state(ctx,|state|{
        let anchor=Anchor{owner:origin.owner,at:rect.center_top(),below:rect.center_bottom(),page:state.page,page_epoch:state.page_epoch,created:Instant::now(),kind};
        if kind==AnchorKind::Section{state.sections.insert(source,anchor);}
        else{state.anchors.insert(source,anchor);}
        // A new explicit action can replace a prior, persistent failure.
        if kind == AnchorKind::Action &&
            state.active.as_ref().is_some_and(|n| n.source == source) {
            state.active = None;
        }
    });
}

/// Record the original click *when it happens*, not after async Rust finishes.
pub fn clicked(ctx: &Context, source: &'static str, response: &Response) {
    if !response.clicked() { return; }
    action_response(ctx,source,response);
}

pub fn action_response(ctx:&Context,source:&'static str,response:&Response){
    let origin=response_origin(response);
    remember_origin(ctx,origin);
    record_origin(ctx,source,origin,AnchorKind::Action);
}
fn response_origin(response:&Response)->ElementOrigin{
    ElementOrigin{rect:response.rect,owner:crate::overlay::response_owner(response)}
}
fn remember_origin(ctx:&Context,origin:ElementOrigin){
    if origin.rect.is_positive(){with_state(ctx,|s|s.last_element=Some(origin));}
}
/// Custom buttons may use a release hit-test instead of Response::clicked.
pub fn action_rect(ctx:&Context,source:&'static str,rect:Rect){
    remember_element(ctx,rect);
    record_element(ctx,source,rect,AnchorKind::Action);
}
/// Shared widgets register actual interactions, never hover or pointer motion.
pub fn remember_element(ctx:&Context,rect:Rect){
    if rect.is_positive(){
        let owner=crate::overlay::modal_scope(ctx);
        with_state(ctx,|state|state.last_element=Some(ElementOrigin{rect,owner}));
    }
}
pub fn remember_response(ctx:&Context,response:&Response){
    if response.clicked() || response.changed(){remember_origin(ctx,response_origin(response));}
}
pub fn capture_origin(ctx:&Context,source:&'static str)->Option<Anchor>{
    with_state(ctx,|s|s.anchors.get(source).filter(|a|
        a.page_epoch==s.page_epoch && a.created.elapsed()<=ANCHOR_VALID_FOR).cloned())
}
pub fn inherit_origin(ctx:&Context,from:&'static str,to:&'static str){
    let origin=capture_origin(ctx,from);
    with_state(ctx,|s|{if let Some(a)=origin{s.anchors.insert(to,a);}
        else{s.anchors.remove(to);}});
}

/// The visible dropdown button, rather than a row in its popup, owns any
/// operation feedback triggered by the selected option. Set this only while
/// the shared popup is being rendered, immediately before its options.
pub fn dropdown_popup_trigger_response(response:&Response){
    let origin=response_origin(response);
    with_state(&response.ctx,|state|state.dropdown_popup_trigger=Some(origin));
}
#[cfg(test)]
fn dropdown_popup_trigger(ctx: &Context, trigger: Rect) {
    let owner=crate::overlay::modal_scope(ctx);
    with_state(ctx, |state| {
        state.dropdown_popup_trigger = Some(ElementOrigin{rect:trigger,owner});
    });
}

/// A selected common option reserves its parent dropdown's anchor for the
/// NEXT action dispatch in THIS frame. Pure filter choices have no dispatch;
/// begin_page clears their unused anchor at the next frame.
pub fn dropdown_option_selected(ctx: &Context) {
    with_state(ctx, |state| {
        state.dropdown_selected_trigger = state.dropdown_popup_trigger;
    });
}

/// For jobs dispatched by several nested buttons that share an async receiver.
/// Prefer the initiating dropdown trigger when a shared option was selected;
/// otherwise use the actual shared widget interacted with in this frame.
/// If no element is known, fall back to the feature section, never the mouse.
pub fn dispatch_pointer(ctx: &Context, source: &'static str) {
    let selected=with_state(ctx,|state|state.dropdown_selected_trigger.take().or(state.last_element));
    if let Some(origin)=selected{record_origin(ctx,source,origin,AnchorKind::Action);}
    else{with_state(ctx,|state|{state.anchors.remove(source);});}
}

/// Preserve existing caller API; anchor to the input element, not its caret.
pub fn sized_text_caret(
    ui:&egui::Ui,source:&'static str,response:&Response,
    _value:&str,_font:egui::FontId,_centered:bool,_horizontal_padding:f32,
){
    if response.changed() || response.gained_focus() || response.clicked(){
        let origin=response_origin(response);
        remember_origin(ui.ctx(),origin);
        record_origin(ui.ctx(),source,origin,AnchorKind::Caret);
    }
    if response.changed(){
        with_state(ui.ctx(),|state|{
            if state.active.as_ref().is_some_and(|n|n.source==source){state.active=None;}
        });
    }
}
pub fn text_edit_caret(
    ctx:&Context,source:&'static str,output:&egui::widgets::text_edit::TextEditOutput,
){
    let response=&output.response;
    if response.changed() || response.gained_focus() || response.clicked(){
        let origin=response_origin(response);
        remember_origin(ctx,origin);
        record_origin(ctx,source,origin,AnchorKind::Caret);
    }
}

/// Contextual automatic processing. Never show unrelated background successes
/// on the current page: the caller must supply a visible functional region.
pub fn section(ctx: &Context, source: &'static str, region: Rect) {
    record_element(ctx,source,region,AnchorKind::Section);
}

fn fallback(ctx: &Context) -> Pos2 {
    let bounds = ctx.content_rect();
    Pos2::new((bounds.right() - 180.0).max(bounds.left() + 16.0), bounds.top() + 100.0)
}

pub fn publish(ctx: &Context, source: &'static str, tone: Tone, message: impl Into<String>) {
    publish_from(ctx,source,tone,message,None);
}
/// A captured operation origin is immutable, even if later clicks occur.
pub fn publish_from(ctx:&Context,source:&'static str,tone:Tone,
    message:impl Into<String>,origin:Option<&Anchor>){
    let message = message.into();
    if message.trim().is_empty() { return; }
    let fallback_anchor = fallback(ctx);
    with_state(ctx, |state| {
        if origin.is_some_and(|a|a.page!=state.page||a.page_epoch!=state.page_epoch){return;}
        let anchor=origin.or_else(||state.anchors.get(source).filter(|a|
            a.page==state.page && a.page_epoch==state.page_epoch &&
            a.created.elapsed()<=ANCHOR_VALID_FOR
        )).or_else(||state.sections.get(source));
        let owner=anchor.and_then(|a|a.owner);
        let (at,below,kind)=anchor.map(|a|(a.at,a.below,a.kind)).unwrap_or((fallback_anchor,fallback_anchor,AnchorKind::Section));
        if state.active.as_ref().is_some_and(|n|
            n.source==source && n.tone==tone && n.message==message && n.at==at && n.owner==owner){return;}
        state.active = Some(Notice {
            owner,message, tone, source, at, below, kind, created: Instant::now(),
        });
    });
    ctx.request_repaint();
}

/// For managers that retain a notice/error String between frames. Emits only
/// once when the *value changes*, and rearms after the owner clears the value.
/// Does not consume an error needed for navigation guards or retry logic.
pub fn sync(
    ctx: &Context, signal: &'static str, source: &'static str,
    tone: Tone, message: Option<&str>,
) {
    let changed = with_state(ctx, |state| {
        match message.filter(|text| !text.trim().is_empty()) {
            Some(value) if state.seen.get(signal).is_some_and(|old| old == value) => false,
            Some(value) => {
                state.seen.insert(signal, value.to_owned());
                true
            }
            None => {
                let cleared = state.seen.remove(signal);
                // Different signals may share one action anchor (e.g. Excel
                // result and error). Clear only the error issued by THIS
                // signal, never a newer failure from a different signal.
                if state.active.as_ref().is_some_and(|n|
                    n.source==source &&
                    matches!(n.tone,Tone::Validation|Tone::Error) &&
                    cleared.as_deref()==Some(n.message.as_str())
                ) {
                    state.active=None;
                }
                false
            }
        }
    });
    if changed {
        if let Some(message) = message {
            publish(ctx, source, tone, message);
        }
    }
}

// All visible bounds come from the same egui galley we actually paint.
// This prevents the CJK/Latin/fallback-font discrepancies of character-count
// estimates and keeps the floating overlay independent of table/card layout.
fn measured_notice(
    ctx: &Context, message: &str, tone: Tone, screen: Rect,
) -> (Vec2, std::sync::Arc<egui::Galley>, f32) {
    let pad_x = f32::from(theme::FLOATING_NOTICE_PADDING_X);
    let pad_y = f32::from(theme::FLOATING_NOTICE_PADDING_Y);
    let close_slot = if tone == Tone::Error {
        theme::SPACE_8 + theme::FLOATING_NOTICE_CLOSE_HIT_SIZE
    } else { 0.0 };
    let chrome = pad_x * 2.0 + theme::FLOATING_NOTICE_ICON_SIZE
        + theme::SPACE_8 + close_slot + theme::FRAME_STROKE_WIDTH * 2.0;
    let max_width = MAX_WIDTH.min((screen.width() - SCREEN_MARGIN * 2.0).max(1.0));
    let wrap_width = (max_width - chrome).max(1.0);
    let galley = ctx.fonts_mut(|fonts| fonts.layout(
        message.to_owned(),
        typography::floating_notice_font(),
        theme::TEXT,
        wrap_width,
    ));
    let width = (galley.size().x + chrome).min(max_width);
    let content_height = galley.size().y.max(theme::FLOATING_NOTICE_ICON_SIZE)
        .max(if tone == Tone::Error {
            theme::FLOATING_NOTICE_CLOSE_HIT_SIZE
        } else { 0.0 });
    let desired_height = content_height + pad_y * 2.0
        + theme::FRAME_STROKE_WIDTH * 2.0;
    let max_height = (screen.height() - SCREEN_MARGIN * 2.0
        - theme::FLOATING_NOTICE_POINTER_HEIGHT).max(1.0);
    (Vec2::new(width, desired_height.min(max_height)), galley, content_height)
}

#[cfg(test)]
fn placement(anchor:Pos2,size:Vec2,screen:Rect,kind:AnchorKind)->(Pos2,bool){
    placement_element(anchor,anchor,size,screen,kind)
}
fn placement_element(anchor:Pos2,below:Pos2,size:Vec2,screen:Rect,_kind:AnchorKind)->(Pos2,bool){
    let min_x = screen.left() + SCREEN_MARGIN;
    let max_x = (screen.right() - SCREEN_MARGIN - size.x).max(min_x);
    // Center above or below the manipulated element, never its click point.
    let offset_x=size.x/2.0;
    let x = (anchor.x - offset_x).clamp(min_x, max_x);

    let above_y = anchor.y - ANCHOR_GAP - size.y;
    let below_y = below.y + ANCHOR_GAP;
    let room_above = anchor.y - screen.top() - SCREEN_MARGIN;
    let room_below = screen.bottom() - SCREEN_MARGIN - below.y;
    let above = above_y >= screen.top() + SCREEN_MARGIN
        || (below_y + size.y > screen.bottom() - SCREEN_MARGIN
            && room_above >= room_below);
    let min_y = screen.top() + SCREEN_MARGIN;
    let max_y = (screen.bottom() - SCREEN_MARGIN - size.y).max(min_y);
    let y = (if above { above_y } else { below_y }).clamp(min_y,max_y);
    (Pos2::new(x,y),above)
}

// Fixed 16pt vector glyphs: never use font-dependent ✓/!/i characters.
// Align to the first text row, not the middle of a multi-line explanation.
fn paint_status_icon(painter: &egui::Painter, center: Pos2, tone: Tone) {
    let ink = match tone {
        Tone::Success => theme::SUCCESS,
        Tone::Info => theme::SECONDARY,
        Tone::Validation | Tone::Error => theme::DANGER,
    };
    let stroke = Stroke::new(1.5_f32, ink);
    painter.circle_stroke(center,6.5,stroke);
    match tone {
        Tone::Success => {
            painter.line_segment([
                center + Vec2::new(-3.1,0.0),
                center + Vec2::new(-0.7,2.4),
            ],stroke);
            painter.line_segment([
                center + Vec2::new(-0.7,2.4),
                center + Vec2::new(3.5,-2.7),
            ],stroke);
        }
        Tone::Info => {
            painter.circle_filled(center + Vec2::new(0.0,-2.6),0.9,ink);
            painter.line_segment([
                center + Vec2::new(0.0,0.1),
                center + Vec2::new(0.0,3.6),
            ],stroke);
        }
        Tone::Validation | Tone::Error => {
            painter.line_segment([
                center + Vec2::new(0.0,-3.4),
                center + Vec2::new(0.0,1.0),
            ],stroke);
            painter.circle_filled(center + Vec2::new(0.0,3.7),0.9,ink);
        }
    }
}

// The arrow belongs to the body's 1pt outline, not a second bordered box.
// Paint a filled triangle, cover ONLY the body border underneath its base,
// then trace the two exposed sides. No doubled base stroke or white seam.
fn paint_tail(
    painter: &egui::Painter, rect: Rect, anchor: Pos2, above: bool,
) {
    let half = theme::FLOATING_NOTICE_POINTER_HALF_WIDTH;
    let height = theme::FLOATING_NOTICE_POINTER_HEIGHT;
    let radius = f32::from(theme::CONTROL_RADIUS);
    let tail_x = anchor.x.clamp(
        rect.left() + radius + half + 2.0,
        rect.right() - radius - half - 2.0,
    );
    let base_y = if above { rect.bottom() - 0.5 } else { rect.top() + 0.5 };
    let tip_y = if above { rect.bottom() + height } else { rect.top() - height };
    // If the screen forced the box to overlap its source, omit the pointer
    // rather than point a triangle at unrelated content.
    if (above && rect.bottom() > anchor.y - height)
        || (!above && rect.top() < anchor.y + height)
    {
        return;
    }
    let left = Pos2::new(tail_x-half,base_y);
    let right = Pos2::new(tail_x+half,base_y);
    let tip = Pos2::new(tail_x,tip_y);
    painter.add(egui::Shape::convex_polygon(
        vec![left,right,tip], theme::FLOATING_NOTICE_BG, Stroke::NONE,
    ));
    painter.line_segment([left,right],Stroke::new(
        theme::FRAME_STROKE_WIDTH + 1.5,theme::FLOATING_NOTICE_BG,
    ));
    let stroke = Stroke::new(theme::FRAME_STROKE_WIDTH, theme::BORDER);
    painter.line_segment([left,tip],stroke);
    painter.line_segment([tip,right],stroke);
}

fn paint_close(painter: &egui::Painter, center: Pos2) {
    let ink = Stroke::new(1.4_f32,theme::SECONDARY);
    painter.line_segment([
        center + Vec2::new(-3.2,-3.2),
        center + Vec2::new(3.2,3.2),
    ],ink);
    painter.line_segment([
        center + Vec2::new(-3.2,3.2),
        center + Vec2::new(3.2,-3.2),
    ],ink);
}

/// Top-layer overlay, measured by the same font/layout used for painting.
/// The popup reserves no size in the underlying fixed/virtualized tables.
pub fn show(ctx: &Context) {
    let active = with_state(ctx, |state| {
        if state.active.as_ref().is_some_and(|n| {
            let ttl = match n.tone {
                Tone::Success => Some(Duration::from_millis(2500)),
                Tone::Info => Some(Duration::from_millis(3000)),
                Tone::Validation => Some(Duration::from_millis(4000)),
                Tone::Error => None,
            };
            ttl.is_some_and(|duration| n.created.elapsed() >= duration)
        }) {
            state.active = None;
        }
        state.active.clone()
    });
    let Some(active) = active else { return };
    match active.tone {
        Tone::Success => ctx.request_repaint_after(Duration::from_millis(2500)
            .saturating_sub(active.created.elapsed())),
        Tone::Info => ctx.request_repaint_after(Duration::from_millis(3000)
            .saturating_sub(active.created.elapsed())),
        Tone::Validation => ctx.request_repaint_after(Duration::from_millis(4000)
            .saturating_sub(active.created.elapsed())),
        Tone::Error => {},
    }

    if !crate::overlay::notice_visible(ctx,active.owner){return;}

    // Preserve original notice identity for sync/clear and deduplication.
    let display=if matches!(active.tone,Tone::Error|Tone::Validation){
        crate::user_error::display(active.source,&active.message)
    }else{std::borrow::Cow::Borrowed(active.message.as_str())};
    let screen = ctx.content_rect();
    let (size, galley, _content_height) = measured_notice(
        ctx, display.as_ref(), active.tone, screen,
    );
    let (at,above)=placement_element(active.at,active.below,size,screen,active.kind);
    let pad_x = f32::from(theme::FLOATING_NOTICE_PADDING_X);
    let pad_y = f32::from(theme::FLOATING_NOTICE_PADDING_Y);
    let mut close = false;
    crate::overlay::area(Id::new(NOTICE_ID).with("bubble"),crate::overlay::AreaRole::Notice)
        .fixed_pos(at)
        .movable(false)
        .interactable(active.tone==Tone::Error || galley.size().y > size.y-2.0*pad_y)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size,Sense::hover());
            let painter = ui.painter().clone();
            // Soft 3pt/down shadow separates overlapping fields from their
            // floating result, without changing the approved grey palette.
            painter.add(egui::epaint::Shadow{
                offset:theme::FLOATING_NOTICE_SHADOW_OFFSET,
                blur:theme::FLOATING_NOTICE_SHADOW_BLUR,
                spread:0,
                color:egui::Color32::from_black_alpha(
                    theme::FLOATING_NOTICE_SHADOW_ALPHA
                ),
            }.as_shape(rect,theme::CONTROL_RADIUS));
            theme::paint_bordered_surface(&painter,rect,theme::CONTROL_RADIUS,theme::FLOATING_NOTICE_BG,theme::BORDER);
            paint_tail(&painter,rect,if above{active.at}else{active.below},above);
            // All rows use the same notice font; average laid-out row height
            // is stable even when egui has wrapped a long message.
            let first_row_height = galley.size().y /
                galley.rows.len().max(1) as f32;
            let first_center = rect.top() + pad_y + first_row_height.max(
                theme::FLOATING_NOTICE_ICON_SIZE
            )/2.0;
            paint_status_icon(&painter,Pos2::new(
                rect.left()+pad_x + theme::FLOATING_NOTICE_ICON_SIZE/2.0,
                first_center
            ),active.tone);
            let text_left = rect.left() + pad_x
                + theme::FLOATING_NOTICE_ICON_SIZE + theme::SPACE_8;
            let text_top = rect.top() + pad_y;
            let close_slot = if active.tone==Tone::Error {
                theme::FLOATING_NOTICE_CLOSE_HIT_SIZE + theme::SPACE_8
            } else { 0.0 };
            let text_right = rect.right() - pad_x - close_slot;
            let text_visible_height = (rect.bottom() - pad_y - text_top)
                .max(1.0);
            let text_rect = Rect::from_min_size(
                Pos2::new(text_left,text_top),
                Vec2::new((text_right - text_left).max(1.0),text_visible_height),
            );
            if galley.size().y <= text_visible_height {
                painter.galley(text_rect.min,galley.clone(),theme::TEXT);
            } else {
                // Only exceptionally long failure details scroll INSIDE the
                // floating box. Never increase the underlying page height.
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("floating-notice-detail-scroll")
                        .max_rect(text_rect),
                    |inner| {
                        inner.set_clip_rect(text_rect);
                        inner.set_width(text_rect.width());
                        crate::components::scroll_area(crate::components::ScrollAxes::Vertical)
                            .id_salt("floating-notice-scroll")
                            .auto_shrink([false,false])
                            .max_height(text_rect.height())
                            .show(inner,|content| {
                                content.set_width(text_rect.width().max(1.0));
                                content.add(egui::Label::new(
                                    typography::floating_notice_text(display.as_ref())
                                ).wrap());
                            });
                    },
                );
            }
            if active.tone == Tone::Error {
                let side=theme::FLOATING_NOTICE_CLOSE_HIT_SIZE;
                let center=Pos2::new(
                    rect.right()-pad_x-side/2.0,
                    rect.top()+pad_y+side/2.0,
                );
                let hit=Rect::from_center_size(center,Vec2::splat(side));
                let response=ui.interact(
                    hit,Id::new(NOTICE_ID).with("close"),Sense::click()
                );
                theme::paint_button_hover(ui,&response,4.0);
                paint_close(&painter,center);
                close=response.clicked();
            }
        });
    if close {
        with_state(ctx,|state|state.active=None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_origin_survives_section_repaint_and_later_action(){
        let ctx=Context::default();
        begin_page(&ctx,"Reception");
        let first=Pos2::new(300.0,220.0);
        record(&ctx,"test.action",first,AnchorKind::Action);
        section(&ctx,"test.action",Rect::from_center_size(Pos2::new(30.0,50.0),Vec2::splat(20.0)));
        publish(&ctx,"test.action",Tone::Success,"saved");
        assert_eq!(with_state(&ctx,|s|s.active.as_ref().unwrap().at),first);
        let origin=capture_origin(&ctx,"test.action").unwrap();
        record(&ctx,"test.action",Pos2::new(600.0,400.0),AnchorKind::Action);
        publish_from(&ctx,"test.action",Tone::Success,"first saved",Some(&origin));
        assert_eq!(with_state(&ctx,|s|s.active.as_ref().unwrap().at),first);
        begin_page(&ctx,"History");
        begin_page(&ctx,"Reception");
        publish_from(&ctx,"test.action",Tone::Success,"old saved",Some(&origin));
        assert!(with_state(&ctx,|s|s.active.is_none()));
    }
    #[test]
    fn dropdown_selection_feedback_is_anchored_to_trigger_not_menu_row(){
        let ctx=Context::default();
        begin_page(&ctx,"Freight");
        let trigger=Rect::from_min_size(
            Pos2::new(440.0,120.0),Vec2::new(160.0,34.0)
        );
        dropdown_popup_trigger(&ctx,trigger);
        dropdown_option_selected(&ctx);
        dispatch_pointer(&ctx,"freight.action");
        publish(&ctx,"freight.action",Tone::Success,"배송 그룹 변경 완료");
        let notice=with_state(&ctx,|state|state.active.clone()).unwrap();
        assert_eq!(notice.at,trigger.center_top());
        assert_eq!(notice.kind,AnchorKind::Action);
    }

    #[test]
    fn uncommitted_dropdown_selection_never_reanchors_a_later_action(){
        let ctx=Context::default();
        begin_page(&ctx,"Freight");
        dropdown_popup_trigger(&ctx,Rect::from_min_size(
            Pos2::new(440.0,120.0),Vec2::new(160.0,34.0)
        ));
        dropdown_option_selected(&ctx);
        begin_page(&ctx,"Freight");
        assert!(with_state(&ctx,|state|state.dropdown_selected_trigger.is_none()));
        assert!(with_state(&ctx,|state|state.dropdown_popup_trigger.is_none()));
    }

    #[test]
    fn element_anchor_ignores_mouse_and_uses_bottom_edge_when_flipped(){
        let ctx=Context::default();
        begin_page(&ctx,"Reception");
        let rect=Rect::from_min_size(Pos2::new(120.0,10.0),Vec2::new(100.0,36.0));
        remember_element(&ctx,rect);
        dispatch_pointer(&ctx,"test.action");
        publish(&ctx,"test.action",Tone::Validation,"input");
        let n=with_state(&ctx,|s|s.active.clone()).unwrap();
        assert_eq!(n.at,rect.center_top());
        assert_eq!(n.below,rect.center_bottom());
        let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(500.0,350.0));
        let (pos,above)=placement_element(n.at,n.below,Vec2::new(160.0,40.0),screen,n.kind);
        assert!(!above);
        assert!(pos.y>=rect.bottom()+ANCHOR_GAP);
        assert_eq!(pos.x+80.0,rect.center().x);
    }
    #[test]
    fn notice_flips_when_there_is_no_room_above() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 350.0));
        let (pos, above) = placement(Pos2::new(320.0, 12.0), Vec2::new(200.0, 40.0), screen, AnchorKind::Action);
        assert!(!above);
        assert!(pos.y > 12.0);
        assert!(pos.x + 200.0 <= screen.right() - SCREEN_MARGIN + 0.01);
    }
    #[test]
    fn notice_prefers_caret_above() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 350.0));
        let (pos, above) = placement(Pos2::new(100.0, 200.0), Vec2::new(180.0, 40.0), screen, AnchorKind::Caret);
        assert!(above);
        assert!(pos.y + 40.0 < 200.0);
    }
    #[test]
    fn measured_korean_and_latin_message_wraps_without_exceeding_maximum() {
        let ctx=Context::default();
        let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(900.0,620.0));
        let mut outcome=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            let message="상품 가격이 저장되지 않았습니다. 거래처 기준과 기존 상품 분류를 확인한 후 다시 시도하세요. ".repeat(6)
                + "detail: RECONCILIATION_CONFLICT";
            let (size,galley,_) = measured_notice(ctx,&message,Tone::Error,screen);
            outcome=Some((size,galley.rows.len()));
        });
        let (size,rows)=outcome.expect("measured inside the font pass");
        assert!(size.x<=theme::FLOATING_NOTICE_MAX_WIDTH +0.01);
        assert!(size.y<=screen.height() - 2.0*SCREEN_MARGIN);
        assert!(rows>1,"long notice must wrap based on actual glyph widths");
    }
    #[test]
    fn narrow_viewport_keeps_notice_rect_inside_screen() {
        let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(240.0,190.0));
        let (pos,_)=placement(
            Pos2::new(238.0,185.0),Vec2::new(220.0,70.0),screen,
            AnchorKind::Section
        );
        assert!(pos.x>=screen.left()+SCREEN_MARGIN);
        assert!(pos.y>=screen.top()+SCREEN_MARGIN);
        assert!(pos.x +220.0<=screen.right()-SCREEN_MARGIN+0.01);
        assert!(pos.y +70.0<=screen.bottom()-SCREEN_MARGIN+0.01);
    }
    #[test]
    fn clearing_success_signal_does_not_hide_a_new_save_failure() {
        let ctx = Context::default();
        begin_page(&ctx,"Excel");
        sync(&ctx,"excel.result","excel.action",Tone::Success,Some("저장했습니다."));
        sync(&ctx,"excel.error","excel.action",Tone::Error,Some("저장에 실패했습니다."));
        sync(&ctx,"excel.result","excel.action",Tone::Success,None);
        let active=with_state(&ctx,|state|state.active.clone()).expect("error persists");
        assert_eq!(active.tone,Tone::Error);
        assert_eq!(active.message,"저장에 실패했습니다.");
        sync(&ctx,"excel.error","excel.action",Tone::Error,None);
        assert!(with_state(&ctx,|state|state.active.is_none()));
    }
    #[test]
    fn page_revisit_does_not_replay_old_success() {
        let ctx = Context::default();
        begin_page(&ctx,"Products");
        sync(&ctx,"product.result","products.action",Tone::Success,Some("가격 저장 완료"));
        begin_page(&ctx,"History");
        begin_page(&ctx,"Products");
        sync(&ctx,"product.result","products.action",Tone::Success,Some("가격 저장 완료"));
        assert!(with_state(&ctx,|state|state.active.is_none()));
    }
}
