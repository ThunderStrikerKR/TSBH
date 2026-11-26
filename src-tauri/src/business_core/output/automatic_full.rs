//! Full auto-message projection from saved/preview State, without the TS
//! business-formatting functions. The React caller owns only draft editing.
use serde_json::{json,Value};
use std::collections::HashSet;
use super::messages::{payment,render,url_is_official};
use super::automatic_request;
use super::super::rules;

fn string<'a>(v:&'a Value,key:&str)->&'a str{
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows<'a>(v:&'a Value,key:&str)->&'a [Value]{
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn by_id<'a>(list:&'a [Value],id:&str)->Option<&'a Value>{
    list.iter().find(|v|string(v,"id")==id)
}
fn not_removed(v:&Value)->bool{
    v.get("deletedAt").is_none_or(Value::is_null)
}
fn is_true(v:&Value,key:&str)->bool{v.get(key).and_then(Value::as_bool)==Some(true)}
fn opt_i64(v:&Value,key:&str)->Option<i64>{v.get(key).and_then(Value::as_i64)}
fn distinct(values:impl IntoIterator<Item=String>)->Vec<String>{
    let mut seen=HashSet::new();
    values.into_iter().filter(|s|seen.insert(s.to_string())).collect()
}
fn resolved_client<'a>(state:&'a Value,order:&'a Value)->Option<&'a Value>{
    if (order.get("confirmedAt").is_some_and(|x|!x.is_null()) ||
        is_true(order,"pricesLocked")) &&
        order.get("clientSnapshot").is_some_and(Value::is_object) {
        return order.get("clientSnapshot");
    }
    by_id(rows(state,"clients"),string(order,"clientId"))
        .or_else(||order.get("clientSnapshot").filter(|c|c.is_object()))
}
fn account_for<'a>(state:&'a Value,view:&Value,client:&Value)->Option<&'a Value>{
    let settings=&state["settings"];
    let accounts=rows(&settings,"accounts");
    if string(view,"accountMode")=="MANUAL"{
        return by_id(accounts,string(view,"accountId"));
    }
    if string(view,"orderType")!="RETAIL"{
        let preferred=client.get("preferredAccountId")
            .and_then(Value::as_str)
            .or_else(||view.get("clientSnapshot")
                .and_then(|s|s.get("preferredAccountId"))
                .and_then(Value::as_str)).unwrap_or("");
        if let Some(a)=by_id(accounts,preferred){return Some(a);}
    }
    let items=rows(view,"recipients").iter()
        .flat_map(|r|rows(r,"items")).collect::<Vec<_>>();
    let mall=!items.is_empty()&&items.iter().all(|i|{
        let catalog=i.get("catalog").and_then(Value::as_str).unwrap_or("");
        if !catalog.is_empty(){return catalog=="BANDOMALL";}
        by_id(rows(state,"products"),string(i,"productId"))
            .is_some_and(|p|string(p,"catalog")=="BANDOMALL")
    });
    let brand=&settings["brandAccounts"];
    let chosen=if mall{string(brand,"BANDOMALL")}
        else{
            let branded=string(brand,"BANDOSTAND");
            if branded.is_empty(){string(settings,"defaultAccountId")}else{branded}
        };
    by_id(accounts,chosen)
}
fn group_view(state:&Value,group_id:&str)->Result<(Value,Vec<(Value,Value)>,i64),String>{
    let workflow=&state["workflow"];
    let group=by_id(rows(workflow,"groups"),group_id)
        .ok_or("현재 주문 묶음을 찾지 못했습니다.")?;
    if !not_removed(group){return Err("묶음의 배송 건이 없습니다.".into());}
    let ships=rows(workflow,"shipments").iter().filter(|s|
        string(s,"groupId")==group_id&&!is_true(s,"legacyArchived")&&
        !by_id(rows(state,"orders"),string(s,"orderId"))
            .is_some_and(|o|!not_removed(o))).collect::<Vec<_>>();
    if ships.is_empty(){return Err("묶음의 배송 건이 없습니다.".into());}
    let first=by_id(rows(state,"orders"),string(ships[0],"orderId"))
        .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    let allowed=ships.iter().map(|s|string(s,"recipientId")).collect::<HashSet<_>>();
    let recipients=rows(first,"recipients").iter().filter(|r|
        allowed.contains(string(r,"id"))).cloned().collect::<Vec<_>>();
    let mut contexts=Vec::with_capacity(recipients.len());
    for recipient in &recipients{
        let ship=ships.iter().find(|s|string(s,"recipientId")==string(recipient,"id"))
            .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
        let amount=rules::execute("calculate_shipment_totals",json!({
            "state":state,"shipmentId":ship["id"]
        }))?;
        contexts.push(((*ship).clone(),amount));
    }
    let shipping=contexts.iter()
        .map(|(_,a)|opt_i64(a,"shipping"))
        .collect::<Option<Vec<_>>>()
        .and_then(|v|v.into_iter().try_fold(0_i64,|sum,n|
            sum.checked_add(n).filter(|x|(0..=9_007_199_254_740_991).contains(x))));
    let step=ships.iter().filter_map(|s|opt_i64(s,"step")).min().unwrap_or(0);
    let mut view=first.clone();
    view["recipients"]=json!(recipients);
    view["shippingFee"]=json!(shipping);
    view["pricesLocked"]=json!(false);
    view["vatAmount"]=Value::Null;
    view["accountMode"]=group.get("accountMode")
        .filter(|v|!v.is_null())
        .cloned().unwrap_or_else(||first.get("accountMode").cloned().unwrap_or(Value::Null));
    if string(group,"accountMode")=="MANUAL"{
        view["accountId"]=group.get("accountId").cloned().unwrap_or(Value::Null);
    }
    Ok((view,contexts,step))
}
fn provisional_notice(state:&Value,item:&Value)->String{
    let pid=string(item,"productId");
    let vid=string(item,"variantId");
    if pid.is_empty()||vid.is_empty(){return String::new();}
    let evidence=&item["selectionEvidence"];
    if string(evidence,"productId")!=pid||string(evidence,"variantId")!=vid{
        return String::new();
    }
    let Some(p)=by_id(rows(state,"products"),pid)else{return String::new()};
    let lead=string(p,"name").split('/').next().unwrap_or("").trim();
    if lead.is_empty(){return String::new();}
    let variant=regex::Regex::new(r"(?i)^BD[\s-]+").expect("brand prefix");
    let bare=variant.replace(lead,"").to_string();
    let names=if bare==lead{vec![lead.to_string()]}
        else{vec![lead.to_string(),bare]};
    let expression=string(evidence,"expression");
    for name in names {
        let mut models=vec![name.trim().to_string()];
        let shortened=regex::Regex::new(r"(?i)^BD[\s-]*")
            .expect("prefix").replace(name.trim(),"").to_string();
        if !models.contains(&shortened){models.push(shortened);}
        for text in models.into_iter().filter(|t|!t.is_empty()){
            let pattern=text.split_whitespace().map(regex::escape)
                .collect::<Vec<_>>().join(r"\s*");
            let Ok(re)=regex::RegexBuilder::new(&pattern).case_insensitive(true).build()
                else{continue};
            for m in re.find_iter(expression){
                let before=expression[..m.start()].chars().next_back();
                let after=expression[m.end()..].chars().next();
                if before.is_none_or(|ch|!ch.is_ascii_alphanumeric()) &&
                    after.is_none_or(|ch|!ch.is_ascii_alphanumeric()&&ch!='-'){
                    return String::new();
                }
            }
        }
    }
    string(&state["settings"]["provisionalProductNotices"],pid).trim().to_string()
}
fn price_issues(view:&Value,client:&Value,account:Option<&Value>,
    amounts:&[(Value,Value)])->Result<Vec<String>,String>{
    let mut issues=Vec::new();
    if string(client,"id").is_empty(){issues.push("거래처 선택 필요".into());}
    let order_issues=rules::execute("order_issues",json!({"order":view}))?;
    for issue in rows(&order_issues,"issues").iter().filter(|i|
        string(i,"severity")=="ACTION"&&
        opt_i64(i,"blockingStage").is_some_and(|n|n<=1)){
        let label=if !string(issue,"itemId").is_empty(){
            rows(view,"recipients").iter().flat_map(|r|rows(r,"items"))
                .find(|item|string(item,"id")==string(issue,"itemId"))
                .map(|item|string(item,"name")).filter(|s|!s.is_empty())
                .unwrap_or("상품").to_owned()
        }else{
            let index=rows(view,"recipients").iter().position(|r|
                string(r,"id")==string(issue,"recipientId"));
            format!("수취인 {}",index.map(|n|n+1).unwrap_or(0))
        };
        issues.push(format!("{label}: {}",string(issue,"title")));
    }
    let items=rows(view,"recipients").iter().flat_map(|r|rows(r,"items"))
        .collect::<Vec<_>>();
    if items.iter().any(|i|string(i,"productId").is_empty() ||
        string(i,"variantId").is_empty()){
        issues.push("제품명이 확인되지 않습니다.".into());
    }
    if !string(client,"id").is_empty() &&
        items.iter().any(|i|i.get("unitPrice").is_none_or(Value::is_null)){
        issues.push("판매가 확인 필요".into());
    }
    // Exact original domain.messages uses totals(groupOrder, client).shipping
    // before a separately computed per-shipment sufficiency filter.
    let result=rules::execute("calculate_price",json!({"order":view,"client":client}))?;
    if result.get("shipping").is_none_or(Value::is_null){
        issues.push("운임 확인 필요".into());
    }
    if account.is_none_or(|a|["bank","number","holder"].iter().any(|k|
        string(a,k).trim().is_empty())){
        issues.push("입금 계좌 등록 필요".into());
    }
    let all_shipping=amounts.iter().all(|(_,a)|a.get("shipping").is_some_and(|v|!v.is_null()));
    issues=distinct(issues);
    if all_shipping{issues.retain(|issue|issue!="운임 확인 필요");}
    if amounts.iter().any(|(_,a)|a.get("shipping").is_none_or(Value::is_null)){
        issues.push("배송 건별 운임 확인 필요".into());
    }
    Ok(distinct(issues))
}
fn payment_input(state:&Value,view:&Value,amounts:&[(Value,Value)],
    account:Option<&Value>,include_links:bool)->Result<Value,String>{
    let client=resolved_client(state,view).cloned().unwrap_or_else(||json!({
        "id":"","shippingFeeExcluded":false
    }));
    let mut recipients=Vec::new();
    for (index,row) in rows(view,"recipients").iter().enumerate(){
        let (ship,amount)=amounts.get(index).ok_or("배송 건 연결을 확인하세요.")?;
        // Use the same effective party (including the confirmed snapshot)
        // as the workflow billing projection, not the numerical zero.
        let original=by_id(rows(state,"orders"),string(ship,"orderId")).unwrap_or(view);
        let exempt=rules::workflow_party(state,original)
            .is_some_and(|party|is_true(party,"shippingFeeExcluded"));
        let items=rows(row,"items").iter().map(|item|{
            let mut item=item.clone();
            let amount=match (opt_i64(&item,"unitPrice"),opt_i64(&item,"quantity")){
                (Some(price),Some(quantity))=>price.checked_mul(quantity)
                    .filter(|v|(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(v)),
                _=>None
            };
            item["amount"]=json!(amount);
            let product=by_id(rows(state,"products"),string(&item,"productId"));
            item["colorNotRequired"]=json!(product.is_some_and(|p|
                string(p,"colorStatus")=="NOT_REQUIRED"));
            item["provisionalNotice"]=json!(provisional_notice(state,&item));
            item
        }).collect::<Vec<_>>();
        let freight=row.get("freightCode").and_then(Value::as_str)
            .unwrap_or_else(||string(view,"freightCode"));
        let vat=row.get("vatOverride").and_then(Value::as_bool)
            .or_else(||row.get("vatDefault").and_then(Value::as_bool))
            .unwrap_or_else(||is_true(view,"applyVat"));
        recipients.push(json!({
            "name":string(row,"name"),"items":items,
            "freightCode":freight,"freightExempt":exempt,"applyVat":vat,
            "shipping":amount.get("shipping"),"vat":amount.get("vat")
        }));
    }
    let totals=amounts.iter().map(|(_,a)|opt_i64(a,"total"))
        .collect::<Option<Vec<_>>>()
        .and_then(|xs|xs.into_iter().try_fold(0_i64,|s,n|s.checked_add(n)
            .filter(|v|(0..=9_007_199_254_740_991).contains(v))));
    Ok(json!({
        "mode":"payment","settings":state["settings"],
        "recipients":recipients,"total":totals,
        "clientSelected":!string(&client,"id").is_empty(),
        "account":account.unwrap_or(&Value::Null),
        "includeLinks":include_links
    }))
}
fn payment_signature(view:&Value,payment_text:&str,account:Option<&Value>,
    account_wire:&[Value])->Result<String,String>{
    let items=rows(view,"recipients").iter().map(|row|
        rows(row,"items").iter().map(|i|json!([
            i.get("productId"),i.get("variantId"),i.get("quantity"),i.get("unitPrice")
        ])).collect::<Vec<_>>()).collect::<Vec<_>>();
    let text=serde_json::to_string(payment_text).map_err(|e|e.to_string())?;
    let item_text=serde_json::to_string(&items).map_err(|e|e.to_string())?;
    let account_text=if let Some(account)=account{
        let wire=account_wire.iter().find(|record|
            string(record,"id")==string(account,"id")).and_then(|r|r.get("json"))
            .and_then(Value::as_str)
            .and_then(|raw|serde_json::from_str::<Value>(raw).ok()
                .filter(|parsed|parsed==account).map(|_|raw.to_string()));
        Some(wire.unwrap_or_else(||serde_json::to_string(account).unwrap_or_default()))
    }else{None};
    Ok(if let Some(raw)=account_text{
        format!(r#"{{"text":{text},"account":{raw},"items":{item_text}}}"#)
    }else{
        format!(r#"{{"text":{text},"items":{item_text}}}"#)
    })
}
/** The immutable workflow notice stores the FULL payment text, including the
 * greeting. The editable automatic-message preview deliberately removes that
 * greeting, so never persist its preview "text" as an audit notice.
 * Source oracle: workflow.ts::groupMessages(state, gid).payment. */
pub(super) fn payment_notice(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("메시지 생성 상태가 없습니다.")?;
    let gid=string(payload,"groupId");
    if gid.is_empty(){return Err("묶음을 선택하세요.".into());}
    let (view,amounts,_)=group_view(state,gid)?;
    let client=resolved_client(state,&view).cloned().unwrap_or_else(||json!({
        "id":"","shippingFeeExcluded":false
    }));
    let account=account_for(state,&view,&client);
    let rendered=payment(&payment_input(state,&view,&amounts,account,false)?,
        &state["settings"])?;
    Ok(json!({"payment":rendered["payment"]}))
}
/** Keep the original copy-only greeting helper in Rust too: request drafts
 * remain staff-editable, so this operation accepts the final edited text.
 */
pub(super) fn decorate_copy(payload:&Value)->Result<Value,String>{
    let original=payload.get("text").and_then(Value::as_str)
        .ok_or("복사할 메시지 본문이 없습니다.")?;
    let include_greeting=is_true(payload,"includeGreeting");
    let decorated=if include_greeting &&
        !original.trim_start().starts_with("안녕하세요 반도스탠드입니다."){
        format!("안녕하세요 반도스탠드입니다.\n\n{original}")
    }else{original.to_string()};
    Ok(json!({"text":decorated}))
}
pub(super) fn execute(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("메시지 생성 상태가 없습니다.")?;
    let gid=string(payload,"groupId");
    if gid.is_empty(){return Err("묶음을 선택하세요.".into());}
    let include_links=is_true(payload,"includeLinks");
    let (view,amounts,step)=group_view(state,gid)?;
    let client=resolved_client(state,&view).cloned().unwrap_or_else(||json!({
        "id":"","shippingFeeExcluded":false
    }));
    let account=account_for(state,&view,&client);
    let wf=rules::execute("workflow_issues",json!({
        "state":state,"groupId":gid
    }))?;
    let request=automatic_request::execute(&json!({
        "settings":state["settings"],"order":view,"issues":wf["issues"],"step":step
    }))?;
    // The link toggle changes formatting, not recipient calculations or
    // provisional product detection. Build the native payment input once.
    let mut payment_data=payment_input(state,&view,&amounts,account,true)?;
    let payment_with_links=payment(&payment_data,&state["settings"])?;
    payment_data["includeLinks"]=json!(false);
    let payment_without_links=payment(&payment_data,&state["settings"])?;
    let issues=price_issues(&view,&client,account,&amounts)?;
    let greeting=render(&state["settings"],"paymentGreeting",&Value::Null)?;
    let payment_body=|value:&Value|{
        let pay=string(value,"payment");
        pay.strip_prefix(greeting.as_str()).map(str::trim_start).unwrap_or(pay).to_owned()
    };
    let valid_links=distinct(rows(&view,"recipients").iter()
        .flat_map(|r|rows(r,"items")).filter(|i|url_is_official(string(i,"url")))
        .map(|i|format!("{}\n{}",string(i,"name"),string(i,"url"))));
    let (plain,with_links)=if string(&request,"phase")=="payment"{
        (payment_body(&payment_without_links),payment_body(&payment_with_links))
    }else{
        let plain=string(&request,"requestText").to_owned();
        let linked=if valid_links.is_empty(){plain.clone()}
            else{format!("{plain}\n\n{}",valid_links.join("\n"))};
        (plain,linked)
    };
    let text=if include_links{&with_links}else{&plain};
    let signature=payment_signature(&view,string(&payment_with_links,"payment"),
        account,rows(payload,"accountWire"))?;
    let copied=||payment_was_copied(state,gid,&signature);
    if string(payload,"mode")=="payment_copy_status"{
        return Ok(json!({"copied":copied()}));
    }
    let mut result=json!({
        "phase":string(&request,"phase"),"text":text,"textWithLinks":with_links,
        "paymentIssues":issues,"signature":signature,
        "validRequestLinks":valid_links
    });
    if is_true(payload,"includeCopyStatus"){
        result["copied"]=json!(copied());
    }
    Ok(result)
}


fn payment_was_copied(state:&Value,gid:&str,signature:&str)->bool{
    rows(&state["workflow"],"events").iter().any(|e|
        string(e,"groupId")==gid&&string(e,"action")=="메시지 복사"&&
        string(&e["after"],"type")=="PAYMENT"&&
        string(&e["after"],"signature")==signature)
}

/// Only a local read-only board preview uses this cache. Mutations, copy
/// authorization, payment notices and remote adapters still call execute.
/// Global masters are intentionally conservative; unknown future fields
/// within an order, shipment, group or master invalidate its entry.
#[derive(Default)]
pub(crate) struct AutomaticPreviewCache{
    master:Option<Value>,
    entries:std::collections::HashMap<String,(Value,Value)>,
    #[cfg(test)]
    renders:usize,
}
impl AutomaticPreviewCache{
    pub(crate) fn preview(&mut self,payload:&Value)->Result<Value,String>{
        let state=payload.get("state").ok_or("메시지 생성 상태가 없습니다.")?;
        let gid=string(payload,"groupId");
        if gid.is_empty() || string(payload,"mode")=="payment_copy_status"{
            return execute(payload);
        }
        let workflow=&state["workflow"];
        let groups=rows(workflow,"groups");
        let all_ships=rows(workflow,"shipments");
        let Some(group)=by_id(groups,gid) else{return execute(payload);};
        let ships=all_ships.iter().filter(|s|string(s,"groupId")==gid)
            .collect::<Vec<_>>();
        if ships.is_empty() || groups.iter().filter(|g|string(g,"id")==gid).count()!=1{
            return execute(payload);
        }
        let ids=ships.iter().map(|ship|string(ship,"id")).collect::<HashSet<_>>();
        let order_ids=ships.iter().map(|ship|string(ship,"orderId")).collect::<HashSet<_>>();
        if ids.contains("") || order_ids.contains("") || ids.len()!=ships.len() ||
            all_ships.iter().filter(|s|ids.contains(string(s,"id"))).count()!=ships.len(){
            return execute(payload);
        }
        let orders=rows(state,"orders").iter().filter(|o|order_ids.contains(string(o,"id")))
            .collect::<Vec<_>>();
        if orders.len()!=order_ids.len(){return execute(payload);}
        let queue=rows(workflow,"queue").iter().filter(|q|
            ids.contains(string(q,"shipmentId"))).collect::<Vec<_>>();
        // Keep the complete source orders: workflow_issues also inspects
        // recipients detached into other groups of the same source order.
        // Shipment totals may read the frozen queue snapshot.
        let key=json!({"group":group,"shipments":ships,"orders":orders,
            "queue":queue,"includeLinks":is_true(payload,"includeLinks"),
            "accountWire":payload["accountWire"]});
        let master_keys=["settings","products","productMappings","clients"];
        if !self.master.as_ref().is_some_and(|master|
            master_keys.iter().all(|k|master[*k]==state[*k])){
            self.entries.clear();
            self.master=Some(json!({"settings":state["settings"],
                "products":state["products"],"productMappings":state["productMappings"],
                "clients":state["clients"]}));
        }
        // Bound retained orders and never cache errors. Entries are process
        // memory only and are removed when their groups disappear.
        self.entries.retain(|id,_|by_id(groups,id).is_some());
        if !self.entries.contains_key(gid) && self.entries.len()>=128{
            self.entries.clear();
        }
        if !self.entries.get(gid).is_some_and(|(old,_)|old==&key){
            let mut request=payload.clone();
            request["includeCopyStatus"]=json!(false);
            let result=execute(&request)?;
            #[cfg(test)]
            {self.renders+=1;}
            self.entries.insert(gid.to_owned(),(key,result));
        }
        let mut result=self.entries.get(gid).ok_or("메시지 캐시 누락")?.1.clone();
        if is_true(payload,"includeCopyStatus"){
            // Copy evidence is not part of the cached presentation.
            // Always check the exact current payment signature in the
            // current authoritative snapshot, including on cache hits.
            let copied=payment_was_copied(state,gid,string(&result,"signature"));
            result["copied"]=json!(copied);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod automatic_preview_cache_tests{
    use super::*;
    fn fixture()->Value{
        json!({
            "groupId":"g","includeLinks":false,"includeCopyStatus":true,
            "state":{
                "settings":{"accounts":[{"id":"a","bank":"가상은행","number":"123","holder":"테스트"}],
                    "defaultAccountId":"a","senderAddress":"서울특별시 중구 세종대로 1","senderPhone":"02-1234-5678"},
                "clients":[{"id":"c","name":"거래처","address":"서울특별시 중구 세종대로 1",
                    "phone":"02-1234-5678","tier":"wholesale","payment":"POSTPAID"}],
                "products":[{"id":"p","name":"709","variants":[{"id":"v","wholesale":10000}]}],
                "orders":[{"id":"o","clientId":"c","orderType":"AGENCY","applyVat":true,
                    "freightCode":"010","recipients":[{"id":"r","name":"테스트",
                        "address":"서울특별시 중구 세종대로 1","phone":"010-1234-5678",
                        "items":[{"id":"i","productId":"p","variantId":"v","name":"709",
                            "quantity":1,"quantityConfirmed":true,"unitPrice":10000,"origin":"FACTORY"}]}]}],
                "workflow":{"groups":[{"id":"g","dealerId":"c","needsNotice":false}],
                    "shipments":[{"id":"s","orderId":"o","recipientId":"r","groupId":"g",
                        "step":2,"charges":{"FACTORY":3000},"transferred":false}],
                    "queue":[],"events":[]}
            }
        })
    }
    #[test]
    fn automatic_preview_cache_link_variants_preserve_payment_signature(){
        let mut p=fixture();
        let url="https://bandostand.com/product/test/123/";
        p["state"]["orders"][0]["recipients"][0]["items"][0]["url"]=json!(url);
        let plain=execute(&p).unwrap();
        assert!(!string(&plain,"text").contains(url));
        assert!(string(&plain,"textWithLinks").contains(url));
        p["includeLinks"]=json!(true);
        let linked=execute(&p).unwrap();
        assert_eq!(linked["text"],plain["textWithLinks"]);
        assert_eq!(linked["signature"],plain["signature"]);
        p["state"]["orders"][0]["recipients"][0]["items"][0]["url"]=json!("https://example.com/product/test/");
        let rejected=execute(&p).unwrap();
        assert!(!string(&rejected,"text").contains("example.com"));
    }
    #[test]
    fn automatic_preview_cache_reuses_body_but_checks_copy_evidence_and_invalidates_money(){
        let mut p=fixture();
        let mut cache=AutomaticPreviewCache::default();
        let first=cache.preview(&p).unwrap();
        assert_eq!(first,execute(&p).unwrap());
        assert_eq!(cache.renders,1);
        p["state"]["revision"]=json!(2);
        p["state"]["orders"].as_array_mut().unwrap().push(json!({"id":"unrelated"}));
        assert_eq!(cache.preview(&p).unwrap(),first);
        assert_eq!(cache.renders,1);
        p["state"]["workflow"]["events"]=json!([{"groupId":"g","action":"메시지 복사",
            "after":{"type":"PAYMENT","signature":first["signature"]}}]);
        let copied=cache.preview(&p).unwrap();
        assert_eq!(copied,execute(&p).unwrap());
        assert_eq!(copied["copied"],true);
        assert_eq!(cache.renders,1);
        p["state"]["workflow"]["shipments"][0]["charges"]["FACTORY"]=json!(4000);
        let changed=cache.preview(&p).unwrap();
        assert_eq!(changed,execute(&p).unwrap());
        assert_ne!(changed["signature"],first["signature"]);
        assert_eq!(changed["copied"],false);
        assert_eq!(cache.renders,2);
        p["state"]["settings"]["accounts"][0]["number"]=json!("456");
        assert_eq!(cache.preview(&p).unwrap(),execute(&p).unwrap());
        assert_eq!(cache.renders,3);
        p["state"]["workflow"]["shipments"][0]["step"]=json!(1);
        assert_eq!(cache.preview(&p).unwrap(),execute(&p).unwrap());
        assert_eq!(cache.renders,4);
        p["includeLinks"]=json!(true);
        assert_eq!(cache.preview(&p).unwrap(),execute(&p).unwrap());
        assert_eq!(cache.renders,5);
    }
}
