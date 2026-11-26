//! Modal presentation and geometry. Overlay owns window layers, input priority and lifetime.
use eframe::egui::{self,Align,Color32,Frame,Layout,Rect,Response,Sense,Stroke,Ui,UiBuilder,Vec2};
use crate::{theme,typography};
use crate::theme::*;
use crate::ui_primitives::{truncated_label,section_divider,form_field_label_sized};
use crate::scrollbar::{scroll_area,ScrollAxes};
pub(crate) fn modal_frame() -> Frame {
    bordered_surface(PANEL,BORDER,PANEL_RADIUS)
        .inner_margin(egui::Margin {
            left: MODAL_PANEL_PADDING_X as i8,
            right: MODAL_PANEL_PADDING_X as i8,
            top: MODAL_PANEL_PADDING_Y as i8,
            bottom: MODAL_PANEL_PADDING_Y as i8,
        })
}

/// Nested surface variants describe a content role, not a screen-specific
/// style override. All variants inherit identical fill, border and rounding.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub(crate) enum ModalNestedRole {
    Standard,
    RecipientChecklist,
    CompactTag,
}

pub(crate) fn modal_nested_role_frame(role:ModalNestedRole)->Frame{
    let margin=match role{
        ModalNestedRole::Standard=>egui::Margin::same(SPACE_8 as i8),
        ModalNestedRole::RecipientChecklist=>egui::Margin::symmetric(SPACE_12 as i8,SPACE_8 as i8),
        ModalNestedRole::CompactTag=>egui::Margin::symmetric(SPACE_8 as i8,SPACE_2 as i8),
    };
    bordered_surface(INPUT,DIVIDER,CONTROL_RADIUS).inner_margin(margin)
}

#[cfg(test)]
mod modal_nested_role_tests{
    use super::*;

    #[test]
    fn nested_roles_preserve_approved_margins_and_shared_surface(){
        let standard=modal_nested_role_frame(ModalNestedRole::Standard);
        for (role,x,y) in [
            (ModalNestedRole::Standard,8,8),
            (ModalNestedRole::RecipientChecklist,12,8),
            (ModalNestedRole::CompactTag,8,2),
        ]{
            let frame=modal_nested_role_frame(role);
            assert_eq!(frame.inner_margin,egui::Margin::symmetric(x,y));
            assert_eq!(frame.fill,standard.fill);
            assert_eq!(frame.stroke,standard.stroke);
            assert_eq!(frame.corner_radius,standard.corner_radius);
        }
    }
}

/// OCR review uses the same KakaoTalk-excluded coordinate space as every
/// other modal. Its outer frame is sized and centered entirely inside the
/// approved work area, never the full viewport hidden behind the Kakao dock.
pub(crate) fn modal_ocr_review_rect(work_area:Rect)->Rect{
    let width=(work_area.width()*0.94).clamp(560.0,1480.0)
        .min(work_area.width().max(1.0));
    let height=(work_area.height()*0.88).clamp(450.0,960.0)
        .min(work_area.height().max(1.0));
    Rect::from_center_size(
        work_area.center(),Vec2::new(width,height)
    )
}

/// Size a fixed OCR master inside the common modal frame, not the entire
/// Window::fixed_size rectangle. Both frame margins and the border occupy
/// outer space. Using the outer height for content caused the window to grow
/// past its fixed bounds and clip the rounded bottom edge. Avoid the paint
/// clip for sizing: it may be empty during the first egui layout pass.
pub(crate) fn modal_master_content_from_ui(available:Rect,outer:Vec2)->Vec2{
    let frame_x=theme::MODAL_PANEL_PADDING_X
        +theme::FRAME_STROKE_WIDTH;
    let frame_y=theme::MODAL_PANEL_PADDING_Y
        +theme::FRAME_STROKE_WIDTH;
    let max_inner=Vec2::new(
        (outer.x-2.0*frame_x).max(0.0),
        (outer.y-2.0*frame_y).max(0.0)
    );
    Vec2::new(
        available.width().max(0.0).min(max_inner.x),
        available.height().max(0.0).min(max_inner.y)
    )
}

/// The OCR master is painted in a fixed overlay child. Its parent
/// reserves exactly the content rect once; independently positioned header,
/// image, editor and footer must NOT expand the enclosing Window's min_rect.
/// Using scope_builder directly on that parent caused the window to outgrow
/// fixed_size and lose its lower rounded frame to viewport clipping.
pub(crate) fn modal_master_overlay_ui(parent:&mut Ui,bounds:Rect,id:&'static str)->Ui{
    let inherited_clip=parent.clip_rect();
    let mut overlay=parent.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(bounds)
            .layout(Layout::top_down(Align::Min))
    );
    overlay.set_clip_rect(bounds.intersect(inherited_clip));
    overlay
}

