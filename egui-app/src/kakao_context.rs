//! Shared Windows KakaoTalk window discovery and event monitor.
//! Reads only verified top-level window metadata. The dock alone owns layout
//! changes; the monitor never reads chat messages or modifies foreign windows.

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
    last_focused: Option<(String, Instant)>,
}

impl KakaoTracker {
    pub fn refresh(&mut self,ctx:&eframe::egui::Context) {
        #[cfg(windows)] {
            windows::start(ctx.clone());
            let snapshot=windows::snapshot();
            let chats:Vec<_>=snapshot.windows.iter().filter(|w|!w.main)
                .map(|w|ChatWindow{handle:format!("{:016X}",w.hwnd as usize),title:w.title.clone()})
                .collect();
            if self.chats!=chats{self.chats=chats;}
            if let Some(chat)=snapshot.windows.iter().find(|w|
                !w.main && w.hwnd==snapshot.foreground){
                self.last_focused=Some((format!("{:016X}",chat.hwnd as usize),Instant::now()));
            }
            if self.last_focused.as_ref().is_some_and(|(handle,_)|
                !self.chats.iter().any(|chat|chat.handle==*handle)){
                self.last_focused=None;
            }
        }
        #[cfg(not(windows))] {let _=ctx;}
    }

    pub fn shutdown(&mut self){
        #[cfg(windows)] windows::shutdown();
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
        let Some(handle) = requested else {return (json!([]), None);};
        if self.verified(handle).is_none() {return (json!([]), None);}
        let windows:Vec<Value>=self.chats.iter().map(|chat|json!({
            "kind":"CHAT","handle":chat.handle,"title":chat.title
        })).collect();
        (json!(windows),Some(handle.to_owned()))
    }
}

#[cfg(windows)]
pub(crate) mod windows {
    use std::{cell::{Cell,RefCell},collections::{HashMap,HashSet},path::Path,
        sync::{OnceLock,Mutex,atomic::{AtomicBool,AtomicU32,Ordering}},
        time::{Duration,Instant}};
    type Hwnd=isize;
    type Handle=isize;
    #[repr(C)]
    #[derive(Default)]
    struct Rect{left:i32,top:i32,right:i32,bottom:i32}
    #[repr(C)]
    #[derive(Default)]
    struct Msg{hwnd:Hwnd,message:u32,wparam:usize,lparam:isize,time:u32,
        x:i32,y:i32,private:u32}
    const WS_SYSMENU:isize=0x00080000;
    const GWL_STYLE:i32=-16;
    const PROCESS_QUERY_LIMITED_INFORMATION:u32=0x1000;
    const FOREGROUND:u32=0x0003;
    const MOVE_END:u32=0x000B;
    const MINIMIZE_START:u32=0x0016;
    const MINIMIZE_END:u32=0x0017;
    const CREATE:u32=0x8000;
    const DESTROY:u32=0x8001;
    const SHOW:u32=0x8002;
    const HIDE:u32=0x8003;
    const NAMECHANGE:u32=0x800C;
    const WM_QUIT:u32=0x0012;
    const QS_ALLINPUT:u32=0x04FF;
    const MWMO_INPUTAVAILABLE:u32=4;
    const PM_REMOVE:u32=1;
    const FALLBACK:Duration=Duration::from_secs(5);
    const COALESCE:Duration=Duration::from_millis(100);

