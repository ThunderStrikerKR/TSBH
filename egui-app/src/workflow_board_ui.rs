//! The right-hand workflow board. Layout/hover/selection only; no business rules.
//! Native Rust provides groups, amounts, freight, stage and shipment identity.
use eframe::egui::{self, Align, Frame, Id, Layout, Pos2, Rect, Ui, Vec2};
use std::time::{Duration, Instant};
use crate::{components, preview_store::WorkingReception, source_highlight, theme, typography};
use std::collections::HashMap;
use crate::workflow_board::{BoardSnapshot, GroupRow, ShipmentRow};

const HOVER_ENTER:Duration=Duration::from_millis(250);
const HOVER_EXIT:Duration=Duration::from_millis(250);
const PENDING_ORDER_HOVER_ENTER:Duration=Duration::from_millis(250);
const PENDING_ORDER_HOVER_EXIT:Duration=Duration::from_millis(250);
const STEP_NAMES:[&str;4]=["주문 확인","운임 적용","계좌 안내","입금 확인"];

/// The base card and its foreground hover preview are both painted within
/// the same frame. Their interactive widgets MUST have different IDs:
/// reusing one ID in Background and Foreground panics in egui 0.33.
fn workflow_widget_id(kind:&'static str,order_id:&str,in_preview:bool)->Id{
    Id::new((kind,order_id,in_preview))
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Selection {
    pub group_id:String,
    pub order_id:String,
    pub shipment_id:String,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct PendingTarget{
    pub order_id:String,
    pub recipient_id:Option<String>,
}
impl PendingTarget{
    fn order(id:&str)->Self{
        Self{order_id:id.into(),recipient_id:None}
    }
    fn recipient(id:&str,recipient:&str)->Self{
        Self{order_id:id.into(),recipient_id:(!recipient.is_empty()).then(||recipient.into())}
    }
}
#[derive(Default)]
pub struct BoardUi {
    pub selection:Option<Selection>,
    pub folded:Option<String>,
    preview_pending:Option<(String,Instant)>,
    preview_active:Option<String>,
    preview_leave:Option<Instant>,
    preview_rect:Option<Rect>,
    pub preview_shipment:Option<String>,
    row_pending:Option<(String,Instant)>,
    row_release:Option<Instant>,
    // Delayed hover navigation for an unconfirmed receipt, independent of
    // the blue workflow group/stage overlay and its selection state.
    pending_order_hover:Option<(PendingTarget,Instant)>,
    pending_visible:Option<PendingTarget>,
    pending_release:Option<Instant>,
    // Source, recipient and goods panes can temporarily retain a pending
    // card's read-only foreground preview without committing selection.
    pending_content_hovered:Option<String>,
}
pub enum Event {
    SelectPending(String),
    SelectPendingRecipient(PendingTarget),
    SelectShipment(Selection),
    ToggleGroup(String),
    TogglePending(String),
    ManageGroup(String),
    DeleteGroup(String),
    AdvanceGroup(String),
    UndoGroup(String),
    ManagePending(String),
    DeletePending(String),
}
impl BoardUi{
    pub fn stop_preview(&mut self){
        self.preview_pending=None;
        self.preview_active=None;
        self.preview_leave=None;
        self.preview_rect=None;
        self.preview_shipment=None;
        self.row_pending=None;
        self.row_release=None;
        self.pending_order_hover=None;
        self.pending_visible=None;
        self.pending_release=None;
        self.pending_content_hovered=None;
    }
    pub fn note_pending_content_hover(&mut self,order_id:&str,hovered:bool){
        if hovered && self.pending_visible.as_ref()
            .is_some_and(|visible|visible.order_id==order_id){
            self.pending_content_hovered=Some(order_id.to_owned());
        }
    }
    pub fn select(&mut self,selection:Selection){
        self.selection=Some(selection);
        self.folded=None;
        self.stop_preview();
    }
    pub fn active_preview(&self)->Option<&str>{
        self.preview_active.as_deref()
    }
    pub fn pending_preview(&self)->Option<&PendingTarget>{
        self.pending_visible.as_ref()
    }
    fn update_row_hover(&mut self,incoming:Option<String>,ctx:&egui::Context){
        let now=Instant::now();
        if let Some(id)=incoming{
            self.row_release=None;
            if self.preview_shipment.as_deref()==Some(id.as_str()){
                self.row_pending=None;
            }else if self.row_pending.as_ref().is_some_and(|(pending,at)|
                pending==&id && now.duration_since(*at)>=Duration::from_millis(100)){
                self.preview_shipment=Some(id);
                self.row_pending=None;
                ctx.request_repaint();
            }else if self.row_pending.as_ref().is_some_and(|(pending,_)|pending==&id){
                ctx.request_repaint_after(Duration::from_millis(100));
            }else{
                self.row_pending=Some((id,now));
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }else{
            self.row_pending=None;
            if self.preview_shipment.is_some(){
                let end=*self.row_release.get_or_insert(now+Duration::from_millis(150));
                if now>=end{self.preview_shipment=None;self.row_release=None;ctx.request_repaint();}
                else{ctx.request_repaint_after(Duration::from_millis(150));}
            }
        }
    }
    /// Hover only affects the visible projection. Committed selection
    /// and its unsaved edit ownership remain unchanged.
    fn update_pending_order_hover(
        &mut self,incoming:Option<PendingTarget>,committed:Option<&PendingTarget>,
        ctx:&egui::Context
    ){
        let now=Instant::now();
        let keep_content=self.pending_content_hovered.take()
            .is_some_and(|order|self.pending_visible.as_ref()
                .is_some_and(|visible|visible.order_id==order));
        if let Some(candidate)=incoming{
            self.pending_release=None;
            if Some(&candidate)==committed ||
                (candidate.recipient_id.is_none() &&
                    committed.is_some_and(|current|
                        current.order_id==candidate.order_id)){
                // The header of the selected order is not a new preview.
                self.pending_order_hover=None;
                if self.pending_visible.take().is_some(){ctx.request_repaint();}
                return;
            }
            if self.pending_visible.as_ref()==Some(&candidate){
                self.pending_order_hover=None;
                return;
            }
            match &self.pending_order_hover{
                Some((pending,at)) if pending==&candidate &&
                    now.duration_since(*at)>=PENDING_ORDER_HOVER_ENTER=>{
                    self.pending_visible=Some(candidate);
                    self.pending_order_hover=None;
                    ctx.request_repaint();
                }
                Some((pending,_)) if pending==&candidate=>{
                    ctx.request_repaint_after(PENDING_ORDER_HOVER_ENTER);
                }
                _=>{
                    self.pending_order_hover=Some((candidate,now));
                    ctx.request_repaint_after(PENDING_ORDER_HOVER_ENTER);
                }
            }
        }else{
            self.pending_order_hover=None;
            if keep_content {
                self.pending_release=None;
                return;
            }
            if self.pending_visible.is_some(){
                let until=*self.pending_release
                    .get_or_insert(now+PENDING_ORDER_HOVER_EXIT);
                if now>=until{
                    self.pending_visible=None;
                    self.pending_release=None;
                    ctx.request_repaint();
                }else{
                    ctx.request_repaint_after(PENDING_ORDER_HOVER_EXIT);
                }
            }
        }
    }

    /// Hovering a stage label previews its group after 250ms. The anchored
    /// overlay is part of the hover region and stays for 250ms on exit.
    pub fn update_hover(&mut self, incoming:Option<String>, ctx:&egui::Context){
        let now=Instant::now();
        let over_overlay=self.preview_rect
            .zip(ctx.input(|i|i.pointer.hover_pos()))
            .is_some_and(|(rect,pointer)|rect.contains(pointer));
        if let Some(group)=incoming.filter(|_|true)
            .or_else(||over_overlay.then(||self.preview_active.clone()).flatten()){
            self.preview_leave=None;
            if self.preview_active.as_deref()==Some(group.as_str()){
                self.preview_pending=None;
                return;
            }
            match &self.preview_pending {
                Some((pending,at)) if pending==&group &&
                    now.duration_since(*at)>=HOVER_ENTER=>{
                        self.preview_active=Some(group);
                        self.preview_pending=None;
                        self.preview_shipment=None;
                        ctx.request_repaint();
                }
                Some((pending,_)) if pending==&group=>{
                    ctx.request_repaint_after(HOVER_ENTER);
                }
                _=>{
                    self.preview_pending=Some((group,now));
                    ctx.request_repaint_after(HOVER_ENTER);
                }
            }
        }else{
            self.preview_pending=None;
            if self.preview_active.is_some(){
                let until=*self.preview_leave.get_or_insert(now+HOVER_EXIT);
                if now>=until {self.stop_preview();ctx.request_repaint();}
                else{ctx.request_repaint_after(HOVER_EXIT);}
            }
        }
    }
}
/// An order group is one full-width card with a 38pt identification band,
/// a 1pt divider, and a 43pt actions band. The two surfaces meet at the
/// outer border: there is no nested card or stray horizontal inset.
fn workflow_card<R>(
    ui:&mut Ui,selected:bool,preview:bool,body:impl FnOnce(&mut Ui)->R
)->R{
    let outer=ui.available_width();
    let frame=Frame::new()
        .fill(if selected || preview{theme::WORKFLOW_SELECTED_BG}
            else{theme::WORKFLOW_CARD_BG})
        .stroke(egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,
            theme::WORKFLOW_CARD_BORDER))
        .corner_radius(egui::CornerRadius::same(theme::WORKFLOW_CARD_RADIUS))
        .inner_margin(0);
    let response=frame.show(ui,|card|{
        // The 42pt upper/lower bands and 1pt divider own all vertical
        // spacing. No additional automatic gap between the compact rows.
        card.spacing_mut().item_spacing.y=0.0;
        card.set_min_width(
            (outer-theme::SOFT_DIVIDER_WIDTH*2.0).max(0.0)
        );
        body(card)
    });
    if selected || preview{
        let r=response.response.rect;
        ui.painter().rect_filled(
            Rect::from_min_max(
                Pos2::new(r.left(),r.top()+theme::SOFT_DIVIDER_WIDTH),
                Pos2::new(r.left()+theme::WORKFLOW_SELECTED_STRIPE,
                    r.bottom()-theme::SOFT_DIVIDER_WIDTH),
            ),
            0.0,if preview{theme::SELECTION}else{theme::WORKFLOW_SELECTED_BORDER}
        );
    }
    response.inner
}
/// Reserve the common 6pt left and right inset for BOTH compact rows.
/// Their horizontal inset matches the 6pt vertical control clearance.
fn workflow_row_content(rect:Rect)->Rect{
    rect.shrink2(Vec2::new(theme::WORKFLOW_CARD_PADDING_X,0.0))
}
fn workflow_cell(x:f32,width:f32,row:Rect)->Rect{
    Rect::from_min_size(Pos2::new(x,row.top()),Vec2::new(width.max(0.0),row.height()))
}
/// The signal group and ALL four buttons use the exact same vertical inset.
/// Top and lower bands are both 42pt; a 30pt control is inset 6pt per side.
/// Avoid screen-local y-offsets which previously made the rows unbalanced.
fn workflow_control_rect(cell:Rect)->Rect{
    let inner=cell.shrink2(Vec2::new(0.0,theme::WORKFLOW_CONTROL_VERTICAL_INSET));
    debug_assert!((inner.height()-theme::WORKFLOW_AUX_CONTROL_HEIGHT).abs()<0.01);
    debug_assert!((theme::WORKFLOW_ACTION_ROW_HEIGHT-inner.height()).abs()<0.01);
    debug_assert!((theme::WORKFLOW_SIGNAL_BLOCK_HEIGHT-inner.height()).abs()<0.01);
    inner
}
/// Manually measured Hangul ellipsis: truncation must not alter the position
/// of four signal lamps or the management controls beside the dealer name.
fn workflow_title_fit(ui:&Ui,title:&str,width:f32)->String{
    if width<=0.0{return String::new();}
    if typography::workflow_dealer_galley(ui,title).size().x<=width{
        return title.to_owned();
    }
    let mut prefix=title.to_owned();
    while !prefix.is_empty(){
        prefix.pop();
        let proposal=format!("{prefix}…");
        if typography::workflow_dealer_galley(ui,&proposal).size().x<=width{
            return proposal;
        }
    }
    if typography::workflow_dealer_galley(ui,"…").size().x<=width{
        "…".to_owned()
    }else{String::new()}
}
/// Draw one fixed-size action with its own collision-free pointer ID.
/// Preview controls carry their own IDs and dispatch the SAME authorized
/// operations as the base card; they never mutate state while painting.
fn workflow_auxiliary_at(
    ui:&mut Ui,id:&str,rect:Rect,
    action:components::WorkflowAuxiliaryAction,
    enabled:bool,in_preview:bool
)->egui::Response{
    let kind=match action{
        components::WorkflowAuxiliaryAction::Manage=>"manage",
        components::WorkflowAuxiliaryAction::Delete=>"delete",
        components::WorkflowAuxiliaryAction::PreviousStage=>"undo",
    };
    let effective=enabled;
    let response=ui.interact(
        rect,Id::new(("workflow-aux",id,kind,in_preview)),
        if effective{egui::Sense::click()}else{egui::Sense::hover()}
    ).on_hover_text(match action{
        components::WorkflowAuxiliaryAction::Manage=>"배송 건 묶음 관리",
        components::WorkflowAuxiliaryAction::Delete=>"주문 묶음 삭제",
        components::WorkflowAuxiliaryAction::PreviousStage=>"이전 단계 취소",
    });
    let painter=ui.painter();
    let (fill,stroke,ink)=if effective{
        (theme::RAISED,theme::BORDER,theme::TEXT)
    }else{
        (theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_BORDER,
         theme::DISABLED_ACTION_TEXT)
    };
    painter.rect_filled(rect,5.0,fill);
    painter.rect_stroke(
        rect,5.0,
        egui::Stroke::new(theme::FRAME_STROKE_WIDTH,stroke),
        egui::StrokeKind::Inside
    );
    match action{
        components::WorkflowAuxiliaryAction::Manage=>{
            painter.text(
                rect.center(),egui::Align2::CENTER_CENTER,"관리",
                egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
                ink
            );
        }
        components::WorkflowAuxiliaryAction::Delete=>{
            components::paint_workflow_icon(
                painter,rect.center(),components::WorkflowIcon::Trash,ink
            );
        }
        components::WorkflowAuxiliaryAction::PreviousStage=>{
            components::paint_workflow_icon(
                painter,rect.center(),components::WorkflowIcon::PreviousStage,ink
            );
        }
    };
    if effective{theme::paint_button_hover(ui,&response,5.0);}
    response
}
/// One allocated 85pt rectangle, not three independent egui allocations.
/// The divider position and both 42pt band bounds come from this same rect.
/// This makes 6pt margins above/below every 30pt control deterministic.
fn workflow_compact_band_rects(rect:Rect)->(Rect,Rect,Rect){
    let upper=Rect::from_min_size(
        rect.min,Vec2::new(rect.width(),theme::WORKFLOW_TITLE_HEIGHT)
    );
    let divider=Rect::from_min_size(
        Pos2::new(rect.left(),upper.bottom()),
        Vec2::new(rect.width(),theme::WORKFLOW_BAND_DIVIDER_HEIGHT)
    );
    let lower=Rect::from_min_size(
        Pos2::new(rect.left(),divider.bottom()),
        Vec2::new(rect.width(),theme::WORKFLOW_BOTTOM_HEIGHT)
    );
    debug_assert!((lower.bottom()-rect.bottom()).abs()<0.01);
    (upper,divider,lower)
}
fn workflow_compact_rows(ui:&mut Ui)->(Rect,Rect){
    let total=theme::WORKFLOW_TITLE_HEIGHT
        +theme::WORKFLOW_BAND_DIVIDER_HEIGHT
        +theme::WORKFLOW_BOTTOM_HEIGHT;
    let (card,_)=ui.allocate_exact_size(
        Vec2::new(ui.available_width(),total),egui::Sense::hover()
    );
    let (upper,divider,lower)=workflow_compact_band_rects(card);
    ui.painter().rect_filled(divider,0.0,theme::WORKFLOW_CARD_BORDER);
    ui.painter().rect_filled(lower,0.0,theme::WORKFLOW_CARD_LOWER_BG);
    (upper,lower)
}
/// All five upper controls occupy stable x positions at every card width.
/// The 1920×1080 reference card (394pt outer) gets a 164pt dealer slot and
/// a trailing 192pt signal/manage/delete group. The lower stage/undo group
/// is also 192pt, and every horizontal inter-control gap is precisely 6pt.
/// Long dealer names are clipped inside their OWN slot.
struct HeaderActions {
    title: egui::Response,
    manage: bool,
    delete: bool,
}
fn workflow_header(
    ui:&mut Ui,row:Rect,id:&str,title:&str,open:bool,step:usize,
    notice:bool,in_preview:bool,can_manage:bool,can_delete:bool
)->HeaderActions{
    let content=workflow_row_content(row);
    let gap=theme::WORKFLOW_CARD_SLOT_GAP;
    let toggle=theme::WORKFLOW_TOGGLE_ICON_WIDTH;
    let fixed=toggle+theme::WORKFLOW_SIGNAL_WIDTH
        +theme::WORKFLOW_MANAGE_WIDTH+theme::WORKFLOW_TRASH_WIDTH+4.0*gap;
    let title_width=(content.width()-fixed).max(0.0);
    let arrow=workflow_cell(content.left(),toggle,row);
    let title_rect=workflow_cell(arrow.right()+gap,title_width,row);
    let signal=workflow_cell(title_rect.right()+gap,theme::WORKFLOW_SIGNAL_WIDTH,row);
    let manage=workflow_cell(signal.right()+gap,theme::WORKFLOW_MANAGE_WIDTH,row);
    let delete=workflow_cell(manage.right()+gap,theme::WORKFLOW_TRASH_WIDTH,row);
    debug_assert!((content.right()-delete.right()).abs()<=0.01);
    debug_assert!((manage.left()-signal.right()-gap).abs()<=0.01);
    debug_assert!((delete.left()-manage.right()-gap).abs()<=0.01);

    components::paint_workflow_icon(
        ui.painter(),arrow.center(),
        if open{components::WorkflowIcon::Expanded}
        else{components::WorkflowIcon::Collapsed},theme::SECONDARY
    );
    let text_rect=Rect::from_min_max(
        Pos2::new(title_rect.left()+2.0,title_rect.top()),
        Pos2::new((title_rect.right()-4.0).max(title_rect.left()+2.0),
            title_rect.bottom()),
    );
    // The agreed dealer caption needs roughly eight Hangul characters.
    // Keep the whole reserved slot for geometry, but limit the readable
    // caption to ~8 characters so unusually long names do not crowd the
    // status group. Full text remains available on hover.
    let title_ink_width=text_rect.width()
        .min(typography::FONT_SUBSECTION_HEADING*9.0);
    let visible=workflow_title_fit(ui,title,title_ink_width);
    let galley=typography::workflow_dealer_galley(ui,&visible);
    ui.painter().with_clip_rect(text_rect).galley(
        Pos2::new(text_rect.left(),text_rect.center().y-galley.size().y/2.0),
        galley,theme::TEXT
    );
    let title_response=ui.interact(
        arrow.union(title_rect),
        workflow_widget_id("workflow-header",id,in_preview),egui::Sense::click()
    ).on_hover_text(title);

    let block=workflow_control_rect(signal);
    ui.painter().rect_filled(
        block,5.0,theme::WORKFLOW_SIGNAL_BLOCK_BG
    );
    ui.painter().rect_stroke(
        block,5.0,
        egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::WORKFLOW_SIGNAL_BLOCK_BORDER),
        egui::StrokeKind::Inside
    );
    let lamp_width=theme::WORKFLOW_SIGNAL_SIZE;
    let lamp_height=theme::WORKFLOW_SIGNAL_HEIGHT;
    let lamp_span=lamp_width*STEP_NAMES.len() as f32
        +theme::WORKFLOW_SIGNAL_GAP*(STEP_NAMES.len()-1) as f32;
    for index in 0..STEP_NAMES.len(){
        let center=Pos2::new(
            block.left()+(block.width()-lamp_span)/2.0+lamp_width/2.0
                +index as f32*(lamp_width+theme::WORKFLOW_SIGNAL_GAP),
            block.center().y
        );
        let lamp=Rect::from_center_size(center,Vec2::new(lamp_width,lamp_height));
        let (fill,border)=if index<step{
            (theme::WORKFLOW_SIGNAL_DONE,theme::WORKFLOW_SIGNAL_DONE_BORDER)
        }else if index==step || (index==step && notice){
            (theme::WORKFLOW_SIGNAL_CURRENT,theme::WORKFLOW_SIGNAL_CURRENT_BORDER)
        }else{
            (theme::WORKFLOW_SIGNAL_PENDING,theme::WORKFLOW_SIGNAL_PENDING_BORDER)
        };
        ui.painter().rect_filled(lamp,theme::WORKFLOW_SIGNAL_RADIUS,fill);
        ui.painter().rect_stroke(
            lamp,theme::WORKFLOW_SIGNAL_RADIUS,
            egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,border),
            egui::StrokeKind::Inside
        );
    }
    let manage_clicked=workflow_auxiliary_at(
        ui,id,workflow_control_rect(manage),
        components::WorkflowAuxiliaryAction::Manage,
        can_manage,in_preview
    ).clicked();
    let delete_clicked=workflow_auxiliary_at(
        ui,id,workflow_control_rect(delete),
        components::WorkflowAuxiliaryAction::Delete,
        can_delete,in_preview
    ).clicked();
    // The header is an actionable target even when the status/control
    // boxes beside it are inert. Hover blue; selected cards remain orange.
    theme::paint_button_hover(ui,&title_response,theme::CONTROL_RADIUS as f32);
    HeaderActions{
        title:title_response,manage:manage_clicked,delete:delete_clicked
    }
}
fn stage_name(step:usize,notice:bool)->&'static str{
    if notice && step>=2 {"계좌 재안내 완료"}
    else if step==4 {"처리완료"}
    else {["주문 확인 완료","운임 적용","계좌 안내 완료","입금 확인 완료"]
        [step.min(3)]}
}
fn money(amount:Option<i64>)->String{
    amount.map(components::display_won).unwrap_or_else(||"—".to_owned())
}
/// Compact one-touch button, 154×30pt at the 1080p reference, preserving
/// every stage label including "계좌 재안내 완료" without shrinking text.
/// This uses the shared Emphasized/disabled palette and 13pt action type,
/// but not egui's minimum text padding (which could enlarge the rectangle).
fn workflow_primary_at(
    ui:&mut Ui,id:&str,rect:Rect,text:&str,enabled:bool,in_preview:bool
)->egui::Response{
    let response=ui.interact(
        rect,workflow_widget_id("workflow-stage-button",id,in_preview),
        if enabled{egui::Sense::click()}else{egui::Sense::hover()}
    ).on_hover_text(text);
    let fill=if enabled{theme::BRIGHT}else{theme::DISABLED_ACTION_BG};
    let ink=if enabled{theme::BRIGHT_TEXT}else{theme::DISABLED_ACTION_TEXT};
    ui.painter().rect_filled(rect,5.0,fill);
    if !enabled{
        ui.painter().rect_stroke(
            rect,5.0,
            egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DISABLED_ACTION_BORDER),
            egui::StrokeKind::Inside
        );
    }
    let galley=ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),ink
    );
    ui.painter().with_clip_rect(rect.shrink2(Vec2::new(theme::SPACE_8,0.0)))
        .galley(Pos2::new(
            rect.center().x-galley.size().x/2.0,
            rect.center().y-galley.size().y/2.0
        ),galley,ink);
    // A completed workflow's stage can be disabled for click yet remains an
    // interactive hover-preview trigger; show the preview affordance.
    theme::paint_button_hover(ui,&response,5.0);
    response
}
/// The lower band is a true full-bleed #292929 surface. Shipping and money
/// share the bottom glyph edge, VAT appears above the amount only when used.
/// At the 394pt 1080p reference: shipping 44, price 132, one-touch 154,
/// undo 32, and gaps 6×3. Signal block left aligns to one-touch left;
/// delete and undo share a right edge. Currency text has 10pt clearance to
/// the one-touch button (6pt slot gap + 4pt inner ink inset).
fn workflow_action_line(
    ui:&mut Ui,lower:Rect,id:&str,shipments:usize,total:Option<i64>,
    vat:Option<&str>,button_text:Option<&str>,enabled:bool,
    undo_enabled:bool,in_preview:bool
)->(bool,bool,bool){
    let content=workflow_row_content(lower);
    let gap=theme::WORKFLOW_CARD_SLOT_GAP;
    let ship_width=theme::WORKFLOW_SHIPPING_WIDTH;
    let action_width=theme::WORKFLOW_CONTROL_WIDTH;
    let undo_width=theme::WORKFLOW_UNDO_WIDTH;
    let price_width=(content.width()-ship_width-action_width-undo_width-3.0*gap)
        .max(0.0);
    let shipping=workflow_cell(content.left(),ship_width,lower);
    let price=workflow_cell(shipping.right()+gap,price_width,lower);
    let action=workflow_cell(price.right()+gap,action_width,lower);
    let undo=workflow_cell(action.right()+gap,undo_width,lower);
    debug_assert!((content.right()-undo.right()).abs()<=0.01);
    debug_assert!((undo.left()-action.right()-gap).abs()<=0.01);
    // signal + manage + delete total width matches one-touch + undo.
    let upper_right_width=theme::WORKFLOW_SIGNAL_WIDTH
        +theme::WORKFLOW_MANAGE_WIDTH+theme::WORKFLOW_TRASH_WIDTH+2.0*gap;
    let lower_right_width=action_width+undo_width+gap;
    debug_assert!((upper_right_width-lower_right_width).abs()<=0.01);
    let bottom_y=lower.bottom()-7.0;

    let shipping_text=format!("배송 {shipments}건");
    let ship_galley=ui.painter().layout_no_wrap(
        shipping_text.clone(),
        egui::FontId::proportional(theme::WORKFLOW_VAT_FONT_SIZE),
        theme::SECONDARY
    );
    ui.painter().with_clip_rect(shipping).galley(
        Pos2::new(shipping.left(),bottom_y-ship_galley.size().y),
        ship_galley,theme::SECONDARY
    );
    ui.interact(shipping,workflow_widget_id("workflow-shipping",id,in_preview),
        egui::Sense::hover()).on_hover_text(shipping_text);

    let amount=total.map(components::display_won)
        .unwrap_or_else(||"금액 미확정".to_owned());
    let money_galley=ui.painter().layout_no_wrap(
        amount.clone(),
        typography::data_value_font(
            typography::DataValueKind::Money,
            typography::DataValueContext::CompactCard
        ),theme::TEXT
    );
    let price_ink=Rect::from_min_max(
        price.min,Pos2::new((price.right()-theme::SPACE_4).max(price.left()),
            price.bottom())
    );
    ui.painter().with_clip_rect(price_ink).galley(
        Pos2::new(price_ink.right()-money_galley.size().x,
            bottom_y-money_galley.size().y),
        money_galley,theme::TEXT
    );
    if let Some(vat)=vat{
        let visible=if vat.starts_with('('){vat.to_owned()}
            else{format!("({vat})")};
        let vat_galley=ui.painter().layout_no_wrap(
            visible,egui::FontId::proportional(theme::WORKFLOW_VAT_FONT_SIZE),
            theme::WORKFLOW_VAT_TEXT
        );
        ui.painter().with_clip_rect(price_ink).galley(
            Pos2::new(price_ink.right()-vat_galley.size().x,price.top()+3.0),
            vat_galley,theme::WORKFLOW_VAT_TEXT
        );
    }
    ui.interact(price,workflow_widget_id("workflow-money",id,in_preview),
        egui::Sense::hover()).on_hover_text(&amount);

    // Pending receptions are NOT a business workflow step. Matching a
    // dealer happens asynchronously through native Rust automatically;
    // no "confirm dealer" stage/button or undo slot is rendered for them.
    if let Some(text)=button_text {
        let response=workflow_primary_at(
            ui,id,workflow_control_rect(action),text,enabled,in_preview
        );
        let undo_clicked=workflow_auxiliary_at(
            ui,id,workflow_control_rect(undo),
            components::WorkflowAuxiliaryAction::PreviousStage,
            undo_enabled,in_preview
        ).clicked();
        (response.clicked(),response.hovered(),undo_clicked)
    }else{
        (false,false,false)
    }
}
/// Keep the expanded shipment detail inset; the compact header and action
/// bands alone are full width. Neither selected nor hover preview changes
/// the surrounding card's fixed geometry.
fn workflow_details<R>(
    ui:&mut Ui,body:impl FnOnce(&mut Ui)->R
)->R{
    ui.add_space(theme::WORKFLOW_CARD_PADDING_Y);
    let result=ui.horizontal(|row|{
        row.spacing_mut().item_spacing.x=0.0;
        row.add_space(theme::WORKFLOW_CARD_PADDING_X);
        let width=(row.available_width()-theme::WORKFLOW_CARD_PADDING_X).max(0.0);
        row.vertical(|details|{
            details.set_width(width);
            details.spacing_mut().item_spacing.y=theme::SPACING;
            body(details)
        }).inner
    }).inner;
    ui.add_space(theme::WORKFLOW_CARD_PADDING_Y);
    result
}
/// This expanded view is an intentionally plain shipment LIST, not the
/// management master table. The products are ordinary single-line text,
/// without the previous chip fill/border or three-column freight header.
fn shipment_product_summary(products:&[String])->String{
    if products.is_empty(){"◆ 상품 확인".to_owned()}
    else{format!("◆ {}",products.join("  ·  ◆ "))}
}

