//! Native application of a Rust-recognized reception to the existing employee draft.
//! Port of src/orders/rust-reception.ts::applyRustReceptionAnalysis:
//! preserve user-confirmed fields, stable IDs, product corrections and evidence.
use serde_json::{json, Value};
use std::collections::HashSet;
use super::{field, has_value, req, required_str, rows, set, str_field};

fn clone_field(target:&mut Value, source:&Value, key:&str)->Result<(),String>{
    if let Some(value)=source.get(key){set(target,key,value.clone())?;}
    else { target.as_object_mut().ok_or("주문 데이터 형식 오류")?.remove(key); }
    Ok(())
}
fn names(row:&Value)->Vec<String>{
    let mut seen=HashSet::new();
    rows(field(row,"sources")).iter()
        .filter(|s|str_field(s,"field")=="name")
        .map(|s|str_field(s,"text").trim().to_string())
        .filter(|name|seen.insert(name.clone()))
        .collect()
}
fn same_identity(incoming:&Value, old:&Value)->bool{
    let a=names(incoming);
    let b=names(old);
    if a.len()==1 && b.len()==1 {return a[0]==b[0];}
    if a.is_empty() && b.is_empty(){
        return ["address","mobile","phone"].iter().all(|key| field(incoming,key)==field(old,key));
    }
    false
}
fn match_recipient<'a>(incoming:&Value, previous:&'a [Value],
    used:&HashSet<String>, incoming_rows:&[Value])->Option<&'a Value>{
    let eligible=previous.iter().filter(|r|!used.contains(str_field(r,"id")))
        .collect::<Vec<_>>();
    let anchor=field(field(incoming,"evidence"),"anchorId");
    let exact=eligible.iter().copied().filter(|r|
        !anchor.is_null() && anchor.as_str().is_some_and(|s|!s.is_empty()) &&
        field(field(r,"evidence"),"anchorId")==anchor &&
        same_identity(incoming,r)
    ).collect::<Vec<_>>();
    if exact.len()==1 {return Some(exact[0]);}
    let first=rows(field(incoming,"sources")).iter()
        .find(|s|str_field(s,"field")=="name");
    let by_source=eligible.iter().copied().filter(|r|{
        let Some(name)=first else{return false};
        let source_match=rows(field(r,"sources")).iter().any(|s|
            str_field(s,"field")=="name" &&
            field(s,"start")==field(name,"start") &&
            field(s,"text")==field(name,"text"));
        let distinct=rows(field(r,"sources")).iter()
            .filter(|s|str_field(s,"field")=="name")
            .map(|s|str_field(s,"text").to_string()).collect::<HashSet<_>>().len();
        source_match && distinct==1
    }).collect::<Vec<_>>();
    if by_source.len()==1{return Some(by_source[0]);}
    let candidates=eligible.iter().copied()
        .filter(|r|same_identity(incoming,r)).collect::<Vec<_>>();
    if candidates.len()==1 && incoming_rows.iter()
        .filter(|row|same_identity(incoming,row)).count()==1 {
        return Some(candidates[0]);
    }
    None
}
fn rebind(candidates:&mut Value,key:&str,from:&str,to:&str){
    if let Some(items)=candidates.as_array_mut(){
        for item in items {
            if str_field(item,key)==from {
                // Valid recognition evidence is a JSON object.
                item[key]=json!(to);
            }
        }
    }
}
// Recognition IDs are stable inside one parsed document, but NOT across
// independently received orders. SQLite core_recipients/core_items use a
// GLOBAL primary key. Materialize per-order identities only at the merge
// boundary, leaving the recognition evidence snapshot untouched.
pub(super) fn scoped_recognition_id(order_id:&str,id:&str,prefix:&str)->String{
    if id.starts_with(prefix){format!("{order_id}:{id}")}else{id.to_owned()}
}

