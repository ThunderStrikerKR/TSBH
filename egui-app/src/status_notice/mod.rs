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
    ui.painter().with_clip_rect(visible).text(
        visible.center(),egui::Align2::CENTER_CENTER,caption,
        egui::FontId::proportional(typography::FONT_METADATA),
        theme::MUTED
    );
}



pub(crate) fn body_error_overlay(ui:&Ui,viewport:Rect,context:&str,error:&str){
    let visible=viewport.intersect(ui.clip_rect());
    if visible.width()<=0.0||visible.height()<=0.0{return;}
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

/// Untyped recognition/workflow notices retain informational coloring.
pub(crate) fn reception_review_status(ui:&mut Ui,message:&str){
    let tone=theme::ReviewTone::Info;
    let display=crate::user_error::display("reception.review",message);
    crate::card::review_issue_card(ui,tone,|content|{
        content.add(egui::Label::new(typography::body(display.as_ref())
            .color(tone.text_color())).wrap());
    });
}
