//! Exact order issue decision projection based on src/validation.ts.
//! Do not use this as the only group approval gate until workflowIssues and
//! stageIssues have their own original-TypeScript differential coverage.
use std::collections::HashSet;
use serde_json::{json,Map,Value};
use super::{field,req,required_str,rows,set,str_field};

fn evidence_value(value:&Value)->String {
    match value {
        Value::Array(xs)=>format!("[{}]",xs.iter().map(evidence_value)
            .collect::<Vec<_>>().join(",")),
        Value::Object(map)=>{
            // The original TS match objects place their discriminant
            // before other fields. serde_json's default BTreeMap sorts
            // keys, which changes JSON.stringify evidence and staff
            // decision fingerprints.
            let mut keys=map.keys().cloned().collect::<Vec<String>>();
            // Exact original parser.ts/item.match insertion order:
            // kind -> term -> productIds -> aliasId. This string is a
            // persisted employee-decision fingerprint, not cosmetic JSON.
            if map.contains_key("kind") && map.contains_key("term") {
                let expected=["kind","term","productIds","aliasId"];
                keys.retain(|key|!expected.contains(&key.as_str()));
                for key in expected.iter().rev(){
                    if map.contains_key(*key){keys.insert(0,(*key).to_owned());}
                }
            } else if map.contains_key("kind"){
                keys.retain(|key|key.as_str()!="kind");
                keys.insert(0,"kind".to_string());
            } else if map.len()==3 && ["field","value","label"].iter()
                .all(|key|map.contains_key(*key)) {
                // Original candidate objects received through the React
                // input may have value before label, unlike sorted maps.
                keys=vec!["field".into(),"value".into(),"label".into()];
            }
            format!("{{{}}}",keys.iter().map(|key|format!("{}:{}",
                json!(key),evidence_value(&map[key]))).collect::<Vec<_>>().join(","))
        },
        _=>serde_json::to_string(value).unwrap_or_else(|_|"null".to_string())
    }
}
fn js_evidence(value:Option<&Value>)->Option<String> {
    value.map(evidence_value)
}
fn issue_id(order:&Value,row:Option<&Value>,item:Option<&Value>,kind:&str)->String {
    format!("issue:{}:{}:{}:{kind}",str_field(order,"id"),
        row.map(|r|str_field(r,"id")).unwrap_or("order"),
        item.map(|i|str_field(i,"id")).unwrap_or("recipient"))
}
fn internal_model(name:&str)->String {
    let mut text=name;
    if let Some(rest)=text.strip_prefix("[가상]"){
        text=rest.trim_start_matches(char::is_whitespace);
    }
    if let Some(prefix)=text.get(..2){
        if prefix.eq_ignore_ascii_case("BD") {
            let suffix=&text[2..];
            if suffix.chars().next().is_some_and(|c|c.is_whitespace()||c.is_ascii_digit()) {
                return suffix.trim_start_matches(char::is_whitespace).to_string();
            }
        }
    }
    text.to_string()
}
// One Rust eligibility predicate is used both to enable the staff button
// and to authorize the final action. The React view must not duplicate it.
fn can_approve_missing_quantity(item:&Value)->bool {
    !str_field(item,"productId").is_empty() &&
    !str_field(item,"variantId").is_empty() &&
    field(item,"quantityConfirmed").as_bool()==Some(false) &&
    str_field(field(item,"match"),"kind")!="QUANTITY" &&
    !rows(field(item,"issues")).iter().any(|v|v.as_str().is_some_and(|s|
        s.contains("여러")||s.contains("불명확")))
}
fn valid_contact(value:&str)->bool {
    let digits:String=value.chars()
        .filter(|c|!c.is_whitespace()&&!['(',')','.','-'].contains(c)).collect();
    digits.chars().all(|c|c.is_ascii_digit()) &&
        ((digits.starts_with('0')&&(9..=11).contains(&digits.len())) ||
          (digits.starts_with("050")&&digits.len()==12)) &&
        (!digits.starts_with("010") || digits.len()==11)
}
pub(super) fn valid_address(value:&str)->bool {
    let s=value.trim();
    if s.is_empty()||s.contains('#')||s.contains('@'){return false;}
    for marker in ["대로","로","길","사서함"]{
        if s.match_indices(marker).any(|(i,_)| s[i+marker.len()..].trim_start()
            .chars().next().is_some_and(|c|c.is_ascii_digit())) {return true;}
    }
    for (i,c) in s.char_indices() {
        if !['동','리','읍','면'].contains(&c){continue;}
        let suffix=&s[i+c.len_utf8()..];
        if !suffix.chars().next().is_some_and(char::is_whitespace){continue;}
        let mut suffix=suffix.trim_start_matches(char::is_whitespace);
        if let Some(after)=suffix.strip_prefix('산'){
            suffix=after.trim_start_matches(char::is_whitespace);
        }
        if suffix.chars().next().is_some_and(|c|c.is_ascii_digit()){return true;}
    }
    false
}
/// Multiple evidence spans can describe ONE address (province/city, road,
/// building/floor). The recognizer already joins these spans in row.address;
/// do not ask the user to choose one fragment and thereby erase the rest.
///
/// Be conservative: an edited/non-matching address, two complete addresses,
/// or multiple region/district introductions remain genuine ambiguities.
fn address_candidates_form_one_location(row:&Value,candidates:&[Value])->bool {
    let address=str_field(row,"address");
    if str_field(row,"addressStatus")=="INCOMPLETE" || !valid_address(address) {
        return false;
    }
    let parts:Vec<&str>=candidates.iter()
        .map(|candidate|str_field(candidate,"value").trim())
        .filter(|value|!value.is_empty()).collect();
    if parts.len()<2 {return false;}
    let normalized=|s:&str|s.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized(&parts.join(" "))!=normalized(address){return false;}
    if parts.iter().filter(|value|valid_address(value)).count()>1{return false;}

    // Two different administrative starts or two districts are not one
    // unambiguous destination, even if an over-eager parser concatenated them.
    const PROVINCES:&[&str]=&[
        "서울","서울특별시","부산","부산광역시","대구","대구광역시",
        "인천","인천광역시","광주","광주광역시","대전","대전광역시",
        "울산","울산광역시","세종","세종특별자치시","경기","경기도",
        "강원","강원도","강원특별자치도","충북","충청북도",
        "충남","충청남도","전북","전라북도","전북특별자치도",
        "전남","전라남도","경북","경상북도","경남","경상남도",
        "제주","제주도","제주특별자치도",
    ];
    let region_starts=parts.iter().filter(|segment|
        segment.split_whitespace().next().is_some_and(|word|PROVINCES.contains(&word))
    ).count();
    if region_starts>1{return false;}
    let words=address.split_whitespace();
    let districts=words.clone().filter(|word|
        word.chars().count()>1 && word.ends_with('구')).count();
    let cities=words.filter(|word|
        word.chars().count()>1 && word.ends_with('시')).count();
    districts<=1 && cities<=1
}

/// Exact recipient-delivery eligibility from src/delivery-requirements.ts.
/// This read-only projection is authoritative in the manual and order screens.
// User approval is scoped to the actual row and exact relevant values.
 // No approval can create missing data or bypass a structural prerequisite.
