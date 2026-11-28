//! One-time visual-density fixtures for the EXISTING isolated Tauri developer-test DB.
//! No production/release/HTTP path, no helper executable, no real customers or orders.
//! Data is appended atomically through the shared v2 Rust writer, once per test DB.
use crate::gateway::{BandoGateway, LocalGateway, ReadOperation};
use rusqlite::OpenFlags;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;

const MARKER: &str = "_eguiUiSamples20261120";
const VERSION: &str = "v1";
const COUNT: usize = 15;

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn unique_append(state: &mut Value, collection: &str, entry: Value) -> Result<(), String> {
    let id = field(&entry, "id");
    let rows = state.get_mut(collection).and_then(Value::as_array_mut)
        .ok_or_else(|| format!("{collection} 테스트 데이터 목록 오류"))?;
    if id.is_empty() || rows.iter().any(|row| field(row, "id") == id) {
        return Err(format!("{collection} 테스트 ID가 중복됩니다: {id}"));
    }
    rows.push(entry);
    Ok(())
}

/// Select real, registered single-variant product IDs that are not assigned to
/// a model family or shipping group. Never manufacture a fake catalog product.
fn free_products(state: &Value) -> Result<Vec<(String, String, String)>, String> {
    let mut used = HashSet::new();
    if let Some(groups) = state["settings"]["colorGuidanceGroups"].as_object() {
        for group in groups.values() {
            for id in group["productIds"].as_array().into_iter().flatten()
                .filter_map(Value::as_str)
            {
                used.insert(id.to_owned());
            }
        }
    }
    let selected = state["products"].as_array().into_iter().flatten()
        .filter(|p| {
            p["active"] != false
                && field(p, "deletedAt").is_empty()
                && field(p, "supersededBy").is_empty()
                && field(p, "catalog") != "DEMO"
                && field(p, "shippingGroupId").is_empty()
                && p["variants"].as_array().is_some_and(|v| v.len() == 1
                    && v.iter().all(|variant| field(variant, "source") != "DEMO"))
                && !used.contains(field(p, "id"))
        })
        .take(COUNT * 2)
        .map(|p| (
            field(p, "id").to_owned(),
            p["variants"][0]["id"].as_str().unwrap_or("").to_owned(),
            field(p, "name").to_owned(),
        ))
        .collect::<Vec<_>>();
    if selected.len() != COUNT * 2 || selected.iter().any(|(_, variant, _)| variant.is_empty()) {
        return Err(format!(
            "테스트 모델군에 필요한 미배정 실상품 {}개를 확보하지 못했습니다 ({}개)",
            COUNT * 2, selected.len()
        ));
    }
    Ok(selected)
}

