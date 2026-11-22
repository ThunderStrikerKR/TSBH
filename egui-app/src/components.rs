//! Reusable egui controls. A screen specifies business purpose and data,
//! while colors, sizes and alignment are owned by the shared UI layer.

use eframe::egui::{self, Align, Color32, Frame, Layout, Rect, Response, Sense, Stroke, Ui, Vec2};
use chrono::{Datelike, Duration as Days, Months, NaiveDate};
use crate::{source_highlight, theme, typography};

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

/// Semantic labels for management table columns. A screen selects the
/// column role; the shared component owns the displayed header wording.
/// This does not change the underlying product description or column size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagementTableColumnLabel {
    ProductName,
}

impl ManagementTableColumnLabel {
    pub const fn text(self) -> &'static str {
        match self {
            Self::ProductName => "상품명",
        }
    }
}

/**
 * Shared management-table checkbox placement.
 *
 * `cell` is the complete first-column rectangle (44pt Freight, 48pt Excel,
 * or another approved width). `vertical_area` is the 42pt data-row rect,
 * or the fixed header's text area excluding its inner 2pt bottom outline.
 * The checkbox always follows Freight Assignment's centered 36pt/34pt
 * hit-box policy rather than a caller-specific `shrink2` or `Ui::put`.
 */
fn management_table_checkbox_rect(
    cell:Rect,vertical_area:Rect,icon_width:f32,icon_spacing:f32
)->Rect{
    debug_assert!(icon_width>=0.0&&icon_spacing>=0.0);
    egui::Rect::from_center_size(
        egui::pos2(cell.center().x,vertical_area.center().y),
        egui::vec2(
            icon_width+icon_spacing,
            vertical_area.height().min(theme::INPUT_HEIGHT)
        )
    )
}

/// Render either a fixed-header select-all checkbox or a body-row checkbox
/// with identical centering, clipping, stable identity and enabled semantics.
/// Keep selection business rules at the call site.
pub fn management_table_checkbox(
    ui:&mut Ui,cell:Rect,vertical_area:Rect,
    checked:&mut bool,enabled:bool,id:impl std::hash::Hash
)->Response {
    let rect=management_table_checkbox_rect(
        cell,vertical_area,ui.spacing().icon_width,ui.spacing().icon_spacing
    );
    let clip=cell.intersect(vertical_area).intersect(ui.clip_rect());
    let mut builder=egui::UiBuilder::new()
        .id_salt(id)
        .max_rect(rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center));
    if !enabled {
        builder=builder.disabled();
    }
    ui.scope_builder(builder,|control|{
        control.set_clip_rect(clip);
        control.checkbox(checked,"")
    }).inner
}

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
                // These controls are intentionally disabled; use the
                // exact disabled Standard/TableSecondary master appearance.
                .fill(theme::DISABLED_ACTION_BG)
                .stroke(Stroke::new(
                    theme::FRAME_STROKE_WIDTH,theme::DISABLED_ACTION_BORDER
                ))
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
                theme::DISABLED_ACTION_TEXT,
            );
        }
        WorkflowAuxiliaryAction::Delete => paint_workflow_icon(
            ui.painter(), response.rect.center(), WorkflowIcon::Trash,
            theme::DISABLED_ACTION_TEXT,
        ),
        WorkflowAuxiliaryAction::PreviousStage => paint_workflow_icon(
            ui.painter(), response.rect.center(), WorkflowIcon::PreviousStage,
            theme::DISABLED_ACTION_TEXT,
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


/// Four distinct panes replace the word "Win". This is a static glyph
/// inside an explanatory keyboard illustration, not a clickable icon.
fn paint_windows_key_glyph(painter:&egui::Painter,face:Rect){
    let size=12.5;
    let gap=1.5;
    let pane=(size-gap)*0.5;
    let left=face.center().x-size*0.5;
    let top=face.center().y-size*0.5;
    for row in 0..2{
        for col in 0..2{
            let x=left+col as f32*(pane+gap);
            let y=top+row as f32*(pane+gap);
            painter.rect_filled(
                Rect::from_min_size(egui::pos2(x,y),Vec2::splat(pane)),
                0.5,theme::TEXT
            );
        }
    }
}

/// Low-angle keycap illustration: the bottom side slopes outward toward
/// the viewer. Distinct top face, tapered side, rim and lip highlight make
/// it three-dimensional without click, hover or pressed button styling.
fn reception_keycap(ui:&mut Ui,key:&str){
    let font=egui::FontId::proportional(typography::FONT_FIELD_LABEL);
    let galley=ui.painter().layout_no_wrap(
        if key=="Win"{String::new()}else{key.to_owned()},
        font,theme::TEXT
    );
    let width=if key=="Win"{30.0}else{
        (galley.size().x+2.0*theme::SPACE_8).ceil().max(26.0)
    };
    let height=theme::RECEPTION_KEYCAP_HEIGHT;
    let depth=theme::RECEPTION_KEYCAP_DEPTH;
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(width,height),Sense::hover()
    );
    let radius=theme::RECEPTION_KEYCAP_RADIUS;
    let face=Rect::from_min_max(
        egui::pos2(rect.left()+2.0,rect.top()+1.0),
        egui::pos2(rect.right()-2.0,rect.bottom()-depth)
    );
    let painter=ui.painter();

    // Silhouette and angled lower wall, visible from slightly below.
    painter.rect_filled(rect,radius,theme::RECEPTION_KEYCAP_SIDE);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(face.left(),face.bottom()-0.5),
            egui::pos2(face.right(),face.bottom()-0.5),
            egui::pos2(rect.right()-1.0,rect.bottom()-1.0),
            egui::pos2(rect.left()+1.0,rect.bottom()-1.0)
        ],
        theme::RECEPTION_KEYCAP_SIDE,
        Stroke::NONE
    ));
    painter.rect_stroke(
        rect,radius,Stroke::new(1.0,theme::BORDER),
        egui::StrokeKind::Inside
    );
    // Recessed upper rim and light-catching front face.
    painter.rect_filled(face,radius,theme::RECEPTION_KEYCAP_FACE);
    painter.rect_stroke(
        face,radius,Stroke::new(1.0,theme::RECEPTION_KEYCAP_EDGE),
        egui::StrokeKind::Inside
    );
    painter.line_segment(
        [
            egui::pos2(face.left()+5.0,face.top()+1.5),
            egui::pos2(face.right()-5.0,face.top()+1.5)
        ],
        Stroke::new(1.0,theme::RECEPTION_KEYCAP_HIGHLIGHT)
    );
    painter.line_segment(
        [
            egui::pos2(rect.left()+4.0,rect.bottom()-1.7),
            egui::pos2(rect.right()-4.0,rect.bottom()-1.7)
        ],
        Stroke::new(1.0,theme::RECEPTION_KEYCAP_BOTTOM_HIGHLIGHT)
    );
    if key=="Win"{
        paint_windows_key_glyph(painter,face);
    }else{
        painter.galley(
            egui::pos2(
                (face.center().x-galley.size().x*0.5).round(),
                (face.center().y-galley.size().y*0.5).round()
            ),
            galley,theme::TEXT
        );
    }
}

/// Reuse the same 4pt key grouping on every reception layout. Unlike plain
/// prose, each shortcut key has its own bordered background and hit-free box.
pub fn reception_capture_shortcut_hint(ui:&mut Ui){
    ui.horizontal_wrapped(|line|{
        line.spacing_mut().item_spacing=egui::vec2(
            theme::SPACE_4,theme::SPACE_4
        );
        line.label(typography::caption("이미지 캡처:"));
        reception_keycap(line,"Win");
        line.label(typography::caption("+"));
        reception_keycap(line,"Shift");
        line.label(typography::caption("+"));
        reception_keycap(line,"S");
        line.label(typography::caption("또는"));
        reception_keycap(line,"Print Screen");
    });
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

/// Backward-compatible title entry point. All ordinary dialogs now use
/// the approved OCR master header: one PANEL surface, one divider, title
/// LEFT and Cancel/Close RIGHT. Compact dialogs omit the second subtitle
/// row; they do not inherit the large OCR dialog's 84pt header.
pub fn modal_header(ui:&mut Ui,title:&str,close_label:&str,enabled:bool)->bool{
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_HEADER_HEIGHT),
        Sense::hover()
    );
    modal_master_header_in_rect(ui,rect,title,"",close_label,enabled)
}

/// Fixed popup title region with its own background and a single
/// Freight-master DIVIDER at the bottom. This is an opt-in popup v2
/// component during the three-screen rollout: old dialogs keep their
/// approved geometry until explicitly migrated.
/// Compatibility for fixed-position compact headers during migration.
pub fn modal_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,close_label:&str,enabled:bool
)->bool {
    modal_master_header_in_rect(ui,rect,title,"",close_label,enabled)
}

/// Standard 56pt title slot. The returned event is still the original
/// user-supplied cancel/close action; this component changes appearance only.
pub fn modal_header_chrome(
    ui:&mut Ui,title:&str,close_label:&str,enabled:bool
)->bool {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_HEADER_HEIGHT),
        Sense::hover()
    );
    modal_header_in_rect(ui,rect,title,close_label,enabled)
}