fn approval_basis(row:&Value,kind:&str)->Option<Value>{
    let nonempty=|key:&str|!str_field(row,key).trim().is_empty();
    let values=match kind{
        "ADDRESS" if nonempty("address")=>json!([field(row,"address")]),
        "CONTACT" if nonempty("phone")||nonempty("mobile")=>
            json!([field(row,"phone"),field(row,"mobile")]),
        "CANDIDATE_name"|"CANDIDATE_address"|"CANDIDATE_phone"|"CANDIDATE_mobile"=>{
            let key=kind.strip_prefix("CANDIDATE_")?;
            if !nonempty(key){return None;}
            let candidates=rows(field(row,"candidates")).iter()
                .filter(|c|str_field(c,"field")==key).cloned().collect::<Vec<_>>();
            json!([field(row,key),candidates])
        },
        _=>return None
    };
    let id=str_field(row,"id");
    if id.is_empty(){return None;}
    Some(json!({"recipientId":id,"kind":kind,"values":values}))
}
fn approved(row:&Value,kind:&str)->bool{
    approval_basis(row,kind).is_some_and(|basis|
        field(row,"validationApprovals").get(kind).is_some_and(|record|
            field(record,"basis")==&basis && !str_field(record,"approvedAt").is_empty()
            && !str_field(record,"approvedBy").is_empty()))
}
fn item_approval_basis(row:&Value,item:&Value,kind:&str)->Option<Value>{
    if ["id","productId","variantId"].iter().any(|key|str_field(item,key).trim().is_empty()){
        return None;
    }
    let values=match kind{
        "OPTION"|"ALIAS_CONFLICT"|"CATEGORY_CONFLICT"|"MODEL_CONFIRM"=>
            json!([field(item,"productId"),field(item,"variantId"),field(item,"color"),
                field(item,"raw"),field(item,"match"),field(item,"issues"),field(item,"modelProposal")]),
        "QUANTITY_MISSING"|"QUANTITY_LINK"=>{
            if !field(item,"quantity").as_i64().is_some_and(|n|(1..=super::MAX_SAFE).contains(&n))
                || super::has_value(field(item,"quantityApproval"))
                || field(item,"defaulted")==true {return None;}
            json!([field(item,"productId"),field(item,"variantId"),field(item,"quantity"),
                field(item,"quantityConfirmed"),field(item,"raw"),field(item,"match"),
                rows(field(item,"issues")).iter().filter(|v|
                    v.as_str().is_some_and(|s|s.contains("수량"))).collect::<Vec<_>>()])
        },
        _=>return None
    };
    Some(json!({"recipientId":field(row,"id"),"itemId":field(item,"id"),"kind":kind,"values":values}))
}
fn approval_key(item:Option<&Value>,kind:&str)->String{
    match item{Some(i)=>format!("{}:{kind}",str_field(i,"id")),None=>kind.to_owned()}
}
fn item_approved(row:&Value,item:&Value,kind:&str)->bool{
    item_approval_basis(row,item,kind).is_some_and(|basis|
        field(row,"validationApprovals").get(approval_key(Some(item),kind)).is_some_and(|record|
            field(record,"basis")==&basis && !str_field(record,"approvedAt").is_empty()
            && !str_field(record,"approvedBy").is_empty()))
}
pub(crate) fn quantity_user_approved(row:&Value,item:&Value)->bool{
    item_approved(row,item,"QUANTITY_MISSING")||item_approved(row,item,"QUANTITY_LINK")
}
pub(super) fn quantity_accepted(row:&Value,item:&Value)->bool{
    field(item,"quantity").as_i64().is_some_and(|n|(1..=super::MAX_SAFE).contains(&n)) &&
        (field(item,"quantityConfirmed")!=&Value::Bool(false) || quantity_user_approved(row,item))
}
pub(super) fn prune_validation_approvals(row:&mut Value){
    let valid=field(row,"validationApprovals").as_object().map(|records|
        records.iter().filter(|(key,record)|{
            let basis=field(record,"basis");
            let kind=str_field(basis,"kind");
            if let Some(item)=rows(field(row,"items")).iter().find(|item|
                !str_field(basis,"itemId").is_empty() && str_field(item,"id")==str_field(basis,"itemId")){
                key.as_str()==approval_key(Some(item),kind).as_str() && item_approved(row,item,kind)
            }else{approved(row,key)}
        }).map(|(key,_)|key.clone()).collect::<HashSet<_>>());
    if let (Some(valid),Some(records))=(valid,row.get_mut("validationApprovals").and_then(Value::as_object_mut)){
        records.retain(|kind,_|valid.contains(kind));
    }
}
fn address_format_ok(row:&Value,allow_legacy:bool)->bool{
    let address=str_field(row,"address");
    !address.trim().is_empty() &&
        ((allow_legacy && str_field(row,"addressStatus")=="CONFIRMED") ||
         (str_field(row,"addressStatus")!="INCOMPLETE" && valid_address(address)))
}
fn contact_format_ok(row:&Value)->bool{
    valid_contact(str_field(row,"phone"))||valid_contact(str_field(row,"mobile"))
}
pub(super) fn delivery_requirements(payload: &Value) -> Result<Value, String> {
    let recipients=rows(req(payload,"rows")?);
    let allow_legacy=field(payload,"allowLegacyAddressConfirmation").as_bool()==Some(true);
    let results=recipients.iter().map(|row|{
        let address_ok=address_format_ok(row,allow_legacy)||approved(row,"ADDRESS");
        let items=rows(field(row,"items"));
        let product_ok=!items.is_empty() && items.iter().all(|i|
            !str_field(i,"productId").is_empty() && !str_field(i,"variantId").is_empty()
        );
        let quantity_ok=!items.is_empty() && items.iter().all(|i|
            quantity_accepted(row,i)
        );
        json!({"recipientId":field(row,"id"),
            "name":!str_field(row,"name").trim().is_empty(),
            "address":address_ok,
            "contact":contact_format_ok(row)||approved(row,"CONTACT"),
            "product":product_ok,"quantity":quantity_ok})
    }).collect::<Vec<_>>();
    Ok(json!({"results":results}))
}

