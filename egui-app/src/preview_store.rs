//! Strictly isolated local SQLite storage for egui development.
//! This database is NOT a migration of BANDO HUB's Tauri business database.
//! Never open, import or modify the operational app_state/core_* tables.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::HashSet, fs, path::PathBuf, time::Duration};

#[path="recognition_learning_store.rs"]
mod recognition_learning_store;

const SIGNATURE: &str = "bando-hub-egui-preview-v1";
const SOURCE_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// Full OCR recognition and employee-approved correction, retained locally
/// for an auditable future pattern-review process. Not an auto-learning rule.
#[derive(Clone, Debug)]
pub struct OcrFeedback {
    pub id: String,
    pub order_id: String,
    /// Verified staff-selected dealer; can be bound later after OCR intake.
    pub client_id: Option<String>,
    pub original_text: String,
    pub corrected_text: String,
    /// Whether the employee enabled local learned suggestions on this review.
    pub learning_used: bool,
    /// Proposed result before employee edits (None if no patterns applied).
    pub model_suggestion: Option<String>,
}

/// One manually verified OCR table-layout example. Source text remains
/// private in the local isolated SQLite database; caller uses it on a
/// background thread and never transmits it to a model service.
#[derive(Clone, Debug)]
pub struct OcrLayoutExample {
    pub order_id:String,
    pub client_id:Option<String>,
    pub original_text:String,
    pub corrected_text:String,
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

/// Learnable layout supervision for narrow-table OCR captures.
///
/// Old line-based LCS describes changed lines, but misses the crucial
/// difference between a character error and a hard table-cell wrap.
/// These examples are aligned by non-whitespace characters, so *only* actual
/// line break/space edits become labeled boundaries. A document that also
/// has transcription errors remains intact and is marked unaligned for a
/// later, more capable alignment engine.
fn layout_correction_trace(before:&str,after:&str)->Value{
    fn split(text:&str)->(Vec<char>,Vec<String>){
        let mut characters=Vec::new();
        let mut gaps=vec![String::new()];
        for ch in text.chars(){
            if ch.is_whitespace(){
                gaps.last_mut().expect("initial gap").push(ch);
            }else{
                characters.push(ch);
                gaps.push(String::new());
            }
        }
        (characters,gaps)
    }
    fn shape(text:&str)->Vec<Value>{
        text.lines().take(200).map(|line|{
            let meaningful=line.chars().filter(|c|!c.is_whitespace())
                .take(100).collect::<Vec<_>>();
            json!({
                "widthBucket":match meaningful.len(){
                    0=>0,1=>1,2=>2,3..=5=>3,6..=10=>4,11..=20=>5,_=>6,
                },
                "containsDigits":meaningful.iter().any(|c|c.is_ascii_digit()),
                "containsHangul":meaningful.iter().any(|c|
                    ('가'..='힣').contains(c)),
                "containsHyphen":meaningful.contains(&'-'),
                "containsParentheses":meaningful.contains(&'(')
                    ||meaningful.contains(&')'),
            })
        }).collect()
    }
    let (left_chars,left_gaps)=split(before);
    let (right_chars,right_gaps)=split(after);
    let mut result=json!({
        "schema":"ocr-layout-boundaries-v1",
        "originalLineCount":before.lines().count(),
        "correctedLineCount":after.lines().count(),
        "originalLineShapes":shape(before),
        "contentAligned":left_chars==right_chars,
        "boundaryChanges":[],
        "truncated":false,
    });
    if left_chars!=right_chars {
        return result;
    }
    let mut changes=Vec::new();
    let mut truncated=false;
    for (i,(old,new)) in left_gaps.iter().zip(right_gaps.iter()).enumerate(){
        if old==new {continue;}
        // We deliberately keep no duplicated text here. Original/corrected
        // full documents already support any future re-alignment.
        if changes.len()==2000 {truncated=true;break;}
        changes.push(json!({
            "afterNonWhitespaceChars":i,
            "before":old,
            "after":new,
            "wasLineBreak":old.contains('\n')||old.contains('\r'),
            "isLineBreak":new.contains('\n')||new.contains('\r'),
        }));
    }
    result["boundaryChanges"]=Value::Array(changes);
    result["truncated"]=json!(truncated);
    result
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
    /// Display-only native analysis projection; rebuild linked spans from the
    /// exact saved source on restore. No full Order or live catalog is stored.
    pub analysis_view: Option<Value>,
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
    /// Persisted snapshots of every employee-applied recipient/item edit.
    /// Kept even if the original source is later replaced or the manual
    /// model is invalidated; the append-only audit table is authoritative.
    pub edit_history: Vec<Value>,
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
    fn directory() -> Result<PathBuf,String>{
        let root = if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            PathBuf::from(local)
        } else if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            return Err("egui 전용 DB 저장 폴더를 찾지 못했습니다.".into());
        };
        let directory = if crate::business_state::test_launch(){
            crate::business_state::test_directory("preview")?
        }else{root.join("BANDO-HUB-egui-preview")};
        Ok(directory)
    }

    pub fn initialize() -> Result<Self, String> {
        let directory=Self::directory()?;
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
                layout_trace_json TEXT CHECK(layout_trace_json IS NULL
                    OR json_valid(layout_trace_json)),
                client_id TEXT,
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
        // Immutable change records are committed in the SAME CAS transaction
        // as the corrected working reception. Deleting a draft does not erase
        // its previously accepted employee edit history.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS reception_edit_audit (
                id TEXT PRIMARY KEY,
                order_id TEXT NOT NULL,
                source_raw TEXT NOT NULL,
                initial_analysis_json TEXT NOT NULL CHECK(json_valid(initial_analysis_json)),
                before_rows_json TEXT NOT NULL CHECK(json_valid(before_rows_json)),
                after_rows_json TEXT NOT NULL CHECK(json_valid(after_rows_json)),
                recorded_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_reception_edit_audit_order
                ON reception_edit_audit(order_id,recorded_at,id);"
        ).map_err(|e|format!("수취인/품목 수정 감사 테이블 준비 실패: {e}"))?;
        ensure_reception_audit_context(&conn)?;
        // Non-destructive migration of corpus rows already collected by
        // earlier 65th-session builds; NEVER drop original/corrected text.
        let mut stmt=conn.prepare("PRAGMA table_info(ocr_feedback)")
            .map_err(|e|e.to_string())?;
        let columns=stmt.query_map([],|row|row.get::<_,String>(1))
            .map_err(|e|e.to_string())?
            .collect::<Result<HashSet<_>,_>>().map_err(|e|e.to_string())?;
        drop(stmt);
        let additions=[
            ("layout_trace_json","TEXT"),
            ("client_id","TEXT"),
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
        // Form learning is always scoped to an explicitly confirmed client.
        // Index after the non-destructive client_id migration so older local
        // preview databases remain compatible.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_ocr_feedback_client_form
                ON ocr_feedback(client_id,recorded_at,id);"
        ).map_err(|e|format!("거래처별 OCR 양식 학습 인덱스 준비 실패: {e}"))?;
        // Incrementally backfill table-layout labels for legacy corpus rows.
        // Full OCR text stays byte-for-byte unchanged, so older employee
        // corrections remain useful when the experimental model evolves.
        // A bounded batch prevents a large old corpus from stalling startup.
        let mut pending_stmt=conn.prepare(
            "SELECT id,original_text,corrected_text FROM ocr_feedback
             WHERE layout_trace_json IS NULL
             ORDER BY recorded_at,id LIMIT 100"
        ).map_err(|e|e.to_string())?;
        let pending=pending_stmt.query_map([],|row|Ok((
            row.get::<_,String>(0)?,
            row.get::<_,String>(1)?,
            row.get::<_,String>(2)?,
        ))).map_err(|e|e.to_string())?
            .collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        drop(pending_stmt);
        for (id,initial,corrected) in pending {
            let layout=layout_correction_trace(&initial,&corrected).to_string();
            conn.execute(
                "UPDATE ocr_feedback SET layout_trace_json=?2
                 WHERE id=?1 AND layout_trace_json IS NULL",
                params![id,layout],
            ).map_err(|e|format!("기존 OCR 줄바꿈 학습 이력 보강 실패: {e}"))?;
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
                analysis_raw, analysis_evidence,
                analysis_view: row.get("analysisView").filter(|v|!v.is_null()).cloned(),
                client_match,
                chat_handle: row.get("chatHandle").and_then(Value::as_str).map(str::to_owned),
                chat_title: row.get("chatTitle").and_then(Value::as_str).map(str::to_owned),
                client_id: row.get("clientId").and_then(Value::as_str).map(str::to_owned),
                client_selection: row.get("clientSelection").and_then(Value::as_str).map(str::to_owned),
                pricing_preview: row.get("pricingPreview").filter(|v| !v.is_null()).cloned(),
                working_version: row.get("workingVersion").and_then(Value::as_u64),
                working_synced: row.get("workingSynced").and_then(Value::as_bool).unwrap_or(false),
                manual_model: row.get("manualModel").filter(|v|!v.is_null()).cloned(),
                edit_history: row.get("editHistory").and_then(Value::as_array)
                    .cloned().unwrap_or_default(),
            });
        }
        // Preserve a previous single-source editing session without changing
        // its SQLite row. A subsequent explicit edit migrates it atomically.
        if drafts.is_empty() && !raw.is_empty() {
            drafts.push(WorkingReception {
                id: "legacy-working-source".into(),
                raw: raw.clone(), order_type: order_type.clone(),
                source_input_method: None, last_input_method: None,
                analysis_raw: None, analysis_evidence: None, analysis_view:None,
                client_match: None,
                chat_handle: None, chat_title: None,
                client_id: None, client_selection: None, pricing_preview: None,
                working_version: None, working_synced: false, manual_model: None,
                edit_history: Vec::new(),
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
            "analysisView": item.analysis_view,
            "clientMatch": item.client_match,
            "chatHandle": item.chat_handle,
            "chatTitle": item.chat_title,
            "clientId": item.client_id,
            "clientSelection": item.client_selection,
            "pricingPreview": item.pricing_preview,
            "workingVersion": item.working_version,
            "workingSynced": item.working_synced,
            "manualModel": item.manual_model,
            "editHistory": item.edit_history,
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
        // Only audit snapshots submitted with a successful source CAS may
        // become durable. INSERT OR IGNORE makes retry/coalescing idempotent.
        // A saved audit is never deleted when an employee deletes the draft.
        for receipt in &source.drafts {
            for event in &receipt.edit_history {
                let inserted=tx.execute(
                    "INSERT OR IGNORE INTO reception_edit_audit
                     (id,order_id,source_raw,initial_analysis_json,
                      before_rows_json,after_rows_json,recorded_at,context_json)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        event["id"].as_str().unwrap_or(""),
                        receipt.id.as_str(),
                        event["sourceRaw"].as_str().unwrap_or(""),
                        event["initialAnalysis"].to_string(),
                        event["beforeRows"].to_string(),
                        event["afterRows"].to_string(),
                        event["at"].as_str().unwrap_or(""),
                        event.get("context").filter(|v|!v.is_null()).map(Value::to_string)
                    ],
                ).map_err(|e|format!("수취인/품목 수정 감사 이력 저장 실패: {e}"))?;
                if inserted==0{
                    // Re-saving an identical immutable event is idempotent.
                    // A conflicting UUID must not be silently accepted.
                    let identical:i64=tx.query_row(
                        "SELECT count(*) FROM reception_edit_audit WHERE
                         id=?1 AND order_id=?2 AND source_raw=?3 AND initial_analysis_json=?4
                         AND before_rows_json=?5 AND after_rows_json=?6 AND recorded_at=?7
                         AND context_json IS ?8",
                        params![event["id"].as_str().unwrap_or(""),receipt.id.as_str(),
                            event["sourceRaw"].as_str().unwrap_or(""),event["initialAnalysis"].to_string(),
                            event["beforeRows"].to_string(),event["afterRows"].to_string(),
                            event["at"].as_str().unwrap_or(""),
                            event.get("context").filter(|v|!v.is_null()).map(Value::to_string)],
                        |r|r.get(0)).map_err(|e|e.to_string())?;
                    if identical!=1{return Err("동일 정정 감사 ID에 상충된 기록이 있습니다.".into());}
                }
            }
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
            let layout_trace=layout_correction_trace(
                &sample.original_text,&sample.corrected_text
            ).to_string();
            tx.execute(
                "INSERT OR IGNORE INTO ocr_feedback
                 (id,order_id,original_text,corrected_text,changes_json,
                  layout_trace_json,client_id,learning_used,model_suggestion)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![sample.id,sample.order_id,sample.original_text,
                    sample.corrected_text,changes,layout_trace,
                    sample.client_id,i64::from(sample.learning_used),
                    sample.model_suggestion],
            ).map_err(|e| format!("OCR 정정 이력 저장 실패: {e}"))?;
        }
        tx.commit().map_err(|e| format!("OCR 정정 이력 확정 실패: {e}"))
    }

    /// Enrich existing OCR pairs after a staff member explicitly assigns
    /// the dealer. The record may predate client selection; do not guess
    /// its origin from OCR text or an unconfirmed client suggestion.
    pub fn bind_verified_ocr_clients(source:&WorkingSource)->Result<(),String>{
        let verified=source.drafts.iter().filter_map(|row|{
            if row.client_selection.as_deref()!=Some("MANUAL"){
                return None;
            }
            Some((row.id.as_str(),row.client_id.as_deref()?))
        }).collect::<Vec<_>>();
        if verified.is_empty(){return Ok(());}
        let store=Self::initialize()?;
        let mut conn=store.open_connection()?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        for (receipt_id,client_id) in verified{
            tx.execute("UPDATE ocr_feedback SET client_id=?2 WHERE order_id=?1",
                params![receipt_id,client_id])
                .map_err(|e|format!("OCR 양식 거래처 연결 실패: {e}"))?;
        }
        tx.commit().map_err(|e|e.to_string())
    }

    /// Candidate examples for the optional local pattern learner. Samples
    /// accepted unchanged from a model proposal are excluded so predictions
    /// cannot reinforce themselves as independent employee corrections.
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

    /// Retrieve ONLY a confirmed client's form examples. A global LIMIT
    /// would starve infrequent clients when the shared OCR corpus grows.
    /// Unverified dealer associations and predictions accepted without an
    /// independent human correction cannot teach that client's layout.
    pub fn list_ocr_layout_examples_for_client(
        client_id:&str,limit:usize
    )->Result<Vec<OcrLayoutExample>,String>{
        if client_id.trim().is_empty(){return Ok(Vec::new());}
        let store=Self::initialize()?;
        let conn=store.open_connection()?;
        let mut stmt=conn.prepare(
            "SELECT order_id,client_id,original_text,corrected_text
             FROM ocr_feedback
             WHERE client_id=?1
               AND (learning_used=0 OR model_suggestion IS NULL
                 OR corrected_text<>model_suggestion)
               AND layout_trace_json IS NOT NULL
             ORDER BY recorded_at DESC,id DESC LIMIT ?2"
        ).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(
            params![client_id,limit.min(1000) as i64],
            |row|Ok(OcrLayoutExample{
                order_id:row.get(0)?,
                client_id:row.get(1)?,
                original_text:row.get(2)?,
                corrected_text:row.get(3)?,
            })
        ).map_err(|e|e.to_string())?;
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

    // Read the existing isolated audit DB without initializing or migrating it.
    fn open_reception_audit_snapshot()->Result<Connection,String>{
        let path=Self::directory()?.join("egui-preview.sqlite3");
        let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e|format!("정정 이력 읽기 실패: {e}"))?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA query_only=ON; BEGIN DEFERRED TRANSACTION;")
            .map_err(|e|e.to_string())?;
        let signature:String=conn.query_row(
            "SELECT value FROM preview_meta WHERE key='signature'",[],|r|r.get(0))
            .map_err(|e|e.to_string())?;
        if signature!=SIGNATURE{return Err("egui 전용 DB 식별자가 다릅니다.".into());}
        Ok(conn)
    }
    pub(crate) fn read_reception_audit_page(query:&Value)->Result<Value,String>{
        let conn=Self::open_reception_audit_snapshot()?;
        let result=query_reception_audit_page(&conn,query)?;
        conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
        Ok(result)
    }

    /// All selected orders and pages come from one read-only audit snapshot.
    pub(crate) fn read_reception_audit_batch(ids:&[String])->Result<std::collections::BTreeMap<String,Vec<Value>>,String>{
        let conn=Self::open_reception_audit_snapshot()?;
        let result=query_reception_audit_batch(&conn,ids)?;
        conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
        Ok(result)
    }

    fn learning_connection()->Result<(Connection,PathBuf,String),String>{
        use sha2::{Digest,Sha256};
        let business=crate::business_state::authorized_business_write_path()?.canonicalize()
            .map_err(|_|"업무 DB 식별 실패")?;
        // Scope by the canonical bound DB, not a potentially shared order ID.
        let scope=format!("{:x}",Sha256::digest(business.as_os_str().as_encoded_bytes()));
        let path=Self::directory()?.join("egui-preview.sqlite3");
        let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_|"기존 정정 이력 DB 연결 실패")?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON").map_err(|e|e.to_string())?;
        let signature:String=conn.query_row("SELECT value FROM preview_meta WHERE key='signature'",[],|r|r.get(0))
            .map_err(|e|e.to_string())?;
        if signature!=SIGNATURE{return Err("egui 전용 DB 식별자가 다릅니다.".into());}
        Ok((conn,business,scope))
    }

    /// Snapshot and project outside the audit write lock; revalidate both
    /// authoritative sources before committing any derived contributions.
    pub(crate) fn sync_recognition_learning(ids:&[String],
        summarize:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
        let (mut conn,business,scope)=Self::learning_connection()?;
        recognition_learning_store::prepare(&conn)?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let (frozen,orders,digests)=crate::business_state::read_learning_inputs(ids,|sources|
            {
                let (mut misses,digests)=select_learning_changes(&tx,&scope,sources)?;
                for (id,digest) in &digests{
                    if !misses.contains(id)&&!recognition_learning_store::combined_evaluation_cached(&tx,&scope,id,digest)?{
                        misses.push(id.clone());
                    }
                }
                Ok((misses,digests))
            })?;
        let orders=attach_learning_audit(&tx,&frozen,orders)?;
        tx.commit().map_err(|e|e.to_string())?;
        // CPU-heavy comparison runs without either SQLite transaction.
        let summaries=prepare_learning_summaries(&orders,summarize)?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        let current=crate::business_state::read_learning_sources(ids)?;
        verify_learning_sync_sources(&tx,&frozen,&digests,&current)?;
        if crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("학습 갱신 중 업무 DB가 변경되었습니다.".into());
        }
        let report=apply_prepared_learning_changes(&tx,&scope,ids,&frozen,&orders,&digests,&summaries)?;
        if crate::business_state::read_learning_sources(ids)?!=frozen ||
            crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("학습 갱신 중 확정 주문 또는 업무 DB가 변경되었습니다.".into());
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    /// Recovery includes ledger-only IDs, so physically removed orders are
    /// reconciled as well as new or changed frozen confirmations.
    pub(crate) fn recognition_learning_page(after:&str,limit:usize)->Result<Vec<String>,String>{
        let (conn,_,scope)=Self::learning_connection()?;
        recognition_learning_store::prepare(&conn)?;
        let stored=recognition_learning_store::order_page(&conn,&scope,after,limit)?;
        let business=crate::business_state::learning_order_page(after,limit)?;
        let combined=stored.into_iter().chain(business).collect::<std::collections::BTreeSet<_>>();
        Ok(combined.into_iter().take(limit).collect())
    }

    pub(crate) fn pending_recognition_learning_ids(limit:usize)->Result<Vec<String>,String>{
        let (conn,_,scope)=Self::learning_connection()?;
        recognition_learning_store::prepare(&conn)?;
        let mut stmt=conn.prepare("SELECT order_id FROM recognition_learning_pending
            WHERE scope=?1 AND feature_version=1 ORDER BY order_id LIMIT ?2").map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![scope,limit.clamp(1,250) as i64],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    pub(crate) fn quarantine_recognition_learning(id:&str)->Result<(),String>{
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        recognition_learning_store::prepare(&tx)?;
        recognition_learning_store::mark_pending(&tx,&scope,id)?;
        tx.commit().map_err(|e|e.to_string())?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        recognition_learning_store::revoke_pending(&tx,&scope,id)?;
        if crate::business_state::authorized_business_write_path()?.canonicalize()
            .map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("학습 격리 중 업무 DB가 변경되었습니다.".into());
        }
        tx.commit().map_err(|e|e.to_string())
    }

    pub(crate) fn capture_recognition_learning(target:&str,projection:&Value)->Result<(Value,Result<Option<Value>,String>),String>{
        use sha2::{Digest,Sha256};
        use std::collections::{BTreeMap,BTreeSet};
        // Diagnostic ratio only; not an automatic classification policy.
        const RATIO:f64=3.0;
        let owners=projection["entries"].as_array().ok_or("귀속 문맥 목록 누락")?;
        let fields=projection["fieldEntries"].as_array().ok_or("필드 문맥 목록 누락")?;
        if owners.len()+fields.len()>64{return Err("실시간 학습 관측 한도 초과".into());}
        let mut keys=BTreeSet::new();
        let mut queries=BTreeSet::<(String,String,Vec<String>)>::new();
        let mut locations=Vec::new();
        let mut skipped=0usize;
        for (entry,owner) in owners.iter().map(|e|(e,true)).chain(fields.iter().map(|e|(e,false))){
            let task=if owner{"recipient_ownership"}else{entry["task"].as_str().ok_or("필드 작업 오류")?};
            if !matches!(task,"recipient_ownership"|"type"|"product"|"variant"|"quantity"){
                return Err("실시간 작업 오류".into());
            }
            let candidates=if owner{vec!["previous".to_owned(),"next".to_owned()]}else{
                entry["shadowCandidates"].as_array().ok_or("필드 후보 오류")?.iter()
                    .map(|v|v.as_str().map(str::to_owned).ok_or("필드 후보 오류".to_owned()))
                    .collect::<Result<Vec<_>,_>>()?
            };
            if candidates.is_empty(){skipped+=1;continue;}
            let source=&entry["source"];
            let start=source["start"].as_u64().ok_or("학습 위치 오류")?;
            let end=source["end"].as_u64().filter(|end|*end>start).ok_or("학습 위치 오류")?;
            let mut local=Vec::new();
            for c in entry["contexts"].as_array().ok_or("학습 문맥 오류")?{
                let key=c["key"].as_str().ok_or("학습 문맥 키 누락")?.to_owned();
                keys.insert(key.clone());local.push(key.clone());
                queries.insert((task.to_owned(),key,candidates.clone()));
            }
            let coverage=if owner{"traced_pair"}else{
                entry["candidateCoverage"].as_str().filter(|s|matches!(*s,"observed_only"|"traced_partial"))
                    .ok_or("후보 추적 범위 오류")?
            };
            locations.push(json!({"start":start,"end":end,"task":task,"contextKeys":local,
                "candidates":candidates,"candidateCoverage":coverage,
                "traceReferences":entry["traceReferences"],"traceIsTrainingEvidence":false}));
        }
        if keys.is_empty(){return Ok((json!({"status":"no_eligible_context","skippedMissingCandidates":skipped,
            "decisionChanged":false,"calibratedProbability":null}),Ok(None)));}
        if queries.len()>32{return Err("실시간 학습 조회 한도 초과".into());}
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let members=recognition_learning_store::shadow_capture_contexts(&tx,&scope,target,&keys)?;
        let ids=members.values().flatten().cloned().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        let (frozen,digests,verified)=if ids.is_empty(){(BTreeMap::new(),BTreeMap::new(),BTreeMap::new())}else{
            let (frozen,_,(digests,verified))=crate::business_state::read_learning_inputs(&ids,|sources|{
                let digests=learning_source_digests(&tx,sources)?;
                if digests.len()!=ids.len(){return Err("실시간 학습 원본 동기화 필요".into());}
                let mut verified=BTreeMap::new();let mut fact_count=0usize;let mut memory_bytes=0usize;
                for (id,digest) in &digests{
                    let facts=recognition_learning_store::verified_facts(&tx,&scope,id,digest)?
                        .filter(|f|f.len()>0).ok_or("실시간 학습 원본 동기화 필요")?;
                    fact_count=fact_count.saturating_add(facts.len());
                    memory_bytes=memory_bytes.saturating_add(facts.memory_bytes());
                    if fact_count>2_000_000||memory_bytes>64*1024*1024{return Err("실시간 검증 기여량 한도 초과".into());}
                    verified.insert(id.clone(),facts);
                }
                Ok((Vec::new(),(digests,verified)))
            })?;
            (frozen,digests,verified)
        };
        let mut results=Vec::new();
        for (task,key,candidates) in &queries{
            let report=recognition_learning_store::assess_verified_support(&json!({
                "task":task,"contextKey":key,"targetOrderId":target,
                "candidates":candidates,"explicitToWeakRatio":RATIO}),&members,&verified)?;
            // Only compact predictive sufficient statistics are persisted,
            // not the correction-validation report or source documents.
            results.push(json!({"contextKey":key,"task":task,"candidateIds":candidates,
                "candidateCoverage":if task=="recipient_ownership"{"traced_pair"}else{"see_locations"},
                "supportVersion":report["supportVersion"],
                "featureVersion":report["featureVersion"],"contributionVersion":report["contributionVersion"],
                "contextOrders":report["contextOrders"],"supportedOrders":report["supportedOrders"],
                "totalSupportMass":report["totalSupportMass"],"weightEffectiveOrders":report["weightEffectiveOrders"],
                "outsideCandidateSupportMass":report["outsideCandidateSupportMass"],
                "candidates":report["candidates"],"contextQuality":report["contextQuality"],
                "calibratedProbability":null}));
        }
        // Both the support and selected model use the same audit snapshot.
        // Reuse verified overlap; independently check only model-only sources.
        let selected_model=(||->Result<Option<Value>,String>{
            if !blend_registry_exists(&tx)?{return Ok(None);}
            let Some(id)=selected_blend_model(&tx,&scope)?else{return Ok(None);};
            let model=read_registered_blend_model(&tx,&scope,&id)?;
            let expected=model["sourceDigests"].as_object().ok_or("모델 원본 지문 누락")?;
            if expected.contains_key(target){return Err("결합 모델 자기 주문 포함".into());}
            let extra=remaining_model_sources(expected,&digests)?;
            if !extra.is_empty(){
                let (source,_,actual)=crate::business_state::read_learning_inputs(&extra,|sources|{
                    let (misses,actual)=select_learning_changes(&tx,&scope,sources)?;
                    if !misses.is_empty(){return Err("결합 모델 원본 동기화 필요".into());}
                    Ok((Vec::new(),actual))
                })?;
                if actual.len()!=extra.len()||actual.iter().any(|(id,d)|expected.get(id).and_then(Value::as_str)!=Some(d.as_str()))||
                    crate::business_state::read_learning_sources(&extra)?!=source{
                    return Err("결합 모델 원본 변경".into());
                }
            }
            Ok(Some(model))
        })();
        if (!ids.is_empty()&&crate::business_state::read_learning_sources(&ids)?!=frozen) ||
            crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("실시간 학습 조회 중 원본 변경".into());
        }
        let selection=recognition_learning_store::select_learning_contexts(&locations,&results);
        let mut hash=Sha256::new();
        hash.update(serde_json::to_vec(&json!(["recognition-shadow-v4",scope,RATIO,digests,results]))
            .map_err(|_|"실시간 학습 모델 지문 오류")?);
        let model=format!("shadow-{:x}",hash.finalize());
        let result=json!({"status":"captured_shadow_support","captureVersion":4,"skippedMissingCandidates":skipped,
            "modelId":model,"capturedAt":chrono::Utc::now().to_rfc3339(),
            "explicitToWeakRatio":RATIO,"verifiedSourceOrders":ids.len(),"locations":locations,
            "contexts":results,"contextSelection":selection,"sourceDigestsMatched":true,"fullHistoryReconciled":false,
            "calibratedProbability":null,"eligibleForAutomaticDecision":false,"decisionChanged":false});
        tx.commit().map_err(|e|e.to_string())?;
        Ok((result,selected_model))
    }

    fn blend_model_registry(input:&Value)->Result<Value,String>{
        let action=input["action"].as_str().ok_or("결합 모델 관리 동작 누락")?;
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=if action=="list"{conn.transaction()}else{
            conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        }.map_err(|e|e.to_string())?;
        if action=="list"{
            if !blend_registry_exists(&tx)?{
                return Ok(json!({"models":[],"selectedModelId":null,"automaticChangeAuthorized":false}));
            }
            let selected=selected_blend_model(&tx,&scope)?;
            let models={
                let mut stmt=tx.prepare("SELECT model_id,created_at,available_after FROM recognition_blend_models
                    WHERE scope=?1 ORDER BY created_at DESC,model_id LIMIT 250").map_err(|e|e.to_string())?;
                let rows=stmt.query_map([&scope],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))
                    .map_err(|e|e.to_string())?;
                let mut models=Vec::new();
                for row in rows{
                    let (id,created,available)=row.map_err(|e|e.to_string())?;
                    models.push(json!({"modelId":id,"createdAt":created,"availableAfter":available,
                        "selected":selected.as_deref()==Some(id.as_str())}));
                }
                models
            };
            tx.commit().map_err(|e|e.to_string())?;
            return Ok(json!({"models":models,"selectedModelId":selected,"automaticChangeAuthorized":false}));
        }
        if !matches!(action,"select"|"clear"){return Err("지원하지 않는 결합 모델 관리 동작".into());}
        prepare_blend_registry(&tx)?;
        let selected=selected_blend_model(&tx,&scope)?;
        let expected=input.get("expectedSelectedModelId").ok_or("기존 선택 모델 확인값 필요")?;
        if *expected!=json!(selected){return Err("선택 모델이 변경되었습니다. 다시 조회하세요.".into());}
        let next=if action=="select"{
            let id=input["modelId"].as_str().ok_or("결합 모델 ID 누락")?;
            let model=read_registered_blend_model(&tx,&scope,id)?;
            verify_blend_model_sources(&tx,&scope,&business,&model,None)?;
            let available=chrono::DateTime::parse_from_rfc3339(model["availableAfter"].as_str().ok_or("모델 시각 오류")?)
                .map_err(|_|"모델 시각 오류")?;
            if available>=chrono::Utc::now(){return Err("결합 모델 사용 가능 시각 미도달".into());}
            Some(id.to_owned())
        }else{None};
        set_selected_blend_model(&tx,&scope,next.as_deref())?;
        prepare_blend_governance(&tx)?;
        tx.execute("DELETE FROM recognition_blend_monitor WHERE scope=?1",[&scope]).map_err(|e|e.to_string())?;
        if crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("모델 관리 중 업무 DB 변경".into());
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(json!({"previousSelectedModelId":selected,"selectedModelId":next,
            "status":"shadow_selection_only","automaticChangeAuthorized":false}))
    }

    // Explicit read-only full scan. Both databases remain on fixed read
    // snapshots; they are not one cross-database atomic transaction.
    fn combined_model_performance(input:&Value)->Result<Value,String>{
        if input.get("orderIds").is_some()||input.get("samples").is_some(){return Err("전체 평가에는 외부 표본을 혼합하지 않습니다.".into());}
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=1",
            [&scope],|r|r.get(0)).map_err(|e|e.to_string())?;
        if pending!=0{return Err("복구 대기 주문이 있어 전체 평가를 보류합니다.".into());}
        let source=crate::business_state::open_business_read_snapshot()?;
        let started=chrono::Utc::now().to_rfc3339();
        let elapsed=std::time::Instant::now();
        let mut cursor=String::new();
        let lifecycle=crate::business_state::learning_lifecycle_index(&source)?;
        let mut totals=recognition_learning_store::CombinedPerformance::default();
        loop{
            let ledger=recognition_learning_store::order_page(&tx,&scope,&cursor,25)?;
            let current=crate::business_state::learning_order_page_from_snapshot(&source,&cursor,25)?;
            let ids=ledger.into_iter().chain(current).collect::<std::collections::BTreeSet<_>>()
                .into_iter().take(25).collect::<Vec<_>>();
            if ids.is_empty(){break;}
            if totals.scanned+ids.len() as u64>100_000{return Err("전체 평가 100000건 안전 한도 초과".into());}
            let sources=crate::business_state::learning_sources_indexed(&source,&ids,Some(&lifecycle))?;
            let digests=learning_source_digests(&tx,&sources)?;
            totals.scanned+=ids.len() as u64;
            totals.inactive+=(ids.len()-digests.len()) as u64;
            recognition_learning_store::accumulate_combined_performance(&tx,&scope,&digests,&mut totals)?;
            cursor=ids.last().cloned().ok_or("평가 순회 위치 오류")?;
        }
        let selected=if blend_registry_exists(&tx)?{selected_blend_model(&tx,&scope)?}else{None};
        let mut report=totals.wire();
        match (input.get("baselineModelId"),input.get("candidateModelId")){
            (None,None)=>{},
            (Some(a),Some(b))=>{
                report["modelComparison"]=totals.compare_models(
                    a.as_str().ok_or("기존 모델 ID 오류")?,b.as_str().ok_or("신규 모델 ID 오류")?)?;
            }
            _=>return Err("비교할 기존·신규 모델 ID를 함께 명시하세요.".into())
        }
        report["selectedModelId"]=json!(selected);
        report["fullSnapshotScanCompleted"]=json!(true);
        report["allActiveOrdersHaveEvaluationRecords"]=json!(totals.unsynchronized==0&&totals.missing==0);
        report["snapshotStartedAt"]=json!(started);
        report["scanElapsedMs"]=json!(elapsed.elapsed().as_millis() as u64);
        report["crossDatabaseAtomicSnapshot"]=json!(false);
        if crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("전체 평가 중 업무 DB 변경".into());
        }
        source.execute_batch("COMMIT").map_err(|e|e.to_string())?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    /// Candidate generation only: the selected model and business data
    /// remain unchanged. The worker calls this after a full ledger pass.
    pub(crate) fn auto_retrain_blend(evaluate:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
        let (mut conn,business,scope)=Self::learning_connection()?;
        // Prepare the metadata schema separately, then acquire a read snapshot.
        conn.execute_batch("CREATE TABLE IF NOT EXISTS recognition_blend_retrain_state(
            scope TEXT NOT NULL,build TEXT NOT NULL,policy_version INTEGER NOT NULL,
            cohort TEXT NOT NULL,attempted_at TEXT NOT NULL,status TEXT NOT NULL,model_id TEXT,
            PRIMARY KEY(scope,build,policy_version));").map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=1",
            [&scope],|r|r.get(0)).map_err(|e|e.to_string())?;
        if pending!=0{return Ok(auto_retrain_status("pending_reconciliation",0));}
        let build=env!("CARGO_PKG_VERSION");
        let source=crate::business_state::open_business_read_snapshot()?;
        // Most recent compatible initial predictions, never imported labels.
        // The existing lifecycle check below rejects inactive orders.
        let ids={
            let mut stmt=source.prepare("SELECT id FROM core_orders
                WHERE json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.schemaVersion')=1
                AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.clientBuildVersion')=?1
                AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.recipientOwnerTrace.scorePolicyVersion')='owner-margin-30-v1'
                AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.learningShadow.captureVersion')=4
                AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.learningShadow.status')='captured_shadow_support'
                AND coalesce(json_extract(payload,'$.deletedAt'),'')=''
                AND julianday(confirmed_at) IS NOT NULL
                AND julianday(json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.learningShadow.capturedAt')) IS NOT NULL
                ORDER BY julianday(json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.learningShadow.capturedAt')) DESC,id DESC LIMIT 250")
                .map_err(|e|e.to_string())?;
            let rows=stmt.query_map([build],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;
            rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
        };
        if ids.len()<AUTO_RETRAIN_MIN_ORDERS{return Ok(auto_retrain_status("insufficient_orders",ids.len()));}
        let prior:Option<String>=tx.query_row("SELECT cohort FROM recognition_blend_retrain_state
            WHERE scope=?1 AND build=?2 AND policy_version=1",params![scope,build],|r|r.get(0))
            .optional().map_err(|e|e.to_string())?;
        let previous=prior.as_deref().map(serde_json::from_str::<LearningDigests>).transpose().map_err(|_|"자동 재학습 상태 손상")?;
        let (frozen,mut orders,(digests,due))=crate::business_state::learning_inputs_from_snapshot(&source,&ids,|sources|{
            let (misses,digests)=select_learning_changes(&tx,&scope,sources)?;
            if !misses.is_empty(){return Err("자동 재학습 원본 동기화 필요".into());}
            let due=previous.as_ref().is_none_or(|prior|auto_retrain_due(prior,&digests));
            let active=if due{sources.iter().filter(|(_,s)|s.active).map(|(id,_)|id.clone()).collect()}else{Vec::new()};
            Ok((active,(digests,due)))
        })?;
        source.execute_batch("COMMIT").map_err(|e|e.to_string())?;
        if !due{return Ok(auto_retrain_status("unchanged_or_insufficient_new_orders",digests.len()));}
        if orders.len()<AUTO_RETRAIN_MIN_ORDERS{return Ok(auto_retrain_status("insufficient_active_orders",orders.len()));}
        let now=chrono::Utc::now().fixed_offset();
        let Some(config)=auto_retrain_config(&orders,now,build)else{
            return Ok(auto_retrain_status("insufficient_distinct_chronological_windows",orders.len()));
        };
        // Persistent attempt state prevents retraining identical cohorts on
        // every scan or after restarting the application.
        let audit_ids=orders.iter().filter_map(|o|o["id"].as_str()).filter(|id|
            frozen.get(*id).is_some_and(|s|s.audit_needed)).map(str::to_owned).collect::<Vec<_>>();
        let mut audit=if audit_ids.is_empty(){std::collections::BTreeMap::new()}
            else{query_reception_audit_batch(&tx,&audit_ids)?};
        for order in &mut orders{
            let id=order["id"].as_str().ok_or("자동 재학습 주문 ID 누락")?;
            if let Some(events)=audit.remove(id){order["recognitionAuditEvents"]=json!(events);}
        }
        tx.commit().map_err(|e|e.to_string())?;
        let started=std::time::Instant::now();
        // No audit read or write transaction is held during CPU-heavy fitting.
        let report=evaluate(&json!({"orders":orders,"config":config}))?;
        let fit_ms=started.elapsed().as_millis() as u64;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        let latest:Option<String>=tx.query_row("SELECT cohort FROM recognition_blend_retrain_state
            WHERE scope=?1 AND build=?2 AND policy_version=1",params![scope,build],|r|r.get(0))
            .optional().map_err(|e|e.to_string())?;
        let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=1",
            [&scope],|r|r.get(0)).map_err(|e|e.to_string())?;
        let current=crate::business_state::read_learning_sources(&ids)?;
        let (misses,current_digests)=select_learning_changes(&tx,&scope,&current)?;
        if latest!=prior||pending!=0||!misses.is_empty()||current_digests!=digests||
            current!=frozen||
            crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("자동 재학습 중 원본 변경".into());
        }
        let mut result=auto_retrain_status("insufficient_calibrated_or_test_samples",digests.len());
        let mut model_id=None;
        // Save only a usable candidate with a nonempty held-out evaluation.
        // This is not an approval or a comparison-based promotion.
        if report["status"]=="blend_weight_diagnostic_only"&&report["runtimeParameters"].is_object()&&
            report["weightOrdersWithBothModels"].as_u64().is_some_and(|n|n>=10)&&
            report["testOrdersWithBothModels"].as_u64().is_some_and(|n|n>=10)&&
            report["overallTestCoverage"].as_f64().is_some_and(|v|v>0.0)&&
            report["models"].as_array().is_some_and(|a|a.iter().any(|m|
                m["weightFitOrders"].as_u64().is_some_and(|n|n>=10)&&m["learningWeight"].is_number())){
            let mut model=report["runtimeParameters"].clone();
            model["businessScope"]=json!(scope);
            model["sourceDigests"]=json!(digests);
            model["generationKind"]=json!("automatic_candidate_v1");
            model["generationPolicyVersion"]=json!(1);
            // Record the actual cutoff boundaries, not only availableAfter.
            model["trainingWindows"]=config.clone();
            verify_blend_model_sources(&tx,&scope,&business,&model,None)?;
            prepare_blend_registry(&tx)?;
            model_id=Some(store_blend_model(&tx,&scope,&model)?);
            result["status"]=json!("candidate_stored");
            result["storedModelId"]=json!(model_id);
        }
        let cohort=serde_json::to_string(&digests).map_err(|_|"재학습 표본 직렬화 오류")?;
        tx.execute("INSERT INTO recognition_blend_retrain_state VALUES(?1,?2,1,?3,?4,?5,?6)
            ON CONFLICT(scope,build,policy_version) DO UPDATE SET cohort=excluded.cohort,
            attempted_at=excluded.attempted_at,status=excluded.status,model_id=excluded.model_id",
            params![scope,build,cohort,now.to_rfc3339(),result["status"].as_str(),model_id]).map_err(|e|e.to_string())?;
        result["trainingWindows"]=config;
        result["fitElapsedMs"]=json!(fit_ms);
        result["weightOrdersWithBothModels"]=report["weightOrdersWithBothModels"].clone();
        result["testOrdersWithBothModels"]=report["testOrdersWithBothModels"].clone();
        result["overallTestCoverage"]=report["overallTestCoverage"].clone();
        tx.commit().map_err(|e|e.to_string())?;
        Ok(result)
    }

    /// Explicit, bounded, read-only paired replay of two immutable registry
    /// revisions. Source orders of either revision cannot be test orders.
    pub(crate) fn paired_blend_evaluation(input:&Value,evaluate:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
        let a=input["baselineModelId"].as_str().ok_or("기존 모델 ID 누락")?;
        let b=input["candidateModelId"].as_str().ok_or("신규 모델 ID 누락")?;
        if a==b||input.get("samples").is_some()||input.get("model").is_some(){return Err("독립 평가 입력 오류".into());}
        let ids=input["orderIds"].as_array().filter(|v|!v.is_empty()&&v.len()<=250).ok_or("독립 평가 주문 한도 오류")?
            .iter().map(|v|v.as_str().filter(|s|!s.is_empty()&&s.len()<=1024).map(str::to_owned).ok_or("주문 ID 오류"))
            .collect::<Result<Vec<_>,_>>()?;
        if ids.iter().collect::<std::collections::BTreeSet<_>>().len()!=ids.len(){return Err("독립 평가 주문 중복".into());}
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let prepared=Self::prepare_paired_blend(&tx,&scope,&business,a,b,&ids,None,None)?;
        tx.commit().map_err(|e|e.to_string())?;
        let report=Self::evaluate_paired_blend(&prepared,evaluate)?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let report=Self::finalize_paired_blend(&tx,&scope,&business,&ids,&prepared,report)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    fn prepare_paired_blend(tx:&rusqlite::Transaction<'_>,scope:&str,business:&std::path::Path,
        a:&str,b:&str,ids:&[String],minimum_after:Option<&str>,alpha:Option<f64>)->Result<PreparedPairedReplay,String>{
        let baseline=read_paired_model(tx,scope,a)?;let candidate=read_paired_model(tx,scope,b)?;
        verify_blend_model_pair_sources(tx,scope,business,&baseline["model"],&candidate["model"])?;
        let (frozen,orders,digests)=crate::business_state::read_learning_inputs(ids,|sources|{
            let (misses,digests)=select_learning_changes(tx,scope,sources)?;
            if !misses.is_empty(){return Err("독립 평가 원본 동기화 필요".into());}
            let active=sources.iter().filter(|(_,s)|s.active).map(|(id,_)|id.clone()).collect();
            Ok((active,digests))
        })?;
        if orders.is_empty(){return Err("독립 평가 활성 주문 없음".into());}
        let orders=attach_learning_audit(tx,&frozen,orders)?;
        let mut payload=json!({"orders":orders,"baseline":baseline,"candidate":candidate,
            "evaluationAsOf":chrono::Utc::now().to_rfc3339()});
        if let Some(cutoff)=minimum_after{payload["minimumPredictionAfter"]=json!(cutoff);}
        if let Some(alpha)=alpha{payload["inferenceAlpha"]=json!(alpha);}
        Ok(PreparedPairedReplay{payload,frozen,digests})
    }

    // No SQLite transaction is retained during independent CPU evaluation.
    fn evaluate_paired_blend(prepared:&PreparedPairedReplay,
        evaluate:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
        let started=std::time::Instant::now();
        let mut report=evaluate(&prepared.payload)?;
        report["replayElapsedMs"]=json!(started.elapsed().as_millis() as u64);
        Ok(report)
    }

    fn finalize_paired_blend(tx:&rusqlite::Transaction<'_>,scope:&str,business:&std::path::Path,
        ids:&[String],prepared:&PreparedPairedReplay,mut report:Value)->Result<Value,String>{
        let current=crate::business_state::read_learning_sources(ids)?;
        if current!=prepared.frozen{return Err("독립 평가 중 원본 변경".into());}
        let (misses,digests)=select_learning_changes(tx,scope,&current)?;
        if !misses.is_empty()||digests!=prepared.digests{return Err("독립 평가 중 학습 원장 변경".into());}
        let baseline=&prepared.payload["baseline"];let candidate=&prepared.payload["candidate"];
        for expected in [baseline,candidate]{
            let id=expected["modelId"].as_str().ok_or("독립 평가 모델 ID 누락")?;
            if read_paired_model(tx,scope,id)?!=*expected{return Err("독립 평가 중 모델 변경".into());}
        }
        verify_blend_model_pair_sources(tx,scope,business,&baseline["model"],&candidate["model"])?;
        if crate::business_state::read_learning_sources(ids)?!=current{
            return Err("독립 평가 원본 재검증 중 변경".into());
        }
        report["requestedOrders"]=json!(ids.len());
        report["missingOrInactiveOrders"]=json!(ids.len()-digests.len());
        report["evaluationSourceDigests"]=json!(digests);
        report["currentSourceDigestsVerified"]=json!(true);
        report["crossDatabaseAtomicSnapshot"]=json!(false);
        Ok(report)
    }

    /// Automatic selection is limited to diagnostic shadow models.
    /// A manual selection is required to bootstrap the comparison baseline.
    pub(crate) fn auto_govern_blend(evaluate:impl Fn(&Value)->Result<Value,String>+Copy)->Result<Value,String>{
        let (mut conn,business,scope)=Self::learning_connection()?;
        prepare_blend_governance(&conn)?;
        let mut tx=conn.transaction().map_err(|e|e.to_string())?;
        let status=|s:&str|json!({"status":s,"policyVersion":1,"automaticChangeAuthorized":false,"decisionChanged":false});
        if !blend_registry_exists(&tx)?{return Ok(status("no_registry"));}
        let Some(current)=selected_blend_model(&tx,&scope)?else{return Ok(status("no_manual_baseline"));};
        let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=1",
            [&scope],|r|r.get(0)).map_err(|e|e.to_string())?;
        if pending!=0{return Ok(status("pending_reconciliation"));}
        let expected_state=blend_governance_state(&tx,&scope)?;
        let monitor:Option<(String,String,String)>=tx.query_row("SELECT previous_model,selected_model,selected_at
            FROM recognition_blend_monitor WHERE scope=?1",[&scope],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
            .optional().map_err(|e|e.to_string())?;
        if let Some((previous,selected,since))=monitor.filter(|(_,selected,_)|selected==&current){
            let checkpoint:Option<(String,i64)>=tx.query_row("SELECT after_at,look_no FROM recognition_blend_monitor_checkpoint
                WHERE scope=?1 AND selected_model=?2 AND selected_at=?3",params![scope,selected,since],|r|Ok((r.get(0)?,r.get(1)?)))
                .optional().map_err(|e|e.to_string())?;
            let (after,look)=checkpoint.unwrap_or((since.clone(),0));
            if !(0..10000).contains(&look){return Ok(status("monitoring_look_limit"));}
            let ids=governance_orders(&after,false)?;
            if ids.len()<250{return Ok(status("waiting_for_fixed_post_selection_cohort"));}
            let alpha=governance_alpha(look as u64+1);
            let prepared=Self::prepare_paired_blend(&tx,&scope,&business,&previous,&selected,&ids,Some(&after),Some(alpha))?;
            tx.commit().map_err(|e|e.to_string())?;
            let report=Self::evaluate_paired_blend(&prepared,evaluate)?;
            tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
            revalidate_governance_state(&tx,&scope,&expected_state)?;
            let report=Self::finalize_paired_blend(&tx,&scope,&business,&ids,&prepared,report)?;
            let verdict=governance_verdict(&report,alpha);
            if verdict=="rollback"{
                // Both revisions and current evaluation inputs were verified
                // within this transaction by the replay path.
                record_blend_transition(&tx,&scope,&selected,&previous,"rollback",&report)?;
                set_selected_blend_model(&tx,&scope,Some(&previous))?;
                tx.execute("INSERT OR IGNORE INTO recognition_blend_blocked VALUES(?1,?2)",params![scope,selected])
                    .map_err(|e|e.to_string())?;
                tx.execute("DELETE FROM recognition_blend_monitor WHERE scope=?1",[&scope]).map_err(|e|e.to_string())?;
                tx.execute("DELETE FROM recognition_blend_monitor_checkpoint WHERE scope=?1",[&scope]).map_err(|e|e.to_string())?;
                tx.commit().map_err(|e|e.to_string())?;
                let mut v=status("shadow_model_rolled_back");v["selectedModelId"]=json!(previous);return Ok(v);
            }
            // Consume the fixed cohort once even when inconclusive. The
            // next look uses only orders whose analysis starts after this look.
            record_blend_transition(&tx,&scope,&previous,&selected,"monitor",&report)?;
            tx.execute("INSERT INTO recognition_blend_monitor_checkpoint VALUES(?1,?2,?3,?4,?5)
                ON CONFLICT(scope) DO UPDATE SET selected_model=excluded.selected_model,selected_at=excluded.selected_at,
                after_at=excluded.after_at,look_no=excluded.look_no",
                params![scope,selected,since,report["evaluationAsOf"].as_str().ok_or("감시 평가 시각 누락")?,look+1])
                .map_err(|e|e.to_string())?;
            if !matches!(verdict,"promote"|"stable"){
                tx.commit().map_err(|e|e.to_string())?;return Ok(status("monitoring_inconclusive"));
            }
            tx.commit().map_err(|e|e.to_string())?;
            tx=conn.transaction().map_err(|e|e.to_string())?;
            if selected_blend_model(&tx,&scope)?.as_deref()!=Some(current.as_str()){
                return Err("감시 이후 선택 모델 변경".into());
            }
        }
        let expected_state=blend_governance_state(&tx,&scope)?;
        let current_created:String=tx.query_row("SELECT created_at FROM recognition_blend_models WHERE scope=?1 AND model_id=?2",
            params![scope,current],|r|r.get(0)).map_err(|e|e.to_string())?;
        let candidate:Option<(String,String)>=tx.query_row("SELECT m.model_id,m.created_at FROM recognition_blend_models m
            WHERE m.scope=?1 AND julianday(m.created_at)>julianday(?2)
            AND json_extract(m.payload,'$.generationKind')='automatic_candidate_v1'
            AND json_extract(m.payload,'$.clientBuildVersion')=?3
            AND NOT EXISTS(SELECT 1 FROM recognition_blend_blocked b WHERE b.scope=m.scope AND b.model_id=m.model_id)
            ORDER BY julianday(m.created_at) ASC,m.model_id LIMIT 1",
            params![scope,current_created,env!("CARGO_PKG_VERSION")],|r|Ok((r.get(0)?,r.get(1)?)))
            .optional().map_err(|e|e.to_string())?;
        let Some((candidate,created))=candidate else{
            tx.commit().map_err(|e|e.to_string())?;return Ok(status("no_new_candidate"));
        };
        let ids=governance_orders(&created,false)?;
        if ids.len()<250{
            tx.commit().map_err(|e|e.to_string())?;return Ok(status("waiting_for_fixed_independent_cohort"));
        }
        let alpha=0.0125;
        let prepared=Self::prepare_paired_blend(&tx,&scope,&business,&current,&candidate,&ids,Some(&created),Some(alpha))?;
        tx.commit().map_err(|e|e.to_string())?;
        let report=Self::evaluate_paired_blend(&prepared,evaluate)?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        revalidate_governance_state(&tx,&scope,&expected_state)?;
        let report=Self::finalize_paired_blend(&tx,&scope,&business,&ids,&prepared,report)?;
        let blocked:i64=tx.query_row("SELECT count(*) FROM recognition_blend_blocked WHERE scope=?1 AND model_id=?2",
            params![scope,candidate],|r|r.get(0)).map_err(|e|e.to_string())?;
        if blocked!=0{return Err("후보 모델 상태 변경".into());}
        if governance_verdict(&report,alpha)!="promote"{
            // The first 250 post-creation orders form the capped evaluation
            // cohort. Do not let a rejected mature candidate starve later ones.
            if ids.len()==250{
                record_blend_transition(&tx,&scope,&current,&candidate,"reject",&report)?;
                tx.execute("INSERT OR IGNORE INTO recognition_blend_blocked VALUES(?1,?2)",params![scope,candidate])
                    .map_err(|e|e.to_string())?;
                tx.commit().map_err(|e|e.to_string())?;
                return Ok(status("candidate_rejected_at_cohort_limit"));
            }
            return Ok(status("candidate_not_approved"));
        }
        record_blend_transition(&tx,&scope,&current,&candidate,"promote",&report)?;
        set_selected_blend_model(&tx,&scope,Some(&candidate))?;
        tx.execute("INSERT INTO recognition_blend_monitor VALUES(?1,?2,?3,?4)
            ON CONFLICT(scope) DO UPDATE SET previous_model=excluded.previous_model,
            selected_model=excluded.selected_model,selected_at=excluded.selected_at",
            params![scope,current,candidate,chrono::Utc::now().to_rfc3339()]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e|e.to_string())?;
        let mut v=status("shadow_model_promoted");v["selectedModelId"]=json!(candidate);Ok(v)
    }

    pub(crate) fn recognition_learning_support(input:&Value,
        evaluate:impl Fn(&[Value])->Result<Value,String>)->Result<Value,String>{
        if let Some(performance)=input.get("combinedModelPerformance"){
            if ["blendModelRegistry","shadowEvaluation","combinedEvaluation","legacyScoreCalibration",
                "shadowCandidateCalibration","blendWeightLearning","localChronologicalReplay"].iter().any(|k|input.get(*k).is_some()){
                return Err("전체 모델 평가 요청을 분리하세요.".into());
            }
            return Self::combined_model_performance(performance);
        }
        if let Some(registry)=input.get("blendModelRegistry"){
            if ["shadowEvaluation","combinedEvaluation","legacyScoreCalibration","shadowCandidateCalibration","blendWeightLearning","localChronologicalReplay"]
                .iter().any(|key|input.get(*key).is_some()){return Err("모델 관리와 평가 요청을 분리하세요.".into());}
            return Self::blend_model_registry(registry);
        }
        let persist=input["blendWeightLearning"]["persistModel"]==true;
        let (mut conn,business,scope)=Self::learning_connection()?;
        let mut tx=conn.transaction().map_err(|e|e.to_string())?;
        // Read first even for explicit persistence. The fit runs outside the
        // transaction; take a write lock only to validate and store the model.
        let evaluation=input.get("shadowEvaluation");
        let combined_evaluation=input.get("combinedEvaluation");
        let calibration=input.get("legacyScoreCalibration");
        let shadow_calibration=input.get("shadowCandidateCalibration");
        let blend=input.get("blendWeightLearning");
        if [evaluation,combined_evaluation,calibration,shadow_calibration,blend,input.get("localChronologicalReplay")].iter().filter(|v|v.is_some()).count()>1{
            return Err("평가와 재현 요청을 분리하세요.".into());
        }
        let report=if let Some(config)=evaluation.or(combined_evaluation).or(calibration).or(shadow_calibration).or(blend).or_else(||input.get("localChronologicalReplay")){
            if input.get("chronologicalValidation").is_some() || config.get("samples").is_some() ||
                config.get("frozenModelId").is_some(){
                return Err("로컬 재현에 외부 예측·정답·모델을 혼합할 수 없습니다.".into());
            }
            let ids=config["orderIds"].as_array().filter(|v|!v.is_empty()&&v.len()<=250)
                .ok_or("로컬 재현은 1~250개 주문 ID를 명시해야 합니다.")?
                .iter().map(|id|id.as_str().filter(|s|!s.trim().is_empty()&&s.len()<=1024)
                    .map(str::to_owned).ok_or("재현 주문 ID 오류".to_owned()))
                .collect::<Result<Vec<_>,_>>()?;
            if ids.iter().collect::<std::collections::BTreeSet<_>>().len()!=ids.len(){
                return Err("재현 주문 ID 중복".into());
            }
            let (frozen,orders,digests)=crate::business_state::read_learning_inputs(&ids,|sources|{
                let (misses,digests)=select_learning_changes(&tx,&scope,sources)?;
                if !misses.is_empty(){return Err("원본과 불일치하는 학습 주문을 먼저 동기화하세요.".into());}
                // Stored combined evaluations need fingerprints, not decoded
                // confirmation snapshots or audit event bodies.
                let active=if combined_evaluation.is_some(){Vec::new()}else{
                    sources.iter().filter(|(_,s)|s.active).map(|(id,_)|id.clone()).collect()
                };
                Ok((active,digests))
            })?;
            let orders=attach_learning_audit(&tx,&frozen,orders)?;
            let mut result=if combined_evaluation.is_some(){
                let mut value=recognition_learning_store::combined_evaluation_report(&tx,&scope,&digests)?;
                value["currentSourceDigestsVerified"]=json!(true);value
            }else if evaluation.is_some()||calibration.is_some()||shadow_calibration.is_some()||blend.is_some(){
                tx.commit().map_err(|e|e.to_string())?;
                let mut value=evaluate(&orders)?;
                tx=if persist{conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)}
                    else{conn.transaction()}.map_err(|e|e.to_string())?;
                let current=crate::business_state::read_learning_sources(&ids)?;
                verify_learning_sync_sources(&tx,&frozen,&digests,&current)?;
                let (misses,latest)=select_learning_changes(&tx,&scope,&current)?;
                if !misses.is_empty()||latest!=digests{return Err("로컬 재현 중 학습 원장이 변경되었습니다.".into());}
                value["currentSourceDigestsVerified"]=json!(true);
                value
            }else{recognition_learning_store::assess_local_replay(&tx,&scope,input,&orders,&digests)?};
            if crate::business_state::read_learning_sources(&ids)?!=frozen{
                return Err("로컬 재현 중 확정 주문이 변경되었습니다.".into());
            }
            if blend.is_some()&&result["runtimeParameters"].is_object(){
                // Optional developer handoff only. No source text or recipient
                // data; model is never installed or enabled by this query.
                result["runtimeParameters"]["businessScope"]=json!(scope);
                result["runtimeParameters"]["sourceDigests"]=json!(digests);
            }
            if persist{
                if orders.len()!=ids.len()||digests.len()!=ids.len()||result["status"]!="blend_weight_diagnostic_only"{
                    return Err("모델 저장에 필요한 확정 원본 또는 학습 결과 누락".into());
                }
                let model=&result["runtimeParameters"];
                verify_blend_model_sources(&tx,&scope,&business,model,None)?;
                prepare_blend_registry(&tx)?;
                let id=store_blend_model(&tx,&scope,model)?;
                result["storedModelId"]=json!(id);
                result["modelAutomaticallySelected"]=json!(false);
            }
            result["requestedOrders"]=json!(ids.len());
            result["missingOrInactiveSourceOrders"]=json!(ids.len()-digests.len());
            result
        }else{recognition_learning_store::assess_support(&tx,&scope,input)?};
        if crate::business_state::authorized_business_write_path()?.canonicalize()
            .map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("학습 지지도 조회 중 업무 DB가 변경되었습니다.".into());
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    pub(crate) fn recognition_learning_integrity()->Result<Value,String>{
        let (mut conn,_,scope)=Self::learning_connection()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        recognition_learning_store::prepare(&tx)?;
        let report=recognition_learning_store::integrity(&tx,&scope)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    pub(crate) fn reset_recognition_learning()->Result<Value,String>{
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        recognition_learning_store::prepare(&tx)?;
        let report=recognition_learning_store::reset_derived(&tx,&scope)?;
        if blend_registry_exists(&tx)?{
            set_selected_blend_model(&tx,&scope,None)?;
            prepare_blend_governance(&tx)?;
            tx.execute("DELETE FROM recognition_blend_monitor WHERE scope=?1",[&scope]).map_err(|e|e.to_string())?;
        }
        if crate::business_state::authorized_business_write_path()?.canonicalize()
            .map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("학습 초기화 중 업무 DB가 변경되었습니다.".into());
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    pub(crate) fn rebuild_recognition_learning()->Result<Value,String>{
        let (mut conn,business,scope)=Self::learning_connection()?;
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        recognition_learning_store::prepare(&tx)?;
        let report=recognition_learning_store::rebuild_totals(&tx,&scope)?;
        if crate::business_state::authorized_business_write_path()?.canonicalize()
            .map_err(|_|"업무 DB 식별 실패")?!=business{
            return Err("학습 재구축 중 업무 DB가 변경되었습니다.".into());
        }
        tx.commit().map_err(|e|e.to_string())?;
        Ok(report)
    }

    pub fn path(&self) -> &std::path::Path { &self.path }
}

/// Read exact stored audit bytes through the existing order/time index.
 /// No serde parse, string copies, or event JSON construction on cache hits.
type LearningDigests=std::collections::BTreeMap<String,String>;
struct PreparedPairedReplay{
    payload:Value,
    frozen:crate::business_state::LearningSources,
    digests:LearningDigests,
}
fn read_paired_model(conn:&Connection,scope:&str,id:&str)->Result<Value,String>{
    let model=read_registered_blend_model(conn,scope,id)?;
    let created:String=conn.query_row("SELECT created_at FROM recognition_blend_models WHERE scope=?1 AND model_id=?2",
        params![scope,id],|r|r.get(0)).map_err(|e|e.to_string())?;
    Ok(json!({"modelId":id,"createdAt":created,"model":model}))
}
fn remaining_model_sources(expected:&serde_json::Map<String,Value>,verified:&LearningDigests)->Result<Vec<String>,String>{
    let mut extra=Vec::new();
    for (id,digest) in expected{
        if let Some(actual)=verified.get(id){
            if digest.as_str()!=Some(actual.as_str()){return Err("결합 모델 원본 변경".into());}
        }else{extra.push(id.clone());}
    }
    Ok(extra)
}
const AUTO_RETRAIN_MIN_ORDERS:usize=60;
const AUTO_RETRAIN_NEW_ORDERS:usize=20;
fn auto_retrain_status(status:&str,n:usize)->Value{
    json!({"status":status,"candidateOrders":n,"policyVersion":1,
        "minimumOrders":AUTO_RETRAIN_MIN_ORDERS,"minimumNewOrders":AUTO_RETRAIN_NEW_ORDERS,
        "modelAutomaticallySelected":false,"automaticChangeAuthorized":false,"decisionChanged":false})
}
fn auto_retrain_due(previous:&LearningDigests,current:&LearningDigests)->bool{
    let new=current.keys().filter(|id|!previous.contains_key(*id)).count();
    let changed=current.iter().any(|(id,digest)|previous.get(id).is_some_and(|old|old!=digest));
    // A modified shared source invalidates the old evidence even without
    // 20 new orders. A rolling-window eviction alone does not trigger.
    changed||new>=AUTO_RETRAIN_NEW_ORDERS
}
fn auto_retrain_config(orders:&[Value],now:chrono::DateTime<chrono::FixedOffset>,build:&str)->Option<Value>{
    if orders.len()<AUTO_RETRAIN_MIN_ORDERS||orders.len()>250{return None;}
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let mut times=Vec::new();let mut starts=Vec::new();
    for order in orders{
        let cap=&order["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["initialPredictionCapture"];
        let (start,predicted,confirmed)=(date(&cap["analysisStartedAt"])?,date(&cap["learningShadow"]["capturedAt"])?,date(&order["confirmedAt"])?);
        if !(start<=predicted&&predicted<confirmed&&confirmed<now){return None;}
        starts.push(start);times.push(predicted);
    }
    times.sort();starts.sort();
    let weight=times[times.len()*3/5];let test=times[times.len()*4/5];
    let fit=starts[0];
    if !(fit<weight&&weight<test&&test<now){return None;}
    let before=times.iter().filter(|t|**t<weight).count();
    let middle=times.iter().filter(|t|**t>=weight&&**t<test).count();
    let after=times.len()-before-middle;
    if before<20||middle<10||after<10{return None;}
    Some(json!({"fitStart":fit.to_rfc3339(),"weightStart":weight.to_rfc3339(),
        "testStart":test.to_rfc3339(),"testEnd":now.to_rfc3339(),
        "evaluationAsOf":now.to_rfc3339(),"clientBuildVersion":build}))
}

fn blend_governance_state(conn:&Connection,scope:&str)->Result<Value,String>{
    let selected=selected_blend_model(conn,scope)?;
    let monitor:Option<(String,String,String)>=conn.query_row("SELECT previous_model,selected_model,selected_at
        FROM recognition_blend_monitor WHERE scope=?1",[scope],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
        .optional().map_err(|e|e.to_string())?;
    let checkpoint:Option<(String,String,String,i64)>=conn.query_row("SELECT selected_model,selected_at,after_at,look_no
        FROM recognition_blend_monitor_checkpoint WHERE scope=?1",[scope],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
        .optional().map_err(|e|e.to_string())?;
    let revision:i64=conn.query_row("SELECT coalesce(max(id),0) FROM recognition_blend_transitions WHERE scope=?1",[scope],|r|r.get(0))
        .map_err(|e|e.to_string())?;
    Ok(json!([selected,monitor,checkpoint,revision]))
}
fn revalidate_governance_state(tx:&rusqlite::Transaction<'_>,scope:&str,expected_state:&Value)->Result<(),String>{
    if &blend_governance_state(tx,scope)?!=expected_state{return Err("모델 선택 또는 감시 상태 변경".into());}
    Ok(())
}
fn prepare_blend_governance(conn:&Connection)->Result<(),String>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS recognition_blend_monitor(
        scope TEXT PRIMARY KEY,previous_model TEXT NOT NULL,selected_model TEXT NOT NULL,selected_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS recognition_blend_monitor_checkpoint(
        scope TEXT PRIMARY KEY,selected_model TEXT NOT NULL,selected_at TEXT NOT NULL,after_at TEXT NOT NULL,look_no INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS recognition_blend_blocked(scope TEXT NOT NULL,model_id TEXT NOT NULL,
        PRIMARY KEY(scope,model_id));
        CREATE TABLE IF NOT EXISTS recognition_blend_transitions(
        id INTEGER PRIMARY KEY,scope TEXT NOT NULL,from_model TEXT NOT NULL,to_model TEXT NOT NULL,
        action TEXT NOT NULL,occurred_at TEXT NOT NULL,policy_version INTEGER NOT NULL,evaluation TEXT NOT NULL);")
        .map_err(|e|e.to_string())
}
fn record_blend_transition(conn:&Connection,scope:&str,from:&str,to:&str,action:&str,report:&Value)->Result<(),String>{
    let encoded=serde_json::to_string(report).map_err(|_|"모델 교체 평가 직렬화 오류")?;
    if encoded.len()>256*1024{return Err("모델 교체 평가 한도 초과".into());}
    conn.execute("INSERT INTO recognition_blend_transitions(scope,from_model,to_model,action,occurred_at,policy_version,evaluation)
        VALUES(?1,?2,?3,?4,?5,1,?6)",params![scope,from,to,action,chrono::Utc::now().to_rfc3339(),encoded])
        .map_err(|e|e.to_string())?;
    Ok(())
}
fn governance_orders(after:&str,recent:bool)->Result<Vec<String>,String>{
    let conn=crate::business_state::open_business_read_snapshot()?;
    let direction=if recent{"DESC"}else{"ASC"};
    let sql=format!("SELECT id FROM core_orders WHERE coalesce(json_extract(payload,'$.deletedAt'),'')=''
        AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.schemaVersion')=1
        AND json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.clientBuildVersion')=?1
        AND julianday(json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.analysisStartedAt'))>julianday(?2)
        AND julianday(confirmed_at) IS NOT NULL
        ORDER BY julianday(json_extract(payload,'$.confirmationSnapshot.recognitionBaseline.initialAnalysis.initialPredictionCapture.analysisStartedAt')) {direction},id {direction} LIMIT 250");
    let ids={
        let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![env!("CARGO_PKG_VERSION"),after],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?
    };
    conn.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    Ok(ids)
}
// Provisional engineering thresholds, not statistical certification.
fn governance_alpha(look:u64)->f64{
    // Per-monitoring-episode alpha budget: sum 1/(k(k+1)) = 1.
    // Four simultaneous one-sided/two-sided summaries per look.
    0.05/(4.0*look.max(1) as f64*(look.max(1) as f64+1.0))
}
fn governance_verdict(report:&Value,alpha:f64)->&'static str{
    if report["status"]!="paired_model_replay_diagnostic_only"||report["currentSourceDigestsVerified"]!=true||
        report["sameFrozenInputs"]!=true||report["missingOrInactiveOrders"]!=0||
        report["inferencePolicyVersion"]!=1||report["inferenceAlpha"].as_f64()!=Some(alpha){return "insufficient";}
    let get=|label:&str,min:u64|->Option<(f64,f64,u64,u64)>{
        let mut rows=report["groups"].as_array()?.iter().filter(|r|r["sourceKind"]=="all"&&r["legacyRoute"]=="all"&&
            r["contextLevel"]=="all"&&r["labelKind"]==label);
        let m=&rows.next()?["metrics"];
        if rows.next().is_some()||m["pairedOrders"].as_u64()?<min||m["pairedCoverage"].as_f64()?<0.8{return None;}
        let old=m["fallbackBaselineAgreement"].as_f64()?;let new=m["fallbackCandidateAgreement"].as_f64()?;
        let paired=m["pairedDelta"].as_f64()?;
        if ![old,new,paired].iter().all(|v|v.is_finite()){return None;}
        Some((new-old,paired,m["baselineOnlyAgrees"].as_u64()?,m["candidateOnlyAgrees"].as_u64()?))
    };
    let Some((u,up,ul,uw))=get("unchanged_proxy",30)else{return "insufficient";};
    let Some((c,cp,cl,cw))=get("explicit_correction",10)else{return "insufficient";};
    let inf=|label:&str,key:&str,field:&str|->Option<f64>{
        let m=report["groups"].as_array()?.iter().find(|r|r["sourceKind"]=="all"&&r["legacyRoute"]=="all"&&
            r["contextLevel"]=="all"&&r["labelKind"]==label)?;
        let v=&m["metrics"][key];
        if v["alpha"].as_f64()!=Some(alpha){return None;}
        v[field].as_f64().filter(|x|x.is_finite()&&(0.0..=1.0).contains(x))
    };
    let bound=|label:&str,key:&str,side:&str|->Option<f64>{
        let m=report["groups"].as_array()?.iter().find(|r|r["sourceKind"]=="all"&&r["legacyRoute"]=="all"&&
            r["contextLevel"]=="all"&&r["labelKind"]==label)?;
        let v=&m["metrics"][key];
        if v["alpha"].as_f64()!=Some(alpha){return None;}
        v["orderMeanInterval"][side].as_f64().filter(|x|x.is_finite()&&(-1.0..=1.0).contains(x))
    };
    let harmed=|label:&str|["fallbackOrderInference","pairedOrderInference"].iter().all(|key|
        bound(label,key,"upper").is_some_and(|x|x<0.0)&&
        inf(label,key,"oneSidedDeteriorationSignP").is_some_and(|p|p<=alpha));
    if (u<=-0.02&&ul>=uw.saturating_add(3)&&harmed("unchanged_proxy"))||
        (c<=-0.10&&cl>=cw.saturating_add(3)&&harmed("explicit_correction")){return "rollback";}
    let safe= ["fallbackOrderInference","pairedOrderInference"].iter().all(|key|
        inf("unchanged_proxy",key,"orderHarmUpper").is_some_and(|x|x<=0.05));
    let superior=["fallbackOrderInference","pairedOrderInference"].iter().all(|key|
        bound("explicit_correction",key,"lower").is_some_and(|x|x>0.0)&&
        inf("explicit_correction",key,"oneSidedImprovementSignP").is_some_and(|p|p<=alpha));
    if safe&&superior&&u>=-1e-10&&up>=-1e-10&&c>=0.05-1e-10&&cp>=0.05-1e-10&&cw>=cl.saturating_add(3){return "promote";}
    if safe&&u>=-1e-10&&up>=-1e-10&&c>=-1e-10&&cp>=-1e-10{return "stable";}
    "insufficient"
}
fn blend_registry_exists(conn:&Connection)->Result<bool,String>{
    conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN
        ('recognition_blend_models','recognition_blend_selection')",[],|r|r.get::<_,i64>(0))
        .map_err(|e|e.to_string()).and_then(|n|match n{0=>Ok(false),2=>Ok(true),_=>Err("결합 모델 저장소 불완전".into())})
}
fn prepare_blend_registry(conn:&Connection)->Result<(),String>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS recognition_blend_models(
        scope TEXT NOT NULL,model_id TEXT NOT NULL,registry_version INTEGER NOT NULL CHECK(registry_version=1),
        payload TEXT NOT NULL,created_at TEXT NOT NULL,available_after TEXT NOT NULL,
        PRIMARY KEY(scope,model_id));
        CREATE TABLE IF NOT EXISTS recognition_blend_selection(
        scope TEXT PRIMARY KEY,model_id TEXT NOT NULL,
        FOREIGN KEY(scope,model_id) REFERENCES recognition_blend_models(scope,model_id));")
        .map_err(|e|e.to_string())
}
fn blend_model_identity(model:&Value)->Result<(String,String),String>{
    use sha2::{Digest,Sha256};
    let encoded=serde_json::to_string(model).map_err(|_|"결합 모델 직렬화 오류")?;
    if encoded.len()>256*1024{return Err("결합 모델 저장 한도 초과".into());}
    Ok((format!("blend-{:x}",Sha256::digest(encoded.as_bytes())),encoded))
}
fn validate_registry_model(scope:&str,model:&Value)->Result<(),String>{
    if model["businessScope"].as_str()!=Some(scope)||model["schemaVersion"]!=1||
        model["automaticChangeAuthorized"]!=false||model["clientBuildVersion"].as_str()!=Some(env!("CARGO_PKG_VERSION"))||
        model["scorePolicyVersion"]!="owner-margin-30-v1"||model["captureVersion"]!=4||model["supportVersion"]!=4||
        model["featureVersion"]!=1||model["contributionVersion"]!=2||model["safetyPolicyVersion"]!=1||
        model["explicitToWeakRatio"].as_f64()!=Some(3.0){
        return Err("결합 모델 규격 또는 업무 DB 불일치".into());
    }
    let available=chrono::DateTime::parse_from_rfc3339(model["availableAfter"].as_str().ok_or("모델 시각 누락")?)
        .map_err(|_|"모델 시각 오류")?;
    if available>chrono::Utc::now(){return Err("미래 시점 결합 모델".into());}
    let sources=model["sourceDigests"].as_object().filter(|m|!m.is_empty()&&m.len()<=250).ok_or("모델 원본 지문 누락")?;
    if sources.iter().any(|(id,d)|id.is_empty()||id.len()>1024||d.as_str().is_none_or(|s|
        s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit()))){return Err("모델 원본 지문 오류".into());}
    for key in ["legacyBins","legacyGates","shadowModels","blendModels"]{
        if model[key].as_array().is_none_or(|a|a.is_empty()||a.len()>64){return Err("모델 보정 구성 누락".into());}
    }
    if !model["blendModels"].as_array().unwrap().iter().any(|m|m["weightFitOrders"].as_u64().is_some_and(|n|n>=10)&&
        m["learningWeight"].as_f64().is_some_and(|w|w.is_finite()&&(0.0..=1.0).contains(&w))){
        return Err("사용 가능한 결합 가중치 없음".into());
    }
    Ok(())
}
fn store_blend_model(conn:&Connection,scope:&str,model:&Value)->Result<String,String>{
    validate_registry_model(scope,model)?;
    let (id,payload)=blend_model_identity(model)?;
    let available=model["availableAfter"].as_str().ok_or("모델 시각 누락")?;
    conn.execute("INSERT INTO recognition_blend_models(scope,model_id,registry_version,payload,created_at,available_after)
        VALUES(?1,?2,1,?3,?4,?5) ON CONFLICT(scope,model_id) DO NOTHING",
        params![scope,id,payload,chrono::Utc::now().to_rfc3339(),available]).map_err(|e|e.to_string())?;
    // A hash collision or corrupted pre-existing row is never overwritten.
    if read_registered_blend_model(conn,scope,&id)?!=*model{return Err("결합 모델 충돌".into());}
    Ok(id)
}
fn read_registered_blend_model(conn:&Connection,scope:&str,id:&str)->Result<Value,String>{
    if id.len()!=70||!id.starts_with("blend-"){return Err("결합 모델 ID 오류".into());}
    let (version,payload):(i64,String)=conn.query_row("SELECT registry_version,payload FROM recognition_blend_models
        WHERE scope=?1 AND model_id=?2",params![scope,id],|r|Ok((r.get(0)?,r.get(1)?)))
        .map_err(|_|"결합 모델 조회 실패")?;
    if version!=1||payload.len()>256*1024{return Err("결합 모델 저장 규격 오류".into());}
    let model:Value=serde_json::from_str(&payload).map_err(|_|"결합 모델 JSON 오류")?;
    let (actual,encoded)=blend_model_identity(&model)?;
    if actual!=id||encoded!=payload{return Err("결합 모델 지문 불일치".into());}
    validate_registry_model(scope,&model)?;
    Ok(model)
}
fn selected_blend_model(conn:&Connection,scope:&str)->Result<Option<String>,String>{
    conn.query_row("SELECT model_id FROM recognition_blend_selection WHERE scope=?1",[scope],|r|r.get(0))
        .optional().map_err(|e|e.to_string())
}
fn set_selected_blend_model(conn:&Connection,scope:&str,id:Option<&str>)->Result<(),String>{
    if let Some(id)=id{
        conn.execute("INSERT INTO recognition_blend_selection(scope,model_id) VALUES(?1,?2)
            ON CONFLICT(scope) DO UPDATE SET model_id=excluded.model_id",params![scope,id]).map_err(|e|e.to_string())?;
    }else{conn.execute("DELETE FROM recognition_blend_selection WHERE scope=?1",[scope]).map_err(|e|e.to_string())?;}
    Ok(())
}
fn verify_blend_model_pair_sources(tx:&rusqlite::Transaction<'_>,scope:&str,business:&std::path::Path,
    first:&Value,second:&Value)->Result<(),String>{
    let mut expected=LearningDigests::new();
    for model in [first,second]{
        validate_registry_model(scope,model)?;
        for (id,value) in model["sourceDigests"].as_object().ok_or("모델 원본 지문 누락")?{
            let digest=value.as_str().ok_or("모델 원본 지문 오류")?;
            if expected.insert(id.clone(),digest.to_owned()).is_some_and(|old|old!=digest){
                return Err("두 모델의 공통 원본 지문 불일치".into());
            }
        }
    }
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=1",
        [scope],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending!=0{return Err("복구 대기 학습 주문".into());}
    let ids=expected.keys().cloned().collect::<Vec<_>>();
    let source=crate::business_state::open_business_read_snapshot()?;
    let mut frozen=crate::business_state::LearningSources::new();
    for chunk in ids.chunks(250){
        let current=crate::business_state::learning_sources(&source,chunk)?;
        let (misses,digests)=select_learning_changes(tx,scope,&current)?;
        if !misses.is_empty()||digests.len()!=chunk.len()||digests.iter().any(|(id,d)|expected.get(id)!=Some(d)){
            return Err("결합 모델 원본 변경".into());
        }
        frozen.extend(current);
    }
    source.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    let latest=crate::business_state::open_business_read_snapshot()?;
    for chunk in ids.chunks(250){
        let current=crate::business_state::learning_sources(&latest,chunk)?;
        if current.len()!=chunk.len()||current.iter().any(|(id,s)|frozen.get(id)!=Some(s)){
            return Err("결합 모델 원본 재검증 중 변경".into());
        }
    }
    latest.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    if crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
        return Err("업무 DB 변경".into());
    }
    Ok(())
}
fn verify_blend_model_sources(tx:&rusqlite::Transaction<'_>,scope:&str,business:&std::path::Path,
    model:&Value,target:Option<&str>)->Result<(),String>{
    validate_registry_model(scope,model)?;
    let expected=model["sourceDigests"].as_object().ok_or("모델 원본 지문 누락")?;
    if target.is_some_and(|id|expected.contains_key(id)){return Err("결합 모델 자기 주문 포함".into());}
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=1",
        [scope],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending!=0{return Err("복구 대기 학습 주문".into());}
    let ids=expected.keys().cloned().collect::<Vec<_>>();
    let (frozen,_,digests)=crate::business_state::read_learning_inputs(&ids,|sources|{
        let (misses,digests)=select_learning_changes(tx,scope,sources)?;
        if !misses.is_empty(){return Err("결합 모델 원본 동기화 필요".into());}
        Ok((Vec::new(),digests))
    })?;
    if json!(digests)!=model["sourceDigests"]||crate::business_state::read_learning_sources(&ids)?!=frozen||
        crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business{
        return Err("결합 모델 원본 변경".into());
    }
    Ok(())
}

fn select_learning_changes(tx:&rusqlite::Transaction<'_>,scope:&str,
    sources:&crate::business_state::LearningSources)->Result<(Vec<String>,LearningDigests),String>{
    let digests=learning_source_digests(tx,sources)?;
    let mut misses=Vec::new();
    for (id,digest) in &digests{
        if recognition_learning_store::cached_count(tx,scope,id,digest)?.is_none(){misses.push(id.clone());}
    }
    Ok((misses,digests))
}
fn learning_source_digests(tx:&rusqlite::Transaction<'_>,
    sources:&crate::business_state::LearningSources)->Result<LearningDigests,String>{
    use sha2::{Digest,Sha256};
    let audit_ids=sources.iter().filter(|(_,source)|source.audit_needed)
        .map(|(id,_)|id.clone()).collect::<Vec<_>>();
    let audit=if audit_ids.is_empty(){std::collections::BTreeMap::new()}
        else{query_learning_audit_digests(tx,&audit_ids)?};
    let mut digests=std::collections::BTreeMap::new();
    for (id,source) in sources.iter().filter(|(_,source)|source.active){
        let mut hash=Sha256::new();hash.update(b"bando-learning-raw-input-v2\0");
        hash.update(source.digest.as_bytes());
        if source.audit_needed{
            hash.update([1]);
            hash.update(audit.get(id).ok_or("학습 감사 지문 누락")?.as_bytes());
        }else{hash.update([0]);}
        let digest=format!("{:x}",hash.finalize());
        digests.insert(id.clone(),digest);
    }
    Ok(digests)
}

fn attach_learning_audit(tx:&rusqlite::Transaction<'_>,
    frozen:&crate::business_state::LearningSources,mut orders:Vec<Value>)->Result<Vec<Value>,String>{
    let audit_ids=orders.iter().filter_map(|order|order["id"].as_str())
        .filter(|id|frozen.get(*id).is_some_and(|source|source.audit_needed))
        .map(str::to_owned).collect::<Vec<_>>();
    let mut audit=if audit_ids.is_empty(){std::collections::BTreeMap::new()}
        else{query_reception_audit_batch(tx,&audit_ids)?};
    for order in &mut orders{
        let id=order["id"].as_str().ok_or("확정 주문 ID 누락")?;
        if let Some(events)=audit.remove(id){order["recognitionAuditEvents"]=json!(events);}
    }
    Ok(orders)
}

fn prepare_learning_summaries(orders:&[Value],
    summarize:impl Fn(&Value)->Result<Value,String>)->Result<std::collections::BTreeMap<String,Value>,String>{
    let mut summaries=std::collections::BTreeMap::new();
    for order in orders{
        let id=order["id"].as_str().ok_or("확정 주문 ID 누락")?;
        if summaries.insert(id.to_owned(),summarize(order)?).is_some(){return Err("학습 주문 중복".into());}
    }
    Ok(summaries)
}

fn verify_learning_sync_sources(tx:&rusqlite::Transaction<'_>,
    frozen:&crate::business_state::LearningSources,digests:&LearningDigests,
    current:&crate::business_state::LearningSources)->Result<(),String>{
    if current!=frozen||learning_source_digests(tx,current)?!=*digests{
        return Err("학습 연산 중 확정 주문 또는 정정 이력이 변경되었습니다.".into());
    }
    Ok(())
}

fn apply_prepared_learning_changes(tx:&rusqlite::Transaction<'_>,scope:&str,ids:&[String],
    frozen:&crate::business_state::LearningSources,orders:&[Value],digests:&LearningDigests,
    summaries:&std::collections::BTreeMap<String,Value>)->Result<Value,String>{
    let mut report=recognition_learning_store::synchronize_with_digests(tx,scope,ids,digests,orders,|order|{
        let id=order["id"].as_str().ok_or("확정 주문 ID 누락")?;
        summaries.get(id).cloned().ok_or_else(||"준비된 학습 결과 누락: 재동기화 필요".to_owned())
    })?;
    report["decodedOrders"]=json!(orders.len());
    let requested=ids.iter().collect::<std::collections::BTreeSet<_>>().len();
    report["missingSelectedOrders"]=json!(requested.saturating_sub(frozen.len()));
    report["inactiveSelectedOrders"]=json!(frozen.values().filter(|source|!source.active).count());
    Ok(report)
}

fn query_learning_audit_digests(conn:&Connection,ids:&[String])->Result<std::collections::BTreeMap<String,String>,String>{
    use sha2::{Digest,Sha256};
    let unique=ids.iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len()>250||unique.iter().any(|id|id.trim().is_empty()){
        return Err("감사 지문 조회 범위 오류".into());
    }
    let has_context:i64=conn.query_row(
        "SELECT count(*) FROM pragma_table_info('reception_edit_audit') WHERE name='context_json'",
        [],|r|r.get(0)).map_err(|e|e.to_string())?;
    let context=if has_context==1{"context_json"}else{"NULL"};
    let sql=format!("SELECT id,order_id,source_raw,initial_analysis_json,before_rows_json,
        after_rows_json,recorded_at,{context} FROM reception_edit_audit
        WHERE order_id=?1 ORDER BY recorded_at,id");
    let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
    let mut out=std::collections::BTreeMap::new();
    let mut total_rows=0usize;let mut total_bytes=0usize;
    for id in unique{
        let mut hash=Sha256::new();hash.update(b"bando-audit-learning-source-v1\0");
        let mut rows=stmt.query(params![id]).map_err(|e|e.to_string())?;
        let mut count=0usize;
        while let Some(row)=rows.next().map_err(|e|e.to_string())?{
            count+=1;total_rows+=1;
            if count>1000||total_rows>4000{return Err("감사 지문 조회 건수 한도 초과".into());}
            for col in 0..8{
                total_bytes=total_bytes.saturating_add(crate::business_state::hash_learning_text(
                    &mut hash,row.get_ref(col).map_err(|e|e.to_string())?)?);
                if total_bytes>32*1024*1024{return Err("감사 지문 조회 용량 한도 초과".into());}
            }
        }
        hash.update((count as u64).to_le_bytes());
        out.insert(id.clone(),format!("{:x}",hash.finalize()));
    }
    Ok(out)
}

fn query_reception_audit_batch(conn:&Connection,ids:&[String])->Result<std::collections::BTreeMap<String,Vec<Value>>,String>{
    let unique=ids.iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len()>250||unique.iter().any(|id|id.trim().is_empty()){
        return Err("감사 조회 주문 범위 오류".into());
    }
    let mut out=std::collections::BTreeMap::new();
    let mut total_rows=0usize;let mut total_bytes=0usize;
    let has_context:i64=conn.query_row(
        "SELECT count(*) FROM pragma_table_info('reception_edit_audit') WHERE name='context_json'",
        [],|r|r.get(0)).map_err(|e|e.to_string())?;
    let context=if has_context==1{"context_json"}else{"NULL"};
    let sql=format!("SELECT id,order_id,source_raw,initial_analysis_json,before_rows_json,after_rows_json,recorded_at,{context}
        FROM reception_edit_audit WHERE order_id=?1 ORDER BY recorded_at,id LIMIT 1001");
    let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
    for id in unique{
        let mut rows=stmt.query(params![id]).map_err(|e|e.to_string())?;
        let mut events=Vec::new();
        while let Some(row)=rows.next().map_err(|e|e.to_string())?{
            total_rows+=1;
            for col in [2,3,4,5,7]{
                match row.get_ref(col).map_err(|e|e.to_string())?{
                    rusqlite::types::ValueRef::Text(bytes)=>total_bytes=total_bytes.saturating_add(bytes.len()),
                    rusqlite::types::ValueRef::Null if col==7=>{},
                    _=>return Err("정정 감사 자료 유형 오류".into()),
                }
            }
            if events.len()>=1000||total_rows>4000||total_bytes>32*1024*1024{
                return Err("정정 감사 조회 한도 초과: 주문 범위를 줄여 다시 조회하세요.".into());
            }
            let parse=|col:usize|->Result<Value,String>{
                let s=row.get::<_,String>(col).map_err(|e|e.to_string())?;
                serde_json::from_str(&s).map_err(|e|format!("저장된 정정 이력 JSON 오류: {e}"))
            };
            let context:Option<String>=row.get(7).map_err(|e|e.to_string())?;
            let context=context.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(|e|e.to_string())?;
            events.push(json!({"id":row.get::<_,String>(0).map_err(|e|e.to_string())?,
                "orderId":row.get::<_,String>(1).map_err(|e|e.to_string())?,
                "sourceRaw":row.get::<_,String>(2).map_err(|e|e.to_string())?,
                "initialAnalysis":parse(3)?,"beforeRows":parse(4)?,"afterRows":parse(5)?,
                "at":row.get::<_,String>(6).map_err(|e|e.to_string())?,"context":context}));
        }
        out.insert(id.clone(),events);
    }
    Ok(out)
}

// Stable keyset paging: no unbounded loading and no write on the audit path.
// A missing historical context is preserved as NULL, not reconstructed.
fn query_reception_audit_page(conn:&Connection,query:&Value)->Result<Value,String>{
    let limit=query.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1,250) as usize;
    let order_id=query.get("orderId").and_then(Value::as_str);
    let client_id=query.get("clientId").and_then(Value::as_str);
    let since=query.get("since").and_then(Value::as_str);
    let cursor=query.get("cursor").filter(|v|!v.is_null());
    let (at,id)=if let Some(c)=cursor{
        (Some(c["at"].as_str().filter(|v|!v.is_empty()).ok_or("감사 조회 커서 시각 오류")?),
         Some(c["id"].as_str().filter(|v|!v.is_empty()).ok_or("감사 조회 커서 ID 오류")?))
    }else{(None,None)};
    let has_context:i64=conn.query_row(
        "SELECT count(*) FROM pragma_table_info('reception_edit_audit') WHERE name='context_json'",
        [],|r|r.get(0)).map_err(|e|e.to_string())?;
    let context=if has_context==1{"context_json"}else{"NULL"};
    let sql=format!("SELECT id,order_id,source_raw,initial_analysis_json,
        before_rows_json,after_rows_json,recorded_at,{context}
        FROM reception_edit_audit
        WHERE (?1 IS NULL OR order_id=?1)
          AND (?2 IS NULL OR json_extract({context},'$.clientId')=?2)
          AND (?3 IS NULL OR recorded_at>=?3)
          AND (?4 IS NULL OR (recorded_at,id)>(?4,?5))
        ORDER BY recorded_at,id LIMIT ?6");
    let mut stmt=conn.prepare(&sql).map_err(|e|e.to_string())?;
    let records=stmt.query_map(params![order_id,client_id,since,at,id,(limit+1) as i64],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
        r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,
        r.get::<_,String>(6)?,r.get::<_,Option<String>>(7)?
    ))).map_err(|e|e.to_string())?;
    let mut events=Vec::new();
    let mut has_more=false;
    for record in records{
        let (id,order,raw,initial,before,after,at,context)=record.map_err(|e|e.to_string())?;
        if events.len()==limit{has_more=true;break;}
        let parse=|s:&str|serde_json::from_str::<Value>(s)
            .map_err(|e|format!("저장된 정정 이력 JSON 오류: {e}"));
        events.push(json!({"id":id,"orderId":order,"sourceRaw":raw,
            "initialAnalysis":parse(&initial)?,"beforeRows":parse(&before)?,
            "afterRows":parse(&after)?,"at":at,
            "context":context.as_deref().map(parse).transpose()?}));
    }
    let next=if has_more{events.last().map(|v|json!({"at":v["at"],"id":v["id"]}))}else{None};
    Ok(json!({"events":events,"nextCursor":next}))
}

// Legacy events have unknown context. Never synthesize historical evidence.
fn ensure_reception_audit_context(conn:&Connection)->Result<(),String>{
    conn.execute_batch("BEGIN IMMEDIATE;").map_err(|e|e.to_string())?;
    let result=(||->Result<(),String>{
        let exists:i64=conn.query_row(
            "SELECT count(*) FROM pragma_table_info('reception_edit_audit') WHERE name='context_json'",
            [],|row|row.get(0)).map_err(|e|e.to_string())?;
        if exists==0{
            conn.execute_batch("ALTER TABLE reception_edit_audit ADD COLUMN context_json TEXT
                CHECK(context_json IS NULL OR json_valid(context_json));")
                .map_err(|e|e.to_string())?;
        }
        conn.execute_batch("COMMIT;").map_err(|e|e.to_string())
    })();
    if result.is_err(){let _=conn.execute_batch("ROLLBACK;");}
    result.map_err(|e|format!("수정 감사 이력 확장 실패: {e}"))
}

/// Audit the actual native worksheet, not an inferred reconstruction of
/// the initial parser output. Identical final values may have different
/// source bindings; a no-op or a reverted gesture must not count as a correction.
pub(crate) fn reception_edit_event(
    row:&WorkingReception,raw:&str,before:&Value,after_model:&Value,
    after_order:&[String],after_rows:&Value,
)->Option<Value>{
    if raw.trim().is_empty() || row.raw!=raw ||
        row.analysis_raw.as_deref()!=Some(raw){return None;}
    let original=row.analysis_evidence.as_ref()
        .filter(|v|v["recipients"].is_array())?;
    let previous=before.get("model")?;
    let before_rows=previous.get("rows").filter(|v|v.is_array())?;
    let snapshot=|model:&Value,order:&Value|json!({
        "blocks":model["blocks"],"assignments":model["assignments"],
        "assignmentOrder":order
    });
    let before_classification=snapshot(previous,&before["assignmentOrder"]);
    let after_classification=snapshot(after_model,&json!(after_order));
    if !valid_audit_classification(&before_classification) ||
       !valid_audit_classification(&after_classification) ||
       !after_model["rows"].is_array() || !after_rows.is_array(){return None;}
    if before_rows==&after_model["rows"] && before_classification==after_classification{
        return None;
    }
    Some(json!({
        "id":uuid::Uuid::new_v4().to_string(),
        "at":chrono::Utc::now().to_rfc3339(),
        "sourceRaw":raw,"initialAnalysis":original,
        "beforeRows":before_rows,"afterRows":after_rows,
        "context":{
            "schemaVersion":1,
            "beforeClassification":before_classification,
            "afterClassification":after_classification,
            "clientId":row.client_id,"clientSelection":row.client_selection,
            "orderType":row.order_type,
            "sourceInputMethod":row.source_input_method,
            "initialRecognition":original.get("recognitionContext"),
            "appVersionAtEdit":env!("CARGO_PKG_VERSION")
        }
    }))
}

fn valid_audit_classification(value:&Value)->bool{
    value.is_object() && value["blocks"].is_array() &&
        value["assignments"].is_object() && value["assignmentOrder"].is_array()
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
        for event in &draft.edit_history {
            if event["id"].as_str().is_none_or(|value|value.trim().is_empty()) ||
               event["at"].as_str().is_none_or(|value|value.trim().is_empty()) ||
               event["sourceRaw"].as_str().is_none_or(str::is_empty) ||
               !event["initialAnalysis"]["recipients"].is_array() ||
               !event["beforeRows"].is_array() ||
               !event["afterRows"].is_array() {
                return Err("수정 전후의 감사 스냅샷이 손상되었습니다.".into());
            }
            if let Some(context)=event.get("context").filter(|v|!v.is_null()){
                if !context.is_object() || context["schemaVersion"]!=1 ||
                    !valid_audit_classification(&context["beforeClassification"]) ||
                    !valid_audit_classification(&context["afterClassification"]) {
                    return Err("원문 배정 감사 이력의 형식이 잘못되었습니다.".into());
                }
            }
            analysis_bytes=analysis_bytes.checked_add(event.to_string().len())
                .ok_or("수정 이력의 크기를 확인할 수 없습니다.")?;
        }
        if let Some(price)=&draft.pricing_preview {
            let keys=["sourceRaw","orderId","orderType","clientId","authorityRevision",
                "status","shippingFee","shippingVersion","vatAmount",
                "subtotal","total","pendingRecipients","notified","paid","linePrices","freightLines","pricingBasis"];
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
            if let Some(basis)=price.get("pricingBasis").filter(|value|!value.is_null()){
                if !basis.is_object() ||
                   !matches!(basis.get("schema").and_then(Value::as_u64),Some(1|2)) ||
                   (basis["schema"]==2 &&
                    !basis.pointer("/orderInputs/recipients")
                        .is_some_and(Value::is_array)) {
                    return Err("거래처·상품·운임 계산 기준 스냅샷이 올바르지 않습니다.".into());
                }
            }
            if let Some(pending)=price.get("pendingRecipients"){
                if !pending.is_array(){
                    return Err("미확정 배송건 계산 결과는 배열이어야 합니다.".into());
                }
                for recipient in pending.as_array().into_iter().flatten(){
                    if recipient.get("recipientId").and_then(Value::as_str).is_none() ||
                       !recipient.get("product").is_some_and(Value::is_boolean) ||
                       !recipient.get("freight").is_some_and(Value::is_boolean) ||
                       !recipient.get("vat").is_some_and(Value::is_boolean) {
                        return Err("미확정 배송건 계산 근거가 올바르지 않습니다.".into());
                    }
                }
            }
            for line in price["linePrices"].as_array().into_iter().flatten() {
                let allowed=["recipientId","itemId","productId","variantId",
                    "quantity","unitPrice","manualUnitPrice","amount"];
                let fields=line.as_object().ok_or("Rust 주문 품목 가격 결과 오류")?;
                if fields.keys().any(|key|!allowed.contains(&key.as_str())) {
                    return Err("Rust 계산 결과에 허용하지 않은 개인정보가 포함됐습니다.".into());
                }
            }
            if let Some(freight)=price.get("freightLines"){
                let rows=freight.as_array().ok_or("수취인별 운임 계산 형식 오류")?;
                for line in rows{
                    let columns=line.as_object().ok_or("수취인 운임 행 오류")?;
                    if columns.keys().any(|key|!matches!(key.as_str(),
                        "recipientId"|"origin"|"appliedCharge"|"billableCharge"
                        |"freightCode"|"source")) {
                        return Err("허용되지 않은 운임 계산 필드입니다.".into());
                    }
                    if !matches!(line["origin"].as_str(),Some("FACTORY")|Some("STORE")){
                        return Err("계산된 운임 발송지를 확인하세요.".into());
                    }
                }
            }
            analysis_bytes=analysis_bytes.checked_add(price.to_string().len())
                .ok_or("Rust 계산 결과 총 크기 초과")?;
        }
        if let Some(view)=draft.analysis_view.as_ref(){
            if draft.analysis_raw.as_deref()!=Some(draft.raw.as_str()) ||
               draft.analysis_evidence.is_none() ||
               draft.client_match.is_none() ||
               !view.is_object() ||
               !view["reference"].is_array() ||
               !view["clientCandidates"].is_array() ||
               !view["directoryCount"].is_u64() {
                return Err("저장된 발주서 인식 화면의 기준이 맞지 않습니다.".into());
            }
            analysis_bytes=analysis_bytes.checked_add(view.to_string().len())
                .ok_or("접수 인식 화면 기록 크기 초과")?;
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

#[cfg(test)]
mod ocr_layout_tests {
    use super::*;

    #[test]
    fn narrow_table_wraps_are_recoverable_without_changing_content(){
        // Synthetic layout, not a customer document or real phone number.
        let raw="김\n수\n현\n010-1234-\n5678\n서울특별시 강남구 테\n헤란로 3\n뮤직\n트레이 회색\n1개\n메모\n문 앞에";
        let fixed="김수현\n010-1234-5678\n서울특별시 강남구 테헤란로 3\n뮤직트레이 회색 1개\n메모 문 앞에";
        let trace=layout_correction_trace(raw,fixed);
        assert_eq!(trace["contentAligned"],true);
        assert_eq!(trace["schema"],"ocr-layout-boundaries-v1");
        let changes=trace["boundaryChanges"].as_array().expect("changes");
        assert!(changes.iter().any(|x|
            x["wasLineBreak"]==true && x["after"]==""));
        assert!(changes.iter().any(|x|
            x["wasLineBreak"]==true && x["after"]==" "));
        assert!(changes.len()>=6);
    }

    #[test]
    fn character_recognition_errors_are_not_guessed_as_layout_only(){
        let trace=layout_correction_trace(
            "서울특별시 테\n헤란로 3","서울특별시 테헤란로 8"
        );
        assert_eq!(trace["contentAligned"],false);
        assert_eq!(trace["boundaryChanges"],json!([]));
    }
}

#[cfg(test)]
mod reception_edit_audit_tests {
    use super::*;
    #[test]
    fn runtime_model_verification_reuses_only_matching_shared_sources(){
        let expected=json!({"shared":"a","model-only":"b"});
        let expected=expected.as_object().unwrap();
        let mut verified=LearningDigests::from([("shared".into(),"a".into()),("support-only".into(),"c".into())]);
        assert_eq!(remaining_model_sources(expected,&verified).unwrap(),vec!["model-only".to_owned()]);
        verified.insert("model-only".into(),"b".into());
        assert!(remaining_model_sources(expected,&verified).unwrap().is_empty());
        verified.insert("shared".into(),"changed".into());
        assert!(remaining_model_sources(expected,&verified).is_err());
    }
    #[test]
    fn governance_never_promotes_on_missing_coverage_or_normal_case_regression(){
        let alpha=0.0125;
        let group=|label:&str,n:u64,old:f64,new:f64,delta:f64,l:u64,w:u64,lower:f64,upper:f64,harm:f64|{
            let inf=json!({"alpha":alpha,"orderMeanInterval":{"lower":lower,"upper":upper},
                "orderHarmUpper":harm,"oneSidedImprovementSignP":0.001,"oneSidedDeteriorationSignP":0.001});
            json!({"sourceKind":"all","legacyRoute":"all","contextLevel":"all","labelKind":label,
                "metrics":{"pairedOrders":n,"pairedCoverage":1.0,"fallbackBaselineAgreement":old,
                    "fallbackCandidateAgreement":new,"pairedDelta":delta,"baselineOnlyAgrees":l,"candidateOnlyAgrees":w,
                    "fallbackOrderInference":inf,"pairedOrderInference":inf}})
        };
        let mut r=json!({"status":"paired_model_replay_diagnostic_only","sameFrozenInputs":true,
            "currentSourceDigestsVerified":true,"missingOrInactiveOrders":0,
            "inferencePolicyVersion":1,"inferenceAlpha":alpha,
            "groups":[group("unchanged_proxy",100,1.0,1.0,0.0,0,0,-0.2,0.2,0.043),
                group("explicit_correction",50,0.2,0.8,0.6,0,30,0.1,0.9,0.04)]});
        assert_eq!(governance_verdict(&r,alpha),"promote");
        r["groups"][0]["metrics"]["pairedCoverage"]=json!(0.5);
        assert_eq!(governance_verdict(&r,alpha),"insufficient");
        r["groups"][0]=group("unchanged_proxy",100,1.0,0.5,-0.5,50,0,-0.8,-0.1,0.6);
        assert_eq!(governance_verdict(&r,alpha),"rollback");
        r["groups"][0]=group("unchanged_proxy",100,1.0,0.99,-0.01,1,0,-0.2,0.2,0.1);
        assert_eq!(governance_verdict(&r,alpha),"insufficient");
        assert!(governance_alpha(2)<governance_alpha(1));
    }

    #[test]
    fn automatic_retrain_uses_distinct_time_windows_and_new_order_threshold(){
        let old=(0..60).map(|i|(format!("id-{i}"),"a".repeat(64))).collect::<LearningDigests>();
        let mut current=old.clone();
        assert!(!auto_retrain_due(&old,&current));
        for i in 60..79{current.insert(format!("id-{i}"),"b".repeat(64));}
        assert!(!auto_retrain_due(&old,&current));
        current.insert("id-79".into(),"b".repeat(64));
        assert!(auto_retrain_due(&old,&current));
        let mut changed=old.clone();changed.insert("id-0".into(),"b".repeat(64));
        assert!(auto_retrain_due(&old,&changed));
        let base=chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap();
        let orders=(0..60).map(|i|{
            let start=base+chrono::Duration::hours(i*2);
            let predicted=start+chrono::Duration::minutes(1);
            let confirmed=start+chrono::Duration::minutes(2);
            json!({"confirmedAt":confirmed.to_rfc3339(),"confirmationSnapshot":{"recognitionBaseline":{"initialAnalysis":{
                "initialPredictionCapture":{"analysisStartedAt":start.to_rfc3339(),"learningShadow":{"capturedAt":predicted.to_rfc3339()}}
            }}}})
        }).collect::<Vec<_>>();
        let now=base+chrono::Duration::days(10);
        assert!(auto_retrain_config(&orders,now,"test").is_some());
        assert!(auto_retrain_config(&orders[..59],now,"test").is_none());
        assert!(auto_retrain_config(&vec![orders[0].clone();60],now,"test").is_none());
    }

    #[test]
    fn blend_registry_stores_immutable_revisions_and_separate_selection(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        assert!(!blend_registry_exists(&conn).unwrap());
        prepare_blend_registry(&conn).unwrap();
        let mut model=json!({"businessScope":"scope","schemaVersion":1,"automaticChangeAuthorized":false,
            "clientBuildVersion":env!("CARGO_PKG_VERSION"),"scorePolicyVersion":"owner-margin-30-v1",
            "captureVersion":4,"supportVersion":4,"featureVersion":1,"contributionVersion":2,"safetyPolicyVersion":1,
            "explicitToWeakRatio":3.0,"availableAfter":"2020-01-01T00:00:00Z",
            "sourceDigests":{"synthetic":"a".repeat(64)},"legacyBins":[{}],"legacyGates":[{}],
            "shadowModels":[{}],"blendModels":[{"weightFitOrders":10,"learningWeight":0.5}]});
        let a=store_blend_model(&conn,"scope",&model).unwrap();
        assert_eq!(store_blend_model(&conn,"scope",&model).unwrap(),a);
        assert_eq!(selected_blend_model(&conn,"scope").unwrap(),None);
        set_selected_blend_model(&conn,"scope",Some(&a)).unwrap();
        model["blendModels"][0]["learningWeight"]=json!(0.6);
        let b=store_blend_model(&conn,"scope",&model).unwrap();
        assert_ne!(a,b);
        assert_eq!(selected_blend_model(&conn,"scope").unwrap(),Some(a.clone()));
        assert!(read_registered_blend_model(&conn,"other",&a).is_err());
        assert!(set_selected_blend_model(&conn,"other",Some(&a)).is_err());
        set_selected_blend_model(&conn,"scope",Some(&b)).unwrap();
        set_selected_blend_model(&conn,"scope",Some(&a)).unwrap();
        conn.execute("UPDATE recognition_blend_models SET payload='{}' WHERE scope='scope' AND model_id=?1",[&a]).unwrap();
        assert!(read_registered_blend_model(&conn,"scope",&a).is_err());
        set_selected_blend_model(&conn,"scope",None).unwrap();
        assert!(selected_blend_model(&conn,"scope").unwrap().is_none());
        assert_eq!(read_registered_blend_model(&conn,"scope",&b).unwrap(),model);
        model["automaticChangeAuthorized"]=json!(true);
        assert!(store_blend_model(&conn,"scope",&model).is_err());
    }

    #[test]
    fn learning_two_database_disk_replay_uses_real_snapshot_and_audit_paths(){
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
        let fixture=Fixture(std::env::temp_dir().join(format!("bando-learning-integration-{}",uuid::Uuid::new_v4())));
        std::fs::create_dir(&fixture.0).unwrap();
        let business_path=fixture.0.join("business.sqlite3");
        let audit_path=fixture.0.join("audit.sqlite3");
        let business=Connection::open(&business_path).unwrap();
        business.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE core_orders(id TEXT PRIMARY KEY,confirmed_at TEXT,payload TEXT);").unwrap();
        let workflow=json!({"workflow":{"groups":[{"id":"g"}],"shipments":[{"orderId":"a","groupId":"g","step":1}]}});
        business.execute("INSERT INTO app_state VALUES(1,?1)",[workflow.to_string()]).unwrap();
        let order=json!({"confirmationSnapshot":{"raw":"raw","recognitionBaseline":{"schemaVersion":1,"auditEventIds":["e"]}}});
        business.execute("INSERT INTO core_orders VALUES('a','t',?1)",[order.to_string()]).unwrap();
        let audit=Connection::open(&audit_path).unwrap();
        audit.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE reception_edit_audit(id TEXT PRIMARY KEY,order_id TEXT,
            source_raw TEXT,initial_analysis_json TEXT,before_rows_json TEXT,after_rows_json TEXT,recorded_at TEXT,context_json TEXT);
            INSERT INTO reception_edit_audit VALUES('e','a','raw','{}','[]','[]','t',NULL);").unwrap();
        recognition_learning_store::prepare(&audit).unwrap();
        drop(audit);drop(business);

        // Synthetic count projection only. The actual SQL snapshot, cache,
        // audit reading and delta application are the production functions.
        let replay=|summarize:fn(&Value)->Result<Value,String>|->Value{
            let business=Connection::open_with_flags(&business_path,OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            business.execute_batch("PRAGMA query_only=ON;BEGIN").unwrap();
            let mut audit=Connection::open(&audit_path).unwrap();
            audit.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            let tx=audit.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
            let ids=vec!["a".to_owned()];
            let (frozen,orders,digests)=crate::business_state::learning_inputs_from_snapshot(&business,&ids,|sources|
                select_learning_changes(&tx,"fixture",sources)).unwrap();
            let orders=attach_learning_audit(&tx,&frozen,orders).unwrap();
            business.execute_batch("COMMIT").unwrap();tx.commit().unwrap();
            let summaries=prepare_learning_summaries(&orders,summarize).unwrap();
            let tx=audit.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
            business.execute_batch("BEGIN").unwrap();
            let current=crate::business_state::learning_sources(&business,&ids).unwrap();
            verify_learning_sync_sources(&tx,&frozen,&digests,&current).unwrap();
            let report=apply_prepared_learning_changes(&tx,"fixture",&ids,&frozen,&orders,&digests,&summaries).unwrap();
            business.execute_batch("COMMIT").unwrap();tx.commit().unwrap();report
        };
        fn summarize(order:&Value)->Result<Value,String>{
            let valid=order["recognitionAuditEvents"].as_array().is_some_and(|events|events.len()==1);
            Ok(json!({"featureVersion":1,"uniqueOrders":1,"status":"diagnostic_only",
                "eligibleForDurableLearning":false,"auditCoverage":{"verifiedManifestOrders":if valid{1}else{0}},
                "statistics":[],"fieldContextStatistics":[],"fieldStatistics":[{"task":"type","counts":{
                "observationCount":1,"independentOrders":1,"weakIndependentOrders":1,
                "weakUnchangedCount":1,"explicitCorrectionCount":0,"explicitCorrectionOrders":0,
                "pendingCorrectionCount":0,"unresolvedCount":0,"eligibleForAutomaticDecision":false,
                "weakCandidates":{"address":1},"weakCandidateOrders":{"address":1},
                "explicitPositiveCandidates":{},"explicitNegativeCandidates":{},
                "explicitPositiveCandidateOrders":{},"explicitNegativeCandidateOrders":{}}}]}))
        }
        assert_eq!(replay(summarize)["storedContributingOrders"],1);
        assert_eq!(replay(|_|panic!("unchanged order must not decode"))["decodedOrders"],0);
        // A concurrent audit writer may commit while projection is running,
        // but its changed source must invalidate the prepared result.
        {
            let business=Connection::open_with_flags(&business_path,OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            business.execute_batch("PRAGMA query_only=ON;BEGIN").unwrap();
            let mut audit=Connection::open(&audit_path).unwrap();
            audit.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            let ids=vec!["a".to_owned()];
            let tx=audit.transaction().unwrap();
            let (frozen,orders,digests)=crate::business_state::learning_inputs_from_snapshot(&business,&ids,|sources|{
                Ok((ids.clone(),learning_source_digests(&tx,sources)?))
            }).unwrap();
            let orders=attach_learning_audit(&tx,&frozen,orders).unwrap();
            business.execute_batch("COMMIT").unwrap();tx.commit().unwrap();
            let summaries=prepare_learning_summaries(&orders,|order|{
                let writer=Connection::open(&audit_path).unwrap();
                writer.execute("UPDATE reception_edit_audit SET recorded_at='changed' WHERE id='e'",[])
                    .map_err(|e|e.to_string())?;
                summarize(order)
            }).unwrap();
            assert_eq!(summaries.len(),1);
            let tx=audit.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
            business.execute_batch("BEGIN").unwrap();
            let current=crate::business_state::learning_sources(&business,&ids).unwrap();
            assert!(verify_learning_sync_sources(&tx,&frozen,&digests,&current).is_err());
            business.execute_batch("COMMIT").unwrap();tx.rollback().unwrap();
            audit.execute("UPDATE reception_edit_audit SET recorded_at='t' WHERE id='e'",[]).unwrap();
        }
        assert_eq!(replay(|_|panic!("unchanged order must not decode"))["decodedOrders"],0);
        let audit=Connection::open(&audit_path).unwrap();
        audit.execute("DELETE FROM reception_edit_audit WHERE id='e'",[]).unwrap();drop(audit);
        let revoked=replay(summarize);assert_eq!(revoked["decodedOrders"],1);
        assert_eq!(revoked["storedContributingOrders"],0);
        let business=Connection::open(&business_path).unwrap();
        business.execute("DELETE FROM core_orders WHERE id='a'",[]).unwrap();drop(business);
        assert_eq!(replay(|_|panic!("missing order"))["missingSelectedOrders"],1);
        let mut audit=Connection::open(&audit_path).unwrap();
        audit.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        let tx=audit.transaction().unwrap();
        assert_eq!(recognition_learning_store::integrity(&tx,"fixture").unwrap()["internallyConsistent"],true);
        assert!(recognition_learning_store::order_page(&tx,"fixture","",16).unwrap().is_empty());
        tx.commit().unwrap();
    }
    #[test]
    fn learning_raw_audit_digest_detects_same_id_edits_missing_rows_and_null(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE reception_edit_audit(id TEXT PRIMARY KEY,order_id TEXT,
            source_raw TEXT,initial_analysis_json TEXT,before_rows_json TEXT,after_rows_json TEXT,
            recorded_at TEXT,context_json TEXT)").unwrap();
        let ids=vec!["a".into()];
        let empty=query_learning_audit_digests(&conn,&ids).unwrap();
        conn.execute("INSERT INTO reception_edit_audit VALUES('e','a','raw','{}','[]','[]','t',NULL)",[]).unwrap();
        let first=query_learning_audit_digests(&conn,&ids).unwrap();
        assert_ne!(first,empty);
        assert_eq!(first,query_learning_audit_digests(&conn,&ids).unwrap());
        conn.execute("UPDATE reception_edit_audit SET after_rows_json='[1]' WHERE id='e'",[]).unwrap();
        let edited=query_learning_audit_digests(&conn,&ids).unwrap();
        assert_ne!(edited,first);
        conn.execute("UPDATE reception_edit_audit SET context_json='null' WHERE id='e'",[]).unwrap();
        assert_ne!(query_learning_audit_digests(&conn,&ids).unwrap(),edited);
        conn.execute("INSERT INTO reception_edit_audit VALUES('unrelated','b','x','{}','[]','[]','t',NULL)",[]).unwrap();
        conn.execute("DELETE FROM reception_edit_audit WHERE id='e'",[]).unwrap();
        assert_eq!(query_learning_audit_digests(&conn,&ids).unwrap(),empty);
    }
    #[test]
    fn audit_batch_reads_complete_selected_pages_and_rejects_oversized_history(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE reception_edit_audit(
            id TEXT PRIMARY KEY,order_id TEXT,source_raw TEXT,initial_analysis_json TEXT,
            before_rows_json TEXT,after_rows_json TEXT,recorded_at TEXT,context_json TEXT);").unwrap();
        for i in 0..251{
            conn.execute("INSERT INTO reception_edit_audit VALUES (?1,'o','709','{}','[]','[]','t',NULL)",
                params![format!("e{i:04}")]).unwrap();
        }
        conn.execute("INSERT INTO reception_edit_audit VALUES ('other','other','709','{}','[]','[]','t',NULL)",[]).unwrap();
        let batch=query_reception_audit_batch(&conn,&["o".into(),"o".into(),"empty".into()]).unwrap();
        assert_eq!(batch.len(),2);
        assert_eq!(batch["o"].len(),251);
        assert_eq!(batch["o"][250]["id"],"e0250");
        assert!(batch["empty"].is_empty());
        assert!(!batch.contains_key("other"));
        for i in 251..1001{
            conn.execute("INSERT INTO reception_edit_audit VALUES (?1,'o','709','{}','[]','[]','t',NULL)",
                params![format!("e{i:04}")]).unwrap();
        }
        assert!(query_reception_audit_batch(&conn,&["o".into()]).is_err());
    }

    #[test]
    fn correction_query_pages_and_filters_without_inventing_legacy_context(){
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE reception_edit_audit(
            id TEXT PRIMARY KEY,order_id TEXT,source_raw TEXT,initial_analysis_json TEXT,
            before_rows_json TEXT,after_rows_json TEXT,recorded_at TEXT,context_json TEXT);
            INSERT INTO reception_edit_audit VALUES
            ('a','o1','709','{\"recipients\":[]}','[]','[]','2026-11-01',NULL),
            ('b','o2','709','{\"recipients\":[]}','[]','[]','2026-11-01','{\"clientId\":\"c1\"}'),
            ('c','o3','270','{\"recipients\":[]}','[]','[]','2026-11-02','{\"clientId\":\"c2\"}');").unwrap();
        let first=query_reception_audit_page(&conn,&json!({"limit":1})).unwrap();
        assert_eq!(first["events"][0]["id"],"a");
        assert!(first["events"][0]["context"].is_null());
        let second=query_reception_audit_page(&conn,&json!({"limit":1,"cursor":first["nextCursor"]})).unwrap();
        assert_eq!(second["events"][0]["id"],"b");
        let client=query_reception_audit_page(&conn,&json!({"clientId":"c1"})).unwrap();
        assert_eq!(client["events"].as_array().unwrap().len(),1);
        assert_eq!(client["events"][0]["id"],"b");
        let last=query_reception_audit_page(&conn,&json!({"since":"2026-11-02","orderId":"o3"})).unwrap();
        assert_eq!(last["events"][0]["id"],"c");
        assert!(last["nextCursor"].is_null());
    }

    #[test]
    fn binding_only_correction_is_captured_without_counting_a_noop(){
        let rows=json!([{"id":"r1","name":"가상","items":[]}]);
        let blocks=json!([{"id":"0:2","text":"가상","field":"address"}]);
        let before=json!({"model":{"rows":rows,"blocks":blocks,"assignments":{}},
            "assignmentOrder":[]});
        let mut after=before["model"].clone();
        after["assignments"]["0:2"]=json!({"recipientId":"r1","field":"name","value":"가상"});
        let row=WorkingReception{
            id:"synthetic-order".into(),raw:"가상".into(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some("가상".into()),
            analysis_evidence:Some(json!({"recipients":rows,"candidates":[],"unassigned":[],
                "recognitionContext":{"catalogRevision":42,"clientId":"c1"}})),
            analysis_view:None,client_match:Some(json!({})),
            chat_handle:None,chat_title:None,client_id:Some("c1".into()),
            client_selection:Some("MANUAL".into()),pricing_preview:None,
            working_version:None,working_synced:false,manual_model:None,edit_history:vec![]
        };
        assert!(reception_edit_event(&row,"가상",&before,&before["model"],&[],&rows).is_none());
        let event=reception_edit_event(&row,"가상",&before,&after,&["0:2".into()],&rows).unwrap();
        assert_eq!(event["beforeRows"],event["afterRows"]);
        assert_eq!(event["context"]["beforeClassification"]["blocks"][0]["field"],"address");
        assert_eq!(event["context"]["afterClassification"]["assignments"]["0:2"]["field"],"name");
        assert_eq!(event["context"]["initialRecognition"]["catalogRevision"],42);
        assert_eq!(event["context"]["clientSelection"],"MANUAL");
        assert!(reception_edit_event(&row,"다른 원문",&before,&after,&["0:2".into()],&rows).is_none());
    }

    #[test]
    fn initial_and_revised_snapshots_are_cas_committed_and_survive_draft_removal() {
        let path=std::env::temp_dir().join(format!(
            "bando-egui-edit-audit-{}.sqlite",uuid::Uuid::new_v4()
        ));
        let conn=Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE working_source(
                id INTEGER PRIMARY KEY,raw TEXT NOT NULL,order_type TEXT NOT NULL,
                revision INTEGER NOT NULL,active_id TEXT NOT NULL,
                drafts_json TEXT NOT NULL,updated_at TEXT
             );
             INSERT INTO working_source
                (id,raw,order_type,revision,active_id,drafts_json)
             VALUES(1,'','AGENCY',0,'','[]');
             CREATE TABLE reception_edit_audit(
                id TEXT PRIMARY KEY,order_id TEXT NOT NULL,source_raw TEXT NOT NULL,
                initial_analysis_json TEXT NOT NULL,
                before_rows_json TEXT NOT NULL,after_rows_json TEXT NOT NULL,
                recorded_at TEXT NOT NULL
             );"
        ).unwrap();
        conn.execute("INSERT INTO reception_edit_audit VALUES
            ('legacy','legacy-order','원문','{}','[]','[]','2026-11-21')",[]).unwrap();
        ensure_reception_audit_context(&conn).unwrap();
        ensure_reception_audit_context(&conn).unwrap();
        let legacy:Option<String>=conn.query_row(
            "SELECT context_json FROM reception_edit_audit WHERE id='legacy'",[],
            |row|row.get(0)).unwrap();
        assert!(legacy.is_none());
        drop(conn);
        let store=PreviewStore{path:path.clone()};
        let initial=json!({
            "recipients":[{"id":"r1","name":"초기 수취인"}],
            "candidates":[],"unassigned":[]
        });
        let updated=json!([{"id":"r1","name":"수정 수취인"}]);
        let draft=WorkingReception{
            id:"order-1".into(),raw:"원문".into(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some("원문".into()),
            analysis_evidence:Some(initial.clone()),analysis_view:None,
            client_match:Some(json!({})),
            chat_handle:None,chat_title:None,client_id:None,
            client_selection:None,pricing_preview:None,
            working_version:None,working_synced:false,manual_model:None,
            edit_history:vec![json!({
                "id":"change-1","at":"2026-11-22T00:00:00Z","sourceRaw":"원문",
                "initialAnalysis":initial,"beforeRows":[{"id":"r1","name":"초기 수취인"}],
                "afterRows":updated,
                "context":{"schemaVersion":1,"clientId":"c1",
                    "beforeClassification":{"blocks":[{"id":"0:3","field":"quantity","text":"709"}],
                        "assignments":{},"assignmentOrder":[]},
                    "afterClassification":{"blocks":[{"id":"0:3","field":"quantity","text":"709"}],
                        "assignments":{"0:3":{"field":"product","recipientId":"r1","itemId":"i1"}},
                        "assignmentOrder":["0:3"]}}
            })]
        };
        let source=WorkingSource{
            raw:"원문".into(),order_type:"AGENCY".into(),revision:0,
            active_id:Some("order-1".into()),drafts:vec![draft]
        };
        assert_eq!(store.save_source(&source).unwrap(),1);
        let conn=store.open_connection().unwrap();
        let (before,after):(String,String)=conn.query_row(
            "SELECT before_rows_json,after_rows_json FROM reception_edit_audit
             WHERE id='change-1'",[],|row|Ok((row.get(0)?,row.get(1)?))
        ).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&before).unwrap()[0]["name"],
            "초기 수취인");
        assert_eq!(serde_json::from_str::<Value>(&after).unwrap()[0]["name"],
            "수정 수취인");
        drop(conn);
        assert_eq!(store.save_source(&WorkingSource{
            raw:"".into(),order_type:"AGENCY".into(),revision:1,
            active_id:None,drafts:vec![]
        }).unwrap(),2);
        let conn=store.open_connection().unwrap();
        let preserved:i64=conn.query_row(
            "SELECT count(*) FROM reception_edit_audit WHERE order_id='order-1'",
            [],|row|row.get(0)
        ).unwrap();
        assert_eq!(preserved,1);
        let context:String=conn.query_row(
            "SELECT context_json FROM reception_edit_audit WHERE id='change-1'",[],
            |row|row.get(0)).unwrap();
        let context:Value=serde_json::from_str(&context).unwrap();
        assert_eq!(context["beforeClassification"]["blocks"][0]["field"],"quantity");
        assert_eq!(context["afterClassification"]["assignments"]["0:3"]["field"],"product");
        assert_eq!(context["clientId"],"c1");
        drop(conn);
        let _=std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod cached_reception_roundtrip_tests {
    use super::*;

    #[test]
    fn completed_pricing_and_recognition_survive_sqlite_save_reopen(){
        let path=std::env::temp_dir().join(format!(
            "bando-egui-cached-quote-{}.sqlite",uuid::Uuid::new_v4()
        ));
        let conn=Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE working_source(
                id INTEGER PRIMARY KEY,raw TEXT NOT NULL,order_type TEXT NOT NULL,
                revision INTEGER NOT NULL,active_id TEXT NOT NULL,
                drafts_json TEXT NOT NULL,updated_at TEXT
             );
             INSERT INTO working_source
                (id,raw,order_type,revision,active_id,drafts_json)
             VALUES(1,'','AGENCY',0,'','[]');"
        ).unwrap();
        drop(conn);
        let store=PreviewStore{path:path.clone()};
        let quote=json!({
            "sourceRaw":"원문","orderId":"order-1","orderType":"AGENCY",
            "clientId":"c1","authorityRevision":44,"status":"REVIEW",
            "shippingFee":3000,"shippingVersion":1,
            "vatAmount":0,"subtotal":12000,"total":15000,
            "pendingRecipients":[],"notified":false,"paid":false,
            "linePrices":[{"recipientId":"r1","itemId":"i1",
                "productId":"p","variantId":"v","quantity":2,
                "unitPrice":6000,"manualUnitPrice":null,"amount":12000}],
            "freightLines":[{"recipientId":"r1","origin":"FACTORY",
                "appliedCharge":3000,"billableCharge":3000,
                "freightCode":"010","source":"RULE"}]
        });
        let recognized=json!({
            "recipients":[{
                "id":"r1","name":"수취인","address":"서울",
                "items":[{"id":"i1","productId":"p","variantId":"v",
                    "quantity":2,"unitPrice":6000}]
            }],"candidates":[],"unassigned":[]
        });
        let cache=json!({
            "reference":[{"remote":null,"remoteError":null,
                "itemPrices":[{"retail":6000,"error":null}]}],
            "clientCandidates":[{"clientId":"c1"}],
            "matchedClientName":"테스트 거래처",
            "chosenClientName":"테스트 거래처","directoryCount":1
        });
        let draft=WorkingReception{
            id:"order-1".into(),raw:"원문".into(),order_type:"AGENCY".into(),
            source_input_method:None,last_input_method:None,
            analysis_raw:Some("원문".into()),
            analysis_evidence:Some(recognized.clone()),
            analysis_view:Some(cache.clone()),
            client_match:Some(json!({"clientId":"c1"})),
            chat_handle:None,chat_title:None,client_id:Some("c1".into()),
            client_selection:Some("MANUAL".into()),
            pricing_preview:Some(quote.clone()),working_version:None,
            working_synced:false,manual_model:None,edit_history:Vec::new(),
        };
        let source=WorkingSource{
            raw:"원문".into(),order_type:"AGENCY".into(),revision:0,
            active_id:Some("order-1".into()),drafts:vec![draft]
        };
        assert_eq!(store.save_source(&source).unwrap(),1);
        let loaded=store.load_source().unwrap();
        assert_eq!(loaded.revision,1);
        assert_eq!(loaded.active_id.as_deref(),Some("order-1"));
        assert_eq!(loaded.drafts[0].pricing_preview.as_ref(),Some(&quote));
        assert_eq!(loaded.drafts[0].analysis_evidence.as_ref(),Some(&recognized));
        assert_eq!(loaded.drafts[0].analysis_view.as_ref(),Some(&cache));
        assert_eq!(loaded.drafts[0].client_id.as_deref(),Some("c1"));
        drop(store);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pending_recipient_details_must_keep_native_field_types(){
        let mut source=WorkingSource{
            raw:"원문".into(),order_type:"AGENCY".into(),revision:0,
            active_id:Some("order-1".into()),drafts:vec![WorkingReception{
                id:"order-1".into(),raw:"원문".into(),
                order_type:"AGENCY".into(),
                source_input_method:None,last_input_method:None,
                analysis_raw:None,analysis_evidence:None,analysis_view:None,
                client_match:None,chat_handle:None,chat_title:None,
                client_id:Some("c1".into()),
                client_selection:Some("MANUAL".into()),
                pricing_preview:Some(json!({
                    "sourceRaw":"원문","orderId":"order-1",
                    "orderType":"AGENCY","clientId":"c1",
                    "authorityRevision":4,"linePrices":[],
                    "freightLines":[],"pendingRecipients":[]
                })),
                working_version:None,working_synced:false,manual_model:None,
                edit_history:Vec::new(),
            }]
        };
        assert!(validate_source(&source).is_ok());
        source.drafts[0].pricing_preview.as_mut().unwrap()["pendingRecipients"]=
            json!([{"recipientId":"r1","product":"yes",
                "freight":true,"vat":false}]);
        assert!(validate_source(&source).is_err());
    }
}
