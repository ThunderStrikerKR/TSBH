//! Full legacy-v0/v1 relational recovery path extracted from Tauri.
//! Restores verify immutable audit rows and reinitialize working receptions.
use rusqlite::{params,Connection,OptionalExtension,Transaction};
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};
use crate::valid_state;
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}
const CORE_KEYS: [&str;3]=["products","clients","orders"];

#[path = "../../../src-tauri/src/sale_unit_migration.rs"]
mod sale_unit_migration;
mod product_name_enrichment;

pub fn get_snapshot(conn:&Connection)->Result<Value,String>{
    if active(conn)? {return crate::read_v2_snapshot(conn);}
    let raw:String=conn.query_row(
        "SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0),
    ).map_err(err)?;
    let mut state:Value=serde_json::from_str(&raw).map_err(err)?;
    hydrate(conn,&mut state)?;
    Ok(state)
}
/// Identical tables to Tauri working_receptions::initialize.
pub fn initialize_working(conn:&Connection)->Result<(),String>{
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

fn sql_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS core_schema(
            key TEXT PRIMARY KEY, version INTEGER NOT NULL CHECK(version > 0)
        );
        CREATE TABLE IF NOT EXISTS core_products(
            id TEXT PRIMARY KEY, position INTEGER NOT NULL,
            code TEXT NOT NULL, name TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_products_name ON core_products(name);
        CREATE INDEX IF NOT EXISTS idx_core_products_code ON core_products(code);
        CREATE TABLE IF NOT EXISTS core_product_prices(
            product_id TEXT PRIMARY KEY REFERENCES core_products(id) ON DELETE CASCADE,
            price_record_id TEXT NOT NULL UNIQUE,
            color TEXT NOT NULL,
            wholesale INTEGER CHECK(wholesale IS NULL OR wholesale >= 0),
            semi INTEGER CHECK(semi IS NULL OR semi >= 0),
            retail INTEGER CHECK(retail IS NULL OR retail >= 0),
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE TABLE IF NOT EXISTS core_clients(
            id TEXT PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
            tier TEXT NOT NULL, address TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_clients_name ON core_clients(name);
        CREATE TABLE IF NOT EXISTS core_orders(
            id TEXT PRIMARY KEY, position INTEGER NOT NULL, client_id TEXT,
            created_at TEXT, confirmed_at TEXT, status TEXT,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_orders_created ON core_orders(created_at);
        CREATE INDEX IF NOT EXISTS idx_core_orders_client ON core_orders(client_id);
        CREATE TABLE IF NOT EXISTS core_recipients(
            id TEXT PRIMARY KEY, order_id TEXT NOT NULL REFERENCES core_orders(id) ON DELETE CASCADE,
            position INTEGER NOT NULL, name TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_recipients_order ON core_recipients(order_id,position);
        CREATE TABLE IF NOT EXISTS core_items(
            id TEXT PRIMARY KEY, recipient_id TEXT NOT NULL REFERENCES core_recipients(id) ON DELETE CASCADE,
            position INTEGER NOT NULL, product_id TEXT, variant_id TEXT,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_items_recipient ON core_items(recipient_id,position);
        CREATE INDEX IF NOT EXISTS idx_core_items_product ON core_items(product_id);
    "#).map_err(err)
}

/// Tests of independent v2 migrations must use the actual schema without
/// re-running historical product transformations, which require a complete
/// production product seed unrelated to client-name migration.
#[cfg(test)]
pub(crate) fn create_v2_schema_for_tests(conn:&Connection)->Result<(),String>{
    sql_schema(conn)
}

fn schema_version(conn:&Connection)->Result<i64,String>{
    let exists:bool=conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='core_schema')",
        [],|r|r.get(0)).map_err(err)?;
    if !exists {return Ok(0);}
    Ok(conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],
        |r|r.get::<_,i64>(0)).optional().map_err(err)?.unwrap_or(0))
}
pub(super) fn active(conn:&Connection)->Result<bool,String>{Ok(schema_version(conn)?==2)}


pub(super) fn strip_core(state: &mut Value) {
    if let Some(map) = state.as_object_mut() {
        for key in CORE_KEYS { map.remove(key); }
    }
}

fn source_list<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value.get(key).and_then(Value::as_array)
        .ok_or_else(|| format!("{key} 목록 데이터가 없습니다."))
}

fn required_id<'a>(entry: &'a Value, label: &str) -> Result<&'a str, String> {
    entry.get("id").and_then(Value::as_str).filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{label} ID가 비어 있습니다."))
}

// The v1 migration still serializes legacy product records in this module.
// The v2 writer owns its own equivalent helper in the shared storage crate.
fn payload(value: &Value, child: Option<&str>) -> Result<String, String> {
    let mut stripped = value.clone();
    if let Some(child) = child {
        stripped.as_object_mut()
            .ok_or("업무 데이터가 객체가 아닙니다.")?.remove(child);
    }
    serde_json::to_string(&stripped).map_err(err)
}

