mod reception;
mod ocr;
mod order_image_inbox;
mod factory_conflict;
mod factory_reply_watch;
mod export_file;
mod kakao_dock;
mod developer_workspace;
mod startup_recovery;
mod backup_manager;
mod notification_store;
mod relational_store;
mod working_receptions;
#[cfg(test)]
mod egui_shipping_bridge;
mod sale_unit_migration;
mod product_name_enrichment;
mod business_core;
#[cfg(test)]
mod workflow_sqlite_integration_tests;
#[cfg(target_os = "windows")]
mod single_instance;
use chrono::{Datelike, Local, DateTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::Mutex};
use tauri::{Manager, State};
use tauri_plugin_window_state::StateFlags;

// The window-state plugin can restore the geometry of a maximized,
 // undecorated HWND while Win32 still considers it normal (IsZoomed == 0).
 // Apply the actual native maximize command once the window has been shown.
 // Only the BANDO main window with persisted maximized=true reaches this path.
#[cfg(target_os = "windows")]
mod native_maximized_restore {
    use std::ffi::c_void;

    // Win32 owns the real show command, not the current physical rectangle.
    // A frameless WebView can fill the monitor while IsZoomed is false.
    #[repr(C)]
    struct Point { x: i32, y: i32 }
    #[repr(C)]
    struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
    #[repr(C)]
    struct WindowPlacement {
        length: u32,
        flags: u32,
        show_cmd: u32,
        min_position: Point,
        max_position: Point,
        normal_position: Rect,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn ShowWindow(hwnd: *mut c_void, command: i32) -> i32;
        fn IsZoomed(hwnd: *mut c_void) -> i32;
        fn GetWindowPlacement(hwnd: *mut c_void, placement: *mut WindowPlacement) -> i32;
        fn SetWindowPlacement(hwnd: *mut c_void, placement: *const WindowPlacement) -> i32;
    }

    pub fn ensure_native_maximized(window: &tauri::Window) {
        let hwnd = match window.hwnd() {
            Ok(hwnd) => hwnd.0 as *mut c_void,
            Err(error) => {
                eprintln!("BANDO native maximize HWND unavailable: {error}");
                return;
            }
        };
        unsafe {
            let before = IsZoomed(hwnd) != 0;
            if !before {
                // First restore the *show command* as well as the maximized
                // rectangle. ShowWindow alone can be undone by WebView's
                // post-show SW_SHOWNORMAL on an undecorated main window.
                let mut placement: WindowPlacement = std::mem::zeroed();
                placement.length = std::mem::size_of::<WindowPlacement>() as u32;
                let restored = GetWindowPlacement(hwnd, &mut placement);
                if restored != 0 {
                    placement.show_cmd = 3; // SW_SHOWMAXIMIZED
                    if SetWindowPlacement(hwnd, &placement) == 0 {
                        eprintln!("BANDO native maximize SetWindowPlacement failed");
                    }
                }
                // ShowWindow's return value indicates previous visibility,
                // not whether the new zoom command succeeded.
                ShowWindow(hwnd, 3);
            }
            let after = IsZoomed(hwnd) != 0;
            if std::env::var_os("GITHUB_ACTIONS").is_some() {
                eprintln!("BANDO_WINDOW_RESTORE_NATIVE hwnd={hwnd:p} before={before} after={after}");
            }
            if !after {
                eprintln!("BANDO native maximize did not reach IsZoomed=true");
            }
        }
    }
}

