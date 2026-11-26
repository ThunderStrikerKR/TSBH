//! Workflow validation projections translated from workflow.ts workflowIssues
//! and stageIssues. No UI, SQLite, recognition, or message generation.
use serde_json::{json,Value};
use std::collections::HashSet;
use super::{field,find_by_id,flag,has_value,rows,set,str_field};
use super::workflow as w;

pub(super) fn historical_product<'a>(state:&'a Value,pid:&str,vid:&str)->Option<&'a Value>{
    if pid.is_empty()||vid.is_empty(){return None;}
    let products=field(state,"products");
    if let Some(p)=find_by_id(products,pid){
        let variants=rows(field(p,"variants"));
        return (variants.len()==1&&str_field(&variants[0],"id")==vid).then_some(p);
    }
    let mapping=format!("{pid}|{vid}");
    let mapped_id=str_field(field(state,"productMappings"),&mapping);
    let p=find_by_id(products,mapped_id)?;
    let variants=rows(field(p,"variants"));
    (str_field(p,"sourceProductId")==pid &&
        variants.len()==1 && str_field(&variants[0],"id")==vid).then_some(p)
}
pub(super) fn sale_problem(p:Option<&Value>)->&str{
    let Some(p)=p else{return "상품 연결 확인 필요";};
    if has_value(field(p,"deletedAt"))||has_value(field(p,"supersededBy")) ||
        field(p,"active").as_bool()==Some(false){return "판매중지";}
    match str_field(p,"saleStatus"){
        ""|"판매중"|"SELLING"|"판매 표시"=>"",other=>other
    }
}
pub(super) fn sender_phone(raw:&str)->String{
    // Sender phone rules in src/explicit-sender.ts permit whitespace and
    // punctuation inside all three blocks of a telephone number.
    let chars=raw.chars().collect::<Vec<_>>();
    let is_sep=|ch:char|ch.is_whitespace()||['(',')','.','-'].contains(&ch);
    for start in 0..chars.len(){
        if chars[start]!='0'||(start>0&&chars[start-1].is_ascii_digit()){continue;}
        for prefix_len in [4,3,2]{
            if start+prefix_len>chars.len(){continue;}
            let prefix=chars[start..start+prefix_len].iter().collect::<String>();
            let valid=match prefix_len{
                4=>prefix.starts_with("050")&&prefix.chars().nth(3)
                    .is_some_and(|ch|ch.is_ascii_digit()),
                3=>prefix.starts_with("01")&&matches!(prefix.chars().nth(2),
                      Some('0'|'1'|'6'|'7'|'8'|'9'))
                    ||prefix=="070"||prefix=="080"
                    ||prefix.starts_with('0')&&matches!(prefix.chars().nth(1),
                      Some('3'|'4'|'5'|'6'))&&prefix.chars().nth(2)
                          .is_some_and(|ch|ch.is_ascii_digit()),
                2=>prefix=="02",_=>false
            };
            if !valid{continue;}
            let mut first=start+prefix_len;
            while first<chars.len()&&is_sep(chars[first]){first+=1;}
            for middle_length in [3,4]{
                let mid_end=first+middle_length;
                if mid_end>chars.len()||!chars[first..mid_end].iter().all(|c|c.is_ascii_digit()){
                    continue;
                }
                let mut last=mid_end;
                while last<chars.len()&&is_sep(chars[last]){last+=1;}
                let end=last+4;
                if end>chars.len()||!chars[last..end].iter().all(|c|c.is_ascii_digit())
                    ||end<chars.len()&&chars[end].is_ascii_digit(){continue;}
                return chars[start..end].iter().collect();
            }
        }
    }
    String::new()
}
fn operational(order:&Value,row:Option<&Value>,group_id:&str,
    kind:&str,title:String,stage:i64,field_name:&str)->Value{
    let base_id=format!("issue:{}:{}:recipient:{kind}",str_field(order,"id"),
        row.map(|r|str_field(r,"id")).unwrap_or("order"));
    let mut issue=json!({
        "issueId":if row.is_some(){base_id}else{format!("{base_id}:{group_id}")},
        "orderId":field(order,"id"),"groupId":group_id,
        "recipientId":row.map(|r|str_field(r,"id")).unwrap_or(""),
        "kind":kind,"title":title,
        "detail":if let Some(r)=row {
            format!("{} / {}",if str_field(r,"name").is_empty(){"수취인"}else{str_field(r,"name")},title)
        }else{title},
        "blockingStage":stage,"severity":"ACTION","resolution":"UNRESOLVED",
        "resolutionSource":"업무 검증","updatedAt":field(order,"updatedAt")
    });
    if let Some(r)=row{
        let _=set(&mut issue,"linkKey",
            json!(format!("{}:{field_name}",str_field(r,"id"))));
    }
    issue
}
fn active_shipments<'a>(state:&'a Value,group_id:&str)->Result<(usize,Vec<&'a Value>),String>{
    let groups=rows(field(field(state,"workflow"),"groups"));
    let gi=groups.iter().position(|g|str_field(g,"id")==group_id)
        .ok_or_else(||"현재 주문 묶음을 찾지 못했습니다.".to_string())?;
    let shipments=if has_value(field(&groups[gi],"deletedAt")){Vec::new()}
        else{rows(field(field(state,"workflow"),"shipments")).iter()
            .filter(|s|str_field(s,"groupId")==group_id&&!flag(s,"legacyArchived")
            &&!find_by_id(field(state,"orders"),str_field(s,"orderId"))
                .is_some_and(|o|has_value(field(o,"deletedAt"))))
            .collect()};
    if shipments.is_empty(){return Err("묶음의 배송 건이 없습니다.".into());}
    Ok((gi,shipments))
}
fn context<'a>(state:&'a Value,ship:&Value)->Result<(&'a Value,&'a Value),String>{
    let order=find_by_id(field(state,"orders"),str_field(ship,"orderId"))
        .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    let recipient=find_by_id(field(order,"recipients"),str_field(ship,"recipientId"))
        .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    Ok((order,recipient))
}
/// Copy-time billing evidence uses the same integer shipment totals as the
/// payment message. Resolve the account from the group's recipient subset.
pub(super) fn payment_copy_basis(state:&Value,group_id:&str)->Result<Value,String>{
    let (gi,ships)=active_shipments(state,group_id)?;
    let group=&state["workflow"]["groups"][gi];
    let (order,_)=context(state,ships[0])?;
    let total=ships.iter().map(|ship|{
        w::shipment_totals(state,str_field(ship,"id")).ok()
            .and_then(|amount|super::safe(field(&amount,"total")))
    }).collect::<Option<Vec<_>>>().and_then(super::checked_sum);
    let mut view=order.clone();
    let recipients=rows(field(order,"recipients")).iter().filter(|row|
        ships.iter().any(|ship|str_field(ship,"recipientId")==str_field(row,"id")))
        .cloned().collect::<Vec<_>>();
    set(&mut view,"recipients",json!(recipients))?;
    if let Some(mode)=group.get("accountMode").filter(|v|!v.is_null()){
        set(&mut view,"accountMode",mode.clone())?;
    }
    let account=w::party(state,&view).and_then(|client|w::account_for(state,group,&view,client));
    Ok(json!({"total":total,"account":account}))
}

