//! Consistent read-only reconstruction of the Tauri v2 business snapshot.
//! This adapter is not an order writer. It cannot change app_state, relational
//! rows, receipts or the revision. On Windows, the existing Tauri app-data
//! path is discovered without creating any operational file. Release uses
//! the existing operating DB; debug builds never auto-discover it. Explicit
//! isolated unit/integration fixtures remain available to the test harness.

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

#[path="test_workspace_init.rs"]
mod test_workspace_init;
#[path="developer_ui_samples.rs"]
mod developer_ui_samples;

/// The Tauri app-data path is RoamingAppData/{identifier}. Read the identifier
/// from the exact bundled Tauri configuration rather than making up a second
/// egui directory. This function is pure, so path precedence can be tested
/// without touching the actual employee files.
fn tauri_database_path(roaming:&std::path::Path)->Result<PathBuf,String>{
    let config:Value=serde_json::from_str(include_str!("../../src-tauri/tauri.conf.json"))
        .map_err(|_|"Tauri 프로그램 식별자 설정을 읽지 못했습니다.".to_owned())?;
    if config.pointer("/app/appDirectoriesOverride").is_some_and(|v|!v.is_null()){
        return Err("Tauri 데이터 폴더가 별도로 지정돼 있어 자동 연결을 중단했습니다.".into());
    }
    let identifier=config.get("identifier").and_then(Value::as_str)
        .filter(|id|!id.is_empty() && !id.contains('/') && !id.contains('\\'))
        .ok_or("Tauri 프로그램 식별자가 올바르지 않습니다.")?;
    Ok(roaming.join(identifier).join("bando-hub-v001.sqlite"))
}

