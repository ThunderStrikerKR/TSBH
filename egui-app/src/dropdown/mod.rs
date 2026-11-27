//! Shared selection triggers, option rows and searchable menus; overlay owns popup lifetime.
use eframe::egui::{self,Color32,Rect,Response,Stroke,Ui,Vec2};
use crate::ui_primitives::UiWidthPolicyExt;
use crate::{theme,typography};
use crate::input::management_search_field;
use crate::theme::{text_ink,TextRole,TextState};

/// Caption state is supplied by the selection owner, not inferred from text.
#[derive(Clone,Copy)]
pub(crate) struct DropdownCaption<'a>{
    label:&'a str,
    placeholder:bool,
}
impl<'a> DropdownCaption<'a>{
    pub(crate) fn new(label:&'a str,placeholder:bool)->Self{Self{label,placeholder}}
}
impl<'a> From<&'a str> for DropdownCaption<'a>{
    fn from(label:&'a str)->Self{Self::new(label,false)}
}
impl<'a> From<&'a String> for DropdownCaption<'a>{
    fn from(label:&'a String)->Self{Self::new(label.as_str(),false)}
}
use crate::scrollbar::{scroll_area,ScrollAxes};
use crate::ui_primitives::{management_body_state_inline,text_exceeds_rect,overflow_tooltip,ManagementChevronDirection,paint_management_chevron_with_color};

/// Configure the shared SINGLE-SELECT popup only. Never recolor
/// unrelated checkboxes, tables, global widget hover, or order selection.
fn management_dropdown_option_palette(ui:&mut Ui){
    crate::highlight::dropdown_options(ui);
}

/// Reference "접수일 기준" dropdown: 34pt dark trigger, separated
/// right-hand chevron, 1pt outline, 6pt corner radius. This replaces native
/// ComboBox triggers where management screens need the same visual language.
/// The caller retains its existing option values and change handling.
pub(crate) fn management_dropdown<'a>(
    ui:&mut Ui,label:impl Into<DropdownCaption<'a>>,width:f32,enabled:bool,
    options:impl FnOnce(&mut Ui)
)->Response {
    let label=label.into();
    management_dropdown_inner(ui,label.label,label.label,width,enabled,false,None,None,label.placeholder,None,options)
}
/// Same management dropdown master, with an optional colored status mark.
/// This only changes the glyph painting; popup placement, hover and row
/// selection remain owned by the shared management dropdown.
pub(crate) fn management_status_dropdown(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,
    status_color:Color32,options:impl FnOnce(&mut Ui)
)->Response {
    management_dropdown_inner(
        ui,label,label,width,enabled,false,Some(status_color),None,false,None,options
    )
}

/// Compact, two-option menus such as 선불 / 착불 never grow wider than their
/// own trigger. Other selectors may expand to fit longer catalog names.
pub(crate) fn management_dropdown_compact(
    ui:&mut Ui,label:&str,width:f32,enabled:bool,
    options:impl FnOnce(&mut Ui)
)->Response {
    management_dropdown_inner(ui,label,label,width,enabled,true,None,None,false,None,options)
}

/// A mixed label keeps the common trigger, menu, selection and overflow
/// behavior. Callers supply semantic value spans, never a local painter.
pub(crate) fn management_dropdown_values(
    ui:&mut Ui,label:&egui::text::LayoutJob,width:f32,enabled:bool,placeholder:bool,
    options:impl FnOnce(&mut Ui)
)->Response{
    management_dropdown_inner(ui,&label.text,&label.text,width,enabled,false,None,Some(label),placeholder,None,options)
}

/// Shared 34pt management trigger; the hierarchical category popup and
/// ordinary selectors must use the SAME paint, chevron and hover geometry.
pub(crate) fn management_dropdown_trigger<'a>(
    ui:&mut Ui,label:impl Into<DropdownCaption<'a>>,width:f32,enabled:bool,
    indicator:Option<Color32>,
)->Response {
    let label=label.into();
    management_dropdown_trigger_full_label(ui,label.label,label.label,width,enabled,indicator,label.placeholder)
}

pub(crate) fn management_dropdown_trigger_full_label(
    ui:&mut Ui,label:&str,full_label:&str,width:f32,enabled:bool,
    indicator:Option<Color32>,placeholder:bool,
)->Response {
    management_dropdown_trigger_inner(ui,label,full_label,width,enabled,indicator,None,placeholder,None)
}