/// Native shipment rows share the SAME linked-preview fill, ink and inset
/// border as the source tokens, recipient values and item cells.
/// Orange remains reserved for an explicitly committed workflow selection.
/// The highlight is paint-only: no row height, width or scroll geometry changes.
fn shipment_line(
    ui:&mut Ui,name:&str,products:&[String],chosen:bool,previewed:bool
)->egui::Response{
    let fill=if chosen{
        theme::WORKFLOW_SELECTED_BORDER.linear_multiply(0.20)
    }else if previewed{
        source_highlight::ACTIVE_FILL
    }else{theme::WORKFLOW_DETAIL_ROW_BG};
    let outer=ui.available_width();
    let summary=shipment_product_summary(products);
    let preview_ink=if previewed && !chosen{
        source_highlight::ACTIVE_TEXT
    }else{theme::TEXT};
    let response=Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(
            theme::SOFT_DIVIDER_WIDTH,theme::WORKFLOW_DETAIL_ROW_BORDER
        ))
        .corner_radius(theme::CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            theme::WORKFLOW_DETAIL_ROW_PADDING_X as i8,
            theme::SPACE_4 as i8
        ))
        .show(ui,|row|{
            // Inner margins do not include the 1pt frame stroke on either
            // edge. Reserve both strokes so neighboring rows have exactly
            // the same width and the list never grows a horizontal rail.
            let inside=(outer-2.0*theme::WORKFLOW_DETAIL_ROW_PADDING_X
                -2.0*theme::SOFT_DIVIDER_WIDTH).max(0.0);
            row.set_min_width(inside);
            row.horizontal(|cols|{
                cols.spacing_mut().item_spacing.x=theme::SPACE_8;
                let name_width=theme::WORKFLOW_DETAIL_NAME_WIDTH
                    .min((inside*0.36).max(0.0));
                let product_width=(inside-name_width-theme::SPACE_8).max(0.0);
                cols.add_sized(
                    [name_width,theme::WORKFLOW_DETAIL_ROW_CONTENT_HEIGHT],
                    egui::Label::new(typography::body(name).color(preview_ink)).truncate()
                ).on_hover_text(name);
                cols.add_sized(
                    [product_width,theme::WORKFLOW_DETAIL_ROW_CONTENT_HEIGHT],
                    egui::Label::new(typography::body(&summary).color(preview_ink)).truncate()
                ).on_hover_text(&summary);
            });
        });
    if chosen || previewed{
        let color=if chosen{theme::WORKFLOW_SELECTED_BORDER}
            else{source_highlight::ACTIVE_BORDER};
        let rect=response.response.rect;
        ui.painter().rect_filled(Rect::from_min_max(
            Pos2::new(rect.left(),rect.top()),
            Pos2::new(rect.left()+theme::WORKFLOW_SELECTED_STRIPE,rect.bottom())
        ),0.0,color);
        if previewed && !chosen {
            // Overlay rather than changing Frame::stroke; changing a frame
            // stroke also changes its content width and can cause clipping.
            ui.painter().rect_stroke(
                rect.shrink(0.5),theme::CONTROL_RADIUS as f32,
                egui::Stroke::new(
                    source_highlight::ACTIVE_BORDER_WIDTH,
                    source_highlight::ACTIVE_BORDER
                ),
                egui::StrokeKind::Inside,
            );
        }
    }
    response.response
}
fn row(
    ui:&mut Ui,ship:&ShipmentRow,chosen:bool,previewed:bool
)->egui::Response{
    let name=if ship.recipient_name.is_empty(){
        "수취인 확인"
    }else{ship.recipient_name.as_str()};
    shipment_line(ui,name,&ship.products,chosen,previewed)
}
fn group_frame(
    ui:&mut Ui,group:&GroupRow,selected:Option<&Selection>,
    open:bool,in_preview:bool,previewed_id:Option<&str>,writable:bool,
    linked_selection:Option<(&str,&str)>,
    trigger:&mut Option<String>,hovered_row:&mut Option<String>
)->Option<Event>{
    let mut event=None;
    let active=selected.is_some_and(|s|s.group_id==group.id);
    workflow_card(ui,active && !in_preview,in_preview,|ui|{
        let (top,bottom)=workflow_compact_rows(ui);
        let btn=workflow_header(
            ui,top,&group.id,&group.dealer_name,open || in_preview,
            group.step,group.needs_notice,in_preview,
            true,writable && !group.transferred
        );
        if btn.manage{event=Some(Event::ManageGroup(group.id.clone()));}
        if btn.delete{event=Some(Event::DeleteGroup(group.id.clone()));}
            if btn.title.clicked(){
                if active {
                    event=Some(Event::ToggleGroup(group.id.clone()));
                }else if let Some(first)=group.shipments.first(){
                    event=Some(Event::SelectShipment(Selection{
                        group_id:group.id.clone(),order_id:first.order_id.clone(),
                        shipment_id:first.id.clone()
                    }));
                }
            }
        let vat=if group.vat_included>0 && group.vat_excluded>0{
            "부가세 일부 포함".to_owned()
        }else if group.vat_included>0{
            "부가세 포함".to_owned()
        }else{"부가세 별도".to_owned()};
        let (advance_clicked,stage_hovered,undo_clicked)=workflow_action_line(
            ui,bottom,&group.id,group.shipments.len(),group.total,
            if group.vat_included>0{Some(vat.as_str())}else{None},
            Some(stage_name(group.step,group.needs_notice)),
            writable && !group.transferred,
            writable && group.step>0 && !group.transferred,in_preview
        );
        if advance_clicked{event=Some(Event::AdvanceGroup(group.id.clone()));}
        if undo_clicked{event=Some(Event::UndoGroup(group.id.clone()));}
        if stage_hovered && !in_preview{
            *trigger=Some(group.id.clone());
        }
        if open || in_preview{
            workflow_details(ui,|ui|{
                components::soft_divider(ui);
                ui.add_space(theme::SPACE_4);
            for ship in &group.shipments{
                // Orange means committed selection ONLY on the base card.
                // Blue means the temporarily hovered row ONLY in the clone.
                let chosen=!in_preview && selected.is_some_and(|s|s.shipment_id==ship.id);
                let previewed=(!chosen && linked_selection
                    ==Some((ship.order_id.as_str(),ship.recipient_id.as_str()))) ||
                    (in_preview && previewed_id==Some(ship.id.as_str()));
                let label=row(ui,ship,chosen,previewed);
                let response=ui.interact(
                    label.rect,Id::new(("workflow-shipment",ship.id.as_str(),in_preview)),
                    egui::Sense::click()
                );
                if in_preview && response.hovered(){
                    *hovered_row=Some(ship.id.clone());
                }
                if response.hovered() && !chosen && !previewed {
                    theme::paint_button_hover(ui,&response,0.0);
                }
                if response.clicked(){
                    event=Some(Event::SelectShipment(Selection{
                        group_id:group.id.clone(),order_id:ship.order_id.clone(),
                        shipment_id:ship.id.clone()
                    }));
                }
            }
            });
        }
    });
    event
}
/// A selected client ID is the source of truth. The automatic-match
/// projection may be cleared after a MANUAL native client change, so never
/// derive the card title solely from client_match.name. Name lookup is a
/// transient read-only directory projection and never affects billing.
fn pending_dealer_title(
    draft:&WorkingReception,names:&HashMap<String,String>,
    active_selection:Option<(&str,&str)>
)->String{
    let Some(client_id)=draft.client_id.as_deref() else{
        return "미확인 거래처".to_owned();
    };
    // For the active order prefer its already verified Rust analysis, which
    // is the same selected name rendered in the source document band.
    let verified_active=active_selection.and_then(|(id,name)|
        (id==client_id && !name.trim().is_empty()).then_some(name)
    );
    verified_active
        .or_else(||names.get(client_id).map(String::as_str))
        .or_else(||draft.client_match.as_ref().and_then(|matched|{
            (draft.analysis_raw.as_deref()==Some(draft.raw.as_str()) &&
                matched.get("clientId").and_then(serde_json::Value::as_str)
                    ==Some(client_id))
                .then(||matched.get("name").and_then(serde_json::Value::as_str))
                .flatten()
        }))
        .filter(|name|!name.trim().is_empty())
        .unwrap_or("거래처 이름 확인 필요")
        .to_owned()
}