struct Database { connection: Mutex<Connection>, base: PathBuf, is_test: bool }

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }
fn lock_db<'a>(db: &'a State<'_, Database>) -> Result<std::sync::MutexGuard<'a, Connection>, String> {
    db.connection.lock().map_err(|_| "데이터베이스 잠금 오류".to_string())
}
fn get_snapshot(conn: &Connection) -> Result<Value, String> {
    // Only schema v2 uses the shared projection. Legacy v0/v1 hydration
    // remains unchanged for non-destructive migrations and recovery.
    if relational_store::active(conn)? {
        return bando_data_access::read_v2_snapshot(conn);
    }
    let text: String = conn.query_row("SELECT payload FROM app_state WHERE id=1", [], |row| row.get(0)).map_err(err)?;
    let mut state: Value = serde_json::from_str(&text).map_err(err)?;
    relational_store::hydrate(conn, &mut state)?;
    Ok(state)
}
fn valid_state(s: &Value) -> Result<(), String> {
    bando_data_access::valid_state(s)
}
fn write_snapshot(conn: &mut Connection, mut state: Value, expected_revision: u64, validate_orders: bool) -> Result<Value, String> {
    // Schema v2 has one authoritative writer shared with the egui host.
    // Keep the original code below only for legacy/pre-migration databases.
    if relational_store::active(conn)? {
        return bando_data_access::write_v2_snapshot(conn,state,expected_revision,validate_orders);
    }
    valid_state(&state)?;
    let tx = conn.transaction().map_err(err)?;
    let current = get_snapshot(&tx)?;
    let current_revision = current["revision"].as_u64().unwrap_or(0);
    if current_revision != expected_revision {
        return Err("다른 작업에서 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    bando_data_access::validate_clients(&current, &state)?;
    bando_data_access::validate_brands(&current, &state)?;
    if validate_orders { reception::validate_reception(&current, &state)?; }
    let archive_entries = reception::new_archive_entries(&current, &state)?;
    let products_changed = current["products"] != state["products"];
    let clients_changed = current["clients"] != state["clients"];
    let orders_changed = current["orders"] != state["orders"];
    state["revision"] = json!(current_revision + 1);
    // The constrained SQLite tables own products, clients and orders.
    // Keep only non-core data in the compatibility envelope, atomically.
    let mut envelope = state.clone();
    if relational_store::active(&tx)? {
        // Settings-only changes never rewrite hundreds of product or order rows.
        if products_changed || clients_changed || orders_changed {
            relational_store::store(&tx, &state)?;
        }
        relational_store::strip_core(&mut envelope);
    }
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
        [serde_json::to_string(&envelope).map_err(err)?]).map_err(err)?;
    // Maintain legacy search indexes only when their actual domain changed.
    if products_changed {
    tx.execute("DELETE FROM product_index", []).map_err(err)?;
    {
        let mut insert = tx.prepare("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)").map_err(err)?;
        for product in state["products"].as_array().ok_or("상품 데이터 오류")? {
            let id = product["id"].as_str().ok_or("상품 ID 누락")?;
            let code = product["code"].as_str().ok_or("상품 코드 누락")?;
            let name = product["name"].as_str().ok_or("상품명 누락")?;
            insert.execute(params![id, code, name, serde_json::to_string(product).map_err(err)?]).map_err(err)?;
        }
    }
    }
    // Order index and the append-only reception archive share the snapshot transaction.
    if orders_changed {
    tx.execute("DELETE FROM order_index", []).map_err(err)?;
    {
        let mut insert = tx.prepare("INSERT INTO order_index(id,client_id,created_at,confirmed_at,status) VALUES (?1,?2,?3,?4,?5)").map_err(err)?;
        for order in state["orders"].as_array().ok_or("주문 데이터 오류")? {
            insert.execute(params![
                order["id"].as_str().ok_or("주문 ID 누락")?,
                order["clientId"].as_str().unwrap_or(""),
                order["createdAt"].as_str().unwrap_or(""),
                order["confirmedAt"].as_str(),
                order["status"].as_str().unwrap_or("REVIEW")
            ]).map_err(err)?;
        }
    }
    }
    for (id, order_id, record) in archive_entries {
        // Never silently discard a colliding audit ID with a different owner or payload.
        let stored: Option<(String, String)> = tx.query_row(
            "SELECT order_id,record FROM receipt_archive WHERE id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(err)?;
        if let Some((stored_owner, stored_payload)) = stored {
            let stored_record: Value = serde_json::from_str(&stored_payload).map_err(err)?;
            if stored_owner != order_id || stored_record != record {
                return Err("기존 원문 감사 기록과 ID가 충돌합니다. 변경을 저장하지 않았습니다.".into());
            }
        } else {
            tx.execute(
                "INSERT INTO receipt_archive(id,order_id,record) VALUES (?1,?2,?3)",
                params![id,order_id,serde_json::to_string(&record).map_err(err)?],
            ).map_err(err)?;
        }
    }
    tx.commit().map_err(err)?;
    Ok(state)
}
// v2 and legacy snapshots share identical brand/client validation through
// bando-data-access. No second host-owned copy of these rules.
fn validate_accounts(state:&Value)->Result<(),String>{
    business_core::rules::settings_accounts::validate(state)
}

fn initialize_database(path: &Path) -> Result<Connection, String> {
    let mut conn = Connection::open(path).map_err(err)?;
    relational_store::backup_legacy_if_needed(&conn,path)?;
    conn.execute_batch(r#"
        PRAGMA journal_mode=WAL;
        PRAGMA busy_timeout=5000;
        PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS app_state(
            id INTEGER PRIMARY KEY CHECK(id=1),payload TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS product_index(
            id TEXT PRIMARY KEY,code TEXT NOT NULL,name TEXT NOT NULL,payload TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_product_code ON product_index(code);
        CREATE INDEX IF NOT EXISTS idx_product_name ON product_index(name);
        CREATE TABLE IF NOT EXISTS order_index(
            id TEXT PRIMARY KEY,client_id TEXT NOT NULL,created_at TEXT NOT NULL,
            confirmed_at TEXT,status TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_order_created ON order_index(created_at);
        CREATE INDEX IF NOT EXISTS idx_order_client ON order_index(client_id);
        CREATE TABLE IF NOT EXISTS receipt_archive(
            id TEXT PRIMARY KEY,order_id TEXT NOT NULL,record TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_receipt_order ON receipt_archive(order_id);
    "#).map_err(err)?;
    // Independent notification table: additive, idempotent, and intentionally
    // does not increment or rewrite the business snapshot revision.
    notification_store::initialize(&conn)?;
    // Persist reception edits independently from the confirmed-order archive.
    // The whole SQLite DB (including this table) belongs to normal backups.
    // The Windows DEV smoke must compile both exposed commands and this
    // initializer: preview registration must never substitute for SQLite.
    working_receptions::initialize(&conn)?;
    let exists: Option<i64> = conn.query_row("SELECT 1 FROM app_state WHERE id=1", [], |r| r.get(0)).optional().map_err(err)?;
    if exists.is_none() {
        let seed: Value = serde_json::from_str(include_str!("../resources/state-seed.json")).map_err(err)?;
        valid_state(&seed)?;
        // Independent 360 sale-unit seed. Do not open or migrate the Electron test database.
        conn.execute("INSERT INTO app_state(id,payload) VALUES (1,?1)", [serde_json::to_string(&seed).map_err(err)?]).map_err(err)?;
        // Current revision starts at 0. Populate the same product index without creating fake test records.
        let transaction = conn.transaction().map_err(err)?;
        {
            let mut insert = transaction.prepare("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)").map_err(err)?;
            for product in seed["products"].as_array().ok_or("상품 초기 데이터 누락")? {
                insert.execute(params![
                    product["id"].as_str().ok_or("상품 ID 누락")?,
                    product["code"].as_str().ok_or("상품 코드 누락")?,
                    product["name"].as_str().ok_or("상품명 누락")?,
                    serde_json::to_string(product).map_err(err)?
                ]).map_err(err)?;
            }
        }
        transaction.commit().map_err(err)?;
    }
    // Upgrade v0.0.1 in place. The app ID and SQLite filename remain unchanged;
    // previous staff-entered product prices and other local data are retained.
    if conn.query_row("SELECT COUNT(*) FROM order_index", [], |r| r.get::<_, i64>(0)).map_err(err)? == 0 {
        let existing = get_snapshot(&conn)?;
        for order in existing["orders"].as_array().ok_or("주문 데이터 오류")? {
            conn.execute("INSERT OR IGNORE INTO order_index(id,client_id,created_at,confirmed_at,status) VALUES (?1,?2,?3,?4,?5)", params![
                order["id"].as_str().ok_or("주문 ID 누락")?,
                order["clientId"].as_str().unwrap_or(""),
                order["createdAt"].as_str().unwrap_or(""),
                order["confirmedAt"].as_str(),
                order["status"].as_str().unwrap_or("REVIEW")
            ]).map_err(err)?;
            if let (Some(id),Some(receipt))=(order["id"].as_str(),order.get("receipt")) {
                if order["confirmedAt"].as_str().is_some() {
                    conn.execute("INSERT OR IGNORE INTO receipt_archive(id,order_id,record) VALUES (?1,?2,?3)",
                        params![format!("receipt:{id}"),id,serde_json::to_string(receipt).map_err(err)?]).map_err(err)?;
                }
            }
            if let Some(order_id)=order["id"].as_str() {
                for correction in order["rawCorrections"].as_array().into_iter().flatten() {
                    if let Some(correction_id)=correction["id"].as_str() {
                        conn.execute("INSERT OR IGNORE INTO receipt_archive(id,order_id,record) VALUES (?1,?2,?3)",
                            params![format!("correction:{correction_id}"),order_id,serde_json::to_string(correction).map_err(err)?]).map_err(err)?;
                    }
                }
            }
        }
    }
    relational_store::migrate(&mut conn)?;
    let data_root = path.parent().ok_or("상품명 변경 백업 경로를 확인할 수 없습니다.")?;
    product_name_enrichment::apply(&mut conn, data_root)?;
    // One-time, backed-up migration of the obsolete invoice sender override.
    // It touches only live client rows, never historic order snapshots.
    bando_data_access::client_names::migrate_v2(&mut conn,data_root)?;
    relational_store::verify(&conn)?;
    Ok(conn)
}
// Pure Rust domain computation only: returning a new workflow state does
// NOT write SQLite. Existing checked state_save/transaction boundaries must
// commit approved results against a fresh persisted revision.
// Recognition scans the catalog and can be CPU intensive. Run it outside the
// Tauri UI thread so continuous reception typing remains responsive.
#[tauri::command]
async fn business_core_call(domain: String, operation: String, payload: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        business_core::execute(&domain, &operation, payload)
    })
    .await
    .map_err(|_| "Rust 업무 엔진 실행이 중단되었습니다.".to_owned())?
}

#[tauri::command]
fn state_load(db: State<'_, Database>) -> Result<Value, String> { get_snapshot(&*lock_db(&db)?) }

#[tauri::command]
fn state_save(db: State<'_, Database>, state: Value) -> Result<Value, String> {
    if db.is_test {
        let protected_root = canonical_excel_directory(&db.base.join("Excel"))?;
        let submitted = state["settings"]["excelDirectory"].as_str()
            .ok_or("테스트 Excel 보관함 정보가 누락되었습니다.")?;
        if canonical_excel_directory(Path::new(submitted))? != protected_root {
            return Err("테스트 작업공간 Excel 보관함은 변경할 수 없습니다.".into());
        }
    }
    let revision = state["revision"].as_u64().ok_or("변경 이력이 없는 데이터를 저장할 수 없습니다.")?;
    // The committed snapshot already contains the new revision.
    write_snapshot(&mut *lock_db(&db)?, state, revision, true)
}

fn canonical_color_key(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "black" | "검정" | "검정색" => "블랙".into(),
        "silver" => "실버".into(),
        "gray" | "grey" | "회색" => "그레이".into(),
        "white" | "흰색" => "화이트".into(),
        "brown" => "브라운".into(),
        "chrome" => "크롬".into(),
        "red" => "레드".into(),
        "blue" => "블루".into(),
        "gold" => "골드".into(),
        other => other.to_owned(),
    }
}

fn validate_color_guidance(state: &Value) -> Result<(), String> {
    let Some(config) = state["settings"].get("colorGuidanceGroups") else { return Ok(()); };
    let groups = config.as_object().ok_or("대표색상 그룹 저장 형식 오류")?;
    let products = state["products"].as_array().ok_or("상품 목록 누락")?;
    let mut names = std::collections::HashSet::new();
    let mut assigned = std::collections::HashSet::new();
    for (id, group) in groups {
        let name = group["name"].as_str().unwrap_or("").trim();
        if id.trim().is_empty() || name.is_empty() {
            return Err("모델군 이름을 입력하세요.".into());
        }
        if !names.insert(name.to_lowercase()) { return Err("모델군 이름이 중복됩니다.".into()); }
        let members = group["productIds"].as_array().ok_or("소속 상품 형식 오류")?;
        if members.len() < 2 { return Err("모델군에는 상품을 2개 이상 등록하세요.".into()); }

        // The settings key is kept for SQLite compatibility. New groups are
        // defined by independent product IDs, not shared color, price or size.
        if let Some(representative) = group.get("representativeProductId") {
            let selected = representative.as_str().unwrap_or("").trim();
            if selected.is_empty() {
                return Err("모델군에 등록된 실제 상품을 대표모델로 선택하세요.".into());
            }
            let mut local = std::collections::HashSet::new();
            for member in members {
                let pid = member.as_str().ok_or("상품 ID 오류")?;
                if !local.insert(pid.to_owned()) || !assigned.insert(pid.to_owned()) {
                    return Err("같은 상품을 중복하거나 여러 모델군에 등록할 수 없습니다.".into());
                }
                products.iter().find(|p|
                    p["id"].as_str() == Some(pid)
                    && p["active"] != false
                    && p["deletedAt"].as_str().is_none()
                    && p["supersededBy"].as_str().is_none()
                    && p["catalog"].as_str() != Some("DEMO")
                    && p["variants"].as_array().is_some_and(|variants|
                        variants.len() == 1 && variants[0]["source"].as_str() != Some("DEMO")))
                    .ok_or("활성 독립 판매 상품만 모델군에 등록할 수 있습니다.")?;
            }
            if !local.contains(selected) {
                return Err("대표모델은 모델군에 속한 등록 상품 중 하나여야 합니다.".into());
            }
            continue;
        }

        // Legacy color-only entries remain readable until an employee
        // explicitly migrates them by choosing a representative product.
        let color = group["representativeColor"].as_str().unwrap_or("").trim();
        if color.is_empty() || color == "단일" || color == "색상 미표기" {
            return Err("기존 대표색상 그룹에 유효한 색상이 없습니다.".into());
        }
        let mut colors = std::collections::HashSet::new();
        let mut price_reference: Option<(Value,Value,Value)> = None;
        let mut price_known=false;
        for member in members {
            let pid = member.as_str().ok_or("상품 ID 오류")?;
            if !assigned.insert(pid.to_owned()) {
                return Err("같은 상품은 두 대표색상 그룹에 등록할 수 없습니다.".into());
            }
            let p=products.iter().find(|p| p["id"].as_str() == Some(pid)
                && p["active"] != false && p["deletedAt"].as_str().is_none()
                && p["supersededBy"].as_str().is_none()
                && p["variants"].as_array().is_some_and(|v| v.len() == 1))
                .ok_or("활성 독립 상품만 그룹에 등록할 수 있습니다.")?;
            let v=&p["variants"][0];
            let color=canonical_color_key(v["color"].as_str().unwrap_or(""));
            if color.is_empty() || color=="단일" || color=="색상 미표기" {
                return Err("실제 색상이 등록된 상품만 그룹에 등록하세요.".into());
            }
            if !colors.insert(color) { return Err("같은 색상이 중복됐습니다. 규격을 확인하세요.".into()); }
            let price=(v["wholesale"].clone(),v["semi"].clone(),v["retail"].clone());
            price_known |= !price.0.is_null() || !price.1.is_null() || !price.2.is_null();
            if let Some(reference)=&price_reference {
                if reference != &price { return Err("색상별 가격이 다른 상품은 같은 그룹에 등록할 수 없습니다.".into()); }
            } else { price_reference=Some(price); }
        }
        if !price_known { return Err("비교할 가격이 없는 상품은 그룹에 등록할 수 없습니다.".into()); }
        if !colors.contains(&canonical_color_key(color)) {
            return Err("대표색상은 등록된 상품의 색상에서 선택하세요.".into());
        }
    }
    Ok(())
}


// Message and provisional-product validation belong to bando-data-access.
// Save just the edited settings keys. Order/product updates do not invalidate
// this edit, but a simultaneous edit of the very same settings key is rejected.
fn apply_settings_patch(
    conn: &mut Connection, is_test: bool, changes: &Value, expected: &Value
) -> Result<Value, String> {
    // Schema v2 writes use the SAME native Rust transaction as egui.
    // Older databases keep Tauri's migration-compatible snapshot writer,
    // but the field CAS and all business rules are still shared.
    if relational_store::active(conn)? {
        return bando_data_access::patch_settings_v2(conn,is_test,changes,expected);
    }
    let state=get_snapshot(conn)?;
    if changes.as_object().is_some_and(serde_json::Map::is_empty){
        return Ok(state);
    }
    let revision=state["revision"].as_u64().ok_or("운영 DB revision 오류")?;
    let updated=bando_data_access::apply_settings_changes(
        state,is_test,changes,expected
    )?;
    write_snapshot(conn,updated,revision,true)
}

/// React/Tauri and native egui share the same settings validation,
/// per-field CAS and revision-checked v2 SQLite writer.
#[tauri::command]
fn settings_patch(
    db: State<'_, Database>, changes: Value, expected: Value
) -> Result<Value,String>{
    let mut conn=lock_db(&db)?;
    apply_settings_patch(&mut conn,db.is_test,&changes,&expected)
}

/// One authoritative CAS implementation for React price_set and the
/// explicitly enabled egui CLI. All pricing/storage semantics stay here.
#[tauri::command]
fn price_set(db: State<'_, Database>, product_id: String, variant_id: String, tier: String,
             price: Option<i64>, expected: Option<i64>) -> Result<Value, String> {
    let mut conn=lock_db(&db)?;
    let mut state=get_snapshot(&conn)?;
    let revision=state["revision"].as_u64().unwrap_or(0);
    bando_data_access::apply_product_price(&mut state,&product_id,&variant_id,&tier,price,expected)?;
    write_snapshot(&mut conn,state,revision,true)
}

#[tauri::command]
fn clipboard_write(text: String) -> Result<(), String> {
    if text.chars().count() > 100000 { return Err("복사할 문장이 너무 깁니다.".into()); }
    arboard::Clipboard::new().map_err(err)?.set_text(text).map_err(err)
}
#[tauri::command]
fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new().map_err(err)?.get_text().map_err(err)
}

#[tauri::command]
fn product_open(target: String) -> Result<(), String> {
    let url = url::Url::parse(&target).map_err(err)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("HTTPS 상품 링크만 열 수 있습니다.".into());
    }
    open::that(target).map_err(err)
}

#[tauri::command]
fn catalog_report() -> Result<Value, String> {
    let mut data: Value = serde_json::from_str(include_str!("../resources/catalog.json")).map_err(err)?;
    data["sourceProductCount"] = json!(data["products"].as_array().map(|x| x.len()).unwrap_or(0));
    Ok(data)
}

// The old catalog approval matched on product IDs and could create or overwrite
// variants without human review. Keep this compatibility command intentionally
// disabled. The reviewed UI uses revision-checked state_save after a backup.
#[tauri::command]
fn catalog_approve(_db: State<'_, Database>) -> Result<Value, String> {
    Err("이전 방식의 일괄 승인은 안전하지 않아 중지되었습니다. 상품 동기화에서 옵션별 미리보기를 사용하세요.".into())
}

#[tauri::command]
fn receipt_archive(db: State<'_, Database>) -> Result<Value, String> {
    let conn = lock_db(&db)?;
    receipt_archive_from_conn(&conn)
}

/// Same archive projection for both React IPC and the egui read-only CLI.
/// Includes legacy/unverified records without fabricating audit evidence.
fn receipt_archive_from_conn(conn:&Connection)->Result<Value,String>{
    bando_data_access::audit_service::receipt_archive_from_conn(conn)
}

#[tauri::command]
fn backup_create(db: State<'_, Database>) -> Result<String, String> {
    let conn = lock_db(&db)?;
    backup_manager::manual_backup(&conn, &db.base)
}
#[tauri::command]
fn backup_list(db: State<'_, Database>) -> Result<Vec<String>, String> {
    backup_manager::list_backups(&db.base)
}
#[tauri::command]
fn backup_restore(db: State<'_, Database>, filename: String) -> Result<(), String> {
    // Hold the live DB mutex for the full preflight, safety copy, and restore.
    // Another app command cannot mutate the DB between those operations.
    let mut conn = lock_db(&db)?;
    backup_manager::restore_backup(&mut conn, &db.base, &filename)?;
    // Restoring a historic v2 backup may reintroduce displayName. Convert
    // only live clients; leave the restored order/audit snapshots intact.
    bando_data_access::client_names::migrate_v2(&mut conn,&db.base)?;
    Ok(())
}

// The native picker returns the path. JavaScript cannot send an arbitrary
// filesystem path to the command that opens the configured Excel directory.
fn canonical_excel_directory(path: &Path) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|_| "선택한 폴더에 접근할 수 없습니다. 폴더가 존재하는지 확인하세요.".to_string())?;
    if !resolved.is_dir() { return Err("폴더만 선택할 수 있습니다.".into()); }
    Ok(resolved)
}
#[tauri::command]
async fn choose_excel_directory(app: tauri::AppHandle, db: State<'_, Database>) -> Result<Option<String>, String> {
    if db.is_test {
        let selected = db.base.join("Excel");
        fs::create_dir_all(&selected).map_err(err)?;
        return Ok(Some(canonical_excel_directory(&selected)?.to_string_lossy().into_owned()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let selected = app.dialog().file().set_title("Excel 보관함 위치 선택").blocking_pick_folder();
        let Some(selected) = selected else { return Ok(None); };
        let picked = selected.into_path().map_err(err)?;
        let verified = canonical_excel_directory(&picked)?;
        // Create the five top-level business folders under this one user-picked root.
        for label in ["공장 발송", "매장 발송", "공장 회신", "상품 관리", "거래처 관리"] {
            let child = verified.join(label);
            fs::create_dir_all(&child).map_err(err)?;
            if !fs::canonicalize(&child).map_err(err)?.starts_with(&verified) {
                return Err("Excel 폴더가 선택한 보관함 밖을 가리킵니다.".into());
            }
        }
        Ok(Some(verified.to_string_lossy().to_string()))
    }).await.map_err(err)?
}
fn workspace_excel_directory(db: &Database, settings: &Value) -> Result<PathBuf, String> {
    if db.is_test {
        return canonical_excel_directory(&db.base.join("Excel"));
    }
    saved_excel_directory(settings)
}
fn saved_excel_directory(settings: &Value) -> Result<PathBuf, String> {
    let chosen = settings.get("excelDirectory").and_then(Value::as_str)
        .map(str::trim).filter(|s| !s.is_empty())
        .ok_or("설정에서 Excel 보관함 위치를 먼저 지정하고 저장하세요.")?;
    canonical_excel_directory(Path::new(chosen))
}
#[tauri::command]
fn open_excel_directory(db: State<'_, Database>) -> Result<(), String> {
    let saved = get_snapshot(&*lock_db(&db)?)?;
    let directory = workspace_excel_directory(&db, &saved["settings"])?;
    open::that(directory).map_err(err)
}


// Date folders are only addressed by a fixed output category. Never accept
// a filesystem path from the frontend for opening an Explorer window.
#[tauri::command]
fn open_daily_excel_directory(db: State<'_, Database>, kind: String) -> Result<(), String> {
    let workbook_kind = match kind.as_str() {
        "FACTORY" => WorkbookKind::Factory,
        "STORE" => WorkbookKind::Store,
        _ => return Err("공장 또는 매장 Excel 폴더만 열 수 있습니다.".into()),
    };
    let state = get_snapshot(&*lock_db(&db)?)?;
    let root = workspace_excel_directory(&db, &state["settings"])?;
    let today = archive_daily_directory(&root, workbook_kind, Local::now())?;
    open::that(today).map_err(err)
}

#[derive(Debug, Clone, Copy)]
enum WorkbookKind {
    Factory,
    Store,
    FactoryReply,
    Product,
    ProductTemplate,
    Client,
    ProductPriceLegacy,
}
impl WorkbookKind {
    fn folder(self) -> &'static str {
        match self {
            Self::Factory => "공장 발송",
            Self::Store => "매장 발송",
            Self::FactoryReply => "공장 회신",
            Self::Product | Self::ProductTemplate | Self::ProductPriceLegacy => "상품 관리",
            Self::Client => "거래처 관리",
        }
    }
    fn stem(self) -> &'static str {
        match self {
            Self::Factory => "공장",
            Self::Store => "매장",
            Self::FactoryReply => "공장회신",
            Self::Product => "상품전체",
            Self::ProductTemplate => "상품등록양식",
            Self::Client => "거래처전체",
            Self::ProductPriceLegacy => "상품가격표",
        }
    }
}
fn archive_daily_directory(root: &Path, kind: WorkbookKind, date: DateTime<Local>) -> Result<PathBuf, String> {
    let dir = root.join(kind.folder())
        .join(format!("{}년", date.year()))
        .join(format!("{:02}월", date.month()))
        .join(format!("{:02}일", date.day()));
    fs::create_dir_all(&dir).map_err(err)?;
    let resolved = fs::canonicalize(&dir).map_err(err)?;
    // Reject linked directories that escape the user-selected archive root.
    if !resolved.starts_with(root) { return Err("Excel 보관함 경로가 설정한 폴더 밖을 가리킵니다.".into()); }
    Ok(resolved)
}
fn archive_workbook_bytes(root: &Path, kind: WorkbookKind, date: DateTime<Local>, bytes: &[u8])
    -> Result<PathBuf, String> {
    use std::io::Write;
    if bytes.len() < 4 || bytes.len() > 25 * 1024 * 1024 || !bytes.starts_with(b"PK") {
        return Err("올바른 XLSX 데이터가 아니거나 파일 크기가 너무 큽니다.".into());
    }
    let dir = archive_daily_directory(root, kind, date)?;
    let stamp = format!("{:04}{:02}{:02}", date.year(), date.month(), date.day());
    // create_new guarantees atomic, no-overwrite numbering across concurrent workers.
    for number in 1..=100_000u32 {
        let path = dir.join(format!("{stamp}({}{number}).xlsx", kind.stem()));
        let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(err(error)),
        };
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(err(error));
        }
        return Ok(path);
    }
    Err("당일 Excel 파일 번호가 허용 범위를 초과했습니다.".into())
}
async fn save_xlsx_in_archive(
    app: tauri::AppHandle, kind: WorkbookKind, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Database>();
        let root = {
            let guard = db.connection.lock().map_err(|_| "데이터베이스 잠금 오류")?;
            let state = get_snapshot(&guard)?;
            workspace_excel_directory(&db, &state["settings"])?
        };
        let saved = archive_workbook_bytes(&root, kind, Local::now(), &bytes)?;
        Ok(Some(saved.to_string_lossy().into_owned()))
    }).await.map_err(err)?
}