/// A split OCR review needs room for fixed header, footer, toolbar, inset
/// and real image/text viewports. If the window is only in a sizing pass,
/// skip child construction instead of passing a negative size into egui.
pub(crate) fn modal_master_ocr_body_is_usable(body:Vec2)->bool{
    const EXTRA_PANE_HEIGHT:f32=2.0*theme::SPACE_12+4.0;
    let required_height=
        theme::MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT
        +theme::MODAL_FOOTER_HEIGHT
        +2.0*theme::MODAL_MASTER_WORKSPACE_GAP
        +theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT
        +EXTRA_PANE_HEIGHT;
    body.x>=240.0 && body.y>=required_height
}
/// Native modal geometry belongs here, not to individual screens.
/// `work_area` excludes the externally foregrounded KakaoTalk dock.
pub(crate) fn modal_window(
    ctx:&egui::Context,title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    crate::overlay::window(ctx,typography::modal_title(title),egui::Id::new(id))
        .title_bar(false)
        .pivot(egui::Align2::CENTER_CENTER)
        // The caller already excluded navigation and KakaoTalk.
        .fixed_pos(work_area.center())
        // Keep both the pivot and the actual window within the SAME safe
        // rectangle. A second header offset pushed tall dialogs upward,
        // where their top edge could be clipped by the viewport.
        .constrain_to(work_area)
        .max_size(work_area.size())
        // All app dialogs are fixed at the approved Kakao-safe center.
        // Scrolling inside a dialog is allowed; dragging and edge-resizing
        // the dialog itself are not. Individual callers must not override it.
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .frame(crate::popup::modal_frame())
}

/// Runtime-measured rects for the common editable modal stack.
/// These are actual egui allocation results, not inferred pixel offsets.
#[derive(Clone,Debug,Default)]
pub(crate) struct ModalEditorBodyGeometry {
    pub(crate) cards:Vec<Rect>,
    pub(crate) divider:Option<Rect>,
    pub(crate) advanced_header:Option<Rect>,
}

#[derive(Clone,Debug,Default)]
pub(crate) struct ModalEditorGeometry {
    pub(crate) header:Option<Rect>,
    pub(crate) header_toolbar:Option<Rect>,
    pub(crate) body:Option<Rect>,
    pub(crate) viewport:Option<Rect>,
    pub(crate) scroll_offset_y:f32,
    pub(crate) footer:Option<Rect>,
    pub(crate) content:ModalEditorBodyGeometry,
}
impl ModalEditorGeometry {
    /// Actual boundary distances measured from allocated screen rects.
    /// The advanced divider contains the intentional 12pt on both sides.
    pub(crate) fn gaps(&self)->Vec<(&'static str,f32)>{
        let mut out=Vec::new();
        if let (Some(header),Some(card))=(self.header,self.content.cards.first()){
            out.push(("header_to_card",card.top()-header.bottom()));
        }
        let cards=&self.content.cards;
        if cards.len()>=2 && (cards[0].top()-cards[1].top()).abs()<1.0{
            out.push(("card_gap_1",cards[1].left()-cards[0].right()));
            if cards.len()>=3 && (cards[1].top()-cards[2].top()).abs()<1.0{
                out.push(("card_gap_2",cards[2].left()-cards[1].right()));
            }
        }
        if let Some(rule)=self.content.divider{
            if !self.content.cards.is_empty(){
                let bottom=self.content.cards.iter()
                    .map(Rect::bottom).fold(f32::NEG_INFINITY,f32::max);
                out.push(("card_to_rule",rule.top()
                    +theme::MODAL_MASTER_WORKSPACE_GAP-bottom));
            }
            if let Some(advanced)=self.content.advanced_header{
                out.push(("rule_to_advanced",advanced.top()
                    -(rule.top()+theme::MODAL_MASTER_WORKSPACE_GAP
                    +theme::SOFT_DIVIDER_WIDTH)));
            }
        }
        if let (Some(toolbar),Some(body))=(self.header_toolbar,self.body){
            out.push(("toolbar_to_body",body.top()-toolbar.bottom()));
        }
        if let (Some(body),Some(footer))=(self.body,self.footer){
            out.push(("body_to_footer",footer.top()-body.bottom()));
        }
        out
    }
    pub(crate) fn invalid_gaps(&self,one_pixel_tolerance:f32)->Vec<(&'static str,f32)>{
        self.gaps().into_iter()
            .filter(|(_,actual)|{
                (*actual-theme::MODAL_MASTER_WORKSPACE_GAP).abs()
                    >one_pixel_tolerance.max(0.5)
            }).collect()
    }
}

