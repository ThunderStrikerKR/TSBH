//! Read-only Rust projection of the live workflow board.
//! Never creates events, allocates IDs, changes persisted state, or saves SQLite.
//! Derives the visible per-shipment and per-group totals from the same Rust
//! rules that validate subsequent staff-approved workflow transitions.
use serde_json::{json, Value};
use super::{checked_sum, field, find_by_id, flag, has_value, req, rows, safe, set, str_field};
use super::{workflow, workflow_issues};

fn sum_field(totals: &[Value], key: &str) -> Value {
    if totals.is_empty() { return Value::Null; }
    totals.iter()
        .map(|row| safe(field(row, key)))
        .collect::<Option<Vec<_>>>()
        .and_then(checked_sum)
        .map_or(Value::Null, |value| json!(value))
}

/// Rust owns the monetary comparison for re-notification; React only displays it.
/// Never convert a missing/invalid historical amount into a misleading zero.
fn notice_delta(group: &Value, current: &Value) -> Value {
    let old = rows(field(group, "notices")).last()
        .and_then(|notice| safe(field(notice, "total")));
    let latest = safe(current);
    match (latest, old) {
        (Some(latest), Some(old)) => latest.checked_sub(old)
            .map_or(Value::Null, |amount| json!(amount)),
        _ => Value::Null,
    }
}

fn vat_applies(order: &Value, recipient: &Value) -> bool {
    field(recipient, "vatOverride").as_bool()
        .or_else(|| field(recipient, "vatDefault").as_bool())
        .unwrap_or_else(|| flag(order, "applyVat"))
}

