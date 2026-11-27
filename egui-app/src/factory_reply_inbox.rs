//! Native factory reply inbox: scan only the saved configured directory,
//! preserve original bytes, and derive every mutation inside the native rules.
use crate::{business_state,excel_service,native_workbooks};
#[cfg(test)]
use crate::gateway::rules;
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
    !n.strip_suffix("(취합).xlsx").is_some_and(|stem|stem.len()==8&&stem.bytes().all(|c|c.is_ascii_digit()))&&
    !n.contains('/')&&!n.contains('\\')&&!n.contains('\0')&&n.to_ascii_lowercase().ends_with(".xlsx")}
pub(crate) fn open_db()->Result<(Connection,PathBuf),String>{
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
    if business_state::test_launch(){return business_state::test_directory("factory-inbox").map(Some);}
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
#[cfg(test)]
fn snapshot(conn:&mut Connection)->Result<Value,String>{
    let tx=conn.transaction().map_err(|e|e.to_string())?;
    let value=bando_data_access::read_v2_snapshot(&tx)?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(value)
}
/// The v2 envelope owns revision and reply history. Do not hydrate products,
/// clients or orders merely to identify a duplicate inbox file.
fn import_metadata(conn:&Connection,file_id:&str)->Result<(u64,bool),String>{
    let (revision,duplicate):(Option<i64>,bool)=conn.query_row(
        "SELECT CASE WHEN json_type(payload,'$.revision')='integer'
                     THEN json_extract(payload,'$.revision') END,
            EXISTS(SELECT 1 FROM json_each(CASE
                WHEN json_type(app_state.payload,'$.factoryReplies')='array'
                THEN json_extract(app_state.payload,'$.factoryReplies') ELSE '[]' END) AS entry
                WHERE CASE WHEN entry.type='object'
                    THEN json_type(entry.value,'$.id')='text'
                         AND json_extract(entry.value,'$.id')=?1 ELSE 0 END)
         FROM app_state WHERE id=1",
        [file_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let revision=revision.and_then(|v|u64::try_from(v).ok()).ok_or("DB 리비전 누락")?;
    Ok((revision,duplicate))
}
fn check_source_path(dir:&Path,path:&Path)->Result<(),String>{
    path.file_name().and_then(|n|n.to_str()).filter(|n|valid_name(n)).ok_or("회신 파일명 오류")?;
    if path.canonicalize().map_err(|e|e.to_string())?.parent()!=Some(dir){
        return Err("지정한 수신 폴더 외부의 회신 파일은 허용하지 않습니다.".into());
    }
    Ok(())
}
/// Check durable content identity before opening or decompressing the XLSX.
/// Carry the same revision into the authoritative transaction; a concurrent
/// writer forces a retry rather than accepting stale metadata.
fn preflight(conn:&Connection,dir:&Path,path:&Path,file_id:&str)->Result<(u64,bool),String>{
    check_source_path(dir,path)?;
    import_metadata(conn,file_id)
}
/// Stable bytes have already been parsed by the scanner. The only hydrated
/// before-image is the CAS transaction's authoritative state.
fn aggregate_target(conn:&Connection,date:&str)->Result<Option<String>,String>{
    let state=bando_data_access::read_v2_export_metadata(conn)?;
    let raw=text(&state["settings"],"excelDirectory").trim();
    if raw.is_empty(){return Ok(None)}
    let path=PathBuf::from(raw);
    if !path.is_absolute(){return Ok(None)}
    let root=path.canonicalize().unwrap_or(path);
    let target=bando_data_access::excel_aggregate_jobs::target(&root,date,"REPLY")?;
    Ok(Some(target.to_str().ok_or("취합 경로 인코딩 오류")?.to_owned()))
}
fn import(conn:&mut Connection,base:&Path,dir:&Path,path:&Path,bytes:&[u8],rev:u64,mut source:Value)->Result<Value,String>{
    check_source_path(dir,path)?;
    if configured(conn)?.as_deref()!=Some(dir){
        return Err("회신 수신 폴더 설정이 변경됐습니다.".into())
    }
    let at=Utc::now().to_rfc3339();
    let n=source["sourceRows"].as_array().map(Vec::len).unwrap_or(0);
    let day=today();
    source["businessDate"]=json!(day);
    if let Some(path)=aggregate_target(conn,&day)?{source["aggregateTargetPath"]=json!(path);}
    source["at"]=json!(at);
    source["freightAt"]=json!(at);
    source["freightEventIds"]=json!(ids(n));
    source["replyEventIds"]=json!(ids(n));
    let result=bando_data_access::update_v2_factory_reply_snapshot(conn,rev,&source,|before|{
        if directory(text(&before["settings"],"factoryReplyDirectory"))?.as_deref()!=Some(dir){
            return Err("회신 폴더 설정 변경".into())
        }
        // Only a semantically valid candidate is archived. Check the live
        // source bytes again before preserving and committing the candidate.
        if read_bounded(path)?!=bytes{return Err("회신 파일이 변경됐습니다.".into())}
        Ok(preserve(base,bytes)?.to_string_lossy().into_owned())
    })?;
    let warning=excel_service::archive_reply_workbook(&result["next"],bytes).err();
    Ok(json!({"duplicate":false,"applied":result["applied"],"held":result["held"],"archiveWarning":warning}))
}
pub fn approve(review:Value)->Result<Value,String>{
    let (mut conn,base)=open_db()?;
    approve_with_db(&mut conn,&base,review)
}
// The authorized live entry point and isolated tests share this transaction.
fn approve_with_db(conn:&mut Connection,base:&Path,review:Value)->Result<Value,String>{
    let revision=review["revision"].as_u64().ok_or("검토 리비전 누락")?;
    let reply_id=text(&review,"id");
    let original_id=reply_id.strip_prefix("factory-reply-").filter(|v|v.len()==64&&v.bytes().all(|c|c.is_ascii_hexdigit()))
        .ok_or("원본 회신 ID 오류")?;
    let dir=base.canonicalize().map_err(|e|e.to_string())?.join("factory-replies");
    let path=dir.join(format!("{original_id}.xlsx"));
    if id(&read_bounded(&path)?)!=reply_id{return Err("보관된 회신 원본이 일치하지 않습니다.".into())}
    let correction=format!("factory-correction-{}",Uuid::new_v4());
    let at=Utc::now().to_rfc3339();
    let mut payload=review;
    let day=today();
    payload["correctionId"]=json!(correction);payload["businessDate"]=json!(day);
    if let Some(path)=aggregate_target(conn,&day)?{payload["aggregateTargetPath"]=json!(path);}
    payload["freightAt"]=json!(at);payload["freightEventIds"]=json!(ids(1));
    payload["replyEventIds"]=json!(ids(1));
    bando_data_access::update_v2_factory_correction_snapshot(conn,revision,&payload)
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
    scan_with_db(cache,&mut conn,&base)
}
// The live caller retains authorization; tests supply an explicitly isolated DB.
fn scan_with_db(cache:&mut Scan,conn:&mut Connection,base:&Path)->Result<Outcome,String>{
    let dir=configured(conn)?;
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
        let file_id=id(&bytes);
        let rev=match preflight(conn,&dir,&path,&file_id){
            Ok((_,true))=>{seen.done=true;continue},
            Ok((rev,false))=>rev,
            Err(e)=>{outcome.error=Some(e);continue}
        };
        // Invalid binary files are reported once until the file changes.
        let source=match native_workbooks::read_factory_reply(name.to_str().unwrap_or(""),&bytes){
            Ok(source)=>source,
            Err(e)=>{
                let detail=format!("회신 파일 검토 필요: {e}");
                if let Err(save)=bando_data_access::notification_store::record_file_error(conn,&file_id,
                    name.to_str().unwrap_or(""),&Utc::now().to_rfc3339(),&detail){
                    outcome.error=Some(format!("{detail} / 알림 저장 실패: {save}"));continue;
                }
                seen.done=true;outcome.error=Some(detail);continue
            }
        };
        if text(&source,"id")!=file_id{
            outcome.error=Some("회신 원본 ID 불일치".into());continue
        }
        match import(conn,base,&dir,&path,&bytes,rev,source){
            Ok(result)=>{
                seen.done=true;
                if result["duplicate"]!=true{
                    outcome.files+=1;outcome.applied+=result["applied"].as_u64().unwrap_or(0) as usize;
                    outcome.held+=result["held"].as_u64().unwrap_or(0) as usize;
                    if let Some(w)=result["archiveWarning"].as_str(){outcome.error=Some(format!("회신 DB 저장 완료, 보관함 복사 실패: {w}"));}
                }
            },
            Err(e)=>{
                if let Err(save)=bando_data_access::notification_store::record_file_error(conn,&file_id,
                    name.to_str().unwrap_or(""),&Utc::now().to_rfc3339(),&e){
                    outcome.error=Some(format!("{e} / 알림 저장 실패: {save}"));
                }else{outcome.error=Some(e);}
            },
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

    // Isolated v2 fixture. Reuse the test workspace schema and seed both
    // relational business rows and the production lookup indexes.
    fn synthetic_db(base:&Path)->(Connection,PathBuf){
        synthetic_db_with_export(base,false)
    }
    fn synthetic_db_with_export(base:&Path,export_ready:bool)->(Connection,PathBuf){
        let inbox=base.join("inbox");
        let archive=base.join("excel");
        fs::create_dir(&inbox).unwrap();
        fs::create_dir(&archive).unwrap();
        let mut conn=Connection::open(base.join("synthetic.sqlite")).unwrap();
        conn.execute_batch(include_str!("test_workspace_schema.sql")).unwrap();
        let recipient=json!({"id":"r1","name":"합성 수취인","address":"서울 종로구 10",
            "phone":"010-0000-0000","mobile":"","actualFreight":null,
            "items":[{"id":"i1","origin":"FACTORY","quantity":1,"unitPrice":10000}]});
        let mut state=json!({"revision":7,"products":[],"clients":[],"aliases":[],"queue":[],"exports":[],
            "factoryReplies":[],"settings":{"accounts":[],"operatorName":"합성 직원",
                "factoryReplyDirectory":inbox.to_string_lossy(),"excelDirectory":archive.to_string_lossy()},
            "orders":[{"id":"o1","clientId":"c1","raw":"합성 원문","recipients":[recipient.clone()]}],
            "workflow":{"sources":[{"id":"src1","orderId":"o1"}],
                "groups":[{"id":"g1","sourceId":"src1","dealerId":"c1","notices":[]}],
                "payments":[{"id":"pay1","dealerId":"c1","amount":13000,
                    "paidAt":"2026-11-13T09:00:00Z","allocations":[{"id":"a1","shipmentId":"s1","amount":13000}]}],
                "confirmations":[{"id":"conf1","groupId":"g1","dealerId":"c1",
                    "shipmentIds":["s1"],"amount":13000,"amounts":{"s1":13000},
                    "allocations":[{"id":"ca1","shipmentId":"s1","amount":13000}]}],
                "deliveries":[],"changeRequests":[],
                "shipments":[{"id":"s1","orderId":"o1","recipientId":"r1","sourceId":"src1","groupId":"g1",
                    "step":4,"transferred":true,"charges":{"FACTORY":3000},"tracking":{},"actualFreight":{}}],
                "queue":[{"id":"q1","shipmentId":"s1","origin":"FACTORY","status":"EXPORTED",
                    "snapshot":{"recipient":recipient,"shipping":3000,"subtotal":10000,"total":13000}}],"events":[]}});
        if export_ready {
            state["products"]=json!([{"id":"p1","name":"합성 상품","code":"SYNTHETIC",
                "origin":"FACTORY","active":true,"variants":[{"id":"v1","color":"단일","wholesale":10000}]}]);
            let client=json!({"id":"c1","name":"합성 거래처","tier":"wholesale",
                "payment":"POSTPAID","address":"서울 종로구 20","phone":"02-000-0000",
                "shippingFeeExcluded":false,"overrides":{}});
            state["clients"]=json!([client]);
            state["orders"][0]["orderType"]=json!("AGENCY");
            state["orders"][0]["clientSnapshot"]=client.clone();
            state["orders"][0]["confirmedAt"]=json!("2026-11-13T09:00:00Z");
            state["orders"][0]["createdAt"]=json!("2026-11-13T09:00:00Z");
            let item=&mut state["orders"][0]["recipients"][0]["items"][0];
            item["name"]=json!("합성 상품");
            item["productId"]=json!("p1");
            item["variantId"]=json!("v1");
            item["color"]=json!("단일");
            let recipient=state["orders"][0]["recipients"][0].clone();
            let q=&mut state["workflow"]["queue"][0];
            q["status"]=json!("QUEUED");
            q["snapshot"]["recipient"]=recipient;
            q["snapshot"]["sender"]=client;
            q["snapshot"]["freightCode"]=json!("010");
            q["snapshot"]["vat"]=json!(0);
        }
        let tx=conn.transaction().unwrap();
        bando_data_access::store_v2_relational(&tx,&state).unwrap();
        // Mirror the index rows maintained by the production snapshot writer.
        for product in state["products"].as_array().unwrap(){
            tx.execute("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)",
                rusqlite::params![product["id"].as_str().unwrap(),
                    product["code"].as_str().unwrap(),product["name"].as_str().unwrap(),
                    product.to_string()]).unwrap();
        }
        for order in state["orders"].as_array().unwrap(){
            tx.execute("INSERT INTO order_index(id,client_id,created_at,confirmed_at,status) VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![order["id"].as_str().unwrap(),
                    order["clientId"].as_str().unwrap_or(""),
                    order["createdAt"].as_str().unwrap_or(""),order["confirmedAt"].as_str(),
                    order["status"].as_str().unwrap_or("REVIEW")]).unwrap();
        }
        for key in ["products","clients","orders"]{state.as_object_mut().unwrap().remove(key);}
        tx.execute("INSERT INTO app_state VALUES(1,?1)",[state.to_string()]).unwrap();
        tx.commit().unwrap();
        (conn,inbox)
    }
    fn reply_bytes(tracking:&str,freight:u32)->Vec<u8>{
        let mut book=rust_xlsxwriter::Workbook::new();
        let sheet=book.add_worksheet();
        sheet.write_string(0,0,"수하인명").unwrap();
        sheet.write_string(0,16,"운송장번호").unwrap();
        for (c,v) in [(0,"합성 수취인"),(2,"서울 종로구 10"),(3,"010-0000-0000"),(16,tracking)]{
            sheet.write_string(1,c,v).unwrap();
        }
        sheet.write_number(1,6,f64::from(freight)).unwrap();
        book.save_to_buffer().unwrap()
    }
    fn scan_ready(cache:&mut Scan,conn:&mut Connection,base:&Path)->Outcome{
        for _ in 0..2{
            let out=scan_with_db(cache,conn,base).unwrap();
            assert_eq!(out.files,0);
            assert!(out.error.is_none(),"{:?}",out.error);
        }
        scan_with_db(cache,conn,base).unwrap()
    }

    #[test]
    fn factory_binary_scan_persists_exact_waybill_freight_and_audit_across_restart(){
        let base=Temp::new();
        let (mut conn,inbox)=synthetic_db(&base.0);
        let before=snapshot(&mut conn).unwrap();
        let bytes=reply_bytes("001234567890",4000);
        let original=inbox.join("reply.xlsx");
        fs::write(&original,&bytes).unwrap();
        let result=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert!(result.error.is_none(),"{:?}",result.error);
        assert_eq!((result.files,result.applied,result.held),(1,1,0));
        drop(conn);
        let mut conn=Connection::open(base.0.join("synthetic.sqlite")).unwrap();
        let stored=snapshot(&mut conn).unwrap();
        assert_eq!(stored["revision"],8);
        let ship=&stored["workflow"]["shipments"][0];
        assert_eq!(ship["tracking"]["FACTORY"],"001234567890");
        assert_eq!(ship["actualFreight"]["FACTORY"],4000);
        assert_eq!(ship["charges"]["FACTORY"],3000);
        assert_eq!(stored["orders"],before["orders"]);
        assert_eq!(stored["workflow"]["queue"],before["workflow"]["queue"]);
        assert_eq!(stored["workflow"]["payments"],before["workflow"]["payments"]);
        assert_eq!(stored["workflow"]["confirmations"],before["workflow"]["confirmations"]);
        let events=stored["workflow"]["events"].as_array().unwrap();
        assert_eq!(events.len(),2);
        assert_eq!(events[0]["action"],"실제 출고 운임 입력");
        assert_eq!(events[1]["action"],"공장 회신 반영");
        assert_eq!(events[1]["after"]["fileId"],id(&bytes));
        let replies=stored["factoryReplies"].as_array().unwrap();
        assert_eq!(replies.len(),1);
        assert_eq!(replies[0]["id"],id(&bytes));
        assert_eq!(replies[0]["status"],"APPLIED");
        assert_eq!(fs::read(replies[0]["archivedPath"].as_str().unwrap()).unwrap(),bytes);
        assert_eq!(fs::read(&original).unwrap(),bytes);
        // Both a new file name and a fresh in-memory watcher must still
        // consult durable content identity rather than repeat the write.
        fs::write(inbox.join("renamed.xlsx"),&bytes).unwrap();
        let replay=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert!(replay.error.is_none(),"{:?}",replay.error);
        assert_eq!((replay.files,replay.applied,replay.held),(0,0,0));
        assert_eq!(snapshot(&mut conn).unwrap(),stored);
    }

    #[test]
    fn exported_xlsx_to_factory_inbox_and_settlement_roundtrip(){
        use calamine::{Data,Reader,Xlsx};
        let base=Temp::new();
        let (mut conn,inbox)=synthetic_db_with_export(&base.0,true);
        let initial=snapshot(&mut conn).unwrap();
        let output=crate::gateway::output::execute;
        let preview=output("prepare_factory_export",json!({"state":initial,"ids":["q1"]})).unwrap();
        let archive=base.0.join("excel").canonicalize().unwrap();
        let exported=excel_service::save_factory_test_export(&archive,&preview).unwrap();
        assert!(exported.is_file());
        let original=fs::read(&exported).unwrap();
        let mut book=Xlsx::new(std::io::Cursor::new(&original)).unwrap();
        let sheet=book.worksheet_range("로젠택배").unwrap();
        assert_eq!(sheet.width(),16);
        assert_eq!(sheet.height(),2);
        assert_eq!(sheet.get_value((1,7)),Some(&Data::String("010".into())));
        assert_eq!(sheet.get_value((1,3)),Some(&Data::String("010-0000-0000".into())));
        let finalized=output("finalize_export",json!({"state":initial,"preview":preview,
            "origin":"FACTORY","savedPath":exported.to_string_lossy(),
            "now":"2026-11-13T09:00:00Z","exportId":"export-roundtrip","eventId":"export-event"})).unwrap();
        let saved=bando_data_access::write_v2_snapshot(&mut conn,finalized["next"].clone(),7,true).unwrap();
        assert_eq!(saved["workflow"]["queue"][0]["status"],"EXPORTED");
        assert_eq!(saved["revision"],8);

        // Simulate a factory returning the actual exported row with G and Q filled.
        let mut reply=rust_xlsxwriter::Workbook::new();
        let dest=reply.add_worksheet();
        for r in 0..2_u32 {
            for c in 0..16_u32 {
                match sheet.get_value((r,c)){
                    Some(Data::String(value))=>{dest.write_string(r,c as u16,value).unwrap();},
                    None|Some(Data::Empty)=>{},
                    value=>panic!("unexpected non-text export cell {r}/{c}: {value:?}"),
                }
            }
        }
        dest.write_string(0,16,"운송장번호").unwrap();
        dest.write_number(1,6,4000.).unwrap();
        dest.write_string(1,16,"001234567890").unwrap();
        let bytes=reply.save_to_buffer().unwrap();
        fs::write(inbox.join("returned-export.xlsx"),&bytes).unwrap();
        let result=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert!(result.error.is_none(),"{:?}",result.error);
        assert_eq!((result.files,result.applied,result.held),(1,1,0));
        drop(conn);
        let mut conn=Connection::open(base.0.join("synthetic.sqlite")).unwrap();
        let stored=snapshot(&mut conn).unwrap();
        assert_eq!(stored["revision"],9);
        let ship=&stored["workflow"]["shipments"][0];
        assert_eq!(ship["id"],"s1");
        assert_eq!(ship["tracking"]["FACTORY"],"001234567890");
        assert_eq!(ship["charges"]["FACTORY"],3000);
        assert_eq!(ship["actualFreight"]["FACTORY"],4000);
        for field in ["orders","exports"]{
            assert_eq!(stored[field],saved[field]);
        }
        for field in ["payments","confirmations","queue"]{
            assert_eq!(stored["workflow"][field],saved["workflow"][field]);
        }
        let history=rules::execute("list_order_history",json!({"state":stored,"query":"","date":"2026-11-13"})).unwrap();
        assert_eq!(history[0]["shipmentIds"],json!(["s1"]));
        assert_eq!(history[0]["status"],"공장 회신 완료");
        let settlement=output("prepare_settlement",json!({"state":stored,"date":"2026-11-13","historyRows":history})).unwrap();
        assert_eq!(settlement["billedKnown"],13000);
        assert_eq!(settlement["paidCash"],13000);
        assert_eq!(settlement["shippingKnown"],3000);
        assert_eq!(settlement["actualFreightKnown"],4000);
        assert_eq!(settlement["freightUnknown"],0);
        assert_eq!(fs::read(&exported).unwrap(),original);
        assert_eq!(fs::read(stored["factoryReplies"][0]["archivedPath"].as_str().unwrap()).unwrap(),bytes);
        fs::write(inbox.join("duplicate.xlsx"),&bytes).unwrap();
        let replay=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert!(replay.error.is_none(),"{:?}",replay.error);
        assert_eq!((replay.files,replay.applied,replay.held),(0,0,0));
        assert_eq!(snapshot(&mut conn).unwrap(),stored);
    }

    #[test]
    fn scoped_factory_reply_matches_full_result_and_checks_unrelated_waybills(){
        let base=Temp::new();
        let (mut conn,_)=synthetic_db(&base.0);
        let mut initial=snapshot(&mut conn).unwrap();
        for (n,name,tracking) in [(2,"두번째 수취인",""),(3,"다른 수취인","001234567890")]{
            let mut recipient=initial["orders"][0]["recipients"][0].clone();
            recipient["id"]=json!(format!("r{n}"));
            recipient["name"]=json!(name);
            recipient["items"][0]["id"]=json!(format!("i{n}"));
            initial["orders"].as_array_mut().unwrap().push(json!({
                "id":format!("o{n}"),"clientId":"c1","recipients":[recipient.clone()]
            }));
            initial["workflow"]["shipments"].as_array_mut().unwrap().push(json!({
                "id":format!("s{n}"),"orderId":format!("o{n}"),"recipientId":format!("r{n}"),
                "charges":{"FACTORY":3000},"actualFreight":{},"tracking":{"FACTORY":tracking}
            }));
            initial["workflow"]["queue"].as_array_mut().unwrap().push(json!({
                "id":format!("q{n}"),"shipmentId":format!("s{n}"),
                "origin":"FACTORY","status":"EXPORTED","snapshot":{"recipient":recipient}
            }));
        }
        let tx=conn.transaction().unwrap();
        bando_data_access::store_v2_relational(&tx,&initial).unwrap();
        let mut envelope=initial.clone();
        for key in ["products","clients","orders"]{envelope.as_object_mut().unwrap().remove(key);}
        tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",[envelope.to_string()]).unwrap();
        tx.commit().unwrap();
        let initial=snapshot(&mut conn).unwrap();
        let bytes=reply_bytes("001234567890",4000);
        let mut source=native_workbooks::read_factory_reply("reply.xlsx",&bytes).unwrap();
        let mut second=source["sourceRows"][0].clone();
        second["row"]=json!(3);
        second["name"]=json!("두번째 수취인");
        second["tracking"]=json!("009876543210");
        source["sourceRows"].as_array_mut().unwrap().push(second);
        source["sourceRowCount"]=json!(2);
        source["businessDate"]=json!("2026-11-25");
        source["at"]=json!("2026-11-25T01:00:00Z");
        source["freightAt"]=source["at"].clone();
        source["actor"]=json!("합성 직원");
        source["freightEventIds"]=json!(["f1","f2"]);
        source["replyEventIds"]=json!(["e1","e2"]);
        let mut expected=rules::import_automatic_factory_reply(&initial,&source).unwrap();
        expected["next"]["revision"]=json!(8);
        expected["next"]["factoryReplies"][0]["archivedPath"]=json!("synthetic.xlsx");
        let result=bando_data_access::update_v2_factory_reply_snapshot(&mut conn,7,&source,|scope|{
            let orders=scope["orders"].as_array().unwrap();
            assert_eq!(orders.iter().map(|o|o["id"].as_str().unwrap()).collect::<Vec<_>>(),vec!["o1","o2"]);
            assert!(scope.get("products").is_none());
            assert!(scope.get("clients").is_none());
            assert_eq!(scope["workflow"]["shipments"].as_array().unwrap().len(),3);
            assert_eq!(scope["workflow"]["payments"],initial["workflow"]["payments"]);
            Ok("synthetic.xlsx".to_owned())
        }).unwrap();
        assert_eq!(result,expected);
        assert_eq!(result["applied"],1);
        assert_eq!(result["held"],1);
        assert_eq!(result["next"]["workflow"]["shipments"][2],initial["workflow"]["shipments"][2]);
        assert_eq!(snapshot(&mut conn).unwrap(),result["next"]);
    }

    #[test]
    fn duplicate_metadata_does_not_hydrate_relational_order_tables(){
        let base=Temp::new();
        let (mut conn,inbox)=synthetic_db(&base.0);
        let bytes=reply_bytes("001234567890",4000);
        fs::write(inbox.join("reply.xlsx"),&bytes).unwrap();
        let imported=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert_eq!((imported.files,imported.applied),(1,1));
        assert!(imported.error.is_none(),"{:?}",imported.error);
        assert_eq!(import_metadata(&conn,&id(&bytes)).unwrap(),(8,true));
        // A missing relational table would make a full hydration fail.
        // A duplicate is nevertheless recognized from the envelope alone.
        conn.execute_batch("DROP TABLE core_items;").unwrap();
        assert!(snapshot(&mut conn).is_err());
        let replay=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert!(replay.error.is_none(),"{:?}",replay.error);
        assert_eq!((replay.files,replay.applied,replay.held),(0,0,0));
        assert_eq!(import_metadata(&conn,&id(&bytes)).unwrap(),(8,true));
    }

    #[test]
    fn durable_duplicate_is_skipped_before_binary_parser(){
        let base=Temp::new();
        let (mut conn,inbox)=synthetic_db(&base.0);
        // Deliberately synthetic history entry: these bytes cannot be decoded.
        // A known content ID must bypass the parser, while unknown malformed
        // bytes must still be reported by the real scanner.
        let known=b"synthetic registered non-XLSX bytes";
        let unknown=b"synthetic unregistered non-XLSX bytes";
        assert!(native_workbooks::read_factory_reply("known.xlsx",known).is_err());
        let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        let mut envelope:Value=serde_json::from_str(&raw).unwrap();
        envelope["factoryReplies"]=json!([{"id":id(known),"status":"APPLIED","rows":[]}]);
        conn.execute("UPDATE app_state SET payload=?1 WHERE id=1",[envelope.to_string()]).unwrap();
        fs::write(inbox.join("renamed-known.xlsx"),known).unwrap();
        let mut cache=Scan::default();
        let skipped=scan_ready(&mut cache,&mut conn,&base.0);
        assert_eq!((skipped.files,skipped.applied,skipped.held),(0,0,0));
        assert!(skipped.error.is_none(),"{:?}",skipped.error);
        assert_eq!(import_metadata(&conn,&id(known)).unwrap(),(7,true));
        fs::write(inbox.join("unknown.xlsx"),unknown).unwrap();
        let invalid=scan_ready(&mut cache,&mut conn,&base.0);
        assert_eq!(invalid.files,0);
        assert!(invalid.error.as_deref().is_some_and(|e|e.contains("회신 파일 검토 필요")));
        assert_eq!(import_metadata(&conn,&id(unknown)).unwrap(),(7,false));
        let after:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&after).unwrap(),envelope);
    }

    #[test]
    fn factory_binary_failed_sqlite_write_preserves_before_image_and_can_retry(){
        let base=Temp::new();
        let (mut conn,inbox)=synthetic_db(&base.0);
        let before=snapshot(&mut conn).unwrap();
        let bytes=reply_bytes("001234567890",4000);
        fs::write(inbox.join("reply.xlsx"),&bytes).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_reply BEFORE UPDATE ON app_state
            BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
        let mut cache=Scan::default();
        let failed=scan_ready(&mut cache,&mut conn,&base.0);
        assert_eq!(failed.files,0);
        assert!(failed.error.is_some());
        assert_eq!(snapshot(&mut conn).unwrap(),before);
        // Source preservation is intentionally outside the DB transaction.
        assert_eq!(fs::read_dir(base.0.join("factory-replies")).unwrap().count(),1);
        conn.execute_batch("DROP TRIGGER reject_reply;").unwrap();
        let retry=scan_with_db(&mut cache,&mut conn,&base.0).unwrap();
        assert!(retry.error.is_none(),"{:?}",retry.error);
        assert_eq!((retry.files,retry.applied,retry.held),(1,1,0));
        assert_eq!(snapshot(&mut conn).unwrap()["revision"],8);
        assert_eq!(fs::read_dir(base.0.join("factory-replies")).unwrap().count(),1);
    }

    #[test]
    fn automatic_factory_correction_updates_only_cost_and_preserves_paid_amounts_and_history(){
        let base=Temp::new();
        let (mut conn,inbox)=synthetic_db(&base.0);
        let initial=snapshot(&mut conn).unwrap();
        let first=reply_bytes("001234567890",4000);
        let correction=reply_bytes("009876543210",5500);
        fs::write(inbox.join("first.xlsx"),&first).unwrap();
        let mut cache=Scan::default();
        let imported=scan_ready(&mut cache,&mut conn,&base.0);
        assert!(imported.error.is_none(),"{:?}",imported.error);
        assert_eq!((imported.files,imported.applied,imported.held),(1,1,0));
        let before=snapshot(&mut conn).unwrap();
        fs::write(inbox.join("corrected.xlsx"),&correction).unwrap();
        let updated=scan_ready(&mut cache,&mut conn,&base.0);
        assert!(updated.error.is_none(),"{:?}",updated.error);
        assert_eq!((updated.files,updated.applied,updated.held),(1,1,0));
        drop(conn);
        let mut conn=Connection::open(base.0.join("synthetic.sqlite")).unwrap();
        let after=snapshot(&mut conn).unwrap();
        assert_eq!(after["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+1);
        let ship=&after["workflow"]["shipments"][0];
        assert_eq!(ship["actualFreight"]["FACTORY"],5500);
        assert_eq!(ship["tracking"]["FACTORY"],"009876543210");
        assert_eq!(ship["charges"]["FACTORY"],3000);
        for key in ["payments","confirmations","queue"]{
            assert_eq!(after["workflow"][key],initial["workflow"][key],"{key}");
        }
        assert_eq!(after["orders"],initial["orders"]);
        let replies=after["factoryReplies"].as_array().unwrap();
        assert_eq!(replies.len(),2);
        assert_eq!(replies[0],before["factoryReplies"][0]);
        assert_eq!(replies[1]["status"],"APPLIED");
        assert_eq!(replies[1]["rows"][0]["shipmentId"],"s1");
        let events=after["workflow"]["events"].as_array().unwrap();
        assert_eq!(events.len(),4);
        assert_eq!(events[2]["before"],4000);
        assert_eq!(events[2]["after"]["amount"],5500);
        assert_eq!(events[3]["action"],"공장 송장 / 운임 정정");
        assert_eq!(fs::read(replies[1]["archivedPath"].as_str().unwrap()).unwrap(),correction);
        let repeated=scan_ready(&mut Scan::default(),&mut conn,&base.0);
        assert!(repeated.error.is_none(),"{:?}",repeated.error);
        assert_eq!(repeated.files,0);
        assert_eq!(snapshot(&mut conn).unwrap(),after);
    }
}
