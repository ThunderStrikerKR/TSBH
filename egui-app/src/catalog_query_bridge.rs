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

pub fn search_products(payload:&Value)->Result<Value,String>{
    native_queries::execute("search_products_with_urls",payload)
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
