//! React ProductManager 상품 목록: Rust-owned catalog selection and a
//! read-only price/detail projection. Product and price writes must not target
//! the isolated egui preview DB or bypass Tauri's operational save boundary.
use crate::{
    category_manager::CategoryManager,
    components,
    gateway::{BandoGateway, ReadOperation},
    theme, typography,
};
use eframe::egui::{self, Align, Context, Layout, Ui};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{mpsc::{self, Receiver, TryRecvError}, Arc},
    time::{Duration, Instant},
};

struct CatalogRows {
    catalog: Value,
    products: Vec<Value>,
}

type SearchReply = (u64, Result<CatalogRows, String>);

fn field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn price(value: &Value) -> String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(|| "—".to_owned())
}
fn option_text(product: &Value, variant: &Value) -> String {
    let mut parts = Vec::new();
    if field(product, "colorStatus") != "NOT_REQUIRED" {
        let color = field(variant, "color");
        if !color.is_empty() && color != "단일" {
            parts.push(color);
        }
    }
    let option = field(variant, "option");
    if !option.trim().is_empty() { parts.push(option); }
    parts.join(" / ")
}

fn category_choices(catalog: &Value) -> Vec<(String,String)> {
    // These two virtual roots are also supplied by the original native
    // category query; they are choices, not catalog writes or new products.
    let mut categories = rows(&catalog["categories"]).to_vec();
    for (id,name) in [
        ("catalog-root:BANDOSTAND","반도스탠드"),
        ("catalog-root:BANDOMALL","반도몰"),
    ] {
        if !categories.iter().any(|row|field(row,"id")==id) {
            categories.push(json!({"id":id,"name":name,"parentId":null}));
        }
    }
    let by_id:HashMap<String,&Value> = categories.iter()
        .filter_map(|row|{
            let id=field(row,"id");
            (!id.is_empty()).then_some((id.to_owned(),row))
        }).collect();
    let mut choices=Vec::new();
    for category in &categories {
        let id=field(category,"id");
        if id.is_empty() { continue; }
        let mut names=Vec::new();
        let mut current=id;
        let mut seen=std::collections::HashSet::new();
        while !current.is_empty() && seen.insert(current.to_owned()) {
            let Some(row)=by_id.get(current) else { break; };
            names.push(field(row,"name").to_owned());
            current=field(row,"parentId");
        }
        names.reverse();
        choices.push((id.to_owned(),names.join(" > ")));
    }
    choices.sort_by(|a,b|a.1.cmp(&b.1));
    choices
}

/// Selects exactly the IDs returned by catalog_clients::management_products.
/// No independent filtering by name, category, brand, active state or price.
fn query_catalog(
    gateway:&dyn BandoGateway,
    cached: Option<Value>,
    search:&str, category:&str, brand:&str, missing:bool
) -> Result<CatalogRows,String> {
    let catalog = match cached {
        Some(value)=>value,
        None=>gateway.load_catalog()?,
    };
    let response=gateway.execute(ReadOperation::ManagementProducts,json!({
        "state":&catalog, "query":search, "category":category,
        "brand":brand, "missing":missing,
    }))?;
    let ids=response.as_array()
        .ok_or("Rust 상품관리 조회 결과가 ID 배열이 아닙니다.")?;
    let indexed:HashMap<String,&Value>=rows(&catalog["products"]).iter()
        .filter_map(|row|{
            let id=field(row,"id");
            (!id.is_empty()).then_some((id.to_owned(),row))
        }).collect();
    let mut products=Vec::with_capacity(ids.len());
    for id in ids {
        let key=id.as_str().ok_or("Rust 상품 ID 형식 오류")?;
        let item=indexed.get(key).ok_or("Rust 상품 조회 결과에 존재하지 않는 상품 ID가 있습니다.")?;
        products.push((*item).clone());
    }
    Ok(CatalogRows{catalog,products})
}

struct PriceDraft {
    original:Option<i64>,
    input:String,
}
impl PriceDraft {
    fn dirty(&self)->bool{
        let candidate=if self.input.trim().is_empty(){None}
            else{self.input.trim().replace(',', "").parse::<i64>().ok()};
        candidate!=self.original || (
            !self.input.trim().is_empty() && candidate.is_none()
        )
    }
    fn parse(&self)->Result<Option<i64>,String>{
        let input=self.input.trim().replace(',', "");
        if input.is_empty(){return Ok(None);}
        let amount=input.parse::<i64>()
            .map_err(|_|"금액은 숫자만 입력하세요.".to_owned())?;
        if amount<0 || amount>9_007_199_254_740_991 {
            return Err("가격은 0원 이상 안전한 정수 범위로 입력하세요.".into());
        }
        Ok(Some(amount))
    }
}

struct ProductWrite {
    kind:String,
    key:Option<String>,
    receiver:Receiver<Result<Value,String>>,
}