// Compatibility entry for existing business callers; the launch profile
// is shared with native window persistence, not owned by the database.
pub use crate::launch_profile::test_launch;
fn test_database_path(roaming:&std::path::Path)->Result<PathBuf,String>{
    let production=tauri_database_path(roaming)?;
    Ok(production.parent().ok_or("업무 DB 폴더 오류")?
        .join("developer-test").join("bando-hub-test.sqlite"))
}
fn checked_test_database(roaming:&std::path::Path)->Result<PathBuf,String>{
    let production=tauri_database_path(roaming)?;
    let root=production.parent().ok_or("업무 DB 폴더 오류")?
        .canonicalize().map_err(|_|"업무 DB 폴더 확인 실패")?;
    let expected=root.join("developer-test").join("bando-hub-test.sqlite");
    let db=test_database_path(roaming)?;
    let actual=db.canonicalize().map_err(|_|"기존 테스트 DB가 없습니다. 운영 DB를 사용하지 않습니다.")?;
    if actual!=expected || !actual.is_file() ||
        production.canonicalize().ok().as_ref()==Some(&actual){
        return Err("테스트 DB 경로가 격리된 위치와 일치하지 않습니다.".into());
    }
    Ok(actual)
}
fn test_path()->Result<PathBuf,String>{
    let roaming=std::env::var_os("APPDATA").ok_or("APPDATA 누락")?;
    checked_test_database(std::path::Path::new(&roaming))
}
pub fn test_directory(name:&str)->Result<PathBuf,String>{
    if !matches!(name,"Excel"|"factory-inbox"|"order-image-inbox"|"preview"){
        return Err("허용되지 않은 테스트 폴더".into());
    }
    let db=authorized_business_write_path()?;
    let root=db.parent().ok_or("테스트 DB 폴더 오류")?;
    let folder=root.join(name);
    std::fs::create_dir_all(&folder).map_err(|e|e.to_string())?;
    let actual=folder.canonicalize().map_err(|e|e.to_string())?;
    if actual!=folder || !actual.is_dir(){return Err("테스트 폴더 격리 위반".into());}
    Ok(actual)
}
fn prepare_test_directories(db:&PathBuf)->Result<(),String>{
    let mut conn=Connection::open_with_flags(db,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e|e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON").map_err(|e|e.to_string())?;
    let state=bando_data_access::read_v2_settings_snapshot(&conn)?;
    let mut changes=serde_json::Map::new();
    let mut expected=serde_json::Map::new();
    for (key,name) in [("excelDirectory","Excel"),("factoryReplyDirectory","factory-inbox"),
        ("orderImageInboxDirectory","order-image-inbox")]{
        let dir=test_directory(name)?;
        let value=Value::String(dir.to_string_lossy().into_owned());
        let old=state["settings"].get(key).unwrap_or(&Value::Null);
        if old!=&value{
            changes.insert(key.into(),value);
            expected.insert(key.into(),old.clone());
        }
    }
    if !changes.is_empty(){
        // Only the already-validated isolated DB is touched, before workers.
        bando_data_access::patch_settings_v2(&mut conn,false,&Value::Object(changes),&Value::Object(expected))?;
    }
    Ok(())
}

/// A developer test workspace is not a valid operating database.
/// Check both lexical and canonical paths (including aliases/junctions).
fn retired_workspace_path(path:&std::path::Path)->bool{
    path.components().any(|part|{
        let name=part.as_os_str().to_string_lossy();
        name.eq_ignore_ascii_case("developer-test") ||
        name.eq_ignore_ascii_case("bando-hub-test.sqlite")
    })
}
fn reject_retired_workspace(path:&std::path::Path)->Result<(),String>{
    if retired_workspace_path(path) ||
        path.canonicalize().ok().is_some_and(|p|retired_workspace_path(&p)){
        return Err("폐기된 개발자 DB는 운영 DB로 사용할 수 없습니다.".into());
    }
    Ok(())
}

/// Existing DB read-only auto-binding happens once at the app entrypoint,
/// before any workers start. Explicit configured DB/index or HTTP gateway wins.
/// Debug builds cannot silently bind the production database.
/// No schema creation, copies, migrations or edits are performed here.
pub fn bind_existing_tauri_database() -> Result<Option<PathBuf>,String>{
    if std::env::var("BANDO_EGUI_DEVELOPER_TEST").ok().as_deref()==Some("1"){
        if !test_launch(){return Err("테스트 실행은 Windows 디버그 빌드에서만 허용됩니다.".into());}
        let roaming=std::env::var_os("APPDATA").ok_or("APPDATA 누락")?;
        let candidate=test_database_path(std::path::Path::new(&roaming))?;
        if !candidate.exists(){
            // Never clone the operating DB. Only a new dedicated test file
            // from the bundled seed may be created, under a verified root.
            let operating=tauri_database_path(std::path::Path::new(&roaming))?;
            let base=operating.parent().ok_or("업무 DB 폴더 오류")?;
            std::fs::create_dir_all(base).map_err(|e|e.to_string())?;
            let canonical_base=base.canonicalize().map_err(|e|e.to_string())?;
            let dir=canonical_base.join("developer-test");
            std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
            if dir.canonicalize().map_err(|e|e.to_string())?!=dir{
                return Err("테스트 폴더가 격리 경로와 일치하지 않습니다.".into());
            }
            test_workspace_init::initialize_new(&dir.join("bando-hub-test.sqlite"))?;
        }
        let db=test_path()?;
        // Ignore inherited operational paths, indexes and remote gateways.
        std::env::set_var("BANDO_HUB_GATEWAY","local");
        std::env::remove_var("BANDO_EGUI_CLIENT_INDEX");
        std::env::set_var("BANDO_EGUI_BUSINESS_DB",&db);
        prepare_test_directories(&db)?;
        developer_ui_samples::seed_if_requested()?;
        return Ok(Some(db));
    }
    if !cfg!(windows) ||
        std::env::var("BANDO_HUB_GATEWAY")
            .ok().is_some_and(|mode|mode.trim().eq_ignore_ascii_case("http")){
        return Ok(None);
    }
    if let Some(path)=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
        .filter(|v|!v.is_empty()){
        let path=PathBuf::from(path);
        reject_retired_workspace(&path)?;
        return Ok(Some(path));
    }
    if std::env::var_os("BANDO_EGUI_CLIENT_INDEX")
        .is_some_and(|v|!v.is_empty()){
        return Ok(None);
    }
    let Some(roaming)=std::env::var_os("APPDATA").filter(|v|!v.is_empty()) else{
        return Ok(None);
    };
    if cfg!(debug_assertions){return Ok(None);}
    let path=tauri_database_path(std::path::Path::new(&roaming))?;
    if !path.is_file(){
        return Ok(None);
    }
    // A malformed existing file is NOT a reason to silently load a preview
    // seed. Fail here without touching it; Tauri owns all migrations.
    let connection=Connection::open_with_flags(
        &path,OpenFlags::SQLITE_OPEN_READ_ONLY
    ).map_err(|_|"기존 Tauri 업무 DB를 읽기 전용으로 열지 못했습니다.".to_owned())?;
    connection.execute_batch("PRAGMA query_only=ON;")
        .map_err(|_|"기존 업무 DB를 읽기 전용으로 검증하지 못했습니다.".to_owned())?;
    let version:i64=connection.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)
    ).map_err(|_|"기존 Tauri DB의 업무 스키마를 확인하지 못했습니다.".to_owned())?;
    if version!=2{
        return Err("기존 Tauri DB의 업무 스키마 버전이 지원되지 않습니다. 자동 변경하지 않았습니다.".into());
    }
    drop(connection);
    std::env::set_var("BANDO_EGUI_BUSINESS_DB",&path);
    Ok(Some(path))
}

