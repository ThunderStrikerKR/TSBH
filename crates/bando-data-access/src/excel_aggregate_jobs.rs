//! Durable, derived Excel archive jobs. Never changes the business revision.
use rusqlite::{params,Connection,OptionalExtension};
use serde_json::Value;
use std::{collections::{HashMap,HashSet},path::{Path,PathBuf}};
use chrono::NaiveDate;

fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn s<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn rows<'a>(v:&'a Value,k:&str)->&'a [Value]{v[k].as_array().map(Vec::as_slice).unwrap_or(&[])}
pub fn initialize(conn:&Connection)->Result<(),String>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS excel_aggregate_jobs(
        business_date TEXT NOT NULL, origin TEXT NOT NULL CHECK(origin IN ('FACTORY','STORE','REPLY')),
        target_path TEXT NOT NULL, desired_revision INTEGER NOT NULL CHECK(desired_revision>=0),
        saved_revision INTEGER NOT NULL DEFAULT 0 CHECK(saved_revision>=0),
        retry_count INTEGER NOT NULL DEFAULT 0 CHECK(retry_count>=0), next_retry_at INTEGER,
        last_error TEXT, file_hash TEXT, CHECK(saved_revision<=desired_revision),
        PRIMARY KEY(business_date,origin,target_path));
        CREATE INDEX IF NOT EXISTS idx_excel_aggregate_pending ON excel_aggregate_jobs(next_retry_at)
        WHERE desired_revision>saved_revision;").map_err(err)
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
    let root=Path::new(path).ancestors().nth(5).ok_or("취합 경로 오류")?;
    if target(root,date,origin)?!=Path::new(path){return Err("취합 경로 불일치".into())}
    initialize(conn)?;
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
    let mut stmt=conn.prepare("SELECT business_date,origin,target_path,desired_revision,saved_revision,retry_count,file_hash
        FROM excel_aggregate_jobs WHERE ?1 OR (desired_revision>saved_revision AND COALESCE(next_retry_at,0)<=?2)
        ORDER BY business_date,origin,target_path").map_err(err)?;
    let iter=stmt.query_map(params![all,now],|r|Ok(Job{date:r.get(0)?,origin:r.get(1)?,path:r.get(2)?,
        desired:r.get(3)?,saved:r.get(4)?,retries:r.get(5)?,hash:r.get(6)?})).map_err(err)?;
    iter.collect::<Result<Vec<_>,_>>().map_err(err)
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
        WHERE business_date=?1 AND origin=?2 AND target_path=?3 AND desired_revision=?4",
        params![j.date,j.origin,j.path,j.desired,now+delay,error]).map_err(err)?;
    Ok(())
}
pub fn invalidate(conn:&Connection,j:&Job)->Result<(),String>{
    conn.execute("UPDATE excel_aggregate_jobs SET saved_revision=0,next_retry_at=NULL,file_hash=NULL
        WHERE business_date=?1 AND origin=?2 AND target_path=?3 AND saved_revision=?4 AND desired_revision=?5",
        params![j.date,j.origin,j.path,j.saved,j.desired]).map_err(err)?;
    Ok(())
}
/// Read only the physical metadata envelope, never hydrate order/catalog tables.
pub fn daily_rows(conn:&Connection,j:&Job)->Result<Vec<Vec<String>>,String>{
    let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).map_err(err)?;
    let state=crate::decode_physical_envelope(conn,&raw)?;
    project(&state,j)
}
fn values(v:&Value,n:usize)->Result<Vec<String>,String>{
    let arr=v.as_array().ok_or("취합 출력값 누락")?;
    if arr.len()!=n{return Err("취합 출력 열 수 오류".into())}
    arr.iter().map(|v|v.as_str().map(str::to_owned).ok_or_else(||"취합 출력값 형식 오류".into())).collect()
}
fn project(state:&Value,j:&Job)->Result<Vec<Vec<String>>,String>{
    let mut out=Vec::new();
    if j.origin!="REPLY"{
        let mut seen=HashSet::new();
        for e in rows(state,"exports"){
            let p=Path::new(s(e,"savedPath"));
            if p.parent()!=Path::new(&j.path).parent(){continue}
            let Ok((date,t))=export_target(p,&j.origin)else{continue};
            if date!=j.date||t!=Path::new(&j.path)||!seen.insert(s(e,"id")){continue}
            for row in rows(e,"outputRows"){out.push(values(&row["values"],16)?);}
        }
    }else{
        let history=rows(state,"factoryReplies");
        let positions:HashMap<&str,usize>=history.iter().enumerate().map(|(i,r)|(s(r,"id"),i)).collect();
        let mut selected:HashMap<&str,((usize,usize),usize,Vec<String>)>=HashMap::new();
        for (i,r) in history.iter().enumerate(){
            if s(r,"aggregateTargetPath")!=j.path||s(r,"businessDate")!=j.date{continue}
            let sequence=positions.get(s(&r["resolves"],"id")).copied().unwrap_or(i);
            for (row_index,row) in rows(r,"rows").iter().enumerate(){
                let shipment=s(row,"shipmentId");
                if row["applied"]!=true||shipment.is_empty(){continue}
                let mut v=if row.get("aggregateValues").is_some(){values(&row["aggregateValues"],17)?}else{
                    let mut v=vec![String::new();17];v[0]=s(row,"name").into();v[2]=s(row,"address").into();v[8]=s(row,"goods").into();
                    for (i,c) in rows(row,"contacts").iter().take(2).enumerate(){v[3+i]=c.as_str().unwrap_or("").to_owned();}v
                };
                v[6]=row["actual"].as_i64().ok_or("회신 운임 누락")?.to_string();
                v[16]=s(row,"tracking").to_owned();
                match selected.get_mut(shipment){
                    Some((_,previous,old)) if sequence>=*previous=>{*previous=sequence;*old=v;}
                    Some(_)=>{},
                    None=>{selected.insert(shipment,((i,row_index),sequence,v));}
                }
            }
        }
        let mut ordered:Vec<_>=selected.into_iter().collect();
        ordered.sort_by(|(a,(i,_,_)),(b,(k,_,_))|i.cmp(k).then_with(||a.cmp(b)));
        out=ordered.into_iter().map(|(_,(_,_,v))|v).collect();
    }
    Ok(out)
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn root()->PathBuf{std::env::temp_dir().join("aggregate-fixture")}
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
