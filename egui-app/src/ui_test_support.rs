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
            // External regression modules contain no production code here.
            if declaration.trim_end().ends_with(';'){continue;}
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
/// Inspect actual egui paint commands, including partially or fully clipped
/// diagnostic text. The orange line can remain visible when its label is clipped.
pub(crate) fn assert_no_unaligned(output:&egui::FullOutput,case:&str){
    assert_unaligned_warnings_empty(&painted_text_rects(output,"Unaligned"),case);
}
fn assert_unaligned_warnings_empty(warnings:&[(Rect,Rect)],case:&str){
    assert!(warnings.is_empty(),"{case}: egui Unaligned warning(s): {} (text rect, clip rect) {:?}",
        warnings.len(),warnings);
}
pub(crate) fn enable_unaligned_diagnostics(ctx:&egui::Context){
    assert!(cfg!(debug_assertions),"Unaligned audit requires a debug test build");
    ctx.style_mut(|style|style.debug.show_unaligned=true);
}

#[cfg(test)]
mod unaligned_detector_tests{
    use super::*;
    #[test]
    fn native_egui_warning_is_detected_and_aligned_control_is_clean(){
        for malformed in [false,true]{
            let ctx=test_context();
            enable_unaligned_diagnostics(&ctx);
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(500.0,300.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    let bounds=Rect::from_min_max(egui::pos2(40.0,40.0),egui::pos2(180.0,100.0));
                    let mut child=ui.new_child(egui::UiBuilder::new().id_salt("unaligned-control").max_rect(bounds));
                    child.expand_to_include_rect(bounds);
                    let bottom=if malformed{100.01}else{100.0};
                    child.expand_to_include_y(bottom);
                    assert_eq!(child.min_rect().bottom(),bottom);
                });
            });
            let warnings=painted_text_rects(&output,"Unaligned");
            if malformed{
                assert!(!warnings.is_empty(),"positive control failed to emit native egui warning");
                let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||assert_no_unaligned(&output,"positive control")));
                assert!(result.is_err(),"detector accepted native warning");
            }else{
                assert_no_unaligned(&output,"aligned control");
            }
        }
    }
}

