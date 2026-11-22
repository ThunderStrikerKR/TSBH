//! egui manual source assignment, powered ONLY by the existing Rust
//! apply_manual_classification reducer. No second classification algorithm.
//!
//! This first stage provides source-block ownership, row/item structure,
//! quantity validation and applying an unconfirmed preview. The chooser never
//! edits a confirmed order or directly writes the Tauri business database.

use std::{sync::{Arc,mpsc::{self,Receiver}},thread,time::{SystemTime,UNIX_EPOCH},sync::atomic::{AtomicU64,Ordering}};
use eframe::egui::{self,Context};
use serde_json::{json,Value};
use crate::{gateway::{BandoGateway,ReadOperation},preview_store::WorkingReception,theme::{self,ButtonRole},typography,components,manual_product_search::{ProductSearch,Event as ProductEvent}};

fn text<'a>(value:&'a Value,key:&str)->&'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(value:&'a Value,key:&str)->&'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn unique_id()->String {
    static NEXT:AtomicU64=AtomicU64::new(0);
    let nanos=SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d|d.as_nanos()).unwrap_or(0);
    format!("egui-manual-{}-{nanos}-{}",std::process::id(),
        NEXT.fetch_add(1,Ordering::Relaxed))
}
fn rule(gateway:&dyn BandoGateway,payload:Value)->Result<Value,String> {
    gateway.execute(ReadOperation::ManualClassification,payload)
}

/// A UI-only reconstruction of the native order shape for Rust manual rules.
/// Recipients and evidence are the ACTUAL Rust analysis, not local parsing.
pub fn native_order(
    gateway:&dyn BandoGateway, row:&WorkingReception, recognition:&Value
)->Result<Value,String> {
    if row.analysis_raw.as_deref()!=Some(row.raw.as_str()) ||
       !recognition.get("recipients").is_some_and(Value::is_array) {
        return Err("현재 원문에 대한 Rust 인식이 끝난 뒤 수동 분류를 진행하세요.".into());
    }
    let now=chrono::Utc::now().to_rfc3339();
    let initialized=gateway.execute(ReadOperation::NewReception,json!({
        "action":"new_reception","id":row.id,"clientId":row.client_id,
        "orderType":row.order_type,"now":now
    }))?;
    let mut order=initialized.get("order").filter(|v|v.is_object())
        .ok_or("Rust 발주 초기화 결과가 올바르지 않습니다.")?.clone();
    order["raw"]=json!(row.raw);
    order["analysisRaw"]=json!(row.raw);
    order["analysisEvidence"]=recognition.get("evidence")
        .cloned().unwrap_or_else(||json!([]));
    order["recipients"]=recognition.get("recipients")
        .cloned().unwrap_or_else(||json!([]));
    if let Some(saved)=row.manual_model.as_ref() {
        let model=saved.get("model").filter(|v|v.is_object())
            .ok_or("저장된 수동 분류 작업본이 올바르지 않습니다.")?;
        order["recipients"]=model.get("rows").cloned().unwrap_or_else(||json!([]));
        order["manualClassification"]=json!({
            "blocks":model.get("blocks"),"assignments":model.get("assignments")
        });
    }
    Ok(order)
}

enum Outcome {
    Initial { order:Value, model:Value, assignment_order:Vec<String> },
    Updated { model:Value, assignment_order:Vec<String> },
    Applied { order:Value, model:Value, assignment_order:Vec<String> },
}
pub enum Event {
    Closed,
    Applied { order_id:String,raw:String,order_type:String,client_id:Option<String>,
        model:Value,assignment_order:Vec<String>,order:Value },
}

pub struct ManualWorkspace {
    pub order_id:String,
    raw:String,
    order_type:String,
    client_id:Option<String>,
    gateway:Arc<dyn BandoGateway>,
    base_order:Option<Value>,
    model:Option<Value>,
    assignment_order:Vec<String>,
    selected_recipient:usize,
    selected_item:Option<String>,
    selected_field:String,
    edit_text:String,
    confirm_discard:bool,
    product_search:Option<ProductSearch>,
    remove_recipient_pending:Option<String>,
    reset_pending:bool,
    receiver:Option<Receiver<Result<Outcome,String>>>,
    error:Option<String>,
    dirty:bool,
    applied:Option<Value>,
}

