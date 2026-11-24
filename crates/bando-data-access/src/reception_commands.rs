//! Native unconfirmed reception CAS writer shared with egui.
//! This performs NO confirmed order insert, payment mutation or audit unlock.
//! Original Rust recognition, manual classification, order edit, repricing
//! and freight rules execute INSIDE one IMMEDIATE SQLite transaction.
//! The caller owns the explicitly authorized existing schema-v2 connection.

use rusqlite::{params,Connection,OptionalExtension,TransactionBehavior};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}

fn payload_hash(order:&Value)->Result<String,String>{
    let mut data=order.clone();
    data.as_object_mut().ok_or("작업본 객체 형식 오류")?
        .remove("_eguiBridgePayloadHash");
    let bytes=serde_json::to_vec(&data).map_err(err)?;
    Ok(format!("{:x}",Sha256::digest(&bytes)))
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("egui 접수 전달에 {key} 값이 없습니다."))
}

fn rule(operation: &str, payload: Value) -> Result<Value, String> {
    crate::rules::execute(operation, payload)
}

fn order_from(result: Value) -> Result<Value, String> {
    result.get("order").filter(|value| value.is_object()).cloned()
        .ok_or_else(|| "Rust 주문 수정 결과에 주문 객체가 없습니다.".into())
}

fn same_id(order: &Value, order_id: &str) -> bool {
    order.get("id").and_then(Value::as_str) == Some(order_id)
}

