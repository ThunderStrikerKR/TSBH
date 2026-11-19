//! One native settings-account rule for both the React Tauri command and
//! the isolated egui settings editor. UI and persistence do not duplicate it.
use serde_json::Value;

pub fn validate(state: &Value) -> Result<(), String> {
    let accounts = state["settings"]["accounts"].as_array()
        .ok_or("입금 계좌 목록 형식 오류")?;
    let mut ids = std::collections::HashSet::new();
    for account in accounts {
        let id = account["id"].as_str().ok_or("계좌 ID 오류")?;
        if id.is_empty() || !ids.insert(id) {
            return Err("입금 계좌 ID가 중복되거나 비어 있습니다.".into());
        }
        if ["bank", "number", "holder"].iter().any(|k|
            account[*k].as_str().unwrap_or("").trim().is_empty()
        ) {
            return Err("은행명, 계좌번호, 예금주를 모두 입력한 뒤 저장하세요.".into());
        }
    }
    let check_id = |id: &Value| -> Result<(), String> {
        if let Some(id) = id.as_str().filter(|x| !x.is_empty()) {
            if !ids.contains(id) {
                return Err("추천 계좌가 등록된 입금 계좌 목록에 없습니다.".into());
            }
        }
        Ok(())
    };
    check_id(&state["settings"]["defaultAccountId"])?;
    for brand in ["BANDOSTAND", "BANDOMALL"] {
        check_id(&state["settings"]["brandAccounts"][brand])?;
    }
    for client in state["clients"].as_array().ok_or("거래처 데이터 형식 오류")? {
        if !client["deletedAt"].is_null() { continue; }
        if let Some(id) = client["preferredAccountId"].as_str()
            .filter(|x| !x.is_empty())
        {
            if !ids.contains(id) {
                return Err(
                    "거래처에서 사용하는 입금 계좌는 삭제할 수 없습니다. 거래처의 주 사용 계좌부터 변경하세요.".into()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn an_active_client_prevents_deletion_of_its_account() {
        let value=json!({"settings":{"accounts":[],"brandAccounts":{}},
            "clients":[{"preferredAccountId":"client-bank","deletedAt":null}]});
        assert!(validate(&value).unwrap_err().contains("거래처에서 사용"));
    }
    #[test]
    fn archived_clients_do_not_reserve_an_account() {
        let value=json!({"settings":{"accounts":[],"brandAccounts":{}},
            "clients":[{"preferredAccountId":"client-bank","deletedAt":"2026-01-01"}]});
        assert!(validate(&value).is_ok());
    }
}
