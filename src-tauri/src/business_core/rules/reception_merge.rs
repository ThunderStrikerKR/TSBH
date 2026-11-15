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
    // Match the original guard against changing final or employee-locked orders.
    if has_value(field(previous,"deletedAt"))||
        has_value(field(previous,"confirmedAt"))||
        has_value(field(previous,"manualClassification"))||
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
                    "vatDefault","vatOverride","shippingCharges"]{
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
                    // Subsequent Rust reprice_order supplies the fresh quote.
                    if !str_field(item,"variantId").is_empty(){
                        set(item,"unitPrice",Value::Null)?;
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
        ("status",json!("REVIEW")),("shippingFee",Value::Null),
        ("shippingMode",json!("AUTO")),("shippingVersion",json!(""))
    ]{set(&mut next,key,value)?;}
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
            "raw":"김가상 709","clientId":"c1","at":"now"})).unwrap();
        let r=&result["order"]["recipients"][0];
        assert_eq!(r["id"],"old");
        assert_eq!(r["name"],"직원확정");
        assert_eq!(r["items"][0]["id"],"i-old");
        assert_eq!(result["order"]["analysisEvidence"][0]["recipientId"],"old");
        assert_eq!(result["order"]["analysisEvidence"][0]["itemId"],"i-old");
        assert_eq!(result["order"]["analysisSnapshot"]["recipients"][0]["id"],"new");
        assert_eq!(result["order"]["status"],"REVIEW");
    }
    #[test]fn exported_orders_cannot_be_reprocessed(){
        let previous=json!({"id":"o","status":"EXPORTED","recipients":[]});
        let result=apply(&json!({"previous":previous,"evidence":{"recipients":[],"candidates":[]},
            "raw":"test","clientId":"c","at":"now"})).unwrap();
        assert_eq!(result["order"],previous);
    }
}
