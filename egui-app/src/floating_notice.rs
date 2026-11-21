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
struct Anchor {
    at: Pos2,
    page: &'static str,
    created: Instant,
    kind: AnchorKind,
}

#[derive(Clone)]
struct Notice {
    message: String,
    tone: Tone,
    source: &'static str,
    at: Pos2,
    kind: AnchorKind,
    created: Instant,
}

#[derive(Default, Clone)]
struct State {
    page: &'static str,
    anchors: HashMap<&'static str, Anchor>,
    seen: HashMap<&'static str, String>,
    active: Option<Notice>,
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
        if state.page != page {
            state.page = page;
            state.anchors.clear();
            // Keep per-signal dedupe state when navigating. Managers may retain
            // their last success String; revisiting a page must not replay it.
            state.active = None;
        }
    });
}

fn record(ctx: &Context, source: &'static str, at: Pos2, kind: AnchorKind) {
    with_state(ctx, |state| {
        state.anchors.insert(source, Anchor {
            at, page: state.page, created: Instant::now(), kind,
        });
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
    let position = ctx.input(|i| i.pointer.latest_pos())
        .filter(|pos| response.rect.contains(*pos))
        .unwrap_or(response.rect.center());
    record(ctx, source, position, AnchorKind::Action);
}

/// For jobs dispatched by several nested buttons that all share an async
/// receiver. Capture the *dispatch-time* pointer, not the later completion-time
/// pointer. Explicit response-based capture remains preferable where available.
pub fn dispatch_pointer(ctx: &Context, source: &'static str) {
    let at = ctx.input(|i| i.pointer.latest_pos()).unwrap_or_else(|| fallback(ctx));
    record(ctx, source, at, AnchorKind::Action);
}

/// Input fallback (for fields that only expose a Response, not TextEditOutput).
/// A field's editing point is preferred over the outer card or table center.
pub fn input(ctx: &Context, source: &'static str, response: &Response) {
    if response.has_focus() || response.changed() {
        let position = ctx.input(|i| i.pointer.latest_pos())
            .filter(|pos| response.rect.contains(*pos))
            .unwrap_or(response.rect.center_top());
        if response.changed() {
            with_state(ctx, |state| {
                if state.active.as_ref().is_some_and(|n| n.source==source) {
                    state.active = None;
                }
            });
        }
        record(ctx, source, position, AnchorKind::Caret);
    }
}

/// Caret anchor for existing add_sized(TextEdit) widgets, without changing
/// their proven 34/36pt geometry. Recover the TextEdit cursor from the widget
/// id and measure with the exact font used by that input.
/// Clamps long/scrolled text back inside the visible field.
pub fn sized_text_caret(
    ui: &egui::Ui, source: &'static str, response: &Response,
    value: &str, font: egui::FontId, centered: bool, horizontal_padding: f32,
) {
    if !response.has_focus() && !response.changed() { return; }
    let caret = egui::TextEdit::load_state(ui.ctx(), response.id)
        .and_then(|state| state.cursor.char_range())
        .map(|range| {
            let galley = ui.fonts_mut(|f| f.layout_no_wrap(
                value.to_owned(), font, theme::TEXT
            ));
            let offset = galley.pos_from_cursor(range.primary);
            let text_left = if centered {
                response.rect.center().x - galley.size().x / 2.0
            } else {
                response.rect.left() + horizontal_padding
            };
            let x = (text_left + offset.left())
                .clamp(response.rect.left() + 3.0, response.rect.right() - 3.0);
            Pos2::new(x, response.rect.center().y - 9.0)
        })
        .unwrap_or(response.rect.center_top());
    if response.changed() {
        with_state(ui.ctx(), |state| {
            if state.active.as_ref().is_some_and(|n| n.source==source) {
                state.active = None;
            }
        });
    }
    record(ui.ctx(), source, caret, AnchorKind::Caret);
}

/// Exact caret location, from TextEditOutput::galley/galley_pos/cursor_range.
pub fn text_edit_caret(
    ctx: &Context, source: &'static str, output: &egui::widgets::text_edit::TextEditOutput,
) {
    if !output.response.has_focus() { return; }
    let caret = output.cursor_range.map(|range|
        output.galley.pos_from_cursor(range.primary)
            .translate(output.galley_pos.to_vec2())
    );
    let at = caret.map(|r| r.center_top())
        .unwrap_or(output.response.rect.center_top());
    record(ctx, source, at, AnchorKind::Caret);
}

/// Contextual automatic processing. Never show unrelated background successes
/// on the current page: the caller must supply a visible functional region.
pub fn section(ctx: &Context, source: &'static str, region: Rect) {
    record(ctx, source, region.center_top(), AnchorKind::Section);
}

fn fallback(ctx: &Context) -> Pos2 {
    let bounds = ctx.screen_rect();
    Pos2::new((bounds.right() - 180.0).max(bounds.left() + 16.0), bounds.top() + 100.0)
}

pub fn publish(ctx: &Context, source: &'static str, tone: Tone, message: impl Into<String>) {
    let message = message.into();
    if message.trim().is_empty() { return; }
    let fallback_anchor = fallback(ctx);
    with_state(ctx, |state| {
        if state.active.as_ref().is_some_and(|n|
            n.source == source && n.tone == tone && n.message == message
        ) {
            return;
        }
        let anchor = state.anchors.get(source).filter(|a|
            a.page == state.page && a.created.elapsed() <= ANCHOR_VALID_FOR
        );
        let (at, kind) = anchor.map(|a| (a.at, a.kind))
            .unwrap_or((fallback_anchor, AnchorKind::Section));
        state.active = Some(Notice {
            message, tone, source, at, kind, created: Instant::now(),
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
        egui::FontId::proportional(theme::FLOATING_NOTICE_TEXT_SIZE),
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

fn placement(anchor: Pos2, size: Vec2, screen: Rect) -> (Pos2, bool) {
    let min_x = screen.left() + SCREEN_MARGIN;
    let max_x = (screen.right() - SCREEN_MARGIN - size.x).max(min_x);
    let x = (anchor.x - 28.0).clamp(min_x, max_x);

    let above_y = anchor.y - ANCHOR_GAP - size.y;
    let below_y = anchor.y + ANCHOR_GAP;
    let room_above = anchor.y - screen.top() - SCREEN_MARGIN;
    let room_below = screen.bottom() - SCREEN_MARGIN - anchor.y;
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
    let stroke = Stroke::new(1.5, ink);
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
    let ink = Stroke::new(1.4,theme::SECONDARY);
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

    let screen = ctx.screen_rect();
    let (size, galley, _content_height) = measured_notice(
        ctx, &active.message, active.tone, screen,
    );
    let (at, above) = placement(active.at, size, screen);
    let pad_x = f32::from(theme::FLOATING_NOTICE_PADDING_X);
    let pad_y = f32::from(theme::FLOATING_NOTICE_PADDING_Y);
    let mut close = false;
    egui::Area::new(Id::new(NOTICE_ID).with("bubble"))
        .order(egui::Order::Tooltip)
        .fixed_pos(at)
        .movable(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size,Sense::hover());
            let painter = ui.painter();
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
            painter.rect(
                rect,theme::CONTROL_RADIUS,theme::FLOATING_NOTICE_BG,
                Stroke::new(theme::FRAME_STROKE_WIDTH,theme::BORDER),
                egui::StrokeKind::Inside,
            );
            paint_tail(painter,rect,active.at,above);
            let first_row_height = galley.rows.first().map(|row|row.rect.height())
                .unwrap_or(theme::FLOATING_NOTICE_TEXT_SIZE + 5.0);
            let first_center = rect.top() + pad_y + first_row_height.max(
                theme::FLOATING_NOTICE_ICON_SIZE
            )/2.0;
            paint_status_icon(painter,Pos2::new(
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
                        egui::ScrollArea::vertical()
                            .id_salt("floating-notice-scroll")
                            .auto_shrink([false,false])
                            .max_height(text_rect.height())
                            .show(inner,|content| {
                                content.set_width(text_rect.width().max(1.0));
                                content.add(egui::Label::new(
                                    egui::RichText::new(&active.message)
                                        .size(theme::FLOATING_NOTICE_TEXT_SIZE)
                                        .color(theme::TEXT)
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
                paint_close(painter,center);
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
    fn notice_flips_when_there_is_no_room_above() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 350.0));
        let (pos, above) = placement(Pos2::new(320.0, 12.0), Vec2::new(200.0, 40.0), screen);
        assert!(!above);
        assert!(pos.y > 12.0);
        assert!(pos.x + 200.0 <= screen.right() - SCREEN_MARGIN + 0.01);
    }
    #[test]
    fn notice_prefers_caret_above() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 350.0));
        let (pos, above) = placement(Pos2::new(100.0, 200.0), Vec2::new(180.0, 40.0), screen);
        assert!(above);
        assert!(pos.y + 40.0 < 200.0);
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
