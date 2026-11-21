//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, Sense, Stroke, Ui, Vec2};
use chrono::{Datelike, Duration as Days, Months, NaiveDate};
use crate::{theme, typography};

pub const LABEL_WIDTH: f32 = theme::FIELD_LABEL_WIDTH;
pub const TABLE_ROW_HEIGHT: f32 = theme::MANAGEMENT_COMPACT_ROW_HEIGHT;
/// Render only the rows intersecting a scroll viewport, while reserving the
/// full logical height and width of the list. This works with BOTH-axis
/// scrolling, unlike a paginated list, and preserves the backing row indices.
///
/// Each caller must render each row at exactly `row_height` with no vertical
/// spacing; widgets/edits must be keyed by stable entity IDs rather than by
/// which row happened to be visible in the current frame.
pub fn virtual_fixed_rows<R>(
    scroll:egui::ScrollArea,
    ui:&mut Ui,
    content_width:f32,
    row_height:f32,
    total_rows:usize,
    add_visible:impl FnOnce(&mut Ui,std::ops::Range<usize>)->R,
) -> egui::containers::scroll_area::ScrollAreaOutput<R> {
    debug_assert!(row_height>0.0 && row_height.is_finite());
    scroll.show_viewport(ui,move |body,viewport|{
        body.spacing_mut().item_spacing.y=0.0;
        body.set_min_width(content_width);
        body.set_min_height(row_height*total_rows as f32);
        let first=((viewport.top()/row_height).floor().max(0.0) as usize)
            .saturating_sub(1).min(total_rows);
        let last=((viewport.bottom()/row_height).ceil().max(0.0) as usize)
            .saturating_add(1).min(total_rows);
        let top=body.max_rect().top()+first as f32*row_height;
        let visible_rect=egui::Rect::from_min_size(
            egui::pos2(body.max_rect().left(),top),
            egui::vec2(content_width,(last-first) as f32*row_height),
        );
        let clip=body.clip_rect();
        body.scope_builder(
            egui::UiBuilder::new()
                .id_salt("virtual-fixed-rows")
                .max_rect(visible_rect),
            |visible|{
                visible.spacing_mut().item_spacing.y=0.0;
                visible.set_clip_rect(clip);
                visible.skip_ahead_auto_ids(first);
                add_visible(visible,first..last)
            }
        ).inner
    })
}

pub const TABLE_HEADER_HEIGHT: f32 = theme::MANAGEMENT_TABLE_HEADER_HEIGHT;

#[derive(Clone, Copy)]
pub enum WorkflowIcon {
    Collapsed,
    Expanded,
    PreviousStage,
    Trash,
}

/// Neutral secondary actions inside a workflow card share one compact
/// Standard-size control. Keep their existing disabled functionality.
#[derive(Clone, Copy)]
pub enum WorkflowAuxiliaryAction {
    Manage,
    Delete,
    PreviousStage,
}

/// A single painted/interactive frame for the three grey workflow controls.
/// The two glyphs and the text label are drawn only after the same disabled
/// egui Button has reserved the same 34pt height, 6pt radius and 1pt border.
pub fn workflow_auxiliary_button(ui: &mut Ui, action: WorkflowAuxiliaryAction) -> Response {
    let width = match action {
        WorkflowAuxiliaryAction::Manage => theme::WORKFLOW_MANAGE_WIDTH,
        WorkflowAuxiliaryAction::Delete | WorkflowAuxiliaryAction::PreviousStage =>
            theme::WORKFLOW_TRASH_WIDTH,
    };
    let response = ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.add_enabled(
            false,
            egui::Button::new("")
                .min_size(Vec2::new(width, theme::WORKFLOW_AUX_CONTROL_HEIGHT))
                .fill(theme::RAISED)
                .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH, theme::BORDER))
                .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS)),
        )
    }).inner;

    match action {
        WorkflowAuxiliaryAction::Manage => {
            ui.painter().text(
                response.rect.center(),
                egui::Align2::CENTER_CENTER,
                "관리",
                egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
                theme::MUTED,
            );
        }
        WorkflowAuxiliaryAction::Delete => paint_workflow_icon(
            ui.painter(), response.rect.center(), WorkflowIcon::Trash, theme::MUTED,
        ),
        WorkflowAuxiliaryAction::PreviousStage => paint_workflow_icon(
            ui.painter(), response.rect.center(), WorkflowIcon::PreviousStage, theme::MUTED,
        ),
    }
    response
}

/// Draw workflow glyphs as stable vectors; the UI font may not contain
/// Unicode chevrons, trash or curved-arrow symbols on Windows.
pub fn paint_workflow_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    icon: WorkflowIcon,
    ink: Color32,
) {
    let stroke = Stroke::new(1.55, ink);
    let p = |x: f32, y: f32| egui::pos2(center.x + x, center.y + y);
    match icon {
        WorkflowIcon::Collapsed => {
            paint_chevron_at_angle(painter,center,0.0,0.0,ink);
        }
        WorkflowIcon::Expanded => {
            paint_chevron_at_angle(
                painter,center,std::f32::consts::FRAC_PI_2,1.0,ink
            );
        }
        WorkflowIcon::PreviousStage => {
            painter.line_segment([p(-5.3, -1.6), p(-1.5, -5.2)], stroke);
            painter.line_segment([p(-5.3, -1.6), p(-1.0, 1.8)], stroke);
            painter.add(egui::Shape::line(
                vec![p(-4.4,-1.6), p(1.2,-1.6), p(4.2,1.0),p(4.2,5.0)],
                stroke
            ));
        }
        WorkflowIcon::Trash => {
            for (a,b) in [
                (p(-5.0,-4.1),p(5.0,-4.1)),
                (p(-2.9,-6.9),p(2.9,-6.9)),
                (p(-4.3,-1.5),p(-3.4,6.2)),
                (p(4.3,-1.5),p(3.4,6.2)),
                (p(-3.4,6.2),p(3.4,6.2)),
                (p(-1.3,-1.2),p(-1.3,3.9)),
                (p(1.3,-1.2),p(1.3,3.9))
            ] {painter.line_segment([a,b],stroke);}
        }
    }
}

/// A compact KakaoTalk identity mark, used only as a visual menu prefix.
/// The native Kakao HWND and order-matching logic remain unchanged.
pub fn kakao_menu_icon(ui:&mut Ui) -> Response {
    let side=theme::KAKAO_MENU_ICON_SIZE;
    let (rect,response)=ui.allocate_exact_size(Vec2::splat(side),Sense::hover());
    let painter=ui.painter();
    painter.rect_filled(rect,4.0,theme::KAKAO_ICON_YELLOW);
    let bubble=Rect::from_min_max(
        egui::pos2(rect.left()+3.6,rect.top()+4.7),
        egui::pos2(rect.right()-3.6,rect.bottom()-5.8),
    );
    painter.rect_filled(bubble,5.0,theme::KAKAO_ICON_INK);
    painter.add(egui::Shape::convex_polygon(vec![
        egui::pos2(rect.left()+6.0,rect.bottom()-6.5),
        egui::pos2(rect.left()+5.3,rect.bottom()-3.0),
        egui::pos2(rect.left()+9.2,rect.bottom()-6.3),
    ],theme::KAKAO_ICON_INK,Stroke::NONE));
    for dx in [-2.8,0.0,2.8] {
        painter.circle_filled(
            egui::pos2(rect.center().x+dx,rect.center().y-0.8),
            0.75,theme::KAKAO_ICON_YELLOW
        );
    }
    response.on_hover_text("카카오톡")
}

/// Keep paired reception actions at their content widths. Available pane
/// space stays empty rather than stretching two buttons to the pane edges.
/// The row wraps only if Korean labels cannot fit without clipping.
pub fn paired_actions(
    ui: &mut Ui,
    left: (&str, theme::ButtonRole, bool),
    right: (&str, theme::ButtonRole, bool),
) -> (Response, Response) {
    ui.horizontal_wrapped(|ui| {
        (
            theme::action(ui, left.0, left.1, left.2),
            theme::action(ui, right.0, right.1, right.2),
        )
    }).inner
}


