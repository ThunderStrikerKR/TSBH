//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, Sense, Stroke, Ui, Vec2};
use crate::{theme, typography};

pub const LABEL_WIDTH: f32 = theme::FIELD_LABEL_WIDTH;
pub const TABLE_ROW_HEIGHT: f32 = 42.0;
pub const TABLE_HEADER_HEIGHT: f32 = 36.0;

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
            painter.line_segment([p(-2.6, -4.2), p(1.7, 0.0)], stroke);
            painter.line_segment([p(1.7, 0.0), p(-2.6, 4.2)], stroke);
        }
        WorkflowIcon::Expanded => {
            painter.line_segment([p(-4.2, -1.8), p(0.0, 2.5)], stroke);
            painter.line_segment([p(0.0, 2.5), p(4.2, -1.8)], stroke);
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

pub fn modal_actions(ui:&mut Ui, cancel:&str, proceed:&str,
    proceed_role:theme::ButtonRole, enabled:bool) -> (bool,bool) {
    let mut cancelled=false;
    let mut submitted=false;
    ui.horizontal(|ui|{
        cancelled=theme::action(ui,cancel,theme::ButtonRole::Quiet,true).clicked();
        submitted=theme::action(ui,proceed,proceed_role,enabled).clicked();
    });
    (cancelled,submitted)
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

/// One common table row, with content-based secondary text. Percentages are
/// supplied by the table's layout role, never overridden per record.
pub fn management_row(
    ui:&mut Ui,row_id:&str,columns:[(&str,&str);3],heading:bool
) -> Response {
    let width=ui.available_width().max(theme::MANAGEMENT_TABLE_MIN_WIDTH);
    let height=if heading {theme::MANAGEMENT_TABLE_HEADER_HEIGHT}
        else {theme::MANAGEMENT_TABLE_ROW_HEIGHT};
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,height),if heading {Sense::hover()} else {Sense::click()}
    );
    let bg=if heading {theme::RAISED}
        else if response.hovered() {theme::INPUT} else {theme::PANEL};
    ui.painter().rect_filled(rect,0.0,bg);
    ui.painter().line_segment(
        [rect.left_bottom(),rect.right_bottom()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    let mut x=rect.left();
    for (i,(primary,secondary)) in columns.iter().enumerate() {
        let col_width=rect.width()*theme::MANAGEMENT_CLIENT_COLS[i];
        let cell=Rect::from_min_size(
            egui::pos2(x+theme::SPACE_12,rect.top()+theme::SPACE_8),
            Vec2::new((col_width-theme::SPACE_24).max(1.0),
                (height-theme::SPACE_8).max(1.0))
        );
        ui.scope_builder(
            egui::UiBuilder::new().id_salt(("management-row-column",row_id,i))
                .max_rect(cell).layout(Layout::top_down(Align::Min)),
            |inner|{
                inner.set_clip_rect(cell);
                inner.label(if heading {
                    typography::table_header(primary)
                } else {
                    typography::table_cell(primary)
                });
                if !secondary.is_empty() {
                    inner.label(typography::caption(secondary));
                }
            }
        );
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
