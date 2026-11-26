//! Shared, transient presentation state. Never persists operational drafts.
//! Page/tab epochs, bounded UI caches and stable selection/focus helpers use
//! one idle-expiration policy. Renderers still own geometry and edit admission.
use eframe::egui::{self,Context,Id,Response};
use std::{collections::{HashMap,HashSet},time::{Duration,Instant}};

pub(crate) fn is_fresh(touched:Instant,now:Instant)->bool{
    now.saturating_duration_since(touched)<crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL
}

/// Scroll and popup caches share the same idle clock; active entries are
/// renewed on the next frame, even if the application slept while visible.
pub(crate) trait TimedEntry {
    fn touched(&self)->Instant;
    fn touch(&mut self,now:Instant);
}
impl<T> TimedEntry for (T,Instant){
    fn touched(&self)->Instant{self.1}
    fn touch(&mut self,now:Instant){self.1=now;}
}
#[derive(Clone)]
pub(crate) struct TimedViewStore<T>{
    pub(crate) entries:HashMap<Id,T>,
    pub(crate) frame:Option<u64>,
    pub(crate) seen:HashSet<Id>,
    last_sweep:Option<Instant>,
}
impl<T> Default for TimedViewStore<T>{
    fn default()->Self{Self{entries:HashMap::new(),frame:None,seen:HashSet::new(),last_sweep:None}}
}
impl<T:TimedEntry> TimedViewStore<T>{
    pub(crate) fn advance_frame(&mut self,frame:u64,now:Instant){
        if self.frame==Some(frame){return;}
        for id in self.seen.drain(){
            if let Some(entry)=self.entries.get_mut(&id){entry.touch(now);}
        }
        self.frame=Some(frame);
    }
    pub(crate) fn sweep(&mut self,now:Instant)->Vec<T>{
        let expired=self.entries.iter().filter_map(|(id,entry)|
            (!is_fresh(entry.touched(),now)).then_some(*id)).collect::<Vec<_>>();
        self.last_sweep=Some(now);
        expired.into_iter().filter_map(|id|self.entries.remove(&id)).collect()
    }
    pub(crate) fn sweep_due(&mut self,now:Instant)->Vec<T>{
        if self.last_sweep.is_some_and(|last|now.saturating_duration_since(last)<Duration::from_secs(60)){
            return Vec::new();
        }
        self.sweep(now)
    }
    pub(crate) fn take(&mut self,id:Id,now:Instant)->Option<T>{
        self.entries.remove(&id).filter(|entry|is_fresh(entry.touched(),now))
    }
    pub(crate) fn put(&mut self,id:Id,entry:T){
        self.seen.insert(id);
        self.entries.insert(id,entry);
    }
}

/// Fixed-size per-tab presentation slots, with no unbounded cache.
pub(crate) fn exchange_tab<T:Default,const N:usize>(
    slots:&mut [T;N],current:&mut T,previous:usize,next:usize
){
    if previous==next{return;}
    slots[previous]=std::mem::take(current);
    *current=std::mem::take(&mut slots[next]);
}
pub(crate) fn retain_selection<K:Ord>(selected:&mut std::collections::BTreeSet<K>,
    valid:&std::collections::BTreeSet<K>,verified:bool){
    if verified{selected.retain(|id|valid.contains(id));}
}
pub(crate) fn retain_target<K>(selected:&mut Option<K>,valid:impl FnOnce(&K)->bool,verified:bool){
    if verified&&selected.as_ref().is_some_and(|id|!valid(id)){*selected=None;}
}

/// Only explicitly opted-in non-mutating search controls can regain focus.
/// Business editors and modal confirmation buttons must never be auto-opened.
#[derive(Clone,Default)]
struct SearchFocus{frame:u64,focused:bool}
type FocusStore=TimedViewStore<(SearchFocus,Instant)>;
fn focus_store_id()->Id{Id::new("shared-view-search-focus")}
pub(crate) fn sweep_focus(ctx:&Context,now:Instant){
    let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data|{
        let store=data.get_temp_mut_or_default::<FocusStore>(focus_store_id());
        store.advance_frame(frame,now);
        store.sweep_due(now);
    });
}
pub(crate) fn remember_search_focus(response:&Response){
    let ctx=&response.ctx;
    let now=Instant::now();
    let frame=ctx.cumulative_frame_nr();
    sweep_focus(ctx,now);
    let previous=ctx.data_mut(|data|
        data.get_temp_mut_or_default::<FocusStore>(focus_store_id()).take(response.id,now));
    let restore=previous.as_ref().is_some_and(|(old,_)|
        old.focused&&frame.saturating_sub(old.frame)>1);
    if restore && response.enabled() && !egui::Popup::is_any_open(ctx)
        && crate::overlay::modal_scope(ctx).is_none()
        && !ctx.input(|input|input.pointer.any_pressed())
        && ctx.memory(|memory|memory.focused().is_none()){
        response.request_focus();
    }
    ctx.data_mut(|data|{
        data.get_temp_mut_or_default::<FocusStore>(focus_store_id()).put(
            response.id,(SearchFocus{frame,focused:response.has_focus()},now));
    });
}