/// The actual content space in a fixed-height docked section. Reserve
/// exactly the existing top/bottom frame padding, title, divider and gaps.
/// Section widgets must not impose independent minimums that can spill
/// across the source/message splitter.
pub fn docked_section_body_height(section_height: f32) -> f32 {
    // Must match main.rs::section_with_header and docked_pane_frame.
    // The compact title releases space to the body without changing the
    // independently sized pane rectangles or introducing new scrollbars.
    let chrome = theme::RECEPTION_PANE_PADDING_Y * 2.0
        + theme::SECTION_HEADER_HEIGHT
        + theme::SECTION_CONTENT_GAP
        + theme::SOFT_DIVIDER_WIDTH
        + theme::SPACE_4;
    (section_height - chrome).max(0.0)
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKind {
    Text,
    Telephone,
    Quantity,
    Money,
    Address,
}

impl FieldKind {
    pub fn preferred_width(self) -> f32 {
        match self {
            Self::Text => 180.0,
            Self::Telephone => 142.0,
            Self::Quantity => 68.0,
            Self::Money => 112.0,
            Self::Address => 320.0,
        }
    }
}

/// Native modal geometry belongs here, not to individual screens.
/// `work_area` excludes the externally foregrounded KakaoTalk dock.
pub fn modal_window(
    title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    egui::Window::new(typography::modal_title(title))
        .title_bar(false)
        .id(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .fixed_pos(egui::pos2(
            work_area.center().x,
            // The work area starts below the 62pt header; its geometric
            // midpoint is therefore lower than the viewport's visual center.
            // Keep the Kakao-safe horizontal center, but center vertically
            // against the full application window.
            work_area.center().y - theme::HEADER_HEIGHT / 2.0,
        ))
        .constrain_to(work_area)
        .max_size(work_area.size())
        // All app dialogs are fixed at the approved Kakao-safe center.
        // Scrolling inside a dialog is allowed; dragging and edge-resizing
        // the dialog itself are not. Individual callers must not override it.
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .frame(theme::modal_frame())
}

/// The single shared title row for all native egui dialogs.
/// Close/Cancel belongs in the TOP-RIGHT, never repeated beside the
/// bottom-right confirmation action. Views use "닫기"; editable dialogs
/// and decisions use "취소". The caller retains its original close guard.
pub fn modal_header(ui:&mut Ui,title:&str,close_label:&str,enabled:bool)->bool{
    let mut close=false;
    ui.horizontal(|bar|{
        bar.label(typography::modal_title(title));
        bar.with_layout(Layout::right_to_left(Align::Center),|right|{
            close=theme::action(right,close_label,theme::ButtonRole::Standard,
                enabled).clicked();
        });
    });
    ui.add_space(theme::SPACE_8);
    close
}

pub fn decision_window(
    title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    modal_window(title,id,work_area)
        .resizable(false)
        .default_width(theme::MODAL_DECISION_MAX_WIDTH.min(work_area.width()))
}

/// Draw the dim over the ENTIRE application viewport, including its header
/// and KakaoTalk reserved pane. Only the actual dialog remains centered in
/// the Kakao-safe work area; do not use that area to clip the backdrop.
/// Render before its dialog; nested confirmations draw one more dim layer.
pub fn modal_backdrop(
    ctx:&egui::Context,id:&'static str,nested:bool
) {
    let viewport=ctx.screen_rect();
    let alpha=if nested {
        theme::MODAL_NESTED_BACKDROP_ALPHA
    } else {
        theme::MODAL_BACKDROP_ALPHA
    };
    let area=egui::Area::new(egui::Id::new(id))
        .order(if nested {egui::Order::Foreground}else{egui::Order::Middle})
        .fixed_pos(viewport.min)
        .movable(false)
        // The backdrop cannot rise above the dialog after an outside click.
        .interactable(false)
        .sense(egui::Sense::hover());
    // The modal layer prevents background input; visual coverage is full-screen.
    let layer=area.layer();
    ctx.memory_mut(|memory|memory.set_modal_layer(layer));
    area.show(ctx,|ui|{
        let (rect,_)=ui.allocate_exact_size(
            viewport.size(),egui::Sense::hover()
        );
        ui.painter().rect_filled(
            rect,0.0,egui::Color32::from_black_alpha(alpha)
        );
    });
}

/// Uses the same height, font and focus styling for every single-line field.
/// Disabled preview controls cannot edit business data.
pub fn field(ui: &mut Ui, value: &mut String, kind: FieldKind, enabled: bool) -> Response {
    let width = kind.preferred_width().min(ui.available_width().max(60.0));
    let editor_font = match kind {
        FieldKind::Money | FieldKind::Quantity => {
            egui::FontSelection::FontId(typography::numeric_editor_font())
        }
        // Preserve the regular Korean body face for names, addresses,
        // telephone and free-form text. Only explicitly numeric data
        // fields use the Consolas / monospace family.
        _ => egui::FontSelection::Style(egui::TextStyle::Body),
    };
    let field = egui::TextEdit::singleline(value)
        .font(editor_font)
        .vertical_align(Align::Center)
        .margin(egui::Margin::symmetric(8, 2))
        .desired_width(width);
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(Vec2::new(width, theme::INPUT_HEIGHT), field)
    }).inner
}

/// UI-only money input: same 36pt control and Consolas font as `field`.
/// Uses the React MoneyInput rule: digits (including zero) or an empty
/// unconfirmed amount, with thousands separators only when unfocused.
/// Invalid keystrokes never become business rules; the Rust command still
/// validates every saved number.
pub fn parse_money_input(raw:&str)->Option<i64>{
    let digits=raw.trim().replace(',',"");
    if digits.is_empty() || !digits.bytes().all(|b|b.is_ascii_digit()){
        return None;
    }
    digits.parse::<i64>().ok()
        .filter(|value|*value<=9_007_199_254_740_991)
}
pub fn money_field(ui:&mut Ui,value:&mut String,enabled:bool)->Response {
    let before=value.clone();
    let response=field(ui,value,FieldKind::Money,enabled);
    if response.changed() {
        let digits=value.replace(',',"");
        if digits.is_empty() || parse_money_input(&digits).is_some(){
            *value=digits;
        }else{
            *value=before;
        }
    }else if !response.has_focus(){
        if let Some(amount)=parse_money_input(value) {
            *value=display_won(amount).trim_end_matches('원').to_owned();
        }
    }
    response
}

/// Shared single-line free-text editor for inline manual cells and search.
/// Uses the same font, internal margin and height as the standard field role.
pub fn flexible_text_field(
    ui:&mut Ui, value:&mut String, width:f32, placeholder:&str
)->Response {
    let width=width.min(ui.available_width().max(60.0));
    let edit=egui::TextEdit::singleline(value)
        .font(egui::TextStyle::Body)
        .vertical_align(Align::Center)
        .margin(egui::Margin::symmetric(8,2))
        .hint_text(placeholder)
        .desired_width(width);
    ui.add_sized(Vec2::new(width,theme::INPUT_HEIGHT),edit)
}

/// Shared compact spreadsheet cell. Selection is an inset border;
/// hovered cells do not resize the row, and a selected cell does not
/// borrow the main navigation style.
pub fn manual_table_cell(ui:&mut Ui,value:&str,width:f32,selected:bool)->Response {
    let label=if value.trim().is_empty(){"미입력"}else{value};
    let border=if selected {
        Stroke::new(theme::DOCK_SPLITTER_HOVER_LINE,theme::SELECTED_ORDER)
    }else{
        // Two adjacent cells share one visual divider. Never create a
        // double 2pt border on every table column.
        Stroke::new(theme::SOFT_DIVIDER_WIDTH/2.0,theme::DIVIDER)
    };
    ui.add_sized(
        Vec2::new(width,theme::MANUAL_TABLE_ROW_HEIGHT),
        egui::Button::new(typography::table_cell(label))
            .truncate()
            .fill(if selected{theme::RAISED}else{theme::INPUT})
            .stroke(border)
            .corner_radius(0),
    )
}
pub fn manual_table_heading(ui:&mut Ui,label:&str,width:f32){
    ui.add_sized(
        Vec2::new(width,theme::MANUAL_TABLE_HEADER_HEIGHT),
        egui::Label::new(typography::field_label(label)),
    );
}

/// A field group's label and editor share the same baseline and label width.
pub fn labeled_field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    kind: FieldKind,
    enabled: bool,
) -> Response {
    ui.horizontal(|ui| {
        let title=ui.add_sized(
            Vec2::new(LABEL_WIDTH, theme::INPUT_HEIGHT),
            egui::Label::new(typography::field_label(label)),
        );
        // The whole field row (including its label) is a single hover
        // target for exact source linking. No geometry is modified.
        let input=field(ui, value, kind, enabled);
        title.union(input)
    }).inner
}

/// Every recipient field shares one 36pt baseline. The two telephone
/// editors occupy the same FULL-WIDTH row: mobile begins at the same x
/// coordinate as the other text inputs, and the landline fills the right.
/// The minimum widths preserve Korean telephone numbers; extra space is
/// distributed to BOTH editors rather than left empty. The two labels and
/// editors are siblings in one horizontal layout (no nested line offsets).
/// Responses retain separate field-level evidence hover/highlight targets.
pub fn reception_recipient_fields(
    ui: &mut Ui,
    name: &mut String,
    address: &mut String,
    mobile: &mut String,
    phone: &mut String,
    note: &mut String,
) -> [Response; 5] {
    fn editor(value: &mut String, width: f32) -> egui::TextEdit<'_> {
        egui::TextEdit::singleline(value)
            .font(egui::TextStyle::Body)
            .vertical_align(Align::Center)
            .margin(egui::Margin::symmetric(8, 2))
            .desired_width(width)
    }

    fn label_cell(ui: &mut Ui, text: &str, width: f32) -> Response {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, theme::INPUT_HEIGHT), Sense::hover(),
        );
        let galley = typography::field_label_galley(ui, text);
        ui.painter().with_clip_rect(rect).galley(
            egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley, theme::SECONDARY,
        );
        response
    }

    fn row(ui: &mut Ui, label: &str, value: &mut String) -> Response {
        ui.horizontal(|line| {
            line.spacing_mut().item_spacing.x = theme::RECIPIENT_LABEL_GAP;
            let title = label_cell(line, label, theme::RECIPIENT_LABEL_WIDTH);
            let width = line.available_width().max(1.0);
            let input = line.add_enabled_ui(false, |cell| {
                cell.add_sized(
                    Vec2::new(width, theme::INPUT_HEIGHT),
                    editor(value, width),
                )
            }).inner;
            title.union(input)
        }).inner
    }

    ui.scope(|fields| {
        fields.spacing_mut().item_spacing.y = theme::RECIPIENT_ROW_GAP;
        let name_response = row(fields, "이름", name);
        let address_response = row(fields, "주소", address);

        let row_width = fields.available_width()
            .max(theme::RECIPIENT_PHONE_ROW_MIN_WIDTH);
        let input_budget = row_width
            - theme::RECIPIENT_LABEL_WIDTH
            - 2.0 * theme::RECIPIENT_LABEL_GAP
            - theme::RECIPIENT_PHONE_COLUMN_GAP
            - theme::RECIPIENT_PHONE_LABEL_WIDTH;
        // At the reference pane width both fields grow and use all space.
        // At an unusually narrow pane the contact row alone can scroll.
        let mobile_width = (input_budget * theme::RECIPIENT_MOBILE_WIDTH_SHARE)
            .clamp(
                theme::RECIPIENT_MOBILE_INPUT_WIDTH,
                input_budget - theme::RECIPIENT_PHONE_INPUT_WIDTH,
            );
        let phone_width = input_budget - mobile_width;
        let (mobile_response, phone_response) = egui::ScrollArea::horizontal()
            .id_salt("recipient-contact-row")
            .drag_to_scroll(false)
            .auto_shrink([false, true])
            .show(fields, |contacts| {
                contacts.set_min_width(row_width);
                contacts.horizontal(|line| {
                    line.spacing_mut().item_spacing.y = 0.0;
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_LABEL_GAP;
                    let mobile_label = label_cell(
                        line, "휴대폰", theme::RECIPIENT_LABEL_WIDTH,
                    );
                    let mobile_input = line.add_enabled_ui(false, |cell| {
                        cell.add_sized(
                            Vec2::new(mobile_width, theme::INPUT_HEIGHT),
                            editor(mobile, mobile_width),
                        )
                    }).inner;
                    // Only the gap between the TWO contact groups is 8pt.
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_PHONE_COLUMN_GAP;
                    let phone_label = label_cell(
                        line, "전화", theme::RECIPIENT_PHONE_LABEL_WIDTH,
                    );
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_LABEL_GAP;
                    let phone_input = line.add_enabled_ui(false, |cell| {
                        cell.add_sized(
                            Vec2::new(phone_width, theme::INPUT_HEIGHT),
                            editor(phone, phone_width),
                        )
                    }).inner;
                    (
                        mobile_label.union(mobile_input),
                        phone_label.union(phone_input),
                    )
                }).inner
            }).inner;
        let note_response = row(fields, "메시지", note);
        [
            name_response, address_response,
            mobile_response, phone_response, note_response,
        ]
    }).inner
}

