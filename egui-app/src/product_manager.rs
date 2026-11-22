//! React ProductManager 상품 목록: Rust-owned catalog selection and a
//! read-only price/detail projection. Product and price writes must not target
//! the isolated egui preview DB or bypass Tauri's operational save boundary.
use crate::{
    category_manager::{CategoryManager,ROOTS},
    alias_manager::AliasManager,
    brand_manager::BrandManager,
    model_group_manager::ModelGroupManager,
    components,
    gateway::BandoGateway,
    management_queries::{self, ManagementProductRows},
    latest_job::LatestJob,
    product_excel_ui::ProductExcelUi,
    product_images::{self,ProductImageCache},
    settings_help, theme, typography,
};
use eframe::egui::{self, Align, Context, Layout, Sense, Ui, UiBuilder, Rect, Pos2, Vec2};
use serde_json::{json, Value};
use std::{
    cmp::Ordering,
    collections::HashMap,
    sync::{mpsc::{self, Receiver, TryRecvError}, Arc},
    time::{Duration, Instant},
};

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
        if !color.is_empty() && color != "단일" && color != "색상 미표기" {
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
    for (id,name) in ROOTS {
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


// A presentation-only three-step sort; the Rust ManagementProducts command
// still owns the filtered membership and the default result order.
#[derive(Clone,Copy,PartialEq,Eq)]
enum ProductSortColumn { Name, Option, Wholesale, Semi, Retail }
#[derive(Clone,Copy,PartialEq,Eq)]
enum ProductSortDirection { Ascending, Descending }
#[derive(Clone,Copy,PartialEq,Eq)]
struct ProductSort { column:ProductSortColumn, direction:ProductSortDirection }
impl ProductSort {
    fn column_at(index:usize)->Option<ProductSortColumn>{
        match index{
            0=>Some(ProductSortColumn::Name),
            1=>Some(ProductSortColumn::Option),
            2=>Some(ProductSortColumn::Wholesale),
            3=>Some(ProductSortColumn::Semi),
            4=>Some(ProductSortColumn::Retail),
            _=>None
        }
    }
    fn next(current:Option<Self>,column:ProductSortColumn)->Option<Self>{
        match current {
            Some(Self{column:c,direction:ProductSortDirection::Ascending})
                if c==column=>Some(Self{column,direction:ProductSortDirection::Descending}),
            Some(Self{column:c,direction:ProductSortDirection::Descending})
                if c==column=>None,
            _=>Some(Self{column,direction:ProductSortDirection::Ascending}),
        }
    }
}
fn product_table_cells(origin:Pos2,widths:&[f32;6],height:f32)->[Rect;6]{
    let mut x=origin.x;
    std::array::from_fn(|i|{
        let rect=Rect::from_min_size(
            egui::pos2(x,origin.y),egui::vec2(widths[i],height)
        );
        x+=widths[i]+theme::SPACE_8;
        rect
    })
}
fn product_table_text(ui:&mut Ui,rect:Rect,text:&str,
    role:typography::ManagementTableCellRole,unique:impl std::hash::Hash){
    let content=rect.shrink2(Vec2::new(theme::SPACE_8,0.0));
    let color=typography::management_table_cell_color(role);
    let galley=typography::management_table_cell_galley(ui,text,role);
    ui.painter().with_clip_rect(content.intersect(ui.clip_rect())).galley(
        egui::pos2(content.left(),content.center().y-galley.size().y/2.0),
        galley,color,
    );
    if !text.is_empty(){
        ui.interact(content.intersect(ui.clip_rect()),
            ui.make_persistent_id(("product-table-text",unique)),
            Sense::hover()).on_hover_text(text);
    }
}
fn product_price_text(ui:&mut Ui,rect:Rect,value:&Value) {
    let text=price(value);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).text(
        egui::pos2(rect.right()-theme::SPACE_8,rect.center().y),
        egui::Align2::RIGHT_CENTER,text,
        typography::management_table_money_font(),
        typography::management_table_cell_color(typography::ManagementTableCellRole::Money)
    );
}
fn displayed_variant<'a>(
    product:&'a Value,selected:&HashMap<String,String>
)->Option<&'a Value>{
    let variants=rows(&product["variants"]);
    let saved=selected.get(field(product,"id")).map(String::as_str);
    variants.iter().find(|v|saved==Some(field(v,"id"))).or_else(||variants.first())
}
fn product_sort_order(
    products:&[Value],selected:&HashMap<String,String>,sort:Option<ProductSort>
)->Vec<usize>{
    let mut order=(0..products.len()).collect::<Vec<_>>();
    if let Some(sort)=sort{
        order.sort_by(|&ai,&bi|{
            let (a,b)=(&products[ai],&products[bi]);
            let result=match sort.column{
                ProductSortColumn::Name=>field(a,"name").to_lowercase()
                    .cmp(&field(b,"name").to_lowercase()),
                ProductSortColumn::Option=>{
                    let left=displayed_variant(a,selected).map(|v|option_text(a,v)).unwrap_or_default();
                    let right=displayed_variant(b,selected).map(|v|option_text(b,v)).unwrap_or_default();
                    left.cmp(&right)
                },
                price_column=>{
                    let tier=match price_column{
                        ProductSortColumn::Wholesale=>"wholesale",
                        ProductSortColumn::Semi=>"semi",
                        ProductSortColumn::Retail=>"retail",
                        _=>unreachable!(),
                    };
                    let left=displayed_variant(a,selected).and_then(|v|v[tier].as_i64());
                    let right=displayed_variant(b,selected).and_then(|v|v[tier].as_i64());
                    left.cmp(&right)
                }
            };
            let result=if sort.direction==ProductSortDirection::Descending{
                result.reverse()
            }else{result};
            if result==Ordering::Equal{ai.cmp(&bi)}else{result}
        });
    }
    order
}

