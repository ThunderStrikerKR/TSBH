use serde_json::{json, Value};

use super::{checked_sum, field, find_by_id, flag, has_value, req, rows, safe, set, str_field, MAX_SAFE};

pub(super) fn resolve_unit_price(
    product: &Value,
    variant_id: &str,
    client: &Value,
) -> Result<Value, String> {
    if str_field(client, "id").is_empty() {
        return Ok(Value::Null);
    }
    let variant = rows(field(product, "variants"))
        .iter()
        .find(|v| str_field(v, "id") == variant_id);
    let price = if str_field(client, "tier") == "special" {
        let overrides = field(client, "overrides");
        let by_id = field(overrides, variant_id);
        if has_value(by_id) {
            by_id.clone()
        } else {
            let by_legacy = variant
                .and_then(|v| {
                    rows(field(v, "legacyIds"))
                        .iter()
                        .find_map(|id| {
                            let p = field(overrides, id.as_str()?);
                            has_value(p).then_some(p.clone())
                        })
                });
            by_legacy
                .or_else(|| variant.map(|v| field(v, "wholesale").clone()))
                .unwrap_or(Value::Null)
        }
    } else {
        let tier = str_field(client, "tier");
        variant.map(|v| field(v, tier).clone()).unwrap_or(Value::Null)
    };
    Ok(if price.as_i64() == Some(0) { Value::Null } else { price })
}

pub(super) fn totals(order: &Value, client: &Value) -> Result<Value, String> {
    let effective_client = if flag(order, "pricesLocked") && has_value(field(order, "clientSnapshot")) {
        field(order, "clientSnapshot")
    } else {
        client
    };
    let items: Vec<&Value> = rows(field(order, "recipients")).iter()
        .flat_map(|r| rows(field(r, "items")).iter())
        .collect();
    let valid = !items.is_empty() && items.iter().all(|item| {
        !str_field(item, "productId").is_empty()
            && !str_field(item, "variantId").is_empty()
            && safe(field(item, "unitPrice")).is_some()
            && field(item, "quantity").as_i64().is_some_and(|q| q > 0 && q <= MAX_SAFE)
    });
    let subtotal = if valid {
        items.iter()
            .map(|item| {
                safe(field(item, "unitPrice"))?.checked_mul(field(item, "quantity").as_i64()?)
                    .filter(|n| (0..=MAX_SAFE).contains(n))
            })
            .collect::<Option<Vec<_>>>()
            .and_then(checked_sum)
    } else {
        None
    };
    let shipping = if flag(effective_client, "shippingFeeExcluded") ||
        str_field(order, "freightCode") == "020" {
        json!(0)
    } else {
        field(order, "shippingFee").clone()
    };
    let complete = subtotal.is_some() && safe(&shipping).is_some();
    let vat = if flag(order, "pricesLocked") && has_value(field(order, "vatAmount")) {
        field(order, "vatAmount").clone()
    } else if !flag(order, "applyVat") {
        json!(0)
    } else if complete {
        let base = subtotal.unwrap() + safe(&shipping).unwrap();
        // JS Math.round((subtotal + shipping) * 0.1) uses binary64.
        let value = ((base as f64) * 0.1).round() as i64;
        json!(value)
    } else {
        Value::Null
    };
    let total = if complete {
        safe(&vat).and_then(|v| checked_sum([subtotal.unwrap(), safe(&shipping).unwrap(), v]))
    } else {
        None
    };
    // TS totals() clears an unsafe total together with the subtotal and VAT.
    if complete && total.is_none() {
        return Ok(json!({"subtotal":null,"shipping":shipping,"vat":null,"total":null}));
    }
    Ok(json!({"subtotal":subtotal,"shipping":shipping,"vat":vat,"total":total}))
}

pub(super) fn should_refresh(payload: &Value) -> Result<bool, String> {
    let order = req(payload, "order")?;
    let id = str_field(order, "id");
    let persisted = rows(field(payload, "persistedOrders"))
        .iter().chain(rows(field(payload, "persistedOrderIds")).iter())
        .any(|v| v.as_str() == Some(id) || str_field(v, "id") == id);
    Ok(!has_value(field(order, "confirmedAt"))
        && !has_value(field(order, "deletedAt"))
        && !flag(order, "pricesLocked")
        && !persisted)
}

