//! Shared, UI-independent relational snapshot hydration for schema v2.
//!
//! Both the legacy Tauri host and the egui host must reconstruct exactly the
//! same business objects from the same SQLite rows. The caller owns the
//! connection and the transaction: this module never opens, creates, writes,
//! migrates or commits a database.

use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;

fn read_entities(conn: &Connection, sql: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn.prepare(sql)
        .map_err(|_| "운영 업무자료 조회를 준비하지 못했습니다.".to_owned())?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| "운영 업무자료 조회에 실패했습니다.".to_owned())?;
    rows.map(|row| {
        let raw = row.map_err(|_| "운영 업무자료 행을 읽지 못했습니다.".to_owned())?;
        serde_json::from_str(&raw)
            .map_err(|_| "운영 업무자료 JSON 형식이 잘못되었습니다.".to_owned())
    }).collect()
}

fn read_children(
    conn: &Connection,
    sql: &str,
) -> Result<HashMap<String, Vec<Value>>, String> {
    let mut stmt = conn.prepare(sql)
        .map_err(|_| "운영 하위자료 조회를 준비하지 못했습니다.".to_owned())?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }).map_err(|_| "운영 하위자료 조회에 실패했습니다.".to_owned())?;
    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    for row in rows {
        let (owner, raw) = row
            .map_err(|_| "운영 하위자료 행을 읽지 못했습니다.".to_owned())?;
        let child: Value = serde_json::from_str(&raw)
            .map_err(|_| "운영 하위자료 JSON 형식이 잘못되었습니다.".to_owned())?;
        grouped.entry(owner).or_default().push(child);
    }
    Ok(grouped)
}

fn attach(
    parents: &mut [Value],
    key: &str,
    mut children: HashMap<String, Vec<Value>>,
) -> Result<(), String> {
    for parent in parents {
        let id = parent.get("id").and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("운영 자료의 ID가 누락되었습니다.")?.to_owned();
        let nested = children.remove(&id).unwrap_or_default();
        parent.as_object_mut()
            .ok_or("운영 자료 객체 형식이 잘못되었습니다.")?
            .insert(key.to_owned(), json!(nested));
    }
    if !children.is_empty() {
        return Err("운영 업무자료에 소속되지 않은 하위 기록이 있습니다.".into());
    }
    Ok(())
}

const CORE_CLIENTS_SQL: &str = "SELECT payload FROM core_clients ORDER BY position,id";

/// The name-only client index has a deliberately smaller read budget than
/// the operational snapshot. Share these limits with the egui JSON index.
pub const CLIENT_INDEX_MAX_ROWS: usize = 5_000;
pub const CLIENT_INDEX_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Read only the client master, with strict bounds for the separate
/// contact-light name index. Unlike the full business snapshot, this query
/// does not open order, product or pricing tables.
pub fn read_client_index_rows(conn: &Connection) -> Result<Vec<Value>, String> {
    let mut stmt = conn.prepare(CORE_CLIENTS_SQL)
        .map_err(|_| "거래처 목록을 읽을 수 없습니다.".to_owned())?;
    let mut query = stmt.query([])
        .map_err(|_| "거래처 목록 조회에 실패했습니다.".to_owned())?;
    let mut clients = Vec::new();
    let mut bytes = 0usize;
    while let Some(row) = query.next()
        .map_err(|_| "거래처 목록 읽기에 실패했습니다.".to_owned())? {
        if clients.len() >= CLIENT_INDEX_MAX_ROWS {
            return Err("거래처 목록이 최대 5,000건을 초과했습니다.".into());
        }
        let raw: String = row.get(0)
            .map_err(|_| "거래처 자료가 손상됐습니다.".to_owned())?;
        bytes = bytes.checked_add(raw.len())
            .ok_or("거래처 목록 크기를 확인할 수 없습니다.")?;
        if bytes > CLIENT_INDEX_MAX_BYTES {
            return Err("거래처 목록이 최대 2MB를 초과했습니다.".into());
        }
        clients.push(serde_json::from_str::<Value>(&raw)
            .map_err(|_| "거래처 JSON 자료가 손상됐습니다.".to_owned())?);
    }
    Ok(clients)
}

/// Read the complete v2 business snapshot through the same relational
/// projector in every desktop host. The caller owns the SQLite connection,
/// snapshot consistency and schema-v2 verification; this function never writes.
pub fn read_v2_snapshot(conn: &Connection) -> Result<Value, String> {
    let raw: String = conn.query_row(
        "SELECT payload FROM app_state WHERE id=1",
        [],
        |row| row.get(0),
    ).map_err(|_| "운영 업무 상태를 읽지 못했습니다.".to_owned())?;
    let mut state: Value = serde_json::from_str(&raw)
        .map_err(|_| "운영 업무 상태의 JSON 형식이 잘못되었습니다.".to_owned())?;
    hydrate_v2(conn, &mut state)?;
    Ok(state)
}

/// Hydrate only an already-verified schema-v2 snapshot.
///
/// The caller must hold a consistent read snapshot or a write transaction.
/// Retains array ordering, nested variants/items/recipients and the legacy
/// app_state envelope without recomputing or modifying any business values.
pub fn hydrate_v2(conn: &Connection, state: &mut Value) -> Result<(), String> {
    let mut products = read_entities(
        conn,
        "SELECT payload FROM core_products ORDER BY position,id",
    )?;
    let prices = read_children(
        conn,
        "SELECT product_id,payload FROM core_product_prices ORDER BY product_id",
    )?;
    attach(&mut products, "variants", prices)?;

    let clients = read_entities(
        conn,
        CORE_CLIENTS_SQL,
    )?;
    let mut orders = read_entities(
        conn,
        "SELECT payload FROM core_orders ORDER BY position,id",
    )?;
    let mut recipients = read_entities(
        conn,
        "SELECT payload FROM core_recipients ORDER BY position,id",
    )?;
    let items = read_children(
        conn,
        "SELECT recipient_id,payload FROM core_items ORDER BY recipient_id,position,id",
    )?;
    attach(&mut recipients, "items", items)?;

    let mut stmt = conn.prepare(
        "SELECT order_id,id FROM core_recipients ORDER BY order_id,position,id",
    ).map_err(|_| "수취인 소속 조회 준비 실패".to_owned())?;
    let owners = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }).map_err(|_| "수취인 소속 조회 실패".to_owned())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| "수취인 소속 결과 오류".to_owned())?;
    drop(stmt);

    let mut by_id: HashMap<String, Value> = HashMap::new();
    for recipient in recipients {
        let id = recipient.get("id").and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("수취인 ID 누락")?.to_owned();
        if by_id.insert(id, recipient).is_some() {
            return Err("중복 수취인 ID가 있습니다.".into());
        }
    }

    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    for (order_id, recipient_id) in owners {
        let recipient = by_id.remove(&recipient_id)
            .ok_or("수취인 소속 기록 불일치")?;
        grouped.entry(order_id).or_default().push(recipient);
    }
    if !by_id.is_empty() {
        return Err("연결되지 않은 수취인 기록이 있습니다.".into());
    }
    attach(&mut orders, "recipients", grouped)?;

    let root = state.as_object_mut()
        .ok_or("업무 상태는 객체여야 합니다.")?;
    root.insert("products".into(), json!(products));
    root.insert("clients".into(), json!(clients));
    root.insert("orders".into(), json!(orders));
    Ok(())
}