fn build_samples(state: &mut Value) -> Result<(), String> {
    let available = free_products(state)?;
    let epoch = state["shipping"]["epoch"].as_u64().unwrap_or(1);
    if !state["shipping"]["groups"].is_array()
        || !state["shipping"]["combinations"].is_array()
    {
        return Err("개발용 배송 그룹/조합 목록이 없습니다.".into());
    }
    if !state["settings"]["colorGuidanceGroups"].is_object() {
        state["settings"]["colorGuidanceGroups"] = json!({});
    }
    let existing_names = state["settings"]["colorGuidanceGroups"]
        .as_object().ok_or("기존 모델군 형식 오류")?
        .values().map(|v| field(v, "name").to_lowercase())
        .collect::<HashSet<_>>();
    let tags = ["표준형", "이동형", "접이식", "특대형", "탁상형"];
    let tiers = ["wholesale", "semi", "retail", "special"];
    for n in 0..COUNT {
        let index = n + 1;
        let suffix = format!("{index:02}");
        let first = &available[n * 2];
        let second = &available[n * 2 + 1];
        let group_id = format!("test-ui-ship-{suffix}");
        let model_id = format!("test-ui-model-{suffix}");
        let model_name = format!("[TEST] 모델군 {suffix} · {} / {}",
            tags[n % tags.len()], first.2.split('/').next().unwrap_or(&first.2).trim());
        if existing_names.contains(&model_name.to_lowercase())
            || state["settings"]["colorGuidanceGroups"].get(&model_id).is_some()
        {
            return Err(format!("모델군 테스트 데이터가 이미 존재합니다: {model_id}"));
        }
        state["settings"]["colorGuidanceGroups"][&model_id] = json!({
            "name":model_name,
            "productIds":[first.0, second.0],
            "representativeProductId":first.0
        });

        let current_version = format!("test-ui-ship-rate-{suffix}-v2");
        let current_at = format!("2026-11-20T10:{index:02}:00+09:00");
        let old_at = format!("2026-10-20T10:{index:02}:00+09:00");
        let base = 2900 + (n as i64) * 140;
        let fees = json!({
            "1":base, "2":base+700, "3":base+1300,
            "4":base+1850, "5":Value::Null, "6":Value::Null
        });
        let old_fees = json!({
            "1":base-200, "2":base+450, "3":base+950,
            "4":Value::Null,"5":Value::Null,"6":Value::Null
        });
        let rule = |version: String, at: String, fees: Value| json!({
            "version":version,"at":at,"fees":fees,
            "maxBundle":4,"maxConfirmed":true,"perItem":Value::Null,
            "notes":format!("[TEST] 배송그룹 {suffix} · UI 목록 및 기준 이력 확인용")
        });
        let current = rule(current_version.clone(), current_at.clone(), fees);
        let old = rule(format!("test-ui-ship-rate-{suffix}-v1"),old_at,old_fees);
        let shipping_name = format!("[TEST] 배송그룹 {suffix} · {}",tags[n % tags.len()]);
        if state["shipping"]["groups"].as_array().is_some_and(|groups|
            groups.iter().any(|g| field(g, "name") == shipping_name))
        {
            return Err(format!("배송그룹 이름이 중복됩니다: {shipping_name}"));
        }
        let group = json!({
            "id":group_id,"name":shipping_name,
            "current":current,"history":[old]
        });
        let groups = state["shipping"]["groups"].as_array_mut()
            .ok_or("배송그룹 목록 오류")?;
        if groups.iter().any(|g| field(g, "id") == group_id) {
            return Err(format!("배송그룹 ID가 중복됩니다: {group_id}"));
        }
        groups.push(group);
        for selected in [first, second] {
            let row = state["products"].as_array_mut().into_iter().flatten()
                .find(|p| field(p, "id") == selected.0)
                .ok_or("배송그룹 대상 상품이 없습니다.")?;
            row["shippingGroupId"] = json!(group_id);
        }

        let client_id = format!("test-ui-client-{suffix}");
        unique_append(state, "clients", json!({
            "id":client_id,
            "name":format!("[TEST] 가상 거래처 {suffix} · {}",tags[n % tags.len()]),
            "displayName":format!("테스트 납품처 {suffix}"),
            "shortNames":format!("가상{suffix}, 테스트거래처{suffix}"),
            "identifiers":format!("UI-SAMPLE-C{suffix}"),
            "address":format!("서울특별시 중구 세종대로 {index} (가상 주소)"),
            "phone":format!("02-0000-{index:04}"),
            "mobile":"",
            "tier":tiers[n % tiers.len()],
            "payment":if n % 3 == 0 {"POSTPAID"} else {"PREPAID"},
            "preferredAccountId":Value::Null,
            "shippingFeeExcluded":n % 5 == 0,
            "defaultApplyVat":n % 3 == 1,
            "active":true,
            "contact":format!("UI 테스트 담당자 {suffix}"),
            "overrides":{}
        }))?;

        unique_append(state, "brands", json!({
            "id":format!("test-ui-brand-{suffix}"),
            "name":format!("UI-TEST-BRAND-{suffix}"),
            "korean":format!("가상 테스트 상표 {suffix}"),
            "aliases":[format!("UITESTBRAND{suffix}"),format!("샘플상표{suffix}")],
            "evidenceUrl":format!("https://example.com/ui-fixture/brand/{suffix}")
        }))?;

        unique_append(state, "aliases", json!({
            "id":format!("test-ui-alias-{suffix}"),
            "text":format!("UITEST-ALIAS-{suffix}"),
            "productId":first.0,
            "variantId":first.1,
            "clientId":if n % 3 == 0 { json!(client_id) } else { Value::Null }
        }))?;

        let quantity = (n % 3 + 1) as i64;
        let combination_key = format!("FACTORY|{}@{}:{quantity}",first.0,first.1);
        let mut versions = serde_json::Map::new();
        versions.insert(format!("assignment:{}", first.0), json!(group_id));
        versions.insert(format!("group:{group_id}"), json!(current_version));
        unique_append_combination(state,json!({
            "id":format!("test-ui-combo-{suffix}"),
            "key":combination_key,
            "epoch":epoch,
            "versions":Value::Object(versions),
            "charge":base+quantity*400,
            "actual":base+quantity*350,
            "at":current_at,
            "active":true,
            "sampleCount":0,
            "description":format!("[TEST] {} × {quantity} / 가상 운임 조합 {suffix}",
                first.2.split('/').next().unwrap_or(&first.2).trim())
        }))?;
    }
    // This marker belongs only to the TEST snapshot and advances atomically
    // with all entries. Startup never edits those entries again.
    state["settings"][MARKER] = json!(VERSION);
    Ok(())
}

fn unique_append_combination(state: &mut Value, entry: Value) -> Result<(), String> {
    let id = field(&entry, "id");
    let rows = state["shipping"]["combinations"].as_array_mut()
        .ok_or("배송 조합 목록 오류")?;
    if rows.iter().any(|r| field(r, "id") == id
        || (r["active"] == true && field(r, "key") == field(&entry, "key")))
    {
        return Err(format!("테스트 운임 조합 중복: {id}"));
    }
    rows.push(entry);
    Ok(())
}

