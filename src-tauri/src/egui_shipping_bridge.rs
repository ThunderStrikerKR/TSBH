//! Narrow, explicit egui freight write bridge.
//! The authoritative Tauri business snapshot is loaded inside the write
//! boundary. Never trust a UI-submitted shipping state or product price.
use super::{business_core, err, get_snapshot, write_snapshot};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

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
fn action_payload(action: &str, data: &Value, before: &Value) -> Result<Value, String> {
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
                if !existing.iter().any(|product|product["id"]==*product_id
                    && product["deletedAt"].is_null()
                    && product["supersededBy"].is_null()) {
                    return Err(format!("등록된 상품을 찾지 못했습니다: {product_id}"));
                }
                let group_id=target.as_str().ok_or("배송 그룹 지정값 형식 오류")?;
                if !group_id.is_empty() && !groups.iter().any(|g|
                    g["id"]==group_id && g["deletedAt"].is_null()){
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
            payload["rule"]=data.get("rule").filter(|v|v.is_object())
                .ok_or("운임표 수정 내용이 없습니다.")?.clone();
            payload["versionId"]=json!(unique("rate",0)?);
        }
        "change_basis" => {
            let target=data.get("groupId").and_then(Value::as_str)
                .filter(|s|!s.is_empty());
            if let Some(id)=target {payload["groupId"]=json!(id);}
            let groups=shipping["groups"].as_array().ok_or("배송 그룹 목록 오류")?;
            let mut group_versions=Map::new();
            for (i,g) in groups.iter().enumerate() {
                if target.is_none_or(|id|g["id"]==id) {
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
                .filter(|v|*v>=0).map(|v|json!(v))
                .ok_or("올바른 청구 운임을 입력하세요.")?;
            payload["newId"]=json!(unique("combination",0)?);
        }
        _=>return Err("허용되지 않은 운임 변경 명령입니다.".into())
    }
    Ok(payload)
}

pub(super) fn run() -> i32 {
    let result=(|| -> Result<Value,String> {
        if std::env::var("BANDO_EGUI_ENABLE_SHIPPING_WRITE")
            .ok().as_deref()!=Some("1") {
            return Err("운영 운임 변경 권한이 설정되지 않았습니다.".into());
        }
        let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .filter(|value|!value.is_empty()).map(PathBuf::from)
            .ok_or("운영 DB 경로가 지정되지 않았습니다.")?;
        if !path.is_file(){return Err("운영 DB 파일이 없습니다.".into());}
        let mut bytes=Vec::new();
        std::io::stdin().take(2_000_001).read_to_end(&mut bytes).map_err(err)?;
        if bytes.is_empty() || bytes.len()>2_000_000 {
            return Err("운임 변경 요청 크기가 허용 범위를 초과했습니다.".into());
        }
        let request:Value=serde_json::from_slice(&bytes)
            .map_err(|_|"운임 변경 요청 JSON이 올바르지 않습니다.".to_owned())?;
        let expected=request["expectedRevision"].as_u64()
            .ok_or("운임 변경 비교 리비전이 없습니다.")?;
        let action=required(&request,"action")?;
        let data=request.get("data").ok_or("운임 변경 내용이 없습니다.")?;
        let mut conn=Connection::open_with_flags(
            &path,OpenFlags::SQLITE_OPEN_READ_WRITE
        ).map_err(|_|"운영 DB를 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
        let version:i64=conn.query_row(
            "SELECT version FROM core_schema WHERE key='business-core'",
            [],|row|row.get(0),
        ).map_err(|_|"운영 DB 스키마를 확인할 수 없습니다.".to_owned())?;
        if version!=2 {return Err("지원하지 않는 운영 DB 버전입니다.".into());}
        let before=get_snapshot(&conn)?;
        let revision=before["revision"].as_u64().ok_or("운영 DB 리비전 오류")?;
        if revision!=expected {
            return Err("다른 작업에서 데이터가 수정되었습니다. 다시 불러오세요.".into());
        }
        let payload=action_payload(action,data,&before)?;
        let result=business_core::execute("rules","apply_shipping_action",payload)?;
        let mut next=if result.get("state").is_some() {
            result["state"].clone()
        } else if let Some(shipping)=result.get("shipping"){
            let mut snapshot=before.clone();
            snapshot["shipping"]=shipping.clone();
            snapshot
        } else {return Err("Rust 운임 처리 결과를 확인할 수 없습니다.".into());};
        // This is the same revision-checked, transactionally persisted native
        // state path used by Tauri. Prices and unrelated data are not reset.
        next["revision"]=json!(revision);
        let committed=write_snapshot(&mut conn,next,expected,true)?;
        Ok(json!({"revision":committed["revision"]}))
    })();
    let response=match &result{
        Ok(value)=>json!({"ok":true,"result":value}),
        Err(message)=>json!({"ok":false,"error":message}),
    };
    let _=std::io::stdout().write_all(response.to_string().as_bytes());
    if result.is_ok(){0}else{2}
}
