//! One reusable drill-down category selector for egui management screens.
//! Presentation-only: selection writes an existing category ID, never a new
//! category, product edit, or implicit business transaction.
use std::collections::{HashMap,HashSet};
use eframe::egui::{self,Align,Align2,Color32,Context,Id,Response,Sense,Stroke,Ui,Vec2};
use serde_json::Value;
use crate::{category_manager::ROOTS,components,floating_notice,theme,typography};

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct CategoryNode {
    pub id:String,
    pub name:String,
    pub parent:String,
}
#[derive(Clone,Debug,Default)]
struct ExplorerState {
    path:Vec<String>,
    search:String,
    ancestors_open:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum CrumbVisual { Ordinary, Current, Applied }

pub fn nodes_from_catalog(catalog:&Value)->Vec<CategoryNode>{
    let mut nodes=catalog.get("categories").and_then(Value::as_array)
        .into_iter().flatten().filter_map(|row|{
            let id=row.get("id").and_then(Value::as_str)?.trim();
            if id.is_empty() || row.get("deletedAt")
                .is_some_and(|value|!value.is_null() && value.as_str()!=Some("")){
                return None;
            }
            Some(CategoryNode {
                id:id.to_owned(),
                name:row.get("name").and_then(Value::as_str)
                    .unwrap_or(id).to_owned(),
                parent:row.get("parentId").and_then(Value::as_str)
                    .unwrap_or("").to_owned(),
            })
        }).collect::<Vec<_>>();
    for (id,name) in ROOTS {
        if !nodes.iter().any(|row|row.id==id){
            nodes.push(CategoryNode{
                id:id.to_owned(),name:name.to_owned(),parent:String::new()
            });
        }
    }
    nodes
}
fn tree_index(nodes:&[CategoryNode])->HashMap<&str,&CategoryNode>{
    nodes.iter().map(|n|(n.id.as_str(),n)).collect()
}
fn chain(index:&HashMap<&str,&CategoryNode>,id:&str)->Vec<String>{
    let mut result=Vec::new();
    let mut seen=HashSet::new();
    let mut current=id;
    while !current.is_empty() && seen.insert(current.to_owned()){
        let Some(row)=index.get(current) else {break};
        result.push(row.id.clone());
        current=&row.parent;
    }
    result.reverse();
    result
}
fn full_name(index:&HashMap<&str,&CategoryNode>,path:&[String])->String{
    path.iter().filter_map(|id|index.get(id.as_str()))
        .map(|node|node.name.as_str()).collect::<Vec<_>>().join(" > ")
}
fn direct_children<'a>(
    nodes:&'a[CategoryNode],index:&HashMap<&str,&CategoryNode>,parent:&str
)->Vec<&'a CategoryNode>{
    let mut result=nodes.iter().filter(|node|
        if parent.is_empty(){
            node.parent.is_empty() || !index.contains_key(node.parent.as_str())
        }else{node.parent==parent}
    ).collect::<Vec<_>>();
    result.sort_by(|a,b|a.name.cmp(&b.name));
    result
}
fn search_results<'a>(
    nodes:&'a[CategoryNode],index:&HashMap<&str,&CategoryNode>,query:&str
)->Vec<&'a CategoryNode>{
    let query=query.trim().to_lowercase();
    if query.is_empty(){return Vec::new();}
    let mut result=nodes.iter().filter(|node|{
        full_name(index,&chain(index,&node.id)).to_lowercase().contains(&query)
    }).collect::<Vec<_>>();
    result.sort_by(|a,b|a.name.cmp(&b.name));
    result
}
// Contract for BOTH search results and ordinary category children.
// A category option is always one 34pt row, regardless of depth or length.
const CATEGORY_RESULT_ROW_HEIGHT:f32=theme::ACTION_STANDARD_HEIGHT;

