//! Read-only order history. The authoritative history/status/pricing/tracking
//! decisions are all projected by existing native Rust rules::history::list_view.
//! The native egui view owns only date/search presentation, sort, expansion,
//! clipboard copy and virtualized fixed-height rows.
use crate::{
    components::{self, calendar_dates, calendar_month_start as month_start,
        date_calendar, shift_calendar_month},
    gateway::{BandoGateway, ReadOperation},
    settings_help, theme, typography,
};
use chrono::{DateTime, Duration as Days, FixedOffset, NaiveDate, Utc};
use eframe::egui::{self, Align, Align2, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{mpsc::{self, Receiver, TryRecvError}, Arc},
};

const ROW_HEIGHT: f32 = theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT;
const HEADER_HEIGHT: f32 = theme::MANAGEMENT_TABLE_HEADER_HEIGHT;
const COL_GAP: f32 = theme::SPACE_8;
// Compact fixed-width metadata columns; the shipping / product column takes
// ALL remaining width (including when the main pane is resized).
// Date text is YYYY-MM-DD; client name shows at most ten characters;
// status is one of four known values; invoice is at most 10M KRW.
const COLS: [f32;7] = [112.0,144.0,300.0,126.0,124.0,112.0,132.0];
const MIN_WIDTH: f32 = 1098.0; // sum(COLS) + six 8pt gaps
// Natural width of [date basis, previous, start date/calendar, end
// date/calendar, next, all dates]. The search stays outside this right slot.
const RANGE_CONTROLS_WIDTH:f32=940.0;
const RANGE_COMPACT_START_WIDTH:f32=480.0;
const RANGE_COMPACT_END_WIDTH:f32=460.0;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum HistoryDateBasis { Reception, Payment }
impl HistoryDateBasis {
    fn key(self)->&'static str{
        match self{Self::Reception=>"RECEPTION",Self::Payment=>"PAYMENT"}
    }
    fn label(self)->&'static str{
        match self{Self::Reception=>"접수일 기준",Self::Payment=>"입금일 기준"}
    }
}

