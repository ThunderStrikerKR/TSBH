//! Shared date selection, segmented navigation and recipient/count traversal.
use chrono::{Datelike,Duration as Days,Months,NaiveDate};
use eframe::egui::{self,Align,Color32,Layout,Rect,Response,Sense,Stroke,Ui,Vec2};
#[cfg(test)]
use eframe::egui::UiBuilder;
use crate::{theme,typography};
use crate::ui_primitives::{date_navigation_segmented_surface,navigation_segment_child,ManagementChevronDirection,paint_management_chevron_with_color,truncated_label_for_explicit_tooltip,overflow_tooltip};
use crate::button::{date_navigation_segment_button,date_navigation_button_surface,DateNavigationSegmentGlyph};
use crate::input::management_date_text_field_impl;
#[cfg(test)]
use crate::{ui_test_support::test_context,ui_primitives::form_field_label};

/// Clamp the selected recipient and expose navigation availability separately
/// from header visibility. A single recipient still has a visible 1/1 header;
/// only its previous/next controls are disabled.
fn reception_recipient_navigation_state(
    index:usize,total:usize,navigation_enabled:bool
)->Option<(usize,bool,bool)>{
    if total==0{return None;}
    let current=index.min(total-1);
    Some((
        current,
        navigation_enabled && current>0,
        navigation_enabled && current+1<total
    ))
}

#[cfg(test)]
mod reception_recipient_navigation_state_tests{
    use super::*;

    #[test]
    fn one_recipient_keeps_name_and_progress_with_both_arrows_disabled(){
        assert_eq!(
            reception_recipient_navigation_state(0,1,true),
            Some((0,false,false))
        );
    }

    #[test]
    fn multiple_recipients_enable_only_the_available_direction(){
        assert_eq!(
            reception_recipient_navigation_state(0,2,true),
            Some((0,false,true))
        );
        assert_eq!(
            reception_recipient_navigation_state(1,2,true),
            Some((1,true,false))
        );
    }

    #[test]
    fn busy_or_pending_work_disables_only_the_arrows(){
        assert_eq!(
            reception_recipient_navigation_state(0,1,false),
            Some((0,false,false))
        );
        assert_eq!(
            reception_recipient_navigation_state(0,3,false),
            Some((0,false,false))
        );
        assert_eq!(
            reception_recipient_navigation_state(1,3,false),
            Some((1,false,false))
        );
    }

    #[test]
    fn empty_recipient_list_has_no_navigator(){
        assert_eq!(reception_recipient_navigation_state(0,0,true),None);
    }
}

