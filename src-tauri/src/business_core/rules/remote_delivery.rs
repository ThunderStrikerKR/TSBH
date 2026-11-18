//! Read-only, reviewed, conservative remote/island locality classifier.
//! Data was compiled from multiple source lists on 2026-11-19. No tariff is
//! calculated here: the optional 2x application rule belongs to the freight
//! engine, and staff must remain free to override the automatic suggestion.
use serde_json::{json, Value};

static ZONES_JSON: &str = include_str!("../../../resources/remote-delivery-zones.json");

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
    let limit = address.len().saturating_sub(parts.len()).min(2);
    (0..=limit).any(|start|
        parts.iter().enumerate().all(|(i, part)| address[start + i] == *part)
    )
}

pub(super) fn detect(payload: &Value) -> Result<Value, String> {
    let address = payload.get("address").and_then(Value::as_str)
        .ok_or("주소 문자열이 필요합니다.")?;
    let dataset: Value = serde_json::from_str(ZONES_JSON)
        .map_err(|e| format!("도서산간 지역 데이터 오류: {e}"))?;
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
    fn no_substring_match_for_building_names_or_other_provinces() {
        assert_eq!(check("경기도 군포시 옥도면타워 24")["status"], "unknown");
        assert_eq!(check("서울특별시 서초구 제주특별자치도빌딩 7")["status"], "unknown");
        assert_eq!(check("부산광역시 강서구 대항동")["status"], "review");
    }
}
