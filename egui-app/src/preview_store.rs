//! Isolated SQLite storage for egui reception work; not the Tauri DB.
//! Every received work item gets its own persistent ID and revision.
//! "주문 확인 완료" (billing/order history) is deliberately NOT implemented.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

const SIGNATURE: &str = "bando-hub-egui-preview-v1";
const SOURCE_LIMIT_BYTES: usize = 2 * 1024 * 1024;
const MAX_WORKING_ORDERS: i64 = 1000;

#[derive(Clone, Debug)]
pub struct WorkingSource {
    pub id: i64,
    pub raw: String,
    pub revision: u64,
    pub created_at: String,
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
            .map_err(|e| format!("egui 전용 DB 폴더 생성 실패: {e}"))?;
        let store = Self { path: directory.join("egui-preview.sqlite3") };
        let existed = store.path.exists();
        let mut conn = store.open_connection()?;
        if existed {
            let found: Option<String> = conn.query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='preview_meta'",
                [], |r| r.get(0)
            ).optional().map_err(|e| e.to_string())?;
            if found.is_none() {
                return Err("기존 DB 파일이 egui 전용 구조가 아닙니다. 덮어쓰지 않았습니다.".into());
            }
            let signature: Option<String> = conn.query_row(
                "SELECT value FROM preview_meta WHERE key='signature'",
                [], |r| r.get(0)
            ).optional().map_err(|e| e.to_string())?;
            if signature.as_deref() != Some(SIGNATURE) {
                return Err("egui DB 식별자가 일치하지 않습니다. 자동 마이그레이션 중단.".into());
            }
        } else {
            let created = (|| -> Result<(), String> {
                let tx = conn.transaction().map_err(|e| e.to_string())?;
                tx.execute_batch(
                    "CREATE TABLE preview_meta (
                        key TEXT PRIMARY KEY, value TEXT NOT NULL
                    );
                    CREATE TABLE catalog_snapshot (
                        id INTEGER PRIMARY KEY CHECK(id=1),
                        payload TEXT NOT NULL CHECK(json_valid(payload))
                    );
                    CREATE TABLE working_source (
                        id INTEGER PRIMARY KEY CHECK(id=1),
                        raw TEXT NOT NULL,
                        revision INTEGER NOT NULL CHECK(revision >= 0),
                        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                    );"
                ).map_err(|e| e.to_string())?;
                let state: Value = serde_json::from_str(include_str!(
                    "../../src-tauri/resources/state-seed.json"
                )).map_err(|e| format!("내장 상품 데이터 손상: {e}"))?;
                let catalog = json!({
                    "products": state["products"],
                    "aliases": state["aliases"],
                    "brands": state["brands"],
                });
                if !catalog["products"].is_array()
                    || !catalog["aliases"].is_array()
                    || !catalog["brands"].is_array()
                {
                    return Err("내장 상품 데이터 형식 오류".into());
                }
                tx.execute("INSERT INTO preview_meta(key,value) VALUES('signature',?1)",
                    [SIGNATURE]).map_err(|e| e.to_string())?;
                tx.execute("INSERT INTO catalog_snapshot(id,payload) VALUES(1,?1)",
                    [catalog.to_string()]).map_err(|e| e.to_string())?;
                tx.execute("INSERT INTO working_source(id,raw,revision) VALUES(1,'',0)",
                    []).map_err(|e| e.to_string())?;
                tx.commit().map_err(|e| e.to_string())
            })();
            if let Err(error) = created {
                drop(conn);
                // Only clean up the file created by this initialization.
                let _ = fs::remove_file(&store.path);
                return Err(error);
            }
        }

        // Idempotent schema addition, never destructive. Older preview files
        // had a single working_source row. Its contents are copied once to
        // the new independent list and the original row is preserved.
        let migration = conn.transaction().map_err(|e| e.to_string())?;
        migration.execute_batch(
            "CREATE TABLE IF NOT EXISTS working_sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                raw TEXT NOT NULL,
                revision INTEGER NOT NULL CHECK(revision >= 0),
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_working_sources_updated
            ON working_sources(updated_at DESC,id DESC);"
        ).map_err(|e| e.to_string())?;
        let count: i64 = migration.query_row(
            "SELECT COUNT(*) FROM working_sources", [], |r| r.get(0)
        ).map_err(|e| e.to_string())?;
        if count == 0 {
            let legacy: Option<(String, i64)> = migration.query_row(
                "SELECT raw,revision FROM working_source WHERE id=1",
                [], |r| Ok((r.get(0)?, r.get(1)?))
            ).optional().map_err(|e| e.to_string())?;
            if let Some((raw, revision)) = legacy {
                if !raw.trim().is_empty() {
                    migration.execute(
                        "INSERT INTO working_sources(raw,revision) VALUES(?1,?2)",
                        params![raw, revision],
                    ).map_err(|e| e.to_string())?;
                }
            }
        }
        migration.commit().map_err(|e| e.to_string())?;
        Ok(store)
    }

    fn open_connection(&self) -> Result<Connection, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn = Connection::open_with_flags(&self.path, flags)
            .map_err(|e| format!("egui 전용 DB 접근 실패: {e}"))?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
            .map_err(|e| e.to_string())?;
        Ok(conn)
    }

    pub fn load_catalog(&self) -> Result<Value, String> {
        let conn = self.open_connection()?;
        let raw: String = conn.query_row(
            "SELECT payload FROM catalog_snapshot WHERE id=1", [], |r| r.get(0)
        ).map_err(|e| format!("상품 데이터 조회 실패: {e}"))?;
        let catalog: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("상품 데이터 JSON 오류: {e}"))?;
        if !catalog["products"].is_array()
            || !catalog["aliases"].is_array()
            || !catalog["brands"].is_array()
        {
            return Err("상품·별칭·브랜드 데이터 형식 오류".into());
        }
        Ok(catalog)
    }

    pub fn load_sources(&self) -> Result<Vec<WorkingSource>, String> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id,raw,revision,created_at FROM working_sources
             ORDER BY updated_at DESC,id DESC"
        ).map_err(|e| e.to_string())?;
        let mapped = stmt.query_map([], |r| {
            let revision: i64 = r.get(2)?;
            Ok(WorkingSource {
                id: r.get(0)?,
                raw: r.get(1)?,
                revision: revision as u64,
                created_at: r.get(3)?,
            })
        }).map_err(|e| e.to_string())?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| format!("접수 작업본 목록 복원 실패: {e}"))
    }

    pub fn create_source(&self) -> Result<WorkingSource, String> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM working_sources", [], |r| r.get(0)
        ).map_err(|e| e.to_string())?;
        if count >= MAX_WORKING_ORDERS {
            return Err("접수 작업본 한도 1000건을 초과했습니다.".into());
        }
        tx.execute("INSERT INTO working_sources(raw,revision) VALUES('',0)",[])
            .map_err(|e| format!("새 접수 작업본 생성 실패: {e}"))?;
        let id = tx.last_insert_rowid();
        let created: String = tx.query_row(
            "SELECT created_at FROM working_sources WHERE id=?1",
            [id], |r| r.get(0)
        ).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(WorkingSource {
            id, raw: String::new(), revision: 0, created_at: created,
        })
    }

    pub fn save_source(&self, id: i64, raw: &str, revision: u64) -> Result<u64, String> {
        if id <= 0 {
            return Err("접수 작업본 ID가 올바르지 않습니다.".into());
        }
        if raw.len() > SOURCE_LIMIT_BYTES {
            return Err("발주 원문 한 건은 최대 2MB까지 저장할 수 있습니다.".into());
        }
        let expected = i64::try_from(revision)
            .map_err(|_| "접수 revision 범위 초과".to_owned())?;
        let next = expected.checked_add(1).ok_or("접수 revision 한도 초과")?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let affected = tx.execute(
            "UPDATE working_sources SET raw=?1,revision=?2,
             updated_at=CURRENT_TIMESTAMP WHERE id=?3 AND revision=?4",
            params![raw, next, id, expected]
        ).map_err(|e| format!("접수 작업본 저장 실패: {e}"))?;
        if affected != 1 {
            return Err("다른 창에서 접수 작업본이 수정됐거나 삭제됐습니다. 덮어쓰지 않았습니다.".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(next as u64)
    }
}
