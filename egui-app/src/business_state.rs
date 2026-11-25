//! Consistent read-only reconstruction of the Tauri v2 business snapshot.
//! This adapter is not an order writer. It cannot change app_state, relational
//! rows, receipts or the revision. On Windows, the existing Tauri app-data
//! path is discovered without creating any operational file. Release uses
//! the existing operating DB; debug builds never auto-discover it. Explicit
//! isolated unit/integration fixtures remain available to the test harness.

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

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

/// Test execution is a launch-only boundary, not a Settings feature.
pub fn test_launch()->bool{
    cfg!(windows) && cfg!(debug_assertions) &&
        std::env::var("BANDO_EGUI_DEVELOPER_TEST").ok().as_deref()==Some("1")
}
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
        let db=test_path()?;
        // Ignore inherited operational paths, indexes and remote gateways.
        std::env::set_var("BANDO_HUB_GATEWAY","local");
        std::env::remove_var("BANDO_EGUI_CLIENT_INDEX");
        std::env::set_var("BANDO_EGUI_BUSINESS_DB",&db);
        prepare_test_directories(&db)?;
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

pub fn load_settings_metadata()->Result<Value,String>{
    load_projection(ReadScope::Settings)
}

pub fn load_export_metadata()->Result<Value,String>{
    load_projection(ReadScope::Exports)
}

enum ReadScope{Full,Catalog,Settings,Exports}

fn load_projection(scope:ReadScope) -> Result<Value,String> {
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
    // Share the same app_state and catalog-hydration rules with Tauri.
    // Only the full business read needs order, recipient and item tables.
    let state=match scope{
        ReadScope::Full=>bando_data_access::read_v2_snapshot(&conn)?,
        ReadScope::Catalog=>bando_data_access::read_v2_catalog_snapshot(&conn)?,
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


#[cfg(test)]
mod auto_database_location_tests {
    use super::*;
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
