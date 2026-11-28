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
                tx.execute(
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
    pub(crate) fn read_reception_audit_page(query:&Value)->Result<Value,String>{
        let path=Self::directory()?.join("egui-preview.sqlite3");
        let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e|format!("정정 이력 읽기 실패: {e}"))?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
        let signature:String=conn.query_row(
            "SELECT value FROM preview_meta WHERE key='signature'",[],|r|r.get(0))
            .map_err(|e|e.to_string())?;
        if signature!=SIGNATURE{return Err("egui 전용 DB 식별자가 다릅니다.".into());}
        query_reception_audit_page(&conn,query)
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

    // Reuse the isolated audit DB and the old transactional delta ledger.
    // A confirmed order never depends on derived learning storage success.
    // Mark/revoke before computing; failure leaves a pending, non-voting order.
    pub(crate) fn synchronize_learning(state:&Value,ids:&[String],
        summarize:impl Fn(&Value)->Result<Value,String>)->Result<(),String>{
        if ids.is_empty(){return Ok(());}
        static SERIAL:std::sync::Mutex<()>=std::sync::Mutex::new(());
        let _guard=SERIAL.lock().map_err(|_|"학습 갱신 잠금 오류")?;
        let (mut conn,business,scope)=Self::learning_connection()?;
        recognition_learning_store::prepare(&conn)?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        for id in ids{recognition_learning_store::mark_pending(&tx,&scope,id)?;}
        tx.commit().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        for id in ids{recognition_learning_store::revoke_pending(&tx,&scope,id)?;}
        tx.commit().map_err(|e|e.to_string())?;
        let business_conn=Connection::open_with_flags(&business,OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e|e.to_string())?;
        business_conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
        let current_revision=||->Result<u64,String>{
            business_conn.query_row("SELECT json_extract(payload,'$.revision') FROM app_state WHERE id=1",
                [],|r|r.get(0)).map_err(|e|e.to_string())
        };
        let revision=state["revision"].as_u64().ok_or("학습 원본 revision 누락")?;
        if current_revision()?!=revision{return Err("학습 원본 변경: 갱신 보류".into());}
        let mut prepared=Vec::new();
        for id in ids{
            let mut input=learning_input(state,id);
            if input["learningActive"]==true{
                let mut events=Vec::new();let mut cursor=Value::Null;
                loop{
                    let page=query_reception_audit_page(&conn,&json!({"orderId":id,"limit":250,"cursor":cursor}))?;
                    events.extend(page["events"].as_array().ok_or("정정 감사 조회 오류")?.iter().cloned());
                    if events.len()>1000{return Err("정정 감사 조회 한도 초과".into());}
                    cursor=page["nextCursor"].clone();
                    if cursor.is_null(){break;}
                }
                input["recognitionAuditEvents"]=json!(events);
            }
            let report=summarize(&input)?;
            prepared.push(recognition_learning_store::prepare_order(&input,&report)?);
        }
        if crate::business_state::authorized_business_write_path()?.canonicalize().map_err(|_|"업무 DB 식별 실패")?!=business
            ||current_revision()?!=revision{return Err("학습 원본 변경: 갱신 보류".into());}
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        for item in &prepared{recognition_learning_store::apply(&tx,&scope,item)?;}
        tx.commit().map_err(|e|e.to_string())?;
        // Another process may have committed between the revision read and
        // the separate derived DB commit. Revoke this stale contribution.
        if current_revision()?!=revision{
            let tx=conn.transaction().map_err(|e|e.to_string())?;
            for id in ids{
                recognition_learning_store::mark_pending(&tx,&scope,id)?;
                recognition_learning_store::revoke_pending(&tx,&scope,id)?;
            }
            tx.commit().map_err(|e|e.to_string())?;
            return Err("학습 원본 변경: 갱신 보류".into());
        }
        Ok(())
    }

    pub fn path(&self) -> &std::path::Path { &self.path }
}

