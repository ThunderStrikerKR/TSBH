//! Native CategoryManager. React CategoryManager.tsx / applied
//! tauri-stage2-products.css own the interaction and overlay layout.
//! All add/rename/move/delete decisions are delegated to catalog_clients.
use crate::{category_dropdown,components,theme,typography};
use eframe::egui::{self,Align,Context,Layout,Rect,Response,Sense,Ui,Vec2};
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};

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
    name:String,
    editor:bool,
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
            query:String::new(),name:String::new(),editor:false,
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
                path(&cats,id).iter().map(|node|node.name.as_str())
                    .collect::<Vec<_>>().join(" > ")
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
struct CategoryMapIndex {
    children:HashMap<String,Vec<usize>>,
    totals:HashMap<String,usize>,
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
        let q=query.trim().to_lowercase();
        let visible=if q.is_empty(){None}else{
            let mut visible=HashSet::new();
            // A matching path shows its ancestors; a matching ancestor's
            // name is already included in every descendant's full path.
            // This is equivalent to the old recursive matches() but avoids
            // scanning the whole tree once per rendered child.
            for item in cats{
                let ancestors=path(cats,&item.id);
                if ancestors.iter().map(|c|c.name.as_str())
                    .collect::<Vec<_>>().join(" > ").to_lowercase().contains(&q){
                    visible.extend(ancestors.into_iter().map(|c|c.id));
                }
            }
            Some(visible)
        };
        Self{children,totals,visible}
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
    pub fn clear_search(&mut self){self.query.clear();}
    pub fn report_error(&mut self,message:String){self.error=Some(message);}

