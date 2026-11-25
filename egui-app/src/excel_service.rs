//! Tauri-equivalent factory/store Excel archive for the native egui shell.
//! All shipment validation and export status decisions are delegated to the
//! original Rust output module; the UI never calculates those rules.
use crate::{business_state, gateway::output, settings_store};
use chrono::{Datelike,Local};
use rusqlite::{Connection,OpenFlags};
use rust_xlsxwriter::{Format,Workbook};
use serde_json::{json,Value};
use std::{fs,io::Write,path::{Path,PathBuf},time::Duration};
use uuid::Uuid;

#[path = "../../src-tauri/src/export_file.rs"]
mod recorded_export;
const HEADERS:[&str;16]=[
    "수하인명","수하인우편번호","수하인주소","수하인전화번호","수하인핸드폰번호",
    "택배수량","택배운임","운임구분","품목명","","메시지","송하인명",
    "(송)우편","(송)주소1","(송)전화","(송)핸드폰"
];
const WIDTHS:[f64;16]=[16.,14.,48.,18.,18.,12.,12.,12.,64.,3.,35.,18.,12.,48.,18.,18.];
fn kind(origin:&str)->Result<(&'static str,&'static str,&'static str),String>{
    match origin {
        "FACTORY"=>Ok(("prepare_factory_export","공장 발송","공장")),
        "STORE"=>Ok(("prepare_store_export","매장 발송","매장")),
        "PRODUCT"=>Ok(("","상품 관리","상품전체")),
        _=>Err("공장 또는 매장 발송만 선택할 수 있습니다.".into())
    }
}
fn archive_root(state:&Value)->Result<PathBuf,String>{
    let configured=state["settings"]["excelDirectory"].as_str()
        .map(str::trim).filter(|s|!s.is_empty())
        .ok_or("설정에서 Excel 보관함 위치를 먼저 지정하고 저장하세요.")?;
    let path=PathBuf::from(configured);
    if !path.is_absolute(){return Err("Excel 보관함은 절대 경로여야 합니다.".into())}
    let root=path.canonicalize().map_err(|_|"Excel 보관함 경로를 확인할 수 없습니다.")?;
    if !root.is_dir(){return Err("Excel 보관함이 폴더가 아닙니다.".into())}
    Ok(root)
}
fn archive_day(root:&Path,origin:&str)->Result<PathBuf,String>{
    let (_,category,_)=kind(origin)?;
    let now=Local::now();
    let dir=root.join(category).join(format!("{}년",now.year()))
        .join(format!("{:02}월",now.month())).join(format!("{:02}일",now.day()));
    fs::create_dir_all(&dir).map_err(|_|"Excel 날짜 폴더 생성 실패")?;
    let canonical=dir.canonicalize().map_err(|_|"Excel 날짜 폴더 확인 실패")?;
    if !canonical.starts_with(root)||!canonical.is_dir(){
        return Err("설정한 Excel 보관함의 외부 경로는 사용할 수 없습니다.".into())
    }
    Ok(canonical)
}
fn encode(preview:&Value)->Result<Vec<u8>,String>{
    let rows=preview["rows"].as_array().ok_or("Rust 출력 행이 없습니다.")?;
    let mut book=Workbook::new();
    let sheet=book.add_worksheet();
    sheet.set_name("로젠택배").map_err(|e|e.to_string())?;
    let header=Format::new().set_bold();
    for (col,title) in HEADERS.iter().enumerate(){
        sheet.write_string_with_format(0,col as u16,*title,&header)
            .map_err(|e|e.to_string())?;
        sheet.set_column_width(col as u16,WIDTHS[col])
            .map_err(|e|e.to_string())?;
    }
    for (i,row) in rows.iter().enumerate(){
        let values=row["values"].as_array().ok_or("로젠 출력값 누락")?;
        if values.len()!=16{return Err("로젠 Excel 열 수가 올바르지 않습니다.".into())}
        for (col,value) in values.iter().enumerate(){
            let str_value=value.as_str().ok_or("로젠 출력값 형식 오류")?;
            // Text-only cells preserve 010/020, postal codes and phone zeros.
            sheet.write_string((i+1) as u32,col as u16,str_value)
                .map_err(|e|e.to_string())?;
        }
    }
    sheet.set_freeze_panes(1,0).map_err(|e|e.to_string())?;
    sheet.autofilter(0,0,0,15).map_err(|e|e.to_string())?;
    book.save_to_buffer().map_err(|e|e.to_string())
}
fn save(root:&Path,origin:&str,bytes:&[u8])->Result<PathBuf,String>{
    if bytes.len()<4||bytes.len()>25*1024*1024||!bytes.starts_with(b"PK\x03\x04"){
        return Err("정상적인 Excel XLSX 파일이 아닙니다.".into())
    }
    let dir=archive_day(root,origin)?;
    let (_,_,stem)=kind(origin)?;
    let date=Local::now().format("%Y%m%d").to_string();
    for n in 1..=100_000_u32 {
        let path=dir.join(format!("{date}({stem}{n}).xlsx"));
        let mut file=match fs::OpenOptions::new().write(true).create_new(true).open(&path){
            Ok(f)=>f,
            Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,
            Err(_)=>return Err("Excel 파일을 생성하지 못했습니다.".into())
        };
        if file.write_all(bytes).and_then(|_|file.sync_all()).is_err(){
            drop(file);
            let _=fs::remove_file(&path);
            return Err("Excel 파일을 끝까지 기록하지 못했습니다.".into())
        }
        return Ok(path)
    }
    Err("당일 Excel 파일 번호가 허용 범위를 초과했습니다.".into())
}
fn conn()->Result<Connection,String>{
    let path=business_state::authorized_business_write_path()?;
    let db=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_|"운영 Excel 이력 DB를 열지 못했습니다.")?;
    db.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON;").map_err(|e|e.to_string())?;
    let version:i64=db.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|r|r.get(0)).map_err(|_|"업무 DB 스키마를 확인할 수 없습니다.")?;
    if version!=2{return Err("지원하지 않는 업무 DB 버전입니다.".into())}
    Ok(db)
}
/// Same date-classified Product workbook archive as Tauri's
/// save_product_sync_workbook (상품 관리/YYYY년/MM월/DD일).
/// This is a file-only export; it never changes the business SQLite state.
pub fn archive_product_workbook(state:&Value,bytes:&[u8])->Result<PathBuf,String>{
    if bytes.len()>crate::product_excel::MAX_BYTES as usize{
        return Err("상품 Excel 파일이 20MB를 초과했습니다.".into());
    }
    let root=archive_root(state)?;
    save(&root,"PRODUCT",bytes)
}

