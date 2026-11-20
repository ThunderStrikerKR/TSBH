//! Native CategoryManager. React CategoryManager.tsx / applied
//! tauri-stage2-products.css own the interaction and overlay layout.
//! All add/rename/move/delete decisions are delegated to catalog_clients.
use crate::{components,theme,typography};
use eframe::egui::{self,Align,Context,Layout,Rect,Response,Sense,Ui,UiBuilder,Vec2};
use serde_json::{json,Value};
use std::collections::HashSet;

const ROOTS:[(&str,&str);2]=[
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
    workspace:Option<Rect>,
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
            workspace:None,delete_parent:None,error:None,
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
    path(cats,id).iter().map(|c|c.name.as_str()).collect::<Vec<_>>().join(" / ")
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
fn total_count(catalog:&Value,cats:&[Category],id:&str)->usize{
    items(catalog,"products").iter()
        .filter(|p|field(p,"deletedAt").is_empty()&&field(p,"supersededBy").is_empty())
        .filter(|p|items(p,"categories").iter().filter_map(Value::as_str)
            .any(|cid|under(cats,cid,id)))
        .count()
}
fn matches(cats:&[Category],node:&Category,query:&str)->bool{
    if query.trim().is_empty(){return true;}
    let q=query.trim().to_lowercase();
    path_text(cats,&node.id).to_lowercase().contains(&q) ||
    cats.iter().any(|item|under(cats,&item.id,&node.id) &&
        item.name.to_lowercase().contains(&q))
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
    pub fn on_saved(&mut self,kind:&str,catalog:&Value){
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
    fn node(&mut self,ui:&mut Ui,cats:&[Category],catalog:&Value,
        item:&Category,depth:usize){
        if !matches(cats,item,&self.query){return;}
        let branches=children(cats,&item.id);
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
        ui.scope_builder(UiBuilder::new().max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),|line|{
                line.add_space(theme::CATALOG_TREE_INDENT*depth as f32
                    +theme::SPACE_12);
                line.label(typography::body(
                    if branches.is_empty(){""}else if open{"⌄"}else{"›"}
                ));
                line.label(if is_root(&item.id){
                    typography::subsection_heading(&item.name)
                }else{typography::body(&item.name)});
                line.with_layout(Layout::right_to_left(Align::Center),|right|{
                    right.label(typography::caption(
                        &total_count(catalog,cats,&item.id).to_string()
                    ));
                    if !branches.is_empty(){
                        right.label(typography::caption(&format!("하위 {}",branches.len())));
                    }
                });
            });
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
            let next=branches.into_iter().cloned().collect::<Vec<_>>();
            for child in next{
                self.node(ui,cats,catalog,&child,depth+1);
            }
        }
    }
    pub fn show_page(&mut self,ui:&mut Ui,catalog:&Value,editable:bool,busy:bool){
        let cats=categories(catalog);
        let root=theme::section_frame().show(ui,|panel|{
            let header=panel.available_width();
            panel.label(typography::section_heading("카테고리 맵"));
            panel.label(typography::caption(
                "분류 행을 클릭하면 하위 구성이 접히거나 펼쳐집니다."
            ));
            egui::ScrollArea::horizontal()
                .id_salt("category-map-header-actions")
                .show(panel,|toolbar|{
                    toolbar.horizontal(|buttons|{
                        if theme::action(buttons,"전체 접기",
                            theme::ButtonRole::Standard,!busy).clicked(){
                            self.expanded.clear();
                        }
                        if theme::action(buttons,"전체 펼치기",
                            theme::ButtonRole::Standard,!busy).clicked(){
                            self.expanded=cats.iter().map(|c|c.id.clone()).collect();
                        }
                        if theme::action(buttons,"선택 분류 편집",
                            theme::ButtonRole::Standard,!busy &&
                                self.current(&cats).is_some()).clicked(){
                            if let Some(current)=self.current(&cats){
                                self.name=current.name;
                                self.editor=true;
                            }
                        }
                        if theme::action(buttons,"+ 하위 분류 추가",
                            theme::ButtonRole::Primary,editable&&!busy &&
                                self.current(&cats).is_some()).clicked(){
                            self.begin_operation(&cats,catalog,Operation::Add);
                        }
                    });
                });
            panel.add_space(theme::SPACE_8);
            panel.horizontal_wrapped(|search|{
                search.label(typography::field_label("상품 검색"));
                components::flexible_text_field(search,&mut self.query,
                    (header-theme::SPACE_24).max(theme::MANAGEMENT_SEARCH_MIN_WIDTH),
                    "카테고리 이름 또는 전체 경로 검색");
            });
            if let Some(current)=self.current(&cats){
                panel.label(typography::caption(&format!(
                    "선택: {}",path_text(&cats,&current.id)
                )));
            }
            if let Some(error)=&self.error{
                panel.label(typography::status(error,theme::DANGER));
            }
            components::soft_divider(panel);
            egui::ScrollArea::vertical()
                .id_salt("category-map-tree")
                .auto_shrink([false,false])
                .max_height(panel.available_height().max(theme::MANAGEMENT_TABLE_ROW_HEIGHT))
                .show(panel,|tree|{
                    for (id,_) in ROOTS{
                        if let Some(current)=cats.iter().find(|c|c.id==id){
                            self.node(tree,&cats,catalog,current,0);
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
                        for row in &legacy{self.node(tree,&cats,catalog,row,0);}
                    }
                });
        });
        self.workspace=Some(root.response.rect);
    }
    pub fn show_overlays(&mut self,ctx:&Context,work_area:Rect,
        catalog:&Value,editable:bool,busy:bool)->Option<CategoryWrite>{
        let cats=categories(catalog);
        let mut write=None;
        if self.editor{
            if let Some(frame)=self.workspace{
                let width=theme::CATALOG_EDITOR_WIDTH.min(frame.width()*0.94);
                let edit_rect=Rect::from_min_max(
                    egui::pos2(frame.right()-width,frame.top()),frame.right_bottom()
                );
                egui::Area::new(egui::Id::new("native-category-editor"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(frame.min)
                    .show(ctx,|area|{
                        area.set_min_size(frame.size());
                        area.painter().rect_filled(
                            Rect::from_min_size(area.min_rect().min,frame.size()),
                            0.0,egui::Color32::from_black_alpha(72)
                        );
                        let outside=Rect::from_min_max(frame.min,
                            egui::pos2(edit_rect.left(),frame.bottom()));
                        if area.interact(outside,
                            area.make_persistent_id("category-editor-outside"),
                            Sense::click()).clicked(){
                            self.request_close_editor(&cats);
                        }
                        area.scope_builder(UiBuilder::new().max_rect(edit_rect)
                            .layout(Layout::top_down(Align::Min)),|editor|{
                                theme::section_frame().show(editor,|body|{
                                    if let Some(current)=self.current(&cats){
                                        body.horizontal(|heading|{
                                            heading.label(typography::section_heading(
                                                &current.name
                                            ));
                                            if theme::action(heading,"닫기 ×",
                                                theme::ButtonRole::Standard,!busy).clicked(){
                                                self.request_close_editor(&cats);
                                            }
                                        });
                                        body.label(typography::caption(
                                            &path_text(&cats,&current.id)
                                        ));
                                        components::soft_divider(body);
                                        body.horizontal_wrapped(|summary|{
                                            summary.label(typography::caption(
                                                &format!("직접 연결 상품 {}",direct_count(catalog,&current.id))
                                            ));
                                            summary.label(typography::caption(
                                                &format!("하위 포함 상품 {}",total_count(catalog,&cats,&current.id))
                                            ));
                                            summary.label(typography::caption(
                                                &format!("직속 하위 분류 {}",children(&cats,&current.id).len())
                                            ));
                                        });
                                        body.label(typography::field_label("분류 이름"));
                                        body.add_enabled_ui(
                                            editable && !busy && !is_root(&current.id),
                                            |field_ui|{
                                                components::flexible_text_field(
                                                    field_ui,&mut self.name,
                                                    (width-theme::SPACE_24).max(
                                                        theme::MANAGEMENT_SEARCH_MIN_WIDTH),
                                                    "분류 이름"
                                                );
                                            }
                                        );
                                        body.label(typography::field_label("상위 카테고리"));
                                        body.horizontal_wrapped(|location|{
                                            let parent_label=if current.parent.is_empty(){
                                                if is_root(&current.id){
                                                    "최상위".to_owned()
                                                }else{"소속 확인 필요".to_owned()}
                                            }else{path_text(&cats,&current.parent)};
                                            location.label(typography::body(&parent_label));
                                            if theme::action(location,"위치 변경",
                                                theme::ButtonRole::Standard,
                                                editable&&!busy&&!is_root(&current.id)
                                            ).clicked(){
                                                self.begin_operation(&cats,catalog,Operation::Move);
                                            }
                                        });
                                        if !is_root(&current.id)&&current.parent.is_empty(){
                                            body.label(typography::caption(
                                                "현재 최상위로 남아 있는 기존 분류입니다. 반도스탠드 또는 반도몰에 직접 소속을 지정하면 기존 분류 ID와 상품 연결은 유지됩니다."
                                            ));
                                        }
                                        if let Some(error)=&self.error{
                                            body.label(typography::status(error,theme::DANGER));
                                        }
                                        body.add_space(theme::SPACE_16);
                                        components::soft_divider(body);
                                        body.horizontal(|footer|{
                                            if theme::action(footer,"분류 삭제",
                                                theme::ButtonRole::Danger,
                                                editable&&!busy&&!is_root(&current.id)
                                                &&children(&cats,&current.id).is_empty()
                                                &&direct_count(catalog,&current.id)==0
                                            ).clicked(){
                                                self.begin_operation(&cats,catalog,Operation::Delete);
                                            }
                                            if theme::action(footer,"변경사항 저장",
                                                theme::ButtonRole::Primary,
                                                editable&&!busy&&self.rename_dirty(&cats)
                                            ).clicked(){
                                                match self.write_request(&cats,catalog){
                                                    Ok(req)=>write=Some(req),
                                                    Err(e)=>self.error=Some(e),
                                                }
                                            }
                                        });
                                    }
                                });
                            });
                    });
            }
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
            components::decision_window(title,"category-operation-modal",work_area)
                .show(ctx,|modal|{
                    if let Some(current)=self.current(&cats){
                        match operation{
                            Operation::Add=>{
                                modal.label(typography::body(&format!(
                                    "생성 위치: {}",path_text(&cats,&current.id)
                                )));
                                modal.label(typography::field_label("새 분류 이름"));
                                components::flexible_text_field(modal,&mut self.new_name,
                                    theme::MANAGEMENT_SEARCH_MIN_WIDTH,"새 분류 이름");
                                let count=direct_count(catalog,&current.id);
                                if count>0{
                                    modal.checkbox(&mut self.transfer,
                                        if children(&cats,&current.id).is_empty(){
                                            format!("직접 연결된 상품 {count}개를 새 하위 분류로 이전하는 데 동의합니다.")
                                        }else{
                                            format!("기존 직접 연결 상품 {count}개는 현재 분류에 남습니다. 하위 분류 추가를 확인했습니다.")
                                        });
                                }
                            }
                            Operation::Move=>{
                                modal.label(typography::body(
                                    "분류 ID와 기존 상품 연결은 유지하고 상위 분류만 변경합니다."
                                ));
                                modal.label(typography::field_label("이동할 상위 분류"));
                                let choices=cats.iter().filter(|row|{
                                    row.active && row.id!=current.id
                                    && !under(&cats,&row.id,&current.id)
                                    && path(&cats,&row.id).iter().all(|c|c.active)
                                }).collect::<Vec<_>>();
                                egui::ComboBox::from_id_salt("category-parent-options")
                                    .selected_text(path_text(&cats,&self.target))
                                    .show_ui(modal,|ui|{
                                        for option in choices{
                                            ui.selectable_value(&mut self.target,
                                                option.id.clone(),
                                                path_text(&cats,&option.id));
                                        }
                                    });
                            }
                            Operation::Delete=>{
                                modal.label(typography::body(
                                    &format!("{} 분류를 삭제할까요? 상품과 하위 분류가 없어야 삭제할 수 있습니다.",
                                        current.name)
                                ));
                            },
                        };
                    }
                    if let Some(error)=&self.error{
                        modal.label(typography::status(error,theme::DANGER));
                    }
                    modal.add_space(theme::SPACE_16);
                    modal.horizontal(|actions|{
                        cancel=theme::action(actions,"취소",
                            theme::ButtonRole::Standard,!busy).clicked();
                        submit=theme::action(actions,
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
            components::decision_window(
                "저장하지 않은 카테고리","category-unsaved-confirm",work_area
            ).show(ctx,|modal|{
                modal.label(typography::body(
                    "카테고리 변경사항을 저장할까요?"
                ));
                modal.horizontal(|actions|{
                    cancel=theme::action(actions,"취소",
                        theme::ButtonRole::Standard,!busy).clicked();
                    discard=theme::action(actions,"저장 안 함",
                        theme::ButtonRole::Standard,!busy).clicked();
                    save=theme::action(actions,"저장하고 이동",
                        theme::ButtonRole::Primary,editable&&!busy).clicked();
                });
            });
            if cancel{self.confirm=None;}
            else if discard{
                self.discard(&cats);
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