fn field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(row: &Value) -> &[Value] {
    row.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn won(value:&Value)->String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(||"확인 필요".to_owned())
}
fn seoul_today()->String{
    Utc::now().with_timezone(&FixedOffset::east_opt(9*3600).expect("Seoul offset"))
        .format("%Y-%m-%d").to_string()
}
fn seoul_date(raw:&str)->String{
    if let Ok(parsed)=DateTime::parse_from_rfc3339(raw) {
        return parsed.with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
            .format("%Y-%m-%d").to_string()
    }
    raw.get(..10).filter(|date|NaiveDate::parse_from_str(date,"%Y-%m-%d").is_ok())
        .unwrap_or("").to_owned()
}
fn move_date(value:&str,step:i64)->String{
    let fallback=seoul_today();
    let base=if value.is_empty(){fallback.as_str()}else{value};
    NaiveDate::parse_from_str(base,"%Y-%m-%d").ok()
        .and_then(|date|date.checked_add_signed(Days::days(step)))
        .map(|date|date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(||value.to_owned())
}
fn model_label(raw:&str)->String{
    let name=raw.trim_start_matches("[가상]").trim_start();
    let remaining=name.strip_prefix("BD")
        .filter(|rest|rest.chars().next().is_some_and(|c|
            c.is_whitespace()||c.is_ascii_digit()));
    remaining.unwrap_or(name).trim_start().to_owned()
}
fn order_date(row:&Value)->String{
    let order=&row["order"];
    let raw=if field(order,"confirmedAt").is_empty(){
        field(order,"createdAt")
    }else{field(order,"confirmedAt")};
    seoul_date(raw)
}
fn order_client(row:&Value)->&str{
    row["client"]["name"].as_str().unwrap_or("과거 거래처")
}
fn display_client_name(name:&str)->String{
    name.chars().take(10).collect()
}
fn order_items_single_line(row:&Value,expanded:bool)->String{
    let items=rows(&row["recipients"]).iter()
        .flat_map(|recipient|rows(&recipient["items"]).iter())
        .map(|item|format!("{} × {}",
            model_label(field(item,"name")),
            item["quantity"].as_i64().map(|quantity|quantity.to_string())
                .unwrap_or_else(||"수량 미확인".to_owned())))
        .collect::<Vec<_>>();
    let lead=if expanded{"▾"}else{"▸"};
    if items.is_empty(){format!("{lead} 품목 없음")}
    else{format!("{lead} {}",items.join(" / "))}
}
fn tracking_metadata(state:&Value)->Value{
    Value::Array(rows(&state["workflow"]["shipments"]).iter().map(|ship|{
        let keys=ship["tracking"].as_object().map(|values|
            values.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        json!({"shipmentId":field(ship,"id"),"keys":keys})
    }).collect())
}
fn fetch(
    gateway:&dyn BandoGateway,query:&str,from:&str,to:&str,
    basis:HistoryDateBasis
)->Result<HistorySnapshot,String>{
    let state=gateway.load_business_state()?;
    let value=gateway.execute(ReadOperation::ListOrderHistoryView,json!({
        "state":&state,"query":query,"dateFrom":from,"dateTo":to,
        "dateBasis":basis.key(),
        "trackingOrder":tracking_metadata(&state)
    }))?;
    let entries=value.as_array().ok_or("Rust 주문내역 조회 결과가 배열이 아닙니다.")?;
    Ok(HistorySnapshot{state,rows:entries.clone()})
}

struct HistorySnapshot {
    state:Value,
    rows:Vec<Value>,
}

#[derive(Clone,Copy,PartialEq,Eq)]
enum SortColumn { Date, Client, Shipment, Status, Total }
#[derive(Clone,Copy,PartialEq,Eq)]
enum SortDirection { Asc, Desc }
#[derive(Clone,Copy,PartialEq,Eq)]
struct Sort { column:SortColumn,direction:SortDirection }
impl Sort {
    fn next(previous:Option<Self>,column:SortColumn)->Option<Self>{
        match previous {
            Some(Self{column:c,direction:SortDirection::Asc}) if c==column=>
                Some(Self{column,direction:SortDirection::Desc}),
            Some(Self{column:c,direction:SortDirection::Desc}) if c==column=>None,
            _=>Some(Self{column,direction:SortDirection::Asc}),
        }
    }
}

#[derive(Clone,Copy)]
enum HistoryLine {
    Order(usize),
    Recipient(usize,usize),
    Item(usize,usize,usize),
    Shipment(usize,usize),
    Billing(usize),
}
fn display_lines(snapshot:&HistorySnapshot,expanded:&BTreeSet<String>,
    sorting:Option<Sort>)->Vec<HistoryLine>{
    let mut indices=(0..snapshot.rows.len()).collect::<Vec<_>>();
    if let Some(sort)=sorting {
        indices.sort_by_cached_key(|i|{
            let row=&snapshot.rows[*i];
            match sort.column {
                SortColumn::Date=>order_date(row),
                SortColumn::Client=>order_client(row).to_lowercase(),
                SortColumn::Shipment=>rows(&row["recipients"]).first()
                    .map(|r|field(r,"name").to_lowercase()).unwrap_or_default(),
                SortColumn::Status=>field(row,"status").to_owned(),
                SortColumn::Total=>row["total"].as_i64()
                    .map(|n|format!("{:020}",n.max(0)))
                    .unwrap_or_else(||"~".into()),
            }
        });
        if sort.direction==SortDirection::Desc{indices.reverse();}
    }
    let mut lines=Vec::new();
    for index in indices {
        let row=&snapshot.rows[index];
        lines.push(HistoryLine::Order(index));
        if !expanded.contains(field(&row["order"],"id")){continue}
        for (recipient_index,recipient) in rows(&row["recipients"]).iter().enumerate(){
            lines.push(HistoryLine::Recipient(index,recipient_index));
            for item_index in 0..rows(&recipient["items"]).len(){
                lines.push(HistoryLine::Item(index,recipient_index,item_index));
            }
            for (shipment_index,shipment) in rows(&snapshot.state["workflow"]["shipments"])
                .iter().enumerate(){
                if field(shipment,"orderId")==field(&row["order"],"id")
                    && field(shipment,"recipientId")==field(recipient,"id"){
                    lines.push(HistoryLine::Shipment(index,shipment_index));
                }
            }
        }
        lines.push(HistoryLine::Billing(index));
    }
    lines
}

fn cells(origin:Pos2,widths:&[f32;7],height:f32)->[Rect;7]{
    let mut left=origin.x;
    std::array::from_fn(|index|{
        let cell=Rect::from_min_size(egui::pos2(left,origin.y),
            Vec2::new(widths[index],height));
        left+=widths[index]+COL_GAP;
        cell
    })
}
fn paint_column_dividers(ui:&Ui,rect:Rect,widths:&[f32;7]){
    let columns=cells(rect.min,widths,rect.height());
    let painter=ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    for column in columns.iter().take(6){
        let x=column.right()+COL_GAP/2.0;
        painter.line_segment(
            [egui::pos2(x,rect.top()),egui::pos2(x,rect.bottom())],
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
        );
    }
}
fn paint_text(ui:&Ui,rect:Rect,value:&str,emphasized:bool,right:bool){
    if rect.width()<=0.0{return}
    let color=if emphasized{theme::TEXT}else{theme::SECONDARY};
    let galley=typography::freight_table_cell_galley(ui,value,emphasized,color);
    // Freight table's text columns use a 6pt leading guide; monetary
    // figures keep their 8pt trailing guide for right alignment.
    let inset=if right{theme::SPACE_8}else{6.0};
    let x=if right{
        (rect.right()-inset-galley.size().x).max(rect.left()+inset)
    }else{rect.left()+inset};
    let y=rect.center().y-galley.size().y/2.0;
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(
        egui::pos2(x,y),galley,color
    );
}
fn paint_center(ui:&Ui,rect:Rect,value:&str){
    let galley=typography::freight_table_cell_galley(ui,value,false,theme::SECONDARY);
    let x=(rect.center().x-galley.size().x/2.0)
        .max(rect.left()+6.0);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(
        egui::pos2(x,rect.center().y-galley.size().y/2.0),galley,theme::SECONDARY
    );
}
fn paint_caption(ui:&Ui,rect:Rect,value:&str){
    let galley=typography::client_table_metadata_galley(ui,value,theme::MUTED);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(
        egui::pos2(rect.left()+6.0,
            rect.center().y-galley.size().y/2.0),galley,theme::MUTED
    );
}
fn draw_line(ui:&mut Ui,rect:Rect,line:HistoryLine,snapshot:&HistorySnapshot,
    expanded:&BTreeSet<String>,widths:&[f32;7],pending_toggle:&mut Option<String>){
    let area=rect.intersect(ui.clip_rect());
    let order_index=match line{
        HistoryLine::Order(i)|HistoryLine::Billing(i)
        |HistoryLine::Recipient(i,_)|HistoryLine::Item(i,_,_)
        |HistoryLine::Shipment(i,_)=>i,
    };
    let row=&snapshot.rows[order_index];
    let id=field(&row["order"],"id");
    match line {
        HistoryLine::Order(_)=>{
            let columns=cells(rect.min,widths,rect.height());
            paint_text(ui,columns[0],&order_date(row),false,false);
            paint_text(ui,columns[1],&display_client_name(order_client(row)),
                true,false);
            ui.interact(columns[1].intersect(area),
                ui.make_persistent_id(("history-client-name",id)),Sense::hover())
                .on_hover_text(order_client(row));
            // Main list: ONLY product name × quantity, all items in one line.
            // Click anywhere except the action button to expand all details.
            let summary=order_items_single_line(row,expanded.contains(id));
            paint_text(ui,columns[2],&summary,true,false);
            let open_area=Rect::from_min_max(
                rect.left_top(),egui::pos2(columns[6].left(),rect.bottom())
            ).intersect(area);
            let response=ui.interact(open_area,
                ui.make_persistent_id(("order-history-toggle",id)),Sense::click())
                .on_hover_text(format!("{summary}\n클릭하여 수취인·품목·운임 상세보기"));
            if response.clicked(){*pending_toggle=Some(id.to_owned());}
            paint_center(ui,columns[3],field(row,"status"));
            let total=won(&row["total"]);
            let mut cautions=Vec::new();
            if field(&row["order"],"orderType")=="RETAIL"{
                cautions.push("소매 주문 / 결제 증빙 별도 확인");
            }
            if row["display"]["paymentEvidenceMissing"]==true{
                cautions.push("기존 입금 표시 / 실제 입금 기록 미확인");
            }
            if cautions.is_empty(){
                paint_text(ui,columns[4],&total,true,true);
            }else{
                let upper=Rect::from_min_max(columns[4].min,
                    egui::pos2(columns[4].right(),columns[4].center().y));
                let lower=Rect::from_min_max(egui::pos2(columns[4].left(),
                    columns[4].center().y),columns[4].max);
                paint_text(ui,upper,&total,true,true);
                paint_caption(ui,lower,&cautions.join(" / "));
                ui.interact(columns[4].intersect(area),
                    ui.make_persistent_id(("history-billing-caution",id)),
                    Sense::hover()).on_hover_text(cautions.join("\n"));
            }
            // Staff confirmation is NOT the bank transfer's paidAt date.
            // It comes from the Rust projection of workflow.confirmations.
            let approval=&row["display"]["employeePaymentConfirmation"];
            let approval_day=field(approval,"date");
            let partial=approval["partial"]==true;
            // This column is date-only: YYYY-MM-DD maximum. Partial
            // approvals are distinguished by muted tint and clear tooltip,
            // never by adding an extra label that widens the date column.
            let approval_label=if approval_day.is_empty(){
                "미확인".to_owned()
            }else{
                approval_day.to_owned()
            };
            paint_center(ui,columns[5],&approval_label);
            let approval_help=if approval_day.is_empty(){
                "직원이 승인한 입금 확인 기록이 없습니다.".to_owned()
            }else{
                let actors=rows(&approval["actors"]).iter()
                    .filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
                format!("직원 입금 확인일: {approval_day}\n{}\n승인 배송 건: {}/{}{}",
                    if partial{"일부 배송 건만 승인됨"}else{"전체 배송 건 승인됨"},
                    approval["approvedShipments"].as_u64().unwrap_or(0),
                    approval["totalShipments"].as_u64().unwrap_or(0),
                    if actors.is_empty(){String::new()}
                    else{format!("\n승인 직원: {actors}")}
                )
            };
            ui.interact(columns[5].intersect(area),
                ui.make_persistent_id(("history-payment-confirmation",id)),
                Sense::hover()).on_hover_text(approval_help);
            let message=field(&row["display"],"trackingMessage");
            // Match the action inset of the Freight Settings column.
            // No expansion of the table or the fixed 42pt row height.
            let control=columns[6].shrink2(
                Vec2::new(theme::SPACE_12,theme::SPACE_4)
            );
            ui.scope_builder(UiBuilder::new()
                .id_salt(("order-history-copy",id))
                .max_rect(control)
                .layout(egui::Layout::left_to_right(Align::Center)
                    .with_main_justify(true)),|actions|{
                    actions.set_clip_rect(control.intersect(area));
                    if theme::action(actions,"운송장 복사",
                        theme::ButtonRole::TableSecondary,!message.is_empty()).clicked(){
                        actions.ctx().copy_text(message.to_owned());
                    }
                });
        },
        HistoryLine::Recipient(_,recipient_index)=>{
            let recipient=&row["recipients"][recipient_index];
            let caption=format!("  {}  /  배송 주소: {}",
                field(recipient,"name"),field(recipient,"address"));
            paint_text(ui,rect,&caption,true,false);
            ui.interact(area,ui.make_persistent_id(("history-recipient",id,recipient_index)),
                Sense::hover()).on_hover_text(caption);
        },
        HistoryLine::Item(_,recipient_index,item_index)=>{
            let recipient=&row["recipients"][recipient_index];
            let item=&recipient["items"][item_index];
            let linked=rows(&row["display"]["references"]).iter().find(|reference|
                field(reference,"recipientId")==field(recipient,"id")
                && field(reference,"itemId")==field(item,"id"))
                .map(|ref_row|&ref_row["link"]);
            let label=format!("    주문 당시: {} × {}{}",
                field(item,"name"),
                item["quantity"].as_i64().map(|n|n.to_string())
                    .unwrap_or_else(||"수량 확인".into()),
                match linked {
                    Some(link) if field(link,"via")=="HISTORICAL"=>
                        format!("  /  현재 연결 상품: {}",field(link,"name")),
                    Some(Value::Null)|None if !field(item,"productId").is_empty()=>
                        "  /  상품 연결 확인 필요".to_owned(),
                    _=>String::new(),
                });
            paint_text(ui,rect,&label,false,false);
            ui.interact(area,ui.make_persistent_id(("history-item",id,recipient_index,item_index)),
                Sense::hover()).on_hover_text(label);
        },
        HistoryLine::Shipment(_,shipment_index)=>{
            let shipment=&snapshot.state["workflow"]["shipments"][shipment_index];
            let tracking=shipment["tracking"].as_object().into_iter().flatten()
                .filter_map(|(_,v)|v.as_str()).filter(|v|!v.is_empty())
                .collect::<Vec<_>>().join(" / ");
            let actual=shipment["actualFreight"].as_object().into_iter().flatten()
                .filter_map(|(_,v)|v.as_i64())
                .map(components::display_won).collect::<Vec<_>>().join(" / ");
            let label=format!("    송장 {}  /  실제 운임 {}",
                if tracking.is_empty(){"확인 필요"}else{&tracking},
                if actual.is_empty(){"확인 필요"}else{&actual});
            paint_text(ui,rect,&label,false,false);
            ui.interact(area,ui.make_persistent_id(("history-shipment",id,shipment_index)),
                Sense::hover()).on_hover_text(label);
        },
        HistoryLine::Billing(_)=>{
            let billing=&row["billing"];
            let label=format!("  상품대금 {}  /  고객 청구 배송비 {}  /  부가세 {}",
                won(&billing["subtotal"]),won(&billing["shipping"]),won(&billing["vat"]));
            paint_text(ui,rect,&label,true,false);
            ui.interact(area,ui.make_persistent_id(("history-billing",id)),
                Sense::hover()).on_hover_text(label);
        },
    }
}

fn valid_date_input(value:&str)->bool{
    value.is_empty()||NaiveDate::parse_from_str(value,"%Y-%m-%d")
        .is_ok_and(|day|day.format("%Y-%m-%d").to_string()==value)
}
fn date_range_error(from:&str,to:&str)->Option<&'static str>{
    if !valid_date_input(from)||!valid_date_input(to){
        return Some("검색 날짜는 YYYY-MM-DD 형식으로 입력하세요.");
    }
    if !from.is_empty()&&!to.is_empty()&&from>to{
        return Some("시작일은 종료일보다 늦을 수 없습니다.");
    }
    None
}
/// Place controls in reverse insertion order inside an RTL parent.
/// Actual left-to-right order is:
/// date basis → [‹ start-date › calendar] → ~ → [‹ end-date › calendar]
/// → all dates. Each arrow changes only its adjacent date.
fn range_date_picker_rtl(
    ui:&mut Ui,label:&str,date:&mut String,month:&mut NaiveDate,
    id:&'static str
)->bool{
    // The RTL parent reverses insertion order. Screen order for EACH date:
    // [‹] [YYYY-MM-DD] [›] [달력], without visible "시작일/종료일" labels.
    let trigger=theme::action(
        ui,"달력",theme::ButtonRole::Standard,true
    ).on_hover_text(&format!("{label}을 달력에서 선택"));
    if trigger.clicked(){
        let day=NaiveDate::parse_from_str(date,"%Y-%m-%d")
            .unwrap_or_else(|_|NaiveDate::parse_from_str(
                &seoul_today(),"%Y-%m-%d").expect("valid today"));
        *month=month_start(day);
    }
    let mut changed=components::date_calendar(
        &trigger,month,date,id
    ).is_some_and(|day|{
        *date=day.format("%Y-%m-%d").to_string();
        true
    });
    if theme::action(ui,"›",theme::ButtonRole::Standard,true)
        .on_hover_text(&format!("{label} 하루 뒤")).clicked(){
        *date=move_date(date,1);
        changed=true;
    }
    changed|=components::flexible_text_field(
        ui,date,theme::SETTLEMENT_DATE_FIELD_WIDTH,"YYYY-MM-DD"
    ).on_hover_text(&format!("{label} 직접 입력"))
        .changed();
    if theme::action(ui,"‹",theme::ButtonRole::Standard,true)
        .on_hover_text(&format!("{label} 하루 전")).clicked(){
        *date=move_date(date,-1);
        changed=true;
    }
    changed
}
fn history_date_range_leading_rtl(
    ui:&mut Ui,basis:&mut HistoryDateBasis,
    from:&mut String,from_month:&mut NaiveDate
)->bool{
    ui.spacing_mut().item_spacing.x=theme::SPACE_8;
    let changed=range_date_picker_rtl(
        ui,"시작일",from,from_month,"order-history-date-from-calendar"
    );
    let previous=*basis;
    // 115pt caused the Korean label/dropdown affordance to clip.
    // Use a full-width content slot; never shorten selected_text.
    egui::ComboBox::from_id_salt("history-range-date-basis")
        .width(150.0)
        .selected_text(basis.label())
        .show_ui(ui,|options|{
            options.selectable_value(basis,HistoryDateBasis::Reception,"접수일 기준");
            options.selectable_value(basis,HistoryDateBasis::Payment,"입금일 기준");
        });
    changed||*basis!=previous
}
fn history_date_range_trailing_rtl(
    ui:&mut Ui,from:&mut String,to:&mut String,
    to_month:&mut NaiveDate
)->bool{
    ui.spacing_mut().item_spacing.x=theme::SPACE_8;
    let mut changed=false;
    if theme::action(ui,"전체 날짜",theme::ButtonRole::Standard,
        !from.is_empty()||!to.is_empty()).clicked(){
        from.clear();to.clear();changed=true;
    }
    changed|=range_date_picker_rtl(
        ui,"종료일",to,to_month,"order-history-date-to-calendar"
    );
    ui.label(typography::caption("~"));
    changed
}
/// The RIGHT edge is fixed; drawing each part in reverse in an RTL layout
/// restores its intended left-to-right visual sequence without guessing its
/// pixel width. Narrow panes get two separately right-anchored rows.
fn history_date_controls(
    ui:&mut Ui,basis:&mut HistoryDateBasis,
    from:&mut String,to:&mut String,
    from_month:&mut NaiveDate,to_month:&mut NaiveDate,
    compact:bool
)->bool{
    let mut changed=false;
    if compact{
        let w=ui.available_width().max(1.0);
        let (rect,_)=ui.allocate_exact_size(
            Vec2::new(w,theme::INPUT_HEIGHT*2.0+theme::SPACE_8),
            Sense::hover()
        );
        let start_width=RANGE_COMPACT_START_WIDTH.min(w);
        let end_width=RANGE_COMPACT_END_WIDTH.min(w);
        let first=Rect::from_min_size(
            egui::pos2(rect.right()-start_width,rect.top()),
            Vec2::new(start_width,theme::INPUT_HEIGHT)
        );
        let second=Rect::from_min_size(
            egui::pos2(rect.right()-end_width,
                rect.top()+theme::INPUT_HEIGHT+theme::SPACE_8),
            Vec2::new(end_width,theme::INPUT_HEIGHT)
        );
        ui.scope_builder(UiBuilder::new()
            .id_salt("history-range-start-line")
            .max_rect(first).layout(egui::Layout::right_to_left(Align::Center)),
            |line|{
                changed|=history_date_range_leading_rtl(
                    line,basis,from,from_month
                );
            });
        ui.scope_builder(UiBuilder::new()
            .id_salt("history-range-end-line")
            .max_rect(second).layout(egui::Layout::right_to_left(Align::Center)),
            |line|{
                changed|=history_date_range_trailing_rtl(
                    line,from,to,to_month
                );
            });
    }else{
        ui.with_layout(egui::Layout::right_to_left(Align::Center),|line|{
            changed|=history_date_range_trailing_rtl(
                line,from,to,to_month
            );
            changed|=history_date_range_leading_rtl(
                line,basis,from,from_month
            );
        });
    }
    // The arrows update their own dates only. Do not reset a calendar
    // merely because the user navigated its month without selecting a day.
    if changed{
        if let Ok(day)=NaiveDate::parse_from_str(from,"%Y-%m-%d"){
            *from_month=month_start(day);
        }
        if let Ok(day)=NaiveDate::parse_from_str(to,"%Y-%m-%d"){
            *to_month=month_start(day);
        }
    }
    changed
}

pub struct OrderHistoryManager {
    gateway:Arc<dyn BandoGateway>,
    receiver:Option<Receiver<Result<HistorySnapshot,String>>>,
    snapshot:Option<HistorySnapshot>,
    error:Option<String>,
    query:String,
    date_basis:HistoryDateBasis,
    date_from:String,
    date_to:String,
    from_input:String,
    to_input:String,
    from_month:NaiveDate,
    to_month:NaiveDate,
    expanded:BTreeSet<String>,
    sort:Option<Sort>,
}
impl OrderHistoryManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let today=seoul_today();
        let mut manager=Self {
            gateway,receiver:None,snapshot:None,error:None,
            query:String::new(),date_basis:HistoryDateBasis::Reception,
            date_from:today.clone(),date_to:today.clone(),
            from_input:today.clone(),to_input:today.clone(),
            from_month:month_start(NaiveDate::parse_from_str(&today,"%Y-%m-%d")
                .expect("today in YYYY-MM-DD")),
            to_month:month_start(NaiveDate::parse_from_str(&today,"%Y-%m-%d")
                .expect("today in YYYY-MM-DD")),
            expanded:BTreeSet::new(),sort:None,
        };
        manager.refresh(ctx);
        manager
    }
    /// Excel rows navigate by the order's reception date. A single-day
    /// range and Reception basis precisely identify the original order.
    pub fn open_order(&mut self,id:&str,date:&str,ctx:egui::Context){
        let parsed=seoul_date(date);
        self.date_basis=HistoryDateBasis::Reception;
        if let Ok(day)=NaiveDate::parse_from_str(&parsed,"%Y-%m-%d"){
            self.date_from=parsed.clone();
            self.date_to=parsed.clone();
            self.from_input=parsed.clone();
            self.to_input=parsed;
            self.from_month=month_start(day);
            self.to_month=month_start(day);
        }else{
            self.date_from.clear();self.date_to.clear();
            self.from_input.clear();self.to_input.clear();
        }
        self.query.clear();
        self.expanded.insert(id.to_owned());
        self.refresh(ctx);
    }
    pub fn refresh(&mut self,ctx:egui::Context){
        self.error=None;
        self.snapshot=None;
        let (sender,receiver)=mpsc::channel();
        self.receiver=Some(receiver);
        let gateway=Arc::clone(&self.gateway);
        let query=self.query.clone();
        let from=self.date_from.clone();
        let to=self.date_to.clone();
        let basis=self.date_basis;
        std::thread::spawn(move||{
            let answer=fetch(gateway.as_ref(),&query,&from,&to,basis);
            let _=sender.send(answer);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self){
        let answer=self.receiver.as_ref().map(Receiver::try_recv);
        match answer{
            Some(Ok(Ok(snapshot)))=>{
                self.snapshot=Some(snapshot);self.error=None;self.receiver=None;
            },
            Some(Ok(Err(error)))=>{
                self.snapshot=None;self.error=Some(error);self.receiver=None;
            },
            Some(Err(TryRecvError::Disconnected))=>{
                self.snapshot=None;self.receiver=None;
                self.error=Some("주문내역 조회 연결이 중단되었습니다.".into());
            },
            _=>{}
        }
    }
    pub fn show_page(&mut self,ui:&mut Ui){
        let page_origin_y=ui.cursor().top();
        ui.spacing_mut().scroll=theme::management_scroll_style();
        self.poll();
        components::management_page_title_top_gap(ui);
        components::management_top_content(ui,|top|{
            top.horizontal(|heading|{
                heading.label(typography::page_heading("주문 내역"));
                settings_help::icon(heading,"주문 내역");
            });
        });
        // No subtabs or KPIs: the approved common-card anchor owns the blank
        // area below this genuine title, without creating placeholder cards.
        ui.add_space(theme::SPACE_16);
        let mut changed=false;
        components::align_management_primary_card(ui,page_origin_y);
        // Match Freight > Shipping Group Assignment: a 16pt interior inset
        // on EVERY side, without moving the 59th-round outer card border.
        let card_min_height=components::management_primary_card_min_inner_height(
            ui,theme::SECTION_PADDING
        );
        theme::section_frame().show(ui,|section|{
            components::fill_management_primary_card(section,card_min_height);
            let mut table_scroll=theme::management_scroll_style();
            table_scroll.bar_inner_margin=0.0;
            table_scroll.bar_outer_margin=0.0;
            section.spacing_mut().scroll=table_scroll;
            // No additional list-strip margin: the card already owns 16pt.
            // A 36pt toolbar and a single 9pt separator match Freight.
            section.scope(|toolbar|{
                let full_width=toolbar.available_width().max(1.0);
                // Reserve the scrollbar rail outside the rightmost data
                // controls, exactly as on the Freight assignment page.
                let toolbar_data_width=(full_width
                    -toolbar.spacing().scroll.allocated_width()).max(1.0);
                // Reserve independent search/date slots. The date slot
                // right-anchors reverse-inserted RTL widgets so their visible
                // order stays natural and the last button meets the right edge.
                let one_row=toolbar_data_width>=
                    theme::MANAGEMENT_SEARCH_MIN_WIDTH
                    +theme::SPACE_16+RANGE_CONTROLS_WIDTH;
                if one_row {
                    let (row_rect,_)=toolbar.allocate_exact_size(
                        Vec2::new(full_width,theme::INPUT_HEIGHT),Sense::hover()
                    );
                    let search_rect=Rect::from_min_size(
                        row_rect.min,Vec2::new(
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,row_rect.height()
                        )
                    );
                    let range_rect=Rect::from_min_size(
                        egui::pos2(row_rect.left()+toolbar_data_width
                            -RANGE_CONTROLS_WIDTH,row_rect.top()),
                        Vec2::new(RANGE_CONTROLS_WIDTH,row_rect.height())
                    );
                    toolbar.scope_builder(UiBuilder::new()
                        .id_salt("history-range-search")
                        .max_rect(search_rect)
                        .layout(egui::Layout::left_to_right(Align::Center)),
                        |search|{
                            search.set_clip_rect(search_rect);
                            if components::flexible_text_field(
                                search,&mut self.query,
                                theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                                "거래처 / 주문 내용 검색"
                            ).changed(){changed=true;}
                        });
                    toolbar.scope_builder(UiBuilder::new()
                        .id_salt("history-range-controls")
                        .max_rect(range_rect)
                        .layout(egui::Layout::left_to_right(Align::Center)),
                        |dates|{
                            dates.set_clip_rect(range_rect);
                            changed|=history_date_controls(
                                dates,&mut self.date_basis,
                                &mut self.from_input,&mut self.to_input,
                                &mut self.from_month,&mut self.to_month,false
                            );
                        });
                }else{
                    if components::flexible_text_field(
                        toolbar,&mut self.query,
                        theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                        "거래처 / 주문 내용 검색"
                    ).changed(){changed=true;}
                    toolbar.add_space(theme::SPACE_8);
                    // At narrower dock widths the date group alone drops to
                    // its own right-aligned row. At very narrow widths it
                    // becomes two right-aligned rows, without clipping text.
                    let compact=toolbar_data_width<RANGE_CONTROLS_WIDTH;
                    let height=if compact {
                        theme::INPUT_HEIGHT*2.0+theme::SPACE_8
                    }else{theme::INPUT_HEIGHT};
                    let (row_rect,_)=toolbar.allocate_exact_size(
                        Vec2::new(full_width,height),Sense::hover()
                    );
                    let width=RANGE_CONTROLS_WIDTH.min(toolbar_data_width);
                    let range_rect=Rect::from_min_size(
                        egui::pos2(row_rect.left()+toolbar_data_width-width,
                            row_rect.top()),
                        Vec2::new(width,row_rect.height())
                    );
                    toolbar.scope_builder(UiBuilder::new()
                        .id_salt("history-range-controls-narrow")
                        .max_rect(range_rect)
                        .layout(egui::Layout::left_to_right(Align::Center)),
                        |dates|{
                            dates.set_clip_rect(range_rect);
                            changed|=history_date_controls(
                                dates,&mut self.date_basis,
                                &mut self.from_input,&mut self.to_input,
                                &mut self.from_month,&mut self.to_month,compact
                            );
                        });
                }
            });
            if changed && date_range_error(
                &self.from_input,&self.to_input
            ).is_none(){
                self.date_from=self.from_input.clone();
                self.date_to=self.to_input.clone();
                self.refresh(section.ctx().clone());
            }
            if let Some(message)=date_range_error(
                &self.from_input,&self.to_input
            ){
                theme::management_list_strip_frame().show(section,|ui|{
                    ui.label(typography::status(message,theme::DANGER));
                });
            }
            if let Some(error)=self.error.as_deref(){
                theme::management_list_strip_frame().show(section,|ui|{
                    ui.label(typography::status(error,theme::DANGER));
                });
            }
            if self.receiver.is_some(){
                theme::management_list_strip_frame().show(section,|ui|{
                    ui.label(typography::caption("주문 이력을 확인 중입니다."));
                });
            }
            // Freight baseline: 4pt above + 1pt dividing rule + 4pt
            // below, followed immediately by the fixed 36pt table header.
            let old_gap=section.spacing().item_spacing.y;
            section.spacing_mut().item_spacing.y=0.0;
            let (separator,_)=section.allocate_exact_size(
                Vec2::new(section.available_width(),
                    theme::SPACE_8+theme::SOFT_DIVIDER_WIDTH),Sense::hover()
            );
            section.painter().rect_filled(
                Rect::from_min_size(
                    egui::pos2(separator.left(),separator.top()+theme::SPACE_4),
                    Vec2::new(separator.width(),theme::SOFT_DIVIDER_WIDTH)
                ),0.0,theme::DIVIDER
            );
            let viewport=section.available_width().max(1.0);
            let gutter=section.spacing().scroll.allocated_width();
            let table_width=(viewport-gutter).max(MIN_WIDTH);
            let mut widths=COLS;
            // No unused horizontal space on fixed-format columns.
            widths[2]+=(table_width-MIN_WIDTH).max(0.0);
            let headings=["접수일","거래처","배송 건 / 상품","관리 상태",
                "청구 합계","입금 확인","처리"];
            let (head_rect,_)=section.allocate_exact_size(
                Vec2::new(viewport,HEADER_HEIGHT),Sense::hover());
            let mut header_origin=None;
            let mut pending_toggle=None;
            let snapshot=self.snapshot.as_ref();
            let lines=snapshot.map(|data|display_lines(data,&self.expanded,self.sort))
                .unwrap_or_default();
            let empty_message=if self.snapshot.as_ref()
                .is_some_and(|data|data.rows.is_empty()){
                Some(if self.query.is_empty()
                    && self.date_from.is_empty()&&self.date_to.is_empty()
                    && self.date_basis==HistoryDateBasis::Reception{
                    "저장된 주문이 없습니다."
                }else{"조건에 맞는 주문이 없습니다."})
            }else{None};
            // Use the exact remaining primary-card height. The old 60vh
            // cap left a dark blank strip beneath long order histories.
            let card_inner_bottom=section.min_rect().top()+card_min_height;
            let scroll_max_height=(card_inner_bottom-section.cursor().top())
                .max(ROW_HEIGHT);
            let scroll_bounds=section.available_rect_before_wrap();
            let scroll_track_bottom=(scroll_bounds.top()+scroll_max_height)
                .min(scroll_bounds.bottom());
            let scroll_track_top=(scroll_bounds.top()+3.0)
                .min(scroll_track_bottom-1.0);
            let scroll_track=Rect::from_min_max(
                egui::pos2(scroll_bounds.left(),scroll_track_top),
                egui::pos2(scroll_bounds.right(),scroll_track_bottom)
            );
            let output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .scroll_bar_rect(scroll_track)
                    .id_salt("order-history-table-rows")
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .max_height(scroll_max_height)
                    .auto_shrink([false,false]),
                section,table_width,ROW_HEIGHT,lines.len(),
                |body,visible|{
                    body.set_min_width(table_width);
                    body.spacing_mut().item_spacing.y=0.0;
                    let first=visible.start;
                    let origin=body.max_rect().min;
                    for line_index in visible {
                        let line=lines[line_index];
                        let row_rect=Rect::from_min_size(
                            egui::pos2(origin.x,
                                origin.y+(line_index-first) as f32*ROW_HEIGHT),
                            Vec2::new(table_width,ROW_HEIGHT));
                        header_origin.get_or_insert(row_rect.left());
                        body.scope_builder(UiBuilder::new()
                            .id_salt(("history-fixed-row",line_index))
                            .max_rect(row_rect)
                            .layout(egui::Layout::top_down(Align::Min)),|row_ui|{
                                row_ui.allocate_exact_size(
                                    Vec2::new(table_width,ROW_HEIGHT),Sense::hover());
                                let background=if matches!(line,HistoryLine::Order(_)){
                                    if line_index%2==0{theme::PANEL}
                                    else{theme::TABLE_ROW_ALT_BG}
                                }else{theme::WORKFLOW_SELECTED_BG};
                                row_ui.painter().rect_filled(row_rect,0.0,background);
                                row_ui.painter().line_segment(
                                    [row_rect.left_bottom(),row_rect.right_bottom()],
                                    Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER));
                                if let Some(snapshot)=snapshot {
                                    draw_line(row_ui,row_rect,line,snapshot,
                                        &self.expanded,&widths,&mut pending_toggle);
                                }
                            });
                    }
                    // Put the empty message inside the scrollable table,
                    // like Freight; placing it after a full-height ScrollArea
                    // would extend the entire outer card past the page bottom.
                    if let Some(message)=empty_message{
                        body.label(typography::caption(message));
                    }
                });
            let left=header_origin.unwrap_or(head_rect.left()-output.state.offset.x);
            let header_cells=cells(egui::pos2(left,head_rect.top()),&widths,HEADER_HEIGHT);
            let painter=section.painter().with_clip_rect(head_rect);
            painter.rect_filled(head_rect,0.0,theme::RAISED);
            // Freight title text centers in the 34pt line box ABOVE the
            // last 2pt filled rule; the table below begins without a gap.
            let header_outline_height=2.0_f32;
            let header_text_center_y=head_rect.top()
                +(head_rect.height()-header_outline_height)/2.0;
            let mut next_sort=None;
            for (index,title) in headings.iter().enumerate(){
                let sort_col=match index{
                    0=>Some(SortColumn::Date),1=>Some(SortColumn::Client),
                    2=>Some(SortColumn::Shipment),3=>Some(SortColumn::Status),
                    4=>Some(SortColumn::Total),_=>None,
                };
                let cell=header_cells[index].intersect(head_rect);
                let indicator=sort_col.map(|column|{
                    match self.sort.filter(|state|state.column==column)
                        .map(|s|s.direction){
                        Some(SortDirection::Asc)=>"▲",
                        Some(SortDirection::Desc)=>"▼",
                        None=>"↕",
                    }
                });
                let heading=if let Some(mark)=indicator{
                    format!("{title} {mark}")
                }else{(*title).into()};
                let label=typography::freight_table_header_galley(section,&heading);
                let x=if index==4{
                    (header_cells[index].right()-theme::SPACE_8-label.size().x)
                        .max(header_cells[index].left()+theme::SPACE_8)
                }else if matches!(index,3|5|6){
                    (header_cells[index].center().x-label.size().x/2.0)
                        .max(header_cells[index].left()+6.0)
                }else{header_cells[index].left()+6.0};
                painter.galley(egui::pos2(x,
                    header_text_center_y-label.size().y/2.0),
                    label,theme::SECONDARY);
                if let Some(column)=sort_col {
                    let response=section.interact(cell,
                        section.make_persistent_id(("order-history-sort",index)),
                        Sense::click());
                    if response.clicked(){
                        next_sort=Some(Sort::next(self.sort,column));
                    }
                }
            }
            if let Some(sort)=next_sort {self.sort=sort;}
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(head_rect.left(),
                        head_rect.bottom()-header_outline_height),
                    head_rect.right_bottom()
                ),0.0,theme::TABLE_HEADER_OUTLINE
            );
            section.spacing_mut().item_spacing.y=old_gap;
            if let Some(order_id)=pending_toggle {
                if !self.expanded.insert(order_id.clone()){
                    self.expanded.remove(&order_id);
                }
            }
        });
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn seoul_date_move_and_read_only_projection_labels(){
        assert_eq!(seoul_date("2026-11-19T16:00:00Z"),"2026-11-20");
        assert_eq!(move_date("2026-11-01",-1),"2026-10-31");
        assert_eq!(HistoryDateBasis::Reception.key(),"RECEPTION");
        assert_eq!(HistoryDateBasis::Payment.label(),"입금일 기준");
        assert!(date_range_error("2026-11-01","2026-11-30").is_none());
        assert!(date_range_error("2026-11-30","2026-11-01").is_some());
        assert!(date_range_error("2026-02-30","").is_some());
        assert!(date_range_error("","2026-11-30").is_none());
        let mut from="2026-11-20".to_owned();
        let to="2026-11-22".to_owned();
        from=move_date(&from,-1);
        assert_eq!((&*from,&*to),("2026-11-19","2026-11-22"));
        from=move_date(&from,1);
        assert_eq!((&*from,&*to),("2026-11-20","2026-11-22"));
        assert_eq!(model_label("BD 709 / 블랙"),"709 / 블랙");
        assert_eq!(won(&Value::Null),"확인 필요");
        let leap=NaiveDate::from_ymd_opt(2024,2,1).unwrap();
        let days=calendar_dates(leap);
        assert_eq!(days.len(),35);
        assert_eq!(days.iter().filter(|d|d.is_some()).count(),29);
        assert_eq!(days[3],Some(leap));
        assert_eq!(shift_calendar_month(leap,12),
            NaiveDate::from_ymd_opt(2025,2,1).unwrap());
        assert_eq!(shift_calendar_month(leap,-2),
            NaiveDate::from_ymd_opt(2023,12,1).unwrap());
        let snapshot=HistorySnapshot{state:json!({"workflow":{"shipments":[]}}),rows:vec![
            json!({"order":{"id":"o1"},"recipients":[{"id":"r1","items":[
                {"id":"i1","name":"BD 709"}]}],"status":"처리 중"})
        ]};
        let mut expanded=BTreeSet::new();
        assert_eq!(display_lines(&snapshot,&expanded,None).len(),1);
        expanded.insert("o1".into());
        assert_eq!(display_lines(&snapshot,&expanded,None).len(),4);
    }
}
