//! Preservation constraints from validateWorkflow / validateReceptionBoundary.
//! Invalid state is rejected without producing any partial mutation.
use std::collections::{HashMap,HashSet};
use serde_json::Value;
use super::{checked_sum, field, find_by_id, flag, has_value, rows, safe, str_field};

// Persisted audit identity, not display copy.
const LEGACY_FREIGHT_INPUT_EVENT:&str="실제 출고 운임 입력";

fn unique_id(values: &Value) -> bool {
    let mut seen = HashSet::new();
    rows(values).iter().all(|v| seen.insert(str_field(v, "id").to_string()))
}
fn array_has(values: &Value, id: &str) -> bool { find_by_id(values,id).is_some() }
fn unchanged_records(old: &Value, new: &Value) -> bool {
    let old=rows(old);
    let new=rows(new);
    // The normal audit path appends new events without touching the old
    // prefix. Compare once; no hash allocation or repeated ID lookup.
    if new.starts_with(old){
        let mut seen=HashSet::with_capacity(old.len());
        if old.iter().all(|row|seen.insert(str_field(row,"id"))){return true;}
    }
    // Preserve find_by_id's first-match semantics for reordered or malformed
    // lists, including empty IDs. Global uniqueness is validated separately.
    let mut index=HashMap::with_capacity(new.len());
    for row in new{
        index.entry(str_field(row,"id")).or_insert(row);
    }
    old.iter().all(|row|index.get(str_field(row,"id")).copied()==Some(row))
}
fn shipment_for<'a>(state: &'a Value, ship: &Value) -> Result<(&'a Value,&'a Value),String> {
    let order = find_by_id(field(state,"orders"),str_field(ship,"orderId"))
        .ok_or_else(|| super::terminology::SHIPMENT_LINK_REQUIRED.to_string())?;
    let row = find_by_id(field(order,"recipients"),str_field(ship,"recipientId"))
        .ok_or_else(|| super::terminology::SHIPMENT_LINK_REQUIRED.to_string())?;
    // shipmentContext() requires orderParty(), not merely a recipient.
    // A retail order synthesizes its company party, while agency/dealer
    // orders require a live client or their own frozen clientSnapshot.
    if str_field(order,"orderType")!="RETAIL" &&
        find_by_id(field(state,"clients"),str_field(order,"clientId")).is_none() &&
        !has_value(field(order,"clientSnapshot")) {
        return Err(super::terminology::SHIPMENT_LINK_REQUIRED.into());
    }
    Ok((order,row))
}
// Shared semantic validation for one shipment; the general writer still
// checks all shipments, whereas factory replies check only their targets.
fn validate_shipment_row(state:&Value,w:&Value,s:&Value)->Result<(),String>{
    let (order,_recipient) = shipment_for(state,s)?;
    let group = find_by_id(field(w,"groups"),str_field(s,"groupId"));
    let step = field(s,"step").as_i64();
    if str_field(order,"orderType") == "RETAIL" {
        let has_payment = rows(field(w,"payments")).iter().any(|p|
            rows(field(p,"allocations")).iter().any(|a| str_field(a,"shipmentId")==str_field(s,"id")));
        let has_confirmation = rows(field(w,"confirmations")).iter().any(|c|
            rows(field(c,"shipmentIds")).iter().any(|v|v.as_str()==Some(str_field(s,"id"))));
        if !flag(s,"transferred") || step != Some(4)
            || group.is_some_and(|g|!rows(field(g,"notices")).is_empty())
            || has_payment || has_confirmation {
            return Err("소매 주문에는 입금 / 계좌 안내 단계를 만들 수 없습니다.".into());
        }
    }
    let source = find_by_id(field(w,"sources"),str_field(s,"sourceId"));
    let valid_link = match (group,source) {
        (Some(g),Some(src)) => str_field(src,"orderId") == str_field(order,"id")
            && str_field(g,"sourceId") == str_field(src,"id")
            && str_field(g,"dealerId") == str_field(order,"clientId"),
        _ => false
    };
    if !valid_link {return Err("발주서 / 묶음 / 배송 건 연결 오류".into());}
    if step.is_none_or(|n| !(0..=4).contains(&n)) {
        return Err("잘못된 진행 단계".into());
    }
    if s.get("stageValidity").is_some(){
        let stages=super::workflow_stages::explicit(s).ok_or("업무 단계 상태 형식 오류")?;
        if Some(super::workflow_stages::prefix(stages))!=step || (stages[3]&&!stages[2]){
            return Err("업무 단계 상태 불일치".into());
        }
    }
    for v in field(s,"actualFreight").as_object().into_iter().flat_map(|m|m.values()) {
        if !v.is_null() && safe(v).is_none() {return Err("출고 운임 금액 오류".into());}
    }
    for v in field(s,"charges").as_object().into_iter().flat_map(|m|m.values()) {
        if !v.is_null() && safe(v).is_none() {return Err("운임 금액 오류".into());}
    }
    Ok(())
}