/// Footer has a stable height, a subtle background and ONE upper divider.
/// Do not create extra cancel buttons inside it.
/// Backward-compatible master footer; reserve the full 36pt primary
/// button height and render a single PANEL upper rule in every dialog.
pub fn modal_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    modal_master_footer_in_rect(ui,rect,actions)
}

/// Approved OCR modal geometry, now the default structural baseline for
/// future egui dialogs. Text length and local scrollbars never move these
/// three regions. Callers choose the appropriate popup size.
#[derive(Clone,Copy,Debug)]
pub struct ModalMasterLayout {
    pub header:Rect,
    pub workspace:Rect,
    pub footer:Rect,
}

pub fn modal_master_layout(
    bounds:Rect,header_height:f32,footer_height:f32,gap:f32
)->ModalMasterLayout {
    let h=bounds.height().max(0.0);
    let title_h=header_height.max(0.0).min(h);
    let actions_h=footer_height.max(0.0).min((h-title_h).max(0.0));
    let header=Rect::from_min_size(bounds.min,Vec2::new(bounds.width(),title_h));
    let footer=Rect::from_min_size(
        egui::pos2(bounds.left(),bounds.bottom()-actions_h),
        Vec2::new(bounds.width(),actions_h)
    );
    let gap=gap.max(0.0);
    let top=(header.bottom()+gap).min(footer.top());
    let bottom=(footer.top()-gap).max(top);
    let workspace=Rect::from_min_max(
        egui::pos2(bounds.left(),top),
        egui::pos2(bounds.right(),bottom)
    );
    ModalMasterLayout{header,workspace,footer}
}

/// A single continuous PANEL surface; toolbar and split are optional.
/// Returned lines are authoritative layout anchors for clipped contents.
/// NO nested cards, doubled strokes, or independent popup color palette.
#[derive(Clone,Copy,Debug)]
pub struct ModalMasterWorkspace {
    pub toolbar_bottom:f32,
    pub divider_x:Option<f32>,
}

/// Align the master edges to physical pixels before painting the header,
/// rounded workspace, or footer. Layout still owns the original dimensions;
/// every edge moves by at most half a device pixel. All strokes and pane fills
/// must use the SAME aligned rect so anti-aliasing cannot expose a seam.
pub fn modal_master_pixel_aligned_rect(rect:Rect,pixels_per_point:f32)->Rect {
    let scale=pixels_per_point.max(1.0);
    let snap=|v:f32|(v*scale).round()/scale;
    Rect::from_min_max(
        egui::pos2(snap(rect.left()),snap(rect.top())),
        egui::pos2(snap(rect.right()),snap(rect.bottom()))
    )
}

/// Paint only the single shared work-surface background. OCR paints its
/// two content panes after this step, then uses `finish` for crisp borders.
/// This order prevents rectangular pane fills from erasing rounded corners.
pub fn modal_master_workspace_background(
    ui:&Ui,rect:Rect,toolbar_height:f32,split_ratio:Option<f32>
)->ModalMasterWorkspace {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,theme::CONTROL_RADIUS,theme::PANEL);
    let snap=|v:f32|(v*ppp).round()/ppp;
    let toolbar_bottom=snap((rect.top()+toolbar_height.max(0.0)).min(rect.bottom()));
    let divider_x=split_ratio.map(|share|{
        snap(rect.left()+rect.width()*share.clamp(0.0,1.0))
    });
    ModalMasterWorkspace{toolbar_bottom,divider_x}
}

/// Paint separators AFTER pane backgrounds and widgets, then redraw exactly
/// one outer outline as the final layer. Pixel-aligned rectangular hairlines
/// have flat endpoints; intersecting dividers therefore do not protrude into
/// the outer frame or leave doubled anti-aliased corner pixels.
pub fn modal_master_workspace_finish(
    ui:&Ui,rect:Rect,geometry:ModalMasterWorkspace
) {
    let painter=ui.painter();
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    let snap=|v:f32|(v*ppp).round()/ppp;
    let line_width=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    let outer_width=(theme::FRAME_STROKE_WIDTH*ppp).round().max(1.0)/ppp;
    let inset=outer_width.max(line_width);
    let left=snap(rect.left()+inset);
    let right=snap(rect.right()-inset);
    let top=snap(rect.top()+inset);
    let bottom=snap(rect.bottom()-inset);
    if right>left && bottom>top {
        if geometry.toolbar_bottom>rect.top() &&
            geometry.toolbar_bottom<rect.bottom() {
            let y=snap(geometry.toolbar_bottom-line_width/2.0)
                .clamp(top,(bottom-line_width).max(top));
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(left,y),egui::pos2(right,y+line_width)
                ),0.0,theme::DIVIDER
            );
        }
        if let Some(divider_x)=geometry.divider_x {
            let x=snap(divider_x-line_width/2.0)
                .clamp(left,(right-line_width).max(left));
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x,top),egui::pos2(x+line_width,bottom)
                ),0.0,theme::DIVIDER
            );
        }
    }
    painter.rect_stroke(
        rect,theme::CONTROL_RADIUS,
        Stroke::new(outer_width,theme::DIVIDER),
        egui::StrokeKind::Inside
    );
}

/// Keep the one-call API for other dialogs. Screens with separately painted
/// content can opt into background/finish without changing the master style.
pub fn modal_master_workspace_surface(
    ui:&Ui,rect:Rect,toolbar_height:f32,split_ratio:Option<f32>
)->ModalMasterWorkspace {
    let geometry=modal_master_workspace_background(
        ui,rect,toolbar_height,split_ratio
    );
    modal_master_workspace_finish(ui,rect,geometry);
    geometry
}

/// Canonical OCR-approved popup header, used for descriptive dialogs:
/// title and top-right Cancel/Close in the first row, optional explanation
/// in the second, ONE bottom divider. Height belongs to modal_master_layout.
/// Use an empty description for dialogs with no explanatory subtitle.
pub fn modal_master_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    // A centered line at rect.bottom() is half clipped and can appear
    // darker at one end. Fill exactly one pixel-aligned INNER rule instead.
    let rule_height=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(rect.left(),rect.bottom()-rule_height),
            rect.right_bottom()
        ),0.0,theme::DIVIDER
    );
    let has_description=!description.trim().is_empty();
    let heading_top=if has_description {
        rect.top()+theme::SPACE_8
    }else{
        rect.top()+((rect.height()-theme::ACTION_STANDARD_HEIGHT)/2.0).max(0.0)
    };
    let heading=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,heading_top),
        Vec2::new((rect.width()-2.0*theme::SPACE_8).max(1.0),
            theme::ACTION_STANDARD_HEIGHT)
    );
    let subtitle=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,heading.bottom()+theme::SPACE_4),
        Vec2::new(heading.width(),20.0)
    );
    let heading_clip=heading.intersect(ui.clip_rect());
    let mut close=false;
    ui.scope_builder(
        egui::UiBuilder::new().id_salt("modal-master-heading")
            .max_rect(heading)
            .layout(Layout::left_to_right(Align::Center)),
        |header|{
            header.set_clip_rect(heading_clip);
            header.set_width(heading.width());
            header.label(typography::modal_title(title));
            header.with_layout(Layout::right_to_left(Align::Center),|right|{
                close=theme::action(
                    right,close_label,theme::ButtonRole::Standard,enabled
                ).clicked();
            });
        },
    );
    if has_description {
        let subtitle_clip=subtitle.intersect(ui.clip_rect());
        ui.scope_builder(
            egui::UiBuilder::new().id_salt("modal-master-description")
                .max_rect(subtitle),
            |details|{
                details.set_clip_rect(subtitle_clip);
                details.set_width(subtitle.width());
                details.label(typography::caption(description));
            },
        );
    }
    close
}


/// A single physical-pixel aligned footer surface for both fixed OCR
/// dialogs and flow-based confirmations. Owning the rule in one place
/// prevents half-clipped line segments or doubled separator strokes.
fn paint_modal_master_footer_surface(ui:&Ui,rect:Rect) {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    let width=(theme::SOFT_DIVIDER_WIDTH*ppp).round().max(1.0)/ppp;
    let rule_bottom=(rect.top()+width).min(rect.bottom());
    ui.painter().rect_filled(
        Rect::from_min_max(
            rect.left_top(),egui::pos2(rect.right(),rule_bottom)
        ),0.0,theme::DIVIDER
    );
}

/// Canonical quiet modal footer: one upper divider and the final action
/// anchored at the right, never an additional bottom Cancel button.
pub fn modal_master_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    paint_modal_master_footer_surface(ui,rect);
    // Primary actions are 36pt, but standard actions are 34pt. The
    // shared footer must reserve the taller control's ACTUAL hit/paint
    // height; a 34pt clip sliced 2pt off the bottom of "적용".
    let action_height=theme::ACTION_PRIMARY_HEIGHT
        .max(theme::ACTION_STANDARD_HEIGHT);
    let content=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,
            rect.top()+((rect.height()-action_height)/2.0).max(0.0)),
        Vec2::new((rect.width()-2.0*theme::SPACE_8).max(1.0),
            action_height)
    );
    let clip=content.intersect(ui.clip_rect());
    ui.scope_builder(
        egui::UiBuilder::new().id_salt("modal-master-actions")
            .max_rect(content)
            .layout(Layout::right_to_left(Align::Center)),
        |footer|{
            footer.set_clip_rect(clip);
            footer.set_width(content.width());
            actions(footer);
        },
    );
}