/// UI-facing delivery projection with native contact error descriptions.
/// The old delivery_requirements operation remains unchanged for parity tests.
pub(super) fn delivery_requirements_with_messages(payload:&Value)->Result<Value,String> {
    let mut projection=delivery_requirements(payload)?;
    let source=rows(req(payload,"rows")?);
    let entries=projection["results"].as_array_mut()
        .ok_or("Rust 배송정보 결과 형식을 확인하세요.")?;
    for (out,row) in entries.iter_mut().zip(source.iter()) {
        for (column,is_mobile) in [("mobile",true),("phone",false)] {
            let value=str_field(row,column);
            let explanation=if value.trim().is_empty() || valid_contact(value) || approved(row,"CONTACT") {
                Value::Null
            }else{
                json!(contact_problem(value,is_mobile))
            };
            let key=if is_mobile{"mobileProblem"}else{"phoneProblem"};
            out[key]=explanation;
        }
    }
    Ok(projection)
}
fn contact_problem(value:&str,mobile:bool)->String {
    let digits:String=value.chars()
        .filter(|c|!c.is_whitespace()&&!['(',')','.','-'].contains(c)).collect();
    if value.trim().is_empty(){
        return (if mobile {"휴대폰 번호가 없습니다."}else{"전화번호가 없습니다."}).to_string();
    }
    if digits.starts_with("010") && digits.len()!=11 {
        return format!("휴대폰 번호가 {}자리입니다. 010으로 시작하는 번호는 숫자 11자리가 필요합니다.",digits.len());
    }
    (if mobile{"휴대폰 번호의 형식과 자릿수를 확인해주세요."}
    else{"일반전화 번호의 지역번호와 자릿수를 확인해주세요."}).to_string()
}
fn unassigned_detail(row:&Value)->String {
    let candidates=rows(field(row,"unassignedEvidence"));
    let mut parts=Vec::new();
    for candidate in candidates.iter().take(4) {
        let source=str_field(candidate,"text");
        let source=if source.is_empty(){str_field(candidate,"value")} else{source};
        let normalized=source.split_whitespace().collect::<Vec<_>>().join(" ");
        let chars=normalized.chars().collect::<Vec<_>>();
        let short=if chars.len()>60 {
            format!("{}…",chars[..60].iter().collect::<String>())
        }else{normalized};
        parts.push(format!("{}: \"{}\"",if str_field(candidate,"kind")=="quantity"{"수량"}else{"상품"},short));
    }
    let more=if candidates.len()>4{format!(" 외 {}개",candidates.len()-4)}else{String::new()};
    format!("연결되지 않은 원문: {}{more}",if parts.is_empty(){"확인할 문구 없음".into()}else{parts.join(" / ")})
}
struct Info<'a>{
    row:Option<&'a Value>,
    item:Option<&'a Value>,
    kind:&'a str,
    title:String,
    evidence:Option<&'a Value>,
    field:&'a str,
    stage:i64,
    customer:Option<String>,
    resolution:Option<&'a str>,
}
fn emit(result:&mut Vec<Value>,order:&Value,info:Info<'_>){
    let Info{row,item,kind,title,evidence,field:field_name,stage,customer,resolution}=info;
    let id=issue_id(order,row,item,kind);
    let evidence=js_evidence(evidence);
    let decision=field(order,"issueResolutions").get(&id)
        .filter(|d|str_field(d,"resolution")!="USER_APPROVED");
    let basis=row.and_then(|r|match item{
        Some(i)=>item_approval_basis(r,i,kind),None=>approval_basis(r,kind)});
    let key=approval_key(item,kind);
    let user_approval=row.filter(|r|match item{
        Some(i)=>item_approved(r,i,kind),None=>approved(r,kind)})
        .and_then(|r|field(r,"validationApprovals").get(&key));
    let status=if user_approval.is_some(){"USER_APPROVED"}else{resolution.unwrap_or_else(||{
        let approved=kind=="QUANTITY_MISSING"&&item.is_some_and(|i|{
            field(field(i,"quantityApproval"),"quantity").as_i64()==Some(1)&&
            field(i,"quantity").as_i64()==Some(1)&&
            field(i,"quantityConfirmed").as_bool()==Some(false)&&
            !str_field(i,"productId").is_empty()&&
            !str_field(i,"variantId").is_empty()
        });
        if approved{"PROVISIONALLY_APPROVED"}
        else if decision.is_some_and(|d|
             str_field(d,"evidence")==evidence.as_deref().unwrap_or("")
             && str_field(d,"resolution")=="PENDING_CUSTOMER") {
            "PENDING_CUSTOMER"
        }else{"UNRESOLVED"}
    })};
    let detail=if status=="USER_APPROVED" {
        let target=match kind{"ADDRESS"=>"배송주소","CONTACT"=>"연락처",
            "CANDIDATE_name"=>"수취인명","CANDIDATE_address"=>"배송주소",
            "CANDIDATE_phone"=>"전화번호","CANDIDATE_mobile"=>"휴대폰 번호",
            "OPTION"=>"상품 옵션","ALIAS_CONFLICT"|"CATEGORY_CONFLICT"|"MODEL_CONFIRM"=>"상품",
            "QUANTITY_MISSING"|"QUANTITY_LINK"=>"수량",_=>"현재 정보"};
        let who=row.map(|r|str_field(r,"name")).filter(|name|!name.is_empty()).unwrap_or("수취인");
        if let Some(item)=item{format!("{who} — {} {target} 사용자 승인",internal_model(str_field(item,"name")))}
        else{format!("{who} — {target} 사용자 승인")}
    }else if kind=="UNASSIGNED_EVIDENCE" {
        row.map(unassigned_detail).unwrap_or_else(||title.clone())
    }else if let Some(row)=row {
        format!("{} / {}",if str_field(row,"name").is_empty(){"수취인"}else{str_field(row,"name")},
            item.map(|i|internal_model(str_field(i,"name"))).unwrap_or_else(||title.clone()))
    }else{title.clone()};
    let resolution_source=user_approval.and_then(|d|d.get("approvedBy")).cloned().or_else(||decision
        .and_then(|d|d.get("resolutionSource")).cloned()).filter(|v|!v.is_null())
        .unwrap_or_else(||json!(match status {
            "AUTO_APPLIED"=>"코어","PROVISIONALLY_APPROVED"=>item
                .map(|i|str_field(field(i,"quantityApproval"),"actor"))
                .filter(|v|!v.is_empty()).unwrap_or("사용자"),_=>"미확인"
        }));
    let updated_at=user_approval.and_then(|d|d.get("approvedAt"))
        .or_else(||decision.and_then(|d|d.get("updatedAt"))).filter(|v|!v.is_null())
        .or_else(||item.and_then(|i|field(i,"quantityApproval").get("at"))
            .filter(|v|!v.is_null()))
        .unwrap_or_else(||field(order,"updatedAt")).clone();
    let mut issue=json!({
        "issueId":id,"orderId":field(order,"id"),
        "groupId":format!("group:{}",str_field(order,"id")),
        "recipientId":row.map(|r|str_field(r,"id")).unwrap_or(""),
        "kind":kind,"title":title,"detail":detail,
        "blockingStage":stage,
        "severity":if ["AUTO_APPLIED","PROVISIONALLY_APPROVED","RESOLVED","USER_APPROVED"].contains(&status){"INFO"}else{"ACTION"},
        "resolution":status,"resolutionSource":resolution_source,"updatedAt":updated_at
    });
    if let Some(e)=evidence {let _=set(&mut issue,"evidence",json!(e));}
    if let Some(basis)=basis {
        issue["approvalBasis"]=basis;
        issue["canApproveCurrent"]=json!(status!="USER_APPROVED");
        issue["canCancelCurrent"]=json!(status=="USER_APPROVED");
    }
    if let Some(item)=item {let _=set(&mut issue,"itemId",field(item,"id").clone());}
    if let Some(row)=row{
        let key=item.map(|i|str_field(i,"id")).unwrap_or_else(||str_field(row,"id"));
        let _=set(&mut issue,"linkKey",json!(format!("{key}:{field_name}")));
    }
    let sources=item.and_then(|i|i.get("sources")).or_else(||row.and_then(|r|r.get("sources")));
    if let Some(span)=sources.and_then(|s|rows(s).iter().find(|v|str_field(v,"field")==field_name)){
        let _=set(&mut issue,"sourceSpan",span.clone());
    }
    if let Some(customer)=customer{let _=set(&mut issue,"customer",json!(customer));}
    result.push(issue);
}
fn add<'a>(result:&mut Vec<Value>,o:&Value,row:Option<&'a Value>,
   item:Option<&'a Value>,kind:&'a str,title:impl Into<String>,field:&'a str,
   evidence:Option<&'a Value>,stage:i64,customer:Option<String>,resolution:Option<&'a str>) {
    emit(result,o,Info{row,item,kind,title:title.into(),evidence,field,stage,customer,resolution});
}
pub(super) fn order_issues(o:&Value)->Vec<Value>{
    let mut issues=Vec::new();
    // The review is action-oriented: neither manual selection nor a missing
    // auto-match ("NONE") warrants an extra provenance card. A missing dealer
    // already has the single actionable CLIENT issue below. Keep the evidence
    // in the order, and retain real matching conflicts/errors as actionable.
    let match_info=field(o,"clientMatch");
    let source=str_field(match_info,"source");
    if match_info.is_object() && source!="MANUAL" && source!="NONE" {
        let matched_id=str_field(match_info,"clientId");
        let actual_id=str_field(o,"clientId");
        let detail=str_field(match_info,"detail");
        if !detail.trim().is_empty() && detail.chars().count()<=500 {
            let has_error=source=="ERROR";
            // An unapplied but valid proposal is not contradictory evidence.
            // Keep the ordinary missing-dealer gate until the actual ID is set.
            let collision=source=="CONFLICT" ||
                (!matched_id.is_empty() && !actual_id.is_empty() && actual_id!=matched_id);
            let applied=!matched_id.is_empty() && actual_id==matched_id
                && ["KAKAO_TITLE","KAKAO_REMEMBERED","RAW_EXPLICIT"].contains(&source);
            let needs_action=collision || has_error;
            if needs_action || applied {
            let name=str_field(field(o,"clientSnapshot"),"name");
            let name=if name.is_empty(){str_field(match_info,"clientName")}else{name};
            let title=if collision{"발주처 매칭 충돌 — 거래처를 확인하세요".to_owned()}
                else if has_error{"거래처 자동 매칭 오류 — 직접 확인하세요".to_owned()}
                else if name.is_empty(){"거래처 자동선택".to_owned()}
                else{format!("거래처 자동선택 ({name})")};
            let kind=if has_error{"CLIENT_MATCH_ERROR"}
                else if collision{"CLIENT_MATCH_CONFLICT"}else{"CLIENT_MATCH_EVIDENCE"};
            issues.push(json!({
                "issueId":format!("issue:{}:client-match",str_field(o,"id")),
                "orderId":field(o,"id"),
                "groupId":format!("group:{}",str_field(o,"id")),
                "recipientId":"",
                "kind":kind,
                "title":title,
                "detail":if needs_action{detail}else{""},
                "blockingStage":1,
                "severity":if needs_action{"ACTION"}else{"INFO"},
                "resolution":if needs_action{"UNRESOLVED"}else{"AUTO_APPLIED"},
                "resolutionSource":"Rust 거래처 매칭",
                "updatedAt":field(o,"updatedAt"),
                "linkKey":"client"
            }));
            if collision && !has_error && !actual_id.is_empty(){
                if let Some(issue)=issues.last_mut(){issue["selectedClientId"]=json!(actual_id);}
            }
            }
        }
    }
    // One user action should be represented by one actionable card. When a
    // detailed automatic-match conflict/error already asks for dealer review,
    // do not also add the generic missing-dealer action.
    let dealer_match_action=issues.iter().any(|issue|
        ["CLIENT_MATCH_ERROR","CLIENT_MATCH_CONFLICT"].contains(
            &str_field(issue,"kind")));
    if str_field(o,"clientId").is_empty() &&
       str_field(o,"orderType")!="RETAIL" && !dealer_match_action {
        add(&mut issues,o,None,None,"CLIENT","거래처를 선택하면 단가가 적용됩니다",
            "client",o.get("clientId"),1,None,None);
    }
    if rows(field(o,"recipients")).is_empty(){
        let zero=json!(0);
        add(&mut issues,o,None,None,"RECIPIENT","수취인과 상품을 추가하세요",
            "recipient",Some(&zero),1,None,None);
    }
    for row in rows(field(o,"recipients")){
        let who=format!("{}님",if str_field(row,"name").is_empty(){"수취인"}else{str_field(row,"name")});
        if !rows(field(row,"unassignedEvidence")).is_empty(){
            add(&mut issues,o,Some(row),None,"UNASSIGNED_EVIDENCE",
                "상품 또는 수량 정보가 연결되지 않았습니다","product",
                row.get("unassignedEvidence"),1,None,None);
        }
        if str_field(row,"name").trim().is_empty(){
            add(&mut issues,o,Some(row),None,"NAME","수취인 이름을 확인하세요","name",
                row.get("name"),1,Some("받으실 분의 성함을 알려주세요.".into()),None);
        }
        let address=str_field(row,"address");
        let legacy=field(o,"receptionVersion").as_i64()!=Some(1);
        let address_ok=address_format_ok(row,legacy);
        if !address_ok{
            let evidence=json!([field(row,"address"),field(row,"addressStatus")]);
            let customer=format!("{who} {}",if !address.trim().is_empty(){
                "상세주소 확인 부탁드려요."
            }else{"배송지 주소 확인 부탁드려요."});
            add(&mut issues,o,Some(row),None,"ADDRESS","상세 배송주소가 부족합니다",
                "address",Some(&evidence),1,Some(customer),None);
        }
        let phone=str_field(row,"phone");
        let mobile=str_field(row,"mobile");
        let contact_ok=contact_format_ok(row);
        if phone.is_empty()&&mobile.is_empty()||!contact_ok{
            let evidence=json!([field(row,"phone"),field(row,"mobile")]);
            let absent=phone.is_empty()&&mobile.is_empty();
            let invalid_mobile=!mobile.is_empty()&&!valid_contact(mobile);
            let (title,field_name)=if absent{("연락처가 없습니다".to_string(),"mobile")}
                else{(contact_problem(if invalid_mobile{mobile}else{phone},invalid_mobile),
                    if invalid_mobile{"mobile"}else{"phone"})};
            add(&mut issues,o,Some(row),None,"CONTACT",title,field_name,
                Some(&evidence),1,Some(format!("{who} 연락처 확인 부탁드려요.")),None);
        }
        for field_name in ["name","address","phone","mobile"]{
            let candidates=rows(field(row,"candidates")).iter()
                .filter(|v|str_field(v,"field")==field_name).cloned().collect::<Vec<_>>();
            let unique=candidates.iter()
                .map(|v|serde_json::to_string(field(v,"value")).unwrap_or_default())
                .collect::<HashSet<_>>();
            let confirmed=rows(field(row,"confirmedFields")).iter()
                .any(|v|v.as_str()==Some(field_name));
            let already_one_address=field_name=="address" &&
                address_candidates_form_one_location(row,&candidates);
            if unique.len()>1 && !confirmed && !already_one_address {
                let evidence=json!(candidates);
                let title=format!("{} 후보 선택 필요",match field_name {
                    "name"=>"수취인명","address"=>"주소",_=>"연락처"
                });
                let kind=format!("CANDIDATE_{field_name}");
                add(&mut issues,o,Some(row),None,&kind,title,field_name,
                    Some(&evidence),1,None,None);
            }
        }
        if rows(field(row,"items")).is_empty(){
            let empty=json!([]);
            add(&mut issues,o,Some(row),None,"ITEM","상품명 누락","product",Some(&empty),1,None,None);
        }
        for item in rows(field(row,"items")){
            let match_kind=str_field(field(item,"match"),"kind");
            let root=match match_kind{
                "QUANTITY"=>"QUANTITY_LINK","CONFLICT"=>"ALIAS_CONFLICT",
                "CATEGORY_CONFLICT"=>"CATEGORY_CONFLICT","CATEGORY"=>"MODEL_CHOICE",_=>"MODEL"
            };
            if str_field(item,"productId").is_empty()||str_field(item,"variantId").is_empty(){
                let title=match root{
                    "QUANTITY_LINK"=>"수량을 연결할 상품이 불명확합니다.",
                    "ALIAS_CONFLICT"=>"상품 연결 정보가 충돌합니다.",
                    "CATEGORY_CONFLICT"=>"상품 종류와 모델이 상충합니다.",
                    "MODEL_CHOICE"=>"모델 선택이 필요합니다.",
                    _=>"미등록 상품입니다."
                };
                let evidence=json!([field(item,"productId"),field(item,"variantId"),field(item,"match")]);
                add(&mut issues,o,Some(row),Some(item),root,title,"product",Some(&evidence),1,None,None);
            }
            if !str_field(item,"productId").is_empty() && !str_field(item,"variantId").is_empty()
                && ["ALIAS_CONFLICT","CATEGORY_CONFLICT","QUANTITY_LINK"].contains(&root){
                let evidence=json!([field(item,"productId"),field(item,"variantId"),field(item,"match")]);
                let title=match root{"ALIAS_CONFLICT"=>"상품 연결 정보가 충돌합니다.",
                    "CATEGORY_CONFLICT"=>"상품 종류와 모델이 상충합니다.",_=>"수량 연결을 확인하세요."};
                add(&mut issues,o,Some(row),Some(item),root,title,
                    if root=="QUANTITY_LINK"{"quantity"}else{"product"},Some(&evidence),1,None,None);
            }
            if field(item,"quantityConfirmed").as_bool()==Some(false)&&root!="QUANTITY_LINK"{
                let evidence=json!([field(item,"quantity"),field(item,"quantityConfirmed")]);
                add(&mut issues,o,Some(row),Some(item),"QUANTITY_MISSING",
                    "상품에 귀속된 수량을 확정하세요.","quantity",Some(&evidence),1,None,None);
            }
            let qty=field(item,"quantity").as_i64();
            if field(item,"quantityConfirmed").as_bool()!=Some(false)&&
                qty.is_none_or(|n|!(-super::MAX_SAFE..=super::MAX_SAFE).contains(&n)||n<1){
                add(&mut issues,o,Some(row),Some(item),"QUANTITY_INVALID","수량을 확인하세요",
                    "quantity",item.get("quantity"),1,None,None);
            }
            let price=field(item,"unitPrice").as_i64();
            if str_field(o,"orderType")!="RETAIL"&&!str_field(o,"clientId").is_empty()&&
               !str_field(item,"productId").is_empty()&&!str_field(item,"variantId").is_empty()&&
                price.is_none_or(|n|!(0..=super::MAX_SAFE).contains(&n)){
                add(&mut issues,o,Some(row),Some(item),"PRICE","판매가를 등록하세요",
                    "price",item.get("unitPrice"),1,None,None);
            }
            if !str_field(item,"productId").is_empty()&&!str_field(item,"variantId").is_empty()&&
                rows(field(item,"issues")).iter().any(|v|v.as_str().is_some_and(|s|
                    s.contains("색상")||s.contains("옵션"))){
                let evidence=json!([field(item,"variantId"),field(item,"color"),field(item,"issues")]);
                add(&mut issues,o,Some(row),Some(item),"OPTION","정확한 상품 구성을 확인하세요",
                    "product",Some(&evidence),1,
                    Some(format!("{who} {} 정확한 상품명을 확인 부탁드려요.",
                        internal_model(str_field(item,"name")))),None);
            }
            if field(field(item,"modelProposal"),"required").as_bool()==Some(true){
                let evidence=json!([field(item,"productId"),field(item,"variantId"),field(item,"modelProposal")]);
                add(&mut issues,o,Some(row),Some(item),"MODEL_CONFIRM",
                    "추천 상품의 고객 확인이 필요합니다","product",Some(&evidence),
                    1,None,Some("PENDING_CUSTOMER"));
            }
            if ["COMMON_INHERIT","CLIENT_ALIAS","COMMON_ALIAS"].contains(&match_kind)&&
                !str_field(item,"productId").is_empty()&&!str_field(item,"variantId").is_empty(){
                let title=match match_kind{
                    "COMMON_INHERIT"=>"공통 상품 자동 적용",
                    "CLIENT_ALIAS"=>"거래처 별칭 적용",_=>"공통 별칭 적용"
                };
                add(&mut issues,o,Some(row),Some(item),"APPLIED",title,"product",
                    item.get("match"),0,None,Some("AUTO_APPLIED"));
            }
        }
    }
    issues
}
/// Staff-facing extension of the unchanged order_issues differential oracle.
 /// The final approval action uses the SAME predicate, never a React copy.
