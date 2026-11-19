//! Windows KakaoTalk top-level-window dock for the native egui shell.
//! Restores every touched OS window on disconnect / normal process exit.
//! Dealer recognition is NOT performed here and there is no chat-picker UI.
use eframe::egui;

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
    pub fn connect(
        &mut self,preferred_chat:Option<&str>,rect:egui::Rect,scale:f32
    )->Result<(),String>{
        #[cfg(windows)] {
            if self.session.is_some(){return Ok(());}
            let mut session=native::Session::connect(preferred_chat)?;
            session.sync(rect,scale)?;
            self.session=Some(session);
            Ok(())
        }
        #[cfg(not(windows))] {
            let _=(preferred_chat,rect,scale);
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
    #[link(name="user32")]
    unsafe extern "system" {
        fn EnumWindows(cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,data:isize)->i32;
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
        let actual=visual(hwnd)?;
        // Win32 may silently expand an app to its enforced minimum size.
        // Report failure instead of covering reception UI outside the dock.
        if actual.left<desired.left-4 || actual.top<desired.top-4 ||
            actual.right>desired.right+4 || actual.bottom>desired.bottom+4 {
            return Err("카카오톡 창이 왼쪽 배치 영역보다 큽니다. 패널 너비를 넓혀 주세요.".into());
        }
        Ok(())
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
        if rect.width()<200||rect.height()<250{
            return Err("카카오톡 배치 영역이 너무 작습니다.".into());
        }
        Ok(rect)
    }
    pub struct Session{
        hub:Hwnd,pid:u32,windows:Vec<Tracked>,active_chat:Option<Hwnd>,
        last_region:Option<Rect>,last_sync:Option<Instant>,
    }
    impl Session{
        pub fn connect(preferred_chat:Option<&str>)->Result<Self,String>{
            let hub=hwnd_of_hub()?;
            let found=candidates();
            if found.is_empty(){
                return Err("실행 중인 카카오톡 창이 없습니다.".into());
            }
            let focused=unsafe{GetForegroundWindow()};
            let chosen=preferred_chat.and_then(|id|{
                found.iter().find(|c|
                    format!("{:016X}",c.hwnd as usize)==id
                )
            }).or_else(||found.iter().find(|c|c.hwnd==focused))
                .or_else(||{
                    let roots=found.iter().filter(|c|c.main).collect::<Vec<_>>();
                    if roots.len()==1{Some(roots[0])}else{None}
                }).or_else(||{
                    let pids=found.iter().map(|c|c.pid)
                        .collect::<std::collections::HashSet<_>>();
                    if pids.len()==1{found.first()}else{None}
                }).cloned().ok_or("카카오톡이 여러 개 실행 중입니다. 연결할 대화창을 활성화한 뒤 다시 연결하세요.")?;
            let pid=chosen.pid;
            let current=found.into_iter().filter(|c|c.pid==pid).collect::<Vec<_>>();
            if !current.iter().any(|c|unsafe{IsIconic(c.hwnd)}==0) {
                return Err("카카오톡 창이 최소화되어 있습니다. 창을 복원하고 다시 연결하세요.".into());
            }
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
            let layout=(||->Result<(),String>{
                for w in &self.windows {
                    if unsafe{IsIconic(w.hwnd)}!=0{continue;}
                    w.unframe();
                    move_visual(w.hwnd,region)?;
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
            // Relative ordering only: do not make KakaoTalk globally TOPMOST
            // over unrelated applications. The chat remains above the hub.
            let foreground_pid=pid(foreground);
            if foreground_pid==unsafe{GetCurrentProcessId()} ||
                foreground_pid==self.pid {
                let top=self.active_chat.or_else(||
                    self.windows.iter().find(|w|w.main).map(|w|w.hwnd)
                );
                if let Some(top)=top {
                    // Keep the active Kakao chat above the hub and other Kakao
                    // windows. SWP_NOACTIVATE prevents focus theft.
                    unsafe{
                        SetWindowPos(top,0,0,0,0,0,
                            SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
                        SetWindowPos(self.hub,top,0,0,0,0,
                            SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
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