/// Client writes require a pre-existing explicitly bound business database.
/// Debug application builds are read-only; compiled test harnesses retain
/// explicitly supplied isolated fixtures. No retired developer workspace.
pub fn authorized_business_write_path() -> Result<PathBuf,String> {
    let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
        .filter(|v|!v.is_empty()).map(PathBuf::from)
        .ok_or("실제 업무 DB가 연결되지 않았습니다.")?;
    if std::env::var_os("BANDO_EGUI_CLIENT_INDEX").is_some_and(|v|!v.is_empty()){
        return Err("거래처 JSON 인덱스를 운영 DB 저장과 함께 사용할 수 없습니다.".into());
    }
    if !path.is_file(){
        return Err("지정한 업무 DB 파일이 없습니다. 새 DB를 만들지 않았습니다.".into());
    }
    if test_launch(){
        let actual=path.canonicalize().map_err(|e|e.to_string())?;
        if actual!=test_path()?{return Err("테스트 실행은 격리 테스트 DB에만 저장할 수 있습니다.".into());}
        return Ok(actual);
    }
    reject_retired_workspace(&path)?;
    if cfg!(debug_assertions) && !cfg!(test){
        return Err("디버그 빌드에서는 운영 DB에 저장할 수 없습니다. Release 실행파일을 사용하세요.".into());
    }
    Ok(path)
}

/// Explicit startup maintenance, separate from read-only DB binding.
/// Existing DB only, with verified backup and revision-CAS in the shared
/// data-access migration. Never run for an ordinary debug or HTTP session.
pub fn migrate_bound_product_colors() -> Result<usize,String> {
    let path=authorized_business_write_path()?;
    let root=path.parent().ok_or("상품 색상 정리 백업 경로를 확인할 수 없습니다.")?;
    let mut conn=Connection::open_with_flags(&path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e|e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON").map_err(|e|e.to_string())?;
    bando_data_access::product_colors::migrate_v2(&mut conn,root)
}

/// Backward-compatible client adapter; all direct writers use the same guard.
pub fn authorized_client_write_path() -> Result<PathBuf,String> {
    authorized_business_write_path()
}

/// The live catalog requires a real v2 Tauri DB. A seed fallback would mix
/// unrelated product IDs with prices and freight rules.
pub fn load() -> Result<Value,String> {
    load_projection(ReadScope::Full)
}

/// The operational catalog does not require order/recipient/item records.
/// Keep the same path, schema, revision and read-only transaction checks as
/// the full business snapshot, but avoid loading unrelated customer orders.
pub fn load_catalog() -> Result<Value,String> {
    load_projection(ReadScope::Catalog)
}
pub fn load_reception_catalog()->Result<Value,String>{
    load_projection(ReadScope::ReceptionCatalog)
}

pub fn load_settings_metadata()->Result<Value,String>{
    load_projection(ReadScope::Settings)
}

pub fn load_export_metadata()->Result<Value,String>{
    load_projection(ReadScope::Exports)
}

enum ReadScope{Full,Catalog,ReceptionCatalog,Settings,Exports}

fn open_business_read_snapshot()->Result<Connection,String>{
    let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
        .filter(|p|!p.is_empty())
        .map(PathBuf::from)
        .ok_or("기존 운영 DB를 찾지 못했습니다. 원본 DB를 새로 생성하거나 초기화하지 않았습니다.")?;
    if test_launch(){
        if path.canonicalize().map_err(|e|e.to_string())?!=test_path()?{
            return Err("테스트 실행은 격리 테스트 DB만 조회할 수 있습니다.".into());
        }
    }else{reject_retired_workspace(&path)?;}
    if std::env::var_os("BANDO_EGUI_CLIENT_INDEX").is_some_and(|p|!p.is_empty()) {
        return Err("JSON 거래처 인덱스는 실제 주문 가격 변경의 근거가 될 수 없습니다. 운영 DB 읽기 전용 연결을 사용하세요.".into());
    }
    if !path.is_file() {return Err("연결한 운영 DB 파일을 찾지 못했습니다.".into());}
    let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_|"운영 DB를 읽기 전용으로 열지 못했습니다.".to_owned())?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=2000; BEGIN DEFERRED TRANSACTION;")
        .map_err(|_|"운영 DB 읽기 전용 트랜잭션을 열지 못했습니다.".to_owned())?;
    // Keep all table reads inside ONE consistent snapshot. Concurrent
    // production writes cannot mix old orders with new prices.
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)
    ).map_err(|_|"운영 DB 스키마 버전을 확인하지 못했습니다.".to_owned())?;
    if version!=2 {return Err("지원되지 않는 운영 DB 스키마입니다. 아무 것도 저장하지 않았습니다.".into());}
    Ok(conn)
}

