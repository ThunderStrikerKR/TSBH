//! Canonical business snapshot validation extracted from the original Tauri writer.
//! Keep business rules and error messages identical for both desktop hosts.

use serde_json::Value;

pub fn valid_state(s: &Value) -> Result<(), String> {
    for field in ["products", "orders", "clients", "aliases", "queue", "exports"] {
        if !s.get(field).is_some_and(Value::is_array) {
            return Err(format!("저장 데이터 형식 오류: {field}"));
        }
    }
    if !s.get("settings").is_some_and(Value::is_object) { return Err("설정 데이터 형식 오류".into()); }
    if s["products"].as_array().is_some_and(|p| p.len() > 50000) { return Err("상품 한도 초과".into()); }
    if s["orders"].as_array().is_some_and(|p| p.len() > 100000) { return Err("주문 한도 초과".into()); }
    validate_message_templates(s)?;
    validate_provisional_product_notices(s)?;
    Ok(())
}

fn brand_terms(brand: &Value) -> Vec<String> {
    let fields = ["name", "korean"];
    let fixed = fields.iter().filter_map(|key| brand[*key].as_str());
    let aliases = brand["aliases"].as_array().into_iter().flatten().filter_map(Value::as_str);
    fixed.chain(aliases)
        .map(|term| term.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase())
        .filter(|term| !term.is_empty()).collect()
}
pub fn validate_brands(before: &Value, next: &Value) -> Result<(), String> {
    let Some(brands) = next["brands"].as_array() else { return Ok(()); };
    for (position, brand) in brands.iter().enumerate() {
        let id = brand["id"].as_str().unwrap_or("");
        if before["brands"].as_array().is_some_and(|previous| {
            previous.iter().any(|old| old["id"].as_str() == Some(id) && old == brand)
        }) { continue; }
        if id.trim().is_empty() || brand["name"].as_str().unwrap_or("").trim().is_empty() {
            return Err("브랜드 공식명과 식별자를 확인하세요.".into());
        }
        let evidence = brand["evidenceUrl"].as_str().unwrap_or("");
        let valid_evidence = url::Url::parse(evidence).is_ok_and(|link|
            link.scheme() == "https" && link.host_str().is_some() &&
            link.username().is_empty() && link.password().is_none());
        if !valid_evidence {
            return Err("브랜드 확인 근거는 HTTPS URL이어야 합니다.".into());
        }
        let terms = brand_terms(brand);
        for (other_position, other) in brands.iter().enumerate() {
            if position == other_position { continue; }
            if other["id"].as_str() == Some(id) {
                return Err("브랜드 식별자가 중복되었습니다.".into());
            }
            let other_terms = brand_terms(other);
            if terms.iter().any(|term| other_terms.contains(term)) {
                return Err("다른 브랜드의 공식명, 한글명 또는 검색 별칭과 중복됩니다.".into());
            }
        }
    }
    Ok(())
}

// Exact equality to the authoritative before-image establishes that every
// existing client would take the unchanged branch of validate_clients.
// Still enforce the same list, account-list and client-ID preconditions.
pub(crate) fn validate_unchanged_clients(state:&Value)->Result<(),String>{
    let clients=state["clients"].as_array().ok_or("거래처 데이터 형식 오류")?;
    state["settings"]["accounts"].as_array().ok_or("입금 계좌 형식 오류")?;
    for client in clients{
        client["id"].as_str().ok_or("거래처 ID 누락")?;
    }
    Ok(())
}

// Validate only newly created or edited clients. Existing historical test rows are
// not silently rewritten, but cannot be re-saved with missing sender information.
pub fn validate_clients(before: &Value, next: &Value) -> Result<(), String> {
    let clients = next["clients"].as_array().ok_or("거래처 데이터 형식 오류")?;
    let old = before["clients"].as_array().ok_or("기존 거래처 데이터 형식 오류")?;
    let accounts = next["settings"]["accounts"].as_array().ok_or("입금 계좌 형식 오류")?;
    for client in clients {
        let id = client["id"].as_str().ok_or("거래처 ID 누락")?;
        if old.iter().any(|c| c["id"].as_str() == Some(id) && c == client) { continue; }
        if !client["deletedAt"].is_null() { continue; }
        if client["name"].as_str().unwrap_or("").trim().is_empty() {
            return Err("거래처명을 입력하세요.".into());
        }
        if !["wholesale", "semi", "retail", "special"].contains(&client["tier"].as_str().unwrap_or("")) {
            return Err("거래처 가격 등급을 선택하세요.".into());
        }
        if client["address"].as_str().unwrap_or("").trim().is_empty() {
            return Err("거래처 주소를 입력하세요.".into());
        }
        if ["phone", "mobile"].iter().all(|k| client[*k].as_str().unwrap_or("").trim().is_empty()) {
            return Err("거래처 전화번호 또는 휴대폰 번호를 입력하세요.".into());
        }
        if let Some(preferred) = client["preferredAccountId"].as_str().filter(|s| !s.is_empty()) {
            if !accounts.iter().any(|a| a["id"].as_str() == Some(preferred)) {
                return Err("거래처 주 사용 계좌가 등록된 입금 계좌 목록에 없습니다.".into());
            }
        }
    }
    Ok(())
}

