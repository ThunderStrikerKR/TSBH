//! Full SQLite backups for production and isolated TEST workspaces.
//! Daily automatic snapshots retain 30 calendar days. The first actual
//! automatic backup in each local calendar month is promoted to permanent.
//! Manual backups and pre-restore safety backups are never pruned.
use rusqlite::Connection;
use serde_json::json;
use std::{fs,path::{Path,PathBuf}};
use chrono::{Datelike,Local};
use crate::valid_state;
use crate::recovery;
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}
fn get_snapshot(conn:&Connection)->Result<serde_json::Value,String>{
    recovery::get_snapshot(conn)
}
use chrono::{Duration as CalendarDuration, NaiveDate};
use rusqlite::{backup::Backup, OpenFlags};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BACKUP_KEEP_DAYS: i64 = 30;

pub fn backup_dir(base: &Path) -> PathBuf { base.join("backups") }

fn filename(kind: &str, date: NaiveDate) -> Result<String, String> {
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?;
    Ok(format!("{kind}-{}-{}-{}.sqlite",
        date.format("%Y%m%d"), unix.as_nanos(), std::process::id()))
}

fn validate_database(conn: &Connection) -> Result<(), String> {
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0)).map_err(err)?;
    if integrity != "ok" { return Err(format!("SQLite 데이터 무결성 오류: {integrity}")); }
    let state = get_snapshot(conn)?;
    valid_state(&state)?;
    recovery::verify(conn)?;
    for table in ["app_state", "product_index", "order_index", "receipt_archive"] {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        conn.query_row(&sql, [], |row| row.get::<_, i64>(0)).map_err(|e|
            format!("백업 파일의 필수 데이터 테이블 {table}을 확인할 수 없습니다: {e}"))?;
    }
    Ok(())
}

pub fn read_backup(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(err)?;
    validate_database(&conn)?;
    Ok(conn)
}

fn write_backup(conn: &Connection, base: &Path, kind: &str, date: NaiveDate)
    -> Result<PathBuf, String> {
    let dir = backup_dir(base);
    fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(filename(kind, date)?);
    if path.exists() { return Err("같은 이름의 백업 파일이 이미 존재합니다.".into()); }
    // VACUUM INTO is consistent with active WAL and never copies a partial
    // database. Incomplete files are removed if creation/verification fails.
    let outcome = (|| {
        conn.execute("VACUUM INTO ?1", [path.to_string_lossy().to_string()]).map_err(err)?;
        let snapshot = read_backup(&path)?;
        drop(snapshot);
        Ok::<_, String>(())
    })();
    if outcome.is_err() { let _ = fs::remove_file(&path); }
    outcome?;
    Ok(path)
}

pub fn manual_backup(conn: &Connection, base: &Path) -> Result<String, String> {
    let path = write_backup(conn, base, "manual", Local::now().date_naive())?;
    Ok(path.to_string_lossy().into_owned())
}

pub fn list_backups(base: &Path) -> Result<Vec<String>, String> {
    let dir = backup_dir(base);
    if !dir.exists() { return Ok(vec![]); }
    let mut names = fs::read_dir(dir).map_err(err)?.filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|v| v == "sqlite") && e.path().is_file())
        .map(|entry| entry.path().to_string_lossy().into_owned()).collect::<Vec<_>>();
    names.sort();
    names.reverse();
    Ok(names)
}

fn name_date(name: &str, prefix: &str) -> Option<NaiveDate> {
    let date = name.strip_prefix(prefix)?.get(..8)?;
    NaiveDate::parse_from_str(date, "%Y%m%d").ok()
}

