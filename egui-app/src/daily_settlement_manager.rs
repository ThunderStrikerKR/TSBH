//! Daily settlement: read-only native Rust projection of the existing
//! order-history -> output.prepare_settlement pipeline.
//! No egui totals, duplicate accounting, local receipt cache or mutations.
use crate::ui_layout::geometry::UiWidthPolicyExt as _;
use crate::{
    gateway::{BandoGateway,ReadOperation},
    settings_help,theme,typography,
};
use chrono::{DateTime,FixedOffset,NaiveDate,Utc};
use eframe::egui::{self,Align2,Rect,Ui,Vec2};
use serde_json::{json,Value};
use std::sync::Arc;

const ROW_HEIGHT:f32=theme::MANAGEMENT_COMPACT_ROW_HEIGHT;
const HEADER_HEIGHT:f32=theme::MANAGEMENT_TABLE_HEADER_HEIGHT;
const COL_GAP:f32=theme::SPACE_8;

fn field<'a>(row:&'a Value,key:&str)->&'a str{
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value:&Value)->&[Value]{
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn won(value:&Value)->String{
    value.as_i64().map(crate::display_format::display_won)
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
#[cfg(test)]
fn move_date(date:&str,days:i64)->String{
    crate::navigation::range_move_date(date,days)
}
/// Keep the report date valid while the shared date control is being edited.
fn settlement_date_toolbar(
    ui:&mut Ui,date:&mut String,date_input:&mut String,
    calendar_month:&mut NaiveDate
)->bool{
    let mut changed=false;
    ui.horizontal_wrapped(|controls|{
        crate::ui_layout::apply_spacing_role(controls,crate::ui_layout::SpacingRole::Toolbar);
        crate::navigation::apply_management_date_control_spacing(controls);
        crate::ui_primitives::form_field_label(controls,"결산 날짜");
        if crate::navigation::range_date_picker_with_notice(
            controls,false,date_input,calendar_month,
            "daily-settlement-date-calendar","settlement.date"
        ) && valid_date(date_input) && date!=date_input{
            *date=date_input.clone();
            changed=true;
        }
    });
    if !valid_date(date_input){
        crate::ui_layout::fixed_status_slot(ui,ui.available_width(),
            typography::METADATA_LINE_HEIGHT,
            Some("결산 날짜는 YYYY-MM-DD 형식으로 입력하세요."),
            None,theme::message_ink(theme::MessageInkRole::Danger));
    }
    changed
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
fn payment_time_key(row:&Value)->Option<DateTime<FixedOffset>>{
    DateTime::parse_from_rfc3339(field(row,"paidAt")).ok()
}
fn sort_unknown(row:&Value,column:usize,payment:bool)->bool{
    if payment{
        if column==0{return payment_time_key(row).is_none();}
        ["amount","allocated","unallocated"].get(column.wrapping_sub(3))
            .is_some_and(|key|money_key(row,key).is_none())
    }else{
        ["subtotal","shippingCharge","vat","total","paidAllocated","outstanding"]
            .get(column.wrapping_sub(3)).is_some_and(|key|money_key(row,key).is_none())
    }
}
fn compare_rows(
    left:&Value,right:&Value,column:usize,payment:bool
)->std::cmp::Ordering{
    if payment {
        match column {
            0=>payment_time_key(left).cmp(&payment_time_key(right)),
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
            1=>field(left,"status").cmp(field(right,"status")),
            2=>field(left,"paymentTerms").cmp(field(right,"paymentTerms")),
            3=>money_key(left,"subtotal").cmp(&money_key(right,"subtotal")),
            4=>money_key(left,"shippingCharge").cmp(&money_key(right,"shippingCharge")),
            5=>money_key(left,"vat").cmp(&money_key(right,"vat")),
            6=>money_key(left,"total").cmp(&money_key(right,"total")),
            7=>money_key(left,"paidAllocated").cmp(&money_key(right,"paidAllocated")),
            8=>money_key(left,"outstanding").cmp(&money_key(right,"outstanding")),
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
            sort_unknown(&data[*a],sort.column,payment)
                .cmp(&sort_unknown(&data[*b],sort.column,payment))
                .then(order).then_with(||a.cmp(b))
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
        return Err("결산 정보를 불러오지 못했습니다.".into());
    }
    Ok(report)
}

/// Preserve the approved numeric-first KPI identity. Monetary cards are
/// wider than freight counts so actual amounts remain legible.
fn kpi(ui:&mut Ui,label:&str,value:&str,state:theme::TextState){
    crate::card::management_summary_card_with_role_and_state(
        ui,label,value,crate::card::SummaryCardRole::Settlement,state
    );
}

fn metric(ui:&mut Ui,label:&str,value:String,state:theme::TextState){
    ui.vertical(|line|{
        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ReportMetricPair);
        line.label(typography::field_label(label));
        // Preserve the original 22pt body line box while displaying a
        // standalone monetary figure in the shared numeric data face.
        line.label(typography::data_value_text_with_state(
            &value,typography::DataValueKind::Money,
            typography::DataValueContext::Form,state
        ).line_height(Some(22.0)));
    });
}

fn paint_cell(
    ui:&Ui,rect:Rect,content:&str,align:Align2,
    role:typography::ManagementTableCellRole,id:impl std::hash::Hash,state:theme::TextState,
){
    crate::table::paint_management_text_cell_with_state(
        ui,rect.shrink2(Vec2::new(theme::SPACE_8,0.0)),content,content,role,
        align,("settlement-cell",id),false,state
    );
}

fn report_body_height(count:usize,screen_height:f32,horizontal_rail:f32)->f32{
    let cap=(screen_height*theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION/ROW_HEIGHT)
        .floor().max(1.0) as usize;
    count.max(1).min(cap) as f32*ROW_HEIGHT+horizontal_rail
}

/// One record per row with one header/row geometry. Header stays pinned
/// while only records scroll. Empty reports retain the column header.
#[allow(clippy::too_many_arguments)]
fn report_table<const N:usize>(
    ui:&mut Ui,id:&str,headings:[&str;N],base:[f32;N],
    align:[Align2;N],roles:[typography::ManagementTableCellRole;N],
    stretch_column:usize,indices:&[usize],ready:bool,scroll_keys:&dyn Fn(usize)->Option<egui::Id>,
    sort:&mut Option<Sort>,empty_notice:Option<&str>,format_row:impl Fn(usize)->[String;N],
    row_states:impl Fn(usize)->[theme::TextState;N],
){
    let previous_gap=crate::table::management_table_container_style(ui);
    let minimum=base.iter().sum::<f32>()+(N.saturating_sub(1)) as f32*COL_GAP;
    // Keep existing column minima; the shared 36pt header now uses the
    // same zero-extra scrollbar reservation as both master tables.
    crate::table::show(ui,crate::table::Header::List(minimum),|ui,layout|{
    let width=layout.table_width;
    let widths=theme::management_single_stretch_columns(
        base,width,COL_GAP,stretch_column
    );
    let header=layout.header_rect;
    // This report has multiple tables below each other: the track ends
    // at its OWN row viewport and cannot extend into later sections.
    let horizontal_rail=if width>layout.viewport_width-layout.scrollbar_gutter+0.01{
        ui.spacing().scroll.allocated_width()
    }else{0.0};
    let scroll_max_height=report_body_height(indices.len(),
        ui.ctx().content_rect().height(),horizontal_rail);
    // A nested report can start below the outer scroll viewport. Reserve
    // its logical row slot independently of the parent's remaining visible
    // height; the inherited clip still hides off-screen rows.
    let mut first_x=None;
    let slot_width=ui.available_width();
    let (_,scroll)=crate::ui_layout::fixed_row(ui,Vec2::new(slot_width,scroll_max_height),
        egui::Layout::top_down(egui::Align::Min),|body|{
    let scroll_track=crate::table::management_table_scroll_track_rect(body,scroll_max_height);
    crate::table::fixed_rows_snapshot(
                body,crate::table::Viewport{width:width,height:scroll_max_height,rail:Some(scroll_track)},
                id,ROW_HEIGHT,indices.len(),ready,Some(scroll_keys),
                |list,visible_rows|{
            crate::table::management_table_row_body(list,width);
            let first=visible_rows.start;
            let origin=list.max_rect().min;
            for index in visible_rows{
                let rect=crate::table::fixed_row_rect(origin,index,first,width,ROW_HEIGHT);
                #[cfg(test)]
                crate::ui_test_support::record(if id=="settlement-orders"{"settlement order row"}else{"settlement payment row"},rect);
                first_x.get_or_insert(rect.left());
                crate::table::fixed_row(list,(id,index),rect,index,&widths,COL_GAP,crate::table::RowTone::Alternating,|row,row_cells|{
                        let data=format_row(indices[index]);
                        let states=row_states(indices[index]);
                        for (column,cell) in row_cells.iter().enumerate(){
                            paint_cell(
                                row,*cell,&data[column],align[column],
                                roles[column],(id,indices[index],column),states[column],
                            );
                        }
                        
                    }
                );
            }
        }
    )
    });
    crate::status_notice::management_body_state_overlay(ui,scroll.inner_rect,empty_notice);
    crate::table::restore_management_table_spacing(ui,previous_gap);
    let left=first_x.unwrap_or(header.left()-scroll.state.offset.x);
    let header_cells=crate::table::management_table_cells(egui::pos2(left,header.top()),&widths,HEADER_HEIGHT,COL_GAP);
    crate::table::header_columns(ui,header,layout.header_content_rect,&header_cells,COL_GAP,|ui,header_text_rect|{
    let mut sort_change=None;
    for (column,title) in headings.iter().enumerate(){
        let cell=header_cells[column];
        let glyph=match (*sort).filter(|v|v.column==column).map(|v|v.direction){
            Some(Direction::Ascending)=>"▲",
            Some(Direction::Descending)=>"▼",
            None=>"↕"
        };
        let galley=typography::management_table_header_galley(
            ui,&format!("{title} {glyph}")
        );
        let clip=cell.intersect(header_text_rect);
        let alignment=crate::table::heading_alignment(align[column],theme::SPACE_8,theme::SPACE_8);
        crate::table::heading(ui,cell,header_text_rect,clip,alignment,galley,None);
        if crate::table::sort_clicked(ui,clip,(id,"sort",column)){
            sort_change=Some(Sort::next(*sort,column));
        }
    }
    
    if let Some(next)=sort_change{*sort=next;}
    
                ()
            });
    
            (scroll.inner_rect,())
        });

}

pub struct DailySettlementManager{
    gateway:Arc<dyn BandoGateway>,
    refresh_job:crate::screen_refresh::ReadRefresh<Value>,
    report:Option<Value>,
    order_cache:crate::screen_refresh::ViewProjectionCache<(Option<Sort>,bool),Vec<usize>>,
    payment_cache:crate::screen_refresh::ViewProjectionCache<(Option<Sort>,bool),Vec<usize>>,
    error:Option<String>,
    date:String,
    date_input:String,
    calendar_month:NaiveDate,
    order_sort:Option<Sort>,
    payment_sort:Option<Sort>,
}
impl DailySettlementManager{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let today=seoul_today();
        let mut manager=Self{
            gateway,refresh_job:Default::default(),report:None,order_cache:Default::default(),payment_cache:Default::default(),error:None,
            date:today.clone(),date_input:today.clone(),
            calendar_month:crate::navigation::calendar_month_start(
                NaiveDate::parse_from_str(&today,"%Y-%m-%d")
                    .expect("today in YYYY-MM-DD")
            ),
            order_sort:None,payment_sort:None,
        };
        manager.refresh(ctx);
        manager
    }
    pub(crate) fn reset_navigation(&mut self,ctx:egui::Context){
        let today=seoul_today();
        let changed=self.date!=today;
        self.date=today.clone();self.date_input=today.clone();
        self.calendar_month=crate::navigation::calendar_month_start(NaiveDate::parse_from_str(&today,"%Y-%m-%d").expect("today in YYYY-MM-DD"));
        self.order_sort=None;self.payment_sort=None;
        if changed{self.refresh(ctx);}
    }
    pub fn refresh(&mut self,ctx:egui::Context){
        // Keep an already verified report only for this exact date.
        let same_scope=self.report.as_ref().is_none_or(|report|report["date"].as_str()==Some(self.date.as_str()));
        crate::screen_refresh::retain_scope(&mut self.report,same_scope);
        if !same_scope{self.order_cache.invalidate();self.payment_cache.invalidate();}
        self.error=None;
        let gateway=Arc::clone(&self.gateway);
        let date=self.date.clone();
        self.refresh_job.submit_required(ctx,move||fetch(gateway.as_ref(),&date));
    }
    fn poll(&mut self){
        match self.refresh_job.poll(){
            Some(Ok(report)) if report["date"].as_str()==Some(self.date.as_str())=>{
                self.refresh_job.publish(&mut self.report,report);
                self.order_cache.invalidate();self.payment_cache.invalidate();
                self.error=None;
            }
            Some(Ok(_))=>self.error=Some("조회한 결산 날짜가 요청 날짜와 일치하지 않습니다.".into()),
            Some(Err(error))=>self.error=Some(error),
            None=>{}
        }
    }
    pub fn show_page(&mut self,ui:&mut Ui){
        let page_origin_y=ui.cursor().top();
        crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Standard);
        self.poll();
        crate::ui_layout::management_page_title_top_gap(ui);
        crate::ui_layout::management_top_content(ui,|top|{
            top.label(typography::page_heading("일일 결산"));
        });
        // Always reserve four KPI slots so async report arrival never moves
        // the date toolbar or the master card below the summary row.
        // Missing values are placeholders, not fabricated settlement totals.
        let report=self.report.as_ref();
        let values=[
            report.map(|r|format!("{}건",rows(&r["orderRows"]).len()))
                .unwrap_or_else(||"—".to_owned()),
            report.map(|r|won(&r["billedKnown"]))
                .unwrap_or_else(||"—".to_owned()),
            report.map(|r|won(&r["paidCash"]))
                .unwrap_or_else(||"—".to_owned()),
            report.map(|r|format!(
                "{}건",r["billedUnknown"].as_u64().unwrap_or(0)
            )).unwrap_or_else(||"—".to_owned()),
        ];
        crate::ui_layout::management_top_content(ui,|top|{
            top.horizontal_wrapped(|strip|{
                crate::ui_layout::apply_spacing_role(strip,crate::ui_layout::SpacingRole::SummaryStrip);
                for (index,(name,value)) in [
                    "접수 주문","청구 합계 확인분",
                    "계좌 입금","미확정 청구 주문"
                ].into_iter().zip(values.iter()).enumerate(){
                    let state=match report{
                        None=>theme::TextState::Pending,
                        Some(r) if index==1=>theme::TextState::value(r["billedKnown"].as_i64().is_some()),
                        Some(r) if index==2=>theme::TextState::value(r["paidCash"].as_i64().is_some()),
                        Some(_)=>theme::TextState::Normal,
                    };
                    kpi(strip,name,value,state);
                }
            });
        });
        ui.add_space(theme::SPACE_12);
        crate::card::align_management_primary_card(ui,page_origin_y);
        let missing=Value::Null;
        let mut visible_report=self.report.as_ref();
        let mut date_changed=false;
        let unavailable=crate::user_error::display("settlement.query",
            self.error.as_deref().unwrap_or("해당 날짜의 결산 자료를 불러올 수 없습니다."));
        let loading=self.refresh_job.pending();
        // The report tables scroll horizontally inside their own row
        // viewports. Keep the OUTER vertical section stack bounded by the
        // visible work area so a wide report cannot expand the card/header
        // beyond the window before its nested horizontal scroller is reached.
        let section_viewport_width=ui.available_rect_before_wrap()
            .intersect(ui.clip_rect()).width().max(1.0);
        let section_inner_max_width=crate::card::inner_width_for_insets(
            section_viewport_width,theme::SECTION_PADDING,theme::SECTION_PADDING,
            theme::FRAME_STROKE_WIDTH,ui.spacing().scroll.allocated_width(),1.0);
        let scroll=crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
            .id_salt("daily-settlement-sections")
            .max_width(section_viewport_width)
            .auto_shrink([false,false]);
        let (scroll,capture)=crate::scrollbar::ScrollCapture::begin(ui,"daily-settlement-sections",
            scroll,visible_report.is_some(),None);
        let output=scroll.show(ui,|body|{
                crate::card::measured_management_section_card(body,|section|{
                    // Keep the section inside the bounded vertical viewport;
                    // its report rows own their independent horizontal scroll.
                    section.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Bounded,section_inner_max_width);
                    // The date selector is the first element of the
                    // settlement card. No redundant title/help row above it.
                    // Like Order History, the date picker is inside the
                    // same large outlined section as the report table.
                    date_changed=settlement_date_toolbar(
                        section,&mut self.date,&mut self.date_input,
                        &mut self.calendar_month
                    );
                    crate::floating_notice::section(section.ctx(),"settlement.query",section.clip_rect());
                    crate::floating_notice::sync(section.ctx(),"settlement.query.error","settlement.query",
                        crate::floating_notice::Tone::Error,self.error.as_deref());
                    crate::ui_primitives::section_divider(section);
                    if date_changed{visible_report=None;}
                    let report=visible_report.unwrap_or(&missing);
                    let orders=rows(&report["orderRows"]);
                    // Order cohort is selected-day; cumulative balances are live.
                    section.label(typography::subsection_heading(
                        "해당 날짜 접수 주문 / 청구 내역"
                    ));
                    section.horizontal_wrapped(|line|{
                        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ReportMetrics);
                        for (title,key) in [
                            ("확정 주문 상품대금","billedProductKnown"),
                            ("확정 주문 청구 배송비","billedShippingKnown"),
                            ("확정 주문 부가세","billedVatKnown"),
                            ("후불 확정 청구액","postpaidKnown"),
                            ("현재 미수 확인분","outstandingKnown"),
                            ("현재 공장 운임 확인분","actualFreightKnown"),
                        ]{
                            metric(line,title,if visible_report.is_some(){won(&report[key])}else{"—".to_owned()},
                                if visible_report.is_some(){theme::TextState::value(report[key].as_i64().is_some())}else{theme::TextState::Pending});
                        }
                    });
                    section.label(typography::caption(
                        "미수·입금 배분 누계·공장 운임은 현재 조회 시점 기준"
                    ));
                    section.horizontal_wrapped(|review|{
                        crate::ui_layout::apply_spacing_role(review,crate::ui_layout::SpacingRole::FormColumns);
                        review.label(typography::caption(&format!(
                            "미수 미확정 {}",
                            report["outstandingUnknown"].as_u64().map(|n|format!("{n}건")).unwrap_or_else(||"—".into())
                        )));
                        review.label(typography::caption(&format!(
                            "공장 운임 미확정 {}",
                            report["freightUnknown"].as_u64().map(|n|format!("{n}건")).unwrap_or_else(||"—".into())
                        )));
                        settings_help::icon(review,"미수금과 출고 운임");
                    });
                    let indices=self.order_cache.get((self.order_sort,visible_report.is_some()),||sorted_indices(orders,self.order_sort,false));
                    let scroll_keys=|i:usize|crate::scrollbar::row_key(field(&orders[indices[i]],"orderId"));
                    report_table(
                        section,"settlement-orders",
                        ["거래처","관리 상태","결제 방식","상품대금","청구 배송비",
                         "부가세","청구 합계","입금 배분 누계","현재 미수"],
                        theme::SETTLEMENT_ORDER_COL_WIDTHS,
                        [Align2::LEFT_CENTER,Align2::CENTER_CENTER,Align2::CENTER_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER],
                        [typography::ManagementTableCellRole::ClientName,typography::ManagementTableCellRole::Status,typography::ManagementTableCellRole::Status,
                         typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::RowTotal,
                         typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money],
                        0,&indices,visible_report.is_some(),&scroll_keys,&mut self.order_sort,
                        if visible_report.is_none(){
                            if date_changed||loading{None}else{Some(unavailable.as_ref())}
                        }else if orders.is_empty(){Some("해당 날짜에 접수한 주문이 없습니다.")}else{None},
                        |index|{
                            let order=&orders[index];
                            [
                                field(order,"clientName").to_owned(),
                                field(order,"status").to_owned(),
                                field(order,"paymentTerms").to_owned(),
                                won(&order["subtotal"]),
                                won(&order["shippingCharge"]),
                                won(&order["vat"]),
                                won(&order["total"]),
                                won(&order["paidAllocated"]),
                                won(&order["outstanding"]),
                            ]
                        },
                        |index|{
                            let order=&orders[index];
                            let mut states=[theme::TextState::Normal;9];
                            for (column,key) in ["subtotal","shippingCharge","vat","total","paidAllocated","outstanding"].into_iter().enumerate(){
                                states[column+3]=theme::TextState::value(order[key].as_i64().is_some());
                            }
                            states
                        }
                    );
                });
                let report=visible_report.unwrap_or(&missing);
                let payments=rows(&report["paymentRows"]);
                body.add_space(theme::SPACE_12);
                crate::card::measured_management_section_card(body,|section|{
                    // Keep the section inside the bounded vertical viewport;
                    // its report rows own their independent horizontal scroll.
                    section.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Bounded,section_inner_max_width);
                    section.label(typography::section_heading(
                        "입금 내역 / 입금 일시 기준"
                    ));
                    crate::ui_primitives::section_divider(section);
                    section.horizontal_wrapped(|line|{
                        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ReportMetrics);
                        for (title,key) in [
                            ("입금 합계","paidCash"),
                            ("배송 건 배분액","allocatedCash"),
                            ("미배분 입금액","unallocatedCash"),
                        ]{
                            metric(line,title,if visible_report.is_some(){won(&report[key])}else{"—".to_owned()},
                                if visible_report.is_some(){theme::TextState::value(report[key].as_i64().is_some())}else{theme::TextState::Pending});
                        }
                    });
                    let indices=self.payment_cache.get((self.payment_sort,visible_report.is_some()),||sorted_indices(payments,self.payment_sort,true));
                    let scroll_keys=|i:usize|crate::scrollbar::row_key(field(&payments[indices[i]],"id"));
                    report_table(
                        section,"settlement-payments",
                        ["입금 일시","거래처","계좌","입금액",
                         "배분액","미배분","메모"],
                        theme::SETTLEMENT_PAYMENT_COL_WIDTHS,
                        [Align2::LEFT_CENTER,Align2::LEFT_CENTER,Align2::LEFT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::LEFT_CENTER],
                        [typography::ManagementTableCellRole::Text,typography::ManagementTableCellRole::ClientName,typography::ManagementTableCellRole::Description,
                         typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money,
                         typography::ManagementTableCellRole::Description],
                        6,&indices,visible_report.is_some(),&scroll_keys,&mut self.payment_sort,
                        if visible_report.is_some()&&payments.is_empty(){
                            Some("해당 날짜의 입금 기록이 없습니다.")
                        }else{None},
                        |index|{
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
                        },
                        |index|{
                            let payment=&payments[index];
                            let mut states=[theme::TextState::Normal;7];
                            for (column,key) in ["amount","allocated","unallocated"].into_iter().enumerate(){
                                states[column+3]=theme::TextState::value(payment[key].as_i64().is_some());
                            }
                            states[6]=theme::TextState::optional(!field(payment,"memo").trim().is_empty());
                            states
                        }
                    );
                });
                body.add_space(theme::SPACE_12);
                crate::card::measured_management_section_card(body,|review|{
                    review.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Bounded,section_inner_max_width);
                    review.label(typography::section_heading("결산 검토 사항"));
                    crate::ui_primitives::section_divider(review);
                    if visible_report.is_none(){
                        crate::ui_primitives::management_body_state_inline(review,
                            if loading||date_changed{"조회 중"}else{"결산 자료 미확인"});
                    }else{
                        let warnings=rows(&report["warnings"]);
                        if warnings.is_empty(){
                            crate::ui_primitives::management_body_state_inline(review,"검토 사항 없음");
                        }
                        for warning in warnings{
                            if let Some(message)=warning.as_str(){
                                review.label(typography::body(message));
                            }
                        }
                    }
                });
            });
        capture.finish(ui,&output,None);
        if date_changed{self.refresh(ui.ctx().clone());}
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn settlement_date_toolbar_uses_one_shared_segmented_date_picker(){
        fn measure(settlement:bool)->Rect{
            let ctx=crate::ui_test_support::test_context();
            let screen=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(900.0,400.0));
            let mut date="2026-11-28".to_owned();
            let mut input=date.clone();
            let mut month=NaiveDate::from_ymd_opt(2026,11,1).unwrap();
            crate::ui_test_support::begin_component_geometry();
            let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    if settlement{
                        assert!(!settlement_date_toolbar(ui,&mut date,&mut input,&mut month));
                    }else{
                        ui.horizontal(|row|{
                            crate::navigation::apply_management_date_control_spacing(row);
                            assert!(!crate::navigation::range_date_picker(
                                row,false,&mut input,&mut month,"settlement-reference-picker"));
                        });
                    }
                });
            });
            assert_eq!(date,"2026-11-28");
            assert_eq!(input,date);
            let samples=crate::ui_test_support::finish_component_geometry(screen);
            let frames=samples.iter().filter(|sample|sample.role=="segmented frame")
                .collect::<Vec<_>>();
            assert_eq!(frames.len(),1,"date picker must allocate exactly one shared outer frame");
            let frame=frames[0];
            assert!(frame.clip.expand(0.5).contains(frame.rect.min));
            assert!(frame.clip.expand(0.5).contains(frame.rect.max));
            frame.rect
        }
        let actual=measure(true);
        let reference=measure(false);
        assert!((actual.width()-reference.width()).abs()<=0.5);
        assert!((actual.height()-reference.height()).abs()<=0.5);
        assert!((actual.height()-theme::INPUT_HEIGHT).abs()<=0.5);
    }

    #[test]
    fn rendered_report_viewport_shrinks_to_actual_record_count(){
        fn measure(count:usize)->f32{
            let ctx=crate::ui_test_support::test_context();
            let indices=(0..count).collect::<Vec<_>>();
            let mut sort=None;
            for _ in 0..3{
                crate::ui_test_support::TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
                let _=ctx.run(egui::RawInput{
                    screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1400.0,1000.0))),
                    ..Default::default()
                },|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Standard);
                        report_table(ui,"settlement-orders",["거래처","금액"],[160.0,130.0],
                            [Align2::LEFT_CENTER,Align2::RIGHT_CENTER],
                            [typography::ManagementTableCellRole::ClientName,typography::ManagementTableCellRole::Money],
                            0,&indices,true,&|_|None,&mut sort,None,
                            |_|["검증 거래처".into(),"1,000원".into()],
                            |_|[theme::TextState::Normal;2]);
                    });
                });
            }
            let trace=crate::ui_test_support::TRACE.with(|trace|trace.borrow_mut().take().unwrap());
            let viewport=trace.iter().find(|(role,_)|*role=="row viewport").expect("report viewport").1;
            assert!(viewport.is_positive());
            assert_eq!(trace.iter().filter(|(role,_)|*role=="settlement order row").count(),count);
            viewport.height()
        }
        let one=measure(1);
        let three=measure(3);
        assert!((three-one-2.0*ROW_HEIGHT).abs()<=1.0,
            "table body must grow by two rows: {one} -> {three}");
    }
    #[test]
    fn settlement_rows_fit_small_reports_and_cap_large_reports(){
        assert_eq!(report_body_height(0,1080.0,0.0),ROW_HEIGHT);
        assert_eq!(report_body_height(1,1080.0,0.0),ROW_HEIGHT);
        assert_eq!(report_body_height(3,1080.0,0.0),3.0*ROW_HEIGHT);
        assert_eq!(report_body_height(3,1080.0,10.0),3.0*ROW_HEIGHT+10.0);
        assert!(report_body_height(100,768.0,0.0)<=768.0*theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION);
        let data=vec![json!({"total":null}),json!({"total":20}),json!({"total":10})];
        assert_eq!(sorted_indices(&data,Some(Sort{column:6,direction:Direction::Ascending}),false),vec![2,1,0]);
        assert_eq!(sorted_indices(&data,Some(Sort{column:6,direction:Direction::Descending}),false),vec![1,2,0]);
        let data=vec![json!({"paidAt":"2026-11-27T12:00:00+09:00"}),json!({"paidAt":"2026-11-27T04:00:00Z"})];
        assert_eq!(sorted_indices(&data,Some(Sort{column:0,direction:Direction::Ascending}),true),vec![0,1]);
    }
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