pub(super) fn order_issues_with_actions(order:&Value)->Vec<Value>{
    let mut issues=order_issues(order);
    for issue in &mut issues {
        if str_field(issue,"kind")!="QUANTITY_MISSING" {continue}
        let rid=str_field(issue,"recipientId");
        let iid=str_field(issue,"itemId");
        let eligible=rows(field(order,"recipients")).iter()
            .find(|row|str_field(row,"id")==rid)
            .and_then(|row|rows(field(row,"items")).iter()
                .find(|item|str_field(item,"id")==iid))
            .is_some_and(can_approve_missing_quantity);
        issue["canApproveOne"]=json!(eligible);
    }
    issues
}
pub(super) fn synchronize(order:&mut Value)->Result<(),String>{
    let issues=order_issues(order);
    let old=field(order,"issueResolutions").as_object()
        .cloned().unwrap_or_else(Map::new);
    let mut next=old.clone();
    let mut present=HashSet::new();
    for issue in issues {
        let id=str_field(&issue,"issueId").to_string();
        present.insert(id.clone());
        let previous=old.get(&id);
        let evidence=if let Some(s)=issue.get("evidence"){s.clone()}
            else{field(&issue,"title").clone()};
        let status=field(&issue,"resolution");
        let updated_at=previous.filter(|prior|
            field(prior,"resolution")==status&&field(prior,"evidence")==&evidence)
            .map(|prior|field(prior,"updatedAt")).unwrap_or_else(||field(order,"updatedAt"));
        next.insert(id,json!({
            "resolution":status,"resolutionSource":field(&issue,"resolutionSource"),
            "updatedAt":updated_at,"evidence":evidence
        }));
    }
    for (id,prior) in &old{
        if !present.contains(id)&&
            ["PENDING_CUSTOMER","UNRESOLVED","PROVISIONALLY_APPROVED","AUTO_APPLIED","USER_APPROVED"]
                .contains(&str_field(prior,"resolution")){
            let mut updated=prior.clone();
            set(&mut updated,"resolution",json!("RESOLVED"))?;
            set(&mut updated,"resolutionSource",json!("사용자 수정 또는 현재 원문 반영"))?;
            set(&mut updated,"updatedAt",field(order,"updatedAt").clone())?;
            next.insert(id.clone(),updated);
        }
    }
    set(order,"issueResolutions",Value::Object(next))?;
    Ok(())
}


