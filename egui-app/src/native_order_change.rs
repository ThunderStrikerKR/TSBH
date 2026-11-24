//! The existing Rust Tauri order-edit engine, not egui, decides dealer
//! changes, item repricing, shipping, billing resets and edit constraints.
//!
//! This adapter reads a consistent LIVE master snapshot but NEVER writes it.
//! The resulting preview is deliberately narrow: customer contacts, payment
//! accounts, full order details and historic financial records are not saved
//! into the isolated egui workspace. Production write_snapshot/state_save
//! remains the sole writer of operational orders.

use crate::{gateway::{BandoGateway,ReadOperation},preview_store::WorkingReception};
use chrono::Utc;
use serde_json::{json,Value};
use std::sync::atomic::{AtomicBool,Ordering};

pub struct ChangeResult {
    pub client_id: String,
    // None means a deliberately isolated manual identity link. Never
    // present it as a priced native order or invent customer-level costs.
    pub pricing_preview: Option<Value>,
    /// Ephemeral full native Order, returned only to the explicit confirmation
    /// worker. It is never copied into the isolated preview SQLite draft.
    pub order: Option<Value>,
    // The existing employee-corrected model rebased to the selected dealer.
    // This is persisted only into the isolated draft, never the business DB.
    pub manual_model: Option<Value>,
    // Exact dealer-specific native recognition produced by this same worker.
    // Reuse it in the reception UI; never schedule another AnalyzeOrder after
    // a successful quote solely to rediscover the chosen dealer's aliases.
    pub recognition: Option<Value>,
}

fn required<'a>(value:&'a Value,key:&str)->Result<&'a Value,String>{
    value.get(key).filter(|v|!v.is_null()).ok_or_else(||
        format!("Rust 주문 변경 결과에 {key}가 없습니다."))
}

/// Preserve the employee's rows, source assignments and editing metadata.
/// Only Rust's newly calculated row prices/freight may replace the row snapshot.
fn rebind_manual_model(saved:&Value,client_id:&str,rows:&Value)->Result<Value,String>{
    let mut updated=saved.clone();
    let model=updated.get_mut("model").and_then(Value::as_object_mut)
        .ok_or("저장된 수동 수정 작업본이 올바르지 않습니다.")?;
    if !rows.is_array(){
        return Err("수동 수정 배송 목록이 올바르지 않습니다.".into());
    }
    model.insert("rows".into(),rows.clone());
    updated["clientId"]=json!(client_id);
    Ok(updated)
}

/// The request snapshot is the precise reception at employee click time.
pub fn prepare(gateway:&dyn BandoGateway, source:&WorkingReception,
               target_id:&str) -> Result<ChangeResult,String> {
    let never_cancel=AtomicBool::new(false);
    prepare_cancellable(gateway,source,target_id,&never_cancel)
}

fn check_cancelled(cancel:&AtomicBool)->Result<(),String>{
    if cancel.load(Ordering::Acquire){
        Err("새 거래처 선택으로 기존 자동 계산이 취소되었습니다.".into())
    }else{Ok(())}
}