pub struct ProductManager {
    gateway:Arc<dyn BandoGateway>,
    catalog:Option<Value>,
    products:Vec<Value>,
    query:String,
    tab:usize,
    category_manager:CategoryManager,
    category:String,
    brand:String,
    missing:bool,
    selected:Option<Value>,
    adding:bool,
    original_selected:Option<Value>,
    detail_prices:HashMap<String,String>,
    confirm_discard:bool,
    confirm_archive:bool,
    price_mode:bool,
    price_drafts:HashMap<String,PriceDraft>,
    writing:Option<ProductWrite>,
    notice:Option<String>,
    result:Option<Receiver<SearchReply>>,
    generation:u64,
    next_query:Option<Instant>,
    error:Option<String>,
}

impl ProductManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let mut page=Self {
            gateway,catalog:None,products:Vec::new(),
            query:String::new(),tab:0,
            category_manager:CategoryManager::default(),category:String::new(),
            brand:String::new(),missing:false,selected:None,
            adding:false,original_selected:None,detail_prices:HashMap::new(),
            confirm_discard:false,confirm_archive:false,
            price_mode:false,price_drafts:HashMap::new(),
            writing:None,notice:None,
            result:None,generation:0,next_query:None,error:None,
        };
        page.reload(ctx);
        page
    }
    pub fn refresh(&mut self,ctx:Context) {
        // Re-read an explicitly connected operating catalog when returning
        // from another page. Never retain a stale edit/master snapshot.
        self.catalog=None;
        self.selected=None;
        self.original_selected=None;
        self.detail_prices.clear();
        self.confirm_archive=false;
        self.confirm_discard=false;
        self.next_query=None;
        self.reload(ctx);
    }
    fn reload(&mut self,ctx:Context) {
        self.generation=self.generation.wrapping_add(1);
        let generation=self.generation;
        let gateway=Arc::clone(&self.gateway);
        let cached=self.catalog.clone();
        let query=self.query.clone();
        let category=self.category.clone();
        let brand=self.brand.clone();
        let missing=self.missing;
        let (tx,rx)=mpsc::channel();
        self.result=Some(rx);
        self.error=None;
        std::thread::spawn(move||{
            let value=query_catalog(
                gateway.as_ref(),cached,&query,&category,&brand,missing,
            );
            let _=tx.send((generation,value));
            ctx.request_repaint();
        });
    }
    fn has_pending_prices(&self)->bool{
        self.price_drafts.values().any(PriceDraft::dirty)
    }
    pub fn blocks_navigation(&self)->bool{
        self.selected.is_some() || self.has_pending_prices()
            || self.writing.is_some()
    }
    fn start_write(
        &mut self,action:&str,data:Value,key:Option<String>,ctx:Context
    ){
        if self.writing.is_some(){return;}
        let Some(revision)=self.catalog.as_ref()
            .and_then(|catalog|catalog["revision"].as_u64()) else{
            self.error=Some("운영 상품 리비전을 확인할 수 없습니다.".into());
            return;
        };
        if !self.gateway.product_write_enabled(){
            self.error=Some("상품 변경 저장 권한이 없습니다.".into());
            return;
        }
        let gateway=Arc::clone(&self.gateway);
        let name=action.to_owned();
        let (tx,rx)=mpsc::channel();
        self.writing=Some(ProductWrite{
            kind:name.clone(),key,receiver:rx,
        });
        self.error=None;
        self.notice=None;
        std::thread::spawn(move||{
            let result=gateway.mutate_product(revision,&name,data);
            let _=tx.send(result);
            ctx.request_repaint();
        });
    }
    fn changed(&mut self,ctx:&Context) {
        self.next_query=Some(Instant::now()+Duration::from_millis(150));
        ctx.request_repaint_after(Duration::from_millis(160));
    }
    fn poll(&mut self,ctx:&Context) {
        let written=self.writing.as_ref().map(|job|job.receiver.try_recv());
        match written {
            Some(Ok(Ok(catalog)))=>{
                if let Some(job)=self.writing.take(){
                    if let Some(key)=job.key {
                        self.price_drafts.remove(&key);
                    }
                    if job.kind=="upsert_product" || job.kind=="archive_product" {
                        self.selected=None;
                        self.original_selected=None;
                        self.detail_prices.clear();
                        self.confirm_archive=false;
                        self.confirm_discard=false;
                    }
                    if matches!(job.kind.as_str(),"change_category"|"delete_category"){
                        self.category_manager.on_saved(&job.kind,&catalog);
                        if self.category_manager.take_switch_to_list(){
                            self.tab=0;
                        }
                    }
                    self.notice=Some(match job.kind.as_str(){
                        "set_price"=>"가격 변경을 저장했습니다.",
                        "upsert_product"=>"상품 상세 정보를 저장했습니다.",
                        "archive_product"=>"상품을 삭제하고 과거 기록을 보존했습니다.",
                        "change_category"=>"카테고리 변경사항을 저장했습니다.",
                        "delete_category"=>"빈 분류를 삭제했습니다.",
                        _=>"상품 변경을 저장했습니다.",
                    }.into());
                    self.catalog=Some(catalog);
                    self.reload(ctx.clone());
                }
            }
            Some(Ok(Err(error)))=>{
                self.writing=None;
                if self.tab==1 {
                    self.category_manager.report_error(error.clone());
                }
                self.error=Some(error);
            }
            Some(Err(TryRecvError::Disconnected))=>{
                self.writing=None;
                self.error=Some("상품 저장 도우미 응답이 중단됐습니다. 재조회 후 확인하세요.".into());
            }
            _=>{}
        }
        if let Some(at)=self.next_query {
            if Instant::now()>=at {
                self.next_query=None;
                self.reload(ctx.clone());
            }else{
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }
        let received=self.result.as_ref().map(|rx|rx.try_recv());
        match received {
            Some(Ok((generation,result)))=>{
                if generation==self.generation {
                    match result {
                        Ok(result)=>{
                            self.catalog=Some(result.catalog);
                            self.products=result.products;
                            self.error=None;
                        }
                        Err(e)=>self.error=Some(e),
                    }
                }
                self.result=None;
            }
            Some(Err(TryRecvError::Disconnected))=>{
                self.result=None;
                self.error=Some("Rust 상품 목록 조회가 중단되었습니다.".to_owned());
            }
            Some(Err(TryRecvError::Empty))=>{
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            None=>{}
        }
    }
    pub fn has_modal(&self)->bool {
        self.selected.is_some() ||
            (self.tab==1 && self.category_manager.has_overlay())
    }
    pub fn close_detail(&mut self){
        self.selected=None;
        self.original_selected=None;
        self.detail_prices.clear();
        self.confirm_archive=false;
        self.confirm_discard=false;
    }
    pub fn allow_leave(&mut self)->bool{
        if self.writing.is_some(){
            self.error=Some("상품 저장이 완료된 뒤 다른 화면으로 이동하세요.".into());
            return false;
        }
        if self.selected.is_some(){return false;}
        if self.has_pending_prices(){
            self.error=Some("가격 셀의 변경사항을 저장하거나 취소한 뒤 이동하세요.".into());
            return false;
        }
        if self.tab==1 {
            if let Some(catalog)=self.catalog.as_ref(){
                if !self.category_manager.request_global_leave(catalog){
                    return false;
                }
            }
            self.category_manager.clear_for_leave();
        }
        true
    }
    pub fn take_category_global_leave(&mut self)->bool{
        self.category_manager.take_global_leave_approved()
    }
    pub fn category_global_leave_in_progress(&self)->bool{
        self.category_manager.global_leave_in_progress()
    }
    fn begin_edit(&mut self,product:Value){
        if self.has_pending_prices(){
            self.error=Some("가격 셀의 수정값부터 저장하거나 취소하세요.".into());
            return;
        }
        self.detail_prices.clear();
        if let Some(variant)=rows(&product["variants"]).first() {
            for tier in ["wholesale","semi","retail"] {
                self.detail_prices.insert(tier.to_owned(),
                    variant[tier].as_i64().map(|v|v.to_string())
                    .unwrap_or_default());
            }
        }
        self.original_selected=Some(product.clone());
        self.selected=Some(product);
        self.confirm_discard=false;
        self.confirm_archive=false;
    }
    fn new_product(&mut self){
        let at=chrono::Utc::now().to_rfc3339();
        self.begin_edit(json!({
            "id":"","code":"","name":"","subtitle":"",
            "category":"","categories":[],"catalog":"BANDOSTAND",
            "origin":"FACTORY","defaultColor":"단일",
            "active":true,"saleUnit":true,"colorStatus":"NOT_REQUIRED",
            "variants":[{
                "id":"","color":"단일","option":"",
                "wholesale":null,"semi":null,"retail":null,
                "url":"","source":"MANUAL"
            }],
            "createdAt":at,"updatedAt":at
        }));
    }

    pub fn show_page(&mut self,ui:&mut Ui){
        self.poll(ui.ctx());
        ui.label(typography::page_heading("상품 관리"));
        ui.add_space(theme::SPACE_12);
        let pressed=components::management_tabs(ui,&[
            ("상품 목록",self.tab==0,true),
            ("카테고리 관리",self.tab==1,true),
            ("모델군 관리",false,false),
            ("별칭 관리",false,false),
            ("브랜드 관리",false,false),
        ]);
        if let Some(next)=pressed {
            if next!=self.tab && self.writing.is_none() &&
                !self.selected.is_some()&&!self.has_pending_prices(){
                if self.tab==1 && next==0 {
                    let allowed=self.catalog.as_ref()
                        .is_none_or(|catalog|
                            self.category_manager.request_tab_leave(catalog));
                    if allowed {
                        self.tab=0;
                        self.query.clear();
                        self.category_manager.clear_for_leave();
                        self.category_manager.clear_search();
                        self.reload(ui.ctx().clone());
                    }
                }else if next==1 {
                    self.tab=1;
                    self.query.clear();
                    self.category_manager.clear_search();
                }
            }else if self.has_pending_prices(){
                self.error=Some("가격 수정값을 저장하거나 취소한 뒤 탭을 이동하세요.".into());
            }
        }
        if self.category_manager.take_switch_to_list(){
            self.tab=0;
            self.query.clear();
            self.reload(ui.ctx().clone());
        }
        ui.add_space(theme::SPACE_12);
        let can_edit=self.gateway.product_write_enabled()
            && self.catalog.as_ref().is_some_and(|c|c["revision"].as_u64().is_some());
        let saving=self.writing.is_some();
        let mut price_write:Option<(String,Value)>=None;
        let mut open_detail:Option<Value>=None;
        let mut begin_new=false;
        if self.tab==1 {
            if let Some(catalog)=self.catalog.as_ref(){
                if let Some(error)=&self.error{
                    ui.label(typography::status(error,theme::DANGER));
                }else if let Some(notice)=&self.notice{
                    ui.label(typography::status(notice,theme::SUCCESS));
                }
                self.category_manager.show_page(ui,catalog,can_edit,saving);
            } else {
                ui.label(typography::caption("카테고리 목록을 불러오는 중입니다."));
            }
            return;
        }
        theme::section_frame().show(ui,|panel|{
            panel.horizontal(|header|{
                header.label(typography::section_heading("상품 가격표"));
                header.label(typography::caption(&format!(
                    "{}개 상품",self.products.len()
                )));
                header.with_layout(Layout::right_to_left(Align::Center),|actions|{
                    if theme::action(
                        actions,"상품 추가",theme::ButtonRole::Primary,!saving
                    ).clicked(){
                        self.adding=!self.adding;
                    }
                    if theme::action(actions,
                        if self.price_mode {"가격 수정 모드 종료"}else{"가격 수정 모드 켜기"},
                        if self.price_mode {theme::ButtonRole::Primary}
                        else {theme::ButtonRole::Standard},
                        can_edit && !saving
                    ).clicked(){
                        if self.price_mode && self.has_pending_prices(){
                            self.error=Some("저장하지 않은 가격이 있습니다. 각 가격 셀의 저장 또는 취소를 눌러주세요.".into());
                        }else{
                            self.price_mode=!self.price_mode;
                            if !self.price_mode {self.price_drafts.clear();}
                        }
                    }
                    theme::action(actions,"Excel 관리",theme::ButtonRole::Standard,false)
                        .on_disabled_hover_text("Excel 관리 기능이 이전되지 않았습니다.");
                });
            });
            components::soft_divider(panel);
            if self.adding {
                panel.horizontal(|actions|{
                    if theme::action(
                        actions,"직접 등록",theme::ButtonRole::Primary,
                        can_edit&&!saving
                    ).clicked(){
                        begin_new=true;
                    }
                });
                components::soft_divider(panel);
            }
            panel.horizontal_wrapped(|filter|{
                filter.label(typography::field_label("상품 검색"));
                let before=self.query.clone();
                if components::flexible_text_field(
                    filter,&mut self.query,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                    "모델명 또는 상품명 검색"
                ).changed(){
                    if self.has_pending_prices(){
                        self.query=before;
                        self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                    }else{self.changed(filter.ctx());}
                }
            });
            let categories=self.catalog.as_ref()
                .map(category_choices).unwrap_or_default();
            let brands=self.catalog.as_ref().map(|catalog|
                rows(&catalog["brands"]).to_vec()).unwrap_or_default();
            panel.horizontal_wrapped(|filters|{
                let before_category=self.category.clone();
                let selected=categories.iter().find(|(id,_)|id==&self.category)
                    .map(|(_,name)|name.as_str()).unwrap_or("전체 카테고리");
                egui::ComboBox::from_id_salt("product-admin-category")
                    .selected_text(selected)
                    .show_ui(filters,|combo|{
                        combo.selectable_value(&mut self.category,
                            String::new(),"전체 카테고리");
                        for (id,name) in &categories {
                            combo.selectable_value(&mut self.category,id.clone(),name);
                        }
                    });
                if self.category!=before_category {
                    if self.has_pending_prices(){
                        self.category=before_category;
                        self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                    }else{self.changed(filters.ctx());}
                }
                let before_brand=self.brand.clone();
                let brand_text=brands.iter().find(|b|field(b,"id")==self.brand)
                    .map(|b|field(b,"name")).unwrap_or("전체 브랜드");
                egui::ComboBox::from_id_salt("product-admin-brand")
                    .selected_text(brand_text)
                    .show_ui(filters,|combo|{
                        combo.selectable_value(&mut self.brand,
                            String::new(),"전체 브랜드");
                        for brand in &brands {
                            combo.selectable_value(&mut self.brand,
                                field(brand,"id").to_owned(),field(brand,"name"));
                        }
                    });
                if self.brand!=before_brand {
                    if self.has_pending_prices(){
                        self.brand=before_brand;
                        self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                    }else{self.changed(filters.ctx());}
                }
                let before_missing=self.missing;
                if filters.checkbox(&mut self.missing,"가격 미등록만").changed(){
                    if self.has_pending_prices(){
                        self.missing=before_missing;
                        self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                    }else{self.changed(filters.ctx());}
                }
            });
            components::soft_divider(panel);
            if let Some(error)=&self.error {
                panel.label(typography::status(error,theme::DANGER));
            }else if let Some(notice)=&self.notice{
                panel.label(typography::status(notice,theme::SUCCESS));
            }
            if saving {
                panel.label(typography::caption("상품 변경 저장 중…"));
            }
            if self.result.is_some() {
                panel.label(typography::caption("상품 가격표 조회 중…"));
            }
            egui::ScrollArea::both()
                .id_salt("egui-product-price-grid")
                .auto_shrink([false,false])
                .show(panel,|table|{
                    table.set_min_width(theme::MANAGEMENT_TABLE_MIN_WIDTH);
                    egui::Grid::new("egui-products-price-table")
                        .striped(true)
                        .spacing([theme::SPACE_24,theme::SPACE_8])
                        .show(table,|grid|{
                            for heading in [
                                "상품명 / 상품설명","도매가","반도매가",
                                "소매가","상세"
                            ] {grid.label(typography::table_header(heading));}
                            grid.end_row();
                            let products=&self.products;
                            for product in products {
                                let name=field(product,"name");
                                let subtitle=field(product,"subtitle");
                                for variant in rows(&product["variants"]) {
                                    grid.vertical(|cell|{
                                        cell.label(typography::table_cell(name));
                                        if !subtitle.is_empty() {
                                            cell.label(typography::caption(subtitle));
                                        }
                                        let detail=option_text(product,variant);
                                        if !detail.is_empty() {
                                            cell.label(typography::caption(&detail));
                                        }
                                    });
                                    for tier in ["wholesale","semi","retail"] {
                                        if self.price_mode {
                                            let key=format!(
                                                "{}:{}:{}",field(product,"id"),
                                                field(variant,"id"),tier
                                            );
                                            let original=variant[tier].as_i64();
                                            let entry=self.price_drafts.entry(key.clone())
                                                .or_insert_with(||PriceDraft{
                                                    original,
                                                    input:original.map(|v|v.to_string())
                                                        .unwrap_or_default(),
                                                });
                                            grid.horizontal(|cell|{
                                                cell.add_enabled_ui(!saving,|inputs|{
                                                    let response=components::money_field(
                                                        inputs,&mut entry.input,true,
                                                    );
                                                    let enter=response.lost_focus() &&
                                                        inputs.input(|i|i.key_pressed(egui::Key::Enter));
                                                    if response.has_focus() &&
                                                        inputs.input(|i|i.key_pressed(egui::Key::Escape)){
                                                        entry.input=entry.original.map(|n|n.to_string())
                                                            .unwrap_or_default();
                                                    }
                                                    if entry.dirty() {
                                                        if theme::action(inputs,"저장",
                                                            theme::ButtonRole::Standard,true
                                                        ).clicked() || enter {
                                                            match entry.parse(){
                                                                Ok(value)=>{
                                                                    price_write=Some((key.clone(),
                                                                        json!({
                                                                            "productId":field(product,"id"),
                                                                            "variantId":field(variant,"id"),
                                                                            "tier":tier,
                                                                            "price":value,
                                                                            "expected":entry.original,
                                                                        })
                                                                    ));
                                                                }
                                                                Err(message)=>self.error=Some(message),
                                                            }
                                                        }
                                                        if theme::action(inputs,"취소",
                                                            theme::ButtonRole::Quiet,true
                                                        ).clicked(){
                                                            entry.input=entry.original
                                                                .map(|n|n.to_string())
                                                                .unwrap_or_default();
                                                        }
                                                    }
                                                });
                                            });
                                        }else {
                                            grid.label(typography::quantity(
                                                &price(&variant[tier])
                                            ));
                                        }
                                    }
                                    if theme::action(
                                        grid,"상세 관리",theme::ButtonRole::Standard,!saving
                                    ).clicked(){
                                        open_detail=Some(product.clone());
                                    }
                                    grid.end_row();
                                }
                            }
                        });
                    if self.products.is_empty() && self.result.is_none()
                        && self.error.is_none(){
                        table.label(typography::caption(
                            if self.query.is_empty() && self.category.is_empty()
                                && self.brand.is_empty() && !self.missing {
                                "등록된 상품이 없습니다."
                            } else {
                                "조건에 맞는 상품이 없습니다."
                            }
                        ));
                    }
                });
        });
        if let Some((key,data))=price_write {
            self.start_write("set_price",data,Some(key),ui.ctx().clone());
        }
        if let Some(product)=open_detail {
            self.begin_edit(product);
        }else if begin_new {
            self.new_product();
        }
    }
    fn edit_text(ui:&mut Ui,draft:&mut Value,key:&str,label:&str,enabled:bool){
        ui.horizontal_wrapped(|line|{
            line.label(typography::field_label(label));
            let mut value=field(draft,key).to_owned();
            line.add_enabled_ui(enabled,|input|{
                if components::flexible_text_field(
                    input,&mut value,theme::MANAGEMENT_SEARCH_MIN_WIDTH,label
                ).changed(){
                    draft[key]=json!(value);
                }
            });
        });
    }
    fn product_price_candidate(
        draft:&Value,inputs:&HashMap<String,String>,
    )->Result<Value,String>{
        let mut next=draft.clone();
        let variant=next["variants"].as_array_mut()
            .and_then(|rows|rows.first_mut())
            .ok_or("상품의 단일 판매 구성을 확인할 수 없습니다.")?;
        for tier in ["wholesale","semi","retail"]{
            let raw=inputs.get(tier).ok_or("상품 가격 입력이 없습니다.")?
                .trim().replace(',', "");
            if raw.is_empty(){
                variant[tier]=Value::Null;
            }else{
                let price=raw.parse::<i64>()
                    .map_err(|_|"가격은 숫자만 입력하세요.".to_owned())?;
                if !(0..=9_007_199_254_740_991).contains(&price){
                    return Err("가격은 0원 이상 안전한 정수 범위로 입력하세요.".into());
                }
                variant[tier]=json!(price);
            }
        }
        Ok(next)
    }
    pub fn show_detail(&mut self,ctx:&Context,work_area:egui::Rect){
        if self.tab==1 {
            if let Some(catalog)=self.catalog.clone(){
                let can_edit=self.gateway.product_write_enabled()
                    && catalog["revision"].as_u64().is_some();
                if let Some(request)=self.category_manager.show_overlays(
                    ctx,work_area,&catalog,can_edit,self.writing.is_some()
                ){
                    self.start_write(request.action,request.data,None,ctx.clone());
                }
            }
            return;
        }
        let Some(mut draft)=self.selected.clone()else{return};
        let original=self.original_selected.clone().unwrap_or_else(||draft.clone());
        let editable=self.gateway.product_write_enabled()
            && self.catalog.as_ref().is_some_and(|v|v["revision"].as_u64().is_some());
        let busy=self.writing.is_some();
        let is_new=field(&draft,"id").is_empty();
        let categories=self.catalog.as_ref().map(category_choices).unwrap_or_default();
        let brands=self.catalog.as_ref().map(|state|
            rows(&state["brands"]).to_vec()).unwrap_or_default();
        let groups=self.catalog.as_ref().map(|state|
            rows(&state["shipping"]["groups"]).iter()
                .filter(|g|g.get("deletedAt").is_none_or(Value::is_null))
                .cloned().collect::<Vec<_>>()).unwrap_or_default();
        let mut prices=self.detail_prices.clone();
        let mut request_close=false;
        let mut request_save=false;
        let mut request_archive=false;
        let saved_error=self.error.clone();
        components::modal_backdrop(ctx,"egui-product-detail-backdrop",false);
        components::modal_window(
            if is_new {"상품 등록"} else {"상품 상세 관리"},
            "egui-product-detail",work_area
        ).resizable(false)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|modal|{
                egui::ScrollArea::vertical()
                    .id_salt("egui-product-detail-content")
                    .max_height(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
                    .show(modal,|body|{
                        body.label(typography::section_heading("상품 정보"));
                        Self::edit_text(body,&mut draft,"name","상품명",editable&&!busy);
                        Self::edit_text(body,&mut draft,"code","모델명",editable&&!busy);
                        Self::edit_text(body,&mut draft,"subtitle","상품설명",editable&&!busy);
                        body.horizontal_wrapped(|line|{
                            line.label(typography::field_label("카테고리"));
                            let before=draft["categories"].as_array()
                                .and_then(|ids|ids.last())
                                .and_then(Value::as_str).unwrap_or("").to_owned();
                            let mut category_id=before.clone();
                            let display=categories.iter()
                                .find(|(id,_)|id==&category_id)
                                .map(|(_,name)|name.as_str())
                                .unwrap_or("카테고리 미지정");
                            line.add_enabled_ui(editable&&!busy,|input|{
                                egui::ComboBox::from_id_salt("product-detail-category")
                                    .selected_text(display)
                                    .show_ui(input,|choices|{
                                        choices.selectable_value(&mut category_id,
                                            String::new(),"카테고리 미지정");
                                        for (id,label) in &categories{
                                            choices.selectable_value(
                                                &mut category_id,id.clone(),label
                                            );
                                        }
                                    });
                            });
                            if category_id!=before{
                                draft["categories"]=if category_id.is_empty(){
                                    json!([])
                                }else{json!([category_id.clone()])};
                                draft["category"]=json!(categories.iter().find(|(id,_)|
                                    id==&category_id).map(|(_,name)|name.as_str())
                                    .unwrap_or(""));
                            }
                        });
                        body.horizontal_wrapped(|line|{
                            line.label(typography::field_label("브랜드"));
                            let mut brand_id=field(&draft,"brandId").to_owned();
                            let before=brand_id.clone();
                            let label=brands.iter().find(|b|field(b,"id")==brand_id)
                                .map(|b|field(b,"name")).unwrap_or("브랜드 미지정");
                            line.add_enabled_ui(editable&&!busy,|input|{
                                egui::ComboBox::from_id_salt("product-detail-brand")
                                    .selected_text(label).show_ui(input,|choices|{
                                        choices.selectable_value(&mut brand_id,
                                            String::new(),"브랜드 미지정");
                                        for brand in &brands{
                                            choices.selectable_value(&mut brand_id,
                                                field(brand,"id").to_owned(),
                                                field(brand,"name"));
                                        }
                                    });
                            });
                            if brand_id!=before{
                                draft["brandId"]=if brand_id.is_empty(){
                                    Value::Null
                                }else{json!(brand_id)};
                            }
                        });
                        body.horizontal_wrapped(|line|{
                            line.label(typography::field_label("발송지"));
                            let mut origin=field(&draft,"origin").to_owned();
                            let before=origin.clone();
                            line.add_enabled_ui(editable&&!busy,|input|{
                                egui::ComboBox::from_id_salt("product-detail-origin")
                                    .selected_text(if origin=="STORE"{"매장"}else{"공장"})
                                    .show_ui(input,|options|{
                                        options.selectable_value(
                                            &mut origin,"FACTORY".into(),"공장");
                                        options.selectable_value(
                                            &mut origin,"STORE".into(),"매장");
                                    });
                            });
                            if origin!=before{draft["origin"]=json!(origin);}
                        });
                        body.horizontal_wrapped(|line|{
                            line.label(typography::field_label("배송 그룹"));
                            let before=field(&draft,"shippingGroupId").to_owned();
                            let mut group=before.clone();
                            let label=groups.iter().find(|g|field(g,"id")==group)
                                .map(|g|field(g,"name")).unwrap_or("미지정");
                            line.add_enabled_ui(editable&&!busy,|input|{
                                egui::ComboBox::from_id_salt("product-detail-shipping")
                                    .selected_text(label).show_ui(input,|choices|{
                                        choices.selectable_value(
                                            &mut group,String::new(),"미지정");
                                        for row in &groups{
                                            choices.selectable_value(
                                                &mut group,field(row,"id").to_owned(),
                                                field(row,"name"));
                                        }
                                    });
                            });
                            if group!=before{
                                draft["shippingGroupId"]=if group.is_empty(){
                                    Value::Null
                                }else{json!(group)};
                            }
                        });
                        Self::edit_text(body,&mut draft,"defaultColor","기본 색상",
                            editable&&!busy);
                        body.add_space(theme::SPACE_12);
                        components::soft_divider(body);
                        body.label(typography::section_heading("단일 판매 구성"));
                        if let Some(variant)=draft["variants"].as_array_mut()
                            .and_then(|rows|rows.first_mut()){
                            Self::edit_text(body,variant,"color","판매 색상",editable&&!busy);
                            Self::edit_text(body,variant,"option","판매 옵션",editable&&!busy);
                            Self::edit_text(body,variant,"url","홈페이지 상품 연결",
                                editable&&!busy);
                        }
                        body.add_space(theme::SPACE_12);
                        body.label(typography::section_heading("상품 가격"));
                        for (label,tier) in [
                            ("도매가","wholesale"),("반도매가","semi"),
                            ("소매가","retail")
                        ]{
                            body.horizontal(|line|{
                                line.label(typography::field_label(label));
                                let text=prices.entry(tier.to_owned())
                                    .or_insert_with(String::new);
                                line.add_enabled_ui(editable&&!busy,|input|{
                                    components::money_field(input,text,true);
                                });
                            });
                        }
                        body.add_space(theme::SPACE_12);
                        if !is_new{
                            let refs=rows(&draft["priceHistory"]);
                            egui::CollapsingHeader::new(
                                format!("가격 변경 이력 {}건",refs.len())
                            ).show(body,|history|{
                                for item in refs.iter().rev(){
                                    history.label(typography::caption(
                                        &format!("{} / {} / {} → {}",
                                            field(item,"at"),field(item,"field"),
                                            item["before"],item["after"]
                                        )
                                    ));
                                }
                            });
                        }
                        if let Some(message)=&saved_error{
                            body.label(typography::status(message,theme::DANGER));
                        }
                    });
                modal.add_space(theme::SPACE_12);
                modal.horizontal(|footer|{
                    if !is_new && theme::action(
                        footer,"상품 삭제",theme::ButtonRole::Danger,
                        editable&&!busy
                    ).clicked(){
                        request_archive=true;
                    }
                    footer.with_layout(Layout::right_to_left(Align::Center),|right|{
                        let price_changed=prices.iter().any(|(tier,raw)|{
                            let before=original["variants"].as_array()
                                .and_then(|v|v.first())
                                .and_then(|v|v.get(tier))
                                .and_then(Value::as_i64)
                                .map(|n|n.to_string()).unwrap_or_default();
                            raw.trim().replace(',', "")!=before
                        });
                        if theme::action(right,"변경사항 저장",
                            theme::ButtonRole::Primary,
                            editable&&!busy&&
                                (is_new || draft!=original || price_changed)
                        ).clicked(){request_save=true;}
                        if theme::action(right,"닫기",theme::ButtonRole::Standard,!busy)
                            .clicked(){request_close=true;}
                    });
                });
            });
        self.selected=Some(draft.clone());
        self.detail_prices=prices;
        if request_save{
            match Self::product_price_candidate(&draft,&self.detail_prices){
                Ok(candidate)=>self.start_write(
                    "upsert_product",json!({"product":candidate}),None,ctx.clone()
                ),
                Err(message)=>self.error=Some(message),
            }
        }
        if request_archive {self.confirm_archive=true;}
        if request_close {
            let candidate=Self::product_price_candidate(&draft,&self.detail_prices).ok();
            if candidate.as_ref().is_some_and(|changed|changed!=&original){
                self.confirm_discard=true;
            }else{
                self.close_detail();
            }
        }
        if self.confirm_archive{
            let mut cancel=false;
            let mut archive=false;
            components::modal_backdrop(ctx,"egui-product-archive-nested",true);
            components::decision_window(
                "상품 삭제","egui-product-archive-confirm",work_area
            ).show(ctx,|dialog|{
                dialog.label(typography::body(
                    "등록 상품에서 삭제할까요? 과거 주문과 가격 기록은 보존하며 홈페이지 재수집으로 자동 복원하지 않습니다."
                ));
                dialog.horizontal(|actions|{
                    if theme::action(actions,"취소",theme::ButtonRole::Standard,!busy)
                        .clicked(){cancel=true;}
                    if theme::action(actions,"상품 삭제",theme::ButtonRole::Danger,
                        editable&&!busy).clicked(){archive=true;}
                });
            });
            if cancel{self.confirm_archive=false;}
            else if archive{
                self.confirm_archive=false;
                self.start_write("archive_product",
                    json!({"productId":field(&draft,"id")}),None,ctx.clone()
                );
            }
        }else if self.confirm_discard {
            let mut keep=false;
            let mut discard=false;
            components::modal_backdrop(ctx,"egui-product-discard-nested",true);
            components::decision_window(
                "저장하지 않은 상품","egui-product-discard-confirm",work_area
            ).show(ctx,|dialog|{
                dialog.label(typography::body(
                    "상품 수정사항을 저장하지 않고 닫을까요?"
                ));
                dialog.horizontal(|actions|{
                    if theme::action(actions,"계속 수정",
                        theme::ButtonRole::Standard,!busy).clicked(){keep=true;}
                    if theme::action(actions,"저장 안 함",
                        theme::ButtonRole::Danger,!busy).clicked(){discard=true;}
                });
            });
            if keep {self.confirm_discard=false;}
            else if discard{self.close_detail();}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_options_do_not_create_mutable_catalog_entries(){
        let catalog=json!({"categories":[
            {"id":"a","name":"보면대","parentId":"catalog-root:BANDOSTAND"}]});
        let ids=category_choices(&catalog);
        assert!(ids.iter().any(|(id,name)|id=="a"&&name.contains("보면대")));
        assert!(ids.iter().any(|(id,_)|id=="catalog-root:BANDOSTAND"));
        assert!(catalog["categories"].as_array().is_some_and(|v|v.len()==1));
    }
}