pub(super) fn validate(state: &Value, before: Option<&Value>) -> Result<(),String> {
    validate_with_prior_core(state,before,before)
}

/// The caller may borrow the same authoritative order/client trees for
/// both snapshots only when relational core data was proven unchanged.
pub(super) fn validate_with_prior_core(state:&Value,before:Option<&Value>,
    prior_core:Option<&Value>)->Result<(),String>{
    let w = field(state,"workflow");
    if w.is_null() {
        if before.is_some_and(|old|!field(old,"workflow").is_null()){
            return Err("기존 업무 상태 전체를 제거할 수 없습니다.".into());
        }
        return Ok(());
    }
    if !w.is_object(){return Err("업무 상태 형식 오류".into());}
    for name in ["sources","groups","shipments","payments","queue","events","deliveries","changeRequests"] {
        if !unique_id(field(w,name)) { return Err("업무 기록 ID가 중복됩니다.".into()); }
    }
    for s in rows(field(w,"shipments")) {
        if flag(s,"legacyArchived") &&
            !find_by_id(field(state,"orders"),str_field(s,"orderId"))
                .is_some_and(|order|rows(field(order,"recipients")).iter()
                    .any(|recipient|str_field(recipient,"id")==str_field(s,"recipientId"))) {
            continue;
        }
        validate_shipment_row(state,w,s)?;
    }
    for p in rows(field(w,"payments")) {
        // TypeScript evaluates amount(payment.amount) first, then sum()
        // before checking individual allocation validity. sum() accepts
        // negative/decimal number inputs to the arithmetic step, but throws
        // an independent "금액 합계를 확인하세요." when its result is not a
        // nonnegative safe integer.
        let Some(payment_amount)=safe(field(p,"amount")) else {
            return Err("입금액 / 배분액 오류".into());
        };
        let mut total=0_f64;
        for allocation in rows(field(p,"allocations")) {
            let value=field(allocation,"amount");
            let amount=match value {
                Value::Number(n)=>n.as_f64().unwrap_or(f64::NAN),
                Value::Null=>0.0,
                Value::Bool(v)=>if *v{1.0}else{0.0},
                _=>f64::NAN
            };
            total+=amount;
        }
        if !total.is_finite() || total<0.0 || total.fract()!=0.0 ||
            total > super::MAX_SAFE as f64 {
            return Err("금액 합계를 확인하세요.".into());
        }
        if total>payment_amount as f64 {
            return Err("입금액 / 배분액 오류".into());
        }
        for allocation in rows(field(p,"allocations")) {
            let shipment = find_by_id(field(w,"shipments"),str_field(allocation,"shipmentId"));
            if safe(field(allocation,"amount")).is_none() ||
                !shipment.is_some_and(|s|shipment_for(state,s)
                    .is_ok_and(|(order,_)|str_field(order,"clientId")==str_field(p,"dealerId"))) {
                return Err("입금 배분 연결 오류".into());
            }
        }
    }
    for q in rows(field(w,"queue")) {
        if !array_has(field(w,"shipments"),str_field(q,"shipmentId")) {
            return Err("엑셀 관리 배송 연결 오류".into());
        }
    }
    let mut delivered = HashSet::new();
    for d in rows(field(w,"deliveries")) {
        let key = format!("{}|{}",str_field(d,"shipmentId"),str_field(d,"origin"));
        if !delivered.insert(key) {
            return Err("같은 배송 건 / 출고지의 배송 완료 기록이 중복됩니다.".into());
        }
    }
    for c in rows(field(w,"confirmations")) {
        if safe(field(c,"amount")).is_none() ||
           !array_has(field(w,"groups"),str_field(c,"groupId")) ||
           rows(field(c,"allocations")).iter().any(|a| safe(field(a,"amount")).is_none() ||
               !array_has(field(w,"shipments"),str_field(a,"shipmentId"))) {
            return Err("입금 확인 연결 / 금액 오류".into());
        }
    }
    if !unique_id(field(w,"confirmations")) {
        return Err("입금 확인 기록 중복".into());
    }
    if let Some(old_state) = before {
        let old_w = field(old_state,"workflow");
        if !old_w.is_null() {
            for old in rows(field(old_w,"confirmations")) {
                let current = find_by_id(field(w,"confirmations"),str_field(old,"id"));
                let old_ids=rows(field(old,"shipmentIds")).iter()
                    .filter_map(Value::as_str).collect::<Vec<_>>();
                if let Some(current)=current {
                    if current==old {continue;}
                    let current_ids=rows(field(current,"shipmentIds")).iter()
                        .filter_map(Value::as_str).collect::<Vec<_>>();
                    let removed=old_ids.iter().filter(|id|!current_ids.contains(id))
                        .copied().collect::<Vec<_>>();
                    let mut expected=old.clone();
                    let surviving=old_ids.iter().filter(|id|current_ids.contains(id))
                        .copied().collect::<Vec<_>>();
                    let allocations=rows(field(old,"allocations")).iter()
                        .filter(|a|surviving.contains(&str_field(a,"shipmentId")))
                        .cloned().collect::<Vec<_>>();
                    let mut amounts=serde_json::Map::new();
                    for id in &surviving {
                        let previous=field(field(old,"amounts"),id);
                        amounts.insert((*id).to_string(),if previous.is_null() {
                            serde_json::json!(0)
                        } else {previous.clone()});
                    }
                    let total=amounts.values().filter_map(safe).collect::<Vec<_>>();
                    let amount=checked_sum(total).ok_or("금액 합계를 확인하세요.")?;
                    super::set(&mut expected,"shipmentIds",serde_json::json!(surviving))?;
                    super::set(&mut expected,"allocations",serde_json::json!(allocations))?;
                    super::set(&mut expected,"amounts",Value::Object(amounts))?;
                    super::set(&mut expected,"amount",serde_json::json!(amount))?;
                    let allowed=!removed.is_empty() && current==&expected &&
                        removed.iter().all(|id|find_by_id(field(w,"shipments"),id)
                            .is_some_and(|s|!flag(s,"transferred") &&
                                !super::workflow_stages::read(s)[3]));
                    if !allowed {
                        return Err("확인 기록은 입금 확인 취소에서만 삭제할 수 있습니다.".into());
                    }
                } else {
                    let allowed=old_ids.iter().all(|id|
                        find_by_id(field(w,"shipments"),id)
                            .is_some_and(|s|!flag(s,"transferred") &&
                                !super::workflow_stages::read(s)[3]));
                    if !allowed {
                        return Err("이관 전 입금 확인 취소에서만 기록을 삭제할 수 있습니다.".into());
                    }
                }
            }            for old in rows(field(old_w,"shipments")) {
                let Some(current) = find_by_id(field(w,"shipments"),str_field(old,"id")) else {
                    return Err("배송 건 고유 ID와 최초 연결은 보존해야 합니다.".into());
                };
                for key in ["sourceId","orderId","recipientId"] {
                    if field(old,key) != field(current,key) {
                        return Err("배송 건 고유 ID와 최초 연결은 보존해야 합니다.".into());
                    }
                }
                for origin in ["STORE","FACTORY"] {
                    let old_actual=field(old,"actualFreight");
                    let current_actual=field(current,"actualFreight");
                    let old_freight = field(old_actual,origin);
                    let current_freight = field(current_actual,origin);
                    // TypeScript uses JSON.stringify(undefined) vs
                    // JSON.stringify(null): an omitted origin differs from
                    // one explicitly present with null. Treat presence as
                    // part of the immutable audit value.
                    if old_actual.get(origin).is_some()==current_actual.get(origin).is_some() &&
                        old_freight == current_freight {continue;}
                    let old_recipient=shipment_for(prior_core.unwrap_or(old_state),old)?.1;
                    let original_freight=if field(old,"actualFreight").get(origin).is_some() {
                        old_freight
                    } else {
                        field(old_recipient,"actualFreight")
                    };
                    let has_evidence = field(current,"actualFreight").get(origin).is_some() &&
                        rows(field(w,"events")).iter().any(|event| {
                            !array_has(field(old_w,"events"),str_field(event,"id")) &&
                            str_field(event,"action") == LEGACY_FREIGHT_INPUT_EVENT &&
                            rows(field(event,"shipmentIds")).iter()
                                .any(|id|id.as_str()==Some(str_field(old,"id"))) &&
                            field(event,"before")==original_freight &&
                            str_field(field(event,"after"),"origin")==origin &&
                            field(field(event,"after"),"amount")==current_freight
                        });
                    if !has_evidence {
                        return Err("출고 운임의 수정 근거를 확인하세요.".into());
                    }
                }
                if has_value(field(old,"delivery")) &&
                    field(old,"delivery") != field(current,"delivery") {
                    return Err("배송 완료 이력은 보존해야 합니다.".into());
                }
            }
            for old in rows(field(old_w,"groups")) {
                let current = find_by_id(field(w,"groups"),str_field(old,"id"));
                for notice in rows(field(old,"notices")) {
                    if current.and_then(|g|find_by_id(field(g,"notices"),str_field(notice,"id")))
                        != Some(notice) {
                        return Err("기존 계좌 안내 이력은 보존해야 합니다.".into());
                    }
                }
            }
            for name in ["payments","events","deliveries","sources","changeRequests"] {
                if !unchanged_records(field(old_w,name),field(w,name)) {
                    return Err("기존 입금 / 원문 / 배송 / 조작 이력은 변경하거나 삭제할 수 없습니다.".into());
                }
            }
            for old in rows(field(old_w,"queue")) {
                let current = find_by_id(field(w,"queue"),str_field(old,"id"))
                    .ok_or_else(||"엑셀 이관 연결과 확정 정보는 보존해야 합니다.".to_string())?;
                if field(old,"shipmentId") != field(current,"shipmentId") ||
                    field(old,"origin") != field(current,"origin") ||
                    (str_field(old,"status")!="CANCELLED" &&
                        field(old,"snapshot") != field(current,"snapshot")) {
                    return Err("엑셀 이관 연결과 확정 정보는 보존해야 합니다.".into());
                }
                if ["EXPORTED","STORE_SENT"].contains(&str_field(old,"status")) &&
                    current != old {
                    // Both exceptions are explicit in the original TypeScript:
                    // staff handoff confirmation and pre-delivery print disposal.
                    let old_file_only=flag(old,"fileOnly");
                    let old_delivered=has_value(field(old,"factoryDeliveredAt"));
                    let current_delivered=has_value(field(current,"factoryDeliveredAt"));
                    let mut metadata_view=current.clone();
                    if let Some(object)=metadata_view.as_object_mut() {
                        for key in ["fileOnly","factoryDeliveredAt"] {
                            if let Some(value)=old.get(key) {
                                object.insert(key.to_string(),value.clone());
                            } else {
                                object.remove(key);
                            }
                        }
                    }
                    let metadata_only=old_file_only && !flag(current,"fileOnly") &&
                        current_delivered && metadata_view==*old &&
                        rows(field(w,"events")).iter().any(|e|
                            super::terminology::canonical_event_action(str_field(e,"action"))==super::terminology::FACTORY_DELIVERY_CONFIRMED &&
                            rows(field(e,"shipmentIds")).iter()
                                .any(|sid|sid.as_str()==Some(str_field(old,"shipmentId"))));
                    let reopened=old_file_only && !old_delivered &&
                        str_field(current,"status")=="CANCELLED" &&
                        rows(field(w,"events")).iter().any(|e|
                            str_field(e,"action")=="전달 전 출력본 폐기 / 재작성" &&
                            rows(field(e,"shipmentIds")).iter()
                                .any(|sid|sid.as_str()==Some(str_field(old,"shipmentId")))) &&
                        rows(field(state,"exports")).iter().any(|e|
                            rows(field(e,"snapshots")).iter().any(|q|
                                field(q,"id")==field(old,"id") &&
                                field(q,"snapshot")==field(old,"snapshot")));
                    if !metadata_only && !reopened {
                        return Err("이미 전달한 엑셀 / 발송 기록은 보존해야 합니다.".into());
                    }
                }
            }

        }
    }
    Ok(())
}
/// Dedicated factory transition. The caller has already proved that the
/// relational order/catalog trees are unchanged. Never revalidate unrelated
/// payment, queue, group, delivery or reception records merely on import.
pub(super) fn validate_factory_transition(state:&Value,before:&Value)->Result<(),String>{
    let old=field(before,"workflow");
    let new=field(state,"workflow");
    let a=old.as_object().ok_or("기존 업무 상태 오류")?;
    let b=new.as_object().ok_or("업무 상태 형식 오류")?;
    let protected=|key:&str|!matches!(key,"shipments"|"events");
    if !a.iter().filter(|(k,_)|protected(k)).eq(b.iter().filter(|(k,_)|protected(k))){
        return Err("공장 회신이 다른 업무 기록을 변경했습니다.".into());
    }
    let prior=field(before,"factoryReplies");
    let old_replies=if prior.is_null(){&[][..]}else{prior.as_array().ok_or("공장 회신 이력 오류")?.as_slice()};
    let replies=field(state,"factoryReplies").as_array().ok_or("공장 회신 이력 오류")?;
    if replies.len()!=old_replies.len()+1||!replies.starts_with(old_replies){
        return Err("기존 공장 회신 이력은 보존해야 합니다.".into());
    }
    let reply=&replies[old_replies.len()];
    let reply_id=str_field(reply,"id");
    if reply_id.is_empty()||old_replies.iter().any(|r|str_field(r,"id")==reply_id)
        || !matches!(str_field(reply,"status"),"APPLIED"|"PARTIAL"|"REVIEW"){
        return Err("공장 회신 기록 ID 또는 상태 오류".into());
    }
    let reply_rows=field(reply,"rows").as_array().ok_or("공장 회신 행 목록 오류")?;
    let mut applied=HashMap::new();
    let mut row_numbers=HashSet::new();
    for row in reply_rows{
        if !field(row,"row").as_u64().is_some_and(|n|n>=2&&row_numbers.insert(n)){
            return Err("공장 회신 행 번호 오류".into());
        }
        if flag(row,"applied"){
            let id=str_field(row,"shipmentId");
            let tracking=str_field(row,"tracking");
            if id.is_empty()||applied.insert(id,row).is_some()||!(8..=30).contains(&tracking.len())
                ||!tracking.bytes().all(|v|v.is_ascii_digit())||safe(field(row,"actual")).is_none(){
                return Err("반영된 공장 회신 행의 배송/송장/운임 오류".into());
            }
        }
    }
    if !field(reply,"resolves").is_null(){
        let origin=find_by_id(prior,str_field(field(reply,"resolves"),"id"))
            .ok_or("원본 공장 회신 누락")?;
        if !rows(field(origin,"rows")).iter().any(|r|field(r,"row")==field(field(reply,"resolves"),"row")){
            return Err("원본 공장 회신 행 누락".into());
        }
    }
    let old_events=field(old,"events").as_array().ok_or("기존 업무 이벤트 목록 오류")?;
    let events=field(new,"events").as_array().ok_or("업무 이벤트 목록 오류")?;
    if !events.starts_with(old_events){return Err("기존 업무 이벤트는 보존해야 합니다.".into());}
    let new_events=&events[old_events.len()..];
    let mut event_ids:HashSet<&str>=old_events.iter().map(|e|str_field(e,"id")).collect();
    let mut events_by_shipment:HashMap<&str,Vec<&Value>>=HashMap::new();
    for e in new_events{
        let id=str_field(e,"id");
        let targets=rows(field(e,"shipmentIds"));
        if id.is_empty()||!event_ids.insert(id)||targets.len()!=1
            || !targets[0].as_str().is_some_and(|id|applied.contains_key(id)){
            return Err("공장 회신 이벤트 연결 또는 ID 오류".into());
        }
        events_by_shipment.entry(targets[0].as_str().unwrap_or("")).or_default().push(e);
        match str_field(e,"action"){
            LEGACY_FREIGHT_INPUT_EVENT=>{},
            "공장 회신 반영"|"공장 송장 / 운임 정정"=>{
                if str_field(field(e,"after"),"fileId")!=reply_id{return Err("공장 회신 이벤트 원본 연결 오류".into());}
            },
            _=>return Err("공장 회신에서 허용되지 않는 이벤트입니다.".into()),
        }
    }
    let old_ships=field(old,"shipments").as_array().ok_or("기존 배송 목록 오류")?;
    let ships=field(new,"shipments").as_array().ok_or("배송 목록 오류")?;
    if old_ships.len()!=ships.len(){return Err("배송 건 고유 ID와 최초 연결은 보존해야 합니다.".into());}
    let mut checked=HashSet::new();
    // Index all tracking owners to retain collision protection, even outside
    // the selected order scope. No unrelated shipment semantics are checked.
    let mut owners:HashMap<&str,usize>=HashMap::new();
    for ship in ships{
        let tracking=str_field(field(ship,"tracking"),"FACTORY");
        if !tracking.is_empty(){*owners.entry(tracking).or_default()+=1;}
    }
    for (previous,ship) in old_ships.iter().zip(ships){
        let id=str_field(ship,"id");
        let Some(row)=applied.get(id).copied() else{
            if ship!=previous{return Err("회신 대상이 아닌 배송 건이 변경됐습니다.".into());}
            continue;
        };
        if !checked.insert(id)||str_field(previous,"id")!=id||flag(ship,"legacyArchived"){
            return Err("공장 회신 배송 연결 오류".into());
        }
        // Compare every non-factory field, including stage, charges,
        // delivery, payment links and other origins.
        let mut restored=ship.clone();
        for key in ["tracking","actualFreight"]{
            let current=field(ship,key);
            let original=field(previous,key);
            let other=|value:&Value|value.as_object().map(|m|
                m.iter().filter(|(k,_)|k.as_str()!="FACTORY").map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<String,Value>>()).unwrap_or_default();
            if other(current)!=other(original) {return Err("공장 외 출고 정보는 변경할 수 없습니다.".into());}
            let map=restored.as_object_mut().ok_or("배송 상태 형식 오류")?;
            if let Some(value)=previous.get(key){map.insert(key.into(),value.clone());}else{map.remove(key);}
        }
        if restored!=*previous{return Err("공장 회신이 다른 배송 정보를 변경했습니다.".into());}
        // Factory reply requires the original shipping target, not the
        // current reception stage, client master or customer charge validity.
        let recipient=find_by_id(field(state,"orders"),str_field(ship,"orderId"))
            .and_then(|order|find_by_id(field(order,"recipients"),str_field(ship,"recipientId")))
            .ok_or("공장 회신 배송 연결 오류")?;
        let actual=|s:&Value|field(s,"actualFreight").get("FACTORY").cloned().unwrap_or_else(||field(recipient,"actualFreight").clone());
        let tracking=str_field(field(ship,"tracking"),"FACTORY");
        if field(ship,"orderId")!=field(row,"orderId")||tracking!=str_field(row,"tracking")
            || actual(ship)!=*field(row,"actual")||owners.get(tracking)!=Some(&1){
            return Err("공장 회신 반영값과 배송 기록 불일치".into());
        }
        let evidence=events_by_shipment.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let previous_actual=actual(previous);
        if field(previous,"actualFreight").get("FACTORY")!=field(ship,"actualFreight").get("FACTORY")
            && !evidence.iter().any(|e|str_field(e,"action")==LEGACY_FREIGHT_INPUT_EVENT
                && field(e,"before")==&previous_actual&&str_field(field(e,"after"),"origin")=="FACTORY"
                && field(field(e,"after"),"amount")==field(row,"actual")){
            return Err("출고 운임의 수정 근거를 확인하세요.".into());
        }
        if ship!=previous&&!evidence.iter().any(|e|matches!(str_field(e,"action"),"공장 회신 반영"|"공장 송장 / 운임 정정")
            && field(field(e,"before"),"tracking")==field(field(previous,"tracking"),"FACTORY")
            && field(field(e,"before"),"actual")==&previous_actual
            && field(field(e,"after"),"tracking")==field(row,"tracking")
            && field(field(e,"after"),"actual")==field(row,"actual")){
            return Err("공장 회신의 변경 근거를 확인하세요.".into());
        }
    }
    if checked.len()!=applied.len(){return Err("공장 회신 배송 연결 오류".into());}
    Ok(())
}

