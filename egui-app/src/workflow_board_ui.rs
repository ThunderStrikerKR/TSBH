//! The right-hand workflow board. Layout/hover/selection only; no business rules.
//! Native Rust provides groups, amounts, freight, stage and shipment identity.
use eframe::egui::{self, Frame, Id, Pos2, Rect, Ui, Vec2};
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
    }
    pub fn select(&mut self,selection:Selection){
        self.selection=Some(selection);
        self.folded=None;
        self.stop_preview();
    }
    pub fn active_preview(&self)->Option<&str>{
        self.preview_active.as_deref()
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
fn lights(ui:&mut Ui,step:usize,notice:bool){
    ui.horizontal(|ui|{
        ui.spacing_mut().item_spacing.x=theme::SPACE_4;
        for index in 0..STEP_NAMES.len(){
            let (rect,_)=ui.allocate_exact_size(
                Vec2::splat(theme::SPACE_12),egui::Sense::hover()
            );
            let color=if index<step {theme::SUCCESS}
                else if index==step && notice {theme::SELECTED_ORDER}
                else if index==step {theme::SECONDARY}
                else{theme::DIVIDER};
            ui.painter().circle_filled(rect.center(),theme::SPACE_4,color);
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
fn row(ui:&mut Ui,ship:&ShipmentRow,chosen:bool,in_preview:bool)->egui::Response{
    ui.scope(|ui|{
        let active=if chosen{theme::SELECTED_ORDER}else if in_preview{
            theme::SELECTION
        }else{theme::DIVIDER};
        Frame::new().fill(theme::INPUT)
            .stroke(egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,active))
            .inner_margin(theme::SPACE_4)
            .show(ui,|ui|{
                let title=if ship.recipient_name.is_empty(){
                    "수취인 확인"
                }else{ship.recipient_name.as_str()};
                ui.label(typography::field_label(title));
                ui.horizontal_wrapped(|ui|{
                    ui.spacing_mut().item_spacing.x=theme::SPACE_4;
                    for product in &ship.products{
                        ui.label(typography::caption(product));
                    }
                    let freight=if ship.collect{"착불".to_owned()}else{money(ship.freight)};
                    ui.label(typography::quantity(&freight));
                });
            });
    }).response
}
fn group_frame(
    ui:&mut Ui,group:&GroupRow,selected:Option<&Selection>,
    open:bool,in_preview:bool,trigger:&mut Option<String>
)->Option<Event>{
    let mut event=None;
    theme::nested_frame().show(ui,|ui|{
        ui.horizontal_wrapped(|ui|{
            let active=selected.is_some_and(|s|s.group_id==group.id);
            let title=format!("{} {}",if open || in_preview{"▾"}else{"▸"},
                group.dealer_name);
            let btn=theme::action(ui,&title,if active{
                theme::ButtonRole::ActiveNavigation
            }else{theme::ButtonRole::Quiet},true);
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
            lights(ui,group.step,group.needs_notice);
        });
        ui.horizontal_wrapped(|ui|{
            ui.label(typography::caption(&format!(
                "배송 {}건",group.shipments.len())));
            ui.label(typography::summary_value(&money(group.total)));
            if group.vat_included>0 && group.vat_excluded>0{
                ui.label(typography::caption(&format!(
                    "포함{} · 별도{}",group.vat_included,group.vat_excluded)));
            }else if group.vat_included>0{
                ui.label(typography::caption("부가세 포함"));
            }else{
                ui.label(typography::caption("부가세 별도"));
            }
        });
        // The employee action itself must remain absent until its native
        // audited mutation + revision checked SQLite writer is migrated.
        // The read-only stage label still owns the approved hover preview.
        let label=ui.label(typography::field_label(
            stage_name(group.step,group.needs_notice)
        )).on_hover_text(STEP_NAMES[group.step.min(3)]);
        if label.hovered() && !in_preview{*trigger=Some(group.id.clone());}
        if open || in_preview{
            components::soft_divider(ui);
            ui.horizontal(|ui|{
                ui.label(typography::table_header("수취인"));
                ui.label(typography::table_header("상품"));
                ui.label(typography::table_header("운임"));
            });
            for ship in &group.shipments{
                let chosen=selected.is_some_and(|s|s.shipment_id==ship.id);
                let label=row(ui,ship,chosen,in_preview && !chosen);
                let response=ui.interact(
                    label.rect,Id::new(("workflow-shipment",ship.id.as_str(),in_preview)),
                    egui::Sense::click()
                );
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
    let mut selected_id=None;
    theme::nested_frame().show(ui,|ui|{
        let title=if draft.client_id.is_none(){
            "미확인 거래처".to_owned()
        }else {
            draft.client_match.as_ref()
                .and_then(|m|m.get("name")).and_then(|n|n.as_str())
                .filter(|n|!n.trim().is_empty()).unwrap_or("거래처 확인").to_owned()
        };
        if theme::action(ui,&format!("{} {}",if open{"▾"}else{"▸"},title),
            if selected{theme::ButtonRole::ActiveNavigation}
            else{theme::ButtonRole::Quiet},true).clicked(){
            selected_id=Some(draft.id.clone());
        }
        let recipients=if draft.analysis_raw.as_deref()==Some(draft.raw.as_str()){
            draft.analysis_evidence.as_ref()
                .and_then(|v|v.get("recipients"))
                .and_then(|v|v.as_array()).map(Vec::as_slice).unwrap_or(&[])
        }else{&[]};
        ui.label(typography::caption(&format!(
            "배송 {}건",recipients.len()
        )));
        if open{
            components::soft_divider(ui);
            if recipients.is_empty(){
                let excerpt=draft.raw.lines().find(|s|!s.trim().is_empty())
                    .unwrap_or("새 발주서");
                ui.label(typography::body(excerpt));
            }else{
                for recipient in recipients{
                    let name=recipient.get("name").and_then(|v|v.as_str())
                        .filter(|s|!s.is_empty()).unwrap_or("수취인 확인");
                    if theme::action_full_width(ui,name,theme::ButtonRole::Quiet,true)
                        .clicked(){
                        selected_id=Some(draft.id.clone());
                    }
                }
            }
        }
    });
    selected_id.map(|id|if selected{Event::TogglePending(id)}
        else{Event::SelectPending(id)})
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
    if let Some(snapshot)=snapshot{
        for group in &snapshot.groups {
            let selected=current.is_some_and(|s|s.group_id==group.id);
            let open=selected && state.folded.as_deref()!=Some(group.id.as_str());
            let width=ui.available_width();
            let pos=ui.cursor().min;
            if let Some(value)=group_frame(ui,group,current,open,false,&mut trigger){
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
            ui.add_space(theme::SPACE_8);
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
        ui.add_space(theme::SPACE_8);
    }
    let hovered=trigger.clone();
    state.update_hover(hovered,ui.ctx());
    if let Some((group,pos,width))=overlay{
        let mut overlay_event=None;
        let area=egui::Area::new(Id::new(("workflow-card-overlay",group.id.clone())))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(pos.x,pos.y));
        let response=area.show(ui.ctx(),|ui|{
            ui.set_min_width(width);
            ui.set_max_width(width);
            let mut ignored_hover=None;
            overlay_event=group_frame(
                ui,&group,current,true,true,&mut ignored_hover
            );
        });
        state.preview_rect=Some(response.response.rect);
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
    }
    event
}
