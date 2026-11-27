//! Identical freight command normalization for Tauri and egui.
//! Shipping policy is still executed by the original business_core/rules module.

use serde_json::{json,Map,Value};
use std::time::{SystemTime,UNIX_EPOCH};
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} 입력값이 없습니다."))
}
fn unique(tag: &str, index: usize) -> Result<String, String> {
    let nanos=SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(err)?.as_nanos();
    Ok(format!("egui-freight-{tag}-{}-{nanos}-{index}",std::process::id()))
}
/// Explicit safety ceiling for editable freight worksheets; shared by UI and command validation.
pub const MAX_BUNDLE_QUANTITY:i64=1000;

pub(crate) fn validate_rate_input(rule:&Value)->Result<(),String>{
    if !rule["maxBundle"].is_null() &&
        !rule["maxBundle"].as_i64().is_some_and(|v|(1..=MAX_BUNDLE_QUANTITY).contains(&v)){
        return Err(format!("최대 묶음 수량은 1~{MAX_BUNDLE_QUANTITY}개까지 입력할 수 있습니다."));
    }
    let fees=rule["fees"].as_object().ok_or("수량별 운임표 형식 오류")?;
    for (quantity,amount) in fees {
        if quantity.parse::<i64>().ok().is_none_or(|n|!(1..=MAX_BUNDLE_QUANTITY).contains(&n)){
            return Err("운임 구간 수량이 올바르지 않습니다.".into());
        }
        if !amount.is_null() &&
            !amount.as_i64().is_some_and(|n|(0..=9_007_199_254_740_991).contains(&n)){
            return Err("올바른 운임을 입력하세요.".into());
        }
    }
    Ok(())
}
pub fn action_payload(action: &str, data: &Value, before: &Value) -> Result<Value, String> {
    let shipping=before.get("shipping").filter(|v|v.is_object())
        .ok_or("운영 운임 기준이 없습니다.")?;
    let now=chrono::Utc::now().to_rfc3339();
    let mut payload=json!({"action":action,"state":before,"shipping":shipping,"now":now});
    match action {
        "create_group" => {
            payload["name"]=json!(required(data,"name")?);
            payload["groupId"]=json!(unique("group",0)?);
            payload["versionId"]=json!(unique("version",0)?);
        }
        "rename_group" => {
            payload["groupId"]=json!(required(data,"groupId")?);
            payload["name"]=json!(required(data,"name")?);
        }
        "delete_group" => {
            payload["groupId"]=json!(required(data,"groupId")?);
            payload["detach"]=json!(data["detach"].as_bool().unwrap_or(false));
        }
        "assign_products" => {
            let assignments=data.get("assignments").and_then(Value::as_object)
                .ok_or("배송 그룹 변경 목록이 없습니다.")?;
            if assignments.is_empty() || assignments.len()>5_000 {
                return Err("배송 그룹 변경 건수가 올바르지 않습니다.".into());
            }
            let existing=before["products"].as_array().ok_or("상품 목록이 없습니다.")?;
            let groups=shipping["groups"].as_array().ok_or("배송 그룹 목록이 없습니다.")?;
            let mut versions=Map::new();
            for (index,(product_id,target)) in assignments.iter().enumerate() {
                if !existing.iter().any(|product|product["id"].as_str()==Some(product_id.as_str())
                    && product["deletedAt"].is_null()
                    && product["supersededBy"].is_null()) {
                    return Err(format!("등록된 상품을 찾지 못했습니다: {product_id}"));
                }
                let group_id=target.as_str().ok_or("배송 그룹 지정값 형식 오류")?;
                if !group_id.is_empty() && !groups.iter().any(|g|
                    g["id"].as_str()==Some(group_id) && g["deletedAt"].is_null()){
                    return Err("현재 존재하는 배송 그룹만 지정할 수 있습니다.".into());
                }
                if group_id.is_empty(){
                    versions.insert(product_id.clone(),json!(unique("unassign",index)?));
                }
            }
            payload["assignments"]=Value::Object(assignments.clone());
            payload["versionIds"]=Value::Object(versions);
        }
        "save_group_rule" => {
            payload["groupId"]=json!(required(data,"groupId")?);
            let rule=data.get("rule").filter(|v|v.is_object())
                .ok_or("운임표 수정 내용이 없습니다.")?;
            validate_rate_input(rule)?;
            payload["rule"]=rule.clone();
            payload["versionId"]=json!(unique("rate",0)?);
        }
        "save_product_rule" | "reset_product_freight_rule" => {
            let product_id=required(data,"productId")?;
            let products=before["products"].as_array().ok_or("상품 목록 오류")?;
            let product=products.iter().find(|p|
                p["id"].as_str()==Some(product_id) &&
                p["deletedAt"].is_null() &&
                p["supersededBy"].is_null() &&
                p["active"]!=false
            ).ok_or("등록된 상품을 찾을 수 없습니다.")?;
            let group_id=product["shippingGroupId"].as_str().unwrap_or("");
            if action=="save_product_rule" && shipping["groups"].as_array()
                .is_some_and(|groups|groups.iter().any(|g|
                    g["id"].as_str()==Some(group_id) && g["deletedAt"].is_null())){
                return Err("배송 그룹 소속 상품은 그룹 운임표에서 수정하세요.".into());
            }
            payload["productId"]=json!(product_id);
            payload["versionId"]=json!(unique("product-rate",0)?);
            if action=="save_product_rule"{
                let rule=data.get("rule").filter(|v|v.is_object())
                    .ok_or("기본 운임 입력이 없습니다.")?;
                validate_rate_input(rule)?;
                payload["rule"]=rule.clone();
            }
        }
        "copy_group_rule" => {
            let group_id=required(data,"groupId")?;
            let group=shipping["groups"].as_array().and_then(|groups|
                groups.iter().find(|g|g["id"].as_str()==Some(group_id)
                    && g["deletedAt"].is_null())
            ).ok_or("확인된 배송 그룹을 선택하세요.")?;
            let source=group["current"].as_object()
                .ok_or("복사할 그룹 운임 기준이 없습니다.")?;
            if !source.contains_key("fees"){
                return Err("복사할 그룹 운임 기준이 없습니다.".into());
            }
            let ids=data["productIds"].as_array()
                .filter(|ids|!ids.is_empty() && ids.len()<=5_000)
                .ok_or("기준을 복사할 상품을 선택하세요.")?;
            let products=before["products"].as_array().ok_or("상품 목록 오류")?;
            let mut version_ids=Map::new();
            for (index,id) in ids.iter().enumerate(){
                let pid=id.as_str().ok_or("상품 식별자 형식 오류")?;
                if !products.iter().any(|p|p["id"].as_str()==Some(pid)
                    && p["deletedAt"].is_null()
                    && p["supersededBy"].is_null()
                    && p["active"]!=false){
                    return Err("등록된 상품을 찾을 수 없습니다.".into());
                }
                if version_ids.insert(pid.to_owned(),json!(unique("copy",index)?))
                    .is_some(){
                    return Err("상품 선택이 중복되었습니다.".into());
                }
            }
            payload["groupId"]=json!(group_id);
            payload["productIds"]=Value::Array(ids.clone());
            payload["versionIds"]=Value::Object(version_ids);
        }
        "change_basis" => {
            let target=data.get("groupId").and_then(Value::as_str)
                .filter(|s|!s.is_empty());
            if let Some(id)=target {payload["groupId"]=json!(id);}
            let groups=shipping["groups"].as_array().ok_or("배송 그룹 목록 오류")?;
            let mut group_versions=Map::new();
            for (i,g) in groups.iter().enumerate() {
                if target.is_none_or(|id|g["id"].as_str()==Some(id)) {
                    let id=g["id"].as_str().ok_or("배송 그룹 ID 오류")?;
                    group_versions.insert(id.to_owned(),json!(unique("basis",i)?));
                }
            }
            payload["groupVersions"]=Value::Object(group_versions);
            if target.is_none(){
                let mut product_versions=Map::new();
                if let Some(rules)=shipping["productRules"].as_object(){
                    for (i,id) in rules.keys().enumerate(){
                        product_versions.insert(id.clone(),json!(unique("product",i)?));
                    }
                }
                payload["productVersions"]=Value::Object(product_versions);
            }
        }
        "revise_combination" => {
            payload["combinationId"]=json!(required(data,"combinationId")?);
            payload["charge"]=data.get("charge").and_then(Value::as_i64)
                .filter(|v|(0..=9_007_199_254_740_991).contains(v)).map(|v|json!(v))
                .ok_or("올바른 청구 운임을 입력하세요.")?;
            payload["newId"]=json!(unique("combination",0)?);
        }
        _=>return Err("허용되지 않은 운임 변경 명령입니다.".into())
    }
    Ok(payload)
}


#[cfg(test)]
#[test]
fn rate_input_supports_more_than_ten_quantities_and_rejects_out_of_bounds(){
    assert!(validate_rate_input(&json!({"maxBundle":12,"fees":{"1":2900,"11":null,"12":9000}})).is_ok());
    assert!(validate_rate_input(&json!({"maxBundle":MAX_BUNDLE_QUANTITY+1,"fees":{}})).is_err());
    assert!(validate_rate_input(&json!({"maxBundle":12,"fees":{"0":3000}})).is_err());
}
