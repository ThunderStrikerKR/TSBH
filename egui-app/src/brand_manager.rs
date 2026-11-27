//! React Classification.tsx::RegistrySettings (categories=false).
//! Only the four explicit brand edit fields live here; normalize/validate
//! and mutate catalog brands with original Rust catalog_clients::upsert_brand.
use crate::{theme,typography};
use eframe::egui::{Align,Context,Layout,Rect,Sense,Ui};
use serde_json::{json,Value};

fn field<'a>(row:&'a Value,key:&str)->&'a str{
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value:&Value)->&[Value]{
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
#[derive(Clone,Default,PartialEq,Eq)]
struct BrandForm{
    name:String,
    korean:String,
    aliases:Vec<String>,
    evidence:String,
    editing:Option<String>,
}
#[derive(Clone,PartialEq,Eq)]
enum Exit {Select(String),Close,Tab(usize),Global}

#[derive(Default)]
pub struct BrandManager{
    form:BrandForm,
    original:BrandForm,
    expected_entity:Value,
    editor_open:bool,
    list_query:String,
    alias_input:String,
    alias_notice:Option<String>,
    pending:Option<Exit>,
    after_save:Option<Exit>,
    next_tab:Option<usize>,
    global_approved:bool,
    error:Option<String>,
    notice:Option<String>,
}
impl BrandManager {
    fn dirty(&self)->bool{self.form!=self.original||!self.alias_input.trim().is_empty()}
    pub fn has_overlay(&self)->bool{self.editor_open||self.pending.is_some()}
    pub fn global_leave_in_progress(&self)->bool{
        self.pending==Some(Exit::Global)||self.after_save==Some(Exit::Global)
    }
    pub fn take_global_approval(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn take_tab(&mut self)->Option<usize>{self.next_tab.take()}
    pub fn reset_navigation(&mut self){
        self.editor_open=false;self.list_query.clear();
        self.form=BrandForm::default();
        self.alias_input.clear();self.alias_notice=None;
        self.original=self.form.clone();
        self.pending=None;
        self.after_save=None;
        self.error=None;
        self.notice=None;
    }
    pub fn request_tab_leave(&mut self,next:usize)->bool{
        if self.pending.is_some(){return false;}
        if self.dirty(){self.pending=Some(Exit::Tab(next));false}
        else{true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.pending.is_some(){return false;}
        if self.dirty(){self.pending=Some(Exit::Global);false}
        else{true}
    }
    fn apply_exit(&mut self,exit:Exit,catalog:&Value){
        match exit {
            Exit::Select(id)=>{
                if let Some(row)=rows(&catalog["brands"]).iter()
                    .find(|v|field(v,"id")==id){
                    self.form=BrandForm{
                        name:field(row,"name").into(),
                        korean:field(row,"korean").into(),
                        aliases:rows(&row["aliases"]).iter()
                            .filter_map(Value::as_str).map(str::to_owned).collect(),
                        evidence:field(row,"evidenceUrl").into(),
                        editing:Some(id),
                    };
                    self.editor_open=true;
                    self.alias_input.clear();self.alias_notice=None;
                    self.original=self.form.clone();
                    self.expected_entity=row.clone();
                    self.error=None;
                    self.notice=None;
                }
            }
            Exit::Close=>{self.close_editor();}
            Exit::Tab(tab)=>{self.close_editor();self.next_tab=Some(tab);}
            Exit::Global=>{self.close_editor();self.global_approved=true;}
        }
    }
    fn choose(&mut self,id:String,catalog:&Value){
        if self.form.editing.as_deref()==Some(id.as_str()){return;}
        if self.dirty(){self.pending=Some(Exit::Select(id));}
        else{self.apply_exit(Exit::Select(id),catalog);}
    }
    fn close_editor(&mut self){
        self.editor_open=false;self.form=BrandForm::default();self.original=self.form.clone();self.error=None;
        self.alias_input.clear();self.alias_notice=None;
    }
    fn add_alias(&mut self){
        let value=self.alias_input.trim();
        if value.is_empty(){return;}
        let key=|text:&str|text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        if self.form.aliases.iter().any(|alias|key(alias)==key(value)){
            self.alias_notice=Some("중복된 검색 별칭은 추가하지 않았습니다.".into());
        }else{
            self.form.aliases.push(value.to_owned());
            self.alias_notice=None;
        }
        self.alias_input.clear();
    }
    fn request(&self)->Value{
        json!({
            "editing":self.form.editing,
            "expectedEntity":if self.form.editing.is_some(){self.expected_entity.clone()}else{Value::Null},
            "name":self.form.name,
            "korean":self.form.korean,
            "aliases":self.form.aliases,
            "evidence":self.form.evidence
        })
    }
    pub fn on_saved(&mut self,catalog:&Value){
        self.close_editor();
        self.notice=Some("브랜드 정보를 저장했습니다. 기존 연결 상품은 유지됩니다.".into());
        if let Some(next)=self.after_save.take(){
            self.apply_exit(next,catalog);
        }
    }
    pub fn on_failed(&mut self,error:String){
        // Keep all four operator-entered fields and selected edit ID.
        self.error=Some(error);
        self.after_save=None;
        self.notice=None;
    }
    pub fn show_page(&mut self,ui:&mut Ui,catalog:&Value,editable:bool,busy:bool)
        ->Option<Value>{
        let mut select=None;
        let card_min_height=crate::card::management_master_table_card_min_inner_height(ui);
        crate::card::measured_management_master_card(ui,|card|{
            crate::card::fill_management_primary_card(card,card_min_height);
            card.horizontal(|toolbar|{
                let width=theme::CATALOG_SEARCH_WIDTH.min((toolbar.available_width()-160.0).max(1.0));
                crate::input::management_search_field(toolbar,&mut self.list_query,width,"브랜드명 또는 검색 별칭 검색");
                toolbar.with_layout(Layout::right_to_left(Align::Center),|actions|{
                    if crate::button::action(actions,"+ 브랜드 추가",theme::ButtonRole::Primary,
                        editable&&!busy&&!self.editor_open).clicked(){
                        self.close_editor();self.editor_open=true;
                    }
                });
            });
            crate::ui_primitives::section_divider(card);
            use typography::ManagementTableCellRole as Role;
            let query=self.list_query.trim().to_lowercase();
            let brands=rows(&catalog["brands"]).iter().filter(|brand|{
                query.is_empty()||["name","korean"].iter().any(|key|field(brand,key).to_lowercase().contains(&query))
                ||rows(&brand["aliases"]).iter().filter_map(Value::as_str).any(|alias|alias.to_lowercase().contains(&query))
            }).collect::<Vec<_>>();
            let height=(card.available_height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0);
            let rect=crate::table::registry_table(card,"brand-registry-list",
                ["브랜드명","한글 표기","검색 별칭"],
                crate::table::RegistryColumns::from([180.0,180.0,220.0+108.0+theme::SPACE_8])
                    .redistribute_to([1.0/3.0,1.0,1.0],2),height,brands.len(),
                |i|self.form.editing.as_deref()==Some(field(&brands[i],"id")),
                |row,cells,i,rect|{
                    let brand=&brands[i];
                    let id=field(brand,"id");
                    let enabled=!busy&&!self.editor_open;
                    let selected=self.form.editing.as_deref()==Some(id);
                    let hit=row.interact(rect,row.make_persistent_id(id),if enabled{Sense::click()}else{Sense::hover()});
                    if hit.hovered()&&!selected&&enabled{
                        crate::table::paint_management_interactive_row_background(row.painter(),rect,i,true);
                    }
                    let aliases=rows(&brand["aliases"]).iter().filter_map(Value::as_str)
                        .map(str::to_owned).collect::<Vec<_>>();
                    crate::table::registry_cell(row,cells[0],field(brand,"name"),Role::Text,false,(id,0));
                    crate::table::registry_cell(row,cells[1],field(brand,"korean"),Role::Text,false,(id,1));
                    crate::table::registry_tag_cell(row,cells[2],&aliases,(id,2));
                    if hit.clicked()&&enabled{select=Some(id.to_owned());}
                    #[cfg(test)]
                    crate::ui_test_support::record("catalog data row",rect);
                    let _=rect;
                });
            if brands.is_empty(){
                crate::status_notice::management_body_state_overlay(card,rect,Some(if query.is_empty(){"등록된 브랜드가 없습니다."}else{"검색 결과가 없습니다."}));
            }

        });
        if let Some(id)=select{self.choose(id,catalog);}
        None
    }
    fn show_editor(&mut self,ctx:&Context,work_area:Rect,editable:bool,busy:bool)->Option<Value>{
        if !self.editor_open{return None;}
        let title=if self.form.editing.is_some(){"브랜드 편집"}else{"브랜드 추가"};
        let mut close=false;let mut save=false;
        let nested=self.pending.is_some();
        crate::popup::modal_backdrop(ctx,"brand-editor-dim",false);
        crate::popup::modal_window(ctx,title,"brand-editor",work_area)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|modal|{
                crate::popup::modal_flow_workspace(modal,|workspace|{
                    close=workspace.header(title,"닫기",!busy&&!nested);
                    workspace.scroll_region("brand-editor-body",crate::popup::modal_editor_body_max_height(work_area.height()),|body|{
                        body.add_enabled_ui(editable&&!busy&&!nested,|body|{
                            crate::popup::modal_form_row_spacing(body);
                            for (label,value,hint) in [
                                ("브랜드명",&mut self.form.name,"브랜드명"),
                                ("한글 표기",&mut self.form.korean,"확인된 한글 표기")]{
                                crate::popup::modal_compact_text_field(body,label,value,hint);
                            }
                            crate::popup::modal_labeled_row_with_width(body,"검색 별칭",theme::MODAL_FORM_WIDE_LABEL_WIDTH,|line|{
                            line.vertical(|body|{
                            let interaction=crate::input::tag_block_editor(body,&self.form.aliases,
                                &mut self.alias_input,"검색 별칭 입력");
                            if interaction.changed{self.alias_notice=None;}
                            if let Some(tag)=interaction.remove{
                                self.form.aliases.retain(|alias|alias!=&tag);
                                self.alias_notice=None;
                            }
                            if interaction.add{
                                crate::floating_notice::dispatch_pointer(ctx,"brands.alias");
                                self.add_alias();
                            }
                            crate::floating_notice::sync(ctx,"brands.alias.feedback","brands.alias",
                                crate::floating_notice::Tone::Validation,self.alias_notice.as_deref());
                            });
                            });
                            crate::popup::modal_compact_text_field(body,"확인 근거 URL",&mut self.form.evidence,"확인 근거 URL");
                        });
                    });
                    workspace.footer_single(|actions|{
                        save=crate::button::action(actions,"브랜드 저장",theme::ButtonRole::Primary,
                            editable&&!busy&&!nested&&self.alias_input.trim().is_empty()&&self.dirty()).clicked();
                        if save{crate::floating_notice::dispatch_pointer(ctx,"brands.save");}
                    });
                    crate::floating_notice::sync(ctx,"brands.save.error","brands.save",
                        crate::floating_notice::Tone::Error,self.error.as_deref());
                });
            });
        if close{if self.dirty(){self.pending=Some(Exit::Close);}else{self.close_editor();}}
        save.then(||self.request())
    }
    pub fn show_dialogs(&mut self,ctx:&Context,work_area:Rect,catalog:&Value,
        editable:bool,busy:bool)->Option<Value>{
        let editor_save=self.show_editor(ctx,work_area,editable,busy);
        let Some(exit)=self.pending.clone()else{return editor_save;};
        crate::popup::modal_backdrop(ctx,"brand-unsaved-dim",true);
        let decision=crate::popup::show_save_confirmation(
            ctx,work_area,"brand-unsaved-confirm","브랜드",
            "브랜드 변경사항을 저장할까요?","저장",!busy,editable&&!busy&&self.alias_input.trim().is_empty()
        );
        let cancel=decision==crate::popup::ChangeDecision::Cancel;
        let discard=decision==crate::popup::ChangeDecision::Discard;
        let save=decision==crate::popup::ChangeDecision::Save;
        if cancel{
            self.pending=None;
            None
        }else if discard{
            self.pending=None;
            self.form=self.original.clone();
            self.apply_exit(exit,catalog);
            None
        }else if save{
            self.pending=None;
            self.after_save=Some(exit);
            Some(self.request())
        }else{None}
    }
}

#[cfg(test)]
mod modal_lifecycle_tests{
    use super::*;
    #[test]
    fn alias_blocks_save_as_array_without_splitting(){
        let catalog=json!({"brands":[{"id":"b","name":"BANDO","aliases":["기존"]}]});
        let mut manager=BrandManager::default();
        manager.choose("b".into(),&catalog);
        manager.alias_input="A, B".into();
        assert!(manager.dirty());
        assert_eq!(manager.request()["aliases"],json!(["기존"]));
        manager.add_alias();
        assert_eq!(manager.request()["aliases"],json!(["기존","A, B"]));
        manager.alias_input="a, b".into();
        manager.add_alias();
        assert_eq!(manager.form.aliases.len(),2);
        assert!(manager.alias_input.is_empty());
    }
    #[test]
    fn actual_brand_popup_feedback_never_moves_controls(){
        use crate::ui_test_support::{begin_component_geometry,finish_component_geometry,
            assert_layout_rects_invariant,test_context};
        let catalog=json!({"brands":[{"id":"b","name":"BANDO","aliases":["existing"]}]});
        let ctx=test_context();
        let mut manager=BrandManager::default();
        manager.choose("b".into(),&catalog);
        let work=Rect::from_min_size(eframe::egui::Pos2::ZERO,eframe::egui::vec2(1400.0,900.0));
        let mut baseline=None;
        for phase in 0..5{
            match phase{
                1=>manager.alias_input="pending alias".into(),
                2=>{manager.alias_input="existing".into();manager.add_alias();
                    assert!(manager.alias_notice.is_some());},
                3=>manager.on_failed("synthetic save error".into()),
                4=>{manager.alias_notice=None;manager.error=None;},
                _=>{}
            }
            let mut slots=Vec::new();
            for _ in 0..6{
                begin_component_geometry();
                let _=ctx.run(eframe::egui::RawInput{screen_rect:Some(work),..Default::default()},|ctx|{
                    crate::floating_notice::begin_page(ctx,"brand feedback test");
                    manager.show_editor(ctx,work,true,false);
                    crate::floating_notice::show(ctx);
                });
                slots=finish_component_geometry(work).into_iter()
                    .filter(|sample|matches!(sample.role,"input"|"button"|"popup"|"scrollbar"))
                    .map(|sample|(sample.role,sample.rect)).collect::<Vec<_>>();
            }
            assert!(slots.iter().filter(|(role,_)|*role=="input").count()>=4);
            if let Some(before)=baseline.as_ref(){
                assert_layout_rects_invariant(before,&slots);
            }else{baseline=Some(slots);}
        }
    }
    #[test]
    fn correction_and_close_preserve_catalog_and_clear_editor(){
        let catalog=json!({"brands":[{"id":"b","name":"BANDO","korean":"반도","aliases":["반도스탠드"],"evidenceUrl":"https://example.com"}]});
        let mut manager=BrandManager::default();
        manager.choose("b".into(),&catalog);
        assert!(manager.has_overlay());assert!(!manager.dirty());
        manager.form.name="Changed".into();
        manager.on_failed("test error".into());
        assert!(manager.has_overlay());assert!(manager.dirty());
        manager.apply_exit(Exit::Close,&catalog);
        assert!(!manager.has_overlay());assert!(!manager.dirty());
        assert_eq!(catalog["brands"][0]["name"],"BANDO");
        manager.choose("b".into(),&catalog);
        manager.on_saved(&catalog);
        assert!(!manager.has_overlay());assert!(!manager.dirty());
    }
}