/// One owner for the popup's title, bounded scrolling workspace, and
/// fixed action footer. Individual editors supply only their contents.
/// All structural gaps are allocated ONCE at the owning level.
pub(crate) struct ModalEditorWorkspace<'a> {
    ui:&'a mut Ui,
    geometry:ModalEditorGeometry,
}
impl ModalEditorWorkspace<'_> {
    pub(crate) fn header(&mut self,title:&str,close_label:&str,enabled:bool)->bool{
        let (header,_)=self.ui.allocate_exact_size(
            Vec2::new(self.ui.available_width(),theme::MODAL_HEADER_HEIGHT),
            Sense::hover()
        );
        self.geometry.header=Some(header);
        modal_master_header_in_rect(self.ui,header,title,"",close_label,enabled)
    }

    /// Header variant for workspaces with a persistent status/action toolbar.
    /// The toolbar sits below the title divider; the body starts one
    /// master gap below the toolbar.
    pub(crate) fn header_with_toolbar<R>(
        &mut self,title:&str,close_label:&str,enabled:bool,
        toolbar:impl FnOnce(&mut Ui)->R,
    )->(bool,R){
        let header_height=modal_master_header_with_toolbar_height();
        let (header,_)=self.ui.allocate_exact_size(
            Vec2::new(self.ui.available_width(),header_height),
            Sense::hover()
        );
        self.geometry.header=Some(header);
        let toolbar_rect=modal_master_header_toolbar_rect(
            header,self.ui.ctx().pixels_per_point().max(1.0)
        );
        self.geometry.header_toolbar=Some(toolbar_rect);
        modal_master_header_with_toolbar_in_rect(
            self.ui,header,title,close_label,enabled,toolbar
        )
    }

    pub(crate) fn disable(&mut self){self.ui.disable();}

    fn before_body(&mut self){
        let top=self.ui.cursor().top();
        let target=self.geometry.header.map(|r|r.bottom()).unwrap_or(top)
            +theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
    }

    /// Fixed dashboards and search/list workspaces keep their own inner scroll
    /// regions. Only the master owns the outer inset and structural gaps.
    pub(crate) fn body_region<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        self.before_body();
        let body=modal_plain_body_region(self.ui,contents);
        self.geometry.body=Some(body.response.rect);
        body.inner
    }

    /// A viewer that already has the master framed body uses its own
    /// 12pt inset exactly once. Do not wrap it in a second body_region.
    pub(crate) fn framed_region<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        self.before_body();
        let region=self.ui.scope(|host|modal_master_body(host,contents));
        self.geometry.body=Some(region.response.rect);
        region.inner
    }

    /// Ordinary single-pane forms use one bounded body scrollbar.
    pub(crate) fn scroll_region<R>(&mut self,id:&str,max_height:f32,contents:impl FnOnce(&mut Ui)->R)->R{
        self.body_region(|host|{
            scroll_area(ScrollAxes::Vertical).id_salt(id)
                .max_height(max_height.max(1.0))
                .auto_shrink([false,true])
                .show(host,contents).inner
        })
    }

    fn before_footer(&mut self){
        let top=self.ui.cursor().top();
        let target=self.geometry.body.map(|r|r.bottom()).unwrap_or(top)
            +theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
    }

    pub(crate) fn footer_single<R>(&mut self,actions:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
        self.before_footer();
        let (response,rect)=modal_action_row_measured(self.ui,actions);
        self.geometry.footer=Some(rect);
        response
    }

    pub(crate) fn body<R>(
        &mut self,max_height:f32,
        contents:impl FnOnce(&mut Ui)->(R,ModalEditorBodyGeometry),
    )->R{
        // Use the *allocated header rect*, not an assumed cursor position.
        // egui can leave the flow cursor inside the reserved header widget
        // area (11pt in the previous Windows Rect audit). Adding 16pt to
        // that cursor produced only 5pt between header and first card.
        let target_top=self.geometry.header
            .map(|rect|rect.bottom()+theme::MODAL_MASTER_WORKSPACE_GAP)
            .unwrap_or(self.ui.cursor().top()+theme::MODAL_MASTER_WORKSPACE_GAP);
        let cursor_top=self.ui.cursor().top();
        self.ui.add_space((target_top-cursor_top).max(0.0));
        let result=self.ui.scope(|host|{
            let scroll=scroll_area(ScrollAxes::Vertical)
                .id_salt("modal-editor-workspace-body")
                .max_height(max_height)
                .auto_shrink([false,true])
                .show(host,|scroller|{
                    Frame::NONE.inner_margin(egui::Margin::symmetric(
                        theme::MODAL_MASTER_CONTENT_INSET as i8,0
                    )).show(scroller,|body|{
                        modal_master_controlled_spacing(body);
                        contents(body)
                    }).inner
                });
            (scroll.inner,scroll.inner_rect,scroll.state.offset.y)
        });
        self.geometry.body=Some(result.response.rect);
        let ((value,measured),viewport,scroll_offset)=result.inner;
        self.geometry.viewport=Some(viewport);
        self.geometry.scroll_offset_y=scroll_offset;
        self.geometry.content=measured;
        value
    }

    pub(crate) fn footer(
        &mut self,
        leading:impl FnOnce(&mut Ui),
        trailing:impl FnOnce(&mut Ui),
    ){
        self.before_footer();
        self.geometry.footer=Some(modal_action_row_split_measured(self.ui,leading,trailing));
    }
}

/// Same master owner for sequential forms, viewers and existing dashboard
/// bodies. No additional scrollbar is imposed on a split/list workspace.
pub(crate) fn modal_flow_workspace<R>(ui:&mut Ui,contents:impl FnOnce(&mut ModalEditorWorkspace<'_>)->R)->R{
    modal_flow_workspace_measured(ui,contents).0
}

pub(crate) fn modal_flow_workspace_measured<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut ModalEditorWorkspace<'_>)->R,
)->(R,ModalEditorGeometry){
    ui.scope(|layout|{
        modal_master_controlled_spacing(layout);
        let mut owner=ModalEditorWorkspace{ui:layout,geometry:ModalEditorGeometry::default()};
        let value=contents(&mut owner);
        (value,owner.geometry)
    }).inner
}

pub(crate) fn modal_workspace_footer_reserve()->f32{
    theme::MODAL_FOOTER_HEIGHT+theme::MODAL_MASTER_WORKSPACE_GAP
}

