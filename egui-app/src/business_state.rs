//! Consistent read-only reconstruction of the Tauri v2 business snapshot.
//! This adapter is not an order writer. It cannot change app_state, relational
//! rows, receipts or the revision. On Windows, the existing Tauri app-data
//! path is discovered without creating any operational file. Debug/DEV binds
//! only to the isolated Tauri developer-test database; release binds to the
//! existing production database. The real mutation rules remain in Rust.

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::PathBuf;

/// The Tauri app-data path is RoamingAppData/{identifier}. Read the identifier
/// from the exact bundled Tauri configuration rather than making up a second
/// egui directory. This function is pure, so path precedence can be tested
/// without touching the actual employee files.
fn tauri_database_path(roaming:&std::path::Path, developer_test:bool)->Result<PathBuf,String>{
    let config:Value=serde_json::from_str(include_str!("../../src-tauri/tauri.conf.json"))
        .map_err(|_|"Tauri 프로그램 식별자 설정을 읽지 못했습니다.".to_owned())?;
    if config.pointer("/app/appDirectoriesOverride").is_some_and(|v|!v.is_null()){
        return Err("Tauri 데이터 폴더가 별도로 지정돼 있어 자동 연결을 중단했습니다.".into());
    }
    let identifier=config.get("identifier").and_then(Value::as_str)
        .filter(|id|!id.is_empty() && !id.contains('/') && !id.contains('\\'))
        .ok_or("Tauri 프로그램 식별자가 올바르지 않습니다.")?;
    let root=roaming.join(identifier);
    Ok(if developer_test {
        root.join("developer-test").join("bando-hub-test.sqlite")
    }else{
        root.join("bando-hub-v001.sqlite")
    })
}

/// Existing DB read-only auto-binding happens once at the app entrypoint,
/// before any workers start. Explicit configured DB/index or HTTP gateway wins.
/// The developer build can NEVER silently fall back to the production DB.
/// No schema creation, copies, migrations or edits are performed here.
pub fn bind_existing_tauri_database() -> Result<Option<PathBuf>,String>{
    if !cfg!(windows) ||
        std::env::var("BANDO_HUB_GATEWAY")
            .ok().is_some_and(|mode|mode.trim().eq_ignore_ascii_case("http")){
        return Ok(None);
    }
    if let Some(path)=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
        .filter(|v|!v.is_empty()){
        return Ok(Some(PathBuf::from(path)));
    }
    if std::env::var_os("BANDO_EGUI_CLIENT_INDEX")
        .is_some_and(|v|!v.is_empty()){
        return Ok(None);
    }
    let Some(roaming)=std::env::var_os("APPDATA").filter(|v|!v.is_empty()) else{
        return Ok(None);
    };
    let test=cfg!(debug_assertions) ||
        std::env::var("BANDO_EGUI_DEVELOPER_TEST")
            .ok().as_deref()==Some("1");
    let path=tauri_database_path(std::path::Path::new(&roaming),test)?;
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

/// The live catalog requires a real v2 Tauri DB. A seed fallback would mix
/// unrelated product IDs with prices and freight rules.
pub fn load() -> Result<Value,String> {
    let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
        .filter(|p|!p.is_empty())
        .map(PathBuf::from)
        .ok_or_else(||{
            if cfg!(debug_assertions) ||
                std::env::var("BANDO_EGUI_DEVELOPER_TEST").ok().as_deref()==Some("1") {
                "개발용 Tauri 테스트 DB가 없습니다. DEV_WINDOWS_TAURI에서 테스트 DB를 만든 뒤 다시 실행하세요. 운영 DB로 자동 전환하지 않습니다.".to_owned()
            }else{
                "기존 Tauri 운영 DB를 찾지 못했습니다. 원본 DB를 새로 생성하거나 초기화하지 않았습니다.".to_owned()
            }
        })?;
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
    // Share app_state parsing and relation hydration with the Tauri host.
    // Keep the existing consistent read-only transaction and v2 guard.
    let state=bando_data_access::read_v2_snapshot(&conn)?;
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
    fn resolves_tauri_production_and_developer_test_as_distinct_paths(){
        let root=std::path::Path::new("roaming-root");
        let production=tauri_database_path(root,false).unwrap();
        let developer=tauri_database_path(root,true).unwrap();
        assert!(production.ends_with(
            std::path::Path::new("com.bandostand.bandohub.tauri")
                .join("bando-hub-v001.sqlite")
        ));
        assert!(developer.ends_with(
            std::path::Path::new("com.bandostand.bandohub.tauri")
                .join("developer-test").join("bando-hub-test.sqlite")
        ));
        assert_ne!(developer,production);
    }
}
