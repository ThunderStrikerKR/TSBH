//! Native factory reply inbox: scan only the saved configured directory,
//! preserve original bytes, and derive every mutation inside the native rules.
use crate::{business_state,excel_service,gateway::rules,native_workbooks};
use chrono::{FixedOffset,Utc};
use eframe::egui::Context;
use rusqlite::{Connection,OpenFlags};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{HashMap,HashSet},fs,io::{Read,Write},path::{Path,PathBuf},
    sync::mpsc::{self,Receiver},time::{Duration,Instant,SystemTime}};
use uuid::Uuid;

const MAX_BYTES:u64=10*1024*1024;
const INTERVAL:Duration=Duration::from_secs(5);
fn id(bytes:&[u8])->String{format!("factory-reply-{:x}",Sha256::digest(bytes))}
fn text<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn valid_name(n:&str)->bool{!n.is_empty()&&n.len()<=240&&!n.starts_with("~$")&&
    !n.contains('/')&&!n.contains('\\')&&!n.contains('\0')&&n.to_ascii_lowercase().ends_with(".xlsx")}
fn open_db()->Result<(Connection,PathBuf),String>{
    if !excel_service::writable(){return Err("공장 회신 저장 권한 없음".into())}
    let path=business_state::authorized_business_write_path()?;
    let base=path.parent().ok_or("업무 DB 경로 오류")?.to_path_buf();
    let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;").map_err(|e|e.to_string())?;
    let v:i64=conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if v!=2{return Err("지원하지 않는 업무 DB 버전".into())}
    Ok((conn,base))
}
fn directory(raw:&str)->Result<Option<PathBuf>,String>{
    if raw.trim().is_empty(){return Ok(None)}
    let path=Path::new(raw.trim());
    if !path.is_absolute(){return Err("공장 회신 폴더는 절대 경로여야 합니다.".into())}
    let path=path.canonicalize().map_err(|e|e.to_string())?;
    if !path.is_dir(){return Err("공장 회신 수신 폴더가 아닙니다.".into())}
    Ok(Some(path))
}
fn configured(conn:&Connection)->Result<Option<PathBuf>,String>{
    let raw:Option<String>=conn.query_row("SELECT json_extract(payload,'$.settings.factoryReplyDirectory') FROM app_state WHERE id=1",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    directory(raw.as_deref().unwrap_or(""))
}
fn stamp(path:&Path)->Result<(u64,SystemTime),String>{
    let m=fs::symlink_metadata(path).map_err(|e|e.to_string())?;
    if !m.file_type().is_file()||m.len()==0||m.len()>MAX_BYTES{return Err("회신 파일 크기 또는 형식 오류".into())}
    Ok((m.len(),m.modified().map_err(|e|e.to_string())?))
}
fn read_bounded(path:&Path)->Result<Vec<u8>,String>{
    let mut bytes=Vec::new();
    fs::File::open(path).map_err(|e|e.to_string())?.take(MAX_BYTES+1)
        .read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.is_empty()||bytes.len() as u64>MAX_BYTES{return Err("10MB 이하 회신 파일만 허용".into())}
    Ok(bytes)
}
fn preserve(base:&Path,bytes:&[u8])->Result<PathBuf,String>{
    let base=base.canonicalize().map_err(|e|e.to_string())?;
    let dir=base.join("factory-replies");
    fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    let dir=dir.canonicalize().map_err(|e|e.to_string())?;
    if !dir.starts_with(&base){return Err("회신 원본 보관 경로 오류".into())}
    let path=dir.join(format!("{}.xlsx",&id(bytes)["factory-reply-".len()..]));
    match fs::OpenOptions::new().create_new(true).write(true).open(&path){
        Ok(mut f)=>{
            if let Err(e)=f.write_all(bytes).and_then(|_|f.sync_all()){
                drop(f);let _=fs::remove_file(&path);return Err(e.to_string())
            }
        },
        Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{
            if fs::symlink_metadata(&path).map_err(|e|e.to_string())?.file_type().is_symlink()||
                read_bounded(&path)?!=bytes{return Err("기존 회신 원본과 일치하지 않습니다.".into())}
        },
        Err(e)=>return Err(e.to_string()),
    }
    Ok(path)
}
fn today()->String{Utc::now().with_timezone(&FixedOffset::east_opt(9*3600).unwrap()).format("%Y-%m-%d").to_string()}
fn ids(n:usize)->Vec<String>{(0..n).map(|_|Uuid::new_v4().to_string()).collect()}
fn snapshot(conn:&mut Connection)->Result<Value,String>{
    let tx=conn.transaction().map_err(|e|e.to_string())?;
    let value=bando_data_access::read_v2_snapshot(&tx)?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(value)
}
/// Only called after a stable file observation. The bytes are verified again
/// before commit; no UI-provided candidate snapshot is accepted.
fn import(conn:&mut Connection,base:&Path,dir:&Path,path:&Path,bytes:&[u8])->Result<Value,String>{
    let filename=path.file_name().and_then(|n|n.to_str()).filter(|n|valid_name(n)).ok_or("회신 파일명 오류")?;
    if path.canonicalize().map_err(|e|e.to_string())?.parent()!=Some(dir){
        return Err("지정한 수신 폴더 외부의 회신 파일은 허용하지 않습니다.".into());
    }
    let source=native_workbooks::read_factory_reply(filename,bytes)?;
    let file_id=text(&source,"id");
    let current=snapshot(conn)?;
    if rules::execute("factory_reply_duplicate",json!({"state":current,"id":file_id}))?["duplicate"]==true{
        return Ok(json!({"duplicate":true}))
    }
    if directory(text(&current["settings"],"factoryReplyDirectory"))?.as_deref()!=Some(dir){
        return Err("회신 수신 폴더 설정이 변경됐습니다.".into())
    }
    let rev=current["revision"].as_u64().ok_or("DB 리비전 누락")?;
    let at=Utc::now().to_rfc3339();
    let business_date=today();
    let n=source["sourceRows"].as_array().map(Vec::len).unwrap_or(0);
    let freight_ids=ids(n);let reply_ids=ids(n);
    let mut analyze=source.clone();
    analyze["state"]=current.clone();
    analyze["businessDate"]=json!(business_date);
    analyze["at"]=json!(at);
    analyze["actor"]=json!(text(&current["settings"],"operatorName"));
    let preview=rules::execute("analyze_factory_reply",analyze)?;
    if read_bounded(path)?!=bytes{return Err("회신 파일이 변경됐습니다.".into())}
    let archived=preserve(base,bytes)?;
    let result=bando_data_access::update_v2_snapshot(conn,rev,true,|before|{
        if directory(text(&before["settings"],"factoryReplyDirectory"))?.as_deref()!=Some(dir){
            return Err("회신 폴더 설정 변경".into())
        }
        let mut data=source.clone();
        data["state"]=before.clone();data["businessDate"]=json!(business_date);
        data["at"]=json!(at);data["actor"]=json!(text(&before["settings"],"operatorName"));
        let fresh=rules::execute("analyze_factory_reply",data)?;
        if fresh!=preview{return Err("회신 연결 결과가 달라졌습니다.".into())}
        let mut next=rules::execute("apply_factory_reply",json!({"state":before,"preview":fresh,
            "freightAt":at,"freightEventIds":freight_ids,"replyEventIds":reply_ids}))?;
        let record=next["next"]["factoryReplies"].as_array_mut().and_then(|r|r.last_mut()).ok_or("회신 이력 누락")?;
        record["archivedPath"]=json!(archived.to_string_lossy());
        next.as_object_mut().and_then(|m|m.remove("next")).ok_or("회신 저장 결과 누락".into())
    })?;
    let warning=excel_service::archive_reply_workbook(&result,bytes).err();
    Ok(json!({"duplicate":false,"applied":preview["applied"],"held":preview["held"],"archiveWarning":warning}))
}
pub fn approve(review:Value)->Result<Value,String>{
    let (mut conn,base)=open_db()?;
    let revision=review["revision"].as_u64().ok_or("검토 리비전 누락")?;
    let reply_id=text(&review,"id");
    let original_id=reply_id.strip_prefix("factory-reply-").filter(|v|v.len()==64&&v.bytes().all(|c|c.is_ascii_hexdigit()))
        .ok_or("원본 회신 ID 오류")?;
    let dir=base.canonicalize().map_err(|e|e.to_string())?.join("factory-replies");
    let path=dir.join(format!("{original_id}.xlsx"));
    if id(&read_bounded(&path)?)!=reply_id{return Err("보관된 회신 원본이 일치하지 않습니다.".into())}
    let correction=format!("factory-correction-{}",Uuid::new_v4());
    let at=Utc::now().to_rfc3339();
    bando_data_access::update_v2_snapshot(&mut conn,revision,true,|before|{
        let mut payload=review.clone();payload["state"]=before.clone();
        payload["correctionId"]=json!(correction);payload["businessDate"]=json!(today());
        payload["freightAt"]=json!(at);payload["freightEventIds"]=json!(ids(1));
        payload["replyEventIds"]=json!(ids(1));
        rules::execute("approve_factory_conflict",payload)
    })
}
#[derive(Clone)]
struct Seen{stamp:(u64,SystemTime),observations:u8,done:bool}
impl Seen{
    fn observe(&mut self,current:(u64,SystemTime))->bool{
        if self.stamp!=current{*self=Self{stamp:current,observations:0,done:false};}
        self.observations=self.observations.saturating_add(1);
        self.observations>=3&&!self.done
    }
}
#[derive(Default)]
struct Scan{seen:HashMap<PathBuf,Seen>,directory:Option<PathBuf>}
#[derive(Default)]
pub struct Outcome{pub files:usize,pub applied:usize,pub held:usize,pub error:Option<String>}
fn scan(cache:&mut Scan)->Result<Outcome,String>{
    let (mut conn,base)=open_db()?;
    let dir=configured(&conn)?;
    if cache.directory!=dir{cache.seen.clear();cache.directory=dir.clone();}
    let Some(dir)=dir else{return Ok(Outcome::default())};
    let mut outcome=Outcome::default();let mut active=HashSet::new();let mut attempted=0;
    let mut entries=fs::read_dir(&dir).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    entries.sort_by_key(|e|e.file_name());
    for entry in entries{
        let path=entry.path();let name=entry.file_name();
        if !name.to_str().is_some_and(valid_name){continue}
        let Ok(current)=stamp(&path)else{continue};
        active.insert(path.clone());
        let seen=cache.seen.entry(path.clone()).or_insert(Seen{stamp:current,observations:0,done:false});
        if !seen.observe(current)||attempted>=8{continue}
        attempted+=1;
        let bytes=match read_bounded(&path){Ok(b)=>b,Err(e)=>{outcome.error=Some(e);continue}};
        if stamp(&path).ok()!=Some(current)||bytes.len() as u64!=current.0{continue}
        // Invalid binary files are reported once until the file changes.
        if let Err(e)=native_workbooks::read_factory_reply(name.to_str().unwrap_or(""),&bytes){
            seen.done=true;outcome.error=Some(format!("회신 파일 검토 필요: {e}"));continue
        }
        match import(&mut conn,&base,&dir,&path,&bytes){
            Ok(result)=>{
                seen.done=true;
                if result["duplicate"]!=true{
                    outcome.files+=1;outcome.applied+=result["applied"].as_u64().unwrap_or(0) as usize;
                    outcome.held+=result["held"].as_u64().unwrap_or(0) as usize;
                    if let Some(w)=result["archiveWarning"].as_str(){outcome.error=Some(format!("회신 DB 저장 완료, 보관함 복사 실패: {w}"));}
                }
            },
            Err(e)=>outcome.error=Some(e),
        }
    }
    cache.seen.retain(|p,_|active.contains(p));Ok(outcome)
}
#[derive(Default)]
pub struct Inbox{cache:Option<Scan>,receiver:Option<Receiver<(Scan,Result<Outcome,String>)>>,
    last:Option<Instant>,last_error:Option<String>}
impl Inbox{
    pub fn poll(&mut self,ctx:&Context)->Option<Outcome>{
        let mut result=None;
        if let Some(rx)=self.receiver.as_ref(){
            match rx.try_recv(){
                Ok((cache,answer))=>{
                    self.receiver=None;self.cache=Some(cache);
                    let mut value=answer.unwrap_or_else(|e|Outcome{error:Some(e),..Default::default()});
                    if value.error==self.last_error{value.error=None;}else{self.last_error=value.error.clone();}
                    if value.files>0||value.error.is_some(){result=Some(value)}
                },
                Err(mpsc::TryRecvError::Disconnected)=>{self.receiver=None;self.cache=None;}
                _=>{}
            }
        }
        if self.receiver.is_none()&&self.last.is_none_or(|t|t.elapsed()>=INTERVAL)&&
            !cfg!(test)&&std::env::var("BANDO_EGUI_ENABLE_EXCEL_WRITE").ok().as_deref()==Some("1"){
            self.last=Some(Instant::now());
            let mut cache=self.cache.take().unwrap_or_default();
            let (tx,rx)=mpsc::channel();self.receiver=Some(rx);let ctx=ctx.clone();
            std::thread::spawn(move||{let answer=scan(&mut cache);let _=tx.send((cache,answer));ctx.request_repaint();});
        }
        if !cfg!(test)&&std::env::var("BANDO_EGUI_ENABLE_EXCEL_WRITE").ok().as_deref()==Some("1"){
            ctx.request_repaint_after(INTERVAL);
        }
        result
    }
}

#[cfg(test)]
mod tests{
    use super::*;

    struct Temp(PathBuf);
    impl Temp{
        fn new()->Self{
            let path=std::env::temp_dir().join(format!("bando-factory-inbox-{}",Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp{
        fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}
    }

    #[test]
    fn stable_observations_reset_on_change_and_skip_completed_files(){
        let first=(10,std::time::UNIX_EPOCH);
        let second=(10,std::time::UNIX_EPOCH+Duration::from_secs(1));
        let third=(11,second.1);
        let mut seen=Seen{stamp:first,observations:0,done:false};
        assert!(!seen.observe(first));
        assert!(!seen.observe(first));
        assert!(!seen.observe(second));
        assert!(!seen.observe(second));
        assert!(seen.observe(second));
        // A failed attempt stays eligible; successful imports stop retrying.
        assert!(seen.observe(second));
        seen.done=true;
        assert!(!seen.observe(second));
        assert!(!seen.observe(third));
        assert!(!seen.observe(third));
        assert!(seen.observe(third));
    }

    #[test]
    fn original_archive_is_content_addressed_and_never_overwrites_mismatch(){
        let base=Temp::new();
        let mut book=rust_xlsxwriter::Workbook::new();
        let sheet=book.add_worksheet();
        sheet.write_string(0,0,"수하인명").unwrap();
        sheet.write_string(0,16,"운송장번호").unwrap();
        sheet.write_string(1,0,"합성 수취인").unwrap();
        sheet.write_string(1,16,"001234567890").unwrap();
        let bytes=book.save_to_buffer().unwrap();
        let original=preserve(&base.0,&bytes).unwrap();
        assert_eq!(read_bounded(&original).unwrap(),bytes);
        assert_eq!(preserve(&base.0,&bytes).unwrap(),original);
        assert_eq!(fs::read_dir(original.parent().unwrap()).unwrap().count(),1);
        fs::write(&original,b"corrupted original").unwrap();
        assert!(preserve(&base.0,&bytes).is_err());
        assert_eq!(fs::read(&original).unwrap(),b"corrupted original");
    }

    #[test]
    fn inbox_ignores_temporary_names_and_rejects_empty_or_oversized_files(){
        assert!(valid_name("회신.XLSX"));
        for name in ["~$회신.xlsx","../회신.xlsx","a\\b.xlsx","reply.xls","reply.tmp",""]{
            assert!(!valid_name(name));
        }
        let base=Temp::new();
        let path=base.0.join("reply.xlsx");
        let file=fs::File::create(&path).unwrap();
        assert!(stamp(&path).is_err());
        assert!(read_bounded(&path).is_err());
        file.set_len(MAX_BYTES+1).unwrap();
        drop(file);
        assert!(stamp(&path).is_err());
        assert!(read_bounded(&path).is_err());
    }
}
