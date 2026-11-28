//! Global explicit-open factory-reply notification center.
use crate::ui_layout::geometry::UiWidthPolicyExt as _;
use eframe::egui::{self,Context,Rect,Ui};
use serde_json::Value;
use std::{sync::mpsc::{self,Receiver},time::{Duration,Instant}};
use crate::{theme,typography};
#[derive(Default)]
pub(crate) struct Center{
    open:bool,items:Vec<Value>,error:Option<String>,
    pending:Option<Receiver<Result<Vec<Value>,String>>>,last:Option<Instant>,
    launch_warning:Option<Option<String>>,
}
enum Action{Load,Read(Option<String>),Dismiss(Option<String>)}
fn open_notification_db(write:bool)->Result<rusqlite::Connection,String>{
    use rusqlite::{Connection,OpenFlags};
    let path=if write{
        crate::business_state::authorized_business_write_path()?
    }else{
        if !crate::settings_store::data_service_available(){
            return Err("업무 데이터가 연결되지 않았습니다.".into());
        }
        std::env::var_os("BANDO_EGUI_BUSINESS_DB").map(std::path::PathBuf::from)
            .ok_or("업무 데이터가 연결되지 않았습니다.")?
    };
    let flags=if write{OpenFlags::SQLITE_OPEN_READ_WRITE}else{OpenFlags::SQLITE_OPEN_READ_ONLY};
    let conn=Connection::open_with_flags(path,flags).map_err(|e|e.to_string())?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    let version:i64=conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if version!=2{return Err("지원하지 않는 업무 데이터 버전".into());}
    Ok(conn)
}
impl Center{
    fn request(&mut self,ctx:&Context,action:Action){
        if self.pending.is_some()||cfg!(test){return;}
        self.last=Some(Instant::now());
        let (tx,rx)=mpsc::channel();self.pending=Some(rx);
        let ctx=ctx.clone();
        std::thread::spawn(move||{
            let result=(||{
                let conn=open_notification_db(!matches!(&action,Action::Load))?;
                use bando_data_access::notification_store as store;
                match action{
                    Action::Load=>{},
                    Action::Read(id)=>store::mark_read(&conn,id.as_deref())?,
                    Action::Dismiss(id)=>store::dismiss(&conn,id.as_deref())?,
                }
                store::load(&conn)
            })();
            let _=tx.send(result);ctx.request_repaint();
        });
    }
    pub fn poll(&mut self,ctx:&Context,changed:bool){
        if let Some(rx)=self.pending.as_ref(){
            match rx.try_recv(){
                Ok(result)=>{self.pending=None;match result{
                    Ok(items)=>{self.items=items;self.error=None;},
                    Err(error)=>self.error=Some(error),
                }},
                Err(mpsc::TryRecvError::Disconnected)=>{self.pending=None;self.error=Some("알림 조회 중단".into());},
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if !cfg!(test)&&self.launch_warning.is_none(){
            self.launch_warning=Some(crate::business_state::production_write_warning());
        }
        if !cfg!(test)&&crate::settings_store::data_service_available(){
            if changed||self.last.is_none_or(|t|t.elapsed()>=Duration::from_secs(5)){self.request(ctx,Action::Load);}
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }
    pub fn header_button(&mut self,ui:&mut Ui){
        let unread=self.items.iter().filter(|n|n["read"]!=true).count()
            + usize::from(self.launch_warning.as_ref().is_some_and(|warning|warning.is_some()));
        let response=crate::button::notification_button(ui,unread);
        if response.clicked(){self.open=true;self.request(ui.ctx(),Action::Load);}
    }
    pub fn show(&mut self,ctx:&Context,area:Rect){
        if !self.open{return;}
        let busy=self.pending.is_some();let mut close=false;let mut action=None;
        let writable=crate::business_state::authorized_business_write_path().is_ok();
        crate::popup::modal_backdrop(ctx,"notification-center-backdrop",false);
        crate::popup::modal_window(ctx,"알림","notification-center",area)
            .default_width(area.width().min(theme::MODAL_DECISION_MAX_WIDTH)).show(ctx,|ui|{
                crate::popup::modal_flow_workspace(ui,|workspace|{
                    let (closed,())=workspace.header_with_toolbar("알림","닫기",true,|toolbar|{
                        toolbar.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
                            if crate::button::action(bar,"전체 삭제",theme::ButtonRole::Standard,!busy&&writable&&!self.items.is_empty()).clicked(){action=Some(Action::Dismiss(None));}
                            if crate::button::action(bar,"모두 읽음",theme::ButtonRole::Standard,!busy&&writable&&!self.items.is_empty()).clicked(){action=Some(Action::Read(None));}
                            // One stable status slot; native elision is the only tooltip owner.
                            let error=self.error.as_deref().unwrap_or("");
                            bar.add(crate::ui_primitives::truncated_label(typography::caption(
                                if error.is_empty(){" "}else{error}
                            )));
                        });
                    });
                    close=closed;
                    let height=crate::popup::modal_body_height_budget(
                        area.height()-(crate::popup::modal_master_header_with_toolbar_height()-theme::MODAL_HEADER_HEIGHT),false);
                    workspace.scroll_region("notification-center-list",height,|ui|{
                        if let Some(Some(warning))=&self.launch_warning{
                            ui.label(typography::body(warning));
                        }
                        if self.items.is_empty()&&self.launch_warning.as_ref().is_none_or(|warning|warning.is_none()){
                            crate::ui_primitives::management_body_state_inline(ui,if busy{"조회 중"}else{"알림이 없습니다."});
                        }
                        ui.vertical(|list|{
                            crate::ui_layout::apply_spacing_role(list,crate::ui_layout::SpacingRole::WorkflowDetail);
                            for n in &self.items{
                                let text=|k:&str|n[k].as_str().unwrap_or("");
                                crate::card::section_frame().show(list,|card|{
                                    let width=card.available_width();
                                    card.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::FillAvailable,width);
                                    // The action column has a stable width for both read states.
                                    card.horizontal(|row|{
                                        row.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
                                            if crate::button::action(bar,"삭제",theme::ButtonRole::Standard,!busy&&writable).clicked(){action=Some(Action::Dismiss(Some(text("id").to_owned())));}
                                            if crate::button::action_with_min_width(bar,if n["read"]==true{"읽음 완료"}else{"읽음"},theme::ButtonRole::Standard,!busy&&writable&&n["read"]!=true,theme::NOTIFICATION_READ_ACTION_WIDTH).clicked(){action=Some(Action::Read(Some(text("id").to_owned())));}
                                            bar.add(crate::ui_primitives::truncated_label(typography::field_label(text("title"))));
                                        });
                                    });
                                    card.label(typography::caption(text("at")));
                                    crate::card::preview_surface_frame().show(card,|detail|{
                                        let width=detail.available_width();
                                        detail.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::FillAvailable,width);
                                        detail.add(egui::Label::new(typography::body(text("detail"))).wrap());
                                    });
                                });
                            }
                        });
                    });
                });
            });
        if close{self.open=false;}
        if let Some(action)=action{self.request(ctx,action);}
    }
}