fn month_has_permanent(dir: &Path, date: NaiveDate) -> Result<bool, String> {
    if !dir.exists() { return Ok(false); }
    for entry in fs::read_dir(dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && name.ends_with(".sqlite")
            && name_date(&name, "monthly-").is_some_and(|saved|
                saved.year() == date.year() && saved.month() == date.month()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn day_has_automatic(dir: &Path, date: NaiveDate) -> Result<bool, String> {
    if !dir.exists() { return Ok(false); }
    for entry in fs::read_dir(dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        if !entry.path().is_file() { continue; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".sqlite") { continue; }
        if name_date(&name, "auto-") == Some(date) ||
            name_date(&name, "monthly-") == Some(date) { return Ok(true); }
    }
    Ok(false)
}

/// Never delete unknown, manual, monthly, pre-restore, or legacy backup names.
/// A missing/invalid date also means preservation, not deletion.
pub fn prune_automatic(dir: &Path, date: NaiveDate) -> Result<(), String> {
    if !dir.exists() { return Ok(()); }
    let oldest = date - CalendarDuration::days(BACKUP_KEEP_DAYS - 1);
    for entry in fs::read_dir(dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        let path = entry.path();
        if !path.is_file() || path.extension().is_none_or(|v| v != "sqlite") { continue; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name_date(&name, "auto-").is_some_and(|saved| saved < oldest) {
            fs::remove_file(path).map_err(err)?;
        }
    }
    Ok(())
}

/// Called once on application startup. An app left open across midnight
/// does not schedule another backup until it is launched again.
/// A day without a backup has no invented file: the next actual backup
/// becomes that month's permanent first-backup day.
pub fn automatic_backup_if_needed(
    conn: &Connection, base: &Path, today: NaiveDate
) -> Result<Option<String>, String> {
    let dir = backup_dir(base);
    fs::create_dir_all(&dir).map_err(err)?;
    if day_has_automatic(&dir, today)? {
        prune_automatic(&dir, today)?;
        return Ok(None);
    }
    let kind = if month_has_permanent(&dir, today)? { "auto" } else { "monthly" };
    let path = write_backup(conn, base, kind, today)?;
    prune_automatic(&dir, today)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn copy_entire_db(source: &Connection, target: &mut Connection) -> Result<(), String> {
    {
        let copy = Backup::new(source, target).map_err(err)?;
        copy.run_to_completion(128, Duration::from_millis(20), None).map_err(err)?;
    }
    validate_database(target)
}

fn restore_selected(conn: &mut Connection, base: &Path, filename: &str)
    -> Result<String, String> {
    let allowed = fs::canonicalize(backup_dir(base)).map_err(err)?;
    let selected = fs::canonicalize(filename).map_err(err)?;
    if selected.parent() != Some(allowed.as_path()) ||
        selected.extension().is_none_or(|ext| ext != "sqlite") ||
        !selected.is_file() {
        return Err("현재 작업공간 백업 폴더 안의 SQLite 파일만 복원할 수 있습니다.".into());
    }
    // Validate the selected backup BEFORE writing anything to the live DB.
    let backup = read_backup(&selected)?;
    let active_revision = get_snapshot(conn)?["revision"].as_u64().ok_or("현재 DB 리비전 오류")?;
    let selected_revision = get_snapshot(&backup)?["revision"].as_u64().ok_or("백업 DB 리비전 오류")?;
    // Never reuse an old revision after restoring history: a stale editor must
    // not accidentally overwrite the newly restored data.
    let next_revision = active_revision.max(selected_revision)
        .checked_add(1).ok_or("리비전 한도 초과로 복원할 수 없습니다.")?;
    // Save the current entire SQLite DB, not just the app_state JSON.
    let safety = write_backup(conn, base, "pre-restore", Local::now().date_naive())?;
    let result = (|| {
        copy_entire_db(&backup, conn)?;
        // Backups made before the split still contain legacy JSON. Convert
        // them transactionally after the entire backup is restored.
        recovery::migrate(conn)?;
        // Older backups predate the working-reception table. A restored DB
        // must still accept durable, pre-confirmation orders immediately.
        recovery::initialize_working(conn)?;
        // Keep the physical split envelope rather than re-embedding events.
        let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",
            [],|row|row.get(0)).map_err(err)?;
        let mut restored=crate::decode_physical_envelope(conn,&raw)?;
        restored["revision"] = json!(next_revision);
        recovery::strip_core(&mut restored);
        conn.execute("UPDATE app_state SET payload=?1 WHERE id=1",
            [crate::encode_physical_envelope(conn,&restored)?]).map_err(err)?;
        // Retire legacy invoice names BEFORE declaring restore success.
        // A failure now follows the established pre-restore rollback path;
        // migration reuses the verified safety copy rather than creating a
        // second backup of intermediate restored data.
        crate::client_names::migrate_v2_with_verified_restore_backup(conn)?;
        // A restored older backup can contain the retired color placeholder.
        // Normalize it within this restore's verified rollback boundary.
        crate::product_colors::migrate_v2_with_verified_restore_backup(conn)?;
        validate_database(conn)
    })();
    if let Err(primary_error) = result {
        // The SQLite backup API is transactional, but independently verify
        // the rollback path and retain the safety copy for manual recovery.
        let rollback = read_backup(&safety)
            .and_then(|previous| copy_entire_db(&previous, conn));
        return match rollback {
            Ok(()) => Err(format!("복원을 취소하고 직전 상태로 되돌렸습니다: {primary_error}")),
            Err(rollback_error) => Err(format!(
                "복원 및 자동 원상복구에 실패했습니다. 안전 백업 {}을 보존했습니다. 오류: {primary_error}; 원상복구: {rollback_error}",
                safety.display())),
        };
    }
    Ok(safety.to_string_lossy().into_owned())
}

pub fn restore_backup(conn: &mut Connection, base: &Path, filename: &str)
    -> Result<String, String> {
    restore_selected(conn, base, filename)
}



#[cfg(test)]
mod operational_backup_tests {
    use super::*;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize,Ordering};

    // Each test owns an isolated directory. Never resolves an app workspace
    // or reads a user DB/environment variable.
    struct Temp(PathBuf);
    impl Temp {
        fn new()->Self{
            static SEQ:AtomicUsize=AtomicUsize::new(0);
            let path=std::env::temp_dir().join(format!("bando-native-backup-{}-{}-{}",
                std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
                SEQ.fetch_add(1,Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp{
        fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}
    }

    fn fixture(base:&Path)->Connection{
        let mut conn=Connection::open(base.join("synthetic.sqlite")).unwrap();
        conn.execute_batch(r#"
            PRAGMA journal_mode=WAL;
            PRAGMA wal_autocheckpoint=0;
            PRAGMA foreign_keys=ON;
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE product_index(id TEXT PRIMARY KEY,code TEXT,name TEXT,payload TEXT);
            CREATE TABLE order_index(id TEXT PRIMARY KEY,client_id TEXT,created_at TEXT,confirmed_at TEXT,status TEXT);
            CREATE TABLE receipt_archive(id TEXT PRIMARY KEY,order_id TEXT,record TEXT);
        "#).unwrap();
        recovery::create_v2_schema_for_tests(&conn).unwrap();
        recovery::initialize_working(&conn).unwrap();
        conn.execute("INSERT INTO core_schema(key,version) VALUES('business-core',2)",[]).unwrap();
        let state=json!({
            "revision":7,"aliases":[{"id":"alias","text":"합성 별칭","productId":"p","variantId":"v"}],
            "queue":[],"exports":[],"settings":{"accounts":[],"operatorName":"합성 담당자"},
            "workflow":{"shipments":[{"id":"s","orderId":"o","recipientId":"r"}],
                "events":[{"id":"event","action":"합성 원문 기록"}],"payments":[]},
            "products":[{"id":"p","code":"709","name":"합성 상품","defaultColor":"Black",
                "variants":[{"id":"v","color":"Black","wholesale":10000,"semi":12000,"retail":15000}]}],
            "clients":[{"id":"c","name":"합성 거래처","tier":"special","address":"합성 주소",
                "phone":"010-0000-0000","overrides":{"v":9000}}],
            "orders":[{"id":"o","clientId":"c","raw":"합성 주문 원문","status":"STATEMENT",
                "createdAt":"2026-11-13T09:00:00Z",
                "recipients":[{"id":"r","name":"합성 수취인","items":[{
                    "id":"i","productId":"p","variantId":"v","unitPrice":9000,"quantity":2}]}]}]
        });
        let tx=conn.transaction().unwrap();
        crate::store_v2_relational(&tx,&state).unwrap();
        tx.execute("INSERT INTO product_index VALUES('p','709','합성 상품',?1)",
            [state["products"][0].to_string()]).unwrap();
        tx.execute("INSERT INTO order_index VALUES('o','c','2026-11-13T09:00:00Z',NULL,'STATEMENT')",[]).unwrap();
        let mut envelope=state;
        recovery::strip_core(&mut envelope);
        tx.execute("INSERT INTO app_state VALUES(1,?1)",[envelope.to_string()]).unwrap();
        tx.execute("INSERT INTO receipt_archive VALUES('receipt:o','o',?1)",
            [json!({"original":"합성 주문 원문"}).to_string()]).unwrap();
        tx.execute("INSERT INTO reception_working_orders VALUES('draft','2026-11-13',?1)",
            [json!({"id":"draft","raw":"합성 임시 발주"}).to_string()]).unwrap();
        tx.execute("INSERT INTO reception_confirmed_edits VALUES('o','2026-11-13',?1,?2)",
            [json!({"id":"o","raw":"이전 원문"}).to_string(),
             json!({"id":"o","raw":"합성 수정안"}).to_string()]).unwrap();
        tx.commit().unwrap();
        validate_database(&conn).unwrap();
        conn
    }
    fn auxiliary(conn:&Connection)->Vec<String>{
        [
            "SELECT record FROM receipt_archive WHERE id='receipt:o'",
            "SELECT payload FROM reception_working_orders WHERE id='draft'",
            "SELECT base_payload FROM reception_confirmed_edits WHERE id='o'",
            "SELECT payload FROM reception_confirmed_edits WHERE id='o'",
            "SELECT payload FROM product_index WHERE id='p'",
            "SELECT status FROM order_index WHERE id='o'"
        ].iter().map(|sql|conn.query_row(sql,[],|r|r.get::<_,String>(0)).unwrap()).collect()
    }

    #[test]
    fn full_wal_backup_restore_preserves_core_auxiliary_and_pre_restore_snapshot(){
        let temp=Temp::new();
        let mut conn=fixture(&temp.0);
        let before=get_snapshot(&conn).unwrap();
        let before_aux=auxiliary(&conn);
        let backup=manual_backup(&conn,&temp.0).unwrap();
        {
            let saved=read_backup(Path::new(&backup)).unwrap();
            assert_eq!(get_snapshot(&saved).unwrap(),before);
            assert_eq!(auxiliary(&saved),before_aux);
        }
        let changed=crate::update_v2_snapshot(&mut conn,7,false,|current|{
            let mut next=current.clone();
            next["products"][0]["variants"][0]["retail"]=json!(16000);
            next["settings"]["operatorName"]=json!("변경 담당자");
            Ok(next)
        }).unwrap();
        assert_eq!(changed["revision"],8);
        conn.execute("UPDATE reception_working_orders SET payload=?1 WHERE id='draft'",
            [json!({"id":"draft","raw":"변경 임시 발주"}).to_string()]).unwrap();
        conn.execute("UPDATE receipt_archive SET record=?1 WHERE id='receipt:o'",
            [json!({"original":"합성 변경 기록"}).to_string()]).unwrap();
        conn.execute("UPDATE reception_confirmed_edits SET payload=?1 WHERE id='o'",
            [json!({"id":"o","raw":"변경 수정안"}).to_string()]).unwrap();
        let changed_aux=auxiliary(&conn);
        let safety=restore_backup(&mut conn,&temp.0,&backup).unwrap();
        assert!(Path::new(&safety).file_name().unwrap().to_string_lossy().starts_with("pre-restore-"));
        {
            let saved=read_backup(Path::new(&safety)).unwrap();
            assert_eq!(get_snapshot(&saved).unwrap(),changed);
            assert_eq!(auxiliary(&saved),changed_aux);
        }
        let mut restored=get_snapshot(&conn).unwrap();
        assert_eq!(restored["revision"],9);
        assert_eq!(auxiliary(&conn),before_aux);
        restored["revision"]=before["revision"].clone();
        assert_eq!(restored,before);
        // A pre-restore editor cannot overwrite a newer restored revision.
        assert!(crate::write_v2_snapshot(&mut conn,changed,8,false).is_err());
        drop(conn);
        let reopened=Connection::open(temp.0.join("synthetic.sqlite")).unwrap();
        validate_database(&reopened).unwrap();
        assert_eq!(get_snapshot(&reopened).unwrap()["revision"],9);
        assert_eq!(auxiliary(&reopened),before_aux);
    }

    #[test]
    fn invalid_and_outside_backups_are_rejected_before_any_restore_write(){
        let temp=Temp::new();
        let mut conn=fixture(&temp.0);
        let before=get_snapshot(&conn).unwrap();
        let before_aux=auxiliary(&conn);
        let saved=manual_backup(&conn,&temp.0).unwrap();
        let outside=temp.0.join("outside.sqlite");
        fs::copy(&saved,&outside).unwrap();
        let corrupt=backup_dir(&temp.0).join("corrupt.sqlite");
        fs::write(&corrupt,b"not a sqlite database").unwrap();
        let invalid=backup_dir(&temp.0).join("missing-price.sqlite");
        fs::copy(&saved,&invalid).unwrap();
        {
            let invalid_db=Connection::open(&invalid).unwrap();
            invalid_db.execute("DELETE FROM core_product_prices WHERE product_id='p'",[]).unwrap();
        }
        let count=list_backups(&temp.0).unwrap().len();
        for path in [&outside,&corrupt,&invalid] {
            assert!(restore_backup(&mut conn,&temp.0,path.to_str().unwrap()).is_err());
            assert_eq!(get_snapshot(&conn).unwrap(),before);
            assert_eq!(auxiliary(&conn),before_aux);
        }
        assert_eq!(list_backups(&temp.0).unwrap().len(),count);
    }

    #[test]
    fn automatic_backup_retention_preserves_monthly_manual_and_safety_files(){
        let temp=Temp::new();
        let conn=fixture(&temp.0);
        let day=|y,m,d|NaiveDate::from_ymd_opt(y,m,d).unwrap();
        let monthly=automatic_backup_if_needed(&conn,&temp.0,day(2026,10,1)).unwrap().unwrap();
        assert!(Path::new(&monthly).file_name().unwrap().to_string_lossy().starts_with("monthly-20261001-"));
        assert!(automatic_backup_if_needed(&conn,&temp.0,day(2026,10,1)).unwrap().is_none());
        let expired=automatic_backup_if_needed(&conn,&temp.0,day(2026,10,2)).unwrap().unwrap();
        let boundary=automatic_backup_if_needed(&conn,&temp.0,day(2026,10,3)).unwrap().unwrap();
        let manual=manual_backup(&conn,&temp.0).unwrap();
        let safety=write_backup(&conn,&temp.0,"pre-restore",day(2026,10,1)).unwrap();
        let unknown=backup_dir(&temp.0).join("legacy-unknown.sqlite");
        fs::copy(&manual,&unknown).unwrap();
        let next=automatic_backup_if_needed(&conn,&temp.0,day(2026,11,1)).unwrap().unwrap();
        assert!(Path::new(&next).file_name().unwrap().to_string_lossy().starts_with("monthly-20261101-"));
        assert!(!Path::new(&expired).exists());
        for path in [Path::new(&monthly),Path::new(&boundary),Path::new(&manual),&safety,&unknown,Path::new(&next)]{
            assert!(path.is_file(),"preserve {}",path.display());
            assert!(read_backup(path).is_ok());
        }
    }

    #[test]
    fn native_revision_compare_and_swap_rejects_stale_independent_writer(){
        let temp=Temp::new();
        let mut first=fixture(&temp.0);
        let mut second=Connection::open(temp.0.join("synthetic.sqlite")).unwrap();
        let old=get_snapshot(&second).unwrap();
        let written=crate::update_v2_snapshot(&mut first,7,false,|state|{
            let mut next=state.clone();
            next["settings"]["operatorName"]=json!("첫 번째 저장");
            Ok(next)
        }).unwrap();
        assert_eq!(written["revision"],8);
        let mut stale=old;
        stale["settings"]["operatorName"]=json!("오래된 저장");
        assert!(crate::write_v2_snapshot(&mut second,stale,7,false).is_err());
        assert_eq!(get_snapshot(&second).unwrap(),written);
    }

    #[test]
    fn failed_envelope_write_rolls_back_relational_price_and_legacy_index(){
        let temp=Temp::new();
        let mut conn=fixture(&temp.0);
        let before=get_snapshot(&conn).unwrap();
        let before_aux=auxiliary(&conn);
        conn.execute_batch("CREATE TRIGGER fail_envelope BEFORE UPDATE ON app_state
            BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
        let result=crate::update_v2_snapshot(&mut conn,7,false,|state|{
            let mut next=state.clone();
            next["products"][0]["variants"][0]["retail"]=json!(99999);
            Ok(next)
        });
        assert!(result.is_err());
        assert_eq!(get_snapshot(&conn).unwrap(),before);
        assert_eq!(auxiliary(&conn),before_aux);
        validate_database(&conn).unwrap();
    }

    #[test]
    fn failure_after_selected_copy_restores_exact_pre_restore_database(){
        let temp=Temp::new();
        let mut conn=fixture(&temp.0);
        let selected=manual_backup(&conn,&temp.0).unwrap();
        // A valid SQLite backup whose trigger fails only when restoration
        // attempts to write its new revision, AFTER copying the selected DB.
        {
            let selected_db=Connection::open(&selected).unwrap();
            selected_db.execute_batch("CREATE TRIGGER synthetic_restore_failure BEFORE UPDATE ON app_state
                BEGIN SELECT RAISE(ABORT,'synthetic post-copy failure'); END;").unwrap();
            validate_database(&selected_db).unwrap();
        }
        let active=crate::update_v2_snapshot(&mut conn,7,false,|state|{
            let mut next=state.clone();
            next["products"][0]["variants"][0]["retail"]=json!(18000);
            next["settings"]["operatorName"]=json!("복원 직전 담당자");
            Ok(next)
        }).unwrap();
        conn.execute("UPDATE reception_working_orders SET payload=?1 WHERE id='draft'",
            [json!({"id":"draft","raw":"복원 직전 작업본"}).to_string()]).unwrap();
        let before_aux=auxiliary(&conn);
        let error=restore_backup(&mut conn,&temp.0,&selected).unwrap_err();
        assert!(error.contains("복원을 취소하고 직전 상태로 되돌렸습니다"),"{error}");
        assert!(error.contains("synthetic post-copy failure"),"{error}");
        assert_eq!(get_snapshot(&conn).unwrap(),active);
        assert_eq!(auxiliary(&conn),before_aux);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='synthetic_restore_failure'",
            [],|r|r.get::<_,i64>(0)).unwrap(),0);
        let safety=list_backups(&temp.0).unwrap().into_iter().find(|p|
            Path::new(p).file_name().unwrap().to_string_lossy().starts_with("pre-restore-")).unwrap();
        assert_eq!(get_snapshot(&read_backup(Path::new(&safety)).unwrap()).unwrap(),active);
        drop(conn);
        let reopened=Connection::open(temp.0.join("synthetic.sqlite")).unwrap();
        validate_database(&reopened).unwrap();
        assert_eq!(get_snapshot(&reopened).unwrap(),active);
        assert_eq!(auxiliary(&reopened),before_aux);
    }
}
