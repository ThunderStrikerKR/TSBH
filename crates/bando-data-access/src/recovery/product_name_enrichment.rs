//! One-time, conservative catalog name enrichment for independent sale units.
//! The mapping records the pre-update stock name and exact price-record ID.
//! The branded prefix alone is removed from active employee-edited names;
//! all other wording, order snapshots, prices and records remain untouched.
use super::*;
use serde_json::Value;

pub(super) fn apply_to_state(state: &mut Value) -> Result<usize, String> {
    let mapping: Value = serde_json::from_str(include_str!(
        "../../../src-tauri/resources/product-name-enrichment-20261109.json"
    )).map_err(err)?;
    let updates = mapping["changes"].as_array()
        .ok_or("상품명 변경 목록이 누락됐습니다.")?;
    let products = state["products"].as_array_mut()
        .ok_or("상품명 갱신 전 상품 목록을 확인할 수 없습니다.")?;
    let mut changed = 0_usize;
    for update in updates {
        let Some(product) = products.iter_mut().find(|p| p["id"] == update["id"]) else {
            continue;
        };
        if product["name"] != update["oldName"]
            || product["originalName"] != update["originalName"]
            || product["variants"].as_array().is_none_or(|v| v.len() != 1)
            || product["variants"][0]["id"] != update["priceRecordId"]
            || product["deletedAt"].as_str().is_some()
            || product["supersededBy"].as_str().is_some()
        {
            continue; // Do not overwrite employee edits or ambiguous records.
        }
        let Some(new_name) = update["newName"].as_str() else {
            return Err("상품명 변경 목록의 새 이름이 올바르지 않습니다.".into());
        };
        if new_name.trim().is_empty() || new_name.contains('◆') {
            return Err("상품명 데이터에는 공장 엑셀용 구분 기호를 저장할 수 없습니다.".into());
        }
        product["name"] = Value::String(new_name.to_string());
        changed += 1;
    }

    // Older SQLite databases can still contain official website titles such as
    // "반도스탠드 BD Desk JS-3 Black 검정 (자바라 길이 43cm)".
    // Only use the full curated name when the source text matches exactly.
    // The complete, independently curated 360-item catalog is authoritative
    // only when the stored name is exactly the unedited official original.
    let catalog: Value = serde_json::from_str(include_str!("../../../src-tauri/resources/state-seed.json"))
        .map_err(err)?;
    let reference = catalog["products"].as_array()
        .ok_or("공식 상품명 기준표가 누락됐습니다.")?;
    let mut catalog_by_id = std::collections::HashMap::new();
    for candidate in reference {
        let id = candidate["id"].as_str()
            .ok_or("공식 상품명 기준표의 상품 ID가 누락됐습니다.")?;
        if catalog_by_id.insert(id, candidate).is_some() {
            return Err(format!("공식 상품명 기준표의 상품 ID가 중복됐습니다: {id}"));
        }
    }
    // Source importers sometimes collapse spaces in expressions such as
    // "자바라 길이" -> "자바라길이". Compare only whitespace-insensitively,
    // never by prefix alone; fallback below keeps all employee-added wording.
    let same_official_text = |a: &Value, b: &Value| -> bool {
        match (a.as_str(), b.as_str()) {
            (Some(left), Some(right)) =>
                left.chars().filter(|ch| !ch.is_whitespace())
                    .eq(right.chars().filter(|ch| !ch.is_whitespace())),
            _ => false,
        }
    };
    for product in products.iter_mut() {
        let Some(id) = product["id"].as_str() else { continue };
        let Some(reference) = catalog_by_id.get(id) else { continue };
        let Some(existing_name) = product["name"].as_str() else { continue };
        if !existing_name.starts_with("반도스탠드 ")
            || !same_official_text(&product["name"], &reference["originalName"])
            || product["variants"].as_array().is_none_or(|v| v.len() != 1)
            || product["variants"][0]["id"] != reference["variants"][0]["id"]
            || product["deletedAt"].as_str().is_some()
            || product["supersededBy"].as_str().is_some()
            || product["priceHistory"].as_array().is_some_and(|history|
                history.iter().any(|entry| entry["field"] == "name"))
        {
            continue;
        }
        let Some(curated) = reference["name"].as_str() else {
            return Err(format!("상품 {id}의 정리된 상품명이 누락됐습니다."));
        };
        if curated.trim().is_empty()
            || curated.starts_with("반도스탠드 ")
            || curated.contains('◆')
        {
            return Err(format!("상품 {id}의 정리된 상품명 표기가 올바르지 않습니다."));
        }
        product["name"] = Value::String(curated.to_string());
        changed += 1;
    }

    // Guaranteed display cleanup: preexisting DB rows may lack siteCode,
    // catalog or source metadata, or contain genuine employee-edited names.
    // The strict mapping above cannot identify all such rows. Remove ONLY
    // the unwanted leading brand token; never change the remaining product
    // wording, identity, price, description, or historical snapshots.
    for product in products.iter_mut() {
        if product["deletedAt"].as_str().is_some()
            || product["supersededBy"].as_str().is_some()
        {
            continue;
        }
        let Some(existing) = product["name"].as_str() else { continue };
        let Some(tail) = existing.strip_prefix("반도스탠드 ") else { continue };
        let clean = tail.trim_start();
        if clean.is_empty() {
            return Err("상품명에서 브랜드명을 제거하면 이름이 비어 갱신할 수 없습니다.".into());
        }
        product["name"] = Value::String(clean.to_string());
        changed += 1;
    }

    // Update the exact previously seeded Desk labels only. Every visible
    // selling configuration remains an independent item with one price ID.
    // Employee-specific product labels are never overwritten.
    let desk_old_names = [
        ("bando-site-247", "BD Desk TS-1 / 미니형 마이크 스탠드"),
        ("bando-site-248", "BD Desk TS-2 / 미니형 마이크 스탠드"),
        ("bando-site-249", "BD Desk TS-3 / 미니형 마이크 스탠드"),
        ("bando-site-250", "BD Desk IS-3 / 바 길이 26cm"),
        ("bando-site-251", "BD Desk IS-2 / 바 길이 15cm"),
        ("bando-site-252", "BD Desk IS-1 / 바 길이 10cm"),
    ];
    for (id, old_name) in desk_old_names {
        let Some(reference) = catalog_by_id.get(id) else {
            return Err(format!("Desk 제품 기준표 누락: {id}"));
        };
        let Some(product) = products.iter_mut().find(|p| p["id"] == id) else {
            continue;
        };
        if product["name"].as_str() != Some(old_name)
            || product["variants"].as_array().is_none_or(|v| v.len() != 1)
            || product["variants"][0]["id"] != reference["variants"][0]["id"]
            || product["deletedAt"].as_str().is_some()
            || product["supersededBy"].as_str().is_some()
        {
            continue;
        }
        let Some(new_name) = reference["name"].as_str() else {
            return Err(format!("Desk 제품 새 이름 누락: {id}"));
        };
        if new_name.contains('◆') || new_name.trim().is_empty() {
            return Err(format!("Desk 제품명 규칙 위반: {id}"));
        }
        product["name"] = Value::String(new_name.to_string());
        // "길이 확인 필요" was an old placeholder, not an actual specification.
        if product["subtitle"].as_str() == Some("길이 확인 필요") {
            product["subtitle"] = Value::String(String::new());
        }
        changed += 1;
    }

    // User-provided DESK/FLOOR specification tables: 18 models, 21
    // independently sold color-specific items. Update only recognized
    // default labels with exact product/price/source identifiers.
    // Bar length and overall height are different measurements (IS-3).
    let stand_specs: Value = serde_json::from_str(include_str!(
        "../../../src-tauri/resources/microphone-stand-specifications-20261109.json"
    )).map_err(err)?;
    let stand_items = stand_specs["items"].as_array()
        .ok_or("마이크 스탠드 규격 목록이 누락됐습니다.")?;
    for spec in stand_items {
        let Some(id) = spec["id"].as_str() else {
            return Err("마이크 스탠드 규격의 상품 ID가 누락됐습니다.".into());
        };
        let Some(reference) = catalog_by_id.get(id) else {
            return Err(format!("마이크 스탠드 기준 상품 누락: {id}"));
        };
        if reference["name"] != spec["name"]
            || reference["originalName"] != spec["originalName"]
            || reference["variants"].as_array().is_none_or(|v| v.len() != 1)
            || reference["variants"][0]["id"] != spec["priceRecordId"]
        {
            return Err(format!("마이크 스탠드 기준 상품이 불일치합니다: {id}"));
        }
        let Some(product) = products.iter_mut().find(|p| p["id"] == id) else {
            continue;
        };
        if product["originalName"] != spec["originalName"]
            || product["variants"].as_array().is_none_or(|v| v.len() != 1)
            || product["variants"][0]["id"] != spec["priceRecordId"]
            || product["deletedAt"].as_str().is_some()
            || product["supersededBy"].as_str().is_some()
            || product["priceHistory"].as_array().is_some_and(|history|
                history.iter().any(|entry| entry["field"] == "name"))
        {
            continue;
        }
        let Some(current_name) = product["name"].as_str() else { continue };
        let Some(target_name) = spec["name"].as_str() else {
            return Err(format!("마이크 스탠드 상품명이 누락됐습니다: {id}"));
        };
        // Previously shipped specification labels can be corrected without
        // guessing which employee-entered wording refers to a physical model.
        let matches_prior_spec = spec["previousNames"].as_array().is_some_and(
            |names| names.iter().any(|name| name.as_str() == Some(current_name))
        );
        if current_name != target_name
            && product["name"] != spec["oldName"]
            && !matches_prior_spec
        {
            continue; // Employee-edited names are authoritative.
        }
        if target_name.trim().is_empty() || target_name.contains('◆') {
            return Err(format!("마이크 스탠드 상품명 규칙 위반: {id}"));
        }
        if current_name != target_name {
            product["name"] = Value::String(target_name.to_string());
            changed += 1;
        }
        let Some(target_description) = spec["subtitle"].as_str() else {
            return Err(format!("마이크 스탠드 설명이 누락됐습니다: {id}"));
        };
        let current_description = product["subtitle"].as_str().unwrap_or("");
        let default_description = spec["oldSubtitle"].as_str().unwrap_or("");
        if current_description != target_description
            && (current_description.is_empty()
                || current_description == default_description
                || current_description == "길이 확인 필요")
        {
            product["subtitle"] = Value::String(target_description.to_string());
            changed += 1;
        }
    }

    // Exact-model label corrections for the Mighty Bright LED family.
    // Previous releases already wrote brand-first names to SQLite; matching
    // only an official source title would not update those installed records.
    // Preserve any custom employee wording and the original website title.
    let led_old_names = [
        ("bando-site-229", "BD LED-2s / Mighty Bright Light"),
        ("bando-site-301", "Mighty Bright LED-2W Wonderflex / 보면대등"),
        ("bando-site-302", "Mighty Bright LED-3N Nuflex / 보면대등"),
    ];
    for (id, old_name) in led_old_names {
        let Some(reference) = catalog_by_id.get(id) else {
            return Err(format!("LED 기준 상품 누락: {id}"));
        };
        let Some(product) = products.iter_mut().find(|p| p["id"] == id) else {
            continue;
        };
        if product["name"].as_str() != Some(old_name)
            || product["variants"].as_array().is_none_or(|v| v.len() != 1)
            || product["variants"][0]["id"] != reference["variants"][0]["id"]
            || product["deletedAt"].as_str().is_some()
            || product["supersededBy"].as_str().is_some()
        {
            continue;
        }
        let Some(new_name) = reference["name"].as_str() else {
            return Err(format!("LED 정리 이름 누락: {id}"));
        };
        if !new_name.starts_with("BD LED-") || new_name.contains('◆') {
            return Err(format!("LED 상품명 규칙 위반: {id}"));
        }
        product["name"] = Value::String(new_name.to_string());
        changed += 1;
    }
    // "Mighty Bright Light" is already in the title of these two
    // independent products. Do not duplicate it as an internal subtitle.
    for id in ["bando-site-228", "bando-site-229"] {
        let Some(product) = products.iter_mut().find(|p| p["id"] == id) else {
            continue;
        };
        if product["subtitle"].as_str() == Some("Mighty Bright Light")
            && product["deletedAt"].as_str().is_none()
            && product["supersededBy"].as_str().is_none()
        {
            if let Some(fields) = product.as_object_mut() {
                fields.remove("subtitle");
                changed += 1;
            }
        }
    }

    // These are physically duplicate sale configurations, not separate
    // options. Keep both original IDs and frozen order references in SQLite,
    // but retire the redundant standalone site listings from all new sales.
    let retired_music_trays = [
        ("bando-site-77", "bando-77-single",
         "sku:bando-site-237:bando-237-P00000JC000H",
         "bando-237-P00000JC000H"),
        ("bando-site-78", "bando-78-single",
         "sku:bando-site-237:bando-237-P00000JC000I",
         "bando-237-P00000JC000I"),
    ];
    for (duplicate_id, old_variant_id, canonical_id, canonical_variant_id)
        in retired_music_trays
    {
        let Some(canonical) = products.iter().find(|p| p["id"] == canonical_id) else {
            return Err(format!("중복 뮤직 트레이의 정본 상품을 찾을 수 없습니다: {canonical_id}"));
        };
        if canonical["variants"].as_array().is_none_or(|v| v.len() != 1)
            || canonical["variants"][0]["id"] != canonical_variant_id
            || canonical["deletedAt"].as_str().is_some()
            || canonical["supersededBy"].as_str().is_some()
        {
            return Err(format!("중복 뮤직 트레이 정본 식별자가 변경됐습니다: {canonical_id}"));
        }
        let Some(duplicate) = products.iter_mut().find(|p| p["id"] == duplicate_id) else {
            continue;
        };
        if duplicate["variants"].as_array().is_none_or(|v| v.len() != 1)
            || duplicate["variants"][0]["id"] != old_variant_id
            || duplicate["deletedAt"].as_str().is_some()
            || duplicate["supersededBy"].as_str().is_some_and(|to| to != canonical_id)
        {
            continue;
        }
        if duplicate["supersededBy"].as_str() != Some(canonical_id)
            || duplicate["active"] != false
        {
            duplicate["supersededBy"] = Value::String(canonical_id.to_string());
            duplicate["active"] = Value::Bool(false);
            changed += 1;
        }
    }

    // A separated sale item must not list all other colors as its subtitle.
    // Preserve any description the employee has already changed.
    for product in products.iter_mut() {
        let id = product["id"].as_str().unwrap_or("");
        if [
            "sku:bando-site-237:bando-237-P00000JC000H",
            "sku:bando-site-237:bando-237-P00000JC000I",
            "sku:bando-site-237:bando-237-P00000JC000J",
        ].contains(&id)
            && product["subtitle"].as_str() == Some("다크그린, 와인, 그레이 옵션")
        {
            if let Some(object) = product.as_object_mut() {
                object.remove("subtitle");
                changed += 1;
            }
        }
    }

    // Never silently claim completion while a visible active product still
    // carries the same unwanted prefix. All writes happen only after this.
    if let Some(product) = products.iter().find(|p| {
        p["active"] != false
            && p["deletedAt"].as_str().is_none()
            && p["supersededBy"].as_str().is_none()
            && p["name"].as_str().is_some_and(|name| name.starts_with("반도스탠드 "))
    }) {
        return Err(format!(
            "상품명 브랜드 접두어 정리 실패: {}",
            product["id"].as_str().unwrap_or("ID 없음")
        ));
    }
    Ok(changed)
}

