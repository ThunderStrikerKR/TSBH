//! Authoritative product-management command through the shared v2 writer.
//! All catalog decisions call the EXACT existing Tauri Rust catalog_clients
//! source. This module never opens a database, uses UI drafts for decisions,
//! invokes a helper EXE, or implements duplicate product validation.
use rusqlite::Connection;
use serde_json::{json,Value};
use std::time::{SystemTime,UNIX_EPOCH};

#[path = "../../../src-tauri/src/business_core/catalog_clients.rs"]
mod catalog_clients;

pub(crate) fn validate_native_model_groups(state:&Value)->Result<(),String>{
    catalog_clients::execute("validate_model_groups",json!({"state":state}))?;
    Ok(())
}

pub fn apply_product_price(
    state:&mut Value,product_id:&str,variant_id:&str,tier:&str,
    price:Option<i64>,expected:Option<i64>,
)->Result<(),String>{
    if !["wholesale","semi","retail"].contains(&tier){
        return Err("가격 등급 오류".into());
    }
    if price.is_some_and(|amount|amount<0){
        return Err("가격은 음수가 될 수 없습니다.".into());
    }
    let product=state["products"].as_array_mut()
        .ok_or("상품 목록 오류")?.iter_mut()
        .find(|p|p["id"].as_str()==Some(product_id))
        .ok_or("상품을 찾을 수 없습니다.")?;
    let variant=product["variants"].as_array_mut()
        .ok_or("상품 옵션 오류")?.iter_mut()
        .find(|v|v["id"].as_str()==Some(variant_id))
        .ok_or("상품 옵션을 찾을 수 없습니다.")?;
    if variant[tier].as_i64()!=expected {
        return Err("가격이 다른 곳에서 수정됐습니다. 새로고침 후 다시 시도하세요.".into());
    }
    variant[tier]=json!(price);
    Ok(())
}


/// Preserve the Tauri settings_patch per-field CAS for model families.
/// Product-group policy is the SAME native catalog validator; the shared
/// snapshot writer owns optimistic revision and atomic SQLite persistence.
/// The caller performs the original mandatory manual backup beforehand.
pub fn patch_model_groups_v2(
    conn:&mut Connection,groups:&Value,expected:&Value
)->Result<Value,String>{
    if !groups.is_object(){
        return Err("모델군 목록은 객체여야 합니다.".into());
    }
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(|_|"운영 DB 스키마를 확인할 수 없습니다.".to_owned())?;
    if !crate::supported_relational_version(version){return Err("지원하지 않는 운영 DB 버전입니다.".into());}
    let mut state=crate::read_v2_snapshot(conn)?;
    let revision=state["revision"].as_u64()
        .ok_or("운영 DB revision 오류")?;
    let settings=state["settings"].as_object_mut()
        .ok_or("설정 데이터 오류")?;
    let prior=settings.get("colorGuidanceGroups").unwrap_or(&Value::Null);
    if prior!=expected && prior!=groups {
        return Err("colorGuidanceGroups 설정이 다른 화면에서 수정됐습니다. 다시 불러온 뒤 저장하세요.".into());
    }
    settings.insert("colorGuidanceGroups".into(),groups.clone());
    catalog_clients::execute(
        "validate_model_groups",json!({"state":&state})
    )?;
    let saved=crate::write_v2_snapshot(conn,state,revision,true)?;
    Ok(json!({"revision":saved["revision"]}))
}

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

/// The workbook's parsing is a UI/file-boundary concern; original catalog
/// validation, ID collision handling and price changes stay in the Tauri
/// Rust engine. Preview never opens SQLite or writes a snapshot.
pub fn preview_unified_product_workbook(
    state:&Value,source:&Value
)->Result<Value,String>{
    let products=source["products"].as_array()
        .ok_or("상품 Excel 상품 행이 없습니다.")?;
    let options=source["options"].as_array()
        .ok_or("상품 Excel 가격 행이 없습니다.")?;
    if products.len()>20_000 || options.len()>20_000 ||
        products.len()!=options.len(){
        return Err("상품 Excel은 최대 20,000개의 독립 상품 행만 지원합니다.".into());
    }
    let prefix=unique_id("workbook")?;
    let ids=(0..products.len())
        .map(|i|format!("{prefix}-product-{i}")).collect::<Vec<_>>();
    let option_ids=(0..options.len())
        .map(|i|format!("{prefix}-variant-{i}")).collect::<Vec<_>>();
    catalog_clients::execute("preview_product_sync_rows",json!({
        "state":state,"source":source,"unified":true,
        "productIds":ids,"optionIds":option_ids,
        "now":chrono::Utc::now().to_rfc3339(),
    }))
}

