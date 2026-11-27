//! Native CategoryManager: tree presentation is owned by category_tree::category_tree_row.
//! Category selection, search, expansion and operations remain in this manager.
//! All add/rename/move/delete decisions are delegated to catalog_clients.
use crate::{category_dropdown,category_tree,theme,typography};
use eframe::egui::{self,Align,Context,Layout,Rect,Sense,Ui,Vec2};
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};
#[derive(Clone,Debug)]
struct TreeMotion{id:String,opening:bool,started:f64,duration:f64}
#[derive(Clone,Debug)]
struct NavMotion{from:Vec<String>,started:f64,duration:f64,reverse:bool}
#[derive(Clone,Debug)]
struct ModeMotion{target:bool,started:f64,duration:f64}
impl ModeMotion{
    // Match the shared breadcrumb transition: fade the old view out, then
    // fade the destination in, without moving the fixed toolbar.
    fn phase(&self,now:f64)->(bool,f32,bool){
        let t=((now-self.started)/self.duration).clamp(0.0,1.0) as f32;
        if t<0.5{(!self.target,1.0-category_dropdown::ease(t*2.0),false)}
        else{(self.target,category_dropdown::ease((t-0.5)*2.0),t>=1.0)}
    }
}
impl TreeMotion{
    fn progress(&self,now:f64)->f32{category_dropdown::ease(((now-self.started)/self.duration).clamp(0.0,1.0) as f32)}
}

pub(crate) const ROOTS:[(&str,&str);2]=[
    ("catalog-root:BANDOSTAND","반도스탠드"),
    ("catalog-root:BANDOMALL","반도몰"),
];

#[derive(Clone)]
struct Category {
    id:String,
    name:String,
    parent:String,
    active:bool,
}
#[derive(Clone,Copy,PartialEq,Eq)]
enum Operation {Add,Move,Delete}
#[derive(Clone,Copy,PartialEq,Eq)]
enum Confirm {CloseEditor,CloseOperation,SwitchTab,GlobalLeave}

pub struct CategoryWrite {
    pub action:&'static str,
    pub data:Value,
}
pub struct CategoryManager {
    selected:String,
    expanded:HashSet<String>,
    query:String,
    focused:bool,
    focus:category_dropdown::ExplorerState,
    crumbs:category_dropdown::ExplorerState,
    tree_motion:Option<TreeMotion>,
    nav_motion:Option<NavMotion>,
    mode_motion:Option<ModeMotion>,
    map_cache:Option<(u64,String,std::sync::Arc<CategoryMapIndex>)>,
    node_cache:crate::screen_refresh::ViewProjectionCache<Option<u64>,(Vec<Category>,Vec<category_dropdown::CategoryNode>)>,
    scroll_epoch:u64,
    scroll_to:Option<String>,
    name:String,
    editor:bool,
    expected_scope:Option<String>,
    operation:Option<Operation>,
    new_name:String,
    target:String,
    transfer:bool,
    confirm:Option<Confirm>,
    after_save:Option<Confirm>,
    switch_to_list:bool,
    global_leave_approved:bool,
    delete_parent:Option<String>,
    error:Option<String>,
}
impl Default for CategoryManager {
    fn default()->Self{
        Self{
            selected:ROOTS[0].0.into(),
            expanded:ROOTS.iter().map(|(id,_)|(*id).to_owned()).collect(),
            query:String::new(),focused:false,focus:Default::default(),crumbs:Default::default(),
            tree_motion:None,nav_motion:None,mode_motion:None,map_cache:None,node_cache:Default::default(),scroll_epoch:0,scroll_to:None,
            name:String::new(),editor:false,expected_scope:None,
            operation:None,new_name:String::new(),target:String::new(),
            transfer:false,confirm:None,after_save:None,
            switch_to_list:false,global_leave_approved:false,
            delete_parent:None,error:None,
        }
    }
}
fn field<'a>(value:&'a Value,key:&str)->&'a str{
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn items<'a>(value:&'a Value,key:&str)->&'a [Value]{
    value.get(key).and_then(Value::as_array)
        .map(Vec::as_slice).unwrap_or(&[])
}
fn categories(catalog:&Value)->Vec<Category>{
    let mut result=items(catalog,"categories").iter().map(|row|Category{
        id:field(row,"id").to_owned(),
        name:field(row,"name").to_owned(),
        parent:field(row,"parentId").to_owned(),
        active:row["active"]!=false,
    }).collect::<Vec<_>>();
    for (id,name) in ROOTS{
        if !result.iter().any(|row|row.id==id){
            result.push(Category{id:id.to_owned(),name:name.into(),
                parent:String::new(),active:true});
        }
    }
    result
}
fn is_root(id:&str)->bool{ROOTS.iter().any(|(root,_)|*root==id)}
fn path(cats:&[Category],id:&str)->Vec<Category>{
    let mut result=Vec::new();
    let mut current=id;
    let mut visited=HashSet::new();
    while !current.is_empty()&&visited.insert(current.to_owned()){
        let Some(row)=cats.iter().find(|row|row.id==current)else{break};
        result.insert(0,row.clone());
        current=&row.parent;
    }
    result
}
fn path_text(cats:&[Category],id:&str)->String{
    path(cats,id).iter().map(|c|c.name.as_str()).collect::<Vec<_>>().join(" > ")
}

