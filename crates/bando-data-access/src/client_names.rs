//! One-time retirement of the legacy invoice sender-name override.
//! Business client.name is the only current sender name. Legacy displayName
//! values that differ from name become searchable shortNames (never history).
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use std::path::Path;

fn text<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn key(raw:&str)->String{
    raw.chars().filter(|ch|!ch.is_whitespace())
        .flat_map(char::to_lowercase).collect()
}
fn terms(raw:&str)->impl Iterator<Item=&str>{
    raw.split(|ch|matches!(ch,','|';'|'|'|'\n'))
        .map(str::trim).filter(|term|!term.is_empty())
}
/// Idempotent conversion of ONE client. Existing search identifiers are not
/// replaced or lost; exact and case/whitespace-equivalent aliases are not
/// duplicated. A sender's official name never becomes an unnecessary tag.
pub fn normalize(client:&mut Value)->bool{
    let Some(map)=client.as_object_mut() else{return false};
    // Never silently drop malformed legacy data. Normal live records use
    // a string; an unexpected non-string value is preserved for diagnosis.
    let Some(legacy)=map.get("displayName").and_then(Value::as_str) else{return false};
    let legacy=legacy.trim().to_owned();
    map.remove("displayName");
    if legacy.is_empty(){return true;}
    let name=map.get("name").and_then(Value::as_str).unwrap_or("");
    let identifiers=map.get("identifiers").and_then(Value::as_str).unwrap_or("");
    let existing=map.get("shortNames").and_then(Value::as_str).unwrap_or("");
    let mut seen=std::collections::HashSet::<String>::new();
    seen.insert(key(name));
    for tag in terms(identifiers).chain(terms(existing)){
        seen.insert(key(tag));
    }
    let mut added=Vec::<String>::new();
    for tag in terms(&legacy){
        let term=key(tag);
        if !term.is_empty() && seen.insert(term){added.push(tag.to_owned());}
    }
    // Preserve the original text of existing identifiers and tags exactly:
    // only append aliases that are genuinely new.
    if !added.is_empty(){
        let combined=if existing.trim().is_empty(){added.join(", ")}
            else{format!("{}, {}",existing.trim(),added.join(", "))};
        map.insert("shortNames".into(),json!(combined));
    }
    true
}

/// Import/edit entrypoint: strips legacy override before any new catalog
/// value reaches the v2 writer. Existing historical order snapshots remain
/// immutable and are never rewritten by this function.
pub fn normalized(client:&Value)->Value{
    let mut next=client.clone();
    normalize(&mut next);
    next
}

/// Only canonical v2 CLIENT rows are migrated. Creates a verified immutable
/// SQLite backup before the first write and bumps the shared CAS revision
/// once. On startup subsequent calls are strictly no-op, including no backup
/// and no revision bump. Does not touch orders, audit rows or snapshots.
pub fn migrate_v2(conn:&mut Connection,base:&Path)->Result<usize,String>{
    migrate_v2_inner(conn,Some(base))
}