pub fn patch_product_v2(conn:&mut Connection, request:&Value)->Result<Value,String>{
    let expected=request.get("expectedRevision").and_then(Value::as_u64)
        .ok_or("상품 변경 비교 리비전이 없습니다.")?;
    let action=required(request,"action")?;
    // Preserve the old Tauri CLI's explicit schema guard before hydration.
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(|_|"운영 DB 스키마를 확인할 수 없습니다.".to_owned())?;
    if !crate::supported_relational_version(version){return Err("지원하지 않는 운영 DB 버전입니다.".into());}
    let before=crate::read_v2_snapshot(conn)?;
    let current=before["revision"].as_u64().ok_or("운영 DB revision 오류")?;
    if expected!=current {
        return Err("다른 작업에서 상품 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    let data=request.get("data").ok_or("상품 변경 입력이 없습니다.")?;
    let mut next=match action {
        "apply_product_workbook"=>{
            let preview=data.get("preview").filter(|value|value.is_object())
                .ok_or("상품 Excel 변경 검토 결과가 없습니다.")?;
            // Reuse the original native reducer's revision and ERROR/HOLD
            // checks. The shared snapshot writer still validates and commits
            // the complete candidate in one optimistic SQLite transaction.
            let result=catalog_clients::execute("apply_product_workbook",
                json!({"state":&before,"preview":preview}))?;
            result.get("next").filter(|value|value.is_object()).cloned()
                .ok_or("상품 Excel Rust 저장 결과 형식 오류")?
        }
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
            apply_product_price(
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
            let result=catalog_clients::execute("upsert_product_from_editor",
                json!({"state":&before,"product":product,"updatedAt":at}),
            )?;
            result.get("state").filter(|s|s.is_object()).cloned()
                .ok_or("Rust 상품 저장 결과 형식 오류")?
        }
        "upsert_brand"=>{
            // React RegistrySettings supplies only the four edited fields.
            // Brand normalization, HTTPS evidence, aliases and collisions
            // belong exclusively to the existing Rust catalog_clients rule.
            let editing=data["editing"].as_str().unwrap_or("");
            let id=if editing.is_empty(){
                unique_id("brand")?
            }else{editing.to_owned()};
            let result=catalog_clients::execute("upsert_brand",
                json!({
                    "state":&before,
                    "editing":editing,
                    "id":id,
                    "name":data["name"],
                    "korean":data["korean"],
                    "aliases":data["aliases"],
                    "evidence":data["evidence"],
                }),
            )?;
            result.get("state").filter(|v|v.is_object()).cloned()
                .ok_or("Rust 브랜드 저장 결과 형식 오류")?
        }
        "save_alias"=>{
            // The original React AliasManager submits one product id and
            // its first sale-unit variant; the Rust catalog engine owns
            // overlap/model/color/option conflict decisions.
            let mut record=data.get("record").filter(|v|v.is_object())
                .cloned().ok_or("별칭 저장 정보가 없습니다.")?;
            let id=record["id"].as_str().unwrap_or("");
            if id.is_empty(){record["id"]=json!(unique_id("alias")?);}
            let target=required(&record,"productId")?;
            let variant=before["products"].as_array()
                .and_then(|items|items.iter().find(|p|p["id"].as_str()==Some(target)))
                .and_then(|p|p["variants"].as_array())
                .and_then(|vs|vs.first())
                .and_then(|v|v["id"].as_str());
            record["variantId"]=variant.map_or(Value::Null,|id|json!(id));
            record["actor"]=json!(before["settings"]["operatorName"]
                .as_str().filter(|v|!v.trim().is_empty()).unwrap_or("직원"));
            record["createdAt"]=json!(chrono::Utc::now().to_rfc3339());
            let result=catalog_clients::execute("save_alias",
                json!({"state":&before,"record":record}),
            )?;
            result.get("state").filter(|v|v.is_object()).cloned()
                .ok_or("Rust 별칭 저장 결과 형식 오류")?
        }
        "change_category" | "delete_category"=>{
            // React CategoryManager supplies virtual catalog roots in its
            // state. The same roots must be present before native validation,
            // while retaining every existing category and product ID.
            let mut state=before.clone();
            let cats=state["categories"].as_array_mut()
                .ok_or("운영 카테고리 목록을 확인할 수 없습니다.")?;
            for (id,name) in [
                ("catalog-root:BANDOSTAND","반도스탠드"),
                ("catalog-root:BANDOMALL","반도몰"),
            ]{
                if !cats.iter().any(|c|c["id"].as_str()==Some(id)){
                    cats.push(json!({"id":id,"name":name,"parentId":null}));
                }
            }
            let payload=if action=="change_category" {
                let category_id=data["categoryId"].as_str().unwrap_or("");
                let id=if category_id.is_empty(){
                    unique_id("category")?
                }else{category_id.to_owned()};
                json!({
                    "state":state,
                    "categoryId":id,
                    "name":required(data,"name")?,
                    "parentId":data.get("parentId").unwrap_or(&Value::Null),
                    "transfer":data.get("transfer")==Some(&Value::Bool(true)),
                    "guardCatalogMove":data.get("guardCatalogMove")==Some(&Value::Bool(true)),
                    "guardError":data.get("guardError").and_then(Value::as_str).unwrap_or(""),
                })
            }else{
                json!({
                    "state":state,
                    "categoryId":required(data,"categoryId")?,
                    "destination":"",
                })
            };
            let result=catalog_clients::execute(action,payload)?;
            result.get("state").filter(|v|v.is_object()).cloned()
                .ok_or("Rust 카테고리 처리 결과 형식 오류")?
        }
        "archive_product"=>{
            let product_id=required(data,"productId")?;
            let result=catalog_clients::execute("archive_product",
                json!({"state":&before,"productId":product_id,
                       "at":chrono::Utc::now().to_rfc3339()}),
            )?;
            result.get("state").filter(|s|s.is_object()).cloned()
                .ok_or("Rust 상품 삭제 결과 형식 오류")?
        }
        _=>return Err("허용되지 않은 상품관리 저장 명령입니다.".into()),
    };
    next["revision"]=json!(expected);
    let saved=crate::write_v2_snapshot(conn,next,expected,true)?;
    Ok(json!({"revision":saved["revision"]}))
}