// Only application-generated XLSX bytes are accepted; the destination is chosen
// in a native Save As dialog. No filesystem path is accepted from JavaScript.
async fn save_xlsx_with_dialog(
    app: tauri::AppHandle,
    label: &'static str,
    bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    if bytes.len() < 4 || bytes.len() > 25 * 1024 * 1024 || !bytes.starts_with(b"PK") {
        return Err("올바른 XLSX 데이터가 아니거나 파일 크기가 너무 큽니다.".into());
    }
    if app.state::<Database>().is_test {
        return Err("테스트 공간에서는 보고서의 외부 파일 저장을 사용할 수 없습니다.".into());
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<Option<String>, String> {
        use tauri_plugin_dialog::DialogExt;
        use std::io::Write;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?.as_millis();
        let filename = format!("BANDO-HUB-{label}-{stamp}.xlsx");
        let preferred = {
            let db = app.state::<Database>();
            let guard = db.connection.lock().map_err(|_| "데이터베이스 잠금 오류")?;
            let settings = get_snapshot(&guard)?;
            saved_excel_directory(&settings["settings"]).ok()
        };
        let mut dialog = app.dialog().file()
            .set_title("Excel 파일 저장 위치 선택")
            .set_file_name(filename)
            .add_filter("Excel 통합 문서", &["xlsx"]);
        if let Some(path) = preferred {
            dialog = dialog.set_directory(path);
        }
        let selected = dialog.blocking_save_file();
        let Some(selected) = selected else { return Ok(None); };
        let mut destination = selected.into_path().map_err(err)?;
        if destination.extension().is_none() {
            destination.set_extension("xlsx");
        }
        if !destination.extension().and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xlsx")) {
            return Err("Excel 파일은 .xlsx 확장자로만 저장할 수 있습니다.".into());
        }
        // Do not overwrite an existing workbook even if the file dialog points at it.
        let mut file = fs::OpenOptions::new()
            .write(true).create_new(true).open(&destination).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "동일한 이름의 파일이 이미 있습니다. 다른 이름으로 저장해 주세요.".into()
                } else {
                    err(error)
                }
            })?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&destination);
            return Err(err(error));
        }
        Ok(Some(destination.to_string_lossy().into_owned()))
    }).await.map_err(err)?
}

