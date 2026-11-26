//! Transport boundary for the egui UI.
//!
//! Views and reception services depend on BandoGateway only. They must not
//! import native Rust modules, reqwest, Tauri, or a database connection.
//! Local and future HTTPS adapters preserve the same JSON business contract.
//!
//! Business queries use a read-only allowlist. Explicit writes use separate
//! native v2 commands behind existing-database authorization, revision CAS
//! and Rust rules validation. The isolated reception workspace never
//! confirms an operating order. No HTTP->local fallback.

use serde_json::Value;
use std::sync::Arc;
use crate::preview_store::{PreviewStore, WorkingSource};

#[path = "../../src-tauri/src/business_core/recognition/mod.rs"]
mod recognition;
#[path = "../../src-tauri/src/business_core/rules/mod.rs"]
pub(crate) mod rules;
#[path = "../../src-tauri/src/business_core/output/mod.rs"]
pub(crate) mod output;
#[path = "../../src-tauri/src/business_core/catalog_clients/title_clients.rs"]
mod native_title_clients;

#[derive(Clone, Copy, Debug)]
pub enum ReadOperation {
    AnalyzeOrder,
    ResolveRetailUnitPrice,
    DetectRemoteDeliveryArea,
    ResolveRemoteDeliveryArea,
    AutoMatchReceptionClient,
    ReceptionClientCandidates,
    ClientCandidates,
    NewReception,
    MergeReceptionAnalysis,
    ChangeOrderClient,
    RepriceOrder,
    CalculatePrice,
    CalculateReceptionQuote,
    ManualClassification,
    SearchProductsWithUrls,
    ManagementProducts,
    PreviewWorkflow,
    PreviewOrder,
    OrderIssuesWithActions,
    ApplyIssueAction,
    ReadShipments,
    ListOrderHistoryView,
    ListOrderHistory,
    PrepareSettlement,
    PrepareFactoryExport,
    PrepareStoreExport,
    ReviewExportSender,
    FactoryReplyReviewStatuses,
    ShippingManagerView,
    ProductFreightView,
    DefaultProductRule,
    RenderAutomaticMessage,
    DecorateAutomaticCopy,
    ValidateAutomaticTemplates,
    ValidateProvisionalProductNotices,
}

impl ReadOperation {
    pub fn domain(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "recognition",
            Self::ResolveRetailUnitPrice | Self::DetectRemoteDeliveryArea
            | Self::ResolveRemoteDeliveryArea
            | Self::ValidateAutomaticTemplates | Self::ValidateProvisionalProductNotices => "rules",
            Self::AutoMatchReceptionClient | Self::ReceptionClientCandidates
            | Self::ClientCandidates | Self::SearchProductsWithUrls
            | Self::ManagementProducts => "catalog_clients",
            Self::NewReception | Self::MergeReceptionAnalysis
            | Self::ChangeOrderClient | Self::RepriceOrder
            | Self::CalculatePrice | Self::CalculateReceptionQuote | Self::ManualClassification
            | Self::PreviewWorkflow | Self::PreviewOrder
            | Self::OrderIssuesWithActions | Self::ApplyIssueAction
            | Self::ReadShipments | Self::ListOrderHistoryView | Self::ListOrderHistory
            | Self::FactoryReplyReviewStatuses | Self::ShippingManagerView
            | Self::ProductFreightView | Self::DefaultProductRule => "rules",
            Self::RenderAutomaticMessage | Self::DecorateAutomaticCopy
            | Self::PrepareSettlement | Self::PrepareFactoryExport | Self::PrepareStoreExport
            | Self::ReviewExportSender => "output",
        }
    }

    pub fn operation(self) -> &'static str {
        match self {
            Self::AnalyzeOrder => "analyze_order",
            Self::ResolveRetailUnitPrice => "resolve_unit_price",
            Self::DetectRemoteDeliveryArea => "detect_remote_delivery_area",
            Self::ResolveRemoteDeliveryArea => "resolve_remote_delivery_area",
            Self::AutoMatchReceptionClient => "auto_match_reception_client",
            Self::ReceptionClientCandidates => "reception_client_candidates",
            Self::ClientCandidates => "client_candidates",
            Self::NewReception => "apply_reception_action",
            Self::MergeReceptionAnalysis => "merge_reception_analysis",
            Self::ChangeOrderClient => "order_edit_rules",
            Self::RepriceOrder => "reprice_order",
            Self::CalculatePrice => "calculate_price",
            Self::CalculateReceptionQuote => "calculate_reception_quote",
            Self::ManualClassification => "apply_manual_classification",
            Self::SearchProductsWithUrls => "search_products_with_urls",
            Self::ManagementProducts => "management_products",
            Self::PreviewWorkflow => "preview_workflow",
            Self::PreviewOrder => "preview_order",
            Self::OrderIssuesWithActions => "order_issues_with_actions",
            Self::ApplyIssueAction => "apply_issue_action",
            Self::ReadShipments => "read_shipments",
            Self::ListOrderHistoryView => "list_order_history_view",
            Self::ListOrderHistory => "list_order_history",
            Self::PrepareSettlement => "prepare_settlement",
            Self::PrepareFactoryExport => "prepare_factory_export",
            Self::PrepareStoreExport => "prepare_store_export",
            Self::ReviewExportSender => "review_export_sender",
            Self::FactoryReplyReviewStatuses => "factory_reply_review_statuses",
            Self::ShippingManagerView => "shipping_manager_view",
            Self::ProductFreightView => "apply_shipping_action",
            Self::DefaultProductRule => "apply_shipping_action",
            Self::RenderAutomaticMessage => "render_automatic_message",
            Self::DecorateAutomaticCopy => "decorate_automatic_copy",
            Self::ValidateAutomaticTemplates => "validate_automatic_templates",
            Self::ValidateProvisionalProductNotices => "validate_provisional_product_notices",
        }
    }
}

/// Only the two reception metadata domains; no order or product projection.
pub struct ReceptionMetadata {
    pub inbox:Result<Option<String>,String>,
    pub clients:Result<Vec<Value>,String>,
}
fn inbox_path(settings:&Value)->Option<String>{
    settings.get("orderImageInboxDirectory").and_then(Value::as_str)
        .map(str::trim).filter(|path|!path.is_empty()).map(str::to_owned)
}