/// Port of validation.ts approveMissingQuantity and pendingIssue.
pub(super) fn issue_action(payload:&Value)->Result<Value,String> {
    let action=required_str(payload,"action")?;
    let mut order=req(payload,"order")?.clone();
    match action {
        "approve_current_information"|"cancel_current_information"=>{
            let id=required_str(payload,"issueId")?;
            let rid=required_str(payload,"recipientId")?;
            let basis=req(payload,"approvalBasis")?;
            let issue=order_issues(&order).into_iter().find(|issue|
                str_field(issue,"issueId")==id && str_field(issue,"recipientId")==rid
                && field(issue,"approvalBasis")==basis
                && field(issue,if action=="approve_current_information"{
                    "canApproveCurrent"}else{"canCancelCurrent"})==&Value::Bool(true))
                .ok_or("현재 정보가 변경되었거나 승인할 수 없는 검증 항목입니다.")?;
            let kind=str_field(&issue,"kind");
            let iid=str_field(&issue,"itemId");
            let row=order["recipients"].as_array_mut().ok_or("배송 정보 누락")?
                .iter_mut().find(|row|str_field(row,"id")==rid)
                .ok_or("승인할 수취인을 찾을 수 없습니다.")?;
            let key=if iid.is_empty(){kind.to_owned()}else{format!("{iid}:{kind}")};
            if action=="approve_current_information"{
                if !field(row,"validationApprovals").is_object(){
                    set(row,"validationApprovals",json!({}))?;
                }
                set(&mut row["validationApprovals"],&key,json!({
                    "basis":basis,"approvedAt":required_str(payload,"now")?,
                    "approvedBy":required_str(payload,"actor")?
                }))?;
            }else if let Some(records)=row["validationApprovals"].as_object_mut(){
                records.remove(&key);
            }
            set(&mut order,"updatedAt",json!(required_str(payload,"now")?))?;
            synchronize(&mut order)?;
            Ok(json!({"order":order}))
        }
        "approve_missing_quantity"=>{
            let rid=required_str(payload,"recipientId")?;
            let iid=required_str(payload,"itemId")?;
            let ri=rows(field(&order,"recipients")).iter()
                .position(|row|str_field(row,"id")==rid);
            let ii=ri.and_then(|ri|rows(field(&order["recipients"][ri],"items"))
                .iter().position(|i|str_field(i,"id")==iid));
            let bad=ri.zip(ii).is_none_or(|(ri,ii)|
                !can_approve_missing_quantity(&order["recipients"][ri]["items"][ii]));
            if bad{return Err("수량 미기재로 확인된 실제 상품만 1개 잠정 처리할 수 있습니다.".into());}
            let ri=ri.unwrap();
            let ii=ii.unwrap();
            let now=required_str(payload,"now")?;
            let actor=required_str(payload,"actor")?;
            let i=&mut order["recipients"][ri]["items"][ii];
            set(i,"quantity",json!(1))?;
            set(i,"quantityApproval",json!({"at":now,"actor":actor,"quantity":1}))?;
            set(i,"manuallyConfirmed",json!(true))?;
            Ok(json!({"order":order}))
        }
        "pending_issue"=>{
            let id=required_str(payload,"issueId")?;
            let matching=order_issues(&order).into_iter()
                .find(|issue|str_field(issue,"issueId")==id &&
                    str_field(issue,"severity")=="ACTION");
            if let Some(issue)=matching {
                let now=required_str(payload,"now")?;
                let actor=required_str(payload,"actor")?;
                if !field(&order,"issueResolutions").is_object(){
                    set(&mut order,"issueResolutions",json!({}))?;
                }
                let evidence=issue.get("evidence").and_then(Value::as_str).unwrap_or("");
                set(&mut order["issueResolutions"],id,json!({
                    "resolution":"PENDING_CUSTOMER","resolutionSource":actor,
                    "updatedAt":now,"evidence":evidence
                }))?;
            }
            Ok(json!({"order":order}))
        }
        _=>Err(format!("지원하지 않는 주문 이슈 action: {action}"))
    }
}

