//! Daily settlement: read-only native Rust projection of the existing
//! order-history -> output.prepare_settlement pipeline.
//! No egui totals, duplicate accounting, local receipt cache or mutations.
use crate::{
    components, gateway::{BandoGateway,ReadOperation},
    settings_help,theme,typography,
};
use chrono::{DateTime,Duration as Days,FixedOffset,NaiveDate,Utc};
use eframe::egui::{self,Align,Align2,Rect,Sense,Stroke,Ui,UiBuilder,Vec2};
use serde_json::{json,Value};
use std::sync::{mpsc::{self,Receiver,TryRecvError},Arc};

const ROW_HEIGHT:f32=theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT;
const HEADER_HEIGHT:f32=theme::MANAGEMENT_TABLE_HEADER_HEIGHT;
const COL_GAP:f32=theme::SPACE_8;

fn field<'a>(row:&'a Value,key:&str)->&'a str{
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value:&Value)->&[Value]{
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn won(value:&Value)->String{
    value.as_i64().map(components::display_won)
        .unwrap_or_else(||"확인 필요".to_owned())
}
fn summary_won(value:&Value)->String{
    value.as_i64().map(components::display_won)
        .unwrap_or_else(||"확인 필요".to_owned())
}
fn seoul_today()->String{
    Utc::now().with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
        .format("%Y-%m-%d").to_string()
}
fn valid_date(date:&str)->bool{
    NaiveDate::parse_from_str(date,"%Y-%m-%d")
        .is_ok_and(|d|d.format("%Y-%m-%d").to_string()==date)
}
fn move_date(date:&str,days:i64)->String{
    let fallback=seoul_today();
    let base=if valid_date(date){date}else{&fallback};
    NaiveDate::parse_from_str(base,"%Y-%m-%d").ok()
        .and_then(|day|day.checked_add_signed(Days::days(days)))
        .map(|day|day.format("%Y-%m-%d").to_string())
        .unwrap_or_else(||base.to_owned())
}
fn seoul_datetime(raw:&str)->String{
    if let Ok(date)=DateTime::parse_from_rfc3339(raw){
        return date.with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
            .format("%Y-%m-%d %H:%M").to_string();
    }
    // Never fabricate a local calendar time from an unparsable bank record.
    raw.to_owned()
}
fn money_key(row:&Value,key:&str)->Option<i64>{
    row.get(key).and_then(Value::as_i64)
}
fn compare_rows(
    left:&Value,right:&Value,column:usize,payment:bool
)->std::cmp::Ordering{
    if payment {
        match column {
            0=>field(left,"paidAt").cmp(field(right,"paidAt")),
            1=>field(left,"dealer").to_lowercase()
                .cmp(&field(right,"dealer").to_lowercase()),
            2=>field(left,"account").to_lowercase()
                .cmp(&field(right,"account").to_lowercase()),
            3=>money_key(left,"amount").cmp(&money_key(right,"amount")),
            4=>money_key(left,"allocated").cmp(&money_key(right,"allocated")),
            5=>money_key(left,"unallocated").cmp(&money_key(right,"unallocated")),
            6=>field(left,"memo").to_lowercase()
                .cmp(&field(right,"memo").to_lowercase()),
            _=>std::cmp::Ordering::Equal,
        }
    }else{
        match column {
            0=>field(left,"clientName").to_lowercase()
                .cmp(&field(right,"clientName").to_lowercase()),
            1=>field(left,"paymentTerms").cmp(field(right,"paymentTerms")),
            2=>money_key(left,"subtotal").cmp(&money_key(right,"subtotal")),
            3=>money_key(left,"shippingCharge").cmp(&money_key(right,"shippingCharge")),
            4=>money_key(left,"vat").cmp(&money_key(right,"vat")),
            5=>money_key(left,"total").cmp(&money_key(right,"total")),
            6=>money_key(left,"paidAllocated").cmp(&money_key(right,"paidAllocated")),
            7=>money_key(left,"outstanding").cmp(&money_key(right,"outstanding")),
            _=>std::cmp::Ordering::Equal,
        }
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Direction{Ascending,Descending}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Sort{column:usize,direction:Direction}
impl Sort{
    // Reference: default ↕ -> ascending ▲ -> descending ▼ -> default ↕.
    fn next(previous:Option<Self>,column:usize)->Option<Self>{
        match previous {
            Some(Self{column:c,direction:Direction::Ascending}) if c==column=>
                Some(Self{column,direction:Direction::Descending}),
            Some(Self{column:c,direction:Direction::Descending}) if c==column=>None,
            _=>Some(Self{column,direction:Direction::Ascending}),
        }
    }
}
fn sorted_indices(data:&[Value],sort:Option<Sort>,payment:bool)->Vec<usize>{
    let mut indices=(0..data.len()).collect::<Vec<_>>();
    if let Some(sort)=sort {
        indices.sort_by(|a,b|{
            let order=compare_rows(&data[*a],&data[*b],sort.column,payment);
            let order=if sort.direction==Direction::Descending{
                order.reverse()
            }else{order};
            order.then_with(||a.cmp(b))
        });
    }
    indices
}
fn fetch(gateway:&dyn BandoGateway,date:&str)->Result<Value,String>{
    if !valid_date(date){return Err("결산 날짜를 확인하세요.".into());}
    // One snapshot for BOTH consecutive authoritative native Rust reads.
    // Never join separately loaded revisions or calculate amounts here.
    let state=gateway.load_business_state()?;
    let history=gateway.execute(ReadOperation::ListOrderHistory,json!({
        "state":&state,"query":"","date":date
    }))?;
    let report=gateway.execute(ReadOperation::PrepareSettlement,json!({
        "state":&state,"date":date,"historyRows":history
    }))?;
    if report["date"].as_str()!=Some(date) ||
        report.get("orderRows").and_then(Value::as_array).is_none() ||
        report.get("paymentRows").and_then(Value::as_array).is_none() ||
        report.get("warnings").and_then(Value::as_array).is_none() ||
        report["revision"]!=state["revision"]{
        return Err("Rust 결산 결과의 날짜·리비전·내역 형식을 확인할 수 없습니다.".into());
    }
    Ok(report)
}

/// Preserve the approved numeric-first KPI identity. Monetary cards are
/// wider than freight counts so actual amounts remain legible.
fn kpi(ui:&mut Ui,label:&str,value:&str){
    let width=theme::SETTLEMENT_KPI_CARD_WIDTH;
    ui.allocate_ui_with_layout(
        Vec2::new(width,theme::FREIGHT_SUMMARY_CARD_HEIGHT),
        egui::Layout::top_down(Align::Min),
        |slot|{
            slot.set_width(width);
            theme::freight_summary_frame().show(slot,|cell|{
                cell.spacing_mut().item_spacing.y=0.0;
                let content_width=width-2.0*theme::FREIGHT_SUMMARY_PADDING_X
                    -2.0*theme::FRAME_STROKE_WIDTH;
                let (number_rect,number_response)=cell.allocate_exact_size(
                    Vec2::new(content_width,theme::FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT),
                    Sense::hover()
                );
                cell.add_space(theme::FREIGHT_SUMMARY_TEXT_GAP);
                let (label_rect,_)=cell.allocate_exact_size(
                    Vec2::new(content_width,theme::FREIGHT_SUMMARY_LABEL_LINE_HEIGHT),
                    Sense::hover()
                );
                let value_galley=typography::freight_summary_large_number_galley(
                    cell,value
                );
                let label_galley=typography::freight_summary_small_label_galley(
                    cell,label
                );
                cell.painter().with_clip_rect(number_rect).galley(
                    egui::pos2(
                        number_rect.center().x-value_galley.size().x/2.0,
                        number_rect.center().y-value_galley.size().y/2.0,
                    ),value_galley,theme::TEXT
                );
                cell.painter().with_clip_rect(label_rect).galley(
                    egui::pos2(
                        label_rect.center().x-label_galley.size().x/2.0,
                        label_rect.center().y-label_galley.size().y/2.0,
                    ),label_galley,theme::SECONDARY
                );
                number_response.on_hover_text(value);
            });
        }
    );
}

fn metric(ui:&mut Ui,label:&str,value:String){
    ui.horizontal(|line|{
        line.spacing_mut().item_spacing.x=theme::SPACE_4;
        line.label(typography::field_label(label));
        line.label(typography::body(&value));
    });
}
fn cells<const N:usize>(
    origin:egui::Pos2,widths:&[f32;N],height:f32
)->[Rect;N]{
    let mut x=origin.x;
    std::array::from_fn(|index|{
        let r=Rect::from_min_size(
            egui::pos2(x,origin.y),Vec2::new(widths[index],height));
        x+=widths[index]+COL_GAP;
        r
    })
}
fn paint_cell(
    ui:&Ui,rect:Rect,content:&str,align:Align2,id:impl std::hash::Hash,
){
    let padded=rect.shrink2(Vec2::new(theme::SPACE_8,0.0));
    let visible=padded.intersect(ui.clip_rect());
    if visible.width()<=0.0 || visible.height()<=0.0{return;}
    let text=typography::freight_table_cell_galley(
        ui,content,false,theme::TEXT
    );
    let x=if align==Align2::RIGHT_CENTER{
        padded.right()-text.size().x
    }else if align==Align2::CENTER_CENTER{
        padded.center().x-text.size().x/2.0
    }else{padded.left()};
    ui.painter().with_clip_rect(visible).galley(
        egui::pos2(x,padded.center().y-text.size().y/2.0),text,theme::TEXT
    );
    if !content.is_empty(){
        ui.interact(
            visible,ui.make_persistent_id(("settlement-cell",id)),
            Sense::hover()
        ).on_hover_text(content);
    }
}

/// One record per row with one header/row geometry. Header stays pinned
/// while only records scroll. Empty reports retain the column header.
#[allow(clippy::too_many_arguments)]
fn report_table<const N:usize>(
    ui:&mut Ui,id:&str,headings:[&str;N],base:[f32;N],
    align:[Align2;N],stretch_column:usize,indices:&[usize],
    sort:&mut Option<Sort>,format_row:impl Fn(usize)->[String;N],
){
    let previous_gap=ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y=0.0;
    let viewport=ui.available_width().max(1.0);
    let reserved=ui.spacing().scroll.allocated_width()+theme::SPACE_4;
    let minimum=base.iter().sum::<f32>()+(N.saturating_sub(1)) as f32*COL_GAP;
    let width=(viewport-reserved).max(minimum);
    let mut widths=base;
    widths[stretch_column]+=(width-minimum).max(0.0);
    let (header,_)=ui.allocate_exact_size(
        Vec2::new(viewport,HEADER_HEIGHT),Sense::hover()
    );
    let mut first_x=None;
    let scroll=components::virtual_fixed_rows(
        egui::ScrollArea::both().id_salt(id)
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
            )
            .max_height(ui.ctx().content_rect().height()
                *theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
            .auto_shrink([false,false]),
        ui,width,ROW_HEIGHT,indices.len(),
        |list,visible_rows|{
            list.set_min_width(width);
            list.spacing_mut().item_spacing.y=0.0;
            let first=visible_rows.start;
            let origin=list.max_rect().min;
            for index in visible_rows{
                let rect=Rect::from_min_size(
                    egui::pos2(origin.x,origin.y+(index-first) as f32*ROW_HEIGHT),
                    Vec2::new(width,ROW_HEIGHT)
                );
                first_x.get_or_insert(rect.left());
                list.scope_builder(
                    UiBuilder::new().id_salt((id,index))
                        .max_rect(rect)
                        .layout(egui::Layout::top_down(Align::Min)),
                    |row|{
                        row.allocate_exact_size(
                            Vec2::new(width,ROW_HEIGHT),Sense::hover()
                        );
                        let row_cells=cells(rect.min,&widths,ROW_HEIGHT);
                        row.painter().rect_filled(
                            rect,0.0,if index%2==0{
                                theme::PANEL
                            }else{theme::TABLE_ROW_ALT_BG}
                        );
                        let data=format_row(indices[index]);
                        for (column,cell) in row_cells.iter().enumerate(){
                            paint_cell(
                                row,*cell,&data[column],align[column],
                                (id,indices[index],column),
                            );
                            if column+1<N{
                                let x=cell.right()+COL_GAP/2.0;
                                row.painter().line_segment(
                                    [egui::pos2(x,rect.top()),egui::pos2(x,rect.bottom())],
                                    Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
                                );
                            }
                        }
                        row.painter().line_segment(
                            [rect.left_bottom(),rect.right_bottom()],
                            Stroke::new(
                                theme::SOFT_DIVIDER_WIDTH,
                                theme::DIVIDER.linear_multiply(0.70)
                            )
                        );
                    }
                );
            }
        }
    );
    ui.spacing_mut().item_spacing.y=previous_gap;
    let left=first_x.unwrap_or(header.left()-scroll.state.offset.x);
    let header_cells=cells(egui::pos2(left,header.top()),&widths,HEADER_HEIGHT);
    let painter=ui.painter().with_clip_rect(header);
    painter.rect_filled(header,0.0,theme::RAISED);
    let mut sort_change=None;
    for (column,title) in headings.iter().enumerate(){
        let cell=header_cells[column];
        let glyph=match sort.filter(|v|v.column==column).map(|v|v.direction){
            Some(Direction::Ascending)=>"▲",
            Some(Direction::Descending)=>"▼",
            None=>"↕"
        };
        let galley=typography::freight_table_header_galley(
            ui,&format!("{title} {glyph}")
        );
        let x=if align[column]==Align2::RIGHT_CENTER{
            cell.right()-theme::SPACE_8-galley.size().x
        }else if align[column]==Align2::CENTER_CENTER{
            cell.center().x-galley.size().x/2.0
        }else{
            cell.left()+theme::SPACE_8
        };
        let clip=cell.intersect(header);
        painter.with_clip_rect(clip).galley(
            egui::pos2(x,cell.center().y-galley.size().y/2.0),
            galley,theme::SECONDARY
        );
        if clip.width()>0.0 && clip.height()>0.0{
            if ui.interact(
                clip,ui.make_persistent_id((id,"sort",column)),
                Sense::click()
            ).clicked(){
                sort_change=Some(Sort::next(*sort,column));
            }
        }
        if column+1<N{
            let x=cell.right()+COL_GAP/2.0;
            painter.line_segment(
                [egui::pos2(x,header.top()),egui::pos2(x,header.bottom())],
                Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
            );
        }
    }
    if let Some(next)=sort_change{*sort=next;}
    painter.line_segment(
        [header.left_bottom(),header.right_bottom()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
    );
}

pub struct DailySettlementManager{
    gateway:Arc<dyn BandoGateway>,
    receiver:Option<Receiver<Result<Value,String>>>,
    report:Option<Value>,
    error:Option<String>,
    date:String,
    date_input:String,
    order_sort:Option<Sort>,
    payment_sort:Option<Sort>,
}
impl DailySettlementManager{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let today=seoul_today();
        let mut manager=Self{
            gateway,receiver:None,report:None,error:None,
            date:today.clone(),date_input:today,
            order_sort:None,payment_sort:None,
        };
        manager.refresh(ctx);
        manager
    }
    pub fn refresh(&mut self,ctx:egui::Context){
        self.report=None;
        self.error=None;
        let (sender,receiver)=mpsc::channel();
        self.receiver=Some(receiver);
        let gateway=Arc::clone(&self.gateway);
        let date=self.date.clone();
        std::thread::spawn(move||{
            let answer=fetch(gateway.as_ref(),&date);
            let _=sender.send(answer);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self){
        let current=self.receiver.as_ref().map(Receiver::try_recv);
        match current{
            Some(Ok(Ok(report)))=>{
                // Old async results are unreachable because refresh drops the
                // previous receiver. Still verify the requested date.
                if report["date"].as_str()==Some(self.date.as_str()){
                    self.report=Some(report);
                    self.error=None;
                }
                self.receiver=None;
            },
            Some(Ok(Err(error)))=>{
                self.report=None;self.error=Some(error);self.receiver=None;
            },
            Some(Err(TryRecvError::Disconnected))=>{
                self.report=None;self.receiver=None;
                self.error=Some("일일 결산 조회 연결이 중단되었습니다.".into());
            },
            _=>{}
        }
    }
    pub fn show_page(&mut self,ui:&mut Ui){
        ui.spacing_mut().scroll=theme::management_scroll_style();
        self.poll();
        ui.label(typography::page_heading("일일 결산"));
        ui.add_space(theme::SPACE_12);
        // The only date control is the original prev/date/next navigator.
        // In particular, OrderHistory's separately-approved calendar is not
        // copied into this page.
        let mut date_changed=false;
        theme::management_list_frame().show(ui,|toolbar|{
            theme::management_list_strip_frame().show(toolbar,|line|{
                line.horizontal(|controls|{
                    controls.spacing_mut().item_spacing.x=theme::SPACE_8;
                    controls.label(typography::field_label("결산 날짜"));
                    if theme::action(
                        controls,"‹",theme::ButtonRole::Standard,true
                    ).clicked(){
                        self.date=move_date(&self.date,-1);
                        self.date_input=self.date.clone();
                        date_changed=true;
                    }
                    if components::flexible_text_field(
                        controls,&mut self.date_input,
                        theme::SETTLEMENT_DATE_FIELD_WIDTH,"YYYY-MM-DD"
                    ).changed(){
                        if valid_date(&self.date_input) &&
                            self.date!=self.date_input {
                            self.date=self.date_input.clone();
                            date_changed=true;
                        }
                    }
                    if theme::action(
                        controls,"›",theme::ButtonRole::Standard,true
                    ).clicked(){
                        self.date=move_date(&self.date,1);
                        self.date_input=self.date.clone();
                        date_changed=true;
                    }
                });
                if !valid_date(&self.date_input){
                    line.label(typography::status(
                        "결산 날짜는 YYYY-MM-DD 형식으로 입력하세요.",
                        theme::DANGER
                    ));
                }
            });
        });
        if date_changed{self.refresh(ui.ctx().clone());}
        if let Some(error)=self.error.as_deref(){
            ui.label(typography::status(error,theme::DANGER));
        }
        if self.receiver.is_some(){
            ui.label(typography::caption("결산 자료 확인 중"));
        }
        let Some(report)=self.report.as_ref() else{return;};
        let orders=rows(&report["orderRows"]);
        let payments=rows(&report["paymentRows"]);
        // The four native Rust summary values are NOT interchangeable:
        // billed totals are reception-day values; cash is bank-deposit day.
        ui.add_space(theme::SPACE_16);
        ui.horizontal_wrapped(|strip|{
            strip.spacing_mut().item_spacing=
                Vec2::new(theme::SPACE_8,theme::SPACE_8);
            kpi(strip,"접수 주문",&format!("{}건",orders.len()));
            kpi(strip,"청구 합계 확인분",&summary_won(&report["billedKnown"]));
            kpi(strip,"실제 계좌 입금",&summary_won(&report["paidCash"]));
            kpi(strip,"미확정 청구 주문",&format!(
                "{}건",report["billedUnknown"].as_u64().unwrap_or(0)
            ));
        });
        ui.add_space(theme::SPACE_16);
        egui::ScrollArea::vertical()
            .id_salt("daily-settlement-sections")
            .auto_shrink([false,false])
            .show(ui,|body|{
                theme::section_frame().show(body,|section|{
                    section.horizontal(|heading|{
                        heading.label(typography::section_heading(
                            &format!("{} / 일일 결산 조회",self.date)
                        ));
                        settings_help::icon(heading,"일일 결산");
                    });
                    components::soft_divider(section);
                    section.label(typography::subsection_heading(
                        "해당 날짜 접수 주문 / 청구 내역"
                    ));
                    section.add_space(theme::SPACE_8);
                    section.horizontal_wrapped(|line|{
                        line.spacing_mut().item_spacing=Vec2::new(
                            theme::SPACE_16,theme::SPACE_8
                        );
                        for (title,key) in [
                            ("상품대금 확인분","productKnown"),
                            ("고객 청구 배송비 확인분","shippingKnown"),
                            ("부가세 확인분","vatKnown"),
                            ("후불 주문 청구액 확인분","postpaidKnown"),
                            ("현재 미수 확인분","outstandingKnown"),
                            ("현재 공장 운임 확인분","actualFreightKnown"),
                        ]{
                            metric(line,title,summary_won(&report[key]));
                        }
                    });
                    section.add_space(theme::SPACE_8);
                    section.horizontal(|review|{
                        review.spacing_mut().item_spacing.x=theme::SPACE_12;
                        review.label(typography::caption(&format!(
                            "미수 미확정 {}건",
                            report["outstandingUnknown"].as_u64().unwrap_or(0)
                        )));
                        review.label(typography::caption(&format!(
                            "실제 운임 미확정 {}건",
                            report["freightUnknown"].as_u64().unwrap_or(0)
                        )));
                        settings_help::icon(review,"미수금과 실제 운임");
                    });
                    section.add_space(theme::SPACE_12);
                    components::soft_divider(section);
                    let indices=sorted_indices(orders,self.order_sort,false);
                    report_table(
                        section,"settlement-orders",
                        ["거래처 / 상태","결제 방식","상품대금","청구 배송비",
                         "부가세","청구 합계","실제 입금 배분 누계","현재 미수"],
                        theme::SETTLEMENT_ORDER_COL_WIDTHS,
                        [Align2::LEFT_CENTER,Align2::LEFT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER],
                        0,&indices,&mut self.order_sort,|index|{
                            let order=&orders[index];
                            [
                                format!("{} · {}",field(order,"clientName"),field(order,"status")),
                                field(order,"paymentTerms").to_owned(),
                                won(&order["subtotal"]),
                                won(&order["shippingCharge"]),
                                won(&order["vat"]),
                                won(&order["total"]),
                                won(&order["paidAllocated"]),
                                won(&order["outstanding"]),
                            ]
                        }
                    );
                    if orders.is_empty(){
                        section.add_space(theme::SPACE_8);
                        section.label(typography::caption(
                            "해당 날짜에 접수한 주문이 없습니다."
                        ));
                    }
                });
                body.add_space(theme::SPACE_12);
                theme::section_frame().show(body,|section|{
                    section.label(typography::section_heading(
                        "실제 입금 내역 / 입금 일시 기준"
                    ));
                    components::soft_divider(section);
                    section.horizontal_wrapped(|line|{
                        line.spacing_mut().item_spacing=Vec2::new(
                            theme::SPACE_16,theme::SPACE_8
                        );
                        for (title,key) in [
                            ("실제 입금 합계","paidCash"),
                            ("배송 건 배분액","allocatedCash"),
                            ("미배분 입금액","unallocatedCash"),
                        ]{
                            metric(line,title,summary_won(&report[key]));
                        }
                    });
                    section.add_space(theme::SPACE_12);
                    components::soft_divider(section);
                    let indices=sorted_indices(payments,self.payment_sort,true);
                    report_table(
                        section,"settlement-payments",
                        ["입금 일시","거래처","계좌","실제 입금액",
                         "배분액","미배분","메모"],
                        theme::SETTLEMENT_PAYMENT_COL_WIDTHS,
                        [Align2::LEFT_CENTER,Align2::LEFT_CENTER,Align2::LEFT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::LEFT_CENTER],
                        6,&indices,&mut self.payment_sort,|index|{
                            let payment=&payments[index];
                            [
                                seoul_datetime(field(payment,"paidAt")),
                                field(payment,"dealer").to_owned(),
                                field(payment,"account").to_owned(),
                                won(&payment["amount"]),
                                won(&payment["allocated"]),
                                won(&payment["unallocated"]),
                                field(payment,"memo").to_owned(),
                            ]
                        }
                    );
                    if payments.is_empty(){
                        section.add_space(theme::SPACE_8);
                        section.label(typography::caption(
                            "해당 날짜의 실제 입금 기록이 없습니다."
                        ));
                    }
                });
                if let Some(warnings)=report["warnings"].as_array()
                    .filter(|warnings|!warnings.is_empty()){
                    body.add_space(theme::SPACE_12);
                    theme::section_frame().show(body,|review|{
                        review.label(typography::section_heading("결산 검토 사항"));
                        components::soft_divider(review);
                        for warning in warnings{
                            if let Some(message)=warning.as_str(){
                                review.label(typography::body(message));
                            }
                        }
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn settlement_dates_and_sort_are_presentation_only(){
        assert!(valid_date("2026-11-20"));
        assert!(!valid_date("2026-02-30"));
        assert!(!valid_date("2026-2-3"));
        assert_eq!(move_date("2024-03-01",-1),"2024-02-29");
        assert_eq!(seoul_datetime("2026-11-19T16:00:00Z"),
                   "2026-11-20 01:00");
        assert_eq!(won(&Value::Null),"확인 필요");
        assert_eq!(Sort::next(None,2),
            Some(Sort{column:2,direction:Direction::Ascending}));
        assert_eq!(Sort::next(
            Some(Sort{column:2,direction:Direction::Ascending}),2
        ),Some(Sort{column:2,direction:Direction::Descending}));
        assert_eq!(Sort::next(
            Some(Sort{column:2,direction:Direction::Descending}),2
        ),None);
    }
}
