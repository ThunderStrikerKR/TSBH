//! React WorkflowQueue's Excel management view, now native egui.
//! Export, sender validation, workflow state and file history remain owned by
//! the original Rust business core and Tauri's existing SQLite database.
use crate::{components,gateway::{BandoGateway,ReadOperation},order_history_manager,settings_help,theme,typography};
use chrono::{DateTime,FixedOffset,NaiveDate,Utc};
use eframe::egui::{self,Align,Align2,Rect,Sense,Stroke,Ui,UiBuilder,Vec2};
use serde_json::{json,Value};
use std::{collections::{BTreeSet,HashMap},sync::{mpsc::{self,Receiver,TryRecvError},Arc}};

const HEIGHT:f32=theme::MANAGEMENT_COMPACT_ROW_HEIGHT;
const HEADER:f32=theme::MANAGEMENT_TABLE_HEADER_HEIGHT;
const GAP:f32=theme::FREIGHT_ASSIGNMENT_COL_GAP;
const QUEUE:[f32;5]=[48.0,212.0,354.0,178.0,182.0];
// Date, agency, exported items and file actions. The long products column
// expands into spare width while keeping the other columns readable.
const HISTORY:[f32;4]=[176.0,210.0,485.0,292.0];
fn text<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(v:&Value)->&[Value]{v.as_array().map(Vec::as_slice).unwrap_or(&[])}
fn find<'a>(array:&'a Value,id:&str)->Option<&'a Value>{
    rows(array).iter().find(|v|text(v,"id")==id)
}
fn datetime(raw:&str)->String{
    if let Ok(dt)=DateTime::parse_from_rfc3339(raw){
        return dt.with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
            .format("%Y-%m-%d %H:%M").to_string()
    }
    raw.to_owned()
}
fn amount(v:&Value)->String{
    v.as_i64().map(components::display_won)
        .unwrap_or_else(||"확인 필요".to_owned())
}
#[derive(Clone,Copy,PartialEq,Eq)]
enum Tab{Factory,Store,History}
impl Tab{
    const ALL:[Tab;3]=[Self::Factory,Self::Store,Self::History];
    fn origin(self)->&'static str{
        match self{Self::Factory=>"FACTORY",Self::Store=>"STORE",Self::History=>""}
    }
}
#[derive(Clone,Copy,PartialEq,Eq)]
enum Direction{Up,Down}
#[derive(Clone,Copy,PartialEq,Eq)]
struct Sort{column:usize,direction:Direction}
impl Sort{
    fn next(previous:Option<Self>,column:usize)->Option<Self>{
        match previous{
            Some(Self{column:c,direction:Direction::Up}) if c==column=>
                Some(Self{column,direction:Direction::Down}),
            Some(Self{column:c,direction:Direction::Down}) if c==column=>None,
            _=>Some(Self{column,direction:Direction::Up}),
        }
    }
}
#[derive(Clone)]
struct QueueRow{
    queue:Value,order:Value,shipment:Value,recipient:Value,client:Value,
    legacy:bool
}
impl QueueRow{
    fn id(&self)->&str{text(&self.queue,"id")}
    fn order_id(&self)->&str{text(&self.order,"id")}
    fn customer(&self)->String{
        if text(&self.order,"orderType")=="RETAIL"{
            "반도스탠드".into()
        }else{
            let name=text(&self.client,"displayName");
            if !name.is_empty(){name.to_owned()}
            else if !text(&self.client,"name").is_empty(){
                text(&self.client,"name").to_owned()
            }else{"거래처 확인 필요".into()}
        }
    }
    fn recipient_summary(&self)->String{
        let title=text(&self.recipient,"name");
        let items=rows(&self.recipient["items"]).iter().map(|item|
            format!("{} × {}",text(item,"name"),
                item["quantity"].as_i64().unwrap_or(0))
        ).collect::<Vec<_>>().join(" / ");
        if items.is_empty(){title.to_owned()}else{format!("{title} · {items}")}
    }
}
fn records(state:&Value,reads:&Value,tab:Tab,search:&str)->Vec<QueueRow>{
    let mut result=Vec::new();
    if tab==Tab::History{return result}
    let query=search.to_lowercase();
    for q in rows(&state["workflow"]["queue"]){
        if text(q,"origin")!=tab.origin()||text(q,"status")!="QUEUED"{continue}
        let Some(ship)=find(&state["workflow"]["shipments"],text(q,"shipmentId")) else {continue};
        let Some(order)=find(&state["orders"],text(ship,"orderId")) else {continue};
        let Some(original)=find(&order["recipients"],text(ship,"recipientId")) else {continue};
        let recipient=q.get("snapshot").and_then(|s|s.get("recipient"))
            .filter(|value|value.is_object()).unwrap_or(original);
        let client=rows(&reads["shipments"]).iter()
            .find(|r|text(r,"shipmentId")==text(ship,"id"))
            .and_then(|r|r.get("client")).cloned().unwrap_or(Value::Null);
        let entry=QueueRow{
            queue:q.clone(),shipment:ship.clone(),order:order.clone(),
            recipient:recipient.clone(),client,legacy:false
        };
        if format!("{} {} {}",entry.customer(),entry.recipient_summary(),
            text(&order,"raw")).to_lowercase().contains(&query){
            result.push(entry);
        }
    }
    if tab==Tab::Factory{
        for old in rows(&state["queue"]){
            let Some(id)=old.as_str() else {continue};
            if rows(&state["workflow"]["queue"]).iter().any(|q|
                find(&state["workflow"]["shipments"],text(q,"shipmentId"))
                    .is_some_and(|ship|text(ship,"orderId")==id)){continue}
            let Some(order)=find(&state["orders"],id) else{continue};
            let customer=text(&order["clientSnapshot"],"name");
            if !format!("{} {}",customer,text(order,"raw")).to_lowercase().contains(&query){
                continue
            }
            result.push(QueueRow{
                queue:json!({"id":id}),order:order.clone(),
                shipment:Value::Null,
                recipient:json!({"name":rows(&order["recipients"]).iter()
                    .map(|r|text(r,"name")).collect::<Vec<_>>().join(", ")}),
                client:order["clientSnapshot"].clone(),legacy:true
            });
        }
    }
    result
}
/// Never reconstruct exported products from today's editable order fields:
/// the XLSX ledger stores the exact frozen recipient snapshot for each row.
#[derive(Clone)]
struct HistoryRow{
    export:Value,
    clients:String,
    products:String,
}
fn export_day(raw:&str)->String{
    if let Ok(dt)=DateTime::parse_from_rfc3339(raw){
        return dt.with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
            .format("%Y-%m-%d").to_string();
    }
    raw.get(..10).filter(|day|
        NaiveDate::parse_from_str(day,"%Y-%m-%d").is_ok()
    ).unwrap_or("").to_owned()
}
fn export_date_matches(raw:&str,from:&str,to:&str)->bool{
    if from.is_empty()&&to.is_empty(){return true;}
    let day=export_day(raw);
    !day.is_empty()&&(from.is_empty()||day.as_str()>=from)
        &&(to.is_empty()||day.as_str()<=to)
}
fn history_projection(state:&Value,export:&Value)->HistoryRow{
    let mut clients=Vec::<String>::new();
    for order_id in rows(&export["orderIds"]).iter().filter_map(Value::as_str){
        let Some(order)=find(&state["orders"],order_id) else{continue};
        let snapshot=&order["confirmationSnapshot"]["client"];
        let client=if snapshot.is_object(){snapshot}
            else{&order["clientSnapshot"]};
        let name=if text(order,"orderType")=="RETAIL"{
            "반도스탠드"
        }else if !text(client,"displayName").is_empty(){
            text(client,"displayName")
        }else{text(client,"name")};
        if !name.is_empty() && !clients.iter().any(|x|x==name){
            clients.push(name.to_owned());
        }
    }
    // Old ledger records may not have orderIds. Do not invent a party name.
    let clients=if clients.is_empty(){"거래처 기록 없음".to_owned()}
        else{clients.join(" / ")};
    let mut product_lines=Vec::new();
    for queued in rows(&export["snapshots"]){
        for item in rows(&queued["snapshot"]["recipient"]["items"]){
            let name=text(item,"name").trim();
            if name.is_empty(){continue;}
            let qty=item["quantity"].as_i64().map(|v|v.to_string())
                .unwrap_or_else(||"수량 미확인".to_owned());
            product_lines.push(format!("◆ {name} × {qty}"));
        }
    }
    let products=if product_lines.is_empty(){
        "출력 당시 상품 기록 없음".to_owned()
    }else{product_lines.join("  ")};
    HistoryRow{export:export.clone(),clients,products}
}
fn export_history(
    state:&Value,query:&str,from:&str,to:&str
)->Vec<HistoryRow>{
    let query=query.trim().to_lowercase();
    let mut result=rows(&state["exports"]).iter()
        .filter(|e|export_date_matches(text(e,"at"),from,to))
        .map(|e|history_projection(state,e))
        .filter(|e|{
            let haystack=format!("{} {} {}",text(&e.export,"filename"),
                e.clients,e.products).to_lowercase();
            haystack.contains(&query)
        }).collect::<Vec<_>>();
    result.reverse();
    result
}
fn cmp_queues(a:&QueueRow,b:&QueueRow,column:usize)->std::cmp::Ordering{
    match column{
        1=>a.customer().cmp(&b.customer()),
        2=>a.recipient_summary().cmp(&b.recipient_summary()),
        3=>a.queue["snapshot"]["total"].as_i64()
            .cmp(&b.queue["snapshot"]["total"].as_i64()),
        _=>std::cmp::Ordering::Equal
    }
}
fn cmp_history(a:&HistoryRow,b:&HistoryRow,column:usize)->std::cmp::Ordering{
    match column{
        0=>text(&a.export,"at").cmp(text(&b.export,"at")),
        1=>a.clients.cmp(&b.clients),
        2=>a.products.cmp(&b.products),
        _=>std::cmp::Ordering::Equal
    }
}
fn sort_queue(lines:&mut [QueueRow],sort:Option<Sort>){
    if let Some(sort)=sort{
        lines.sort_by(|a,b|{
            let cmp=cmp_queues(a,b,sort.column);
            if sort.direction==Direction::Down{cmp.reverse()}else{cmp}
        });
    }
}
fn sort_history(lines:&mut [HistoryRow],sort:Option<Sort>){
    if let Some(sort)=sort{
        lines.sort_by(|a,b|{
            let cmp=cmp_history(a,b,sort.column);
            if sort.direction==Direction::Down{cmp.reverse()}else{cmp}
        });
    }
}
fn geometry<const N:usize>(
    origin:egui::Pos2,widths:&[f32;N],height:f32
)->[Rect;N]{
    let mut x=origin.x;
    std::array::from_fn(|col|{
        let cell=Rect::from_min_size(egui::pos2(x,origin.y),Vec2::new(widths[col],height));
        x+=widths[col]+GAP;
        cell
    })
}
fn cell(ui:&Ui,rect:Rect,visible:&str,align:Align2,
    role:typography::ManagementTableCellRole,id:impl std::hash::Hash){
    let padded=if align==Align2::LEFT_CENTER{
        Rect::from_min_max(
            egui::pos2(rect.left()+theme::MANAGEMENT_TABLE_TEXT_LEADING_INSET,rect.top()),
            egui::pos2(rect.right()-theme::SPACE_8,rect.bottom())
        )
    }else{
        rect.shrink2(Vec2::new(theme::SPACE_8,0.0))
    };
    let clip=padded.intersect(ui.clip_rect());
    if clip.width()<=0.0||clip.height()<=0.0{return}
    let color=typography::management_table_cell_color(role);
    let label=typography::management_table_cell_galley(ui,visible,role);
    let x=if align==Align2::RIGHT_CENTER{
        padded.right()-label.size().x
    }else if align==Align2::CENTER_CENTER{
        padded.center().x-label.size().x/2.0
    }else{padded.left()};
    ui.painter().with_clip_rect(clip).galley(
        egui::pos2(x,padded.center().y-label.size().y/2.0),label,color
    );
    if !visible.is_empty(){
        ui.interact(clip,ui.make_persistent_id(("excel-table-cell",id)),Sense::hover())
            .on_hover_text(visible);
    }
}
/// Each Excel tab shows one table at a time. Loading and empty-state text
/// belongs to the scrollable table BODY, not the search-to-header transition.
/// Only expanded detail editors may reduce the visible table height.
#[derive(Clone,Copy)]
enum ExcelTableHeightPolicy {
    FillCard { card_min_height:f32 },
    LegacyForExpandedDetails,
}