#[tauri::command]
async fn save_product_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_in_archive(app, WorkbookKind::ProductPriceLegacy, bytes).await
}

// The renderer supplies only a SHA-256 ID. No path from JavaScript is trusted.
fn reply_digest(id: &str) -> Result<&str, String> {
    let digest = id.strip_prefix("factory-reply-")
        .ok_or("공장 회신 식별자가 올바르지 않습니다.")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err("공장 회신 식별자가 올바르지 않습니다.".into());
    }
    Ok(digest)
}

#[tauri::command]
fn archive_factory_reply(db: State<'_, Database>, id: String, bytes: Vec<u8>) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let digest = reply_digest(&id)?;
    if bytes.len() < 4 || bytes.len() > 10 * 1024 * 1024 || !bytes.starts_with(b"PK") {
        return Err("10MB 이하의 정상적인 XLSX 파일만 보관할 수 있습니다.".into());
    }
    if format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err("선택한 공장 회신 파일과 검토한 파일이 일치하지 않습니다.".into());
    }
    let directory = db.base.join("factory-replies");
    fs::create_dir_all(&directory).map_err(err)?;
    let destination = directory.join(format!("{digest}.xlsx"));
    if destination.exists() {
        if fs::read(&destination).map_err(err)? != bytes {
            return Err("보관 파일과 같은 식별자의 파일 내용이 일치하지 않습니다.".into());
        }
    } else {
        let mut file = fs::OpenOptions::new().write(true).create_new(true)
            .open(&destination).map_err(err)?;
        if let Err(e) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&destination);
            return Err(err(e));
        }
    }
    // Preserve the digest-named original in private app data for audit and
    // conflict review. The user-facing date-classified copy is not the audit authority.
    let root = {
        let guard = lock_db(&db)?;
        let state = get_snapshot(&guard)?;
        workspace_excel_directory(&db, &state["settings"])?
    };
    archive_workbook_bytes(&root, WorkbookKind::FactoryReply, Local::now(), &bytes)?;
    Ok(destination.to_string_lossy().into_owned())
}