#[cfg(test)]
mod manual_dealer_selection_review_tests {
    use super::*;

    #[test]
    fn manually_selected_dealer_never_emits_a_redundant_review_notice() {
        let order=json!({
            "id":"order-1","clientId":"dealer-1","recipients":[],
            "clientMatch":{"source":"MANUAL","clientId":"dealer-1",
                "detail":"사용자가 거래처 ‘테스트 상사’를 직접 선택했습니다."}
        });
        let issues=order_issues(&order);
        assert!(issues.iter().all(|issue|
            !str_field(issue,"kind").starts_with("CLIENT_MATCH_")),
            "Direct user selection is an accepted decision, not an alert");
    }

    #[test]
    fn no_match_and_no_dealer_has_only_one_actionable_dealer_card() {
        let order=json!({
            "id":"order-2","clientId":"","orderType":"AGENCY","recipients":[],
            "clientMatch":{"source":"NONE","clientId":null,
                "detail":"거래처를 찾지 못했습니다. 직접 선택하세요."}
        });
        let issues=order_issues(&order);
        let dealers:Vec<_>=issues.iter().filter(|issue|{
            let kind=str_field(issue,"kind");
            kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_")
        }).collect();
        assert_eq!(dealers.len(),1,"Missing auto-match is not a second notice");
        assert_eq!(dealers[0]["kind"],"CLIENT");
        assert_eq!(dealers[0]["severity"],"ACTION");
        assert!(!issues.iter().any(|issue|
            str_field(issue,"title")=="거래처 자동 연결 근거가 없습니다"));
    }