pub(crate) fn modal_editor_workspace<R>(
    ui:&mut Ui,title:&str,close_label:&str,enabled:bool,
    contents:impl FnOnce(&mut ModalEditorWorkspace<'_>)->R,
)->(bool,R,ModalEditorGeometry){
    modal_master_controlled_spacing(ui);
    let (header,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_HEADER_HEIGHT),
        Sense::hover()
    );
    let close=modal_master_header_in_rect(
        ui,header,title,"",close_label,enabled
    );
    let mut owner=ModalEditorWorkspace{
        ui,geometry:ModalEditorGeometry{
            header:Some(header),..Default::default()
        },
    };
    let value=contents(&mut owner);
    (close,value,owner.geometry)
}

/// Approved OCR modal geometry, now the default structural baseline for
/// future egui dialogs. Text length and local scrollbars never move these
/// three regions. Callers choose the appropriate popup size.
#[derive(Clone,Copy,Debug)]
pub(crate) struct ModalMasterLayout {
    pub(crate) header:Rect,
    pub(crate) workspace:Rect,
    pub(crate) footer:Rect,
}

pub(crate) fn modal_master_layout(
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
pub(crate) struct ModalMasterWorkspace {
    pub(crate) toolbar_bottom:f32,
    pub(crate) divider_x:Option<f32>,
}

/// Align the master edges to physical pixels before painting the header,
/// rounded workspace, or footer. Layout still owns the original dimensions;
/// every edge moves by at most half a device pixel. All strokes and pane fills
/// must use the SAME aligned rect so anti-aliasing cannot expose a seam.
pub(crate) fn modal_master_pixel_aligned_rect(rect:Rect,pixels_per_point:f32)->Rect {
    let scale=pixels_per_point.max(1.0);
    let snap=|v:f32|(v*scale).round()/scale;
    Rect::from_min_max(
        egui::pos2(snap(rect.left()),snap(rect.top())),
        egui::pos2(snap(rect.right()),snap(rect.bottom()))
    )
}

/// One horizontal content grid for the modal title, toolbar, body, rules and footer.
/// The outer frame owns its own 12pt inset; this second inset is shared by every
/// visible child so the popup never has competing left/right alignment columns.
fn modal_master_content_rect(rect:Rect)->Rect {
    let inset=theme::MODAL_MASTER_CONTENT_INSET.min(rect.width().max(0.0)*0.5);
    Rect::from_min_max(
        egui::pos2(rect.left()+inset,rect.top()),
        egui::pos2(rect.right()-inset,rect.bottom())
    )
}

/// Pixel-aligned rule using the same left/right edges as modal content.
fn modal_master_rule_rect(rect:Rect,ppp:f32,at_bottom:bool)->Rect {
    let scale=ppp.max(1.0);
    let height=(theme::SOFT_DIVIDER_WIDTH*scale).round().max(1.0)/scale;
    let content=modal_master_content_rect(rect);
    let top=if at_bottom {rect.bottom()-height}else{rect.top()};
    Rect::from_min_max(
        egui::pos2(content.left(),top),
        egui::pos2(content.right(),(top+height).min(rect.bottom()))
    )
}

/// Balance footer actions against the outer frame padding and top rule
/// while keeping the same horizontal content edges.
fn modal_master_action_area(rect:Rect,action_height:f32)->Rect {
    let content=modal_master_content_rect(rect);
    let height=action_height.max(0.0).min(rect.height().max(0.0));
    let free=(rect.height()-height).max(0.0);
    let outer=theme::MODAL_PANEL_PADDING_Y+theme::FRAME_STROKE_WIDTH;
    let top=rect.top()+((free+outer+theme::SOFT_DIVIDER_WIDTH)*0.5).min(free);
    Rect::from_min_size(
        egui::pos2(content.left(),top),
        Vec2::new(content.width(),height)
    )
}

/// Paint only the single shared work-surface background. OCR paints its
/// two content panes after this step, then uses `finish` for crisp borders.
/// This order prevents rectangular pane fills from erasing rounded corners.
pub(crate) fn modal_master_workspace_background(
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
pub(crate) fn modal_master_workspace_finish(
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

/// Draw the shared title/close controls inside an already allocated title row.
/// Background and divider ownership stays with the enclosing header component.
fn modal_master_header_content_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool{
    let has_description=!description.trim().is_empty();
    let heading_top=if has_description {
        rect.top()+theme::SPACE_8
    }else{
        rect.top()+((rect.height()-theme::ACTION_STANDARD_HEIGHT)/2.0
            -theme::MODAL_EDGE_ACTION_SHIFT).max(0.0)
    };
    let content=modal_master_content_rect(rect);
    let heading=Rect::from_min_size(
        egui::pos2(content.left(),heading_top),
        Vec2::new(content.width().max(1.0),theme::ACTION_STANDARD_HEIGHT)
    );
    let subtitle=Rect::from_min_size(
        egui::pos2(content.left(),heading.bottom()+theme::SPACE_4),
        Vec2::new(content.width(),20.0)
    );
    let heading_clip=heading.intersect(ui.clip_rect());
    // Reserve the real close-button hitbox first; a long heading may
    // truncate, but must never push the only exit outside the modal.
    let mut close_ui=ui.new_child(
        egui::UiBuilder::new().id_salt("modal-master-heading-close")
            .max_rect(heading)
            .layout(Layout::right_to_left(Align::Center))
    );
    close_ui.set_clip_rect(heading_clip);
    let response=crate::button::action(&mut close_ui,close_label,theme::ButtonRole::Standard,enabled);
    let close=response.clicked();
    let title_right=(response.rect.left()-theme::SPACE_8).max(heading.left());
    if title_right>heading.left(){
        let title_rect=Rect::from_min_max(heading.min,egui::pos2(title_right,heading.bottom()));
        let mut title_ui=ui.new_child(
            egui::UiBuilder::new().id_salt("modal-master-heading-title")
                .max_rect(title_rect)
                .layout(Layout::left_to_right(Align::Center))
        );
        title_ui.set_clip_rect(title_rect.intersect(heading_clip));
        title_ui.add(truncated_label(typography::modal_title(title)))
            ;
    }
    if has_description{
        let subtitle_clip=subtitle.intersect(ui.clip_rect());
        ui.scope_builder(
            egui::UiBuilder::new().id_salt("modal-master-description")
                .max_rect(subtitle),
            |details|{
                details.set_clip_rect(subtitle_clip);
                details.set_width(subtitle.width());
                details.add(truncated_label(typography::caption(description)))
                    ;
            },
        );
    }
    close
}

/// Canonical OCR-approved popup header, used for descriptive dialogs:
/// title and top-right Cancel/Close in the first row, optional explanation
/// in the second, ONE bottom divider. Height belongs to modal_master_layout.
/// Use an empty description for dialogs with no explanatory subtitle.
pub(crate) fn modal_master_header_in_rect(
    ui:&mut Ui,rect:Rect,title:&str,description:&str,
    close_label:&str,enabled:bool
)->bool {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    // A centered line at rect.bottom() is half clipped and can appear
    // darker at one end. Fill exactly one pixel-aligned INNER rule instead.
    ui.painter().rect_filled(
        modal_master_rule_rect(rect,ppp,true),0.0,theme::DIVIDER
    );
    modal_master_header_content_in_rect(
        ui,rect,title,description,close_label,enabled
    )
}

/// Shared height budget for the title, divider and below-rule toolbar.
pub(crate) fn modal_master_header_with_toolbar_height()->f32{
    theme::MODAL_HEADER_HEIGHT
        +theme::SOFT_DIVIDER_WIDTH
        +theme::MODAL_MASTER_TOOLBAR_RULE_GAP
        +theme::MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT
}

/// The single divider occupies the slot directly after the title row.
fn modal_master_toolbar_rule_rect(rect:Rect,ppp:f32)->Rect{
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    let top=(rect.top()+theme::MODAL_HEADER_HEIGHT).min(rect.bottom());
    let slot=Rect::from_min_max(
        egui::pos2(rect.left(),top),
        egui::pos2(rect.right(),(top+theme::SOFT_DIVIDER_WIDTH).min(rect.bottom()))
    );
    modal_master_rule_rect(slot,ppp,false)
}

/// Toolbar uses the same horizontal inset as the body, below the rule.
fn modal_master_header_toolbar_rect(rect:Rect,ppp:f32)->Rect{
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    let content=modal_master_content_rect(rect);
    let top=(rect.top()+theme::MODAL_HEADER_HEIGHT
        +theme::SOFT_DIVIDER_WIDTH+theme::MODAL_MASTER_TOOLBAR_RULE_GAP)
        .min(rect.bottom());
    Rect::from_min_max(
        egui::pos2(content.left(),top),
        egui::pos2(content.right(),rect.bottom())
    )
}

/// Common header: title/close, single divider, then the status/action row.
pub(crate) fn modal_master_header_with_toolbar_in_rect<R>(
    ui:&mut Ui,rect:Rect,title:&str,close_label:&str,enabled:bool,
    toolbar:impl FnOnce(&mut Ui)->R,
)->(bool,R){
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    ui.painter().rect_filled(
        modal_master_toolbar_rule_rect(rect,ppp),0.0,theme::DIVIDER
    );
    let title_rect=Rect::from_min_size(
        rect.min,Vec2::new(rect.width(),theme::MODAL_HEADER_HEIGHT)
    );
    let close=modal_master_header_content_in_rect(
        ui,title_rect,title,"",close_label,enabled
    );
    let toolbar_rect=modal_master_header_toolbar_rect(rect,ppp);
    let mut toolbar_ui=ui.new_child(
        egui::UiBuilder::new().id_salt("modal-master-header-toolbar")
            .max_rect(toolbar_rect)
            .layout(Layout::left_to_right(Align::Center))
    );
    toolbar_ui.set_clip_rect(toolbar_rect.intersect(ui.clip_rect()));
    let result=toolbar(&mut toolbar_ui);
    (close,result)
}


/// A single physical-pixel aligned footer surface for both fixed OCR
/// dialogs and flow-based confirmations. Owning the rule in one place
/// prevents half-clipped line segments or doubled separator strokes.
fn paint_modal_master_footer_surface(ui:&Ui,rect:Rect) {
    let ppp=ui.ctx().pixels_per_point().max(1.0);
    let rect=modal_master_pixel_aligned_rect(rect,ppp);
    ui.painter().rect_filled(rect,0.0,theme::PANEL);
    ui.painter().rect_filled(
        modal_master_rule_rect(rect,ppp,false),0.0,theme::DIVIDER
    );
}

/// Canonical quiet modal footer: one upper divider and the final action
/// anchored at the right, never an additional bottom Cancel button.
pub(crate) fn modal_master_footer_in_rect(
    ui:&mut Ui,rect:Rect,actions:impl FnOnce(&mut Ui)
) {
    paint_modal_master_footer_surface(ui,rect);
    // Primary actions are 36pt, but standard actions are 34pt. The
    // shared footer must reserve the taller control's ACTUAL hit/paint
    // height; a 34pt clip sliced 2pt off the bottom of "적용".
    let action_height=theme::ACTION_PRIMARY_HEIGHT
        .max(theme::ACTION_STANDARD_HEIGHT);
    let content=modal_master_action_area(rect,action_height);
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


/// Flow-layout sibling of modal_master_footer_in_rect for compact
/// confirmations. Shares the OCR master footer appearance and 36pt hitbox
/// while retaining egui's InnerResponse<R>, so existing confirmation state
/// machines (including .inner results) remain unchanged.
pub(crate) fn modal_action_row<R>(
    ui:&mut Ui,actions:impl FnOnce(&mut Ui)->R
)->egui::InnerResponse<R> {
    modal_action_row_measured(ui,actions).0
}

fn modal_action_row_measured<R>(
    ui:&mut Ui,actions:impl FnOnce(&mut Ui)->R
)->(egui::InnerResponse<R>,Rect) {
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),theme::MODAL_FOOTER_HEIGHT),
        Sense::hover()
    );
    paint_modal_master_footer_surface(ui,rect);
    let action_height=theme::ACTION_PRIMARY_HEIGHT
        .max(theme::ACTION_STANDARD_HEIGHT);
    let area=modal_master_action_area(rect,action_height);
    let clip=area.intersect(ui.clip_rect());
    let response=ui.scope_builder(
        egui::UiBuilder::new().id_salt("modal-master-flow-actions")
            .max_rect(area)
            .layout(Layout::right_to_left(Align::Center)),
        |buttons|{
            buttons.set_clip_rect(clip);
            buttons.set_width(area.width());
            actions(buttons)
        }
    );
    (response,rect)
}

/// Maximum scrollable editor body height after reserving the OCR-master
/// header/footer and the native modal's fixed outer margins. Screen modules
/// must not independently subtract arbitrary offsets from the full work area:
/// doing so can push the final 58pt action row outside a short window.
pub(crate) fn modal_editor_body_max_height(work_area_height:f32)->f32 {
    modal_body_height_budget(work_area_height,true)
}

/// The same structural budget for every flowing modal. Viewers without
/// a footer do not reserve a phantom action row.
pub(crate) fn modal_body_height_budget(work_area_height:f32,has_footer:bool)->f32 {
    let fixed=2.0*theme::MODAL_PANEL_PADDING_Y
        +2.0*theme::FRAME_STROKE_WIDTH
        +theme::MODAL_HEADER_HEIGHT
        +theme::MODAL_MASTER_WORKSPACE_GAP
        +if has_footer {modal_workspace_footer_reserve()} else {0.0};
    (work_area_height-fixed).max(1.0)
        .min(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
}

/// Bound a list after its search toolbar and filters have actually been
/// laid out. The parent owns the outer header and optional footer budget;
/// the caller can choose a preferred list height, never exceed the budget.
pub(crate) fn modal_list_max_height(
    body:&Ui,work_area_height:f32,preferred:f32,has_footer:bool
)->f32{
    let consumed=(body.cursor().top()-body.min_rect().top()).max(0.0);
    (modal_body_height_budget(work_area_height,has_footer)-consumed)
        .max(1.0).min(preferred.max(1.0))
}

fn modal_action_row_split_measured(
    ui:&mut Ui,
    leading:impl FnOnce(&mut Ui),
    trailing:impl FnOnce(&mut Ui),
)->Rect {
    modal_action_row_measured(ui,|footer|{
        footer.columns(2,|columns|{
            columns[0].with_layout(
                Layout::left_to_right(Align::Center),|left|leading(left)
            );
            columns[1].with_layout(
                Layout::right_to_left(Align::Center),|right|trailing(right)
            );
        });
    }).1
}

/// Single plain-body inset for both decision and editor workspaces.
/// Keep the actual content edges equal to the title/footer content grid.
fn modal_plain_body_region<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut Ui)->R
)->egui::InnerResponse<R>{
    let inset=theme::MODAL_MASTER_CONTENT_INSET;
    let width=(ui.available_width()-2.0*inset).max(1.0);
    Frame::NONE.inner_margin(egui::Margin::symmetric(inset as i8,0))
        .show(ui,|content|{
            content.set_width(width);
            content.spacing_mut().item_spacing.y=theme::SPACING;
            contents(content)
        })
}

