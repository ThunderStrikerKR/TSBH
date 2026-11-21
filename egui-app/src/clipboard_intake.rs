//! Native clipboard reception adapter for the isolated egui client.
//! Reads Windows clipboard text or RGBA images, then uses the SAME local
//! PP-OCRv5 Python script/model contract as src-tauri/src/ocr.rs.
//! No business decisions, DB writes or Tauri dependency live here.

use std::{
    fs, path::PathBuf, process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use arboard::Clipboard;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrderType {
    #[default]
    Agency,
    Dealer,
}
impl OrderType {
    pub const fn as_str(self) -> &'static str {
        match self { Self::Agency => "AGENCY", Self::Dealer => "DEALER" }
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "AGENCY" => Ok(Self::Agency),
            "DEALER" => Ok(Self::Dealer),
            _ => Err("저장된 접수 유형이 올바르지 않습니다.".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipboardKind {
    #[default]
    Empty,
    Text,
    Image,
}
impl ClipboardKind {
    pub const fn has_intake(self) -> bool { !matches!(self, Self::Empty) }
}

#[derive(Debug)]
pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum ClipboardPayload {
    Text(String),
    Image(ClipboardImage),
}

pub struct ClipboardMonitor {
    last_sequence: Option<u32>,
    last_probe: Option<Instant>,
    kind: ClipboardKind,
}
impl Default for ClipboardMonitor {
    fn default() -> Self {
        Self { last_sequence: None, last_probe: None, kind: ClipboardKind::Empty }
    }
}
impl ClipboardMonitor {
    pub fn kind(&self) -> ClipboardKind { self.kind }

    /// No clipboard read on every egui frame. On Windows a change sequence
    /// guards costly get_image operations, and the 400ms interval bounds the
    /// fallback cost when the Win32 sequence API cannot be used.
    pub fn refresh(&mut self) {
        // Windows exposes a cheap change counter. Check it BEFORE the
        // 400ms fallback throttle, otherwise clearing the clipboard can
        // leave the Paste actions enabled with a stale cached kind.
        let sequence = clipboard_sequence();
        if sequence.is_some() && sequence == self.last_sequence { return; }
        // Only non-Windows / unavailable-sequence polling needs a throttle.
        if sequence.is_none() && self.last_probe.is_some_and(|time|
            time.elapsed() < Duration::from_millis(400)) {
            return;
        }
        self.last_probe = Some(Instant::now());
        self.last_sequence = sequence;
        self.kind = match read() {
            Ok(ClipboardPayload::Text(_)) => ClipboardKind::Text,
            Ok(ClipboardPayload::Image(_)) => ClipboardKind::Image,
            Err(_) => ClipboardKind::Empty,
        };
    }
}

#[cfg(windows)]
fn clipboard_sequence() -> Option<u32> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    (sequence != 0).then_some(sequence)
}

#[cfg(not(windows))]
fn clipboard_sequence() -> Option<u32> { None }

/// Capture the Windows clipboard change counter along with the bytes we
/// actually imported. If the source changes while reading, do not queue an
/// ambiguous intake. Other platforms can read but cannot automatically
/// clear a clipboard without a reliable ownership/change guard.
pub fn read_with_sequence() -> Result<(ClipboardPayload, Option<u32>), String> {
    let before=clipboard_sequence();
    let payload=read()?;
    let after=clipboard_sequence();
    if before!=after {
        return Err("클립보드 내용이 접수 도중 변경되었습니다. 다시 시도해 주세요.".into());
    }
    Ok((payload,after))
}

/// Clear ONLY the clipboard version that was read for the successful
/// reception. Locking the Windows clipboard makes the comparison and
/// EmptyClipboard atomic with respect to concurrent clipboard writers.
/// A new copy, an inaccessible sequence, or a non-Windows platform is never
/// cleared. The return value means whether this call actually cleared it.
#[cfg(windows)]
pub fn clear_if_unchanged(sequence: Option<u32>) -> Result<bool, String> {
    use std::{ffi::c_void,ptr};
    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(owner: *mut c_void) -> i32;
        fn EmptyClipboard() -> i32;
        fn CloseClipboard() -> i32;
    }
    let Some(expected)=sequence else { return Ok(false); };
    // Opening with no owner is valid when only EmptyClipboard is needed.
    if unsafe { OpenClipboard(ptr::null_mut()) }==0 {
        return Err("접수는 저장됐지만 클립보드가 사용 중이어서 비우지 못했습니다.".into());
    }
    let current=clipboard_sequence();
    let result=if current!=Some(expected){
        Ok(false)
    }else if unsafe { EmptyClipboard() }==0{
        Err("접수는 저장됐지만 클립보드를 비우지 못했습니다.".into())
    }else{
        Ok(true)
    };
    let _=unsafe { CloseClipboard() };
    result
}

#[cfg(not(windows))]
pub fn clear_if_unchanged(_sequence: Option<u32>) -> Result<bool, String> {
    Ok(false)
}

pub fn read() -> Result<ClipboardPayload, String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("클립보드를 열 수 없습니다: {e}"))?;
    if let Ok(text) = clipboard.get_text() {
        if !text.trim().is_empty() {
            return Ok(ClipboardPayload::Text(text));
        }
    }
    if let Ok(image) = clipboard.get_image() {
        let count = image.width.checked_mul(image.height)
            .and_then(|x| x.checked_mul(4)).ok_or("이미지 크기 오류")?;
        if count == 0 || count > 24_000_000 || count != image.bytes.len() {
            return Err("클립보드 이미지가 너무 크거나 형식이 올바르지 않습니다.".into());
        }
        return Ok(ClipboardPayload::Image(ClipboardImage {
            width: image.width, height: image.height,
            rgba: image.bytes.into_owned(),
        }));
    }
    Err("클립보드에 접수할 텍스트 또는 이미지가 없습니다.".into())
}

