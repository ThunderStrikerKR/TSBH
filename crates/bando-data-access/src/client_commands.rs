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
/// The final revision check and every SQL change occur in write_v2_snapshot.
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
    if version!=2{return Err("지원하지 않는 운영 DB 버전입니다.".into());}

    let before=crate::read_v2_snapshot(conn)?;
    let revision=before["revision"].as_u64().ok_or("운영 DB 리비전 오류")?;
    if revision!=expected_revision{
        return Err("다른 작업에서 거래처가 변경됐습니다. 새로고침 후 다시 저장하세요.".into());
    }

    let transition=if action=="upsert_client"{
        let canonical=crate::client_names::normalized(client);
        native_client_mutation::validate_client(&before,&canonical)?;
        native_client_mutation::upsert(&before,&canonical,"clients")?
    }else{
        let id=client.get("id").and_then(Value::as_str).unwrap_or("");
        let now=chrono::Utc::now().to_rfc3339();
        native_client_mutation::archive_client(&before,id,&now)?
    };
    let next=transition.get("state")
        .ok_or("Rust 거래처 변경 결과가 없습니다.")?;
    validate_limits(&next["clients"])?;
    // CAS is checked again INSIDE the SQLite transaction.
    // Never commit a UI-provided full snapshot or bypass the shared validation.
    crate::write_v2_snapshot(conn,next.clone(),expected_revision,true)
}