pub fn writable()->bool{
    std::env::var("BANDO_EGUI_ENABLE_EXCEL_WRITE").ok().as_deref()==Some("1") &&
    settings_store::writable_business() &&
    business_state::authorized_business_write_path().is_ok()
}
pub fn create(origin:&str,ids:&[String])->Result<Value,String>{
    // The PRODUCT archive uses only the shared file-naming helper; product
    // workbooks must never enter the factory/store shipment export contract.
    if !matches!(origin,"FACTORY"|"STORE"){
        return Err("공장 또는 매장 발송만 선택할 수 있습니다.".into());
    }
    if !writable(){return Err("Excel 출력 권한 또는 운영 DB 연결이 없습니다.".into())}
    if ids.is_empty()||ids.len()>500{return Err("배송 건 1~500개를 선택하세요.".into())}
    let (operation,_,_)=kind(origin)?;
    let state=business_state::load()?;
    let revision=state["revision"].as_u64().ok_or("업무 DB revision 누락")?;
    let preview=output::execute(operation,json!({"state":&state,"ids":ids}))?;
    let root=archive_root(&state)?;
    // Exact native manual backup before file write and DB transaction.
    settings_store::data("backup",None)?;
    let bytes=encode(&preview)?;
    let path=save(&root,origin,&bytes)?;
    let saved=path.to_string_lossy().to_string();
    let outcome=||->Result<Value,String>{
        let mut db=conn()?;
        let current=bando_data_access::read_v2_snapshot(&db)?;
        if current["revision"].as_u64()!=Some(revision){
            return Err("다른 작업으로 주문 데이터가 변경되었습니다.".into())
        }
        let result=output::execute("finalize_export",json!({
            "state":&current,"origin":origin,"preview":&preview,
            "savedPath":saved,"now":Local::now().to_rfc3339(),
            "exportId":Uuid::new_v4().to_string(),"eventId":Uuid::new_v4().to_string(),
        }))?;
        let mut next=result["next"].clone();
        next["revision"]=json!(revision);
        let commit=bando_data_access::write_v2_snapshot(&mut db,next,revision,true)?;
        let new_rev=commit["revision"].as_u64().ok_or("출력 이력 revision 누락")?;
        drop(db);
        let fresh=business_state::load()?;
        if fresh["revision"].as_u64()!=Some(new_rev){
            return Err("출력은 저장됐지만 최신 DB가 다시 변경됐습니다.".into())
        }
        Ok(json!({"state":fresh,"exportId":result["exportId"],"savedPath":saved,
            "count":preview["rows"].as_array().map_or(0,Vec::len),"origin":origin}))
    };
    outcome().map_err(|e|format!(
        "Excel 파일은 저장됐지만 출력 이력 기록에 실패했습니다. 이 파일을 사용하지 마세요: {saved} / {e}"
    ))
}
/// Only actions exposed by the original React Excel queue/history are
/// accepted. The original Rust reducer and workflow validator remain the
/// single authority. One SQLite CAS commit follows its verified output.
pub fn workflow_action(
    expected:u64,action:&str,shipment_id:&str,queue_id:&str,origin:&str,value:Value
)->Result<Value,String>{
    if !writable(){return Err("Excel 관리 변경 권한이 없습니다.".into())}
    if !matches!(action,
        "record_actual_freight"|"restore_shipment"|"mark_store_sent"|
        "mark_factory_delivered"|"reopen_unsent_export"){
        return Err("Excel 관리에서 허용되지 않은 업무 변경입니다.".into())
    }
    let mut db=conn()?;
    let state=bando_data_access::read_v2_snapshot(&db)?;
    let revision=state["revision"].as_u64().ok_or("업무 DB revision 누락")?;
    if revision!=expected{
        return Err("다른 작업에서 업무 데이터가 변경되었습니다. 목록을 다시 확인하세요.".into())
    }
    let now=Local::now().to_rfc3339();
    let actor=state["settings"]["operatorName"].as_str().unwrap_or("").trim();
    let actor=if actor.is_empty(){"사용자(이름 미설정)".to_owned()}else{actor.to_owned()};
    let before=state.clone();
    let mut source=state;
    if action=="restore_shipment"{
        // Original nativeReconcileWorkflow call precedes restore_shipment,
        // but NOT mark_store_sent nor direct actual-freight recording.
        let count=(12*source["workflow"]["shipments"].as_array().map_or(0,Vec::len)
            +8*source["orders"].as_array().map_or(0,Vec::len)+64).max(256);
        if count>15_000{return Err("재검증 이벤트가 너무 많습니다.".into())}
        let ids=(0..count).map(|_|Uuid::new_v4().to_string()).collect::<Vec<_>>();
        source=crate::gateway::rules::execute(
            "reconcile_workflow",json!({
                "state":&source,"now":now,"eventIds":ids
            })
        )?["state"].clone();
    }
    let payload=if matches!(action,"mark_factory_delivered"|"reopen_unsent_export"){
        json!({"state":&source,"action":action,"shipmentId":shipment_id,
            "now":now,"ids":(0..3).map(|_|Uuid::new_v4().to_string())
                .collect::<Vec<_>>()})
    }else{
        json!({
            "state":&source,"action":action,
            "shipmentId":shipment_id,"queueId":queue_id,
            "origin":origin,
            "value":value,"now":now,"actor":actor,
            "eventId":Uuid::new_v4().to_string(),
            "auditEventIds":(0..2048)
                .map(|_|Uuid::new_v4().to_string()).collect::<Vec<_>>(),
            "newGroupId":Uuid::new_v4().to_string(),
            "confirmationId":Uuid::new_v4().to_string(),
            "noticeId":Uuid::new_v4().to_string(),
        })
    };
    let operation=if matches!(action,"mark_factory_delivered"|"reopen_unsent_export"){
        "apply_correction_action"
    }else{"apply_workflow_action"};
    let result=crate::gateway::rules::execute(operation,payload)?;
    let next=result["state"].clone();
    crate::gateway::rules::execute("validate_workflow",
        json!({"state":&next,"before":&before}))?;
    let mut new=next;
    new["revision"]=json!(revision);
    let committed=bando_data_access::write_v2_snapshot(&mut db,new,revision,true)?;
    let current=committed["revision"].as_u64().ok_or("업무 저장 revision 누락")?;
    drop(db);
    let fresh=business_state::load()?;
    if fresh["revision"].as_u64()!=Some(current){
        return Err("업무 변경은 저장됐지만 최신 데이터가 달라졌습니다. 재조회하세요.".into())
    }
    let shipment_reads=crate::gateway::rules::execute("read_shipments",
        json!({"state":&fresh}))?;
    Ok(json!({"state":fresh,"shipmentReads":shipment_reads,
        "action":action,"queueId":queue_id,"shipmentId":shipment_id}))
}