pub trait BandoGateway: Send + Sync {
    fn label(&self) -> &'static str;
    /// A minimal catalog snapshot (products, aliases, brands), plus an
    /// explicitly configured name-only client index. No operational database,
    /// confirmed orders, customer contacts or workflow state.
    /// This method does not persist or mutate anything.
    fn load_catalog(&self) -> Result<Value, String>;
    /// Client management uses only the authoritative Tauri v2 business DB.
    /// Upsert/archive requires explicit write authorization; no egui shadow DB.
    fn load_client_master(&self) -> Result<Value, String> {
        Err("거래처 관리 조회를 지원하지 않는 연결입니다.".into())
    }
    fn mutate_client_master(&self,_expected:Value,_action:&str,_client:Value)
        -> Result<Value,String> {
        Err("이 연결은 거래처 변경 저장을 지원하지 않습니다.".into())
    }
    fn archive_client_workbook(&self)->Result<String,String>{Err("거래처 Excel 미지원 연결".into())}
    fn preview_client_workbook(&self,_source:Value)->Result<Value,String>{Err("거래처 Excel 미지원 연결".into())}
    fn apply_client_workbook(&self,_review:Value)->Result<Value,String>{Err("거래처 Excel 미지원 연결".into())}
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
    /// Opt-in native freight write through the shared Rust v2 transaction.
    /// Does not spawn Tauri or write to the isolated egui preview DB.
    fn mutate_excel_workflow(
        &self,_revision:u64,_action:&str,_shipment_id:&str,
        _queue_id:&str,_origin:&str,_value:Value,
    )->Result<Value,String>{
        Err("이 연결에서는 Excel 관리 작업 저장을 지원하지 않습니다.".into())
    }
    fn excel_write_enabled(&self)->bool{false}
    fn export_excel(&self,_origin:&str,_ids:&[String])->Result<Value,String>{
        Err("이 연결에서는 Excel 파일 생성 및 출력 이력 저장을 지원하지 않습니다.".into())
    }
    fn open_excel_file(&self,_id:&str)->Result<(),String>{
        Err("이 연결에서는 Excel 파일 열기를 지원하지 않습니다.".into())
    }
    fn open_excel_folder(&self,_id:Option<&str>,_origin:Option<&str>)->Result<(),String>{
        Err("이 연결에서는 Excel 폴더 열기를 지원하지 않습니다.".into())
    }
    fn shipping_write_enabled(&self) -> bool { false }
    fn mutate_shipping(&self,_revision:u64,_action:&str,_data:Value)
        -> Result<Value,String> {
        Err("이 연결에서는 운영 운임 변경을 지원하지 않습니다.".into())
    }
    /// All product-management writes call the shared native Rust service.
    /// A read-only index or the egui preview catalog is never a fallback.
    fn product_write_enabled(&self)->bool {false}
    fn mutate_product(&self,_revision:u64,_action:&str,_data:Value)
        -> Result<Value,String>{
        Err("이 연결에서는 운영 상품 변경을 지원하지 않습니다.".into())
    }
    /// Export product workbook to the configured, dated Excel archive.
    /// Download is read-only with respect to the business DB.
    fn archive_product_workbook(&self)->Result<String,String>{
        Err("이 연결에서는 상품 Excel 보관함 저장을 지원하지 않습니다.".into())
    }
    fn open_product_export_folder(&self,_path:&std::path::Path)->Result<(),String>{
        Err("이 연결에서는 상품 Excel 폴더 열기를 지원하지 않습니다.".into())
    }
    /// Preview the parsed product workbook using the original native catalog engine.
    fn preview_product_workbook(&self,_source:Value)->Result<Value,String>{
        Err("상품 Excel 변경 검토를 지원하지 않는 연결입니다.".into())
    }
    /// Apply a reviewed workbook only after manual backup and native revision CAS.
    fn apply_product_workbook(&self,_preview:Value)->Result<Value,String>{
        Err("상품 Excel 변경 저장을 지원하지 않는 연결입니다.".into())
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
    /// An operating working reception requires an existing, authorized v2
    /// business SQLite file and explicit write opt-in. The native Rust
    /// transaction never confirms an order or writes to a preview fallback.
    fn working_commit_enabled(&self) -> bool { false }
    fn commit_working_reception(&self, _request: Value) -> Result<Value,String> {
        Err("운영 미확정 접수 저장 연결이 설정되지 않았습니다.".into())
    }
    /// Group progression, undo, delete/restore and shipment grouping all
    /// use the original native Rust rule reducers and the v2 revision CAS.
    /// Never mutate the read-only egui preview store or an HTTP fallback.
    fn workflow_write_enabled(&self)->bool{false}
    /// Adapters without a native revision probe refresh only on entry.
    fn reception_metadata(&self,_known:Option<u64>)
        ->Result<(Option<u64>,Option<ReceptionMetadata>),String>{
        let inbox=self.load_settings().and_then(|v|{
            let settings=v.get("settings").filter(|v|v.is_object())
                .ok_or("저장된 폴더 설정 형식 오류")?;
            Ok(inbox_path(settings))
        });
        let clients=self.load_catalog().and_then(|v|v.get("clients")
            .and_then(Value::as_array).cloned().ok_or("거래처 목록 형식 오류".into()));
        Ok((None,Some(ReceptionMetadata{inbox,clients})))
    }
    /// Native shipment mutations support order-scoped projection when the commit proves its scope.
    fn supports_scoped_freight_refresh(&self)->bool{false}
    fn mutate_workflow(
        &self,_expected_revision:u64,_action:&str,_args:Value
    )->Result<Value,String>{
        Err("현재 연결은 주문 묶음 업무 변경을 지원하지 않습니다.".into())
    }
    /// A per-shipment employee management memo is writable only through
    /// the existing authorized operational shipment DB boundary.
    fn shipment_memo_write_enabled(&self)->bool{false}
    fn save_shipment_management_memo(
        &self,_revision:u64,_shipment_id:&str,_memo:&str
    )->Result<Value,String>{
        Err("이 연결에서는 발송 건 관리 메모 저장을 지원하지 않습니다.".into())
    }
    /// The currently open, unconfirmed source in a separate storage domain.
    /// This never creates a confirmed business order.
    fn load_working_source(&self) -> Result<WorkingSource, String>;
    /// Conditional write: another process may not overwrite a newer source.
    fn save_working_source(&self, source: &WorkingSource) -> Result<u64, String>;
    /// Edit-only native preview. HTTP and test adapters retain the full quote.
    fn calculate_reception_edit_quote(&self,payload:Value)->Result<Value,String>{
        self.execute(ReadOperation::CalculateReceptionQuote,payload)
    }
    /// Display only; copy authorization must use execute directly.
    fn render_automatic_preview(&self,payload:Value)->Result<Value,String>{
        self.execute(ReadOperation::RenderAutomaticMessage,payload)
    }
    /// Execute one of the explicitly allowed read-only business operations.
    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String>;
}

fn management_catalog_projection(full:&Value)->Result<Value,String>{
    let (clients,mut settings)=crate::client_directory::project_index(full)?;
    settings["colorGuidanceGroups"]=full["settings"]
        .get("colorGuidanceGroups").cloned().unwrap_or(Value::Null);
    Ok(serde_json::json!({
        "revision":full.get("revision"),"products":full.get("products"),
        "aliases":full.get("aliases"),"brands":full.get("brands"),
        "categories":full.get("categories"),"shipping":full.get("shipping"),
        "clients":clients,"settings":settings
    }))
}

fn message_account_wire(state:&Value)->Result<Vec<Value>,String>{
    state["settings"]["accounts"].as_array().into_iter().flatten().map(|account|
        Ok(serde_json::json!({
            "id":account["id"],
            "json":serde_json::to_string(account).map_err(|error|error.to_string())?
        }))
    ).collect()
}

/// Read all reception metadata from one consistent read transaction.
/// An equal revision does not even touch core_clients.
fn read_reception_metadata(conn:&rusqlite::Connection,known:Option<u64>)
    ->Result<(Option<u64>,Option<ReceptionMetadata>),String>{
    let revision:i64=conn.query_row(
        "SELECT CASE (SELECT version FROM core_schema WHERE key='business-core') WHEN 3 THEN json_extract(payload,'$[0].revision') WHEN 2 THEN json_extract(payload,'$.revision') END FROM app_state WHERE id=1",
        [],|row|row.get(0)).map_err(|e|format!("업무 DB 리비전 확인 실패: {e}"))?;
    let revision=u64::try_from(revision).map_err(|_|"업무 DB 리비전 범위 오류")?;
    if known==Some(revision){return Ok((Some(revision),None));}
    let path:Option<String>=conn.query_row(
        "SELECT CASE (SELECT version FROM core_schema WHERE key='business-core') WHEN 3 THEN json_extract(payload,'$[0].settings.orderImageInboxDirectory') WHEN 2 THEN json_extract(payload,'$.settings.orderImageInboxDirectory') END FROM app_state WHERE id=1",
        [],|row|row.get(0)).map_err(|e|e.to_string())?;
    let clients=bando_data_access::read_client_index_rows(conn)?;
    let inbox=if crate::business_state::test_launch(){
        crate::business_state::test_directory("order-image-inbox")
            .map(|p|Some(p.to_string_lossy().into_owned()))
    }else{Ok(path.map(|p|p.trim().to_owned()).filter(|p|!p.is_empty()))};
    Ok((Some(revision),Some(ReceptionMetadata{inbox,clients:Ok(clients)})))
}

#[derive(Default)]
pub struct LocalGateway;

impl BandoGateway for LocalGateway {
    fn label(&self) -> &'static str {
        "로컬 Rust"
    }

