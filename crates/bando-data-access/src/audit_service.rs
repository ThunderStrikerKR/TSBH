//! Read-only immutable receipt archive projection, identical to Tauri.
use rusqlite::Connection;
use serde_json::{json,Value};
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}

pub fn receipt_archive_from_conn(conn:&Connection)->Result<Value,String>{
    let mut statement = conn.prepare("SELECT id,order_id,record FROM receipt_archive ORDER BY rowid DESC LIMIT 10000").map_err(err)?;
    let mut rows = statement.query([]).map_err(err)?;
    let mut archive = Vec::new();
    while let Some(row) = rows.next().map_err(err)? {
        let id: String = row.get(0).map_err(err)?;
        let order_id: String = row.get(1).map_err(err)?;
        let record: String = row.get(2).map_err(err)?;
        archive.push(json!({"id":id,"orderId":order_id,"record":serde_json::from_str::<Value>(&record).map_err(err)?}));
    }
    // Historical orders may predate the independent audit table. Display their
    // current snapshot as unverified reference data; never fabricate an audit row.
    let known: std::collections::HashSet<String> = archive.iter()
        .filter_map(|row| row["id"].as_str().map(str::to_owned)).collect();
    let snapshot = crate::recovery::get_snapshot(&conn)?;
    for order in snapshot["orders"].as_array().into_iter().flatten() {
        if order["confirmedAt"].as_str().is_none() { continue; }
        let Some(order_id) = order["id"].as_str() else { continue; };
        if !known.contains(&format!("receipt:{order_id}")) {
            let original = order["receipt"]["original"].as_str()
                .or_else(|| order["rawHistory"].get(0).and_then(|r| r["raw"].as_str()))
                .or_else(|| order["raw"].as_str()).unwrap_or("");
            archive.push(json!({"id":format!("legacy:{order_id}"),"orderId":order_id,
                "record":{"original":original,
                    "at":order["receipt"]["at"].as_str().or_else(||order["createdAt"].as_str()).unwrap_or(""),
                    "actor":"과거 주문 / 최초 접수 검증 불가",
                    "legacy":true,"verified":false}}));
        }
        for correction in order["rawCorrections"].as_array().into_iter().flatten() {
            let Some(cid) = correction["id"].as_str() else { continue; };
            if known.contains(&format!("correction:{cid}")) { continue; }
            let mut reference = correction.clone();
            if let Some(fields) = reference.as_object_mut() {
                fields.insert("legacy".into(), json!(true));
                fields.insert("verified".into(), json!(false));
            }
            archive.push(json!({"id":format!("legacy-correction:{cid}"),
                "orderId":order_id,"record":reference}));
        }
    }
    Ok(json!(archive))
}