fn management_dropdown_trigger_inner(
    ui:&mut Ui,label:&str,full_label:&str,width:f32,enabled:bool,
    indicator:Option<Color32>,values:Option<&egui::text::LayoutJob>,placeholder:bool,emphasis:Option<crate::highlight::Role>,
)->Response {
    // Keep exact toolbar/cell width without stretching the subsequent action
    // or exceeding a narrow selector slot.
    let width=width.min(ui.available_width()).max(1.0);
    // Parent Ui may be disabled independently of the explicit enabled flag.
    let interactive=enabled && ui.is_enabled();
    let (fill,ink,stroke)=crate::button::field_action_palette(interactive,theme::text_ink(theme::TextRole::Value,theme::TextState::Normal));
    let arrow_ink=theme::icon_ink(theme::IconRole::Secondary,theme::VisualState::enabled(interactive));
    // Inline modal rows opt into the 36pt form interaction height;
    // standard management toolbars retain their 34pt minimum.
    let height=theme::ACTION_STANDARD_HEIGHT.max(ui.spacing().interact_size.y);
    let trigger=ui.scope(|ui|{
        let mut button=egui::Button::new("")
            .min_size(Vec2::new(width,height))
            .corner_radius(crate::highlight::Geometry::Control.corners());
        if interactive{
            crate::highlight::action_widget(ui,fill,stroke,(false,emphasis));
        }else{
            let (fill,stroke)=crate::highlight::persistent_surface(fill,stroke,emphasis);
            button=button.fill(fill).stroke(stroke);
        }
        ui.add_enabled(enabled,button)
    }).inner;
    let rect=trigger.rect;
    #[cfg(test)]
    crate::ui_test_support::record_component_geometry("dropdown",rect,ui.clip_rect(),Some(height));
    let painter=ui.painter().with_clip_rect(rect);
    let arrow_left=rect.right()-30.0;
    let caption_ink=if placeholder{text_ink(TextRole::Value,TextState::Placeholder)}
        else if interactive{indicator.unwrap_or(text_ink(TextRole::Value,TextState::Normal))}
        else{text_ink(TextRole::Value,TextState::Disabled)};
    let caption=if let Some(values)=values{
        ui.fonts_mut(|fonts|fonts.layout_job(typography::dropdown_value_job(values,false,caption_ink)))
    }else{
        typography::management_control_text_galley(ui,label,caption_ink)
    };
    // Sales status selectors sit in a narrow price column. Reserve the
    // indicator without clipping the longest label ("판매준비중").
    let caption_x=rect.left()+if indicator.is_some(){
        theme::SPACE_24
    }else{theme::SPACE_12};
    let text_clip=Rect::from_min_max(
        egui::pos2(caption_x,rect.top()),
        egui::pos2((arrow_left-theme::SPACE_4).max(caption_x),rect.bottom())
    );
    if let Some(color)=indicator{
        painter.circle_filled(
            egui::pos2(rect.left()+theme::SPACE_12,rect.center().y),
            3.5,color
        );
    }
    let overflow=text_exceeds_rect(caption.size(),text_clip.intersect(ui.clip_rect()));
    painter.with_clip_rect(text_clip).galley(
        egui::pos2(caption_x,
            rect.center().y-caption.size().y/2.0
                +typography::MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y),
        caption,caption_ink
    );
    painter.line_segment(
        [egui::pos2(arrow_left,rect.top()+5.0),
         egui::pos2(arrow_left,rect.bottom()-5.0)],
        Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::border_ink(theme::BorderRole::Divider))
    );
    // Restore the pre-enlargement dropdown chevron dimensions; the date
    // prev/next controls rotate this same shared shape.
    paint_management_chevron_with_color(
        &painter,egui::pos2(rect.right()-15.0,rect.center().y),
        ManagementChevronDirection::Down,arrow_ink
    );
    if interactive {
        crate::floating_notice::remember_response(ui.ctx(),&trigger);
    }
    overflow_tooltip(trigger,overflow||label!=full_label,full_label)
}

/// Common vertical spacing for normal and searchable dropdown menus.
fn configure_option_menu(ui:&mut Ui){
    management_dropdown_option_palette(ui);
    ui.spacing_mut().item_spacing.y=theme::SPACE_4;
}
fn configure_option_rows(ui:&mut Ui){
    ui.spacing_mut().item_spacing.y=theme::SPACE_4;
    ui.spacing_mut().interact_size.y=theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT;
}