/// Navigate one recipient from the 34pt section HEADER. The name and 1/N
/// progress remain visible for one shipment and while work is pending; only
/// the two directional segments become disabled.
pub fn reception_recipient_navigator(
    ui:&mut Ui,name:&str,index:usize,total:usize,navigation_enabled:bool
)->Option<usize>{
    let (current,previous_enabled,next_enabled)=
        reception_recipient_navigation_state(index,total,navigation_enabled)?;
    let wanted=theme::RECEPTION_RECIPIENT_NAV_WIDTH;
    let width=ui.available_width().min(wanted).max(0.0);
    if width<2.0*theme::RECEPTION_RECIPIENT_NAV_ARROW_WIDTH
        +theme::RECEPTION_RECIPIENT_NAV_COUNT_WIDTH
        +2.0*theme::FRAME_STROKE_WIDTH
    {return None;}
    let height=theme::SECTION_HEADER_HEIGHT;
    let arrow_width=theme::RECEPTION_RECIPIENT_NAV_ARROW_WIDTH;
    let count_width=theme::RECEPTION_RECIPIENT_NAV_COUNT_WIDTH;
    let name_width=(width-2.0*theme::FRAME_STROKE_WIDTH
        -2.0*arrow_width-count_width).max(0.0);
    let (_,selected)=date_navigation_segmented_surface(
        ui,"reception-recipient-nav-header",Vec2::new(width,height),
        |segments,inner|{
            let mut selected=None;
            segments.set_width(inner.width());
            segments.set_height(inner.height());
            segments.spacing_mut().item_spacing=Vec2::ZERO;

            let previous=date_navigation_segment_button(
                segments,arrow_width,previous_enabled,
                DateNavigationSegmentGlyph::Chevron(
                    ManagementChevronDirection::Left
                )
            );
            if previous.clicked(){selected=Some(current-1);}

            let fallback=format!("수취인 {}",current+1);
            let display=if name.trim().is_empty(){fallback.as_str()}else{name};
            let natural_width=segments.painter().layout_no_wrap(
                display.to_owned(),
                egui::FontId::proportional(typography::FONT_BODY),
                theme::TEXT
            ).size().x;
            let content_width=(name_width-2.0*theme::SPACE_8).max(0.0);
            let clipped=natural_width>content_width;
            let label=segments.add_sized(
                Vec2::new(name_width,inner.height()),
                truncated_label_for_explicit_tooltip(typography::body(display))
                    .halign(Align::Center)
                    .sense(if clipped{Sense::click()}else{Sense::hover()})
            );
            let label=overflow_tooltip(label,clipped,display);
            if clipped{
                crate::overlay::show_menu(&label,None,|popup|{
            popup.set_max_width(320.0);
                        popup.add(egui::Label::new(typography::body(display)).wrap());
                    });
            }

            let next=date_navigation_segment_button(
                segments,arrow_width,next_enabled,
                DateNavigationSegmentGlyph::Chevron(
                    ManagementChevronDirection::Right
                )
            );
            if next.clicked(){selected=Some(current+1);}

            segments.add_sized(
                Vec2::new(count_width,inner.height()),
                egui::Label::new(typography::field_label(
                    &format!("{}/{}",current+1,total)
                )).halign(Align::Center)
            );
            paint_navigation_vertical_rules(segments,inner,[
                inner.left()+arrow_width,
                inner.left()+arrow_width+name_width,
                inner.left()+2.0*arrow_width+name_width
            ],theme::DIVIDER);
            selected
        }
    );
    selected
}

/// Shared date-picker calendar from the approved Order History screen.
/// Use the SAME popup, month navigation, selected-day style and 7-column
/// grid for all management date selectors; each caller supplies its own ID.
pub fn calendar_month_start(date: NaiveDate)->NaiveDate{
    NaiveDate::from_ymd_opt(date.year(),date.month(),1).unwrap_or(date)
}

pub fn shift_calendar_month(month:NaiveDate,offset:i32)->NaiveDate{
    let first=calendar_month_start(month);
    if offset>=0{
        first.checked_add_months(Months::new(offset as u32))
    }else{
        first.checked_sub_months(Months::new(offset.unsigned_abs()))
    }.unwrap_or(first)
}

pub fn calendar_dates(month:NaiveDate)->Vec<Option<NaiveDate>>{
    let first=calendar_month_start(month);
    let leading=first.weekday().num_days_from_monday() as usize;
    let next=shift_calendar_month(first,1);
    let days=next.pred_opt().map_or(31,|end|end.day() as usize);
    let visible=((leading+days+6)/7)*7;
    (0..visible).map(|index|{
        (index>=leading && index<leading+days)
            .then(||first.checked_add_signed(Days::days((index-leading) as i64)))
            .flatten()
    }).collect()
}

/// One 36pt allocation per date group. All children share the 34pt inset
/// and cannot extend hover painting into the outer rounded frame.
pub fn management_date_group<R>(
    ui:&mut Ui,id:&'static str,width:f32,
    add_contents:impl FnOnce(&mut Ui,Rect)->R
)->R{
    date_navigation_segmented_surface(
        ui,("management-date-group",id),Vec2::new(width,theme::INPUT_HEIGHT),
        add_contents
    ).1
}

fn paint_navigation_vertical_rules(
    ui:&Ui,inner:Rect,positions:impl IntoIterator<Item=f32>,color:Color32
){
    for x in positions{
        ui.painter().line_segment(
            [egui::pos2(x,inner.top()+4.0),egui::pos2(x,inner.bottom()-4.0)],
            Stroke::new(theme::SOFT_DIVIDER_WIDTH,color)
        );
    }
}

fn date_navigation_count_step_state(
    value:&str,current:usize,enabled:bool,minimum:usize,maximum:usize
)->(bool,bool){
    let step_count=value.parse::<usize>().unwrap_or(current);
    (
        enabled && step_count>minimum,
        enabled && step_count<maximum
    )
}

