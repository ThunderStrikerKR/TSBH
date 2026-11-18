//! Windows screen region capture with local PP-OCRv5.
//! Captures are never saved in the order database or retained after the request.
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

static OCR_RUNNING: AtomicBool = AtomicBool::new(false);
static OCR_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
const OCR_CANCELLED: &str = "OCR_CANCELLED";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrCapture {
    text: String,
    image_data_url: String,
    width: usize,
    height: usize,
}

struct RunningGuard;
impl Drop for RunningGuard {
    fn drop(&mut self) {
        OCR_RUNNING.store(false, Ordering::Release);
        OCR_CANCEL_REQUESTED.store(false, Ordering::Release);
    }
}

#[tauri::command]
pub async fn capture_ocr(app: tauri::AppHandle) -> Result<OcrCapture, String> {
    if OCR_RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("이미 OCR 캡처를 진행 중입니다.".into());
    }
    OCR_CANCEL_REQUESTED.store(false, Ordering::Release);
    let guard = RunningGuard;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        capture_blocking(app)
    }).await.map_err(|e| format!("OCR 캡처 작업 중단: {e}"))?
}


#[tauri::command]
pub async fn clipboard_ocr(app: tauri::AppHandle) -> Result<OcrCapture, String> {
    if OCR_RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("이미 OCR 인식을 진행 중입니다.".into());
    }
    OCR_CANCEL_REQUESTED.store(false, Ordering::Release);
    let guard = RunningGuard;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        #[cfg(windows)]
        {
            let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
            let image = clipboard.get_image().map_err(|_| "클립보드에 이미지가 없습니다.".to_string())?;
            recognize_rgba(app, image.width, image.height, &image.bytes)
        }
        #[cfg(not(windows))]
        {
            let _ = app;
            Err("클립보드 이미지 OCR은 Windows에서만 지원합니다.".into())
        }
    }).await.map_err(|e| format!("클립보드 OCR 작업 중단: {e}"))?
}

pub async fn recognize_inbox_image(app: tauri::AppHandle, path: std::path::PathBuf)
    -> Result<OcrCapture, String>
{
    if OCR_RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("이미 OCR 인식을 진행 중입니다.".into());
    }
    OCR_CANCEL_REQUESTED.store(false, Ordering::Release);
    let guard = RunningGuard;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        #[cfg(windows)]
        {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            if bytes.is_empty() || bytes.len() > 15_000_000 {
                return Err("발주 이미지 크기가 허용 범위를 벗어났습니다.".into());
            }
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
            let mime = match ext.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "bmp" => "image/bmp",
                _ => return Err("PNG·JPG·BMP 이미지만 인식할 수 있습니다.".into()),
            };
            recognize_image_bytes(app, &bytes, &ext, mime, 0, 0)
        }
        #[cfg(not(windows))]
        {
            let _ = (app, path);
            Err("발주 이미지 OCR은 Windows에서만 지원합니다.".into())
        }
    }).await.map_err(|e| format!("발주 이미지 OCR 작업 중단: {e}"))?
}

/// Cancels both the screen-selection wait and an active PaddleOCR process.
/// No order text is changed by a cancelled request.
#[tauri::command]
pub fn cancel_ocr() {
    if OCR_RUNNING.load(Ordering::Acquire) {
        OCR_CANCEL_REQUESTED.store(true, Ordering::Release);
    }
}

#[cfg(not(windows))]
fn capture_blocking(_app: tauri::AppHandle) -> Result<OcrCapture, String> {
    Err("화면 캡처 OCR은 Windows에서만 사용할 수 있습니다.".into())
}

#[cfg(windows)]
struct CaptureWindowGuard {
    app: tauri::AppHandle,
    was_maximized: bool,
}

#[cfg(windows)]
impl CaptureWindowGuard {
    fn minimize(app: &tauri::AppHandle) -> Result<Self, String> {
        use tauri::Manager;
        let window = app.get_webview_window("main")
            .ok_or("반도허브 메인 창을 찾을 수 없습니다.")?;
        let was_maximized = window.is_maximized().map_err(|e| e.to_string())?;
        window.minimize().map_err(|e| format!("화면 캡처를 위한 최소화 실패: {e}"))?;
        Ok(Self { app: app.clone(), was_maximized })
    }
}

