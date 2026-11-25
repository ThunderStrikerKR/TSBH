//! Same authoritative v2 snapshot transaction for Tauri and egui.
//! Validates the current and requested state, synchronizes core relation rows,
//! legacy indexes and immutable receipt archive, and commits atomically.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json,Value};
use crate::{validate_brands,validate_clients};
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}
#[path = "../../../src-tauri/src/reception.rs"]
mod reception;

fn read_current(tx:&Transaction<'_>,expected_revision:u64)->Result<Value,String>{
    let version:i64=tx.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(err)?;
    if version!=2 {return Err("지원하지 않는 운영 DB 버전입니다.".into());}
    // The envelope owns revision. Reject stale writers before loading any
    // relational rows, using the same transaction snapshot.
    let mut current=crate::read_envelope(tx)?;
    if current["revision"].as_u64()!=Some(expected_revision){
        return Err("다른 작업에서 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    crate::hydrate_v2(tx,&mut current)?;
    Ok(current)
}

pub fn write_v2_snapshot(conn: &mut Connection, mut state: Value, expected_revision: u64, validate_orders: bool) -> Result<Value, String> {
    crate::product_colors::normalize_products(&mut state)?;
    crate::valid_state(&state)?;
    let tx=conn.transaction().map_err(err)?;
    let current=read_current(&tx,expected_revision)?;
    let (written,_)=write_validated(&tx,&current,state,expected_revision,validate_orders)?;
    tx.commit().map_err(err)?;
    Ok(written)
}

/// A native read-modify-write uses one consistent before-image for the
/// reducer AND all write validations. No UI-supplied snapshot is trusted.
/// The deferred transaction rolls back on reducer/validation errors or if
/// a concurrent writer makes the read snapshot impossible to upgrade.
pub fn update_v2_snapshot<F>(
    conn:&mut Connection,expected_revision:u64,validate_orders:bool,mutate:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<Value,String>{
    let tx=conn.transaction().map_err(err)?;
    let current=read_current(&tx,expected_revision)?;
    let mut state=mutate(&current)?;
    crate::product_colors::normalize_products(&mut state)?;
    crate::valid_state(&state)?;
    let changes_before=total_changes(&tx)?;
    let (written,envelope_only)=write_validated(&tx,&current,state,expected_revision,validate_orders)?;
    let verified=verify_written(&tx,current,written,envelope_only,changes_before)?;
    tx.commit().map_err(err)?;
    Ok(verified)
}

// SQLite's total_changes includes trigger and foreign-key writes, unlike
// changes(). Only a single app_state UPDATE with no other writes can reuse
// the authoritative core rows already hydrated in this same transaction.
fn total_changes(tx:&Transaction<'_>)->Result<i64,String>{
    tx.query_row("SELECT total_changes()",[],|row|row.get(0)).map_err(err)
}

fn verify_written(tx:&Transaction<'_>,mut current:Value,written:Value,
    envelope_only:bool,changes_before:i64)->Result<Value,String>{
    let revision=written["revision"].as_u64().ok_or("업무 저장 revision 누락")?;
    let verified=if envelope_only &&
        total_changes(tx)?.checked_sub(changes_before)==Some(1){
        // Read the actual persisted envelope, not the requested settings.
        // No core SQL ran and SQLite reports no additional row writes.
        let mut envelope=crate::read_envelope(tx)?;
        let root=envelope.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
        let prior=current.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
        for key in ["products","clients","orders"]{
            let value=prior.remove(key).ok_or("기존 업무자료 누락")?;
            root.insert(key.to_owned(),value);
        }
        envelope
    }else{
        // Changed core data or unexpected trigger/cascade writes: retain
        // the original full persisted verification, still before COMMIT.
        drop(current);
        drop(written);
        crate::read_v2_snapshot(tx)?
    };
    if verified["revision"].as_u64()!=Some(revision){
        return Err("업무 저장 검증에 실패했습니다.".into());
    }
    Ok(verified)
}

fn write_validated(tx:&Transaction<'_>,current:&Value,mut state:Value,
    current_revision:u64,validate_orders:bool)->Result<(Value,bool),String>{
    let products_changed = current["products"] != state["products"];
    let clients_changed = current["clients"] != state["clients"];
    let orders_changed = current["orders"] != state["orders"];
    if clients_changed {
        validate_clients(current, &state)?;
    }else{
        crate::validation::validate_unchanged_clients(&state)?;
    }
    validate_brands(current, &state)?;
    if validate_orders {
        if orders_changed {
            reception::validate_reception(current, &state)?;
        }else{
            reception::validate_order_ids(&state)?;
        }
    }
    // Only skip archive generation if equality and unique order IDs were
    // both established. The legacy validate_orders=false route retains
    // its original archive checks.
    let archive_entries=if validate_orders && !orders_changed{
        Vec::new()
    }else{
        reception::new_archive_entries(current,&state)?
    };
    let envelope_only=!products_changed && !clients_changed &&
        !orders_changed && archive_entries.is_empty();
    state["revision"] = json!(current_revision.checked_add(1)
        .ok_or("운영 DB revision 범위를 초과했습니다.")?);
    // The constrained SQLite tables own products, clients and orders.
    // Keep only non-core data in the compatibility envelope, atomically.
    // Settings-only changes never rewrite unchanged core rows.
    if products_changed || clients_changed || orders_changed {
        crate::relational_write::store_v2_relational_delta(tx,current,&state)?;
    }
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
        [serialize_envelope(&state)?]).map_err(err)?;
    // Legacy search indexes are projections of the same before-image.
    // Child-only order edits do not change indexed columns.
    if products_changed {
        sync_product_index(tx,current,&state)?;
    }
    if orders_changed {
        sync_order_index(tx,current,&state)?;
    }
    for (id, order_id, record) in archive_entries {
        // Never silently discard a colliding audit ID with a different owner or payload.
        let stored: Option<(String, String)> = tx.query_row(
            "SELECT order_id,record FROM receipt_archive WHERE id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(err)?;
        if let Some((stored_owner, stored_payload)) = stored {
            let stored_record: Value = serde_json::from_str(&stored_payload).map_err(err)?;
            if stored_owner != order_id || stored_record != record {
                return Err("기존 원문 감사 기록과 ID가 충돌합니다. 변경을 저장하지 않았습니다.".into());
            }
        } else {
            tx.execute(
                "INSERT INTO receipt_archive(id,order_id,record) VALUES (?1,?2,?3)",
                params![id,order_id,serde_json::to_string(&record).map_err(err)?],
            ).map_err(err)?;
        }
    }
    Ok((state,envelope_only))
}


// Serialize borrowed non-core fields only. Core rows have their own tables;
// do not clone their nested products, variants, orders or items just to
// discard them. Unknown envelope keys are preserved for compatibility.
fn serialize_envelope(state:&Value)->Result<String,String>{
    let root=state.as_object().ok_or("업무 상태는 객체여야 합니다.")?;
    let envelope:std::collections::BTreeMap<&str,&Value>=root.iter()
        .filter(|(key,_)| !matches!(key.as_str(),"products"|"clients"|"orders"))
        .map(|(key,value)|(key.as_str(),value)).collect();
    serde_json::to_string(&envelope).map_err(err)
}

fn index_rows<'a>(value:&'a Value,key:&str)->Result<std::collections::HashMap<&'a str,&'a Value>,String>{
    let mut map=std::collections::HashMap::new();
    for row in value[key].as_array().ok_or("검색 인덱스 목록 오류")?{
        let id=row["id"].as_str().ok_or("검색 인덱스 ID 누락")?;
        if map.insert(id,row).is_some(){return Err("검색 인덱스 ID 중복".into());}
    }
    Ok(map)
}
fn sync_product_index(tx:&Transaction<'_>,before:&Value,state:&Value)->Result<(),String>{
    let old=index_rows(before,"products")?;
    let next=index_rows(state,"products")?;
    let mut insert=tx.prepare("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)
        ON CONFLICT(id) DO UPDATE SET code=excluded.code,name=excluded.name,payload=excluded.payload").map_err(err)?;
    for (id,product) in &next{
        if old.get(id)==Some(product){continue;}
        insert.execute(params![id,product["code"].as_str().ok_or("상품 코드 누락")?,
            product["name"].as_str().ok_or("상품명 누락")?,
            serde_json::to_string(product).map_err(err)?]).map_err(err)?;
    }
    for id in old.keys(){
        if !next.contains_key(id){tx.execute("DELETE FROM product_index WHERE id=?1",[id]).map_err(err)?;}
    }
    Ok(())
}
fn order_index_fields(order:&Value)->(&str,&str,Option<&str>,&str){
    (order["clientId"].as_str().unwrap_or(""),
        order["createdAt"].as_str().unwrap_or(""),order["confirmedAt"].as_str(),
        order["status"].as_str().unwrap_or("REVIEW"))
}
fn sync_order_index(tx:&Transaction<'_>,before:&Value,state:&Value)->Result<(),String>{
    let old=index_rows(before,"orders")?;
    let next=index_rows(state,"orders")?;
    let mut insert=tx.prepare("INSERT INTO order_index(id,client_id,created_at,confirmed_at,status)
        VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET
        client_id=excluded.client_id,created_at=excluded.created_at,
        confirmed_at=excluded.confirmed_at,status=excluded.status").map_err(err)?;
    for (id,order) in &next{
        let fields=order_index_fields(order);
        if old.get(id).is_some_and(|row|order_index_fields(row)==fields){continue;}
        insert.execute(params![id,fields.0,fields.1,fields.2,fields.3]).map_err(err)?;
    }
    for id in old.keys(){
        if !next.contains_key(id){tx.execute("DELETE FROM order_index WHERE id=?1",[id]).map_err(err)?;}
    }
    Ok(())
}

#[cfg(test)]
mod snapshot_cost_tests {
    use super::*;

    #[test]
    fn borrowed_envelope_preserves_all_non_core_fields(){
        let state=json!({
            "revision":42,"products":[{"variants":[{"id":"v"}]}],
            "clients":[{"id":"c"}],"orders":[{"recipients":[{"items":[{"id":"i"}]}]}],
            "workflow":{"events":[{"id":"e","detail":"배송"}]},
            "settings":{"vat":10},"future_field":{"nested":[null,true,3,"한글"]}
        });
        let mut expected=state.clone();
        for key in ["products","clients","orders"]{
            expected.as_object_mut().unwrap().remove(key);
        }
        let actual:Value=serde_json::from_str(&serialize_envelope(&state).unwrap()).unwrap();
        assert_eq!(actual,expected);
        assert_eq!(state["orders"][0]["recipients"][0]["items"][0]["id"],"i");
        assert!(serialize_envelope(&Value::Null).is_err());
    }

    #[test]
    fn envelope_only_verification_reuses_core_rows_and_falls_back_on_trigger(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE side_effect(id INTEGER PRIMARY KEY);
            INSERT INTO app_state VALUES (1,'{}');").unwrap();
        let current=json!({"revision":1,"products":[{"id":"p"}],
            "clients":[{"id":"c"}],"orders":[{"id":"o"}]});
        let mut written=current.clone();
        written["revision"]=json!(2);
        written["settings"]=json!({"uiScale":1.2});
        {
            let tx=conn.transaction().unwrap();
            let before=total_changes(&tx).unwrap();
            tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
                [serialize_envelope(&written).unwrap()]).unwrap();
            // There are deliberately no core tables. Successful verification
            // proves they were not queried again for the envelope-only write.
            assert_eq!(verify_written(&tx,current.clone(),written.clone(),true,before).unwrap(),written);
            tx.rollback().unwrap();
        }
        conn.execute_batch("CREATE TRIGGER unexpected AFTER UPDATE ON app_state
            BEGIN INSERT INTO side_effect VALUES (1); END;").unwrap();
        {
            let tx=conn.transaction().unwrap();
            let before=total_changes(&tx).unwrap();
            tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
                [serialize_envelope(&written).unwrap()]).unwrap();
            // A trigger write must force the original full read, which fails
            // here because core tables are intentionally absent.
            assert!(verify_written(&tx,current,written,true,before).is_err());
            tx.rollback().unwrap();
        }
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM side_effect",[],
            |row|row.get::<_,i64>(0)).unwrap(),0);
    }

    #[test]
    fn stale_revision_is_rejected_before_relational_hydration(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_schema(key TEXT,version INTEGER);
            INSERT INTO core_schema VALUES ('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        conn.execute("INSERT INTO app_state VALUES (1,?1)",
            [json!({"revision":7}).to_string()]).unwrap();
        // Deliberately omit every relational table: stale requests must
        // fail on revision, not try to hydrate, and never invoke the reducer.
        let error=update_v2_snapshot(&mut conn,6,true,|_|{
            panic!("stale revision must not execute the reducer")
        }).unwrap_err();
        assert!(error.contains("다른 작업에서 데이터가 수정되었습니다"));
        let error=update_v2_snapshot(&mut conn,7,true,|_|{
            panic!("invalid relational snapshot must not execute the reducer")
        }).unwrap_err();
        assert!(!error.contains("다른 작업에서 데이터가 수정되었습니다"));
        let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],
            |row|row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap()["revision"],7);
    }
}
