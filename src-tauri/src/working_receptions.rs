//! Durable SQLite storage for unconfirmed receptions, independent of the
//! confirmed order ledger. A preview on the workflow board is NOT itself a
//! persistence boundary; these working orders survive a process restart.
//! Only the existing step-1 '주문 확인 완료' action creates immutable billing
//! and receipt snapshots and makes a row eligible for 주문내역.
use std::collections::{HashMap,HashSet};
use rusqlite::{params, Connection};
use serde_json::{json,Value};
use tauri::State;
use super::{Database, err, lock_db};

pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS reception_working_orders (
            id TEXT PRIMARY KEY,
            changed_at TEXT NOT NULL,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE TABLE IF NOT EXISTS reception_confirmed_edits (
            id TEXT PRIMARY KEY,
            changed_at TEXT NOT NULL,
            base_payload TEXT NOT NULL CHECK(json_valid(base_payload)),
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

/// Recreate the FULL confirmed order from the relational three-table
/// schema. core_orders.payload intentionally omits recipients and item rows,
/// so comparing that JSON alone would miss external recipient/item changes.
fn confirmed_baselines(conn:&Connection)->Result<HashMap<String,Value>,String>{
    let mut stmt=conn.prepare("SELECT id,payload FROM core_orders ORDER BY position,id").map_err(err)?;
    let order_rows=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?
    ))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
    let mut orders=HashMap::new();
    for (id,raw) in order_rows{
        orders.insert(id,serde_json::from_str::<Value>(&raw).map_err(err)?);
    }
    let mut stmt=conn.prepare(
        "SELECT recipient_id,payload FROM core_items ORDER BY recipient_id,position,id"
    ).map_err(err)?;
    let item_rows=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?
    ))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
    let mut items:HashMap<String,Vec<Value>>=HashMap::new();
    for (owner,raw) in item_rows{
        items.entry(owner).or_default().push(serde_json::from_str(&raw).map_err(err)?);
    }
    let mut stmt=conn.prepare(
        "SELECT id,order_id,payload FROM core_recipients ORDER BY order_id,position,id"
    ).map_err(err)?;
    let recipient_rows=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?
    ))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
    let mut recipients:HashMap<String,Vec<Value>>=HashMap::new();
    for (id,owner,raw) in recipient_rows{
        let mut recipient:Value=serde_json::from_str(&raw).map_err(err)?;
        recipient["items"]=json!(items.remove(&id).unwrap_or_default());
        recipients.entry(owner).or_default().push(recipient);
    }
    if !items.is_empty(){return Err("품목을 소유한 수취인이 없습니다.".into());}
    for (id,order) in &mut orders{
        order["recipients"]=json!(recipients.remove(id).unwrap_or_default());
    }
    if !recipients.is_empty(){return Err("수취인이 속한 주문이 없습니다.".into());}
    Ok(orders)
}
fn prior_edit_baselines(conn:&Connection)->Result<HashMap<String,Value>,String>{
    let mut stmt=conn.prepare("SELECT id,base_payload FROM reception_confirmed_edits").map_err(err)?;
    let rows=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?
    ))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
    rows.into_iter().map(|(id,raw)|{
        Ok((id,serde_json::from_str::<Value>(&raw).map_err(err)?))
    }).collect()
}
pub(super) fn load(conn:&Connection)->Result<Vec<Value>,String>{
    let permanent=confirmed_order_ids(conn)?;
    let baselines=confirmed_baselines(conn)?;
    let mut stmt=conn.prepare(
        "SELECT payload FROM reception_working_orders ORDER BY changed_at DESC,id"
    ).map_err(err)?;
    let raws=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
    let mut orders=Vec::new();
    for raw in raws{
        let order:Value=serde_json::from_str(&raw).map_err(err)?;
        let id=order["id"].as_str().ok_or("접수 ID 누락")?;
        if !permanent.contains(id){orders.push(order);}
    }
    let mut stmt=conn.prepare(
        "SELECT id,base_payload,payload FROM reception_confirmed_edits ORDER BY changed_at DESC,id"
    ).map_err(err)?;
    let edits=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?
    ))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
    for (id,base_raw,raw) in edits{
        let base:Value=serde_json::from_str(&base_raw).map_err(err)?;
        if baselines.get(&id)!=Some(&base){
            // Never silently overwrite or delete the employee's working copy.
            return Err(format!("확정 주문 {id}의 DB 원본이 변경되어 복원을 중단했습니다. 수정본은 보존했습니다."));
        }
        let order:Value=serde_json::from_str(&raw).map_err(err)?;
        orders.push(order);
    }
    Ok(orders)
}

