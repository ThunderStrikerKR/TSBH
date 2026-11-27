//! Layout-neutral status and error notices plus the shared informational review card.
use eframe::egui::{self,Rect,Ui};
use crate::{theme,typography};
use crate::ui_primitives::{overflow_region_tooltip,text_exceeds_rect};

/// Zero-layout feedback for fixed management table/scroll viewports.
///
/// Pending, empty and no-result text must never create an egui row, change a
/// header's Y coordinate, consume scrollbar height or resize the enclosing
/// card. Call this ONLY after the caller has reserved a stable body rectangle.
/// Operational errors and decisions continue through FloatingNotice/modals.
pub(crate) fn management_body_state_overlay(
    ui:&Ui,viewport:Rect,caption:Option<&str>
){
    let Some(caption)=caption else {return};
    let visible=viewport.intersect(ui.clip_rect());
    if visible.width()<=0.0 || visible.height()<=0.0{return;}
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("notice",visible,ui.clip_rect(),None);
    ui.painter().with_clip_rect(visible).text(
        visible.center(),egui::Align2::CENTER_CENTER,caption,
        egui::FontId::proportional(typography::FONT_METADATA),
        theme::MUTED
    );
}



pub(crate) fn body_error_overlay(ui:&Ui,viewport:Rect,context:&str,error:&str){
    let visible=viewport.intersect(ui.clip_rect());
    if visible.width()<=0.0||visible.height()<=0.0{return;}
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("notice",visible,ui.clip_rect(),None);
    let display=crate::user_error::display(context,error);
    let painter=ui.painter().with_clip_rect(visible);
    let galley=painter.layout(display.as_ref().to_owned(),
        egui::FontId::proportional(typography::FONT_METADATA),
        theme::DANGER,(visible.width()-2.0*theme::SPACE_8).max(1.0));
    let pos=egui::pos2(
        visible.center().x-galley.size().x/2.0,
        (visible.center().y-galley.size().y/2.0).max(visible.top())
    );
    let clipped=text_exceeds_rect(galley.size(),visible);
    painter.galley(pos,galley,theme::DANGER);
    overflow_region_tooltip(ui,visible,ui.id().with(("body-error-overlay",context)),
        clipped,display.as_ref());
}

/// Untyped recognition notices retain informational coloring. Callers
/// that actually know an operation failed must use the explicit error path.
pub(crate) fn reception_review_status(ui:&mut Ui,message:&str){
    reception_review_notice(ui,"reception.review",message,theme::ReviewTone::Info);
}
pub(crate) fn reception_preview_status(ui:&mut Ui,message:&str){
    reception_review_notice(ui,"reception.preview",message,theme::ReviewTone::Info);
}
pub(crate) fn reception_review_error(ui:&mut Ui,context:&str,message:&str){
    reception_review_notice(ui,context,message,theme::ReviewTone::Error);
}
fn reception_review_notice(ui:&mut Ui,context:&str,message:&str,tone:theme::ReviewTone){
    let display=crate::user_error::display(context,message);
    crate::card::review_issue_card(ui,tone,|content|{
        content.add(egui::Label::new(typography::body(display.as_ref())
            .color(tone.text_color())).wrap());
    });
}


#[cfg(test)]
mod shared_status_notice_rendered_geometry_tests{
    use super::*;
    use eframe::egui::{Vec2,Sense};
    use crate::ui_test_support::{test_context,painted_text_rects,assert_rect_geometry,
        assert_rect_inside,assert_geometry_value};

    #[test]
    fn management_feedback_is_centered_clipped_and_never_allocates_layout(){
        let ctx=test_context();
        let mut baseline=None;
        for caption in [None,Some("조회 중"),Some("조회 결과 없음"),None]{
            let mut measured=None;
            let output=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    let viewport=ui.allocate_exact_size(Vec2::new(300.0,110.0),Sense::hover()).0;
                    let cursor=ui.cursor();
                    management_body_state_overlay(ui,viewport,caption);
                    assert_eq!(ui.cursor(),cursor,"status overlay allocated a row");
                    let next=ui.allocate_exact_size(Vec2::new(80.0,20.0),Sense::hover()).0;
                    measured=Some((viewport,next));
                });
            });
            let (viewport,next)=measured.expect("status overlay layout");
            if let Some((old,old_next))=baseline{
                assert_rect_geometry("status viewport",viewport,old,0.5);
                assert_rect_geometry("following control",next,old_next,0.5);
            }else{baseline=Some((viewport,next));}
            if let Some(label)=caption{
                let (ink,clip)=painted_text_rects(&output,label).into_iter().next().expect("status paint");
                assert_rect_inside("status caption",ink,viewport,1.0);
                assert_rect_geometry("status clip",clip,viewport,1.0);
                assert_geometry_value("status","caption center x",ink.center().x,viewport.center().x,1.0);
                assert_geometry_value("status","caption center y",ink.center().y,viewport.center().y,1.0);
            }else{
                assert!(painted_text_rects(&output,"조회 중").is_empty());
                assert!(painted_text_rects(&output,"조회 결과 없음").is_empty());
            }
        }
    }
}
