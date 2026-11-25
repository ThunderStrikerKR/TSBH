//! Shared egui overlay lifetime and input-priority policy.
//! Geometry and business actions remain owned by their existing components.
use eframe::egui::{self,Context,Id,LayerId,Order};

#[derive(Clone,Default)]
struct FrameScope {
    pass:u64,
    modal:Option<LayerId>,
    popup_at_start:bool,
    escape_claimed:bool,
}
fn with_scope<R>(ctx:&Context,f:impl FnOnce(&mut FrameScope)->R)->R{
    let pass=ctx.cumulative_pass_nr();
    let popup_at_start=egui::Popup::is_any_open(ctx);
    ctx.data_mut(|data|{
        let id=Id::new("bando-overlay-frame-scope");
        let mut state=data.get_temp::<FrameScope>(id).unwrap_or_else(||FrameScope{
            pass,popup_at_start,..Default::default()
        });
        if state.pass!=pass{
            state=FrameScope{pass,popup_at_start,..Default::default()};
        }
        let out=f(&mut state);
        data.insert_temp(id,state);
        out
    })
}
/// Snapshot popup ownership BEFORE any render path can consume Escape.
pub fn begin_frame(ctx:&Context){with_scope(ctx,|_|());}
pub fn register_modal(ctx:&Context,layer:LayerId){
    with_scope(ctx,|state|state.modal=Some(layer));
}
pub fn modal_scope(ctx:&Context)->Option<LayerId>{
    with_scope(ctx,|state|state.modal)
}
/// Same-order popups stay immediately above their own window, not above
/// a subsequently opened modal. Lower-order owners need no sublayer.
pub fn attach_popup(ctx:&Context,parent:LayerId,child:LayerId){
    if parent.order==Order::Foreground && child.order==parent.order && parent!=child{
        ctx.set_sublayer(parent,child);
    }
}
/// Both card preview variants belong to the background page. Include the
/// previous pass's modal input boundary, used by egui hit testing.
pub fn background_preview_allowed(ctx:&Context)->bool{
    modal_scope(ctx).is_none()
        && ctx.memory(|memory|memory.top_modal_layer().is_none())
        && !egui::Popup::is_any_open(ctx)
}
/// A parent notice may reappear after a child closes; an operation may
/// report after its own dialog closes. Never cover another active modal
/// or an open selection menu.
pub fn notice_visible(ctx:&Context,owner:Option<LayerId>)->bool{
    !egui::Popup::is_any_open(ctx)
        && modal_scope(ctx).is_none_or(|active|owner==Some(active))
}
/// An open native popup handles Escape first, even if it closes during
/// this pass. Otherwise only one eligible modal handler may claim it.
pub fn take_modal_escape(ctx:&Context)->bool{
    if !ctx.input(|input|input.key_pressed(egui::Key::Escape)){return false;}
    with_scope(ctx,|state|{
        if state.popup_at_start || state.escape_claimed{return false;}
        state.escape_claimed=true;
        true
    })
}

#[cfg(test)]
mod modal_input_routing_tests{
    use super::*;
    fn escape_input()->egui::RawInput{
        let mut raw=egui::RawInput::default();
        raw.events.push(egui::Event::Key{key:egui::Key::Escape,physical_key:None,
            pressed:true,repeat:false,modifiers:egui::Modifiers::NONE});
        raw
    }
    #[test]
    fn notices_and_background_previews_respect_modal_ownership(){
        let ctx=Context::default();
        let root=LayerId::new(Order::Foreground,Id::new("root"));
        let child=LayerId::new(Order::Foreground,Id::new("child"));
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            begin_frame(ctx);
            assert!(background_preview_allowed(ctx));
            register_modal(ctx,root);
            assert!(!background_preview_allowed(ctx));
            assert!(notice_visible(ctx,Some(root)));
            assert!(!notice_visible(ctx,None));
            register_modal(ctx,child);
            assert!(!notice_visible(ctx,Some(root)));
            assert!(notice_visible(ctx,Some(child)));
        });
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            begin_frame(ctx);
            assert!(notice_visible(ctx,Some(root)));
        });
    }
    #[test]
    fn escape_is_claimed_only_once_per_pass(){
        let ctx=Context::default();
        let _=ctx.run(escape_input(),|ctx|{
            begin_frame(ctx);
            assert!(take_modal_escape(ctx));
            assert!(!take_modal_escape(ctx));
        });
    }
    #[test]
    fn popup_escape_does_not_also_close_parent(){
        let ctx=Context::default();
        let _=ctx.run(escape_input(),|ctx|{
            egui::Popup::open_id(ctx,Id::new("test-popup"));
            begin_frame(ctx);
            assert!(!notice_visible(ctx,None));
            egui::Popup::close_all(ctx);
            assert!(!take_modal_escape(ctx));
        });
    }
}
