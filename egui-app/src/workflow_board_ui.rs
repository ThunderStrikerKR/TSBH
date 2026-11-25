//! The right-hand workflow board. Layout/hover/selection only; no business rules.
//! Native Rust provides groups, amounts, freight, stage and shipment identity.
use eframe::egui::{self, Id, Pos2, Rect, Ui, Vec2};
use std::time::{Duration, Instant};
use crate::{components, preview_store::WorkingReception, source_highlight, theme, typography};
use std::collections::{HashMap,HashSet};
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
    // Canonical slot identity is unchanged when a draft becomes group:<id>.
    card_order:Vec<String>,
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
    pending_stage_target:Option<PendingTarget>,
}
pub enum Event {
    SelectPending(String),
    ConfirmDraft(String),
    SelectPendingRecipient(PendingTarget),
    SelectShipment(Selection),
    ToggleGroup(String),
    TogglePending,
    ManageGroup(String),
    DeleteGroup(String),
    AdvanceGroup(String),
    UndoGroup(String),
    ManagePending(String),
    DeletePending(String),
}
impl BoardUi{
    pub fn clear_card_order(&mut self){self.card_order.clear();}
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
        self.pending_stage_target=None;
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
        self.preview_active.as_deref().filter(|id|!id.starts_with("pending-stage:"))
    }
    pub fn active_pending_stage(&self)->Option<&str>{
        self.preview_active.as_deref()?.strip_prefix("pending-stage:")
    }
    pub fn pending_preview(&self)->Option<&PendingTarget>{
        self.pending_stage_target.as_ref().or(self.pending_visible.as_ref())
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
        self.update_pending_order_hover_at(incoming,committed,ctx,Instant::now());
    }
    // Only the clock is injected for deterministic 249/250ms transition
    // tests; the user-facing hit targets and production delay are unchanged.
    fn update_pending_order_hover_at(
        &mut self,incoming:Option<PendingTarget>,committed:Option<&PendingTarget>,
        ctx:&egui::Context,now:Instant
    ){
        let keep_content=self.pending_content_hovered.take()
            .is_some_and(|order|self.pending_visible.as_ref()
                .is_some_and(|visible|visible.order_id==order));
        if let Some(candidate)=incoming{
            self.pending_release=None;
            if candidate.recipient_id.is_none() &&
                committed.is_some_and(|current|
                    current.order_id==candidate.order_id){
                // The selected ORDER HEADER must not replace its recipient.
                // The selected RECIPIENT ROW is different: hovering it must
                // still emit the transient source/detail highlight, without
                // committing any selection or changing the orange row state.
                self.pending_order_hover=None;
                if self.pending_visible.take().is_some(){ctx.request_repaint();}
                return;
            }
            if self.pending_visible.as_ref()==Some(&candidate){
                self.pending_order_hover=None;
                return;
            }
            // Another recipient is now under the pointer. Do not leave the
            // old recipient blue while the new target waits out its 250ms
            // entrance delay. The 250ms exit grace applies only to genuine
            // pointer gaps (incoming=None), not to A -> B row transitions.
            if self.pending_visible.take().is_some(){
                ctx.request_repaint();
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
    let frame=theme::workflow_group_card_frame(selected||preview);
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
            0.0,if preview{source_highlight::ACTIVE_BORDER}else{theme::WORKFLOW_SELECTED_BORDER}
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
    components::workflow_fixed_action(
        ui,Id::new(("workflow-aux",id,kind,in_preview)),rect,
        components::WorkflowFixedAction::Auxiliary(action),enabled
    )
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
    let divider=workflow_row_content(divider);
    ui.painter().rect_filled(divider,0.0,theme::WORKFLOW_CARD_BORDER);
    // The enclosing rounded card owns both bands' background.
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
/// No stage is active while the dealer is still unconfirmed.
fn workflow_signal_palette(step:Option<usize>,index:usize)->(egui::Color32,egui::Color32){
    match step{
        Some(step) if index<step=>(theme::WORKFLOW_SIGNAL_DONE,theme::WORKFLOW_SIGNAL_DONE_BORDER),
        Some(step) if index==step=>(theme::WORKFLOW_SIGNAL_CURRENT,theme::WORKFLOW_SIGNAL_CURRENT_BORDER),
        _=>(theme::WORKFLOW_SIGNAL_PENDING,theme::WORKFLOW_SIGNAL_PENDING_BORDER),
    }
}

fn workflow_header(
    ui:&mut Ui,row:Rect,id:&str,title:&str,open:bool,step:Option<usize>,
    in_preview:bool,can_manage:bool,can_delete:bool
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
    // Fit the actual allocated title slot; never limit by character count.
    // Full text is available only when the measured glyphs overflow.
    let title_ink_width=text_rect.width();
    let visible=workflow_title_fit(ui,title,title_ink_width);
    let galley=typography::workflow_dealer_galley(ui,&visible);
    ui.painter().with_clip_rect(text_rect).galley(
        Pos2::new(text_rect.left(),text_rect.center().y-galley.size().y/2.0),
        galley,theme::TEXT
    );
    let title_response=ui.interact(
        arrow.union(title_rect),
        workflow_widget_id("workflow-header",id,in_preview),egui::Sense::click()
    );
    let title_response=components::shortened_text_tooltip(title_response,&visible,title);

    let block=workflow_control_rect(signal);
    theme::paint_bordered_surface(ui.painter(),block,5.0,
        theme::WORKFLOW_SIGNAL_BLOCK_BG,theme::WORKFLOW_SIGNAL_BLOCK_BORDER);
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
        let (fill,border)=workflow_signal_palette(step,index);
        theme::paint_bordered_surface(ui.painter(),lamp,theme::WORKFLOW_SIGNAL_RADIUS,fill,border);
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
/// Compact one-touch button, 154×30pt at the 1080p reference, preserving
/// every stage label including "계좌 재안내 완료" without shrinking text.
/// This uses the shared Emphasized/disabled palette and 13pt action type,
/// but not egui's minimum text padding (which could enlarge the rectangle).
/// egui can replace a pending card's base widget with its foreground
/// hover-preview clone between pointer-down and pointer-up. The widget ID then
/// changes during one physical click, so Response::clicked() can lose it.
/// Recover that release only when it lands inside this exact control AND the
/// control's layer is the topmost layer at the pointer (never click through a
/// modal or another foreground window).
fn primary_release_inside(ui:&Ui,rect:Rect)->bool{
    let ctx=ui.ctx();
    let press_origin_id=Id::new("bando-workflow-primary-press-origin");
    let (pressed,released,clicked,origin,release_pos)=ctx.input(|input|{
        (
            input.pointer.button_pressed(egui::PointerButton::Primary),
            input.pointer.button_released(egui::PointerButton::Primary),
            input.pointer.button_clicked(egui::PointerButton::Primary),
            input.pointer.press_origin(),
            input.pointer.interact_pos()
        )
    });
    if pressed{
        ctx.data_mut(|data|data.insert_temp(press_origin_id,origin));
    }
    if !released{return false;}
    if !clicked{
        ctx.data_mut(|data|data.insert_temp(press_origin_id,None::<Pos2>));
        return false;
    }
    let Some(pos)=release_pos.filter(|pos|
        rect.contains(*pos) && ctx.layer_id_at(*pos)==Some(ui.layer_id())
    )else{return false;};
    let origin=ctx.data_mut(|data|{
        let origin=data.get_temp::<Option<Pos2>>(press_origin_id).flatten();
        data.insert_temp(press_origin_id,None::<Pos2>);
        origin
    });
    origin.is_some_and(|start|rect.contains(start)) && rect.contains(pos)
}
fn workflow_primary_at(
    ui:&mut Ui,id:&str,rect:Rect,text:&str,enabled:bool,in_preview:bool
)->(egui::Response,bool){
    let response=components::workflow_fixed_action(
        ui,workflow_widget_id("workflow-stage-button",id,in_preview),rect,
        components::WorkflowFixedAction::Stage(text),enabled
    );
    let clicked=enabled && (response.clicked() || primary_release_inside(ui,rect));
    (response,clicked)
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
    undo_enabled:bool,in_preview:bool,payment_emphasis:bool,idle_caption:Option<&str>
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
    let shipping_clipped=components::text_exceeds_rect(ship_galley.size(),shipping.intersect(ui.clip_rect()));
    ui.painter().with_clip_rect(shipping).galley(
        Pos2::new(shipping.left(),bottom_y-ship_galley.size().y),
        ship_galley,theme::SECONDARY
    );
    components::overflow_region_tooltip(ui,shipping,
        workflow_widget_id("workflow-shipping",id,in_preview),shipping_clipped,&shipping_text);

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
    if payment_emphasis{
        let amount_rect=Rect::from_min_size(Pos2::new((price_ink.right()-money_galley.size().x-2.0).max(price_ink.left()),bottom_y-money_galley.size().y-2.0),Vec2::new((money_galley.size().x+4.0).min(price_ink.width()),money_galley.size().y+4.0)).intersect(price_ink);
        crate::one_touch::background(ui,amount_rect);crate::one_touch::outline(ui,amount_rect);
    }
    let money_clipped=components::text_exceeds_rect(money_galley.size(),price_ink.intersect(ui.clip_rect()));
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
    components::overflow_region_tooltip(ui,price,
        workflow_widget_id("workflow-money",id,in_preview),money_clipped,&amount);

    // A pending dealer match is automatic. The existing disabled controls
    // show the next action without creating a new business transition.
    if let Some(text)=button_text {
        let (response,clicked)=workflow_primary_at(
            ui,id,workflow_control_rect(action),text,enabled,in_preview
        );
        let undo_response=workflow_auxiliary_at(
            ui,id,workflow_control_rect(undo),
            components::WorkflowAuxiliaryAction::PreviousStage,
            undo_enabled,in_preview
        );
        let undo_clicked=undo_response.clicked();
        if clicked{crate::floating_notice::action_rect(ui.ctx(),"reception.workflow",response.rect);}
        if undo_clicked{crate::floating_notice::clicked(ui.ctx(),"reception.workflow",&undo_response);}
        (clicked,response.hovered(),undo_clicked)
    }else{
        if let Some(caption)=idle_caption{
            components::workflow_idle_caption(ui,action,caption);
        }
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
fn shipment_row_fill(chosen:bool,previewed:bool)->egui::Color32{
    if chosen {
        // A confirmed choice must never be overwritten by a hover preview.
        theme::workflow_selected_row_fill()
    }else if previewed {
        source_highlight::ACTIVE_FILL
    }else {
        theme::WORKFLOW_DETAIL_ROW_BG
    }
}
fn shipment_line(
    ui:&mut Ui,name:&str,products:&[String],chosen:bool,previewed:bool
)->egui::Response{
    shipment_line_with_freight(ui,name,products,chosen,previewed,None,false)
}
fn shipment_line_with_freight(
    ui:&mut Ui,name:&str,products:&[String],chosen:bool,previewed:bool,freight:Option<&str>,freight_emphasis:bool
)->egui::Response{
    components::workflow_shipment_row(
        ui,name,&shipment_product_summary(products),shipment_row_fill(chosen,previewed),
        chosen,previewed,freight,freight_emphasis
    )
}

fn row(
    ui:&mut Ui,ship:&ShipmentRow,chosen:bool,previewed:bool,freight_emphasis:bool
)->egui::Response{
    let name=if ship.recipient_name.is_empty(){
        "수취인 확인"
    }else{ship.recipient_name.as_str()};
    let freight={
        if ship.collect{"착불".to_owned()}else{
        let values=["FACTORY","STORE"].into_iter().filter_map(|origin|ship.display_charges.get(origin)).map(|amount|amount.map(components::display_won).unwrap_or_else(||"—".into())).collect::<Vec<_>>();
        if values.is_empty(){"—".to_owned()}else{values.join(" / ")}
        }
    };
    shipment_line_with_freight(ui,name,&ship.products,chosen,previewed,Some(&freight),freight_emphasis)
}
fn group_frame(
    ui:&mut Ui,group:&GroupRow,selected:Option<&Selection>,
    open:bool,in_preview:bool,previewed_id:Option<&str>,writable:bool,
    freight_stage_group:Option<&str>,linked_selection:Option<(&str,&str)>,
    trigger:&mut Option<String>,hovered_row:&mut Option<String>,stage_emphasis:bool
)->Option<Event>{
    let mut event=None;
    let active=selected.is_some_and(|s|s.group_id==group.id);
    workflow_card(ui,active && !in_preview,in_preview,|ui|{
        let (top,bottom)=workflow_compact_rows(ui);
        let btn=workflow_header(
            ui,top,&group.id,&group.dealer_name,open || in_preview,
            Some(group.step),in_preview,
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
            "부가세 일부 포함"
        }else if group.vat_included>0{
            "부가세 포함"
        }else{"부가세 별도"};
        let (advance_clicked,stage_hovered,undo_clicked)=workflow_action_line(
            ui,bottom,&group.id,group.shipments.len(),group.total,
            if group.vat_included>0{Some(vat)}else{None},
            Some(stage_name(group.step,group.needs_notice)),
            (writable || freight_stage_group==Some(group.id.as_str())) && !group.transferred,
            writable && group.step>0 && !group.transferred,in_preview,stage_emphasis && group.step>=3,None
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
                let label=row(ui,ship,chosen,previewed,stage_emphasis && group.step==1);
                let response=ui.interact(
                    label.rect,Id::new(("workflow-shipment",ship.id.as_str(),in_preview)),
                    egui::Sense::click()
                );
                if in_preview && response.hovered(){
                    *hovered_row=Some(ship.id.clone());
                }
                components::workflow_shipment_hover(ui,&response,chosen,previewed);
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
fn pending_dealer_title<'a>(
    draft:&'a WorkingReception,names:&'a HashMap<String,String>,
    active_selection:Option<(&str,&'a str)>
)->&'a str{
    let Some(client_id)=draft.client_id.as_deref() else{
        return "미확인 거래처";
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
}

/// A selected dealer is not enough to approve an order. The quote must belong
/// to this exact source/client and include the native quote structures. The
/// first stage does not require freight settlement; that is a later step.
fn pending_quote_ready(draft:&WorkingReception)->bool{
    let Some(client_id)=draft.client_id.as_deref()
        .filter(|id|!id.trim().is_empty()) else{return false;};
    let source_ready=if draft.raw.trim().is_empty(){
        draft.manual_model.as_ref().is_some_and(|model|
            model.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT") &&
            model.get("sourceRaw").and_then(serde_json::Value::as_str)==Some("")
        )
    }else{
        draft.analysis_raw.as_deref()==Some(draft.raw.as_str()) &&
            draft.analysis_evidence.is_some()
    };
    let Some(quote)=draft.pricing_preview.as_ref() else{return false;};
    source_ready &&
        quote.get("orderId").and_then(serde_json::Value::as_str)==Some(draft.id.as_str()) &&
        quote.get("sourceRaw").and_then(serde_json::Value::as_str)==Some(draft.raw.as_str()) &&
        quote.get("orderType").and_then(serde_json::Value::as_str)==Some(draft.order_type.as_str()) &&
        quote.get("clientId").and_then(serde_json::Value::as_str)==Some(client_id) &&
        quote.get("authorityRevision").and_then(serde_json::Value::as_u64).is_some() &&
        quote.get("linePrices").and_then(serde_json::Value::as_array).is_some() &&
        quote.get("freightLines").and_then(serde_json::Value::as_array).is_some() &&
        quote.get("pendingRecipients").and_then(serde_json::Value::as_array).is_some()
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

/// Render the stable action label independently of calculation readiness.
fn pending_button_text(has_client:bool,retail:bool)->Option<&'static str>{
    if !has_client{Some("거래처 확인")}
    else if retail{None}
    else{Some("주문 확인 완료")}
}

/// Show only explicit recipient VAT settings in a current native quote.
/// Missing/null override is not evidence that VAT is included or excluded.
fn pending_vat_label(rows:&[serde_json::Value],quote_ready:bool)->Option<&'static str>{
    if !quote_ready || rows.is_empty(){return None;}
    let mut included=0;
    for row in rows{
        if row.get("vatOverride")?.as_bool()?{included+=1;}
    }
    Some(if included==rows.len(){"부가세 포함"}
        else if included>0{"부가세 일부 포함"}
        else{"부가세 별도"})
}

pub(crate) fn pending_recipients(draft:&WorkingReception)->&[serde_json::Value]{
    // A corrected/manual working receipt is the active view. The first
    // OCR recognition remains immutable audit evidence, not the display
    // source for recipient IDs or product quantities.
    if draft.raw.trim().is_empty() &&
        draft.manual_model.as_ref().is_some_and(|model|
            model.get("mode").and_then(serde_json::Value::as_str)==Some("DIRECT")
        ){
        draft.manual_model.as_ref()
            .and_then(|model|model.pointer("/model/rows"))
            .and_then(|value|value.as_array())
            .map(Vec::as_slice).unwrap_or(&[])
    }else if draft.analysis_raw.as_deref()==Some(draft.raw.as_str()){
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
    }else{&[]}
}

fn pending_frame(
    ui:&mut Ui,draft:&WorkingReception,selected:bool,open:bool,in_preview:bool,
    writable:bool,dealer_name:&str,committed_recipient:Option<&str>,
    preview:Option<&PendingTarget>,linked_selection:Option<(&str,&str)>,
    hovered_pending:&mut Option<PendingTarget>,stage_trigger:&mut Option<String>
)->Option<Event>{
    let mut event=None;
    workflow_card(ui,selected && !in_preview,in_preview,|ui|{
        let (top,bottom)=workflow_compact_rows(ui);
        let has_client=draft.client_id.as_deref()
            .is_some_and(|id|!id.trim().is_empty());
        let header=workflow_header(
            ui,top,&draft.id,dealer_name,open,has_client.then_some(0),in_preview,true,true
        );
        if header.manage{event=Some(Event::ManagePending(draft.id.clone()));}
        if header.delete{event=Some(Event::DeletePending(draft.id.clone()));}
        if header.title.hovered() {
            *hovered_pending=Some(PendingTarget::order(&draft.id));
        }
        if header.title.clicked(){
            event=Some(if selected{Event::TogglePending}
                else{Event::SelectPending(draft.id.clone())});
        }
        let recipients=pending_recipients(draft);
        let quote_ready=pending_quote_ready(draft);
        let quote=draft.pricing_preview.as_ref();
        let total=if quote_ready{
            quote.and_then(|value|value.get("total"))
                .and_then(serde_json::Value::as_i64)
        }else{None};
        let retail=draft.order_type=="RETAIL";
        let button_text=pending_button_text(has_client,retail);
        let vat=pending_vat_label(recipients,quote_ready);
        // A valid selected-client quote exposes the explicit first stage even
        // when freight/total is pending. Rust blocks only the issues assigned
        // to this stage; freight and payment remain later workflow steps.
        let (confirm_clicked,stage_hovered,_)=workflow_action_line(
            ui,bottom,&draft.id,recipients.len(),total,vat,
            button_text,quote_ready && writable && !retail,
            false,in_preview,false,(has_client && retail).then_some("소매 발주서")
        );
        if stage_hovered && quote_ready && draft.order_type!="RETAIL"{
            *stage_trigger=Some(format!("pending-stage:{}",draft.id));
            *hovered_pending=Some(PendingTarget::order(&draft.id));
        }
        if confirm_clicked{
            event=Some(Event::ConfirmDraft(draft.id.clone()));
        }
        if open{
            workflow_details(ui,|ui|{
                components::soft_divider(ui);
                ui.add_space(theme::SPACE_4);
            if recipients.is_empty(){
                let excerpt=draft.raw.lines().find(|s|!s.trim().is_empty())
                    .unwrap_or("새 발주서");
                let summary=format!("원문: {excerpt}");
                let response=components::workflow_shipment_row(
                    ui,"수취인 확인",&summary,shipment_row_fill(false,false),
                    false,false,None,false
                );
                let action=ui.interact(response.rect,
                    Id::new(("pending-reception-row",draft.id.as_str(),0usize,in_preview)),
                    egui::Sense::click());
                components::workflow_shipment_hover(ui,&action,false,false);
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
                        .into_iter().flatten().map(crate::workflow_board::product_label)
                        .collect::<Vec<_>>();
                    let recipient_id=crate::gateway::rules::scope_reception_edit_ids(
                        &draft.id,recipient.get("id")
                            .and_then(|value|value.as_str()).unwrap_or(""),None).0;
                    let recipient_id=recipient_id.as_str();
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
                    components::workflow_shipment_hover(ui,&action,chosen,previewed);
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
/// Compact cards have an exact fixed extent, including the frame stroke.
fn compact_card_height()->f32{
    theme::WORKFLOW_TITLE_HEIGHT+theme::WORKFLOW_BAND_DIVIDER_HEIGHT+
        theme::WORKFLOW_BOTTOM_HEIGHT+2.0*theme::SOFT_DIVIDER_WIDTH
}
fn compact_card_visible(top:f32,clip:Rect)->bool{
    // Keep one card of overscan on both sides of the viewport.
    let overscan=compact_card_height()+theme::WORKFLOW_CARD_GAP;
    top+compact_card_height()>=clip.top()-overscan && top<=clip.bottom()+overscan
}
fn reserve_compact_card(ui:&mut Ui){
    // Reserve the exact same width and height as workflow_card's compact
    // frame. No buttons, text shaping, recipient projection or hover tests.
    ui.allocate_space(Vec2::new(ui.available_width(),compact_card_height()));
    ui.add_space(theme::WORKFLOW_CARD_GAP);
}

/// Keep the relative order of already-visible cards. Insert new keys at
/// their ordinary neighbors without moving the slot of a confirmed draft.
fn sync_card_order(order:&mut Vec<String>,normal:&[String]){
    let valid:HashSet<&str>=normal.iter().map(String::as_str).collect();
    order.retain(|id|valid.contains(id.as_str()));
    let mut known:HashSet<String>=order.iter().cloned().collect();
    for (i,key) in normal.iter().enumerate(){
        if !known.insert(key.clone()){continue;}
        let next=normal[i+1..].iter().find_map(|id|order.iter().position(|s|s==id));
        order.insert(next.unwrap_or(order.len()),key.clone());
    }
}

pub fn show(
    ui:&mut Ui,
    snapshot:Option<&BoardSnapshot>,
    drafts:&[WorkingReception],active_id:Option<&str>,
    client_names:&HashMap<String,String>,
    active_selected_client:Option<(&str,&str)>,
    state:&mut BoardUi,writable:bool,freight_stage_group:Option<&str>,
    allow_pending_hover_switch:bool,
    active_recipient_id:Option<&str>,linked_selection:Option<(&str,&str)>
)->Option<Event>{
    // Prevent implicit vertical item spacing from doubling the explicit
    // WORKFLOW_CARD_GAP between order bundles. This is scoped to the list
    // inside its own ScrollArea; workflow_card restores internal spacing.
    ui.spacing_mut().item_spacing.y=0.0;
    let previews_allowed=allow_pending_hover_switch
        && crate::overlay::background_preview_allowed(ui.ctx());
    if !previews_allowed{state.stop_preview();}
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
    let groups:HashMap<&str,&GroupRow>=snapshot.into_iter()
        .flat_map(|b|b.groups.iter()).map(|g|(g.id.as_str(),g)).collect();
    let confirmed:HashSet<&str>=groups.values().flat_map(|g|g.shipments.iter())
        .map(|s|s.order_id.as_str()).collect();
    let pending:HashMap<String,&WorkingReception>=drafts.iter()
        .filter(|d|!confirmed.contains(d.id.as_str()))
        .map(|d|(format!("group:{}",d.id),d)).collect();
    let normal:Vec<String>=snapshot.into_iter().flat_map(|b|b.groups.iter())
        .map(|g|g.id.clone()).chain(drafts.iter().rev()
            .filter(|d|!confirmed.contains(d.id.as_str()))
            .map(|d|format!("group:{}",d.id))).collect();
    sync_card_order(&mut state.card_order,&normal);
    let card_order=state.card_order.clone();
    for slot in &card_order{
        if let Some(&group)=groups.get(slot.as_str()){
            let selected=current.is_some_and(|s|s.group_id==group.id);
            let open=selected && state.folded.as_deref()!=Some(group.id.as_str());
            let width=ui.available_width();
            let pos=ui.cursor().min;
            // Expanded and hover-owned cards must retain their full geometry
            // and overlay anchor, even when scrolling past the viewport.
            if !open && state.active_preview()!=Some(group.id.as_str()) &&
                !compact_card_visible(pos.y,ui.clip_rect()){
                reserve_compact_card(ui);
                continue;
            }
            let mut ignored_row=None;
            if let Some(value)=group_frame(ui,group,current,open,false,None,
                writable,freight_stage_group,linked_selection,&mut trigger,&mut ignored_row,
                state.active_preview()==Some(group.id.as_str())){
                match value{
                    Event::ToggleGroup(id)=>{
                        state.folded=if open{Some(id)}else{None};
                    }
                    other=>event=Some(other),
                }
            }
            if state.active_preview()==Some(group.id.as_str()) && !open{
                overlay=Some((group,pos,width));
            }
            ui.add_space(theme::WORKFLOW_CARD_GAP);
            continue;
        }
        let Some(&draft)=pending.get(slot)else{continue;};
        let selected=state.selection.is_none() && active_id==Some(draft.id.as_str());
        let key=format!("pending:{}",draft.id);
        let open=selected && state.folded.as_deref()!=Some(key.as_str());
        let preview_owned=previews_allowed && !selected &&
            state.pending_visible.as_ref().is_some_and(|target|target.order_id==draft.id);
        if !open && !preview_owned && !compact_card_visible(ui.cursor().min.y,ui.clip_rect()){
            reserve_compact_card(ui);
            continue;
        }
        let name=pending_dealer_title(
            draft,client_names,
            if selected{active_selected_client}else{None}
        );
        if preview_owned{
            pending_overlay=Some((draft,name,ui.cursor().min,
                ui.available_width()));
        }
        if let Some(value)=pending_frame(
            ui,draft,selected,open,false,writable,name,
            if selected{active_recipient_id}else{None},
            state.pending_visible.as_ref(),
            linked_selection,
            &mut hovered_pending,&mut trigger
        ){
            match value{
                Event::TogglePending=>{
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
        let area=crate::overlay::area(Id::new(("pending-card-overlay",draft.id.clone())),crate::overlay::AreaRole::Preview)
            .fixed_pos(Pos2::new(pos.x,pos.y))
            .movable(false);
        let response=area.show(ui.ctx(),|overlay_ui|{
            overlay_ui.set_clip_rect(pane_clip);
            overlay_ui.set_min_width(width);
            overlay_ui.set_max_width(width);
            overlay_event=pending_frame(
                overlay_ui,draft,false,true,true,writable,name,None,
                state.pending_visible.as_ref(),linked_selection,&mut overlay_hover,&mut trigger
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
    if previews_allowed && event.is_none(){
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
    if previews_allowed{state.update_hover(hovered,ui.ctx());}
    else{state.stop_preview();}
    state.pending_stage_target=state.active_pending_stage().map(PendingTarget::order);
    if let Some((group,pos,width))=overlay{
        let pane_clip=ui.clip_rect();
        let mut overlay_event=None;
        let mut hovered_row=None;
        let area=crate::overlay::area(Id::new(("workflow-card-overlay",group.id.clone())),crate::overlay::AreaRole::Preview)
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
                ui,group,current,true,true,state.preview_shipment.as_deref(),
                writable,freight_stage_group,linked_selection,&mut ignored_hover,&mut hovered_row,
                state.active_preview()==Some(group.id.as_str())
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
    fn shipment_freight_hover_does_not_resize_row(){
        let ctx=egui::Context::default();
        let mut measured=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.set_width(390.0);
                for emphasized in [false,true]{
                    let response=shipment_line_with_freight(ui,"수취인",&["상품명".into()],false,false,Some("4,000원"),emphasized);
                    measured.push(response.rect.size());
                }
            });
        });
        assert_eq!(measured.len(),2);
        assert_eq!(measured[0],measured[1]);
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
        assert_eq!(theme::WORKFLOW_DETAIL_ROW_BG,theme::WORKFLOW_CARD_BG);
    }
}

#[cfg(test)]
mod pending_matching_tests {
    use super::*;

    #[test]
    fn pending_labels_are_stable_and_vat_is_never_inferred_from_amount(){
        use serde_json::json;
        assert_eq!(pending_button_text(false,false),Some("거래처 확인"));
        assert_eq!(pending_button_text(true,false),Some("주문 확인 완료"));
        assert_eq!(pending_button_text(true,true),None);
        let yes=json!({"vatOverride":true});
        let no=json!({"vatOverride":false});
        let unknown=json!({"vatOverride":null});
        assert_eq!(pending_vat_label(&[yes.clone()],false),None);
        assert_eq!(pending_vat_label(&[],true),None);
        assert_eq!(pending_vat_label(&[yes.clone()],true),Some("부가세 포함"));
        assert_eq!(pending_vat_label(&[yes.clone(),no.clone()],true),Some("부가세 일부 포함"));
        assert_eq!(pending_vat_label(&[no],true),Some("부가세 별도"));
        assert_eq!(pending_vat_label(&[yes,unknown],true),None);
    }

    #[test]
    fn unconfirmed_dealer_has_four_gray_lamps(){
        let pending=(theme::WORKFLOW_SIGNAL_PENDING,theme::WORKFLOW_SIGNAL_PENDING_BORDER);
        let current=(theme::WORKFLOW_SIGNAL_CURRENT,theme::WORKFLOW_SIGNAL_CURRENT_BORDER);
        let done=(theme::WORKFLOW_SIGNAL_DONE,theme::WORKFLOW_SIGNAL_DONE_BORDER);
        for index in 0..STEP_NAMES.len(){
            assert_eq!(workflow_signal_palette(None,index),pending);
            assert_eq!(workflow_signal_palette(Some(0),index),
                if index==0{current}else{pending});
            assert_eq!(workflow_signal_palette(Some(1),index),
                if index==0{done}else if index==1{current}else{pending});
        }
    }

    #[test]
    fn confirmation_keeps_the_pending_card_slot(){
        let mut order=Vec::new();
        sync_card_order(&mut order,&["group:old".into(),"group:d2".into(),"group:d1".into()]);
        // d1 now belongs to the confirmed board, normally sorted ahead of d2.
        sync_card_order(&mut order,&["group:d1".into(),"group:old".into(),"group:d2".into()]);
        assert_eq!(order,vec!["group:old","group:d2","group:d1"]);
        sync_card_order(&mut order,&["group:d1".into(),"group:d2".into()]);
        assert_eq!(order,vec!["group:d2","group:d1"]);
    }

    #[test]
    fn pending_receipt_never_emits_a_fake_dealer_confirmation_action(){
        let ctx=components::test_context();
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
                    ui,&draft,false,false,false,false,"확인된 거래처",None,None,
                    None,&mut hovered,&mut None
                );
            });
        });
        assert!(tapped.is_none());
        // Card and management dialog must never display stale recognition.
        let mut changed=draft.clone();
        changed.analysis_raw=Some("이전 원문".to_owned());
        changed.analysis_evidence=Some(serde_json::json!({"recipients":[{"name":"이전 수취인"}]}));
        assert!(pending_recipients(&changed).is_empty());
        changed.analysis_raw=Some(changed.raw.clone());
        changed.manual_model=Some(serde_json::json!({"model":{"rows":[{"name":"수정한 수취인"}]}}));
        assert_eq!(pending_recipients(&changed)[0]["name"].as_str(),Some("수정한 수취인"));
        changed.raw.clear();
        changed.manual_model=Some(serde_json::json!({"mode":"DIRECT","model":{"rows":[{"name":"직접 입력"}]}}));
        assert_eq!(pending_recipients(&changed)[0]["name"].as_str(),Some("직접 입력"));
    }

    #[test]
    fn primary_release_fallback_recovers_a_click_on_the_topmost_button_layer(){
        let ctx=egui::Context::default();
        let pos=Pos2::new(120.0,100.0);
        let rect=Rect::from_center_size(pos,Vec2::new(80.0,30.0));
        let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(400.0,300.0));
        let mut press=egui::RawInput::default();
        press.screen_rect=Some(screen);
        press.events=vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton{
                pos,button:egui::PointerButton::Primary,pressed:true,
                modifiers:egui::Modifiers::default()
            }
        ];
        let _=ctx.run(press,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let _=primary_release_inside(ui,rect);
            });
        });
        let mut release=egui::RawInput::default();
        release.screen_rect=Some(screen);
        release.events=vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton{
                pos,button:egui::PointerButton::Primary,pressed:false,
                modifiers:egui::Modifiers::default()
            }
        ];
        let mut detected=false;
        let _=ctx.run(release,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                detected=primary_release_inside(ui,rect);
            });
        });
        assert!(detected,"a primary-button release inside the active control layer must be recovered");
    }

    #[test]
    fn selected_dealer_exposes_first_stage_without_waiting_for_freight_total(){
        let raw="테스트 주문";
        let mut draft=WorkingReception{
            id:"egui-pending-test".to_owned(),
            raw:raw.to_owned(),
            order_type:"AGENCY".to_owned(),
            source_input_method:None,
            last_input_method:None,
            analysis_raw:Some(raw.to_owned()),
            analysis_evidence:Some(serde_json::json!({"recipients":[{}]})),
            analysis_view:None,
            client_match:None,
            chat_handle:None,
            chat_title:None,
            client_id:Some("verified-client".to_owned()),
            client_selection:Some("MANUAL".to_owned()),
            pricing_preview:Some(serde_json::json!({
                "orderId":"egui-pending-test",
                "sourceRaw":raw,
                "orderType":"AGENCY",
                "clientId":"verified-client",
                "authorityRevision":7,
                "total":33000,
                "linePrices":[{"amount":30000}],
                "freightLines":[{"billableCharge":3000}],
                "pendingRecipients":[]
            })),
            working_version:None,
            working_synced:false,
            manual_model:None,
            edit_history:Vec::new()
        };
        assert!(pending_quote_ready(&draft));
        draft.pricing_preview.as_mut().unwrap()["total"]=serde_json::Value::Null;
        draft.pricing_preview.as_mut().unwrap()["pendingRecipients"]=
            serde_json::json!([{"recipientId":"r1","freight":true}]);
        assert!(pending_quote_ready(&draft),
            "freight settlement is not a prerequisite for first-stage confirmation");

        draft.pricing_preview.as_mut().unwrap()["sourceRaw"]=
            serde_json::json!("다른 원문");
        assert!(!pending_quote_ready(&draft));
        draft.pricing_preview.as_mut().unwrap()["sourceRaw"]=serde_json::json!(raw);
        draft.pricing_preview.as_mut().unwrap()["clientId"]=
            serde_json::json!("another-client");
        assert!(!pending_quote_ready(&draft));
        draft.pricing_preview.as_mut().unwrap()["clientId"]=
            serde_json::json!("verified-client");
        draft.pricing_preview.as_mut().unwrap()["freightLines"]=serde_json::Value::Null;
        assert!(!pending_quote_ready(&draft));
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
    fn hovering_committed_recipient_still_projects_evidence_without_reselecting(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let committed=PendingTarget::recipient("order-a","kim");
        let other=PendingTarget::recipient("order-a","pyo");
        // A different recipient has an ordinary temporary preview.
        board.pending_order_hover=Some((
            other.clone(),Instant::now()-PENDING_ORDER_HOVER_ENTER
        ));
        board.update_pending_order_hover(Some(other.clone()),Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&other));
        // Returning to the selected row immediately clears Pyo's stale
        // preview, but waits the ordinary 250ms before highlighting Kim.
        board.update_pending_order_hover(Some(committed.clone()),Some(&committed),&ctx);
        assert!(board.pending_preview().is_none());
        assert_eq!(
            board.pending_order_hover.as_ref().map(|(target,_)|target),
            Some(&committed)
        );
        board.pending_order_hover=Some((
            committed.clone(),Instant::now()-PENDING_ORDER_HOVER_ENTER
        ));
        board.update_pending_order_hover(Some(committed.clone()),Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&committed));
        assert!(board.selection.is_none(),
            "hover must never alter committed selection");
        // A selected recipient can still generate its source span highlight.
        assert_eq!(visible_pending_recipient(
            "order-a",board.pending_preview(),None
        ),Some("kim"));
        board.update_pending_order_hover(None,Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&committed));
        board.pending_release=Some(Instant::now()-Duration::from_millis(1));
        board.update_pending_order_hover(None,Some(&committed),&ctx);
        assert!(board.pending_preview().is_none());
    }

    #[test]
    fn crossing_from_other_recipient_to_selected_drops_stale_preview_immediately(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let selected=PendingTarget::recipient("order","kim");
        let other=PendingTarget::recipient("order","pyo");
        board.pending_visible=Some(other);
        // Kim is already selected, but still owns a real hover target.
        board.update_pending_order_hover(Some(selected.clone()),Some(&selected),&ctx);
        assert!(board.pending_preview().is_none(),
            "Pyo must stop highlighting as soon as Kim is under the pointer");
        assert_eq!(board.pending_order_hover.as_ref().map(|(target,_)|target),
            Some(&selected));
        board.pending_order_hover=Some((
            selected.clone(),Instant::now()-PENDING_ORDER_HOVER_ENTER
        ));
        board.update_pending_order_hover(Some(selected.clone()),Some(&selected),&ctx);
        assert_eq!(board.pending_preview(),Some(&selected));
        // The selected recipient is an external committed target, not
        // changed by any of these transient board transitions.
        assert!(board.selection.is_none());
    }

    #[test]
    fn rapid_alternating_recipient_hover_never_activates_a_superseded_target(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let committed=PendingTarget::recipient("order","kim");
        let other=PendingTarget::recipient("order","pyo");
        for i in 0..100{
            let incoming=if i%2==0{&committed}else{&other};
            board.update_pending_order_hover(Some((*incoming).clone()),Some(&committed),&ctx);
            assert!(board.pending_preview().is_none(),
                "fast mouse transitions should keep the original selection visible");
            assert_eq!(
                board.pending_order_hover.as_ref().map(|(target,_)|target),
                Some(incoming)
            );
        }
        // Only the stable final target is allowed to enter after 250ms.
        board.pending_order_hover=Some((
            other.clone(),Instant::now()-PENDING_ORDER_HOVER_ENTER
        ));
        board.update_pending_order_hover(Some(other.clone()),Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&other));
        board.update_pending_order_hover(Some(committed.clone()),Some(&committed),&ctx);
        assert!(board.pending_preview().is_none());
        assert_eq!(board.pending_order_hover.as_ref().map(|(target,_)|target),
            Some(&committed));
    }

    #[test]
    fn brief_pointer_gap_preserves_preview_but_new_row_cancels_old_exit_grace(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let committed=PendingTarget::recipient("order","kim");
        let other=PendingTarget::recipient("order","pyo");
        board.pending_visible=Some(other.clone());
        board.update_pending_order_hover(None,Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&other));
        assert!(board.pending_release.is_some());
        // Crossing to Kim clears Pyo and cancels its release timer.
        board.update_pending_order_hover(Some(committed.clone()),Some(&committed),&ctx);
        assert!(board.pending_preview().is_none());
        assert!(board.pending_release.is_none());
        assert_eq!(board.pending_order_hover.as_ref().map(|(target,_)|target),
            Some(&committed));
    }

    #[test]
    fn reentering_same_recipient_within_release_grace_does_not_flicker(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let committed=PendingTarget::recipient("order","kim");
        let other=PendingTarget::recipient("order","pyo");
        board.pending_visible=Some(other.clone());
        board.update_pending_order_hover(None,Some(&committed),&ctx);
        assert!(board.pending_release.is_some());
        board.update_pending_order_hover(Some(other.clone()),Some(&committed),&ctx);
        assert_eq!(board.pending_preview(),Some(&other));
        assert!(board.pending_release.is_none());
        assert!(board.pending_order_hover.is_none());
    }

    #[test]
    fn same_recipient_id_in_another_order_never_inherits_hover_preview(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let first=PendingTarget::recipient("order-a","kim");
        let second=PendingTarget::recipient("order-b","kim");
        board.pending_visible=Some(first);
        board.update_pending_order_hover(Some(second.clone()),None,&ctx);
        assert!(board.pending_preview().is_none());
        assert_eq!(board.pending_order_hover.as_ref().map(|(target,_)|target),
            Some(&second));
        assert_eq!(visible_pending_recipient(
            "order-a",board.pending_preview(),None
        ),None);
    }

    #[test]
    fn deterministic_rapid_hover_respects_each_250ms_boundary_and_keeps_click(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let selected=PendingTarget::recipient("order","kim");
        let other=PendingTarget::recipient("order","pyo");
        let t0=Instant::now();
        let tick=|ms|t0+Duration::from_millis(ms);

        board.update_pending_order_hover_at(
            Some(selected.clone()),Some(&selected),&ctx,tick(0));
        assert!(board.pending_preview().is_none());
        board.update_pending_order_hover_at(
            Some(other.clone()),Some(&selected),&ctx,tick(100));
        board.update_pending_order_hover_at(
            Some(selected.clone()),Some(&selected),&ctx,tick(200));
        board.update_pending_order_hover_at(
            Some(other.clone()),Some(&selected),&ctx,tick(240));
        board.update_pending_order_hover_at(
            Some(other.clone()),Some(&selected),&ctx,tick(489));
        assert!(board.pending_preview().is_none(),"249ms cannot activate Pyo");

        board.update_pending_order_hover_at(
            Some(other.clone()),Some(&selected),&ctx,tick(490));
        assert_eq!(board.pending_preview(),Some(&other));
        board.update_pending_order_hover_at(
            Some(selected.clone()),Some(&selected),&ctx,tick(500));
        assert!(board.pending_preview().is_none(),
            "entering Kim must instantly remove Pyo's blue highlight");
        board.update_pending_order_hover_at(
            Some(selected.clone()),Some(&selected),&ctx,tick(749));
        assert!(board.pending_preview().is_none(),"249ms cannot activate Kim");
        board.update_pending_order_hover_at(
            Some(selected.clone()),Some(&selected),&ctx,tick(750));
        assert_eq!(board.pending_preview(),Some(&selected));

        board.update_pending_order_hover_at(None,Some(&selected),&ctx,tick(760));
        board.update_pending_order_hover_at(None,Some(&selected),&ctx,tick(1009));
        assert_eq!(board.pending_preview(),Some(&selected),
            "a pointer gap shorter than 250ms retains the preview");
        board.update_pending_order_hover_at(None,Some(&selected),&ctx,tick(1010));
        assert!(board.pending_preview().is_none());
        assert_eq!(visible_pending_recipient(
            "order",board.pending_preview(),None
        ),None);
        assert!(board.selection.is_none(),
            "rapid hover cannot commit a different recipient");
    }

    #[test]
    fn hovered_header_keeps_selected_order_but_clears_other_preview(){
        let ctx=egui::Context::default();
        let mut board=BoardUi::default();
        let committed=PendingTarget::recipient("order-a","kim");
        board.pending_visible=Some(PendingTarget::recipient("order-a","pyo"));
        board.update_pending_order_hover(
            Some(PendingTarget::order("order-a")),Some(&committed),&ctx
        );
        assert!(board.pending_preview().is_none());
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
mod linked_highlight_palette_tests {
    use super::*;
    #[test]
    fn preview_row_matches_linked_source_recipient_and_item_palette() {
        assert_eq!(shipment_row_fill(false,true),source_highlight::ACTIVE_FILL);
        assert_ne!(shipment_row_fill(true,true),source_highlight::ACTIVE_FILL);
        assert_eq!(shipment_row_fill(false,false),theme::WORKFLOW_DETAIL_ROW_BG);
        assert_eq!(source_highlight::ACTIVE_BORDER_WIDTH,2.0);
    }

    #[test]
    fn shared_selection_palette_preserves_selection_geometry_and_white_buttons() {
        use egui::Color32;
        assert_eq!(theme::SELECTED_ORDER,Color32::from_rgb(245,158,11));
        assert_eq!(theme::WORKFLOW_SELECTED_BORDER,theme::SELECTED_ORDER);
        assert_eq!(theme::WORKFLOW_SELECTED_STRIPE,2.0);
        assert_eq!(theme::WORKFLOW_SELECTED_BG,theme::WORKFLOW_CARD_BG);
        assert_eq!(theme::WORKFLOW_CARD_BG,theme::INPUT);
        assert_ne!(theme::WORKFLOW_CARD_BG,theme::PANEL);
        assert_eq!(theme::CANVAS,Color32::from_rgb(16,16,16));
        assert_eq!(theme::PANEL,Color32::from_rgb(24,24,24));
        assert_eq!(theme::INPUT,Color32::from_rgb(34,34,34));
        assert_eq!(theme::RAISED,Color32::from_rgb(41,41,41));
        assert_eq!(shipment_row_fill(true,false),Color32::from_rgb(48,42,34));
        let selected=theme::workflow_shipment_row_frame(shipment_row_fill(true,false),true);
        let normal=theme::workflow_shipment_row_frame(shipment_row_fill(false,false),false);
        assert_eq!(selected.fill,Color32::from_rgb(48,42,34));
        assert_eq!(selected.stroke.color,Color32::TRANSPARENT);
        assert_eq!(normal.stroke.color,Color32::TRANSPARENT);
        assert_eq!(selected.stroke.width,normal.stroke.width);
        assert_eq!(selected.inner_margin,normal.inner_margin);
        assert_eq!(selected.corner_radius,normal.corner_radius);
        assert_eq!(theme::WORKFLOW_SELECTED_ROW_TEXT,Color32::from_rgb(244,214,162));
        assert_eq!(shipment_row_fill(true,true),shipment_row_fill(true,false));
        assert_eq!(theme::ReviewTone::Action.palette(),(
            theme::PANEL,theme::REQUIRED_REVIEW_BORDER,
            theme::REQUIRED_REVIEW_TEXT,Color32::from_rgb(217,188,152)
        ));
        assert_eq!(theme::MANUAL_SOURCE_UNASSIGNED_FILL,Color32::from_rgb(214,214,214));
        assert_eq!(theme::BRIGHT,Color32::WHITE);
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

#[cfg(test)]
mod compact_virtualization_tests{
    use super::*;
    #[test]
    fn compact_placeholder_preserves_exact_card_scroll_extent(){
        let ctx=egui::Context::default();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.spacing_mut().item_spacing.y=0.0;
                let top=ui.cursor().min.y;
                workflow_card(ui,false,false,|card|{workflow_compact_rows(card);});
                ui.add_space(theme::WORKFLOW_CARD_GAP);
                let rendered=ui.cursor().min.y-top;
                let next=ui.cursor().min.y;
                reserve_compact_card(ui);
                let placeholder=ui.cursor().min.y-next;
                assert!((rendered-placeholder).abs()<=0.5,"rendered {rendered}, placeholder {placeholder}");
            });
        });
    }
    #[test]
    fn compact_visibility_keeps_overscan_and_skips_distant_cards(){
        let clip=Rect::from_min_max(Pos2::new(0.0,1000.0),Pos2::new(400.0,1400.0));
        assert!(compact_card_visible(1000.0,clip));
        assert!(compact_card_visible(950.0,clip));
        assert!(compact_card_visible(1420.0,clip));
        assert!(!compact_card_visible(0.0,clip));
        assert!(!compact_card_visible(1700.0,clip));
    }
}
