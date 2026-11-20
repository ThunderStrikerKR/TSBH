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
    let indexed: HashMap<String, &Value> = catalog["products"].as_array()
        .map(Vec::as_slice).unwrap_or(&[])
        .iter()
        .filter_map(|row| {
            let id = row.get("id").and_then(Value::as_str).unwrap_or("");
            (!id.is_empty()).then_some((id.to_owned(), row))
        })
        .collect();
    let mut products = Vec::with_capacity(ids.len());
    for id in ids {
        let key = id.as_str().ok_or("Rust 상품 ID 형식 오류")?;
        let item = indexed.get(key)
            .ok_or("Rust 상품 조회 결과에 존재하지 않는 상품 ID가 있습니다.")?;
        products.push((*item).clone());
    }
    Ok(ManagementProductRows { catalog, products })
}
