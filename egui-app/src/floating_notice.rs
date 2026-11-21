//! Unified, layout-free transient feedback for BANDO HUB egui.
//! Never use this for empty/loading/dirty states or decisions that require a modal.
//! All coordinates are egui logical points, not OS/device pixels.

use std::{collections::HashMap, time::{Duration, Instant}};
use eframe::egui::{
    self, Context, Frame, Id, Pos2, Rect, Response, Sense, Stroke, Vec2,
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
                state.seen.remove(signal);
                // A resolved validation or recovered backend failure must
                // not leave an obsolete sticky error in the overlay.
                if state.active.as_ref().is_some_and(|n|
                    n.source==source &&
                    matches!(n.tone,Tone::Validation|Tone::Error)
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

fn estimate_width(message: &str, screen: Rect) -> f32 {
    let glyphs = message.chars().map(|c| if c.is_ascii() { 7.1 } else { 13.0 })
        .sum::<f32>();
    (glyphs + 68.0).clamp(136.0, MAX_WIDTH)
        .min((screen.width() - SCREEN_MARGIN * 2.0).max(1.0))
}

fn placement(anchor: Pos2, size: Vec2, screen: Rect) -> (Pos2, bool) {
    let left = (anchor.x - 28.0)
        .clamp(screen.left() + SCREEN_MARGIN,
            (screen.right() - size.x - SCREEN_MARGIN).max(screen.left() + SCREEN_MARGIN));
    let above = anchor.y - ANCHOR_GAP - size.y;
    let fits_above = above >= screen.top() + SCREEN_MARGIN;
    let top = if fits_above { above } else { anchor.y + ANCHOR_GAP };
    let top = top.clamp(screen.top() + SCREEN_MARGIN,
        (screen.bottom() - size.y - SCREEN_MARGIN).max(screen.top() + SCREEN_MARGIN));
    (Pos2::new(left, top), fits_above)
}

/// One top-layer overlay, measured without adding anything to table/card flow.
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
    let width = estimate_width(&active.message, screen);
    // Reserve two text lines for long feedback and a short pointer-tail.
    let estimated_lines = (active.message.chars().count() as f32 /
        ((width - 56.0) / 13.0).max(1.0)).ceil().clamp(1.0, 4.0);
    let height = (estimated_lines * 20.0 + 20.0).max(36.0);
    let (at, above) = placement(active.at, Vec2::new(width, height + 9.0), screen);
    let mut close = false;
    let layer = egui::Area::new(Id::new(NOTICE_ID).with("bubble"))
        .order(egui::Order::Tooltip)
        .fixed_pos(at)
        .movable(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(theme::FLOATING_NOTICE_BG)
                .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH, theme::BORDER))
                .corner_radius(theme::CONTROL_RADIUS)
                .inner_margin(egui::Margin::symmetric(
                    theme::FLOATING_NOTICE_PADDING_X,
                    theme::FLOATING_NOTICE_PADDING_Y
                ))
                .show(ui, |body| {
                    body.set_max_width((width - 24.0).max(40.0));
                    body.horizontal_wrapped(|line| {
                        line.spacing_mut().item_spacing.x = theme::SPACE_8;
                        let (glyph, color) = match active.tone {
                            Tone::Success => ("✓", theme::SUCCESS),
                            Tone::Info => ("i", theme::SECONDARY),
                            Tone::Validation => ("!", theme::DANGER),
                            Tone::Error => ("!", theme::DANGER),
                        };
                        line.label(egui::RichText::new(glyph).size(theme::FLOATING_NOTICE_ICON_SIZE).color(color));
                        line.add(egui::Label::new(egui::RichText::new(&active.message)
                            .size(typography::FONT_FIELD_LABEL).color(theme::TEXT)).wrap());
                        if active.tone == Tone::Error {
                            close = line.add(egui::Label::new(
                                egui::RichText::new("×").size(theme::FLOATING_NOTICE_ICON_SIZE).color(theme::SECONDARY))
                                .sense(Sense::click())).clicked();
                        }
                    });
                });
        });
    // The tail is painted in the same top-layer as the bubble, so it never
    // reserves height in the underlying table and follows the selected anchor.
    let rect = layer.response.rect;
    let tail_x = active.at.x.clamp(rect.left() + 14.0, rect.right() - 14.0);
    let painter = ctx.layer_painter(layer.response.layer_id);
    let fill = theme::FLOATING_NOTICE_BG;
    let vertices = if above {
        vec![
            Pos2::new(tail_x - 6.0, rect.bottom() - 1.0),
            Pos2::new(tail_x + 6.0, rect.bottom() - 1.0),
            Pos2::new(tail_x, rect.bottom() + 7.0),
        ]
    } else {
        vec![
            Pos2::new(tail_x - 6.0, rect.top() + 1.0),
            Pos2::new(tail_x + 6.0, rect.top() + 1.0),
            Pos2::new(tail_x, rect.top() - 7.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        vertices, fill, Stroke::new(theme::FRAME_STROKE_WIDTH, theme::BORDER)
    ));
    // This expression intentionally reads the anchor kind: the caller can
    // distinguish caret/button/section without changing visual styling.
    let _ = active.kind;
    if close {
        with_state(ctx, |state| state.active = None);
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
}