/// The source editor and automatic-message canvas use the SAME surface
/// tokens. Painting a reserved rect avoids Frame::show shrinking an empty
/// message body and moving the fixed copy-action row.
pub fn paint_source_document_surface(ui: &Ui, rect: Rect) {
    ui.painter().rect(
        rect,
        theme::CONTROL_RADIUS as f32,
        theme::INPUT,
        Stroke::new(theme::FRAME_STROKE_WIDTH, theme::BORDER),
        egui::StrokeKind::Inside,
    );
}

/// A subtle separation between data belonging to the same record.
/// Sections themselves are framed by the caller.
pub fn soft_divider(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, theme::SOFT_DIVIDER_WIDTH), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER),
    );
}

/// The 6pt split interaction region is always the workspace (CANVAS)
/// between PANEL cards, including the Kakao reservation boundary.
/// Only hover/drag shows an axis cue; drag targets and saved sizing persist.
pub fn paint_dock_splitter(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
    paint_dock_axis_line(ui, rect, vertical, active);
}

/// Pixel-exact axis line within a gutter rectangle. A 6-*physical*-pixel
/// gutter has a center between two pixels, so its normal line must occupy
/// the middle TWO pixels. A one-pixel line can only sit to one side.
/// Odd-width gutters use a single centered pixel. Hover uses the shared
/// 2pt token, adjusted only when necessary for symmetrical pixel margins.
pub fn paint_dock_axis_line(ui: &Ui, rect: Rect, vertical: bool, active: bool) {
    // No permanent 1-2pt splitter bar: section/card strokes are enough.
    // A thin resize cue appears only when the pointer is over its hit zone.
    if !active { return; }
    let scale = ui.ctx().pixels_per_point().max(1.0);
    let (low, high) = if vertical {
        (rect.left(), rect.right())
    } else {
        (rect.top(), rect.bottom())
    };
    let start_px = (low * scale).round() as i32;
    let end_px = (high * scale).round() as i32;
    let span_px = (end_px - start_px).max(1);
    // Honor the 1pt / 2pt design tokens where they can be truly centered.
    // For a 6px gutter, a requested 1px rule must expand to the middle 2px.
    let logical_width = if active {
        theme::DOCK_SPLITTER_HOVER_LINE
    } else {
        theme::DOCK_SPLITTER_LINE
    };
    let mut line_width_px = ((logical_width * scale).round() as i32)
        .clamp(1, span_px);
    if (span_px - line_width_px) % 2 != 0 {
        // Match slot-width parity so left/right margins are always equal.
        line_width_px += if line_width_px < span_px { 1 } else { -1 };
    }
    let line_start = start_px + (span_px - line_width_px) / 2;
    let line_end = line_start + line_width_px;

    let snapped = |n: f32| (n * scale).round() / scale;
    let line = if vertical {
        Rect::from_min_max(
            egui::pos2(line_start as f32 / scale, snapped(rect.top())),
            egui::pos2(line_end as f32 / scale, snapped(rect.bottom())),
        )
    } else {
        Rect::from_min_max(
            egui::pos2(snapped(rect.left()), line_start as f32 / scale),
            egui::pos2(snapped(rect.right()), line_end as f32 / scale),
        )
    };
    ui.painter().rect_filled(
        line, 0.0,
        if active { theme::BORDER } else { theme::DIVIDER },
    );
}

/// Repaint the complete reception splitter topology in ONE parent layer.
/// Bridges and junctions use the SAME measured pane-divider coordinates.
/// Painting only the bridge's left junction left a visible gap where its
/// right endpoint met a vertical bar at fractional DPI scaling.
pub fn paint_dock_network(
    ui: &Ui,
    verticals: &[(Rect, bool)],
    bridges: &[(usize, usize, f32)],
) {
    // Paint each horizontal bridge only between the CENTER AXES of its
    // two vertical boundaries. Full-width junction squares used to extend
    // the horizontal rule past the vertical line and create protruding tips.
    for &(left, right, center_y) in bridges {
        if left >= verticals.len() || right >= verticals.len() { continue; }
        let x_start = verticals[left].0.center().x;
        let x_end = verticals[right].0.center().x;
        let area = Rect::from_min_max(
            egui::pos2(x_start, center_y - crate::layout::SPLITTER / 2.0),
            egui::pos2(x_end, center_y + crate::layout::SPLITTER / 2.0),
        );
        paint_dock_axis_line(ui, area, false, false);
    }
    // Draw the continuous vertical rules last. This seals T joints with
    // snapped pixels without extending the horizontal tips past the axis.
    for &(rect, highlighted) in verticals {
        paint_dock_axis_line(ui, rect, true, highlighted);
    }
}

/// Shared date-picker calendar from the approved Order History screen.
/// Use the SAME popup, month navigation, selected-day style and 7-column
/// grid for all management date selectors; each caller supplies its own ID.
pub fn calendar_month_start(date: NaiveDate)->NaiveDate{
    NaiveDate::from_ymd_opt(date.year(),date.month(),1).unwrap_or(date)
}
pub fn shift_calendar_month(month:NaiveDate,offset:i32)->NaiveDate{
    let first=calendar_month_start(month);
    if offset>=0{
        first.checked_add_months(Months::new(offset as u32))
    }else{
        first.checked_sub_months(Months::new(offset.unsigned_abs()))
    }.unwrap_or(first)
}
pub fn calendar_dates(month:NaiveDate)->Vec<Option<NaiveDate>>{
    let first=calendar_month_start(month);
    let leading=first.weekday().num_days_from_monday() as usize;
    let next=shift_calendar_month(first,1);
    let days=next.pred_opt().map_or(31,|end|end.day() as usize);
    let visible=((leading+days+6)/7)*7;
    (0..visible).map(|index|{
        (index>=leading && index<leading+days)
            .then(||first.checked_add_signed(Days::days((index-leading) as i64)))
            .flatten()
    }).collect()
}
pub fn date_calendar(
    trigger:&egui::Response,month:&mut NaiveDate,selected:&str,
    grid_id:&'static str
)->Option<NaiveDate>{
    let mut chosen=None;
    let selected_date=NaiveDate::parse_from_str(selected,"%Y-%m-%d").ok();
    egui::Popup::menu(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|calendar|{
            calendar.spacing_mut().item_spacing=egui::vec2(
                theme::SPACE_4,theme::SPACE_4
            );
            calendar.horizontal(|nav|{
                if theme::action(nav,"«",theme::ButtonRole::Standard,true)
                    .on_hover_text("이전 연도").clicked(){
                    *month=shift_calendar_month(*month,-12);
                }
                if theme::action(nav,"‹",theme::ButtonRole::Standard,true)
                    .on_hover_text("이전 달").clicked(){
                    *month=shift_calendar_month(*month,-1);
                }
                nav.label(typography::field_label(&format!(
                    "{}년 {}월",month.year(),month.month()
                )));
                if theme::action(nav,"›",theme::ButtonRole::Standard,true)
                    .on_hover_text("다음 달").clicked(){
                    *month=shift_calendar_month(*month,1);
                }
                if theme::action(nav,"»",theme::ButtonRole::Standard,true)
                    .on_hover_text("다음 연도").clicked(){
                    *month=shift_calendar_month(*month,12);
                }
            });
            calendar.add_space(theme::SPACE_8);
            let width=theme::ACTION_STANDARD_HEIGHT;
            let dates=calendar_dates(*month);
            egui::Grid::new(grid_id)
                .num_columns(7)
                .spacing(egui::vec2(theme::SPACE_4,theme::SPACE_4))
                .show(calendar,|grid|{
                    for day in ["월","화","수","목","금","토","일"]{
                        grid.add_sized(
                            egui::vec2(width,width),
                            egui::Label::new(typography::caption(day))
                                .halign(Align::Center),
                        );
                    }
                    grid.end_row();
                    for (index,day) in dates.iter().enumerate(){
                        if let Some(date)=day{
                            let mut button=egui::Button::new(typography::body(
                                &date.day().to_string()
                            ));
                            if selected_date==Some(*date){
                                button=button.fill(theme::WORKFLOW_SELECTED_BG)
                                    .stroke(Stroke::new(1.0,theme::SELECTION));
                            }
                            if grid.add_sized(egui::vec2(width,width),button).clicked(){
                                chosen=Some(*date);
                                grid.close();
                            }
                        }else{
                            grid.allocate_space(egui::vec2(width,width));
                        }
                        if index%7==6{grid.end_row();}
                    }
                });
        });
    chosen
}