impl ManualWorkspace {
    pub fn new(source:WorkingReception,gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let (tx,rx)=mpsc::channel();
        let row=source.clone();
        let service=Arc::clone(&gateway);
        thread::spawn(move||{
            let result=(||->Result<Outcome,String>{
                let recognition=row.analysis_evidence.as_ref()
                    .ok_or("현재 원문의 Rust 인식 결과가 없습니다.")?;
                let order=native_order(service.as_ref(),&row,recognition)?;
                let assignment_order=row.manual_model.as_ref()
                    .and_then(|saved|saved.get("assignmentOrder"))
                    .and_then(Value::as_array).cloned().unwrap_or_default();
                let initial=rule(service.as_ref(),json!({
                    "action":"initial_manual","order":order,
                    "assignmentOrder":assignment_order
                }))?;
                let model=initial.get("model").filter(|v|v.is_object())
                    .ok_or("Rust 수동 분류 초기 결과가 올바르지 않습니다.")?.clone();
                let keys=initial.get("assignmentOrder").and_then(Value::as_array)
                    .ok_or("Rust 배정 순서가 없습니다.")?.iter()
                    .map(|v|v.as_str().map(str::to_owned).ok_or("Rust 배정 ID 오류".to_owned()))
                    .collect::<Result<Vec<_>,_>>()?;
                Ok(Outcome::Initial{order,model,assignment_order:keys})
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
        Self {
            order_id:source.id,raw:source.raw,order_type:source.order_type,
            client_id:source.client_id,gateway,base_order:None,model:None,
            assignment_order:Vec::new(),selected_recipient:0,
            selected_item:None,selected_field:"name".into(),
            edit_text:String::new(),confirm_discard:false,
            product_search:None,remove_recipient_pending:None,
            reset_pending:false,
            receiver:Some(rx),error:None,dirty:false,applied:None,
        }
    }
    /// Open the existing native editor on the recipient currently selected
    /// in the main intake pane, not silently on the first shipment.
    pub fn focus_recipient(&mut self,index:usize){
        self.selected_recipient=index;
        self.selected_item=None;
        self.selected_field="name".to_owned();
    }
    fn busy(&self)->bool {self.receiver.is_some()}
    fn poll(&mut self) {
        let received=self.receiver.as_ref().map(|r|r.try_recv());
        match received {
            Some(Ok(Ok(Outcome::Initial{order,model,assignment_order})))=>{
                self.base_order=Some(order);
                let count=list(&model,"rows").len();
                self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.receiver=None;
                self.edit_text=self.current_value();
            }
            Some(Ok(Ok(Outcome::Updated{model,assignment_order})))=>{
                self.model=Some(model);
                self.assignment_order=assignment_order;
                let count=self.model.as_ref().map(|m|list(m,"rows").len()).unwrap_or(0);
                self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
                if self.selected_item.as_ref().is_some_and(|id|
                    !self.model.as_ref().is_some_and(|m|
                        list(m,"rows").get(self.selected_recipient).is_some_and(|r|
                            list(r,"items").iter().any(|item|text(item,"id")==id)
                        )
                    )
                ){
                    self.selected_item=None;
                    self.selected_field="name".into();
                }
                self.dirty=true;
                self.error=None;
                self.receiver=None;
                self.edit_text=self.current_value();
            }
            Some(Ok(Ok(Outcome::Applied{order,model,assignment_order})))=>{
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.applied=Some(order);
                self.receiver=None;
            }
            Some(Ok(Err(error)))=>{
                self.error=Some(error);
                self.receiver=None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.error=Some("Rust 수동 분류 처리가 중단되었습니다.".into());
                self.receiver=None;
            }
            _=>{}
        }
    }
    fn select_cell(&mut self,row:usize,item:Option<String>,field:&str) {
        if self.edit_text!=self.current_value() {
            self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
            return;
        }
        self.selected_recipient=row;
        self.selected_item=item;
        self.selected_field=field.to_owned();
        self.edit_text=self.current_value();
    }

    fn current_value(&self)->String {
        let Some(model)=self.model.as_ref() else{return String::new()};
        let Some(row)=list(model,"rows").get(self.selected_recipient) else {
            return String::new();
        };
        if let Some(id)=&self.selected_item {
            let Some(item)=list(row,"items").iter().find(|v|text(v,"id")==id)
                else{return String::new()};
            return if self.selected_field=="quantity" {
                item.get("quantity").and_then(Value::as_i64)
                    .map(|n|n.to_string()).unwrap_or_default()
            } else {
                text(item,"name").to_owned()
            };
        }
        if self.selected_field=="shippingCharge"{
            // The input is a NEW applied charge, validated/normalized only
            // by the native remote-delivery rule on confirmation. Avoid
            // calculating a shown 2x charge in the egui presentation layer.
            return String::new();
        }
        text(row,&self.selected_field).to_owned()
    }

    fn action(&mut self,action:&str,extra:Value,ctx:&Context) {
        if self.busy(){return;}
        let Some(model)=self.model.clone() else{return};
        let order=self.base_order.clone().unwrap_or(Value::Null);
        let keys=self.assignment_order.clone();
        let service=Arc::clone(&self.gateway);
        let action=action.to_owned();
        let (tx,rx)=mpsc::channel();
        self.receiver=Some(rx);
        self.error=None;
        let ctx=ctx.clone();
        thread::spawn(move||{
            let result=(||->Result<Outcome,String>{
                let mut payload=json!({
                    "action":action.clone(),"model":model.clone(),
                    "assignmentOrder":keys.clone(),"order":order.clone()
                });
                if let Some(fields)=extra.as_object() {
                    for (key,value) in fields {
                        payload[key]=value.clone();
                    }
                }
                // Product recognition relies on the same native catalog and
                // customer pricing context as the main intake, not UI rules.
                if ["assign_manual_block","release_manual_block","set_manual_product"]
                    .contains(&action.as_str()) {
                    // A real dealer-specific manual product decision must
                    // see the complete operational price tier and overrides.
                    // Never price from the read-only name search projection.
                    let state=if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
                        .is_some_and(|value|!value.is_empty()) {
                        service.load_business_state()?
                    }else{
                        service.load_catalog()?
                    };
                    payload["state"]=state;
                    payload["candidateIds"]=json!((0..20).map(|_|unique_id()).collect::<Vec<_>>());
                }
                let result=if action=="native_confirm_quantity" {
                    let first=rule(service.as_ref(),json!({
                        "action":"set_manual_quantity_draft",
                        "model":&model,"cell":extra.get("cell"),
                        "value":extra.get("value")
                    }))?;
                    rule(service.as_ref(),json!({
                        "action":"confirm_manual_quantity",
                        "model":first.get("model"),"cell":extra.get("cell"),
                        "assignmentOrder":keys.clone()
                    }))?
                }else{
                    rule(service.as_ref(),payload)?
                };
                let next=result.get("model").filter(|v|v.is_object())
                    .ok_or("Rust 수동 분류 변경 결과가 올바르지 않습니다.")?.clone();
                let assignment_order=result.get("assignmentOrder")
                    .and_then(Value::as_array).map(|v|v.iter().filter_map(|s|s.as_str()
                        .map(str::to_owned)).collect()).unwrap_or_else(||keys.clone());
                Ok(Outcome::Updated{model:next,assignment_order})
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn apply(&mut self,ctx:&Context) {
        if self.busy(){return;}
        let (Some(model),Some(order))=(self.model.clone(),self.base_order.clone())
            else{return};
        let keys=self.assignment_order.clone();
        let service=Arc::clone(&self.gateway);
        let (tx,rx)=mpsc::channel();
        self.receiver=Some(rx);
        self.error=None;
        let ctx=ctx.clone();
        thread::spawn(move||{
            let result=(||->Result<Outcome,String>{
                let quantity=rule(service.as_ref(),json!({
                    "action":"manual_quantity_status","model":model.clone()
                }))?;
                if quantity.get("pending").and_then(Value::as_bool)==Some(true) {
                    return Err("선택한 상품의 수량을 먼저 확정하세요.".into());
                }
                let applied=rule(service.as_ref(),json!({
                    "action":"apply_manual","order":order.clone(),
                    "model":model.clone(),"assignmentOrder":keys.clone()
                }))?;
                let order=applied.get("order").filter(|v|v.is_object())
                    .ok_or("Rust 수동 분류 적용 결과가 올바르지 않습니다.")?.clone();
                Ok(Outcome::Applied{order,model,assignment_order:keys})
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn show(&mut self,ctx:&Context,work_area:egui::Rect)->Option<Event> {
        self.poll();
        if let Some(order)=self.applied.take() {
            if order.get("id").and_then(Value::as_str)!=Some(self.order_id.as_str()) ||
               order.get("raw").and_then(Value::as_str)!=Some(self.raw.as_str()) {
                self.error=Some("적용 결과의 발주서 ID나 원문이 일치하지 않습니다.".into());
            }else if let Some(model)=self.model.take() {
                return Some(Event::Applied {
                    order_id:self.order_id.clone(),raw:self.raw.clone(),
                    order_type:self.order_type.clone(),client_id:self.client_id.clone(),
                    model,assignment_order:self.assignment_order.clone(),order
                });
            }
        }
        let mut open=true;
        let mut chosen:Option<(String,Value)>=None;
        let mut apply=false;
        let mut discard=false;
        let mut discard_confirmed=false;
        let mut submit_direct=false;
        let mut lookup:Option<(String,String)>=None;
        components::modal_window("수동 분류","bando-native-manual-classification",work_area)
            
            .default_width((work_area.width()-theme::MANUAL_DIALOG_SIDE_GAP)
                .min(theme::MANUAL_DIALOG_MAX_WIDTH).max(0.0))
            .default_height((work_area.height()-theme::MANUAL_DIALOG_VERTICAL_GAP)
                .max(0.0))
            .constrain_to(work_area)
            .show(ctx,|ui|{
                if components::modal_header(
                    ui,"수동 분류","취소",
                    !self.busy()&&self.product_search.is_none()
                        &&self.remove_recipient_pending.is_none()
                        &&!self.reset_pending&&!self.confirm_discard
                ){open=false;}
                // Prompts and asynchronous native mutations own input while
                // open. The background worksheet remains visible but inert.
                ui.set_enabled(!self.busy() &&
                    self.product_search.is_none() &&
                    self.remove_recipient_pending.is_none() &&
                    !self.reset_pending && !self.confirm_discard);
                if self.busy() {
                    ui.label(typography::caption("처리 중입니다."));
                }
                let Some(model)=self.model.as_ref() else {return};
                let rows=list(model,"rows").to_vec();
                let blocks=list(model,"blocks").to_vec();
                let assignments=model.get("assignments").cloned().unwrap_or_else(||json!({}));
                let busy=self.busy();
                let unassigned=blocks.iter().filter(|b|
                    assignments.get(text(b,"id")).is_none()).count();
                // Display worksheet context together. Final actions belong
                // to the footer, not between the status and editing panes.
                ui.horizontal(|status|{
                    let count=status.label(typography::caption(
                        &format!("미배정됨: {unassigned}개")
                    ));
                    if status.available_width()>=theme::MANUAL_INPUT_WIDTH {
                        status.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |right|{
                                right.label(typography::caption(
                                    "정식 접수는 주문 확인 완료"
                                ));
                            }
                        );
                    }else{
                        // The explanation remains available when the window
                        // is too narrow for two unbroken status labels.
                        count.on_hover_text("정식 접수는 주문 확인 완료");
                    }
                });
                ui.separator();
                // Match React's 34:66 columns. On narrow windows keep
                // both minimum widths and scroll the workspace horizontally
                // instead of letting one panel clip or move the footer.
                let content_width=ui.available_width().max(
                    theme::MANUAL_SOURCE_MIN_WIDTH+
                    theme::MANUAL_SHEET_MIN_WIDTH+
                    theme::MANUAL_COLUMN_GAP
                );
                // The former 34pt+24pt approximation reserved only the
                // footer itself, not the 12pt gap and egui row spacing.
                // Reserve all of these before sizing the two inner panes.
                let footer_clearance=theme::MODAL_FOOTER_HEIGHT
                    +theme::SPACE_12+2.0*theme::SPACING;
                let content_height=(ui.available_height().min(work_area.height())
                    -footer_clearance).max(1.0);
                egui::ScrollArea::horizontal()
                    .id_salt("manual-columns-horizontal-scroll")
                    .max_height(content_height)
                    .auto_shrink([false,false])
                    .show(ui,|outer|{
                        outer.set_min_width(content_width);
                        outer.horizontal(|ui|{
                    let width=content_width;
                    let height=content_height;
                    let source_width=(width*theme::MANUAL_SOURCE_SHARE)
                        .max(theme::MANUAL_SOURCE_MIN_WIDTH)
                        .min((width-theme::MANUAL_SHEET_MIN_WIDTH
                            -theme::MANUAL_COLUMN_GAP)
                            .max(theme::MANUAL_SOURCE_MIN_WIDTH));
                    ui.allocate_ui_with_layout(
                        egui::vec2(source_width,height),
                        egui::Layout::top_down(egui::Align::Min),
                        |source_ui|{
                            theme::modal_nested_frame().show(source_ui,|panel|{
                                panel.set_min_width(
                                    (source_width-2.0*theme::SPACE_8).max(0.0)
                                );
                                panel.set_min_height(
                                    (height-2.0*theme::SPACE_8).max(0.0)
                                );
                                panel.label(typography::subsection_heading("붙여넣은 내용"));
                                egui::ScrollArea::vertical().id_salt("manual-source-blocks")
                                    .max_height((height-theme::SECTION_HEADER_HEIGHT
                                        -2.0*theme::SPACE_8).max(0.0))
                                    .show(panel,|ui|{
                            if let Some(id)=crate::manual_source_view::source_document(
                                ui,&self.raw,&blocks,&assignments,!busy
                            ){
                                if assignments.get(&id).is_some() {
                                    chosen=Some(("release_manual_block".into(),json!({
                                        "id":id,"clear":true
                                    })));
                                }else if let Some(row)=rows.get(self.selected_recipient){
                                    let mut cell=json!({
                                        "recipientId":row.get("id"),
                                        "field":self.selected_field
                                    });
                                    if let Some(item)=self.selected_item.as_ref() {
                                        cell["itemId"]=json!(item);
                                    }
                                    chosen=Some(("assign_manual_block".into(),json!({
                                        "id":id,"cell":cell
                                    })));
                                }
                            }
                                    });
                            });
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(),height),
                        egui::Layout::top_down(egui::Align::Min),
                        |sheet_ui|{
                            theme::modal_nested_frame().show(sheet_ui,|panel|{
                                panel.set_min_height(
                                    (height-2.0*theme::SPACE_8).max(0.0)
                                );
                                panel.label(typography::subsection_heading("배송 정보 구성표"));
                                panel.horizontal(|ui|{
                                ui.label(typography::field_label("배송 건수"));
                                if theme::action(ui,"−",ButtonRole::Standard,
                                    !busy && rows.len()>1).clicked(){
                                    let last=&rows[rows.len()-1];
                                    let rid=text(last,"id");
                                    let populated=["name","address","mobile","phone","note"]
                                        .iter().any(|field|!text(last,field).is_empty()) ||
                                        list(last,"items").iter().any(|item|
                                            !text(item,"name").is_empty() ||
                                            !text(item,"raw").is_empty() ||
                                            item.get("productId").is_some_and(|v|!v.is_null()) ||
                                            item.get("variantId").is_some_and(|v|!v.is_null()) ||
                                            item.get("quantity").is_some_and(|v|!v.is_null())
                                        ) ||
                                        assignments.as_object().is_some_and(|map|
                                            map.values().any(|cell|
                                                text(cell,"recipientId")==rid
                                            ));
                                    if populated{
                                        self.remove_recipient_pending=Some(rid.to_owned());
                                    } else {
                                        chosen=Some(("resize_manual_rows".into(),json!({
                                            "count":rows.len()-1,"newRecipientIds":[]
                                        })));
                                    }
                                }
                                ui.label(typography::quantity(&rows.len().to_string()));
                                if theme::action(ui,"+",ButtonRole::Standard,
                                    !busy && rows.len()<100).clicked(){
                                    chosen=Some(("resize_manual_rows".into(),json!({
                                        "count":rows.len()+1,
                                        "newRecipientIds":[unique_id()]
                                    })));
                                }
                            });
                            egui::ScrollArea::both()
                                .id_salt("manual-recipients-table")
                                .auto_shrink([false,false])
                                .max_height((height-theme::SECTION_HEADER_HEIGHT
                                    -theme::INPUT_HEIGHT
                                    -theme::ACTION_STANDARD_HEIGHT
                                    -theme::SPACE_24)
                                    .max(3.0*theme::MANUAL_TABLE_ROW_HEIGHT))
                                .show(panel,|ui|{
                                    // The React source displays ALL shipments
                                    // as a spreadsheet with six stable
                                    // recipient columns and product subrows.
                                    ui.set_min_width(theme::MANUAL_TABLE_MIN_WIDTH);
                                    for (index,row) in rows.iter().enumerate() {
                                        let rid=text(row,"id");
                                        egui::Grid::new(("manual-delivery",rid))
                                            .spacing(egui::Vec2::ZERO)
                                            .show(ui,|grid|{
                                                for (label,width) in [
                                                    ("배송",theme::MANUAL_TABLE_DELIVERY_COL),
                                                    ("이름",theme::MANUAL_TABLE_NAME_COL),
                                                    ("주소",theme::MANUAL_TABLE_ADDRESS_COL),
                                                    ("휴대폰",theme::MANUAL_TABLE_PHONE_COL),
                                                    ("전화",theme::MANUAL_TABLE_PHONE_COL),
                                                    ("배송 요청",theme::MANUAL_TABLE_NOTE_COL)
                                                ] {
                                                    components::manual_table_heading(grid,label,width);
                                                }
                                                grid.end_row();
                                                let summary=format!("배송 {}",index+1);
                                                if components::manual_table_cell(
                                                    grid,&summary,theme::MANUAL_TABLE_DELIVERY_COL,
                                                    index==self.selected_recipient
                                                ).clicked(){
                                                    self.select_cell(index,None,"name");
                                                }
                                                for (field,width) in [
                                                    ("name",theme::MANUAL_TABLE_NAME_COL),
                                                    ("address",theme::MANUAL_TABLE_ADDRESS_COL),
                                                    ("mobile",theme::MANUAL_TABLE_PHONE_COL),
                                                    ("phone",theme::MANUAL_TABLE_PHONE_COL),
                                                    ("note",theme::MANUAL_TABLE_NOTE_COL)
                                                ] {
                                                    let selected=index==self.selected_recipient &&
                                                        self.selected_item.is_none() &&
                                                        self.selected_field==field;
                                                    if components::manual_table_cell(
                                                        grid,text(row,field),width,selected
                                                    ).clicked(){
                                                        self.select_cell(index,None,field);
                                                    }
                                                }
                                                grid.end_row();
                                            });
                                        ui.label(typography::field_label("상품"));
                                        egui::Grid::new(("manual-products",rid))
                                            .spacing(egui::Vec2::ZERO)
                                            .show(ui,|grid|{
                                                components::manual_table_heading(
                                                    grid,"상품",theme::MANUAL_TABLE_PRODUCT_COL
                                                );
                                                components::manual_table_heading(
                                                    grid,"수량",theme::MANUAL_TABLE_QUANTITY_COL
                                                );
                                                components::manual_table_heading(
                                                    grid,"행 동작",theme::MANUAL_TABLE_ACTION_COL*2.0
                                                );
                                                grid.end_row();
                                                for item in list(row,"items"){
                                                    let item_id=text(item,"id");
                                                    let selected=index==self.selected_recipient &&
                                                        self.selected_item.as_deref()==Some(item_id);
                                                    if components::manual_table_cell(
                                                        grid,text(item,"name"),
                                                        theme::MANUAL_TABLE_PRODUCT_COL,
                                                        selected && self.selected_field=="product"
                                                    ).clicked() {
                                                        self.select_cell(index,Some(item_id.into()),"product");
                                                    }
                                                    let quantity=item.get("quantity")
                                                        .and_then(Value::as_i64)
                                                        .map(|n|n.to_string()).unwrap_or_default();
                                                    if components::manual_table_cell(
                                                        grid,&quantity,
                                                        theme::MANUAL_TABLE_QUANTITY_COL,
                                                        selected && self.selected_field=="quantity"
                                                    ).clicked() {
                                                        self.select_cell(index,Some(item_id.into()),"quantity");
                                                    }
                                                    grid.horizontal(|actions|{
                                                        if theme::action(actions,"검색",
                                                            ButtonRole::Standard,!busy).clicked(){
                                                            lookup=Some((rid.to_owned(),item_id.to_owned()));
                                                        }
                                                        if theme::action(actions,"상품 제거",
                                                            ButtonRole::Quiet,!busy).clicked(){
                                                            chosen=Some(("remove_manual_item".into(),json!({
                                                                "recipientId":rid,"itemId":item_id
                                                            })));
                                                        }
                                                    });
                                                    grid.end_row();
                                                }
                                            });
                                        if theme::action(ui,"상품 추가",ButtonRole::Standard,
                                            !busy).clicked(){
                                            chosen=Some(("add_manual_item".into(),json!({
                                                "recipientId":rid,"itemId":unique_id()
                                            })));
                                        }
                                        if index==self.selected_recipient {
                                            let origin=text(row,"dispatchOrigin");
                                            let origin=if origin=="STORE"{"STORE"}else{"FACTORY"};
                                            let code=text(row,"freightCode");
                                            let code=if code=="020"{"020"}else{"010"};
                                            let remote=row.get("remoteDelivery")
                                                .and_then(|v|v.get("checked"))
                                                .and_then(Value::as_bool).unwrap_or(false);
                                            let vat=row.get("vatOverride")
                                                .and_then(Value::as_bool).unwrap_or(false);
                                            ui.horizontal_wrapped(|options|{
                                                options.spacing_mut().item_spacing.x=theme::SPACE_8;
                                                options.label(typography::field_label("발송지"));
                                                for (label,value) in [
                                                    ("공장","FACTORY"),("매장","STORE")
                                                ]{
                                                    if theme::action(
                                                        options,label,
                                                        if origin==value{ButtonRole::TableSecondary}
                                                        else{ButtonRole::Standard},!busy
                                                    ).clicked(){
                                                        chosen=Some((
                                                            "set_manual_delivery_option".into(),
                                                            json!({"cell":{"recipientId":rid,
                                                                "field":"dispatchOrigin"},
                                                                "value":value})
                                                        ));
                                                    }
                                                }
                                                for (label,value) in [
                                                    ("선불","010"),("착불","020")
                                                ]{
                                                    if theme::action(options,label,
                                                        if code==value{ButtonRole::TableSecondary}
                                                        else{ButtonRole::Standard},!busy).clicked(){
                                                        chosen=Some((
                                                            "set_manual_delivery_option".into(),
                                                            json!({"cell":{"recipientId":rid,
                                                                "field":"freightCode"},
                                                                "value":value})
                                                        ));
                                                    }
                                                }
                                            });
                                            ui.horizontal_wrapped(|options|{
                                                options.spacing_mut().item_spacing.x=theme::SPACE_8;
                                                if theme::action(options,
                                                    if remote{"도서산간 적용"}else{"도서산간 미적용"},
                                                    ButtonRole::Standard,!busy
                                                ).clicked(){
                                                    chosen=Some((
                                                        "set_manual_delivery_option".into(),
                                                        json!({"cell":{"recipientId":rid,
                                                            "field":"remoteDelivery"},
                                                            "value":!remote})
                                                    ));
                                                }
                                                if theme::action(options,
                                                    if vat{"부가세 포함"}else{"부가세 별도"},
                                                    ButtonRole::Standard,!busy
                                                ).clicked(){
                                                    chosen=Some((
                                                        "set_manual_delivery_option".into(),
                                                        json!({"cell":{"recipientId":rid,
                                                            "field":"vatOverride"},
                                                            "value":!vat})
                                                    ));
                                                }
                                                if theme::action(options,
                                                    "운임 수정",ButtonRole::Standard,!busy
                                                ).clicked(){
                                                    self.select_cell(index,None,"shippingCharge");
                                                }
                                            });
                                        }
                                        components::soft_divider(ui);
                                    }
                                });
                            // Single selected-field editor; never repeat it
                            // under every shipment or change the panel height.
                            panel.horizontal(|ui|{
                                let label=if self.selected_item.is_some(){
                                    if self.selected_field=="quantity"{"수량"}else{"상품명"}
                                } else {
                                    match self.selected_field.as_str() {
                                        "name"=>"이름","address"=>"주소",
                                        "mobile"=>"휴대폰","phone"=>"전화",
                                        "note"=>"배송 요청",
                                        "shippingCharge"=>"적용 운임",_=>"이름"
                                    }
                                };
                                ui.label(typography::field_label(label));
                                components::flexible_text_field(
                                    ui,&mut self.edit_text,
                                    theme::MANUAL_INPUT_WIDTH,""
                                );
                                if theme::action(ui,"입력 반영",ButtonRole::Standard,
                                    !busy && self.edit_text!=self.current_value()
                                ).clicked(){submit_direct=true;}
                            });
                            });
                        },
                    );
                        });
                    });
                ui.add_space(theme::SPACE_12);
                components::modal_action_row_split(
                    ui,
                    |left|{
                        // Reset is a destructive worksheet operation with its
                        // existing confirmation, not a toolbar filter.
                        if theme::action(left,"초기화",ButtonRole::Danger,!busy)
                            .clicked(){
                            if self.edit_text!=self.current_value(){
                                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
                            }else{
                                self.reset_pending=true;
                            }
                        }
                    },
                    |right|{
                        if theme::action(right,"초안에 적용",
                            ButtonRole::Primary,!busy
                        ).clicked(){apply=true;}
                    }
                );
            });
        // Search opens only for the current native recipient/item identity.
        // The underlying worksheet is disabled while a chooser is open.
        if let Some((recipient_id,item_id))=lookup {
            if self.edit_text!=self.current_value(){
                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
            }else if self.product_search.is_none(){
                self.product_search=Some(ProductSearch::new(
                    recipient_id,item_id,self.client_id.clone().unwrap_or_default(),
                    Arc::clone(&self.gateway),ctx.clone()
                ));
            }
        }
        if self.product_search.is_some() ||
            self.remove_recipient_pending.is_some() ||
            self.reset_pending || self.confirm_discard {
            components::modal_backdrop(
                ctx,"manual-nested-modal-backdrop",true
            );
        }
        let product_event=self.product_search.as_mut().and_then(|search|
            search.show(ctx,Arc::clone(&self.gateway),work_area)
        );
        if let Some(event)=product_event {
            let search=self.product_search.take();
            if let (Some(search),ProductEvent::Chosen(product))=(search,event){
                // Never apply a delayed search result to a removed product
                // row or another shipment; Rust still owns product selection.
                let exists=self.model.as_ref().is_some_and(|model|
                    list(model,"rows").iter().any(|row|
                        text(row,"id")==search.recipient_id &&
                        list(row,"items").iter().any(|item|
                            text(item,"id")==search.item_id
                        )
                    )
                );
                if exists && !self.busy(){
                    let title=if text(&product,"name").is_empty(){
                        text(&product,"code").to_owned()
                    }else{text(&product,"name").to_owned()};
                    self.action("set_manual_product",json!({
                        "cell":{"recipientId":search.recipient_id,
                            "itemId":search.item_id,"field":"product"},
                        "value":title,"product":product
                    }),ctx);
                }
            }
        }

        if submit_direct {
            let rid=self.model.as_ref().and_then(|m|list(m,"rows")
                .get(self.selected_recipient)).and_then(|r|r.get("id"))
                .and_then(Value::as_str).map(str::to_owned);
            if let Some(id)=rid {
                let mut cell=json!({"recipientId":id,"field":self.selected_field});
                if let Some(item)=self.selected_item.as_ref() {
                    cell["itemId"]=json!(item);
                }
                let mut value=json!(self.edit_text.clone());
                let action=if self.selected_field=="quantity" {
                    "native_confirm_quantity"
                }else if self.selected_field=="product"{
                    "set_manual_product"
                }else if self.selected_field=="shippingCharge"{
                    // The UI only parses an entered integer. The existing
                    // Rust rule is the sole authority for 2x normalization,
                    // COD and allowed base freight.
                    let raw=self.edit_text.trim().replace(',',"");
                    if raw.is_empty(){
                        value=Value::Null;
                    }else if let Ok(n)=raw.parse::<i64>(){
                        value=json!(n);
                    }else{
                        self.error=Some("운임은 0 이상의 정수로 입력하세요.".into());
                        return None;
                    }
                    "set_manual_delivery_option"
                }else{
                    "set_manual_recipient_field"
                };
                chosen=Some((action.into(),json!({"cell":cell,"value":value})));
            }
        }
        if let Some((action,payload))=chosen {
            if self.edit_text!=self.current_value() && !submit_direct {
                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
            }else{
                self.action(&action,payload,ctx);
            }
        }
        // React already confirms truncation only when the final shipment
        // holds actual values or source assignments. No popup for empty rows.
        if let Some(target)=self.remove_recipient_pending.clone(){
            let mut confirm=false;
            let mut cancel=false;
            components::decision_window("배송 건수 줄이기","manual-shipment-removal",work_area)
                .constrain_to(work_area)
                .show(ctx,|ui|{
                    cancel=components::modal_header(
                        ui,"배송 건수 줄이기","취소",true
                    );
                    ui.label("마지막 배송 건의 입력값과 원문 배정을 제거할까요?");
                    components::modal_action_row(ui,|actions|{
                        if theme::action(actions,"확인",ButtonRole::Danger,true).clicked(){
                            confirm=true;
                        }
                    });
                });
            if cancel {self.remove_recipient_pending=None;}
            if confirm {
                let count=self.model.as_ref().map(|m|list(m,"rows").len()).unwrap_or(0);
                let unchanged=self.model.as_ref().and_then(|m|list(m,"rows").last())
                    .is_some_and(|row|text(row,"id")==target);
                self.remove_recipient_pending=None;
                if unchanged && count>1 && !self.busy() &&
                   self.edit_text==self.current_value() {
                    self.action("resize_manual_rows",json!({
                        "count":count-1,"newRecipientIds":[]
                    }),ctx);
                }else{
                    self.error=Some("삭제 대상이 변경됐습니다. 현재 배송 목록을 확인하세요.".into());
                }
            }
        }
        if self.reset_pending {
            let mut confirm=false;
            let mut cancel=false;
            components::decision_window("전체 초기화","manual-reset-confirm",work_area)
                .constrain_to(work_area)
                .show(ctx,|ui|{
                    cancel=components::modal_header(
                        ui,"전체 초기화","취소",true
                    );
                    ui.label("구성표와 배정을 초기화할까요? 최초 원문과 마스터는 보존됩니다.");
                    confirm=components::modal_action_row(ui,|actions|{
                            theme::action(
                                actions,"확인",ButtonRole::Primary,true
                            ).clicked()
                        }
                    ).inner;
                });
            if cancel{self.reset_pending=false;}
            if confirm {
                self.reset_pending=false;
                self.action("reset_manual_rows",json!({
                    "recipientId":unique_id()
                }),ctx);
            }
        }
        if apply {
            if self.edit_text!=self.current_value() {
                self.error=Some("아직 반영되지 않은 입력 내용이 있습니다.".into());
            }else{
                self.apply(ctx);
            }
        }
        if discard || !open {
            // Dirty typed text is just as important as a completed Rust edit.
            // A parent close cannot destroy an active modal/async operation.
            if self.busy() || self.product_search.is_some() ||
               self.remove_recipient_pending.is_some() || self.reset_pending {
                return None;
            }
            if self.dirty || self.edit_text!=self.current_value() {
                self.confirm_discard=true;
            } else {
                return Some(Event::Closed);
            }
        }
        if self.confirm_discard {
            let mut confirm=true;
            components::decision_window("수동 분류 변경 취소","manual-discard-prompt",work_area)
                .constrain_to(work_area)
                .show(ctx,|ui|{
                    if components::modal_header(
                        ui,"수동 분류 변경 취소","취소",true
                    ){confirm=false;}
                    ui.label("적용하지 않은 수동 분류 변경사항을 버릴까요?");
                    components::modal_action_row(ui,|actions|{
                        if theme::action(actions,"변경 취소",ButtonRole::Standard,true).clicked(){
                            discard_confirmed=true;
                        }
                    });
                });
            if discard_confirmed {return Some(Event::Closed);}
            if !confirm {self.confirm_discard=false;}
        }
        None
    }
}
