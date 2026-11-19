//! Windows KakaoTalk top-level-window dock for the native egui shell.
//! Restores every touched OS window on disconnect / normal process exit.
//! Dealer recognition is NOT performed here and there is no chat-picker UI.
use eframe::egui;

#[derive(Clone,Copy,Debug,PartialEq,Eq,Default)]
pub enum DockMode {
    #[default]
    Together,
    Split,
}

#[derive(Default)]
pub struct KakaoDock {
    #[cfg(windows)]
    session: Option<native::Session>,
}
impl KakaoDock {
    pub fn connected(&self)->bool {
        #[cfg(windows)] {self.session.is_some()}
        #[cfg(not(windows))] {false}
    }
    pub fn mode(&self)->DockMode {
        #[cfg(windows)] {self.session.as_ref().map(|s|s.mode).unwrap_or_default()}
        #[cfg(not(windows))] {DockMode::Together}
    }
    pub fn set_mode(
        &mut self,mode:DockMode,rect:egui::Rect,scale:f32
    )->Result<(),String>{
        #[cfg(windows)] {
            let session=self.session.as_mut().ok_or("카카오톡이 연결되지 않았습니다.")?;
            if session.mode==mode {return Ok(());}
            let previous=session.mode;
            session.mode=mode;
            session.last_sync=None;
            if let Err(reason)=session.sync(rect,scale) {
                session.mode=previous;
                session.last_sync=None;
                // The previous valid mode remains connected.
                if session.sync(rect,scale).is_err(){self.disconnect();}
                return Err(reason);
            }
        }
        #[cfg(not(windows))] {let _=(mode,rect,scale);
            return Err("카카오톡 도킹은 Windows 전용입니다.".into());}
        Ok(())
    }
    pub fn connect(
        &mut self,rect:egui::Rect,scale:f32
    )->Result<(),String>{
        #[cfg(windows)] {
            if self.session.is_some(){return Ok(());}
            let mut session=native::Session::connect()?;
            session.sync(rect,scale)?;
            self.session=Some(session);
            Ok(())
        }
        #[cfg(not(windows))] {
            let _=(rect,scale);
            Err("카카오톡 창 도킹은 Windows 전용 기능입니다.".into())
        }
    }
    pub fn sync(&mut self,rect:egui::Rect,scale:f32)->Result<(),String>{
        #[cfg(windows)] {
            if let Some(session)=self.session.as_mut(){
                if let Err(reason)=session.sync(rect,scale){
                    self.disconnect();
                    return Err(reason)
                }
            }
        }
        #[cfg(not(windows))] {let _=(rect,scale);}
        Ok(())
    }
    pub fn disconnect(&mut self){
        #[cfg(windows)] {self.session.take();}
    }
}
impl Drop for KakaoDock {
    fn drop(&mut self){self.disconnect();}
}

