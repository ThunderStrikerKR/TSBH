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
        // The surrounding list has zero automatic vertical spacing so
        // adjacent cards use the exact 4pt gap token. Restore normal
        // spacing INSIDE each card (header/action/content rows).
        card.spacing_mut().item_spacing.y=theme::SPACING;
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
/// Exactly one compact header row. The dealer title is a separate left-
/// aligned interactive area; stage lamps are adjacent but NOT part of its
/// hit area or hover decoration. The original 18pt lamp diameter is fixed
/// regardless of window width.
fn workflow_header(
    ui:&mut Ui,id:&str,title:&str,open:bool,step:usize,notice:bool
)->(egui::Response,bool){
    let width=ui.available_width();
    let inline_title=(width-theme::WORKFLOW_SIGNAL_WIDTH
        -theme::WORKFLOW_MANAGE_WIDTH-theme::WORKFLOW_TRASH_WIDTH
        -3.0*theme::SPACE_4).max(0.0);
    // All four 18pt status lamps remain intact. When the dealer caption
    // would lose its ten-character reading area, move only the two
    // disabled auxiliary actions to a right-aligned row below the summary.
    let wrapped_actions=inline_title<theme::WORKFLOW_TITLE_MIN_WIDTH;
    let title_width=if wrapped_actions{
        (width-theme::WORKFLOW_SIGNAL_WIDTH-theme::SPACE_4).max(0.0)
    }else{inline_title};
    let mut title_response=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        let (title_rect,_)=bar.allocate_exact_size(
            Vec2::new(title_width,theme::WORKFLOW_TITLE_HEIGHT),
            egui::Sense::hover(),
        );
        let icon_center=Pos2::new(
            title_rect.left()+theme::WORKFLOW_TOGGLE_ICON_WIDTH/2.0,
            title_rect.center().y,
        );
        components::paint_workflow_icon(
            bar.painter(),icon_center,
            if open{components::WorkflowIcon::Expanded}
            else{components::WorkflowIcon::Collapsed},theme::SECONDARY
        );
        // Anchor the actual glyphs to the LEFT, immediately after the
        // fold arrow. Label/put layout can center short names inside the
        // reserved heading rectangle even with halign(Min).
        // Draw with the standard Section typography galley and clip only
        // the text, not the title hit area or the neighboring status lamps.
        let text_left=title_rect.left()
            +theme::WORKFLOW_TOGGLE_ICON_WIDTH+theme::SPACE_4;
        let text_rect=Rect::from_min_max(
            Pos2::new(text_left,title_rect.top()),
            title_rect.max,
        );
        let galley=typography::section_heading_galley(bar,title);
        let text_pos=Pos2::new(
            text_rect.left(),
            text_rect.center().y-galley.size().y/2.0,
        );
        bar.painter().with_clip_rect(text_rect)
            .galley(text_pos,galley,theme::TEXT);
        let heading=bar.interact(
            title_rect,Id::new(("workflow-header",id)),
            egui::Sense::click()
        ).on_hover_text(title);
        title_response=Some(heading);

        // Independent status area: not part of the heading's click target,
        // background or border. Never use a smaller compact-lamp variant.
        let (status_rect,_)=bar.allocate_exact_size(
            Vec2::new(theme::WORKFLOW_SIGNAL_WIDTH,theme::WORKFLOW_TITLE_HEIGHT),
            egui::Sense::hover()
        );
        let diameter=theme::WORKFLOW_SIGNAL_SIZE;
        for index in 0..STEP_NAMES.len(){
            let center=Pos2::new(
                status_rect.left()+diameter/2.0
                    +index as f32*(diameter+theme::WORKFLOW_SIGNAL_GAP),
                status_rect.center().y,
            );
            let (fill,border)=if index<step{
                (theme::WORKFLOW_SIGNAL_DONE,theme::WORKFLOW_SIGNAL_DONE_BORDER)
            }else if index==step || (index==step && notice){
                (theme::WORKFLOW_SIGNAL_CURRENT,theme::WORKFLOW_SIGNAL_CURRENT_BORDER)
            }else{
                (theme::WORKFLOW_SIGNAL_PENDING,theme::WORKFLOW_SIGNAL_PENDING_BORDER)
            };
            bar.painter().circle_filled(center,diameter/2.0,fill);
            bar.painter().circle_stroke(
                center,diameter/2.0,
                egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,border),
            );
        }

        if !wrapped_actions {
            components::workflow_auxiliary_button(
                bar, components::WorkflowAuxiliaryAction::Manage,
            );
            components::workflow_auxiliary_button(
                bar, components::WorkflowAuxiliaryAction::Delete,
            );
        }
    });
    (
        title_response.expect("workflow heading must reserve a clickable title"),
        wrapped_actions
    )
}