/// Returns a pure projection of all active workflow groups, with no state
/// mutation. All amounts and action blockers are calculated in compiled Rust.
/// React remains responsible only for presenting this result.
pub(super) fn project(payload: &Value) -> Result<Value, String> {
    let state = req(payload, "state")?;
    let orders = field(state, "orders");
    let workflow_state = field(state, "workflow");
    let all_shipments = rows(field(workflow_state, "shipments"));
    let mut projected = Vec::new();

    let scope=payload.get("groupIds").map(|ids|ids.as_array()
        .ok_or("업무 미리보기 묶음 목록 오류")).transpose()?;
    for group in rows(field(workflow_state, "groups")) {
        if has_value(field(group, "deletedAt")) { continue; }
        let group_id = str_field(group, "id");
        if scope.is_some_and(|ids|!ids.iter().any(|id|id.as_str()==Some(group_id))){
            continue;
        }
        let ships = all_shipments.iter()
            .filter(|ship| str_field(ship, "groupId") == group_id)
            .filter(|ship| !flag(ship, "legacyArchived"))
            .filter(|ship| !find_by_id(orders, str_field(ship, "orderId"))
                .is_some_and(|order| has_value(field(order, "deletedAt"))))
            .collect::<Vec<_>>();
        if ships.is_empty() { continue; }

        // GroupAccount in the original TypeScript sees only this group's
        // recipients, which matters when one source order is split between
        // BANDOMALL and BANDOSTAND account mappings.
        let first_order = find_by_id(orders, str_field(ships[0], "orderId"))
            .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
        let mut group_order = first_order.clone();
        let group_rows = rows(field(first_order, "recipients")).iter()
            .filter(|row| ships.iter().any(|ship|
                str_field(ship, "recipientId") == str_field(row, "id")))
            .cloned().collect::<Vec<_>>();
        set(&mut group_order, "recipients", json!(group_rows))?;
        if let Some(mode) = group.get("accountMode").filter(|v| !v.is_null()) {
            set(&mut group_order, "accountMode", mode.clone())?;
        }
        if str_field(group, "accountMode") == "MANUAL" {
            set(&mut group_order, "accountId", field(group, "accountId").clone())?;
        }
        let account_key = |value: &Value| json!([
            field(value, "id"),
            str_field(value, "bank").trim(),
            str_field(value, "number").trim(),
            str_field(value, "holder").trim()
        ]);
        // Retail can legitimately have no saved client; it has no account
        // allocations to display in group management.
        let account_id = workflow::party(state, &group_order)
            .and_then(|party| workflow::account_for(state, group, &group_order, party))
            .map(account_key);

        let mut totals = Vec::with_capacity(ships.len());
        let mut shipment_projections = Vec::with_capacity(ships.len());
        let mut all_vat = true;
        let mut any_vat = false;
        for ship in &ships {
            let order = find_by_id(orders, str_field(ship, "orderId"))
                .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
            let recipient = find_by_id(field(order, "recipients"), str_field(ship, "recipientId"))
                .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
            let vat = vat_applies(order, recipient);
            let freight_code = workflow::shipment_freight_code(order, recipient);
            let excluded = workflow::party(state, order)
                .is_some_and(|client| flag(client, "shippingFeeExcluded"));
            let remote = super::remote_delivery::resolve_shipment(recipient,ship,order)?;
            let mut display_charges = serde_json::Map::new();
            for item in rows(field(recipient, "items")) {
                let origin = str_field(item, "origin");
                if !["FACTORY", "STORE"].contains(&origin) { continue; }
                let value = if freight_code == "020" || excluded {
                    json!(0)
                } else {
                    let base=field(field(ship,"charges"),origin);
                    if base.is_null() {Value::Null} else {
                        super::remote_delivery::multiply_shipment(safe(base),recipient,ship,order)
                            .map(|amount|json!(amount)).unwrap_or(Value::Null)
                    }
                };
                display_charges.insert(origin.to_string(), value);
            }
            all_vat &= vat;
            any_vat |= vat;
            let amount = workflow::shipment_totals(state, str_field(ship, "id"))?;
            // Only settled allocations with the selected group's account
            // count as paid; confirmations do not create a second payment.
            let mut paid = 0_i64;
            for payment in rows(field(workflow_state, "payments")).iter().filter(|payment|
                str_field(payment, "dealerId") == str_field(order, "clientId") &&
                account_id.as_ref().is_some_and(|key| *key == account_key(field(payment, "account")))) {
                for allocation in rows(field(payment, "allocations")).iter().filter(|allocation|
                    str_field(allocation, "shipmentId") == str_field(ship, "id")) {
                    let value = safe(field(allocation, "amount"))
                        .ok_or("입금 배분 금액을 확인하세요.")?;
                    paid = checked_sum([paid, value])
                        .ok_or("입금액 합계를 확인하세요.")?;
                }
            }
            let mut shipment_projection=json!({
                "shipmentId": field(ship, "id"),
                "paid": paid,
                "totals": amount,
                "freightCode": freight_code,
                "displayCharges": display_charges,
                "freightEditingBlocked": freight_code == "020" || excluded,
                "vatIncluded": vat
            });
            // Omit the optional locality projection for ordinary orders to
            // avoid changing unrelated legacy snapshot contracts.
            if matches!(str_field(&remote,"status"),"auto"|"review") ||
                flag(&remote,"manual") || flag(&remote,"legacy") {
                shipment_projection["remoteDelivery"]=remote;
            }
            shipment_projections.push(shipment_projection);
            totals.push(amount);
        }
        let mut stage_validity=super::workflow_stages::aggregate(ships.iter().copied());
        let step=super::workflow_stages::prefix(stage_validity);
        let needs_notice = workflow_issues::notice_pending(state, group_id)?;
        if needs_notice{stage_validity[2]=false;stage_validity[3]=false;}
        let next_step = if needs_notice && step >= 2 { 3 } else { step + 1 };
        let blockers = if next_step <= 4 {
            workflow_issues::stage_issues(state, group_id, next_step)?
        } else { Vec::new() };
        let mut issues = workflow_issues::workflow_issues(state, group_id)?;
        // The original TS ValidationIssue omits an unknown updatedAt
        // (JSON.stringify(undefined)), while Rust serde_json null would
        // expose a real field. Preserve the original preview payload shape.
        for issue in &mut issues {
            if field(issue, "updatedAt").is_null() {
                if let Some(map) = issue.as_object_mut() {
                    map.remove("updatedAt");
                }
            }
        }
        let group_total = sum_field(&totals, "total");
        projected.push(json!({
            "groupId": group_id,
            "step": step,
            "stageValidity": stage_validity,
            "nextStep": next_step,
            "needsNotice": needs_notice,
            "transferred": ships.iter().any(|s| flag(s, "transferred")),
            "shipmentCount": ships.len(),
            "totals": {
                "subtotal": sum_field(&totals, "subtotal"),
                "shipping": sum_field(&totals, "shipping"),
                "vat": sum_field(&totals, "vat"),
                "total": group_total,
            },
            "noticeDelta": notice_delta(group, &group_total),
            "vatLabel": if all_vat { "부가세 포함" }
                else if any_vat { "부가세 포함/별도" }
                else { "부가세 별도" },
            "shipments": shipment_projections,
            "issues": issues,
            "nextStageBlockers": blockers
        }));
    }
    Ok(json!({ "groups": projected }))
}


/// Read-only line-price projection shared by the full order preview and
/// the independent instant line quote. The Rust engine is the ONLY place
/// multiplying the business unit price and quantity.
fn line_amounts(recipients:&[Value])->Vec<Value>{
    recipients.iter().flat_map(|recipient| {
        rows(field(recipient,"items")).iter().map(|item| {
            let amount=if str_field(item,"productId").is_empty() ||
                str_field(item,"variantId").is_empty() { None } else {
                safe(field(item,"unitPrice"))
                    .zip(field(item,"quantity").as_i64())
                    .and_then(|(price,qty)|price.checked_mul(qty))
                    .filter(|value|(-super::MAX_SAFE..=super::MAX_SAFE).contains(value))
            };
            json!({"recipientId":field(recipient,"id"),
                "itemId":field(item,"id"),"amount":amount})
        })
    }).collect()
}