fn account_identity(account:&Value)->Option<[&str;3]>{
    let keys=["bank","number","holder"].map(|key|str_field(account,key).trim());
    keys.iter().all(|s|!s.is_empty()).then_some(keys)
}

/// The allocation ledger additionally uses the registered account ID.
/// Do not retain an approval across different ledger identities.
pub(super) fn same_payment_account(a:&Value,b:&Value)->bool{
    field(a,"id")==field(b,"id") &&
        account_identity(a).zip(account_identity(b)).is_some_and(|(a,b)|a==b)
}

/// Older copy events contain the actual rendered message in the signature,
/// but no typed total. Only recover a unique, unambiguous default-format
/// total line; custom templates without a proven amount remain unknown.
fn legacy_copied_total(signature:&Value)->Option<i64>{
    let text=field(signature,"text").as_str()?;
    let mut totals=text.lines().filter_map(|line|{
        let tail=line.strip_prefix("합계 : ")?;
        let (amount,suffix)=tail.split_once("원 입니다. ")?;
        if !matches!(suffix,"(부가세 포함)"|"(부가세 별도)"|"(부가세 포함/별도)"){return None;}
        if amount.is_empty()||!amount.chars().all(|ch|ch.is_ascii_digit()||ch==','){return None;}
        let parsed=amount.replace(',',"").parse::<i64>().ok()?;
        let canonical=won(Some(parsed));
        if canonical.strip_suffix("원")!=Some(amount){return None;}
        super::safe(&json!(parsed))
    });
    let first=totals.next()?;
    totals.next().is_none().then_some(first)
}

