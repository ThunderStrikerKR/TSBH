//! Device-pixel alignment for paint only. Never use these results for UI allocation,
//! hit testing, clipping, or logical layout. Native OS coordinates are separate.
use eframe::egui::{self,Color32,CornerRadius,Painter,Rect,Stroke};

/// Paint-only rectangle in egui points, snapped to the device grid.
/// No production accessor, Deref or conversion into logical Rect.
#[derive(Clone,Copy,Debug)]
pub(crate) struct PaintRect(Rect);

impl PaintRect {
    pub(crate) fn fill(self,painter:&Painter,radius:impl Into<CornerRadius>,color:Color32){
        painter.rect_filled(self.0,radius,color);
    }
    pub(crate) fn edge_rule(self,logical_width:f32,inset:f32,ppp:f32,at_bottom:bool)->Self{
        let rect=self.0;
        let height=paint_stroke_width(logical_width,ppp);
        let inset=inset.min(rect.width().max(0.0)*0.5);
        let top=if at_bottom {rect.bottom()-height}else{rect.top()};
        Self(Rect::from_min_max(
            egui::pos2(rect.left()+inset,top),
            egui::pos2(rect.right()-inset,(top+height).min(rect.bottom()))
        ))
    }
    pub(crate) fn toolbar_rule(self,title_height:f32,logical_width:f32,inset:f32,ppp:f32)->Self{
        let rect=self.0;
        let top=(rect.top()+title_height).min(rect.bottom());
        Self(Rect::from_min_max(
            egui::pos2(rect.left(),top),
            egui::pos2(rect.right(),(top+logical_width).min(rect.bottom()))
        )).edge_rule(logical_width,inset,ppp,false)
    }
    pub(crate) fn finish_frame(self,painter:&Painter,ppp:f32,toolbar_bottom:f32,divider_x:Option<f32>,
        divider_width:f32,frame_width:f32,radius:impl Into<CornerRadius>,color:Color32){
        let rect=self.0;
        let snap=|v:f32|snap_paint_coordinate(v,ppp);
        let line_width=paint_stroke_width(divider_width,ppp);
        let outer_width=paint_stroke_width(frame_width,ppp);
        let inset=outer_width.max(line_width);
        let left=snap(rect.left()+inset);
        let right=snap(rect.right()-inset);
        let top=snap(rect.top()+inset);
        let bottom=snap(rect.bottom()-inset);
        if right>left && bottom>top {
            if toolbar_bottom>rect.top() &&
                toolbar_bottom<rect.bottom() {
                let y=snap(toolbar_bottom-line_width/2.0)
                    .clamp(top,(bottom-line_width).max(top));
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(left,y),egui::pos2(right,y+line_width)
                    ),0.0,color
                );
            }
            if let Some(divider_x)=divider_x {
                let x=snap(divider_x-line_width/2.0)
                    .clamp(left,(right-line_width).max(left));
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(x,top),egui::pos2(x+line_width,bottom)
                    ),0.0,color
                );
            }
        }
        painter.rect_stroke(
            rect,radius,
            Stroke::new(outer_width,color),
            egui::StrokeKind::Inside
        );
    }
    #[cfg(test)]
    pub(crate) fn test_rect(self)->Rect { self.0 }
}

fn paint_scale(pixels_per_point:f32)->f32 {
    pixels_per_point.max(1.0)
}

/// Round a coordinate onto the physical pixel grid, in egui points.
fn snap_paint_coordinate(value:f32,pixels_per_point:f32)->f32 {
    let scale=paint_scale(pixels_per_point);
    (value*scale).round()/scale
}

/// Round absolute paint edges, not the logical allocation.
pub(crate) fn snap_paint_rect(rect:Rect,pixels_per_point:f32)->PaintRect {
    let snap=|v|snap_paint_coordinate(v,pixels_per_point);
    PaintRect(Rect::from_min_max(
        egui::pos2(snap(rect.left()),snap(rect.top())),
        egui::pos2(snap(rect.right()),snap(rect.bottom()))
    ))
}

/// Preserve at least one physical pixel for an ornamental stroke.
fn paint_stroke_width(width:f32,pixels_per_point:f32)->f32 {
    let scale=paint_scale(pixels_per_point);
    (width*scale).round().max(1.0)/scale
}

/// Center a filled axis line in the gutter with equal integer-pixel margins.
/// Keep all hit targets and pane boundaries in the original logical rect.
pub(crate) fn centered_paint_axis_rect(rect:Rect,vertical:bool,logical_width:f32,pixels_per_point:f32)->PaintRect {
    let scale=paint_scale(pixels_per_point);
    let (low,high)=if vertical {(rect.left(),rect.right())}else{(rect.top(),rect.bottom())};
    let start_px=(low*scale).round() as i32;
    let end_px=(high*scale).round() as i32;
    let span_px=(end_px-start_px).max(1);
    let mut width_px=((logical_width*scale).round() as i32).clamp(1,span_px);
    if (span_px-width_px)%2!=0 {
        width_px+=if width_px<span_px {1}else{-1};
    }
    let start=start_px+(span_px-width_px)/2;
    let end=start+width_px;
    let snap=|v|snap_paint_coordinate(v,scale);
    PaintRect(if vertical {
        Rect::from_min_max(
            egui::pos2(start as f32/scale,snap(rect.top())),
            egui::pos2(end as f32/scale,snap(rect.bottom()))
        )
    } else {
        Rect::from_min_max(
            egui::pos2(snap(rect.left()),start as f32/scale),
            egui::pos2(snap(rect.right()),end as f32/scale)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paint_rect_has_no_implicit_layout_conversion(){
        {
            trait NoInto<A>{fn check(){}}
            impl<T> NoInto<()> for T {}
            impl<T:Into<Rect>> NoInto<u8> for T {}
            let _=<PaintRect as NoInto<_>>::check;
        }
        {
            trait NoDeref<A>{fn check(){}}
            impl<T> NoDeref<()> for T {}
            impl<T:std::ops::Deref<Target=Rect>> NoDeref<u8> for T {}
            let _=<PaintRect as NoDeref<_>>::check;
        }
        {
            trait NoAsRef<A>{fn check(){}}
            impl<T> NoAsRef<()> for T {}
            impl<T:AsRef<Rect>> NoAsRef<u8> for T {}
            let _=<PaintRect as NoAsRef<_>>::check;
        }
    }
    #[test]
    fn paint_grid_preserves_half_pixel_bound_and_center_parity(){
        let rect=Rect::from_min_max(egui::pos2(5.25,7.25),egui::pos2(11.25,39.25));
        for scale in [1.0_f32,1.25,1.5,1.75,2.0] {
            let snapped=snap_paint_rect(rect,scale).test_rect();
            assert!((snapped.left()-rect.left()).abs()<=0.5/scale+0.0001);
            assert!((snapped.bottom()-rect.bottom()).abs()<=0.5/scale+0.0001);
            let width=paint_stroke_width(1.0,scale);
            assert!(width*scale>=1.0-0.0001);
            for vertical in [true,false] {
                let line=centered_paint_axis_rect(rect,vertical,2.0,scale).test_rect();
                let a=if vertical {line.left()-snapped.left()}else{line.top()-snapped.top()};
                let b=if vertical {snapped.right()-line.right()}else{snapped.bottom()-line.bottom()};
                assert!((a-b).abs()<0.0001);
            }
        }
    }
}