/// Approved 59th-round Freight page heading: 16pt from the top of the
/// workspace. The workspace shell already supplies a 6pt outer gutter, so
/// only the difference is added. Reception must not call this helper.
pub fn management_page_title_top_gap(ui:&mut Ui) {
    ui.add_space((theme::SPACE_16-theme::PANEL_GAP).max(0.0));
}

/// One horizontal guide for a management heading, its optional tabs,
/// top-right metadata and optional KPI strip. This affects ONLY the top
/// content; primary card outer borders keep the established full width.
pub fn management_top_content<R>(
    ui:&mut Ui, add:impl FnOnce(&mut Ui)->R
)->R{
    Frame::new()
        .inner_margin(egui::Margin::symmetric(theme::SECTION_PADDING as i8,0))
        .show(ui,|inset|{
            inset.set_min_width(inset.available_width());
            add(inset)
        }).inner
}

/// Align the OUTER top border of a management page's principal card with
/// Freight > Shipping Group Assignment. Pages without four KPI cards leave
/// an intentional blank area. This is not table-cell padding or row geometry.
pub fn align_management_primary_card(ui: &mut Ui, page_origin_y: f32) {
    let target_top = page_origin_y
        + theme::management_primary_card_top_offset(ui.available_width());
    let remaining = target_top - ui.cursor().top();
    if remaining > 0.5 {
        ui.add_space(remaining);
    }
}

/// Expand one primary list card to the bottom of the management workspace
/// when its rows do not already require more room. Nested/multiple-card
/// pages must not use this: their own content needs ordinary scrolling.
pub fn management_primary_card_min_inner_height(ui: &Ui, vertical_inset: f32) -> f32 {
    management_card_min_height_for_insets(
        ui.available_height(),vertical_inset,vertical_inset
    )
}

/// Same OUTER card height regardless of whether its role uses a symmetric
/// inset or the single-master table's zero bottom inset.
fn management_card_min_height_for_insets(
    available_height:f32,top_inset:f32,bottom_inset:f32
)->f32{
    (available_height-top_inset-bottom_inset
        -2.0*theme::FRAME_STROKE_WIDTH).max(0.0)
}

/// Both master tables preserve 16pt padding on all four inner edges.
/// Compute their inner minimum from the matching frame insets without
/// changing the outer card bounds or the shared row/scroll viewport edge.
pub fn management_master_table_card_min_inner_height(ui:&Ui)->f32{
    management_card_min_height_for_insets(
        ui.available_height(),theme::SECTION_PADDING,
        theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING
    )
}
pub fn fill_management_primary_card(ui: &mut Ui, min_inner_height: f32) {
    ui.set_min_width(ui.available_width().max(1.0));
    ui.set_min_height(min_inner_height);
}

/// CONFLICT-004=A: use the remaining inner height of a SINGLE-list card,
/// not a fixed fraction of the whole application's content rectangle.
/// Measure AFTER toolbars, notices and the pinned header have been laid out.
/// `reserve_below` protects a footer, an empty-state message or other
/// controls that the screen must still render underneath its scroll area.
/// Never use this for multi-table reports or adjacent editor panels.
pub fn management_single_list_scroll_height(
    ui:&Ui,card_min_inner_height:f32,reserve_below:f32
)->f32 {
    management_single_list_scroll_height_geometry(
        ui.min_rect().top()+card_min_inner_height,
        ui.cursor().top(),ui.available_height(),reserve_below
    )
}

/// Pure calculation used by the management list geometry check. A 1pt
/// minimum avoids requesting an invalid/negative scroll area when a split
/// window is shorter than its non-scrolling toolbar and reserved footer.
fn management_single_list_scroll_height_geometry(
    card_bottom:f32,rows_top:f32,available_height:f32,reserve_below:f32
)->f32 {
    let remaining=(card_bottom-rows_top-reserve_below.max(0.0)).max(0.0);
    remaining.min(available_height.max(0.0)).max(1.0)
}

/// C-approved dual-master toolbar alignment contract:
/// right-aligned search/date/bulk controls end at the PRIMARY CARD'S inner
/// right edge. The scrollbar gutter belongs only to table headers and rows.
/// Slot height/vertical stacking may differ by screen, but not its right edge.
/// Returns egui logical-point coordinates; convert to device pixels with
/// Context::pixels_per_point() when reporting runtime measurements.
pub fn management_toolbar_trailing_rect(
    toolbar:Rect,desired_width:f32,top_offset:f32,height:f32
)->Rect {
    debug_assert!(desired_width.is_finite() && desired_width>=0.0);
    debug_assert!(top_offset.is_finite() && top_offset>=0.0);
    debug_assert!(height.is_finite() && height>=0.0);
    debug_assert!(top_offset+height<=toolbar.height()+0.01,
        "trailing controls must stay within the reserved toolbar");
    let width=desired_width.min(toolbar.width()).max(0.0);
    let rect=Rect::from_min_size(
        egui::pos2(toolbar.right()-width,toolbar.top()+top_offset),
        Vec2::new(width,height)
    );
    debug_assert!((rect.right()-toolbar.right()).abs()<0.01,
        "management toolbar trailing edge must match the card's inner right edge");
    rect
}

/// SSOT scroll-track geometry for all native management tables.
///
/// The row viewport is ALWAYS limited by `scroll_height`; only the native
/// scrollbar track may extend into a card's non-scrolling bottom inset.
/// The two single-table masters pass the common 3pt optical correction,
/// so their rail ends at the painted body viewport rather than protruding
/// into the card's 16pt lower padding. Other lists may extend through a
/// non-scrolling inset, but never through a real footer.
pub fn management_table_scroll_track_rect(
    ui:&Ui,scroll_height:f32,bottom_extension:f32
)->Rect {
    management_table_scroll_track_geometry(
        ui.available_rect_before_wrap(),scroll_height,bottom_extension
    )
}

/// The only place that computes top/bottom of a management table rail.
/// The rail starts exactly at the fixed header's lower edge (0pt top gap)
/// and stops 3pt before its requested bottom. This avoids the broken
/// header-to-scrollbar join while retaining bottom clearance. The row
/// viewport and the data columns are never extended by this function.
fn management_table_scroll_track_geometry(
    bounds:Rect,scroll_height:f32,bottom_extension:f32
)->Rect {
    debug_assert!(scroll_height.is_finite()&&scroll_height>=0.0);
    debug_assert!(bottom_extension.is_finite()&&bottom_extension>=0.0);
    let rows_bottom=(bounds.top()+scroll_height).min(bounds.bottom());
    let track_bottom=(rows_bottom+bottom_extension
        -theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET)
        .max(bounds.top()+1.0);
    let track_top=(bounds.top()+theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET)
        .min(track_bottom-1.0);
    Rect::from_min_max(
        egui::pos2(bounds.left(),track_top),
        egui::pos2(bounds.right(),track_bottom)
    )
}

/// One measured vertical transition for every fixed-header management table.
///
/// Call with `item_spacing.y == 0` *from the toolbar allocation onward*.
/// This owns the symmetric 16pt toolbar-to-divider breathing room,
/// 1pt divider, 4pt after it, and 36pt sticky header. Screens must
/// not add a separate `add_space` or rely on egui's preceding item spacing.
#[derive(Clone,Copy,Debug)]
pub struct ManagementTableHeader {
    pub header_rect:Rect,
    pub header_content_rect:Rect,
    pub viewport_width:f32,
    pub scrollbar_gutter:f32,
    pub table_width:f32,
}

/// One minimum-width calculation for both sticky master tables and
/// other fixed-header management lists. The gutter supplied here is the
/// egui scrollbar allocation only (CONFLICT-003=A), without any spacer.
fn management_table_data_width(
    viewport_width:f32,scrollbar_gutter:f32,min_table_width:f32
)->f32{
    debug_assert!(scrollbar_gutter>=0.0 && scrollbar_gutter.is_finite());
    debug_assert!(viewport_width.is_finite() && min_table_width.is_finite());
    (viewport_width-scrollbar_gutter).max(min_table_width)
}

/// Standard 21pt transition for a toolbar that touches the card cursor.
/// Existing inset panels can consume part of the 16pt pre-divider margin.
pub fn management_sticky_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP
    )
}

/// CONFLICT-005 (approved): the parent toolbar/strip may already contain an
/// 8pt bottom inner margin. Subtract that existing inset from the 16pt
/// pre-divider breathing room instead of adding another full 21pt strip.
/// Call only for a directly adjacent toolbar and fixed table header.
pub fn management_sticky_table_header_after_inset(
    ui:&mut Ui,min_table_width:f32,consumed_bottom_inset:f32
)->ManagementTableHeader{
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,
        management_remaining_predivider_gap(consumed_bottom_inset)
    )
}

/// Pure SSOT conversion: the frame's existing bottom inset is part of the
/// 16pt pre-divider whitespace, not space to add on top of it.
fn management_remaining_predivider_gap(consumed_bottom_inset:f32)->f32{
    debug_assert!(consumed_bottom_inset.is_finite()&&consumed_bottom_inset>=0.0
        && consumed_bottom_inset<=theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP,
        "toolbar bottom inset must fit inside the standard top gap");
    (theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP-consumed_bottom_inset).max(0.0)
}

