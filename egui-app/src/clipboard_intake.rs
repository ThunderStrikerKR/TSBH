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

/// A bounded snapshot sent to the Reception UI before OCR finishes.
/// Only pixels are copied; the original full-resolution image continues to
/// PaddleOCR and is retained for employee review unchanged.
pub fn preview_thumbnail(image:&ClipboardImage,max_edge:usize)->ClipboardImage{
    let expected=image.width.saturating_mul(image.height).saturating_mul(4);
    if image.width==0 || image.height==0 || image.rgba.len()!=expected{
        return ClipboardImage{
            width:1,height:1,rgba:vec![0,0,0,255]
        };
    }
    let edge=max_edge.max(1);
    let longest=image.width.max(image.height);
    let (width,height)=if longest<=edge{
        (image.width,image.height)
    }else{
        let ratio=edge as f64/longest as f64;
        (
            ((image.width as f64*ratio).round() as usize).max(1),
            ((image.height as f64*ratio).round() as usize).max(1)
        )
    };
    let mut rgba=vec![0u8;width*height*4];
    for y in 0..height{
        let src_y=y*image.height/height;
        for x in 0..width{
            let src_x=x*image.width/width;
            let src=(src_y*image.width+src_x)*4;
            let dst=(y*width+x)*4;
            rgba[dst..dst+4].copy_from_slice(&image.rgba[src..src+4]);
        }
    }
    ClipboardImage{width,height,rgba}
}

#[derive(Debug)]
pub enum ClipboardPayload {
    Text(String),
    Image(ClipboardImage),
}

#[derive(Default)]
pub struct ClipboardMonitor {
    #[cfg(windows)]
    watcher:Option<clipboard_events::Watcher>,
    #[cfg(windows)]
    cleared_sequence:Option<u32>,
    #[cfg(not(windows))]
    last_probe:Option<Instant>,
    kind:ClipboardKind,
}
impl ClipboardMonitor {
    pub fn kind(&self)->ClipboardKind{
        #[cfg(windows)]
        if let Some(watcher)=self.watcher.as_ref(){
            if let Some((sequence,kind))=watcher.snapshot(){
                if self.cleared_sequence.is_some() && sequence==self.cleared_sequence{
                    return ClipboardKind::Empty;
                }
                return kind;
            }
        }
        self.kind
    }
    /// Windows events wake egui directly. Only a failed worker launch or
    /// a non-Windows adapter uses the conservative fallback.
    pub fn refresh(&mut self,ctx:&eframe::egui::Context){
        #[cfg(windows)] {
            if self.watcher.is_none(){
                self.watcher=clipboard_events::Watcher::start(ctx.clone()).ok();
                if self.watcher.is_none(){
                    ctx.request_repaint_after(Duration::from_secs(1));
                }
            }
        }
        #[cfg(not(windows))] {
            if self.last_probe.is_none_or(|t|t.elapsed()>=Duration::from_millis(400)){
                self.last_probe=Some(Instant::now());
                self.kind=match read(){
                    Ok(ClipboardPayload::Text(_))=>ClipboardKind::Text,
                    Ok(ClipboardPayload::Image(_))=>ClipboardKind::Image,
                    Err(_)=>ClipboardKind::Empty,
                };
            }
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }
    /// Autosave cleared exactly this imported version under the clipboard
    /// lock. Suppress a stale worker snapshot until the next publication.
    pub fn mark_cleared(&mut self,sequence:Option<u32>){
        self.kind=ClipboardKind::Empty;
        #[cfg(windows)] {
            self.cleared_sequence=sequence;
            if let Some(watcher)=self.watcher.as_ref(){watcher.invalidate_after_clear(sequence);}
        }
        #[cfg(not(windows))] {let _=sequence;}
    }
    pub fn shutdown(&mut self){
        #[cfg(windows)] {self.watcher.take();}
    }
}

fn classify_available(text_nonempty:bool,image_available:bool)->ClipboardKind{
    if text_nonempty{ClipboardKind::Text}
    else if image_available{ClipboardKind::Image}
    else{ClipboardKind::Empty}
}

