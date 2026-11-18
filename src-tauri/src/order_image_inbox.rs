//! Isolated order-image inbox. Never writes orders before OCR review.
use crate::{canonical_excel_directory, err, get_snapshot, lock_db, Database};
use std::{
    collections::{HashMap, HashSet}, fs,
    path::{Path, PathBuf}, sync::Mutex, time::SystemTime,
};
use tauri::State;

#[derive(Clone, Copy, Eq, PartialEq)]
struct Stamp { len: u64, modified: SystemTime }
struct ImageState { stamp: Stamp, observations: u8, delivered: bool }
#[derive(Default)]
pub(crate) struct OrderImageWatch(pub Mutex<(bool, HashMap<PathBuf, ImageState>)>);

fn inbox_dir(base: &Path, test: bool, settings: &serde_json::Value) -> Result<Option<PathBuf>, String> {
    if test {
        let dir = base.join("Excel").join("발주 이미지 수신");
        fs::create_dir_all(&dir).map_err(err)?;
        return Ok(Some(canonical_excel_directory(&dir)?));
    }
    let Some(path) = settings["orderImageInboxDirectory"].as_str()
        .map(str::trim).filter(|path| !path.is_empty()) else { return Ok(None); };
    Ok(Some(canonical_excel_directory(Path::new(path))?))
}
fn supported(name: &str) -> bool {
    name.len() < 240 && !name.is_empty() && !name.starts_with('.')
        && !name.chars().any(|c| matches!(c, '/' | '\\' | '\0'))
        && ["png", "jpg", "jpeg", "bmp"].iter().any(|ext|
            name.to_ascii_lowercase().ends_with(&format!(".{ext}")))
}
#[tauri::command]
pub(crate) async fn choose_order_image_inbox_directory(
    app: tauri::AppHandle, db: State<'_, Database>,
) -> Result<Option<String>, String> {
    if db.is_test {
        return Ok(inbox_dir(&db.base, true, &serde_json::Value::Null)?
            .map(|p| p.to_string_lossy().into_owned()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let chosen = app.dialog().file()
            .set_title("발주 이미지 자동 수신 폴더 선택").blocking_pick_folder();
        let Some(chosen) = chosen else { return Ok(None); };
        let path = chosen.into_path().map_err(err)?;
        Ok(Some(canonical_excel_directory(&path)?.to_string_lossy().into_owned()))
    }).await.map_err(err)?
}
#[tauri::command]
pub(crate) fn open_order_image_inbox_directory(db: State<'_, Database>) -> Result<(), String> {
    let current = get_snapshot(&*lock_db(&db)?)?;
    let directory = inbox_dir(&db.base, db.is_test, &current["settings"])?
        .ok_or("발주 이미지 수신 폴더가 지정되지 않았습니다.")?;
    open::that(directory).map_err(err)
}

#[tauri::command]
pub(crate) fn order_image_watch_scan(
    db: State<'_, Database>, watch: State<'_, OrderImageWatch>,
) -> Result<Vec<String>, String> {
    let current = get_snapshot(&*lock_db(&db)?)?;
    let Some(dir) = inbox_dir(&db.base, db.is_test, &current["settings"])? else {
        return Ok(Vec::new());
    };
    let mut rows = fs::read_dir(&dir).map_err(err)?
        .collect::<Result<Vec<_>,_>>().map_err(err)?;
    rows.sort_by_key(|entry| entry.file_name());
    let mut state = watch.0.lock().map_err(|_| "발주 이미지 감시 잠금 오류")?;
    let first_scan = !state.0;
    state.0 = true;
    let seen = &mut state.1;
    let mut active = HashSet::new();
    let mut ready = Vec::new();
    for entry in rows {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !supported(&name) || !entry.file_type().map_err(err)?.is_file() { continue; }
        let path = entry.path();
        let metadata = entry.metadata().map_err(err)?;
        if metadata.len() == 0 || metadata.len() > 15_000_000 { continue; }
        let stamp = Stamp { len: metadata.len(), modified: metadata.modified().map_err(err)? };
        active.insert(path.clone());
        let record = seen.entry(path).or_insert(ImageState {
            stamp, observations: 0, delivered: first_scan,
        });
        if record.stamp != stamp {
            *record = ImageState { stamp, observations: 0, delivered: false };
        }
        record.observations = record.observations.saturating_add(1);
        if record.observations >= 3 && !record.delivered && ready.is_empty() {
            ready.push(name);
            record.delivered = true;
        }
    }
    seen.retain(|path, _| active.contains(path));
    Ok(ready)
}
#[tauri::command]
pub(crate) async fn order_image_ocr(
    app: tauri::AppHandle, db: State<'_, Database>, filename: String,
) -> Result<crate::ocr::OcrCapture, String> {
    if !supported(&filename) { return Err("허용되지 않는 발주 이미지 파일명입니다.".into()); }
    let current = get_snapshot(&*lock_db(&db)?)?;
    let dir = inbox_dir(&db.base, db.is_test, &current["settings"])?
        .ok_or("발주 이미지 수신 폴더를 먼저 지정하세요.")?;
    let path = fs::canonicalize(dir.join(&filename)).map_err(err)?;
    if path.parent() != Some(dir.as_path()) || !path.is_file() {
        return Err("발주 이미지가 지정한 폴더에 없습니다.".into());
    }
    let len = fs::metadata(&path).map_err(err)?.len();
    if len == 0 || len > 15_000_000 { return Err("발주 이미지가 너무 큽니다.".into()); }
    crate::ocr::recognize_inbox_image(app, path).await
}