/// Advisory automatic association is preemptible between native operations.
/// Explicit employee edits keep using prepare() and cannot be cancelled by
/// another UI selection while their result is pending.
pub fn prepare_cancellable(
    gateway:&dyn BandoGateway,source:&WorkingReception,
    target_id:&str,cancel:&AtomicBool
)->Result<ChangeResult,String>{
    check_cancelled(cancel)?;
    if source.raw.trim().is_empty() &&
        source.manual_model.as_ref().is_some_and(|saved|
            saved.get("mode").and_then(Value::as_str)==Some("DIRECT")
        ){
        return prepare_direct(gateway,source,target_id,cancel);
    }
    if source.raw.trim().is_empty() {
        return Err("발주원문이 없는 주문은 거래처를 변경할 수 없습니다.".into());
    }
    if source.manual_model.as_ref().is_some_and(|saved|
        saved.get("sourceRaw").and_then(Value::as_str)!=Some(source.raw.as_str()) ||
        saved.get("orderType").and_then(Value::as_str)!=Some(source.order_type.as_str())
    ){
        return Err("원문이나 접수 유형과 일치하지 않는 수동 수정 내역은 재적용할 수 없습니다.".into());
    }
    if target_id.trim().is_empty() {
        return Err("변경할 거래처가 없습니다.".into());
    }
    if source.analysis_raw.as_deref()!=Some(source.raw.as_str()) ||
       source.analysis_evidence.is_none() {
        return Err("현재 원문의 Rust 분석이 완료된 뒤 거래처를 선택하세요.".into());
    }

    // An explicitly isolated egui client master has no full billing state.
    // Permit only an employee-confirmed identity reference, validated against
    // the current Rust catalog; no pricing or production order mutation.
    // This is an intentional separate scope from full change_order_client.
    if gateway.isolated_client_identity_link() {
        if source.pricing_preview.is_some() {
            // Do not silently downgrade an existing native pricing result
            // into a name-only association. Historical bridge-version
            // metadata alone must not lock a local reception, however.
            return Err("기존 Rust 계산 결과가 있어 격리 거래처 연결로 전환할 수 없습니다.".into());
        }
        let index=gateway.load_catalog()?;
        check_cancelled(cancel)?;
        let exists=index.get("clients").and_then(Value::as_array)
            .is_some_and(|clients| clients.iter().any(|client|
                client.get("id").and_then(Value::as_str)==Some(target_id) &&
                client.get("deletedAt").is_none_or(|value|
                    value.is_null() || value.as_str()==Some("")) &&
                client.get("active")!=Some(&Value::Bool(false))
            ));
        if !exists {
            return Err("현재 Rust 거래처 명단에 없는 거래처는 선택할 수 없습니다.".into());
        }
        let manual_model=source.manual_model.as_ref()
            .map(|saved|rebind_manual_model(
                saved,target_id,&saved["model"]["rows"]
            )).transpose()?;
        return Ok(ChangeResult {
            client_id:target_id.to_owned(),
            pricing_preview:None,
            order:None,
            manual_model,
            recognition:None,
        });
    }

    // Direct-entry orders have no source analysis to rerun. Rebind the
    // existing manual rows through the same native client-change and quote rules.
    fn prepare_direct(
        gateway:&dyn BandoGateway,source:&WorkingReception,
        target_id:&str,cancel:&AtomicBool
    )->Result<ChangeResult,String>{
        check_cancelled(cancel)?;
        if target_id.trim().is_empty(){
            return Err("변경할 거래처가 없습니다.".into());
        }
        let saved=source.manual_model.as_ref()
            .filter(|saved|saved.get("mode").and_then(Value::as_str)==Some("DIRECT"))
            .ok_or("직접 입력 작업본이 없습니다.")?;
        if saved.get("sourceRaw").and_then(Value::as_str)!=Some("") ||
           saved.get("orderType").and_then(Value::as_str)!=Some(source.order_type.as_str()) ||
           saved.get("clientId").and_then(Value::as_str)!=source.client_id.as_deref(){
            return Err("직접 입력 작업본의 접수 유형이 변경되었습니다.".into());
        }
        let model=saved.get("model").filter(|value|value.is_object())
            .ok_or("저장된 직접 입력 작업본이 올바르지 않습니다.")?;
        let keys=saved.get("assignmentOrder").filter(|value|value.is_array())
            .ok_or("직접 입력 배정 순서가 없습니다.")?;
        if !model.get("blocks").and_then(Value::as_array).is_some_and(|blocks|blocks.is_empty()) ||
            !model.get("assignments").and_then(Value::as_object)
                .is_some_and(|assignments|assignments.is_empty()) ||
            !keys.as_array().is_some_and(|keys|keys.is_empty()){
            return Err("직접 입력에는 발주원문 블록이나 원문 배정이 존재할 수 없습니다.".into());
        }

        // An isolated catalog supports identity only. Never invent prices.
        if gateway.isolated_client_identity_link(){
            let index=gateway.load_catalog()?;
            check_cancelled(cancel)?;
            let exists=index.get("clients").and_then(Value::as_array)
                .is_some_and(|clients|clients.iter().any(|client|
                    client.get("id").and_then(Value::as_str)==Some(target_id) &&
                    client.get("deletedAt").is_none_or(|value|
                        value.is_null() || value.as_str()==Some("")) &&
                    client.get("active")!=Some(&Value::Bool(false))
                ));
            if !exists{
                return Err("현재 Rust 거래처 명단에 없는 거래처는 선택할 수 없습니다.".into());
            }
            return Ok(ChangeResult{
                client_id:target_id.to_owned(),pricing_preview:None,
                order:None,
                manual_model:Some(rebind_manual_model(saved,target_id,&model["rows"])?),
                recognition:None,
            });
        }

        let state=gateway.load_business_state()?;
        check_cancelled(cancel)?;
        let authority_revision=state.get("revision").and_then(Value::as_u64)
            .ok_or("운영 DB revision이 유효하지 않습니다.")?;
        if state.get("orders").and_then(Value::as_array).is_some_and(|orders|
            orders.iter().any(|order|
                order.get("id").and_then(Value::as_str)==Some(source.id.as_str())
            )){
            return Err("운영 DB에 이미 존재하는 주문은 egui 미확정 발주서로 수정할 수 없습니다.".into());
        }
        let client=state.get("clients").and_then(Value::as_array)
            .and_then(|clients|clients.iter().find(|client|
                client.get("id").and_then(Value::as_str)==Some(target_id) &&
                client.get("deletedAt").is_none_or(Value::is_null) &&
                client.get("active")!=Some(&Value::Bool(false))
            )).ok_or("운영 거래처 명단에서 선택한 거래처를 확인할 수 없습니다.")?;
        let now=Utc::now().to_rfc3339();
        let created=gateway.execute(ReadOperation::NewReception,json!({
            "action":"new_reception","id":source.id,
            "clientId":"","now":now,"orderType":source.order_type
        }))?;
        check_cancelled(cancel)?;
        let blank=required(&created,"order")?;
        let changed=gateway.execute(ReadOperation::ChangeOrderClient,json!({
            "action":"change_order_client","state":state,"order":blank,
            "dealerId":target_id,"now":now
        }))?;
        check_cancelled(cancel)?;
        let selected=required(&changed,"order")?;
        let applied=gateway.execute(ReadOperation::ManualClassification,json!({
            "action":"preview_manual","order":selected,
            "model":model,"assignmentOrder":keys
        }))?;
        check_cancelled(cancel)?;
        let edited=required(&applied,"order")?;
        let quote=gateway.execute(ReadOperation::CalculateReceptionQuote,json!({
            "order":edited,"state":state,"client":client
        }))?;
        check_cancelled(cancel)?;
        let order=required(&quote,"order")?;
        let totals=required(&quote,"totals")?;
        if order.get("id").and_then(Value::as_str)!=Some(source.id.as_str()) ||
           order.get("clientId").and_then(Value::as_str)!=Some(target_id) ||
           order.get("raw").and_then(Value::as_str)!=Some(""){
            return Err("직접 입력 거래처 재계산 결과가 현재 작업본과 일치하지 않습니다.".into());
        }
        let recipients=required(order,"recipients")?;
        let manual_model=rebind_manual_model(saved,target_id,recipients)?;
        let preview=json!({
            "sourceRaw":"","orderId":source.id,
            "orderType":source.order_type,"clientId":target_id,
            "authorityRevision":authority_revision,
            "status":order.get("status"),
            "shippingFee":order.get("shippingFee"),
            "shippingVersion":order.get("shippingVersion"),
            "vatAmount":totals.get("vat"),
            "subtotal":totals.get("subtotal"),
            "total":totals.get("total"),
            "pendingRecipients":totals.get("pendingRecipients"),
            "notified":order.get("notified"),"paid":order.get("paid"),
            "linePrices":quote.get("linePrices"),
            "freightLines":quote.get("freightLines"),
            "pricingBasis":crate::pricing_basis::snapshot(&state,client,order)
        });
        Ok(ChangeResult{
            client_id:target_id.to_owned(),pricing_preview:Some(preview),
            order:Some(order.clone()),
            manual_model:Some(manual_model),recognition:None
        })
    }

    // Full operating state follows the same Rust change_order_client →
    // merge → reprice → calculate_price chain as the original React path.
    // Never use the name-only client index or the embedded retail seed to
    // calculate actual customer prices, shipping waivers or overrides.
    let state=gateway.load_business_state()?;
    check_cancelled(cancel)?;
    let authority_revision=state.get("revision").and_then(Value::as_u64)
        .ok_or("운영 DB revision이 유효하지 않습니다.")?;
    if state.get("orders").and_then(Value::as_array).is_some_and(|orders|
        orders.iter().any(|order|
            order.get("id").and_then(Value::as_str)==Some(source.id.as_str())
        )) {
        return Err("운영 DB에 이미 존재하는 주문은 egui 미확정 발주서로 수정할 수 없습니다.".into());
    }
    // A stale quote is NEVER copied to a newly selected client. This worker
    // already calculates fresh prices from the latest native business state;
    // blocking on the unrelated global revision only prevents valid edits.
    let client=state.get("clients").and_then(Value::as_array)
        .and_then(|clients|clients.iter().find(|c|
            c.get("id").and_then(Value::as_str)==Some(target_id) &&
            c.get("deletedAt").is_none_or(Value::is_null) &&
            c.get("active")!=Some(&Value::Bool(false))
        ))
        .ok_or("운영 거래처 명단에서 선택한 거래처를 확인할 수 없습니다.")?;
    let now=Utc::now().to_rfc3339();
    // Client-specific product aliases can change the recognized product IDs.
    // Re-recognize using the SELECTED full master client and catalog before
    // merging, instead of repricing stale evidence from another dealer.
    let evidence=gateway.execute(ReadOperation::AnalyzeOrder,json!({
        "raw":source.raw, "state":state, "client":client,
    }))?;
    check_cancelled(cancel)?;
    if !evidence.get("recipients").is_some_and(Value::is_array) ||
       !evidence.get("candidates").is_some_and(Value::is_array) {
        return Err("선택한 거래처 기준 Rust 발주 인식 결과가 잘못되었습니다.".into());
    }

    // Establish a real Rust Order shape; an egui WorkingReception is NOT a
    // full business Order and cannot be passed directly to change_client.
    let created=gateway.execute(ReadOperation::NewReception,json!({
        "action":"new_reception", "id":source.id,
        "clientId":"", "now":now, "orderType":source.order_type,
    }))?;
    check_cancelled(cancel)?;
    let blank=required(&created,"order")?;
    let changed=gateway.execute(ReadOperation::ChangeOrderClient,json!({
        "action":"change_order_client", "state":state, "order":blank,
        "dealerId":target_id, "now":now,
    }))?;
    check_cancelled(cancel)?;
    let selected=required(&changed,"order")?;
    // Only native Rust may merge recognized recipients, stable item IDs and
    // customer-specific business defaults into the Order.
    let merged=gateway.execute(ReadOperation::MergeReceptionAnalysis,json!({
        "previous":selected, "evidence":&evidence,
        "raw":source.raw, "clientId":target_id,
        "client":client, "state":state, "at":now,
    }))?;
    check_cancelled(cancel)?;
    let merged_order=required(&merged,"order")?;

    // Manual corrections are authoritative recipient/item input, not an old
    // dealer's quote. Reapply them to the NEW client order through the same
    // native Rust reducer used by the in-place editor; then reprice all
    // non-overridden lines, freight and VAT with the selected client.
    if let Some(saved)=source.manual_model.as_ref(){
        let model=saved.get("model").filter(|value|value.is_object())
            .ok_or("저장된 수동 수정 작업본이 올바르지 않습니다.")?;
        let keys=saved.get("assignmentOrder").filter(|value|value.is_array())
            .ok_or("수동 수정 원문 연결 순서가 없습니다.")?;
        let applied=gateway.execute(ReadOperation::ManualClassification,json!({
            "action":"preview_manual","order":merged_order,
            "model":model,"assignmentOrder":keys,
        }))?;
        check_cancelled(cancel)?;
        let edited=required(&applied,"order")?;
        let quote=gateway.execute(ReadOperation::CalculateReceptionQuote,json!({
            "order":edited,"state":state,"client":client,
        }))?;
        check_cancelled(cancel)?;
        let order=required(&quote,"order")?;
        let totals=required(&quote,"totals")?;
        if order.get("id").and_then(Value::as_str)!=Some(source.id.as_str()) ||
           order.get("clientId").and_then(Value::as_str)!=Some(target_id) ||
           order.get("raw").and_then(Value::as_str)!=Some(source.raw.as_str()) {
            return Err("수동 수정 거래처 재계산 결과가 현재 발주서와 일치하지 않습니다.".into());
        }
        let recipients=required(order,"recipients")?;
        let manual_model=rebind_manual_model(saved,target_id,recipients)?;
        let mut selected_recognition=evidence.clone();
        selected_recognition["recipients"]=recipients.clone();
        let preview=json!({
            "sourceRaw":source.raw,"orderId":source.id,
            "orderType":source.order_type,"clientId":target_id,
            "authorityRevision":authority_revision,
            "status":order.get("status"),
            "shippingFee":order.get("shippingFee"),
            "shippingVersion":order.get("shippingVersion"),
            "vatAmount":totals.get("vat"),
            "subtotal":totals.get("subtotal"),
            "total":totals.get("total"),
            "pendingRecipients":totals.get("pendingRecipients"),
            "notified":order.get("notified"),
            "paid":order.get("paid"),
            "linePrices":quote.get("linePrices"),
            "freightLines":quote.get("freightLines"),
            "pricingBasis":crate::pricing_basis::snapshot(
                &state,client,order
            ),
        });
        return Ok(ChangeResult {
            client_id:target_id.to_owned(),
            pricing_preview:Some(preview),
            order:Some(order.clone()),
            manual_model:Some(manual_model),
            recognition:Some(selected_recognition),
        });
    }

    // Use the SAME shipment-aware Rust quote as manual edits. The legacy
    // CalculatePrice route has no per-recipient freight, no amount fields in
    // linePrices, and no pendingRecipients diagnostics for incomplete orders.
    let quote=gateway.execute(ReadOperation::CalculateReceptionQuote,json!({
        "order":merged_order,"state":state,"client":client,
    }))?;
    check_cancelled(cancel)?;
    let order=required(&quote,"order")?;
    let totals=required(&quote,"totals")?;
    if order.get("id").and_then(Value::as_str)!=Some(source.id.as_str()) ||
       order.get("clientId").and_then(Value::as_str)!=Some(target_id) ||
       order.get("raw").and_then(Value::as_str)!=Some(source.raw.as_str()) {
        return Err("Rust 주문 변경 결과가 현재 발주서와 일치하지 않습니다.".into());
    }
    if !quote.get("linePrices").is_some_and(Value::is_array) ||
       !quote.get("freightLines").is_some_and(Value::is_array) {
        return Err("Rust 품목·운임 계산 결과가 올바르지 않습니다.".into());
    }

    // Allowlist: never store the full customer snapshot or recipient PII.
    // Amounts and freight are calculated by native Rust, never multiplied in UI.
    let price_summary=json!({
        "sourceRaw":source.raw,
        "orderId":source.id,
        "orderType":source.order_type,
        "clientId":target_id,
        "authorityRevision":authority_revision,
        "status":order.get("status"),
        "shippingFee":order.get("shippingFee"),
        "shippingVersion":order.get("shippingVersion"),
        "vatAmount":totals.get("vat"),
        "subtotal":totals.get("subtotal"),
        "total":totals.get("total"),
        "pendingRecipients":totals.get("pendingRecipients"),
        "notified":order.get("notified"),
        "paid":order.get("paid"),
        "linePrices":quote.get("linePrices"),
        "freightLines":quote.get("freightLines"),
        "pricingBasis":crate::pricing_basis::snapshot(&state,client,order),
    });
    Ok(ChangeResult {
        client_id:target_id.to_owned(),
        pricing_preview:Some(price_summary),
        order:Some(order.clone()),
        manual_model:None,
        recognition:Some(evidence),
    })
}