fn copied_billing_changed(before:&Value,current:&Value)->bool{
    let signature=field(before,"signature").as_str()
        .and_then(|raw|serde_json::from_str::<Value>(raw).ok())
        .unwrap_or(Value::Null);
    let basis=field(before,"billing");
    let prior_total=if basis.is_object(){super::safe(field(basis,"total"))}
        else{legacy_copied_total(&signature)};
    let current_total=super::safe(field(current,"total"));
    let amount_changed=prior_total.zip(current_total).is_some_and(|(a,b)|a!=b);
    let prior_account=if basis.is_object(){field(basis,"account")}else{field(&signature,"account")};
    let account_changed=account_identity(prior_account).zip(account_identity(field(current,"account")))
        .is_some_and(|(a,b)|a!=b);
    amount_changed||account_changed
}

/// Only the most recent successful PAYMENT clipboard event counts. A request
/// copy, old matching event, stage rollback or generic dirty bit is not proof.
fn payment_renotice_required(state:&Value,group_id:&str)->Result<bool,String>{
    let Some(event)=rows(field(field(state,"workflow"),"events")).iter().rev().find(|e|
        str_field(e,"groupId")==group_id&&str_field(e,"action")=="메시지 복사"&&
        str_field(field(e,"after"),"type")=="PAYMENT") else{return Ok(false);};
    Ok(copied_billing_changed(field(event,"after"),&payment_copy_basis(state,group_id)?))
}