// Open only a previously recorded XLSX export. Never accept a frontend path.
#[tauri::command]
fn open_export_file(db: State<'_, Database>, id: String) -> Result<(), String> {
    let snapshot = get_snapshot(&*lock_db(&db)?)?;
    let file = export_file::resolve_export_path(&snapshot, &id)?;
    open::that(file).map_err(err)
}

// Open the directory of a stored XLSX export; never accept a raw path from UI.
#[tauri::command]
fn open_export_directory(db: State<'_, Database>, id: String) -> Result<(), String> {
    let snapshot = get_snapshot(&*lock_db(&db)?)?;
    let folder = export_file::resolve_export_directory(&snapshot, &id)?;
    open::that(folder).map_err(err)
}

#[tauri::command]
fn open_factory_reply_file(db: State<'_, Database>, id: String) -> Result<(), String> {
    let digest = reply_digest(&id)?;
    let data = get_snapshot(&*db.connection.lock().map_err(|_| "DB 잠금 오류")?)?;
    if !data["factoryReplies"].as_array().is_some_and(|rows|
        rows.iter().any(|row| row["id"].as_str() == Some(id.as_str()) &&
            row["archivedPath"].as_str().is_some())) {
        return Err("보관된 공장 회신 파일을 찾지 못했습니다.".into());
    }
    let path = db.base.join("factory-replies").join(format!("{digest}.xlsx"));
    if !path.is_file() { return Err("보관된 회신 파일이 존재하지 않습니다.".into()); }
    open::that(path).map_err(err)
}

#[tauri::command]
async fn save_settlement_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_with_dialog(app, "일일결산", bytes).await
}

#[tauri::command]
async fn save_order_history_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_with_dialog(app, "주문내역", bytes).await
}

#[tauri::command]
async fn save_factory_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_in_archive(app, WorkbookKind::Factory, bytes).await
}

#[tauri::command]
async fn save_store_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_in_archive(app, WorkbookKind::Store, bytes).await
}

#[tauri::command]
async fn save_product_sync_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_in_archive(app, WorkbookKind::Product, bytes).await
}

#[tauri::command]
async fn save_client_workbook(
    app: tauri::AppHandle, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    save_xlsx_in_archive(app, WorkbookKind::Client, bytes).await
}

#[tauri::command]
async fn save_registration_workbook(
    app: tauri::AppHandle, kind: String, bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    if kind != "양식" {
        return Err("상품 등록 결과는 반도허브 화면에서 확인하세요.".into());
    }
    save_xlsx_in_archive(app, WorkbookKind::ProductTemplate, bytes).await
}

fn validate_official_product_url(target: &str) -> Result<url::Url, String> {
    if target.len() > 2048 {
        return Err("상품 URL이 너무 깁니다.".into());
    }
    let url = url::Url::parse(target).map_err(|_| "상품 상세 URL을 확인하세요.".to_string())?;
    if url.scheme() != "https" ||
        url.port().is_some() ||
        !url.username().is_empty() ||
        url.password().is_some() ||
        !matches!(url.host_str(), Some("bandostand.com" | "www.bandostand.com" |
            "bandomall.co.kr" | "www.bandomall.co.kr")) {
        return Err("반도스탠드 또는 반도몰의 HTTPS 상품 상세 주소만 허용됩니다.".into());
    }
    let segments: Vec<&str> = url.path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect())
        .unwrap_or_default();
    let is_cafe24 = url.path() == "/product/detail.html" &&
        url.query_pairs().any(|(key, value)| key == "product_no" &&
            !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()));
    let is_seo = segments.len() >= 3 && segments[0] == "product" &&
        segments[1] != "detail.html" && segments[2].chars().all(|c| c.is_ascii_digit());
    if !is_cafe24 && !is_seo {
        return Err("상품번호가 포함된 공식 상세페이지 URL을 사용하세요.".into());
    }
    Ok(url)
}

