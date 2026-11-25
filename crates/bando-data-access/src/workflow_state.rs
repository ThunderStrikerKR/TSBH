//! Explicit opt-in shipment/group split, after the event ledger split.
//! Keep native workflow JSON contract; SQL rows own physical persistence.
//! Legacy archived shipments may reference a retired order or group, so
//! native workflow validation remains the semantic link authority.
use rusqlite::{params,Connection,Transaction,TransactionBehavior};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},path::Path};

pub(crate) const MARKER:&str="_workflowStateRelational";
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}

pub(crate) fn active(conn:&Connection)->Result<bool,String>{
    let count:i64=conn.query_row("SELECT COUNT(*) FROM main.sqlite_master
        WHERE type='table' AND name IN ('core_workflow_groups','core_workflow_shipments')",
        [],|row|row.get(0)).map_err(err)?;
    match count{0=>Ok(false),2=>Ok(true),_=>Err("배송 상태 관계형 테이블 일부 누락".into())}
}

fn rows<'a>(state:&'a Value,key:&str)->Result<&'a Vec<Value>,String>{
    state["workflow"][key].as_array().ok_or_else(||format!("업무 {key} 목록 오류"))
}
fn id(row:&Value)->Result<&str,String>{
    row["id"].as_str().filter(|id|!id.trim().is_empty()).ok_or("업무 상태 ID 누락".into())
}
fn table(key:&str)->Result<&'static str,String>{
    match key{
        "groups"=>Ok("core_workflow_groups"),
        "shipments"=>Ok("core_workflow_shipments"),
        _=>Err("지원하지 않는 업무 상태 테이블".into())
    }
}
pub(crate) fn marker(state:&Value)->Result<Value,String>{
    Ok(json!({"version":1,"groups":rows(state,"groups")?.len(),
        "shipments":rows(state,"shipments")?.len()}))
}
fn read_rows(conn:&Connection,key:&str)->Result<Vec<Value>,String>{
    let sql=format!("SELECT position,id,payload FROM {} ORDER BY position,id",table(key)?);
    let mut stmt=conn.prepare(&sql).map_err(err)?;
    let mut query=stmt.query([]).map_err(err)?;
    let mut out=Vec::new();
    while let Some(row)=query.next().map_err(err)?{
        let position:i64=row.get(0).map_err(err)?;
        let stored_id:String=row.get(1).map_err(err)?;
        let raw:String=row.get(2).map_err(err)?;
        let value:Value=serde_json::from_str(&raw).map_err(err)?;
        if position!=i64::try_from(out.len()).map_err(err)? || id(&value)?!=stored_id.as_str(){
            return Err("배송 상태의 순서 또는 ID 무결성 오류".into());
        }
        out.push(value);
    }
    Ok(out)
}

pub(crate) fn hydrate(conn:&Connection,state:&mut Value)->Result<(),String>{
    if !active(conn)?{
        if state.get(MARKER).is_some(){return Err("배송 상태 테이블 누락".into());}
        return Ok(());
    }
    if !crate::workflow_events::active(conn)?{return Err("업무 이벤트 테이블 누락".into());}
    let root=state.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    let stored=root.remove(MARKER).ok_or("배송 상태 저장 형식 누락")?;
    let workflow=root.get_mut("workflow").and_then(Value::as_object_mut).ok_or("업무 상태 목록 누락")?;
    for key in ["groups","shipments"]{
        if workflow.contains_key(key){return Err("배송 상태 이중 저장 오류".into());}
        let data=read_rows(conn,key)?;
        if stored[key].as_u64()!=u64::try_from(data.len()).ok() || stored["version"]!=json!(1){
            return Err("배송 상태 저장 개수 또는 버전 불일치".into());
        }
        workflow.insert(key.into(),Value::Array(data));
    }
    Ok(())
}

fn validate_ids(data:&[Value])->Result<(),String>{
    let mut ids=HashSet::with_capacity(data.len());
    for row in data{
        if !ids.insert(id(row)?){return Err("배송 상태 ID 중복".into());}
    }
    Ok(())
}

