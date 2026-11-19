//! Transport boundary for the egui UI.
//!
//! Views and reception services depend on BandoGateway only. They must not
//! import native Rust modules, reqwest, Tauri, or a database connection.
//! Local and future HTTPS adapters preserve the same JSON business contract.
//!
//! Business operations remain an explicit read-only allowlist. The gateway
//! saves a versioned *preview-only multi-reception workspace*; this never
//! confirms a production order. No HTTP->local fallback.

use serde_json::Value;
use std::sync::Arc;
use crate::preview_store::{PreviewStore, WorkingSource};

#[path = "../../src-tauri/src/business_core/recognition/mod.rs"]
mod recognition;
#[path = "../../src-tauri/src/business_core/rules/mod.rs"]
mod rules;
#[path = "../../src-tauri/src/business_core/catalog_clients/title_clients.rs"]
mod native_title_clients;
#[path = "../../src-tauri/src/business_core/catalog_clients/client_search.rs"]
mod native_client_search;

#[derive(Clone, Copy, Debug)]
pub enum ReadOperation {
    AnalyzeOrder,
    ResolveRetailUnitPrice,
    DetectRemoteDeliveryArea,
    AutoMatchReceptionClient,
    ReceptionClientCandidates,
    ClientCandidates,
    NewReception,
    MergeReceptionAnalysis,
    ChangeOrderClient,
    RepriceOrder,
    CalculatePrice,
}

impl ReadOperation {
    pub fn domain(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "recognition",
            Self::ResolveRetailUnitPrice | Self::DetectRemoteDeliveryArea => "rules",
            Self::AutoMatchReceptionClient | Self::ReceptionClientCandidates
            | Self::ClientCandidates => "catalog_clients",
            Self::NewReception | Self::MergeReceptionAnalysis
            | Self::ChangeOrderClient | Self::RepriceOrder
            | Self::CalculatePrice => "rules",
        }
    }

    pub fn operation(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "analyze_order",
            Self::ResolveRetailUnitPrice => "resolve_unit_price",
            Self::DetectRemoteDeliveryArea => "detect_remote_delivery_area",
            Self::AutoMatchReceptionClient => "auto_match_reception_client",
            Self::ReceptionClientCandidates => "reception_client_candidates",
            Self::ClientCandidates => "client_candidates",
            Self::NewReception => "apply_reception_action",
            Self::MergeReceptionAnalysis => "merge_reception_analysis",
            Self::ChangeOrderClient => "order_edit_rules",
            Self::RepriceOrder => "reprice_order",
            Self::CalculatePrice => "calculate_price",
        }
    }
}

pub trait BandoGateway: Send + Sync {
    fn label(&self) -> &'static str;
    /// A minimal catalog snapshot (products, aliases, brands), plus an
    /// explicitly configured name-only client index. No operational database,
    /// confirmed orders, customer contacts or workflow state.
    /// This method does not persist or mutate anything.
    fn load_catalog(&self) -> Result<Value, String>;
    /// Full v2 business snapshot for native rules only, from an explicitly
    /// configured read-only operational DB. This must never mutate SQLite.
    fn load_business_state(&self) -> Result<Value, String> {
        Err("서버에는 실제 주문 수정 미리보기 계약이 아직 없습니다.".into())
    }
    /// The currently open, unconfirmed source in a separate storage domain.
    /// This never creates a confirmed business order.
    fn load_working_source(&self) -> Result<WorkingSource, String>;
    /// Conditional write: another process may not overwrite a newer source.
    fn save_working_source(&self, source: &WorkingSource) -> Result<u64, String>;
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
        // When a live master DB is explicitly selected, recognition must use
        // the same product IDs, variants and aliases as the native price
        // engine. Never parse against a possibly stale embedded seed and
        // calculate prices against a different production catalog.
        if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|value| !value.is_empty())
        {
            let full = crate::business_state::load()?;
            let (clients, settings) = crate::client_directory::project_index(&full)?;
            return Ok(serde_json::json!({
                "products":full.get("products"),
                "aliases":full.get("aliases"),
                "brands":full.get("brands"),
                "categories":full.get("categories"),
                "clients":clients,
                "settings":settings,
            }));
        }
        crate::client_directory::attach(PreviewStore::initialize()?.load_catalog()?)
    }
    fn load_business_state(&self) -> Result<Value, String> {
        crate::business_state::load()
    }

    fn load_working_source(&self) -> Result<WorkingSource, String> {
        PreviewStore::initialize()?.load_source()
    }

    fn save_working_source(&self, source: &WorkingSource) -> Result<u64, String> {
        PreviewStore::initialize()?.save_source(source)
    }

    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String> {
        match operation {
            ReadOperation::AnalyzeOrder => {
                recognition::execute(operation.operation(), payload)
            }
            ReadOperation::ResolveRetailUnitPrice
            | ReadOperation::DetectRemoteDeliveryArea
            | ReadOperation::NewReception
            | ReadOperation::MergeReceptionAnalysis
            | ReadOperation::ChangeOrderClient
            | ReadOperation::RepriceOrder
            | ReadOperation::CalculatePrice => {
                rules::execute(operation.operation(), payload)
            }
            ReadOperation::AutoMatchReceptionClient
            | ReadOperation::ReceptionClientCandidates => {
                native_title_clients::execute(operation.operation(), &payload)
            }
            ReadOperation::ClientCandidates => {
                native_client_search::execute(&payload)
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
