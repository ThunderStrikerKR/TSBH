//! React AliasManager.tsx and ProductSearchDialog.tsx replacement.
//! Alias collision, product matching and persistence decisions remain in
//! original catalog_clients Rust. This UI owns only explicit operator edits.
use crate::{catalog_query_bridge,category_manager::ROOTS,components,theme,typography};
use eframe::egui::{self,Align,Context,Layout,Rect,Ui};
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};
use std::sync::Arc;
use crate::latest_job::LatestJob;

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
fn alias_diagnostic_slot(ui:&mut Ui,item:&Value,warnings:&Warnings)->Rect{
    // Always reserve the third caption line. Diagnostic arrival and message
    // length must not change this alias row's height or the next row's Y.
    let slot_width=ui.min_rect().width().max(1.0);
    let detail=warnings.get(field(item,"id")).filter(|v|!v.is_empty())
        .map(|issues|format!(
            "충돌: {} / 영향 범위: {}",
            issues.join(" / "),
            if field(item,"clientId").is_empty(){
                "모든 거래처의 이후 주문"
            }else{"해당 거래처의 이후 주문"}
        ));
    components::fixed_status_slot(
        ui,slot_width,typography::FONT_METADATA+theme::SPACE_4,
        detail.as_deref(),detail.as_deref(),theme::MUTED
    )
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
enum Exit {Select(String),Tab(usize),Global}
#[derive(Default)]
pub struct AliasManager{
    form:Form,
    original:Form,
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
    diagnostics:LatestJob<Warnings>,
    warnings:Arc<Warnings>,
    picker_catalog:Option<Arc<Value>>,
    picker_key:Option<PickerKey>,
    picker:LatestJob<PickerResult>,
    picker_result:Option<Arc<PickerResult>>,
}
impl AliasManager {
    /// Called only when ProductManager accepts a catalog snapshot, never on paint.
    pub fn set_catalog(&mut self,catalog:&Value,ctx:&Context){
        if self.picker_catalog.as_deref()!=Some(catalog){
            self.picker_catalog=Some(Arc::new(catalog.clone()));
            self.picker_key=None;
            self.picker_result=None;
            self.picker.invalidate();
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
    pub fn has_overlay(&self)->bool{self.product_search||self.pending.is_some()}
    pub fn global_leave_in_progress(&self)->bool{
        self.pending==Some(Exit::Global)||self.after_save==Some(Exit::Global)
    }
    pub fn take_global_approval(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn take_tab(&mut self)->Option<usize>{self.next_tab.take()}
    pub fn reset_navigation(&mut self){
        self.picker.invalidate();
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
        else{self.reset_navigation();true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.product_search{return false;}
        if self.dirty(){self.pending=Some(Exit::Global);false}
        else{self.reset_navigation();true}
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
                    self.original=self.form.clone();
                }
            }
            Exit::Tab(tab)=>{self.reset_navigation();self.next_tab=Some(tab);}
            Exit::Global=>{self.reset_navigation();self.global_approved=true;}
        }
    }
    fn choose(&mut self,id:String,catalog:&Value){
        if self.form.editing.as_deref()==Some(&id){return;}
        if self.dirty(){self.pending=Some(Exit::Select(id));}
        else{self.apply_exit(Exit::Select(id),catalog);}
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
        // React: reset fields, preserve the selected scope after save.
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
        let mut save=false;
        let mut choose=None;
        self.poll_diagnostics();
        let warnings=Arc::clone(&self.warnings);
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        components::measured_management_master_card(ui,|card|{
            components::fill_management_primary_card(card,card_min_height);
            card.label(typography::section_heading("공통 / 거래처 별칭 관리"));
            card.add_space(theme::SPACE_8);
            card.label(typography::body(
                "넓은 상품 종류명과 실제 개별 상품에 충돌하는 별칭은 등록하지 않습니다. 기존 충돌은 삭제하지 않고 아래에 표시합니다."
            ));
            card.add_space(theme::SPACE_12);
            // Like the React auto-fit grid, controls wrap within the form
            // while sharing the existing management field/button tokens.
            card.horizontal_wrapped(|form|{
                form.vertical(|v|{
                    v.label(typography::field_label("별칭"));
                    v.add_enabled_ui(editable&&!busy,|input|{
                        components::flexible_text_field(
                            input,&mut self.form.text,
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"관리 별칭");
                    });
                });
                form.vertical(|v|{
                    v.label(typography::field_label("적용 범위"));
                    let scope_name=if self.form.scope=="common"{
                        "회사 공통 별칭"
                    }else{
                        rows(&catalog["clients"]).iter()
                            .find(|c|field(c,"id")==self.form.scope)
                            .map(|c|field(c,"name"))
                            .unwrap_or("거래처 확인 필요")
                    };
                    v.add_enabled_ui(editable&&!busy,|input|{
                        components::management_dropdown(
                            input,scope_name,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                            true,|choices|{
                                components::management_dropdown_option(
                                    choices,&mut self.form.scope,
                                    "common".to_owned(),"회사 공통 별칭");
                                for client in rows(&catalog["clients"])
                                    .iter().filter(|c|is_client_active(c)){
                                    components::management_dropdown_option(
                                        choices,&mut self.form.scope,
                                        field(client,"id").to_owned(),field(client,"name"));
                                }
                            });
                    });
                });
                form.vertical(|v|{
                    v.label(typography::field_label("연결 상품"));
                    let label=product(catalog,&self.form.target)
                        .map(|p|display_model(field(p,"name")))
                        .unwrap_or_else(||"등록 상품 선택".into());
                    if theme::action(v,&label,theme::ButtonRole::Standard,
                        editable&&!busy).clicked(){
                        self.product_search=true;
                        self.query.clear();self.category.clear();self.brand.clear();
                    }
                });
                form.vertical(|v|{
                    // Align the single primary action with the shared field
                    // baseline without altering any shared button dimensions.
                    v.add_space(theme::SPACE_24);
                    if theme::action(v,"별칭 저장",theme::ButtonRole::Primary,
                        editable&&!busy).clicked(){save=true;}
                });
            });
            // Shared Freight master: toolbar-to-header transition,
            // fixed 36pt register header and 10pt solid scroll rail.
            components::management_table_container_style(card);
            let header=components::management_master_register_header(
                card,"별칭 / 적용 범위 / 연결 상품","관리",132.0
            );
            let scroll_height=components::management_single_list_scroll_height(
                card,card_min_height,0.0
            );
            let scroll_track=components::management_table_scroll_track_rect(
                card,scroll_height
            );
            egui::ScrollArea::vertical()
                .id_salt("alias-registered-list")
                .scroll_bar_rect(scroll_track)
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                )
                .max_height(scroll_height)
                .auto_shrink([false,false])
                .show(card,|list|{
                    list.set_min_width(header.table_width);
                    for item in rows(&catalog["aliases"]){
                        let id=field(item,"id");
                        let target=product(catalog,field(item,"productId"))
                            .map(|p|display_model(field(p,"name")))
                            .unwrap_or_else(||"과거 상품".into());
                        let scope=if field(item,"clientId").is_empty(){
                            "회사 공통"
                        }else{
                            rows(&catalog["clients"]).iter()
                                .find(|c|field(c,"id")==field(item,"clientId"))
                                .map(|c|field(c,"name")).unwrap_or("")
                        };
                        let _row=list.horizontal(|row|{
                            row.vertical(|info|{
                                info.label(typography::body(field(item,"text")));
                                info.label(typography::caption(
                                    &format!("→ {target} / {scope}")));
                                alias_diagnostic_slot(info,item,&warnings);
                            });
                            row.with_layout(Layout::right_to_left(Align::Center),|actions|{
                                if theme::action(actions,"연결 정정",
                                    theme::ButtonRole::Standard,!busy).clicked(){
                                    choose=Some(id.to_owned());
                                }
                            });
                        });
                        #[cfg(test)]
                        components::general_screen_geometry_tests::record("catalog data row",_row.response.rect);
                        components::soft_divider(list);
                    }
                });
        });
        if let Some(id)=choose{self.choose(id,catalog);}
        save.then(||self.save_request())
    }
    pub fn show_dialogs(&mut self,ctx:&Context,work_area:Rect,catalog:&Value,
        editable:bool,busy:bool)->Option<Value>{
        self.poll_picker();
        let mut save=None;
        if self.product_search{
            let mut close=false;
            let mut chosen=None;
            components::modal_backdrop(ctx,"alias-product-search-dim",false);
            components::modal_window("상품 검색","alias-product-search",work_area)
                .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
                .show(ctx,|modal|{

                    components::modal_flow_workspace(modal,|workspace|{
                        if workspace.header("상품 검색","닫기",!busy){
                            close=true;
                        }
                        workspace.body_region(|modal|{
                            let width=modal.available_width();
                            components::management_search_field(modal,&mut self.query,
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
                                    let response=components::management_dropdown(
                                        filters,&visible,picker_width,true,|choice|{
                                            components::management_dropdown_option(
                                                choice,&mut value,String::new(),
                                                if depth==0{"전체 카테고리"}else{"현재 분류 전체"});
                                            for (id,name) in &children{
                                                let visible=typography::fit_category_path(
                                                    choice,name,text_width,
                                                    typography::CategoryPathFont::Body
                                                );
                                                let response=components::management_dropdown_option(
                                                    choice,&mut value,id.clone(),&visible
                                                );
                                                components::shortened_text_tooltip(response,&visible,name.as_str());
                                            }
                                        });
                                    components::shortened_text_tooltip(response,&visible,label);
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
                                components::management_dropdown(
                                    filters,brand,theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH,
                                    true,|choice|{
                                        components::management_dropdown_option(
                                            choice,&mut self.brand,String::new(),"전체 브랜드");
                                        for b in rows(&catalog["brands"])
                                            .iter().filter(|b|{
                                                available.contains(field(b,"id")) ||
                                                    field(b,"id")==selected_brand
                                            }){
                                            components::management_dropdown_option(
                                                choice,&mut self.brand,field(b,"id").to_owned(),
                                                field(b,"name"));
                                        }
                                    });
                                if !self.category.is_empty() &&
                                    theme::action(filters,"카테고리 초기화",
                                        theme::ButtonRole::Standard,true).clicked(){
                                    self.category.clear();
                                }
                            });
                            self.request_picker(ctx);
                            let found=self.picker_result.clone();
                            components::section_divider(modal);
                            let results_height=components::modal_list_max_height(
                                modal,work_area.height(),theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT,false
                            );
                            egui::ScrollArea::vertical()
                                .id_salt("alias-product-results")
                                .max_height(results_height)
                                .show(modal,|results|{
                                    match &found{
                                        Some(result)=>{
                                            let data=&result.data;
                                            let detail=rows(&data["detailLinkIds"]).iter()
                                                .filter_map(Value::as_str).collect::<HashSet<_>>();
                                            let ids=rows(&data["ids"]).iter()
                                                .filter_map(Value::as_str).take(100)
                                                .collect::<Vec<_>>();
                                            if ids.is_empty(){
                                                results.label(typography::body("검색 결과가 없습니다."));
                                            }
                                            for id in ids{
                                                if let Some(p)=product(catalog,id){
                                                    results.horizontal(|row|{
                                                        if theme::action(row,
                                                            &display_model(field(p,"name")),
                                                            theme::ButtonRole::Standard,editable&&!busy)
                                                            .clicked(){chosen=Some(id.to_owned());}
                                                        let description=if field(p,"subtitle").is_empty(){
                                                            field(p,"specification")
                                                        }else{field(p,"subtitle")};
                                                        row.label(typography::caption(description));
                                                        if detail.contains(id){
                                                            if let Some(url)=rows(&p["variants"]).first()
                                                                .and_then(|v|v["url"].as_str()){
                                                                if theme::action(row,"상세보기 ↗",
                                                                    theme::ButtonRole::Quiet,true).clicked(){
                                                                    ctx.open_url(egui::OpenUrl::new_tab(url));
                                                                }
                                                            }
                                                        }
                                                    });
                                                    components::soft_divider(results);
                                                }
                                            }
                                        }
                                        None=>{}
                                    }
                                });
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
            components::modal_backdrop(ctx,"alias-unsaved-dim",true);
            components::show_decision_window(ctx,"상품 별칭","alias-unsaved-confirm",work_area,|dialog|{
                components::decision_workspace(dialog,|decision|{
                    cancel=decision.header("상품 별칭","취소",!busy
                    );
                    decision.body(|dialog|{
                        dialog.label(typography::body("상품 별칭 변경사항을 저장할까요?"));
                    });
                    decision.actions(|actions|{
                            save_now=theme::action(actions,"저장",
                                theme::ButtonRole::Primary,editable&&!busy).clicked();
                            discard=theme::action(actions,"저장 안 함",
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
    fn diagnostic_slot_keeps_next_alias_row_fixed_when_long_warnings_arrive(){
        fn render(warnings:Warnings)->(Rect,f32){
            let ctx=Context::default();
            crate::typography::install(&ctx);
            let mut measured=None;
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(Rect::from_min_size(
                egui::Pos2::ZERO,egui::vec2(720.0,360.0)
            ));
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(620.0);
                    let mut slots=Vec::new();
                    let mut second_row_top=0.0;
                    for index in 0..2{
                        if index==1{second_row_top=ui.cursor().top();}
                        let item=json!({
                            "id":format!("alias-{index}"),
                            "text":format!("별칭 {index}"),
                            "clientId":""
                        });
                        ui.horizontal(|row|{
                            row.vertical(|info|{
                                info.label(typography::body(field(&item,"text")));
                                info.label(typography::caption("→ 709 / 회사 공통"));
                                slots.push(alias_diagnostic_slot(info,&item,&warnings));
                            });
                            row.with_layout(Layout::right_to_left(Align::Center),|actions|{
                                theme::action(
                                    actions,"연결 정정",theme::ButtonRole::Standard,true
                                );
                            });
                        });
                        components::soft_divider(ui);
                    }
                    measured=Some((slots,second_row_top));
                });
            });
            let (slots,second_top)=measured.expect("alias rows were rendered");
            (slots[0],second_top)
        }
        let plain=render(Warnings::new());
        let mut warnings=Warnings::new();
        warnings.insert("alias-0".into(),vec![
            format!("중복 후보 / {}", "매우 긴 충돌 설명 ".repeat(40))
        ]);
        let diagnosed=render(warnings);
        let source=include_str!("alias_manager.rs");
        let page=source.split("pub fn show_page(").nth(1).unwrap()
            .split("pub fn show_dialogs(").next().unwrap();
        assert!(page.contains("alias_diagnostic_slot(info,item,&warnings);"));
        let near=|a:f32,b:f32|(a-b).abs()<=0.5;
        assert!(near(plain.0.top(),diagnosed.0.top()));
        assert!(near(plain.0.width(),diagnosed.0.width()));
        assert!(near(plain.0.height(),diagnosed.0.height()));
        assert!(near(plain.1,diagnosed.1),
            "diagnostic text moved the next alias row: plain={}, diagnosed={}",
            plain.1,diagnosed.1);
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
