//! Transport boundary for the egui UI.
//!
//! Views and reception services depend on BandoGateway only. They must not
//! import native Rust modules, reqwest, Tauri, or a database connection.
//! Local and future HTTPS adapters preserve the same JSON business contract.
//!
//! Business operations remain an explicit read-only allowlist. The gateway
//! also saves *one preview editor source* with revision checking; this is not
//! a production order nor a confirmation workflow. No HTTP->local fallback.

use serde_json::Value;
use std::sync::Arc;
use crate::preview_store::{PreviewStore, WorkingSource};

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
    /// A minimal catalog snapshot (products, aliases, brands). No orders,
    /// clients, workflow or other persisted operational state.
    /// This method does not persist or mutate anything.
    fn load_catalog(&self) -> Result<Value, String>;
    /// The currently open, unconfirmed source in a separate storage domain.
    /// This never creates a confirmed business order.
    fn load_working_sources(&self) -> Result<Vec<WorkingSource>, String>;
    /// Insert a first-class, independent reception item.
    fn create_working_source(&self) -> Result<WorkingSource, String>;
    /// Conditional write: another process may not overwrite a newer source.
    fn save_working_source(&self, id: i64, raw: &str, revision: u64) -> Result<u64, String>;
    /// Execute one of the explicitly allowed read-only business operations.
    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String>;
}

#[derive(Default)]
pub struct LocalGateway;

impl BandoGateway for LocalGateway {
    fn label(&self) -> &'static str {
        "로컬 Rust"
    }

    fn load_catalog(&self) -> Result<Value, String> {
        PreviewStore::initialize()?.load_catalog()
    }

    fn load_working_sources(&self) -> Result<Vec<WorkingSource>, String> {
        PreviewStore::initialize()?.load_sources()
    }

    fn create_working_source(&self) -> Result<WorkingSource, String> {
        PreviewStore::initialize()?.create_source()
    }

    fn save_working_source(&self, id: i64, raw: &str, revision: u64) -> Result<u64, String> {
        PreviewStore::initialize()?.save_source(id, raw, revision)
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
        "local" => {
            // Fail early if the isolated DB cannot be initialized or opened.
            // Never attempt to recover by opening a Tauri/production DB.
            PreviewStore::initialize()?;
            Ok(Arc::new(LocalGateway))
        },
        "http" => Ok(Arc::new(crate::http_gateway::HttpGateway::from_environment()?)),
        _ => Err("BANDO_HUB_GATEWAY는 local 또는 http여야 합니다.".into()),
    }
}
