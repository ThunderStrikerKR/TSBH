//! Large, read-only order detail dashboard.
//! All business values come from the saved order-history projection and its
//! matching workflow records. This module never recalculates prices or writes
//! historical data. Only content-heavy panes have their own scroll areas.
use crate::ui_layout::geometry::SharedUiClipExt as _;
use crate::ui_layout::geometry::SharedAllocationExt as _;
use crate::ui_layout::geometry::AlignedUiBuilderExt as _;
use crate::{theme,typography};
use crate::ui_layout::geometry::UiWidthPolicyExt;
use chrono::{DateTime,FixedOffset};
use eframe::egui::{self,Align,Layout,Rect,Sense,Ui,UiBuilder,Vec2};
use serde_json::Value;
use std::collections::HashSet;

const PANE_GAP:f32=theme::SPACE_8;
const INSET_X:f32=theme::SPACE_12;
const INSET_Y:f32=theme::SPACE_8;
const PANE_TITLE_HEIGHT:f32=31.0;

fn field<'a>(value:&'a Value,key:&str)->&'a str{
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value:&Value)->&[Value]{
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn kst(raw:&str)->String{
    if raw.is_empty(){return "기록 없음".to_owned();}
    DateTime::parse_from_rfc3339(raw).ok()
        .map(|stamp|stamp.with_timezone(
            &FixedOffset::east_opt(9*3600).expect("KST"))
            .format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(||raw.to_owned())
}
fn money(value:&Value)->String{
    value.as_i64().map(crate::display_format::display_won)
        .unwrap_or_else(||"미확정".to_owned())
}
fn display_or<'a>(value:&'a str,fallback:&'a str)->&'a str{
    if value.trim().is_empty(){fallback}else{value}
}
fn order_type(value:&str)->&str{
    match value {
        "AGENCY"=>"대리발송","DEALER"=>"대리점 납품",
        "RETAIL"=>"소매",_=>"구분 미확인"
    }
}
fn freight_label(code:&str)->&str{
    match code {"010"=>"선불","020"=>"착불",_=>"운임 구분 미확인"}
}
fn text_kv(ui:&mut Ui,name:&str,value:&str){
    text_kv_with_state(ui,name,value,theme::TextState::Normal);
}
fn text_kv_with_state(ui:&mut Ui,name:&str,value:&str,state:theme::TextState){
    ui.horizontal_wrapped(|row|{
        row.label(typography::caption(name));
        row.label(typography::body_with_state(value,state));
    });
}
fn money_kv(ui:&mut Ui,name:&str,value:&str,state:theme::TextState){
    ui.horizontal_wrapped(|row|{
        row.label(typography::caption(name));
        row.label(typography::data_value_text_with_state(
            value,typography::DataValueKind::Money,
            typography::DataValueContext::Form,state
        ).line_height(Some(22.0)));
    });
}
fn section_panel(
    ui:&mut Ui,rect:Rect,title:&str,id:&'static str,order_id:&str,
    body:impl FnOnce(&mut Ui)
){
    // This module supplies only order/pane identity and approved geometry.
    // Frame paint, title clipping and independent scrolling belong to the
    // shared component so every fixed detail pane behaves identically.
    crate::card::fixed_scroll_panel(
        ui,rect,title,
        ("history-detail-heading",order_id,id),
        ("history-detail-pane",order_id,id),
        Vec2::new(INSET_X,INSET_Y),
        PANE_TITLE_HEIGHT,theme::SPACE_4,body
    );
}
fn overview(ui:&mut Ui,rect:Rect,row:&Value,shipment_count:usize){
    let palette=theme::surface_palette(theme::SurfaceRole::RaisedInset,theme::VisualState::Normal);
    theme::paint_bordered_surface(ui.painter(),rect,theme::CONTROL_RADIUS as f32,palette.fill,palette.border);
    let inner=rect.shrink2(Vec2::new(theme::SPACE_12,theme::SPACE_8));
    let clip=inner.intersect(ui.clip_rect());
    let order=&row["order"];
    ui.scope_builder(UiBuilder::new()
        .id_salt("history-detail-overview")
        .aligned_max_rect(inner).layout(Layout::top_down(Align::Min)),|panel|{
            panel.restrict_shared_clip(clip);
            panel.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Fixed,inner.width());
            panel.horizontal_wrapped(|line|{
                line.label(typography::section_heading(
                    display_or(field(&row["client"],"name"),"거래처 미확인")
                ));
                line.label(typography::status(
                    display_or(field(row,"status"),"상태 미확인"),
                    theme::text_ink(theme::TextRole::Label,theme::TextState::Normal)
                ));
                line.label(typography::caption(&format!(
                    "{} · 발송 건 {}건",
                    order_type(field(order,"orderType")),shipment_count
                )));
            });
            panel.horizontal_wrapped(|line|{
                line.label(typography::caption(&format!(
                    "접수 {}",kst(field(order,"createdAt"))
                )));
                line.label(typography::caption(&format!(
                    "주문 확인 {}",kst(field(order,"confirmedAt"))
                )));
                let channel=field(order,"channel");
                if !channel.is_empty(){
                    line.label(typography::caption(&format!("접수 경로 {channel}")));
                }
            });
        });
}
fn raw_text(order:&Value)->(&'static str,&str){
    let original=field(&order["receipt"],"original");
    if !original.is_empty(){return ("최초 발주 원문",original);}
    let confirmed=field(&order["confirmationSnapshot"],"raw");
    if !confirmed.is_empty(){return ("주문 확인 당시 원문",confirmed);}
    ("저장된 발주 원문",field(order,"raw"))
}
fn payment_panel(
    ui:&mut Ui,order:&Value,row:&Value,payments:&[&Value],
    confirmations:&[&Value]
){
    text_kv(ui,"접수일시", &kst(field(order,"createdAt")));
    text_kv(ui,"주문 확인일시", &kst(field(order,"confirmedAt")));
    let staff=&row["display"]["employeePaymentConfirmation"];
    text_kv(ui,"입금 확인 일시",display_or(field(staff,"dateTime"),
        display_or(field(staff,"date"),"미확인")));
    ui.add_space(theme::SPACE_4);
    if payments.is_empty(){
        crate::ui_primitives::management_body_state_inline(ui,"해당 주문에 연결된 입금 기록이 없습니다.");
    }
    for (i,payment) in payments.iter().enumerate(){
        if i>0{crate::ui_primitives::soft_divider(ui);}
        text_kv(ui,"입금 처리일시",&kst(field(payment,"recordedAt")));
        text_kv(ui,"입금 일시",&kst(field(payment,"paidAt")));
        money_kv(ui,"입금액",&money(&payment["amount"]),theme::TextState::value(payment["amount"].as_i64().is_some()));
        let account=&payment["account"];
        let bank=field(account,"bank");
        let number=field(account,"number");
        text_kv(ui,"입금계좌번호",
            &format!("{} {}",bank,display_or(number,"기록 없음")));
        let holder=field(account,"holder");
        if !holder.is_empty(){text_kv(ui,"예금주",holder);}
    }
    // An announced account is not proof that money reached this account.
    // Show it separately so unpaid orders still expose the instructed bank.
    for (i,confirm) in confirmations.iter().enumerate(){
        let account=&confirm["announcedAccount"];
        let number=field(account,"number");
        if number.is_empty(){continue;}
        if i==0{ui.add_space(theme::SPACE_4);}
        text_kv(ui,"안내 계좌번호",&format!(
            "{} {}",field(account,"bank"),number
        ));
    }
}
fn billing_panel(ui:&mut Ui,order:&Value,billing:&Value){
    let vat=match billing["vat"].as_i64(){
        Some(n) if n>0=>"부가세 청구",
        Some(0)=>"부가세 미청구",
        _=>match order["applyVat"].as_bool(){
            Some(true)=>"부가세 청구 설정",
            Some(false)=>"부가세 미청구 설정",
            None=>"부가세 여부 미확인"
        }
    };
    text_kv(ui,"부가세 여부",vat);
    ui.add_space(theme::SPACE_4);
    for (label,key) in [
        ("품목 금액","subtotal"),("운임","shipping"),
        ("부가세","vat")
    ]{
        ui.horizontal(|line|{
            line.label(typography::body(label));
            line.with_layout(Layout::right_to_left(Align::Center),|right|{
                right.label(typography::data_value_text_with_state(
                    &money(&billing[key]),typography::DataValueKind::Money,
                    typography::DataValueContext::Form,theme::TextState::value(billing[key].as_i64().is_some())
                ).line_height(Some(22.0)));
            });
        });
    }
    crate::ui_primitives::section_divider(ui);
    ui.horizontal(|line|{
        line.label(typography::section_heading("청구 합계"));
        line.with_layout(Layout::right_to_left(Align::Center),|right|{
            right.label(typography::data_value_text_with_state(
                &money(&billing["total"]),typography::DataValueKind::Money,
                typography::DataValueContext::Summary,theme::TextState::value(billing["total"].as_i64().is_some())
            ).line_height(Some(23.0)));
        });
    });
}
fn shipment_values(ship:&Value)->(String,String){
    let freight=ship["actualFreight"].as_object().into_iter()
        .flat_map(|o|o.values())
        .filter_map(Value::as_i64)
        .map(crate::display_format::display_won).collect::<Vec<_>>();
    let tracking=ship["tracking"].as_object().into_iter()
        .flat_map(|o|o.values()).filter_map(Value::as_str)
        .filter(|s|!s.is_empty()).collect::<Vec<_>>();
    (
        if freight.is_empty(){"미확인".to_owned()}else{freight.join(" / ")},
        if tracking.is_empty(){"미확인".to_owned()}else{tracking.join(" / ")}
    )
}
fn shipment_entry(
    ui:&mut Ui,order:&Value,recipient:&Value,ship:Option<&Value>,index:usize
){
    let label=display_or(field(recipient,"name"),"수취인 미확인");
    crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::Standard).show(ui,|card|{
        card.horizontal_wrapped(|header|{
            header.label(typography::subsection_heading(&format!(
                "발송 건 {} · {}",index+1,label
            )));
            if let Some(ship)=ship{
                header.label(typography::caption(&format!(
                    "처리 단계 {}",
                    ship["step"].as_i64().map(|n|n.to_string())
                        .unwrap_or_else(||"미확인".to_owned())
                )));
            }else{
                header.label(typography::caption("발송 건 미생성"));
            }
        });
        let address=field(recipient,"address");
        if !address.is_empty(){text_kv(card,"주소",address);}
        let contact=display_or(field(recipient,"mobile"),field(recipient,"phone"));
        if !contact.is_empty(){text_kv(card,"연락처",contact);}
        let shipping=display_or(field(recipient,"freightCode"),"");
        if !shipping.is_empty(){text_kv(card,"운임 구분",freight_label(shipping));}
        let sender=ship.and_then(|s|s.get("sender"))
            .filter(|v|v.is_object()).unwrap_or(&order["senderSnapshot"]);
        let sender_name=display_or(field(sender,"displayName"),field(sender,"name"));
        if !sender_name.is_empty(){text_kv(card,"송하인",sender_name);}
        let request=field(recipient,"note");
        if !request.is_empty(){text_kv(card,"배송 요청사항",request);}
        card.add_space(theme::SPACE_4);
        for item in rows(&recipient["items"]){
            let name=display_or(field(item,"name"),"품목명 미확인");
            let qty=item["quantity"].as_i64().map(|v|v.to_string())
                .unwrap_or_else(||"미확인".into());
            let prefix=format!("◆ {name} × ");
            let price=item["unitPrice"].as_i64().map(crate::display_format::display_won);
            let job=if let Some(price)=price.as_deref(){
                typography::inline_body_values(&[
                    (prefix.as_str(),None),
                    (qty.as_str(),Some(typography::DataValueKind::Quantity)),
                    (" · 단가 ",None),
                    (price,Some(typography::DataValueKind::Money)),
                ])
            }else{
                typography::inline_body_values(&[
                    (prefix.as_str(),None),
                    (qty.as_str(),Some(typography::DataValueKind::Quantity)),
                ])
            };
            card.add(egui::Label::new(job));
        }
        if rows(&recipient["items"]).is_empty(){
            crate::ui_primitives::management_body_state_inline(card,"품목 정보 없음");
        }
        card.add_space(theme::SPACE_4);
        if let Some(ship)=ship{
            let (freight,tracking)=shipment_values(ship);
            money_kv(card,"출고 운임",&freight,theme::TextState::value(ship["actualFreight"].as_object().is_some_and(|o|o.values().any(|v|v.as_i64().is_some()))));
            text_kv_with_state(card,"운송장번호",&tracking,theme::TextState::optional(ship["tracking"].as_object().is_some_and(|o|o.values().any(|v|v.as_str().is_some_and(|s|!s.is_empty())))));
            let note=field(ship,"managementMemo");
            text_kv_with_state(card,"관리 메모",display_or(note,"없음"),theme::TextState::optional(!note.trim().is_empty()));
            text_kv(card,"최근 메모 갱신",&kst(field(ship,"managementMemoUpdatedAt")));
        }else{
            crate::ui_primitives::management_body_state_inline(card,"현재 연결된 운송장과 발송 건 메모가 없습니다.");
        }
    });
}
fn shipment_panel(ui:&mut Ui,order:&Value,shipments:&[&Value]){
    let recipients=rows(&order["recipients"]);
    let mut numbered=0usize;
    for recipient in recipients{
        let related=shipments.iter().copied().filter(|ship|
            field(ship,"recipientId")==field(recipient,"id")
        ).collect::<Vec<_>>();
        if related.is_empty(){
            shipment_entry(ui,order,recipient,None,numbered);
            numbered+=1;
            ui.add_space(theme::SPACE_8);
        }else{
            for ship in related{
                let frozen=ship.get("recipientSnapshot")
                    .filter(|v|v.is_object())
                    .unwrap_or(recipient);
                shipment_entry(ui,order,frozen,Some(ship),numbered);
                numbered+=1;
                ui.add_space(theme::SPACE_8);
            }
        }
    }
    if recipients.is_empty(){
        crate::ui_primitives::management_body_state_inline(ui,"저장된 수취인 및 발송 건 정보가 없습니다.");
    }
}
struct AuditEntry {at:String,title:String,detail:String}
fn audit_value(value:&Value)->String{
    let full=match value {
        Value::String(text)=>text.clone(),
        Value::Null=>String::new(),
        other=>other.to_string()
    };
    let max=100usize;
    let mut short=full.chars().take(max).collect::<String>();
    if full.chars().count()>max{short.push('…');}
    short
}
fn change_entries(order:&Value,state:&Value,shipment_ids:&HashSet<&str>)->Vec<AuditEntry>{
    let mut output=Vec::new();
    for change in rows(&order["rawHistory"]){
        output.push(AuditEntry{
            at:field(change,"at").to_owned(),
            title:"발주 원문 변경".into(),
            detail:format!("담당자 {} · 변경 전 원문: {}",
                bando_data_access::terminology::display_actor(field(change,"actor")),
                audit_value(&change["raw"]))
        });
    }
    for change in rows(&order["rawCorrections"]){
        output.push(AuditEntry{
            at:field(change,"at").to_owned(),
            title:"발주 원문 정정".into(),
            detail:format!(
                "담당자 {} · {} · 변경 전 {} → 변경 후 {}",
                bando_data_access::terminology::display_actor(field(change,"actor")),
                display_or(field(change,"reason"),"정정 사유 미기록"),
                audit_value(&change["before"]),audit_value(&change["after"])
            )
        });
    }
    for history in rows(&order["billingHistory"]){
        output.push(AuditEntry{
            at:field(history,"at").to_owned(),
            title:"청구금액 이전 기록".into(),
            detail:format!("{} · 당시 합계 {}",
                field(history,"reason"),money(&history["total"]))
        });
    }
    for event in rows(&state["workflow"]["events"]){
        if !rows(&event["shipmentIds"]).iter().any(|sid|
            sid.as_str().is_some_and(|id|shipment_ids.contains(id))
        ){continue;}
        let before=event.get("before").filter(|v|!v.is_null());
        let after=event.get("after").filter(|v|!v.is_null());
        let difference=match (before,after) {
            (Some(old),Some(new))=>format!(
                "변경 전 {} → 변경 후 {}",audit_value(old),audit_value(new)
            ),
            (Some(old),None)=>format!("변경 전 {}",audit_value(old)),
            (None,Some(new))=>format!("변경 후 {}",audit_value(new)),
            (None,None)=>String::new()
        };
        let details=[
            bando_data_access::terminology::display_actor(field(event,"actor")).to_owned(),
            bando_data_access::terminology::display_reason(field(event,"reason")).to_owned(),
            difference
        ].into_iter().filter(|text|!text.trim().is_empty())
            .collect::<Vec<_>>().join(" · ");
        output.push(AuditEntry{
            at:field(event,"at").to_owned(),
            title:display_or(bando_data_access::terminology::display_event_action(field(event,"action")),"업무 기록").to_owned(),
            detail:details
        });
    }
    output.sort_by(|a,b|b.at.cmp(&a.at));
    output
}
fn audit_panel(ui:&mut Ui,changes:&[AuditEntry]){
    if changes.is_empty(){
        crate::ui_primitives::management_body_state_inline(ui,"현재 저장된 관련 변경 기록이 없습니다.");
    }
    for change in changes{
        ui.label(typography::subsection_heading(&change.title));
        ui.label(typography::caption(&kst(&change.at)));
        ui.label(typography::body(
            display_or(&change.detail,"세부 기록 없음")
        ));
        ui.add_space(theme::SPACE_8);
    }
}

