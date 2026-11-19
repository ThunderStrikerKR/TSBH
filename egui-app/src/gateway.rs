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
#[path = "../../src-tauri/src/business_core/output/mod.rs"]
mod output;
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
    ManualClassification,
    SearchProductsWithUrls,
    PreviewWorkflow,
    PreviewOrder,
    ReadShipments,
    RenderAutomaticMessage,
}

impl ReadOperation {
    pub fn domain(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "recognition",
            Self::ResolveRetailUnitPrice | Self::DetectRemoteDeliveryArea => "rules",
            Self::AutoMatchReceptionClient | Self::ReceptionClientCandidates
            | Self::ClientCandidates | Self::SearchProductsWithUrls => "catalog_clients",
            Self::NewReception | Self::MergeReceptionAnalysis
            | Self::ChangeOrderClient | Self::RepriceOrder
            | Self::CalculatePrice | Self::ManualClassification
            | Self::PreviewWorkflow | Self::PreviewOrder
            | Self::ReadShipments => "rules",
            Self::RenderAutomaticMessage => "output",
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
            Self::ManualClassification => "apply_manual_classification",
            Self::SearchProductsWithUrls => "search_products_with_urls",
            Self::PreviewWorkflow => "preview_workflow",
            Self::PreviewOrder => "preview_order",
            Self::ReadShipments => "read_shipments",
            Self::RenderAutomaticMessage => "render_automatic_message",
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
    /// Full client-management projection. An external operational reference
    /// is explicitly read-only; local edits stay in the isolated egui DB.
    fn load_client_master(&self) -> Result<Value, String> {
        Err("거래처 관리 조회를 지원하지 않는 연결입니다.".into())
    }
    fn mutate_client_master(&self,_revision:u64,_action:&str,_client:Value)
        -> Result<Value,String> {
        Err("이 연결은 거래처 변경 저장을 지원하지 않습니다.".into())
    }
    /// Full v2 business snapshot for native rules only, from an explicitly
    /// configured read-only operational DB. This must never mutate SQLite.
    fn load_business_state(&self) -> Result<Value, String> {
        Err("서버에는 실제 주문 수정 미리보기 계약이 아직 없습니다.".into())
    }
    /// A production working reception requires a separate, explicitly
    /// configured native Tauri executable and user action. Never send to an
    /// unimplemented HTTPS endpoint or silently fall back to direct SQL.
    fn working_commit_enabled(&self) -> bool { false }
    fn commit_working_reception(&self, _request: Value) -> Result<Value,String> {
        Err("운영 미확정 접수 저장 연결이 설정되지 않았습니다.".into())
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
        let mut catalog=PreviewStore::initialize()?.load_catalog()?;
        if crate::client_master::editable() {
            // The local draft database is the single owner of egui-managed
            // clients. Do not silently overlay an unrelated external index.
            let state=crate::client_master::load()?;
            let (clients,settings)=crate::client_directory::project_index(&state)?;
            catalog["clients"]=clients;
            catalog["settings"]=settings;
            Ok(catalog)
        } else {
            crate::client_directory::attach(catalog)
        }
    }
    fn load_client_master(&self) -> Result<Value,String> {
        crate::client_master::load()
    }
    fn mutate_client_master(&self,revision:u64,action:&str,client:Value)
        -> Result<Value,String> {
        crate::client_master::mutate(revision,action,client)
    }
    fn load_business_state(&self) -> Result<Value, String> {
        crate::business_state::load()
    }
    fn working_commit_enabled(&self) -> bool {
        std::env::var("BANDO_EGUI_ENABLE_WORKING_COMMIT").ok().as_deref()==Some("1") &&
        std::env::var_os("BANDO_EGUI_TAURI_HELPER").is_some_and(|p|!p.is_empty()) &&
        std::env::var_os("BANDO_EGUI_BUSINESS_DB").is_some_and(|p|!p.is_empty())
    }
    fn commit_working_reception(&self, request: Value) -> Result<Value,String> {
        use std::io::Write;
        use std::process::{Command,Stdio};
        if !self.working_commit_enabled() {
            return Err("운영 접수 저장은 Tauri helper, DB 경로 및 명시적 허용 설정이 필요합니다.".into());
        }
        let helper=std::env::var_os("BANDO_EGUI_TAURI_HELPER")
            .ok_or("Tauri 저장 도우미 경로가 없습니다.")?;
        let path=std::path::Path::new(&helper);
        if !path.is_file() {
            return Err("Tauri 저장 도우미 실행 파일을 찾을 수 없습니다.".into());
        }
        let request=serde_json::to_vec(&request)
            .map_err(|_|"운영 접수 요청 형식이 잘못되었습니다.".to_owned())?;
        if request.len()>2_000_000 {
            return Err("운영 접수 요청 한도를 초과했습니다.".into());
        }
        // No shell and no arbitrary command-line order/contact data.
        let mut child=Command::new(path)
            .arg("--bando-egui-reception-commit")
            .stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn().map_err(|_|"Tauri 주문 저장 도우미를 실행하지 못했습니다.".to_owned())?;
        let mut input=child.stdin.take()
            .ok_or("Tauri 주문 저장 도우미 입력을 열지 못했습니다.")?;
        input.write_all(&request)
            .map_err(|_|"Tauri 주문 저장 도우미에 접수 원문을 전달하지 못했습니다.".to_owned())?;
        drop(input);
        let output=child.wait_with_output()
            .map_err(|_|"Tauri 주문 저장 도우미의 결과를 받지 못했습니다.".to_owned())?;
        if output.stdout.len()>100_000 {
            return Err("Tauri 저장 결과 크기가 올바르지 않습니다.".into());
        }
        let result:Value=serde_json::from_slice(&output.stdout)
            .map_err(|_|"Tauri 주문 저장 도우미가 올바른 결과를 반환하지 않았습니다.".to_owned())?;
        if result.get("ok")!=Some(&Value::Bool(true)) || !output.status.success() {
            return Err(result.get("error").and_then(Value::as_str)
                .unwrap_or("Tauri 운영 접수 저장에 실패했습니다.").to_owned());
        }
        result.get("result").filter(|v|v.is_object()).cloned()
            .ok_or("Tauri 저장 결과에 미확정 접수 정보가 없습니다.".into())
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
            | ReadOperation::CalculatePrice
            | ReadOperation::ManualClassification
            | ReadOperation::PreviewWorkflow
            | ReadOperation::PreviewOrder
            | ReadOperation::ReadShipments => {
                rules::execute(operation.operation(), payload)
            }
            ReadOperation::AutoMatchReceptionClient
            | ReadOperation::ReceptionClientCandidates => {
                native_title_clients::execute(operation.operation(), &payload)
            }
            ReadOperation::ClientCandidates => {
                native_client_search::execute(&payload)
            }
            ReadOperation::SearchProductsWithUrls => {
                crate::catalog_query_bridge::search_products(&payload)
            }
            ReadOperation::RenderAutomaticMessage => {
                output::execute(operation.operation(),payload)
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
