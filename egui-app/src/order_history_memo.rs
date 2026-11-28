//! Shipment-scoped management notes for the order history screen.
//! Never write to a preview DB or bypass the native revision transaction.
use crate::{business_state,excel_service,gateway};
use chrono::Utc;
use rusqlite::OpenFlags;
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
    let mut conn=crate::business_state::open_business_connection(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_|"업무 DB를 쓰기 모드로 열지 못했습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|e|e.to_string())?;
    let mut expected_updated_at=None;
    let committed=bando_data_access::update_v2_snapshot_if_changed(
        &mut conn,expected_revision,true,|before|{
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
                return Ok(None);
            }
            let updated_at=Utc::now().to_rfc3339();
            shipment["managementMemo"]=json!(memo);
            shipment["managementMemoUpdatedAt"]=json!(updated_at);
            gateway::rules::execute("validate_workflow",
                json!({"state":&next,"before":before}))?;
            expected_updated_at=Some(updated_at);
            Ok(Some(next))
        }
    )?;
    let saved_revision=committed["revision"].as_u64()
        .ok_or("관리 메모 저장 후 리비전을 확인할 수 없습니다.")?;
    let stored=committed["workflow"]["shipments"].as_array()
        .and_then(|ships|ships.iter().find(|item|
            item["id"].as_str()==Some(shipment_id)))
        .ok_or("저장 후 발송 건을 확인하지 못했습니다.")?;
    if stored["managementMemo"].as_str().unwrap_or("")!=memo ||
       expected_updated_at.as_deref().is_some_and(|at|
           stored["managementMemoUpdatedAt"].as_str()!=Some(at)){
        return Err("저장 후 관리 메모와 갱신시각을 확인하지 못했습니다. 목록을 새로고침하세요.".into());
    }
    Ok(json!({
        "revision":saved_revision,"shipmentId":shipment_id,
        "memo":memo,"updatedAt":stored["managementMemoUpdatedAt"]
    }))
}