/// One atomically replaced working set, serialized by the frontend autosave
/// queue. Never touches the confirmed app_state/core_orders ledger or revision.
pub(super) fn replace(conn:&mut Connection,orders:Vec<Value>)->Result<(),String>{
    if orders.len()>1000{return Err("발주서 작업본 저장 한도를 초과했습니다.".into());}
    let permanent=confirmed_order_ids(conn)?;
    let baselines=confirmed_baselines(conn)?;
    let prior=prior_edit_baselines(conn)?;
    let mut seen=HashSet::new();
    let mut fresh=Vec::new();
    let mut edits=Vec::new();
    for order in &orders{
        let id=order["id"].as_str().filter(|id|!id.trim().is_empty())
            .ok_or("접수 ID 누락")?;
        if !seen.insert(id.to_string()){return Err("작업본 ID 중복".into());}
        let raw=serde_json::to_string(order).map_err(err)?;
        if raw.len()>50_000_000{return Err("발주서 용량 초과".into());}
        if permanent.contains(id){
            // Reconcile may still hold an old unconfirmed preview for a now
            // confirmed order: never revive it as a confirmed edit.
            if order["confirmedAt"].as_str().map_or(true,|s|s.is_empty()){continue;}
            let saved=baselines.get(id).ok_or("확정 주문 DB 본문이 없습니다.")?;
            let baseline=prior.get(id).unwrap_or(saved);
            if baseline!=saved {
                return Err(format!("확정 주문 {id}이 다른 작업에서 수정됐습니다. 작업본은 덮어쓰지 않았습니다."));
            }
            if order!=saved {
                edits.push((id.to_string(),raw,serde_json::to_string(baseline).map_err(err)?));
            }
        }else{
            if order["receptionVersion"].as_i64()!=Some(1) ||
                order["confirmedAt"].as_str().is_some_and(|s|!s.is_empty()){
                return Err("확정 주문 ID가 없는 작업본은 저장할 수 없습니다.".into());
            }
            fresh.push((id.to_string(),raw));
        }
    }
    // Validate every row before replacing either table. A failed write rolls
    // both collections back and leaves the last good edit intact.
    let tx=conn.transaction().map_err(err)?;
    // The legacy React autosave replaces its full snapshot. If egui added
    // a working order AFTER React last loaded the list, that replacement
    // must fail instead of silently deleting the new order. Existing egui
    // rows also use an optimistic per-row version to prevent stale writes.
    {
        let mut query=tx.prepare(
            "SELECT id,json_extract(payload,'$._eguiBridgeVersion')
             FROM reception_working_orders
             WHERE json_type(payload,'$._eguiBridgeVersion')='integer'"
        ).map_err(err)?;
        let rows=query.query_map([],|row|
            Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?))
        ).map_err(err)?;
        for row in rows {
            let (id,version)=row.map_err(err)?;
            if permanent.contains(&id) {continue;}
            let incoming=orders.iter().find(|order|
                order["id"].as_str()==Some(&id)
            ).ok_or_else(||
                format!("egui에서 접수한 주문 {id}이 다른 창에서 추가됐습니다. 최신 주문을 불러온 뒤 저장하세요.")
            )?;
            if incoming["_eguiBridgeVersion"].as_i64()!=Some(version) {
                return Err(format!("egui 작업본 {id}이 다른 창에서 변경됐습니다. 새로고침 후 저장하세요."));
            }
        }
    }
    tx.execute("DELETE FROM reception_working_orders",[]).map_err(err)?;
    tx.execute("DELETE FROM reception_confirmed_edits",[]).map_err(err)?;
    let timestamp=chrono::Utc::now().to_rfc3339();
    for (id,raw) in fresh{
        tx.execute(
            "INSERT INTO reception_working_orders(id,changed_at,payload) VALUES(?1,?2,?3)",
            params![id,timestamp,raw]
        ).map_err(err)?;
    }
    for (id,raw,base) in edits{
        tx.execute(
            "INSERT INTO reception_confirmed_edits(id,changed_at,base_payload,payload) VALUES(?1,?2,?3,?4)",
            params![id,timestamp,base,raw]
        ).map_err(err)?;
    }
    tx.commit().map_err(err)
}

#[tauri::command]
pub(super) fn working_receptions_load(db: State<'_, Database>) -> Result<Vec<Value>, String> {
    let conn = lock_db(&db)?;
    load(&*conn)
}