/// Exact source-equivalent sender requirements from order-types.ts.
pub(super) fn company_sender_missing(state:&Value)->Vec<String>{
    let settings=field(state,"settings");
    let mut issues=Vec::new();
    if str_field(settings,"senderAddress").trim().is_empty(){
        issues.push("설정의 반도스탠드 주소를 등록하세요.".into());
    }
    let preferred=if str_field(settings,"senderPhone").is_empty(){
        str_field(settings,"senderMobile")
    }else{str_field(settings,"senderPhone")};
    if preferred.trim().is_empty(){
        issues.push("설정의 반도스탠드 연락처를 등록하세요.".into());
    }
    issues
}

// Mirrors the source retailIssues() order: regular order ACTION issues,
// dispatch/freight/product/quantity per recipient, company sender and sale
// state. Deduplicate titles, preserving the original first occurrence order.
pub(super) fn retail_issues(order:&Value,state:&Value)->Vec<String>{
    let mut issues=super::issues::order_issues(order).into_iter()
        .filter(|i|str_field(i,"severity")=="ACTION")
        .map(|i|str_field(&i,"title").to_string()).collect::<Vec<_>>();
    for row in rows(field(order,"recipients")){
        if !["FACTORY","STORE"].contains(&str_field(row,"dispatchOrigin")){
            issues.push("출고지를 선택하세요.".into());
        }
        let actual=field(row,"actualFreight");
        if !actual.is_null()&&safe(actual).is_none(){
            issues.push(super::terminology::DISPATCH_FREIGHT_REQUIRED.into());
        }
        for item in rows(field(row,"items")){
            let found=rows(field(state,"products")).iter().any(|p|
                str_field(p,"id")==str_field(item,"productId")&&
                !has_value(field(p,"deletedAt"))&&
                !has_value(field(p,"supersededBy"))&&
                field(p,"active").as_bool()!=Some(false)&&
                array_has(field(p,"variants"),str_field(item,"variantId")));
            if !found {issues.push("등록된 상품과 옵션을 선택하세요.".into());}
            if field(item,"quantityConfirmed").as_bool()!=Some(true)
                && !super::issues::quantity_user_approved(row,item){
                issues.push("상품 수량을 직접 확정하세요.".into());
            }
        }
    }
    // Sender contact is checked at output, not at retail intake.
    for row in rows(field(order,"recipients")){
        for item in rows(field(row,"items")){
            let pid=str_field(item,"productId");
            if pid.is_empty(){continue;}
            let product=super::workflow_issues::historical_product(
                state,pid,str_field(item,"variantId"));
            let problem=if product.is_none(){"상품 연결 확인 필요"}
                else{super::workflow_issues::sale_problem(product)};
            if !problem.is_empty() && !super::workflow_issues::sale_approved(state,row,item){
                issues.push(format!("{}: {}",
                    if str_field(row,"name").is_empty(){"수취인"}else{str_field(row,"name")},
                    super::workflow_issues::sale_issue_detail(str_field(item,"name"),problem)));
            }
        }
    }
    let mut seen=HashSet::new();
    issues.into_iter().filter(|issue|seen.insert(issue.clone())).collect()
}
fn snapshot_sender(order:&Value,state:&Value)->Value{
    if ["DEALER","RETAIL"].contains(&str_field(order,"orderType")){
        return super::reconciliation::company_client(state);
    }
    let client=find_by_id(field(state,"clients"),str_field(order,"clientId"));
    let raw=field(order,"analysisRaw").as_str()
        .unwrap_or_else(||str_field(order,"raw"));
    client.and_then(|base|super::reconciliation::explicit_sender(raw,base))
        .or_else(||client.cloned())
        .unwrap_or(Value::Null)
}
pub(super) fn company_party(state:&Value)->Value{
    let mut company=super::reconciliation::company_client(state);
    let _=super::set(&mut company,"name",serde_json::json!("반도스탠드"));
    company
}
fn truthy(value:&Value)->bool {
    match value {
        Value::Null=>false,
        Value::Bool(b)=>*b,
        Value::Number(n)=>n.as_f64().is_some_and(|x|x!=0.0),
        Value::String(s)=>!s.is_empty(),
        _=>true
    }
}
pub(super) fn validate_reception(before: &Value, state: &Value) -> Result<(),String> {
    for order in rows(field(state,"orders")) {
        let previous=find_by_id(field(before,"orders"),str_field(order,"id"));
        let order_type=str_field(order,"orderType");
        if !order_type.is_empty()&&!["AGENCY","DEALER","RETAIL"].contains(&order_type){
            return Err("주문 유형을 확인하세요.".into());
        }
        let fresh=previous.is_none();
        let v1=field(order,"receptionVersion").as_i64()==Some(1);
        if fresh&&order_type=="RETAIL"&&!v1 {
            return Err("소매 주문 전용 접수 절차를 사용하세요.".into());
        }
        if fresh&&v1&&(!truthy(field(order,"confirmedAt"))||
            !truthy(field(order,"confirmationSnapshot"))){
            return Err("주문 확인 완료 전에는 정식 주문으로 저장할 수 없습니다.".into());
        }
        if let Some(old)=previous{
            if truthy(field(old,"confirmedAt"))&&
                (field(old,"confirmedAt")!=field(order,"confirmedAt")||
                 field(old,"raw")!=field(order,"raw")||
                 field(old,"confirmationSnapshot")!=field(order,"confirmationSnapshot")){
                return Err("최초 확정 원문 / 시각 / 주문 스냅샷은 변경할 수 없습니다.".into());
            }
            if field(old,"orderType")!=field(order,"orderType")||
                old.get("senderSnapshot")!=order.get("senderSnapshot"){
                return Err("확정 주문의 유형과 송하인 스냅샷은 변경할 수 없습니다.".into());
            }
        }
        if !fresh||!v1{continue;}
        let expected_sender=snapshot_sender(order,state);
        if field(order,"senderSnapshot")!=&expected_sender{
            return Err("주문 유형에 맞는 송하인 스냅샷을 확인하세요.".into());
        }
        if order_type=="RETAIL"&&field(order,"clientSnapshot")!=&company_party(state){
            return Err("소매 주문의 주문처는 반도스탠드입니다.".into());
        }
        if order_type=="RETAIL"{
            let problems=retail_issues(order,state);
            if !problems.is_empty(){return Err(problems.join("\n"));}
            if truthy(field(order,"clientId"))||truthy(field(order,"raw"))||
                truthy(field(order,"analysisSnapshot"))||
                rows(field(order,"recipients")).iter().any(|row|
                    rows(field(row,"items")).iter().any(|item|
                        item.get("unitPrice")!=Some(&Value::Null)))||
                !truthy(field(order,"paid")){
                return Err("소매 주문은 별도 판매금액 / 원문 분석 없이 접수하세요.".into());
            }
        }
        // Original source checks product registration BEFORE comparing the
        // historical receipt and confirmation snapshot. Preserve priority.
        for row in rows(field(order,"recipients")){
            for item in rows(field(row,"items")){
                let valid=rows(field(state,"products")).iter().any(|p|
                    str_field(p,"id")==str_field(item,"productId") &&
                    !has_value(field(p,"deletedAt")) &&
                    field(p,"active").as_bool()!=Some(false) &&
                    array_has(field(p,"variants"),str_field(item,"variantId")));
                if !valid{return Err("등록된 독립 상품과 가격 기록을 확인하세요.".into());}
            }
        }
        let snapshot=field(order,"confirmationSnapshot");
        if field(snapshot,"raw")!=field(order,"raw")||
            !truthy(field(snapshot,"data"))||
            field(field(snapshot,"data"),"id")!=field(order,"id")||
            field(field(snapshot,"data"),"raw")!=field(order,"raw")||
            field(field(snapshot,"data"),"recipients")!=field(order,"recipients")||
            field(snapshot,"at")!=field(order,"confirmedAt")||
            field(field(order,"receipt"),"original")!=field(order,"raw")||
            field(field(order,"receipt"),"at")!=field(order,"confirmedAt")||
            field(snapshot,"recipients")!=field(order,"recipients")||
            field(field(snapshot,"client"),"id")!=field(order,"clientId"){
            return Err("정식 주문의 최초 확정 정보를 확인하세요.".into());
        }
        let ships=rows(field(field(state,"workflow"),"shipments")).iter()
            .filter(|ship|str_field(ship,"orderId")==str_field(order,"id")&&
                !flag(ship,"legacyArchived")).collect::<Vec<_>>();
        let present=ships.iter().map(|ship|str_field(ship,"recipientId"))
            .collect::<HashSet<_>>();
        if rows(field(order,"recipients")).iter()
            .any(|row|!present.contains(str_field(row,"id"))){
            return Err("일부 배송 건을 제외하고 정식 주문으로 저장할 수 없습니다.".into());
        }
        let valid_event=rows(field(field(state,"workflow"),"events")).iter().any(|e|
            str_field(e,"action")=="주문 확인 완료"&&
            rows(field(e,"shipmentIds")).iter().any(|id|
                ships.iter().any(|ship|id.as_str()==Some(str_field(ship,"id")))));
        if ships.is_empty()||
            ships.iter().any(|ship|field(ship,"step").as_f64().is_some_and(|n|n<1.0))||
            !valid_event{
            return Err("주문 확인 완료 기록이 없습니다.".into());
        }
        let mut seen=HashSet::new();
        for ship in &ships {
            let gid=str_field(ship,"groupId");
            if !seen.insert(gid.to_string()){continue;}
            let problems=super::workflow_issues::stage_issues(state,gid,1)?;
            if !problems.is_empty(){return Err(problems.join("\n"));}
        }
    }
    Ok(())
}