#[cfg(windows)]
impl Drop for CaptureWindowGuard {
    fn drop(&mut self) {
        use tauri::Manager;
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.unminimize();
            if self.was_maximized && !window.is_maximized().unwrap_or(false) {
                let _ = window.maximize();
            }
            let _ = window.set_focus();
        }
    }
}

#[cfg(windows)]
fn capture_blocking(app: tauri::AppHandle) -> Result<OcrCapture, String> {
    use std::time::Duration;

    let (width, height, rgba) = {
        // Scope the window guard to capture only. OCR recognition must start
        // after the exact previous window geometry has been restored.
        let _restore = CaptureWindowGuard::minimize(&app)?;
        std::thread::sleep(Duration::from_millis(250));
        screen_snip::snip()?
    };
    recognize_rgba(app, width, height, &rgba)
}

#[cfg(windows)]
fn recognize_rgba(app: tauri::AppHandle, width: usize, height: usize, rgba: &[u8]) -> Result<OcrCapture, String> {
    if OCR_CANCEL_REQUESTED.load(Ordering::Acquire) {
        return Err(OCR_CANCELLED.into());
    }
    let count = width.checked_mul(height).and_then(|n| n.checked_mul(4))
        .ok_or("캡처 이미지의 크기를 확인할 수 없습니다.")?;
    if width == 0 || height == 0 || count > 24_000_000 || count != rgba.len() {
        return Err("캡처 범위가 너무 크거나 이미지 형식이 잘못됐습니다. 더 작은 범위를 선택해 주세요.".into());
    }
    let bmp = bmp_from_rgba(width, height, rgba)?;
    recognize_image_bytes(app, &bmp, "bmp", "image/bmp", width, height)
}

