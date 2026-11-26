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

pub(crate) fn management_scroll_style() -> egui::style::ScrollStyle {
    let mut style=egui::style::ScrollStyle::solid();
    style.bar_width=10.0;
    style.handle_min_length=44.0;
    style.bar_inner_margin=SPACE_4;
    style.bar_outer_margin=SPACE_4;
    style.foreground_color=true;
    style
}

/// CONFLICT-003=A: table rails use the same 10pt solid style but WITHOUT
/// independent 4pt inner/outer margins. Non-table scroll areas keep the
/// original management_scroll_style until their role is reviewed.
pub(crate) fn management_table_scroll_style()->egui::style::ScrollStyle{
    let mut style=management_scroll_style();
    style.bar_inner_margin=0.0;
    style.bar_outer_margin=0.0;
    style
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
    anchor:Option<(String,f32)>,
    pending:bool,
}
#[derive(Clone,Copy)]
pub(crate) enum RowGeometry<'a>{Fixed(f32),Variable(&'a [f32])}
#[derive(Clone,Copy)]
pub(crate) struct ScrollRows<'a>{
    pub keys:&'a [String],
    pub geometry:RowGeometry<'a>,
}
impl ScrollRows<'_>{
    fn top(self,index:usize)->f32{
        match self.geometry{RowGeometry::Fixed(h)=>index as f32*h,
            RowGeometry::Variable(offsets)=>offsets.get(index).copied().unwrap_or(0.0)}
    }
    fn anchor(self,y:f32)->Option<(String,f32)>{
        if self.keys.is_empty(){return None;}
        let index=match self.geometry{
            RowGeometry::Fixed(h)=>(y/h.max(1.0)).floor().max(0.0) as usize,
            RowGeometry::Variable(offsets)=>offsets.partition_point(|v|*v<=y).saturating_sub(1),
        }.min(self.keys.len()-1);
        let key=&self.keys[index];
        if key.is_empty(){return None;}
        Some((key.clone(),(y-self.top(index)).max(0.0)))
    }
    fn restore(self,anchor:&(String,f32))->Option<f32>{
        let i=self.keys.iter().position(|key|key==&anchor.0)?;
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
pub(crate) struct ScrollCapture{
    id:egui::Id,
    state:ScrollSnapshot,
    ready:bool,
}
impl ScrollCapture{
    pub(crate) fn begin(ui:&Ui,id:impl std::hash::Hash,scroll:egui::ScrollArea,
        ready:bool,rows:Option<ScrollRows<'_>>)->(egui::ScrollArea,Self){
        let id=ui.make_persistent_id(("navigation-scroll-snapshot",id));
        let state=ui.ctx().data_mut(|data|data.get_temp::<ScrollSnapshot>(id).unwrap_or_default());
        // A worker can complete before the first returning frame. Resolve
        // changed row positions even when no loading frame was painted.
        let moved=ready&&rows.zip(state.anchor.as_ref()).is_some_and(|(rows,anchor)|
            rows.anchor(state.offset.y).as_ref()!=Some(anchor));
        let scroll=if ready&&(state.pending||moved){scroll.scroll_offset(state.restore(rows))}else{scroll};
        (scroll,Self{id,state,ready})
    }
    pub(crate) fn finish<R>(mut self,ui:&Ui,
        output:&egui::containers::scroll_area::ScrollAreaOutput<R>,rows:Option<ScrollRows<'_>>){
        self.state.observe(self.ready,output.state.offset,rows);
        ui.ctx().data_mut(|data|data.insert_temp(self.id,self.state));
    }
}
#[cfg(test)]
mod snapshot_tests{
    use super::*;
    #[test]
    fn transient_empty_preserves_both_axes_and_row_anchor(){
        let keys=["a","b","c"].map(str::to_owned);
        let rows=ScrollRows{keys:&keys,geometry:RowGeometry::Fixed(40.0)};
        let mut s=ScrollSnapshot::default();
        s.observe(true,egui::vec2(22.0,45.0),Some(rows));
        s.observe(false,egui::Vec2::ZERO,None);
        s.observe(false,egui::Vec2::ZERO,None);
        let new=["new","a","b","c"].map(str::to_owned);
        let rows=ScrollRows{keys:&new,geometry:RowGeometry::Fixed(40.0)};
        assert_eq!(s.restore(Some(rows)),egui::vec2(22.0,85.0));
        s.observe(true,egui::vec2(22.0,85.0),Some(rows));
        assert!(!s.pending);
        s.observe(true,egui::Vec2::ZERO,None);
        assert_eq!(s.offset,egui::Vec2::ZERO);
    }
    #[test]
    fn deleted_anchor_falls_back_to_offset_and_variable_height_is_resolved(){
        let keys=["a","b"].map(str::to_owned);
        let mut s=ScrollSnapshot::default();
        s.observe(true,egui::vec2(8.0,56.0),Some(ScrollRows{
            keys:&keys,geometry:RowGeometry::Variable(&[0.0,50.0,90.0])}));
        s.observe(false,egui::Vec2::ZERO,None);
        assert_eq!(s.restore(Some(ScrollRows{keys:&keys,geometry:RowGeometry::Variable(&[0.0,70.0,90.0])})).y,76.0);
        assert_eq!(s.restore(Some(ScrollRows{keys:&keys[..1],geometry:RowGeometry::Fixed(40.0)})).y,56.0);
    }
}
