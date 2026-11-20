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
const COLS: [f32;6] = [124.0,180.0,422.0,138.0,156.0,132.0];
const MIN_WIDTH: f32 = 1192.0; // sum(COLS) + five 8pt gaps

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
fn item_count(row:&Value)->usize{
    rows(&row["recipients"]).iter().map(|r|rows(&r["items"]).len()).sum()
}
fn tracking_metadata(state:&Value)->Value{
    Value::Array(rows(&state["workflow"]["shipments"]).iter().map(|ship|{
        let keys=ship["tracking"].as_object().map(|values|
            values.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        json!({"shipmentId":field(ship,"id"),"keys":keys})
    }).collect())
}
fn fetch(gateway:&dyn BandoGateway,query:&str,date:&str)
    ->Result<HistorySnapshot,String>{
    let state=gateway.load_business_state()?;
    let value=gateway.execute(ReadOperation::ListOrderHistoryView,json!({
        "state":&state,"query":query,"date":date,
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

fn cells(origin:Pos2,widths:&[f32;6],height:f32)->[Rect;6]{
    let mut left=origin.x;
    std::array::from_fn(|index|{
        let cell=Rect::from_min_size(egui::pos2(left,origin.y),
            Vec2::new(widths[index],height));
        left+=widths[index]+COL_GAP;
        cell
    })
}
fn paint_text(ui:&Ui,rect:Rect,value:&str,emphasized:bool,right:bool){
    if rect.width()<=0.0{return}
    let color=if emphasized{theme::TEXT}else{theme::SECONDARY};
    let galley=typography::freight_table_cell_galley(ui,value,emphasized,color);
    let inset=theme::SPACE_8;
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
        .max(rect.left()+theme::SPACE_8);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(
        egui::pos2(x,rect.center().y-galley.size().y/2.0),galley,theme::SECONDARY
    );
}
fn paint_caption(ui:&Ui,rect:Rect,value:&str){
    let galley=typography::client_table_metadata_galley(ui,value,theme::MUTED);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(
        egui::pos2(rect.left()+theme::SPACE_8,
            rect.center().y-galley.size().y/2.0),galley,theme::MUTED
    );
}
fn draw_line(ui:&mut Ui,rect:Rect,line:HistoryLine,snapshot:&HistorySnapshot,
    expanded:&BTreeSet<String>,widths:&[f32;6],pending_toggle:&mut Option<String>){
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
            paint_text(ui,columns[1],order_client(row),true,false);
            ui.interact(columns[1].intersect(area),
                ui.make_persistent_id(("history-client-name",id)),Sense::hover())
                .on_hover_text(order_client(row));
            let recipients=rows(&row["recipients"]);
            let first=recipients.first().map(|r|field(r,"name"))
                .unwrap_or("수취인 미확인");
            let toggled=expanded.contains(id);
            let lead=if toggled{"▾"}else{"▸"};
            let main=format!("{lead} {first} · 배송 {}건 · {}품목",
                recipients.len(),item_count(row));
            let product_preview=recipients.iter()
                .flat_map(|r|rows(&r["items"]).iter())
                .take(2)
                .map(|item|format!("{} × {}",
                    model_label(field(item,"name")),
                    item["quantity"].as_i64().map(|n|n.to_string())
                        .unwrap_or_else(||"수량 확인".into())))
                .collect::<Vec<_>>().join(" / ");
            let remaining=item_count(row).saturating_sub(2);
            let preview=if remaining>0{format!("{product_preview} 외 {remaining}품목")}
                else{product_preview};
            let upper=Rect::from_min_max(columns[2].min,
                egui::pos2(columns[2].right(),columns[2].center().y));
            let lower=Rect::from_min_max(egui::pos2(columns[2].left(),
                columns[2].center().y),columns[2].max);
            paint_text(ui,upper,&main,true,false);
            paint_caption(ui,lower,&preview);
            let response=ui.interact(columns[2].intersect(area),
                ui.make_persistent_id(("order-history-toggle",id)),Sense::click())
                .on_hover_text(recipients.iter().map(|r|field(r,"name"))
                    .collect::<Vec<_>>().join(", "));
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
            let message=field(&row["display"],"trackingMessage");
            let control=columns[5].shrink2(Vec2::new(theme::SPACE_4,theme::SPACE_4));
            ui.scope_builder(UiBuilder::new()
                .id_salt(("order-history-copy",id))
                .max_rect(control)
                .layout(egui::Layout::left_to_right(Align::Center)),|actions|{
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

/// One right-aligned date-control group. Its visual order is fixed:
/// reception day -> previous -> input -> next -> calendar -> all dates.
/// All date/search semantics remain on the existing Rust history query path.
fn history_date_controls(
    ui:&mut Ui,date:&mut String,date_input:&mut String,
    calendar_month:&mut NaiveDate
)->bool {
    let mut changed=false;
    ui.spacing_mut().item_spacing.x=theme::SPACE_8;
    ui.label(typography::field_label("접수일"));
    if theme::action(ui,"‹",theme::ButtonRole::Standard,true).clicked(){
        *date=move_date(date,-1);
        *date_input=date.clone();
        if let Ok(day)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){
            *calendar_month=month_start(day);
        }
        changed=true;
    }
    if components::flexible_text_field(
        ui,date_input,130.0,"YYYY-MM-DD"
    ).changed(){
        if date_input.is_empty() ||
            NaiveDate::parse_from_str(date_input,"%Y-%m-%d").is_ok()
        {
            *date=date_input.clone();
            if let Ok(day)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){
                *calendar_month=month_start(day);
            }
            changed=true;
        }
    }
    if theme::action(ui,"›",theme::ButtonRole::Standard,true).clicked(){
        *date=move_date(date,1);
        *date_input=date.clone();
        if let Ok(day)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){
            *calendar_month=month_start(day);
        }
        changed=true;
    }
    let trigger=theme::action(
        ui,"달력",theme::ButtonRole::Standard,true
    ).on_hover_text("접수일을 달력에서 선택");
    if trigger.clicked(){
        let open=NaiveDate::parse_from_str(date,"%Y-%m-%d")
            .unwrap_or_else(|_|NaiveDate::parse_from_str(
                &seoul_today(),"%Y-%m-%d").expect("valid today"));
        *calendar_month=month_start(open);
    }
    if let Some(day)=date_calendar(
        &trigger,calendar_month,date,"order-history-reception-date-calendar"
    ){
        *date=day.format("%Y-%m-%d").to_string();
        *date_input=date.clone();
        changed=true;
    }
    if theme::action(ui,"전체 날짜",
        theme::ButtonRole::Standard,!date.is_empty()).clicked(){
        date.clear();
        date_input.clear();
        changed=true;
    }
    changed
}

pub struct OrderHistoryManager {
    gateway:Arc<dyn BandoGateway>,
    receiver:Option<Receiver<Result<HistorySnapshot,String>>>,
    snapshot:Option<HistorySnapshot>,
    error:Option<String>,
    query:String,
    date:String,
    date_input:String,
    calendar_month:NaiveDate,
    expanded:BTreeSet<String>,
    sort:Option<Sort>,
}
impl OrderHistoryManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let today=seoul_today();
        let mut manager=Self {
            gateway,receiver:None,snapshot:None,error:None,
            query:String::new(),date:today.clone(),date_input:today,
            calendar_month:month_start(
                NaiveDate::parse_from_str(&seoul_today(),"%Y-%m-%d")
                    .expect("today in YYYY-MM-DD")
            ),
            expanded:BTreeSet::new(),sort:None,
        };
        manager.refresh(ctx);
        manager
    }
    /// Navigate from an exported Excel row to the exact original order.
    /// The caller passes the saved order's confirmed/created day, not a
    /// guessed current-day filter. This is a presentation-only selection.
    pub fn open_order(&mut self,id:&str,date:&str,ctx:egui::Context){
        let parsed=seoul_date(date);
        if let Ok(day)=NaiveDate::parse_from_str(&parsed,"%Y-%m-%d"){
            self.date=parsed.clone();
            self.date_input=parsed;
            self.calendar_month=month_start(day);
        }else{
            self.date.clear();
            self.date_input.clear();
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
        let date=self.date.clone();
        std::thread::spawn(move||{
            let answer=fetch(gateway.as_ref(),&query,&date);
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
        let card_min_height=components::management_primary_card_min_inner_height(ui,theme::FRAME_STROKE_WIDTH);
        theme::management_list_frame().show(ui,|section|{
            components::fill_management_primary_card(section,card_min_height);
            theme::management_list_strip_frame().show(section,|toolbar|{
                // On wide panes, the search stays left while all date controls
                // occupy one natural-width row aligned to the right edge.
                // In narrow split panes the date row drops below the search
                // without overlapping or changing its internal order.
                let narrow=toolbar.available_width()
                    < theme::MANAGEMENT_SEARCH_MIN_WIDTH + 520.0;
                if narrow {
                    if components::flexible_text_field(
                        toolbar,&mut self.query,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                        "거래처 / 주문 내용 검색"
                    ).changed(){changed=true;}
                    toolbar.add_space(theme::SPACE_8);
                    toolbar.with_layout(
                        egui::Layout::right_to_left(Align::Center),|right|{
                            right.horizontal(|dates|{
                                changed |= history_date_controls(
                                    dates,&mut self.date,&mut self.date_input,
                                    &mut self.calendar_month
                                );
                            });
                        }
                    );
                }else{
                    toolbar.horizontal(|tools|{
                        tools.spacing_mut().item_spacing.x=theme::SPACE_8;
                        if components::flexible_text_field(
                            tools,&mut self.query,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                            "거래처 / 주문 내용 검색"
                        ).changed(){changed=true;}
                        tools.with_layout(
                            egui::Layout::right_to_left(Align::Center),|right|{
                                right.horizontal(|dates|{
                                    changed |= history_date_controls(
                                        dates,&mut self.date,&mut self.date_input,
                                        &mut self.calendar_month
                                    );
                                });
                            }
                        );
                    });
                }
            });
            if changed {self.refresh(section.ctx().clone());}
            if !self.date_input.is_empty() && self.date!=self.date_input {
                theme::management_list_strip_frame().show(section,|ui|{
                    ui.label(typography::status("접수일은 YYYY-MM-DD 형식으로 입력하세요.",
                        theme::DANGER));
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
            components::soft_divider(section);
            let old_gap=section.spacing().item_spacing.y;
            section.spacing_mut().item_spacing.y=0.0;
            let viewport=section.available_width().max(1.0);
            let gutter=section.spacing().scroll.allocated_width()+theme::SPACE_4;
            let table_width=(viewport-gutter).max(MIN_WIDTH);
            let mut widths=COLS;
            let extra=(table_width-MIN_WIDTH).max(0.0);
            widths[1]+=extra*0.25;
            widths[2]+=extra*0.75;
            let headings=["접수일","거래처","배송 건 / 상품","관리 상태","청구 합계","처리"];
            let (head_rect,_)=section.allocate_exact_size(
                Vec2::new(viewport,HEADER_HEIGHT),Sense::hover());
            let mut header_origin=None;
            let mut pending_toggle=None;
            let snapshot=self.snapshot.as_ref();
            let lines=snapshot.map(|data|display_lines(data,&self.expanded,self.sort))
                .unwrap_or_default();
            let output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .id_salt("order-history-table-rows")
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .max_height(section.ctx().content_rect().height()
                        *theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
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
                });
            let left=header_origin.unwrap_or(head_rect.left()-output.state.offset.x);
            let header_cells=cells(egui::pos2(left,head_rect.top()),&widths,HEADER_HEIGHT);
            let painter=section.painter().with_clip_rect(head_rect);
            painter.rect_filled(head_rect,0.0,theme::RAISED);
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
                }else if index==3 || index==5{
                    (header_cells[index].center().x-label.size().x/2.0)
                        .max(header_cells[index].left()+theme::SPACE_8)
                }else{header_cells[index].left()+theme::SPACE_8};
                painter.galley(egui::pos2(x,
                    header_cells[index].center().y-label.size().y/2.0),
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
            section.spacing_mut().item_spacing.y=old_gap;
            if self.snapshot.as_ref().is_some_and(|s|s.rows.is_empty()) {
                theme::management_list_strip_frame().show(section,|empty|{
                    empty.label(typography::caption(if self.query.is_empty()
                        && self.date.is_empty(){
                        "저장된 주문이 없습니다."
                    }else{"조건에 맞는 주문이 없습니다."}));
                });
            }
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