/// One framed content work surface for flow-based popup bodies. Use one
/// per dialog rather than wrapping every subsection in another card.
pub(crate) fn modal_master_body<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut Ui)->R
)->R {
    let inset=theme::MODAL_SECTION_PADDING;
    let width=(ui.available_width()-2.0*inset
        -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
    theme::bordered_surface(theme::PANEL,theme::DIVIDER,theme::CONTROL_RADIUS)
        .inner_margin(inset)
        .show(ui,|body|{
            body.set_min_width(width);
            contents(body)
        }).inner
}

/// Shared modal-section heading geometry. Both margins around its
/// 1pt rule are explicit 16pt, independent of egui's default item spacing.
/// Its content gets a separate scope to prevent an implicit extra 8pt gap.
fn modal_section_heading_with_rule(ui:&mut Ui,title:&str){
    ui.spacing_mut().item_spacing.y=0.0;
    ui.label(typography::section_heading(title));
    section_divider(ui);
}

/// A heading and one divider within the modal master work surface.
/// Uses the same heading-rule-content geometry as the boxed modal section.
pub(crate) fn modal_master_body_section<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->R {
    ui.scope(|section|{
        modal_section_heading_with_rule(section,title);
        section.scope(|body|{
            body.spacing_mut().item_spacing.y=theme::SPACING;
            contents(body)
        }).inner
    }).inner
}

/// Exact outer card rectangle from the same canonical section renderer.
/// Do not estimate it from the inner body or content text size.
pub(crate) fn modal_section_measured<R>(
    ui:&mut Ui,title:&str,contents:impl FnOnce(&mut Ui)->R
)->egui::InnerResponse<R>{
    let inner_width=(ui.available_width()-2.0*theme::MODAL_SECTION_PADDING
        -2.0*theme::FRAME_STROKE_WIDTH).max(1.0);
    theme::bordered_surface(theme::MODAL_SECTION_BG,theme::DIVIDER,theme::MODAL_SECTION_RADIUS)
        .inner_margin(theme::MODAL_SECTION_PADDING)
        .show(ui,|section|{
            section.set_min_width(inner_width);
            modal_section_heading_with_rule(section,title);
            section.scope(|body|{
                body.spacing_mut().item_spacing.y=theme::SPACING;
                contents(body)
            }).inner
        })
}

/// Structural spacing for an explicitly managed modal workspace.
///
/// Eguis default item_spacing is additive to manual add_space() calls.
/// A controlled modal sets vertical spacing to zero, and structural gaps
/// are then allocated exactly once by the master components below.
/// Keep normal 8pt spacing inside the shared section field content.
pub(crate) fn modal_master_controlled_spacing(ui:&mut Ui){
    ui.spacing_mut().item_spacing.y=0.0;
}

/// A nested decision remains an interactive Foreground window.
/// Tooltip order is reserved for transient overlays and is not a reliable
/// interaction surface for modal actions in egui 0.33.
pub(crate) fn nested_modal_window(
    ctx:&egui::Context,title:&str,id:&'static str,work_area:egui::Rect
)->egui::Window<'static>{
    modal_window(ctx,title,id,work_area)
}

