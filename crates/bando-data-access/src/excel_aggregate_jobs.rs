//! Durable, derived Excel archive jobs. Never changes the business revision.
use rusqlite::{params,Connection,OptionalExtension};
use serde_json::Value;
use std::{collections::{BTreeMap,HashMap,HashSet},path::{Path,PathBuf}};
use chrono::NaiveDate;

fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn s<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn rows<'a>(v:&'a Value,k:&str)->&'a [Value]{v[k].as_array().map(Vec::as_slice).unwrap_or(&[])}
pub fn initialize(conn:&Connection)->Result<(),String>{
    // Normal reads and writes must not execute schema DDL. Check for
    // both the current column and index so older installations still migrate.
    let ready:bool=conn.query_row("SELECT
        EXISTS(SELECT 1 FROM pragma_table_info('excel_aggregate_jobs') WHERE name='legacy_reply_ids')
        AND EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_excel_aggregate_pending')",
        [],|r|r.get(0)).map_err(err)?;
    if ready{return Ok(())}
    conn.execute_batch("CREATE TABLE IF NOT EXISTS excel_aggregate_jobs(
        business_date TEXT NOT NULL, origin TEXT NOT NULL CHECK(origin IN ('FACTORY','STORE','REPLY')),
        target_path TEXT NOT NULL, desired_revision INTEGER NOT NULL CHECK(desired_revision>=0),
        saved_revision INTEGER NOT NULL DEFAULT 0 CHECK(saved_revision>=0),
        retry_count INTEGER NOT NULL DEFAULT 0 CHECK(retry_count>=0), next_retry_at INTEGER,
        last_error TEXT, file_hash TEXT, legacy_reply_ids TEXT NOT NULL DEFAULT '[]', CHECK(saved_revision<=desired_revision),
        PRIMARY KEY(business_date,origin,target_path));
        CREATE INDEX IF NOT EXISTS idx_excel_aggregate_pending ON excel_aggregate_jobs(next_retry_at)
        WHERE desired_revision>saved_revision;").map_err(err)?;
    // Existing development databases may have the first version of this table.
    let has:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('excel_aggregate_jobs') WHERE name='legacy_reply_ids')",[],|r|r.get(0)).map_err(err)?;
    if !has{conn.execute_batch("ALTER TABLE excel_aggregate_jobs ADD COLUMN legacy_reply_ids TEXT NOT NULL DEFAULT '[]';").map_err(err)?;}
    Ok(())
}
pub fn target(root:&Path,date:&str,origin:&str)->Result<PathBuf,String>{
    let day=NaiveDate::parse_from_str(date,"%Y-%m-%d").map_err(err)?;
    let category=match origin{"FACTORY"=>"공장 발송","STORE"=>"매장 발송","REPLY"=>"공장 회신",_=>return Err("취합 유형 오류".into())};
    if !root.is_absolute(){return Err("취합 보관함은 절대 경로여야 합니다.".into())}
    Ok(root.join(category).join(day.format("%Y년").to_string())
        .join(day.format("%m월").to_string()).join(day.format("%d일").to_string())
        .join(format!("{}(취합).xlsx",day.format("%Y%m%d"))))
}
pub fn export_target(path:&Path,origin:&str)->Result<(String,PathBuf),String>{
    let file=path.file_name().and_then(|x|x.to_str()).ok_or("출력 파일명 오류")?;
    let raw=file.get(..8).ok_or("출력 날짜 누락")?;
    let date=NaiveDate::parse_from_str(raw,"%Y%m%d").map_err(err)?.format("%Y-%m-%d").to_string();
    let root=path.ancestors().nth(5).ok_or("출력 보관 경로 오류")?;
    let t=target(root,&date,origin)?;
    if t.parent()!=path.parent(){return Err("출력 날짜 또는 유형 경로 불일치".into())}
    Ok((date,t))
}
pub(crate) fn enqueue(conn:&Connection,date:&str,origin:&str,path:&str,revision:u64)->Result<(),String>{
    initialize(conn)?;
    enqueue_initialized(conn,date,origin,path,revision)
}
fn enqueue_initialized(conn:&Connection,date:&str,origin:&str,path:&str,revision:u64)->Result<(),String>{
    let root=Path::new(path).ancestors().nth(5).ok_or("취합 경로 오류")?;
    if target(root,date,origin)?!=Path::new(path){return Err("취합 경로 불일치".into())}
    let rev=i64::try_from(revision).map_err(err)?;
    if rev==0{return Err("취합 버전 오류".into())}
    conn.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision)
        VALUES (?1,?2,?3,?4) ON CONFLICT(business_date,origin,target_path) DO UPDATE SET
        desired_revision=MAX(desired_revision,excluded.desired_revision), retry_count=0,next_retry_at=NULL,last_error=NULL",
        params![date,origin,path,rev]).map_err(err)?;
    Ok(())
}
#[derive(Debug,Clone)]
pub struct Job{pub date:String,pub origin:String,pub path:String,pub desired:i64,pub saved:i64,pub retries:i64,pub hash:Option<String>}
pub fn jobs(conn:&Connection,now:i64,all:bool)->Result<Vec<Job>,String>{
    initialize(conn)?;
    if all{all_initialized(conn)}else{pending_initialized(conn,now,-1)}
}
fn job_row(r:&rusqlite::Row<'_>)->rusqlite::Result<Job>{
    Ok(Job{date:r.get(0)?,origin:r.get(1)?,path:r.get(2)?,desired:r.get(3)?,
        saved:r.get(4)?,retries:r.get(5)?,hash:r.get(6)?})
}
pub fn all_initialized(conn:&Connection)->Result<Vec<Job>,String>{
    let mut stmt=conn.prepare("SELECT business_date,origin,target_path,desired_revision,saved_revision,retry_count,file_hash
        FROM excel_aggregate_jobs ORDER BY business_date,origin,target_path").map_err(err)?;
    let iter=stmt.query_map([],job_row).map_err(err)?;
    iter.collect::<Result<Vec<_>,_>>().map_err(err)
}
/// Indexed pending-only query; limit at SQL boundary instead of allocating
/// every pending job in the process.
pub fn pending_initialized(conn:&Connection,now:i64,limit:i64)->Result<Vec<Job>,String>{
    let mut stmt=conn.prepare("SELECT business_date,origin,target_path,desired_revision,saved_revision,retry_count,file_hash
        FROM excel_aggregate_jobs WHERE desired_revision>saved_revision
        AND (next_retry_at IS NULL OR next_retry_at<=?1)
        ORDER BY business_date,origin,target_path LIMIT ?2").map_err(err)?;
    let iter=stmt.query_map(params![now,limit],job_row).map_err(err)?;
    iter.collect::<Result<Vec<_>,_>>().map_err(err)
}
/// The worker sleeps until the actual next durable retry, not a fixed poll.
pub fn next_due(conn:&Connection)->Result<Option<i64>,String>{
    conn.query_row("SELECT MIN(COALESCE(next_retry_at,0)) FROM excel_aggregate_jobs
        WHERE desired_revision>saved_revision",[],|r|r.get(0)).map_err(err)
}
pub fn current(conn:&Connection,j:&Job)->Result<Option<Job>,String>{
    conn.query_row("SELECT business_date,origin,target_path,desired_revision,saved_revision,retry_count,file_hash
        FROM excel_aggregate_jobs WHERE business_date=?1 AND origin=?2 AND target_path=?3",params![j.date,j.origin,j.path],
        |r|Ok(Job{date:r.get(0)?,origin:r.get(1)?,path:r.get(2)?,desired:r.get(3)?,saved:r.get(4)?,retries:r.get(5)?,hash:r.get(6)?}))
        .optional().map_err(err)
}
pub fn mark_saved(conn:&Connection,j:&Job,hash:&str)->Result<(),String>{
    conn.execute("UPDATE excel_aggregate_jobs SET saved_revision=?4,file_hash=?5,
        retry_count=0,next_retry_at=NULL,last_error=NULL WHERE business_date=?1 AND origin=?2 AND target_path=?3
        AND desired_revision>=?4 AND saved_revision<=?4",params![j.date,j.origin,j.path,j.desired,hash]).map_err(err)?;
    Ok(())
}
pub fn mark_failed(conn:&Connection,j:&Job,now:i64,error:&str)->Result<(),String>{
    let delay=match j.retries{0=>5,1=>15,2=>30,_=>60};
    conn.execute("UPDATE excel_aggregate_jobs SET retry_count=retry_count+1,next_retry_at=?5,last_error=?6
        WHERE business_date=?1 AND origin=?2 AND target_path=?3 AND desired_revision=?4
        AND desired_revision>saved_revision",
        params![j.date,j.origin,j.path,j.desired,now+delay,error]).map_err(err)?;
    Ok(())
}
pub fn invalidate(conn:&Connection,j:&Job)->Result<(),String>{
    conn.execute("UPDATE excel_aggregate_jobs SET saved_revision=0,next_retry_at=NULL,file_hash=NULL
        WHERE business_date=?1 AND origin=?2 AND target_path=?3 AND saved_revision=?4 AND desired_revision=?5",
        params![j.date,j.origin,j.path,j.saved,j.desired]).map_err(err)?;
    Ok(())
}
/// Project only the aggregate-owned arrays at the SQLite boundary. JSON1
/// still parses the envelope; unrelated workflow data is not materialized in Rust.
fn history_prefix(conn:&Connection)->Result<&'static str,String>{
    let version=crate::recovery::schema_version(conn)?;
    if version>3{return Err("이 프로그램보다 새로운 DB 구조입니다.".into())}
    if crate::workflow_events::active(conn)?!=(version==3){return Err("업무 이벤트 테이블과 DB 버전 불일치".into())}
    let prefix=if version==3{"$[0]"}else{"$"};
    let root_valid:bool=conn.query_row("SELECT json_type(payload,?1)='object'
        AND (?2=0 OR (json_type(payload)='array' AND json_array_length(payload)=1)) FROM app_state WHERE id=1",
        params![prefix,version==3],|r|r.get(0)).map_err(err)?;
    if !root_valid{return Err("업무 상태 저장 형식 오류".into())}
    Ok(prefix)
}
fn read_history(conn:&Connection,origin:Option<&str>)->Result<Value,String>{
    let prefix=history_prefix(conn)?;
    let path=|key:&str|format!("{prefix}.{key}");
    let raw:String=conn.query_row("SELECT json_object(
        'revision',json_extract(payload,?1),
        'exports',CASE WHEN ?4 THEN json_extract(payload,?2) ELSE NULL END,
        'factoryReplies',CASE WHEN ?5 THEN json_extract(payload,?3) ELSE NULL END)
        FROM app_state WHERE id=1",
        params![path("revision"),path("exports"),path("factoryReplies"),origin!=Some("REPLY"),
            origin.is_none()||origin==Some("REPLY")],|r|r.get(0)).map_err(err)?;
    serde_json::from_str(&raw).map_err(err)
}
/// v2 native settings lookup: no trade partner, export or workflow hydration.
pub fn archive_directory_v2(conn:&Connection)->Result<Option<String>,String>{
    conn.query_row("SELECT json_extract(payload,'$.settings.excelDirectory') FROM app_state WHERE id=1",
        [],|r|r.get(0)).map_err(err)
}
pub fn legacy_ids(conn:&Connection,j:&Job)->Result<Vec<String>,String>{
    let raw:String=conn.query_row("SELECT legacy_reply_ids FROM excel_aggregate_jobs WHERE business_date=?1 AND origin=?2 AND target_path=?3",
        params![j.date,j.origin,j.path],|r|r.get(0)).map_err(err)?;
    serde_json::from_str(&raw).map_err(err)
}
fn reply_date(record:&Value)->Option<String>{
    let date=s(record,"businessDate");
    if let Ok(d)=NaiveDate::parse_from_str(date,"%Y-%m-%d"){return Some(d.format("%Y-%m-%d").to_string())}
    chrono::DateTime::parse_from_rfc3339(s(record,"at")).ok().map(|d|
        d.with_timezone(&chrono::FixedOffset::east_opt(9*3600).expect("KST")).format("%Y-%m-%d").to_string())
}
/// Caller owns one immediate transaction. Backfill references only, never edit
/// immutable business history. Once assigned, a legacy reply keeps its path.
pub fn recover_history(conn:&Connection,configured_root:Option<&Path>)->Result<(),String>{
    initialize(conn)?;
    let state=read_history(conn,None)?;
    let revision=state["revision"].as_u64().ok_or("취합 복구 revision 누락")?;
    if revision==0{return Ok(())}
    let existing=all_initialized(conn)?;
    let mut keys:HashSet<(String,String,String)>=existing.iter().map(|j|(j.date.clone(),j.origin.clone(),j.path.clone())).collect();
    for e in rows(&state,"exports"){
        let path=Path::new(s(e,"savedPath"));
        for origin in ["FACTORY","STORE"]{
            let Ok((date,target))=export_target(path,origin)else{continue};
            let path=target.to_str().ok_or("취합 경로 인코딩 오류")?;
            if keys.insert((date.clone(),origin.into(),path.into())){enqueue_initialized(conn,&date,origin,path,revision)?;}
        }
    }
    let mut assigned=HashSet::new();
    // Fetch all legacy bindings in one query, rather than one per past day.
    let mut stmt=conn.prepare("SELECT legacy_reply_ids FROM excel_aggregate_jobs WHERE origin='REPLY'").map_err(err)?;
    for raw in stmt.query_map([],|r|r.get::<_,String>(0)).map_err(err)?{
        assigned.extend(serde_json::from_str::<Vec<String>>(&raw.map_err(err)?).map_err(err)?);
    }
    drop(stmt);
    let mut groups:HashMap<(String,String),Vec<String>>=HashMap::new();
    for r in rows(&state,"factoryReplies"){
        if !rows(r,"rows").iter().any(|row|row["applied"]==true){continue}
        let Some(date)=reply_date(r)else{continue};
        let explicit=s(r,"aggregateTargetPath");
        if !explicit.is_empty(){
            if keys.insert((date.clone(),"REPLY".into(),explicit.into())){enqueue_initialized(conn,&date,"REPLY",explicit,revision)?;}
            continue
        }
        let id=s(r,"id");
        if id.is_empty()||assigned.contains(id){continue}
        let Some(root)=configured_root else{continue};
        let path=target(root,&date,"REPLY")?.to_str().ok_or("취합 경로 인코딩 오류")?.to_owned();
        groups.entry((date,path)).or_default().push(id.to_owned());
        assigned.insert(id.to_owned());
    }
    for ((date,path),ids) in groups{
        enqueue_initialized(conn,&date,"REPLY",&path,revision)?;
        let j=Job{date,origin:"REPLY".into(),path,desired:0,saved:0,retries:0,hash:None};
        let mut all=legacy_ids(conn,&j)?;all.extend(ids);
        // Invalidate even if the legacy binding is discovered at the same
        // business revision as an already published explicit reply.
        conn.execute("UPDATE excel_aggregate_jobs SET legacy_reply_ids=?4,saved_revision=0,file_hash=NULL,next_retry_at=NULL
            WHERE business_date=?1 AND origin=?2 AND target_path=?3",
            params![j.date,j.origin,j.path,serde_json::to_string(&all).map_err(err)?]).map_err(err)?;
    }
    Ok(())
}
/// A restored snapshot is a new generation. Any pre-restore worker must
/// fail its final generation check, including jobs copied from older backups.
pub fn invalidate_after_restore(conn:&Connection,revision:u64)->Result<(),String>{
    initialize(conn)?;
    conn.execute("UPDATE excel_aggregate_jobs SET desired_revision=?1,saved_revision=0,file_hash=NULL,
        next_retry_at=NULL,retry_count=0,last_error=NULL",[i64::try_from(revision).map_err(err)?]).map_err(err)?;
    Ok(())
}
/// Read only the metadata envelope, not order/catalog tables.
pub fn daily_rows<F>(conn:&Connection,j:&Job,legacy:F)->Result<Vec<Vec<String>>,String>
where F:FnMut(&Value,&Value)->Result<Vec<String>,String>{
    let ids=legacy_ids(conn,j)?;
    daily_rows_with_binding(conn,j,&ids,legacy)
}
pub fn daily_rows_with_binding<F>(conn:&Connection,j:&Job,ids:&[String],legacy:F)->Result<Vec<Vec<String>>,String>
where F:FnMut(&Value,&Value)->Result<Vec<String>,String>{
    let state=read_daily_history(conn,j,ids)?;
    project_with(&state,j,ids,legacy)
}
// Only deserialize candidate records. SQLite still parses the physical JSON
// envelope; the original business record remains the source of truth.
fn read_daily_history(conn:&Connection,j:&Job,ids:&[String])->Result<Value,String>{
    let prefix=history_prefix(conn)?;
    let mut state=serde_json::Map::new();
    if j.origin!="REPLY"{
        let path=format!("{prefix}.exports");
        let day=NaiveDate::parse_from_str(&j.date,"%Y-%m-%d").map_err(err)?.format("%Y%m%d").to_string();
        // The required filename date is a conservative prefilter, including
        // noncanonical separators. Rust still validates exact origin/path.
        let mut stmt=conn.prepare("SELECT e.value FROM app_state a, json_each(a.payload,?1) e
            WHERE a.id=1 AND e.type='object' AND instr(json_extract(e.value,'$.savedPath'),?2)>0
            ORDER BY CAST(e.key AS INTEGER)").map_err(err)?;
        let mut exports=Vec::new();
        for raw in stmt.query_map(params![path,day],|r|r.get::<_,String>(0)).map_err(err)?{
            exports.push(serde_json::from_str(&raw.map_err(err)?).map_err(err)?);
        }
        state.insert("exports".into(),Value::Array(exports));
    }else{
        let path=format!("{prefix}.factoryReplies");
        let bindings=serde_json::to_string(ids).map_err(err)?;
        let mut stmt=conn.prepare("SELECT CAST(e.key AS INTEGER),e.value
            FROM app_state a,json_each(a.payload,?1) e WHERE a.id=1 AND e.type='object'
            AND (json_extract(e.value,'$.aggregateTargetPath')=?2 OR
            (CASE WHEN json_type(e.value,'$.aggregateTargetPath')='text'
              THEN json_extract(e.value,'$.aggregateTargetPath') ELSE '' END = ''
            AND json_extract(e.value,'$.id') IN (SELECT value FROM json_each(?3))))").map_err(err)?;
        let mut selected:BTreeMap<i64,Value>=BTreeMap::new();
        let mut referenced=HashSet::new();
        for item in stmt.query_map(params![path,j.path,bindings],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(err)?{
            let (position,raw)=item.map_err(err)?;
            let record:Value=serde_json::from_str(&raw).map_err(err)?;
            if reply_date(&record).as_deref()!=Some(j.date.as_str()){continue}
            let reference=s(&record["resolves"],"id");
            if !reference.is_empty(){referenced.insert(reference.to_owned());}
            selected.insert(position,record);
        }
        drop(stmt);
        // A correction may reference an original on a different business
        // day. Keep that source at its original relative ledger position,
        // not at the approval time. Include all matches (last ID wins).
        if !referenced.is_empty(){
            let keys=serde_json::to_string(&referenced).map_err(err)?;
            let mut stmt=conn.prepare("SELECT CAST(e.key AS INTEGER),e.value FROM app_state a,
                json_each(a.payload,?1) e WHERE a.id=1 AND e.type='object'
                AND json_extract(e.value,'$.id') IN (SELECT value FROM json_each(?2))").map_err(err)?;
            for item in stmt.query_map(params![path,keys],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(err)?{
                let (position,raw)=item.map_err(err)?;
                if let std::collections::btree_map::Entry::Vacant(entry)=selected.entry(position){
                    entry.insert(serde_json::from_str(&raw).map_err(err)?);
                }
            }
        }
        state.insert("factoryReplies".into(),Value::Array(selected.into_values().collect()));
    }
    Ok(Value::Object(state))
}
fn values(v:&Value,n:usize)->Result<Vec<String>,String>{
    let arr=v.as_array().ok_or("취합 출력값 누락")?;
    if arr.len()!=n{return Err("취합 출력 열 수 오류".into())}
    arr.iter().map(|v|v.as_str().map(str::to_owned).ok_or_else(||"취합 출력값 형식 오류".into())).collect()
}
#[cfg(test)]
fn project(state:&Value,j:&Job)->Result<Vec<Vec<String>>,String>{
    project_with(state,j,&[],|_,_|Err("기존 회신 원본 필요".into()))
}
fn project_with<F>(state:&Value,j:&Job,legacy_ids:&[String],mut legacy:F)->Result<Vec<Vec<String>>,String>
where F:FnMut(&Value,&Value)->Result<Vec<String>,String>{
    let mut out=Vec::new();
    if j.origin!="REPLY"{
        let mut seen=HashSet::new();
        for e in rows(state,"exports"){
            let p=Path::new(s(e,"savedPath"));
            if p.parent()!=Path::new(&j.path).parent(){continue}
            let Ok((date,t))=export_target(p,&j.origin)else{continue};
            if date!=j.date||t!=Path::new(&j.path)||!seen.insert(s(e,"id")){continue}
            let records=e["outputRows"].as_array().filter(|r|!r.is_empty()).ok_or("기존 추출 이력의 원본 출력 행이 없습니다.")?;
            for row in records{out.push(values(&row["values"],16)?);}
        }
    }else{
        let history=rows(state,"factoryReplies");
        let positions:HashMap<&str,usize>=history.iter().enumerate().map(|(i,r)|(s(r,"id"),i)).collect();
        let legacy_ids:HashSet<&str>=legacy_ids.iter().map(String::as_str).collect();
        let mut selected:HashMap<&str,((usize,usize),usize,&Value,&Value)>=HashMap::new();
        for (i,r) in history.iter().enumerate(){
            let bound=s(r,"aggregateTargetPath")==j.path || (s(r,"aggregateTargetPath").is_empty()&&legacy_ids.contains(s(r,"id")));
            if !bound||reply_date(r).as_deref()!=Some(j.date.as_str()){continue}
            let sequence=positions.get(s(&r["resolves"],"id")).copied().unwrap_or(i);
            for (row_index,row) in rows(r,"rows").iter().enumerate(){
                let shipment=s(row,"shipmentId");
                if row["applied"]!=true||shipment.is_empty(){continue}
                match selected.get_mut(shipment){
                    Some((_,previous,old_record,old_row)) if sequence>=*previous=>{*previous=sequence;*old_record=r;*old_row=row;}
                    Some(_)=>{},
                    None=>{selected.insert(shipment,((i,row_index),sequence,r,row));}
                }
            }
        }
        let mut ordered:Vec<_>=selected.into_iter().collect();
        ordered.sort_by(|(a,(i,_,_,_)),(b,(k,_,_,_))|i.cmp(k).then_with(||a.cmp(b)));
        for (_,(_,_,record,row)) in ordered{
            let mut v=if row.get("aggregateValues").is_some(){values(&row["aggregateValues"],17)?}else{
                let original=positions.get(s(&record["resolves"],"id")).map(|i|&history[*i]).unwrap_or(record);
                legacy(original,row)?
            };
            if v.len()!=17{return Err("회신 원본 열 수 오류".into())}
            v[6]=row["actual"].as_i64().ok_or("회신 운임 누락")?.to_string();
            v[16]=s(row,"tracking").to_owned();
            out.push(v);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn root()->PathBuf{std::env::temp_dir().join("aggregate-fixture")}
    #[test]
    fn pending_batch_is_bounded_and_settings_lookup_needs_no_client_tables(){
        let c=Connection::open_in_memory().unwrap();initialize(&c).unwrap();
        c.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT INTO core_schema VALUES('business-core',2);CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        c.execute("INSERT INTO app_state VALUES(1,?1)",[json!({"revision":1,"settings":{"excelDirectory":"fixture"},
            "workflow":{"events":[{"id":"unused"}]},"exports":[],"factoryReplies":[]}).to_string()]).unwrap();
        assert_eq!(archive_directory_v2(&c).unwrap().as_deref(),Some("fixture"));
        let p=read_history(&c,Some("STORE")).unwrap();
        assert!(p.get("workflow").is_none());assert!(p["factoryReplies"].is_null());
        for day in 1..=20{
            let date=format!("2026-11-{day:02}");
            let p=target(&root(),&date,"STORE").unwrap();
            enqueue(&c,&date,"STORE",p.to_str().unwrap(),1).unwrap();
        }
        let batch=pending_initialized(&c,0,16).unwrap();assert_eq!(batch.len(),16);
        for j in batch{mark_saved(&c,&j,"hash").unwrap();}
        assert_eq!(pending_initialized(&c,0,16).unwrap().len(),4);
        assert_eq!(all_initialized(&c).unwrap().len(),20);
    }
    #[test]
    fn next_due_ignores_completed_and_tracks_actual_retry(){
        let c=Connection::open_in_memory().unwrap();initialize(&c).unwrap();
        assert_eq!(next_due(&c).unwrap(),None);
        let p=target(&root(),"2026-11-27","STORE").unwrap();
        enqueue(&c,"2026-11-27","STORE",p.to_str().unwrap(),1).unwrap();
        assert_eq!(next_due(&c).unwrap(),Some(0));
        let j=jobs(&c,100,false).unwrap().remove(0);
        mark_failed(&c,&j,100,"locked").unwrap();
        assert_eq!(next_due(&c).unwrap(),Some(105));
        mark_saved(&c,&j,"hash").unwrap();
        assert_eq!(next_due(&c).unwrap(),None);
    }
    #[test]
    fn transaction_rollback_and_generation_coalescing(){
        let mut c=Connection::open_in_memory().unwrap();
        initialize(&c).unwrap();
        let p=target(&root(),"2026-11-27","FACTORY").unwrap();let p=p.to_str().unwrap();
        {let tx=c.transaction().unwrap();enqueue(&tx,"2026-11-27","FACTORY",p,1).unwrap();}
        assert!(jobs(&c,0,true).unwrap().is_empty());
        enqueue(&c,"2026-11-27","FACTORY",p,2).unwrap();
        let old=jobs(&c,0,false).unwrap().remove(0);
        enqueue(&c,"2026-11-27","FACTORY",p,3).unwrap();
        mark_saved(&c,&old,"hash").unwrap();
        let pending=jobs(&c,0,false).unwrap();assert_eq!(pending.len(),1);
        assert_eq!((pending[0].desired,pending[0].saved),(3,2));
        mark_failed(&c,&pending[0],100,"locked").unwrap();
        assert!(jobs(&c,104,false).unwrap().is_empty());assert_eq!(jobs(&c,105,false).unwrap().len(),1);
    }
    #[test]
    fn duplicate_export_id_not_duplicate_identical_rows(){
        let p=target(&root(),"2026-11-27","STORE").unwrap();
        let file=p.with_file_name("20261127(매장1).xlsx");
        let e=json!({"id":"one","savedPath":file,"outputRows":[{"values":vec!["same";16]}]});
        let mut e2=e.clone();e2["id"]=json!("two");
        let j=Job{date:"2026-11-27".into(),origin:"STORE".into(),path:p.to_string_lossy().into_owned(),desired:2,saved:0,retries:0,hash:None};
        assert_eq!(project(&json!({"exports":[e.clone(),e,e2]}),&j).unwrap().len(),2);
    }
    fn metadata(conn:&Connection,state:&Value){
        conn.execute_batch("CREATE TABLE IF NOT EXISTS core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT OR IGNORE INTO core_schema VALUES('business-core',2);
            CREATE TABLE IF NOT EXISTS app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        conn.execute("INSERT INTO app_state(id,payload) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            [state.to_string()]).unwrap();
    }
    fn export(root:&Path,date:&str,origin:&str,n:u32)->Value{
        let target=target(root,date,origin).unwrap();
        let name=format!("{}({}{n}).xlsx",date.replace('-',""),
            if origin=="STORE"{"매장"}else{"공장"});
        json!({"id":format!("{date}-{origin}-{n}"),"savedPath":target.with_file_name(name),
            "outputRows":[{"values":vec![format!("{n}");16]}]})
    }
    #[test]
    fn scoped_projection_preserves_cross_day_correction_and_ledger_order(){
        let c=Connection::open_in_memory().unwrap();let root=root();
        let path=target(&root,"2026-11-28","REPLY").unwrap();
        let j=Job{date:"2026-11-28".into(),origin:"REPLY".into(),path:path.to_str().unwrap().into(),desired:8,saved:0,retries:0,hash:None};
        let old=target(&root,"2026-11-27","REPLY").unwrap();
        let state=json!({"revision":8,"factoryReplies":[
            {"id":"original","businessDate":"2026-11-27","aggregateTargetPath":old,"rows":[{"row":2,"applied":false}]},
            {"id":"unrelated","businessDate":"2026-11-27","aggregateTargetPath":old,"rows":[]},
            {"id":"newer","businessDate":"2026-11-28","aggregateTargetPath":j.path,"rows":[{"row":2,"applied":true,"shipmentId":"s","actual":3000,"tracking":"00000009","aggregateValues":vec!["";17]}]},
            {"id":"correction","businessDate":"2026-11-28","resolves":{"id":"original","row":2},"rows":[{"row":2,"applied":true,"shipmentId":"s","actual":0,"tracking":"00000002"}]},
            {"id":"other-correction","businessDate":"2026-11-28","resolves":{"id":"original","row":2},"rows":[{"row":2,"applied":true,"shipmentId":"t","actual":0,"tracking":"00000003"}]}]});
        metadata(&c,&state);
        let ids=vec!["correction".into(),"other-correction".into()];
        let legacy=|r:&Value,_:&Value|{assert_eq!(r["id"],"original");Ok(vec!["".to_owned();17])};
        let scoped=read_daily_history(&c,&j,&ids).unwrap();
        assert_eq!(rows(&scoped,"factoryReplies").len(),4);
        let a=project_with(&state,&j,&ids,legacy).unwrap();
        let b=daily_rows_with_binding(&c,&j,&ids,legacy).unwrap();
        assert_eq!(a,b);assert_eq!(b[0][16],"00000009");assert_eq!(b[1][16],"00000003");
    }
    #[test]
    #[ignore = "isolated Rust performance comparison; run with --ignored --nocapture --test-threads=1"]
    fn scoped_history_benchmark_10000_and_100000_historical_records(){
        use std::time::{Duration,Instant};
        fn median(samples:&mut [Duration])->f64{
            samples.sort_unstable();samples[samples.len()/2].as_secs_f64()*1000.
        }
        let root=root();
        eprintln!("AGGREGATE_BENCH profile={} sqlite={} samples=5 daily=100",
            if cfg!(debug_assertions){"debug"}else{"release"},rusqlite::version());
        for origin in ["STORE","REPLY"]{
            for historical in [10_000_u32,100_000]{
                let c=Connection::open_in_memory().unwrap();
                let p=target(&root,"2026-11-27",origin).unwrap();
                let j=Job{date:"2026-11-27".into(),origin:origin.into(),path:p.to_str().unwrap().into(),desired:1,saved:0,retries:0,hash:None};
                let old=target(&root,"2025-11-27",origin).unwrap();
                let state=if origin=="STORE"{
                    let mut records=(1..=historical).map(|n|export(&root,"2025-11-27",origin,n)).collect::<Vec<_>>();
                    records.extend((1..=100).map(|n|export(&root,"2026-11-27",origin,n)));
                    json!({"revision":1,"exports":records})
                }else{
                    let record=|i:u32,date:&str,path:&Path|json!({"id":format!("reply-{i}"),"businessDate":date,"aggregateTargetPath":path,
                        "rows":[{"shipmentId":format!("s-{i}"),"applied":true,"actual":3000,
                        "tracking":format!("{i:012}"),"aggregateValues":vec!["";17]}]});
                    let mut records=(1..=historical).map(|n|record(n,"2025-11-27",&old)).collect::<Vec<_>>();
                    records.extend((historical+1..=historical+100).map(|n|record(n,"2026-11-27",&p)));
                    // One actual cross-day correction reference in the daily
                    // scope; it cannot supersede the newer accepted source.
                    records.last_mut().unwrap()["resolves"]=json!({"id":"reply-1","row":2});
                    json!({"revision":1,"factoryReplies":records})
                };
                metadata(&c,&state);drop(state);
                let bytes:i64=c.query_row("SELECT length(CAST(payload AS BLOB)) FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
                let key=if origin=="REPLY"{"factoryReplies"}else{"exports"};
                let expected=project(&read_history(&c,Some(origin)).unwrap(),&j).unwrap();
                assert_eq!(expected.len(),100);
                assert_eq!(daily_rows_with_binding(&c,&j,&[],|_,_|panic!("unexpected legacy lookup")).unwrap(),expected);
                let scoped_records=rows(&read_daily_history(&c,&j,&[]).unwrap(),key).len();
                assert_eq!(scoped_records,if origin=="REPLY"{101}else{100});
                let mut baseline=Vec::new();let mut scoped=Vec::new();
                for i in 0..5{
                    let full=||{
                        let t=Instant::now();
                        let data=read_history(&c,Some(origin)).unwrap();
                        let result=project(&data,&j).unwrap();
                        let elapsed=t.elapsed();
                        assert_eq!(result,expected);assert_eq!(rows(&data,key).len(),historical as usize+100);
                        elapsed
                    };
                    let filtered=||{
                        let t=Instant::now();
                        let result=daily_rows_with_binding(&c,&j,&[],|_,_|panic!("unexpected legacy lookup")).unwrap();
                        let elapsed=t.elapsed();assert_eq!(result,expected);elapsed
                    };
                    // Alternate order to reduce warm-cache ordering bias.
                    if i%2==0{baseline.push(full());scoped.push(filtered());}
                    else{scoped.push(filtered());baseline.push(full());}
                }
                eprintln!("AGGREGATE_PROJECTION origin={origin} historical={historical} daily=100 payload_bytes={bytes} full_records={} scoped_records={scoped_records} baseline_median_ms={:.3} scoped_median_ms={:.3}",
                    historical+100,median(&mut baseline),median(&mut scoped));
            }
        }
    }
    #[test]
    fn ten_exports_accumulate_without_cross_origin_or_date(){
        let c=Connection::open_in_memory().unwrap();let root=root();
        let mut records:Vec<_>=(1..=10).map(|n|export(&root,"2026-11-27","STORE",n)).collect();
        records.push(export(&root,"2026-11-27","FACTORY",1));
        records.push(export(&root,"2026-11-28","STORE",1));
        metadata(&c,&json!({"revision":12,"exports":records}));
        recover_history(&c,Some(&root)).unwrap();
        let jobs=jobs(&c,0,true).unwrap();assert_eq!(jobs.len(),3);
        let j=jobs.iter().find(|j|j.date=="2026-11-27"&&j.origin=="STORE").unwrap();
        let rows=daily_rows(&c,j,|_,_|panic!("no reply")).unwrap();
        assert_eq!(rows.len(),10);assert_eq!(rows[0][0],"1");assert_eq!(rows[9][0],"10");
        recover_history(&c,Some(&root)).unwrap();
        assert_eq!(super::jobs(&c,0,true).unwrap().len(),3);
    }
    #[test]
    fn historical_legacy_reply_waits_for_configuration_then_keeps_original_binding(){
        let c=Connection::open_in_memory().unwrap();let root=root();let other=root.join("new-root");
        let reply=json!({"id":"legacy","businessDate":"2026-11-27","rows":[{"row":2,"applied":true,
            "shipmentId":"s","actual":3000,"tracking":"00000001"}]});
        metadata(&c,&json!({"revision":4,"factoryReplies":[reply]}));
        recover_history(&c,None).unwrap();assert!(jobs(&c,0,true).unwrap().is_empty());
        recover_history(&c,Some(&root)).unwrap();
        let j=jobs(&c,0,true).unwrap().remove(0);
        assert_eq!(j.path,target(&root,"2026-11-27","REPLY").unwrap().to_str().unwrap());
        let v=daily_rows(&c,&j,|record,row|{assert_eq!(record["id"],"legacy");assert_eq!(row["row"],2);
            Ok(vec!["value".to_owned();17])}).unwrap();
        assert_eq!(v[0][6],"3000");assert_eq!(v[0][16],"00000001");
        recover_history(&c,Some(&other)).unwrap();assert_eq!(jobs(&c,0,true).unwrap().len(),1);
        mark_saved(&c,&j,"hash").unwrap();recover_history(&c,Some(&root)).unwrap();
        assert!(jobs(&c,0,false).unwrap().is_empty());
    }
    #[test]
    fn legacy_correction_reads_original_and_missing_original_fails_closed(){
        let root=root();let p=target(&root,"2026-11-28","REPLY").unwrap();
        let j=Job{date:"2026-11-28".into(),origin:"REPLY".into(),path:p.to_str().unwrap().into(),desired:8,saved:0,retries:0,hash:None};
        let state=json!({"factoryReplies":[
            {"id":"original","businessDate":"2026-11-27","rows":[{"row":2,"applied":false}]},
            {"id":"correction","businessDate":"2026-11-28","resolves":{"id":"original","row":2},"rows":[
                {"row":2,"applied":true,"shipmentId":"s","actual":0,"tracking":"00000002"}]}]});
        assert!(project_with(&state,&j,&["correction".into()],|_,_|Err("missing".into())).is_err());
        let rows=project_with(&state,&j,&["correction".into()],|original,row|{
            assert_eq!(original["id"],"original");assert_eq!(row["row"],2);Ok(vec!["".into();17])
        }).unwrap();
        assert_eq!(rows[0][16],"00000002");assert_eq!(rows[0][6],"0");
    }
    #[test]
    fn legacy_date_uses_korea_midnight_and_leap_year(){
        assert_eq!(reply_date(&json!({"at":"2026-11-27T15:00:00Z"})).as_deref(),Some("2026-11-28"));
        assert_eq!(reply_date(&json!({"at":"2026-12-31T15:00:00Z"})).as_deref(),Some("2027-01-01"));
        assert!(target(&root(),"2028-02-29","STORE").is_ok());
        assert!(target(&root(),"2026-02-29","STORE").is_err());
        assert!(target(Path::new("relative"),"2026-11-27","STORE").is_err());
        assert!(target(&root(),"2026-11-27","PRODUCT").is_err());
    }
    #[test]
    fn restore_reopens_generation_and_preserves_business_revision(){
        let c=Connection::open_in_memory().unwrap();let root=root();
        metadata(&c,&json!({"revision":7,"exports":[export(&root,"2026-11-27","STORE",1)]}));
        recover_history(&c,Some(&root)).unwrap();
        let old=jobs(&c,0,true).unwrap().remove(0);
        mark_saved(&c,&old,"old").unwrap();
        invalidate_after_restore(&c,20).unwrap();
        let now=current(&c,&old).unwrap().unwrap();
        assert_eq!((now.desired,now.saved,now.hash),(20,0,None));
        assert_eq!(read_history(&c,None).unwrap()["revision"],7);
    }
    #[test]
    fn retry_delays_cap_and_new_work_resets_wait(){
        let c=Connection::open_in_memory().unwrap();
        let path=target(&root(),"2026-11-27","STORE").unwrap();let p=path.to_str().unwrap();
        enqueue(&c,"2026-11-27","STORE",p,1).unwrap();
        let mut now=100;
        for delay in [5,15,30,60,60]{
            let j=jobs(&c,now,false).unwrap().remove(0);
            mark_failed(&c,&j,now,"locked").unwrap();
            assert!(jobs(&c,now+delay-1,false).unwrap().is_empty());
            now+=delay;
        }
        let j=jobs(&c,now,false).unwrap().remove(0);
        mark_failed(&c,&j,now,"locked").unwrap();enqueue(&c,"2026-11-27","STORE",p,2).unwrap();
        let next=jobs(&c,now,false).unwrap().remove(0);
        assert_eq!((next.desired,next.retries),(2,0));
    }
    #[test]
    fn missing_export_rows_never_generates_silent_empty_workbook(){
        let c=Connection::open_in_memory().unwrap();let root=root();
        let mut e=export(&root,"2026-11-27","STORE",1);e.as_object_mut().unwrap().remove("outputRows");
        metadata(&c,&json!({"revision":1,"exports":[e]}));
        recover_history(&c,Some(&root)).unwrap();
        let j=jobs(&c,0,true).unwrap().remove(0);
        assert!(daily_rows(&c,&j,|_,_|panic!("no reply")).is_err());
    }
    #[test]
    fn latest_reply_wins_without_approval_time_reversal(){
        let p=target(&root(),"2026-11-27","REPLY").unwrap();
        let j=Job{date:"2026-11-27".into(),origin:"REPLY".into(),path:p.to_string_lossy().into_owned(),desired:3,saved:0,retries:0,hash:None};
        let record=|id:&str,tracking:&str|json!({"id":id,"businessDate":j.date,"aggregateTargetPath":j.path,
            "rows":[{"shipmentId":"s","applied":true,"tracking":tracking,"actual":3000,"aggregateValues":vec!["";17]}]});
        let mut correction=record("c","00000001");correction["resolves"]=json!({"id":"a"});
        let state=json!({"factoryReplies":[record("a","00000001"),record("b","00000002"),correction]});
        let result=project(&state,&j).unwrap();assert_eq!(result.len(),1);assert_eq!(result[0][16],"00000002");
    }
}