#[cfg(test)]
mod manual_rebind_tests {
    use super::*;
    #[test]
    fn client_change_keeps_staff_rows_and_source_assignments(){
        let saved=json!({
            "sourceRaw":"원문","orderType":"AGENCY","clientId":"old",
            "model":{
                "rows":[{"id":"r1","name":"수정 전","items":[]}],
                "blocks":[{"id":"0:2","text":"원문"}],
                "assignments":{"0:2":{"recipientId":"r1","field":"name"}}
            },
            "assignmentOrder":["0:2"]
        });
        let recalculated=json!([{"id":"r1","name":"직접 수정한 이름",
            "address":"직접 수정한 주소",
            "items":[{"id":"i1","quantity":3,"manualUnitPrice":9900,"unitPrice":9900}]}]);
        let updated=rebind_manual_model(&saved,"new",&recalculated).unwrap();
        assert_eq!(updated["clientId"],"new");
        assert_eq!(updated["model"]["rows"],recalculated);
        assert_eq!(updated["model"]["blocks"],saved["model"]["blocks"]);
        assert_eq!(updated["model"]["assignments"],saved["model"]["assignments"]);
        assert_eq!(updated["assignmentOrder"],saved["assignmentOrder"]);
        assert_eq!(saved["clientId"],"old");
    }

