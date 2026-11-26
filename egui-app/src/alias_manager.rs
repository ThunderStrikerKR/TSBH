//! React AliasManager.tsx and ProductSearchDialog.tsx replacement.
//! Alias collision, product matching and persistence decisions remain in
//! original catalog_clients Rust. This UI owns only explicit operator edits.
use crate::{catalog_query_bridge,category_manager::ROOTS,theme,typography};
use eframe::egui::{self,Align,Context,Layout,Rect,Ui};
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};
use std::sync::Arc;
use crate::screen_refresh::ReadRefresh;

type Warnings=HashMap<String,Vec<String>>;
#[derive(Clone,PartialEq,Eq)]
struct PickerKey{query:String,category:String,brand:String}
struct PickerResult{brands:HashSet<String>,data:Value}

fn field<'a>(v:&'a Value,k:&str)->&'a str{
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn rows(v:&Value)->&[Value]{
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn display_model(name:&str)->String{
    // UI-only equivalent of React matching.ts::internalModel.
    let text=name.strip_prefix("[가상]").map(str::trim_start).unwrap_or(name);
    let no_bd=text.get(..2).is_some_and(|s|s.eq_ignore_ascii_case("BD"));
    if no_bd {
        let rest=&text[2..];
        if rest.chars().next().is_some_and(|c|c.is_whitespace()||c.is_ascii_digit()){
            return rest.trim_start().to_owned();
        }
    }
    text.to_owned()
}
fn product<'a>(state:&'a Value,id:&str)->Option<&'a Value>{
    rows(&state["products"]).iter().find(|p|field(p,"id")==id)
}
fn alias_diagnostic(item:&Value,warnings:&Warnings)->String{
    warnings.get(field(item,"id")).filter(|v|!v.is_empty())
        .map(|issues|format!("충돌: {} / 영향 범위: {}",issues.join(" / "),
            if field(item,"clientId").is_empty(){"모든 거래처의 이후 주문"}
            else{"해당 거래처의 이후 주문"}))
        .unwrap_or_default()
}
fn is_client_active(v:&Value)->bool{v.get("deletedAt").is_none_or(Value::is_null)}
fn category_levels(catalog:&Value,selected:&str)
    ->Vec<(Option<String>,String,Vec<(String,String)>)>{
    // React categoryFilterLevels: all selected ancestors each reveal one
    // more dropdown, with no depth cap. Only filter widget construction is
    // local; eligible product IDs come from native Rust catalog queries.
    let mut all=rows(&catalog["categories"]).to_vec();
    for (id,name) in ROOTS {
        if !all.iter().any(|v|field(v,"id")==id){
            all.push(json!({"id":id,"name":name,"parentId":null,"active":true}));
        }
    }
    let mut selected_path=Vec::new();
    let mut seen=HashSet::new();
    let mut current=selected;
    while !current.is_empty()&&seen.insert(current.to_owned()){
        let Some(node)=all.iter().find(|v|field(v,"id")==current)else{
            selected_path.clear();break;
        };
        selected_path.insert(0,current.to_owned());
        current=field(node,"parentId");
    }
    let mut levels=Vec::new();
    for depth in 0..=selected_path.len(){
        let parent=if depth==0{None}else{Some(selected_path[depth-1].clone())};
        let choices=all.iter().filter(|v|{
            v["active"]!=false &&
            v.get("parentId").and_then(Value::as_str)==parent.as_deref()
        }).map(|v|(field(v,"id").to_owned(),field(v,"name").to_owned()))
          .collect::<Vec<_>>();
        if depth>0 && choices.is_empty(){break;}
        let value=selected_path.get(depth).cloned().unwrap_or_default();
        levels.push((parent,value,choices));
    }
    levels
}
#[derive(Clone,PartialEq,Eq)]
struct Form {
    text:String,
    target:String,
    scope:String,
    editing:Option<String>,
}
impl Default for Form {
    fn default()->Self{
        Self{text:String::new(),target:String::new(),
            scope:"common".into(),editing:None}
    }
}
#[derive(Clone,PartialEq,Eq)]
enum Exit {Select(String),Close,Tab(usize),Global}
#[derive(Default)]
pub struct AliasManager{
    form:Form,
    original:Form,
    editor_open:bool,
    list_query:String,
    product_search:bool,
    query:String,
    category:String,
    brand:String,
    pending:Option<Exit>,
    after_save:Option<Exit>,
    next_tab:Option<usize>,
    global_approved:bool,
    error:Option<String>,
    catalog_snapshot:Option<Arc<Value>>,
    diagnostics:ReadRefresh<Warnings>,
    warnings:Arc<Warnings>,
    picker_catalog:Option<Arc<Value>>,
    picker_key:Option<PickerKey>,
    picker:ReadRefresh<PickerResult>,
    picker_result:Option<Arc<PickerResult>>,
}
impl AliasManager {
    /// Called only when ProductManager accepts a catalog snapshot, never on paint.
    pub fn set_catalog(&mut self,catalog:&Value,ctx:&Context){
        if self.picker_catalog.as_deref()!=Some(catalog){
            self.picker_catalog=Some(Arc::new(catalog.clone()));
            self.picker_key=None;
            self.picker_result=None;
            self.picker.reset();
        }
        let snapshot=json!({"products":catalog["products"],"aliases":catalog["aliases"]});
        if self.catalog_snapshot.as_deref()==Some(&snapshot){return;}
        let snapshot=Arc::new(snapshot);
        self.catalog_snapshot=Some(Arc::clone(&snapshot));
        self.warnings=Arc::default();
        self.diagnostics.submit(ctx.clone(),move||{
            let problems=catalog_query_bridge::alias_problems(&snapshot)?;
            Ok(rows(&problems).iter().map(|row|{
                (field(row,"id").to_owned(),rows(&row["problems"]).iter()
                    .filter_map(Value::as_str).map(str::to_owned).collect())
            }).collect())
        });
    }
    fn poll_diagnostics(&mut self){
        if let Some(result)=self.diagnostics.poll(){
            match result{
                Ok(warnings)=>self.warnings=Arc::new(warnings),
                Err(error)=>self.error=Some(error),
            }
        }
    }
    fn request_picker(&mut self,ctx:&Context){
        let key=PickerKey{query:self.query.clone(),category:self.category.clone(),
            brand:self.brand.clone()};
        if self.picker_key.as_ref()==Some(&key){return;}
        let Some(catalog)=self.picker_catalog.clone()else{return};
        self.picker_key=Some(key.clone());
        self.picker_result=None;
        self.picker.submit(ctx.clone(),move||{
            let classified=catalog_query_bridge::classified_products(
                &json!({"state":&*catalog,"category":key.category}))?;
            let brands=rows(&classified).iter().filter_map(Value::as_str)
                .filter_map(|id|product(&catalog,id))
                .map(|p|field(p,"brandId").to_owned()).collect();
            let data=catalog_query_bridge::search_products(&json!({
                "state":&*catalog,"query":key.query,"major":key.category,
                "brand":key.brand,"excludeDemo":true
            }))?;
            Ok(PickerResult{brands,data})
        });
    }
    fn poll_picker(&mut self){
        if let Some(result)=self.picker.poll(){
            match result{
                Ok(result)=>self.picker_result=Some(Arc::new(result)),
                Err(error)=>self.error=Some(error),
            }
        }
    }
    fn dirty(&self)->bool{self.form!=self.original}
    pub fn has_overlay(&self)->bool{self.editor_open||self.product_search||self.pending.is_some()}
    pub fn global_leave_in_progress(&self)->bool{
        self.pending==Some(Exit::Global)||self.after_save==Some(Exit::Global)
    }
    pub fn take_global_approval(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn take_tab(&mut self)->Option<usize>{self.next_tab.take()}
    pub fn reset_navigation(&mut self){
        self.editor_open=false;self.list_query.clear();
        self.picker.reset();
        self.picker_key=None;
        self.picker_result=None;
        self.product_search=false;
        self.pending=None;
        self.after_save=None;
        self.query.clear();self.category.clear();self.brand.clear();
        self.form=Form::default();
        self.original=self.form.clone();
        self.error=None;
    }
    pub fn request_tab_leave(&mut self,tab:usize)->bool{
        if self.product_search{return false;}
        if self.dirty(){self.pending=Some(Exit::Tab(tab));false}
        else{true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.product_search{return false;}
        if self.dirty(){self.pending=Some(Exit::Global);false}
        else{true}
    }
    fn apply_exit(&mut self,next:Exit,catalog:&Value){
        match next{
            Exit::Select(id)=>{
                if let Some(row)=rows(&catalog["aliases"])
                    .iter().find(|a|field(a,"id")==id){
                    self.form=Form{
                        text:field(row,"text").into(),
                        target:field(row,"productId").into(),
                        scope:if field(row,"clientId").is_empty(){
                            "common".into()
                        }else{field(row,"clientId").into()},
                        editing:Some(id),
                    };
                    self.editor_open=true;self.error=None;
                    self.original=self.form.clone();
                }
            }
            Exit::Close=>{self.close_editor();}
            Exit::Tab(tab)=>{self.close_editor();self.next_tab=Some(tab);}
            Exit::Global=>{self.close_editor();self.global_approved=true;}
        }
    }
    fn choose(&mut self,id:String,catalog:&Value){
        if self.form.editing.as_deref()==Some(&id){return;}
        if self.dirty(){self.pending=Some(Exit::Select(id));}
        else{self.apply_exit(Exit::Select(id),catalog);}
    }
    fn close_editor(&mut self){
        self.editor_open=false;self.product_search=false;
        let scope=self.original.scope.clone();
        self.form=Form{scope,..Form::default()};self.original=self.form.clone();self.error=None;
    }
    fn save_request(&self)->Value{
        json!({"record":{
            "id":self.form.editing.as_deref().unwrap_or(""),
            "text":self.form.text,
            "productId":self.form.target,
            "clientId":if self.form.scope=="common"{
                Value::Null
            }else{json!(self.form.scope)}
        }})
    }
    pub fn on_saved(&mut self,catalog:&Value){
        self.editor_open=false;self.product_search=false;
        // Preserve the last saved scope for the next registration.
        let scope=self.form.scope.clone();
        self.form=Form{scope,..Form::default()};
        self.original=self.form.clone();
        self.error=None;
        if let Some(next)=self.after_save.take(){self.apply_exit(next,catalog);}
    }
    pub fn on_failed(&mut self,message:String){
        self.error=Some(message);
        self.after_save=None;
    }
    pub fn show_page(&mut self,ui:&mut Ui,catalog:&Value,editable:bool,busy:bool)
        ->Option<Value>{
        let mut choose=None;
        self.poll_diagnostics();
        let products:HashMap<&str,&Value>=rows(&catalog["products"]).iter().rev().map(|p|(field(p,"id"),p)).collect();
        let clients:HashMap<&str,&Value>=rows(&catalog["clients"]).iter().rev().map(|c|(field(c,"id"),c)).collect();
        let warnings=Arc::clone(&self.warnings);
        let card_min_height=crate::card::management_master_table_card_min_inner_height(ui);
        crate::card::measured_management_master_card(ui,|card|{
            crate::card::fill_management_primary_card(card,card_min_height);
            card.horizontal(|toolbar|{
                let width=theme::CATALOG_SEARCH_WIDTH.min((toolbar.available_width()-160.0).max(1.0));
                crate::input::management_search_field(toolbar,&mut self.list_query,width,"별칭, 적용 범위 또는 연결 상품 검색");
                toolbar.with_layout(Layout::right_to_left(Align::Center),|actions|{
                    if crate::button::action(actions,"+ 별칭 추가",theme::ButtonRole::Primary,
                        editable&&!busy&&!self.editor_open).clicked(){
                        self.close_editor();self.editor_open=true;
                    }
                });
            });
            crate::ui_primitives::section_divider(card);
            use typography::ManagementTableCellRole as Role;
            let query=self.list_query.trim().to_lowercase();
            let aliases=rows(&catalog["aliases"]).iter().filter(|item|{
                let scope=if field(item,"clientId").is_empty(){"회사 공통"}else{
                    clients.get(field(item,"clientId")).map(|c|field(c,"name")).unwrap_or("거래처 확인 필요")};
                query.is_empty()||field(item,"text").to_lowercase().contains(&query)
                    ||scope.to_lowercase().contains(&query)
                    ||products.get(field(item,"productId")).is_some_and(|p|
                        field(p,"name").to_lowercase().contains(&query))
            }).collect::<Vec<_>>();
            let height=(card.available_height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0);
            let rect=crate::table::registry_table(card,"alias-registered-list",
                ["별칭","적용 범위","연결 상품","충돌 안내","관리"],
                [150.0,160.0,150.0,220.0,120.0],height,aliases.len(),
                |i|self.form.editing.as_deref()==Some(field(&aliases[i],"id")),
                |row,cells,i,rect|{
                    let item=&aliases[i];let id=field(item,"id");
                    let target=products.get(field(item,"productId"))
                        .map(|p|display_model(field(p,"name"))).unwrap_or_else(||"과거 상품".into());
                    let scope=if field(item,"clientId").is_empty(){"회사 공통"}
                        else{clients.get(field(item,"clientId")).map(|c|field(c,"name")).unwrap_or("거래처 확인 필요")};
                    let diagnostic=alias_diagnostic(item,&warnings);
                    crate::table::registry_cell(row,cells[0],field(item,"text"),Role::Text,false,(id,0));
                    crate::table::registry_cell(row,cells[1],scope,Role::Text,false,(id,1));
                    crate::table::registry_cell(row,cells[2],&target,Role::ProductName,false,(id,2));
                    crate::table::registry_cell(row,cells[3],&diagnostic,Role::Status,false,(id,3));
                    crate::table::registry_actions(row,cells[4],|actions|{
                        if crate::button::action(actions,"연결 정정",theme::ButtonRole::Standard,!busy&&!self.editor_open).clicked(){
                            choose=Some(id.to_owned());
                        }
                    });
                    #[cfg(test)]
                    crate::ui_test_support::record("catalog data row",rect);
                    let _=rect;
                });
            if aliases.is_empty(){
                crate::status_notice::management_body_state_overlay(card,rect,Some(if query.is_empty(){"등록된 별칭이 없습니다."}else{"검색 결과가 없습니다."}));
            }

        });
        if let Some(id)=choose{self.choose(id,catalog);}
        None
    }
    fn show_editor(&mut self,ctx:&Context,work_area:Rect,catalog:&Value,editable:bool,busy:bool)->Option<Value>{
        if !self.editor_open||self.product_search{return None;}
        let title=if self.form.editing.is_some(){"별칭 정정"}else{"별칭 추가"};
        let mut close=false;let mut save=false;
        let nested=self.pending.is_some();
        crate::popup::modal_backdrop(ctx,"alias-editor-dim",false);
        crate::popup::modal_window(ctx,title,"alias-editor",work_area)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|modal|{
                crate::popup::modal_flow_workspace(modal,|workspace|{
                    close=workspace.header(title,"닫기",!busy&&!nested);
                    workspace.scroll_region("alias-editor-body",crate::popup::modal_editor_body_max_height(work_area.height()),|body|{
                        body.add_enabled_ui(editable&&!busy&&!nested,|body|{
                            body.label(typography::field_label("별칭"));
                            let width=body.available_width();
                            crate::input::flexible_text_field(body,&mut self.form.text,width,"관리 별칭");
                            body.label(typography::field_label("적용 범위"));
                            let scope_name=if self.form.scope=="common"{"회사 공통 별칭"}else{
                                rows(&catalog["clients"]).iter().find(|c|field(c,"id")==self.form.scope)
                                    .map(|c|field(c,"name")).unwrap_or("거래처 확인 필요")};
                            let width=body.available_width();
                            crate::dropdown::management_dropdown(body,scope_name,width,true,|choices|{
                                crate::dropdown::management_dropdown_option(choices,&mut self.form.scope,"common".to_owned(),"회사 공통 별칭");
                                for client in rows(&catalog["clients"]).iter().filter(|c|is_client_active(c)){
                                    crate::dropdown::management_dropdown_option(choices,&mut self.form.scope,
                                        field(client,"id").to_owned(),field(client,"name"));
                                }
                            });
                            body.label(typography::field_label("연결 상품"));
                            let label=product(catalog,&self.form.target).map(|p|display_model(field(p,"name")))
                                .unwrap_or_else(||"등록 상품 선택".into());
                            if crate::button::action(body,&label,theme::ButtonRole::Standard,true).clicked(){
                                self.product_search=true;self.query.clear();self.category.clear();self.brand.clear();
                            }
                        });
                        if let Some(error)=self.error.as_deref(){body.label(typography::caption(error));}
                    });
                    workspace.footer_single(|actions|{
                        save=crate::button::action(actions,"별칭 저장",theme::ButtonRole::Primary,
                            editable&&!busy&&!nested&&self.dirty()).clicked();
                    });
                });
            });
        if close{if self.dirty(){self.pending=Some(Exit::Close);}else{self.close_editor();}}
        save.then(||self.save_request())
    }
    pub fn show_dialogs(&mut self,ctx:&Context,work_area:Rect,catalog:&Value,
        editable:bool,busy:bool)->Option<Value>{
        self.poll_picker();
        let mut save=self.show_editor(ctx,work_area,catalog,editable,busy);
        if self.product_search{
            let mut close=false;
            let mut chosen=None;
            crate::popup::modal_backdrop(ctx,"alias-product-search-dim",false);
            crate::popup::modal_window(ctx,"상품 검색","alias-product-search",work_area)
                .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
                .show(ctx,|modal|{

                    crate::popup::modal_flow_workspace(modal,|workspace|{
                        if workspace.header("상품 검색","닫기",!busy){
                            close=true;
                        }
                        workspace.body_region(|modal|{
                            let width=modal.available_width();
                            crate::input::management_search_field(modal,&mut self.query,
                                width,"브랜드 / 상품명 / 모델 / 별칭 검색");
                            modal.horizontal_wrapped(|filters|{
                                for (depth,(parent,selected,children)) in
                                    category_levels(catalog,&self.category).into_iter().enumerate(){
                                    let mut value=selected.clone();
                                    let label=children.iter()
                                        .find(|(id,_)|*id==value)
                                        .map(|(_,name)|name.as_str())
                                        .unwrap_or(if depth==0{
                                            "전체 카테고리"
                                        }else{"현재 분류 전체"});
                                    let picker_width=theme::CATEGORY_PATH_PICKER_WIDTH;
                                    let text_width=picker_width-
                                        theme::CATEGORY_PATH_PICKER_TEXT_PADDING;
                                    let visible=typography::fit_category_path(
                                        filters,label,text_width,
                                        typography::CategoryPathFont::Body
                                    );
                                    crate::dropdown::management_dropdown_full_label(
                                        filters,&visible,label,picker_width,true,|choice|{
                                            crate::dropdown::management_dropdown_option(
                                                choice,&mut value,String::new(),
                                                if depth==0{"전체 카테고리"}else{"현재 분류 전체"});
                                            for (id,name) in &children{
                                                let visible=typography::fit_category_path(
                                                    choice,name,text_width,
                                                    typography::CategoryPathFont::Body
                                                );
                                                let response=crate::dropdown::management_dropdown_option(
                                                    choice,&mut value,id.clone(),&visible
                                                );
                                                crate::ui_primitives::shortened_text_tooltip(response,&visible,name.as_str());
                                            }
                                        });
                                    if value!=selected{
                                        self.category=if value.is_empty(){
                                            parent.unwrap_or_default()
                                        }else{value};
                                    }
                                }
                                let available=self.picker_result.as_ref()
                                    .filter(|_|self.picker_key.as_ref()
                                        .is_some_and(|key|key.category==self.category))
                                    .map(|result|result.brands.clone()).unwrap_or_default();
                                let selected_brand=self.brand.clone();
                                let brand=rows(&catalog["brands"]).iter()
                                    .find(|b|field(b,"id")==selected_brand)
                                    .map(|b|field(b,"name")).unwrap_or("전체 브랜드");
                                crate::dropdown::management_dropdown(
                                    filters,brand,theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH,
                                    true,|choice|{
                                        crate::dropdown::management_dropdown_option(
                                            choice,&mut self.brand,String::new(),"전체 브랜드");
                                        for b in rows(&catalog["brands"])
                                            .iter().filter(|b|{
                                                available.contains(field(b,"id")) ||
                                                    field(b,"id")==selected_brand
                                            }){
                                            crate::dropdown::management_dropdown_option(
                                                choice,&mut self.brand,field(b,"id").to_owned(),
                                                field(b,"name"));
                                        }
                                    });
                                if !self.category.is_empty() &&
                                    crate::button::action(filters,"카테고리 초기화",
                                        theme::ButtonRole::Standard,true).clicked(){
                                    self.category.clear();
                                }
                            });
                            self.request_picker(ctx);
                            let found=self.picker_result.clone();
                            crate::ui_primitives::section_divider(modal);
                            let results_height=crate::popup::modal_list_max_height(
                                modal,work_area.height(),theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT,false
                            );
                            use typography::ManagementTableCellRole as Role;
                            let detail=found.as_ref().map(|result|rows(&result.data["detailLinkIds"])
                                .iter().filter_map(Value::as_str).collect::<HashSet<_>>()).unwrap_or_default();
                            let choices=found.as_ref().map(|result|rows(&result.data["ids"]).iter()
                                .filter_map(Value::as_str).take(100)
                                .filter_map(|id|product(catalog,id).map(|p|(id,p)))
                                .collect::<Vec<_>>()).unwrap_or_default();
                            let rect=crate::table::registry_table(modal,"alias-product-results",
                                ["상품명","상세 정보","상세보기","선택"],
                                [140.0,200.0,132.0,96.0],
                                (results_height-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0),
                                choices.len(),|_|false,|row,cells,i,_|{
                                    let (id,p)=choices[i];
                                    let description=if field(p,"subtitle").is_empty(){field(p,"specification")}
                                        else{field(p,"subtitle")};
                                    crate::table::registry_cell(row,cells[0],&display_model(field(p,"name")),Role::ProductName,false,(id,0));
                                    crate::table::registry_cell(row,cells[1],description,Role::Description,false,(id,1));
                                    if detail.contains(id){
                                        if let Some(url)=rows(&p["variants"]).first().and_then(|v|v["url"].as_str()){
                                            crate::table::registry_actions(row,cells[2],|actions|{
                                                if crate::button::action(actions,"상세보기 ↗",theme::ButtonRole::Quiet,true).clicked(){
                                                    ctx.open_url(egui::OpenUrl::new_tab(url));
                                                }
                                            });
                                        }
                                    }
                                    crate::table::registry_actions(row,cells[3],|actions|{
                                        if crate::button::action(actions,"선택",theme::ButtonRole::Standard,editable&&!busy).clicked(){
                                            chosen=Some(id.to_owned());
                                        }
                                    });
                                });
                            if choices.is_empty(){
                                crate::status_notice::management_body_state_overlay(modal,rect,Some(
                                    if found.is_some(){"검색 결과가 없습니다."}else{"상품을 확인하는 중입니다."}));
                            }

                        });
                    });
                });
            if close{self.product_search=false;}
            if let Some(id)=chosen{
                self.form.target=id;
                self.product_search=false;
            }
        }
        if let Some(exit)=self.pending.clone(){
            let mut cancel=false;
            let mut discard=false;
            let mut save_now=false;
            crate::popup::modal_backdrop(ctx,"alias-unsaved-dim",true);
            crate::popup::show_decision_window(ctx,"상품 별칭","alias-unsaved-confirm",work_area,|dialog|{
                crate::popup::decision_workspace(dialog,|decision|{
                    cancel=decision.header("상품 별칭","취소",!busy
                    );
                    decision.body(|dialog|{
                        dialog.label(typography::body("상품 별칭 변경사항을 저장할까요?"));
                    });
                    decision.actions(|actions|{
                            save_now=crate::button::action(actions,"저장",
                                theme::ButtonRole::Primary,editable&&!busy).clicked();
                            discard=crate::button::action(actions,"저장 안 함",
                                theme::ButtonRole::Danger,!busy).clicked();
                        }
                    );
                });
            });
            if cancel{self.pending=None;}
            else if discard{
                self.form=self.original.clone();
                self.pending=None;
                self.apply_exit(exit,catalog);
            }else if save_now{
                self.pending=None;
                self.after_save=Some(exit);
                save=Some(self.save_request());
            }
        }
        save
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    fn finish(manager:&mut AliasManager){
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while manager.diagnostics.pending(){
            manager.poll_diagnostics();
            assert!(std::time::Instant::now()<deadline,"alias diagnostics did not finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn product_picker_uses_latest_filter_and_reuses_completed_results(){
        let ctx=Context::default();
        let mut manager=AliasManager::default();
        let catalog=json!({"products":[
            {"id":"p1","name":"BD 709","catalog":"BANDOSTAND","active":true,
             "variants":[{"id":"v1","source":"OFFICIAL"}]},
            {"id":"p2","name":"BD 708","catalog":"BANDOSTAND","active":true,
             "variants":[{"id":"v2","source":"OFFICIAL"}]}],
             "aliases":[],"categories":[],"brands":[]});
        manager.set_catalog(&catalog,&ctx);
        manager.query="709".into();manager.request_picker(&ctx);
        manager.query="708".into();manager.request_picker(&ctx);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while manager.picker.pending(){
            manager.poll_picker();
            assert!(std::time::Instant::now()<deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let result=manager.picker_result.as_ref().unwrap().clone();
        assert_eq!(result.data["ids"],json!(["p2"]));
        for _ in 0..100{manager.request_picker(&ctx);}
        assert!(!manager.picker.pending());
        assert!(Arc::ptr_eq(&result,manager.picker_result.as_ref().unwrap()));
        manager.reset_navigation();
        assert!(manager.picker_result.is_none());
    }
    #[test]
    fn diagnostic_cache_reuses_unchanged_data_and_invalidates_without_revision(){
        let ctx=Context::default();
        let mut manager=AliasManager::default();
        let mut catalog=json!({"products":[
            {"id":"p1","name":"BD 709"},{"id":"p2","name":"BD 708"}],
            "aliases":[{"id":"a1","text":"special","productId":"p1","clientId":null}]});
        manager.set_catalog(&catalog,&ctx);
        finish(&mut manager);
        assert!(manager.warnings["a1"].is_empty());
        let original=Arc::clone(&manager.warnings);
        for _ in 0..100{manager.set_catalog(&catalog,&ctx);}
        assert!(!manager.diagnostics.pending());
        assert!(Arc::ptr_eq(&original,&manager.warnings));
        // Same IDs and no revision: a changed product name must still recheck.
        catalog["products"][1]["name"]=json!("special");
        manager.set_catalog(&catalog,&ctx);
        finish(&mut manager);
        assert!(!manager.warnings["a1"].is_empty());
        catalog["aliases"][0]["text"]=json!("unique");
        manager.set_catalog(&catalog,&ctx);
        finish(&mut manager);
        assert!(manager.warnings["a1"].is_empty());
    }
    #[test]
    fn diagnostic_column_keeps_next_alias_row_fixed_when_long_warnings_arrive(){
        fn render(warnings:Warnings)->Vec<Rect>{
            let ctx=Context::default();
            crate::typography::install(&ctx);
            let mut measured=Vec::new();
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1000.0,400.0)));
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    crate::table::registry_table(ui,"test-alias-diagnostic",
                        ["별칭","충돌 안내"],[120.0,240.0],180.0,2,|_|false,|row,cells,index,rect|{
                            let item=json!({"id":format!("alias-{index}"),"clientId":""});
                            let detail=alias_diagnostic(&item,&warnings);
                            crate::table::registry_cell(row,cells[1],&detail,
                                typography::ManagementTableCellRole::Status,false,index);
                            measured.push(rect);
                        });
                });
            });
            measured
        }
        let plain=render(Warnings::new());
        let mut warnings=Warnings::new();
        warnings.insert("alias-0".into(),vec!["매우 긴 충돌 설명 ".repeat(40)]);
        let diagnosed=render(warnings);
        assert_eq!(plain.len(),2);
        assert_eq!(plain,diagnosed,"diagnostic text must not change row geometry");
    }

    #[test]
    fn category_filter_virtual_roots_are_shared_and_read_only() {
        let original=json!({"categories":[]});
        let levels=category_levels(&original,"");
        let roots=&levels[0].2;
        for (id,name) in ROOTS {
            assert!(roots.iter().any(|(key,title)|key==id && title==name));
        }
        assert_eq!(original["categories"],json!([]),
            "UI catalog roots must never mutate stored categories");
    }
}

#[cfg(test)]
mod modal_lifecycle_tests{
    use super::*;
    #[test]
    fn correction_and_close_preserve_catalog_and_clear_editor(){
        let catalog=json!({"aliases":[{"id":"a","text":"709","productId":"p","clientId":"c"}]});
        let mut manager=AliasManager::default();
        manager.choose("a".into(),&catalog);
        assert!(manager.has_overlay());assert!(!manager.dirty());
        manager.form.target="other".into();
        manager.on_failed("test error".into());
        assert!(manager.has_overlay());assert!(manager.dirty());
        manager.apply_exit(Exit::Close,&catalog);
        assert!(!manager.has_overlay());assert!(!manager.dirty());
        assert_eq!(catalog["aliases"][0]["productId"],"p");
        manager.choose("a".into(),&catalog);
        manager.on_saved(&catalog);
        assert!(!manager.has_overlay());assert!(!manager.dirty());
        assert_eq!(manager.form.scope,"c");
    }
}