fn scope_source_refs(order_id:&str,value:&mut Value)->Result<(),String>{
    for (key,prefix) in [("recipientId","recognition:recipient:"),
        ("itemId","recognition:item:")] {
        if let Some(id)=value.get(key).and_then(Value::as_str).map(str::to_owned){
            let scoped=scoped_recognition_id(order_id,&id,prefix);
            if scoped!=id{set(value,key,json!(scoped))?;}
        }
    }
    if let Some(id)=value.get("targetId").and_then(Value::as_str).map(str::to_owned){
        let scoped=scoped_recognition_id(order_id,&id,"recognition:recipient:");
        let scoped=scoped_recognition_id(order_id,&scoped,"recognition:item:");
        if scoped!=id{set(value,"targetId",json!(scoped))?;}
    }
    Ok(())
}

pub(super) fn scope_recognition_ids(order_id:&str, recipients:&mut [Value],
    candidates:&mut Value)->Result<(),String>{
    if order_id.trim().is_empty(){return Err("발주서 ID가 없습니다.".into());}
    if let Some(entries)=candidates.as_array_mut(){
        for candidate in entries{scope_source_refs(order_id,candidate)?;}
    }
    for recipient in recipients {
        let provisional_rid=str_field(recipient,"id").to_string();
        let stable_rid=scoped_recognition_id(order_id,&provisional_rid,"recognition:recipient:");
        if stable_rid!=provisional_rid {
            rebind(candidates,"recipientId",&provisional_rid,&stable_rid);
            set(recipient,"id",json!(stable_rid))?;
        }
        if let Some(sources)=recipient.get_mut("sources").and_then(Value::as_array_mut){
            for span in sources {
                scope_source_refs(order_id,span)?;
                if str_field(span,"recipientId")==provisional_rid {
                    set(span,"recipientId",json!(stable_rid))?;
                }
                if str_field(span,"targetId")==provisional_rid {
                    set(span,"targetId",json!(stable_rid))?;
                }
            }
        }
        if let Some(items)=recipient.get_mut("items").and_then(Value::as_array_mut){
            for item in items {
                let provisional_iid=str_field(item,"id").to_string();
                let stable_iid=scoped_recognition_id(order_id,&provisional_iid,"recognition:item:");
                if stable_iid!=provisional_iid {
                    rebind(candidates,"itemId",&provisional_iid,&stable_iid);
                    set(item,"id",json!(stable_iid))?;
                }
                if let Some(sources)=item.get_mut("sources").and_then(Value::as_array_mut){
                    for span in sources {
                        scope_source_refs(order_id,span)?;
                        if str_field(span,"recipientId")==provisional_rid {
                            set(span,"recipientId",json!(stable_rid))?;
                        }
                        if str_field(span,"targetId")==provisional_iid {
                            set(span,"targetId",json!(stable_iid))?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
/// Apply the same per-order identity namespace to the persisted manual
/// worksheet. The rows and assignment targets must move together; otherwise
/// preview_manual would restore provisional recognition IDs after a merge.
pub(super) fn scope_manual_model_ids(order_id:&str,model:&mut Value)->Result<(),String>{
    {
        let recipients=model.get_mut("rows").and_then(Value::as_array_mut)
            .ok_or("수동 분류 배송 목록이 없습니다.")?;
        let mut no_candidates=Value::Array(Vec::new());
        scope_recognition_ids(order_id,recipients,&mut no_candidates)?;
    }
    if let Some(blocks)=model.get_mut("blocks").and_then(Value::as_array_mut){
        for block in blocks{scope_source_refs(order_id,block)?;}
    }
    if let Some(assignments)=model.get_mut("assignments").and_then(Value::as_object_mut){
        for assignment in assignments.values_mut(){
            for (key,prefix) in [
                ("recipientId","recognition:recipient:"),
                ("itemId","recognition:item:")
            ]{
                let id=assignment.get(key).and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(id)=id{
                    assignment[key]=json!(scoped_recognition_id(order_id,&id,prefix));
                }
            }
        }
    }
    Ok(())
}

fn qty_sources(item:&Value)->Vec<Value>{
    rows(field(item,"sources")).iter().filter(|s|str_field(s,"field")=="quantity")
        .map(|s|field(s,"text").clone()).collect()
}
fn merge_item(incoming:&Value,prior:&Value)->Result<Value,String>{
    let qty_changed=qty_sources(incoming)!=qty_sources(prior);
    let mut item=if field(prior,"manuallyConfirmed")==true &&
        field(prior,"raw")==field(incoming,"raw"){
        let mut saved=prior.clone();
        clone_field(&mut saved,incoming,"sources")?;
        saved
    }else{ incoming.clone() };
    clone_field(&mut item,prior,"id")?;
    // An explicit unit price is independent of recognition confirmation.
    // Retain it only for the same identified product and variant.
    if !str_field(item,"productId").is_empty() &&
        !str_field(item,"variantId").is_empty() &&
        field(item,"productId")==field(prior,"productId") &&
        field(item,"variantId")==field(prior,"variantId") {
        clone_field(&mut item,prior,"manualUnitPrice")?;
    }
    if qty_changed {
        clone_field(&mut item,incoming,"quantity")?;
        clone_field(&mut item,incoming,"quantityConfirmed")?;
        let mut issues=rows(field(&item,"issues")).iter()
            .filter(|v|!v.as_str().unwrap_or("").contains("수량"))
            .cloned().collect::<Vec<_>>();
        issues.extend(rows(field(incoming,"issues")).iter()
            .filter(|v|v.as_str().unwrap_or("").contains("수량")).cloned());
        set(&mut item,"issues",json!(issues))?;
        item.as_object_mut().ok_or("상품 데이터 형식 오류")?.remove("quantityApproval");
    }
    let new_id=field(&item,"id").clone();
    if let Some(spans)=item.get_mut("sources").and_then(Value::as_array_mut){
        for span in spans {set(span,"targetId",new_id.clone())?;}
    }
    Ok(item)
}
fn apply(payload:&Value)->Result<Value,String>{
    let previous=req(payload,"previous")?;
    let evidence=req(payload,"evidence")?;
    let raw=required_str(payload,"raw")?;
    let client_id=required_str(payload,"clientId")?;
    let at=required_str(payload,"at")?;
    let state=req(payload,"state")?;
    let client=req(payload,"client")?;
    if str_field(client,"id")!=client_id {
        return Err("주문 병합의 거래처와 가격 계산 거래처가 일치하지 않습니다.".into());
    }
    // Match the original guard against changing final or employee-locked orders.
    if has_value(field(previous,"deletedAt"))||
        has_value(field(previous,"confirmedAt"))||
        (has_value(field(previous,"manualClassification")) &&
         field(previous,"analysisRaw").as_str()==Some(raw))||
        field(previous,"pricesLocked")==true||
        str_field(previous,"status")=="EXPORTED" ||
        (field(previous,"analysisRaw").as_str()==Some(raw) &&
         field(previous,"analysisClientId").as_str()==Some(client_id)) {
        return Ok(json!({"order":previous}));
    }
    let original=rows(field(evidence,"recipients")).to_vec();
    let mut parsed=original.clone();
    let mut candidates=field(evidence,"candidates").clone();
    if !candidates.is_array(){return Err("발주 분석 근거를 확인하세요.".into());}
    let prior_rows=rows(field(previous,"recipients"));
    let mut used=HashSet::new();
    for recipient in &mut parsed {
        // All recipient matches are computed against the complete original
        // analysis, never a partially rewritten recipient ID array.
        let Some(prior)=match_recipient(recipient,prior_rows,&used,&original) else{continue};
        let new_id=str_field(recipient,"id").to_string();
        let old_id=str_field(prior,"id").to_string();
        used.insert(old_id.clone());
        rebind(&mut candidates,"recipientId",&new_id,&old_id);
        set(recipient,"id",json!(old_id))?;
        if let Some(spans)=recipient.get_mut("sources").and_then(Value::as_array_mut){
            for span in spans {
                set(span,"targetId",json!(old_id))?;
                set(span,"recipientId",json!(old_id))?;
            }
        }
        for confirmed in rows(field(prior,"confirmedFields")){
            let key=confirmed.as_str().unwrap_or("");
            if ["name","address","phone","mobile","note"].contains(&key){
                clone_field(recipient,prior,key)?;
            }
        }
        for key in ["confirmedFields","dispatchOrigin","actualFreight","freightCode",
                    "vatDefault","vatOverride","vatRateBps","shippingCharges"]{
            clone_field(recipient,prior,key)?;
        }
        if rows(field(prior,"confirmedFields")).iter().any(|v|v=="address"){
            clone_field(recipient,prior,"addressStatus")?;
            if !field(prior,"addressParts").is_null(){
                clone_field(recipient,prior,"addressParts")?;
            }else{
                set(recipient,"addressParts",
                    super::manual_classification::manual_address_parts(str_field(prior,"address")))?;
            }
        }
        let mut matched_items=HashSet::new();
        if let Some(items)=recipient.get_mut("items").and_then(Value::as_array_mut) {
            for item in items {
                let found=rows(field(prior,"items")).iter().find(|candidate|
                    field(candidate,"raw")==field(item,"raw") &&
                    !matched_items.contains(str_field(candidate,"id")));
                if let Some(prior_item)=found {
                    let provisional=str_field(item,"id").to_string();
                    let original_id=str_field(prior_item,"id").to_string();
                    matched_items.insert(original_id.clone());
                    rebind(&mut candidates,"itemId",&provisional,&original_id);
                    *item=merge_item(item,prior_item)?;
                    // Match the original TypeScript reception merge: reprice
                    // this retained item before publishing the merged order.
                    // Never publish a temporary null quote and depend on a
                    // later React effect to repair it.
                    let pid=str_field(item,"productId").to_string();
                    let vid=str_field(item,"variantId").to_string();
                    if !super::pricing::apply_manual_unit_price(item)? && !vid.is_empty() {
                        if let Some(product)=super::find_by_id(field(state,"products"),&pid) {
                            let price=super::pricing::resolve_unit_price(product,&vid,client)?;
                            set(item,"unitPrice",price)?;
                        }
                    }
                }
                let item_id=field(item,"id").clone();
                if let Some(spans)=item.get_mut("sources").and_then(Value::as_array_mut){
                    for span in spans {
                        set(span,"targetId",item_id.clone())?;
                        set(span,"recipientId",json!(old_id))?;
                    }
                }
            }
        }
    }
    scope_recognition_ids(required_str(previous,"id")?,&mut parsed,&mut candidates)?;
    if !str_field(previous,"orderType").is_empty(){
        for recipient in &mut parsed {
            if field(recipient,"dispatchOrigin").is_null(){
                set(recipient,"dispatchOrigin",json!("FACTORY"))?;
            }
            let origin=field(recipient,"dispatchOrigin").clone();
            if let Some(items)=recipient.get_mut("items").and_then(Value::as_array_mut){
                for item in items {set(item,"origin",origin.clone())?;}
            }
        }
    }
    let mut next=previous.clone();
    set(&mut next,"analysisEvidence",candidates)?;
    let original_snapshot=field(previous,"analysisSnapshot");
    if str_field(original_snapshot,"raw")!=raw {
        set(&mut next,"analysisSnapshot",json!({
            "at":at,"raw":raw,"ruleVersion":"rust-recognition-v1",
            "recipients":original
        }))?;
    }
    for (key,value) in [
        ("raw",json!(raw)),("analysisRaw",json!(raw)),("analysisClientId",json!(client_id)),
        ("recipients",json!(parsed)),("pricesLocked",json!(false)),
        ("notified",json!(false)),("paid",json!(false)),("postpaidApproved",json!(false)),
        ("status",json!("REVIEW")),("shippingMode",json!("AUTO"))
    ]{set(&mut next,key,value)?;}
    // Match the original TypeScript's atomic post-merge freight calculation.
    // No transient null/empty-version state may reach React.
    let freight=super::shipping::reception_recommend(&next,state)?;
    let fee=if field(client,"shippingFeeExcluded")==&json!(true) {
        json!(0)
    } else { field(&freight,"total").clone() };
    set(&mut next,"shippingFee",fee)?;
    set(&mut next,"shippingVersion",field(&freight,"version").clone())?;
    Ok(json!({"order":next}))
}
pub(super) fn execute(payload:&Value)->Result<Value,String>{apply(payload)}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn matches_recipient_by_original_name_span_and_preserves_confirmations(){
        let before=json!({"id":"o","raw":"김가상 709","orderType":"DEALER",
            "recipients":[{"id":"old","name":"직원확정","address":"서울",
                "mobile":"010-1111-2222","phone":"","confirmedFields":["name"],
                "sources":[{"field":"name","start":0,"text":"김가상"}],
                "items":[{"id":"i-old","raw":"709 2개","manuallyConfirmed":true,
                    "name":"BD 709","quantity":2,"quantityConfirmed":true,
                    "issues":[],"sources":[{"field":"quantity","text":"2개"}]}]}]});
        let evidence=json!({"recipients":[{"id":"new","name":"김가상","address":"서울",
            "mobile":"010-1111-2222","phone":"",
            "sources":[{"field":"name","start":0,"text":"김가상"}],
            "items":[{"id":"i-new","raw":"709 2개","name":"709","variantId":"v",
                "quantity":2,"quantityConfirmed":true,"issues":[],
                "sources":[{"field":"quantity","text":"2개"}]}]}],
            "candidates":[{"id":"c1","recipientId":"new","itemId":"i-new"}]});
        let result=apply(&json!({"previous":before,"evidence":evidence,
            "raw":"김가상 709","clientId":"c1","at":"now",
            "state":{"products":[]},"client":{"id":"c1","tier":"wholesale"}})).unwrap();
        let r=&result["order"]["recipients"][0];
        assert_eq!(r["id"],"old");
        assert_eq!(r["name"],"직원확정");
        assert_eq!(r["items"][0]["id"],"i-old");
        assert_eq!(result["order"]["analysisEvidence"][0]["recipientId"],"old");
        assert_eq!(result["order"]["analysisEvidence"][0]["itemId"],"i-old");
        assert_eq!(result["order"]["analysisSnapshot"]["recipients"][0]["id"],"new");
        assert_eq!(result["order"]["status"],"REVIEW");
    }
    #[test]
    fn distinct_orders_never_share_recipient_or_item_sqlite_identity(){
        let evidence=json!({"recipients":[{
            "id":"recognition:recipient:0","name":"김가상","address":"서울",
            "mobile":"010-1111-2222",
            "sources":[{"field":"name","text":"김가상","start":0,
                "recipientId":"recognition:recipient:0",
                "targetId":"recognition:recipient:0"}],
            "items":[{"id":"recognition:item:3:0","raw":"709 2개","name":"709",
                "sources":[{"field":"name","text":"709","start":3,
                    "recipientId":"recognition:recipient:0",
                    "targetId":"recognition:item:3:0"}]}]}],
            "candidates":[{"id":"e1","kind":"name",
                "recipientId":"recognition:recipient:0",
                "itemId":"recognition:item:3:0"}]});
        let received=|id:&str|apply(&json!({
            "previous":{"id":id,"raw":"김가상 709 2개","orderType":"AGENCY",
                "recipients":[]},
            "evidence":evidence,"raw":"김가상 709 2개",
            "clientId":"dealer","at":"now",
            "state":{"products":[]},"client":{"id":"dealer","tier":"wholesale"}
        })).unwrap()["order"].clone();
        let a=received("order-one");
        let b=received("order-two");
        assert_eq!(a["recipients"][0]["id"],"order-one:recognition:recipient:0");
        assert_eq!(b["recipients"][0]["id"],"order-two:recognition:recipient:0");
        assert_ne!(a["recipients"][0]["id"],b["recipients"][0]["id"]);
        assert_ne!(a["recipients"][0]["items"][0]["id"],
            b["recipients"][0]["items"][0]["id"]);
        for order in [&a,&b]{
            let r=&order["recipients"][0];
            let item=&r["items"][0];
            assert_eq!(r["sources"][0]["targetId"],r["id"]);
            assert_eq!(r["sources"][0]["recipientId"],r["id"]);
            assert_eq!(item["sources"][0]["targetId"],item["id"]);
            assert_eq!(item["sources"][0]["recipientId"],r["id"]);
            assert_eq!(order["analysisEvidence"][0]["recipientId"],r["id"]);
            assert_eq!(order["analysisEvidence"][0]["itemId"],item["id"]);
            // Immutable raw recognition snapshot remains an honest audit trace.
            assert_eq!(order["analysisSnapshot"]["recipients"][0]["id"],
                "recognition:recipient:0");
        }
    }
    #[test]
    fn manual_model_scopes_rows_and_assignment_targets_per_order(){
        let original=json!({
            "rows":[{"id":"recognition:recipient:0","sources":[{
                "recipientId":"recognition:recipient:0",
                "targetId":"recognition:recipient:0"}],
                "items":[{"id":"recognition:item:3:0","sources":[{
                    "recipientId":"recognition:recipient:0",
                    "targetId":"recognition:item:3:0"}]}]}],
            "blocks":[{"id":"block-1","targetId":"recognition:item:3:0"}],
            "assignments":{"block-1":{"recipientId":"recognition:recipient:0",
                "itemId":"recognition:item:3:0","field":"quantity"}}
        });
        let mut first=original.clone();
        scope_manual_model_ids("order-one",&mut first).unwrap();
        assert_eq!(first["rows"][0]["id"],"order-one:recognition:recipient:0");
        assert_eq!(first["rows"][0]["items"][0]["id"],
            "order-one:recognition:item:3:0");
        assert_eq!(first["assignments"]["block-1"]["recipientId"],
            "order-one:recognition:recipient:0");
        assert_eq!(first["assignments"]["block-1"]["itemId"],
            "order-one:recognition:item:3:0");
        assert_eq!(first["rows"][0]["items"][0]["sources"][0]["recipientId"],
            "order-one:recognition:recipient:0");
        assert_eq!(first["blocks"][0]["id"],"block-1");
        assert_eq!(first["blocks"][0]["targetId"],"order-one:recognition:item:3:0");
        // Mixed older candidate/target references survive even if rows were
        // already normalized by a previous client calculation.
        first["rows"][0]["items"][0]["sources"][0]["targetId"]=json!("recognition:item:3:0");
        let mut candidates=json!([{"recipientId":"recognition:recipient:0","itemId":"recognition:item:3:0"}]);
        scope_recognition_ids("order-one",first["rows"].as_array_mut().unwrap(),&mut candidates).unwrap();
        assert_eq!(candidates[0]["recipientId"],"order-one:recognition:recipient:0");
        assert_eq!(candidates[0]["itemId"],"order-one:recognition:item:3:0");
        assert_eq!(first["rows"][0]["items"][0]["sources"][0]["targetId"],"order-one:recognition:item:3:0");
        let once=first.clone();
        scope_manual_model_ids("order-one",&mut first).unwrap();
        assert_eq!(first,once,"scoping must be idempotent");
        let mut second=original;
        scope_manual_model_ids("order-two",&mut second).unwrap();
        assert_ne!(first["rows"][0]["id"],second["rows"][0]["id"]);
        assert_ne!(first["rows"][0]["items"][0]["id"],
            second["rows"][0]["items"][0]["id"]);
    }

    #[test]
    fn reanalysis_retains_scoped_recipient_and_item_id(){
        let evidence=json!({"recipients":[{
            "id":"recognition:recipient:0","name":"김가상","address":"서울",
            "mobile":"010-1111-2222",
            "sources":[{"field":"name","text":"김가상","start":0,
                "recipientId":"recognition:recipient:0",
                "targetId":"recognition:recipient:0"}],
            "items":[{"id":"recognition:item:3:0","raw":"709 2개","name":"709",
                "sources":[{"field":"name","text":"709","start":3,
                    "recipientId":"recognition:recipient:0",
                    "targetId":"recognition:item:3:0"}]}]}],"candidates":[]});
        let first=apply(&json!({"previous":{
            "id":"order-c","raw":"김가상 709 2개","orderType":"AGENCY","recipients":[]},
            "evidence":evidence,"raw":"김가상 709 2개",
            "clientId":"dealer","at":"now",
            "state":{"products":[]},"client":{"id":"dealer","tier":"wholesale"}
        })).unwrap()["order"].clone();
        let second=apply(&json!({"previous":first,"evidence":evidence,
            "raw":"김가상 709 2개 ","clientId":"dealer","at":"later",
            "state":{"products":[]},"client":{"id":"dealer","tier":"wholesale"}})).unwrap();
        assert_eq!(second["order"]["recipients"][0]["id"],
            "order-c:recognition:recipient:0");
        assert_eq!(second["order"]["recipients"][0]["items"][0]["id"],
            "order-c:recognition:item:3:0");
    }
    #[test]
    fn reanalysis_requotes_retained_item_before_returning_to_react(){
        let previous=json!({"id":"o","raw":"김가상 709 2개","orderType":"DEALER",
            "recipients":[{"id":"r","name":"김가상","address":"서울",
                "mobile":"","phone":"","sources":[{"field":"name","text":"김가상","start":0}],
                "items":[{"id":"old-i","raw":"709 2개","productId":"p","variantId":"v",
                    "unitPrice":3000,"manuallyConfirmed":true,"name":"BD 709",
                    "quantity":2,"quantityConfirmed":true,"issues":[],
                    "sources":[{"field":"quantity","text":"2개"}]}]}]});
        let evidence=json!({"recipients":[{"id":"parsed-r","name":"김가상",
            "address":"서울","mobile":"","phone":"",
            "sources":[{"field":"name","text":"김가상","start":0}],
            "items":[{"id":"parsed-i","raw":"709 2개","productId":"p","variantId":"v",
                "unitPrice":3000,"quantity":2,"quantityConfirmed":true,"issues":[],
                "sources":[{"field":"quantity","text":"2개"}]}]}],
            "candidates":[]});
        let state=json!({"products":[{"id":"p","variants":[{"id":"v",
            "wholesale":5000,"semi":7500,"retail":10000}]}]});
        let client=json!({"id":"dealer","tier":"semi"});
        let result=apply(&json!({"previous":previous,"evidence":evidence,
            "raw":"김가상 709 2개","clientId":"dealer","at":"now",
            "state":state,"client":client})).unwrap();
        let item=&result["order"]["recipients"][0]["items"][0];
        assert_eq!(item["id"],"old-i");
        assert_eq!(item["unitPrice"],7500);
        assert_eq!(item["quantity"],2);
    }
    #[test]
    fn reanalysis_keeps_explicit_prices_and_freight_for_same_product(){
        let previous=json!({"id":"o","orderType":"DEALER",
            "recipients":[{"id":"r","name":"테스트","address":"서울",
                "dispatchOrigin":"FACTORY","freightCode":"010",
                "shippingCharges":{"FACTORY":7000},
                "items":[{"id":"i","raw":"테스트 상품","productId":"p","variantId":"v",
                    "quantity":2,"origin":"FACTORY","manualUnitPrice":9900,"unitPrice":9900}]}]});
        let evidence=json!({"recipients":[{"id":"new-r","name":"테스트","address":"서울",
            "items":[{"id":"new-i","raw":"테스트 상품","productId":"p","variantId":"v",
                "quantity":2,"origin":"FACTORY","unitPrice":12000}]}],"candidates":[]});
        let state=json!({"products":[{"id":"p","origin":"FACTORY",
            "variants":[{"id":"v","wholesale":12000},{"id":"v2","wholesale":15000}]}],
            "shipping":{"epoch":1,"productRules":{"p":{"version":"1","fees":{"2":5200}}}}});
        let client=json!({"id":"c","tier":"wholesale"});
        let merge=|previous:&Value,evidence:&Value|apply(&json!({
            "previous":previous,"evidence":evidence,"raw":"테스트 상품",
            "clientId":"c","at":"now","state":state,"client":client
        })).unwrap()["order"].clone();
        let kept=merge(&previous,&evidence);
        assert_eq!(kept["recipients"][0]["items"][0]["unitPrice"],9900);
        assert_eq!(kept["recipients"][0]["items"][0]["manualUnitPrice"],9900);
        assert_eq!(kept["shippingFee"],7000);
        let mut zero=previous.clone();
        zero["recipients"][0]["items"][0]["manualUnitPrice"]=json!(0);
        zero["recipients"][0]["shippingCharges"]["FACTORY"]=json!(0);
        let kept_zero=merge(&zero,&evidence);
        assert_eq!(kept_zero["recipients"][0]["items"][0]["unitPrice"],0);
        assert_eq!(kept_zero["shippingFee"],0);
        let mut changed=evidence.clone();
        changed["recipients"][0]["items"][0]["variantId"]=json!("v2");
        let changed=merge(&previous,&changed);
        assert!(changed["recipients"][0]["items"][0].get("manualUnitPrice").is_none());
        assert_eq!(changed["recipients"][0]["items"][0]["unitPrice"],15000);
    }
    #[test]
    fn merged_price_respects_current_dealer_special_override(){
        let previous=json!({"id":"o","orderType":"DEALER",
            "recipients":[{"id":"r","name":"김가상","address":"서울",
                "mobile":"","phone":"","sources":[{"field":"name","text":"김가상","start":0}],
                "items":[{"id":"old-i","raw":"709 2개","productId":"p","variantId":"v",
                    "unitPrice":5000,"manuallyConfirmed":true,
                    "sources":[{"field":"quantity","text":"2개"}]}]}]});
        let evidence=json!({"recipients":[{"id":"new-r","name":"김가상",
            "address":"서울","mobile":"","phone":"",
            "sources":[{"field":"name","text":"김가상","start":0}],
            "items":[{"id":"new-i","raw":"709 2개","productId":"p","variantId":"v",
                "unitPrice":5000,"sources":[{"field":"quantity","text":"2개"}]}]}],
            "candidates":[]});
        let result=apply(&json!({"previous":previous,"evidence":evidence,
            "raw":"김가상 709 2개","clientId":"dealer","at":"now",
            "state":{"products":[{"id":"p","variants":[{"id":"v","wholesale":5000}]}]},
            "client":{"id":"dealer","tier":"special","overrides":{"v":13500}}
        })).unwrap();
        assert_eq!(result["order"]["recipients"][0]["items"][0]["unitPrice"],13500);
    }
    #[test]fn exported_orders_cannot_be_reprocessed(){
        let previous=json!({"id":"o","status":"EXPORTED","recipients":[]});
        let result=apply(&json!({"previous":previous,"evidence":{"recipients":[],"candidates":[]},
            "raw":"test","clientId":"c","at":"now",
            "state":{"products":[]},"client":{"id":"c","tier":"wholesale"}})).unwrap();
        assert_eq!(result["order"],previous);
    }
}
