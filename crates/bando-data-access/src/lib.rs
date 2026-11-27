//! Shared, UI-independent relational snapshot hydration for schema v2.
//!
//! Both the legacy Tauri host and the egui host must reconstruct exactly the
//! same business objects from the same SQLite rows. The caller owns the
//! connection; only explicitly invoked write_v2_snapshot commits changes.
//! Database selection, schema creation and migration remain host-owned.

// Native catalog modules are compiled both inside this shared crate and
// inside the Tauri/parity hosts. Resolve their shared names consistently.
extern crate self as bando_data_access;

use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;

mod validation;
// Read-only analysis interface for the upcoming validation planner.
pub mod change_set;
pub mod validation_dependencies;
pub mod validation_gate;
mod validation_transitions;
mod validation_policy;
mod category_assignment;
pub use category_assignment::{validate_product_assignment,validate_category_transition};
mod relational_write;
mod storage;
pub mod notification_store;
pub mod workflow_events;
pub mod workflow_state;
mod client_commands;
mod product_commands;
mod settings_commands;
// Keep the original business_core module names: nested native manual
// classification rules resolve super::super::recognition.
#[path = "../../../src-tauri/src/business_core/rules/mod.rs"]
mod rules;
pub use rules::terminology;
#[path = "../../../src-tauri/src/business_core/recognition/mod.rs"]
mod recognition;
mod reception_manual;
mod reception_commands;
mod recovery;
pub mod backup_service;
pub mod client_names;
pub mod product_colors;
pub mod audit_service;
pub mod shipping_action;
pub use validation::{
    valid_state, validate_brands, validate_clients,
    validate_message_templates, validate_provisional_product_notices,
};
pub use relational_write::store_v2_relational;
pub use storage::{write_v2_snapshot,update_v2_snapshot,update_v2_snapshot_if_changed,update_v2_freight_snapshot,update_v2_shipment_snapshot,update_v2_factory_reply_snapshot,update_v2_factory_correction_snapshot};
pub use client_commands::{mutate_client_v2,mutate_client_checked_v2,preview_client_workbook,commit_client_workbook_v2};
pub use product_commands::{patch_product_v2,patch_model_groups_v2,apply_product_price,
    preview_unified_product_workbook,management_scope_token};
pub use storage::{update_v2_entity_if_changed,is_factory_reply_revision_conflict};
pub(crate) use product_commands::validate_native_model_groups;
pub use settings_commands::{apply_settings_changes,patch_settings_v2};
pub use reception_commands::commit_reception_v2;


fn read_entities(conn: &Connection, sql: &str) -> Result<Vec<Value>, String> {
    read_entities_with(conn,sql,[])
}

fn read_entities_with(conn:&Connection,sql:&str,params:impl rusqlite::Params)
    ->Result<Vec<Value>,String>{
    let mut stmt = conn.prepare(sql)
        .map_err(|_| "운영 업무자료 조회를 준비하지 못했습니다.".to_owned())?;
    let rows = stmt.query_map(params, |row| row.get::<_, String>(0))
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
    read_children_with(conn,sql,[])
}