    #[test]
    fn explicit_auto_match_error_is_one_action_not_two() {
        for (source, expected) in [
            ("ERROR","CLIENT_MATCH_ERROR"),
            ("CONFLICT","CLIENT_MATCH_CONFLICT"),
        ] {
            let order=json!({
                "id":"order-3","clientId":"","orderType":"AGENCY","recipients":[],
                "clientMatch":{"source":source,"clientId":"dealer-unknown",
                    "detail":"자동 거래처 연결을 확인해야 합니다."}
            });
            let issues=order_issues(&order);
            let dealers:Vec<_>=issues.iter().filter(|issue|{
                let kind=str_field(issue,"kind");
                kind=="CLIENT" || kind.starts_with("CLIENT_MATCH_")
            }).collect();
            assert_eq!(dealers.len(),1,"{source}: only one dealer-related action");
            assert_eq!(dealers[0]["kind"],expected);
            assert_eq!(dealers[0]["severity"],"ACTION");
        }
    }

    #[test]
    fn automatic_match_provenance_and_conflict_still_appear() {
        let auto=json!({
            "id":"order-1","clientId":"dealer-1","recipients":[],
            "clientMatch":{"source":"RAW_EXPLICIT","clientId":"dealer-1",
                "clientName":"테스트 상사","detail":"발주서에 거래처명이 명시되었습니다."}
        });
        let evidence=order_issues(&auto);
        assert!(evidence.iter().any(|issue|
            str_field(issue,"kind")=="CLIENT_MATCH_EVIDENCE" &&
            str_field(issue,"severity")=="INFO" &&
            str_field(issue,"title")=="거래처 자동선택 (테스트 상사)" &&
            str_field(issue,"detail").is_empty()));

        let mut pending=auto.clone();
        pending["clientId"]=json!("");
        let pending_issues=order_issues(&pending);
        assert!(!pending_issues.iter().any(|issue|
            str_field(issue,"kind")=="CLIENT_MATCH_CONFLICT" ||
            str_field(issue,"kind")=="CLIENT_MATCH_EVIDENCE"));
        assert!(pending_issues.iter().any(|issue|str_field(issue,"kind")=="CLIENT" &&
            str_field(issue,"severity")=="ACTION"));

        let conflict=json!({
            "id":"order-1","clientId":"dealer-1","recipients":[],
            "clientMatch":{"source":"CONFLICT","clientId":"dealer-2",
                "detail":"서로 다른 거래처 근거가 충돌합니다."}
        });
        let conflicting=order_issues(&conflict);
        assert!(conflicting.iter().any(|issue|
            str_field(issue,"kind")=="CLIENT_MATCH_CONFLICT" &&
            str_field(issue,"severity")=="ACTION"));
    }
}

#[cfg(test)]
mod address_fragment_review_tests {
    use super::*;

    fn order(address:&str,segments:&[&str])->Value {
        let candidates=segments.iter().enumerate().map(|(i,s)|json!({
            "id":format!("address:{i}"),"field":"address",
            "value":s,"label":s,"start":i*20,"end":i*20+s.len()
        })).collect::<Vec<_>>();
        json!({
            "id":"address-case","clientId":"dealer","receptionVersion":1,
            "recipients":[{
                "id":"receiver","name":"이유준",
                "address":address,"addressStatus":"COMPLETE",
                "mobile":"010-2308-9794","items":[],
                "candidates":candidates
            }]
        })
    }

    fn has_address_choice(o:&Value)->bool {
        order_issues(o).iter().any(|issue|
            str_field(issue,"kind")=="CANDIDATE_address")
    }

    #[test]
    fn multiline_province_road_and_floor_are_one_complete_address() {
        let o=order(
            "경상남도 창원시 진해구 안청남로 40 커피빈스 1층",
            &["경상남도 창원시","진해구 안청남로 40","커피빈스 1층"]
        );
        let row=&o["recipients"][0];
        assert!(valid_address(str_field(row,"address")));
        assert!(address_candidates_form_one_location(row,rows(field(row,"candidates"))));
        assert!(!has_address_choice(&o),"Do not replace a full address with a fragment");
        assert!(!order_issues(&o).iter().any(|issue|
            str_field(issue,"kind")=="ADDRESS"),"The full address needs no repair");
    }

    #[test]
    fn two_independent_complete_addresses_still_require_review() {
        let o=order(
            "경상남도 창원시 진해구 안청남로 40 서울특별시 마포구 월드컵로 20",
            &["경상남도 창원시 진해구 안청남로 40","서울특별시 마포구 월드컵로 20"]
        );
        assert!(has_address_choice(&o),"Two real destinations must not be auto-approved");
    }

    #[test]
    fn conflicting_administrative_fragments_are_not_auto_approved() {
        let o=order(
            "경상남도 창원시 진해구 안청남로 40 마산합포구",
            &["경상남도 창원시","진해구 안청남로 40","마산합포구"]
        );
        assert!(has_address_choice(&o),"Two different districts require review");
        let altered=order(
            "경상남도 창원시 진해구 안청남로 44 커피빈스 1층",
            &["경상남도 창원시","진해구 안청남로 40","커피빈스 1층"]
        );
        assert!(has_address_choice(&altered),
            "A manually changed address must not be mistaken for an identical composite");
    }
}

#[cfg(test)]
mod quantity_staff_projection_tests {
    use super::*;
    fn order(item:Value)->Value{
        json!({"id":"order-1","clientId":"dealer","recipients":[{
            "id":"r","name":"김가상","address":"서울 종로구 10",
            "phone":"010-1111-2222","mobile":"","items":[item]}]})
    }
    fn item()->Value{
        json!({"id":"i","productId":"p","variantId":"v",
            "name":"BD 709","quantity":null,"quantityConfirmed":false,
            "issues":[],"match":{"kind":"EXACT"}})
    }
    fn eligible_flag(o:&Value)->Option<bool> {
        order_issues_with_actions(o).iter()
            .find(|i|str_field(i,"kind")=="QUANTITY_MISSING")
            .and_then(|i|field(i,"canApproveOne").as_bool())
    }
    #[test] fn staff_button_follows_exact_same_rust_guard_as_actual_action() {
        let good=order(item());
        assert_eq!(eligible_flag(&good),Some(true));
        let args=|o|json!({"action":"approve_missing_quantity","order":o,
            "recipientId":"r","itemId":"i","now":"2026-11-17T09:00:00Z","actor":"직원"});
        let applied=issue_action(&args(good.clone())).unwrap();
        assert_eq!(applied["order"]["recipients"][0]["items"][0]["quantity"],1);
        for mutate in [
            (|i:&mut Value|i["productId"]=json!("")) as fn(&mut Value),
            |i:&mut Value|i["variantId"]=json!(""),
            |i:&mut Value|i["quantityConfirmed"]=json!(true),
            |i:&mut Value|i["match"]=json!({"kind":"QUANTITY"}),
            |i:&mut Value|i["issues"]=json!(["여러 수량 후보"]),
            |i:&mut Value|i["issues"]=json!(["수량이 불명확"]),
        ]{
            let mut blocked=item();
            mutate(&mut blocked);
            let rejected=order(blocked);
            assert_ne!(eligible_flag(&rejected),Some(true));
            assert!(issue_action(&args(rejected)).is_err());
        }
        // Retain the old issue payload for compatibility with existing
        // TypeScript-origin differential tests. Only the UI projection adds
        // an actionable flag.
        assert!(order_issues(&good).iter().all(|i|i.get("canApproveOne").is_none()));
    }
}