pub(super) fn reprice_order(payload: &Value) -> Result<Value, String> {
    let mut order = req(payload, "order")?.clone();
    if has_value(field(&order, "confirmedAt")) || flag(&order, "pricesLocked") {
        return Ok(json!({"order":order,"refreshed":false}));
    }
    // An explicit employee edit may refresh persisted orders, but never frozen orders.
    let refresh = flag(payload, "explicitEdit") || should_refresh(payload)?;
    if !refresh {
        return Ok(json!({"order":order,"refreshed":false}));
    }
    let client = req(payload, "client")?;
    let products = payload.get("products")
        .or_else(|| payload.get("state").map(|s| field(s, "products")))
        .ok_or_else(|| "상품 목록 입력이 필요합니다.".to_string())?;
    // Rust also owns the post-reprice freeze. React must not overwrite
    // returned prices with a second, independent TypeScript business rule.
    let order_id=str_field(&order,"id").to_string();
    let frozen:Vec<String>=if flag(payload,"explicitEdit") {Vec::new()} else {
        rows(field(field(field(payload,"state"),"workflow"),"shipments"))
            .iter().filter(|ship|
                str_field(ship,"orderId")==order_id &&
                (flag(ship,"transferred") ||
                 field(ship,"step").as_i64().is_some_and(|step|step>=3)))
            .map(|ship|str_field(ship,"recipientId").to_string()).collect()
    };
    let recipients = order.get_mut("recipients").and_then(Value::as_array_mut)
        .ok_or_else(|| "수취인 목록을 확인하세요.".to_string())?;
    for recipient in recipients {
        if frozen.iter().any(|id|id==str_field(recipient,"id")) { continue; }
        let items = recipient.get_mut("items").and_then(Value::as_array_mut)
            .ok_or_else(|| "상품 목록을 확인하세요.".to_string())?;
        for item in items {
            if let Some(override_price)=item.get("manualUnitPrice") {
                // A staff-confirmed override belongs to this order line
                // only. Never rewrite the catalog, and never silently
                // overwrite an explicit unit price during native repricing.
                let price=safe(override_price)
                    .ok_or("직접 수정한 상품 단가가 0 이상의 안전한 정수가 아닙니다.")?;
                set(item,"unitPrice",json!(price))?;
                continue;
            }
            let pid = str_field(item, "productId").to_string();
            let vid = str_field(item, "variantId").to_string();
            if vid.is_empty() { continue; }
            if let Some(product) = find_by_id(products, &pid) {
                let unit_price = resolve_unit_price(product, &vid, client)?;
                set(item, "unitPrice", unit_price)?;
                if flag(field(item, "modelProposal"), "required") {
                    if let Some(variant) = rows(field(product, "variants"))
                        .iter().find(|v| str_field(v, "id") == vid) {
                        if has_value(field(variant, "url")) {
                            set(item, "url", field(variant, "url").clone())?;
                        }
                    }
                }
            }
        }
    }
    Ok(json!({"order":order,"refreshed":true}))
}


