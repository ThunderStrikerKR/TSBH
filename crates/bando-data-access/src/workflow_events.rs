//! Explicit opt-in event split. Older binaries are not compatible.
//! No automatic startup conversion; qualify all entry points first.
use rusqlite::{params,Connection,Transaction,TransactionBehavior};
use serde_json::{json,Value};
use std::{collections::{HashSet,BTreeMap},path::Path};

const MARKER:&str="_workflowEventsRelational";
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}
pub(crate) fn active(conn:&Connection)->Result<bool,String>{
    conn.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master
        WHERE type='table' AND name='core_workflow_events')",[],|row|row.get(0)).map_err(err)
}
fn events(state:&Value)->Result<&Vec<Value>,String>{
    state["workflow"]["events"].as_array().ok_or("업무 이벤트 목록 오류".into())
}
fn event_id(row:&Value)->Result<&str,String>{
    row["id"].as_str().filter(|id|!id.trim().is_empty()).ok_or("업무 이벤트 ID 누락".into())
}
fn read_rows(conn:&Connection)->Result<Vec<Value>,String>{
    let mut stmt=conn.prepare("SELECT sequence,id,payload FROM core_workflow_events ORDER BY sequence").map_err(err)?;
    let mut query=stmt.query([]).map_err(err)?;
    let mut out=Vec::new();
    while let Some(row)=query.next().map_err(err)?{
        let sequence:i64=row.get(0).map_err(err)?;
        let id:String=row.get(1).map_err(err)?;
        let raw:String=row.get(2).map_err(err)?;
        let value:Value=serde_json::from_str(&raw).map_err(err)?;
        if sequence!=i64::try_from(out.len()).map_err(err)? || event_id(&value)?!=id.as_str(){
            return Err("업무 이벤트 순서 또는 ID 무결성 오류".into());
        }
        out.push(value);
    }
    Ok(out)
}

pub(crate) fn hydrate(conn:&Connection,state:&mut Value)->Result<(),String>{
    let enabled=active(conn)?;
    // Preserve legacy behavior for malformed/non-object envelopes when
    // no split table is present; downstream existing validation rejects them.
    if !enabled{
        if state.get(MARKER).is_some(){return Err("업무 이벤트 테이블 누락".into());}
        return Ok(());
    }
    let root=state.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    if root.remove(MARKER)!=Some(json!(1)){return Err("업무 이벤트 저장 형식 불일치".into());}
    let workflow=root.get_mut("workflow").and_then(Value::as_object_mut).ok_or("업무 상태 목록 누락")?;
    if workflow.contains_key("events"){return Err("업무 이벤트 이중 저장 오류".into());}
    workflow.insert("events".into(),Value::Array(read_rows(conn)?));
    Ok(())
}

fn envelope(state:&Value)->Result<String,String>{
    // Stream borrowed fields with serde_json's normal escaping, without
    // introducing another serde dependency or cloning old event payloads.
    let root=state.as_object().ok_or("업무 상태는 객체여야 합니다.")?;
    let workflow=state["workflow"].as_object().ok_or("업무 상태 목록 누락")?;
    let workflow:BTreeMap<&str,&Value>=workflow.iter().filter(|(k,_)|k.as_str()!="events")
        .map(|(k,v)|(k.as_str(),v)).collect();
    let mut bytes=vec![b'{'];
    serde_json::to_writer(&mut bytes,MARKER).map_err(err)?;
    bytes.extend_from_slice(b":1");
    for (key,value) in root{
        if matches!(key.as_str(),"products"|"clients"|"orders"|"workflow") || key==MARKER{continue;}
        bytes.push(b',');
        serde_json::to_writer(&mut bytes,key).map_err(err)?;
        bytes.push(b':');
        serde_json::to_writer(&mut bytes,value).map_err(err)?;
    }
    bytes.extend_from_slice(b",\"workflow\":");
    serde_json::to_writer(&mut bytes,&workflow).map_err(err)?;
    bytes.push(b'}');
    String::from_utf8(bytes).map_err(err)
}

fn insert_events(tx:&Transaction<'_>,start:usize,rows:&[Value])->Result<usize,String>{
    let mut stmt=tx.prepare("INSERT INTO core_workflow_events(sequence,id,payload) VALUES (?1,?2,?3)").map_err(err)?;
    let mut count=0;
    for (i,row) in rows.iter().enumerate(){
        let sequence=i64::try_from(start.checked_add(i).ok_or("업무 이벤트 순서 초과")?).map_err(err)?;
        count+=stmt.execute(params![sequence,event_id(row)?,serde_json::to_string(row).map_err(err)?]).map_err(err)?;
    }
    Ok(count)
}

pub(crate) fn write(tx:&Transaction<'_>,before:&Value,next:&Value)->Result<usize,String>{
    let old=events(before)?;
    let new=events(next)?;
    if !new.starts_with(old){return Err("기존 업무 이벤트는 수정하거나 삭제할 수 없습니다.".into());}
    let count:i64=tx.query_row("SELECT COUNT(*) FROM core_workflow_events",[],|row|row.get(0)).map_err(err)?;
    if usize::try_from(count).ok()!=Some(old.len()){return Err("업무 이벤트 저장 개수 불일치".into());}
    let mut direct=insert_events(tx,old.len(),&new[old.len()..])?;
    direct+=tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",[envelope(next)?]).map_err(err)?;
    Ok(direct)
}

/// Explicit conversion with verified whole-DB backup and atomic round trip.
/// Not called by startup or UI. Requires compatible binaries before rollout.
pub fn migrate_with_backup(conn:&mut Connection,base:&Path)->Result<Option<String>,String>{
    if active(conn)?{crate::read_v2_snapshot(conn)?;return Ok(None);}
    let version:i64=conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],|row|row.get(0)).map_err(err)?;
    if version!=2{return Err("업무 이벤트 이관은 관계형 DB v2만 지원합니다.".into());}
    let backup=crate::backup_service::manual_backup(conn,base)?;
    let backup_conn=crate::backup_service::read_backup(Path::new(&backup))?;
    let backed_up=crate::read_v2_snapshot(&backup_conn)?;
    drop(backup_conn);
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    if active(&tx)?{return Err("다른 작업에서 업무 이벤트 이관이 진행되었습니다.".into());}
    let mut before=crate::read_v2_snapshot(&tx)?;
    if before!=backed_up{return Err("백업 후 업무 상태가 변경되어 이관하지 않았습니다.".into());}
    let rows=events(&before)?;
    let mut ids=HashSet::new();
    for row in rows{
        if !ids.insert(event_id(row)?){return Err("업무 이벤트 ID 중복으로 이관하지 않았습니다.".into());}
    }
    tx.execute_batch("CREATE TABLE core_workflow_events(
        sequence INTEGER PRIMARY KEY CHECK(sequence>=0),
        id TEXT NOT NULL UNIQUE CHECK(length(trim(id))>0),
        payload TEXT NOT NULL CHECK(json_valid(payload)
            AND json_type(payload,'$.id') IS 'text'
            AND json_extract(payload,'$.id')=id)
    );").map_err(err)?;
    insert_events(&tx,0,rows)?;
    let revision=before["revision"].as_u64().and_then(|r|r.checked_add(1))
        .ok_or("업무 이벤트 이관 revision 오류")?;
    before["revision"]=json!(revision);
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",[envelope(&before)?]).map_err(err)?;
    if crate::read_v2_snapshot(&tx)?!=before{return Err("업무 이벤트 이관 결과 검증에 실패했습니다.".into());}
    tx.commit().map_err(err)?;
    Ok(Some(backup))
}