    #[test]
    fn dealer_selection_without_manual_edits_uses_complete_native_quote(){
        // This path formerly called CalculatePrice and omitted both the
        // per-line amount and shipment freight data. The mock implements
        // CalculateReceptionQuote only, so a regression fails explicitly.
        let source=WorkingReception{
            id:"order-1".into(),raw:"원문".into(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some("원문".into()),
            analysis_evidence:Some(json!({"recipients":[],"candidates":[]})),
            analysis_view:None,client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,manual_model:None,
            edit_history:vec![]
        };
        let result=prepare(&PreviewGateway,&source,"new").unwrap();
        assert_eq!(result.order.as_ref().and_then(|order|order["id"].as_str()),
            Some("order-1"),"the explicit confirmation path needs the exact native Order");
        assert_eq!(result.recognition,Some(json!({
            "recipients":[],"candidates":[]
        })),"the selected-client recognition must accompany the price quote");
        let cancelled=AtomicBool::new(true);
        assert!(prepare_cancellable(&PreviewGateway,&source,"new",&cancelled)
            .err().is_some_and(|error|error.contains("취소")),
            "a superseded auto-match must stop without entering native rules");
        let preview=result.pricing_preview.unwrap();
        assert_eq!(preview["total"],27300);
        assert_eq!(preview["pendingRecipients"],json!([]));
        assert_eq!(preview["linePrices"],json!([]));
        assert_eq!(preview["freightLines"],json!([]));
        assert_eq!(preview["clientId"],"new");
    }

    #[test]
    fn direct_entry_client_change_uses_native_quote_without_source_recognition(){
        let model=json!({"rows":[{
            "id":"egui-manual-direct-r1","name":"직접 수정한 이름",
            "address":"서울특별시 강남구 테헤란로 10",
            "phone":"","mobile":"","note":"",
            "items":[{"id":"egui-manual-direct-i1","productId":"p","variantId":"v",
                "name":"BD 709","quantity":2,"quantityConfirmed":true,
                "manualUnitPrice":9900,"unitPrice":9900}]
        }],"blocks":[],"assignments":{}});
        let source=WorkingReception{
            id:"egui-direct-order".into(),raw:String::new(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:None,analysis_evidence:None,analysis_view:None,
            client_match:None,chat_handle:None,chat_title:None,
            client_id:None,client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,
            manual_model:Some(json!({
                "mode":"DIRECT","sourceRaw":"","orderType":"AGENCY",
                "clientId":null,"model":model,"assignmentOrder":[]
            })),
            edit_history:vec![]
        };
        let result=prepare(&PreviewGateway,&source,"new").unwrap();
        assert_eq!(result.order.as_ref().and_then(|order|order["id"].as_str()),
            Some("egui-direct-order"));
        assert!(result.recognition.is_none());
        assert_eq!(result.manual_model.as_ref().unwrap()["mode"],"DIRECT");
        assert_eq!(result.manual_model.as_ref().unwrap()["clientId"],"new");
        assert_eq!(result.manual_model.as_ref().unwrap()["model"]["rows"][0]["name"],
            "직접 수정한 이름");
        assert_eq!(result.pricing_preview.as_ref().unwrap()["total"],27300);
        assert_eq!(result.pricing_preview.as_ref().unwrap()["sourceRaw"],"");
    }

    #[test]
    fn malformed_model_does_not_replace_a_valid_client_selection(){
        let saved=json!({"model":{"rows":[]}});
        assert!(rebind_manual_model(&saved,"new",&json!({})).is_err());
        assert!(rebind_manual_model(&json!({}),"new",&json!([])).is_err());
    }

    // Exercise the complete adapter request chain with deterministic native
    // gateway responses. Core pricing and manual reducer rules have their
    // own native tests; this verifies that the adapter actually invokes them.
    struct PreviewGateway;
    impl BandoGateway for PreviewGateway {
        fn label(&self)->&'static str{"manual-price-preview-test"}
        fn load_catalog(&self)->Result<Value,String>{Ok(json!({}))}
        fn load_working_source(&self)
            ->Result<crate::preview_store::WorkingSource,String>{
            Err("unused".into())
        }
        fn save_working_source(&self,_:&crate::preview_store::WorkingSource)
            ->Result<u64,String>{Err("unused".into())}
        fn load_business_state(&self)->Result<Value,String>{
            Ok(json!({"revision":7,"orders":[],
                "clients":[{"id":"new","active":true}],
                "products":[]}))
        }
        fn execute(&self,op:ReadOperation,payload:Value)->Result<Value,String>{
            match op {
                ReadOperation::AnalyzeOrder=>Ok(json!({
                    "recipients":[],"candidates":[]
                })),
                ReadOperation::NewReception=>Ok(json!({"order":{
                    "id":payload["id"],"raw":"","clientId":payload["clientId"],
                    "orderType":payload["orderType"],"recipients":[]
                }})),
                ReadOperation::ChangeOrderClient=>Ok(json!({"order":{
                    "id":payload["order"]["id"],"raw":payload["order"]["raw"],
                    "clientId":payload["dealerId"],
                    "orderType":payload["order"]["orderType"],
                    "recipients":[]
                }})),
                ReadOperation::MergeReceptionAnalysis=>Ok(json!({"order":{
                    "id":payload["previous"]["id"],
                    "clientId":payload["clientId"],
                    "orderType":payload["previous"]["orderType"],
                    "raw":payload["raw"],"status":"REVIEW","recipients":[]
                }})),
                ReadOperation::ManualClassification=>{
                    assert_eq!(payload["action"],"preview_manual");
                    assert_eq!(payload["order"]["clientId"],"new");
                    assert_eq!(payload["model"]["rows"][0]["name"],"직접 수정한 이름");
                    let mut order=payload["order"].clone();
                    order["recipients"]=payload["model"]["rows"].clone();
                    Ok(json!({"order":order}))
                }
                ReadOperation::CalculateReceptionQuote=>{
                    assert_eq!(payload["client"]["id"],"new");
                    let mut order=payload["order"].clone();
                    let rows=order["recipients"].as_array_mut().unwrap();
                    for row in rows {
                        for item in row["items"].as_array_mut().unwrap() {
                            let unit_price=item.get("manualUnitPrice")
                                .filter(|price|!price.is_null())
                                .cloned().unwrap_or_else(||json!(5000));
                            item["unitPrice"]=unit_price;
                        }
                    }
                    order["shippingFee"]=json!(2500);
                    Ok(json!({"order":order,
                        "totals":{"subtotal":24800,"vat":0,
                            "total":27300,"pendingRecipients":[]},
                        "linePrices":[],"freightLines":[]}))
                }
                _=>Err(format!("Unexpected operation: {:?}",op))
            }
        }
    }

    #[test]
    fn changing_dealer_requotes_manual_rows_without_losing_edits(){
        let raw="김가상\\n테스트 발주";
        let model=json!({"rows":[{
            "id":"recipient-1","name":"직접 수정한 이름",
            "address":"직접 수정한 주소",
            "items":[
                {"id":"manual-line","productId":"p","variantId":"v",
                    "quantity":2,"manualUnitPrice":9900,"unitPrice":9900},
                {"id":"catalog-line","productId":"p","variantId":"v",
                    "quantity":1,"unitPrice":1000}
            ]}],
            "blocks":[{"id":"0:2","text":"김가"}],
            "assignments":{}
        });
        let row=WorkingReception{
            id:"order-1".into(),raw:raw.into(),
            order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some(raw.into()),
            analysis_evidence:Some(json!({"recipients":[]})),
            analysis_view:None,client_match:None,chat_handle:None,chat_title:None,
            client_id:Some("old".into()),
            client_selection:Some("MANUAL".into()),
            pricing_preview:None,working_version:None,
            working_synced:false,
            manual_model:Some(json!({"sourceRaw":raw,
                "orderType":"AGENCY","clientId":"old",
                "model":model.clone(),"assignmentOrder":[]})),
            edit_history:vec![json!({"beforeRows":[],"afterRows":[1]})],
        };
        let result=prepare(&PreviewGateway,&row,"new").unwrap();
        let manual=result.manual_model.unwrap();
        assert_eq!(result.client_id,"new");
        assert_eq!(result.pricing_preview.unwrap()["total"],27300);
        assert_eq!(manual["clientId"],"new");
        assert_eq!(manual["model"]["rows"][0]["name"],"직접 수정한 이름");
        assert_eq!(manual["model"]["rows"][0]["address"],"직접 수정한 주소");
        assert_eq!(manual["model"]["rows"][0]["items"][0]["unitPrice"],9900);
        assert_eq!(manual["model"]["rows"][0]["items"][1]["unitPrice"],5000);
        assert_eq!(manual["model"]["blocks"],model["blocks"]);
        assert_eq!(manual["model"]["assignments"],model["assignments"]);
        assert_eq!(row.edit_history.len(),1);
        assert_eq!(row.client_id.as_deref(),Some("old"));
    }
}
