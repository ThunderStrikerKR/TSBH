//! Direct reuse of the *same* recognition implementation used by
//! src-tauri/src/business_core/mod.rs. No Tauri/webview/SQLite IPC.
//!
//! This is intentionally an isolated, read-only first integration. Prices,
//! freight, workflow mutations and persistence are NOT simulated here.

use serde_json::{json, Value};
use std::sync::OnceLock;

#[path = "../../src-tauri/src/business_core/recognition/mod.rs"]
mod recognition;

/// Analyze an entered purchase-order source with the shipped catalog seed.
/// The seed is immutable, may differ from the live user's catalog, and never
/// touches the production app state or working reception database.
pub fn analyze_raw(raw: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Err("발주원문을 먼저 입력하세요.".to_owned());
    }
    // Loading the bundled catalog repeatedly would parse a large JSON file
    // every time staff check a newly pasted source. Keep one immutable seed.
    static SEED: OnceLock<Result<Value, String>> = OnceLock::new();
    let seed = SEED.get_or_init(|| serde_json::from_str(
        include_str!("../../src-tauri/resources/state-seed.json")
    ).map_err(|error| format!("기본 상품 데이터 파싱 실패: {error}")));
    let state = seed.as_ref().map_err(Clone::clone)?;

    recognition::execute("analyze_order", json!({
        "raw": raw,
        "state": state,
    }))
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
