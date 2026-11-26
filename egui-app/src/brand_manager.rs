//! React Classification.tsx::RegistrySettings (categories=false).
//! Only the four explicit brand edit fields live here; normalize/validate
//! and mutate catalog brands with original Rust catalog_clients::upsert_brand.
use crate::{theme,typography};
use eframe::egui::{Vec2,Align,Context,Layout,Rect,Ui};
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
                            .filter_map(Value::as_str).collect::<Vec<_>>().join(","),
                        evidence:field(row,"evidenceUrl").into(),
                        editing:Some(id),
                    };
                    self.original=self.form.clone();
                    self.error=None;
                    self.notice=None;
                }
            }
            Exit::Tab(tab)=>{self.next_tab=Some(tab);}
            Exit::Global=>{self.global_approved=true;}
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
        let card_min_height=crate::card::management_master_table_card_min_inner_height(ui);
        crate::card::measured_management_master_card(ui,|card|{
            crate::card::fill_management_primary_card(card,card_min_height);
            card.label(typography::section_heading("브랜드 관리"));
            card.add_space(theme::SPACE_8);
            // React registry-edit-form uses auto-fit columns >=240px,
            // with a right-aligned primary action at the field baseline.
            // Reserve actual bounded field columns BEFORE allocating their
            // contents. A horizontal_wrapped vertical child can claim its
            // intrinsic minimum before wrapping, growing the parent card.
            // Calculate rows from the available card width instead.
            let available=card.available_width().max(1.0);
            let gap=theme::SPACE_8;
            let field_width=theme::MANAGEMENT_SEARCH_MIN_WIDTH.min(available);
            let columns=(((available+gap)/(field_width+gap)).floor() as usize).clamp(1,5);
            let row_height=theme::SPACE_24+theme::INPUT_HEIGHT;
            for first in (0..5).step_by(columns){
                card.horizontal(|form|{
                    form.spacing_mut().item_spacing.x=gap;
                    for index in first..(first+columns).min(5){
                        form.allocate_ui_with_layout(Vec2::new(field_width,row_height),
                            Layout::top_down(Align::Min),|column|{
                                column.set_width(field_width);
                                if index==4{
                                    column.add_space(theme::SPACE_24);
                                    column.with_layout(Layout::right_to_left(Align::Center),|actions|{
                                        if crate::button::action(actions,"브랜드 저장",
                                            theme::ButtonRole::Primary,editable&&!busy).clicked(){
                                            save=true;
                                        }
                                    });
                                }else{
                                    let (label,value,hint)=match index{
                                        0=>("공식 브랜드명",&mut self.form.name,"공식 브랜드명"),
                                        1=>("한글 표기",&mut self.form.korean,"확인된 한글 표기"),
                                        2=>("검색 별칭",&mut self.form.aliases,"별칭 (쉼표 구분)"),
                                        _=>("확인 근거 URL",&mut self.form.evidence,"확인 근거 URL"),
                                    };
                                    column.label(typography::field_label(label));
                                    column.add_enabled_ui(editable&&!busy,|input|{
                                        crate::input::flexible_text_field(input,value,field_width,hint);
                                    });
                                }
                            });
                    }
                });
            }
            crate::ui_primitives::section_divider(card);
            use typography::ManagementTableCellRole as Role;
            let brands=rows(&catalog["brands"]);
            let height=(card.available_height()-theme::MANAGEMENT_TABLE_HEADER_HEIGHT).max(1.0);
            let rect=crate::table::registry_table(card,"brand-registry-list",
                ["공식 브랜드명","한글 표기","검색 별칭","관리"],
                [180.0,180.0,220.0,108.0],height,brands.len(),
                |i|self.form.editing.as_deref()==Some(field(&brands[i],"id")),
                |row,cells,i,rect|{
                    let brand=&brands[i];
                    let id=field(brand,"id");
                    let aliases=rows(&brand["aliases"]).iter().filter_map(Value::as_str)
                        .collect::<Vec<_>>().join(", ");
                    crate::table::registry_cell(row,cells[0],field(brand,"name"),Role::Text,false,(id,0));
                    crate::table::registry_cell(row,cells[1],field(brand,"korean"),Role::Text,false,(id,1));
                    crate::table::registry_cell(row,cells[2],&aliases,Role::Description,false,(id,2));
                    crate::table::registry_actions(row,cells[3],|actions|{
                        if crate::button::action(actions,"정정",theme::ButtonRole::Standard,!busy).clicked(){
                            select=Some(id.to_owned());
                        }
                    });
                    #[cfg(test)]
                    crate::ui_test_support::record("catalog data row",rect);
                    let _=rect;
                });
            if brands.is_empty(){
                crate::status_notice::management_body_state_overlay(card,rect,Some("등록된 브랜드가 없습니다."));
            }

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
        crate::popup::modal_backdrop(ctx,"brand-unsaved-dim",true);
        crate::popup::show_decision_window(ctx,"브랜드","brand-unsaved-confirm",work_area,|dialog|{
                crate::popup::decision_workspace(dialog,|decision|{
                    cancel=decision.header("브랜드","취소",!busy);
                    decision.body(|dialog|{
                        dialog.label(typography::body("브랜드 변경사항을 저장할까요?"));
                    });
                    decision.actions(|buttons|{
                            save=crate::button::action(buttons,"저장",
                                theme::ButtonRole::Primary,editable&&!busy).clicked();
                            discard=crate::button::action(buttons,"저장 안 함",
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
