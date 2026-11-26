//! Native "자동 메시지 편집": approved React behavior and freight UI roles.
//! The 20 template labels/defaults below are DISPLAY metadata only, copied
//! from src/automatic-message-templates.ts. Validation, business message
//! generation, settings CAS and SQLite stay in the existing Rust services.
use crate::{gateway::{BandoGateway,ReadOperation},
    settings_help,theme::{self,ButtonRole},typography};
use eframe::egui::{self,Align,Context,Layout,Ui,Vec2};
use serde_json::{json,Map,Value};
use std::{sync::{mpsc::{self,Receiver},Arc,OnceLock},thread};
use regex::Regex;

#[derive(Clone,Copy)]
struct TemplateDef {
    id:&'static str,
    label:&'static str,
    category:&'static str,
    description:&'static str,
    body:&'static str,
    variables:&'static [&'static str],
    required:&'static [&'static str],
    sample:&'static [(&'static str,&'static str)],
}
const TEMPLATES:&[TemplateDef]=&[
    TemplateDef{ id:"recipientName", label:"수취인 이름 누락",
        category:"확인 안내", description:"수취인 이름이 누락된 주문의 확인 문구",
        body:"받으실 분의 성함을 알려주세요.", variables:&[],
        required:&[],
        sample:&[] },
    TemplateDef{ id:"addressMissing", label:"배송지 주소 누락",
        category:"확인 안내", description:"배송지가 없는 발주서",
        body:"{수취인} 배송지 주소 확인 부탁드려요.", variables:&["수취인"],
        required:&["수취인"],
        sample:&[("수취인","김민수")] },
    TemplateDef{ id:"addressDetail", label:"상세주소 부족",
        category:"확인 안내", description:"기재된 주소가 불완전한 경우",
        body:"{수취인} 상세주소 확인 부탁드려요.", variables:&["수취인"],
        required:&["수취인"],
        sample:&[("수취인","김민수")] },
    TemplateDef{ id:"contactMissing", label:"수취인 연락처 누락",
        category:"확인 안내", description:"수취인 연락처가 없거나 올바르지 않을 때",
        body:"{수취인} 연락처 확인 부탁드려요.", variables:&["수취인"],
        required:&["수취인"],
        sample:&[("수취인","김민수")] },
    TemplateDef{ id:"quantityMissing", label:"주문 수량 확인",
        category:"확인 안내", description:"상품 수량이 확인되지 않은 경우",
        body:"{상품명} 수량을 확인 부탁드립니다.", variables:&["상품명"],
        required:&["상품명"],
        sample:&[("상품명","BD 709")] },
    TemplateDef{ id:"modelMissing", label:"상품 모델 확인",
        category:"확인 안내", description:"정확한 모델명이나 상품 선택이 필요한 경우",
        body:"{상품명}: 정확한 모델명을 확인해 주세요.", variables:&["상품명"],
        required:&["상품명"],
        sample:&[("상품명","보면대")] },
    TemplateDef{ id:"modelConfirm", label:"추천 상품 확인",
        category:"확인 안내", description:"직원이 제안한 상품이 맞는지 안내하는 문구. 링크는 별도 자동 첨부",
        body:"{상품명} 상품으로 보내드리면 될까요?", variables:&["상품명"],
        required:&["상품명"],
        sample:&[("상품명","BD 709")] },
    TemplateDef{ id:"requestRecipient", label:"확인 항목 묶음",
        category:"확인 안내", description:"여러 누락 항목을 묶어 수취인에게 안내",
        body:"{번호}. {수취인}: {확인항목}", variables:&["번호","수취인","확인항목"],
        required:&["수취인","확인항목"],
        sample:&[("번호","1"),("수취인","김민수"),("확인항목","배송지 주소, 연락처")] },
    TemplateDef{ id:"requestClosing", label:"확인 요청 마무리",
        category:"확인 안내", description:"회신이 필요한 안내 메시지의 마무리",
        body:"위 내용 확인 부탁드립니다.", variables:&[],
        required:&[],
        sample:&[] },
    TemplateDef{ id:"colorDefault", label:"대표색상 통보",
        category:"색상 및 수량", description:"색상이 빠진 발주서에 직원이 지정한 대표색상으로 안내",
        body:"{색상}으로 보내드릴게요.", variables:&["색상","상품명"],
        required:&["색상"],
        sample:&[("색상","블랙"),("상품명","BD 709")] },
    TemplateDef{ id:"modelRepresentative", label:"대표모델 선택 통보",
        category:"모델 및 옵션", description:"모호한 발주서에 직원이 모델군의 대표 상품을 직접 선택한 경우에만 안내",
        body:"{상품명} 모델로 보내드릴게요.", variables:&["상품명"],
        required:&["상품명"],
        sample:&[("상품명","BD 709 / 블랙 / 타공 상판")] },
    TemplateDef{ id:"quantityOne", label:"수량 1개 잠정 안내",
        category:"색상 및 수량", description:"수량이 없고 1개로 안내한 경우",
        body:"{수취인} {상품명} 상품은 수량을 1개로 보내드리겠습니다.", variables:&["수취인","상품명"],
        required:&["수취인","상품명"],
        sample:&[("수취인","김민수"),("상품명","BD 709")] },
    TemplateDef{ id:"paymentGreeting", label:"입금 안내 인사말",
        category:"입금 안내", description:"명세서 안내문 첫머리",
        body:"안녕하세요.", variables:&[],
        required:&[],
        sample:&[] },
    TemplateDef{ id:"paymentItem", label:"상품별 금액 표시",
        category:"입금 안내", description:"상품명, 수량, 가격은 실제 주문 자료로 채움",
        body:"{상품명} * {수량}개 : {금액}", variables:&["상품명","수량","금액"],
        required:&["상품명","수량","금액"],
        sample:&[("상품명","BD 709 (블랙)"),("수량","3"),("금액","90,000원")] },
    TemplateDef{ id:"paymentFreight", label:"운임 안내",
        category:"입금 안내", description:"운임은 실제 배송 계산 결과로 채움",
        body:"운임 : {운임}", variables:&["운임"],
        required:&["운임"],
        sample:&[("운임","3,000원")] },
    TemplateDef{ id:"paymentVat", label:"부가세 안내",
        category:"입금 안내", description:"부가세 포함 주문에서만 표시",
        body:"부가세 : {부가세}", variables:&["부가세"],
        required:&["부가세"],
        sample:&[("부가세","9,300원")] },
    TemplateDef{ id:"paymentTotal", label:"총 청구액 안내",
        category:"입금 안내", description:"정산 결과를 그대로 표시",
        body:"합계 : {합계} 입니다.", variables:&["합계"],
        required:&["합계"],
        sample:&[("합계","102,300원")] },
    TemplateDef{ id:"paymentAccount", label:"입금 계좌 안내",
        category:"입금 안내", description:"선택된 실제 계좌정보로 채움",
        body:"{은행명} {계좌번호} {예금주} 입니다.", variables:&["은행명","계좌번호","예금주"],
        required:&["은행명","계좌번호","예금주"],
        sample:&[("은행명","국민은행"),("계좌번호","000-0000-0000"),("예금주","반도")] },
    TemplateDef{ id:"optionsIntro", label:"주문 내용 확인 시작",
        category:"확인 안내", description:"상품 옵션 확인 안내문의 시작",
        body:"주문하신 내역 확인 부탁드립니다.", variables:&[],
        required:&[],
        sample:&[] },
    TemplateDef{ id:"optionsClosing", label:"주문 확인 마무리",
        category:"확인 안내", description:"상품 옵션 확인 안내문의 마무리",
        body:"주문 내용 확인 후 회신 부탁드립니다.", variables:&[],
        required:&[],
        sample:&[] },
];