/// UI-only number conversion used by both table cells and detail editor.
/// The original React PriceCell explicitly maps 0 -> null; MoneyInput in
/// product detail does not. Native Rust remains the price mutation owner.
fn parse_product_price(raw:&str,field_name:&str,zero_unregistered:bool)
    ->Result<Option<i64>,String>{
    let input=raw.trim().replace(',', "");
    if input.is_empty(){return Ok(None);}
    let amount=input.parse::<i64>()
        .map_err(|_|format!("{field_name}은 숫자만 입력하세요."))?;
    if !(0..=9_007_199_254_740_991).contains(&amount){
        return Err("가격은 0원 이상 안전한 정수 범위로 입력하세요.".into());
    }
    if zero_unregistered && amount==0 {Ok(None)} else {Ok(Some(amount))}
}
struct PriceDraft {
    original:Option<i64>,
    input:String,
}
impl PriceDraft {
    fn dirty(&self)->bool{
        // Historic price rows may store literal 0. Their initial "0" must
        // stay pristine until an operator actually changes the field.
        let initial=self.original.map(|n|n.to_string()).unwrap_or_default();
        if self.input.trim().replace(',', "")==initial{return false;}
        match self.parse(){
            Ok(value)=>value!=self.original,
            Err(_)=>true,
        }
    }
    fn parse(&self)->Result<Option<i64>,String>{
        parse_product_price(&self.input,"금액",true)
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
    model_groups:ModelGroupManager,
    aliases:AliasManager,
    brands:BrandManager,
    category_next_tab:Option<usize>,
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
    selected_variants:HashMap<String,String>,
    sort:Option<ProductSort>,
    excel:ProductExcelUi,
    photos:ProductImageCache,
    writing:Option<ProductWrite>,
    notice:Option<String>,
    result:LatestJob<ManagementProductRows>,
    next_query:Option<Instant>,
    error:Option<String>,
}

impl ProductManager {
    pub fn active_tab_name(&self)->&'static str{
        ["상품 목록","카테고리 관리","모델군 관리","별칭 관리","브랜드 관리"]
            .get(self.tab).copied().unwrap_or("상품 관리")
    }
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let mut page=Self {
            gateway,catalog:None,products:Vec::new(),
            query:String::new(),tab:0,
            category_manager:CategoryManager::default(),
            model_groups:ModelGroupManager::default(),
            aliases:AliasManager::default(),
            brands:BrandManager::default(),category_next_tab:None,
            category:String::new(),
            brand:String::new(),missing:false,selected:None,
            adding:false,original_selected:None,detail_prices:HashMap::new(),
            confirm_discard:false,confirm_archive:false,
            price_mode:false,price_drafts:HashMap::new(),
            selected_variants:HashMap::new(),sort:None,
            excel:ProductExcelUi::default(),
            photos:ProductImageCache::default(),
            writing:None,notice:None,
            result:LatestJob::default(),next_query:None,error:None,
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
        self.photos.clear();
        self.selected_variants.clear();
        self.confirm_archive=false;
        self.confirm_discard=false;
        self.next_query=None;
        self.model_groups.reset_navigation();
        self.brands.reset_navigation();
        self.reload(ctx);
    }
    fn reload(&mut self,ctx:Context) {
        let gateway=Arc::clone(&self.gateway);
        let cached=self.catalog.clone();
        let query=self.query.clone();
        let category=self.category.clone();
        let brand=self.brand.clone();
        let missing=self.missing;
        self.error=None;
        self.result.submit(ctx,move||{
            management_queries::query_management_products(
                gateway.as_ref(),cached,&query,&category,&brand,missing,
            )
        });
    }
    fn has_pending_prices(&self)->bool{
        self.price_drafts.values().any(PriceDraft::dirty)
    }
    pub fn blocks_navigation(&self)->bool{
        self.selected.is_some() || self.has_pending_prices()
            || self.writing.is_some() || self.excel.blocks_navigation()
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
        if action!="set_price"{
            crate::floating_notice::dispatch_pointer(&ctx,"products.action");
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
        if let Some(catalog)=self.excel.poll(ctx){
            self.model_groups.set_catalog(&catalog,ctx);
            self.aliases.set_catalog(&catalog,ctx);
            self.catalog=Some(catalog);
            self.reload(ctx.clone());
        }
        if let Some(catalog)=self.model_groups.poll(ctx){
            self.model_groups.set_catalog(&catalog,ctx);
            self.aliases.set_catalog(&catalog,ctx);
            self.catalog=Some(catalog);
            self.reload(ctx.clone());
        }
        let written=self.writing.as_ref().map(|job|job.receiver.try_recv());
        match written {
            Some(Ok(Ok(catalog)))=>{
                if let Some(job)=self.writing.take(){
                    if let Some(key)=job.key {
                        self.price_drafts.remove(&key);
                    }
                    if job.kind=="save_alias" {
                        self.aliases.on_saved(&catalog);
                    }
                    if job.kind=="upsert_brand" {
                        self.brands.on_saved(&catalog);
                    }
                    if job.kind=="upsert_product" || job.kind=="archive_product" {
                        self.selected=None;
                        self.original_selected=None;
                        self.detail_prices.clear();
                        self.confirm_archive=false;
                        self.confirm_discard=false;
                    }
                    let category_notice=if matches!(
                        job.kind.as_str(),"change_category"|"delete_category"
                    ){
                        let notice=self.category_manager.on_saved(&job.kind,&catalog);
                        if self.category_manager.take_switch_to_list(){
                            self.tab=0;
                        }
                        Some(notice)
                    }else{None};
                    self.notice=Some(category_notice.unwrap_or(match job.kind.as_str(){
                        "set_price"=>"가격 변경을 저장했습니다.",
                        "upsert_product"=>"상품 상세 정보를 저장했습니다.",
                        "archive_product"=>"상품을 삭제하고 과거 기록을 보존했습니다.",
                        "change_category"=>"카테고리 변경사항을 저장했습니다.",
                        "delete_category"=>"빈 분류를 삭제했습니다.",
                        "save_alias"=>"별칭을 저장했습니다.",
                        "upsert_brand"=>"브랜드 정보를 저장했습니다. 기존 연결 상품은 유지됩니다.",
                        _=>"상품 변경을 저장했습니다.",
                    }).into());
                    self.model_groups.set_catalog(&catalog,ctx);
                    self.aliases.set_catalog(&catalog,ctx);
                    self.catalog=Some(catalog);
                    self.reload(ctx.clone());
                }
            }
            Some(Ok(Err(error)))=>{
                self.writing=None;
                if self.tab==1 {
                    self.category_manager.report_error(error.clone());
                }
                if self.tab==3 {
                    self.aliases.on_failed(error.clone());
                }
                if self.tab==4 {
                    self.brands.on_failed(error.clone());
                }
                self.error=Some(error);
            }
            Some(Err(TryRecvError::Disconnected))=>{
                let brand_write=self.writing.as_ref()
                    .is_some_and(|job|job.kind=="upsert_brand");
                self.writing=None;
                let message="상품 저장 도우미 응답이 중단됐습니다. 재조회 후 확인하세요.".to_owned();
                if brand_write{self.brands.on_failed(message.clone());}
                self.error=Some(message);
            }
            _=>{}
        }
        if let Some(at)=self.next_query {
            if Instant::now()>=at {
                self.next_query=None;
                self.reload(ctx.clone());
            }
        }
        if let Some(result)=self.result.poll(){
            match result{
                Ok(result)=>{
                    self.model_groups.set_catalog(&result.catalog,ctx);
                    self.aliases.set_catalog(&result.catalog,ctx);
                    self.catalog=Some(result.catalog);
                    self.products=result.products;
                    self.error=None;
                }
                Err(e)=>self.error=Some(e),
            }
        }
        // LatestJob already requests a repaint on worker completion.
    }
    pub fn has_modal(&self)->bool {
        self.selected.is_some() || self.excel.has_modal() ||
            (self.tab==1 && self.category_manager.has_overlay()) ||
            (self.tab==2 && self.model_groups.has_overlay()) ||
            (self.tab==3 && self.aliases.has_overlay()) ||
            (self.tab==4 && self.brands.has_overlay())
    }
    pub fn close_detail(&mut self){
        self.selected=None;
        self.original_selected=None;
        self.detail_prices.clear();
        self.confirm_archive=false;
        self.confirm_discard=false;
    }
    pub fn allow_leave(&mut self)->bool{
        if self.excel.blocks_navigation(){
            self.error=Some("상품 Excel 검토 또는 저장을 마친 뒤 이동하세요.".into());
            return false;
        }
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
                if !self.category_manager.request_global_leave(catalog){return false;}
            }
            self.category_manager.clear_for_leave();
        }
        if self.tab==2 && !self.model_groups.request_global_leave(){
            return false;
        }
        if self.tab==3 && !self.aliases.request_global_leave(){
            return false;
        }
        if self.tab==4 && !self.brands.request_global_leave(){
            return false;
        }
        true
    }
    pub fn take_category_global_leave(&mut self)->bool{
        self.category_manager.take_global_leave_approved()
            ||self.model_groups.take_global_approval()
            ||self.aliases.take_global_approval()
            ||self.brands.take_global_approval()
    }
    pub fn category_global_leave_in_progress(&self)->bool{
        self.category_manager.global_leave_in_progress()
            ||self.model_groups.global_leave_in_progress()
            ||self.aliases.global_leave_in_progress()
            ||self.brands.global_leave_in_progress()
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
        let page_origin_y=ui.cursor().top();
        self.poll(ui.ctx());
        let price_result=self.notice.as_deref().is_some_and(|msg|
            msg.starts_with("가격 변경"));
        crate::floating_notice::sync(
            ui.ctx(),"products.result",
            if price_result{"products.price"}else{"products.action"},
            crate::floating_notice::Tone::Success,self.notice.as_deref()
        );
        crate::floating_notice::sync(
            ui.ctx(),"products.error",
            if self.price_mode&&self.tab==0{"products.price"}else{"products.action"},
            crate::floating_notice::Tone::Error,self.error.as_deref()
        );
        components::management_page_title_top_gap(ui);
        components::management_top_content(ui,|top|{
            top.label(typography::page_heading("상품 관리"));
        });
        ui.add_space(theme::SPACE_8);
        let pressed=components::management_top_content(ui,|top|
            components::management_tabs(top,&[
                ("상품 목록",self.tab==0,true),
                ("카테고리 관리",self.tab==1,true),
                ("모델군 관리",self.tab==2,true),
                ("별칭 관리",self.tab==3,true),
                ("브랜드 관리",self.tab==4,true),
            ])
        );
        if let Some(next)=pressed {
            if next!=self.tab && self.writing.is_none() &&
                self.selected.is_none() && !self.has_pending_prices(){
                let allowed=match self.tab{
                    1=>{
                        let proceed=self.catalog.as_ref()
                            .is_none_or(|c|self.category_manager.request_tab_leave(c));
                        if !proceed{self.category_next_tab=Some(next);}
                        proceed
                    }
                    2=>self.model_groups.request_tab_leave(next),
                    3=>self.aliases.request_tab_leave(next),
                    4=>self.brands.request_tab_leave(next),
                    _=>true,
                };
                if allowed{
                    if self.tab==1{
                        self.category_manager.clear_for_leave();
                        self.category_manager.clear_search();
                    }
                    self.tab=next;
                    self.query.clear();
                    if next==1{self.category_manager.clear_search();}
                    self.reload(ui.ctx().clone());
                }
            }else if self.has_pending_prices(){
                self.error=Some("가격 수정값을 저장하거나 취소한 뒤 탭을 이동하세요.".into());
            }
        }
        if self.category_manager.take_switch_to_list(){
            self.tab=self.category_next_tab.take().unwrap_or(0);
            self.query.clear();
            self.reload(ui.ctx().clone());
        }
        if let Some(next)=self.model_groups.take_tab_change(){
            self.tab=next;
            self.query.clear();
            self.reload(ui.ctx().clone());
        }
        if let Some(next)=self.aliases.take_tab(){
            self.tab=next;
            self.query.clear();
            self.reload(ui.ctx().clone());
        }
        if let Some(next)=self.brands.take_tab(){
            self.tab=next;
            self.query.clear();
            self.reload(ui.ctx().clone());
        }
        // The category editor keeps its distinct tree layout. The other
        // product tabs share Freight Assignment's actual four-card KPI strip.
        if self.tab==1{
            ui.add_space(theme::SPACE_16);
        }else{
            ui.add_space(theme::SPACE_12);
            let (products,model_groups,aliases,brands)=self.catalog.as_ref()
                .map(|catalog|{
                    (rows(&catalog["products"]).len(),
                    catalog["settings"]["colorGuidanceGroups"]
                        .as_object().map_or(0,|groups|groups.len()),
                    rows(&catalog["aliases"]).len(),
                    rows(&catalog["brands"]).len())
                }).unwrap_or((0,0,0,0));
            components::management_top_content(ui,|top|{
                top.horizontal_wrapped(|strip|{
                    strip.spacing_mut().item_spacing=Vec2::new(
                        theme::SPACE_8,theme::SPACE_8
                    );
                    components::management_summary_card(strip,"등록 상품",products);
                    components::management_summary_card(strip,"모델군",model_groups);
                    components::management_summary_card(strip,"별칭",aliases);
                    components::management_summary_card(strip,"브랜드",brands);
                });
            });
            ui.add_space(theme::SPACE_16);
        }
        let can_edit=self.gateway.product_write_enabled()
            && self.catalog.as_ref().is_some_and(|c|c["revision"].as_u64().is_some());
        let saving=self.writing.is_some()||self.excel.busy();
        let mut price_write:Option<(String,Value)>=None;
        let mut open_detail:Option<Value>=None;
        let mut begin_new=false;
        // Category/model/alias/brand views own one primary section each.
        // Only the product-list tab has extra actions above its table card.
        if self.tab!=0 {
            components::align_management_primary_card(ui,page_origin_y);
        }
        if self.tab==4 {
            if let Some(catalog)=self.catalog.as_ref(){
                if let Some(data)=self.brands.show_page(ui,catalog,can_edit,saving){
                    self.start_write("upsert_brand",data,None,ui.ctx().clone());
                }
            }else{
                ui.label(typography::caption("브랜드 목록을 불러오는 중입니다."));
            }
            return;
        }
        if self.tab==3 {
            if let Some(catalog)=self.catalog.as_ref(){
                if let Some(data)=self.aliases.show_page(
                    ui,catalog,can_edit,saving
                ){
                    self.start_write("save_alias",data,None,ui.ctx().clone());
                }
            }else{
                ui.label(typography::caption("별칭 목록을 불러오는 중입니다."));
            }
            return;
        }
        if self.tab==2 {
            if let Some(catalog)=self.catalog.as_ref(){
                self.model_groups.show_page(ui,catalog,Arc::clone(&self.gateway));
            }else{
                ui.label(typography::caption("모델군 목록을 불러오는 중입니다."));
            }
            return;
        }
        if self.tab==1 {
            if let Some(catalog)=self.catalog.as_ref(){
                self.category_manager.show_page(ui,catalog,can_edit,saving);
            } else {
                ui.label(typography::caption("카테고리 목록을 불러오는 중입니다."));
            }
            return;
        }
        // Product list starts at the exact Freight Assignment card anchor.
        // Actions and optional Excel/registration editors stay INSIDE the card.
        components::align_management_primary_card(ui,page_origin_y);
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        theme::management_master_table_frame().show(ui,|panel|{
            components::fill_management_primary_card(panel,card_min_height);
            panel.spacing_mut().item_spacing.y=0.0;
            // Freight-style dual-slot toolbar at wide widths. Only when
            // product filters genuinely need the space do actions wrap onto
            // a second right-aligned row; never overlay a search selector.
            let toolbar_width=panel.available_width();
            let toolbar_rect=Rect::from_min_size(
                panel.cursor().left_top(),
                Vec2::new(toolbar_width,theme::INPUT_HEIGHT)
            );
            let dual_slot=toolbar_width>=1320.0;
            panel.horizontal_wrapped(|filter|{
                    filter.spacing_mut().item_spacing.x=theme::SPACE_8;
                    let before=self.query.clone();
                    if components::management_search_field(
                        filter,&mut self.query,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                        "모델명 또는 상품명 검색"
                    ).changed(){
                        if self.has_pending_prices(){
                            self.query=before;
                            self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    let categories=self.catalog.as_ref()
                        .map(category_choices).unwrap_or_default();
                    let brands=self.catalog.as_ref().map(|catalog|
                        rows(&catalog["brands"]).to_vec()).unwrap_or_default();
                    let before_category=self.category.clone();
                    let selected_category=categories.iter()
                        .find(|(id,_)|id==&self.category)
                        .map(|(_,name)|name.as_str()).unwrap_or("전체 카테고리");
                    let picker_width=theme::CATEGORY_PATH_PICKER_WIDTH;
                    let text_width=picker_width-theme::CATEGORY_PATH_PICKER_TEXT_PADDING;
                    let visible=typography::fit_category_path(
                        filter,selected_category,text_width,
                        typography::CategoryPathFont::Body
                    );
                    // Same split-chevron selector and active/disabled
                    // colors as the two approved masters; keep the existing
                    // 280pt category-path role and full-name hover details.
                    components::management_dropdown(
                        filter,&visible,picker_width,true,selected_category,
                        |combo|{
                            components::management_dropdown_option(
                                combo,&mut self.category,String::new(),
                                "전체 카테고리"
                            );
                            for (id,name) in &categories{
                                let short=typography::fit_category_path(
                                    combo,name,text_width,
                                    typography::CategoryPathFont::Body
                                );
                                components::management_dropdown_option(
                                    combo,&mut self.category,id.clone(),&short
                                ).on_hover_text(name);
                            }
                        }
                    );
                    if self.category!=before_category{
                        if self.has_pending_prices(){
                            self.category=before_category;
                            self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    let before_brand=self.brand.clone();
                    let brand_text=brands.iter().find(|b|field(b,"id")==self.brand)
                        .map(|b|field(b,"name")).unwrap_or("전체 브랜드");
                    components::management_dropdown(
                        filter,brand_text,theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH,
                        true,brand_text,|combo|{
                            components::management_dropdown_option(
                                combo,&mut self.brand,String::new(),"전체 브랜드"
                            );
                            for brand in &brands{
                                components::management_dropdown_option(
                                    combo,&mut self.brand,field(brand,"id").to_owned(),
                                    field(brand,"name")
                                );
                            }
                        }
                    );
                    if self.brand!=before_brand{
                        if self.has_pending_prices(){
                            self.brand=before_brand;
                            self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    let before_missing=self.missing;
                    if filter.checkbox(&mut self.missing,"가격 미등록만").changed(){
                        if self.has_pending_prices(){
                            self.missing=before_missing;
                            self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                        }else{self.changed(filter.ctx());}
                    }
            });

            // The trailing slot ends at the CARD inner-right edge, without
            // subtracting the table-only scrollbar gutter (master contract).
            let actions_rect=if dual_slot{
                components::management_toolbar_trailing_rect(
                    toolbar_rect,390.0,0.0,theme::INPUT_HEIGHT
                )
            }else{
                panel.add_space(theme::SPACE_8);
                let (rect,_)=panel.allocate_exact_size(
                    Vec2::new(panel.available_width(),theme::INPUT_HEIGHT),
                    Sense::hover()
                );
                rect
            };
            panel.scope_builder(
                UiBuilder::new().id_salt("product-master-actions")
                    .max_rect(actions_rect)
                    .layout(Layout::right_to_left(Align::Center)),
                |actions|{
                    actions.set_clip_rect(actions_rect);
                    actions.set_width(actions_rect.width());
                    actions.set_height(actions_rect.height());
                    actions.spacing_mut().item_spacing.x=theme::SPACE_8;
                    if theme::action(actions,"상품 추가",
                        theme::ButtonRole::Primary,!saving
                    ).clicked(){
                        self.adding=!self.adding;
                    }
                    if theme::action(actions,
                        if self.price_mode{"가격 수정 모드 종료"}
                        else{"가격 수정 모드 켜기"},
                        if self.price_mode{theme::ButtonRole::Primary}
                        else{theme::ButtonRole::Standard},
                        can_edit&&!saving
                    ).clicked(){
                        if self.price_mode && self.has_pending_prices(){
                            self.error=Some("저장하지 않은 가격이 있습니다. 각 가격 셀의 저장 또는 취소를 눌러주세요.".into());
                        }else{
                            self.price_mode=!self.price_mode;
                            if !self.price_mode{self.price_drafts.clear();}
                        }
                    }
                    if theme::action(actions,"Excel 관리",
                        theme::ButtonRole::Standard,!saving
                    ).clicked(){
                        if self.has_pending_prices(){
                            self.error=Some("먼저 가격 수정값을 저장하거나 취소하세요.".into());
                        }else{self.excel.toggle();}
                    }
                }
            );
            if self.adding {
                panel.add_space(theme::SPACE_8);
                panel.horizontal(|actions|{
                    if theme::action(
                        actions,"직접 등록",theme::ButtonRole::Primary,can_edit&&!saving
                    ).clicked(){begin_new=true;}
                });
            }
            if self.excel.open(){panel.add_space(theme::SPACE_8);}
            self.excel.show_panel(
                panel,Arc::clone(&self.gateway),
                self.catalog.is_some()&&self.writing.is_none()
                    &&!self.has_pending_prices()
            );
            // Keep the toolbar, header, and virtualized rows in one outline.
            // Only the list body scrolls; the page title and controls do not.
            if saving{
                theme::management_list_strip_frame().show(panel,|status|{
                    status.label(typography::caption("상품 변경 저장 중…"));
                });
            }
            if self.result.pending(){
                theme::management_list_strip_frame().show(panel,|status|{
                    status.label(typography::caption("상품 가격표 조회 중…"));
                });
            }
            // Only a DIRECT filter-to-table transition uses the approved
            // 21pt strip. Inline saving/loading notices are separate
            // information blocks, so preserve their original divider.
            let direct_filter_to_header=!saving&&!self.result.pending()
                &&!self.adding&&!self.excel.open();
            if !direct_filter_to_header{
                components::soft_divider(panel);
            }
            let original_spacing=panel.spacing().item_spacing.y;
            panel.spacing_mut().item_spacing.y=0.0;
            panel.spacing_mut().scroll=theme::management_table_scroll_style();
            let option_width=theme::PRODUCT_TABLE_OPTION_WIDTH;
            let price_width=if self.price_mode{
                theme::PRODUCT_TABLE_EDIT_PRICE_WIDTH
            }else{theme::PRODUCT_TABLE_PRICE_WIDTH};
            let action_width=theme::PRODUCT_TABLE_ACTION_WIDTH;
            let gap=theme::SPACE_8;
            let base=[theme::PRODUCT_TABLE_NAME_MIN_WIDTH,option_width,
                price_width,price_width,price_width,action_width];
            let min_width=base.iter().sum::<f32>()+5.0*gap;
            // Preserve the actual price-editing minimum and existing
            // native scrollbar allocation while using the shared table calculator.
            let min_table_width=theme::PRODUCT_TABLE_MIN_WIDTH.max(min_width);
            let layout=if direct_filter_to_header{
                components::management_sticky_table_header(
                    panel,min_table_width
                )
            }else{
                components::management_list_table_header(panel,min_table_width)
            };
            let table_width=layout.table_width;
            let widths=theme::management_single_stretch_columns(
                base,table_width,gap,0
            );
            let headings=[
                components::ManagementTableColumnLabel::ProductName.text(),
                "색상/옵션","도매가",
                "반도매가","소매가","상세"];
            let selection_locked=self.has_pending_prices();
            let ordered=product_sort_order(
                &self.products,&self.selected_variants,self.sort
            );
            let header_rect=layout.header_rect;
            // Product Excel actions, optional add controls and transient
            // notices have already consumed their measured space above.
            // Protect the empty-state caption under the fixed header.
            let empty_reserve=if self.products.is_empty()
                && !self.result.pending() && self.error.is_none(){
                theme::INPUT_HEIGHT
            }else{0.0};
            let scroll_max_height=components::management_single_list_scroll_height(
                panel,card_min_height,empty_reserve
            );
            // The empty-state reserve affects only viewport height.
            // The shared component owns the 7pt rail extension.
            let scroll_track=components::management_table_scroll_track_rect(
                panel,scroll_max_height
            );
            let mut row_left=None;
            let scroll_output=components::virtual_fixed_rows(
                egui::ScrollArea::both()
                    .scroll_bar_rect(scroll_track)
                    .id_salt("egui-product-price-grid")
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                    )
                    .max_height(scroll_max_height)
                    .auto_shrink([false,false]),
                panel,table_width,theme::PRODUCT_TABLE_ROW_HEIGHT,ordered.len(),
                |table,visible_rows|{
                    table.set_min_width(table_width);
                    table.spacing_mut().item_spacing.y=0.0;
                    theme::apply_table_row_colors(table);
                    let first=visible_rows.start;
                    let origin=table.max_rect().min;
                    for row_index in visible_rows {
                        let product=&self.products[ordered[row_index]];
                        let product_id=field(product,"id");
                        let row_rect=Rect::from_min_size(
                            egui::pos2(
                                origin.x,
                                origin.y+(row_index-first) as f32
                                    *theme::PRODUCT_TABLE_ROW_HEIGHT
                            ),
                            Vec2::new(table_width,theme::PRODUCT_TABLE_ROW_HEIGHT)
                        );
                        row_left.get_or_insert(row_rect.left());
                        table.scope_builder(
                            UiBuilder::new()
                                .id_salt(("product-table-fixed-row",product_id))
                                .max_rect(row_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |row|{
                                // All child widgets are anchored to shared
                                // column rectangles. No Grid intrinsic widths
                                // or conditional controls can move the next row.
                                row.allocate_exact_size(
                                    Vec2::new(table_width,theme::PRODUCT_TABLE_ROW_HEIGHT),
                                    Sense::hover()
                                );
                                row.painter().rect_filled(
                                    row_rect,0.0,
                                    if row_index%2==0{theme::PANEL}
                                    else{theme::TABLE_ROW_ALT_BG}
                                );
                                let cells=product_table_cells(
                                    row_rect.min,&widths,theme::PRODUCT_TABLE_ROW_HEIGHT
                                );
                                // Capture before descendant scope borrows row mutably.
                                let row_clip=row.clip_rect();
                                let name=field(product,"name");
                                let subtitle=field(product,"subtitle");
                                let display=if subtitle.is_empty(){name.to_owned()}
                                    else{format!("{name} / {subtitle}")};
                                product_table_text(
                                    row,cells[0],&display,typography::ManagementTableCellRole::ProductName,(product_id,0)
                                );
                                let variants=rows(&product["variants"]);
                                let mut selected_id=self.selected_variants
                                    .get(product_id).cloned().unwrap_or_default();
                                if !variants.iter().any(|v|
                                    field(v,"id")==selected_id.as_str()){
                                    selected_id=variants.first()
                                        .map(|v|field(v,"id").to_owned())
                                        .unwrap_or_default();
                                }
                                let variant_cell=cells[1].shrink2(
                                    Vec2::new(theme::SPACE_4,0.0)
                                );
                                if variants.len()>1{
                                    let chosen=variants.iter().find(|v|
                                        field(v,"id")==selected_id.as_str())
                                        .unwrap_or(&variants[0]);
                                    let current_label=option_text(product,chosen);
                                    let mut builder=UiBuilder::new()
                                        .id_salt(("product-variant",product_id))
                                        .max_rect(variant_cell)
                                        .layout(Layout::left_to_right(Align::Center));
                                    if selection_locked{builder=builder.disabled();}
                                    row.scope_builder(builder,|controls|{
                                            controls.set_clip_rect(
                                                variant_cell.intersect(row_clip)
                                            );
                                            components::management_dropdown(
                                                controls,
                                                if current_label.is_empty(){"기본"}
                                                else{&current_label},
                                                (variant_cell.width()-theme::SPACE_8).max(1.0),
                                                true,"상품 옵션 선택",|choices|{
                                                    for variant in variants{
                                                        let label=option_text(product,variant);
                                                        components::management_dropdown_option(
                                                            choices,&mut selected_id,
                                                            field(variant,"id").to_owned(),
                                                            if label.is_empty(){"기본"}else{&label}
                                                        );
                                                    }
                                                });
                                        }
                                    );
                                }else if let Some(variant)=variants.first(){
                                    let label=option_text(product,variant);
                                    product_table_text(row,cells[1],
                                        if label.is_empty(){"—"}else{&label},
                                        typography::ManagementTableCellRole::Option,(product_id,1));
                                }else{
                                    product_table_text(row,cells[1],"—",typography::ManagementTableCellRole::Option,(product_id,1));
                                }
                                self.selected_variants.insert(
                                    product_id.to_owned(),selected_id.clone()
                                );
                                let selected_variant=variants.iter().find(|v|
                                    field(v,"id")==selected_id.as_str()
                                );
                                for (offset,tier) in ["wholesale","semi","retail"]
                                    .iter().enumerate(){
                                    let price_rect=cells[offset+2].shrink2(
                                        Vec2::new(theme::SPACE_4,0.0)
                                    );
                                    if let Some(variant)=selected_variant{
                                        if self.price_mode {
                                            let key=format!(
                                                "{}:{}:{}",product_id,
                                                field(variant,"id"),tier
                                            );
                                            let original=variant[*tier].as_i64();
                                            let entry=self.price_drafts.entry(key.clone())
                                                .or_insert_with(||PriceDraft{
                                                    original,
                                                    input:original.map(|v|v.to_string())
                                                        .unwrap_or_default()
                                                });
                                            let mut builder=UiBuilder::new()
                                                .id_salt(("product-price-edit",key.as_str()))
                                                .max_rect(price_rect)
                                                .layout(Layout::left_to_right(Align::Center));
                                            if saving{builder=builder.disabled();}
                                            row.scope_builder(builder,|inputs|{
                                                    inputs.set_clip_rect(
                                                        price_rect.intersect(row_clip)
                                                    );
                                                    inputs.push_id(
                                                        ("product-price-cell",product_id,
                                                         field(variant,"id"),tier),
                                                        |inputs|{
                                                            let response=components::money_field(
                                                                inputs,&mut entry.input,true
                                                            );
                                                            crate::floating_notice::sized_text_caret(
                                                                inputs,"products.price",&response,
                                                                &entry.input,typography::numeric_editor_font(),
                                                                false,8.0
                                                            );
                                                            let enter=response.lost_focus() &&
                                                                inputs.input(|i|
                                                                    i.key_pressed(egui::Key::Enter));
                                                            if response.has_focus() &&
                                                                inputs.input(|i|
                                                                    i.key_pressed(egui::Key::Escape)){
                                                                entry.input=entry.original.map(|n|
                                                                    n.to_string()).unwrap_or_default();
                                                            }
                                                            if entry.dirty(){
                                                                if theme::action(inputs,"저장",
                                                                    theme::ButtonRole::Standard,true
                                                                ).clicked()||enter {
                                                                    match entry.parse(){
                                                                        Ok(value)=>{
                                                                            price_write=Some((
                                                                                key.clone(),json!({
                                                                                    "productId":product_id,
                                                                                    "variantId":field(variant,"id"),
                                                                                    "tier":tier,"price":value,
                                                                                    "expected":entry.original
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
                                                        }
                                                    );
                                                }
                                            );
                                        }else{
                                            product_price_text(
                                                row,cells[offset+2],&variant[*tier]
                                            );
                                        }
                                    }else{
                                        product_table_text(row,cells[offset+2],"—",
                                            typography::ManagementTableCellRole::Money,(product_id,offset+2));
                                    }
                                }
                                let action_rect=Rect::from_center_size(
                                    cells[5].center(),Vec2::new(
                                        cells[5].width()-theme::SPACE_8,
                                        theme::ACTION_STANDARD_HEIGHT
                                    )
                                );
                                let mut builder=UiBuilder::new()
                                    .id_salt(("product-table-action",product_id))
                                    .max_rect(action_rect)
                                    .layout(Layout::left_to_right(Align::Center));
                                if saving{builder=builder.disabled();}
                                row.scope_builder(builder,|actions|{
                                        actions.set_clip_rect(action_rect.intersect(row_clip));
                                        if theme::action(
                                            actions,"상세 관리",
                                            theme::ButtonRole::TableSecondary,!saving
                                        ).clicked(){
                                            open_detail=Some(product.clone());
                                        }
                                    }
                                );
                                row.painter().line_segment(
                                    [row_rect.left_bottom(),row_rect.right_bottom()],
                                    egui::Stroke::new(
                                        theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER
                                    )
                                );
                            }
                        );
                    }
                }
            );
            // Paint the same six-column geometry after the scroll output,
            // translating only the shared horizontal scroll offset.
            let header_left=row_left.unwrap_or(
                header_rect.left()-scroll_output.state.offset.x
            );
            let header_cells=product_table_cells(
                egui::pos2(header_left,header_rect.top()),
                &widths,header_rect.height()
            );
            // Master header owns the same 34pt text area and inset 2pt
            // bottom rule as Freight Assignment; no clipped 1pt edge stroke.
            let header_text_rect=components::management_master_header_begin(
                panel,header_rect,layout.header_content_rect
            );
            let painter=panel.painter().with_clip_rect(header_rect);
            let mut clicked_sort=None;
            for (index,title) in headings.iter().enumerate(){
                let rect=header_cells[index].intersect(header_text_rect);
                if rect.width()<=0.0{continue;}
                let column=ProductSort::column_at(index);
                let title_galley=typography::freight_table_header_galley(panel,title);
                let arrow=column.map(|column|{
                    let (text,color)=match self.sort
                        .filter(|state|state.column==column).map(|sort|sort.direction){
                        Some(ProductSortDirection::Ascending)=>("▲",theme::SELECTION),
                        Some(ProductSortDirection::Descending)=>("▼",theme::SELECTION),
                        None=>("↕",theme::MUTED),
                    };
                    (typography::freight_table_header_galley(panel,text),color)
                });
                let arrow_width=arrow.as_ref().map_or(0.0,|(glyph,_)|glyph.size().x);
                let icon_gap=if arrow.is_some(){theme::SPACE_4}else{0.0};
                let ink_width=title_galley.size().x+icon_gap+arrow_width;
                let x=if matches!(index,2|3|4){
                    (header_cells[index].right()-theme::SPACE_8-ink_width)
                        .max(header_cells[index].left()+theme::SPACE_8)
                }else if index==5{
                    header_cells[index].center().x-ink_width/2.0
                }else{
                    header_cells[index].left()+theme::SPACE_8
                };
                let ink=painter.with_clip_rect(rect);
                ink.galley(
                    egui::pos2(x,rect.center().y-title_galley.size().y/2.0),
                    title_galley.clone(),theme::SECONDARY
                );
                if let Some((glyph,color))=arrow{
                    ink.galley(
                        egui::pos2(x+title_galley.size().x+icon_gap,
                            rect.center().y-glyph.size().y/2.0),
                        glyph,color
                    );
                }
                if let Some(column)=column{
                    let response=panel.interact(
                        rect,panel.make_persistent_id(("product-price-sort",index)),
                        Sense::click()
                    ).on_hover_text(match ProductSort::next(self.sort,column){
                        Some(ProductSort{direction:ProductSortDirection::Ascending,..})
                            =>"오름차순 정렬",
                        Some(ProductSort{direction:ProductSortDirection::Descending,..})
                            =>"내림차순 정렬",
                        None=>"기본 순서로 복귀",
                    });
                    if response.hovered(){
                        components::management_master_header_sort_hover(panel,rect);
                    }
                    if response.clicked(){clicked_sort=Some(ProductSort::next(self.sort,column));}
                }
                if index<5{
                    let edge=header_cells[index].right()+gap/2.0;
                    painter.line_segment(
                        [egui::pos2(edge,header_text_rect.top()),
                         egui::pos2(edge,header_text_rect.bottom())],
                        egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER)
                    );
                }
            }
            components::management_master_header_finish(panel,header_rect);
            if let Some(next)=clicked_sort{
                self.sort=next;
                panel.ctx().request_repaint();
            }
            panel.spacing_mut().item_spacing.y=original_spacing;
            if self.products.is_empty() && !self.result.pending() && self.error.is_none(){
                theme::management_list_strip_frame().show(panel,|empty|{
                    empty.label(typography::caption(
                        if self.query.is_empty() && self.category.is_empty()
                            && self.brand.is_empty() && !self.missing{
                            "등록된 상품이 없습니다."
                        }else{"조건에 맞는 상품이 없습니다."}
                    ));
                });
            }
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
    fn product_price_candidate(
        draft:&Value,inputs:&HashMap<String,String>,
    )->Result<Value,String>{
        let mut next=draft.clone();
        let variant=next["variants"].as_array_mut()
            .and_then(|rows|rows.first_mut())
            .ok_or("상품의 단일 판매 구성을 확인할 수 없습니다.")?;
        for tier in ["wholesale","semi","retail"]{
            let raw=inputs.get(tier).ok_or("상품 가격 입력이 없습니다.")?;
            variant[tier]=json!(parse_product_price(raw,"가격",false)?);
        }
        Ok(next)
    }
    /// The section masters ADD 12pt inner padding and 1pt border on BOTH
    /// edges. Account for that before assigning widths or the image/fields
    /// will clip, particularly with the approved inline label + editor rows.
    fn detail_columns(width:f32)->Option<(f32,f32,f32)>{
        let section_inset=2.0*(theme::MODAL_SECTION_PADDING+
            theme::FRAME_STROKE_WIDTH);
        let image=theme::PRODUCT_DETAIL_IMAGE_SIZE+section_inset;
        let gap=theme::SPACE_16+2.0*theme::SPACE_8;
        let usable=width-image-2.0*gap;
        if usable<560.0{return None;}
        let price=(usable*0.36).clamp(240.0,255.0);
        let overview=usable-price;
        if overview<310.0{return None;}
        Some((overview,price,image))
    }
    fn detail_wide_text(
        ui:&mut Ui,draft:&mut Value,key:&str,label:&str,enabled:bool
    ){
        components::modal_labeled_row(ui,label,|line|{
            let mut value=field(draft,key).to_owned();
            line.add_enabled_ui(enabled,|input|{
                let width=input.available_width().max(1.0);
                if components::flexible_text_field(
                    input,&mut value,width,label
                ).changed(){
                    draft[key]=json!(value);
                }
            });
        });
    }
    fn detail_image_column(
        &mut self,ui:&mut Ui,draft:&mut Value,
        editable:bool,busy:bool,photo_error:&mut Option<String>
    ){
        ui.label(typography::section_heading("상품 이미지"));
        ui.add_space(theme::SPACE_8);
        // Never shrink an image to fill a narrower sibling column.
        // The responsive fallback stacks the editor instead.
        self.photos.photo(ui,draft,theme::PRODUCT_DETAIL_IMAGE_SIZE);
        ui.add_space(theme::SPACE_8);
        ui.horizontal_wrapped(|actions|{
            if theme::action(
                actions,"이미지 등록",theme::ButtonRole::Standard,
                editable&&!busy
            ).clicked(){
                match product_images::choose_and_optimize_photo(){
                    Ok(Some(data))=>{
                        draft["imageData"]=json!(data);
                        draft["imageOverride"]=json!("MANUAL");
                        self.photos.clear();
                    }
                    Ok(None)=>{}
                    Err(error)=>*photo_error=Some(error),
                }
            }
            if theme::action(
                actions,"이미지 제거",theme::ButtonRole::Quiet,
                editable&&!busy
            ).clicked(){
                draft["imageData"]=Value::Null;
                draft["imageOverride"]=json!("HIDDEN");
                self.photos.clear();
            }
        });
        ui.label(typography::caption("로컬에 저장된 이미지 사용"));
    }
    fn detail_overview_column(
        ui:&mut Ui,draft:&mut Value,
        brands:&[Value],categories:&[(String,String)],
        editable:bool,busy:bool
    ){
        Self::detail_wide_text(ui,draft,"name","상품명",editable&&!busy);
        ui.add_space(theme::SPACE_12);
        Self::detail_wide_text(
            ui,draft,"subtitle","상품 옵션",editable&&!busy
        );
        ui.add_space(theme::SPACE_12);
        components::modal_labeled_row(ui,"카테고리",|row|{
            let before=draft["categories"].as_array()
                .and_then(|ids|ids.last())
                .and_then(Value::as_str).unwrap_or("").to_owned();
            let mut category_id=before.clone();
            let display=categories.iter()
                .find(|(id,_)|id==&category_id)
                .map(|(_,name)|name.as_str())
                .unwrap_or("카테고리 미지정");
            row.add_enabled_ui(editable&&!busy,|input|{
                let width=input.available_width().max(1.0);
                let visible=typography::fit_category_path(
                    input,display,
                    width-theme::CATEGORY_PATH_PICKER_TEXT_PADDING,
                    typography::CategoryPathFont::Body
                );
                components::management_dropdown(
                    input,&visible,width,true,display,|choices|{
                        components::management_dropdown_option(
                            choices,&mut category_id,
                            String::new(),"카테고리 미지정");
                        for (id,label) in categories{
                            let visible=typography::fit_category_path(
                                choices,label,
                                width-theme::CATEGORY_PATH_PICKER_TEXT_PADDING,
                                typography::CategoryPathFont::Body
                            );
                            components::management_dropdown_option(
                                choices,&mut category_id,id.clone(),&visible
                            ).on_hover_text(label);
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
        ui.add_space(theme::SPACE_12);
        components::modal_labeled_row(ui,"브랜드",|row|{
            let before=field(draft,"brandId").to_owned();
            let mut brand_id=before.clone();
            let label=brands.iter().find(|b|field(b,"id")==brand_id)
                .map(|b|field(b,"name")).unwrap_or("브랜드 미지정");
            row.add_enabled_ui(editable&&!busy,|input|{
                let width=input.available_width().max(1.0);
                components::management_dropdown(
                    input,label,width,true,"브랜드 선택",|choices|{
                        components::management_dropdown_option(
                            choices,&mut brand_id,String::new(),"브랜드 미지정");
                        for item in brands{
                            components::management_dropdown_option(
                                choices,&mut brand_id,
                                field(item,"id").to_owned(),field(item,"name"));
                        }
                    });
            });
            if brand_id!=before{
                draft["brandId"]=if brand_id.is_empty(){
                    Value::Null
                }else{json!(brand_id)};
            }
        });
    }
    fn detail_price_column(
        ui:&mut Ui,draft:&mut Value,
        prices:&mut HashMap<String,String>,editable:bool,busy:bool
    ){
        components::modal_labeled_row(ui,"판매 상태",|line|{
            // Rust catalog stores a boolean active flag, not four invented
            // selling modes. A real shared dropdown edits that same draft.
            let current=draft["active"].as_bool()!=Some(false);
            let mut selected=current;
            line.add_enabled_ui(editable&&!busy,|input|{
                let width=input.available_width().max(1.0);
                components::management_dropdown(
                    input,if selected{"판매중"}else{"판매중지"},
                    width,true,"판매 상태 선택",|options|{
                        components::management_dropdown_option(
                            options,&mut selected,true,"판매중");
                        components::management_dropdown_option(
                            options,&mut selected,false,"판매중지");
                    });
            });
            if selected!=current{draft["active"]=json!(selected);}
        });
        ui.add_space(theme::SPACE_12);
        for (label,tier) in [
            ("도매가","wholesale"),("반도매가","semi"),
            ("소매가","retail")
        ]{
            components::modal_labeled_row(ui,label,|line|{
                let value=prices.entry(tier.to_owned())
                    .or_insert_with(String::new);
                // Reserve the '원' unit inside the same row rather than
                // placing it below the price or beyond the modal section.
                let unit_slot=theme::SPACE_24;
                let width=(line.available_width()-unit_slot).max(48.0);
                line.allocate_ui_with_layout(
                    Vec2::new(width,theme::INPUT_HEIGHT),
                    Layout::left_to_right(Align::Center),
                    |input|{
                        input.add_enabled_ui(editable&&!busy,|editor|{
                            components::money_field(editor,value,true);
                        });
                    }
                );
                line.label(typography::field_label("원"));
            });
            ui.add_space(theme::SPACE_12);
        }
    }
    pub fn show_detail(&mut self,ctx:&Context,work_area:egui::Rect){
        if self.tab==0 && self.excel.has_modal(){
            self.excel.show_review(
                ctx,work_area,Arc::clone(&self.gateway),
                self.gateway.product_write_enabled()&&!self.writing.is_some()
            );
            return;
        }
        if self.tab==4 {
            if let Some(catalog)=self.catalog.clone(){
                let editable=self.gateway.product_write_enabled()
                    && catalog["revision"].as_u64().is_some();
                if let Some(data)=self.brands.show_dialogs(
                    ctx,work_area,&catalog,editable,self.writing.is_some()
                ){
                    self.start_write("upsert_brand",data,None,ctx.clone());
                }
            }
            return;
        }
        if self.tab==3 {
            if let Some(catalog)=self.catalog.clone(){
                let editable=self.gateway.product_write_enabled()
                    && catalog["revision"].as_u64().is_some();
                if let Some(data)=self.aliases.show_dialogs(
                    ctx,work_area,&catalog,editable,self.writing.is_some()
                ){
                    self.start_write("save_alias",data,None,ctx.clone());
                }
            }
            return;
        }
        if self.tab==2{
            self.model_groups.show_dialogs(ctx,work_area,Arc::clone(&self.gateway));
            return;
        }
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
        let mut photo_error=None;
        components::modal_backdrop(ctx,"egui-product-detail-backdrop",false);
        components::modal_window(
            if is_new {"상품 등록"} else {"상품 상세 관리"},
            "egui-product-detail",work_area
        ).resizable(false)
            .default_width(theme::PRODUCT_DETAIL_MODAL_WIDTH.min(
                (work_area.width()-2.0*theme::MODAL_VIEWPORT_GUTTER).max(1.0)
            ))
            .show(ctx,|modal|{
                let dialog_title=if is_new{"상품 등록"}else{"상품 상세 관리"};
                if components::modal_header(
                    modal,dialog_title,"취소",!busy
                ){request_close=true;}
                egui::ScrollArea::vertical()
                    .id_salt("egui-product-detail-content")
                    .max_height(components::modal_editor_body_max_height(
                        work_area.height()
                    ))
                    .auto_shrink([false,false])
                    .show(modal,|scroller|{
                        // The scrollbar belongs to the scroll surface;
                        // use the master 16pt inset INSIDE it so fields
                        // never touch the rail or panel's right edge.
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(
                                theme::MODAL_MASTER_CONTENT_INSET as i8,0
                            ))
                            .show(scroller,|body|{
                        // Keep identification, prices, and visual reference
                        // adjacent in the requested overview / price / image
                        // order. Each column uses the same shared controls.
                        let content_width=body.available_width().max(1.0);
                        if let Some((overview_width,price_width,image_width))=
                            Self::detail_columns(content_width)
                        {
                            body.horizontal(|top|{
                                top.vertical(|overview|{
                                    overview.set_width(overview_width);
                                    Self::detail_overview_column(
                                        overview,&mut draft,&brands,&categories,
                                        editable,busy
                                    );
                                });
                                top.add_space(theme::SPACE_16);
                                top.vertical(|prices_ui|{
                                    prices_ui.set_width(price_width);
                                    Self::detail_price_column(
                                        prices_ui,&draft,&mut prices,
                                        editable,busy
                                    );
                                });
                                top.add_space(theme::SPACE_16);
                                top.vertical(|media|{
                                    media.set_width(image_width);
                                    self.detail_image_column(
                                        media,&mut draft,editable,busy,
                                        &mut photo_error
                                    );
                                });
                            });
                        }else{
                            // Narrow Kakao-safe work areas stack sections;
                            // never shrink the 320px image or clip fields.
                            body.vertical(|stack|{
                                Self::detail_overview_column(
                                    stack,&mut draft,&brands,&categories,
                                    editable,busy
                                );
                                stack.add_space(theme::SPACE_16);
                                Self::detail_price_column(
                                    stack,&draft,&mut prices,editable,busy
                                );
                                stack.add_space(theme::SPACE_16);
                                self.detail_image_column(
                                    stack,&mut draft,editable,busy,&mut photo_error
                                );
                            });
                        }
                        body.add_space(theme::SPACE_16);
                        components::soft_divider(body);
                        egui::CollapsingHeader::new(
                            "고급 정보"
                        )
                            .id_salt("product-detail-advanced")
                            .default_open(false)
                            .icon(components::paint_collapsing_header_icon)
                            .show(body,|advanced|{
                                advanced.add_space(theme::SPACE_8);
                                advanced.vertical(|group_ui|{
                                    group_ui.label(typography::field_label("배송 그룹"));
                                    let before=field(&draft,"shippingGroupId").to_owned();
                                    let mut group=before.clone();
                                    let label=groups.iter().find(|g|field(g,"id")==group)
                                        .map(|g|field(g,"name")).unwrap_or("미지정");
                                    let width=group_ui.available_width().max(1.0);
                                    group_ui.add_enabled_ui(editable&&!busy,|input|{
                                        components::management_dropdown(
                                            input,label,width,true,"배송 그룹 선택",|choices|{
                                                components::management_dropdown_option(
                                                    choices,&mut group,String::new(),"미지정");
                                                for row in &groups{
                                                    components::management_dropdown_option(
                                                        choices,&mut group,field(row,"id").to_owned(),
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
                                advanced.add_space(theme::SPACE_12);
                                advanced.vertical(|origin_ui|{
                                    origin_ui.label(typography::field_label("발송지"));
                                    let before=field(&draft,"origin").to_owned();
                                    let mut origin=before.clone();
                                    origin_ui.add_enabled_ui(editable&&!busy,|input|{
                                        let width=input.available_width();
                                        components::management_dropdown(
                                            input,if origin=="STORE"{"매장"}else{"공장"},
                                            width,true,"발송지 선택",|options|{
                                                components::management_dropdown_option(
                                                    options,&mut origin,
                                                    "FACTORY".into(),"공장");
                                                components::management_dropdown_option(
                                                    options,&mut origin,
                                                    "STORE".into(),"매장");
                                            });
                                    });
                                    if origin!=before{draft["origin"]=json!(origin);}
                                });
                                if let Some(variant)=draft["variants"].as_array_mut()
                                    .and_then(|variants|variants.first_mut()){
                                    Self::detail_wide_text(
                                        advanced,variant,"url","홈페이지 상품 연결",
                                        editable&&!busy
                                    );
                                }
                                if !is_new {
                                    let history=rows(&draft["priceHistory"]);
                                    advanced.label(typography::section_heading(
                                        &format!("가격 변경 이력 {}건",history.len())
                                    ));
                                    for item in history.iter().rev(){
                                        advanced.label(typography::caption(
                                            &format!("{} / {} / {} → {}",
                                                field(item,"at"),field(item,"field"),
                                                item["before"],item["after"]
                                            )
                                        ));
                                    }
                                }
                            });
                        });
                    });
                modal.add_space(theme::SPACE_12);
                components::modal_action_row_split(
                    modal,
                    |left|{
                        if !is_new && theme::action(
                            left,"상품 삭제",theme::ButtonRole::Danger,
                            editable&&!busy
                        ).clicked(){
                            request_archive=true;
                        }
                    },
                    |right|{
                        let price_changed=prices.iter().any(|(tier,raw)|{
                            let before=original["variants"].as_array()
                                .and_then(|v|v.first())
                                .and_then(|v|v.get(tier))
                                .and_then(Value::as_i64)
                                .map(|n|n.to_string()).unwrap_or_default();
                            raw.trim().replace(',', "")!=before
                        });
                        let label=if is_new {"상품 등록"} else {"변경사항 저장"};
                        if theme::action(right,label,
                            theme::ButtonRole::Primary,
                            editable&&!busy&&
                                (is_new || draft!=original || price_changed)
                        ).clicked(){request_save=true;}
                    }
                );
            });
        self.selected=Some(draft.clone());
        self.detail_prices=prices;
        if let Some(error)=photo_error{self.error=Some(error);}
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
                cancel=components::modal_header(
                    dialog,"상품 삭제","취소",!busy
                );
                dialog.label(typography::body(
                    "등록 상품에서 삭제할까요? 과거 주문과 가격 기록은 보존하며 홈페이지 재수집으로 자동 복원하지 않습니다."
                ));
                components::modal_action_row(dialog,|actions|{
                        if theme::action(actions,"상품 삭제",theme::ButtonRole::Danger,
                            editable&&!busy).clicked(){archive=true;}
                    }
                );
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
                keep=components::modal_header(
                    dialog,"저장하지 않은 상품","취소",!busy
                );
                dialog.label(typography::body(
                    "상품 수정사항을 저장하지 않고 닫을까요?"
                ));
                components::modal_action_row(dialog,|actions|{
                        if theme::action(actions,"저장 안 함",
                            theme::ButtonRole::Danger,!busy).clicked(){discard=true;}
                    }
                );
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
    fn product_detail_reacts_to_actual_kakao_safe_modal_width(){
        // Three columns fit only when a 320px image, a 300px overview,
        // and a 210px price form plus two egui horizontal gaps all fit.
        assert!(ProductManager::detail_columns(893.0).is_none());
        for width in [894.0,940.0,1000.0,1100.0]{
            let (overview,price,image)=ProductManager::detail_columns(width).unwrap();
            assert_eq!(image,theme::PRODUCT_DETAIL_IMAGE_SIZE);
            assert!(overview>=300.0 && price>=210.0 && price<=250.0);
            let gap=theme::SPACE_16+2.0*theme::SPACE_8;
            assert!(overview+price+image+2.0*gap<=width+0.01,
                "columns overflow {width}pt");
        }
        assert!(ProductManager::detail_columns(420.0).is_none());
    }
    #[test]
    fn price_draft_uses_react_price_cell_zero_as_unregistered(){
        let empty=PriceDraft{original:None,input:"0".into()};
        assert!(!empty.dirty());
        assert_eq!(empty.parse().unwrap(),None);
        let recorded=PriceDraft{original:Some(4500),input:"0".into()};
        assert!(recorded.dirty());
        assert_eq!(recorded.parse().unwrap(),None);
        assert_eq!(parse_product_price("0","가격",false).unwrap(),Some(0));
        let historical_zero=PriceDraft{original:Some(0),input:"0".into()};
        assert!(!historical_zero.dirty());
        assert_eq!(historical_zero.parse().unwrap(),None);
        assert_eq!(parse_product_price("4,500","금액",true).unwrap(),Some(4500));
        assert!(parse_product_price("-1","가격",false).is_err());
        assert!(parse_product_price("9007199254740992","가격",false).is_err());
    }
    #[test]
    fn catalog_options_do_not_create_mutable_catalog_entries(){
        let catalog=json!({"categories":[
            {"id":"a","name":"보면대","parentId":"catalog-root:BANDOSTAND"}]});
        let ids=category_choices(&catalog);
        assert!(ids.iter().any(|(id,name)|id=="a"&&name.contains("보면대")));
        assert!(ids.iter().any(|(id,_)|id=="catalog-root:BANDOSTAND"));
        assert!(catalog["categories"].as_array().is_some_and(|v|v.len()==1));
    }
    #[test]
    fn product_table_sort_cycles_without_changing_native_catalog_membership(){
        let catalog=vec![
            json!({"id":"two","name":"BD 702","variants":[{
                "id":"v2","color":"단일","wholesale":5500,"semi":6000,"retail":9000
            }]}),
            json!({"id":"one","name":"BD 701","variants":[{
                "id":"v1","color":"단일","wholesale":3500,"semi":5000,"retail":8000
            }]}),
        ];
        let original=catalog.clone();
        let selected=HashMap::new();
        assert_eq!(product_sort_order(&catalog,&selected,None),vec![0,1]);
        let asc=ProductSort::next(None,ProductSortColumn::Name).unwrap();
        assert_eq!(product_sort_order(&catalog,&selected,Some(asc)),vec![1,0]);
        let desc=ProductSort::next(Some(asc),ProductSortColumn::Name).unwrap();
        assert_eq!(product_sort_order(&catalog,&selected,Some(desc)),vec![0,1]);
        assert!(ProductSort::next(Some(desc),ProductSortColumn::Name).is_none());
        let asc_price=ProductSort::next(None,ProductSortColumn::Wholesale).unwrap();
        assert_eq!(product_sort_order(&catalog,&selected,Some(asc_price)),vec![1,0]);
        assert_eq!(catalog,original);
    }
    #[test]
    fn fixed_product_table_columns_share_identical_header_and_row_geometry(){
        let widths=[340.0,182.0,116.0,116.0,116.0,100.0];
        let start=egui::pos2(20.0,30.0);
        let header=product_table_cells(start,&widths,theme::MANAGEMENT_TABLE_HEADER_HEIGHT);
        let row=product_table_cells(
            egui::pos2(start.x,start.y+theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
            &widths,theme::PRODUCT_TABLE_ROW_HEIGHT
        );
        for i in 0..widths.len(){
            assert_eq!(header[i].left(),row[i].left());
            assert_eq!(header[i].right(),row[i].right());
            assert_eq!(row[i].height(),theme::PRODUCT_TABLE_ROW_HEIGHT);
            if i>0{
                assert_eq!(row[i].left()-row[i-1].right(),theme::SPACE_8);
            }
        }
    }
}
