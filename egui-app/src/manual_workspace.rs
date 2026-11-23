//! egui manual source assignment, powered ONLY by the existing Rust
//! apply_manual_classification reducer. No second classification algorithm.
//!
//! This first stage provides source-block ownership, row/item structure,
//! quantity validation and applying an unconfirmed preview. The chooser never
//! edits a confirmed order or directly writes the Tauri business database.

use std::{sync::{Arc,mpsc::{self,Receiver}},thread,time::{SystemTime,UNIX_EPOCH},sync::atomic::{AtomicU64,Ordering}};
use eframe::egui::{self,Context};
use serde_json::{json,Value};
use crate::{
    gateway::{BandoGateway,ReadOperation},
    preview_store::WorkingReception,
    theme::{self,ButtonRole},
    typography,components,source_highlight,
    manual_product_search::{ProductSearch,Event as ProductEvent},
};

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

/// Each decision is computed by native Rust off the egui frame and is
/// paired with both recipient ID and exact address to reject stale matches.
type RemoteRow=(String,String,Result<Value,String>);

#[derive(Clone)]
struct ManualHistoryFrame{
    model:Value,
    assignment_order:Vec<String>,
    remote_rows:Vec<RemoteRow>,
    selected_recipient:usize,
    selected_item:Option<String>,
    selected_field:String,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum ManualNavigation{
    Next,
    Previous,
    RowUp,
    RowDown,
}

#[derive(Clone,Debug,PartialEq,Eq)]
struct ManualCellTarget{
    row:usize,
    item:Option<String>,
    field:String,
}

fn item_quantity_pending(item:&Value)->bool{
    let has_product=item.get("productId").is_some_and(|value|!value.is_null()) ||
        !text(item,"name").trim().is_empty();
    let confirmed=item.get("quantityConfirmed").and_then(Value::as_bool)==Some(true);
    has_product && !confirmed
}

fn model_pending_quantity_count(model:&Value)->usize{
    list(model,"rows").iter().map(|row|
        list(row,"items").iter().filter(|item|item_quantity_pending(item)).count()
    ).sum()
}

fn model_has_pending_quantity(model:&Value)->bool{
    model_pending_quantity_count(model)>0
}

/// While a product has just been selected, React locks every other cell
/// until its quantity is confirmed. Keep that lock tied to the exact cell.
fn manual_cell_allowed(
    pending:Option<&ManualCellTarget>,target:&ManualCellTarget
)->bool{
    pending.is_none_or(|cell|cell==target)
}

/// React's useBlock permits releasing an assignment on the same pending item,
/// or assigning that item's quantity block. It rejects all other source moves.
fn manual_source_assignment_allowed(
    assigned:bool,
    assigned_item:Option<&str>,
    pending_item:Option<&str>,
    source_field:&str,
)->bool{
    let Some(pending_item)=pending_item else{return true};
    if assigned {
        assigned_item==Some(pending_item)
    }else{
        source_field=="quantity"
    }
}

fn push_manual_history(
    history:&mut Vec<ManualHistoryFrame>,frame:ManualHistoryFrame
){
    history.push(frame);
    if history.len()>100 {history.remove(0);}
}

fn manual_cell_targets(model:&Value)->Vec<ManualCellTarget>{
    let mut cells=Vec::new();
    for (row_index,row) in list(model,"rows").iter().enumerate(){
        for field in ["name","address","mobile","phone","note"]{
            cells.push(ManualCellTarget{
                row:row_index,item:None,field:field.into()
            });
        }
        for item in list(row,"items"){
            let id=text(item,"id").to_owned();
            for field in ["product","quantity","unitPrice"]{
                cells.push(ManualCellTarget{
                    row:row_index,item:Some(id.clone()),field:field.into()
                });
            }
        }
    }
    cells
}

fn resolve_remote_rows(gateway:&dyn BandoGateway,model:&Value)->Vec<RemoteRow>{
    list(model,"rows").iter().map(|row|{
        let id=text(row,"id").to_owned();
        let address=text(row,"address").to_owned();
        let decision=if address.trim().is_empty(){Ok(Value::Null)}
        else{gateway.execute(ReadOperation::ResolveRemoteDeliveryArea,json!({
            "recipient":row
        }))};
        (id,address,decision)
    }).collect()
}
fn remote_for<'a>(rows:&'a [RemoteRow],recipient:&Value)
    ->Option<&'a Result<Value,String>>
{
    let id=text(recipient,"id");
    let address=text(recipient,"address");
    rows.iter().find(|(key,source,_)|key==id && source==address)
        .map(|(_,_,decision)|decision)
}

enum Outcome {
    Initial { order:Value, model:Value, assignment_order:Vec<String>,
        remote_rows:Vec<RemoteRow> },
    Updated { model:Value, assignment_order:Vec<String>,
        remote_rows:Vec<RemoteRow> },
    Applied { order:Value, model:Value, assignment_order:Vec<String>,
        quote:Option<Value>,quote_error:Option<String> },
}
pub enum Event {
    Closed,
    Applied { order_id:String,raw:String,order_type:String,client_id:Option<String>,
        model:Value,assignment_order:Vec<String>,order:Value,
        quote:Option<Value>,quote_error:Option<String> },
}

/// A direct in-place edit from the reception pane. The visible cell owns
/// focus and typing; the Rust business reducer remains the only mutation
/// authority. No modal or UI-side price/quantity calculation is used.
#[derive(Clone,Debug)]
pub struct InlineRequest {
    pub recipient_id:String,
    pub item_id:Option<String>,
    pub field:String,
    pub value:Value,
}
/// Native pricing depends on these ordered line/recipient values and
/// shipment/VAT options, NOT on recipient name, phone, note or OCR source
/// assignment metadata. This is a comparison of Rust business inputs only;
/// no monetary arithmetic is performed in the UI.
fn price_input_snapshot(order:&Value)->Value{
    let root=|name:&str|order.get(name).cloned().unwrap_or(Value::Null);
    let recipients=list(order,"recipients").iter().map(|recipient|{
        let get=|name:&str|recipient.get(name).cloned().unwrap_or(Value::Null);
        let items=list(recipient,"items").iter().map(|item|{
            let item_get=|name:&str|item.get(name).cloned().unwrap_or(Value::Null);
            json!({
                "id":item_get("id"),"productId":item_get("productId"),
                "variantId":item_get("variantId"),"quantity":item_get("quantity"),
                "unitPrice":item_get("unitPrice"),
                "manualUnitPrice":item_get("manualUnitPrice"),
                "origin":item_get("origin"),"freightCode":item_get("freightCode"),
                "shippingGroupId":item_get("shippingGroupId")
            })
        }).collect::<Vec<_>>();
        json!({
            "id":get("id"),"address":get("address"),
            "dispatchOrigin":get("dispatchOrigin"),
            "freightCode":get("freightCode"),
            "shippingCharges":get("shippingCharges"),
            "shippingCharge":get("shippingCharge"),
            "remoteDelivery":get("remoteDelivery"),
            "vatOverride":get("vatOverride"),
            "vatRateBps":get("vatRateBps"),
            "items":items
        })
    }).collect::<Vec<_>>();
    json!({
        "applyVat":root("applyVat"),
        "vatRateBps":root("vatRateBps"),
        "freightCode":root("freightCode"),
        "recipients":recipients
    })
}

