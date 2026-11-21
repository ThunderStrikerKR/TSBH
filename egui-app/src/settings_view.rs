//! React SettingsView + SettingsPanel native UI. Controls, field keys,
 //! section order, dirty-tab guard and CAS saves match the original React
 //! implementation. The shared theme/components own visual tokens.
use crate::{components, gateway::BandoGateway, settings_store, theme::{self,ButtonRole}, typography};
use eframe::egui::{self,Align,Context,Layout,Ui,Vec2};
use serde_json::{json,Map,Value};
use std::{sync::{mpsc::{self,Receiver},Arc},thread};

#[derive(Copy,Clone,Eq,PartialEq)]
pub enum SettingsTab { Basic, Files, Data, System }
impl SettingsTab{
    const ALL:[Self;4]=[Self::Basic,Self::Files,Self::Data,Self::System];
    fn label(self)->&'static str {
        match self {
            Self::Basic=>"기본 설정",Self::Files=>"파일 관리",
            Self::Data=>"데이터 관리",Self::System=>"시스템"
        }
    }
}

#[derive(Clone,Copy,Eq,PartialEq)]
enum SaveKind{Regular,Scale}
type Pending=Receiver<Result<Value,String>>;

pub struct SettingsView{
    tab:SettingsTab,
    source:Value,
    draft:Value,
    loaded:bool,
    loading:Option<Pending>,
    saving:Option<(SaveKind,Pending)>,
    folder_picker:Option<(String,Receiver<Result<Option<String>,String>>)>,
    data_request:Option<(String,Pending)>,
    archive:Option<Vec<Value>>,
    archive_query:String,
    archive_open:bool,
    backups:Vec<String>,
    backup_refresh_after_create:bool,
    restore_target:Option<String>,
    restored:bool,
    requested_tab:Option<SettingsTab>,
    switch_after_save:bool,
    reload_confirmation:bool,
    pub error:Option<String>,
    notice:Option<String>,
}
fn settings(state:&Value)->&Value{
    state.get("settings").unwrap_or(&Value::Null)
}
fn field(value:&Value,key:&str)->String{
    value.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}
