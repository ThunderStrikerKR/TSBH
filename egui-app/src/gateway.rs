//! Transport boundary for the egui UI.
//!
//! Views and reception services depend on BandoGateway only. They must not
//! import native Rust modules, reqwest, Tauri, or a database connection.
//! Local and future HTTPS adapters preserve the same JSON business contract.
//!
//! This preview explicitly allows ONLY the read-only operations enumerated
//! here; it has no order-saving API. There is never a silent HTTP->local
//! fallback.

use serde_json::Value;
use std::sync::Arc;

#[path = "../../src-tauri/src/business_core/recognition/mod.rs"]
mod recognition;
#[path = "../../src-tauri/src/business_core/rules/mod.rs"]
mod rules;

#[derive(Clone, Copy, Debug)]
pub enum ReadOperation {
    AnalyzeOrder,
    ResolveRetailUnitPrice,
    DetectRemoteDeliveryArea,
}

impl ReadOperation {
    pub fn domain(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "recognition",
            Self::ResolveRetailUnitPrice | Self::DetectRemoteDeliveryArea => "rules",
        }
    }

    pub fn operation(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "analyze_order",
            Self::ResolveRetailUnitPrice => "resolve_unit_price",
            Self::DetectRemoteDeliveryArea => "detect_remote_delivery_area",
        }
    }
}

pub trait BandoGateway: Send + Sync {
    fn label(&self) -> &'static str;
    /// A snapshot used only for read-only analysis in this preview.
    /// This method does not persist or mutate anything.
    fn load_state(&self) -> Result<Value, String>;
    /// Execute one of the explicitly allowed read-only business operations.
    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String>;
}

#[derive(Default)]
pub struct LocalGateway;

impl BandoGateway for LocalGateway {
    fn label(&self) -> &'static str {
        "로컬 Rust"
    }

    fn load_state(&self) -> Result<Value, String> {
        // This embedded source is NOT the live staff/client database.
        static SEED: std::sync::OnceLock<Result<Value, String>> =
            std::sync::OnceLock::new();
        SEED.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../src-tauri/resources/state-seed.json"
            )).map_err(|error| format!("기본 상품 데이터 오류: {error}"))
        }).as_ref().map(Clone::clone).map_err(Clone::clone)
    }

    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String> {
        match operation {
            ReadOperation::AnalyzeOrder => {
                recognition::execute(operation.operation(), payload)
            }
            ReadOperation::ResolveRetailUnitPrice
            | ReadOperation::DetectRemoteDeliveryArea => {
                rules::execute(operation.operation(), payload)
            }
        }
    }
}

/// Build exactly the explicitly selected backend. Invalid remote config is an
/// error, not permission to fall back and silently process locally.
pub fn from_environment() -> Result<Arc<dyn BandoGateway>, String> {
    let mode = std::env::var("BANDO_HUB_GATEWAY")
        .unwrap_or_else(|_| "local".to_owned());
    match mode.trim().to_ascii_lowercase().as_str() {
        "local" => Ok(Arc::new(LocalGateway)),
        "http" => Ok(Arc::new(crate::http_gateway::HttpGateway::from_environment()?)),
        _ => Err("BANDO_HUB_GATEWAY는 local 또는 http여야 합니다.".into()),
    }
}