/// D-type current table state, deliberately distinct from A/B/C action notices.
/// A failed load is reported through FloatingNotice, never as "0 results".
fn table_state_message(
    tab:Tab, query:&str, count:usize, loading:bool, failed:bool,
)->Option<&'static str>{
    if count>0 {return None;}
    if loading {return Some("Excel 자료를 확인 중입니다.");}
    if failed {return None;}
    match tab {
        Tab::History => Some(if query.is_empty(){
            "Excel 작성 이력이 없습니다."
        }else{"검색 조건에 맞는 Excel 작성 이력이 없습니다."}),
        Tab::Factory | Tab::Store => Some(if query.is_empty(){
            "발송 대기 건이 없습니다."
        }else{"검색 조건에 맞는 발송 대기 건이 없습니다."}),
    }
}
/// Pure visible-body geometry. Scrollbar allocation is excluded on the right,
/// and all painting clips to the card rather than creating layout rows.
fn table_state_body_rect(
    scroll_track:Rect,viewport_width:f32,gutter:f32,
    scroll_height:f32,clip:Rect,
)->Option<Rect>{
    let width=(viewport_width-gutter).min(scroll_track.width()).max(0.0);
    let body=Rect::from_min_size(
        scroll_track.min,Vec2::new(width,scroll_height.max(0.0))
    );
    let visible=body.intersect(clip);
    if visible.width()>0.0&&visible.height()>0.0 {Some(visible)}
    else{None}
}

