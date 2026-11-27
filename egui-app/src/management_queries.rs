//! Read-only management projections shared by the egui product and freight
//! screens. This layer composes the existing authoritative Rust business
//! operations; it never duplicates pricing/shipping rules or accesses SQLite.
//!
//! The gateway owns transport and operational data selection. Views own only
//! screen state and rendering. Both initial freight loads and post-save reads
//! use exactly the same projection composition.

use crate::gateway::{BandoGateway, ReadOperation};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ManagementProductRows {
    pub catalog: Value,
    pub products: Vec<Value>,
}

pub struct ManualProductRows {
    pub catalog: Value,
    pub products: Vec<Value>,
    pub detail_ids: Vec<String>,
}

/// Join native Rust query IDs with existing catalog records. The two UI
/// consumers historically differ only in the maximum result count and
/// which duplicate row wins; keep those legacy choices unchanged.
fn product_rows_by_ids(
    catalog_rows: &[Value],
    ids: &[Value],
    first_wins: bool,
    limit: usize,
    invalid_id_error: &'static str,
    missing_id_error: &'static str,
) -> Result<Vec<Value>, String> {
    let mut indexed: HashMap<String, &Value> = HashMap::new();
    for row in catalog_rows {
        let id = row.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if first_wins {
            indexed.entry(id.to_owned()).or_insert(row);
        } else {
            indexed.insert(id.to_owned(), row);
        }
    }
    let mut products = Vec::with_capacity(ids.len().min(limit));
    for value in ids.iter().take(limit) {
        let id = value.as_str().ok_or(invalid_id_error)?;
        let row = indexed.get(id).ok_or(missing_id_error)?;
        products.push((*row).clone());
    }
    Ok(products)
}

/// One load and one pair of native Rust freight projections. This does not
/// compute or infer fees, bundles, shipping history or effective product rules.
pub fn load_freight(gateway: &dyn BandoGateway) -> Result<(Value, Value), String> {
    let state = gateway.load_business_state()?;
    let projection = project_freight(gateway, &state)?;
    Ok((state, projection))
}

/// Reused after an authoritative shipping write. The caller supplies the
/// revision-verified state; do not re-read another revision or use UI drafts.
pub fn project_freight(
    gateway: &dyn BandoGateway,
    state: &Value,
) -> Result<Value, String> {
    let shipping = state.get("shipping")
        .filter(|value| value.is_object())
        .ok_or("운영 DB에서 운임 기준을 찾을 수 없습니다.")?;
    let mut projection = gateway.execute(
        ReadOperation::ShippingManagerView,
        json!({"state":state,"shipping":shipping}),
    )?;
    let product_view = gateway.execute(
        ReadOperation::ProductFreightView,
        json!({"action":"product_freight_view","state":{
            "products":state["products"],"shipping":shipping
        }}),
    )?;
    projection["productViews"] = product_view["views"].clone();
    Ok(projection)
}

/// The catalog and selected products use the same native management_products
/// query as the existing Tauri business core. IDs are resolved without any
/// second egui search/filter/sort decision.
pub fn query_management_products(
    gateway: &dyn BandoGateway,
    cached: Option<Value>,
    search: &str,
    category: &str,
    brand: &str,
    missing: bool,
) -> Result<ManagementProductRows, String> {
    let catalog = match cached {
        Some(value) => value,
        None => gateway.load_catalog()?,
    };
    let ids = gateway.execute(ReadOperation::ManagementProducts, json!({
        "state":&catalog, "query":search, "category":category,
        "brand":brand, "missing":missing,
    }))?;
    let ids = ids.as_array()
        .ok_or("Rust 상품관리 조회 결과가 ID 배열이 아닙니다.")?;
    let products = product_rows_by_ids(
        catalog["products"].as_array().map(Vec::as_slice).unwrap_or(&[]),
        ids, false, usize::MAX,
        "Rust 상품 ID 형식 오류",
        "Rust 상품 조회 결과에 존재하지 않는 상품 ID가 있습니다.",
    )?;
    Ok(ManagementProductRows { catalog, products })
}

/// Reuse the authoritative native catalog search for the manual order chooser.
/// The 100-item cap and detail-link IDs are presentation requirements, not
/// independent product eligibility or sorting rules.
pub fn query_manual_products(
    gateway: &dyn BandoGateway,
    cached: Option<Value>,
    query: &str,
    client_id: &str,
    major: &str,
    brand: &str,
) -> Result<ManualProductRows, String> {
    query_product_picker(gateway,cached,query,client_id,major,brand,None,&Default::default())
}

pub(crate) fn query_reference_products(
    gateway:&dyn BandoGateway,cached:Option<Value>,query:&str,major:&str,brand:&str,
    eligible:Option<&std::collections::HashSet<String>>,excluded:&std::collections::HashSet<String>,
)->Result<ManualProductRows,String>{
    query_product_picker(gateway,cached,query,"",major,brand,eligible,excluded)
}

fn query_product_picker(
    gateway:&dyn BandoGateway,cached:Option<Value>,query:&str,client_id:&str,major:&str,brand:&str,
    eligible:Option<&std::collections::HashSet<String>>,excluded:&std::collections::HashSet<String>,
)->Result<ManualProductRows,String>{
    let catalog = match cached {
        Some(value) => value,
        None => gateway.load_catalog()?,
    };
    let result = gateway.execute(ReadOperation::SearchProductsWithUrls, json!({
        "state":&catalog,"query":query,"clientId":client_id,
        "major":major,"brand":brand,"excludeDemo":true
    }))?;
    let ids = result.get("ids").and_then(Value::as_array)
        .ok_or("Rust 상품 검색 결과 형식 오류")?;
    let all = catalog.get("products").and_then(Value::as_array)
        .ok_or("상품 목록이 없습니다.")?;
    let filtered=ids.iter().filter(|value|value.as_str().is_none_or(|id|
        eligible.is_none_or(|allowed|allowed.contains(id)) && !excluded.contains(id)
    )).cloned().collect::<Vec<_>>();
    let products = product_rows_by_ids(
        all, &filtered, true, 100,
        "Rust 상품 검색 ID 오류",
        "상품 ID와 목록이 일치하지 않습니다.",
    )?;
    let detail_ids = result.get("detailLinkIds")
        .and_then(Value::as_array).into_iter().flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    Ok(ManualProductRows { catalog, products, detail_ids })
}
