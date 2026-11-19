//! Controlled egui -> Tauri persistence bridge.
//!
//! It writes ONLY an unconfirmed reception to the EXISTING
//! reception_working_orders table. It never inserts into core_orders or
//! app_state, never confirms an order and never changes payment/shipment
//! history. Business logic comes exclusively from the existing Rust core.
//!
//! The caller must explicitly choose the SQLite path and the Tauri executable.
//! This is a one-shot CLI service, not a background listener or an HTTP port.

use super::{business_core, err, get_snapshot};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::path::Path;

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("egui 접수 전달에 {key} 값이 없습니다."))
}

fn rule(operation: &str, payload: Value) -> Result<Value, String> {
    business_core::execute("rules", operation, payload)
}

fn order_from(result: Value) -> Result<Value, String> {
    result.get("order").filter(|value| value.is_object()).cloned()
        .ok_or_else(|| "Rust 주문 수정 결과에 주문 객체가 없습니다.".into())
}

fn same_id(order: &Value, order_id: &str) -> bool {
    order.get("id").and_then(Value::as_str) == Some(order_id)
}

fn commit_transaction(conn: &mut Connection, request: &Value) -> Result<Value, String> {
    let order_id = field(request, "orderId")?;
    let raw = field(request, "sourceRaw")?;
    let kind = field(request, "orderType")?;
    let client_id = field(request, "clientId")?;
    let at = field(request, "at")?;
    let expected_revision = request.get("expectedAuthorityRevision")
        .and_then(Value::as_u64)
        .ok_or("운영 DB revision이 누락됐습니다.")?;
    let expected_working_version = request.get("expectedWorkingVersion")
        .and_then(Value::as_u64);
    if !order_id.starts_with("egui-") || order_id.len() > 180 {
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
    let state = get_snapshot(&tx)?;
    if state.get("revision").and_then(Value::as_u64) != Some(expected_revision) {
        return Err("운영 거래처·가격·운임 기준이 변경됐습니다. 다시 조회한 후 저장하세요.".into());
    }
    if state.get("orders").and_then(Value::as_array).is_some_and(|orders|
        orders.iter().any(|order| same_id(order, order_id))
    ) {
        return Err("이미 주문 확인 완료된 발주서는 미확정 접수로 덮어쓸 수 없습니다.".into());
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
            if existing.get("confirmedAt").and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()) {
                return Err("이미 확인 완료된 주문은 이 경로로 변경할 수 없습니다.".into());
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
    if previous.get("raw").and_then(Value::as_str)
        .is_some_and(|old| old != raw) &&
        expected_working_version.is_some() {
        // Do not discard employee edits from the persisted native order.
        return Err("운영 작업본의 발주원문이 다릅니다. 내용을 확인한 뒤 다시 진행하세요.".into());
    }

    // Never trust price/total figures or a clientSnapshot supplied by egui.
    // Re-run recognition, dealer guard, item reprice and freight using the
    // fresh, authoritative state from inside the write transaction.
    let evidence = business_core::execute("recognition", "analyze_order", json!({
        "raw":raw, "state":&state, "client":selected_client
    }))?;
    if !evidence.get("recipients").is_some_and(Value::is_array) ||
       !evidence.get("candidates").is_some_and(Value::is_array) {
        return Err("발주 인식 결과가 올바르지 않습니다.".into());
    }
    let changed = order_from(rule("order_edit_rules", json!({
        "action":"change_order_client",
        "state":&state, "order":previous,
        "dealerId":client_id, "now":at
    }))?)?;
    let merged = order_from(rule("merge_reception_analysis", json!({
        "previous":changed,
        "evidence":evidence,
        "raw":raw, "clientId":client_id,
        "client":selected_client, "state":&state, "at":at
    }))?)?;
    let mut next = order_from(rule("reprice_order", json!({
        "order":merged, "state":&state, "client":selected_client,
        "explicitEdit":true, "persistedOrders":[]
    }))?)?;
    if !same_id(&next, order_id) ||
       next.get("raw").and_then(Value::as_str) != Some(raw) ||
       next.get("clientId").and_then(Value::as_str) != Some(client_id) ||
       next.get("confirmedAt").and_then(Value::as_str)
           .is_some_and(|v| !v.is_empty()) {
        return Err("재계산된 미확정 주문이 요청과 일치하지 않습니다.".into());
    }
    let totals = rule("calculate_price", json!({
        "order":&next, "client":selected_client
    }))?;
    let working_version = expected_working_version.unwrap_or(0).checked_add(1)
        .ok_or("미확정 주문 버전 범위를 초과했습니다.")?;
    next["_eguiBridgeVersion"] = json!(working_version);
    next["_eguiBridgeAuthorityRevision"] = json!(expected_revision);
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
    Ok(json!({
        "orderId":order_id,
        "clientId":client_id,
        "workingVersion":working_version,
        "authorityRevision":expected_revision,
        "status":"WORKING",
        "subtotal":totals.get("subtotal"),
        "shippingFee":next.get("shippingFee"),
        "vatAmount":totals.get("vat"),
        "total":totals.get("total")
    }))
}

pub(super) fn commit_file(path: &Path, request: Value) -> Result<Value, String> {
    if !path.is_file() {
        return Err("지정한 Tauri 운영 DB가 존재하지 않습니다.".into());
    }
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| "Tauri 운영 DB를 쓰기 모드로 열 수 없습니다.".to_owned())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;").map_err(err)?;
    commit_transaction(&mut conn, &request)
}