/// The editable delivery count is one segmented date-selector control:
/// minus | centered numeric editor | plus. The parent frame owns one border.
pub struct DateNavigationCountControlResponse{
    pub decrement_clicked:bool,
    pub increment_clicked:bool,
    pub input:Response,
    #[cfg(test)]
    pub rect:Rect,
    #[cfg(test)]
    decrement_rect:Rect,
    #[cfg(test)]
    increment_rect:Rect,
}

pub fn date_navigation_count_control(
    ui:&mut Ui,id_salt:impl std::hash::Hash,value:&mut String,
    current:usize,enabled:bool,minimum:usize,maximum:usize
)->DateNavigationCountControlResponse{
    let enabled=enabled && ui.is_enabled();
    let (decrement_enabled,increment_enabled)=date_navigation_count_step_state(
        value,current,enabled,minimum,maximum
    );
    let arrow_width=theme::CALENDAR_NAV_SIZE;
    let input_width=64.0;
    // The inline date group (not the small popup calendar buttons) is the
    // master: 36pt outer height and 34pt interior.
    let height=theme::INPUT_HEIGHT;
    let width=2.0*arrow_width+input_width+2.0*theme::FRAME_STROKE_WIDTH;
    let (_rect,(decrement,input,increment))=
        date_navigation_segmented_surface(
            ui,id_salt,Vec2::new(width,height),|segments,inner|{
                let x1=inner.left()+arrow_width;
                let x2=inner.right()-arrow_width;
                let left=Rect::from_min_max(inner.min,egui::pos2(x1,inner.bottom()));
                let center=Rect::from_min_max(egui::pos2(x1,inner.top()),egui::pos2(x2,inner.bottom()));
                let right=Rect::from_min_max(egui::pos2(x2,inner.top()),inner.max);
                // Every segment gets its own fixed rectangle and ID. TextEdit
                // margins or a disabled button cannot shift the next segment.
                let mut minus=navigation_segment_child(segments,"count-minus",left,Layout::top_down(Align::Center));
                let decrement=date_navigation_segment_button(
                    &mut minus,arrow_width,
                    decrement_enabled,DateNavigationSegmentGlyph::CountStep{increase:false}
                );
                let mut entry=navigation_segment_child(segments,"count-value",center,Layout::centered_and_justified(egui::Direction::TopDown));
                entry.spacing_mut().interact_size=Vec2::ZERO;
                let hint=format!("{minimum}~{maximum}");
                let input=entry.add_enabled_ui(enabled,|entry|{
                    management_date_text_field_impl(
                        entry,value,center.width(),center.height(),hint.as_str(),Some(3)
                    )
                }).inner;
                let mut plus=navigation_segment_child(segments,"count-plus",right,Layout::top_down(Align::Center));
                let increment=date_navigation_segment_button(
                    &mut plus,arrow_width,
                    increment_enabled,DateNavigationSegmentGlyph::CountStep{increase:true}
                );
                paint_navigation_vertical_rules(segments,inner,[
                    inner.left()+arrow_width,
                    inner.left()+arrow_width+input_width
                ],theme::BORDER);
                (decrement,input,increment)
            }
        );
    DateNavigationCountControlResponse{
        decrement_clicked:decrement.clicked(),input,increment_clicked:increment.clicked(),
        #[cfg(test)]
        rect:_rect,
        #[cfg(test)]
        decrement_rect:decrement.rect,
        #[cfg(test)]
        increment_rect:increment.rect,
    }
}

/// Shared calendar navigation: identical 28pt hit targets, border, radius,
/// and vector chevrons for year/month movement in either direction.
fn calendar_nav_button(
    ui:&mut Ui,direction:ManagementChevronDirection,
    year:bool)->bool{
    let (response,ink)=date_navigation_button_surface(ui,true);
    let painter=ui.painter().with_clip_rect(response.rect);
    if year{
        for offset in [-3.8,3.8]{
            paint_management_chevron_with_color(
                &painter,response.rect.center()+egui::vec2(offset,0.0),
                direction,ink
            );
        }
    }else{
        paint_management_chevron_with_color(
            &painter,response.rect.center(),direction,ink
        );
    }
    response.clicked()
}