#[tauri::command]
pub(super) fn working_receptions_replace(
    db: State<'_, Database>, orders: Vec<Value>,
) -> Result<(), String> {
    let mut conn = lock_db(&db)?;
    replace(&mut *conn, orders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn db() -> Connection {
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE order_index(id TEXT PRIMARY KEY);
             CREATE TABLE core_orders(id TEXT PRIMARY KEY,position INTEGER NOT NULL DEFAULT 0,payload TEXT NOT NULL);
             CREATE TABLE core_recipients(id TEXT PRIMARY KEY,order_id TEXT NOT NULL,position INTEGER NOT NULL DEFAULT 0,payload TEXT NOT NULL);
             CREATE TABLE core_items(id TEXT PRIMARY KEY,recipient_id TEXT NOT NULL,position INTEGER NOT NULL DEFAULT 0,payload TEXT NOT NULL);"
        ).unwrap();
        initialize(&conn).unwrap();
        conn
    }
    #[test]
    fn bulk_autosave_cannot_erase_or_rewind_egui_working_receipt() {
        let mut conn=db();
        let order=json!({"id":"egui-test","receptionVersion":1,
            "raw":"접수 원문","_eguiBridgeVersion":2});
        conn.execute(
            "INSERT INTO reception_working_orders(id,changed_at,payload)
             VALUES('egui-test','now',?1)",
            [order.to_string()]
        ).unwrap();
        assert!(replace(&mut conn,vec![]).unwrap_err().contains("다른 창"));
        let mut stale=order.clone();
        stale["_eguiBridgeVersion"]=json!(1);
        assert!(replace(&mut conn,vec![stale]).unwrap_err().contains("다른 창"));
        assert_eq!(load(&conn).unwrap(),vec![order.clone()]);
        replace(&mut conn,vec![order.clone()]).unwrap();
        assert_eq!(load(&conn).unwrap(),vec![order]);
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
            conn.execute_batch(
            "CREATE TABLE order_index(id TEXT PRIMARY KEY);
             CREATE TABLE core_orders(id TEXT PRIMARY KEY,position INTEGER NOT NULL DEFAULT 0,payload TEXT NOT NULL);
             CREATE TABLE core_recipients(id TEXT PRIMARY KEY,order_id TEXT NOT NULL,position INTEGER NOT NULL DEFAULT 0,payload TEXT NOT NULL);
             CREATE TABLE core_items(id TEXT PRIMARY KEY,recipient_id TEXT NOT NULL,position INTEGER NOT NULL DEFAULT 0,payload TEXT NOT NULL);"
        ).unwrap();
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
    fn saved_order_edit_stays_out_of_confirmed_ledger() {
        let mut conn=db();
        let frozen=json!({"id":"c","confirmedAt":"2026-11-19T00:00:00Z","raw":"원본"});
        conn.execute("INSERT INTO order_index(id) VALUES('c')",[]).unwrap();
        conn.execute("INSERT INTO core_orders(id,payload) VALUES('c',?1)",[frozen.to_string()]).unwrap();
        let mut edited=frozen.clone();edited["raw"]=json!("작업자 수정");
        replace(&mut conn,vec![edited.clone()]).unwrap();
        assert_eq!(load(&conn).unwrap(),vec![edited.clone()]);
        let stored:String=conn.query_row("SELECT payload FROM core_orders WHERE id='c'",[],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(),json!({
            "id":"c","confirmedAt":"2026-11-19T00:00:00Z","raw":"원본"
        }));
        // A later writer changing the frozen ledger cannot be overwritten by
        // the old local edit. The working copy remains in SQLite for recovery.
        let mut changed=frozen.clone();changed["raw"]=json!("다른 세션 변경");
        conn.execute("UPDATE core_orders SET payload=?1 WHERE id='c'",[changed.to_string()]).unwrap();
        assert!(load(&conn).is_err());
        assert!(replace(&mut conn,vec![edited]).is_err());
        let still:String=conn.query_row(
            "SELECT payload FROM reception_confirmed_edits WHERE id='c'",[],|r|r.get(0)).unwrap();
        assert!(still.contains("작업자 수정"));
    }
    #[test]
    fn normalized_recipient_changes_invalidate_confirmed_edit_baseline() {
        let mut conn=db();
        let frozen=json!({"id":"full","confirmedAt":"now","raw":"원본","recipients":[
            {"id":"r1","name":"가상","items":[{"id":"i1","quantity":1}]}
        ]});
        conn.execute("INSERT INTO order_index(id) VALUES('full')",[]).unwrap();
        conn.execute("INSERT INTO core_orders(id,payload) VALUES('full',?1)",
            [json!({"id":"full","confirmedAt":"now","raw":"원본"}).to_string()]).unwrap();
        conn.execute("INSERT INTO core_recipients(id,order_id,payload) VALUES('r1','full',?1)",
            [json!({"id":"r1","name":"가상"}).to_string()]).unwrap();
        conn.execute("INSERT INTO core_items(id,recipient_id,payload) VALUES('i1','r1',?1)",
            [json!({"id":"i1","quantity":1}).to_string()]).unwrap();
        let mut edit=frozen.clone();edit["raw"]=json!("작업자 수정");
        replace(&mut conn,vec![edit.clone()]).unwrap();
        assert_eq!(load(&conn).unwrap(),vec![edit.clone()]);
        conn.execute("UPDATE core_items SET payload=?1 WHERE id='i1'",
            [json!({"id":"i1","quantity":2}).to_string()]).unwrap();
        assert!(load(&conn).is_err());
        assert!(replace(&mut conn,vec![edit]).is_err());
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