/// Shared decision layout. The owner allocates header, body and action
/// regions in sequence, so form edits are available to the footer immediately.
/// The caller supplies contents and actions, not outer padding or separators.
pub(crate) struct DecisionWorkspace<'a>{
    ui:&'a mut Ui,
    header_bottom:Option<f32>,
    body_bottom:Option<f32>,
}

impl DecisionWorkspace<'_>{
    pub(crate) fn header(&mut self,title:&str,cancel_label:&str,enabled:bool)->bool{
        let (rect,_)=self.ui.allocate_exact_size(
            Vec2::new(self.ui.available_width(),theme::MODAL_HEADER_HEIGHT),
            Sense::hover()
        );
        self.header_bottom=Some(rect.bottom());
        modal_master_header_in_rect(self.ui,rect,title,"",cancel_label,enabled)
    }

    pub(crate) fn body<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        let top=self.ui.cursor().top();
        let target=self.header_bottom.unwrap_or(top)+theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
        let body=modal_plain_body_region(self.ui,contents);
        self.body_bottom=Some(body.response.rect.bottom());
        body.inner
    }

    /// A framed decision owns one surface, instead of adding a framed
    /// master inside the already inset plain decision body.
    pub(crate) fn framed_body<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->R{
        let top=self.ui.cursor().top();
        let target=self.header_bottom.unwrap_or(top)+theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
        let region=self.ui.scope(|host|modal_master_body(host,contents));
        self.body_bottom=Some(region.response.rect.bottom());
        region.inner
    }

    fn before_actions(&mut self){
        let top=self.ui.cursor().top();
        let target=self.body_bottom.unwrap_or(top)+theme::MODAL_MASTER_WORKSPACE_GAP;
        self.ui.add_space((target-top).max(0.0));
    }

    pub(crate) fn actions<R>(&mut self,contents:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
        self.before_actions();
        modal_action_row(self.ui,contents)
    }
}