/// Both pending read-only panes and workflow rows resolve recipient hover
/// from the same order-scoped link, never from matching display names.
pub fn visible_pending_recipient<'a>(
    order_id:&str,preview:Option<&'a PendingTarget>,
    linked_selection:Option<(&'a str,&'a str)>
)->Option<&'a str>{
    linked_selection.and_then(|(source_order,id)|
        (source_order==order_id).then_some(id)
    ).or_else(||preview.filter(|target|target.order_id==order_id)
        .and_then(|target|target.recipient_id.as_deref()))
}

fn pending_frame(
    ui:&mut Ui,draft:&WorkingReception,selected:bool,open:bool,in_preview:bool,
    dealer_name:&str,committed_recipient:Option<&str>,
    preview:Option<&PendingTarget>,linked_selection:Option<(&str,&str)>,
    hovered_pending:&mut Option<PendingTarget>
)->Option<Event>{
    let mut event=None;
    workflow_card(ui,selected && !in_preview,in_preview,|ui|{
        let (top,bottom)=workflow_compact_rows(ui);
        let header=workflow_header(
            ui,top,&draft.id,dealer_name,open,0,false,in_preview,true,true
        );
        if header.manage{event=Some(Event::ManagePending(draft.id.clone()));}
        if header.delete{event=Some(Event::DeletePending(draft.id.clone()));}
        if header.title.hovered() {
            *hovered_pending=Some(PendingTarget::order(&draft.id));
        }
        if header.title.clicked(){
            event=Some(if selected{Event::TogglePending(draft.id.clone())}
                else{Event::SelectPending(draft.id.clone())});
        }
        // A corrected/manual working receipt is the active view. The first
        // OCR recognition remains immutable audit evidence, not the display
        // source for recipient IDs or product quantities.
        let recipients=if draft.analysis_raw.as_deref()==Some(draft.raw.as_str()){
            draft.manual_model.as_ref()
                .and_then(|model|model.pointer("/model/rows"))
                .and_then(|value|value.as_array())
                .or_else(||draft.analysis_view.as_ref()
                    .and_then(|view|view.pointer("/recognition/recipients"))
                    .and_then(|value|value.as_array()))
                .or_else(||draft.analysis_evidence.as_ref()
                    .and_then(|value|value.get("recipients"))
                    .and_then(|value|value.as_array()))
                .map(Vec::as_slice).unwrap_or(&[])
        }else{&[]};
        // Keep the familiar workflow action slot, but dealer matching is
        // fully automatic and never a manually executable stage.
        let _=workflow_action_line(
            ui,bottom,&draft.id,recipients.len(),None,None,
            Some("거래처 확인"),false,false,in_preview
        );
        if open{
            workflow_details(ui,|ui|{
                components::soft_divider(ui);
                ui.add_space(theme::SPACE_4);
            if recipients.is_empty(){
                let excerpt=draft.raw.lines().find(|s|!s.trim().is_empty())
                    .unwrap_or("새 발주서");
                let response=shipment_line(ui,"수취인 확인",
                    &[excerpt.chars().take(60).collect()],false,false);
                let action=ui.interact(response.rect,
                    Id::new(("pending-reception-row",draft.id.as_str(),0usize,in_preview)),
                    egui::Sense::click());
                theme::paint_button_hover(ui,&action,0.0);
                if action.hovered() {
                    *hovered_pending=Some(PendingTarget::order(&draft.id));
                }
                if action.clicked(){
                    event=Some(Event::SelectPending(draft.id.clone()));
                }
            }else{
                for (index,recipient) in recipients.iter().enumerate(){
                    let name=recipient.get("name").and_then(|v|v.as_str())
                        .filter(|s|!s.is_empty()).unwrap_or("수취인 확인");
                    let products=recipient.get("items").and_then(|items|items.as_array())
                        .into_iter().flatten().map(|item|{
                            let name=item.get("name").and_then(|x|x.as_str())
                                .filter(|n|!n.trim().is_empty()).or_else(||
                                    item.get("raw").and_then(|x|x.as_str())
                                ).unwrap_or("미확정 상품");
                            let qty=item.get("quantity").and_then(|x|x.as_i64())
                                .filter(|q|*q>0).map_or("?".to_owned(),|q|q.to_string());
                            format!("{name} × {qty}")
                        }).collect::<Vec<_>>();
                    let recipient_id=recipient.get("id")
                        .and_then(|value|value.as_str()).unwrap_or("");
                    let chosen=selected && !recipient_id.is_empty() &&
                        committed_recipient==Some(recipient_id);
                    let displayed=visible_pending_recipient(
                        &draft.id,preview,linked_selection
                    );
                    let previewed=displayed==Some(recipient_id)
                        && !recipient_id.is_empty() && !chosen;
                    let response=shipment_line(ui,name,&products,chosen,previewed);
                    let action=ui.interact(response.rect,
                        Id::new(("pending-reception-row",draft.id.as_str(),index,in_preview)),
                        egui::Sense::click());
                    theme::paint_button_hover(ui,&action,0.0);
                    if action.hovered(){
                        *hovered_pending=Some(
                            PendingTarget::recipient(&draft.id,recipient_id)
                        );
                    }
                    if action.clicked(){
                        event=Some(Event::SelectPendingRecipient(
                            PendingTarget::recipient(&draft.id,recipient_id)
                        ));
                    }
                }
            }
            });
        }
    });
    event
}
/// Render once in the existing fixed pane. The floating expansion uses an egui
/// foreground Area anchored to the card; it DOES NOT resize/reorder other cards.
pub fn show(
    ui:&mut Ui,
    snapshot:Option<&BoardSnapshot>,
    drafts:&[WorkingReception],active_id:Option<&str>,
    client_names:&HashMap<String,String>,
    active_selected_client:Option<(&str,&str)>,
    state:&mut BoardUi,writable:bool,allow_pending_hover_switch:bool,
    active_recipient_id:Option<&str>,linked_selection:Option<(&str,&str)>
)->Option<Event>{
    // Prevent implicit vertical item spacing from doubling the explicit
    // WORKFLOW_CARD_GAP between order bundles. This is scoped to the list
    // inside its own ScrollArea; workflow_card restores internal spacing.
    ui.spacing_mut().item_spacing.y=0.0;
    let mut event=None;
    let mut trigger=None;
    let mut hovered_pending=None;
    let mut overlay=None;
    let mut pending_overlay=None;
    let selected=state.selection.clone();
    let current=selected.as_ref();
    if drafts.is_empty() && snapshot.is_none_or(|view|view.groups.is_empty()){
        ui.label(typography::caption(
            "거래처와 발주 원문을 입력하면 수취인별 배송 건이 생성됩니다."
        ));
    }
    if let Some(snapshot)=snapshot{
        for group in &snapshot.groups {
            let selected=current.is_some_and(|s|s.group_id==group.id);
            let open=selected && state.folded.as_deref()!=Some(group.id.as_str());
            let width=ui.available_width();
            let pos=ui.cursor().min;
            let mut ignored_row=None;
            if let Some(value)=group_frame(ui,group,current,open,false,None,
                writable,linked_selection,&mut trigger,&mut ignored_row){
                match value{
                    Event::ToggleGroup(id)=>{
                        state.folded=if open{Some(id)}else{None};
                    }
                    other=>event=Some(other),
                }
            }
            if state.active_preview()==Some(group.id.as_str()) && !open{
                overlay=Some((group.clone(),pos,width));
            }
            ui.add_space(theme::WORKFLOW_CARD_GAP);
        }
    }
    for draft in drafts.iter().rev(){
        // Do not duplicate an order that is already in a native group.
        if snapshot.is_some_and(|snapshot|snapshot.groups.iter().any(|group|
            group.shipments.iter().any(|ship|ship.order_id==draft.id))){continue;}
        let selected=state.selection.is_none() && active_id==Some(draft.id.as_str());
        let key=format!("pending:{}",draft.id);
        let open=selected && state.folded.as_deref()!=Some(key.as_str());
        let name=pending_dealer_title(
            draft,client_names,
            if selected{active_selected_client}else{None}
        );
        if allow_pending_hover_switch && !selected &&
            state.pending_visible.as_ref().is_some_and(|target|
                target.order_id==draft.id){
            pending_overlay=Some((draft.clone(),name.clone(),ui.cursor().min,
                ui.available_width()));
        }
        if let Some(value)=pending_frame(
            ui,draft,selected,open,false,&name,
            if selected{active_recipient_id}else{None},
            state.pending_visible.as_ref(),
            linked_selection,
            &mut hovered_pending
        ){
            match value{
                Event::TogglePending(_)=>{
                    state.folded=if open{Some(key)}else{None};
                }
                other=>event=Some(other),
            }
        }
        ui.add_space(theme::WORKFLOW_CARD_GAP);
    }
    // The pending foreground clone uses the same card geometry without
    // changing list height. Every interactive ID differs from its base card.
    if let Some((draft,name,pos,width))=pending_overlay{
        let pane_clip=ui.clip_rect();
        let mut overlay_event=None;
        let mut overlay_hover=None;
        let area=egui::Area::new(Id::new(("pending-card-overlay",draft.id.clone())))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(pos.x,pos.y))
            .movable(false);
        let response=area.show(ui.ctx(),|overlay_ui|{
            overlay_ui.set_clip_rect(pane_clip);
            overlay_ui.set_min_width(width);
            overlay_ui.set_max_width(width);
            overlay_event=pending_frame(
                overlay_ui,&draft,false,true,true,&name,None,
                state.pending_visible.as_ref(),linked_selection,&mut overlay_hover
            );
        });
        if overlay_hover.is_some(){
            hovered_pending=overlay_hover;
        }else if ui.ctx().input(|input|input.pointer.hover_pos()
            .is_some_and(|pointer|response.response.rect.contains(pointer))){
            // Whitespace between rows is still a hover-connected surface.
            hovered_pending=state.pending_visible.clone();
        }
        if let Some(value)=overlay_event{event=Some(value);}
    }
    // Hover changes display only. Only actual clicks emit SelectPending.
    if allow_pending_hover_switch && event.is_none(){
        let committed=if state.selection.is_none(){
            active_id.map(|id|PendingTarget{
                order_id:id.to_owned(),
                recipient_id:active_recipient_id.map(str::to_owned)
            })
        }else{None};
        state.update_pending_order_hover(hovered_pending,committed.as_ref(),ui.ctx());
    }else{
        state.pending_order_hover=None;
        state.pending_visible=None;
        state.pending_release=None;
        state.pending_content_hovered=None;
    }
    let hovered=trigger.clone();
    state.update_hover(hovered,ui.ctx());
    if let Some((group,pos,width))=overlay{
        let pane_clip=ui.clip_rect();
        let mut overlay_event=None;
        let mut hovered_row=None;
        let area=egui::Area::new(Id::new(("workflow-card-overlay",group.id.clone())))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(pos.x,pos.y))
            .movable(false);
        let response=area.show(ui.ctx(),|ui|{
            // The temporary card can grow downward, but must not bleed
            // over the source/details panes or the app header.
            ui.set_clip_rect(pane_clip);
            ui.set_min_width(width);
            ui.set_max_width(width);
            let mut ignored_hover=None;
            overlay_event=group_frame(
                ui,&group,current,true,true,state.preview_shipment.as_deref(),
                writable,linked_selection,&mut ignored_hover,&mut hovered_row
            );
        });
        state.preview_rect=Some(response.response.rect);
        state.update_row_hover(hovered_row,ui.ctx());
        if let Some(overlay_event)=overlay_event {
            if let Event::ToggleGroup(id)=overlay_event {
                state.folded=None;
                if let Some(first)=group.shipments.first(){
                    event=Some(Event::SelectShipment(Selection{
                        group_id:id,order_id:first.order_id.clone(),shipment_id:first.id.clone()
                    }));
                }
            }else {
                // The foreground card is fully usable: forwarding its
                // actions is safe because every widget ID includes
                // in_preview, avoiding cross-layer egui ID collisions.
                event=Some(overlay_event);
            }
        }
    }else{
        state.preview_rect=None;
        state.update_row_hover(None,ui.ctx());
    }
    event
}

