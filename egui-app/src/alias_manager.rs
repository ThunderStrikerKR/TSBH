//! React AliasManager.tsx and ProductSearchDialog.tsx replacement.
//! Alias collision, product matching and persistence decisions remain in
//! original catalog_clients Rust. This UI owns only explicit operator edits.
use crate::{catalog_query_bridge,product_manager::ProductManager,gateway::BandoGateway,theme,typography};
use eframe::egui::{Align,Context,Layout,Rect,Sense,Ui};
#[cfg(test)]
use eframe::egui;
use serde_json::{json,Value};
use std::collections::HashMap;
use std::sync::Arc;
use crate::screen_refresh::ReadRefresh;

type Warnings=HashMap<String,Vec<String>>;
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
    expected_entity:Value,
    editor_open:bool,
    list_query:String,
    product_search:bool,
    product_picker:Option<Box<ProductManager>>,
    pending:Option<Exit>,
    confirm_delete:bool,
    after_save:Option<Exit>,
    next_tab:Option<usize>,
    global_approved:bool,
    error:Option<String>,
    catalog_snapshot:Option<Arc<Value>>,
    diagnostics:ReadRefresh<Warnings>,
    warnings:Arc<Warnings>,

}
pub(crate) struct AliasWrite{pub action:&'static str,pub data:Value}
impl AliasManager {
    /// Called only when ProductManager accepts a catalog snapshot, never on paint.
    pub fn set_catalog(&mut self,catalog:&Value,ctx:&Context){
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
    fn dirty(&self)->bool{self.form!=self.original}
    pub fn has_overlay(&self)->bool{self.editor_open||self.product_search||self.pending.is_some()||self.confirm_delete}
    pub fn global_leave_in_progress(&self)->bool{
        self.pending==Some(Exit::Global)||self.after_save==Some(Exit::Global)
    }
    pub fn take_global_approval(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn take_tab(&mut self)->Option<usize>{self.next_tab.take()}
    pub fn reset_navigation(&mut self){
        self.editor_open=false;self.list_query.clear();
        self.product_picker=None;
        self.product_search=false;
        self.pending=None;
        self.confirm_delete=false;
        self.after_save=None;
        self.form=Form::default();
        self.original=self.form.clone();
        self.error=None;
    }
    pub fn request_tab_leave(&mut self,tab:usize)->bool{
        if self.product_search||self.confirm_delete{return false;}
        if self.dirty(){self.pending=Some(Exit::Tab(tab));false}
        else{true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.product_search||self.confirm_delete{return false;}
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
                    self.expected_entity=row.clone();
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
        self.editor_open=false;self.product_search=false;self.product_picker=None;
        self.confirm_delete=false;
        let scope=self.original.scope.clone();
        self.form=Form{scope,..Form::default()};self.original=self.form.clone();self.error=None;
    }
    fn save_request(&self)->Value{
        json!({"expectedEntity":if self.form.editing.is_some(){self.expected_entity.clone()}else{Value::Null},"record":{
            "id":self.form.editing.as_deref().unwrap_or(""),
            "text":self.form.text,
            "productId":self.form.target,
            "clientId":if self.form.scope=="common"{
                Value::Null
            }else{json!(self.form.scope)}
        }})
    }
    fn delete_request(&self)->Option<AliasWrite>{
        let id=self.form.editing.as_deref()?;
        Some(AliasWrite{action:"delete_alias",data:json!({
            "aliasId":id,"expectedEntity":self.expected_entity
        })})
    }
    pub fn on_saved(&mut self,catalog:&Value){
        self.editor_open=false;self.product_search=false;self.product_picker=None;
        self.confirm_delete=false;
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
                ["별칭","적용 범위","연결 상품","비고"],
                crate::table::RegistryColumns::from([150.0,160.0,150.0,220.0+120.0+theme::SPACE_8])
                    .redistribute_with_minimum([0.5,1.7,2.0,1.0],3,48.0),
                height,aliases.len(),
                |i|self.form.editing.as_deref()==Some(field(&aliases[i],"id")),
                |row,cells,i,rect|{
                    let item=&aliases[i];let id=field(item,"id");
                    let enabled=!busy&&!self.editor_open;
                    let selected=self.form.editing.as_deref()==Some(id);
                    let hit=row.interact(rect,row.make_persistent_id(id),if enabled{Sense::click()}else{Sense::hover()});
                    if hit.hovered()&&!selected&&enabled{
                        crate::table::paint_management_interactive_row_background(row.painter(),rect,i,true);
                    }
                    let target=products.get(field(item,"productId"))
                        .map(|p|display_model(field(p,"name"))).unwrap_or_else(||"과거 상품".into());
                    let scope=if field(item,"clientId").is_empty(){"회사 공통"}
                        else{clients.get(field(item,"clientId")).map(|c|field(c,"name")).unwrap_or("거래처 확인 필요")};
                    let diagnostic=alias_diagnostic(item,&warnings);
                    crate::table::registry_cell(row,cells[0],field(item,"text"),Role::Text,false,(id,0));
                    crate::table::registry_cell(row,cells[1],scope,Role::Text,false,(id,1));
                    crate::table::registry_cell(row,cells[2],&target,Role::ProductName,false,(id,2));
                    crate::table::registry_cell(row,cells[3],&diagnostic,Role::Status,false,(id,3));
                    if hit.clicked()&&enabled{choose=Some(id.to_owned());}
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
        let title=if self.form.editing.is_some(){"별칭 편집"}else{"별칭 추가"};
        let mut close=false;let mut save=false;
        let nested=self.pending.is_some()||self.confirm_delete;
        crate::popup::modal_backdrop(ctx,"alias-editor-dim",false);
        crate::popup::modal_window(ctx,title,"alias-editor",work_area)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|modal|{
                crate::popup::modal_flow_workspace(modal,|workspace|{
                    close=workspace.header(title,"닫기",!busy&&!nested);
                    workspace.scroll_region("alias-editor-body",crate::popup::modal_editor_body_max_height(work_area.height()),|body|{
                        if self.form.editing.is_some(){
                            body.horizontal(|toolbar|toolbar.with_layout(Layout::right_to_left(Align::Center),|actions|{
                                if crate::button::action(actions,"별칭 삭제",theme::ButtonRole::Danger,
                                    editable&&!busy&&!nested).clicked(){self.confirm_delete=true;}
                            }));
                        }
                        body.add_enabled_ui(editable&&!busy&&!nested&&!self.confirm_delete,|body|{
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
                                self.product_search=true;
                            }
                        });
                        if let Some(error)=self.error.as_deref(){body.label(typography::caption(error));}
                    });
                    workspace.footer_single(|actions|{
                        save=crate::button::action(actions,"별칭 저장",theme::ButtonRole::Primary,
                            editable&&!busy&&!nested&&!self.confirm_delete&&self.dirty()).clicked();
                    });
                });
            });
        if close{if self.dirty(){self.pending=Some(Exit::Close);}else{self.close_editor();}}
        save.then(||self.save_request())
    }
    pub fn show_dialogs(&mut self,ctx:&Context,work_area:Rect,catalog:&Value,
        editable:bool,busy:bool,gateway:Arc<dyn BandoGateway>)->Option<AliasWrite>{
        let mut save=self.show_editor(ctx,work_area,catalog,editable,busy);
        if self.confirm_delete{
            crate::popup::modal_backdrop(ctx,"alias-delete-dim",true);
            let decision=crate::popup::show_delete_confirmation(
                ctx,work_area,"alias-delete-confirm","별칭 삭제","삭제",
                !busy,editable&&!busy&&self.form.editing.is_some(),crate::popup::DeleteBody::Plain,|body|{
                    body.label(typography::body("별칭을 삭제할까요? 기존 상품과 주문은 유지됩니다."));
                    if self.dirty(){body.label(typography::caption("저장하지 않은 별칭 변경사항도 사라집니다."));}
                }
            );
            if decision==crate::popup::ConfirmDecision::Cancel{self.confirm_delete=false;}
            if decision==crate::popup::ConfirmDecision::Confirm{
                self.confirm_delete=false;
                return self.delete_request();
            }
            return None;
        }
        if self.product_search{
            if !editable||busy{
                self.product_search=false;
                self.product_picker=None;
            }else{
                let picker=self.product_picker.get_or_insert_with(||Box::new(
                    ProductManager::new_reference_selection(
                        Arc::clone(&gateway),ctx.clone(),catalog.clone(),None,Default::default(),Default::default()
                    )
                ));
                crate::popup::modal_backdrop(ctx,"alias-product-search-dim",false);
                let (cancelled,chosen)=picker.show_selection_modal(ctx,work_area,"alias-product-search");
                if let Some(id)=chosen.as_ref().and_then(|value|value["productId"].as_str()){
                    if product(catalog,id).is_some(){self.form.target=id.to_owned();}
                }
                if cancelled||chosen.is_some(){self.product_search=false;self.product_picker=None;}
            }
        }
        if let Some(exit)=self.pending.clone(){
            crate::popup::modal_backdrop(ctx,"alias-unsaved-dim",true);
            let decision=crate::popup::show_save_confirmation(
                ctx,work_area,"alias-unsaved-confirm","상품 별칭",
                "상품 별칭 변경사항을 저장할까요?","저장",!busy,editable&&!busy
            );
            let cancel=decision==crate::popup::ChangeDecision::Cancel;
            let discard=decision==crate::popup::ChangeDecision::Discard;
            let save_now=decision==crate::popup::ChangeDecision::Save;
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
        save.map(|data|AliasWrite{action:"save_alias",data})
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
    fn notes_show_only_existing_alias_diagnostics(){
        let item=json!({"id":"a","clientId":null});
        let mut warnings=Warnings::new();
        assert!(alias_diagnostic(&item,&warnings).is_empty());
        warnings.insert("a".into(),vec![]);
        assert!(alias_diagnostic(&item,&warnings).is_empty());
        warnings.insert("a".into(),vec!["같은 적용 범위의 별칭이 충돌합니다.".into()]);
        assert_eq!(alias_diagnostic(&item,&warnings),
            "충돌: 같은 적용 범위의 별칭이 충돌합니다. / 영향 범위: 모든 거래처의 이후 주문");
        assert!(alias_diagnostic(&json!({"id":"b"}),&warnings).is_empty());
        let source=include_str!("alias_manager.rs");
        let list=source.split("pub fn show_page(").nth(1).unwrap()
            .split("fn show_editor(").next().unwrap();
        assert!(list.contains("[\"별칭\",\"적용 범위\",\"연결 상품\",\"비고\"]"));
        assert!(list.contains("registry_cell(row,cells[3],&diagnostic,Role::Status,false,(id,3))"));
        assert!(!list.contains("cells[4]"));
    }
    #[test]
    fn notes_column_keeps_next_alias_row_fixed_when_long_warnings_arrive(){
        fn render(warnings:Warnings)->Vec<Rect>{
            let ctx=Context::default();
            crate::typography::install(&ctx);
            let mut measured=Vec::new();
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1000.0,400.0)));
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    crate::table::registry_table(ui,"test-alias-diagnostic",
                        ["별칭","비고"],[120.0,240.0],180.0,2,|_|false,|row,cells,index,rect|{
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


}

#[cfg(test)]
mod modal_lifecycle_tests{
    use super::*;
    #[test]
    fn delete_request_uses_original_entity_and_requires_existing_alias(){
        let catalog=json!({"aliases":[{"id":"a","text":"709","productId":"p","clientId":"c"}]});
        let mut manager=AliasManager::default();
        assert!(manager.delete_request().is_none());
        manager.choose("a".into(),&catalog);
        manager.form.text="unsaved".into();
        let request=manager.delete_request().unwrap();
        assert_eq!(request.action,"delete_alias");
        assert_eq!(request.data["aliasId"],"a");
        assert_eq!(request.data["expectedEntity"],catalog["aliases"][0]);
    }
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