fn management_sticky_table_header_with_top_gap(
    ui:&mut Ui,min_table_width:f32,top_gap:f32
)->ManagementTableHeader{
    debug_assert!(ui.spacing().item_spacing.y.abs()<0.01,
        "table toolbar and header must use zero implicit vertical spacing");
    let viewport_width=ui.available_width().max(1.0);
    let scrollbar_gutter=ui.spacing().scroll.allocated_width();
    let table_width=management_table_data_width(
        viewport_width,scrollbar_gutter,min_table_width
    );

    // One allocation for the full toolbar-to-header separator strip.
    // Keeping it in a single Ui layout operation also avoids a fractional
    // pixel difference between manual add_space and automatic item gaps.
    let transition_height=top_gap+theme::SOFT_DIVIDER_WIDTH
        +theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP;
    let (transition,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,transition_height),Sense::hover()
    );
    ui.painter().rect_filled(
        Rect::from_min_size(
            egui::pos2(transition.left(),
                transition.top()+top_gap),
            Vec2::new(viewport_width,theme::SOFT_DIVIDER_WIDTH)
        ),0.0,theme::DIVIDER
    );
    let (header_rect,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
        Sense::hover()
    );
    // The rightmost scrollbar rail is not another table column. Both the
    // sticky header and virtualized body should use the same data width.
    let header_content_rect=Rect::from_min_max(
        header_rect.min,
        egui::pos2(
            (header_rect.right()-scrollbar_gutter)
                .max(header_rect.left()+1.0),
            header_rect.bottom()
        )
    );
    // Fixed-header contract shared by BOTH approved master tables.
    // Validate real allocated Rects, not just intended token values.
    debug_assert!((header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<0.01,
        "management table header must retain the shared 36pt height");
    if header_rect.width()>=scrollbar_gutter+1.0 {
        debug_assert!((header_content_rect.right()+scrollbar_gutter
            -header_rect.right()).abs()<0.01,
            "header's data right edge must exclude the table scrollbar gutter");
    }
    ManagementTableHeader{
        header_rect,header_content_rect,viewport_width,
        scrollbar_gutter,table_width
    }
}

/// CONFLICT-003=A: list-table data width reserves only the native
/// scrollbar allocation. CONFLICT-005: this *bare-header* variant is still
/// required when status messages, nested editor controls, or independent
/// section headings intervene between a toolbar and its table. For adjacent
/// toolbars use management_sticky_table_header[_after_inset] instead.
pub fn management_list_table_header(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader{
    let viewport_width=ui.available_width().max(1.0);
    let scrollbar_gutter=ui.spacing().scroll.allocated_width();
    let table_width=management_table_data_width(
        viewport_width,scrollbar_gutter,min_table_width
    );
    let (header_rect,_)=ui.allocate_exact_size(
        Vec2::new(viewport_width,theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
        Sense::hover()
    );
    let header_content_rect=Rect::from_min_max(
        header_rect.min,
        egui::pos2((header_rect.right()-scrollbar_gutter)
            .max(header_rect.left()+1.0),header_rect.bottom())
    );
    debug_assert!(
        (header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).abs()<0.01,
        "management list header must use the shared 36pt height"
    );
    ManagementTableHeader{
        header_rect,header_content_rect,viewport_width,
        scrollbar_gutter,table_width
    }
}

/// One 10pt / 1.8pt-stroke chevron for selectors, date stepping and
/// expand/collapse affordances. Rotating the same vector keeps the glyphs
/// identical at all orientations and avoids Windows font-dependent triangles.
#[derive(Clone,Copy)]
pub(crate) enum ManagementChevronDirection { Down, Left, Right }

fn paint_chevron_at_angle(
    painter:&egui::Painter,center:egui::Pos2,angle:f32,optical_raise:f32,
    ink:Color32
){
    const HALF_SPAN:f32=5.0;
    const DEPTH:f32=2.5;
    const STROKE:f32=1.8;
    let (s,c)=angle.sin_cos();
    let rotated=|x:f32,y:f32|egui::pos2(
        center.x+x*c-y*s,
        center.y+x*s+y*c-optical_raise
    );
    let a=rotated(-DEPTH,-HALF_SPAN);
    let tip=rotated(DEPTH,0.0);
    let b=rotated(-DEPTH,HALF_SPAN);
    painter.line_segment([a,tip],Stroke::new(STROKE,ink));
    painter.line_segment([tip,b],Stroke::new(STROKE,ink));
}

fn paint_management_chevron_with_color(
    painter:&egui::Painter,center:egui::Pos2,
    direction:ManagementChevronDirection,color:Color32
){
    let (angle,raise)=match direction {
        ManagementChevronDirection::Right=>(0.0,0.0),
        ManagementChevronDirection::Down=>(std::f32::consts::FRAC_PI_2,1.0),
        ManagementChevronDirection::Left=>(std::f32::consts::PI,0.0),
    };
    paint_chevron_at_angle(painter,center,angle,raise,color);
}

pub(crate) fn paint_management_chevron(
    painter:&egui::Painter,center:egui::Pos2,direction:ManagementChevronDirection
){
    paint_management_chevron_with_color(
        painter,center,direction,theme::SECONDARY
    );
}

/// Native CollapsingHeader keeps its original click/keyboard/state/indent
/// behavior; only its default filled triangle becomes the common line icon.
/// Rotate smoothly with egui's animated openness (0 = right, 1 = down).
pub(crate) fn paint_collapsing_header_icon(
    ui:&mut Ui,openness:f32,response:&Response
){
    let t=openness.clamp(0.0,1.0);
    paint_chevron_at_angle(
        ui.painter(),response.rect.center(),
        t*std::f32::consts::FRAC_PI_2,t,theme::SECONDARY
    );
}

/// Reference "접수일 기준" dropdown: 34pt dark trigger, separated
/// right-hand chevron, 1pt outline, 6pt corner radius. This replaces native
/// ComboBox triggers where management screens need the same visual language.
/// The caller retains its existing option values and change handling.
pub fn management_dropdown(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,tooltip:&str,
    options:impl FnOnce(&mut Ui)
)->Response {
    // Keep exact toolbar/cell width without stretching the subsequent action
    // or exceeding a narrow selector slot.
    let width=width.min(ui.available_width()).max(1.0);
    // Parent Ui may be disabled independently of the explicit enabled flag.
    let interactive=enabled && ui.is_enabled();
    let fill=if interactive{theme::INPUT}
        else{theme::MANAGEMENT_DROPDOWN_DISABLED_BG};
    let border=if interactive{theme::BORDER}
        else{theme::DISABLED_ACTION_BORDER};
    let ink=if interactive{theme::TEXT}
        else{theme::DISABLED_ACTION_TEXT};
    let arrow_ink=if interactive{theme::SECONDARY}
        else{theme::DISABLED_ACTION_TEXT};
    let trigger=ui.add_enabled(
        enabled,
        egui::Button::new("")
            .min_size(Vec2::new(width,theme::ACTION_STANDARD_HEIGHT))
            .fill(fill)
            .stroke(Stroke::new(theme::SOFT_DIVIDER_WIDTH,border))
            .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
    );
    let trigger=if tooltip.is_empty(){trigger}
        else{trigger.on_hover_text(tooltip)};
    let rect=trigger.rect;
    let painter=ui.painter().with_clip_rect(rect);
    let arrow_left=rect.right()-30.0;
    let caption=typography::freight_table_cell_galley(
        ui,label,false,ink
    );
    let text_clip=Rect::from_min_max(
        rect.min+Vec2::new(theme::SPACE_12,0.0),
        egui::pos2((arrow_left-theme::SPACE_4).max(rect.left()+theme::SPACE_12),
            rect.bottom())
    );
    painter.with_clip_rect(text_clip).galley(
        egui::pos2(rect.left()+theme::SPACE_12,
            rect.center().y-caption.size().y/2.0
                +typography::MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y),
        caption,ink
    );
    painter.line_segment(
        [egui::pos2(arrow_left,rect.top()+5.0),
         egui::pos2(arrow_left,rect.bottom()-5.0)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    // Restore the pre-enlargement dropdown chevron dimensions; the date
    // prev/next controls rotate this same shared shape.
    paint_management_chevron_with_color(
        &painter,egui::pos2(rect.right()-15.0,rect.center().y),
        ManagementChevronDirection::Down,arrow_ink
    );
    egui::Popup::menu(&trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|menu|{
            menu.set_min_width(width);
            options(menu);
        });
    trigger
}

/// Select a value and close its popup on the same click. All selectors share
/// this action so menus don't remain open after immediate per-row autosave.
pub fn management_dropdown_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str
)->Response {
    let response=ui.selectable_value(current,value,label);
    if response.clicked(){ui.close();}
    response
}

/// Shared management tab roles (content width, no full-page stretching).
pub fn management_tabs(ui:&mut Ui,labels:&[(&str,bool,bool)]) -> Option<usize> {
    let mut pressed=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        for (index,(title,active,enabled)) in labels.iter().enumerate() {
            let role=if *active {theme::ButtonRole::ActiveManagementTab}
                else {theme::ButtonRole::ManagementTab};
            if theme::action(bar,title,role,*enabled).clicked() {
                pressed=Some(index);
            }
        }
    });
    pressed
}

/// Select a row in an editor's left-hand list. This is NOT a table
/// record: it has a 14pt item title and a separate 12pt state caption,
/// following the original 48pt message-template chooser hierarchy.
/// Painting is confined to the reserved fixed-height rectangle, so hover
/// and selected styles never change the following row positions.
pub fn management_chooser_row(
    ui:&mut Ui,id:&str,title:&str,status:&str,selected:bool
)->Response{
    let width=ui.available_width().max(1.0);
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::MANAGEMENT_CHOOSER_ROW_HEIGHT),Sense::click()
    );
    let background=if selected {
        theme::SELECTION.linear_multiply(0.22)
    }else if response.hovered(){theme::INPUT}else{theme::PANEL};
    let painter=ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect,theme::CONTROL_RADIUS as f32,background);
    if selected{
        painter.rect_stroke(
            rect.shrink(0.5),theme::CONTROL_RADIUS as f32,
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SELECTION),
            egui::StrokeKind::Inside
        );
    }
    let primary=typography::client_table_cell_galley(ui,title,true);
    let secondary=typography::client_table_metadata_galley(
        ui,status,theme::MUTED
    );
    let lines=primary.size().y+secondary.size().y+theme::SPACE_4;
    let x=rect.left()+theme::SPACE_12;
    let y=rect.center().y-lines/2.0;
    let content_rect=Rect::from_min_max(
        egui::pos2(x,rect.top()),egui::pos2(rect.right()-theme::SPACE_12,rect.bottom())
    );
    let ink=painter.with_clip_rect(content_rect);
    ink.galley(egui::pos2(x,y),primary,theme::TEXT);
    ink.galley(
        egui::pos2(x,y+lines-secondary.size().y),
        secondary,theme::MUTED
    );
    response.on_hover_text(format!("{title} / {status}"))
}