/// Compose a single horizontal path: muted ancestors and the bright leaf.
/// Overlong paths discard old ancestors first; only if the leaf itself cannot
/// fit do we ellipsize the END of the leaf. Never duplicate the leaf or wrap.
fn fitted_category_row_text<F>(
    path:&[&str],max_width:f32,measure:F
)->(String,String)
where F:Fn(&str)->f32 {
    let Some((leaf,parents))=path.split_last()else{
        return (String::new(),String::new());
    };
    let leaf=*leaf;
    let normal_ancestors=if parents.is_empty(){String::new()}
        else{format!("{} > ",parents.join(" > "))};
    if measure(&normal_ancestors)+measure(leaf)<=max_width{
        return (normal_ancestors,leaf.to_owned());
    }
    if measure(leaf)>max_width{
        if measure("…")>max_width{
            return (String::new(),String::new());
        }
        for (end,_) in leaf.char_indices().rev(){
            if end==0{continue;}
            let candidate=format!("{}…",&leaf[..end]);
            if measure(&candidate)<=max_width{
                return (String::new(),candidate);
            }
        }
        return (String::new(),"…".to_owned());
    }
    // Keep the nearest parents when possible, folding earlier ancestors.
    for keep in (0..parents.len()).rev(){
        let suffix=parents[parents.len()-keep..].join(" > ");
        let ancestors=if suffix.is_empty(){
            "… > ".to_owned()
        }else{format!("… > {suffix} > ")};
        if measure(&ancestors)+measure(leaf)<=max_width{
            return (ancestors,leaf.to_owned());
        }
    }
    (String::new(),leaf.to_owned())
}

/// THE sole category list row renderer, used identically for search and
/// normal navigation. One clipped baseline and one hit rectangle; only the
/// text's ancestor segment differs between the two modes.
fn category_option_row(
    ui:&mut Ui,node:&CategoryNode,
    index:&HashMap<&str,&CategoryNode>,path:&[String],
    width:f32,selected:&str,show_ancestors:bool
)->Response{
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,CATEGORY_RESULT_ROW_HEIGHT),Sense::click()
    );
    if node.id==selected{
        ui.painter().rect(
            rect,theme::CONTROL_RADIUS,theme::CATEGORY_EXPLORER_APPLIED_BG,
            Stroke::new(
                theme::SOFT_DIVIDER_WIDTH,theme::CATEGORY_EXPLORER_APPLIED_BORDER
            ),egui::StrokeKind::Inside
        );
    }else if response.hovered()||response.has_focus(){
        ui.painter().rect_filled(
            rect,theme::CONTROL_RADIUS,theme::MANAGEMENT_DROPDOWN_HOVER_BG
        );
    }
    let font=egui::FontId::proportional(typography::FONT_STANDARD_ACTION);
    let mut labels=if show_ancestors{
        path.iter().filter_map(|id|index.get(id.as_str()))
            .map(|row|row.name.as_str()).collect::<Vec<_>>()
    }else{Vec::new()};
    if !show_ancestors{labels.push(&node.name);}
    // The right chevron has a fixed, separate 16pt reserve. Text length
    // cannot expand the row or alter the popup's fixed 546pt width.
    let available=(rect.width()-theme::SPACE_8*2.0-theme::SPACE_16).max(0.0);
    let measure=|value:&str|ui.painter().layout_no_wrap(
        value.to_owned(),font.clone(),theme::TEXT
    ).size().x;
    let (ancestors,leaf)=fitted_category_row_text(&labels,available,measure);
    let text_left=rect.left()+theme::SPACE_8;
    let painter=ui.painter().with_clip_rect(rect.shrink(1.0));
    let mut x=text_left;
    if !ancestors.is_empty(){
        painter.text(
            egui::pos2(x,rect.center().y),Align2::LEFT_CENTER,&ancestors,
            font.clone(),theme::MUTED
        );
        x+=ui.painter().layout_no_wrap(
            ancestors,font.clone(),theme::MUTED
        ).size().x;
    }
    painter.text(
        egui::pos2(x,rect.center().y),Align2::LEFT_CENTER,leaf,
        font.clone(),theme::TEXT
    );
    painter.text(
        egui::pos2(rect.right()-theme::SPACE_12,rect.center().y),
        Align2::CENTER_CENTER,"›",font,theme::SECONDARY
    );
    response
}