// Match only learning-relevant source fields, never prices or shipping fees.
// A live re-correction with no new verified audit chain revokes the old vote.
fn learning_rows(rows:&Value)->Value{
    let sources=|r:&Value|r["sources"].as_array().into_iter().flatten()
        .map(|s|json!([s["start"],s["end"],s["field"],s["text"]])).collect::<Vec<_>>();
    Value::Array(rows.as_array().into_iter().flatten().map(|r|{
        let items=r["items"].as_array().into_iter().flatten().map(|i|json!({
            "productId":i["productId"],"variantId":i["variantId"],"name":i["name"],
            "color":i["color"],"quantity":i["quantity"],"quantityConfirmed":i["quantityConfirmed"],
            "sources":sources(i)
        })).collect::<Vec<_>>();
        json!({"name":r["name"],"address":r["address"],"phone":r["phone"],"mobile":r["mobile"],
            "note":r["note"],"sources":sources(r),"items":items})
    }).collect())
}
pub(crate) fn learning_input(state:&Value,id:&str)->Value{
    let order=state["orders"].as_array().into_iter().flatten()
        .find(|o|o["id"].as_str()==Some(id)).unwrap_or(&Value::Null);
    let snapshot=&order["confirmationSnapshot"];
    let mut shipments=state["workflow"]["shipments"].as_array().into_iter().flatten()
        .filter(|s|s["orderId"].as_str()==Some(id)&&s["legacyArchived"]!=true).peekable();
    let active=shipments.peek().is_some()&&shipments.all(|s|{
        s["step"].as_i64().is_some_and(|v|(1..=4).contains(&v))&&
        state["workflow"]["groups"].as_array().into_iter().flatten().any(|g|
            g["id"]==s["groupId"]&&g["deletedAt"].as_str().is_none_or(|v|v.is_empty()))
    });
    // A later explicit edit is not an untouched sample even if the user
    // restores the same value. Reuse the existing workflow audit history.
    let shipment_ids=state["workflow"]["shipments"].as_array().into_iter().flatten()
        .filter(|s|s["orderId"].as_str()==Some(id))
        .filter_map(|s|s["id"].as_str()).collect::<HashSet<_>>();
    let confirmed=order["confirmedAt"].as_str().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok());
    let later_edit=state["workflow"]["events"].as_array().into_iter().flatten().any(|e|
        e["action"]=="확정 주문 수정"&&matches!(e["reason"].as_str(),
            Some("name"|"address"|"phone"|"mobile"|"note"|"quantity"|"product"|"color"|"variant"|"addItem"|"removeItem"))
        &&e["shipmentIds"].as_array().into_iter().flatten().any(|s|s.as_str().is_some_and(|id|shipment_ids.contains(id)))
        &&e["at"].as_str().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok())
            .zip(confirmed.as_ref()).is_some_and(|(a,b)|a>=*b));
    let active=active&&!later_edit&&order["deletedAt"].as_str().is_none_or(|v|v.is_empty())
        &&snapshot["recognitionBaseline"]["schemaVersion"]==1
        &&order["raw"]==snapshot["raw"]&&order["clientId"]==snapshot["data"]["clientId"]
        &&learning_rows(&order["recipients"])==learning_rows(&snapshot["recipients"]);
    json!({"id":id,"confirmedAt":order["confirmedAt"],"deletedAt":order["deletedAt"],
        "learningActive":active,"confirmationSnapshot":snapshot,"recognitionAuditEvents":[]})
}
#[cfg(test)]
mod pattern_learning_lifecycle_tests{
    use super::*;
    #[test]
    fn learning_lifecycle_retracts_cancelled_deleted_and_recorrected_orders(){
        let row=json!({"name":"가","items":[{"productId":"p","quantity":1}]});
        let mut state=json!({"orders":[{"id":"a","confirmedAt":"t","raw":"가","clientId":"c",
            "recipients":[row],"confirmationSnapshot":{"at":"t","raw":"가","recipients":[row],
            "data":{"clientId":"c"},"recognitionBaseline":{"schemaVersion":1}}}],
            "workflow":{"groups":[{"id":"g"}],"shipments":[{"orderId":"a","groupId":"g","step":1}]}});
        assert_eq!(learning_input(&state,"a")["learningActive"],true);
        state["workflow"]["shipments"][0]["step"]=json!(0);
        assert_eq!(learning_input(&state,"a")["learningActive"],false);
        state["workflow"]["shipments"][0]["step"]=json!(1);
        state["workflow"]["groups"][0]["deletedAt"]=json!("t");
        assert_eq!(learning_input(&state,"a")["learningActive"],false);
        state["workflow"]["groups"][0]["deletedAt"]=Value::Null;
        state["orders"][0]["recipients"][0]["items"][0]["quantity"]=json!(2);
        assert_eq!(learning_input(&state,"a")["learningActive"],false);
        state["orders"][0]["recipients"][0]["items"][0]["quantity"]=json!(1);
        assert_eq!(learning_input(&state,"a")["learningActive"],true);
        assert_eq!(learning_input(&state,"missing")["learningActive"],false);
    }
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
