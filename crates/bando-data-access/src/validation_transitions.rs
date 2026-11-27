//! Read-only transition checks for the ten previously unbound requirements.
//! No mutations, clocks, SQL, or command replay. Existing command-specific
//! guards remain authoritative and must not be removed by this stage.
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};
use crate::product_commands::catalog_clients as catalog;
type CheckResult=Result<(),String>;
fn text<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn list(v:&Value)->&[Value]{v.as_array().map(Vec::as_slice).unwrap_or(&[])}
fn find<'a>(v:&'a Value,id:&str)->Option<&'a Value>{list(v).iter().find(|r|text(r,"id")==id)}
fn archived(p:&Value)->bool{!text(p,"deletedAt").is_empty()||!text(p,"supersededBy").is_empty()}
fn changed_ids<'a>(before:&'a Value,next:&'a Value)->CheckResult{
    if before==next{return Ok(());}
    if !next.is_array(){return Err("업무 목록 형식 오류".into());}
    let mut ids=BTreeSet::new();
    for row in list(next){
        let id=text(row,"id");
        if id.trim().is_empty()||!ids.insert(id){return Err("업무 ID 누락 또는 중복".into());}
    }
    Ok(())
}
fn append_only<'a>(old:&Value,new:&'a Value)->Result<&'a [Value],String>{
    if old==new{return Ok(&[]);}
    let old=if old.is_null(){&[][..]}else{old.as_array().ok_or("기존 이력 형식 오류")?.as_slice()};
    let new=new.as_array().ok_or("변경 이력 형식 오류")?;
    if !new.starts_with(old){return Err("기존 이력을 수정하거나 삭제할 수 없습니다.".into());}
    Ok(&new[old.len()..])
}
fn changed_product_rows<'a>(before:&Value,next:&'a Value)->impl Iterator<Item=&'a Value>{
    let changed:BTreeSet<String>=list(&next["products"]).iter()
        .filter(|p|find(&before["products"],text(p,"id"))!=Some(*p))
        .map(|p|text(p,"id").to_owned()).collect();
    list(&next["products"]).iter().filter(move|p|changed.contains(text(p,"id")))
}
pub(super) fn product(before:&Value,next:&Value)->CheckResult{
    changed_ids(&before["products"],&next["products"])?;
    for p in changed_product_rows(before,next).filter(|p|!archived(p)){
        let prior=find(&before["products"],text(p,"id"));
        catalog::validate_product_fields(next,p,prior.is_none())?;
    }
    Ok(())
}
pub(super) fn price(before:&Value,next:&Value)->CheckResult{
    for p in changed_product_rows(before,next).filter(|p|!archived(p)){
        let prior=find(&before["products"],text(p,"id"));
        for v in list(&p["variants"]){
            let old=prior.and_then(|p|find(&p["variants"],text(v,"id")));
            for field in ["wholesale","semi","retail","officialRetail"]{
                if old.is_some_and(|o|o[field]==v[field]){continue;}
                if !catalog::valid_money(&v[field]){return Err("상품 가격은 0 이상의 안전한 원 단위 정수여야 합니다.".into());}
            }
        }
        if prior.is_none_or(|old|old["baseRetail"]!=p["baseRetail"])&&!catalog::valid_money(&p["baseRetail"]){
            return Err("기본 소매가 금액 오류".into());
        }
    }
    Ok(())
}
pub(super) fn price_history(before:&Value,next:&Value)->CheckResult{
    if before["products"]==next["products"]{return Ok(());}
    for old in list(&before["products"]){
        let Some(p)=find(&next["products"],text(old,"id"))else{
            if !list(&old["priceHistory"]).is_empty(){return Err("가격 이력을 가진 상품을 제거할 수 없습니다.".into());}
            continue;
        };
        if old==p{continue;}
        let added=append_only(&old["priceHistory"],&p["priceHistory"])?;
        let mut expected=BTreeMap::new();
        for v in list(&p["variants"]){
            let Some(prior)=find(&old["variants"],text(v,"id"))else{continue;};
            for tier in ["wholesale","semi","retail"]{
                if prior[tier]!=v[tier]{
                    expected.insert(format!("{}.{tier}",text(v,"id")),(&prior[tier],&v[tier]));
                }
            }
        }
        for entry in added{
            let key=text(entry,"field");
            let Some((a,b))=expected.remove(key)else{return Err("실제 가격 변경과 일치하지 않는 이력".into());};
            if entry["before"]!=*a||entry["after"]!=*b||text(entry,"at").is_empty()||text(entry,"source").is_empty(){
                return Err("가격 변경 이력의 전후 금액 또는 기록 정보 오류".into());
            }
        }
        if !expected.is_empty(){return Err("실제 가격 변경의 이력이 누락되었습니다.".into());}
    }
    // New product registration may have an empty or absent history; no
    // prior persisted price exists. Imported historical rows need a
    // separately reviewed migration policy.
    for p in list(&next["products"]){
        if find(&before["products"],text(p,"id")).is_none()&&!p["priceHistory"].is_null()&&
            p["priceHistory"].as_array().is_none_or(|h|!h.is_empty()){
            return Err("신규 상품의 기존 가격 이력은 별도 확인이 필요합니다.".into());
        }
    }
    Ok(())
}
pub(super) fn brand_link(before:&Value,next:&Value)->CheckResult{
    for p in list(&next["products"]){
        let id=text(p,"brandId");if id.is_empty(){continue;}
        if find(&next["brands"],id).is_some(){continue;}
        let existed=find(&before["products"],text(p,"id")).is_some_and(|o|text(o,"brandId")==id);
        if !existed||find(&before["brands"],id).is_some(){return Err("등록되지 않은 브랜드 연결".into());}
    }
    Ok(())
}
pub(super) fn alias(before:&Value,next:&Value)->CheckResult{
    changed_ids(&before["aliases"],&next["aliases"])?;
    if before["aliases"]==next["aliases"]&&before["products"]==next["products"]&&before["clients"]==next["clients"]{return Ok(());}
    for row in list(&next["aliases"]){
        let prior=find(&before["aliases"],text(row,"id"));
        let old=prior.map(|r|catalog::alias_diagnostics(before,r)).unwrap_or_default();
        for problem in catalog::alias_diagnostics(next,row){
            if prior!=Some(row)||!old.contains(&problem){return Err(problem);}
        }
        let cid=text(row,"clientId");
        if !cid.is_empty()&&find(&next["clients"],cid).is_none()&&
            (prior.is_none_or(|p|text(p,"clientId")!=cid)||find(&before["clients"],cid).is_some()){
            return Err("별칭의 거래처 연결이 없습니다.".into());
        }
    }
    Ok(())
}
fn active_group<'a>(state:&'a Value,id:&str)->Option<&'a Value>{
    find(&state["shipping"]["groups"],id).filter(|g|g["deletedAt"].is_null())
}
fn rate(rule:&Value)->CheckResult{
    crate::shipping_action::validate_rate_input(rule)?;
    if !catalog::valid_money(&rule["perItem"]){return Err("개별 운임 금액 오류".into());}
    if let Some(max)=rule["maxBundle"].as_i64(){
        if rule["fees"].as_object().is_some_and(|fees|fees.iter().any(|(n,v)|
            !v.is_null()&&n.parse::<i64>().is_ok_and(|n|n>max))){
            return Err("최대 묶음 수량을 초과한 운임 구간".into());
        }
    }
    if text(rule,"version").is_empty()||text(rule,"at").is_empty(){return Err("운임 버전 또는 기록 시각 누락".into());}
    Ok(())
}
pub(super) fn shipping(before:&Value,next:&Value)->CheckResult{
    let a=&before["shipping"];let b=&next["shipping"];
    if a!=b{
        if !b.is_object(){return Err("운임 정보 형식 오류".into());}
        changed_ids(&a["groups"],&b["groups"])?;
        changed_ids(&a["combinations"],&b["combinations"])?;
        if !b["productRules"].is_object()||!b["productHistory"].is_object(){return Err("개별 운임 목록 형식 오류".into());}
        for group in list(&b["groups"]){
            let prior=find(&a["groups"],text(group,"id"));
            if prior!=Some(group)&&text(group,"name").trim().is_empty(){return Err("배송 그룹명 누락".into());}
            if prior.is_none_or(|old|old["current"]!=group["current"]){rate(&group["current"])?;}
        }
        for (id,rule) in b["productRules"].as_object().ok_or("개별 운임 목록 오류")?{
            if a["productRules"][id]!=*rule{
                if find(&next["products"],id).is_none(){return Err("개별 운임 상품 연결 오류".into());}
                rate(rule)?;
            }
        }
        for record in list(&b["combinations"]){
            if find(&a["combinations"],text(record,"id"))!=Some(record)&&
                (text(record,"key").is_empty()||!record["charge"].is_number()||
                !catalog::valid_money(&record["charge"])||!catalog::valid_money(&record["actual"])){
                return Err("조합 운임 정보 오류".into());
            }
        }
    }
    for p in list(&next["products"]).iter().filter(|p|!archived(p)){
        let id=text(p,"shippingGroupId");
        if id.is_empty()||active_group(next,id).is_some(){continue;}
        let old=find(&before["products"],text(p,"id"));
        if old.is_none_or(|o|text(o,"shippingGroupId")!=id)||active_group(before,id).is_some(){
            return Err("상품 배송 그룹 연결 오류".into());
        }
    }
    Ok(())
}
fn rate_history(old:&Value,new:&Value,history_old:&Value,history_new:&Value)->CheckResult{
    let added=append_only(history_old,history_new)?;
    if old!=new&&!old.is_null(){
        if added.first()!=Some(old)||text(old,"version")==text(new,"version"){
            return Err("변경 전 운임 또는 신규 버전이 누락되었습니다.".into());
        }
    }
    Ok(())
}
pub(super) fn shipping_history(before:&Value,next:&Value)->CheckResult{
    let a=&before["shipping"];let b=&next["shipping"];if a==b{return Ok(());}
    for group in list(&a["groups"]){
        let new=find(&b["groups"],text(group,"id")).ok_or("기존 배송 그룹 이력 삭제")?;
        rate_history(&group["current"],&new["current"],&group["history"],&new["history"])?;
    }
    if let Some(rules)=a["productRules"].as_object(){
        for (id,old) in rules{
            rate_history(old,&b["productRules"][id],&a["productHistory"][id],&b["productHistory"][id])?;
        }
    }
    if let Some(histories)=a["productHistory"].as_object(){
        for (id,history) in histories{append_only(history,&b["productHistory"][id])?;}
    }
    for record in list(&a["combinations"]){
        let new=find(&b["combinations"],text(record,"id")).ok_or("기존 운임 조합 이력 삭제")?;
        // Operational observations and retirement are legitimate updates.
        for key in ["key","epoch","versions","charge","actual","at","description"]{
            if record[key]!=new[key]{return Err("기존 운임 조합 기준을 덮어쓸 수 없습니다.".into());}
        }
    }
    Ok(())
}
pub(super) fn factory_reply(before:&Value,next:&Value)->CheckResult{
    let a=&before["factoryReplies"];let b=&next["factoryReplies"];
    if a==b{return Ok(());}
    changed_ids(a,b)?;
    let added=append_only(a,b)?;
    for record in added{
        if !matches!(text(record,"status"),"APPLIED"|"PARTIAL"|"REVIEW")||!record["rows"].is_array(){
            return Err("공장 회신 상태 또는 행 목록 오류".into());
        }
        if !record["resolves"].is_null(){
            let origin=find(a,text(&record["resolves"],"id")).ok_or("원본 공장 회신 누락")?;
            if !list(&origin["rows"]).iter().any(|r|r["row"]==record["resolves"]["row"]){
                return Err("원본 공장 회신 행 누락".into());
            }
        }
        let mut claimed=BTreeSet::new();
        for row in list(&record["rows"]).iter().filter(|r|r["applied"]==true){
            let ship=text(row,"shipmentId");
            let tracking=text(row,"tracking");
            if ship.is_empty()||!claimed.insert(ship)||!(8..=30).contains(&tracking.len())||
                !tracking.bytes().all(|b|b.is_ascii_digit())||!row["actual"].is_number()||
                !catalog::valid_money(&row["actual"]){
                return Err("반영된 공장 회신 행의 배송/송장/운임 오류".into());
            }
            let target=find(&next["workflow"]["shipments"],ship).ok_or("공장 회신 배송 연결 오류")?;
            if target["orderId"]!=row["orderId"]||target["tracking"]["FACTORY"]!=row["tracking"]||
                target["actualFreight"]["FACTORY"]!=row["actual"]{
                return Err("공장 회신 반영값과 배송 기록 불일치".into());
            }
        }
    }
    Ok(())
}
pub(super) fn registration(before:&Value,next:&Value)->CheckResult{
    let a=&before["registrationBatches"];let b=&next["registrationBatches"];
    if a!=b{
        changed_ids(a,b)?;
        let added=append_only(a,b)?;
        let mut claimed=BTreeSet::new();
        for batch in added{
            if text(batch,"at").is_empty()||!batch["rows"].is_array()||!batch["createdIds"].is_array(){
                return Err("상품 등록 기록 형식 오류".into());
            }
            for id in list(&batch["createdIds"]){
                let id=id.as_str().filter(|s|!s.is_empty()).ok_or("등록 상품 ID 오류")?;
                if !claimed.insert(id)||find(&before["products"],id).is_some()||find(&next["products"],id).is_none(){
                    return Err("상품 등록 기록과 신규 상품 불일치".into());
                }
            }
        }
    }
    if before["deletedProductKeys"]!=next["deletedProductKeys"]{
        let keys=next["deletedProductKeys"].as_array().ok_or("삭제 상품 기록 형식 오류")?;
        let mut seen=BTreeSet::new();
        for key in keys{
            if !key.as_str().is_some_and(|k|!k.is_empty()&&seen.insert(k)){return Err("삭제 상품 기록 오류".into());}
        }
        if list(&before["deletedProductKeys"]).iter().any(|k|!keys.contains(k)){return Err("기존 삭제 상품 기록 제거".into());}
    }
    Ok(())
}
pub(super) fn mapping(before:&Value,next:&Value)->CheckResult{
    for key in ["productMappings","variantMappings"]{
        let a=&before[key];let b=&next[key];
        if a!=b&&!b.is_object(){return Err("상품 매핑 목록 형식 오류".into());}
        let Some(map)=b.as_object()else{continue;};
        for (source,target) in map{
            if key=="variantMappings"{
                // No current native writer/consumer for this legacy map
                // was identified; do not invent a semantic mapping rule.
                if a[source]!=*target{return Err("기존 옵션 매핑 변경은 별도 규격 확인이 필요합니다.".into());}
                continue;
            }
            let valid=|state:&Value,value:&Value|->bool{
                let Some((pid,vid))=source.split_once('|')else{return false;};
                let Some(dest)=value.as_str().and_then(|id|find(&state["products"],id))else{return false;};
                !pid.is_empty()&&!vid.is_empty()&&text(dest,"sourceProductId")==pid&&
                    list(&dest["variants"]).len()==1&&text(&dest["variants"][0],"id")==vid
            };
            if !valid(next,target)&&(a[source]!=*target||valid(before,&a[source])){
                return Err("과거 상품 매핑 대상 오류".into());
            }
        }
        // The existing migration appends mappings, never removes them.
        if let Some(old)=a.as_object(){
            if old.keys().any(|k|!map.contains_key(k)){return Err("기존 상품 매핑 제거".into());}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test] fn product_and_price_reject_bad_candidate(){
        let a=json!({"products":[]});
        let b=json!({"products":[{"id":"p","variants":[{"id":"v","retail":-1}]}]});
        assert!(product(&a,&b).is_err());assert!(price(&a,&b).is_err());
    }
    #[test] fn price_history_matches_actual_delta_and_is_immutable(){
        let a=json!({"products":[{"id":"p","variants":[{"id":"v","retail":1}],"priceHistory":[{"old":true}]}]});
        let mut b=a.clone();b["products"][0]["variants"][0]["retail"]=json!(2);
        assert!(price_history(&a,&b).is_err());
        b["products"][0]["priceHistory"].as_array_mut().unwrap().push(json!({"field":"v.retail","before":1,"after":2,"at":"now","source":"MANUAL"}));
        assert!(price_history(&a,&b).is_ok());
        b["products"][0]["priceHistory"][0]=json!({});assert!(price_history(&a,&b).is_err());
    }
    #[test] fn brand_deletion_and_alias_target_deletion_are_detected(){
        let a=json!({"products":[{"id":"p","brandId":"b","name":"P"}],"brands":[{"id":"b"}],"aliases":[{"id":"a","text":"별칭","productId":"p"}]});
        let mut b=a.clone();b["brands"]=json!([]);assert!(brand_link(&a,&b).is_err());
        b["products"]=json!([]);assert!(alias(&a,&b).is_err());
    }
    #[test] fn shipping_requires_valid_rate_and_previous_version(){
        let a=json!({"shipping":{"groups":[{"id":"g","name":"G","current":{"version":"a","at":"t","fees":{"1":1},"maxBundle":1},"history":[]}],"combinations":[],"productRules":{},"productHistory":{}}});
        let mut b=a.clone();b["shipping"]["groups"][0]["current"]["fees"]["1"]=json!(-1);
        assert!(shipping(&a,&b).is_err());assert!(shipping_history(&a,&b).is_err());
        b["shipping"]["groups"][0]["current"]=json!({"version":"b","at":"u","fees":{"1":2},"maxBundle":1});
        b["shipping"]["groups"][0]["history"]=json!([a["shipping"]["groups"][0]["current"]]);
        assert!(shipping(&a,&b).is_ok());assert!(shipping_history(&a,&b).is_ok());
    }
    #[test] fn factory_and_registration_history_cannot_be_rewritten(){
        let a=json!({"factoryReplies":[{"id":"f"}],"registrationBatches":[{"id":"r"}]});
        let b=json!({"factoryReplies":[],"registrationBatches":[]});
        assert!(factory_reply(&a,&b).is_err());assert!(registration(&a,&b).is_err());
    }
    #[test] fn mapping_must_reference_exact_migrated_sale_unit(){
        let a=json!({"products":[],"productMappings":{}});
        let mut b=json!({"products":[{"id":"p","sourceProductId":"old","variants":[{"id":"v"}]}],"productMappings":{"old|v":"p"}});
        assert!(mapping(&a,&b).is_ok());
        b["products"][0]["variants"][0]["id"]=json!("other");assert!(mapping(&a,&b).is_err());
    }
}