/// The item's Rust-calculated amount must not wait for workflow reconciliation,
/// shipment totals, customer issue validation or account allocation IPC.
/// This is a read-only response; there are no TS fallback business calculations.
pub(super) fn project_order_lines(payload:&Value)->Result<Value,String>{
    let draft=req(payload,"draft")?;
    let id=super::required_str(draft,"id")?;
    Ok(json!({"orderId":id,
        "lineAmounts":line_amounts(rows(field(draft,"recipients")))}))
}

/** The order-detail footer includes even transferred shipments, except
 * archived/deleted ones, exactly like workflow.ts::orderGrandTotal.
 * Recipient VAT display is separate from the monetary snapshot.
 */
pub(super) fn project_order(payload: &Value) -> Result<Value, String> {
    let state = req(payload, "state")?;
    let order_id = super::required_str(payload, "orderId")?;
    let order = find_by_id(field(state, "orders"), order_id)
        .or_else(|| payload.get("draft").filter(|draft| str_field(draft, "id") == order_id))
        .ok_or("주문 미리보기의 발주서를 확인하세요.")?;
    let groups = rows(field(field(state, "workflow"), "groups"));
    let ships = rows(field(field(state, "workflow"), "shipments")).iter()
        .filter(|ship| str_field(ship, "orderId") == order_id)
        .filter(|ship| !flag(ship, "legacyArchived"))
        .filter(|ship| !groups.iter().any(|group|
            str_field(group, "id") == str_field(ship, "groupId")
                && has_value(field(group, "deletedAt"))))
        .collect::<Vec<_>>();
    let amounts = ships.iter()
        .map(|ship| workflow::shipment_totals(state, str_field(ship, "id")))
        .collect::<Result<Vec<_>, _>>()?;
    let recipients = rows(field(order, "recipients"));
    let line_amounts = line_amounts(recipients);
    let has_vat = recipients.iter().any(|row| vat_applies(order, row));
    let has_excluded = recipients.iter().any(|row| !vat_applies(order, row));
    Ok(json!({
        "orderId": order_id,
        "shipmentCount": ships.len(),
        "lineAmounts": line_amounts,
        "total": sum_field(&amounts, "total"),
        "vatLabel": if has_vat && has_excluded { "부가세 포함/별도" }
            else if has_vat { "부가세 포함" } else { "부가세 별도" }
    }))
}

#[cfg(test)]
mod tests {
    use super::{notice_delta, project, project_order_lines};
    use serde_json::{json, Value};

    #[test]
    fn independent_native_item_price_is_available_without_shipments_or_workflow(){
        let order=json!({"id":"o","recipients":[{"id":"r","items":[
            {"id":"i","productId":"p","variantId":"v","unitPrice":42000,"quantity":2},
            {"id":"pending","productId":"p","variantId":"v","unitPrice":null,"quantity":2}
        ]}]});
        let result=project_order_lines(&json!({"draft":order})).unwrap();
        assert_eq!(result["orderId"],"o");
        assert_eq!(result["lineAmounts"][0]["amount"],84000);
        assert_eq!(result["lineAmounts"][1]["amount"],Value::Null);
        let invalid=json!({"id":"o","recipients":[{"id":"r","items":[{
            "id":"i","productId":"p","variantId":"v",
            "unitPrice":9007199254740991_i64,"quantity":2
        }]}]});
        let result=project_order_lines(&json!({"draft":invalid})).unwrap();
        assert_eq!(result["lineAmounts"][0]["amount"],Value::Null);
    }
    #[test]
    fn re_notified_amounts_match_original_typescript_arithmetic() {
        // Original UI: current group total - last issued notice.total.
        let group = json!({"notices":[{"total":70000}]});
        assert_eq!(notice_delta(&group, &json!(72000)), json!(2000));
        assert_eq!(notice_delta(&group, &json!(69000)), json!(-1000));
        assert_eq!(notice_delta(&group, &json!(70000)), json!(0));
        assert_eq!(notice_delta(&json!({"notices":[]}), &json!(72000)), json!(null));
        assert_eq!(notice_delta(&group, &Value::Null), json!(null));
        assert_eq!(notice_delta(&json!({"notices":[{"total":null}]}), &json!(70000)), json!(null));
        assert_eq!(notice_delta(&json!({"notices":[{"total":"70000"}]}), &json!(72000)), json!(null));
    }

