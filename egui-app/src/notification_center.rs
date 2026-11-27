//! Global explicit-open factory-reply notification center.
use eframe::egui::{self,Context,Rect,Ui};
use serde_json::Value;
use std::{sync::mpsc::{self,Receiver},time::{Duration,Instant}};
use crate::{theme,typography};
#[derive(Default)]
pub(crate) struct Center{
    open:bool,items:Vec<Value>,error:Option<String>,
    pending:Option<Receiver<Result<Vec<Value>,String>>>,last:Option<Instant>,
}
enum Action{Load,Read(Option<String>),Dismiss(Option<String>)}
impl Center{
    fn request(&mut self,ctx:&Context,action:Action){
        if self.pending.is_some()||cfg!(test){return;}
        self.last=Some(Instant::now());
        let (tx,rx)=mpsc::channel();self.pending=Some(rx);
        let ctx=ctx.clone();
        std::thread::spawn(move||{
            let result=(||{
                let (conn,_)=crate::factory_reply_inbox::open_db()?;
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
        if !cfg!(test)&&std::env::var("BANDO_EGUI_ENABLE_EXCEL_WRITE").ok().as_deref()==Some("1"){
            if changed||self.last.is_none_or(|t|t.elapsed()>=Duration::from_secs(5)){self.request(ctx,Action::Load);}
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }
    pub fn header_button(&mut self,ui:&mut Ui){
        let unread=self.items.iter().filter(|n|n["read"]!=true).count();
        let response=crate::button::notification_button(ui,unread);
        if response.clicked(){self.open=true;self.request(ui.ctx(),Action::Load);}
    }
    pub fn show(&mut self,ctx:&Context,area:Rect){
        if !self.open{return;}
        let busy=self.pending.is_some();let mut close=false;let mut action=None;
        crate::popup::modal_backdrop(ctx,"notification-center-backdrop",false);
        crate::popup::modal_window(ctx,"알림","notification-center",area)
            .default_width(area.width().min(theme::MODAL_DECISION_MAX_WIDTH)).show(ctx,|ui|{
                crate::popup::modal_flow_workspace(ui,|workspace|{
                    let (closed,())=workspace.header_with_toolbar("알림","닫기",true,|toolbar|{
                        toolbar.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
                            if crate::button::action(bar,"전체 삭제",theme::ButtonRole::Standard,!busy&&!self.items.is_empty()).clicked(){action=Some(Action::Dismiss(None));}
                            if crate::button::action(bar,"모두 읽음",theme::ButtonRole::Standard,!busy&&!self.items.is_empty()).clicked(){action=Some(Action::Read(None));}
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
                        if self.items.is_empty(){crate::ui_primitives::management_body_state_inline(ui,if busy{"조회 중"}else{"알림이 없습니다."});}
                        ui.vertical(|list|{
                            crate::ui_layout::apply_spacing_role(list,crate::ui_layout::SpacingRole::WorkflowDetail);
                            for n in &self.items{
                                let text=|k:&str|n[k].as_str().unwrap_or("");
                                crate::card::section_frame().show(list,|card|{
                                    let width=card.available_width();
                                    crate::card::apply_width_policy(card,crate::card::CardWidthPolicy::FillAvailable,width);
                                    // The action column has a stable width for both read states.
                                    card.horizontal(|row|{
                                        row.with_layout(egui::Layout::right_to_left(egui::Align::Center),|bar|{
                                            if crate::button::action(bar,"삭제",theme::ButtonRole::Standard,!busy).clicked(){action=Some(Action::Dismiss(Some(text("id").to_owned())));}
                                            if crate::button::action_with_min_width(bar,if n["read"]==true{"읽음 완료"}else{"읽음"},theme::ButtonRole::Standard,!busy&&n["read"]!=true,theme::NOTIFICATION_READ_ACTION_WIDTH).clicked(){action=Some(Action::Read(Some(text("id").to_owned())));}
                                            bar.add(crate::ui_primitives::truncated_label(typography::field_label(text("title"))));
                                        });
                                    });
                                    card.label(typography::caption(text("at")));
                                    crate::card::preview_surface_frame().show(card,|detail|{
                                        let width=detail.available_width();
                                        crate::card::apply_width_policy(detail,crate::card::CardWidthPolicy::FillAvailable,width);
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
