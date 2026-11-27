//! Native window placement is UI-only, independent of SQLite and KakaoTalk.
//! GetWindowPlacement preserves the normal restore rectangle even when the
//! window is maximized/minimized. Never persist a minimized show command.
use std::{io,path::{Path,PathBuf}};

fn scoped_path(root:&Path,test:bool,name:&str)->PathBuf{
    root.join(if test{"developer-test"}else{"production"}).join(name)
}
pub(crate) fn ui_state_path(name:&str)->Option<PathBuf>{
    let root=std::env::var_os("LOCALAPPDATA").filter(|v|!v.is_empty())
        .map(|p|PathBuf::from(p).join("BANDO-HUB-egui-preview"))
        .or_else(||std::env::var_os("HOME").filter(|v|!v.is_empty())
            .map(|p|PathBuf::from(p).join(".config").join("bando-hub-egui-preview")))?;
    Some(scoped_path(&root,crate::launch_profile::test_launch(),name))
}
pub(crate) fn write_atomic(path:&Path,bytes:&[u8])->io::Result<()>{
    use std::io::Write;
    if let Some(parent)=path.parent(){std::fs::create_dir_all(parent)?;}
    let temp=path.with_extension(format!("tmp-{}",std::process::id()));
    let result=(||{
        let mut file=std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp,path)
    })();
    if result.is_err(){let _=std::fs::remove_file(&temp);}
    result
}