/// Backup restore already owns a verified pre-restore safety copy and rolls
/// back the entire database if any subsequent migration step fails.
/// Reuse that guarantee rather than creating a second backup AFTER restoring.
pub(crate) fn migrate_v2_with_verified_restore_backup(
    conn:&mut Connection
)->Result<usize,String>{
    migrate_v2_inner(conn,None)
}
fn migrate_v2_inner(conn:&mut Connection,backup_base:Option<&Path>)
    ->Result<usize,String>{
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|r|r.get(0)
    ).map_err(|e|e.to_string())?;
    if !crate::supported_relational_version(version){return Err("지원하지 않는 운영 DB 버전입니다.".into());}
    let mut changes=Vec::<(String,String,String)>::new();
    {
        let mut stmt=conn.prepare("SELECT id,payload FROM core_clients ORDER BY position,id")
            .map_err(|e|e.to_string())?;
        let entries=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))
            .map_err(|e|e.to_string())?;
        for entry in entries{
            let (id,old)=entry.map_err(|e|e.to_string())?;
            let mut client:Value=serde_json::from_str(&old).map_err(|e|e.to_string())?;
            if normalize(&mut client){
                let new=serde_json::to_string(&client).map_err(|e|e.to_string())?;
                changes.push((id,old,new));
            }
        }
    }
    if changes.is_empty(){return Ok(0);}
    // Standalone migration always creates a verified before-image.
    // Restore migration instead relies on the already verified pre-restore
    // backup and the restore engine's full rollback protection.
    if let Some(base)=backup_base{
        crate::backup_service::manual_backup(conn,base)?;
    }
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e|e.to_string())?;
    for (id,old,new) in &changes{
        let count=tx.execute(
            "UPDATE core_clients SET payload=?1 WHERE id=?2 AND payload=?3",
            params![new,id,old]
        ).map_err(|e|e.to_string())?;
        if count!=1{
            return Err("거래처 데이터가 이관 도중 변경됐습니다. 기존 값을 유지하고 다시 시도하세요.".into());
        }
    }
    let raw:String=tx.query_row("SELECT payload FROM app_state WHERE id=1",[],
        |r|r.get(0)).map_err(|e|e.to_string())?;
    let mut envelope=crate::decode_physical_envelope(&tx,&raw)?;
    let revision=envelope["revision"].as_u64().ok_or("거래처 이관 DB revision 오류")?;
    envelope["revision"]=json!(revision.checked_add(1)
        .ok_or("거래처 이관 DB revision 한도 초과")?);
    let updated=tx.execute("UPDATE app_state SET payload=?1 WHERE id=1 AND payload=?2",
        params![crate::encode_physical_envelope(&tx,&envelope)?,raw]
    ).map_err(|e|e.to_string())?;
    if updated!=1{return Err("거래처 이관 중 DB 변경을 감지했습니다.".into());}
    tx.commit().map_err(|e|e.to_string())?;
    Ok(changes.len())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn legacy_display_name_moves_without_duplicate_or_official_name(){
        let mut c=json!({"name":"테스트상사","displayName":"테스트 납품처 01",
            "shortNames":"옛별칭, 기존 태그","identifiers":"고유 식별자"});
        assert!(normalize(&mut c));
        assert_eq!(c["shortNames"],"옛별칭, 기존 태그, 테스트 납품처 01");
        assert!(c.get("displayName").is_none());
        assert!(!normalize(&mut c));
        let same=json!({"name":"테스트상사","displayName":"테스트상사",
            "shortNames":"기존 태그"});
        assert_eq!(normalized(&same)["shortNames"],"기존 태그");
        let duplicated=json!({"name":"테스트상사","displayName":" 테스트 납품처 01 ",
            "shortNames":"테스트납품처01"});
        assert_eq!(normalized(&duplicated)["shortNames"],"테스트납품처01");
        let varied=json!({"name":"공식명","displayName":"과거명",
            "shortNames":"기존;과거명","identifiers":"고유|옛상호"});
        let result=normalized(&varied);
        assert_eq!(result["shortNames"],"기존;과거명");
        assert_eq!(result["identifiers"],"고유|옛상호");
        let malformed=json!({"name":"공식명","displayName":{"unexpected":1}});
        assert_eq!(normalized(&malformed),malformed);
    }
    #[test]
    fn existing_fields_and_historical_state_stay_intact(){
        let client=json!({"id":"c","name":"공식상호","displayName":"과거상호",
            "shortNames":"A","identifiers":"B","tier":"special","phone":"010-1"});
        let before=client.clone();
        let next=normalized(&client);
        assert_eq!(client,before);
        assert_eq!(next["shortNames"],"A, 과거상호");
        for key in ["id","name","identifiers","tier","phone"]{
            assert_eq!(next[key],client[key]);
        }
        assert!(next.get("displayName").is_none());
    }

    #[test]
    fn sqlite_client_migration_is_backed_up_idempotent_and_keeps_orders(){
        let nanos=std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base=std::env::temp_dir().join(format!(
            "tsbh-client-name-migration-{}-{nanos}",std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path=base.join("business.sqlite");
        let mut conn=Connection::open(&path).unwrap();
        // Reuse the production schema DDL, but do not rerun an unrelated
        // historical product transform that requires the entire seeded
        // product catalog. This fixture is already a valid v2 database.
        conn.execute_batch(r#"
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE product_index(id TEXT,code TEXT,name TEXT,payload TEXT);
            CREATE TABLE order_index(id TEXT,client_id TEXT,created_at TEXT,confirmed_at TEXT,status TEXT);
            CREATE TABLE receipt_archive(id TEXT,order_id TEXT,record TEXT);
        "#).unwrap();
        crate::recovery::create_v2_schema_for_tests(&conn).unwrap();
        conn.execute("INSERT INTO core_schema(key,version) VALUES('business-core',2)",
            []).unwrap();
        conn.execute("INSERT INTO app_state VALUES (1,?1)",
            [json!({"revision":7,"aliases":[],"queue":[],"exports":[],
                "settings":{"accounts":[]}}).to_string()]).unwrap();
        let original_client=json!({
            "id":"c","name":"테스트상사","displayName":"내부상호",
            "tier":"wholesale","address":"서울","phone":"02-1234",
            "shortNames":"기존태그","identifiers":"식별코드"
        }).to_string();
        conn.execute(
            "INSERT INTO core_clients(id,position,name,tier,address,payload)
             VALUES('c',0,'테스트상사','wholesale','서울',?1)",
            [&original_client]
        ).unwrap();
        let original_order=json!({
            "id":"o","clientId":"c","status":"REVIEW",
            "createdAt":"2026-11-20T00:00:00","recipients":[],
            "clientSnapshot":{"name":"테스트상사",
                "displayName":"역사적 송장 상호"}
        }).to_string();
        conn.execute(
            "INSERT INTO core_orders(
                id,position,client_id,created_at,confirmed_at,status,payload
            ) VALUES('o',0,'c','2026-11-20T00:00:00',NULL,'REVIEW',?1)",
            [&original_order]
        ).unwrap();
        let historical_text:String=conn.query_row(
            "SELECT payload FROM core_orders WHERE id='o'",[],
            |r|r.get(0)).unwrap();
        // A real legacy backup must remain restorable even after the new
        // program migrates the client; restored history is immutable.
        let legacy_backup=crate::backup_service::manual_backup(&conn,&base).unwrap();
        assert_eq!(migrate_v2(&mut conn,&base).unwrap(),1);
        let text:String=conn.query_row("SELECT payload FROM core_clients WHERE id='c'",
            [],|r|r.get(0)).unwrap();
        let client:Value=serde_json::from_str(&text).unwrap();
        assert_eq!(client["name"],"테스트상사");
        assert_eq!(client["shortNames"],"기존태그, 내부상호");
        assert!(client.get("displayName").is_none());
        let envelope:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",
            [],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&envelope).unwrap()["revision"],8);
        let order:String=conn.query_row("SELECT payload FROM core_orders WHERE id='o'",
            [],|r|r.get(0)).unwrap();
        assert_eq!(order,historical_text);
        assert_eq!(migrate_v2(&mut conn,&base).unwrap(),0);
        let after:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",
            [],|r|r.get(0)).unwrap();
        assert_eq!(after,envelope);
        let backups=std::fs::read_dir(base.join("backups")).unwrap().count();
        assert_eq!(backups,2);
        crate::backup_service::restore_backup(
            &mut conn,&base,&legacy_backup
        ).unwrap();
        let restored:String=conn.query_row(
            "SELECT payload FROM core_clients WHERE id='c'",
            [],|r|r.get(0)
        ).unwrap();
        let restored:Value=serde_json::from_str(&restored).unwrap();
        assert_eq!(restored["name"],"테스트상사");
        assert_eq!(restored["shortNames"],"기존태그, 내부상호");
        assert!(restored.get("displayName").is_none());
        let order_after:String=conn.query_row(
            "SELECT payload FROM core_orders WHERE id='o'",[],
            |r|r.get(0)
        ).unwrap();
        assert_eq!(order_after,historical_text);
        let revision:Value=crate::recovery::get_snapshot(&conn).unwrap();
        // Restore increments the revision, and conversion increments once
        // more so stale editors cannot overwrite normalized records.
        assert_eq!(revision["revision"],10);
        assert_eq!(migrate_v2(&mut conn,&base).unwrap(),0);
        let backups=std::fs::read_dir(base.join("backups")).unwrap().count();
        assert_eq!(backups,3);
        drop(conn);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