pub(crate) fn decision_workspace<R>(
    ui:&mut Ui,contents:impl FnOnce(&mut DecisionWorkspace<'_>)->R
)->R{
    ui.scope(|layout|{
        modal_master_controlled_spacing(layout);
        contents(&mut DecisionWorkspace{
            ui:layout,header_bottom:None,body_bottom:None,
        })
    }).inner
}

/// Convenience entry point for simple decisions. Uses the same workspace
/// as interactive decision forms; never assembles a second layout.
pub(crate) fn decision_content<R>(
    ui:&mut Ui,title:&str,cancel_label:&str,enabled:bool,
    body:impl FnOnce(&mut Ui),actions:impl FnOnce(&mut Ui)->R
)->(bool,R){
    decision_workspace(ui,|decision|{
        let cancelled=decision.header(title,cancel_label,enabled);
        decision.body(body);
        (cancelled,decision.actions(actions).inner)
    })
}

fn decision_window(
    ctx:&egui::Context,title:&str,id:&'static str,work_area:egui::Rect
) -> egui::Window<'static> {
    nested_modal_window(ctx,title,id,work_area)
        // Decisions are short, blocking actions: show their controls
        // immediately rather than fading an input surface in over the parent.
        .fade_in(false)
        // Confirmation dialogs must size to their content. Enabling an outer
        // ScrollArea here makes egui greedily occupy the available height,
        // leaving a large blank panel below the final action button.
        // Large editors keep their own explicitly bounded body scroll areas.
        .resizable(false)
        .default_width(theme::MODAL_DECISION_MAX_WIDTH.min(work_area.width()))
}

