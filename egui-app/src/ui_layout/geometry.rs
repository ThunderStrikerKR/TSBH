//! Low-level UI geometry. Depends on egui only, not screen or component modules.
use eframe::egui::{self,Layout,Rect,Sense,Ui,UiBuilder,Vec2};

/// Align logical UI boundaries to egui's own layout lattice. Round the
/// absolute edges, not individual sizes: neighboring rectangles retain a
/// common edge and accumulated section widths/heights do not drift.
/// Physical-pixel stroke snapping is a separate paint-only operation.
pub(crate) fn aligned_rect(rect:Rect)->Rect{
    use egui::emath::GuiRounding as _;
    rect.round_ui()
}

/// The only production owner of explicit UiBuilder rectangle placement.
/// Native flow layout remains egui-owned. Callers that also paint or clip
/// the same slot should use aligned_rect before sharing the rectangle.
pub(crate) trait AlignedUiBuilderExt{
    fn aligned_max_rect(self,rect:Rect)->Self;
}
impl AlignedUiBuilderExt for UiBuilder{
    fn aligned_max_rect(self,rect:Rect)->Self{
        self.max_rect(aligned_rect(rect))
    }
}

/// Size constraints use the same logical lattice as explicit placements.
/// Keep egui's next-widget-relative semantics: a fixed constraint sets
/// both bounds, while a minimum reservation must still permit growth.
fn aligned_extent(value:f32)->f32{
    use egui::emath::GuiRounding as _;
    value.round_ui()
}
pub(crate) fn set_fixed_height(ui:&mut Ui,height:f32){
    ui.set_height(aligned_extent(height));
}
pub(crate) fn reserve_min_height(ui:&mut Ui,height:f32){
    ui.set_min_height(aligned_extent(height));
}
pub(crate) fn reserve_min_size(ui:&mut Ui,size:Vec2){
    ui.set_min_size(aligned_allocation_size(size));
}

/// Explicit allocation inputs share the logical size lattice. Keep native
/// egui negotiation, cursor advancement, IDs, response/sense, and layout.
/// In particular an exact content rectangle and its response can differ.
fn aligned_allocation_size(size:Vec2)->Vec2{
    Vec2::new(aligned_extent(size.x),aligned_extent(size.y))
}

pub(crate) trait SharedAllocationExt{
    fn shared_allocate_exact_size(&mut self,size:Vec2,sense:Sense)->(Rect,egui::Response);
    fn shared_allocate_space(&mut self,size:Vec2)->(egui::Id,Rect);
    fn shared_allocate_ui_with_layout<R>(&mut self,size:Vec2,layout:Layout,draw:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>;
    fn shared_add_sized(&mut self,size:impl Into<Vec2>,widget:impl egui::Widget)->egui::Response;
    fn shared_allocate_response(&mut self,size:Vec2,sense:Sense)->egui::Response;
}
impl SharedAllocationExt for Ui{
    fn shared_allocate_exact_size(&mut self,size:Vec2,sense:Sense)->(Rect,egui::Response){
        self.allocate_exact_size(aligned_allocation_size(size),sense)
    }
    fn shared_allocate_space(&mut self,size:Vec2)->(egui::Id,Rect){
        self.allocate_space(aligned_allocation_size(size))
    }
    fn shared_allocate_ui_with_layout<R>(&mut self,size:Vec2,layout:Layout,draw:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>{
        self.allocate_ui_with_layout(aligned_allocation_size(size),layout,draw)
    }
    fn shared_add_sized(&mut self,size:impl Into<Vec2>,widget:impl egui::Widget)->egui::Response{
        self.add_sized(aligned_allocation_size(size.into()),widget)
    }
    fn shared_allocate_response(&mut self,size:Vec2,sense:Sense)->egui::Response{
        self.allocate_response(aligned_allocation_size(size),sense)
    }
}

/// Clip boundaries are visibility limits, not allocation requests.
/// Preserve their exact intersections: rounding outward can expose a
/// neighboring row, and rounding inward can cut off a border or glyph.
pub(crate) trait SharedUiClipExt{
    /// Ordinary children cannot extend their inherited visible region.
    fn restrict_shared_clip(&mut self,requested:Rect);
    /// A separate foreground Area borrows the explicit clip of its owner
    /// pane, rather than the independent Area's inherited clip.
    fn use_owner_pane_clip(&mut self,owner_pane:Rect);
}
impl SharedUiClipExt for Ui{
    fn restrict_shared_clip(&mut self,requested:Rect){
        self.set_clip_rect(self.clip_rect().intersect(requested));
    }
    fn use_owner_pane_clip(&mut self,owner_pane:Rect){
        self.set_clip_rect(owner_pane);
    }
}

/// Painter clipping always narrows the inherited paint region. It never
/// changes widget allocation, hit targets, IDs or layout cursor.
pub(crate) trait SharedPainterClipExt{
    fn with_shared_clip(&self,requested:Rect)->egui::Painter;
}
impl SharedPainterClipExt for egui::Painter{
    fn with_shared_clip(&self,requested:Rect)->egui::Painter{
        self.with_clip_rect(requested)
    }
}

/// Card width is independent of height. Natural cards do not reserve space;
/// full-width cards reserve a minimum, fixed columns retain their allocation,
/// and bounded report cards additionally constrain wrapping before scrolling.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(crate) enum CardWidthPolicy {
    #[default]
    FitContent,
    FillAvailable,
    Fixed,
    Bounded,
    /// Upper bound only: preserve the content's natural minimum width.
    UpperLimit,
}
pub(crate) fn apply_width_policy(ui:&mut Ui,policy:CardWidthPolicy,target:f32){
    if !target.is_finite(){return;}
    let width=aligned_extent(target.max(0.0));
    match policy{
        CardWidthPolicy::FitContent=>{},
        CardWidthPolicy::FillAvailable=>ui.set_min_width(width),
        // egui set_width already applies both min and max constraints.
        // Retain the distinct semantic roles for screen callers.
        CardWidthPolicy::Fixed|CardWidthPolicy::Bounded=>ui.set_width(width),
        CardWidthPolicy::UpperLimit=>ui.set_max_width(width),
    }
}

/// Same policy for owned Ui and borrowed Ui receivers.
pub(crate) trait UiWidthPolicyExt {
    fn apply_shared_width(&mut self,policy:CardWidthPolicy,target:f32);
}
impl UiWidthPolicyExt for Ui {
    fn apply_shared_width(&mut self,policy:CardWidthPolicy,target:f32){
        apply_width_policy(self,policy,target);
    }
}