#[cfg(windows)]
fn recognize_image_bytes(
    app: tauri::AppHandle, bytes: &[u8], ext: &str, mime: &str,
    width: usize, height: usize,
) -> Result<OcrCapture, String> {
    use base64::Engine;
    use tauri::Emitter;
    use std::{fs, io::Write, process::{Command, Stdio}, time::{Duration, Instant}};
    // Locate the private, relocatable OCR runtime next to bando_hub.exe.
    // Never read the employee's system Python, PATH or old AppData venv.
    let executable = std::env::current_exe()
        .map_err(|e| format!("실행파일 위치 확인 실패: {e}"))?;
    let app_dir = executable.parent().ok_or("프로그램 설치 폴더를 찾을 수 없습니다.")?;
    let ocr_dir = app_dir.join("ocr");
    let python = ocr_dir.join("runtime").join("python.exe");
    let models = ocr_dir.join("models");
    if !python.is_file()
        || !models.join("PP-OCRv5_mobile_det").is_dir()
        || !models.join("korean_PP-OCRv5_mobile_rec").is_dir()
    {
        return Err("OCR 실행 파일 또는 한글 모델이 없습니다. 배포 ZIP을 모두 압축 해제하고 bando_hub.exe와 ocr 폴더를 함께 보관하세요.".into());
    }
    let temp = TempWorkspace::create()?;
    let input = temp.path.join(format!("capture.{ext}"));
    let stdout = temp.path.join("output.txt");
    let stderr = temp.path.join("error.txt");
    fs::write(&input, bytes).map_err(|e| format!("OCR 임시 이미지 쓰기 실패: {e}"))?;
    let mut command = Command::new(python);
    command.arg("-I").arg("-B").arg("-X").arg("utf8").arg("-c").arg(include_str!("../resources/ocr/recognize.py")).arg(&input);
    command.current_dir(&ocr_dir)
        .env("BANDO_OCR_MODEL_ROOT", &models)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PADDLE_PDX_CACHE_HOME", temp.path.join("paddlex-cache"))
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH");
    command.stdout(Stdio::from(fs::File::create(&stdout).map_err(|e| e.to_string())?));
    command.stderr(Stdio::from(fs::File::create(&stderr).map_err(|e| e.to_string())?));
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000); // no console window
    if OCR_CANCEL_REQUESTED.load(Ordering::Acquire) {
        return Err(OCR_CANCELLED.into());
    }
    let mut child = command.spawn().map_err(|e| format!("PaddleOCR 실행 실패: {e}"))?;
    // The overlay is gone and recognition has truly started. The React UI
    // must never label the employee's selection step as OCR processing.
    let _ = app.emit("bando-ocr-phase", "recognizing");
    let started = Instant::now();
    let status = loop {
        if OCR_CANCEL_REQUESTED.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OCR_CANCELLED.into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? { break status; }
        if started.elapsed() > Duration::from_secs(180) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("OCR 처리 시간이 초과됐습니다. 한글 모델 설치 여부를 확인하세요.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    // Cancellation can race with a worker exiting normally. The employee's
    // explicit cancel still wins: no review dialog should be opened.
    if OCR_CANCEL_REQUESTED.load(Ordering::Acquire) {
        return Err(OCR_CANCELLED.into());
    }
    let out = String::from_utf8_lossy(&fs::read(&stdout).map_err(|e| e.to_string())?).into_owned();
    // Windows native extensions sometimes crash before Python can write our
    // BANDO_OCR_ERROR marker. Never turn an undecodable stderr into an empty
    // generic message: preserve a bounded, copyable failure diagnosis.
    let err = String::from_utf8_lossy(&fs::read(&stderr).unwrap_or_default()).into_owned();
    if !status.success() {
        return Err(describe_ocr_failure(status.code(), &out, &err));
    }
    let payload = out.lines().rev().find_map(|line| line.strip_prefix("BANDO_OCR_RESULT="))
        .ok_or("OCR 결과를 읽을 수 없습니다.")?;
    let json: serde_json::Value = serde_json::from_str(payload)
        .map_err(|e| format!("OCR 결과 형식 오류: {e}"))?;
    let text = json["text"].as_str().ok_or("OCR 문자열 형식 오류")?.trim().to_owned();
    Ok(OcrCapture {
        text,
        image_data_url: format!("data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)),
        width, height,
    })
}

#[cfg(windows)]
fn describe_ocr_failure(exit_code: Option<i32>, stdout: &str, stderr: &str) -> String {
    let code = exit_code.map(|value| format!("0x{:08X}", value as u32))
        .unwrap_or_else(|| "unknown".to_string());
    let guidance = match exit_code.map(|value| value as u32) {
        Some(0xC0000135) => "필수 Windows DLL을 찾을 수 없습니다.",
        Some(0xC000007B) => "Windows DLL 비트 수 또는 형식이 맞지 않습니다.",
        Some(0xC0000142) => "Windows DLL 초기화에 실패했습니다.",
        Some(0xC000001D) => "현재 PC CPU에서 지원하지 않는 명령을 실행했습니다.",
        Some(0xC0000005) => "OCR 라이브러리의 네이티브 코드가 메모리 접근 오류로 종료됐습니다.",
        _ => "OCR 하위 프로세스가 비정상 종료됐습니다.",
    };
    let marker = stderr.lines().rev()
        .find_map(|line| line.trim().strip_prefix("BANDO_OCR_ERROR="));
    let mut last_lines: Vec<&str> = stderr.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(7)
        .collect();
    last_lines.reverse();
    let detail = marker.map(str::to_owned).or_else(|| {
        let joined = last_lines.join("\n");
        if joined.is_empty() { None } else { Some(joined) }
    }).or_else(|| {
        let last = stdout.lines().rev().find(|line| !line.trim().is_empty())?;
        Some(last.to_string())
    }).unwrap_or_else(|| "추가 오류 로그가 기록되지 않았습니다.".to_owned());
    format!(
        "OCR 인식 실패 (종료 코드 {code}). {guidance}\n상세 원인: {}",
        detail.chars().take(1000).collect::<String>()
    )
}

#[cfg(windows)]
struct TempWorkspace { path: std::path::PathBuf }
#[cfg(windows)]
impl TempWorkspace {
    fn create() -> Result<Self, String> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let path = std::env::temp_dir().join(format!("bando-ocr-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&path).map_err(|e| format!("OCR 임시 작업공간 생성 실패: {e}"))?;
        Ok(Self { path })
    }
}
#[cfg(windows)]
impl Drop for TempWorkspace {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); }
}

// Uncompressed 32-bit BGRA BMP: browser image preview and OpenCV input
// without adding an image encoder dependency to the Windows executable.
#[cfg(windows)]
fn bmp_from_rgba(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let pixels = width.checked_mul(height).and_then(|n| n.checked_mul(4))
        .ok_or("이미지 크기 오류")?;
    if pixels != rgba.len() || width > i32::MAX as usize || height > i32::MAX as usize {
        return Err("이미지 픽셀 크기 오류".into());
    }
    let size = 54usize.checked_add(pixels).ok_or("이미지 파일 크기 오류")?;
    if size > u32::MAX as usize { return Err("이미지 파일이 너무 큽니다.".into()); }
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&(size as u32).to_le_bytes());
    bytes.extend_from_slice(&[0u8; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(width as i32).to_le_bytes());
    bytes.extend_from_slice(&(height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(pixels as u32).to_le_bytes());
    bytes.extend_from_slice(&[0u8; 16]);
    for y in (0..height).rev() {
        for x in 0..width {
            let i = (y * width + x) * 4;
            bytes.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], 255]);
        }
    }
    Ok(bytes)
}