/// A single popup for Order History and Excel export history. The header
/// has the exact width of the seven-column date grid, so the month label
/// and four navigation buttons remain aligned at every month length.
pub fn date_calendar(
    trigger:&egui::Response,month:&mut NaiveDate,selected:&str,
    grid_id:&'static str
)->Option<NaiveDate>{
    let mut chosen=None;
    let selected_date=NaiveDate::parse_from_str(selected,"%Y-%m-%d").ok();
    let today=(chrono::Utc::now()+Days::hours(9)).date_naive();
    crate::overlay::show_menu(trigger,None,|calendar|{
            calendar.spacing_mut().item_spacing=egui::vec2(
                theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
            );
            calendar.set_min_width(theme::CALENDAR_GRID_WIDTH);
            calendar.horizontal(|nav|{
                nav.spacing_mut().item_spacing.x=theme::CALENDAR_GRID_GAP;
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,true){
                    *month=shift_calendar_month(*month,-12);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Left,false){
                    *month=shift_calendar_month(*month,-1);
                }
                let title_width=theme::CALENDAR_GRID_WIDTH
                    -4.0*theme::CALENDAR_NAV_SIZE
                    -4.0*theme::CALENDAR_GRID_GAP;
                nav.add_sized(
                    egui::vec2(title_width,theme::CALENDAR_NAV_SIZE),
                    egui::Label::new(typography::field_label(&format!(
                        "{}년 {}월",month.year(),month.month()
                    ))).halign(Align::Center)
                );
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,false){
                    *month=shift_calendar_month(*month,1);
                }
                if calendar_nav_button(
                    nav,ManagementChevronDirection::Right,true){
                    *month=shift_calendar_month(*month,12);
                }
            });
            calendar.add_space(theme::SPACE_8);
            crate::ui_primitives::soft_divider(calendar);
            calendar.add_space(theme::SPACE_4);
            let width=theme::CALENDAR_DAY_SIZE;
            let dates=calendar_dates(*month);
            egui::Grid::new(grid_id)
                .num_columns(7)
                .spacing(egui::vec2(
                    theme::CALENDAR_GRID_GAP,theme::CALENDAR_GRID_GAP
                ))
                .show(calendar,|grid|{
                    for day in ["월","화","수","목","금","토","일"]{
                        grid.add_sized(
                            egui::vec2(width,width),
                            egui::Label::new(typography::caption(day))
                                .halign(Align::Center),
                        );
                    }
                    grid.end_row();
                    for (index,day) in dates.iter().enumerate(){
                        if let Some(date)=day{
                            let response=crate::button::calendar_day(grid,&date.day().to_string(),width,selected_date==Some(*date),*date==today);
                            if response.clicked(){
                                chosen=Some(*date);
                                grid.close();
                            }
                        }else{
                            grid.allocate_space(egui::vec2(width,width));
                        }
                        if index%7==6{grid.end_row();}
                    }
                });
        });
    chosen
}

#[cfg(test)]
mod date_navigation_segmented_control_tests{
    use super::*;

