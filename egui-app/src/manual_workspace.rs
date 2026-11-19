//! egui manual source assignment, powered ONLY by the existing Rust
//! apply_manual_classification reducer. No second classification algorithm.
//!
//! This first stage provides source-block ownership, row/item structure,
//! quantity validation and applying an unconfirmed preview. The chooser never
//! edits a confirmed order or directly writes the Tauri business database.

use std::{sync::{Arc,mpsc::{self,Receiver}},thread,time::{SystemTime,UNIX_EPOCH},sync::atomic::{AtomicU64,Ordering}};
use eframe::egui::{self,Context};
use serde_json::{json,Value};
use crate::{gateway::{BandoGateway,ReadOperation},preview_store::WorkingReception,theme::{self,ButtonRole},typography};

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
            receiver:Some(rx),error:None,dirty:false,applied:None,
        }
    }
    fn busy(&self)->bool {self.receiver.is_some()}
    fn poll(&mut self) {
        let received=self.receiver.as_ref().map(|r|r.try_recv());
        match received {
            Some(Ok(Ok(Outcome::Initial{order,model,assignment_order})))=>{
                self.base_order=Some(order);
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.receiver=None;
                self.edit_text=self.current_value();
            }
            Some(Ok(Ok(Outcome::Updated{model,assignment_order})))=>{
                self.model=Some(model);
                self.assignment_order=assignment_order;
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
                    "action":action,"model":model,"assignmentOrder":keys,
                    "order":order
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
                        "assignmentOrder":keys
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
                    "action":"manual_quantity_status","model":model
                }))?;
                if quantity.get("pending").and_then(Value::as_bool)==Some(true) {
                    return Err("선택한 상품의 수량을 먼저 확정하세요.".into());
                }
                let applied=rule(service.as_ref(),json!({
                    "action":"apply_manual","order":order,
                    "model":model,"assignmentOrder":keys
                }))?;
                let order=applied.get("order").filter(|v|v.is_object())
                    .ok_or("Rust 수동 분류 적용 결과가 올바르지 않습니다.")?.clone();
                Ok(Outcome::Applied{order,model,assignment_order:keys})
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn show(&mut self,ctx:&Context)->Option<Event> {
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
        egui::Window::new("수동 분류")
            .id(egui::Id::new("bando-native-manual-classification"))
            .open(&mut open)
            .collapsible(false).resizable(true)
            .default_width(1050.0).default_height(650.0)
            .min_width(700.0).min_height(420.0)
            .show(ctx,|ui|{
                if let Some(error)=self.error.as_ref() {
                    ui.label(typography::status(error,theme::DANGER));
                }
                if self.busy() {
                    ui.label(typography::caption("Rust에서 수동 분류를 처리하고 있습니다."));
                }
                let Some(model)=self.model.as_ref() else {return};
                let rows=list(model,"rows").to_vec();
                let blocks=list(model,"blocks").to_vec();
                let assignments=model.get("assignments").cloned().unwrap_or_else(||json!({}));
                let busy=self.busy();
                let unassigned=blocks.iter().filter(|b|
                    assignments.get(text(b,"id")).is_none()).count();
                ui.label(typography::caption(
                    &format!("미배정 원문 {unassigned}개 · 배송 {}건",rows.len())
                ));
                ui.separator();
                ui.columns(2,|cols|{
                    cols[0].label(typography::subsection_heading("붙여넣은 내용"));
                    egui::ScrollArea::vertical().id_salt("manual-source-blocks")
                        .max_height(465.0).show(&mut cols[0],|ui|{
                            for block in &blocks {
                                let id=text(block,"id");
                                let fragment=text(block,"text");
                                let assigned=assignments.get(id).is_some();
                                let prefix=if assigned {"배정됨"} else {"미배정"};
                                let label=format!("{prefix} · {}",
                                    if fragment.is_empty(){"(빈 원문)"}else{fragment});
                                if theme::action(ui,&label,if assigned {
                                    ButtonRole::ActiveNavigation
                                }else{ButtonRole::Quiet},!busy).clicked() {
                                    if assigned {
                                        chosen=Some(("release_manual_block".into(),json!({
                                            "id":id,"clear":true
                                        })));
                                    }else{
                                        let recipient=rows.get(self.selected_recipient);
                                        if let Some(row)=recipient {
                                            let mut cell=json!({
                                                "recipientId":row.get("id"),
                                                "field":self.selected_field
                                            });
                                            if let Some(item_id)=self.selected_item.as_ref() {
                                                cell["itemId"]=json!(item_id);
                                            }
                                            chosen=Some(("assign_manual_block".into(),json!({
                                                "id":id,"cell":cell
                                            })));
                                        }
                                    }
                                }
                            }
                            if blocks.is_empty(){
                                ui.label(typography::caption(
                                    "Rust가 분리한 원문 블록이 없습니다."
                                ));
                            }
                        });
                    cols[1].label(typography::subsection_heading("배송 정보 구성표"));
                    cols[1].horizontal(|ui|{
                        if theme::action(ui,"배송 추가",ButtonRole::Standard,
                            !busy && rows.len()<100).clicked() {
                            chosen=Some(("resize_manual_rows".into(),json!({
                                "count":rows.len()+1,
                                "newRecipientIds":[unique_id()]
                            })));
                        }
                        if theme::action(ui,"마지막 배송 제거",ButtonRole::Quiet,
                            !busy && rows.len()>1).clicked() {
                            // A destructive operation is never implicit.
                            self.error=Some(
                                "배송 제거는 입력 내용 확인 기능을 이전한 뒤 사용할 수 있습니다.".into()
                            );
                        }
                    });
                    egui::ScrollArea::vertical().id_salt("manual-recipients-list")
                        .max_height(445.0).show(&mut cols[1],|ui|{
                            for (index,row) in rows.iter().enumerate() {
                                let title=format!("배송 {} · {}",
                                    index+1,if text(row,"name").is_empty(){
                                        "수취인 미지정"
                                    }else{text(row,"name")});
                                if theme::action(ui,&title,
                                    if index==self.selected_recipient{
                                        ButtonRole::ActiveNavigation
                                    }else{ButtonRole::Quiet},true).clicked(){
                                    if self.edit_text==self.current_value() {
                                        self.selected_recipient=index;
                                        self.selected_item=None;
                                        self.selected_field="name".into();
                                        self.edit_text=self.current_value();
                                    } else {
                                        self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
                                    }
                                }
                                if index!=self.selected_recipient {continue;}
                                for (field,label) in [
                                    ("name","이름"),("address","주소"),
                                    ("mobile","휴대전화"),("phone","전화"),
                                    ("note","배송 요청")
                                ] {
                                    ui.horizontal(|ui|{
                                        let active=self.selected_item.is_none() &&
                                            self.selected_field==field;
                                        if theme::action(ui,label,if active{
                                            ButtonRole::ActiveNavigation
                                        }else{ButtonRole::Quiet},true).clicked(){
                                            if self.edit_text==self.current_value() {
                                                self.selected_item=None;
                                                self.selected_field=field.into();
                                                self.edit_text=self.current_value();
                                            } else {
                                                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
                                            }
                                        }
                                        ui.label(typography::body(
                                            if text(row,field).is_empty(){
                                                "미입력"
                                            }else{text(row,field)}
                                        ));
                                    });
                                }
                                for item in list(row,"items") {
                                    let item_id=text(item,"id");
                                    ui.horizontal_wrapped(|ui|{
                                        if theme::action(ui,"상품",if self.selected_item
                                            .as_deref()==Some(item_id)&&self.selected_field=="product" {
                                            ButtonRole::ActiveNavigation
                                        }else{ButtonRole::Quiet},true).clicked(){
                                            if self.edit_text==self.current_value() {
                                                self.selected_item=Some(item_id.into());
                                                self.selected_field="product".into();
                                                self.edit_text=self.current_value();
                                            } else {
                                                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
                                            }
                                        }
                                        ui.label(typography::body(
                                            if text(item,"name").is_empty(){
                                                "미지정 상품"
                                            }else{text(item,"name")}
                                        ));
                                        if theme::action(ui,"수량",if self.selected_item
                                            .as_deref()==Some(item_id)&&self.selected_field=="quantity" {
                                            ButtonRole::ActiveNavigation
                                        }else{ButtonRole::Quiet},true).clicked(){
                                            if self.edit_text==self.current_value() {
                                                self.selected_item=Some(item_id.into());
                                                self.selected_field="quantity".into();
                                                self.edit_text=self.current_value();
                                            } else {
                                                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
                                            }
                                        }
                                        let quantity=item.get("quantity")
                                            .and_then(Value::as_i64).map(|x|x.to_string())
                                            .unwrap_or_else(||"미확정".into());
                                        ui.label(typography::quantity(&quantity));
                                    });
                                }
                                ui.separator();
                                ui.label(typography::caption("선택한 필드 직접 입력"));
                                ui.horizontal(|ui|{
                                    let label=if self.selected_item.is_some() {
                                        if self.selected_field=="quantity"{"수량"}else{"상품명"}
                                    }else{self.selected_field.as_str()};
                                    ui.label(typography::caption(label));
                                    ui.add(egui::TextEdit::singleline(&mut self.edit_text)
                                        .desired_width(225.0));
                                    if theme::action(ui,"입력 반영",ButtonRole::Standard,
                                        !busy && self.edit_text!=self.current_value()
                                    ).clicked(){submit_direct=true;}
                                });
                                if theme::action(ui,"상품 추가",ButtonRole::Standard,!busy)
                                    .clicked(){
                                    chosen=Some(("add_manual_item".into(),json!({
                                        "recipientId":row.get("id"),"itemId":unique_id()
                                    })));
                                }
                                ui.separator();
                            }
                        });
                });
                ui.separator();
                ui.horizontal(|ui|{
                    if theme::action(ui,"초안에 적용",ButtonRole::Primary,!busy).clicked(){
                        apply=true;
                    }
                    if theme::action(ui,"닫기",ButtonRole::Quiet,!busy).clicked(){
                        discard=true;
                    }
                    ui.label(typography::caption(
                        if self.dirty {
                            "변경 중 · 적용 전까지 기존 발주서는 유지됩니다."
                        }else{
                            "원문 블록 클릭 → 선택된 배송 필드에 배정"
                        }
                    ));
                });
            });
        if submit_direct {
            let rid=self.model.as_ref().and_then(|m|list(m,"rows")
                .get(self.selected_recipient)).and_then(|r|r.get("id"))
                .and_then(Value::as_str).map(str::to_owned);
            if let Some(id)=rid {
                let mut cell=json!({"recipientId":id,"field":self.selected_field});
                if let Some(item)=self.selected_item.as_ref() {
                    cell["itemId"]=json!(item);
                }
                let value=self.edit_text.clone();
                let action=if self.selected_field=="quantity" {
                    "native_confirm_quantity"
                }else if self.selected_field=="product"{
                    "set_manual_product"
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
        if apply {
            if self.edit_text!=self.current_value() {
                self.error=Some("아직 반영되지 않은 입력 내용이 있습니다.".into());
            }else{
                self.apply(ctx);
            }
        }
        if discard || !open {
            if self.dirty {
                self.confirm_discard=true;
            } else {
                return Some(Event::Closed);
            }
        }
        if self.confirm_discard {
            let mut confirm=true;
            egui::Window::new("수동 분류 변경 취소")
                .id(egui::Id::new("manual-discard-prompt"))
                .open(&mut confirm)
                .collapsible(false).resizable(false)
                .show(ctx,|ui|{
                    ui.label("적용하지 않은 수동 분류 변경사항을 버릴까요?");
                    ui.horizontal(|ui|{
                        if ui.button("변경 취소").clicked(){discard_confirmed=true;}
                        if ui.button("계속 편집").clicked(){self.confirm_discard=false;}
                    });
                });
            if discard_confirmed {return Some(Event::Closed);}
            if !confirm {self.confirm_discard=false;}
        }
        None
    }
}
