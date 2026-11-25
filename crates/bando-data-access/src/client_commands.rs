//! Client command service shared by desktop hosts.
//! The original Rust catalog client validator/mutator owns all domain decisions.
//! This service reads the authoritative v2 snapshot and delegates the CAS,
//! relation synchronization, indexes and archive to the shared writer.

use rusqlite::Connection;
use serde_json::Value;

#[path = "../../../src-tauri/src/business_core/catalog_clients/client_mutation.rs"]
mod native_client_mutation;

const CLIENT_LIMIT: usize = 5_000;
const PAYLOAD_LIMIT: usize = 2 * 1024 * 1024;

fn validate_limits(clients: &Value) -> Result<(), String> {
    let rows=clients.as_array().ok_or("거래처 데이터가 배열이 아닙니다.")?;
    if rows.len()>CLIENT_LIMIT || clients.to_string().len()>PAYLOAD_LIMIT {
        return Err("거래처 데이터가 허용 범위를 초과했습니다.".into());
    }
    let mut ids=std::collections::HashSet::new();
    for row in rows {
        let id=row.get("id").and_then(Value::as_str)
            .filter(|s|!s.trim().is_empty()).ok_or("거래처 ID 누락")?;
        if !ids.insert(id){return Err("중복된 거래처 ID가 있습니다.".into());}
    }
    Ok(())
}

/// Execute only the original customer upsert/soft-archive commands.
/// The caller must hold an explicitly authorized existing v2 DB connection.
/// The final revision check and every SQL change occur in update_v2_snapshot.
pub fn mutate_client_v2(
    conn: &mut Connection,
    expected_revision: u64,
    action: &str,
    client: &Value,
) -> Result<Value, String> {
    if !matches!(action,"upsert_client"|"archive_client"){
        return Err("허용되지 않은 거래처 변경 명령입니다.".into());
    }
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(|_|"운영 DB 스키마를 확인할 수 없습니다.".to_owned())?;
    if !crate::supported_relational_version(version){return Err("지원하지 않는 운영 DB 버전입니다.".into());}

    let revision=crate::read_envelope(conn)?["revision"].as_u64()
        .ok_or("운영 DB 리비전 오류")?;
    if revision!=expected_revision{
        return Err("다른 작업에서 거래처가 변경됐습니다. 새로고침 후 다시 저장하세요.".into());
    }
    // The same transactional before-image is used for native mutation,
    // validation and persistence. No second pre-write ledger hydration.
    crate::update_v2_snapshot(conn,expected_revision,true,|before|{
        let mut transition=if action=="upsert_client"{
            let canonical=crate::client_names::normalized(client);
            native_client_mutation::validate_client(before,&canonical)?;
            native_client_mutation::upsert(before,&canonical,"clients")?
        }else{
            let id=client.get("id").and_then(Value::as_str).unwrap_or("");
            let now=chrono::Utc::now().to_rfc3339();
            native_client_mutation::archive_client(before,id,&now)?
        };
        let next=transition.as_object_mut().and_then(|obj|obj.remove("state"))
            .ok_or("Rust 거래처 변경 결과가 없습니다.")?;
        validate_limits(&next["clients"])?;
        Ok(next)
    })
}

#[path = "../../../src-tauri/src/business_core/catalog_clients/client_preview.rs"]
mod native_client_preview;

/// Reuse the existing row validator. Keep the original source and assigned
/// IDs so commit can reproduce the exact preview instead of trusting UI rows.
pub fn preview_client_workbook(state:&Value,source:&Value)->Result<Value,String>{
    let rows=source["input"].as_array().ok_or("거래처 Excel 행 누락")?;
    let ids=source["newIds"].as_array().ok_or("거래처 신규 ID 누락")?;
    if rows.len()>20_000||ids.len()!=rows.len()||source.to_string().len()>40_000_000{
        return Err("거래처 Excel 검토 범위를 초과했습니다.".into());
    }
    let mut payload=source.clone();
    payload["state"]=state.clone();
    let mut review=native_client_preview::preview(&payload)?;
    review["_source"]=source.clone();
    Ok(review)
}

/// Backup before the authoritative CAS transaction. Recheck the reviewed
/// projection inside the transaction and apply only the shared reducer.
pub fn commit_client_workbook_v2(conn:&mut Connection,base:&std::path::Path,
    reviewed:&Value)->Result<Value,String>{
    let revision=reviewed["revision"].as_u64().ok_or("거래처 Excel 리비전 누락")?;
    let source=reviewed.get("_source").ok_or("거래처 Excel 원본 검토 누락")?;
    let before=crate::read_v2_snapshot(conn)?;
    let fresh=preview_client_workbook(&before,source)?;
    if &fresh!=reviewed{return Err("검토 내용이 달라졌습니다. Excel을 다시 검증하세요.".into());}
    let checked=native_client_mutation::apply_client_workbook(&before,&fresh)?;
    if checked["created"].as_u64().unwrap_or(0)+checked["updated"].as_u64().unwrap_or(0)==0{
        return Err("저장할 거래처 변경사항이 없습니다.".into());
    }
    validate_limits(&checked["next"]["clients"])?;
    drop(before);
    drop(checked);
    crate::backup_service::manual_backup(conn,base)?;
    crate::update_v2_snapshot(conn,revision,true,|current|{
        let fresh=preview_client_workbook(current,source)?;
        if &fresh!=reviewed{return Err("검토 후 거래처가 변경되었습니다.".into());}
        let mut result=native_client_mutation::apply_client_workbook(current,&fresh)?;
        let next=result.as_object_mut().and_then(|o|o.remove("next"))
            .ok_or("거래처 Excel 저장 결과 누락")?;
        validate_limits(&next["clients"])?;
        Ok(next)
    })
}
