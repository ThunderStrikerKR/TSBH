//! Native egui client-management adapter to the single Tauri business DB.
//! Never creates an alternative customer master in egui-preview.sqlite3.
//! Client commands reuse the original native Rust rules and shared v2 writer;
//! the separate egui preview DB remains exclusively for unconfirmed sources.

use rusqlite::{Connection,OpenFlags};
use serde_json::{json,Value};
use std::time::Duration;

fn external_index()->bool{
    std::env::var_os("BANDO_EGUI_CLIENT_INDEX").is_some_and(|v|!v.is_empty())
}
fn external_business()->bool{
    std::env::var_os("BANDO_EGUI_BUSINESS_DB").is_some_and(|v|!v.is_empty())
}

/// An operating client master is writable only with explicit authorization.
/// An exported JSON index and an unbound egui preview are never writable.
pub fn editable()->bool{
    external_business() && !external_index() &&
    std::env::var("BANDO_EGUI_ENABLE_CLIENT_WRITE")
        .ok().as_deref()==Some("1") &&
    crate::business_state::authorized_client_write_path().is_ok()
}

pub fn load()->Result<Value,String>{
    if external_business(){
        // Read directly from the same relational business DB as order pricing.
        let state=crate::business_state::load_catalog()?;
        return Ok(json!({
            "revision":state.get("revision").and_then(Value::as_u64)
                .ok_or("거래처 revision을 확인할 수 없습니다.")?,
            "clients":state.get("clients").cloned().unwrap_or_else(||json!([])),
            "products":state.get("products").cloned().unwrap_or_else(||json!([])),
            "aliases":state.get("aliases").cloned().unwrap_or_else(||json!([])),
            "settings":{"accounts":state.pointer("/settings/accounts").cloned()
                .unwrap_or_else(||json!([]))},
            "readOnly":!editable(),
        }));
    }
    if external_index(){
        // Exported client data can supply names for search, never edits.
        let catalog=crate::client_directory::attach(json!({}))?;
        return Ok(json!({
            "revision":0,
            "clients":catalog.get("clients").cloned().unwrap_or_else(||json!([])),
            "products":[],"aliases":[],"settings":{"accounts":[]},
            "readOnly":true,
        }));
    }
    Err("거래처관리에는 Tauri 업무 DB 연결이 필요합니다. egui 전용 거래처 명단은 사용하지 않습니다.".into())
}

/// Update the canonical business master only. The common service re-reads
/// authoritative SQLite values and checks revision again inside the transaction.
pub fn mutate(expected:u64,action:&str,client:Value)->Result<Value,String>{
    if !editable(){
        return Err("거래처 저장 권한이 없거나 실제 업무 DB가 연결되지 않았습니다.".into());
    }
    let path=crate::business_state::authorized_client_write_path()?;
    let mut conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_|"업무 거래처 DB를 쓰기 모드로 열 수 없습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|e|e.to_string())?;

    let committed=bando_data_access::mutate_client_v2(
        &mut conn,expected,action,&client
    )?;
    let revision=committed["revision"].as_u64()
        .ok_or("거래처 저장 결과의 revision을 확인할 수 없습니다.")?;
    drop(conn);
    // Never present an optimistic form draft as committed customer data.
    let current=load()?;
    if current["revision"].as_u64()!=Some(revision){
        return Err("거래처는 저장됐지만 이후 다른 작업에서 자료가 수정됐습니다. 최신 상태를 다시 확인하세요.".into());
    }
    let id=client.get("id").and_then(Value::as_str)
        .ok_or("거래처 ID가 없습니다.")?;
    let saved=current["clients"].as_array().into_iter().flatten()
        .find(|row|row["id"].as_str()==Some(id))
        .ok_or("거래처 저장 후 실제 목록에서 대상을 찾지 못했습니다.")?;
    if action=="upsert_client" &&
        saved!=&bando_data_access::client_names::normalized(&client) {
        return Err("거래처는 저장됐지만 최신 데이터가 입력 내용과 일치하지 않습니다. 다시 확인하세요.".into());
    }
    if action=="archive_client" && saved["deletedAt"].is_null(){
        return Err("거래처 보관 후 변경 상태를 확인하지 못했습니다.".into());
    }
    Ok(current)
}
