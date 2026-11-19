//! Consistent read-only reconstruction of the Tauri v2 business snapshot.
//! This adapter is not an order writer. It cannot change app_state, relational
//! rows, receipts or the revision. No automatic production database discovery.
//! The real mutation rules remain in business_core/rules/order_edit.rs.

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf};

fn read_rows(conn:&Connection, sql:&str)->Result<Vec<Value>,String> {
    let mut stmt=conn.prepare(sql).map_err(|_|"운영 업무자료 조회를 준비하지 못했습니다.".to_owned())?;
    let rows=stmt.query_map([],|r|r.get::<_,String>(0))
        .map_err(|_|"운영 업무자료 조회에 실패했습니다.".to_owned())?;
    rows.map(|r|{
        let s=r.map_err(|_|"운영 업무자료 행을 읽지 못했습니다.".to_owned())?;
        serde_json::from_str(&s).map_err(|_|"운영 업무자료 JSON 형식이 잘못되었습니다.".to_owned())
    }).collect()
}
fn read_children(conn:&Connection,sql:&str)->Result<HashMap<String,Vec<Value>>,String> {
    let mut stmt=conn.prepare(sql).map_err(|_|"운영 하위자료 조회를 준비하지 못했습니다.".to_owned())?;
    let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))
        .map_err(|_|"운영 하위자료 조회에 실패했습니다.".to_owned())?;
    let mut grouped:HashMap<String,Vec<Value>>=HashMap::new();
    for row in rows {
        let (key,raw)=row.map_err(|_|"운영 하위자료 행을 읽지 못했습니다.".to_owned())?;
        let value=serde_json::from_str(&raw)
            .map_err(|_|"운영 하위자료 JSON 형식이 잘못되었습니다.".to_owned())?;
        grouped.entry(key).or_default().push(value);
    }
    Ok(grouped)
}
fn attach(parents:&mut [Value],key:&str,mut children:HashMap<String,Vec<Value>>) -> Result<(),String> {
    for parent in parents {
        let id=parent.get("id").and_then(Value::as_str)
            .filter(|s|!s.is_empty()).ok_or("운영 자료의 ID가 누락되었습니다.")?.to_owned();
        let nested=children.remove(&id).unwrap_or_default();
        parent.as_object_mut().ok_or("운영 자료 객체 형식이 잘못되었습니다.")?
            .insert(key.to_owned(),json!(nested));
    }
    if !children.is_empty() {
        return Err("운영 업무자료에 소속되지 않은 하위 기록이 있습니다.".into());
    }
    Ok(())
}

/// The explicit database path is a prerequisite: the seed catalog has no
/// authentic client tiers, per-dealer price overrides or shipment history.
pub fn load() -> Result<Value,String> {
    let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
        .filter(|p|!p.is_empty())
        .map(PathBuf::from)
        .ok_or("실제 Rust 단가·운임 검증은 BANDO_EGUI_BUSINESS_DB 읽기 전용 연결이 필요합니다.")?;
    if std::env::var_os("BANDO_EGUI_CLIENT_INDEX").is_some_and(|p|!p.is_empty()) {
        return Err("JSON 거래처 인덱스는 실제 주문 가격 변경의 근거가 될 수 없습니다. 운영 DB 읽기 전용 연결을 사용하세요.".into());
    }
    if !path.is_file() {return Err("연결한 운영 DB 파일을 찾지 못했습니다.".into());}
    let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_|"운영 DB를 읽기 전용으로 열지 못했습니다.".to_owned())?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=2000; BEGIN DEFERRED TRANSACTION;")
        .map_err(|_|"운영 DB 읽기 전용 트랜잭션을 열지 못했습니다.".to_owned())?;
    // Keep all table reads inside ONE consistent snapshot. Concurrent
    // production writes cannot mix old orders with new prices.
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)
    ).map_err(|_|"운영 DB 스키마 버전을 확인하지 못했습니다.".to_owned())?;
    if version!=2 {return Err("지원되지 않는 운영 DB 스키마입니다. 아무 것도 저장하지 않았습니다.".into());}
    let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0))
        .map_err(|_|"운영 업무 상태를 읽지 못했습니다.".to_owned())?;
    let mut state:Value=serde_json::from_str(&raw)
        .map_err(|_|"운영 업무 상태의 JSON 형식이 잘못되었습니다.".to_owned())?;
    let mut products=read_rows(&conn,"SELECT payload FROM core_products ORDER BY position,id")?;
    let prices=read_children(&conn,"SELECT product_id,payload FROM core_product_prices ORDER BY product_id")?;
    attach(&mut products,"variants",prices)?;
    let clients=read_rows(&conn,"SELECT payload FROM core_clients ORDER BY position,id")?;
    let mut orders=read_rows(&conn,"SELECT payload FROM core_orders ORDER BY position,id")?;
    let mut recipients=read_rows(&conn,"SELECT payload FROM core_recipients ORDER BY position,id")?;
    let items=read_children(&conn,"SELECT recipient_id,payload FROM core_items ORDER BY recipient_id,position,id")?;
    attach(&mut recipients,"items",items)?;
    let owners={
        let mut stmt=conn.prepare("SELECT order_id,id FROM core_recipients ORDER BY order_id,position,id")
            .map_err(|_|"수취인 소속 조회 준비 실패".to_owned())?;
        let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))
            .map_err(|_|"수취인 소속 조회 실패".to_owned())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_|"수취인 소속 결과 오류".to_owned())?
    };
    let mut by_id:HashMap<String,Value>=HashMap::new();
    for row in recipients {
        let id=row.get("id").and_then(Value::as_str)
            .ok_or("수취인 ID 누락")?.to_owned();
        if by_id.insert(id,row).is_some(){return Err("중복 수취인 ID가 있습니다.".into());}
    }
    let mut grouped:HashMap<String,Vec<Value>>=HashMap::new();
    for (order_id,recipient_id) in owners {
        let recipient=by_id.remove(&recipient_id).ok_or("수취인 소속 기록 불일치")?;
        grouped.entry(order_id).or_default().push(recipient);
    }
    if !by_id.is_empty(){return Err("연결되지 않은 수취인 기록이 있습니다.".into());}
    attach(&mut orders,"recipients",grouped)?;
    let root=state.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    root.insert("products".into(),json!(products));
    root.insert("clients".into(),json!(clients));
    root.insert("orders".into(),json!(orders));
    let revision=root.get("revision").and_then(Value::as_u64)
        .ok_or("운영 DB revision을 확인할 수 없습니다.")?;
    if revision==u64::MAX{return Err("운영 DB revision 범위를 확인할 수 없습니다.".into());}
    conn.execute_batch("COMMIT")
        .map_err(|_|"운영 DB 읽기 트랜잭션을 완료하지 못했습니다.".to_owned())?;
    Ok(state)
}