/// Read-only, ID-based full category paths for management tables.
/// Use the same parentId tree (including virtual BANDOSTAND/BANDOMALL roots)
/// as the category map; do not trust the denormalized product.category label
/// when category IDs can resolve the current hierarchy.
pub(crate) struct ProductCategoryPaths {
    by_product: HashMap<String,String>,
}
impl ProductCategoryPaths {
    pub(crate) fn new(state:&Value,products:&[&Value])->Self{
        let cats=categories(state);
        let by_id=cats.iter().map(|c|(c.id.as_str(),c))
            .collect::<HashMap<_,_>>();
        let mut by_product=HashMap::new();
        for product in products{
            // The product editor and Rust category mutation use the final
            // category ID as the leaf of the current category path.
            let leaf=items(product,"categories").iter().rev()
                .filter_map(Value::as_str).find(|id|by_id.contains_key(id));
            let display=if let Some(id)=leaf{
                // Identical ancestor traversal to the Category Map.
                let mut names=Vec::new();
                let mut seen=HashSet::new();
                let mut cursor=id;
                while !cursor.is_empty()&&seen.insert(cursor){
                    let Some(node)=by_id.get(cursor)else{break;};
                    names.push(node.name.as_str());
                    cursor=node.parent.as_str();
                }
                names.reverse();names.join(" > ")
            }else{
                // Legacy records without a valid category ID are not silently
                // assigned to a root: keep their stored display as fallback.
                field(product,"category").to_owned()
            };
            by_product.insert(field(product,"id").to_owned(),display);
        }
        Self{by_product}
    }
    pub(crate) fn for_product<'a>(&'a self,product:&Value)->&'a str{
        self.by_product.get(field(product,"id"))
            .map(String::as_str).unwrap_or("")
    }
}
fn under(cats:&[Category],id:&str,ancestor:&str)->bool{
    path(cats,id).iter().any(|part|part.id==ancestor)
}
fn children<'a>(cats:&'a[Category],id:&str)->Vec<&'a Category>{
    cats.iter().filter(|c|c.parent==id).collect()
}
fn direct_count(catalog:&Value,id:&str)->usize{
    items(catalog,"products").iter()
        .filter(|p|field(p,"deletedAt").is_empty()&&field(p,"supersededBy").is_empty())
        .filter(|p|items(p,"categories").iter().any(|cat|cat.as_str()==Some(id)))
        .count()
}
/// Build the category tree's child links, deduplicated product totals and
/// search visibility ONCE per map render. The old row renderer scanned every
/// product and walked its entire category ancestry for EACH visible row,
/// multiplying the work by the number of expanded categories every frame.
#[derive(Clone)]
struct CategoryMapIndex {
    children:std::sync::Arc<HashMap<String,Vec<usize>>>,
    totals:std::sync::Arc<HashMap<String,usize>>,
    visible:Option<HashSet<String>>,
}
impl CategoryMapIndex {
    fn new(cats:&[Category],catalog:&Value,query:&str)->Self{
        let mut children:HashMap<String,Vec<usize>>=HashMap::new();
        let mut by_id:HashMap<&str,usize>=HashMap::new();
        for (index,item) in cats.iter().enumerate(){
            children.entry(item.parent.clone()).or_default().push(index);
            // Match path()'s first-ID-wins lookup if catalog data is malformed.
            by_id.entry(item.id.as_str()).or_insert(index);
        }
        let mut totals=HashMap::new();
        // A product contributes at most once to any ancestor even when it
        // belongs to multiple nested categories. Deleted or superseded
        // products remain excluded exactly as in the original total_count().
        let mut counted=HashSet::new();
        let mut walked=HashSet::new();
        for product in items(catalog,"products").iter().filter(|p|
            field(p,"deletedAt").is_empty() &&
            field(p,"supersededBy").is_empty()
        ){
            counted.clear();
            for category in items(product,"categories").iter()
                .filter_map(Value::as_str){
                walked.clear();
                let mut cursor=category;
                while let Some(&index)=by_id.get(cursor){
                    if !walked.insert(index){break;}
                    if counted.insert(index){
                        *totals.entry(cats[index].id.clone()).or_insert(0_usize)+=1;
                    }
                    cursor=cats[index].parent.as_str();
                }
            }
        }
        Self{children:std::sync::Arc::new(children),totals:std::sync::Arc::new(totals),
            visible:Self::visibility(cats,query)}
    }
    fn visibility(cats:&[Category],query:&str)->Option<HashSet<String>>{
        let q=query.trim().to_lowercase();
        if q.is_empty(){return None;}
        let mut by_id=HashMap::new();
        for item in cats{by_id.entry(item.id.as_str()).or_insert(item);}
        let mut visible=HashSet::new();
        for item in cats{
            let mut ancestors=Vec::new();
            let mut seen=HashSet::new();
            let mut current=item.id.as_str();
            while !current.is_empty()&&seen.insert(current){
                let Some(node)=by_id.get(current)else{break;};
                ancestors.push(*node);
                current=node.parent.as_str();
            }
            if ancestors.iter().rev().map(|c|c.name.as_str()).collect::<Vec<_>>()
                .join(" > ").to_lowercase().contains(&q){
                visible.extend(ancestors.iter().map(|c|c.id.clone()));
            }
        }
        Some(visible)
    }
    fn branches(&self,id:&str)->&[usize]{
        self.children.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
    fn total(&self,id:&str)->usize{
        self.totals.get(id).copied().unwrap_or(0)
    }
    fn shows(&self,id:&str)->bool{
        self.visible.as_ref().is_none_or(|visible|visible.contains(id))
    }
}
impl CategoryManager {
    fn current(&self,cats:&[Category])->Option<Category>{
        cats.iter().find(|row|row.id==self.selected).cloned()
    }
    fn rename_dirty(&self,cats:&[Category])->bool{
        self.editor && self.current(cats).is_some_and(|current|
            !is_root(&current.id)&&self.name.trim()!=current.name.trim())
    }
    fn operation_dirty(&self,cats:&[Category])->bool{
        let Some(current)=self.current(cats)else{return false};
        match self.operation {
            Some(Operation::Add)=>!self.new_name.trim().is_empty(),
            Some(Operation::Move)=>!self.target.is_empty()
                && self.target!=if current.parent.is_empty(){
                    ROOTS[0].0
                }else{current.parent.as_str()},
            _=>false,
        }
    }
    pub fn dirty(&self,catalog:&Value)->bool{
        let cats=categories(catalog);
        self.rename_dirty(&cats)||self.operation_dirty(&cats)
    }
    pub fn has_overlay(&self)->bool{
        self.editor||self.operation.is_some()||self.confirm.is_some()
    }
    pub fn request_tab_leave(&mut self,catalog:&Value)->bool{
        if self.dirty(catalog){
            self.confirm=Some(Confirm::SwitchTab);
            false
        }else{true}
    }
    pub fn request_global_leave(&mut self,catalog:&Value)->bool{
        if self.dirty(catalog){
            self.confirm=Some(Confirm::GlobalLeave);
            false
        }else{true}
    }
    pub fn take_switch_to_list(&mut self)->bool{
        std::mem::take(&mut self.switch_to_list)
    }
    pub fn take_global_leave_approved(&mut self)->bool{
        std::mem::take(&mut self.global_leave_approved)
    }
    pub fn global_leave_in_progress(&self)->bool{
        self.confirm==Some(Confirm::GlobalLeave)
            || self.after_save==Some(Confirm::GlobalLeave)
    }
    pub fn report_error(&mut self,message:String){self.error=Some(message);}

