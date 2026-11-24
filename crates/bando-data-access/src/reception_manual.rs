//! Validate and replay an egui employee manual draft inside the authoritative
//! Tauri SQLite transaction. Never trust item price, product snapshots,
//! recipient sources, freight, or client identity supplied by the UI.
//! Validate source-backed staff edits against fresh Rust recognition and
//! direct-entry edits against an empty native source model, then replay typed
//! overrides through the same reducer. Never accept a raw unitPrice.
//!
//! All meaningful mutations are replayed through existing Rust reducers.
//! UI-submitted rows provide ONLY target IDs, editable strings and quantities;
//! reconstructed blocks/assignments/products come from the current master.

use crate::rules;
use serde_json::{json,Value};
use std::collections::HashSet;

fn s<'a>(v:&'a Value,k:&str)->&'a str{
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(v:&'a Value,k:&str)->Result<&'a Vec<Value>,String>{
    v.get(k).and_then(Value::as_array).ok_or_else(||format!("{k} 목록 오류"))
}
fn rule(action:&str,mut input:Value)->Result<Value,String>{
    input["action"]=json!(action);
    rules::execute("apply_manual_classification",input)
}
fn model(result:Value)->Result<(Value,Vec<String>),String>{
    let obj=result.get("model").filter(|v|v.is_object())
        .ok_or("Rust 수동 분류 모델 오류")?.clone();
    let keys=result.get("assignmentOrder").and_then(Value::as_array)
        .ok_or("Rust 수동 배정 순서 오류")?.iter()
        .map(|v|v.as_str().map(str::to_owned).ok_or("수동 배정 ID 오류".to_owned()))
        .collect::<Result<Vec<_>,_>>()?;
    Ok((obj,keys))
}
fn scoped(order_id:&str,id:&str)->String{
    if id.starts_with("recognition:recipient:") ||
       id.starts_with("recognition:item:") {
        format!("{order_id}:{id}")
    } else {id.to_owned()}
}
fn submit_id(order_id:&str,v:&Value)->Result<String,String>{
    let id=v.as_str().filter(|id|!id.is_empty()&&id.len()<220)
        .ok_or("수동 분류 식별자 형식 오류")?;
    Ok(scoped(order_id,id))
}
fn client_field<'a>(row:&'a Value,key:&str)->Result<&'a str,String>{
    match row.get(key) {
        Some(Value::String(value))=>Ok(value),
        Some(Value::Null)|None=>Ok(""),
        _=>Err(format!("수취인 {key} 입력값 형식 오류")),
    }
}
fn apply_step(
    action:&str,m:&mut Value,keys:&mut Vec<String>,extra:Value
)->Result<(),String>{
    let mut input=json!({"model":m,"assignmentOrder":keys});
    if let Some(fields)=extra.as_object(){
        for (key,value) in fields {input[key]=value.clone();}
    }
    let mut outcome=rule(action,input)?;
    if outcome.get("assignmentOrder").is_none(){
        // Direct recipient field edits return only their modified model.
        // The native reducer leaves the source assignment order untouched.
        outcome["assignmentOrder"]=json!(keys);
    }
    let (next,next_keys)=model(outcome)?;
    *m=next;
    *keys=next_keys;
    Ok(())
}
fn trusted_blocks(native:&Value,submission:&Value)->Result<(),String>{
    let canonical=list(native,"blocks")?;
    let submitted=list(submission,"blocks")?;
    if canonical.len()!=submitted.len() || canonical.len()>4096 {
        return Err("발주원문 수동 분류 블록 개수가 최신 Rust 분석과 다릅니다.".into());
    }
    for (a,b) in canonical.iter().zip(submitted){
        for key in ["id","start","end","text"] {
            if a.get(key)!=b.get(key){
                return Err("원문 블록 내용 또는 위치가 현재 Rust 인식과 다릅니다.".into());
            }
        }
    }
    Ok(())
}
fn valid_new_id(id:&str)->bool{
    id.starts_with("egui-manual-") && id.len()<180 &&
    id.chars().all(|c|c.is_ascii_alphanumeric()||"-_:".contains(c))
}
fn canonical_rows(
    order_id:&str,native:&mut Value,keys:&mut Vec<String>,submitted:&Value
)->Result<(),String>{
    let requested=list(submitted,"rows")?;
    if requested.is_empty()||requested.len()>100 {
        return Err("배송 건수는 1~100건입니다.".into());
    }
    let old=list(native,"rows")?.clone();
    let old_len=old.len();
    // Existing Rust-recognized shipment rows may only be removed from the
    // end; arbitrary reordering of same-name recipients is never guessed.
    let employee_reset=old.first().is_none_or(|row|
        submit_id(order_id,&requested[0]["id"]).ok().as_deref()!=Some(s(row,"id"))
    );
    let base_len=if employee_reset {
        let first=submit_id(order_id,&requested[0]["id"])?;
        if !valid_new_id(&first){
            return Err("전체 초기화 후 새 수취인 ID가 잘못되었습니다.".into());
        }
        apply_step("reset_manual_rows",native,keys,json!({
            "recipientId":first
        }))?;
        1
    }else{old_len};
    for (idx,row) in requested.iter().enumerate().take(base_len){
        if submit_id(order_id,&row["id"])?
            !=s(&list(native,"rows")?[idx],"id") {
            return Err("배송 건의 원본 소속 또는 순서가 변경됐습니다.".into());
        }
    }
    if requested.len()!=base_len{
        let extra=if requested.len()>base_len{
            requested.iter().skip(base_len).map(|row|{
                let id=submit_id(order_id,&row["id"])?;
                if !valid_new_id(&id){return Err("추가 배송 건의 식별자가 잘못됐습니다.".into());}
                Ok(id)
            }).collect::<Result<Vec<_>,String>>()?
        }else{Vec::new()};
        apply_step("resize_manual_rows",native,keys,json!({
            "count":requested.len(),"newRecipientIds":extra
        }))?;
    }
    // Item IDs are stable across recognized lines. New employee rows are
    // appended; existing IDs cannot move to another recipient or be forged.
    for (idx,row) in requested.iter().enumerate(){
        let rid=submit_id(order_id,&row["id"])?;
        let wanted=list(row,"items")?;
        if wanted.len()>200{return Err("배송 건당 상품 수가 너무 많습니다.".into());}
        let current=list(&list(native,"rows")?[idx],"items")?.clone();
        let existing=current.iter().map(|v|s(v,"id").to_owned())
            .collect::<HashSet<_>>();
        let mut seen=HashSet::new();
        for item in wanted {
            let id=submit_id(order_id,&item["id"])?;
            if !seen.insert(id.clone()){return Err("중복 상품 ID가 있습니다.".into());}
            if !existing.contains(&id) && !valid_new_id(&id){
                return Err("추가 상품의 ID를 확인하세요.".into());
            }
        }
        for item in current{
            let id=s(&item,"id");
            if !seen.contains(id){
                apply_step("remove_manual_item",native,keys,json!({
                    "recipientId":rid,"itemId":id
                }))?;
            }
        }
        for item in wanted{
            let id=submit_id(order_id,&item["id"])?;
            if !existing.contains(&id){
                apply_step("add_manual_item",native,keys,json!({
                    "recipientId":rid,"itemId":id
                }))?;
            }
        }
        let native_items=list(&list(native,"rows")?[idx],"items")?;
        let resulting=native_items.iter().map(|v|s(v,"id")).collect::<Vec<_>>();
        let wanted_ids=wanted.iter().map(|v|
            submit_id(order_id,&v["id"])
        ).collect::<Result<Vec<_>,_>>()?;
        if resulting!=wanted_ids.iter().map(String::as_str).collect::<Vec<_>>(){
            return Err("상품 행의 원본 순서를 변경할 수 없습니다.".into());
        }
    }
    Ok(())
}
fn assign_selected(
    order_id:&str,native:&mut Value,keys:&mut Vec<String>,
    submitted:&Value,state:&Value,order:&Value
)->Result<(),String>{
    let desired=submitted.get("assignments").and_then(Value::as_object)
        .ok_or("원문 블록 배정 정보가 없습니다.")?;
    let order_keys=list(submitted,"assignmentOrder")?;
    if desired.len()!=order_keys.len(){
        return Err("원문 블록 배정 순서와 개수가 다릅니다.".into());
    }
    let mut seen=HashSet::new();
    for id in order_keys{
        let id=id.as_str().ok_or("원문 블록 ID 오류")?;
        if !seen.insert(id.to_owned()) || !desired.contains_key(id){
            return Err("원문 블록을 중복 배정했거나 누락했습니다.".into());
        }
    }
    // Release automatically inferred links before replaying employee links.
    // Keep recognized values until the exact final edited fields are applied.
    for id in keys.clone(){
        apply_step("release_manual_block",native,keys,json!({
            "id":id,"clear":false
        }))?;
    }
    let mut targets=HashSet::new();
    for id in order_keys{
        let id=id.as_str().ok_or("원문 배정 ID 오류")?;
        if !list(native,"blocks")?.iter().any(|block|s(block,"id")==id){
            return Err("최신 Rust 원문 블록에 없는 배정입니다.".into());
        }
        let entry=&desired[id];
        let rid=submit_id(order_id,&entry["recipientId"])?;
        let field=s(entry,"field");
        let iid=entry.get("itemId").and_then(Value::as_str)
            .filter(|x|!x.is_empty()).map(|x|scoped(order_id,x));
        let allowed=if iid.is_some(){
            ["product","quantity"].contains(&field)
        }else{
            ["name","address","mobile","phone","note"].contains(&field)
        };
        if !allowed {return Err("수동 분류 필드 유형 오류".into());}
        let row=list(native,"rows")?.iter().find(|row|s(row,"id")==rid)
            .ok_or("배정할 수취인 ID가 존재하지 않습니다.")?;
        if let Some(ref iid)=iid{
            if !list(row,"items")?.iter().any(|item|s(item,"id")==iid){
                return Err("배정할 상품 행이 존재하지 않습니다.".into());
            }
        }
        if !targets.insert(format!("{rid}:{iid:?}:{field}")){
            return Err("같은 수취인 셀에 원문 블록을 중복 배정했습니다.".into());
        }
        let mut cell=json!({"recipientId":rid,"field":field});
        if let Some(iid)=iid{cell["itemId"]=json!(iid);}
        apply_step("assign_manual_block",native,keys,json!({
            "id":id,"cell":cell,"state":state,"order":order,
            "candidateIds":[]
        }))?;
    }
    if *keys!=order_keys.iter().filter_map(Value::as_str)
        .map(str::to_owned).collect::<Vec<_>>() {
        return Err("Rust가 확정한 배정 순서가 제출된 순서와 다릅니다.".into());
    }
    Ok(())
}
fn edit_fields(
    order_id:&str,native:&mut Value,keys:&mut Vec<String>,
    submitted:&Value,state:&Value,order:&Value,client:&Value
)->Result<(),String>{
    let requested=list(submitted,"rows")?;
    for (idx,row) in requested.iter().enumerate(){
        let rid=submit_id(order_id,&row["id"])?;
        for field in ["name","address","phone","mobile","note"]{
            let value=client_field(row,field)?;
            if value.chars().count()>10000{return Err("수취인 입력값이 너무 깁니다.".into());}
            if s(&list(native,"rows")?[idx],field)!=value {
                apply_step("set_manual_recipient_field",native,keys,json!({
                    "cell":{"recipientId":rid,"field":field},"value":value
                }))?;
            }
        }
        for desired_item in list(row,"items")?{
            let iid=submit_id(order_id,&desired_item["id"])?;
            let current=list(native,"rows")?[idx]["items"]
                .as_array().and_then(|items|items.iter().find(|item|s(item,"id")==iid))
                .ok_or("상품 ID가 일치하지 않습니다.")?.clone();
            let current_pid=s(&current,"productId");
            let current_vid=s(&current,"variantId");
            let desired_pid=s(desired_item,"productId");
            let desired_vid=s(desired_item,"variantId");
            let typed=s(desired_item,"raw");
            let name=s(desired_item,"name");
            let value=if !typed.trim().is_empty(){typed}else{name};
            if value.chars().count()>1000{return Err("상품명 입력이 너무 깁니다.".into());}
            if current_pid!=desired_pid || current_vid!=desired_vid ||
               (current_pid.is_empty() && s(&current,"raw")!=value) {
                let product=if !desired_pid.is_empty(){
                    Some(list(state,"products")?.iter().find(|p|
                        s(p,"id")==desired_pid &&
                        p.get("catalog").and_then(Value::as_str)!=Some("DEMO") &&
                        p.get("active")!=Some(&Value::Bool(false))
                    ).ok_or("등록되지 않은 상품을 수동으로 선택했습니다.")?)
                }else{None};
                if let Some(product)=product {
                    if !list(product,"variants")?.iter().any(|v|
                        s(v,"id")==desired_vid && 
                        s(v,"source")!="DEMO"
                    ){return Err("상품 옵션이 현재 마스터와 다릅니다.".into());}
                }
                let mut extra=json!({
                    "cell":{"recipientId":rid,"itemId":iid,"field":"product"},
                    "value":value,"order":order,"state":state
                });
                if let Some(product)=product{extra["product"]=product.clone();}
                apply_step("set_manual_product",native,keys,extra)?;
            }
            let product_current=list(native,"rows")?[idx]["items"]
                .as_array().and_then(|items|items.iter().find(|v|s(v,"id")==iid))
                .ok_or("수동 상품 행이 없습니다.")?;
            if s(product_current,"productId")!=desired_pid ||
               s(product_current,"variantId")!=desired_vid {
                return Err("선택 상품·옵션이 Rust 재검증 결과와 다릅니다.".into());
            }
            let submitted_q=desired_item.get("quantity").and_then(Value::as_i64);
            let confirmed=desired_item.get("quantityConfirmed")==Some(&Value::Bool(true));
            let native_q=product_current.get("quantity").and_then(Value::as_i64);
            if submitted_q!=native_q || (confirmed &&
               product_current.get("quantityConfirmed")!=Some(&Value::Bool(true))) {
                if let Some(quantity)=submitted_q{
                    if !(1..=9_007_199_254_740_991).contains(&quantity){
                        return Err("수동 상품 수량 범위를 초과했습니다.".into());
                    }
                    apply_step("set_manual_quantity_draft",native,keys,json!({
                        "cell":{"recipientId":rid,"itemId":iid,"field":"quantity"},
                        "value":quantity.to_string()
                    }))?;
                    if confirmed{
                        apply_step("confirm_manual_quantity",native,keys,json!({
                            "cell":{"recipientId":rid,"itemId":iid,"field":"quantity"}
                        }))?;
                    }
                } else {
                    apply_step("set_manual_quantity_draft",native,keys,json!({
                        "cell":{"recipientId":rid,"itemId":iid,"field":"quantity"},
                        "value":""
                    }))?;
                }
            }
            // Unit prices supplied in the JSON model are NOT authoritative.
            // Only a distinct validated staff override may change them.
            if let Some(value)=desired_item.get("manualUnitPrice") {
                if !value.is_null(){
                    apply_step("set_manual_unit_price",native,keys,json!({
                        "cell":{"recipientId":rid,"itemId":iid},"value":value
                    }))?;
                }
            }
        }
        // Dispatch, COD, locality and approved base freight must also be
        // replayed from the recipient-bound draft. Previously these fields
        // were silently discarded on the native SQLite commit.
        let origin=row.get("dispatchOrigin").and_then(Value::as_str)
            .unwrap_or("FACTORY");
        if !matches!(origin,"FACTORY"|"STORE"){
            return Err("발송지는 공장 또는 매장이어야 합니다.".into());
        }
        let native_origin=s(&list(native,"rows")?[idx],"dispatchOrigin");
        if native_origin!=origin && !(native_origin.is_empty()&&origin=="FACTORY"){
            apply_step("set_manual_delivery_option",native,keys,json!({
                "cell":{"recipientId":rid,"field":"dispatchOrigin"},
                "value":origin
            }))?;
        }
        let code=row.get("freightCode").and_then(Value::as_str)
            .unwrap_or("010");
        if !matches!(code,"010"|"020"){
            return Err("착불·선불 구분을 확인하세요.".into());
        }
        let native_code=s(&list(native,"rows")?[idx],"freightCode");
        if native_code!=code && !(native_code.is_empty()&&code=="010"){
            apply_step("set_manual_delivery_option",native,keys,json!({
                "cell":{"recipientId":rid,"field":"freightCode"},
                "value":code
            }))?;
        }
        if let Some(remote)=row.get("remoteDelivery").filter(|value|
            value.get("manual")==Some(&Value::Bool(true))
        ){
            if remote.get("address").and_then(Value::as_str)!=Some(s(row,"address")){
                return Err("도서산간 설정이 현재 수취인 주소와 다릅니다.".into());
            }
            let checked=remote.get("checked").and_then(Value::as_bool)
                .ok_or("도서산간 적용값이 잘못되었습니다.")?;
            apply_step("set_manual_delivery_option",native,keys,json!({
                "cell":{"recipientId":rid,"field":"remoteDelivery"},
                "value":checked
            }))?;
        }
        if let Some(vat)=row.get("vatOverride") {
            if !vat.is_null() && !vat.is_boolean(){
                return Err("부가세 포함 여부가 올바르지 않습니다.".into());
            }
            if list(native,"rows")?[idx].get("vatOverride")!=Some(vat){
                apply_step("set_manual_delivery_option",native,keys,json!({
                    "cell":{"recipientId":rid,"field":"vatOverride"},
                    "value":vat
                }))?;
            }
        }
        if let Some(charges)=row.get("shippingCharges"){
            let map=charges.as_object().ok_or("수취인 운임 정보 형식 오류")?;
            if map.keys().any(|key|!matches!(key.as_str(),"FACTORY"|"STORE")){
                return Err("수취인 운임 발송지 정보 오류".into());
            }
            if let Some(value)=map.get(origin){
                apply_step("set_manual_delivery_option",native,keys,json!({
                    "cell":{"recipientId":rid,"field":"shippingBaseCharge"},
                    "value":value
                }))?;
            }
        }
    }
    // Client reference was loaded from SQLite inside the same transaction.
    if s(client,"id")!=s(order,"clientId"){
        return Err("수동 분류의 거래처가 현재 주문과 일치하지 않습니다.".into());
    }
    Ok(())
}
pub(crate) fn verify_apply(
    order:&Value,state:&Value,client:&Value,source:&Value
)->Result<Value,String>{
    let order_id=s(order,"id");
    let raw=s(order,"raw");
    let direct=raw.is_empty() && s(source,"mode")=="DIRECT";
    if order_id.is_empty() || (raw.is_empty() && !direct){
        return Err("수동 분류할 발주서가 없습니다.".into());
    }
    if (s(source,"mode")=="DIRECT" && !direct) ||
       s(source,"sourceRaw")!=raw ||
       s(source,"orderType")!=s(order,"orderType") ||
       s(source,"clientId")!=s(order,"clientId"){
        return Err("수동 분류 작업본의 원문·유형·거래처가 다릅니다.".into());
    }
    if serde_json::to_vec(source).map_err(|e|e.to_string())?.len()>2_000_000{
        return Err("수동 분류 모델 크기 한도를 초과했습니다.".into());
    }
    let submission=source.get("model").filter(|v|v.is_object())
        .ok_or("수동 분류 모델이 없습니다.")?;
    if direct{
        if !list(submission,"blocks")?.is_empty() ||
           submission.get("assignments").and_then(Value::as_object)
                .is_none_or(|assignments|!assignments.is_empty()) ||
           source.get("assignmentOrder").and_then(Value::as_array)
                .is_none_or(|keys|!keys.is_empty()){
            return Err("직접 입력에는 발주원문 블록이나 원문 배정이 존재할 수 없습니다.".into());
        }
    }
    let (mut native,mut keys)=model(rule("initial_manual",json!({"order":order}))?)?;
    trusted_blocks(&native,submission)?;
    canonical_rows(order_id,&mut native,&mut keys,submission)?;
    let mut submitted=submission.clone();
    submitted["assignmentOrder"]=source.get("assignmentOrder").cloned()
        .ok_or("원문 배정 순서 누락")?;
    assign_selected(order_id,&mut native,&mut keys,&submitted,state,order)?;
    edit_fields(order_id,&mut native,&mut keys,submission,state,order,client)?;
    let status=rule("manual_quantity_status",json!({"model":native}))?;
    if status.get("pending")==Some(&Value::Bool(true)){
        return Err("수량 확인되지 않은 상품이 있습니다.".into());
    }
    let outcome=rule("apply_manual",json!({
        "order":order,"model":native,"assignmentOrder":keys
    }))?;
    let next=outcome.get("order").filter(|v|v.is_object())
        .ok_or("수동 분류 적용 결과가 올바르지 않습니다.")?.clone();
    if s(&next,"id")!=order_id || s(&next,"raw")!=raw ||
       s(&next,"clientId")!=s(client,"id") ||
       !next.get("manualClassification").is_some_and(Value::is_object) {
        return Err("수동 분류 결과가 기존 주문과 일치하지 않습니다.".into());
    }
    Ok(next)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn only_native_recognition_blocks_are_eligible(){
        let real=json!({"blocks":[{"id":"0:2","start":0,"end":2,"text":"가나"}]});
        let forged=json!({"blocks":[{"id":"0:2","start":0,"end":2,"text":"다라"}]});
        assert!(trusted_blocks(&real,&forged).is_err());
    }
    #[test]
    fn direct_manual_entry_replays_without_fabricating_source_evidence(){
        let order=json!({
            "id":"egui-direct-example","raw":"","orderType":"AGENCY",
            "clientId":"c","recipients":[]
        });
        let initial=rule("initial_manual",json!({"order":order})).unwrap();
        let resized=rule("resize_manual_rows",json!({
            "model":initial["model"],"count":1,
            "newRecipientIds":["egui-manual-direct-recipient"],
            "assignmentOrder":initial["assignmentOrder"]
        })).unwrap();
        let mut model=resized["model"].clone();
        model["rows"][0]["name"]=json!("김가상");
        model["rows"][0]["address"]=json!("서울특별시 강남구 테헤란로 10");
        let state=json!({"products":[],"aliases":[],"clients":[{
            "id":"c","tier":"wholesale","payment":"PREPAID",
            "overrides":{},"shippingFeeExcluded":false
        }]});
        let submitted=json!({
            "mode":"DIRECT","sourceRaw":"","orderType":"AGENCY",
            "clientId":"c","model":model,"assignmentOrder":[]
        });
        let result=verify_apply(&order,&state,&state["clients"][0],&submitted).unwrap();
        assert_eq!(result["raw"],"");
        assert_eq!(result["analysisRaw"],"");
        assert_eq!(result["recipients"][0]["name"],"김가상");
        assert_eq!(result["recipients"][0]["address"],"서울특별시 강남구 테헤란로 10");
        assert_eq!(result["manualClassification"]["blocks"],json!([]));
        assert_eq!(result["manualClassification"]["assignments"],json!({}));
        assert!(result.get("analysisEvidence").is_none());
    }

    #[test]
    fn direct_mode_rejects_fabricated_source_blocks_and_assignments(){
        let order=json!({"id":"egui-direct-example","raw":"","orderType":"AGENCY","clientId":"c"});
        let state=json!({"products":[],"aliases":[],"clients":[{"id":"c"}]});
        let initial=rule("initial_manual",json!({"order":order.clone()})).unwrap();
        let resized=rule("resize_manual_rows",json!({
            "model":initial["model"],"count":1,
            "newRecipientIds":["egui-manual-direct-forged"],
            "assignmentOrder":initial["assignmentOrder"]
        })).unwrap();
        let mut model=resized["model"].clone();
        model["blocks"]=json!([{"id":"0:1","start":0,"end":1,"text":"x"}]);
        let submitted=json!({"mode":"DIRECT","sourceRaw":"","orderType":"AGENCY",
            "clientId":"c","model":model,"assignmentOrder":[]});
        assert!(verify_apply(&order,&state,&state["clients"][0],&submitted).is_err());
    }

    #[test]
    fn empty_source_without_direct_mode_is_rejected(){
        let order=json!({"id":"egui-direct-example","raw":"","orderType":"AGENCY","clientId":"c"});
        let submitted=json!({"sourceRaw":"","orderType":"AGENCY","clientId":"c",
            "model":{"rows":[{"id":"egui-manual-r","name":"","address":"",
                "phone":"","mobile":"","note":"","items":[]}],
                "blocks":[],"assignments":{}},"assignmentOrder":[]});
        assert!(verify_apply(&order,&json!({"clients":[{"id":"c"}]}),
            &json!({"id":"c"}),&submitted).is_err());
    }

    #[test]
    fn canonical_native_manual_fields_can_be_replayed_without_trusting_ui_prices(){
        let rid="egui-example:recognition:recipient:0";
        let order=json!({
            "id":"egui-example","raw":"김가상","analysisRaw":"김가상",
            "orderType":"AGENCY","clientId":"c",
            "recipients":[{"id":rid,"name":"김가상",
                "address":"","phone":"","mobile":"","note":"",
                "items":[],"sources":[{"id":"name:0:3","field":"name",
                    "start":0,"end":3,"text":"김가상",
                    "targetId":rid,"recipientId":rid}]}],
            "analysisEvidence":[]
        });
        let initial=rule("initial_manual",json!({"order":order})).unwrap();
        let mut model=initial["model"].clone();
        model["rows"][0]["address"]=json!("서울특별시 강남구 테헤란로 10");
        let submitted=json!({
            "sourceRaw":"김가상","orderType":"AGENCY","clientId":"c",
            "model":model,"assignmentOrder":initial["assignmentOrder"]
        });
        let result=verify_apply(
            &order,&json!({"products":[],"aliases":[],"clients":[{"id":"c"}]}),
            &json!({"id":"c"}),&submitted
        ).unwrap();
        assert_eq!(result["recipients"][0]["address"],
            "서울특별시 강남구 테헤란로 10");
        assert!(result["manualClassification"]["blocks"].is_array());
    }
    #[test]
    fn sqlite_replay_keeps_staff_unit_price_quantity_and_base_freight() {
        let rid="egui-example:recognition:recipient:0";
        let iid="egui-example:recognition:item:0";
        let address="제주특별자치도 제주시 연동 1";
        let order=json!({"id":"egui-example","raw":"가상 발주",
            "analysisRaw":"가상 발주","orderType":"AGENCY","clientId":"c",
            "recipients":[{"id":rid,"name":"가상","address":address,
                "phone":"","mobile":"","note":"","dispatchOrigin":"FACTORY",
                "items":[{"id":iid,"name":"709","raw":"709","productId":"p",
                    "variantId":"v","quantity":2,"quantityConfirmed":true,
                    "unitPrice":12000,"origin":"FACTORY","sources":[]}],
                "sources":[]}],"analysisEvidence":[]});
        let initialized=rule("initial_manual",json!({"order":order})).unwrap();
        let mut model=initialized["model"].clone();
        model["rows"][0]["items"][0]["quantity"]=json!(3);
        model["rows"][0]["items"][0]["quantityConfirmed"]=json!(true);
        model["rows"][0]["items"][0]["manualUnitPrice"]=json!(9900);
        // Deliberately forged UI field: the authoritative reducer must
        // ignore it and derive unitPrice from explicit manualUnitPrice.
        model["rows"][0]["items"][0]["unitPrice"]=json!(1);
        model["rows"][0]["remoteDelivery"]=json!({
            "address":address,"manual":true,"checked":true
        });
        model["rows"][0]["shippingCharges"]=json!({"FACTORY":3000});
        let state=json!({"products":[{"id":"p","active":true,
            "variants":[{"id":"v","wholesale":12000}]}],
            "clients":[{"id":"c","tier":"wholesale"}],"shipping":{"epoch":1}});
        let verified=verify_apply(
            &order,&state,&state["clients"][0],
            &json!({"sourceRaw":"가상 발주","orderType":"AGENCY",
                "clientId":"c","model":model,
                "assignmentOrder":initialized["assignmentOrder"]})
        ).unwrap();
        let item=&verified["recipients"][0]["items"][0];
        assert_eq!(item["quantity"],3);
        assert_eq!(item["manualUnitPrice"],9900);
        assert_eq!(item["unitPrice"],9900);
        assert_eq!(verified["recipients"][0]["shippingCharges"]["FACTORY"],3000);
        let quote=rules::execute("calculate_reception_quote",json!({
            "order":verified,"state":state,"client":state["clients"][0]
        })).unwrap();
        assert_eq!(quote["linePrices"][0]["amount"],29700);
        assert_eq!(quote["freightLines"][0]["appliedCharge"],6000);
        assert_eq!(quote["totals"]["total"],35700);
    }
    #[test]
    fn only_new_employee_ids_bypass_existing_scoped_ids(){
        assert_eq!(scoped("egui-1","recognition:recipient:0"),
            "egui-1:recognition:recipient:0");
        assert!(!valid_new_id("other-recipient-id"));
        assert!(valid_new_id("egui-manual-123-456"));
    }
}