/// The tier palette belongs to the shared design system. Badge geometry is
/// painted inside the client column and cannot expand the row's layout.
fn client_tier_colors(tier:&str)->(Color32,Color32) {
    match tier {
        "wholesale"=>(theme::CLIENT_TIER_WHOLESALE_BG,theme::CLIENT_TIER_WHOLESALE_TEXT),
        "semi"=>(theme::CLIENT_TIER_SEMI_BG,theme::CLIENT_TIER_SEMI_TEXT),
        "retail"=>(theme::CLIENT_TIER_RETAIL_BG,theme::CLIENT_TIER_RETAIL_TEXT),
        "special"=>(theme::CLIENT_TIER_SPECIAL_BG,theme::CLIENT_TIER_SPECIAL_TEXT),
        _=>(theme::RAISED,theme::SECONDARY),
    }
}

/// Paint a measured text line at a literal left boundary. The header and
/// values call the same helper, so layout alignment never depends on egui's
/// Label allocation/halign behavior inside a wide subcell.
fn paint_client_line(
    ui:&Ui,cell:Rect,x:f32,galley:std::sync::Arc<egui::Galley>,color:Color32
) {
    let y=cell.center().y-galley.size().y/2.0;
    ui.painter().with_clip_rect(cell).galley(
        egui::pos2(x,y),galley,color
    );
}

/// Keep client-list headings visible while the rows scroll. This uses
/// the SAME 30/38/32 column coordinates and 12pt text insets as the
/// existing client row renderer; horizontal scroll shifts only the glyphs,
/// never the header's fixed 36pt viewport.
pub fn management_client_header(
    ui:&mut Ui,header:Rect,table_left:f32,table_width:f32,
    current_sort:Option<(usize,bool)>
)->Option<usize>{
    let titles=["거래처","발송인 정보","거래 조건"];
    let sort_keys=["거래처명","주소","가격 등급"];
    let painter=ui.painter().with_clip_rect(header);
    painter.rect_filled(header,0.0,theme::RAISED);
    let mut x=table_left;
    let mut selected=None;
    for (i,title) in titles.iter().enumerate(){
        let col_width=table_width*theme::MANAGEMENT_CLIENT_COLS[i];
        let column=Rect::from_min_max(
            egui::pos2(x,header.top()),
            egui::pos2(x+col_width,header.bottom())
        );
        let visible=column.intersect(header);
        if visible.width()>0.0 {
            let response=ui.interact(
                visible,ui.make_persistent_id(("client-management-sort",i)),
                Sense::click()
            ).on_hover_text(format!(
                "{} · {}",sort_keys[i],match current_sort{
                    Some((col,false)) if col==i=>"내림차순 정렬",
                    Some((col,true)) if col==i=>"기본 순서로 복귀",
                    _=>"오름차순 정렬"
                }
            ));
            if response.clicked(){selected=Some(i);}
            if response.hovered(){
                painter.rect_stroke(
                    visible.shrink(1.0),2.0,
                    Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER),
                    egui::StrokeKind::Inside
                );
            }
            let text_rect=Rect::from_min_max(
                egui::pos2(column.left()+theme::SPACE_12,header.top()),
                egui::pos2(column.right()-theme::SPACE_12,header.bottom())
            ).intersect(header);
            if text_rect.width()>0.0 {
                // Same 14pt header hierarchy as Freight Assignment.
                let title_galley=typography::management_table_header_galley(ui,title);
                let (arrow,color)=match current_sort{
                    Some((col,false)) if col==i=>("▲",theme::SELECTION),
                    Some((col,true)) if col==i=>("▼",theme::SELECTION),
                    _=>("↕",theme::MUTED)
                };
                let arrow_galley=typography::management_table_header_galley(ui,arrow);
                let title_x=column.left()+theme::SPACE_12;
                let text_clip=painter.with_clip_rect(text_rect);
                text_clip.galley(
                    egui::pos2(title_x,header.center().y-title_galley.size().y/2.0),
                    title_galley.clone(),theme::SECONDARY
                );
                text_clip.galley(
                    egui::pos2(title_x+title_galley.size().x+theme::SPACE_4,
                        header.center().y-arrow_galley.size().y/2.0),
                    arrow_galley,color
                );
            }
        }
        x+=col_width;
    }
    painter.line_segment(
        [header.left_bottom(),header.right_bottom()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    selected
}

/// Three React-derived client columns (30/38/32), with the heading and all
/// data anchored at identical left edges. Every client occupies one row.
/// Address/alias/contact text clips inside its own region without wrapping.
pub fn management_row(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);3],heading:bool,tier:Option<&str>
) -> Response {
    management_row_impl(ui,row_id,columns,heading,tier,None)
}

/// Draw a client record with the shared low-contrast alternate-row surface.
/// The index is the visible row position after filtering, not a persistent
/// customer ID, so the stripes remain consistent during search.
pub fn management_row_striped(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);3],tier:Option<&str>,
    visible_index:usize
) -> Response {
    management_row_impl(ui,row_id,columns,false,tier,Some(visible_index))
}

fn management_row_impl(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);3],heading:bool,
    tier:Option<&str>,visible_index:Option<usize>
) -> Response {
    let width=ui.available_width().max(theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH);
    let height=if heading {theme::MANAGEMENT_TABLE_HEADER_HEIGHT}
        else {theme::MANAGEMENT_CLIENT_ROW_HEIGHT};
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,height),if heading {Sense::hover()} else {Sense::click()}
    );
    let bg=if heading {theme::RAISED}
        else if response.hovered() {theme::INPUT}
        else if visible_index.is_some_and(|index|index%2==1) {
            theme::TABLE_ROW_ALT_BG
        }else{theme::PANEL};
    ui.painter().rect_filled(rect,0.0,bg);
    ui.painter().line_segment(
        [rect.left_bottom(),rect.right_bottom()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );

    let mut x=rect.left();
    for (i,(primary,secondary)) in columns.iter().enumerate() {
        let col_width=rect.width()*theme::MANAGEMENT_CLIENT_COLS[i];
        let cell=Rect::from_min_max(
            egui::pos2(x+theme::SPACE_12,rect.top()),
            egui::pos2(x+col_width-theme::SPACE_12,rect.bottom())
        );
        // In particular, heading '발송인 정보' and the first address
        // glyph both start at cell.left(), never at its centered label rect.
        if heading {
            paint_client_line(
                ui,cell,cell.left(),typography::table_header_galley(ui,primary),
                theme::SECONDARY
            );
        }else if i==2 {
            // Inline tier badge: intrinsic text width + 4pt horizontal and
            // 2pt vertical inset. No full-width or full-height badge block.
            let (fill,ink)=client_tier_colors(tier.unwrap_or(""));
            let badge_text=typography::client_table_metadata_galley(ui,primary,ink);
            let badge_width=(badge_text.size().x+theme::SPACE_8).min(cell.width());
            let badge_height=badge_text.size().y+theme::SPACE_4;
            let badge=Rect::from_min_size(
                egui::pos2(cell.left(),cell.center().y-badge_height/2.0),
                Vec2::new(badge_width,badge_height)
            );
            ui.painter().rect_filled(badge,theme::CONTROL_RADIUS as f32,fill);
            paint_client_line(
                ui,badge,cell.left()+theme::SPACE_4,badge_text,ink
            );
            if !secondary.is_empty() {
                let second=Rect::from_min_max(
                    egui::pos2((badge.right()+theme::SPACE_8).min(cell.right()),cell.top()),
                    cell.max
                );
                paint_client_line(
                    ui,second,second.left(),
                    typography::client_table_metadata_galley(ui,secondary,theme::MUTED),
                    theme::MUTED
                );
            }
        }else if primary.is_empty() || secondary.is_empty() {
            let visible=if primary.is_empty(){secondary}else{primary};
            if !visible.is_empty() {
                let galley=if primary.is_empty() {
                    typography::client_table_metadata_galley(ui,visible,theme::MUTED)
                }else{
                    typography::client_table_cell_galley(ui,visible,i==0)
                };
                paint_client_line(
                    ui,cell,cell.left(),galley,
                    if primary.is_empty(){theme::MUTED}else{theme::TEXT}
                );
            }
        }else{
            // Only the *second* datum is offset. Primary text always uses
            // the exact header start and is not centered within its share.
            let available=(cell.width()-theme::SPACE_8).max(0.0);
            let first_width=available*if i==1 {0.69}else{0.64};
            let first=Rect::from_min_size(
                cell.min,Vec2::new(first_width,cell.height())
            );
            let second=Rect::from_min_max(
                egui::pos2((first.right()+theme::SPACE_8).min(cell.right()),cell.top()),
                cell.max
            );
            paint_client_line(
                ui,first,first.left(),
                typography::client_table_cell_galley(ui,primary,i==0),
                theme::TEXT
            );
            paint_client_line(
                ui,second,second.left(),
                typography::client_table_metadata_galley(ui,secondary,theme::MUTED),
                theme::MUTED
            );
        }
        // Keep full, unclipped information accessible without changing the
        // row-wide click-to-edit behavior or inserting extra visible controls.
        if !heading {
            let full=if secondary.is_empty(){primary.to_string()}
                else if primary.is_empty(){secondary.to_string()}
                else{format!("{primary} / {secondary}")};
            if !full.is_empty() {
                let hover_id=ui.make_persistent_id(("management-row-hover",row_id,i));
                ui.interact(cell,hover_id,Sense::hover()).on_hover_text(full);
            }
        }
        x+=col_width;
    }
    response
}