/// Formal stage completion remains separate from a clipboard copy. The
/// dirty flag alone is insufficient when all notice amounts and allocation
/// identities can be proven unchanged. Missing evidence stays conservative.
pub(super) fn notice_pending(state:&Value,group_id:&str)->Result<bool,String>{
    let (gi,ships)=active_shipments(state,group_id)?;
    let group=&state["workflow"]["groups"][gi];
    if payment_renotice_required(state,group_id)?{return Ok(true);}
    let Some(notice)=rows(field(group,"notices")).last()else{
        return Ok(flag(group,"needsNotice"));
    };
    // Some pre-stageValidity notices have no explicit shipmentIds array.
    // Their frozen amounts map already identifies the exact billed shipments.
    // Never infer membership from the group total or current order alone.
    let Some(amounts)=field(notice,"amounts").as_object() else{return Ok(true);};
    if amounts.len()!=ships.len() || ships.iter().any(|ship|
        !amounts.contains_key(str_field(ship,"id"))){return Ok(true);}
    if let Some(ids)=notice.get("shipmentIds"){
        let Some(ids)=ids.as_array()else{return Ok(true);};
        if ids.len()!=ships.len()||ships.iter().any(|ship|
            !ids.iter().any(|id|id.as_str()==Some(str_field(ship,"id")))){
            return Ok(true);
        }
    }
    let basis=payment_copy_basis(state,group_id)?;
    if !same_payment_account(field(notice,"account"),field(&basis,"account")){
        return Ok(true);
    }
    for ship in ships{
        let sid=str_field(ship,"id");
        let total=w::shipment_totals(state,sid)?;
        if !super::safe(field(field(notice,"amounts"),sid)).zip(super::safe(field(&total,"total")))
            .is_some_and(|(a,b)|a==b){return Ok(true);}
    }
    let sum=amounts.values().map(super::safe).collect::<Option<Vec<_>>>().and_then(super::checked_sum);
    if !sum.zip(super::safe(field(notice,"total"))).is_some_and(|(a,b)|a==b){
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn workflow_issues(state:&Value,group_id:&str)->Result<Vec<Value>,String>{
    let (gi,shipments)=active_shipments(state,group_id)?;
    let group=&state["workflow"]["groups"][gi];
    let (order,_) = context(state,shipments[0])?;
    let recipient_ids=shipments.iter().map(|s|str_field(s,"recipientId"))
        .collect::<HashSet<_>>();
    let mut issues=super::issues::order_issues(order).into_iter()
        .filter(|i|!has_value(field(order,"confirmedAt"))
            ||str_field(i,"recipientId").is_empty()
            ||recipient_ids.contains(str_field(i,"recipientId")))
        .map(|mut issue|{
            let _=set(&mut issue,"groupId",json!(group_id));issue
        }).collect::<Vec<_>>();
    // Historical reference validation runs for ALL original recipients,
    // including those moved to another operational group.
    for row in rows(field(order,"recipients")){
        for item in rows(field(row,"items")){
            let pid=str_field(item,"productId");
            if pid.is_empty(){continue;}
            let problem=sale_problem(historical_product(state,pid,str_field(item,"variantId")));
            if !problem.is_empty(){
                issues.push(operational(order,Some(row),group_id,"SALE_STATUS",
                    format!("{} / {} — 판매 상태 확인 필요",str_field(item,"name"),problem),
                    1,"product"));
            }
        }
    }
    // Sender fields are checked at Excel output, not at order confirmation.
    for ship in &shipments{
        let (shipment_order,row)=context(state,ship)?;
        for item in rows(field(row,"items")){
            let pid=str_field(item,"productId");
            let vid=str_field(item,"variantId");
            if pid.is_empty()||vid.is_empty()||has_value(field(order,"confirmedAt")){continue;}
            let valid=rows(field(state,"products")).iter().any(|p|
                str_field(p,"id")==pid&&!has_value(field(p,"deletedAt"))&&
                field(p,"active").as_bool()!=Some(false)&&
                !has_value(field(p,"supersededBy"))&&
                rows(field(p,"variants")).iter().any(|v|str_field(v,"id")==vid));
            if !valid&&!issues.iter().any(|i|
                str_field(i,"itemId")==str_field(item,"id")&&str_field(i,"kind")=="MODEL"){
                let mut issue=operational(order,Some(row),group_id,"MODEL",
                    "등록된 상품과 옵션을 확인하세요".into(),1,"product");
                set(&mut issue,"itemId",field(item,"id").clone())?;
                issues.push(issue);
            }
        }
        if rows(field(row,"items")).iter().any(|item|
            !["STORE","FACTORY"].contains(&str_field(item,"origin"))){
            issues.push(operational(order,Some(row),group_id,"ORIGIN",
                "발송지를 확인하세요".into(),1,"product"));
        }
        let shipment_client=w::party(state,shipment_order).ok_or("거래처 정보를 확인하세요.")?;
        if w::billable_fee_total(ship,row,shipment_order,shipment_client).is_none() &&
            !issues.iter().any(|i|str_field(i,"recipientId")==str_field(row,"id")&&
                has_value(field(i,"itemId"))&&str_field(i,"severity")=="ACTION"&&
                ["MODEL","MODEL_CHOICE","ALIAS_CONFLICT","QUANTITY_LINK",
                    "QUANTITY_INVALID"].contains(&str_field(i,"kind"))){
            issues.push(operational(order,Some(row),group_id,"FREIGHT",
                "배송 운임을 확인하세요".into(),2,"freight"));
        }
    }
    if str_field(order,"orderType")=="RETAIL"{
        issues.retain(|i|!["PRICE","FREIGHT"].contains(&str_field(i,"kind")));
        return Ok(issues);
    }
    let client=w::party(state,order).ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    let account=w::account_for(state,group,order,client);
    if account.is_none_or(|a|["bank","number","holder"].iter().any(|key|
        str_field(a,key).trim().is_empty())){
        issues.push(operational(order,None,group_id,"ACCOUNT",
            "입금 계좌를 등록 / 선택하세요".into(),3,"account"));
    }
    if payment_renotice_required(state,group_id)?{
        issues.push(operational(order,None,group_id,"NOTICE",
            "변경된 금액 / 계좌 재안내 필요".into(),3,"account"));
    }
    Ok(issues)
}
fn won(value:Option<i64>)->String{
    let Some(value)=value else{return "확인 필요".into()};
    let chars=value.to_string();
    let parts=chars.chars().rev().enumerate().map(|(i,ch)|
        if i>0&&i%3==0{format!(",{ch}")}else{ch.to_string()})
        .collect::<String>().chars().rev().collect::<String>();
    format!("{parts}원")
}
pub(super) fn stage_issues(state:&Value,group_id:&str,next:i64)->Result<Vec<String>,String>{
    let (_,ships)=active_shipments(state,group_id)?;
    let (first_order,_) = context(state,ships[0])?;
    // orderParty in TypeScript synthesizes the house client for a retail
    // order even if it has no dealer ID and no client snapshot.
    let retail_company=json!({"payment":"POSTPAID"});
    let client=w::party(state,first_order).or_else(||{
        (str_field(first_order,"orderType")=="RETAIL").then_some(&retail_company)
    }).ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    let wf_issues=workflow_issues(state,group_id)?;
    let mut messages=Vec::new();
    let mut group_due=Some(0_i64);
    for issue in wf_issues {
        if str_field(&issue,"severity")!="ACTION"||
            field(&issue,"blockingStage").as_i64().unwrap_or(i64::MAX)>next {continue;}
        let ship=ships.iter().copied().find(|s|
            str_field(s,"recipientId")==str_field(&issue,"recipientId"))
            .unwrap_or(ships[0]);
        let (_,row)=context(state,ship)?;
        messages.push(format!("{}: {}",if str_field(row,"name").is_empty(){"수취인"}
            else{str_field(row,"name")},str_field(&issue,"title")));
    }
    for ship in ships {
        let (order,row)=context(state,ship)?;
        let name=if str_field(row,"name").is_empty(){"수취인"}else{str_field(row,"name")};
        let prefix=format!("{name}: ");
        if str_field(order,"orderType")!="RETAIL"{
            if next>=2&&field(&w::shipment_totals(state,str_field(ship,"id"))?,"total").is_null()
                &&messages.is_empty(){
                messages.push(format!("{prefix}청구액 미확정"));
            }
            if next>=3&&!super::workflow_stages::read(ship)[1]&&
                w::shipment_freight_code(order,row)!="020"{
                messages.push(format!("{prefix}운임 적용 필요"));
            }
            if next>=4{
                if !super::workflow_stages::read(ship)[2]{
                    messages.push(format!("{prefix}계좌 안내 완료 필요"));
                }
                if str_field(client,"payment")=="PREPAID"&&!flag(ship,"legacyApproval"){
                    let due=w::payment_due(state,ship,order,client)?;
                    group_due=match (group_due,due){
                        (Some(sum),Some(amount))=>sum.checked_add(amount),
                        _=>None,
                    };
                }
            }
        }
        for item in rows(field(row,"items")){
            let pid=str_field(item,"productId");
            if pid.is_empty(){continue;}
            let problem=sale_problem(historical_product(state,pid,str_field(item,"variantId")));
            if !problem.is_empty(){
                messages.push(format!("{name}: {} / {problem} — 판매 상태 확인 필요",
                    str_field(item,"name")));
            }
        }
    }
    // Payment is requested from the dealer, not each delivery recipient.
    // Keep the same approval blocker: only the label and aggregation change.
    if group_due!=Some(0){
        let dealer=["displayName","name"].iter().map(|key|str_field(client,key).trim())
            .find(|name|!name.is_empty()).unwrap_or("거래처");
        messages.push(match group_due{
            Some(due)=>format!("{dealer} 입금 청구 {}",won(Some(due))),
            None=>format!("{dealer} 입금 청구액 확인 필요"),
        });
    }
    let mut seen=HashSet::new();
    Ok(messages.into_iter().filter(|m|seen.insert(m.clone())).collect())
}

#[cfg(test)]
mod freight_issue_tests {
    use super::*;

    #[test]
    fn missing_prepaid_freight_is_a_structured_stage_two_issue(){
        let mut state=json!({
            "clients":[{
                "id":"c","name":"가상 거래처","address":"서울 주소",
                "phone":"02-1234-5678","payment":"PREPAID",
                "tier":"agency","shippingFeeExcluded":false,
                "overrides":{}
            }],
            "orders":[{
                "id":"o","clientId":"c","orderType":"AGENCY",
                "confirmedAt":"2026-11-25T08:00:00+09:00",
                "updatedAt":"2026-11-25T08:00:00+09:00",
                "freightCode":"010","recipients":[{
                    "id":"r","name":"가상 수취인",
                    "address":"서울특별시 중구 세종대로 1",
                    "items":[{"id":"i","name":"가상 상품","origin":"FACTORY","quantity":1,
                        "productId":"","variantId":""}]
                }]
            }],
            "products":[],
            "settings":{"accounts":[],"senderName":"반도스탠드",
                "senderAddress":"서울 주소","senderPhone":"02-1234-5678"},
            "workflow":{
                "groups":[{"id":"g","needsNotice":false,"notices":[]}],
                "shipments":[{
                    "id":"s","orderId":"o","recipientId":"r","groupId":"g",
                    "step":1,"charges":{"FACTORY":null},"transferred":false
                }],
                "payments":[],"confirmations":[],"events":[],"queue":[]
            }
        });
        // An unresolved product prevents freight calculation. Preserve the
        // existing root-cause-first issue policy, then verify the independent
        // missing freight issue once the product and quantity are resolved.
        let unresolved=workflow_issues(&state,"g").expect("workflow issues");
        assert!(unresolved.iter().any(|issue|str_field(issue,"kind")=="MODEL"));
        assert!(!unresolved.iter().any(|issue|str_field(issue,"kind")=="FREIGHT"));
        state["products"]=json!([{"id":"p","variants":[{"id":"v"}]}]);
        let item=&mut state["orders"][0]["recipients"][0]["items"][0];
        item["productId"]=json!("p");
        item["variantId"]=json!("v");
        item["unitPrice"]=json!(10000);
        item["quantityConfirmed"]=json!(true);
        state["settings"]["senderAddress"]=json!("");
        state["settings"]["senderPhone"]=json!("");
        state["clients"][0]["tier"]=json!("retail");
        for kind in ["AGENCY","DEALER"]{
            state["orders"][0]["orderType"]=json!(kind);
            assert!(!workflow_issues(&state,"g").unwrap().iter()
                .any(|issue|str_field(issue,"kind").starts_with("SENDER_")));
            assert!(!stage_issues(&state,"g",1).unwrap().iter()
                .any(|message|message.contains("송하인")||message.contains("반도스탠드")));
        }
        state["orders"][0]["orderType"]=json!("AGENCY");
        let issues=workflow_issues(&state,"g").expect("workflow issues");
        let freight=issues.iter().find(|issue|str_field(issue,"kind")=="FREIGHT")
            .expect("missing prepaid freight must be visible");
        assert_eq!(str_field(freight,"title"),"배송 운임을 확인하세요");
        assert_eq!(str_field(freight,"detail"),"가상 수취인 / 배송 운임을 확인하세요");
        assert_eq!(field(freight,"blockingStage").as_i64(),Some(2));
        assert_eq!(str_field(freight,"severity"),"ACTION");
        state["workflow"]["shipments"][0]["charges"]["FACTORY"]=json!(3000);
        assert!(!workflow_issues(&state,"g").unwrap().iter()
            .any(|issue|str_field(issue,"kind")=="FREIGHT"));
        state["workflow"]["shipments"][0]["charges"]["FACTORY"]=Value::Null;
        state["orders"][0]["recipients"][0]["freightCode"]=json!("020");
        assert!(!workflow_issues(&state,"g").unwrap().iter()
            .any(|issue|str_field(issue,"kind")=="FREIGHT"));
    }
}

#[cfg(test)]
mod copied_payment_notice_tests{
    use super::*;
    #[test]
    fn only_verified_total_or_account_changes_trigger_notice(){
        let a=json!({"bank":"은행","number":"123","holder":"예금주"});
        let before=json!({"billing":{"total":30000,"account":a}});
        assert!(!copied_billing_changed(&before,&json!({"total":30000,"account":a})));
        assert!(copied_billing_changed(&before,&json!({"total":31000,"account":a})));
        assert!(copied_billing_changed(&before,&json!({"total":29000,"account":a})));
        assert!(!copied_billing_changed(&before,&json!({"total":null,"account":a})));
        assert!(copied_billing_changed(&before,&json!({"total":30000,
            "account":{"bank":"은행","number":"456","holder":"예금주"}})));
        assert!(!copied_billing_changed(&json!({}),&json!({"total":30000,"account":a})));
    }
    #[test]
    fn legacy_copy_total_is_recovered_only_from_unambiguous_rendered_total(){
        let old=json!({"signature":json!({"text":"운임 : 3,000원\n합계 : 30,000원 입니다. (부가세 포함)"}).to_string()});
        assert!(!copied_billing_changed(&old,&json!({"total":30000})));
        assert!(copied_billing_changed(&old,&json!({"total":31000})));
        let ambiguous=json!({"signature":json!({"text":"합계 : 30,000원 입니다. (부가세 포함)\n합계 : 31,000원 입니다. (부가세 별도)"}).to_string()});
        assert!(!copied_billing_changed(&ambiguous,&json!({"total":31000})));
    }
}