pub(crate) fn assert_rect_inside(role:&str,inner:Rect,outer:Rect,tolerance:f32){
    assert!(inner.is_positive()&&outer.is_positive(),"{role}: invalid rectangles");
    assert!(inner.left()>=outer.left()-tolerance&&inner.right()<=outer.right()+tolerance
        &&inner.top()>=outer.top()-tolerance&&inner.bottom()<=outer.bottom()+tolerance,
        "{role}: {inner:?} escapes {outer:?}");
}
/// Inspect the ordered paint stream, not merely a widget's allocated Rect.
#[derive(Clone,Copy)]
enum CaptionPaint {
    Text{rect:Rect,clip:Rect,elided:bool},
    OpaqueRect(Rect),
}
fn caption_paint(output:&egui::FullOutput,label:&str)->Vec<CaptionPaint>{
    fn visit(shape:&egui::Shape,clip:Rect,label:&str,out:&mut Vec<CaptionPaint>){
        match shape{
            egui::Shape::Text(text) if text.galley.text()==label=>{
                out.push(CaptionPaint::Text{
                    rect:text.galley.rect.translate(text.pos.to_vec2()),clip,elided:text.galley.elided
                });
            }
            // Only prove complete occlusion by a solid rectangular fill.
            // Rounded corners, translucent fills, strokes and arbitrary
            // meshes are intentionally not treated as opaque rectangles.
            egui::Shape::Rect(shape) if shape.fill.a()==255
                &&shape.corner_radius==egui::CornerRadius::ZERO=>{
                let visible=shape.rect.intersect(clip);
                if visible.is_positive(){out.push(CaptionPaint::OpaqueRect(visible));}
            }
            egui::Shape::Vec(shapes)=>for shape in shapes{visit(shape,clip,label,out);},
            _=>{}
        }
    }
    let mut out=Vec::new();
    for shape in &output.shapes{visit(&shape.shape,shape.clip_rect,label,&mut out);}
    out
}
/// Check a named caption within a known visible slot. Horizontal clipping is
/// allowed only for an explicitly overflow-capable value; vertical clipping
/// and full occlusion remain failures. This is not a global text audit:
/// offscreen scroll rows and editable text have distinct visibility policies.
pub(crate) fn assert_caption_visibility(output:&egui::FullOutput,label:&str,widget:Rect,
    allow_horizontal_overflow:bool)->Rect{
    let paint=caption_paint(output,label);
    let mut failure="missing painted caption";
    for (index,entry) in paint.iter().enumerate(){
        let CaptionPaint::Text{rect,clip,elided}=*entry else{continue};
        if !rect.intersect(widget).is_positive(){continue;}
        let visible=rect.intersect(clip).intersect(widget);
        if !visible.is_positive(){failure="fully clipped caption";continue;}
        if rect.top()<clip.top()-1.0||rect.bottom()>clip.bottom()+1.0
            ||rect.top()<widget.top()-1.0||rect.bottom()>widget.bottom()+1.0{
            failure="vertically clipped caption";continue;
        }
        if !allow_horizontal_overflow && (elided||
            rect.left()<clip.left()-1.0||rect.right()>clip.right()+1.0||
            rect.left()<widget.left()-1.0||rect.right()>widget.right()+1.0){
            failure="horizontally clipped or elided required caption";continue;
        }
        if paint[index+1..].iter().any(|entry|matches!(entry,CaptionPaint::OpaqueRect(cover)
            if cover.contains_rect(visible))){
            failure="fully occluded caption";continue;
        }
        return rect;
    }
    panic!("{failure}: {label} in {widget:?}");
}
pub(crate) fn assert_painted_caption(output:&egui::FullOutput,label:&str,widget:Rect)->Rect{
    assert_caption_visibility(output,label,widget,false)
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
    fn paint_case(mode:&str)->(egui::FullOutput,Rect){
        let ctx=test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(500.0,300.0));
        let widget=Rect::from_min_size(egui::pos2(30.0,30.0),egui::vec2(220.0,40.0));
        let output=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let painter=ui.painter();
                let galley=painter.layout_no_wrap("검증 라벨".to_owned(),
                    egui::FontId::proportional(18.0),egui::Color32::WHITE);
                let pos=widget.center()-galley.size()/2.0;
                let ink=galley.rect.translate(pos.to_vec2());
                let clip=match mode{
                    "vertical"=>Rect::from_min_max(egui::pos2(widget.left(),ink.center().y),widget.max),
                    "horizontal"=>Rect::from_min_max(widget.min,egui::pos2(ink.center().x,widget.bottom())),
                    "hidden"=>Rect::from_min_max(widget.min,egui::pos2(ink.left()-2.0,widget.bottom())),
                    _=>widget,
                };
                if mode=="background"{painter.rect_filled(widget,0.0,egui::Color32::BLACK);}
                painter.with_clip_rect(clip).galley(pos,galley,egui::Color32::WHITE);
                if mode=="covered"{painter.rect_filled(widget,0.0,egui::Color32::BLACK);}
                if mode=="translucent"{painter.rect_filled(widget,0.0,egui::Color32::from_black_alpha(80));}
            });
        });
        (output,widget)
    }
    #[test]
    fn caption_detector_accepts_visible_background_and_explicit_horizontal_overflow(){
        for mode in ["normal","background","translucent","horizontal"]{
            let (output,widget)=paint_case(mode);
            assert_caption_visibility(&output,"검증 라벨",widget,mode=="horizontal");
        }
    }
    #[test]
    fn caption_detector_rejects_clipped_missing_and_opaque_covered_text(){
        for (mode,allow,expected) in [
            ("vertical",true,"vertically clipped"),
            ("horizontal",false,"horizontally clipped"),
            ("hidden",true,"fully clipped"),
            ("covered",true,"fully occluded"),
        ]{
            let (output,widget)=paint_case(mode);
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                assert_caption_visibility(&output,"검증 라벨",widget,allow)));
            let error=result.expect_err("caption detector accepted malformed paint");
            let message=error.downcast_ref::<String>().map(String::as_str)
                .or_else(||error.downcast_ref::<&str>().copied()).unwrap_or("");
            // egui may omit a completely clipped paint command.
            assert!(message.contains(expected)||(mode=="hidden"&&message.contains("missing painted caption")),
                "{mode}: unexpected failure {message}");
        }
        let (output,widget)=paint_case("normal");
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            assert_painted_caption(&output,"존재하지 않는 라벨",widget))).is_err());
    }
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


