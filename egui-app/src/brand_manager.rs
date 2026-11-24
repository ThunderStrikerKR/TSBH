//! React Classification.tsx::RegistrySettings (categories=false).
//! Only the four explicit brand edit fields live here; normalize/validate
//! and mutate catalog brands with original Rust catalog_clients::upsert_brand.
use crate::{components,theme,typography};
use eframe::egui::{self,Align,Context,Layout,Rect,Ui};
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
    aliases:String,
    evidence:String,
    editing:Option<String>,
}
#[derive(Clone,PartialEq,Eq)]
enum Exit {Select(String),Tab(usize),Global}

#[derive(Default)]
pub struct BrandManager{
    form:BrandForm,
    original:BrandForm,
    pending:Option<Exit>,
    after_save:Option<Exit>,
    next_tab:Option<usize>,
    global_approved:bool,
    error:Option<String>,
    notice:Option<String>,
}
impl BrandManager {
    fn dirty(&self)->bool{self.form!=self.original}
    pub fn has_overlay(&self)->bool{self.pending.is_some()}
    pub fn global_leave_in_progress(&self)->bool{
        self.pending==Some(Exit::Global)||self.after_save==Some(Exit::Global)
    }
    pub fn take_global_approval(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn take_tab(&mut self)->Option<usize>{self.next_tab.take()}
    pub fn reset_navigation(&mut self){
        self.form=BrandForm::default();
        self.original=self.form.clone();
        self.pending=None;
        self.after_save=None;
        self.error=None;
        self.notice=None;
    }
    pub fn request_tab_leave(&mut self,next:usize)->bool{
        if self.pending.is_some(){return false;}
        if self.dirty(){self.pending=Some(Exit::Tab(next));false}
        else{self.reset_navigation();true}
    }
    pub fn request_global_leave(&mut self)->bool{
        if self.pending.is_some(){return false;}
        if self.dirty(){self.pending=Some(Exit::Global);false}
        else{self.reset_navigation();true}
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
                            .filter_map(Value::as_str).collect::<Vec<_>>().join(","),
                        evidence:field(row,"evidenceUrl").into(),
                        editing:Some(id),
                    };
                    self.original=self.form.clone();
                    self.error=None;
                    self.notice=None;
                }
            }
            Exit::Tab(tab)=>{self.reset_navigation();self.next_tab=Some(tab);}
            Exit::Global=>{self.reset_navigation();self.global_approved=true;}
        }
    }
    fn choose(&mut self,id:String,catalog:&Value){
        if self.form.editing.as_deref()==Some(id.as_str()){return;}
        if self.dirty(){self.pending=Some(Exit::Select(id));}
        else{self.apply_exit(Exit::Select(id),catalog);}
    }
    fn request(&self)->Value{
        json!({
            "editing":self.form.editing,
            "name":self.form.name,
            "korean":self.form.korean,
            "aliases":self.form.aliases,
            "evidence":self.form.evidence
        })
    }
    pub fn on_saved(&mut self,catalog:&Value){
        // Original React RegistrySettings resets all fields on success.
        self.form=BrandForm::default();
        self.original=self.form.clone();
        self.error=None;
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
        let mut save=false;
        let mut select=None;
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        theme::management_master_table_frame().show(ui,|card|{
            components::fill_management_primary_card(card,card_min_height);
            card.label(typography::section_heading("브랜드 관리"));
            card.add_space(theme::SPACE_8);
            card.label(typography::body(
                "공식명 / 한글명 / 검색 별칭을 직원이 확인해 연결합니다. 유사한 이름을 자동 병합하지 않습니다."
            ));
            card.add_space(theme::SPACE_12);
            // React registry-edit-form uses auto-fit columns >=240px,
            // with a right-aligned primary action at the field baseline.
            card.horizontal_wrapped(|form|{
                form.vertical(|column|{
                    column.label(typography::field_label("공식 브랜드명"));
                    column.add_enabled_ui(editable&&!busy,|input|{
                        components::flexible_text_field(input,&mut self.form.name,
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"공식 브랜드명");
                    });
                });
                form.vertical(|column|{
                    column.label(typography::field_label("한글 표기"));
                    column.add_enabled_ui(editable&&!busy,|input|{
                        components::flexible_text_field(input,&mut self.form.korean,
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"확인된 한글 표기");
                    });
                });
                form.vertical(|column|{
                    column.label(typography::field_label("검색 별칭"));
                    column.add_enabled_ui(editable&&!busy,|input|{
                        components::flexible_text_field(input,&mut self.form.aliases,
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"별칭 (쉼표 구분)");
                    });
                });
                form.vertical(|column|{
                    column.label(typography::field_label("확인 근거 URL"));
                    column.add_enabled_ui(editable&&!busy,|input|{
                        components::flexible_text_field(input,&mut self.form.evidence,
                            theme::MANAGEMENT_SEARCH_MIN_WIDTH,"확인 근거 URL");
                    });
                });
                form.vertical(|column|{
                    // React .registry-edit-actions: right-aligned inside its
                    // auto-fit field column, not left-anchored like a label.
                    column.set_min_width(theme::MANAGEMENT_SEARCH_MIN_WIDTH);
                    column.add_space(theme::SPACE_24);
                    column.with_layout(Layout::right_to_left(Align::Center),|actions|{
                        if theme::action(actions,"브랜드 저장",
                            theme::ButtonRole::Primary,editable&&!busy).clicked(){
                            save=true;
                        }
                    });
                });
            });
            // Shared Freight master: toolbar-to-header transition,
            // fixed 36pt register header and 10pt solid scroll rail.
            card.spacing_mut().item_spacing.y=0.0;
            card.spacing_mut().scroll=theme::management_table_scroll_style();
            let header=components::management_master_register_header(
                card,"브랜드명 / 한글 표기 / 검색 별칭","관리",132.0
            );
            let scroll_height=components::management_single_list_scroll_height(
                card,card_min_height,0.0
            );
            let scroll_track=components::management_table_scroll_track_rect(
                card,scroll_height
            );
            egui::ScrollArea::vertical()
                .id_salt("brand-registry-list")
                .scroll_bar_rect(scroll_track)
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                )
                .max_height(scroll_height)
                .auto_shrink([false,false])
                .show(card,|list|{
                    list.set_min_width(header.table_width);
                    for brand in rows(&catalog["brands"]){
                        let id=field(brand,"id");
                        list.horizontal(|line|{
                            line.vertical(|description|{
                                description.label(typography::body(field(brand,"name")));
                                description.label(typography::caption(&format!(
                                    "한글 표기: {}",if field(brand,"korean").is_empty(){
                                        "없음"
                                    }else{field(brand,"korean")})));
                                let aliases=rows(&brand["aliases"]).iter()
                                    .filter_map(Value::as_str)
                                    .collect::<Vec<_>>().join(", ");
                                description.label(typography::caption(&format!(
                                    "검색 별칭: {}",if aliases.is_empty(){
                                        "없음"
                                    }else{&aliases})));
                            });
                            line.with_layout(Layout::right_to_left(Align::Center),|actions|{
                                if theme::action(actions,"정정",
                                    theme::ButtonRole::Standard,!busy).clicked(){
                                    select=Some(id.to_owned());
                                }
                            });
                        });
                        components::soft_divider(list);
                    }
                });
        });
        if let Some(id)=select{self.choose(id,catalog);}
        save.then(||self.request())
    }
    pub fn show_dialogs(&mut self,ctx:&Context,work_area:Rect,catalog:&Value,
        editable:bool,busy:bool)->Option<Value>{
        let Some(exit)=self.pending.clone()else{return None;};
        let mut cancel=false;
        let mut discard=false;
        let mut save=false;
        components::modal_backdrop(ctx,"brand-unsaved-dim",true);
        components::decision_window("브랜드","brand-unsaved-confirm",work_area)
            .show(ctx,|dialog|{
                components::decision_workspace(dialog,|decision|{
                    cancel=decision.header("브랜드","취소",!busy);
                    decision.body(|dialog|{
                        dialog.label(typography::body("브랜드 변경사항을 저장할까요?"));
                    });
                    decision.actions(|buttons|{
                            save=theme::action(buttons,"저장",
                                theme::ButtonRole::Primary,editable&&!busy).clicked();
                            discard=theme::action(buttons,"저장 안 함",
                                theme::ButtonRole::Danger,!busy).clicked();
                        }
                    );
                });
            });
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
