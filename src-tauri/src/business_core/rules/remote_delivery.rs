//! Read-only, reviewed, conservative remote/island locality classifier.
//! Data was compiled from multiple source lists on 2026-11-19. No tariff is
//! calculated here: the optional 2x application rule belongs to the freight
//! engine, and staff must remain free to override the automatic suggestion.
use serde_json::{json, Value};
use std::sync::OnceLock;

static ZONES_JSON: &str = include_str!("../../../resources/remote-delivery-zones.json");
static ZONES: OnceLock<Result<Value,String>> = OnceLock::new();

fn tokens(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|v| v.trim_matches(|c: char|
            matches!(c, ',' | ':' | '：' | '(' | ')' | '（' | '）' | '[' | ']')
        ).to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

// Match a complete administrative name token sequence in the beginning of an
// address. A municipality must NEVER be inferred from a road, building name,
// substring or an unrelated sentence mentioning the same city.
fn locality_matches(address: &[String], path: &str) -> bool {
    let parts: Vec<_> = path.split_whitespace().collect();
    if parts.is_empty() || address.len() < parts.len() { return false; }
    // A postcode may appear before the administrative address. Otherwise
    // candidate localities may start only at the beginning of the address or
    // after an actual province, never after an unrelated city/building.
    let offset = usize::from(address.first().is_some_and(|s|
        s.chars().all(|c| c.is_ascii_digit()) && s.len() == 5
        || s.starts_with('[') && s.ends_with(']')
    ));
    let is_province = |token: &str| matches!(token,
        "서울특별시" | "부산광역시" | "대구광역시" | "인천광역시" |
        "광주광역시" | "대전광역시" | "울산광역시" | "세종특별자치시" |
        "경기도" | "강원도" | "강원특별자치도" | "충청북도" |
        "충청남도" | "전라북도" | "전북특별자치도" | "전라남도" |
        "경상북도" | "경상남도" | "제주도" | "제주특별자치도" |
        "전남광주통합특별시" | "서울" | "부산" | "대구" | "인천" |
        "광주" | "대전" | "울산" | "경기" | "강원" | "충북" |
        "충남" | "전북" | "전남" | "경북" | "경남"
    );
    let matched = |start: usize| address.len() >= start + parts.len() &&
        parts.iter().enumerate().all(|(i, part)| address[start + i] == *part);
    matched(offset) || address.get(offset).is_some_and(|s| is_province(s)) &&
        matched(offset + 1)
}

pub(super) fn detect(payload: &Value) -> Result<Value, String> {
    let address = payload.get("address").and_then(Value::as_str)
        .ok_or("주소 문자열이 필요합니다.")?;
    // Shipping, preview and settlement all query the same embedded data:
    // parse the verified 99-zone resource only once per process.
    let dataset=ZONES.get_or_init(|| serde_json::from_str(ZONES_JSON)
        .map_err(|e| format!("도서산간 지역 데이터 오류: {e}")))
        .as_ref().map_err(Clone::clone)?;
    let source_tokens = tokens(address);
    let mut chosen: Option<&Value> = None;
    let mut length = 0usize;
    for rule in dataset["rules"].as_array().ok_or("도서산간 데이터 누락")? {
        let path = rule["path"].as_str().unwrap_or("");
        let count = path.split_whitespace().count();
        if count > length && locality_matches(&source_tokens, path) {
            chosen = Some(rule);
            length = count;
        }
    }
    let result = chosen.map(|rule| rule["decision"].as_str().unwrap_or("review"))
        .unwrap_or("unknown");
    // "unknown" means NOT automatically checked, not proof of ordinary freight.
    Ok(json!({
        "status": result,
        "suggestedChecked": result == "auto",
        "requiresReview": result == "review",
        "matchedPath": chosen.map(|r| r["path"].clone()).unwrap_or(Value::Null),
        "reason": chosen.map(|r| r["reason"].clone()).unwrap_or(Value::Null),
        "sources": chosen.map(|r| r["sources"].clone()).unwrap_or(json!([])),
        "dataVersion": dataset["compiledAt"],
    }))
}

/// Resolve the checked state for a recipient without changing the source
/// address, stored base freight or tariff. A staff choice is authoritative only
/// for that exact address; changing the address re-enables auto-detection.
pub(super) fn resolve_recipient(recipient: &Value) -> Result<Value, String> {
    let address = recipient.get("address").and_then(Value::as_str).unwrap_or("");
    let mut analysis = detect(&json!({"address": address}))?;
    let decision = recipient.get("remoteDelivery").unwrap_or(&Value::Null);
    let saved_for_address = decision.get("checked").and_then(Value::as_bool).is_some() &&
        decision.get("address").and_then(Value::as_str) == Some(address);
    let manually_set = saved_for_address &&
        decision.get("manual").and_then(Value::as_bool) == Some(true);
    let checked = if saved_for_address {
        decision["checked"].as_bool().unwrap_or(false)
    } else {
        analysis["suggestedChecked"].as_bool().unwrap_or(false)
    };
    analysis["checked"] = json!(checked);
    analysis["manual"] = json!(manually_set);
    // Tie a visible engine decision to the exact delivery address.
    // Async UI results from an old address must never auto-check a new one.
    analysis["address"] = json!(address);
    Ok(analysis)
}

/// The only multiplier authority: base freight is never pre-doubled or
/// rewritten just because the checkbox is enabled.
pub(super) fn multiply(base: Option<i64>, recipient: &Value) -> Option<i64> {
    let resolved = resolve_recipient(recipient).ok()?;
    let checked = resolved["checked"].as_bool()?;
    let value = base?;
    value.checked_mul(if checked {2} else {1})
        .filter(|n| (0..=super::MAX_SAFE).contains(n))
}

/// A read-only history or preview can run before reconciliation migrates
/// a previously billed order. Do not retroactively double such shipments
/// merely because their old recipient JSON has no locality decision field.
pub(super) fn resolve_shipment(
    recipient: &Value, shipment: &Value, order: &Value
) -> Result<Value, String> {
    let mut result=resolve_recipient(recipient)?;
    let legacy_unmarked=!recipient.get("remoteDelivery")
        .is_some_and(Value::is_object) &&
        (super::flag(shipment,"transferred") ||
         super::field(shipment,"step").as_i64().unwrap_or(0)>=1 ||
         super::has_value(super::field(order,"confirmedAt")) ||
         super::flag(order,"pricesLocked"));
    if legacy_unmarked {
        result["checked"]=json!(false);
        result["legacy"]=json!(true);
    }
    Ok(result)
}

pub(super) fn multiply_shipment(
    base: Option<i64>, recipient: &Value, shipment: &Value, order: &Value
) -> Option<i64> {
    let result=resolve_shipment(recipient,shipment,order).ok()?;
    let amount=base?;
    amount.checked_mul(if result["checked"].as_bool()? {2} else {1})
        .filter(|n| (0..=super::MAX_SAFE).contains(n))
}

/// Convert the APPLIED amount typed by staff into the undoubled charge to
/// persist. The checked-state decision and the 2x constraint live in Rust,
/// exactly as they do for shipment previews and totals.
pub(super) fn normalize_applied_input(payload: &Value) -> Result<Value, String> {
    let recipient = super::req(payload, "recipient")?;
    let shipment = super::req(payload, "shipment")?;
    let order = super::req(payload, "order")?;
    let checked = resolve_shipment(recipient, shipment, order)?["checked"]
        .as_bool().ok_or("도서·산간 운임 적용 상태를 확인하세요.")?;
    let input = super::req(payload, "appliedCharge")?;
    if input.is_null() {
        return Ok(json!({"baseCharge": Value::Null}));
    }
    let amount = super::safe(input).ok_or("운임은 0 이상의 안전한 정수여야 합니다.")?;
    if checked && amount % 2 != 0 {
        return Err("도서/산간 적용 운임은 기준 운임의 2배입니다. 짝수 금액으로 입력하세요.".into());
    }
    Ok(json!({"baseCharge": if checked { amount / 2 } else { amount }}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(address: &str) -> Value {
        detect(&json!({"address": address})).unwrap()
    }

    #[test]
    fn do_not_flag_entire_gunsan_city() {
        assert!(!check("전북특별자치도 군산시 소룡동 공단대로 100")["suggestedChecked"].as_bool().unwrap());
        let island = check("전북특별자치도 군산시 옥도면 어청도리 12");
        assert_eq!(island["status"], "auto");
        assert_eq!(island["matchedPath"], "군산시 옥도면");
    }

    #[test]
    fn bridged_gunsan_islands_are_review_not_automatic_charge() {
        for name in ["신시도리", "무녀도리", "선유도리", "장자도리", "야미도리"] {
            let address = format!("전라북도 군산시 옥도면 {name} 123");
            let result = check(&address);
            assert_eq!(result["status"], "review", "{address}");
            assert_eq!(result["suggestedChecked"], false, "{address}");
        }
    }

    #[test]
    fn all_jeju_is_suggested_even_without_a_road_level_match() {
        for addr in [
            "제주특별자치도 제주시 아라일동 300",
            "제주특별자치도 서귀포시 대정읍 에듀시티로 23",
            "[63644] 제주특별자치도 서귀포시 대정읍 에듀시티로 23",
            "제주도 서귀포시",
            "서귀포시 대정읍",
        ] {
            assert_eq!(check(addr)["suggestedChecked"], true, "{addr}");
        }
    }

    #[test]
    fn partial_islands_are_not_generalized_to_entire_districts() {
        assert_eq!(check("전남 신안군 압해읍 가란리")["status"], "auto");
        assert_eq!(check("전남 신안군 압해읍 신장리")["status"], "review");
        assert_eq!(check("전남 신안군 지도읍 선도리")["status"], "auto");
        assert_eq!(check("전남 신안군 지도읍 읍내리")["status"], "review");
        assert_eq!(check("전남 여수시 화정면 백야리")["status"], "normal");
        assert_eq!(check("전남 여수시 화정면 개도리")["status"], "auto");
        assert_eq!(check("충남 보령시 오천면 원산도리")["status"], "review");
        assert_eq!(check("충남 보령시 오천면 녹도리")["status"], "auto");
    }

    #[test]
    fn resolved_ui_decision_contains_address_auto_check_and_detection_evidence(){
        let address="제주특별자치도 제주시 연동 1";
        let result=resolve_recipient(&json!({"address":address})).unwrap();
        assert_eq!(result["address"],address);
        assert_eq!(result["status"],"auto");
        assert_eq!(result["checked"],true);
        assert_eq!(result["manual"],false);
        assert!(result["matchedPath"].is_string());
        let overridden=resolve_recipient(&json!({
            "address":address,"remoteDelivery":{
                "address":address,"checked":false,"manual":true
            }
        })).unwrap();
        assert_eq!(overridden["suggestedChecked"],true);
        assert_eq!(overridden["checked"],false);
        assert_eq!(overridden["manual"],true);
        assert_eq!(multiply(Some(3000),&json!({"address":address})),Some(6000));
    }
    #[test]
    fn auto_check_can_be_manually_overridden_and_address_change_resets_it() {
        let address = "제주특별자치도 서귀포시 대정읍 에듀시티로 23";
        let mut recipient = json!({"address":address});
        assert_eq!(resolve_recipient(&recipient).unwrap()["checked"],true);
        assert_eq!(multiply(Some(3000),&recipient),Some(6000));
        recipient["remoteDelivery"]=json!({
            "address":address,"checked":false,"manual":true
        });
        assert_eq!(resolve_recipient(&recipient).unwrap()["checked"],false);
        assert_eq!(multiply(Some(3000),&recipient),Some(3000));
        recipient["remoteDelivery"]["checked"]=json!(true);
        assert_eq!(multiply(Some(3000),&recipient),Some(6000));
        recipient["address"]=json!("서울특별시 강남구 테헤란로 1");
        assert_eq!(resolve_recipient(&recipient).unwrap()["checked"],false);
        assert_eq!(multiply(Some(3000),&recipient),Some(3000));
    }

    #[test]
    fn previously_billed_legacy_area_can_preserve_its_original_freight() {
        let addr="제주특별자치도 제주시 가상로 1";
        let recipient=json!({"address":addr,
            "remoteDelivery":{"address":addr,"checked":false,"manual":false,"legacy":true}});
        let result=resolve_recipient(&recipient).unwrap();
        assert_eq!(result["status"],"auto");
        assert_eq!(result["checked"],false);
        assert_eq!(result["manual"],false);
        assert_eq!(multiply(Some(3000),&recipient),Some(3000));
    }

    #[test]
    fn unreconciled_legacy_paid_shipments_keep_their_original_amounts() {
        let row=json!({"address":"제주특별자치도 서귀포시 대정읍 테스트로 3"});
        let ordinary=json!({"freightCode":"010"});
        let paid=json!({"step":2,"transferred":false});
        assert_eq!(resolve_shipment(&row,&paid,&ordinary).unwrap()["checked"],false);
        assert_eq!(multiply_shipment(Some(3000),&row,&paid,&ordinary),Some(3000));
        let drafted=json!({"step":0,"transferred":false});
        assert_eq!(multiply_shipment(Some(3000),&row,&drafted,&ordinary),Some(6000));
        let checked=json!({"address":"제주특별자치도 서귀포시 대정읍 테스트로 3",
            "remoteDelivery":{"address":"제주특별자치도 서귀포시 대정읍 테스트로 3",
                "checked":true,"manual":false}});
        assert_eq!(multiply_shipment(Some(3000),&checked,&paid,&ordinary),Some(6000));
    }

    #[test]
    fn multiplier_is_applied_once_to_base_and_preserves_null_overflow() {
        let remote=json!({"address":"제주특별자치도 제주시 아라일동 3"});
        assert_eq!(multiply(Some(2500),&remote),Some(5000));
        assert_eq!(multiply(None,&remote),None);
        assert_eq!(multiply(Some(super::super::MAX_SAFE),&remote),None);
        let ordinary=json!({"address":"전북특별자치도 군산시 소룡동 3"});
        assert_eq!(multiply(Some(2500),&ordinary),Some(2500));
        let review=json!({"address":"군산시 옥도면 선유도리 3"});
        assert_eq!(multiply(Some(2500),&review),Some(2500));
    }

    #[test]
    fn no_substring_match_for_building_names_or_other_provinces() {
        assert_eq!(check("경기도 군포시 옥도면타워 24")["status"], "unknown");
        assert_eq!(check("서울특별시 서초구 제주특별자치도빌딩 7")["status"], "unknown");
        assert_eq!(check("부산광역시 강서구 대항동")["status"], "review");
        assert_eq!(check("서울특별시 서초구 제주특별자치도")["status"], "unknown");
    }
}
