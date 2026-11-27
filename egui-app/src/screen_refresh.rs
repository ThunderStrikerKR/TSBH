//! Shared asynchronous screen-refresh lifecycle.
//! UI snapshots remain owned by their views; the refresh coordinator owns
//! request ordering, freshness and the atomic publication boundary.
use crate::latest_job::LatestJob;
use eframe::egui::Context;

pub struct ReadRefresh<T> {
    job: LatestJob<T>,
    required: bool,
}
impl<T> Default for ReadRefresh<T> {
    fn default() -> Self { Self { job: LatestJob::default(), required: false } }
}
impl<T: Send + 'static> ReadRefresh<T> {
    pub fn pending(&self) -> bool { self.job.pending() }
    pub fn required(&self) -> bool { self.required }
    pub fn require(&mut self) { self.required = true; }
    pub fn verified(&mut self) { self.required = false; }
    pub fn cancel(&mut self) { self.job.invalidate(); }
    pub fn reset(&mut self) { self.cancel(); self.verified(); }

    /// Same-scope reads leave the previously verified view intact.
    pub fn submit(&mut self, ctx: Context, work: impl FnOnce()->Result<T,String>+Send+'static) {
        self.job.submit(ctx, work);
    }
    pub fn submit_required(&mut self, ctx: Context, work: impl FnOnce()->Result<T,String>+Send+'static) {
        self.require();
        self.submit(ctx, work);
    }
    /// Receiving an answer does NOT unlock writes: the caller must validate
    /// the revision/scope and publish the projection first.
    pub fn poll(&mut self) -> Option<Result<T,String>> { self.job.poll() }
    pub fn publish(&mut self, slot: &mut Option<T>, value: T) {
        *slot = Some(value);
        self.verified();
    }
    pub fn discard_advisory(&mut self) -> bool {
        if self.required || !self.pending() { return false; }
        self.cancel();
        true
    }
    #[cfg(test)]
    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<T,String>)>{
        self.require();
        self.job.geometry_begin_read()
    }
    #[cfg(test)]
    pub(crate) fn test_advisory_read(&mut self)->Box<dyn FnOnce(Result<T,String>)>{
        self.job.geometry_begin_read()
    }
}

/// Scope changes invalidate displayed query results, not ordinary refreshes.
pub fn retain_scope<T:Default>(slot:&mut T, same_scope:bool) {
    if !same_scope { *slot=T::default(); }
}

/// Native revision/CAS is still authoritative. Pending advisory reads alone
/// need not block writes, while a mandatory refresh or read failure must.
pub fn write_admitted(has_revision:bool, writing:bool, required:bool, failed:bool)->bool{
    has_revision && !writing && !required && !failed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_refresh_keeps_snapshot_until_verified_publication(){
        let mut refresh=ReadRefresh::default();
        let mut visible=Some(7);
        let finish=refresh.geometry_begin_read();
        assert!(refresh.required()&&refresh.pending());
        assert_eq!(visible,Some(7));
        finish(Ok(8));
        let value=refresh.poll().unwrap().unwrap();
        assert!(refresh.required());
        refresh.publish(&mut visible,value);
        assert_eq!(visible,Some(8));
        assert!(!refresh.required());
    }
    #[test]
    fn failure_keeps_snapshot_and_write_guard(){
        let mut refresh=ReadRefresh::<u64>::default();
        let visible=Some(7);
        refresh.geometry_begin_read()(Err("synthetic".into()));
        assert!(refresh.poll().unwrap().is_err());
        assert_eq!(visible,Some(7));
        assert!(!write_admitted(true,false,refresh.required(),true));
    }
    #[test]
    fn advisory_can_be_discarded_but_required_read_cannot(){
        let mut refresh=ReadRefresh::<u64>::default();
        let old=refresh.test_advisory_read();
        assert!(refresh.discard_advisory());
        let current=refresh.geometry_begin_read();
        assert!(!refresh.discard_advisory());
        old(Ok(1));
        assert!(refresh.poll().is_none());
        current(Ok(2));
        assert_eq!(refresh.poll(),Some(Ok(2)));
        assert!(refresh.required());
    }
    #[test]
    fn migrated_read_only_views_do_not_create_independent_workers(){
        // The retired manual product-search module has been replaced by the
        // ProductManager chooser. ProductManager's coordinator is checked by
        // the management-view contract below (it also has write workers).
        assert!(include_str!("manual_workspace.rs").contains(".show_selection_modal("));
        assert!(include_str!("product_manager.rs").contains("ReadRefresh"));
        for (name,source) in [
            ("client chooser",include_str!("client_chooser.rs")),
            ("item editor",include_str!("item_editor.rs")),
            ("review panel",include_str!("review_panel.rs")),
        ]{
            assert!(source.contains("screen_refresh::ReadRefresh"),"{name}: shared refresh missing");
            assert!(!source.contains(concat!("mpsc::","channel()")),"{name}: private read channel");
            assert!(!source.contains(concat!("thread::","spawn(")),"{name}: private read worker");
        }
    }
    #[test]
    fn management_views_do_not_bypass_refresh_coordinator(){
        for (name,source) in [
            ("reception",include_str!("main.rs")),
            ("history",include_str!("order_history_manager.rs")),
            ("settlement",include_str!("daily_settlement_manager.rs")),
            ("excel",include_str!("excel_manager.rs")),
            ("freight",include_str!("freight_manager.rs")),
            ("products",include_str!("product_manager.rs")),
            ("clients",include_str!("client_manager.rs")),
            ("settings",include_str!("settings_view.rs")),
            ("automatic messages",include_str!("automatic_message_manager.rs")),
            ("model groups",include_str!("model_group_manager.rs")),
            ("aliases",include_str!("alias_manager.rs")),
        ]{
            assert!(source.contains("ReadRefresh"),"{name}: shared refresh missing");
            assert!(!source.contains(concat!("latest_job::","LatestJob")),"{name}: direct low-level worker");
        }
    }
    #[test]
    fn changed_scope_only_clears_the_previous_result(){
        let mut value=Some(1);
        retain_scope(&mut value,true);
        assert_eq!(value,Some(1));
        retain_scope(&mut value,false);
        assert_eq!(value,None);
    }
}


/// A single bounded, presentation-only projection. The owner invalidates on
/// authoritative publication; key changes cover local view controls.
pub(crate) struct ViewProjectionCache<K,V>{
    entry:Option<(K,std::sync::Arc<V>)>,
}
impl<K,V> Default for ViewProjectionCache<K,V>{
    fn default()->Self{Self{entry:None}}
}
impl<K:PartialEq,V> ViewProjectionCache<K,V>{
    pub(crate) fn get(&mut self,key:K,build:impl FnOnce()->V)->std::sync::Arc<V>{
        if let Some((stored,value))=&self.entry{
            if *stored==key{return std::sync::Arc::clone(value);}
        }
        let value=std::sync::Arc::new(build());
        self.entry=Some((key,std::sync::Arc::clone(&value)));
        value
    }
    pub(crate) fn invalidate(&mut self){self.entry=None;}
}
#[cfg(test)]
#[test]
fn view_projection_reuses_frames_and_invalidates_on_publication(){
    let mut cache=ViewProjectionCache::default();
    let first=cache.get(1,||vec![1,2]);
    for _ in 0..1000{
        assert!(std::sync::Arc::ptr_eq(&first,&cache.get(1,||panic!("unchanged projection rebuilt"))));
    }
    assert_eq!(&*cache.get(2,||vec![2,1]),&[2,1]);
    cache.invalidate();
    assert_eq!(&*cache.get(2,||vec![3]),&[3]);
}