    pub fn clear_for_leave(&mut self){
        if self.focused&&self.focus.moving(){
            self.focus.finish_motion();self.selected=self.focus.path.last().cloned().unwrap_or_default();
        }
        self.tree_motion=None;self.nav_motion=None;
        self.editor=false;
        self.operation=None;
        self.confirm=None;
        self.after_save=None;
    }
    fn discard(&mut self,cats:&[Category]){
        self.name=self.current(cats).map(|c|c.name).unwrap_or_default();
        self.new_name.clear();
        self.transfer=false;
        self.target.clear();
        self.operation=None;
    }
    fn begin_operation(&mut self,cats:&[Category],catalog:&Value,op:Operation){
        let Some(current)=self.current(cats)else{return};
        if op!=Operation::Add && is_root(&current.id){return}
        if op==Operation::Delete &&
            (direct_count(catalog,&current.id)>0||!children(cats,&current.id).is_empty()){
            self.error=Some("연결 상품이나 하위 분류가 있는 경우 먼저 소속을 정리하세요.".into());
            return;
        }
        if !self.editor || self.expected_scope.is_none(){
            self.expected_scope=Some(bando_data_access::management_scope_token(catalog,"categories"));
        }
        self.operation=Some(op);
        if op==Operation::Delete {
            self.delete_parent=Some(if current.parent.is_empty(){ROOTS[0].0.to_owned()}
                else{current.parent.clone()});
        }
        self.new_name.clear();
        self.transfer=false;
        self.target=if current.parent.is_empty(){
            ROOTS[0].0.into()
        }else{current.parent};
        self.error=None;
    }
    fn request_close_editor(&mut self,cats:&[Category]){
        if self.rename_dirty(cats){
            self.confirm=Some(Confirm::CloseEditor);
        }else{self.editor=false;}
    }
    fn request_close_operation(&mut self,cats:&[Category]){
        if self.operation_dirty(cats){
            self.confirm=Some(Confirm::CloseOperation);
        }else{self.operation=None;}
    }
    fn write_request(&self,cats:&[Category],catalog:&Value)
        ->Result<CategoryWrite,String>{
        let current=self.current(cats).ok_or("분류가 선택되지 않았습니다.")?;
        let mut request:CategoryWrite=match self.operation {
            Some(Operation::Add)=>{
                if self.new_name.trim().is_empty() ||
                   (direct_count(catalog,&current.id)>0 && !self.transfer){
                    return Err("분류 이름과 상품 이전 확인을 완료해 주세요.".into());
                }
                Ok::<CategoryWrite,String>(CategoryWrite{
                    action:"change_category",
                    data:json!({
                        "categoryId":"","name":self.new_name.trim(),
                        "parentId":current.id,
                        "transfer":self.transfer &&
                            children(cats,&current.id).is_empty(),
                    }),
                })
            }
            Some(Operation::Move)=>{
                if self.target.is_empty(){return Err("이동할 상위 분류를 선택하세요.".into());}
                Ok::<CategoryWrite,String>(CategoryWrite{
                    action:"change_category",
                    data:json!({
                        "categoryId":current.id,
                        "name":current.name,
                        "parentId":self.target,
                        "transfer":false,
                        "guardCatalogMove":true,
                        "guardError":"이 분류에는 이동할 최상위 구분과 다른 상품이 연결되어 있습니다. 상품 소속을 먼저 확인하세요.",
                    }),
                })
            }
            Some(Operation::Delete)=>{
                Ok::<CategoryWrite,String>(CategoryWrite{
                    action:"delete_category",
                    data:json!({"categoryId":current.id}),
                })
            }
            None=>{
                if !self.rename_dirty(cats){return Err("저장할 분류 변경사항이 없습니다.".into());}
                Ok::<CategoryWrite,String>(CategoryWrite{
                    action:"change_category",
                    data:json!({
                        "categoryId":current.id,
                        "name":self.name.trim(),
                        "parentId":if current.parent.is_empty(){
                            Value::Null
                        }else{json!(current.parent)},
                    }),
                })
            }
        }?;
        if let Some(token)=self.expected_scope.as_ref(){request.data["expectedScope"]=json!(token);}
        Ok(request)
    }
    pub fn on_saved(&mut self,kind:&str,catalog:&Value)->&'static str{
        self.expected_scope=Some(bando_data_access::management_scope_token(catalog,"categories"));
        let success=match (kind,self.operation){
            ("delete_category",_)=>"빈 분류를 삭제했습니다.",
            ("change_category",Some(Operation::Add))=>"하위 분류를 추가했습니다.",
            ("change_category",Some(Operation::Move))=>"분류 위치를 변경했습니다.",
            _=>"분류 이름을 변경했습니다.",
        };
        let cats=categories(catalog);
        if kind=="delete_category"{
            self.selected=self.delete_parent.take()
                .unwrap_or_else(||ROOTS[0].0.to_owned());
            self.editor=false;
        } else if self.operation.is_none() {
            self.editor=false;
        }
        let operation_was_open=self.operation.is_some();
        self.operation=None;
        self.confirm=None;
        // React keeps the inline rename draft when an unrelated add/move
        // operation succeeds; it must not be silently overwritten by refresh.
        if !operation_was_open || kind=="delete_category"{
            self.name=self.current(&cats).map(|c|c.name).unwrap_or_default();
        }
        self.new_name.clear();
        self.transfer=false;
        self.error=None;
        if let Some(next)=self.after_save.take(){
            self.apply_confirm(next,&cats);
        }
        success
    }
    fn apply_confirm(&mut self,intent:Confirm,cats:&[Category]){
        match intent{
            Confirm::CloseEditor=>{
                self.editor=false;
                self.name=self.current(cats).map(|c|c.name).unwrap_or_default();
            }
            Confirm::CloseOperation=>{
                // React closeOperation discards only the operation draft.
                // Do not erase an unrelated unsaved inline rename.
                self.operation=None;
                self.new_name.clear();
                self.target.clear();
                self.transfer=false;
            }
            Confirm::SwitchTab=>{
                self.clear_for_leave();
                self.switch_to_list=true;
            }
            Confirm::GlobalLeave=>{
                self.clear_for_leave();
                self.global_leave_approved=true;
            }
        }
        self.confirm=None;
    }
    pub(crate) fn reset_navigation(&mut self){
        let epoch=self.scroll_epoch.wrapping_add(1);
        self.selected=ROOTS[0].0.to_owned();
        self.expanded=ROOTS.iter().map(|(id,_)|(*id).to_owned()).collect();
        self.query.clear();self.focused=false;self.focus=Default::default();self.crumbs=Default::default();
        self.scroll_epoch=epoch;self.focus.scroll_epoch=epoch;self.tree_motion=None;self.nav_motion=None;self.mode_motion=None;self.scroll_to=None;
        self.invalidate_map_cache();
    }
    pub(crate) fn invalidate_map_cache(&mut self){self.map_cache=None;self.node_cache.invalidate();}
    fn map_index(&mut self,cats:&[Category],catalog:&Value)->std::sync::Arc<CategoryMapIndex>{
        let Some(revision)=catalog["revision"].as_u64()else{
            return std::sync::Arc::new(CategoryMapIndex::new(cats,catalog,&self.query));
        };
        if let Some((cached,query,index))=&mut self.map_cache{
            if *cached==revision{
                if *query!=self.query{
                    let mut updated=index.as_ref().clone();
                    updated.visible=CategoryMapIndex::visibility(cats,&self.query);
                    *index=std::sync::Arc::new(updated);
                    query.clone_from(&self.query);
                }
                return std::sync::Arc::clone(index);
            }
        }
        let index=std::sync::Arc::new(CategoryMapIndex::new(cats,catalog,&self.query));
        self.map_cache=Some((revision,self.query.clone(),std::sync::Arc::clone(&index)));
        index
    }
    fn select_map(&mut self,cats:&[Category],target:&str,now:f64){
        if self.selected==target{return;}
        let from=path(cats,&self.selected).into_iter().map(|c|c.id).collect::<Vec<_>>();
        let to=path(cats,target).into_iter().map(|c|c.id).collect::<Vec<_>>();
        let reverse=from.starts_with(&to);
        let depth=if reverse{from.len().saturating_sub(to.len())}else{1};
        self.nav_motion=Some(NavMotion{from,started:now,duration:category_dropdown::motion_ms(depth)/1000.0,reverse});
        self.selected=target.to_owned();
        self.name=self.current(cats).map(|c|c.name).unwrap_or_default();
    }
    fn start_fold(&mut self,id:&str,opening:bool,now:f64,depth:usize){
        if opening{self.expanded.insert(id.to_owned());}else{self.expanded.remove(id);}
        self.tree_motion=Some(TreeMotion{id:id.to_owned(),opening,started:now,duration:category_dropdown::motion_ms(depth)/1000.0});
    }
    fn visible_descendants(&self,cats:&[Category],index:&CategoryMapIndex,id:&str,seen:&mut HashSet<String>)->usize{
        if !seen.insert(id.to_owned()){return 0;}
        index.branches(id).iter().filter(|&&i|index.shows(&cats[i].id)).map(|&i|{
            let child=&cats[i];
            1+if self.expanded.contains(&child.id){self.visible_descendants(cats,index,&child.id,seen)}else{0}
        }).sum()
    }
    fn node(&mut self,ui:&mut Ui,cats:&[Category],index:&CategoryMapIndex,
        item:&Category,depth:usize,ancestor_guides:&[bool],is_last:bool,ancestors:&mut HashSet<String>){
        if !ancestors.insert(item.id.clone()){return;}
        if !index.shows(&item.id){ancestors.remove(&item.id);return;}
        let branches=index.branches(&item.id);
        let open=self.expanded.contains(&item.id)||!self.query.trim().is_empty();
        let motion=self.tree_motion.clone().filter(|m|m.id==item.id);
        let locked=self.tree_motion.is_some()||self.nav_motion.is_some()||self.mode_motion.is_some();
        let count=index.total(&item.id);
        let summary=if branches.is_empty(){
            format!("(상품 {count})")
        }else{
            format!("(상품 {count} · 하위 분류 {})",branches.len())
        };
        let response=category_tree::category_tree_row(ui,category_tree::CategoryTreeRow{
            name:&item.name,summary:&summary,depth,ancestor_guides,is_last,
            root:is_root(&item.id),has_children:!branches.is_empty(),open,
            selected:self.selected==item.id,
        });
        #[cfg(test)]
        if item.id.starts_with("geometry-cat-"){
            crate::ui_test_support::record("catalog data row",response.rect);
        }
        if self.scroll_to.as_deref()==Some(item.id.as_str()){
            response.scroll_to_me(None);self.scroll_to=None;
        }
        if response.clicked()&&!locked{
            self.select_map(cats,&item.id,ui.input(|i|i.time));
            ui.ctx().request_repaint();
            if !branches.is_empty() && self.query.trim().is_empty(){
                self.start_fold(&item.id,!open,ui.input(|i|i.time),1);
                ui.ctx().request_repaint();
            }
        }
        if open||motion.is_some() {
            let visible_children=branches.iter()
                .copied().filter(|&child|index.shows(&cats[child].id))
                .collect::<Vec<_>>();
            let mut child_guides=ancestor_guides.to_vec();
            if depth>0{child_guides.push(!is_last);}
            let draw=|this:&mut Self,rows:&mut Ui,ancestors:&mut HashSet<String>|{
                for (position,&child) in visible_children.iter().enumerate(){
                    this.node(rows,cats,index,&cats[child],depth+1,&child_guides,
                        position+1==visible_children.len(),ancestors);
                }
            };
            if let Some(m)=motion{
                let t=m.progress(ui.input(|i|i.time));let fraction=if m.opening{t}else{1.0-t};
                let full=self.visible_descendants(cats,index,&item.id,&mut HashSet::new()) as f32*theme::CATALOG_TREE_ROW_HEIGHT;
                let (slot,_)=ui.allocate_exact_size(Vec2::new(ui.available_width(),full*fraction),Sense::hover());
                let mut child=ui.new_child(egui::UiBuilder::new().id_salt(("animated-category",item.id.as_str()))
                    .max_rect(Rect::from_min_size(slot.min,Vec2::new(slot.width(),full)))
                    .layout(Layout::top_down(Align::Min)).disabled());
                child.set_clip_rect(slot.intersect(ui.clip_rect()));
                child.set_opacity(fraction);
                crate::category_tree::configure_tree_rows(&mut child);
                draw(self,&mut child,ancestors);
            }else{draw(self,ui,ancestors);}
        }
        ancestors.remove(&item.id);
    }
    /// Only action controls remain from the former title row. Both wide and
    /// narrow layouts render the same actions with the approved button roles.
    fn map_toolbar_actions(
        &mut self,actions:&mut Ui,cats:&[Category],catalog:&Value,
        editable:bool,busy:bool
    ){
        if crate::button::action(actions,"+ 하위 분류 추가",
            theme::ButtonRole::Primary,editable&&!busy &&
                self.current(cats).is_some()).clicked(){
            self.begin_operation(cats,catalog,Operation::Add);
        }
        if crate::button::action(actions,"선택 분류 편집",
            theme::ButtonRole::Standard,!busy &&
                self.current(cats).is_some()).clicked(){
            if let Some(current)=self.current(cats){
                self.name=current.name;
                self.editor=true;
                self.expected_scope=Some(bando_data_access::management_scope_token(catalog,"categories"));
            }
        }
        // This toolbar flows right-to-left: the checkbox is immediately
        // left of Edit, and uses the same shared control as other screens.
        let mut focused=self.mode_motion.as_ref().map(|m|m.target).unwrap_or(self.focused);
        if crate::checkbox::checkbox(actions,&mut focused,"집중 탐색",!busy).changed(){
            let depth=path(cats,&self.selected).len();
            self.mode_motion=Some(ModeMotion{target:focused,started:actions.input(|i|i.time),
                duration:category_dropdown::motion_ms(depth)/1000.0});
            actions.ctx().request_repaint();
        }
    }
    pub fn show_page(&mut self,ui:&mut Ui,catalog:&Value,editable:bool,busy:bool){
        let revision=catalog["revision"].as_u64();
        if revision.is_none(){self.node_cache.invalidate();}
        let projection=self.node_cache.get(revision,||{
            let cats=categories(catalog);
            let nodes=cats.iter().map(|c|category_dropdown::CategoryNode{
                id:c.id.clone(),name:c.name.clone(),parent:c.parent.clone()
            }).collect::<Vec<_>>();
            (cats,nodes)
        });
        let cats=&projection.0;
        if !self.selected.is_empty()&&!cats.iter().any(|c|c.id==self.selected){self.selected=ROOTS[0].0.to_owned();}
        if let Some(m)=&self.tree_motion{
            if ui.input(|i|i.time)>=m.started+m.duration{self.tree_motion=None;}
            else{ui.ctx().request_repaint();}
        }
        if let Some(n)=&self.nav_motion{
            if ui.input(|i|i.time)>=n.started+n.duration{self.nav_motion=None;}
            else{ui.ctx().request_repaint();}
        }
        let busy=busy||self.tree_motion.is_some()||self.nav_motion.is_some()||self.focus.moving()||self.mode_motion.is_some();
        let card_min_height=crate::card::management_primary_card_min_inner_height(
            ui,theme::SECTION_PADDING
        );
        crate::card::measured_management_section_card(ui,|panel|{
            crate::card::fill_management_primary_card(panel,card_min_height);
            // No vacated heading or help-text rows. Reserve one 36pt
            // search-and-actions toolbar, exactly as the management master.
            crate::category_tree::configure_tree_rows(panel);
            let width=panel.available_width().max(1.0);
            // Reserve the checkbox label and shared inter-control gap in both layouts.
            let focus_action_width=theme::INPUT_HEIGHT*3.0+theme::SPACE_12;
            if width<theme::CATALOG_MAP_TOOLBAR_STACK_WIDTH+focus_action_width{
                // On a narrow pane, preserve usable search and button hit
                // targets instead of squeezing them into clipped columns.
                let (actions_rect,_)=panel.allocate_exact_size(
                    Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover()
                );
                panel.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("category-map-actions-narrow")
                        .max_rect(actions_rect)
                        .layout(Layout::right_to_left(Align::Center)),
                    |actions|{
                        actions.set_width(actions_rect.width());
                        self.map_toolbar_actions(actions,&cats,catalog,editable,busy);
                    }
                );
                panel.add_space(theme::SPACE_8);
                let (search_rect,_)=panel.allocate_exact_size(
                    Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover()
                );
                panel.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("category-map-search-narrow")
                        .max_rect(search_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |search|{
                        search.set_width(search_rect.width());
                        search.add_enabled_ui(!busy,|search|crate::input::management_search_field(
                            search,&mut self.query,
                            theme::CATALOG_SEARCH_WIDTH.min(search_rect.width()),
                            "카테고리 이름 또는 전체 경로 검색"
                        ));
                    }
                );
            }else{
                let (toolbar_rect,_)=panel.allocate_exact_size(
                    Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover()
                );
                let actions_width=theme::CATALOG_MAP_TOOLBAR_ACTIONS_WIDTH+focus_action_width;
                let search_width=theme::CATALOG_SEARCH_WIDTH.min(
                    (toolbar_rect.width()-actions_width-theme::SPACE_12).max(1.0)
                );
                let search_rect=Rect::from_min_size(
                    toolbar_rect.min,Vec2::new(search_width,toolbar_rect.height())
                );
                let actions_rect=crate::ui_layout::management_toolbar_trailing_rect(
                    toolbar_rect,actions_width,0.0,toolbar_rect.height()
                );
                panel.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("category-map-search-toolbar")
                        .max_rect(search_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |search|{
                        search.set_width(search_rect.width());
                        search.add_enabled_ui(!busy,|search|crate::input::management_search_field(
                            search,&mut self.query,search_rect.width(),
                            "카테고리 이름 또는 전체 경로 검색"
                        ));
                    }
                );
                panel.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("category-map-actions-toolbar")
                        .max_rect(actions_rect)
                        .layout(Layout::right_to_left(Align::Center)),
                    |actions|{
                        actions.set_width(actions_rect.width());
                        self.map_toolbar_actions(actions,&cats,catalog,editable,busy);
                    }
                );
            }
            // The search/actions toolbar and navigation each own one fixed row.
            // Use the same symmetric structural divider on both sides of navigation.
            crate::ui_primitives::section_divider(panel);
            // The mode switch and path both use the existing shared controls.
            let nodes=&projection.1;
            let node_index=category_dropdown::tree_index(&nodes);
            let busy=busy||self.mode_motion.is_some();
            if let Some(motion)=self.mode_motion.clone(){
                let (focused,alpha,done)=motion.phase(panel.input(|i|i.time));
                if focused!=self.focused{
                    self.focused=focused;
                    if focused{
                        category_dropdown::navigate(&mut self.focus,category_dropdown::chain(&node_index,&self.selected));
                        self.focus.search=self.query.clone();
                    }else{
                        for ancestor in path(&cats,&self.selected){self.expanded.insert(ancestor.id);}
                        self.scroll_to=Some(self.selected.clone());
                    }
                }
                panel.set_opacity(alpha);
                if done{self.mode_motion=None;}
                panel.ctx().request_repaint();
            }
            if self.focused{
                let index=self.map_index(&cats,catalog);
                if !self.focus.moving(){
                    let current=category_dropdown::chain(&node_index,&self.selected);
                    if self.focus.path!=current{category_dropdown::navigate(&mut self.focus,current);}
                    self.focus.search=self.query.clone();
                }
                let height=(panel.available_height()-theme::CATEGORY_EXPLORER_CRUMB_HEIGHT-theme::SECTION_DIVIDER_HEIGHT)
                    .max(theme::CATALOG_TREE_ROW_HEIGHT);
                let search_focused=panel.ctx().memory(|mem|mem.focused().is_some());
                category_dropdown::explorer_body(panel,&nodes,&node_index,&mut self.focus,
                    !busy,height,search_focused,Some(&index.totals));
                if !self.focus.moving(){
                    let new=self.focus.path.last().cloned().unwrap_or_default();
                    if self.selected!=new{
                        self.selected=new;
                        self.name=self.current(&cats).map(|c|c.name).unwrap_or_default();
                    }
                    self.query=self.focus.search.clone();
                }
            }else{
                let current=category_dropdown::chain(&node_index,&self.selected);
                let now=panel.input(|i|i.time);
                let (nav_path,alpha,slide)=if let Some(n)=&self.nav_motion{
                    let t=((now-n.started)/n.duration).clamp(0.0,1.0) as f32;
                    let dir=if n.reverse{1.0}else{-1.0};
                    if t<0.5{(n.from.clone(),1.0-t*2.0,dir*t*16.0)}
                    else{(current.clone(),(t-0.5)*2.0,-dir*(1.0-t)*16.0)}
                }else{(current.clone(),1.0,0.0)};
                if let Some(target)=category_dropdown::crumbs_row(panel,&mut self.crumbs,&node_index,
                    &nav_path,!busy&&self.nav_motion.is_none(),alpha,slide){
                    let depth=current.len().saturating_sub(target.len());
                    let branch=current.get(target.len()).cloned();
                    self.select_map(&cats,target.last().map(String::as_str).unwrap_or(""),now);
                    panel.ctx().request_repaint();
                    self.query.clear();
                    for ancestor in &target{self.expanded.insert(ancestor.clone());}
                    if let Some(branch)=branch{
                        if self.expanded.contains(&branch){self.start_fold(&branch,false,panel.input(|i|i.time),depth);}
                    }
                    self.scroll_to=Some(self.selected.clone());
                }
            // The same structural divider as Reception and management tables.
            crate::ui_primitives::section_divider(panel);
            let index=self.map_index(&cats,catalog);
            let output=crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                .id_salt(("category-map-tree",self.scroll_epoch))
                .auto_shrink([false,false])
                .max_height(panel.available_height().max(theme::CATALOG_TREE_ROW_HEIGHT))
                .show(panel,|tree|{
                    // Adjacent rows must touch so parent/child guide lines
                    // form one continuous tree rather than broken dashes.
                    crate::category_tree::configure_tree_rows(tree);
                    let root=category_tree::category_tree_row(tree,category_tree::CategoryTreeRow{
                        name:"전체",summary:"",depth:0,ancestor_guides:&[],is_last:true,
                        root:true,has_children:true,open:true,selected:self.selected.is_empty(),
                    });
                    if root.clicked()&&!busy{
                        self.select_map(&cats,"",tree.input(|i|i.time));tree.ctx().request_repaint();
                    }
                    if self.scroll_to.as_deref()==Some(""){root.scroll_to_me(Some(Align::Min));self.scroll_to=None;}
                    let roots=ROOTS.iter().filter_map(|(id,_)|cats.iter().find(|c|c.id==*id))
                        .filter(|c|index.shows(&c.id)).collect::<Vec<_>>();
                    for (i,current) in roots.iter().enumerate(){
                        self.node(tree,&cats,&index,current,1,&[],i+1==roots.len(),&mut HashSet::new());
                    }
                    let legacy=cats.iter().filter(|c|c.parent.is_empty()&&!is_root(&c.id))
                        .cloned().collect::<Vec<_>>();
                    if !legacy.is_empty(){
                        tree.add_space(theme::SPACE_12);
                        tree.label(typography::subsection_heading(
                            &format!("상위 분류 소속 확인 ({})",legacy.len())
                        ));
                        for row in &legacy{
                            self.node(tree,&cats,&index,row,0,&[],true,&mut HashSet::new());
                        }
                    }
                });
            crate::scrollbar::track_native(panel,&output);
            }
        });
    }
    pub fn show_overlays(&mut self,ctx:&Context,work_area:Rect,
        catalog:&Value,editable:bool,busy:bool)->Option<CategoryWrite>{
        let cats=categories(catalog);
        let mut write=None;
        if self.editor{
            let nested_dialog=self.operation.is_some()||self.confirm.is_some();
            crate::popup::modal_backdrop(ctx,"category-editor-dim",false);
            // The category editor uses the same centered, Kakao-safe modal
            // geometry as other native dialogs, not a right-side drawer.
            crate::popup::modal_window(ctx,
                "선택 분류 편집","native-category-editor",work_area
            )
                .default_width(theme::MODAL_DECISION_MAX_WIDTH.min(work_area.width()))
                .show(ctx,|modal|{

                    crate::popup::modal_flow_workspace(modal,|workspace|{
                        if workspace.header("선택 분류 편집","취소",!busy&&!nested_dialog
                        ){
                            self.request_close_editor(&cats);
                        }
                        workspace.scroll_region("native-category-editor-fields",crate::popup::modal_editor_body_max_height(
                            work_area.height()
                        ),|scroll|{
                                    scroll.add_enabled_ui(!nested_dialog,|body|{
                                if let Some(current)=self.current(&cats){
                                    let name_display=typography::fit_category_path(
                                        body,&current.name,
                                        (body.available_width()-theme::CATEGORY_PARENT_ACTION_RESERVE_WIDTH)
                                            .max(0.0),
                                        typography::CategoryPathFont::Subsection
                                    );
                                    let name_response=body.add(crate::ui_primitives::label_for_explicit_tooltip(
                                        typography::subsection_heading(&name_display)
                                    ));
                                    crate::ui_primitives::overflow_tooltip(name_response,name_display!=current.name,&current.name);
                                    let breadcrumb=path_text(&cats,&current.id);
                                    let visible=typography::fit_category_path(
                                        body,&breadcrumb,body.available_width(),
                                        typography::CategoryPathFont::Metadata
                                    );
                                    let breadcrumb_response=body.add(crate::ui_primitives::label_for_explicit_tooltip(typography::caption(&visible)));
                                    crate::ui_primitives::overflow_tooltip(breadcrumb_response,visible!=breadcrumb,&breadcrumb);
                                    // Compact context for an editor, not KPI cards.
                                    // Keep a single divider between context and fields.
                                    body.label(typography::caption(&format!(
                                        "직접 연결 상품 {}개 · 하위 분류 {}개",
                                        direct_count(catalog,&current.id),
                                        children(&cats,&current.id).len()
                                    )));
                                    crate::ui_primitives::section_divider(body);
                                    crate::popup::modal_labeled_row(body,"분류 이름",|row|{
                                        row.add_enabled_ui(
                                            editable && !busy && !is_root(&current.id),
                                            |field_ui|{
                                                let input_width=field_ui.available_width().max(1.0);
                                                crate::input::flexible_text_field(
                                                    field_ui,&mut self.name,input_width,"분류 이름"
                                                );
                                            }
                                        );
                                    });
                                    body.add_space(theme::SPACE_12);
                                    crate::popup::modal_labeled_row(body,"상위 분류",|location|{
                                        // Align category text to the button's vertical
                                        // center; keep them adjacent at a shared 8pt gap.
                                        crate::ui_layout::apply_spacing_role(location,crate::ui_layout::SpacingRole::Toolbar);
                                        let parent_label=if current.parent.is_empty(){
                                            if is_root(&current.id){
                                                "최상위".to_owned()
                                            }else{"소속 확인 필요".to_owned()}
                                        }else{path_text(&cats,&current.parent)};
                                        let name_width=(location.available_width()-
                                            theme::CATEGORY_PARENT_ACTION_RESERVE_WIDTH)
                                            .max(0.0);
                                        let visible=typography::fit_category_path(
                                            location,&parent_label,name_width,
                                            typography::CategoryPathFont::Body
                                        );
                                        let galley=typography::category_tree_name_galley(
                                            location,&visible,false
                                        );
                                        let (text_rect,text_response)=location.allocate_exact_size(
                                            Vec2::new(galley.size().x.min(name_width),
                                                theme::ACTION_STANDARD_HEIGHT),
                                            Sense::hover()
                                        );
                                        location.painter().with_clip_rect(text_rect).galley(
                                            egui::pos2(text_rect.left(),
                                                text_rect.center().y-galley.size().y/2.0),
                                            galley,theme::TEXT
                                        );
                                        crate::ui_primitives::overflow_tooltip(text_response,visible!=parent_label,&parent_label);
                                        if crate::button::action(location,"위치 변경",
                                            theme::ButtonRole::Standard,
                                            editable&&!busy&&!is_root(&current.id)
                                        ).clicked(){
                                            self.begin_operation(&cats,catalog,Operation::Move);
                                        }
                                    });
                                    let legacy_notice=(!is_root(&current.id)
                                        &&current.parent.is_empty()).then_some(
                                        "현재 최상위로 남아 있는 기존 분류입니다. 반도스탠드 또는 반도몰에 직접 소속을 지정하면 기존 분류 ID와 상품 연결은 유지됩니다."
                                    );
                                    let notice_width=body.available_width();
                                    crate::ui_layout::fixed_status_slot(
                                        body,notice_width,typography::METADATA_LINE_HEIGHT,
                                        legacy_notice,legacy_notice,theme::MUTED
                                    );
                                }
                            });
                        });
                        // The footer stays visible independently of form length.
                        // Capture intent before calling the existing state machine.
                        let can_delete=self.current(&cats).is_some_and(|cur|{
                            !is_root(&cur.id)
                                &&children(&cats,&cur.id).is_empty()
                                &&direct_count(catalog,&cur.id)==0
                        });
                        let can_save=self.current(&cats).is_some()
                            &&self.rename_dirty(&cats);
                        let mut request_delete=false;
                        let mut request_save=false;
                        workspace.footer(|left|{
                                request_delete=crate::button::action(left,"분류 삭제",
                                    theme::ButtonRole::Danger,
                                    editable&&!busy&&!nested_dialog&&can_delete
                                ).clicked();
                            },
                            |right|{
                                request_save=crate::button::action(right,"변경사항 저장",
                                    theme::ButtonRole::Primary,
                                    editable&&!busy&&!nested_dialog&&can_save
                                ).clicked();
                            }
                        );
                        if request_delete {
                            self.begin_operation(&cats,catalog,Operation::Delete);
                        }
                        if request_save {
                            match self.write_request(&cats,catalog){
                                Ok(req)=>write=Some(req),
                                Err(e)=>self.error=Some(e),
                            }
                        }
                    });
                });
        }
        if let Some(operation)=self.operation{
            let mut cancel=false;
            let mut submit=false;
            crate::popup::modal_backdrop(ctx,"category-operation-dim",self.editor);
            let title=match operation {
                Operation::Add=>"하위 카테고리 추가",
                Operation::Move=>"상위 카테고리 변경",
                Operation::Delete=>"분류 삭제 확인",
            };
            crate::popup::show_decision_window(ctx,title,"category-operation-modal",work_area,|modal|{
                    crate::popup::decision_workspace(modal,|decision|{
                        cancel=decision.header(title,"취소",!busy);
                        decision.body(|modal|{
                            if let Some(current)=self.current(&cats){
                                match operation{
                                    Operation::Add=>{
                                        // Freight composition reference: identify the
                                        // target first, then group the actual input,
                                        // and keep the mandatory consent as its own
                                        // clearly separated information block.
                                        let location_path=path_text(&cats,&current.id);
                                        crate::popup::modal_labeled_row(
                                            modal,"생성 위치",|line|{
                                                let path_width=line.available_width().max(1.0);
                                                let visible=typography::fit_category_path(
                                                    line,&location_path,path_width,
                                                    typography::CategoryPathFont::Body
                                                );
                                                let response=line.add_sized(
                                                    [path_width,theme::INPUT_HEIGHT],
                                                    crate::ui_primitives::truncated_label_for_explicit_tooltip(typography::body(&visible))
                                                );
                                                crate::ui_primitives::overflow_tooltip(response,visible!=location_path,&location_path);
                                            }
                                        );
                                        crate::ui_primitives::section_divider(modal);
                                        crate::popup::modal_labeled_row(
                                            modal,"새 분류 이름",|line|{
                                                let input_width=line.available_width().max(1.0);
                                                crate::input::flexible_text_field(
                                                    line,&mut self.new_name,input_width,
                                                    "새 하위 분류 이름"
                                                );
                                            }
                                        );
                                        let count=direct_count(catalog,&current.id);
                                        if count>0{
                                            modal.add_space(theme::SPACE_12);
                                            crate::popup::modal_nested_role_frame(crate::popup::ModalNestedRole::Standard).show(modal,|notice|{
                                                let transfer_label=if children(&cats,&current.id).is_empty(){
                                                    format!("직접 연결된 상품 {count}개를 새 하위 분류로 이전하는 데 동의합니다.")
                                                }else{
                                                    format!("기존 직접 연결 상품 {count}개는 현재 분류에 남습니다. 하위 분류 추가를 확인했습니다.")
                                                };
                                                crate::checkbox::checkbox(
                                                    notice,&mut self.transfer,&transfer_label,true
                                                );
                                            });
                                        }
                                    }
                                    Operation::Move=>{
                                        modal.label(typography::body(
                                            "분류 ID와 기존 상품 연결은 유지하고 상위 분류만 변경합니다."
                                        ));
                                        let choices=cats.iter().filter(|row|{
                                            row.active && row.id!=current.id
                                            && !under(&cats,&row.id,&current.id)
                                            && path(&cats,&row.id).iter().all(|c|c.active)
                                        }).collect::<Vec<_>>();
                                        // A move can only target active categories
                                        // outside the current subtree. Reuse the
                                        // same hierarchical popup without inventing
                                        // an alternative category hierarchy.
                                        let allowed=choices.iter()
                                            .map(|row|row.id.as_str())
                                            .collect::<HashSet<_>>();
                                        let picker_nodes=
                                            category_dropdown::nodes_from_catalog(catalog)
                                            .into_iter()
                                            .filter(|node|allowed.contains(node.id.as_str()))
                                            .collect::<Vec<_>>();
                                        crate::popup::modal_labeled_row(
                                            modal,"상위 분류",|line|{
                                                let picker_width=line.available_width()
                                                    .min(theme::CATEGORY_PATH_PICKER_WIDTH).max(1.0);
                                                category_dropdown::category_dropdown(
                                                    line,&picker_nodes,&mut self.target,
                                                    picker_width,"상위 분류 선택",true,false
                                                );
                                            }
                                        );
                                    }
                                    Operation::Delete=>{
                                        modal.label(typography::body(
                                            &format!("{} 분류를 삭제할까요? 상품과 하위 분류가 없어야 삭제할 수 있습니다.",
                                                current.name)
                                        ));
                                    },
                                };
                            }
                        });
                        decision.actions(|right|{
                            submit=crate::button::action(right,
                                match operation{
                                    Operation::Add=>"분류 생성",
                                    Operation::Move=>"위치 변경",
                                    Operation::Delete=>"분류 삭제",
                                },
                                if operation==Operation::Delete{
                                    theme::ButtonRole::Danger
                                }else{theme::ButtonRole::Primary},
                                editable&&!busy && match operation{
                                    Operation::Add=>!self.new_name.trim().is_empty()
                                        && (self.current(&cats).is_some_and(|cur|
                                            direct_count(catalog,&cur.id)==0)||self.transfer),
                                    Operation::Move=>!self.target.is_empty(),
                                    Operation::Delete=>true,
                                }
                            ).clicked();
                        });
                    });
                });
            if cancel{self.request_close_operation(&cats);}
            else if submit{
                match self.write_request(&cats,catalog){
                    Ok(req)=>write=Some(req),
                    Err(e)=>self.error=Some(e),
                }
            }
        }
        if let Some(intent)=self.confirm {
            crate::popup::modal_backdrop(ctx,"category-pending-dim",true);
            let decision=crate::popup::show_save_confirmation(
                ctx,work_area,"category-unsaved-confirm","저장하지 않은 카테고리",
                "카테고리 변경사항을 저장할까요?","저장하고 이동",!busy,editable&&!busy
            );
            let cancel=decision==crate::popup::ChangeDecision::Cancel;
            let discard=decision==crate::popup::ChangeDecision::Discard;
            let save=decision==crate::popup::ChangeDecision::Save;
            if cancel{self.confirm=None;}
            else if discard{
                if matches!(intent,Confirm::SwitchTab|Confirm::GlobalLeave){
                    self.discard(&cats);
                }
                self.apply_confirm(intent,&cats);
            }else if save{
                match self.write_request(&cats,catalog){
                    Ok(req)=>{
                        self.after_save=Some(intent);
                        self.confirm=None;
                        write=Some(req);
                    },
                    Err(e)=>self.error=Some(e),
                }
            }
        }
        write
    }
}