#[cfg(test)]
mod shipment_list_tests {
    use super::*;
    #[test]
    fn plain_rows_display_products_as_text_without_chip_metadata(){
        assert_eq!(
            shipment_product_summary(&[
                "BD 709 / 블랙 / 타공 상판 × 4".into()
            ]),
            "◆ BD 709 / 블랙 / 타공 상판 × 4"
        );
        assert_eq!(
            shipment_product_summary(&[
                "BD 709 × 4".into(),"BD 뮤직 트레이 × 1".into()
            ]),
            "◆ BD 709 × 4  ·  ◆ BD 뮤직 트레이 × 1"
        );
    }

    #[test]
    fn plain_list_rows_keep_same_geometry_without_zebra_background(){
        let ctx=egui::Context::default();
        let mut measured=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(390.0);
                for (selected,preview) in [(false,false),(false,false),
                    (true,false),(false,true)]{
                    let response=shipment_line(
                        ui,"김준희",&["BD 709 / 블랙 × 4".into()],
                        selected,preview
                    );
                    measured.push(response.rect);
                }
            });
        });
        assert_eq!(measured.len(),4);
        for rect in &measured{
            assert_eq!(rect.width(),measured[0].width());
            assert_eq!(rect.height(),measured[0].height());
            assert!(rect.height()>=40.0 && rect.height()<=46.0);
        }
        assert_eq!(theme::WORKFLOW_DETAIL_ROW_BG,theme::PANEL);
    }
}

