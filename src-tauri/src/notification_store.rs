//! Independent SQLite notification inbox.
//!
//! Do not place notification read/clear state inside the business snapshot:
//! doing so would consume a business revision while staff edit orders.
//! A separate table in the same production/test database survives restarts
//! and is included in the existing SQLite backup, without changing orders.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::State;
use super::{lock_db, Database};

const LIMIT: i64 = 100;

fn db_error(error: impl std::fmt::Display) -> String { error.to_string() }

pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS hub_notifications(
            id TEXT PRIMARY KEY NOT NULL,
            at TEXT NOT NULL,
            read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
            payload TEXT NOT NULL
        );",
    ).map_err(db_error)?;
    Ok(())
}

fn load(conn: &Connection) -> Result<Vec<Value>, String> {
    let mut query = conn.prepare(
        "SELECT payload, read FROM hub_notifications ORDER BY rowid DESC LIMIT ?1"
    ).map_err(db_error)?;
    let rows = query.query_map(params![LIMIT], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }).map_err(db_error)?;
    let mut notices = Vec::new();
    for row in rows {
        let (text, read) = row.map_err(db_error)?;
        let mut entry: Value = serde_json::from_str(&text).map_err(db_error)?;
        entry["read"] = Value::Bool(read != 0);
        notices.push(entry);
    }
    Ok(notices)
}

fn validate(notice: &Value) -> Result<(&str, &str), String> {
    let obj = notice.as_object().ok_or("알림 데이터 형식 오류")?;
    let field = |name: &str| obj.get(name).and_then(Value::as_str).unwrap_or("");
    let id = field("id");
    let at = field("at");
    if id.is_empty() || id.len() > 128 || at.is_empty() || at.len() > 64 {
        return Err("알림 ID 또는 수신시각 오류".into());
    }
    if !["시스템", "공장 회신"].contains(&field("source")) {
        return Err("알림 출처 오류".into());
    }
    if !["info", "success", "error"].contains(&field("tone")) {
        return Err("알림 유형 오류".into());
    }
    let title = field("title");
    let detail = field("detail");
    if title.trim().is_empty() || title.len() > 2048 || detail.len() > 80_000 {
        return Err("알림 내용 오류".into());
    }
    if let Some(issues) = obj.get("issues") {
        if !issues.is_null() && !issues.as_array().is_some_and(|v| v.len() <= 1000) {
            return Err("알림에 표시할 오류 행이 너무 많습니다.".into());
        }
    }
    Ok((id, at))
}

fn insert(conn: &mut Connection, mut notice: Value) -> Result<Vec<Value>, String> {
    let (id, at) = validate(&notice)?;
    let (id, at) = (id.to_string(), at.to_string());
    notice["read"] = Value::Bool(false);
    let tx = conn.transaction().map_err(db_error)?;

    // Repeated watcher scans of the exact same error must not spam the inbox.
    let recent: Option<String> = tx.query_row(
        "SELECT payload FROM hub_notifications ORDER BY rowid DESC LIMIT 1",
        [], |row| row.get(0),
    ).optional().map_err(db_error)?;
    if let Some(recent) = recent {
        let prior: Value = serde_json::from_str(&recent).map_err(db_error)?;
        if prior["title"] == notice["title"]
            && prior["detail"] == notice["detail"]
            && prior["filename"] == notice["filename"] {
            tx.commit().map_err(db_error)?;
            return load(conn);
        }
    }

    let body = serde_json::to_string(&notice).map_err(db_error)?;
    tx.execute(
        "INSERT OR IGNORE INTO hub_notifications(id,at,read,payload)
         VALUES (?1,?2,0,?3)",
        params![id, at, body],
    ).map_err(db_error)?;
    tx.execute(
        "DELETE FROM hub_notifications WHERE id NOT IN
        (SELECT id FROM hub_notifications ORDER BY rowid DESC LIMIT ?1)",
        params![LIMIT],
    ).map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    load(conn)
}

fn mark_read(conn: &Connection, id: &str) -> Result<Vec<Value>, String> {
    conn.execute(
        "UPDATE hub_notifications SET read=1 WHERE id=?1",
        params![id],
    ).map_err(db_error)?;
    load(conn)
}