/// Called only by the opt-in DEV launcher after egui binds an existing v2 DB.
pub fn seed_if_requested() -> Result<(), String> {
    if std::env::var("BANDO_EGUI_UI_SAMPLE_DATA").ok().as_deref() != Some("1") {
        return Ok(());
    }
    if !cfg!(windows) || !cfg!(debug_assertions)
        || std::env::var("BANDO_EGUI_DEVELOPER_TEST").ok().as_deref() != Some("1")
        || std::env::var("BANDO_HUB_GATEWAY").ok()
            .is_some_and(|v| !v.eq_ignore_ascii_case("local"))
    {
        return Err("UI 테스트 데이터는 Windows 개발용 격리 DB에서만 등록합니다.".into());
    }
    let path = crate::business_state::authorized_business_write_path()?;
    let mut conn = crate::business_state::open_business_connection(&path,OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_|"개발용 테스트 DB를 쓰기 모드로 열지 못했습니다.".to_owned())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    conn.execute_batch("PRAGMA foreign_keys=ON;").map_err(|e|e.to_string())?;
    let before = bando_data_access::read_v2_snapshot(&conn)?;
    if before["settings"][MARKER] == VERSION {
        eprintln!("[BANDO HUB TEST] UI 샘플 데이터가 이미 등록되어 있어 건너뜁니다.");
        return Ok(());
    }
    let revision = before["revision"].as_u64()
        .ok_or("개발용 DB revision을 확인할 수 없습니다.")?;
    let mut next = before.clone();
    build_samples(&mut next)?;
    bando_data_access::valid_state(&next)?;
    bando_data_access::validate_clients(&before,&next)?;
    bando_data_access::validate_brands(&before,&next)?;
    crate::catalog_query_bridge::validate_model_groups(
        &next,&next["settings"]["colorGuidanceGroups"])?;
    let diagnostics = crate::catalog_query_bridge::alias_problems(&next)?;
    for row in diagnostics.as_array().into_iter().flatten() {
        if field(row, "id").starts_with("test-ui-alias-")
            && row["problems"].as_array().is_some_and(|items|!items.is_empty())
        {
            return Err(format!("테스트 별칭 업무 규칙 검증 실패: {}",row["problems"]));
        }
    }
    let projection = LocalGateway.execute(ReadOperation::ShippingManagerView,
        json!({"state":&next,"shipping":&next["shipping"]}))?;
    for index in 1..=COUNT {
        let id = format!("test-ui-combo-{index:02}");
        if projection["currentCombinations"][&id] != true {
            return Err(format!("테스트 배송 조합 기준이 유효하지 않습니다: {id}"));
        }
    }
    let saved = bando_data_access::write_v2_snapshot(&mut conn,next,revision,true)?;
    if saved["revision"].as_u64() != Some(revision+1)
        || saved["settings"][MARKER] != VERSION
    {
        return Err("테스트 데이터 저장 결과 revision이 일치하지 않습니다.".into());
    }
    eprintln!(
        "[BANDO HUB TEST] UI 샘플 등록: 모델군/별칭/브랜드/배송그룹/배송조합/기준이력/거래처 각각 {COUNT}건"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_samples_populate_all_seven_views_without_replacing_real_catalog() {
        let mut state: Value = serde_json::from_str(include_str!(
            "../../src-tauri/resources/state-seed.json"
        )).expect("bundled production catalog seed must be well formed");
        let products_before = state["products"].as_array().unwrap().len();
        let orders_before = state["orders"].clone();
        build_samples(&mut state).expect("synthetic UI fixture should be valid");
        assert_eq!(state["products"].as_array().unwrap().len(), products_before);
        assert_eq!(state["orders"], orders_before);
        assert_eq!(state["brands"].as_array().unwrap().len(), COUNT);
        assert_eq!(state["aliases"].as_array().unwrap().len(), COUNT);
        assert_eq!(state["clients"].as_array().unwrap().len(), COUNT);
        assert_eq!(state["shipping"]["groups"].as_array().unwrap().len(), COUNT);
        assert_eq!(state["shipping"]["combinations"].as_array().unwrap().len(), COUNT);
        assert_eq!(state["settings"]["colorGuidanceGroups"]
            .as_object().unwrap().len(), COUNT);
        assert_eq!(state["shipping"]["groups"].as_array().unwrap().iter()
            .map(|group| group["history"].as_array().unwrap().len())
            .sum::<usize>(), COUNT);
        bando_data_access::valid_state(&state).unwrap();
        let original: Value = serde_json::from_str(include_str!(
            "../../src-tauri/resources/state-seed.json"
        )).unwrap();
        bando_data_access::validate_clients(&original,&state).unwrap();
        bando_data_access::validate_brands(&original,&state).unwrap();
        crate::catalog_query_bridge::validate_model_groups(
            &state,&state["settings"]["colorGuidanceGroups"]).unwrap();
        let problems = crate::catalog_query_bridge::alias_problems(&state).unwrap();
        assert!(problems.as_array().unwrap().iter().all(|r|
            r["problems"].as_array().is_some_and(Vec::is_empty)));
        let view = LocalGateway.execute(ReadOperation::ShippingManagerView,
            json!({"state":&state,"shipping":&state["shipping"]})).unwrap();
        assert!(view["currentCombinations"].as_object().unwrap()
            .values().all(|active|active.as_bool() == Some(true)));
    }
}