fn canonical(key:&str,value:Option<&Value>)->Value{
    match key{
        "uiScale"=>json!(value.and_then(Value::as_u64).unwrap_or(100)),
        "accounts"=>value.filter(|v|v.is_array())
            .cloned().unwrap_or_else(||json!([])),
        "brandAccounts"=>json!({
            "BANDOSTAND":value.and_then(|v|v.get("BANDOSTAND"))
                .and_then(Value::as_str).unwrap_or(""),
            "BANDOMALL":value.and_then(|v|v.get("BANDOMALL"))
                .and_then(Value::as_str).unwrap_or(""),
        }),
        "senderName"=>{
            let value=value.and_then(Value::as_str).filter(|v|!v.is_empty())
                .unwrap_or("반도스탠드");
            json!(value)
        },
        _=>json!(value.and_then(Value::as_str).unwrap_or("")),
    }
}
const EDIT_KEYS:[&str;13]=[
    "uiScale","operatorName","senderName","senderAddress","senderPostcode",
    "senderPhone","senderMobile","accounts","defaultAccountId","brandAccounts",
    "excelDirectory","factoryReplyDirectory","orderImageInboxDirectory"
];
fn diff(saved:&Value,edited:&Value,only_scale:bool)->(Value,Value){
    let mut changes=Map::new();
    let mut expected=Map::new();
    for key in EDIT_KEYS{
        if only_scale && key!="uiScale"{continue;}
        let before=canonical(key,saved.get(key));
        let after=canonical(key,edited.get(key));
        if before==after{continue;}
        changes.insert(key.to_owned(),after);
        expected.insert(key.to_owned(),saved.get(key).cloned().unwrap_or(Value::Null));
    }
    (Value::Object(changes),Value::Object(expected))
}
fn changed(saved:&Value,edited:&Value)->bool{
    !diff(saved,edited,false).0.as_object().is_none_or(Map::is_empty)
}
fn account_complete(account:&Value)->bool {
    ["bank","number","holder"].iter().all(|field|
        account.get(field).and_then(Value::as_str)
            .is_some_and(|value|!value.trim().is_empty())
    )
}
fn edit_input(ui:&mut Ui,draft:&mut Value,key:&str,label:&str){
    ui.label(typography::field_label(label));
    let mut current=if key=="senderName"{
        canonical(key,draft.get(key)).as_str().unwrap_or("").to_owned()
    }else{field(draft,key)};
    let placeholder=if key=="operatorName"{
        "진행 / 입금 / 발송 이력에 기록할 이름"
    }else{""};
    if components::flexible_text_field(
        ui,&mut current,ui.available_width(),placeholder
    ).changed(){
        draft[key]=json!(current);
    }
}
fn section(ui:&mut Ui,title:&str,body:impl FnOnce(&mut Ui)){
    crate::settings_help::heading(ui,title);
    ui.add_space(theme::SPACE_8);
    body(ui);
}
fn card(ui:&mut Ui,title:&str,body:impl FnOnce(&mut Ui)){
    theme::section_frame().show(ui,|content|{
        section(content,title,body);
    });
}
fn account_display(account:&Value)->String{
    format!("{} {} {}",field(account,"bank"),field(account,"number"),field(account,"holder"))
}
/// Both brand recommendation selectors share the same account master
/// and are stacked vertically for consistent settings-form flow.
fn recommendation_picker(ui:&mut Ui,draft:&mut Value,key:&str,label:&str){
    ui.label(typography::field_label(label));
    let mut selected=draft.pointer(&format!("/brandAccounts/{key}"))
        .and_then(Value::as_str).unwrap_or("").to_owned();
    let options=draft.get("accounts").and_then(Value::as_array)
        .cloned().unwrap_or_default();
    let preview=options.iter().find(|account|field(account,"id")==selected)
        .map(account_display).unwrap_or_else(||"계좌 선택".to_owned());
    egui::ComboBox::from_id_salt(("settings-recommendation",key))
        .selected_text(preview).show_ui(ui,|choices|{
            choices.selectable_value(&mut selected,String::new(),"계좌 선택");
            for account in &options{
                if account_complete(account){
                    choices.selectable_value(
                        &mut selected,field(account,"id"),account_display(account)
                    );
                }
            }
        });
    if !draft.get("brandAccounts").is_some_and(Value::is_object){
        draft["brandAccounts"]=json!({"BANDOSTAND":"","BANDOMALL":""});
    }
    draft["brandAccounts"][key]=json!(selected);
}
impl SettingsView{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self{
        let (sender,receiver)=mpsc::channel();
        thread::spawn(move||{
            let result=gateway.load_settings();
            let _=sender.send(result);
            ctx.request_repaint();
        });
        Self{
            tab:SettingsTab::Basic,source:Value::Null,draft:Value::Null,
            loaded:false,loading:Some(receiver),saving:None,
            folder_picker:None,data_request:None,
            archive:None,archive_query:String::new(),archive_open:false,
            backups:Vec::new(),backup_refresh_after_create:false,
            restore_target:None,restored:false,
            requested_tab:None,switch_after_save:false,
            reload_confirmation:false,
            error:None,notice:None
        }
    }
    pub fn take_restored(&mut self)->bool{
        std::mem::take(&mut self.restored)
    }
    pub fn dirty(&self)->bool{
        self.loaded && changed(settings(&self.source),&self.draft)
    }
    pub fn busy(&self)->bool{
        self.loading.is_some()||self.saving.is_some()||
            self.folder_picker.is_some()||self.data_request.is_some()
    }
    pub fn read_only(&self)->bool {
        self.source.get("readOnly").and_then(Value::as_bool).unwrap_or(true)
    }
    fn data_available(&self)->bool {
        self.source.get("dataAvailable").and_then(Value::as_bool)
            .unwrap_or(false)
    }
    pub fn discard(&mut self){
        self.draft=settings(&self.source).clone();
        self.error=None;
        self.requested_tab=None;
        self.switch_after_save=false;
    }
    fn data_action(
        &mut self,operation:&str,filename:Option<String>,
        gateway:Arc<dyn BandoGateway>,ctx:Context
    ){
        if self.data_request.is_some(){return;}
        crate::floating_notice::dispatch_pointer(&ctx,"settings.action");
        let (tx,rx)=mpsc::channel();
        self.data_request=Some((operation.to_owned(),rx));
        self.error=None;
        let operation=operation.to_owned();
        thread::spawn(move||{
            let result=gateway.settings_data(&operation,filename.as_deref());
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn reload(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        let (tx,rx)=mpsc::channel();
        self.loading=Some(rx);
        self.loaded=false;
        thread::spawn(move||{
            let result=gateway.load_settings();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self,ctx:&Context,gateway:Arc<dyn BandoGateway>){
        if let Some(rx)=self.loading.as_ref(){
            match rx.try_recv(){
                Ok(Ok(state))=>{
                    self.draft=settings(&state).clone();
                    self.source=state;
                    self.loading=None;self.loaded=true;self.error=None;
                    self.apply_scale(ctx);
                }
                Ok(Err(error))=>{
                    self.error=Some(error);self.loading=None;self.loaded=true;
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.error=Some("설정 조회가 중단되었습니다.".into());
                    self.loading=None;self.loaded=true;
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if let Some((kind,rx))=self.saving.as_ref(){
            match rx.try_recv(){
                Ok(Ok(state))=>{
                    let was_scale=*kind==SaveKind::Scale;
                    // A scale-only save may run while unrelated draft fields
                    // are dirty. Keep those employee edits untouched.
                    if was_scale{
                        self.source=state;
                        self.draft["uiScale"]=settings(&self.source)["uiScale"].clone();
                    }else{
                        self.source=state;
                        self.draft=settings(&self.source).clone();
                    }
                    self.saving=None;self.error=None;
                    self.notice=Some("설정을 저장했습니다.".into());
                    self.apply_scale(ctx);
                    if self.switch_after_save{
                        if let Some(next)=self.requested_tab.take(){self.tab=next;}
                        self.switch_after_save=false;
                    }
                }
                Ok(Err(error))=>{
                    self.saving=None;self.error=Some(error);
                    self.switch_after_save=false;
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.saving=None;
                    self.error=Some("설정 저장 연결이 중단되었습니다.".into());
                    self.switch_after_save=false;
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if let Some((action,receiver))=self.data_request.as_ref(){
            match receiver.try_recv(){
                Ok(Ok(payload))=>{
                    let kind=action.clone();
                    self.data_request=None;
                    match kind.as_str(){
                        "archive"=>{
                            self.archive=payload.as_array().cloned();
                            self.archive_query.clear();
                            self.archive_open=true;
                        }
                        "backups"=>{
                            self.backups=payload.as_array().cloned()
                                .unwrap_or_default().iter()
                                .filter_map(Value::as_str).map(str::to_owned).collect();
                        }
                        "backup"=>{
                            self.notice=Some(format!(
                                "백업 완료: {}",payload.as_str().unwrap_or("")
                            ));
                            self.backup_refresh_after_create=true;
                        }
                        "restore"=>{
                            self.notice=Some(
                                "백업을 복구했습니다. 주문 내역을 다시 확인하세요.".into()
                            );
                            self.restored=true;
                            self.archive=None;self.backups.clear();
                            self.reload(Arc::clone(&gateway),ctx.clone());
                        }
                        _=>{}
                    }
                }
                Ok(Err(error))=>{
                    self.data_request=None;
                    self.error=Some(error);
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.data_request=None;
                    self.error=Some("데이터 관리 연결이 중단되었습니다.".into());
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if self.backup_refresh_after_create && self.data_request.is_none(){
            self.backup_refresh_after_create=false;
            self.data_action("backups",None,Arc::clone(&gateway),ctx.clone());
        }
        if let Some((key,rx))=self.folder_picker.as_ref(){
            match rx.try_recv(){
                Ok(Ok(selected))=>{
                    if let Some(path)=selected{self.draft[key]=json!(path);}
                    self.folder_picker=None;
                }
                Ok(Err(error))=>{
                    self.error=Some(error);
                    self.folder_picker=None;
                }
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.error=Some("폴더 선택을 완료하지 못했습니다.".into());
                    self.folder_picker=None;
                }
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
    }
    fn apply_scale(&self,ctx:&Context){
        if let Some(scale)=settings(&self.source).get("uiScale").and_then(Value::as_u64){
            if matches!(scale,90|100|110|125){
                ctx.set_zoom_factor(scale as f32/100.0);
            }
        }
    }
    pub fn save(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        self.save_keys(gateway,ctx,SaveKind::Regular);
    }
    fn save_keys(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context,kind:SaveKind){
        if !self.loaded || self.busy() || self.read_only(){return;}
        let (changes,expected)=diff(
            settings(&self.source),&self.draft,kind==SaveKind::Scale
        );
        if changes.as_object().is_none_or(Map::is_empty){
            if self.switch_after_save{
                if let Some(tab)=self.requested_tab.take(){self.tab=tab;}
                self.switch_after_save=false;
            }
            return;
        }
        crate::floating_notice::dispatch_pointer(&ctx,"settings.action");
        let (tx,rx)=mpsc::channel();
        self.saving=Some((kind,rx));
        self.error=None;
        self.notice=None;
        thread::spawn(move||{
            let result=gateway.patch_settings(changes,expected);
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn request_tab(&mut self,next:SettingsTab){
        if self.tab==next || self.busy(){return;}
        if self.dirty(){self.requested_tab=Some(next);}
        else{self.tab=next;}
    }
    pub fn has_tab_confirmation(&self)->bool{
        self.requested_tab.is_some()
    }
    pub fn show_tab_confirmation(
        &mut self,ctx:&Context,area:egui::Rect,gateway:Arc<dyn BandoGateway>
    ){
        if self.requested_tab.is_none(){return;}
        components::modal_backdrop(ctx,"settings-tab-changes-backdrop",false);
        components::decision_window(
            "저장하지 않은 변경사항","settings-tab-unsaved",area
        ).show(ctx,|ui|{
            if components::modal_header(
                ui,"저장하지 않은 변경사항","취소",!self.busy()
            ){self.requested_tab=None;}
            ui.label(typography::body(
                "현재 탭에서 수정한 설정이 있습니다. 변경사항을 저장할까요?"
            ));
            ui.add_space(theme::SPACE_12);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|actions|{
                if theme::action(actions,"저장하고 이동",ButtonRole::Primary,
                    !self.busy() && !self.read_only()).clicked(){
                    self.switch_after_save=true;
                    self.save(gateway.clone(),ctx.clone());
                }
                if theme::action(actions,"저장 안 함",ButtonRole::Standard,
                    !self.busy()).clicked(){
                    self.draft=settings(&self.source).clone();
                    if let Some(next)=self.requested_tab.take(){self.tab=next;}
                }
            });
        });
    }
    fn choose_folder(&mut self,key:&str,ctx:Context){
        if self.folder_picker.is_some()||self.saving.is_some()||self.read_only(){return;}
        let key=key.to_owned();
        let (tx,rx)=mpsc::channel();
        self.folder_picker=Some((key.clone(),rx));
        thread::spawn(move||{
            #[cfg(target_os="windows")]
            let picked=(||->Result<Option<String>,String>{
                let Some(path)=rfd::FileDialog::new()
                    .set_title("폴더 선택").pick_folder() else{
                    return Ok(None);
                };
                let path=std::fs::canonicalize(&path)
                    .map_err(|_|"선택한 폴더에 접근할 수 없습니다.".to_owned())?;
                if !path.is_dir(){
                    return Err("파일이 아닌 폴더를 선택해야 합니다.".into());
                }
                if key=="excelDirectory"{
                    // The original Tauri choose_excel_directory creates
                    // these five top-level business folders when accepted.
                    for name in [
                        "공장 발송","매장 발송","공장 회신","상품 관리","거래처 관리"
                    ]{
                        let child=path.join(name);
                        std::fs::create_dir_all(&child)
                            .map_err(|e|format!("{name} 보관 폴더 생성 실패: {e}"))?;
                        let resolved=std::fs::canonicalize(&child)
                            .map_err(|_|"보관함 폴더를 확인할 수 없습니다.".to_owned())?;
                        if !resolved.starts_with(&path){
                            return Err("Excel 하위 폴더가 보관함 밖에 있습니다.".into());
                        }
                    }
                }
                Ok(Some(path.to_string_lossy().into_owned()))
            })();
            #[cfg(not(target_os="windows"))]
            let picked:Result<Option<String>,String>=Ok(None);
            let _=tx.send(picked);
            ctx.request_repaint();
        });
    }
    fn open_folder(&mut self,path:&str){
        if !std::path::Path::new(path).is_dir(){
            self.error=Some("설정된 보관함 폴더를 찾을 수 없습니다.".into());
            return;
        }
        #[cfg(target_os="windows")]
        {
            if std::process::Command::new("explorer").arg(path)
                .spawn().is_err(){
                self.error=Some("보관함 폴더를 열 수 없습니다.".into());
            }
        }
        #[cfg(not(target_os="windows"))]
        {self.error=Some("폴더 열기는 Windows 환경에서 사용할 수 있습니다.".into());}
    }
    pub fn show_page(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>){
        let page_origin_y=ui.cursor().top();
        self.poll(ui.ctx(),Arc::clone(&gateway));
        crate::floating_notice::sync(
            ui.ctx(),"settings.result","settings.action",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"settings.error","settings.action",
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        components::management_page_title_top_gap(ui);
        components::management_top_content(ui,|top|{
            top.label(typography::page_heading("설정"));
        });
        ui.add_space(theme::SPACE_8);
        let selected_tab=components::management_top_content(ui,|top|
            components::management_tabs(top,&[
                ("기본 설정",self.tab==SettingsTab::Basic,true),
                ("파일 관리",self.tab==SettingsTab::Files,true),
                ("데이터 관리",self.tab==SettingsTab::Data,true),
                ("시스템",self.tab==SettingsTab::System,true),
            ])
        );
        if let Some(tab)=selected_tab {
            self.request_tab(SettingsTab::ALL[tab]);
        }
        ui.add_space(theme::SPACE_16);
        if let Some(error)=self.error.as_deref(){
            // React SettingsPanel conflict recovery: the employee can
            // reopen the saved version after acknowledging draft loss.
            let conflict=error.contains("다른 화면에서 바뀌었습니다")
                || error.contains("다른 작업에서 변경된 설정")
                || error.contains("설정 충돌");
            // Suppress unsolicited error prose; retain the targeted
            // conflict recovery action and its existing validation state.
            if conflict && components::management_top_content(ui,|top|{
                theme::action(
                    top,"저장된 설정 불러오기",ButtonRole::Standard,
                    !self.busy()
                ).clicked()
            }){
                self.reload_confirmation=true;
            }
            ui.add_space(theme::SPACE_8);
        }
        if !self.loaded{
            ui.label(typography::caption("설정을 불러오는 중입니다."));
            return;
        }
        if self.source.is_null(){return;}
        // Settings may contain multiple cards. Align the first card's top
        // without forcing any one section to fill or hide later sections.
        components::align_management_primary_card(ui,page_origin_y);
        egui::ScrollArea::vertical()
            .id_salt(("settings-view",self.tab.label()))
            .auto_shrink([false,false])
            .show(ui,|scroll|{
                let editable=!self.read_only() && !self.busy();
                match self.tab{
                    SettingsTab::Basic=>self.show_basic(scroll,editable,Arc::clone(&gateway)),
                    SettingsTab::Files=>self.show_files(scroll,editable,Arc::clone(&gateway)),
                    SettingsTab::Data=>self.show_data(scroll,Arc::clone(&gateway)),
                    SettingsTab::System=>self.show_system(scroll),
                }
            });
    }
    fn show_basic(&mut self,ui:&mut Ui,editable:bool,gateway:Arc<dyn BandoGateway>){
        // The React settings.card owns the fields AND their save control.
        // Only the inputs are disabled during a save, never the visible
        // action row itself; otherwise the saving state is hidden.
        theme::section_frame().show(ui,|card|{
            card.add_enabled_ui(editable,|form|{
            let available=form.available_width();
            if available>theme::SETTINGS_TOP_TWO_COL_MIN_WIDTH{
                // React settings-basic-top-grid: 1fr : 1.4fr,
                // top aligned, with exactly the shared management gap.
                form.with_layout(Layout::left_to_right(Align::Min),|row|{
                    row.spacing_mut().item_spacing.x=theme::SETTINGS_GRID_GAP_X;
                    let total=(row.available_width()-theme::SETTINGS_GRID_GAP_X).max(0.0);
                    let ratios=theme::SETTINGS_TOP_COL_RATIOS;
                    let left=total*ratios[0]/(ratios[0]+ratios[1]);
                    let right=total-left;
                    row.vertical(|col|{
                        col.set_width(left);
                        section(col,"화면 글자 크기",|cell|{
                            self.scale_selector(cell,Arc::clone(&gateway));
                        });
                    });
                    row.vertical(|col|{
                        col.set_width(right);
                        section(col,"작업 이력",|cell|{
                            edit_input(cell,&mut self.draft,"operatorName","작업자 이름");
                        });
                    });
                });
            }else{
                section(form,"화면 글자 크기",|cell|{
                    self.scale_selector(cell,Arc::clone(&gateway));
                });
                form.add_space(theme::SPACE_12);
                section(form,"작업 이력",|cell|{
                    edit_input(cell,&mut self.draft,"operatorName","작업자 이름");
                });
            }
            form.add_space(theme::SPACE_12);
            section(form,"반도스탠드 정보",|content|{
                let width=content.available_width();
                if width>theme::SETTINGS_SENDER_THREE_COL_MIN_WIDTH{
                    let ratios=theme::SETTINGS_SENDER_COL_RATIOS;
                    let sum=ratios.iter().sum::<f32>();
                    // Original CSS grid row 1: company (1.1fr) and
                    // address spanning the next 1.6fr + 0.8fr tracks.
                    content.with_layout(Layout::left_to_right(Align::Min),|row|{
                        row.spacing_mut().item_spacing.x=theme::SETTINGS_GRID_GAP_X;
                        let total=(row.available_width()-theme::SETTINGS_GRID_GAP_X).max(0.0);
                        let name_width=total*ratios[0]/sum;
                        row.vertical(|col|{
                            col.set_width(name_width);
                            edit_input(col,&mut self.draft,"senderName","상호명");
                        });
                        row.vertical(|col|{
                            col.set_width(total-name_width);
                            edit_input(col,&mut self.draft,"senderAddress","주소");
                        });
                    });
                    content.add_space(theme::SPACE_8);
                    // Original CSS grid row 2: postcode / phone / mobile.
                    content.with_layout(Layout::left_to_right(Align::Min),|row|{
                        row.spacing_mut().item_spacing.x=theme::SETTINGS_GRID_GAP_X;
                        let total=(row.available_width()-2.0*theme::SETTINGS_GRID_GAP_X).max(0.0);
                        for (index,(key,label)) in [
                            ("senderPostcode","우편번호"),
                            ("senderPhone","전화번호"),
                            ("senderMobile","휴대폰 번호")
                        ].into_iter().enumerate(){
                            row.vertical(|col|{
                                col.set_width(total*ratios[index]/sum);
                                edit_input(col,&mut self.draft,key,label);
                            });
                        }
                    });
                }else if width>theme::SETTINGS_TOP_TWO_COL_MIN_WIDTH{
                    // Original <=1120px CSS: address spans both tracks.
                    content.columns(2,|cols|{
                        edit_input(&mut cols[0],&mut self.draft,"senderName","상호명");
                    });
                    content.add_space(theme::SPACE_8);
                    edit_input(content,&mut self.draft,"senderAddress","주소");
                    content.add_space(theme::SPACE_8);
                    content.columns(2,|cols|{
                        edit_input(&mut cols[0],&mut self.draft,"senderPostcode","우편번호");
                        edit_input(&mut cols[1],&mut self.draft,"senderPhone","전화번호");
                    });
                    content.columns(2,|cols|{
                        edit_input(&mut cols[0],&mut self.draft,"senderMobile","휴대폰 번호");
                    });
                }else{
                    edit_input(content,&mut self.draft,"senderName","상호명");
                    edit_input(content,&mut self.draft,"senderAddress","주소");
                    edit_input(content,&mut self.draft,"senderPostcode","우편번호");
                    edit_input(content,&mut self.draft,"senderPhone","전화번호");
                    edit_input(content,&mut self.draft,"senderMobile","휴대폰 번호");
                }
            });
            form.add_space(theme::SPACE_12);
            section(form,"입금 계좌",|content|{
                self.show_accounts(content);
            });
            });
            self.save_bar(card,gateway);
        });
    }
    fn scale_selector(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>){
        ui.label(typography::field_label("전체 UI 크기"));
        let mut scale=self.draft.get("uiScale").and_then(Value::as_u64)
            .unwrap_or(100);
        egui::ComboBox::from_id_salt("settings-ui-scale")
            .selected_text(format!("{scale}%"))
            .show_ui(ui,|choices|{
                for candidate in [90u64,100,110,125]{
                    choices.selectable_value(
                        &mut scale,candidate,format!("{candidate}%")
                    );
                }
            });
        if self.draft.get("uiScale").and_then(Value::as_u64)!=Some(scale){
            self.draft["uiScale"]=json!(scale);
            // React persists only uiScale immediately. Unrelated edits
            // remain dirty, and the per-field CAS is preserved.
            self.save_keys(gateway,ui.ctx().clone(),SaveKind::Scale);
        }
        if self.dirty(){
            ui.label(typography::caption("저장하지 않은 설정이 있습니다."));
        }
    }
    fn show_accounts(&mut self,ui:&mut Ui){
        let mut accounts=self.draft.get("accounts")
            .and_then(Value::as_array).cloned().unwrap_or_default();
        let mut delete=None;
        for (index,account) in accounts.iter_mut().enumerate(){
            // Match the original React account-row four-track grid.
            // The delete button is aligned with the input controls,
            // not placed above a separate three-column form.
            if ui.available_width()>=theme::SETTINGS_ACCOUNT_ROW_MIN_WIDTH{
                ui.horizontal(|row|{
                    row.spacing_mut().item_spacing.x=theme::SPACE_8;
                    let usable=(row.available_width()
                        -theme::SETTINGS_ACCOUNT_DELETE_WIDTH
                        -3.0*theme::SPACE_8).max(0.0);
                    let weights=theme::SETTINGS_ACCOUNT_COL_RATIOS;
                    let total=weights.iter().sum::<f32>();
                    for (column,(key,label)) in [
                        ("bank","은행명"),("number","계좌번호"),
                        ("holder","예금주")
                    ].into_iter().enumerate(){
                        let width=usable*weights[column]/total;
                        row.allocate_ui_with_layout(
                            Vec2::new(width,theme::SPACE_24),
                            Layout::top_down(Align::Min),|field_ui|{
                                field_ui.set_width(width);
                                edit_input(field_ui,account,key,label);
                            }
                        );
                    }
                    row.allocate_ui_with_layout(
                        Vec2::new(theme::SETTINGS_ACCOUNT_DELETE_WIDTH,
                            theme::SPACE_24),
                        Layout::top_down(Align::Min),|delete_ui|{
                            // Match the bottom alignment of input control,
                            // after its field label.
                            delete_ui.add_space(theme::SPACE_24);
                            if theme::action_full_width(
                                delete_ui,"계좌 삭제",ButtonRole::Danger,true
                            ).clicked(){
                                delete=Some(index);
                            }
                        }
                    );
                });
            }else{
                // Narrow cards must never squeeze an account number
                // down to character-by-character wrapping.
                ui.label(typography::field_label(&format!("계좌 {}",index+1)));
                edit_input(ui,account,"bank","은행명");
                edit_input(ui,account,"number","계좌번호");
                edit_input(ui,account,"holder","예금주");
                if theme::action(
                    ui,"계좌 삭제",ButtonRole::Danger,true
                ).clicked(){
                    delete=Some(index);
                }
            }
            ui.add_space(theme::SPACE_8);
            components::soft_divider(ui);
        }
        if let Some(index)=delete{
            let id=field(&accounts[index],"id");
            let used=self.source.get("usedAccounts")
                .and_then(Value::as_array)
                .is_some_and(|ids|ids.iter().any(|v|v.as_str()==Some(id.as_str())));
            if used{
                self.error=Some("이 계좌를 사용하는 거래처가 있습니다. 거래처 설정을 변경한 후 삭제하세요.".into());
            }else{
                accounts.remove(index);
                let brand=self.draft.get_mut("brandAccounts");
                if let Some(map)=brand.and_then(Value::as_object_mut){
                    for key in ["BANDOSTAND","BANDOMALL"]{
                        if map.get(key).and_then(Value::as_str)==Some(id.as_str()){
                            map.insert(key.into(),json!(""));
                        }
                    }
                }
                if field(&self.draft,"defaultAccountId")==id{
                    self.draft["defaultAccountId"]=json!("");
                }
            }
        }
        if theme::action(ui,"＋ 계좌 추가",ButtonRole::Standard,true).clicked(){
            accounts.push(json!({
                "id":settings_store::new_account_id(),
                "bank":"","number":"","holder":""
            }));
        }
        self.draft["accounts"]=json!(accounts);
        ui.add_space(theme::SPACE_12);
        let recommendations=[
            ("BANDOSTAND","반도스탠드 추천 계좌"),
            ("BANDOMALL","반도몰 추천 계좌"),
        ];
        for (index,(key,label)) in recommendations.into_iter().enumerate(){
            if index>0{
                ui.add_space(theme::SPACE_12);
            }
            recommendation_picker(ui,&mut self.draft,key,label);
        }
    }
    fn show_files(&mut self,ui:&mut Ui,editable:bool,gateway:Arc<dyn BandoGateway>){
        let sections=[
            ("excelDirectory","Excel 보관함 위치",
              "설정되지 않음","보관함 폴더 선택"),
            ("factoryReplyDirectory","공장 회신 자동 수신 폴더",
              "미설정: 수동 Excel 업로드만 사용","수신 폴더 선택"),
            ("orderImageInboxDirectory","발주 이미지 자동 수신 폴더",
              "미설정: 이미지 폴더 자동 수신 중지","수신 폴더 선택"),
        ];
        // React files tab has ONE section.card with three consecutive
        // information groups separated by whitespace, not three boxes.
        theme::section_frame().show(ui,|form|{
        for (index,(key,title,empty,button)) in sections.iter().enumerate(){
            if index>0{form.add_space(theme::SPACE_12);}
            section(form,title,|area|{
                let value=field(&self.draft,key);
                area.label(typography::body(
                    if value.is_empty(){empty}else{&value}
                ));
                area.add_space(theme::SPACE_8);
                area.horizontal_wrapped(|buttons|{
                    if theme::action(
                        buttons,button,ButtonRole::Standard,
                        editable&&self.folder_picker.is_none()
                    ).clicked(){
                        self.choose_folder(key,buttons.ctx().clone());
                    }
                    // React's "보관함 열기" uses the SAVED Excel path
                    // (state.settings), not a newly chosen unsaved draft.
                    let saved_excel=field(settings(&self.source),"excelDirectory");
                    if index==0 && !saved_excel.is_empty(){
                        if theme::action(
                            buttons,"보관함 열기",ButtonRole::Standard,true
                        ).clicked(){
                            self.open_folder(&saved_excel);
                        }
                    }else if index>0 && !value.is_empty(){
                        if theme::action(buttons,"자동 수신 중지",
                            ButtonRole::Quiet,editable).clicked(){
                            self.draft[*key]=json!("");
                        }
                    }
                });
            });
        }
        self.save_bar(form,gateway);
        });
    }
    fn save_bar(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>){
        ui.add_space(theme::SPACE_12);
        ui.horizontal(|bar|{
            if theme::action(
                bar,if self.saving.is_some(){"저장 중…"}else{"설정 저장"},
                ButtonRole::Primary,!self.read_only()&&!self.busy()
            ).clicked(){
                self.save(gateway.clone(),bar.ctx().clone());
            }
        });
    }
    fn show_data(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>){
        let business=self.source.get("mode").and_then(Value::as_str)==Some("business");
        let reading=self.data_request.is_some()||self.loading.is_some();
        card(ui,"접수 원문 감사 보관함",|panel|{
            if theme::action(panel,
                if self.data_request.as_ref().is_some_and(|(op,_)|op=="archive"){
                    "기록 조회 중…"
                }else{"감사 기록 열기"},ButtonRole::Standard,
                business&&self.data_available()&&!reading
            ).clicked(){
                self.data_action("archive",None,Arc::clone(&gateway),panel.ctx().clone());
            }
        });
        ui.add_space(theme::SPACE_12);
        card(ui,"데이터 백업 및 복구",|panel|{
            panel.horizontal_wrapped(|actions|{
                if theme::action(actions,"지금 백업",ButtonRole::Standard,
                    business&&self.data_available()&&!self.read_only()&&!reading).clicked(){
                    self.data_action(
                        "backup",None,Arc::clone(&gateway),actions.ctx().clone()
                    );
                }
                if theme::action(actions,"백업 목록",ButtonRole::Standard,
                    business&&self.data_available()&&!reading).clicked(){
                    self.data_action(
                        "backups",None,Arc::clone(&gateway),actions.ctx().clone()
                    );
                }
            });
            for filename in self.backups.clone(){
                components::soft_divider(panel);
                let name=std::path::Path::new(&filename).file_name()
                    .and_then(|p|p.to_str()).unwrap_or(&filename);
                let policy=if name.starts_with("monthly-"){
                    "월간 최초 자동 백업 (영구 보관)"
                }else if name.starts_with("auto-"){
                    "일반 자동 백업 (30일 보관)"
                }else if name.starts_with("pre-restore-"){
                    "복원 직전 안전 백업 (영구 보관)"
                }else if name.starts_with("manual-"){
                    "수동 백업 (영구 보관)"
                }else{
                    "기존 백업 (자동 삭제 안 함)"
                };
                panel.horizontal_wrapped(|row|{
                    row.vertical(|name_column|{
                        name_column.label(typography::body(name));
                        name_column.label(typography::caption(policy));
                    });
                    if theme::action(row,"이 백업으로 복구",
                        ButtonRole::Standard,
                        !self.read_only()&&!reading).clicked(){
                        self.restore_target=Some(filename.clone());
                    }
                });
            }
        });
        ui.add_space(theme::SPACE_12);
        // React shows the heading and its on-demand help, not a filesystem
        // debug path in the regular settings body.
        card(ui,"업무 데이터 저장",|_panel|{});
    }
    pub fn has_data_modal(&self)->bool{
        self.archive_open||self.restore_target.is_some()||self.reload_confirmation
    }
    pub fn show_data_modals(
        &mut self,ctx:&Context,area:egui::Rect,gateway:Arc<dyn BandoGateway>
    ){
        if self.reload_confirmation{
            components::modal_backdrop(ctx,"settings-reload-backdrop",false);
            components::decision_window(
                "저장된 설정 불러오기","settings-reload-confirm",area
            ).show(ctx,|ui|{
                if components::modal_header(
                    ui,"저장된 설정 불러오기","취소",true
                ){self.reload_confirmation=false;}
                ui.label(typography::body(
                    "입력 중인 설정을 닫고 저장된 설정을 다시 불러올까요?"
                ));
                ui.add_space(theme::SPACE_12);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|actions|{
                    if theme::action(
                        actions,"불러오기",ButtonRole::Primary,true
                    ).clicked(){
                        self.reload_confirmation=false;
                        self.reload(Arc::clone(&gateway),ctx.clone());
                    }
                });
            });
            return;
        }
        if self.archive_open{
            components::modal_backdrop(ctx,"settings-audit-backdrop",false);
            components::modal_window(
                "접수 원문 감사 보관함","settings-audit-records",area
            ).default_width(720.0).show(ctx,|ui|{
                if components::modal_header(
                    ui,"접수 원문 감사 보관함","닫기",true
                ){self.archive_open=false;}
                let search_width=ui.available_width();
                components::flexible_text_field(
                    ui,&mut self.archive_query,search_width,
                    "원문 / 담당자 / 주문 ID 검색"
                );
                let matches=self.archive.as_ref().into_iter().flatten()
                    .filter(|row|self.archive_query.trim().is_empty() ||
                        row.to_string().to_lowercase().contains(
                            &self.archive_query.trim().to_lowercase()
                        )).collect::<Vec<_>>();
                ui.label(typography::caption(&format!("{}건 조회",matches.len())));
                egui::ScrollArea::vertical().id_salt("settings-audit-records-scroll")
                    .max_height(area.height()*0.62)
                    .show(ui,|entries|{
                        for row in matches{
                            let id=field(row,"id");
                            let order_id=field(row,"orderId");
                            let record=&row["record"];
                            let at=field(record,"at");
                            let actor=record.get("actor").and_then(Value::as_str)
                                .filter(|v|!v.is_empty()).unwrap_or("담당자 미확인");
                            let correction=id.starts_with("correction:")||
                                id.starts_with("legacy-correction:");
                            let unverified=record.get("verified")==Some(&Value::Bool(false));
                            let title=format!(
                                "{} / {} / {} / {}{}",
                                order_id,at,actor,
                                if correction{"원문 정정"}else{"최초 접수"},
                                if unverified{" (과거 자료 / 독립 감사 미확인)"}else{""}
                            );
                            egui::CollapsingHeader::new(title)
                                .icon(components::paint_collapsing_header_icon)
                                .show(entries,|detail|{
                                if correction{
                                    detail.label(typography::field_label(
                                        "정정 전 작업용 원문"
                                    ));
                                    detail.add(egui::Label::new(typography::body(
                                        &field(record,"before")
                                    )).selectable(true));
                                    detail.label(typography::field_label(
                                        "정정 후 작업용 원문"
                                    ));
                                    detail.add(egui::Label::new(typography::body(
                                        &field(record,"after")
                                    )).selectable(true));
                                    detail.label(typography::caption(&format!(
                                        "정정 사유: {}",
                                        record.get("reason").and_then(Value::as_str)
                                            .filter(|v|!v.is_empty())
                                            .unwrap_or("과거 기록 / 사유 미확인")
                                    )));
                                }else{
                                    detail.label(typography::field_label(
                                        "최초 접수 원문"
                                    ));
                                    detail.add(egui::Label::new(typography::body(
                                        &field(record,"original")
                                    )).selectable(true));
                                }
                            });
                        }
                    });
              });
        }
        if let Some(filename)=self.restore_target.clone(){
            components::modal_backdrop(ctx,"settings-restore-backdrop",false);
            components::decision_window(
                "데이터 복구 확인","settings-restore-confirm",area
            ).show(ctx,|ui|{
                if components::modal_header(
                    ui,"데이터 복구 확인","취소",!self.busy()
                ){self.restore_target=None;}
                ui.label(typography::body(
                    "선택한 시점의 전체 데이터베이스로 복원할까요? 현재 데이터는 먼저 안전 백업으로 보관합니다. 주문과 감사 기록도 선택한 시점으로 되돌아갑니다."
                ));
                ui.add_space(theme::SPACE_12);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|actions|{
                    if theme::action(actions,"이 백업으로 복구",ButtonRole::Primary,
                        self.data_available()&&!self.read_only()&&!self.busy()).clicked(){
                        self.restore_target=None;
                        self.data_action(
                            "restore",Some(filename),
                            Arc::clone(&gateway),ctx.clone()
                        );
                    }
                });
            });
        }
    }
    fn show_system(&mut self,ui:&mut Ui){
        card(ui,"프로그램 정보",|area|{
            let config:Value=serde_json::from_str(
                include_str!("../../src-tauri/tauri.conf.json")
            ).unwrap_or(Value::Null);
            let version=config.get("version").and_then(Value::as_str)
                .unwrap_or("확인 불가");
            area.label(typography::body(&format!("현재 버전: v{version}")));
            area.label(typography::body(&format!(
                "빌드 ID: {}",
                option_env!("BANDO_BUILD_ID").unwrap_or("미지정")
            )));
        });
    }
}
