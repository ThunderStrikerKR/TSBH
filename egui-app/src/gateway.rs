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
    ManagementProducts,
    PreviewWorkflow,
    PreviewOrder,
    ReadShipments,
    ShippingManagerView,
    ProductFreightView,
    DefaultProductRule,
    RenderAutomaticMessage,
}

impl ReadOperation {
    pub fn domain(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "recognition",
            Self::ResolveRetailUnitPrice | Self::DetectRemoteDeliveryArea => "rules",
            Self::AutoMatchReceptionClient | Self::ReceptionClientCandidates
            | Self::ClientCandidates | Self::SearchProductsWithUrls
            | Self::ManagementProducts => "catalog_clients",
            Self::NewReception | Self::MergeReceptionAnalysis
            | Self::ChangeOrderClient | Self::RepriceOrder
            | Self::CalculatePrice | Self::ManualClassification
            | Self::PreviewWorkflow | Self::PreviewOrder
            | Self::ReadShipments | Self::ShippingManagerView
            | Self::ProductFreightView | Self::DefaultProductRule => "rules",
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
            Self::ManagementProducts => "management_products",
            Self::PreviewWorkflow => "preview_workflow",
            Self::PreviewOrder => "preview_order",
            Self::ReadShipments => "read_shipments",
            Self::ShippingManagerView => "shipping_manager_view",
            Self::ProductFreightView => "apply_shipping_action",
            Self::DefaultProductRule => "apply_shipping_action",
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
    /// Native settings are separate from the order editor. The local adapter
    /// either persists in the isolated egui DB or explicitly delegates to
    /// Tauri's existing settings_patch transaction. HTTP is never fallback.
    fn load_settings(&self)->Result<Value,String>{
        Err("이 연결은 설정 조회를 지원하지 않습니다.".into())
    }
    fn patch_settings(&self,_changes:Value,_expected:Value)->Result<Value,String>{
        Err("이 연결은 설정 저장을 지원하지 않습니다.".into())
    }
    fn model_group_write_enabled(&self)->bool {false}
    fn patch_model_groups(&self,_groups:Value,_expected:Value)->Result<Value,String>{
        Err("이 연결에서는 모델군 저장을 지원하지 않습니다.".into())
    }
    fn settings_data(&self,_operation:&str,_filename:Option<&str>)->Result<Value,String>{
        Err("이 연결은 업무 감사 기록·백업을 지원하지 않습니다.".into())
    }
    /// Opt-in, revision-checked operational freight write through Tauri's
    /// existing Rust rule/SQLite transaction. Never writes the preview DB.
    fn shipping_write_enabled(&self) -> bool { false }
    fn mutate_shipping(&self,_revision:u64,_action:&str,_data:Value)
        -> Result<Value,String> {
        Err("이 연결에서는 운영 운임 변경을 지원하지 않습니다.".into())
    }
    /// Product changes require an explicitly enabled native Tauri helper.
    /// A read-only external master or the egui preview catalog is never a
    /// writable fallback.
    fn product_write_enabled(&self)->bool {false}
    fn mutate_product(&self,_revision:u64,_action:&str,_data:Value)
        -> Result<Value,String>{
        Err("이 연결에서는 운영 상품 변경을 지원하지 않습니다.".into())
    }
    /// Full v2 business snapshot for native rules only, from an explicitly
    /// configured read-only operational DB. This must never mutate SQLite.
    fn load_business_state(&self) -> Result<Value, String> {
        Err("서버에는 실제 주문 수정 미리보기 계약이 아직 없습니다.".into())
    }
    /// Explicit client ID selection may be persisted in the isolated egui
    /// receipt when no business DB is configured; it is NOT native pricing,
    /// freight, order commit or an alternative to the Tauri writer.
    fn isolated_client_identity_link(&self) -> bool { false }
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
            let (clients, mut settings) = crate::client_directory::project_index(&full)?;
            // Model-group settings are safe catalog metadata, not client
            // contacts. Retain the REAL settings field for native CAS rather
            // than the client-index-only projection.
            settings["colorGuidanceGroups"]=full["settings"]
                .get("colorGuidanceGroups").cloned().unwrap_or(Value::Null);
            return Ok(serde_json::json!({
                "revision":full.get("revision"),
                "products":full.get("products"),
                "aliases":full.get("aliases"),
                "brands":full.get("brands"),
                "categories":full.get("categories"),
                "shipping":full.get("shipping"),
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
    fn load_settings(&self)->Result<Value,String>{
        crate::settings_store::load()
    }
    fn patch_settings(&self,changes:Value,expected:Value)->Result<Value,String>{
        crate::settings_store::patch(changes,expected)
    }
    fn model_group_write_enabled(&self)->bool{
        crate::settings_store::writable_business()
    }
    fn patch_model_groups(&self,groups:Value,expected:Value)->Result<Value,String>{
        if !self.model_group_write_enabled(){
            return Err("모델군 설정 변경 권한이 없습니다.".into());
        }
        if !groups.is_object() {
            return Err("모델군 목록은 객체여야 합니다.".into());
        }
        let current=self.load_catalog()?;
        crate::catalog_query_bridge::validate_model_groups(&current,&groups)?;
        // React ColorGuidanceManager: full operating SQLite manual backup
        // BEFORE the per-field CAS settings_patch. Never bypass Tauri.
        self.settings_data("backup",None)?;
        self.patch_settings(
            serde_json::json!({"colorGuidanceGroups":groups}),
            serde_json::json!({"colorGuidanceGroups":expected})
        )?;
        let catalog=self.load_catalog()?;
        // An unrelated process may commit another group edit between
        // settings_patch and this read. A non-null key alone is NOT proof
        // that the editor's exact draft survived the SQLite round trip.
        if catalog["settings"]["colorGuidanceGroups"]!=groups{
            return Err("모델군 저장 후 최신 데이터가 일치하지 않습니다. 다시 불러온 뒤 확인하세요.".into());
        }
        Ok(catalog)
    }
    fn settings_data(&self,operation:&str,filename:Option<&str>)->Result<Value,String>{
        crate::settings_store::data(operation,filename)
    }
    fn load_business_state(&self) -> Result<Value, String> {
        crate::business_state::load()
    }
    fn isolated_client_identity_link(&self)->bool {
        std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_none_or(|value|value.is_empty())
    }
    fn product_write_enabled(&self)->bool{
        std::env::var("BANDO_EGUI_ENABLE_PRODUCT_WRITE")
            .ok().as_deref()==Some("1")
        && std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|path|!path.is_empty())
        && std::env::var_os("BANDO_EGUI_TAURI_HELPER")
            .is_some_and(|path|!path.is_empty())
    }
    fn mutate_product(&self,revision:u64,action:&str,data:Value)
        -> Result<Value,String>{
        use std::io::Write;
        use std::process::{Command,Stdio};
        if !self.product_write_enabled(){
            return Err("운영 상품을 수정할 수 있는 연결이 아닙니다.".into());
        }
        let helper=std::env::var_os("BANDO_EGUI_TAURI_HELPER")
            .ok_or("Tauri 상품 저장 도우미가 없습니다.")?;
        if !std::path::Path::new(&helper).is_file(){
            return Err("Tauri 상품 저장 도우미 실행 파일이 없습니다.".into());
        }
        let request=serde_json::json!({
            "expectedRevision":revision,"action":action,"data":data,
        }).to_string();
        if request.len()>2_000_000 {
            return Err("상품 변경 요청이 허용 크기를 초과했습니다.".into());
        }
        let mut child=Command::new(helper)
            .arg("--bando-egui-product-patch")
            .stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::null()).spawn()
            .map_err(|_|"Tauri 상품 저장 도우미 실행 실패".to_owned())?;
        let mut stdin=child.stdin.take()
            .ok_or("Tauri 상품 변경 입력을 열지 못했습니다.")?;
        stdin.write_all(request.as_bytes()).map_err(|e|e.to_string())?;
        drop(stdin);
        let output=child.wait_with_output().map_err(|e|e.to_string())?;
        if output.stdout.len()>65536 {
            return Err("상품 변경 응답이 허용 크기를 초과했습니다.".into());
        }
        let response:Value=serde_json::from_slice(&output.stdout)
            .map_err(|_|"Tauri 상품 변경 응답 형식 오류".to_owned())?;
        if !output.status.success() || response["ok"]!=true {
            return Err(response["error"].as_str()
                .unwrap_or("상품 변경을 저장하지 못했습니다.").to_owned());
        }
        let catalog=self.load_catalog()?;
        if catalog["revision"]!=response["result"]["revision"] {
            return Err("상품은 저장됐지만 최신 상태 조회가 일치하지 않습니다. 새로고침하세요.".into());
        }
        Ok(catalog)
    }
    fn shipping_write_enabled(&self) -> bool {
        std::env::var("BANDO_EGUI_ENABLE_SHIPPING_WRITE")
            .ok().as_deref()==Some("1") &&
        std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|v|!v.is_empty()) &&
        std::env::var_os("BANDO_EGUI_TAURI_HELPER")
            .is_some_and(|v|!v.is_empty())
    }
    fn mutate_shipping(&self,revision:u64,action:&str,data:Value)
        -> Result<Value,String> {
        use std::io::Write;
        use std::process::{Command,Stdio};
        if !self.shipping_write_enabled() {
            return Err("운영 운임을 변경할 수 있는 연결이 아닙니다.".into());
        }
        let path=std::env::var_os("BANDO_EGUI_TAURI_HELPER")
            .ok_or("Tauri 업무 저장 도우미가 없습니다.")?;
        if !std::path::Path::new(&path).is_file(){
            return Err("Tauri 업무 저장 도우미 파일이 없습니다.".into());
        }
        let request=serde_json::json!({
            "expectedRevision":revision,"action":action,"data":data
        }).to_string();
        if request.len()>2_000_000 {
            return Err("운임 변경 요청이 허용 크기를 초과했습니다.".into());
        }
        let mut child=Command::new(path)
            .arg("--bando-egui-shipping-patch")
            .stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn().map_err(|_|"운임 저장 도우미 실행 실패".to_owned())?;
        let mut stdin=child.stdin.take().ok_or("운임 변경 입력을 열지 못했습니다.")?;
        stdin.write_all(request.as_bytes()).map_err(|e|e.to_string())?;
        drop(stdin);
        let output=child.wait_with_output().map_err(|e|e.to_string())?;
        if output.stdout.len()>65536{
            return Err("운임 저장 응답이 허용 범위를 초과했습니다.".into());
        }
        let reply:Value=serde_json::from_slice(&output.stdout)
            .map_err(|_|"운임 저장 응답 형식 오류".to_owned())?;
        if !output.status.success() || reply["ok"]!=true {
            return Err(reply["error"].as_str()
                .unwrap_or("운임 변경을 저장하지 못했습니다.").to_owned());
        }
        // Re-read the authoritative state after commit, never optimistic UI
        // state or a second SQLite write performed by egui. Another writer
        // may commit between helper success and this read: a mismatched
        // revision must never be presented as our saved freight revision.
        let committed_revision=reply["result"]["revision"].as_u64()
            .ok_or("운임 저장 도우미가 확정 리비전을 반환하지 않았습니다.")?;
        let state=self.load_business_state()?;
        if state["revision"].as_u64()!=Some(committed_revision) {
            return Err(
                "운임은 저장됐지만 이후 다른 작업에서 데이터가 다시 수정되었습니다. 최신 상태를 불러와 확인하세요.".into()
            );
        }
        // Use the same authoritative projection as initial freight loads.
        let projection=crate::management_queries::project_freight(self,&state)?;
        Ok(serde_json::json!({"state":state,"projection":projection}))
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
            | ReadOperation::ReadShipments
            | ReadOperation::ShippingManagerView
            | ReadOperation::ProductFreightView
            | ReadOperation::DefaultProductRule => {
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
            ReadOperation::ManagementProducts => {
                crate::catalog_query_bridge::management_products(&payload)
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


#[cfg(test)]
mod shipping_gateway_process_roundtrip_tests {
    use super::{BandoGateway,LocalGateway};
    use serde_json::{json,Value};

    /// Real LocalGateway -> spawned Tauri CLI -> native shipping rule ->
    /// revision-checked v2 SQLite -> read-only egui re-query. No GUI and no
    /// mocks. CI supplies a newly created isolated test file and compiled CLI.
    #[test]
    fn shipping_gateway_process_roundtrip() {
        let db=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .expect("test fixture DB is explicitly required");
        let helper=std::env::var_os("BANDO_EGUI_TAURI_HELPER")
            .expect("compiled native Tauri helper is explicitly required");
        assert_eq!(
            std::path::Path::new(&db).file_name().and_then(|x|x.to_str()),
            Some("bando-freight-gateway-e2e.sqlite"),
            "refuse to run against any operational DB"
        );
        assert!(std::path::Path::new(&db).is_file());
        assert!(std::path::Path::new(&helper).is_file());
        let api=LocalGateway;
        assert!(api.shipping_write_enabled(),"native freight bridge must be enabled");
        let before=api.load_business_state().expect("egui reads initial native v2 state");
        let revision=before["revision"].as_u64().unwrap();
        let groups=before["shipping"]["groups"].as_array().unwrap();
        let group=groups.iter().find(|g|g["name"]=="격리 전체 경로 검증")
            .expect("real Rust shipping fixture group");
        let group_id=group["id"].as_str().unwrap().to_owned();
        let previous=group["current"].clone();
        let original_products=before["products"].clone();
        let original_orders=before["orders"].clone();
        let original_clients=before["clients"].clone();
        let staff_price=original_products[0]["variants"][0]["retail"].clone();
        assert!(!original_products[1]["deletedAt"].is_null(),
            "staff deletion marker must already exist in the fixture");
        let mut rule=previous.clone();
        rule["maxBundle"]=json!(2);
        rule["perItem"]=Value::Null;
        rule["fees"]=json!({"1":4321,"2":5678});
        rule["notes"]=json!("실제 Tauri CLI 왕복 운임 검증");
        let update=json!({"groupId":group_id,"rule":rule});
        let response=api.mutate_shipping(revision,"save_group_rule",update.clone())
            .expect("egui Gateway invokes native shipping CLI and reloads DB");
        assert_eq!(response["state"]["revision"],json!(revision+1));
        assert!(response["projection"]["productViews"].is_object());
        let after=api.load_business_state()
            .expect("read-only egui reload after subprocess commit");
        assert_eq!(response["state"],after,
            "gateway response must match authoritative persisted snapshot");
        assert_eq!(after["products"],original_products,
            "product edits/deletions must not change during shipping write");
        assert_eq!(after["orders"],original_orders);
        assert_eq!(after["clients"],original_clients);
        assert_eq!(after["products"][0]["variants"][0]["retail"],staff_price);
        assert!(!after["products"][1]["deletedAt"].is_null());
        let saved=after["shipping"]["groups"].as_array().unwrap()
            .iter().find(|g|g["id"]==group_id).unwrap();
        assert_eq!(saved["current"]["fees"]["1"],json!(4321));
        assert_eq!(saved["current"]["fees"]["2"],json!(5678));
        assert_eq!(saved["history"].as_array().unwrap().last(),Some(&previous));
        let stale=api.mutate_shipping(revision,"save_group_rule",update.clone())
            .expect_err("stale revision must be rejected by real CLI");
        assert!(stale.contains("다른 작업에서 데이터가 수정되었습니다."),"{stale}");
        assert_eq!(api.load_business_state().unwrap(),after,
            "rejected stale write must leave DB unchanged");
        let invalid=api.mutate_shipping(revision+1,"unknown_shipping_action",json!({}))
            .expect_err("unknown command must be rejected by real CLI");
        assert!(invalid.contains("허용되지 않은 운임 변경 명령"),"{invalid}");
        assert_eq!(api.load_business_state().unwrap(),after);
        std::env::set_var("BANDO_EGUI_ENABLE_SHIPPING_WRITE","0");
        assert!(!api.shipping_write_enabled());
        let disabled=api.mutate_shipping(revision+1,"save_group_rule",update)
            .expect_err("disabled gateway must not launch a writer");
        assert!(disabled.contains("운영 운임을 변경할 수 있는 연결이 아닙니다."));
        assert_eq!(api.load_business_state().unwrap(),after);
        // The CI runner owns and disposes of this dedicated test DB. Do not
        // delete any operational file or write to a non-isolated path.
    }
}