#[cfg(test)]
mod official_url_tests {
    use super::validate_official_product_url;

    #[test]
    fn accepts_cafe24_and_seo_product_urls() {
        assert!(validate_official_product_url(
            "https://bandostand.com/product/detail.html?product_no=42"
        ).is_ok());
        assert!(validate_official_product_url(
            "https://www.bandostand.com/product/stand-bd-701/54/"
        ).is_ok());
        assert!(validate_official_product_url(
            "https://bandomall.co.kr/product/guitar-string/3449/category/1/display/1/"
        ).is_ok());
    }
    #[test]
    fn rejects_non_official_and_unsafe_urls() {
        for item in [
            "http://bandostand.com/product/x/42/",
            "https://bandostand.com.evil.example/product/x/42/",
            "https://127.0.0.1/product/x/42/",
            "https://user:pass@bandostand.com/product/x/42/",
            "https://bandostand.com:8443/product/x/42/",
            "https://bandostand.com/product/detail.html?product_no=foo",
            "https://bandostand.com/shopinfo/guide.html",
            "https://bandostand.com/product/no-number/",
        ] {
            assert!(validate_official_product_url(item).is_err(), "{item}");
        }
    }
}

// Read-only official product fetch. URL validation and redirect blocking prevent
// arbitrary network requests from the renderer (including local-network targets).
#[tauri::command]
async fn fetch_official_product_html(target: String) -> Result<String, String> {
    const MAX_BODY: usize = 4 * 1024 * 1024;
    let url = validate_official_product_url(&target)?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("BANDO-HUB/0.0.4 (official product preview)")
        .build().map_err(err)?;
    let mut response = client.get(url).send().await.map_err(|e| format!("공식 홈페이지 연결 실패: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("상품 페이지를 읽지 못했습니다 (HTTP {}).", response.status()));
    }
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("");
    if !content_type.to_ascii_lowercase().starts_with("text/html") &&
        !content_type.to_ascii_lowercase().starts_with("application/xhtml+xml") {
        return Err("상품 페이지가 HTML 형식이 아닙니다.".into());
    }
    if response.content_length().is_some_and(|length| length > MAX_BODY as u64) {
        return Err("상품 페이지가 허용 크기를 초과했습니다.".into());
    }
    let mut body = Vec::new();
    while let Some(part) = response.chunk().await.map_err(err)? {
        if body.len().saturating_add(part.len()) > MAX_BODY {
            return Err("상품 페이지가 허용 크기를 초과했습니다.".into());
        }
        body.extend_from_slice(&part);
    }
    String::from_utf8(body).map_err(|_| "현재 페이지의 문자 인코딩을 안전하게 해석할 수 없습니다.".into())
}


fn start_automatic_backup(app: tauri::AppHandle) {
    let handle = app;
            std::thread::spawn(move || {
                let db = handle.state::<Database>();
                match db.connection.lock() {
                    Ok(conn) => {
                        if let Err(reason) = backup_manager::automatic_backup_if_needed(
                            &conn, &db.base, Local::now().date_naive()) {
                            eprintln!("반도허브 자동 백업 실패 (다음 실행 시 재시도): {reason}");
                        }
                    }
                    Err(reason) => eprintln!("반도허브 자동 백업 DB 잠금 실패: {reason}"),
                };
            });
}

pub fn run() {
    // The production app and developer TEST app may run together.
    // Never let their last window placements overwrite one another.
    let developer_test = std::env::args().skip(1)
        .any(|arg| arg == "--bando-developer-test");
    // Acquire before Tauri constructs any window or connects to SQLite.
    // The isolated developer TEST process uses a separate named guard so
    // the production app and its test lab may coexist.
    #[cfg(target_os = "windows")]
    let instance = match single_instance::acquire(developer_test) {
        Ok(single_instance::Acquisition::First(guard)) => guard,
        Ok(single_instance::Acquisition::Existing) => return,
        Err(error) => panic!("반도허브 중복 실행 검사 실패: {error}"),
    };
    tauri::Builder::default()
        .manage(kakao_dock::Dock::default())
        .manage(factory_reply_watch::InboxWatch::default())
        .manage(order_image_inbox::OrderImageWatch::default())
        .plugin(tauri_plugin_dialog::init())
        // Restore only the BANDO HUB main window. External KakaoTalk windows
        // are managed separately by kakao_dock and must never be persisted here.
        // Tauri tracks the last non-maximized bounds, monitor placement and
        // maximized state. VISIBLE also shows the initially hidden window only
        // after restoration, preventing a default-size flash at launch.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_filename(if developer_test {
                    "window-state-developer.json"
                } else {
                    "window-state.json"
                })
                .with_filter(|label| label == "main")
                .with_state_flags(
                    StateFlags::SIZE | StateFlags::POSITION |
                    StateFlags::MAXIMIZED | StateFlags::VISIBLE,
                )
                .build(),
        )
        // The window-state plugin restores SIZE/POSITION and shows the hidden
        // main HWND. On Windows, show() may undo an early maximize. Retrying
        // restore_state() synchronously in another window-ready callback races
        // with that show. Defer one maximize to the native event-loop queue.
        .plugin(
            tauri::plugin::Builder::<_, ()>::new("bando-maximize-after-window-state")
                .on_window_ready(move |window| {
                    if !cfg!(target_os = "windows") || window.label() != "main" {
                        return;
                    }
                    // Use the persisted decision, not transient HWND state
                    // or a cache modified by the move/resize events at launch.
                    let filename = if developer_test {
                        "window-state-developer.json"
                    } else {
                        "window-state.json"
                    };
                    let restore_maximized = window.app_handle().path().app_config_dir()
                        .ok()
                        .and_then(|directory| fs::read(directory.join(filename)).ok())
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                        .and_then(|saved| saved.get("main")?
                            .get("maximized")?.as_bool())
                        .unwrap_or(false);
                    if std::env::var_os("GITHUB_ACTIONS").is_some() {
                        eprintln!("BANDO_WINDOW_RESTORE_SAVED maximize={restore_maximized} filename={filename}");
                    }
                    if !restore_maximized {
                        return;
                    }
                    // Window-ready and the first native ShowWindow happen in
                    // different event-loop turns. Queuing an immediate
                    // run_on_main_thread still happens too early and later
                    // ShowWindow can reset the Win32 SW_MAXIMIZE flag.
                    // Apply it after the initial show has completed, without
                    // delaying the UI thread or touching persisted geometry.
                    let ready_window = window.clone();
                    let app = window.app_handle().clone();
                    std::thread::spawn(move || {
                        // The plugin already restores saved MAXIMIZED on
                        // window-ready. Reconcile only twice after initial
                        // show; do not keep overriding a user's normal restore.
                        for (pass, delay_ms) in [(1, 650u64), (2, 1750u64)] {
                            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                            if std::env::var_os("GITHUB_ACTIONS").is_some() {
                                eprintln!("BANDO_WINDOW_RESTORE_DISPATCH pass={pass}");
                            }
                            let candidate = ready_window.clone();
                            if let Err(error) = app.run_on_main_thread(move || {
                                #[cfg(target_os = "windows")]
                                native_maximized_restore::ensure_native_maximized(&candidate);
                            }) {
                                eprintln!("BANDO post-show maximize scheduling failed: {error}");
                                break;
                            }
                        }
                    });
                })
                .build(),
        )
        .setup(move |app| {
            #[cfg(target_os = "windows")]
            single_instance::listen(instance, app.handle().clone())
                .map_err(std::io::Error::other)?;
            // Keep the native window alive on initialization failures. The
            // managed StartupRecovery owns the diagnosis until a guarded retry
            // successfully registers a Database; no failed database is managed.
            let is_test = developer_test;
            let startup = startup_recovery::StartupRecovery::new(is_test);
            startup_recovery::initial_open(app.handle(), &startup);
            app.manage(startup);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            state_load, state_save, business_core_call, settings_patch, price_set, clipboard_write, clipboard_read,
            working_receptions::working_receptions_load, working_receptions::working_receptions_replace,
            ocr::capture_ocr, ocr::clipboard_ocr, ocr::cancel_ocr,
            order_image_inbox::choose_order_image_inbox_directory,
            order_image_inbox::open_order_image_inbox_directory,
            order_image_inbox::order_image_watch_scan, order_image_inbox::order_image_ocr,
            notification_store::hub_notifications_list,
            notification_store::hub_notifications_add,
            notification_store::hub_notifications_mark_read,
            notification_store::hub_notifications_delete,
            notification_store::hub_notifications_clear,
            product_open, catalog_report, catalog_approve, receipt_archive, backup_create, backup_list, backup_restore, choose_excel_directory, open_excel_directory, open_daily_excel_directory,
            factory_reply_watch::choose_factory_reply_directory,
            factory_reply_watch::factory_reply_watch_scan,
            factory_reply_watch::factory_reply_watch_commit,
            save_product_workbook, save_registration_workbook, save_client_workbook, save_product_sync_workbook, save_factory_workbook, save_store_workbook, save_settlement_workbook, save_order_history_workbook, archive_factory_reply, open_factory_reply_file, open_export_file, open_export_directory, factory_conflict::approve_factory_conflict,
            fetch_official_product_html,
            kakao_dock::dock_windows_native, kakao_dock::dock_status_native,
            kakao_dock::dock_cycle_native, kakao_dock::dock_mode_native, kakao_dock::dock_start_native,
            kakao_dock::dock_select_native, kakao_dock::dock_stop_native,
            kakao_dock::dock_region_native,
            startup_recovery::startup_status,
            startup_recovery::startup_metrics,
            startup_recovery::retry_startup,
            developer_workspace::workspace_mode,
            developer_workspace::open_test_workspace,
            developer_workspace::test_database_preview,
            developer_workspace::reset_test_database,
            developer_workspace::test_record_run,
            developer_workspace::test_reception_boundary,
            developer_workspace::test_run_history
        ])
        .build(tauri::generate_context!())
        .expect("BANDO HUB Tauri runtime build error")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                kakao_dock::restore_on_exit(&app.state::<kakao_dock::Dock>());
            }
        });
}


