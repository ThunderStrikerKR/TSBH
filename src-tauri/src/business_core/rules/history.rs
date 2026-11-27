//! Read-only order history: exactly the previous src/app/order-history.ts rules.
 //! Both history UI and daily settlement consume this one Rust projection.
 //! Never reprice confirmed orders against today's catalog.
use super::{field,find_by_id,flag,has_value,pricing,rows,str_field,workflow};
use chrono::{DateTime,FixedOffset,NaiveDate};
use serde_json::{json,Value};
use std::collections::HashSet;

fn client_for<'a>(state:&'a Value,order:&'a Value)->Option<Value>{
    let saved=field(order,"clientSnapshot");
    if str_field(order,"orderType")=="RETAIL" {
        if has_value(saved){return Some(saved.clone())}
        return Some(super::validation::company_party(state))
    }
    if (has_value(field(order,"confirmedAt"))||flag(order,"pricesLocked")) && has_value(saved){
        return Some(saved.clone())
    }
    find_by_id(field(state,"clients"),str_field(order,"clientId")).cloned()
        .or_else(||has_value(saved).then(||saved.clone()))
}
fn active_shipments<'a>(state:&'a Value,id:&str)->Vec<&'a Value>{
    let w=field(state,"workflow");
    rows(field(w,"shipments")).iter().filter(|ship|{
        str_field(ship,"orderId")==id && !flag(ship,"legacyArchived") &&
        !find_by_id(field(w,"groups"),str_field(ship,"groupId"))
            .is_some_and(|g|has_value(field(g,"deletedAt")))
    }).collect()
}
fn only_deleted_groups(state:&Value,order_id:&str)->bool{
    let w=field(state,"workflow");
    let groups=rows(field(w,"groups")).iter().filter(|group|{
        rows(field(w,"shipments")).iter().any(|s|
            str_field(s,"orderId")==order_id && str_field(s,"groupId")==str_field(group,"id"))
    }).collect::<Vec<_>>();
    !groups.is_empty() && groups.iter().all(|g|has_value(field(g,"deletedAt")))
}
fn date_seoul(raw:&str)->String{
    if raw.is_empty(){return String::new()}
    if let Ok(t)=DateTime::parse_from_rfc3339(raw) {
        let seoul=FixedOffset::east_opt(9*3600).unwrap();
        return t.with_timezone(&seoul).format("%Y-%m-%d").to_string()
    }
    // JS Date.parse treats ISO dates without a timezone as local time.
    if let Ok(d)=NaiveDate::parse_from_str(raw,"%Y-%m-%d"){
        return d.format("%Y-%m-%d").to_string()
    }
    if let Some(date)=raw.get(..10){
        if NaiveDate::parse_from_str(date,"%Y-%m-%d").is_ok(){
            return date.to_owned()
        }
    }
    String::new()
}
pub(super) fn status(state:&Value,order_id:&str)->String{
    let order=find_by_id(field(state,"orders"),order_id);
    let ships=active_shipments(state,order_id);
    if ships.is_empty(){
        return if order.is_some_and(|o|str_field(o,"status")=="EXPORTED"){
            "엑셀 추출 완료"
        }else{"처리 중"}.into()
    }
    if ships.iter().any(|s|!flag(s,"transferred")){return "처리 중".into()}
    let ids=ships.iter().map(|s|str_field(s,"id")).collect::<HashSet<_>>();
    let entries=rows(field(field(state,"workflow"),"queue")).iter()
        .filter(|q|ids.contains(str_field(q,"shipmentId")) &&
            str_field(q,"status")!="CANCELLED").collect::<Vec<_>>();
    if ships.iter().any(|s| !entries.iter().any(|e|
        str_field(e,"shipmentId")==str_field(s,"id"))) ||
        entries.iter().any(|e|str_field(e,"status")=="QUEUED"){
        return "엑셀 추출 대기".into()
    }
    if !entries.iter().all(|e|
        ["EXPORTED","STORE_SENT"].contains(&str_field(e,"status"))){
        return "엑셀 추출 대기".into()
    }
    let factory=entries.iter().filter(|e|str_field(e,"origin")=="FACTORY").collect::<Vec<_>>();
    let replies=rows(field(state,"factoryReplies")).iter()
        .flat_map(|reply|rows(field(reply,"rows"))).collect::<Vec<_>>();
    let complete=!factory.is_empty() && factory.iter().all(|entry|{
        let id=str_field(entry,"shipmentId");
        let tracking=ships.iter().find(|s|str_field(s,"id")==id)
            .map(|s|str_field(field(s,"tracking"),"FACTORY").trim())
            .unwrap_or("");
        !tracking.is_empty() && replies.iter().any(|row|flag(row,"applied")
            && str_field(row,"shipmentId")==id && str_field(row,"tracking")==tracking)
    });
    if complete{"공장 회신 완료"}else{"엑셀 추출 완료"}.into()
}
fn component(parts:&[Value],key:&str)->Value{
    if parts.is_empty(){return Value::Null}
    let mut sum=0_i64;
    for part in parts{
        let Some(n)=field(part,key).as_i64() else{return Value::Null};
        let Some(next)=sum.checked_add(n) else{return Value::Null};
        sum=next;
    }
    json!(sum)
}
/// Original single-day history contract used by settlement and legacy TS.
pub(super) fn list(state:&Value,query:&str,date:&str)->Result<Value,String>{
    list_filtered(state,query,|order|{
        if date.is_empty(){return true}
        let day=if has_value(field(order,"confirmedAt")){
            str_field(order,"confirmedAt")
        }else{str_field(order,"createdAt")};
        date_seoul(day)==date
    })
}
fn list_filtered(
    state:&Value,query:&str,mut matches_date:impl FnMut(&Value)->bool
)->Result<Value,String>{
    let query_lower=query.to_lowercase();
    let mut result=Vec::new();
    for order in rows(field(state,"orders")){
        let id=str_field(order,"id");
        if has_value(field(order,"deletedAt"))||only_deleted_groups(state,id)||
            (field(order,"receptionVersion").as_i64()==Some(1) &&
                !has_value(field(order,"confirmedAt"))){continue}
        if !matches_date(order){continue}
        let client=client_for(state,order);
        let client_name=client.as_ref().map(|v|str_field(v,"name")).unwrap_or("");
        let searchable=format!("{client_name} {}",str_field(order,"raw")).to_lowercase();
        if !searchable.contains(&query_lower){continue}
        let ships=active_shipments(state,id);
        // TS shipmentContext requires both the exact recipient and a client,
        // except when the frozen bill exists (which TS obtains before contexts).
        let mut parts=Vec::new();
        let mut recipients=Vec::new();
        if !ships.is_empty(){
            // Original shipmentContext reports one stable linkage error before
            // pricing when a shipment has no real client or recipient.
            if client.is_none() || ships.iter().any(|ship|
                !rows(field(order,"recipients")).iter().any(|r|
                    str_field(r,"id")==str_field(ship,"recipientId"))) {
                return Err("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.".into());
            }
            for ship in &ships{
                let shipment_id=str_field(ship,"id");
                parts.push(workflow::shipment_totals(state,shipment_id)?);
            }
            for ship in &ships{
                let recipient=rows(field(order,"recipients")).iter()
                    .find(|r|str_field(r,"id")==str_field(ship,"recipientId"));
                if client.is_none()||recipient.is_none(){
                    return Err("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.".into())
                }
                recipients.push(recipient.unwrap().clone());
            }
        }else{
            recipients=rows(field(order,"recipients")).to_vec();
            if let Some(c)=client.as_ref(){parts.push(pricing::totals(order,c)?)}
        }
        let billing=json!({
            "subtotal":component(&parts,"subtotal"),
            "shipping":component(&parts,"shipping"),
            "vat":component(&parts,"vat"),
            "total":component(&parts,"total")
        });
        let mut row=json!({
            "order":order,"client":client,"recipients":recipients,
            "total":if ships.is_empty(){
                field(&billing,"total").clone()
            }else{component(&parts,"total")},
            "billing":billing,
            "shipmentIds":ships.iter().map(|s|str_field(s,"id")).collect::<Vec<_>>(),
            "deleted":only_deleted_groups(state,id),
            "status":status(state,id)
        });
        if client.is_none() {
            // JSON.stringify(TS history) omits an undefined client key.
            row.as_object_mut().unwrap().remove("client");
        }
        result.push(row);
    }
    Ok(json!(result))
}