/// Real screen popup transition harness. Hold the same records, viewport and
/// scroll state; change only transient UI state in the caller. Capture the final
/// egui pass of *every* displayed frame, not just the settled transition.
pub(crate) fn assert_popup_transition_geometry(
    ctx:&egui::Context,screen:Rect,phases:usize,
    mut render:impl FnMut(&egui::Context,usize)
){
    enable_unaligned_diagnostics(ctx);
    let mut baseline:Option<Vec<Sample>>=None;
    for phase in 0..phases{
        for frame in 0..6{
            let output=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                begin_component_geometry();
                crate::floating_notice::begin_page(ctx,"actual popup transition");
                render(ctx,phase);
                crate::floating_notice::show(ctx);
            });
            let samples=finish_component_geometry(screen);
            assert_no_unaligned(&output,&format!("{} popup phase {phase} frame {frame}",std::thread::current().name().unwrap_or("popup")));
            let slots=samples.into_iter().filter(|s|matches!(s.role,
                "popup"|"input"|"button"|"dropdown"|"checkbox"|"scrollbar"))
                .map(|s|(s.role,s.rect)).collect::<Vec<_>>();
            if phase==0{
                if frame==5{
                    assert!(slots.iter().any(|(role,_)|*role=="popup"),"real popup trace is empty");
                    assert!(slots.iter().any(|(role,_)|*role=="button"),"real popup controls are missing");
                    baseline=Some(slots);
                }
            }else{
                assert_layout_rects_invariant(baseline.as_ref().unwrap(),&slots);
            }
        }
    }
}

/// Isolated test gateway for rendering screen-owned popups without touching
/// any real catalog or operational database. Mutations are never authorized
/// beyond the UI permission bit and always fail if accidentally dispatched.
pub(crate) struct PopupGeometryGateway{
    pub writable:bool,
}
impl crate::gateway::BandoGateway for PopupGeometryGateway{
    fn load_catalog(&self)->Result<serde_json::Value,String>{
        Err("isolated popup fixture".into())
    }
    fn load_working_source(&self)->Result<crate::preview_store::WorkingSource,String>{
        Err("isolated popup fixture".into())
    }
    fn save_working_source(&self,_:&crate::preview_store::WorkingSource)->Result<u64,String>{
        Err("popup fixture may not write".into())
    }
    fn execute(&self,_:crate::gateway::ReadOperation,_:serde_json::Value)->Result<serde_json::Value,String>{
        Err("isolated popup fixture".into())
    }
    fn model_group_write_enabled(&self)->bool{self.writable}
    fn product_write_enabled(&self)->bool{self.writable}
    fn shipping_write_enabled(&self)->bool{self.writable}
    fn excel_write_enabled(&self)->bool{self.writable}
}