#[cfg(test)]
mod receipt_sqlite_integration_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path() -> PathBuf {
        let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("bando-receipt-ci-{}-{id}.sqlite", std::process::id()))
    }

    fn seed() -> Value {
        json!({
            "revision": 0,
            "products": [{
                "id": "p-709", "code": "709B", "name": "709B",
                "active": true, "variants": [{ "id": "v-black" }]
            }],
            "orders": [],
            "clients": [{
                "id": "client-1", "name": "테스트 거래처",
                "address": "서울시 테스트로 1", "phone": "02-1234-5678",
                "mobile": "", "tier": "wholesale"
            }],
            "aliases": [], "queue": [], "exports": [],
            "settings": { "accounts": [], "operatorName": "검증 담당자" },
            "workflow": { "shipments": [], "events": [] }
        })
    }

    fn open_db(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(r#"
            CREATE TABLE app_state(id INTEGER PRIMARY KEY, payload TEXT NOT NULL);
            CREATE TABLE product_index(id TEXT PRIMARY KEY, code TEXT NOT NULL,
                name TEXT NOT NULL, payload TEXT NOT NULL);
            CREATE TABLE order_index(id TEXT PRIMARY KEY, client_id TEXT NOT NULL,
                created_at TEXT NOT NULL, confirmed_at TEXT, status TEXT NOT NULL);
            CREATE TABLE receipt_archive(id TEXT PRIMARY KEY, order_id TEXT NOT NULL,
                record TEXT NOT NULL);
        "#).unwrap();
        conn.execute("INSERT INTO app_state(id,payload) VALUES (1,?1)",
            [seed().to_string()]).unwrap();
        conn
    }

    fn confirmed(base: &Value) -> Value {
        let mut next = base.clone();
        let at = "2026-11-07T12:00:00+09:00";
        let raw = "709B 블랙 2개 / 수취인 김테스트";
        let recipients = json!([{
            "id": "r-1", "name": "김테스트",
            "items": [{ "id": "i-1", "productId": "p-709",
                "variantId": "v-black", "quantity": 2 }]
        }]);
        let mut order = json!({
            "id": "order-1", "receptionVersion": 1,
            "clientId": "client-1", "orderType": "AGENCY",
            "raw": raw, "createdAt": at, "confirmedAt": at,
            "status": "STATEMENT", "recipients": recipients,
            "receipt": { "original": raw, "at": at, "actor": "검증 담당자",
                "clientId": "client-1", "legacy": false },
            "rawCorrections": []
        });
        order["confirmationSnapshot"] = json!({
            "at": at, "raw": raw, "client": next["clients"][0].clone(),
            "recipients": recipients, "data": order.clone()
        });
        next["orders"] = json!([order]);
        next["workflow"] = json!({
            "shipments": [{
                "id": "ship-1", "orderId": "order-1",
                "recipientId": "r-1", "step": 1
            }],
            "events": [{ "action": "주문 확인 완료", "shipmentIds": ["ship-1"] }]
        });
        next
    }

    fn correction(id: &str) -> Value {
        json!({
            "id": id, "at": "2026-11-07T12:05:00+09:00",
            "actor": "검증 담당자", "reason": "붙여넣기 오타",
            "before": "709B 블랙 2개 / 수취인 김테스트",
            "after": "709B 블랙 3개 / 수취인 김테스트",
            "affectedRecipientIds": ["r-1"]
        })
    }

    fn count_archive(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM receipt_archive", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn confirmed_receipt_and_correction_survive_sqlite_reopen() {
        let path = test_path();
        let mut conn = open_db(&path);
        let first = write_snapshot(&mut conn, confirmed(&seed()), 0, true).unwrap();
        assert_eq!(first["revision"], 1);
        assert_eq!(count_archive(&conn), 1);
        let original: String = conn.query_row(
            "SELECT record FROM receipt_archive WHERE id='receipt:order-1'",
            [], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&original).unwrap()["original"],
            "709B 블랙 2개 / 수취인 김테스트");

        let mut edited = first.clone();
        edited["orders"][0]["analysisRaw"] = json!("709B 블랙 3개 / 수취인 김테스트");
        edited["orders"][0]["rawCorrections"] = json!([correction("fix-1")]);
        let second = write_snapshot(&mut conn, edited, 1, true).unwrap();
        assert_eq!(second["revision"], 2);
        assert_eq!(count_archive(&conn), 2);

        // Saving unrelated settings must not replay or duplicate any receipt event.
        let mut unrelated = second.clone();
        unrelated["settings"]["uiScale"] = json!(110);
        let third = write_snapshot(&mut conn, unrelated, 2, true).unwrap();
        assert_eq!(third["revision"], 3);
        assert_eq!(count_archive(&conn), 2);
        drop(conn);

        let reopened = Connection::open(&path).unwrap();
        let restored = get_snapshot(&reopened).unwrap();
        assert_eq!(restored["revision"], 3);
        assert_eq!(restored["orders"][0]["raw"],
            "709B 블랙 2개 / 수취인 김테스트");
        assert_eq!(restored["orders"][0]["analysisRaw"],
            "709B 블랙 3개 / 수취인 김테스트");
        assert_eq!(restored["orders"][0]["receipt"]["original"],
            "709B 블랙 2개 / 수취인 김테스트");
        assert_eq!(restored["orders"][0]["rawCorrections"].as_array().unwrap().len(), 1);
        assert_eq!(count_archive(&reopened), 2);
        let actual_correction: String = reopened.query_row(
            "SELECT record FROM receipt_archive WHERE id='correction:fix-1'",
            [], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&actual_correction).unwrap()["reason"],
            "붙여넣기 오타");
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tampered_original_and_colliding_audit_id_rollback_entire_save() {
        let path = test_path();
        let mut conn = open_db(&path);
        let first = write_snapshot(&mut conn, confirmed(&seed()), 0, true).unwrap();

        let mut tampered = first.clone();
        tampered["orders"][0]["raw"] = json!("위조된 최초 원문");
        assert!(write_snapshot(&mut conn, tampered, 1, true).unwrap_err()
            .contains("최초 원문"));
        assert_eq!(get_snapshot(&conn).unwrap(), first);
        assert_eq!(count_archive(&conn), 1);

        // Simulate an ID already held by a different archive owner.
        conn.execute(
            "INSERT INTO receipt_archive(id,order_id,record) VALUES (?1,?2,?3)",
            params!["correction:collision", "other-order", r#"{"unexpected":true}"#]
        ).unwrap();
        let mut edited = first.clone();
        edited["orders"][0]["analysisRaw"] = json!("709B 블랙 3개 / 수취인 김테스트");
        edited["orders"][0]["rawCorrections"] = json!([correction("collision")]);
        assert!(write_snapshot(&mut conn, edited, 1, true).unwrap_err().contains("충돌"));
        assert_eq!(get_snapshot(&conn).unwrap(), first);
        assert_eq!(count_archive(&conn), 2);
        let original_order_index: String = conn.query_row(
            "SELECT id FROM order_index WHERE id='order-1'", [], |r| r.get(0)).unwrap();
        assert_eq!(original_order_index, "order-1");

        drop(conn);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stale_revision_and_incomplete_draft_never_commit_audit() {
        let path = test_path();
        let mut conn = open_db(&path);
        let mut draft = seed();
        draft["orders"] = json!([{
            "id":"draft-1", "receptionVersion":1, "raw":"709 2개"
        }]);
        assert!(write_snapshot(&mut conn, draft, 0, true).unwrap_err().contains("주문 확인 완료"));
        assert_eq!(count_archive(&conn), 0);
        let committed = write_snapshot(&mut conn, confirmed(&seed()), 0, true).unwrap();
        assert_eq!(committed["revision"], 1);
        assert!(write_snapshot(&mut conn, committed.clone(), 0, true).unwrap_err()
            .contains("다른 작업"));
        assert_eq!(get_snapshot(&conn).unwrap(), committed);
        assert_eq!(count_archive(&conn), 1);
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod excel_folder_tests {
    use super::*;
    #[test]
    fn requires_saved_folder_and_rejects_files() {
        assert!(saved_excel_directory(&json!({})).is_err());
        assert!(saved_excel_directory(&json!({"excelDirectory": "  "})).is_err());
        let cargo = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(canonical_excel_directory(&cargo).is_err());
        assert!(canonical_excel_directory(&cargo.with_file_name("missing-folder-bando-2026")).is_err());
    }
    #[test]
    fn resolves_saved_directory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = saved_excel_directory(&json!({"excelDirectory": root.to_string_lossy()})).unwrap();
        assert_eq!(dir, fs::canonicalize(root).unwrap());
    }
}


#[cfg(test)]
mod dated_archive_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn factory_and_store_names_increment_without_overwriting_and_reset_by_day() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let folder = std::env::temp_dir()
            .join(format!("bando-archive-{}-{unique}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let root = canonical_excel_directory(&folder).unwrap();
        let day1 = Local.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).single().unwrap();
        let day2 = Local.with_ymd_and_hms(2026, 11, 8, 12, 0, 0).single().unwrap();
        let bytes = b"PK\x03\x04synthetic-test-workbook";
        let first = archive_workbook_bytes(&root, WorkbookKind::Factory, day1.clone(), bytes).unwrap();
        let second = archive_workbook_bytes(&root, WorkbookKind::Factory, day1.clone(), bytes).unwrap();
        let store = archive_workbook_bytes(&root, WorkbookKind::Store, day1.clone(), bytes).unwrap();
        let next_day = archive_workbook_bytes(&root, WorkbookKind::Factory, day2, bytes).unwrap();
        assert_eq!(first.file_name().unwrap().to_str(), Some("20261107(공장1).xlsx"));
        assert_eq!(second.file_name().unwrap().to_str(), Some("20261107(공장2).xlsx"));
        assert_eq!(store.file_name().unwrap().to_str(), Some("20261107(매장1).xlsx"));
        assert_eq!(next_day.file_name().unwrap().to_str(), Some("20261108(공장1).xlsx"));
        assert!(first.to_string_lossy().contains("공장 발송"));
        assert!(first.to_string_lossy().contains("2026년"));
        assert!(first.to_string_lossy().contains("11월"));
        assert!(first.to_string_lossy().contains("07일"));
        assert_eq!(fs::read(&first).unwrap(), bytes);
        assert_eq!(fs::read(&second).unwrap(), bytes);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn todays_factory_and_store_folders_are_separate_and_safe_to_create() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let folder = std::env::temp_dir()
            .join(format!("bando-today-folder-{}-{unique}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let root = canonical_excel_directory(&folder).unwrap();
        let date = Local.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).single().unwrap();
        let factory = archive_daily_directory(&root, WorkbookKind::Factory, date.clone()).unwrap();
        let store = archive_daily_directory(&root, WorkbookKind::Store, date).unwrap();
        assert_ne!(factory, store);
        assert!(factory.is_dir());
        assert!(store.is_dir());
        assert!(factory.starts_with(&root));
        assert!(store.starts_with(&root));
        assert!(factory.ends_with(Path::new("공장 발송/2026년/11월/13일")));
        assert!(store.ends_with(Path::new("매장 발송/2026년/11월/13일")));
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn templates_clients_and_replies_share_root_without_sharing_name_sequence() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let folder = std::env::temp_dir()
            .join(format!("bando-archive-kinds-{}-{unique}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let root = canonical_excel_directory(&folder).unwrap();
        let date = Local.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).single().unwrap();
        let bytes = b"PK\x03\x04synthetic-test-workbook";
        for (kind, expected) in [
            (WorkbookKind::Product, "20261107(상품전체1).xlsx"),
            (WorkbookKind::ProductTemplate, "20261107(상품등록양식1).xlsx"),
            (WorkbookKind::FactoryReply, "20261107(공장회신1).xlsx"),
            (WorkbookKind::Client, "20261107(거래처전체1).xlsx"),
        ] {
            let path = archive_workbook_bytes(&root, kind, date.clone(), bytes).unwrap();
            assert_eq!(path.file_name().unwrap().to_str(), Some(expected));
            assert!(path.starts_with(&root));
        }
        fs::remove_dir_all(&folder).unwrap();
    }
}

#[cfg(test)]
mod representative_model_group_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn same_color_and_different_price_products_can_share_a_model_group() {
        let state = json!({
            "settings": {"colorGuidanceGroups": {
                "staff": {
                    "name": "709 타공 / 비타공",
                    "productIds": ["p709", "p709-1", "pglow"],
                    "representativeProductId": "p709"
                }
            }},
            "products": [
                {"id":"p709","active":true,
                 "variants":[{"color":"Black","retail":59000,"wholesale":null,"semi":null}]},
                {"id":"p709-1","active":true,
                 "variants":[{"color":"Black","retail":59000,"wholesale":null,"semi":null}]},
                {"id":"pglow","active":true,
                 "variants":[{"color":"색상 미표기","retail":7000,"wholesale":null,"semi":null}]}
            ]
        });
        assert!(validate_color_guidance(&state).is_ok());
        let mut wrong = state.clone();
        wrong["settings"]["colorGuidanceGroups"]["staff"]["representativeProductId"] =
            json!("unknown");
        assert!(validate_color_guidance(&wrong).unwrap_err().contains("대표모델"));
        wrong["settings"]["colorGuidanceGroups"]["staff"]["representativeProductId"] =
            json!("p709");
        wrong["settings"]["colorGuidanceGroups"]["staff"]["productIds"] =
            json!(["p709", "p709"]);
        assert!(validate_color_guidance(&wrong).is_err());
    }
}
