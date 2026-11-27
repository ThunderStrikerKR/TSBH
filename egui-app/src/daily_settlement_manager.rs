//! Daily settlement: read-only native Rust projection of the existing
//! order-history -> output.prepare_settlement pipeline.
//! No egui totals, duplicate accounting, local receipt cache or mutations.
use crate::{
    gateway::{BandoGateway,ReadOperation},
    settings_help,theme,typography,
};
use chrono::{DateTime,Duration as Days,FixedOffset,NaiveDate,Utc};
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
fn move_date(date:&str,days:i64)->String{
    let fallback=seoul_today();
    let base=if valid_date(date){date}else{&fallback};
    NaiveDate::parse_from_str(base,"%Y-%m-%d").ok()
        .and_then(|day|day.checked_add_signed(Days::days(days)))
        .map(|day|day.format("%Y-%m-%d").to_string())
        .unwrap_or_else(||base.to_owned())
}
/// Same date field / arrows / calendar control as Order History, located
/// INSIDE the first settlement report card rather than in an outer panel.
/// Return true only when a different valid date is selected.
fn settlement_date_toolbar(
    ui:&mut Ui,date:&mut String,date_input:&mut String,
    calendar_month:&mut NaiveDate
)->bool{
    let mut changed=false;
    ui.horizontal_wrapped(|controls|{
        crate::ui_layout::apply_spacing_role(controls,crate::ui_layout::SpacingRole::Toolbar);
        crate::ui_primitives::form_field_label(controls,"결산 날짜");
        if crate::button::action(controls,"‹",theme::ButtonRole::Standard,true).clicked(){
            *date=move_date(date,-1);
            *date_input=date.clone();
            if let Ok(day)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){
                *calendar_month=crate::navigation::calendar_month_start(day);
            }
            changed=true;
        }
        if crate::input::flexible_text_field(
            controls,date_input,theme::SETTLEMENT_DATE_FIELD_WIDTH,"YYYY-MM-DD"
        ).changed() && valid_date(date_input) && date!=date_input{
            *date=date_input.clone();
            if let Ok(day)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){
                *calendar_month=crate::navigation::calendar_month_start(day);
            }
            changed=true;
        }
        let trigger=crate::button::action(
            controls,"달력",theme::ButtonRole::Standard,true
        );
        if trigger.clicked(){
            let open=NaiveDate::parse_from_str(date,"%Y-%m-%d")
                .unwrap_or_else(|_|NaiveDate::parse_from_str(
                    &seoul_today(),"%Y-%m-%d").expect("valid today"));
            *calendar_month=crate::navigation::calendar_month_start(open);
        }
        if let Some(day)=crate::navigation::date_calendar(
            &trigger,calendar_month,date,"daily-settlement-date-calendar"
        ){
            *date=day.format("%Y-%m-%d").to_string();
            *date_input=date.clone();
            changed=true;
        }
        if crate::button::action(controls,"›",theme::ButtonRole::Standard,true).clicked(){
            *date=move_date(date,1);
            *date_input=date.clone();
            if let Ok(day)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){
                *calendar_month=crate::navigation::calendar_month_start(day);
            }
            changed=true;
        }
    });
    let date_error=(!valid_date(date_input)).then_some(
        "결산 날짜는 YYYY-MM-DD 형식으로 입력하세요."
    );
    let status_width=ui.available_width();
    crate::ui_layout::fixed_status_slot(
        ui,status_width,typography::METADATA_LINE_HEIGHT,
        date_error,date_error,theme::DANGER
    );
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
        return Err("결산 정보를 불러오지 못했습니다.".into());
    }
    Ok(report)
}

/// Preserve the approved numeric-first KPI identity. Monetary cards are
/// wider than freight counts so actual amounts remain legible.
fn kpi(ui:&mut Ui,label:&str,value:&str){
    crate::card::management_summary_card_with_role(
        ui,label,value,crate::card::SummaryCardRole::Settlement
    );
}