fn table<const N:usize>(
    ui:&mut Ui,id:&str,titles:[&str;N],base:[f32;N],
    align:[Align2;N],stretch:usize,rows:usize,
    height_policy:ExcelTableHeightPolicy,
    sort:&mut Option<Sort>,sort_enabled:[bool;N],
    header_selection:Option<(bool,bool)>,
    empty_message:Option<&str>,
    mut render:impl FnMut(&mut Ui,[Rect;N],usize)
)->Option<bool>{
    let old_gap=ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y=0.0;
    let minimum=base.iter().sum::<f32>()+(N-1) as f32*GAP;
    // The toolbar and 36pt header always share the two-master 16+1+4
    // transition. No status or error line may change their coordinates.
    let layout=components::management_sticky_table_header(ui,minimum);
    let width=layout.table_width;
    let widths=theme::management_single_stretch_columns(base,width,GAP,stretch);
    let header=layout.header_rect;
    let header_content=layout.header_content_rect;
    let scroll_max_height=match height_policy {
        ExcelTableHeightPolicy::FillCard{card_min_height}=>
            components::management_single_list_scroll_height(
                ui,card_min_height,0.0
            ),
        ExcelTableHeightPolicy::LegacyForExpandedDetails=>
            ui.ctx().content_rect().height()*theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION,
    };
    // No 36pt empty-state footer: the caption is painted inside the body.
    // Rail extension still uses the same shared master scroll policy.
    let scroll_track=components::management_table_scroll_track_rect(
        ui,scroll_max_height
    );
    let mut left=None;
    let output=components::virtual_fixed_rows(
        egui::ScrollArea::both().scroll_bar_rect(scroll_track).id_salt(id)
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .max_height(scroll_max_height)
            .auto_shrink([false,false]),
        ui,width,HEIGHT,rows,|body,range|{
            let first=range.start;
            let origin=body.max_rect().min;
            for index in range{
                let r=Rect::from_min_size(egui::pos2(origin.x,
                    origin.y+(index-first) as f32*HEIGHT),Vec2::new(width,HEIGHT));
                left.get_or_insert(r.left());
                body.scope_builder(UiBuilder::new().id_salt((id,index)).max_rect(r)
                    .layout(egui::Layout::top_down(Align::Min)),|row|{
                    row.allocate_exact_size(Vec2::new(width,HEIGHT),Sense::hover());
                    row.painter().rect_filled(r,0.0,
                        if index%2==0{theme::PANEL}else{theme::TABLE_ROW_ALT_BG});
                    let cols=geometry(r.min,&widths,HEIGHT);
                    render(row,cols,index);
                    for c in cols.iter().take(N-1){
                        let x=c.right()+GAP/2.0;
                        row.painter().line_segment(
                            [egui::pos2(x,r.top()),egui::pos2(x,r.bottom())],
                            Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER));
                    }
                    row.painter().line_segment([r.left_bottom(),r.right_bottom()],
                        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER));
                });
            }
        });
    ui.spacing_mut().item_spacing.y=old_gap;
    let left=left.unwrap_or(header.left()-output.state.offset.x);
    let heads=geometry(egui::pos2(left,header.top()),&widths,HEADER);
    let painter=ui.painter().with_clip_rect(header);
    painter.rect_filled(header,0.0,theme::RAISED);
    // Same inner 2pt outline and 34pt caption area as Freight Assignment.
    // The rightmost native scrollbar gutter is NEVER part of header text.
    let header_text_rect=Rect::from_min_max(
        header_content.min,
        egui::pos2(
            header_content.right(),
            header_content.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
        )
    );
    let mut clicked=None;
    for (i,title) in titles.iter().enumerate(){
        let h=heads[i];
        let title=if i==0&&header_selection.is_some(){""}else{title};
        let label=typography::freight_table_header_galley(ui,title);
        let arrow=if sort_enabled[i]{
            let (glyph,color)=match (*sort).filter(|s|s.column==i)
                .map(|s|s.direction){
                Some(Direction::Up)=>("▲",theme::SELECTION),
                Some(Direction::Down)=>("▼",theme::SELECTION),
                None=>("↕",theme::MUTED)
            };
            Some((typography::freight_table_header_galley(ui,glyph),color))
        }else{None};
        let gap=if arrow.is_some(){theme::SPACE_4}else{0.0};
        let arrow_width=arrow.as_ref().map_or(0.0,|(g,_)|g.size().x);
        let total_width=label.size().x+gap+arrow_width;
        let x=if align[i]==Align2::RIGHT_CENTER{
            h.right()-theme::SPACE_8-total_width
        }else if align[i]==Align2::CENTER_CENTER{
            h.center().x-total_width/2.0
        }else{h.left()+theme::MANAGEMENT_TABLE_TEXT_LEADING_INSET};
        let visible=h.intersect(header_text_rect);
        if visible.width()>0.0 && visible.height()>0.0 {
            let ink=painter.with_clip_rect(visible);
            ink.galley(
                egui::pos2(x,header_text_rect.center().y-label.size().y/2.0),
                label.clone(),theme::SECONDARY
            );
            if let Some((glyph,color))=arrow{
                ink.galley(
                    egui::pos2(x+label.size().x+gap,
                        header_text_rect.center().y-glyph.size().y/2.0),
                    glyph,color
                );
            }
        }
        if sort_enabled[i]{
            let hit=h.intersect(header_text_rect);
            if hit.width()>0.0&&hit.height()>0.0&&ui.interact(hit,
                ui.make_persistent_id((id,"sort",i)),Sense::click()).clicked(){
                clicked=Some(Sort::next(*sort,i));
            }
        }
    }
    if let Some(next)=clicked{*sort=next;}
    // Paint the very same column separators as the data rows.
    for c in heads.iter().take(N.saturating_sub(1)) {
        let x=c.right()+GAP/2.0;
        if x>=header_content.left() && x<=header_content.right() {
            painter.line_segment(
                [egui::pos2(x,header.top()),egui::pos2(x,header.bottom())],
                Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
            );
        }
    }
    // Solid outline is painted wholly INSIDE the 36pt header, like Master.
    painter.rect_filled(
        Rect::from_min_max(
            egui::pos2(
                header.left(),
                header.bottom()-theme::MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT
            ),
            header.right_bottom()
        ),
        0.0,theme::TABLE_HEADER_OUTLINE
    );
    if let Some((all,enabled))=header_selection{
        let mut checked=all;
        if components::management_table_checkbox(
            ui,heads[0],header_text_rect,&mut checked,enabled,
            (id,"select-all")
        ).changed(){
            return Some(checked);
        }
    }
    if let Some(message)=empty_message.filter(|_|rows==0){
        // D-type status lives in the visible body without reserving even 1pt.
        // Center inside the DATA viewport (scrollbar gutter excluded), like
        // Order History, and clip to the actual master card.
        if let Some(visible)=table_state_body_rect(
            scroll_track,layout.viewport_width,layout.scrollbar_gutter,
            scroll_max_height,ui.clip_rect()
        ){
            ui.painter().with_clip_rect(visible).text(
                visible.center(),Align2::CENTER_CENTER,message,
                egui::FontId::proportional(typography::FONT_METADATA),
                theme::MUTED
            );
        }
    }
    None
}
#[derive(Default)]
struct Snapshot{state:Value,shipments:Value}
pub struct ExcelManager{
    gateway:Arc<dyn BandoGateway>,
    receiver:Option<Receiver<Result<Snapshot,String>>>,
    result:Option<Snapshot>,
    operation:Option<Receiver<Result<Value,String>>>,
    operation_name:Option<String>,
    preview:Option<Value>,
    error:Option<String>,
    notice:Option<String>,
    tab:Tab,
    query:String,
    selected:BTreeSet<String>,
    sort_queue:Option<Sort>,
    sort_history:Option<Sort>,
    export_from:String,
    export_to:String,
    export_from_month:NaiveDate,
    export_to_month:NaiveDate,
    managed_queue:Option<String>,
    freight_drafts:HashMap<String,String>,
    open_order:Option<String>,
    open_settings:bool,
}
impl ExcelManager{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let today=Utc::now().with_timezone(
            &FixedOffset::east_opt(9*3600).expect("Seoul offset")
        ).date_naive();
        let mut manager=Self{
            gateway,receiver:None,result:None,operation:None,operation_name:None,
            preview:None,error:None,notice:None,tab:Tab::Factory,
            query:String::new(),selected:BTreeSet::new(),
            sort_queue:None,sort_history:None,
            export_from:String::new(),export_to:String::new(),
            export_from_month:components::calendar_month_start(today),
            export_to_month:components::calendar_month_start(today),
            managed_queue:None,freight_drafts:HashMap::new(),
            open_order:None,open_settings:false,
        };
        manager.refresh(ctx);manager
    }
    pub fn take_open_order(&mut self)->Option<(String,String)>{
        let id=self.open_order.take()?;
        let when=self.result.as_ref()
            .and_then(|snapshot|find(&snapshot.state["orders"],&id))
            .map(|order|{
                let confirmed=text(order,"confirmedAt");
                if confirmed.is_empty(){text(order,"createdAt")}
                else{confirmed}
            }).unwrap_or("").to_owned();
        Some((id,when))
    }
    pub fn take_open_settings(&mut self)->bool{
        std::mem::take(&mut self.open_settings)
    }
    pub fn refresh(&mut self,ctx:egui::Context){
        if self.operation.is_some(){return}
        self.error=None;self.result=None;self.preview=None;
        let (sender,receiver)=mpsc::channel();
        self.receiver=Some(receiver);
        let gateway=Arc::clone(&self.gateway);
        std::thread::spawn(move||{
            let result=gateway.load_business_state().and_then(|state|{
                let shipments=gateway.execute(ReadOperation::ReadShipments,
                    json!({"state":&state}))?;
                Ok(Snapshot{state,shipments})
            });
            let _=sender.send(result);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self){
        let read=self.receiver.as_ref().map(Receiver::try_recv);
        match read{
            Some(Ok(Ok(value)))=>{
                self.result=Some(value);self.error=None;self.receiver=None;
            }
            Some(Ok(Err(message)))=>{
                self.error=Some(message);self.result=None;self.receiver=None;
            }
            Some(Err(TryRecvError::Disconnected))=>{
                self.error=Some("Excel 목록 조회가 중단되었습니다.".into());
                self.result=None;self.receiver=None;
            }
            _=>{}
        }
        let write=self.operation.as_ref().map(Receiver::try_recv);
        match write{
            Some(Ok(Ok(result)))=>{
                self.operation=None;
                let operation=self.operation_name.take().unwrap_or_default();
                if operation=="preview"{
                    self.preview=Some(result);
                }else if operation=="workflow"{
                    let action=text(&result,"action");
                    if action=="record_actual_freight"{
                        self.freight_drafts.remove(text(&result,"queueId"));
                    }
                    self.notice=Some(match action{
                        "restore_shipment"=>"배송 건을 처리 중 주문으로 복구했습니다.",
                        "mark_store_sent"=>"매장 발송을 확인했습니다.",
                        _=>"Excel 관리 변경사항을 저장했습니다.",
                    }.into());
                    self.result=Some(Snapshot{
                        state:result["state"].clone(),
                        shipments:result["shipmentReads"].clone()
                    });
                    self.selected.clear();
                }else if operation=="export"{
                    let count=result["count"].as_u64().unwrap_or(0);
                    let origin=text(&result,"origin");
                    self.notice=Some(format!(
                        "{} {}건 Excel 저장과 출력 이력을 기록했습니다.",
                        if origin=="FACTORY"{"공장 발송"}else{"매장 발송"},count
                    ));
                    self.selected.clear();
                    if let Some(warning)=result["folderWarning"].as_str(){
                        self.notice=Some(format!(
                            "Excel과 출력 이력은 저장됐지만 폴더를 열지 못했습니다: {warning}"
                        ));
                    }
                    self.result=None;
                    // A saved file must be reflected via a fresh DB read,
                    // not through an optimistic local snapshot.
                    self.result=Some(Snapshot{
                        state:result["state"].clone(),
                        shipments:result["shipmentReads"].clone()
                    });
                    // Never display a stale shipment projection after saving.
                    self.receiver=None;
                }else{
                    self.notice=Some("요청한 Excel 작업을 처리했습니다.".into());
                }
            },
            Some(Ok(Err(error)))=>{
                self.error=Some(error);self.operation=None;
                self.operation_name=None;
            },
            Some(Err(TryRecvError::Disconnected))=>{
                self.error=Some("Excel 작업 연결이 중단되었습니다.".into());
                self.operation=None;self.operation_name=None;
            },
            _=>{}
        }
    }
    fn task(&mut self,action:&str,ctx:egui::Context,
        work:impl FnOnce(Arc<dyn BandoGateway>)->Result<Value,String>+Send+'static){
        if self.operation.is_some(){return}
        crate::floating_notice::dispatch_pointer(&ctx,"excel.action");
        let (sender,receiver)=mpsc::channel();
        self.operation=Some(receiver);self.operation_name=Some(action.into());
        self.error=None;self.notice=None;
        let gateway=Arc::clone(&self.gateway);
        std::thread::spawn(move||{
            let result=work(gateway);
            let _=sender.send(result);
            ctx.request_repaint();
        });
    }
    pub fn show_page(&mut self,ui:&mut Ui){
        let page_origin_y=ui.cursor().top();
        ui.spacing_mut().scroll=theme::management_scroll_style();
        self.poll();
        crate::floating_notice::sync(
            ui.ctx(),"excel.result","excel.action",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"excel.error","excel.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        components::management_page_title_top_gap(ui);
        components::management_top_content(ui,|top|{
            top.horizontal(|title|{
                title.label(typography::page_heading("엑셀 관리"));
                settings_help::icon(title,"발송 Excel 관리");
            });
        });
        ui.add_space(theme::SPACE_8);
        let busy=self.operation.is_some()||self.receiver.is_some();
        let next_tab=components::management_top_content(ui,|top|
            components::management_tabs(top,&[
                ("공장 엑셀 대기",self.tab==Tab::Factory,!busy),
                ("매장 발송 대기",self.tab==Tab::Store,!busy),
                ("엑셀 작성 이력",self.tab==Tab::History,!busy),
            ])
        );
        if let Some(next)=next_tab{
            let selected=Tab::ALL[next];
            if selected!=self.tab{
                self.tab=selected;
                self.selected.clear();self.query.clear();
                self.sort_history=None;self.sort_queue=None;
                self.managed_queue=None;
            }
        }
        ui.add_space(theme::SPACE_16);
        let mut exported=None;
        let mut open_file=None;
        let mut open_daily=false;
        let mut workflow_action:Option<(String,String,String,String,Value)>=None;
        let mut manage_clicked=None;
        let mut open_order=None;
        let mut copy_path=None;
        let mut go_settings=false;
        // Resolve the currently selectable queue BEFORE allocating the fixed
        // toolbar: its right-hand action caption and enabled state must use
        // the same visible selection as the table below.
        let mut queued=if self.tab==Tab::History{
            Vec::new()
        }else if let Some(snapshot)=self.result.as_ref(){
            records(&snapshot.state,&snapshot.shipments,self.tab,&self.query)
        }else{Vec::new()};
        sort_queue(&mut queued,self.sort_queue);
        let mut visible:BTreeSet<String>=queued.iter().filter(|r|!r.legacy)
            .map(|r|r.id().to_owned()).collect();
        if self.tab!=Tab::History{
            self.selected.retain(|id|visible.contains(id));
        }
        components::align_management_primary_card(ui,page_origin_y);
        // Exact same 16pt interior inset and frame as the approved master.
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        theme::management_master_table_frame().show(ui,|section|{
            components::fill_management_primary_card(section,card_min_height);
            section.spacing_mut().scroll=theme::management_table_scroll_style();
            // Allocate a true 36pt master toolbar. Search and trailing
            // actions use the SAME 16pt card interior width as the header.
            section.spacing_mut().item_spacing.y=0.0;
            let toolbar_width=section.available_width().max(1.0);
            let has_actions=self.tab!=Tab::History;
            let history_tab=self.tab==Tab::History;
            let history_range_width=order_history_manager::EXCEL_EXPORT_DATE_CONTROLS_WIDTH;
            let history_one_row=history_tab&&toolbar_width>=
                theme::MANAGEMENT_SEARCH_MIN_WIDTH+theme::SPACE_16
                    +history_range_width;
            let history_compact=history_tab&&toolbar_width<history_range_width;
            let stacked=has_actions&&toolbar_width
                <theme::MANAGEMENT_SEARCH_MIN_WIDTH+theme::SPACE_16
                    +theme::MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH;
            let toolbar_height=if history_tab{
                if history_one_row{theme::INPUT_HEIGHT}else{
                    theme::INPUT_HEIGHT+theme::SPACE_8+
                        if history_compact{
                            theme::INPUT_HEIGHT*2.0+theme::SPACE_8
                        }else{theme::INPUT_HEIGHT}
                }
            }else if stacked{
                theme::INPUT_HEIGHT*2.0+theme::SPACE_8
            }else{theme::INPUT_HEIGHT};
            let (toolbar_rect,_)=section.allocate_exact_size(
                Vec2::new(toolbar_width,toolbar_height),Sense::hover()
            );
            let search_width=if history_one_row{
                (toolbar_width-history_range_width-theme::SPACE_16).max(1.0)
            }else if stacked||history_tab{
                toolbar_width
            }else{
                (toolbar_width-theme::MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH
                    -theme::SPACE_16).max(1.0)
            };
            let search_rect=Rect::from_min_size(
                toolbar_rect.min,Vec2::new(search_width,theme::INPUT_HEIGHT)
            );
            section.scope_builder(
                UiBuilder::new().id_salt("excel-master-search")
                    .max_rect(search_rect)
                    .layout(egui::Layout::left_to_right(Align::Center)),
                |search|{
                    search.set_clip_rect(search_rect);
                    search.set_width(search_rect.width());
                    if components::flexible_text_field(
                        search,&mut self.query,
                        theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                        if history_tab{
                            "거래처 / 상품명 / 파일 검색"
                        }else{"거래처 / 수취인 / 상품 검색"}
                    ).changed(){
                        self.selected.clear();
                        // Rebuild rows in this SAME frame, so changing the
                        // query cannot leave an out-of-date table beneath
                        // the master toolbar until the next repaint.
                        if self.tab!=Tab::History{
                            queued=if let Some(snapshot)=self.result.as_ref(){
                                records(&snapshot.state,&snapshot.shipments,
                                    self.tab,&self.query)
                            }else{Vec::new()};
                            sort_queue(&mut queued,self.sort_queue);
                            visible=queued.iter().filter(|r|!r.legacy)
                                .map(|r|r.id().to_owned()).collect();
                        }
                        search.ctx().request_repaint();
                    }
                }
            );
            if history_tab{
                // Use the exact date/calendar widgets from Order History.
                // The Excel archive has only one date basis: file creation.
                let date_height=if history_compact{
                    theme::INPUT_HEIGHT*2.0+theme::SPACE_8
                }else{theme::INPUT_HEIGHT};
                let dates_rect=components::management_toolbar_trailing_rect(
                    toolbar_rect,history_range_width.min(toolbar_width),
                    if history_one_row{0.0}else{
                        theme::INPUT_HEIGHT+theme::SPACE_8
                    },date_height
                );
                section.scope_builder(
                    UiBuilder::new().id_salt("excel-export-dates")
                        .max_rect(dates_rect)
                        .layout(egui::Layout::left_to_right(Align::Center)
                            .with_main_align(Align::Max)),
                    |dates|{
                        dates.set_width(dates_rect.width());
                        dates.set_height(dates_rect.height());
                        order_history_manager::excel_export_date_controls(
                            dates,&mut self.export_from,&mut self.export_to,
                            &mut self.export_from_month,&mut self.export_to_month,
                            history_compact
                        );
                    }
                );
                let range_error=order_history_manager::date_range_error(
                    &self.export_from,&self.export_to
                );
                crate::floating_notice::sync(
                    section.ctx(),"excel.export-dates","excel.history-dates",
                    crate::floating_notice::Tone::Error,range_error
                );
            }
            if has_actions{
                let actions_y=if stacked{
                    theme::INPUT_HEIGHT+theme::SPACE_8
                }else{0.0};
                let actions_rect=components::management_toolbar_trailing_rect(
                    toolbar_rect,
                    theme::MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH.min(toolbar_width),
                    actions_y,theme::INPUT_HEIGHT
                );
                section.scope_builder(
                    UiBuilder::new().id_salt("excel-master-actions")
                        .max_rect(actions_rect)
                        .layout(egui::Layout::right_to_left(Align::Center)),
                    |actions|{
                        actions.set_clip_rect(actions_rect);
                        actions.set_width(actions_rect.width());
                        if theme::action(actions,"폴더 열기",
                            theme::ButtonRole::Standard,!busy).clicked(){
                            open_daily=true;
                        }
                        if theme::action(actions,
                            &format!("선택 {}건 {} Excel 작성",
                                self.selected.len(),
                                if self.tab==Tab::Factory{"공장"}else{"매장"}),
                            theme::ButtonRole::Primary,
                            !self.selected.is_empty()&&!busy
                                &&self.gateway.excel_write_enabled()
                        ).clicked(){
                            exported=Some(self.selected.iter().cloned()
                                .collect::<Vec<_>>());
                        }
                    }
                );
            }
            // One stable search-to-header transition for loading, empty,
            // populated and error states. Action notices float independently;
            // D-type loading/empty captions are drawn in the BODY below.
            let state=self.result.as_ref().map(|r|&r.state);
            let native=self.result.as_ref().map(|r|&r.shipments);
            if self.tab==Tab::History{
                let mut history=state.map(|s|{
                    export_history(s,&self.query,&self.export_from,&self.export_to)
                }).unwrap_or_default();
                sort_history(&mut history,self.sort_history);
                let indexes=0..history.len();
                let order_list=history.clone();
                let empty_message=table_state_message(
                    self.tab,&self.query,history.len(),self.receiver.is_some(),
                    self.error.is_some()
                );
                let _=table(section,"excel-history-table",
                    ["작성 날짜","거래처","상품 목록","조회"],
                    HISTORY,[Align2::LEFT_CENTER,Align2::LEFT_CENTER,
                        Align2::LEFT_CENTER,Align2::CENTER_CENTER],
                    2,indexes.len(),
                    ExcelTableHeightPolicy::FillCard{card_min_height},
                    &mut self.sort_history,
                    [true,true,true,false],None,empty_message,|row,cells,i|{
                        let entry=&order_list[i];
                        let id=text(&entry.export,"id");
                        cell(row,cells[0],&datetime(text(&entry.export,"at")),
                            Align2::LEFT_CENTER,typography::ManagementTableCellRole::Text,(id,0));
                        cell(row,cells[1],&entry.clients,
                            Align2::LEFT_CENTER,typography::ManagementTableCellRole::ClientName,(id,1));
                        cell(row,cells[2],&entry.products,
                            Align2::LEFT_CENTER,typography::ManagementTableCellRole::ProductName,(id,2));
                        row.scope_builder(UiBuilder::new()
                            .id_salt(("excel-history-actions",id))
                            .max_rect(cells[3].shrink2(Vec2::new(theme::SPACE_8,0.0)))
                            .layout(egui::Layout::left_to_right(Align::Center)),
                            |buttons|{
                                if !text(&entry.export,"savedPath").is_empty() {
                                    if theme::action(buttons,"파일 열기",
                                        theme::ButtonRole::TableSecondary,!busy).clicked(){
                                        open_file=Some(id.to_owned());
                                    }
                                    if theme::action(buttons,"경로 복사",
                                        theme::ButtonRole::TableSecondary,!busy).clicked(){
                                        copy_path=Some(text(&entry.export,"savedPath").to_owned());
                                    }
                                }
                            });
                    });
            }else{
                let all=!visible.is_empty()
                    &&self.selected.len()==visible.len();
                let records=queued;
                // The Excel/export actions are now in the fixed search
                // toolbar. Do not reserve an obsolete bottom footer.
                // Dynamic per-shipment editors still use the conservative cap.
                let queue_height_policy=if self.managed_queue.is_some(){
                    ExcelTableHeightPolicy::LegacyForExpandedDetails
                }else{
                    ExcelTableHeightPolicy::FillCard{card_min_height}
                };
                let empty_message=table_state_message(
                    self.tab,&self.query,records.len(),self.receiver.is_some(),
                    self.error.is_some()
                );
                let toggle_all=table(section,"excel-queue-table",
                    ["선택","거래처 / 발주","수취인 / 배송 품목","금액","처리"],
                    QUEUE,[Align2::CENTER_CENTER,Align2::LEFT_CENTER,
                        Align2::LEFT_CENTER,Align2::RIGHT_CENTER,
                        Align2::CENTER_CENTER],
                    2,records.len(),queue_height_policy,
                    &mut self.sort_queue,
                    [false,true,true,true,false],Some((all,!visible.is_empty())),
                    empty_message,|row,cells,index|{
                        let entry=&records[index];
                        let id=entry.id().to_owned();
                        let mut checked=self.selected.contains(&id);
                        let selectable=!entry.legacy;
                        let checked_response=components::management_table_checkbox(
                            row,cells[0],cells[0],&mut checked,selectable,
                            ("excel-queue-checkbox",id.clone())
                        );
                        if checked_response.changed() && selectable{
                            if checked{self.selected.insert(id.clone());}
                            else{self.selected.remove(&id);}
                        }
                        if entry.legacy {
                            checked_response.on_disabled_hover_text(
                                "기존 주문은 출고 이관 후 출력할 수 있습니다."
                            );
                        }
                        cell(row,cells[1],&entry.customer(),
                            Align2::LEFT_CENTER,typography::ManagementTableCellRole::ClientName,(id.clone(),1));
                        cell(row,cells[2],&entry.recipient_summary(),
                            Align2::LEFT_CENTER,typography::ManagementTableCellRole::Description,(id.clone(),2));
                        let price=if entry.legacy{"기존 확정 금액 유지".to_owned()}
                            else if text(&entry.order,"orderType")=="RETAIL"{
                                "결제 완료 주문".into()
                            }else{amount(&entry.queue["snapshot"]["total"])};
                        cell(row,cells[3],&price,
                            Align2::RIGHT_CENTER,typography::ManagementTableCellRole::Money,(id.clone(),3));
                        row.scope_builder(UiBuilder::new()
                            .id_salt(("excel-queue-action",id.clone()))
                            .max_rect(cells[4].shrink2(Vec2::new(theme::SPACE_8,0.0)))
                            .layout(egui::Layout::left_to_right(Align::Center)),
                            |buttons|{
                                if theme::action(buttons,"상세 보기",
                                    theme::ButtonRole::TableSecondary,true).clicked(){
                                    open_order=Some(entry.order_id().to_owned());
                                }
                                if selectable && theme::action(
                                    buttons,"관리",theme::ButtonRole::TableSecondary,true
                                ).clicked(){
                                    manage_clicked=Some(id.clone());
                                }
                            });
                    }
                );
                if let Some(checked)=toggle_all{
                    if checked{self.selected=visible;}
                    else{self.selected.clear();}
                }
                if let Some(id)=manage_clicked.take(){
                    self.managed_queue=if self.managed_queue.as_deref()==Some(&id){
                        None
                    }else{Some(id)};
                }
                if let Some(id)=self.managed_queue.as_deref(){
                    if let Some(entry)=records.iter().find(|r|!r.legacy&&r.id()==id){
                        section.scope(|details|{
                            details.horizontal(|line|{
                                line.label(typography::subsection_heading("관리"));
                                // Actual-freight edits remain local until the
                                // original Rust record_actual_freight action
                                // commits through the exact revision guard.
                                let origin=self.tab.origin();
                                let saved=native.and_then(|read|rows(&read["shipments"])
                                    .iter().find(|ship|
                                        text(ship,"shipmentId")==text(&entry.shipment,"id")
                                    )).and_then(|ship|ship["actualFreight"][origin].as_i64());
                                let source=saved.map(|v|v.to_string())
                                    .unwrap_or_default();
                                let draft=self.freight_drafts.entry(id.to_owned())
                                    .or_insert_with(||source.clone());
                                line.label(typography::field_label(
                                    if self.tab==Tab::Factory{
                                        "공장 실제 출고 운임"
                                    }else{"매장 실제 출고 운임"}
                                ));
                                components::money_field(
                                    line,draft,!busy&&self.gateway.excel_write_enabled()
                                );
                                let parsed=if draft.trim().is_empty(){
                                    Some(None)
                                }else{
                                    components::parse_money_input(draft).map(Some)
                                };
                                let dirty=parsed!=Some(saved);
                                if theme::action(
                                    line,"실제 운임 저장",
                                    theme::ButtonRole::Primary,
                                    dirty&&!busy&&self.gateway.excel_write_enabled()
                                ).clicked(){
                                    let value=if draft.trim().is_empty(){
                                        Some(Value::Null)
                                    }else{
                                        components::parse_money_input(draft)
                                            .map(|amount|json!(amount))
                                    };
                                    if let Some(value)=value{
                                        workflow_action=Some((
                                            "record_actual_freight".into(),
                                            text(&entry.shipment,"id").into(),
                                            id.into(),origin.into(),value
                                        ));
                                    }else{
                                        self.error=Some(
                                            "올바른 실제 운임을 입력하세요.".into()
                                        );
                                    }
                                }
                                if text(&entry.order,"orderType")!="RETAIL"{
                                    if theme::action(line,"이관 복구",
                                        theme::ButtonRole::Standard,
                                        !busy&&self.gateway.excel_write_enabled()
                                    ).clicked(){
                                        workflow_action=Some((
                                            "restore_shipment".into(),
                                            text(&entry.shipment,"id").into(),
                                            id.into(),self.tab.origin().into(),Value::Null
                                        ));
                                    }
                                }
                                if self.tab==Tab::Store&&theme::action(
                                    line,"매장 발송 확인",theme::ButtonRole::Standard,
                                    !busy&&self.gateway.excel_write_enabled()
                                ).clicked(){
                                    workflow_action=Some((
                                        "mark_store_sent".into(),
                                        text(&entry.shipment,"id").into(),
                                        id.into(),self.tab.origin().into(),Value::Null
                                    ));
                                }
                            });
                        });
                    }
                }
            }
        });
        if let Some(id)=open_file{
            self.task("file",ui.ctx().clone(),move|g|{
                g.open_excel_file(&id)?;
                Ok(json!({}))
            });
        }
        if let Some(path)=copy_path{
            crate::floating_notice::dispatch_pointer(ui.ctx(),"excel.action");
            ui.ctx().copy_text(path);
            self.notice=Some("저장 경로를 복사했습니다.".into());
        }
        if open_daily{
            let origin=self.tab.origin().to_owned();
            self.task("folder",ui.ctx().clone(),move|g|{
                g.open_excel_folder(None,Some(&origin))?;Ok(json!({}))
            });
        }
        if let Some(id)=open_order{self.open_order=Some(id);}
        if let Some(ids)=exported{
            let Some(state)=self.result.as_ref().map(|x|x.state.clone()) else{return};
            let origin=self.tab.origin().to_owned();
            self.task("preview",ui.ctx().clone(),move|g|{
                let operation=if origin=="FACTORY"{
                    ReadOperation::PrepareFactoryExport
                }else{ReadOperation::PrepareStoreExport};
                let preview=g.execute(operation,json!({"state":state,"ids":ids}))?;
                let sender=g.execute(ReadOperation::ReviewExportSender,json!({
                    "state":state,"ids":ids
                }))?;
                Ok(json!({"preview":preview,"sender":sender}))
            });
        }
        if let Some((action,shipment_id,queue_id,origin,value))=workflow_action{
            let Some(revision)=self.result.as_ref()
                .and_then(|s|s.state["revision"].as_u64()) else{
                self.error=Some("업무 DB 리비전을 확인할 수 없습니다.".into());
                return;
            };
            self.task("workflow",ui.ctx().clone(),move|gateway|{
                gateway.mutate_excel_workflow(
                    revision,&action,&shipment_id,&queue_id,&origin,value
                )
            });
        }
        if self.preview.is_some(){
            let response=self.preview.as_ref().unwrap().clone();
            let missing=response["sender"]["companyMissing"]==true;
            let count=rows(&response["preview"]["rows"]).len();
            let ids=rows(&response["preview"]["ids"]).iter()
                .filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();
            let origin=self.tab.origin().to_owned();
            components::modal_backdrop(ui.ctx(),"excel-output-backdrop",false);
            components::decision_window(
                "Excel 작성 확인","excel-export-confirm",ui.max_rect()
            ).show(ui.ctx(),|dialog|{
                dialog.label(typography::body(&format!(
                    "{} {}건의 로젠택배 Excel을 작성할까요?",
                    if origin=="FACTORY"{"공장 발송"}else{"매장 발송"},count
                )));
                dialog.label(typography::caption(
                    "Excel 파일 작성은 실제 포장 또는 발송 완료가 아닙니다."
                ));
                if missing {
                    dialog.label(typography::status(
                        "송하인 정보가 미완성입니다. 설정을 확인하세요.",theme::DANGER
                    ));
                    if theme::action(dialog,"반도스탠드 정보 설정",
                        theme::ButtonRole::Standard,true).clicked(){
                        go_settings=true;
                    }
                }
                dialog.horizontal(|bar|{
                    if theme::action(bar,"취소",theme::ButtonRole::Standard,true).clicked(){
                        self.preview=None;
                    }
                    if theme::action(bar,"Excel 작성",theme::ButtonRole::Primary,
                        !busy&&self.gateway.excel_write_enabled()).clicked(){
                        self.preview=None;
                        let folder_after=origin=="FACTORY";
                        self.task("export",ui.ctx().clone(),move|g|{
                            let mut result=g.export_excel(&origin,&ids)?;
                            let projection=g.execute(ReadOperation::ReadShipments,
                                json!({"state":&result["state"]}))?;
                            result["shipmentReads"]=projection;
                            if folder_after{
                                if let Some(export_id)=result["exportId"].as_str(){
                                    // Explorer failure is NOT an export failure.
                                    if let Err(reason)=g.open_excel_folder(Some(export_id),None){
                                        result["folderWarning"]=json!(reason);
                                    }
                                }
                            }
                            Ok(result)
                        });
                    }
                });
            });
        }
        if go_settings{self.open_settings=true;}
        crate::floating_notice::sync(
            ui.ctx(),"excel.result","excel.action",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"excel.error","excel.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
    }
}

#[cfg(test)]
mod table_state_tests {
    use super::*;

    #[test]
    fn history_table_has_only_file_actions_and_no_related_order_panel() {
        // The archive lists files; it does not mutate or navigate orders.
        // Source-level guard is intentional: this regression was an inline
        // panel and mutable workflow action, not a data-contract change.
        let source=include_str!("excel_manager.rs");
        let start=source.find("let mut history=state.map")
            .expect("history table must still exist");
        let end=start+source[start..]
            .find("}else{\n                let all=!visible.is_empty()")
            .expect("history branch must end before queue actions");
        let history=&source[start..end];
        assert!(history.contains("ExcelTableHeightPolicy::FillCard{card_min_height}"));
        assert!(history.contains("\"파일 열기\""));
        assert!(history.contains("\"경로 복사\""));
        for obsolete in [
            concat!("관련", " 주문"),
            concat!("주문", " 상세 보기"),
            concat!("실제 공장", " 전달 확인"),
            concat!("전달 전 정정", " / 재출력"),
            concat!("매장 발송", " 확인"),
            concat!("history", "_detail"),
            concat!("pending", "_confirmation"),
            "LegacyForExpandedDetails",
        ] {
            assert!(!history.contains(obsolete),
                "Excel export history must not expose: {obsolete}");
        }
        assert!(!source.contains(concat!("struct Pending", "Confirmation")));
        assert!(!source.contains(concat!("self.pending", "_confirmation")));
        // Queue-level details remain a separate, unchanged workflow.
        assert!(source.contains("ExcelTableHeightPolicy::LegacyForExpandedDetails"));
        assert!(source.contains("pub fn take_open_order"));
    }

    #[test]
    fn loading_empty_and_failed_states_never_create_an_inline_footer() {
        // The only output is content for the existing table-body painter.
        assert_eq!(
            table_state_message(Tab::Factory,"",0,true,false),
            Some("Excel 자료를 확인 중입니다.")
        );
        assert_eq!(
            table_state_message(Tab::Store,"악기",0,false,false),
            Some("검색 조건에 맞는 발송 대기 건이 없습니다.")
        );
        assert_eq!(
            table_state_message(Tab::History,"",0,false,false),
            Some("Excel 작성 이력이 없습니다.")
        );
        assert_eq!(
            table_state_message(Tab::History,"관리",0,false,false),
            Some("검색 조건에 맞는 Excel 작성 이력이 없습니다.")
        );
        assert_eq!(table_state_message(Tab::Factory,"",0,false,true),None);
        assert_eq!(table_state_message(Tab::History,"",0,false,true),None);
        assert_eq!(table_state_message(Tab::Factory,"",3,true,false),None);
        assert_eq!(table_state_message(Tab::History,"",3,false,false),None);
    }

    #[test]
    fn empty_status_centers_inside_table_body_not_between_toolbar_and_header() {
        let track=Rect::from_min_size(
            egui::pos2(32.0,136.0),Vec2::new(600.0,400.0)
        );
        let clip=Rect::from_min_max(
            egui::pos2(16.0,30.0),egui::pos2(632.0,536.0)
        );
        let body=table_state_body_rect(track,600.0,10.0,400.0,clip)
            .expect("visible body");
        assert!((body.top()-136.0).abs()<0.01);
        assert!((body.right()-622.0).abs()<0.01);
        assert!((body.center().y-336.0).abs()<0.01);
        assert!(body.top()>100.0,"body caption cannot shift toolbar/header");
    }

    #[test]
    fn empty_status_is_clipped_inside_actual_card_and_can_be_absent() {
        let track=Rect::from_min_size(
            egui::pos2(32.0,136.0),Vec2::new(600.0,400.0)
        );
        let clipped=Rect::from_min_max(
            egui::pos2(32.0,150.0),egui::pos2(420.0,260.0)
        );
        let visible=table_state_body_rect(track,600.0,10.0,400.0,clipped)
            .expect("clipped body");
        assert_eq!(visible,clipped);
        let outside=Rect::from_min_max(
            egui::pos2(0.0,0.0),egui::pos2(10.0,10.0)
        );
        assert!(table_state_body_rect(track,600.0,10.0,400.0,outside).is_none());
    }
}