#[cfg(windows)]
mod native {
    use std::{path::Path,time::{Duration,Instant}};
    use super::egui;
    type Hwnd=isize;
    type Handle=isize;
    #[repr(C)]
    #[derive(Clone,Copy,Default,PartialEq,Eq)]
    struct Rect{left:i32,top:i32,right:i32,bottom:i32}
    impl Rect {
        fn width(self)->i32{self.right-self.left}
        fn height(self)->i32{self.bottom-self.top}
    }
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct Point{x:i32,y:i32}
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct MinMaxInfo{
        reserved:Point,max_size:Point,max_position:Point,
        min_track:Point,max_track:Point,
    }
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct MonitorInfo{
        size:u32,monitor:Rect,work:Rect,flags:u32,
    }
    #[repr(C)]
    #[derive(Clone,Copy,Default)]
    struct Placement{
        length:u32, flags:u32, show_cmd:u32,
        min_pos:Point,max_pos:Point,normal:Rect
    }
    const GWL_STYLE:i32=-16;
    const GWL_EXSTYLE:i32=-20;
    const WS_CAPTION:isize=0x00C00000;
    const WS_SYSMENU:isize=0x00080000;
    const WS_EX_TOPMOST:isize=0x00000008;
    const SWP_NOSIZE:u32=0x0001;
    const SWP_NOMOVE:u32=0x0002;
    const SWP_NOZORDER:u32=0x0004;
    const SWP_NOACTIVATE:u32=0x0010;
    const SWP_FRAMECHANGED:u32=0x0020;
    const HWND_TOPMOST:Hwnd=-1;
    const HWND_NOTOPMOST:Hwnd=-2;
    const PROCESS_QUERY_LIMITED_INFORMATION:u32=0x1000;
    const DWMWA_EXTENDED_FRAME_BOUNDS:u32=9;
    const WM_GETMINMAXINFO:u32=0x24;
    const SMTO_ABORTIFHUNG:u32=0x2;
    #[link(name="user32")]
    unsafe extern "system" {
        fn EnumWindows(cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,data:isize)->i32;
        fn EnumChildWindows(hwnd:Hwnd,cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,
            data:isize)->i32;
        fn SendMessageTimeoutW(hwnd:Hwnd,msg:u32,wparam:usize,lparam:isize,
            flags:u32,timeout:u32,result:*mut usize)->isize;
        fn GetDpiForWindow(hwnd:Hwnd)->u32;
        fn MonitorFromWindow(hwnd:Hwnd,flags:u32)->Handle;
        fn GetMonitorInfoW(handle:Handle,info:*mut MonitorInfo)->i32;
        fn GetWindowTextW(hwnd:Hwnd,buffer:*mut u16,len:i32)->i32;
        fn GetWindowThreadProcessId(hwnd:Hwnd,pid:*mut u32)->u32;
        fn GetForegroundWindow()->Hwnd;
        fn IsWindow(hwnd:Hwnd)->i32;
        fn IsWindowVisible(hwnd:Hwnd)->i32;
        fn IsIconic(hwnd:Hwnd)->i32;
        fn GetWindowRect(hwnd:Hwnd,rect:*mut Rect)->i32;
        fn GetClientRect(hwnd:Hwnd,rect:*mut Rect)->i32;
        fn ClientToScreen(hwnd:Hwnd,pos:*mut Point)->i32;
        fn GetWindowPlacement(hwnd:Hwnd,place:*mut Placement)->i32;
        fn SetWindowPlacement(hwnd:Hwnd,place:*const Placement)->i32;
        fn GetWindowLongPtrW(hwnd:Hwnd,index:i32)->isize;
        fn SetWindowLongPtrW(hwnd:Hwnd,index:i32,value:isize)->isize;
        fn SetWindowPos(hwnd:Hwnd,above:Hwnd,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
    }
    #[link(name="dwmapi")]
    unsafe extern "system" {
        fn DwmGetWindowAttribute(hwnd:Hwnd,attribute:u32,buffer:*mut Rect,size:u32)->i32;
    }
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access:u32,inherit:i32,pid:u32)->Handle;
        fn QueryFullProcessImageNameW(process:Handle,flags:u32,text:*mut u16,len:*mut u32)->i32;
        fn CloseHandle(process:Handle)->i32;
        fn GetCurrentProcessId()->u32;
    }
    fn os(label:&str)->String{
        format!("{label}: {}",std::io::Error::last_os_error())
    }
    fn title(hwnd:Hwnd)->String {
        let mut buffer=[0u16;512];
        let len=unsafe{GetWindowTextW(hwnd,buffer.as_mut_ptr(),buffer.len() as i32)};
        String::from_utf16_lossy(&buffer[..len.max(0) as usize])
    }
    fn pid(hwnd:Hwnd)->u32 {
        let mut out=0;
        unsafe{GetWindowThreadProcessId(hwnd,&mut out);}
        out
    }
    fn kakao_process(process_id:u32)->bool{
        if process_id==0 || process_id==unsafe{GetCurrentProcessId()}{return false;}
        unsafe{
            let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,process_id);
            if process==0{return false;}
            let mut buffer=[0u16;2048];
            let mut len=buffer.len() as u32;
            let ok=QueryFullProcessImageNameW(
                process,0,buffer.as_mut_ptr(),&mut len
            )!=0;
            CloseHandle(process);
            ok && Path::new(&String::from_utf16_lossy(&buffer[..len as usize]))
                .file_name().and_then(|name|name.to_str())
                .is_some_and(|name|name.eq_ignore_ascii_case("KakaoTalk.exe"))
        }
    }
    #[derive(Clone)]
    struct Candidate{hwnd:Hwnd,pid:u32,title:String,main:bool}
    fn candidates()->Vec<Candidate>{
        unsafe extern "system" fn collect(hwnd:Hwnd,param:isize)->i32{
            unsafe{
                if IsWindowVisible(hwnd)==0 {return 1;}
                if GetWindowLongPtrW(hwnd,GWL_STYLE)&WS_SYSMENU==0{return 1;}
                let process_id=pid(hwnd);
                if !kakao_process(process_id){return 1;}
                if IsIconic(hwnd)==0 {
                    let mut r=Rect::default();
                    if GetWindowRect(hwnd,&mut r)==0 ||
                        r.width()<140 || r.height()<150 {return 1;}
                }
                let name=title(hwnd);
                if name.trim().is_empty(){return 1;}
                let main=name=="카카오톡" || name.eq_ignore_ascii_case("KakaoTalk");
                let target=&mut *(param as *mut Vec<Candidate>);
                target.push(Candidate{hwnd,pid:process_id,title:name,main});
            }
            1
        }
        let mut list=Vec::new();
        unsafe{EnumWindows(Some(collect),&mut list as *mut _ as isize);}
        list
    }
    fn hwnd_of_hub()->Result<Hwnd,String>{
        unsafe extern "system" fn collect(hwnd:Hwnd,param:isize)->i32{
            let target=unsafe{&mut *(param as *mut Option<Hwnd>)};
            if unsafe{IsWindowVisible(hwnd)}!=0 &&
                pid(hwnd)==unsafe{GetCurrentProcessId()} &&
                title(hwnd)=="BANDO HUB" {
                *target=Some(hwnd);
                return 0;
            }
            1
        }
        let mut found=None;
        unsafe{EnumWindows(Some(collect),&mut found as *mut _ as isize);}
        found.ok_or("반도허브 창을 찾지 못했습니다.".into())
    }
    fn original_placement(hwnd:Hwnd)->Result<Placement,String>{
        let mut p=Placement{
            length:std::mem::size_of::<Placement>() as u32,
            ..Default::default()
        };
        if unsafe{GetWindowPlacement(hwnd,&mut p)}==0 {
            Err(os("카카오톡 창 원위치 조회 실패"))
        }else{Ok(p)}
    }
    fn outer(hwnd:Hwnd)->Result<Rect,String>{
        let mut r=Rect::default();
        if unsafe{GetWindowRect(hwnd,&mut r)}==0{
            Err(os("카카오톡 창 위치 조회 실패"))
        }else{Ok(r)}
    }
    fn visual(hwnd:Hwnd)->Result<Rect,String>{
        let r=outer(hwnd)?;
        let mut visual=Rect::default();
        let success=unsafe{DwmGetWindowAttribute(
            hwnd,DWMWA_EXTENDED_FRAME_BOUNDS,&mut visual,
            std::mem::size_of::<Rect>() as u32
        )}==0;
        Ok(if success && visual.width()>0 && visual.height()>0{visual}else{r})
    }
    fn move_visual(hwnd:Hwnd,desired:Rect)->Result<(),String>{
        let out=outer(hwnd)?;
        let vis=visual(hwnd)?;
        if vis==desired{return Ok(());}
        let x=desired.left-(vis.left-out.left);
        let y=desired.top-(vis.top-out.top);
        let w=desired.width()+(out.width()-vis.width());
        let h=desired.height()+(out.height()-vis.height());
        if unsafe{SetWindowPos(
            hwnd,0,x,y,w,h,SWP_NOACTIVATE|SWP_NOZORDER
        )}==0{
            return Err(os("카카오톡 창 배치 실패"));
        }
        // Match Tauri's set_visible_rect: Win32 minimum sizing is handled by
        // the selected layout, not an arbitrary egui overflow warning.
        Ok(())
    }
    fn min_visible_height(hwnd:Hwnd)->Result<i32,String>{
        let mut info=MinMaxInfo::default();
        let mut result=0usize;
        let ok=unsafe{SendMessageTimeoutW(
            hwnd,WM_GETMINMAXINFO,0,&mut info as *mut _ as isize,
            SMTO_ABORTIFHUNG,500,&mut result
        )};
        if ok==0 || info.min_track.y<=0{
            return Err("카카오톡 창 최소 높이 조회 실패".into());
        }
        let out=outer(hwnd)?;
        let shown=visual(hwnd)?;
        Ok((info.min_track.y-(out.height()-shown.height())).max(1))
    }
    unsafe extern "system" fn banner_child(hwnd:Hwnd,param:isize)->i32{
        let (reference,dpi,found)=unsafe{
            &mut *(param as *mut (Rect,u32,Vec<i32>))
        };
        if unsafe{IsWindowVisible(hwnd)}==0 {return 1;}
        if let Ok(child)=outer(hwnd){
            let tolerance=((3.0*(*dpi as f64)/96.0).ceil() as i32).max(2);
            if child.width() as f64>=reference.width() as f64*0.8
                && child.height() as f64>=24.0*(*dpi as f64)/96.0
                && child.height() as f64<=reference.height() as f64*0.35
                && (child.bottom-reference.bottom).abs()<=tolerance{
                found.push(child.top);
            }
        }
        1
    }
    fn banner_top(hwnd:Hwnd,reference:Rect)->i32{
        let dpi=unsafe{GetDpiForWindow(hwnd)}.max(96);
        let mut query=(reference,dpi,Vec::<i32>::new());
        unsafe{EnumChildWindows(hwnd,Some(banner_child),&mut query as *mut _ as isize);}
        query.2.into_iter().min().unwrap_or(reference.bottom)
    }
    fn offscreen_main_target(hub:Hwnd,main:Hwnd,area:Rect)->Result<Rect,String>{
        // React/Tauri OVERLAP: keep the conversation visible, move the MAIN
        // footer advertisement below the monitor. The chat composer stays in
        // the reserved pane.
        let handle=unsafe{MonitorFromWindow(hub,2)};
        if handle==0 {return Err("카카오톡 모니터를 확인할 수 없습니다.".into());}
        let mut monitor=MonitorInfo{
            size:std::mem::size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        if unsafe{GetMonitorInfoW(handle,&mut monitor)}==0 {
            return Err(os("모니터 크기 조회 실패"));
        }
        let dpi=unsafe{GetDpiForWindow(main)}.max(96);
        let extra=((160.0*dpi as f64/96.0).ceil() as i32).clamp(160,400);
        Ok(Rect{bottom:area.bottom.max(monitor.monitor.bottom)
            .saturating_add(extra),..area})
    }
    fn stack_targets(area:Rect,main:Hwnd,chats:&[Hwnd])
        ->Result<(Rect,Rect),String>{
        // Same geometry as Tauri's Stack mode, not a fixed 50/50 split.
        if chats.is_empty(){
            let old=visual(main)?;
            let height=if old.height()>=100 && old.height()<area.height(){
                old.height()
            }else{area.height()};
            return Ok((Rect{
                left:area.left,top:area.top,right:area.right,
                bottom:area.top+height
            },area));
        }
        let old=outer(main)?;
        let work=(||->Result<(Rect,Rect),String>{
            let previous=visual(main)?;
            move_visual(main,Rect{
                left:area.left,top:area.top,right:area.right,
                bottom:area.top+previous.height()
            })?;
            let minimum=min_visible_height(main)?;
            if minimum>=area.height()-100{
                return Err("카카오톡 기본창과 대화창을 나누어 표시할 공간이 부족합니다.".into());
            }
            let upper=Rect{
                left:area.left,top:area.top,right:area.right,
                bottom:area.top+minimum
            };
            move_visual(main,upper)?;
            let actual=visual(main)?;
            let chat_top=banner_top(main,actual)
                .clamp(area.top+1,area.bottom);
            let lower=Rect{
                left:area.left,top:chat_top,
                right:area.right,bottom:area.bottom
            };
            if lower.height()<100 {
                return Err("카카오톡 대화창 영역이 부족합니다.".into());
            }
            for chat in chats{
                let old=visual(*chat)?;
                move_visual(*chat,Rect{
                    left:area.left,top:chat_top,right:area.right,
                    bottom:chat_top+old.height()
                })?;
                if min_visible_height(*chat)?>lower.height(){
                    return Err("카카오톡 대화창 최소 높이를 확보할 수 없습니다.".into());
                }
            }
            Ok((upper,lower))
        })();
        if work.is_err(){
            unsafe{SetWindowPos(main,0,old.left,old.top,
                old.width(),old.height(),SWP_NOZORDER|SWP_NOACTIVATE);}
        }
        work
    }
    #[derive(Clone,Copy)]
    struct Tracked{
        hwnd:Hwnd,pid:u32,original:Placement,style:isize,topmost:bool,main:bool,
    }
    impl Tracked {
        fn new(c:&Candidate)->Result<Self,String>{
            Ok(Self{
                hwnd:c.hwnd,pid:c.pid,original:original_placement(c.hwnd)?,
                style:unsafe{GetWindowLongPtrW(c.hwnd,GWL_STYLE)},
                topmost:unsafe{GetWindowLongPtrW(c.hwnd,GWL_EXSTYLE)}
                    & WS_EX_TOPMOST!=0,
                main:c.main,
            })
        }
        fn alive(self)->bool{
            (unsafe{IsWindow(self.hwnd)}!=0)
                && pid(self.hwnd)==self.pid && kakao_process(self.pid)
        }
        fn restore(self){
            if !self.alive(){return;}
            let style=unsafe{GetWindowLongPtrW(self.hwnd,GWL_STYLE)};
            if style!=self.style {
                unsafe {
                    SetWindowLongPtrW(self.hwnd,GWL_STYLE,self.style);
                    SetWindowPos(self.hwnd,0,0,0,0,0,
                        SWP_NOMOVE|SWP_NOSIZE|SWP_NOZORDER|
                        SWP_NOACTIVATE|SWP_FRAMECHANGED);
                }
            }
            unsafe {
                SetWindowPos(self.hwnd,
                    if self.topmost{HWND_TOPMOST}else{HWND_NOTOPMOST},
                    0,0,0,0,
                    SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
                SetWindowPlacement(self.hwnd,&self.original);
            }
        }
        fn unframe(self){
            let style=unsafe{GetWindowLongPtrW(self.hwnd,GWL_STYLE)};
            if style & WS_CAPTION!=0 {
                unsafe{
                    SetWindowLongPtrW(self.hwnd,GWL_STYLE,style & !WS_CAPTION);
                    SetWindowPos(self.hwnd,0,0,0,0,0,
                        SWP_NOMOVE|SWP_NOSIZE|SWP_NOZORDER|
                        SWP_NOACTIVATE|SWP_FRAMECHANGED);
                }
            }
        }
    }
    fn physical_region(hub:Hwnd,ui_rect:egui::Rect,scale:f32)
        ->Result<Rect,String>{
        if !scale.is_finite() || scale<=0.0 {
            return Err("카카오톡 화면 배율이 올바르지 않습니다.".into());
        }
        let mut origin=Point::default();
        let mut client=Rect::default();
        if unsafe{ClientToScreen(hub,&mut origin)}==0 ||
            unsafe{GetClientRect(hub,&mut client)}==0 {
            return Err(os("반도허브 좌표 확인 실패"));
        }
        let to_px=|v:f32|->i32{(v*scale).round() as i32};
        let rect=Rect{
            left:origin.x+to_px(ui_rect.left()),
            top:origin.y+to_px(ui_rect.top()),
            right:origin.x+to_px(ui_rect.right()),
            bottom:origin.y+to_px(ui_rect.bottom()),
        };
        let client_right=origin.x+client.width();
        let client_bottom=origin.y+client.height();
        let rect=Rect{
            left:rect.left.max(origin.x),
            top:rect.top.max(origin.y),
            right:rect.right.min(client_right),
            bottom:rect.bottom.min(client_bottom),
        };
        // Tauri dock_region_native accepts a region from 100x100 upward.
        // The shell owns the actual React-sized Kakao minimum; do not add an
        // unrelated native "resize the pane" requirement.
        if rect.width()<100||rect.height()<100{
            return Err("카카오톡 배치 영역을 확인하지 못했습니다.".into());
        }
        Ok(rect)
    }
    pub struct Session{
        hub:Hwnd,pid:u32,windows:Vec<Tracked>,active_chat:Option<Hwnd>,
        pub(super) mode:super::DockMode,
        last_region:Option<Rect>,pub(super) last_sync:Option<Instant>,
    }
    impl Session{
        pub fn connect()->Result<Self,String>{
            let hub=hwnd_of_hub()?;
            let found=candidates();
            // React useAppModel::docking: CHAT first, then MAIN, then first.
            // Do not put a chat/dealer-selection step between Connect and dock.
            let chosen=found.iter().find(|c|!c.main)
                .or_else(||found.iter().find(|c|c.main))
                .or_else(||found.first()).cloned()
                .ok_or("실행 중인 카카오톡 창이 없습니다.")?;
            let focused=unsafe{GetForegroundWindow()};
            let pid=chosen.pid;
            let current=found.into_iter().filter(|c|c.pid==pid).collect::<Vec<_>>();
            let mut tracked=Vec::new();
            for c in current.iter().take(32){
                tracked.push(Tracked::new(c)?);
            }
            let active_chat=if !chosen.main{Some(chosen.hwnd)}else{
                let focused_chat=tracked.iter().find(|t|
                    !t.main && t.hwnd==focused
                ).map(|t|t.hwnd);
                focused_chat.or_else(||tracked.iter().find(|t|!t.main).map(|t|t.hwnd))
            };
            Ok(Self{
                hub,pid,windows:tracked,active_chat,
                mode:super::DockMode::Together,
                last_region:None,last_sync:None,
            })
        }
        pub fn sync(&mut self,rect:egui::Rect,scale:f32)->Result<(),String>{
            if unsafe{IsWindow(self.hub)}==0 ||
                pid(self.hub)!=unsafe{GetCurrentProcessId()} {
                return Err("반도허브 창 연결이 끊어졌습니다.".into());
            }
            // Check every frame for geometry changes, but scan/move no faster
            // than once per 350ms when the viewport hasn't changed.
            let region=physical_region(self.hub,rect,scale)?;
            // The dock must not float over other windows while the hub is
            // minimized. Restore normal positioning when the hub reappears.
            if unsafe{IsIconic(self.hub)}!=0{return Ok(());}
            if self.last_region==Some(region) &&
                self.last_sync.is_some_and(|t|t.elapsed()<Duration::from_millis(350)){
                return Ok(());
            }
            self.last_region=Some(region);
            self.last_sync=Some(Instant::now());
            let discovered=candidates();
            self.windows.retain(|w|w.alive());
            for c in discovered.iter().filter(|c|c.pid==self.pid){
                if !self.windows.iter().any(|w|w.hwnd==c.hwnd) &&
                    self.windows.len()<32 {
                    self.windows.push(Tracked::new(c)?);
                    if !c.main {self.active_chat=Some(c.hwnd);}
                }
            }
            if self.windows.is_empty(){
                return Err("연결된 카카오톡 창이 종료됐습니다.".into());
            }
            let foreground=unsafe{GetForegroundWindow()};
            if self.windows.iter().any(|w|!w.main && w.hwnd==foreground){
                self.active_chat=Some(foreground);
            }
            if self.active_chat.is_some_and(|hwnd|
                !self.windows.iter().any(|w|w.hwnd==hwnd)){
                self.active_chat=self.windows.iter().find(|w|!w.main).map(|w|w.hwnd);
            }
            let before=self.windows.iter().filter_map(|w|
                outer(w.hwnd).ok().map(|rect|(w.hwnd,rect))
            ).collect::<Vec<_>>();
            let base=self.windows.iter().find(|w|w.main && w.alive())
                .map(|w|w.hwnd);
            let chats=self.windows.iter().filter(|w|
                !w.main && w.alive() && unsafe{IsIconic(w.hwnd)}==0
            ).map(|w|w.hwnd).collect::<Vec<_>>();
            let layout=(||->Result<(),String>{
                for window in &self.windows {
                    if unsafe{IsIconic(window.hwnd)}==0{window.unframe();}
                }
                // Mirror Tauri native::sync_related: each mode uses a
                // different MAIN/CHAT geometry, not overlapping every HWND
                // over the same complete rectangle.
                let (main_target,chat_target)=if self.mode==super::DockMode::Split {
                    if let Some(base)=base {
                        if unsafe{IsIconic(base)}!=0 {(region,region)}
                        else {stack_targets(region,base,&chats)?}
                    } else {(region,region)}
                }else {(region,region)};
                for window in &self.windows {
                    if unsafe{IsIconic(window.hwnd)}!=0 {continue;}
                    let target=if window.main {
                        if self.mode==super::DockMode::Split {
                            main_target
                        }else{
                            offscreen_main_target(self.hub,window.hwnd,region)?
                        }
                    }else if self.mode==super::DockMode::Split && base.is_some(){
                        chat_target
                    }else{region};
                    move_visual(window.hwnd,target)?;
                }
                Ok(())
            })();
            if let Err(e)=layout {
                // Failed movement never leaves already moved windows docked.
                for (hwnd,prev) in before {
                    let _=unsafe{SetWindowPos(
                        hwnd,0,prev.left,prev.top,prev.width(),prev.height(),
                        SWP_NOZORDER|SWP_NOACTIVATE
                    )};
                }
                return Err(e);
            }
            // Keep the BANDO HUB HWND's z-order entirely untouched. The
            // old call SetWindowPos(self.hub, kakao_hwnd, ...) moved the HUB
            // *down* to KakaoTalk's previous layer; when that layer was behind
            // other apps, clicking Connect buried the entire BANDO HUB window.
            // Instead, when BANDO HUB or KakaoTalk is the foreground owner,
            // raise the selected KakaoTalk window within the normal (non-
            // topmost) z-order group, then position its messenger behind it.
            // An unrelated foreground app must never be overtaken by sync.
            let foreground_pid=pid(foreground);
            if foreground_pid==unsafe{GetCurrentProcessId()} ||
                foreground_pid==self.pid {
                let visible_chat=self.active_chat.filter(|chat|
                    chats.contains(chat) && unsafe{IsIconic(*chat)}==0
                );
                if let Some(top)=visible_chat.or(base) {
                    if unsafe{SetWindowPos(top,0,0,0,0,0,
                        SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)}==0 {
                        return Err(os("카카오톡 창 표시 순서 변경 실패"));
                    }
                    if let (Some(base),Some(chat))=(base,visible_chat) {
                        if base!=chat && unsafe{IsIconic(base)}==0 {
                            if unsafe{SetWindowPos(base,chat,0,0,0,0,
                                SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)}==0 {
                                return Err(os("카카오톡 기본창 표시 순서 변경 실패"));
                            }
                        }
                    }
                }
            }
            Ok(())
        }
    }
    impl Drop for Session {
        fn drop(&mut self){
            for window in self.windows.drain(..){
                window.restore();
            }
        }
    }
}
