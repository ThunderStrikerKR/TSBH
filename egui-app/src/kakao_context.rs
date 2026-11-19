//! Windows-only KakaoTalk **window metadata** adapter for the egui intake.
//! Reuses the same verification criteria as Tauri's kakao_dock.rs:
//! process executable name KakaoTalk.exe, visible top-level windows,
//! system-menu style, plausible dimensions, and genuine Win32 handles.
//! No window movement, reparenting, message spying, clipboard reads or hooks.

use std::time::{Duration, Instant};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatWindow {
    pub handle: String,
    pub title: String,
}

#[derive(Default)]
pub struct KakaoTracker {
    chats: Vec<ChatWindow>,
    last_probe: Option<Instant>,
    last_focused: Option<(String, Instant)>,
}

impl KakaoTracker {
    /// Native window metadata is bounded to one scan per 400ms. A remembered
    /// HWND is never accepted unless it still resolves to a verified chat.
    pub fn refresh(&mut self) {
        if self.last_probe.is_some_and(|last| last.elapsed() < Duration::from_millis(400)) {
            return;
        }
        self.last_probe = Some(Instant::now());
        let (chats, foreground) = native::probe();
        self.chats = chats;
        if let Some(handle) = foreground {
            if self.chats.iter().any(|chat| chat.handle == handle) {
                self.last_focused = Some((handle, Instant::now()));
            }
        }
        if self.last_focused.as_ref().is_some_and(|(handle, _)| {
            !self.chats.iter().any(|chat| chat.handle == *handle)
        }) {
            self.last_focused = None;
        }
    }

    pub fn chats(&self) -> &[ChatWindow] { &self.chats }

    pub fn verified(&self, handle: &str) -> Option<&ChatWindow> {
        self.chats.iter().find(|chat| chat.handle == handle)
    }

    /// A recent verified Kakao foreground window is stronger than an old
    /// manually chosen one. Never infer "active" from mere window enumeration.
    pub fn recent_focus(&self) -> Option<&str> {
        self.last_focused.as_ref()
            .filter(|(_, seen)| seen.elapsed() <= Duration::from_secs(12))
            .map(|(handle, _)| handle.as_str())
    }

    pub fn context(&self, requested: Option<&str>) -> (Value, Option<String>) {
        let Some(handle) = requested else {
            return (json!([]), None);
        };
        if self.verified(handle).is_none() {
            return (json!([]), None);
        }
        let windows: Vec<Value> = self.chats.iter().map(|chat| json!({
            "kind": "CHAT",
            "handle": chat.handle,
            "title": chat.title,
        })).collect();
        (json!(windows), Some(handle.to_owned()))
    }
}

#[cfg(windows)]
mod native {
    use super::ChatWindow;
    use std::path::Path;

    type Hwnd = isize;
    type Handle = isize;
    #[repr(C)]
    #[derive(Default)]
    struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
    const WS_SYSMENU: isize = 0x0008_0000;
    const GWL_STYLE: i32 = -16;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(callback: Option<unsafe extern "system" fn(Hwnd, isize)->i32>, data: isize)->i32;
        fn IsWindow(hwnd: Hwnd)->i32;
        fn IsWindowVisible(hwnd: Hwnd)->i32;
        fn IsIconic(hwnd: Hwnd)->i32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect)->i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32)->u32;
        fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32)->i32;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32)->isize;
        fn GetForegroundWindow()->Hwnd;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32)->Handle;
        fn QueryFullProcessImageNameW(process: Handle, flags:u32,
            text:*mut u16, size:*mut u32)->i32;
        fn CloseHandle(handle: Handle)->i32;
        fn GetCurrentProcessId()->u32;
    }

    fn process_is_kakao(pid: u32) -> bool {
        if pid == 0 || pid == unsafe { GetCurrentProcessId() } { return false; }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle == 0 { return false; }
            let mut name = [0u16; 2048];
            let mut length = name.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, name.as_mut_ptr(), &mut length) != 0;
            CloseHandle(handle);
            ok && Path::new(&String::from_utf16_lossy(&name[..length as usize]))
                .file_name().and_then(|text| text.to_str())
                .is_some_and(|text| text.eq_ignore_ascii_case("KakaoTalk.exe"))
        }
    }

    fn title(hwnd: Hwnd) -> String {
        let mut wide = [0u16; 512];
        let count = unsafe { GetWindowTextW(hwnd, wide.as_mut_ptr(), wide.len() as i32) };
        String::from_utf16_lossy(&wide[..count.max(0) as usize])
    }

    fn checked(hwnd: Hwnd) -> Option<ChatWindow> {
        unsafe {
            if hwnd == 0 || IsWindow(hwnd) == 0 || IsWindowVisible(hwnd) == 0 {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if !process_is_kakao(pid) { return None; }
            if GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_SYSMENU == 0 { return None; }
            if IsIconic(hwnd) == 0 {
                let mut rect = Rect::default();
                if GetWindowRect(hwnd, &mut rect) == 0 ||
                    rect.right - rect.left < 140 ||
                    rect.bottom - rect.top < 150 { return None; }
            }
        }
        let name = title(hwnd);
        if name.trim().is_empty() ||
            name == "카카오톡" || name.eq_ignore_ascii_case("KakaoTalk") {
            return None;
        }
        Some(ChatWindow {
            handle: format!("{:016X}", hwnd as usize),
            title: name,
        })
    }

    unsafe extern "system" fn collect(hwnd: Hwnd, data: isize) -> i32 {
        if let Some(chat) = checked(hwnd) {
            let output = unsafe { &mut *(data as *mut Vec<ChatWindow>) };
            output.push(chat);
        }
        1
    }

    pub fn probe() -> (Vec<ChatWindow>, Option<String>) {
        let mut windows: Vec<ChatWindow> = Vec::new();
        unsafe { EnumWindows(Some(collect), &mut windows as *mut _ as isize); }
        let foreground = checked(unsafe { GetForegroundWindow() })
            .and_then(|focus| windows.iter()
                .find(|chat| chat.handle == focus.handle).map(|v| v.handle.clone()));
        (windows, foreground)
    }
}

#[cfg(not(windows))]
mod native {
    use super::ChatWindow;
    pub fn probe() -> (Vec<ChatWindow>, Option<String>) { (Vec::new(), None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unselected_or_unknown_handle_never_yields_an_active_chat() {
        let mut tracker = KakaoTracker::default();
        tracker.chats = vec![
            ChatWindow { handle: "A".into(), title: "홍천악기".into() },
            ChatWindow { handle: "B".into(), title: "서울악기".into() },
        ];
        assert_eq!(tracker.context(None).1, None);
        assert_eq!(tracker.context(Some("missing")).1, None);
        let (windows, active) = tracker.context(Some("B"));
        assert_eq!(active.as_deref(), Some("B"));
        assert_eq!(windows.as_array().unwrap().len(), 2);
    }
}