#[cfg(test)]
mod immutable_history_index_tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn immutable_history_index_matches_original_first_match_rule(){
        let old=json!([{"id":"a","value":1},{"id":"b","value":2}]);
        for next in [
            json!([{"id":"a","value":1},{"id":"b","value":2},{"id":"c"}]),
            json!([{"id":"b","value":2},{"id":"a","value":1}]),
            json!([{"id":"a","value":9},{"id":"a","value":1},{"id":"b","value":2}]),
            json!([{"id":"a","value":1}]),
            Value::Null
        ]{
            let expected=rows(&old).iter().all(|row|find_by_id(&next,str_field(row,"id"))==Some(row));
            assert_eq!(unchanged_records(&old,&next),expected);
        }
        let duplicate=json!([{"id":"a","value":1},{"id":"a","value":2}]);
        assert!(!unchanged_records(&duplicate,&duplicate));
    }
}

#[cfg(test)]
#[test]
fn factory_scope_ignores_unchanged_unrelated_faults_but_protects_reply_targets(){
    let old=serde_json::json!({
        "orders":[{"id":"o","clientId":"c","recipients":[{"id":"r","actualFreight":null}]}],
        "clients":[{"id":"c"}],"factoryReplies":[],
        "workflow":{
            "sources":[{"id":"src","orderId":"o"}],
            "groups":[{"id":"g","sourceId":"src","dealerId":"c"}],
            "shipments":[
                {"id":"s","orderId":"o","recipientId":"r","sourceId":"src","groupId":"g",
                 "step":4,"charges":{"FACTORY":3000},"tracking":{},"actualFreight":{}},
                {"id":"unrelated","orderId":"missing","step":99}
            ],
            "payments":[{"id":"old-invalid","amount":-1}],"events":[]
        }
    });
    let mut new=old.clone();
    new["workflow"]["shipments"][0]["tracking"]["FACTORY"]=serde_json::json!("001234567890");
    new["workflow"]["shipments"][0]["actualFreight"]["FACTORY"]=serde_json::json!(4000);
    new["factoryReplies"]=serde_json::json!([{"id":"reply","status":"APPLIED","rows":[
        {"row":2,"applied":true,"shipmentId":"s","orderId":"o","tracking":"001234567890","actual":4000}
    ]}]);
    new["workflow"]["events"]=serde_json::json!([
        {"id":"fee","action":LEGACY_FREIGHT_INPUT_EVENT,"shipmentIds":["s"],"before":null,
         "after":{"origin":"FACTORY","amount":4000}},
        {"id":"reply-event","action":"공장 회신 반영","shipmentIds":["s"],
         "before":{"tracking":null,"actual":null},
         "after":{"tracking":"001234567890","actual":4000,"fileId":"reply"}}
    ]);
    assert!(validate(&new,Some(&old)).is_err());
    assert!(validate_factory_transition(&new,&old).is_ok());
    let mut invalid=new.clone();
    invalid["workflow"]["payments"][0]["amount"]=serde_json::json!(0);
    assert!(validate_factory_transition(&invalid,&old).is_err());
    let mut invalid=new.clone();
    invalid["workflow"]["shipments"][0]["charges"]["FACTORY"]=serde_json::json!(4000);
    assert!(validate_factory_transition(&invalid,&old).is_err());
    let mut invalid=new.clone();
    invalid["workflow"]["events"].as_array_mut().unwrap().remove(0);
    assert!(validate_factory_transition(&invalid,&old).is_err());
    let mut invalid=new.clone();
    invalid["workflow"]["shipments"][1]["step"]=serde_json::json!(4);
    assert!(validate_factory_transition(&invalid,&old).is_err());
    let mut duplicate_old=old.clone();
    duplicate_old["workflow"]["shipments"][1]["tracking"]=serde_json::json!({"FACTORY":"001234567890"});
    let mut duplicate_new=new.clone();
    duplicate_new["workflow"]["shipments"][1]=duplicate_old["workflow"]["shipments"][1].clone();
    assert!(validate_factory_transition(&duplicate_new,&duplicate_old).is_err());
    let mut missing_link=old.clone();
    missing_link["orders"]=serde_json::json!([]);
    let mut missing_next=new.clone();
    missing_next["orders"]=serde_json::json!([]);
    assert!(validate_factory_transition(&missing_next,&missing_link).is_err());
}