/// Retain the pilot API so older callers can migrate without visual churn.
#[allow(dead_code)]
pub fn modal_review_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool {
    modal_master_header_in_rect(ui,rect,title,description,close_label,enabled)
}

/// Retain the pilot API while new screens use the master naming.
#[allow(dead_code)]
pub fn modal_review_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    modal_master_footer_in_rect(ui,rect,actions);
}

/// Flow-layout sibling of modal_master_footer_in_rect for compact
/// confirmations. Shares the OCR master footer appearance and 36pt hitbox
/// while retaining egui's InnerResponse<R>, so existing confirmation state
/// machines (including .inner results) remain unchanged.
pub fn modal_action_row<R>(
    ui:&mut Ui,actions:impl FnOnce(&mut Ui)->R
)->egui::InnerResponse<R> {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_FOOTER_HEIGHT),
        Sense::hover()
    );
    paint_modal_master_footer_surface(ui,rect);
    let action_height=theme::ACTION_PRIMARY_HEIGHT
        .max(theme::ACTION_STANDARD_HEIGHT);
    let area=Rect::from_min_size(
        egui::pos2(rect.left()+theme::SPACE_8,
            rect.top()+((rect.height()-action_height)/2.0).max(0.0)),
        Vec2::new((rect.width()-2.0*theme::SPACE_8).max(1.0),
            action_height)
    );
    let clip=area.intersect(ui.clip_rect());
    ui.scope_builder(
        egui::UiBuilder::new().id_salt("modal-master-flow-actions")
            .max_rect(area)
            .layout(Layout::right_to_left(Align::Center)),
        |buttons|{
            buttons.set_clip_rect(clip);
            buttons.set_width(area.width());
            actions(buttons)
        }
    )
}

/// Maximum scrollable editor body height after reserving the OCR-master
/// header/footer and the native modal's fixed outer margins. Screen modules
/// must not independently subtract arbitrary offsets from the full work area:
/// doing so can push the final 58pt action row outside a short window.
pub fn modal_editor_body_max_height(work_area_height:f32)->f32 {
    let fixed=2.0*theme::MODAL_PANEL_PADDING_Y
        +2.0*theme::FRAME_STROKE_WIDTH
        +theme::MODAL_HEADER_HEIGHT
        +theme::MODAL_FOOTER_HEIGHT
        +theme::SPACE_12
        +3.0*theme::SPACING;
    (work_area_height-fixed).max(1.0)
        .min(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
}

/// Two-ended footer for an editor with both a contextual/danger action
/// and a final Save/Apply action. The OCR-master surface, rule, height and
/// button hitbox still belong to `modal_action_row`.
///
/// Independent LEFT and RIGHT slots prevent the primary action from being
/// pushed off-screen by a longer destructive label or variable-width status.
pub fn modal_action_row_split(
    ui:&mut Ui,
    leading:impl FnOnce(&mut Ui),
    trailing:impl FnOnce(&mut Ui),
) {
    modal_action_row(ui,|footer|{
        footer.columns(2,|columns|{
            columns[0].with_layout(
                Layout::left_to_right(Align::Center),|left|leading(left)
            );
            columns[1].with_layout(
                Layout::right_to_left(Align::Center),|right|trailing(right)
            );
        });
    });
}

pub fn modal_footer_chrome(ui:&mut Ui,actions:impl FnOnce(&mut Ui)) {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_FOOTER_HEIGHT),
        Sense::hover()
    );
    modal_footer_in_rect(ui,rect,actions);
}

/// One framed content work surface for flow-based popup bodies. Use one
/// per dialog rather than wrapping every subsection in another card.
pub fn modal_master_body<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut Ui)->R
)->R {
    let inset=theme::MODAL_SECTION_PADDING;
    let width=(ui.available_width()-2.0*inset
        -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER))
        .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
        .inner_margin(inset)
        .show(ui,|body|{
            body.set_min_width(width);
            contents(body)
        }).inner
}

/// A simple heading and one divider WITHIN the same master work surface.
/// No additional per-section fill, rounded card, or duplicate outline.
pub fn modal_master_body_section<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->R {
    ui.label(typography::section_heading(title));
    theme::divider(ui);
    contents(ui)
}

/// One coherent nested information section, visually related to Freight
/// Shipping-Group Assignment's PANEL/DIVIDER hierarchy. Continuous fields
/// share the same frame rather than each receiving a redundant border.
pub fn modal_section<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->R {
    // Frame::show is otherwise content-sized and the section border can
    // become narrower when a list is empty or an error message disappears.
    let inner_width=(ui.available_width()-2.0*theme::MODAL_SECTION_PADDING
        -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
    Frame::new()
        .fill(theme::MODAL_SECTION_BG)
        .stroke(Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER))
        .corner_radius(egui::CornerRadius::same(theme::MODAL_SECTION_RADIUS))
        .inner_margin(theme::MODAL_SECTION_PADDING)
        .show(ui,|section|{
            section.set_min_width(inner_width);
            section.label(typography::section_heading(title));
            theme::divider(section);
            contents(section)
        }).inner
}

