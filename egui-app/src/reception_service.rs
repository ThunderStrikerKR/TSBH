//! Read-only reception orchestration.
//! It depends only on BandoGateway, never on transport, SQL or Tauri.
//! Recognition and rule decisions belong to the configured gateway.
//!
//! Any displayed retail figure is reference-only, never an invoice total.
//! This service performs no database writes.

use serde_json::{json, Value};
use std::ops::Deref;
use crate::{gateway::{BandoGateway, ReadOperation}, source_highlight};

#[derive(Debug)]
pub struct PriceReference {
    /// From the real Rust "resolve_unit_price" rule at the *retail* tier.
    /// This is a catalog reference only, never a confirmed recipient price.
    pub retail: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct RecipientReference {
    /// Rust resolved remote-area suggestion, checked state and matched reason.
    /// This is a read-only decision tied to the exact recipient address.
    pub remote: Option<Value>,
    pub remote_error: Option<String>,
    pub item_prices: Vec<PriceReference>,
}

#[derive(Debug)]
pub struct ReceptionAnalysis {
    pub recognition: Value,
    pub reference: Vec<RecipientReference>,
    /// Advisory until an actual order-client change is committed in Rust.
    pub client_match: Value,
    pub client_candidates: Vec<Value>,
    pub matched_client_name: Option<String>,
    pub chosen_client_name: Option<String>,
    pub directory_count: usize,
    /// Rust-owned UTF-16 source locations, projected once per analysis.
    pub linked_spans: Vec<source_highlight::LinkedSpan>,
}

impl Deref for ReceptionAnalysis {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.recognition
    }
}

/// Analyze a source, then derive separate *read-only* references through
/// the selected backend (local Rust or an explicitly configured HTTPS API). Build all results in
/// the existing worker thread; never run these computations every UI frame.
pub fn analyze_raw(
    gateway: &dyn BandoGateway, raw: &str,
    windows: Value, active_chat_handle: Option<String>,
    manual_client_id: Option<String>,
) -> Result<ReceptionAnalysis, String> {
    if raw.trim().is_empty() {
        return Err("발주원문을 먼저 입력하세요.".to_owned());
    }
    let state = gateway.load_catalog()?;
    let directory = state.get("clients").and_then(Value::as_array);
    let directory_count = directory.map(Vec::len).unwrap_or(0);

    // The same Rust matcher used by Tauri owns all client-selection logic.
    // Only OS-verified and explicitly selected KakaoTalk windows may enter
    // this boundary. Unselected/invalid handles are represented as [].
    let context = json!({
        "state": &state, "raw": raw,
        "windows": windows,
        "activeChatHandle": active_chat_handle,
    });
    let client_match = gateway.execute(
        ReadOperation::AutoMatchReceptionClient, context.clone()
    )?;
    let candidates = gateway.execute(
        ReadOperation::ReceptionClientCandidates, context
    )?;
    let client_candidates = candidates.get("candidates").and_then(Value::as_array)
        .cloned().unwrap_or_default();

    let client_for = |id: &str| {
        directory?.iter().find(|client|
            text(client, "id") == id &&
            client.get("deletedAt").is_none_or(Value::is_null) &&
            client.get("active") != Some(&Value::Bool(false))
        )
    };
    let automatic = client_match.get("clientId").and_then(Value::as_str)
        .and_then(client_for);
    let matched_client_name = automatic.map(|client| text(client, "name").to_owned());
    let selected = manual_client_id.as_deref().and_then(client_for).or(automatic);
    let chosen_client_name = selected.map(|client| text(client, "name").to_owned());

    // The client ID is valid for Rust product aliases, but this name-only
    // index has NO price tier. Passing only the ID deliberately leaves price
    // calculation unconfirmed; a future production rule owns billing.
    let recognition_client = selected.map(|client| json!({
        "id": text(client, "id"),
    }));
    let analysis = gateway.execute(ReadOperation::AnalyzeOrder, json!({
        "raw": raw, "state": &state, "client": recognition_client,
    }))?;
    let linked_spans=source_highlight::collect(raw,&analysis);
    let reference = recipients(&analysis).iter()
        .map(|recipient| recipient_reference(gateway, recipient, &state))
        .collect();
    Ok(ReceptionAnalysis {
        recognition: analysis,
        reference,
        client_match,
        client_candidates,
        matched_client_name,
        chosen_client_name,
        directory_count,
        linked_spans,
    })
}


/// Resolve the checked state on the complete recipient, not just the
/// address. The native engine owns auto detection, manual override and
/// address-change invalidation; the UI never guesses the checkbox value.
pub fn remote_decision(
    gateway:&dyn BandoGateway,recipient:&Value
)->Result<Value,String>{
    gateway.execute(ReadOperation::ResolveRemoteDeliveryArea,json!({
        "recipient":recipient
    }))
}

/// Read-only, synchronous helper used ONLY inside existing background workers
/// after a manual model has replaced the initial recognition projection.
pub fn refresh_remote_references(
    gateway:&dyn BandoGateway,analysis:&mut ReceptionAnalysis
){
    let recipients=recipients(&analysis.recognition).to_vec();
    for (index,recipient) in recipients.iter().enumerate(){
        let result=if text(recipient,"address").trim().is_empty(){
            Ok(None)
        }else{
            remote_decision(gateway,recipient).map(Some)
        };
        let entry=if let Some(reference)=analysis.reference.get_mut(index){
            reference
        }else{
            analysis.reference.push(RecipientReference{
                remote:None,remote_error:None,item_prices:Vec::new()
            });
            analysis.reference.last_mut().expect("pushed recipient reference")
        };
        match result{
            Ok(value)=>{entry.remote=value;entry.remote_error=None;},
            Err(error)=>{entry.remote=None;entry.remote_error=Some(error);}
        }
    }
    analysis.reference.truncate(recipients.len());
}