fn management_dropdown_inner(
    ui:&mut Ui,label:&str,full_label:&str,width:f32,enabled:bool,
    compact:bool,indicator:Option<Color32>,values:Option<&egui::text::LayoutJob>,placeholder:bool,emphasis:Option<crate::highlight::Role>,
    options:impl FnOnce(&mut Ui)
)->Response {
    let width=width.min(ui.available_width()).max(1.0);
    let trigger=management_dropdown_trigger_inner(
        ui,label,full_label,width,enabled,indicator,values,placeholder,emphasis
    );
    crate::overlay::show_menu(&trigger,None,|menu|{
            // Every selectable row reports its parent trigger, never its
            // popup-row pointer, to the shared action-notice origin.
            menu.apply_shared_width(crate::ui_primitives::CardWidthPolicy::FillAvailable,width);
            if compact {menu.apply_shared_width(crate::ui_primitives::CardWidthPolicy::UpperLimit,width);}
            configure_option_menu(menu);
            // The popup frame owns its 4pt inset. Use full-height selectable
            // rows, so Hangul glyph ink has space above AND below its center.
            // Horizontal auto-shrink is mandatory: forcing a wide ScrollArea
            // caused 90pt 선불/착불 selectors to open ~600pt-wide menus.
            let scroll=scroll_area(ScrollAxes::Vertical)
                .max_height(theme::MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT)
                .auto_shrink([true,true]);
            let scroll=if compact {scroll.max_width(width)}else{scroll};
            scroll.show(menu,|choices|{
                configure_option_rows(choices);
                if compact{choices.apply_shared_width(crate::ui_primitives::CardWidthPolicy::Fixed,width);}
                options(choices);
            });
        });
    trigger
}

/// One search-capable choice for a management single-select dropdown.
/// Its stable ID is the value passed to the existing catalog/filter workflow;
/// search_text can additionally contain localized names and registry aliases.
#[derive(Clone,Debug,PartialEq,Eq)]
pub(crate) struct ManagementSearchChoice {
    pub(crate) value:String,
    pub(crate) label:String,
    pub(crate) search_text:String,
}
impl ManagementSearchChoice {
    pub(crate) fn new(value:String,label:String,search_text:String)->Self{
        Self{value,label,search_text}
    }
}

fn management_search_choice_matches(
    choice:&ManagementSearchChoice,query:&str
)->bool{
    let query=query.trim().to_lowercase();
    query.is_empty()
        ||choice.label.to_lowercase().contains(&query)
        ||choice.search_text.to_lowercase().contains(&query)
}

/// A searchable variant of the existing single-select master. The trigger,
/// palette, selection semantics, 28pt option rows, click-to-close and toast
/// anchor all remain shared; only a 36pt search field is added above the
/// internally scrolling choices. Search never changes the selected value.
pub(crate) fn management_searchable_dropdown<'a>(
    ui:&mut Ui,label:impl Into<DropdownCaption<'a>>,width:f32,enabled:bool,
    current:&mut String,options:&[ManagementSearchChoice],hint:&str,
)->Response{
    let trigger=management_dropdown_trigger(ui,label,width,enabled,None);
    let id=trigger.id.with("management-searchable-dropdown");
    let ctx=ui.ctx().clone();
    let mut search=ctx.data_mut(|data|
        data.get_temp::<String>(id).unwrap_or_default()
    );
    if trigger.clicked(){search.clear();}
    // The search field needs room even on a 148pt toolbar trigger, but
    // long brand names must never stretch the popup or surrounding toolbar.
    let popup_width=width.max(theme::MANAGEMENT_SEARCHABLE_POPUP_MIN_WIDTH)
        .min((ctx.content_rect().width()-theme::SPACE_24).max(1.0));
    crate::overlay::show_menu(&trigger,None,|menu|{
            menu.apply_shared_width(crate::ui_primitives::CardWidthPolicy::Bounded,popup_width);
            configure_option_menu(menu);
            let input_width=menu.available_width().max(1.0);
            management_search_field(menu,&mut search,input_width,hint);
            let visible=options.iter()
                .filter(|choice|management_search_choice_matches(choice,&search))
                .collect::<Vec<_>>();
            scroll_area(ScrollAxes::Vertical)
                .id_salt("management-searchable-options")
                .max_height(theme::MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT)
                .auto_shrink([false,true])
                .show(menu,|list|{
                    configure_option_rows(list);
                    if visible.is_empty(){
                        management_body_state_inline(list,"검색 결과가 없습니다.");
                    }else{
                        for choice in visible{
                            management_dropdown_option(
                                list,current,choice.value.clone(),&choice.label
                            );
                        }
                    }
                });
        });
    ctx.data_mut(|data|data.insert_temp(id,search));
    trigger
}

/// All menu options share native selection, optical text alignment and
/// notification/close behavior. Compact rows retain exact-width allocation;
/// status rows retain their indicator dot and label inset.
#[derive(Clone,Copy)]
enum ManagementDropdownOptionStyle {
    Standard,
    Status(Color32),
    Compact(f32),
}

