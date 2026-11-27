//! Source-grounded correction candidates only. Never installs recognition rules.
use serde_json::{json,Value};
use std::collections::HashSet;
use super::{model,quantity,utf16};

fn rows(v:&Value)->impl Iterator<Item=&Value>{v.as_array().into_iter().flatten()}
fn text<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn find<'a>(v:&'a Value,id:&str)->Option<&'a Value>{
    if id.is_empty(){return None;}
    rows(v).find(|r|text(r,"id")==id)
}
fn target<'a>(rs:&'a Value,cell:&Value)->Option<&'a Value>{
    let row=find(rs,text(cell,"recipientId"))?;
    if let Some(id)=cell["itemId"].as_str(){find(&row["items"],id)}else{Some(row)}
}
fn supported(cell:&Value)->bool{
    if cell["itemId"].is_string(){matches!(text(cell,"field"),"product"|"quantity")}
    else{matches!(text(cell,"field"),"name"|"address"|"mobile"|"phone"|"note")}
}
fn value_at(rs:&Value,cell:&Value)->Value{
    let Some(t)=target(rs,cell) else{return Value::Null};
    match text(cell,"field"){
        "product"=>json!({"productId":t["productId"],"variantId":t["variantId"],"name":t["name"]}),
        "quantity"=>t["quantity"].clone(),
        field=>t[field].clone(),
    }
}
fn present(v:&Value)->bool{
    !v.is_null() && v.as_str().is_none_or(|s|!s.trim().is_empty())
}
fn span(raw:&str,block:&Value)->Option<(usize,usize)>{
    let a=usize::try_from(block["start"].as_u64()?).ok()?;
    let b=usize::try_from(block["end"].as_u64()?).ok()?;
    if b<=a{return None;}
    let byte=|offset:usize|{
        let mut n=0;
        for (i,ch) in raw.char_indices(){
            if n==offset{return Some(i);}
            n+=ch.len_utf16();
        }
        (n==offset).then_some(raw.len())
    };
    let (start,end)=(byte(a)?,byte(b)?);
    (raw.get(start..end)?==text(block,"text")).then_some((start,end))
}
fn cell_key(c:&Value)->(String,String,String){
    (text(c,"recipientId").into(),text(c,"itemId").into(),text(c,"field").into())
}
fn candidate(raw:&str,event:&Value,cell:&Value,kind:&str,method:&str,
    start:usize,end:usize,old_field:Option<&str>,before:&Value,after:&Value)->Value{
    let left=raw[..start].rfind('\n').map_or(0,|p|p+1);
    let right=end+raw[end..].find('\n').unwrap_or(raw.len()-end);
    // Keep actual source and the containing line; never create replacement text.
    json!({"auditId":event["id"],"orderId":event["orderId"],
        "recordedAt":event["at"],"clientId":event["context"]["clientId"],
        "clientSelection":event["context"]["clientSelection"],
        "kind":kind,"method":method,"requiresReview":true,
        "source":{"start":utf16(raw,start),"end":utf16(raw,end),"text":&raw[start..end],
            "line":&raw[left..right]},
        "originalField":old_field,"target":cell,"beforeValue":before,"afterValue":after,
        "initialRecognition":event["context"]["initialRecognition"]})
}
fn unique_literal(raw:&str,value:&str)->Option<(usize,usize)>{
    let v=value.trim();
    if v.is_empty(){return None;}
    let mut matches=raw.match_indices(v).filter(|(a,s)|{
        let b=*a+s.len();
        let first=v.chars().next().unwrap();
        let last=v.chars().next_back().unwrap();
        let left=raw[..*a].chars().next_back();
        let right=raw[b..].chars().next();
        !(first.is_alphanumeric() && left.is_some_and(char::is_alphanumeric)) &&
        !(last.is_alphanumeric() && right.is_some_and(char::is_alphanumeric))
    });
    let (a,s)=matches.next()?;
    if matches.next().is_some(){return None;}
    Some((a,a+s.len()))
}
fn unique_model(raw:&str,item:&Value)->Option<(usize,usize)>{
    let mut spans=HashSet::new();
    // Reuse the native catalog-name/model-code boundary matcher. Historical
    // aliases are not reconstructed from a different current catalog revision.
    for term in model::model_names(item){
        let mut offset=0;
        while let Some((a,b))=model::find_term(&raw[offset..],&term,true){
            if b==0{break;}
            spans.insert((offset+a,offset+b));
            offset+=b;
            if spans.len()>1{return None;}
        }
    }
    if spans.len()==1{spans.into_iter().next()}else{None}
}
fn direct_span(raw:&str,after:&Value,cell:&Value,value:&Value)->Option<(usize,usize)>{
    match text(cell,"field"){
        "product"=>unique_model(raw,target(after,cell)?),
        "quantity"=>{
            let q=value.as_i64().filter(|q|*q>0)?;
            let item=target(after,cell)?;
            if item["quantityConfirmed"]!=true{return None;}
            let (a,b)=unique_model(raw,item)?;
            let left=raw[..a].rfind('\n').map_or(0,|p|p+1);
            let right=b+raw[b..].find('\n').unwrap_or(raw.len()-b);
            // Multiple quantities or repeated models are ambiguous. No
            // nearest-neighbour guess becomes a learning label.
            let matches=quantity::extract(&raw[left..right]);
            if matches.len()!=1 || matches[0].value!=q{return None;}
            Some((left+matches[0].start,left+matches[0].end))
        }
        _=>unique_literal(raw,value.as_str()?),
    }
}
fn direct_cells(after:&Value)->Vec<Value>{
    let mut out=Vec::new();
    for r in rows(after){
        let rid=text(r,"id");
        if rid.is_empty(){continue;}
        for field in ["name","address","mobile","phone","note"]{
            out.push(json!({"recipientId":rid,"field":field}));
        }
        for i in rows(&r["items"]){
            let iid=text(i,"id");
            if iid.is_empty(){continue;}
            for field in ["product","quantity"]{
                out.push(json!({"recipientId":rid,"itemId":iid,"field":field}));
            }
        }
    }
    out
}
pub(super) fn extract(event:&Value)->Result<Value,String>{
    let raw=event["sourceRaw"].as_str().filter(|s|!s.trim().is_empty())
        .ok_or("정정 원문이 없습니다.")?;
    let before=&event["beforeRows"];
    let after=&event["afterRows"];
    if !before.is_array() || !after.is_array() || !event["initialAnalysis"]["recipients"].is_array(){
        return Err("정정 이력 스냅샷을 확인하세요.".into());
    }
    let mut candidates=Vec::new();
    let mut explicit=HashSet::new();
    let ctx=&event["context"];
    if ctx["schemaVersion"]==1{
        let old=&ctx["beforeClassification"];
        let new=&ctx["afterClassification"];
        if let Some(assignments)=new["assignments"].as_object(){
            for (id,cell) in assignments{
                if !supported(cell){continue;}
                let Some(block)=find(&new["blocks"],id) else{continue};
                let Some(prior_block)=find(&old["blocks"],id) else{continue};
                if prior_block["field"]!=block["field"] || prior_block["text"]!=block["text"]
                    || prior_block["start"]!=block["start"] || prior_block["end"]!=block["end"]{continue;}
                let Some((a,b))=span(raw,block) else{continue};
                let old_assignment=old["assignments"].get(id);
                let changed=old_assignment.is_none_or(|c|cell_key(c)!=cell_key(cell));
                if !changed{continue;}
                let v=value_at(after,cell);
                if !present(&v) || (text(cell,"field")=="product" && !present(&v["name"])){continue;}
                if text(cell,"field")=="quantity" && target(after,cell)
                    .is_none_or(|t|t["quantityConfirmed"]!=true){continue;}
                // A subsequent direct edit can leave the old assignment
                // object behind. Do not promote a stale link.
                let final_label=if text(cell,"field")=="product"{v["name"].clone()}else{v.clone()};
                let label=final_label.as_str().map(str::to_owned).unwrap_or_else(||final_label.to_string());
                if cell["value"].as_str()!=Some(label.as_str()){continue;}
                let initial=text(prior_block,"field");
                let kind=if initial!=text(cell,"field"){"type_change"}
                    else if old_assignment.is_some(){"assignment_change"}else{"unbound_assignment"};
                candidates.push(candidate(raw,event,cell,kind,"explicit_binding",a,b,
                    Some(initial),&value_at(before,cell),&v));
                explicit.insert(cell_key(cell));
            }
        }
    }
    for cell in direct_cells(after){
        if explicit.contains(&cell_key(&cell)){continue;}
        let old=value_at(before,&cell);
        let new=value_at(after,&cell);
        if old==new || !present(&new) || (text(&cell,"field")=="product" && !present(&new["name"])){continue;}
        let Some((a,b))=direct_span(raw,after,&cell,&new)else{continue};
        let kind=if old.is_null() || old.as_str()==Some("") ||
            (text(&cell,"field")=="product" && !present(&old["name"])){
                "direct_missing"
            }else{"direct_changed"};
        candidates.push(candidate(raw,event,&cell,kind,"source_matched_edit",a,b,None,&old,&new));
    }
    Ok(json!({"candidates":candidates,"legacyContext":ctx["schemaVersion"]!=1}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn explicit_type_change_uses_exact_utf16_span_and_identical_values(){
        let raw="🎵709 2개";
        let block=json!({"id":"2:5","start":2,"end":5,"text":"709","field":"quantity"});
        let rs=json!([{"id":"r","items":[{"id":"i","name":"BD 709","productId":"p","quantity":2,"quantityConfirmed":true}]}]);
        let e=json!({"id":"a","orderId":"o","sourceRaw":raw,"initialAnalysis":{"recipients":rs},
            "beforeRows":rs,"afterRows":rs,"context":{"schemaVersion":1,
            "beforeClassification":{"blocks":[block],"assignments":{}},
            "afterClassification":{"blocks":[block],"assignments":{"2:5":{
                "recipientId":"r","itemId":"i","field":"product","value":"BD 709"}}}}});
        let out=extract(&e).unwrap();
        assert_eq!(out["candidates"].as_array().unwrap().len(),1);
        assert_eq!(out["candidates"][0]["kind"],"type_change");
        assert_eq!(out["candidates"][0]["source"]["start"],2);
        let mut invalid=e.clone();
        invalid["context"]["afterClassification"]["blocks"][0]["text"]=json!("708");
        assert!(extract(&invalid).unwrap()["candidates"].as_array().unwrap().is_empty());
    }
    #[test]
    fn direct_missing_product_and_quantity_but_not_external_address(){
        let before=json!([{"id":"r","address":"서울시 테헤란로 123","items":[]}]);
        let after=json!([{"id":"r","address":"서울시 테헤란로 123 502호","items":[
            {"id":"i","name":"BD 709","productId":"p","quantity":3,"quantityConfirmed":true}]}]);
        let e=json!({"id":"a","orderId":"o","sourceRaw":"서울시 테헤란로 123\n709 3개",
            "initialAnalysis":{"recipients":before},"beforeRows":before,"afterRows":after});
        let out=extract(&e).unwrap();
        assert_eq!(out["candidates"].as_array().unwrap().len(),2);
        assert!(out["legacyContext"]==true);
        assert!(rows(&out["candidates"]).all(|v|v["kind"]=="direct_missing"));
        let mut ambiguous=e.clone();
        ambiguous["sourceRaw"]=json!("709 3개\n709 3개");
        assert!(extract(&ambiguous).unwrap()["candidates"].as_array().unwrap().is_empty());
    }
}