#[cfg(test)]
mod pending_matching_tests {
    use super::*;

    #[test]
    fn pending_receipt_never_emits_a_fake_dealer_confirmation_action(){
        let ctx=egui::Context::default();
        let mut tapped=None;
        let draft=WorkingReception{
            id:"egui-pending-test".to_owned(),
            raw:"테스트 주문".to_owned(),
            order_type:"AGENCY".to_owned(),
            source_input_method:None,
            last_input_method:None,
            analysis_raw:None,
            analysis_evidence:None,
            analysis_view:None,
            client_match:None,
            chat_handle:None,
            chat_title:None,
            client_id:Some("verified-client".to_owned()),
            client_selection:Some("AUTO".to_owned()),
            pricing_preview:None,
            working_version:None,
            working_synced:false,
            manual_model:None,
            edit_history:Vec::new()
        };
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let mut hovered=None;
                tapped=pending_frame(
                    ui,&draft,false,false,false,"확인된 거래처",None,None,
                    None,&mut hovered
                );
            });
        });
        assert!(tapped.is_none());
    }
}

#[cfg(test)]
mod pending_hover_navigation_tests{
    use super::*;
    #[test]
    fn common_preview_resolver_prioritizes_scoped_evidence_without_cross_order_leak(){
        let preview=PendingTarget::recipient("order-a","r1");
        assert_eq!(visible_pending_recipient(
            "order-a",Some(&preview),Some(("order-a","r2"))
        ),Some("r2"));
        assert_eq!(visible_pending_recipient(
            "order-a",Some(&preview),Some(("order-b","r2"))
        ),Some("r1"));
        assert_eq!(visible_pending_recipient(
            "order-b",Some(&preview),Some(("order-a","r2"))
        ),None);
    }

