//! One reusable drill-down category selector for egui management screens.
//! Presentation-only: selection writes an existing category ID, never a new
//! category, product edit, or implicit business transaction.
use std::collections::{HashMap,HashSet};
use crate::card::UiWidthPolicyExt;
use eframe::egui::{self,Context,Id,Response,Ui,Vec2};
use serde_json::Value;
use crate::{category_manager::ROOTS,category_tree,floating_notice,theme,typography};

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct CategoryNode {
    pub id:String,
    pub name:String,
    pub parent:String,
}
#[derive(Clone,Debug,Default)]
pub(crate) struct ExplorerState {
    pub(crate) path:Vec<String>,
    pub(crate) search:String,
    ancestors_open:bool,
    motion:Option<Motion>,
    pub(crate) scroll_epoch:u64,
    committed_anchor:Option<String>,
    scroll_positions:HashMap<String,f32>,
    rendered_context:Option<String>,
    rendered_query:String,
    restore_scroll:Option<f32>,
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
pub(crate) fn tree_index(nodes:&[CategoryNode])->HashMap<&str,&CategoryNode>{
    nodes.iter().map(|n|(n.id.as_str(),n)).collect()
}
pub(crate) fn chain(index:&HashMap<&str,&CategoryNode>,id:&str)->Vec<String>{
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
    result.sort_by(|a,b|{
        let rank=|id:&str| ROOTS.iter().position(|(root,_)|*root==id).unwrap_or(ROOTS.len());
        rank(&a.id).cmp(&rank(&b.id)).then_with(||a.name.cmp(&b.name))
    });
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
// Empty path is a virtual root, not a persisted category ID.
pub(crate) fn navigate(state:&mut ExplorerState,path:Vec<String>){
    state.path=path;
    state.search.clear();
    state.ancestors_open=false;
    state.motion=None;
    state.restore_scroll=None;
}
fn commit_path(state:&ExplorerState,selected:&mut String,changed:&mut bool,
    enabled:bool,allow_empty:bool)->bool{
    if !enabled || (!allow_empty && state.path.is_empty()){return false;}
    let id=state.path.last().cloned().unwrap_or_default();
    if *selected!=id{*selected=id;*changed=true;}
    true
}
fn category_option_row(ui:&mut Ui,name:&str,depth:usize,is_last:bool,
    has_children:bool,open:bool,selected:bool)->Response{
    category_tree::category_tree_row(ui,category_tree::CategoryTreeRow{
        name,summary:"",depth,ancestor_guides:&[],is_last,
        root:depth==0&&open,has_children,open,selected,
    })
}
fn category_back_button(ui:&mut Ui,enabled:bool)->Response{
    crate::button::action_with_min_width(ui,"← 상위로",theme::ButtonRole::Standard,
        enabled,theme::CATEGORY_EXPLORER_BACK_WIDTH)
}
pub(crate) fn motion_ms(depth:usize)->f64{
    (450+depth.saturating_sub(1).min(4)*150).min(1000) as f64
}
#[derive(Clone,Debug)]
struct Motion{
    from:Vec<String>,
    to:Vec<String>,
    started:f64,
    duration:f64,
    reverse:bool,
    source_y:f32,
    source_depth:usize,
    target_y:f32,
    target_depth:usize,
    target_scroll:f32,
    source_scroll:f32,
    ghost_name:String,
}
impl Motion{
    fn elapsed(&self,now:f64)->f32{
        ((now-self.started)/self.duration).clamp(0.0,1.0) as f32*450.0
    }
}
pub(crate) fn ease(t:f32)->f32{let t=t.clamp(0.0,1.0);1.0-(1.0-t).powi(3)}
fn lerp(a:f32,b:f32,t:f32)->f32{a+(b-a)*ease(t)}
// Normalized to 450ms: fade, horizontal, vertical, unfold; exact reverse.
fn motion_pose(m:&Motion,t:f32)->(f32,f32){
    let (h,v)=if m.reverse{((t-260.0)/100.0,(t-120.0)/140.0)}
        else{((t-90.0)/100.0,(t-190.0)/140.0)};
    (lerp(m.source_depth as f32,m.target_depth as f32,h)*theme::CATALOG_TREE_INDENT,
        lerp(m.source_y,m.target_y,v))
}
fn label_for<'a>(index:&'a HashMap<&str,&CategoryNode>,path:&[String])->&'a str{
    path.last().and_then(|id|index.get(id.as_str())).map(|n|n.name.as_str()).unwrap_or("전체")
}
// Terminal categories remain selected in their sibling list until Apply.
fn context_path(nodes:&[CategoryNode],path:&[String])->Vec<String>{
    let mut result=path.to_vec();
    if let Some(id)=path.last(){
        if !nodes.iter().any(|n|n.parent==*id){result.pop();}
    }
    result
}
fn begin_move(state:&mut ExplorerState,path:Vec<String>,nodes:&[CategoryNode],
    index:&HashMap<&str,&CategoryNode>,now:f64,source:Option<(f32,f32)>,height:f32){
    if state.motion.is_some()||state.path==path{return;}
    if !state.search.trim().is_empty(){navigate(state,path);return;}
    let old_context=context_path(nodes,&state.path);
    let new_context=context_path(nodes,&path);
    if old_context==new_context{navigate(state,path);return;}
    let reverse=state.path.starts_with(&path);
    let forward=path.starts_with(&old_context)&&path.len()==old_context.len()+1;
    if !reverse&&!forward{navigate(state,path);return;}
    let source_path=if reverse{state.path.clone()}else{path.clone()};
    let source_depth=usize::from(!reverse||state.path!=old_context);
    let row_h=theme::CATALOG_TREE_ROW_HEIGHT;
    let old_parent=old_context.last().map(String::as_str).unwrap_or("");
    let source_index=if source_depth==0{0}else{
        direct_children(nodes,index,old_parent).iter().position(|n|Some(&n.id)==source_path.last()).unwrap_or(0)+1
    };
    let previous=state.scroll_positions.get(old_parent).copied().unwrap_or(0.0);
    let (source_y,source_scroll)=source.unwrap_or((source_index as f32*row_h-previous,previous));
    state.scroll_positions.insert(old_parent.to_owned(),source_scroll);
    let (target_y,target_depth,target_scroll)=if reverse{
        let dest=path.last().map(String::as_str).unwrap_or("");
        let branch=state.path.get(path.len());
        let pos=direct_children(nodes,index,dest).iter().position(|n|Some(&n.id)==branch).unwrap_or(0)+1;
        let raw=pos as f32*row_h;
        // Keep the last parent viewport unless the returning branch is outside it.
        let saved=state.scroll_positions.get(dest).copied().unwrap_or(0.0);
        let scroll=saved.clamp((raw+row_h-height.max(row_h)).max(0.0),raw.max(0.0));
        (raw-scroll,1,scroll)
    }else{(0.0,0,0.0)};
    let m=Motion{from:state.path.clone(),to:path,started:now,
        duration:motion_ms(if reverse{state.path.len().saturating_sub(new_context.len())}else{1})/1000.0,
        reverse,source_y:source_y.clamp(0.0,(height-row_h).max(0.0)),source_depth,
        target_y,target_depth,target_scroll,source_scroll,
        ghost_name:label_for(index,&source_path).to_owned()};
    state.search.clear();state.ancestors_open=false;state.motion=Some(m);
}
fn omitted_count(width:f32,ancestors:&[f32],current:f32,dots:f32,separator:f32)->usize{
    for omitted in 0..=ancestors.len(){
        let rest=&ancestors[omitted..];
        let needed=current+rest.iter().sum::<f32>()+separator*rest.len() as f32
            +if omitted>0{dots+separator}else{0.0};
        if needed<=width{return omitted;}
    }
    ancestors.len()
}
fn crumb_width(ui:&Ui,name:&str)->f32{
    typography::category_tree_name_galley(ui,name,false).size().x+12.0
}
fn crumb_text(ui:&mut Ui,name:&str,width:f32,current:bool,enabled:bool)->Response{
    let (rect,response)=ui.allocate_exact_size(Vec2::new(width.max(0.0),theme::CATEGORY_EXPLORER_CRUMB_HEIGHT),
        if enabled&&!current{egui::Sense::click()}else{egui::Sense::hover()});
    #[cfg(test)]
    crate::ui_test_support::record_highlight_target("category breadcrumb",response.rect,ui.clip_rect());
    if enabled&&!current{crate::button::paint_button_hover(ui,&response,crate::highlight::Geometry::Control);}
    let visible=typography::fit_category_path(ui,name,(width-12.0).max(0.0),typography::CategoryPathFont::Body);
    let galley=typography::category_tree_name_galley(ui,&visible,false);
    let pos=egui::pos2(rect.left()+theme::CATEGORY_EXPLORER_CRUMB_TEXT_INSET,rect.center().y-galley.size().y/2.0);
    ui.painter().with_clip_rect(rect).galley_with_override_text_color(pos,galley,
        if current{theme::CATALOG_TREE_SELECTED_INK}else{theme::CATEGORY_EXPLORER_ANCESTOR_INK});
    crate::ui_primitives::overflow_tooltip(response,visible!=name,name)
}
pub(crate) fn crumbs_row(ui:&mut Ui,state:&mut ExplorerState,index:&HashMap<&str,&CategoryNode>,
    path:&[String],enabled:bool,alpha:f32,slide:f32)->Option<Vec<String>>{
    let mut next=None;
    let height=theme::CATEGORY_EXPLORER_CRUMB_HEIGHT;
    let (nav_rect,_)=ui.allocate_exact_size(Vec2::new(ui.available_width().max(1.0),height),egui::Sense::hover());
    // The back action remains in a fixed slot, outside the moving/fading path.
    // Root and busy states disable it without shifting the breadcrumb or tree.
    let back_width=theme::CATEGORY_EXPLORER_BACK_WIDTH.min(nav_rect.width());
    let back_rect=egui::Rect::from_min_size(nav_rect.min,Vec2::new(back_width,height));
    let mut back=ui.new_child(egui::UiBuilder::new().id_salt("category-back")
        .max_rect(back_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    back.set_clip_rect(back_rect.intersect(ui.clip_rect()));
    if category_back_button(&mut back,enabled&&!path.is_empty()).clicked(){
        next=Some(path[..path.len().saturating_sub(1)].to_vec());
    }
    let rail=(back_width+theme::SPACE_8).min((nav_rect.width()-1.0).max(0.0));
    let rect=egui::Rect::from_min_max(nav_rect.min+egui::vec2(rail,0.0),nav_rect.max);
    let width=rect.width().max(1.0);
    let mut bar=ui.new_child(egui::UiBuilder::new().max_rect(rect.translate(egui::vec2(slide,0.0)))
        .layout(egui::Layout::left_to_right(egui::Align::Center)));
    bar.set_clip_rect(rect.intersect(ui.clip_rect()));
    // Preserve an enclosing manager-mode fade when no path motion is active.
    if alpha<1.0{bar.set_opacity(alpha);}
    crate::category_tree::configure_breadcrumb_row(&mut bar);
    let names=(0..=path.len()).map(|i|label_for(index,&path[..i])).collect::<Vec<_>>();
    let widths=names.iter().map(|n|crumb_width(&bar,n)).collect::<Vec<_>>();
    let last=widths.len()-1;
    let dots=theme::CATEGORY_EXPLORER_OMITTED_WIDTH;
    let sep=theme::CATEGORY_EXPLORER_SEPARATOR_WIDTH;
    let omitted=omitted_count(width,&widths[..last],widths[last],dots,sep);
    let separator=|bar:&mut Ui|{let (r,_)=bar.allocate_exact_size(Vec2::new(sep,height),egui::Sense::hover());
        let galley=typography::category_tree_name_galley(bar,"›",false);
        bar.painter().galley_with_override_text_color(r.center()-galley.size()/2.0,galley,theme::CATEGORY_EXPLORER_SEPARATOR_INK);};
    if omitted>0{
        if crumb_text(&mut bar,"…",dots,false,enabled).clicked(){
            state.ancestors_open=!state.ancestors_open;
        }
        separator(&mut bar);
    }
    for i in omitted..last{
        if crumb_text(&mut bar,names[i],widths[i],false,enabled).clicked(){next=Some(path[..i].to_vec());}
        separator(&mut bar);
    }
    let remaining=(width-(if omitted>0{dots+sep}else{0.0})-
        widths[omitted..last].iter().sum::<f32>()-sep*(last-omitted) as f32).max(0.0);
    crumb_text(&mut bar,names[last],remaining.min(widths[last]),true,false);
    if state.ancestors_open&&omitted>0&&enabled{
        // A foreground area overlays the list; opening it never resizes the popup.
        let id=ui.id().with("category-omitted-paths");
        let pos=rect.left_bottom();
        crate::overlay::area(id,crate::overlay::AreaRole::Help).fixed_pos(pos).show(ui.ctx(),|area|{
            crate::card::category_ancestor_frame().show(area,|rows|{
                rows.apply_shared_width(crate::card::CardWidthPolicy::UpperLimit,width);
                crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                    .id_salt("category-explorer-ancestors")
                    .max_height(theme::CATEGORY_EXPLORER_ANCESTOR_MAX_HEIGHT)
                    .show(rows,|rows|{
                        for i in 0..omitted{
                            if category_option_row(rows,names[i],0,true,true,false,false).clicked(){
                                next=Some(path[..i].to_vec());
                            }
                        }
                    });
            });
        });
    }
    next
}
impl ExplorerState{
    pub(crate) fn moving(&self)->bool{self.motion.is_some()}
    pub(crate) fn finish_motion(&mut self){
        if let Some(m)=self.motion.take(){
            let offset=m.target_scroll;
            navigate(self,m.to);
            self.restore_scroll=Some(offset);
        }
    }
}
fn menu_body(
    menu:&mut Ui,nodes:&[CategoryNode],index:&HashMap<&str,&CategoryNode>,
    state:&mut ExplorerState,selected:&mut String,enabled:bool,
    allow_empty:bool,changed:&mut bool,popup_width:f32,
){
    crate::category_tree::configure_explorer_popup(menu,popup_width);
    let available=menu.available_width().max(1.0);
    let search_response=menu.add_enabled_ui(enabled&&!state.moving(),|menu|
        crate::input::management_search_field(menu,&mut state.search,available,"카테고리 이름 또는 경로 검색")
    ).inner;
    let list_height=theme::CATEGORY_EXPLORER_LIST_MAX_HEIGHT
        .min((menu.ctx().content_rect().height()-270.0).max(100.0));
    explorer_body(menu,nodes,index,state,enabled,list_height,search_response.has_focus(),None);
    let busy=state.moving();
    crate::ui_primitives::soft_divider(menu);
    let mut applied=false;
    menu.with_layout(egui::Layout::right_to_left(egui::Align::Center),|right|{
        applied=crate::button::action_with_min_width(right,"적용",theme::ButtonRole::Primary,
            enabled&&!busy&&state.motion.is_none()&&(allow_empty||!state.path.is_empty()),
            theme::CATEGORY_EXPLORER_APPLY_WIDTH).clicked();
    });
    if applied&&commit_path(state,selected,changed,enabled,allow_empty){
        if *changed{floating_notice::dropdown_option_selected(menu.ctx());}
        menu.close();
    }
}

/// Shared focused explorer: the manager uses this without an Apply footer.
pub(crate) fn explorer_body(menu:&mut Ui,nodes:&[CategoryNode],index:&HashMap<&str,&CategoryNode>,
    state:&mut ExplorerState,enabled:bool,list_height:f32,search_focused:bool,
    totals:Option<&HashMap<String,usize>>){
    let now=menu.input(|i|i.time);
    if state.motion.as_ref().is_some_and(|m|m.elapsed(now)>=450.0){state.finish_motion();menu.ctx().request_repaint();}
    else if state.motion.is_some(){menu.ctx().request_repaint();}
    let motion=state.motion.clone();
    let busy=motion.is_some();
    let mut next_path=None;
    let mut source=None;
    if !search_focused&&enabled&&!busy&&menu.input(|i|i.key_pressed(egui::Key::ArrowLeft)){
        let mut p=state.path.clone();p.pop();next_path=Some(p);
    }
    let t=motion.as_ref().map(|m|m.elapsed(now)).unwrap_or(450.0);
    let final_phase=motion.as_ref().is_some_and(|m|t>=if m.reverse{360.0}else{330.0});
    let visual_path=motion.as_ref().map(|m|if final_phase{m.to.clone()}else{m.from.clone()}).unwrap_or_else(||state.path.clone());
    let alpha=motion.as_ref().map(|m|{
        let fade=if m.reverse{120.0}else{90.0};
        let reveal=if m.reverse{360.0}else{330.0};
        if t<fade{1.0-t/fade}else if t>=reveal{(t-reveal)/(450.0-reveal)}else{0.0}
    }).unwrap_or(1.0);
    let nav_slide=motion.as_ref().map(|m|if final_phase{(1.0-alpha)*if m.reverse{-8.0}else{8.0}}
        else{(1.0-alpha)*if m.reverse{8.0}else{-8.0}}).unwrap_or(0.0);
    if let Some(p)=crumbs_row(menu,state,index,&visual_path,enabled&&!busy,alpha,nav_slide){next_path=Some(p);}
    if totals.is_some(){crate::ui_primitives::section_divider(menu);}
    else{crate::ui_primitives::soft_divider(menu);}
    let searching=!state.search.trim().is_empty();
    let context=context_path(nodes,&visual_path);
    let current=context.last().map(String::as_str).unwrap_or("");
    let items=if searching{search_results(nodes,index,&state.search)}else{direct_children(nodes,index,current)};
    let row_h=theme::CATALOG_TREE_ROW_HEIGHT;
    let mut child_counts=HashMap::<&str,usize>::new();
    for node in nodes{*child_counts.entry(node.parent.as_str()).or_default()+=1;}
    let context_changed=state.rendered_context.as_deref()!=Some(current);
    let query_changed=state.rendered_query!=state.search;
    if context_changed{
        state.scroll_positions.retain(|id,_|id.is_empty()||index.contains_key(id.as_str()));
        state.rendered_context=Some(current.to_owned());
    }
    if query_changed{state.rendered_query.clone_from(&state.search);}
    // Navigation owns the back button; the tree uses the full viewport width.
    menu.vertical(|tree_ui|{
            let mut scroll=crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                .id_salt(("category-explorer-list",state.scroll_epoch,searching,busy))
                .max_height(list_height).auto_shrink([false,false]);
            if let Some(m)=&motion{scroll=scroll.vertical_scroll_offset(if final_phase{m.target_scroll}else{m.source_scroll});}
            else if let Some(offset)=state.restore_scroll.take(){scroll=scroll.vertical_scroll_offset(offset);}
            else if searching&&query_changed{scroll=scroll.vertical_scroll_offset(0.0);}
            else if !searching&&(context_changed||query_changed){
                scroll=scroll.vertical_scroll_offset(state.scroll_positions.get(current).copied().unwrap_or(0.0));
            }
            let output=scroll.show(tree_ui,|list|{
                crate::category_tree::configure_tree_rows(list);
                let top=list.next_widget_position().y;
                let clip=list.clip_rect();
                let mut paint_row=|list:&mut Ui,name:&str,path:Vec<String>,depth:usize,last:bool,has_children:bool,open:bool|{
                    let hide=motion.as_ref().is_some_and(|m|{
                        if final_phase{!m.reverse&&path==m.to}else if m.reverse{path==m.from}else{path==m.to}
                    });
                    let response=list.scope(|row|{
                        // A fully opaque local row must not cancel the parent mode fade.
                        if hide{row.set_opacity(0.0);}
                        else if alpha<1.0{row.set_opacity(alpha);}
                        if let Some(totals)=totals{
                            let id=path.last().map(String::as_str).unwrap_or("");
                            let summary=if id.is_empty(){String::new()}else{
                                let count=totals.get(id).copied().unwrap_or(0);
                                let children=child_counts.get(id).copied().unwrap_or(0);
                                if children==0{format!("(상품 {count})")}else{format!("(상품 {count} · 하위 분류 {children})")}
                            };
                            category_tree::category_tree_row(row,category_tree::CategoryTreeRow{
                                name,summary:&summary,depth,ancestor_guides:&[],is_last:last,
                                root:depth==0&&open,has_children,open,selected:path==visual_path,
                            })
                        }else{category_option_row(row,name,depth,last,has_children,open,path==visual_path)}
                    }).inner;
                    if enabled&&!busy&&response.clicked(){
                        source=Some((response.rect.top()-clip.top(),(clip.top()-top).max(0.0)));
                        next_path=Some(path);
                    }
                };
                if !searching{
                    paint_row(list,label_for(index,&context),context.clone(),0,true,!items.is_empty(),!items.is_empty());
                }
                // Empty search feedback occupies a reserved row, not a new conditional layout.
                if searching{
                    let message=if items.is_empty(){"검색 결과가 없습니다."}else{""};
                    if items.is_empty(){crate::category_tree::empty_search_row(list,message);}
                }
                for (i,node) in items.iter().enumerate(){
                    let path=if searching{chain(index,&node.id)}else{
                        let mut path=context.clone();path.push(node.id.clone());path
                    };
                    let has_children=child_counts.contains_key(node.id.as_str());
                    let label=if searching{full_name(index,&path)}else{node.name.clone()};
                    paint_row(list,&label,path,usize::from(!searching),i+1==items.len(),has_children,false);
                }
            });
            crate::scrollbar::track_native(tree_ui,&output);
            if !busy&&!searching{
                if let Some(offset)=state.scroll_positions.get_mut(current){*offset=output.state.offset.y;}
                else{state.scroll_positions.insert(current.to_owned(),output.state.offset.y);}
            }
            if let Some(m)=&motion{
                let (x,y)=motion_pose(m,t);
                let clip=output.inner_rect.intersect(tree_ui.clip_rect());
                let rect=egui::Rect::from_min_size(clip.min+egui::vec2(x,y),
                    Vec2::new((clip.width()-x).max(1.0),row_h));
                let mut ghost=tree_ui.new_child(egui::UiBuilder::new().id_salt("moving-category")
                    .max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
                ghost.set_clip_rect(clip);
                ghost.set_opacity(if m.reverse&&final_phase{1.0-alpha}else{1.0});
                category_option_row(&mut ghost,&m.ghost_name,0,true,true,!m.reverse&&final_phase,true);
            }
    });
    if let Some(path)=next_path{
        begin_move(state,path,nodes,index,now,source,list_height);
        menu.ctx().request_repaint();
    }
}

type PopupStates=crate::view_state::TimedViewStore<(ExplorerState,std::time::Instant)>;
fn popup_states_id()->Id{Id::new("category-explorer-popup-states")}
pub(crate) fn sweep_popup_states(ctx:&Context,now:std::time::Instant){
    let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data|{
        let states=data.get_temp_mut_or_default::<PopupStates>(popup_states_id());
        states.advance_frame(frame,now);
        states.sweep_due(now);
    });
}
/// Same trigger, same popup owner, same hover and one-shot feedback anchor as
/// the management dropdown; only its *contents* implement hierarchical
/// navigation and a fixed, explicitly committed category ID.
pub fn category_dropdown(
    ui:&mut Ui,nodes:&[CategoryNode],selected:&mut String,
    width:f32,empty_label:&str,enabled:bool,allow_empty:bool,empty_is_placeholder:bool
)->Response{
    let index=tree_index(nodes);
    let full=if selected.is_empty(){empty_label.to_owned()}else{
        full_name(&index,&chain(&index,selected))
    };
    let placeholder=full.is_empty()||(selected.is_empty()&&empty_is_placeholder);
    let full=if full.is_empty(){empty_label.to_owned()}else{full};
    let visible=typography::fit_category_path(
        ui,&full,(width-theme::CATEGORY_PATH_PICKER_TEXT_PADDING).max(1.0),
        typography::CategoryPathFont::Body
    );
    let mut trigger=crate::dropdown::management_dropdown_trigger_full_label(
        ui,&visible,&full,width,enabled,None,placeholder
    );
    let state_id:Id=trigger.id.with("hierarchical-category-explorer");
    let ctx:Context=ui.ctx().clone();
    let mut changed=false;
    // Fixed width across the product list, detail editor, and category
    // manager. Never grow with the trigger width or breadcrumb text.
    // Only shrink to the actual viewport on a genuinely narrow screen.
    let popup_width=theme::CATEGORY_EXPLORER_POPUP_WIDTH
        .min((ui.ctx().content_rect().width()-theme::SPACE_24).max(1.0));
    crate::overlay::show_menu(&trigger,None,|menu|{
            let mut state=ctx.data_mut(|data|{
                let states=data.get_temp_mut_or_default::<PopupStates>(popup_states_id());
                let now=std::time::Instant::now();
                states.take(state_id,now).map(|entry|entry.0).unwrap_or_default()
            });
            if trigger.clicked(){
                // Restore browsing only for the same committed value and
                // a still-valid path. An external change starts a new scope.
                state.finish_motion();
                let valid=state.path.last().is_none_or(|id|chain(&index,id)==state.path);
                if state.committed_anchor.as_deref()!=Some(selected.as_str())||!valid{
                    navigate(&mut state,chain(&index,selected));
                    state.rendered_context=None;
                }
                state.scroll_positions.retain(|id,_|id.is_empty()||index.contains_key(id.as_str()));
            }
            menu_body(
                menu,nodes,&index,&mut state,selected,enabled,allow_empty,
                &mut changed,popup_width
            );
            state.committed_anchor=Some(selected.clone());
            ctx.data_mut(|data|{
                let states=data.get_temp_mut_or_default::<PopupStates>(popup_states_id());
                states.put(state_id,(state,std::time::Instant::now()));
            });
        });
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
    fn repeated_popup_navigation_reclaims_old_explorers_and_folder_offsets(){
        let start=std::time::Instant::now();
        let step=std::time::Duration::from_secs(60);
        let ttl=crate::ui_tokens::NAVIGATION_SNAPSHOT_TTL;
        let mut states=PopupStates::default();
        let active=Id::new("always-open-explorer");
        let mut explorer=ExplorerState::default();
        explorer.scroll_positions.insert("active-folder".into(),77.0);
        states.entries.insert(active,(explorer,start));
        for n in 0..2048_u32{
            let now=start+step*n;
            states.advance_frame(u64::from(n),now);
            states.sweep_due(now);
            let id=Id::new(("transient-explorer",n));
            let mut state=ExplorerState::default();
            for folder in 0..32{
                state.scroll_positions.insert(format!("folder-{folder}"),folder as f32);
            }
            states.entries.insert(id,(state,now));
            states.seen.insert(id);
            states.seen.insert(active);
            assert!(states.entries.len()<=32,"expired explorer retained at {n}");
            assert_eq!(states.entries[&active].0.scroll_positions["active-folder"],77.0);
        }
        let closed=start+step*2048;
        states.advance_frame(2048,closed);
        states.sweep_due(closed);
        states.advance_frame(2049,closed+ttl);
        states.sweep_due(closed+ttl);
        assert!(states.entries.is_empty());
        assert!(states.seen.is_empty());
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
    fn deepest_visible_suffix_and_root_order(){
        assert_eq!(omitted_count(200.0,&[40.0,60.0],50.0,36.0,12.0),0);
        assert_eq!(omitted_count(120.0,&[40.0,60.0],50.0,36.0,12.0),2);
        let nodes=nodes_from_catalog(&serde_json::json!({}));
        let index=tree_index(&nodes);
        let roots=direct_children(&nodes,&index,"");
        assert_eq!(roots.iter().map(|r|r.id.as_str()).collect::<Vec<_>>(),ROOTS.iter().map(|r|r.0).collect::<Vec<_>>());
    }
    #[test]
    fn motion_duration_and_orthogonal_sequence(){
        assert_eq!((1..=6).map(motion_ms).collect::<Vec<_>>(),[450.0,600.0,750.0,900.0,1000.0,1000.0]);
        let nodes=fixture();let index=tree_index(&nodes);
        let mut state=ExplorerState::default();
        begin_move(&mut state,vec!["root".into()],&nodes,&index,0.0,Some((42.0,0.0)),252.0);
        let m=state.motion.unwrap();
        assert_eq!(motion_pose(&m,90.0),(theme::CATALOG_TREE_INDENT,42.0));
        assert_eq!(motion_pose(&m,190.0),(0.0,42.0));
        assert_eq!(motion_pose(&m,330.0),(0.0,0.0));
        let mut state=ExplorerState::default();
        navigate(&mut state,chain(&index,"node-4"));
        begin_move(&mut state,vec![],&nodes,&index,0.0,None,252.0);
        let m=state.motion.unwrap();
        assert_eq!(m.duration,1.0);
        assert_eq!(motion_pose(&m,260.0),(0.0,42.0));
        assert_eq!(motion_pose(&m,360.0),(theme::CATALOG_TREE_INDENT,42.0));
    }
    #[test]
    fn reverse_motion_reuses_parent_viewport_without_overwriting_it(){
        let nodes=fixture();let index=tree_index(&nodes);
        let mut state=ExplorerState::default();
        let root=vec!["root".to_owned()];
        navigate(&mut state,chain(&index,"node-0"));
        state.scroll_positions.insert(root[0].clone(),20.0);
        begin_move(&mut state,root.clone(),&nodes,&index,0.0,None,252.0);
        assert_eq!(state.motion.as_ref().unwrap().target_scroll,20.0);
        state.finish_motion();
        assert_eq!(state.restore_scroll,Some(20.0));
        assert_eq!(state.scroll_positions.get(&root[0]),Some(&20.0));
    }
    #[test]
    fn leaf_selection_is_pending_and_retains_siblings(){
        let nodes=fixture();let index=tree_index(&nodes);
        let mut state=ExplorerState::default();
        let path=chain(&index,"node-29");
        navigate(&mut state,path.clone());
        assert_eq!(context_path(&nodes,&state.path),path[..path.len()-1]);
        let mut selected="old".to_owned();let mut changed=false;
        assert!(commit_path(&state,&mut selected,&mut changed,true,true));
        assert_eq!(selected,"node-29");assert!(changed);
    }
    #[test]
    fn manual_branch_and_empty_selection_respect_existing_guards(){
        let mut state=ExplorerState::default();
        let mut selected="old".to_owned();let mut changed=false;
        assert!(!commit_path(&state,&mut selected,&mut changed,true,false));
        assert!(!commit_path(&state,&mut selected,&mut changed,false,true));
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
        crate::typography::install(&ctx);
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
    fn breadcrumb_palette_keeps_only_current_folder_prominent(){
        assert!(theme::CATEGORY_EXPLORER_BACK_WIDTH>theme::CATEGORY_EXPLORER_CRUMB_HEIGHT);
        assert_eq!(theme::CATEGORY_EXPLORER_ANCESTOR_INK,theme::MUTED);
        assert!(theme::CATEGORY_EXPLORER_SEPARATOR_INK.r()<theme::CATEGORY_EXPLORER_ANCESTOR_INK.r());
        assert!(theme::CATALOG_TREE_SELECTED_INK.r()>theme::CATEGORY_EXPLORER_ANCESTOR_INK.r());
        let source=include_str!("category_dropdown.rs");
        let row=source.split("fn crumb_text(").nth(1).unwrap().split("fn crumbs_row(").next().unwrap();
        assert!(row.contains("if current{theme::CATALOG_TREE_SELECTED_INK}else{theme::CATEGORY_EXPLORER_ANCESTOR_INK}"));
    }
    #[test]
    fn category_back_is_a_centered_standard_block(){
        let ctx=egui::Context::default();
        crate::typography::install(&ctx);
        let mut measurements=Vec::new();
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                for enabled in [true,false]{
                    let (slot,_)=ui.allocate_exact_size(Vec2::new(theme::CATEGORY_EXPLORER_BACK_WIDTH,
                        theme::CATEGORY_EXPLORER_CRUMB_HEIGHT),egui::Sense::hover());
                    let mut row=ui.new_child(egui::UiBuilder::new().id_salt(enabled).max_rect(slot)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)));
                    let response=category_back_button(&mut row,enabled);
                    measurements.push((slot,response.rect));
                }
            });
        });
        for (slot,button) in measurements{
            assert!((slot.center().y-button.center().y).abs()<=1.0);
            assert!(button.width()<=slot.width()+1.0);
            assert!((button.height()-theme::ACTION_STANDARD_HEIGHT).abs()<=1.0);
        }
    }
    #[test]
    fn shared_rows_and_single_motion_are_used(){
        let source=include_str!("category_dropdown.rs");
        assert!(source.contains("category_tree::category_tree_row("));
        assert!(source.contains(".auto_shrink([false,false])"));
        assert!(source.contains("motion_pose(m,t)"));
        assert!(source.contains("omitted_count(width,"));
    }
}