/// Same nested-section surface for absolute-layout dialogs such as OCR.
/// Caller is responsible for fixed content hit areas and clipping.
pub fn modal_section_surface(ui:&Ui,rect:Rect,header_height:f32) {
    let painter=ui.painter();
    painter.rect_filled(
        rect,theme::MODAL_SECTION_RADIUS,theme::MODAL_SECTION_BG
    );
    let band=Rect::from_min_max(
        rect.min+Vec2::splat(1.0),
        egui::pos2(
            (rect.right()-1.0).max(rect.left()+1.0),
            (rect.top()+header_height).min(rect.bottom()).max(rect.top()+1.0)
        )
    );
    painter.rect_filled(
        band,theme::MODAL_SECTION_RADIUS,theme::MODAL_SECTION_HEADER_BG
    );
    let y=(rect.top()+header_height).min(rect.bottom());
    painter.line_segment(
        [egui::pos2(rect.left()+1.0,y),egui::pos2(rect.right()-1.0,y)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    painter.rect_stroke(
        rect,theme::MODAL_SECTION_RADIUS,
        Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER),
        egui::StrokeKind::Inside
    );
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

/// Navigate one recipient from the 34pt section HEADER, never from a
/// separate row over the recipient fields. Geometry and arrow glyphs follow
/// Order History's segmented date selector: INPUT fill, 28pt arrow hits,
/// zero internal spacing, 1pt outline and subtle vertical separators.
/// The name is a read-only truncated label with the full value on hover.
pub fn reception_recipient_navigator(
    ui:&mut Ui,name:&str,index:usize,total:usize
)->Option<usize>{
    if total<=1{return None;}
    let wanted=theme::RECEPTION_RECIPIENT_NAV_WIDTH;
    let width=ui.available_width().min(wanted).max(0.0);
    if width<2.0*theme::RECEPTION_RECIPIENT_NAV_ARROW_WIDTH
        +theme::RECEPTION_RECIPIENT_NAV_COUNT_WIDTH
        +2.0*theme::FRAME_STROKE_WIDTH
    {return None;}
    let height=theme::SECTION_HEADER_HEIGHT;
    let (outer,_)=ui.allocate_exact_size(
        Vec2::new(width,height),Sense::hover()
    );
    let inner=outer.shrink(theme::FRAME_STROKE_WIDTH);
    let arrow_width=theme::RECEPTION_RECIPIENT_NAV_ARROW_WIDTH;
    let count_width=theme::RECEPTION_RECIPIENT_NAV_COUNT_WIDTH;
    let name_width=(inner.width()-2.0*arrow_width-count_width).max(0.0);
    let current=index.min(total-1);
    let mut selected=None;
    ui.painter().rect_filled(
        outer,theme::CONTROL_RADIUS as f32,theme::INPUT
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt("reception-recipient-nav-header")
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |segments|{
            segments.set_width(inner.width());
            segments.set_height(inner.height());
            segments.spacing_mut().item_spacing=Vec2::ZERO;
            let arrow=|row:&mut Ui,direction:ManagementChevronDirection,
                       enabled:bool,hint:&str|{
                let response=row.add_enabled(
                    enabled,
                    egui::Button::new("")
                        .fill(if enabled{theme::INPUT}
                            else{theme::RECEPTION_RECIPIENT_NAV_DISABLED_BG})
                        .stroke(Stroke::NONE)
                        .corner_radius(0)
                        .min_size(Vec2::new(arrow_width,inner.height()))
                );
                // Clearly recessed endpoints, not merely egui's subtle
                // disabled tint. Use the same direction glyph as date nav.
                let painter=row.painter().with_clip_rect(response.rect);
                if !enabled {
                    painter.rect_filled(
                        response.rect,0.0,
                        theme::RECEPTION_RECIPIENT_NAV_DISABLED_BG
                    );
                }
                paint_management_chevron_with_color(
                    &painter,response.rect.center(),direction,
                    if enabled{theme::RECEPTION_RECIPIENT_NAV_ACTIVE_ICON}
                    else{theme::RECEPTION_RECIPIENT_NAV_DISABLED_ICON}
                );
                if enabled{response.on_hover_text(hint)}else{response}
            };

            let prev=arrow(
                segments,ManagementChevronDirection::Left,
                current>0,"이전 수취인"
            );
            if prev.clicked(){selected=Some(current-1);}

            let fallback=format!("수취인 {}",current+1);
            let display=if name.trim().is_empty(){fallback.as_str()}else{name};
            let natural_width=segments.painter().layout_no_wrap(
                display.to_owned(),
                egui::FontId::proportional(typography::FONT_BODY),
                theme::TEXT
            ).size().x;
            let content_width=(name_width-2.0*theme::SPACE_8).max(0.0);
            let clipped=natural_width>content_width;
            let label=segments.add_sized(
                Vec2::new(name_width,inner.height()),
                egui::Label::new(typography::body(display))
                    .truncate().halign(Align::Center)
                    .sense(if clipped{Sense::click()}else{Sense::hover()})
            ).on_hover_text(display);
            if clipped {
                // Long names stay within the header slot; click the name
                // to inspect its entire value in a wrapping anchored popup.
                egui::Popup::menu(&label)
                    .close_behavior(
                        egui::PopupCloseBehavior::CloseOnClickOutside
                    )
                    .show(|popup|{
                        popup.set_max_width(320.0);
                        popup.add(
                            egui::Label::new(typography::body(display)).wrap()
                        );
                    });
            }

            let next=arrow(
                segments,ManagementChevronDirection::Right,
                current+1<total,"다음 수취인"
            );
            if next.clicked(){selected=Some(current+1);}

            // Progress is informational metadata OUTSIDE the arrow pair:
            // ‹  김준희  ›  |  1/2
            segments.add_sized(
                Vec2::new(count_width,inner.height()),
                egui::Label::new(typography::field_label(
                    &format!("{}/{}",current+1,total)
                )).halign(Align::Center)
            );
            for x in [
                inner.left()+arrow_width,
                inner.left()+arrow_width+name_width,
                inner.left()+2.0*arrow_width+name_width
            ]{
                segments.painter().line_segment(
                    [
                        egui::pos2(x,inner.top()+4.0),
                        egui::pos2(x,inner.bottom()-4.0)
                    ],
                    Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
                );
            }
        }
    );
    ui.painter().rect_stroke(
        outer.shrink(0.5),theme::CONTROL_RADIUS as f32,
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER),
        egui::StrokeKind::Inside
    );
    selected
}

/// Every recipient field shares one 36pt baseline. The two telephone
/// editors occupy the same FULL-WIDTH row: mobile begins at the same x
/// coordinate as the other text inputs, and the landline fills the right.
/// The minimum widths preserve Korean telephone numbers; extra space is
/// distributed to BOTH editors rather than left empty. The two labels and
/// editors are siblings in one horizontal layout (no nested line offsets).
/// Only the five actual input controls are hover/highlight targets.
/// Static Korean field labels remain in their normal theme color.
pub fn reception_recipient_fields(
    ui: &mut Ui,
    name: &mut String,
    address: &mut String,
    mobile: &mut String,
    phone: &mut String,
    note: &mut String,
    active: [bool; 5],
) -> [Response; 5] {
    fn editor(value: &mut String, width: f32, active: bool) -> egui::TextEdit<'_> {
        let input=egui::TextEdit::singleline(value)
            .font(egui::FontId::proportional(
                typography::FONT_RECEPTION_RECIPIENT_TEXT
            ))
            .vertical_align(Align::Center)
            .margin(egui::Margin::symmetric(8, 2))
            .desired_width(width);
        if active {
            input.background_color(source_highlight::ACTIVE_FILL)
                .text_color(source_highlight::ACTIVE_TEXT)
        } else {input}
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

    fn row(ui: &mut Ui, label: &str, value: &mut String, active: bool) -> Response {
        ui.horizontal(|line| {
            line.spacing_mut().item_spacing.x = theme::RECIPIENT_LABEL_GAP;
            label_cell(line, label, theme::RECIPIENT_LABEL_WIDTH);
            let width = line.available_width().max(1.0);
            // The response belongs to the input, NOT the combined row.
            // This is read-only while retaining hover and native text layout.
            line.add_sized(
                Vec2::new(width, theme::INPUT_HEIGHT),
                editor(value, width, active).interactive(false),
            )
        }).inner
    }

    ui.scope(|fields| {
        fields.spacing_mut().item_spacing.y = theme::RECIPIENT_ROW_GAP;
        let name_response = row(fields, "이름", name, active[0]);
        let address_response = row(fields, "주소", address, active[1]);

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
                    label_cell(line, "휴대폰", theme::RECIPIENT_LABEL_WIDTH);
                    let mobile_input = line.add_sized(
                        Vec2::new(mobile_width, theme::INPUT_HEIGHT),
                        editor(mobile, mobile_width, active[2]).interactive(false),
                    );
                    // Only the gap between the TWO contact groups is 8pt.
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_PHONE_COLUMN_GAP;
                    label_cell(line, "전화", theme::RECIPIENT_PHONE_LABEL_WIDTH);
                    line.spacing_mut().item_spacing.x =
                        theme::RECIPIENT_LABEL_GAP;
                    let phone_input = line.add_sized(
                        Vec2::new(phone_width, theme::INPUT_HEIGHT),
                        editor(phone, phone_width, active[3]).interactive(false),
                    );
                    (mobile_input,phone_input)
                }).inner
            }).inner;
        let note_response = row(fields, "메시지", note, active[4]);
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

/// The 70pt client identification band is INFORMATION, not an editable
/// source document. Keep its exact reserved Rect and use the management
/// master's muted inner-row colors to establish a quieter visual tier.
pub fn paint_reception_client_summary_surface(ui:&Ui,rect:Rect){
    ui.painter().rect(
        rect,theme::CONTROL_RADIUS as f32,
        theme::RECEPTION_CLIENT_SUMMARY_BG,
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,
            theme::RECEPTION_CLIENT_SUMMARY_BORDER),
        egui::StrokeKind::Inside,
    );
}

/// Reception section titles are NOT fixed table headers. Keep a soft
/// 1pt DIVIDER under them; the strong 2pt TABLE_HEADER_OUTLINE belongs
/// exclusively to the management/list column headers.
/// Preserve the existing one-point slot and all dock splitter coordinates.
pub fn reception_section_divider(ui:&mut Ui){
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::SOFT_DIVIDER_WIDTH),Sense::hover()
    );
    ui.painter().rect_filled(rect,0.0,theme::DIVIDER);
}

/// Readability for actionable recognition/workflow issues. Full-width
/// variable-height rows wrap long content without clipping or creating
/// another bordered card inside the Reception docked pane.
pub fn reception_review_status(ui:&mut Ui,message:&str){
    let width=ui.available_width();
    let output=Frame::new()
        .fill(theme::TABLE_ROW_ALT_BG)
        .corner_radius(theme::CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            theme::SPACE_8 as i8,theme::SPACE_4 as i8
        ))
        .show(ui,|content|{
            content.set_min_width((width-2.0*theme::SPACE_8).max(1.0));
            content.add(
                egui::Label::new(typography::status(message,theme::SELECTED_ORDER))
                    .wrap()
            );
        });
    let rect=output.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left()+theme::WORKFLOW_SELECTED_STRIPE,rect.bottom())
        ),0.0,theme::SELECTED_ORDER
    );
}