    #[test]
    fn no_workflow_groups_returns_empty_projection_without_creating_events() {
        let state = json!({
            "orders": [], "workflow": {
                "groups": [], "shipments": [], "events": []
            }
        });
        let initial = state.clone();
        let projected = project(&json!({"state": state})).expect("empty preview");
        assert_eq!(projected, json!({"groups": []}));
        assert_eq!(initial["workflow"]["events"], json!([]));
    }

    #[test]
    fn deleted_and_empty_groups_do_not_show_or_create_rows() {
        let state = json!({
            "orders": [], "workflow": {
                "groups": [
                    {"id": "deleted", "deletedAt": "2026-11-13T00:00:00Z"},
                    {"id": "no-shipments"}
                ],
                "shipments": [], "events": []
            }
        });
        assert_eq!(
            project(&json!({"state": state})).expect("hidden groups"),
            json!({"groups": []})
        );
    }

    #[test]
    fn malformed_active_shipments_are_not_silently_reported_as_zero() {
        let state = json!({
            "orders": [],
            "workflow": {
                "groups": [{"id": "group:one"}],
                "shipments": [{
                    "id": "shipment:one", "groupId": "group:one",
                    "orderId": "missing-order", "recipientId": "recipient:one"
                }]
            }
        });
        assert!(project(&json!({"state": state})).is_err());
    }
}


#[cfg(test)]
mod workflow_performance_benchmark{
    use super::*;
    use std::time::Instant;

    fn fixture(count:usize)->Value{
        let mut orders=Vec::new();
        let mut groups=Vec::new();
        let mut ships=Vec::new();
        for n in 0..count{
            orders.push(json!({
                "id":format!("o{n}"),"clientId":"c","orderType":"AGENCY",
                "applyVat":true,"freightCode":"010","confirmedAt":"2026-11-25",
                "recipients":[{"id":format!("r{n}"),"name":"합성 수취인",
                    "address":"서울특별시 중구 세종대로 1","phone":"010-1234-5678",
                    "items":[{"id":format!("i{n}"),"productId":"p","variantId":"v",
                        "name":"합성 상품","quantity":2,"quantityConfirmed":true,
                        "unitPrice":10000,"origin":"FACTORY"}]}]
            }));
            groups.push(json!({"id":format!("g{n}"),"dealerId":"c","needsNotice":false}));
            ships.push(json!({"id":format!("s{n}"),"orderId":format!("o{n}"),
                "recipientId":format!("r{n}"),"groupId":format!("g{n}"),
                "step":1,"charges":{"FACTORY":3000},"transferred":false}));
        }
        json!({"orders":orders,"products":[{"id":"p","variants":[{"id":"v"}]}],
            "clients":[{"id":"c","name":"합성 거래처","tier":"wholesale",
                "payment":"POSTPAID","address":"서울특별시 중구 세종대로 1",
                "phone":"02-1234-5678"}],
            "settings":{"accounts":[{"id":"a","bank":"가상은행","number":"123",
                "holder":"합성 계좌"}],"defaultAccountId":"a"},
            "workflow":{"groups":groups,"shipments":ships,"events":[],
                "payments":[],"confirmations":[],"queue":[]}})
    }
    fn median_ms(values:&mut [f64])->f64{
        values.sort_by(f64::total_cmp);
        values[values.len()/2]
    }
    #[test]
    fn synthetic_full_vs_scoped_workflow_projection(){
        for count in [1,10,30,100]{
            let state=fixture(count);
            // Construct both payloads outside the timer: this measures native
            // projection only, not SQLite IO, JSON cloning or UI response.
            let full_request=json!({"state":&state});
            let scoped_request=json!({"state":&state,"groupIds":["g0"]});
            let full=project(&full_request).unwrap();
            let scoped=project(&scoped_request).unwrap();
            assert_eq!(full["groups"].as_array().unwrap().len(),count);
            assert_eq!(scoped["groups"],json!([full["groups"][0]]));
            assert_eq!(scoped["groups"][0]["totals"]["total"],25300);
            let mut full_ms=Vec::new();
            let mut scoped_ms=Vec::new();
            for n in 0..11{
                let measure=|request:&Value| {
                    let start=Instant::now();
                    let value=project(std::hint::black_box(request)).unwrap();
                    std::hint::black_box(value);
                    start.elapsed().as_secs_f64()*1000.0
                };
                if n%2==0{
                    full_ms.push(measure(&full_request));
                    scoped_ms.push(measure(&scoped_request));
                }else{
                    scoped_ms.push(measure(&scoped_request));
                    full_ms.push(measure(&full_request));
                }
            }
            let full=median_ms(&mut full_ms);
            let scoped=median_ms(&mut scoped_ms);
            println!("[workflow-bench] groups={count} samples=11 full_median_ms={full:.3} scoped_median_ms={scoped:.3}");
        }
    }
}
