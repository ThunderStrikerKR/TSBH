//! Strictly isolated local SQLite storage for egui development.
//! This database is NOT a migration of BANDO HUB's Tauri business database.
//! Never open, import or modify the operational app_state/core_* tables.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::HashSet, fs, path::PathBuf, time::Duration};

const SIGNATURE: &str = "bando-hub-egui-preview-v1";
const SOURCE_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Full OCR recognition and employee-approved correction, retained locally
/// for an auditable future pattern-review process. Not an auto-learning rule.
#[derive(Clone, Debug)]
pub struct OcrFeedback {
    pub id: String,
    pub order_id: String,
    pub original_text: String,
    pub corrected_text: String,
    /// Whether the employee enabled local learned suggestions on this review.
    pub learning_used: bool,
    /// Proposed result before employee edits (None if no patterns applied).
    pub model_suggestion: Option<String>,
}

/// Compute reproducible, candidate line changes. The two full documents are
/// authoritative: this digest is never treated as an automatic correction.
/// LCS is bounded so pasted 20k-character documents cannot force an
/// unbounded allocation. Large documents retain full text + a fallback digest.
fn correction_changes(before: &str, after: &str) -> Value {
    let old: Vec<&str> = before.split('\n').collect();
    let new: Vec<&str> = after.split('\n').collect();
    let n = old.len();
    let m = new.len();
    if n.saturating_mul(m) > 40_000 {
        return json!({
            "algorithm":"line-lcs-v1", "truncated":true,
            "originalLines":n, "correctedLines":m,
            "same":before==after,
        });
    }
    let mut dp = vec![vec![0usize; m+1]; n+1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old[i]==new[j] {
                dp[i+1][j+1]+1
            } else {dp[i+1][j].max(dp[i][j+1])};
        }
    }
    let (mut i,mut j)=(0usize,0usize);
    let mut changes=Vec::new();
    while i<n || j<m {
        if i<n && j<m && old[i]==new[j] {
            i+=1; j+=1;
        } else if i<n && (j==m || dp[i+1][j]>=dp[i][j+1]) {
            changes.push(json!({"kind":"delete","originalLine":i+1,
                "before":old[i]}));
            i+=1;
        } else if j<m {
            changes.push(json!({"kind":"insert","correctedLine":j+1,
                "after":new[j]}));
            j+=1;
        }
    }
    json!({"algorithm":"line-lcs-v1","truncated":false,
        "same":before==after,"changes":changes})
}

/// Key-level delta of the SAME Rust parser on original and corrected
/// OCR text. Full JSON snapshots are retained separately for future audit.
fn compare_ocr_analyses(initial:&Value,corrected:&Value)->Value{
    let mut changed=Vec::<String>::new();
    fn visit(a:&Value,b:&Value,path:&str,depth:usize,changes:&mut Vec<String>){
        if changes.len()>=150 || a==b {return;}
        if depth>=8 {changes.push(path.to_owned());return;}
        if let (Some(x),Some(y))=(a.as_object(),b.as_object()){
            let keys=x.keys().map(String::as_str)
                .chain(y.keys().map(String::as_str))
                .collect::<HashSet<_>>();
            for key in keys {
                if changes.len()>=150 {break;}
                visit(x.get(key).unwrap_or(&Value::Null),
                    y.get(key).unwrap_or(&Value::Null),
                    &format!("{path}/{key}"),depth+1,changes);
            }
        }else if let (Some(x),Some(y))=(a.as_array(),b.as_array()){
            if x.len()!=y.len(){changes.push(format!("{path}/length"));}
            for (i,(a,b)) in x.iter().zip(y.iter()).enumerate(){
                if changes.len()>=150{break;}
                visit(a,b,&format!("{path}/{i}"),depth+1,changes);
            }
        }else {
            changes.push(path.to_owned());
        }
    }
    visit(initial,corrected,"",0,&mut changed);
    changed.sort();
    json!({
        "schema":"ocr-rust-analysis-diff-v1",
        "analysisChanged":initial!=corrected,
        "changedPaths":changed,
        "initialRecipientCount":initial.get("recipients")
            .and_then(Value::as_array).map(Vec::len),
        "correctedRecipientCount":corrected.get("recipients")
            .and_then(Value::as_array).map(Vec::len),
    })
}