#[cfg(windows)]
mod clipboard_events{
    use super::{clipboard_sequence,classify_available,ClipboardKind,Clipboard};
    use std::{cell::Cell,ptr,sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU32,Ordering}},
        time::{Duration,Instant}};
    type Hwnd=isize;
    type Handle=isize;
    type Snapshot=(Option<u32>,ClipboardKind);
    const WM_CLIPBOARDUPDATE:u32=0x031D;
    const WM_QUIT:u32=0x0012;
    const HWND_MESSAGE:Hwnd=-3;
    const PM_REMOVE:u32=1;
    const QS_ALLINPUT:u32=0x04FF;
    const MWMO_INPUTAVAILABLE:u32=4;
    #[repr(C)]
    #[derive(Default)]
    struct Msg{hwnd:Hwnd,message:u32,wparam:usize,lparam:isize,time:u32,
        x:i32,y:i32,private:u32}
    #[repr(C)]
    struct WindowClass{
        style:u32,procedure:Option<unsafe extern "system" fn(Hwnd,u32,usize,isize)->isize>,
        class_extra:i32,window_extra:i32,instance:Handle,icon:Handle,cursor:Handle,
        background:Handle,menu:*const u16,name:*const u16,
    }
    #[link(name="user32")]
    unsafe extern "system"{
        fn RegisterClassW(class:*const WindowClass)->u16;
        fn UnregisterClassW(name:*const u16,instance:Handle)->i32;
        fn CreateWindowExW(ex_style:u32,class:*const u16,name:*const u16,style:u32,
            x:i32,y:i32,w:i32,h:i32,parent:Hwnd,menu:Handle,instance:Handle,param:isize)->Hwnd;
        fn DestroyWindow(hwnd:Hwnd)->i32;
        fn DefWindowProcW(hwnd:Hwnd,msg:u32,wparam:usize,lparam:isize)->isize;
        fn AddClipboardFormatListener(hwnd:Hwnd)->i32;
        fn RemoveClipboardFormatListener(hwnd:Hwnd)->i32;
        fn IsClipboardFormatAvailable(format:u32)->i32;
        fn RegisterClipboardFormatW(name:*const u16)->u32;
        fn PeekMessageW(msg:*mut Msg,hwnd:Hwnd,min:u32,max:u32,remove:u32)->i32;
        fn TranslateMessage(msg:*const Msg)->i32;
        fn DispatchMessageW(msg:*const Msg)->isize;
        fn PostThreadMessageW(thread:u32,msg:u32,wparam:usize,lparam:isize)->i32;
        fn MsgWaitForMultipleObjectsEx(count:u32,handles:*const Handle,ms:u32,mask:u32,flags:u32)->u32;
    }
    #[link(name="kernel32")]
    unsafe extern "system"{
        fn GetModuleHandleW(name:*const u16)->Handle;
        fn GetCurrentThreadId()->u32;
    }
    thread_local!{static DIRTY:Cell<bool>=const{Cell::new(false)};}
    unsafe extern "system" fn window_proc(hwnd:Hwnd,msg:u32,wparam:usize,lparam:isize)->isize{
        if msg==WM_CLIPBOARDUPDATE{DIRTY.with(|d|d.set(true));return 0;}
        unsafe{DefWindowProcW(hwnd,msg,wparam,lparam)}
    }
    struct Listener{hwnd:Hwnd,instance:Handle,class:Vec<u16>,registered:bool}
    impl Listener{
        fn new()->Self{
            let class:Vec<u16>="BandoHubClipboardMonitor".encode_utf16().chain(Some(0)).collect();
            let instance=unsafe{GetModuleHandleW(ptr::null())};
            let definition=WindowClass{style:0,procedure:Some(window_proc),class_extra:0,
                window_extra:0,instance,icon:0,cursor:0,background:0,menu:ptr::null(),name:class.as_ptr()};
            let registered=instance!=0 && unsafe{RegisterClassW(&definition)}!=0;
            let hwnd=if registered{unsafe{CreateWindowExW(0,class.as_ptr(),class.as_ptr(),0,
                0,0,0,0,HWND_MESSAGE,0,instance,0)}}else{0};
            Self{hwnd,instance,class,registered}
        }
    }
    impl Drop for Listener{
        fn drop(&mut self){
            if self.hwnd!=0{unsafe{RemoveClipboardFormatListener(self.hwnd);DestroyWindow(self.hwnd);}}
            if self.registered{unsafe{UnregisterClassW(self.class.as_ptr(),self.instance);}}
        }
    }
    struct Shared{
        stop:AtomicBool,thread:AtomicU32,snapshot:Mutex<Option<Snapshot>>,
    }
    pub(super) struct Watcher{shared:Arc<Shared>}
    impl Watcher{
        pub fn start(ctx:eframe::egui::Context)->std::io::Result<Self>{
            let shared=Arc::new(Shared{stop:AtomicBool::new(false),thread:AtomicU32::new(0),
                snapshot:Mutex::new(None)});
            let worker=Arc::clone(&shared);
            std::thread::Builder::new().name("bando-clipboard-events".into())
                .spawn(move||run(worker,ctx))?;
            Ok(Self{shared})
        }
        pub fn snapshot(&self)->Option<Snapshot>{
            *self.shared.snapshot.lock().unwrap_or_else(|p|p.into_inner())
        }
        pub fn invalidate_after_clear(&self,imported:Option<u32>){
            let mut snapshot=self.shared.snapshot.lock().unwrap_or_else(|p|p.into_inner());
            let live=clipboard_sequence();
            if live.is_none() || snapshot.is_some_and(|s|s.0==imported || s.0!=live){
                *snapshot=None;
            }
        }
    }
    impl Drop for Watcher{
        fn drop(&mut self){
            self.shared.stop.store(true,Ordering::Release);
            let thread=self.shared.thread.load(Ordering::Acquire);
            if thread!=0{unsafe{PostThreadMessageW(thread,WM_QUIT,0,0);}}
        }
    }
    /// Availability metadata only for images. Text is inspected once on
    /// change to preserve the original non-whitespace/text-first policy.
    /// No image decoding or OCR occurs in the monitor.
    fn probe(png:u32)->Option<Snapshot>{
        let before=clipboard_sequence();
        let available=|format:u32|format!=0 && unsafe{IsClipboardFormatAvailable(format)}!=0;
        let image=available(8)||available(17)||available(2)||available(png);
        let text=if available(13)||available(1)||available(7){
            let mut clipboard=Clipboard::new().ok()?;
            match clipboard.get_text(){
                Ok(text)=>!text.trim().is_empty(),
                Err(arboard::Error::ContentNotAvailable)=>false,
                Err(_)=>return None,
            }
        }else{false};
        let after=clipboard_sequence();
        if before!=after{return None;}
        Some((after,classify_available(text,image)))
    }
    fn run(shared:Arc<Shared>,ctx:eframe::egui::Context){
        let mut msg=Msg::default();
        unsafe{PeekMessageW(&mut msg,0,0,0,0);}
        shared.thread.store(unsafe{GetCurrentThreadId()},Ordering::Release);
        let listener=Listener::new();
        let hooked=listener.hwnd!=0 && unsafe{AddClipboardFormatListener(listener.hwnd)}!=0;
        if !hooked{eprintln!("[BANDO HUB] 클립보드 이벤트 등록 실패: 백그라운드 변경 확인 유지");}
        let png_name:Vec<u16>="PNG".encode_utf16().chain(Some(0)).collect();
        let png=unsafe{RegisterClipboardFormatW(png_name.as_ptr())};
        let fallback=if hooked{Duration::from_secs(5)}else{Duration::from_millis(400)};
        let mut last=Instant::now()-fallback;
        let mut dirty=true;
        let mut retry=false;
        let mut retry_delay=Duration::from_millis(150);
        while !shared.stop.load(Ordering::Acquire){
            for _ in 0..256{
                if unsafe{PeekMessageW(&mut msg,0,0,0,PM_REMOVE)}==0{break;}
                if msg.message==WM_QUIT{shared.stop.store(true,Ordering::Release);break;}
                unsafe{TranslateMessage(&msg);DispatchMessageW(&msg);}
            }
            if shared.stop.load(Ordering::Acquire){break;}
            dirty|=DIRTY.with(|d|d.replace(false));
            if dirty{retry_delay=Duration::from_millis(150);}
            if dirty || last.elapsed()>=fallback ||
                (retry && last.elapsed()>=retry_delay){
                let previous=*shared.snapshot.lock().unwrap_or_else(|p|p.into_inner());
                let current=clipboard_sequence();
                if dirty || retry || current.is_none() || previous.is_none_or(|p|p.0!=current){
                    match probe(png){
                        Some(next)=>{
                            let mut snapshot=shared.snapshot.lock().unwrap_or_else(|p|p.into_inner());
                            // Do not publish a probe superseded by autosave
                            // clearing or by another application copying.
                            if clipboard_sequence()==next.0{
                                let changed=*snapshot!=Some(next);
                                *snapshot=Some(next);drop(snapshot);
                                if changed{ctx.request_repaint();}
                                retry=false;
                                retry_delay=Duration::from_millis(150);
                            }else{retry=true;}
                        }
                        None=>{
                            retry=true;
                            retry_delay=(retry_delay*2).min(Duration::from_secs(2));
                        }
                    }
                }
                last=Instant::now();dirty=false;
            }
            let delay=if retry{retry_delay.min(fallback)}else{fallback};
            let ms=delay.saturating_sub(last.elapsed()).as_millis().max(1) as u32;
            let result=unsafe{MsgWaitForMultipleObjectsEx(0,ptr::null(),ms,QS_ALLINPUT,MWMO_INPUTAVAILABLE)};
            if result==u32::MAX{std::thread::sleep(Duration::from_millis(150));}
        }
        drop(listener);
        shared.thread.store(0,Ordering::Release);
    }
}