fn management_dropdown_option_inner<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str,
    style:ManagementDropdownOptionStyle,values:Option<&egui::text::LayoutJob>
)->Response{
    // Invisible native label reserves the same semantic glyph widths as
    // the visible label while preserving native selection and keyboard input.
    let invisible:egui::WidgetText=if let Some(values)=values{
        typography::dropdown_value_job(values,true,Color32::TRANSPARENT).into()
    }else{
        typography::invisible_option_label(label).into()
    };
    let selected=*current==value;
    let response=match style{
        ManagementDropdownOptionStyle::Compact(width)=>{
            let mut response=ui.add_sized(
                Vec2::new(width,theme::MANAGEMENT_DROPDOWN_OPTION_HEIGHT),
                egui::Button::selectable(selected,invisible)
            );
            if response.clicked() && *current!=value{
                *current=value;
                response.mark_changed();
            }
            response
        }
        _=>ui.selectable_value(current,value,invisible),
    };
    let row=response.rect;
    let painter=ui.painter().with_clip_rect(row.shrink(2.0));
    #[cfg(test)]
    crate::ui_test_support::record_highlight_target("dropdown option",row,ui.clip_rect());
    let caption_ink=match style{
        ManagementDropdownOptionStyle::Status(color)=>color,
        _ if selected=>theme::ORANGE_EMPHASIS_TEXT,
        _=>theme::text_ink(theme::TextRole::Value,theme::TextState::Normal),
    };
    let galley=if let Some(values)=values{
        ui.fonts_mut(|fonts|fonts.layout_job(typography::dropdown_value_job(values,true,caption_ink)))
    }else{
        ui.painter().layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
            caption_ink,
        )
    };
    let x=match style{
        ManagementDropdownOptionStyle::Standard=>row.left()+theme::SPACE_8,
        ManagementDropdownOptionStyle::Compact(_)=>row.center().x-galley.size().x*0.5,
        ManagementDropdownOptionStyle::Status(color)=>{
            painter.circle_filled(
                egui::pos2(row.left()+theme::SPACE_12,row.center().y),4.0,color
            );
            row.left()+theme::SPACE_24
        }
    };
    painter.galley(
        egui::pos2(
            x,row.top()+typography::management_dropdown_option_text_y(
                row.height(),galley.size().y
            ),
        ),
        galley,caption_ink,
    );
    if response.clicked(){
        crate::floating_notice::dropdown_option_selected(ui.ctx());
        ui.close();
    }
    response
}

pub(crate) fn management_dropdown_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str
)->Response {
    management_dropdown_option_inner(ui,current,value,label,
        ManagementDropdownOptionStyle::Standard,None)
}

pub(crate) fn management_dropdown_option_values<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&egui::text::LayoutJob
)->Response{
    management_dropdown_option_inner(ui,current,value,&label.text,
        ManagementDropdownOptionStyle::Standard,Some(label))
}

pub(crate) fn management_status_dropdown_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str,color:Color32
)->Response {
    management_dropdown_option_inner(ui,current,value,label,
        ManagementDropdownOptionStyle::Status(color),None)
}

pub(crate) fn management_dropdown_compact_option<T:PartialEq>(
    ui:&mut Ui,current:&mut T,value:T,label:&str,width:f32
)->Response {
    management_dropdown_option_inner(ui,current,value,label,
        ManagementDropdownOptionStyle::Compact(width),None)
}


// Shared controls and presentation profiles.
/// Do not swap the COD selector for a textual caption during a Rust save.
/// The shared dropdown keeps its width, baseline, outline and chevron in
/// BOTH states; only hit-testing is disabled while another edit is pending.
pub(crate) struct CodState{editable:bool,review:bool}
impl From<bool> for CodState{
    fn from(editable:bool)->Self{Self{editable,review:false}}
}
impl From<(bool,bool)> for CodState{
    fn from((editable,review):(bool,bool))->Self{Self{editable,review}}
}
pub(crate) fn reception_cod_dropdown(
    ui:&mut Ui,collect:Option<bool>,state:impl Into<CodState>
)->(bool,Response){
    let CodState{editable,review}=state.into();
    let mut selected=collect.unwrap_or(false);
    let label=match collect {
        Some(true)=>"착불",
        Some(false)=>"선불",
        None if editable=>"선불",
        None=>"—",
    };
    let response=management_dropdown_inner(ui,label,label,90.0,editable,true,None,None,false,
        crate::highlight::freight_role(false,review),|menu|{
        management_dropdown_compact_option(menu,&mut selected,false,"선불",90.0);
        management_dropdown_compact_option(menu,&mut selected,true,"착불",90.0);
    });
    (editable && selected!=collect.unwrap_or(false),response)
}

#[cfg(test)]
mod regression_tests;
