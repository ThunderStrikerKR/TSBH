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

/// Compare against the exact normalized record shown when editing began.
/// Null means creation, not permission to overwrite an existing ID.
fn checked_client_transition(before:&Value,expected:&Value,action:&str,client:&Value)
    ->Result<Option<Value>,String>{
    let id=client.get("id").and_then(Value::as_str)
        .filter(|id|!id.is_empty()).ok_or("거래처 ID가 없습니다.")?;
    let actual=before["clients"].as_array().ok_or("거래처 목록 누락")?.iter()
        .find(|row|row["id"].as_str()==Some(id))
        .map(crate::client_names::normalized).unwrap_or(Value::Null);
    let expected=crate::client_names::normalized(expected);
    let canonical=crate::client_names::normalized(client);
    if actual!=expected{
        return Err("편집 중인 거래처의 정보가 변경되었습니다. 입력 내용은 유지됩니다. 최신 거래처 정보를 확인하세요.".into());
    }
    let mut transition=match action{
        "upsert_client"=>{
            native_client_mutation::validate_client(before,&canonical)?;
            if actual==canonical{return Ok(None);}
            native_client_mutation::upsert(before,&canonical,"clients")?
        },
        "archive_client"=>{
            if actual.is_null(){return Err("삭제할 거래처를 찾지 못했습니다.".into());}
            if !actual["deletedAt"].is_null(){return Ok(None);}
            native_client_mutation::archive_client(before,id,&chrono::Utc::now().to_rfc3339())?
        },
        _=>return Err("허용되지 않은 거래처 변경 명령입니다.".into())
    };
    let next=transition.as_object_mut().and_then(|obj|obj.remove("state"))
        .ok_or("Rust 거래처 변경 결과가 없습니다.")?;
    validate_limits(&next["clients"])?;
    Ok(Some(next))
}

pub fn mutate_client_checked_v2(conn:&mut Connection,expected:&Value,action:&str,client:&Value)
    ->Result<Value,String>{
    crate::storage::update_v2_entity_if_changed(conn,true,|before|
        checked_client_transition(before,expected,action,client))
}

#[cfg(test)]
mod client_entity_cas_tests{
    use super::*;
    use serde_json::json;
    fn client(id:&str,name:&str)->Value{
        json!({"id":id,"name":name,"address":"서울","phone":"02-1234-5678"})
    }
    #[test]
    fn unrelated_revision_and_other_client_do_not_block_target_edit(){
        let old=client("c","이전");
        let new=client("c","수정");
        let other=client("d","다른 거래처");
        let before=json!({"revision":99,"clients":[old,other],"orders":[{"id":"o"}]});
        let next=checked_client_transition(&before,&old,"upsert_client",&new).unwrap().unwrap();
        assert_eq!(next["clients"][0],new);
        assert_eq!(next["clients"][1],other);
        assert_eq!(next["orders"],before["orders"]);
    }
    #[test]
    fn same_client_conflict_and_new_id_collision_are_rejected(){
        let old=client("c","이전");
        let latest=client("c","외부 수정");
        let proposed=client("c","내 수정");
        let before=json!({"clients":[latest]});
        assert!(checked_client_transition(&before,&old,"upsert_client",&proposed).is_err());
        assert!(checked_client_transition(&before,&old,"archive_client",&old).is_err());
        assert!(checked_client_transition(&before,&Value::Null,"upsert_client",&proposed).is_err());
        assert!(checked_client_transition(&before,&latest,"upsert_client",&latest).unwrap().is_none());
    }
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
