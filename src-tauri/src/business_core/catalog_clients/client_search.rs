//! Single Rust owner for full client searching in both Tauri and egui.
//! Keep the established query semantics from queries.rs::client_search:
//! ignore whitespace/case; look at name, displayName, phone, mobile,
//! address, shortNames, identifiers; never return deleted clients.
//! This module does not change any client or order.

use serde_json::{json, Value};

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn compact(input: &str) -> String {
    input.chars().filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase).collect()
}

pub fn search(state: &Value, payload: &Value) -> Value {
    let clients = state.get("clients").and_then(Value::as_array)
        .map(Vec::as_slice).unwrap_or(&[]);
    let query = compact(field(payload, "query"));
    let raw = field(payload, "raw");
    let available = clients.iter().filter(|client|
        field(client, "deletedAt").is_empty()
    );
    let ids: Vec<String> = available.clone().filter(|client| {
        !query.is_empty() &&
        ["name", "displayName", "phone", "mobile", "address",
         "shortNames", "identifiers"].iter()
            .any(|key| compact(field(client, key)).contains(&query))
    }).map(|client| field(client, "id").to_owned()).collect();
    let suggested: Vec<String> = available.filter(|client|
        ["name", "displayName"].iter().any(|key| {
            let term = field(client, key).trim();
            term.encode_utf16().count() >= 2 && raw.contains(term)
        })
    ).map(|client| field(client, "id").to_owned()).collect();
    json!({"ids": ids, "suggestedIds": suggested})
}

pub fn execute(payload: &Value) -> Result<Value, String> {
    let state = payload.get("state").ok_or("거래처 검색용 상태가 없습니다.")?;
    Ok(search(state, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_search_fields_and_deleted_filter() {
        let state = json!({"clients":[
            {"id":"a","name":"경기악기사","shortNames":"경기악기","address":"의정부시",
             "phone":"031-123-4567","active":true},
            {"id":"b","name":"전남악기사","mobile":"010-9876-5432","identifiers":"전남",
             "active":true},
            {"id":"c","name":"삭제된 거래처","phone":"031-123-4567","deletedAt":"2026-11-01"}
        ]});
        for query in ["경기 악기", "의정부", "031-123-4567"] {
            assert_eq!(search(&state, &json!({"query":query}))["ids"], json!(["a"]));
        }
        assert_eq!(search(&state, &json!({"query":"0109876"}))["ids"], json!(["b"]));
        assert_eq!(search(&state, &json!({"query":"삭제"}))["ids"], json!([]));
    }
}