#[cfg(windows)]
mod native{
    use super::*;
    use std::time::{Duration,Instant};
    type Hwnd=isize;
    #[repr(C)]
    #[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
    struct Rect{left:i32,top:i32,right:i32,bottom:i32}
    impl Rect{
        fn valid(self)->bool{
            let w=i64::from(self.right)-i64::from(self.left);
            let h=i64::from(self.bottom)-i64::from(self.top);
            w>=64&&h>=64&&w<=100_000&&h<=100_000
                && [self.left,self.top,self.right,self.bottom].iter()
                    .all(|v|(-100_000..=100_000).contains(v))
        }
    }
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct Point{x:i32,y:i32}
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct Placement{length:u32,flags:u32,show_cmd:u32,min_pos:Point,max_pos:Point,normal:Rect}
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct MonitorInfo{size:u32,monitor:Rect,work:Rect,flags:u32}
    const SW_SHOWNORMAL:u32=1;
    const SW_SHOWMAXIMIZED:u32=3;
    const WPF_RESTORETOMAXIMIZED:u32=2;
    const MONITOR_DEFAULTTONEAREST:u32=2;
    #[link(name="user32")]
    unsafe extern "system"{
        fn EnumWindows(cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,param:isize)->i32;
        fn GetWindowThreadProcessId(hwnd:Hwnd,pid:*mut u32)->u32;
        fn GetWindowTextW(hwnd:Hwnd,buffer:*mut u16,len:i32)->i32;
        fn IsWindowVisible(hwnd:Hwnd)->i32;
        fn IsWindow(hwnd:Hwnd)->i32;
        fn IsIconic(hwnd:Hwnd)->i32;
        fn IsZoomed(hwnd:Hwnd)->i32;
        fn GetWindowPlacement(hwnd:Hwnd,placement:*mut Placement)->i32;
        fn SetWindowPlacement(hwnd:Hwnd,placement:*const Placement)->i32;
        fn MonitorFromRect(rect:*const Rect,flags:u32)->isize;
        fn GetMonitorInfoW(monitor:isize,info:*mut MonitorInfo)->i32;
    }
    fn hub()->Option<Hwnd>{
        unsafe extern "system" fn find(hwnd:Hwnd,param:isize)->i32{
            let mut pid=0u32;
            unsafe{GetWindowThreadProcessId(hwnd,&mut pid);}
            if pid!=std::process::id()||unsafe{IsWindowVisible(hwnd)}==0{return 1;}
            let mut title=[0u16;64];
            let len=unsafe{GetWindowTextW(hwnd,title.as_mut_ptr(),title.len() as i32)};
            if len>0&&String::from_utf16_lossy(&title[..len as usize])=="BANDO HUB"{
                unsafe{*(param as *mut Option<Hwnd>)=Some(hwnd);}
                return 0;
            }
            1
        }
        let mut result=None;
        unsafe{EnumWindows(Some(find),&mut result as *mut _ as isize);}
        result
    }
    #[derive(Clone,Copy,Debug,PartialEq,Eq)]
    struct Saved{normal:Rect,maximized:bool}
    impl Saved{
        fn decode(raw:&str)->Option<Self>{
            let v:serde_json::Value=serde_json::from_str(raw).ok()?;
            if v["version"].as_u64()!=Some(1){return None;}
            let get=|key:&str|i32::try_from(v[key].as_i64()?).ok();
            let normal=Rect{left:get("left")?,top:get("top")?,right:get("right")?,bottom:get("bottom")?};
            normal.valid().then_some(Self{normal,maximized:v["maximized"].as_bool()?})
        }
        fn encode(self)->String{
            serde_json::json!({"version":1,"left":self.normal.left,"top":self.normal.top,
                "right":self.normal.right,"bottom":self.normal.bottom,"maximized":self.maximized}).to_string()
        }
        fn capture(hwnd:Hwnd,previous:Option<Self>)->Option<Self>{
            let mut p=Placement{length:std::mem::size_of::<Placement>() as u32,..Default::default()};
            if unsafe{GetWindowPlacement(hwnd,&mut p)}==0||!p.normal.valid(){return None;}
            let minimized=unsafe{IsIconic(hwnd)}!=0;
            // On minimize, keep the last normal/maximized state; the
            // restore-to-maximized flag also covers immediate minimize.
            let maximized=if minimized{
                p.flags&WPF_RESTORETOMAXIMIZED!=0
                    ||previous.is_some_and(|old|old.maximized)
            }else{(unsafe{IsZoomed(hwnd)})!=0||p.show_cmd==SW_SHOWMAXIMIZED};
            Some(Self{normal:p.normal,maximized})
        }
        fn restore(self,hwnd:Hwnd)->io::Result<()>{
            let p=Placement{length:std::mem::size_of::<Placement>() as u32,
                show_cmd:if self.maximized{SW_SHOWMAXIMIZED}else{SW_SHOWNORMAL},
                normal:fit_to_monitor(self.normal),..Default::default()};
            if unsafe{SetWindowPlacement(hwnd,&p)}==0{Err(io::Error::last_os_error())}
            else{Ok(())}
        }
    }
    // WINDOWPLACEMENT normal coordinates are workspace-relative. Translate
    // the selected monitor's work area into that same coordinate space.
    fn fit_to_monitor(rect:Rect)->Rect{
        let monitor=unsafe{MonitorFromRect(&rect,MONITOR_DEFAULTTONEAREST)};
        let mut info=MonitorInfo{size:std::mem::size_of::<MonitorInfo>() as u32,..Default::default()};
        if monitor==0||unsafe{GetMonitorInfoW(monitor,&mut info)}==0{return rect;}
        let work=Rect{left:info.monitor.left,top:info.monitor.top,
            right:info.monitor.left+(info.work.right-info.work.left),
            bottom:info.monitor.top+(info.work.bottom-info.work.top)};
        fit(rect,work)
    }
    fn fit(rect:Rect,work:Rect)->Rect{
        if !work.valid(){return rect;}
        let w=(rect.right-rect.left).min(work.right-work.left);
        let h=(rect.bottom-rect.top).min(work.bottom-work.top);
        let left=rect.left.clamp(work.left,work.right-w);
        let top=rect.top.clamp(work.top,work.bottom-h);
        Rect{left,top,right:left+w,bottom:top+h}
    }
    #[derive(Default)]
    pub struct Tracker{
        initialized:bool,
        hwnd:Option<Hwnd>,
        last_write:Option<Instant>,
        last_observed:Option<Saved>,
        last_saved:Option<Saved>,
    }
    impl Tracker{
        fn handle(&mut self)->Option<Hwnd>{
            if let Some(hwnd)=self.hwnd{
                let mut pid=0u32;
                unsafe{GetWindowThreadProcessId(hwnd,&mut pid);}
                if pid==std::process::id()&&unsafe{IsWindow(hwnd)}!=0{
                    return Some(hwnd);
                }
            }
            self.hwnd=hub();
            self.hwnd
        }
        pub fn update(&mut self,ctx:&eframe::egui::Context){
            let Some(hwnd)=self.handle() else{
                if !self.initialized{ctx.request_repaint_after(Duration::from_millis(100));}
                return;
            };
            if !self.initialized{
                self.initialized=true;
                self.last_saved=ui_state_path("window-state.json")
                    .and_then(|p|std::fs::read_to_string(p).ok())
                    .and_then(|s|Saved::decode(&s));
                self.last_observed=self.last_saved;
                if let Some(saved)=self.last_saved{
                    if let Err(error)=saved.restore(hwnd){
                        eprintln!("[BANDO HUB] 창 상태 복원 실패: {error}");
                    }
                }
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            // Observe before every UI action (including auto-minimize), but
            // throttle actual disk writes. Native HWND lookup is cached.
            self.observe(hwnd);
            if self.last_observed==self.last_saved{return;}
            let remaining=self.last_write.map(|t|
                Duration::from_millis(500).saturating_sub(t.elapsed()))
                .unwrap_or_default();
            if !remaining.is_zero(){
                ctx.request_repaint_after(remaining);
                return;
            }
            self.save();
        }
        fn observe(&mut self,hwnd:Hwnd){
            if let Some(current)=Saved::capture(hwnd,self.last_observed){
                self.last_observed=Some(current);
            }
        }
        fn save(&mut self){
            let Some(current)=self.last_observed else{return;};
            if self.last_saved==Some(current){return;}
            let Some(path)=ui_state_path("window-state.json") else{return;};
            self.last_write=Some(Instant::now());
            match write_atomic(&path,current.encode().as_bytes()){
                Ok(())=>self.last_saved=Some(current),
                Err(error)=>eprintln!("[BANDO HUB] 창 상태 저장 실패: {error}"),
            }
        }
        pub fn finish(&mut self){
            if !self.initialized{return;}
            // The cached HWND may still be valid when eframe hides the
            // window on close. If already destroyed, flush last observation.
            if let Some(hwnd)=self.handle(){self.observe(hwnd);}
            self.save();
        }
    }
    #[cfg(test)]
    mod tests{
        use super::*;
        #[test]
        fn window_state_roundtrip_and_invalid_file(){
            let saved=Saved{normal:Rect{left:-1500,top:40,right:-100,bottom:840},maximized:true};
            assert_eq!(Saved::decode(&saved.encode()),Some(saved));
            assert_eq!(Saved::decode("{}"),None);
            assert_eq!(Saved::decode(r#"{"version":1,"left":0,"top":0,"right":0,"bottom":0,"maximized":false}"#),None);
        }
        #[test]
        fn missing_monitor_clamps_to_current_work_area(){
            let work=Rect{left:0,top:0,right:1920,bottom:1040};
            let rect=Rect{left:-1700,top:50,right:-100,bottom:950};
            assert_eq!(fit(rect,work),Rect{left:0,top:50,right:1600,bottom:950});
        }
    }
}
#[cfg(windows)]
pub use native::Tracker;
#[cfg(not(windows))]
#[derive(Default)]
pub struct Tracker;
#[cfg(not(windows))]
impl Tracker{
    pub fn update(&mut self,_ctx:&eframe::egui::Context){}
    pub fn finish(&mut self){}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn operating_and_test_ui_files_are_separate(){
        let root=Path::new("ui");
        for name in ["layout.txt","window-state.json"]{
            assert_ne!(scoped_path(root,false,name),scoped_path(root,true,name));
        }
    }
}
