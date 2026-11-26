use serde_json::{json, Value};

use super::{checked_sum, field, find_by_id, flag, has_value, req, rows, safe, set, str_field, MAX_SAFE};

/// Apply only the explicit order-line override. Return false for automatic pricing.
pub(super) fn apply_manual_unit_price(item:&mut Value)->Result<bool,String>{
    let Some(price)=super::manual_unit_price(item)? else{return Ok(false);};
    set(item,"unitPrice",json!(price))?;
    Ok(true)
}

/// Retain an explicit amount only when the resolved sellable item is identical.
pub(super) fn retain_manual_unit_price(item:&mut Value,previous:&Value)->Result<(),String>{
    let same=!str_field(item,"productId").is_empty() &&
        !str_field(item,"variantId").is_empty() &&
        field(item,"productId")==field(previous,"productId") &&
        field(item,"variantId")==field(previous,"variantId");
    let retained=if same{super::manual_unit_price(previous)?}else{None};
    item.as_object_mut().ok_or("상품 데이터 오류")?.remove("manualUnitPrice");
    if let Some(amount)=retained{
        set(item,"manualUnitPrice",json!(amount))?;
        apply_manual_unit_price(item)?;
    }
    Ok(())
}

#[cfg(test)]
mod manual_price_retention_tests{
    use super::*;
    #[test]
    fn identical_catalog_identity_preserves_zero_and_positive_override(){
        for amount in [0,9900]{
            let old=json!({"productId":"p","variantId":"v","manualUnitPrice":amount});
            let mut next=json!({"productId":"p","variantId":"v","unitPrice":15000});
            retain_manual_unit_price(&mut next,&old).unwrap();
            assert_eq!(next["unitPrice"],amount);
            assert_eq!(next["manualUnitPrice"],amount);
            let mut different=json!({"productId":"p","variantId":"v2","unitPrice":20000,
                "manualUnitPrice":amount});
            retain_manual_unit_price(&mut different,&old).unwrap();
            assert!(different.get("manualUnitPrice").is_none());
            assert_eq!(different["unitPrice"],20000);
        }
    }
}

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

/// All registered and manually entered unit prices exclude VAT.
pub(crate) fn vat_applies(order:&Value,recipient:&Value)->bool{
    field(recipient,"vatOverride").as_bool()
        .or_else(||field(recipient,"vatDefault").as_bool())
        .unwrap_or_else(||flag(order,"applyVat"))
}
pub(super) fn vat_rate(recipient:&Value)->Result<i64,String>{
    let rate=field(recipient,"vatRateBps").as_i64().unwrap_or(1000);
    if !(0..=10_000).contains(&rate){
        return Err("부가세율이 허용 범위를 벗어났습니다.".into());
    }
    Ok(rate)
}
fn item_subtotal<'a>(items:impl IntoIterator<Item=&'a Value>)->Option<i64>{
    let amounts=items.into_iter().map(|item|{
        if str_field(item,"productId").is_empty() || str_field(item,"variantId").is_empty(){
            return None;
        }
        let unit=safe(field(item,"unitPrice"))?;
        let quantity=field(item,"quantity").as_i64().filter(|n|*n>0 && *n<=MAX_SAFE)?;
        unit.checked_mul(quantity).filter(|n|*n<=MAX_SAFE)
    }).collect::<Option<Vec<_>>>()?;
    if amounts.is_empty(){None}else{checked_sum(amounts)}
}
fn calculate_vat(subtotal:Option<i64>,shipping:Option<i64>,enabled:bool,rate:i64)->Option<i64>{
    if !enabled{return Some(0);}
    let base=checked_sum([subtotal?,shipping?])?;
    i64::try_from((i128::from(base)*i128::from(rate)+5000)/10000)
        .ok().filter(|n|*n<=MAX_SAFE)
}
fn payable(subtotal:Option<i64>,shipping:Option<i64>,vat:Option<i64>)->Option<i64>{
    checked_sum([subtotal?,shipping?,vat?])
}
pub(super) fn shipment_amounts(order:&Value,row:&Value,shipping:Option<i64>)->Result<Value,String>{
    let subtotal=item_subtotal(rows(field(row,"items")));
    let vat=calculate_vat(subtotal,shipping,vat_applies(order,row),vat_rate(row)?);
    Ok(json!({"subtotal":subtotal,"shipping":shipping,"vat":vat,
        "total":payable(subtotal,shipping,vat)}))
}