/// Show a decision and establish the real window as the input boundary
/// only after egui has created its layer. This applies to nested and root
/// decisions alike, avoiding a full-screen dim swallowing button input.
pub(crate) fn show_decision_window<R>(
    ctx:&egui::Context,title:&str,id:&'static str,work_area:Rect,
    contents:impl FnOnce(&mut Ui)->R,
){
    if decision_window(ctx,title,id,work_area).show(ctx,contents).is_some(){
        modal_foreground_for_window(ctx,id);
    }
}

/// Draw the dim over the ENTIRE application viewport, including its header
/// and KakaoTalk reserved pane. Only the actual dialog remains centered in
/// the Kakao-safe work area; do not use that area to clip the backdrop.
/// Render before its dialog; nested confirmations draw one more dim layer.
pub(crate) fn modal_backdrop(
    ctx:&egui::Context,id:&'static str,nested:bool
) {
    let viewport=ctx.content_rect();
    let alpha=if nested {
        theme::MODAL_NESTED_BACKDROP_ALPHA
    } else {
        theme::MODAL_BACKDROP_ALPHA
    };
    let area=crate::overlay::area(egui::Id::new(id),crate::overlay::AreaRole::Backdrop{nested})
        .fixed_pos(viewport.min)
        .movable(false)
        // The backdrop cannot rise above the dialog after an outside click.
        .interactable(false)
        .sense(egui::Sense::hover());
    // The modal layer prevents background input; visual coverage is full-screen.
    let layer=area.layer();
    crate::overlay::activate_modal(ctx,layer,false);
    area.show(ctx,|ui|{
        let (rect,_)=ui.allocate_exact_size(
            viewport.size(),egui::Sense::hover()
        );
        ui.painter().rect_filled(
            rect,0.0,egui::Color32::from_black_alpha(alpha)
        );
    });
}

/// Layer occupied by the interactive nested decision window.
/// It intentionally shares Foreground order with its parent and backdrop;
/// explicit stacking below keeps the dialog above the full-screen dim Area.
pub(crate) fn modal_window_layer(id:&'static str)->egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground,egui::Id::new(id))
}

/// A child decision is rendered AFTER its disabled parent worksheet. Raise
/// only the real decision Window layer, and do so after Window::show created
/// it. An extra full-screen Foreground Area can intercept every button when
/// egui resolves previous-frame hit testing; the outer Middle backdrop and
/// disabled parent already block background input.
pub(crate) fn modal_foreground_for_window(
    ctx:&egui::Context,window_id:&'static str
){
    let dialog_layer=modal_window_layer(window_id);
    crate::overlay::activate_modal(ctx,dialog_layer,true);
}

/// Approved inline modal form: label and editor occupy ONE 36pt row.
/// The editor is responsible for its own minimum width and disabled state.
/// Reuse this for product, client and other management dialogs instead of
/// screen-specific vertical label/input stacks.
pub(crate) fn modal_labeled_row<R>(
    ui:&mut Ui,label:&str,control:impl FnOnce(&mut Ui)->R
)->R {
    modal_labeled_row_with_width(ui,label,theme::MODAL_FORM_LABEL_WIDTH,control)
}

/// A wider label column for forms containing longer localized captions.
/// The same row geometry applies to all controls, including selectors.
pub(crate) fn modal_labeled_row_with_width<R>(
    ui:&mut Ui,label:&str,label_width:f32,control:impl FnOnce(&mut Ui)->R
)->R {
    ui.horizontal(|line|{
        line.spacing_mut().item_spacing.x=theme::SPACE_8;
        line.spacing_mut().interact_size.y=theme::INPUT_HEIGHT;
        form_field_label_sized(line,label,label_width);
        control(line)
    }).inner
}


// Shared controls and presentation profiles.
/// Removable search-tag chip. The modal nested-frame master owns its fill,
/// outline and radius; only the chip's approved compact vertical inset differs.
pub(crate) fn removable_tag_chip(ui:&mut Ui,tag:&str)->Response{
    let visible=if tag.chars().count()>22{
        format!("{}…",tag.chars().take(22).collect::<String>())
    }else{tag.to_owned()};
    modal_nested_role_frame(ModalNestedRole::CompactTag)
        .show(ui,|chip|{
            chip.horizontal(|line|{
                line.spacing_mut().item_spacing.x=theme::SPACE_4;
                let label=line.label(typography::body(&visible));
                crate::ui_primitives::shortened_text_tooltip(label,&visible,tag);
                crate::button::icon_action_button(
                    line,"×",true
                )
            }).inner
        }).inner
}

#[cfg(test)]
mod regression_tests;
