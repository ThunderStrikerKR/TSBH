//! Seoul-day settlement from frozen order-history and append-only bank evidence.
//! Billed orders and received cash are deliberately separate accounting clocks.
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::{json, Value};
use std::collections::{HashMap,HashSet};

const MAX_SAFE: i64 = 9_007_199_254_740_991;
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn safe(value: &Value) -> Option<i64> {
    value.as_i64().filter(|n| *n >= 0 && *n <= MAX_SAFE)
}
fn add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b).filter(|n| *n <= MAX_SAFE)
        .ok_or_else(|| "집계 금액이 안전한 정수 범위를 초과했습니다.".into())
}
fn sum(items: impl IntoIterator<Item=i64>) -> Result<i64, String> {
    items.into_iter().try_fold(0_i64, add)
}
fn seoul_date(value: &str) -> String {
    if value.is_empty() { return String::new(); }
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        let seoul = FixedOffset::east_opt(9 * 3600).unwrap();
        return dt.with_timezone(&seoul).format("%Y-%m-%d").to_string();
    }
    // The runtime also stores legacy ISO dates without offsets.
    if let Ok(day) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return day.format("%Y-%m-%d").to_string();
    }
    if let Some(day) = value.get(..10) {
        if NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok() {
            return day.to_string();
        }
    }
    String::new()
}
fn allocations_for(
    state: &Value, shipment_ids: &HashSet<&str>, approval: bool
) -> Result<HashMap<String,i64>, String> {
    let workflow = &state["workflow"];
    let records = if approval { list(workflow, "confirmations") } else { list(workflow, "payments") };
    let mut seen = HashSet::new();
    let mut amounts=HashMap::new();
    for record in records {
        if !seen.insert(s(record, "id")) {
            return Err(if approval { "입금 승인 기록 ID가 중복되었습니다." }
                else { "입금 기록 ID가 중복되었습니다." }.into());
        }
        for allocation in list(record, "allocations") {
            let id = s(allocation, "shipmentId");
            if !shipment_ids.contains(id) { continue; }
            if approval && list(workflow, "events").iter().any(|event|
                s(event, "action") == "입금 승인 재확인 필요"
                    && event.get("before").and_then(Value::as_str) == Some(s(record, "id"))
                    && list(event, "shipmentIds").iter().any(|entry| entry.as_str() == Some(id))
            ) { continue; }
            let n = safe(&allocation["amount"]).ok_or(
                if approval { "입금 승인 기록에 잘못된 금액이 있습니다." } else { "입금 배분액이 올바르지 않습니다." }
            )?;
            let amount=amounts.entry(id.to_owned()).or_insert(0);
            *amount=add(*amount,n)?;
        }
    }
    Ok(amounts)
}
fn freight(state: &Value, row: &Value) -> Result<(i64,bool), String> {
    let shipment_ids = list(row, "shipmentIds");
    let recipients = list(&row["order"], "recipients");
    let mut complete = !shipment_ids.is_empty();
    let mut total = 0_i64;
    for id in shipment_ids {
        let id = id.as_str().unwrap_or("");
        let Some(ship) = list(&state["workflow"], "shipments")
            .iter().find(|ship| s(ship, "id") == id) else { complete = false; continue };
        let Some(recipient) = recipients.iter().find(|r| s(r,"id") == s(ship,"recipientId"))
        else { complete = false; continue };
        let origins: HashSet<&str> = list(recipient,"items").iter().map(|item|s(item,"origin")).collect();
        for origin in &origins {
            let actual = ship.get("actualFreight").and_then(|a|a.get(*origin));
            let cost = if actual.is_none() && origins.len() == 1 {
                recipient.get("actualFreight")
            } else { actual };
            let Some(n) = cost.and_then(safe) else { complete = false; continue };
            total = add(total,n)?;
        }
    }
    Ok((total,complete))
}
fn receipt_rows(state: &Value, date: &str) -> Result<Vec<Value>, String> {
    let mut seen = HashSet::new();
    let mut receipts = vec![];
    for payment in list(&state["workflow"],"payments") {
        if seoul_date(s(payment,"paidAt")) != date { continue; }
        if !seen.insert(s(payment,"id")) {
            return Err("입금 기록 ID가 중복되었습니다. 결산 전에 확인하세요.".into());
        }
        let amount = safe(&payment["amount"]).ok_or("입금액이 올바르지 않습니다.")?;
        let mut allocations = vec![];
        for item in list(payment,"allocations") {
            allocations.push(safe(&item["amount"]).ok_or("입금 배분액이 올바르지 않습니다.")?);
        }
        let allocated = sum(allocations)?;
        if allocated > amount { return Err("입금 배분액이 입금액보다 큽니다.".into()); }
        let dealer_id = s(payment,"dealerId");
        let dealer = list(state,"clients").iter()
            .find(|client|s(client,"id")==dealer_id)
            .map(|client|s(client,"name")).unwrap_or(dealer_id);
        let account = &payment["account"];
        let account_line = ["bank","holder","number"].iter()
            .map(|k|s(account,k)).filter(|v|!v.is_empty()).collect::<Vec<_>>().join(" / ");
        receipts.push(json!({
            "id":s(payment,"id"), "paidAt":s(payment,"paidAt"),
            "dealer":dealer, "account":account_line,
            "amount":amount,"allocated":allocated,"unallocated":amount-allocated,
            "memo":s(payment,"memo")
        }));
    }
    receipts.sort_by(|a,b|s(a,"paidAt").cmp(s(b,"paidAt")));
    Ok(receipts)
}
fn known(rows: &[Value], field: &str) -> Result<i64,String> {
    sum(rows.iter().filter_map(|row|row.get(field).and_then(Value::as_i64)))
}
fn count_unknown(rows: &[Value], field: &str) -> usize {
    rows.iter().filter(|row|row.get(field).is_none_or(Value::is_null)).count()
}
pub(super) fn prepare(state: &Value, date: &str, history_rows: &Value) -> Result<Value,String> {
    let parsed = NaiveDate::parse_from_str(date,"%Y-%m-%d")
        .map_err(|_|"결산 날짜를 확인하세요.")?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        return Err("결산 날짜를 확인하세요.".into());
    }
    let history = history_rows.as_array().ok_or("결산 주문 이력 목록이 없습니다.")?;
    let mut orders = vec![];
    for row in history {
        let order = &row["order"];
        let client = &row["client"];
        let snapshot_payment = order.get("clientSnapshot").and_then(|c|c.get("payment"))
            .and_then(Value::as_str);
        let payment = snapshot_payment.unwrap_or_else(||s(client,"payment"));
        let terms = if s(order,"orderType")=="RETAIL" { "소매" } else {
            match payment { "POSTPAID"=>"후불", "PREPAID"=>"선입금", _=>"미확인" }
        };
        let shipment_ids: HashSet<&str> = list(row,"shipmentIds").iter().filter_map(Value::as_str).collect();
        let paid_by_shipment=allocations_for(state,&shipment_ids,false)?;
        let approved_by_shipment=allocations_for(state,&shipment_ids,true)?;
        let paid=sum(paid_by_shipment.values().copied())?;
        let approved=sum(approved_by_shipment.values().copied())?;
        // Approval and bank evidence may describe the same payment.
        // Without an explicit link, use the greater amount per shipment,
        // never the sum of two potentially overlapping evidence sources.
        let recognized=sum(shipment_ids.iter().map(|id|
            (*paid_by_shipment.get(*id).unwrap_or(&0))
                .max(*approved_by_shipment.get(*id).unwrap_or(&0))))?;
        let (freight_value, complete) = freight(state,row)?;
        let bill = &row["billing"];
        let total = bill.get("total").and_then(safe);
        let outstanding = if shipment_ids.is_empty() || terms=="소매" {
            None
        } else if let Some(total) = total {
            Some(total.saturating_sub(recognized).max(0))
        } else { None };
        let client_name = if !s(client,"name").is_empty() { s(client,"name") }
            else if !s(&order["clientSnapshot"],"name").is_empty() { s(&order["clientSnapshot"],"name") }
            else { "거래처 미확인" };
        let legacy = s(order,"confirmedAt").is_empty();
        let date_of_order = if legacy { s(order,"createdAt") } else { s(order,"confirmedAt") };
        orders.push(json!({
            "orderId":s(order,"id"),"clientName":client_name,
            "date":seoul_date(date_of_order),"status":s(row,"status"),
            "paymentTerms":terms,
            "subtotal":bill.get("subtotal").cloned().unwrap_or(Value::Null),
            "shippingCharge":bill.get("shipping").cloned().unwrap_or(Value::Null),
            "vat":bill.get("vat").cloned().unwrap_or(Value::Null),
            "total":bill.get("total").cloned().unwrap_or(Value::Null),
            "paidAllocated":paid,"paymentApproved":approved,"outstanding":outstanding,
            "freightCost":if complete {Some(freight_value)} else {None},
            "freightComplete":complete,
            "cashEvidenceMissing":order.get("paid").and_then(Value::as_bool)==Some(true) && paid==0,
            "legacyDate":legacy,
            "shipmentIds":row["shipmentIds"]
        }));
    }
    let receipts = receipt_rows(state,date)?;
    let billed_unknown = count_unknown(&orders,"total");
    let mut warnings = vec![];
    if billed_unknown>0 { warnings.push(format!("청구 합계가 미확정된 주문 {}건은 확정 청구액 및 확정 주문 구성 금액에서 제외했습니다.",billed_unknown)); }
    if orders.iter().any(|r|r["legacyDate"]==true) {
        warnings.push("확정일이 없는 과거 주문은 최초 작성일을 기준으로 분류했습니다.".into());
    }
    if orders.iter().any(|r|r["cashEvidenceMissing"]==true) {
        warnings.push("기존 입금 완료 표시는 입금 기록이 아니므로 수입에 포함하지 않았습니다.".into());
    }
    if orders.iter().any(|r|r["paymentApproved"].as_i64().unwrap_or(0)>0) {
        warnings.push("입금 승인 기록은 계좌 입금액과 구별하며 현금 입금액에 포함하지 않았습니다.".into());
    }
    if orders.iter().any(|r|s(r,"paymentTerms")=="소매") {
        warnings.push("소매 주문은 이 결산에 결제 수단 기록이 없어 별도 입금으로 추정하지 않습니다.".into());
    }
    if orders.iter().any(|r|r["freightComplete"]==false) {
        warnings.push("출고 운임은 현재 기록된 값이며 해당 날짜의 현금 지출액을 의미하지 않습니다.".into());
    }
    if orders.iter().any(|r|r["paymentApproved"].as_i64().unwrap_or(0)>0 && r["paidAllocated"].as_i64().unwrap_or(0)>0){
        warnings.push("입금 승인과 계좌 입금이 함께 있는 경우 배송 건별 큰 금액만 미수 차감에 반영합니다. 별도 입금 여부는 입금 배분 기록으로 확인하세요.".into());
    }
    let billed_orders=orders.iter().filter(|r|r["total"].as_i64().is_some()).collect::<Vec<_>>();
    let billed_component=|key:&str|->Result<Value,String>{
        if billed_orders.iter().any(|row|row[key].as_i64().is_none()){return Ok(Value::Null);}
        Ok(json!(sum(billed_orders.iter().filter_map(|r|r[key].as_i64()))?))
    };
    let paid_cash = known(&receipts,"amount")?;
    let allocated_cash = known(&receipts,"allocated")?;
    let postpaid = sum(orders.iter().filter(|r|s(r,"paymentTerms")=="후불")
        .filter_map(|r|r["total"].as_i64()))?;
    Ok(json!({
        "date":date, "revision":state["revision"].as_i64().unwrap_or(0),
        "orderRows":orders,"paymentRows":receipts,
        "billedKnown":known(&orders,"total")?,
        "billedUnknown":billed_unknown,
        "billedProductKnown":billed_component("subtotal")?,
        "billedShippingKnown":billed_component("shippingCharge")?,
        "billedVatKnown":billed_component("vat")?,
        "productKnown":known(&orders,"subtotal")?,
        "shippingKnown":known(&orders,"shippingCharge")?,
        "vatKnown":known(&orders,"vat")?,
        "paidCash":paid_cash,"allocatedCash":allocated_cash,"unallocatedCash":paid_cash-allocated_cash,
        "postpaidKnown":postpaid,"outstandingKnown":known(&orders,"outstanding")?,
        "outstandingUnknown":count_unknown(&orders,"outstanding"),
        "actualFreightKnown":known(&orders,"freightCost")?,
        "freightUnknown":orders.iter().filter(|r|r["freightComplete"]==false).count(),
        "approvedPaymentKnown":known(&orders,"paymentApproved")?,
        "warnings":warnings
    }))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn settlement_with_shipments(shipments: Value, shipment_ids: Value) -> Value {
        let state = json!({"workflow": {"shipments": shipments}});
        let history = json!([{
            "shipmentIds": shipment_ids,
            "order": {"id": "order-a", "recipients": [
                {"id": "recipient-a", "items": [{"origin": "factory"}]},
                {"id": "recipient-b", "items": [{"origin": "factory"}]}
            ]},
            "billing": {"shipping": 5000, "total": 15000}
        }]);
        prepare(&state, "2026-11-25", &history).unwrap()
    }

    #[test]
    fn confirmed_components_share_scope_and_approval_does_not_double_count() {
        let state=json!({"workflow":{
            "payments":[{"id":"p","paidAt":"2026-11-25T01:00:00Z","amount":40,
                "allocations":[{"shipmentId":"s1","amount":40}]}],
            "confirmations":[{"id":"c","allocations":[{"shipmentId":"s1","amount":40},{"shipmentId":"s2","amount":10}]}]
        }});
        let history=json!([
            {"order":{"id":"o1"},"shipmentIds":["s1","s2"],
                "billing":{"subtotal":80,"shipping":10,"vat":10,"total":100}},
            {"order":{"id":"o2"},"shipmentIds":[],"billing":{"subtotal":500,"total":null}}
        ]);
        let report=prepare(&state,"2026-11-25",&history).unwrap();
        assert_eq!(report["productKnown"],580);
        assert_eq!(report["billedProductKnown"],80);
        assert_eq!(report["billedShippingKnown"],10);
        assert_eq!(report["billedVatKnown"],10);
        assert_eq!(report["billedKnown"],100);
        assert_eq!(report["outstandingKnown"],50);
        assert_eq!(report["paidCash"],40);
    }
    #[test]
    fn missing_shipment_does_not_confirm_partial_actual_freight() {
        let result = settlement_with_shipments(json!([
            {"id": "shipment-a", "recipientId": "recipient-a", "actualFreight": {"factory": 3000}}
        ]), json!(["shipment-a", "shipment-b"]));
        assert_eq!(result["orderRows"][0]["freightComplete"], json!(false));
        assert!(result["orderRows"][0]["freightCost"].is_null());
        assert_eq!(result["freightUnknown"], json!(1));
        assert_eq!(result["actualFreightKnown"], json!(0));
        assert_eq!(result["shippingKnown"], json!(5000));
    }

    #[test]
    fn all_missing_shipments_do_not_confirm_zero_actual_freight() {
        let result = settlement_with_shipments(json!([]), json!(["shipment-a"]));
        assert_eq!(result["orderRows"][0]["freightComplete"], json!(false));
        assert!(result["orderRows"][0]["freightCost"].is_null());
        assert_eq!(result["freightUnknown"], json!(1));
    }

    #[test]
    fn complete_actual_freight_preserves_independent_shipping_charge() {
        let result = settlement_with_shipments(json!([
            {"id": "shipment-a", "recipientId": "recipient-a", "actualFreight": {"factory": 3000}},
            {"id": "shipment-b", "recipientId": "recipient-b", "actualFreight": {"factory": 4000}}
        ]), json!(["shipment-a", "shipment-b"]));
        assert_eq!(result["orderRows"][0]["freightComplete"], json!(true));
        assert_eq!(result["orderRows"][0]["freightCost"], json!(7000));
        assert_eq!(result["actualFreightKnown"], json!(7000));
        assert_eq!(result["freightUnknown"], json!(0));
        assert_eq!(result["shippingKnown"], json!(5000));
    }
}