/// Compatibility calculation for older unlinked orders with only an
/// aggregate freight amount. Do not invent per-recipient allocations.
pub(super) fn totals(order:&Value,client:&Value)->Result<Value,String>{
    let effective_client=if flag(order,"pricesLocked") && has_value(field(order,"clientSnapshot")){
        field(order,"clientSnapshot")
    }else{client};
    let recipients=rows(field(order,"recipients"));
    let row=if recipients.len()==1{&recipients[0]}else{&Value::Null};
    let subtotal=item_subtotal(recipients.iter().flat_map(|r|rows(field(r,"items")).iter()));
    let shipping=if flag(effective_client,"shippingFeeExcluded") ||
        super::workflow::shipment_freight_code(order,row)=="020"{
        json!(0)
    }else{field(order,"shippingFee").clone()};
    let charge=safe(&shipping);
    let vat=if flag(order,"pricesLocked") && has_value(field(order,"vatAmount")){
        field(order,"vatAmount").clone()
    }else{json!(calculate_vat(subtotal,charge,vat_applies(order,row),vat_rate(row)?))};
    let total=payable(subtotal,charge,safe(&vat));
    if subtotal.is_some() && charge.is_some() && total.is_none(){
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
            if apply_manual_unit_price(item)? {continue;}
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


/// Reception billing is shipment-aware: each recipient may override VAT,
/// COD, origin and remote-area shipping independently. Never substitute a
/// partial subtotal for a final payable amount.
fn reception_totals(order:&Value,client:&Value,freight:&Value)->Result<Value,String>{
    let recipients=rows(field(order,"recipients"));
    if recipients.is_empty(){
        return Ok(json!({"subtotal":null,"shipping":null,"vat":null,"total":null}));
    }
    let mut subtotals=Vec::with_capacity(recipients.len());
    let mut shippings=Vec::with_capacity(recipients.len());
    let mut vats=Vec::with_capacity(recipients.len());
    let mut pending_recipients=Vec::new();
    for recipient in recipients{
        let bundles=rows(field(freight,"bundles")).iter()
            .filter(|bundle|field(bundle,"recipientId")==field(recipient,"id"))
            .collect::<Vec<_>>();
        let shipping=if flag(client,"shippingFeeExcluded"){Some(0)}
            else if bundles.is_empty(){None}
            else{
                bundles.iter()
                    .map(|bundle|safe(field(bundle,"billableCharge")))
                    .collect::<Option<Vec<i64>>>().and_then(checked_sum)
            };
        let amounts=shipment_amounts(order,recipient,shipping)?;
        let subtotal=safe(field(&amounts,"subtotal"));
        let vat=safe(field(&amounts,"vat"));
        if subtotal.is_none() || shipping.is_none() || vat.is_none(){
            pending_recipients.push(json!({
                "recipientId":field(recipient,"id"),
                "product":subtotal.is_none(),
                "freight":shipping.is_none(),
                "vat":vat.is_none()
            }));
        }
        subtotals.push(subtotal);
        shippings.push(shipping);
        vats.push(vat);
    }
    let subtotal=subtotals.into_iter().collect::<Option<Vec<_>>>()
        .and_then(checked_sum);
    let shipping=shippings.into_iter().collect::<Option<Vec<_>>>()
        .and_then(checked_sum);
    let vat=vats.into_iter().collect::<Option<Vec<_>>>()
        .and_then(checked_sum);
    let total=payable(subtotal,shipping,vat);
    Ok(json!({
        "subtotal":subtotal,"shipping":shipping,"vat":vat,"total":total,
        "pendingRecipients":pending_recipients
    }))
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
    let order=repriced.get("order").filter(|value|value.is_object())
        .ok_or("Rust 품목 재계산 결과가 없습니다.")?.clone();
    finish_reception_quote(order,state,client)
}

fn finish_reception_quote(mut order:Value,state:&Value,client:&Value)->Result<Value,String>{
    let freight=super::shipping::reception_recommend_for_client(&order,state,client)?;
    let shipping=field(&freight,"total").clone();
    set(&mut order,"shippingFee",shipping)?;
    set(&mut order,"shippingVersion",field(&freight,"version").clone())?;
    let amounts=reception_totals(&order,client,&freight)?;
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
        "billingReason":field(bundle,"billingReason"),
        "freightCode":field(bundle,"freightCode"),
        "source":field(bundle,"source"),
    })).collect::<Vec<_>>();
    Ok(json!({
        "order":order,"totals":amounts,"linePrices":lines,
        "freightLines":freight_lines
    }))
}