/// Display wording only; status, match path, checked state and reason are
/// always those returned by the Rust remote-area engine.
pub fn remote_delivery_notice(decision:&Value)->Option<String>{
    let status=decision.get("status").and_then(Value::as_str).unwrap_or("unknown");
    let checked=decision.get("checked").and_then(Value::as_bool).unwrap_or(false);
    let path=decision.get("matchedPath").and_then(Value::as_str)
        .unwrap_or("주소 기준 미확정");
    let reason=decision.get("reason").and_then(Value::as_str).unwrap_or("");
    let evidence=if reason.is_empty(){path.to_owned()}
        else{format!("{path} · {reason}")};
    match status{
        "auto"=>Some(if checked{
            format!("도서/산간 자동 판정: {evidence} · 운임 2배 적용")
        }else{
            format!("도서/산간 판정: {evidence} · 수동으로 적용 해제")
        }),
        "review"=>Some(if checked{
            format!("도서/산간 확인 필요: {evidence} · 수동 적용으로 운임 2배")
        }else{
            format!("도서/산간 확인 필요: {evidence} · 확인 후 체크")
        }),
        _ if checked=>Some("도서/산간 수동 적용 · 운임 2배 적용".to_owned()),
        _=>None,
    }
}

fn recipient_reference(gateway: &dyn BandoGateway, recipient: &Value, state: &Value) -> RecipientReference {
    let address = text(recipient, "address");
    let (remote, remote_error) = if address.trim().is_empty() {
        (None,None)
    }else{
        match remote_decision(gateway,recipient){
            Ok(result)=>(Some(result),None),
            Err(error)=>(None,Some(error))
        }
    };

    let item_prices = recipient.get("items").and_then(Value::as_array)
        .map(|items| items.iter().map(|item| retail_reference(gateway, item, state)).collect())
        .unwrap_or_default();
    RecipientReference { remote, remote_error, item_prices }
}

fn retail_reference(gateway: &dyn BandoGateway, item: &Value, state: &Value) -> PriceReference {
    let product_id = text(item, "productId");
    let variant_id = text(item, "variantId");
    if product_id.is_empty() || variant_id.is_empty() {
        return PriceReference { retail: None, error: None };
    }
    let Some(product) = state.get("products").and_then(Value::as_array)
        .and_then(|products| products.iter().find(|product|
            text(product, "id") == product_id
        ))
    else {
        return PriceReference { retail: None, error: None };
    };

    // The pure Rust rule requires a nonempty client ID to resolve a price.
    // This explicitly synthetic identity is NEVER saved. It requests a
    // read-only retail-tier reference, not an actual client's billing tier.
    match gateway.execute(ReadOperation::ResolveRetailUnitPrice, json!({
        "product": product,
        "variantId": variant_id,
        "client": { "id": "__egui_readonly_retail_reference__", "tier": "retail" },
    })) {
        Ok(value) => PriceReference {
            retail: value.get("unitPrice").and_then(Value::as_i64)
                .filter(|amount| *amount > 0),
            error: None,
        },
        Err(error) => PriceReference { retail: None, error: Some(error) },
    }
}

pub fn recipients(result: &Value) -> &[Value] {
    result.get("recipients").and_then(Value::as_array)
        .map(Vec::as_slice).unwrap_or(&[])
}

pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn item_title(item: &Value) -> String {
    let title = text(item, "name");
    if title.trim().is_empty() {
        let original = text(item, "raw");
        if original.trim().is_empty() {
            return "미확정 상품".to_owned();
        }
        return original.to_owned();
    }
    title.to_owned()
}

pub fn item_quantity(item: &Value) -> String {
    match item.get("quantity").and_then(Value::as_i64) {
        Some(quantity) if quantity > 0 => format!("{quantity}개"),
        _ => "수량 확인 필요".to_owned(),
    }
}

/// Display formatting only. The monetary amount is always calculated by
/// the Rust rules above; formatting it never changes the business decision.
pub fn display_won(amount: i64) -> String {
    let digits = amount.to_string();
    let mut output = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    format!("{output}원")
}

#[cfg(test)]
mod remote_notice_tests{
    use super::*;
    #[test]
    fn render_auto_checked_and_manual_override_without_ui_classification(){
        let detection=json!({"status":"auto","checked":true,
            "matchedPath":"제주특별자치도",
            "reason":"제주특별자치도 전역: 항공·도서 권역"});
        let checked=remote_delivery_notice(&detection).unwrap();
        assert!(checked.contains("자동 판정"));
        assert!(checked.contains("제주특별자치도"));
        assert!(checked.contains("운임 2배"));
        let mut override_detection=detection;
        override_detection["checked"]=json!(false);
        let unchecked=remote_delivery_notice(&override_detection).unwrap();
        assert!(unchecked.contains("적용 해제"));
        let review=json!({"status":"review","checked":false,
            "matchedPath":"군산시 옥도면 신시도리",
            "reason":"연륙교 연결 지역"});
        assert!(remote_delivery_notice(&review).unwrap().contains("확인 필요"));
        let normal=json!({"status":"normal","checked":false});
        assert!(remote_delivery_notice(&normal).is_none());
    }
}