/// A separate trace for nonstandard hit rectangles. Existing component traces
/// retain their original contract and no production code allocates a trace.
thread_local!{
    static HIGHLIGHT_TARGETS:RefCell<Option<Vec<(&'static str,Rect,Rect)>>>=RefCell::new(None);
}
pub(crate) fn record_highlight_target(role:&'static str,rect:Rect,clip:Rect){
    HIGHLIGHT_TARGETS.with(|trace|{
        if let Some(samples)=trace.borrow_mut().as_mut(){samples.push((role,rect,clip));}
    });
}
#[derive(Debug)]
struct PaintedLabel{ text:String,rect:Rect,clip:Rect }
pub(crate) struct HighlightSnapshot{
    slots:Vec<Sample>,
    components:Vec<ComponentGeometry>,
    targets:Vec<(&'static str,Rect,Rect)>,
    labels:Vec<PaintedLabel>,
    icon_segments:Vec<([egui::Pos2;2],Rect)>,
    unaligned:Vec<(Rect,Rect)>,
}
fn painted_labels(output:&egui::FullOutput,screen:Rect)->Vec<PaintedLabel>{
    fn visit(shape:&egui::Shape,clip:Rect,screen:Rect,out:&mut Vec<PaintedLabel>){
        match shape{
            egui::Shape::Text(text)=>{
                let rect=text.galley.rect.translate(text.pos.to_vec2());
                if rect.intersect(clip).intersect(screen).is_positive()&&!text.galley.text().is_empty(){
                    out.push(PaintedLabel{text:text.galley.text().to_owned(),rect,clip});
                }
            }
            egui::Shape::Vec(shapes)=>for shape in shapes{visit(shape,clip,screen,out);},
            _=>{}
        }
    }
    let mut out=Vec::new();
    for shape in &output.shapes{visit(&shape.shape,shape.clip_rect,screen,&mut out);}
    out
}
fn painted_icon_segments(output:&egui::FullOutput,screen:Rect)->Vec<([egui::Pos2;2],Rect)>{
    fn visit(shape:&egui::Shape,clip:Rect,screen:Rect,out:&mut Vec<([egui::Pos2;2],Rect)>){
        match shape{
            egui::Shape::LineSegment{points,..}=>{
                let delta=points[1]-points[0];
                // Short diagonal segments are glyph strokes (e.g. chevron/X).
                // Axis-aligned table dividers, cursors and border rules are not.
                if delta.x.abs()>0.5&&delta.y.abs()>0.5&&delta.length()<=40.0
                    &&Rect::from_two_pos(points[0],points[1]).intersect(clip).intersect(screen).is_positive(){
                    out.push((*points,clip));
                }
            }
            egui::Shape::Vec(shapes)=>for shape in shapes{visit(shape,clip,screen,out);},
            _=>{}
        }
    }
    let mut out=Vec::new();
    for shape in &output.shapes{visit(&shape.shape,shape.clip_rect,screen,&mut out);}
    out
}
fn edges_near(a:Rect,b:Rect)->bool{
    [(a.left(),b.left()),(a.right(),b.right()),(a.top(),b.top()),(a.bottom(),b.bottom())]
        .into_iter().all(|(x,y)|x==y||(x-y).abs()<=0.5)
}
/// Measure the final egui pass of each frame. Input time advances without
/// dispatching any click, wheel, keyboard edit, or business operation.
pub(crate) fn highlight_frame(ctx:&egui::Context,screen:Rect,pointer:Option<egui::Pos2>,
    mut draw:impl FnMut(&egui::Context))->HighlightSnapshot{
    let time=ctx.input(|input|input.time)+0.05;
    let events=vec![pointer.map_or(egui::Event::PointerGone,egui::Event::PointerMoved)];
    let output=ctx.run(egui::RawInput{screen_rect:Some(screen),time:Some(time),predicted_dt:0.05,
        events,..Default::default()},|ctx|{
        TRACE.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
        begin_component_geometry();
        HIGHLIGHT_TARGETS.with(|trace|*trace.borrow_mut()=Some(Vec::new()));
        draw(ctx);
    });
    HighlightSnapshot{
        slots:TRACE.with(|trace|trace.borrow_mut().take().unwrap()),
        components:finish_component_geometry(screen),
        targets:HIGHLIGHT_TARGETS.with(|trace|trace.borrow_mut().take().unwrap()),
        labels:painted_labels(&output,screen),
        icon_segments:painted_icon_segments(&output,screen),
        unaligned:painted_text_rects(&output,"Unaligned"),
    }
}
impl HighlightSnapshot{
    /// Only visible hit targets, not structural cards, table frames or rails.
    /// Deduplicate coincident response/slot observations without dropping rows.
    pub(crate) fn pointer_targets(&self,screen:Rect)->Vec<egui::Pos2>{
        let mut result:Vec<egui::Pos2>=Vec::new();
        let components=self.components.iter().filter(|s|matches!(s.role,"button"|"input"|"dropdown"|"checkbox"))
            .map(|s|(s.rect,s.clip));
        let targets=self.targets.iter().map(|(_,rect,clip)|(*rect,*clip));
        let cells=self.slots.iter().filter(|(role,_)|matches!(*role,
            "recipient name"|"recipient address"|"recipient mobile"|"recipient phone"|"recipient note"|
            "item product"|"item quantity"|"item amount"|"freight amount"|
            "catalog data row"|"notice product row"|"settlement order row"|"settlement payment row"))
            .map(|(_,rect)|(*rect,screen));
        for (rect,clip) in components.chain(targets).chain(cells){
            let visible=rect.intersect(clip).intersect(screen);
            if visible.width()<4.0||visible.height()<4.0{continue;}
            let p=visible.center();
            if !result.iter().any(|old|(*old-p).length()<1.0){result.push(p);}
        }
        result
    }
    pub(crate) fn assert_icon_invariant(&self,other:&Self,case:&str){
        let mut used=vec![false;other.icon_segments.len()];
        for (points,clip) in &self.icon_segments{
            let index=other.icon_segments.iter().enumerate().position(|(i,(next,next_clip))|
                !used[i]&&edges_near(*clip,*next_clip)&&
                (points[0]-next[0]).length()<=0.5&&(points[1]-next[1]).length()<=0.5)
                .unwrap_or_else(||panic!("{case}: vector icon segment moved or disappeared: {points:?}"));
            used[index]=true;
        }
    }
    pub(crate) fn assert_invariant(&self,other:&Self,case:&str){
        assert_eq!(self.slots.len(),other.slots.len(),"{case}: layout slot count changed");
        for (index,((role,a),(next,b))) in self.slots.iter().zip(&other.slots).enumerate(){
            assert_eq!(role,next,"{case}: slot identity changed at {index}");
            assert!(edges_near(*a,*b),"{case}: slot {role}[{index}] moved {a:?} -> {b:?}");
        }
        assert_eq!(self.components.len(),other.components.len(),"{case}: component count changed");
        for (index,(a,b)) in self.components.iter().zip(&other.components).enumerate(){
            assert_eq!(a.role,b.role,"{case}: component identity changed at {index}");
            assert!(edges_near(a.rect,b.rect)&&edges_near(a.clip,b.clip),
                "{case}: {}[{index}] allocation/clip changed: {a:?} -> {b:?}",a.role);
        }
        assert_eq!(self.targets.len(),other.targets.len(),"{case}: hit-target count changed");
        for (index,((role,a,clip),(next,b,next_clip))) in self.targets.iter().zip(&other.targets).enumerate(){
            assert_eq!(role,next,"{case}: hit-target identity changed at {index}");
            assert!(edges_near(*a,*b)&&edges_near(*clip,*next_clip),
                "{case}: hit target {role}[{index}] changed {a:?} -> {b:?}");
        }
        // Match repeated labels one-to-one. New overlay/tooltip text may be
        // added, but cannot replace, move, resize or newly clip any old label.
        let mut used=vec![false;other.labels.len()];
        for old in &self.labels{
            let index=other.labels.iter().enumerate().position(|(i,new)|
                !used[i]&&old.text==new.text&&edges_near(old.rect,new.rect)&&edges_near(old.clip,new.clip))
                .unwrap_or_else(||panic!("{case}: painted label moved, disappeared or changed clip: {old:?}"));
            used[index]=true;
        }
    }
}
/// All observed visible targets, every sampled enter/leave frame. The
/// initial settle permits the existing first-mount scrollbar animation only.
pub(crate) fn sweep_highlight_geometry(ctx:&egui::Context,screen:Rect,case:&str,
    mut draw:impl FnMut(&egui::Context)){
    enable_unaligned_diagnostics(ctx);
    let mut baseline=highlight_frame(ctx,screen,None,&mut draw);
    assert_unaligned_warnings_empty(&baseline.unaligned,&format!("{case} initial frame 0"));
    for frame in 1..12{
        baseline=highlight_frame(ctx,screen,None,&mut draw);
        assert_unaligned_warnings_empty(&baseline.unaligned,&format!("{case} initial frame {frame}"));
    }
    let targets=baseline.pointer_targets(screen);
    assert!(!targets.is_empty(),"{case}: no actual highlight hit targets discovered");
    for (index,point) in targets.iter().copied().enumerate(){
        for phase in 0..6{
            let current=highlight_frame(ctx,screen,(phase<3).then_some(point),&mut draw);
            let sample=format!("{case} target {index} {point:?} frame {phase}");
            assert_unaligned_warnings_empty(&current.unaligned,&sample);
            baseline.assert_invariant(&current,&sample);
            baseline.assert_icon_invariant(&current,&sample);
        }
    }
    println!("HIGHLIGHT_GEOMETRY_SWEEP {case}: {} observed hit targets",targets.len());
}
#[cfg(test)]
mod highlight_invariance_detector_tests{
    use super::*;
    #[test]
    #[should_panic(expected="painted label moved")]
    fn detector_rejects_single_frame_text_movement(){
        let ctx=test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(500.0,300.0));
        let draw=|shift:f32|highlight_frame(&ctx,screen,None,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.painter().text(egui::pos2(50.0+shift,50.0),egui::Align2::LEFT_TOP,
                    "unchanged label",egui::FontId::proportional(16.0),egui::Color32::WHITE);
            });
        });
        let baseline=draw(0.0);
        baseline.assert_invariant(&draw(2.0),"deliberate one-frame regression");
    }
    #[test]
    #[should_panic(expected="vector icon segment moved")]
    fn detector_rejects_moved_vector_icon(){
        let ctx=test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(500.0,300.0));
        let draw=|shift:f32|highlight_frame(&ctx,screen,None,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.painter().line_segment([egui::pos2(50.0+shift,50.0),egui::pos2(56.0+shift,56.0)],
                    egui::Stroke::new(1.0,egui::Color32::WHITE));
            });
        });
        draw(0.0).assert_icon_invariant(&draw(2.0),"deliberate vector shift");
    }
    #[test]
    #[should_panic(expected="allocation/clip changed")]
    fn detector_rejects_component_size_change(){
        let ctx=test_context();
        let screen=Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(500.0,300.0));
        let draw=|width:f32|highlight_frame(&ctx,screen,None,|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let (rect,_)=ui.allocate_exact_size(egui::vec2(width,36.0),egui::Sense::hover());
                record_component_geometry("button",rect,ui.clip_rect(),None);
            });
        });
        draw(100.0).assert_invariant(&draw(104.0),"deliberate resize");
    }
}
