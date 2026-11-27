//! Reuse the actual native catalog query source with no UI-owned filtering.
//! This wrapper supplies only the existing parent-module helpers needed by
//! catalog_clients/queries.rs. The query/ordering algorithm is NOT copied.

use serde_json::{Value,json};
fn field<'a>(value:&'a Value,key:&str)->&'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn items<'a>(value:&'a Value,key:&str)->&'a [Value] {
    value.get(key).and_then(Value::as_array)
        .map(Vec::as_slice).unwrap_or(&[])
}
fn product_url_ok(value:&str)->bool {
    let Ok(url)=reqwest::Url::parse(value) else{return false};
    url.scheme()=="https" &&
        ["bandostand.com","www.bandostand.com",
         "bandomall.co.kr","www.bandomall.co.kr"]
            .contains(&url.host_str().unwrap_or("")) &&
        url.path().starts_with("/product/")
}
#[path = "../../src-tauri/src/business_core/catalog_clients/client_search.rs"]
mod client_search;
#[path = "../../src-tauri/src/business_core/catalog_clients/queries.rs"]
mod native_queries;
#[path = "../../src-tauri/src/business_core/catalog_clients/model_groups.rs"]
mod native_model_groups;
#[path = "../../src-tauri/src/business_core/catalog_clients/aliases.rs"]
mod native_aliases;

/// Render the exact Rust alias diagnostics from the original React query.
/// No second duplicate, color-suffix or model-conflict heuristic in egui.
pub fn alias_problems(catalog:&Value)->Result<Value,String>{
    native_aliases::execute("alias_problems",&json!({"state":catalog}))
}


/// The exact Rust catalog_clients validator is also the pre-backup gate.
/// Return only product IDs the existing model-family Rust validator accepts.
/// The egui candidate list must not duplicate active/demo/sale-unit rules.
pub fn eligible_model_group_products(catalog:&Value)->Result<Value,String>{
    native_model_groups::execute("eligible_model_group_product_ids",
        &json!({"state":catalog}))
}

pub fn validate_model_groups(catalog:&Value,groups:&Value)->Result<(),String>{
    let payload=json!({
        "state":{
            "products":catalog.get("products").ok_or("상품 목록을 확인할 수 없습니다.")?,
            "settings":{"colorGuidanceGroups":groups}
        }
    });
    native_model_groups::execute("validate_model_groups",&payload)?;
    Ok(())
}

/// Reuse the client-search module already loaded by native catalog queries.
/// The gateway must not compile another independent copy of the same rule.
pub fn client_candidates(payload:&Value)->Result<Value,String>{
    client_search::execute(payload)
}

pub fn search_products(payload:&Value)->Result<Value,String>{
    native_queries::execute("search_products_with_urls",payload)
}

/// Use React ProductManager's exact Rust query, not a second egui filter.
/// The IDs are joined back to catalog rows for display only.
pub fn management_products(payload:&Value)->Result<Value,String>{
    native_queries::execute("management_products",payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_catalog_search_respects_alias_and_demo() {
        let state=json!({
            "products":[
                {"id":"one","name":"BD 709","active":true,"catalog":"BANDOSTAND",
                 "variants":[{"id":"v1","source":"OFFICIAL",
                   "url":"https://bandostand.com/product/bd-709"}]},
                {"id":"two","name":"BD 708","active":true,"catalog":"DEMO",
                 "variants":[{"id":"v2","source":"DEMO","url":""}]}
            ],
            "aliases":[{"productId":"one","clientId":null,"text":"보면대"}],
            "categories":[],"brands":[]
        });
        let a=search_products(&json!({"state":state,"query":"보면대",
            "excludeDemo":true})).unwrap();
        assert_eq!(a["ids"],json!(["one"]));
        assert_eq!(a["detailLinkIds"],json!(["one"]));
    }
}
