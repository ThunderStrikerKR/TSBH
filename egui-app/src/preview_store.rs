//! Strictly isolated local SQLite storage for egui development.
//! This database is NOT a migration of BANDO HUB's Tauri business database.
//! Never open, import or modify the operational app_state/core_* tables.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

const SIGNATURE: &str = "bando-hub-egui-preview-v1";
const SOURCE_LIMIT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct WorkingSource {
    pub raw: String,
    pub revision: u64,
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
                     revision INTEGER NOT NULL CHECK(revision >= 0),
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
                    "INSERT INTO working_source(id,raw,revision) VALUES(1,'',0)",
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
        let (raw, rev): (String, i64) = conn.query_row(
            "SELECT raw,revision FROM working_source WHERE id=1",
            [], |r| Ok((r.get(0)?, r.get(1)?))
        ).map_err(|e| format!("egui 접수 원문을 복원하지 못했습니다: {e}"))?;
        let revision = u64::try_from(rev)
            .map_err(|_| "egui 저장 revision이 잘못되었습니다.".to_owned())?;
        Ok(WorkingSource { raw, revision })
    }

    /// Revision-checked write to prevent two preview windows silently
    /// overwriting each other's source. Does NOT create a confirmed order.
    pub fn save_source(&self, raw: &str, revision: u64) -> Result<u64, String> {
        if raw.len() > SOURCE_LIMIT_BYTES {
            return Err("egui 발주원문은 최대 2MB까지 저장할 수 있습니다.".into());
        }
        let expected = i64::try_from(revision)
            .map_err(|_| "egui 접수 저장 revision 범위를 초과했습니다.".to_owned())?;
        let next = expected.checked_add(1)
            .ok_or("egui 접수 revision 한도 초과")?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let affected = tx.execute(
            "UPDATE working_source SET raw=?1,revision=?2,updated_at=CURRENT_TIMESTAMP
             WHERE id=1 AND revision=?3",
            params![raw, next, expected],
        ).map_err(|e| format!("egui 원문 저장 실패: {e}"))?;
        if affected != 1 {
            return Err("다른 egui 창에서 원문이 변경됐습니다. 저장을 덮어쓰지 않았습니다.".into());
        }
        tx.commit().map_err(|e| format!("egui 원문 저장 확정 실패: {e}"))?;
        Ok(next as u64)
    }

    pub fn path(&self) -> &std::path::Path { &self.path }
}