fn metric(ui:&mut Ui,label:&str,value:String){
    ui.horizontal(|line|{
        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ReportMetricPair);
        line.label(typography::field_label(label));
        // Preserve the original 22pt body line box while displaying a
        // standalone monetary figure in the shared numeric data face.
        line.label(typography::data_value_text(
            &value,typography::DataValueKind::Money,
            typography::DataValueContext::Form
        ).line_height(Some(22.0)));
    });
}

fn paint_cell(
    ui:&Ui,rect:Rect,content:&str,align:Align2,
    role:typography::ManagementTableCellRole,id:impl std::hash::Hash,
){
    crate::table::paint_management_text_cell(
        ui,rect.shrink2(Vec2::new(theme::SPACE_8,0.0)),content,content,role,
        align,("settlement-cell",id),false
    );
}

/// One record per row with one header/row geometry. Header stays pinned
/// while only records scroll. Empty reports retain the column header.
#[allow(clippy::too_many_arguments)]
fn report_table<const N:usize>(
    ui:&mut Ui,id:&str,headings:[&str;N],base:[f32;N],
    align:[Align2;N],roles:[typography::ManagementTableCellRole;N],
    stretch_column:usize,indices:&[usize],ready:bool,scroll_keys:&dyn Fn(usize)->Option<egui::Id>,
    sort:&mut Option<Sort>,empty_notice:Option<&str>,format_row:impl Fn(usize)->[String;N],
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
    let scroll_max_height=ui.ctx().content_rect().height()
        *theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION;
    let scroll_track=crate::table::management_table_scroll_track_rect(
        ui,scroll_max_height
    );
    let mut first_x=None;
    let scroll=crate::table::fixed_rows_snapshot(
                ui,crate::table::Viewport{width:width,height:scroll_max_height,rail:Some(scroll_track)},
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
                        for (column,cell) in row_cells.iter().enumerate(){
                            paint_cell(
                                row,*cell,&data[column],align[column],
                                roles[column],(id,indices[index],column),
                            );
                        }
                        
                    }
                );
            }
        }
    );
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
                for (name,value) in [
                    "접수 주문","청구 합계 확인분",
                    "실제 계좌 입금","미확정 청구 주문"
                ].into_iter().zip(values.iter()){
                    kpi(strip,name,value);
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
                    crate::card::apply_width_policy(section,crate::card::CardWidthPolicy::Bounded,section_inner_max_width);
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
                    section.label(typography::subsection_heading(
                        "해당 날짜 접수 주문 / 청구 내역"
                    ));
                    section.add_space(theme::SPACE_8);
                    section.horizontal_wrapped(|line|{
                        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ReportMetrics);
                        for (title,key) in [
                            ("상품대금 확인분","productKnown"),
                            ("고객 청구 배송비 확인분","shippingKnown"),
                            ("부가세 확인분","vatKnown"),
                            ("후불 주문 청구액 확인분","postpaidKnown"),
                            ("현재 미수 확인분","outstandingKnown"),
                            ("현재 공장 운임 확인분","actualFreightKnown"),
                        ]{
                            metric(line,title,if visible_report.is_some(){won(&report[key])}else{"—".to_owned()});
                        }
                    });
                    section.add_space(theme::SPACE_8);
                    section.horizontal(|review|{
                        crate::ui_layout::apply_spacing_role(review,crate::ui_layout::SpacingRole::FormColumns);
                        review.label(typography::caption(&format!(
                            "미수 미확정 {}",
                            report["outstandingUnknown"].as_u64().map(|n|format!("{n}건")).unwrap_or_else(||"—".into())
                        )));
                        review.label(typography::caption(&format!(
                            "실제 운임 미확정 {}",
                            report["freightUnknown"].as_u64().map(|n|format!("{n}건")).unwrap_or_else(||"—".into())
                        )));
                        settings_help::icon(review,"미수금과 실제 운임");
                    });
                    crate::ui_primitives::section_divider(section);
                    let indices=self.order_cache.get((self.order_sort,visible_report.is_some()),||sorted_indices(orders,self.order_sort,false));
                    let scroll_keys=|i:usize|crate::scrollbar::row_key(field(&orders[indices[i]],"orderId"));
                    report_table(
                        section,"settlement-orders",
                        ["거래처 / 상태","결제 방식","상품대금","청구 배송비",
                         "부가세","청구 합계","실제 입금 배분 누계","현재 미수"],
                        theme::SETTLEMENT_ORDER_COL_WIDTHS,
                        [Align2::LEFT_CENTER,Align2::LEFT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER,
                         Align2::RIGHT_CENTER,Align2::RIGHT_CENTER],
                        [typography::ManagementTableCellRole::ClientName,typography::ManagementTableCellRole::Status,
                         typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::RowTotal,
                         typography::ManagementTableCellRole::Money,typography::ManagementTableCellRole::Money],
                        0,&indices,visible_report.is_some(),&scroll_keys,&mut self.order_sort,
                        if visible_report.is_none(){
                            if date_changed||loading{None}else{Some(unavailable.as_ref())}
                        }else if orders.is_empty(){Some("해당 날짜에 접수한 주문이 없습니다.")}else{None},
                        |index|{
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
                });
                let report=visible_report.unwrap_or(&missing);
                let payments=rows(&report["paymentRows"]);
                body.add_space(theme::SPACE_12);
                crate::card::measured_management_section_card(body,|section|{
                    // Keep the section inside the bounded vertical viewport;
                    // its report rows own their independent horizontal scroll.
                    crate::card::apply_width_policy(section,crate::card::CardWidthPolicy::Bounded,section_inner_max_width);
                    section.label(typography::section_heading(
                        "실제 입금 내역 / 입금 일시 기준"
                    ));
                    crate::ui_primitives::section_divider(section);
                    section.horizontal_wrapped(|line|{
                        crate::ui_layout::apply_spacing_role(line,crate::ui_layout::SpacingRole::ReportMetrics);
                        for (title,key) in [
                            ("실제 입금 합계","paidCash"),
                            ("배송 건 배분액","allocatedCash"),
                            ("미배분 입금액","unallocatedCash"),
                        ]{
                            metric(line,title,if visible_report.is_some(){won(&report[key])}else{"—".to_owned()});
                        }
                    });
                    crate::ui_primitives::section_divider(section);
                    let indices=self.payment_cache.get((self.payment_sort,visible_report.is_some()),||sorted_indices(payments,self.payment_sort,true));
                    let scroll_keys=|i:usize|crate::scrollbar::row_key(field(&payments[indices[i]],"id"));
                    report_table(
                        section,"settlement-payments",
                        ["입금 일시","거래처","계좌","실제 입금액",
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
                            Some("해당 날짜의 실제 입금 기록이 없습니다.")
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
                        }
                    );
                });
                body.add_space(theme::SPACE_12);
                crate::card::measured_management_section_card(body,|review|{
                    crate::card::apply_width_policy(review,crate::card::CardWidthPolicy::Bounded,section_inner_max_width);
                    review.label(typography::section_heading("결산 검토 사항"));
                    crate::ui_primitives::section_divider(review);
                    crate::ui_layout::fixed_result_viewport(review,"settlement-warnings",100.0,|list|{
                        for warning in rows(&report["warnings"]){
                            if let Some(message)=warning.as_str(){
                                list.label(typography::body(message));
                            }
                        }
                    });
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
                    "outstandingKnown":count as i64*13300,"actualFreightKnown":count as i64*3000,
                    "outstandingUnknown":0,"freightUnknown":0,"allocatedCash":count as i64*95000,"unallocatedCash":count as i64*5000})
            });
            finish(result);
        })
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
}
