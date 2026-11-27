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

/// Derive committed history from the authoritative before/after product, not
/// from the editable draft or workbook preview. The existing history is
/// append-only; a missing price and explicit null are equivalent.
fn record_price_changes(
    before:&Value,after:&mut Value,at:&str,source:&str,
)->Result<(),String>{
    let mut entries=Vec::new();
    if let (Some(old),Some(new))=(before["variants"].as_array(),after["variants"].as_array()){
        for variant in new{
            let Some(id)=variant["id"].as_str().filter(|id|!id.is_empty()) else{continue};
            let Some(prior)=old.iter().find(|v|v["id"].as_str()==Some(id)) else{continue};
            for tier in ["wholesale","semi","retail"]{
                let previous=prior.get(tier).unwrap_or(&Value::Null);
                let current=variant.get(tier).unwrap_or(&Value::Null);
                if previous!=current{
                    entries.push(json!({
                        "at":at,"field":format!("{id}.{tier}"),
                        "before":previous,"after":current,"source":source
                    }));
                }
            }
        }
    }
    let target=after.as_object_mut().ok_or("상품 정보 형식 오류")?;
    // Never trust a UI draft to rewrite committed history; workbook preview
    // entries are replaced by the single authoritative commit-time record.
    if let Some(history)=before.get("priceHistory"){
        target.insert("priceHistory".into(),history.clone());
    }else{
        target.remove("priceHistory");
    }
    if !entries.is_empty(){
        let history=target.entry("priceHistory".to_owned()).or_insert_with(||json!([]));
        if history.is_null(){*history=json!([]);}
        history.as_array_mut().ok_or("상품 가격 이력이 올바르지 않습니다.")?
            .extend(entries);
    }
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
    let before=product.clone();
    let variant=product["variants"].as_array_mut()
        .ok_or("상품 옵션 오류")?.iter_mut()
        .find(|v|v["id"].as_str()==Some(variant_id))
        .ok_or("상품 옵션을 찾을 수 없습니다.")?;
    if variant[tier].as_i64()!=expected {
        return Err("가격이 다른 곳에서 수정됐습니다. 새로고침 후 다시 시도하세요.".into());
    }
    if price==expected{return Ok(());}
    variant[tier]=json!(price);
    record_price_changes(&before,product,&chrono::Utc::now().to_rfc3339(),"MANUAL")
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
    crate::storage::update_v2_entity_if_changed(conn,true,|before|{
    let mut state=before.clone();
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
    Ok((state!=*before).then_some(state))
    })
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

// Price cells already carry an exact field CAS. Editor commands carry
// the original entity; older callers without an entity retain global CAS.
// Bulk workbook review deliberately keeps its reviewed revision boundary.
/// Only fields participating in classification or freight operations enter
/// their scope token; unrelated order, customer, price and settings changes
/// cannot invalidate a pending management edit.
pub fn management_scope_token(state:&Value,domain:&str)->String{
    use sha2::{Digest,Sha256};
    let keys:&[&str]=if domain=="categories"{
        &["id","category","categories","catalog","deletedAt","supersededBy"]
    }else{
        &["id","shippingGroupId","origin","active","deletedAt","supersededBy"]
    };
    let products=state["products"].as_array().into_iter().flatten().map(|p|{
        let mut row=serde_json::Map::new();
        for key in keys{row.insert((*key).into(),p[*key].clone());}
        if domain=="shipping"{
            row.insert("variants".into(),json!(p["variants"].as_array().into_iter().flatten()
                .map(|v|v["id"].clone()).collect::<Vec<_>>()));
        }
        Value::Object(row)
    }).collect::<Vec<_>>();
    let input=json!({"domain":domain,"data":state[domain],"products":products});
    format!("{:x}",Sha256::digest(input.to_string().as_bytes()))
}

fn check_catalog_write_scope(before:&Value,action:&str,data:&Value,
    expected:u64,current:u64)->Result<(),String>{
    if action=="set_price"{return Ok(());}
    if matches!(action,"change_category"|"delete_category"|"move_products_to_category"){
        if let Some(token)=data["expectedScope"].as_str(){
            if token!=management_scope_token(before,"categories"){
                return Err("분류 또는 연결 상품이 변경되었습니다. 입력 내용을 유지합니다.".into());
            }
            return Ok(());
        }
    }
    if action=="delete_brand" && !data.get("expectedEntity").is_some_and(Value::is_object){
        return Err("삭제할 브랜드의 원본 정보가 없습니다.".into());
    }
    if action=="delete_alias" && !data.get("expectedEntity").is_some_and(Value::is_object){
        return Err("삭제할 별칭의 원본 정보가 없습니다.".into());
    }
    if let Some(original)=data.get("expectedEntity"){
        let (collection,id)=match action{
            "upsert_product"=>("products",data["product"]["id"].as_str().unwrap_or("")),
            "archive_product"=>("products",data["productId"].as_str().unwrap_or("")),
            "upsert_brand"=>("brands",data["editing"].as_str().unwrap_or("")),
            "delete_brand"=>("brands",data["brandId"].as_str().unwrap_or("")),
            "save_alias"=>("aliases",data["record"]["id"].as_str().unwrap_or("")),
            "delete_alias"=>("aliases",data["aliasId"].as_str().unwrap_or("")),
            _=>return Err("지원하지 않는 개별 저장 기준입니다.".into())
        };
        let actual=before[collection].as_array().into_iter().flatten()
            .find(|row|!id.is_empty() && row["id"].as_str()==Some(id))
            .unwrap_or(&Value::Null);
        if actual!=original{
            return Err("편집 중인 항목이 변경되었습니다. 입력 내용을 유지합니다.".into());
        }
        return Ok(());
    }
    if expected!=current{
        return Err("다른 작업에서 상품 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    Ok(())
}

// A product editor opened before a brand was deleted must not re-create
// a dangling link. Unchanged legacy references are left untouched.
fn validate_product_brand_selection(before:&Value,product:&Value)->Result<(),String>{
    let Some(id)=product["brandId"].as_str().filter(|id|!id.is_empty())else{return Ok(());};
    let prior=before["products"].as_array().into_iter().flatten()
        .find(|p|p["id"]==product["id"]);
    if prior.is_some_and(|p|p["brandId"].as_str()==Some(id)){return Ok(());}
    if !before["brands"].as_array().into_iter().flatten().any(|b|b["id"].as_str()==Some(id)){
        return Err("선택한 브랜드가 삭제되었거나 등록되어 있지 않습니다. 다시 조회한 후 선택하세요.".into());
    }
    Ok(())
}

/// Resolve complete ancestry from authoritative category records. Virtual
/// roots are available even before their first persisted category operation.
fn move_category_path(state:&Value,id:&str)->Result<Vec<(String,String)>,String>{
    let mut path=Vec::new();
    let mut visited=std::collections::HashSet::new();
    let mut cursor=id;
    while !cursor.is_empty(){
        if !visited.insert(cursor){return Err("카테고리 경로 오류".into());}
        if let Some(cat)=state["categories"].as_array().into_iter().flatten()
            .find(|c|c["id"].as_str()==Some(cursor)){
            if cat["active"]==false||cat["deletedAt"].as_str().is_some_and(|v|!v.is_empty()){
                return Err("이동할 카테고리를 사용할 수 없습니다.".into());
            }
            path.push((cursor.to_owned(),required(cat,"name")?.to_owned()));
            cursor=cat["parentId"].as_str().unwrap_or("");
        }else{
            let name=match cursor{
                "catalog-root:BANDOSTAND"=>"반도스탠드",
                "catalog-root:BANDOMALL"=>"반도몰",
                _=>return Err("카테고리가 존재하지 않습니다.".into()),
            };
            path.push((cursor.to_owned(),name.to_owned()));
            break;
        }
    }
    path.reverse();
    if path.is_empty(){return Err("카테고리 경로가 없습니다.".into());}
    Ok(path)
}

/// Derive logical leaf memberships before replacing the dragged membership.
/// Redundant ancestor IDs are not separate placements. Reject inconsistent
/// or cross-catalog moves rather than silently changing product identity.
fn moved_product(before:&Value,actual:&Value,source:&str,target:&str,at:&str)->Result<Value,String>{
    let target_path=move_category_path(before,target)?;
    let source_path=move_category_path(before,source)?;
    if source_path.first().map(|p|&p.0)!=target_path.first().map(|p|&p.0)
        && [source_path.first(),target_path.first()].into_iter().flatten()
            .any(|p|p.0.starts_with("catalog-root:")){
        return Err("최상위 상품 구분이 다른 카테고리로 이동할 수 없습니다.".into());
    }
    if let Some((root,_))=target_path.first(){
        let expected=match root.as_str(){
            "catalog-root:BANDOSTAND"=>Some("BANDOSTAND"),
            "catalog-root:BANDOMALL"=>Some("BANDOMALL"),
            _=>None,
        };
        if let Some(expected)=expected{
            if actual["catalog"].as_str().is_some_and(|v|!v.is_empty()&&v!="DEMO"&&v!=expected){
                return Err("상품의 최상위 구분과 이동 대상이 일치하지 않습니다.".into());
            }
        }
    }
    if before["categories"].as_array().into_iter().flatten()
        .any(|c|c["parentId"].as_str()==Some(target)){
        return Err("활성 최하위 카테고리에 배정하세요.".into());
    }
    let ids=actual["categories"].as_array().ok_or("상품 카테고리 목록 오류")?;
    let mut paths=Vec::new();
    for id in ids{
        let id=id.as_str().filter(|s|!s.is_empty()).ok_or("상품 카테고리 ID 오류")?;
        if !paths.iter().any(|(key,_):&(String,Vec<(String,String)>)|key==id){
            paths.push((id.to_owned(),move_category_path(before,id)?));
        }
    }
    let mut leaves=paths.iter().filter(|(id,_)|!paths.iter().any(|(other,path)|
        other!=id&&path.iter().any(|(key,_)|key==id)))
        .map(|(id,_)|id.clone()).collect::<Vec<_>>();
    let slot=leaves.iter().position(|id|id==source).ok_or("이동할 상품 소속이 변경되었습니다.")?;
    if leaves.iter().any(|id|id==target){
        leaves.remove(slot);
    }else{
        leaves[slot]=target.to_owned();
    }
    let primary=leaves.last().ok_or("상품 카테고리 목록 오류")?;
    let primary_path=move_category_path(before,primary)?;
    let mut product=actual.clone();
    product["categories"]=json!(leaves);
    product["category"]=json!(primary_path.iter().map(|(_,name)|name.as_str()).collect::<Vec<_>>().join(" > "));
    product["updatedAt"]=json!(at);
    Ok(product)
}

/// All before-images are checked inside the same IMMEDIATE SQLite write
/// transaction. Category moves are narrow edits, not product editor saves.
fn apply_category_moves(before:&Value,data:&Value)->Result<Value,String>{
    let changes=data["changes"].as_array().filter(|v|!v.is_empty())
        .ok_or("이동할 상품이 없습니다.")?;
    let mut seen=std::collections::HashSet::new();
    let mut next=before.clone();
    let at=chrono::Utc::now().to_rfc3339();
    for change in changes{
        let original=change.get("expectedEntity").filter(|v|v.is_object()).ok_or("상품 원본 정보가 없습니다.")?;
        let id=required(original,"id")?;
        let source=required(change,"source")?;
        let target=required(change,"target")?;
        if !seen.insert(id.to_owned())||source==target{return Err("중복 또는 동일 카테고리 이동입니다.".into());}
        let (position,actual)=before["products"].as_array().into_iter().flatten().enumerate()
            .find(|(_,p)|p["id"].as_str()==Some(id)).ok_or("이동할 상품이 삭제되었습니다.")?;
        if actual!=original||actual["deletedAt"].as_str().is_some_and(|v|!v.is_empty())
            ||actual["supersededBy"].as_str().is_some_and(|v|!v.is_empty()){
            return Err("선택 상품이 변경되었습니다. 전체 이동을 취소합니다.".into());
        }
        next["products"][position]=moved_product(before,actual,source,target,&at)?;
    }
    Ok(next)
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
    let data=request.get("data").ok_or("상품 변경 입력이 없습니다.")?;
    let mutate=|before:&Value|->Result<Option<Value>,String>{
    let current=before["revision"].as_u64().ok_or("운영 DB revision 오류")?;
    check_catalog_write_scope(before,action,data,expected,current)?;
    let mut next=match action {
        "apply_product_workbook"=>{
            let preview=data.get("preview").filter(|value|value.is_object())
                .ok_or("상품 Excel 변경 검토 결과가 없습니다.")?;
            // Reuse the original native reducer's revision and ERROR/HOLD
            // checks. The shared snapshot writer still validates and commits
            // the complete candidate in one optimistic SQLite transaction.
            let result=catalog_clients::execute("apply_product_workbook",
                json!({"state":&before,"preview":preview}))?;
            let mut candidate=result.get("next").filter(|value|value.is_object()).cloned()
                .ok_or("상품 Excel Rust 저장 결과 형식 오류")?;
            let old_by_id:std::collections::HashMap<&str,&Value>=before["products"]
                .as_array().ok_or("상품 목록 오류")?.iter()
                .filter_map(|p|p["id"].as_str().map(|id|(id,p))).collect();
            let at=chrono::Utc::now().to_rfc3339();
            for product in candidate["products"].as_array_mut().ok_or("상품 목록 오류")?{
                if let Some(prior)=product["id"].as_str().and_then(|id|old_by_id.get(id)){
                    record_price_changes(prior,product,&at,"MANUAL_EXCEL")?;
                }
            }
            candidate
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
        "move_products_to_category"=>{
            apply_category_moves(before,data)?
        }
        "upsert_product"=>{
            let mut product=data.get("product").filter(|value|value.is_object())
                .cloned().ok_or("상품 상세 입력이 없습니다.")?;
            validate_product_brand_selection(before,&product)?;
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
            if let Some(prior)=before["products"].as_array().into_iter().flatten()
                .find(|p|p["id"]==product["id"]){
                record_price_changes(prior,&mut product,&at,"MANUAL")?;
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
        "delete_brand"=>{
            let id=required(data,"brandId")?;
            let result=catalog_clients::execute("delete_brand",
                json!({"state":before,"brandId":id,"expectedProductIds":data["expectedProductIds"]}))?;
            result.get("state").filter(|v|v.is_object()).cloned()
                .ok_or("Rust 브랜드 삭제 결과 형식 오류")?
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
        "delete_alias"=>{
            let id=required(data,"aliasId")?;
            let result=catalog_clients::execute("delete_alias",json!({"state":before,"aliasId":id}))?;
            result.get("state").filter(|v|v.is_object()).cloned()
                .ok_or("Rust 별칭 삭제 결과 형식 오류")?
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
    next["revision"]=json!(current);
    Ok((next!=*before).then_some(next))
    };
    if action=="apply_product_workbook"{
        crate::storage::update_v2_reviewed_workbook(conn,mutate)
    }else if action=="delete_brand"{
        crate::storage::update_v2_brand_delete(conn,mutate)
    }else if matches!(action,"move_products_to_category"|"change_category"|"delete_category"){
        crate::storage::update_v2_category_change(conn,mutate)
    }else{
        crate::storage::update_v2_entity_if_changed(conn,true,mutate)
    }
}


#[cfg(test)]
mod management_scope_cas_tests{
    use super::*;
    #[test]
    fn category_batch_move_is_all_or_nothing_and_keeps_prices(){
        let product=|id:&str|json!({"id":id,"code":id,"name":id,
            "createdAt":"2026-01-01","updatedAt":"2026-01-01","active":true,
            "origin":"FACTORY","catalog":"BANDOSTAND","defaultColor":"Black",
            "categories":["a"],"category":"이전","variants":[
                {"id":format!("{id}-v"),"color":"Black","source":"MANUAL","url":"",
                "wholesale":1000,"semi":1500,"retail":2000} ]});
        let p1=product("p1");let p2=product("p2");
        let before=json!({"revision":7,"categories":[
            {"id":"a","name":"이전","parentId":"catalog-root:BANDOSTAND"},
            {"id":"b","name":"대상","parentId":"catalog-root:BANDOSTAND"}
        ],"products":[p1,p2]});
        let change=|product:&Value|json!({"expectedEntity":product,"source":"a","target":"b"});
        let good=json!({"changes":[change(&before["products"][0]),change(&before["products"][1])]});
        let after=apply_category_moves(&before,&good).unwrap();
        for i in 0..2{
            assert_eq!(after["products"][i]["categories"],json!(["b"]));
            assert_eq!(after["products"][i]["variants"],before["products"][i]["variants"]);
        }
        let mut bad=good.clone();
        bad["changes"][1]["expectedEntity"]["name"]=json!("stale");
        assert!(apply_category_moves(&before,&bad).is_err());
        assert_eq!(before["products"][0]["categories"],json!(["a"]));
        let mut bad=good.clone();
        bad["changes"][1]["target"]=json!("missing");
        assert!(apply_category_moves(&before,&bad).is_err());
        let mut bad=good.clone();
        bad["changes"][1]=bad["changes"][0].clone();
        assert!(apply_category_moves(&before,&bad).is_err());
    }

    #[test]
    fn category_move_preserves_independent_memberships_and_all_non_category_fields(){
        let before=json!({"revision":1,"categories":[
            {"id":"a","name":"A","parentId":"catalog-root:BANDOSTAND"},
            {"id":"a1","name":"A1","parentId":"a"},
            {"id":"b","name":"B","parentId":"catalog-root:BANDOSTAND"},
            {"id":"c","name":"C","parentId":"catalog-root:BANDOSTAND"},
            {"id":"x","name":"X","parentId":"catalog-root:BANDOMALL"}
        ],"products":[{"id":"p","catalog":"BANDOSTAND","modelKey":"unchanged","name":"상품",
            "categories":["a","a1","c"],"category":"C","defaultColor":"검정",
            "variants":[{"id":"v","color":"검정","retail":2000}],"priceHistory":[{"at":"old"}]},
            {"id":"other","defaultColor":"검정"}],"orders":[{"id":"o","price":5000}]});
        let original=&before["products"][0];
        let request=|target:&str|json!({"changes":[{"expectedEntity":original,"source":"a1","target":target}]});
        let after=apply_category_moves(&before,&request("b")).unwrap();
        let mut expected=before.clone();
        expected["products"][0]["categories"]=json!(["b","c"]);
        expected["products"][0]["category"]=json!("반도스탠드 > C");
        expected["products"][0]["updatedAt"]=after["products"][0]["updatedAt"].clone();
        assert_eq!(after,expected);
        assert!(apply_category_moves(&before,&request("x")).is_err());
        assert!(apply_category_moves(&before,&request("a")).is_err());
        let dedup=apply_category_moves(&before,&request("c")).unwrap();
        assert_eq!(dedup["products"][0]["categories"],json!(["c"]));
    }
    #[test]
    fn management_scopes_ignore_unrelated_order_revision_and_prices(){
        let old=json!({"revision":1,"orders":[],"clients":[],
            "categories":[{"id":"c","name":"분류"}],
            "shipping":{"groups":[{"id":"g","name":"그룹"}]},
            "products":[{"id":"p","category":"c","shippingGroupId":"g","origin":"FACTORY",
                "variants":[{"id":"v","wholesale":1000}]}]});
        let cat=management_scope_token(&old,"categories");
        let freight=management_scope_token(&old,"shipping");
        let mut new=old.clone();
        new["revision"]=json!(7);
        new["orders"]=json!([{"id":"o"}]);
        new["clients"]=json!([{"id":"dealer"}]);
        new["products"][0]["variants"][0]["wholesale"]=json!(2000);
        assert_eq!(management_scope_token(&new,"categories"),cat);
        assert_eq!(management_scope_token(&new,"shipping"),freight);
        new["products"][0]["shippingGroupId"]=json!("g2");
        assert_ne!(management_scope_token(&new,"shipping"),freight);
        assert_eq!(management_scope_token(&new,"categories"),cat);
        new["products"][0]["category"]=json!("c2");
        assert_ne!(management_scope_token(&new,"categories"),cat);
    }
    #[test]
    fn alias_delete_checks_original_entity_not_unrelated_revision(){
        let alias=json!({"id":"a","text":"709","productId":"p"});
        let state=json!({"aliases":[alias.clone()]});
        let request=json!({"aliasId":"a","expectedEntity":alias});
        assert!(check_catalog_write_scope(&state,"delete_alias",&request,1,2).is_ok());
        assert!(check_catalog_write_scope(&state,"delete_alias",&json!({"aliasId":"a"}),2,2).is_err());
        let changed=json!({"aliases":[{"id":"a","text":"other","productId":"p"}]});
        assert!(check_catalog_write_scope(&changed,"delete_alias",&request,2,2).is_err());
    }
    #[test]
    fn brand_delete_rejects_stale_product_editor_relink(){
        let state=json!({"brands":[{"id":"current"}],"products":[{"id":"p","brandId":null}]});
        assert!(validate_product_brand_selection(&state,&json!({"id":"p","brandId":"deleted"})).is_err());
        assert!(validate_product_brand_selection(&state,&json!({"id":"p","brandId":"current"})).is_ok());
        assert!(validate_product_brand_selection(&state,&json!({"id":"p","brandId":null})).is_ok());
    }
    #[test]
    fn brand_delete_requires_original_entity_and_rejects_stale_edits(){
        let brand=json!({"id":"b","name":"BANDO"});
        let state=json!({"brands":[brand.clone()]});
        let request=json!({"brandId":"b","expectedEntity":brand});
        assert!(check_catalog_write_scope(&state,"delete_brand",&request,1,2).is_ok());
        assert!(check_catalog_write_scope(&state,"delete_brand",&json!({"brandId":"b"}),2,2).is_err());
        let changed=json!({"brands":[{"id":"b","name":"changed"}]});
        assert!(check_catalog_write_scope(&changed,"delete_brand",&request,2,2).is_err());
        assert!(check_catalog_write_scope(&json!({"brands":[]}),"delete_brand",&request,2,2).is_err());
    }
    #[test]
    fn target_entity_cas_and_bulk_review_have_different_scope(){
        let original=json!({"id":"p","name":"원본"});
        let before=json!({"revision":12,"products":[original,{"id":"other"}]});
        let request=json!({"product":{"id":"p","name":"수정"},"expectedEntity":original});
        assert!(check_catalog_write_scope(&before,"upsert_product",&request,1,12).is_ok());
        assert!(check_catalog_write_scope(&before,"set_price",&json!({}),1,12).is_ok());
        let mut changed=before.clone();
        changed["products"][0]["name"]=json!("외부 변경");
        assert!(check_catalog_write_scope(&changed,"upsert_product",&request,1,12).is_err());
        assert!(check_catalog_write_scope(&before,"apply_product_workbook",&json!({}),1,12).is_err());
    }
}

#[cfg(test)]
mod price_history_tests{
    use super::*;

    fn product()->Value{
        json!({"id":"p1","variants":[{"id":"v1","wholesale":1000,
            "semi":null,"retail":3000}],"priceHistory":[{
                "at":"old","field":"v1.retail","before":2000,"after":3000,
                "source":"MANUAL"}]})
    }

    #[test]
    fn detail_and_workbook_commit_use_actual_changes_and_preserve_history(){
        for source in ["MANUAL","MANUAL_EXCEL"]{
            let before=product();
            let mut after=before.clone();
            after["variants"][0]["wholesale"]=json!(1200);
            after["variants"][0]["semi"]=json!(2000);
            after["variants"][0]["retail"]=Value::Null;
            // Simulate an untrusted draft or a workbook preview record.
            after["priceHistory"]=json!([{"field":"preview"}]);
            record_price_changes(&before,&mut after,"now",source).unwrap();
            let history=after["priceHistory"].as_array().unwrap();
            assert_eq!(history.len(),4);
            assert_eq!(history[0],before["priceHistory"][0]);
            assert_eq!(history[1],json!({"at":"now","field":"v1.wholesale",
                "before":1000,"after":1200,"source":source}));
            assert_eq!(history[2]["before"],Value::Null);
            assert_eq!(history[3]["after"],Value::Null);
            let prior=after.clone();
            record_price_changes(&prior,&mut after,"later",source).unwrap();
            assert_eq!(after["priceHistory"],prior["priceHistory"]);
        }
    }

    #[test]
    fn cell_save_records_once_and_rejects_stale_price_without_history(){
        let mut state=json!({"products":[product()]});
        apply_product_price(&mut state,"p1","v1","wholesale",Some(1200),Some(1000)).unwrap();
        let saved=state.clone();
        let history=state["products"][0]["priceHistory"].as_array().unwrap();
        assert_eq!(history.len(),2);
        assert_eq!(history[1]["field"],"v1.wholesale");
        assert_eq!(history[1]["before"],1000);
        assert_eq!(history[1]["after"],1200);
        assert!(history[1]["at"].as_str().is_some_and(|at|!at.is_empty()));
        assert!(apply_product_price(&mut state,"p1","v1","wholesale",Some(1400),Some(1000)).is_err());
        assert_eq!(state,saved);
        apply_product_price(&mut state,"p1","v1","wholesale",Some(1200),Some(1200)).unwrap();
        assert_eq!(state,saved);
        apply_product_price(&mut state,"p1","v1","wholesale",Some(1000),Some(1200)).unwrap();
        assert_eq!(state["products"][0]["priceHistory"].as_array().unwrap().len(),3);
    }

    #[test]
    fn unchanged_missing_price_and_metadata_edits_do_not_create_history(){
        let before=json!({"id":"p","variants":[{"id":"v","retail":null}]});
        let mut after=json!({"id":"p","name":"edited","variants":[{"id":"v"}]});
        record_price_changes(&before,&mut after,"now","MANUAL").unwrap();
        assert!(after.get("priceHistory").is_none());
    }
}
