//! Exact original TS model-group validation and explicit representative picker.
//! Oracle: src/model-groups.ts::validateModelGroups / validateModelGroupProducts.
use serde_json::{json,Value};
use std::collections::HashSet;
use super::{field,items};

fn registered(p:&Value)->bool{
    p.get("active")!=Some(&Value::Bool(false)) &&
    field(p,"deletedAt").is_empty() &&
    field(p,"supersededBy").is_empty()
}
fn eligible_product(p:&Value)->bool{
    registered(p) && field(p,"catalog")!="DEMO" &&
    items(p,"variants").len()==1 &&
    items(p,"variants").iter().all(|v|field(v,"source")!="DEMO")
}
fn validate_product_ids(state:&Value,ids:&[Value],rep:&str)->Result<(),String>{
    if ids.len()<2{
        return Err("모델군에는 등록된 상품이 2개 이상 있어야 합니다.".into())
    }
    let unique=ids.iter().filter_map(Value::as_str).collect::<HashSet<_>>();
    if unique.len()!=ids.len(){
        return Err("모델군에 같은 상품을 중복 등록할 수 없습니다.".into())
    }
    let all=items(state,"products");
    if ids.iter().any(|id|{
        let Some(id)=id.as_str()else{return true};
        !all.iter().any(|p|field(p,"id")==id && eligible_product(p))
    }){
        return Err("등록된 독립 판매 상품만 모델군에 추가할 수 있습니다.".into())
    }
    if rep.is_empty()||!ids.iter().any(|v|v.as_str()==Some(rep)){
        return Err("모델군에 등록된 상품 중 하나를 대표모델로 지정하세요.".into())
    }
    Ok(())
}
pub(super) fn validate(state:&Value)->Result<(),String>{
    let Some(groups)=state.get("settings").and_then(|s|s.get("colorGuidanceGroups"))
        .filter(|v|v.is_object()) else{return Ok(())};
    let mut names=HashSet::new();
    let mut used=HashSet::new();
    for (id,group) in groups.as_object().unwrap(){
        let name=field(group,"name").trim();
        if id.trim().is_empty()||name.is_empty(){
            return Err("모델군 이름을 입력하세요.".into())
        }
        let key=name.to_lowercase();
        if !names.insert(key){return Err("모델군 이름이 중복됩니다.".into())}
        let members=items(group,"productIds");
        if !field(group,"representativeProductId").is_empty(){
            validate_product_ids(state,members,field(group,"representativeProductId"))?;
        }else{
            if field(group,"representativeColor").is_empty(){
                return Err("기존 그룹도 대표모델을 직접 선택한 뒤 저장하세요.".into())
            }
            if members.len()<2 ||
                members.iter().filter_map(Value::as_str).collect::<HashSet<_>>().len()!=members.len(){
                return Err("기존 그룹의 상품 연결을 확인하세요.".into())
            }
        }
        for member in members {
            let id=member.as_str().unwrap_or("");
            if !used.insert(id.to_owned()){
                return Err("상품은 하나의 모델군에만 등록할 수 있습니다.".into())
            }
        }
    }
    Ok(())
}
pub(super) fn execute(operation:&str,payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("현재 상태가 없습니다.")?;
    match operation{
        "validate_model_groups"=>{super::validate_model_groups(state)?;Ok(json!({"valid":true}))},
        "eligible_model_group_product_ids"=>{
            Ok(json!(items(state,"products").iter()
                .filter(|p|eligible_product(p))
                .map(|p|field(p,"id"))
                .filter(|id|!id.is_empty()).collect::<Vec<_>>()))
        },
        "validate_model_group_products"=>{
            validate_product_ids(state,items(payload,"productIds"),
                field(payload,"representativeProductId"))?;
            Ok(json!({"valid":true}))
        }
        _=>Err(format!("지원하지 않는 모델군 검증입니다: {operation}"))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn eligible_ids_share_exact_validator_rules_and_exclude_demo_or_multi_sale(){
        let state=json!({"products":[
            {"id":"ok","active":true,"catalog":"BANDOSTAND",
                "variants":[{"id":"v1","source":"MANUAL"}]},
            {"id":"demo","active":true,"catalog":"DEMO",
                "variants":[{"id":"v2","source":"DEMO"}]},
            {"id":"two","active":true,"catalog":"BANDOSTAND",
                "variants":[{"id":"a","source":"MANUAL"},{"id":"b","source":"MANUAL"}]},
            {"id":"deleted","active":false,"catalog":"BANDOSTAND",
                "variants":[{"id":"v3","source":"MANUAL"}]}
        ]});
        let ids=execute("eligible_model_group_product_ids",&json!({"state":state.clone()})).unwrap();
        assert_eq!(ids,json!(["ok"]));
        let bad=validate_product_ids(&state,&[json!("ok"),json!("demo")],"ok");
        assert!(bad.is_err());
    }
    #[test]
    fn two_products_of_same_color_but_distinct_prices_can_group(){
        let state=json!({"products":[
            {"id":"p1","active":true,"variants":[{"id":"v1","source":"MANUAL","color":"Black","retail":10000}]},
            {"id":"p2","active":true,"variants":[{"id":"v2","source":"MANUAL","color":"Black","retail":30000}]}
        ],"settings":{"colorGuidanceGroups":{"g":{"name":"709",
            "productIds":["p1","p2"],"representativeProductId":"p1"}}}});
        assert!(validate(&state).is_ok());
    }
    #[test]
    fn model_group_requires_two_real_products_and_explicit_representative(){
        let mut state=json!({"products":[
            {"id":"p1","active":true,"variants":[{"id":"v1","source":"MANUAL"}]},
            {"id":"p2","active":true,"variants":[{"id":"v2","source":"MANUAL"}]}
        ],"settings":{"colorGuidanceGroups":{"g":{
            "name":"709","productIds":["p1","p2"],
            "representativeProductId":""
        }}}});
        assert_eq!(validate(&state).unwrap_err(),
            "기존 그룹도 대표모델을 직접 선택한 뒤 저장하세요.");
        state["settings"]["colorGuidanceGroups"]["g"]["representativeProductId"]=json!("p3");
        assert_eq!(validate(&state).unwrap_err(),
            "모델군에 등록된 상품 중 하나를 대표모델로 지정하세요.");
        state["settings"]["colorGuidanceGroups"]["g"]["representativeProductId"]=json!("p1");
        assert!(validate(&state).is_ok());
        state["settings"]["colorGuidanceGroups"]["g"]["productIds"]=json!(["p1"]);
        assert_eq!(validate(&state).unwrap_err(),
            "모델군에는 등록된 상품이 2개 이상 있어야 합니다.");
    }
    #[test]
    fn model_group_rejects_multiple_memberships_without_modifying_products(){
        let state=json!({"products":[
            {"id":"p1","active":true,"variants":[{"id":"v1","source":"MANUAL"}]},
            {"id":"p2","active":true,"variants":[{"id":"v2","source":"MANUAL"}]},
            {"id":"p3","active":true,"variants":[{"id":"v3","source":"MANUAL"}]}
        ],"settings":{"colorGuidanceGroups":{
            "g1":{"name":"709","productIds":["p1","p2"],"representativeProductId":"p1"},
            "g2":{"name":"710","productIds":["p2","p3"],"representativeProductId":"p3"}
        }}});
        let before=state["products"].clone();
        assert_eq!(validate(&state).unwrap_err(),
            "상품은 하나의 모델군에만 등록할 수 있습니다.");
        assert_eq!(state["products"],before);
    }
}
