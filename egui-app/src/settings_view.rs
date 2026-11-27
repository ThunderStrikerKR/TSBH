//! React SettingsView + SettingsPanel native UI. Controls, field keys,
 //! section order, dirty-tab guard and CAS saves match the original React
 //! implementation. The shared theme/components own visual tokens.
use crate::{gateway::BandoGateway, settings_store, theme::{self,ButtonRole}, typography};
use crate::card::UiWidthPolicyExt;
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
    tab_snapshots:crate::view_state::NavigationSnapshots<4>,
    source:Value,
    draft:Value,
    loaded:bool,
    loading:crate::screen_refresh::ReadRefresh<Value>,
    saving:Option<(SaveKind,Pending)>,
    folder_picker:Option<(String,Receiver<Result<Option<String>,String>>)>,
    data_request:Option<(String,Pending)>,
    archive:Option<Vec<Value>>,
    archive_cache:crate::screen_refresh::ViewProjectionCache<String,Vec<usize>>,
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
/// A Settings text field keeps the shared 36pt input style but not the
/// available width of an entire management card.
fn edit_input_width(
    ui:&mut Ui,draft:&mut Value,key:&str,label:&str,width:f32
){
    let mut current=if key=="senderName"{
        canonical(key,draft.get(key)).as_str().unwrap_or("").to_owned()
    }else{field(draft,key)};
    let response=crate::popup::modal_labeled_row(ui,label,|line|{
        let width=width.min(line.available_width());
        crate::input::flexible_text_field(line,&mut current,width,"")
    });
    if response.changed(){draft[key]=json!(current);}
}
fn section(ui:&mut Ui,title:&str,body:impl FnOnce(&mut Ui)){
    crate::settings_help::heading(ui,title);
    ui.add_space(theme::SPACE_8);
    body(ui);
}
fn card(ui:&mut Ui,title:&str,body:impl FnOnce(&mut Ui)){
    crate::card::measured_management_section_card(ui,|content|{
        section(content,title,body);
    });
}
fn account_display(account:&Value)->String{
    format!("{} {} {}",field(account,"bank"),field(account,"number"),field(account,"holder"))
}
/// Both brand recommendation selectors share the same account master
/// and are stacked vertically for consistent settings-form flow.
fn recommendation_picker(ui:&mut Ui,draft:&mut Value,key:&str,label:&str){
    let mut selected=draft.pointer(&format!("/brandAccounts/{key}"))
        .and_then(Value::as_str).unwrap_or("").to_owned();
    let options=draft.get("accounts").and_then(Value::as_array)
        .cloned().unwrap_or_default();
    let preview=options.iter().find(|account|field(account,"id")==selected)
        .map(account_display).unwrap_or_else(||"계좌 선택".to_owned());
    crate::popup::modal_labeled_row_with_width(ui,label,theme::MODAL_FORM_WIDE_LABEL_WIDTH,|line|{
    crate::dropdown::management_dropdown(
        line,crate::dropdown::DropdownCaption::new(&preview,
            selected.is_empty()||!options.iter().any(|account|field(account,"id")==selected)),
        line.available_width().min(theme::SETTINGS_ACCOUNT_PICKER_MAX_WIDTH),
        true,|choices|{
            crate::dropdown::management_dropdown_option(
                choices,&mut selected,String::new(),"계좌 선택"
            );
            for account in &options{
                if account_complete(account){
                    crate::dropdown::management_dropdown_option(
                        choices,&mut selected,field(account,"id"),
                        &account_display(account)
                    );
                }
            }
        }
    );
    });
    if !draft.get("brandAccounts").is_some_and(Value::is_object){
        draft["brandAccounts"]=json!({"BANDOSTAND":"","BANDOMALL":""});
    }
    draft["brandAccounts"][key]=json!(selected);
}
fn settings_title_row(
    ui:&mut Ui,conflict:bool,restore_enabled:bool
)->(bool,Option<egui::Rect>,egui::Rect,f32){
    let mut restore=false;
    let mut restore_rect=None;
    let mut title_rect=egui::Rect::NOTHING;
    let mut content_right=0.0;
    crate::ui_layout::management_page_title_top_gap(ui);
    crate::ui_layout::management_top_content(ui,|top|{
        let width=top.available_width().max(1.0);
        content_right=top.available_rect_before_wrap().right();
        let row=top.allocate_ui_with_layout(
            Vec2::new(width,theme::ACTION_STANDARD_HEIGHT),
            Layout::left_to_right(Align::Center),
            |heading|{
                heading.label(typography::page_heading("설정"));
                if conflict{
                    heading.with_layout(Layout::right_to_left(Align::Center),|actions|{
                        let response=crate::button::action(
                            actions,"저장된 설정 불러오기",
                            ButtonRole::Standard,restore_enabled
                        );
                        restore_rect=Some(response.rect);
                        restore=response.clicked();
                    });
                }
            }
        );
        title_rect=row.response.rect;
    });
    (restore,restore_rect,title_rect,content_right)
}

