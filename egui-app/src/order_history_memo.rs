//! Shipment-scoped management notes for the order history screen.
//! Never write to a preview DB or bypass the native revision transaction.
use crate::{business_state,excel_service,gateway};
use chrono::Utc;
use rusqlite::{Connection,OpenFlags};
use serde_json::{json,Value};
use std::time::Duration;

pub fn writable()->bool{
    // This is shipment workflow metadata: use the existing authorized
    // operational shipment writer permission, not Settings or a new DB.
    excel_service::writable()
}

pub fn save(expected_revision:u64,shipment_id:&str,memo:&str)
    ->Result<Value,String>{
    if !writable(){
        return Err("발송 건 관리 메모 저장 권한 또는 업무 DB 연결이 없습니다.".into());
    }
    if shipment_id.trim().is_empty(){
        return Err("관리 메모를 저장할 발송 건을 확인하세요.".into());
    }
    let memo=memo.trim();
    if memo.chars().count()>2_000{
        return Err("관리 메모는 2,000자 이내로 작성하세요.".into());
    }
    let path=business_state::authorized_business_write_path()?;
    let mut conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_|"업무 DB를 쓰기 모드로 열지 못했습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|e|e.to_string())?;
    let before=bando_data_access::read_v2_snapshot(&conn)?;
    let revision=before["revision"].as_u64().ok_or("업무 DB 리비전 누락")?;
    if revision!=expected_revision{
        return Err("다른 작업에서 업무 데이터가 변경되었습니다. 다시 불러온 뒤 저장하세요.".into());
    }
    let mut next=before.clone();
    let shipments=next["workflow"]["shipments"].as_array_mut()
        .ok_or("발송 건 목록을 확인할 수 없습니다.")?;
    let shipment=shipments.iter_mut()
        .find(|item|item["id"].as_str()==Some(shipment_id))
        .ok_or("발송 건이 변경되거나 삭제되었습니다.")?;
    if shipment["legacyArchived"]==true{
        return Err("보관 처리된 발송 건은 변경할 수 없습니다.".into());
    }
    if shipment["managementMemo"].as_str().unwrap_or("")==memo{
        return Ok(json!({
            "revision":revision,"shipmentId":shipment_id,"memo":memo,
            "updatedAt":shipment["managementMemoUpdatedAt"]
        }));
    }
    // The memo text and its last edited time commit atomically; unchanged
    // text must not create a fake new edit timestamp.
    let updated_at=Utc::now().to_rfc3339();
    shipment["managementMemo"]=json!(memo);
    shipment["managementMemoUpdatedAt"]=json!(updated_at);
    // Preserve the existing workflow invariants before an atomic v2 CAS
    // write. The database writer performs the second revision check.
    gateway::rules::execute("validate_workflow",
        json!({"state":&next,"before":&before}))?;
    next["revision"]=json!(revision);
    let committed=bando_data_access::write_v2_snapshot(
        &mut conn,next,revision,true
    )?;
    let saved_revision=committed["revision"].as_u64()
        .ok_or("관리 메모 저장 후 리비전을 확인할 수 없습니다.")?;
    drop(conn);
    let fresh=business_state::load()?;
    if fresh["revision"].as_u64()!=Some(saved_revision){
        return Err("메모는 저장됐지만 업무 데이터가 다시 변경되었습니다. 새로고침하세요.".into());
    }
    let stored=fresh["workflow"]["shipments"].as_array()
        .and_then(|ships|ships.iter().find(|item|
            item["id"].as_str()==Some(shipment_id)));
    if stored.and_then(|row|row["managementMemo"].as_str())!=Some(memo) ||
       stored.and_then(|row|row["managementMemoUpdatedAt"].as_str())!=Some(updated_at.as_str()){
        return Err("저장 후 관리 메모와 갱신시각을 확인하지 못했습니다. 목록을 새로고침하세요.".into());
    }
    Ok(json!({
        "revision":saved_revision,"shipmentId":shipment_id,
        "memo":memo,"updatedAt":updated_at
    }))
}