/// Read-only UI projection in addition to the accounting/history rows.
/// The item-reference and bank-evidence decisions previously lived inside
/// OrderHistory.tsx; canonical list_order_history stays byte-for-byte stable
/// for settlement and the original TypeScript parity oracle.
pub(super) fn list_view(
    state:&Value, query:&str, date:&str, tracking_order:&Value,
)->Result<Value,String> {
    enrich_view(state,list(state,query,date)?,tracking_order)
}

/// Extended UI-only date filtering. The original list/order history contract
/// and the daily settlement date calculations remain unchanged.
///
/// PAYMENT means a real bank payment with a positive amount ALLOCATED to an
/// active shipment of the order, matched by paidAt in Korea/Seoul. A dealer's
/// unallocated payment or an approval/paid flag is not enough evidence.
pub(super) fn list_view_range(
    state:&Value,query:&str,from:&str,to:&str,basis:&str,
    tracking_order:&Value
)->Result<Value,String>{
    for day in [from,to]{
        if !day.is_empty() && NaiveDate::parse_from_str(day,"%Y-%m-%d")
            .map_or(true,|parsed|parsed.format("%Y-%m-%d").to_string()!=day)
        {
            return Err("주문 내역 검색 날짜는 YYYY-MM-DD 형식이어야 합니다.".into());
        }
    }
    if !from.is_empty() && !to.is_empty() && from>to{
        return Err("검색 시작일이 종료일보다 늦습니다.".into());
    }
    let in_range=|day:&str|{
        !day.is_empty() && (from.is_empty()||day>=from)
            && (to.is_empty()||day<=to)
    };
    let result=match basis {
        "RECEPTION"=>{
            list_filtered(state,query,|order|{
                if from.is_empty() && to.is_empty(){return true}
                let date=if has_value(field(order,"confirmedAt")){
                    str_field(order,"confirmedAt")
                }else{str_field(order,"createdAt")};
                in_range(&date_seoul(date))
            })?
        }
        "PAYMENT"=>{
            // Join payment.allocations[].shipmentId to workflow.shipments[]
            // and the shipment's orderId; never guess from dealer name.
            let paid_shipment_ids=rows(field(field(state,"workflow"),"payments"))
                .iter().filter(|payment|{
                    in_range(&date_seoul(str_field(payment,"paidAt")))
                }).flat_map(|payment|rows(field(payment,"allocations")))
                .filter(|allocation|
                    field(allocation,"amount").as_i64().is_some_and(|n|n>0))
                .map(|allocation|str_field(allocation,"shipmentId"))
                .filter(|id|!id.is_empty())
                .collect::<HashSet<_>>();
            list_filtered(state,query,|order|{
                active_shipments(state,str_field(order,"id")).iter().any(|ship|
                    paid_shipment_ids.contains(str_field(ship,"id")))
            })?
        }
        _=>return Err("지원하지 않는 주문 내역 날짜 조회 기준입니다.".into())
    };
    enrich_view(state,result,tracking_order)
}

