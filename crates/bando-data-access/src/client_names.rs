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
/// Idempotent conversion of ONE client. Existing search identifiers are not
/// replaced or lost; exact and case/whitespace-equivalent aliases are not
/// duplicated. A sender's official name never becomes an unnecessary tag.
pub fn normalize(client:&mut Value)->bool{
    let Some(map)=client.as_object_mut() else{return false};
    let Some(legacy)=map.remove("displayName") else{return false};
    let name=map.get("name").and_then(Value::as_str).unwrap_or("");
    let name_key=key(name);
    let legacy=legacy.as_str().unwrap_or("").trim();
    if legacy.is_empty(){return true;}
    let identifiers=map.get("identifiers").and_then(Value::as_str).unwrap_or("");
    let existing=map.get("shortNames").and_then(Value::as_str).unwrap_or("");
    let mut tags=existing.split(',').map(str::trim)
        .filter(|tag|!tag.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let mut seen=std::collections::HashSet::<String>::new();
    seen.insert(name_key);
    for tag in identifiers.split(',').map(str::trim){
        if !tag.is_empty(){seen.insert(key(tag));}
    }
    // Preserve all existing tag strings in original order. The legacy
    // override is appended only if it adds a genuinely new search key.
    for tag in &tags{seen.insert(key(tag));}
    for tag in legacy.split(',').map(str::trim){
        let term=key(tag);
        if !term.is_empty() && seen.insert(term){tags.push(tag.to_owned());}
    }
    if tags.len()>existing.split(',').filter(|v|!v.trim().is_empty()).count(){
        map.insert("shortNames".into(),json!(tags.join(", ")));
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
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|r|r.get(0)
    ).map_err(|e|e.to_string())?;
    if version!=2{return Ok(0);}
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
    // No user data is written if a verified backup cannot be created.
    crate::backup_service::manual_backup(conn,base)?;
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
    let mut envelope:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
    let revision=envelope["revision"].as_u64().ok_or("거래처 이관 DB revision 오류")?;
    envelope["revision"]=json!(revision.checked_add(1)
        .ok_or("거래처 이관 DB revision 한도 초과")?);
    let updated=tx.execute("UPDATE app_state SET payload=?1 WHERE id=1 AND payload=?2",
        params![serde_json::to_string(&envelope).map_err(|e|e.to_string())?,raw]
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
        conn.execute_batch(r#"
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE core_schema(key TEXT PRIMARY KEY,version INTEGER);
            INSERT INTO core_schema VALUES('business-core',2);
            CREATE TABLE core_products(id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE core_product_prices(product_id TEXT,payload TEXT,price_record_id TEXT);
            CREATE TABLE core_clients(id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE core_orders(id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE core_recipients(id TEXT,order_id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE core_items(id TEXT,recipient_id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE product_index(id TEXT,code TEXT,name TEXT,payload TEXT);
            CREATE TABLE order_index(id TEXT,client_id TEXT,created_at TEXT,confirmed_at TEXT,status TEXT);
            CREATE TABLE receipt_archive(id TEXT,order_id TEXT,record TEXT);
        "#).unwrap();
        conn.execute("INSERT INTO app_state VALUES (1,?1)",
            [json!({"revision":7,"aliases":[],"queue":[],"exports":[],
                "settings":{}}).to_string()]).unwrap();
        conn.execute("INSERT INTO core_clients VALUES ('c',0,?1)",
            [json!({"id":"c","name":"테스트상사","displayName":"내부상호",
                "tier":"wholesale","address":"서울","phone":"02-1234",
                "shortNames":"기존태그","identifiers":"식별코드"}).to_string()]).unwrap();
        let historical=json!({"id":"o","clientSnapshot":{
            "name":"테스트상사","displayName":"과거 송장 표기"}});
        let historical_text=historical.to_string();
        conn.execute("INSERT INTO core_orders VALUES ('o',0,?1)",
            [&historical_text]).unwrap();
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
        assert_eq!(backups,1);
        drop(conn);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
