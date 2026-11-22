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
            | Self::ValidateAutomaticTemplates | Self::ValidateProvisionalProductNotices => "rules",
            Self::AutoMatchReceptionClient | Self::ReceptionClientCandidates
            | Self::ClientCandidates | Self::SearchProductsWithUrls
            | Self::ManagementProducts => "catalog_clients",
            Self::NewReception | Self::MergeReceptionAnalysis
            | Self::ChangeOrderClient | Self::RepriceOrder
            | Self::CalculatePrice | Self::ManualClassification
            | Self::PreviewWorkflow | Self::PreviewOrder
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
            let full = crate::business_state::load_catalog()?;
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
        let catalog=PreviewStore::initialize()?.load_catalog()?;
        // No shadow customer master; an exported index is read-only.
        crate::client_directory::attach(catalog)
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
        let revision=saved["revision"].as_u64()
            .ok_or("모델군 저장 결과 revision 오류")?;
        drop(conn);
        let catalog=self.load_catalog()?;
        // A non-null key alone is NOT proof that the editor's exact draft
        // survived a concurrent commit or the SQLite round trip.
        if catalog["revision"].as_u64()!=Some(revision) ||
            catalog["settings"]["colorGuidanceGroups"]!=groups{
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
        let written_revision=saved["revision"].as_u64()
            .ok_or("상품 변경 결과의 revision을 확인할 수 없습니다.")?;
        drop(conn);
        let catalog=self.load_catalog()?;
        if catalog["revision"].as_u64()!=Some(written_revision){
            return Err("상품은 저장됐지만 최신 상태 조회가 일치하지 않습니다. 새로고침하세요.".into());
        }
        Ok(catalog)
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
        std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .is_some_and(|v|!v.is_empty())
    }
    fn mutate_shipping(&self,revision:u64,action:&str,data:Value)
        -> Result<Value,String> {
        use rusqlite::{Connection,OpenFlags};
        if !self.shipping_write_enabled() {
            return Err("운영 운임을 변경할 수 있는 연결이 아닙니다.".into());
        }
        let path=std::env::var_os("BANDO_EGUI_BUSINESS_DB")
            .filter(|value|!value.is_empty()).map(std::path::PathBuf::from)
            .ok_or("운영 DB 경로가 지정되지 않았습니다.")?;
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
        if version!=2{return Err("지원하지 않는 운영 DB 버전입니다.".into());}
        let before=bando_data_access::read_v2_snapshot(&conn)?;
        let current=before["revision"].as_u64().ok_or("운영 DB 리비전 오류")?;
        if current!=revision{
            return Err("다른 작업에서 데이터가 수정되었습니다. 다시 불러오세요.".into());
        }
        let payload=bando_data_access::shipping_action::action_payload(
            action,&data,&before
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
        next["revision"]=serde_json::json!(revision);
        let committed=bando_data_access::write_v2_snapshot(
            &mut conn,next,revision,true
        )?;
        let committed_revision=committed["revision"].as_u64()
            .ok_or("운임 저장 결과 리비전을 확인하지 못했습니다.")?;
        drop(conn);
        // Never present an optimistic draft as confirmed saved data.
        let state=self.load_business_state()?;
        if state["revision"].as_u64()!=Some(committed_revision) {
            return Err(
                "운임은 저장됐지만 이후 다른 작업에서 데이터가 다시 수정되었습니다. 최신 상태를 불러와 확인하세요.".into()
            );
        }
        let projection=crate::management_queries::project_freight(self,&state)?;
        Ok(serde_json::json!({"state":state,"projection":projection}))
    }
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
            |"detach_shipment"|"regroup_shipment"
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
        let before=bando_data_access::read_v2_snapshot(&conn)?;
        if before["revision"].as_u64()!=Some(expected_revision){
            return Err("다른 작업에서 주문이 변경되었습니다. 새로고침 후 다시 진행하세요.".into());
        }
        let now=chrono::Utc::now().to_rfc3339();
        let event_id=uuid::Uuid::new_v4().to_string();
        let actor=before["settings"]["operatorName"].as_str()
            .filter(|name|!name.trim().is_empty())
            .unwrap_or("사용자(이름 미설정)").to_owned();
        let request=if group_delete {
            serde_json::json!({
                "state":&before,"action":action,
                "groupId":args["groupId"],"now":now,"eventId":event_id
            })
        }else{
            // Reconcile changes to prices, tariff signatures and late
            // corrections before attempting the transition. The native
            // reducer produces all immutable audit entries and validations.
            let count=(12*before["workflow"]["shipments"]
                .as_array().map(Vec::len).unwrap_or(0)
                +8*before["orders"].as_array().map(Vec::len).unwrap_or(0)+64)
                .max(256);
            if count>15000{
                return Err("재검증 이벤트 수가 허용 범위를 초과했습니다.".into());
            }
            let event_ids=(0..count).map(|_|
                uuid::Uuid::new_v4().to_string()).collect::<Vec<_>>();
            let reconciled=rules::execute("reconcile_workflow",serde_json::json!({
                "state":&before,"now":now,"eventIds":event_ids
            }))?;
            let prepared=reconciled.get("state")
                .ok_or("업무 재검증 결과에 상태가 없습니다.")?;
            let next_step=if action=="advance_group"{
                Some(rules::execute("evaluate_workflow",serde_json::json!({
                    "state":prepared,"groupId":args["groupId"]
                }))?.get("nextStep").and_then(Value::as_u64)
                    .ok_or("다음 진행 단계를 확인하지 못했습니다.")?)
            }else{None};
            let payment_message=if next_step==Some(3){
                Some(output::execute("render_payment_notice",
                    serde_json::json!({
                        "state":prepared,"groupId":args["groupId"]
                    })
                )?.get("payment").and_then(Value::as_str)
                    .ok_or("입금 안내 문구를 생성하지 못했습니다.")?.to_owned())
            }else{None};
            let audit_ids=(0..2048).map(|_|
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
                    "groupId"|"shipmentId"|"targetGroupId"|"ack"
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
        let op=if group_delete{"apply_group_action"}
            else{"apply_workflow_action"};
        let result=rules::execute(op,request)?;
        let changed=result.get("state")
            .ok_or("업무 처리 결과에 상태가 없습니다.")?.clone();
        rules::execute("validate_workflow",serde_json::json!({
            "state":&changed,"before":&before
        }))?;
        if changed==before{
            return Err("현재 상태에서 수행할 변경이 없습니다.".into());
        }
        // The same authoritative writer as Tauri: transactional revision
        // compare, relational order validation, audit and single commit.
        let written=bando_data_access::write_v2_snapshot(
            &mut conn,changed,expected_revision,true
        )?;
        let committed_revision=written["revision"].as_u64()
            .ok_or("업무 처리 결과의 revision이 없습니다.")?;
        drop(conn);
        let verified=self.load_business_state()?;
        if verified["revision"].as_u64()!=Some(committed_revision){
            return Err("저장 후 다른 작업에서 상태가 변경되었습니다. 다시 조회하세요.".into());
        }
        Ok(written)
    }
    fn working_commit_enabled(&self) -> bool {
        std::env::var("BANDO_EGUI_ENABLE_WORKING_COMMIT")
            .ok().as_deref()==Some("1") &&
        crate::business_state::authorized_business_write_path().is_ok()
    }
    fn commit_working_reception(&self, request: Value) -> Result<Value,String> {
        use rusqlite::{Connection,OpenFlags};
        if !self.working_commit_enabled() {
            return Err("운영 접수 저장에는 기존 업무 DB와 명시적인 쓰기 허용 설정이 필요합니다.".into());
        }
        let path=crate::business_state::authorized_business_write_path()?;
        let bytes=serde_json::to_vec(&request)
            .map_err(|_|"운영 접수 요청 형식이 잘못되었습니다.".to_owned())?;
        if bytes.is_empty() || bytes.len()>2_000_000 {
            return Err("운영 접수 요청 한도를 초과했습니다.".into());
        }
        let mut conn=Connection::open_with_flags(
            path,OpenFlags::SQLITE_OPEN_READ_WRITE
        ).map_err(|_|"업무 DB를 쓰기 모드로 열 수 없습니다.".to_owned())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e|e.to_string())?;
        // Reuse the original native recognition, manual-edit, repricing and
        // freight rules inside ONE IMMEDIATE SQLite working-reception commit.
        // This path cannot create confirmed orders or mutate financial history.
        let ack=bando_data_access::commit_reception_v2(&mut conn,&request)?;
        let id=ack["orderId"].as_str().ok_or("접수 결과 ID가 없습니다.")?;
        let persisted:String=conn.query_row(
            "SELECT payload FROM reception_working_orders WHERE id=?1",
            [id],|row|row.get(0),
        ).map_err(|_|"접수는 저장됐지만 실제 작업본을 재조회하지 못했습니다.".to_owned())?;
        let saved:Value=serde_json::from_str(&persisted)
            .map_err(|_|"운영 작업본 JSON 형식이 손상됐습니다.".to_owned())?;
        if saved["id"]!=ack["orderId"] ||
           saved["_eguiBridgeVersion"]!=ack["workingVersion"] ||
           saved["_eguiBridgeAuthorityRevision"]!=ack["authorityRevision"] ||
           saved["clientId"]!=ack["clientId"] ||
           saved["confirmedAt"].as_str().is_some_and(|value|!value.is_empty()) {
            return Err("접수는 저장됐지만 최신 작업본의 버전이나 내용이 다릅니다. 다시 조회하세요.".into());
        }
        Ok(ack)
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