    #[test]
    fn shared_navigation_child_retains_id_rect_clip_and_parent_cursor(){
        let ctx=test_context();
        let mut observed=Vec::new();
        for shared in [false,true]{
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    let (rect,_)=ui.allocate_exact_size(Vec2::new(112.0,36.0),Sense::hover());
                    let slot=rect.shrink(1.0);
                    let before=ui.available_rect_before_wrap().min;
                    let child=if shared{
                        navigation_segment_child(ui,"geometry",slot,Layout::top_down(Align::Center))
                    }else{
                        let mut child=ui.new_child(UiBuilder::new().id_salt("geometry")
                            .max_rect(slot).layout(Layout::top_down(Align::Center)));
                        child.set_clip_rect(slot.intersect(ui.clip_rect()));
                        child
                    };
                    observed.push((child.id(),child.max_rect(),child.clip_rect(),before,
                        ui.available_rect_before_wrap().min));
                });
            });
        }
        assert_eq!(observed[0],observed[1]);
        assert_eq!(observed[1].3,observed[1].4);
    }


    #[test]
    fn shipment_count_is_one_outer_frame_with_three_adjacent_segments(){
        let ctx=test_context();
        let mut value="2".to_owned();
        let mut rendered=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                rendered=Some(date_navigation_count_control(
                    ui,"test-shipment-count",&mut value,2,true,1,100
                ));
            });
        });
        let control=rendered.expect("count control must render");
        assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
        assert_eq!(control.input.rect.height(),theme::ACTION_STANDARD_HEIGHT);
        assert_eq!(control.input.rect.width(),64.0);
        let expected_input_left=control.rect.left()
            +theme::FRAME_STROKE_WIDTH+theme::CALENDAR_NAV_SIZE;
        assert!(
            (control.input.rect.left()-expected_input_left).abs()<0.5,
            "numeric editor must sit directly between the two 28pt arrow segments"
        );
        let expected_input_right=control.rect.right()
            -theme::FRAME_STROKE_WIDTH-theme::CALENDAR_NAV_SIZE;
        assert!(
            (control.input.rect.right()-expected_input_right).abs()<0.5,
            "the plus segment must directly follow the numeric editor"
        );
    }

    #[test]
    fn shipment_count_segments_do_not_overflow_in_actual_horizontal_row(){
        for (value,enabled) in [("1",true),("50",true),("100",true),("1",false)]{
            let ctx=test_context();
            let mut value=value.to_owned();
            let current=value.parse().unwrap();
            let mut rendered=None;
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.horizontal(|row|{
                        let label=form_field_label(row,"배송 건수");
                        let control=date_navigation_count_control(
                            row,"count-geometry",&mut value,current,enabled,1,100
                        );
                        let unit=form_field_label(row,"건");
                        rendered=Some((control,label.rect,unit.rect));
                    });
                });
            });
            let (control,label,unit)=rendered.unwrap();
            assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
            assert_eq!(label.height(),control.rect.height());
            assert_eq!(unit.height(),control.rect.height());
            assert!((label.center().y-control.rect.center().y).abs()<0.5);
            assert!((unit.center().y-control.rect.center().y).abs()<0.5);
            let inner=control.rect.shrink(theme::FRAME_STROKE_WIDTH);
            for segment in [control.decrement_rect,control.input.rect,control.increment_rect]{
                assert!(inner.expand(0.5).contains(segment.min)
                    && inner.expand(0.5).contains(segment.max),
                    "segment outside outer frame: {segment:?}, inner={inner:?}");
                assert!((segment.center().y-inner.center().y).abs()<0.6,
                    "segment is vertically off-center");
            }
            assert!((control.decrement_rect.right()-control.input.rect.left()).abs()<0.6);
            assert!((control.input.rect.right()-control.increment_rect.left()).abs()<0.6);
            assert!(unit.left()>=control.rect.right()-0.5,
                "parent cursor must advance exactly once past the control");
        }
    }

    #[test]
    fn count_step_symbols_are_centered_vector_strokes_in_the_rendered_control(){
        let ctx=test_context();
        let mut value="2".to_owned();
        let mut rendered=None;
        let output=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.horizontal(|row|{
                    form_field_label(row,"배송 건수");
                    rendered=Some(date_navigation_count_control(
                        row,"count-symbols",&mut value,2,true,1,100
                    ));
                    form_field_label(row,"건");
                });
            });
        });
        let control=rendered.unwrap();
        let minus=control.decrement_rect.center();
        let plus=control.increment_rect.center();
        let expected=[
            [minus-egui::vec2(4.0,0.0),minus+egui::vec2(4.0,0.0)],
            [plus-egui::vec2(4.0,0.0),plus+egui::vec2(4.0,0.0)],
            [plus-egui::vec2(0.0,4.0),plus+egui::vec2(0.0,4.0)],
        ];
        for endpoints in expected{
            assert!(output.shapes.iter().any(|paint|
                matches!(&paint.shape,egui::Shape::LineSegment{points,stroke}
                    if *points==endpoints && (stroke.width-1.8).abs()<0.01
                        && stroke.color==theme::TEXT)
            ),"missing centered count symbol stroke: {endpoints:?}");
        }
        let source=include_str!("navigation.rs");
        let count=source.split("pub fn date_navigation_count_control(").nth(1).unwrap()
            .split("/// Shared calendar navigation").next().unwrap();
        assert!(count.contains("management_date_text_field_impl("));
        assert!(!count.contains("numeric_editor_font("));
        assert_eq!(control.rect.height(),theme::INPUT_HEIGHT);
    }

    #[test]
    fn count_stepper_palette_distinguishes_actions_from_editor_and_disabled_end_stops(){
        let (_,fill,ink,_)=crate::button::action_style(theme::ButtonRole::Standard,true,false);
        assert_eq!((fill,ink),(theme::RAISED,theme::TEXT));
        let (_,fill,ink,_)=crate::button::action_style(theme::ButtonRole::Standard,false,false);
        assert_eq!((fill,ink),(theme::DISABLED_ACTION_BG,theme::DISABLED_ACTION_TEXT));
        assert!(theme::RAISED.r()>theme::INPUT.r());
        assert!(theme::INPUT.r()>theme::DISABLED_ACTION_BG.r());
        assert!(theme::TEXT.r()>theme::DISABLED_ACTION_TEXT.r());
    }

    #[test]
    fn count_stepper_bounds_follow_the_visible_draft_value(){
        assert_eq!(date_navigation_count_step_state("1",2,true,1,100),(false,true));
        assert_eq!(date_navigation_count_step_state("100",2,true,1,100),(true,false));
        assert_eq!(date_navigation_count_step_state("",2,true,1,100),(true,true));
        assert_eq!(date_navigation_count_step_state("0",2,true,1,100),(false,true));
        assert_eq!(date_navigation_count_step_state("101",2,true,1,100),(true,false));
        assert_eq!(date_navigation_count_step_state("2",2,false,1,100),(false,false));
    }

    #[test]
    fn single_recipient_still_allocates_name_and_one_of_one_navigator(){
        let ctx=test_context();
        let mut consumed_height=0.0;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let before=ui.cursor().min.y;
                let selected=reception_recipient_navigator(
                    ui,"권기엽",0,1,false
                );
                assert_eq!(selected,None);
                consumed_height=ui.cursor().min.y-before;
            });
        });
        assert!(
            consumed_height>=theme::SECTION_HEADER_HEIGHT-0.5,
            "the 1/1 recipient navigator must remain allocated even with both arrows disabled"
        );
    }
}


