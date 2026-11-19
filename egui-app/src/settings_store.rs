//! Settings for native egui, with strict storage ownership.
//!
//! - Explicit business DB: read snapshot through the existing read-only
//!   projection; writes are opt-in one-shot Tauri settings_patch commands.
//! - No business DB: isolated egui-preview.sqlite3 settings domain, with
//!   per-field CAS and a local account-reference guard.
//! No implicit operational database discovery and no shadow settings rules.
use crate::{business_state, client_master, preview_store::PreviewStore};
use rusqlite::{params,Connection,TransactionBehavior};
use serde_json::{json,Map,Value};
use std::{io::Write,process::{Command,Stdio},time::{Duration,SystemTime,UNIX_EPOCH}};

const SETTINGS_CAP:usize=512*1024;
const EDIT_FIELDS:[&str;13]=[
    "uiScale","operatorName","senderName","senderAddress","senderPostcode",
    "senderPhone","senderMobile","accounts","defaultAccountId",
    "brandAccounts","excelDirectory","factoryReplyDirectory",
    "orderImageInboxDirectory",
];
fn local_defaults()->Value {
    json!({
        "uiScale":100,"operatorName":"",
        "senderName":"반도스탠드","senderAddress":"",
        "senderPostcode":"","senderPhone":"","senderMobile":"",
        "accounts":[],"defaultAccountId":"",
        "brandAccounts":{"BANDOSTAND":"","BANDOMALL":""},
        "excelDirectory":"","factoryReplyDirectory":"",
        "orderImageInboxDirectory":"",
    })
}
fn using_business()->bool {
    std::env::var_os("BANDO_EGUI_BUSINESS_DB").is_some_and(|v|!v.is_empty())
}
pub fn writable_business()->bool{
    using_business() &&
    std::env::var("BANDO_EGUI_ENABLE_SETTINGS_WRITE").ok().as_deref()==Some("1") &&
    std::env::var_os("BANDO_EGUI_TAURI_HELPER").is_some_and(|v|!v.is_empty())
}
fn used_accounts(clients:&Value)->Vec<String>{
    let mut refs=Vec::new();
    for client in clients.as_array().into_iter().flatten(){
        if client.get("deletedAt").is_some_and(|deleted|!deleted.is_null()){continue;}
        if let Some(id)=client.get("preferredAccountId")
            .and_then(Value::as_str).filter(|id|!id.is_empty()){
            if !refs.iter().any(|existing|existing==id){refs.push(id.to_owned());}
        }
    }
    refs
}
fn connection()->Result<Connection,String>{
    let store=PreviewStore::initialize()?;
    let mut conn=Connection::open(store.path())
        .map_err(|_|"격리 설정 DB를 열지 못했습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    let defaults=local_defaults().to_string();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS egui_settings (
          id INTEGER PRIMARY KEY CHECK(id=1),
          revision INTEGER NOT NULL CHECK(revision>=0),
          payload TEXT NOT NULL CHECK(json_valid(payload))
         );"
    ).map_err(|e|format!("격리 설정 테이블을 열지 못했습니다: {e}"))?;
    conn.execute(
        "INSERT OR IGNORE INTO egui_settings(id,revision,payload) VALUES(1,0,?1)",
        params![defaults]
    ).map_err(|e|format!("격리 설정 초기화 실패: {e}"))?;
    Ok(conn)
}
fn read_local(conn:&Connection)->Result<(u64,Value),String>{
    let (revision,raw):(i64,String)=conn.query_row(
        "SELECT revision,payload FROM egui_settings WHERE id=1",
        [],|row|Ok((row.get(0)?,row.get(1)?))
    ).map_err(|_|"저장된 설정을 읽지 못했습니다.".to_owned())?;
    let revision=u64::try_from(revision).map_err(|_|"설정 revision 오류")?;
    let settings:Value=serde_json::from_str(&raw)
        .map_err(|_|"저장된 설정 JSON이 잘못되었습니다.".to_owned())?;
    if !settings.is_object(){return Err("설정은 JSON 객체여야 합니다.".into());}
    Ok((revision,settings))
}
pub fn load()->Result<Value,String>{
    if using_business(){
        let state=business_state::load()?;
        let revision=state.get("revision").and_then(Value::as_u64)
            .ok_or("운영 설정 revision 확인 실패")?;
        let settings=state.get("settings").filter(|value|value.is_object())
            .ok_or("운영 설정 데이터가 올바르지 않습니다.")?;
        return Ok(json!({
            "revision":revision,"settings":settings,
            "readOnly":!writable_business(),"mode":"business",
            "usedAccounts":used_accounts(&state["clients"])
        }));
    }
    let conn=connection()?;
    let (revision,settings)=read_local(&conn)?;
    let clients=client_master::load()?;
    Ok(json!({
        "revision":revision,"settings":settings,
        "readOnly":false,"mode":"isolated",
        "usedAccounts":used_accounts(&clients["clients"])
    }))
}
fn validate(changes:&Map<String,Value>,expected:&Map<String,Value>)->Result<(),String>{
    if changes.len()>EDIT_FIELDS.len() || changes.is_empty(){
        return Err("변경할 설정이 없거나 변경 수가 올바르지 않습니다.".into());
    }
    for (key,value) in changes{
        if !EDIT_FIELDS.contains(&key.as_str()) || !expected.contains_key(key){
            return Err(format!("변경할 수 없는 설정 항목입니다: {key}"));
        }
        if key=="uiScale"{
            if !matches!(value.as_u64(),Some(90|100|110|125)){
                return Err("UI 배율은 90/100/110/125%만 선택할 수 있습니다.".into());
            }
        }else if key=="accounts"{
            if !value.is_array(){return Err("입금 계좌는 목록이어야 합니다.".into());}
        }else if key=="brandAccounts"{
            if !value.is_object(){return Err("추천 계좌 설정 형식이 잘못되었습니다.".into());}
        }else if !value.is_string() && !value.is_null(){
            return Err(format!("설정 문자 필드 형식 오류: {key}"));
        }
    }
    Ok(())
}
fn validate_accounts(settings:&Value, used:&[String])->Result<(),String>{
    let accounts=settings.get("accounts").and_then(Value::as_array)
        .ok_or("입금 계좌 목록이 잘못되었습니다.")?;
    let mut ids=std::collections::HashSet::new();
    for account in accounts{
        let id=account.get("id").and_then(Value::as_str)
            .filter(|id|!id.is_empty()).ok_or("입금 계좌 ID가 없습니다.")?;
        if !ids.insert(id.to_owned()){return Err("입금 계좌 ID가 중복되었습니다.".into());}
        for name in ["bank","number","holder"]{
            if account.get(name).and_then(Value::as_str)
                .is_none_or(|value|value.trim().is_empty()){
                return Err("계좌의 은행명, 계좌번호, 예금주를 모두 입력하세요.".into());
            }
        }
    }
    let reference=|id:&str| -> Result<(),String>{
        if !id.is_empty() && !ids.contains(id) {
            return Err("추천 계좌가 입금 계좌 목록에 없습니다.".into());
        }
        Ok(())
    };
    reference(settings.get("defaultAccountId").and_then(Value::as_str).unwrap_or(""))?;
    for brand in ["BANDOSTAND","BANDOMALL"]{
        reference(settings.pointer(&format!("/brandAccounts/{brand}"))
            .and_then(Value::as_str).unwrap_or(""))?;
    }
    for id in used{reference(id)?;}
    Ok(())
}
fn patch_local(changes:Value,expected:Value)->Result<Value,String>{
    let changes=changes.as_object().ok_or("설정 변경 객체가 없습니다.")?;
    let expected=expected.as_object().ok_or("설정 기존값 객체가 없습니다.")?;
    validate(changes,expected)?;
    // The client master uses another SQLite connection; read references
    // BEFORE acquiring the IMMEDIATE settings write lock.
    let clients=client_master::load()?;
    let used=used_accounts(&clients["clients"]);
    let mut conn=connection()?;
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_|"설정 저장 트랜잭션 시작 실패")?;
    let (revision,mut settings)=read_local(&tx)?;
    let entries=settings.as_object_mut().ok_or("설정 구조 오류")?;
    for (key,value) in changes{
        let before=entries.get(key).unwrap_or(&Value::Null);
        let original=&expected[key];
        if before!=original && before!=value{
            return Err(format!("{key} 설정이 다른 화면에서 바뀌었습니다. 다시 불러오세요."));
        }
    }
    for (key,value) in changes{
        if value.is_null(){entries.remove(key);}
        else{entries.insert(key.clone(),value.clone());}
    }
    // Account IDs and client references are not a UI-only decision.
    validate_accounts(&settings,&used)?;
    let next=revision.checked_add(1).ok_or("설정 revision 한도 초과")?;
    let payload=serde_json::to_string(&settings).map_err(|e|e.to_string())?;
    if payload.len()>SETTINGS_CAP{
        return Err("설정 저장 크기 한도를 초과했습니다.".into());
    }
    let affected=tx.execute(
        "UPDATE egui_settings SET revision=?1,payload=?2 WHERE id=1 AND revision=?3",
        params![next as i64,payload,revision as i64],
    ).map_err(|e|format!("설정 저장 실패: {e}"))?;
    if affected!=1{return Err("설정 저장 중 다른 변경이 발생했습니다.".into());}
    tx.commit().map_err(|e|format!("설정 저장 확정 실패: {e}"))?;
    load()
}
fn patch_business(changes:Value,expected:Value)->Result<Value,String>{
    if !writable_business() {
        return Err("운영 설정은 읽기 전용입니다. 설정 변경 권한이 필요합니다.".into());
    }
    let changes_obj=changes.as_object().ok_or("설정 변경 객체 오류")?;
    let expected_obj=expected.as_object().ok_or("설정 기준값 객체 오류")?;
    validate(changes_obj,expected_obj)?;
    let helper=std::env::var_os("BANDO_EGUI_TAURI_HELPER")
        .ok_or("Tauri 설정 저장 도우미 경로가 없습니다.")?;
    if !std::path::Path::new(&helper).is_file() {
        return Err("Tauri 설정 저장 도우미 실행파일을 찾지 못했습니다.".into());
    }
    let payload=json!({"changes":changes,"expected":expected}).to_string();
    if payload.len()>SETTINGS_CAP{return Err("설정 변경 내용이 너무 큽니다.".into());}
    let mut child=Command::new(helper)
        .arg("--bando-egui-settings-patch")
        .stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn().map_err(|_|"Tauri 설정 저장 도우미를 실행할 수 없습니다.".to_owned())?;
    let mut stdin=child.stdin.take()
        .ok_or("Tauri 설정 저장 입력 채널을 열지 못했습니다.")?;
    stdin.write_all(payload.as_bytes())
        .map_err(|_|"Tauri 설정 저장 데이터를 전달하지 못했습니다.".to_owned())?;
    drop(stdin);
    let output=child.wait_with_output()
        .map_err(|_|"Tauri 설정 저장 결과를 받을 수 없습니다.".to_owned())?;
    if output.stdout.len()>SETTINGS_CAP{
        return Err("Tauri 설정 저장 결과 크기가 올바르지 않습니다.".into());
    }
    let reply:Value=serde_json::from_slice(&output.stdout)
        .map_err(|_|"Tauri 설정 저장 응답 형식이 잘못되었습니다.".to_owned())?;
    if !output.status.success() || reply["ok"]!=Value::Bool(true){
        return Err(reply.get("error").and_then(Value::as_str)
            .unwrap_or("운영 설정 저장에 실패했습니다.").to_owned());
    }
    load() // Reload the authoritative settings; never trust echoed UI state.
}
pub fn patch(changes:Value,expected:Value)->Result<Value,String>{
    if changes.as_object().is_some_and(Map::is_empty){return load();}
    if using_business(){patch_business(changes,expected)}
    else{patch_local(changes,expected)}
}
pub fn new_account_id()->String{
    let time=SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|time|time.as_nanos()).unwrap_or(0);
    format!("egui-account-{}-{time}",std::process::id())
}