#[cfg(windows)]
mod screen_snip {
    use super::{OCR_CANCELLED, OCR_CANCEL_REQUESTED};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    #[link(name = "user32")]
    unsafe extern "system" {
        fn keybd_event(key: u8, scan: u8, flags: u32, extra: usize);
        fn GetClipboardSequenceNumber() -> u32;
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    const RELEASE: u32 = 2;
    pub(super) fn snip() -> Result<(usize, usize, Vec<u8>), String> {
        let original = unsafe { GetClipboardSequenceNumber() };
        unsafe {
            keybd_event(0x5B, 0, 0, 0);  // Win
            keybd_event(0x10, 0, 0, 0);  // Shift
            keybd_event(0x53, 0, 0, 0);  // S
            keybd_event(0x53, 0, RELEASE, 0);
            keybd_event(0x10, 0, RELEASE, 0);
            keybd_event(0x5B, 0, RELEASE, 0);
        }
        // Never consume a prior clipboard image. The screenshot enters the
        // same native selection overlay as pressing Windows+Shift+S.
        let started = Instant::now();
        // Check the current ESC key state rather than clipboard timeout.
        // The 180ms polling loop must release promptly when Snipping Tool
        // closes on Escape, without displaying an OCR error.
        let mut esc_released = unsafe { (GetAsyncKeyState(0x1B) as u16 & 0x8000) == 0 };
        while started.elapsed() < Duration::from_secs(75) {
            if OCR_CANCEL_REQUESTED.load(Ordering::Acquire) {
                return Err(OCR_CANCELLED.into());
            }
            // The high bit catches held Escape; the low bit catches a quick
            // tap between polls (GetAsyncKeyState's since-last-call flag).
            let escape_state = unsafe { GetAsyncKeyState(0x1B) as u16 };
            let escape_pressed = escape_state & 0x8000 != 0;
            let escape_tapped = escape_state & 0x0001 != 0;
            if !escape_pressed { esc_released = true; }
            if esc_released && (escape_pressed || escape_tapped) {
                return Err(OCR_CANCELLED.into());
            }
            if unsafe { GetClipboardSequenceNumber() } != original {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    if let Ok(image) = clipboard.get_image() {
                        return Ok((image.width, image.height, image.bytes.into_owned()));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(35));
        }
        Err("화면 캡처가 완료되지 않았습니다. 범위를 드래그한 뒤 다시 실행해 주세요.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_request_guard_releases_lock() {
        OCR_RUNNING.store(true, Ordering::Release);
        { let _guard = RunningGuard; }
        assert!(!OCR_RUNNING.load(Ordering::Acquire));
    }
    #[cfg(windows)]
    #[test]
    fn native_failures_keep_exit_code_and_nonempty_diagnostics() {
        let detail = describe_ocr_failure(Some(0xC0000135u32 as i32), "", "");
        assert!(detail.contains("0xC0000135"));
        assert!(detail.contains("DLL"));
        let detail = describe_ocr_failure(Some(1), "", "traceback\nBANDO_OCR_ERROR=model corrupted");
        assert!(detail.contains("model corrupted"));
        assert!(!detail.contains("추가 오류 로그가 기록되지 않았습니다"));
    }
    #[cfg(windows)]
    #[test]
    fn bmp_encoder_has_valid_header_and_bottom_up_bgra_pixels() {
        let out = bmp_from_rgba(1, 1, &[10, 20, 30, 255]).unwrap();
        assert_eq!(&out[..2], b"BM");
        assert_eq!(&out[54..], &[30, 20, 10, 255]);
        assert_eq!(out.len(), 58);
    }
}