impl SettingsView{
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self{
        let mut loading=crate::screen_refresh::ReadRefresh::default();
        loading.submit_required(ctx,move||gateway.load_settings());
        Self{
            tab:SettingsTab::Basic,tab_snapshots:crate::view_state::NavigationSnapshots::new(0),source:Value::Null,draft:Value::Null,
            loaded:false,loading,saving:None,
            folder_picker:None,data_request:None,
            archive:None,archive_cache:Default::default(),archive_query:String::new(),archive_open:false,
            backups:Vec::new(),backup_refresh_after_create:false,
            restore_target:None,restored:false,
            requested_tab:None,switch_after_save:false,
            reload_confirmation:false,
            error:None,notice:None
        }
    }
    pub(crate) fn suspend_navigation(&mut self,now:std::time::Instant){self.tab_snapshots.suspend(now);}
    fn sync_tab_navigation(&mut self){
        if self.tab_snapshots.visit(self.tab as usize,std::time::Instant::now())&&self.tab==SettingsTab::Data{
            self.archive_query.clear();self.archive_open=false;
        }
    }
    pub(crate) fn reset_navigation(&mut self){
        if self.dirty()||self.busy(){return;}
        self.tab=SettingsTab::Basic;self.tab_snapshots=crate::view_state::NavigationSnapshots::new(0);self.archive_query.clear();self.archive_open=false;
    }
    pub fn take_restored(&mut self)->bool{
        std::mem::take(&mut self.restored)
    }
    pub fn dirty(&self)->bool{
        self.loaded && changed(settings(&self.source),&self.draft)
    }
    pub fn busy(&self)->bool{
        self.loading.pending()||self.saving.is_some()||
            self.folder_picker.is_some()||self.data_request.is_some()
    }
    pub fn read_only(&self)->bool {
        self.loading.required() || self.source.get("readOnly").and_then(Value::as_bool).unwrap_or(true)
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
        // Keep the previous view until the requested reload is verified.
        self.loading.submit_required(ctx,move||gateway.load_settings());
    }
    pub fn poll(&mut self,ctx:&Context,gateway:Arc<dyn BandoGateway>){
        match self.loading.poll(){
            Some(Ok(state))=>{
                self.draft=settings(&state).clone();
                self.source=state;
                self.loading.verified();
                self.loaded=true;self.error=None;
                self.apply_scale(ctx);
            }
            Some(Err(error))=>{self.error=Some(error);self.loaded=true;}
            None=>{}
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
                            self.archive=payload.as_array().cloned();self.archive_cache.invalidate();
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
                            self.archive=None;self.archive_cache.invalidate();self.backups.clear();
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
        crate::popup::modal_backdrop(ctx,"settings-tab-changes-backdrop",false);
        match crate::popup::show_save_confirmation(
            ctx,area,"settings-tab-unsaved","저장하지 않은 변경사항",
            "현재 탭에서 수정한 설정이 있습니다. 변경사항을 저장할까요?",
            "저장하고 이동",!self.busy(),!self.busy()&&!self.read_only()
        ){
            crate::popup::ChangeDecision::Cancel=>self.requested_tab=None,
            crate::popup::ChangeDecision::Save=>{
                self.switch_after_save=true;
                self.save(gateway.clone(),ctx.clone());
            }
            crate::popup::ChangeDecision::Discard=>{
                self.draft=settings(&self.source).clone();
                if let Some(next)=self.requested_tab.take(){self.tab=next;}
            }
            crate::popup::ChangeDecision::Pending=>{}
        }
    }
    fn choose_folder(&mut self,key:&str,ctx:Context){
        if self.folder_picker.is_some()||self.saving.is_some()||self.read_only(){return;}
        crate::floating_notice::dispatch_pointer(&ctx,"settings.action");
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
        // Conflict recovery stays in the existing title bar instead of
        // inserting a row between tabs and content on an async error.
        let conflict=self.error.as_deref().is_some_and(|error|
            error.contains("다른 화면에서 바뀌었습니다")
                ||error.contains("다른 작업에서 변경된 설정")
                ||error.contains("설정 충돌")
        );
        let (reload,_,_,_)=settings_title_row(
            ui,conflict,!self.busy()
        );
        if reload{self.reload_confirmation=true;}
        ui.add_space(theme::SPACE_8);
        let selected_tab=crate::ui_layout::management_top_content(ui,|top|
            crate::button::management_tabs(top,&[
                ("기본 설정",self.tab==SettingsTab::Basic,true),
                ("파일 관리",self.tab==SettingsTab::Files,true),
                ("데이터 관리",self.tab==SettingsTab::Data,true),
                ("시스템",self.tab==SettingsTab::System,true),
            ])
        );
        if let Some(tab)=selected_tab {
            self.request_tab(SettingsTab::ALL[tab]);
        }
        ui.add_space(theme::SPACE_12);
        self.sync_tab_navigation();
        ui.push_id(("settings-navigation",self.tab as usize,self.tab_snapshots.epoch(self.tab as usize)),|ui|{
        if !self.loaded || self.source.is_null(){
            if self.tab!=SettingsTab::Basic{
                crate::card::align_management_primary_card(ui,page_origin_y);
            }
            crate::card::management_pending_primary_card(ui);
            return;
        }
        // Basic needs no KPI-alignment spacer above its compact cards.
        // Other management tabs retain their master-card alignment.
        if self.tab!=SettingsTab::Basic{
            crate::card::align_management_primary_card(ui,page_origin_y);
        }
        let editable=!self.read_only() && !self.busy();
        if self.tab==SettingsTab::Basic{
            self.show_basic(ui,editable,Arc::clone(&gateway));
        }else{
            crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Standard);
            crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                .id_salt(("settings-view",self.tab.label()))
                .auto_shrink([false,false])
                .show(ui,|scroll|{
                    match self.tab{
                        SettingsTab::Files=>self.show_files(
                            scroll,editable,Arc::clone(&gateway)
                        ),
                        SettingsTab::Data=>self.show_data(
                            scroll,Arc::clone(&gateway)
                        ),
                        SettingsTab::System=>self.show_system(scroll),
                        SettingsTab::Basic=>unreachable!(),
                    }
                });
        }
        });
    }
    /// A compact two-pane basic page: work/sender information on the left,
    /// scalable bank accounts on the right. No outer scrolling is introduced.
    fn show_basic(&mut self,ui:&mut Ui,editable:bool,gateway:Arc<dyn BandoGateway>){
        let available_width=ui.available_width();
        let available_height=ui.available_height();
        if available_width>=theme::SETTINGS_BASIC_SPLIT_MIN_WIDTH{
            let left_width=theme::SETTINGS_BASIC_LEFT_PANE_WIDTH;
            let right_width=crate::card::remaining_column_width(available_width,left_width,theme::SPACE_12).max(1.0);
            ui.horizontal(|row|{
                crate::ui_layout::apply_spacing_role(row,crate::ui_layout::SpacingRole::FormColumns);
                row.vertical(|left|{
                    crate::card::apply_width_policy(left,crate::card::CardWidthPolicy::Fixed,left_width);
                    self.show_work_settings(left,editable,Arc::clone(&gateway));
                    left.add_space(theme::SPACE_12);
                    self.show_sender_settings(left,editable);
                });
                row.vertical(|right|{
                    crate::card::apply_width_policy(right,crate::card::CardWidthPolicy::Fixed,right_width);
                    self.show_accounts(
                        right,editable,
                        (available_height-theme::SETTINGS_BASIC_SAVE_RESERVE).max(0.0)
                    );
                });
            });
        }else{
            self.show_work_settings(ui,editable,Arc::clone(&gateway));
            ui.add_space(theme::SPACE_12);
            self.show_sender_settings(ui,editable);
            ui.add_space(theme::SPACE_12);
            let remaining=(ui.available_height()
                -theme::SETTINGS_BASIC_SAVE_RESERVE).max(0.0);
            self.show_accounts(ui,editable,remaining);
        }
        self.save_bar(ui,gateway);
    }
    fn show_work_settings(
        &mut self,ui:&mut Ui,editable:bool,gateway:Arc<dyn BandoGateway>
    ){
        crate::card::measured_management_section_card(ui,|card|{
            card.add_enabled_ui(editable,|form|{
                section(form,"작업 환경",|content|{
                    crate::popup::modal_form_row_spacing(content);
                    self.scale_selector(content,Arc::clone(&gateway));
                    edit_input_width(content,&mut self.draft,"operatorName","작업자 이름",theme::SETTINGS_OPERATOR_FIELD_WIDTH);
                });
            });
        });
    }
    fn show_sender_settings(&mut self,ui:&mut Ui,editable:bool){
        crate::card::measured_management_section_card(ui,|card|{
            card.add_enabled_ui(editable,|form|{
                section(form,"반도스탠드 정보",|content|{
                    crate::popup::modal_form_row_spacing(content);
                    for (key,label,width) in [
                        ("senderName","상호명",theme::SETTINGS_SENDER_NAME_WIDTH),
                        ("senderPostcode","우편번호",theme::SETTINGS_SENDER_POSTCODE_WIDTH),
                        ("senderAddress","주소",theme::SETTINGS_SENDER_ADDRESS_WIDTH),
                        ("senderPhone","전화번호",theme::SETTINGS_SENDER_PHONE_WIDTH),
                        ("senderMobile","휴대폰 번호",theme::SETTINGS_SENDER_PHONE_WIDTH),
                    ]{
                        edit_input_width(content,&mut self.draft,key,label,width);
                    }
                });
            });
        });
    }
    fn scale_selector(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>){
        let mut scale=self.draft.get("uiScale").and_then(Value::as_u64)
            .unwrap_or(100);
        crate::popup::modal_labeled_row(ui,"전체 UI 크기",|line|{
        crate::dropdown::management_dropdown(
            line,&format!("{scale}%"),
            theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH.min(line.available_width()),
            !self.read_only()&&!self.busy(),|choices|{
                for candidate in [90u64,100,110,125]{
                    crate::dropdown::management_dropdown_option(
                        choices,&mut scale,candidate,&format!("{candidate}%")
                    );
                }
            }
        );
        });
        if self.draft.get("uiScale").and_then(Value::as_u64)!=Some(scale){
            self.draft["uiScale"]=json!(scale);
            // React persists only uiScale immediately. Unrelated edits
            // remain dirty, and the per-field CAS is preserved.
            self.save_keys(gateway,ui.ctx().clone(),SaveKind::Scale);
        }
    }
    fn show_accounts(&mut self,ui:&mut Ui,editable:bool,card_budget:f32){
        crate::card::measured_management_section_card(ui,|card|{
            section(card,"입금 계좌",|content|{
                content.add_enabled_ui(editable,|form|{
                    let wide_recommendations=form.available_width()
                        >=2.0*(theme::SETTINGS_ACCOUNT_PICKER_MAX_WIDTH+
                            theme::MODAL_FORM_WIDE_LABEL_WIDTH+theme::SPACE_8)+theme::SPACE_12;
                    let fixed_height=theme::SETTINGS_ACCOUNT_PANEL_FIXED_HEIGHT
                        +if wide_recommendations{0.0}
                         else{theme::SETTINGS_ACCOUNT_STACKED_EXTRA_HEIGHT};
                    let list_height=(card_budget-fixed_height).clamp(
                        theme::SETTINGS_ACCOUNT_LIST_MIN_HEIGHT,
                        theme::SETTINGS_ACCOUNT_LIST_MAX_HEIGHT
                    );
                    crate::scrollbar::apply_management_scroll_role(form,crate::scrollbar::ManagementScrollRole::Standard);
                    let _account_viewport=crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                        .id_salt("settings-bank-account-list")
                        .max_height(list_height)
                        .auto_shrink([false,true])
                        .show(form,|rows|{
                            self.show_account_rows(rows);
                        });
                    #[cfg(test)]
                    crate::ui_test_support::record("account viewport",_account_viewport.inner_rect);
                    form.add_space(theme::SPACE_8);
                    if crate::button::action(
                        form,"＋ 계좌 추가",ButtonRole::Standard,true
                    ).clicked(){
                        let mut accounts=self.draft.get("accounts")
                            .and_then(Value::as_array).cloned()
                            .unwrap_or_default();
                        accounts.push(json!({
                            "id":settings_store::new_account_id(),
                            "bank":"","number":"","holder":""
                        }));
                        self.draft["accounts"]=json!(accounts);
                    }
                    crate::ui_primitives::section_divider(form);
                    let recommendations=[
                        ("BANDOSTAND","반도스탠드 추천 계좌"),
                        ("BANDOMALL","반도몰 추천 계좌")
                    ];
                    if wide_recommendations{
                        form.horizontal(|row|{
                            crate::ui_layout::apply_spacing_role(row,crate::ui_layout::SpacingRole::FormColumns);
                            for (key,label) in recommendations{
                                row.vertical(|col|{
                                    col.apply_shared_width(crate::card::CardWidthPolicy::Fixed,theme::SETTINGS_ACCOUNT_PICKER_MAX_WIDTH+
                                        theme::MODAL_FORM_WIDE_LABEL_WIDTH+theme::SPACE_8);
                                    recommendation_picker(
                                        col,&mut self.draft,key,label
                                    );
                                });
                            }
                        });
                    }else{
                        for (index,(key,label)) in recommendations.into_iter().enumerate(){
                            if index>0{form.add_space(theme::SPACE_8);}
                            recommendation_picker(form,&mut self.draft,key,label);
                        }
                    }
                });
            });
        });
    }
    /// Only account rows scroll. Add, delete policy, brand recommendations
    /// and the page's save control remain outside the scroll viewport.
    fn show_account_rows(&mut self,ui:&mut Ui){
        let mut accounts=self.draft.get("accounts")
            .and_then(Value::as_array).cloned().unwrap_or_default();
        let mut delete=None;
        let account_count=accounts.len();
        for (index,account) in accounts.iter_mut().enumerate(){
            let labels=[("bank","은행명"),("number","계좌번호"),("holder","예금주")];
            let label_space=theme::MODAL_FORM_LABEL_WIDTH+theme::SPACE_8;
            let wide=theme::SETTINGS_ACCOUNT_ROW_MIN_WIDTH+3.0*label_space;
            if ui.available_width()>=wide{
                ui.horizontal(|row|{
                    crate::ui_layout::apply_spacing_role(row,crate::ui_layout::SpacingRole::AccountFields);
                    let usable=(row.available_width()-theme::SETTINGS_ACCOUNT_DELETE_WIDTH
                        -3.0*theme::SPACE_8-3.0*label_space).max(1.0);
                    let targets=theme::SETTINGS_ACCOUNT_FIELD_WIDTHS;
                    let factor=(usable/targets.iter().sum::<f32>()).min(1.0);
                    for (column,(key,label)) in labels.into_iter().enumerate(){
                        let width=targets[column]*factor;
                        row.allocate_ui_with_layout(
                            Vec2::new(width+label_space,theme::INPUT_HEIGHT),
                            Layout::top_down(Align::Min),|input|{
                                input.apply_shared_width(crate::card::CardWidthPolicy::Fixed,width+label_space);
                                edit_input_width(input,account,key,label,width);
                            });
                    }
                    if crate::button::action(row,"계좌 삭제",ButtonRole::Danger,true).clicked(){
                        delete=Some(index);
                    }
                });
            }else{
                crate::popup::modal_form_row_spacing(ui);
                for (column,(key,label)) in labels.into_iter().enumerate(){
                    edit_input_width(ui,account,key,label,theme::SETTINGS_ACCOUNT_FIELD_WIDTHS[column]);
                }
                ui.with_layout(Layout::right_to_left(Align::Center),|row|{
                    if crate::button::action(row,"계좌 삭제",ButtonRole::Danger,true).clicked(){
                        delete=Some(index);
                    }
                });
            }
            if index+1<account_count{
                ui.add_space(theme::SPACE_8);
                crate::ui_primitives::soft_divider(ui);
                ui.add_space(theme::SPACE_8);
            }
        }
        if let Some(index)=delete{
            let id=field(&accounts[index],"id");
            let used=self.source.get("usedAccounts")
                .and_then(Value::as_array)
                .is_some_and(|ids|ids.iter().any(|v|v.as_str()==Some(id.as_str())));
            if used{
                self.error=Some(
                    "이 계좌를 사용하는 거래처가 있습니다. 거래처 설정을 변경한 후 삭제하세요.".into()
                );
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
        self.draft["accounts"]=json!(accounts);
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
        crate::card::measured_management_section_card(ui,|form|{
        for (index,(key,title,empty,button)) in sections.iter().enumerate(){
            if index>0{
                crate::ui_primitives::section_divider(form);
            }
            section(form,title,|area|{
                let value=field(&self.draft,key);
                let displayed=if value.is_empty(){*empty}else{value.as_str()};
                area.add(crate::ui_primitives::truncated_label(typography::body(displayed)));
                area.add_space(theme::SPACE_8);
                area.horizontal_wrapped(|buttons|{
                    if crate::button::action(
                        buttons,button,ButtonRole::Standard,
                        editable&&self.folder_picker.is_none()
                    ).clicked(){
                        self.choose_folder(key,buttons.ctx().clone());
                    }
                    // React's "보관함 열기" uses the SAVED Excel path
                    // (state.settings), not a newly chosen unsaved draft.
                    let saved_excel=field(settings(&self.source),"excelDirectory");
                    if index==0 && !saved_excel.is_empty(){
                        if crate::button::action(
                            buttons,"보관함 열기",ButtonRole::Standard,true
                        ).clicked(){
                            self.open_folder(&saved_excel);
                        }
                    }else if index>0 && !value.is_empty(){
                        if crate::button::action(buttons,"자동 수신 중지",
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
        ui.add_space(theme::SPACE_8);
        ui.horizontal(|bar|{
            bar.with_layout(Layout::right_to_left(Align::Center),|actions|{
                let response=crate::button::action(
                    actions,"설정 저장",
                    ButtonRole::Primary,!self.read_only()&&!self.busy()
                );
                if response.clicked(){
                    self.save(gateway.clone(),actions.ctx().clone());
                }

            });
        });
    }
    fn show_data(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>){
        let business=self.source.get("mode").and_then(Value::as_str)==Some("business");
        let reading=self.data_request.is_some()||self.loading.pending();
        card(ui,"접수 원문 감사 보관함",|panel|{
            if crate::button::action(panel,
                "감사 기록 열기",ButtonRole::Standard,
                business&&self.data_available()&&!reading
            ).clicked(){
                self.data_action("archive",None,Arc::clone(&gateway),panel.ctx().clone());
            }
        });
        ui.add_space(theme::SPACE_12);
        card(ui,"데이터 백업 및 복구",|panel|{
            panel.horizontal_wrapped(|actions|{
                if crate::button::action(actions,"지금 백업",ButtonRole::Standard,
                    business&&self.data_available()&&!self.read_only()&&!reading).clicked(){
                    self.data_action(
                        "backup",None,Arc::clone(&gateway),actions.ctx().clone()
                    );
                }
                if crate::button::action(actions,"백업 목록",ButtonRole::Standard,
                    business&&self.data_available()&&!reading).clicked(){
                    self.data_action(
                        "backups",None,Arc::clone(&gateway),actions.ctx().clone()
                    );
                }
            });
            let restore_allowed=!self.read_only()&&!reading;
            let mut chosen_backup=None;
            for filename in &self.backups{
                panel.add_space(theme::SPACE_8);
                crate::ui_primitives::soft_divider(panel);
                panel.add_space(theme::SPACE_8);
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
                    if crate::button::action(row,"이 백업으로 복구",
                        ButtonRole::Standard,
                        restore_allowed).clicked(){
                        chosen_backup=Some(filename.clone());
                    }
                });
            }
            if let Some(filename)=chosen_backup{self.restore_target=Some(filename);}
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
            crate::popup::modal_backdrop(ctx,"settings-reload-backdrop",false);
            crate::popup::show_decision_window(ctx,"저장된 설정 불러오기","settings-reload-confirm",area,|ui|{
                crate::popup::decision_workspace(ui,|decision|{
                    if decision.header("저장된 설정 불러오기","취소",true
                    ){self.reload_confirmation=false;}
                    decision.body(|ui|{
                        ui.label(typography::body(
                            "입력 중인 설정을 닫고 저장된 설정을 다시 불러올까요?"
                        ));
                    });
                    decision.actions(|actions|{
                        if crate::button::action(
                            actions,"불러오기",ButtonRole::Primary,true
                        ).clicked(){
                            self.reload_confirmation=false;
                            self.reload(Arc::clone(&gateway),ctx.clone());
                        }
                    });
                });
            });
            return;
        }
        if self.archive_open{
            crate::popup::modal_backdrop(ctx,"settings-audit-backdrop",false);
            crate::popup::modal_window(ctx,
                "접수 원문 감사 보관함","settings-audit-records",area
            ).default_width(720.0).show(ctx,|ui|{

                crate::popup::modal_flow_workspace(ui,|workspace|{
                    if workspace.header("접수 원문 감사 보관함","닫기",true
                    ){self.archive_open=false;}
                    workspace.body_region(|ui|{
                        crate::scrollbar::apply_management_scroll_role(ui,crate::scrollbar::ManagementScrollRole::Standard);
                        let search_width=ui.available_width();
                        crate::input::management_search_field(
                            ui,&mut self.archive_query,search_width,
                            "원문 / 담당자 / 주문 ID 검색"
                        );
                        let needle=self.archive_query.trim().to_lowercase();
                        let indices=self.archive_cache.get(needle.clone(),||{
                            self.archive.as_ref().into_iter().flatten().enumerate()
                                .filter(|(_,row)|needle.is_empty()||row.to_string().to_lowercase().contains(&needle))
                                .map(|(i,_)|i).collect()
                        });
                        let matches=indices.iter().filter_map(|&i|self.archive.as_ref().and_then(|rows|rows.get(i))).collect::<Vec<_>>();
                        ui.label(typography::caption(&format!("{}건 조회",matches.len())));
                        let results_height=crate::popup::modal_list_max_height(
                            ui,area.height(),area.height()*0.62,false
                        );
                        crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical).id_salt("settings-audit-records-scroll")
                            .max_height(results_height)
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
                                    crate::button::collapsing_header(title)
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
                });
            });
        }
        if let Some(filename)=self.restore_target.clone(){
            crate::popup::modal_backdrop(ctx,"settings-restore-backdrop",false);
            crate::popup::show_decision_window(ctx,"데이터 복구 확인","settings-restore-confirm",area,|ui|{
                crate::popup::decision_workspace(ui,|decision|{
                    if decision.header("데이터 복구 확인","취소",!self.busy()
                    ){self.restore_target=None;}
                    decision.body(|ui|{
                        ui.label(typography::body(
                            "선택한 백업 시점으로 업무 데이터를 복구할까요?\n\n현재 데이터는 자동으로 백업되며, 주문내역과 변경 기록도 해당 시점으로 돌아갑니다."
                        ));
                    });
                    decision.actions(|actions|{
                        if crate::button::action(actions,"이 백업으로 복구",ButtonRole::Danger,
                            self.data_available()&&!self.read_only()&&!self.busy()).clicked(){
                            self.restore_target=None;
                            self.data_action(
                                "restore",Some(filename),
                                Arc::clone(&gateway),ctx.clone()
                            );
                        }
                    });
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

#[cfg(test)]
mod management_async_layout_tests{
    use super::*;

    #[test]
    fn conflict_restore_action_stays_in_fixed_title_slot_without_moving_tabs(){
        fn measure(conflict:bool)->(f32,f32,f32,egui::Rect,Option<egui::Rect>,f32){
            let ctx=Context::default();
            crate::typography::install(&ctx);
            let mut measured=None;
            let mut raw=egui::RawInput::default();
            raw.screen_rect=Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,Vec2::new(1280.0,720.0)
            ));
            let _=ctx.run(raw,|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.set_width(1248.0);
                    let (clicked,button,title,right)=settings_title_row(
                        ui,conflict,true
                    );
                    assert!(!clicked);
                    let title_bottom=ui.cursor().top();
                    ui.add_space(theme::SPACE_8);
                    let tabs_top=ui.cursor().top();
                    crate::ui_layout::management_top_content(ui,|top|{
                        crate::button::management_tabs(top,&[
                            ("기본 설정",true,true),
                            ("파일 관리",false,true),
                            ("데이터 관리",false,true),
                            ("시스템",false,true)
                        ]);
                    });
                    let tabs_bottom=ui.cursor().top();
                    measured=Some((
                        title_bottom,tabs_top,tabs_bottom,title,button,right
                    ));
                });
            });
            measured.expect("settings title and tabs were rendered")
        }
        let source=include_str!("settings_view.rs");
        let page=source.split("pub fn show_page(").nth(1).unwrap()
            .split("let selected_tab=").next().unwrap();
        assert!(page.contains("settings_title_row("));
        let normal=measure(false);
        let conflict=measure(true);
        let near=|a:f32,b:f32|(a-b).abs()<=0.5;
        assert!(near(normal.0,conflict.0),
            "conflict action changed the title row bottom");
        assert!(near(normal.1,conflict.1),
            "conflict action moved the settings tabs");
        assert!(near(normal.2,conflict.2),
            "conflict action changed the tab row height");
        assert!(near(normal.3.top(),conflict.3.top()));
        assert!(near(normal.3.bottom(),conflict.3.bottom()));
        assert!(near(conflict.3.height(),theme::ACTION_STANDARD_HEIGHT));
        let button=conflict.4.expect("conflict state must expose restore action");
        assert!(near(button.right(),conflict.5),
            "restore action is not aligned to the title content's right edge");
        assert!(near(button.height(),theme::ACTION_STANDARD_HEIGHT));
        assert!(near(button.center().y,conflict.3.center().y));
    }
}

#[cfg(test)]
impl SettingsView {
    pub(crate) fn geometry_fixture(gateway:Arc<dyn BandoGateway>,ctx:egui::Context,tab:usize,loaded:bool)->Self{
        let mut page=Self::new(gateway,ctx);
        page.loading.reset();page.tab=SettingsTab::ALL[tab];page.loaded=loaded;if loaded{page.source=serde_json::json!({"settings":{},"readOnly":true,"dataAvailable":false});page.draft=serde_json::json!({});}
        page
    }

    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        let finish=self.loading.geometry_begin_read();
        self.error=None;
        Box::new(move |answer|{
            let result=answer.map(|count|{
                let accounts=(0..count).map(|i|json!({"id":format!("geometry-account-{i}"),
                    "bank":"합성 은행","number":format!("TEST-{i:04}"),"holder":"검증 전용"})).collect::<Vec<_>>();
                let folder=if count==0{String::new()}else{"C:/geometry-only/long-synthetic-path/not-a-real-directory".repeat(3)};
                json!({"settings":{"accounts":accounts,"brandAccounts":{},
                    "excelDirectory":folder.clone(),"factoryReplyDirectory":folder.clone(),
                    "orderImageInboxDirectory":folder},"readOnly":true,"dataAvailable":false})
            });
            finish(result);
        })
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
}
