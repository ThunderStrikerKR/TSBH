//! The right-hand workflow board. Layout/hover/selection only; no business rules.
//! Native Rust provides groups, amounts, freight, stage and shipment identity.
use eframe::egui::{self, Align, Frame, Id, Layout, Pos2, Rect, Ui, Vec2};
use std::time::{Duration, Instant};
use crate::{components, preview_store::WorkingReception, theme, typography};
use crate::workflow_board::{BoardSnapshot, GroupRow, ShipmentRow};

const HOVER_ENTER:Duration=Duration::from_millis(250);
const HOVER_EXIT:Duration=Duration::from_millis(250);
const STEP_NAMES:[&str;4]=["주문 확인","운임 적용","계좌 안내","입금 확인"];

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Selection {
    pub group_id:String,
    pub order_id:String,
    pub shipment_id:String,
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
}
pub enum Event {
    SelectPending(String),
    SelectShipment(Selection),
    ToggleGroup(String),
    TogglePending(String),
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
    }
    pub fn select(&mut self,selection:Selection){
        self.selection=Some(selection);
        self.folded=None;
        self.stop_preview();
    }
    pub fn active_preview(&self)->Option<&str>{
        self.preview_active.as_deref()
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
/// React's .workflow-group is a full-width 7px-radius card, not a
/// shrink-to-content nested Frame or a blue navigation-button tile.
/// Reserve exactly the pane width for BOTH expanded and folded cards.
fn workflow_card<R>(
    ui:&mut Ui,selected:bool,preview:bool,body:impl FnOnce(&mut Ui)->R
)->R{
    let outer=ui.available_width();
    let frame=Frame::new()
        .fill(if selected || preview{theme::WORKFLOW_SELECTED_BG}else{theme::WORKFLOW_CARD_BG})
        .stroke(egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,
            theme::WORKFLOW_CARD_BORDER))
        .corner_radius(egui::CornerRadius::same(theme::WORKFLOW_CARD_RADIUS))
        .inner_margin(egui::Margin::symmetric(
            theme::WORKFLOW_CARD_PADDING_X as i8,
            theme::WORKFLOW_CARD_PADDING_Y as i8
        ));
    let response=frame.show(ui,|card|{
        card.set_min_width(
            (outer-theme::WORKFLOW_CARD_PADDING_X*2.0
                -theme::SOFT_DIVIDER_WIDTH*2.0).max(0.0)
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
/// A clickable, left-aligned title occupying the same reserved header width
/// with the workflow lights; no highlighted pill around the dealer name.
fn group_heading(
    ui:&mut Ui,id:&str,title:&str,step:usize,notice:bool
)->egui::Response{
    // The original React title BUTTON includes both the company name and
    // signal lights. Do not restrict the click target to the text glyphs.
    let heading=ui.horizontal(|header|{
        header.spacing_mut().item_spacing.x=theme::SPACE_4;
        let title_width=(header.available_width()-theme::WORKFLOW_SIGNAL_WIDTH
            -theme::SPACE_4).max(0.0);
        header.add_sized(
            [title_width,theme::WORKFLOW_TITLE_HEIGHT],
            egui::Label::new(typography::section_heading(title)).truncate()
        );
        lights(header,step,notice);
    });
    ui.interact(heading.response.rect,Id::new(("workflow-header",id)),
        egui::Sense::click())
}
/// React header: title and lights occupy the flexible left slot;
/// its existing manage/delete controls retain their fixed right slots.
/// Until the native audited writer is available, preserve their visible
/// roles disabled rather than wiring destructive fake operations.
fn workflow_header(ui:&mut Ui,id:&str,title:&str,step:usize,notice:bool)
    ->egui::Response {
    let width=ui.available_width();
    let title_width=(width-theme::WORKFLOW_MANAGE_WIDTH
        -theme::WORKFLOW_TRASH_WIDTH-theme::SPACE_8).max(0.0);
    let mut title_response=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        bar.allocate_ui_with_layout(
            Vec2::new(title_width,theme::WORKFLOW_TITLE_HEIGHT),
            Layout::top_down(Align::Min),|left|{
                left.set_min_width(title_width);
                title_response=Some(group_heading(left,id,title,step,notice));
            }
        );
        bar.allocate_ui_with_layout(
            Vec2::new(theme::WORKFLOW_MANAGE_WIDTH,theme::WORKFLOW_TITLE_HEIGHT),
            Layout::top_down(Align::Min),|manage|{
                theme::action_full_width(manage,"묶음 관리",
                    theme::ButtonRole::Standard,false);
            }
        );
        // A small disabled trash control, drawn as line art as in React.
        // It must never delete a native workflow group without an audited
        // write + undo transaction.
        let (rect,_)=bar.allocate_exact_size(
            Vec2::new(theme::WORKFLOW_TRASH_WIDTH,
                theme::WORKFLOW_TITLE_HEIGHT),
            egui::Sense::hover(),
        );
        let paint=bar.painter();
        paint.rect_filled(rect,theme::CONTROL_RADIUS as f32,theme::RAISED);
        let c=rect.center();
        let ink=egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SECONDARY);
        paint.line_segment([
            Pos2::new(c.x-7.0,c.y-5.0),Pos2::new(c.x+7.0,c.y-5.0)
        ],ink);
        paint.line_segment([
            Pos2::new(c.x-4.0,c.y-8.0),Pos2::new(c.x+4.0,c.y-8.0)
        ],ink);
        for (x1,y1,x2,y2) in [
            (-5.0,-3.0,-4.0,8.0),(5.0,-3.0,4.0,8.0),
            (-4.0,8.0,4.0,8.0),(-1.5,-1.0,-1.5,5.0),
            (1.5,-1.0,1.5,5.0)
        ]{
            paint.line_segment([
                Pos2::new(c.x+x1,c.y+y1),Pos2::new(c.x+x2,c.y+y2)
            ],ink);
        }
    });
    title_response.expect("workflow header always renders a clickable title")
}
fn lights(ui:&mut Ui,step:usize,notice:bool){
    ui.horizontal(|row|{
        row.spacing_mut().item_spacing.x=theme::SPACE_4;
        for index in 0..STEP_NAMES.len(){
            let (rect,_)=row.allocate_exact_size(
                Vec2::splat(theme::SPACE_16),egui::Sense::hover()
            );
            let color=if index<step {theme::SUCCESS}
                else if index==step && notice {theme::SELECTED_ORDER}
                else if index==step {theme::SECONDARY}
                else{theme::DIVIDER};
            row.painter().circle_filled(rect.center(),theme::SPACE_4,color);
        }
    });
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
/// Original React workflow-action-line: a fixed summary slot on the left,
/// 140px primary action and a 34px back-stage slot on the right. The stage
/// action remains disabled until the audited Rust mutation is implemented,
/// but its 250ms read-only hover preview is still available.
fn workflow_action_line(
    ui:&mut Ui,id:&str,shipments:usize,total:Option<i64>,
    vat:Option<&str>,button_text:&str,enabled:bool
)->(bool,bool){
    let width=ui.available_width();
    let actions=theme::WORKFLOW_CONTROL_WIDTH
        +theme::WORKFLOW_TRASH_WIDTH+theme::SPACE_4;
    let summary_width=(width-actions-theme::SPACE_4).max(0.0);
    let mut stage_button=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        bar.allocate_ui_with_layout(
            Vec2::new(summary_width,theme::WORKFLOW_ACTION_ROW_HEIGHT),
            Layout::top_down(Align::Min),|summary|{
                summary.set_max_width(summary_width);
                summary.spacing_mut().item_spacing.y=0.0;
                summary.add_sized(
                    [summary_width,theme::SPACE_16],
                    egui::Label::new(typography::caption(
                        &format!("배송 {shipments}건")
                    )).truncate()
                );
                let detail=match total{
                    Some(amount)=>{
                        let amount=money(Some(amount));
                        if let Some(vat)=vat{
                            format!("{vat} · {amount}")
                        }else{amount}
                    }
                    None=>"금액 미확정".to_owned(),
                };
                summary.add_sized(
                    [summary_width,theme::SPACE_16],
                    egui::Label::new(typography::quantity(&detail)).truncate()
                );
            }
        );
        bar.allocate_ui_with_layout(
            Vec2::new(theme::WORKFLOW_CONTROL_WIDTH,
                theme::WORKFLOW_ACTION_ROW_HEIGHT),
            Layout::top_down(Align::Min),|control|{
                stage_button=Some(theme::action_full_width(
                    control,button_text,theme::ButtonRole::Primary,enabled
                ));
            }
        );
        bar.add_enabled(false,
            egui::Button::new("↶").min_size(Vec2::new(
                theme::WORKFLOW_TRASH_WIDTH,
                theme::WORKFLOW_ACTION_ROW_HEIGHT
            ))
        );
    });
    let response=stage_button.expect("workflow action occupies a fixed slot");
    let hovered=if enabled{
        response.hovered()
    }else{
        // A disabled save action must not block a non-mutating hover preview.
        ui.interact(response.rect,Id::new(("workflow-stage-hover",id)),
            egui::Sense::hover()).hovered()
    };
    (response.clicked(),hovered)
}
fn workflow_table_header(ui:&mut Ui){
    let width=ui.available_width();
    ui.horizontal(|columns|{
        columns.spacing_mut().item_spacing.x=theme::SPACE_4;
        columns.add_sized([
            theme::WORKFLOW_RECIPIENT_COL,theme::SPACE_24
        ],egui::Label::new(typography::table_header("수취인")));
        let products=(width-theme::WORKFLOW_RECIPIENT_COL
            -theme::WORKFLOW_FREIGHT_COL-theme::SPACE_8).max(0.0);
        columns.add_sized([
            products,theme::SPACE_24
        ],egui::Label::new(typography::table_header("상품")));
        columns.add_sized([
            theme::WORKFLOW_FREIGHT_COL,theme::SPACE_24
        ],egui::Label::new(typography::table_header("운임")));
    });
}
/// One product occupies a compact chip, exactly as the React
/// .workflow-shipment .product-chip style, not an unframed text fragment.
fn product_chip(ui:&mut Ui,label:&str){
    Frame::new()
        .fill(theme::WORKFLOW_CHIP_FILL)
        .stroke(egui::Stroke::new(
            theme::SOFT_DIVIDER_WIDTH,theme::WORKFLOW_CHIP_BORDER
        ))
        .corner_radius(theme::WORKFLOW_CHIP_RADIUS)
        .inner_margin(egui::Margin{
            left:theme::WORKFLOW_CHIP_PAD_X,
            right:theme::WORKFLOW_CHIP_PAD_X,
            top:theme::WORKFLOW_CHIP_PAD_Y,
            bottom:theme::WORKFLOW_CHIP_PAD_Y,
        }).show(ui,|chip|{
            chip.label(typography::table_cell(label));
        });
}
/// React .workflow-shipment uses a 64 / flexible / 68 three-column row.
/// No secondary boxed cards: selection is a subtle background plus a thin
/// accent stripe, not an extra input-style border around every recipient.
fn shipment_line(
    ui:&mut Ui,name:&str,products:&[String],freight:&str,
    chosen:bool,previewed:bool
)->egui::Response{
    let fill=if chosen{
        theme::WORKFLOW_SELECTED_BORDER.linear_multiply(0.20)
    }else if previewed{
        theme::SELECTION.linear_multiply(0.14)
    }else{egui::Color32::TRANSPARENT};
    let outer=ui.available_width();
    let response=Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(
            theme::SPACE_4 as i8,theme::SPACE_4 as i8
        ))
        .show(ui,|row|{
            let inside=(outer-theme::SPACE_8).max(0.0);
            row.set_min_width(inside);
            row.horizontal(|cols|{
                cols.spacing_mut().item_spacing.x=theme::SPACE_4;
                let name_width=theme::WORKFLOW_RECIPIENT_COL;
                let freight_width=theme::WORKFLOW_FREIGHT_COL;
                let product_width=(inside-name_width-freight_width
                    -theme::SPACE_8).max(0.0);
                cols.add_sized(
                    [name_width,theme::WORKFLOW_TITLE_HEIGHT],
                    egui::Label::new(typography::body(name)).truncate()
                );
                cols.allocate_ui_with_layout(
                    Vec2::new(product_width,theme::WORKFLOW_TITLE_HEIGHT),
                    Layout::top_down(Align::Min),|product_col|{
                        product_col.set_width(product_width);
                        product_col.horizontal_wrapped(|chips|{
                            chips.spacing_mut().item_spacing.x=theme::SPACE_4;
                            for product in products{
                                product_chip(chips,product);
                            }
                        });
                    }
                );
                cols.with_layout(Layout::right_to_left(Align::Center),|freight_col|{
                    freight_col.add_sized(
                        [freight_width,theme::WORKFLOW_TITLE_HEIGHT],
                        egui::Label::new(typography::quantity(freight)).truncate()
                    );
                });
            });
        });
    if chosen || previewed{
        let color=if chosen{theme::WORKFLOW_SELECTED_BORDER}else{theme::SELECTION};
        let rect=response.response.rect;
        ui.painter().rect_filled(Rect::from_min_max(
            Pos2::new(rect.left(),rect.top()),
            Pos2::new(rect.left()+theme::WORKFLOW_SELECTED_STRIPE,
                rect.bottom())
        ),0.0,color);
    }
    response.response
}
fn row(ui:&mut Ui,ship:&ShipmentRow,chosen:bool,in_preview:bool)->egui::Response{
    let name=if ship.recipient_name.is_empty(){
        "수취인 확인"
    }else{ship.recipient_name.as_str()};
    let freight=if ship.collect{"착불".to_owned()}else{money(ship.freight)};
    shipment_line(ui,name,&ship.products,&freight,chosen,in_preview)
}
fn group_frame(
    ui:&mut Ui,group:&GroupRow,selected:Option<&Selection>,
    open:bool,in_preview:bool,previewed_id:Option<&str>,
    trigger:&mut Option<String>,hovered_row:&mut Option<String>
)->Option<Event>{
    let mut event=None;
    let active=selected.is_some_and(|s|s.group_id==group.id);
    workflow_card(ui,active && !in_preview,in_preview,|ui|{
        let title=format!("{}  {}",if open || in_preview{"▾"}else{"▸"},
            group.dealer_name);
        let btn=workflow_header(ui,&group.id,&title,group.step,group.needs_notice);
            if btn.clicked(){
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
            format!("포함{}·별도{}",group.vat_included,group.vat_excluded)
        }else if group.vat_included>0{
            "부가세 포함".to_owned()
        }else{"부가세 별도".to_owned()};
        let (_,stage_hovered)=workflow_action_line(
            ui,&group.id,group.shipments.len(),group.total,
            Some(&vat),stage_name(group.step,group.needs_notice),false
        );
        if stage_hovered && !in_preview{
            *trigger=Some(group.id.clone());
        }
        if open || in_preview{
            components::soft_divider(ui);
            workflow_table_header(ui);
            for ship in &group.shipments{
                // Orange means committed selection ONLY on the base card.
                // Blue means the temporarily hovered row ONLY in the clone.
                let chosen=!in_preview && selected.is_some_and(|s|s.shipment_id==ship.id);
                let previewed=in_preview && previewed_id==Some(ship.id.as_str());
                let label=row(ui,ship,chosen,previewed);
                let response=ui.interact(
                    label.rect,Id::new(("workflow-shipment",ship.id.as_str(),in_preview)),
                    egui::Sense::click()
                );
                if in_preview && response.hovered(){
                    *hovered_row=Some(ship.id.clone());
                }
                if response.clicked(){
                    event=Some(Event::SelectShipment(Selection{
                        group_id:group.id.clone(),order_id:ship.order_id.clone(),
                        shipment_id:ship.id.clone()
                    }));
                }
            }
        }
    });
    event
}
fn pending_frame(ui:&mut Ui,draft:&WorkingReception,selected:bool,open:bool)->Option<Event>{
    let mut event=None;
    workflow_card(ui,selected,false,|ui|{
        let title=if draft.client_id.is_none(){
            "미확인 거래처".to_owned()
        }else {
            draft.client_match.as_ref()
                .and_then(|m|m.get("name")).and_then(|n|n.as_str())
                .filter(|n|!n.trim().is_empty()).unwrap_or("거래처 확인").to_owned()
        };
        if workflow_header(ui,&draft.id,
            &format!("{}  {}",if open{"▾"}else{"▸"},title),0,false
        ).clicked(){
            event=Some(if selected{Event::TogglePending(draft.id.clone())}
                else{Event::SelectPending(draft.id.clone())});
        }
        let recipients=if draft.analysis_raw.as_deref()==Some(draft.raw.as_str()){
            draft.analysis_evidence.as_ref()
                .and_then(|v|v.get("recipients"))
                .and_then(|v|v.as_array()).map(Vec::as_slice).unwrap_or(&[])
        }else{&[]};
        let (clicked,_)=workflow_action_line(
            ui,&draft.id,recipients.len(),None,None,"거래처 확인",
            draft.client_id.is_none()
        );
        if clicked{event=Some(Event::SelectPending(draft.id.clone()));}
        if open{
            components::soft_divider(ui);
            workflow_table_header(ui);
            if recipients.is_empty(){
                let excerpt=draft.raw.lines().find(|s|!s.trim().is_empty())
                    .unwrap_or("새 발주서");
                let response=shipment_line(ui,"수취인 확인",
                    &[excerpt.chars().take(60).collect()],"—",false,false);
                if ui.interact(response.rect,
                    Id::new(("pending-reception-row",draft.id.as_str(),0usize)),
                    egui::Sense::click()).clicked(){
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
                    let response=shipment_line(ui,name,&products,"—",false,false);
                    if ui.interact(response.rect,
                        Id::new(("pending-reception-row",draft.id.as_str(),index)),
                        egui::Sense::click()).clicked(){
                        event=Some(Event::SelectPending(draft.id.clone()));
                    }
                }
            }
        }
    });
    event
}
/// Render once in the existing fixed pane. The floating expansion uses an egui
/// foreground Area anchored to the card; it DOES NOT resize/reorder other cards.
pub fn show(
    ui:&mut Ui,
    snapshot:Option<&BoardSnapshot>,
    drafts:&[WorkingReception],active_id:Option<&str>,state:&mut BoardUi
)->Option<Event>{
    let mut event=None;
    let mut trigger=None;
    let mut overlay=None;
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
                &mut trigger,&mut ignored_row){
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
        if let Some(value)=pending_frame(ui,draft,selected,open){
            match value{
                Event::TogglePending(_)=>{
                    state.folded=if open{Some(key)}else{None};
                }
                other=>event=Some(other),
            }
        }
        ui.add_space(theme::WORKFLOW_CARD_GAP);
    }
    let hovered=trigger.clone();
    state.update_hover(hovered,ui.ctx());
    if let Some((group,pos,width))=overlay{
        let pane_clip=ui.clip_rect();
        let mut overlay_event=None;
        let mut hovered_row=None;
        let area=egui::Area::new(Id::new(("workflow-card-overlay",group.id.clone())))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(pos.x,pos.y));
        let response=area.show(ui.ctx(),|ui|{
            // The temporary card can grow downward, but must not bleed
            // over the source/details panes or the app header.
            ui.set_clip_rect(pane_clip);
            ui.set_min_width(width);
            ui.set_max_width(width);
            let mut ignored_hover=None;
            overlay_event=group_frame(
                ui,&group,current,true,true,state.preview_shipment.as_deref(),
                &mut ignored_hover,&mut hovered_row
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
            }else { event=Some(overlay_event); }
        }
    }else{
        state.preview_rect=None;
        state.update_row_hover(None,ui.ctx());
    }
    event
}
