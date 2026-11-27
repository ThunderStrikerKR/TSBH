//! React WorkflowQueue's Excel management view, now native egui.
//! Export, sender validation, workflow state and file history remain owned by
//! the original Rust business core and Tauri's existing SQLite database.
use crate::ui_layout::geometry::SharedAllocationExt as _;
use crate::{gateway::{BandoGateway,ReadOperation},order_history_manager,settings_help,theme,typography};
use chrono::{DateTime,FixedOffset,NaiveDate,Utc};
use eframe::egui::{self,Align,Align2,Rect,Sense,Ui,Vec2};
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
    v.as_i64().map(crate::display_format::display_won)
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
/// The Excel archive opens on the current Seoul calendar day, with an
/// inclusive start/end range. "전체 날짜" can still clear both values.
fn default_export_date_range(today:NaiveDate)->(String,String){
    let day=today.format("%Y-%m-%d").to_string();
    (day.clone(),day)
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
fn apply_valid_export_date_range(
    from:&mut String,to:&mut String,from_input:&str,to_input:&str
)->bool {
    if order_history_manager::date_range_error(from_input,to_input).is_some(){
        return false;
    }
    let changed=from.as_str()!=from_input||to.as_str()!=to_input;
    if changed{
        *from=from_input.to_owned();
        *to=to_input.to_owned();
    }
    changed
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
    crate::table::paint_management_text_cell(
        ui,padded,visible,visible,role,align,("excel-table-cell",id),false
    );
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
    ready:bool,scroll_keys:&dyn Fn(usize)->Option<egui::Id>,
    height_policy:ExcelTableHeightPolicy,
    sort:&mut Option<Sort>,sort_enabled:[bool;N],
    header_selection:Option<(bool,bool)>,
    empty_message:Option<&str>,
    mut render:impl FnMut(&mut Ui,[Rect;N],usize)
)->Option<bool>{
    let old_gap=ui.spacing().item_spacing.y;
    crate::ui_layout::apply_spacing_role(ui,crate::ui_layout::SpacingRole::ExplicitVertical);
    let minimum=base.iter().sum::<f32>()+(N-1) as f32*GAP;
    // The toolbar and 36pt header always share the two-master 16+1+4
    // transition. No status or error line may change their coordinates.
    crate::table::show(ui,crate::table::Header::Sticky(minimum),|ui,layout|{
    let width=layout.table_width;
    let widths=theme::management_single_stretch_columns(base,width,GAP,stretch);
    let header=layout.header_rect;
    let header_content=layout.header_content_rect;
    let scroll_max_height=match height_policy {
        ExcelTableHeightPolicy::FillCard{card_min_height}=>
            crate::scrollbar::management_single_list_scroll_height(
                ui,card_min_height,0.0
            ),
        ExcelTableHeightPolicy::LegacyForExpandedDetails=>
            ui.ctx().content_rect().height()*theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION,
    };
    // No 36pt empty-state footer: the caption is painted inside the body.
    // Rail extension still uses the same shared master scroll policy.
    let scroll_track=crate::table::management_table_scroll_track_rect(
        ui,scroll_max_height
    );
    let mut left=None;
    let output=crate::table::fixed_rows_snapshot(
                ui,crate::table::Viewport{width:width,height:scroll_max_height,rail:Some(scroll_track)},
                id,HEIGHT,rows,ready,Some(scroll_keys),
                |body,range|{
            let first=range.start;
            let origin=body.max_rect().min;
            for index in range{
                let r=crate::table::fixed_row_rect(origin,index,first,width,HEIGHT);
                #[cfg(test)]
                crate::ui_test_support::record("data row",r);
                left.get_or_insert(r.left());
                crate::table::fixed_row(body,(id,index),r,index,&widths,GAP,crate::table::RowTone::Alternating,|row,cols|{
                        render(row,cols,index);
                    
                });
            }
        });
    crate::table::restore_management_table_spacing(ui,old_gap);
    let left=left.unwrap_or(header.left()-output.state.offset.x);
    let heads=crate::table::management_table_cells(egui::pos2(left,header.top()),&widths,HEADER,GAP);
    let (header_text_rect,_)=crate::table::header_columns(ui,header,header_content,&heads,GAP,|ui,header_text_rect|{
    let mut clicked=None;
    for (i,title) in titles.iter().enumerate(){
        let h=heads[i];
        let title=if i==0&&header_selection.is_some(){""}else{title};
        let label=typography::management_table_header_galley(ui,title);
        let arrow=if sort_enabled[i]{
            let (glyph,color)=match (*sort).filter(|s|s.column==i)
                .map(|s|s.direction){
                Some(Direction::Up)=>("▲",theme::icon_ink(theme::IconRole::SortIndicator,theme::VisualState::Selected)),
                Some(Direction::Down)=>("▼",theme::icon_ink(theme::IconRole::SortIndicator,theme::VisualState::Selected)),
                None=>("↕",theme::icon_ink(theme::IconRole::SortIndicator,theme::VisualState::Normal))
            };
            Some((typography::management_table_header_galley(ui,glyph),color))
        }else{None};
        let alignment=crate::table::heading_alignment(align[i],theme::MANAGEMENT_TABLE_TEXT_LEADING_INSET,theme::SPACE_8);
        crate::table::heading(ui,h,header_text_rect,h.intersect(header_text_rect),alignment,label,arrow);
        if sort_enabled[i]{
            let hit=h.intersect(header_text_rect);
            if hit.width()>0.0&&hit.height()>0.0{
                if crate::table::sort_clicked(ui,hit,(id,"sort",i)){
                    clicked=Some(Sort::next(*sort,i));
                }
            }
        }
    }
    if let Some(next)=clicked{*sort=next;}
    // Paint the very same column separators as the data rows.
    
    
                ()
            });
    
    if let Some((all,enabled))=header_selection{
        let mut checked=all;
        if crate::checkbox::management_table_checkbox(
            ui,heads[0],header_text_rect,&mut checked,enabled,
            (id,"select-all")
        ).changed(){
            return (output.inner_rect,Some(checked));
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
            crate::status_notice::management_body_state_overlay(ui,visible,Some(message));
        }
    }

            (output.inner_rect,None)
        })
}
#[derive(Default)]
struct Snapshot{state:Value,shipments:Value}
pub struct ExcelManager{
    gateway:Arc<dyn BandoGateway>,
    refresh_job:crate::screen_refresh::ReadRefresh<Snapshot>,
    result:Option<Snapshot>,
    queue_cache:crate::screen_refresh::ViewProjectionCache<(Tab,String,Option<Sort>),(Vec<QueueRow>,BTreeSet<String>)>,
    history_cache:crate::screen_refresh::ViewProjectionCache<(String,String,String,Option<Sort>),Vec<HistoryRow>>,
    operation:Option<Receiver<Result<Value,String>>>,
    operation_name:Option<String>,
    error:Option<String>,
    notice:Option<String>,
    tab:Tab,
    tab_snapshots:crate::view_state::NavigationSnapshots<3>,
    tab_filters:[(String,Option<Sort>,BTreeSet<String>);3],
    query:String,
    selected:BTreeSet<String>,
    sort_queue:Option<Sort>,
    sort_history:Option<Sort>,
    export_from:String,
    export_to:String,
    export_from_input:String,
    export_to_input:String,
    export_from_month:NaiveDate,
    export_to_month:NaiveDate,
    managed_queue:Option<String>,
    freight_drafts:HashMap<String,String>,
    open_order:Option<String>,
    reply_review:bool,
    reply_confirm:Option<Value>,
    reply_changed:bool,
    reply_refresh_pending:bool,
    // Retain verified rows for display, but never write using a stale revision.
    reply_rows:Vec<Value>,
    reply_rows_revision:Option<u64>,
    reply_page:usize,
}
impl ExcelManager{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let today=Utc::now().with_timezone(
            &FixedOffset::east_opt(9*3600).expect("Seoul offset")
        ).date_naive();
        let (export_from,export_to)=default_export_date_range(today);
        let mut manager=Self{
            gateway,refresh_job:Default::default(),result:None,queue_cache:Default::default(),history_cache:Default::default(),operation:None,operation_name:None,
            error:None,notice:None,tab:Tab::Factory,
            tab_snapshots:crate::view_state::NavigationSnapshots::new(0),tab_filters:Default::default(),
            query:String::new(),selected:BTreeSet::new(),
            sort_queue:None,sort_history:None,
            export_from:export_from.clone(),export_to:export_to.clone(),
            export_from_input:export_from,export_to_input:export_to,
            export_from_month:crate::navigation::calendar_month_start(today),
            export_to_month:crate::navigation::calendar_month_start(today),
            managed_queue:None,freight_drafts:HashMap::new(),
            open_order:None,
            reply_review:false,reply_confirm:None,reply_changed:false,reply_refresh_pending:false,
            reply_rows:Vec::new(),reply_rows_revision:None,reply_page:0,
        };
        manager.refresh(ctx);manager
    }
    pub(crate) fn suspend_navigation(&mut self,now:std::time::Instant){self.tab_snapshots.suspend(now);}
    fn sync_tab_navigation(&mut self){
        let next=self.tab as usize;
        let previous=self.tab_snapshots.active();
        if next!=previous{
            let mut current=(std::mem::take(&mut self.query),self.sort_queue.take(),std::mem::take(&mut self.selected));
            crate::view_state::exchange_tab(&mut self.tab_filters,&mut current,previous,next);
            (self.query,self.sort_queue,self.selected)=current;
            // An actual-freight edit panel is not a read-only navigation snapshot.
            self.managed_queue=None;
        }
        if !self.tab_snapshots.visit(next,std::time::Instant::now()){return;}
        self.query.clear();self.sort_queue=None;self.selected.clear();self.managed_queue=None;
        if self.tab==Tab::History{
            let today=Utc::now().with_timezone(&FixedOffset::east_opt(9*3600).expect("Seoul offset")).date_naive();
            let (from,to)=default_export_date_range(today);
            self.export_from=from.clone();self.export_to=to.clone();
            self.export_from_input=from;self.export_to_input=to;
            self.export_from_month=crate::navigation::calendar_month_start(today);
            self.export_to_month=self.export_from_month;self.sort_history=None;
        }
    }
    pub(crate) fn reset_navigation(&mut self){
        if self.operation.is_some()||self.blocks_reply_navigation(){return;}
        // Unsaved freight inputs are not a navigation snapshot.
        let today=Utc::now().with_timezone(&FixedOffset::east_opt(9*3600).expect("Seoul offset")).date_naive();
        let (from,to)=default_export_date_range(today);
        self.tab=Tab::Factory;self.tab_snapshots=crate::view_state::NavigationSnapshots::new(0);self.tab_filters=Default::default();
        self.query.clear();self.selected.clear();self.sort_queue=None;self.sort_history=None;
        self.managed_queue=None;
        self.export_from=from.clone();self.export_to=to.clone();
        self.export_from_input=from;self.export_to_input=to;
        self.export_from_month=crate::navigation::calendar_month_start(today);
        self.export_to_month=self.export_from_month;
        self.reply_page=0;
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
    pub fn request_reply_refresh(&mut self){self.reply_refresh_pending=true;}
    pub fn take_reply_changed(&mut self)->bool{std::mem::take(&mut self.reply_changed)}
    pub fn blocks_reply_navigation(&self)->bool{self.reply_review||self.reply_confirm.is_some()||
        self.operation_name.as_deref()==Some("reply-correction")}
    pub fn refresh(&mut self,ctx:egui::Context){
        if self.operation.is_some(){return}
        // Any read started after the notification satisfies the pending
        // inbox refresh, including the existing page-entry read.
        self.reply_refresh_pending=false;
        self.error=None;
        let gateway=Arc::clone(&self.gateway);
        self.refresh_job.submit_required(ctx,move||{
            let state=gateway.load_business_state()?;
            let shipments=gateway.execute(ReadOperation::ReadShipments,json!({"state":&state}))?;
            Ok(Snapshot{state,shipments})
        });
    }
    pub fn poll(&mut self,ctx:&egui::Context){
        match self.refresh_job.poll(){
            Some(Ok(value))=>{
                self.refresh_job.publish(&mut self.result,value);
                self.queue_cache.invalidate();self.history_cache.invalidate();
                self.error=None;
            }
            Some(Err(message))=>self.error=Some(message),
            None=>{}
        }
        let write=self.operation.as_ref().map(Receiver::try_recv);
        match write{
            Some(Ok(Ok(result)))=>{
                self.operation=None;
                let operation=self.operation_name.take().unwrap_or_default();
                if operation=="workflow"{
                    let action=text(&result,"action");
                    if action=="record_actual_freight"{
                        self.freight_drafts.remove(text(&result,"queueId"));
                    }
                    self.notice=Some(match action{
                        "restore_shipment"=>"배송 건을 처리 중 주문으로 복구했습니다.",
                        "mark_store_sent"=>"매장 발송을 확인했습니다.",
                        _=>"Excel 관리 변경사항을 저장했습니다.",
                    }.into());
                    self.queue_cache.invalidate();self.history_cache.invalidate();
                    self.result=Some(Snapshot{
                        state:result["state"].clone(),
                        shipments:result["shipmentReads"].clone()
                    });
                    self.refresh_job.verified();
                    // The list reconciles only IDs that actually left the queue.
                }else if operation=="reply-correction"{
                    self.accept_reply_correction(result);
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
                    // A committed export must not be reported as failed due to a later read.
                    self.refresh_job.cancel();
                    if result.get("shipmentReads").is_some() && result["state"]["revision"].as_u64().is_some(){
                        self.queue_cache.invalidate();self.history_cache.invalidate();
                    self.result=Some(Snapshot{
                            state:result["state"].clone(),
                            shipments:result["shipmentReads"].clone()
                        });
                        self.refresh_job.verified();
                    }else{
                        self.refresh_job.require();
                        self.reply_refresh_pending=true;
                        self.error=result["projectionError"].as_str().map(str::to_owned);
                    }
                    if result["folderOpened"]==true{
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
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
    fn accept_reply_correction(&mut self,mut result:Value){
        self.reply_confirm=None;
        self.reply_changed=true;
        self.refresh_job.cancel();
        self.notice=Some("공장 회신 정정 내용을 저장했습니다. 기존 원본 및 이력은 유지됩니다.".into());
        if result.get("shipmentReads").is_some() && result["state"]["revision"].as_u64().is_some(){
            self.queue_cache.invalidate();self.history_cache.invalidate();
                    self.result=Some(Snapshot{
                state:result["state"].take(),
                shipments:result["shipmentReads"].take(),
            });
            self.refresh_job.verified();
            self.error=None;
            // Do not clear an independent inbox notification that arrived
            // while the correction worker was running.
        }else{
            // A projection error cannot undo an already committed reply.
            // Keep the success notification and fall back to a fresh read.
            self.refresh_job.require();
            self.reply_refresh_pending=true;
            self.error=result["projectionError"].as_str().map(str::to_owned);
        }
    }

    fn task(&mut self,action:&str,ctx:egui::Context,
        work:impl FnOnce(Arc<dyn BandoGateway>)->Result<Value,String>+Send+'static){
        if self.operation.is_some()||self.refresh_job.pending()||
            (self.refresh_job.required()&&!matches!(action,"file"|"folder")){return}
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
    fn start_export(&mut self,ctx:egui::Context,origin:String,ids:Vec<String>){
        self.task("export",ctx,move|g|{
            let mut result=g.export_excel(&origin,&ids)?;
            // A failed follow-up read cannot undo a committed file and history.
            match g.execute(ReadOperation::ReadShipments,json!({"state":&result["state"]})){
                Ok(projection)=>result["shipmentReads"]=projection,
                Err(reason)=>result["projectionError"]=json!(reason),
            }
            // Both origins open the recorded output directory.
            // Keep the app visible if the folder could not be opened.
            let folder=match result["exportId"].as_str(){
                Some(export_id)=>g.open_excel_folder(Some(export_id),None),
                None=>Err("저장된 Excel 출력 이력 ID가 없습니다.".into()),
            };
            match folder{
                Ok(())=>result["folderOpened"]=json!(true),
                Err(reason)=>result["folderWarning"]=json!(reason),
            }
            Ok(result)
        });
    }
    /// Review only unresolved rows from the authoritative Rust review projector.
    /// Refresh the projection once per DB revision, never on every painted row.
    fn update_reply_review(&mut self){
        let Some(snapshot)=self.result.as_ref() else{return};
        let revision=snapshot.state["revision"].as_u64();
        if revision.is_some()&&self.reply_rows_revision==revision{return}
        let replies=&snapshot.state["factoryReplies"];
        let result=crate::gateway::rules::execute("factory_reply_review_statuses",
            json!({"state":{"factoryReplies":replies}}));
        let Ok(statuses)=result else{return};
        let mut held=Vec::new();
        for record in rows(replies){
            let Some(status)=find(&statuses["reviews"],text(record,"id"))else{continue};
            for row in rows(&record["rows"]){
                if row["applied"]==true||rows(&status["resolvedRows"]).iter()
                    .any(|r|r["row"]==row["row"]&&r["resolved"]==true){continue}
                let candidates=if text(row,"issue").starts_with("배송 건이 여러 개입니다."){
                    crate::gateway::rules::execute("factory_reply_candidates",json!({
                        "state":&snapshot.state,"id":record["id"],"rowNumber":row["row"]
                    })).ok().and_then(|r|r.get("candidates").cloned()).unwrap_or(json!([]))
                }else{json!([])};
                held.push(json!({"candidates":candidates,"id":record["id"],"filename":record["filename"],
                    "row":row["row"],"name":row["name"],"issue":row["issue"],
                    "tracking":row["tracking"],"actual":row["actual"],"shipmentId":row["shipmentId"]}));
            }
        }
        self.reply_rows=held;
        self.reply_rows_revision=revision;
        self.reply_page=self.reply_page.min(self.reply_rows.len().saturating_sub(1)/50);
    }
    pub fn show_reply_review(&mut self,ctx:&egui::Context,area:Rect){
        if !self.reply_review{return}
        self.update_reply_review();
        let busy=self.operation.is_some()||self.refresh_job.pending();
        let writable_snapshot=!busy&&!self.refresh_job.required();
        let mut close=false;let mut selected:Option<Value>=None;
        let count=self.reply_rows.len();
        crate::popup::modal_backdrop(ctx,"factory-reply-review-backdrop",false);
        crate::popup::modal_window(ctx,"공장 회신 검토","factory-reply-review",area)
            .default_width(area.width().min(900.0)).show(ctx,|ui|{
                crate::popup::modal_flow_workspace(ui,|workspace|{
                    close=workspace.header("공장 회신 검토","닫기",!busy&&self.reply_confirm.is_none());
                    if self.reply_confirm.is_some(){workspace.disable();}
                    workspace.scroll_region("factory-reply-review-body",
                        crate::popup::modal_editor_body_max_height(area.height()),|ui|{
                        ui.label(typography::field_label(&format!("미해결 회신 {count}행")));
                        ui.label(typography::caption(crate::native_workbooks::FACTORY_REPLY_FORMAT_GUIDANCE));
                        ui.label(typography::caption("정상 회신은 자동 반영됩니다. 연결할 배송 건이 여러 개일 때만 대상을 확인하세요."));
                        if count==0{crate::ui_primitives::management_body_state_inline(ui,if busy{"조회 중"}else{"검토할 회신이 없습니다."});}
                        for row in self.reply_rows.iter().skip(self.reply_page*50).take(50){
                            crate::ui_primitives::soft_divider(ui);
                            ui.horizontal_wrapped(|line|{
                                line.label(typography::field_label(&format!("{} / {}행 / {}",
                                    text(row,"filename"),row["row"],text(row,"name"))));
                                // Legacy conflicts alone expose the action; reserve its
                                // geometry even for rows without an actionable conflict.
                                let legacy_conflict=text(row,"issue").contains("충돌")&&!text(row,"shipmentId").is_empty();
                                line.add_visible_ui(legacy_conflict,|slot|{
                                    if crate::button::action(slot,"충돌 검토",theme::ButtonRole::Standard,
                                        legacy_conflict&&writable_snapshot&&self.gateway.excel_write_enabled()).clicked(){selected=Some(row.clone());}
                                });
                            });
                            ui.label(typography::table_cell(&format!("송장 {} / 실제 운임 {} / {}",
                                text(row,"tracking"),amount(&row["actual"]),text(row,"issue").replace("직원 확인 필요","확인 필요"))));
                            for candidate in rows(&row["candidates"]){
                                let row_number=candidate["exportRow"].as_u64().map(|n|n.to_string())
                                    .unwrap_or_else(||"미확인".into());
                                ui.horizontal_wrapped(|line|{
                                    line.label(typography::table_cell(&format!("{} / {}행 / {}",
                                        text(candidate,"filename"),row_number,text(candidate,"exportAt"))));
                                    if crate::button::action(line,"이 배송 건 선택",theme::ButtonRole::Standard,
                                        writable_snapshot&&self.gateway.excel_write_enabled()&&candidate["eligible"]==true).clicked(){
                                        let mut chosen=row.clone();
                                        chosen["selectedShipmentId"]=candidate["shipmentId"].clone();
                                        selected=Some(chosen);
                                    }
                                });
                                ui.label(typography::table_cell(&format!("품목: {} / 기존 송장: {} / 실제 운임: {}",
                                    text(candidate,"items"),text(candidate,"tracking"),amount(&candidate["actual"]))));
                                if candidate["eligible"]!=true{
                                    ui.label(typography::caption("기존 회신 또는 배송 상태로 인해 연결할 수 없습니다."));
                                }
                            }
                        }
                        ui.horizontal(|bar|{
                            if crate::button::action(bar,"이전",theme::ButtonRole::Standard,!busy&&self.reply_page>0).clicked(){self.reply_page-=1;}
                            bar.label(typography::caption(&format!("{}/{}",self.reply_page+1,(count.saturating_add(49)/50).max(1))));
                            if crate::button::action(bar,"다음",theme::ButtonRole::Standard,!busy&&(self.reply_page+1)*50<count).clicked(){self.reply_page+=1;}
                            let error=self.error.as_deref();
                            let error_width=bar.available_width();
                            crate::ui_layout::fixed_status_slot(bar,error_width,
                                typography::METADATA_LINE_HEIGHT,error,error,theme::DANGER);
                        });
                    });
                    workspace.footer_single(|bar|{if crate::button::action(bar,"닫기",theme::ButtonRole::Standard,!busy&&self.reply_confirm.is_none()).clicked(){close=true;}});
                });
            });
        if close{self.reply_review=false;self.reply_confirm=None;return}
        if let (Some(row),Some(snapshot))=(selected,self.result.as_ref()){
            match crate::gateway::rules::execute("factory_conflict_confirmation",json!({
                "state":&snapshot.state,"id":row["id"],"rowNumber":row["row"],
                "selectedShipmentId":row["selectedShipmentId"]})){
                Ok(confirm)=>{self.reply_confirm=Some(json!({"revision":snapshot.state["revision"],
                    "id":row["id"],"rowNumber":row["row"],"selectedShipmentId":row["selectedShipmentId"],
                    "confirmedText":confirm["text"]}));},
                Err(e)=>self.error=Some(e),
            }
        }
        if let Some(review)=self.reply_confirm.clone(){
            crate::popup::modal_backdrop(ctx,"factory-reply-confirm-backdrop",true);
            let mut cancel=false;let mut approve=false;
            crate::popup::show_decision_window(ctx,"공장 회신 한 행 정정","factory-reply-confirm",area,|dialog|{
                crate::popup::decision_workspace(dialog,|decision|{
                    cancel=decision.header("공장 회신 한 행 정정","취소",!busy);
                    decision.body(|body|{body.label(typography::table_cell(text(&review,"confirmedText")));});
                    decision.actions(|bar|{
                        if crate::button::action(bar,"정정 반영",theme::ButtonRole::Primary,
                            writable_snapshot&&self.gateway.excel_write_enabled()).clicked(){approve=true;}
                    });
                });
            });
            if cancel{self.reply_confirm=None;}
            if approve{
                self.reply_confirm=None;
                self.task("reply-correction",ctx.clone(),move|gateway|{
                    if !gateway.excel_write_enabled(){return Err("공장 회신 저장 권한 없음".into())}
                    let state=crate::factory_reply_inbox::approve(review)?;
                    let projection=gateway.execute(ReadOperation::ReadShipments,json!({"state":&state}));
                    // The transaction has committed. A failed view projection
                    // must not be reported as a failed correction mutation.
                    Ok(match projection{
                        Ok(shipments)=>json!({"state":state,"shipmentReads":shipments}),
                        Err(error)=>json!({"projectionError":error}),
                    })
                });
            }
        }
    }

    fn queue_projection(&mut self)->std::sync::Arc<(Vec<QueueRow>,BTreeSet<String>)>{
        self.queue_cache.get((self.tab,self.query.clone(),self.sort_queue),||{
            let mut queued=if self.tab==Tab::History{Vec::new()}
            else if let Some(snapshot)=self.result.as_ref(){
                records(&snapshot.state,&snapshot.shipments,self.tab,&self.query)
            }else{Vec::new()};
            sort_queue(&mut queued,self.sort_queue);
            let visible=queued.iter().filter(|r|!r.legacy).map(|r|r.id().to_owned()).collect();
            (queued,visible)
        })
    }
    pub fn show_page(&mut self,ui:&mut Ui){
        // Also consume worker results when this page is hosted independently.
        self.poll(ui.ctx());
        let page_origin_y=ui.cursor().top();
        crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Standard);
        if self.reply_refresh_pending&&self.operation.is_none()&&!self.refresh_job.pending(){
            self.refresh(ui.ctx().clone());
        }
        crate::floating_notice::sync(
            ui.ctx(),"excel.result","excel.action",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"excel.error","excel.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        crate::ui_layout::management_page_title_top_gap(ui);
        crate::ui_layout::management_top_content(ui,|top|{
            top.horizontal(|title|{
                title.label(typography::page_heading("엑셀 관리"));
                settings_help::icon(title,"발송 Excel 관리");
                title.with_layout(egui::Layout::right_to_left(egui::Align::Center),|actions|{
                    if crate::button::action(actions,"공장 회신 검토",theme::ButtonRole::Standard,
                        self.result.is_some()&&self.operation.is_none()
                    ).clicked(){self.reply_review=true;self.reply_rows_revision=None;self.reply_page=0;}
                });
            });
        });
        ui.add_space(theme::SPACE_8);
        let busy=self.operation.is_some()||self.refresh_job.pending();
        let writable_snapshot=!busy&&!self.refresh_job.required();
        let next_tab=crate::ui_layout::management_top_content(ui,|top|
            crate::button::management_tabs(top,&[
                ("공장 엑셀 대기",self.tab==Tab::Factory,!busy),
                ("매장 발송 대기",self.tab==Tab::Store,!busy),
                ("엑셀 작성 이력",self.tab==Tab::History,!busy),
            ])
        );
        if let Some(next)=next_tab{
            let selected=Tab::ALL[next];
            if selected!=self.tab{
                self.tab=selected;
            }
        }
        ui.add_space(theme::SPACE_12);
        self.sync_tab_navigation();
        ui.push_id(("excel-navigation",self.tab as usize,self.tab_snapshots.epoch(self.tab as usize)),|ui|{
        let mut exported=None;
        let mut open_file=None;
        let mut open_daily=false;
        let mut workflow_action:Option<(String,String,String,String,Value)>=None;
        let mut manage_clicked=None;
        let mut open_order=None;
        let mut copy_path=None;
        // Resolve the currently selectable queue BEFORE allocating the fixed
        // toolbar: its right-hand action caption and enabled state must use
        // the same visible selection as the table below.
        let mut queued=self.queue_projection();
        if self.tab!=Tab::History{
            let verified=self.result.is_some()&&!self.refresh_job.required();
            crate::view_state::retain_selection(&mut self.selected,&queued.1,verified);
            crate::view_state::retain_target(&mut self.managed_queue,|id|queued.1.contains(id),verified);
        }
        crate::card::align_management_primary_card(ui,page_origin_y);
        // Exact same 16pt interior inset and frame as the approved master.
        let card_min_height=crate::card::management_master_table_card_min_inner_height(ui);
        crate::card::measured_management_master_card(ui,|section|{
            crate::card::fill_management_primary_card(section,card_min_height);
            // Allocate a true 36pt master toolbar. Search and trailing
            // actions use the SAME 16pt card interior width as the header.
            crate::table::management_table_container_style(section);
            let toolbar_width=section.available_width().max(1.0);
            let has_actions=self.tab!=Tab::History;
            let history_tab=self.tab==Tab::History;
            let history_range_width=crate::navigation::EXCEL_EXPORT_DATE_CONTROLS_WIDTH;
            let history_one_row=history_tab&&toolbar_width>=
                theme::MANAGEMENT_SEARCH_MIN_WIDTH+theme::SPACE_12
                    +history_range_width;
            let history_compact=history_tab&&toolbar_width<history_range_width;
            let stacked=has_actions&&toolbar_width
                <theme::MANAGEMENT_SEARCH_MIN_WIDTH+theme::SPACE_12
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
            let (toolbar_rect,_)=section.shared_allocate_exact_size(
                Vec2::new(toolbar_width,toolbar_height),Sense::hover()
            );
            let search_width=if history_tab{
                // The Order History reference uses a fixed 280pt search
                // control even when the date group wraps onto a new row.
                theme::MANAGEMENT_SEARCH_MIN_WIDTH.min(toolbar_width)
            }else if stacked{
                toolbar_width
            }else{
                (toolbar_width-theme::MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH
                    -theme::SPACE_12).max(1.0)
            };
            let search_rect=Rect::from_min_size(
                toolbar_rect.min,Vec2::new(search_width,theme::INPUT_HEIGHT)
            );
            crate::ui_layout::management_toolbar_slot(section,"excel-master-search",
                search_rect,egui::Layout::left_to_right(Align::Center),true,false,|search|{
                    if crate::input::management_search_field(
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
                            queued=self.queue_projection();
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
                let dates_rect=crate::ui_layout::management_toolbar_trailing_rect(
                    toolbar_rect,history_range_width.min(toolbar_width),
                    if history_one_row{0.0}else{
                        theme::INPUT_HEIGHT+theme::SPACE_8
                    },date_height
                );
                crate::ui_layout::management_toolbar_slot(section,"excel-export-dates",
                    dates_rect,egui::Layout::left_to_right(Align::Center).with_main_align(Align::Max),
                    false,true,|dates|{
                        if crate::navigation::excel_export_date_controls(
                            dates,&mut self.export_from_input,
                            &mut self.export_to_input,
                            &mut self.export_from_month,&mut self.export_to_month,
                            history_compact
                        ){
                            crate::floating_notice::inherit_origin(dates.ctx(),"history.date","excel.history-dates");
                            apply_valid_export_date_range(
                                &mut self.export_from,&mut self.export_to,
                                &self.export_from_input,&self.export_to_input
                            );
                        }
                    }
                );
                let range_error=order_history_manager::date_range_error(
                    &self.export_from_input,&self.export_to_input
                );
                crate::floating_notice::sync(
                    section.ctx(),"excel.export-dates","excel.history-dates",
                    crate::floating_notice::Tone::Error,range_error
                );
            }else{
                // Closing the archive tab also closes any invalid-date
                // notice; it must not leak into factory/store queue tabs.
                crate::floating_notice::sync(
                    section.ctx(),"excel.export-dates","excel.history-dates",
                    crate::floating_notice::Tone::Error,None
                );
            }
            if has_actions{
                let actions_y=if stacked{
                    theme::INPUT_HEIGHT+theme::SPACE_8
                }else{0.0};
                let actions_rect=crate::ui_layout::management_toolbar_trailing_rect(
                    toolbar_rect,
                    theme::MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH.min(toolbar_width),
                    actions_y,theme::INPUT_HEIGHT
                );
                crate::ui_layout::management_toolbar_slot(section,"excel-master-actions",
                    actions_rect,egui::Layout::right_to_left(Align::Center),true,false,|actions|{
                        if crate::button::action(actions,"폴더 열기",
                            theme::ButtonRole::Standard,!busy).clicked(){
                            open_daily=true;
                        }
                        if crate::button::action(actions,
                            &format!("선택 {}건 {} Excel 작성",
                                self.selected.len(),
                                if self.tab==Tab::Factory{"공장"}else{"매장"}),
                            theme::ButtonRole::Primary,
                            !self.selected.is_empty()&&writable_snapshot
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
                let order_list=self.history_cache.get((self.query.clone(),self.export_from.clone(),self.export_to.clone(),self.sort_history),||{
                    let mut history=state.map(|s|{
                        export_history(s,&self.query,&self.export_from,&self.export_to)
                    }).unwrap_or_default();
                    sort_history(&mut history,self.sort_history);
                    history
                });
                let indexes=0..order_list.len();
                let empty_message=table_state_message(
                    self.tab,&self.query,order_list.len(),self.refresh_job.pending(),
                    self.error.is_some()
                );
                let scroll_keys=|i:usize|crate::scrollbar::row_key(text(&order_list[i].export,"id"));
                let _=table(section,"excel-history-table",
                    ["작성 일시","거래처","상품 목록","조회"],
                    HISTORY,[Align2::LEFT_CENTER,Align2::LEFT_CENTER,
                        Align2::LEFT_CENTER,Align2::CENTER_CENTER],
                    2,indexes.len(),state.is_some(),&scroll_keys,
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
                        crate::table::control_cell(row,("excel-history-actions",id),cells[3].shrink2(Vec2::new(theme::SPACE_8,0.0)),
                            cells[3],egui::Layout::left_to_right(Align::Center),true,false,|buttons|{
                                if !text(&entry.export,"savedPath").is_empty() {
                                    if crate::button::action(buttons,"파일 열기",
                                        theme::ButtonRole::TableSecondary,!busy).clicked(){
                                        open_file=Some(id.to_owned());
                                    }
                                    if crate::button::action(buttons,"경로 복사",
                                        theme::ButtonRole::TableSecondary,!busy).clicked(){
                                        copy_path=Some(text(&entry.export,"savedPath").to_owned());
                                    }
                                }
                            });
                    });
            }else{
                let visible=&queued.1;
                let all=!visible.is_empty()
                    &&self.selected.len()==visible.len();
                let records=&queued.0;
                // The Excel/export actions are now in the fixed search
                // toolbar. Do not reserve an obsolete bottom footer.
                // Dynamic per-shipment editors still use the conservative cap.
                let queue_height_policy=if self.managed_queue.is_some(){
                    ExcelTableHeightPolicy::LegacyForExpandedDetails
                }else{
                    ExcelTableHeightPolicy::FillCard{card_min_height}
                };
                let empty_message=table_state_message(
                    self.tab,&self.query,records.len(),self.refresh_job.pending(),
                    self.error.is_some()
                );
                let scroll_keys=|i:usize|crate::scrollbar::row_key(records[i].id());
                let toggle_all=table(section,"excel-queue-table",
                    ["선택","거래처 / 발주","수취인 / 배송 품목","금액","처리"],
                    QUEUE,[Align2::CENTER_CENTER,Align2::LEFT_CENTER,
                        Align2::LEFT_CENTER,Align2::RIGHT_CENTER,
                        Align2::CENTER_CENTER],
                    2,records.len(),state.is_some(),&scroll_keys,queue_height_policy,
                    &mut self.sort_queue,
                    [false,true,true,true,false],Some((all,!visible.is_empty())),
                    empty_message,|row,cells,index|{
                        let entry=&records[index];
                        let id=entry.id().to_owned();
                        let mut checked=self.selected.contains(&id);
                        let selectable=!entry.legacy;
                        let checked_response=crate::checkbox::management_table_checkbox(
                            row,cells[0],cells[0],&mut checked,selectable,
                            ("excel-queue-checkbox",id.clone())
                        );
                        if checked_response.changed() && selectable{
                            if checked{self.selected.insert(id.clone());}
                            else{self.selected.remove(&id);}
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
                        crate::table::control_cell(row,("excel-queue-action",id.clone()),cells[4].shrink2(Vec2::new(theme::SPACE_8,0.0)),
                            cells[4],egui::Layout::left_to_right(Align::Center),true,false,|buttons|{
                                if crate::button::action(buttons,"상세 보기",
                                    theme::ButtonRole::TableSecondary,true).clicked(){
                                    open_order=Some(entry.order_id().to_owned());
                                }
                                if selectable && crate::button::action(
                                    buttons,"관리",theme::ButtonRole::TableSecondary,true
                                ).clicked(){
                                    manage_clicked=Some(id.clone());
                                }
                            });
                    }
                );
                if let Some(checked)=toggle_all{
                    if checked{self.selected=visible.clone();}
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
                                crate::ui_primitives::form_field_label(
                                    line,if self.tab==Tab::Factory{
                                        "공장 실제 출고 운임"
                                    }else{"매장 실제 출고 운임"}
                                );
                                crate::input::money_field(
                                    line,draft,writable_snapshot&&self.gateway.excel_write_enabled()
                                );
                                let parsed=if draft.trim().is_empty(){
                                    Some(None)
                                }else{
                                    crate::input::parse_money_input(draft).map(Some)
                                };
                                let dirty=parsed!=Some(saved);
                                if crate::button::action(
                                    line,"실제 운임 저장",
                                    theme::ButtonRole::Primary,
                                    dirty&&writable_snapshot&&self.gateway.excel_write_enabled()
                                ).clicked(){
                                    let value=if draft.trim().is_empty(){
                                        Some(Value::Null)
                                    }else{
                                        crate::input::parse_money_input(draft)
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
                                    if crate::button::action(line,"이관 복구",
                                        theme::ButtonRole::Standard,
                                        writable_snapshot&&self.gateway.excel_write_enabled()
                                    ).clicked(){
                                        workflow_action=Some((
                                            "restore_shipment".into(),
                                            text(&entry.shipment,"id").into(),
                                            id.into(),self.tab.origin().into(),Value::Null
                                        ));
                                    }
                                }
                                if self.tab==Tab::Store&&crate::button::action(
                                    line,"매장 발송 확인",theme::ButtonRole::Standard,
                                    writable_snapshot&&self.gateway.excel_write_enabled()
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
            self.start_export(ui.ctx().clone(),self.tab.origin().to_owned(),ids);
        }
        if let Some((action,shipment_id,queue_id,origin,value))=workflow_action{
            let Some(revision)=self.result.as_ref()
                .and_then(|s|s.state["revision"].as_u64()) else{
                self.error=Some("최신 배송 정보를 확인할 수 없습니다.".into());
                return;
            };
            self.task("workflow",ui.ctx().clone(),move|gateway|{
                gateway.mutate_excel_workflow(
                    revision,&action,&shipment_id,&queue_id,&origin,value
                )
            });
        }
        crate::floating_notice::sync(
            ui.ctx(),"excel.result","excel.action",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"excel.error","excel.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        });
    }
}

#[cfg(test)]
mod table_state_tests {
    use super::*;

    #[test]
    fn shipment_exports_share_direct_execution_and_success_only_minimize(){
        let source=include_str!("excel_manager.rs");
        let start=source.find("if let Some(ids)=exported{").unwrap();
        let end=start+source[start..].find("if let Some((action,shipment_id").unwrap();
        let branch=&source[start..end];
        assert!(branch.contains("self.start_export(ui.ctx().clone(),self.tab.origin().to_owned(),ids)"));
        assert!(!branch.contains("self.tab==Tab::Factory"));
        assert!(!source.contains(concat!("excel-export-", "confirm")));
        let start=source.find("fn start_export(").unwrap();
        let end=start+source[start..].find("/// Review only unresolved").unwrap();
        let worker=&source[start..end];
        assert!(worker.find("g.export_excel(").unwrap()<worker.find("g.open_excel_folder(").unwrap());
        assert!(worker.contains("Ok(())=>result[\"folderOpened\"]=json!(true)"));
        assert!(worker.contains("Err(reason)=>result[\"folderWarning\"]=json!(reason)"));
        assert!(worker.contains("Err(reason)=>result[\"projectionError\"]=json!(reason)"));
        assert!(!worker.contains("json!({\"state\":&result[\"state\"]}))?;"));
        let start=source.find("}else if operation==\"export\"{").unwrap();
        let end=start+source[start..].find("요청한 Excel 작업").unwrap();
        let completion=&source[start..end];
        let completion=completion.split_whitespace().collect::<String>();
        assert!(completion.contains("ifresult[\"folderOpened\"]==true{ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));"));
    }

    #[test]
    fn history_table_has_only_file_actions_and_no_related_order_panel() {
        // The archive lists files; it does not mutate or navigate orders.
        // Source-level guard is intentional: this regression was an inline
        // panel and mutable workflow action, not a data-contract change.
        let source=include_str!("excel_manager.rs");
        let start=source.find("let order_list=self.history_cache.get")
            .expect("history table must still exist");
        let end=start+source[start..]
            .find("let all=!visible.is_empty()")
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
    fn excel_history_defaults_to_today_for_both_date_fields(){
        let today=NaiveDate::from_ymd_opt(2026,11,22).unwrap();
        let (from,to)=default_export_date_range(today);
        assert_eq!(from,"2026-11-22");
        assert_eq!(to,"2026-11-22");
        assert!(export_date_matches("2026-11-22T12:30:00+09:00",&from,&to));
        assert!(!export_date_matches("2026-11-21T23:59:59+09:00",&from,&to));
        assert!(export_date_matches("2026-11-21T23:59:59+09:00","",""));
    }

    #[test]
    fn invalid_manual_date_does_not_replace_applied_search_range(){
        let mut from="2026-11-01".to_owned();
        let mut to="2026-11-20".to_owned();
        assert!(!apply_valid_export_date_range(
            &mut from,&mut to,"2026-11-","2026-11-20"
        ));
        assert_eq!(from,"2026-11-01");
        assert_eq!(to,"2026-11-20");
        assert!(!apply_valid_export_date_range(
            &mut from,&mut to,"2026-11-21","2026-11-20"
        ));
        assert_eq!(from,"2026-11-01");
        assert!(apply_valid_export_date_range(
            &mut from,&mut to,"2026-11-10","2026-11-20"
        ));
        assert_eq!(from,"2026-11-10");
        assert!(apply_valid_export_date_range(
            &mut from,&mut to,"",""
        ));
        assert!(from.is_empty()&&to.is_empty());
    }

    #[test]
    fn archive_filters_by_exported_korean_date_inclusive_and_all_dates(){
        let entry=json!({
            "id":"file1","at":"2026-11-21T16:30:00Z",
            "orderIds":["o1"],"filename":"공장.xlsx",
            "snapshots":[{"snapshot":{"recipient":{"items":[
                {"name":"709 보면대","quantity":2}
            ]}}}]
        });
        let state=json!({
            "exports":[entry],
            "orders":[{"id":"o1","clientSnapshot":{"name":"과거 거래처"}}]
        });
        // 16:30Z on Nov 21 is 01:30 on Nov 22 in Seoul.
        assert_eq!(export_day("2026-11-21T16:30:00Z"),"2026-11-22");
        assert_eq!(export_history(&state,"","2026-11-22","2026-11-22").len(),1);
        assert!(export_history(&state,"","2026-11-21","2026-11-21").is_empty());
        assert_eq!(export_history(&state,"","","").len(),1);
        assert_eq!(export_history(&state,"","2026-11-23","").len(),0);
        assert_eq!(export_history(&state,"","","2026-11-21").len(),0);
    }

    #[test]
    fn archive_shows_all_clients_and_the_immutable_exported_items(){
        let state=json!({
            "exports":[{"id":"e1","at":"2026-11-21T07:00:00+09:00",
                "filename":"출력_001.xlsx","savedPath":"C:/history/출력.xlsx",
                "orderIds":["o1","o2"],
                "snapshots":[
                    {"snapshot":{"recipient":{"items":[
                        {"name":"709 보면대","quantity":3},
                        {"name":"GT-1","quantity":1}
                    ]}}},
                    {"snapshot":{"recipient":{"items":[
                        {"name":"Desk TS","quantity":5}
                    ]}}}
                ]
            }],
            "orders":[
                {"id":"o1","clientSnapshot":{"name":"변경된 거래처"},
                    "confirmationSnapshot":{"client":{"name":"확정 당시 A"}},
                    "recipients":[{"items":[{"name":"수정 후 상품","quantity":99}]}]},
                {"id":"o2","clientSnapshot":{"name":"거래처 B"}}
            ]
        });
        let rows=export_history(&state,"","","");
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].clients,"확정 당시 A / 거래처 B");
        assert_eq!(rows[0].products,
            "◆ 709 보면대 × 3  ◆ GT-1 × 1  ◆ Desk TS × 5");
        assert!(!rows[0].products.contains("수정 후 상품"));
        assert_eq!(export_history(&state,"gt-1","","").len(),1);
        assert_eq!(export_history(&state,"거래처 b","","").len(),1);
        assert_eq!(export_history(&state,"출력_001","","").len(),1);
        assert!(export_history(&state,"없는 상품","","").is_empty());
    }

    #[test]
    fn archive_without_frozen_item_snapshots_does_not_invent_products(){
        let s=json!({
            "exports":[{"id":"old","at":"2026-10-01",
                "orderIds":["o1"],"snapshots":[],
                "filename":"이전 파일.xlsx"}],
            "orders":[{"id":"o1","clientSnapshot":{"name":"과거 거래처"},
                "recipients":[{"items":[{"name":"현재 주문 품목","quantity":42}]}]}]
        });
        let rows=export_history(&s,"","","");
        assert_eq!(rows[0].clients,"과거 거래처");
        assert_eq!(rows[0].products,"출력 당시 상품 기록 없음");
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
    fn committed_reply_projection_reuses_verified_state_and_preserves_independent_refresh(){
        let ctx=egui::Context::default();
        struct NoReadGateway;
        impl BandoGateway for NoReadGateway{
            fn load_catalog(&self)->Result<Value,String>{Err("no catalog".into())}
            fn load_working_source(&self)->Result<crate::preview_store::WorkingSource,String>{Err("no source".into())}
            fn save_working_source(&self,_:&crate::preview_store::WorkingSource)->Result<u64,String>{Err("no write".into())}
            fn load_business_state(&self)->Result<Value,String>{Err("no business read".into())}
            fn execute(&self,_:ReadOperation,_:Value)->Result<Value,String>{Err("no rules".into())}
        }
        let mut manager=ExcelManager::geometry_fixture(Arc::new(NoReadGateway),ctx,0,true);
        let result=json!({"state":{"revision":8},"shipmentReads":{"shipments":[]}});
        manager.accept_reply_correction(result.clone());
        assert!(manager.take_reply_changed());
        assert!(!manager.reply_refresh_pending);
        assert_eq!(manager.result.as_ref().unwrap().state["revision"],8);
        manager.request_reply_refresh();
        manager.accept_reply_correction(result);
        assert!(manager.reply_refresh_pending);
        manager.reply_refresh_pending=false;
        manager.accept_reply_correction(json!({"projectionError":"synthetic"}));
        assert!(manager.take_reply_changed());
        assert!(manager.reply_refresh_pending);
        assert!(manager.result.is_some());
        assert!(manager.refresh_job.required());
    }

    #[test]
    fn committed_export_projection_error_keeps_success_and_requests_refresh(){
        let ctx=egui::Context::default();
        struct NoReadGateway;
        impl BandoGateway for NoReadGateway{
            fn load_catalog(&self)->Result<Value,String>{Err("no catalog".into())}
            fn load_working_source(&self)->Result<crate::preview_store::WorkingSource,String>{Err("no source".into())}
            fn save_working_source(&self,_:&crate::preview_store::WorkingSource)->Result<u64,String>{Err("no write".into())}
            fn load_business_state(&self)->Result<Value,String>{Err("no business read".into())}
            fn execute(&self,_:ReadOperation,_:Value)->Result<Value,String>{Err("no rules".into())}
        }
        for origin in ["FACTORY","STORE"]{
            let mut manager=ExcelManager::geometry_fixture(Arc::new(NoReadGateway),ctx.clone(),0,true);
            let (sender,receiver)=mpsc::channel();
            manager.operation=Some(receiver);
            manager.operation_name=Some("export".into());
            sender.send(Ok(json!({"origin":origin,"count":1,"state":{"revision":8},
                "projectionError":"synthetic","folderOpened":true}))).unwrap();
            let _=ctx.run(Default::default(),|ctx|manager.poll(ctx));
            assert!(manager.operation.is_none());
            assert!(manager.notice.as_deref().unwrap().contains("Excel 저장과 출력 이력을 기록했습니다."));
            assert!(manager.result.is_some());
            assert!(manager.refresh_job.required());
            assert!(manager.reply_refresh_pending);
            assert_eq!(manager.error.as_deref(),Some("synthetic"));
        }
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

#[cfg(test)]
impl ExcelManager {
    pub(crate) fn geometry_fixture(gateway:Arc<dyn BandoGateway>,ctx:egui::Context,tab:usize,loaded:bool)->Self{
        let mut page=Self::new(gateway,ctx);
        page.refresh_job.reset();page.tab=Tab::ALL[tab];if loaded{page.result=Some(Snapshot{state:serde_json::json!({}),shipments:serde_json::json!([])});}
        page
    }

    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        let finish=self.refresh_job.geometry_begin_read();
        self.error=None;
        let origin=if self.tab==Tab::Store{"STORE"}else{"FACTORY"};
        if self.tab==Tab::History{self.export_from.clear();self.export_to.clear();self.export_from_input.clear();self.export_to_input.clear();}
        Box::new(move |answer|{
            let result=answer.map(|count|{
                let orders=(0..count).map(|i|json!({"id":format!("geometry-o-{i}"),"clientSnapshot":{"name":"검증 거래처"},"recipients":[{"id":format!("r-{i}"),"name":"합성 수취인","items":[{"name":"검증 상품","quantity":2}]}]})).collect::<Vec<_>>();
                let shipments=(0..count).map(|i|json!({"id":format!("geometry-s-{i}"),"orderId":format!("geometry-o-{i}"),"recipientId":format!("r-{i}")})).collect::<Vec<_>>();
                let queue=(0..count).map(|i|json!({"id":format!("geometry-q-{i}"),"shipmentId":format!("geometry-s-{i}"),"origin":origin,"status":"QUEUED"})).collect::<Vec<_>>();
                let exports=(0..count).map(|i|json!({"id":format!("geometry-export-{i}"),"at":"2026-11-24T09:00:00+09:00","filename":format!("검증-{i}.xlsx"),"orderIds":[format!("geometry-o-{i}")],"snapshots":[{"snapshot":{"recipient":{"items":[{"name":"검증 상품","quantity":2}]}}}]})).collect::<Vec<_>>();
                Snapshot{state:json!({"orders":orders,"exports":exports,"workflow":{"shipments":shipments,"queue":queue}}),shipments:json!({"shipments":[]})}
            });
            finish(result);
        })
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
}