/// The message editor/footer uses a 12pt spacer with a 1pt centered divider.
/// The caller subtracts the SAME token before sizing the editable canvas.
pub fn reception_message_footer_gap(ui:&mut Ui){
    let (gap_rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::RECEPTION_MESSAGE_ACTION_GAP),Sense::hover()
    );
    let y=gap_rect.center().y;
    ui.painter().line_segment(
        [egui::pos2(gap_rect.left(),y),egui::pos2(gap_rect.right(),y)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
}

/// One shared action layout for preview and working states. Button clicks
/// return the requested Rust copy variant (false=unchanged, true=greeting);
/// the caller alone owns clipboard writes and workflow-safe enablement.
pub fn reception_message_copy_actions(ui:&mut Ui,enabled:bool)->Option<bool>{
    let mut greeting=None;
    if ui.available_width()<280.0 {
        ui.with_layout(Layout::top_down(Align::Max),|actions|{
            actions.spacing_mut().item_spacing.y=theme::SPACE_8;
            if theme::action(actions,"그대로 복사",theme::ButtonRole::Standard,enabled)
                .clicked(){greeting=Some(false);}
            if theme::action(actions,"인사말 포함 복사",
                theme::ButtonRole::Standard,enabled).clicked(){greeting=Some(true);}
        });
    }else{
        ui.with_layout(Layout::right_to_left(Align::Center),|actions|{
            actions.spacing_mut().item_spacing.x=theme::SPACE_8;
            if theme::action(actions,"인사말 포함 복사",
                theme::ButtonRole::Standard,enabled).clicked(){greeting=Some(true);}
            if theme::action(actions,"그대로 복사",theme::ButtonRole::Standard,enabled)
                .clicked(){greeting=Some(false);}
        });
    }
    greeting
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
/// Shared calendar navigation: identical 28pt hit targets, border, radius,
/// and vector chevrons for year/month movement in either direction.
fn calendar_nav_button(
    ui:&mut Ui,direction:ManagementChevronDirection,
    year:bool,tooltip:&str
)->bool{
    let response=ui.add_sized(
        Vec2::splat(theme::CALENDAR_NAV_SIZE),
        egui::Button::new("")
            .fill(theme::INPUT)
            .stroke(Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER))
            .corner_radius(theme::CONTROL_RADIUS)
    ).on_hover_text(tooltip);
    let ink=if response.hovered(){theme::TEXT}else{theme::SECONDARY};
    let painter=ui.painter().with_clip_rect(response.rect);
    if year{
        for offset in [-3.8,3.8]{
            paint_management_chevron_with_color(
                &painter,response.rect.center()+egui::vec2(offset,0.0),
                direction,ink
            );
        }
    }else{
        paint_management_chevron_with_color(
            &painter,response.rect.center(),direction,ink
        );
    }
    response.clicked()
}

/// A single popup for Order History and Excel export history. The header
/// has the exact width of the seven-column date grid, so the month label
/// and four navigation buttons remain aligned at every month length.
pub fn date_calendar(
    trigger:&egui::Response,month:&mut NaiveDate,selected:&str,
    grid_id:&'static str
)->Option<NaiveDate>{
    let mut chosen=None;
    let selected_date=NaiveDate::parse_from_str(selected,"%Y-%m-%d").ok();
    let today=(chrono::Utc::now()+Days::hours(9)).date_naive();
    egui::Popup::menu(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|calendar|{
            calendar.spacing_mut().item_spacing=egui::vec2(
                theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
            );
            calendar.set_min_width(theme::CALENDAR_GRID_WIDTH);
            calendar.horizontal(|nav|{
                nav.spacing_mut().item_spacing.x=theme::CALENDAR_GRID_GAP;
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,true,"이전 연도"
                ){
                    *month=shift_calendar_month(*month,-12);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,false,"이전 달"
                ){
                    *month=shift_calendar_month(*month,-1);
                }
                let title_width=theme::CALENDAR_GRID_WIDTH
                    -4.0*theme::CALENDAR_NAV_SIZE
                    -4.0*theme::CALENDAR_GRID_GAP;
                nav.add_sized(
                    egui::vec2(title_width,theme::CALENDAR_NAV_SIZE),
                    egui::Label::new(typography::field_label(&format!(
                        "{}년 {}월",month.year(),month.month()
                    ))).halign(Align::Center)
                );
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,false,"다음 달"
                ){
                    *month=shift_calendar_month(*month,1);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,true,"다음 연도"
                ){
                    *month=shift_calendar_month(*month,12);
                }
            });
            calendar.add_space(theme::SPACE_8);
            calendar.separator();
            calendar.add_space(theme::SPACE_4);
            let width=theme::CALENDAR_DAY_SIZE;
            let dates=calendar_dates(*month);
            egui::Grid::new(grid_id)
                .num_columns(7)
                .spacing(egui::vec2(
                    theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
                ))
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
                            )).corner_radius(theme::CONTROL_RADIUS);
                            if selected_date==Some(*date){
                                button=button.fill(theme::WORKFLOW_SELECTED_BG)
                                    .stroke(Stroke::new(1.0,theme::SELECTION));
                            }else if *date==today{
                                button=button.stroke(Stroke::new(1.0,theme::BORDER));
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

/// Shared numeric KPI card used by the two approved management masters
/// and any other page copying their heading / tabs / KPI hierarchy.
/// Keep the 228pt card, 28pt value, 13pt caption and insets in one place.
pub fn management_summary_card(ui:&mut Ui,label:&str,count:usize){
    let card_width=theme::FREIGHT_SUMMARY_CARD_WIDTH;
    ui.allocate_ui_with_layout(
        egui::vec2(card_width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |slot|{
            slot.set_width(card_width);
            theme::freight_summary_frame().show(slot,|cell|{
                cell.spacing_mut().item_spacing.y=0.0;
                let content_width=card_width-2.0*theme::FREIGHT_SUMMARY_PADDING_X
                    -2.0*theme::FRAME_STROKE_WIDTH;
                let (number_row,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                cell.add_space(theme::FREIGHT_SUMMARY_TEXT_GAP);
                let (label_row,_)=cell.allocate_exact_size(
                    egui::vec2(content_width,theme::FREIGHT_SUMMARY_LABEL_LINE_HEIGHT),
                    egui::Sense::hover()
                );
                let number=typography::freight_summary_large_number_galley(
                    cell,&count.to_string()
                );
                let caption=typography::freight_summary_small_label_galley(cell,label);
                cell.painter().galley(
                    egui::pos2(
                        number_row.center().x-number.size().x/2.0,
                        number_row.center().y-number.size().y/2.0
                    ),number,theme::TEXT
                );
                cell.painter().galley(
                    egui::pos2(
                        label_row.center().x-caption.size().x/2.0,
                        label_row.center().y-caption.size().y/2.0
                    ),caption,theme::SECONDARY
                );
            });
        }
    );
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

/// One shared 7pt rail extension for EVERY management table, regardless of
/// its page, outer frame or footer. Each caller owns ONLY the viewport height
/// (and reserves its footer separately); the scrollbar offset is not a
/// per-page layout policy. The native 3pt bottom inset leaves 4pt beyond rows.
pub fn management_table_scroll_track_rect(
    ui:&Ui,scroll_height:f32
)->Rect {
    management_table_scroll_track_geometry(
        ui.available_rect_before_wrap(),scroll_height
    )
}

/// The only place that computes top/bottom of a management table rail.
/// The rail starts exactly below its fixed header (0pt gap); 7pt of common
/// extension minus 3pt optical shortening puts its end 4pt past the body.
/// The row viewport, data columns and any reserved footer stay unchanged.
fn management_table_scroll_track_geometry(
    bounds:Rect,scroll_height:f32
)->Rect {
    debug_assert!(scroll_height.is_finite()&&scroll_height>=0.0);
    let rows_bottom=(bounds.top()+scroll_height).min(bounds.bottom());
    let track_bottom=(rows_bottom+theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION
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

/// A history overview without interactive toolbar controls still has the
/// same fixed-header origin as the assignment master: 36pt toolbar role plus
/// the approved 21pt transition. Reserve those 57pt as ONE separator area,
/// not a phantom interactive toolbar widget with its own implicit spacing.
/// This keeps the first row and the 1pt divider on the master coordinates.
pub fn management_sticky_table_header_without_toolbar(
    ui:&mut Ui,min_table_width:f32
)->ManagementTableHeader {
    management_sticky_table_header_with_top_gap(
        ui,min_table_width,
        theme::INPUT_HEIGHT+theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP
    )
}

/// Paint a freight master header's background and return its 34pt text area.
/// Shared by assignment, group management and combination history so text,
/// arrow centers and sort hit rects never use the lower 2pt outline.
pub fn management_master_header_begin(
    ui:&Ui,header_rect:Rect,header_content_rect:Rect
)->Rect {
    debug_assert!((header_rect.height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT)
        .abs()<0.01);
    let text_rect=Rect::from_min_max(
        header_content_rect.min,
        egui::pos2(
            header_content_rect.right(),
            header_content_rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
        )
    );
    debug_assert!((text_rect.height()-
        (theme::MANAGEMENT_TABLE_HEADER_HEIGHT-
            theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT)).abs()<0.01);
    ui.painter().with_clip_rect(header_rect)
        .rect_filled(header_rect,0.0,theme::RAISED);
    text_rect
}

/// Draw the master header's solid 2pt bottom band AFTER labels, separators
/// and hover chrome. A centered 1pt line at bottom() is half clipped.
pub fn management_master_header_finish(ui:&Ui,header_rect:Rect){
    ui.painter().with_clip_rect(header_rect).rect_filled(
        Rect::from_min_max(
            egui::pos2(
                header_rect.left(),
                header_rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
            ),header_rect.right_bottom()
        ),0.0,theme::TABLE_HEADER_OUTLINE
    );
}

/// Identical sort-hover outline for all freight management list headers.
/// The caller supplies the column intersected with the 34pt text area.
pub fn management_master_header_sort_hover(ui:&Ui,hit:Rect){
    if hit.width()>0.0 && hit.height()>0.0 {
        ui.painter().with_clip_rect(hit).rect_stroke(
            hit.shrink(1.0),2.0,
            Stroke::new(1.0,theme::BORDER),
            egui::StrokeKind::Inside
        );
    }
}

/// Plain management-register header for two-column editor/list pages.
/// The same master allocates the 21pt toolbar transition, 36pt fixed header,
/// native scrollbar gutter and inner 2pt bottom rule. Only the row content
/// is allowed to be taller than a compact 42pt product row.
pub fn management_master_register_header(
    ui:&mut Ui,primary:&str,action:&str,action_width:f32
)->ManagementTableHeader{
    let layout=management_sticky_table_header(ui,0.0);
    let text_rect=management_master_header_begin(
        ui,layout.header_rect,layout.header_content_rect
    );
    let x=(text_rect.right()-action_width).max(text_rect.left());
    let left=typography::freight_table_header_galley(ui,primary);
    let right=typography::freight_table_header_galley(ui,action);
    let painter=ui.painter().with_clip_rect(text_rect);
    // Long alias/brand titles must not run under the right-hand
    // management column on a narrow workspace.
    let primary_rect=Rect::from_min_max(
        text_rect.min,egui::pos2(x,text_rect.bottom())
    );
    painter.with_clip_rect(primary_rect).galley(
        egui::pos2(text_rect.left()+theme::SPACE_8,
            text_rect.center().y-left.size().y/2.0),
        left,theme::SECONDARY
    );
    painter.galley(
        egui::pos2((x+(action_width-right.size().x)/2.0)
            .max(x).min(text_rect.right()),
            text_rect.center().y-right.size().y/2.0),
        right,theme::SECONDARY
    );
    painter.line_segment(
        [egui::pos2(x,text_rect.top()),egui::pos2(x,text_rect.bottom())],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
    management_master_header_finish(ui,layout.header_rect);
    layout
}

/// One 10pt / 1.8pt-stroke chevron for selectors, date stepping and
/// expand/collapse affordances. Rotate this same vector for every screen.
/// Draw the TWO diagonal arms as ONE continuous path. Separate
/// line_segment() calls blend anti-aliased coverage twice at their shared
/// corner, making the arrow tip darker and giving inconsistent edge colors
/// on Windows / fractional DPI. A single path has one joined stroke.
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
    // One tessellated stroke owns the corner, diagonal edges and caps.
    // Preserve the approved 10pt shape, rotation and 1.8pt thickness.
    painter.add(egui::Shape::line(
        vec![a,tip,b],Stroke::new(STROKE,ink)
    ));
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
    let caption=typography::management_control_text_galley(
        ui,label,ink
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
    let primary=typography::management_table_cell_galley(
        ui,title,typography::ManagementTableCellRole::ClientName
    );
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

/// Shared management-table column separators. Freight Assignment and
/// Client Management both call this painter for header and body instead
/// of leaving vertical rules to independently implemented screen widgets.
///
/// The caller supplies DATA coordinates, not window coordinates. `region`
/// clips the header before its scrollbar gutter and keeps body rules inside
/// each virtualized row. Header bottom 2pt outline remains owned by the
/// existing `management_master_header_finish` function.
pub fn management_master_column_rules(
    ui:&Ui,region:Rect,boundaries:&[f32]
){
    let clip=region.intersect(ui.clip_rect());
    if clip.width()<=0.0 || clip.height()<=0.0{return;}
    let painter=ui.painter().with_clip_rect(clip);
    for &x in boundaries{
        if x>clip.left() && x<clip.right() {
            painter.line_segment(
                [egui::pos2(x,region.top()),egui::pos2(x,region.bottom())],
                Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
            );
        }
    }
}

/// The client header and row share the same 30/38/32 column positions,
/// including the scroll-offset table origin. Do not recompute these from
/// the fixed header rect, which stays in place while body rows scroll.
fn management_client_column_edges(table_left:f32,table_width:f32)->[f32;2]{
    let first=table_left+table_width*theme::MANAGEMENT_CLIENT_COLS[0];
    let second=first+table_width*theme::MANAGEMENT_CLIENT_COLS[1];
    [first,second]
}

/// Client columns share the EXACT 36pt / 34pt text / 2pt footer band
/// of Freight Assignment. Column widths and 12pt label starts still
/// belong to the client row contract; only the master chrome is reused.
/// The visible data rect excludes the scrollbar, and horizontally scrolled
/// labels move together with the row data rather than with the rail.
pub fn management_client_header(
    ui:&mut Ui,header:Rect,header_content_rect:Rect,
    table_left:f32,table_width:f32,current_sort:Option<(usize,bool)>
)->Option<usize>{
    let titles=["거래처","주소 / 전화번호","거래 조건"];
    let sort_keys=["거래처명","주소","가격 등급"];
    let text_row=management_master_header_begin(
        ui,header,header_content_rect
    );
    let painter=ui.painter().with_clip_rect(text_row);
    let mut x=table_left;
    let mut selected=None;
    for (i,title) in titles.iter().enumerate(){
        let col_width=table_width*theme::MANAGEMENT_CLIENT_COLS[i];
        let column=Rect::from_min_max(
            egui::pos2(x,text_row.top()),
            egui::pos2(x+col_width,text_row.bottom())
        );
        let visible=column.intersect(text_row);
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
                management_master_header_sort_hover(ui,visible);
            }
            let text_rect=Rect::from_min_max(
                egui::pos2(column.left()+theme::SPACE_12,text_row.top()),
                egui::pos2(column.right()-theme::SPACE_12,text_row.bottom())
            ).intersect(text_row);
            if text_rect.width()>0.0 {
                let title_galley=typography::management_table_header_galley(ui,title);
                let (arrow,color)=match current_sort{
                    Some((col,false)) if col==i=>("▲",theme::SELECTION),
                    Some((col,true)) if col==i=>("▼",theme::SELECTION),
                    _=>("↕",theme::MUTED)
                };
                let arrow_galley=typography::management_table_header_galley(ui,arrow);
                let title_x=column.left()+theme::SPACE_12;
                let clip=painter.with_clip_rect(text_rect);
                clip.galley(
                    egui::pos2(title_x,
                        text_row.center().y-title_galley.size().y/2.0),
                    title_galley.clone(),theme::SECONDARY
                );
                clip.galley(
                    egui::pos2(title_x+title_galley.size().x+theme::SPACE_4,
                        text_row.center().y-arrow_galley.size().y/2.0),
                    arrow_galley,color
                );
            }
        }
        x+=col_width;
    }
    management_master_column_rules(
        ui,text_row,
        &management_client_column_edges(table_left,table_width)
    );
    management_master_header_finish(ui,header);
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
    // Paint the 1pt row rule inside its allocated rect. A centered
    // line_segment on bottom() was half-clipped and looked inconsistent
    // against the master's crisp 2pt header band at fractional DPI.
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(rect.left(),
                rect.bottom()-theme::SOFT_DIVIDER_WIDTH),
            rect.right_bottom()
        ),0.0,theme::DIVIDER
    );

    let mut x=rect.left();
    for (i,(primary,secondary)) in columns.iter().enumerate() {
        let col_width=rect.width()*theme::MANAGEMENT_CLIENT_COLS[i];
        let cell=Rect::from_min_max(
            egui::pos2(x+theme::SPACE_12,rect.top()),
            egui::pos2(x+col_width-theme::SPACE_12,rect.bottom())
        );
        // The '주소 / 전화번호' heading and the first address glyph
        // share the same left anchor, not a centered label allocation.
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
                // A real phone is primary business data, not muted
                // metadata. Only the client-name secondary label keeps
                // the smaller and quieter visual hierarchy.
                let metadata=primary.is_empty() && i==0;
                let galley=if metadata {
                    typography::client_table_metadata_galley(
                        ui,visible,theme::MUTED
                    )
                }else{
                    typography::management_table_cell_galley(
                        ui,visible,if i==0 {
                            typography::ManagementTableCellRole::ClientName
                        }else{
                            typography::ManagementTableCellRole::Text
                        }
                    )
                };
                paint_client_line(
                    ui,cell,cell.left(),galley,
                    if metadata{theme::MUTED}else{theme::TEXT}
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
                typography::management_table_cell_galley(
                    ui,primary,if i==0 {
                        typography::ManagementTableCellRole::ClientName
                    }else{
                        typography::ManagementTableCellRole::Text
                    }
                ),
                theme::TEXT
            );
            // The address and its paired phone number are equally
            // important. Retain metadata styling only for the dealer
            // name's secondary description.
            let secondary_galley=if i==1 {
                typography::management_table_cell_galley(
                    ui,secondary,typography::ManagementTableCellRole::Text
                )
            }else{
                typography::client_table_metadata_galley(
                    ui,secondary,theme::MUTED
                )
            };
            paint_client_line(
                ui,second,second.left(),secondary_galley,
                if i==1 {theme::TEXT} else {theme::MUTED}
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
    management_master_column_rules(
        ui,rect,
        &management_client_column_edges(rect.left(),rect.width())
    );
    response
}

#[cfg(test)]
mod management_client_column_divider_tests{
    use super::*;
    #[test]
    fn table_header_and_rows_share_the_same_three_column_edges(){
        let expected=[130.0,320.0];
        // 30/38/32 shares at width 500.0. The header follows the
        // horizontally scrolled row origin rather than its fixed outer edge.
        assert_eq!(management_client_column_edges(-20.0,500.0),
            [-20.0+150.0,-20.0+340.0]);
        assert_eq!(management_client_column_edges(0.0,500.0),
            [150.0,340.0]);
        assert_eq!(expected,management_client_column_edges(-20.0,500.0));
    }
    #[test]
    fn narrow_table_keeps_boundaries_ordered_and_inside_data_width(){
        for width in [theme::MANAGEMENT_CLIENT_TABLE_MIN_WIDTH,900.0,1200.0]{
            let edges=management_client_column_edges(25.0,width);
            assert!(edges[0]>25.0);
            assert!(edges[1]>edges[0]);
            assert!(edges[1]<25.0+width);
        }
    }
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
/// Stable grid geometry for both the item header and rows.
/// The product column uses remaining viewport space, but the numeric columns
/// retain exactly 76pt and 136pt regardless of content or window dimensions.
/// Below the minimum, the entire grid scrolls instead of compressing columns.
fn reception_item_table_width(available:f32)->f32 {
    available.max(theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH)
}
fn reception_item_columns(rect: Rect) -> [Rect; 3] {
    let qty=theme::RECEPTION_ITEMS_QTY_WIDTH;
    let amount=theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH;
    let name_right=rect.right()-qty-amount;
    let qty_right=name_right+qty;
    [
        Rect::from_min_max(rect.min,egui::pos2(name_right,rect.bottom())),
        Rect::from_min_max(egui::pos2(name_right,rect.top()),
            egui::pos2(qty_right,rect.bottom())),
        Rect::from_min_max(egui::pos2(qty_right,rect.top()),rect.max),
    ]
}

/// Ellipsize only the displayed product label, using the exact 16pt glyph
/// width. This does not alter the Rust item title, matching key or hover text.
/// Unicode char boundaries prevent cutting a Hangul syllable or UTF-8 byte.
fn reception_ellipsized_product(
    full:&str,max_width:f32,mut measure:impl FnMut(&str)->f32
)->String {
    if max_width<=0.0 {return String::new();}
    if measure(full)<=max_width {return full.to_owned();}
    if measure("…")>max_width {return String::new();}
    let boundaries:Vec<usize>=full.char_indices().map(|(i,_)|i)
        .chain(std::iter::once(full.len())).collect();
    let mut low=0usize;
    let mut high=boundaries.len()-1;
    while low<high {
        let mid=low+(high-low+1)/2;
        let candidate=format!("{}…",&full[..boundaries[mid]]);
        if measure(&candidate)<=max_width {low=mid;}
        else {high=mid-1;}
    }
    format!("{}…",full[..boundaries[low]].trim_end())
}

/// A deliberately compact presentation for exceptional quantities.
/// Backend quantities, pricing and full-value tooltips remain untouched.
fn reception_compact_quantity(full:&str)->&str {
    let count=full.strip_suffix('개')
        .and_then(|digits|digits.parse::<u64>().ok());
    if count.is_some_and(|n|n>=1000) {"999+"} else {full}
}

pub struct ReceptionItemCells {
    pub product: Response,
    pub quantity: Response,
    pub amount: Response,
}

/// Product text needs equal breathing room on both sides of the blue
/// outline. Header and rows use the same 4pt extra horizontal inset;
/// quantity/amount alignment and highlight geometry remain untouched.
fn reception_item_text_clip(rect:Rect,index:usize)->Rect {
    if index==0 {
        rect.shrink2(egui::vec2(theme::SPACE_4,0.0))
    }else{rect}
}

pub fn reception_items_header(ui: &mut Ui) {
    let width = reception_item_table_width(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_HEADER_HEIGHT), Sense::hover()
    );
    // The reception list has its own approved 32pt compact header, but
    // follows the master's raised header surface and INSIDE bottom band.
    // No extra layout height or clipped 1pt edge is introduced.
    ui.painter().rect_filled(rect,0.0,theme::RAISED);
    let columns = reception_item_columns(rect);
    for (index, label) in ["품목", "수량", "금액"].iter().enumerate() {
        // Header and value cells use identical text start coordinates.
        let cell = reception_item_text_clip(
            columns[index].shrink2(egui::vec2(theme::SPACE_8, 0.0)),index
        );
        let galley = typography::management_table_header_galley(ui, label);
        // The product header starts at the same LEFT edge as product text;
        // quantity and amount end at the same RIGHT edges as their values.
        let x = if index == 0 {
            cell.left()
        } else {
            cell.right() - galley.size().x
        };
        let text_bottom = cell.bottom() - theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT;
        let text_center_y = (cell.top() + text_bottom) / 2.0;
        ui.painter().with_clip_rect(cell).galley(
            egui::pos2(x, text_center_y - galley.size().y / 2.0),
            galley, theme::SECONDARY,
        );
    }
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(
                rect.left(),
                rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
            ),
            rect.right_bottom()
        ),
        0.0,theme::TABLE_HEADER_OUTLINE
    );
}

/// Use the actual laid-out glyph box for all value cells.
/// Product text is left aligned; quantity and amount share a right edge and
/// the exact same vertical midpoint independently of their string length.
fn reception_item_value_origin(cell:Rect,glyphs:Vec2,right:bool)->egui::Pos2 {
    egui::pos2(
        if right {cell.right()-glyphs.x} else {cell.left()},
        cell.center().y-glyphs.y/2.0,
    )
}

pub fn reception_item_row(
    ui: &mut Ui, name: &str, quantity: &str, amount: &str,
    active: [bool; 3],
) -> ReceptionItemCells {
    let width = reception_item_table_width(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::RECEPTION_ITEMS_ROW_HEIGHT), Sense::hover()
    );
    let columns = reception_item_columns(rect);
    // A 42pt row previously lost 8pt top and bottom, leaving a 26pt
    // highlight rectangle. Give 16pt glyphs a 34pt value surface instead;
    // the 4pt inset keeps both rounded corners and the border inside the row.
    let padded = |index: usize| columns[index].shrink2(
        egui::vec2(theme::SPACE_4, theme::SPACE_4)
    );
    let cells = [padded(0), padded(1), padded(2)];
    let compact_quantity=reception_compact_quantity(quantity);
    // Use explicit column anchors for all three text cells. Ui::put(Label)
    // was moving text toward the center inside the row, while the table
    // header used another alignment calculation.
    // One 16pt proportional font for product names, digits and Korean units.
    // Mono font fallback gave quantity digits and '개' differing visual heights.
    for (index, (text, color)) in [
        (name, theme::TEXT),
        (quantity, theme::SECONDARY),
        (amount, theme::TEXT),
    ].into_iter().enumerate() {
        let font=egui::FontId::proportional(typography::FONT_RECEPTION_ITEM_TEXT);
        let cell = cells[index];
        let right = index != 0;
        let row_text=if index==1 {compact_quantity} else {text};
        // Draw the same blue fill as recipient TextEdit, before value glyphs.
        // No header, label, surrounding table row or column divider is filled.
        if active[index] {
            source_highlight::paint_active_background(ui,cell);
        }
        // Text must stay clear of the 2pt inset outline. Keeping the
        // value clip and its anchor inside the highlighted cell also stops
        // the last digit / descender from touching a blue edge.
        let value_clip=reception_item_text_clip(
            cell.shrink2(egui::vec2(theme::SPACE_4,0.0)),index
        );
        // Use the same font metrics as Painter::text, not a character-count
        // guess. Only the product column truncates; financial values do not.
        let visible_product=(index==0).then(||reception_ellipsized_product(
            text,(value_clip.width()-0.5).max(0.0),
            |candidate|ui.painter().layout_no_wrap(
                candidate.to_owned(),font.clone(),color
            ).size().x,
        ));
        let ink=if active[index] {source_highlight::ACTIVE_TEXT} else {color};
        let label=visible_product.as_deref().unwrap_or(row_text);
        let galley=ui.painter().layout_no_wrap(label.to_owned(),font,ink);
        let origin=reception_item_value_origin(value_clip,galley.size(),right);
        ui.painter().with_clip_rect(value_clip).galley(origin,galley,ink);
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
    ).on_hover_text(quantity);
    let amount = ui.interact(
        cells[2], ui.make_persistent_id(("item-cell", row_id, 2)), Sense::hover(),
    ).on_hover_text(amount);
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
    let mut amount_text=amount
        .map(|value|display_won(value).trim_end_matches('원').to_owned())
        .unwrap_or_else(||"—".to_owned());
    ui.horizontal_wrapped(|line| {
        line.spacing_mut().item_spacing=Vec2::new(theme::SPACE_4,theme::SPACE_4);
        // Keep the label, 36pt input, and its currency unit together in a
        // single centered row. The options may wrap independently as before.
        // 8pt left inset matches product/header text in the item grid.
        line.allocate_ui_with_layout(
            Vec2::new(168.0,theme::INPUT_HEIGHT),
            Layout::left_to_right(Align::Center),
            |price|{
                price.spacing_mut().item_spacing.x=theme::SPACE_4;
                price.add_space(theme::SPACE_8);
                let (label_rect,_)=price.allocate_exact_size(
                    Vec2::new(40.0,theme::INPUT_HEIGHT),Sense::hover()
                );
                let galley=price.painter().layout_no_wrap(
                    label.to_owned(),
                    egui::FontId::proportional(typography::FONT_RECEPTION_ITEM_TEXT),
                    theme::TEXT,
                );
                price.painter().galley(
                    egui::pos2(
                        label_rect.left(),
                        label_rect.center().y-galley.size().y/2.0
                    ),
                    galley,theme::TEXT
                );
                // The exact amount (including the unknown '—') is
                // right-aligned. It remains a disabled, native 36pt input.
                price.add_enabled_ui(false,|cell|{
                    cell.add_sized(
                        Vec2::new(84.0,theme::INPUT_HEIGHT),
                        egui::TextEdit::singleline(&mut amount_text)
                            .font(egui::FontId::proportional(
                                typography::FONT_RECEPTION_ITEM_TEXT
                            ))
                            .horizontal_align(Align::RIGHT)
                            .vertical_align(Align::Center)
                            .margin(egui::Margin::symmetric(8,2))
                            .desired_width(84.0),
                    )
                });
                price.label(typography::body("원")
                    .size(typography::FONT_RECEPTION_ITEM_TEXT));
            }
        );
        if let Some(collect)=collect {
            line.label(typography::body(
                if collect {"(착불)"} else {"(선불)"}
            ).size(typography::FONT_RECEPTION_ITEM_TEXT));
        }
        if let Some(checked)=remote {
            let mut read_only=checked;
            line.add_enabled(false,egui::Checkbox::new(
                &mut read_only,"도서/산간"
            ));
        }
        if inquiry_visible {
            theme::action(
                line,"공장에 운임 문의",theme::ButtonRole::Standard,false
            );
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
    fn all_freight_headers_share_34pt_text_area_and_2pt_bottom_rule(){
        let ctx=egui::Context::default();
        let mut observed=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let header=Rect::from_min_size(
                    egui::pos2(16.0,216.0),Vec2::new(900.0,36.0)
                );
                let content=Rect::from_min_max(
                    header.min,egui::pos2(header.right()-10.0,header.bottom())
                );
                let text=management_master_header_begin(ui,header,content);
                management_master_header_finish(ui,header);
                observed=Some((header,content,text));
            });
        });
        let (header,content,text)=observed.expect("header geometry");
        assert_eq!(text.top(),header.top());
        assert_eq!(text.bottom(),header.bottom()-2.0);
        assert_eq!(text.height(),34.0);
        assert_eq!(text.right(),content.right());
        assert_eq!(header.center().y-text.center().y,1.0,
            "full-height centering must not be reused");
        assert_eq!(theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT,2.0);
    }

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
    fn product_name_column_header_has_one_short_shared_label(){
        // Freight Assignment and Product List must both use this semantic
        // column label; the product-description value stays in the row.
        assert_eq!(ManagementTableColumnLabel::ProductName.text(),"상품명");
    }

    #[test]
    fn table_checkbox_is_centered_for_freight_and_excel_column_widths(){
        // The 44pt and 48pt first columns differ in width but NOT their
        // widget-centering rule; no 8pt/4pt shrink offsets are permitted.
        for width in [44.0,48.0] {
            let cell=Rect::from_min_size(
                egui::pos2(100.0,200.0),Vec2::new(width,42.0)
            );
            let rect=management_table_checkbox_rect(cell,cell,18.0,4.0);
            assert_eq!(rect.center(),cell.center());
            assert_eq!(rect.height(),theme::INPUT_HEIGHT);
            assert_eq!(rect.width(),22.0);
            assert_eq!(rect.left()-cell.left(),cell.right()-rect.right());
            assert_eq!(rect.top()-cell.top(),cell.bottom()-rect.bottom());
            assert!(cell.contains(rect.min)&&cell.contains(rect.max));
        }
    }

    #[test]
    fn reception_value_origins_share_vertical_center_and_right_edge() {
        let cell=Rect::from_min_size(
            egui::pos2(10.0,104.0),Vec2::new(76.0,34.0)
        );
        let quantity=reception_item_value_origin(cell,Vec2::new(42.0,19.0),true);
        let short=reception_item_value_origin(cell,Vec2::new(10.0,19.0),true);
        let product=reception_item_value_origin(cell,Vec2::new(65.0,19.0),false);
        assert_eq!(quantity.x+42.0,cell.right());
        assert_eq!(short.x+10.0,cell.right());
        assert_eq!(product.x,cell.left());
        for pos in [quantity,short,product] {
            assert_eq!(pos.y+19.0/2.0,cell.center().y);
        }
    }

    #[test]
    fn reception_product_text_keeps_eight_point_gaps_on_both_sides() {
        let row=Rect::from_min_size(
            egui::pos2(0.0,0.0),Vec2::new(480.0,42.0)
        );
        let columns=reception_item_columns(row);
        let highlighted=columns[0].shrink2(
            egui::vec2(theme::SPACE_4,theme::SPACE_4)
        );
        let row_text=reception_item_text_clip(
            highlighted.shrink2(egui::vec2(theme::SPACE_4,0.0)),0
        );
        let header_text=reception_item_text_clip(
            columns[0].shrink2(egui::vec2(theme::SPACE_8,0.0)),0
        );
        assert_eq!(row_text.left()-highlighted.left(),theme::SPACE_8);
        assert_eq!(highlighted.right()-row_text.right(),theme::SPACE_8);
        assert_eq!(row_text.left(),header_text.left());
        assert_eq!(row_text.right(),header_text.right());
        assert_eq!(
            reception_item_text_clip(columns[1],1),columns[1]
        );
        assert_eq!(
            reception_item_text_clip(columns[2],2),columns[2]
        );
    }

    #[test]
    fn reception_item_columns_keep_numeric_widths_on_resize() {
        // Header and every row use the same reserved width; no data value
        // changes the column geometry.
        for viewport in [220.0,350.0,392.0,520.0,820.0] {
            let width=reception_item_table_width(viewport);
            let area=Rect::from_min_size(
                egui::pos2(0.0,0.0),Vec2::new(width,42.0)
            );
            let [product,quantity,amount]=reception_item_columns(area);
            assert_eq!(quantity.width(),theme::RECEPTION_ITEMS_QTY_WIDTH);
            assert_eq!(amount.width(),theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH);
            assert!(product.width()>=theme::RECEPTION_ITEMS_PRODUCT_MIN_WIDTH);
            assert_eq!(product.right(),quantity.left());
            assert_eq!(quantity.right(),amount.left());
            assert_eq!(amount.right(),area.right());
        }
        assert_eq!(reception_item_table_width(220.0),
            theme::RECEPTION_ITEMS_MIN_TABLE_WIDTH);
    }

    #[test]
    fn reception_quantity_displays_up_to_999_then_uses_compact_indicator() {
        assert_eq!(reception_compact_quantity("1개"),"1개");
        assert_eq!(reception_compact_quantity("999개"),"999개");
        assert_eq!(reception_compact_quantity("1000개"),"999+");
        assert_eq!(reception_compact_quantity("12345개"),"999+");
        assert_eq!(reception_compact_quantity("—"),"—");
        assert_eq!(reception_compact_quantity("0개"),"0개");
        let row=Rect::from_min_size(egui::pos2(0.0,0.0),Vec2::new(600.0,42.0));
        let columns=reception_item_columns(row);
        assert_eq!(columns[1].width(),theme::RECEPTION_ITEMS_QTY_WIDTH);
        assert_eq!(columns[2].width(),theme::RECEPTION_ITEMS_AMOUNT_MAX_WIDTH);
    }

    #[test]
    fn reception_product_ellipsis_respects_width_and_korean_boundaries() {
        let measure=|value:&str|value.chars().count() as f32;
        assert_eq!(reception_ellipsized_product("연주용보면대708-1",5.0,measure),"연주용보…");
        assert_eq!(reception_ellipsized_product("짧은품목",8.0,measure),"짧은품목");
        assert_eq!(reception_ellipsized_product("긴품목",1.0,measure),"…");
        assert_eq!(reception_ellipsized_product("긴품목",0.5,measure),"");
        assert_eq!(reception_ellipsized_product("긴품목",0.0,measure),"");
    }

    #[test]
    fn table_header_checkbox_is_centered_above_inner_two_point_rule(){
        let first_col=Rect::from_min_size(
            egui::pos2(100.0,300.0),Vec2::new(48.0,36.0)
        );
        let text_area=Rect::from_min_max(
            egui::pos2(0.0,300.0),
            egui::pos2(600.0,336.0-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT)
        );
        let rect=management_table_checkbox_rect(first_col,text_area,18.0,4.0);
        assert_eq!(rect.center().x,first_col.center().x);
        assert_eq!(rect.center().y,text_area.center().y);
        assert_eq!(rect.height(),34.0);
        assert_eq!(rect.bottom(),text_area.bottom());
        assert_eq!(rect.left()-first_col.left(),first_col.right()-rect.right());
        assert!(rect.bottom()<=first_col.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT);
    }

    #[test]
    fn management_table_scroll_policy_is_identical_for_every_table(){
        let bounds=Rect::from_min_size(
            egui::pos2(16.0,300.0),Vec2::new(800.0,484.0)
        );
        let rows_bottom=bounds.bottom();
        let rail=management_table_scroll_track_geometry(bounds,bounds.height());
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION,7.0);
        assert_eq!(theme::MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET,0.0);
        assert_eq!(rail.top(),bounds.top()); // no gap under the header
        assert_eq!(rail.bottom(),rows_bottom+theme::SPACE_4);
        assert_eq!(rail.bottom(),788.0);

        // Neither a list's footer nor a master card's 16pt lower inset
        // changes the shared rail offset. The scroll viewport is unchanged.
        for height in [120.0,400.0,484.0,1200.0] {
            let track=management_table_scroll_track_geometry(bounds,height);
            let viewport_bottom=(bounds.top()+height).min(bounds.bottom());
            assert_eq!(track.bottom(),viewport_bottom+theme::SPACE_4);
        }
        let tiny=management_table_scroll_track_geometry(bounds,1.0);
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