/// Returns recognized text for employee verification beside the source image.
/// Approval in the caller preserves the original input route as OCR.
/// The image stays in the caller's buffer, never in the DB.
#[cfg(windows)]
pub fn recognize_image(image: &ClipboardImage, cancelled: &AtomicBool) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    if cancelled.load(Ordering::Acquire) { return Err("OCR_CANCELLED".into()); }
    let runtime_dir = if let Some(dir) = std::env::var_os("BANDO_OCR_RUNTIME_DIR") {
        PathBuf::from(dir)
    } else {
        std::env::current_exe()
            .map_err(|e| format!("실행파일 경로 확인 실패: {e}"))?
            .parent().ok_or("실행파일 폴더 확인 실패")?
            .join("ocr")
    };
    let python = runtime_dir.join("runtime").join("python.exe");
    let models = runtime_dir.join("models");
    if !python.is_file() ||
        !models.join("PP-OCRv5_mobile_det").is_dir() ||
        !models.join("korean_PP-OCRv5_mobile_rec").is_dir()
    {
        return Err("OCR 실행 파일 또는 한글 모델이 없습니다. 기존 OCR runtime/models 폴더를 egui 실행파일 옆 ocr 폴더에 두거나 BANDO_OCR_RUNTIME_DIR로 지정해 주세요.".into());
    }
    let bmp = bmp_from_rgba(image)?;
    let temp = TempFolder::create()?;
    let input = temp.path.join("capture.bmp");
    let stdout = temp.path.join("output.txt");
    let stderr = temp.path.join("error.txt");
    fs::write(&input, bmp).map_err(|e| format!("OCR 이미지 저장 실패: {e}"))?;

    let mut command = Command::new(python);
    command.arg("-I").arg("-B").arg("-X").arg("utf8").arg("-c")
        .arg(include_str!("../../src-tauri/resources/ocr/recognize.py"))
        .arg(&input)
        .current_dir(&runtime_dir)
        .env("BANDO_OCR_MODEL_ROOT", &models)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PADDLE_PDX_CACHE_HOME", temp.path.join("paddlex-cache"))
        .env_remove("PYTHONHOME").env_remove("PYTHONPATH")
        .creation_flags(0x08000000);
    command.stdout(Stdio::from(fs::File::create(&stdout).map_err(|e| e.to_string())?));
    command.stderr(Stdio::from(fs::File::create(&stderr).map_err(|e| e.to_string())?));
    let mut child = command.spawn().map_err(|e| format!("PaddleOCR 실행 실패: {e}"))?;
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill(); let _ = child.wait();
            return Err("OCR_CANCELLED".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? { break status; }
        if started.elapsed() > Duration::from_secs(180) {
            let _ = child.kill(); let _ = child.wait();
            return Err("OCR 처리 시간이 초과됐습니다.".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    };
    if cancelled.load(Ordering::Acquire) { return Err("OCR_CANCELLED".into()); }
    let out = String::from_utf8_lossy(&fs::read(&stdout).unwrap_or_default()).into_owned();
    if !status.success() {
        let err = String::from_utf8_lossy(&fs::read(&stderr).unwrap_or_default()).into_owned();
        let detail = err.lines().rev().find_map(|line|
            line.trim().strip_prefix("BANDO_OCR_ERROR=")
        ).or_else(|| err.lines().rev().find(|line| !line.trim().is_empty()))
            .unwrap_or("추가 진단 정보 없음");
        return Err(format!(
            "OCR 실행 실패 (코드 {:?}): {}",
            status.code(), detail.chars().take(1000).collect::<String>()
        ));
    }
    let payload = out.lines().rev().find_map(|line| line.strip_prefix("BANDO_OCR_RESULT="))
        .ok_or("OCR 결과가 반환되지 않았습니다.")?;
    let result: serde_json::Value = serde_json::from_str(payload)
        .map_err(|e| format!("OCR 결과 형식 오류: {e}"))?;
    let text = result.get("text").and_then(serde_json::Value::as_str)
        .unwrap_or("").trim().to_owned();
    if text.is_empty() { return Err("클립보드 이미지에서 문자를 인식하지 못했습니다.".into()); }
    Ok(text)
}

#[cfg(not(windows))]
pub fn recognize_image(_image: &ClipboardImage, _cancelled: &AtomicBool)
    -> Result<String, String>
{
    Err("이미지 OCR 접수는 Windows에서 지원합니다.".into())
}

#[cfg(windows)]
struct TempFolder { path: PathBuf }
#[cfg(windows)]
impl TempFolder {
    fn create() -> Result<Self, String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_nanos();
        let path = std::env::temp_dir().join(
            format!("bando-egui-ocr-{}-{nanos}", std::process::id())
        );
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(Self { path })
    }
}
#[cfg(windows)]
impl Drop for TempFolder {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.path); }
}

/// Same 32bit bottom-up BGRA bitmap format as the proven Tauri OCR runner.
#[cfg(windows)]
fn bmp_from_rgba(image: &ClipboardImage) -> Result<Vec<u8>, String> {
    let pixels = image.width.checked_mul(image.height)
        .and_then(|n| n.checked_mul(4)).ok_or("이미지 크기 오류")?;
    if pixels != image.rgba.len() || pixels > 24_000_000
        || image.width > i32::MAX as usize || image.height > i32::MAX as usize
    { return Err("클립보드 이미지 크기가 잘못되었습니다.".into()); }
    let size = 54usize.checked_add(pixels).ok_or("BMP 크기 오류")?;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(image.width as i32).to_le_bytes());
    out.extend_from_slice(&(image.height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixels as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for y in (0..image.height).rev() {
        for x in 0..image.width {
            let i = (y * image.width + x) * 4;
            out.extend_from_slice(&[image.rgba[i + 2], image.rgba[i + 1], image.rgba[i], 255]);
        }
    }
    Ok(out)
}