// Presentation-only state lifetime. Expiration is checked on re-entry;
// no timer task, persistence, or business mutation is needed.
pub(crate) const NAVIGATION_SNAPSHOT_TTL:std::time::Duration=std::time::Duration::from_secs(30*60);
pub(crate) struct NavigationSnapshots<const N:usize>{
    active:usize,
    left:[Option<std::time::Instant>;N],
    epochs:[u64;N],
}
impl<const N:usize> NavigationSnapshots<N>{
    pub(crate) fn new(active:usize)->Self{Self{active,left:[None;N],epochs:[0;N]}}
    pub(crate) fn visit(&mut self,next:usize,now:std::time::Instant)->bool{
        if next==self.active{return false;}
        self.left[self.active]=Some(now);
        self.active=next;
        let expired=self.left[next].take().is_some_and(|left|
            now.saturating_duration_since(left)>=NAVIGATION_SNAPSHOT_TTL);
        if expired{self.epochs[next]=self.epochs[next].wrapping_add(1);}
        expired
    }
    pub(crate) fn epoch(&self,index:usize)->u64{self.epochs[index]}
}
#[cfg(test)]
mod navigation_snapshot_tests{
    use super::*;
    #[test]
    fn expiry_is_per_page_and_starts_on_departure(){
        let t=std::time::Instant::now();
        let seconds=std::time::Duration::from_secs;
        let mut cache=NavigationSnapshots::<8>::new(0);
        assert!(!cache.visit(0,t+seconds(7200)));
        assert!(!cache.visit(1,t+seconds(7200)));
        assert!(!cache.visit(0,t+seconds(8999)));
        assert_eq!(cache.epoch(0),0);
        assert!(!cache.visit(2,t+seconds(9000)));
        assert!(cache.visit(0,t+seconds(10800)));
        assert_eq!(cache.epoch(0),1);
        assert!(cache.visit(1,t+seconds(10801)));
        assert_eq!(cache.epoch(1),1);
        assert!(cache.visit(2,t+seconds(10802)));
        assert_eq!(cache.epoch(2),1);
    }
}
