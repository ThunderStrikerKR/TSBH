//! Shared tag block geometry. Read-only and removable tags differ only
//! by the reserved action slot and the caller's display policy.
use eframe::egui::{self,Align,Layout,Rect,Response,Sense,Ui,Vec2};
use crate::{theme,typography};

pub(crate) struct TagChip{
    frame:egui::Frame,
    galley:std::sync::Arc<egui::Galley>,
    inset:Vec2,
    action_width:f32,
}
impl TagChip{
    pub(crate) fn new(ui:&Ui,text:&str,removable:bool)->Self{
        let frame=super::modal_nested_role_frame(super::ModalNestedRole::CompactTag);
        let inset=Vec2::new(frame.inner_margin.left as f32+frame.stroke.width,
            frame.inner_margin.top as f32+frame.stroke.width);
        Self{frame,galley:typography::management_control_text_galley(ui,text,theme::TEXT),
            inset,action_width:if removable{theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH+theme::SPACE_4}else{0.0}}
    }
    pub(crate) fn size(&self)->Vec2{
        Vec2::new(self.galley.size().x.ceil()+self.action_width+2.0*self.inset.x,
            theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH+2.0*self.inset.y)
    }
    pub(crate) fn paint(&self,ui:&Ui,rect:Rect,clip:Rect)->Rect{
        let clip=clip.intersect(ui.clip_rect());
        let painter=ui.painter().with_clip_rect(clip);
        theme::paint_bordered_surface(&painter,rect,self.frame.corner_radius,self.frame.fill,self.frame.stroke.color);
        let inner=rect.shrink2(self.inset);
        let label=Rect::from_min_max(inner.min,egui::pos2(inner.right()-self.action_width,inner.bottom()));
        let pos=egui::pos2(label.left(),label.center().y-self.galley.size().y*0.5);
        painter.with_clip_rect(label.intersect(clip)).galley(pos,self.galley.clone(),theme::TEXT);
        label
    }
}
pub(crate) fn removable_tag_chip(ui:&mut Ui,tag:&str)->Response{
    let visible=if tag.chars().count()>22{
        format!("{}…",tag.chars().take(22).collect::<String>())
    }else{tag.to_owned()};
    let chip=TagChip::new(ui,&visible,true);
    let (rect,_)=ui.allocate_exact_size(chip.size(),Sense::hover());
    let label_rect=chip.paint(ui,rect,ui.clip_rect());
    let label=ui.interact(label_rect,ui.make_persistent_id(("tag-label",tag)),Sense::hover());
    crate::ui_primitives::shortened_text_tooltip(label,&visible,tag);
    let icon=theme::MANAGEMENT_DATE_NAV_BUTTON_WIDTH;
    let inner=rect.shrink2(chip.inset);
    let icon_rect=Rect::from_min_size(egui::pos2(inner.right()-icon,inner.top()),Vec2::splat(icon));
    let mut action=ui.new_child(egui::UiBuilder::new().id_salt(("tag-remove",tag))
        .max_rect(icon_rect).layout(Layout::left_to_right(Align::Center)));
    action.set_clip_rect(icon_rect.intersect(ui.clip_rect()));
    action.spacing_mut().interact_size=Vec2::splat(icon);
    let response=crate::button::icon_action_button(&mut action,"×",true);
    #[cfg(test)]
    {
        crate::ui_test_support::record("tag chip",rect);
        crate::ui_test_support::record("tag label",label_rect);
        crate::ui_test_support::record("tag remove",response.rect);
    }
    response
}