/// Read-only pricing projection for an employee-modified, unconfirmed intake.
/// Both the UI preview and SQLite working-reception writer use THIS route.
pub(super) fn reception_quote(payload:&Value)->Result<Value,String>{
    let state=req(payload,"state")?;
    let client=req(payload,"client")?;
    let original=req(payload,"order")?;
    if has_value(field(original,"confirmedAt")) || flag(original,"pricesLocked"){
        return Err("확정된 주문은 미확정 접수 편집으로 재계산할 수 없습니다.".into());
    }
    let repriced=reprice_order(&json!({
        "order":original,"state":state,"client":client,
        "explicitEdit":true,"persistedOrders":[]
    }))?;
    let mut order=repriced.get("order").filter(|value|value.is_object())
        .ok_or("Rust 품목 재계산 결과가 없습니다.")?.clone();
    let freight=super::shipping::reception_recommend(&order,state)?;
    let shipping=if flag(client,"shippingFeeExcluded"){json!(0)}else{
        field(&freight,"total").clone()
    };
    set(&mut order,"shippingFee",shipping)?;
    set(&mut order,"shippingVersion",field(&freight,"version").clone())?;
    let amounts=totals(&order,client)?;
    let mut lines=Vec::new();
    for recipient in rows(field(&order,"recipients")){
        for item in rows(field(recipient,"items")){
            let amount=safe(field(item,"unitPrice")).and_then(|unit|
                field(item,"quantity").as_i64().filter(|quantity|*quantity>0)
                    .and_then(|quantity|unit.checked_mul(quantity))
                    .filter(|value|(0..=MAX_SAFE).contains(value))
            );
            lines.push(json!({
                "recipientId":field(recipient,"id"),
                "itemId":field(item,"id"),
                "productId":field(item,"productId"),
                "variantId":field(item,"variantId"),
                "quantity":field(item,"quantity"),
                "unitPrice":field(item,"unitPrice"),
                "amount":amount,
                "manualUnitPrice":field(item,"manualUnitPrice"),
            }));
        }
    }
    let freight_lines=rows(field(&freight,"bundles")).iter().map(|bundle|json!({
        "recipientId":field(bundle,"recipientId"),
        "origin":field(bundle,"origin"),
        "appliedCharge":field(bundle,"appliedCharge"),
        "billableCharge":field(bundle,"billableCharge"),
        "freightCode":field(bundle,"freightCode"),
        "source":field(bundle,"source"),
    })).collect::<Vec<_>>();
    Ok(json!({
        "order":order,"totals":amounts,"linePrices":lines,
        "freightLines":freight_lines
    }))
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn native_reception_quote_multiplies_quantity_and_manual_price_and_freight() {
        let state=json!({"products":[{"id":"p","variants":[{"id":"v",
            "wholesale":12000}]}],"shipping":{"epoch":1}});
        let order=json!({"id":"draft","raw":"가상","orderType":"AGENCY",
            "recipients":[{"id":"r","address":"제주특별자치도 제주시 연동 1",
                "freightCode":"010","dispatchOrigin":"FACTORY",
                "shippingCharges":{"FACTORY":3000},
                "remoteDelivery":{"address":"제주특별자치도 제주시 연동 1",
                    "checked":true,"manual":true},
                "items":[{"id":"i","productId":"p","variantId":"v",
                    "quantity":2,"unitPrice":9900,"manualUnitPrice":9900,
                    "origin":"FACTORY"}]}]});
        let client=json!({"id":"c","tier":"wholesale"});
        let quote=reception_quote(&json!({"order":order,"state":state,"client":client}))
            .unwrap();
        assert_eq!(quote["linePrices"][0]["unitPrice"],9900);
        assert_eq!(quote["linePrices"][0]["amount"],19800);
        assert_eq!(quote["freightLines"][0]["appliedCharge"],6000);
        assert_eq!(quote["totals"]["subtotal"],19800);
        assert_eq!(quote["totals"]["shipping"],6000);
        assert_eq!(quote["totals"]["total"],25800);
    }
    #[test]
    fn manual_unit_price_survives_reprice_but_auto_lines_follow_catalog() {
        let state=json!({"products":[{"id":"p","variants":[{"id":"v",
            "wholesale":12000}]}],"workflow":{"shipments":[]}});
        let order=json!({"id":"draft","recipients":[{"id":"r","items":[
            {"id":"a","productId":"p","variantId":"v",
             "unitPrice":9900,"manualUnitPrice":9900,"quantity":2},
            {"id":"b","productId":"p","variantId":"v",
             "unitPrice":8000,"quantity":1}
        ]}]});
        let result=reprice_order(&json!({"order":order,"state":state,
            "client":{"id":"c","tier":"wholesale"},"explicitEdit":true}))
            .unwrap()["order"].clone();
        assert_eq!(result["recipients"][0]["items"][0]["unitPrice"],9900);
        assert_eq!(result["recipients"][0]["items"][1]["unitPrice"],12000);
        assert_eq!(totals(&json!({"recipients":result["recipients"],
            "shippingFee":3000,"applyVat":false}),
            &json!({"id":"c"})).unwrap()["subtotal"],31800);
    }
    #[test]
    fn passive_catalog_reprice_keeps_frozen_shipments_at_original_price() {
        let state=json!({
            "products":[{"id":"p","variants":[{"id":"v","wholesale":12000}]}],
            "workflow":{"shipments":[
                {"orderId":"o","recipientId":"frozen","transferred":true,"step":1},
                {"orderId":"o","recipientId":"confirmed","transferred":false,"step":3},
                {"orderId":"other","recipientId":"open","transferred":true,"step":3}
            ]}
        });
        let order=json!({
            "id":"o","recipients":[
                {"id":"frozen","items":[{"id":"i1","productId":"p","variantId":"v","unitPrice":8000}]},
                {"id":"confirmed","items":[{"id":"i2","productId":"p","variantId":"v","unitPrice":9000}]},
                {"id":"open","items":[{"id":"i3","productId":"p","variantId":"v","unitPrice":10000}]}
            ]
        });
        let client=json!({"id":"dealer","tier":"wholesale"});
        let next=reprice_order(&json!({"state":state,"order":order,"client":client,
            "persistedOrders":[]})).unwrap();
        assert_eq!(next["refreshed"],true);
        assert_eq!(next["order"]["recipients"][0]["items"][0]["unitPrice"],8000);
        assert_eq!(next["order"]["recipients"][1]["items"][0]["unitPrice"],9000);
        assert_eq!(next["order"]["recipients"][2]["items"][0]["unitPrice"],12000);
    }
}
