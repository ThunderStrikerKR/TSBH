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

const ROW_HEIGHT: f32 = theme::MANAGEMENT_COMPACT_ROW_HEIGHT;
// An expanded detail is a contiguous card + two-button group. Do not cap
// the card at 780/1140pt: that used to leave a huge empty strip to its right.
// Give the actions a fixed slot and allocate ALL remaining width to products.
const DETAIL_ACTIONS_WIDTH: f32 = 208.0;
const DETAIL_ACTIONS_GAP: f32 = theme::SPACE_8;
const DETAIL_RIGHT_INSET: f32 = theme::SPACE_12;
// All THREE visible distances are exactly 8pt: parent-to-first card,
// between sibling cards, and last card-to-next parent. For adjacent rows,
// each row owns half of the 8pt inter-card gap.
const DETAIL_BLOCK_HEIGHT: f32 = theme::ACTION_STANDARD_HEIGHT;
const DETAIL_GROUP_GAP: f32 = theme::SPACE_8;
#[derive(Clone,Copy,Debug)]
struct DetailSpacing { top:f32, bottom:f32 }
impl DetailSpacing {
    fn symmetric()->Self{Self{
        top:DETAIL_GROUP_GAP/2.0,
        bottom:DETAIL_GROUP_GAP/2.0
    }}
}
const EXCEL_ITEM_MARKER: &str = "◆ ";
const HEADER_HEIGHT: f32 = theme::MANAGEMENT_TABLE_HEADER_HEIGHT;
const COL_GAP: f32 = theme::SPACE_8;
// Compact fixed-width metadata columns; the shipping / product column takes
// ALL remaining width (including when the main pane is resized).
// The action column holds TWO order-level controls side by side without
// reducing the product column's 300pt minimum.
// Date text is YYYY-MM-DD; client name shows at most ten characters;
// status is one of four known values; invoice is at most 10M KRW.
// All seven approved widths and the 1242pt minimum are owned by theme.rs.
// Local names stay as aliases for the existing cell-layout helpers and tests.
const COLS: [f32;7] = theme::ORDER_HISTORY_COL_WIDTHS;
const MIN_WIDTH: f32 = theme::ORDER_HISTORY_TABLE_MIN_WIDTH;
// Natural width of [date basis, previous, start date/calendar, end
// date/calendar, next, all dates]. The search stays outside this right slot.
// The group is measured from the real control sizes, not the previously
// over-reserved 940pt strip; this keeps all controls right-aligned.
const HISTORY_DATE_FIELD_WIDTH:f32=106.0;
const HISTORY_DATE_BASIS_WIDTH:f32=theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH;
const RANGE_CONTROLS_WIDTH:f32=704.0;
const RANGE_COMPACT_START_WIDTH:f32=390.0;
const RANGE_COMPACT_END_WIDTH:f32=350.0;
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
fn memo_last_updated_label(raw:&str)->String{
    DateTime::parse_from_rfc3339(raw).ok()
        .map(|stamp|stamp.with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
            .format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(||"기록 없음".to_owned())
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
fn order_items_single_line(row:&Value)->String{
    let items=rows(&row["recipients"]).iter()
        .flat_map(|recipient|rows(&recipient["items"]).iter())
        .map(|item|format!("{EXCEL_ITEM_MARKER}{} × {}",
            model_label(field(item,"name")),
            item["quantity"].as_i64().map(|quantity|quantity.to_string())
                .unwrap_or_else(||"수량 미확인".to_owned())))
        .collect::<Vec<_>>();
    if items.is_empty(){"품목 없음".to_owned()}
    else{items.join("  ")}
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

struct MemoEditor {
    shipment_id:String,
    expected_revision:Option<u64>,
    original:String,
    draft:String,
    updated_at:String,
}
struct MemoSaveJob {
    shipment_id:String,
    receiver:Receiver<Result<Value,String>>,
}

#[derive(Clone,Copy,PartialEq,Eq)]
enum SortColumn { Date, Client, Shipment, Status, Total, PaymentConfirmation }
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

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum HistoryLine {
    Order(usize),
    // One information block per ACTUAL shipment; None is a recipient
    // waiting for its first shipment. Both keep the one-line detail layout.
    Recipient(usize,usize,Option<usize>),
}

// Expanded order is an accordion: opening a different order closes the
// previous one; clicking the already-open order collapses it.
fn toggle_history_expansion(expanded:&mut BTreeSet<String>,order_id:&str){
    if expanded.len()==1 && expanded.contains(order_id){
        expanded.clear();
    }else{
        expanded.clear();
        expanded.insert(order_id.to_owned());
    }
}

// Determine tree endings from the actual flat display order. This remains
// correct when one recipient has several shipments or history is sorted.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum HistoryBranch { Root, Continue, Last }
fn history_branch(lines:&[HistoryLine],index:usize)->Option<HistoryBranch>{
    let current=*lines.get(index)?;
    let next=lines.get(index+1);
    match current {
        HistoryLine::Order(order)=>{
            matches!(next,Some(HistoryLine::Recipient(child,_,_)) if *child==order)
                .then_some(HistoryBranch::Root)
        },
        HistoryLine::Recipient(order,_,_)=>{
            if matches!(next,Some(HistoryLine::Recipient(child,_,_)) if *child==order) {
                Some(HistoryBranch::Continue)
            }else{Some(HistoryBranch::Last)}
        },
    }
}
fn history_detail_spacing(lines:&[HistoryLine],index:usize)->DetailSpacing{
    let Some(HistoryLine::Recipient(order,_,_))=lines.get(index) else{
        return DetailSpacing::symmetric();
    };
    let previous_same=matches!(
        index.checked_sub(1).and_then(|i|lines.get(i)),
        Some(HistoryLine::Recipient(other,_,_)) if other==order
    );
    let next_same=matches!(
        lines.get(index+1),
        Some(HistoryLine::Recipient(other,_,_)) if other==order
    );
    DetailSpacing {
        top:if previous_same{DETAIL_GROUP_GAP/2.0}else{DETAIL_GROUP_GAP},
        bottom:if next_same{DETAIL_GROUP_GAP/2.0}else{DETAIL_GROUP_GAP},
    }
}
// The branch tip and the shipment block share the SAME horizontal anchor.
fn history_detail_block_rect(
    row_rect:Rect,widths:&[f32;7],spacing:DetailSpacing
)->Rect{
    let left=row_rect.left()+widths[0]-theme::SPACE_4;
    let width=(row_rect.right()-left-DETAIL_RIGHT_INSET
        -DETAIL_ACTIONS_WIDTH-DETAIL_ACTIONS_GAP).max(1.0);
    let block_height=(row_rect.height()-spacing.top-spacing.bottom)
        .max(DETAIL_BLOCK_HEIGHT);
    Rect::from_min_size(
        egui::pos2(left,row_rect.top()+spacing.top),
        Vec2::new(width,block_height)
    )
}
// The expanded shipment is a compact standalone summary, not seven table
// cells stretched across the history grid.
fn history_detail_actions_rect(row_rect:Rect,block:Rect)->Rect{
    Rect::from_min_max(
        egui::pos2(block.right()+DETAIL_ACTIONS_GAP,
            block.center().y-theme::ACTION_STANDARD_HEIGHT/2.0),
        egui::pos2(row_rect.right()-DETAIL_RIGHT_INSET,
            block.center().y+theme::ACTION_STANDARD_HEIGHT/2.0),
    )
}
fn history_detail_cells(block:Rect)->[Rect;4]{
    let content=block.shrink2(Vec2::new(theme::SPACE_12,0.0));
    let recipient=96.0;
    let freight=116.0;
    let tracking=156.0;
    let product=(content.width()-recipient-freight-tracking
        -3.0*theme::SPACE_8).max(1.0);
    let sizes=[recipient,product,freight,tracking];
    let mut left=content.left();
    std::array::from_fn(|i|{
        let rect=Rect::from_min_size(
            egui::pos2(left,content.top()),Vec2::new(sizes[i],content.height())
        );
        left+=sizes[i]+theme::SPACE_8;
        rect
    })
}
fn paint_history_branch(ui:&Ui,row_rect:Rect,widths:&[f32;7],
    branch:HistoryBranch,spacing:DetailSpacing){
    let painter=ui.painter().with_clip_rect(row_rect.intersect(ui.clip_rect()));
    // The last 12pt of the reception-date cell is blank even when a
    // YYYY-MM-DD is present; do not displace the date or column divider.
    let x=row_rect.left()+widths[0]-theme::SPACE_12;
    let detail=history_detail_block_rect(row_rect,widths,spacing);
    let tip=detail.left();
    let center=if branch==HistoryBranch::Root{
        row_rect.center().y
    }else{
        detail.center().y
    };
    let stroke=Stroke::new(1.25,theme::MUTED);
    match branch{
        HistoryBranch::Root=>{
            painter.circle_filled(egui::pos2(x,center),2.0,theme::MUTED);
            painter.line_segment([
                egui::pos2(x,center),egui::pos2(x,row_rect.bottom())],stroke);
        },
        HistoryBranch::Continue=>{
            painter.line_segment([
                egui::pos2(x,row_rect.top()),egui::pos2(x,row_rect.bottom())],stroke);
            painter.line_segment([
                egui::pos2(x,center),egui::pos2(tip,center)],stroke);
        },
        HistoryBranch::Last=>{
            painter.line_segment([
                egui::pos2(x,row_rect.top()),egui::pos2(x,center)],stroke);
            painter.line_segment([
                egui::pos2(x,center),egui::pos2(tip,center)],stroke);
        },
    }
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
                // This header now shows products, not recipient names.
                SortColumn::Shipment=>rows(&row["recipients"]).iter()
                    .flat_map(|recipient|rows(&recipient["items"]).iter())
                    .next()
                    .map(|item|model_label(field(item,"name")).to_lowercase())
                    .unwrap_or_default(),
                SortColumn::Status=>field(row,"status").to_owned(),
                SortColumn::Total=>row["total"].as_i64()
                    .map(|n|format!("{:020}",n.max(0)))
                    .unwrap_or_else(||"~".into()),
                // The approval date, not the bank paidAt timestamp.
                // YYYY-MM-DD sorts chronologically as ordinary text.
                SortColumn::PaymentConfirmation=>field(
                    &row["display"]["employeePaymentConfirmation"],"date"
                ).to_owned(),
            }
        });
        if sort.direction==SortDirection::Desc{indices.reverse();}
        if sort.column==SortColumn::PaymentConfirmation{
            // An unconfirmed order must be last in BOTH sort directions.
            // Stable sort preserves the date order of confirmed entries.
            indices.sort_by_key(|i|field(
                &snapshot.rows[*i]["display"]["employeePaymentConfirmation"],
                "date"
            ).is_empty());
        }
    }
    let mut lines=Vec::new();
    for index in indices {
        let row=&snapshot.rows[index];
        lines.push(HistoryLine::Order(index));
        if !expanded.contains(field(&row["order"],"id")){continue}
        let shipments=rows(&snapshot.state["workflow"]["shipments"]);
        for (recipient_index,recipient) in rows(&row["recipients"]).iter().enumerate(){
            let recipient_id=field(recipient,"id");
            let mut found=false;
            for (shipment_index,shipment) in shipments.iter().enumerate(){
                if field(shipment,"orderId")==field(&row["order"],"id") &&
                   field(shipment,"recipientId")==recipient_id &&
                   shipment["legacyArchived"]!=true &&
                   !rows(&snapshot.state["workflow"]["groups"]).iter().any(|group|
                       field(group,"id")==field(shipment,"groupId") &&
                       !group["deletedAt"].is_null() &&
                       group["deletedAt"].as_str()!=Some("")
                   ) {
                    lines.push(HistoryLine::Recipient(
                        index,recipient_index,Some(shipment_index)
                    ));
                    found=true;
                }
            }
            if !found{
                // A confirmed recipient remains inspectable before export.
                lines.push(HistoryLine::Recipient(index,recipient_index,None));
            }
        }
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
/// The header background and its bottom outline stay pinned to the
/// viewport, but its columns and vertical separators scroll together with
/// the body. Never derive the separators from the unscrolled header rect.
fn history_scrolled_header_row(
    header_viewport:Rect,scrolled_left:f32,table_width:f32
)->Rect{
    Rect::from_min_size(
        egui::pos2(scrolled_left,header_viewport.top()),
        Vec2::new(table_width,header_viewport.height())
    )
}
fn paint_column_dividers(
    ui:&Ui,rect:Rect,widths:&[f32;7],detail:bool,content_clip:Rect
){
    let columns=cells(rect.min,widths,rect.height());
    let painter=ui.painter().with_clip_rect(content_clip.intersect(ui.clip_rect()));
    // Expanded rows merge columns 2–6 to show long recipient, product,
    // freight and billing details. Do not draw lines THROUGH merged text.
    let separators=if detail{2}else{6};
    for column in columns.iter().take(separators){
        let x=column.right()+COL_GAP/2.0;
        painter.line_segment(
            [egui::pos2(x,rect.top()),egui::pos2(x,rect.bottom())],
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
        );
    }
}
fn paint_text(ui:&Ui,rect:Rect,value:&str,
    role:typography::ManagementTableCellRole,right:bool){
    if rect.width()<=0.0{return}
    let color=typography::management_table_cell_color(role);
    let galley=typography::management_table_cell_galley(ui,value,role);
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
    let galley=typography::management_table_cell_galley(
        ui,value,typography::ManagementTableCellRole::Status
    );
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
// One distinct product per line in EXPANDED shipment detail only.
// The parent order summary retains its compact single-line formatter.
fn history_items_multiline(items:&Value)->String{
    let products=rows(items).iter().map(|item|{
        format!("{EXCEL_ITEM_MARKER}{} × {}",
            field(item,"name"),
            item["quantity"].as_i64().map(|n|n.to_string())
                .unwrap_or_else(||"수량 미확인".to_owned()))
    }).collect::<Vec<_>>();
    if products.is_empty(){"상품 정보 없음".to_owned()}
    else{products.join("\n")}
}
fn history_recipient_products(row:&Value,recipient_index:usize)->String{
    history_items_multiline(&row["recipients"][recipient_index]["items"])
}
fn history_detail_products(snapshot:&HistorySnapshot,order_index:usize,
    recipient_index:usize,shipment_index:Option<usize>)->String{
    let row=&snapshot.rows[order_index];
    let shipment=shipment_index.and_then(|index|
        rows(&snapshot.state["workflow"]["shipments"]).get(index)
    );
    shipment.and_then(|ship|ship.get("recipientSnapshot"))
        .filter(|value|value.is_object())
        .and_then(|value|value.get("items"))
        .filter(|value|value.is_array())
        .map(history_items_multiline)
        .unwrap_or_else(||history_recipient_products(row,recipient_index))
}
fn history_line_height(ui:&Ui,line:HistoryLine,
    snapshot:&HistorySnapshot,widths:&[f32;7],spacing:DetailSpacing)->f32{
    let HistoryLine::Recipient(order_index,recipient_index,shipment_index)=line
        else {return ROW_HEIGHT};
    let product=history_detail_products(snapshot,order_index,recipient_index,
        shipment_index);
    let table_width=widths.iter().sum::<f32>()+COL_GAP*6.0;
    let placeholder=Rect::from_min_size(
        egui::pos2(0.0,0.0),Vec2::new(table_width,ROW_HEIGHT)
    );
    let card=history_detail_block_rect(placeholder,widths,spacing);
    let product_cell=history_detail_cells(card)[1];
    let galley=typography::freight_table_wrapped_galley(
        ui,&product,(product_cell.width()-theme::SPACE_12).max(1.0)
    );
    // Only the inner text padding belongs to the card's content height.
    // Group spacing is accounted for independently, so first/middle/last
    // rows all have identical visible gaps even for multiline products.
    let block_height=(galley.size().y+2.0*theme::SPACE_4).ceil()
        .max(DETAIL_BLOCK_HEIGHT);
    block_height+spacing.top+spacing.bottom
}
fn paint_detail_products(ui:&Ui,rect:Rect,text:&str){
    let galley=typography::freight_table_wrapped_galley(
        ui,text,(rect.width()-theme::SPACE_12).max(1.0)
    );
    let pos=egui::pos2(rect.left()+6.0,
        rect.center().y-galley.size().y/2.0);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(pos,galley,theme::TEXT);
}
fn history_recipient_product_notes(row:&Value,recipient_index:usize)->String{
    let recipient=&row["recipients"][recipient_index];
    let notes=rows(&recipient["items"]).iter().filter_map(|item|{
        let linked=rows(&row["display"]["references"]).iter().find(|reference|
            field(reference,"recipientId")==field(recipient,"id")
            && field(reference,"itemId")==field(item,"id"))
            .map(|ref_row|&ref_row["link"]);
        match linked{
            Some(link) if field(link,"via")=="HISTORICAL"=>
                Some(format!("{}: 현재 연결 상품 {}",
                    field(item,"name"),field(link,"name"))),
            Some(Value::Null)|None if !field(item,"productId").is_empty()=>
                Some(format!("{}: 상품 연결 확인 필요",field(item,"name"))),
            _=>None
        }
    }).collect::<Vec<_>>();
    notes.join("\n")
}
fn history_shipment_logistics(shipment:Option<&Value>)->(String,String){
    // A detail block belongs to exactly one shipment. Never merge freight or
    // tracking from two distinct shipments for the same recipient.
    let freight=shipment.into_iter()
        .flat_map(|s|s["actualFreight"].as_object().into_iter())
        .flat_map(|values|values.values())
        .filter_map(Value::as_i64).map(components::display_won)
        .collect::<Vec<_>>();
    let tracking=shipment.into_iter()
        .flat_map(|s|s["tracking"].as_object().into_iter())
        .flat_map(|values|values.values())
        .filter_map(Value::as_str).filter(|value|!value.is_empty())
        .map(str::to_owned).collect::<Vec<_>>();
    (
        if freight.is_empty(){"운임 미확인".to_owned()}else{freight.join(" / ")},
        if tracking.is_empty(){"운송장 미확인".to_owned()}
        else{tracking.join(" / ")}
    )
}
fn draw_line(ui:&mut Ui,rect:Rect,line:HistoryLine,snapshot:&HistorySnapshot,
    expanded:&BTreeSet<String>,widths:&[f32;7],spacing:DetailSpacing,
    pending_toggle:&mut Option<String>,pending_memo:&mut Option<MemoEditor>,
    pending_detail:&mut Option<String>){
    let area=rect.intersect(ui.clip_rect());
    let order_index=match line{
        HistoryLine::Order(i)|HistoryLine::Recipient(i,_,_)=>i,
    };
    let row=&snapshot.rows[order_index];
    let id=field(&row["order"],"id");
    match line {
        HistoryLine::Order(_)=>{
            let columns=cells(rect.min,widths,rect.height());
            paint_text(ui,columns[0],&order_date(row),typography::ManagementTableCellRole::Description,false);
            paint_text(ui,columns[1],&display_client_name(order_client(row)),
                typography::ManagementTableCellRole::ClientName,false);
            ui.interact(columns[1].intersect(area),
                ui.make_persistent_id(("history-client-name",id)),Sense::hover())
                .on_hover_text(order_client(row));
            // Main list: ONLY product name × quantity, all items in one line.
            // Click anywhere except the action button to expand all details.
            let summary=order_items_single_line(row);
            // The order disclosure uses the same stroked chevron as
            // workflow cards, category trees, date arrows and dropdowns.
            // Keep the existing chevron center and the row's click area.
            let center=egui::pos2(columns[2].left()+11.0,columns[2].center().y);
            components::paint_management_chevron(
                &ui.painter().with_clip_rect(columns[2].intersect(area)),
                center,
                if expanded.contains(id){
                    components::ManagementChevronDirection::Down
                }else{components::ManagementChevronDirection::Right}
            );
            // The 10pt arrow reaches 16pt from the cell's left edge when
            // expanded. Reserve a 4pt text gap without changing column or row.
            let summary_rect=Rect::from_min_max(
                egui::pos2(columns[2].left()+20.0,columns[2].top()),
                columns[2].right_bottom()
            );
            paint_text(ui,summary_rect,&summary,typography::ManagementTableCellRole::ProductName,false);
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
                paint_text(ui,columns[4],&total,typography::ManagementTableCellRole::RowTotal,true);
            }else{
                let upper=Rect::from_min_max(columns[4].min,
                    egui::pos2(columns[4].right(),columns[4].center().y));
                let lower=Rect::from_min_max(egui::pos2(columns[4].left(),
                    columns[4].center().y),columns[4].max);
                paint_text(ui,upper,&total,typography::ManagementTableCellRole::RowTotal,true);
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
            // Keep two INDEPENDENT controls in this fixed 264pt column.
            // Main-justify expanded the first button to the full cell width,
            // silently clipping the second one. Allocate exactly half each.
            let control=columns[6].shrink2(
                Vec2::new(theme::SPACE_8,theme::SPACE_4)
            );
            let action_gap=theme::SPACE_4;
            let action_width=(control.width()-action_gap)/2.0;
            ui.scope_builder(UiBuilder::new()
                .id_salt(("order-history-actions",id))
                .max_rect(control)
                .layout(egui::Layout::left_to_right(Align::Center)),|actions|{
                    actions.set_clip_rect(control.intersect(area));
                    actions.set_width(control.width());
                    actions.spacing_mut().item_spacing.x=action_gap;
                    // Copy the FULL order's existing Rust-generated message.
                    if theme::action_with_min_width(actions,"운송장 복사",
                        theme::ButtonRole::TableSecondary,!message.is_empty(),
                        action_width).clicked(){
                        actions.ctx().copy_text(message.to_owned());
                    }
                    if theme::action_with_min_width(actions,"발주서 상세",
                        theme::ButtonRole::TableSecondary,true,
                        action_width).clicked(){
                        *pending_detail=Some(id.to_owned());
                    }
                });
        },
        HistoryLine::Recipient(_,recipient_index,shipment_index)=>{
            let recipient=&row["recipients"][recipient_index];
            let columns=history_detail_cells(history_detail_block_rect(rect,widths,spacing));
            // The four fields belong to ONE shipment block and are independent
            // of the parent table's seven column locations.
            let shipment=shipment_index.and_then(|index|
                rows(&snapshot.state["workflow"]["shipments"]).get(index)
            );
            // A transferred shipment can preserve its original recipient
            // snapshot even if the current order recipient was later edited.
            let shipment_recipient=shipment
                .and_then(|s|s.get("recipientSnapshot"))
                .filter(|snapshot|snapshot.is_object());
            let display_recipient=shipment_recipient.unwrap_or(recipient);
            let recipient_name=field(display_recipient,"name");
            let name=if recipient_name.is_empty(){"수취인 미확인"}
                else{recipient_name};
            let products=history_detail_products(snapshot,order_index,
                recipient_index,shipment_index);
            let product_notes=history_recipient_product_notes(row,recipient_index);
            let (freight,tracking)=history_shipment_logistics(shipment);
            let freight_label=if freight=="운임 미확인"{freight.clone()}
                else{format!("운임 {freight}")};
            let tracking_label=if tracking=="운송장 미확인"{tracking.clone()}
                else{format!("송장 {tracking}")};
            paint_text(ui,columns[0],name,typography::ManagementTableCellRole::ClientName,false);
            paint_detail_products(ui,columns[1],&products);
            paint_text(ui,columns[2],&freight_label,typography::ManagementTableCellRole::Description,false);
            paint_text(ui,columns[3],&tracking_label,typography::ManagementTableCellRole::Description,false);
            let address=field(display_recipient,"address");
            let address=if address.is_empty(){"배송 주소 확인 필요"}
                else{address};
            ui.interact(columns[0].intersect(area),
                ui.make_persistent_id(("history-recipient",id,recipient_index,shipment_index)),
                Sense::hover()).on_hover_text(
                    format!("수취인: {name}\n배송 주소: {address}\n발송건: {}",
                        shipment.map(|s|field(s,"id")).unwrap_or("발송 대기"))
                );
            ui.interact(columns[1].intersect(area),
                ui.make_persistent_id(("history-recipient-products",id,recipient_index,shipment_index)),
                Sense::hover()).on_hover_text(if product_notes.is_empty(){
                    products.clone()
                }else{
                    format!("{products}\n\n{product_notes}")
                });
            ui.interact(columns[2].intersect(area),
                ui.make_persistent_id(("history-recipient-freight",id,recipient_index,shipment_index)),
                Sense::hover()).on_hover_text(format!("실제 운임: {freight}"));
            ui.interact(columns[3].intersect(area),
                ui.make_persistent_id(("history-recipient-tracking",id,recipient_index,shipment_index)),
                Sense::hover()).on_hover_text(format!("운송장 번호: {tracking}"));
            // Actions live OUTSIDE the compact shipment card. Both actions
            // target this one shipment; the order-level copy remains separate.
            let block=history_detail_block_rect(rect,widths,spacing);
            let actions_rect=history_detail_actions_rect(rect,block);
            let memo_text=shipment.map(|s|field(s,"managementMemo")).unwrap_or("");
            ui.scope_builder(UiBuilder::new()
                .id_salt(("history-shipment-actions",id,recipient_index,shipment_index))
                .max_rect(actions_rect)
                .layout(egui::Layout::left_to_right(Align::Center)),|actions|{
                    actions.set_clip_rect(actions_rect.intersect(area));
                    actions.spacing_mut().item_spacing.x=theme::SPACE_8;
                    let memo_button=theme::action(actions,"관리 메모",
                        theme::ButtonRole::TableSecondary,shipment.is_some());
                    if memo_button.on_hover_text(if memo_text.is_empty(){
                        "이 발송 건의 관리 메모를 작성하거나 수정합니다."
                    }else{memo_text}).clicked(){
                        if let Some(ship)=shipment{
                            *pending_memo=Some(MemoEditor{
                                shipment_id:field(ship,"id").to_owned(),
                                expected_revision:snapshot.state["revision"].as_u64(),
                                original:memo_text.to_owned(),
                                draft:memo_text.to_owned(),
                                updated_at:field(ship,"managementMemoUpdatedAt").to_owned(),
                            });
                        }
                    }
                    if theme::action(actions,"운송장 복사",
                        theme::ButtonRole::TableSecondary,
                        shipment.is_some() && tracking!="운송장 미확인"
                    ).clicked(){
                        actions.ctx().copy_text(tracking.clone());
                    }
                });
        },
    }
}

fn valid_date_input(value:&str)->bool{
    value.is_empty()||NaiveDate::parse_from_str(value,"%Y-%m-%d")
        .is_ok_and(|day|day.format("%Y-%m-%d").to_string()==value)
}
pub(crate) fn date_range_error(from:&str,to:&str)->Option<&'static str>{
    if !valid_date_input(from)||!valid_date_input(to){
        return Some("검색 날짜는 YYYY-MM-DD 형식으로 입력하세요.");
    }
    if !from.is_empty()&&!to.is_empty()&&from>to{
        return Some("시작일은 종료일보다 늦을 수 없습니다.");
    }
    None
}
/// Flat segment, sized by egui itself. Never use Ui::put here:
/// Ui::put advances the parent's RTL layout cursor and previously collapsed
/// the remaining date controls onto the right edge.
fn history_date_segment(
    ui:&mut Ui,label:&str,width:f32
)->egui::Response{
    // Use the same drawn chevron as the dropdown, rotated horizontally.
    // Keep calendar text and the segment's click geometry unchanged.
    let direction=match label {
        "‹"=>Some(components::ManagementChevronDirection::Left),
        "›"=>Some(components::ManagementChevronDirection::Right),
        _=>None
    };
    let display=if direction.is_some(){""}else{label};
    let response=ui.add_sized(
        Vec2::new(width,theme::ACTION_STANDARD_HEIGHT),
        egui::Button::new(typography::action_label(
            display,typography::FONT_STANDARD_ACTION,false,theme::TEXT
        ))
            .fill(theme::INPUT)
            .stroke(Stroke::NONE)
            .corner_radius(0)
    );
    if let Some(direction)=direction {
        let painter=ui.painter().with_clip_rect(response.rect);
        components::paint_management_chevron(
            &painter,response.rect.center(),direction
        );
    }
    response
}
/// Reserve exactly one 36pt outer row. Frame::show combined with an
/// additional horizontal/with_layout child could grow the measured row by
/// 4pt on Windows and push it below the 36pt search field. Allocate the
/// outer rectangle ONCE, then draw four LTR controls in its 34pt interior.
/// This differs from the former broken Ui::put approach: the child Ui owns
/// one normal, sequential left-to-right layout; the parent cursor advances
/// only for its explicitly reserved 36pt rectangle.
fn range_date_picker_rtl(
    ui:&mut Ui,label:&str,all_dates:bool,date:&mut String,month:&mut NaiveDate,
    id:&'static str
)->bool{
    const PREV:f32=theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH;
    const NEXT:f32=theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH;
    const CALENDAR:f32=theme::MANAGEMENT_DATE_CALENDAR_BUTTON_WIDTH;
    const FRAME_INSET:f32=1.0;
    const GROUP_WIDTH:f32=PREV+HISTORY_DATE_FIELD_WIDTH+NEXT
        +CALENDAR+FRAME_INSET*2.0;
    const INNER_HEIGHT:f32=theme::ACTION_STANDARD_HEIGHT;
    let (outer,_)=ui.allocate_exact_size(
        Vec2::new(GROUP_WIDTH,theme::INPUT_HEIGHT),Sense::hover()
    );
    let inner=Rect::from_center_size(
        outer.center(),Vec2::new(GROUP_WIDTH-2.0*FRAME_INSET,INNER_HEIGHT)
    );
    debug_assert!((INNER_HEIGHT+FRAME_INSET*2.0-theme::INPUT_HEIGHT).abs()<0.01);
    debug_assert!((outer.center().y-inner.center().y).abs()<0.01);

    ui.painter().rect_filled(
        outer,theme::CONTROL_RADIUS as f32,theme::INPUT
    );
    let mut changed=false;
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(("history-date-control",id))
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(Align::Center)),
        |row|{
            row.set_width(inner.width());
            row.set_height(inner.height());
            row.spacing_mut().item_spacing=Vec2::ZERO;
            let prev=history_date_segment(row,"‹",PREV)
                .on_hover_text(&format!("{label} 하루 전"));
            if prev.clicked(){
                *date=move_date(date,-1);
                changed=true;
            }
            let edit=row.add_sized(
                Vec2::new(HISTORY_DATE_FIELD_WIDTH,INNER_HEIGHT),
                egui::TextEdit::singleline(date)
                    .font(egui::TextStyle::Body)
                    .frame(false)
                    .horizontal_align(Align::Center)
                    .vertical_align(Align::Center)
                    // Match the *visible* digit center, not only the
                    // TextEdit's glyph line box; keep caret and hint aligned.
                    .margin(typography::management_date_text_margin())
                    .hint_text(if all_dates{"전체"}else{"YYYY-MM-DD"})
                    .desired_width(HISTORY_DATE_FIELD_WIDTH)
            ).on_hover_text(&format!("{label} 직접 입력"));
            crate::floating_notice::sized_text_caret(
                row,"history.date",&edit,date,
                egui::FontId::proportional(typography::FONT_BODY),
                true,2.0
            );
            changed|=edit.changed();
            let next=history_date_segment(row,"›",NEXT)
                .on_hover_text(&format!("{label} 하루 뒤"));
            debug_assert!((prev.rect.width()-next.rect.width()).abs()<0.01);
            if next.clicked(){
                *date=move_date(date,1);
                changed=true;
            }
            let trigger=history_date_segment(row,"📅",CALENDAR)
                .on_hover_text(&format!("{label}을 달력에서 선택"));
            if trigger.clicked(){
                let day=NaiveDate::parse_from_str(date,"%Y-%m-%d")
                    .unwrap_or_else(|_|NaiveDate::parse_from_str(
                        &seoul_today(),"%Y-%m-%d").expect("valid today"));
                *month=month_start(day);
            }
            if let Some(day)=components::date_calendar(
                &trigger,month,date,id
            ){
                *date=day.format("%Y-%m-%d").to_string();
                changed=true;
            }
            for x in [prev.rect.right(),edit.rect.right(),next.rect.right()]{
                row.painter().line_segment(
                    [egui::pos2(x,inner.top()+4.0),
                     egui::pos2(x,inner.bottom()-4.0)],
                    Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
                );
            }
        }
    );
    ui.painter().rect_stroke(
        outer.shrink(0.5),theme::CONTROL_RADIUS as f32,
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER),
        egui::StrokeKind::Inside
    );
    changed
}
/// Date-basis selection uses the same split-chevron trigger as the
/// Shipping Group Assignment selectors. The values and menu remain unchanged.
fn history_basis_dropdown(ui:&mut Ui,basis:&mut HistoryDateBasis)->bool{
    let previous=*basis;
    components::management_dropdown(
        ui,basis.label(),HISTORY_DATE_BASIS_WIDTH,true,
        "조회 날짜 기준 선택",|options|{
            components::management_dropdown_option(
                options,basis,HistoryDateBasis::Reception,"접수일 기준"
            );
            components::management_dropdown_option(
                options,basis,HistoryDateBasis::Payment,"입금일 기준"
            );
        }
    );
    *basis!=previous
}
/// Only the OUTER toolbar is anchored to the right. Its child controls
/// always render left-to-right; do not nest an RTL layout into date groups.
fn history_date_range_start(
    ui:&mut Ui,basis:&mut HistoryDateBasis,
    from:&mut String,to:&str,from_month:&mut NaiveDate
)->bool{
    let mut changed=history_basis_dropdown(ui,basis);
    let all_dates=from.is_empty()&&to.is_empty();
    changed|=range_date_picker_rtl(
        ui,"시작일",all_dates,from,from_month,"order-history-date-from-calendar"
    );
    changed
}
fn history_date_range_end(
    ui:&mut Ui,from:&mut String,to:&mut String,
    to_month:&mut NaiveDate
)->bool{
    // Fixed 36pt separator slot: the glyph is centered on both axes,
    // with the same external 8pt gaps as every other date control.
    let (separator,_)=ui.allocate_exact_size(
        Vec2::new(theme::MANAGEMENT_DATE_RANGE_SEPARATOR_WIDTH,
            theme::INPUT_HEIGHT),
        Sense::hover()
    );
    ui.painter().text(
        separator.center(),egui::Align2::CENTER_CENTER,"~",
        egui::FontId::proportional(typography::FONT_METADATA),
        theme::MUTED
    );
    let all_dates=from.is_empty()&&to.is_empty();
    let mut changed=range_date_picker_rtl(
        ui,"종료일",all_dates,to,to_month,"order-history-date-to-calendar"
    );
    if theme::action(ui,"기간 전체",theme::ButtonRole::Standard,
        !from.is_empty()||!to.is_empty()).clicked(){
        from.clear();to.clear();changed=true;
    }
    changed
}
/// Excel export history reuses the approved Order History calendar,
/// date field, chevrons, date separator and all-dates action. Unlike order
/// history it has only one date basis: the Excel file's creation date.
pub(crate) const EXCEL_EXPORT_DATE_CONTROLS_WIDTH:f32=536.0;

