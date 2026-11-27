//! Shared egui overlay lifetime and input-priority policy.
//! Geometry and business actions remain owned by their existing components.
use eframe::egui::{self,Context,Id,LayerId,Order,Response,Ui};
use std::collections::{HashMap,HashSet};

#[derive(Clone,Default)]
struct FrameScope {
    pass:u64,
    page:Option<&'static str>,
    page_epoch:u64,
    modals:Vec<LayerId>,
    popup_owners:HashMap<LayerId,LayerId>,
    seen_popups:HashSet<LayerId>,
    popup_at_start:bool,
    popup_trigger:Option<(egui::Rect,Option<LayerId>)>,
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
            state=FrameScope{pass,popup_at_start,page:state.page,page_epoch:state.page_epoch,
                popup_owners:std::mem::take(&mut state.popup_owners),..Default::default()};
        }
        let out=f(&mut state);
        data.insert_temp(id,state);
        out
    })
}
/// Snapshot popup ownership BEFORE any render path can consume Escape.
pub fn begin_frame(ctx:&Context){with_scope(ctx,|_|());}
pub fn register_modal(ctx:&Context,layer:LayerId){
    with_scope(ctx,|state|{
        state.modals.retain(|old|*old!=layer);
        state.modals.push(layer);
    });
}
pub fn modal_scope(ctx:&Context)->Option<LayerId>{
    with_scope(ctx,|state|state.modals.last().copied())
}
/// Page transitions explicitly retire native popup memory, including a
/// menu whose trigger was virtualized away before the next render.
pub fn begin_page(ctx:&Context,page:&'static str)->bool{
    let changed=with_scope(ctx,|s|{
        let changed=s.page.is_some_and(|old|old!=page);
        s.page=Some(page);
        if changed{s.page_epoch=s.page_epoch.wrapping_add(1);s.popup_owners.clear();s.seen_popups.clear();s.modals.clear();}
        changed
    });
    if changed{egui::Popup::close_all(ctx);}
    changed
}
pub fn page_epoch(ctx:&Context)->u64{with_scope(ctx,|s|s.page_epoch)}
fn owner_in_scope(s:&FrameScope,mut layer:LayerId)->Option<LayerId>{
    for _ in 0..=s.popup_owners.len(){
        if s.modals.contains(&layer){return Some(layer);}
        let Some(parent)=s.popup_owners.get(&layer)else{return None;};
        if *parent==layer{return None;}
        layer=*parent;
    }
    None
}
/// Use the actual widget layer, not the last modal rendered.
pub fn response_owner(response:&Response)->Option<LayerId>{
    with_scope(&response.ctx,|s|owner_in_scope(s,response.layer_id))
}
fn record_popup(owner:&Response,popup:&Ui){
    let child=popup.layer_id();
    with_scope(popup.ctx(),|s|{
        s.popup_owners.insert(child,owner.layer_id);
        s.seen_popups.insert(child);
    });
    attach_popup(popup.ctx(),owner.layer_id,child);
    let origin=(owner.rect,response_owner(owner));
    with_scope(&owner.ctx,|s|s.popup_trigger=Some(origin));
}
/// Origin of the currently rendered popup; notification consumers may read it.
pub(crate) fn popup_trigger_origin(ctx:&Context)->Option<(egui::Rect,Option<LayerId>)>{
    with_scope(ctx,|s|s.popup_trigger)
}
pub(crate) fn clear_popup_trigger(ctx:&Context){with_scope(ctx,|s|s.popup_trigger=None);}
#[cfg(test)]
pub(crate) fn set_popup_trigger_for_test(ctx:&Context,rect:egui::Rect){
    let owner=modal_scope(ctx);
    with_scope(ctx,|s|s.popup_trigger=Some((rect,owner)));
}
/// egui owns native outside-click and option-close behavior; all callers
/// use the same owner registration and close policy.
pub fn show_menu<R>(owner:&Response,align:Option<egui::RectAlign>,
    content:impl FnOnce(&mut Ui)->R)->Option<egui::InnerResponse<R>>{
    if !owner.enabled(){
        egui::Popup::close_id(&owner.ctx,egui::Popup::default_response_id(owner));
        return None;
    }
    let popup=egui::Popup::menu(owner)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let popup=if let Some(align)=align{popup.align(align)}else{popup};
    popup.show(|ui|{record_popup(owner,ui);content(ui)})
}
pub fn show_context_menu(owner:&Response,content:impl FnOnce(&mut Ui)){
    if !owner.enabled(){
        egui::Popup::close_id(&owner.ctx,egui::Popup::default_response_id(owner));
        return;
    }
    egui::Popup::context_menu(owner).show(|ui|{record_popup(owner,ui);content(ui);});
}
/// Close only orphaned native popup IDs, never a different live menu.
pub fn finish_frame(ctx:&Context){
    let stale=with_scope(ctx,|s|{
        let active=s.modals.last().copied();
        let stale:Vec<_>=s.popup_owners.iter().filter_map(|(popup,parent)|
            (!s.seen_popups.contains(popup)||owner_in_scope(s,*parent)!=active)
                .then_some(*popup)).collect();
        for popup in &stale{s.popup_owners.remove(popup);}
        stale
    });
    for popup in stale{egui::Popup::close_id(ctx,popup.id);}
}
pub enum AreaRole{Backdrop{nested:bool},Preview,Notice,Help}
pub fn area(id:Id,role:AreaRole)->egui::Area{
    let order=match role{
        AreaRole::Backdrop{nested:false}=>Order::Middle,
        AreaRole::Notice=>Order::Tooltip,
        _=>Order::Foreground,
    };
    egui::Area::new(id).order(order).movable(false)
}
pub fn activate_modal(ctx:&Context,layer:LayerId,raise:bool){
    register_modal(ctx,layer);
    ctx.memory_mut(|memory|{
        if raise{memory.areas_mut().move_to_top(layer);}
        memory.set_modal_layer(layer);
    });
}
pub fn window(ctx:&Context,title:impl Into<egui::WidgetText>,id:Id)->egui::Window<'static>{
    register_modal(ctx,LayerId::new(Order::Foreground,id));
    egui::Window::new(title).id(id).order(Order::Foreground)
}
pub fn drag_painter(ctx:&Context,owner:LayerId,id:Id)->Option<egui::Painter>{
    (modal_scope(ctx)==Some(owner)&&!egui::Popup::is_any_open(ctx))
        .then(||ctx.layer_painter(LayerId::new(Order::Tooltip,id)))
}
pub(crate) fn page_drag_painter(ctx:&Context,owner:LayerId,id:Id)->Option<egui::Painter>{
    (modal_scope(ctx).is_none()&&!egui::Popup::is_any_open(ctx)
        &&ctx.memory(|m|m.allows_interaction(owner)))
        .then(||ctx.layer_painter(LayerId::new(Order::Tooltip,id)))
}
pub fn tooltip_allowed(response:&Response)->bool{
    let ctx=&response.ctx;
    let popup_layer=with_scope(ctx,|s|s.seen_popups.contains(&response.layer_id));
    (!egui::Popup::is_any_open(ctx)||popup_layer)
        && modal_scope(ctx).is_none_or(|active|response_owner(response)==Some(active))
        && ctx.memory(|m|m.allows_interaction(response.layer_id))
}
/// A popup owns keyboard shortcuts for the entire pass, including the
/// pass in which it is dismissed. Do not mutate its underlying editor.
pub fn popup_blocks_shortcuts(ctx:&Context)->bool{
    with_scope(ctx,|s|s.popup_at_start)||egui::Popup::is_any_open(ctx)
}
pub fn cell_escape_requested(ctx:&Context)->bool{
    ctx.input(|input|input.key_pressed(egui::Key::Escape))
        && with_scope(ctx,|s|!s.popup_at_start&&!s.escape_claimed)
}
pub fn take_cell_escape(response:&Response,was_focused:bool)->bool{
    if !(response.has_focus()||response.lost_focus()||was_focused)
        || !response.enabled()
        || !response.ctx.memory(|m|m.allows_interaction(response.layer_id)){
        return false;
    }
    take_modal_escape(&response.ctx)
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
    fn popup_owner_is_resolved_from_the_actual_layer_and_orphans_close(){
        let ctx=Context::default();
        let root=LayerId::new(Order::Foreground,Id::new("root"));
        let child=LayerId::new(Order::Foreground,Id::new("child"));
        let popup=LayerId::new(Order::Foreground,Id::new("popup"));
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            begin_page(ctx,"Products");
            register_modal(ctx,root);
            with_scope(ctx,|s|{
                s.popup_owners.insert(popup,root);
                s.seen_popups.insert(popup);
                assert_eq!(owner_in_scope(s,popup),Some(root));
            });
            egui::Popup::open_id(ctx,popup.id);
            register_modal(ctx,child);
            finish_frame(ctx);
            assert!(!egui::Popup::is_id_open(ctx,popup.id));
        });
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::Popup::open_id(ctx,popup.id);
            let old_epoch=page_epoch(ctx);
            assert!(begin_page(ctx,"History"));
            assert_eq!(page_epoch(ctx),old_epoch.wrapping_add(1));
            assert!(!begin_page(ctx,"History"));
            assert_eq!(page_epoch(ctx),old_epoch.wrapping_add(1));
            assert!(!egui::Popup::is_id_open(ctx,popup.id));
        });
    }
    #[test]
    fn popup_escape_does_not_also_close_parent(){
        let ctx=Context::default();
        let _=ctx.run(escape_input(),|ctx|{
            egui::Popup::open_id(ctx,Id::new("test-popup"));
            begin_frame(ctx);
            assert!(!notice_visible(ctx,None));
            assert!(popup_blocks_shortcuts(ctx));
            egui::Popup::close_all(ctx);
            assert!(popup_blocks_shortcuts(ctx));
            assert!(!take_modal_escape(ctx));
        });
    }
}