fn crumb(
    ui:&mut Ui,label:&str,width:f32,visual:CrumbVisual
)->Response{
    let (rect,response)=ui.allocate_exact_size(
        Vec2::new(width,theme::CATEGORY_EXPLORER_CRUMB_HEIGHT),Sense::click()
    );
    let hovered=response.hovered()||response.has_focus();
    let (background,border,ink)=match visual{
        CrumbVisual::Applied=>(
            theme::CATEGORY_EXPLORER_APPLIED_BG,
            theme::CATEGORY_EXPLORER_APPLIED_BORDER,theme::TEXT
        ),
        CrumbVisual::Current=>(
            theme::CATEGORY_EXPLORER_CURRENT_BG,
            theme::CATEGORY_EXPLORER_CURRENT_BORDER,theme::TEXT
        ),
        CrumbVisual::Ordinary if hovered=>(
            theme::MANAGEMENT_DROPDOWN_HOVER_BG,
            theme::SELECTION,theme::TEXT
        ),
        CrumbVisual::Ordinary=>(
            Color32::TRANSPARENT,Color32::TRANSPARENT,theme::SECONDARY
        ),
    };
    let stroke=if border==Color32::TRANSPARENT{Stroke::NONE}
        else{Stroke::new(theme::SOFT_DIVIDER_WIDTH,border)};
    let painter=ui.painter().with_clip_rect(rect);
    painter.rect(
        rect,theme::CONTROL_RADIUS,background,stroke,egui::StrokeKind::Inside
    );
    // A breadcrumb is a single category, not a whole path. Keep its first
    // characters on overflow (ending ellipsis), never the trailing suffix
    // produced by fit_category_path's category-leaf algorithm.
    let visible=fit_breadcrumb_label(ui,label,(width-theme::SPACE_8).max(1.0));
    painter.text(
        rect.center(),Align2::CENTER_CENTER,visible,
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),ink
    );
    response.on_hover_text(label)
}
/// A breadcrumb represents ONE category, not a flattened category path.
/// Retain its beginning on overflow; full text is still available on hover.
fn fit_breadcrumb_label(ui:&Ui,name:&str,max_width:f32)->String{
    let font=egui::FontId::proportional(typography::FONT_STANDARD_ACTION);
    let width=|text:&str|ui.painter().layout_no_wrap(
        text.to_owned(),font.clone(),theme::TEXT
    ).size().x;
    if max_width<=0.0{return String::new();}
    if width(name)<=max_width{return name.to_owned();}
    if width("…")>max_width{return String::new();}
    for (end,_) in name.char_indices().rev(){
        if end==0{continue;}
        let candidate=format!("{}…",&name[..end]);
        if width(&candidate)<=max_width{return candidate;}
    }
    "…".to_owned()
}
/// Always expose the last three actual category levels. "전체" is a
/// separate root-navigation control, not counted as a category level.
fn shown_category_count(depth:usize)->usize{depth.min(3)}
/// Category names keep their measured width whenever the footer has room.
/// If three long names exceed the fixed budget, shorten the names (not the
/// popup) while retaining all three clickable breadcrumb slots.
fn fitted_crumb_widths(intrinsic:&[f32],budget:f32)->Vec<f32>{
    if intrinsic.is_empty(){return Vec::new();}
    let min_width=(budget/intrinsic.len() as f32).min(26.0).max(0.0);
    let widths=intrinsic.iter().map(|width|
        width.max(min_width)
    ).collect::<Vec<_>>();
    let sum=widths.iter().sum::<f32>();
    if sum<=budget{return widths;}
    let flexible=widths.iter().map(|width|width-min_width).sum::<f32>();
    if flexible<=0.0{return vec![min_width;intrinsic.len()];}
    let remaining=(budget-min_width*intrinsic.len() as f32).max(0.0);
    widths.iter().map(|width|
        min_width+(width-min_width)*remaining/flexible
    ).collect()
}
fn crumbs_row(
    ui:&mut Ui,state:&mut ExplorerState,index:&HashMap<&str,&CategoryNode>,
    selected:&mut String,enabled:bool,allow_empty:bool,changed:&mut bool
){
    let path=state.path.clone();
    let len=path.len();
    let row_width=ui.available_width().max(1.0);
    // Keep three explicit category levels at every depth. Earlier levels
    // collapse into the ancestor selector only from the fourth category.
    let shown_count=shown_category_count(len);
    let hidden=len-shown_count;
    let visible=path[hidden..].to_vec();
    let apply_width=70.0;
    // Include the 4pt gap between controls and all separator labels.
    // The Primary action's reserved slot never gets displaced by a name.
    let separators=visible.len()+usize::from(hidden>0);
    let widget_count=2+visible.len()+2*separators+usize::from(hidden>0);
    let fixed=42.0+if hidden>0{31.0}else{0.0}
        +separators as f32*10.0+apply_width+theme::SPACE_8
        +widget_count.saturating_sub(1) as f32*theme::SPACE_4;
    let budget=(row_width-fixed).max(0.0);
    let intrinsic=visible.iter().map(|id|{
        let name=index.get(id.as_str()).map(|node|node.name.as_str())
            .unwrap_or("분류 없음");
        let galley=ui.painter().layout_no_wrap(
            name.to_owned(),
            egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
            theme::TEXT
        );
        galley.size().x+theme::SPACE_16
    }).collect::<Vec<_>>();
    let crumb_widths=fitted_crumb_widths(&intrinsic,budget);
    let mut go_to:Option<Vec<String>>=None;
    let mut toggle_ancestors=false;
    let mut apply=false;
    ui.horizontal(|bar|{
        bar.spacing_mut().item_spacing.x=theme::SPACE_4;
        let role=if selected.is_empty() && allow_empty {
            CrumbVisual::Applied
        }else if path.is_empty(){CrumbVisual::Current}
            else{CrumbVisual::Ordinary};
        if crumb(bar,"전체",42.0,role).clicked(){
            go_to=Some(Vec::new());
        }
        if hidden>0{
            bar.label(typography::caption("›"));
            if crumb(bar,"···",31.0,CrumbVisual::Ordinary).clicked(){
                toggle_ancestors=true;
            }
        }
        for (offset,id) in visible.iter().enumerate(){
            bar.label(typography::caption("›"));
            let name=index.get(id.as_str()).map(|n|n.name.as_str())
                .unwrap_or("분류 없음");
            let role=if id.as_str()==selected.as_str(){CrumbVisual::Applied}
                else if offset+hidden+1==len{CrumbVisual::Current}
                else{CrumbVisual::Ordinary};
            if crumb(bar,name,crumb_widths[offset],role).clicked()
                && role!=CrumbVisual::Current{
                go_to=Some(path[..hidden+offset+1].to_vec());
            }
        }
        let spacer=(bar.available_width()-apply_width-theme::SPACE_4).max(0.0);
        if spacer>0.0 {bar.add_space(spacer);}
        if theme::action(
            bar,"적용",theme::ButtonRole::Primary,
            enabled && (allow_empty || !path.is_empty())
                && path.last().map(String::as_str).unwrap_or("")!=selected.as_str()
        ).clicked(){apply=true;}
    });
    if toggle_ancestors{state.ancestors_open=!state.ancestors_open;}
    if let Some(next)=go_to{
        state.path=next;state.search.clear();state.ancestors_open=false;
    }
    if apply{
        *selected=state.path.last().cloned().unwrap_or_default();
        *changed=true;
        floating_notice::dropdown_option_selected(ui.ctx());
        ui.close();
    }
}
fn menu_body(
    menu:&mut Ui,nodes:&[CategoryNode],index:&HashMap<&str,&CategoryNode>,
    state:&mut ExplorerState,selected:&mut String,enabled:bool,
    allow_empty:bool,changed:&mut bool,trigger_rect:egui::Rect,popup_width:f32,
){
    floating_notice::dropdown_popup_trigger(menu.ctx(),trigger_rect);
    // Explicitly fix the menu's content rect. Text length and breadcrumb
    // count may change internal allocation but never the popup width.
    menu.set_min_width(popup_width);
    menu.set_max_width(popup_width);
    menu.set_width(popup_width);
    menu.spacing_mut().item_spacing.y=theme::SPACE_8;
    let available=menu.available_width().max(1.0);
    let search_response=components::management_search_field(
        menu,&mut state.search,available,"카테고리 이름 또는 경로 검색"
    );
    let search_has_focus=search_response.has_focus();
    // Browsing arrows must not override keyboard editing in the search field.
    if !search_has_focus && menu.input(|i|i.key_pressed(egui::Key::ArrowLeft)){
        state.path.pop();state.ancestors_open=false;
    }
    menu.horizontal(|nav|{
        let can_go_back=!state.path.is_empty();
        if theme::action(nav,"← 상위",theme::ButtonRole::Standard,can_go_back)
            .clicked(){
            state.path.pop();state.search.clear();state.ancestors_open=false;
        }
        nav.with_layout(egui::Layout::right_to_left(Align::Center),|right|{
            right.label(typography::caption(&format!(
                "{}단계",state.path.len()
            )));
        });
    });
    components::soft_divider(menu);
    let searching=!state.search.trim().is_empty();
    let items=if searching{
        search_results(nodes,index,&state.search)
    }else{
        direct_children(
            nodes,index,state.path.last().map(String::as_str).unwrap_or("")
        )
    };
    menu.horizontal(|heading|{
        heading.label(typography::caption(if searching{
            "카테고리 검색 결과"
        }else{"하위 카테고리"}));
        heading.label(typography::caption(&format!("{}개",items.len())));
    });
    // Stable six-row viewport in search, navigation and empty states.
    // Reduce it only if the OS viewport cannot physically fit the popup.
    let list_height=theme::CATEGORY_EXPLORER_LIST_MAX_HEIGHT
        .min((menu.ctx().screen_rect().height()-270.0).max(100.0));
    let mut next_path=None;
    egui::ScrollArea::vertical()
        .id_salt(("category-explorer-list",state.path.clone(),state.search.clone()))
        .max_height(list_height)
        // Always reserve all six rows, even if the category list is empty.
        // The seventh row and beyond scroll inside this area only.
        .auto_shrink([false,false])
        .show(menu,|list|{
            list.spacing_mut().item_spacing.y=theme::SPACE_4;
            let list_width=list.available_width().max(1.0);
            if items.is_empty(){
                list.label(typography::caption(if searching{
                    "검색 결과가 없습니다."
                }else{"하위 카테고리가 없습니다."}));
            }
            for node in items{
                let path=chain(index,&node.id);
                let full=full_name(index,&path);
                let response=category_option_row(
                    list,node,index,&path,list_width,selected,searching
                ).on_hover_text(&full);
                if response.clicked(){next_path=Some(path);}
            }
        });
    if let Some(path)=next_path{
        state.path=path;
        state.search.clear();
        state.ancestors_open=false;
    }
    if state.ancestors_open && state.path.len()>3{
        egui::Frame::new()
            .fill(theme::INPUT)
            .stroke(Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::BORDER))
            .corner_radius(theme::CONTROL_RADIUS)
            .inner_margin(theme::SPACE_8)
            .show(menu,|ancestor_menu|{
                ancestor_menu.label(typography::caption("생략된 상위 경로"));
                let path=state.path.clone();
                let mut next=None;
                egui::ScrollArea::vertical()
                    .id_salt("category-explorer-ancestors")
                    .max_height(theme::CATEGORY_EXPLORER_ANCESTOR_MAX_HEIGHT)
                    .show(ancestor_menu,|rows|{
                        for (i,id) in path.iter().enumerate().take(path.len()-shown_category_count(path.len())){
                            let name=index.get(id.as_str())
                                .map(|node|node.name.as_str()).unwrap_or(id);
                            if theme::action(
                                rows,&format!("{}단계 · {name}",i+1),
                                theme::ButtonRole::Quiet,true
                            ).clicked(){next=Some(path[..=i].to_vec());}
                        }
                    });
                if let Some(path)=next{
                    state.path=path;state.search.clear();state.ancestors_open=false;
                }
            });
    }
    components::soft_divider(menu);
    menu.label(typography::caption("현재 경로"));
    crumbs_row(menu,state,index,selected,enabled,allow_empty,changed);
}