pub fn open_file(id:&str)->Result<(),String>{
    let state=business_state::load()?;
    let path=recorded_export::resolve_export_path(&state,id)?;
    open::that(path).map_err(|_|"저장된 Excel 파일을 열지 못했습니다.".to_owned())
}
pub fn open_directory(id:&str)->Result<(),String>{
    let state=business_state::load()?;
    let path=recorded_export::resolve_export_directory(&state,id)?;
    open::that(path).map_err(|_|"Excel 파일 폴더를 열지 못했습니다.".to_owned())
}
pub fn open_daily(origin:&str)->Result<(),String>{
    let state=business_state::load()?;
    let root=archive_root(&state)?;
    let path=archive_day(&root,origin)?;
    open::that(path).map_err(|_|"당일 Excel 폴더를 열지 못했습니다.".to_owned())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn product_archive_preserves_date_folder_and_never_overwrites(){
        let root=std::env::temp_dir().join(format!(
            "bando-egui-product-archive-{}-{}",std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let canonical=root.canonicalize().unwrap();
        let bytes=crate::product_excel::export(&json!({
            "revision":1,"products":[],"categories":[],"brands":[]
        })).unwrap();
        let first=save(&canonical,"PRODUCT",&bytes).unwrap();
        let second=save(&canonical,"PRODUCT",&bytes).unwrap();
        assert_ne!(first,second);
        assert_eq!(first.parent(),second.parent());
        assert!(first.to_string_lossy().contains("상품 관리"));
        assert!(first.file_name().unwrap().to_string_lossy().contains("(상품전체1).xlsx"));
        assert!(second.file_name().unwrap().to_string_lossy().contains("(상품전체2).xlsx"));
        assert_eq!(std::fs::read(&first).unwrap(),bytes);
        assert_eq!(std::fs::read(&second).unwrap(),bytes);
        assert_eq!(crate::product_excel::read(&first).unwrap()["revision"],1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn logen_xlsx_keeps_all_columns_as_strings(){
        assert_eq!(HEADERS.len(),16);
        let values=vec![
            "홍길동","01234","서울","0123","01012345678","","","010",
            "◆ 709 1개","","메모","반도","00001","주소","02","010"
        ];
        let bytes=encode(&json!({"rows":[{"values":values}]})).unwrap();
        assert!(bytes.starts_with(b"PK\x03\x04"));
        // Reopen the exact output bytes, not just the ZIP signature.
        use calamine::{Data,Reader,Xlsx};
        let mut book=Xlsx::new(std::io::Cursor::new(bytes)).unwrap();
        let sheet=book.worksheet_range("로젠택배").unwrap();
        assert_eq!(sheet.width(),16);
        for (column,header) in HEADERS.iter().enumerate(){
            assert_eq!(sheet.get_value((0,column as u32)),Some(&Data::String((*header).to_string())));
        }
        for (column,expected) in values.iter().enumerate(){
            let cell=sheet.get_value((1,column as u32));
            if expected.is_empty(){
                assert!(matches!(cell,None|Some(Data::Empty)) ||
                    cell==Some(&Data::String(String::new())),"blank column {column}: {cell:?}");
            }else{
                assert_eq!(cell,Some(&Data::String((*expected).to_string())),
                    "text and leading zeros at column {column}");
            }
        }
    }
}