/// Employee confirmation timestamps are NOT bank payment dates.
///
/// Only a confirmation attached to one of this order's current shipments is
/// evidence. Exclude shipments whose confirmation was explicitly invalidated
/// by an "입금 승인 재확인 필요" workflow event; never infer a confirmation
/// from the legacy order.paid flag or payments[].paidAt.
fn employee_payment_confirmation(state:&Value,shipment_ids:&HashSet<&str>)->Value{
    if shipment_ids.is_empty(){return Value::Null}
    let workflow=field(state,"workflow");
    let events=rows(field(workflow,"events"));
    let mut dates=Vec::new();
    let mut covered=HashSet::new();
    let mut actors=Vec::new();
    for confirm in rows(field(workflow,"confirmations")){
        let confirmed_at=str_field(confirm,"confirmedAt");
        let date=date_seoul(confirmed_at);
        if date.is_empty(){continue}
        let confirm_id=field(confirm,"id");
        let mut matched=false;
        for id in rows(field(confirm,"shipmentIds")).iter()
            .filter_map(Value::as_str)
            .filter(|id|shipment_ids.contains(*id))
        {
            let invalidated=events.iter().any(|event|
                str_field(event,"action")=="입금 승인 재확인 필요" &&
                field(event,"before")==confirm_id &&
                rows(field(event,"shipmentIds")).iter().any(|event_id|
                    event_id.as_str()==Some(id)));
            if invalidated{continue}
            matched=true;
            covered.insert(id);
        }
        if matched {
            dates.push((confirmed_at,date));
            let actor=str_field(confirm,"actor");
            if !actor.is_empty(){actors.push(actor.to_owned());}
        }
    }
    if dates.is_empty(){return Value::Null}
    // RFC3339 strings generally compare chronologically when uniform, but
    // offset differences can reorder them. Sort by actual parsed instant
    // where possible, otherwise the converted local calendar date.
    dates.sort_by(|a,b|{
        let instant=|value:&str|DateTime::parse_from_rfc3339(value).ok()
            .map(|date|date.timestamp_millis());
        a.1.cmp(&b.1).then_with(||instant(a.0).cmp(&instant(b.0)))
    });
    let (confirmed_at,latest)=dates.last().expect("nonempty confirmation dates");
    let datetime=DateTime::parse_from_rfc3339(confirmed_at).ok()
        .map(|stamp|stamp.with_timezone(&FixedOffset::east_opt(9*3600).unwrap())
            .format("%Y-%m-%d %H:%M").to_string());
    actors.sort();actors.dedup();
    json!({
        "date":latest,
        "confirmedAt":confirmed_at,
        "dateTime":datetime,
        "partial":covered.len()<shipment_ids.len(),
        "approvedShipments":covered.len(),
        "totalShipments":shipment_ids.len(),
        "actors":actors
    })
}