    fn reception_metadata(&self,known:Option<u64>)
        ->Result<(Option<u64>,Option<ReceptionMetadata>),String>{
        if !std::env::var_os("BANDO_EGUI_BUSINESS_DB").is_some_and(|p|!p.is_empty()){
            let inbox=crate::settings_store::load().map(|v|inbox_path(&v["settings"]));
            let clients=crate::client_directory::attach(serde_json::json!({}))
                .and_then(|v|v.get("clients").and_then(Value::as_array).cloned()
                    .ok_or("거래처 목록 형식 오류".into()));
            return Ok((None,Some(ReceptionMetadata{inbox,clients})));
        }
        if std::env::var_os("BANDO_EGUI_CLIENT_INDEX").is_some_and(|p|!p.is_empty()){
            return Err("JSON 거래처 인덱스와 운영 DB를 동시에 사용할 수 없습니다.".into());
        }
        let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB").ok_or("업무 DB 연결 없음")?;
        let mut conn=rusqlite::Connection::open_with_flags(std::path::PathBuf::from(path),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(2)).map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA query_only=ON").map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let result=read_reception_metadata(&tx,known)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(result)
    }

    fn load_catalog(&self) -> Result<Value, String> {
        // When a live master DB is explicitly selected, recognition must use
        // the same product IDs, variants and aliases as the native price
        // engine. Never parse against a possibly stale embedded seed and
        // calculate prices against a different production catalog.
        if std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|value| !value.is_empty())
        {
            let full = crate::business_state::load_catalog()?;
            return management_catalog_projection(&full);
        }
        let catalog=PreviewStore::initialize()?.load_catalog()?;
        // No shadow customer master; an exported index is read-only.
        crate::client_directory::attach(catalog)
    }
    fn load_client_master(&self) -> Result<Value,String> {
        crate::client_master::load()
    }
    fn mutate_client_master(&self,expected:Value,action:&str,client:Value)
        -> Result<Value,String> {
        crate::client_master::mutate(expected,action,client)
    }
    fn archive_client_workbook(&self)->Result<String,String>{crate::client_master::archive_workbook()}
    fn preview_client_workbook(&self,source:Value)->Result<Value,String>{crate::client_master::preview_workbook(source)}
    fn apply_client_workbook(&self,review:Value)->Result<Value,String>{crate::client_master::apply_workbook(review)}
    fn load_settings(&self)->Result<Value,String>{
        crate::settings_store::load()
    }
    fn patch_settings(&self,changes:Value,expected:Value)->Result<Value,String>{
        crate::settings_store::patch(changes,expected)
    }
    fn model_group_write_enabled(&self)->bool{
        // Retain the required manual backup permission until backup itself
        // is migrated. The model-family SAVE no longer uses settings CLI.
        self.product_write_enabled() &&
        crate::settings_store::writable_business() &&
        crate::settings_store::data_service_available()
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
        // React ColorGuidanceManager requires an operating DB backup BEFORE
        // each model-family change. Keep this existing backup operation.
        self.settings_data("backup",None)?;
        // The settings CLI is no longer involved in saving model families.
        // The shared service implements the original per-key CAS + native
        // Rust model-group validation inside one revision-checked writer.
        use rusqlite::{Connection,OpenFlags};
        let path=crate::business_state::authorized_business_write_path()?;
        let mut conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_|"운영 DB를 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e|e.to_string())?;
        let saved=bando_data_access::patch_model_groups_v2(
            &mut conn,&groups,&expected
        )?;
        drop(conn);
        management_catalog_projection(&saved)
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
            .ok().as_deref()==Some("1") &&
        crate::business_state::authorized_business_write_path().is_ok()
    }
    fn mutate_product(&self,revision:u64,action:&str,data:Value)
        -> Result<Value,String>{
        use rusqlite::{Connection,OpenFlags};
        if !self.product_write_enabled(){
            return Err("운영 상품을 수정할 수 있는 연결이 아닙니다.".into());
        }
        // The explicit path is validated against the Tauri test DB in
        // debug/developer mode; never fall back to egui's preview SQLite.
        let path=crate::business_state::authorized_business_write_path()?;
        let request=serde_json::json!({
            "expectedRevision":revision,"action":action,"data":data,
        });
        let limit=if action=="apply_product_workbook"{60_000_000}else{2_000_000};
        if request.to_string().len()>limit{
            return Err("상품 변경 요청이 허용 크기를 초과했습니다.".into());
        }
        let mut conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_|"운영 DB를 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e|e.to_string())?;
        // All seven original catalog operations and price CAS now run
        // in-process. The shared writer owns the commit and revision check.
        let saved=bando_data_access::patch_product_v2(&mut conn,&request)?;
        drop(conn);
        management_catalog_projection(&saved)
    }
    fn mutate_excel_workflow(
        &self,revision:u64,action:&str,shipment_id:&str,
        queue_id:&str,origin:&str,value:Value,
    )->Result<Value,String>{
        crate::excel_service::workflow_action(
            revision,action,shipment_id,queue_id,origin,value
        )
    }
    fn excel_write_enabled(&self)->bool{crate::excel_service::writable()}
    fn shipment_memo_write_enabled(&self)->bool{
        crate::order_history_memo::writable()
    }
    fn save_shipment_management_memo(
        &self,revision:u64,shipment_id:&str,memo:&str
    )->Result<Value,String>{
        crate::order_history_memo::save(revision,shipment_id,memo)
    }
    fn export_excel(&self,origin:&str,ids:&[String])->Result<Value,String>{
        crate::excel_service::create(origin,ids)
    }
    fn open_excel_file(&self,id:&str)->Result<(),String>{
        crate::excel_service::open_file(id)
    }
    fn open_excel_folder(&self,id:Option<&str>,origin:Option<&str>)->Result<(),String>{
        if let Some(id)=id{crate::excel_service::open_directory(id)}
        else if let Some(origin)=origin{crate::excel_service::open_daily(origin)}
        else{Err("열어 볼 Excel 폴더를 선택하세요.".into())}
    }
    fn archive_product_workbook(&self)->Result<String,String>{
        let state=self.load_business_state()?;
        let bytes=crate::product_excel::export(&state)?;
        let path=crate::excel_service::archive_product_workbook(&state,&bytes)?;
        Ok(path.to_string_lossy().to_string())
    }
    fn open_product_export_folder(&self,path:&std::path::Path)->Result<(),String>{
        if !path.is_absolute()||!path.is_file(){
            return Err("저장된 상품 Excel 파일 위치를 확인할 수 없습니다.".into())
        }
        let folder=path.parent().ok_or("상품 Excel 저장 폴더를 찾지 못했습니다.")?;
        open::that(folder).map_err(|e|format!("상품 Excel 폴더 열기 실패: {e}"))
    }
    fn preview_product_workbook(&self,source:Value)->Result<Value,String>{
        if source.to_string().len()>40_000_000{
            return Err("상품 Excel 검토 요청이 허용 크기를 초과했습니다.".into());
        }
        let snapshot=self.load_business_state()?;
        bando_data_access::preview_unified_product_workbook(&snapshot,&source)
    }
    fn apply_product_workbook(&self,preview:Value)->Result<Value,String>{
        if !self.product_write_enabled(){
            return Err("운영 상품 변경 저장 권한이 없습니다.".into());
        }
        let revision=preview["revision"].as_u64()
            .ok_or("상품 Excel 검토 리비전을 확인할 수 없습니다.")?;
        let now=self.load_business_state()?;
        if now["revision"].as_u64()!=Some(revision){
            return Err("검토 후 데이터가 변경되었습니다. 파일을 다시 검증하세요.".into());
        }
        if !preview["rows"].as_array().is_some_and(|rows|!rows.is_empty()) ||
            preview["rows"].as_array().is_some_and(|rows|rows.iter().any(|row|
                matches!(row["status"].as_str(),Some("ERROR"|"HOLD")))){
            return Err("상품 Excel에 오류나 중복 의심 항목이 있습니다. 변경사항을 저장하지 않습니다.".into());
        }
        if preview["createdProducts"].as_u64().unwrap_or(0)==0 &&
            preview["updatedProducts"].as_u64().unwrap_or(0)==0 &&
            preview["createdOptions"].as_u64().unwrap_or(0)==0 &&
            preview["updatedOptions"].as_u64().unwrap_or(0)==0 {
            return Err("상품 Excel에 저장할 변경사항이 없습니다.".into());
        }
        // Product Excel must create the original mandatory manual backup.
        // Settings-write permission is unrelated to product-write permission.
        // Use the native backup service, the same authorized business DB
        // path, and then the existing revision-checked product writer.
        let path=crate::business_state::authorized_business_write_path()?;
        let conn=rusqlite::Connection::open_with_flags(
            &path,rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        ).map_err(|_|"운영 DB를 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e|e.to_string())?;
        let base=path.parent().ok_or("운영 백업 폴더를 확인할 수 없습니다.")?;
        bando_data_access::backup_service::manual_backup(&conn,base)?;
        drop(conn);
        self.mutate_product(revision,"apply_product_workbook",serde_json::json!({"preview":preview}))
    }
    fn shipping_write_enabled(&self) -> bool {
        std::env::var("BANDO_EGUI_ENABLE_SHIPPING_WRITE")
            .ok().as_deref()==Some("1") &&
        crate::business_state::authorized_business_write_path().is_ok()
    }
    fn mutate_shipping(&self,revision:u64,action:&str,data:Value)
        -> Result<Value,String> {
        use rusqlite::{Connection,OpenFlags};
        if !self.shipping_write_enabled() {
            return Err("운영 운임을 변경할 수 있는 연결이 아닙니다.".into());
        }
        let path=crate::business_state::authorized_business_write_path()?;
        if !path.is_file(){
            return Err("운영 DB 파일이 없습니다.".into());
        }
        // Preserve the existing helper's request-size contract.
        let request=serde_json::json!({
            "expectedRevision":revision,"action":action,"data":&data,
        }).to_string();
        if request.is_empty() || request.len()>2_000_000 {
            return Err("운임 변경 요청 크기가 허용 범위를 초과했습니다.".into());
        }
        // Direct native Rust call. No Tauri EXE or process IPC is involved.
        let mut conn=Connection::open_with_flags(
            &path,OpenFlags::SQLITE_OPEN_READ_WRITE
        ).map_err(|_|"운영 DB를 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e|e.to_string())?;
        let version:i64=conn.query_row(
            "SELECT version FROM core_schema WHERE key='business-core'",
            [],|row|row.get(0),
        ).map_err(|_|"운영 DB 스키마를 확인할 수 없습니다.".to_owned())?;
        if !bando_data_access::supported_relational_version(version){return Err("지원하지 않는 운영 DB 버전입니다.".into());}
        let committed=bando_data_access::update_v2_entity_if_changed(&mut conn,true,|before|{
        let current=before["revision"].as_u64().ok_or("운영 DB 리비전 오류")?;
        if let Some(token)=data["expectedScope"].as_str(){
            if token!=bando_data_access::management_scope_token(before,"shipping"){
                return Err("운임 기준 또는 배송 그룹 구성이 변경되었습니다. 입력 내용을 유지합니다.".into());
            }
        }else if current!=revision{
            return Err("다른 작업에서 데이터가 수정되었습니다. 다시 불러오세요.".into());
        }
        let payload=bando_data_access::shipping_action::action_payload(
            action,&data,before
        )?;
        // Execute the SAME original Rust shipping mutation rule as Tauri.
        let result=rules::execute("apply_shipping_action",payload)?;
        let mut next=if result.get("state").is_some(){
            result["state"].clone()
        }else if let Some(shipping)=result.get("shipping"){
            let mut snapshot=before.clone();
            snapshot["shipping"]=shipping.clone();
            snapshot
        }else{return Err("Rust 운임 처리 결과를 확인할 수 없습니다.".into());};
        next["revision"]=serde_json::json!(current);
        Ok((next!=*before).then_some(next))
        })?;
        drop(conn);
        let projection=crate::management_queries::project_freight(self,&committed)?;
        Ok(serde_json::json!({"state":committed,"projection":projection}))
    }
    fn supports_scoped_freight_refresh(&self)->bool{true}
    fn workflow_write_enabled(&self)->bool{
        // A named, pre-existing v2 database is the user's explicit binding.
        // The writer guard restricts debug Windows to the developer TEST DB.
        crate::business_state::authorized_business_write_path().is_ok()
    }
    fn mutate_workflow(
        &self,expected_revision:u64,action:&str,args:Value
    )->Result<Value,String>{
        use rusqlite::{Connection,OpenFlags};
        // These are the existing React/Rust actions, not UI-invented stages.
        let group_delete=matches!(action,"delete_group"|"restore_group");
        if !group_delete && !matches!(
            action,
            "advance_group"|"undo_group"|"confirm_group_payment"|"process_group"
            |"detach_shipment"|"regroup_shipment"|"set_shipment_freight"|"record_message_copy"
            |"change_confirmed_dispatch_origin"|"edit_confirmed_recipient"
        ){
            return Err("지원하지 않는 주문 묶음 작업입니다.".into());
        }
        let path=crate::business_state::authorized_business_write_path()?;
        let mut conn=Connection::open_with_flags(
            path,OpenFlags::SQLITE_OPEN_READ_WRITE
        ).map_err(|_|"업무 DB를 쓰기 모드로 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error|error.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|error|error.to_string())?;
        let profile=std::env::var_os("BANDO_EGUI_PROFILE_WORKFLOW").is_some();
        let started=std::time::Instant::now();
        let mut previous=started;
        let mut checkpoint=|phase:&str|{
            let now=std::time::Instant::now();
            if profile{
                eprintln!("[workflow-perf] action={action} phase={phase} ms={:.3} total_ms={:.3}",
                    now.duration_since(previous).as_secs_f64()*1000.0,
                    now.duration_since(started).as_secs_f64()*1000.0);
            }
            previous=now;
        };
        let scoped_freight=action=="set_shipment_freight" && args.get("draft").is_none();
        let scoped_group_move=matches!(action,"detach_shipment"|"regroup_shipment");
        let scoped_shipment=scoped_freight || scoped_group_move;
        let confirmed_order_edit=matches!(action,"change_confirmed_dispatch_origin"|"edit_confirmed_recipient");
        let mutate=|before:&Value|->Result<Value,String>{
        checkpoint("read_snapshot");
        // An unconfirmed egui draft may enter the operational ledger ONLY
        // through the explicit first-stage approval. The full Order is a
        // fresh native quote result, not the isolated WorkingReception row.
        if let Some(draft)=args.get("draft"){
            if !draft.is_object() || group_delete || action!="advance_group"{
                return Err("미확정 발주서는 첫 번째 주문 확인 단계에서만 확정할 수 있습니다.".into());
            }
            let draft_id=draft.get("id").and_then(Value::as_str)
                .filter(|id|!id.trim().is_empty())
                .ok_or("미확정 발주서 ID가 없습니다.")?;
            let expected_group=format!("group:{draft_id}");
            if args["groupId"].as_str()!=Some(expected_group.as_str()){
                return Err("미확정 발주서와 업무 묶음 ID가 일치하지 않습니다.".into());
            }
            if before["orders"].as_array().is_some_and(|orders|orders.iter().any(|order|
                order.get("id").and_then(Value::as_str)==Some(draft_id)
            )){
                return Err("이미 운영 주문으로 등록된 발주서입니다.".into());
            }
            if draft["receptionVersion"].as_i64()!=Some(1) ||
                draft.get("confirmedAt").is_some_and(|value|!value.is_null()) ||
                draft["pricesLocked"]==Value::Bool(true) ||
                !["AGENCY","DEALER"].contains(&draft["orderType"].as_str().unwrap_or("")){
                return Err("정식 확정 대상이 아닌 미확정 발주서입니다.".into());
            }
            if draft["recipients"].as_array().is_none_or(|rows|rows.is_empty()){
                return Err("수취인별 배송 건이 없어 주문을 확정할 수 없습니다.".into());
            }
            let client_id=draft["clientId"].as_str()
                .filter(|id|!id.trim().is_empty())
                .ok_or("거래처를 먼저 선택하세요.")?;
            let client_exists=before["clients"].as_array().is_some_and(|clients|
                clients.iter().any(|client|
                    client["id"].as_str()==Some(client_id) &&
                    client.get("deletedAt").is_none_or(|value|
                        value.is_null() || value.as_str()==Some("")
                    ) &&
                    client["active"]!=Value::Bool(false)
                )
            );
            if !client_exists{
                return Err("선택한 거래처가 현재 운영 명단에 없습니다. 다시 선택하세요.".into());
            }
        }
        let now=chrono::Utc::now().to_rfc3339();
        let event_id=uuid::Uuid::new_v4().to_string();
        let actor=before["settings"]["operatorName"].as_str()
            .filter(|name|!name.trim().is_empty())
            .unwrap_or("사용자(이름 미설정)").to_owned();
        if confirmed_order_edit{
            let result=rules::execute("order_edit_rules",serde_json::json!({
                "action":action,"state":before,"shipmentId":args["shipmentId"],
                "groupId":args["groupId"],"origin":args["origin"],
                "change":args["change"],"now":now,"eventSeed":event_id
            }))?;
            return result.get("state").cloned()
                .ok_or("확정 주문 수정 결과에 상태가 없습니다.".into());
        }
        let request=if action=="record_message_copy"{
            // Copy evidence must describe the same current native message.
            // Never reconcile or mutate prices as a side effect of copying.
            let group_id=args["groupId"].as_str().ok_or("주문 묶음 ID 누락")?;
            let group_active=before["workflow"]["groups"].as_array().is_some_and(|groups|
                groups.iter().any(|group|group["id"].as_str()==Some(group_id)
                    && group["deletedAt"].is_null()));
            let shipment_active=before["workflow"]["shipments"].as_array().is_some_and(|ships|
                ships.iter().any(|ship|ship["groupId"].as_str()==Some(group_id)
                    && ship["transferred"]!=true && ship["legacyArchived"]!=true));
            if !group_active || !shipment_active{
                return Err("처리 중인 주문 묶음을 다시 선택하세요.".into());
            }
            let wire=message_account_wire(&before)?;
            let current=output::execute("render_automatic_message",serde_json::json!({
                "state":&before,"groupId":group_id,
                "includeLinks":args["includeLinks"].as_bool().unwrap_or(false),"accountWire":wire
            }))?;
            let copy_type=match current["phase"].as_str(){
                Some("request")=>"REQUEST",
                Some("payment") if current["paymentIssues"].as_array().is_some_and(|v|v.is_empty())=>"PAYMENT",
                _=>return Err("복사 가능한 계좌 안내 메시지가 아닙니다.".into())
            };
            if args["copyType"].as_str()!=Some(copy_type) ||
                current["text"]!=args["messageText"] ||
                (copy_type=="PAYMENT" && (current["signature"].as_str().is_none_or(str::is_empty)
                    || current["signature"]!=args["signature"])){
                return Err("복사 후 안내 내용이 변경되었습니다. 최신 메시지를 다시 복사하세요.".into());
            }
            serde_json::json!({
                "state":&before,"groupId":group_id,"copyType":copy_type,
                "signature":current["signature"],"token":args["token"],
                "now":now,"eventId":event_id
            })
        }else if group_delete {
            serde_json::json!({
                "state":&before,"action":action,
                "groupId":args["groupId"],"now":now,"eventId":event_id
            })
        }else{
            // Reconcile changes to prices, tariff signatures and late
            // corrections before attempting the transition. The native
            // reducer produces all immutable audit entries and validations.
            let reconciled=if scoped_shipment{
                // The native engine resolves the order from the shipment,
                // not from a UI-provided order ID. Reconcile only that order
                // and its split groups; global validation and CAS remain.
                rules::execute("reconcile_shipment_workflow",serde_json::json!({
                    "state":&before,"now":now,
                    "shipmentId":args["shipmentId"],"groupId":args["groupId"],
                    "eventSeed":event_id
                }))?
            }else{
                let count=(12*before["workflow"]["shipments"]
                    .as_array().map(Vec::len).unwrap_or(0)
                    +8*before["orders"].as_array().map(Vec::len).unwrap_or(0)+64)
                    .max(256);
                if count>15000{
                    return Err("재검증 이벤트 수가 허용 범위를 초과했습니다.".into());
                }
                let event_ids=(0..count).map(|_|
                    uuid::Uuid::new_v4().to_string()).collect::<Vec<_>>();
                let mut reconcile_payload=serde_json::json!({
                    "state":&before,"now":now,"eventIds":event_ids
                });
                // Draft confirmation and other transitions retain the full
                // reconciliation and their existing audit wire contract.
                if let Some(draft)=args.get("draft"){
                    reconcile_payload["draft"]=draft.clone();
                }
                rules::execute("reconcile_workflow",reconcile_payload)?
            };
            checkpoint("reconcile");
            let prepared=reconciled.get("state")
                .ok_or("업무 재검증 결과에 상태가 없습니다.")?;
            let next_step=if action=="advance_group"{
                Some(rules::execute("evaluate_workflow",serde_json::json!({
                    "state":prepared,"groupId":args["groupId"]
                }))?.get("nextStep").and_then(Value::as_u64)
                    .ok_or("다음 진행 단계를 확인하지 못했습니다.")?)
            }else{None};
            let payment_message=if next_step==Some(3){
                // A prior copy is only evidence for this exact financial
                // signature. The alternative delivery path needs explicit staff approval.
                if args["ack"]!=true{
                    let wire=message_account_wire(prepared)?;
                    let status=output::execute("payment_copy_status",serde_json::json!({
                        "state":prepared,"groupId":args["groupId"],
                        "accountWire":wire,"mode":"payment_copy_status"
                    }))?;
                    if status["copied"]!=true{
                        return Err("현재 계좌 안내 메시지의 복사 기록이 없습니다. 다시 복사하거나 다른 방법으로 전달했는지 확인하세요.".into());
                    }
                }
                Some(output::execute("render_payment_notice",
                    serde_json::json!({
                        "state":prepared,"groupId":args["groupId"]
                    })
                )?.get("payment").and_then(Value::as_str)
                    .ok_or("입금 안내 문구를 생성하지 못했습니다.")?.to_owned())
            }else{None};
            // Freight can invalidate only the target shipment's confirmations
            // plus its own stage recheck. Keep UUID audit identities but do
            // not allocate a fixed pool for every single-cell edit.
            let audit_count=if action=="set_shipment_freight"{
                let shipment_id=args["shipmentId"].as_str();
                let needs_recheck=prepared["workflow"]["shipments"].as_array()
                    .is_some_and(|ships|ships.iter().any(|ship|
                        shipment_id.is_some() && ship["id"].as_str()==shipment_id &&
                        ship["step"].as_i64().unwrap_or(0)>1));
                if needs_recheck{
                    1+prepared["workflow"]["confirmations"].as_array()
                        .map(|rows|rows.iter().filter(|row|
                            row["shipmentIds"].as_array().is_some_and(|ids|
                                ids.iter().any(|id|id.as_str()==shipment_id))
                        ).count()).unwrap_or(0)
                }else{0}
            }else if scoped_group_move{
                // Both group moves append their single event using eventId.
                // Reconciliation already emits any necessary scoped audit events.
                0
            }else{2048};
            let audit_ids=(0..audit_count).map(|_|
                uuid::Uuid::new_v4().to_string()).collect::<Vec<_>>();
            let mut request=serde_json::json!({
                "state":prepared,"action":action,"now":now,"actor":actor,
                "eventId":event_id,"auditEventIds":audit_ids,
                "newGroupId":uuid::Uuid::new_v4().to_string(),
                "confirmationId":uuid::Uuid::new_v4().to_string(),
                "noticeId":uuid::Uuid::new_v4().to_string()
            });
            let extras=args.as_object()
                .ok_or("주문 묶음 작업 인자 형식이 올바르지 않습니다.")?;
            let object=request.as_object_mut().ok_or("업무 요청 형식 오류")?;
            for (key,value) in extras{
                if matches!(key.as_str(),
                    "groupId"|"shipmentId"|"targetGroupId"|"ack"|
                    "origin"|"value"|"appliedCharge"|"freightCode"|"remoteChecked"
                ){
                    object.insert(key.clone(),value.clone());
                }
            }
            if let Some(payment_message)=payment_message{
                object.insert("paymentMessage".into(),
                    Value::String(payment_message));
            }
            request
        };
        let op=if action=="record_message_copy"{"record_message_copy"}
            else if group_delete{"apply_group_action"}
            else{"apply_workflow_action"};
        let result=rules::execute(op,request)?;
        checkpoint("apply_action");
        let changed=result.get("state")
            .ok_or("업무 처리 결과에 상태가 없습니다.")?.clone();
        if !scoped_shipment{
            rules::execute("validate_workflow",serde_json::json!({
                "state":&changed,"before":&before
            }))?;
            checkpoint("validate");
            if &changed==before{
                return Err("현재 상태에서 수행할 변경이 없습니다.".into());
            }
        }
        Ok(changed)
        };
        let verified=if scoped_shipment{
            let shipment_id=args["shipmentId"].as_str().ok_or("배송 건 ID 누락")?;
            let group_id=args["groupId"].as_str().ok_or("주문 묶음 ID 누락")?;
            let (mut state,stable)=bando_data_access::update_v2_shipment_snapshot(
                &mut conn,expected_revision,shipment_id,group_id,mutate)?;
            // Transient response metadata only, never written to SQLite.
            // A fallback may include catalog normalization or trigger effects.
            state["_scopedShipmentRefresh"]=Value::Bool(stable);
            state
        }else{
            bando_data_access::update_v2_snapshot(
                &mut conn,expected_revision,true,mutate)?
        };
        // Same transactional before-image, validation, write, and persisted
        // projection verification. A concurrent update cannot be mistaken
        // for a failed commit after the employee's own write succeeded.
        checkpoint("write_verify_commit");
        Ok(verified)
    }
    // The egui intake is a local autosaved draft, NOT a separate
    // operational working-order write. The old opt-in bridge was not part
    // of the approved intake workflow; keep it disabled even if an older
    // launcher still sets BANDO_EGUI_ENABLE_WORKING_COMMIT=1.
    fn working_commit_enabled(&self) -> bool { false }
    fn commit_working_reception(&self, _request: Value) -> Result<Value,String> {
        Err("egui에서 별도 운영 작업본 저장은 지원하지 않습니다.".into())
    }

    fn load_working_source(&self) -> Result<WorkingSource, String> {
        PreviewStore::initialize()?.load_source()
    }

    fn save_working_source(&self, source: &WorkingSource) -> Result<u64, String> {
        PreviewStore::initialize()?.save_source(source)
    }

    fn render_automatic_preview(&self,payload:Value)->Result<Value,String>{
        static CACHE:std::sync::OnceLock<std::sync::Mutex<output::AutomaticPreviewCache>>=
            std::sync::OnceLock::new();
        let cache=CACHE.get_or_init(||std::sync::Mutex::new(
            output::AutomaticPreviewCache::default()));
        match cache.try_lock(){
            Ok(mut cache)=>cache.preview(&payload),
            Err(_)=>self.execute(ReadOperation::RenderAutomaticMessage,payload)
        }
    }

    fn calculate_reception_edit_quote(&self,payload:Value)->Result<Value,String>{
        static CACHE:std::sync::OnceLock<std::sync::Mutex<rules::ReceptionEditQuoteCache>>=
            std::sync::OnceLock::new();
        let cache=CACHE.get_or_init(||std::sync::Mutex::new(
            rules::ReceptionEditQuoteCache::default()));
        // Avoid making independent worker requests wait for this cache.
        match cache.try_lock(){
            Ok(mut cache)=>cache.quote(&payload),
            Err(_)=>self.execute(ReadOperation::CalculateReceptionQuote,payload)
        }
    }

    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String> {
        match operation {
            ReadOperation::AnalyzeOrder => {
                recognition::execute(operation.operation(), payload)
            }
            ReadOperation::ResolveRetailUnitPrice
            | ReadOperation::DetectRemoteDeliveryArea
            | ReadOperation::ResolveRemoteDeliveryArea
            | ReadOperation::NewReception
            | ReadOperation::MergeReceptionAnalysis
            | ReadOperation::ChangeOrderClient
            | ReadOperation::RepriceOrder
            | ReadOperation::CalculatePrice
            | ReadOperation::CalculateReceptionQuote
            | ReadOperation::ManualClassification
            | ReadOperation::PreviewWorkflow
            | ReadOperation::PreviewOrder
            | ReadOperation::OrderIssuesWithActions
            | ReadOperation::ApplyIssueAction
            | ReadOperation::ReadShipments
            | ReadOperation::ListOrderHistoryView
            | ReadOperation::ListOrderHistory
            | ReadOperation::FactoryReplyReviewStatuses
            | ReadOperation::ShippingManagerView
            | ReadOperation::ProductFreightView
            | ReadOperation::DefaultProductRule
            | ReadOperation::ValidateAutomaticTemplates
            | ReadOperation::ValidateProvisionalProductNotices => {
                rules::execute(operation.operation(), payload)
            }
            ReadOperation::AutoMatchReceptionClient
            | ReadOperation::ReceptionClientCandidates => {
                native_title_clients::execute(operation.operation(), &payload)
            }
            ReadOperation::ClientCandidates => {
                crate::catalog_query_bridge::client_candidates(&payload)
            }
            ReadOperation::SearchProductsWithUrls => {
                crate::catalog_query_bridge::search_products(&payload)
            }
            ReadOperation::ManagementProducts => {
                crate::catalog_query_bridge::management_products(&payload)
            }
            ReadOperation::RenderAutomaticMessage | ReadOperation::DecorateAutomaticCopy
            | ReadOperation::PrepareSettlement
            | ReadOperation::PrepareFactoryExport | ReadOperation::PrepareStoreExport
            | ReadOperation::ReviewExportSender => {
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
mod shipping_gateway_direct_roundtrip_tests {
    use super::{BandoGateway,LocalGateway};
    use serde_json::{json,Value};

    /// Real LocalGateway -> shared Rust shipping service -> v2 SQLite ->
    /// read-only egui re-query. No helper EXE, GUI, or mocked persistence.
    /// An explicitly created isolated test database is mandatory.
    #[test]
    fn shipping_gateway_direct_roundtrip() {
        let db=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .expect("test fixture DB is explicitly required");
        assert_eq!(
            std::path::Path::new(&db).file_name().and_then(|x|x.to_str()),
            Some("bando-freight-gateway-e2e.sqlite"),
            "refuse to run against any operational DB"
        );
        assert!(std::path::Path::new(&db).is_file());

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
        rule["notes"]=json!("Rust 직접 호출 운임 검증");
        let update=json!({"groupId":group_id,"rule":rule});
        let response=api.mutate_shipping(revision,"save_group_rule",update.clone())
            .expect("egui Gateway calls native storage directly and reloads DB");
        assert_eq!(response["state"]["revision"],json!(revision+1));
        assert!(response["projection"]["productViews"].is_object());
        let after=api.load_business_state()
            .expect("read-only egui reload after direct Rust commit");
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
            .expect_err("stale revision must be rejected by real storage");
        assert!(stale.contains("다른 작업에서 데이터가 수정되었습니다."),"{stale}");
        assert_eq!(api.load_business_state().unwrap(),after,
            "rejected stale write must leave DB unchanged");
        let invalid=api.mutate_shipping(revision+1,"unknown_shipping_action",json!({}))
            .expect_err("unknown command must be rejected by native Rust");
        assert!(invalid.contains("허용되지 않은 운임 변경 명령"),"{invalid}");
        assert_eq!(api.load_business_state().unwrap(),after);
        std::env::set_var("BANDO_EGUI_ENABLE_SHIPPING_WRITE","0");
        assert!(!api.shipping_write_enabled());
        let disabled=api.mutate_shipping(revision+1,"save_group_rule",update)
            .expect_err("disabled gateway must not write");
        assert!(disabled.contains("운영 운임을 변경할 수 있는 연결이 아닙니다."));
        assert_eq!(api.load_business_state().unwrap(),after);
        // The CI runner owns and disposes of this dedicated test DB. Do not
        // delete any operational file or write to a non-isolated path.
    }
}

#[cfg(test)]
mod reception_metadata_tests {
    use super::*;
    #[test]
    fn unchanged_revision_skips_client_and_setting_projection(){
        let conn=rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_schema(key TEXT,version INTEGER); INSERT INTO core_schema VALUES('business-core',2); CREATE TABLE app_state(id INTEGER,payload TEXT); INSERT INTO app_state VALUES(1,'{\"revision\":7,\"settings\":{}}');").unwrap();
        let (revision,metadata)=read_reception_metadata(&conn,Some(7)).unwrap();
        assert_eq!(revision,Some(7)); assert!(metadata.is_none());
        // There is deliberately no core_clients table in this fixture.
        assert!(read_reception_metadata(&conn,Some(6)).is_err());
        conn.execute_batch("CREATE TABLE core_clients(id TEXT,position INTEGER,payload TEXT); INSERT INTO core_clients VALUES('c',0,'{\"id\":\"c\",\"name\":\"가상 거래처\"}');").unwrap();
        let (_,metadata)=read_reception_metadata(&conn,None).unwrap();
        let metadata=metadata.unwrap();
        assert_eq!(metadata.inbox.unwrap(),None);
        assert_eq!(metadata.clients.unwrap()[0]["id"],"c");
        conn.execute("UPDATE app_state SET payload=?1 WHERE id=1",[serde_json::json!({"revision":8,"settings":{"orderImageInboxDirectory":"  inbox  "}}).to_string()]).unwrap();
        let (revision,metadata)=read_reception_metadata(&conn,Some(7)).unwrap();
        assert_eq!(revision,Some(8));
        assert_eq!(metadata.unwrap().inbox.unwrap().as_deref(),Some("inbox"));
        assert!(read_reception_metadata(&conn,Some(8)).unwrap().1.is_none());
    }
}