fn clear(conn: &Connection) -> Result<Vec<Value>, String> {
    conn.execute("DELETE FROM hub_notifications", []).map_err(db_error)?;
    load(conn)
}

#[tauri::command]
pub(super) fn hub_notifications_list(db: State<'_, Database>) -> Result<Vec<Value>, String> {
    load(&lock_db(&db)?)
}

#[tauri::command]
pub(super) fn hub_notifications_add(
    db: State<'_, Database>, notice: Value,
) -> Result<Vec<Value>, String> {
    insert(&mut lock_db(&db)?, notice)
}

#[tauri::command]
pub(super) fn hub_notifications_mark_read(
    db: State<'_, Database>, id: String,
) -> Result<Vec<Value>, String> {
    mark_read(&lock_db(&db)?, &id)
}

#[tauri::command]
pub(super) fn hub_notifications_clear(db: State<'_, Database>) -> Result<Vec<Value>, String> {
    clear(&lock_db(&db)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        conn
    }
    fn notice(id: usize) -> Value {
        json!({
            "id":format!("n{id}"),"at":"2026-11-16T12:00:00Z",
            "tone":"error","source":"공장 회신",
            "title":format!("송장번호 확인 {id}"),
            "detail":format!("Q열 {id}행 수치를 확인하세요"),
            "filename":"factory.xlsx",
            "issues":[{"row":id,"name":"테스트","issue":"정밀도 확인"}],
            "read":false
        })
    }

    #[test]
    fn existing_sqlite_upgrades_without_writing_business_snapshot() {
        let folder = std::env::temp_dir()
            .join(format!("bando-notification-test-{}-{}.sqlite", std::process::id(),
                std::thread::current().name().unwrap_or("upgrade")));
        let _ = std::fs::remove_file(&folder);
        let mut conn = Connection::open(&folder).unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            INSERT INTO app_state(id,payload) VALUES (1,'{\"revision\":19}');").unwrap();
        initialize(&conn).unwrap();
        insert(&mut conn,notice(1)).unwrap();
        mark_read(&conn, "n1").unwrap();
        drop(conn);
        let conn = Connection::open(&folder).unwrap();
        initialize(&conn).unwrap(); // idempotent on every restart
        let restored = load(&conn).unwrap();
        assert_eq!(restored.len(),1);
        assert_eq!(restored[0]["read"],true);
        assert_eq!(restored[0]["issues"][0]["row"],1);
        let state: String = conn.query_row(
            "SELECT payload FROM app_state WHERE id=1",[],|row|row.get(0)).unwrap();
        assert!(state.contains("\"revision\":19"));
        drop(conn);
        let _ = std::fs::remove_file(folder);
    }

    #[test]
    fn mark_and_clear_survive_and_do_not_duplicate() {
        let mut db=memory();
        assert_eq!(insert(&mut db,notice(1)).unwrap().len(),1);
        assert_eq!(insert(&mut db,notice(1)).unwrap().len(),1);
        insert(&mut db,notice(2)).unwrap();
        assert_eq!(mark_read(&db,"n1").unwrap()[1]["read"],true);
        assert_eq!(mark_read(&db,"unknown").unwrap().len(),2);
        assert!(clear(&db).unwrap().is_empty());
        assert!(load(&db).unwrap().is_empty());
    }

    #[test]
    fn bounded_history_retains_unread_without_corrupting_order() {
        let mut db=memory();
        for i in 0..105 { insert(&mut db,notice(i)).unwrap(); }
        let items=load(&db).unwrap();
        assert_eq!(items.len(),100);
        assert_eq!(items[0]["id"],"n104");
        assert_eq!(items[99]["id"],"n5");
        assert!(items.iter().all(|n|n["read"]==false));
    }

    #[test]
    fn invalid_notices_never_modify_persistent_inbox() {
        let mut db=memory();
        insert(&mut db,notice(0)).unwrap();
        let mut bad=notice(1);
        bad["source"]=json!("untrusted");
        assert!(insert(&mut db,bad).is_err());
        assert_eq!(load(&db).unwrap().len(),1);
    }
}