    #[link(name="user32")]
    unsafe extern "system" {
        fn EnumWindows(cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,data:isize)->i32;
        fn IsWindow(hwnd:Hwnd)->i32;
        fn IsWindowVisible(hwnd:Hwnd)->i32;
        fn IsIconic(hwnd:Hwnd)->i32;
        fn GetWindowRect(hwnd:Hwnd,rect:*mut Rect)->i32;
        fn GetWindowThreadProcessId(hwnd:Hwnd,pid:*mut u32)->u32;
        fn GetWindowTextW(hwnd:Hwnd,text:*mut u16,max:i32)->i32;
        fn GetWindowLongPtrW(hwnd:Hwnd,index:i32)->isize;
        fn GetForegroundWindow()->Hwnd;
        fn GetAncestor(hwnd:Hwnd,flags:u32)->Hwnd;
        fn SetWinEventHook(min:u32,max:u32,module:Handle,
            cb:Option<unsafe extern "system" fn(Handle,u32,Hwnd,i32,i32,u32,u32)>,
            process:u32,thread:u32,flags:u32)->Handle;
        fn UnhookWinEvent(hook:Handle)->i32;
        fn PeekMessageW(msg:*mut Msg,hwnd:Hwnd,min:u32,max:u32,remove:u32)->i32;
        fn TranslateMessage(msg:*const Msg)->i32;
        fn DispatchMessageW(msg:*const Msg)->isize;
        fn PostThreadMessageW(thread:u32,msg:u32,wparam:usize,lparam:isize)->i32;
        fn MsgWaitForMultipleObjectsEx(count:u32,handles:*const Handle,ms:u32,
            mask:u32,flags:u32)->u32;
    }
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access:u32,inherit:i32,pid:u32)->Handle;
        fn QueryFullProcessImageNameW(process:Handle,flags:u32,text:*mut u16,size:*mut u32)->i32;
        fn CloseHandle(handle:Handle)->i32;
        fn GetCurrentProcessId()->u32;
        fn GetCurrentThreadId()->u32;
    }
    pub(crate) fn pid(hwnd:Hwnd)->u32{
        let mut pid=0; unsafe{GetWindowThreadProcessId(hwnd,&mut pid);} pid
    }
    pub(crate) fn process_is_kakao(pid:u32)->bool{
        if pid==0 || pid==unsafe{GetCurrentProcessId()}{return false;}
        unsafe{
            let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);
            if handle==0{return false;}
            let mut name=[0u16;2048];let mut len=name.len() as u32;
            let ok=QueryFullProcessImageNameW(handle,0,name.as_mut_ptr(),&mut len)!=0;
            CloseHandle(handle);
            ok && Path::new(&String::from_utf16_lossy(&name[..len as usize]))
                .file_name().and_then(|s|s.to_str())
                .is_some_and(|s|s.eq_ignore_ascii_case("KakaoTalk.exe"))
        }
    }
    fn title(hwnd:Hwnd)->String{
        let mut buf=[0u16;512];
        let len=unsafe{GetWindowTextW(hwnd,buf.as_mut_ptr(),buf.len() as i32)};
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }
    #[derive(Clone,Debug,PartialEq,Eq)]
    pub(crate) struct Candidate{
        pub hwnd:Hwnd,pub pid:u32,pub title:String,pub main:bool,pub iconic:bool,
    }
    #[derive(Clone,Default)]
    pub(crate) struct Snapshot{
        pub windows:Vec<Candidate>,pub foreground:Hwnd,pub layout_epoch:u64,
    }
    impl Snapshot{
        pub fn layout_key(&self,pid:u32)->Vec<(Hwnd,bool,bool)>{
            let mut key:Vec<_>=self.windows.iter().filter(|w|w.pid==pid)
                .map(|w|(w.hwnd,w.main,w.iconic)).collect();
            key.sort_unstable();key
        }
    }
    /// Only one native discovery implementation. Within a scan, verify each
    /// process once even when it owns multiple top-level windows.
    pub(crate) fn scan()->Snapshot{
        #[derive(Default)]
        struct Scan{windows:Vec<Candidate>,processes:HashMap<u32,bool>}
        unsafe extern "system" fn collect(hwnd:Hwnd,data:isize)->i32{
            let scan=unsafe{&mut *(data as *mut Scan)};
            if unsafe{IsWindow(hwnd)}==0 || unsafe{IsWindowVisible(hwnd)}==0 ||
                unsafe{GetWindowLongPtrW(hwnd,GWL_STYLE)}&WS_SYSMENU==0{return 1;}
            let process=pid(hwnd);
            if !*scan.processes.entry(process).or_insert_with(||process_is_kakao(process)){return 1;}
            let iconic=unsafe{IsIconic(hwnd)}!=0;
            if !iconic{
                let mut rect=Rect::default();
                if unsafe{GetWindowRect(hwnd,&mut rect)}==0 ||
                    rect.right-rect.left<140 || rect.bottom-rect.top<150{return 1;}
            }
            let title=title(hwnd);
            if title.trim().is_empty(){return 1;}
            let main=title=="카카오톡" || title.eq_ignore_ascii_case("KakaoTalk");
            scan.windows.push(Candidate{hwnd,pid:process,title,main,iconic});
            1
        }
        let mut state=Scan::default();
        unsafe{EnumWindows(Some(collect),&mut state as *mut _ as isize);}
        Snapshot{windows:state.windows,foreground:unsafe{GetForegroundWindow()},layout_epoch:0}
    }
    struct Monitor{
        snapshot:Mutex<Snapshot>,stop:AtomicBool,thread:AtomicU32,
    }
    static MONITOR:OnceLock<Monitor>=OnceLock::new();
    thread_local!{
        static DIRTY:Cell<bool>=const{Cell::new(false)};
        static GEOMETRY:Cell<bool>=const{Cell::new(false)};
        static KNOWN:RefCell<HashSet<Hwnd>>=RefCell::new(HashSet::new());
    }
    unsafe extern "system" fn event(_:Handle,event:u32,hwnd:Hwnd,object:i32,
        child:i32,_thread:u32,_time:u32){
        if event==FOREGROUND{DIRTY.with(|d|d.set(true));return;}
        if hwnd==0 || object!=0 || child!=0{return;}
        let known=KNOWN.with(|set|set.borrow().contains(&hwnd));
        if matches!(event,MOVE_END|MINIMIZE_START|MINIMIZE_END){
            if known || pid(hwnd)==unsafe{GetCurrentProcessId()}{
                GEOMETRY.with(|d|d.set(true));DIRTY.with(|d|d.set(true));
            }
            return;
        }
        if !matches!(event,CREATE|DESTROY|SHOW|HIDE|NAMECHANGE){return;}
        // A destroy event can refer to an already invalid HWND. Retain its
        // cached identity for invalidation; do not query a dead process.
        if known || (event!=DESTROY && unsafe{GetAncestor(hwnd,2)}==hwnd
            && unsafe{IsWindowVisible(hwnd)}!=0 && process_is_kakao(pid(hwnd))){
            DIRTY.with(|d|d.set(true));
        }
    }
    fn worker(m:&'static Monitor,ctx:eframe::egui::Context){
        let mut msg=Msg::default();
        // Force a thread message queue before publishing the thread ID.
        unsafe{PeekMessageW(&mut msg,0,0,0,0);}
        m.thread.store(unsafe{GetCurrentThreadId()},Ordering::Release);
        // Out-of-context hooks run on this message-pumping worker, never in
        // Kakao or the egui rendering thread. No location-change hook:
        // reacting to our own SetWindowPos would cause a feedback loop.
        let hooks=[(FOREGROUND,FOREGROUND),(MOVE_END,MOVE_END),
            (MINIMIZE_START,MINIMIZE_END),(CREATE,NAMECHANGE)]
            .map(|(a,b)|unsafe{SetWinEventHook(a,b,0,Some(event),0,0,0)});
        if hooks.iter().any(|h|*h==0){
            eprintln!("[BANDO HUB] 일부 카카오톡 창 이벤트 연결 실패: 안전 조회 유지");
        }
        let mut last=Instant::now()-FALLBACK;
        let mut epoch=0u64;
        let mut dirty=true;
        while !m.stop.load(Ordering::Acquire){
            // Delivery of WinEvent callbacks is serviced by PeekMessage.
            for _ in 0..256{
                if unsafe{PeekMessageW(&mut msg,0,0,0,PM_REMOVE)}==0{break;}
                if msg.message==WM_QUIT{m.stop.store(true,Ordering::Release);break;}
                unsafe{TranslateMessage(&msg);DispatchMessageW(&msg);}
            }
            if m.stop.load(Ordering::Acquire){break;}
            dirty|=DIRTY.with(|d|d.replace(false));
            if last.elapsed()>=FALLBACK || (dirty && last.elapsed()>=COALESCE){
                let mut next=scan();
                if GEOMETRY.with(|d|d.replace(false)){epoch=epoch.wrapping_add(1);}
                next.layout_epoch=epoch;
                KNOWN.with(|known|{
                    *known.borrow_mut()=next.windows.iter().map(|w|w.hwnd).collect();
                });
                let mut current=m.snapshot.lock().unwrap_or_else(|p|p.into_inner());
                let changed=current.windows!=next.windows || current.foreground!=next.foreground ||
                    current.layout_epoch!=next.layout_epoch;
                *current=next;drop(current);
                if changed{ctx.request_repaint();}
                last=Instant::now();dirty=false;
            }
            let delay=if dirty{COALESCE.saturating_sub(last.elapsed())}
                else{FALLBACK.saturating_sub(last.elapsed())};
            let ms=delay.as_millis().max(1).min(5000) as u32;
            unsafe{MsgWaitForMultipleObjectsEx(0,std::ptr::null(),ms,QS_ALLINPUT,MWMO_INPUTAVAILABLE);}
        }
        for hook in hooks.into_iter().filter(|h|*h!=0){unsafe{UnhookWinEvent(hook);}}
        m.thread.store(0,Ordering::Release);
    }
    pub(crate) fn start(ctx:eframe::egui::Context){
        let m=MONITOR.get_or_init(||Monitor{snapshot:Mutex::new(Snapshot::default()),
            stop:AtomicBool::new(false),thread:AtomicU32::new(0)});
        // Only one UI owner exists. Thread ID is initially zero, so use a
        // separate one-shot gate to avoid starting duplicate workers.
        static STARTED:OnceLock<()>=OnceLock::new();
        STARTED.get_or_init(||{std::thread::spawn(move||worker(m,ctx));});
    }
    pub(crate) fn snapshot()->Snapshot{
        MONITOR.get().map(|m|m.snapshot.lock().unwrap_or_else(|p|p.into_inner()).clone())
            .unwrap_or_default()
    }
    pub(crate) fn shutdown(){
        if let Some(m)=MONITOR.get(){
            m.stop.store(true,Ordering::Release);
            let id=m.thread.load(Ordering::Acquire);
            if id!=0{unsafe{PostThreadMessageW(id,WM_QUIT,0,0);}}
        }
    }
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
