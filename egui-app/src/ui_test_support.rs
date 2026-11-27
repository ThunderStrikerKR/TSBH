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


/// Real screen popup transition harness. Hold the same records, viewport and
/// scroll state; change only transient UI state in the caller. Capture the final
/// egui pass of *every* displayed frame, not just the settled transition.
pub(crate) fn assert_popup_transition_geometry(
    ctx:&egui::Context,screen:Rect,phases:usize,
    mut render:impl FnMut(&egui::Context,usize)
){
    let mut baseline:Option<Vec<Sample>>=None;
    for phase in 0..phases{
        for frame in 0..6{
            let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                begin_component_geometry();
                crate::floating_notice::begin_page(ctx,"actual popup transition");
                render(ctx,phase);
                crate::floating_notice::show(ctx);
            });
            let samples=finish_component_geometry(screen);
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
    fn label(&self)->&'static str{"synthetic popup geometry"}
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
    let mut baseline=highlight_frame(ctx,screen,None,&mut draw);
    for _ in 0..11{baseline=highlight_frame(ctx,screen,None,&mut draw);}
    let targets=baseline.pointer_targets(screen);
    assert!(!targets.is_empty(),"{case}: no actual highlight hit targets discovered");
    for (index,point) in targets.iter().copied().enumerate(){
        for phase in 0..6{
            let current=highlight_frame(ctx,screen,(phase<3).then_some(point),&mut draw);
            baseline.assert_invariant(&current,&format!("{case} target {index} {point:?} frame {phase}"));
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