#[cfg(test)]
mod native_contact_detail_tests {
    use super::*;
    #[test] fn invalid_phone_explanation_is_rust_generated_without_changing_base_projection(){
        let payload=json!({"rows":[{"id":"r","name":"김가상",
            "phone":"","mobile":"010-123","address":"서울 종로구 10","items":[]}],
            "allowLegacyAddressConfirmation":false});
        let base=delivery_requirements(&payload).unwrap();
        let ui=delivery_requirements_with_messages(&payload).unwrap();
        assert!(base["results"][0].get("mobileProblem").is_none());
        assert_eq!(ui["results"][0]["contact"],false);
        assert_eq!(ui["results"][0]["mobileProblem"],
            "휴대폰 번호가 6자리입니다. 010으로 시작하는 번호는 숫자 11자리가 필요합니다.");
        assert!(ui["results"][0]["phoneProblem"].is_null());
        let valid=json!({"rows":[{"id":"r","phone":"02-1234-5678",
            "mobile":"010-123","address":"서울 종로구 10","items":[]}],
            "allowLegacyAddressConfirmation":false});
        let view=delivery_requirements_with_messages(&valid).unwrap();
        assert_eq!(view["results"][0]["contact"],true);
        assert!(view["results"][0]["mobileProblem"].is_string());
    }
}

#[cfg(test)]
mod current_information_approval_tests {
    use super::*;
    fn sample()->Value{
        json!({"id":"o","clientId":"c","receptionVersion":1,"recipients":[
            {"id":"r","name":"가상 수취인","address":"가평군 문화로 120 #별관",
             "mobile":"0101234567","phone":"","items":[]}]})
    }
    fn action(order:&Value,kind:&str,operation:&str)->Result<Value,String>{
        let issue=order_issues(order).into_iter().find(|i|str_field(i,"kind")==kind).unwrap();
        issue_action(&json!({"order":order,"action":operation,"issueId":issue["issueId"],
            "recipientId":"r","approvalBasis":issue["approvalBasis"],"actor":"사용자",
            "now":"2026-11-27T00:00:00Z"})).map(|v|v["order"].clone())
    }
    #[test]
    fn approvals_are_value_bound_and_shared_by_delivery_and_order_gates(){
        let o=sample();
        let a=action(&o,"ADDRESS","approve_current_information").unwrap();
        let mut a=action(&a,"CONTACT","approve_current_information").unwrap();
        assert!(order_issues(&a).iter().filter(|i|["ADDRESS","CONTACT"].contains(&str_field(i,"kind")))
            .all(|i|i["severity"]=="INFO" && i["resolution"]=="USER_APPROVED"));
        let d=delivery_requirements(&json!({"rows":a["recipients"]})).unwrap();
        assert_eq!(d["results"][0]["address"],true);
        assert_eq!(d["results"][0]["contact"],true);
        a["recipients"][0]["address"]=json!("다른 주소");
        prune_validation_approvals(&mut a["recipients"][0]);
        assert!(order_issues(&a).iter().any(|i|i["kind"]=="ADDRESS"&&i["severity"]=="ACTION"));
        assert!(order_issues(&a).iter().any(|i|i["kind"]=="CONTACT"&&i["severity"]=="INFO"));
        a["recipients"][0]["address"]=o["recipients"][0]["address"].clone();
        assert!(order_issues(&a).iter().any(|i|i["kind"]=="ADDRESS"&&i["severity"]=="ACTION"));
        let a=action(&a,"CONTACT","cancel_current_information").unwrap();
        assert!(order_issues(&a).iter().any(|i|i["kind"]=="CONTACT"&&i["severity"]=="ACTION"));
    }
    #[test]
    fn candidate_approval_cannot_follow_changed_value_or_candidates(){
        let mut o=sample();
        o["recipients"][0]["candidates"]=json!([
            {"field":"name","value":"가상 수취인","label":"가상 수취인"},
            {"field":"name","value":"다른 수취인","label":"다른 수취인"}]);
        let mut o=action(&o,"CANDIDATE_name","approve_current_information").unwrap();
        assert!(order_issues(&o).iter().any(|i|i["kind"]=="CANDIDATE_name" && i["resolution"]=="USER_APPROVED"));
        o["recipients"][0]["candidates"][1]["value"]=json!("새 후보");
        prune_validation_approvals(&mut o["recipients"][0]);
        assert!(order_issues(&o).iter().any(|i|i["kind"]=="CANDIDATE_name" && i["severity"]=="ACTION"));
    }
    #[test]
    fn linked_item_approvals_are_independent_and_cannot_create_missing_values(){
        let mut o=sample();
        o["recipients"][0]["items"]=json!([
            {"id":"i","name":"709","productId":"p","variantId":"v","color":"Black",
             "quantity":3,"quantityConfirmed":false,"unitPrice":1000,
             "issues":["색상 확인 필요"]},
            {"id":"j","name":"709","productId":"p","variantId":"v","color":"Black",
             "quantity":2,"quantityConfirmed":true,"unitPrice":1000,
             "issues":["색상 확인 필요"]}]);
        let mut o=action(&o,"OPTION","approve_current_information").unwrap();
        let issues=order_issues(&o);
        assert!(issues.iter().any(|i|i["kind"]=="OPTION"&&i["itemId"]=="i"&&i["resolution"]=="USER_APPROVED"));
        assert!(issues.iter().any(|i|i["kind"]=="OPTION"&&i["itemId"]=="j"&&i["severity"]=="ACTION"));
        o=action(&o,"QUANTITY_MISSING","approve_current_information").unwrap();
        assert!(quantity_accepted(&o["recipients"][0],&o["recipients"][0]["items"][0]));
        o["recipients"][0]["items"][0]["quantity"]=json!(4);
        prune_validation_approvals(&mut o["recipients"][0]);
        assert!(!quantity_accepted(&o["recipients"][0],&o["recipients"][0]["items"][0]));
        assert!(order_issues(&o).iter().any(|i|i["kind"]=="OPTION"&&i["itemId"]=="i"&&i["resolution"]=="USER_APPROVED"));
        o["recipients"][0]["items"][0]["quantity"]=Value::Null;
        assert!(action(&o,"QUANTITY_MISSING","approve_current_information").is_err());
        o["recipients"][0]["items"][0]["variantId"]=Value::Null;
        assert!(!order_issues(&o).iter().any(|i|i["itemId"]=="i"&&i["canApproveCurrent"]==true));
    }
    #[test]
    fn missing_data_and_stale_basis_cannot_be_approved(){
        let mut o=sample();
        let issue=order_issues(&o).into_iter().find(|i|i["kind"]=="ADDRESS").unwrap();
        o["recipients"][0]["address"]=json!("");
        assert!(action(&o,"ADDRESS","approve_current_information").is_err());
        o["recipients"][0]["address"]=json!("changed address");
        assert!(issue_action(&json!({"order":o,"action":"approve_current_information",
            "issueId":issue["issueId"],"recipientId":"r","approvalBasis":issue["approvalBasis"],
            "actor":"사용자","now":"2026-11-27T00:00:00Z"})).is_err());
    }
}
