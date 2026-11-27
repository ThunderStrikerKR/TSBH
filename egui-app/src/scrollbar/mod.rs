//! Native scroll construction, rail style and common viewport height policy.
use eframe::egui::{self,Ui};
use crate::theme::*;
/// Common native scroll construction. Screen-specific IDs, bounds and shrink
/// behavior are applied by the caller; shared role policies build on this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollAxes { Vertical, Horizontal, Both }

pub(crate) fn scroll_area(axes:ScrollAxes)->egui::ScrollArea{
    match axes {
        ScrollAxes::Vertical=>egui::ScrollArea::vertical(),
        ScrollAxes::Horizontal=>egui::ScrollArea::horizontal(),
        ScrollAxes::Both=>egui::ScrollArea::both(),
    }
}

// Native scrollbars inherit the shared theme widget-state palette. Only rail
// geometry varies by semantic role; do not recolor descendant widgets.
pub(crate) use crate::theme::ScrollRailRole as ManagementScrollRole;

fn apply_rail_geometry(style:&mut egui::style::ScrollStyle,role:ManagementScrollRole){
    let geometry=scroll_rail_geometry(role);
    style.bar_width=geometry.width;
    style.handle_min_length=geometry.minimum_handle;
    style.bar_inner_margin=geometry.inner_margin;
    style.bar_outer_margin=geometry.outer_margin;
    style.foreground_color=true;
}

pub(crate) fn management_scroll_style() -> egui::style::ScrollStyle {
    let mut style=egui::style::ScrollStyle::solid();
    apply_rail_geometry(&mut style,ManagementScrollRole::Standard);
    style
}

/// Tables keep the same solid rail and handle dimensions, without independent
/// inner/outer margins. This preserves the pinned header and body boundary.
pub(crate) fn management_table_scroll_style()->egui::style::ScrollStyle{
    let mut style=management_scroll_style();
    apply_rail_geometry(&mut style,ManagementScrollRole::Table);
    style
}

pub(crate) fn apply_management_scroll_role(ui:&mut Ui,role:ManagementScrollRole){
    ui.spacing_mut().scroll=match role{
        ManagementScrollRole::Standard=>management_scroll_style(),
        ManagementScrollRole::Table=>management_table_scroll_style(),
    };
}

/// CONFLICT-004=A: use the remaining inner height of a SINGLE-list card,
/// not a fixed fraction of the whole application's content rectangle.
/// Measure AFTER toolbars, notices and the pinned header have been laid out.
/// `reserve_below` protects a footer, an empty-state message or other
/// controls that the screen must still render underneath its scroll area.
/// Never use this for multi-table reports or adjacent editor panels.
pub(crate) fn management_single_list_scroll_height(
    ui:&Ui,card_min_inner_height:f32,reserve_below:f32
)->f32 {
    management_single_list_scroll_height_geometry(
        ui.min_rect().top()+card_min_inner_height,
        ui.cursor().top(),ui.available_height(),reserve_below
    )
}

/// Pure calculation used by the management list geometry check. A 1pt
/// minimum avoids requesting an invalid/negative scroll area when a split
/// window is shorter than its non-scrolling toolbar and reserved footer.
pub(crate) fn management_single_list_scroll_height_geometry(
    card_bottom:f32,rows_top:f32,available_height:f32,reserve_below:f32
)->f32 {
    let remaining=(card_bottom-rows_top-reserve_below.max(0.0)).max(0.0);
    remaining.min(available_height.max(0.0)).max(1.0)
}


#[cfg(test)]
mod regression_tests;


