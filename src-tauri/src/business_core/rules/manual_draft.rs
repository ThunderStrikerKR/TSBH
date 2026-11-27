//! Native equivalent of the original TS manual source-block reducers.
use serde_json::{json,Value,Map};
use super::{field,rows,req,required_str,str_field,set,has_value};
fn utf16_slice(raw:&str,start:usize,end:usize)->String{
    let units:Vec<u16>=raw.encode_utf16().collect();
    if end>units.len() || start>end{return String::new();}
    String::from_utf16_lossy(&units[start..end])
}
fn manual_value(model:&Value,cell:&Value)->String{
    let Some(r)=rows(field(model,"rows")).iter()
        .find(|r|str_field(r,"id")==str_field(cell,"recipientId"))else{return String::new()};
    let v=if let Some(id)=cell.get("itemId").and_then(Value::as_str){
        let Some(i)=rows(field(r,"items")).iter().find(|i|str_field(i,"id")==id)
            else{return String::new()};
        if str_field(cell,"field")=="product"{field(i,"name")}else{field(i,"quantity")}
    }else{field(r,str_field(cell,"field"))};
    match v{Value::Null=>String::new(),Value::String(s)=>s.clone(),x=>x.to_string()}
}
fn assignment_order(payload:&Value,model:&Value)->Vec<String>{
    let keys=rows(field(payload,"assignmentOrder")).iter()
        .filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    if !keys.is_empty(){return keys;}
    field(model,"assignments").as_object().map(|m|m.keys().cloned().collect())
        .unwrap_or_default()
}
fn initial(payload:&Value)->Result<Value,String>{
    let order=req(payload,"order")?;
    if has_value(field(order,"confirmedAt")){return Err("정식 주문은 수동 분류로 변경할 수 없습니다.".into());}
    let raw=order.get("analysisRaw").and_then(Value::as_str)
        .unwrap_or_else(||str_field(order,"raw"));
    let recipients=rows(field(order,"recipients")).to_vec();
    let mut spans=vec![];
    for r in &recipients{
        spans.extend(rows(field(r,"sources")).iter().cloned());
        for c in rows(field(r,"candidates")){
            let mut s=c.clone();
            set(&mut s,"targetId",field(r,"id").clone())?;
            let a=field(c,"start").as_u64().unwrap_or(u64::MAX) as usize;
            let b=field(c,"end").as_u64().unwrap_or(u64::MAX) as usize;
            if b<=raw.encode_utf16().count() && a<=b{
                set(&mut s,"text",json!(utf16_slice(raw,a,b)))?;
            }
            spans.push(s);
        }
        for item in rows(field(r,"items")){spans.extend(rows(field(item,"sources")).iter().cloned());}
        for c in rows(field(r,"unassignedEvidence")){
            let mut s=c.clone();
            set(&mut s,"field",field(c,"kind").clone())?;
            set(&mut s,"targetId",field(r,"id").clone())?;
            spans.push(s);
        }
    }
    for c in rows(field(order,"analysisEvidence")){
        let mut s=c.clone();
        set(&mut s,"field",field(c,"kind").clone())?;
        let target=c.get("itemId").filter(|v|!v.is_null())
            .or_else(||c.get("recipientId").filter(|v|!v.is_null()))
            .unwrap_or_else(||field(order,"id"));
        set(&mut s,"targetId",target.clone())?;
        spans.push(s);
    }
    spans.sort_by(|a,b|field(a,"start").as_i64().unwrap_or(i64::MAX)
        .cmp(&field(b,"start").as_i64().unwrap_or(i64::MAX))
        .then_with(||field(b,"end").as_i64().unwrap_or(0)
            .cmp(&field(a,"end").as_i64().unwrap_or(0))));
    let mut blocks:Vec<Value>=vec![];
    let limit=raw.encode_utf16().count() as i64;
    for mut s in spans{
        let a=field(&s,"start").as_i64().unwrap_or(-1);
        let b=field(&s,"end").as_i64().unwrap_or(-1);
        if a<0||b<=a||b>limit||blocks.iter().any(|v|
            a<field(v,"end").as_i64().unwrap_or(0)&&b>field(v,"start").as_i64().unwrap_or(0)){
            continue;
        }
        set(&mut s,"id",json!(format!("{a}:{b}")))?;
        set(&mut s,"text",json!(utf16_slice(raw,a as usize,b as usize)))?;
        blocks.push(s);
    }
    let draft=json!({"rows":recipients,"blocks":blocks,"assignments":{}});
    let mut assignments=Map::new();
    let mut keys=vec![];
    for block in &blocks{
        let target=str_field(block,"targetId");
        let Some(r)=recipients.iter().find(|r|str_field(r,"id")==target||
            rows(field(r,"items")).iter().any(|i|str_field(i,"id")==target))else{continue};
        let mut cell=json!({"recipientId":field(r,"id"),"field":field(block,"field")});
        if let Some(item)=rows(field(r,"items")).iter().find(|i|str_field(i,"id")==target){
            set(&mut cell,"itemId",field(item,"id").clone())?;
        }
        if !["name","address","phone","mobile","note","product","quantity"]
            .contains(&str_field(&cell,"field")){continue;}
        let v=manual_value(&draft,&cell);
        if v.is_empty(){continue;}
        set(&mut cell,"value",json!(v))?;
        let id=str_field(block,"id").to_string();
        assignments.insert(id.clone(),cell);
        keys.push(id);
    }
    if let Some(saved)=order.get("manualClassification").filter(|v|!v.is_null()){
        let old=field(saved,"assignments");
        return Ok(json!({"model":{"rows":recipients,"blocks":field(saved,"blocks"),
            "assignments":old},"assignmentOrder":assignment_order(payload,&json!({"assignments":old}))}));
    }
    Ok(json!({"model":{"rows":recipients,"blocks":blocks,"assignments":assignments},
        "assignmentOrder":keys}))
}
fn set_item(payload:&Value,value:&str)->Result<Value,String>{
    let cell=req(payload,"cell")?;
    let mut model=req(payload,"model")?.clone();
    let rid=required_str(cell,"recipientId")?;
    let iid=required_str(cell,"itemId")?;
    let r=model["rows"].as_array_mut().ok_or("배송 행을 선택하세요.")?
        .iter_mut().find(|r|str_field(r,"id")==rid).ok_or("배송 행을 선택하세요.")?;
    let idx=rows(field(r,"items")).iter().position(|i|str_field(i,"id")==iid)
        .ok_or("상품 행을 선택하세요.")?;
    let previous=r["items"][idx].clone();
    let preserve=payload.get("preserveExistingQuantity")
        .and_then(Value::as_bool)==Some(true)
        && payload.get("product").is_some_and(|v|v.is_object());
    let mut item=if str_field(cell,"field")=="quantity"{
        let parsed=super::manual_classification::apply(&json!({"action":"parse_manual_quantity",
            "value":value}))?;
        let i=&mut r["items"][idx];
        let q=field(&parsed,"quantity");
        set(i,"quantity",q.clone())?;
        set(i,"quantityConfirmed",json!(!q.is_null()))?;
        i.as_object_mut().unwrap().remove("quantityApproval");
        if let Some(issues)=i.get_mut("issues").and_then(Value::as_array_mut){
            issues.retain(|x|!x.as_str().unwrap_or("").contains("수량"));
        }
        return Ok(json!({"model":model,"assignmentOrder":assignment_order(payload,req(payload,"model")?)}));
    }else if str_field(cell,"field")!="product"{
        return super::manual_classification::apply(&json!({"action":"set_manual_recipient_field",
            "model":model,"cell":cell,"value":value}));
    }else if let Some(product)=payload.get("product").filter(|v|!v.is_null()){
        let state=req(payload,"state")?;
        let client=rows(field(state,"clients")).iter().find(|c|
            str_field(c,"id")==str_field(req(payload,"order").unwrap_or(&Value::Null),"clientId"))
            .cloned().unwrap_or_else(||json!({"id":"","tier":"wholesale",
                "payment":"PREPAID","overrides":{},"shippingFeeExcluded":false}));
        // React passes the chosen independent variant and color to
        // create_catalog_item. Without these fields, a valid sole Black
        // variant may be rejected whenever defaultColor is absent.
        let variants=rows(field(product,"variants"));
        let variant=if variants.len()==1 &&
            field(product,"requiresOption")!=&Value::Bool(true){
            variants.first()
        }else{None};
        super::catalog_item::make(&json!({
            "product":product,"client":client,"newItemId":iid,"quantity":null,
            "variantId":variant.map(|v|field(v,"id")),
            "color":variant.and_then(|v|v.get("color"))
        }))?["item"].clone()
    }else if value.is_empty(){Value::Null}else{
        let state=req(payload,"state")?;
        let client=rows(field(state,"clients")).iter().find(|c|
            str_field(c,"id")==str_field(req(payload,"order").unwrap_or(&Value::Null),"clientId"))
            .cloned().unwrap_or_else(||json!({"id":"","tier":"wholesale",
                "payment":"PREPAID","overrides":{},"shippingFeeExcluded":false}));
        let analyzed=super::super::recognition::execute("analyze_order",
            json!({"raw":value,"client":client,"state":state}))?;
        let items=rows(field(&analyzed,"recipients")).iter()
            .flat_map(|r|rows(field(r,"items")).iter()).collect::<Vec<_>>();
        if items.len()>1{return Err("상품 셀에는 상품 하나를 입력하세요.".into());}
        items.first().map(|i|(*i).clone()).unwrap_or(Value::Null)
    };
    if item.is_null(){
        item=json!({"id":iid,"productId":null,"variantId":null,
            "raw":value,"name":value,"color":"","quantity":null,
            "unitPrice":null,"origin":"FACTORY","defaulted":false,
            "issues":[],"url":"","quantityConfirmed":false});
    }
    // Preserve the original parser.ts distinction between the raw line and
    // a leading/model-code label, even when no catalog item was resolved.
    if str_field(field(&item,"match"),"kind")=="UNKNOWN" {
        let model=regex::Regex::new(r"(?i)(?:BD\s*)?\d{3}[A-Za-z0-9-]*")
            .map_err(|e|e.to_string())?.find(value).map(|m|m.as_str().to_string());
        if let Some(code)=model {
            set(&mut item,"name",json!(code))?;
            set(&mut item["match"],"term",json!(code))?;
        }else{
            let re=regex::Regex::new(r"(?i)\s*(?:\d+|세|삼)\s*(?:개|대|ea).*")
                .map_err(|e|e.to_string())?;
            let name=re.replace(value,"").trim().to_string();
            set(&mut item,"name",json!(if name.is_empty(){"상품 확인 필요".to_string()}else{name}))?;
        }
    }
    // TS parseOrder allocates opaque UUIDs for candidate evidence. React
    // provides UUIDs, Rust owns candidate decisions and the final ordering.
    if let Some(candidates)=item.get_mut("productCandidates").and_then(Value::as_array_mut){
        let ids=rows(field(payload,"candidateIds"));
        for (i,candidate) in candidates.iter_mut().enumerate(){
            if let Some(id)=ids.get(i).and_then(Value::as_str){
                set(candidate,"id",json!(id))?;
            }
        }
    }
    set(&mut item,"id",json!(iid))?;
    // React's OrderItemEditor preserves the original evidence, item ID and
    // confirmed quantity when replacing only the catalog product. Preserve
    // the quantity assignment too; invalidate only the old product binding.
    set(&mut item,"raw",if preserve{
        field(&previous,"raw").clone()
    }else{json!(value)})?;
    set(&mut item,"quantity",if preserve{
        field(&previous,"quantity").clone()
    }else{Value::Null})?;
    set(&mut item,"quantityConfirmed",if preserve{
        field(&previous,"quantityConfirmed").clone()
    }else{json!(false)})?;
    set(&mut item,"manuallyConfirmed",
        json!(payload.get("product").is_some_and(|v|!v.is_null())))?;
    set(&mut item,"sources",if preserve{
        field(&previous,"sources").clone()
    }else{json!([])})?;
    if preserve && field(&previous,"origin").is_string(){
        set(&mut item,"origin",field(&previous,"origin").clone())?;
    }
    item.as_object_mut().ok_or("상품 데이터 오류")?.remove("quantityApproval");
    super::pricing::retain_manual_unit_price(&mut item,&previous)?;
    r["items"][idx]=item;
    model["assignments"].as_object_mut().ok_or("배정 정보 오류")?
        .retain(|_,v|str_field(v,"itemId")!=iid ||
            if preserve{
                str_field(v,"field")!="product"
            }else{
                !["product","quantity"].contains(&str_field(v,"field"))
            });
    let keys=assignment_order(payload,req(payload,"model")?).into_iter()
        .filter(|key|model["assignments"].get(key).is_some()).collect::<Vec<_>>();
    Ok(json!({"model":model,"assignmentOrder":keys}))
}
fn assign(payload:&Value)->Result<Value,String>{
    let model=req(payload,"model")?;
    let id=required_str(payload,"id")?;
    if model["assignments"].get(id).is_some(){
        return Err("이미 배정한 블록입니다. 먼저 연결을 회수하세요.".into());
    }
    let block=rows(field(model,"blocks")).iter().find(|b|str_field(b,"id")==id)
        .ok_or("원문 블록을 확인하세요.")?;
    let cell=req(payload,"cell")?;
    let has_item=cell.get("itemId").is_some_and(|v|!v.is_null());
    let target_field=str_field(cell,"field");
    let value=str_field(block,"text");
    // The original recognition type is advisory. Only the target's
    // supported field and value syntax constrain an explicit assignment.
    if (has_item && !["product","quantity"].contains(&target_field)) ||
        (!has_item && !["name","address","mobile","phone","note"].contains(&target_field)){
        return Err("원문을 배정할 수 없는 입력 셀입니다.".into());
    }
    if has_item && target_field=="quantity"{
        let parsed=super::manual_classification::apply(&json!({
            "action":"parse_manual_quantity","value":value
        }))?;
        if field(&parsed,"quantity").is_null(){
            return Err("원문에서 유효한 수량을 확인할 수 없습니다.".into());
        }
    }
    let mut call=payload.clone();
    set(&mut call,"cell",cell.clone())?;
    let next=if has_item{
        set_item(&call,str_field(block,"text"))?
    }else{
        super::manual_classification::apply(&json!({"action":"set_manual_recipient_field",
            "model":model,"cell":cell,"value":field(block,"text")}))?
    };
    let mut updated=next["model"].clone();
    let target=format!("{}:{}:{}",str_field(cell,"recipientId"),
        str_field(cell,"itemId"),str_field(cell,"field"));
    let removed=updated["assignments"].as_object().ok_or("배정 정보 오류")?
        .iter().filter(|(_,v)|format!("{}:{}:{}",str_field(v,"recipientId"),
            str_field(v,"itemId"),str_field(v,"field"))==target)
        .map(|(k,_)|k.clone()).collect::<Vec<_>>();
    for key in &removed{updated["assignments"].as_object_mut().unwrap().remove(key);}
    let mut assignment=cell.clone();
    set(&mut assignment,"value",json!(manual_value(&updated,cell)))?;
    updated["assignments"].as_object_mut().unwrap().insert(id.to_owned(),assignment);
    let mut keys=assignment_order(payload,model).into_iter()
        .filter(|k|!removed.contains(k) && updated["assignments"].get(k).is_some())
        .collect::<Vec<_>>();
    keys.push(id.to_owned());
    Ok(json!({"model":updated,"assignmentOrder":keys}))
}
fn release(payload:&Value)->Result<Value,String>{
    let model=req(payload,"model")?;
    let id=required_str(payload,"id")?;
    let Some(assigned)=model["assignments"].get(id)else{
        return Ok(json!({"model":model,"assignmentOrder":assignment_order(payload,model)}));
    };
    let mut next=if field(payload,"clear").as_bool()==Some(true){
        if assigned.get("itemId").is_some_and(|v|!v.is_null()){
            let mut call=payload.clone();
            set(&mut call,"cell",assigned.clone())?;
            set_item(&call,"")?
        }else{
            super::manual_classification::apply(&json!({"action":"set_manual_recipient_field",
                "model":model,"cell":assigned,"value":""}))?
        }
    }else{json!({"model":model})};
    next["model"]["assignments"].as_object_mut().ok_or("배정 정보 오류")?.remove(id);
    let keys=assignment_order(payload,model).into_iter()
        .filter(|k|next["model"]["assignments"].get(k).is_some())
        .collect::<Vec<_>>();
    next["assignmentOrder"]=json!(keys);
    Ok(next)
}
pub(super) fn execute(payload:&Value)->Result<Value,String>{
    match required_str(payload,"action")?{
        "initial_manual"=>initial(payload),
        "set_manual_product"=>{
            // Clearing a wrong manual product is deliberate and valid.
            // set_item already handles the empty string as an unknown item;
            // required_str incorrectly rejected that supported branch.
            let value=payload.get("value").and_then(Value::as_str)
                .ok_or("상품 셀의 입력값은 문자열이어야 합니다.")?;
            set_item(payload,value)
        },
        "assign_manual_block"=>assign(payload),
        "release_manual_block"=>release(payload),
        action=>Err(format!("수동 분류 초안 명령이 없습니다: {action}"))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn detects_source_overlap(){
        let order=json!({"id":"o","raw":"김가상 😀 709",
            "recipients":[{"id":"r","name":"김가상","items":[],
                "sources":[{"start":0,"end":3,"targetId":"r","field":"name"},
                    {"start":0,"end":6,"targetId":"r","field":"name"}]}]});
        let x=initial(&json!({"order":order})).unwrap();
        assert_eq!(x["model"]["blocks"].as_array().unwrap().len(),1);
        assert_eq!(x["model"]["blocks"][0]["id"],"0:6");
    }
    #[test]
    fn explicit_catalog_replacement_keeps_existing_quantity_and_source(){
        let product=json!({"id":"p2","name":"BD 709","origin":"FACTORY",
            "variants":[{"id":"v2","color":"Black","wholesale":59000}],
            "requiresOption":false});
        let item=json!({"id":"i1","name":"예전 상품","raw":"원문상품 4개",
            "productId":"p1","variantId":"v1","quantity":4,
            "quantityConfirmed":true,"sources":[{"start":0,"end":6}],
            "origin":"STORE","issues":[]});
        let model=json!({"rows":[{"id":"r1","items":[item]}],
            "blocks":[],"assignments":{
                "product":{"recipientId":"r1","itemId":"i1","field":"product"},
                "quantity":{"recipientId":"r1","itemId":"i1","field":"quantity"}
            }});
        let next=execute(&json!({
            "action":"set_manual_product","model":model,
            "order":{"clientId":"c1"},
            "state":{"clients":[{"id":"c1","tier":"wholesale"}]},
            "cell":{"recipientId":"r1","itemId":"i1","field":"product"},
            "value":"BD 709","product":product,
            "preserveExistingQuantity":true,
            "assignmentOrder":["product","quantity"]
        })).unwrap();
        let item=&next["model"]["rows"][0]["items"][0];
        assert_eq!(item["productId"],"p2");
        assert_eq!(item["variantId"],"v2");
        assert_eq!(item["quantity"],4);
        assert_eq!(item["quantityConfirmed"],true);
        assert_eq!(item["raw"],"원문상품 4개");
        assert_eq!(item["origin"],"STORE");
        assert_eq!(item["sources"],json!([{"start":0,"end":6}]));
        assert!(next["model"]["assignments"].get("product").is_none());
        assert!(next["model"]["assignments"].get("quantity").is_some());
    }

    #[test]
    fn cross_type_assignment_preserves_original_type_and_validates_target_value(){
        let model=json!({"rows":[{"id":"r","name":"","items":[
            {"id":"i","name":"상품","quantity":null,"quantityConfirmed":false}
        ]}],"blocks":[
            {"id":"0:2","text":"2개","field":"product"},
            {"id":"3:6","text":"김가상","field":"address"},
            {"id":"7:10","text":"서울시","field":"product"}
        ],"assignments":{}});
        let result=assign(&json!({"model":model,"id":"0:2","assignmentOrder":[],
            "cell":{"recipientId":"r","itemId":"i","field":"quantity"}})).unwrap();
        assert_eq!(result["model"]["rows"][0]["items"][0]["quantity"],2);
        assert_eq!(result["model"]["blocks"][0]["field"],"product");
        assert_eq!(result["model"]["assignments"]["0:2"]["field"],"quantity");
        let named=assign(&json!({"model":result["model"],"id":"3:6",
            "assignmentOrder":result["assignmentOrder"],
            "cell":{"recipientId":"r","field":"name"}})).unwrap();
        assert_eq!(named["model"]["rows"][0]["name"],"김가상");
        assert_eq!(named["model"]["blocks"][1]["field"],"address");
        assert_eq!(named["model"]["assignments"]["3:6"]["field"],"name");
        let invalid=json!({"model":named["model"],"id":"7:10",
            "assignmentOrder":named["assignmentOrder"],
            "cell":{"recipientId":"r","itemId":"i","field":"quantity"}});
        assert!(assign(&invalid).is_err());
        assert_eq!(invalid["model"]["rows"][0]["items"][0]["quantity"],2);
        assert!(invalid["model"]["assignments"].get("7:10").is_none());
        let mut unsupported=invalid.clone();
        unsupported["cell"]["field"]=json!("unitPrice");
        assert!(assign(&unsupported).is_err());
    }

    #[test]fn blocks_reject_duplicate_binding(){
        let model=json!({"rows":[{"id":"r","name":"","items":[]}],
            "blocks":[{"id":"0:3","text":"가상명","field":"name"}],
            "assignments":{"0:3":{"recipientId":"r","field":"name","value":"가상명"}}});
        assert!(assign(&json!({"model":model,"id":"0:3",
            "cell":{"recipientId":"r","field":"name"}})).is_err());
    }
}