pub(crate) fn excel_export_date_controls(
    ui:&mut Ui,from:&mut String,to:&mut String,
    from_month:&mut NaiveDate,to_month:&mut NaiveDate,compact:bool
)->bool{
    let mut changed=false;
    ui.spacing_mut().item_spacing.x=theme::MANAGEMENT_DATE_CONTROL_GAP;
    let inner_height=theme::INPUT_HEIGHT;
    if compact {
        // Below 536pt use two right-aligned rows with a common 8pt gap.
        // No controls are clipped, even when the search itself wraps.
        let width=ui.available_width().max(1.0);
        let (outer,_)=ui.allocate_exact_size(
            Vec2::new(width,inner_height*2.0+theme::SPACE_8),Sense::hover()
        );
        let first=Rect::from_min_size(
            egui::pos2(outer.right()-width,outer.top()),
            Vec2::new(width,inner_height)
        );
        let second=Rect::from_min_size(
            egui::pos2(outer.right()-width,
                outer.top()+inner_height+theme::SPACE_8),
            Vec2::new(width,inner_height)
        );
        ui.scope_builder(UiBuilder::new().id_salt("excel-export-from-line")
            .max_rect(first)
            .layout(egui::Layout::left_to_right(Align::Center)
                .with_main_align(Align::Max)),|line|{
                line.set_width(first.width());
                line.spacing_mut().item_spacing.x=theme::MANAGEMENT_DATE_CONTROL_GAP;
                let all_dates=from.is_empty()&&to.is_empty();
                changed|=range_date_picker_rtl(line,"시작일",all_dates,from,from_month,
                    "excel-export-from-calendar");
            });
        ui.scope_builder(UiBuilder::new().id_salt("excel-export-to-line")
            .max_rect(second)
            .layout(egui::Layout::left_to_right(Align::Center)
                .with_main_align(Align::Max)),|line|{
                line.set_width(second.width());
                line.spacing_mut().item_spacing.x=theme::MANAGEMENT_DATE_CONTROL_GAP;
                changed|=excel_export_date_end(line,from,to,to_month);
            });
    }else{
        let all_dates=from.is_empty()&&to.is_empty();
        changed|=range_date_picker_rtl(
            ui,"시작일",all_dates,from,from_month,"excel-export-from-calendar");
        changed|=excel_export_date_end(ui,from,to,to_month);
    }
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

fn excel_export_date_end(
    ui:&mut Ui,from:&mut String,to:&mut String,to_month:&mut NaiveDate
)->bool{
    let (separator,_)=ui.allocate_exact_size(
        Vec2::new(theme::MANAGEMENT_DATE_RANGE_SEPARATOR_WIDTH,
            theme::INPUT_HEIGHT),Sense::hover()
    );
    ui.painter().text(
        separator.center(),Align2::CENTER_CENTER,"~",
        egui::FontId::proportional(typography::FONT_METADATA),theme::MUTED
    );
    let all_dates=from.is_empty()&&to.is_empty();
    let mut changed=range_date_picker_rtl(
        ui,"종료일",all_dates,to,to_month,"excel-export-to-calendar"
    );
    if theme::action(ui,"기간 전체",theme::ButtonRole::Standard,
        !from.is_empty()||!to.is_empty()).clicked(){
        from.clear();to.clear();changed=true;
    }
    changed
}

/// The date toolbar is drawn in a fixed LTR slot, positioned at the
/// right-hand data-column edge by the page. Compact mode reserves
/// two physical lines; it does not reverse the children in either line.
fn history_date_controls(
    ui:&mut Ui,basis:&mut HistoryDateBasis,
    from:&mut String,to:&mut String,
    from_month:&mut NaiveDate,to_month:&mut NaiveDate,
    compact:bool
)->bool{
    let mut changed=false;
    ui.spacing_mut().item_spacing.x=theme::MANAGEMENT_DATE_CONTROL_GAP;
    if compact{
        let width=ui.available_width().max(1.0);
        let (area,_)=ui.allocate_exact_size(
            Vec2::new(width,theme::INPUT_HEIGHT*2.0+theme::SPACE_8),
            Sense::hover()
        );
        let start_width=RANGE_COMPACT_START_WIDTH.min(width);
        let end_width=RANGE_COMPACT_END_WIDTH.min(width);
        let first=Rect::from_min_size(
            egui::pos2(area.right()-start_width,area.top()),
            Vec2::new(start_width,theme::INPUT_HEIGHT)
        );
        let second=Rect::from_min_size(
            egui::pos2(area.right()-end_width,
                area.top()+theme::INPUT_HEIGHT+theme::SPACE_8),
            Vec2::new(end_width,theme::INPUT_HEIGHT)
        );
        ui.scope_builder(UiBuilder::new()
            .id_salt("history-range-start-line")
            .max_rect(first)
            // Anchor the visible controls to the right, while preserving
            // their left-to-right order inside this compact row.
            .layout(egui::Layout::left_to_right(Align::Center)
                .with_main_align(Align::Max)),
            |line|{
                line.set_width(first.width());
                line.set_height(first.height());
                line.spacing_mut().item_spacing.x=theme::MANAGEMENT_DATE_CONTROL_GAP;
                changed|=history_date_range_start(
                    line,basis,from,to,from_month
                );
            });
        ui.scope_builder(UiBuilder::new()
            .id_salt("history-range-end-line")
            .max_rect(second)
            .layout(egui::Layout::left_to_right(Align::Center)
                .with_main_align(Align::Max)),
            |line|{
                line.set_width(second.width());
                line.set_height(second.height());
                line.spacing_mut().item_spacing.x=theme::MANAGEMENT_DATE_CONTROL_GAP;
                changed|=history_date_range_end(
                    line,from,to,to_month
                );
            });
    }else{
        changed|=history_date_range_start(ui,basis,from,to,from_month);
        changed|=history_date_range_end(ui,from,to,to_month);
    }
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
    detail_order_id:Option<String>,
    memo_editor:Option<MemoEditor>,
    memo_save_job:Option<MemoSaveJob>,
    memo_error:Option<String>,
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
            detail_order_id:None,
            memo_editor:None,memo_save_job:None,memo_error:None,
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
        // Navigating from Excel must not leave another order expanded.
        self.expanded.clear();
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
    fn poll_memo_save(&mut self,ctx:egui::Context){
        let result=self.memo_save_job.as_ref().map(|job|job.receiver.try_recv());
        let outcome=match result {
            Some(Ok(answer))=>Some(answer),
            Some(Err(TryRecvError::Disconnected))=>
                Some(Err("관리 메모 저장 연결이 중단되었습니다.".into())),
            _=>None
        };
        if let Some(answer)=outcome {
            if let Some(job)=self.memo_save_job.take(){
                if self.memo_editor.as_ref().is_some_and(|editor|
                    editor.shipment_id==job.shipment_id){
                    match answer {
                        Ok(_)=>{
                            self.memo_editor=None;
                            self.memo_error=None;
                            self.refresh(ctx);
                        },
                        Err(error)=>self.memo_error=Some(error),
                    }
                }
            }
        }
    }

    /// Order-level overview is an independent wide, read-only dashboard.
    /// This view does not duplicate workflow mutations or edit history.
    pub fn show_order_detail_modal(&mut self,ctx:&egui::Context,work_area:Rect){
        let Some(id)=self.detail_order_id.as_deref() else{return};
        let Some(snapshot)=self.snapshot.as_ref() else{
            self.detail_order_id=None;
            return;
        };
        let Some(row)=snapshot.rows.iter().find(|row|
            field(&row["order"],"id")==id) else{
            self.detail_order_id=None;
            return;
        };
        let close=crate::order_history_detail::show(
            ctx,work_area,row,&snapshot.state
        );
        if close{self.detail_order_id=None;}
    }

    pub fn show_memo_modal(&mut self,ctx:&egui::Context,work_area:Rect){
        if self.memo_editor.is_none(){return;}
        components::modal_backdrop(ctx,"history-memo-modal-backdrop",false);
        let busy=self.memo_save_job.is_some();
        let writable=self.gateway.shipment_memo_write_enabled();
        let mut cancel=false;
        let mut save=false;
        components::modal_window("관리 메모","history-shipment-memo",work_area)
            .default_width(520.0_f32.min(work_area.width()))
            .show(ctx,|ui|{
                cancel=components::modal_header(ui,"관리 메모","취소",!busy);
                if let Some(editor)=self.memo_editor.as_mut(){
                    ui.label(typography::caption(&format!(
                        "마지막 갱신: {}",
                        memo_last_updated_label(&editor.updated_at)
                    )));
                    ui.add_space(theme::SPACE_8);
                    // Use the full modal content width. The former nested
                    // enabled UI could shrink the TextEdit to a narrow slot.
                    // Twelve visible lines make this a real memo editor.
                    let editor_width=ui.available_width().max(1.0);
                    ui.add_enabled(!busy,
                        egui::TextEdit::multiline(&mut editor.draft)
                            .desired_width(editor_width)
                            .desired_rows(12)
                            .hint_text("관리 메모를 입력하세요")
                    );
                    ui.label(typography::caption(&format!(
                        "{} / 2,000자",editor.draft.chars().count()
                    )));
                    if !writable{
                        ui.label(typography::caption(
                            "현재 연결은 운영 메모 저장 권한이 없습니다."
                        ));
                    }
                    if let Some(message)=self.memo_error.as_deref(){
                        ui.colored_label(theme::DANGER,message);
                    }
                    ui.add_space(theme::SPACE_12);
                    ui.with_layout(egui::Layout::right_to_left(Align::Center),|bar|{
                        if theme::action(bar,"저장",theme::ButtonRole::Primary,
                            !busy && writable &&
                            editor.draft!=editor.original &&
                            editor.draft.chars().count()<=2_000 &&
                            editor.expected_revision.is_some()).clicked(){
                            save=true;
                        }
                    });
                }
            });
        if cancel && !busy{
            self.memo_editor=None;
            self.memo_error=None;
        }
        if save && !busy {
            if let Some(editor)=self.memo_editor.as_ref(){
                let (sender,receiver)=mpsc::channel();
                let gateway=Arc::clone(&self.gateway);
                let Some(revision)=editor.expected_revision else{return;};
                let shipment_id=editor.shipment_id.clone();
                let memo=editor.draft.clone();
                let wake=ctx.clone();
                std::thread::spawn(move||{
                    let result=gateway.save_shipment_management_memo(
                        revision,&shipment_id,&memo
                    );
                    let _=sender.send(result);
                    wake.request_repaint();
                });
                self.memo_save_job=Some(MemoSaveJob{
                    shipment_id:editor.shipment_id.clone(),receiver
                });
                self.memo_error=None;
            }
        }
    }

    pub fn show_page(&mut self,ui:&mut Ui){
        let page_origin_y=ui.cursor().top();
        ui.spacing_mut().scroll=theme::management_scroll_style();
        self.poll_memo_save(ui.ctx().clone());
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
        // Same full-height single-table card as Freight Assignment:
        // preserve 16pt clearance on all four sides and keep both the
        // scrolling rows and the native rail within the inner card.
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        theme::management_master_table_frame().show(ui,|section|{
            components::fill_management_primary_card(section,card_min_height);
            section.spacing_mut().scroll=theme::management_table_scroll_style();
            // Use the identical zero-implicit-gap toolbar-to-header
            // allocator as Freight Assignment. Do not inject extra space
            // or move the header independently on either screen.
            let old_gap=section.spacing().item_spacing.y;
            section.spacing_mut().item_spacing.y=0.0;
            let full_width=section.available_width().max(1.0);
            // The search toolbar itself has NO scrollbar. Align its last
            // "전체 날짜" control to the card's 16pt inner right edge.
            // Only the table header/rows below reserve the 10pt rail.
            // The toolbar owns the complete card-inner width, including
            // the 10pt strip reserved only by the table below it.
            let toolbar_content_width=full_width;
            let one_row=toolbar_content_width>=
                theme::MANAGEMENT_SEARCH_MIN_WIDTH
                +theme::SPACE_16+RANGE_CONTROLS_WIDTH;
            let compact=toolbar_content_width<RANGE_CONTROLS_WIDTH;
            let date_height=if compact{
                theme::INPUT_HEIGHT*2.0+theme::SPACE_8
            }else{theme::INPUT_HEIGHT};
            let toolbar_height=if one_row{theme::INPUT_HEIGHT}
                else{theme::INPUT_HEIGHT+theme::SPACE_8+date_height};
            let (toolbar_rect,_)=section.allocate_exact_size(
                Vec2::new(full_width,toolbar_height),Sense::hover()
            );
            let search_rect=Rect::from_min_size(
                toolbar_rect.min,
                Vec2::new(theme::MANAGEMENT_SEARCH_MIN_WIDTH
                    .min(toolbar_content_width),theme::INPUT_HEIGHT)
            );
            section.scope_builder(UiBuilder::new()
                .id_salt("history-range-search")
                .max_rect(search_rect)
                .layout(egui::Layout::left_to_right(Align::Center)),
                |search|{
                    search.set_width(search_rect.width());
                    search.set_height(search_rect.height());
                    if components::flexible_text_field(
                        search,&mut self.query,search_rect.width(),
                        "거래처 / 주문 내용 검색"
                    ).changed(){changed=true;}
                });
            let date_rect=components::management_toolbar_trailing_rect(
                toolbar_rect,RANGE_CONTROLS_WIDTH,
                if one_row{0.0}else{theme::INPUT_HEIGHT+theme::SPACE_8},
                if one_row{theme::INPUT_HEIGHT}else{date_height}
            );
            section.scope_builder(UiBuilder::new()
                .id_salt("history-range-controls")
                .max_rect(date_rect)
                // Anchor the actual trailing "전체 날짜" button to the
                // card's right content boundary (16pt inset). The table's
                // independent scrollbar gutter is NOT subtracted here.
                .layout(egui::Layout::left_to_right(Align::Center)
                    .with_main_align(Align::Max)),
                |dates|{
                    dates.set_width(date_rect.width());
                    dates.set_height(date_rect.height());
                    changed|=history_date_controls(
                        dates,&mut self.date_basis,
                        &mut self.from_input,&mut self.to_input,
                        &mut self.from_month,&mut self.to_month,compact
                    );
                });
            if changed && date_range_error(
                &self.from_input,&self.to_input
            ).is_none(){
                self.date_from=self.from_input.clone();
                self.date_to=self.to_input.clone();
                self.refresh(section.ctx().clone());
            }
            // Same measured transition as Freight > Shipping Group
            // Assignment: symmetric toolbar breathing room, divider and header come from
            // ONE shared allocator instead of a separate add_space call.
            // Only empty/loading state is painted inside the table body;
            // validation and request failures use the shared floating overlay.
            crate::floating_notice::section(
                section.ctx(),"history.query",toolbar_rect
            );
            let table_header=components::management_sticky_table_header(
                section,MIN_WIDTH
            );
            let viewport=table_header.viewport_width;
            let gutter=table_header.scrollbar_gutter;
            let table_width=table_header.table_width;
            let head_rect=table_header.header_rect;
            let header_content_rect=table_header.header_content_rect;
            // Fixed columns retain their approved widths; the central
            // master spec directs all spare space to the shipment/product
            // column and leaves expandable detail rows variable-height.
            let widths=theme::ORDER_HISTORY_MASTER_TABLE.widths_for(table_width);
            let headings=["접수일","거래처","상품 목록","관리 상태",
                "청구 합계","입금 확인","처리"];
            let mut header_origin=None;
            let mut pending_toggle=None;
            let mut pending_memo=None;
            let mut pending_detail=None;
            let snapshot=self.snapshot.as_ref();
            let lines=snapshot.map(|data|display_lines(data,&self.expanded,self.sort))
                .unwrap_or_default();
            let validation=date_range_error(
                &self.from_input,&self.to_input
            );
            crate::floating_notice::sync(
                section.ctx(),"history.date.validation","history.date",
                crate::floating_notice::Tone::Validation,validation
            );
            crate::floating_notice::sync(
                section.ctx(),"history.query.error","history.query",
                crate::floating_notice::Tone::Error,self.error.as_deref()
            );
            // Empty, loading and no-result states remain in the table body.
            // Input/server errors are floating notices anchored to the toolbar.
            let notice=if self.receiver.is_some(){
                Some(("주문 이력을 확인 중입니다.",theme::MUTED))
            }else if snapshot.is_some_and(|data|data.rows.is_empty()){
                Some((if self.query.is_empty()
                    && self.date_from.is_empty()&&self.date_to.is_empty()
                    && self.date_basis==HistoryDateBasis::Reception{
                    "저장된 주문이 없습니다."
                }else{"조건에 맞는 주문이 없습니다."},theme::MUTED))
            }else{None};
            // Keep the body 16pt above the card's lower border. Only the
            // rail extends 4pt into the inset, leaving 12pt below it;
            // the scroll viewport and the displayed data rows are unchanged.
            let scroll_max_height=components::management_single_list_scroll_height(
                section,card_min_height,0.0
            );
            let scroll_bounds=section.available_rect_before_wrap();
            let scroll_track=components::management_table_scroll_track_rect(
                section,scroll_max_height
            );
            // Mixed fixed parent rows and auto-height shipment details need
            // prefix offsets. A fixed-height virtual grid would clip wrapped
            // products and could paint the next shipment on top.
            let detail_spacings=(0..lines.len()).map(|i|
                history_detail_spacing(&lines,i)
            ).collect::<Vec<_>>();
            let line_heights=if let Some(data)=snapshot{
                lines.iter().zip(detail_spacings.iter()).map(|(line,spacing)|
                    history_line_height(section,*line,data,&widths,*spacing)
                ).collect::<Vec<_>>()
            }else{Vec::new()};
            let mut offsets=Vec::with_capacity(line_heights.len()+1);
            offsets.push(0.0_f32);
            for height in &line_heights{
                offsets.push(offsets.last().copied().unwrap_or(0.0)+height);
            }
            let total_height=*offsets.last().unwrap_or(&0.0);
            // Freight reference: header, rows and the right-hand 10pt rail
            // use one measured data width. An unconditional two-axis
            // ScrollArea exposed a bottom scrollbar even at desktop widths.
            // Only allow horizontal scrolling if the viewport is actually
            // narrower than the seven-column minimum.
            let needs_horizontal=(viewport-gutter)+0.5<MIN_WIDTH;
            let table_scroll=if needs_horizontal{
                egui::ScrollArea::both()
            }else{
                egui::ScrollArea::vertical()
            };
            let output=table_scroll
                .scroll_bar_rect(scroll_track)
                .id_salt("order-history-table-rows")
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .max_height(scroll_max_height)
                .auto_shrink([false,false])
                .show_viewport(section,|body,viewport|{
                    body.spacing_mut().item_spacing.y=0.0;
                    body.set_min_width(table_width);
                    body.set_min_height(total_height);
                    let first=offsets.partition_point(|value|*value<=viewport.top())
                        .saturating_sub(1).min(lines.len());
                    let last=offsets.partition_point(|value|*value<=viewport.bottom())
                        .saturating_add(1).min(lines.len());
                    let origin=body.max_rect().min;
                    let visible_rect=Rect::from_min_size(
                        egui::pos2(origin.x,origin.y+offsets[first]),
                        Vec2::new(table_width,(offsets[last]-offsets[first]).max(0.0))
                    );
                    let clip=body.clip_rect();
                    body.scope_builder(UiBuilder::new()
                        .id_salt("history-variable-rows")
                        .max_rect(visible_rect),|visible|{
                            visible.spacing_mut().item_spacing.y=0.0;
                            visible.set_clip_rect(clip);
                            visible.skip_ahead_auto_ids(first);
                            for line_index in first..last {
                                let line=lines[line_index];
                                let spacing=detail_spacings[line_index];
                                let row_rect=Rect::from_min_size(
                                    egui::pos2(origin.x,origin.y+offsets[line_index]),
                                    Vec2::new(table_width,line_heights[line_index])
                                );
                                header_origin.get_or_insert(row_rect.left());
                                visible.scope_builder(UiBuilder::new()
                                    .id_salt(("history-variable-row",line_index))
                                    .max_rect(row_rect)
                                    .layout(egui::Layout::top_down(Align::Min)),|row_ui|{
                                        row_ui.allocate_exact_size(
                                            Vec2::new(table_width,row_rect.height()),Sense::hover()
                                        );
                                        match line {
                                            HistoryLine::Order(_)=>{
                                                let background=if line_index%2==0{theme::PANEL}
                                                    else{theme::TABLE_ROW_ALT_BG};
                                                row_ui.painter().rect_filled(
                                                    row_rect,0.0,background);
                                                row_ui.painter().line_segment(
                                                    [row_rect.left_bottom(),row_rect.right_bottom()],
                                                    Stroke::new(theme::FRAME_STROKE_WIDTH,theme::DIVIDER));
                                            },
                                            HistoryLine::Recipient(_,_,_)=>{
                                                row_ui.painter().rect_filled(
                                                    row_rect,0.0,theme::CANVAS);
                                                let block_rect=history_detail_block_rect(
                                                    row_rect,&widths,spacing);
                                                row_ui.painter().rect(
                                                    block_rect,
                                                    theme::WORKFLOW_CHIP_RADIUS as f32,
                                                    theme::PANEL,
                                                    Stroke::new(1.0,theme::DIVIDER),
                                                    egui::StrokeKind::Inside
                                                );
                                            }
                                        }
                                        if let Some(snapshot)=snapshot {
                                            draw_line(row_ui,row_rect,line,snapshot,
                                                &self.expanded,&widths,spacing,
                                                &mut pending_toggle,&mut pending_memo,
                                                &mut pending_detail);
                                        }
                                        if matches!(line,HistoryLine::Order(_)) {
                                            paint_column_dividers(
                                                row_ui,row_rect,&widths,false,row_rect
                                            );
                                        }
                                        if snapshot.is_some() {
                                            if let Some(branch)=history_branch(&lines,line_index){
                                                paint_history_branch(
                                                    row_ui,row_rect,&widths,branch,spacing
                                                );
                                            }
                                        }
                                    });
                            }
                        });
                });
            if let Some((message,color))=notice{
                let notice_rect=Rect::from_min_size(
                    scroll_bounds.min,
                    Vec2::new(
                        (viewport-gutter).max(1.0).min(scroll_bounds.width()),
                        scroll_max_height.min(scroll_bounds.height())
                            .max(ROW_HEIGHT)
                    )
                );
                let galley=typography::management_control_text_galley(
                    section,message,color
                );
                let origin=notice_rect.center()-galley.size()/2.0;
                section.painter()
                    .with_clip_rect(notice_rect.intersect(section.clip_rect()))
                    .galley(origin,galley,color);
            }
            let left=header_origin.unwrap_or(head_rect.left()-output.state.offset.x);
            let scrolled_header=history_scrolled_header_row(
                head_rect,left,table_width
            );
            let header_cells=cells(scrolled_header.min,&widths,HEADER_HEIGHT);
            let painter=section.painter().with_clip_rect(head_rect);
            painter.rect_filled(head_rect,0.0,theme::RAISED);
            // Header labels AND column separators must use the same
            // horizontally scrolled origin as the data rows. The viewport
            // background, bottom outline and scrollbar rail remain fixed.
            paint_column_dividers(
                section,scrolled_header,&widths,false,header_content_rect
            );
            // Freight centers headings in the 34pt area above its
            // INSIDE-the-header 2pt outline, excluding the right rail.
            let header_outline_height=2.0_f32;
            let header_text_rect=Rect::from_min_max(
                header_content_rect.min,
                egui::pos2(header_content_rect.right(),
                    header_content_rect.bottom()-header_outline_height)
            );
            let header_text_center_y=header_text_rect.center().y;
            let mut next_sort=None;
            for (index,title) in headings.iter().enumerate(){
                let sort_col=match index{
                    0=>Some(SortColumn::Date),1=>Some(SortColumn::Client),
                    2=>Some(SortColumn::Shipment),3=>Some(SortColumn::Status),
                    4=>Some(SortColumn::Total),
                    5=>Some(SortColumn::PaymentConfirmation),_=>None,
                };
                let cell=header_cells[index].intersect(header_text_rect);
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
                painter.with_clip_rect(cell).galley(
                    egui::pos2(x,header_text_center_y-label.size().y/2.0),
                    label,theme::SECONDARY
                );
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
                toggle_history_expansion(&mut self.expanded,&order_id);
            }
            if let Some(editor)=pending_memo {
                self.memo_editor=Some(editor);
                self.memo_error=None;
            }
            if let Some(order_id)=pending_detail {
                self.detail_order_id=Some(order_id);
            }
        });
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn expanded_order_groups_two_recipients_and_preserves_full_delivery_details(){
        let snapshot=HistorySnapshot{
            state:json!({"workflow":{"shipments":[
                {"id":"s1","orderId":"o1","recipientId":"r1",
                 "tracking":{"parcel":"123456789"},
                 "actualFreight":{"parcel":3500}}
            ]}}),
            rows:vec![json!({
                "order":{"id":"o1"},
                "recipients":[
                    {"id":"r1","name":"수취인 가","address":"광주",
                     "items":[{"id":"i1","name":"BD 709 / 블랙","quantity":4},
                              {"id":"i2","name":"뮤직 트레이","quantity":1}]},
                    {"id":"r2","name":"수취인 나","address":"담양",
                     "items":[{"id":"i3","name":"BD 803","quantity":2}]}
                ],
                "billing":{"subtotal":268000,"shipping":9000,"vat":0},
                "total":277000
            })]
        };
        let mut expanded=BTreeSet::new();
        assert_eq!(display_lines(&snapshot,&expanded,None),vec![
            HistoryLine::Order(0)
        ]);
        expanded.insert("o1".to_owned());
        // The parent owns billed total and employee approval. Detail
        // rows use one block per actual shipment (or an unshipped recipient).
        let expanded_lines=display_lines(&snapshot,&expanded,None);
        assert_eq!(expanded_lines,vec![
            HistoryLine::Order(0),
            HistoryLine::Recipient(0,0,Some(0)),
            HistoryLine::Recipient(0,1,None)
        ]);
        assert_eq!(history_branch(&expanded_lines,0),Some(HistoryBranch::Root));
        assert_eq!(history_branch(&expanded_lines,1),Some(HistoryBranch::Continue));
        assert_eq!(history_branch(&expanded_lines,2),Some(HistoryBranch::Last));
        assert_eq!(history_recipient_products(&snapshot.rows[0],0),
            "◆ BD 709 / 블랙 × 4\n◆ 뮤직 트레이 × 1");
        assert_eq!(history_shipment_logistics(
            rows(&snapshot.state["workflow"]["shipments"]).first()
        ),("3,500원".to_owned(),"123456789".to_owned()));
        assert_eq!(history_shipment_logistics(None),
            ("운임 미확인".to_owned(),"운송장 미확인".to_owned()));
        // Multiple shipments for one recipient now occupy TWO individual
        // one-line blocks; values cannot be merged across shipment IDs.
        let mut multiple=HistorySnapshot{
            state:snapshot.state.clone(),rows:snapshot.rows.clone()
        };
        multiple.state["workflow"]["shipments"].as_array_mut().unwrap().push(
            json!({"id":"s2","orderId":"o1","recipientId":"r1",
                "tracking":{"parcel":"987654321"},
                "actualFreight":{"parcel":2500}})
        );
        let split_lines=display_lines(&multiple,&expanded,None);
        assert_eq!(split_lines,vec![
            HistoryLine::Order(0),
            HistoryLine::Recipient(0,0,Some(0)),
            HistoryLine::Recipient(0,0,Some(1)),
            HistoryLine::Recipient(0,1,None)
        ]);
        assert_eq!(history_branch(&split_lines,2),Some(HistoryBranch::Continue));
        assert_eq!(history_branch(&split_lines,3),Some(HistoryBranch::Last));
        assert_eq!(history_shipment_logistics(
            rows(&multiple.state["workflow"]["shipments"]).get(1)
        ),("2,500원".into(),"987654321".into()));
        multiple.state["workflow"]["shipments"][1]["recipientSnapshot"]=json!({
            "id":"r1","name":"과거 수취인","address":"과거 주소",
            "items":[{"name":"709 검정","quantity":1}]
        });
        let original=multiple.state["workflow"]["shipments"][1]
            ["recipientSnapshot"]["items"].clone();
        assert_eq!(history_items_multiline(&original),"◆ 709 검정 × 1");
        let detailed_items=json!([
            {"name":"BD 708-1 / 블랙 / 비타공 상판","quantity":30},
            {"name":"Conductor / 지휘자 보면대","quantity":1},
            {"name":"Cart-10 / 보면대카트","quantity":2}
        ]);
        assert_eq!(history_items_multiline(&detailed_items),
            "◆ BD 708-1 / 블랙 / 비타공 상판 × 30\n◆ Conductor / 지휘자 보면대 × 1\n◆ Cart-10 / 보면대카트 × 2");
        assert!(!order_items_single_line(&json!({"recipients":[{"items":detailed_items}]}))
            .contains('\n'));
    }
    #[test]
    fn detail_products_and_actions_share_edges_and_vertical_alignment(){
        // All horizontal space except two controls and deliberate spacers
        // goes to the card, regardless of the text width.
        let mut prior_product_width=0.0;
        let spacing=DetailSpacing {
            top:DETAIL_GROUP_GAP,bottom:DETAIL_GROUP_GAP
        };
        for width in [MIN_WIDTH,1600.0,2100.0] {
            let row=Rect::from_min_size(
                egui::pos2(20.0,70.0),
                Vec2::new(width,DETAIL_BLOCK_HEIGHT+spacing.top+spacing.bottom)
            );
            let block=history_detail_block_rect(row,&COLS,spacing);
            let actions=history_detail_actions_rect(row,block);
            let branch_x=row.left()+COLS[0]-theme::SPACE_12;
            assert_eq!(block.left(),branch_x+theme::SPACE_8);
            assert_eq!(block.height(),DETAIL_BLOCK_HEIGHT);
            assert_eq!(block.top()-row.top(),DETAIL_GROUP_GAP);
            assert_eq!(row.bottom()-block.bottom(),DETAIL_GROUP_GAP);
            assert_eq!(actions.height(),theme::ACTION_STANDARD_HEIGHT);
            assert!((block.top()-actions.top()).abs()<0.01);
            assert!((block.right()+DETAIL_ACTIONS_GAP-actions.left()).abs()<0.01);
            assert!((actions.width()-DETAIL_ACTIONS_WIDTH).abs()<0.01);
            assert!((row.right()-actions.right()-DETAIL_RIGHT_INSET).abs()<0.01);
            let cells=history_detail_cells(block);
            assert!(cells[0].left()>block.left());
            assert!(cells[3].right()<block.right());
            assert!(cells[1].width()>prior_product_width);
            prior_product_width=cells[1].width();
            for pair in cells.windows(2) {
                assert!((pair[1].left()-pair[0].right()-theme::SPACE_8).abs()<0.01);
            }
        }
        // A multiline card keeps the same external spacing; actions remain
        // vertically centered even when card and row heights increase.
        let tall=Rect::from_min_size(
            egui::pos2(20.0,70.0),Vec2::new(MIN_WIDTH,102.0)
        );
        let block=history_detail_block_rect(tall,&COLS,spacing);
        let actions=history_detail_actions_rect(tall,block);
        assert_eq!(block.height(),102.0-2.0*DETAIL_GROUP_GAP);
        assert_eq!(actions.height(),theme::ACTION_STANDARD_HEIGHT);
        assert_eq!(block.center().y,actions.center().y);
    }
    #[test]
    fn expanded_group_uses_equal_8pt_top_between_and_bottom_gaps(){
        let lines=vec![
            HistoryLine::Order(0),
            HistoryLine::Recipient(0,0,Some(0)),
            HistoryLine::Recipient(0,1,Some(1)),
            HistoryLine::Order(1),
            HistoryLine::Recipient(1,0,None),
            HistoryLine::Order(2),
        ];
        let first=history_detail_spacing(&lines,1);
        let last=history_detail_spacing(&lines,2);
        let single=history_detail_spacing(&lines,4);
        assert_eq!((first.top,first.bottom),
            (DETAIL_GROUP_GAP,DETAIL_GROUP_GAP/2.0));
        assert_eq!((last.top,last.bottom),
            (DETAIL_GROUP_GAP/2.0,DETAIL_GROUP_GAP));
        assert_eq!((single.top,single.bottom),
            (DETAIL_GROUP_GAP,DETAIL_GROUP_GAP));

        let parent_bottom=42.0;
        let first_row=Rect::from_min_size(
            egui::pos2(0.0,parent_bottom),
            Vec2::new(MIN_WIDTH,DETAIL_BLOCK_HEIGHT+first.top+first.bottom)
        );
        let first_card=history_detail_block_rect(first_row,&COLS,first);
        let last_row=Rect::from_min_size(
            egui::pos2(0.0,first_row.bottom()),
            Vec2::new(MIN_WIDTH,DETAIL_BLOCK_HEIGHT+last.top+last.bottom)
        );
        let last_card=history_detail_block_rect(last_row,&COLS,last);
        assert_eq!(first_card.top()-parent_bottom,DETAIL_GROUP_GAP);
        assert_eq!(last_card.top()-first_card.bottom(),DETAIL_GROUP_GAP);
        assert_eq!(last_row.bottom()-last_card.bottom(),DETAIL_GROUP_GAP);
        // One-card group is padded equally on BOTH outside edges.
        let single_row=Rect::from_min_size(
            egui::pos2(0.0,0.0),
            Vec2::new(MIN_WIDTH,DETAIL_BLOCK_HEIGHT+single.top+single.bottom)
        );
        let single_card=history_detail_block_rect(single_row,&COLS,single);
        assert_eq!(single_card.top()-single_row.top(),DETAIL_GROUP_GAP);
        assert_eq!(single_row.bottom()-single_card.bottom(),DETAIL_GROUP_GAP);
    }
    #[test]
    fn horizontal_scroll_translates_header_text_and_separators_together(){
        // Background/header bottom outline remain at this viewport, but
        // table cells, their captions, sort hit areas, and vertical dividers
        // must all follow the body scroll offset.
        let viewport=Rect::from_min_size(
            egui::pos2(40.0,100.0),Vec2::new(820.0,HEADER_HEIGHT)
        );
        let mut previous:Option<(f32,f32)>=None;
        for offset in [0.0,12.0,106.0,240.0] {
            let row=history_scrolled_header_row(
                viewport,viewport.left()-offset,MIN_WIDTH
            );
            let text_cells=cells(row.min,&COLS,HEADER_HEIGHT);
            let divider_cells=cells(row.min,&COLS,HEADER_HEIGHT);
            assert_eq!(row.top(),viewport.top());
            assert_eq!(row.height(),viewport.height());
            assert_eq!(row.width(),MIN_WIDTH);
            assert!((text_cells[0].left()-(viewport.left()-offset)).abs()<0.01);
            for index in 0..6{
                // paint_column_dividers() gets the SAME scrolled row rect.
                let divider=divider_cells[index].right()+COL_GAP/2.0;
                let heading_boundary=text_cells[index].right()+COL_GAP/2.0;
                assert!((divider-heading_boundary).abs()<0.01);
            }
            if let Some((last_offset,last_edge))=previous {
                let edge=divider_cells[0].right()+COL_GAP/2.0;
                assert!((edge-last_edge+(offset-last_offset)).abs()<0.01);
            }
            previous=Some((offset,divider_cells[0].right()+COL_GAP/2.0));
        }
    }
    #[test]
    fn expanding_another_order_closes_the_previous_one(){
        let mut expanded=BTreeSet::new();
        toggle_history_expansion(&mut expanded,"o1");
        assert_eq!(expanded.iter().map(String::as_str).collect::<Vec<_>>(),vec!["o1"]);
        toggle_history_expansion(&mut expanded,"o2");
        assert_eq!(expanded.iter().map(String::as_str).collect::<Vec<_>>(),vec!["o2"]);
        toggle_history_expansion(&mut expanded,"o2");
        assert!(expanded.is_empty());
        toggle_history_expansion(&mut expanded,"o3");
        assert_eq!(expanded.len(),1);
        assert!(expanded.contains("o3"));
    }
    #[test]
    fn employee_approval_column_sorts_by_confirmation_date_and_keeps_unknown_last(){
        let snapshot=HistorySnapshot{
            state:json!({"workflow":{"shipments":[]}}),
            rows:vec![
                json!({"order":{"id":"a"},
                    "display":{"employeePaymentConfirmation":{"date":"2026-11-21"}}}),
                json!({"order":{"id":"b"},
                    "display":{"employeePaymentConfirmation":null}}),
                json!({"order":{"id":"c"},
                    "display":{"employeePaymentConfirmation":{"date":"2026-11-19"}}}),
                json!({"order":{"id":"d"},
                    "display":{"employeePaymentConfirmation":{"date":"2026-11-20"}}})
            ]
        };
        let expanded=BTreeSet::new();
        let order=|direction|display_lines(
            &snapshot,&expanded,Some(Sort{
                column:SortColumn::PaymentConfirmation,direction
            })
        ).into_iter().filter_map(|line|{
            if let HistoryLine::Order(index)=line{Some(index)}else{None}
        }).collect::<Vec<_>>();
        assert_eq!(order(SortDirection::Asc),vec![2,3,0,1]);
        assert_eq!(order(SortDirection::Desc),vec![0,3,2,1]);

        let first=Sort::next(None,SortColumn::PaymentConfirmation).unwrap();
        assert!(first.direction==SortDirection::Asc);
        let second=Sort::next(Some(first),SortColumn::PaymentConfirmation).unwrap();
        assert!(second.direction==SortDirection::Desc);
        assert!(Sort::next(Some(second),SortColumn::PaymentConfirmation).is_none());
    }

    #[test]
    fn seoul_date_move_and_read_only_projection_labels(){
        assert_eq!(seoul_date("2026-11-19T16:00:00Z"),"2026-11-20");
        assert_eq!(memo_last_updated_label("2026-11-20T16:30:00+00:00"),
            "2026-11-21 01:30");
        assert_eq!(memo_last_updated_label(""),"기록 없음");
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
        assert_eq!(display_client_name("가나다라마바사아자차카타"),"가나다라마바사아자차");
        assert_eq!(display_client_name("짧은상호"),"짧은상호");
        assert_eq!(COLS.iter().sum::<f32>()+COL_GAP*6.0,MIN_WIDTH);
        let sample=json!({"recipients":[
            {"items":[{"name":"BD 709","quantity":2},{"name":"Desk TS","quantity":1}]},
            {"items":[{"name":"GT","quantity":3}]}
        ]});
        assert_eq!(order_items_single_line(&sample),
            "◆ 709 × 2  ◆ Desk TS × 1  ◆ GT × 3");
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
        assert_eq!(display_lines(&snapshot,&expanded,None).len(),2);
    }
}