fn read_entities(conn:&Connection,sql:&str)->Result<Vec<Value>,String>{
    let mut statement=conn.prepare(sql).map_err(err)?;
    let rows=statement.query_map([],|r|r.get::<_,String>(0)).map_err(err)?;
    rows.map(|r|serde_json::from_str::<Value>(&r.map_err(err)?).map_err(err)).collect()
}
fn read_children(conn:&Connection,sql:&str)->Result<HashMap<String,Vec<Value>>,String>{
    let mut statement=conn.prepare(sql).map_err(err)?;
    let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?;
    let mut grouped=HashMap::new();
    for row in rows {
        let (owner,json)=row.map_err(err)?;
        grouped.entry(owner).or_insert_with(Vec::new)
            .push(serde_json::from_str::<Value>(&json).map_err(err)?);
    }
    Ok(grouped)
}
fn attach(parents:&mut [Value],key:&str,mut children:HashMap<String,Vec<Value>>)->Result<(),String>{
    for parent in parents {
        let id=required_id(parent,"관계형 부모")?.to_owned();
        let nested = children.remove(&id).unwrap_or_default();
        parent.as_object_mut().ok_or("관계형 데이터 객체 오류")?
            .insert(key.to_string(),json!(nested));
    }
    if !children.is_empty() { return Err(format!("관계형 데이터에서 소유자가 없는 {key}을 발견했습니다.")); }
    Ok(())
}

pub fn hydrate(conn:&Connection,state:&mut Value)->Result<(),String>{
    let version=schema_version(conn)?;
    if version==0 { return Ok(()); }
    // Schema v2 hydration is shared by the Tauri and egui hosts.
    // Legacy v1 migration remains on the original implementation below.
    if version==2 { return crate::hydrate_v2(conn,state); }
    let mut products=read_entities(conn,"SELECT payload FROM core_products ORDER BY position,id")?;
    let variants=read_children(conn,if version==1 {
        "SELECT product_id,payload FROM core_variants ORDER BY product_id,position,id"
    }else{
        "SELECT product_id,payload FROM core_product_prices ORDER BY product_id"
    })?;
    attach(&mut products,"variants",variants)?;
    let clients=read_entities(conn,"SELECT payload FROM core_clients ORDER BY position,id")?;
    let mut orders=read_entities(conn,"SELECT payload FROM core_orders ORDER BY position,id")?;
    let mut recipients=read_entities(conn,"SELECT payload FROM core_recipients ORDER BY position,id")?;
    let items=read_children(conn,
        "SELECT recipient_id,payload FROM core_items ORDER BY recipient_id,position,id")?;
    attach(&mut recipients,"items",items)?;
    let mut recipient_groups:HashMap<String,Vec<Value>>=HashMap::new();
    let mut read=conn.prepare("SELECT order_id,id FROM core_recipients ORDER BY order_id,position,id").map_err(err)?;
    let ids=read.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(err)?
        .collect::<Result<Vec<_>,_>>().map_err(err)?;
    drop(read);
    let mut by_id:HashMap<String,Value>=recipients.into_iter()
        .map(|r|Ok((required_id(&r,"수취인")?.to_owned(),r)))
        .collect::<Result<_,String>>()?;
    for (owner,id) in ids {
        let value=by_id.remove(&id).ok_or("수취인 관계형 데이터가 누락되었습니다.")?;
        recipient_groups.entry(owner).or_default().push(value);
    }
    if !by_id.is_empty() { return Err("관계형 수취인 ID가 중복되었거나 누락되었습니다.".into()); }
    attach(&mut orders,"recipients",recipient_groups)?;
    let root=state.as_object_mut().ok_or("업무 상태가 객체가 아닙니다.")?;
    root.insert("products".into(),json!(products));
    root.insert("clients".into(),json!(clients));
    root.insert("orders".into(),json!(orders));
    Ok(())
}

/// Recreate missing immutable audit rows from already-confirmed legacy order
/// snapshots when older SQLite backups only contain receipt/correction JSON.
/// Do not infer absent receipts and never overwrite conflicting audit evidence.
fn restore_legacy_receipt_archive(tx:&Transaction<'_>,state:&Value)->Result<(),String>{
    let mut seen=HashSet::new();
    for order in source_list(state,"orders")?{
        let Some(confirmed)=order["confirmedAt"].as_str() else {continue};
        if confirmed.is_empty(){continue;}
        let order_id=required_id(order,"확정 주문")?;
        let mut records:Vec<(String,&Value)>=Vec::new();
        if let Some(receipt)=order.get("receipt").filter(|v|!v.is_null()){
            records.push((format!("receipt:{order_id}"),receipt));
        }
        for correction in order["rawCorrections"].as_array().into_iter().flatten(){
            let correction_id=required_id(correction,"접수 정정")?;
            records.push((format!("correction:{correction_id}"),correction));
        }
        for (id,record) in records{
            if !seen.insert(id.clone()){
                return Err(format!("접수 감사 ID {id}가 중복됐습니다. 이전을 중단합니다."));
            }
            let existing:Option<(String,String)>=tx.query_row(
                "SELECT order_id,record FROM receipt_archive WHERE id=?1",
                [id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))
            ).optional().map_err(err)?;
            if let Some((existing_owner,existing_raw))=existing{
                let old:Value=serde_json::from_str(&existing_raw).map_err(err)?;
                if existing_owner!=order_id || old!=*record{
                    return Err(format!("접수 감사 기록 {id}가 기존 데이터와 충돌해 이전을 중단했습니다."));
                }
            }else{
                tx.execute("INSERT INTO receipt_archive(id,order_id,record)
                    VALUES (?1,?2,?3)",
                    params![id,order_id,serde_json::to_string(record).map_err(err)?]
                ).map_err(err)?;
            }
        }
    }
    Ok(())
}