/// Ephemeral native cache for employee edit previews only. The owning host
/// keeps it in memory; neither persisted drafts nor remote callers supply
/// cached money. Full quote remains the final approval/write authority.
/// Compare complete pricing masters conservatively, and the exact recipient
/// inputs, so a changed price, tariff, VAT, address or override cannot leak.
#[derive(Default)]
pub(crate) struct ReceptionEditQuoteCache{
    master:Option<Value>,
    order_header:Option<Value>,
    entries:std::collections::HashMap<String,RecipientQuoteEntry>,
    #[cfg(test)]
    recalculations:usize,
    #[cfg(test)]
    repricings:usize,
}
struct RecipientQuoteEntry{
    input:Value,
    quote:Value,
}
impl ReceptionEditQuoteCache{
    pub(crate) fn quote(&mut self,payload:&Value)->Result<Value,String>{
        let state=req(payload,"state")?;
        let client=req(payload,"client")?;
        let original=req(payload,"order")?;
        if has_value(field(original,"confirmedAt")) || flag(original,"pricesLocked"){
            return reception_quote(payload);
        }
        let recipients=rows(field(original,"recipients"));
        let mut ids=std::collections::HashSet::new();
        if recipients.is_empty() || recipients.len()>100 ||
            str_field(original,"id").is_empty() ||
            recipients.iter().any(|row|{
                let id=str_field(row,"id");
                id.is_empty() || !ids.insert(id)
            }){
            return reception_quote(payload);
        }
        let master=json!({"products":field(state,"products"),
            "shipping":field(state,"shipping"),"client":client});
        let mut header=original.clone();
        header.as_object_mut().ok_or("발주서 형식 오류")?.remove("recipients");
        // These two derived fields are overwritten by every native quote.
        if let Some(map)=header.as_object_mut(){
            map.remove("shippingFee");
            map.remove("shippingVersion");
        }
        if self.master.as_ref()!=Some(&master) || self.order_header.as_ref()!=Some(&header){
            self.entries.clear();
            self.master=Some(master);
            self.order_header=Some(header);
        }
        self.entries.retain(|id,_|ids.contains(id.as_str()));
        let mut quoted_rows=Vec::with_capacity(recipients.len());
        let mut amounts=Vec::with_capacity(recipients.len());
        let mut lines=Vec::new();
        let mut freight_lines=Vec::new();
        let mut versions=Vec::new();
        let mut pending=Vec::new();
        // Match shipping::recommend's exact version string, including key
        // ordering within each group. Do not parse/re-serialize that string.
        let epoch=field(field(state,"shipping"),"epoch").as_i64().unwrap_or(1);
        let prefix=format!("{{\"epoch\":{epoch},\"groups\":[");
        for recipient in recipients{
            let id=str_field(recipient,"id");
            if !self.entries.get(id).is_some_and(|entry|entry.input==*recipient){
                let mut single=original.clone();
                set(&mut single,"recipients",json!([recipient]))?;
                #[cfg(test)]
                {self.recalculations+=1;}
                // A freight/VAT/address-only edit cannot change the line
                // unit prices: the exact items and price master are unchanged.
                // Keep the new recipient metadata and the native priced items.
                let reusable_items=self.entries.get(id)
                    .filter(|entry|field(&entry.input,"items")==field(recipient,"items"))
                    .and_then(|entry|rows(field(field(&entry.quote,"order"),"recipients")).first())
                    .and_then(|row|row.get("items"))
                    .filter(|items|items.is_array()).cloned();
                let quote=if let Some(items)=reusable_items{
                    set(&mut single["recipients"][0],"items",items)?;
                    finish_reception_quote(single,state,client)?
                }else{
                    #[cfg(test)]
                    {self.repricings+=1;}
                    reception_quote(&json!({
                        "order":single,"state":state,"client":client
                    }))?
                };
                self.entries.insert(id.to_owned(),RecipientQuoteEntry{
                    input:recipient.clone(),quote
                });
            }
            let quote=&self.entries.get(id).ok_or("수취인 계산 캐시 누락")?.quote;
            let quoted=rows(field(field(quote,"order"),"recipients"));
            if quoted.len()!=1{return reception_quote(payload);}
            quoted_rows.push(quoted[0].clone());
            amounts.push(field(quote,"totals").clone());
            lines.extend(rows(field(quote,"linePrices")).iter().cloned());
            freight_lines.extend(rows(field(quote,"freightLines")).iter().cloned());
            pending.extend(rows(field(field(quote,"totals"),"pendingRecipients")).iter().cloned());
            let version=str_field(field(quote,"order"),"shippingVersion");
            let Some(inner)=version.strip_prefix(&prefix).and_then(|v|v.strip_suffix("]}"))
                else{return reception_quote(payload);};
            if !inner.is_empty(){versions.push(inner.to_owned());}
        }
        let sum=|key:&str|amounts.iter().map(|amount|safe(field(amount,key)))
            .collect::<Option<Vec<_>>>().and_then(checked_sum);
        let subtotal=sum("subtotal");
        let shipping=sum("shipping");
        let vat=sum("vat");
        let total=payable(subtotal,shipping,vat);
        // shippingFee on the order is the aggregate bundle quote, not the
        // per-recipient total (a recipient with no bundles remains pending).
        let shipping_fee=if flag(client,"shippingFeeExcluded"){Some(0)}
            else if freight_lines.is_empty(){None}
            else{freight_lines.iter().map(|line|safe(field(line,"billableCharge")))
                .collect::<Option<Vec<_>>>().and_then(checked_sum)};
        let mut order=original.clone();
        set(&mut order,"recipients",json!(quoted_rows))?;
        set(&mut order,"shippingFee",json!(shipping_fee))?;
        set(&mut order,"shippingVersion",json!(format!("{prefix}{}]}}",versions.join(","))))?;
        Ok(json!({"order":order,
            "totals":{"subtotal":subtotal,"shipping":shipping,"vat":vat,"total":total,
                "pendingRecipients":pending},
            "linePrices":lines,"freightLines":freight_lines}))
    }
}