#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Section { General, Provisional }
#[derive(Clone)]
enum Destination {
    ClientList,
    GlobalPage,
    Subtab(Section),
    Template(usize),
    Product(String),
}
type Saved=Receiver<Result<Value,String>>;

pub struct AutomaticMessageManager {
    pub section:Section,
    tab_snapshots:crate::view_state::NavigationSnapshots<2>,
    selected:usize,
    selected_product:String,
    product_search:String,
    settings:Value,
    products:Vec<Value>,
    product_cache:crate::screen_refresh::ViewProjectionCache<String,Vec<usize>>,
    loading:crate::screen_refresh::ReadRefresh<(Value,Value)>,
    saving:Option<Saved>,
    loaded:bool,
    base_templates:Map<String,Value>,
    draft_templates:Map<String,Value>,
    base_notices:Map<String,Value>,
    draft_notices:Map<String,Value>,
    pending:Option<Destination>,
    after_save:Option<Destination>,
    clients_approved:bool,
    global_approved:bool,
    error:Option<String>,
    notice:Option<String>,
}
fn message_map(settings:&Value,key:&str)->Map<String,Value>{
    settings.get(key).and_then(Value::as_object).cloned().unwrap_or_default()
}
fn product_name(product:&Value)->&str{
    product.get("name").and_then(Value::as_str).unwrap_or("")
}
fn product_id(product:&Value)->&str{
    product.get("id").and_then(Value::as_str).unwrap_or("")
}
fn has_registered_product(product:&Value)->bool{
    !product_id(product).is_empty()
        && product["active"]!=Value::Bool(false)
        && product.get("deletedAt").is_none_or(Value::is_null)
        && product.get("supersededBy").is_none_or(|value|
            value.is_null() || value.as_str().is_some_and(str::is_empty))
}
/// Presentation-only natural name order: React uses ko numeric collation.
/// Numeric model segments (BD 9 vs BD 10) are ordered by integer magnitude
/// without changing any catalog identity, match or pricing rule.
fn natural_name_cmp(a:&str,b:&str)->std::cmp::Ordering{
    use std::cmp::Ordering;
    let mut left=a.chars().peekable();
    let mut right=b.chars().peekable();
    loop {
        match (left.peek().copied(),right.peek().copied()){
            (None,None)=>return Ordering::Equal,
            (None,Some(_))=>return Ordering::Less,
            (Some(_),None)=>return Ordering::Greater,
            (Some(x),Some(y)) if x.is_ascii_digit()&&y.is_ascii_digit()=>{
                let mut a_num=String::new();
                let mut b_num=String::new();
                while left.peek().is_some_and(char::is_ascii_digit){
                    a_num.push(left.next().unwrap_or_default());
                }
                while right.peek().is_some_and(char::is_ascii_digit){
                    b_num.push(right.next().unwrap_or_default());
                }
                let a_sig=a_num.trim_start_matches('0');
                let b_sig=b_num.trim_start_matches('0');
                let cmp=a_sig.len().cmp(&b_sig.len())
                    .then_with(||a_sig.cmp(b_sig));
                if cmp!=Ordering::Equal{return cmp;}
            },
            (Some(x),Some(y))=>{
                left.next();right.next();
                let cmp=x.to_lowercase().cmp(y.to_lowercase());
                if cmp!=Ordering::Equal{return cmp;}
            }
        }
    }
}
fn preview_message(def:TemplateDef,body:&str)->String{
    static TOKENS:OnceLock<Regex>=OnceLock::new();
    let regex=TOKENS.get_or_init(||Regex::new(r"\{([^{}]+)\}").expect("literal"));
    regex.replace_all(body,|captured:&regex::Captures<'_>|{
        def.sample.iter().find(|(key,_)|*key==&captured[1])
            .map(|(_,value)|*value).unwrap_or("").to_owned()
    }).into_owned()
}
impl AutomaticMessageManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self{
        let mut loading=crate::screen_refresh::ReadRefresh::default();
        loading.submit_required(ctx,move||{
            let settings=gateway.load_settings()?;
            let catalog=gateway.load_catalog()?;
            Ok((settings,catalog))
        });
        Self{
            section:Section::General,tab_snapshots:crate::view_state::NavigationSnapshots::new(0),selected:TEMPLATES.iter()
                .position(|def|def.id=="colorDefault").unwrap_or(0),
            selected_product:String::new(),product_search:String::new(),
            settings:Value::Null,products:Vec::new(),product_cache:Default::default(),loading,
            saving:None,loaded:false,
            base_templates:Map::new(),draft_templates:Map::new(),
            base_notices:Map::new(),draft_notices:Map::new(),
            pending:None,after_save:None,
            clients_approved:false,global_approved:false,
            error:None,notice:None
        }
    }
    pub fn poll(&mut self){
        match self.loading.poll(){
            Some(Ok((settings,catalog)))=>{
                self.products=catalog["products"].as_array().cloned().unwrap_or_default();self.product_cache.invalidate();
                self.set_saved(settings);
                self.loading.verified();
                self.loaded=true;self.error=None;
            }
            Some(Err(message))=>self.error=Some(message),
            None=>{}
        }
        if let Some(rx)=self.saving.as_ref(){
            match rx.try_recv(){
                Ok(Ok(settings))=>{
                    self.set_saved(settings);
                    self.saving=None;self.error=None;
                    self.notice=Some("자동 메시지 설정을 저장했습니다.".into());
                    if let Some(destination)=self.after_save.take(){
                        self.apply_destination(destination);
                    }
                },
                Ok(Err(message))=>{
                    self.error=Some(message);self.saving=None;
                    self.after_save=None;
                    // The saved draft stays intact after validation/CAS errors.
                },
                Err(mpsc::TryRecvError::Disconnected)=>{
                    self.error=Some("자동 메시지 저장 연결이 중단되었습니다.".into());
                    self.saving=None;self.after_save=None;
                },
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
    }
    fn set_saved(&mut self,settings:Value){
        let current=&settings["settings"];
        self.base_templates=message_map(current,"automaticMessageTemplates");
        self.draft_templates=self.base_templates.clone();
        self.base_notices=message_map(current,"provisionalProductNotices");
        {self.draft_notices=self.base_notices.clone();self.product_cache.invalidate();};
        self.settings=settings;
    }
    fn template_value(map:&Map<String,Value>,def:TemplateDef)->String{
        map.get(def.id).and_then(Value::as_str)
            .unwrap_or(def.body).replace("{수취인}님","{수취인}")
    }
    pub(crate) fn suspend_navigation(&mut self,now:std::time::Instant){self.tab_snapshots.suspend(now);}
    fn sync_tab_navigation(&mut self){
        if !self.tab_snapshots.visit(self.section as usize,std::time::Instant::now()){return;}
        match self.section{
            Section::General=>self.selected=TEMPLATES.iter().position(|def|def.id=="colorDefault").unwrap_or(0),
            Section::Provisional=>{self.selected_product.clear();self.product_search.clear();}
        }
    }
    pub(crate) fn reset_navigation(&mut self){
        if self.dirty()||self.busy(){return;}
        self.section=Section::General;self.tab_snapshots=crate::view_state::NavigationSnapshots::new(0);
        self.selected=TEMPLATES.iter().position(|def|def.id=="colorDefault").unwrap_or(0);
        self.selected_product.clear();self.product_search.clear();
    }
    pub fn dirty(&self)->bool{
        if !self.loaded{return false;}
        match self.section{
            Section::General=>TEMPLATES.iter().copied().any(|def|
                Self::template_value(&self.base_templates,def)
                    !=Self::template_value(&self.draft_templates,def)),
            Section::Provisional=>self.base_notices!=self.draft_notices,
        }
    }
    pub fn busy(&self)->bool{self.saving.is_some() || self.loading.pending()}
    fn writable(&self)->bool{
        self.loaded && !self.loading.required() && self.settings["readOnly"]!=Value::Bool(true)
            && (self.settings["mode"].as_str()!=Some("business")
                ||self.settings["dataAvailable"]==Value::Bool(true))
    }
    fn request(&mut self,destination:Destination){
        if self.busy()||self.pending.is_some(){return;}
        if self.dirty(){self.pending=Some(destination);}
        else{self.apply_destination(destination);}
    }
    fn apply_destination(&mut self,destination:Destination){
        match destination{
            Destination::ClientList=>self.clients_approved=true,
            Destination::GlobalPage=>self.global_approved=true,
            Destination::Subtab(tab)=>self.section=tab,
            Destination::Template(index)=>self.selected=index,
            Destination::Product(id)=>self.selected_product=id,
        }
        self.pending=None;self.error=None;
    }
    pub fn request_clients(&mut self){self.request(Destination::ClientList);}
    pub fn take_clients_approved(&mut self)->bool{
        std::mem::take(&mut self.clients_approved)
    }
    pub fn request_global_leave(&mut self)->bool{
        if !self.loaded || !self.dirty(){return true;}
        self.request(Destination::GlobalPage);
        false
    }
    pub fn take_global_approved(&mut self)->bool{
        std::mem::take(&mut self.global_approved)
    }
    pub fn has_confirmation(&self)->bool{self.pending.is_some()}
    pub fn global_leave_in_progress(&self)->bool{
        matches!(self.pending.as_ref(),Some(Destination::GlobalPage))
            ||matches!(self.after_save.as_ref(),Some(Destination::GlobalPage))
    }
    fn discard_active(&mut self){
        match self.section{
            Section::General=>self.draft_templates=self.base_templates.clone(),
            Section::Provisional=>{self.draft_notices=self.base_notices.clone();self.product_cache.invalidate();},
        }
        self.error=None;
    }
    fn start_save(&mut self,gateway:Arc<dyn BandoGateway>,ctx:Context){
        if self.busy()||!self.dirty()||!self.writable(){return;}
        let field=match self.section{
            Section::General=>"automaticMessageTemplates",
            Section::Provisional=>"provisionalProductNotices"
        };
        let draft=if self.section==Section::General{
            let mut templates=self.draft_templates.clone();
            for body in templates.values_mut(){
                if let Some(value)=body.as_str(){
                    *body=json!(value.replace("{수취인}님","{수취인}"));
                }
            }
            Value::Object(templates)
        }else{Value::Object(self.draft_notices.clone())};
        let original=self.settings["settings"].get(field)
            .cloned().unwrap_or(Value::Null);
        let business=self.settings["mode"]==Value::String("business".into());
        let keys=if self.section==Section::General{
            TEMPLATES.iter().filter(|def|
                self.draft_templates.contains_key(def.id)
            ).map(|def|def.id).collect::<Vec<_>>()
        }else{Vec::new()};
        crate::floating_notice::dispatch_pointer(&ctx,"clients.automatic.message");
        let (tx,rx)=mpsc::channel();
        self.saving=Some(rx);self.error=None;self.notice=None;
        thread::spawn(move||{
            let result=(||{
                if field=="automaticMessageTemplates"{
                    gateway.execute(ReadOperation::ValidateAutomaticTemplates,json!({
                        "edits":draft,"orderedKeys":keys
                    }))?;
                }else{
                    let catalog=gateway.load_catalog()?;
                    gateway.execute(ReadOperation::ValidateProvisionalProductNotices,json!({
                        "state":{"products":catalog["products"],
                            "settings":{"provisionalProductNotices":draft}}
                    }))?;
                }
                // The original React editor backs up BEFORE a real settings
                // transaction. Isolated preview settings never touch that DB.
                if business{gateway.settings_data("backup",None)?;}
                let mut changes=Map::new();changes.insert(field.into(),draft.clone());
                let mut expected=Map::new();expected.insert(field.into(),original);
                let saved=gateway.patch_settings(
                    Value::Object(changes),Value::Object(expected)
                )?;
                if saved["settings"].get(field)!=Some(&draft){
                    return Err("저장 후 자동 메시지 설정이 입력 내용과 다릅니다.".into());
                }
                Ok(saved)
            })();
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn list_height(ui:&Ui)->f32{
        (ui.ctx().content_rect().height()*theme::MANAGEMENT_TABLE_VIEWPORT_FRACTION)
            .min(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
            .min((ui.available_height()-theme::SPACE_24).max(1.0))
    }
    fn provisional_visible(&mut self)->std::sync::Arc<Vec<usize>>{
        self.product_cache.get(self.product_search.clone(),||{
        let query=self.product_search.trim().to_lowercase();
        let mut visible=self.products.iter().enumerate()
            .filter(|(_,p)|has_registered_product(p))
            .filter(|(_,p)|{
                if query.is_empty(){
                    self.draft_notices.get(product_id(p))
                        .and_then(Value::as_str).is_some_and(|s|!s.is_empty())
                }else{
                    format!("{} {}",product_name(p),p["subtitle"].as_str().unwrap_or(""))
                        .to_lowercase().contains(&query)
                }
            }).map(|(i,_)|i).collect::<Vec<_>>();
        visible.sort_by(|&a,&b|{
            let (a,b)=(&self.products[a],&self.products[b]);
            let registered=|v:&Value|self.draft_notices.get(product_id(v))
                .and_then(Value::as_str).is_some_and(|s|!s.is_empty());
            registered(b).cmp(&registered(a))
                .then_with(||natural_name_cmp(product_name(a),product_name(b)))
        });
        visible.truncate(80);
        visible
        })
    }
    fn general_list(&mut self,ui:&mut Ui,target:&mut Option<Destination>){
        ui.horizontal(|heading|{
            heading.label(typography::section_heading("상황별 자동 메시지"));
            settings_help::icon(heading,"상황별 자동 메시지");
        });
        ui.add_space(theme::SPACE_8);
        crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical).id_salt("auto-template-types")
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .max_height(Self::list_height(ui))
            .show(ui,|list|{
                list.spacing_mut().item_spacing.y=theme::SPACE_4;
                for category in ["확인 안내","색상 및 수량","모델 및 옵션","입금 안내"]{
                    list.label(typography::subsection_heading(category));
                    for (index,def) in TEMPLATES.iter().enumerate()
                        .filter(|(_,def)|def.category==category){
                        let caption=if self.draft_templates.contains_key(def.id){
                            "수정됨"
                        }else{"기본 문구"};
                        if crate::composite_client::management_chooser_row(
                            list,def.label,caption,index==self.selected
                        ).clicked(){
                            *target=Some(Destination::Template(index));
                        }
                    }
                    list.add_space(theme::SPACE_8);
                }
            });
    }
    fn provisional_list(&mut self,ui:&mut Ui,target:&mut Option<Destination>){
        ui.horizontal(|heading|{
            heading.label(typography::section_heading("상품별 안내문 등록"));
            settings_help::icon(heading,"상품별 안내문");
        });
        ui.add_space(theme::SPACE_8);
        ui.label(typography::field_label("안내문을 등록할 상품 검색"));
        let width=ui.available_width();
        crate::input::management_search_field(
            ui,&mut self.product_search,width,"상품명 검색"
        );
        ui.add_space(theme::SPACE_8);
        let visible=self.provisional_visible();
        crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical).id_salt("provisional-notice-products")
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .max_height((Self::list_height(ui)-theme::INPUT_HEIGHT
                -theme::SPACE_24).max(1.0))
            .show(ui,|list|{
                list.spacing_mut().item_spacing.y=theme::SPACE_4;
                if visible.is_empty(){
                    crate::ui_primitives::management_body_state_inline(list,if self.product_search.trim().is_empty(){
                        "아직 등록된 안내문이 없습니다. 상품을 검색해 추가하세요."
                    }else{"검색 결과가 없습니다."});
                }
                for &index in visible.iter(){
                    let product=&self.products[index];
                    let id=product_id(product);
                    let caption=if self.draft_notices.contains_key(id){
                        "안내문 등록됨"
                    }else{"안내문 미등록"};
                    let choice=crate::composite_client::management_chooser_row(
                        list,product_name(product),caption,self.selected_product==id
                    );
                    #[cfg(test)]
                    crate::ui_test_support::record("notice product row",choice.rect);
                    if choice.clicked(){
                        *target=Some(Destination::Product(id.to_owned()));
                    }
                }
            });
    }
    fn general_editor(&mut self,ui:&mut Ui){
        let def=TEMPLATES[self.selected];
        ui.horizontal(|heading|{
            heading.label(typography::section_heading(def.label));
            settings_help::icon(heading,"선택한 자동 메시지");
            heading.with_layout(Layout::right_to_left(Align::Center),|actions|{
                if crate::button::action(actions,"기본 문구 복원",ButtonRole::Standard,
                    !self.busy() && self.draft_templates.contains_key(def.id)
                ).clicked(){
                    self.draft_templates.remove(def.id);
                    self.error=None;
                }
            });
        });
        ui.label(typography::caption(def.description));
        crate::ui_primitives::section_divider(ui);
        ui.label(typography::field_label("복사용 자동 메시지 편집"));
        let mut content=Self::template_value(&self.draft_templates,def);
        let width=ui.available_width();
        if crate::input::multiline_text_field(
            ui,&mut content,!self.busy(),width,6,None
        ).changed(){
            self.draft_templates.insert(def.id.into(),json!(content.clone()));
            self.error=None;
        }
        let vars=if def.variables.is_empty(){"없음".into()}else{
            def.variables.iter().map(|v|format!("{{{v}}}"))
                .collect::<Vec<_>>().join(", ")
        };
        ui.horizontal_wrapped(|metadata|{
            metadata.label(typography::caption(&format!(
                "안내문에 넣을 수 있는 자동 표시 항목: {vars}{}",
                if def.required.is_empty(){""}else{" (필수 변수는 삭제할 수 없습니다.)"}
            )));
            settings_help::icon(metadata,"자동 표시 항목");
        });
        ui.add_space(theme::SPACE_8);
        ui.label(typography::subsection_heading("예시 미리보기"));
        crate::card::preview_surface_frame().show(ui,|preview|{
                preview.label(typography::body(&preview_message(def,&content)));
            });
    }
    fn provisional_editor(&mut self,ui:&mut Ui){
        let product=self.products.iter()
            .find(|p|product_id(p)==self.selected_product)
            .cloned();
        if let Some(product)=product{
            let id=product_id(&product);
            let mut content=self.draft_notices.get(id).and_then(Value::as_str)
                .unwrap_or("").to_owned();
            ui.horizontal(|heading|{
                heading.label(typography::section_heading(product_name(&product)));
                heading.with_layout(Layout::right_to_left(Align::Center),|actions|{
                    if crate::button::action(actions,"안내문 삭제",ButtonRole::Standard,
                        !self.busy() && !content.is_empty()
                    ).clicked(){
                        self.draft_notices.remove(id);self.product_cache.invalidate();
                        content.clear();self.error=None;
                    }
                });
            });
            crate::ui_primitives::section_divider(ui);
            ui.label(typography::field_label(
                "잠정 선택 시 복사용 계좌안내에 추가할 문구"
            ));
            let width=ui.available_width();
            if crate::input::multiline_text_field(
                ui,&mut content,!self.busy(),width,5,
                Some("예: 709는 상판에 구멍이 있습니다. 구멍이 없는 제품은 709-1 모델을 확인해 주세요.")
            ).changed(){
                if content.trim().is_empty(){self.draft_notices.remove(id);self.product_cache.invalidate();}
                else{self.draft_notices.insert(id.into(),json!(content.clone()));self.product_cache.invalidate();}
                self.error=None;
            }
            ui.add_space(theme::SPACE_8);
            ui.label(typography::subsection_heading("계좌안내에 표시될 위치"));
            crate::card::preview_surface_frame().show(ui,|preview|{
                    for item in [
                        "상품명, 수량, 금액",
                        "운임 및 부가세",
                        "합계, 입금 계좌",
                    ]{preview.label(typography::caption(item));}
                    preview.label(typography::body(if content.trim().is_empty(){
                        "이 상품에 등록된 안내문이 없습니다."
                    }else{content.trim()}));
                    preview.label(typography::caption("상품 링크 (추가한 경우에만)"));
                });
        }else{
            ui.label(typography::caption(
                "상품을 검색해서 선택하면 해당 상품의 전용 안내문을 등록할 수 있습니다."
            ));
        }
    }
    pub fn show(&mut self,ui:&mut Ui,gateway:Arc<dyn BandoGateway>,page_origin_y:f32){
        self.poll();
        self.sync_tab_navigation();
        crate::floating_notice::sync(
            ui.ctx(),"clients.auto.result","clients.automatic.message",
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"clients.auto.error","clients.automatic.message",
            crate::floating_notice::Tone::Error,
            self.error.as_deref()
        );
        // The nested message tabs follow the same guide as the client
        // page's main heading and tabs, without moving either editor card.
        let tab=crate::ui_layout::management_top_content(ui,|top|
            crate::button::management_tabs(top,&[
                ("상황별 공통 안내",self.section==Section::General,true),
                ("잠정 상품 선택 안내",self.section==Section::Provisional,true),
            ])
        );
        ui.push_id(("automatic-message-navigation",self.section as usize,self.tab_snapshots.epoch(self.section as usize)),|ui|{
        ui.add_space(theme::SPACE_12);
        if !self.loaded{
            crate::card::align_management_primary_card(ui,page_origin_y);
            crate::card::management_pending_primary_card(ui);
            return;
        }
        let mut target=tab.map(|index|Destination::Subtab(
            if index==0{Section::General}else{Section::Provisional}
        ));
        // Two functional regions, matching the original left-list/right-edit
        // workspace. Do not import Freight's large KPI cards into this editor.
        // The extra message sub-tab stays above the shared primary card line.
        crate::card::align_management_primary_card(ui,page_origin_y);
        let width=ui.available_width().max(1.0);
        let gap=theme::SPACE_12;
        let left_width=(width*0.35).clamp(250.0,theme::FAMILY_LIST_WIDTH)
            .min((width-gap).max(160.0));
        let right_width=(width-left_width-gap).max(260.0);
        // Use the actual remaining management pane height. A 340pt
        // minimum previously forced the footer below short windows.
        let height=(ui.available_height()-theme::SPACE_8).max(1.0);
        let card_min_height=(height-2.0*(theme::SECTION_PADDING
            +theme::FRAME_STROKE_WIDTH)).max(0.0);
        let mut save=false;
        ui.horizontal(|panes|{
            panes.spacing_mut().item_spacing.x=gap;
            panes.allocate_ui_with_layout(
                Vec2::new(left_width,height),Layout::top_down(Align::Min),
                |list|{
                    list.set_width(left_width);
                    crate::card::measured_management_section_card(list,|body|{
                        crate::card::fill_management_primary_card(body,card_min_height);
                        if self.section==Section::General{
                            self.general_list(body,&mut target);
                        }else{self.provisional_list(body,&mut target);}
                    });
                }
            );
            panes.allocate_ui_with_layout(
                Vec2::new(right_width,height),Layout::top_down(Align::Min),
                |editor|{
                    editor.set_width(right_width);
                    crate::card::measured_management_section_card(editor,|body|{
                        crate::card::fill_management_primary_card(body,card_min_height);
                        crate::scrollbar::scroll_area(crate::scrollbar::ScrollAxes::Vertical)
                            .id_salt("automatic-message-editor-fields")
                            .max_height((height-theme::INPUT_HEIGHT
                                -theme::SPACE_24*2.0).max(1.0))
                            .show(body,|fields|{
                                if self.section==Section::General{
                                    self.general_editor(fields);
                                }else{self.provisional_editor(fields);}
                            });
                        body.add_space(theme::SPACE_12);
                        crate::ui_primitives::soft_divider(body);
                        body.horizontal(|footer|{
                            footer.with_layout(Layout::right_to_left(Align::Center),|right|{
                                let action=crate::button::action(right,
                                    "변경사항 저장",
                                    ButtonRole::Primary,self.writable()&&!self.busy()
                                        &&self.dirty());
                                save=action.clicked();

                            });
                        });
                        // One-off save errors use the shared floating notice.
                        // Initial-load errors above stay visible as page state.
                    });
                }
            );
        });
        if save{self.start_save(Arc::clone(&gateway),ui.ctx().clone());}
        if let Some(destination)=target{
            let same=match &destination{
                Destination::Subtab(tab)=>self.section==*tab,
                Destination::Template(index)=>self.selected==*index,
                Destination::Product(id)=>self.selected_product==*id,
                _=>false
            };
            if !same{self.request(destination);}
        }
        });
    }
    pub fn show_confirmation(
        &mut self,ctx:&Context,area:egui::Rect,gateway:Arc<dyn BandoGateway>
    ){
        let Some(destination)=self.pending.clone()else{return;};
        let mut cancel=false;let mut discard=false;let mut save=false;
        crate::popup::modal_backdrop(ctx,"message-editor-leave-backdrop",false);
        crate::popup::show_decision_window(ctx,"저장하지 않은 변경사항","message-editor-leave",area,|dialog|{
            crate::popup::decision_workspace(dialog,|decision|{
                cancel=decision.header("저장하지 않은 변경사항","취소",!self.busy()
                );
                decision.body(|dialog|{
                    dialog.label(typography::body(
                        "자동 메시지 편집 변경사항을 저장할까요?"
                    ));
                });
                decision.actions(|actions|{
                    save=crate::button::action(actions,"저장하고 이동",ButtonRole::Primary,
                        self.writable()&&!self.busy()).clicked();
                    discard=crate::button::action(actions,"저장 안 함",ButtonRole::Danger,
                        !self.busy()).clicked();
                });
            });
        });
        if cancel{self.pending=None;}
        else if discard{
            self.discard_active();
            self.apply_destination(destination);
        }else if save{
            self.pending=None;
            self.after_save=Some(destination);
            self.start_save(gateway,ctx.clone());
        }
    }
}

#[cfg(test)]
impl AutomaticMessageManager {
    pub(crate) fn geometry_fixture(gateway:Arc<dyn BandoGateway>,ctx:egui::Context,tab:usize,loaded:bool)->Self{
        let mut page=Self::new(gateway,ctx);
        page.loading.reset();page.section=if tab==0{Section::General}else{Section::Provisional};if loaded{page.set_saved(serde_json::json!({"settings":{}}));page.loaded=true;}
        page
    }

    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        let finish=self.loading.geometry_begin_read();
        self.error=None;
        Box::new(move |answer|{
            let result=answer.map(|count|{
                let products=(0..count).map(|i|serde_json::json!({"id":format!("geometry-p-{i}"),"name":format!("검증 상품 {i}"),"active":true})).collect::<Vec<_>>();
                let notices=(0..count).map(|i|(format!("geometry-p-{i}"),Value::String("검증 안내문: 줄바꿈과 긴 문장을 포함합니다.".repeat(5))))
                    .collect::<serde_json::Map<String,Value>>();
                (serde_json::json!({"settings":{"provisionalProductNotices":notices}}),
                 serde_json::json!({"products":products}))
            });
            finish(result);
        })
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
    pub(crate) fn geometry_select_first(&mut self){self.selected_product="geometry-p-0".into();}
}
