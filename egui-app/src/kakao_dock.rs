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
            // The footer belongs to the Kakao MAIN HWND, not to the view
            // mode. Re-measuring the already extended HWND would inflate it.
            session.last_sync=None;
            if let Err(reason)=session.sync(rect,scale) {
                session.mode=previous;
                session.last_sync=None;
                // The previous valid mode remains connected.
                if session.sync(rect,scale).is_err(){self.disconnect();}
                return Err(reason);
            }
            Ok(())
        }
        #[cfg(not(windows))] {let _=(mode,rect,scale);
            Err("카카오톡 도킹은 Windows 전용입니다.".into())}
    }
    pub fn connect(
        &mut self,rect:egui::Rect,scale:f32
    )->Result<(),String>{
        #[cfg(windows)] {
            if self.session.is_some(){return Ok(());}
            let mut session=native::Session::connect()?;
            session.restore_minimized()?;
            // Owner-linked HWNDs follow the HUB window without a foreground
            // event hook. Unowned Kakao windows retain a throttled fallback.
            session.attach_independent_windows();
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
    use std::time::{Duration,Instant};
    use crate::kakao_context::windows::{self,Candidate,process_is_kakao};
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
    const GWLP_HWNDPARENT:i32=-8; // Top-level window OWNER, not child parent.
    const GW_OWNER:u32=4;
    const WS_CAPTION:isize=0x00C00000;
    const WS_EX_TOPMOST:isize=0x00000008;
    const SWP_NOSIZE:u32=0x0001;
    const SWP_NOMOVE:u32=0x0002;
    const SWP_NOZORDER:u32=0x0004;
    const SWP_NOACTIVATE:u32=0x0010;
    const SWP_FRAMECHANGED:u32=0x0020;
    const GW_HWNDPREV:u32=3;
    const HWND_TOP:Hwnd=0;
    const HWND_TOPMOST:Hwnd=-1;
    const HWND_NOTOPMOST:Hwnd=-2;
    const DWMWA_EXTENDED_FRAME_BOUNDS:u32=9;
    const WM_GETMINMAXINFO:u32=0x24;
    const SMTO_ABORTIFHUNG:u32=0x2;
    const MONITOR_DEFAULTTONEAREST:u32=2;
    #[link(name="user32")]
    unsafe extern "system" {
        fn EnumWindows(cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,data:isize)->i32;
        fn EnumChildWindows(hwnd:Hwnd,cb:Option<unsafe extern "system" fn(Hwnd,isize)->i32>,
            data:isize)->i32;
        fn SendMessageTimeoutW(hwnd:Hwnd,msg:u32,wparam:usize,lparam:isize,
            flags:u32,timeout:u32,result:*mut usize)->isize;
        fn GetDpiForWindow(hwnd:Hwnd)->u32;
        fn MonitorFromWindow(hwnd:Hwnd,flags:u32)->Handle;
        fn GetMonitorInfoW(monitor:Handle,info:*mut MonitorInfo)->i32;
        fn GetWindowTextW(hwnd:Hwnd,buffer:*mut u16,len:i32)->i32;
        fn GetWindowThreadProcessId(hwnd:Hwnd,pid:*mut u32)->u32;
        fn GetForegroundWindow()->Hwnd;
        fn GetWindow(hwnd:Hwnd,command:u32)->Hwnd;
        fn IsWindow(hwnd:Hwnd)->i32;
        fn IsWindowVisible(hwnd:Hwnd)->i32;
        fn IsIconic(hwnd:Hwnd)->i32;
        fn ShowWindow(hwnd:Hwnd,command:i32)->i32;
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
        fn GetCurrentProcessId()->u32;
        fn SetLastError(code:u32);
        fn GetLastError()->u32;
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
    // Find whether 'window' is behind 'reference' in the current Z-order.
    // Avoid constant SetWindowPos calls if the correct order already holds.
    fn below(window:Hwnd,reference:Hwnd)->bool {
        if window==0 || reference==0 || window==reference {return false;}
        let mut cursor=window;
        for _ in 0..512 {
            cursor=unsafe{GetWindow(cursor,GW_HWNDPREV)};
            if cursor==0{return false;}
            if cursor==reference{return true;}
        }
        false
    }

    // Owner-linked Kakao HWNDs already follow the HUB. This periodic check
    // only repairs unowned HWND layering and main/chat relative order.
    #[derive(Clone,Copy)]
    struct DockedHandle{hwnd:Hwnd,main:bool}
    fn restore_docked_order(
        hub:Hwnd,windows:&[DockedHandle],active_chat:Option<Hwnd>,foreground:Hwnd
    )->Result<(),String>{
        if unsafe{IsWindow(hub)}==0 || unsafe{IsIconic(hub)}!=0 {
            return Ok(());
        }
        // A third-party foreground window must NEVER cause a Kakao raise.
        if foreground!=hub && !windows.iter().any(|w|w.hwnd==foreground){
            return Ok(());
        }
        let shown=|hwnd:Hwnd|unsafe{IsWindow(hwnd)}!=0 &&
            unsafe{IsIconic(hwnd)}==0 && unsafe{IsWindowVisible(hwnd)}!=0;
        let base=windows.iter().find(|w|w.main && shown(w.hwnd))
            .map(|w|w.hwnd);
        let chat=active_chat.filter(|hwnd|windows.iter().any(|w|
            !w.main && w.hwnd==*hwnd && shown(*hwnd)
        ));
        let Some(top)=chat.or(base) else{return Ok(())};
        if (below(top,hub) || base.is_some_and(|main|
            chat.is_some_and(|active|main!=active && below(active,main))))
            && unsafe{SetWindowPos(top,HWND_TOP,0,0,0,0,
                SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)}==0 {
            return Err(os("카카오톡 창 표시 순서 복원 실패"));
        }
        if let (Some(main),Some(active))=(base,chat){
            if main!=active && (below(main,hub) || below(active,main))
                && unsafe{SetWindowPos(main,active,0,0,0,0,
                    SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)}==0 {
                return Err(os("카카오톡 기본창 표시 순서 복원 실패"));
            }
        }
        Ok(())
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
        // DWM may clip extended-frame bounds at a physical screen edge.
        // Such clipping must never be interpreted as a larger border
        // on each 350ms sync, which previously inflated the main HWND.
        let dpi=unsafe{GetDpiForWindow(hwnd)}.max(96);
        let cap=((24.0*dpi as f64/96.0).ceil() as i32).clamp(24,80);
        let left=(vis.left-out.left).clamp(-cap,cap);
        let top=(vis.top-out.top).clamp(-cap,cap);
        let right=(out.right-vis.right).clamp(-cap,cap);
        let observed_bottom=out.bottom-vis.bottom;
        let bottom=if observed_bottom.abs()>cap {
            top.max(left).max(right).max(0)
        }else{
            observed_bottom
        };
        let x=desired.left-left;
        let y=desired.top-top;
        let w=(desired.width()+left+right).max(100);
        let h=(desired.height()+top+bottom).max(100);
        let target=Rect{left:x,top:y,right:x+w,bottom:y+h};
        if out==target{return Ok(());}
        if unsafe{SetWindowPos(
            hwnd,0,x,y,w,h,SWP_NOACTIVATE|SWP_NOZORDER
        )}==0{
            return Err(os("카카오톡 창 배치 실패"));
        }
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
    fn measure_footer_extension(main:Hwnd)->Result<i32,String>{
        // Read the bottom-aligned advertisement/banner once BEFORE stretching
        // the main Kakao window beyond the panel. Off-screen geometry is
        // never used as a measurement input.
        let shown=visual(main)?;
        let dpi=unsafe{GetDpiForWindow(main)}.max(96);
        let top=banner_top(main,shown);
        let detected=shown.bottom.saturating_sub(top);
        let minimum=((48.0*dpi as f64/96.0).ceil() as i32).max(48);
        let maximum=((300.0*dpi as f64/96.0).ceil() as i32).min(600);
        if detected>=minimum && detected<=maximum {
            return Ok(detected);
        }
        // Kakao sometimes renders its advertisement in the main surface,
        // with no independent child HWND to measure. Use the existing
        // Kakao footer allowance then, never the monitor-bottom distance.
        Ok(((160.0*dpi as f64/96.0).ceil() as i32).clamp(160,400))
    }
    fn overlap_main_target(area:Rect,footer:i32)->Rect{
        Rect{bottom:area.bottom.saturating_add(footer.max(0)),..area}
    }
    fn monitor_screen_bottom(hub:Hwnd)->Option<i32>{
        // Resolve the physical monitor of BANDO HUB, NOT the Kakao MAIN
        // window, which can already extend far offscreen after docking.
        let monitor=unsafe{MonitorFromWindow(hub,MONITOR_DEFAULTTONEAREST)};
        if monitor==0{return None;}
        let mut info=MonitorInfo{
            size:std::mem::size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        if unsafe{GetMonitorInfoW(monitor,&mut info)}==0 ||
            info.monitor.height()<=0 {
            return None;
        }
        Some(info.monitor.bottom)
    }
    fn split_main_offscreen_target(area:Rect,screen_bottom:i32,footer:i32)->Rect{
        // A Split MAIN occupies only the upper part of the Kakao dock.
        // Adding the footer just to that upper slot leaves the ad visible.
        // Extend the MAIN *behind* the CHAT until its banner is physically
        // beyond the monitor. CHAT keeps its original lower target intact.
        Rect{
            bottom:area.bottom.max(screen_bottom)
                .saturating_add(footer.max(0)),
            ..area
        }
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
        original_owner:Hwnd,owner_attempted:bool,owned_by_hub:Hwnd,
    }
    impl Tracked {
        fn new(c:&Candidate)->Result<Self,String>{
            // Worker snapshots are advisory. Recheck the actual HWND/PID
            // before taking ownership or recording a restoration target.
            if unsafe{IsWindow(c.hwnd)}==0 || pid(c.hwnd)!=c.pid || !process_is_kakao(c.pid){
                return Err("카카오톡 창이 변경되었습니다.".into());
            }
            Ok(Self{
                hwnd:c.hwnd,pid:c.pid,original:original_placement(c.hwnd)?,
                style:unsafe{GetWindowLongPtrW(c.hwnd,GWL_STYLE)},
                topmost:unsafe{GetWindowLongPtrW(c.hwnd,GWL_EXSTYLE)}
                    & WS_EX_TOPMOST!=0,
                main:c.main,
                original_owner:unsafe{GetWindow(c.hwnd,GW_OWNER)},
                owner_attempted:false,owned_by_hub:0,
            })
        }
        fn alive(self)->bool{
            (unsafe{IsWindow(self.hwnd)}!=0)
                && pid(self.hwnd)==self.pid && process_is_kakao(self.pid)
        }
        // Touch only genuinely independent top-level Kakao HWNDs. A Kakao
        // dialog that already has its own owner must retain that relationship.
        fn try_own(&mut self,hub:Hwnd)->Result<(),String>{
            if self.owner_attempted || !self.alive(){return Ok(());}
            self.owner_attempted=true;
            if self.original_owner!=0 || unsafe{GetWindow(self.hwnd,GW_OWNER)}!=0{
                return Ok(());
            }
            unsafe{SetLastError(0);}
            let previous=unsafe{SetWindowLongPtrW(
                self.hwnd,GWLP_HWNDPARENT,hub
            )};
            let error=unsafe{GetLastError()};
            if previous==0 && error!=0{
                return Err(format!("카카오톡 소유 창 지정 불가 (Windows 오류 {error})"));
            }
            if unsafe{GetWindow(self.hwnd,GW_OWNER)}!=hub{
                // Never retain a half-applied foreign-window ownership edit.
                unsafe{SetWindowLongPtrW(self.hwnd,GWLP_HWNDPARENT,self.original_owner);}
                return Err("카카오톡 창의 소유 관계를 확인하지 못했습니다.".into());
            }
            self.owned_by_hub=hub;
            unsafe{SetWindowPos(self.hwnd,HWND_TOP,0,0,0,0,
                SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE|SWP_FRAMECHANGED);}
            Ok(())
        }
        fn detach_owner(&mut self)->Result<(),String>{
            if self.owned_by_hub==0 || !self.alive(){return Ok(());}
            // Kakao may have re-owned the window while connected. Never
            // overwrite an independent ownership change made by Kakao.
            if unsafe{GetWindow(self.hwnd,GW_OWNER)}!=self.owned_by_hub{
                self.owned_by_hub=0;
                return Ok(());
            }
            unsafe{SetLastError(0);}
            let previous=unsafe{SetWindowLongPtrW(
                self.hwnd,GWLP_HWNDPARENT,self.original_owner
            )};
            let error=unsafe{GetLastError()};
            if previous==0 && error!=0{
                return Err(format!("카카오톡 소유 관계 해제 불가 (Windows 오류 {error})"));
            }
            unsafe{SetWindowPos(self.hwnd,HWND_TOP,0,0,0,0,
                SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE|SWP_FRAMECHANGED);}
            if unsafe{GetWindow(self.hwnd,GW_OWNER)}!=self.original_owner{
                return Err("카카오톡 소유 관계 해제를 확인하지 못했습니다.".into());
            }
            self.owned_by_hub=0;
            Ok(())
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
        last_layout_key:Vec<(Hwnd,bool,bool)>,last_layout_epoch:u64,
        last_observed:Option<std::sync::Arc<windows::Snapshot>>,
        last_foreground:Hwnd,last_placed:Vec<(Hwnd,Rect)>,
        pub(super) overlap_footer:Option<i32>,
    }
    impl Session{
        pub(super) fn attach_independent_windows(&mut self){
            for window in &mut self.windows{
                if let Err(reason)=window.try_own(self.hub){
                    // Ownership can be denied by UIPI / another Kakao owner.
                    // Continue with the already-installed foreground fallback.
                    eprintln!("[BANDO HUB] 카카오톡 소유 창 건너뜀: {reason}");
                }
            }
        }
        // Capture original placements before restoring. Session::drop restores
        // the minimized state too if initial layout fails or is disconnected.
        pub(super) fn restore_minimized(&mut self)->Result<(),String>{
            for window in &self.windows {
                if window.alive() && unsafe{IsIconic(window.hwnd)}!=0 {
                    unsafe{ShowWindow(window.hwnd,9);}
                    if unsafe{IsIconic(window.hwnd)}!=0 {
                        return Err("최소화된 카카오톡 창을 복원하지 못했습니다. 카카오톡 창을 연 뒤 다시 연결하세요.".into());
                    }
                }
            }
            Ok(())
        }
        pub fn connect()->Result<Self,String>{
            let hub=hwnd_of_hub()?;
            // Explicit Connect must verify fresh identities even before the
            // background monitor has published its first snapshot.
            let found=windows::scan().windows;
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
                last_region:None,last_sync:None,last_layout_key:Vec::new(),
                last_layout_epoch:0,last_observed:None,last_foreground:0,last_placed:Vec::new(),overlap_footer:None,
            })
        }
        pub fn foreground_related(&self)->bool {
            let foreground=unsafe{GetForegroundWindow()};
            if foreground==0 || unsafe{IsIconic(self.hub)}!=0 {return false;}
            let owner_pid=pid(foreground);
            owner_pid==unsafe{GetCurrentProcessId()} || owner_pid==self.pid
        }
        /// Throttled fallback for unowned windows and conversation ordering.
        /// Native owner-linked windows require no per-click event hook.
        fn restore_relative_z_order(&mut self)->Result<(),String>{
            if !self.foreground_related(){return Ok(());}
            let foreground=unsafe{GetForegroundWindow()};
            if self.windows.iter().any(|w|!w.main && w.hwnd==foreground){
                self.active_chat=Some(foreground);
            }
            let windows=self.windows.iter().filter(|w|w.alive())
                .map(|w|DockedHandle{hwnd:w.hwnd,main:w.main})
                .collect::<Vec<_>>();
            restore_docked_order(self.hub,&windows,self.active_chat,foreground)
        }
        pub fn sync(&mut self,rect:egui::Rect,scale:f32)->Result<(),String>{
            if unsafe{IsWindow(self.hub)}==0 ||
                pid(self.hub)!=unsafe{GetCurrentProcessId()} {
                return Err("반도허브 창 연결이 끊어졌습니다.".into());
            }
            // No EnumWindows on the rendering thread while idle. The common
            // monitor publishes window membership and native move events.
            if unsafe{IsIconic(self.hub)}!=0{self.last_sync=None;return Ok(());}
            let region=physical_region(self.hub,rect,scale)?;
            let observed=windows::snapshot();
            let layout_key=self.last_observed.as_ref()
                .filter(|old|std::sync::Arc::ptr_eq(old,&observed))
                .map(|_|None).unwrap_or_else(||Some(observed.layout_key(self.pid)));
            let foreground=unsafe{GetForegroundWindow()};
            let fallback=self.last_sync.is_some_and(|t|t.elapsed()>=Duration::from_secs(5));
            let changed=self.last_sync.is_none() || self.last_region!=Some(region) ||
                layout_key.as_ref().is_some_and(|key|key!=&self.last_layout_key) ||
                self.last_layout_epoch!=observed.layout_epoch;
            // A slow safety check repairs missed native events, not a full
            // split layout calculation. Compare actual outer rectangles only.
            let drift=fallback && self.last_placed.iter().any(|(hwnd,expected)|
                outer(*hwnd).ok()!=Some(*expected));
            let lost=fallback && self.windows.iter().any(|w|!w.alive());
            self.last_observed=Some(observed.clone());
            if !changed && !drift && !lost{
                if fallback || foreground!=self.last_foreground{
                    self.last_foreground=foreground;
                    if fallback{self.last_sync=Some(Instant::now());}
                    self.restore_relative_z_order()?;
                }
                return Ok(());
            }
            self.last_region=Some(region);
            self.last_sync=Some(Instant::now());
            if let Some(key)=layout_key{self.last_layout_key=key;}
            self.last_layout_epoch=observed.layout_epoch;
            self.last_foreground=foreground;
            // Read the HUB monitor, never the already offscreen Kakao MAIN.
            let screen_bottom=monitor_screen_bottom(self.hub).unwrap_or(region.bottom);
            let discovered=&observed.windows;
            self.windows.retain(|w|w.alive());
            for c in discovered.iter().filter(|c|c.pid==self.pid){
                if !self.windows.iter().any(|w|w.hwnd==c.hwnd) &&
                    self.windows.len()<32 {
                    // A window may close between the worker observation and
                    // this UI frame. Ignore that stale candidate only.
                    if let Ok(window)=Tracked::new(c){
                        self.windows.push(window);
                        if !c.main {self.active_chat=Some(c.hwnd);}
                    }
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
            self.attach_independent_windows();
            let before=self.windows.iter().filter_map(|w|
                outer(w.hwnd).ok().map(|rect|(w.hwnd,rect))
            ).collect::<Vec<_>>();
            let base=self.windows.iter().find(|w|w.main && w.alive())
                .map(|w|w.hwnd);
            // The ad offset is measured before the initial Together layout
            // extends the MAIN HWND. Preserve it on Split/Together switches:
            // reading already extended offscreen DWM bounds is not stable.
            if self.overlap_footer.is_none() {
                self.overlap_footer=base.map(measure_footer_extension).transpose()?;
            }
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
                            // The split line (chat_top) was measured BEFORE
                            // this extension. Extending only by footer height
                            // below that split left the ad visible onscreen.
                            // Push the MAIN ad below the physical monitor
                            // while CHAT stays unchanged and on top.
                            if chats.is_empty(){
                                main_target
                            }else{
                                split_main_offscreen_target(
                                    main_target,screen_bottom,
                                    self.overlap_footer.unwrap_or(160)
                                )
                            }
                        }else{
                            overlap_main_target(region,self.overlap_footer.unwrap_or(160))
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
            self.last_placed=self.windows.iter().filter_map(|w|
                outer(w.hwnd).ok().map(|rect|(w.hwnd,rect))).collect();
            self.restore_relative_z_order()
        }
    }
    impl Drop for Session {
        fn drop(&mut self){
            // IMPORTANT: detach external Kakao windows before restoring
            // their placement and before eframe tears down the HUB HWND.
            for window in &mut self.windows{
                if let Err(reason)=window.detach_owner(){
                    eprintln!("[BANDO HUB] 카카오톡 소유 관계 복원 실패: {reason}");
                }
            }
            for window in self.windows.drain(..){
                window.restore();
            }
        }
    }
}
