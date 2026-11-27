//! Derived daily workbook publisher. The SQLite reservation is authoritative.
use bando_data_access::excel_aggregate_jobs as jobs;
use calamine::{Data,Reader,Xlsx};
use chrono::Utc;
use rusqlite::{Connection,TransactionBehavior};
use rust_xlsxwriter::{Format,Workbook};
use sha2::{Digest,Sha256};
use std::{collections::HashMap,fs,io::{Cursor,Read,Write},path::{Path,PathBuf},sync::{OnceLock,mpsc::{self,SyncSender,Receiver,RecvTimeoutError},atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
use uuid::Uuid;

fn err(e:impl std::fmt::Display)->String{e.to_string()}
// No idle reconciliation timer. Reconcile once at startup, or after
// an explicit archive configuration change / successful database restore.
const WORKER_ERROR_RETRY:Duration=Duration::from_secs(60);
static WORKER:OnceLock<Option<SyncSender<()>>>=OnceLock::new();
static RECONCILE_REQUESTED:AtomicBool=AtomicBool::new(false);

fn encode(rows:&[Vec<String>],reply:bool)->Result<Vec<u8>,String>{
    let columns=if reply{17}else{16};
    let mut book=Workbook::new();let sheet=book.add_worksheet();
    sheet.set_name("로젠택배").map_err(err)?;
    let header=Format::new().set_bold();
    for col in 0..columns{
        let title=crate::excel_service::HEADERS.get(col).copied().unwrap_or("운송장번호");
        let width=crate::excel_service::WIDTHS.get(col).copied().unwrap_or(22.);
        sheet.write_string_with_format(0,col as u16,title,&header).map_err(err)?;
        sheet.set_column_width(col as u16,width).map_err(err)?;
    }
    for (i,row) in rows.iter().enumerate(){
        if row.len()!=columns{return Err("취합 열 수 오류".into())}
        let n=u32::try_from(i+1).map_err(err)?;
        for (c,value) in row.iter().enumerate(){sheet.write_string(n,c as u16,value).map_err(err)?;}
    }
    sheet.set_freeze_panes(1,0).map_err(err)?;
    sheet.autofilter(0,0,u32::try_from(rows.len()).map_err(err)?,(columns-1) as u16).map_err(err)?;
    let bytes=book.save_to_buffer().map_err(err)?;
    // Validate actual serialized cell values, including all leading zeros.
    let mut check=Xlsx::new(Cursor::new(bytes.as_slice())).map_err(err)?;
    let range=check.worksheet_range("로젠택배").map_err(err)?;
    if range.end().map(|(r,_)|r as usize)!=Some(rows.len()){return Err("취합 Excel 행 수 불일치".into())}
    for (i,row) in rows.iter().enumerate(){
        for (c,value) in row.iter().enumerate(){
            match range.get_value(((i+1) as u32,c as u32)).unwrap_or(&Data::Empty){
                Data::String(s) if s==value=>{},
                Data::Empty if value.is_empty()=>{},
                _=>return Err("취합 Excel 기록값 불일치".into()),
            }
        }
    }
    Ok(bytes)
}
fn file_hash(path:&Path)->Result<String,String>{
    let mut f=fs::File::open(path).map_err(err)?;
    let mut h=Sha256::new();let mut buffer=[0u8;65536];
    loop{let n=f.read(&mut buffer).map_err(err)?;if n==0{break}h.update(&buffer[..n]);}
    Ok(format!("{:x}",h.finalize()))
}
fn checked_target(j:&jobs::Job)->Result<PathBuf,String>{
    let isolated=if crate::business_state::test_launch(){Some(crate::business_state::test_directory("Excel")?)}else{None};
    checked_target_in(j,isolated.as_deref())
}
fn checked_target_in(j:&jobs::Job,isolated:Option<&Path>)->Result<PathBuf,String>{
    let path=Path::new(&j.path);
    let root=path.ancestors().nth(5).ok_or("취합 보관함 경로 오류")?;
    if jobs::target(root,&j.date,&j.origin)?!=path{return Err("취합 파일 경로 오류".into())}
    let actual=root.canonicalize().map_err(err)?;
    if !actual.is_dir(){return Err("취합 보관함 폴더가 없습니다.".into())}
    if isolated.is_some_and(|root|actual!=root){
        return Err("테스트 취합 폴더 격리 위반".into())
    }
    let relative=path.strip_prefix(root).map_err(err)?;
    let mut current=actual;
    let parent=relative.parent().ok_or("취합 상대 경로 오류")?;
    for component in parent.components(){
        match component{std::path::Component::Normal(c)=>current.push(c),_=>return Err("취합 경로 오류".into())}
        match fs::create_dir(&current){
            Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(err(e)),
        }
        if current.canonicalize().map_err(err)?!=current||!current.is_dir(){return Err("취합 경로 리디렉션 금지".into())}
    }
    current.push(path.file_name().ok_or("취합 파일명 오류")?);
    match fs::symlink_metadata(&current){
        Ok(m) if !m.file_type().is_file()=>return Err("취합 경로는 일반 파일이어야 합니다.".into()),
        Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(err(e)),
    }
    Ok(current)
}
struct Temp(PathBuf);
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_file(&self.0);}}
fn read_legacy_reply(base:&Path,record:&serde_json::Value,row:&serde_json::Value,cache:&mut HashMap<String,serde_json::Value>)->Result<Vec<String>,String>{
    let id=record["id"].as_str().ok_or("원본 회신 ID 누락")?;
    let digest=id.strip_prefix("factory-reply-").filter(|s|s.len()==64&&s.bytes().all(|c|c.is_ascii_hexdigit()))
        .ok_or("기존 회신 원본 ID를 검증할 수 없습니다.")?;
    if !cache.contains_key(id){
        let dir=base.canonicalize().map_err(err)?.join("factory-replies");
        let path=dir.join(format!("{digest}.xlsx"));
        if dir.canonicalize().map_err(err)?!=dir||path.canonicalize().map_err(err)?!=path{
            return Err("회신 원본 보관 경로 오류".into())
        }
        let file=fs::File::open(&path).map_err(err)?;
        let mut bytes=Vec::new();file.take(10*1024*1024+1).read_to_end(&mut bytes).map_err(err)?;
        if bytes.len()>10*1024*1024||format!("{:x}",Sha256::digest(&bytes))!=digest{
            return Err("회신 원본 파일 해시 불일치".into())
        }
        let source=crate::native_workbooks::read_factory_reply("original.xlsx",&bytes)?;
        if source["id"].as_str()!=Some(id){return Err("회신 원본 식별자 불일치".into())}
        cache.insert(id.to_owned(),source);
    }
    let source=cache.get(id).ok_or("회신 원본 캐시 누락")?;
    let original=source["sourceRows"].as_array().and_then(|rows|rows.iter().find(|r|r["row"]==row["row"]))
        .ok_or("회신 원본 행 누락")?;
    let v=original["aggregateValues"].as_array().ok_or("회신 원본 열 누락")?;
    v.iter().map(|x|x.as_str().map(str::to_owned).ok_or_else(||"회신 원본 값 오류".into())).collect()
}
#[cfg(test)]
fn crash_checkpoint(stage:&str){
    if std::env::var("BANDO_AGGREGATE_CRASH_STAGE").ok().as_deref()!=Some(stage){return}
    let root=PathBuf::from(std::env::var_os("BANDO_AGGREGATE_CRASH_ROOT").expect("isolated crash fixture"));
    fs::write(root.join("crash-ready"),stage).expect("write crash checkpoint");
    loop{std::thread::sleep(Duration::from_millis(100));}
}
pub(crate) fn publish(conn:&mut Connection,requested:&jobs::Job,base:&Path)->Result<(),String>{
    publish_with(conn,requested,base,|_|Ok(()))
}
fn publish_with<F>(conn:&mut Connection,requested:&jobs::Job,base:&Path,mut checkpoint:F)->Result<(),String>
where F:FnMut(&str)->Result<(),String>{
    // Capture generation and projected rows from a consistent SQLite read.
    let tx=conn.transaction().map_err(err)?;
    let Some(j)=jobs::current(&tx,requested)? else{return Ok(())};
    if j.desired<=j.saved{return Ok(())}
    let binding=jobs::legacy_ids(&tx,&j)?;
    let mut cache=HashMap::new();
    let rows=jobs::daily_rows(&tx,&j,|record,row|read_legacy_reply(base,record,row,&mut cache))?;
    tx.commit().map_err(err)?;
    #[cfg(test)]
    crash_checkpoint("after_db_commit");
    let bytes=encode(&rows,j.origin=="REPLY")?;
    checkpoint("before_target")?;
    let target=checked_target(&j)?;
    let dir=target.parent().ok_or("취합 저장 폴더 누락")?;
    let tmp=Temp(dir.join(format!(".bando-aggregate-{}.tmp",Uuid::new_v4())));
    checkpoint("before_temp_open")?;
    let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&tmp.0).map_err(err)?;
    checkpoint("before_temp_write")?;
    #[cfg(test)]
    {
        let half=bytes.len()/2;
        file.write_all(&bytes[..half]).map_err(err)?;
        crash_checkpoint("during_temp_write");
        file.write_all(&bytes[half..]).and_then(|_|file.sync_all()).map_err(err)?;
    }
    #[cfg(not(test))]
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(err)?;
    drop(file);
    checkpoint("after_temp_write")?;
    let hash=format!("{:x}",Sha256::digest(&bytes));
    if file_hash(&tmp.0)?!=hash{return Err("임시 취합 Excel 해시 불일치".into())}
    checkpoint("before_commit_lock")?;
    // Hold only the short final publish transaction; concurrent writers cannot
    // replace a newer generation with this obsolete snapshot.
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    let Some(latest)=jobs::current(&tx,&j)? else{return Ok(())};
    if latest.desired!=j.desired||latest.saved>=j.desired||jobs::legacy_ids(&tx,&j)?!=binding{return Ok(())}
    if checked_target(&j)?!=target{return Err("취합 보관함 경로 변경".into())}
    // Same-directory rename replaces the destination without truncating it
    // first. On Windows a sharing violation leaves the previous file intact.
    #[cfg(test)]
    crash_checkpoint("before_replace");
    checkpoint("before_replace")?;
    fs::rename(&tmp.0,&target).map_err(err)?;
    #[cfg(test)]
    crash_checkpoint("after_replace");
    #[cfg(unix)]
    fs::File::open(dir).and_then(|f|f.sync_all()).map_err(err)?;
    jobs::mark_saved(&tx,&j,&hash)?;
    #[cfg(test)]
    crash_checkpoint("after_mark_before_commit");
    tx.commit().map_err(err)?;
    Ok(())
}
fn verify_completed(conn:&Connection)->Result<(),String>{
    for j in jobs::jobs(conn,Utc::now().timestamp(),true)?{
        if j.saved!=j.desired{continue}
        let valid=checked_target(&j).and_then(|p|file_hash(&p)).ok().as_ref()==j.hash.as_ref()&&j.hash.is_some();
        if !valid{jobs::invalidate(conn,&j)?;}
    }
    Ok(())
}
fn clean_stale_temps(conn:&mut Connection,min_age:Duration)->Result<(),String>{
    // Do not hold the business writer lock while scanning archive directories.
    for j in jobs::jobs(conn,0,true)?{
        let Ok(target)=checked_target(&j)else{continue};
        let Some(dir)=target.parent()else{continue};
        let Ok(entries)=fs::read_dir(dir)else{continue};
        for entry in entries.flatten(){
            let name=entry.file_name();let Some(name)=name.to_str()else{continue};
            let Some(token)=name.strip_prefix(".bando-aggregate-").and_then(|s|s.strip_suffix(".tmp"))else{continue};
            if Uuid::parse_str(token).is_err(){continue}
            let Ok(meta)=fs::symlink_metadata(entry.path())else{continue};
            if meta.file_type().is_file()&&meta.modified().ok().and_then(|t|t.elapsed().ok()).is_some_and(|age|age>=min_age){
                let _=fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}
fn run(reconcile:bool)->Result<Option<Duration>,String>{
    let (mut conn,base)=crate::factory_reply_inbox::open_db()?;
    if reconcile{
        let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
        let settings=bando_data_access::read_v2_settings_snapshot(&tx)?;
        let configured=settings["settings"]["excelDirectory"].as_str().filter(|p|!p.trim().is_empty()).map(PathBuf::from);
        let root=if crate::business_state::test_launch(){Some(crate::business_state::test_directory("Excel")?)}else{
            configured.filter(|p|p.is_absolute()).map(|p|p.canonicalize().unwrap_or(p))
        };
        jobs::recover_history(&tx,root.as_deref())?;
        tx.commit().map_err(err)?;
        verify_completed(&conn)?;
        clean_stale_temps(&mut conn,Duration::from_secs(24*3600))?;
    }
    let now=Utc::now().timestamp();
    for job in jobs::jobs(&conn,now,false)?.into_iter().take(16){
        if let Err(e)=publish(&mut conn,&job,&base){
            jobs::mark_failed(&conn,&job,Utc::now().timestamp(),&e)?;
        }
    }
    let now=Utc::now().timestamp();
    Ok(jobs::next_due(&conn)?.map(|due|Duration::from_secs(due.saturating_sub(now).max(0) as u64)))
}
/// Start once on the first application frame, not once per poll interval.
pub fn start(){
    if cfg!(test){return}
    let _=WORKER.get_or_init(||{
        // Launch configuration is fixed, but an authorized DB may be
        // temporarily unavailable. Actual authorization stays in open_db().
        if std::env::var("BANDO_EGUI_ENABLE_EXCEL_WRITE").ok().as_deref()!=Some("1")
            ||std::env::var("BANDO_EGUI_ENABLE_SETTINGS_WRITE").ok().as_deref()!=Some("1")
            ||std::env::var_os("BANDO_EGUI_BUSINESS_DB").is_none_or(|p|p.is_empty()){return None}
        let (tx,rx)=mpsc::sync_channel(1);
        match std::thread::Builder::new().name("bando-excel-aggregate".into()).spawn(move||worker_loop(rx)){
            Ok(_)=>Some(tx),Err(e)=>{eprintln!("Excel aggregate worker: {e}");None}
        }
    });
}
/// Only call after a successful business commit. The channel is a wake-up
/// hint; the transactional SQLite reservation remains authoritative.
pub(crate) fn notify(reconcile:bool){
    if cfg!(test){return}
    if reconcile{RECONCILE_REQUESTED.store(true,Ordering::Release);}
    // Initialization can race with the first commit. Ensure the channel
    // has been published before sending the wakeup.
    start();
    if let Some(Some(tx))=WORKER.get(){let _=tx.try_send(());}
}
fn worker_loop(rx:Receiver<()>){
    worker_loop_with(rx,&RECONCILE_REQUESTED,WORKER_ERROR_RETRY,run);
}
fn worker_loop_with<F>(rx:Receiver<()>,requested_flag:&AtomicBool,error_retry:Duration,mut run_work:F)
where F:FnMut(bool)->Result<Option<Duration>,String>{
    let mut retry_reconcile=true; // one startup recovery
    loop{
        let requested=requested_flag.swap(false,Ordering::AcqRel);
        let reconcile=retry_reconcile||requested;
        let retry=match run_work(reconcile){
            Ok(next)=>{
                retry_reconcile=false;
                next
            },
            Err(e)=>{
                eprintln!("Excel aggregate: {e}");
                retry_reconcile=reconcile;
                Some(error_retry)
            }
        };
        // An empty durable queue has no deadline at all. Do not wake to
        // inspect the database or archive when no work was scheduled.
        match retry{
            Some(delay)=>match rx.recv_timeout(delay){
                Ok(())|Err(RecvTimeoutError::Timeout)=>{},
                Err(RecvTimeoutError::Disconnected)=>break,
            },
            None=>if rx.recv().is_err(){break},
        }
    }
}
#[cfg(test)]
fn spawn_worker(ctx:&eframe::egui::Context,work:impl FnOnce()+Send+'static)->std::thread::JoinHandle<()>{
    let ctx=ctx.clone();
    std::thread::spawn(move||{work();ctx.request_repaint();})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn resident_worker_waits_for_events_and_durable_retry_without_idle_polling(){
        let (wake_tx,wake_rx)=mpsc::sync_channel(1);
        let (observed_tx,observed_rx)=mpsc::channel();
        let requested=std::sync::Arc::new(AtomicBool::new(false));
        let flag=requested.clone();
        let handle=std::thread::spawn(move||{
            let mut count=0;
            worker_loop_with(wake_rx,&flag,Duration::from_secs(60),|reconcile|{
                count+=1;observed_tx.send((count,reconcile)).unwrap();
                Ok(if count==2{Some(Duration::from_millis(30))}else{None})
            });
        });
        let bound=Duration::from_secs(5);
        assert_eq!(observed_rx.recv_timeout(bound).unwrap(),(1,true));
        assert!(matches!(observed_rx.recv_timeout(Duration::from_millis(100)),Err(RecvTimeoutError::Timeout)));
        wake_tx.send(()).unwrap();
        assert_eq!(observed_rx.recv_timeout(bound).unwrap(),(2,false));
        assert_eq!(observed_rx.recv_timeout(bound).unwrap(),(3,false));
        assert!(matches!(observed_rx.recv_timeout(Duration::from_millis(100)),Err(RecvTimeoutError::Timeout)));
        requested.store(true,Ordering::Release);wake_tx.send(()).unwrap();
        assert_eq!(observed_rx.recv_timeout(bound).unwrap(),(4,true));
        assert!(matches!(observed_rx.recv_timeout(Duration::from_millis(100)),Err(RecvTimeoutError::Timeout)));
        drop(wake_tx);handle.join().unwrap();
    }
    #[test]
    fn serialized_reply_keeps_q_waybill_and_g_freight_as_text(){
        let mut row=vec![String::new();17];row[0]="가상 수하인".into();row[6]="3000".into();row[16]="0000123456".into();
        let b=encode(&[row],true).unwrap();let mut x=Xlsx::new(Cursor::new(b)).unwrap();
        let r=x.worksheet_range("로젠택배").unwrap();
        assert_eq!(r.get_value((1,16)),Some(&Data::String("0000123456".into())));
        assert_eq!(r.get_value((1,6)),Some(&Data::String("3000".into())));
    }
    struct Fixture{root:PathBuf,conn:Connection,job:jobs::Job}
    impl Fixture{
        fn new()->Self{
            let root=std::env::temp_dir().join(format!("bando-aggregate-db-{}",Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let root=root.canonicalize().unwrap();
            let conn=Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
                INSERT INTO core_schema VALUES('business-core',2);
                CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
            jobs::initialize(&conn).unwrap();
            let target=jobs::target(&root,"2026-11-27","STORE").unwrap();
            let job=jobs::Job{date:"2026-11-27".into(),origin:"STORE".into(),path:target.to_str().unwrap().into(),
                desired:1,saved:0,retries:0,hash:None};
            conn.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision)
                VALUES (?1,?2,?3,1)",rusqlite::params![job.date,job.origin,job.path]).unwrap();
            Self{root,conn,job}
        }
        fn update(&mut self,n:i64){
            let path=Path::new(&self.job.path);
            let records=(1..=n).map(|i|{
                let mut v=vec![String::new();16];v[0]=format!("수하인{i}");v[1]="01234".into();
                serde_json::json!({"id":format!("id-{i}"),"savedPath":path.with_file_name(format!("20261127(매장{i}).xlsx")),
                    "outputRows":[{"values":v}]})
            }).collect::<Vec<_>>();
            let state=serde_json::json!({"revision":n,"exports":records});
            self.conn.execute("INSERT INTO app_state(id,payload) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
                [state.to_string()]).unwrap();
            self.conn.execute("UPDATE excel_aggregate_jobs SET desired_revision=?1,next_retry_at=NULL",[n]).unwrap();
            self.job=jobs::current(&self.conn,&self.job).unwrap().unwrap();
        }
    }
    impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}
    #[test]
    fn actual_publish_rebuilds_from_db_after_damage_and_preserves_generation(){
        let mut f=Fixture::new();f.update(1);
        publish(&mut f.conn,&f.job,&f.root).unwrap();
        let one=fs::read(&f.job.path).unwrap();
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,1);
        f.update(2);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let mut x=Xlsx::new(Cursor::new(fs::read(&f.job.path).unwrap())).unwrap();
        let r=x.worksheet_range("로젠택배").unwrap();
        assert_eq!(r.get_value((2,1)),Some(&Data::String("01234".into())));
        assert_ne!(one,fs::read(&f.job.path).unwrap());
        fs::write(&f.job.path,b"damaged").unwrap();
        verify_completed(&f.conn).unwrap();
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,0);
        publish(&mut f.conn,&f.job,&f.root).unwrap();
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,2);
        fs::remove_file(&f.job.path).unwrap();verify_completed(&f.conn).unwrap();
        publish(&mut f.conn,&f.job,&f.root).unwrap();assert!(Path::new(&f.job.path).is_file());
    }
    #[cfg(windows)]
    #[test]
    fn actual_publish_retries_locked_file_without_losing_generation(){
        use std::os::windows::fs::OpenOptionsExt;
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let old=fs::read(&f.job.path).unwrap();
        let lock=fs::OpenOptions::new().read(true).share_mode(0).open(&f.job.path).unwrap();
        f.update(2);let e=publish(&mut f.conn,&f.job,&f.root).unwrap_err();
        jobs::mark_failed(&f.conn,&f.job,100,&e).unwrap();
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,1);
        drop(lock);assert_eq!(fs::read(&f.job.path).unwrap(),old);
        assert!(jobs::jobs(&f.conn,104,false).unwrap().is_empty());
        let pending=jobs::jobs(&f.conn,105,false).unwrap();assert_eq!(pending.len(),1);
        publish(&mut f.conn,&pending[0],&f.root).unwrap();
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,2);
    }
    #[test]
    fn stale_temp_cleanup_ignores_original_and_unknown_files(){
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let dir=Path::new(&f.job.path).parent().unwrap();
        let stale=dir.join(format!(".bando-aggregate-{}.tmp",Uuid::new_v4()));
        let unknown=dir.join(".bando-aggregate-not-a-uuid.tmp");
        let original=dir.join("20261127(매장1).xlsx");
        for p in [&stale,&unknown,&original]{fs::write(p,b"test").unwrap();}
        clean_stale_temps(&mut f.conn,Duration::ZERO).unwrap();
        assert!(!stale.exists());assert!(unknown.exists());assert!(original.exists());
        assert!(Path::new(&f.job.path).exists());
    }
    #[test]
    fn legacy_original_is_verified_and_read_from_exact_row(){
        let f=Fixture::new();let dir=f.root.join("factory-replies");fs::create_dir(&dir).unwrap();
        let mut row=vec![String::new();17];row[0]="가상 수하인".into();row[1]="01234".into();
        row[6]="3000".into();row[16]="0000123456".into();
        let bytes=encode(&[row],true).unwrap();let digest=format!("{:x}",Sha256::digest(&bytes));
        let path=dir.join(format!("{digest}.xlsx"));fs::write(&path,&bytes).unwrap();
        let record=serde_json::json!({"id":format!("factory-reply-{digest}")});
        let source=serde_json::json!({"row":2});
        let mut cache=HashMap::new();
        let v=read_legacy_reply(&f.root,&record,&source,&mut cache).unwrap();
        assert_eq!(v[1],"01234");assert_eq!(v[16],"0000123456");
        fs::write(path,b"bad").unwrap();
        assert!(read_legacy_reply(&f.root,&record,&source,&mut HashMap::new()).is_err());
    }
    #[test]
    fn aggregate_crash_child(){
        let Some(root)=std::env::var_os("BANDO_AGGREGATE_CRASH_ROOT").map(PathBuf::from)else{return};
        let mut conn=Connection::open(root.join("crash.sqlite")).unwrap();
        let job=jobs::jobs(&conn,0,false).unwrap().remove(0);
        publish(&mut conn,&job,&root).unwrap();
        panic!("crash checkpoint not reached");
    }
    #[test]
    fn process_kill_recovers_all_publication_boundaries(){
        use std::process::{Command,Stdio};
        for stage in ["after_db_commit","during_temp_write","before_replace","after_replace","after_mark_before_commit"]{
            let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
            let old=fs::read(&f.job.path).unwrap();f.update(2);
            let state:String=f.conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
            let disk=Connection::open(f.root.join("crash.sqlite")).unwrap();
            disk.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
                INSERT INTO core_schema VALUES('business-core',2);
                CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
            disk.execute("INSERT INTO app_state VALUES(1,?1)",[state]).unwrap();
            jobs::initialize(&disk).unwrap();
            disk.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision,saved_revision,file_hash)
                VALUES (?1,?2,?3,2,1,?4)",rusqlite::params![f.job.date,f.job.origin,f.job.path,f.job.hash]).unwrap();
            drop(disk);
            let mut child=Command::new(std::env::current_exe().unwrap())
                .args(["--exact","excel_aggregate::tests::aggregate_crash_child","--nocapture"])
                .env("BANDO_AGGREGATE_CRASH_ROOT",&f.root).env("BANDO_AGGREGATE_CRASH_STAGE",stage)
                .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            let deadline=Instant::now()+Duration::from_secs(30);
            let ready=loop{
                if fs::read_to_string(f.root.join("crash-ready")).ok().as_deref()==Some(stage){break true}
                if child.try_wait().unwrap().is_some()||Instant::now()>=deadline{break false}
                std::thread::sleep(Duration::from_millis(20));
            };
            let _=child.kill();let _=child.wait();assert!(ready,"child never reached {stage}");
            let after_kill=fs::read(&f.job.path).unwrap();
            if matches!(stage,"after_db_commit"|"during_temp_write"|"before_replace"){
                assert_eq!(after_kill,old,"old file was changed before replace: {stage}");
            }else{assert_ne!(after_kill,old);}
            let mut reopened=Connection::open(f.root.join("crash.sqlite")).unwrap();
            let pending=jobs::jobs(&reopened,0,false).unwrap();
            assert_eq!(pending.len(),1);assert_eq!(pending[0].saved,1);
            publish(&mut reopened,&pending[0],&f.root).unwrap();
            let mut book=Xlsx::new(Cursor::new(fs::read(&f.job.path).unwrap())).unwrap();
            let range=book.worksheet_range("로젠택배").unwrap();
            assert_eq!(range.end().unwrap().0,2);
            assert_eq!(range.get_value((2,1)),Some(&Data::String("01234".into())));
            assert!(jobs::jobs(&reopened,0,false).unwrap().is_empty());
            clean_stale_temps(&mut reopened,Duration::ZERO).unwrap();
            assert!(!fs::read_dir(Path::new(&f.job.path).parent().unwrap()).unwrap()
                .filter_map(Result::ok).any(|e|e.file_name().to_string_lossy().starts_with(".bando-aggregate-")));
        }
    }
    #[test]
    fn injected_io_failures_preserve_old_workbook_and_pending_revision(){
        for (stage,kind) in [("before_temp_open",std::io::ErrorKind::PermissionDenied),
            ("before_temp_write",std::io::ErrorKind::StorageFull),
            ("after_temp_write",std::io::ErrorKind::Other),
            ("before_replace",std::io::ErrorKind::PermissionDenied)]{
            let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
            let old=fs::read(&f.job.path).unwrap();f.update(2);
            let result=publish_with(&mut f.conn,&f.job,&f.root,|s|{
                if s==stage{Err(std::io::Error::from(kind).to_string())}else{Ok(())}
            });
            assert!(result.is_err(),"{stage}");assert_eq!(fs::read(&f.job.path).unwrap(),old);
            let pending=jobs::current(&f.conn,&f.job).unwrap().unwrap();
            assert_eq!((pending.saved,pending.desired),(1,2));
            assert!(!fs::read_dir(Path::new(&f.job.path).parent().unwrap()).unwrap()
                .filter_map(Result::ok).any(|e|e.file_name().to_string_lossy().starts_with(".bando-aggregate-")));
            publish(&mut f.conn,&f.job,&f.root).unwrap();
            assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,2);
        }
    }
    #[test]
    fn missing_archive_root_recovers_without_redirecting_to_other_folder(){
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let old=fs::read(&f.job.path).unwrap();f.update(2);
        let offline=f.root.with_extension("offline");
        fs::rename(&f.root,&offline).unwrap();
        let failed=publish(&mut f.conn,&f.job,&f.root);
        fs::rename(&offline,&f.root).unwrap();
        assert!(failed.is_err());assert_eq!(fs::read(&f.job.path).unwrap(),old);
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,1);
        publish(&mut f.conn,&f.job,&f.root).unwrap();
        assert_eq!(jobs::current(&f.conn,&f.job).unwrap().unwrap().saved,2);
    }
    #[test]
    fn ten_locked_generations_coalesce_to_one_latest_workbook(){
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let old=fs::read(&f.job.path).unwrap();
        for n in 2..=11{
            f.update(n);
            assert!(publish_with(&mut f.conn,&f.job,&f.root,|s|if s=="before_replace"{Err("sharing violation fixture".into())}else{Ok(())}).is_err());
            assert_eq!(fs::read(&f.job.path).unwrap(),old);
        }
        let pending=jobs::jobs(&f.conn,0,false).unwrap();assert_eq!(pending.len(),1);
        publish(&mut f.conn,&pending[0],&f.root).unwrap();
        let mut book=Xlsx::new(Cursor::new(fs::read(&f.job.path).unwrap())).unwrap();
        assert_eq!(book.worksheet_range("로젠택배").unwrap().end().unwrap().0,11);
        assert!(jobs::jobs(&f.conn,0,false).unwrap().is_empty());
    }
    #[test]
    fn independent_connections_cannot_publish_a_stale_generation(){
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        f.update(2);
        let db=f.root.join("race.sqlite");
        let mut a=Connection::open(&db).unwrap();
        a.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT INTO core_schema VALUES('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let before:String=f.conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        a.execute("INSERT INTO app_state VALUES(1,?1)",[before]).unwrap();jobs::initialize(&a).unwrap();
        a.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision,saved_revision,file_hash)
            VALUES (?1,?2,?3,2,1,?4)",rusqlite::params![f.job.date,f.job.origin,f.job.path,f.job.hash]).unwrap();
        f.update(3);
        let newest:String=f.conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        let root=f.root.clone();let job=jobs::jobs(&a,0,false).unwrap().remove(0);
        let mut b=Connection::open(&db).unwrap();
        let mut swapped=false;
        publish_with(&mut a,&job,&root,|stage|{
            if stage=="before_commit_lock"&&!swapped{
                let tx=b.transaction().unwrap();
                tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",[&newest]).unwrap();
                tx.execute("UPDATE excel_aggregate_jobs SET desired_revision=3",[]).unwrap();tx.commit().unwrap();
                let pending=jobs::jobs(&b,0,false).unwrap().remove(0);
                publish(&mut b,&pending,&root).unwrap();swapped=true;
            }
            Ok(())
        }).unwrap();
        assert!(swapped);
        let j=jobs::current(&a,&job).unwrap().unwrap();
        assert_eq!((j.saved,j.desired),(3,3));
        let mut book=Xlsx::new(Cursor::new(fs::read(&j.path).unwrap())).unwrap();
        assert_eq!(book.worksheet_range("로젠택배").unwrap().end().unwrap().0,3);
    }
    #[test]
    fn next_day_reply_does_not_modify_previous_daily_file(){
        let mut f=Fixture::new();
        f.conn.execute("DELETE FROM excel_aggregate_jobs",[]).unwrap();
        let dates=["2026-11-27","2026-11-28"];
        let paths=dates.map(|d|jobs::target(&f.root,d,"REPLY").unwrap());
        let records:Vec<_>=dates.iter().enumerate().map(|(i,date)|{
            let mut values=vec!["".to_owned();17];values[0]="가상 수취인".into();
            serde_json::json!({"id":format!("reply-{i}"),"businessDate":date,"aggregateTargetPath":paths[i],
                "rows":[{"shipmentId":"s","applied":true,"tracking":format!("{:012}",i+1),"actual":3000,"aggregateValues":values}]})
        }).collect();
        for n in 1..=2{
            let state=serde_json::json!({"revision":n,"factoryReplies":records[..n]});
            f.conn.execute("INSERT INTO app_state VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",[state.to_string()]).unwrap();
            jobs::recover_history(&f.conn,Some(&f.root)).unwrap();
            let pending=jobs::jobs(&f.conn,0,false).unwrap();assert_eq!(pending.len(),1);
            assert_eq!(pending[0].date,dates[n-1]);
            publish(&mut f.conn,&pending[0],&f.root).unwrap();
            assert!(jobs::jobs(&f.conn,0,false).unwrap().is_empty());
        }
        for (i,path) in paths.iter().enumerate(){
            let mut book=Xlsx::new(Cursor::new(fs::read(path).unwrap())).unwrap();
            assert_eq!(book.worksheet_range("로젠택배").unwrap().get_value((1,16)),
                Some(&Data::String(format!("{:012}",i+1))));
        }
    }
    #[test]
    fn parallel_publisher_child(){
        let Some(root)=std::env::var_os("BANDO_AGGREGATE_PARALLEL_ROOT").map(PathBuf::from)else{return};
        let mut c=Connection::open(root.join("parallel.sqlite")).unwrap();
        c.busy_timeout(Duration::from_secs(10)).unwrap();
        let job=jobs::jobs(&c,0,false).unwrap().into_iter().next();
        if let Some(job)=job{publish(&mut c,&job,&root).unwrap();}
    }
    #[test]
    fn parallel_processes_publish_one_complete_generation(){
        use std::process::{Command,Stdio};
        let mut f=Fixture::new();f.update(100);
        let path=f.root.join("parallel.sqlite");
        let c=Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT INTO core_schema VALUES('business-core',2);CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let state:String=f.conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        c.execute("INSERT INTO app_state VALUES(1,?1)",[state]).unwrap();jobs::initialize(&c).unwrap();
        c.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision)
            VALUES (?1,?2,?3,100)",rusqlite::params![f.job.date,f.job.origin,f.job.path]).unwrap();
        drop(c);
        let mut children=(0..4).map(|_|Command::new(std::env::current_exe().unwrap())
            .args(["--exact","excel_aggregate::tests::parallel_publisher_child"])
            .env("BANDO_AGGREGATE_PARALLEL_ROOT",&f.root)
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()).collect::<Vec<_>>();
        let deadline=Instant::now()+Duration::from_secs(45);
        let mut statuses=Vec::new();
        for child in &mut children{
            let status=loop{
                if let Some(status)=child.try_wait().unwrap(){break status}
                if Instant::now()>=deadline{let _=child.kill();break child.wait().unwrap()}
                std::thread::sleep(Duration::from_millis(20));
            };
            statuses.push(status);
        }
        assert!(statuses.iter().all(|s|s.success()),"{statuses:?}");
        let c=Connection::open(path).unwrap();let j=jobs::current(&c,&f.job).unwrap().unwrap();
        assert_eq!((j.saved,j.desired),(100,100));
        let mut book=Xlsx::new(Cursor::new(fs::read(&j.path).unwrap())).unwrap();
        assert_eq!(book.worksheet_range("로젠택배").unwrap().end().unwrap().0,100);
        assert_eq!(fs::read_dir(Path::new(&j.path).parent().unwrap()).unwrap().count(),1);
    }
    #[test]
    fn same_revision_legacy_binding_change_prevents_stale_replace(){
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let old=fs::read(&f.job.path).unwrap();f.update(2);
        let db=f.root.join("binding.sqlite");let mut a=Connection::open(&db).unwrap();
        a.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT INTO core_schema VALUES('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let state:String=f.conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        a.execute("INSERT INTO app_state VALUES(1,?1)",[state]).unwrap();jobs::initialize(&a).unwrap();
        a.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision,saved_revision)
            VALUES (?1,?2,?3,2,1)",rusqlite::params![f.job.date,f.job.origin,f.job.path]).unwrap();
        let b=Connection::open(&db).unwrap();
        let j=jobs::jobs(&a,0,false).unwrap().remove(0);
        publish_with(&mut a,&j,&f.root,|stage|{
            if stage=="before_commit_lock"{
                b.execute("UPDATE excel_aggregate_jobs SET legacy_reply_ids=?1,saved_revision=0",
                    ["[\"new-binding\"]"]).unwrap();
            }
            Ok(())
        }).unwrap();
        assert_eq!(fs::read(&j.path).unwrap(),old);
        let pending=jobs::current(&a,&j).unwrap().unwrap();
        assert_eq!((pending.saved,pending.desired),(0,2));
        publish(&mut a,&pending,&f.root).unwrap();
        assert_eq!(jobs::current(&a,&j).unwrap().unwrap().saved,2);
    }
    #[test]
    fn durable_backoff_survives_connection_restart(){
        let mut f=Fixture::new();f.update(1);publish(&mut f.conn,&f.job,&f.root).unwrap();
        let old=fs::read(&f.job.path).unwrap();f.update(2);
        let path=f.root.join("retry.sqlite");let c=Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT INTO core_schema VALUES('business-core',2);CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let state:String=f.conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
        c.execute("INSERT INTO app_state VALUES(1,?1)",[state]).unwrap();jobs::initialize(&c).unwrap();
        c.execute("INSERT INTO excel_aggregate_jobs(business_date,origin,target_path,desired_revision,saved_revision)
            VALUES (?1,?2,?3,2,1)",rusqlite::params![f.job.date,f.job.origin,f.job.path]).unwrap();
        let j=jobs::jobs(&c,100,false).unwrap().remove(0);
        jobs::mark_failed(&c,&j,100,"offline").unwrap();drop(c);
        let mut c=Connection::open(&path).unwrap();
        assert!(jobs::jobs(&c,104,false).unwrap().is_empty());assert_eq!(fs::read(&j.path).unwrap(),old);
        let pending=jobs::jobs(&c,100_000,false).unwrap().remove(0);
        publish(&mut c,&pending,&f.root).unwrap();
        assert!(jobs::jobs(&c,100_000,false).unwrap().is_empty());
        assert_eq!(jobs::current(&c,&j).unwrap().unwrap().saved,2);
    }
    #[test]
    fn worker_does_not_block_egui_frames_while_publication_waits(){
        use std::sync::mpsc;
        let ctx=eframe::egui::Context::default();
        let (entered_tx,entered_rx)=mpsc::channel();
        let (release_tx,release_rx)=mpsc::channel();
        let worker=spawn_worker(&ctx,move||{
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut rendered=0;
        for _ in 0..10{
            let _=ctx.run(eframe::egui::RawInput::default(),|ctx|{
                eframe::egui::CentralPanel::default().show(ctx,|ui|{ui.label("업무 화면");rendered+=1;});
            });
        }
        release_tx.send(()).unwrap();worker.join().unwrap();
        assert!(rendered>=10);
    }
    #[test]
    fn isolated_archive_guard_rejects_other_workspace_before_creating_folders(){
        let f=Fixture::new();
        let other=f.root.join("isolated-test");fs::create_dir(&other).unwrap();
        let other=other.canonicalize().unwrap();
        assert!(checked_target_in(&f.job,Some(&other)).is_err());
        assert!(!Path::new(&f.job.path).parent().unwrap().exists());
        let mut isolated=f.job.clone();
        isolated.path=jobs::target(&other,&isolated.date,&isolated.origin).unwrap().to_str().unwrap().into();
        assert!(checked_target_in(&isolated,Some(&other)).is_ok());
        assert!(!Path::new(&f.job.path).parent().unwrap().exists());
    }
    #[test]
    fn large_daily_workbook_roundtrip_without_individual_files(){
        let mut f=Fixture::new();f.update(5000);
        let start=Instant::now();publish(&mut f.conn,&f.job,&f.root).unwrap();
        let elapsed=start.elapsed();
        let mut book=Xlsx::new(Cursor::new(fs::read(&f.job.path).unwrap())).unwrap();
        let range=book.worksheet_range("로젠택배").unwrap();
        assert_eq!(range.end().unwrap().0,5000);
        for (i,h) in crate::excel_service::HEADERS.iter().enumerate(){
            if h.is_empty(){continue}
            assert_eq!(range.get_value((0,i as u32)),Some(&Data::String((*h).into())));
        }
        assert_eq!(range.get_value((5000,1)),Some(&Data::String("01234".into())));
        assert_eq!(fs::read_dir(Path::new(&f.job.path).parent().unwrap()).unwrap().count(),1);
        eprintln!("synthetic 5000-row aggregate publish: {elapsed:?}");
    }
    #[test]
    fn same_directory_replacement_preserves_complete_bytes(){
        let d=std::env::temp_dir().join(format!("bando-aggregate-{}",Uuid::new_v4()));
        fs::create_dir(&d).unwrap();let p=d.join("daily.xlsx");let t=d.join("new.tmp");
        fs::write(&p,b"old").unwrap();fs::write(&t,b"new").unwrap();
        fs::rename(&t,&p).unwrap();assert_eq!(fs::read(&p).unwrap(),b"new");
        fs::remove_dir_all(d).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn windows_exclusive_handle_preserves_old_file_on_sharing_violation(){
        use std::os::windows::fs::OpenOptionsExt;
        let d=std::env::temp_dir().join(format!("bando-aggregate-lock-{}",Uuid::new_v4()));
        fs::create_dir(&d).unwrap();let p=d.join("daily.xlsx");let t=d.join("new.tmp");
        fs::write(&p,b"old").unwrap();fs::write(&t,b"new").unwrap();
        let lock=fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
        assert!(fs::rename(&t,&p).is_err());drop(lock);
        assert_eq!(fs::read(&p).unwrap(),b"old");fs::rename(&t,&p).unwrap();
        assert_eq!(fs::read(&p).unwrap(),b"new");fs::remove_dir_all(d).unwrap();
    }
}