fn sync_rows(tx:&Transaction<'_>,key:&str,old:&[Value],new:&[Value])->Result<usize,String>{
    validate_ids(old)?;
    validate_ids(new)?;
    if old==new{return Ok(0);}
    let table=table(key)?;
    let count:i64=tx.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|row|row.get(0)).map_err(err)?;
    if usize::try_from(count).ok()!=Some(old.len()){return Err("배송 상태 이전 개수 불일치".into());}
    let mut old_by_id=HashMap::with_capacity(old.len());
    for (position,row) in old.iter().enumerate(){old_by_id.insert(id(row)?,(position,row));}
    let mut direct=0;
    let mut upsert=tx.prepare(&format!("INSERT INTO {table}(id,position,payload)
        VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET
        position=excluded.position,payload=excluded.payload")).map_err(err)?;
    let mut retained=HashSet::with_capacity(new.len());
    for (position,row) in new.iter().enumerate(){
        let id=id(row)?;
        retained.insert(id);
        if old_by_id.get(id).is_some_and(|(p,v)|*p==position && *v==row){continue;}
        direct+=upsert.execute(params![id,i64::try_from(position).map_err(err)?,serde_json::to_string(row).map_err(err)?]).map_err(err)?;
    }
    drop(upsert);
    let mut delete=tx.prepare(&format!("DELETE FROM {table} WHERE id=?1")).map_err(err)?;
    for id in old_by_id.keys(){
        if !retained.contains(id){
            // Shipment identity is immutable even if a caller bypasses the
            // full reducer. Group removal remains subject to native rules.
            if key=="shipments"{return Err("배송 건 고유 ID와 최초 연결은 보존해야 합니다.".into());}
            direct+=delete.execute([*id]).map_err(err)?;
        }
    }
    Ok(direct)
}

pub(crate) fn write(tx:&Transaction<'_>,before:&Value,next:&Value)->Result<usize,String>{
    let mut direct=0;
    for key in ["groups","shipments"]{
        direct+=sync_rows(tx,key,rows(before,key)?,rows(next,key)?)?;
    }
    Ok(direct)
}

/// Opt-in migration only. The event split must already be active.
/// No UI or startup invocation until the complete compatibility gate passes.
pub fn migrate_with_backup(conn:&mut Connection,base:&Path)->Result<Option<String>,String>{
    if active(conn)?{crate::read_v2_snapshot(conn)?;return Ok(None);}
    if !crate::workflow_events::active(conn)?{return Err("업무 이벤트를 먼저 이관하세요.".into());}
    let version:i64=conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)).map_err(err)?;
    if version!=2{return Err("관계형 DB v2만 지원합니다.".into());}
    let backup=crate::backup_service::manual_backup(conn,base)?;
    let backup_conn=crate::backup_service::read_backup(Path::new(&backup))?;
    let backed_up=crate::read_v2_snapshot(&backup_conn)?;
    drop(backup_conn);
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    if active(&tx)?{return Err("다른 작업에서 배송 상태를 이관했습니다.".into());}
    let mut state=crate::read_v2_snapshot(&tx)?;
    if state!=backed_up{return Err("백업 후 업무 상태가 변경되어 이관하지 않았습니다.".into());}
    crate::rules::validate_workflow_transition(&state,&state)?;
    for key in ["groups","shipments"]{
        let name=table(key)?;
        // Position can temporarily overlap during an atomic reorder.
        // The full projection verifies contiguous unique positions.
        tx.execute_batch(&format!("CREATE TABLE {name}(
            id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id))>0),
            position INTEGER NOT NULL CHECK(position>=0),
            payload TEXT NOT NULL CHECK(json_valid(payload)
                AND json_type(payload,'$.id') IS 'text'
                AND json_extract(payload,'$.id')=id)
        ); CREATE INDEX {name}_position ON {name}(position,id);")).map_err(err)?;
        sync_rows(&tx,key,&[],rows(&state,key)?)?;
    }
    tx.execute_batch("CREATE INDEX core_workflow_groups_source ON core_workflow_groups(json_extract(payload,'$.sourceId'));
        CREATE INDEX core_workflow_shipments_group ON core_workflow_shipments(json_extract(payload,'$.groupId'));
        CREATE INDEX core_workflow_shipments_order ON core_workflow_shipments(json_extract(payload,'$.orderId'));
        CREATE INDEX core_workflow_shipments_recipient ON core_workflow_shipments(json_extract(payload,'$.recipientId'));").map_err(err)?;
    let revision=state["revision"].as_u64().and_then(|r|r.checked_add(1)).ok_or("배송 상태 이관 revision 오류")?;
    state["revision"]=json!(revision);
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",[crate::workflow_events::envelope(&state,true)?]).map_err(err)?;
    if crate::read_v2_snapshot(&tx)?!=state{return Err("배송 상태 이관 결과 불일치".into());}
    tx.commit().map_err(err)?;
    Ok(Some(backup))
}
