//! Explicit, revision-checked egui product writes through the ORIGINAL
//! Tauri business-core transition and SQLite snapshot transaction.
//! The egui preview DB and arbitrary JSON files are never write targets.
use super::{err, get_snapshot, write_snapshot};
use crate::business_core;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn required<'a>(value:&'a Value,key:&str)->Result<&'a str,String>{
    value.get(key).and_then(Value::as_str)
        .filter(|text|!text.trim().is_empty())
        .ok_or_else(||format!("{key} 필수값이 없습니다."))
}
fn unique_id(kind:&str)->Result<String,String>{
    let now=SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_|"상품 등록 ID 생성에 실패했습니다.")?;
    Ok(format!("egui-{kind}-{}-{}",std::process::id(),now.as_nanos()))
}

fn patch(conn:&mut Connection, request:&Value)->Result<Value,String>{
    let expected=request.get("expectedRevision").and_then(Value::as_u64)
        .ok_or("상품 변경 비교 리비전이 없습니다.")?;
    let action=required(request,"action")?;
    let before=get_snapshot(conn)?;
    let current=before["revision"].as_u64().ok_or("운영 DB revision 오류")?;
    if expected!=current {
        return Err("다른 작업에서 상품 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    let data=request.get("data").ok_or("상품 변경 입력이 없습니다.")?;
    let mut next=match action {
        "set_price"=>{
            let product_id=required(data,"productId")?;
            let variant_id=required(data,"variantId")?;
            let tier=required(data,"tier")?;
            // Null is the explicit "price not registered" state, and is
            // checked by the same Tauri price_set CAS rule.
            let new_price=match data.get("price") {
                Some(Value::Null)=>None,
                Some(v)=>Some(v.as_i64().ok_or("상품 단가 형식 오류")?),
                None=>return Err("새로운 상품 단가가 없습니다.".into()),
            };
            let old_price=match data.get("expected") {
                Some(Value::Null)=>None,
                Some(v)=>Some(v.as_i64().ok_or("기존 상품 단가 형식 오류")?),
                None=>return Err("기존 상품 단가를 확인할 수 없습니다.".into()),
            };
            let mut changed=before.clone();
            super::apply_product_price(
                &mut changed,product_id,variant_id,tier,new_price,old_price,
            )?;
            changed
        }
        "upsert_product"=>{
            let mut product=data.get("product").filter(|value|value.is_object())
                .cloned().ok_or("상품 상세 입력이 없습니다.")?;
            let at=chrono::Utc::now().to_rfc3339();
            if product["id"].as_str().unwrap_or("").is_empty(){
                product["id"]=json!(unique_id("product")?);
                product["createdAt"]=json!(at);
            }
            let variants=product["variants"].as_array_mut()
                .ok_or("상품 가격 구성이 없습니다.")?;
            if variants.len()!=1 {return Err("한 상품에는 하나의 판매 구성만 등록합니다.".into());}
            if variants[0]["id"].as_str().unwrap_or("").is_empty(){
                variants[0]["id"]=json!(unique_id("variant")?);
            }
            let result=business_core::execute(
                "catalog_clients","upsert_product_from_editor",
                json!({"state":&before,"product":product,"updatedAt":at}),
            )?;
            result.get("state").filter(|s|s.is_object()).cloned()
                .ok_or("Rust 상품 저장 결과 형식 오류")?
        }
        "archive_product"=>{
            let product_id=required(data,"productId")?;
            let result=business_core::execute(
                "catalog_clients","archive_product",
                json!({"state":&before,"productId":product_id,
                       "at":chrono::Utc::now().to_rfc3339()}),
            )?;
            result.get("state").filter(|s|s.is_object()).cloned()
                .ok_or("Rust 상품 삭제 결과 형식 오류")?
        }
        _=>return Err("허용되지 않은 상품관리 저장 명령입니다.".into()),
    };
    next["revision"]=json!(expected);
    let saved=write_snapshot(conn,next,expected,true)?;
    Ok(json!({"revision":saved["revision"]}))
}

pub(super) fn run()->i32{
    let result=(||->Result<Value,String>{
        if std::env::var("BANDO_EGUI_ENABLE_PRODUCT_WRITE")
            .ok().as_deref()!=Some("1") {
            return Err("운영 상품 변경 권한이 설정되지 않았습니다.".into());
        }
        let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .filter(|p|!p.is_empty()).map(PathBuf::from)
            .ok_or("운영 DB 경로가 지정되지 않았습니다.")?;
        if !path.is_file() {return Err("운영 DB 파일이 없습니다.".into());}
        let mut bytes=Vec::new();
        std::io::stdin().take(2_000_001).read_to_end(&mut bytes).map_err(err)?;
        if bytes.is_empty() || bytes.len()>2_000_000 {
            return Err("상품 변경 요청 크기가 허용 범위를 초과했습니다.".into());
        }
        let request:Value=serde_json::from_slice(&bytes)
            .map_err(|_|"상품 변경 요청 JSON 형식 오류".to_owned())?;
        let mut conn=Connection::open_with_flags(&path,OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_|"운영 DB를 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;").map_err(err)?;
        let version:i64=conn.query_row(
            "SELECT version FROM core_schema WHERE key='business-core'",
            [],|row|row.get(0),
        ).map_err(|_|"운영 DB 스키마를 확인할 수 없습니다.".to_owned())?;
        if version!=2 {return Err("지원하지 않는 운영 DB 버전입니다.".into());}
        patch(&mut conn,&request)
    })();
    let output=match &result {
        Ok(value)=>json!({"ok":true,"result":value}),
        Err(error)=>json!({"ok":false,"error":error}),
    };
    let _=std::io::stdout().write_all(output.to_string().as_bytes());
    if result.is_ok(){0}else{2}
}