/// Display-only won formatting. No business price or rounding decisions live
/// in the UI layer; amounts are supplied by the Rust business engine.
pub fn display_won(amount: i64) -> String {
    let digits = amount.to_string();
    let (sign, positive) = if let Some(text) = digits.strip_prefix('-') {
        ("-", text)
    } else { ("", digits.as_str()) };
    let mut groups = String::new();
    for (index, character) in positive.chars().enumerate() {
        if index > 0 && (positive.len() - index) % 3 == 0 {
            groups.push(',');
        }
        groups.push(character);
    }
    format!("{sign}{groups}원")
}

pub fn summary_strip(ui: &mut Ui, label: &str, value: &str, footnote: &str) {
    ui.vertical(|ui| {
        soft_divider(ui);
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|ui| {
            ui.label(typography::field_label(label));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(typography::summary_value(value));
            });
        });
        if !footnote.trim().is_empty() {
            ui.label(typography::caption(footnote));
        }
    });
}

/// Geometry shared by recognized working orders and native workflow rows.
/// The name receives all width not reserved for the two narrow numeric columns.
fn reception_item_columns(rect: Rect) -> [Rect; 3] {
    let qty = (rect.width() * 0.22).min(theme::RECEPTION_ITEMS_QTY_MAX_WIDTH).max(52.0);
    let amount = (rect.width() * 0.30)
        .min(theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH).max(74.0);
    let name_right = (rect.right() - qty - amount).max(rect.left() + 12.0);
    let qty_right = (name_right + qty).min(rect.right());
    [
        Rect::from_min_max(rect.min, egui::pos2(name_right, rect.bottom())),
        Rect::from_min_max(egui::pos2(name_right, rect.top()),
            egui::pos2(qty_right, rect.bottom())),
        Rect::from_min_max(egui::pos2(qty_right, rect.top()), rect.max),
    ]
}

pub struct ReceptionItemCells {
    pub product: Response,
    pub quantity: Response,
    pub amount: Response,
}

pub fn reception_items_header(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_HEADER_HEIGHT), Sense::hover()
    );
    let columns = reception_item_columns(rect);
    for (index, label) in ["품목", "수량", "금액"].iter().enumerate() {
        let cell = columns[index].shrink2(egui::vec2(theme::SPACE_4, 0.0));
        let galley = typography::table_header_galley(ui, label);
        // The product header starts at the same LEFT edge as product text;
        // quantity and amount end at the same RIGHT edges as their values.
        let x = if index == 0 {
            cell.left()
        } else {
            cell.right() - galley.size().x
        };
        ui.painter().with_clip_rect(cell).galley(
            egui::pos2(x, cell.center().y - galley.size().y / 2.0),
            galley, theme::SECONDARY,
        );
    }
    ui.painter().line_segment(
        [egui::pos2(rect.left(), rect.bottom()),
         egui::pos2(rect.right(), rect.bottom())],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER)
    );
}

pub fn reception_item_row(
    ui: &mut Ui, name: &str, quantity: &str, amount: &str,
) -> ReceptionItemCells {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_ROW_HEIGHT), Sense::hover()
    );
    let columns = reception_item_columns(rect);
    let padded = |index: usize| columns[index].shrink2(
        egui::vec2(theme::SPACE_4, theme::SPACE_8)
    );
    let cells = [padded(0), padded(1), padded(2)];
    // Use explicit column anchors for all three text cells. Ui::put(Label)
    // was moving text toward the center inside the row, while the table
    // header used another alignment calculation.
    for (index, (text, font, color)) in [
        (name, egui::FontId::proportional(typography::FONT_BODY), theme::TEXT),
        (quantity, egui::FontId::monospace(typography::FONT_FIELD_LABEL), theme::SECONDARY),
        (amount, egui::FontId::monospace(typography::FONT_FIELD_LABEL), theme::TEXT),
    ].into_iter().enumerate() {
        let cell = cells[index];
        let right = index != 0;
        ui.painter().with_clip_rect(cell).text(
            egui::pos2(if right { cell.right() } else { cell.left() }, cell.center().y),
            if right { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER },
            text,
            font,
            color,
        );
    }
    // Preserve the five-source-field highlighting contract: these three
    // hover responses still map to the exact Rust product/quantity/price keys.
    // Use a distinct id for each column in each row. next_auto_id()
    // alone would not necessarily advance for manually painted cells.
    let row_id = (rect.top().to_bits(), rect.left().to_bits());
    let product = ui.interact(
        cells[0], ui.make_persistent_id(("item-cell", row_id, 0)), Sense::hover(),
    ).on_hover_text(name);
    let quantity = ui.interact(
        cells[1], ui.make_persistent_id(("item-cell", row_id, 1)), Sense::hover(),
    );
    let amount = ui.interact(
        cells[2], ui.make_persistent_id(("item-cell", row_id, 2)), Sense::hover(),
    );
    ui.painter().line_segment(
        [egui::pos2(rect.left(), rect.bottom()),
         egui::pos2(rect.right(), rect.bottom())],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER)
    );
    ReceptionItemCells { product, quantity, amount }
}

/// Presentation-only freight field: numbers and prepay/COD are supplied by
/// native Rust. Disabled text input mirrors the React field without implying
/// that a backend edit command exists in egui.
pub fn reception_freight_row(
    ui: &mut Ui, label: &str, amount: Option<i64>, collect: Option<bool>,
    remote: Option<bool>, inquiry_visible: bool,
) {
    ui.horizontal_wrapped(|line| {
        line.spacing_mut().item_spacing = Vec2::new(theme::SPACE_4, theme::SPACE_4);
        line.label(typography::field_label(label));
        let mut amount_text = amount
            .map(|value| display_won(value).trim_end_matches('원').to_owned())
            .unwrap_or_else(||"—".to_owned());
        // Match shared 36pt input geometry; the previous bare TextEdit
        // rendered at native text height and looked misaligned beside labels.
        line.add_enabled_ui(false, |cell| {
            cell.add_sized(
                Vec2::new(84.0, theme::INPUT_HEIGHT),
                egui::TextEdit::singleline(&mut amount_text)
                    .font(typography::numeric_editor_font())
                    .vertical_align(Align::Center)
                    .margin(egui::Margin::symmetric(8, 2))
                    .desired_width(84.0),
            )
        });
        line.label(typography::caption("원"));
        if let Some(collect) = collect {
            line.label(typography::caption(
                if collect { "(착불)" } else { "(선불)" }
            ));
        }
        if let Some(checked) = remote {
            let mut read_only = checked;
            line.add_enabled(false, egui::Checkbox::new(&mut read_only, "도서/산간"));
        }
        if inquiry_visible {
            theme::action(line, "공장에 운임 문의", theme::ButtonRole::Standard, false);
        }
    });
}

/// Original React lower action row. Keep its already-approved actions
/// visible but disabled until the actual native editing commands are wired.
pub fn reception_item_actions(ui: &mut Ui, dispatch_origin: Option<&str>) {
    ui.horizontal_wrapped(|bar| {
        bar.spacing_mut().item_spacing.x = theme::SPACE_4;
        theme::action(bar, "＋ 상품 추가", theme::ButtonRole::Standard, false);
        bar.with_layout(Layout::right_to_left(Align::Center), |right| {
            let button = |right: &mut Ui, label: &str, selected: bool| {
                right.add_enabled(false,
                    egui::Button::new(typography::action_label(
                        label, typography::FONT_STANDARD_ACTION, false,
                        if selected { theme::TEXT } else { theme::MUTED }
                    ))
                    .min_size(Vec2::new(42.0, theme::ACTION_STANDARD_HEIGHT))
                    .fill(if selected { theme::RAISED } else { theme::PANEL })
                    .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH,
                        if selected { theme::BORDER } else { theme::DIVIDER }))
                    .corner_radius(theme::CONTROL_RADIUS)
                );
            };
            // right-to-left construction keeps the complete label at the
            // left of the choice pair in the final visual order.
            button(right, "매장", dispatch_origin == Some("STORE"));
            button(right, "공장", dispatch_origin == Some("FACTORY"));
            right.label(typography::caption("발송지:"));
        });
    });
}

pub fn reception_vat_row(ui: &mut Ui, vat_included: Option<bool>) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(width, theme::RECEPTION_VAT_ROW_HEIGHT),
        Layout::left_to_right(Align::Center), |line| {
            if let Some(included) = vat_included {
                let mut readonly = included;
                line.add_enabled(false,
                    egui::Checkbox::new(&mut readonly, "부가세 포함")
                );
            } else {
                line.label(typography::caption("부가세 확인 전"));
            }
        }
    );
}

