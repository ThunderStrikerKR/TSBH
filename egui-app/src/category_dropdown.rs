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
    let visible=typography::fit_category_path(
        ui,label,(width-theme::SPACE_8).max(1.0),
        typography::CategoryPathFont::Body
    );
    painter.text(
        rect.center(),Align2::CENTER_CENTER,visible,
        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),ink
    );
    response.on_hover_text(label)
}
fn crumbs_row(
    ui:&mut Ui,state:&mut ExplorerState,index:&HashMap<&str,&CategoryNode>,
    selected:&mut String,enabled:bool,allow_empty:bool,changed:&mut bool
){
    let path=state.path.clone();
    let len=path.len();
    let visible=if len>2 {path[len-2..].to_vec()}else{path.clone()};
    let hidden=len.saturating_sub(2);
    let row_width=ui.available_width().max(1.0);
    let apply_width=70.0;
    // Reserve the last control FIRST. No category name can grow the footer
    // or push the apply button outside its right edge.
    let separators=visible.len()+usize::from(hidden>0);
    let fixed=42.0+if hidden>0{31.0}else{0.0}
        +separators as f32*10.0+apply_width+theme::SPACE_8;
    let crumb_width=if visible.is_empty(){0.0}else{
        ((row_width-fixed)/visible.len() as f32).max(18.0).min(135.0)
    };
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
            if crumb(bar,name,crumb_width,role).clicked()
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
    menu.set_min_width(popup_width);
    menu.set_max_width(popup_width);
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
    let list_height=(menu.ctx().screen_rect().height()-270.0)
        .max(100.0).min(if state.ancestors_open{120.0}
            else{theme::CATEGORY_EXPLORER_LIST_MAX_HEIGHT});
    let mut next_path=None;
    egui::ScrollArea::vertical()
        .id_salt(("category-explorer-list",state.path.clone(),state.search.clone()))
        .max_height(list_height)
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
                let height=if searching{52.0}else{theme::ACTION_STANDARD_HEIGHT};
                let (rect,response)=list.allocate_exact_size(
                    Vec2::new(list_width,height),Sense::click()
                );
                let hovered=response.hovered()||response.has_focus();
                // The selected category is orange anywhere it is visible:
                // breadcrumb, search result or direct child. Hover never
                // overwrites its committed state with blue.
                if node.id.as_str()==selected.as_str(){
                    list.painter().rect(
                        rect,theme::CONTROL_RADIUS,
                        theme::CATEGORY_EXPLORER_APPLIED_BG,
                        Stroke::new(
                            theme::SOFT_DIVIDER_WIDTH,
                            theme::CATEGORY_EXPLORER_APPLIED_BORDER
                        ),egui::StrokeKind::Inside
                    );
                }else if hovered{
                    list.painter().rect_filled(
                        rect,theme::CONTROL_RADIUS,
                        theme::MANAGEMENT_DROPDOWN_HOVER_BG
                    );
                }
                let label=typography::fit_category_path(
                    list,&node.name,(rect.width()-theme::SPACE_24).max(1.0),
                    typography::CategoryPathFont::Body
                );
                let painter=list.painter().with_clip_rect(rect);
                painter.text(
                    egui::pos2(rect.left()+theme::SPACE_8,
                        if searching{rect.top()+14.0}else{rect.center().y}),
                    Align2::LEFT_CENTER,label,
                    egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
                    theme::TEXT
                );
                if searching{
                    let display=typography::fit_category_path(
                        list,&full,(rect.width()-theme::SPACE_16).max(1.0),
                        typography::CategoryPathFont::Body
                    );
                    painter.text(
                        egui::pos2(rect.left()+theme::SPACE_8,rect.top()+37.0),
                        Align2::LEFT_CENTER,display,
                        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
                        theme::SECONDARY
                    );
                }else{
                    painter.text(
                        egui::pos2(rect.right()-theme::SPACE_12,rect.center().y),
                        Align2::CENTER_CENTER,"›",
                        egui::FontId::proportional(typography::FONT_STANDARD_ACTION),
                        theme::SECONDARY
                    );
                }
                let response=response.on_hover_text(&full);
                if response.clicked(){next_path=Some(path);}
            }
        });
    if let Some(path)=next_path{
        state.path=path;
        state.search.clear();
        state.ancestors_open=false;
    }
    if state.ancestors_open && state.path.len()>2{
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
                        for (i,id) in path.iter().enumerate().take(path.len()-2){
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
