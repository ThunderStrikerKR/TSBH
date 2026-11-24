//! Staff-driven model families. React oracle: ColorGuidanceManager.tsx.
//! Saved data stays in settings.colorGuidanceGroups, never product/order rows.
//! Rust catalog_clients::model_groups validates; Tauri settings_patch CAS saves.
use crate::{catalog_query_bridge,components, gateway::BandoGateway, theme, typography};
use crate::latest_job::LatestJob;
use eframe::egui::{self, Align, Context, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use serde_json::{json, Value};
use std::{
    collections::{HashMap,HashSet},
    sync::{Arc,atomic::{AtomicU64,Ordering},mpsc::{self,Receiver,TryRecvError}},
    time::Duration,
};
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
    eligibility:LatestJob<HashSet<String>>,
    eligible:Arc<HashSet<String>>,
}
impl Default for ModelGroupManager{
    fn default()->Self{
        Self{initialized:false,baseline:Value::Null,draft:json!({}),
            selected:String::new(),new_name:String::new(),
            query:String::new(),search:String::new(),status:String::new(),
            write:None,confirm:None,after_save:None,confirm_delete:false,
            leave_tab:None,global_approved:false,
            eligibility_snapshot:None,eligibility:LatestJob::default(),eligible:Arc::default()}
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
        let latest=original_groups(catalog);
        if !self.initialized || (!self.dirty()&&self.new_name.trim().is_empty()
            &&self.write.is_none()&&self.baseline!=latest){
            self.initialized=true;
            self.baseline=latest.clone();
            self.draft=if latest.is_object(){latest}else{json!({})};
            if self.draft.get(&self.selected).is_none(){self.selected.clear();}
        }
    }
    pub fn dirty(&self)->bool{
        let old=if self.baseline.is_object(){self.baseline.clone()}else{json!({})};
        self.draft!=old
    }
    fn unsaved(&self)->bool{self.dirty()||!self.new_name.trim().is_empty()}
    pub fn busy(&self)->bool{self.write.is_some()}
    pub fn has_overlay(&self)->bool{self.confirm_delete||self.confirm.is_some()}
    pub fn request_tab_leave(&mut self,index:usize)->bool{
        if self.busy(){return false;}
        if self.unsaved(){self.confirm=Some(Exit::Tab(index));false}
        else{self.reset_navigation();true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.busy(){return false;}
        if self.unsaved(){self.confirm=Some(Exit::Global);false}
        else{self.reset_navigation();true}
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
        self.draft=if self.baseline.is_object(){self.baseline.clone()}else{json!({})};
        self.new_name.clear();self.status.clear();self.selected.clear();
        self.search.clear();
    }
    fn finish_exit(&mut self,next:Exit){
        match next{
            Exit::Select(id)=>{self.selected=id;self.search.clear();self.status.clear();}
            Exit::Close=>{self.selected.clear();self.search.clear();self.status.clear();}
            Exit::Tab(index)=>{self.reset_navigation();self.leave_tab=Some(index);}
            Exit::Global=>{self.reset_navigation();self.global_approved=true;}
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
        self.draft[&id]=json!({"name":name,"productIds":[],"representativeProductId":""});
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
        if !self.new_name.trim().is_empty(){
            self.status="새 그룹 이름은 먼저 그룹 생성으로 등록하세요.".into();return;
        }
        if !gateway.model_group_write_enabled(){
            self.status="모델군 변경 저장 권한이 없습니다.".into();return;
        }
        crate::floating_notice::dispatch_pointer(&ctx,"products.model");
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
    fn row(&mut self,ui:&mut Ui,id:&str,g:&Value,catalog:&Value){
        let (rect,response)=ui.allocate_exact_size(
            Vec2::new(ui.available_width(),theme::FAMILY_ROW_HEIGHT),Sense::click());
        if self.selected==id||response.hovered(){
            ui.painter().rect_filled(rect,theme::CONTROL_RADIUS as f32,
                if self.selected==id{theme::RAISED}else{theme::INPUT});
        }
        if self.selected!=id && !self.busy(){
            theme::paint_button_hover(
                ui,&response,theme::CONTROL_RADIUS as f32
            );
        }
        ui.scope_builder(UiBuilder::new().max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),|line|{
                line.add_space(theme::SPACE_12);
                line.vertical(|info|{
                    info.label(typography::body(text(g,"name")));
                    let rep=text(g,"representativeProductId");
                    let subtitle=if let Some(p)=product(catalog,rep){
                        format!("대표모델: {}",text(p,"name"))
                    }else if !text(g,"representativeColor").is_empty(){
                        format!("이전 대표색상 {} / 대표모델 재지정 필요",
                            text(g,"representativeColor"))
                    }else{"대표모델: 미지정".into()};
                    info.label(typography::caption(&subtitle));
                });
                line.with_layout(Layout::right_to_left(Align::Center),|end|{
                    end.add_space(theme::SPACE_12);
                    end.label(typography::caption(&format!(
                        "{}개 상품",items(&g["productIds"]).len())));
                });
            });
        if response.clicked()&&!self.busy(){
            self.request_exit(Exit::Select(id.to_owned()));
        }
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
        let groups=self.draft.as_object().map(|m|
            m.iter().map(|(id,g)|(id.clone(),g.clone())).collect::<Vec<_>>())
            .unwrap_or_default();
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        let mut save=false;
        theme::management_master_table_frame().show(ui,|outer|{
            components::fill_management_primary_card(outer,card_min_height);
            outer.spacing_mut().scroll=theme::management_table_scroll_style();
            let available=outer.available_width();
            let left_width=theme::FAMILY_LIST_WIDTH.min(available*0.45);
            let height=outer.available_height().max(
                theme::MANAGEMENT_TABLE_ROW_HEIGHT*6.0
            );
            outer.horizontal(|whole|{
                whole.allocate_ui_with_layout(
                    Vec2::new(left_width,height),
                    Layout::top_down(Align::Min),|left|{
                        left.label(typography::section_heading("직원 등록 모델군"));
                        left.label(typography::caption(
                            "색상, 크기, 규격, 타공 여부가 다른 상품을 직원이 직접 묶고 그중 대표모델을 정합니다. 자동으로 묶거나 주문에 자동 적용하지 않습니다."
                        ));
                        components::soft_divider(left);
                        left.label(typography::field_label("새 그룹 이름"));
                        left.horizontal(|form|{
                            form.add_enabled_ui(writable&&!busy,|entry|{
                                let edit=components::flexible_text_field(
                                    entry,&mut self.new_name,
                                    theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                                    "직원이 알아보기 쉬운 이름"
                                );
                                crate::floating_notice::sized_text_caret(
                                    entry,"products.model.input",&edit,&self.new_name,
                                    egui::FontId::proportional(typography::FONT_BODY),
                                    false,8.0
                                );
                            });
                            if theme::action(form,"그룹 생성",theme::ButtonRole::Standard,
                                writable&&!busy).clicked(){
                                crate::floating_notice::dispatch_pointer(form.ctx(),"products.model");
                                self.create();
                            }
                        });
                        components::soft_divider(left);
                        left.horizontal(|search|{
                            components::form_field_label(search,"상품 검색");
                            components::management_search_field(search,&mut self.query,
                                theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                                "그룹 이름 또는 상품명 검색");
                        });
                        // Fixed register header and one local scroll
                        // viewport, as in the Shipping Assignment master.
                        left.spacing_mut().item_spacing.y=0.0;
                        left.spacing_mut().scroll=theme::management_table_scroll_style();
                        let header=components::management_master_register_header(
                            left,"모델군","상품 수",104.0
                        );
                        let list_height=left.available_height().max(1.0);
                        let scroll_track=components::management_table_scroll_track_rect(
                            left,list_height
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("family-list")
                            .scroll_bar_rect(scroll_track)
                            .scroll_bar_visibility(
                                egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                            )
                            .max_height(list_height)
                            .auto_shrink([false,false])
                            .show(left,|list|{
                            list.set_min_width(header.table_width);
                            for (id,g) in &groups{
                                let names=items(&g["productIds"]).iter()
                                    .filter_map(Value::as_str).filter_map(|pid|product(catalog,pid))
                                    .map(|p|text(p,"name")).collect::<Vec<_>>().join(" ");
                                if !format!("{} {names}",text(g,"name")).to_lowercase()
                                    .contains(&self.query.trim().to_lowercase()){continue;}
                                self.row(list,id,g,catalog);
                            }
                            if groups.is_empty(){
                                list.label(typography::body("등록된 모델군이 없습니다."));
                            }
                        });
                    }
                );
                // React .family-list-panel has a single border-right;
                // the shared horizontal divider would add the wrong axis.
                let (separator,_)=whole.allocate_exact_size(
                    Vec2::new(theme::SOFT_DIVIDER_WIDTH,height),Sense::hover());
                whole.painter().rect_filled(separator,0.0,theme::BORDER);
                whole.allocate_ui_with_layout(
                    Vec2::new(whole.available_width(),height),
                    Layout::top_down(Align::Min),|right|{
                        if let Some(g)=self.draft.get(&self.selected).cloned(){
                            right.horizontal(|head|{
                                head.label(typography::section_heading("모델군 및 대표모델 설정"));
                                head.with_layout(Layout::right_to_left(Align::Center),|buttons|{
                                    if theme::action(buttons,"그룹 삭제",theme::ButtonRole::Danger,
                                        writable&&!busy).clicked(){
                                        self.confirm_delete=true;
                                    }
                                });
                            });
                            components::soft_divider(right);
                            egui::ScrollArea::vertical().id_salt("family-editor")
                                .auto_shrink([false,false])
                                .max_height((height-theme::FAMILY_FOOTER_HEIGHT).max(
                                    theme::MANAGEMENT_TABLE_ROW_HEIGHT))
                                .show(right,|body|{
                                    body.label(typography::field_label("그룹 이름"));
                                    let mut name=text(&g,"name").to_owned();
                                    body.add_enabled_ui(writable&&!busy,|field|{
                                        components::flexible_text_field(field,&mut name,
                                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"그룹 이름");
                                    });
                                    if name!=text(&g,"name"){
                                        self.draft[&self.selected]["name"]=json!(name);
                                        self.status.clear();
                                    }
                                    body.add_space(theme::SPACE_16);
                                    components::soft_divider(body);
                                    body.label(typography::subsection_heading("상품 추가"));
                                    body.add_enabled_ui(writable&&!busy,|search|{
                                        components::management_search_field(search,&mut self.search,
                                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                                            "상품명으로 찾아서 추가");
                                    });
                                    let assigned=self.membership();
                                    if !self.search.trim().is_empty(){
                                        let query=self.search.trim().to_lowercase();
                                        let choices=items(&catalog["products"]).iter()
                                            .filter(|p|eligible.contains(text(p,"id")))
                                            .filter(|p|!items(&g["productIds"]).iter()
                                                .any(|id|id.as_str()==Some(text(p,"id"))))
                                            .filter(|p|format!("{} {}",text(p,"name"),
                                                text(p,"subtitle")).to_lowercase().contains(&query))
                                            .take(30).cloned().collect::<Vec<_>>();
                                        if choices.is_empty()&&!eligibility_pending{
                                            body.label(typography::body("일치하는 상품이 없습니다."));
                                        }
                                        for p in choices{
                                            body.horizontal(|row|{
                                                row.vertical(|name|{
                                                    name.label(typography::body(text(&p,"name")));
                                                    if !text(&p,"subtitle").is_empty(){
                                                        name.label(typography::caption(text(&p,"subtitle")));
                                                    }
                                                    if let Some(group)=assigned.get(text(&p,"id")){
                                                        name.label(typography::caption(&format!(
                                                            "현재 그룹: {group}")));
                                                    }
                                                });
                                                row.with_layout(Layout::right_to_left(Align::Center),
                                                    |actions|{
                                                        if theme::action(actions,"추가",
                                                            theme::ButtonRole::Standard,
                                                            writable&&!busy&&
                                                                !assigned.contains_key(text(&p,"id"))
                                                        ).clicked(){
                                                            if let Some(ids)=self.draft[&self.selected]
                                                                ["productIds"].as_array_mut(){
                                                                ids.push(json!(text(&p,"id")));
                                                                self.status.clear();
                                                            }
                                                        }
                                                    });
                                            });
                                            components::soft_divider(body);
                                        }
                                    }
                                    body.add_space(theme::SPACE_16);
                                    components::soft_divider(body);
                                    body.horizontal(|heading|{
                                        heading.label(typography::subsection_heading("등록한 상품"));
                                        heading.label(typography::caption(
                                            &format!("{}개",items(&g["productIds"]).len())));
                                    });
                                    let representative_notice=if
                                        !text(&g,"representativeColor").is_empty()
                                        &&text(&g,"representativeProductId").is_empty(){
                                        Some(format!(
                                            "이전 대표색상 설정({})이 있습니다. 아래 상품의 '대표 지정' 버튼으로 실제 대표모델을 선택하세요.",
                                            text(&g,"representativeColor")
                                        ))
                                    }else{None};
                                    let notice_width=body.available_width();
                                    components::fixed_status_slot(
                                        body,notice_width,typography::METADATA_LINE_HEIGHT,
                                        representative_notice.as_deref(),
                                        representative_notice.as_deref(),theme::MUTED
                                    );
                                    let ids=items(&g["productIds"]).iter().filter_map(Value::as_str)
                                        .map(str::to_owned).collect::<Vec<_>>();
                                    if ids.is_empty(){
                                        body.label(typography::body("위에서 상품을 검색해 추가하세요."));
                                    }
                                    for id in ids{
                                        body.horizontal(|row|{
                                            if let Some(p)=product(catalog,&id){
                                                row.vertical(|info|{
                                                    info.label(typography::body(text(p,"name")));
                                                    info.label(typography::caption(text(p,"subtitle")));
                                                });
                                            }else{row.label(typography::body("확인할 상품"));}
                                            row.with_layout(Layout::right_to_left(Align::Center),
                                                |actions|{
                                                    if theme::action(actions,"제외",
                                                        theme::ButtonRole::Standard,
                                                        writable&&!busy).clicked(){
                                                        let values=items(&self.draft[&self.selected]
                                                            ["productIds"]).iter()
                                                            .filter(|v|v.as_str()!=Some(&id))
                                                            .cloned().collect::<Vec<_>>();
                                                        self.draft[&self.selected]["productIds"]=json!(values);
                                                        if text(&self.draft[&self.selected],
                                                            "representativeProductId")==id{
                                                            self.draft[&self.selected]
                                                                ["representativeProductId"]=json!("");
                                                        }
                                                        self.status.clear();
                                                    }
                                                    if text(&g,"representativeProductId")==id{
                                                        actions.label(typography::caption("대표모델"));
                                                    }else if theme::action(actions,"대표 지정",
                                                        theme::ButtonRole::Standard,
                                                        writable&&!busy&&product(catalog,&id).is_some()
                                                    ).clicked(){
                                                        self.draft[&self.selected]
                                                            ["representativeProductId"]=json!(id);
                                                        if let Some(group)=self.draft.get_mut(&self.selected)
                                                            .and_then(Value::as_object_mut){
                                                            group.remove("representativeColor");
                                                        }
                                                        self.status.clear();
                                                    }
                                                });
                                        });
                                        components::soft_divider(body);
                                    }
                                });
                            components::soft_divider(right);
                            right.horizontal(|footer|{
                                footer.label(typography::caption(if self.dirty(){"저장하지 않은 변경사항 있음"} else {"변경사항 없음"}));
                                footer.with_layout(Layout::right_to_left(Align::Center),|buttons|{
                                    if theme::action(buttons,
                                        "변경사항 저장",
                                        theme::ButtonRole::Primary,
                                        writable&&!busy&&self.dirty()).clicked(){
                                        save=true;
                                    }
                                    if theme::action(buttons,"닫기",
                                        theme::ButtonRole::Standard,!busy).clicked(){
                                        self.request_exit(Exit::Close);
                                    }
                                });
                            });
                        }else{
                            right.label(typography::body(
                                "모델군을 만든 다음 상품을 추가하고 대표모델을 직접 지정하세요."));
                            // Keep the footer, both actions and their hit
                            // rectangles stable when dirty state changes.
                            components::soft_divider(right);
                            right.horizontal(|footer|{
                                footer.label(typography::caption(
                                    if self.dirty(){"저장하지 않은 변경사항 있음"}
                                    else{"변경사항 없음"}
                                ));
                                if theme::action(footer,"닫기",
                                    theme::ButtonRole::Standard,!busy).clicked(){
                                    self.request_exit(Exit::Close);
                                }
                                if theme::action(footer,"변경사항 저장",
                                    theme::ButtonRole::Primary,
                                    writable&&!busy&&self.dirty()).clicked(){
                                    save=true;
                                }
                            });
                        }
                    }
                );
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
            components::modal_backdrop(ctx,"family-delete-dim",false);
            components::decision_window("그룹 삭제","family-delete-confirm",rect)
                .show(ctx,|dialog|{
                    components::decision_workspace(dialog,|decision|{
                        cancel=decision.header("그룹 삭제","취소",true);
                        decision.body(|dialog|{
                            dialog.label(typography::body(
                                "이 그룹을 삭제할까요? 기존 주문과 상품은 유지됩니다."));
                        });
                        decision.actions(|actions|{
                                delete=theme::action(actions,"삭제",
                                    theme::ButtonRole::Danger,true).clicked();
                            }
                        );
                    });
                });
            if cancel{self.confirm_delete=false;}
            else if delete{
                if let Some(groups)=self.draft.as_object_mut(){
                    groups.remove(&self.selected);
                }
                self.selected.clear();self.confirm_delete=false;self.status.clear();
            }
        }
        if let Some(pending)=self.confirm.clone(){
            let mut cancel=false;let mut discard=false;let mut save=false;
            components::modal_backdrop(ctx,"family-unsaved-dim",true);
            components::decision_window("모델군 관리","family-unsaved-confirm",rect)
                .show(ctx,|dialog|{
                    components::decision_workspace(dialog,|decision|{
                        cancel=decision.header("모델군 관리","취소",!self.busy()
                        );
                        decision.body(|dialog|{
                            dialog.label(typography::body("모델군 관리의 변경사항을 저장할까요?"));
                        });
                        decision.actions(|actions|{
                                save=theme::action(actions,"저장",
                                    theme::ButtonRole::Primary,
                                    gateway.model_group_write_enabled()&&!self.busy()&&self.dirty()
                                    &&self.new_name.trim().is_empty()).clicked();
                                discard=theme::action(actions,"저장 안 함",
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