pub fn validate_provisional_product_notices(state: &Value) -> Result<(), String> {
    let Some(config) = state["settings"].get("provisionalProductNotices") else {
        return Ok(());
    };
    let entries = config.as_object().ok_or("잠정 상품 안내문 설정 형식 오류")?;
    if entries.len() > 5000 {
        return Err("잠정 상품 안내문 등록 한도를 초과했습니다.".into());
    }
    let products = state["products"].as_array().ok_or("상품 목록이 누락됐습니다.")?;
    for (product_id, notice) in entries {
        if !products.iter().any(|p| p["id"].as_str() == Some(product_id.as_str())) {
            return Err("등록된 상품에만 잠정 상품 안내문을 연결할 수 있습니다.".into());
        }
        let message = notice.as_str().ok_or("잠정 상품 안내문은 문자여야 합니다.")?;
        if message.trim().is_empty() || message.chars().count() > 2000 {
            return Err("잠정 상품 안내문은 1자 이상 2000자 이하로 입력하세요.".into());
        }
    }
    Ok(())
}

pub fn validate_message_templates(state: &Value) -> Result<(), String> {
    let Some(config) = state["settings"].get("automaticMessageTemplates") else { return Ok(()); };
    let entries = config.as_object().ok_or("자동 메시지 설정은 객체여야 합니다.")?;
    if entries.len() > 20 { return Err("자동 메시지 유형 개수가 올바르지 않습니다.".into()); }
    for (key, value) in entries {
        let (vars, required): (&[&str], &[&str]) = match key.as_str() {
            "recipientName" | "requestClosing" | "paymentGreeting"
              | "optionsIntro" | "optionsClosing" => (&[], &[]),
            "addressMissing" | "addressDetail" | "contactMissing" => (&["수취인"], &["수취인"]),
            "quantityMissing" | "modelMissing" | "modelConfirm" | "modelRepresentative" => (&["상품명"], &["상품명"]),
            "requestRecipient" => (&["번호", "수취인", "확인항목"], &["수취인", "확인항목"]),
            "colorDefault" => (&["색상", "상품명"], &["색상"]),
            "quantityOne" => (&["수취인", "상품명"], &["수취인", "상품명"]),
            "paymentItem" => (&["상품명", "수량", "금액"], &["상품명", "수량", "금액"]),
            "paymentFreight" => (&["운임"], &["운임"]),
            "paymentVat" => (&["부가세"], &["부가세"]),
            "paymentTotal" => (&["합계"], &["합계"]),
            "paymentAccount" => (&["은행명", "계좌번호", "예금주"], &["은행명", "계좌번호", "예금주"]),
            _ => return Err("등록되지 않은 자동 메시지 유형입니다.".into()),
        };
        let text = value.as_str().ok_or("자동 메시지는 문자열이어야 합니다.")?;
        if text.trim().is_empty() || text.chars().count() > 2000 {
            return Err("자동 메시지 길이를 확인하세요 (1~2000자).".into());
        }
        for required_var in required {
            if !text.contains(&format!("{{{required_var}}}")) {
                return Err(format!("{key}: 필수 변수를 삭제할 수 없습니다."));
            }
        }
        let mut remaining = text;
        while let Some(start) = remaining.find('{') {
            if remaining[..start].contains('}') {
                return Err("자동 메시지의 변수 괄호를 확인하세요.".into());
            }
            let tail = &remaining[start+1..];
            let Some(end) = tail.find('}') else {
                return Err("자동 메시지의 변수 괄호를 확인하세요.".into());
            };
            let token = &tail[..end];
            if !vars.contains(&token) || token.contains('{') {
                return Err(format!("{key}: 허용되지 않은 메시지 변수입니다."));
            }
            remaining = &tail[end+1..];
        }
        if remaining.contains('}') { return Err("자동 메시지의 변수 괄호를 확인하세요.".into()); }
    }
    Ok(())
}


#[cfg(test)]
mod scoped_client_validation_tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn unchanged_client_validation_matches_full(){
        for state in [
            json!({"clients":[{"id":"old"}],"settings":{"accounts":[]}}),
            json!({"clients":[{"id":"same"},{"id":"same"}],"settings":{"accounts":[]}}),
            json!({"clients":[{"name":"missing id"}],"settings":{"accounts":[]}}),
            json!({"clients":[],"settings":{"accounts":null}}),
            json!({"clients":null,"settings":{"accounts":[]}})
        ]{
            assert_eq!(validate_unchanged_clients(&state),validate_clients(&state,&state));
        }
    }
}
