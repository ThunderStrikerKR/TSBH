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

pub struct ChangeResult {
    pub client_id: String,
    pub pricing_preview: Value,
}

fn required<'a>(value:&'a Value,key:&str)->Result<&'a Value,String>{
    value.get(key).filter(|v|!v.is_null()).ok_or_else(||
        format!("Rust 주문 변경 결과에 {key}가 없습니다."))
}

/// The request snapshot is the precise reception at employee click time.
pub fn prepare(gateway:&dyn BandoGateway, source:&WorkingReception,
               target_id:&str) -> Result<ChangeResult,String> {
    if source.raw.trim().is_empty() {
        return Err("발주원문이 없는 주문은 거래처를 변경할 수 없습니다.".into());
    }
    if target_id.trim().is_empty() {
        return Err("변경할 거래처가 없습니다.".into());
    }
    if source.analysis_raw.as_deref()!=Some(source.raw.as_str()) ||
       source.analysis_evidence.is_none() {
        return Err("현재 원문의 Rust 분석이 완료된 뒤 거래처를 선택하세요.".into());
    }

    // Never use the name-only client index or the embedded retail seed to
    // calculate actual customer prices, shipping waivers or overrides.
    let state=gateway.load_business_state()?;
    let authority_revision=state.get("revision").and_then(Value::as_u64)
        .ok_or("운영 DB revision이 유효하지 않습니다.")?;
    if state.get("orders").and_then(Value::as_array).is_some_and(|orders|
        orders.iter().any(|order|
            order.get("id").and_then(Value::as_str)==Some(source.id.as_str())
        )) {
        return Err("운영 DB에 이미 존재하는 주문은 egui 미확정 발주서로 수정할 수 없습니다.".into());
    }
    if let Some(old)=&source.pricing_preview {
        // A stored price quote must never survive a changed master revision
        // when the employee is proposing a subsequent client edit.
        if old.get("authorityRevision").and_then(Value::as_u64)
            != Some(authority_revision) {
            return Err("거래처·가격·운임 기준이 변경됐습니다. 최신 DB를 기준으로 다시 확인하세요.".into());
        }
    }
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
    let blank=required(&created,"order")?;
    let changed=gateway.execute(ReadOperation::ChangeOrderClient,json!({
        "action":"change_order_client", "state":state, "order":blank,
        "dealerId":target_id, "now":now,
    }))?;
    let selected=required(&changed,"order")?;
    // Only native Rust may merge recognized recipients, stable item IDs and
    // customer-specific business defaults into the Order.
    let merged=gateway.execute(ReadOperation::MergeReceptionAnalysis,json!({
        "previous":selected, "evidence":evidence,
        "raw":source.raw, "clientId":target_id,
        "client":client, "state":state, "at":now,
    }))?;
    let merged_order=required(&merged,"order")?;
    let priced=gateway.execute(ReadOperation::RepriceOrder,json!({
        "order":merged_order, "state":state, "client":client,
        "explicitEdit":true, "persistedOrders":[],
    }))?;
    let order=required(&priced,"order")?;
    // Billing remains a pure native calculation, never an egui formula.
    // The result may legitimately be null until items or freight are known.
    let totals=gateway.execute(ReadOperation::CalculatePrice,json!({
        "order":order, "client":client,
    }))?;
    if order.get("id").and_then(Value::as_str)!=Some(source.id.as_str()) ||
       order.get("clientId").and_then(Value::as_str)!=Some(target_id) ||
       order.get("raw").and_then(Value::as_str)!=Some(source.raw.as_str()) {
        return Err("Rust 주문 변경 결과가 현재 발주서와 일치하지 않습니다.".into());
    }

    // The preview is intentionally an allowlist; it must never persist the
    // full clientSnapshot, recipient PII, company payment details or history.
    let receipts=order.get("recipients").and_then(Value::as_array)
        .ok_or("Rust 주문 수취인 목록이 잘못되었습니다.")?;
    let lines:Vec<Value>=receipts.iter().flat_map(|recipient| {
        recipient.get("items").and_then(Value::as_array)
            .into_iter().flatten().map(|item|json!({
                "itemId":item.get("id"),
                "productId":item.get("productId"),
                "variantId":item.get("variantId"),
                "quantity":item.get("quantity"),
                "unitPrice":item.get("unitPrice"),
            }))
    }).collect();
    let price_summary=json!({
        "sourceRaw":source.raw,
        "orderId":source.id,
        "clientId":target_id,
        "authorityRevision":authority_revision,
        "status":order.get("status"),
        "shippingFee":order.get("shippingFee"),
        "shippingVersion":order.get("shippingVersion"),
        "vatAmount":totals.get("vat"),
        "subtotal":totals.get("subtotal"),
        "total":totals.get("total"),
        "notified":order.get("notified"),
        "paid":order.get("paid"),
        "linePrices":lines,
    });
    Ok(ChangeResult {
        client_id:target_id.to_owned(),
        pricing_preview:price_summary,
    })
}
