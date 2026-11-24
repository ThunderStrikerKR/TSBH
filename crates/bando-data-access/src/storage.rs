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
    let current=crate::read_v2_snapshot(tx)?;
    if current["revision"].as_u64()!=Some(expected_revision){
        return Err("다른 작업에서 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    Ok(current)
}

pub fn write_v2_snapshot(conn: &mut Connection, mut state: Value, expected_revision: u64, validate_orders: bool) -> Result<Value, String> {
    crate::product_colors::normalize_products(&mut state)?;
    crate::valid_state(&state)?;
    let tx=conn.transaction().map_err(err)?;
    let current=read_current(&tx,expected_revision)?;
    let written=write_validated(&tx,&current,state,expected_revision,validate_orders)?;
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
    let written=write_validated(&tx,&current,state,expected_revision,validate_orders)?;
    // Read the persisted relational projection while still holding the
    // transaction. A failed verification must roll back, not report a
    // failure after a successful commit.
    let verified=crate::read_v2_snapshot(&tx)?;
    if verified["revision"]!=written["revision"]{
        return Err("업무 저장 검증에 실패했습니다.".into());
    }
    tx.commit().map_err(err)?;
    Ok(verified)
}

fn write_validated(tx:&Transaction<'_>,current:&Value,mut state:Value,
    current_revision:u64,validate_orders:bool)->Result<Value,String>{
    validate_clients(current, &state)?;
    validate_brands(current, &state)?;
    if validate_orders { reception::validate_reception(current, &state)?; }
    let archive_entries = reception::new_archive_entries(current, &state)?;
    let products_changed = current["products"] != state["products"];
    let clients_changed = current["clients"] != state["clients"];
    let orders_changed = current["orders"] != state["orders"];
    state["revision"] = json!(current_revision.checked_add(1)
        .ok_or("운영 DB revision 범위를 초과했습니다.")?);
    // The constrained SQLite tables own products, clients and orders.
    // Keep only non-core data in the compatibility envelope, atomically.
    let mut envelope = state.clone();
    // Settings-only changes never rewrite unchanged core rows.
    if products_changed || clients_changed || orders_changed {
        crate::store_v2_relational(tx, &state)?;
    }
    let root=envelope.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    for key in ["products","clients","orders"] {root.remove(key);}
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
        [serde_json::to_string(&envelope).map_err(err)?]).map_err(err)?;
    // Maintain legacy search indexes only when their actual domain changed.
    if products_changed {
    tx.execute("DELETE FROM product_index", []).map_err(err)?;
    {
        let mut insert = tx.prepare("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)").map_err(err)?;
        for product in state["products"].as_array().ok_or("상품 데이터 오류")? {
            let id = product["id"].as_str().ok_or("상품 ID 누락")?;
            let code = product["code"].as_str().ok_or("상품 코드 누락")?;
            let name = product["name"].as_str().ok_or("상품명 누락")?;
            insert.execute(params![id, code, name, serde_json::to_string(product).map_err(err)?]).map_err(err)?;
        }
    }
    }
    // Order index and the append-only reception archive share the snapshot transaction.
    if orders_changed {
    tx.execute("DELETE FROM order_index", []).map_err(err)?;
    {
        let mut insert = tx.prepare("INSERT INTO order_index(id,client_id,created_at,confirmed_at,status) VALUES (?1,?2,?3,?4,?5)").map_err(err)?;
        for order in state["orders"].as_array().ok_or("주문 데이터 오류")? {
            insert.execute(params![
                order["id"].as_str().ok_or("주문 ID 누락")?,
                order["clientId"].as_str().unwrap_or(""),
                order["createdAt"].as_str().unwrap_or(""),
                order["confirmedAt"].as_str(),
                order["status"].as_str().unwrap_or("REVIEW")
            ]).map_err(err)?;
        }
    }
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
    Ok(state)
}