#[derive(Clone,Copy)]
struct DashboardGrid {
    raw:Rect,changes:Rect,payment:Rect,billing:Rect,shipments:Rect
}
fn dashboard_grid(rect:Rect)->DashboardGrid{
    let left_width=(rect.width()*0.41).max(1.0)
        .min((rect.width()-PANE_GAP-1.0).max(1.0));
    let left=Rect::from_min_size(
        rect.min,Vec2::new(left_width,rect.height())
    );
    let right=Rect::from_min_max(
        egui::pos2(left.right()+PANE_GAP,rect.top()),rect.max
    );
    let raw_height=(left.height()*0.61).max(1.0)
        .min((left.height()-PANE_GAP-1.0).max(1.0));
    let raw=Rect::from_min_size(left.min,Vec2::new(left.width(),raw_height));
    let changes=Rect::from_min_max(
        egui::pos2(left.left(),raw.bottom()+PANE_GAP),left.max
    );
    let info_height=(right.height()*0.40).max(1.0)
        .min((right.height()-PANE_GAP-1.0).max(1.0));
    let info=Rect::from_min_size(
        right.min,Vec2::new(right.width(),info_height)
    );
    let shipments=Rect::from_min_max(
        egui::pos2(right.left(),info.bottom()+PANE_GAP),right.max
    );
    let payment_width=(info.width()*0.55).max(1.0)
        .min((info.width()-PANE_GAP-1.0).max(1.0));
    let payment=Rect::from_min_size(
        info.min,Vec2::new(payment_width,info.height())
    );
    let billing=Rect::from_min_max(
        egui::pos2(payment.right()+PANE_GAP,info.top()),info.max
    );
    DashboardGrid{raw,changes,payment,billing,shipments}
}

