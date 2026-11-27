#![cfg(test)]
//! Inspect the actual egui draw list; no mock layout or screenshot inference.
use super::*;
use crate::ui_test_support::{test_context,assert_rect_geometry};
use eframe::egui::{Pos2,Vec2,Sense};

#[derive(Clone,Copy,Debug)]
struct Outline{
    rect:Rect,clip:Rect,stroke:Stroke,corners:CornerRadius,kind:egui::StrokeKind,
}
fn outlines(output:&egui::FullOutput)->Vec<Outline>{
    fn visit(shape:&egui::Shape,clip:Rect,out:&mut Vec<Outline>){
        match shape{
            egui::Shape::Rect(s) if s.stroke.width>0.0 && s.stroke.color!=Color32::TRANSPARENT=>{
                out.push(Outline{rect:s.rect,clip,stroke:s.stroke,corners:s.corner_radius,kind:s.stroke_kind});
            }
            egui::Shape::Vec(shapes)=>for shape in shapes{visit(shape,clip,out);},
            _=>{}
        }
    }
    let mut out=Vec::new();
    for clipped in &output.shapes{visit(&clipped.shape,clipped.clip_rect,&mut out);}
    out
}
fn near(a:Rect,b:Rect,tolerance:f32)->bool{
    (a.left()-b.left()).abs()<=tolerance&&(a.right()-b.right()).abs()<=tolerance
        &&(a.top()-b.top()).abs()<=tolerance&&(a.bottom()-b.bottom()).abs()<=tolerance
}
fn contained(inner:Rect,outer:Rect,tolerance:f32)->bool{
    inner.left()>=outer.left()-tolerance&&inner.right()<=outer.right()+tolerance
        &&inner.top()>=outer.top()-tolerance&&inner.bottom()<=outer.bottom()+tolerance
}
fn ink_bounds(outline:Outline)->Rect{
    let extension=match outline.kind{
        egui::StrokeKind::Inside=>0.0,
        egui::StrokeKind::Middle=>outline.stroke.width*0.5,
        egui::StrokeKind::Outside=>outline.stroke.width,
    };
    outline.rect.expand(extension)
}
/// A fully visible target must have exactly one stroke on its perimeter.
/// Count neutral as well as semantic strokes, including up to 3pt inset
/// (the former default-frame + accent overlay). Smaller inner glyphs,
/// separate controls, structural column rules and status stripes are not
/// perimeter candidates.
fn inspect(output:&egui::FullOutput,target:Rect,expected:Stroke,geometry:Geometry)->Result<(),String>{
    let candidates:Vec<_>=outlines(output).into_iter()
        .filter(|s|near(s.rect,target,3.0)).collect();
    if candidates.len()!=1{return Err(format!("duplicate or missing perimeter: expected 1, found {} for {target:?}: {candidates:?}",candidates.len()));}
    let actual=candidates[0];
    // Rounding to physical pixels may move a shape edge by half a pixel.
    let tolerance=(0.5/output.pixels_per_point.max(1.0))+0.75;
    if !near(actual.rect,target,tolerance){return Err(format!("perimeter geometry drift: {actual:?} vs {target:?}"));}
    if actual.stroke.color!=expected.color||(actual.stroke.width-expected.width).abs()>0.05{
        return Err(format!("unexpected stroke: {actual:?}; expected {expected:?}"));
    }
    if actual.corners!=geometry.corners(){return Err(format!("corner mismatch: {actual:?}; expected {:?}",geometry.corners()));}
    if !contained(ink_bounds(actual),actual.clip,tolerance){
        return Err(format!("clipped highlight perimeter: {actual:?}"));
    }
    Ok(())
}
fn assert_outline(output:&egui::FullOutput,target:Rect,expected:Stroke,geometry:Geometry){
    inspect(output,target,expected,geometry).unwrap_or_else(|e|panic!("{e}"));
}
fn frame(ctx:&egui::Context,width:f32,events:Vec<egui::Event>,mut draw:impl FnMut(&mut Ui))->egui::FullOutput{
    ctx.run(egui::RawInput{
        screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(width,650.0))),
        events,..Default::default()
    },|ctx|{egui::CentralPanel::default().show(ctx,|ui|draw(ui));})
}
#[test]
fn detector_rejects_duplicate_and_clipped_actual_shapes(){
    let ctx=test_context();
    let rect=Rect::from_min_size(egui::pos2(60.0,60.0),egui::vec2(160.0,36.0));
    let p=palette(Role::Hover).stroke.unwrap();
    let good=frame(&ctx,600.0,vec![],|ui|paint(ui,rect,Geometry::Control,Role::Hover));
    assert_outline(&good,rect,p,Geometry::Control);
    let duplicate=frame(&ctx,600.0,vec![],|ui|{
        ui.painter().rect_stroke(rect.shrink(0.5),Geometry::Control.corners(),
            Stroke::new(1.0,theme::BORDER),egui::StrokeKind::Inside);
        paint(ui,rect,Geometry::Control,Role::Hover);
    });
    assert!(inspect(&duplicate,rect,p,Geometry::Control).unwrap_err().contains("duplicate"));
    let clipped=frame(&ctx,600.0,vec![],|ui|{
        let clip=Rect::from_min_max(rect.min,egui::pos2(rect.right()-8.0,rect.bottom()));
        paint_outline(&ui.painter().with_clip_rect(clip),rect,Geometry::Control,Role::Hover);
    });
    assert!(inspect(&clipped,rect,p,Geometry::Control).unwrap_err().contains("clipped"));
}
#[test]
fn partially_visible_scroll_highlight_preserves_parent_clip(){
    let ctx=test_context();
    let rect=Rect::from_min_size(egui::pos2(60.0,60.0),egui::vec2(160.0,36.0));
    let viewport=Rect::from_min_max(rect.min,egui::pos2(rect.right()-20.0,rect.bottom()));
    let output=frame(&ctx,600.0,vec![],|ui|{
        let p=ui.painter().with_clip_rect(viewport);
        paint_outline(&p,rect,Geometry::Control,Role::Hover);
        paint_outline(&p,rect.translate(egui::vec2(300.0,0.0)),Geometry::Control,Role::Hover);
    });
    let matching:Vec<_>=outlines(&output).into_iter()
        .filter(|s|s.stroke.color==palette(Role::Hover).stroke.unwrap().color && near(s.rect,rect,1.0)).collect();
    assert_eq!(matching.len(),1);
    assert_eq!(matching[0].clip,viewport);
    assert!(!contained(ink_bounds(matching[0]),viewport,0.1),
        "this fixture intentionally clips a scrolling item, not a full visible control");
    assert!(!outlines(&output).iter().any(|s|near(s.rect,rect.translate(egui::vec2(300.0,0.0)),1.0)));
}
#[test]
fn native_action_hover_and_persistent_error_keep_one_outline_at_dpi_and_widths(){
    use theme::ButtonRole;
    for scale in [1.0,1.25,1.5,2.0]{
        for width in [420.0,800.0]{
            for (role,persistent) in [
                (ButtonRole::Standard,None),(ButtonRole::Primary,None),
                (ButtonRole::Standard,Some(Role::RequiredInput)),
                (ButtonRole::Standard,Some(Role::ReviewTint)),
            ]{
                let ctx=test_context();ctx.set_pixels_per_point(scale);
                let mut rect=Rect::NOTHING;
                let mut baseline=Rect::NOTHING;
                let mut hovered=false;
                for phase in 0..3{
                    let events=if phase==0{vec![]}else{vec![egui::Event::PointerMoved(rect.center())]};
                    let output=frame(&ctx,width,events,|ui|{
                        let response=crate::button::action_with_emphasis(ui,"확인",role,true,persistent);
                        rect=response.rect;hovered=response.hovered();
                    });
                    if phase==0{baseline=rect;}
                    if phase==2{
                        assert!(hovered,"fixture never reached real widget hover");
                        assert_rect_geometry("native action hover",rect,baseline,0.75);
                        let (_,rest,_,_)=crate::button::action_style(role,true,false);
                        let highlight=persistent.unwrap_or(if rest==theme::BRIGHT{Role::LightHover}else{Role::Hover});
                        assert_outline(&output,rect,palette(highlight).stroke.unwrap(),button_geometry(role));
                    }
                }
            }
        }
    }
}
#[test]
fn actual_fixed_workflow_action_and_cod_dropdown_use_single_perimeter(){
    for scale in [1.0,1.5,2.0]{
        let ctx=test_context();ctx.set_pixels_per_point(scale);
        let mut fixed=Rect::NOTHING;
        let mut dropdown=Rect::NOTHING;
        let mut hovered=false;
        for phase in 0..3{
            let events=if phase==0{vec![]}else{vec![egui::Event::PointerMoved(fixed.center())]};
            let output=frame(&ctx,550.0,events,|ui|{
                let (rect,_)=ui.allocate_exact_size(egui::vec2(140.0,34.0),Sense::hover());
                let response=crate::button::workflow_fixed_action(ui,egui::Id::new("outline-workflow"),
                    rect,crate::button::WorkflowFixedAction::Auxiliary(crate::button::WorkflowAuxiliaryAction::Manage),true);
                fixed=response.rect;hovered=response.hovered();
                dropdown=crate::dropdown::reception_cod_dropdown(ui,Some(false),(true,true)).1.rect;
            });
            if phase==2{
                assert!(hovered);
                assert_outline(&output,fixed,palette(Role::Hover).stroke.unwrap(),Geometry::Navigation);
                assert_outline(&output,dropdown,palette(Role::ReviewTint).stroke.unwrap(),Geometry::Control);
            }
        }
    }
}
#[test]
fn actual_selected_calendar_and_table_row_keep_one_perimeter(){
    let ctx=test_context();
    let mut day=Rect::NOTHING;
    let mut row=Rect::NOTHING;
    for phase in 0..3{
        let events=if phase==0{vec![]}else{vec![egui::Event::PointerMoved(day.center())]};
        let output=frame(&ctx,600.0,events,|ui|{
            day=crate::button::calendar_day(ui,"27",36.0,true,true).rect;
            row=Rect::from_min_size(ui.cursor().min,egui::vec2(308.0,42.0));
            crate::table::fixed_row(ui,"selected-outline",row,0,&[44.0,256.0],8.0,
                crate::table::RowTone::SelectedIf(true),|_,_|{});
        });
        if phase==2{
            assert_outline(&output,day,Stroke::new(theme::SELECTION_STROKE_WIDTH,theme::SELECTION),Geometry::Control);
            assert_outline(&output,row,palette(Role::Selected).stroke.unwrap(),Geometry::TableRow);
        }
    }
}
#[test]
fn actual_required_recipient_and_quantity_editor_choose_error_outline(){
    for scale in [1.0,1.5,2.0]{
        let ctx=test_context();ctx.set_pixels_per_point(scale);
        let mut name="".to_owned();let mut address="서울".to_owned();
        let mut mobile="010".to_owned();let mut phone=String::new();let mut note=String::new();
        let mut quantity="".to_owned();
        let mut required=Rect::NOTHING;let mut quantity_outer=Rect::NOTHING;
        let output=frame(&ctx,800.0,vec![],|ui|{
            let responses=crate::composite_reception::reception_recipient_fields(
                ui,&mut name,&mut address,&mut mobile,&mut phone,&mut note,
                FieldHighlights::new([true,false,false,false,false],[true,false,false,false,false],[false;5]));
            required=responses[0].rect;
            let (cell,_)=ui.allocate_exact_size(egui::vec2(128.0,42.0),Sense::hover());
            let _=crate::input::reception_item_numeric_editor(ui,cell,&mut quantity,true,None,
                (crate::input::ReceptionNumericCell::Quantity,true));
            quantity_outer=segment_outline_rect(cell.shrink2(egui::Vec2::splat(theme::SPACE_2)));
        });
        let error=palette(Role::RequiredInput).stroke.unwrap();
        assert_outline(&output,required,error,Geometry::Control);
        assert_outline(&output,quantity_outer,error,Geometry::Control);
    }
}
#[test]
fn source_block_geometry_and_value_clearance_survive_real_draw_list(){
    let ctx=test_context();
    let normal=source_block_rect(80.0,140.0,50.0,22.0).unwrap();
    let focused=focused_source_rect(normal);
    let slot=Rect::from_min_max(egui::pos2(70.0,90.0),egui::pos2(170.0,130.0));
    let value=value_rect(Rect::from_min_max(egui::pos2(85.0,100.0),egui::pos2(145.0,119.0)),slot);
    let output=frame(&ctx,600.0,vec![],|ui|{
        paint(ui,focused,Geometry::Source,Role::LinkedPreview);
        paint(ui,value,Geometry::Control,Role::RequiredReview);
    });
    assert_outline(&output,focused,palette(Role::LinkedPreview).stroke.unwrap(),Geometry::Source);
    assert_outline(&output,value,palette(Role::RequiredReview).stroke.unwrap(),Geometry::Control);
    assert_eq!(normal.left(),77.5);assert_eq!(normal.top(),51.0);
    assert_eq!(value.left(),81.0);assert_eq!(value.top(),96.0);
}