fn read_children_with(conn:&Connection,sql:&str,params:impl rusqlite::Params)
    ->Result<HashMap<String,Vec<Value>>,String>{
    let mut stmt = conn.prepare(sql)
        .map_err(|_| "운영 하위자료 조회를 준비하지 못했습니다.".to_owned())?;
    let rows = stmt.query_map(params, |row| {
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
            .insert(key.to_owned(), Value::Array(nested));
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

/// The v3 physical root is deliberately not an object: older v2 projectors
/// cannot mistake a split ledger for an ordinary incomplete snapshot.
pub fn supported_relational_version(version:i64)->bool{matches!(version,2|3)}

pub(crate) fn decode_physical_envelope(conn:&Connection,raw:&str)->Result<Value,String>{
    let version=recovery::schema_version(conn)?;
    if version>3{return Err("이 프로그램보다 새로운 DB 구조입니다.".into());}
    let split=workflow_events::active(conn)?;
    if split!=(version==3){return Err("업무 이벤트 테이블과 DB 버전 불일치".into());}
    let mut stored:Value=serde_json::from_str(raw)
        .map_err(|_|"운영 업무 상태의 JSON 형식이 잘못되었습니다.".to_owned())?;
    if version==3{
        let items=stored.as_array_mut().ok_or("DB v3 저장 형식 불일치")?;
        if items.len()!=1 || !items[0].is_object(){return Err("DB v3 저장 형식 불일치".into());}
        return Ok(items.pop().expect("checked single element"));
    }
    Ok(stored)
}

pub(crate) fn encode_physical_envelope(conn:&Connection,state:&Value)->Result<String,String>{
    if !state.is_object(){return Err("업무 상태는 객체여야 합니다.".into());}
    let version=recovery::schema_version(conn)?;
    match version{
        0|1|2=>serde_json::to_string(state),
        3=>serde_json::to_string(&[state]),
        _=>return Err("지원하지 않는 DB 저장 형식입니다.".into()),
    }.map_err(|e|e.to_string())
}

fn read_envelope(conn: &Connection) -> Result<Value, String> {
    let raw: String = conn.query_row(
        "SELECT payload FROM app_state WHERE id=1",
        [],
        |row| row.get(0),
    ).map_err(|_| "운영 업무 상태를 읽지 못했습니다.".to_owned())?;
    let mut state=decode_physical_envelope(conn,&raw)?;
    workflow_events::hydrate(conn,&mut state)?;
    workflow_state::hydrate(conn,&mut state)?;
    Ok(state)
}

fn read_product_entities(conn:&Connection)->Result<Vec<Value>,String>{
    let mut products = read_entities(
        conn,
        "SELECT payload FROM core_products ORDER BY position,id",
    )?;
    let prices = read_children(
        conn,
        "SELECT product_id,payload FROM core_product_prices ORDER BY product_id",
    )?;
    attach(&mut products, "variants", prices)?;
    Ok(products)
}

fn read_catalog_entities(conn: &Connection) -> Result<(Vec<Value>, Vec<Value>), String> {
    Ok((read_product_entities(conn)?,read_entities(conn, CORE_CLIENTS_SQL)?))
}

/// Read the complete v2 business snapshot through the same relational
/// projector in every desktop host. The caller owns the SQLite connection,
/// snapshot consistency and schema-v2 verification; this function never writes.
pub fn read_v2_snapshot(conn: &Connection) -> Result<Value, String> {
    let mut state = read_envelope(conn)?;
    hydrate_v2(conn, &mut state)?;
    Ok(state)
}

/// Read only product, price, client and envelope metadata for catalog screens.
/// Excludes order, recipient and item tables: those are never returned by the
/// egui load_catalog contract. The caller maintains one read transaction and
/// verifies the schema before calling this function.
pub fn read_v2_catalog_snapshot(conn: &Connection) -> Result<Value, String> {
    let mut state = read_envelope(conn)?;
    let (products, clients) = read_catalog_entities(conn)?;
    let root = state.as_object_mut()
        .ok_or("업무 상태는 객체여야 합니다.")?;
    root.insert("products".into(), Value::Array(products));
    root.insert("clients".into(), Value::Array(clients));
    // Never leak an older compatibility envelope's core orders to a catalog.
    root.remove("orders");
    Ok(state)
}

/// Metadata-only read: settings and export records are owned by the
/// physical envelope, not the relational order or workflow ledger tables.
/// The caller must hold the same consistent read transaction as full reads.
fn read_metadata_fields(conn:&Connection,fields:&[&str])->Result<Value,String>{
    let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],
        |row|row.get(0)).map_err(|e|e.to_string())?;
    let mut envelope=decode_physical_envelope(conn,&raw)?;
    let root=envelope.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    let mut projected=serde_json::Map::new();
    for key in fields{
        if let Some(value)=root.remove(*key){projected.insert((*key).to_owned(),value);}
    }
    Ok(Value::Object(projected))
}

/// Settings needs the current client account references but no product,
/// order, recipient, item or historical workflow rows.
pub fn read_v2_settings_snapshot(conn:&Connection)->Result<Value,String>{
    let mut state=read_metadata_fields(conn,&["revision","settings"])?;
    state["clients"]=Value::Array(read_entities(conn,CORE_CLIENTS_SQL)?);
    Ok(state)
}

/// Recorded export file/folder opening does not require order hydration.
pub fn read_v2_export_metadata(conn:&Connection)->Result<Value,String>{
    read_metadata_fields(conn,&["revision","settings","exports"])
}

#[cfg(test)]
mod metadata_snapshot_cost_tests{
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_and_export_reads_do_not_require_unrelated_relational_tables(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_schema(key TEXT,version INTEGER);
            INSERT INTO core_schema VALUES ('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE core_clients(id TEXT PRIMARY KEY,position INTEGER,payload TEXT);").unwrap();
        let root=json!({"revision":7,"settings":{"excelDirectory":"synthetic","accounts":[]},
            "exports":[{"id":"x","filename":"test.xlsx"}],
            "orders":[{"id":"legacy"}],"workflow":{"events":[{"id":"e"}]}});
        let client=json!({"id":"c","accountId":"a"});
        conn.execute("INSERT INTO app_state VALUES (1,?1)",[root.to_string()]).unwrap();
        conn.execute("INSERT INTO core_clients VALUES ('c',0,?1)",[client.to_string()]).unwrap();
        assert_eq!(read_v2_settings_snapshot(&conn).unwrap(),
            json!({"revision":7,"settings":root["settings"],"clients":[client]}));
        conn.execute("DROP TABLE core_clients",[]).unwrap();
        assert_eq!(read_v2_export_metadata(&conn).unwrap(),
            json!({"revision":7,"settings":root["settings"],"exports":root["exports"]}));
        assert!(read_v2_settings_snapshot(&conn).is_err());
    }

    #[test]
    fn conditional_update_noop_preserves_revision_and_executes_no_dml(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_schema(key TEXT,version INTEGER);
            INSERT INTO core_schema VALUES ('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE core_products(id TEXT PRIMARY KEY,position INTEGER,payload TEXT);
            CREATE TABLE core_product_prices(product_id TEXT,payload TEXT);
            CREATE TABLE core_clients(id TEXT PRIMARY KEY,position INTEGER,payload TEXT);
            CREATE TABLE core_orders(id TEXT PRIMARY KEY,position INTEGER,payload TEXT);
            CREATE TABLE core_recipients(id TEXT PRIMARY KEY,order_id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE core_items(id TEXT PRIMARY KEY,recipient_id TEXT,position INTEGER,payload TEXT);").unwrap();
        let root=json!({"revision":7,"settings":{},"workflow":{"shipments":[]}});
        conn.execute("INSERT INTO app_state VALUES (1,?1)",[root.to_string()]).unwrap();
        let before=read_v2_snapshot(&conn).unwrap();
        let count:i64=conn.query_row("SELECT total_changes()",[],|row|row.get(0)).unwrap();
        let result=update_v2_snapshot_if_changed(&mut conn,7,true,|current|{
            assert_eq!(current,&before);
            Ok(None)
        }).unwrap();
        assert_eq!(result,before);
        assert_eq!(conn.query_row("SELECT total_changes()",[],|row|row.get::<_,i64>(0)).unwrap(),count);
        assert!(update_v2_snapshot_if_changed(&mut conn,6,true,|_|{
            panic!("stale revision must not run the no-op reducer")
        }).is_err());
        assert_eq!(read_v2_snapshot(&conn).unwrap(),before);
    }
}

// Full, single-order and complementary projections share ownership checks.
fn read_order_entities(conn:&Connection,only:Option<&str>)->Result<Vec<Value>,String>{
    read_order_partition(conn,only,false)
}

fn read_order_partition(conn:&Connection,only:Option<&str>,exclude:bool)
    ->Result<Vec<Value>,String>{
    let (orders,recipients,items,owner_sql)=if exclude{
        let id=only.ok_or("제외할 주문 ID 누락")?;
        (
            read_entities_with(conn,"SELECT payload FROM core_orders WHERE id<>?1 ORDER BY position,id",[id])?,
            read_entities_with(conn,"SELECT payload FROM core_recipients WHERE order_id<>?1 ORDER BY position,id",[id])?,
            read_children_with(conn,"SELECT recipient_id,payload FROM core_items
                WHERE recipient_id NOT IN (SELECT id FROM core_recipients WHERE order_id=?1)
                ORDER BY recipient_id,position,id",[id])?,
            "SELECT order_id,id FROM core_recipients WHERE order_id<>?1 ORDER BY order_id,position,id"
        )
    }else if let Some(id)=only{
        (
            read_entities_with(conn,"SELECT payload FROM core_orders WHERE id=?1 ORDER BY position,id",[id])?,
            read_entities_with(conn,"SELECT payload FROM core_recipients WHERE order_id=?1 ORDER BY position,id",[id])?,
            read_children_with(conn,"SELECT recipient_id,payload FROM core_items
                WHERE recipient_id IN (SELECT id FROM core_recipients WHERE order_id=?1)
                ORDER BY recipient_id,position,id",[id])?,
            "SELECT order_id,id FROM core_recipients WHERE order_id=?1 ORDER BY order_id,position,id"
        )
    }else{
        (
            read_entities(conn,"SELECT payload FROM core_orders ORDER BY position,id")?,
            read_entities(conn,"SELECT payload FROM core_recipients ORDER BY position,id")?,
            read_children(conn,"SELECT recipient_id,payload FROM core_items ORDER BY recipient_id,position,id")?,
            "SELECT order_id,id FROM core_recipients ORDER BY order_id,position,id"
        )
    };
    attach_order_children(conn,orders,recipients,items,owner_sql,rusqlite::params_from_iter(only))
}

/// Multi-order partition for factory replies. A single JSON-array parameter
/// avoids SQL variable limits and uses the same child ownership checks.
fn read_factory_order_partition(conn:&Connection,ids:&[String],exclude:bool)
    ->Result<Vec<Value>,String>{
    let ids=serde_json::to_string(ids).map_err(|e|e.to_string())?;
    let op=if exclude{"NOT IN"}else{"IN"};
    let orders_sql=format!("SELECT payload FROM core_orders WHERE id {op} (SELECT value FROM json_each(?1)) ORDER BY position,id");
    let recipients_sql=format!("SELECT payload FROM core_recipients WHERE order_id {op} (SELECT value FROM json_each(?1)) ORDER BY position,id");
    let items_sql=format!("SELECT recipient_id,payload FROM core_items WHERE recipient_id IN (SELECT id FROM core_recipients WHERE order_id {op} (SELECT value FROM json_each(?1))) ORDER BY recipient_id,position,id");
    let owner_sql=format!("SELECT order_id,id FROM core_recipients WHERE order_id {op} (SELECT value FROM json_each(?1)) ORDER BY order_id,position,id");
    let orders=read_entities_with(conn,&orders_sql,[&ids])?;
    let recipients=read_entities_with(conn,&recipients_sql,[&ids])?;
    let items=read_children_with(conn,&items_sql,[&ids])?;
    attach_order_children(conn,orders,recipients,items,&owner_sql,[&ids])
}
fn attach_order_children(conn:&Connection,mut orders:Vec<Value>,mut recipients:Vec<Value>,
    items:HashMap<String,Vec<Value>>,owner_sql:&str,params:impl rusqlite::Params)
    ->Result<Vec<Value>,String>{
    attach(&mut recipients, "items", items)?;
    let mut stmt=conn.prepare(owner_sql).map_err(|_|"수취인 소속 조회 준비 실패".to_owned())?;
    let owners=stmt.query_map(params,|row|{
        Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))
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
    Ok(orders)
}

/// Hydrate a schema-v2 snapshot within a consistent transaction.
pub fn hydrate_v2(conn: &Connection, state: &mut Value) -> Result<(), String> {
    let (products,clients)=read_catalog_entities(conn)?;
    let orders=read_order_entities(conn,None)?;
    let root = state.as_object_mut()
        .ok_or("업무 상태는 객체여야 합니다.")?;
    root.insert("products".into(), Value::Array(products));
    root.insert("clients".into(), Value::Array(clients));
    root.insert("orders".into(), Value::Array(orders));
    Ok(())
}
