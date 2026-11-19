//! Read-only reception orchestration.
//! It depends only on BandoGateway, never on transport, SQL or Tauri.
//! Recognition and rule decisions belong to the configured gateway.
//!
//! Any displayed retail figure is reference-only, never an invoice total.
//! This service performs no database writes.

use serde_json::{json, Value};
use std::ops::Deref;
use crate::gateway::{BandoGateway, ReadOperation};

#[derive(Debug)]
pub struct PriceReference {
    /// From the real Rust "resolve_unit_price" rule at the *retail* tier.
    /// This is a catalog reference only, never a confirmed recipient price.
    pub retail: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct RecipientReference {
    /// Rust "detect_remote_delivery_area" output, not a freight quote.
    pub remote: Option<Value>,
    pub remote_error: Option<String>,
    pub item_prices: Vec<PriceReference>,
}

#[derive(Debug)]
pub struct NativeAnalysis {
    pub recognition: Value,
    pub reference: Vec<RecipientReference>,
}

impl Deref for NativeAnalysis {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.recognition
    }
}

/// Analyze a source, then derive separate *read-only* references using the
/// same pure-Rust rules that React's Tauri bridge calls. Build all results in
/// the existing worker thread; never run these computations every UI frame.
pub fn analyze_raw(gateway: &dyn BandoGateway, raw: &str) -> Result<NativeAnalysis, String> {
    if raw.trim().is_empty() {
        return Err("발주원문을 먼저 입력하세요.".to_owned());
    }
    let state = gateway.load_state()?;
    let analysis = gateway.execute(ReadOperation::AnalyzeOrder, json!({
        "raw": raw,
        "state": state,
    }))?;
    let reference = recipients(&analysis).iter()
        .map(|recipient| recipient_reference(gateway, recipient, &state))
        .collect();
    Ok(NativeAnalysis { recognition: analysis, reference })
}

fn recipient_reference(gateway: &dyn BandoGateway, recipient: &Value, state: &Value) -> RecipientReference {
    let address = text(recipient, "address");
    let (remote, remote_error) = if address.trim().is_empty() {
        (None, None)
    } else {
        match gateway.execute(ReadOperation::DetectRemoteDeliveryArea, json!({
            "address": address,
        })) {
            Ok(result) => (Some(result), None),
            Err(error) => (None, Some(error)),
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
    // This explicitly synthetic identity is NEVER saved and only requests
    // the seed's retail-tier reference, not an actual client's billing tier.
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
