//! Explicit, read-only customer name index for the isolated egui preview.
//! Reads production Tauri client master ONLY with an explicit read-only DB
//! path. Never modifies Tauri data or saves contact/financial/order records
//! into preview storage.
//!
//! Opt in with BANDO_EGUI_CLIENT_INDEX (exported JSON) OR
//! BANDO_EGUI_BUSINESS_DB (read-only SQLite). No implicit path discovery.
//! Missing configuration means no client match.

use serde_json::{json, Value};
use std::{collections::HashSet, fs, path::PathBuf};

const MAX_CLIENT_INDEX_BYTES: u64 = bando_data_access::CLIENT_INDEX_MAX_BYTES as u64;
const MAX_CLIENTS: usize = bando_data_access::CLIENT_INDEX_MAX_ROWS;

pub fn attach(mut catalog: Value) -> Result<Value, String> {
    let index = std::env::var_os("BANDO_EGUI_CLIENT_INDEX").filter(|v| !v.is_empty());
    let business = std::env::var_os("BANDO_EGUI_BUSINESS_DB").filter(|v| !v.is_empty());
    let (clients, settings) = match (index, business) {
        (Some(_), Some(_)) =>
            return Err("거래처 인덱스와 읽기 전용 운영 DB 경로를 동시에 지정할 수 없습니다.".into()),
        (Some(file), None) => load_index(&PathBuf::from(file))?,
        (None, Some(file)) => load_business_db(&PathBuf::from(file))?,
        (None, None) => (json!([]), json!({"kakaoTitleMappings": {}})),
    };
    catalog["clients"] = clients;
    catalog["settings"] = settings;
    Ok(catalog)
}

fn load_index(file: &std::path::Path) -> Result<(Value, Value), String> {
    let len = fs::metadata(file)
        .map_err(|_| "거래처 인덱스 파일을 확인할 수 없습니다.".to_owned())?.len();
    if len > MAX_CLIENT_INDEX_BYTES {
        return Err("거래처 인덱스 파일은 최대 2MB까지 사용할 수 있습니다.".into());
    }
    let source = fs::read_to_string(file)
        .map_err(|_| "거래처 인덱스 파일을 읽지 못했습니다.".to_owned())?;
    let value: Value = serde_json::from_str(&source)
        .map_err(|_| "거래처 인덱스 JSON 형식이 잘못되었습니다.".to_owned())?;
    project_index(&value)
}

/// Explicit opt-in read of the Tauri relational client master. Does not run
/// migrations, execute write SQL, copy the DB, read order rows, or save
/// customer details to egui SQLite. This is a read-only live lookup, not an
/// ownership transfer. WAL mode is not altered.
fn load_business_db(file: &std::path::Path) -> Result<(Value, Value), String> {
    use rusqlite::{Connection, OpenFlags};
    if !file.is_file() {
        return Err("지정한 운영 DB 파일을 찾을 수 없습니다.".into());
    }
    let conn = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "운영 DB 읽기 전용 열기에 실패했습니다.".to_owned())?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=2000;")
        .map_err(|_| "운영 DB 읽기 전용 설정에 실패했습니다.".to_owned())?;
    load_business_from_connection(&conn)
}

fn load_business_from_connection(conn: &rusqlite::Connection) -> Result<(Value, Value), String> {
    let version: i64 = conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [], |row| row.get(0),
    ).map_err(|_| "현재 관계형 거래처 DB 버전을 확인할 수 없습니다.".to_owned())?;
    if version != 2 {
        return Err("지원하지 않는 거래처 DB 버전입니다. 운영 DB를 변경하지 않았습니다.".into());
    }
    let clients = bando_data_access::read_client_index_rows(conn)?;
    // Only read settings; app_state may also contain other legacy data, which
    // is never selected, projected, or saved here.
    let setting: Option<String> = conn.query_row(
        "SELECT json_extract(payload,'$.settings.kakaoTitleMappings')
         FROM app_state WHERE id=1",
        [], |row| row.get(0),
    ).map_err(|_| "기존 카카오톡 거래처 연결 정보를 읽을 수 없습니다.".to_owned())?;
    let mappings: Value = match setting {
        Some(text) => serde_json::from_str(&text)
            .map_err(|_| "기존 카카오톡 거래처 매핑이 잘못되었습니다.".to_owned())?,
        None => json!({}),
    };
    project_index(&json!({
        "clients": clients,
        "settings": {"kakaoTitleMappings": mappings},
    }))
}