fn load_projection(scope:ReadScope) -> Result<Value,String> {
    let conn=open_business_read_snapshot()?;
    // Share the same app_state and catalog-hydration rules with Tauri.
    // Only the full business read needs order, recipient and item tables.
    let state=match scope{
        ReadScope::Full=>bando_data_access::read_v2_snapshot(&conn)?,
        ReadScope::Catalog=>bando_data_access::read_v2_catalog_snapshot(&conn)?,
        ReadScope::ReceptionCatalog=>{
            let mut catalog=bando_data_access::read_v2_catalog_snapshot(&conn)?;
            // Only compact match metadata, not recipients/items or raw orders.
            let mut stmt=conn.prepare("SELECT json_extract(payload,'$.clientId'),
                json_extract(payload,'$.clientMatch.chatTitle'), COUNT(*)
                FROM core_orders
                WHERE coalesce(json_extract(payload,'$.confirmedAt'),'')<>'' 
                AND coalesce(json_extract(payload,'$.deletedAt'),'')=''
                AND json_extract(payload,'$.clientMatch.source') IN ('KAKAO_TITLE','KAKAO_REMEMBERED')
                AND coalesce(json_extract(payload,'$.clientMatch.chatTitle'),'')<>''
                GROUP BY 1,2").map_err(|e|e.to_string())?;
            let records=stmt.query_map([],|row|Ok((row.get::<_,String>(0)?,
                row.get::<_,String>(1)?,row.get::<_,u64>(2)?))).map_err(|e|e.to_string())?;
            let mut counts=serde_json::Map::new();
            for record in records{
                let (id,title,count)=record.map_err(|e|e.to_string())?;
                let title=crate::gateway::kakao_title_key(&title);
                let entry=counts.entry(title).or_insert_with(||serde_json::json!({}));
                let previous=entry.get(&id).and_then(Value::as_u64).unwrap_or(0);
                entry[&id]=serde_json::json!(previous.saturating_add(count));
            }
            catalog["confirmedTitleCounts"]=Value::Object(counts);
            catalog
        },
        ReadScope::Settings=>bando_data_access::read_v2_settings_snapshot(&conn)?,
        ReadScope::Exports=>bando_data_access::read_v2_export_metadata(&conn)?,
    };
    let revision=state.get("revision").and_then(Value::as_u64)
        .ok_or("운영 DB revision을 확인할 수 없습니다.")?;
    if revision==u64::MAX{return Err("운영 DB revision 범위를 확인할 수 없습니다.".into());}
    conn.execute_batch("COMMIT")
        .map_err(|_|"운영 DB 읽기 트랜잭션을 완료하지 못했습니다.".to_owned())?;
    Ok(state)
}


/// Explicit diagnostic only. Fetch exactly the requested order IDs, not the
/// full ledger or its live recipients/items. The frozen snapshot remains the
/// sole authority for a historical recognition comparison.
fn owner_snapshot_batch(conn:&Connection,ids:&[String])->Result<Vec<Value>,String>{
    let unique=ids.iter().map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if unique.is_empty()||ids.len()>250||unique.iter().any(|id|id.trim().is_empty()){
        return Err("귀속 통계는 1~250개 주문 ID를 명시해야 합니다.".into());
    }
    let placeholders=vec!["?";unique.len()].join(",");
    let sql=format!("SELECT id,confirmed_at,json_extract(payload,'$.deletedAt'),
        json_extract(payload,'$.confirmationSnapshot') FROM core_orders
        WHERE id IN ({placeholders}) ORDER BY id");
    let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
    let records=stmt.query_map(rusqlite::params_from_iter(unique.iter().copied()),|r|
        Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,
            r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?)))
        .map_err(|e|e.to_string())?;
    let mut orders=Vec::new();
    let mut snapshot_bytes=0usize;
    for row in records{
        let (id,confirmed_at,deleted_at,snapshot)=row.map_err(|e|e.to_string())?;
        snapshot_bytes=snapshot_bytes.saturating_add(snapshot.as_ref().map_or(0,String::len));
        if snapshot_bytes>32*1024*1024{
            return Err("확정 스냅샷 조회 한도 초과: 주문 범위를 줄여 다시 조회하세요.".into());
        }
        let snapshot:Value=match snapshot{
            Some(s)=>serde_json::from_str(&s).map_err(|_|"확정 스냅샷 형식 오류")?,
            None=>Value::Null
        };
        orders.push(serde_json::json!({
            "id":id,"confirmedAt":confirmed_at,"deletedAt":deleted_at,
            "confirmationSnapshot":snapshot
        }));
    }
    Ok(orders)
}
// The original frozen confirmation is retained even after stage-one undo.
 // Only currently active, fully stage-one-approved, non-archived shipments
 // contribute a whole-order sample. A partly deleted/undone split order is
 // excluded until all its live shipments are eligible again.