/// Returns true only when the top-right close action was selected.
/// All panes share the approved non-Kakao work area, but use local scrolling
/// so long receipts or dozens of shipments never push the other cards away.
pub fn show(ctx:&egui::Context,work_area:Rect,row:&Value,state:&Value)->bool{
    let order=&row["order"];
    let order_id=field(order,"id");
    let shipments=rows(&state["workflow"]["shipments"]).iter()
        .filter(|ship|field(ship,"orderId")==order_id &&
            ship["legacyArchived"]!=true)
        .collect::<Vec<_>>();
    let ids=shipments.iter().map(|ship|field(ship,"id"))
        .collect::<HashSet<_>>();
    let payments=rows(&state["workflow"]["payments"]).iter()
        .filter(|payment|rows(&payment["allocations"]).iter()
            .any(|allocation|ids.contains(field(allocation,"shipmentId"))))
        .collect::<Vec<_>>();
    let confirmations=rows(&state["workflow"]["confirmations"]).iter()
        .filter(|confirm|rows(&confirm["shipmentIds"]).iter()
            .any(|sid|sid.as_str().is_some_and(|id|ids.contains(id))))
        .collect::<Vec<_>>();
    let changes=change_entries(order,state,&ids);
    let (raw_title,raw)=raw_text(order);
    let modal_size=Vec2::new(
        (work_area.width()-2.0*theme::SPACE_8).max(1.0),
        (work_area.height()-2.0*theme::SPACE_8).max(1.0)
    );
    let mut close=false;
    crate::popup::modal_backdrop(ctx,"history-order-detail-backdrop",false);
    crate::popup::modal_window(ctx,"발주서 상세","history-order-detail",work_area)
        .fixed_size(modal_size)
        .show(ctx,|ui|{

            // Reserve a full-window dashboard with a fixed grid, rather than
            // the old single vertical ScrollArea around every information row.
            ui.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Fixed,(modal_size.x-2.0*theme::MODAL_PANEL_PADDING_X-2.0).max(1.0));
            crate::popup::modal_flow_workspace(ui,|workspace|{
                close=workspace.header("발주서 상세","닫기",true);
                workspace.body_region(|ui|{
                    let body=ui.available_rect_before_wrap();
                    let summary_height=74.0_f32.min((body.height()*0.22).max(1.0));
                    let (summary_rect,_)=ui.shared_allocate_exact_size(
                        Vec2::new(body.width(),summary_height),Sense::hover()
                    );
                    overview(ui,summary_rect,row,shipments.len());
                    ui.add_space(PANE_GAP);
                    let remaining=ui.available_rect_before_wrap();
                    let (grid_rect,_)=ui.shared_allocate_exact_size(
                        Vec2::new(remaining.width(),remaining.height().max(1.0)),
                        Sense::hover()
                    );
                    let panes=dashboard_grid(grid_rect);
                    section_panel(ui,panes.raw,raw_title,"raw",order_id,|panel|{
                        if raw.is_empty(){
                            crate::ui_primitives::management_body_state_inline(panel,"보존된 텍스트 원문이 없습니다.");
                        }else{
                            panel.label(typography::body(raw));
                        }
                        if raw_title=="저장된 발주 원문"{
                            panel.add_space(theme::SPACE_8);
                            panel.label(typography::caption(
                                "최초 원문 별도 보관 기록은 확인되지 않습니다."
                            ));
                        }
                        let attachments=rows(&order["photos"]).len();
                        if attachments>0{
                            crate::ui_primitives::section_divider(panel);
                            panel.label(typography::caption(&format!(
                                "첨부 이미지 {}건 · 이미지 미리보기는 아직 연결되지 않았습니다.",
                                attachments
                            )));
                        }
                    });
                    section_panel(ui,panes.changes,
                        &format!("변경·처리 이력 · {}건",changes.len()),"changes",order_id,
                        |panel|audit_panel(panel,&changes)
                    );
                    section_panel(ui,panes.payment,"접수 및 입금 정보","payment",order_id,
                        |panel|payment_panel(panel,order,row,&payments,&confirmations)
                    );
                    section_panel(ui,panes.billing,"청구금액 상세","billing",order_id,
                        |panel|billing_panel(panel,order,&row["billing"])
                    );
                    section_panel(ui,panes.shipments,
                        &format!("발송 건별 상세 · {}건",shipments.len()),"shipments",order_id,
                        |panel|shipment_panel(panel,order,&shipments)
                    );
                });
            });
        });
    close
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layout_has_fixed_independent_panes_and_no_horizontal_overflow(){
        for (width,height) in [(820.0,440.0),(1180.0,650.0),(1490.0,810.0)] {
            let bounds=Rect::from_min_size(
                egui::pos2(12.0,20.0),Vec2::new(width,height)
            );
            let parts=dashboard_grid(bounds);
            for pane in [
                parts.raw,parts.changes,parts.payment,parts.billing,parts.shipments
            ] {
                assert!(pane.width()>0.0&&pane.height()>0.0);
                assert!(bounds.contains(pane.left_top()));
                assert!(bounds.contains(pane.right_bottom()));
            }
            assert!((parts.changes.top()-parts.raw.bottom()-PANE_GAP).abs()<0.01);
            assert!((parts.billing.left()-parts.payment.right()-PANE_GAP).abs()<0.01);
            assert!((parts.shipments.top()-parts.payment.bottom()-PANE_GAP).abs()<0.01);
            assert!((parts.shipments.left()-parts.raw.right()-PANE_GAP).abs()<0.01);
            assert!((parts.shipments.bottom()-bounds.bottom()).abs()<0.01);
        }
    }

    #[test]
    fn original_order_text_is_preferred_and_not_overwritten_by_edits(){
        let order=json!({
            "raw":"현재 수정된 원문",
            "receipt":{"original":"최초 접수 원문"},
            "confirmationSnapshot":{"raw":"주문 확인 원문"}
        });
        assert_eq!(raw_text(&order),("최초 발주 원문","최초 접수 원문"));
        let confirmed=json!({"raw":"현재 원문",
            "confirmationSnapshot":{"raw":"주문 확인 원문"}});
        assert_eq!(raw_text(&confirmed),("주문 확인 당시 원문","주문 확인 원문"));
        assert_eq!(raw_text(&json!({"raw":"저장 원문"})),
            ("저장된 발주 원문","저장 원문"));
    }

    #[test]
    fn history_shows_only_linked_shipment_events_newest_first(){
        let state=json!({"workflow":{"events":[
            {"at":"2026-11-19T11:00:00Z","action":"재검증","shipmentIds":["s1"]},
            {"at":"2026-11-20T09:00:00Z","action":"외부 주문","shipmentIds":["s2"]},
            {"at":"2026-11-21T11:00:00Z","action":"확인","shipmentIds":["s1"]}
        ]}});
        let ids=HashSet::from(["s1"]);
        let entries=change_entries(&json!({}),&state,&ids);
        assert_eq!(entries.len(),2);
        assert_eq!(entries[0].title,"확인");
        assert_eq!(entries[1].title,"재검증");
    }
}
