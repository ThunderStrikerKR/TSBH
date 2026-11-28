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
        if editable(){
            // The egui host can run without the Tauri initializer. Retire
            // legacy sender overrides the first time an authorized client
            // master is opened, using the identical backed-up v2 migration.
            // Read-only previews never open an operational DB for writes.
            let path=crate::business_state::authorized_client_write_path()?;
            let base=path.parent().ok_or("거래처 DB 폴더를 확인할 수 없습니다.")?;
            let mut conn=crate::business_state::open_business_connection(&path,OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e|e.to_string())?;
            conn.busy_timeout(Duration::from_secs(5))
                .map_err(|e|e.to_string())?;
            bando_data_access::client_names::migrate_v2(&mut conn,base)?;
        }
        // Read directly from the same relational business DB as order pricing.
        let state=crate::business_state::load_catalog()?;
        return Ok(json!({
            "revision":state.get("revision").and_then(Value::as_u64)
                .ok_or("거래처 revision을 확인할 수 없습니다.")?,
            "clients":json!(state.get("clients").and_then(Value::as_array)
                .into_iter().flatten()
                .map(bando_data_access::client_names::normalized)
                .collect::<Vec<_>>()),
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
pub fn mutate(expected:Value,action:&str,client:Value)->Result<Value,String>{
    if !editable(){
        return Err("거래처 저장 권한이 없거나 운영 DB가 연결되지 않았습니다.".into());
    }
    let path=crate::business_state::authorized_client_write_path()?;
    let mut conn=crate::business_state::open_business_connection(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_|"업무 거래처 DB를 쓰기 모드로 열 수 없습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|e|e.to_string())?;

    let committed=bando_data_access::mutate_client_checked_v2(
        &mut conn,&expected,action,&client
    )?;
    drop(conn);
    // The shared writer already verified and committed this authoritative
    // snapshot. A later unrelated commit is not a failed customer save.
    // If the refresh itself fails, still report the successful commit.
    let projection=||json!({
        "revision":committed["revision"],
        "clients":committed["clients"].as_array().into_iter().flatten()
            .map(bando_data_access::client_names::normalized).collect::<Vec<_>>(),
        "products":committed["products"],"aliases":committed["aliases"],
        "settings":{"accounts":committed.pointer("/settings/accounts").cloned().unwrap_or_else(||json!([]))},
        "readOnly":!editable()
    });
    let id=client["id"].as_str().ok_or("거래처 ID가 없습니다.")?;
    let saved=|state:&Value|state["clients"].as_array().into_iter().flatten()
        .find(|row|row["id"].as_str()==Some(id))
        .map(bando_data_access::client_names::normalized);
    match load(){
        Ok(mut current)=>{
            if saved(&current)!=saved(&committed){
                current["saveNotice"]=json!("거래처는 저장됐지만 이후 해당 거래처가 다시 변경되었습니다. 최신 목록을 표시합니다.");
            }
            Ok(current)
        },
        Err(_)=>{
            let mut current=projection();
            current["saveNotice"]=json!("거래처는 저장됐지만 최신 목록을 불러오지 못했습니다. 저장 당시 목록을 표시합니다.");
            Ok(current)
        },
    }
}

pub fn archive_workbook()->Result<String,String>{
    if !editable(){return Err("거래처 Excel 권한이 없습니다.".into());}
    let state=crate::business_state::load_catalog()?;
    let bytes=crate::native_workbooks::export_clients(&state)?;
    crate::excel_service::archive_client_workbook(&state,&bytes)
        .map(|p|p.to_string_lossy().into_owned())
}
#[cfg(windows)]
pub fn preview_workbook(mut source:Value)->Result<Value,String>{
    if !editable(){return Err("거래처 Excel 권한이 없습니다.".into());}
    let count=source["input"].as_array().ok_or("거래처 Excel 행 누락")?.len();
    if count>20_000{return Err("거래처 최대 20,000행".into());}
    source["newIds"]=json!((0..count).map(|_|uuid::Uuid::new_v4().to_string()).collect::<Vec<_>>());
    let state=crate::business_state::load_catalog()?;
    bando_data_access::preview_client_workbook(&state,&source)
}
pub fn apply_workbook(reviewed:Value)->Result<Value,String>{
    if !editable(){return Err("거래처 Excel 저장 권한이 없습니다.".into());}
    let path=crate::business_state::authorized_client_write_path()?;
    let base=path.parent().ok_or("백업 경로 누락")?;
    let mut conn=crate::business_state::open_business_connection(&path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e|e.to_string())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON;").map_err(|e|e.to_string())?;
    let committed=bando_data_access::commit_client_workbook_v2(&mut conn,base,&reviewed)?;
    let revision=committed["revision"].as_u64().ok_or("거래처 저장 리비전 누락")?;
    drop(conn);
    let current=load()?;
    if current["revision"].as_u64()!=Some(revision){
        return Err("거래처 Excel은 저장됐지만 이후 데이터가 변경되었습니다. 최신 목록을 다시 확인하세요.".into());
    }
    Ok(current)
}