/// React's pinned 56px total: first line is title / fixed right amount,
/// second line is shipment count / fixed right VAT. Neither line is allowed
/// to resize the item scroller or move when the monetary text changes.
pub fn reception_total_footer(
    ui: &mut Ui, total: Option<i64>, count: Option<usize>, vat: &str,
    note: &str,
) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), theme::RECEPTION_TOTAL_HEIGHT),
        Sense::hover(),
    );
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH, theme::DIVIDER),
    );
    // Both summary lines occupy fixed vertical centers. The item table and
    // VAT row no longer push this footer below the pane's bottom boundary.
    let left = rect.left() + theme::SPACE_8;
    let right = rect.right() - theme::SPACE_8;
    let right_start = (right - 148.0).max(left + 50.0);
    let amount = total.map(display_won).unwrap_or_else(|| "—".to_owned());
    ui.painter().text(
        egui::pos2(left, rect.top() + 17.0), egui::Align2::LEFT_CENTER,
        "총 주문", egui::FontId::proportional(typography::FONT_FIELD_LABEL),
        theme::TEXT,
    );
    let first_amount = Rect::from_min_max(
        egui::pos2(right_start, rect.top()), egui::pos2(right, rect.top() + 30.0),
    );
    ui.painter().with_clip_rect(first_amount).text(
        egui::pos2(right, rect.top() + 17.0), egui::Align2::RIGHT_CENTER,
        &amount, egui::FontId::monospace(18.0), theme::TEXT,
    );
    // A missing shipment count is unknown, not zero.
    let count_text = count.map(|n| format!("배송 {n}건"))
        .unwrap_or_else(|| "배송 건수 미확정".to_owned());
    let second_left = Rect::from_min_max(
        egui::pos2(left, rect.top() + 30.0),
        egui::pos2(right_start, rect.bottom()),
    );
    ui.painter().with_clip_rect(second_left).text(
        egui::pos2(left, rect.top() + 43.0), egui::Align2::LEFT_CENTER,
        count_text, egui::FontId::proportional(typography::FONT_METADATA),
        theme::SECONDARY,
    );
    let second_right = Rect::from_min_max(
        egui::pos2(right_start, rect.top() + 30.0),
        egui::pos2(right, rect.bottom()),
    );
    ui.painter().with_clip_rect(second_right).text(
        egui::pos2(right, rect.top() + 43.0), egui::Align2::RIGHT_CENTER,
        vat, egui::FontId::proportional(typography::FONT_METADATA),
        theme::SECONDARY,
    );
    if !note.trim().is_empty() {
        response.on_hover_text(note);
    }
}

pub fn record_card(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::PANEL_RADIUS)
        .inner_margin(theme::CARD_PADDING)
        .show(ui, content);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusTone {
    Neutral,
    Active,
    Complete,
    Warning,
    Error,
    Linked,
}
impl StatusTone {
    pub fn color(self) -> Color32 {
        match self {
            Self::Neutral => theme::BORDER,
            Self::Active | Self::Warning => theme::SELECTED_ORDER,
            Self::Complete => theme::SUCCESS,
            Self::Error => theme::DANGER,
            Self::Linked => theme::SELECTION,
        }
    }
}

pub fn status_dots(ui: &mut Ui, tones: &[StatusTone]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for tone in tones {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(12.0, 18.0), egui::Sense::hover(),
            );
            ui.painter().circle_filled(rect.center(), 4.5, tone.color());
        }
    });
}

pub fn table_header(ui: &mut Ui, columns: &[(&str, f32)]) {
    Frame::new().fill(theme::RAISED).inner_margin(egui::Margin::symmetric(theme::SPACE_8 as i8, theme::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (title, width) in columns {
                    ui.add_sized(
                        Vec2::new(*width, TABLE_HEADER_HEIGHT - 8.0),
                        egui::Label::new(
                            typography::table_header(title),
                        ),
                    );
                }
            });
        });
}

pub fn table_row(ui: &mut Ui, columns: &[(&str, f32)]) {
    ui.horizontal(|ui| {
        for (value, width) in columns {
            ui.add_sized(
                Vec2::new(*width, TABLE_ROW_HEIGHT),
                egui::Label::new(typography::table_cell(value)),
            );
        }
    });
    soft_divider(ui);
}

// Narrow geometry contract check only, not a business-data regression suite.
#[cfg(test)]
mod management_toolbar_geometry_tests {
    use super::*;
    #[test]
    fn master_table_keeps_symmetric_padding_without_moving_card_border(){
        let outer_height=800.0;
        let regular=management_card_min_height_for_insets(
            outer_height,theme::SECTION_PADDING,theme::SECTION_PADDING
        );
        let master=management_card_min_height_for_insets(
            outer_height,theme::SECTION_PADDING,
            theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING
        );
        assert_eq!(master,regular);
        assert_eq!(theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING,
            theme::SECTION_PADDING);
        let regular_outer=regular+2.0*theme::FRAME_STROKE_WIDTH
            +2.0*theme::SECTION_PADDING;
        let master_outer=master+2.0*theme::FRAME_STROKE_WIDTH
            +theme::SECTION_PADDING
            +theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING;
        assert_eq!(regular_outer,outer_height);
        assert_eq!(master_outer,outer_height);
    }

    #[test]
    fn master_table_scroll_rail_ends_at_row_viewport_without_moving_rows(){
        // Both the rows and the native rail finish on the same measured
        // bottom edge, preserving 16pt of lower card padding.
        let bounds=Rect::from_min_size(
            egui::pos2(16.0,300.0),Vec2::new(800.0,484.0)
        );
        let painted_rows_bottom=bounds.bottom();
        let outer_card_bottom=painted_rows_bottom
            +theme::MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING;
        let track=management_table_scroll_track_geometry(
            bounds,bounds.height(),theme::MANAGEMENT_MASTER_TABLE_SCROLL_RAIL_EXTENSION
        );
        assert_eq!(outer_card_bottom-painted_rows_bottom,
            theme::SECTION_PADDING);
        assert_eq!(track.top(),bounds.top()); // joins fixed header
        assert_eq!(painted_rows_bottom,784.0); // unchanged row viewport
        assert_eq!(outer_card_bottom,800.0); // unchanged outer card
        assert_eq!(track.bottom(),painted_rows_bottom);
        assert_eq!(track.bottom(),784.0);
        assert_eq!(outer_card_bottom-track.bottom(),theme::SECTION_PADDING);
        assert_eq!(theme::MANAGEMENT_MASTER_TABLE_SCROLL_RAIL_EXTENSION,
            theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET);
    }

    #[test]
    fn management_scroll_track_uses_common_top_and_card_bottom_policy(){
        let bounds=Rect::from_min_size(
            egui::pos2(16.0,300.0),Vec2::new(800.0,484.0)
        );
        let inset=management_table_scroll_track_geometry(
            bounds,484.0,theme::SECTION_PADDING
        );
        assert_eq!(inset.top(),300.0);
        assert_eq!(inset.bottom(),797.0); // 3pt before card bottom
        assert_eq!(inset.left(),16.0);
        assert_eq!(inset.right(),816.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET,0.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET,3.0);

        // When a footer follows the table, the rail remains limited to its
        // viewport and only the bottom 3pt clearance remains.
        let with_footer=management_table_scroll_track_geometry(bounds,400.0,0.0);
        assert_eq!(with_footer.top(),300.0);
        assert_eq!(with_footer.bottom(),697.0);

        // The rows remain capped by the available data-viewport bounds.
        let clipped=management_table_scroll_track_geometry(
            bounds,1200.0,theme::SECTION_PADDING
        );
        assert_eq!(clipped.bottom(),797.0);

        // Very small panes must still produce a non-inverted rail Rect.
        let tiny=management_table_scroll_track_geometry(bounds,1.0,0.0);
        assert!(tiny.height()>=1.0);
        assert!(tiny.top()>=bounds.top());
    }

    #[test]
    fn list_and_master_table_gutters_reserve_only_native_scrollbar_width() {
        // CONFLICT-003=A. Every role uses the same 10pt allocation;
        // none adds the previous 4pt between data and the rail.
        for (viewport,minimum) in [
            (900.0,900.0),(1100.0,1100.0),(1440.0,942.0),
            (1440.0,1242.0),
        ] {
            let native_gutter=10.0;
            let data_width=management_table_data_width(
                viewport,native_gutter,minimum
            );
            if viewport-native_gutter>=minimum {
                assert!((data_width+native_gutter-viewport).abs()<0.01);
            }else{
                assert!((data_width-minimum).abs()<0.01);
            }
        }
    }

    #[test]
    fn toolbar_to_header_transition_remains_21pt_with_preexisting_inset() {
        let top=theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP;
        let rule=theme::SOFT_DIVIDER_WIDTH;
        let bottom=theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP;
        assert_eq!(top+rule+bottom,21.0);
        let existing_toolbar_inset=theme::SPACE_8;
        let remaining=management_remaining_predivider_gap(existing_toolbar_inset);
        assert_eq!(remaining,8.0);
        assert_eq!(existing_toolbar_inset+remaining+rule+bottom,21.0);
        assert_eq!(management_remaining_predivider_gap(0.0),16.0);
    }

    #[test]
    fn single_list_scroll_height_reserves_footer_and_uses_card_bottom() {
        // 200pt card top + 600pt card height = 800pt bottom.
        // The toolbar and sticky header have already used 60pt.
        let full=management_single_list_scroll_height_geometry(
            800.0,260.0,700.0,0.0
        );
        let with_footer=management_single_list_scroll_height_geometry(
            800.0,260.0,700.0,72.0
        );
        let capped_by_actual_window=management_single_list_scroll_height_geometry(
            800.0,260.0,300.0,0.0
        );
        let very_short=management_single_list_scroll_height_geometry(
            410.0,402.0,20.0,36.0
        );
        assert_eq!(full,540.0);
        assert_eq!(with_footer,468.0);
        assert_eq!(capped_by_actual_window,300.0);
        assert_eq!(very_short,1.0);
    }

    #[test]
    fn trailing_slot_ends_at_card_inner_edge_without_scrollbar_subtraction() {
        for width in [400.0, 704.0, 1280.0] {
            let toolbar=Rect::from_min_size(
                egui::pos2(16.0,216.0),Vec2::new(width,124.0)
            );
            let bulk=management_toolbar_trailing_rect(toolbar,500.0,0.0,36.0);
            let dates=management_toolbar_trailing_rect(toolbar,704.0,44.0,80.0);
            for slot in [bulk,dates] {
                assert!((slot.right()-toolbar.right()).abs()<0.01);
                assert!(slot.left()>=toolbar.left()-0.01);
                assert!(slot.right()<=toolbar.right()+0.01);
            }
        }
    }
}