pub fn migrate(conn:&mut Connection)->Result<(),String>{
    sql_schema(conn)?;
    let prior=schema_version(conn)?;
    if prior==2{return Ok(());}
    if prior>2 {return Err("이 프로그램보다 새로운 DB 구조입니다. 자동으로 변경하지 않았습니다.".into());}
    // Hydrate legacy v1 rows or v0.0.1 full JSON BEFORE converting.
    // A verified SQLite pre-migration backup is created by initialize_database.
    let old=get_snapshot(conn)?;
    valid_state(&old)?;
    let state=sale_unit_migration::transform(old)?;
    valid_state(&state)?;
    let tx=conn.transaction().map_err(err)?;
    crate::store_v2_relational(&tx,&state)?;
    restore_legacy_receipt_archive(&tx,&state)?;
    // Legacy search indexes are non-authoritative copies and rebuilt after migration.
    tx.execute("DELETE FROM product_index",[]).map_err(err)?;
    {
        let mut insert=tx.prepare(
            "INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)"
        ).map_err(err)?;
        for p in source_list(&state,"products")?{
            insert.execute(params![required_id(p,"상품")?,
                p["code"].as_str().ok_or("상품 코드 누락")?,
                p["name"].as_str().ok_or("상품명 누락")?,payload(p,None)?]
            ).map_err(err)?;
        }
    }
    tx.execute("DELETE FROM order_index",[]).map_err(err)?;
    {
        let mut insert=tx.prepare(
            "INSERT INTO order_index(id,client_id,created_at,confirmed_at,status)
             VALUES (?1,?2,?3,?4,?5)"
        ).map_err(err)?;
        for o in source_list(&state,"orders")?{
            insert.execute(params![required_id(o,"주문")?,
                o["clientId"].as_str().unwrap_or(""),
                o["createdAt"].as_str().unwrap_or(""),
                o["confirmedAt"].as_str(),
                o["status"].as_str().unwrap_or("REVIEW")]
            ).map_err(err)?;
        }
    }
    let mut envelope=state;
    strip_core(&mut envelope);
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
        [serde_json::to_string(&envelope).map_err(err)?]).map_err(err)?;
    if prior==1 {
        // An old physical option table must not remain part of the new schema.
        tx.execute("DROP TABLE core_variants",[]).map_err(err)?;
        tx.execute("UPDATE core_schema SET version=2 WHERE key='business-core'",[]).map_err(err)?;
    }else{
        tx.execute("INSERT INTO core_schema(key,version) VALUES ('business-core',2)",[]).map_err(err)?;
    }
    tx.commit().map_err(err)
}

pub fn verify(conn:&Connection)->Result<(),String>{
    if !active(conn)? { return Ok(()); }
    // A price record points to an existing product via FK, but that alone
    // does not ensure EVERY product still has its single required price row.
    let missing_price:i64=conn.query_row(
        "SELECT COUNT(*) FROM core_products p LEFT JOIN core_product_prices pr
         ON pr.product_id=p.id WHERE pr.product_id IS NULL",
        [],|r|r.get(0)).map_err(err)?;
    if missing_price>0 {
        return Err(format!("독립 상품 중 가격 기록이 누락된 항목이 {missing_price}개입니다."));
    }
    let mismatched_id:i64=conn.query_row(
        "SELECT COUNT(*) FROM core_product_prices
         WHERE json_extract(payload,'$.id') IS NULL OR
               json_extract(payload,'$.id')<>price_record_id",
        [],|r|r.get(0)).map_err(err)?;
    if mismatched_id>0 {
        return Err(format!("상품 가격 기록의 원본 식별자가 다른 항목이 {mismatched_id}개입니다."));
    }
    let broken:Option<String>=conn.query_row(
        "SELECT \"table\" || ':' || rowid FROM pragma_foreign_key_check LIMIT 1",
        [],|r|r.get(0)).optional().map_err(err)?;
    if let Some(details)=broken { return Err(format!("SQLite 관계형 참조 무결성 오류: {details}")); }
    let mut snapshot:Value=serde_json::from_str(&conn.query_row(
        "SELECT payload FROM app_state WHERE id=1",[],|r|r.get::<_,String>(0)
    ).map_err(err)?).map_err(err)?;
    hydrate(conn,&mut snapshot)?;
    valid_state(&snapshot)
}

