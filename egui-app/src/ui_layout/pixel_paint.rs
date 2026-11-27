//! Device-pixel alignment for paint only. Never use these results for UI allocation,
//! hit testing, clipping, or logical layout. Native OS coordinates are separate.
use eframe::egui::{self,Rect};

fn paint_scale(pixels_per_point:f32)->f32 {
    pixels_per_point.max(1.0)
}

/// Round a coordinate onto the physical pixel grid, in egui points.
pub(crate) fn snap_paint_coordinate(value:f32,pixels_per_point:f32)->f32 {
    let scale=paint_scale(pixels_per_point);
    (value*scale).round()/scale
}

/// Round absolute paint edges, not the logical allocation.
pub(crate) fn snap_paint_rect(rect:Rect,pixels_per_point:f32)->Rect {
    let snap=|v|snap_paint_coordinate(v,pixels_per_point);
    Rect::from_min_max(
        egui::pos2(snap(rect.left()),snap(rect.top())),
        egui::pos2(snap(rect.right()),snap(rect.bottom()))
    )
}

/// Preserve at least one physical pixel for an ornamental stroke.
pub(crate) fn paint_stroke_width(width:f32,pixels_per_point:f32)->f32 {
    let scale=paint_scale(pixels_per_point);
    (width*scale).round().max(1.0)/scale
}

/// Center a filled axis line in the gutter with equal integer-pixel margins.
/// Keep all hit targets and pane boundaries in the original logical rect.
pub(crate) fn centered_paint_axis_rect(rect:Rect,vertical:bool,logical_width:f32,pixels_per_point:f32)->Rect {
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
    if vertical {
        Rect::from_min_max(
            egui::pos2(start as f32/scale,snap(rect.top())),
            egui::pos2(end as f32/scale,snap(rect.bottom()))
        )
    } else {
        Rect::from_min_max(
            egui::pos2(snap(rect.left()),start as f32/scale),
            egui::pos2(snap(rect.right()),end as f32/scale)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paint_grid_preserves_half_pixel_bound_and_center_parity(){
        let rect=Rect::from_min_max(egui::pos2(5.25,7.25),egui::pos2(11.25,39.25));
        for scale in [1.0_f32,1.25,1.5,1.75,2.0] {
            let snapped=snap_paint_rect(rect,scale);
            assert!((snapped.left()-rect.left()).abs()<=0.5/scale+0.0001);
            assert!((snapped.bottom()-rect.bottom()).abs()<=0.5/scale+0.0001);
            let width=paint_stroke_width(1.0,scale);
            assert!(width*scale>=1.0-0.0001);
            for vertical in [true,false] {
                let line=centered_paint_axis_rect(rect,vertical,2.0,scale);
                let a=if vertical {line.left()-snapped.left()}else{line.top()-snapped.top()};
                let b=if vertical {snapped.right()-line.right()}else{snapped.bottom()-line.bottom()};
                assert!((a-b).abs()<0.0001);
            }
        }
    }
}