fn attach_learning_lifecycle(conn:&Connection,orders:&mut [Value])->Result<(),String>{
    // v2 keeps workflow groups and shipments in the physical app_state
    // envelope. open_business_read_snapshot rejects other schema versions.
    let valid:i64=conn.query_row("SELECT count(*) FROM app_state WHERE id=1
        AND json_type(payload,'$.workflow.groups')='array'
        AND json_type(payload,'$.workflow.shipments')='array'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if valid!=1{return Err("학습 주문 업무 단계 자료 누락".into());}
    let ids=orders.iter().map(|o|o["id"].as_str().ok_or("학습 주문 ID 누락"))
        .collect::<Result<std::collections::BTreeSet<_>,_>>()?;
    let mut eligibility=std::collections::BTreeMap::<String,bool>::new();
    if !ids.is_empty(){
        let placeholders=vec!["?";ids.len()].join(",");
        let sql=format!("WITH groups AS (
            SELECT json_extract(g.value,'$.id') AS id,
              coalesce(json_extract(g.value,'$.deletedAt'),'') AS deleted
            FROM app_state a,json_each(a.payload,'$.workflow.groups') g WHERE a.id=1),
            shipments AS (
            SELECT json_extract(s.value,'$.orderId') AS order_id,
              json_extract(s.value,'$.groupId') AS group_id,
              json_extract(s.value,'$.step') AS step
            FROM app_state a,json_each(a.payload,'$.workflow.shipments') s
            WHERE a.id=1 AND coalesce(json_extract(s.value,'$.legacyArchived'),0)=0)
            SELECT s.order_id,MIN(CASE WHEN g.id IS NOT NULL AND g.deleted=''
              AND typeof(s.step)='integer' AND s.step BETWEEN 1 AND 4 THEN 1 ELSE 0 END)
            FROM shipments s LEFT JOIN groups g ON g.id=s.group_id
            WHERE s.order_id IN ({placeholders}) GROUP BY s.order_id");
        let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(rusqlite::params_from_iter(ids.iter().copied()),|r|
            Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).map_err(|e|e.to_string())?;
        for row in rows{let (id,active)=row.map_err(|e|e.to_string())?;eligibility.insert(id,active==1);}
    }
    drop(ids);
    for order in orders{
        let active=order["id"].as_str().and_then(|id|eligibility.get(id)).copied().unwrap_or(false);
        order["learningActive"]=Value::Bool(active);
    }
    Ok(())
}

#[derive(Debug,PartialEq,Eq)]
pub(crate) struct LearningSource{
    pub active:bool,
    pub audit_needed:bool,
    pub digest:String,
}
pub(crate) type LearningSources=std::collections::BTreeMap<String,LearningSource>;

/// Hash SQLite's borrowed text bytes before any serde JSON allocation.
pub(crate) fn hash_learning_text(hash:&mut sha2::Sha256,value:rusqlite::types::ValueRef<'_>)->Result<usize,String>{
    use sha2::Digest;
    match value{
        rusqlite::types::ValueRef::Null=>{hash.update([0]);Ok(0)}
        rusqlite::types::ValueRef::Text(bytes)=>{
            hash.update([1]);hash.update((bytes.len() as u64).to_le_bytes());hash.update(bytes);Ok(bytes.len())
        }
        _=>Err("학습 원본 SQLite 자료 유형 오류".into()),
    }
}

fn learning_sources(conn:&Connection,ids:&[String])->Result<LearningSources,String>{
    use sha2::{Digest,Sha256};
    let unique=ids.iter().map(String::as_str).collect::<std::collections::BTreeSet<_>>();
    if unique.is_empty()||ids.len()>250||unique.iter().any(|id|id.trim().is_empty()){
        return Err("학습 조회는 1~250개 주문 ID를 명시해야 합니다.".into());
    }
    let placeholders=vec!["?";unique.len()].join(",");
    let sql=format!("SELECT id,confirmed_at,json_extract(payload,'$.deletedAt'),
        json_extract(payload,'$.confirmationSnapshot'),
        coalesce(json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.schemaVersion')=1,0)
        FROM core_orders WHERE id IN ({placeholders}) ORDER BY id");
    let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
    let mut rows=stmt.query(rusqlite::params_from_iter(unique.iter().copied())).map_err(|e|e.to_string())?;
    let mut sources=LearningSources::new();
    let mut metadata=Vec::new();
    let mut bytes=0usize;
    while let Some(row)=rows.next().map_err(|e|e.to_string())?{
        let id:String=row.get(0).map_err(|e|e.to_string())?;
        let deleted:Option<String>=row.get(2).map_err(|e|e.to_string())?;
        let schema:i64=row.get(4).map_err(|e|e.to_string())?;
        let mut hash=Sha256::new();hash.update(b"bando-business-learning-source-v1\0");
        for col in 0..4{
            bytes=bytes.saturating_add(hash_learning_text(&mut hash,row.get_ref(col).map_err(|e|e.to_string())?)?);
            if bytes>32*1024*1024{return Err("학습 스냅샷 조회 한도 초과".into());}
        }
        metadata.push(serde_json::json!({"id":id}));
        sources.insert(id,LearningSource{active:false,audit_needed:schema==1 &&
            deleted.as_deref().is_none_or(str::is_empty),digest:format!("{:x}",hash.finalize())});
    }
    drop(rows);drop(stmt);
    attach_learning_lifecycle(conn,&mut metadata)?;
    for row in metadata{
        let id=row["id"].as_str().ok_or("학습 주문 ID 누락")?;
        let source=sources.get_mut(id).ok_or("학습 주문 연결 오류")?;
        source.active=row["learningActive"]==true;
        source.audit_needed&=source.active;
    }
    Ok(sources)
}

/// Select cache misses and decode only those, inside the SAME business
/// snapshot as the raw fingerprints. The caller may hold an audit write
/// transaction; this connection never writes to the business database.
pub(crate) fn read_learning_inputs<T>(ids:&[String],
    choose:impl FnOnce(&LearningSources)->Result<(Vec<String>,T),String>
)->Result<(LearningSources,Vec<Value>,T),String>{
    let conn=open_business_read_snapshot()?;
    let result=learning_inputs_from_snapshot(&conn,ids,choose)?;
    conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    Ok(result)
}

// The caller owns the read transaction. Shared with isolated integration tests.
pub(crate) fn learning_inputs_from_snapshot<T>(conn:&Connection,ids:&[String],
    choose:impl FnOnce(&LearningSources)->Result<(Vec<String>,T),String>
)->Result<(LearningSources,Vec<Value>,T),String>{
    let sources=learning_sources(conn,ids)?;
    let (misses,extra)=choose(&sources)?;
    if misses.iter().any(|id|!sources.get(id).is_some_and(|s|s.active)){
        return Err("학습 캐시 미스 범위 오류".into());
    }
    let mut orders=if misses.is_empty(){Vec::new()}else{owner_snapshot_batch(conn,&misses)?};
    for order in &mut orders{order["learningActive"]=Value::Bool(true);}
    if orders.len()!=misses.len(){return Err("학습 캐시 미스 주문 누락".into());}
    Ok((sources,orders,extra))
}

pub(crate) fn read_learning_sources(ids:&[String])->Result<LearningSources,String>{
    let conn=open_business_read_snapshot()?;
    let sources=learning_sources(&conn,ids)?;
    conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    Ok(sources)
}

pub(crate) fn learning_order_page(after:&str,limit:usize)->Result<Vec<String>,String>{
    if !(1..=25).contains(&limit){return Err("학습 주문 페이지 범위 오류".into());}
    let conn=open_business_read_snapshot()?;
    let result={
        let mut stmt=conn.prepare("SELECT id FROM core_orders WHERE id>?1
            AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.schemaVersion')=1
            ORDER BY id LIMIT ?2").map_err(|e|e.to_string())?;
        let rows=stmt.query_map(rusqlite::params![after,limit as i64],|r|r.get::<_,String>(0))
            .map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
    };
    conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    Ok(result)
}

pub(crate) fn read_owner_snapshots(ids:&[String])->Result<Vec<Value>,String>{
    let conn=open_business_read_snapshot()?;
    let orders=owner_snapshot_batch(&conn,ids)?;
    conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    // Release the SQLite snapshot before CPU comparison and aggregation.
    Ok(orders)
}


#[cfg(test)]
mod auto_database_location_tests {
    use super::*;
    #[test]
    fn learning_raw_snapshot_digest_tracks_content_and_approval(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT NOT NULL);
            CREATE TABLE core_orders(id TEXT PRIMARY KEY,confirmed_at TEXT,payload TEXT)").unwrap();
        let mut workflow=serde_json::json!({"workflow":{"groups":[{"id":"g"}],
            "shipments":[{"orderId":"a","groupId":"g","step":1}]}});
        conn.execute("INSERT INTO app_state VALUES(1,?1)",[workflow.to_string()]).unwrap();
        let mut order=serde_json::json!({"confirmationSnapshot":{"raw":"a",
            "recognitionBaseline":{"schemaVersion":1}}});
        conn.execute("INSERT INTO core_orders VALUES('a','t',?1)",[order.to_string()]).unwrap();
        let ids=vec!["a".into()];let first=learning_sources(&conn,&ids).unwrap();
        assert!(first["a"].active&&first["a"].audit_needed);
        assert_eq!(learning_sources(&conn,&ids).unwrap(),first);
        order["confirmationSnapshot"]["raw"]=serde_json::json!("b");
        conn.execute("UPDATE core_orders SET payload=?1",[order.to_string()]).unwrap();
        let changed=learning_sources(&conn,&ids).unwrap();
        assert_ne!(changed["a"].digest,first["a"].digest);
        workflow["workflow"]["shipments"][0]["step"]=serde_json::json!(0);
        conn.execute("UPDATE app_state SET payload=?1",[workflow.to_string()]).unwrap();
        let inactive=learning_sources(&conn,&ids).unwrap();
        assert_eq!(inactive["a"].digest,changed["a"].digest);
        assert!(!inactive["a"].active&&!inactive["a"].audit_needed);
        assert_ne!(inactive,changed);
    }
    #[test]
    fn learning_lifecycle_excludes_cancelled_deleted_and_partial_orders(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT NOT NULL)").unwrap();
        let mut state=serde_json::json!({"workflow":{"groups":[{"id":"g"},{"id":"h"}],
            "shipments":[{"orderId":"a","groupId":"g","step":1},
                {"orderId":"a","groupId":"h","step":2},
                {"orderId":"b","groupId":"g","step":0},
                {"orderId":"c","groupId":"missing","step":2},
                {"orderId":"d","groupId":"g","step":0,"legacyArchived":true}]}});
        conn.execute("INSERT INTO app_state VALUES(1,?1)",[state.to_string()]).unwrap();
        let mut orders=vec![serde_json::json!({"id":"a"}),serde_json::json!({"id":"b"}),
            serde_json::json!({"id":"c"}),serde_json::json!({"id":"d"})];
        attach_learning_lifecycle(&conn,&mut orders).unwrap();
        assert_eq!(orders.iter().map(|o|o["learningActive"].as_bool().unwrap()).collect::<Vec<_>>(),
            vec![true,false,false,false]);
        state["workflow"]["groups"][1]["deletedAt"]=serde_json::json!("t");
        conn.execute("UPDATE app_state SET payload=?1",[state.to_string()]).unwrap();
        attach_learning_lifecycle(&conn,&mut orders).unwrap();
        assert_eq!(orders[0]["learningActive"],false);
        state["workflow"]["groups"][1]["deletedAt"]=Value::Null;
        state["workflow"]["shipments"][1]["step"]=serde_json::json!(0);
        conn.execute("UPDATE app_state SET payload=?1",[state.to_string()]).unwrap();
        attach_learning_lifecycle(&conn,&mut orders).unwrap();
        assert_eq!(orders[0]["learningActive"],false);
        state["workflow"]["shipments"][1]["step"]=serde_json::json!(1);
        conn.execute("UPDATE app_state SET payload=?1",[state.to_string()]).unwrap();
        attach_learning_lifecycle(&conn,&mut orders).unwrap();
        assert_eq!(orders[0]["learningActive"],true);
    }
    #[test]
    fn owner_snapshot_batch_reads_only_requested_frozen_snapshots(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_orders(
            id TEXT PRIMARY KEY,confirmed_at TEXT,payload TEXT NOT NULL
        );").unwrap();
        for id in ["a","b"]{
            let payload=serde_json::json!({
                "raw":"live raw not needed","confirmationSnapshot":{"at":"t","raw":id}
            });
            conn.execute("INSERT INTO core_orders VALUES (?1,'t',?2)",
                rusqlite::params![id,payload.to_string()]).unwrap();
        }
        conn.execute_batch("PRAGMA query_only=ON;").unwrap();
        let orders=owner_snapshot_batch(&conn,&["b".into(),"b".into()]).unwrap();
        assert_eq!(orders.len(),1);
        assert_eq!(orders[0]["id"],"b");
        assert_eq!(orders[0]["confirmationSnapshot"]["raw"],"b");
        assert!(orders[0].get("raw").is_none());
        assert!(owner_snapshot_batch(&conn,&[]).is_err());
    }
    #[test]
    fn resolves_only_the_existing_operating_database(){
        let root=std::path::Path::new("roaming-root");
        let production=tauri_database_path(root).unwrap();
        assert!(production.ends_with(
            std::path::Path::new("com.bandostand.bandohub.tauri")
                .join("bando-hub-v001.sqlite")));
        assert!(!retired_workspace_path(&production));
    }
    #[test]
    fn developer_test_location_is_distinct_from_operating_database(){
        let root=std::path::Path::new("roaming-root");
        let test=test_database_path(root).unwrap();
        assert!(test.ends_with("developer-test/bando-hub-test.sqlite"));
        assert_ne!(test,tauri_database_path(root).unwrap());
        assert!(reject_retired_workspace(&test).is_err());
    }
    #[test]
    fn retired_workspace_cannot_be_selected_as_operating_database(){
        let root=std::path::Path::new("roaming-root");
        assert!(reject_retired_workspace(&root.join("developer-test").join("other.sqlite")).is_err());
        assert!(reject_retired_workspace(&root.join("BANDO-HUB-TEST.SQLITE")).is_err());
        assert!(reject_retired_workspace(&root.join("bando-freight-gateway-e2e.sqlite")).is_ok());
    }
}