#[cfg(test)]
mod modal_layout_contract_tests{
    use super::*;
    #[test]
    fn navigation_reset_clears_cached_map_and_focus(){
        let mut manager=CategoryManager::default();
        manager.focused=true;manager.query="보면대".into();
        let catalog=json!({"revision":1,"products":[]});
        let cats=categories(&catalog);
        let first=manager.map_index(&cats,&catalog);
        let second=manager.map_index(&cats,&catalog);
        assert!(std::sync::Arc::ptr_eq(&first,&second));
        manager.query.clear();
        let changed=manager.map_index(&cats,&catalog);
        assert!(std::sync::Arc::ptr_eq(&first.totals,&changed.totals));
        manager.mode_motion=Some(ModeMotion{target:false,started:0.0,duration:0.45});
        manager.reset_navigation();
        assert!(manager.mode_motion.is_none());
        assert!(!manager.focused);assert!(manager.query.is_empty());
        assert!(manager.map_cache.is_none());
        assert_eq!(manager.scroll_epoch,1);
    }
    #[test]
    fn mode_transition_fades_out_then_in_for_both_directions(){
        for target in [true,false]{
            let m=ModeMotion{target,started:0.0,duration:1.0};
            assert_eq!(m.phase(0.0),(!target,1.0,false));
            assert!(m.phase(0.25).1<1.0);
            assert_eq!(m.phase(0.5),(target,0.0,false));
            assert!(m.phase(0.75).1>0.0);
            assert_eq!(m.phase(1.0),(target,1.0,true));
        }
    }
    #[test]
    fn category_map_focus_switch_is_in_the_shared_trailing_toolbar(){
        let source=include_str!("category_manager.rs");
        let toolbar=source.split("    fn map_toolbar_actions(").nth(1).unwrap()
            .split("    pub fn show_page(").next().unwrap();
        assert!(toolbar.find("선택 분류 편집").unwrap()<toolbar.find("crate::checkbox::checkbox(actions").unwrap());
        let page=source.split("    pub fn show_page(").nth(1).unwrap()
            .split("    pub fn show_overlays(").next().unwrap();
        assert!(!page.contains("crate::checkbox::checkbox("));
        assert!(page.contains("panel.set_opacity(alpha)"));
    }
    #[test]
    fn category_map_index_counts_each_product_once_per_ancestor(){
        let cats=categories(&json!({"categories":[
            {"id":"a","name":"악기","parentId":"catalog-root:BANDOSTAND"},
            {"id":"b","name":"보면대","parentId":"a"},
            {"id":"c","name":"접이식","parentId":"b"},
            {"id":"d","name":"고정식","parentId":"b"}
        ]}));
        let catalog=json!({"products":[
            {"id":"p1","categories":["b","c"]},
            {"id":"p2","categories":["c"]},
            {"id":"p3","categories":["d"]},
            {"id":"p4","categories":["c"],"deletedAt":"2026-01-01"},
            {"id":"p5","categories":["d"],"supersededBy":"p6"},
            {"id":"p6","categories":["missing"]}
        ]});
        let index=CategoryMapIndex::new(&cats,&catalog,"");
        for (id,expected) in [
            ("catalog-root:BANDOSTAND",3),
            ("catalog-root:BANDOMALL",0),
            ("a",3),("b",3),("c",2),("d",1)
        ]{
            assert_eq!(index.total(id),expected,"{id}");
        }
        assert_eq!(index.branches("b").len(),2);
        assert!(index.shows("d"));
    }
    #[test]
    fn category_map_query_preserves_ancestor_and_descendant_visibility(){
        let cats=categories(&json!({"categories":[
            {"id":"a","name":"키보드","parentId":"catalog-root:BANDOSTAND"},
            {"id":"b","name":"X자형","parentId":"a"},
            {"id":"c","name":"Z자형","parentId":"a"},
            {"id":"d","name":"보면대","parentId":"catalog-root:BANDOSTAND"}
        ]}));
        let catalog=json!({"products":[]});
        let by_leaf=CategoryMapIndex::new(&cats,&catalog,"X자형");
        for id in ["catalog-root:BANDOSTAND","a","b"]{
            assert!(by_leaf.shows(id),"{id} must be visible");
        }
        for id in ["c","d","catalog-root:BANDOMALL"]{
            assert!(!by_leaf.shows(id),"{id} must be hidden");
        }
        let by_parent=CategoryMapIndex::new(&cats,&catalog,"키보드");
        for id in ["catalog-root:BANDOSTAND","a","b","c"]{
            assert!(by_parent.shows(id),"{id} must be visible");
        }
        assert!(!by_parent.shows("d"));
        let by_path=CategoryMapIndex::new(&cats,&catalog,"키보드 > Z자형");
        assert!(by_path.shows("c"));
        assert!(by_path.shows("a"));
        assert!(!by_path.shows("b"));
        assert!(CategoryMapIndex::new(&cats,&catalog," ").shows("d"));
    }
    #[test]
    fn category_map_does_not_scan_products_inside_each_tree_row(){
        let source=include_str!("category_manager.rs");
        let node=source.split("    fn node(").nth(1).unwrap()
            .split("    pub fn show_page(").next().unwrap();
        assert!(!node.contains("items(catalog,\"products\")"));
        assert!(!node.contains("total_count("));
        assert!(!node.contains("matches(cats"));
        assert!(node.contains("index.total(&item.id)"));
        // Look only at the rendered page: the test's own expected strings
        // must not trigger these negative assertions.
        let page=source.split("    pub fn show_page(").nth(1).unwrap()
            .split("    pub fn show_overlays(").next().unwrap();
        assert!(!page.contains("typography::section_heading(\"카테고리 맵\")"));
        assert!(!page.contains("분류 행을 클릭하면 하위 구성이 접히거나 펼쳐집니다."));
    }
    #[test]
    fn category_map_no_empty_title_row_and_toolbar_has_master_spacing(){
        let source=include_str!("category_manager.rs");
        let page=source.split("    pub fn show_page(").nth(1).unwrap()
            .split("    pub fn show_overlays(").next().unwrap();
        assert!(!page.contains("typography::section_heading("));
        assert!(!page.contains("분류 행을 클릭하면"));
        assert!(!page.contains("panel.horizontal(|header|"));
        assert!(!page.contains("panel.horizontal_wrapped(|search|"));
        assert!(page.contains("measured_management_section_card(ui"));
        assert!(page.contains("fill_management_primary_card(panel,card_min_height)"));
        assert!(page.contains("crate::category_tree::configure_tree_rows(panel);"));
        assert!(page.contains("management_toolbar_trailing_rect("));
        assert!(page.contains("theme::CATALOG_MAP_TOOLBAR_STACK_WIDTH"));
        assert!(page.contains("self.map_toolbar_actions("));
        assert_eq!(page.matches("crate::ui_primitives::section_divider(panel)").count(),2);
        let toolbar_end=page.find("// The search/actions toolbar").unwrap();
        let mode_start=page.find("if self.focused{").unwrap();
        assert!(page[toolbar_end..mode_start].contains("crate::ui_primitives::section_divider(panel);"));
        assert!(page.contains("theme::CATEGORY_EXPLORER_CRUMB_HEIGHT-theme::SECTION_DIVIDER_HEIGHT"));
        assert!(!page.contains("panel.add_space(theme::SPACE_8);\n            panel.horizontal_wrapped"));
        // Management master: 12pt above and below the 1pt structural rule.
        // Measure the shared renderer in the same zero-spacing layout as
        // show_page, rather than keeping the obsolete 16/1/16pt expectation.
        assert_eq!(theme::SECTION_DIVIDER_HEIGHT,25.0);
        let ctx=egui::Context::default();
        let mut measured=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|panel|{
                crate::category_tree::configure_tree_rows(panel);
                let strip=crate::ui_primitives::section_divider(panel);
                measured=Some(strip.height());
            });
        });
        assert_eq!(measured,Some(theme::SECTION_DIVIDER_HEIGHT));
    }
    #[test]
    fn category_editor_add_move_use_only_shared_inline_form_rows(){
        // The form contract covers all five labeled value/input slots:
        // edit name + parent, add target + name, and move parent selector.
        // Keep this source-level gate confined to the dialog UI, not to the
        // main category tree or the read-only decision/confirmation notices.
        let source=include_str!("category_manager.rs");
        let production=crate::ui_test_support::production_source_for_contract(source);
        let dialogs=production.split("    pub fn show_overlays(").nth(1).unwrap();
        let count=dialogs.matches("crate::popup::modal_labeled_row(").count();
        assert_eq!(count,5,"category modal has a nonstandard field stack");
        assert!(!dialogs.contains("body.label(typography::field_label("));
        assert!(!dialogs.contains("modal.label(typography::field_label("));
        assert!(dialogs.contains("category_dropdown::category_dropdown("));
        assert!(dialogs.contains("crate::popup::modal_flow_workspace("));
        assert!(dialogs.contains("workspace.footer("));
        assert!(dialogs.contains("crate::popup::decision_workspace("));
        assert!(dialogs.contains("decision.actions("));
        assert!(!dialogs.contains("components::modal_action_row_split("));
        assert!(!dialogs.contains("crate::popup::modal_action_row("));
    }
}
