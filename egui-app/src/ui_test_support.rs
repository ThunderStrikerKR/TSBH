//! Shared test-only geometry trace and egui context; no dependency on composite tests.
use eframe::egui::{self,Rect};
use std::cell::RefCell;
use crate::{theme,typography};
pub(crate) fn test_context()->egui::Context {
    let ctx=egui::Context::default();
    theme::install(&ctx);
    typography::install(&ctx);
    ctx
}


pub(crate) type Sample=(&'static str,Rect);
thread_local!{
    pub(crate) static TRACE:RefCell<Option<Vec<Sample>>>=RefCell::new(None);
}
pub(crate) fn record(name:&'static str,rect:Rect){
    TRACE.with(|trace|{if let Some(samples)=trace.borrow_mut().as_mut(){samples.push((name,rect));}});
}

/// Source-contract helper for the repository's top-level test-module style.
/// Preserve production code both before and after each test module; inline
/// geometry probes are not module boundaries. This is a lexical convention,
/// not a Rust parser: a top-level module must close with an unindented brace.
pub(crate) fn production_source_for_contract(file:&str)->String {
    let mut lines=file.split_inclusive('\n').peekable();
    let mut result=String::with_capacity(file.len());
    while let Some(line)=lines.next(){
        if line.trim_end()=="#[cfg(test)]" &&
            lines.peek().is_some_and(|next|next.starts_with("mod ")){
            let declaration=lines.next().unwrap();
            assert!(declaration.trim_end().ends_with('{'),
                "unsupported source-contract test module declaration: {declaration}");
            assert!(lines.by_ref().any(|line|line.trim_end()=="}"),
                "unterminated source-contract test module: {declaration}");
            continue;
        }
        result.push_str(line);
    }
    result
}

pub(crate) fn assert_layout_rects_invariant(
    before:&[(&str,Rect)],after:&[(&str,Rect)]
){
    assert_eq!(before.len(),after.len(),"protected layout slot count changed");
    for ((name,a),(other,b)) in before.iter().zip(after){
        assert_eq!(name,other,"protected layout slot identity changed");
        for (edge,x,y) in [
            ("left",a.left(),b.left()),("top",a.top(),b.top()),
            ("right",a.right(),b.right()),("bottom",a.bottom(),b.bottom())
        ]{
            assert!((x-y).abs()<=0.5,
                "layout drift {name}.{edge}: {x} -> {y}");
        }
    }
}


 
/// Geometry assertions use the actual allocated frame and widget Rects, not
/// a second copy of the layout formula. Insets are [left, top, right, bottom]
/// and may include explicitly approved policy slack.
pub(crate) fn assert_geometry_value(role:&str,edge:&str,actual:f32,expected:f32,tolerance:f32){
    assert!(actual.is_finite() && expected.is_finite() &&
        (actual-expected).abs()<=tolerance,
        "{role}: {edge} actual={actual:.2}pt expected={expected:.2}pt tolerance={tolerance:.2}pt");
}
pub(crate) fn card_content_insets(card:Rect,content:Rect)->[f32;4]{
    [content.left()-card.left(),content.top()-card.top(),
        card.right()-content.right(),card.bottom()-content.bottom()]
}
pub(crate) fn assert_card_content_insets(
    role:&str,card:Rect,content:Rect,expected:[f32;4],tolerance:f32
){
    assert!(card.is_positive()&&content.is_positive(),"{role}: invalid Rects: {card:?}, {content:?}");
    for ((edge,actual),expected) in ["left","top","right","bottom"].into_iter()
        .zip(card_content_insets(card,content)).zip(expected){
        assert!(actual>=-tolerance,"{role}: content exceeds {edge} edge: {actual}pt");
        assert_geometry_value(role,edge,actual,expected,tolerance);
    }
}
pub(crate) fn assert_rect_geometry(role:&str,actual:Rect,expected:Rect,tolerance:f32){
    for (edge,a,b) in [
        ("left",actual.left(),expected.left()),("top",actual.top(),expected.top()),
        ("right",actual.right(),expected.right()),("bottom",actual.bottom(),expected.bottom())
    ]{
        assert_geometry_value(role,edge,a,b,tolerance);
    }
}


 
/// Actual text paint and effective clipping, shared by control regressions.
pub(crate) fn painted_text_rects(output:&egui::FullOutput,label:&str)->Vec<(Rect,Rect)>{
    fn visit(shape:&egui::Shape,clip:Rect,label:&str,out:&mut Vec<(Rect,Rect)>){
        match shape{
            egui::Shape::Text(text) if text.galley.text()==label=>
                out.push((text.galley.rect.translate(text.pos.to_vec2()),clip)),
            egui::Shape::Vec(shapes)=>for shape in shapes{visit(shape,clip,label,out);},
            _=>{}
        }
    }
    let mut out=Vec::new();
    for shape in &output.shapes{visit(&shape.shape,shape.clip_rect,label,&mut out);}
    out
}
pub(crate) fn assert_rect_inside(role:&str,inner:Rect,outer:Rect,tolerance:f32){
    assert!(inner.is_positive()&&outer.is_positive(),"{role}: invalid rectangles");
    assert!(inner.left()>=outer.left()-tolerance&&inner.right()<=outer.right()+tolerance
        &&inner.top()>=outer.top()-tolerance&&inner.bottom()<=outer.bottom()+tolerance,
        "{role}: {inner:?} escapes {outer:?}");
}
pub(crate) fn assert_painted_caption(output:&egui::FullOutput,label:&str,widget:Rect)->Rect{
    let (ink,clip)=painted_text_rects(output,label).into_iter()
        .find(|(r,_)|widget.contains(r.center()))
        .unwrap_or_else(||panic!("missing painted caption {label} in {widget:?}"));
    assert_rect_inside(label,ink,widget,1.0);
    assert_rect_inside(label,ink,clip,1.0);
    ink
}


 
/// Independent trace for production component calls, separate from fixed slots.
#[derive(Clone,Copy,Debug)]
pub(crate) struct ComponentGeometry{
    pub role:&'static str,
    pub rect:Rect,
    pub clip:Rect,
    pub minimum_height:Option<f32>,
}
thread_local!{
    static COMPONENT_TRACE:RefCell<Option<Vec<ComponentGeometry>>>=RefCell::new(None);
}
pub(crate) fn begin_component_geometry(){
    COMPONENT_TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
}
pub(crate) fn record_component_geometry(role:&'static str,rect:Rect,clip:Rect,minimum_height:Option<f32>){
    COMPONENT_TRACE.with(|trace|{
        if let Some(samples)=trace.borrow_mut().as_mut(){
            samples.push(ComponentGeometry{role,rect,clip,minimum_height});
        }
    });
}
pub(crate) fn finish_component_geometry(screen:Rect)->Vec<ComponentGeometry>{
    let samples=COMPONENT_TRACE.with(|trace|trace.borrow_mut().take().expect("component trace not started"));
    for sample in &samples{
        let ComponentGeometry{role,rect,clip,minimum_height}=*sample;
        assert!([rect.left(),rect.right(),rect.top(),rect.bottom()].iter().all(|v|v.is_finite()),
            "{role}: non-finite actual component geometry {rect:?}");
        // Overscan and nested scroll contents are allowed outside the viewport.
        let visible=rect.intersect(clip).intersect(screen);
        if !visible.is_positive(){continue;}
        assert!(rect.is_positive(),"{role}: invalid visible allocation {rect:?}");
        assert_rect_inside(role,visible,screen,0.5);
        if let Some(height)=minimum_height{
            assert!(rect.height()+1.0>=height,
                "{role}: actual control height {} below shared minimum {height}",rect.height());
        }
    }
    samples
}
pub(crate) fn assert_component_roles(samples:&[ComponentGeometry],required:&[&str]){
    for role in required{
        assert!(samples.iter().any(|sample|sample.role==*role&&sample.rect.is_positive()),
            "actual workflow never reached shared {role} renderer");
    }
}

#[cfg(test)]
mod shared_component_screen_trace_tests{
    use super::*;
    #[test]
    #[should_panic(expected="below shared minimum")]
    fn detector_rejects_visible_control_height_regression(){
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(400.0,300.0));
        begin_component_geometry();
        record_component_geometry("button",Rect::from_min_size(screen.min,egui::vec2(100.0,20.0)),screen,Some(34.0));
        finish_component_geometry(screen);
    }
    #[test]
    fn virtualized_offscreen_allocation_is_not_a_visible_geometry_failure(){
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(400.0,300.0));
        begin_component_geometry();
        record_component_geometry("input",Rect::from_min_size(egui::pos2(0.0,500.0),egui::vec2(100.0,20.0)),screen,Some(36.0));
        assert_eq!(finish_component_geometry(screen).len(),1);
    }
}
