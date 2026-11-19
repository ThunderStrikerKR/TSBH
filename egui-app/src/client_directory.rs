//! Explicit, read-only customer name index for the isolated egui preview.
//! NEVER opens the production Tauri database or copies customer addresses,
//! phone numbers, financial terms or confirmed orders into preview storage.
//!
//! Opt in by setting BANDO_EGUI_CLIENT_INDEX to a JSON file whose root has
//! "clients" and (optionally) "settings.kakaoTitleMappings". The path is never
//! discovered from Tauri settings. Missing configuration means no client match.

use serde_json::{json, Value};
use std::{collections::HashSet, fs, path::PathBuf};

const MAX_CLIENT_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_CLIENTS: usize = 5_000;

pub fn attach(mut catalog: Value) -> Result<Value, String> {
    let Some(path) = std::env::var_os("BANDO_EGUI_CLIENT_INDEX") else {
        catalog["clients"] = json!([]);
        catalog["settings"] = json!({"kakaoTitleMappings": {}});
        return Ok(catalog);
    };
    if path.is_empty() {
        return Err("BANDO_EGUI_CLIENT_INDEX 파일 경로가 비어 있습니다.".into());
    }
    let file = PathBuf::from(path);
    let len = fs::metadata(&file)
        .map_err(|e| format!("거래처 인덱스 파일에 접근할 수 없습니다: {e}"))?.len();
    if len > MAX_CLIENT_INDEX_BYTES {
        return Err("거래처 인덱스 파일은 최대 2MB까지 사용할 수 있습니다.".into());
    }
    let source = fs::read_to_string(&file)
        .map_err(|e| format!("거래처 인덱스 파일을 읽지 못했습니다: {e}"))?;
    let value: Value = serde_json::from_str(&source)
        .map_err(|e| format!("거래처 인덱스 JSON 형식 오류: {e}"))?;
    let (clients, settings) = project_index(&value)?;
    catalog["clients"] = clients;
    catalog["settings"] = settings;
    Ok(catalog)
}

/// Only information required by the existing native matcher is allowed out.
fn project_index(value: &Value) -> Result<(Value, Value), String> {
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
        compact.push(json!({
            "id": id,
            "name": name,
            "displayName": text("displayName"),
            "shortNames": text("shortNames"),
            "identifiers": text("identifiers"),
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
        assert!(clients[0].get("address").is_none());
        assert!(clients[0].get("phone").is_none());
        assert!(clients[0].get("tier").is_none());
        assert!(settings["kakaoTitleMappings"].get("unknown").is_none());
    }
    #[test]
    fn duplicate_client_ids_fail_closed() {
        let data = json!({"clients":[{"id":"c","name":"A"},{"id":"c","name":"B"}]});
        assert!(project_index(&data).is_err());
    }
}