#[cfg(test)]
mod shared_notification_center_screen_geometry_tests{
    use super::*;
    use crate::ui_test_support::{test_context,begin_component_geometry,finish_component_geometry,assert_component_roles};
    #[test]
    fn actual_notification_popup_keeps_rects_during_read_and_error_changes(){
        use crate::ui_test_support::assert_popup_transition_geometry;
        let ctx=test_context();
        let mut center=Center{open:true,..Default::default()};
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1280.0,800.0));
        center.items=vec![serde_json::json!({
            "id":"synthetic-notice","title":"검증 알림","detail":"합성 변경 내역",
            "at":"2026-11-27","read":false
        })];
        assert_popup_transition_geometry(&ctx,screen,4,|ctx,phase|{
            center.items[0]["read"]=serde_json::json!(phase==1);
            center.error=(phase==2).then(||"검증 오류".into());
            center.show(ctx,screen);
        });
    }
    #[test]
    fn actual_center_uses_shared_popup_and_buttons_in_empty_unread_and_read_states(){
        let ctx=test_context();
        let mut center=Center{open:true,..Default::default()};
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1280.0,800.0));
        for read in [None,Some(false),Some(true),None]{
            center.items=read.map(|value|vec![serde_json::json!({
                "id":"synthetic-notice","title":"검증 알림","detail":"합성 변경 내역",
                "at":"2026-11-27","read":value
            })]).unwrap_or_default();
            for frame in 0..6{
                begin_component_geometry();
                let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},
                    |ctx|center.show(ctx,screen));
                let components=finish_component_geometry(screen);
                if frame==5{
                    assert_component_roles(&components,&["popup","button"]);
                    assert!(center.open);
                }
            }
        }
    }
}
