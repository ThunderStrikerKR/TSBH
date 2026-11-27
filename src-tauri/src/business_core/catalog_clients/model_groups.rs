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
type Finding=(Vec<String>,Value,String);

pub(super) fn validate(state:&Value)->Result<(),String>{
    diagnostics(state).into_iter().next().map_or(Ok(()),|(_,_,message)|Err(message))
}

/// Enumerate group/member/pair-specific defects instead of stopping at the
/// first group. The strict command still returns the first finding.
pub(super) fn diagnostics(state:&Value)->Vec<Finding>{
    let mut out=Vec::new();
    let Some(groups)=state["settings"]["colorGuidanceGroups"].as_object()else{return out;};
    let mut names:std::collections::HashMap<String,Vec<String>>=std::collections::HashMap::new();
    let mut used:std::collections::HashMap<String,Vec<String>>=std::collections::HashMap::new();
    for (id,group) in groups{
        let name=field(group,"name").trim();
        let key=|rule:&str|if id.trim().is_empty(){vec![]}else{vec![rule.to_owned(),id.clone()]};
        if id.trim().is_empty()||name.is_empty(){
            out.push((key("name"),group["name"].clone(),"모델군 이름을 입력하세요.".into()));
        }
        let name_key=name.to_lowercase();
        let peers=names.entry(name_key.clone()).or_default();
        for prior in peers.iter(){
            let identity=if prior.trim().is_empty()||id.trim().is_empty(){vec![]}else{vec!["name-conflict".into(),prior.clone(),id.clone()]};
            out.push((identity,json!(name_key),"모델군 이름이 중복됩니다.".into()));
        }
        peers.push(id.clone());
        let members=items(group,"productIds");
        let rep=field(group,"representativeProductId");
        let unique=members.iter().filter_map(Value::as_str).collect::<HashSet<_>>();
        if !rep.is_empty(){
            if members.len()<2{
                out.push((key("member-count"),group["productIds"].clone(),"모델군에는 등록된 상품이 2개 이상 있어야 합니다.".into()));
            }
            if unique.len()!=members.len(){
                out.push((key("member-identity"),group["productIds"].clone(),"모델군에 같은 상품을 중복 등록할 수 없습니다.".into()));
            }
            for member in members{
                let Some(pid)=member.as_str()else{
                    out.push((vec![],Value::Null,"등록된 독립 판매 상품만 모델군에 추가할 수 있습니다.".into()));continue;
                };
                let candidates=items(state,"products").iter().filter(|p|field(p,"id")==pid).collect::<Vec<_>>();
                if !candidates.iter().any(|p|eligible_product(p)){
                    // All inputs of eligible_product; unrelated prices/name
                    // are not copied into the evidence.
                    let evidence=candidates.iter().map(|p|json!({
                        "active":p["active"],"deletedAt":p["deletedAt"],"supersededBy":p["supersededBy"],
                        "catalog":p["catalog"],"sources":items(p,"variants").iter().map(|v|&v["source"]).collect::<Vec<_>>()
                    })).collect::<Vec<_>>();
                    let identity=if id.trim().is_empty()||pid.trim().is_empty()||candidates.len()>1{vec![]}else{vec!["ineligible-member".into(),id.clone(),pid.into()]};
                    out.push((identity,json!(evidence),"등록된 독립 판매 상품만 모델군에 추가할 수 있습니다.".into()));
                }
            }
            if !members.iter().any(|v|v.as_str()==Some(rep)){
                out.push((key("representative"),json!([rep,group["productIds"]]),"모델군에 등록된 상품 중 하나를 대표모델로 지정하세요.".into()));
            }
        }else{
            if field(group,"representativeColor").is_empty(){
                out.push((key("legacy-representative"),json!([group["representativeProductId"],group["representativeColor"]]),"기존 그룹도 대표모델을 직접 선택한 뒤 저장하세요.".into()));
            }
            if members.len()<2||unique.len()!=members.len(){
                out.push((key("legacy-members"),group["productIds"].clone(),"기존 그룹의 상품 연결을 확인하세요.".into()));
            }
        }
        for member in members{
            let pid=member.as_str().unwrap_or("");
            let previous=used.entry(pid.to_owned()).or_default();
            for prior in previous.iter(){
                let identity=if pid.is_empty()||prior==id||prior.trim().is_empty()||id.trim().is_empty(){vec![]}else{vec!["member-conflict".into(),prior.clone(),id.clone(),pid.into()]};
                out.push((identity,Value::Null,"상품은 하나의 모델군에만 등록할 수 있습니다.".into()));
            }
            previous.push(id.clone());
        }
    }
    out
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