    #[test]
    fn hover_previews_recipient_without_committing_and_restores_on_leave(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let committed=PendingTarget::recipient("order","kim");
        let preview=PendingTarget::recipient("order","pyo");
        board.update_pending_order_hover(Some(preview.clone()),Some(&committed),&ctx);
        assert!(board.pending_preview().is_none());
        board.pending_order_hover=Some((
            preview.clone(),Instant::now()-Duration::from_millis(260)
        ));
        board.update_pending_order_hover(Some(preview.clone()),Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&preview));
        assert!(board.selection.is_none());
        board.update_pending_order_hover(None,Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&preview));
        board.pending_release=Some(Instant::now()-Duration::from_millis(1));
        board.update_pending_order_hover(None,Some(&committed),&ctx);
        assert!(board.pending_preview().is_none());
        assert_eq!(PENDING_ORDER_HOVER_ENTER,Duration::from_millis(250));
        assert_eq!(PENDING_ORDER_HOVER_EXIT,Duration::from_millis(250));
    }
    #[test]
    fn source_and_detail_hover_retain_pending_preview_across_panel_transition(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let preview=PendingTarget::recipient("order-b","r2");
        board.pending_visible=Some(preview.clone());
        board.note_pending_content_hover("order-a",true);
        board.update_pending_order_hover(None,None,&ctx);
        assert!(board.pending_release.is_some());
        board.note_pending_content_hover("order-b",true);
        board.update_pending_order_hover(None,None,&ctx);
        assert_eq!(board.pending_preview(),Some(&preview));
        assert!(board.pending_release.is_none());
        // The source-panel keepalive cannot turn a preview into a click.
        assert!(board.selection.is_none());
        assert!(board.pending_content_hovered.is_none());
    }

    #[test]
    fn hovering_selected_order_header_does_not_replace_recipient(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let selected=PendingTarget::recipient("order","kim");
        board.update_pending_order_hover(
            Some(PendingTarget::order("order")),Some(&selected),&ctx
        );
        assert!(board.pending_preview().is_none());
        assert!(board.pending_order_hover.is_none());
    }
    #[test]
    fn moving_to_another_recipient_restarts_the_timer(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        board.update_pending_order_hover(
            Some(PendingTarget::recipient("order","kim")),None,&ctx
        );
        let new_target=PendingTarget::recipient("order","pyo");
        board.update_pending_order_hover(Some(new_target.clone()),None,&ctx);
        assert_eq!(board.pending_order_hover.as_ref().unwrap().0,new_target);
        assert!(board.pending_preview().is_none());
        board.stop_preview();
        assert!(board.pending_order_hover.is_none());
    }
}

#[cfg(test)]
mod widget_id_tests {
    use super::workflow_widget_id;

    #[test]
    fn foreground_preview_ids_never_reuse_base_card_ids() {
        for kind in [
            "workflow-header", "workflow-stage-button",
            "workflow-shipping", "workflow-money",
        ] {
            let base=workflow_widget_id(kind,"group-123",false);
            let preview=workflow_widget_id(kind,"group-123",true);
            assert_ne!(base,preview,"{kind} must be unique per layer");
            assert_ne!(base,workflow_widget_id(kind,"group-456",false));
        }
    }

    #[test]
    fn distinct_card_controls_never_reuse_each_others_ids() {
        let kinds=[
            "workflow-header", "workflow-stage-button",
            "workflow-shipping", "workflow-money",
        ];
        for (i,kind) in kinds.iter().enumerate() {
            for other in kinds.iter().skip(i+1) {
                assert_ne!(
                    workflow_widget_id(kind,"group-123",false),
                    workflow_widget_id(other,"group-123",false)
                );
            }
        }
    }
}
