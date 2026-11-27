//! Native brand edit and validation, preserving original
//! src/product-registry.ts::validateBrands semantics.
use serde_json::{json,Value};
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;
use url::Url;
use super::{field,items};

fn registry_key(value:&str)->String{
    value.nfkc().collect::<String>().trim()
        .chars().flat_map(char::to_lowercase).collect::<String>()
        .split_whitespace().collect::<Vec<_>>().join(" ")
}
fn terms(brand:&Value)->Vec<String>{
    ["name","korean"].iter().filter_map(|key|brand.get(*key).and_then(Value::as_str))
        .chain(items(brand,"aliases").iter().filter_map(Value::as_str))
        .filter(|term|!term.is_empty()).map(registry_key).collect()
}
pub(super) fn validate(before:&Value,next:&Value)->Result<(),String>{
    let brands=items(next,"brands");
    for (position,brand) in brands.iter().enumerate(){
        let id=field(brand,"id");
        if items(before,"brands").iter().any(|old|field(old,"id")==id&&old==brand){
            continue
        }
        if id.is_empty() || field(brand,"name").trim().is_empty(){
            return Err("브랜드 공식명을 입력하세요.".into())
        }
        let evidence=field(brand,"evidenceUrl").trim();
        let valid=Url::parse(evidence).is_ok_and(|url|
            url.scheme()=="https" && url.host_str().is_some() &&
            url.username().is_empty() && url.password().is_none());
        if !valid{return Err("브랜드 확인 근거는 HTTPS URL이어야 합니다.".into())}
        let own=terms(brand);
        for (index,other) in brands.iter().enumerate(){
            if index==position{continue}
            if field(other,"id")!=id {
                let other_terms=terms(other);
                if own.iter().any(|name|other_terms.contains(name)){
                    return Err("브랜드 공식명, 한글명 또는 검색 별칭이 다른 브랜드와 중복됩니다.".into())
                }
            }
        }
        // The original TS duplicate-ID check runs *after* alias collisions.
        if brands.iter().enumerate().any(|(index,other)|
            index!=position && field(other,"id")==id){
            return Err("브랜드 식별자가 중복되었습니다.".into())
        }
    }
    Ok(())
}
fn upsert_brand(state:&Value,payload:&Value)->Result<Value,String>{
    let edit_id=field(payload,"editing");
    let id=if edit_id.is_empty(){field(payload,"id")}else{edit_id};
    if id.is_empty(){return Err("브랜드 ID가 없습니다.".into())}
    let name=field(payload,"name").trim();
    let korean=field(payload,"korean").trim();
    let evidence=field(payload,"evidence").trim();
    // Original React Map(registryKey -> last written alias) retains the
    // order of the first occurrence for repeated normalized aliases.
    let mut aliases=Vec::<String>::new();
    let mut positions=HashMap::<String,usize>::new();
    // Native block editors send an array; preserve commas inside a block.
    // Legacy callers still send their comma-separated string.
    let raw_aliases:Vec<&str>=match payload.get("aliases"){
        Some(Value::Array(values))=>values.iter().map(|value|value.as_str()
            .ok_or("검색 별칭은 텍스트여야 합니다.")).collect::<Result<Vec<_>,_>>()?,
        _=>field(payload,"aliases").split(',').collect(),
    };
    for alias in raw_aliases{
        let alias=alias.trim();
        if alias.is_empty(){continue}
        let key=registry_key(alias);
        if let Some(position)=positions.get(&key).copied(){
            aliases[position]=alias.to_owned();
        }else{
            positions.insert(key,aliases.len());
            aliases.push(alias.to_owned());
        }
    }
    let row=json!({
        "id":id,"name":name,"korean":korean,
        "aliases":aliases,"evidenceUrl":evidence
    });
    let mut next=state.clone();
    let mut brands=items(state,"brands").iter()
        .filter(|b|edit_id.is_empty()||field(b,"id")!=edit_id)
        .cloned().collect::<Vec<_>>();
    brands.push(row);
    next["brands"]=json!(brands);
    validate(state,&next)?;
    Ok(json!({"state":next}))
}
// Deletion is deliberately restricted to unreferenced brands. Do not
// implicitly change a product's identity or its historical data.
fn delete_brand(state:&Value,payload:&Value)->Result<Value,String>{
    let id=field(payload,"brandId");
    if id.trim().is_empty(){return Err("삭제할 브랜드를 확인할 수 없습니다.".into());}
    let brands=state["brands"].as_array().ok_or("브랜드 목록이 없습니다.")?;
    let index=brands.iter().position(|row|field(row,"id")==id)
        .ok_or("삭제할 브랜드를 찾을 수 없습니다.")?;
    if brands.iter().filter(|row|field(row,"id")==id).count()!=1{
        return Err("브랜드 식별자가 중복되어 삭제할 수 없습니다.".into());
    }
    let linked=state["products"].as_array().ok_or("상품 목록이 없습니다.")?
        .iter().filter(|product|field(product,"brandId")==id).count();
    if linked>0{
        return Err(format!("연결된 상품 {linked}개가 있습니다. 브랜드 연결을 변경한 후 삭제하세요."));
    }
    let mut next=state.clone();
    next["brands"].as_array_mut().ok_or("브랜드 목록이 없습니다.")?.remove(index);
    validate(state,&next)?;
    Ok(json!({"state":next}))
}
pub(super) fn execute(operation:&str,payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("현재 상태가 없습니다.")?;
    match operation{
        "upsert_brand"=>upsert_brand(state,payload),
        "delete_brand"=>delete_brand(state,payload),
        "validate_brands"=>{
            validate(payload.get("before").unwrap_or(&Value::Null),state)?;
            Ok(json!({"valid":true}))
        }
        _=>Err(format!("지원하지 않는 브랜드 업무입니다: {operation}"))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn brand_delete_only_removes_the_selected_unreferenced_brand(){
        let state=json!({"brands":[{"id":"b"},{"id":"other"}],
            "products":[{"id":"p","brandId":"other","priceHistory":[{"at":"old"}]}],
            "aliases":[{"id":"a","productId":"p"}],"orders":[{"id":"o"}],
            "workflow":{"shipments":[{"id":"s"}]}});
        let next=execute("delete_brand",&json!({"state":state,"brandId":"b"})).unwrap();
        let mut expected=state.clone();
        expected["brands"]=json!([{"id":"other"}]);
        assert_eq!(next["state"],expected);
        for id in ["","missing","other"]{
            assert!(execute("delete_brand",&json!({"state":state,"brandId":id})).is_err());
        }
    }
    #[test]
    fn brand_delete_blocks_even_archived_and_superseded_product_references(){
        for product in [json!({"id":"p","brandId":"b","active":false}),
            json!({"id":"p","brandId":"b","deletedAt":"old"}),
            json!({"id":"p","brandId":"b","supersededBy":"new"})]{
            let state=json!({"brands":[{"id":"b"}],"products":[product]});
            assert!(execute("delete_brand",&json!({"state":state,"brandId":"b"}))
                .unwrap_err().contains("연결된 상품 1개"));
        }
    }
    #[test]
    fn block_aliases_preserve_literal_commas_and_normalized_duplicates(){
        let state=json!({"brands":[]});
        let result=upsert_brand(&state,&json!({
            "id":"b","name":"BANDO","aliases":["A, B","a, b","C"],
            "evidence":"https://example.com/brand"
        })).unwrap();
        assert_eq!(result["state"]["brands"][0]["aliases"],json!(["a, b","C"]));
    }
    #[test]
    fn brand_aliases_are_deduplicated_and_other_product_references_survive(){
        let state=json!({"brands":[],"products":[{"id":"p1","brandId":"b1"}],
            "orders":[{"id":"old","total":20000}]});
        let r=upsert_brand(&state,&json!({
            "id":"b1","name":" BANDO ","korean":"반도",
            "aliases":"스탠드, STAND, 스탠드","evidence":"https://example.com/brand"
        })).unwrap();
        assert_eq!(r["state"]["brands"][0]["name"],"BANDO");
        assert_eq!(r["state"]["brands"][0]["aliases"],json!(["스탠드","STAND"]));
        assert_eq!(r["state"]["products"][0]["brandId"],"b1");
        assert_eq!(r["state"]["orders"][0]["total"],20000);
    }
}