#[cfg(test)]
mod reception_edit_cache_tests{
    use super::*;
    #[test]
    fn recipient_cache_matches_full_quote_after_freight_vat_master_and_address_edits(){
        let mut payload=json!({
            "state":{"products":[{"id":"p","origin":"FACTORY","variants":[{"id":"v","wholesale":10000}]}],
                "shipping":{"epoch":1,"productRules":{"p":{"version":"p1","fees":{"1":3000}}}}},
            "client":{"id":"c","tier":"wholesale"},
            "order":{"id":"draft","applyVat":true,"recipients":[
                {"id":"a","address":"서울특별시 중구 세종대로 1","freightCode":"010",
                    "shippingCharges":{"FACTORY":3000},"items":[{"id":"i1","productId":"p","variantId":"v","quantity":1,"origin":"FACTORY"}]},
                {"id":"b","address":"제주특별자치도 제주시 연동 1","freightCode":"010",
                    "shippingCharges":{"FACTORY":3000},"items":[{"id":"i2","productId":"p","variantId":"v","quantity":1,"origin":"FACTORY"}]}
            ]}
        });
        let mut cache=ReceptionEditQuoteCache::default();
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        let unchanged=cache.entries["b"].quote.clone();
        assert_eq!((cache.recalculations,cache.repricings),(2,2));
        payload["order"]["recipients"][0]["shippingCharges"]["FACTORY"]=json!(4000);
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        assert_eq!(cache.entries["b"].quote,unchanged);
        assert_eq!((cache.recalculations,cache.repricings),(3,2));
        payload["order"]["recipients"][0]["freightCode"]=json!("020");
        payload["order"]["recipients"][0]["vatOverride"]=json!(false);
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        payload["order"]["recipients"][1]["address"]=json!("서울특별시 강남구 테헤란로 1");
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        payload["state"]["products"][0]["variants"][0]["wholesale"]=json!(12000);
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        payload["state"]["shipping"]["epoch"]=json!(2);
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        payload["order"]["recipients"][1]["items"]=json!([]);
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
        payload["client"]["shippingFeeExcluded"]=json!(true);
        assert_eq!(cache.quote(&payload).unwrap(),reception_quote(&payload).unwrap());
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn freight_billing_reasons_distinguish_exempt_missing_collect_and_zero(){
        let mut payload=json!({
            "state":{"products":[{"id":"p","origin":"FACTORY","variants":[{"id":"v","wholesale":10000}]}],
                "shipping":{"epoch":1}},
            "client":{"id":"c","tier":"wholesale","shippingFeeExcluded":false},
            "order":{"id":"draft","freightCode":"010","applyVat":true,"recipients":[
                {"id":"r","dispatchOrigin":"FACTORY","address":"서울특별시 중구 세종대로 1",
                    "items":[{"id":"i","productId":"p","variantId":"v","origin":"FACTORY","quantity":1}]}
            ]}
        });
        let missing=reception_quote(&payload).unwrap();
        assert_eq!(missing["freightLines"][0]["billingReason"],"UNREGISTERED");
        assert!(missing["freightLines"][0]["billableCharge"].is_null());
        assert!(missing["totals"]["total"].is_null());
        payload["client"]["shippingFeeExcluded"]=json!(true);
        let waived=reception_quote(&payload).unwrap();
        assert_eq!(waived["freightLines"][0]["billingReason"],"EXEMPT");
        assert!(waived["freightLines"][0]["appliedCharge"].is_null());
        assert_eq!(waived["freightLines"][0]["billableCharge"],0);
        assert_eq!(waived["totals"]["total"],11000);
        payload["order"]["recipients"][0]["shippingCharges"]=json!({"FACTORY":3000});
        let waived_manual=reception_quote(&payload).unwrap();
        assert_eq!(waived_manual["freightLines"][0]["appliedCharge"],3000);
        assert_eq!(waived_manual["freightLines"][0]["billableCharge"],0);
        assert_eq!(waived_manual["order"]["recipients"][0]["shippingCharges"]["FACTORY"],3000);
        payload["client"]["shippingFeeExcluded"]=json!(false);
        let prepaid=reception_quote(&payload).unwrap();
        assert_eq!(prepaid["freightLines"][0]["billableCharge"],3000);
        payload["order"]["recipients"][0]["shippingCharges"]["FACTORY"]=json!(0);
        let zero=reception_quote(&payload).unwrap();
        assert_eq!(zero["freightLines"][0]["billingReason"],"CHARGED");
        assert_eq!(zero["freightLines"][0]["billableCharge"],0);
        payload["order"]["recipients"][0]["shippingCharges"]["FACTORY"]=Value::Null;
        payload["order"]["freightCode"]=json!("020");
        let collect=reception_quote(&payload).unwrap();
        assert_eq!(collect["freightLines"][0]["billingReason"],"COLLECT");
        assert_eq!(collect["freightLines"][0]["billableCharge"],0);
        assert_eq!(collect["totals"]["total"],11000);
    }
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
    fn two_recipients_with_complete_freight_and_mixed_vat_get_order_total(){
        let state=json!({"products":[{"id":"p","variants":[{"id":"v",
            "wholesale":59000}]}],"shipping":{"epoch":1}});
        let order=json!({"id":"draft","raw":"가상","orderType":"AGENCY",
            "applyVat":false,"recipients":[
              {"id":"r1","address":"제주특별자치도 제주시 연동 1",
                "freightCode":"010","dispatchOrigin":"FACTORY",
                "shippingCharges":{"FACTORY":3000},
                "remoteDelivery":{"address":"제주특별자치도 제주시 연동 1",
                    "checked":true,"manual":true},
                "vatOverride":true,
                "items":[{"id":"i1","productId":"p","variantId":"v",
                    "quantity":4,"unitPrice":59000,"manualUnitPrice":59000,
                    "origin":"FACTORY"}]},
              {"id":"r2","address":"서울특별시 중구 세종대로 1",
                "freightCode":"010","dispatchOrigin":"STORE",
                "shippingCharges":{"STORE":2500},
                "vatOverride":false,
                "items":[{"id":"i2","productId":"p","variantId":"v",
                    "quantity":1,"unitPrice":59000,"manualUnitPrice":59000,
                    "origin":"STORE"}]}
            ]});
        let client=json!({"id":"c","tier":"wholesale"});
        let quote=reception_quote(&json!({"order":order,"state":state,"client":client}))
            .unwrap();
        assert_eq!(quote["linePrices"][0]["amount"],236000);
        assert_eq!(quote["linePrices"][1]["amount"],59000);
        assert_eq!(quote["totals"]["subtotal"],295000);
        assert_eq!(quote["totals"]["shipping"],8500);
        assert_eq!(quote["totals"]["vat"],24200);
        assert_eq!(quote["totals"]["total"],327700);
        assert_eq!(quote["totals"]["pendingRecipients"],json!([]));
    }
    #[test]
    fn seven_point_five_percent_applies_only_to_selected_shipment(){
        let order=json!({"applyVat":false,"recipients":[
            {"id":"a","vatOverride":true,"vatRateBps":750,"items":[
                {"productId":"p","variantId":"v","unitPrice":10000,"quantity":1}]},
            {"id":"b","vatOverride":false,"vatRateBps":750,"items":[
                {"productId":"p","variantId":"v","unitPrice":10000,"quantity":1}]}
        ]});
        let freight=json!({"bundles":[
            {"recipientId":"a","billableCharge":4000},
            {"recipientId":"b","billableCharge":3000}]});
        let total=reception_totals(&order,&json!({}),&freight).unwrap();
        assert_eq!(total["vat"],1050);
        assert_eq!(total["total"],28050);
    }
    #[test]
    fn incomplete_second_recipient_does_not_show_false_final_total(){
        let order=json!({"applyVat":false,"recipients":[
            {"id":"r1","items":[{"productId":"p","variantId":"v",
                "quantity":4,"unitPrice":59000}]},
            {"id":"r2","items":[{"productId":"p","variantId":"v",
                "quantity":1,"unitPrice":59000}]}
        ]});
        let freight=json!({"bundles":[
            {"recipientId":"r1","billableCharge":6000},
            {"recipientId":"r2","billableCharge":null}
        ]});
        let amounts=reception_totals(&order,&json!({"id":"c"}),&freight).unwrap();
        assert_eq!(amounts["subtotal"],295000);
        assert_eq!(amounts["shipping"],Value::Null);
        assert_eq!(amounts["total"],Value::Null);
        assert_eq!(amounts["pendingRecipients"],json!([{
            "recipientId":"r2","product":false,"freight":true,"vat":false
        }]));
        // When the final shipping charge becomes known, the same Rust quote
        // returns the complete order total automatically.
        let completed=json!({"bundles":[
            {"recipientId":"r1","billableCharge":6000},
            {"recipientId":"r2","billableCharge":2500}
        ]});
        let complete=reception_totals(&order,&json!({"id":"c"}),&completed).unwrap();
        assert_eq!(complete["total"],303500);
        assert_eq!(complete["pendingRecipients"],json!([]));
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