/// Only the native matching and local chooser fields are projected. No
/// order history, overrides, payment terms or account details are copied.
pub(crate) fn project_index(value: &Value) -> Result<(Value, Value), String> {
    let clients = value.get("clients").and_then(Value::as_array)
        .ok_or("거래처 인덱스에는 clients 배열이 필요합니다.")?;
    if clients.len() > MAX_CLIENTS {
        return Err("거래처 인덱스는 최대 5,000개 거래처까지 지원합니다.".into());
    }
    let mut seen = HashSet::new();
    let mut compact = Vec::with_capacity(clients.len());
    for client in clients {
        let id = client.get("id").and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("거래처에 유효한 id가 없습니다.")?;
        if !seen.insert(id) {
            return Err("중복된 거래처 id가 있습니다.".into());
        }
        let name = client.get("name").and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("거래처에 유효한 name이 없습니다.")?;
        let text = |key| client.get(key).and_then(Value::as_str).unwrap_or("");
        // Read-only imported indexes may still have the legacy key.
        // Convert it in memory so it remains searchable without copying
        // the override into the active client model or changing the DB.
        let canonical=bando_data_access::client_names::normalized(client);
        let search_text=|key| canonical.get(key).and_then(Value::as_str)
            .unwrap_or("").to_owned();
        compact.push(json!({
            "id": id,
            "name": name,
            "shortNames": search_text("shortNames"),
            "identifiers": search_text("identifiers"),
            // Local read-only search/display fields; never persisted to the
            // isolated egui receipt or the original Tauri database.
            "address": text("address"),
            "phone": text("phone"),
            "mobile": text("mobile"),
            "active": client.get("active").and_then(Value::as_bool).unwrap_or(true),
            "deletedAt": client.get("deletedAt").cloned().unwrap_or(Value::Null),
        }));
    }
    let mappings = value.pointer("/settings/kakaoTitleMappings")
        .cloned().unwrap_or_else(|| json!({}));
    let Some(entries) = mappings.as_object() else {
        return Err("카카오톡 거래처 연결 정보는 객체여야 합니다.".into());
    };
    let mut safe_mappings = serde_json::Map::new();
    for (title, entry) in entries {
        let id = entry.get("clientId").and_then(Value::as_str).unwrap_or("");
        if seen.contains(id) {
            safe_mappings.insert(title.clone(), json!({
                "clientId": id,
                "ambiguous": entry.get("ambiguous").and_then(Value::as_bool).unwrap_or(false),
            }));
        }
    }
    Ok((json!(compact), json!({"kakaoTitleMappings": safe_mappings})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_excludes_contact_details_and_orders() {
        let data = json!({
            "clients": [{"id":"c","name":"경기악기사",
                "address":"private","phone":"private","tier":"special",
                "shortNames":"경기악기"}],
            "orders":[{"id":"sensitive"}],
            "settings":{"kakaoTitleMappings":{
                "경기 악기":{"clientId":"c","ambiguous":false},
                "unknown":{"clientId":"unavailable"}
            }}
        });
        let (clients, settings) = project_index(&data).unwrap();
        assert_eq!(clients[0]["shortNames"], "경기악기");
        assert_eq!(clients[0]["address"], "private");
        assert_eq!(clients[0]["phone"], "private");
        assert!(clients[0].get("tier").is_none());
        assert!(settings["kakaoTitleMappings"].get("unknown").is_none());
    }
    #[test]
    fn live_relational_database_is_read_only_and_exposes_only_names() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(r#"CREATE TABLE core_schema(key TEXT,version INTEGER);
          CREATE TABLE core_clients(id TEXT,position INTEGER,payload TEXT);
          CREATE TABLE app_state(id INTEGER,payload TEXT);
          INSERT INTO core_schema VALUES('business-core',2);
          INSERT INTO core_clients VALUES('c1',0,
            '{"id":"c1","name":"악기사","active":true,"phone":"hidden","tier":"special"}');
          INSERT INTO app_state VALUES(1,
            '{"settings":{"kakaoTitleMappings":{"악기사":{"clientId":"c1","ambiguous":false}}}}');"#)
            .unwrap();
        let (clients, settings) = load_business_from_connection(&conn).unwrap();
        assert_eq!(clients[0]["name"], "악기사");
        assert!(clients[0].get("phone").is_none());
        assert!(clients[0].get("tier").is_none());
        assert_eq!(settings["kakaoTitleMappings"]["악기사"]["clientId"], "c1");
        // This adapter creates no user-data tables or modifications.
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM core_clients",
            [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
    }
    #[test]
    fn duplicate_client_ids_fail_closed() {
        let data = json!({"clients":[{"id":"c","name":"A"},{"id":"c","name":"B"}]});
        assert!(project_index(&data).is_err());
    }
}