/// Convert a previously calculated, complete native quote back to the
/// worker response shape. No rules are duplicated and no amounts changed.
/// Explicit edits that affect any pricing input always call native Rust.
fn reuse_native_quote(saved:Option<&Value>,updated:&Value)->Option<Value>{
    let saved=saved?;
    if !saved.get("authorityRevision").is_some_and(Value::is_u64) ||
       !saved.get("pendingRecipients").is_some_and(Value::is_array) ||
       !saved.get("freightLines").is_some_and(Value::is_array) ||
       !saved.get("linePrices").and_then(Value::as_array)
           .is_some_and(|lines|lines.iter().all(|row|row.get("amount").is_some())) {
        return None;
    }
    let mut order=updated.clone();
    order["shippingFee"]=saved.get("shippingFee").cloned().unwrap_or(Value::Null);
    order["shippingVersion"]=saved.get("shippingVersion").cloned().unwrap_or(Value::Null);
    Some(json!({
        "order":order,"authorityRevision":saved["authorityRevision"],
        "pricingBasis":saved.get("pricingBasis"),
        "totals":{
            "subtotal":saved.get("subtotal"),
            "vat":saved.get("vatAmount"),"total":saved.get("total"),
            "pendingRecipients":saved.get("pendingRecipients")
        },
        "linePrices":saved.get("linePrices"),
        "freightLines":saved.get("freightLines")
    }))
}

pub fn apply_inline(
    gateway:&dyn BandoGateway,source:&WorkingReception,change:&InlineRequest
)->Result<Event,String>{
    let evidence=source.analysis_evidence.as_ref()
        .ok_or("최초 Rust 인식 결과가 없습니다.")?;
    let order=native_order(gateway,source,evidence)?;
    let previous=source.manual_model.as_ref();
    let initialized=rule(gateway,json!({
        "action":"initial_manual","order":&order,
        "assignmentOrder":previous.and_then(|v|v.get("assignmentOrder"))
            .cloned().unwrap_or_else(||json!([]))
    }))?;
    let mut model=previous.and_then(|v|v.get("model")).filter(|v|v.is_object())
        .cloned().or_else(||initialized.get("model").cloned())
        .ok_or("수취인 편집 작업본이 없습니다.")?;
    let keys=previous.and_then(|v|v.get("assignmentOrder"))
        .or_else(||initialized.get("assignmentOrder"))
        .and_then(Value::as_array).cloned().unwrap_or_default();
    let mut cell=json!({"recipientId":change.recipient_id,"field":change.field});
    if let Some(item)=change.item_id.as_deref(){cell["itemId"]=json!(item);}
    let mut action=if change.item_id.is_some(){
        match change.field.as_str(){
            "quantity"=>"native_confirm_quantity",
            "approveOne"=>"native_approve_one",
            "cancelApproval"=>"native_cancel_approval",
            "unitPrice"=>"set_manual_unit_price",
            "product"=>"set_manual_product",
            "removeItem"=>"remove_manual_item",
            _=>return Err("지원하지 않는 품목 입력 항목입니다.".into())
        }
    }else{
        match change.field.as_str(){
            "name"|"address"|"mobile"|"phone"|"note"=>"set_manual_recipient_field",
            "shippingCharge"|"dispatchOrigin"|"freightCode"|
            "remoteDelivery"|"vatOverride"=>"set_manual_delivery_option",
            "vatSettings"=>"set_manual_vat_settings",
            "addItem"=>"add_manual_item",
            _=>return Err("지원하지 않는 수취인 입력 항목입니다.".into())
        }
    };
    let mut payload=json!({
        "action":action,"order":&order,"model":&model,
        "assignmentOrder":&keys,"cell":cell,"value":change.value
    });
    if action=="add_manual_item"{
        payload["recipientId"]=json!(change.recipient_id);
        payload["itemId"]=json!(unique_id());
    }
    if action=="remove_manual_item"{
        payload["recipientId"]=json!(change.recipient_id);
        payload["itemId"]=json!(change.item_id.as_deref()
            .ok_or("삭제할 품목을 확인하세요.")?);
    }
    if action=="set_manual_product"{
        // React's product dialog sends a verified catalog row rather than
        // free text. Rust owns the independent variant, pricing and model.
        // An existing row retains its confirmed quantity and original source.
        if let Some(product)=change.value.get("product").filter(|v|v.is_object()){
            let title=product.get("name").and_then(Value::as_str)
                .or_else(||product.get("code").and_then(Value::as_str))
                .filter(|name|!name.trim().is_empty())
                .ok_or("선택한 상품에 이름이 없습니다.")?.to_owned();
            payload["value"]=json!(title);
            payload["product"]=product.clone();
            payload["preserveExistingQuantity"]=json!(true);
        }
        // Follow the original native editor's catalog/recognition path.
        let state=if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|value|!value.is_empty()){
            gateway.load_business_state()?
        }else{gateway.load_catalog()?};
        payload["state"]=state;
        payload["candidateIds"]=json!((0..20).map(|_|unique_id()).collect::<Vec<_>>());
    }
    let response=if action=="native_approve_one" || action=="native_cancel_approval"{
        let rid=change.recipient_id.as_str();
        let iid=change.item_id.as_deref()
            .ok_or("수량 승인 대상 품목이 없습니다.")?;
        let chosen=model["rows"].as_array_mut()
            .and_then(|rows|rows.iter_mut().find(|row|
                text(row,"id")==rid))
            .and_then(|row|row["items"].as_array_mut())
            .and_then(|items|items.iter_mut().find(|item|
                text(item,"id")==iid))
            .ok_or("수량 승인 대상이 최신 주문에 없습니다.")?;
        if action=="native_approve_one"{
            // The authoritative Rust issue predicate decides eligibility.
            // Copy its exact approved item (including quantityApproval)
            // into the native manual model; never emulate approval in UI.
            let actor=gateway.load_settings().ok()
                .and_then(|settings|settings.pointer("/settings/operatorName")
                    .and_then(Value::as_str).map(str::trim)
                    .filter(|name|!name.is_empty()).map(str::to_owned))
                .unwrap_or_else(||"사용자".to_owned());
            let approved=gateway.execute(ReadOperation::ApplyIssueAction,json!({
                "action":"approve_missing_quantity","order":&order,
                "recipientId":rid,"itemId":iid,
                "actor":actor,"now":chrono::Utc::now().to_rfc3339()
            }))?;
            let approved_item=approved["order"]["recipients"].as_array()
                .into_iter().flatten().find(|row|text(row,"id")==rid)
                .and_then(|row|row["items"].as_array())
                .into_iter().flatten().find(|item|text(item,"id")==iid)
                .ok_or("Rust 수량 승인 결과가 현재 품목과 일치하지 않습니다.")?;
            *chosen=approved_item.clone();
        }else{
            // React cancels the provisional decision by deleting only its
            // approval record; the existing quantity stays unconfirmed.
            if chosen.get("quantityApproval").is_none_or(Value::is_null){
                return Err("취소할 잠정 승인 기록이 없습니다.".into());
            }
            chosen.as_object_mut().ok_or("품목 형식이 올바르지 않습니다.")?
                .remove("quantityApproval");
        }
        json!({"model":model})
    }else if action=="native_confirm_quantity"{
        let first=rule(gateway,json!({
            "action":"set_manual_quantity_draft","model":model,
            "cell":payload["cell"],"value":payload["value"]
        }))?;
        rule(gateway,json!({
            "action":"confirm_manual_quantity","model":first["model"],
            "cell":payload["cell"],"assignmentOrder":&keys
        }))?
    }else{
        rule(gateway,payload)?
    };
    model=response.get("model").filter(|v|v.is_object())
        .ok_or("Rust 직접 수정 결과가 올바르지 않습니다.")?.clone();
    let status=rule(gateway,json!({
        "action":"manual_quantity_status","model":&model
    }))?;
    if status["pending"]==true{
        // Editing the next cell must not silently commit a partly confirmed
        // quantity. The draft still lives only in the native edit projection.
        // Other existing quantity issues can be resolved one field at a time.
    }
    let applied=rule(gateway,json!({
        "action":"preview_manual","order":&order,"model":&model,
        "assignmentOrder":&keys
    }))?;
    let updated=applied.get("order").filter(|v|v.is_object())
        .ok_or("Rust 수정 적용 결과에 주문 데이터가 없습니다.")?.clone();
    let (quote,quote_error)=if let Some(id)=source.client_id.as_deref(){
        let result=(||->Result<Value,String>{
            let state=gateway.load_business_state()?;
            let client=state["clients"].as_array().and_then(|clients|
                clients.iter().find(|client|client["id"].as_str()==Some(id))
            ).ok_or("거래처의 가격 기준이 없습니다.")?;
            let basis=crate::pricing_basis::snapshot(&state,client,&updated);
            // The native PRICE MASTER must also be unchanged. An unrelated
            // database revision does not alter this order-scoped dependency.
            if price_input_snapshot(&order)==price_input_snapshot(&updated) &&
               source.pricing_preview.as_ref().is_some_and(|saved|
                   saved.get("pricingBasis")==Some(&basis)) {
                if let Some(reused)=reuse_native_quote(
                    source.pricing_preview.as_ref(),&updated
                ){return Ok(reused);}
            }
            let mut projected=gateway.execute(
                ReadOperation::CalculateReceptionQuote,
                json!({"order":&updated,"state":&state,"client":client})
            )?;
            projected["authorityRevision"]=state.get("revision").cloned()
                .unwrap_or(Value::Null);
            let quoted_basis=crate::pricing_basis::snapshot(
                &state,client,projected.get("order").unwrap_or(&updated)
            );
            projected["pricingBasis"]=quoted_basis;
            Ok(projected)
        })();
        match result{
            Ok(quote)=>(Some(quote),None),
            Err(error)=>(None,Some(format!("Rust 재계산 실패: {error}")))
        }
    }else{(None,None)};
    Ok(Event::Applied{
        order_id:source.id.clone(),raw:source.raw.clone(),
        order_type:source.order_type.clone(),client_id:source.client_id.clone(),
        model,assignment_order:keys.iter().filter_map(Value::as_str)
            .map(str::to_owned).collect(),
        order:updated,quote,quote_error
    })
}