fn enrich_view(
    state:&Value,mut result:Value,tracking_order:&Value
)->Result<Value,String>{
    let Some(history_rows)=result.as_array_mut() else {
        return Err("주문 이력 조회 결과 형식을 확인하세요.".into())
    };
    for row in history_rows {
        let order=field(row,"order");
        let order_id=str_field(order,"id").to_owned();
        let shipment_ids=rows(field(row,"shipmentIds")).iter()
            .filter_map(Value::as_str).collect::<HashSet<_>>();
        let evidence_missing=flag(order,"paid") &&
            !rows(field(field(state,"workflow"),"payments")).iter().any(|p|
                rows(field(p,"allocations")).iter()
                    .any(|a|shipment_ids.contains(str_field(a,"shipmentId"))));
        let mut links=Vec::new();
        for recipient in rows(field(row,"recipients")) {
            for item in rows(field(recipient,"items")) {
                links.push(json!({
                    "recipientId":str_field(recipient,"id"),
                    "itemId":str_field(item,"id"),
                    "link":historical_link(state,item)
                }));
            }
        }
        let text=tracking_message(state,&order_id,tracking_order);
        let approval=employee_payment_confirmation(state,&shipment_ids);
        row["display"]=json!({
            "trackingMessage":text,
            "paymentEvidenceMissing":evidence_missing,
            "employeePaymentConfirmation":approval,
            "references":links,
        });
    }
    Ok(result)
}
/// The historical product reference is checked by BOTH old parent and old
/// price-record IDs. Never match by name/color or replace the frozen price.
fn historical_link(state:&Value,item:&Value)->Value {
    let product_id=str_field(item,"productId");
    let variant_id=str_field(item,"variantId");
    if product_id.is_empty()||variant_id.is_empty(){return Value::Null}
    let products=field(state,"products");
    if let Some(found)=find_by_id(products,product_id) {
        let variants=rows(field(found,"variants"));
        if variants.len()==1 && str_field(&variants[0],"id")==variant_id {
            return json!({"name":str_field(found,"name"),"via":"CURRENT"})
        }
        return Value::Null
    }
    let key=format!("{product_id}|{variant_id}");
    let mapped_id=str_field(field(state,"productMappings"),&key);
    if mapped_id.is_empty(){return Value::Null}
    if let Some(found)=find_by_id(products,mapped_id) {
        let variants=rows(field(found,"variants"));
        if str_field(found,"sourceProductId")==product_id &&
            variants.len()==1 && str_field(&variants[0],"id")==variant_id {
            return json!({"name":str_field(found,"name"),"via":"HISTORICAL"})
        }
    }
    Value::Null
}
fn tracking_message(state:&Value,order_id:&str,tracking_order:&Value)->String {
    let mut lines=Vec::<String>::new();
    let order=find_by_id(field(state,"orders"),order_id);
    for ship in rows(field(field(state,"workflow"),"shipments")) {
        if str_field(ship,"orderId")!=order_id{continue}
        let recipient=order.and_then(|o|rows(field(o,"recipients")).iter()
            .find(|r|str_field(r,"id")==str_field(ship,"recipientId")));
        let recipient_name=recipient
            .map(|r|str_field(r,"name")).unwrap_or("수취인");
        let tracking=field(ship,"tracking");
        let requested=rows(tracking_order).iter()
            .find(|entry|str_field(entry,"shipmentId")==str_field(ship,"id"))
            .and_then(|entry|field(entry,"keys").as_array());
        // JavaScript Object.values historically preserves insertion order;
        // serde_json::Value's default map is lexicographically sorted.
        // The UI sends *only key order* as transport metadata so Rust can
        // reproduce original copied text without running TS business logic.
        if let Some(keys)=requested {
            for key in keys.iter().filter_map(Value::as_str) {
                let val=str_field(tracking,key);
                if !val.is_empty(){lines.push(format!("{recipient_name} {val}"))}
            }
        } else if let Some(map)=tracking.as_object(){
            for value in map.values().filter_map(Value::as_str) {
                if !value.is_empty(){lines.push(format!("{recipient_name} {value}"))}
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn legacy_export_and_missing_shipments(){
        let state=json!({"orders":[
            {"id":"legacy","raw":"legacy","status":"EXPORTED","createdAt":"2026-11-13",
                "recipients":[],"clientId":"c1"}
        ],"clients":[],"workflow":{"shipments":[],"queue":[],"groups":[]}});
        let rows=list(&state,"","2026-11-13").unwrap();
        assert_eq!(rows[0]["status"],"엑셀 추출 완료");
        assert_eq!(rows[0]["total"],Value::Null);
    }
    #[test] fn factory_reply_completes_only_with_applied_matching_tracking(){
        let state=json!({"orders":[{"id":"o1","clientId":"c1","raw":"709","createdAt":"2026-11-13",
            "recipients":[{"id":"r1","items":[] }]}],
            "clients":[{"id":"c1","tier":"wholesale"}],
            "workflow":{"shipments":[{"id":"s1","orderId":"o1","recipientId":"r1",
                "groupId":"g1","transferred":true,"tracking":{"FACTORY":"000123456789"}}],
                "groups":[{"id":"g1"}],
                "queue":[{"shipmentId":"s1","origin":"FACTORY","status":"EXPORTED",
                    "snapshot":{"subtotal":0,"shipping":0,"vat":0,"total":0}}]},
            "factoryReplies":[{"rows":[{"shipmentId":"s1","tracking":"000123456789","applied":true}]}]});
        let rows=list(&state,"","").unwrap();
        assert_eq!(rows[0]["status"],"공장 회신 완료");
        assert_eq!(rows[0]["billing"]["total"],0);
    }

    #[test]
    fn read_only_view_preserves_copy_order_and_historical_product_link() {
        let state=json!({
            "orders":[{"id":"o1","clientId":"c1","raw":"709",
                "createdAt":"2026-11-13","paid":true,
                "recipients":[{"id":"r1","name":"박수취","items":[{
                    "id":"i1","productId":"old","variantId":"v1"
                }]}]}],
            "clients":[{"id":"c1","name":"기존 거래처","tier":"wholesale"}],
            "products":[{"id":"new","sourceProductId":"old",
                "name":"현재 연결 상품","variants":[{"id":"v1"}]}],
            "productMappings":{"old|v1":"new"},
            "workflow":{"shipments":[{"id":"s1","orderId":"o1",
                "recipientId":"r1","groupId":"g1","transferred":true,
                "tracking":{"FACTORY":"11111111","STORE":"22222222"}}],
                "groups":[{"id":"g1"}],
                "queue":[{"shipmentId":"s1","origin":"FACTORY",
                    "status":"EXPORTED","snapshot":{
                        "subtotal":0,"shipping":0,"vat":0,"total":0}}],
                "payments":[]}
        });
        let before=state.clone();
        let keys=json!([{"shipmentId":"s1","keys":["STORE","FACTORY"]}]);
        let view=list_view(&state,"","",&keys).unwrap();
        assert_eq!(view[0]["display"]["trackingMessage"],
            "박수취 22222222\n박수취 11111111");
        assert_eq!(view[0]["display"]["paymentEvidenceMissing"],true);
        assert_eq!(view[0]["display"]["references"][0]["link"]["via"],"HISTORICAL");
        assert_eq!(view[0]["display"]["references"][0]["link"]["name"],"현재 연결 상품");
        assert_eq!(state,before,"history view must never mutate saved state");
    }

    #[test]
    fn historical_direct_product_mismatch_does_not_rebind_and_bank_evidence_hides_warning() {
        let mut state=json!({
            "orders":[{"id":"o1","clientId":"c1","raw":"709",
                "createdAt":"2026-11-13","paid":true,
                "recipients":[{"id":"r1","name":"박수취","items":[{
                    "id":"i1","productId":"old","variantId":"v1"
                }]}]}],
            "clients":[{"id":"c1","tier":"wholesale"}],
            "products":[
                {"id":"old","name":"현재 직접 연결 실패","variants":[{"id":"other"}]},
                {"id":"new","sourceProductId":"old","name":"연계 차단",
                    "variants":[{"id":"v1"}]}
            ],
            "productMappings":{"old|v1":"new"},
            "workflow":{"shipments":[],"groups":[],"queue":[],
                "payments":[{"id":"p1","allocations":[{"shipmentId":"s1","amount":1}]}]}
        });
        assert_eq!(historical_link(&state,&state["orders"][0]["recipients"][0]["items"][0]),
            Value::Null);
        state["workflow"]["shipments"]=json!([{"id":"s1","orderId":"o1",
            "recipientId":"r1","groupId":"g1","transferred":true,
            "tracking":{} }]);
        state["workflow"]["groups"]=json!([{"id":"g1"}]);
        state["workflow"]["queue"]=json!([{"shipmentId":"s1","origin":"FACTORY",
            "status":"EXPORTED","snapshot":{
                "subtotal":0,"shipping":0,"vat":0,"total":0}}]);
        let view=list_view(&state,"","",&Value::Null).unwrap();
        assert_eq!(view[0]["display"]["references"][0]["link"],Value::Null);
        assert_eq!(view[0]["display"]["paymentEvidenceMissing"],false);
    }
    #[test]
    fn employee_payment_approval_uses_confirmed_at_not_paid_at_and_respects_rechecks(){
        let mut state=json!({"workflow":{
            "shipments":[{"id":"s1","orderId":"o1"},{"id":"s2","orderId":"o1"}],
            "confirmations":[
                {"id":"c1","confirmedAt":"2026-11-19T16:30:00Z",
                 "actor":"작업자 A","shipmentIds":["s1","s2"]},
                {"id":"c2","confirmedAt":"2026-11-20T18:00:00Z",
                 "actor":"작업자 B","shipmentIds":["s2"]}
            ],
            "events":[{"action":"입금 승인 재확인 필요",
                "before":"c1","shipmentIds":["s2"]}],
            "payments":[{"paidAt":"2026-11-15T02:00:00Z",
                "allocations":[{"shipmentId":"s1","amount":500}]}]
        }});
        let ids=HashSet::from(["s1","s2"]);
        let approval=employee_payment_confirmation(&state,&ids);
        assert_eq!(approval["date"],"2026-11-21");
        assert_eq!(approval["confirmedAt"],"2026-11-20T18:00:00Z");
        assert_eq!(approval["dateTime"],"2026-11-21 03:00");
        assert_eq!(approval["partial"],false);
        assert_eq!(approval["approvedShipments"],2);
        assert_eq!(approval["actors"],json!(["작업자 A","작업자 B"]));

        // Reconfirm invalidation affects only the targeted shipment.
        state["workflow"]["confirmations"].as_array_mut().unwrap().pop();
        let partial=employee_payment_confirmation(&state,&ids);
        assert_eq!(partial["date"],"2026-11-20");
        assert_eq!(partial["dateTime"],"2026-11-20 01:30");
        assert_eq!(partial["partial"],true);
        assert_eq!(partial["approvedShipments"],1);

        // A recorded bank transfer by itself never populates staff approval.
        state["workflow"]["confirmations"]=json!([]);
        assert!(employee_payment_confirmation(&state,&ids).is_null());
        assert!(employee_payment_confirmation(&state,&HashSet::new()).is_null());
    }

    #[test]
    fn date_range_reception_and_real_payment_allocations_are_distinct() {
        let state=json!({
            "orders":[
                {"id":"o1","clientId":"c1","raw":"709","createdAt":"2026-11-15",
                 "recipients":[{"id":"r1","items":[]}]},
                {"id":"o2","clientId":"c1","raw":"709","createdAt":"2026-11-17",
                 "recipients":[{"id":"r2","items":[]}]},
                {"id":"o3","clientId":"c1","raw":"709","createdAt":"2026-11-18",
                 "recipients":[{"id":"r3","items":[]}]}
            ],
            "clients":[{"id":"c1","name":"거래처","tier":"wholesale"}],
            "workflow":{
                "groups":[{"id":"g1"}],
                "shipments":[
                    {"id":"s1","orderId":"o1","recipientId":"r1","groupId":"g1","transferred":true},
                    {"id":"s2","orderId":"o2","recipientId":"r2","groupId":"g1","transferred":true},
                    {"id":"s3","orderId":"o3","recipientId":"r3","groupId":"g1","transferred":true}
                ],
                "queue":[
                    {"shipmentId":"s1","status":"EXPORTED","origin":"STORE",
                     "snapshot":{"subtotal":0,"shipping":0,"vat":0,"total":0}},
                    {"shipmentId":"s2","status":"EXPORTED","origin":"STORE",
                     "snapshot":{"subtotal":0,"shipping":0,"vat":0,"total":0}},
                    {"shipmentId":"s3","status":"EXPORTED","origin":"STORE",
                     "snapshot":{"subtotal":0,"shipping":0,"vat":0,"total":0}}
                ],
                "payments":[
                    {"id":"p1","paidAt":"2026-11-19T16:00:00Z",
                     "allocations":[{"shipmentId":"s1","amount":1500}]},
                    {"id":"p2","paidAt":"2026-11-21T03:00:00Z",
                     "allocations":[{"shipmentId":"s2","amount":500}]},
                    {"id":"p3","paidAt":"2026-11-20T01:00:00Z","allocations":[]},
                    {"id":"p4","paidAt":"2026-11-20T05:00:00Z",
                     "allocations":[{"shipmentId":"s3","amount":0}]}
                ]
            }
        });
        let ids=|items:&Value|items.as_array().unwrap().iter()
            .map(|row|str_field(field(row,"order"),"id").to_owned())
            .collect::<Vec<_>>();
        let before=state.clone();
        assert_eq!(ids(&list_view_range(
            &state,"","2026-11-20","2026-11-20","PAYMENT",&Value::Null
        ).unwrap()),vec!["o1"]);
        assert_eq!(ids(&list_view_range(
            &state,"","2026-11-20","2026-11-21","PAYMENT",&Value::Null
        ).unwrap()),vec!["o1","o2"]);
        assert_eq!(ids(&list_view_range(
            &state,"","2026-11-17","2026-11-18","RECEPTION",&Value::Null
        ).unwrap()),vec!["o2","o3"]);
        assert_eq!(ids(&list_view_range(
            &state,"","","2026-11-20","PAYMENT",&Value::Null
        ).unwrap()),vec!["o1"]);
        assert_eq!(ids(&list_view_range(
            &state,"","2026-11-21","","PAYMENT",&Value::Null
        ).unwrap()),vec!["o2"]);
        assert_eq!(ids(&list_view_range(
            &state,"","","","RECEPTION",&Value::Null
        ).unwrap()),vec!["o1","o2","o3"]);
        assert_eq!(ids(&list_view_range(
            &state,"","","","PAYMENT",&Value::Null
        ).unwrap()),vec!["o1","o2"]);
        assert!(list_view_range(
            &state,"","2026-11-22","2026-11-20","PAYMENT",&Value::Null
        ).is_err());
        assert!(list_view_range(
            &state,"","2026-02-30","","RECEPTION",&Value::Null
        ).is_err());
        assert_eq!(state,before,"searching cannot mutate business data");
        // The original date-only entrypoint remains unchanged for settlement.
        assert_eq!(ids(&list(&state,"","2026-11-17").unwrap()),vec!["o2"]);
    }

}
