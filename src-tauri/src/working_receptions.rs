//! Durable SQLite storage for unconfirmed receptions, independent of the
//! confirmed order ledger. A preview on the workflow board is NOT itself a
//! persistence boundary; these working orders survive a process restart.
//! Only the existing step-1 '주문 확인 완료' action creates immutable billing
//! and receipt snapshots and makes a row eligible for 주문내역.
use std::collections::HashSet;
use rusqlite::{params, Connection};
use serde_json::Value;
use tauri::State;
use super::{Database, err, lock_db};

pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS reception_working_orders (
            id TEXT PRIMARY KEY,
            changed_at TEXT NOT NULL,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );"
    ).map_err(err)
}

fn confirmed_order_ids(conn: &Connection) -> Result<HashSet<String>, String> {
    let mut stmt = conn.prepare("SELECT id FROM order_index").map_err(err)?;
    let mapped = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(err)?;
    let ids: Result<HashSet<String>, rusqlite::Error> = mapped.collect();
    ids.map_err(err)
}

pub(super) fn load(conn: &Connection) -> Result<Vec<Value>, String> {
    let permanent = confirmed_order_ids(conn)?;
    let mut stmt = conn.prepare(
        "SELECT payload FROM reception_working_orders ORDER BY changed_at DESC, id"
    ).map_err(err)?;
    let payloads: Vec<String> = stmt.query_map([], |row| row.get(0)).map_err(err)?
        .collect::<Result<_, _>>().map_err(err)?;
    payloads.into_iter().map(|raw| {
        let order: Value = serde_json::from_str(&raw).map_err(err)?;
        let id = order["id"].as_str().ok_or("접수 데이터 ID 누락")?;
        if permanent.contains(id) { return Ok(None); }
        Ok(Some(order))
    }).collect::<Result<Vec<_>, String>>()
      .map(|orders| orders.into_iter().flatten().collect())
}

/// One atomically replaced working set, serialized by the frontend autosave
/// queue. Never touches the confirmed app_state/core_orders ledger or revision.
pub(super) fn replace(conn: &mut Connection, orders: Vec<Value>) -> Result<(), String> {
    if orders.len() > 1000 { return Err("미확인 접수 보관 한도를 초과했습니다.".into()); }
    let permanent = confirmed_order_ids(conn)?;
    let mut seen = HashSet::new();
    let mut serialized = Vec::new();
    for order in &orders {
        let id = order["id"].as_str().filter(|id| !id.trim().is_empty())
            .ok_or("미확인 접수 ID가 없습니다.")?;
        if !seen.insert(id.to_string()) { return Err("미확인 접수 ID가 중복됩니다.".into()); }
        if permanent.contains(id) { continue; }
        if order["receptionVersion"].as_i64() != Some(1) ||
            order["confirmedAt"].as_str().is_some_and(|s| !s.is_empty()) {
            return Err("확정된 주문은 미확인 접수 보관함에 저장할 수 없습니다.".into());
        }
        let raw = serde_json::to_string(order).map_err(err)?;
        if raw.len() > 50_000_000 { return Err("발주서 용량이 너무 큽니다.".into()); }
        serialized.push((id.to_string(), raw));
    }
    let tx = conn.transaction().map_err(err)?;
    tx.execute("DELETE FROM reception_working_orders", []).map_err(err)?;
    let timestamp = chrono::Utc::now().to_rfc3339();
    for (id, raw) in serialized {
        tx.execute(
            "INSERT INTO reception_working_orders(id,changed_at,payload) VALUES (?1,?2,?3)",
            params![id, timestamp, raw]
        ).map_err(err)?;
    }
    tx.commit().map_err(err)
}

#[tauri::command]
pub(super) fn working_receptions_load(db: State<'_, Database>) -> Result<Vec<Value>, String> {
    load(&lock_db(&db)?)
}

#[tauri::command]
pub(super) fn working_receptions_replace(
    db: State<'_, Database>, orders: Vec<Value>,
) -> Result<(), String> {
    replace(&mut lock_db(&db)?, orders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn db() -> Connection {
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE order_index(id TEXT PRIMARY KEY);").unwrap();
        initialize(&conn).unwrap();
        conn
    }
    #[test]
    fn unconfirmed_receptions_support_atomic_replacement() {
        let mut conn=db();
        let one=json!({"id":"a","receptionVersion":1,"raw":"접수 원문",
            "recipients":[{"id":"r1","items":[]}]});
        let two=json!({"id":"b","receptionVersion":1,"raw":"두 번째 원문"});
        replace(&mut conn,vec![one.clone(),two.clone()]).unwrap();
        assert_eq!(load(&conn).unwrap().len(),2);
        replace(&mut conn,vec![two.clone()]).unwrap();
        assert_eq!(load(&conn).unwrap(),vec![two]);
    }
    #[test]
    fn sqlite_file_reopen_recovers_the_exact_unconfirmed_working_copy() {
        let stamp = std::time::SystemTime::now().duration_since(
            std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bando-working-reception-{}-{}.sqlite",std::process::id(),stamp));
        let order = json!({"id":"restored-1","receptionVersion":1,
            "raw":"원본 한글 발주서","recipients":[{"id":"r1","name":"이가상",
            "address":"제주특별자치도 서귀포시","items":[{"id":"i1","quantity":2}]}]});
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE order_index(id TEXT PRIMARY KEY);").unwrap();
            initialize(&conn).unwrap();
            replace(&mut conn, vec![order.clone()]).unwrap();
        }
        {
            let conn = Connection::open(&path).unwrap();
            initialize(&conn).unwrap();
            assert_eq!(load(&conn).unwrap(),vec![order]);
        }
        let _=std::fs::remove_file(path);
    }
    #[test]
    fn confirmed_order_ids_are_never_reintroduced() {
        let mut conn=db();
        let one=json!({"id":"a","receptionVersion":1,"raw":"접수 원문"});
        replace(&mut conn,vec![one.clone()]).unwrap();
        conn.execute("INSERT INTO order_index(id) VALUES ('a')",[]).unwrap();
        assert!(load(&conn).unwrap().is_empty());
        replace(&mut conn,vec![one]).unwrap();
        assert!(load(&conn).unwrap().is_empty());
    }
    #[test]
    fn invalid_replace_rolls_back_existing_work() {
        let mut conn=db();
        let one=json!({"id":"a","receptionVersion":1,"raw":"원본"});
        replace(&mut conn,vec![one.clone()]).unwrap();
        let err=replace(&mut conn,vec![
            one.clone(),json!({"id":"b","receptionVersion":1,"confirmedAt":"now"})]);
        assert!(err.is_err());
        assert_eq!(load(&conn).unwrap(),vec![one]);
    }
}
