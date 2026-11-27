//! Manually configured model families.
//! Saved data stays in settings.colorGuidanceGroups, never product/order rows.
//! Rust catalog_clients::model_groups validates; Tauri settings_patch CAS saves.
use crate::{catalog_query_bridge,gateway::BandoGateway, theme, typography};
use crate::screen_refresh::ReadRefresh;
use eframe::egui::{self, Align, Context, Layout, Rect, Sense, Ui};
use serde_json::{json, Value};
use std::{
    collections::{HashMap,HashSet},
    sync::{Arc,atomic::{AtomicU64,Ordering},mpsc::{self,Receiver,TryRecvError}},
};
#[cfg(test)]
use std::time::Duration;
static NEXT_ID:AtomicU64=AtomicU64::new(0);
fn text<'a>(v:&'a Value,k:&str)->&'a str{v.get(k).and_then(Value::as_str).unwrap_or("")}
fn items(v:&Value)->&[Value]{v.as_array().map(Vec::as_slice).unwrap_or(&[])}
fn original_groups(catalog:&Value)->Value{
    catalog["settings"].get("colorGuidanceGroups")
        .cloned().unwrap_or(Value::Null)
}
#[derive(Clone,PartialEq,Eq)]
enum Exit {Select(String),Close,Tab(usize),Global}
pub struct ModelGroupManager {
    initialized:bool,
    baseline:Value,
    draft:Value,
    groups_cache:crate::screen_refresh::ViewProjectionCache<(),Vec<(String,Value)>>,
    selected:String,
    editor_open:bool,
    new_name:String,
    query:String,
    product_picker:Option<Box<crate::product_manager::ProductManager>>,
    status:String,
    write:Option<Receiver<Result<Value,String>>>,
    confirm:Option<Exit>,
    after_save:Option<Exit>,
    confirm_delete:bool,
    leave_tab:Option<usize>,
    global_approved:bool,
    eligibility_snapshot:Option<Arc<Value>>,
    eligibility:ReadRefresh<HashSet<String>>,
    eligible:Arc<HashSet<String>>,
}
impl Default for ModelGroupManager{
    fn default()->Self{
        Self{initialized:false,baseline:Value::Null,draft:json!({}),groups_cache:Default::default(),
            selected:String::new(),editor_open:false,new_name:String::new(),
            query:String::new(),product_picker:None,status:String::new(),
            write:None,confirm:None,after_save:None,confirm_delete:false,
            leave_tab:None,global_approved:false,
            eligibility_snapshot:None,eligibility:ReadRefresh::default(),eligible:Arc::default()}
    }
}
impl ModelGroupManager {
    pub fn set_catalog(&mut self,catalog:&Value,ctx:&Context){
        let snapshot=json!({"products":catalog["products"]});
        if self.eligibility_snapshot.as_deref()==Some(&snapshot){return;}
        let snapshot=Arc::new(snapshot);
        self.eligibility_snapshot=Some(Arc::clone(&snapshot));
        self.eligible=Arc::default();
        self.eligibility.submit(ctx.clone(),move||{
            let ids=catalog_query_bridge::eligible_model_group_products(&snapshot)?;
            Ok(items(&ids).iter().filter_map(Value::as_str).map(str::to_owned).collect())
        });
    }
    pub fn sync(&mut self,catalog:&Value){
        let latest=catalog["settings"].get("colorGuidanceGroups").unwrap_or(&Value::Null);
        if !self.initialized || (!self.dirty()&&self.new_name.trim().is_empty()
            &&self.write.is_none()&& &self.baseline!=latest){
            self.initialized=true;
            self.baseline=latest.clone();
            self.draft=if latest.is_object(){latest.clone()}else{json!({})};
            self.groups_cache.invalidate();
            if self.draft.get(&self.selected).is_none(){self.selected.clear();}
        }
    }
    pub fn dirty(&self)->bool{
        if self.baseline.is_object(){self.draft!=self.baseline}
        else{self.draft.as_object().is_none_or(|groups|!groups.is_empty())}
    }
    fn unsaved(&self)->bool{self.dirty()||!self.new_name.trim().is_empty()}
    pub fn busy(&self)->bool{self.write.is_some()}
    pub fn has_overlay(&self)->bool{self.editor_open||self.confirm_delete||self.confirm.is_some()}
    pub fn request_tab_leave(&mut self,index:usize)->bool{
        if self.busy()||self.product_picker.is_some(){return false;}
        if self.unsaved(){self.confirm=Some(Exit::Tab(index));false}
        else{true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.busy()||self.product_picker.is_some(){return false;}
        if self.unsaved(){self.confirm=Some(Exit::Global);false}
        else{true}
    }
    pub fn global_leave_in_progress(&self)->bool{
        self.confirm==Some(Exit::Global)||self.after_save==Some(Exit::Global)
    }
    pub fn take_global_approval(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn take_tab_change(&mut self)->Option<usize>{self.leave_tab.take()}
    pub fn reset_navigation(&mut self){
        self.editor_open=false;self.selected.clear();self.product_picker=None;self.query.clear();
        self.new_name.clear();self.status.clear();
        self.confirm=None;self.after_save=None;self.confirm_delete=false;
    }
    fn discard(&mut self){
        self.draft=if self.baseline.is_object(){self.baseline.clone()}else{json!({})};self.groups_cache.invalidate();
        self.new_name.clear();self.status.clear();self.selected.clear();
        self.product_picker=None;
    }
    fn finish_exit(&mut self,next:Exit){
        match next{
            Exit::Select(id)=>{self.editor_open=true;self.selected=id;self.product_picker=None;self.status.clear();}
            Exit::Close=>{self.editor_open=false;self.selected.clear();self.product_picker=None;self.status.clear();}
            Exit::Tab(index)=>{self.editor_open=false;self.selected.clear();self.leave_tab=Some(index);}
            Exit::Global=>{self.editor_open=false;self.selected.clear();self.global_approved=true;}
        }
    }
    fn request_exit(&mut self,next:Exit){
        if self.unsaved(){self.confirm=Some(next)}
        else{self.finish_exit(next)}
    }
    fn create(&mut self){
        let name=self.new_name.trim();
        if name.is_empty(){self.status="그룹 이름을 입력하세요.".into();return;}
        if self.draft.as_object().is_some_and(|groups|groups.values().any(|g|
            text(g,"name").trim().to_lowercase()==name.to_lowercase())){
            self.status="이미 같은 이름의 그룹이 있습니다.".into();return;
        }
        let id=format!("egui-model-{}-{}-{}",std::process::id(),
            chrono::Utc::now().timestamp_micros(),
            NEXT_ID.fetch_add(1,Ordering::Relaxed));
        self.draft[&id]=json!({"name":name,"productIds":[],"representativeProductId":""});self.groups_cache.invalidate();
        self.editor_open=true;self.selected=id;self.new_name.clear();self.status.clear();
    }
    pub fn poll(&mut self,_ctx:&Context)->Option<Value>{
        let response=self.write.as_ref().map(|rx|rx.try_recv());
        match response{
            Some(Ok(Ok(catalog)))=>{
                self.write=None;
                self.baseline=original_groups(&catalog);
                self.draft=if self.baseline.is_object(){
                    self.baseline.clone()
                }else{json!({})};
                self.groups_cache.invalidate();
                self.new_name.clear();
                if let Some(exit)=self.after_save.take(){self.finish_exit(exit);}
                self.status="모델군 설정을 저장했습니다. 기존 상품과 주문은 변경하지 않았습니다.".into();
                Some(catalog)
            }
            Some(Ok(Err(e)))=>{
                self.write=None;self.after_save=None;self.status=e;None
            }
            Some(Err(TryRecvError::Disconnected))=>{
                self.write=None;self.after_save=None;
                self.status="모델군 저장 응답을 확인할 수 없습니다. 재조회가 필요합니다.".into();None
            }
            // The write worker calls ctx.request_repaint() immediately after
            // sending its result. No 100ms polling frames are required.
            Some(Err(TryRecvError::Empty))=>None,
            None=>None,
        }
    }
    fn save(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        if self.busy()||!self.dirty(){return;}
        crate::floating_notice::dispatch_pointer(&ctx,"products.model");
        if !self.new_name.trim().is_empty(){
            self.status="새 그룹 이름은 먼저 그룹 생성으로 등록하세요.".into();return;
        }
        if !gateway.model_group_write_enabled(){
            self.status="모델군 변경 저장 권한이 없습니다.".into();return;
        }
        let groups=self.draft.clone();
        let expected=self.baseline.clone();
        let (tx,rx)=mpsc::channel();
        self.write=Some(rx);self.status.clear();
        std::thread::spawn(move||{
            let value=gateway.patch_model_groups(groups,expected);
            let _=tx.send(value);ctx.request_repaint();
        });
    }
    fn membership(&self)->HashMap<String,String>{
        let mut assigned=HashMap::new();
        if let Some(groups)=self.draft.as_object(){
            for group in groups.values(){
                for id in items(&group["productIds"]).iter().filter_map(Value::as_str){
                    assigned.insert(id.to_owned(),text(group,"name").to_owned());
                }
            }
        }
        assigned
    }
    pub fn show_page(&mut self,ui:&mut Ui,catalog:&Value,
        gateway:Arc<dyn BandoGateway>){
        self.sync(catalog);
        let writable=gateway.model_group_write_enabled();
        let busy=self.busy();
        if let Some(result)=self.eligibility.poll(){
            match result{
                Ok(ids)=>self.eligible=Arc::new(ids),
                Err(error)=>self.status=error,
            }
        }
        let groups=self.groups_cache.get((),||self.draft.as_object().map(|m|
            m.iter().map(|(id,g)|(id.clone(),g.clone())).collect::<Vec<_>>()).unwrap_or_default());
        let products:HashMap<&str,&Value>=items(&catalog["products"]).iter().rev().map(|p|(text(p,"id"),p)).collect();
        use typography::ManagementTableCellRole as Role;
        let card_min_height=crate::card::management_master_table_card_min_inner_height(ui);
        crate::card::measured_management_master_card(ui,|panel|{
            crate::card::fill_management_primary_card(panel,card_min_height);
            panel.horizontal(|toolbar|{
                let width=theme::CATALOG_SEARCH_WIDTH.min((toolbar.available_width()-160.0).max(1.0));
                crate::input::management_search_field(toolbar,&mut self.query,width,"모델군 이름 또는 상품명 검색");
                toolbar.with_layout(Layout::right_to_left(Align::Center),|actions|{
                    if crate::button::action(actions,"+ 모델군 추가",theme::ButtonRole::Primary,
                        writable&&!busy&&!self.editor_open).clicked(){
                        self.editor_open=true;self.selected.clear();self.new_name.clear();
                        self.product_picker=None;self.status.clear();
                    }
                });
            });
            crate::ui_primitives::section_divider(panel);
                    let query=self.query.trim().to_lowercase();
                    let visible=groups.iter().filter(|(_,g)|{
                        query.is_empty()||text(g,"name").to_lowercase().contains(&query)
                        ||items(&g["productIds"]).iter().filter_map(Value::as_str)
                            .filter_map(|id|products.get(id))
                            .any(|p|text(p,"name").to_lowercase().contains(&query))
                    }).collect::<Vec<_>>();
                    let body_height=(panel.available_height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0);
                    let selected_id=self.selected.clone();
                    let rect=crate::table::registry_table(panel,"family-list",["모델군","대표모델","상품 수","비고"],
                        crate::table::RegistryColumns::balanced_text([240.0,220.0,96.0,108.0],2)
                            .redistribute_to([0.7,0.8,1.0,1.0],3),
                        body_height,visible.len(),
                        |i|visible[i].0==selected_id,|row,cells,i,rect|{
                            let (id,g)=visible[i];
                            let selected=self.selected==*id;
                            let response=row.interact(rect,row.make_persistent_id(id),if busy||self.editor_open{Sense::hover()}else{Sense::click()});
                            if response.hovered()&&!selected&&!busy{
                                crate::table::paint_management_interactive_row_background(row.painter(),rect,i,true);
                            }
                            let rep=products.get(text(g,"representativeProductId"))
                                .map(|p|text(p,"name")).unwrap_or(if text(g,"representativeColor").is_empty(){
                                    "미지정"}else{"재지정 필요"});
                            crate::table::registry_cell(row,cells[0],text(g,"name"),Role::Text,false,(id,0));
                            crate::table::registry_cell(row,cells[1],rep,Role::ProductName,false,(id,1));
                            crate::table::registry_cell(row,cells[2],&items(&g["productIds"]).len().to_string(),Role::Quantity,true,(id,2));
                            #[cfg(test)]
                            crate::ui_test_support::record("catalog data row",rect);
                            crate::table::registry_cell(row,cells[3],"",Role::Text,false,(id,3));
                            if response.clicked()&&!busy&&!self.editor_open{self.request_exit(Exit::Select(id.clone()));}
                        });
                    if visible.is_empty(){
                        crate::status_notice::management_body_state_overlay(panel,rect,Some(
                            if groups.is_empty(){"등록된 모델군이 없습니다."}else{"검색 결과가 없습니다."}));
                    }
        });
        let input_error=self.status.contains("그룹 이름") ||
            self.status.contains("같은 이름");
        crate::floating_notice::sync(
            ui.ctx(),"products.model.status",
            if input_error{"products.model.input"}else{"products.model"},
            if self.status.contains("저장했습니다"){
                crate::floating_notice::Tone::Success
            }else if input_error{
                crate::floating_notice::Tone::Validation
            }else{
                crate::floating_notice::Tone::Error
            },
            if self.status.is_empty(){None}else{Some(self.status.as_str())}
        );
    }

    fn show_editor(&mut self,ctx:&Context,rect:Rect,catalog:&Value,gateway:Arc<dyn BandoGateway>){
        if !self.editor_open{return;}
        let writable=gateway.model_group_write_enabled();
        let busy=self.busy();
        let nested=self.confirm_delete||self.confirm.is_some()||self.product_picker.is_some();
        let eligibility_pending=self.eligibility.pending();
        let products:HashMap<&str,&Value>=items(&catalog["products"]).iter().rev().map(|p|(text(p,"id"),p)).collect();
        use typography::ManagementTableCellRole as Role;
        let mut save=false;
        let mut close=false;
        let title=if self.selected.is_empty()&&!self.dirty(){"모델군 추가"}else{"모델군 편집"};
        crate::popup::modal_backdrop(ctx,"model-group-editor-dim",false);
        crate::popup::modal_window(ctx,title,"model-group-editor",rect)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(rect.width()))
            .show(ctx,|modal|{
                let body_height=crate::popup::modal_editor_body_max_height(rect.height());
                crate::popup::modal_flow_workspace(modal,|workspace|{
                    close=workspace.header(title,"닫기",!busy&&!nested);
                    workspace.body_region(|body|{
                        body.add_enabled_ui(!nested,|body|{
                            let body_top=body.cursor().top();
                            if let Some(g)=self.draft.get(&self.selected).cloned(){
                                body.horizontal(|toolbar|toolbar.with_layout(Layout::right_to_left(Align::Center),|actions|{
                                    if crate::button::action(actions,"그룹 삭제",theme::ButtonRole::Danger,writable&&!busy).clicked(){
                                        self.confirm_delete=true;
                                    }
                                }));
                        let mut name=text(&g,"name").to_owned();
                        body.horizontal(|form|{
                            crate::ui_primitives::form_field_label(form,"모델군 이름");
                            let width=form.available_width();
                            form.add_enabled_ui(writable&&!busy,|field|{
                                crate::input::flexible_text_field(field,&mut name,width,"모델군 이름");
                            });
                        });
                        if name!=text(&g,"name"){
                            self.draft[&self.selected]["name"]=json!(name);
                            self.groups_cache.invalidate();self.status.clear();
                        }
                        if crate::button::action(body,"상품 추가",theme::ButtonRole::Standard,
                            writable&&!busy&&!eligibility_pending).clicked(){
                            let excluded=items(&g["productIds"]).iter().filter_map(Value::as_str)
                                .map(str::to_owned).collect();
                            self.product_picker=Some(Box::new(
                                crate::product_manager::ProductManager::new_reference_selection(
                                    Arc::clone(&gateway),ctx.clone(),catalog.clone(),
                                    Some((*self.eligible).clone()),self.membership(),excluded
                                )
                            ));
                        }
                        crate::ui_primitives::section_divider(body);
                        let ids=items(&g["productIds"]).iter().filter_map(Value::as_str).collect::<Vec<_>>();
                        body.horizontal(|heading|{
                            heading.label(typography::subsection_heading("등록 상품"));
                            heading.label(typography::caption(&format!("{}개",ids.len())));
                        });
                        if !text(&g,"representativeColor").is_empty()&&text(&g,"representativeProductId").is_empty(){
                            body.label(typography::caption(&format!("이전 대표색상: {} · 대표모델 재지정 필요",text(&g,"representativeColor"))));
                        }
                        let body_height=(body_height-(body.cursor().top()-body_top)-theme::MANAGEMENT_TABLE_HEADER_HEIGHT-theme::SPACE_8).max(theme::MANAGEMENT_COMPACT_ROW_HEIGHT);
                        let rect=crate::table::registry_table(body,("model-group-registered-products",self.selected.clone()),
                            ["상품명","상세 정보","대표모델","관리"],[140.0,100.0,96.0,176.0],
                            body_height,ids.len(),|_|false,|row,cells,i,_|{
                                let id=ids[i];
                                let p=products.get(id).copied();
                                let rep=text(&g,"representativeProductId")==id;
                                crate::table::registry_cell(row,cells[0],p.map(|p|text(p,"name")).unwrap_or("확인할 상품"),Role::ProductName,false,(id,0));
                                crate::table::registry_cell(row,cells[1],p.map(|p|text(p,"subtitle")).unwrap_or(""),Role::Description,false,(id,1));
                                crate::table::registry_cell(row,cells[2],if rep{"대표모델"}else{"—"},Role::Status,true,(id,2));
                                crate::table::registry_actions(row,cells[3],|actions|{
                                    if crate::button::action(actions,"제외",theme::ButtonRole::Standard,writable&&!busy).clicked(){
                                        let values=items(&self.draft[&self.selected]["productIds"]).iter()
                                            .filter(|v|v.as_str()!=Some(id)).cloned().collect::<Vec<_>>();
                                        self.draft[&self.selected]["productIds"]=json!(values);
                                        self.groups_cache.invalidate();
                                        if text(&self.draft[&self.selected],"representativeProductId")==id{
                                            self.draft[&self.selected]["representativeProductId"]=json!("");
                                        }
                                        self.status.clear();
                                    }
                                    if crate::button::action_with_min_width(actions,"대표 지정",theme::ButtonRole::Standard,
                                        !rep&&writable&&!busy&&p.is_some(),100.0).clicked(){
                                        self.draft[&self.selected]["representativeProductId"]=json!(id);
                                        self.groups_cache.invalidate();
                                        if let Some(group)=self.draft.get_mut(&self.selected).and_then(Value::as_object_mut){
                                            group.remove("representativeColor");
                                        }
                                        self.status.clear();
                                    }
                                });
                            });
                        if ids.is_empty(){
                            crate::status_notice::management_body_state_overlay(body,rect,Some("상품을 검색해 추가하세요."));
                        }
                            }else if self.dirty(){
                                body.label(typography::body("모델군이 삭제되었습니다. 변경사항을 저장하세요."));
                            }else{
                                body.horizontal(|form|{
                                    let width=(form.available_width()-144.0).max(1.0);
                                    form.add_enabled_ui(writable&&!busy,|entry|{
                                        let edit=crate::input::flexible_text_field(entry,&mut self.new_name,width,"새 모델군 이름");
                                        crate::floating_notice::sized_text_caret(entry,"products.model.input",&edit,
                                            &self.new_name,typography::body_font(),false,8.0);
                                    });
                                    if crate::button::action(form,"그룹 생성",theme::ButtonRole::Primary,writable&&!busy).clicked(){
                                        crate::floating_notice::dispatch_pointer(form.ctx(),"products.model");
                                        self.create();
                                    }
                                });
                            }
                        });
                    });
                    workspace.footer_single(|buttons|{
                        buttons.add_enabled_ui(!nested,|buttons|{
                            if crate::button::action(buttons,"변경사항 저장",theme::ButtonRole::Primary,
                                writable&&!busy&&self.dirty()).clicked(){save=true;}
                        });
                    });
                });
            });
        if close{self.request_exit(Exit::Close);}
        if save{self.after_save=Some(Exit::Close);self.save(gateway,ctx.clone());}
    }
    pub fn show_dialogs(&mut self,ctx:&Context,rect:Rect,
        catalog:&Value,gateway:Arc<dyn BandoGateway>){
        self.show_editor(ctx,rect,catalog,Arc::clone(&gateway));
        if let Some(picker)=self.product_picker.as_mut(){
            crate::popup::modal_backdrop(ctx,"model-group-product-picker-dim",true);
            let (cancelled,chosen)=picker.show_selection_modal(ctx,rect,"model-group-product-picker");
            if let Some(id)=chosen.as_ref().and_then(|value|value["productId"].as_str()){
                if gateway.model_group_write_enabled()&&!self.busy()
                    && self.eligible.contains(id)&&!self.membership().contains_key(id){
                    if let Some(ids)=self.draft[&self.selected]["productIds"].as_array_mut(){
                        ids.push(json!(id));self.groups_cache.invalidate();self.status.clear();
                    }
                }
            }
            if cancelled||chosen.is_some(){self.product_picker=None;}
        }
        if self.confirm_delete{
            crate::popup::modal_backdrop(ctx,"family-delete-dim",false);
            let decision=crate::popup::show_delete_confirmation(
                ctx,rect,"family-delete-confirm","그룹 삭제","삭제",
                true,true,crate::popup::DeleteBody::Plain,|dialog|{
                    dialog.label(typography::body("이 그룹을 삭제할까요? 기존 주문과 상품은 유지됩니다."));
                }
            );
            let cancel=decision==crate::popup::ConfirmDecision::Cancel;
            let delete=decision==crate::popup::ConfirmDecision::Confirm;
            if cancel{self.confirm_delete=false;}
            else if delete{
                if let Some(groups)=self.draft.as_object_mut(){
                    groups.remove(&self.selected);self.groups_cache.invalidate();
                }
                self.selected.clear();self.confirm_delete=false;self.status.clear();
            }
        }
        if let Some(pending)=self.confirm.clone(){
            crate::popup::modal_backdrop(ctx,"family-unsaved-dim",true);
            let decision=crate::popup::show_save_confirmation(
                ctx,rect,"family-unsaved-confirm","모델군 관리",
                "모델군 관리의 변경사항을 저장할까요?","저장",!self.busy(),gateway.model_group_write_enabled()&&!self.busy()&&self.dirty()&&self.new_name.trim().is_empty()
            );
            let cancel=decision==crate::popup::ChangeDecision::Cancel;
            let discard=decision==crate::popup::ChangeDecision::Discard;
            let save=decision==crate::popup::ChangeDecision::Save;
            if cancel{self.confirm=None;self.status.clear();}
            else if discard{
                self.confirm=None;self.discard();self.finish_exit(pending);
            }else if save{
                self.confirm=None;self.after_save=Some(pending);
                self.save(gateway,ctx.clone());
            }
        }
    }
}

#[cfg(test)]
mod responsiveness_tests{
    use super::*;
    fn finish(manager:&mut ModelGroupManager){
        let deadline=std::time::Instant::now()+Duration::from_secs(5);
        while manager.eligibility.pending(){
            if let Some(result)=manager.eligibility.poll(){
                manager.eligible=Arc::new(result.unwrap());
            }
            assert!(std::time::Instant::now()<deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn model_group_tables_share_fixed_rows_and_independent_columns(){
        let source=include_str!("model_group_manager.rs");
        let body=source.split("    pub fn show_page(").nth(1).unwrap()
            .split("    pub fn show_dialogs(").next().unwrap();
        assert!(body.contains("crate::table::registry_table("));
        assert!(body.contains("crate::table::registry_cell("));
        assert!(body.contains("crate::table::registry_actions("));
        assert!(body.contains("[\"모델군\",\"대표모델\",\"상품 수\",\"비고\"]"));
        assert!(body.contains("[\"상품명\",\"상세 정보\",\"대표모델\",\"관리\"]"));
        assert!(body.contains("ProductManager::new_reference_selection("));
        assert!(!body.contains("model-group-search-results"));
        assert!(!body.contains("직원"));
        let list=body.split("    fn show_editor(").next().unwrap();
        assert!(!list.contains("allocate_ui_with_layout("));
        assert!(!list.contains("crate::table::registry_actions("));
        assert!(list.contains(".redistribute_to([0.7,0.8,1.0,1.0],3)"));
        assert!(list.contains("if response.clicked()&&!busy&&!self.editor_open"));
        assert!(list.contains("crate::table::registry_cell(row,cells[3],\"\",Role::Text"));
        assert!(body.contains("crate::popup::modal_window("));
        assert!(body.contains("crate::popup::modal_flow_workspace("));
    }
    #[test]
    fn eligibility_tracks_product_edits_and_avoids_repainting_queries(){
        let ctx=Context::default();
        let mut manager=ModelGroupManager::default();
        let mut catalog=json!({"products":[{"id":"p","active":true,
            "catalog":"BANDOSTAND","variants":[{"source":"OFFICIAL"}]}]});
        manager.set_catalog(&catalog,&ctx);finish(&mut manager);
        assert!(manager.eligible.contains("p"));
        let original=Arc::clone(&manager.eligible);
        for _ in 0..100{manager.set_catalog(&catalog,&ctx);}
        assert!(!manager.eligibility.pending());
        assert!(Arc::ptr_eq(&original,&manager.eligible));
        catalog["products"][0]["deletedAt"]=json!("2026-11-20");
        manager.set_catalog(&catalog,&ctx);finish(&mut manager);
        assert!(!manager.eligible.contains("p"));
    }
}