    pub fn clear_for_leave(&mut self){
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
        match self.operation {
            Some(Operation::Add)=>{
                if self.new_name.trim().is_empty() ||
                   (direct_count(catalog,&current.id)>0 && !self.transfer){
                    return Err("분류 이름과 상품 이전 확인을 완료해 주세요.".into());
                }
                Ok(CategoryWrite{
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
                Ok(CategoryWrite{
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
                Ok(CategoryWrite{
                    action:"delete_category",
                    data:json!({"categoryId":current.id}),
                })
            }
            None=>{
                if !self.rename_dirty(cats){return Err("저장할 분류 변경사항이 없습니다.".into());}
                Ok(CategoryWrite{
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
        }
    }
    pub fn on_saved(&mut self,kind:&str,catalog:&Value)->&'static str{
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
    fn node(&mut self,ui:&mut Ui,cats:&[Category],index:&CategoryMapIndex,
        item:&Category,depth:usize,ancestor_guides:&[bool],is_last:bool){
        if !index.shows(&item.id){return;}
        let branches=index.branches(&item.id);
        let open=self.expanded.contains(&item.id)||!self.query.trim().is_empty();
        let width=ui.available_width();
        let height=if is_root(&item.id){
            theme::CATALOG_ROOT_ROW_HEIGHT
        }else{theme::CATALOG_TREE_ROW_HEIGHT};
        let (rect,response)=ui.allocate_exact_size(Vec2::new(width,height),Sense::click());
        if self.selected==item.id || response.hovered(){
            ui.painter().rect_filled(
                rect,theme::CONTROL_RADIUS as f32,
                if self.selected==item.id {theme::RAISED}else{theme::INPUT}
            );
        }
        if self.selected!=item.id{
            theme::paint_button_hover(
                ui,&response,theme::CONTROL_RADIUS as f32
            );
        }
        // A very faint row rule groups one category and its counts without
        // creating table-like zebra bands. Root boundaries are a little firmer.
        let row_rule=if is_root(&item.id){theme::DIVIDER}
            else{theme::CATALOG_TREE_ROW_RULE_COLOR};
        ui.painter().line_segment([
            egui::pos2(rect.left()+theme::SPACE_12,rect.bottom()-0.5),
            egui::pos2(rect.right()-theme::SPACE_12,rect.bottom()-0.5),
        ],egui::Stroke::new(theme::CATALOG_TREE_ROW_RULE_WIDTH,row_rule));

        let icon_x=rect.left()+theme::SPACE_12
            +theme::CATALOG_TREE_INDENT*depth as f32;
        let icon_rect=egui::Rect::from_min_size(
            egui::pos2(icon_x,rect.center().y-theme::SPACE_8),
            Vec2::splat(theme::SPACE_16)
        );
        let tree_stroke=egui::Stroke::new(
            theme::CATALOG_TREE_GUIDE_WIDTH,theme::CATALOG_TREE_GUIDE_COLOR
        );
        // TreeView-style continuation guides (│): a guide persists through
        // descendants only when that ancestor has a following visible sibling.
        let guide_root_x=rect.left()+theme::SPACE_12+theme::SPACE_8;
        for (level,continued) in ancestor_guides.iter().enumerate(){
            if *continued{
                let x=guide_root_x+theme::CATALOG_TREE_INDENT*level as f32;
                ui.painter().line_segment([
                    egui::pos2(x,rect.top()),egui::pos2(x,rect.bottom())
                ],tree_stroke);
            }
        }
        if depth>0{
            // An elbow joins the immediate parent to this category.
            // Last siblings end at the center instead of leaving a
            // dangling vertical line below the final child.
            let parent_x=guide_root_x
                +theme::CATALOG_TREE_INDENT*(depth-1) as f32;
            ui.painter().line_segment([
                egui::pos2(parent_x,rect.top()),
                egui::pos2(parent_x,if is_last{rect.center().y}else{rect.bottom()})
            ],tree_stroke);
            ui.painter().line_segment([
                egui::pos2(parent_x,rect.center().y),
                egui::pos2((icon_rect.left()-theme::SPACE_4).max(parent_x),
                    rect.center().y)
            ],tree_stroke);
        }
        if open && !branches.is_empty(){
            // Continue below an expanded parent into the first child row.
            ui.painter().line_segment([
                egui::pos2(icon_rect.center().x,
                    (icon_rect.bottom()+theme::SPACE_4).min(rect.bottom())),
                egui::pos2(icon_rect.center().x,rect.bottom())
            ],tree_stroke);
        }
        if !branches.is_empty(){
            // Match the same vector-chevron geometry used by order history,
            // workflow cards and management dropdowns. Tree guides remain.
            components::paint_management_chevron(
                ui.painter(),icon_rect.center(),
                if open{components::ManagementChevronDirection::Down}
                else{components::ManagementChevronDirection::Right}
            );
        }
        // A small monochrome folder icon mirrors the Explorer navigation
        // pane without introducing bright decorative color or font glyphs.
        let folder_x=icon_rect.right()+theme::SPACE_8;
        let folder_y=rect.center().y-theme::CATALOG_TREE_FOLDER_HEIGHT/2.0;
        let folder_right=folder_x+theme::CATALOG_TREE_FOLDER_WIDTH;
        let folder_bottom=folder_y+theme::CATALOG_TREE_FOLDER_HEIGHT;
        ui.painter().add(egui::Shape::closed_line(vec![
            egui::pos2(folder_x,folder_y+theme::SPACE_4),
            egui::pos2(folder_x,folder_y+1.0),
            egui::pos2(folder_x+theme::SPACE_4,folder_y+1.0),
            egui::pos2(folder_x+theme::SPACE_8-2.0,folder_y+theme::SPACE_4),
            egui::pos2(folder_right,folder_y+theme::SPACE_4),
            egui::pos2(folder_right,folder_bottom),
            egui::pos2(folder_x,folder_bottom)
        ],egui::Stroke::new(1.0,theme::SECONDARY)));
        let count=index.total(&item.id);
        // Show totals immediately after the category name, not in a
        // spreadsheet-style column at the far right of the workspace.
        // Count includes this category and every descendant; the child
        // count is the number of immediate subcategories.
        let summary=if branches.is_empty(){
            format!("(상품 {count})")
        }else{
            format!("(상품 {count} · 하위 분류 {})",branches.len())
        };
        let summary_galley=typography::client_table_metadata_galley(
            ui,&summary,theme::SECONDARY
        );
        let name_x=folder_right+theme::SPACE_8;
        let right_edge=rect.right()-theme::SPACE_12;
        // Reserve the count before fitting the name. Only the name loses
        // its earliest characters when the tree becomes deeply nested.
        let name_width=(right_edge-name_x-theme::SPACE_8
            -summary_galley.size().x).max(0.0);
        let font=if is_root(&item.id){
            typography::CategoryPathFont::Subsection
        }else{typography::CategoryPathFont::Body};
        let visible=typography::fit_category_path(
            ui,&item.name,name_width,font
        );
        let name_galley=typography::category_tree_name_galley(
            ui,&visible,is_root(&item.id)
        );
        let name_end=name_x+name_galley.size().x.min(name_width);
        let name_clip=egui::Rect::from_min_max(
            egui::pos2(name_x,rect.top()),
            egui::pos2(name_end.max(name_x),rect.bottom())
        );
        ui.painter().with_clip_rect(name_clip).galley(
            egui::pos2(name_x,rect.center().y-name_galley.size().y/2.0),
            name_galley,theme::TEXT
        );
        let summary_x=name_end+theme::SPACE_8;
        // Never let count glyphs paint beyond the row's right inset,
        // including in unusually narrow/deeply indented tree views.
        let summary_clip=egui::Rect::from_min_max(
            egui::pos2(summary_x.min(right_edge),rect.top()),
            egui::pos2(right_edge,rect.bottom())
        );
        ui.painter().with_clip_rect(summary_clip).galley(
            egui::pos2(summary_x,rect.center().y-summary_galley.size().y/2.0),
            summary_galley,theme::SECONDARY
        );
        let response=response.on_hover_text(&item.name);
        if response.clicked(){
            self.selected=item.id.clone();
            self.name=item.name.clone();
            if !branches.is_empty() && self.query.trim().is_empty(){
                if !self.expanded.insert(item.id.clone()){
                    self.expanded.remove(&item.id);
                }
            }
        }
        if open {
            let visible_children=branches.iter()
                .copied().filter(|&child|index.shows(&cats[child].id))
                .collect::<Vec<_>>();
            let mut child_guides=ancestor_guides.to_vec();
            if depth>0{child_guides.push(!is_last);}
            for (position,&child) in visible_children.iter().enumerate(){
                self.node(ui,cats,index,&cats[child],depth+1,
                    &child_guides,position+1==visible_children.len());
            }
        }
    }
    /// Only action controls remain from the former title row. Both wide and
    /// narrow layouts render the same actions with the approved button roles.
    fn map_toolbar_actions(
        &mut self,actions:&mut Ui,cats:&[Category],catalog:&Value,
        editable:bool,busy:bool
    ){
        if theme::action(actions,"+ 하위 분류 추가",
            theme::ButtonRole::Primary,editable&&!busy &&
                self.current(cats).is_some()).clicked(){
            self.begin_operation(cats,catalog,Operation::Add);
        }
        if theme::action(actions,"선택 분류 편집",
            theme::ButtonRole::Standard,!busy &&
                self.current(cats).is_some()).clicked(){
            if let Some(current)=self.current(cats){
                self.name=current.name;
                self.editor=true;
            }
        }
    }
    pub fn show_page(&mut self,ui:&mut Ui,catalog:&Value,editable:bool,busy:bool){
        let cats=categories(catalog);
        let card_min_height=components::management_primary_card_min_inner_height(
            ui,theme::SECTION_PADDING
        );
        theme::section_frame().show(ui,|panel|{
            components::fill_management_primary_card(panel,card_min_height);
            // No vacated heading or help-text rows. Reserve one 36pt
            // search-and-actions toolbar, exactly as the management master.
            panel.spacing_mut().item_spacing.y=0.0;
            let width=panel.available_width().max(1.0);
            if width<theme::CATALOG_MAP_TOOLBAR_STACK_WIDTH{
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
                        components::management_search_field(
                            search,&mut self.query,
                            theme::CATALOG_SEARCH_WIDTH.min(search_rect.width()),
                            "카테고리 이름 또는 전체 경로 검색"
                        );
                    }
                );
            }else{
                let (toolbar_rect,_)=panel.allocate_exact_size(
                    Vec2::new(width,theme::INPUT_HEIGHT),Sense::hover()
                );
                let actions_width=theme::CATALOG_MAP_TOOLBAR_ACTIONS_WIDTH;
                let search_width=theme::CATALOG_SEARCH_WIDTH.min(
                    (toolbar_rect.width()-actions_width-theme::SPACE_16).max(1.0)
                );
                let search_rect=Rect::from_min_size(
                    toolbar_rect.min,Vec2::new(search_width,toolbar_rect.height())
                );
                let actions_rect=components::management_toolbar_trailing_rect(
                    toolbar_rect,actions_width,0.0,toolbar_rect.height()
                );
                panel.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("category-map-search-toolbar")
                        .max_rect(search_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |search|{
                        search.set_width(search_rect.width());
                        components::management_search_field(
                            search,&mut self.query,search_rect.width(),
                            "카테고리 이름 또는 전체 경로 검색"
                        );
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
            // One explicit 21pt toolbar-to-tree transition: 16/1/4.
            // No implicit egui gaps or phantom title/help-row spacing.
            panel.add_space(theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP);
            components::soft_divider(panel);
            panel.add_space(theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP);
            let index=CategoryMapIndex::new(&cats,catalog,&self.query);
            egui::ScrollArea::vertical()
                .id_salt("category-map-tree")
                .auto_shrink([false,false])
                .max_height(panel.available_height().max(theme::MANAGEMENT_TABLE_ROW_HEIGHT))
                .show(panel,|tree|{
                    // Adjacent rows must touch so parent/child guide lines
                    // form one continuous tree rather than broken dashes.
                    tree.spacing_mut().item_spacing.y=0.0;
                    for (id,_) in ROOTS{
                        if let Some(current)=cats.iter().find(|c|c.id==id){
                            self.node(tree,&cats,&index,current,0,&[],true);
                        }
                    }
                    let legacy=cats.iter().filter(|c|c.parent.is_empty()&&!is_root(&c.id))
                        .cloned().collect::<Vec<_>>();
                    if !legacy.is_empty(){
                        tree.add_space(theme::SPACE_16);
                        tree.label(typography::subsection_heading(
                            &format!("상위 분류 소속 확인 ({})",legacy.len())
                        ));
                        tree.label(typography::caption(
                            "기존 분류의 소속은 자동으로 변경하지 않습니다. 선택 후 위치를 지정하세요."
                        ));
                        for row in &legacy{
                            self.node(tree,&cats,&index,row,0,&[],true);
                        }
                    }
                });
        });
    }
    pub fn show_overlays(&mut self,ctx:&Context,work_area:Rect,
        catalog:&Value,editable:bool,busy:bool)->Option<CategoryWrite>{
        let cats=categories(catalog);
        let mut write=None;
        if self.editor{
            let nested_dialog=self.operation.is_some()||self.confirm.is_some();
            components::modal_backdrop(ctx,"category-editor-dim",false);
            // The category editor uses the same centered, Kakao-safe modal
            // geometry as other native dialogs, not a right-side drawer.
            components::modal_window(
                "선택 분류 편집","native-category-editor",work_area
            )
                .default_width(theme::MODAL_DECISION_MAX_WIDTH.min(work_area.width()))
                .show(ctx,|modal|{

                    components::modal_flow_workspace(modal,|workspace|{
                        if workspace.header("선택 분류 편집","취소",!busy&&!nested_dialog
                        ){
                            self.request_close_editor(&cats);
                        }
                        workspace.scroll_region("native-category-editor-fields",components::modal_editor_body_max_height(
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
                                    body.label(typography::subsection_heading(
                                        &name_display
                                    )).on_hover_text(&current.name);
                                    let breadcrumb=path_text(&cats,&current.id);
                                    let visible=typography::fit_category_path(
                                        body,&breadcrumb,body.available_width(),
                                        typography::CategoryPathFont::Metadata
                                    );
                                    body.label(typography::caption(&visible))
                                        .on_hover_text(&breadcrumb);
                                    // Compact context for an editor, not KPI cards.
                                    // Keep a single divider between context and fields.
                                    body.label(typography::caption(&format!(
                                        "직접 연결 상품 {}개 · 하위 분류 {}개",
                                        direct_count(catalog,&current.id),
                                        children(&cats,&current.id).len()
                                    )));
                                    // Default 8pt item gaps around the single divider
                                    // already provide enough breathing room.
                                    components::soft_divider(body);
                                    components::modal_labeled_row(body,"분류 이름",|row|{
                                        row.add_enabled_ui(
                                            editable && !busy && !is_root(&current.id),
                                            |field_ui|{
                                                let input_width=field_ui.available_width().max(1.0);
                                                components::flexible_text_field(
                                                    field_ui,&mut self.name,input_width,"분류 이름"
                                                );
                                            }
                                        );
                                    });
                                    body.add_space(theme::SPACE_12);
                                    components::modal_labeled_row(body,"상위 분류",|location|{
                                        // Align category text to the button's vertical
                                        // center; keep them adjacent at a shared 8pt gap.
                                        location.spacing_mut().item_spacing.x=theme::SPACE_8;
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
                                        text_response.on_hover_text(&parent_label);
                                        if theme::action(location,"위치 변경",
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
                                    components::fixed_status_slot(
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
                                request_delete=theme::action(left,"분류 삭제",
                                    theme::ButtonRole::Danger,
                                    editable&&!busy&&!nested_dialog&&can_delete
                                ).clicked();
                            },
                            |right|{
                                request_save=theme::action(right,"변경사항 저장",
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
            components::modal_backdrop(ctx,"category-operation-dim",self.editor);
            let title=match operation {
                Operation::Add=>"하위 카테고리 추가",
                Operation::Move=>"상위 카테고리 변경",
                Operation::Delete=>"분류 삭제 확인",
            };
            components::show_decision_window(ctx,title,"category-operation-modal",work_area,|modal|{
                    components::decision_workspace(modal,|decision|{
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
                                        components::modal_labeled_row(
                                            modal,"생성 위치",|line|{
                                                let path_width=line.available_width().max(1.0);
                                                let visible=typography::fit_category_path(
                                                    line,&location_path,path_width,
                                                    typography::CategoryPathFont::Body
                                                );
                                                line.add_sized(
                                                    [path_width,theme::INPUT_HEIGHT],
                                                    egui::Label::new(typography::body(&visible))
                                                        .truncate()
                                                ).on_hover_text(&location_path);
                                            }
                                        );
                                        modal.add_space(theme::SPACE_12);
                                        components::soft_divider(modal);
                                        modal.add_space(theme::SPACE_12);
                                        components::modal_labeled_row(
                                            modal,"새 분류 이름",|line|{
                                                let input_width=line.available_width().max(1.0);
                                                components::flexible_text_field(
                                                    line,&mut self.new_name,input_width,
                                                    "새 하위 분류 이름"
                                                );
                                            }
                                        );
                                        let count=direct_count(catalog,&current.id);
                                        if count>0{
                                            modal.add_space(theme::SPACE_12);
                                            theme::modal_nested_frame().show(modal,|notice|{
                                                let transfer_label=if children(&cats,&current.id).is_empty(){
                                                    format!("직접 연결된 상품 {count}개를 새 하위 분류로 이전하는 데 동의합니다.")
                                                }else{
                                                    format!("기존 직접 연결 상품 {count}개는 현재 분류에 남습니다. 하위 분류 추가를 확인했습니다.")
                                                };
                                                components::checkbox(
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
                                        components::modal_labeled_row(
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
                            submit=theme::action(right,
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
            let mut cancel=false;
            let mut discard=false;
            let mut save=false;
            components::modal_backdrop(ctx,"category-pending-dim",true);
            components::show_decision_window(ctx,"저장하지 않은 카테고리","category-unsaved-confirm",work_area,|modal|{
                components::decision_workspace(modal,|decision|{
                    cancel=decision.header("저장하지 않은 카테고리","취소",!busy
                    );
                    decision.body(|modal|{
                        modal.label(typography::body(
                            "카테고리 변경사항을 저장할까요?"
                        ));
                    });
                    decision.actions(|actions|{
                            save=theme::action(actions,"저장하고 이동",
                                theme::ButtonRole::Primary,editable&&!busy).clicked();
                            discard=theme::action(actions,"저장 안 함",
                                theme::ButtonRole::Danger,!busy).clicked();
                        }
                    );
                });
            });
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
        assert!(page.contains("section_frame().show(ui"));
        assert!(page.contains("fill_management_primary_card(panel,card_min_height)"));
        assert!(page.contains("panel.spacing_mut().item_spacing.y=0.0;"));
        assert!(page.contains("management_toolbar_trailing_rect("));
        assert!(page.contains("theme::CATALOG_MAP_TOOLBAR_STACK_WIDTH"));
        assert!(page.contains("self.map_toolbar_actions("));
        assert!(page.contains("theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP"));
        assert!(page.contains("theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP"));
        assert!(!page.contains("panel.add_space(theme::SPACE_8);\n            panel.horizontal_wrapped"));
        assert_eq!(
            theme::MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP
                +theme::SOFT_DIVIDER_WIDTH
                +theme::MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP,
            21.0
        );
    }
    #[test]
    fn category_editor_add_move_use_only_shared_inline_form_rows(){
        // The form contract covers all five labeled value/input slots:
        // edit name + parent, add target + name, and move parent selector.
        // Keep this source-level gate confined to the dialog UI, not to the
        // main category tree or the read-only decision/confirmation notices.
        let source=include_str!("category_manager.rs");
        let dialogs=source.split("    pub fn show_overlays(").nth(1).unwrap()
            .split("#[cfg(test)]").next().unwrap();
        let count=dialogs.matches("components::modal_labeled_row(").count();
        assert_eq!(count,5,"category modal has a nonstandard field stack");
        assert!(!dialogs.contains("body.label(typography::field_label("));
        assert!(!dialogs.contains("modal.label(typography::field_label("));
        assert!(dialogs.contains("category_dropdown::category_dropdown("));
        assert!(dialogs.contains("components::modal_flow_workspace("));
        assert!(dialogs.contains("workspace.footer("));
        assert!(dialogs.contains("components::decision_workspace("));
        assert!(dialogs.contains("decision.actions("));
        assert!(!dialogs.contains("components::modal_action_row_split("));
        assert!(!dialogs.contains("components::modal_action_row("));
    }
}