/// Same trigger, same popup owner, same hover and one-shot feedback anchor as
/// the management dropdown; only its *contents* implement hierarchical
/// navigation and a fixed, explicitly committed category ID.
pub fn category_dropdown(
    ui:&mut Ui,nodes:&[CategoryNode],selected:&mut String,
    width:f32,empty_label:&str,enabled:bool,allow_empty:bool
)->Response{
    let index=tree_index(nodes);
    let full=if selected.is_empty(){empty_label.to_owned()}else{
        full_name(&index,&chain(&index,selected))
    };
    let full=if full.is_empty(){empty_label.to_owned()}else{full};
    let visible=typography::fit_category_path(
        ui,&full,(width-theme::CATEGORY_PATH_PICKER_TEXT_PADDING).max(1.0),
        typography::CategoryPathFont::Body
    );
    let mut trigger=components::management_dropdown_trigger(
        ui,&visible,width,enabled,&full,None
    );
    let state_id:Id=trigger.id.with("hierarchical-category-explorer");
    let ctx:Context=ui.ctx().clone();
    let mut state=ctx.data_mut(|data|
        data.get_temp::<ExplorerState>(state_id).unwrap_or_default()
    );
    if trigger.clicked(){
        state.path=chain(&index,selected);
        state.search.clear();
        state.ancestors_open=false;
    }
    let mut changed=false;
    // Constrain the expanded popup by the viewport, not by the 280pt trigger.
    let popup_width=theme::CATEGORY_EXPLORER_POPUP_WIDTH
        .min((ui.ctx().screen_rect().width()-theme::SPACE_24).max(1.0))
        .max(width.min(ui.ctx().screen_rect().width()-theme::SPACE_24));
    egui::Popup::menu(&trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|menu|{
            menu_body(
                menu,nodes,&index,&mut state,selected,enabled,allow_empty,
                &mut changed,trigger.rect,popup_width
            );
        });
    ctx.data_mut(|data|data.insert_temp(state_id,state));
    if changed{trigger.mark_changed();}
    trigger
}

