//! Native settings CAS service used by both Tauri and egui.
//! No UI-provided snapshot is accepted: every write reads the authoritative
//! business state and the shared v2 SQLite writer checks revision in its
//! transaction. Per-field CAS preserves unrelated settings edits.

use rusqlite::Connection;
use serde_json::Value;

/// Apply the original Tauri settings_patch rules to an authoritative snapshot.
/// Also used for legacy Tauri snapshots so no second host-owned rule remains.
pub fn apply_settings_changes(
    mut state:Value,is_test:bool,changes:&Value,expected:&Value
)->Result<Value,String>{
    let changes=changes.as_object().ok_or("설정 변경 형식 오류")?;
    let expected=expected.as_object().ok_or("설정 기준값 형식 오류")?;
    if changes.is_empty(){return Ok(state);}
    if is_test && ["excelDirectory","factoryReplyDirectory","orderImageInboxDirectory"]
        .iter().any(|key|changes.contains_key(*key)){
        return Err("격리 작업공간의 Excel 보관 경로는 변경할 수 없습니다.".into());
    }
    let allowed=[
        "senderPostcode","excelDirectory","factoryReplyDirectory",
        "orderImageInboxDirectory","colorGuidanceGroups",
        "automaticMessageTemplates","provisionalProductNotices",
        "kakaoTitleMappings","kakaoTitleEnabled","panelRatios",
        "receptionVerticalRatio","operatorName","uiScale","accounts",
        "defaultAccountId","senderName","senderAddress","senderPhone",
        "senderMobile","rounding","brandAccounts","specialPolicy",
    ];
    for (field,updated) in changes{
        if !allowed.contains(&field.as_str()) || !expected.contains_key(field){
            return Err("허용되지 않은 설정 변경이거나 기준값이 누락되었습니다.".into());
        }
        let before=state["settings"].get(field.as_str()).unwrap_or(&Value::Null);
        if before!=&expected[field] && before!=updated{
            return Err(crate::terminology::settings_changed(field));
        }
    }
    {
        let settings=state["settings"].as_object_mut().ok_or("설정 데이터 오류")?;
        for (field,updated) in changes{
            if updated.is_null(){settings.remove(field);}
            else{settings.insert(field.clone(),updated.clone());}
        }
    }
    if changes.contains_key("colorGuidanceGroups"){
        // Reuse the exact Rust SKU/model-family validator.
        crate::validate_native_model_groups(&state)?;
    }
    if changes.contains_key("automaticMessageTemplates"){
        crate::validate_message_templates(&state)?;
    }
    if changes.contains_key("provisionalProductNotices"){
        crate::validate_provisional_product_notices(&state)?;
    }
    if ["accounts","defaultAccountId","brandAccounts"]
        .iter().any(|key|changes.contains_key(*key)){
        crate::validation::validate_accounts(&state)?;
    }
    Ok(state)
}

/// Schema-v2 settings mutation, including per-field CAS and the shared SQLite
/// transaction. This performs no migrations, backups or path selection.
pub fn patch_settings_v2(
    conn:&mut Connection,is_test:bool,changes:&Value,expected:&Value
)->Result<Value,String>{
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(|_|"운영 DB 스키마 버전 확인에 실패했습니다.".to_owned())?;
    if !crate::supported_relational_version(version){return Err("지원하지 않는 업무 DB 버전입니다.".into());}
    if changes.as_object().is_some_and(serde_json::Map::is_empty){
        return crate::read_v2_snapshot(conn);
    }
    crate::storage::update_v2_entity_if_changed(conn,true,|before|{
        let next=apply_settings_changes(before.clone(),is_test,changes,expected)?;
        Ok((next!=*before).then_some(next))
    })
}
