//! Manually configured model families.
//! Saved data stays in settings.colorGuidanceGroups, never product/order rows.
//! Rust catalog_clients::model_groups validates; Tauri settings_patch CAS saves.
use crate::{catalog_query_bridge,gateway::BandoGateway, theme, typography};
use crate::screen_refresh::ReadRefresh;
use eframe::egui::{self, Align, Context, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
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
fn product<'a>(catalog:&'a Value,id:&str)->Option<&'a Value>{
    items(&catalog["products"]).iter().find(|p|text(p,"id")==id)
}
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
    new_name:String,
    query:String,
    search:String,
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
            selected:String::new(),new_name:String::new(),
            query:String::new(),search:String::new(),status:String::new(),
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
    pub fn has_overlay(&self)->bool{self.confirm_delete||self.confirm.is_some()}
    pub fn request_tab_leave(&mut self,index:usize)->bool{
        if self.busy(){return false;}
        if self.unsaved(){self.confirm=Some(Exit::Tab(index));false}
        else{true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.busy(){return false;}
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
        self.selected.clear();self.search.clear();self.query.clear();
        self.new_name.clear();self.status.clear();
        self.confirm=None;self.after_save=None;self.confirm_delete=false;
    }
    fn discard(&mut self){
        self.draft=if self.baseline.is_object(){self.baseline.clone()}else{json!({})};self.groups_cache.invalidate();
        self.new_name.clear();self.status.clear();self.selected.clear();
        self.search.clear();
    }
    fn finish_exit(&mut self,next:Exit){
        match next{
            Exit::Select(id)=>{self.selected=id;self.search.clear();self.status.clear();}
            Exit::Close=>{self.selected.clear();self.search.clear();self.status.clear();}
            Exit::Tab(index)=>{self.leave_tab=Some(index);}
            Exit::Global=>{self.global_approved=true;}
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
        self.selected=id;self.new_name.clear();self.status.clear();
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
                self.status="모델군 설정을 저장했습니다. 기존 상품과 주문은 변경하지 않았습니다.".into();
                if let Some(exit)=self.after_save.take(){self.finish_exit(exit);}
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
    // Shared fixed-column table; the three model-group views differ only
    // in their column data and actions, never their header or row geometry.
    fn table<const N:usize>(
        ui:&mut Ui,id:impl std::hash::Hash,labels:[&str;N],base:[f32;N],
        height:f32,count:usize,mut draw:impl FnMut(&mut Ui,[Rect;N],usize,Rect),
    )->Rect{
        let gap=theme::SPACE_8;
        let minimum=base.iter().sum::<f32>()+N.saturating_sub(1) as f32*gap;
        let spacing=crate::table::management_table_container_style(ui);
        let inner=crate::table::show(ui,crate::table::Header::WithoutToolbar(minimum),|ui,layout|{
            let mut widths=base;
            widths[0]+=(layout.table_width-minimum).max(0.0);
            let cells=crate::table::management_table_cells(
                layout.header_content_rect.min,&widths,layout.header_content_rect.height(),gap);
            crate::table::header_columns(ui,layout.header_rect,layout.header_content_rect,&cells,gap,|ui,area|{
                for (i,cell) in cells.iter().enumerate(){
                    crate::table::heading(ui,*cell,area,cell.intersect(area),
                        if i==0{crate::table::HeadingAlign::Left(theme::SPACE_8)}
                        else{crate::table::HeadingAlign::Center{min_left:None}},
                        typography::management_table_header_galley(ui,labels[i]),None);
                }
            });
            let output=crate::table::fixed_rows(ui,crate::table::Viewport{
                width:layout.table_width,height:height.max(1.0),rail:None
            },id,theme::MANAGEMENT_TABLE_ROW_HEIGHT,count,|body,visible|{
                let origin=body.max_rect().min;
                let first=visible.start;
                for index in visible{
                    let rect=crate::table::fixed_row_rect(origin,index,first,layout.table_width,
                        theme::MANAGEMENT_TABLE_ROW_HEIGHT);
                    crate::table::fixed_row(body,index,rect,index,&widths,gap,
                        crate::table::RowTone::Alternating,|row,cells|draw(row,cells,index,rect));
                }
            });
            (output.inner_rect,output.inner_rect)
        });
        ui.spacing_mut().item_spacing.y=spacing;
        inner
    }
    fn cell(ui:&Ui,rect:Rect,value:&str,role:typography::ManagementTableCellRole,
        center:bool,id:impl std::hash::Hash){
        crate::table::paint_management_text_cell(ui,rect.shrink2(Vec2::new(theme::SPACE_8,0.0)),
            value,value,role,if center{egui::Align2::CENTER_CENTER}else{egui::Align2::LEFT_CENTER},
            id,false);
    }
    fn actions<R>(ui:&mut Ui,cell:Rect,draw:impl FnOnce(&mut Ui)->R)->R{
        ui.scope_builder(UiBuilder::new().max_rect(cell.shrink2(Vec2::new(theme::SPACE_4,0.0)))
            .layout(Layout::right_to_left(Align::Center)),|actions|{
                actions.set_clip_rect(actions.clip_rect().intersect(cell));
                draw(actions)
            }).inner
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
        let eligible=Arc::clone(&self.eligible);
        let eligibility_pending=self.eligibility.pending();
        let groups=self.groups_cache.get((),||self.draft.as_object().map(|m|
            m.iter().map(|(id,g)|(id.clone(),g.clone())).collect::<Vec<_>>())
            .unwrap_or_default());
        let products:HashMap<&str,&Value>=items(&catalog["products"]).iter().rev().map(|p|(text(p,"id"),p)).collect();
        use typography::ManagementTableCellRole as Role;
        let card_min_height=crate::card::management_master_table_card_min_inner_height(ui);
        let mut save=false;
        crate::card::measured_management_master_card(ui,|outer|{
            crate::card::fill_management_primary_card(outer,card_min_height);
            let available=outer.available_width();
            let left_width=(available*0.44).max(1.0);
            let height=outer.available_height().max(theme::MANAGEMENT_TABLE_ROW_HEIGHT*6.0);
            let right_width=(available-left_width-theme::SPACE_16).max(1.0);
            outer.spacing_mut().item_spacing.x=theme::SPACE_16;
            outer.horizontal_top(|whole|{
                whole.allocate_ui_with_layout(Vec2::new(left_width,height),Layout::top_down(Align::Min),|left|{
                    left.horizontal(|head|{
                        head.label(typography::section_heading("모델군 목록"));
                        head.label(typography::caption(&format!("{}개",groups.len())));
                    });
                    left.horizontal(|form|{
                        let width=(form.available_width()-theme::MANAGEMENT_SEARCH_MIN_WIDTH*0.5).max(1.0);
                        form.add_enabled_ui(writable&&!busy,|entry|{
                            let edit=crate::input::flexible_text_field(entry,&mut self.new_name,width,"새 모델군 이름");
                            crate::floating_notice::sized_text_caret(entry,"products.model.input",&edit,
                                &self.new_name,egui::FontId::proportional(typography::FONT_BODY),false,8.0);
                        });
                        if crate::button::action(form,"그룹 생성",theme::ButtonRole::Standard,writable&&!busy).clicked(){
                            crate::floating_notice::dispatch_pointer(form.ctx(),"products.model");
                            self.create();
                        }
                    });
                    let width=left.available_width();
                    crate::input::management_search_field(left,&mut self.query,width,"모델군 이름 또는 상품명 검색");
                    crate::ui_primitives::section_divider(left);
                    let query=self.query.trim().to_lowercase();
                    let visible=groups.iter().filter(|(_,g)|{
                        query.is_empty()||text(g,"name").to_lowercase().contains(&query)
                        ||items(&g["productIds"]).iter().filter_map(Value::as_str)
                            .filter_map(|id|products.get(id))
                            .any(|p|text(p,"name").to_lowercase().contains(&query))
                    }).collect::<Vec<_>>();
                    let body_height=(left.available_height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0);
                    let rect=Self::table(left,"family-list",["모델군","대표모델","상품 수"],
                        [160.0,156.0,76.0],body_height,visible.len(),|row,cells,i,rect|{
                            let (id,g)=visible[i];
                            let selected=self.selected==*id;
                            if selected{
                                row.painter().rect_filled(rect,0.0,theme::WORKFLOW_SELECTED_BG);
                                row.painter().rect_filled(Rect::from_min_size(rect.min,
                                    Vec2::new(theme::WORKFLOW_SELECTED_STRIPE,rect.height())),0.0,theme::SELECTION);
                            }
                            let response=row.interact(rect,row.make_persistent_id(id),if busy{Sense::hover()}else{Sense::click()});
                            if response.hovered()&&!selected&&!busy{
                                row.painter().rect_filled(rect,0.0,theme::INPUT);
                            }
                            let rep=products.get(text(g,"representativeProductId"))
                                .map(|p|text(p,"name")).unwrap_or(if text(g,"representativeColor").is_empty(){
                                    "미지정"}else{"재지정 필요"});
                            Self::cell(row,cells[0],text(g,"name"),Role::Text,false,(id,0));
                            Self::cell(row,cells[1],rep,Role::ProductName,false,(id,1));
                            Self::cell(row,cells[2],&items(&g["productIds"]).len().to_string(),Role::Quantity,true,(id,2));
                            #[cfg(test)]
                            crate::ui_test_support::record("catalog data row",rect);
                            if response.clicked()&&!busy&&!selected{self.request_exit(Exit::Select(id.clone()));}
                        });
                    if visible.is_empty(){
                        crate::status_notice::management_body_state_overlay(left,rect,Some(
                            if groups.is_empty(){"등록된 모델군이 없습니다."}else{"검색 결과가 없습니다."}));
                    }
                });
                whole.allocate_ui_with_layout(Vec2::new(right_width,height),Layout::top_down(Align::Min),|right|{
                    if let Some(g)=self.draft.get(&self.selected).cloned(){
                        right.horizontal(|head|{
                            head.label(typography::section_heading("모델군 상세"));
                            head.with_layout(Layout::right_to_left(Align::Center),|buttons|{
                                if crate::button::action(buttons,"그룹 삭제",theme::ButtonRole::Danger,writable&&!busy).clicked(){
                                    self.confirm_delete=true;
                                }
                            });
                        });
                        let mut name=text(&g,"name").to_owned();
                        right.horizontal(|form|{
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
                        let width=right.available_width();
                        right.add_enabled_ui(writable&&!busy,|search|{
                            crate::input::management_search_field(search,&mut self.search,width,"추가할 상품 검색");
                        });
                        let assigned=self.membership();
                        let searching=!self.search.trim().is_empty();
                        if searching{
                            let query=self.search.trim().to_lowercase();
                            let choices=items(&catalog["products"]).iter()
                                .filter(|p|eligible.contains(text(p,"id")))
                                .filter(|p|!items(&g["productIds"]).iter().any(|id|id.as_str()==Some(text(p,"id"))))
                                .filter(|p|format!("{} {}",text(p,"name"),text(p,"subtitle")).to_lowercase().contains(&query))
                                .take(30).collect::<Vec<_>>();
                            right.label(typography::subsection_heading("상품 검색 결과"));
                            let result_height=(right.available_height()*0.30).min(theme::MANAGEMENT_TABLE_ROW_HEIGHT*3.0).max(1.0);
                            let rect=Self::table(right,"model-group-search-results",
                                ["상품명","상세 정보","소속 모델군","추가"],[140.0,100.0,110.0,80.0],
                                result_height,choices.len(),|row,cells,i,_|{
                                    let p=choices[i];let id=text(p,"id");
                                    Self::cell(row,cells[0],text(p,"name"),Role::ProductName,false,(id,0));
                                    Self::cell(row,cells[1],text(p,"subtitle"),Role::Description,false,(id,1));
                                    Self::cell(row,cells[2],assigned.get(id).map(String::as_str).unwrap_or("—"),Role::Text,false,(id,2));
                                    Self::actions(row,cells[3],|actions|{
                                        if crate::button::action(actions,"추가",theme::ButtonRole::Standard,
                                            writable&&!busy&&!assigned.contains_key(id)).clicked(){
                                            if let Some(ids)=self.draft[&self.selected]["productIds"].as_array_mut(){
                                                ids.push(json!(id));self.groups_cache.invalidate();self.status.clear();
                                            }
                                        }
                                    });
                                });
                            if choices.is_empty(){
                                crate::status_notice::management_body_state_overlay(right,rect,Some(
                                    if eligibility_pending{"상품을 확인하는 중입니다."}else{"일치하는 상품이 없습니다."}));
                            }
                        }
                        crate::ui_primitives::section_divider(right);
                        let ids=items(&g["productIds"]).iter().filter_map(Value::as_str).collect::<Vec<_>>();
                        right.horizontal(|heading|{
                            heading.label(typography::subsection_heading("등록 상품"));
                            heading.label(typography::caption(&format!("{}개",ids.len())));
                        });
                        if !text(&g,"representativeColor").is_empty()&&text(&g,"representativeProductId").is_empty(){
                            right.label(typography::caption(&format!("이전 대표색상: {} · 대표모델 재지정 필요",text(&g,"representativeColor"))));
                        }
                        let body_height=(right.available_height()-theme::FAMILY_FOOTER_HEIGHT-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0);
                        let rect=Self::table(right,("model-group-registered-products",self.selected.clone()),
                            ["상품명","상세 정보","대표모델","관리"],[140.0,100.0,96.0,176.0],
                            body_height,ids.len(),|row,cells,i,_|{
                                let id=ids[i];
                                let p=products.get(id).copied();
                                let rep=text(&g,"representativeProductId")==id;
                                Self::cell(row,cells[0],p.map(|p|text(p,"name")).unwrap_or("확인할 상품"),Role::ProductName,false,(id,0));
                                Self::cell(row,cells[1],p.map(|p|text(p,"subtitle")).unwrap_or(""),Role::Description,false,(id,1));
                                Self::cell(row,cells[2],if rep{"대표모델"}else{"—"},Role::Status,true,(id,2));
                                Self::actions(row,cells[3],|actions|{
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
                            crate::status_notice::management_body_state_overlay(right,rect,Some("상품을 검색해 추가하세요."));
                        }
                    }else{
                        let (rect,_)=right.allocate_exact_size(Vec2::new(right.available_width(),
                            (right.available_height()-theme::FAMILY_FOOTER_HEIGHT).max(1.0)),Sense::hover());
                        crate::status_notice::management_body_state_overlay(right,rect,Some("모델군을 선택하세요."));
                    }
                    crate::ui_primitives::soft_divider(right);
                    right.horizontal(|footer|{
                        footer.label(typography::caption(if self.dirty(){"저장하지 않은 변경사항 있음"}else{"변경사항 없음"}));
                        footer.with_layout(Layout::right_to_left(Align::Center),|buttons|{
                            if crate::button::action(buttons,"변경사항 저장",theme::ButtonRole::Primary,
                                writable&&!busy&&self.dirty()).clicked(){save=true;}
                            if crate::button::action(buttons,"닫기",theme::ButtonRole::Standard,!busy).clicked(){
                                self.request_exit(Exit::Close);
                            }
                        });
                    });
                });
            });
        });
        if save{self.save(gateway,ui.ctx().clone());}
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
    pub fn show_dialogs(&mut self,ctx:&Context,rect:Rect,
        gateway:Arc<dyn BandoGateway>){
        if self.confirm_delete{
            let mut delete=false;let mut cancel=false;
            crate::popup::modal_backdrop(ctx,"family-delete-dim",false);
            crate::popup::show_decision_window(ctx,"그룹 삭제","family-delete-confirm",rect,|dialog|{
                    crate::popup::decision_workspace(dialog,|decision|{
                        cancel=decision.header("그룹 삭제","취소",true);
                        decision.body(|dialog|{
                            dialog.label(typography::body(
                                "이 그룹을 삭제할까요? 기존 주문과 상품은 유지됩니다."));
                        });
                        decision.actions(|actions|{
                                delete=crate::button::action(actions,"삭제",
                                    theme::ButtonRole::Danger,true).clicked();
                            }
                        );
                    });
                });
            if cancel{self.confirm_delete=false;}
            else if delete{
                if let Some(groups)=self.draft.as_object_mut(){
                    groups.remove(&self.selected);self.groups_cache.invalidate();
                }
                self.selected.clear();self.confirm_delete=false;self.status.clear();
            }
        }
        if let Some(pending)=self.confirm.clone(){
            let mut cancel=false;let mut discard=false;let mut save=false;
            crate::popup::modal_backdrop(ctx,"family-unsaved-dim",true);
            crate::popup::show_decision_window(ctx,"모델군 관리","family-unsaved-confirm",rect,|dialog|{
                    crate::popup::decision_workspace(dialog,|decision|{
                        cancel=decision.header("모델군 관리","취소",!self.busy()
                        );
                        decision.body(|dialog|{
                            dialog.label(typography::body("모델군 관리의 변경사항을 저장할까요?"));
                        });
                        decision.actions(|actions|{
                                save=crate::button::action(actions,"저장",
                                    theme::ButtonRole::Primary,
                                    gateway.model_group_write_enabled()&&!self.busy()&&self.dirty()
                                    &&self.new_name.trim().is_empty()).clicked();
                                discard=crate::button::action(actions,"저장 안 함",
                                    theme::ButtonRole::Danger,!self.busy()).clicked();
                            }
                        );
                    });
                });
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
