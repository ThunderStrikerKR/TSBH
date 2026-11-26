//! One reusable drill-down category selector for egui management screens.
//! Presentation-only: selection writes an existing category ID, never a new
//! category, product edit, or implicit business transaction.
use std::collections::{HashMap,HashSet};
use eframe::egui::{self,Context,Id,Response,Ui,Vec2};
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
// Show only the current category as parent of its immediate children.
// Earlier ancestors are available exclusively through the ellipsis button.
fn hidden_ancestor_count(depth:usize)->usize{depth.saturating_sub(1)}

fn navigate(state:&mut ExplorerState,path:Vec<String>){
    state.path=path;
    state.search.clear();
    state.ancestors_open=false;
}

/// Commit a category ID, not a label. A repeated leaf click closes the
/// selector without emitting a duplicate changed event.
fn commit_path(state:&ExplorerState,selected:&mut String,changed:&mut bool,
    enabled:bool,allow_empty:bool)->bool{
    if !enabled || (!allow_empty && state.path.is_empty()){return false;}
    let id=state.path.last().cloned().unwrap_or_default();
    if *selected!=id{
        *selected=id;
        *changed=true;
    }
    true
}

/// Branches are navigation only; terminal nodes are immediately applied.
/// Both search results and ordinary tree rows use this same transition.
fn activate_path(state:&mut ExplorerState,path:Vec<String>,has_children:bool,
    selected:&mut String,changed:&mut bool,enabled:bool,allow_empty:bool)->bool{
    if !enabled{return false;}
    navigate(state,path);
    if has_children{false}else{commit_path(state,selected,changed,enabled,allow_empty)}
}

fn category_option_row(ui:&mut Ui,name:&str,depth:usize,is_last:bool,
    has_children:bool,open:bool,selected:bool)->Response{
    components::category_tree_row(ui,components::CategoryTreeRow{
        name,summary:"",depth,ancestor_guides:&[],is_last,
        root:depth==0&&open,has_children,open,selected,
    })
}

fn category_back_button(ui:&mut Ui,enabled:bool)->Response{
    theme::action_with_min_width(
        ui,"← 상위로",theme::ButtonRole::Ghost,enabled,
        theme::CATEGORY_EXPLORER_BACK_WIDTH
    )
}

/// Keep root navigation and explicit Apply for a nonterminal category.
/// The ellipsis is a separate control, never an inert truncation label.
fn crumbs_row(ui:&mut Ui,state:&mut ExplorerState,selected:&mut String,
    enabled:bool,allow_empty:bool,changed:&mut bool)->bool{
    let mut apply=false;
    let width=ui.available_width().max(1.0);
    ui.allocate_ui_with_layout(
        Vec2::new(width,theme::CATEGORY_EXPLORER_CRUMB_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),|bar|{
            bar.spacing_mut().item_spacing.x=theme::SPACE_4;
            if theme::action_with_min_width(bar,"전체",theme::ButtonRole::Standard,
                enabled&&!state.path.is_empty(),theme::CATEGORY_EXPLORER_ROOT_WIDTH).clicked(){
                navigate(state,Vec::new());
            }
            if hidden_ancestor_count(state.path.len())>0{
                if theme::action_with_min_width(bar,"…",theme::ButtonRole::Standard,
                    enabled,theme::CATEGORY_EXPLORER_OMITTED_WIDTH).clicked(){
                    state.ancestors_open=!state.ancestors_open;
                }
            }
            bar.with_layout(egui::Layout::right_to_left(egui::Align::Center),|right|{
                let current=state.path.last().map(String::as_str).unwrap_or("");
                apply=theme::action_with_min_width(right,"적용",theme::ButtonRole::Primary,
                    enabled&&(allow_empty||!state.path.is_empty())&&current!=selected.as_str(),
                    theme::CATEGORY_EXPLORER_APPLY_WIDTH).clicked();
            });
        }
    );
    apply&&commit_path(state,selected,changed,enabled,allow_empty)
}