// Presentation-only state lifetime. Expiration is checked on re-entry;
// no timer task, persistence, or business mutation is needed.
pub(crate) struct NavigationSnapshots<const N:usize>{
    active:usize,
    paused:bool,
    left:[Option<std::time::Instant>;N],
    epochs:[u64;N],
}
impl<const N:usize> NavigationSnapshots<N>{
    pub(crate) fn new(active:usize)->Self{Self{active,paused:false,left:[None;N],epochs:[0;N]}}
    pub(crate) fn visit(&mut self,next:usize,now:std::time::Instant)->bool{
        if next==self.active&&!self.paused{return false;}
        if !self.paused{self.left[self.active]=Some(now);}
        self.active=next;
        self.paused=false;
        let expired=self.left[next].take().is_some_and(|left|
            !is_fresh(left,now));
        if expired{self.epochs[next]=self.epochs[next].wrapping_add(1);}
        expired
    }
    pub(crate) fn active(&self)->usize{self.active}
    pub(crate) fn suspend(&mut self,now:std::time::Instant){
        if !self.paused{self.left[self.active]=Some(now);self.paused=true;}
    }
    pub(crate) fn epoch(&self,index:usize)->u64{self.epochs[index]}
}
#[cfg(test)]
mod navigation_snapshot_tests{
    use super::*;
    #[test]
    fn nested_tabs_expire_independently_and_suspend_is_idempotent(){
        let t=std::time::Instant::now();
        let sec=std::time::Duration::from_secs;
        let mut tabs=NavigationSnapshots::<3>::new(0);
        assert!(!tabs.visit(1,t));
        tabs.suspend(t+sec(100));
        tabs.suspend(t+sec(1700));
        assert!(!tabs.visit(1,t+sec(1800)));
        assert!(tabs.visit(0,t+sec(1801)));
        assert_eq!(tabs.epoch(0),1);
        assert_eq!(tabs.epoch(1),0);
        tabs.suspend(t+sec(1802));
        assert!(tabs.visit(0,t+sec(3602)));
        assert_eq!(tabs.epoch(0),2);
    }
    #[test]
    fn repeated_page_and_tab_expiry_does_not_grow_fixed_slots(){
        let t=std::time::Instant::now();
        let ttl=crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL;
        let mut pages=NavigationSnapshots::<8>::new(0);
        let mut tabs=NavigationSnapshots::<5>::new(0);
        let bytes=std::mem::size_of_val(&pages)+std::mem::size_of_val(&tabs);
        for cycle in 0..4096_u32{
            let now=t+ttl*(cycle*2);
            pages.visit(1,now);
            tabs.suspend(now);
            assert!(pages.visit(0,now+ttl));
            assert!(tabs.visit(0,now+ttl));
            assert_eq!(pages.epoch(0),u64::from(cycle)+1);
            assert_eq!(tabs.epoch(0),u64::from(cycle)+1);
            assert_eq!(std::mem::size_of_val(&pages)+std::mem::size_of_val(&tabs),bytes);
        }
    }
    #[test]
    fn expiry_is_per_page_and_starts_on_departure(){
        let t=std::time::Instant::now();
        let seconds=std::time::Duration::from_secs;
        let mut cache=NavigationSnapshots::<8>::new(0);
        assert!(!cache.visit(0,t+seconds(7200)));
        assert!(!cache.visit(1,t+seconds(7200)));
        assert!(!cache.visit(0,t+seconds(8999)));
        assert_eq!(cache.epoch(0),0);
        assert!(!cache.visit(2,t+seconds(9000)));
        assert!(cache.visit(0,t+seconds(10800)));
        assert_eq!(cache.epoch(0),1);
        assert!(cache.visit(1,t+seconds(10801)));
        assert_eq!(cache.epoch(1),1);
        assert!(cache.visit(2,t+seconds(10802)));
        assert_eq!(cache.epoch(2),1);
    }
}

#[cfg(test)]
mod view_state_tests{
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn selection_survives_unverified_refresh_and_reconciles_verified_ids(){
        let mut selected=BTreeSet::from(["a","b"]);
        let mut target=Some("b");
        let valid=BTreeSet::from(["a"]);
        retain_selection(&mut selected,&valid,false);
        retain_target(&mut target,|id|valid.contains(id),false);
        assert_eq!(selected,BTreeSet::from(["a","b"]));
        assert_eq!(target,Some("b"));
        retain_selection(&mut selected,&valid,true);
        retain_target(&mut target,|id|valid.contains(id),true);
        assert_eq!(selected,valid);
        assert_eq!(target,None);
    }
    #[test]
    fn tab_slots_keep_independent_values(){
        let mut slots=[String::new(),String::new()];
        let mut current="factory".to_owned();
        exchange_tab(&mut slots,&mut current,0,1);
        assert!(current.is_empty());
        current="store".into();
        exchange_tab(&mut slots,&mut current,1,0);
        assert_eq!(current,"factory");
        exchange_tab(&mut slots,&mut current,0,1);
        assert_eq!(current,"store");
    }
    #[test]
    fn timed_cache_keeps_visible_entries_and_expires_hidden_entries(){
        let start=Instant::now();
        let ttl=crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL;
        let visible=Id::new("visible");
        let hidden=Id::new("hidden");
        let mut store=TimedViewStore::default();
        store.advance_frame(1,start);
        store.put(visible,(1,start));
        store.entries.insert(hidden,(2,start));
        store.advance_frame(2,start+ttl);
        assert_eq!(store.sweep(start+ttl),vec![(2,start)]);
        assert_eq!(store.take(visible,start+ttl),Some((1,start+ttl)));
    }
}