pub struct ManualWorkspace {
    pub order_id:String,
    raw:String,
    order_type:String,
    client_id:Option<String>,
    gateway:Arc<dyn BandoGateway>,
    base_order:Option<Value>,
    cached_quote:Option<Value>,
    model:Option<Value>,
    assignment_order:Vec<String>,
    remote_rows:Vec<RemoteRow>,
    selected_recipient:usize,
    selected_item:Option<String>,
    selected_field:String,
    edit_text:String,
    focus_editor:bool,
    history:Vec<ManualHistoryFrame>,
    advance_after_apply:Option<ManualNavigation>,
    last_action:Option<String>,
    action_pending:bool,
    confirm_discard:bool,
    complete_pending:bool,
    product_search:Option<ProductSearch>,
    remove_recipient_pending:Option<String>,
    reset_pending:bool,
    receiver:Option<Receiver<Result<Outcome,String>>>,
    error:Option<String>,
    dirty:bool,
    applied:Option<(Value,Option<Value>,Option<String>)>,
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
                let remote_rows=resolve_remote_rows(service.as_ref(),&model);
                Ok(Outcome::Initial{
                    order,model,assignment_order:keys,remote_rows
                })
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
        Self {
            order_id:source.id,raw:source.raw,order_type:source.order_type,
            client_id:source.client_id,gateway,base_order:None,
            cached_quote:source.pricing_preview,model:None,
            assignment_order:Vec::new(),remote_rows:Vec::new(),
            selected_recipient:0,
            selected_item:None,selected_field:"name".into(),
            edit_text:String::new(),focus_editor:false,history:Vec::new(),
            advance_after_apply:None,last_action:None,action_pending:false,
            confirm_discard:false,complete_pending:false,
            product_search:None,remove_recipient_pending:None,
            reset_pending:false,
            receiver:Some(rx),error:None,dirty:false,applied:None,
        }
    }
    /// Open the existing native editor on the recipient currently selected
    /// in the main intake pane, not silently on the first shipment.
    pub fn focus_recipient(&mut self,index:usize){
        self.focus_cell(index,None,"name");
    }
    pub fn focus_cell(&mut self,index:usize,item:Option<String>,field:&str){
        // Called before the asynchronous native model is initialized. The
        // Initial outcome then loads this exact cell's current value.
        let same_cell=self.selected_recipient==index &&
            self.selected_item.as_ref()==item.as_ref() &&
            self.selected_field==field;
        if !same_cell {self.last_action=None;}
        self.selected_recipient=index;
        self.selected_item=item;
        self.selected_field=field.to_owned();
        self.focus_editor=true;
    }
    fn busy(&self)->bool {self.receiver.is_some()}

    fn nested_modal_window_id(&self)->Option<&'static str>{
        nested_modal_id(
            self.product_search.is_some(),
            self.remove_recipient_pending.is_some(),
            self.reset_pending,
            self.complete_pending,
            self.confirm_discard,
        )
    }

    fn poll(&mut self) {
        let received=self.receiver.as_ref().map(|r|r.try_recv());
        match received {
            Some(Ok(Ok(Outcome::Initial{
                order,model,assignment_order,remote_rows
            })))=>{
                self.base_order=Some(order);
                self.remote_rows=remote_rows;
                let count=list(&model,"rows").len();
                self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.receiver=None;
                self.edit_text=self.current_value();
                self.focus_editor=true;
            }
            Some(Ok(Ok(Outcome::Updated{
                model,assignment_order,remote_rows
            })))=>{
                if let Some(previous)=self.model.as_ref(){
                    push_manual_history(&mut self.history,ManualHistoryFrame{
                        model:previous.clone(),
                        assignment_order:self.assignment_order.clone(),
                        remote_rows:self.remote_rows.clone(),
                        selected_recipient:self.selected_recipient,
                        selected_item:self.selected_item.clone(),
                        selected_field:self.selected_field.clone(),
                    });
                }
                self.remote_rows=remote_rows;
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
                self.action_pending=false;
                self.edit_text=self.current_value();
                if let Some(navigation)=self.advance_after_apply.take(){
                    self.move_selection(navigation);
                }
            }
            Some(Ok(Ok(Outcome::Applied{
                order,model,assignment_order,quote,quote_error
            })))=>{
                self.model=Some(model);
                self.assignment_order=assignment_order;
                self.applied=Some((order,quote,quote_error));
                self.receiver=None;
            }
            Some(Ok(Err(error)))=>{
                self.error=Some(error);
                self.advance_after_apply=None;
                if self.action_pending {self.last_action=None;}
                self.action_pending=false;
                self.receiver=None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected))=>{
                self.error=Some("Rust 수동 분류 처리가 중단되었습니다.".into());
                self.advance_after_apply=None;
                if self.action_pending {self.last_action=None;}
                self.action_pending=false;
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
        let target=ManualCellTarget{
            row,item:item.clone(),field:field.to_owned()
        };
        let pending=self.pending_quantity_cell();
        if !manual_cell_allowed(pending.as_ref(),&target) {
            self.error=Some("선택한 상품의 수량을 먼저 확정하세요.".into());
            return;
        }
        self.selected_recipient=row;
        self.selected_item=item;
        self.selected_field=field.to_owned();
        self.edit_text=self.current_value();
        self.focus_editor=true;
    }

    fn undo(&mut self){
        if self.busy() || self.product_search.is_some() ||
            self.remove_recipient_pending.is_some() || self.reset_pending ||
            self.confirm_discard {return;}
        if self.edit_text!=self.current_value(){
            self.edit_text=self.current_value();
            self.error=None;
            return;
        }
        let cancel_product=matches!(
            self.last_action.as_deref(),
            Some("set_manual_product"|"assign_manual_block")
        );
        let Some(frame)=self.history.pop() else {
            self.error=Some("되돌릴 수 있는 변경사항이 없습니다.".into());
            return;
        };
        self.model=Some(frame.model);
        self.assignment_order=frame.assignment_order;
        self.remote_rows=frame.remote_rows;
        self.selected_recipient=frame.selected_recipient;
        self.selected_item=frame.selected_item;
        self.selected_field=if cancel_product{
            "product".into()
        }else{frame.selected_field};
        let count=self.model.as_ref().map(|model|list(model,"rows").len()).unwrap_or(0);
        self.selected_recipient=self.selected_recipient.min(count.saturating_sub(1));
        if self.selected_item.as_ref().is_some_and(|id|
            !self.model.as_ref().is_some_and(|model|
                list(model,"rows").get(self.selected_recipient).is_some_and(|row|
                    list(row,"items").iter().any(|item|text(item,"id")==id)
                )
            )
        ){
            self.selected_item=None;
            self.selected_field="name".into();
        }
        self.edit_text=self.current_value();
        self.dirty=true;
        self.error=None;
        self.focus_editor=true;
        self.last_action=None;
        self.action_pending=false;
    }

    fn move_selection(&mut self,navigation:ManualNavigation){
        let Some(model)=self.model.as_ref() else{return};
        let cells=manual_cell_targets(model);
        if cells.is_empty(){return;}
        let current=cells.iter().position(|cell|
            cell.row==self.selected_recipient &&
            cell.item==self.selected_item &&
            cell.field==self.selected_field
        );
        let target=match navigation{
            ManualNavigation::Next=>current
                .and_then(|index|cells.get((index+1).min(cells.len()-1))),
            ManualNavigation::Previous=>current
                .and_then(|index|cells.get(index.saturating_sub(1))),
            ManualNavigation::RowUp|ManualNavigation::RowDown=>{
                let direction=if navigation==ManualNavigation::RowUp{-1isize}else{1};
                let row_index=self.selected_recipient as isize+direction;
                if row_index<0{return;}
                let row=list(model,"rows").get(row_index as usize);
                let Some(row)=row else{return};
                let item=if let Some(item_id)=self.selected_item.as_deref(){
                    let current_index=list(model,"rows").get(self.selected_recipient)
                        .map(|current|list(current,"items").iter().position(|item|
                            text(item,"id")==item_id
                        )).flatten();
                    current_index.and_then(|index|
                        list(row,"items").get(index).map(|item|text(item,"id").to_owned())
                    )
                }else{None};
                let Some(candidate)=cells.iter().find(|cell|
                    cell.row==row_index as usize &&
                    cell.item==item &&
                    cell.field==self.selected_field
                )else{return};
                Some(candidate)
            }
        };
        if let Some(target)=target.cloned(){
            self.select_cell(target.row,target.item,&target.field);
        }
    }

    fn selected_quantity_pending(&self)->bool{
        let Some(model)=self.model.as_ref() else{return false};
        let Some(item_id)=self.selected_item.as_deref() else{return false};
        list(model,"rows").get(self.selected_recipient)
            .and_then(|row|list(row,"items").iter().find(|item|
                text(item,"id")==item_id
            ))
            .is_some_and(item_quantity_pending)
    }

    fn pending_quantity_cell(&self)->Option<ManualCellTarget>{
        if !self.selected_quantity_pending() ||
            !matches!(self.last_action.as_deref(),
                Some("set_manual_product"|"assign_manual_block")) {
            return None;
        }
        Some(ManualCellTarget{
            row:self.selected_recipient,
            item:self.selected_item.clone(),
            field:"quantity".into(),
        })
    }

    fn current_value(&self)->String {
        let Some(model)=self.model.as_ref() else{return String::new()};
        let Some(row)=list(model,"rows").get(self.selected_recipient) else {
            return String::new();
        };
        if let Some(id)=&self.selected_item {
            let Some(item)=list(row,"items").iter().find(|v|text(v,"id")==id)
                else{return String::new()};
            return match self.selected_field.as_str(){
                "quantity"=>item.get("quantity").and_then(Value::as_i64)
                    .map(|n|n.to_string()).unwrap_or_default(),
                "unitPrice"=>item.get("manualUnitPrice")
                    .or_else(||item.get("unitPrice"))
                    .and_then(Value::as_i64)
                    .map(|n|n.to_string()).unwrap_or_default(),
                _=>text(item,"name").to_owned()
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
        self.last_action=Some(action.clone());
        self.action_pending=true;
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
                let remote_rows=resolve_remote_rows(service.as_ref(),&next);
                Ok(Outcome::Updated{
                    model:next,assignment_order,remote_rows
                })
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn apply(&mut self,ctx:&Context) {
        if self.busy(){return;}
        let (Some(model),Some(order))=(self.model.clone(),self.base_order.clone())
            else{return};
        let cached_quote=self.cached_quote.clone();
        let keys=self.assignment_order.clone();
        let service=Arc::clone(&self.gateway);
        let selected_client=self.client_id.clone();
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
                let updated=applied.get("order").filter(|v|v.is_object())
                    .ok_or("Rust 수동 분류 적용 결과가 올바르지 않습니다.")?.clone();
                let price_inputs_unchanged=
                    price_input_snapshot(&order)==price_input_snapshot(&updated);
                let order=updated;
                let (quote,quote_error)=if let Some(id)=selected_client.as_deref(){
                    let result=(||->Result<Value,String>{
                        let state=service.load_business_state()?;
                        let client=state["clients"].as_array()
                            .and_then(|clients|clients.iter().find(|client|
                                client["id"].as_str()==Some(id)))
                            .ok_or("선택한 거래처의 단가 기준을 찾지 못했습니다.")?;
                        let basis=crate::pricing_basis::snapshot(&state,client,&order);
                        if price_inputs_unchanged &&
                           cached_quote.as_ref().is_some_and(|saved|
                               saved.get("pricingBasis")==Some(&basis)) {
                            if let Some(reused)=reuse_native_quote(
                                cached_quote.as_ref(),&order
                            ){return Ok(reused);}
                        }
                        let revision=state["revision"].as_u64()
                            .ok_or("Rust 기준 revision이 없습니다.")?;
                        let mut quote=service.execute(
                            ReadOperation::CalculateReceptionQuote,
                            json!({"order":&order,"state":&state,"client":client})
                        )?;
                        quote["authorityRevision"]=json!(revision);
                        let quoted_basis=crate::pricing_basis::snapshot(
                            &state,client,quote.get("order").unwrap_or(&order)
                        );
                        quote["pricingBasis"]=quoted_basis;
                        Ok(quote)
                    })();
                    match result{
                        Ok(quote)=>(Some(quote),None),
                        Err(error)=>(None,Some(format!(
                            "Rust 금액 재계산에 실패했습니다: {error}"
                        )))
                    }
                }else{(None,None)};
                Ok(Outcome::Applied{
                    order,model,assignment_order:keys,quote,quote_error
                })
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn show(
        &mut self,
        ctx:&Context,
        work_area:egui::Rect,
        link_hover:&mut source_highlight::HoverState,
        source_hover:&mut Option<source_highlight::LinkKey>,
    )->Option<Event> {
        self.poll();
        if ctx.input(|input|input.modifiers.command && input.key_pressed(egui::Key::Z)){
            self.undo();
        }
        if let Some((order,quote,quote_error))=self.applied.take() {
            if order.get("id").and_then(Value::as_str)!=Some(self.order_id.as_str()) ||
               order.get("raw").and_then(Value::as_str)!=Some(self.raw.as_str()) {
                self.error=Some("적용 결과의 발주서 ID나 원문이 일치하지 않습니다.".into());
            }else if let Some(model)=self.model.take() {
                return Some(Event::Applied {
                    order_id:self.order_id.clone(),raw:self.raw.clone(),
                    order_type:self.order_type.clone(),client_id:self.client_id.clone(),
                    model,assignment_order:self.assignment_order.clone(),order,
                    quote,quote_error
                });
            }
        }
        let mut open=true;
        if ctx.input(|input|input.key_pressed(egui::Key::Escape)) {
            match escape_action(
                self.busy(),
                self.product_search.is_some(),
                self.remove_recipient_pending.is_some(),
                self.reset_pending,
                self.confirm_discard,
                self.edit_text!=self.current_value(),
            ){
                EscapeAction::CloseProductSearch=>self.product_search=None,
                EscapeAction::CancelShipmentRemoval=>self.remove_recipient_pending=None,
                EscapeAction::CancelReset=>self.reset_pending=false,
                EscapeAction::CancelDiscard=>self.confirm_discard=false,
                EscapeAction::RevertCell=>{
                    self.edit_text=self.current_value();
                    self.error=None;
                }
                EscapeAction::RequestClose=>open=false,
                EscapeAction::Ignore=>{},
            }
        }
        let spans=self.model.as_ref().map(|model|{
            source_highlight::collect_manual(
                &self.raw,list(model,"rows"),list(model,"blocks"),
                model.get("assignments").unwrap_or(&Value::Null)
            )
        }).unwrap_or_default();
        let selected_recipient_id=self.model.as_ref().and_then(|model|
            list(model,"rows").get(self.selected_recipient)
                .map(|row|text(row,"id").to_owned())
        );
        let active_link=link_hover.active_for(&self.order_id).cloned();
        let scroll_link=link_hover.scroll_to.clone();
        let mut hovered_source:Option<source_highlight::LinkKey>=None;
        let mut hovered_detail:Option<source_highlight::LinkKey>=None;
        let mut source_did_scroll=false;
        let mut navigate_now:Option<ManualNavigation>=None;
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
                if self.busy() || self.product_search.is_some() ||
                    self.remove_recipient_pending.is_some() ||
                    self.reset_pending || self.confirm_discard {
                    ui.disable();
                }
                let Some(model)=self.model.as_ref() else {return};
                let rows=list(model,"rows").to_vec();
                let blocks=list(model,"blocks").to_vec();
                let assignments=model.get("assignments").cloned().unwrap_or_else(||json!({}));
                let busy=self.busy();
                let pending_quantity_cell=self.pending_quantity_cell();
                let pending_quantity_active=pending_quantity_cell.is_some();
                let has_pending_quantity=model_has_pending_quantity(model);
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
                            let interaction=crate::manual_source_view::source_document(
                                ui,&self.raw,&spans,selected_recipient_id.as_deref(),
                                active_link.as_ref(),scroll_link.as_ref(),!busy
                            );
                            hovered_source=interaction.hovered;
                            source_did_scroll=interaction.did_scroll;
                            if let Some(id)=interaction.clicked_block {
                                let assigned=assignments.get(&id);
                                let source_field=blocks.iter()
                                    .find(|block|text(block,"id")==id)
                                    .map(|block|text(block,"field"))
                                    .unwrap_or("");
                                let pending_item=pending_quantity_cell.as_ref()
                                    .and_then(|cell|cell.item.as_deref());
                                let assigned_item=assigned
                                    .and_then(|cell|cell.get("itemId"))
                                    .and_then(Value::as_str);
                                if !manual_source_assignment_allowed(
                                    assigned.is_some(),assigned_item,pending_item,source_field
                                ){
                                    self.error=Some(
                                        "선택한 상품의 수량을 먼저 확정하세요.".into()
                                    );
                                }else if assigned.is_some(){
                                    chosen=Some(("release_manual_block".into(),json!({
                                        "id":id,"clear":true
                                    })));
                                }else if let Some(row)=rows.get(self.selected_recipient){
                                    if !manual_cell_matches_source(
                                        self.selected_item.is_some(),
                                        &self.selected_field,source_field
                                    ){
                                        self.error=Some(
                                            "원문 항목과 같은 종류의 입력 셀을 먼저 선택하세요.".into()
                                        );
                                    }else{
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
                                components::form_field_label(ui,"배송 건수");
                                if theme::action(ui,"−",ButtonRole::Standard,
                                    !busy && !pending_quantity_active && rows.len()>1).clicked(){
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
                                    !busy && !pending_quantity_active && rows.len()<100).clicked(){
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
                                                    let response=components::manual_table_cell(
                                                        grid,text(row,field),width,selected
                                                    );
                                                    let key=source_highlight::key(rid,rid,field);
                                                    if response.hovered(){
                                                        hovered_detail=Some(key.clone());
                                                    }
                                                    source_highlight::paint_active_if(
                                                        grid,&response,
                                                        source_highlight::is_active(
                                                            active_link.as_ref(),&key
                                                        )
                                                    );
                                                    if response.clicked(){
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
                                                    grid,"단가",theme::MANUAL_TABLE_UNIT_PRICE_COL
                                                );
                                                components::manual_table_heading(
                                                    grid,"행 동작",theme::MANUAL_TABLE_ACTION_COL*2.0
                                                );
                                                grid.end_row();
                                                for item in list(row,"items"){
                                                    let item_id=text(item,"id");
                                                    let selected=index==self.selected_recipient &&
                                                        self.selected_item.as_deref()==Some(item_id);
                                                    let product_response=components::manual_table_cell(
                                                        grid,text(item,"name"),
                                                        theme::MANUAL_TABLE_PRODUCT_COL,
                                                        selected && self.selected_field=="product"
                                                    );
                                                    let product_key=source_highlight::key(
                                                        rid,item_id,"product"
                                                    );
                                                    if product_response.hovered(){
                                                        hovered_detail=Some(product_key.clone());
                                                    }
                                                    source_highlight::paint_active_if(
                                                        grid,&product_response,
                                                        source_highlight::is_active(
                                                            active_link.as_ref(),&product_key
                                                        )
                                                    );
                                                    if product_response.clicked() {
                                                        self.select_cell(index,Some(item_id.into()),"product");
                                                    }
                                                    let quantity=item.get("quantity")
                                                        .and_then(Value::as_i64)
                                                        .map(|n|n.to_string()).unwrap_or_default();
                                                    let quantity_response=components::manual_table_cell(
                                                        grid,&quantity,
                                                        theme::MANUAL_TABLE_QUANTITY_COL,
                                                        selected && self.selected_field=="quantity"
                                                    );
                                                    let quantity_key=source_highlight::key(
                                                        rid,item_id,"quantity"
                                                    );
                                                    if quantity_response.hovered(){
                                                        hovered_detail=Some(quantity_key.clone());
                                                    }
                                                    source_highlight::paint_active_if(
                                                        grid,&quantity_response,
                                                        source_highlight::is_active(
                                                            active_link.as_ref(),&quantity_key
                                                        )
                                                    );
                                                    if quantity_response.clicked() {
                                                        self.select_cell(index,Some(item_id.into()),"quantity");
                                                    }
                                                    let price=item.get("unitPrice")
                                                        .and_then(Value::as_i64)
                                                        .map(components::display_won)
                                                        .unwrap_or_else(||"—".into());
                                                    if components::manual_table_cell(
                                                        grid,&price,
                                                        theme::MANUAL_TABLE_UNIT_PRICE_COL,
                                                        selected && self.selected_field=="unitPrice"
                                                    ).on_hover_text(if item.get("manualUnitPrice")
                                                        .is_some(){"직접 수정된 단가"}
                                                        else{"상품·거래처 기준 단가"})
                                                        .clicked(){
                                                        self.select_cell(
                                                            index,Some(item_id.into()),"unitPrice"
                                                        );
                                                    }
                                                    grid.horizontal(|actions|{
                                                        if theme::action(actions,"검색",
                                                            ButtonRole::Standard,
                                                            !busy && !pending_quantity_active).clicked(){
                                                            lookup=Some((rid.to_owned(),item_id.to_owned()));
                                                        }
                                                        if theme::action(actions,"상품 제거",
                                                            ButtonRole::Quiet,
                                                            !busy && !pending_quantity_active).clicked(){
                                                            chosen=Some(("remove_manual_item".into(),json!({
                                                                "recipientId":rid,"itemId":item_id
                                                            })));
                                                        }
                                                    });
                                                    grid.end_row();
                                                }
                                            });
                                        if theme::action(ui,"상품 추가",ButtonRole::Standard,
                                            !busy && !pending_quantity_active).clicked(){
                                            chosen=Some(("add_manual_item".into(),json!({
                                                "recipientId":rid,"itemId":unique_id()
                                            })));
                                        }
                                        if index==self.selected_recipient {
                                            let origin=text(row,"dispatchOrigin");
                                            let origin=if origin=="STORE"{"STORE"}else{"FACTORY"};
                                            let code=text(row,"freightCode");
                                            let code=if code=="020"{"020"}else{"010"};
                                            // A missing staff override does not mean
                                            // an automatically detected area is unchecked.
                                            let remote_result=remote_for(&self.remote_rows,row)
                                                .cloned();
                                            let remote_value=remote_result.as_ref()
                                                .and_then(|result|result.as_ref().ok())
                                                .filter(|value|value.is_object());
                                            let remote=remote_value.and_then(|decision|
                                                decision["checked"].as_bool()
                                            ).unwrap_or(false);
                                            let remote_ready=remote_value.is_some();
                                            if let Some(decision)=remote_value {
                                                if let Some(message)=
                                                    crate::reception_service::remote_delivery_notice(decision)
                                                {
                                                    ui.label(typography::status(
                                                        &message,theme::SELECTED_ORDER
                                                    ));
                                                }
                                            }else if let Some(Err(error))=remote_result.as_ref(){
                                                ui.label(typography::status(
                                                    &format!("도서/산간 판정 실패 · {error}"),
                                                    theme::SELECTED_ORDER
                                                ));
                                            }
                                            ui.horizontal_wrapped(|options|{
                                                options.spacing_mut().item_spacing.x=theme::SPACE_8;
                                                components::form_field_label(options,"발송지");
                                                for (label,value) in [
                                                    ("공장","FACTORY"),("매장","STORE")
                                                ]{
                                                    if theme::action(
                                                        options,label,
                                                        if origin==value{ButtonRole::TableSecondary}
                                                        else{ButtonRole::Standard},!busy && !pending_quantity_active
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
                                                        else{ButtonRole::Standard},!busy && !pending_quantity_active).clicked(){
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
                                                    ButtonRole::Standard,!busy && !pending_quantity_active && remote_ready
                                                ).clicked(){
                                                    chosen=Some((
                                                        "set_manual_delivery_option".into(),
                                                        json!({"cell":{"recipientId":rid,
                                                            "field":"remoteDelivery"},
                                                            "value":!remote})
                                                    ));
                                                }
                                                if theme::action(options,
                                                    "운임 수정",ButtonRole::Standard,
                                                    !busy && !pending_quantity_active && code!="020"
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
                                    match self.selected_field.as_str(){
                                        "quantity"=>"수량","unitPrice"=>"단가",
                                        _=>"상품명"
                                    }
                                } else {
                                    match self.selected_field.as_str() {
                                        "name"=>"이름","address"=>"주소",
                                        "mobile"=>"휴대폰","phone"=>"전화",
                                        "note"=>"배송 요청",
                                        "shippingCharge"=>"적용 운임",_=>"이름"
                                    }
                                };
                                components::form_field_label(ui,label);
                                let editor=components::flexible_text_field(
                                    ui,&mut self.edit_text,
                                    theme::MANUAL_INPUT_WIDTH,""
                                );
                                if self.focus_editor && !busy &&
                                    self.product_search.is_none() &&
                                    self.remove_recipient_pending.is_none() &&
                                    !self.reset_pending && !self.confirm_discard {
                                    editor.request_focus();
                                    self.focus_editor=false;
                                }
                                if editor.has_focus(){
                                    let navigation=ctx.input(|input|{
                                        if input.key_pressed(egui::Key::Enter){
                                            Some(ManualNavigation::Next)
                                        }else if input.key_pressed(egui::Key::Tab){
                                            Some(if input.modifiers.shift{
                                                ManualNavigation::Previous
                                            }else{ManualNavigation::Next})
                                        }else if input.key_pressed(egui::Key::ArrowUp){
                                            Some(ManualNavigation::RowUp)
                                        }else if input.key_pressed(egui::Key::ArrowDown){
                                            Some(ManualNavigation::RowDown)
                                        }else{None}
                                    });
                                    if let Some(navigation)=navigation{
                                        let needs_commit=
                                            self.edit_text!=self.current_value() ||
                                            self.selected_field=="quantity";
                                        if needs_commit{
                                            submit_direct=true;
                                            self.advance_after_apply=Some(navigation);
                                        }else{
                                            navigate_now=Some(navigation);
                                        }
                                    }
                                }
                                if self.selected_item.is_some() &&
                                    self.selected_field=="quantity" {
                                    let quantity=rows.get(self.selected_recipient)
                                        .and_then(|row|list(row,"items").iter().find(|item|
                                            self.selected_item.as_deref()==Some(text(item,"id"))
                                        ))
                                        .and_then(|item|item.get("quantity"))
                                        .and_then(Value::as_i64).unwrap_or(0);
                                    let settled=self.edit_text==self.current_value();
                                    if theme::action(ui,"−",ButtonRole::Standard,
                                        !busy && settled && quantity>1
                                    ).clicked(){
                                        if let (Some(row),Some(item))=(
                                            rows.get(self.selected_recipient),
                                            self.selected_item.as_ref()
                                        ){
                                            self.focus_editor=true;
                                            chosen=Some(("step_manual_quantity".into(),json!({
                                                "cell":{"recipientId":text(row,"id"),
                                                    "itemId":item},
                                                "delta":-1
                                            })));
                                        }
                                    }
                                    if theme::action(ui,"+",ButtonRole::Standard,
                                        !busy && settled
                                    ).clicked(){
                                        if let (Some(row),Some(item))=(
                                            rows.get(self.selected_recipient),
                                            self.selected_item.as_ref()
                                        ){
                                            self.focus_editor=true;
                                            chosen=Some(("step_manual_quantity".into(),json!({
                                                "cell":{"recipientId":text(row,"id"),
                                                    "itemId":item},
                                                "delta":1
                                            })));
                                        }
                                    }
                                    if self.selected_quantity_pending() &&
                                        matches!(self.last_action.as_deref(),
                                            Some("set_manual_product"|"assign_manual_block")) &&
                                        theme::action(ui,"상품 선택 취소",ButtonRole::Quiet,
                                            !busy && settled
                                        ).clicked(){
                                        self.undo();
                                    }
                                }
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
                let input_settled=self.edit_text==self.current_value();
                components::modal_action_row_split(
                    ui,
                    |left|{
                        // Reset is a destructive worksheet operation with its
                        // existing confirmation, not a toolbar filter.
                        if theme::action(left,"초기화",ButtonRole::Danger,
                            !busy && !pending_quantity_active && input_settled
                        ).clicked(){
                            if input_settled {
                                self.reset_pending=true;
                            }
                        }
                    },
                    |right|{
                        if theme::action(right,"초안에 적용",
                            ButtonRole::Primary,!busy && input_settled &&
                                !has_pending_quantity
                        ).clicked(){apply=true;}
                    }
                );
            });
        if let Some(navigation)=navigate_now{
            self.move_selection(navigation);
        }
        // Search opens only for the current native recipient/item identity.
        // The underlying worksheet is disabled while a chooser is open.
        if let Some((recipient_id,item_id))=lookup {
            if self.edit_text!=self.current_value(){
                self.error=Some("입력 중인 내용을 먼저 반영하세요.".into());
            }else if self.product_search.is_none(){
                let target=self.model.as_ref().and_then(|model|
                    list(model,"rows").iter().position(|row|
                        text(row,"id")==recipient_id &&
                        list(row,"items").iter().any(|item|text(item,"id")==item_id)
                    )
                );
                if let Some(index)=target {
                    // Search belongs to the clicked product row, not whatever
                    // recipient/item happened to be selected previously.
                    self.select_cell(index,Some(item_id.clone()),"product");
                    self.product_search=Some(ProductSearch::new(
                        recipient_id,item_id,self.client_id.clone().unwrap_or_default(),
                        Arc::clone(&self.gateway),ctx.clone()
                    ));
                }else{
                    self.error=Some("상품 검색 대상이 현재 배송 목록에 없습니다.".into());
                }
            }
        }
        if let Some(window_id)=self.nested_modal_window_id(){
            components::modal_backdrop_for_window(
                ctx,"manual-nested-modal-backdrop",window_id
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
                    let target=self.model.as_ref().and_then(|model|
                        list(model,"rows").iter().position(|row|
                            text(row,"id")==search.recipient_id
                        )
                    );
                    if let Some(index)=target {
                        // React's product chooser hands focus to quantity next.
                        self.select_cell(
                            index,Some(search.item_id.clone()),"quantity"
                        );
                    }
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
            self.focus_editor=true;
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
                }else if self.selected_field=="unitPrice"{
                    let raw=self.edit_text.trim().replace(',',"");
                    if raw.is_empty(){
                        value=Value::Null;
                    }else if let Ok(price)=raw.parse::<i64>(){
                        value=json!(price);
                    }else{
                        self.error=Some("단가는 0 이상의 정수로 입력하세요.".into());
                        return None;
                    }
                    "set_manual_unit_price"
                }else if self.selected_field=="shippingCharge"{
                    let collect=self.model.as_ref().and_then(|model|
                        list(model,"rows").get(self.selected_recipient)
                    ).is_some_and(|row|text(row,"freightCode")=="020");
                    if collect {
                        self.error=Some("착불인 주문은 운임을 수정할 수 없습니다.".into());
                        return None;
                    }
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
                if action=="assign_manual_block" &&
                    self.selected_item.is_some() && self.selected_field=="product" {
                    self.select_cell(
                        self.selected_recipient,self.selected_item.clone(),"quantity"
                    );
                }
                if action=="set_manual_product" &&
                    self.selected_item.is_some() && self.selected_field=="product" {
                    self.select_cell(
                        self.selected_recipient,self.selected_item.clone(),"quantity"
                    );
                }
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
        let hovered=hovered_detail.or_else(||hovered_source.clone())
            .filter(|key|spans.iter().any(|span|span.key==*key));
        link_hover.update_for(&self.order_id,hovered,ctx);
        *source_hover=hovered_source.filter(|key|
            spans.iter().any(|span|span.key==*key)
        );
        if source_did_scroll {link_hover.scroll_to=None;}
        None
    }
}

fn manual_cell_matches_source(
    item_selected:bool,selected_field:&str,source_field:&str
)->bool{
    if item_selected {
        ["product","quantity"].contains(&source_field) &&
            selected_field==source_field
    }else{
        ["name","address","mobile","phone","note"].contains(&source_field) &&
            selected_field==source_field
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum EscapeAction {
    Ignore,
    CloseProductSearch,
    CancelShipmentRemoval,
    CancelReset,
    CancelDiscard,
    RevertCell,
    RequestClose,
}

fn escape_action(
    busy:bool,
    product_search:bool,
    remove_recipient:bool,
    reset:bool,
    discard:bool,
    cell_dirty:bool,
)->EscapeAction{
    if busy {EscapeAction::Ignore}
    else if product_search {EscapeAction::CloseProductSearch}
    else if remove_recipient {EscapeAction::CancelShipmentRemoval}
    else if reset {EscapeAction::CancelReset}
    else if discard {EscapeAction::CancelDiscard}
    else if cell_dirty {EscapeAction::RevertCell}
    else {EscapeAction::RequestClose}
}

fn nested_modal_id(
    product_search:bool,
    remove_recipient:bool,
    reset:bool,
    discard:bool,
)->Option<&'static str>{
    if product_search {Some("native-manual-product-search")}
    else if remove_recipient {Some("manual-shipment-removal")}
    else if reset {Some("manual-reset-confirm")}
    else if discard {Some("manual-discard-prompt")}
    else {None}
}

#[cfg(test)]
mod manual_workspace_modal_routing_tests {
    use super::*;

    #[test]
    fn product_selection_requires_a_confirmed_quantity_before_it_is_complete(){
        assert!(item_quantity_pending(&json!({
            "productId":"p1","quantity":null,"quantityConfirmed":false
        })));
        assert!(item_quantity_pending(&json!({
            "name":"BD 709","quantity":0,"quantityConfirmed":false
        })));
        assert!(!item_quantity_pending(&json!({
            "quantity":2,"quantityConfirmed":true
        })));
        assert!(!item_quantity_pending(&json!({
            "productId":"p1","quantity":2,"quantityConfirmed":true
        })));
    }

    #[test]
    fn keyboard_cell_order_matches_the_manual_worksheet(){
        let model=json!({"rows":[
            {"id":"r1","items":[{"id":"i1"},{"id":"i2"}]},
            {"id":"r2","items":[{"id":"i3"}]}
        ]});
        let cells=manual_cell_targets(&model);
        let keys=cells.iter().map(|cell|(
            cell.row,cell.item.as_deref(),cell.field.as_str()
        )).collect::<Vec<_>>();
        assert_eq!(keys[0],(0,None,"name"));
        assert_eq!(keys[4],(0,None,"note"));
        assert_eq!(keys[5],(0,Some("i1"),"product"));
        assert_eq!(keys[6],(0,Some("i1"),"quantity"));
        assert_eq!(keys[7],(0,Some("i1"),"unitPrice"));
        assert_eq!(keys[10],(0,Some("i2"),"unitPrice"));
        assert_eq!(keys[11],(1,None,"name"));
    }

    #[test]
    fn manual_undo_history_is_bounded_and_keeps_the_latest_frames(){
        let mut history=Vec::new();
        for value in 0..101{
            push_manual_history(&mut history,ManualHistoryFrame{
                model:json!({"value":value}),
                assignment_order:Vec::new(),
                remote_rows:Vec::new(),
                selected_recipient:0,
                selected_item:None,
                selected_field:"name".into(),
            });
        }
        assert_eq!(history.len(),100);
        assert_eq!(history.first().unwrap().model["value"],1);
        assert_eq!(history.last().unwrap().model["value"],100);
        assert_eq!(history.pop().unwrap().model["value"],100);
    }

    #[test]
    fn source_block_assignment_requires_a_matching_cell_kind(){
        assert!(manual_cell_matches_source(false,"name","name"));
        assert!(manual_cell_matches_source(false,"address","address"));
        assert!(!manual_cell_matches_source(false,"unitPrice","product"));
        assert!(!manual_cell_matches_source(false,"name","quantity"));
        assert!(manual_cell_matches_source(true,"product","product"));
        assert!(manual_cell_matches_source(true,"quantity","quantity"));
        assert!(!manual_cell_matches_source(true,"product","name"));
    }

    #[test]
    fn every_nested_manual_dialog_owns_its_backdrop_input_layer(){
        assert_eq!(nested_modal_id(true,false,false,false),
            Some("native-manual-product-search"));
        assert_eq!(nested_modal_id(false,true,false,false),
            Some("manual-shipment-removal"));
        assert_eq!(nested_modal_id(false,false,true,false),
            Some("manual-reset-confirm"));
        assert_eq!(nested_modal_id(false,false,false,true),
            Some("manual-discard-prompt"));
        assert_eq!(nested_modal_id(false,false,false,false),None);
    }

    #[test]
    fn escape_closes_only_the_topmost_nested_prompt(){
        assert_eq!(escape_action(false,true,true,true,true,false),
            EscapeAction::CloseProductSearch);
        assert_eq!(escape_action(false,false,true,true,true,false),
            EscapeAction::CancelShipmentRemoval);
        assert_eq!(escape_action(false,false,false,true,true,false),
            EscapeAction::CancelReset);
        assert_eq!(escape_action(false,false,false,false,true,false),
            EscapeAction::CancelDiscard);
    }

    #[test]
    fn escape_reverts_uncommitted_cell_before_requesting_workspace_close(){
        assert_eq!(escape_action(false,false,false,false,false,true),
            EscapeAction::RevertCell);
        assert_eq!(escape_action(false,false,false,false,false,false),
            EscapeAction::RequestClose);
        assert_eq!(escape_action(true,false,false,false,false,true),
            EscapeAction::Ignore);
    }
}

#[cfg(test)]
mod manual_pending_interaction_tests {
    use super::*;

    #[test]
    fn pending_quantity_locks_other_cells_but_keeps_its_own_cell_editable(){
        let pending=ManualCellTarget{
            row:1,item:Some("item-2".into()),field:"quantity".into()
        };
        let same=ManualCellTarget{
            row:1,item:Some("item-2".into()),field:"quantity".into()
        };
        let other_field=ManualCellTarget{
            row:1,item:Some("item-2".into()),field:"product".into()
        };
        let other_item=ManualCellTarget{
            row:1,item:Some("item-3".into()),field:"quantity".into()
        };
        let other_recipient=ManualCellTarget{
            row:0,item:Some("item-2".into()),field:"quantity".into()
        };
        assert!(manual_cell_allowed(Some(&pending),&same));
        assert!(!manual_cell_allowed(Some(&pending),&other_field));
        assert!(!manual_cell_allowed(Some(&pending),&other_item));
        assert!(!manual_cell_allowed(Some(&pending),&other_recipient));
        assert!(manual_cell_allowed(None,&other_field));
    }

    #[test]
    fn source_assignment_guard_matches_react_use_block_contract(){
        // No active product/quantity hand-off: any valid block can be used.
        assert!(manual_source_assignment_allowed(false,None,None,"product"));
        assert!(manual_source_assignment_allowed(true,Some("item-1"),None,"name"));

        // An already assigned block on the same pending item may be released.
        assert!(manual_source_assignment_allowed(
            true,Some("item-1"),Some("item-1"),"product"
        ));
        // A block owned by another item or recipient must not be released.
        assert!(!manual_source_assignment_allowed(
            true,Some("item-2"),Some("item-1"),"product"
        ));
        assert!(!manual_source_assignment_allowed(
            true,None,Some("item-1"),"name"
        ));
        // Only the matching quantity evidence can complete the pending item.
        assert!(manual_source_assignment_allowed(
            false,None,Some("item-1"),"quantity"
        ));
        assert!(!manual_source_assignment_allowed(
            false,None,Some("item-1"),"product"
        ));
    }

    #[test]
    fn any_unconfirmed_product_quantity_blocks_final_apply(){
        let ready=json!({"rows":[{"items":[
            {"productId":"p1","quantity":2,"quantityConfirmed":true}
        ]}]});
        let pending=json!({"rows":[{"items":[
            {"productId":"p1","quantity":null,"quantityConfirmed":false}
        ]}]});
        let empty=json!({"rows":[{"items":[{"quantity":null}]}]});
        assert!(!model_has_pending_quantity(&ready));
        assert!(model_has_pending_quantity(&pending));
        assert!(!model_has_pending_quantity(&empty));
    }
}

#[cfg(test)]
mod manual_remote_row_tests {
    use super::*;
    #[test]
    fn decision_is_bound_to_both_recipient_and_exact_address(){
        let row=json!({"id":"r1","address":"제주특별자치도 제주시 1"});
        let reference=vec![("r1".to_owned(),"제주특별자치도 제주시 1".to_owned(),
            Ok(json!({"status":"auto","checked":true})))];
        let decision=remote_for(&reference,&row)
            .and_then(|result|result.as_ref().ok()).unwrap();
        assert_eq!(decision["checked"],true);
        assert!(remote_for(&reference,&json!({"id":"r1","address":"서울특별시 1"}))
            .is_none());
        assert!(remote_for(&reference,&json!({
            "id":"r2","address":"제주특별자치도 제주시 1"
        })).is_none());
    }
}

#[cfg(test)]
mod quote_reuse_tests {
    use super::*;

    fn sample()->Value {
        json!({
            "id":"draft","applyVat":false,
            "recipients":[{
                "id":"r1","name":"원본 이름","address":"서울특별시 중구",
                "mobile":"010-1234-5678","phone":"","note":"문 앞",
                "freightCode":"010","dispatchOrigin":"FACTORY",
                "shippingCharges":{"FACTORY":3000},
                "remoteDelivery":{"address":"서울특별시 중구",
                    "checked":false,"manual":false},
                "vatOverride":false,"vatRateBps":1000,
                "items":[{
                    "id":"i1","productId":"p1","variantId":"v1",
                    "quantity":2,"unitPrice":6000,"origin":"FACTORY"
                }]
            }]
        })
    }

    #[test]
    fn contact_edits_do_not_change_native_price_inputs(){
        let original=sample();
        for field in ["name","mobile","phone","note"]{
            let mut updated=original.clone();
            updated["recipients"][0][field]=json!("직원 수정");
            assert_eq!(
                price_input_snapshot(&original),price_input_snapshot(&updated),
                "editing {field} must not refresh the invoice"
            );
        }
        let mut source_labels=original.clone();
        source_labels["recipients"][0]["sources"]=json!([{"field":"name"}]);
        assert_eq!(price_input_snapshot(&original),price_input_snapshot(&source_labels));
    }

    #[test]
    fn financial_changes_invalidate_the_prior_quote(){
        let original=sample();
        for (path,value) in [
            ("address",json!("제주특별자치도 제주시")),
            ("dispatchOrigin",json!("STORE")),
            ("freightCode",json!("020")),
            ("shippingCharges",json!({"FACTORY":4500})),
            ("remoteDelivery",json!({"checked":true,"manual":true})),
            ("vatOverride",json!(true)),
            ("vatRateBps",json!(2000)),
        ]{
            let mut updated=original.clone();
            updated["recipients"][0][path]=value;
            assert_ne!(price_input_snapshot(&original),
                price_input_snapshot(&updated),"{path}");
        }
        for (path,value) in [
            ("productId",json!("p2")),("variantId",json!("v2")),
            ("quantity",json!(3)),("unitPrice",json!(7300)),
            ("manualUnitPrice",json!(7200)),("origin",json!("STORE"))
        ]{
            let mut updated=original.clone();
            updated["recipients"][0]["items"][0][path]=value;
            assert_ne!(price_input_snapshot(&original),
                price_input_snapshot(&updated),"{path}");
        }
        let mut vat=original.clone();
        vat["applyVat"]=json!(true);
        assert_ne!(price_input_snapshot(&original),price_input_snapshot(&vat));
    }

    #[test]
    fn existing_complete_quote_is_reused_without_arithmetic(){
        let mut updated=sample();
        updated["recipients"][0]["note"]=json!("관리 메모만 수정됨");
        let saved=json!({
            "authorityRevision":9,"sourceRaw":"원문","orderId":"draft",
            "clientId":"c1","orderType":"AGENCY",
            "shippingFee":3000,"shippingVersion":2,
            "subtotal":12000,"vatAmount":0,"total":15000,
            "pendingRecipients":[],
            "linePrices":[{"itemId":"i1","amount":12000}],
            "freightLines":[{"recipientId":"r1","origin":"FACTORY",
                "appliedCharge":3000,"billableCharge":3000}]
        });
        let restored=reuse_native_quote(Some(&saved),&updated).unwrap();
        assert_eq!(restored["totals"]["total"],15000);
        assert_eq!(restored["totals"]["pendingRecipients"],json!([]));
        assert_eq!(restored["linePrices"],saved["linePrices"]);
        assert_eq!(restored["freightLines"],saved["freightLines"]);
        assert_eq!(restored["order"]["recipients"][0]["note"],
            "관리 메모만 수정됨");
        assert_eq!(restored["order"]["shippingFee"],3000);
    }

    #[test]
    fn legacy_incomplete_quote_requires_native_recalculation(){
        let order=sample();
        let stale=json!({
            "authorityRevision":1,"linePrices":[{"itemId":"i1"}],
            "freightLines":[]
        });
        assert!(reuse_native_quote(Some(&stale),&order).is_none());
        assert!(reuse_native_quote(None,&order).is_none());
    }
}