fn menu_body(
    menu:&mut Ui,nodes:&[CategoryNode],index:&HashMap<&str,&CategoryNode>,
    state:&mut ExplorerState,selected:&mut String,enabled:bool,
    allow_empty:bool,changed:&mut bool,popup_width:f32,
){
    menu.set_min_width(popup_width);
    menu.set_max_width(popup_width);
    menu.set_width(popup_width);
    menu.spacing_mut().item_spacing.y=theme::SPACE_8;
    let available=menu.available_width().max(1.0);
    let search_response=components::management_search_field(
        menu,&mut state.search,available,"카테고리 이름 또는 경로 검색"
    );
    if !search_response.has_focus() && enabled &&
        menu.input(|i|i.key_pressed(egui::Key::ArrowLeft)){
        let mut path=state.path.clone();
        path.pop();
        navigate(state,path);
    }
    components::soft_divider(menu);
    let searching=!state.search.trim().is_empty();
    let current=state.path.last().and_then(|id|index.get(id.as_str())).copied();
    let items=if searching{
        search_results(nodes,index,&state.search)
    }else{
        direct_children(nodes,index,current.map(|node|node.id.as_str()).unwrap_or(""))
    };
    // Use the tree master's 42pt rows and zero row gap, so the connecting
    // rails between the current parent and children never break.
    let list_height=theme::CATEGORY_EXPLORER_LIST_MAX_HEIGHT
        .min((menu.ctx().content_rect().height()-270.0).max(100.0));
    let mut next_path=None;
    let mut next_has_children=false;
    components::scroll_area(components::ScrollAxes::Vertical)
        .id_salt(("category-explorer-list",state.path.clone(),state.search.clone()))
        .max_height(list_height)
        .auto_shrink([false,false])
        .show(menu,|list|{
            list.spacing_mut().item_spacing.y=0.0;
            if !searching{
                if let Some(parent)=current{
                    let has_children=!items.is_empty();
                    let response=category_option_row(list,&parent.name,0,true,
                        has_children,has_children,parent.id==*selected);
                    // The open parent is the browsing context, not a second
                    // selection action. A terminal parent may close again.
                    if response.clicked()&&!has_children{
                        next_path=Some(chain(index,&parent.id));
                        next_has_children=false;
                    }
                }
            }
            if items.is_empty()&&(searching||current.is_none()){
                list.label(typography::caption(if searching{
                    "검색 결과가 없습니다."
                }else{"하위 카테고리가 없습니다."}));
            }
            let depth=usize::from(!searching&&current.is_some());
            for (position,node) in items.iter().enumerate(){
                let path=chain(index,&node.id);
                let has_children=nodes.iter().any(|child|child.parent==node.id);
                // Search is flat but retains the full path to disambiguate
                // identical leaf names. Ordinary navigation shows just names.
                let label=if searching{full_name(index,&path)}else{node.name.clone()};
                let response=category_option_row(list,&label,depth,
                    position+1==items.len(),has_children,false,node.id==*selected);
                if response.clicked(){
                    next_path=Some(path);
                    next_has_children=has_children;
                }
            }
        });
    if let Some(path)=next_path{
        if activate_path(state,path,next_has_children,selected,changed,enabled,allow_empty){
            if *changed{floating_notice::dropdown_option_selected(menu.ctx());}
            menu.close();
            return;
        }
    }
    // Omitted ancestors are a separate, bounded navigation panel. They do
    // not affect selection or replace the current category until clicked.
    if state.ancestors_open && state.path.len()>1{
        theme::category_ancestor_frame().show(menu,|ancestor_menu|{
            ancestor_menu.label(typography::caption("생략된 상위 경로"));
            let path=state.path.clone();
            let mut next=None;
            components::scroll_area(components::ScrollAxes::Vertical)
                .id_salt("category-explorer-ancestors")
                .max_height(theme::CATEGORY_EXPLORER_ANCESTOR_MAX_HEIGHT)
                .show(ancestor_menu,|rows|{
                    rows.spacing_mut().item_spacing.y=0.0;
                    let hidden=hidden_ancestor_count(path.len());
                    for (i,id) in path.iter().enumerate().take(hidden){
                        let name=index.get(id.as_str())
                            .map(|node|node.name.as_str()).unwrap_or(id);
                        if category_option_row(rows,name,0,true,true,false,id==selected.as_str()).clicked(){
                            next=Some(path[..=i].to_vec());
                        }
                    }
                });
            if let Some(path)=next{navigate(state,path);}
        });
    }
    components::soft_divider(menu);
    let mut applied=false;
    menu.scope(|footer|{
        footer.spacing_mut().item_spacing.y=theme::SPACE_4;
        footer.horizontal(|row|{
            if category_back_button(row,enabled&&!state.path.is_empty()).clicked(){
                let mut path=state.path.clone();
                path.pop();
                navigate(state,path);
            }
        });
        applied=crumbs_row(footer,state,selected,enabled,allow_empty,changed);
    });
    if applied{
        if *changed{floating_notice::dropdown_option_selected(menu.ctx());}
        menu.close();
    }
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
    let mut trigger=components::management_dropdown_trigger_full_label(
        ui,&visible,&full,width,enabled,None
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
    // Fixed width across the product list, detail editor, and category
    // manager. Never grow with the trigger width or breadcrumb text.
    // Only shrink to the actual viewport on a genuinely narrow screen.
    let popup_width=theme::CATEGORY_EXPLORER_POPUP_WIDTH
        .min((ui.ctx().content_rect().width()-theme::SPACE_24).max(1.0));
    crate::overlay::show_menu(&trigger,None,|menu|{
            menu_body(
                menu,nodes,&index,&mut state,selected,enabled,allow_empty,
                &mut changed,popup_width
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
    fn only_current_parent_is_visible_and_all_earlier_ancestors_are_omitted(){
        for depth in [0_usize,1,2,3,30]{
            assert_eq!(hidden_ancestor_count(depth),depth.saturating_sub(1));
        }
        let nodes=fixture();
        let index=tree_index(&nodes);
        let mut state=ExplorerState::default();
        navigate(&mut state,chain(&index,"node-4"));
        assert_eq!(state.path.last().unwrap(),"node-4");
        assert_eq!(hidden_ancestor_count(state.path.len()),5);
        let children=direct_children(&nodes,&index,state.path.last().unwrap());
        assert_eq!(children.len(),1);
        assert_eq!(children[0].id,"node-5");
        navigate(&mut state,chain(&index,"node-1"));
        assert_eq!(state.path.last().unwrap(),"node-1");
    }
    #[test]
    fn branches_navigate_without_committing_and_leaf_applies_immediately(){
        let mut state=ExplorerState::default();
        let mut selected="previous".to_owned();
        let mut changed=false;
        assert!(!activate_path(&mut state,vec!["root".into()],true,
            &mut selected,&mut changed,true,true));
        assert_eq!(selected,"previous");
        assert!(!changed);
        assert!(activate_path(&mut state,vec!["root".into(),"leaf".into()],false,
            &mut selected,&mut changed,true,true));
        assert_eq!(selected,"leaf");
        assert!(changed);
        changed=false;
        assert!(activate_path(&mut state,vec!["root".into(),"leaf".into()],false,
            &mut selected,&mut changed,true,true));
        assert!(!changed);
    }
    #[test]
    fn search_leaf_uses_same_transition_and_clears_search(){
        let nodes=fixture();let index=tree_index(&nodes);
        let mut state=ExplorerState{search:"카테고리 29".into(),..Default::default()};
        let result=search_results(&nodes,&index,&state.search);
        let leaf=result.iter().find(|node|node.id=="node-29").unwrap();
        let mut selected=String::new();
        let mut changed=false;
        assert!(activate_path(&mut state,chain(&index,&leaf.id),false,
            &mut selected,&mut changed,true,true));
        assert_eq!(selected,"node-29");
        assert!(state.search.is_empty());
        assert!(changed);
    }
    #[test]
    fn manual_branch_and_empty_selection_respect_existing_guards(){
        let mut state=ExplorerState::default();
        let mut selected="old".to_owned();let mut changed=false;
        assert!(!commit_path(&state,&mut selected,&mut changed,true,false));
        assert!(!activate_path(&mut state,vec!["leaf".into()],false,
            &mut selected,&mut changed,false,true));
        assert_eq!(selected,"old");
        navigate(&mut state,vec!["branch".into()]);
        assert!(commit_path(&state,&mut selected,&mut changed,true,false));
        assert_eq!(selected,"branch");
        navigate(&mut state,Vec::new());
        assert!(commit_path(&state,&mut selected,&mut changed,true,true));
        assert!(selected.is_empty());
    }
    #[test]
    fn category_search_and_navigation_share_actual_tree_row_height(){
        let ctx=egui::Context::default();
        let mut heights=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                heights.push(category_option_row(ui,"반도스탠드",0,true,true,true,false).rect.height());
                heights.push(category_option_row(ui,"보면대",1,true,false,false,true).rect.height());
                heights.push(category_option_row(ui,"반도스탠드 > 보면대",0,true,false,false,false).rect.height());
            });
        });
        assert_eq!(heights,[42.0,42.0,42.0]);
        assert_eq!(theme::CATEGORY_EXPLORER_POPUP_WIDTH,655.0);
        assert_eq!(theme::CATEGORY_EXPLORER_VISIBLE_ROWS,6);
        assert_eq!(theme::CATEGORY_EXPLORER_LIST_MAX_HEIGHT,252.0);
    }
    #[test]
    fn tree_menu_uses_shared_rows_and_a_separate_ellipsis_control(){
        let source=include_str!("category_dropdown.rs");
        let renderer=source.split("fn category_option_row(").nth(1).unwrap()
            .split("fn category_back_button(").next().unwrap();
        assert!(renderer.contains("components::category_tree_row("));
        let body=source.split("fn menu_body(").nth(1).unwrap()
            .split("pub fn category_dropdown(").next().unwrap();
        assert!(body.contains("list.spacing_mut().item_spacing.y=0.0;"));
        assert!(body.contains(".auto_shrink([false,false])"));
        assert!(body.contains("if state.ancestors_open && state.path.len()>1"));
        assert!(body.contains("activate_path("));
        let footer=source.split("fn crumbs_row(").nth(1).unwrap()
            .split("fn menu_body(").next().unwrap();
        assert!(footer.contains("hidden_ancestor_count(state.path.len())>0"));
        assert!(footer.contains("bar,\"…\""));
    }
}
