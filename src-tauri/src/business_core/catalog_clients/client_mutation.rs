//! Shared native customer mutations. Both Tauri's business dispatcher and
//! the isolated egui client master call THESE functions. No UI-side rules.
use serde_json::{json,Value};

fn field<'a>(v:&'a Value,key:&str)->&'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn present(v:&Value,key:&str)->bool {
    !field(v,key).trim().is_empty()
}
pub fn validate_client(state:&Value,client:&Value)->Result<(),String> {
    if !present(client,"name") {
        return Err("거래처명을 입력하세요.".into());
    }
    if !present(client,"address") {
        return Err("거래처 주소를 입력하세요.".into());
    }
    if !present(client,"phone") && !present(client,"mobile") {
        return Err("거래처 전화번호 또는 휴대폰 번호를 입력하세요.".into());
    }
    let account_id=field(client,"preferredAccountId");
    if !account_id.is_empty()
        && !state.pointer("/settings/accounts")
            .and_then(Value::as_array).into_iter().flatten()
            .any(|a|field(a,"id")==account_id)
    {
        return Err("설정에 등록된 입금 계좌를 선택하세요.".into());
    }
    Ok(())
}
pub fn upsert(
    state:&Value,entity:&Value,collection:&str
)->Result<Value,String>{
    let id=field(entity,"id");
    if id.is_empty() {
        return Err(format!("{collection} 항목 ID가 없습니다."));
    }
    let mut next=state.clone();
    let rows=next.get_mut(collection).and_then(Value::as_array_mut)
        .ok_or_else(||format!("{collection} 목록이 없습니다."))?;
    if let Some(index)=rows.iter().position(|row|field(row,"id")==id) {
        rows[index]=entity.clone();
    } else {
        rows.push(entity.clone());
    }
    Ok(json!({"state":next}))
}
pub fn archive_client(state:&Value,id:&str,at:&str)->Result<Value,String>{
    if id.is_empty() || at.is_empty() {
        return Err("거래처 삭제 요청 정보가 없습니다.".into());
    }
    let mut next=state.clone();
    let clients=next.get_mut("clients").and_then(Value::as_array_mut)
        .ok_or("거래처 목록이 없습니다.")?;
    let client=clients.iter_mut().find(|client|field(client,"id")==id)
        .ok_or("삭제할 거래처를 찾지 못했습니다.")?;
    client["deletedAt"]=json!(at);
    Ok(json!({"state":next}))
}
pub fn apply_client_workbook(state: &Value, preview: &Value) -> Result<Value, String> {
    let revision = state.get("revision").and_then(Value::as_i64).unwrap_or(0);
    if revision != preview.get("revision").and_then(Value::as_i64).unwrap_or(-1) {
        return Err("검토 후 거래처 데이터가 변경되었습니다. Excel을 다시 검증하세요.".into());
    }
    if preview["rows"].as_array().into_iter().flatten().any(|row|
        ["ERROR", "HOLD"].contains(&field(row, "status")))
    {
        return Err("중복 또는 오류 항목이 남아 있어 Excel 변경사항을 저장할 수 없습니다.".into());
    }
    let mut next = state.clone();
    let mut created = 0_i64;
    let mut updated = 0_i64;
    for row in preview["rows"].as_array().into_iter().flatten() {
        let status = field(row, "status");
        if status != "ADD" && status != "UPDATE" { continue; }
        let Some(client) = row.get("client").filter(|c| !c.is_null()) else { continue };
        let canonical=bando_data_access::client_names::normalized(client);
        validate_client(&next, &canonical)?;
        let clients = next.get_mut("clients").and_then(Value::as_array_mut)
            .ok_or("거래처 목록이 없습니다.")?;
        let id = field(&canonical, "id");
        if status == "ADD" {
            if clients.iter().any(|c| field(c, "id") == id) {
                return Err("거래처 ID 충돌입니다.".into());
            }
            clients.push(canonical.clone());
            created += 1;
        } else {
            let index = clients.iter().position(|c| field(c, "id") == id
                && !present(c, "deletedAt"))
                .ok_or("기존 거래처 정보를 확인할 수 없습니다.")?;
            clients[index] = canonical;
            updated += 1;
        }
    }
    Ok(json!({"next":next, "created":created, "updated":updated}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_and_soft_archive_match_existing_business_contract() {
        let state=json!({"clients":[],"settings":{"accounts":[]}});
        let client=json!({"id":"c","name":"경기악기사","address":"경기도",
            "phone":"031-100-1000","preferredAccountId":null});
        validate_client(&state,&client).unwrap();
        let next=upsert(&state,&client,"clients").unwrap();
        assert_eq!(next["state"]["clients"][0]["name"],"경기악기사");
        let after=archive_client(&next["state"],"c","2026-11-20").unwrap();
        assert_eq!(after["state"]["clients"][0]["deletedAt"],"2026-11-20");
    }
}
