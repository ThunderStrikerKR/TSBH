//! React WorkflowQueue's Excel management view, now native egui.
//! Export, sender validation, workflow state and file history remain owned by
//! the original Rust business core and Tauri's existing SQLite database.
use crate::{components,gateway::{BandoGateway,ReadOperation},settings_help,theme,typography};
use chrono::{DateTime,FixedOffset};
use eframe::egui::{self,Align,Align2,Rect,Sense,Stroke,Ui,UiBuilder,Vec2};
use serde_json::{json,Value};
use std::{collections::BTreeSet,sync::{mpsc::{self,Receiver,TryRecvError},Arc}};

const HEIGHT:f32=theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT;
const HEADER:f32=theme::MANAGEMENT_TABLE_HEADER_HEIGHT;
const GAP:f32=theme::FREIGHT_ASSIGNMENT_COL_GAP;
const QUEUE:[f32;5]=[48.0,212.0,354.0,178.0,182.0];
const HISTORY:[f32;4]=[176.0,122.0,485.0,292.0];
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
fn account_for(state:&Value,ship:&Value)->Value{
    let id=text(ship,"id");
    rows(&state["shipmentReads"]["shipments"]).iter()
        .find(|v|text(v,"shipmentId")==id)
        .and_then(|v|v.get("client")).cloned().unwrap_or(Value::Null)
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
            .filter(Value::is_object).unwrap_or(original);
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
fn export_history(state:&Value,query:&str)->Vec<Value>{
    let mut result=rows(&state["exports"]).iter().filter(|e|{
        text(e,"filename").contains(query)||
        rows(&e["orderIds"]).iter().any(|id|{
            id.as_str().and_then(|id|find(&state["orders"],id))
                .is_some_and(|order|text(&order["clientSnapshot"],"name").contains(query))
        })
    }).cloned().collect::<Vec<_>>();
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
fn cmp_history(a:&Value,b:&Value,column:usize)->std::cmp::Ordering{
    match column{
        0=>text(a,"at").cmp(text(b,"at")),
        1=>rows(&a["queueEntryIds"]).len()
            .cmp(&rows(&b["queueEntryIds"]).len()),
        2=>text(a,"filename").cmp(text(b,"filename")),
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
fn sort_history(lines:&mut [Value],sort:Option<Sort>){
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
fn cell(ui:&Ui,rect:Rect,visible:&str,align:Align2,id:impl std::hash::Hash){
    let padded=rect.shrink2(Vec2::new(theme::SPACE_8,0.0));
    let clip=padded.intersect(ui.clip_rect());
    if clip.width()<=0.0||clip.height()<=0.0{return}
    let label=typography::freight_table_cell_galley(ui,visible,false,theme::TEXT);
    let x=if align==Align2::RIGHT_CENTER{
        padded.right()-label.size().x
    }else if align==Align2::CENTER_CENTER{
        padded.center().x-label.size().x/2.0
    }else{padded.left()};
    ui.painter().with_clip_rect(clip).galley(
        egui::pos2(x,padded.center().y-label.size().y/2.0),label,theme::TEXT
    );
    if !visible.is_empty(){
        ui.interact(clip,ui.make_persistent_id(("excel-table-cell",id)),Sense::hover())
            .on_hover_text(visible);
    }
}
fn table<const N:usize>(
    ui:&mut Ui,id:&str,titles:[&str;N],base:[f32;N],
    align:[Align2;N],stretch:usize,rows:usize,
    sort:&mut Option<Sort>,sort_enabled:[bool;N],
    mut render:impl FnMut(&mut Ui,[Rect;N],usize)
){
    let old_gap=ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y=0.0;
    let viewport=ui.available_width().max(1.0);
    let reserved=ui.spacing().scroll.allocated_width()+theme::SPACE_4;
    let minimum=base.iter().sum::<f32>()+(N-1) as f32*GAP;
    let width=(viewport-reserved).max(minimum);
    let mut widths=base;
    widths[stretch]+=(width-minimum).max(0.0);
    let (header,_)=ui.allocate_exact_size(Vec2::new(viewport,HEADER),Sense::hover());
    let mut left=None;
    let output=components::virtual_fixed_rows(
        egui::ScrollArea::both().id_salt(id)
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .max_height(ui.ctx().content_rect().height()
                *theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
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
    let mut clicked=None;
    for (i,title) in titles.iter().enumerate(){
        let h=heads[i];
        let suffix=if sort_enabled[i] {
            match sort.filter(|s|s.column==i).map(|s|s.direction){
                Some(Direction::Up)=>" ▲",Some(Direction::Down)=>" ▼",None=>" ↕"
            }
        }else{""};
        let galley=typography::freight_table_header_galley(ui,&format!("{title}{suffix}"));
        let x=if align[i]==Align2::RIGHT_CENTER{
            h.right()-theme::SPACE_8-galley.size().x
        }else if align[i]==Align2::CENTER_CENTER{
            h.center().x-galley.size().x/2.0
        }else{h.left()+theme::SPACE_8};
        painter.with_clip_rect(h.intersect(header)).galley(
            egui::pos2(x,h.center().y-galley.size().y/2.0),galley,theme::SECONDARY);
        if sort_enabled[i]{
            let hit=h.intersect(header);
            if hit.width()>0.0&&hit.height()>0.0&&ui.interact(hit,
                ui.make_persistent_id((id,"sort",i)),Sense::click()).clicked(){
                clicked=Some(Sort::next(*sort,i));
            }
        }
    }
    if let Some(next)=clicked{*sort=next;}
    painter.line_segment([header.left_bottom(),header.right_bottom()],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER));
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
    history_detail:Option<String>,
    open_order:Option<String>,
    open_settings:bool,
}
impl ExcelManager{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:egui::Context)->Self{
        let mut manager=Self{
            gateway,receiver:None,result:None,operation:None,operation_name:None,
            preview:None,error:None,notice:None,tab:Tab::Factory,
            query:String::new(),selected:BTreeSet::new(),
            sort_queue:None,sort_history:None,history_detail:None,
            open_order:None,open_settings:false,
        };
        manager.refresh(ctx);manager
    }
    pub fn take_open_order(&mut self)->Option<String>{self.open_order.take()}
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
                }else if operation=="export"{
                    let count=result["count"].as_u64().unwrap_or(0);
                    let origin=text(&result,"origin");
                    self.notice=Some(format!(
                        "{} {}건 Excel 저장과 출력 이력을 기록했습니다.",
                        if origin=="FACTORY"{"공장 발송"}else{"매장 발송"},count
                    ));
                    self.selected.clear();
                    self.result=None;
                    // A saved file must be reflected via a fresh DB read,
                    // not through an optimistic local snapshot.
                    self.result=Some(Snapshot{
                        state:result["state"].clone(),shipments:Value::Null
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
        ui.spacing_mut().scroll=theme::management_scroll_style();
        self.poll();
        ui.horizontal(|title|{
            title.label(typography::page_heading("엑셀 관리"));
            settings_help::icon(title,"발송 Excel 관리");
        });
        ui.add_space(theme::SPACE_12);
        let busy=self.operation.is_some()||self.receiver.is_some();
        if let Some(next)=components::management_tabs(ui,&[
            ("공장 엑셀 대기",self.tab==Tab::Factory,!busy),
            ("매장 발송 대기",self.tab==Tab::Store,!busy),
            ("엑셀 작성 이력",self.tab==Tab::History,!busy),
        ]){
            let selected=Tab::ALL[next];
            if selected!=self.tab{
                self.tab=selected;
                self.selected.clear();self.query.clear();
                self.sort_history=None;self.sort_queue=None;
                self.history_detail=None;
            }
        }
        ui.add_space(theme::SPACE_16);
        let mut exported=None;
        let mut open_file=None;
        let mut open_daily=false;
        let mut open_history=None;
        let mut open_order=None;
        let mut copy_path=None;
        let mut company_check=false;
        let mut go_settings=false;
        theme::management_list_frame().show(ui,|section|{
            theme::management_list_strip_frame().show(section,|toolbar|{
                toolbar.horizontal(|line|{
                    if components::flexible_text_field(
                        line,&mut self.query,
                        theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                        "거래처 / 수취인 / 상품 검색"
                    ).changed(){
                        self.selected.clear();
                    }
                });
            });
            if let Some(error)=self.error.as_deref(){
                theme::management_list_strip_frame().show(section,|line|{
                    line.label(typography::status(error,theme::DANGER));
                });
            }
            if let Some(message)=self.notice.as_deref(){
                theme::management_list_strip_frame().show(section,|line|{
                    line.label(typography::caption(message));
                });
            }
            if busy{
                theme::management_list_strip_frame().show(section,|line|{
                    line.label(typography::caption("Excel 자료를 확인 중입니다."));
                });
            }
            components::soft_divider(section);
            let state=self.result.as_ref().map(|r|&r.state);
            let native=self.result.as_ref().map(|r|&r.shipments);
            if self.tab==Tab::History{
                let mut history=state.map(|s|export_history(s,&self.query)).unwrap_or_default();
                sort_history(&mut history,self.sort_history);
                let indexes=0..history.len();
                let order_list=history.clone();
                table(section,"excel-history-table",
                    ["작성 날짜","작성 건수","파일 / 주문","조회"],
                    HISTORY,[Align2::LEFT_CENTER,Align2::RIGHT_CENTER,
                        Align2::LEFT_CENTER,Align2::CENTER_CENTER],
                    2,indexes.len(),&mut self.sort_history,
                    [true,true,true,false],|row,cells,i|{
                        let entry=&order_list[i];
                        let id=text(entry,"id");
                        cell(row,cells[0],&datetime(text(entry,"at")),
                            Align2::LEFT_CENTER,(id,0));
                        let count=rows(&entry["queueEntryIds"]).len().max(
                            rows(&entry["orderIds"]).len()
                        );
                        cell(row,cells[1],&format!("{count}건"),
                            Align2::RIGHT_CENTER,(id,1));
                        cell(row,cells[2],text(entry,"filename"),
                            Align2::LEFT_CENTER,(id,2));
                        row.scope_builder(UiBuilder::new()
                            .id_salt(("excel-history-actions",id))
                            .max_rect(cells[3].shrink2(Vec2::new(theme::SPACE_8,0.0)))
                            .layout(egui::Layout::left_to_right(Align::Center)),
                            |buttons|{
                                if !text(entry,"savedPath").is_empty() {
                                    if theme::action(buttons,"파일 열기",
                                        theme::ButtonRole::TableSecondary,!busy).clicked(){
                                        open_file=Some(id.to_owned());
                                    }
                                    if theme::action(buttons,"경로 복사",
                                        theme::ButtonRole::TableSecondary,!busy).clicked(){
                                        copy_path=Some(text(entry,"savedPath").to_owned());
                                    }
                                }
                                if theme::action(buttons,"관련 주문",
                                    theme::ButtonRole::TableSecondary,true).clicked(){
                                    open_history=Some(id.to_owned());
                                }
                            });
                    });
                if history.is_empty()&&self.receiver.is_none(){
                    section.add_space(theme::SPACE_8);
                    section.label(typography::caption(
                        if self.query.is_empty(){"Excel 작성 이력이 없습니다."}
                        else{"검색 조건에 맞는 Excel 작성 이력이 없습니다."}));
                }
                if let Some(id)=self.history_detail.as_deref(){
                    if let Some(record)=history.iter().find(|r|text(r,"id")==id){
                        theme::management_list_strip_frame().show(section,|details|{
                            details.label(typography::subsection_heading("관련 주문"));
                            details.horizontal_wrapped(|bar|{
                                for order in rows(&record["orderIds"]){
                                    if let Some(order_id)=order.as_str(){
                                        if theme::action(bar,"주문 상세 보기",
                                            theme::ButtonRole::Standard,true).clicked(){
                                            open_order=Some(order_id.to_owned());
                                        }
                                    }
                                }
                            });
                        });
                    }
                }
            }else{
                let mut queued=if let (Some(s),Some(r))=(state,native){
                    records(s,r,self.tab,&self.query)
                }else{Vec::new()};
                sort_queue(&mut queued,self.sort_queue);
                let visible:BTreeSet<String>=queued.iter().filter(|r|!r.legacy)
                    .map(|r|r.id().to_owned()).collect();
                self.selected.retain(|id|visible.contains(id));
                let chosen=self.selected.len();
                let all=!visible.is_empty()&&chosen==visible.len();
                let mut check_all=all;
                let records=queued.clone();
                table(section,"excel-queue-table",
                    ["선택","거래처 / 발주","수취인 / 배송 품목","금액","처리"],
                    QUEUE,[Align2::CENTER_CENTER,Align2::LEFT_CENTER,
                        Align2::LEFT_CENTER,Align2::RIGHT_CENTER,
                        Align2::CENTER_CENTER],
                    2,records.len(),&mut self.sort_queue,
                    [false,true,true,true,false],
                    |row,cells,index|{
                        let entry=&records[index];
                        let id=entry.id().to_owned();
                        let mut checked=self.selected.contains(&id);
                        let selectable=!entry.legacy;
                        let checked_response=row.put(
                            cells[0].shrink2(Vec2::new(theme::SPACE_8,theme::SPACE_4)),
                            egui::Checkbox::without_text(&mut checked)
                        );
                        if checked_response.clicked() && selectable{
                            if checked{self.selected.insert(id.clone());}
                            else{self.selected.remove(&id);}
                        }
                        if entry.legacy {
                            checked_response.on_disabled_hover_text(
                                "기존 주문은 출고 이관 후 출력할 수 있습니다."
                            );
                        }
                        cell(row,cells[1],&entry.customer(),
                            Align2::LEFT_CENTER,(id.clone(),1));
                        cell(row,cells[2],&entry.recipient_summary(),
                            Align2::LEFT_CENTER,(id.clone(),2));
                        let price=if entry.legacy{"기존 확정 금액 유지".to_owned()}
                            else if text(&entry.order,"orderType")=="RETAIL"{
                                "결제 완료 주문".into()
                            }else{amount(&entry.queue["snapshot"]["total"])};
                        cell(row,cells[3],&price,
                            Align2::RIGHT_CENTER,(id.clone(),3));
                        row.scope_builder(UiBuilder::new()
                            .id_salt(("excel-queue-action",id))
                            .max_rect(cells[4].shrink2(Vec2::new(theme::SPACE_8,0.0)))
                            .layout(egui::Layout::left_to_right(Align::Center)),
                            |buttons|{
                                if theme::action(buttons,"상세 보기",
                                    theme::ButtonRole::TableSecondary,true).clicked(){
                                    open_order=Some(entry.order_id().to_owned());
                                }
                                // The legacy React row has a separate
                                // management detail editor. Until that editor
                                // is connected, no fake "관리" action is shown.
                            });
                    }
                );
                if records.is_empty()&&self.receiver.is_none(){
                    section.add_space(theme::SPACE_8);
                    section.label(typography::caption(
                        if self.query.is_empty(){"발송 대기 건이 없습니다."}
                        else{"검색 조건에 맞는 발송 대기 건이 없습니다."}
                    ));
                }
                theme::management_list_strip_frame().show(section,|footer|{
                    footer.horizontal(|line|{
                        line.label(typography::caption(
                            "파일 작성은 실제 상품 발송 완료를 의미하지 않습니다."
                        ));
                        line.with_layout(egui::Layout::right_to_left(Align::Center),|right|{
                            if theme::action(right,"폴더 열기",
                                theme::ButtonRole::Standard,!busy).clicked(){
                                open_daily=true;
                            }
                            if theme::action(right,
                                &format!("선택 {}건 {} Excel 작성",
                                    chosen,if self.tab==Tab::Factory{"공장"}else{"매장"}),
                                theme::ButtonRole::Primary,
                                chosen>0&&!busy&&self.gateway.excel_write_enabled()
                            ).clicked(){
                                exported=Some(self.selected.iter().cloned()
                                    .collect::<Vec<_>>());
                            }
                        });
                    });
                });
            }
        });
        if let Some(id)=open_file{
            self.task("file",ui.ctx().clone(),move|g|{
                g.open_excel_file(&id)?;
                Ok(json!({}))
            });
        }
        if let Some(path)=copy_path{ui.ctx().copy_text(path);self.notice=Some("저장 경로를 복사했습니다.".into());}
        if open_daily{
            let origin=self.tab.origin().to_owned();
            self.task("folder",ui.ctx().clone(),move|g|{
                g.open_excel_folder(None,Some(&origin))?;Ok(json!({}))
            });
        }
        if let Some(id)=open_history{
            self.history_detail=if self.history_detail.as_deref()==Some(&id){
                None
            }else{Some(id)};
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
        if self.preview.is_some(){
            let response=self.preview.as_ref().unwrap();
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
                            let result=g.export_excel(&origin,&ids)?;
                            if folder_after{
                                if let Some(export_id)=result["exportId"].as_str(){
                                    // Explorer failure is NOT an export failure.
                                    let _=g.open_excel_folder(Some(export_id),None);
                                }
                            }
                            Ok(result)
                        });
                    }
                });
            });
        }
        if go_settings{self.open_settings=true;}
    }
}
