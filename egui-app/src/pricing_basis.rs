//! Source-scoped *inputs* to the native pricing and freight rules.
//! This records dependency values only. No monetary calculation occurs here.
//! Unrelated client/product/shipping edits must not invalidate an old quote.

use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

fn value<'a>(row:&'a Value,key:&str)->&'a Value{
    row.get(key).unwrap_or(&Value::Null)
}
fn text<'a>(row:&'a Value,key:&str)->&'a str{
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(row:&'a Value,key:&str)->&'a [Value]{
    row.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn rule_inputs(rule:&Value)->Value{
    json!({
        "version":value(rule,"version"),"fees":value(rule,"fees"),
        "perItem":value(rule,"perItem"),
        "maxBundle":value(rule,"maxBundle"),
        "maxConfirmed":value(rule,"maxConfirmed")
    })
}
/// Retain only the minimal rows used to compute the native quote basis.
/// These are from the quoted Rust Order, not a pre-client OCR projection.
/// Never store the recipient name, address, phone or full product master here.
pub fn order_inputs(order:&Value)->Value{
    json!({"recipients":list(order,"recipients").iter().map(|row|json!({
        "items":list(row,"items").iter().map(|item|json!({
            "productId":value(item,"productId"),
            "variantId":value(item,"variantId"),
            "manualUnitPrice":value(item,"manualUnitPrice"),
            "origin":value(item,"origin"),
            "quantity":value(item,"quantity")
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>()})
}
/// Catalog dependency projection for the selected dealer, actual line variants
/// and the shipment combinations used by THIS draft. The client's contact
/// information, other products, other customers and unrelated freight groups
/// never enter this state signature.
pub fn snapshot(state:&Value,client:&Value,order:&Value)->Value{
    let tier=text(client,"tier");
    let products=list(state,"products");
    let shipping=value(state,"shipping");
    let mut used=BTreeMap::<String,BTreeSet<String>>::new();
    let mut combination_keys=BTreeSet::<String>::new();
    let mut catalog_priced=BTreeSet::<(String,String)>::new();
    for recipient in list(order,"recipients"){
        for origin in ["FACTORY","STORE"]{
            let mut grouped=BTreeMap::<String,i64>::new();
            for item in list(recipient,"items"){
                let pid=text(item,"productId");
                let vid=text(item,"variantId");
                if pid.is_empty() || vid.is_empty(){continue;}
                used.entry(pid.to_owned()).or_default().insert(vid.to_owned());
                if !matches!(crate::gateway::rules::manual_amount(value(item,"manualUnitPrice")),Ok(Some(_))){
                    catalog_priced.insert((pid.into(),vid.into()));
                }
                if text(item,"origin")!=origin{continue;}
                let key=format!("{pid}@{vid}");
                let count=item.get("quantity").and_then(Value::as_i64).unwrap_or(0);
                // Actual invalid/overflow quantities are rejected by native
                // pricing; a missing key here only forces a later refresh.
                let next=grouped.get(&key).copied().unwrap_or(0)
                    .saturating_add(count);
                grouped.insert(key,next);
            }
            if !grouped.is_empty(){
                combination_keys.insert(format!("{}|{}",origin,grouped.iter()
                    .map(|(key,count)|format!("{key}:{count}"))
                    .collect::<Vec<_>>().join("|")));
            }
        }
    }
    let mut chosen_groups=BTreeSet::<String>::new();
    let mut chosen_products=Vec::new();
    for (pid,variants) in used {
        let product=products.iter().find(|p|text(p,"id")==pid);
        let group=product.map(|p|text(p,"shippingGroupId"))
            .unwrap_or("").to_owned();
        if !group.is_empty(){chosen_groups.insert(group.clone());}
        let origin=product.map(|p|value(p,"origin")).unwrap_or(&Value::Null);
        let mut priced_variants=Vec::new();
        for vid in variants {
            let variant=product.and_then(|p|list(p,"variants").iter()
                .find(|v|text(v,"id")==vid));
            // Mixed manual/automatic lines sharing one variant must still
            // invalidate on changes to the catalog-priced line.
            let manual=!catalog_priced.contains(&(pid.clone(),vid.clone()));
            let aliases=variant.map(|v|value(v,"legacyIds")).unwrap_or(&Value::Null);
            let override_prices=if tier=="special" && !manual {
                let mut map=serde_json::Map::new();
                map.insert(vid.clone(),value(value(client,"overrides"),&vid).clone());
                for legacy in aliases.as_array().into_iter().flatten(){
                    if let Some(legacy)=legacy.as_str(){
                        map.insert(legacy.into(),
                            value(value(client,"overrides"),legacy).clone());
                    }
                }
                Value::Object(map)
            }else{Value::Null};
            priced_variants.push(json!({
                "variantId":vid,"manual":manual,
                "tierPrice":if manual{Value::Null}else{
                    variant.map(|v|value(v,tier).clone()).unwrap_or(Value::Null)
                },
                "wholesaleFallback":if tier=="special" && !manual {
                    variant.map(|v|value(v,"wholesale").clone()).unwrap_or(Value::Null)
                }else{Value::Null},
                "legacyIds":if tier=="special" && !manual {aliases.clone()}
                    else{Value::Null},
                "overrides":override_prices
            }));
        }
        let group_is_active=list(shipping,"groups").iter().any(|record|
            text(record,"id")==group &&
            record.get("deletedAt").is_none_or(Value::is_null)
        );
        let standalone_rule=if !group_is_active{
            value(value(shipping,"productRules"),&pid)
        }else{&Value::Null};
        chosen_products.push(json!({
            "id":pid,"origin":origin,"shippingGroupId":group,
            "productRule":rule_inputs(standalone_rule),
            "variants":priced_variants
        }));
    }
    let mut relevant_groups=Vec::new();
    for group_id in chosen_groups{
        let group=list(shipping,"groups").iter()
            .find(|g|text(g,"id")==group_id);
        relevant_groups.push(json!({
            "id":group_id,
            "deletedAt":group.map(|g|value(g,"deletedAt")).unwrap_or(&Value::Null),
            "current":rule_inputs(group.map(|g|value(g,"current")).unwrap_or(&Value::Null))
        }));
    }
    let combinations=list(shipping,"combinations").iter()
        .filter(|row|combination_keys.contains(text(row,"key")))
        .map(|row|json!({
            "id":value(row,"id"),"key":value(row,"key"),
            "active":value(row,"active"),"epoch":value(row,"epoch"),
            "versions":value(row,"versions"),"charge":value(row,"charge"),
            "at":value(row,"at")
        })).collect::<Vec<_>>();
    json!({
        // Invalidate quotes calculated before the freight billing reason
        // and client exemption were represented consistently per shipment.
        "schema":4,
        "orderInputs":order_inputs(order),
        "client":{
            "id":value(client,"id"),"tier":value(client,"tier"),
            "shippingFeeExcluded":value(client,"shippingFeeExcluded"),
            "defaultApplyVat":value(client,"defaultApplyVat"),
            "active":value(client,"active"),"deletedAt":value(client,"deletedAt")
        },
        "products":chosen_products,"groups":relevant_groups,
        "combinationEpoch":if combinations.is_empty(){Value::Null}
            else{value(shipping,"epoch").clone()},
        "combinations":combinations
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn unrelated_master_changes_do_not_invalidate_receipt_basis(){
        let state=json!({
            "products":[
                {"id":"p1","origin":"FACTORY","shippingGroupId":"g1",
                    "variants":[{"id":"v1","wholesale":12000}]},
                {"id":"p2","origin":"STORE","shippingGroupId":"g2",
                    "variants":[{"id":"v2","wholesale":6000}]}
            ],
            "shipping":{"epoch":1,"productRules":{},"combinations":[],
                "groups":[
                    {"id":"g1","current":{"version":"a","fees":{"1":3000}}},
                    {"id":"g2","current":{"version":"b","fees":{"1":5000}}}
                ]}
        });
        let client=json!({"id":"c1","tier":"wholesale","shippingFeeExcluded":false});
        let order=json!({"recipients":[{"id":"r1","items":[{
            "id":"i1","productId":"p1","variantId":"v1",
            "origin":"FACTORY","quantity":1
        }]}]});
        let saved=snapshot(&state,&client,&order);
        let mut unrelated=state.clone();
        unrelated["products"][1]["variants"][0]["wholesale"]=json!(9999);
        unrelated["shipping"]["groups"][1]["current"]["fees"]["1"]=json!(7000);
        unrelated["shipping"]["epoch"]=json!(2);
        assert_eq!(saved,snapshot(&unrelated,&client,&order));
        let mut changed=state.clone();
        changed["products"][0]["variants"][0]["wholesale"]=json!(13000);
        assert_ne!(saved,snapshot(&changed,&client,&order));
        let mut freight=state.clone();
        freight["shipping"]["groups"][0]["current"]["fees"]["1"]=json!(4500);
        assert_ne!(saved,snapshot(&freight,&client,&order));
    }

    #[test]
    fn explicit_unit_price_does_not_depend_on_changed_catalog_price(){
        let mut state=json!({"products":[{"id":"p",
            "variants":[{"id":"v","wholesale":10000}]}]});
        let client=json!({"id":"c","tier":"wholesale"});
        let order=json!({"recipients":[{"items":[{
            "productId":"p","variantId":"v","quantity":2,
            "manualUnitPrice":8500,"origin":"FACTORY"
        }]}]});
        let saved=snapshot(&state,&client,&order);
        state["products"][0]["variants"][0]["wholesale"]=json!(14000);
        assert_eq!(saved,snapshot(&state,&client,&order));
    }
}