/// Preserve both auxiliary controls at the same 52/34pt widths when a
/// narrow reception column cannot also fit a readable dealer title.
fn workflow_wrapped_header_actions(ui:&mut Ui){
    ui.with_layout(Layout::right_to_left(Align::Center),|right|{
        right.spacing_mut().item_spacing.x=theme::SPACE_4;
        components::workflow_auxiliary_button(
            right,components::WorkflowAuxiliaryAction::Delete
        );
        components::workflow_auxiliary_button(
            right,components::WorkflowAuxiliaryAction::Manage
        );
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
/// Same two CARD rows as React: summary and the one-touch action share
/// the second row. The summary itself is allowed two short text lines inside
/// the EXISTING button height so shipping, VAT and actual total remain
/// readable without clipping or adding a third card row.

fn workflow_action_line(
    ui:&mut Ui,id:&str,shipments:usize,total:Option<i64>,
    vat:Option<&str>,button_text:&str,enabled:bool
)->(bool,bool){
    let width=ui.available_width();
    // The one-touch button is the SAME Emphasized role as "+ 새 주문".
    // Measure its entire real caption before reserving the action slot.
    // Do not shorten "계좌 재안내 완료" or allow it to wrap.
    let caption_width=ui.painter().layout_no_wrap(
        button_text.to_owned(),
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
        theme::BRIGHT_TEXT,
    ).size().x;
    let button_width=theme::WORKFLOW_CONTROL_WIDTH.max(
        (caption_width+theme::SPACE_24+theme::SPACE_8).ceil()
    );
    let actions=button_width+theme::WORKFLOW_TRASH_WIDTH+theme::SPACE_4;
    let summary_width=(width-actions-theme::SPACE_4).max(0.0);
    let mut stage_button=None;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        let amount=total.map(components::display_won)
            .unwrap_or_else(||"금액 미확정".to_owned());
        bar.allocate_ui_with_layout(
            Vec2::new(summary_width,theme::WORKFLOW_ACTION_ROW_HEIGHT),
            Layout::top_down(Align::Min),|summary|{
                summary.spacing_mut().item_spacing.y=0.0;
                summary.set_width(summary_width);
                // React flow-summary-compact: shipment count is LEFT,
                // included/mixed VAT is RIGHT. Both are on the top line.
                summary.allocate_ui_with_layout(
                    Vec2::new(summary_width,theme::WORKFLOW_SUMMARY_LINE_HEIGHT),
                    Layout::left_to_right(Align::Center),|line|{
                        line.spacing_mut().item_spacing.x=theme::SPACE_4;
                        line.label(typography::field_label(
                            &format!("배송 {shipments}건")
                        ).color(theme::SECONDARY));
                        if let Some(vat)=vat{
                            line.with_layout(Layout::right_to_left(Align::Center),|right|{
                                right.label(
                                    typography::caption(vat)
                                        .color(theme::WORKFLOW_VAT_TEXT)
                                ).on_hover_text(vat);
                            });
                        }
                    }
                );
                // Right-anchored and truncation-aware. Even an unusually
                // long amount cannot paint into the adjacent one-touch slot.
                summary.add_sized(
                    Vec2::new(summary_width,theme::WORKFLOW_SUMMARY_LINE_HEIGHT),
                    egui::Label::new(typography::body(&amount))
                        .halign(Align::RIGHT).truncate()
                ).on_hover_text(&amount);
            }
        );
        bar.allocate_ui_with_layout(
            Vec2::new(button_width,
                theme::WORKFLOW_ACTION_ROW_HEIGHT),
            Layout::top_down(Align::Min),|control|{
                stage_button=Some(theme::action_full_width(
                    control,button_text,theme::ButtonRole::Emphasized,enabled
                ));
            }
        );
        components::workflow_auxiliary_button(
            bar, components::WorkflowAuxiliaryAction::PreviousStage,
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
    // 28pt reception compact role, rendered with the same raised header
    // surface, 2pt inset underline and consistent content columns as masters.
    let (rect,_)=ui.allocate_exact_size(
        Vec2::new(width,theme::WORKFLOW_DETAIL_HEADER_HEIGHT),egui::Sense::hover()
    );
    ui.painter().rect_filled(rect,0.0,theme::WORKFLOW_DETAIL_HEADER_BG);
    let content_bottom=rect.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT;
    let first=Rect::from_min_max(
        rect.min,Pos2::new(rect.left()+theme::WORKFLOW_RECIPIENT_COL,content_bottom)
    );
    let last=Rect::from_min_max(
        Pos2::new(rect.right()-theme::WORKFLOW_FREIGHT_COL,rect.top()),
        Pos2::new(rect.right(),content_bottom)
    );
    let middle=Rect::from_min_max(
        Pos2::new(first.right()+theme::SPACE_4,rect.top()),
        Pos2::new(last.left()-theme::SPACE_4,content_bottom)
    );
    for (text,cell,align_right) in [
        ("수취인",first,false),("상품",middle,false),("운임",last,true)
    ]{
        if cell.width()<=0.0{continue;}
        let galley=typography::management_table_header_galley(ui,text);
        let x=if align_right{
            cell.right()-theme::SPACE_4-galley.size().x
        }else{cell.left()+theme::SPACE_4};
        ui.painter().with_clip_rect(cell).galley(
            Pos2::new(x,cell.center().y-galley.size().y/2.0),
            galley,theme::SECONDARY
        );
    }
    ui.painter().rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(),content_bottom),rect.right_bottom()
        ),0.0,theme::TABLE_HEADER_OUTLINE
    );
}
/// One product occupies a compact chip, exactly as the React
/// .workflow-shipment .product-chip style, not an unframed text fragment.
fn product_chip(ui:&mut Ui,label:&str,max_width:f32){
    // A chip may wrap to the NEXT product row, but never one letter per
    // vertical line. Measure its unwrapped text and cap the chip itself to
    // the allocated PRODUCT column, preserving the complete name in tooltip.
    let natural=ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(typography::FONT_FIELD_LABEL),
        theme::TEXT,
    ).size().x;
    let width=(natural+2.0*theme::WORKFLOW_CHIP_PAD_X as f32
        +2.0*theme::SOFT_DIVIDER_WIDTH).min(max_width).max(0.0);
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::WORKFLOW_CHIP_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,theme::WORKFLOW_CHIP_RADIUS as f32,
        theme::WORKFLOW_CHIP_FILL
    );
    ui.painter().rect_stroke(
        rect,theme::WORKFLOW_CHIP_RADIUS as f32,
        egui::Stroke::new(
            theme::SOFT_DIVIDER_WIDTH,theme::WORKFLOW_CHIP_BORDER
        ),egui::StrokeKind::Inside,
    );
    let text_rect=rect.shrink2(Vec2::new(
        theme::WORKFLOW_CHIP_PAD_X as f32,
        theme::WORKFLOW_CHIP_PAD_Y as f32,
    ));
    ui.put(
        text_rect,egui::Label::new(typography::table_cell(label)).truncate()
    );
    response.on_hover_text(label);
}
/// React .workflow-shipment uses a 64 / flexible / 68 three-column row.
/// No secondary boxed cards: selection is a subtle background plus a thin
/// accent stripe, not an extra input-style border around every recipient.
fn shipment_line(
    ui:&mut Ui,name:&str,products:&[String],freight:&str,
    chosen:bool,previewed:bool,alternate:bool
)->egui::Response{
    let fill=if chosen{
        theme::WORKFLOW_SELECTED_BORDER.linear_multiply(0.20)
    }else if previewed{
        theme::SELECTION.linear_multiply(0.14)
    }else if alternate{theme::WORKFLOW_DETAIL_ALT_BG}
    else{egui::Color32::TRANSPARENT};
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
                                product_chip(chips,product,product_width);
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
fn row(
    ui:&mut Ui,ship:&ShipmentRow,chosen:bool,in_preview:bool,alternate:bool
)->egui::Response{
    let name=if ship.recipient_name.is_empty(){
        "수취인 확인"
    }else{ship.recipient_name.as_str()};
    let freight=if ship.collect{"착불".to_owned()}else{money(ship.freight)};
    shipment_line(ui,name,&ship.products,&freight,chosen,in_preview,alternate)
}
fn group_frame(
    ui:&mut Ui,group:&GroupRow,selected:Option<&Selection>,
    open:bool,in_preview:bool,previewed_id:Option<&str>,
    trigger:&mut Option<String>,hovered_row:&mut Option<String>
)->Option<Event>{
    let mut event=None;
    let active=selected.is_some_and(|s|s.group_id==group.id);
    workflow_card(ui,active && !in_preview,in_preview,|ui|{
        let (btn,wrapped_actions)=workflow_header(
            ui,&group.id,&group.dealer_name,open || in_preview,
            group.step,group.needs_notice
        );
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
            if group.vat_included>0{Some(vat.as_str())}else{None},
            stage_name(group.step,group.needs_notice),false
        );
        if stage_hovered && !in_preview{
            *trigger=Some(group.id.clone());
        }
        if wrapped_actions{workflow_wrapped_header_actions(ui);}
        if open || in_preview{
            components::soft_divider(ui);
            ui.add_space(theme::SPACE_4);
            workflow_table_header(ui);
            for (index,ship) in group.shipments.iter().enumerate(){
                // Orange means committed selection ONLY on the base card.
                // Blue means the temporarily hovered row ONLY in the clone.
                let chosen=!in_preview && selected.is_some_and(|s|s.shipment_id==ship.id);
                let previewed=in_preview && previewed_id==Some(ship.id.as_str());
                let label=row(ui,ship,chosen,previewed,index%2==1);
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
        let (header,wrapped_actions)=workflow_header(ui,&draft.id,&title,open,0,false);
        if header.clicked(){
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
        if wrapped_actions{workflow_wrapped_header_actions(ui);}
        if open{
            components::soft_divider(ui);
            ui.add_space(theme::SPACE_4);
            workflow_table_header(ui);
            if recipients.is_empty(){
                let excerpt=draft.raw.lines().find(|s|!s.trim().is_empty())
                    .unwrap_or("새 발주서");
                let response=shipment_line(ui,"수취인 확인",
                    &[excerpt.chars().take(60).collect()],"—",false,false,false);
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
                    let response=shipment_line(
                        ui,name,&products,"—",false,false,index%2==1
                    );
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
    // Prevent implicit vertical item spacing from doubling the explicit
    // WORKFLOW_CARD_GAP between order bundles. This is scoped to the list
    // inside its own ScrollArea; workflow_card restores internal spacing.
    ui.spacing_mut().item_spacing.y=0.0;
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
