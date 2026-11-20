//! egui-only client master. The production customer database is NEVER edited
//! through this store. Existing Rust catalog client validation owns mutations.
use rusqlite::{params,Connection,TransactionBehavior};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use crate::preview_store::PreviewStore;

#[path = "../../src-tauri/src/business_core/catalog_clients/client_mutation.rs"]
mod native_client_mutation;

const CLIENT_LIMIT: usize = 5000;
const PAYLOAD_LIMIT: usize = 2 * 1024 * 1024;

fn external_index() -> bool {
    std::env::var_os("BANDO_EGUI_CLIENT_INDEX").is_some_and(|v| !v.is_empty())
}
fn external_business() -> bool {
    std::env::var_os("BANDO_EGUI_BUSINESS_DB").is_some_and(|v| !v.is_empty())
}
pub fn editable() -> bool {
    !external_index() && !external_business()
}
fn connection() -> Result<Connection, String> {
    let store=PreviewStore::initialize()?;
    let path: PathBuf=store.path().to_owned();
    let conn=Connection::open(path)
        .map_err(|_|"격리 거래처 저장소를 열지 못했습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS egui_client_master (
           id INTEGER PRIMARY KEY CHECK(id=1),
           revision INTEGER NOT NULL CHECK(revision >= 0),
           payload TEXT NOT NULL CHECK(json_valid(payload))
         );
         INSERT OR IGNORE INTO egui_client_master(id,revision,payload) VALUES(1,0,'[]');"
    ).map_err(|e|format!("egui 거래처 저장소 초기화 실패: {e}"))?;
    Ok(conn)
}
fn validate(clients:&Value) -> Result<(),String> {
    let rows=clients.as_array().ok_or("거래처 데이터가 배열이 아닙니다.")?;
    if rows.len()>CLIENT_LIMIT || clients.to_string().len()>PAYLOAD_LIMIT {
        return Err("거래처 데이터가 허용 범위를 초과했습니다.".into());
    }
    let mut ids=std::collections::HashSet::new();
    for row in rows {
        let id=row.get("id").and_then(Value::as_str)
            .filter(|s|!s.trim().is_empty()).ok_or("거래처 ID 누락")?;
        if !ids.insert(id) {return Err("중복된 거래처 ID가 있습니다.".into());}
    }
    Ok(())
}
fn read_local(conn:&Connection) -> Result<(u64,Value),String> {
    let (rev,text):(i64,String)=conn.query_row(
        "SELECT revision,payload FROM egui_client_master WHERE id=1",
        [],|row|Ok((row.get(0)?,row.get(1)?))
    ).map_err(|_|"egui 거래처 목록을 읽지 못했습니다.".to_owned())?;
    let rev=u64::try_from(rev).map_err(|_|"거래처 revision이 잘못되었습니다.")?;
    let clients:Value=serde_json::from_str(&text).map_err(|_|"거래처 데이터 형식 오류")?;
    validate(&clients)?;
    Ok((rev,clients))
}

/// The current live master is an explicit read-only reference. Development
/// clients live ONLY in the independently identified egui preview database.
pub fn load() -> Result<Value,String> {
    if external_business() {
        // Client-management needs product references, not order history.
        let state=crate::business_state::load_catalog()?;
        return Ok(json!({
            "revision":state.get("revision").and_then(Value::as_u64).unwrap_or(0),
            "clients":state.get("clients").cloned().unwrap_or_else(||json!([])),
            "products":state.get("products").cloned().unwrap_or_else(||json!([])),
            "aliases":state.get("aliases").cloned().unwrap_or_else(||json!([])),
            "settings":{"accounts":state.pointer("/settings/accounts").cloned()
                .unwrap_or_else(||json!([]))},
            "readOnly":true,
        }));
    }
    if external_index() {
        let catalog=crate::client_directory::attach(json!({}))?;
        return Ok(json!({
            "revision":0,"clients":catalog.get("clients").cloned().unwrap_or_else(||json!([])),
            "products":[],"aliases":[],"settings":{"accounts":[]},"readOnly":true,
        }));
    }
    let conn=connection()?;
    let (revision,clients)=read_local(&conn)?;
    let catalog=PreviewStore::initialize()?.load_catalog()?;
    Ok(json!({
        "revision":revision,"clients":clients,
        "products":catalog.get("products").cloned().unwrap_or_else(||json!([])),
        "aliases":catalog.get("aliases").cloned().unwrap_or_else(||json!([])),
        "settings":{"accounts":crate::settings_store::load_local_accounts()?},
        "readOnly":false,
    }))
}

/// Optimistic revision + SQLite transaction; the unchanged original Tauri
/// Rust rule decides every upsert/archive transition, not an egui algorithm.
pub fn mutate(expected:u64,action:&str,client:Value)->Result<Value,String> {
    if !editable() {
        return Err("연결된 운영 거래처 명단은 읽기 전용입니다. 개발용 격리 저장소로 전환해야 합니다.".into());
    }
    if !matches!(action,"upsert_client"|"archive_client") {
        return Err("허용되지 않은 거래처 변경 명령입니다.".into());
    }
    let mut conn=connection()?;
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_|"거래처 저장 트랜잭션 시작 실패")?;
    let (revision,clients)=read_local(&tx)?;
    if revision!=expected {
        return Err("다른 작업에서 거래처가 변경됐습니다. 새로고침 후 다시 저장하세요.".into());
    }
    let state=json!({"revision":revision,"clients":clients,
        "settings":{"accounts":[]},"orders":[],"products":[]});
    let transition=if action=="upsert_client" {
        native_client_mutation::validate_client(&state,&client)?;
        native_client_mutation::upsert(&state,&client,"clients")?
    } else {
        let at=chrono::Utc::now().to_rfc3339();
        native_client_mutation::archive_client(
            &state,client.get("id").and_then(Value::as_str).unwrap_or(""),&at
        )?
    };
    let next=transition.get("state").ok_or("Rust 거래처 변경 결과가 없습니다.")?;
    let next_clients=next.get("clients").ok_or("거래처 목록이 없습니다.")?;
    validate(next_clients)?;
    let new_revision=revision.checked_add(1).ok_or("거래처 revision 한도 초과")?;
    tx.execute(
        "UPDATE egui_client_master SET revision=?1,payload=?2 WHERE id=1 AND revision=?3",
        params![new_revision as i64,next_clients.to_string(),revision as i64]
    ).map_err(|e|format!("거래처 저장 실패: {e}"))?;
    tx.commit().map_err(|e|format!("거래처 저장 확정 실패: {e}"))?;
    load()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_mutations_are_restricted_to_two_native_rules() {
        assert!(!matches!("delete_all","upsert_client"|"archive_client"));
    }
}