#[cfg(test)]
mod clipboard_monitor_tests{
    use super::*;
    #[test]
    fn clipboard_format_classification_prefers_nonempty_text(){
        assert_eq!(classify_available(true,true),ClipboardKind::Text);
        assert_eq!(classify_available(true,false),ClipboardKind::Text);
        assert_eq!(classify_available(false,true),ClipboardKind::Image);
        assert_eq!(classify_available(false,false),ClipboardKind::Empty);
    }
    #[test]
    fn successful_clear_invalidates_cached_intake(){
        let mut monitor=ClipboardMonitor::default();
        monitor.kind=ClipboardKind::Image;
        monitor.mark_cleared(Some(42));
        assert_eq!(monitor.kind(),ClipboardKind::Empty);
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

/// Start Windows' native region selector (Win+Shift+S), and accept ONLY
/// an image produced after this request. Unlike clipboard paste, this
/// operation is available even when the clipboard starts completely empty.
/// Escape, the explicit OCR cancel button, and a 75-second timeout leave the
/// order untouched. The caller minimizes/restores the HUB around selection.
#[cfg(windows)]
pub fn capture_screen_region(
    cancelled:&AtomicBool
)->Result<(ClipboardImage,Option<u32>),String>{
    #[link(name="user32")]
    unsafe extern "system" {
        fn keybd_event(key:u8,scan:u8,flags:u32,extra:usize);
        fn GetAsyncKeyState(key:i32)->i16;
    }
    const RELEASE:u32=0x0002;
    const VK_WIN:u8=0x5B;
    const VK_SHIFT:u8=0x10;
    const VK_S:u8=0x53;
    const VK_ESCAPE:i32=0x1B;
    let original=clipboard_sequence()
        .ok_or("Windows 클립보드 변경 상태를 확인할 수 없습니다.")?;
    if cancelled.load(Ordering::Acquire){return Err("OCR_CANCELLED".into());}
    unsafe {
        keybd_event(VK_WIN,0,0,0);
        keybd_event(VK_SHIFT,0,0,0);
        keybd_event(VK_S,0,0,0);
        keybd_event(VK_S,0,RELEASE,0);
        keybd_event(VK_SHIFT,0,RELEASE,0);
        keybd_event(VK_WIN,0,RELEASE,0);
    }
    let started=Instant::now();
    let mut esc_released=unsafe{
        (GetAsyncKeyState(VK_ESCAPE) as u16&0x8000)==0
    };
    while started.elapsed()<Duration::from_secs(75){
        if cancelled.load(Ordering::Acquire){
            return Err("OCR_CANCELLED".into());
        }
        let state=unsafe{GetAsyncKeyState(VK_ESCAPE) as u16};
        let down=state&0x8000!=0;
        if !down{esc_released=true;}
        if esc_released && (down || state&1!=0){
            return Err("OCR_CANCELLED".into());
        }
        if clipboard_sequence()!=Some(original){
            match read_with_sequence(){
                Ok((ClipboardPayload::Image(image),sequence))=>{
                    return Ok((image,sequence));
                }
                Ok((ClipboardPayload::Text(_),_))=>{
                    return Err("캡처 도중 다른 텍스트가 복사되었습니다. 다시 시도해 주세요.".into());
                }
                Err(_)=>{ /* Native snip can publish the image asynchronously. */ }
            }
        }
        std::thread::sleep(Duration::from_millis(35));
    }
    Err("화면 캡처가 완료되지 않았습니다. 범위를 선택하거나 Esc로 취소해 주세요.".into())
}

#[cfg(not(windows))]
pub fn capture_screen_region(
    _cancelled:&AtomicBool
)->Result<(ClipboardImage,Option<u32>),String>{
    Err("캡처 범위 지정은 Windows에서만 지원합니다.".into())
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

#[cfg(test)]
mod ocr_processing_preview_tests{
    use super::*;
    #[test]
    fn thumbnail_preserves_aspect_and_bounds(){
        let input=ClipboardImage{
            width:100,height:200,rgba:vec![255;100*200*4]
        };
        let preview=preview_thumbnail(&input,64);
        assert_eq!((preview.width,preview.height),(32,64));
        assert_eq!(preview.rgba.len(),32*64*4);
        assert_eq!(input.width,100);
        assert_eq!(input.height,200);
    }
    #[test]
    fn small_images_are_not_upscaled(){
        let input=ClipboardImage{
            width:2,height:1,rgba:vec![255;8]
        };
        let preview=preview_thumbnail(&input,640);
        assert_eq!((preview.width,preview.height),(2,1));
    }
}