// Presentation-only snapshots are scoped by the stable page/tab/epoch UI ID.
// A transient empty projection must never replace the last valid viewport.
#[derive(Clone,Debug,Default)]
struct ScrollSnapshot{
    offset:egui::Vec2,
    anchor:Option<(egui::Id,f32)>,
    pending:bool,
}
#[derive(Clone,Copy)]
pub(crate) enum RowGeometry<'a>{Fixed(f32),Variable(&'a [f32])}
#[derive(Clone,Copy)]
pub(crate) struct ScrollRows<'a>{
    pub count:usize,
    pub key:&'a dyn Fn(usize)->Option<egui::Id>,
    pub geometry:RowGeometry<'a>,
}
impl ScrollRows<'_>{
    fn top(self,index:usize)->f32{
        match self.geometry{RowGeometry::Fixed(h)=>index as f32*h,
            RowGeometry::Variable(offsets)=>offsets.get(index).copied().unwrap_or(0.0)}
    }
    fn anchor(self,y:f32)->Option<(egui::Id,f32)>{
        if self.count==0{return None;}
        let index=match self.geometry{
            RowGeometry::Fixed(h)=>(y/h.max(1.0)).floor().max(0.0) as usize,
            RowGeometry::Variable(offsets)=>offsets.partition_point(|v|*v<=y).saturating_sub(1),
        }.min(self.count-1);
        Some(((self.key)(index)?,(y-self.top(index)).max(0.0)))
    }
    fn restore(self,anchor:&(egui::Id,f32))->Option<f32>{
        let i=(0..self.count).find(|&i|(self.key)(i)==Some(anchor.0))?;
        let height=(self.top(i+1)-self.top(i)).max(0.0);
        Some(self.top(i)+anchor.1.min((height-0.01).max(0.0)))
    }
}
impl ScrollSnapshot{
    fn restore(&self,rows:Option<ScrollRows<'_>>)->egui::Vec2{
        let mut offset=self.offset;
        if let (Some(rows),Some(anchor))=(rows,&self.anchor){
            if let Some(y)=rows.restore(anchor){offset.y=y;}
        }
        offset
    }
    fn observe(&mut self,ready:bool,offset:egui::Vec2,rows:Option<ScrollRows<'_>>){
        if !ready{self.pending=true;return;}
        self.offset=offset;
        self.anchor=rows.and_then(|r|r.anchor(offset.y));
        self.pending=false;
    }
}
pub(crate) fn row_key(id:&str)->Option<egui::Id>{
    (!id.is_empty()).then(||egui::Id::new(id))
}
type ScrollStore=crate::view_state::TimedViewStore<ScrollEntry>;
#[derive(Clone)]
struct ScrollEntry{
    snapshot:ScrollSnapshot,
    touched:std::time::Instant,
    native:egui::Id,
}
impl crate::view_state::TimedEntry for ScrollEntry{
    fn touched(&self)->std::time::Instant{self.touched}
    fn touch(&mut self,now:std::time::Instant){self.touched=now;}
}
fn store_id()->egui::Id{egui::Id::new("navigation-scroll-store")}
pub(crate) fn sweep_snapshots(ctx:&egui::Context,now:std::time::Instant){
    let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data|{
        let retired={
            let store=data.get_temp_mut_or_default::<ScrollStore>(store_id());
            store.advance_frame(frame,now);
            store.sweep_due(now).into_iter().map(|entry|entry.native).collect::<Vec<_>>()
        };
        for id in retired{data.remove::<egui::containers::scroll_area::State>(id);}
    });
}
// Non-table scrollers that have their own viewport restoration still
// register native state for the same expiration and reclamation policy.
pub(crate) fn track_native<R>(ui:&Ui,output:&egui::containers::scroll_area::ScrollAreaOutput<R>){
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("scrollbar",output.inner_rect,ui.clip_rect(),None);
    let now=std::time::Instant::now();
    sweep_snapshots(ui.ctx(),now);
    ui.ctx().data_mut(|data|{
        let store=data.get_temp_mut_or_default::<ScrollStore>(store_id());
        let id=output.id.with("native-only");
        store.put(id,ScrollEntry{snapshot:Default::default(),touched:now,native:output.id});
    });
}
pub(crate) struct ScrollCapture{
    id:egui::Id,
    state:ScrollSnapshot,
    ready:bool,
}
impl ScrollCapture{
    pub(crate) fn begin(ui:&Ui,id:impl std::hash::Hash,scroll:egui::ScrollArea,
        ready:bool,rows:Option<ScrollRows<'_>>)->(egui::ScrollArea,Self){
        let id=ui.make_persistent_id(("navigation-scroll-snapshot",id));
        let now=std::time::Instant::now();
        sweep_snapshots(ui.ctx(),now);
        let state=ui.ctx().data_mut(|data|{
            let entry=data.get_temp_mut_or_default::<ScrollStore>(store_id()).entries.remove(&id);
            match entry{
                Some(entry) if crate::view_state::is_fresh(entry.touched,now)=>entry.snapshot,
                Some(entry)=>{data.remove::<egui::containers::scroll_area::State>(entry.native);ScrollSnapshot::default()}
                None=>ScrollSnapshot::default(),
            }
        });
        // A worker can complete before the first returning frame. Resolve
        // changed row positions even when no loading frame was painted.
        let moved=ready&&rows.zip(state.anchor.as_ref()).is_some_and(|(rows,anchor)|
            rows.anchor(state.offset.y).as_ref()!=Some(anchor));
        let scroll=if ready&&(state.pending||moved){scroll.scroll_offset(state.restore(rows))}else{scroll};
        (scroll,Self{id,state,ready})
    }
    pub(crate) fn finish<R>(mut self,ui:&Ui,
        output:&egui::containers::scroll_area::ScrollAreaOutput<R>,rows:Option<ScrollRows<'_>>){
        #[cfg(test)]
        crate::ui_test_support::record_component_geometry("scrollbar",output.inner_rect,ui.clip_rect(),None);
        self.state.observe(self.ready,output.state.offset,rows);
        ui.ctx().data_mut(|data|{
            let store=data.get_temp_mut_or_default::<ScrollStore>(store_id());
            store.put(self.id,ScrollEntry{
                snapshot:self.state,touched:std::time::Instant::now(),native:output.id,
            });
        });
    }
}
#[cfg(test)]
mod snapshot_tests{
    use super::*;
    #[test]
    fn transient_empty_preserves_both_axes_and_row_anchor(){
        let keys=["a","b","c"];
        let key=|i:usize|row_key(keys[i]);
        let rows=ScrollRows{count:keys.len(),key:&key,geometry:RowGeometry::Fixed(40.0)};
        let mut s=ScrollSnapshot::default();
        s.observe(true,egui::vec2(22.0,45.0),Some(rows));
        s.observe(false,egui::Vec2::ZERO,None);
        s.observe(false,egui::Vec2::ZERO,None);
        let new=["new","a","b","c"];
        let key=|i:usize|row_key(new[i]);
        let rows=ScrollRows{count:new.len(),key:&key,geometry:RowGeometry::Fixed(40.0)};
        assert_eq!(s.restore(Some(rows)),egui::vec2(22.0,85.0));
        s.observe(true,egui::vec2(22.0,85.0),Some(rows));
        assert!(!s.pending);
        s.observe(true,egui::Vec2::ZERO,None);
        assert_eq!(s.offset,egui::Vec2::ZERO);
    }
    #[test]
    fn expired_entries_are_removed_and_recent_entries_survive(){
        let now=std::time::Instant::now();
        let mut store=ScrollStore::default();
        let a=egui::Id::new("a");let b=egui::Id::new("b");
        store.entries.insert(a,ScrollEntry{snapshot:Default::default(),touched:now,native:a});
        store.entries.insert(b,ScrollEntry{snapshot:Default::default(),touched:now+std::time::Duration::from_secs(1),native:b});
        assert_eq!(store.sweep(now+crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL).into_iter().map(|entry|entry.native).collect::<Vec<_>>(),vec![a]);
        assert!(store.entries.contains_key(&b));assert_eq!(store.entries.len(),1);
    }
    #[test]
    fn idle_visible_viewport_is_not_expired(){
        let now=std::time::Instant::now();
        let mut store=ScrollStore::default();
        let id=egui::Id::new("visible");
        store.frame=Some(1);store.seen.insert(id);
        store.entries.insert(id,ScrollEntry{snapshot:Default::default(),touched:now,native:id});
        let later=now+crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL;
        store.advance_frame(2,later);
        assert!(store.sweep(later).is_empty());
        assert_eq!(store.sweep(later+crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL).into_iter().map(|entry|entry.native).collect::<Vec<_>>(),vec![id]);
    }
    #[test]
    fn repeated_generations_reclaim_native_scroll_entries(){
        let ctx=egui::Context::default();
        let start=std::time::Instant::now();
        let ttl=crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL;
        let mut previous=Vec::new();
        // Exercise the production context sweep, including native egui
        // state removal, for 1,024 generations of 16 viewports.
        for generation in 0..1024_u32{
            let now=start+ttl*generation;
            sweep_snapshots(&ctx,now);
            ctx.data_mut(|data|{
                for id in &previous{
                    assert!(data.get_persisted::<egui::containers::scroll_area::State>(*id).is_none());
                }
                let store=data.get_temp_mut_or_default::<ScrollStore>(store_id());
                assert!(store.entries.is_empty());
            });
            previous.clear();
            for pane in 0..16{
                let id=egui::Id::new(("navigation-generation",generation,pane));
                let mut snapshot=ScrollSnapshot::default();
                snapshot.observe(true,egui::vec2(12.0,240.0),None);
                ctx.data_mut(|data|{
                    data.get_temp_mut_or_default::<ScrollStore>(store_id()).entries
                        .insert(id,ScrollEntry{snapshot,touched:now,native:id});
                    data.insert_persisted(id,egui::containers::scroll_area::State::default());
                });
                previous.push(id);
            }
            ctx.data_mut(|data|{
                let store=data.get_temp_mut_or_default::<ScrollStore>(store_id());
                assert_eq!(store.entries.len(),16);
            });
        }
        sweep_snapshots(&ctx,start+ttl*1024);
        ctx.data_mut(|data|{
            for id in previous{assert!(data.get_persisted::<egui::containers::scroll_area::State>(id).is_none());}
            assert!(data.get_temp_mut_or_default::<ScrollStore>(store_id()).entries.is_empty());
        });
    }
    #[test]
    fn repeated_navigation_keeps_recent_offsets_and_retires_old_ones(){
        let start=std::time::Instant::now();
        let ttl=crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL;
        let step=std::time::Duration::from_secs(60);
        let mut store=ScrollStore::default();
        for n in 0..10_000_u32{
            let now=start+step*n;
            store.advance_frame(u64::from(n),now);
            let id=egui::Id::new(("viewport",n));
            let mut snapshot=ScrollSnapshot::default();
            snapshot.observe(true,egui::vec2(3.0,n as f32),None);
            store.entries.insert(id,ScrollEntry{snapshot,touched:now,native:id});
            store.seen.insert(id);
            store.sweep(now);
            assert_eq!(store.entries[&id].snapshot.offset.y,n as f32);
            assert!(store.entries.len()<=31,"expired viewport retained at {n}");
            if n>31{
                assert!(!store.entries.contains_key(&egui::Id::new(("viewport",n-31))));
            }
        }
        let end=start+step*10_000;
        store.advance_frame(10_000,end);
        store.sweep(end+ttl);
        assert!(store.entries.is_empty());
        assert!(store.seen.is_empty());
    }
    #[test]
    fn ordinary_anchor_reads_only_one_key(){
        let calls=std::cell::Cell::new(0);
        let key=|i:usize|{calls.set(calls.get()+1);Some(egui::Id::new(i))};
        let rows=ScrollRows{count:100_000,key:&key,geometry:RowGeometry::Fixed(40.0)};
        assert_eq!(rows.anchor(400.0),Some((egui::Id::new(10),0.0)));
        assert_eq!(calls.get(),1);
    }
    #[test]
    fn deleted_anchor_falls_back_to_offset_and_variable_height_is_resolved(){
        let keys=["a","b"];
        let key=|i:usize|row_key(keys[i]);
        let mut s=ScrollSnapshot::default();
        s.observe(true,egui::vec2(8.0,56.0),Some(ScrollRows{
            count:keys.len(),key:&key,geometry:RowGeometry::Variable(&[0.0,50.0,90.0])}));
        s.observe(false,egui::Vec2::ZERO,None);
        assert_eq!(s.restore(Some(ScrollRows{count:keys.len(),key:&key,geometry:RowGeometry::Variable(&[0.0,70.0,90.0])})).y,76.0);
        assert_eq!(s.restore(Some(ScrollRows{count:1,key:&key,geometry:RowGeometry::Fixed(40.0)})).y,56.0);
    }
}