#[derive(Clone, Debug)]
pub struct WorkingReception {
    pub id: String,
    pub raw: String,
    pub order_type: String,
    /// Route of the FIRST source content; None means legacy/unknown.
    /// Adding OCR later never overwrites the original route.
    pub source_input_method: Option<String>,
    /// Most recent imported input route, even when sources were appended.
    pub last_input_method: Option<String>,
    /// Rust evidence is valid only for this exact source string.
    pub analysis_raw: Option<String>,
    pub analysis_evidence: Option<Value>,
    /// Advisory Rust dealer decision, never a confirmed client assignment.
    pub client_match: Option<Value>,
    /// Verified on each use against the current native chat list.
    pub chat_handle: Option<String>,
    pub chat_title: Option<String>,
    /// Isolated preview selection; never a confirmed order-client edit.
    pub client_id: Option<String>,
    pub client_selection: Option<String>, // AUTO or MANUAL
    /// Small, source-bound pricing/freight result of the real Rust rules.
    /// Never store the native Order or the full live client/state here.
    pub pricing_preview: Option<Value>,
    /// Cross-process CAS token from the native Tauri working-reception row.
    /// This is not an order-confirmation status.
    pub working_version: Option<u64>,
    /// True only when current raw, kind and client match the last native save.
    pub working_synced: bool,
    /// The employee-approved native manual model for THIS exact raw/kind/
    /// dealer. The full Tauri Order and master client details are excluded.
    pub manual_model: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct WorkingSource {
    pub raw: String,
    pub order_type: String,
    pub revision: u64,
    pub active_id: Option<String>,
    pub drafts: Vec<WorkingReception>,
}

pub struct PreviewStore {
    path: PathBuf,
}

impl PreviewStore {
    pub fn initialize() -> Result<Self, String> {
        let root = if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            PathBuf::from(local)
        } else if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            return Err("egui 전용 DB 저장 폴더를 찾지 못했습니다.".into());
        };
        let directory = root.join("BANDO-HUB-egui-preview");
        fs::create_dir_all(&directory)
            .map_err(|e| format!("egui 전용 DB 폴더를 만들지 못했습니다: {e}"))?;
        let path = directory.join("egui-preview.sqlite3");
        let store = Self { path };
        let existed = store.path.exists();
        let conn = store.open_connection()?;
        if existed {
            let signature: Option<String> = conn.query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='preview_meta'",
                [],
                |r| r.get(0),
            ).optional().map_err(|e| e.to_string())?;
            if signature.is_none() {
                return Err("기존 egui DB 파일의 구조가 다릅니다. 자동 덮어쓰기를 중단했습니다.".into());
            }
            let stored: Option<String> = conn.query_row(
                "SELECT value FROM preview_meta WHERE key='signature'",
                [],
                |r| r.get(0),
            ).optional().map_err(|e| e.to_string())?;
            if stored.as_deref() != Some(SIGNATURE) {
                return Err("egui 전용 DB 식별자가 다릅니다. 자동 마이그레이션하지 않습니다.".into());
            }
        } else {
            let initialization = (|| {
                conn.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE preview_meta (
                     key TEXT PRIMARY KEY, value TEXT NOT NULL
                 );
                 CREATE TABLE catalog_snapshot (
                     id INTEGER PRIMARY KEY CHECK(id=1),
                     payload TEXT NOT NULL CHECK(json_valid(payload))
                 );
                 CREATE TABLE working_source (
                     id INTEGER PRIMARY KEY CHECK(id=1),
                     raw TEXT NOT NULL,
                     order_type TEXT NOT NULL DEFAULT 'AGENCY'
                       CHECK(order_type IN ('AGENCY','DEALER')),
                     revision INTEGER NOT NULL CHECK(revision >= 0),
                     active_id TEXT NOT NULL DEFAULT '',
                     drafts_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(drafts_json)),
                     updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );"
                ).map_err(|e| e.to_string())?;
                let full: Value = serde_json::from_str(
                    include_str!("../../src-tauri/resources/state-seed.json")
                ).map_err(|e| format!("내장 상품 시드가 손상되었습니다: {e}"))?;
                let catalog = json!({
                    "products": full["products"],
                    "aliases": full["aliases"],
                    "brands": full["brands"],
                });
                if !catalog["products"].is_array() || !catalog["aliases"].is_array() {
                    return Err("내장 상품 목록이 올바르지 않습니다.".into());
                }
                conn.execute(
                    "INSERT INTO preview_meta(key,value) VALUES('signature',?1)",
                    [SIGNATURE],
                ).map_err(|e| e.to_string())?;
                conn.execute(
                    "INSERT INTO catalog_snapshot(id,payload) VALUES(1,?1)",
                    [catalog.to_string()],
                ).map_err(|e| e.to_string())?;
                conn.execute(
                    "INSERT INTO working_source(id,raw,order_type,revision)
                     VALUES(1,'','AGENCY',0)",
                    [],
                ).map_err(|e| e.to_string())?;
                conn.execute_batch("COMMIT;").map_err(|e| e.to_string())?;
                Ok::<_, String>(())
            })();
            if let Err(error) = initialization {
                let _ = conn.execute_batch("ROLLBACK;");
                drop(conn);
                // Only a file that did NOT exist before this initialization
                // can be discarded. Never remove an existing database.
                let _ = fs::remove_file(&store.path);
                return Err(error);
            }
        }
        // Private, append-only OCR verification corpus. Retain BOTH full
        // texts so false correction rules can be independently re-audited.
        // No screenshot, external upload, or runtime model training is stored.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ocr_feedback (
                id TEXT PRIMARY KEY,
                order_id TEXT NOT NULL,
                original_text TEXT NOT NULL,
                corrected_text TEXT NOT NULL,
                changes_json TEXT NOT NULL CHECK(json_valid(changes_json)),
                recorded_at TEXT NOT NULL DEFAULT
                    (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                learning_used INTEGER NOT NULL DEFAULT 0,
                model_suggestion TEXT,
                initial_analysis_json TEXT CHECK(initial_analysis_json IS NULL
                    OR json_valid(initial_analysis_json)),
                corrected_analysis_json TEXT CHECK(corrected_analysis_json IS NULL
                    OR json_valid(corrected_analysis_json)),
                analysis_comparison_json TEXT CHECK(analysis_comparison_json IS NULL
                    OR json_valid(analysis_comparison_json)),
                analysis_status TEXT NOT NULL DEFAULT 'pending',
                analysis_error TEXT,
                analyzer_label TEXT,
                catalog_revision TEXT,
                analyzed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_ocr_feedback_order
                ON ocr_feedback(order_id,recorded_at);"
        ).map_err(|e| format!("OCR 정정 이력 테이블 준비 실패: {e}"))?;
        // Non-destructive migration of corpus rows already collected by
        // earlier 65th-session builds; NEVER drop original/corrected text.
        let mut stmt=conn.prepare("PRAGMA table_info(ocr_feedback)")
            .map_err(|e|e.to_string())?;
        let columns=stmt.query_map([],|row|row.get::<_,String>(1))
            .map_err(|e|e.to_string())?
            .collect::<Result<HashSet<_>,_>>().map_err(|e|e.to_string())?;
        drop(stmt);
        let additions=[
            ("learning_used","INTEGER NOT NULL DEFAULT 0"),
            ("model_suggestion","TEXT"),
            ("initial_analysis_json","TEXT"),
            ("corrected_analysis_json","TEXT"),
            ("analysis_comparison_json","TEXT"),
            ("analysis_status","TEXT NOT NULL DEFAULT 'pending'"),
            ("analysis_error","TEXT"),
            ("analyzer_label","TEXT"),
            ("catalog_revision","TEXT"),
            ("analyzed_at","TEXT"),
        ];
        for (name,definition) in additions {
            if !columns.contains(name) {
                conn.execute_batch(&format!(
                    "ALTER TABLE ocr_feedback ADD COLUMN {name} {definition}"
                )).map_err(|e|format!("OCR 학습 이력 확장 실패 ({name}): {e}"))?;
            }
        }
        // Non-destructive migration of the isolated egui preview database.
        // Never touch Tauri operational tables or truncate an existing source.
        let mut stmt = conn.prepare("PRAGMA table_info(working_source)")
            .map_err(|e| e.to_string())?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        if !columns.iter().any(|name| name == "order_type") {
            conn.execute_batch(
                "ALTER TABLE working_source ADD COLUMN order_type TEXT
                 NOT NULL DEFAULT 'AGENCY'
                 CHECK(order_type IN ('AGENCY','DEALER'))"
            ).map_err(|e| format!("egui 접수 유형 마이그레이션 실패: {e}"))?;
        }
        if !columns.iter().any(|name| name == "active_id") {
            conn.execute_batch(
                "ALTER TABLE working_source ADD COLUMN active_id TEXT NOT NULL DEFAULT ''"
            ).map_err(|e| format!("egui 활성 접수 마이그레이션 실패: {e}"))?;
        }
        if !columns.iter().any(|name| name == "drafts_json") {
            conn.execute_batch(
                "ALTER TABLE working_source ADD COLUMN drafts_json TEXT NOT NULL DEFAULT '[]'"
            ).map_err(|e| format!("egui 접수 목록 마이그레이션 실패: {e}"))?;
        }
        Ok(store)
    }

    fn open_connection(&self) -> Result<Connection, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn = Connection::open_with_flags(&self.path, flags)
            .map_err(|e| format!("egui 전용 SQLite 접근 오류: {e}"))?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
            .map_err(|e| e.to_string())?;
        Ok(conn)
    }

    pub fn load_catalog(&self) -> Result<Value, String> {
        let conn = self.open_connection()?;
        let serialized: String = conn.query_row(
            "SELECT payload FROM catalog_snapshot WHERE id=1", [], |r| r.get(0)
        ).map_err(|e| format!("egui 카탈로그를 읽지 못했습니다: {e}"))?;
        let catalog: Value = serde_json::from_str(&serialized)
            .map_err(|e| format!("egui 카탈로그 JSON 오류: {e}"))?;
        if !catalog["products"].is_array()
            || !catalog["aliases"].is_array()
            || !catalog["brands"].is_array()
        {
            return Err("egui 상품·별칭·브랜드 데이터 형식이 올바르지 않습니다.".into());
        }
        Ok(catalog)
    }

    pub fn load_source(&self) -> Result<WorkingSource, String> {
        let conn = self.open_connection()?;
        let (raw, order_type, rev, active, encoded): (String, String, i64, String, String) =
            conn.query_row(
                "SELECT raw,order_type,revision,active_id,drafts_json
                 FROM working_source WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            ).map_err(|e| format!("egui 접수 원문을 복원하지 못했습니다: {e}"))?;
        if !matches!(order_type.as_str(), "AGENCY" | "DEALER") {
            return Err("egui 저장된 접수 유형이 잘못되었습니다.".into());
        }
        let revision = u64::try_from(rev)
            .map_err(|_| "egui 저장 revision이 잘못되었습니다.".to_owned())?;
        let values: Value = serde_json::from_str(&encoded)
            .map_err(|e| format!("egui 접수 목록 형식 오류: {e}"))?;
        let rows = values.as_array()
            .ok_or("egui 접수 목록은 JSON 배열이어야 합니다.")?;
        let mut drafts = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.get("id").and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or("저장된 접수 ID가 잘못되었습니다.")?;
            let source = row.get("raw").and_then(Value::as_str)
                .ok_or("저장된 접수 원문이 잘못되었습니다.")?;
            let kind = row.get("orderType").and_then(Value::as_str)
                .ok_or("저장된 접수 유형이 잘못되었습니다.")?;
            if !matches!(kind, "AGENCY" | "DEALER") {
                return Err("저장된 접수 유형이 잘못되었습니다.".into());
            }
            let analysis_raw = row.get("analysisRaw").and_then(Value::as_str)
                .map(str::to_owned);
            let analysis_evidence = row.get("analysisEvidence")
                .filter(|value| !value.is_null()).cloned();
            let client_match = row.get("clientMatch")
                .filter(|value| !value.is_null()).cloned();
            drafts.push(WorkingReception {
                id: id.to_owned(), raw: source.to_owned(), order_type: kind.to_owned(),
                source_input_method: row.get("sourceInputMethod").and_then(Value::as_str).map(str::to_owned),
                last_input_method: row.get("lastInputMethod").and_then(Value::as_str).map(str::to_owned),
                analysis_raw, analysis_evidence, client_match,
                chat_handle: row.get("chatHandle").and_then(Value::as_str).map(str::to_owned),
                chat_title: row.get("chatTitle").and_then(Value::as_str).map(str::to_owned),
                client_id: row.get("clientId").and_then(Value::as_str).map(str::to_owned),
                client_selection: row.get("clientSelection").and_then(Value::as_str).map(str::to_owned),
                pricing_preview: row.get("pricingPreview").filter(|v| !v.is_null()).cloned(),
                working_version: row.get("workingVersion").and_then(Value::as_u64),
                working_synced: row.get("workingSynced").and_then(Value::as_bool).unwrap_or(false),
                manual_model: row.get("manualModel").filter(|v|!v.is_null()).cloned(),
            });
        }
        // Preserve a previous single-source editing session without changing
        // its SQLite row. A subsequent explicit edit migrates it atomically.
        if drafts.is_empty() && !raw.is_empty() {
            drafts.push(WorkingReception {
                id: "legacy-working-source".into(),
                raw: raw.clone(), order_type: order_type.clone(),
                source_input_method: None, last_input_method: None,
                analysis_raw: None, analysis_evidence: None, client_match: None,
                chat_handle: None, chat_title: None,
                client_id: None, client_selection: None, pricing_preview: None,
                working_version: None, working_synced: false, manual_model: None,
            });
        }
        let active_id = if !active.is_empty() {
            Some(active)
        } else {
            drafts.first().map(|item| item.id.clone())
        };
        let result = WorkingSource { raw, order_type, revision, active_id, drafts };
        validate_source(&result)?;
        Ok(result)
    }

    /// Atomically persist the complete unconfirmed reception workspace.
    /// This never changes the Tauri ledger or marks a receipt confirmed.
    pub fn save_source(&self, source: &WorkingSource) -> Result<u64, String> {
        validate_source(source)?;
        let expected = i64::try_from(source.revision)
            .map_err(|_| "egui 접수 저장 revision 범위를 초과했습니다.".to_owned())?;
        let next = expected.checked_add(1).ok_or("egui 접수 revision 한도 초과")?;
        let payload = Value::Array(source.drafts.iter().map(|item| json!({
            "id": item.id,
            "raw": item.raw,
            "orderType": item.order_type,
            "sourceInputMethod": item.source_input_method,
            "lastInputMethod": item.last_input_method,
            "analysisRaw": item.analysis_raw,
            "analysisEvidence": item.analysis_evidence,
            "clientMatch": item.client_match,
            "chatHandle": item.chat_handle,
            "chatTitle": item.chat_title,
            "clientId": item.client_id,
            "clientSelection": item.client_selection,
            "pricingPreview": item.pricing_preview,
            "workingVersion": item.working_version,
            "workingSynced": item.working_synced,
            "manualModel": item.manual_model,
        })).collect());
        let encoded = payload.to_string();
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let affected = tx.execute(
            "UPDATE working_source SET raw=?1,order_type=?2,revision=?3,
             active_id=?4,drafts_json=?5,updated_at=CURRENT_TIMESTAMP
             WHERE id=1 AND revision=?6",
            params![
                source.raw, source.order_type, next,
                source.active_id.as_deref().unwrap_or(""), encoded, expected
            ],
        ).map_err(|e| format!("egui 접수 목록 저장 실패: {e}"))?;
        if affected != 1 {
            return Err("다른 egui 창에서 원문이 변경됐습니다. 저장을 덮어쓰지 않았습니다.".into());
        }
        tx.commit().map_err(|e| format!("egui 접수 목록 저장 확정 실패: {e}"))?;
        Ok(next as u64)
    }

    /// Commit approved OCR comparisons only AFTER the reception source was
    /// successfully persisted. A stable UUID makes worker retries idempotent.
    pub fn append_ocr_feedback(samples: &[OcrFeedback]) -> Result<(), String> {
        if samples.is_empty() {return Ok(());}
        let store = Self::initialize()?;
        let mut conn = store.open_connection()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for sample in samples {
            if sample.id.trim().is_empty() || sample.order_id.trim().is_empty() ||
                sample.original_text.trim().is_empty() ||
                sample.corrected_text.trim().is_empty() ||
                sample.original_text.chars().count()>20_000 ||
                sample.corrected_text.chars().count()>20_000 {
                return Err("OCR 학습용 원문·정정문 자료가 유효하지 않습니다.".into());
            }
            let changes=correction_changes(
                &sample.original_text,&sample.corrected_text
            ).to_string();
            tx.execute(
                "INSERT OR IGNORE INTO ocr_feedback
                 (id,order_id,original_text,corrected_text,changes_json,
                  learning_used,model_suggestion)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![sample.id,sample.order_id,sample.original_text,
                    sample.corrected_text,changes,
                    i64::from(sample.learning_used),sample.model_suggestion],
            ).map_err(|e| format!("OCR 정정 이력 저장 실패: {e}"))?;
        }
        tx.commit().map_err(|e| format!("OCR 정정 이력 확정 실패: {e}"))
    }

    /// Read-back for later audit and candidate-rule review. No learning
    /// model is trained or enabled by collecting these records.
    pub fn list_ocr_feedback(limit: usize) -> Result<Vec<(String,String,String,String)>,String> {
        let store=Self::initialize()?;
        let conn=store.open_connection()?;
        let mut stmt=conn.prepare(
            "SELECT id,order_id,original_text,corrected_text
             FROM ocr_feedback
             WHERE learning_used=0 OR model_suggestion IS NULL
                 OR corrected_text<>model_suggestion
             ORDER BY recorded_at DESC,id DESC LIMIT ?1"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map([limit.min(1000) as i64],|row|Ok((
            row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?
        ))).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Store BOTH Rust-order-analysis snapshots and their structured
    /// difference. A future engine version can re-run either original text.
    /// This local artifact is independent from the employee's approved order.
    pub fn store_ocr_analysis(
        id:&str,initial:&Value,corrected:&Value,catalog_revision:Option<String>
    )->Result<(),String>{
        let store=Self::initialize()?;
        let conn=store.open_connection()?;
        let diff=compare_ocr_analyses(initial,corrected);
        conn.execute(
            "UPDATE ocr_feedback SET initial_analysis_json=?2,
             corrected_analysis_json=?3,analysis_comparison_json=?4,
             analysis_status='complete',analysis_error=NULL,
             analyzer_label='rust.analyze_order',
             catalog_revision=?5,
             analyzed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id=?1",
            params![id,initial.to_string(),corrected.to_string(),
                diff.to_string(),catalog_revision],
        ).map_err(|e|format!("OCR 양쪽 분석 결과 저장 실패: {e}"))?;
        Ok(())
    }

    /// Preserve full originals even when analysis cannot run. Failures
    /// are visible to future reprocessing; reception stays unaffected.
    pub fn fail_ocr_analysis(id:&str,error:&str)->Result<(),String>{
        let store=Self::initialize()?;
        let conn=store.open_connection()?;
        conn.execute(
            "UPDATE ocr_feedback SET analysis_status='failed',
             analysis_error=?2,analyzer_label='rust.analyze_order',
             analyzed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id=?1",
            params![id,error.chars().take(1000).collect::<String>()],
        ).map_err(|e|format!("OCR 분석 실패 이력 저장 실패: {e}"))?;
        Ok(())
    }

    pub fn path(&self) -> &std::path::Path { &self.path }
}