pub fn commit_reception_v2(conn: &mut Connection, request: &Value) -> Result<Value, String> {
    let order_id = field(request, "orderId")?;
    let raw = request.get("sourceRaw").and_then(Value::as_str)
        .ok_or("egui 접수 전달에 sourceRaw 값이 없습니다.")?;
    let kind = field(request, "orderType")?;
    let client_id = field(request, "clientId")?;
    // A caller must not forge native order timestamps or audit chronology.
    let at = chrono::Utc::now().to_rfc3339();
    let expected_revision = request.get("expectedAuthorityRevision")
        .and_then(Value::as_u64)
        .ok_or("운영 DB revision이 누락됐습니다.")?;
    let expected_working_version = request.get("expectedWorkingVersion")
        .and_then(Value::as_u64);
    let manual = request.get("manualModel").filter(|v|!v.is_null());
    let direct=manual.is_some_and(|model|
        model.get("mode").and_then(Value::as_str)==Some("DIRECT")
    );
    if raw.trim().is_empty() && !direct{
        return Err("원문이 없는 접수는 직접 입력 작업본이어야 합니다.".into());
    }
    if direct && !raw.trim().is_empty(){
        return Err("직접 입력 작업본에는 발주원문을 포함할 수 없습니다.".into());
    }
    if (!order_id.starts_with("egui-") && order_id!="legacy-working-source") ||
       order_id.len() > 180 {
        return Err("egui에서 생성한 미확정 접수 ID만 이 경로로 저장할 수 있습니다.".into());
    }
    if !matches!(kind, "AGENCY" | "DEALER") {
        return Err("대리발송/대리점 납품 외의 접수는 이 경로에서 처리하지 않습니다.".into());
    }
    if raw.encode_utf16().count() > 100_000 {
        return Err("발주원문 길이가 100,000자를 초과했습니다.".into());
    }
    // Use the same instant of truth for rule evaluation and SQLite insertion.
    // A Tauri state_save and another egui helper cannot interleave this
    // transaction. A stale master revision always aborts, no partial writes.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    let version: i64 = tx.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [], |row| row.get(0),
    ).map_err(|_| "운영 DB의 관계형 스키마를 확인할 수 없습니다.".to_owned())?;
    if version != 2 {
        return Err("운영 DB 관계형 버전 2만 지원합니다.".into());
    }
    let state = crate::read_v2_snapshot(&tx)?;
    if state.get("revision").and_then(Value::as_u64) != Some(expected_revision) {
        return Err("운영 거래처·가격·운임 기준이 변경됐습니다. 다시 조회한 후 저장하세요.".into());
    }
    if state.get("orders").and_then(Value::as_array).is_some_and(|orders|
        orders.iter().any(|order| same_id(order, order_id))
    ) {
        return Err("이미 주문 확인 완료된 발주서는 미확정 접수로 덮어쓸 수 없습니다.".into());
    }
    let indexed:bool=tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM order_index WHERE id=?1)",
        params![order_id],|row|row.get(0),
    ).map_err(err)?;
    if indexed {
        return Err("운영 주문 인덱스에 있는 주문은 미확정 접수로 저장할 수 없습니다.".into());
    }
    let selected_client = state.get("clients").and_then(Value::as_array)
        .and_then(|clients| clients.iter().find(|client|
            client.get("id").and_then(Value::as_str) == Some(client_id) &&
            client.get("deletedAt").is_none_or(Value::is_null) &&
            client.get("active") != Some(&Value::Bool(false))
        ))
        .ok_or("선택한 거래처를 운영 DB에서 찾을 수 없습니다.")?;

    let saved: Option<String> = tx.query_row(
        "SELECT payload FROM reception_working_orders WHERE id=?1",
        params![order_id], |row| row.get(0),
    ).optional().map_err(err)?;
    let previous = match saved {
        Some(serialized) => {
            let existing: Value = serde_json::from_str(&serialized).map_err(err)?;
            if !same_id(&existing, order_id) {
                return Err("운영 미확정 발주서 ID가 일치하지 않습니다.".into());
            }
            let version = existing.get("_eguiBridgeVersion").and_then(Value::as_u64)
                .ok_or("기존 Tauri 작업본을 egui가 임의로 덮어쓸 수 없습니다.")?;
            if Some(version) != expected_working_version {
                return Err("미확정 주문이 다른 창에서 수정됐습니다. 새로고침 후 다시 확인하세요.".into());
            }
            let saved_hash=existing.get("_eguiBridgePayloadHash").and_then(Value::as_str)
                .ok_or("egui 작업본에 무결성 확인값이 없습니다.")?;
            if saved_hash!=payload_hash(&existing)? {
                return Err("운영 작업본이 다른 화면에서 수정됐습니다. 내용을 확인한 뒤 다시 진행하세요.".into());
            }
            if existing.get("confirmedAt").and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()) {
                return Err("이미 확인 완료된 주문은 이 경로로 변경할 수 없습니다.".into());
            }
            if manual.is_none() &&
               existing.get("manualClassification").is_some_and(|v|!v.is_null()) &&
               existing.get("raw").and_then(Value::as_str)==Some(raw) {
                return Err("수동 분류 작업본을 자동 인식 결과로 덮어쓸 수 없습니다.".into());
            }
            existing
        }
        None => {
            if expected_working_version.is_some() {
                return Err("기존 미확정 주문이 사라졌습니다. 다시 확인하세요.".into());
            }
            order_from(rule("apply_reception_action", json!({
                "action": "new_reception",
                "id": order_id, "clientId": "",
                "orderType": kind, "now": at,
            }))?)?
        }
    };
    if previous.get("orderType").and_then(Value::as_str) != Some(kind) &&
        expected_working_version.is_some() {
        // Changing order type needs its own explicit business transition.
        return Err("이미 저장한 주문의 접수 유형 변경은 별도 검증 후 처리해야 합니다.".into());
    }
    // Only the existing Rust reception transition is allowed to reset and
    // replace a changed raw source; never patch the Order fields by hand.
    let previous=if manual.is_some(){
        // The employee's approved source-bound manual model is replayed
        // against fresh native recognition from an empty reception. The
        // persisted receipt is still used above for CAS and tamper checks.
        order_from(rule("apply_reception_action",json!({
            "action":"new_reception","id":order_id,
            "orderType":kind,"clientId":"","now":at
        }))?)?
    }else if previous.get("raw").and_then(Value::as_str)!=Some(raw){
        order_from(rule("apply_reception_action",json!({
            "action":"replace_reception","order":previous,
            "raw":raw,"now":at
        }))?)?
    }else{previous};

    // Source-backed reception runs native recognition and merges evidence.
    // Direct entry has no original source, so it skips recognition entirely
    // and replays the same manual model against a blank native order.
    let merged=if direct{
        let changed=order_from(rule("order_edit_rules",json!({
            "action":"change_order_client",
            "state":&state,"order":previous,
            "dealerId":client_id,"now":at
        }))?)?;
        crate::reception_manual::verify_apply(
            &changed,&state,selected_client,
            manual.ok_or("직접 입력 작업본이 없습니다.")?
        )?
    }else{
        let evidence=crate::recognition::execute("analyze_order",json!({
            "raw":raw,"state":&state,"client":selected_client
        }))?;
        if !evidence.get("recipients").is_some_and(Value::is_array) ||
           !evidence.get("candidates").is_some_and(Value::is_array){
            return Err("발주 인식 결과가 올바르지 않습니다.".into());
        }
        let changed=order_from(rule("order_edit_rules",json!({
            "action":"change_order_client",
            "state":&state,"order":previous,
            "dealerId":client_id,"now":at
        }))?)?;
        let merged=order_from(rule("merge_reception_analysis",json!({
            "previous":changed,"evidence":evidence,
            "raw":raw,"clientId":client_id,
            "client":selected_client,"state":&state,"at":at
        }))?)?;
        if let Some(manual_model)=manual{
            // Keep the source-backed editor's empty analysisEvidence contract,
            // then restore native evidence for the operational snapshot.
            let mut baseline=merged.clone();
            baseline["analysisEvidence"]=json!([]);
            let mut applied=crate::reception_manual::verify_apply(
                &baseline,&state,selected_client,manual_model
            )?;
            applied["analysisEvidence"]=merged.get("analysisEvidence")
                .cloned().unwrap_or(Value::Null);
            applied
        }else{merged}
    };
    let mut next = order_from(rule("reprice_order", json!({
        "order":merged, "state":&state, "client":selected_client,
        "explicitEdit":true, "persistedOrders":[]
    }))?)?;
    let mut manual_quote=None;
    if manual.is_some(){
        // The *same* read-only native quote as the egui editing preview:
        // item unit price overrides, quantity, per-recipient base charges,
        // COD, and remote 2x are resolved against this transaction's state.
        let quoted=rule("calculate_reception_quote",json!({
            "order":&next,"state":&state,"client":selected_client
        }))?;
        next=order_from(quoted.clone())?;
        manual_quote=Some(quoted);
    }
    if !same_id(&next, order_id) ||
       next.get("raw").and_then(Value::as_str) != Some(raw) ||
       next.get("clientId").and_then(Value::as_str) != Some(client_id) ||
       next.get("receptionVersion").and_then(Value::as_i64)!=Some(1) ||
       next.get("status").and_then(Value::as_str)==Some("EXPORTED") ||
       next.get("confirmedAt").and_then(Value::as_str)
           .is_some_and(|v| !v.is_empty()) {
        return Err("재계산된 미확정 주문이 요청과 일치하지 않습니다.".into());
    }
    let totals=if let Some(quoted)=manual_quote.as_ref(){
        quoted.get("totals").filter(|value|value.is_object())
            .ok_or("재계산된 청구합계가 없습니다.")?.clone()
    }else{
        rule("calculate_price", json!({
            "order":&next, "client":selected_client
        }))?
    };
    let working_version = expected_working_version.unwrap_or(0).checked_add(1)
        .ok_or("미확정 주문 버전 범위를 초과했습니다.")?;
    next["_eguiBridgeVersion"] = json!(working_version);
    next["_eguiBridgeAuthorityRevision"] = json!(expected_revision);
    next["_eguiBridgePayloadHash"] = json!(payload_hash(&next)?);
    let encoded = serde_json::to_string(&next).map_err(err)?;
    if encoded.len() > 50_000_000 {
        return Err("미확정 주문 크기 한도를 초과했습니다.".into());
    }
    let changed_at = chrono::Utc::now().to_rfc3339();
    let affected = if expected_working_version.is_some() {
        tx.execute(
            "UPDATE reception_working_orders SET payload=?1,changed_at=?2
             WHERE id=?3 AND json_extract(payload,'$._eguiBridgeVersion')=?4",
            params![encoded, changed_at, order_id, expected_working_version],
        ).map_err(err)?
    } else {
        tx.execute(
            "INSERT INTO reception_working_orders(id,changed_at,payload)
             VALUES(?1,?2,?3)",
            params![order_id, changed_at, encoded],
        ).map_err(err)?
    };
    if affected != 1 {
        return Err("미확정 주문 버전 충돌로 저장하지 않았습니다.".into());
    }
    tx.commit().map_err(err)?;
    // The CLI never returns contact details, recipients, account information,
    // customer snapshots, or full order history to the egui UI.
    let lines=if let Some(quote)=manual_quote.as_ref(){
        quote.get("linePrices").and_then(Value::as_array)
            .ok_or("수동 수정 품목별 재계산 결과가 누락됐습니다.")?.clone()
    }else{
        next.get("recipients").and_then(Value::as_array)
            .into_iter().flatten().flat_map(|row|
                row.get("items").and_then(Value::as_array).into_iter().flatten()
            ).map(|item|json!({
                "itemId":item.get("id"),
                "productId":item.get("productId"),
                "variantId":item.get("variantId"),
                "quantity":item.get("quantity"),
                "unitPrice":item.get("unitPrice"),
            })).collect::<Vec<_>>()
    };
    let freight_lines=manual_quote.as_ref().and_then(|quote|
        quote.get("freightLines").and_then(Value::as_array))
        .cloned().unwrap_or_default();
    let mut acknowledgement=json!({
        "orderId":order_id,
        "clientId":client_id,
        "workingVersion":working_version,
        "authorityRevision":expected_revision,
        "status":"WORKING",
        "subtotal":totals.get("subtotal"),
        "shippingFee":next.get("shippingFee"),
        "shippingVersion":next.get("shippingVersion"),
        "vatAmount":totals.get("vat"),
        "total":totals.get("total"),
        "linePrices":lines,
        "freightLines":freight_lines,
        "manualApplied":manual.is_some()
    });
    // The one-shot CLI has a deliberately bounded stdout contract. Truncate
    // no individual quote: if its full list is too large, return totals
    // and an empty detail list rather than losing the commit acknowledgement.
    if serde_json::to_vec(&acknowledgement).map_err(err)?.len()>90_000 {
        acknowledgement["linePrices"]=json!([]);
        acknowledgement["freightLines"]=json!([]);
    }
    Ok(acknowledgement)
}