#[cfg(test)]
impl DailySettlementManager {
    pub(crate) fn geometry_fixture(gateway:Arc<dyn BandoGateway>,ctx:egui::Context,_tab:usize,loaded:bool)->Self{
        let mut page=Self::new(gateway,ctx);
        page.refresh_job.reset();if loaded{page.report=Some(serde_json::json!({"date":page.date.clone(),"orderRows":[],"paymentRows":[]}));}
        page
    }

    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        let finish=self.refresh_job.geometry_begin_read();
        self.error=None;
        let date=self.date.clone();
        Box::new(move |answer|{
            let result=answer.map(|count|{
                let orders=(0..count).map(|i|json!({"clientName":format!("검증 거래처 {i}"),
                    "status":"처리 중","paymentTerms":"선불","subtotal":100000,
                    "shippingCharge":3000,"vat":10300,"total":113300,"paidAllocated":100000,"outstanding":13300})).collect::<Vec<_>>();
                let payments=(0..count).map(|i|json!({"dealer":format!("검증 거래처 {i}"),
                    "paidAt":"2026-11-24T09:00:00+09:00","account":"합성 계좌","amount":100000,
                    "allocated":95000,"unallocated":5000,"memo":"검증 메모"})).collect::<Vec<_>>();
                let total=count as i64*113300;
                json!({"date":date,"orderRows":orders,"paymentRows":payments,"warnings":(0..count).map(|i|format!("합성 확인 사항 {i}")).collect::<Vec<_>>(),
                    "billedKnown":total,"paidCash":count as i64*100000,"billedUnknown":0,"productKnown":count as i64*100000,
                    "shippingKnown":count as i64*3000,"vatKnown":count as i64*10300,"postpaidKnown":0,
                    "billedProductKnown":count as i64*100000,"billedShippingKnown":count as i64*3000,"billedVatKnown":count as i64*10300,
                    "outstandingKnown":count as i64*13300,"actualFreightKnown":count as i64*3000,
                    "outstandingUnknown":0,"freightUnknown":0,"allocatedCash":count as i64*95000,"unallocatedCash":count as i64*5000})
            });
            finish(result);
        })
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
}