fn validate_source(source: &WorkingSource) -> Result<(), String> {
    if !matches!(source.order_type.as_str(), "AGENCY" | "DEALER") {
        return Err("접수 유형은 대리발송 또는 대리점 납품이어야 합니다.".into());
    }
    let mut ids = HashSet::new();
    if source.drafts.len() > 1000 {
        return Err("egui 미확정 접수는 최대 1,000건까지 보관할 수 있습니다.".into());
    }
    let mut bytes = 0usize;
    let mut analysis_bytes = 0usize;
    for draft in &source.drafts {
        if draft.id.trim().is_empty() || !ids.insert(draft.id.as_str()) {
            return Err("접수 ID가 비었거나 중복되었습니다.".into());
        }
        if !matches!(draft.order_type.as_str(), "AGENCY" | "DEALER") {
            return Err("저장할 접수 유형이 잘못되었습니다.".into());
        }
        // Imported legacy drafts have unknown provenance. Never silently
        // reinterpret their input as typed text or OCR.
        for method in [
            draft.source_input_method.as_deref(),
            draft.last_input_method.as_deref(),
        ].into_iter().flatten() {
            if !matches!(method, "TEXT_PASTE" | "OCR" | "DIRECT") {
                return Err("발주원문 접수 경로 정보가 잘못되었습니다.".into());
            }
        }
        if draft.last_input_method.is_some() && draft.source_input_method.is_none()
            && draft.raw.trim().is_empty() {
            return Err("빈 발주서에는 마지막 접수 경로를 기록할 수 없습니다.".into());
        }
        bytes = bytes.checked_add(draft.raw.len())
            .ok_or("접수 원문 총 크기가 너무 큽니다.")?;
        // A new/cleared selection must not masquerade as a confirmed customer.
        if draft.chat_handle.is_some() != draft.chat_title.is_some() ||
           draft.chat_handle.as_deref().is_some_and(|x| x.trim().is_empty()) ||
           draft.chat_title.as_deref().is_some_and(|x| x.trim().is_empty()) {
            return Err("대화창 연결 정보가 올바르지 않습니다.".into());
        }
        match (draft.client_id.as_deref(), draft.client_selection.as_deref()) {
            (None, None) => {}
            (Some(id), Some(source)) if !id.trim().is_empty() &&
                matches!(source, "AUTO" | "MANUAL") => {}
            _ => return Err("거래처 선택 상태가 올바르지 않습니다.".into()),
        }
        if draft.working_version == Some(0) ||
           (draft.working_synced && (draft.working_version.is_none() ||
               draft.client_id.is_none() || draft.pricing_preview.is_none())) {
            return Err("운영 작업본 저장 상태가 올바르지 않습니다.".into());
        }
        if let Some(manual)=&draft.manual_model {
            let fields=["sourceRaw","orderType","clientId","model","assignmentOrder"];
            let record=manual.as_object()
                .ok_or("수동 분류 작업본 형식이 잘못되었습니다.")?;
            if record.keys().any(|key|!fields.contains(&key.as_str())) ||
               manual.get("sourceRaw").and_then(Value::as_str)!=Some(draft.raw.as_str()) ||
               manual.get("orderType").and_then(Value::as_str)!=Some(draft.order_type.as_str()) ||
               manual.get("clientId").and_then(Value::as_str)!=draft.client_id.as_deref() ||
               !manual["model"]["rows"].is_array() ||
               !manual["model"]["blocks"].is_array() ||
               !manual["model"]["assignments"].is_object() ||
               !manual["assignmentOrder"].is_array() ||
               manual["model"]["rows"].as_array().is_some_and(|rows|rows.is_empty()||
                   rows.len()>100) {
                return Err("수동 분류 기록이 현재 발주서와 일치하지 않습니다.".into());
            }
            analysis_bytes=analysis_bytes.checked_add(manual.to_string().len())
                .ok_or("수동 분류 작업본 크기 초과")?;
        }
        if let Some(price)=&draft.pricing_preview {
            let keys=["sourceRaw","orderId","orderType","clientId","authorityRevision",
                "status","shippingFee","shippingVersion","vatAmount",
                "subtotal","total","notified","paid","linePrices"];
            let map=price.as_object()
                .ok_or("Rust 가격 계산 결과는 JSON 객체여야 합니다.")?;
            if map.keys().any(|key|!keys.contains(&key.as_str())) ||
               price.get("sourceRaw").and_then(Value::as_str)!=Some(draft.raw.as_str()) ||
               price.get("orderId").and_then(Value::as_str)!=Some(draft.id.as_str()) ||
               price.get("orderType").and_then(Value::as_str)!=Some(draft.order_type.as_str()) ||
               price.get("clientId").and_then(Value::as_str)!=draft.client_id.as_deref() ||
               price.get("authorityRevision").and_then(Value::as_u64).is_none() ||
               price.get("linePrices").and_then(Value::as_array).is_none() {
                return Err("Rust 주문 가격 근거가 원문 또는 거래처와 일치하지 않습니다.".into());
            }
            for line in price["linePrices"].as_array().into_iter().flatten() {
                let allowed=["itemId","productId","variantId","quantity","unitPrice"];
                let fields=line.as_object().ok_or("Rust 주문 품목 가격 결과 오류")?;
                if fields.keys().any(|key|!allowed.contains(&key.as_str())) {
                    return Err("Rust 계산 결과에 허용하지 않은 개인정보가 포함됐습니다.".into());
                }
            }
            analysis_bytes=analysis_bytes.checked_add(price.to_string().len())
                .ok_or("Rust 계산 결과 총 크기 초과")?;
        }
        match (&draft.analysis_raw, &draft.analysis_evidence, &draft.client_match) {
            (None, None, None) => {}
            (Some(raw), Some(evidence), Some(match_result)) if raw == &draft.raw &&
                evidence.get("recipients").is_some_and(Value::is_array) &&
                evidence.get("candidates").is_some_and(Value::is_array) &&
                evidence.get("unassigned").is_some_and(Value::is_array) &&
                match_result.is_object() => {
                analysis_bytes = analysis_bytes.checked_add(evidence.to_string().len())
                    .and_then(|size| size.checked_add(match_result.to_string().len()))
                    .ok_or("접수 분석 근거 총 크기가 너무 큽니다.")?;
            }
            _ => return Err("현재 발주원문과 분석 근거가 일치하지 않습니다.".into()),
        }
    }
    if analysis_bytes > 8 * 1024 * 1024 {
        return Err("접수 분석 근거 전체는 최대 8MB까지 저장할 수 있습니다.".into());
    }
    if bytes > SOURCE_LIMIT_BYTES {
        return Err("egui 접수 원문 전체는 최대 2MB까지 저장할 수 있습니다.".into());
    }
    match &source.active_id {
        Some(active_id) => {
            let active = source.drafts.iter().find(|row| &row.id == active_id)
                .ok_or("활성 접수 ID를 찾지 못했습니다.")?;
            if active.raw != source.raw || active.order_type != source.order_type {
                return Err("활성 접수와 저장할 원문이 일치하지 않습니다.".into());
            }
        }
        None if source.drafts.is_empty() && source.raw.is_empty() => {}
        None => return Err("활성 접수 ID가 없습니다.".into()),
    }
    Ok(())
}