#[cfg(test)]
mod tests{
    use super::*;
    fn fixture()->Vec<CategoryNode>{
        let mut nodes=vec![CategoryNode{
            id:"root".into(),name:"반도스탠드".into(),parent:String::new()
        }];
        let mut parent="root".to_owned();
        for depth in 0..30{
            let id=format!("node-{depth}");
            nodes.push(CategoryNode{
                id:id.clone(),name:format!("카테고리 {depth}"),parent
            });
            parent=id;
        }
        nodes
    }
    #[test]
    fn thirty_levels_can_go_to_any_ancestor_without_recursive_rendering(){
        let nodes=fixture();let index=tree_index(&nodes);
        let path=chain(&index,"node-29");
        assert_eq!(path.len(),31);
        assert_eq!(path[0],"root");
        assert_eq!(path[30],"node-29");
        assert_eq!(path[..=4].last().unwrap(),"node-3");
    }
    #[test]
    fn duplicate_names_and_search_resolve_by_id_and_full_path(){
        let nodes=vec![
            CategoryNode{id:"a".into(),name:"A".into(),parent:String::new()},
            CategoryNode{id:"b".into(),name:"B".into(),parent:String::new()},
            CategoryNode{id:"a1".into(),name:"소형".into(),parent:"a".into()},
            CategoryNode{id:"b1".into(),name:"소형".into(),parent:"b".into()},
        ];
        let index=tree_index(&nodes);
        assert_eq!(full_name(&index,&chain(&index,"a1")),"A > 소형");
        assert_eq!(full_name(&index,&chain(&index,"b1")),"B > 소형");
        let result=search_results(&nodes,&index,"B > 소형");
        assert_eq!(result.len(),1);assert_eq!(result[0].id,"b1");
    }
    #[test]
    fn cycles_terminate_and_orphan_roots_stay_navigable(){
        let nodes=vec![
            CategoryNode{id:"a".into(),name:"A".into(),parent:"b".into()},
            CategoryNode{id:"b".into(),name:"B".into(),parent:"a".into()},
            CategoryNode{id:"orphan".into(),name:"유실 부모".into(),parent:"missing".into()},
        ];
        let index=tree_index(&nodes);
        assert_eq!(chain(&index,"a").len(),2);
        assert_eq!(direct_children(&nodes,&index,"")[0].id,"orphan");
    }
    #[test]
    fn breadcrumb_labels_use_intrinsic_width_and_shrink_only_when_needed(){
        assert_eq!(fitted_crumb_widths(&[58.0,112.0],220.0),vec![58.0,112.0]);
        let fitted=fitted_crumb_widths(&[58.0,112.0],120.0);
        assert_eq!(fitted.len(),2);
        assert!((fitted.iter().sum::<f32>()-120.0).abs()<0.01);
        assert!(fitted[0]<fitted[1]);
        assert!(fitted.iter().all(|width|*width>=26.0));
        assert!(fitted_crumb_widths(&[],100.0).is_empty());
    }
    #[test]
    fn category_rows_use_one_fixed_height_and_one_painter_in_both_modes(){
        assert_eq!(CATEGORY_RESULT_ROW_HEIGHT,34.0);
        let nodes=vec![
            CategoryNode{id:"root".into(),name:"반도스탠드".into(),parent:String::new()},
            CategoryNode{id:"child".into(),name:"보면대".into(),parent:"root".into()},
        ];
        let index=tree_index(&nodes);
        let path=chain(&index,"child");
        let ctx=egui::Context::default();
        let mut heights=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                for show_ancestors in [false,true]{
                    let response=category_option_row(
                        ui,&nodes[1],&index,&path,520.0,"",show_ancestors
                    );
                    heights.push(response.rect.height());
                }
            });
        });
        assert_eq!(heights,[34.0,34.0]);
        // Inspect only the row renderer and its actual invocation.
        // Searching the whole source also matches assertions in this test.
        let source=include_str!("category_dropdown.rs");
        let renderer=source.split("fn category_option_row(").nth(1).unwrap()
            .split("fn crumb(").next().unwrap();
        assert!(!renderer.contains("52.0"));
        assert!(!renderer.contains("rect.top()+37.0"));
        assert!(source.contains("category_option_row(\n                    list,node,index,&path,list_width,selected,searching"));
    }
    #[test]
    fn path_search_row_shows_leaf_once_with_muted_ancestors(){
        let labels=["반도스탠드","보면대","책상,국악,성악용 보면대"];
        let measure=|s:&str|s.chars().count() as f32;
        let (parents,leaf)=fitted_category_row_text(&labels,100.0,measure);
        assert_eq!(parents,"반도스탠드 > 보면대 > ");
        assert_eq!(leaf,"책상,국악,성악용 보면대");
        assert_eq!(format!("{parents}{leaf}").matches(leaf.as_str()).count(),1);
        let (parents,leaf)=fitted_category_row_text(&labels,25.0,measure);
        assert!(parents.starts_with('…') || parents.is_empty());
        assert_eq!(leaf,"책상,국악,성악용 보면대");
        let (parents,leaf)=fitted_category_row_text(&labels,10.0,measure);
        assert!(parents.is_empty());
        assert!(leaf.ends_with('…'));
        assert!(measure(&leaf)<=10.0);
    }
    #[test]
    fn first_three_categories_are_never_hidden_by_the_depth_rule(){
        for depth in 0..=3{
            assert_eq!(shown_category_count(depth),depth);
        }
        assert_eq!(shown_category_count(4),3);
        assert_eq!(shown_category_count(30),3);
    }
    #[test]
    fn fixed_popup_width_is_thirty_percent_wider_than_previous_design(){
        assert_eq!(theme::CATEGORY_EXPLORER_POPUP_WIDTH,546.0);
        let source=include_str!("category_dropdown.rs");
        assert!(source.contains("menu.set_width(popup_width);"));
    }
    #[test]
    fn category_list_reserves_six_rows_regardless_of_item_count(){
        assert_eq!(theme::CATEGORY_EXPLORER_VISIBLE_ROWS,6);
        let expected=6.0*CATEGORY_RESULT_ROW_HEIGHT+5.0*theme::SPACE_4;
        assert_eq!(theme::CATEGORY_EXPLORER_LIST_MAX_HEIGHT,expected);
        assert_eq!(expected,224.0);
        let source=include_str!("category_dropdown.rs");
        let list=source.split(".id_salt((\"category-explorer-list\"").nth(1).unwrap()
            .split("    if let Some(path)=next_path{").next().unwrap();
        assert!(list.contains(".max_height(list_height)"));
        assert!(list.contains(".auto_shrink([false,false])"));
        assert!(!list.contains(".auto_shrink([false,true])"));
        assert!(!list.contains("state.ancestors_open{120.0}"));
    }
    #[test]
    fn selected_and_current_categories_are_distinct_color_roles(){
        assert_ne!(
            theme::CATEGORY_EXPLORER_APPLIED_BG,
            theme::CATEGORY_EXPLORER_CURRENT_BG
        );
        assert_eq!(
            theme::CATEGORY_EXPLORER_APPLIED_BORDER,theme::SELECTED_ORDER
        );
        assert_eq!(
            theme::MANAGEMENT_DROPDOWN_HOVER_BG,
            egui::Color32::from_rgb(57,91,118)
        );
    }
}
