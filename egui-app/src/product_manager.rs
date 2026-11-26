//! React ProductManager 상품 목록: Rust-owned catalog selection and a
//! read-only price/detail projection. Product and price writes must not target
//! the isolated egui preview DB or bypass Tauri's operational save boundary.
use crate::{
    category_manager::{CategoryManager,ROOTS},
    category_dropdown,
    cell_navigation::{self, CellNavigationCell, CellNavigationKey},
    alias_manager::AliasManager,
    brand_manager::BrandManager,
    model_group_manager::ModelGroupManager,
    components,
    gateway::BandoGateway,
    management_queries::{self, ManagementProductRows},
    latest_job::LatestJob,
    product_excel_ui::ProductExcelUi,
    product_images::{self,ProductImageCache},
    theme, typography,
};
use eframe::egui::{self, Align, Context, Layout, Sense, Ui, UiBuilder, Rect, Vec2};
use serde_json::{json, Value};
use std::{
    cmp::Ordering,
    collections::{HashMap,VecDeque},
    sync::{mpsc::{self, Receiver, TryRecvError}, Arc},
    time::{Duration, Instant},
};

fn field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

/// React PriceCell selects the existing amount when a cell first receives
/// focus. Match that behavior so typing replaces the cell value instead of
/// unexpectedly inserting digits in the middle of the old amount.
fn select_all_price_text(ui:&Ui,response:&egui::Response,text:&str){
    if let Some(mut state)=egui::TextEdit::load_state(ui.ctx(),response.id){
        let mut range=state.cursor.char_range().unwrap_or_else(||
            egui::text::CCursorRange::one(egui::text::CCursor::new(0))
        );
        range.primary=egui::text::CCursor::new(text.chars().count());
        range.secondary=egui::text::CCursor::new(0);
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(),response.id);
        ui.ctx().request_repaint();
    }
}

/// Match the current React sale-status contract. The catalog 'active' bit
/// denotes legacy activation, while 'saleStatus' holds the four user-facing
/// sales states; do not derive sold-out status from 'active'.
const SALE_STATES:[&str;4]=["판매중","품절","판매준비중","판매중지"];

fn displayed_sale_status(product:&Value)->&str{
    if product["active"].as_bool()==Some(false){return "판매중지";}
    match field(product,"saleStatus"){
        ""|"판매 표시"|"SELLING"|"판매중"=>"판매중",
        "등록 대기"=>"판매준비중",
        status=>status,
    }
}

fn sale_status_color(status:&str)->egui::Color32{
    match status{
        "판매중"=>theme::SALE_STATUS_SELLING,
        "품절"=>theme::SALE_STATUS_SOLD_OUT,
        "판매준비중"=>theme::SALE_STATUS_PREPARING,
        _=>theme::SALE_STATUS_STOPPED,
    }
}

fn set_sale_status(product:&mut Value,status:&str){
    product["saleStatus"]=json!(status);
    // Keep React ProductManager's explicit status edit semantics: choosing
    // a selling mode activates the catalog record, including '판매중지'.
    // The sale issue check still rejects any non-selling saleStatus.
    product["active"]=json!(true);
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// Search by the displayed brand name AND its catalog Korean name/aliases.
/// Selection continues to use the unchanged catalog ID.
fn brand_search_choices(
    brands:&[Value],empty_label:&str
)->Vec<components::ManagementSearchChoice>{
    let mut choices=vec![components::ManagementSearchChoice::new(
        String::new(),empty_label.to_owned(),empty_label.to_owned()
    )];
    for brand in brands{
        let id=field(brand,"id");
        if id.is_empty(){continue;}
        let name=field(brand,"name");
        let korean=field(brand,"korean");
        let aliases=rows(&brand["aliases"]).iter()
            .filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
        choices.push(components::ManagementSearchChoice::new(
            id.to_owned(),name.to_owned(),
            format!("{name} {korean} {aliases}")
        ));
    }
    choices
}
fn price(value: &Value) -> String {
    value.as_i64().map(components::display_won)
        .unwrap_or_else(|| "—".to_owned())
}
fn price_history_row(entry:&Value)->Vec<String>{
    let raw_field=field(entry,"field");
    let tier=match raw_field.rsplit('.').next().unwrap_or(raw_field){
        "wholesale"=>"도매가","semi"=>"반도매가","retail"=>"소매가",
        other=>other,
    };
    let raw_at=field(entry,"at");
    let at=chrono::DateTime::parse_from_rfc3339(raw_at)
        .map(|date|date.with_timezone(&chrono::Local).format("%Y.%m.%d %H:%M").to_string())
        .unwrap_or_else(|_|raw_at.to_owned());
    let amount=|value:&Value|if value.is_null(){"미등록".to_owned()}else{
        value.as_i64().map(components::display_won).unwrap_or_else(||
            value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string()))
    };
    let source=match field(entry,"source"){
        "MANUAL"=>"직접 수정","MANUAL_EXCEL"|"XLSX"=>"엑셀 수정",
        "OFFICIAL_URL"|"OFFICIAL"=>"공식 가격 갱신",""=>"—",other=>other,
    };
    vec![at,tier.to_owned(),amount(&entry["before"]),amount(&entry["after"]),source.to_owned()]
}
use crate::product_option_display::variant_label as option_text;

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
/// The product table uses one column geometry in browse and price-edit mode.
fn product_table_base_widths()->[f32;6]{
    [theme::PRODUCT_TABLE_NAME_MIN_WIDTH,theme::PRODUCT_TABLE_OPTION_WIDTH,
     theme::PRODUCT_TABLE_PRICE_WIDTH,theme::PRODUCT_TABLE_PRICE_WIDTH,
     theme::PRODUCT_TABLE_PRICE_WIDTH,theme::PRODUCT_TABLE_ACTION_WIDTH]
}

fn product_table_column_widths(data_width:f32)->[f32;6]{
    theme::management_single_stretch_columns(
        product_table_base_widths(),data_width,theme::SPACE_8,0
    )
}

/// A 116pt price column must hold the shared 112pt money field with a 2pt
/// horizontal breathing room on EACH side. The painting clip belongs to the
/// containing column, not the narrower editor rectangle: otherwise egui's
/// focused TextEdit outline loses its right-hand stroke at the clip edge.
fn product_price_editor_rects(cell:Rect,scroll_clip:Rect)->(Rect,Rect){
    let inset=theme::SPACE_4/2.0;
    (cell.shrink2(Vec2::new(inset,0.0)),cell.intersect(scroll_clip))
}


fn product_table_text(ui:&mut Ui,rect:Rect,text:&str,
    role:typography::ManagementTableCellRole,unique:impl std::hash::Hash){
    components::paint_management_text_cell(
        ui,rect.shrink2(Vec2::new(theme::SPACE_8,0.0)),text,role,
        egui::Align2::LEFT_CENTER,("product-table-text",unique)
    );
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
fn accepts_catalog_revision(current:u64,incoming:u64)->bool{
    incoming>=current
}
#[cfg(test)]
fn price_navigation_key(
    enter:bool,tab:bool,left:bool,right:bool,up:bool,down:bool,shift:bool
)->Option<CellNavigationKey>{
    if tab{Some(if shift{CellNavigationKey::Previous}else{CellNavigationKey::Next})}
    else if enter{Some(if shift{CellNavigationKey::Up}else{CellNavigationKey::Down})}
    else if left{Some(CellNavigationKey::Left)}
    else if right{Some(CellNavigationKey::Right)}
    else if up{Some(CellNavigationKey::Up)}
    else if down{Some(CellNavigationKey::Down)}
    else{None}
}
fn price_navigation_target(
    order:&[CellNavigationCell],current:&str,key:CellNavigationKey
)->Option<String>{
    cell_navigation::resolve_cell_navigation_target(order,current,key)
}
struct PriceDraft {
    original:Option<i64>,
    input:String,
}
impl PriceDraft {
    /// The write has committed. Advance the comparison baseline immediately,
    /// before the asynchronous product-list query can repaint stale rows.
    fn mark_saved(&mut self,value:Option<i64>){
        self.original=value;
        self.input=value.map(|amount|amount.to_string()).unwrap_or_default();
    }
    /// When the edited text is numerically identical to the committed value,
    /// restore the canonical representation on blur (e.g. 0 on an empty cell
    /// means "unregistered", not a visible literal zero).
    fn reset_to_original(&mut self){
        self.input=self.original.map(|amount|amount.to_string())
            .unwrap_or_default();
    }
    fn normalize_if_unchanged(&mut self){
        if !self.dirty(){self.reset_to_original();}
    }
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

#[derive(Clone,Debug,PartialEq,Eq)]
struct PriceUpdate {
    product_id:String,
    variant_id:String,
    tier:String,
    value:Option<i64>,
}
impl PriceUpdate {
    fn from_payload(data:&Value)->Option<Self>{
        let product_id=data.get("productId")?.as_str()?.to_owned();
        let variant_id=data.get("variantId")?.as_str()?.to_owned();
        let tier=data.get("tier")?.as_str()?;
        if !["wholesale","semi","retail"].contains(&tier){return None;}
        let value=match data.get("price")?{
            Value::Null=>None,
            Value::Number(number)=>Some(number.as_i64()?),
            _=>return None,
        };
        Some(Self{product_id,variant_id,tier:tier.to_owned(),value})
    }
    /// Update the currently displayed query snapshot synchronously. The
    /// subsequent async query is still authoritative, but must not repaint
    /// the just-saved cell with its previous value in the meantime.
    fn apply_to_products(&self,products:&mut [Value])->bool{
        let Some(product)=products.iter_mut().find(|product|
            field(product,"id")==self.product_id
        )else{return false;};
        let Some(variant)=product["variants"].as_array_mut()
            .and_then(|variants|variants.iter_mut().find(|variant|
                field(variant,"id")==self.variant_id
            ))else{return false;};
        variant[self.tier.as_str()]=self.value.map_or(
            Value::Null,|amount|json!(amount)
        );
        true
    }
}
struct QueuedPriceWrite {
    key:String,
    data:Value,
}
impl QueuedPriceWrite {
    fn from_draft(key:&str,draft:&PriceDraft)->Result<Self,String>{
        let mut parts=key.splitn(3,':');
        let (Some(product_id),Some(variant_id),Some(tier))=
            (parts.next(),parts.next(),parts.next())else{
            return Err("가격 셀 식별자를 확인할 수 없습니다.".into());
        };
        if product_id.is_empty()||variant_id.is_empty()
            ||!["wholesale","semi","retail"].contains(&tier){
            return Err("가격 셀 식별자를 확인할 수 없습니다.".into());
        }
        let value=draft.parse()?;
        Ok(Self{key:key.to_owned(),data:json!({
            "productId":product_id,"variantId":variant_id,"tier":tier,
            "price":value,"expected":draft.original
        })})
    }
}
struct ProductWrite {
    kind:String,
    key:Option<String>,
    price_update:Option<PriceUpdate>,
    receiver:Receiver<Result<Value,String>>,
}

pub struct ProductManager {
    gateway:Arc<dyn BandoGateway>,
    selection_client:Option<String>,
    selection_result:Option<Value>,
    focus_search:bool,
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
    original_selected:Option<Value>,
    detail_prices:HashMap<String,String>,
    confirm_discard:bool,
    confirm_archive:bool,
    price_mode:bool,
    price_drafts:HashMap<String,PriceDraft>,
    price_write_queue:VecDeque<QueuedPriceWrite>,
    price_mode_exit_requested:bool,
    price_focus_target:Option<String>,
    // Track the actual final focus owner, not just egui's transient
    // lost_focus flag (which can miss a release between render passes).
    price_previous_focus:Option<egui::Id>,
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

fn selectable_order_product(product:&Value)->bool{
    product["active"]!=false && product["requiresOption"]!=true
        && rows(&product["variants"]).len()==1
}

fn product_summary_counts(catalog:Option<&Value>)->Option<[usize;4]>{
    let catalog=catalog?;
    Some([
        rows(&catalog["products"]).len(),
        catalog["settings"]["colorGuidanceGroups"]
            .as_object().map_or(0,|groups|groups.len()),
        rows(&catalog["aliases"]).len(),
        rows(&catalog["brands"]).len()
    ])
}

impl ProductManager {
    pub fn active_tab_name(&self)->&'static str{
        ["상품 목록","카테고리 관리","모델군 관리","별칭 관리","브랜드 관리"]
            .get(self.tab).copied().unwrap_or("상품 관리")
    }
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let mut page=Self {
            gateway,selection_client:None,selection_result:None,focus_search:false,
            catalog:None,products:Vec::new(),
            query:String::new(),tab:0,
            category_manager:CategoryManager::default(),
            model_groups:ModelGroupManager::default(),
            aliases:AliasManager::default(),
            brands:BrandManager::default(),category_next_tab:None,
            category:String::new(),
            brand:String::new(),missing:false,selected:None,
            original_selected:None,detail_prices:HashMap::new(),
            confirm_discard:false,confirm_archive:false,
            price_mode:false,price_drafts:HashMap::new(),
            price_write_queue:VecDeque::new(),
            price_mode_exit_requested:false,
            price_focus_target:None,
            price_previous_focus:None,
            selected_variants:HashMap::new(),sort:None,
            excel:ProductExcelUi::default(),
            photos:ProductImageCache::default(),
            writing:None,notice:None,
            result:LatestJob::default(),next_query:None,error:None,
        };
        page.reload(ctx);
        page
    }
    pub fn new_selection(gateway:Arc<dyn BandoGateway>,ctx:Context,client_id:String)->Self{
        let mut page=Self::new(gateway,ctx.clone());
        page.selection_client=Some(client_id);
        page.focus_search=true;
        page.reload(ctx);
        page
    }
    pub fn show_selection_modal(
        &mut self,ctx:&Context,work_area:Rect,window_id:&'static str
    )->(bool,Option<Value>){
        let mut cancelled=false;
        let width=(work_area.width()-theme::SPACE_12*2.0).clamp(1.0,1120.0);
        let height=components::modal_body_height_budget(work_area.height(),false).min(650.0);
        components::modal_window(ctx,"상품 선택",window_id,work_area)
            .default_width(width).show(ctx,|ui|{

                ui.set_width(width);
                components::modal_flow_workspace(ui,|workspace|{
                    cancelled=workspace.header("상품 선택","취소",true);
                    workspace.body_region(|ui|{
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(),height.min(ui.available_height().max(1.0))),Layout::top_down(Align::Min),
                            |body|self.show_page(body)
                        );
                    });
                });
            });
        (cancelled || crate::overlay::take_modal_escape(ctx),
            self.take_selection())
    }
    pub fn take_selection(&mut self)->Option<Value>{
        self.selection_result.take()
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
        self.price_write_queue.clear();
        self.price_mode_exit_requested=false;
        self.price_focus_target=None;
        self.price_previous_focus=None;
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
        if !self.has_pending_prices(){self.error=None;}
        let selection_client=self.selection_client.clone();
        self.result.submit(ctx,move||{
            if let Some(client)=selection_client{
                let result=management_queries::query_manual_products(
                    gateway.as_ref(),cached,&query,&client,&category,&brand
                )?;
                Ok(ManagementProductRows{catalog:result.catalog,products:result.products})
            }else{
                management_queries::query_management_products(
                    gateway.as_ref(),cached,&query,&category,&brand,missing,
                )
            }
        });
    }
    fn has_pending_prices(&self)->bool{
        self.price_drafts.values().any(PriceDraft::dirty)
    }
    #[cfg(test)]
    pub fn blocks_navigation(&self)->bool{
        self.selected.is_some() || self.has_pending_prices()
            || !self.price_write_queue.is_empty()
            || self.writing.is_some() || self.excel.blocks_navigation()
    }
    fn price_cell_busy(&self,key:&str)->bool{
        self.writing.as_ref().is_some_and(|job|
            job.kind=="set_price"&&job.key.as_deref()==Some(key)
        )||self.price_write_queue.iter().any(|queued|queued.key==key)
    }
    fn queue_dirty_price_writes(&mut self,ctx:Context)->Result<(),String>{
        let mut writes=Vec::new();
        for (key,draft) in &self.price_drafts{
            if draft.dirty()&&!self.price_cell_busy(key){
                writes.push(QueuedPriceWrite::from_draft(key,draft)?);
            }
        }
        for write in writes{
            self.start_write("set_price",write.data,Some(write.key),ctx.clone());
        }
        Ok(())
    }
    fn finish_price_mode_exit_if_ready(&mut self){
        if !self.price_mode_exit_requested||self.writing.is_some()
            ||!self.price_write_queue.is_empty(){return;}
        self.price_mode_exit_requested=false;
        if !self.has_pending_prices(){
            self.price_mode=false;
            self.price_focus_target=None;
            self.price_previous_focus=None;
            self.price_drafts.clear();
        }
    }
    fn start_write(
        &mut self,action:&str,data:Value,key:Option<String>,ctx:Context
    ){
        if action=="set_price"{
            let Some(price_key)=key.as_deref()else{
                self.error=Some("가격 셀 식별자를 확인할 수 없습니다.".into());
                return;
            };
            if self.price_cell_busy(price_key){return;}
            if self.writing.is_some(){
                if self.writing.as_ref().is_some_and(|job|job.kind=="set_price"){
                    self.price_write_queue.push_back(QueuedPriceWrite{
                        key:price_key.to_owned(),data
                    });
                }
                return;
            }
        }else if self.writing.is_some(){return;}
        self.launch_write(action,data,key,ctx);
    }
    fn launch_next_price_write(&mut self,ctx:Context){
        if self.writing.is_some(){return;}
        if let Some(next)=self.price_write_queue.pop_front(){
            self.launch_write("set_price",next.data,Some(next.key),ctx);
        }
    }
    fn launch_write(
        &mut self,action:&str,data:Value,key:Option<String>,ctx:Context
    ){
        if action!="set_price"{
            crate::floating_notice::dispatch_pointer(&ctx,"products.action");
        }
        let Some(revision)=self.catalog.as_ref()
            .and_then(|catalog|catalog["revision"].as_u64()) else{
            if action=="set_price"{
                self.price_write_queue.clear();
                self.price_mode_exit_requested=false;
            }
            self.error=Some("최신 상품 정보를 확인할 수 없습니다.".into());
            return;
        };
        if !self.gateway.product_write_enabled(){
            if action=="set_price"{
                self.price_write_queue.clear();
                self.price_mode_exit_requested=false;
            }
            self.error=Some("상품 변경 저장 권한이 없습니다.".into());
            return;
        }
        let price_update=if action=="set_price"{
            let Some(update)=PriceUpdate::from_payload(&data)else{
                self.price_write_queue.clear();
                self.price_mode_exit_requested=false;
                self.error=Some("가격 저장 정보를 확인할 수 없습니다.".into());
                return;
            };
            Some(update)
        }else{None};
        let gateway=Arc::clone(&self.gateway);
        let name=action.to_owned();
        let (tx,rx)=mpsc::channel();
        self.writing=Some(ProductWrite{
            kind:name.clone(),key,price_update,receiver:rx,
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
                    let price_job=job.kind=="set_price";
                    if let Some(update)=job.price_update.as_ref(){
                        update.apply_to_products(&mut self.products);
                        if let Some(key)=job.key.as_deref(){
                            self.price_drafts.entry(key.to_owned())
                                .and_modify(|draft|draft.mark_saved(update.value))
                                .or_insert_with(||PriceDraft{
                                    original:update.value,
                                    input:update.value.map(|amount|
                                        amount.to_string()
                                    ).unwrap_or_default()
                                });
                        }
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
                    // Spreadsheet-style price edits commit silently per cell.
                    // A global toast for every blur would interrupt rapid entry;
                    // validation and write failures still use the error channel.
                    self.notice=if price_job{
                        None
                    }else{
                        Some(category_notice.unwrap_or(match job.kind.as_str(){
                            "upsert_product"=>"상품 상세 정보를 저장했습니다.",
                            "archive_product"=>"상품을 삭제하고 과거 기록을 보존했습니다.",
                            "change_category"=>"카테고리 변경사항을 저장했습니다.",
                            "delete_category"=>"빈 분류를 삭제했습니다.",
                            "save_alias"=>"별칭을 저장했습니다.",
                            "upsert_brand"=>"브랜드 정보를 저장했습니다. 기존 연결 상품은 유지됩니다.",
                            _=>"상품 변경을 저장했습니다.",
                        }).into())
                    };
                    self.model_groups.set_catalog(&catalog,ctx);
                    self.aliases.set_catalog(&catalog,ctx);
                    self.catalog=Some(catalog);
                    if price_job&&!self.price_write_queue.is_empty(){
                        self.launch_next_price_write(ctx.clone());
                    }else{
                        self.reload(ctx.clone());
                        self.finish_price_mode_exit_if_ready();
                    }
                }
            }
            Some(Ok(Err(error)))=>{
                if let Some(job)=self.writing.take(){
                    if job.kind=="set_price"{
                        // Keep every cell draft for correction/retry, but
                        // discard queued snapshots so edits cannot go stale.
                        self.price_write_queue.clear();
                        self.price_mode_exit_requested=false;
                    }
                }
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
                let disconnected=self.writing.take();
                let brand_write=disconnected.as_ref()
                    .is_some_and(|job|job.kind=="upsert_brand");
                if disconnected.as_ref().is_some_and(|job|job.kind=="set_price"){
                    self.price_write_queue.clear();
                    self.price_mode_exit_requested=false;
                }
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
                    let current_revision=self.catalog.as_ref()
                        .and_then(|catalog|catalog["revision"].as_u64())
                        .unwrap_or(0);
                    let incoming_revision=result.catalog["revision"].as_u64()
                        .unwrap_or(0);
                    if accepts_catalog_revision(current_revision,incoming_revision){
                        self.model_groups.set_catalog(&result.catalog,ctx);
                        self.aliases.set_catalog(&result.catalog,ctx);
                        self.catalog=Some(result.catalog);
                        self.products=result.products;
                        if !self.has_pending_prices(){self.error=None;}
                    }else{
                        // A query started before a price write may finish
                        // after it. Never let that older snapshot repaint
                        // the newly committed price back to its prior value.
                        self.reload(ctx.clone());
                    }
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
        if self.writing.is_some()||!self.price_write_queue.is_empty(){
            self.error=Some("상품 저장이 완료된 뒤 다른 화면으로 이동하세요.".into());
            return false;
        }
        if self.selected.is_some(){return false;}
        if self.has_pending_prices(){
            self.error=Some("가격 입력을 마치고 저장이 완료된 뒤 이동하세요.".into());
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
            self.error=Some("가격 입력 저장이 완료된 뒤 상품 상세를 열 수 있습니다.".into());
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
            "origin":"FACTORY","defaultColor":"",
            "active":true,"saleStatus":"판매중","saleUnit":true,"colorStatus":"NOT_REQUIRED",
            "variants":[{
                "id":"","color":"","option":"",
                "wholesale":null,"semi":null,"retail":null,
                "url":"","source":"MANUAL"
            }],
            "createdAt":at,"updatedAt":at
        }));
    }

    pub fn show_page(&mut self,ui:&mut Ui){
        let page_origin_y=ui.cursor().top();
        self.poll(ui.ctx());
        // A pointer action supersedes any deferred keyboard focus target.
        if ui.input(|input|input.pointer.any_click()){
            self.price_focus_target=None;
        }
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
        let selecting=self.selection_client.is_some();
        if !selecting{
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
                self.error=Some("가격 입력 저장이 완료된 뒤 탭을 이동할 수 있습니다.".into());
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
            ui.add_space(theme::SPACE_12);
        }else{
            ui.add_space(theme::SPACE_12);
            let counts=product_summary_counts(self.catalog.as_ref());
            components::management_top_content(ui,|top|{
                top.horizontal_wrapped(|strip|{
                    strip.spacing_mut().item_spacing=Vec2::new(
                        theme::SPACE_8,theme::SPACE_8
                    );
                    if let Some([products,model_groups,aliases,brands])=counts{
                        components::management_summary_card(strip,"등록 상품",products);
                        components::management_summary_card(strip,"모델군",model_groups);
                        components::management_summary_card(strip,"별칭",aliases);
                        components::management_summary_card(strip,"브랜드",brands);
                    }else{
                        for label in ["등록 상품","모델군","별칭","브랜드"]{
                            components::management_summary_card_value(strip,label,"—");
                        }
                    }
                });
            });
            ui.add_space(theme::SPACE_12);
        }
        }
        let can_edit=!selecting && self.gateway.product_write_enabled()
            && self.catalog.as_ref().is_some_and(|c|c["revision"].as_u64().is_some());
        let saving=self.writing.is_some()||self.excel.busy();
        let mut price_write:Option<(String,Value)>=None;
        let mut open_detail:Option<Value>=None;
        let mut picked:Option<Value>=None;
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
                components::management_pending_primary_card(ui);
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
                components::management_pending_primary_card(ui);
            }
            return;
        }
        if self.tab==2 {
            if let Some(catalog)=self.catalog.as_ref(){
                self.model_groups.show_page(ui,catalog,Arc::clone(&self.gateway));
            }else{
                components::management_pending_primary_card(ui);
            }
            return;
        }
        if self.tab==1 {
            if let Some(catalog)=self.catalog.as_ref(){
                self.category_manager.show_page(ui,catalog,can_edit,saving);
            }else{
                components::management_pending_primary_card(ui);
            }
            return;
        }
        // Product list starts at the exact Freight Assignment card anchor.
        // Actions and optional Excel/registration editors stay INSIDE the card.
        components::align_management_primary_card(ui,page_origin_y);
        let card_min_height=components::management_master_table_card_min_inner_height(ui);
        components::measured_management_master_card(ui,|panel|{
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
                    let search_response=components::management_search_field(
                        filter,&mut self.query,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                        if selecting{"상품명 / 모델 / 별칭 검색"}else{"모델명 또는 상품명 검색"}
                    );
                    if self.focus_search{
                        search_response.request_focus();
                        self.focus_search=false;
                    }
                    if search_response.changed(){
                        if self.has_pending_prices(){
                            self.query=before;
                            self.error=Some("가격 입력 저장이 완료된 뒤 검색 조건을 변경할 수 있습니다.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    let category_nodes=self.catalog.as_ref()
                        .map(category_dropdown::nodes_from_catalog)
                        .unwrap_or_default();
                    let brands=self.catalog.as_ref().map(|catalog|
                        rows(&catalog["brands"]).to_vec()).unwrap_or_default();
                    let before_category=self.category.clone();
                    // Product list uses a 30%-wider 364pt trigger while
                    // retaining the shared 34pt control height. Category
                    // hierarchy popup width and other selectors stay unchanged.
                    category_dropdown::category_dropdown(
                        filter,&category_nodes,&mut self.category,
                        theme::PRODUCT_CATEGORY_FILTER_TRIGGER_WIDTH,
                        "전체 카테고리",true,true
                    );
                    if self.category!=before_category{
                        if self.has_pending_prices(){
                            self.category=before_category;
                            self.error=Some("가격 입력 저장이 완료된 뒤 검색 조건을 변경할 수 있습니다.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    let before_brand=self.brand.clone();
                    let brand_text=brands.iter().find(|b|field(b,"id")==self.brand)
                        .map(|b|field(b,"name")).unwrap_or("전체 브랜드");
                    let brand_choices=brand_search_choices(&brands,"전체 브랜드");
                    components::management_searchable_dropdown(
                        filter,brand_text,theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH,
                        true,&mut self.brand,&brand_choices,"브랜드 검색"
                    );
                    if self.brand!=before_brand{
                        if self.has_pending_prices(){
                            self.brand=before_brand;
                            self.error=Some("가격 입력 저장이 완료된 뒤 검색 조건을 변경할 수 있습니다.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    if !selecting{
                    let before_missing=self.missing;
                    if components::checkbox(
                        filter,&mut self.missing,"가격 미등록만",true
                    ).changed(){
                        if self.has_pending_prices(){
                            self.missing=before_missing;
                            self.error=Some("가격 입력 저장이 완료된 뒤 검색 조건을 변경할 수 있습니다.".into());
                        }else{self.changed(filter.ctx());}
                    }
                    }
            });

            // The trailing slot ends at the CARD inner-right edge, without
            // subtracting the table-only scrollbar gutter (master contract).
            if !selecting{
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
                        theme::ButtonRole::Primary,can_edit&&!saving&&!self.has_pending_prices()
                    ).clicked(){
                        begin_new=true;
                    }
                    if theme::action(actions,
                        if self.price_mode{"가격 수정 모드 종료"}
                        else{"가격 수정 모드 켜기"},
                        if self.price_mode{theme::ButtonRole::Primary}
                        else{theme::ButtonRole::Standard},
                        can_edit&&!saving
                    ).clicked(){
                        if self.price_mode{
                            // The table commits the focused cell first; all
                            // remaining dirty drafts are queued after render.
                            self.price_mode_exit_requested=true;
                        }else{
                            self.price_mode=true;
                            self.price_mode_exit_requested=false;
                            self.price_focus_target=None;
                        }
                    }
                    // Match the shared management dropdown and overlay layer.
                    let trigger=components::management_dropdown_trigger(
                        actions,"Excel 관리",112.0,
                        !saving&&!self.has_pending_prices(),None
                    );
                    self.excel.show_menu(
                        actions,&trigger,Arc::clone(&self.gateway),
                        self.catalog.is_some()&&self.writing.is_none()
                            &&!self.has_pending_prices()
                    );
                }
            );
            }
            // Product list geometry must NOT depend on transient fetch or
            // save state. Inline "loading/saving" strips used to shift the
            // sticky header and virtualized table whenever a filter changed.
            // Loading is represented by existing disabled-action state;
            // completed results and errors use their established channels.
            // Registration and Excel use floating dialogs/menus;
            // neither inserts a row or moves the table header.
            let original_spacing=components::management_table_container_style(panel);
            // Fixed six-column geometry: editing changes only a cell's
            // contents, never its width, column origin or scroll range.
            let base=product_table_base_widths();
            let min_width=base.iter().sum::<f32>()+5.0*theme::SPACE_8;
            let min_table_width=theme::PRODUCT_TABLE_MIN_WIDTH.max(min_width);
            let layout=components::management_sticky_table_header(
                panel,min_table_width
            );
            let table_width=layout.table_width;
            let widths=product_table_column_widths(table_width);
            let headings=[
                components::ManagementTableColumnLabel::ProductName.text(),
                "색상/옵션","도매가",
                "반도매가","소매가",if selecting{"선택"}else{"상세"}];
            let selection_locked=self.has_pending_prices();
            let selection_ready=selecting && self.next_query.is_none()
                && !self.result.pending() && self.error.is_none();
            let ordered=product_sort_order(
                &self.products,&self.selected_variants,self.sort
            );
            // Each selected variant contributes an explicit three-column row.
            // Busy cells remain in the map as disabled slots so coordinates
            // never shift when an asynchronous write is in flight.
            let mut price_order=Vec::new();
            let other_price_write_busy=self.writing.as_ref()
                .is_some_and(|job|job.kind!="set_price");
            for (row_index,product_index) in ordered.iter().enumerate(){
                let product=&self.products[*product_index];
                let product_id=field(product,"id");
                let variants=rows(&product["variants"]);
                let selected_id=self.selected_variants.get(product_id)
                    .filter(|selected|variants.iter().any(|variant|
                        field(variant,"id")==selected.as_str()
                    ))
                    .cloned()
                    .or_else(||variants.first().map(|variant|
                        field(variant,"id").to_owned()));
                if let Some(variant_id)=selected_id{
                    for (column,tier) in ["wholesale","semi","retail"].iter().copied().enumerate(){
                        let key=format!("{}:{}:{}",product_id,variant_id,tier);
                        price_order.push(CellNavigationCell{
                            id:key.clone(),row:row_index,column,
                            order:row_index*3+column,
                            enabled:!other_price_write_busy &&
                                !self.price_mode_exit_requested &&
                                !self.price_cell_busy(&key),
                        });
                    }
                }
            }
            let header_rect=layout.header_rect;
            // Only optional add rows consume space; the Excel popup,
            // asynchronous loading, and saving never do.
            // Pending, empty and populated results share ONE table height.
            // Never subtract an empty-state row from the scroll viewport.
            let scroll_max_height=components::management_single_list_scroll_height(
                panel,card_min_height,0.0
            );
            // The shared component owns the 7pt rail extension.
            let scroll_track=components::management_table_scroll_track_rect(
                panel,scroll_max_height
            );
            let mut row_left=None;
            let scroll_output=components::virtual_fixed_rows(
                components::management_fixed_table_scroll_area(panel,table_width)
                    .scroll_bar_rect(scroll_track)
                    .id_salt("egui-product-price-grid")
                    .max_height(scroll_max_height)
                    .auto_shrink([false,false]),
                panel,table_width,theme::PRODUCT_TABLE_ROW_HEIGHT,ordered.len(),
                |table,visible_rows|{
                    components::management_table_row_body(table,table_width);
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
                        #[cfg(test)]
                        components::general_screen_geometry_tests::record("data row",row_rect);
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
                                components::paint_management_table_row_background(row.painter(),row_rect,row_index);
                                let cells=components::management_table_cells(
                                    row_rect.min,&widths,theme::PRODUCT_TABLE_ROW_HEIGHT,theme::SPACE_8);
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
                                                true,|choices|{
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
                                        &label,
                                        typography::ManagementTableCellRole::Option,(product_id,1));
                                }else{
                                    product_table_text(row,cells[1],"",typography::ManagementTableCellRole::Option,(product_id,1));
                                }
                                self.selected_variants.insert(
                                    product_id.to_owned(),selected_id.clone()
                                );
                                let selected_variant=variants.iter().find(|v|
                                    field(v,"id")==selected_id.as_str()
                                );
                                for (offset,tier) in ["wholesale","semi","retail"]
                                    .iter().enumerate(){
                                    let (price_rect,price_paint_clip)=
                                        product_price_editor_rects(cells[offset+2],row_clip);
                                    if let Some(variant)=selected_variant{
                                        if self.price_mode {
                                            let key=format!(
                                                "{}:{}:{}",product_id,
                                                field(variant,"id"),tier
                                            );
                                            let original=variant[*tier].as_i64();
                                            let cell_busy=self.writing.as_ref().is_some_and(|job|
                                                job.kind=="set_price"&&job.key.as_deref()==Some(key.as_str())
                                            )||self.price_write_queue.iter().any(|queued|
                                                queued.key==key
                                            );
                                            let other_write_busy=saving&&!self.writing.as_ref()
                                                .is_some_and(|job|job.kind=="set_price");
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
                                            if other_write_busy||cell_busy
                                                ||self.price_mode_exit_requested{
                                                builder=builder.disabled();
                                            }
                                            row.scope_builder(builder,|inputs|{
                                                    // Do not clip TextEdit's
                                                    // focus stroke to its own
                                                    // 112pt allocation. Preserve
                                                    // the adjacent 2pt gutters.
                                                    inputs.set_clip_rect(price_paint_clip);
                                                    inputs.push_id(
                                                        ("product-price-cell",product_id,
                                                         field(variant,"id"),tier),
                                                        |inputs|{
                                                            // Keep the numeric ending and the won
                                                            // suffix on the same right-hand anchor
                                                            // as the read-only "26,000원" cell.
                                                            // The field ends 2pt before its column,
                                                            // whereas display text ends 8pt before
                                                            // it: 8 - 2 = 6pt suffix inset.
                                                            // Capture and consume the current focused cell's
                                                            // arrows BEFORE TextEdit treats them as caret moves.
                                                            // Keep each variant/tier's IME state isolated.
                                                            let editor_slot=egui::Id::new((
                                                                "product-price-grid-editor",key.as_str()
                                                            ));
                                                            let navigation_key=cell_navigation::
                                                                consume_worksheet_navigation_before_editor(
                                                                    inputs.ctx(),editor_slot,
                                                                    CellNavigationKey::Down,
                                                                    CellNavigationKey::Up
                                                                );
                                                            let escape_pressed=crate::overlay::cell_escape_requested(inputs.ctx());
                                                            let response=components::money_field_with_unit(
                                                                inputs,&mut entry.input,true,"원",
                                                                theme::SPACE_8-theme::SPACE_4/2.0
                                                            );
                                                            cell_navigation::remember_worksheet_editor(
                                                                inputs.ctx(),editor_slot,response.id
                                                            );
                                                            if response.gained_focus(){
                                                                select_all_price_text(
                                                                    inputs,&response,&entry.input
                                                                );
                                                            }
                                                            crate::floating_notice::sized_text_caret(
                                                                inputs,"products.price",&response,
                                                                &entry.input,typography::numeric_editor_font(),
                                                                false,8.0
                                                            );
                                                            // Excel-style cell editing: blur commits
                                                            // automatically. Enter moves vertically,
                                                            // Tab moves horizontally, and Escape only
                                                            // restores this cell's last committed value.
                                                            if response.clicked(){
                                                                self.price_focus_target=None;
                                                            }
                                                            // egui can surrender TextEdit focus on Escape
                                                            // before producing the Response. The former
                                                            // focus owner must cancel, not enter blur-save.
                                                            let was_focused=self.price_previous_focus==Some(response.id);
                                                            let blurred=response.lost_focus()
                                                                ||(was_focused&&!response.has_focus());
                                                            let escape=escape_pressed&&(was_focused||response.lost_focus()
                                                                ||(response.has_focus()&&!response.gained_focus()))
                                                                && crate::overlay::take_cell_escape(&response,was_focused);
                                                            let navigation_target=navigation_key
                                                                .and_then(|navigation|
                                                                    price_navigation_target(
                                                                        &price_order,&key,navigation
                                                                    )
                                                                );
                                                            let arrow_edge=matches!(
                                                                navigation_key,
                                                                Some(CellNavigationKey::Left|
                                                                    CellNavigationKey::Right|
                                                                    CellNavigationKey::Up|
                                                                    CellNavigationKey::Down)
                                                            )&&navigation_target.is_none();
                                                            let keyboard_commit=(response.has_focus()
                                                                ||response.lost_focus())
                                                                &&navigation_key.is_some()
                                                                &&!arrow_edge;
                                                            if escape{
                                                                entry.reset_to_original();
                                                                response.surrender_focus();
                                                                self.price_focus_target=None;
                                                                self.error=None;
                                                            }else if keyboard_commit{
                                                                match entry.parse(){
                                                                    Ok(value)=>{
                                                                        self.price_focus_target=navigation_target;
                                                                        response.surrender_focus();
                                                                        if entry.dirty()&&!cell_busy{
                                                                            price_write=Some((
                                                                                key.clone(),json!({
                                                                                    "productId":product_id,
                                                                                    "variantId":field(variant,"id"),
                                                                                    "tier":tier,"price":value,
                                                                                    "expected":entry.original
                                                                                })
                                                                            ));
                                                                        }
                                                                    }
                                                                    Err(message)=>{
                                                                        self.error=Some(message);
                                                                        self.price_focus_target=None;
                                                                        response.request_focus();
                                                                    }
                                                                }
                                                                if !entry.dirty(){
                                                                    entry.normalize_if_unchanged();
                                                                }
                                                            }else if blurred{
                                                                if entry.dirty()&&!cell_busy{
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
                                                                }else if !entry.dirty(){
                                                                    entry.normalize_if_unchanged();
                                                                }
                                                            }
                                                            if self.price_focus_target.as_deref()
                                                                ==Some(key.as_str()){
                                                                response.request_focus();
                                                                // Programmatic worksheet navigation must select the
                                                                // existing amount immediately, including a target
                                                                // rendered later in the same frame.
                                                                select_all_price_text(inputs,&response,&entry.input);
                                                                self.price_focus_target=None;
                                                            }
                                                            if response.changed(){self.error=None;}
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
                                        if selecting{
                                            // Order engine accepts only independent sale units.
                                            let allowed=selection_ready && selectable_order_product(product)
                                                && selected_variant.is_some_and(|v|!field(v,"id").is_empty());
                                            if theme::action(actions,"선택",
                                                theme::ButtonRole::TableSecondary,allowed
                                            ).clicked(){
                                                picked=Some(serde_json::json!({
                                                    "productId":product_id,"variantId":selected_id
                                                }));
                                            }
                                        }else if theme::action(
                                            actions,"상세 관리",
                                            theme::ButtonRole::TableSecondary,!saving
                                        ).clicked(){
                                            open_detail=Some(product.clone());
                                        }
                                    }
                                );
                                components::management_table_column_rules(row,row_rect,&cells,theme::SPACE_8);
                                components::paint_management_table_row_rule(row.painter(),row_rect);
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
            let header_cells=components::management_table_cells(
                egui::pos2(header_left,header_rect.top()),
                &widths,header_rect.height(),theme::SPACE_8);
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
                let title_galley=typography::management_table_header_galley(panel,title);
                let arrow=column.map(|column|{
                    let (text,color)=match self.sort
                        .filter(|state|state.column==column).map(|sort|sort.direction){
                        Some(ProductSortDirection::Ascending)=>("▲",theme::SELECTION),
                        Some(ProductSortDirection::Descending)=>("▼",theme::SELECTION),
                        None=>("↕",theme::MUTED),
                    };
                    (typography::management_table_header_galley(panel,text),color)
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
                    );
                    if response.hovered(){
                        components::management_master_header_sort_hover(panel,rect);
                    }
                    if response.clicked(){clicked_sort=Some(ProductSort::next(self.sort,column));}
                }
            }
            components::management_table_column_rules(panel,header_text_rect,&header_cells,theme::SPACE_8);
            components::management_master_header_finish(panel,header_rect);
            if let Some(next)=clicked_sort{
                self.sort=next;
                panel.ctx().request_repaint();
            }
            panel.spacing_mut().item_spacing.y=original_spacing;
            let empty_state=if self.products.is_empty()
                && !self.result.pending() && self.error.is_none(){
                Some(if self.query.is_empty() && self.category.is_empty()
                    && self.brand.is_empty() && !self.missing{
                    "등록된 상품이 없습니다."
                }else{"조건에 맞는 상품이 없습니다."})
            }else{None};
            // Paint inside the preallocated body. An empty search result
            // must not add a row or change scrollbar/table geometry.
            let body_rect=Rect::from_min_size(
                scroll_track.min,
                Vec2::new(
                    (layout.viewport_width-layout.scrollbar_gutter)
                        .min(scroll_track.width()).max(0.0),
                    scroll_max_height
                )
            );
            components::management_body_state_overlay(
                panel,body_rect,empty_state
            );
        });
        // Capture focus after the entire grid (including a later cell's
        // click or keyboard handoff) has been rendered. This identifies
        // exactly one prior owner if focus disappears between passes.
        self.price_previous_focus=if self.price_mode{
            ui.memory(|memory|memory.focused())
        }else{None};
        if let Some((key,data))=price_write {
            self.start_write("set_price",data,Some(key),ui.ctx().clone());
        }
        if self.price_mode_exit_requested{
            if let Err(message)=self.queue_dirty_price_writes(ui.ctx().clone()){
                self.price_mode_exit_requested=false;
                self.error=Some(message);
            }
        }
        self.finish_price_mode_exit_if_ready();
        if let Some(product)=picked{
            self.selection_result=Some(product);
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
    /// egui Window width describes its inner content, not the outer frame.
    /// Keep the frame and the requested gutter within the Kakao-safe area.
    fn detail_modal_width(work_width:f32)->f32{
        let outer_reserve=2.0*(theme::MODAL_VIEWPORT_GUTTER
            +theme::MODAL_PANEL_PADDING_X+theme::FRAME_STROKE_WIDTH);
        theme::PRODUCT_DETAIL_MODAL_WIDTH.min((work_width-outer_reserve).max(1.0))
    }
    /// Keep the fixed image and allocate all remaining space to the overview.
    fn detail_columns(width:f32)->Option<(f32,f32)>{
        let inset=2.0*(theme::MODAL_SECTION_PADDING+theme::FRAME_STROKE_WIDTH);
        let image=theme::PRODUCT_DETAIL_IMAGE_SIZE+inset;
        let overview=width-image-theme::MODAL_MASTER_WORKSPACE_GAP;
        if overview<310.0{return None;}
        Some((overview,image))
    }
    fn detail_image_size(available:f32)->f32{
        theme::PRODUCT_DETAIL_IMAGE_SIZE.min(available.max(1.0))
    }
    fn detail_history_widths(available:f32)->Vec<f32>{
        // Share the visible width without a 500pt minimum that creates an
        // unnecessary horizontal scroll. Tooltips retain full cell text.
        let usable=(available-4.0*theme::SPACE_8).max(5.0);
        [0.27,0.13,0.19,0.19,0.22].into_iter().map(|ratio|usable*ratio).collect()
    }
    /// Both cards share the taller natural body, with each gap counted once.
    fn detail_section_body_height()->f32{
        let image=theme::PRODUCT_DETAIL_IMAGE_SIZE+theme::SPACING
            +theme::ACTION_STANDARD_HEIGHT;
        let fields=8.0*theme::INPUT_HEIGHT+6.0*theme::SPACING
            +2.0*theme::MODAL_MASTER_WORKSPACE_GAP+theme::SOFT_DIVIDER_WIDTH;
        image.max(fields)
    }
    fn detail_wide_text(
        ui:&mut Ui,draft:&mut Value,key:&str,label:&str,enabled:bool
    ){
        Self::detail_wide_text_with_width(ui,draft,key,label,enabled,theme::MODAL_FORM_LABEL_WIDTH);
    }
    fn detail_wide_text_with_width(
        ui:&mut Ui,draft:&mut Value,key:&str,label:&str,enabled:bool,label_width:f32
    ){
        components::modal_labeled_row_with_width(ui,label,label_width,|line|{
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
        // The caller's shared modal_section owns the title and divider.
        // Keep 320pt whenever possible. Only a genuinely narrow stacked
        // workspace may reduce the square to its available card width.
        let size=Self::detail_image_size(ui.available_width());
        self.photos.photo(ui,draft,size);
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
    }
    fn detail_overview_column(
        ui:&mut Ui,draft:&mut Value,
        brands:&[Value],categories:&[(String,String)],
        category_nodes:&[category_dropdown::CategoryNode],
        editable:bool,busy:bool
    ){
        Self::detail_wide_text(ui,draft,"name","상품명",editable&&!busy);
        Self::detail_wide_text(
            ui,draft,"subtitle","상품 옵션",editable&&!busy
        );
        components::modal_labeled_row(ui,"카테고리",|row|{
            let before=draft["categories"].as_array()
                .and_then(|ids|ids.last())
                .and_then(Value::as_str).unwrap_or("").to_owned();
            let mut category_id=before.clone();
            row.add_enabled_ui(editable&&!busy,|input|{
                let width=input.available_width().max(1.0);
                category_dropdown::category_dropdown(
                    input,category_nodes,&mut category_id,width,
                    "카테고리 미지정",true,true
                );
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
        components::modal_labeled_row(ui,"브랜드",|row|{
            let before=field(draft,"brandId").to_owned();
            let mut brand_id=before.clone();
            let label=brands.iter().find(|b|field(b,"id")==brand_id)
                .map(|b|field(b,"name")).unwrap_or("브랜드 미지정");
            row.add_enabled_ui(editable&&!busy,|input|{
                let width=input.available_width().max(1.0);
                let brand_choices=brand_search_choices(brands,"브랜드 미지정");
                components::management_searchable_dropdown(
                    input,label,width,true,
                    &mut brand_id,&brand_choices,"브랜드 검색"
                );
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
            let current=displayed_sale_status(draft);
            let mut selected=current.to_owned();
            line.add_enabled_ui(editable&&!busy,|input|{
                let width=input.available_width().max(1.0);
                components::management_status_dropdown(
                    input,current,width,true,
                    sale_status_color(current),|options|{
                        for state in SALE_STATES{
                            components::management_status_dropdown_option(
                                options,&mut selected,state.to_owned(),
                                state,sale_status_color(state)
                            );
                        }
                    });
            });
            if selected!=current{set_sale_status(draft,&selected);}
        });
        let mut navigation=cell_navigation::WorksheetFocus::new(
            ui.make_persistent_id("product-detail-price-cells"),
            cell_navigation::vertical_worksheet_cells(["wholesale","semi","retail"],editable&&!busy)
        );
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
                            navigation.before(editor.ctx(),tier);
                            let response=components::money_field(editor,value,true);
                            navigation.record(tier,response);
                        });
                    }
                );
                line.label(typography::field_label("원"));
            });
        }
        navigation.finish(ui.ctx());
    }
    fn detail_information_column(
        ui:&mut Ui,draft:&mut Value,prices:&mut HashMap<String,String>,
        brands:&[Value],categories:&[(String,String)],
        category_nodes:&[category_dropdown::CategoryNode],editable:bool,busy:bool
    ){
        // The shared divider owns its full 12pt on each side; group scopes
        // own only their internal 8pt row gaps, not the structural gap.
        components::modal_master_controlled_spacing(ui);
        ui.scope(|group|{
            group.spacing_mut().item_spacing.y=theme::SPACING;
            Self::detail_overview_column(group,draft,brands,categories,category_nodes,editable,busy);
        });
        components::section_divider(ui);
        ui.scope(|group|{
            group.spacing_mut().item_spacing.y=theme::SPACING;
            Self::detail_price_column(group,draft,prices,editable,busy);
        });
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
        let category_nodes=self.catalog.as_ref()
            .map(category_dropdown::nodes_from_catalog).unwrap_or_default();
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
        let modal_width=Self::detail_modal_width(work_area.width());
        components::modal_backdrop(ctx,"egui-product-detail-backdrop",false);
        components::modal_window(ctx,
            if is_new {"상품 등록"} else {"상품 상세 관리"},
            "egui-product-detail",work_area
        ).resizable(false)
            // Constrain both limits as well as the default so the previous
            // 1120pt window memory cannot keep the old three-column width.
            .default_width(modal_width)
            .min_width(modal_width)
            .max_width(modal_width)
            .show(ctx,|modal|{
                let dialog_title=if is_new{"상품 등록"}else{"상품 상세 관리"};
                // One shared owner allocates the true header, scrolling
                // body, and footer. Every boundary can now be measured.
                let (closed,_,geometry)=components::modal_editor_workspace(
                    modal,dialog_title,"취소",!busy,|workspace|{
                        workspace.body(
                            components::modal_editor_body_max_height(
                                work_area.height()
                            ),|body|{
                        let mut measured=components::ModalEditorBodyGeometry::default();
                        let content_width=body.available_width().max(1.0);
                        if let Some((overview_width,image_width))=
                            Self::detail_columns(content_width)
                        {
                            // Use the measured natural content heights from the
                            // previous frame, not the height of already padded cards.
                            let height_id=body.id().with("product-detail-paired-height");
                            let previous=body.ctx().data(|data|data.get_temp::<(f32,f32)>(height_id));
                            let target=previous.filter(|(width,_)|(width-content_width).abs()<0.5)
                                .map(|(_,height)|height).unwrap_or_else(Self::detail_section_body_height);
                            let mut natural_image=0.0_f32;
                            let mut natural_overview=0.0_f32;
                            // Image first; all eight editable fields share one overview.
                            body.horizontal(|top|{
                                top.spacing_mut().item_spacing.x=theme::MODAL_MASTER_WORKSPACE_GAP;
                                top.vertical(|media|{
                                    media.set_width(image_width);
                                    let section=components::modal_section_measured(
                                        media,"상품 이미지",|photos|{
                                            self.detail_image_column(
                                                photos,&mut draft,editable,busy,&mut photo_error
                                            );
                                            natural_image=photos.min_rect().height();
                                            photos.set_min_height(target);
                                        }
                                    );
                                    measured.cards.push(section.response.rect);
                                });
                                top.vertical(|overview|{
                                    overview.set_width(overview_width);
                                    let section=components::modal_section_measured(
                                        overview,"상품 개요",|fields|{
                                            Self::detail_information_column(
                                                fields,&mut draft,&mut prices,&brands,&categories,
                                                &category_nodes,editable,busy
                                            );
                                            natural_overview=fields.min_rect().height();
                                            fields.set_min_height(target);
                                        }
                                    );
                                    measured.cards.push(section.response.rect);
                                });
                            });
                            let measured_height=natural_image.max(natural_overview);
                            if (measured_height-target).abs()>0.25{
                                body.ctx().request_repaint();
                            }
                            body.ctx().data_mut(|data|data.insert_temp(
                                height_id,(content_width,measured_height)
                            ));
                        }else{
                            body.vertical(|stack|{
                                let section=components::modal_section_measured(
                                    stack,"상품 이미지",|photos|{
                                        self.detail_image_column(
                                            photos,&mut draft,editable,busy,&mut photo_error
                                        );
                                    }
                                );
                                measured.cards.push(section.response.rect);
                                stack.add_space(theme::SPACE_12);
                                let section=components::modal_section_measured(
                                    stack,"상품 개요",|fields|{
                                        Self::detail_information_column(
                                            fields,&mut draft,&mut prices,&brands,&categories,
                                            &category_nodes,editable,busy
                                        );
                                    }
                                );
                                measured.cards.push(section.response.rect);
                            });
                        }
                        // A single shared structural separator reserves
                        // exactly 12pt on both sides of its divider.
                        measured.divider=Some(
                            components::section_divider(body)
                        );
                        let advanced=components::collapsing_header(
                            "고급 정보"
                        )
                            .id_salt("product-detail-advanced")
                            .default_open(false)
                            .show(body,|advanced|{
                                advanced.spacing_mut().item_spacing.y=theme::SPACING;
                                // Reserve the longest label using the shared font
                                // metrics, and use that width for every advanced row.
                                let label_width=typography::field_label_galley(
                                    advanced,"홈페이지 상품 연결"
                                ).size().x.ceil().max(theme::MODAL_FORM_LABEL_WIDTH);
                                components::modal_labeled_row_with_width(
                                    advanced,"배송 그룹",label_width,|group_ui|{
                                        let before=field(&draft,"shippingGroupId").to_owned();
                                        let mut group=before.clone();
                                        let label=groups.iter().find(|g|field(g,"id")==group)
                                            .map(|g|field(g,"name")).unwrap_or("미지정");
                                        group_ui.add_enabled_ui(editable&&!busy,|input|{
                                            let width=input.available_width().max(1.0);
                                            components::management_dropdown(
                                                input,label,width,true,|choices|{
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
                                    }
                                );
                                components::modal_labeled_row_with_width(
                                    advanced,"발송지",label_width,|origin_ui|{
                                        let before=field(&draft,"origin").to_owned();
                                        let mut origin=before.clone();
                                        origin_ui.add_enabled_ui(editable&&!busy,|input|{
                                            let width=input.available_width().max(1.0);
                                            components::management_dropdown(
                                                input,if origin=="STORE"{"매장"}else{"공장"},
                                                width,true,|options|{
                                                    components::management_dropdown_option(
                                                        options,&mut origin,
                                                        "FACTORY".into(),"공장");
                                                    components::management_dropdown_option(
                                                        options,&mut origin,
                                                        "STORE".into(),"매장");
                                                });
                                        });
                                        if origin!=before{draft["origin"]=json!(origin);}
                                    }
                                );
                                if let Some(variant)=draft["variants"].as_array_mut()
                                    .and_then(|variants|variants.first_mut()){
                                    Self::detail_wide_text_with_width(
                                        advanced,variant,"url","홈페이지 상품 연결",
                                        editable&&!busy,label_width
                                    );
                                }
                                if !is_new {
                                    let history=rows(&draft["priceHistory"]);
                                    advanced.label(typography::section_heading(
                                        &format!("가격 변경 이력 {}건",history.len())
                                    ));
                                    if history.is_empty(){
                                        components::management_body_state_inline(advanced,"가격 변경 이력이 없습니다.");
                                    }else{
                                        let entries=history.iter().rev()
                                            .map(price_history_row).collect::<Vec<_>>();
                                        let height=entries.len().min(5) as f32*
                                            (theme::MANAGEMENT_COMPACT_ROW_HEIGHT+theme::SOFT_DIVIDER_WIDTH);
                                        components::management_readonly_table(
                                            advanced,"product-price-history",
                                            &["변경일시","항목","이전값","변경값","수정 경로"],
                                            &entries,height,&[2,3],Self::detail_history_widths
                                        );
                                    }
                                }
                            });
                        measured.advanced_header=Some(advanced.header_response.rect);
                        ((),measured)
                        });
                        workspace.footer(
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
                    }
                );
                request_close=closed;
                #[cfg(test)]
                modal.ctx().data_mut(|data|data.insert_temp(
                    egui::Id::new("test-product-detail-geometry"),geometry.clone()
                ));
                // Capture actual egui allocation geometry each frame.
                // The common audit is also exercised with a headless Context.
                #[cfg(debug_assertions)]
                {
                    let mismatches=geometry.invalid_gaps(
                        1.0/modal.ctx().pixels_per_point().max(1.0)
                    );
                    if !mismatches.is_empty(){
                        // Store the latest diagnostic in egui temporary data
                        // instead of spamming a production log every frame.
                        modal.ctx().data_mut(|data|{
                            data.insert_temp(
                                egui::Id::new("product-detail-modal-layout-audit"),
                                mismatches.iter().map(|(label,actual)|
                                    format!("{label}: {actual:.1}pt")).collect::<Vec<_>>()
                            );
                        });
                    }
                }
                #[cfg(not(debug_assertions))]
                let _=geometry;
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
            components::show_decision_window(ctx,"상품 삭제","egui-product-archive-confirm",work_area,|dialog|{
                components::decision_workspace(dialog,|decision|{
                    cancel=decision.header("상품 삭제","취소",!busy
                    );
                    decision.body(|dialog|{
                        dialog.label(typography::body(
                            "등록된 상품을 삭제하시겠습니까?\n\n삭제 후에도 기존 주문내역과 가격 기록은 보존됩니다."
                        ));
                    });
                    decision.actions(|actions|{
                            if theme::action(actions,"상품 삭제",theme::ButtonRole::Danger,
                                editable&&!busy).clicked(){archive=true;}
                        }
                    );
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
            components::show_decision_window(ctx,"저장하지 않은 상품","egui-product-discard-confirm",work_area,|dialog|{
                components::decision_workspace(dialog,|decision|{
                    keep=decision.header("저장하지 않은 상품","취소",!busy
                    );
                    decision.body(|dialog|{
                        dialog.label(typography::body(
                            "상품 수정사항을 저장하지 않고 닫을까요?"
                        ));
                    });
                    decision.actions(|actions|{
                            if theme::action(actions,"저장 안 함",
                                theme::ButtonRole::Danger,!busy).clicked(){discard=true;}
                        }
                    );
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
    fn price_history_uses_korean_tier_and_won_without_internal_id(){
        let entry=json!({"at":"2026-11-26T01:00:00+09:00","field":"v1.semi",
            "before":null,"after":15000});
        let row=price_history_row(&entry);
        assert_eq!(&row[1..4],["반도매가","미등록","15,000원"]);
        assert!(row.iter().all(|value|!value.contains("v1")));
    }
    #[test]
    fn product_list_fetch_does_not_insert_transient_loading_strips(){
        let source=include_str!("product_manager.rs");
        let list=source.split("pub fn show_page(").nth(1).unwrap()
            .split("    fn product_price_candidate(").next().unwrap();
        assert!(!list.contains("상품 가격표 조회 중"));
        assert!(!list.contains("상품 변경 저장 중"));
        assert!(!list.contains("if self.result.pending(){\n                theme::management_list_strip_frame()"));
        assert!(list.contains("components::management_sticky_table_header("));
        assert!(!list.contains(concat!("self.", "adding")));
        assert!(!list.contains(concat!("직접", " 등록")));
        assert!(!list.contains("components::management_list_table_header("));
        let add=list.split("theme::action(actions,\"상품 추가\"").nth(1).unwrap()
            .split("if theme::action(actions,").next().unwrap();
        assert!(add.contains("can_edit&&!saving&&!self.has_pending_prices()"));
        assert!(add.contains("begin_new=true;"));
        let registration=list.split_whitespace().collect::<String>();
        assert!(registration.contains("}elseifbegin_new{self.new_product();"));
        assert!(list.contains("self.excel.show_menu("));
        assert!(!list.contains("self.excel.show_panel("));
        assert!(!list.contains("self.excel.open()"));
        assert!(!list.contains("let empty_reserve=if self.products.is_empty(){"));
        assert!(list.contains("components::management_body_state_overlay("));
        assert!(list.contains("let scroll_max_height=components::management_single_list_scroll_height("));
    }
    #[test]
    fn product_summary_distinguishes_unloaded_catalog_from_real_zero_counts(){
        let empty=json!({
            "products":[],
            "settings":{"colorGuidanceGroups":{}},
            "aliases":[],
            "brands":[]
        });
        assert_eq!(product_summary_counts(None),None);
        assert_eq!(product_summary_counts(Some(&empty)),Some([0,0,0,0]));
        let source=include_str!("product_manager.rs");
        let rendered=source.split("pub fn show_page(").nth(1).unwrap()
            .split("let can_edit=").next().unwrap();
        assert!(rendered.contains("product_summary_counts(self.catalog.as_ref())"));
        assert!(rendered.contains(
            "components::management_summary_card_value(strip,label,\"—\")"
        ));
        assert!(!rendered.contains("unwrap_or((0,0,0,0))"));
    }
    #[test]
    fn product_detail_inline_rows_fit_with_fixed_320_image(){
        let body=ProductManager::detail_section_body_height();
        let rows=8.0*theme::INPUT_HEIGHT+6.0*theme::SPACING
            +2.0*theme::MODAL_MASTER_WORKSPACE_GAP+theme::SOFT_DIVIDER_WIDTH;
        assert!(body>=theme::PRODUCT_DETAIL_IMAGE_SIZE);
        assert_eq!(body,rows.max(theme::PRODUCT_DETAIL_IMAGE_SIZE
            +theme::SPACING+theme::ACTION_STANDARD_HEIGHT));
        assert_eq!(theme::PRODUCT_DETAIL_IMAGE_SIZE,320.0);
    }
    #[test]
    fn product_detail_modal_uses_one_shared_gap_and_exact_divider_spacing(){
        let source=include_str!("product_manager.rs");
        let composition=source.split("    pub fn show_detail(").nth(1).unwrap()
            .split("        self.selected=Some(draft.clone());").next().unwrap();
        assert!(!composition.contains("modal_header_with_workspace_gap("));
        assert!(composition.contains("components::modal_editor_workspace("));
        assert!(composition.contains("workspace.body("));
        assert!(composition.contains("workspace.footer("));
        assert!(composition.contains("ModalEditorBodyGeometry::default()"));
        assert!(composition.contains("section_divider(body)"));
        assert!(composition.contains("top.spacing_mut().item_spacing.x="));
        assert!(!composition.contains("top.add_space(theme::SPACE_12)"));
        assert!(!composition.contains("modal.add_space(theme::SPACE_12)"));
        assert!(!composition.contains("components::soft_divider(body)"));
        assert_eq!(theme::MODAL_MASTER_WORKSPACE_GAP,12.0);
        assert_eq!(theme::MODAL_SECTION_PADDING,12.0);
        assert_eq!(theme::MODAL_SECTION_HEADER_RULE_GAP,12.0);
    }
    #[test]
    fn product_detail_narrow_media_and_history_fit_available_width(){
        assert_eq!(ProductManager::detail_image_size(400.0),320.0);
        assert_eq!(ProductManager::detail_image_size(240.0),240.0);
        for width in [260.0,400.0,520.0,800.0]{
            let columns=ProductManager::detail_history_widths(width);
            assert_eq!(columns.len(),5);
            assert!(columns.iter().all(|value|*value>0.0));
            assert!((columns.iter().sum::<f32>()+4.0*theme::SPACE_8-width).abs()<0.01);
        }
    }
    #[test]
    fn product_detail_modal_width_uses_compact_master_and_safe_frame(){
        assert_eq!(theme::PRODUCT_DETAIL_MODAL_WIDTH,880.0);
        assert_eq!(ProductManager::detail_modal_width(1760.0),880.0);
        let frame=2.0*(theme::MODAL_PANEL_PADDING_X+theme::FRAME_STROKE_WIDTH);
        let gutters=2.0*theme::MODAL_VIEWPORT_GUTTER;
        let width=ProductManager::detail_modal_width(800.0);
        assert!(width+frame+gutters<=800.0);
        assert_eq!(ProductManager::detail_modal_width(10.0),1.0);
        let body=880.0-2.0*theme::MODAL_MASTER_CONTENT_INSET;
        assert!(ProductManager::detail_columns(body).is_some());
    }
    #[test]
    fn product_detail_reacts_to_actual_kakao_safe_modal_width(){
        // Two sections share the same inset and keep the fixed image.
        for width in [420.0,650.0,679.0]{
            assert!(ProductManager::detail_columns(width).is_none());
        }
        let inset=2.0*(theme::MODAL_SECTION_PADDING+
            theme::FRAME_STROKE_WIDTH);
        for width in [680.0,893.0,946.0,1100.0]{
            let (overview,image)=ProductManager::detail_columns(width).unwrap();
            assert_eq!(image,theme::PRODUCT_DETAIL_IMAGE_SIZE+inset);
            assert!(overview>=310.0);
            let gap=theme::MODAL_MASTER_WORKSPACE_GAP;
            assert!(overview+image+gap<=width+0.01,
                "boxed columns overflow {width}pt");
        }
    }
    #[test]
    fn product_detail_visual_order_is_image_overview_price_in_both_layouts(){
        let source=include_str!("product_manager.rs");
        let composition=source.split("    pub fn show_detail(").nth(1).unwrap()
            .split("        self.selected=Some(draft.clone());").next().unwrap();
        let horizontal=composition.split("body.horizontal(|top|{").nth(1).unwrap()
            .split("                        }else{").next().unwrap();
        let stacked=composition.split("body.vertical(|stack|{").nth(1).unwrap()
            .split("                        // A single shared structural separator").next().unwrap();
        for section in [horizontal,stacked]{
            let image=section.find("\"상품 이미지\"").unwrap();
            let overview=section.find("\"상품 개요\"").unwrap();
            assert!(image<overview);
            assert!(section.contains("Self::detail_information_column("));
            assert!(!section.contains("\"판매 · 가격\""));
        }
        assert_eq!(theme::PRODUCT_DETAIL_IMAGE_SIZE,320.0);
        assert!(horizontal.contains("theme::MODAL_MASTER_WORKSPACE_GAP"));
    }
    #[test]
    fn brand_selector_supports_korean_names_and_registered_aliases(){
        let brands=vec![json!({
            "id":"brand-1","name":"K&M","korean":"케이앤엠",
            "aliases":["독일브랜드","K and M"]
        })];
        let options=brand_search_choices(&brands,"전체 브랜드");
        assert_eq!(options.len(),2);
        assert_eq!(options[0].value,"");
        assert_eq!(options[0].label,"전체 브랜드");
        assert_eq!(options[1].value,"brand-1");
        assert_eq!(options[1].label,"K&M");
        for fragment in ["K&M","케이앤엠","독일브랜드","K and M"]{
            assert!(options[1].search_text.contains(fragment));
        }
        let source=include_str!("product_manager.rs");
        let filter=source.split("let brand_choices=brand_search_choices(&brands").nth(1).unwrap()
            .split("let before_missing=").next().unwrap();
        assert!(filter.contains("management_searchable_dropdown("));
        let detail=source.split("let brand_choices=brand_search_choices(brands").nth(1).unwrap()
            .split("if brand_id!=before{").next().unwrap();
        assert!(detail.contains("management_searchable_dropdown("));
    }
    #[test]
    fn product_detail_cannot_reintroduce_stacked_fields_or_static_sales_label(){
        // Architecture gate: section/field composition must stay on the
        // approved shared components, not custom vertical label + TextEdit.
        let source=include_str!("product_manager.rs");
        let detail=source.split("    fn detail_columns(").nth(1).unwrap()
            .split("    pub fn show_detail(").next().unwrap();
        assert!(detail.contains("components::modal_labeled_row("));
        assert!(!detail.contains("ui.vertical(|row|"));
        assert!(!detail.contains("● 판매중"));
        assert!(detail.contains("components::management_status_dropdown("));
        for label in ["상품명","상품 옵션","카테고리","브랜드","판매 상태"]{
            assert!(detail.contains(label),"{label} missing from inline form");
        }
        let composition=source.split("    pub fn show_detail(").nth(1).unwrap()
            .split("        self.selected=Some(draft.clone());").next().unwrap();
        for heading in ["상품 개요","상품 이미지"]{
            assert!(composition.contains(&format!(
                "components::modal_section_measured("
            )) && composition.contains(heading));
        }
    }
    #[test]
    fn four_sales_states_restore_react_values_and_prevent_sold_out_mislabeling(){
        assert_eq!(SALE_STATES,["판매중","품절","판매준비중","판매중지"]);
        for (stored,expected) in [
            ("판매 표시","판매중"),
            ("SELLING","판매중"),
            ("등록 대기","판매준비중"),
            ("판매중","판매중"),
            ("품절","품절"),
            ("판매준비중","판매준비중"),
            ("판매중지","판매중지"),
        ]{
            let item=json!({"active":true,"saleStatus":stored});
            assert_eq!(displayed_sale_status(&item),expected, "{stored}");
        }
        assert_eq!(displayed_sale_status(&json!({"active":true})),"판매중");
        assert_eq!(displayed_sale_status(&json!({
            "active":false,"saleStatus":"품절"
        })),"판매중지");
        for state in SALE_STATES{
            let color=sale_status_color(state);
            assert_ne!(color,egui::Color32::TRANSPARENT);
        }
        assert_ne!(sale_status_color("품절"),sale_status_color("판매중"));
        assert_ne!(sale_status_color("판매준비중"),sale_status_color("판매중지"));
    }
    #[test]
    fn product_status_edit_persists_without_price_or_variant_loss(){
        for status in SALE_STATES {
            let mut item=json!({"id":"p1","active":false,"saleStatus":"판매 표시",
                "variants":[{"id":"v1","wholesale":500,"semi":700,"retail":1000}]
            });
            let original_variants=item["variants"].clone();
            set_sale_status(&mut item,status);
            assert_eq!(item["saleStatus"],json!(status));
            assert_eq!(item["active"],true);
            assert_eq!(item["variants"],original_variants);
            let inputs=HashMap::from([
                ("wholesale".to_owned(),"500".to_owned()),
                ("semi".to_owned(),"700".to_owned()),
                ("retail".to_owned(),"1,000".to_owned())
            ]);
            let candidate=ProductManager::product_price_candidate(&item,&inputs)
                .expect("valid product candidate");
            assert_eq!(candidate["saleStatus"],json!(status));
            assert_eq!(candidate["variants"],original_variants);
        }
    }
    #[test]
    fn product_active_flag_is_preserved_in_existing_catalog_write(){
        let original=json!({"id":"p1","active":false,"variants":[{
            "id":"v1","wholesale":1000,"semi":1200,"retail":1500
        }]});
        let mut prices=HashMap::new();
        prices.insert("wholesale".into(),"1,000".into());
        prices.insert("semi".into(),"1,200".into());
        prices.insert("retail".into(),"1,500".into());
        let candidate=ProductManager::product_price_candidate(&original,&prices)
            .expect("valid prices");
        assert_eq!(candidate["active"],json!(false));
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
    fn clean_price_drafts_restore_react_display_value_after_blur(){
        let mut empty_zero=PriceDraft{original:None,input:"0".into()};
        assert!(!empty_zero.dirty());
        empty_zero.normalize_if_unchanged();
        assert_eq!(empty_zero.input,"");
        let mut same_amount=PriceDraft{original:Some(15000),input:"015000".into()};
        assert!(!same_amount.dirty());
        same_amount.normalize_if_unchanged();
        assert_eq!(same_amount.input,"15000");
        let mut historical_zero=PriceDraft{original:Some(0),input:"0".into()};
        historical_zero.normalize_if_unchanged();
        assert_eq!(historical_zero.input,"0");
        let mut cancelled=PriceDraft{original:Some(15000),input:"19000".into()};
        cancelled.reset_to_original();
        assert_eq!(cancelled.input,"15000");
    }
    #[test]
    fn committed_price_draft_advances_before_stale_query_rows_repaint(){
        let mut draft=PriceDraft{original:None,input:"15,000".into()};
        assert!(draft.dirty());
        draft.mark_saved(Some(15000));
        assert!(!draft.dirty());
        assert_eq!(draft.original,Some(15000));
        assert_eq!(draft.parse().unwrap(),Some(15000));
        // Explicitly clearing a previously registered price is a committed
        // null, not a new unsaved blank draft.
        draft.input.clear();
        assert!(draft.dirty());
        draft.mark_saved(None);
        assert!(!draft.dirty());
        assert_eq!(draft.parse().unwrap(),None);
    }
    #[test]
    fn price_write_updates_visible_rows_before_async_catalog_refresh(){
        let mut products=vec![json!({
            "id":"p1","variants":[
                {"id":"v1","wholesale":null,"semi":19000,"retail":26000}
            ]
        })];
        let update=PriceUpdate::from_payload(&json!({
            "productId":"p1","variantId":"v1","tier":"wholesale",
            "price":15000,"expected":null
        })).expect("price update payload");
        assert!(update.apply_to_products(&mut products));
        assert_eq!(products[0]["variants"][0]["wholesale"],json!(15000));
        let clear=PriceUpdate::from_payload(&json!({
            "productId":"p1","variantId":"v1","tier":"semi",
            "price":null,"expected":19000
        })).expect("explicit null price payload");
        assert!(clear.apply_to_products(&mut products));
        assert_eq!(products[0]["variants"][0]["semi"],Value::Null);
        assert_eq!(products[0]["variants"][0]["retail"],json!(26000));
    }
    // A controlled, in-memory transport for the real product manager.
    // No LocalGateway, files, SQLite, or operational credentials.
    struct PriceTestRequest{
        revision:u64,
        action:String,
        data:Value,
        reply:mpsc::Sender<Result<Value,String>>,
    }
    struct PriceTestGateway{
        requests:mpsc::Sender<PriceTestRequest>,
    }
    fn price_test_catalog(revision:u64,wholesale:Option<i64>,semi:Option<i64>)->Value{
        json!({
            "revision":revision,"products":[{"id":"p1","name":"Synthetic",
                "variants":[{"id":"v1","wholesale":wholesale,"semi":semi,"retail":26000}]}],
            "aliases":[],"brands":[],"categories":[],"settings":{"colorGuidanceGroups":{}}
        })
    }
    impl BandoGateway for PriceTestGateway{
        fn label(&self)->&'static str{"synthetic-price-test"}
        fn load_catalog(&self)->Result<Value,String>{
            Ok(price_test_catalog(1,None,Some(19000)))
        }
        fn load_working_source(&self)->Result<crate::preview_store::WorkingSource,String>{
            Err("working source not allowed in price test".into())
        }
        fn save_working_source(&self,_:&crate::preview_store::WorkingSource)->Result<u64,String>{
            Err("working source not allowed in price test".into())
        }
        fn execute(&self,operation:crate::gateway::ReadOperation,_:Value)->Result<Value,String>{
            match operation{
                crate::gateway::ReadOperation::ManagementProducts=>Ok(json!(["p1"])),
                _=>Err(format!("unexpected price-test read: {operation:?}"))
            }
        }
        fn product_write_enabled(&self)->bool{true}
        fn mutate_product(&self,revision:u64,action:&str,data:Value)->Result<Value,String>{
            let (reply,receiver)=mpsc::channel();
            self.requests.send(PriceTestRequest{revision,action:action.into(),data,reply})
                .map_err(|_|"price-test request channel closed".to_owned())?;
            receiver.recv_timeout(Duration::from_secs(10))
                .map_err(|_|"price-test reply timed out".to_owned())?
        }
    }
    fn poll_price_until(manager:&mut ProductManager,ctx:&Context,
        condition:impl Fn(&ProductManager)->bool){
        let deadline=Instant::now()+Duration::from_secs(10);
        loop{
            manager.poll(ctx);
            if condition(manager){return;}
            assert!(Instant::now()<deadline,"product manager did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn price_test_manager()->(ProductManager,Context,Receiver<PriceTestRequest>){
        let (requests,receiver)=mpsc::channel();
        let ctx=components::test_context();
        let mut manager=ProductManager::new(Arc::new(PriceTestGateway{requests}),ctx.clone());
        poll_price_until(&mut manager,&ctx,|m|m.catalog.is_some()&&!m.result.pending());
        assert_eq!(manager.products.len(),1);
        manager.price_mode=true;
        (manager,ctx,receiver)
    }
    fn submit_test_price(manager:&mut ProductManager,ctx:&Context,key:&str){
        let write=QueuedPriceWrite::from_draft(key,&manager.price_drafts[key]).unwrap();
        manager.start_write("set_price",write.data,Some(write.key),ctx.clone());
    }
    fn next_price_request(receiver:&Receiver<PriceTestRequest>)->PriceTestRequest{
        let request=receiver.recv_timeout(Duration::from_secs(10)).expect("price request");
        assert_eq!(request.action,"set_price");
        request
    }

    #[test]
    fn failed_price_write_keeps_dirty_draft_available_for_retry(){
        let (mut manager,ctx,requests)=price_test_manager();
        let wholesale="p1:v1:wholesale";
        let semi="p1:v1:semi";
        manager.price_drafts.insert(wholesale.into(),PriceDraft{original:None,input:"15,000".into()});
        manager.price_drafts.insert(semi.into(),PriceDraft{original:Some(19000),input:String::new()});
        submit_test_price(&mut manager,&ctx,wholesale);
        let first=next_price_request(&requests);
        assert_eq!(first.revision,1);
        assert_eq!(first.data["price"],15000);
        assert_eq!(first.data["expected"],Value::Null);
        submit_test_price(&mut manager,&ctx,semi);
        assert!(manager.price_cell_busy(semi));
        assert_eq!(manager.price_write_queue.len(),1);
        assert!(requests.try_recv().is_err(),"queued cell must not write concurrently");
        manager.price_mode_exit_requested=true;

        first.reply.send(Err("synthetic save failure".into())).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.writing.is_none());
        assert_eq!(manager.error.as_deref(),Some("synthetic save failure"));
        assert!(manager.price_write_queue.is_empty());
        assert!(manager.price_mode);
        assert!(!manager.price_mode_exit_requested);
        assert!(manager.blocks_navigation());
        assert!(manager.price_drafts[wholesale].dirty());
        assert_eq!(manager.price_drafts[wholesale].input,"15,000");
        assert_eq!(manager.price_drafts[wholesale].original,None);
        assert!(manager.price_drafts[semi].dirty());
        assert_eq!(manager.price_drafts[semi].input,"");
        assert_eq!(manager.price_drafts[semi].original,Some(19000));
        assert_eq!(manager.products[0]["variants"][0]["wholesale"],Value::Null);
        assert_eq!(manager.products[0]["variants"][0]["semi"],19000);
        assert!(requests.try_recv().is_err(),"failure must discard stale queued payloads");

        // Retry uses the current draft, not the stale queued payload.
        manager.price_drafts.get_mut(wholesale).unwrap().input="16,000".into();
        manager.price_drafts.get_mut(semi).unwrap().input="20,000".into();
        manager.queue_dirty_price_writes(ctx.clone()).unwrap();
        let mut w=None;
        let mut s=Some(19000);
        let mut seen=std::collections::HashSet::new();
        for revision in 1..=2{
            let request=next_price_request(&requests);
            assert_eq!(request.revision,revision);
            assert_eq!(request.data["productId"],"p1");
            assert_eq!(request.data["variantId"],"v1");
            let tier=request.data["tier"].as_str().unwrap().to_owned();
            assert!(seen.insert(tier.clone()),"retry must save each cell once");
            match tier.as_str(){
                "wholesale"=>{
                    assert_eq!(request.data["price"],16000);
                    assert_eq!(request.data["expected"],Value::Null);
                    w=Some(16000);
                }
                "semi"=>{
                    assert_eq!(request.data["price"],20000);
                    assert_eq!(request.data["expected"],19000);
                    s=Some(20000);
                }
                _=>panic!("unexpected retry tier {tier}")
            }
            request.reply.send(Ok(price_test_catalog(revision+1,w,s))).unwrap();
            poll_price_until(&mut manager,&ctx,|m|m.catalog.as_ref()
                .and_then(|c|c["revision"].as_u64())==Some(revision+1));
            assert!(manager.notice.is_none());
        }
        assert!(!manager.has_pending_prices());
        assert!(!manager.blocks_navigation());
        assert!(manager.price_mode);
        assert!(manager.error.is_none());
        assert_eq!(manager.products[0]["variants"][0]["wholesale"],16000);
        assert_eq!(manager.products[0]["variants"][0]["semi"],20000);
    }
    #[test]
    fn stale_catalog_queries_cannot_overwrite_newer_price_revisions(){
        assert!(accepts_catalog_revision(12,12));
        assert!(accepts_catalog_revision(12,13));
        assert!(!accepts_catalog_revision(13,12));
    }
    // Exercise the production page and actual egui TextEdit/navigation path,
    // not a copy of the editor's logic. The transport is fully synthetic.
    fn render_price_grid_frame(manager:&mut ProductManager,ctx:&Context,events:Vec<egui::Event>){
        let _=ctx.run(egui::RawInput{
            screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1760.0,1000.0))),
            events,..Default::default()
        },|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|manager.show_page(ui));
        });
    }
    fn rendered_price_editor_id(ctx:&Context,key:&str)->egui::Id{
        ctx.data(|data|data.get_temp::<egui::Id>(
            egui::Id::new(("product-price-grid-editor",key))
        )).expect("production price editor must have been rendered")
    }
    #[test]
    fn rendered_price_grid_commits_typed_amount_and_moves_to_adjacent_cell(){
        let (mut manager,ctx,requests)=price_test_manager();
        let wholesale="p1:v1:wholesale";
        let semi="p1:v1:semi";
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        manager.price_focus_target=Some(wholesale.into());
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        let editor=rendered_price_editor_id(&ctx,wholesale);
        let next_editor=rendered_price_editor_id(&ctx,semi);
        assert_ne!(editor,next_editor);
        assert!(ctx.memory(|m|m.has_focus(editor)));
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Text("15000".into())]);
        assert_eq!(manager.price_drafts[wholesale].parse().unwrap(),Some(15000));
        assert!(manager.price_drafts[wholesale].dirty());
        assert!(requests.try_recv().is_err(),"typing alone must not write");
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Key{
            key:egui::Key::ArrowRight,physical_key:None,pressed:true,repeat:false,
            modifiers:egui::Modifiers::NONE
        }]);
        let request=next_price_request(&requests);
        assert_eq!(request.revision,1);
        assert_eq!(request.data["productId"],"p1");
        assert_eq!(request.data["variantId"],"v1");
        assert_eq!(request.data["tier"],"wholesale");
        assert_eq!(request.data["price"],15000);
        assert_eq!(request.data["expected"],Value::Null);
        assert!(ctx.memory(|m|m.has_focus(next_editor)),
            "Right must focus the adjacent price cell, not move the old caret");
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        assert!(manager.price_write_queue.is_empty());
        assert!(requests.try_recv().is_err(),"blur after navigation must not duplicate the write");
        request.reply.send(Ok(price_test_catalog(2,Some(15000),Some(19000)))).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.writing.is_none());
        assert_eq!(manager.products[0]["variants"][0]["wholesale"],15000);
        assert_eq!(manager.products[0]["variants"][0]["semi"],19000);
        assert!(!manager.price_drafts[wholesale].dirty());
        assert!(manager.notice.is_none());
        assert!(manager.error.is_none());
        assert!(!manager.has_modal());
    }

    fn collect_product_text(shape:&egui::Shape,out:&mut Vec<(String,Rect)>){
        match shape{
            egui::Shape::Text(text)=>out.push((
                text.galley.text().to_owned(),
                Rect::from_min_size(text.pos,text.galley.size())
            )),
            egui::Shape::Vec(shapes)=>for shape in shapes{
                collect_product_text(shape,out);
            },
            _=>{}
        }
    }
    fn product_page_text(manager:&mut ProductManager,ctx:&Context)->Vec<(String,Rect)>{
        let mut text=Vec::new();
        for _ in 0..3{
            let output=ctx.run(egui::RawInput{
                screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1760.0,1000.0))),
                ..Default::default()
            },|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|manager.show_page(ui));
            });
            text.clear();
            for shape in &output.shapes{collect_product_text(&shape.shape,&mut text);}
        }
        text
    }
    #[test]
    fn rendered_product_kpis_show_unknown_until_catalog_arrives(){
        let (mut manager,ctx,_requests)=price_test_manager();
        manager.price_mode=false;
        manager.catalog=None;
        manager.products.clear();
        let pending=product_page_text(&mut manager,&ctx);
        manager.catalog=Some(json!({"revision":1,"products":[],"categories":[],
            "brands":[],"aliases":[],"settings":{"colorGuidanceGroups":{}}}));
        let empty=product_page_text(&mut manager,&ctx);
        for label in ["등록 상품","모델군","별칭","브랜드"]{
            let locate=|text:&[(String,Rect)],value:&str|{
                let caption=text.iter().filter(|(s,_)|s==label)
                    .map(|(_,rect)|*rect).find(|caption|{
                        text.iter().any(|(s,rect)|s==value
                            &&(rect.center().x-caption.center().x).abs()<=1.0
                            &&rect.bottom()<=caption.top()
                            &&caption.top()-rect.bottom()<60.0)
                    }).unwrap_or_else(||panic!("missing KPI {label} value {value}"));
                caption
            };
            let before=locate(&pending,"—");
            let after=locate(&empty,"0");
            assert!((before.center()-after.center()).length()<=1.0,
                "KPI {label} shifted between unloaded and empty");
        }
        assert!(!pending.iter().any(|(s,_)|s=="상품 가격표 조회 중"));
        assert!(!empty.iter().any(|(s,_)|s=="상품 변경 저장 중"));
    }
    #[test]
    fn rendered_product_detail_measures_real_card_order_and_master_gaps(){
        let (mut manager,ctx,_requests)=price_test_manager();
        manager.price_mode=false;
        manager.begin_edit(manager.products[0].clone());
        let work=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1760.0,1000.0));
        let mut text=Vec::new();
        for _ in 0..6{
            let output=ctx.run(egui::RawInput{screen_rect:Some(work),..Default::default()},
                |ctx|manager.show_detail(ctx,work));
            text.clear();
            for shape in &output.shapes{collect_product_text(&shape.shape,&mut text);}
        }
        let geometry=ctx.data(|data|data.get_temp::<components::ModalEditorGeometry>(
            egui::Id::new("test-product-detail-geometry")
        )).expect("actual product detail geometry");
        let header=geometry.header.expect("actual modal header");
        assert!((header.width()-theme::PRODUCT_DETAIL_MODAL_WIDTH).abs()<=1.0,
            "product detail must use the compact modal width: {}",header.width());
        let cards=&geometry.content.cards;
        assert_eq!(cards.len(),2);
        for (card,label) in cards.iter().zip(["상품 이미지","상품 개요"]){
            assert!(text.iter().any(|(s,rect)|s==label&&card.contains(rect.center())),
                "heading {label} is not in its measured card");
        }
        assert!(cards.windows(2).all(|pair|pair[0].right()<pair[1].left()));
        assert!((cards[0].top()-cards[1].top()).abs()<=1.0,
            "card tops must align: {cards:?}");
        assert!((cards[0].bottom()-cards[1].bottom()).abs()<=1.0,
            "card bottoms must align: {cards:?}");
        for label in ["상품명","상품 옵션","카테고리","브랜드","판매 상태","도매가","반도매가","소매가"]{
            assert!(text.iter().any(|(s,rect)|s==label&&cards[1].contains(rect.center())),
                "field {label} must stay inside the compact overview");
        }
        assert!(geometry.header.is_some()&&geometry.footer.is_some());
        assert!(geometry.content.divider.is_some()&&geometry.content.advanced_header.is_some());
        let gaps=geometry.gaps();
        for name in ["header_to_card","card_gap_1","card_to_rule",
            "rule_to_advanced","body_to_footer"]{
            assert!(gaps.iter().any(|(label,_)|*label==name),"missing {name}: {gaps:?}");
        }
        assert!(geometry.invalid_gaps(1.0).is_empty(),"actual product modal: {gaps:?}");
    }
    #[test]
    fn rendered_price_grid_escape_cancels_and_blur_saves_only_current_cell(){
        let (mut manager,ctx,requests)=price_test_manager();
        let key="p1:v1:semi";
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        manager.price_focus_target=Some(key.into());
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        let editor=rendered_price_editor_id(&ctx,key);
        // Allow the real editor to receive focus and select the old amount.
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Text("21000".into())]);
        assert_eq!(manager.price_drafts[key].parse().unwrap(),Some(21000));
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Key{
            key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,
            modifiers:egui::Modifiers::NONE
        }]);
        assert!(!manager.price_drafts[key].dirty());
        assert_eq!(manager.price_drafts[key].parse().unwrap(),Some(19000));
        assert!(!ctx.memory(|m|m.has_focus(editor)));
        assert!(requests.try_recv().is_err());
        assert!(manager.error.is_none());
        manager.price_focus_target=Some(key.into());
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Text("22000".into())]);
        assert_eq!(manager.price_drafts[key].parse().unwrap(),Some(22000));
        assert_eq!(manager.price_previous_focus,Some(editor));
        ctx.memory_mut(|m|m.surrender_focus(editor));
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        let request=next_price_request(&requests);
        assert_eq!(request.data["tier"],"semi");
        assert_eq!(request.data["expected"],19000);
        assert_eq!(request.data["price"],22000);
        request.reply.send(Ok(price_test_catalog(2,None,Some(22000)))).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.writing.is_none());
        assert_eq!(manager.products[0]["variants"][0]["semi"],22000);
        assert_eq!(manager.products[0]["variants"][0]["retail"],26000);
        assert!(requests.try_recv().is_err());
        assert!(!manager.has_pending_prices());
        assert!(manager.notice.is_none());
    }

    #[test]
    fn rendered_price_grid_escape_does_not_cancel_unfocused_dirty_cell(){
        let (mut manager,ctx,requests)=price_test_manager();
        let focused="p1:v1:semi";
        let other="p1:v1:retail";
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        manager.price_focus_target=Some(focused.into());
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        render_price_grid_frame(&mut manager,&ctx,vec![]);
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Text("21000".into())]);
        assert_eq!(manager.price_drafts[focused].parse().unwrap(),Some(21000));
        // An unrelated pending draft (for example, a failed earlier write)
        // must not be reset or committed by Escape in the focused cell.
        manager.price_drafts.get_mut(other).unwrap().input="27000".into();
        render_price_grid_frame(&mut manager,&ctx,vec![egui::Event::Key{
            key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,
            modifiers:egui::Modifiers::NONE
        }]);
        assert_eq!(manager.price_drafts[focused].parse().unwrap(),Some(19000));
        assert!(!manager.price_drafts[focused].dirty());
        assert_eq!(manager.price_drafts[other].parse().unwrap(),Some(27000));
        assert!(manager.price_drafts[other].dirty());
        assert!(requests.try_recv().is_err());
        assert!(manager.price_write_queue.is_empty());
    }

    #[test]
    fn price_cell_commit_contract_is_excel_style_without_action_popup(){
        let source=include_str!("product_manager.rs");
        // The live editor delegates Enter/Tab/arrows to the shared
        // focus-scoped worksheet navigator; inspect the current production
        // path rather than the retired local key-extraction implementation.
        let production=source.split("mod tests {").next().unwrap_or(source);
        let editor=production.split("let navigation_key=cell_navigation::").nth(1).unwrap()
            .split("let action_rect=Rect::from_center_size(").next().unwrap_or("");
        assert!(editor.contains("consume_worksheet_navigation_before_editor("));
        assert!(editor.contains("remember_worksheet_editor("));
        let shared_navigation=include_str!("cell_navigation.rs");
        for key in ["Key::ArrowLeft","Key::ArrowRight","Key::ArrowUp",
            "Key::ArrowDown","Key::Tab","Key::Enter"] {
            assert!(shared_navigation.contains(key),"shared navigation missing {key}");
        }
        assert!(editor.contains("response.lost_focus()"));
        assert!(editor.contains("response.gained_focus()"));
        assert!(editor.contains("select_all_price_text("));
        assert!(editor.contains("crate::overlay::take_cell_escape("));
        assert!(editor.contains("entry.reset_to_original()"));
        let escape_branch=editor.split("if escape{").nth(1).unwrap()
            .split("}else if keyboard_commit").next().unwrap_or("");
        assert!(escape_branch.contains("response.surrender_focus()"));
        assert!(editor.contains("response.surrender_focus()"));
        assert!(editor.contains("response.request_focus()"));
        assert!(!editor.contains("Popup::"));
        assert!(!editor.contains("theme::action"));
        assert!(source.contains("self.price_write_queue.push_back("));
        assert!(source.contains("self.launch_next_price_write(ctx.clone())"));
    }
    #[test]
    fn focused_price_cell_selects_existing_amount_for_direct_replacement(){
        let ctx=Context::default();
        let mut value="15,000".to_owned();
        let mut id=None;
        let _=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let response=ui.add(egui::TextEdit::singleline(&mut value));
                id=Some(response.id);
                select_all_price_text(ui,&response,&value);
            });
        });
        let state=egui::TextEdit::load_state(&ctx,id.unwrap()).unwrap();
        let range=state.cursor.char_range().unwrap();
        assert_eq!(range.primary.index,value.chars().count());
        assert_eq!(range.secondary.index,0);
    }
    #[test]
    fn spreadsheet_price_commits_do_not_show_per_cell_success_popup(){
        let (mut manager,ctx,requests)=price_test_manager();
        let wholesale="p1:v1:wholesale";
        let semi="p1:v1:semi";
        manager.price_drafts.insert(wholesale.into(),PriceDraft{original:None,input:"15000".into()});
        manager.price_drafts.insert(semi.into(),PriceDraft{original:Some(19000),input:String::new()});
        submit_test_price(&mut manager,&ctx,wholesale);
        let first=next_price_request(&requests);
        submit_test_price(&mut manager,&ctx,semi);
        manager.price_mode_exit_requested=true;
        manager.finish_price_mode_exit_if_ready();
        assert!(manager.price_mode,"do not leave price mode while writes are pending");
        first.reply.send(Ok(price_test_catalog(2,Some(15000),Some(19000)))).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.catalog.as_ref()
            .and_then(|c|c["revision"].as_u64())==Some(2));
        let second=next_price_request(&requests);
        assert_eq!(second.revision,2);
        assert_eq!(second.data["price"],Value::Null);
        assert_eq!(second.data["expected"],19000);
        assert!(manager.price_mode);
        assert!(manager.price_mode_exit_requested);
        assert!(manager.notice.is_none());
        assert!(!manager.has_modal());
        assert!(!manager.price_drafts[wholesale].dirty());
        assert!(manager.price_drafts[semi].dirty());
        second.reply.send(Ok(price_test_catalog(3,Some(15000),None))).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.writing.is_none());
        assert_eq!(manager.catalog.as_ref().unwrap()["revision"],3);
        assert!(manager.notice.is_none());
        assert!(manager.error.is_none());
        assert!(!manager.has_modal());
        assert!(!manager.price_mode);
        assert!(!manager.price_mode_exit_requested);
        assert!(!manager.blocks_navigation());
        assert!(manager.price_drafts.is_empty());
        assert_eq!(manager.products[0]["variants"][0]["wholesale"],15000);
        assert_eq!(manager.products[0]["variants"][0]["semi"],Value::Null);
    }
    #[test]
    fn spreadsheet_navigation_keys_match_enter_tab_and_arrow_semantics(){
        assert_eq!(price_navigation_key(true,false,false,false,false,false,false),Some(CellNavigationKey::Down));
        assert_eq!(price_navigation_key(true,false,false,false,false,false,true),Some(CellNavigationKey::Up));
        assert_eq!(price_navigation_key(false,true,false,false,false,false,false),Some(CellNavigationKey::Next));
        assert_eq!(price_navigation_key(false,true,false,false,false,false,true),Some(CellNavigationKey::Previous));
        assert_eq!(price_navigation_key(false,false,true,false,false,false,false),Some(CellNavigationKey::Left));
        assert_eq!(price_navigation_key(false,false,false,true,false,false,false),Some(CellNavigationKey::Right));
        assert_eq!(price_navigation_key(false,false,false,false,true,false,false),Some(CellNavigationKey::Up));
        assert_eq!(price_navigation_key(false,false,false,false,false,true,false),Some(CellNavigationKey::Down));
    }
    #[test]
    fn price_grid_keyboard_navigation_uses_explicit_rows_and_columns(){
        let order=vec![
            CellNavigationCell{id:"p1:w".into(),row:0,column:0,order:0,enabled:true},
            CellNavigationCell{id:"p1:s".into(),row:0,column:1,order:1,enabled:true},
            CellNavigationCell{id:"p1:r".into(),row:0,column:2,order:2,enabled:true},
            CellNavigationCell{id:"p2:w".into(),row:1,column:0,order:3,enabled:true},
            CellNavigationCell{id:"p2:s".into(),row:1,column:1,order:4,enabled:true},
            CellNavigationCell{id:"p2:r".into(),row:1,column:2,order:5,enabled:true},
        ];
        assert_eq!(price_navigation_target(&order,"p1:w",CellNavigationKey::Right).as_deref(),Some("p1:s"));
        assert_eq!(price_navigation_target(&order,"p1:r",CellNavigationKey::Left).as_deref(),Some("p1:s"));
        assert_eq!(price_navigation_target(&order,"p1:s",CellNavigationKey::Down).as_deref(),Some("p2:s"));
        assert_eq!(price_navigation_target(&order,"p2:s",CellNavigationKey::Up).as_deref(),Some("p1:s"));
        assert_eq!(price_navigation_target(&order,"p1:r",CellNavigationKey::Right),None);
        assert_eq!(price_navigation_target(&order,"p1:w",CellNavigationKey::Left),None);
    }
    #[test]
    fn excel_price_write_payload_commits_values_and_empty_as_null(){
        let new=PriceDraft{original:None,input:"15,000".into()};
        let write=QueuedPriceWrite::from_draft("p1:v1:wholesale",&new).unwrap();
        assert_eq!(write.data["price"],json!(15000));
        assert_eq!(write.data["expected"],Value::Null);
        let clear=PriceDraft{original:Some(19000),input:String::new()};
        let write=QueuedPriceWrite::from_draft("p1:v1:semi",&clear).unwrap();
        assert_eq!(write.data["price"],Value::Null);
        assert_eq!(write.data["expected"],json!(19000));
    }
    #[test]
    fn price_write_queue_preserves_order_and_cell_identity(){
        let (mut manager,ctx,requests)=price_test_manager();
        let wholesale="p1:v1:wholesale";
        let semi="p1:v1:semi";
        manager.price_drafts.insert(wholesale.into(),PriceDraft{original:None,input:"15000".into()});
        manager.price_drafts.insert(semi.into(),PriceDraft{original:Some(19000),input:"21000".into()});
        submit_test_price(&mut manager,&ctx,wholesale);
        let first=next_price_request(&requests);
        assert_eq!(first.data["tier"],"wholesale");
        assert_eq!(first.data["price"],15000);
        submit_test_price(&mut manager,&ctx,wholesale);
        submit_test_price(&mut manager,&ctx,semi);
        submit_test_price(&mut manager,&ctx,semi);
        assert_eq!(manager.price_write_queue.len(),1);
        assert!(requests.try_recv().is_err(),"queued writes must remain serialized");
        first.reply.send(Ok(price_test_catalog(2,Some(15000),Some(19000)))).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.catalog.as_ref()
            .and_then(|c|c["revision"].as_u64())==Some(2));
        let second=next_price_request(&requests);
        assert_eq!(second.revision,2);
        assert_eq!(second.data["productId"],"p1");
        assert_eq!(second.data["variantId"],"v1");
        assert_eq!(second.data["tier"],"semi");
        assert_eq!(second.data["expected"],19000);
        assert_eq!(second.data["price"],21000);
        second.reply.send(Ok(price_test_catalog(3,Some(15000),Some(21000)))).unwrap();
        poll_price_until(&mut manager,&ctx,|m|m.writing.is_none());
        assert!(manager.price_write_queue.is_empty());
        assert!(requests.try_recv().is_err(),"each cell must commit exactly once");
        assert_eq!(manager.products[0]["variants"][0]["wholesale"],15000);
        assert_eq!(manager.products[0]["variants"][0]["semi"],21000);
        assert_eq!(manager.products[0]["variants"][0]["retail"],26000);
        assert!(!manager.has_pending_prices());
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
    fn price_editor_focus_outline_is_inside_column_paint_clip(){
        let cell=Rect::from_min_size(
            egui::pos2(100.0,60.0),
            Vec2::new(theme::PRODUCT_TABLE_PRICE_WIDTH,
                theme::PRODUCT_TABLE_ROW_HEIGHT)
        );
        let viewport=Rect::from_min_max(
            egui::pos2(0.0,0.0),egui::pos2(900.0,900.0)
        );
        let (editor,clip)=product_price_editor_rects(cell,viewport);
        assert_eq!(editor.width(),components::FieldKind::Money.preferred_width());
        assert_eq!(editor.left()-cell.left(),2.0);
        assert_eq!(cell.right()-editor.right(),2.0);
        // Even a 1pt anti-aliased focused border outside the widget's
        // response Rect must remain inside the COLUMN clip on both sides.
        assert!(clip.left()<=editor.left()-1.0);
        assert!(clip.right()>=editor.right()+1.0);
        assert_eq!(clip,cell);
        let partly_scrolled=Rect::from_min_max(
            egui::pos2(105.0,0.0),egui::pos2(900.0,900.0)
        );
        let (_,scrolled_clip)=product_price_editor_rects(cell,partly_scrolled);
        assert_eq!(scrolled_clip.left(),105.0);
        assert_eq!(scrolled_clip.right(),cell.right());
    }
    #[test]
    fn rendered_price_grid_keeps_won_and_header_anchors(){
        let (mut manager,ctx,requests)=price_test_manager();
        // Unique nonempty amounts make all three rendered cells identifiable.
        manager.products[0]["variants"][0]["wholesale"]=json!(15000);
        manager.catalog.as_mut().unwrap()["products"][0]["variants"][0]["wholesale"]=json!(15000);
        manager.price_mode=false;
        let browse=product_page_text(&mut manager,&ctx);
        manager.price_mode=true;
        let edit=product_page_text(&mut manager,&ctx);
        let find=|text:&[(String,Rect)],label:&str|->Rect{
            text.iter().find(|(s,_)|s==label).map(|(_,rect)|*rect)
                .unwrap_or_else(||panic!("missing rendered product text {label}"))
        };
        let mut suffixes=edit.iter().filter(|(s,_)|s=="원")
            .map(|(_,rect)|*rect).collect::<Vec<_>>();
        suffixes.sort_by(|a,b|a.left().total_cmp(&b.left()));
        assert_eq!(suffixes.len(),3,"one non-editable won unit per price cell");
        for (i,(header,amount)) in [("도매가","15,000"),("반도매가","19,000"),("소매가","26,000")]
            .into_iter().enumerate(){
            let normal=find(&browse,&format!("{amount}원"));
            let number=find(&edit,amount);
            let unit=suffixes[i];
            let before_header=find(&browse,header);
            let after_header=find(&edit,header);
            assert!((before_header.min-after_header.min).length()<=1.0,
                "column header {header} shifted: {before_header:?} -> {after_header:?}");
            assert!((normal.right()-unit.right()).abs()<=1.0,
                "won right anchor changed for {header}: {normal:?} -> {unit:?}");
            assert!((normal.center().y-unit.center().y).abs()<=1.0,
                "won vertical alignment changed for {header}");
            // The TextEdit margin rounds the reserved suffix width to a whole
            // point. Allow its rounding and the glyph-layout pixel rounding.
            assert!(number.right()<=unit.left()+1.0,
                "number overlaps its non-editable won suffix: {number:?} {unit:?}");
            assert!((number.center().y-unit.center().y).abs()<=1.0,
                "number and suffix are vertically misaligned for {header}");
        }
        for tier in ["wholesale","semi","retail"]{
            let draft=&manager.price_drafts[&format!("p1:v1:{tier}")];
            assert!(!draft.input.contains('원'));
            assert!(!draft.dirty());
        }
        assert!(requests.try_recv().is_err(),"switching display mode must not write");
    }

    // Use the real list filter or the real product-detail editor. Locate
    // controls from rendered text and deliver native egui pointer/key events.
    fn brand_test_frame(manager:&mut ProductManager,ctx:&Context,detail:bool,
        events:Vec<egui::Event>)->Vec<(String,Rect)>{
        let work=Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(1760.0,1000.0));
        let time=ctx.input(|i|i.time)+0.1;
        let output=ctx.run(egui::RawInput{
            screen_rect:Some(work),time:Some(time),predicted_dt:0.1,
            events,..Default::default()
        },|ctx|{
            if detail{manager.show_detail(ctx,work);}
            else{egui::CentralPanel::default().show(ctx,|ui|manager.show_page(ui));}
        });
        let mut text=Vec::new();
        for shape in &output.shapes{collect_product_text(&shape.shape,&mut text);}
        text
    }
    fn brand_test_click(manager:&mut ProductManager,ctx:&Context,detail:bool,pos:egui::Pos2)
        ->Vec<(String,Rect)>{
        brand_test_frame(manager,ctx,detail,vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true,false]{
            brand_test_frame(manager,ctx,detail,vec![egui::Event::PointerButton{
                pos,button:egui::PointerButton::Primary,pressed,
                modifiers:egui::Modifiers::NONE
            }]);
        }
        brand_test_frame(manager,ctx,detail,vec![])
    }
    fn brand_test_label(text:&[(String,Rect)],label:&str)->egui::Pos2{
        text.iter().rev().find(|(s,_)|s==label)
            .unwrap_or_else(||panic!("missing rendered brand control {label}")).1.center()
    }
    #[test]
    fn rendered_brand_search_selects_registered_id_in_list_and_detail(){
        for detail in [false,true]{
            for query in ["k&m","케이앤엠","독일브랜드","K AND M"]{
                let (mut manager,ctx,requests)=price_test_manager();
                manager.price_mode=false;
                manager.catalog.as_mut().unwrap()["brands"]=json!([
                    {"id":"brand-target","name":"K&M","korean":"케이앤엠",
                        "aliases":["독일브랜드","K and M"]},
                    {"id":"brand-other","name":"Other Brand","korean":"다른 브랜드","aliases":[]}
                ]);
                if detail{manager.begin_edit(manager.products[0].clone());}
                let mut text=Vec::new();
                for _ in 0..6{text=brand_test_frame(&mut manager,&ctx,detail,vec![]);}
                let trigger=brand_test_label(&text,if detail{"브랜드 미지정"}else{"전체 브랜드"});
                text=brand_test_click(&mut manager,&ctx,detail,trigger);
                assert!(text.iter().any(|(s,_)|s=="K&M"),"brand popup did not open");
                let search=brand_test_label(&text,"브랜드 검색");
                brand_test_click(&mut manager,&ctx,detail,search);
                text=brand_test_frame(&mut manager,&ctx,detail,
                    vec![egui::Event::Text(query.into())]);
                assert!(text.iter().any(|(s,_)|s=="K&M"),"query {query} missed its brand");
                assert!(!text.iter().any(|(s,_)|s=="Other Brand"),
                    "query {query} did not filter an unrelated option");
                if detail{
                    assert!(field(manager.selected.as_ref().unwrap(),"brandId").is_empty());
                }else{assert!(manager.brand.is_empty());}
                let option=brand_test_label(&text,"K&M");
                brand_test_click(&mut manager,&ctx,detail,option);
                if detail{
                    assert_eq!(manager.selected.as_ref().unwrap()["brandId"],"brand-target");
                    assert_eq!(manager.selected.as_ref().unwrap()["variants"],
                        manager.original_selected.as_ref().unwrap()["variants"]);
                    assert!(field(manager.original_selected.as_ref().unwrap(),"brandId").is_empty());
                }else{assert_eq!(manager.brand,"brand-target");}
                assert!(requests.try_recv().is_err(),"brand search/selection must not save a product");
            }
        }
    }

    #[test]
    fn price_editor_won_suffix_shares_read_only_right_alignment(){
        let cell=Rect::from_min_size(
            egui::pos2(10.0,10.0),
            Vec2::new(theme::PRODUCT_TABLE_PRICE_WIDTH,
                theme::PRODUCT_TABLE_ROW_HEIGHT)
        );
        let (editor,_) =product_price_editor_rects(
            cell,Rect::from_min_size(egui::Pos2::ZERO,Vec2::splat(600.0))
        );
        let display_right=cell.right()-theme::SPACE_8;
        let suffix_inset=theme::SPACE_8-theme::SPACE_4/2.0;
        let edit_suffix_right=editor.right()-suffix_inset;
        assert!((display_right-edit_suffix_right).abs()<0.01,
            "normal and edit mode won units must share the exact x coordinate");
        assert_eq!(editor.center().y,cell.center().y,
            "editable numbers and read-only amounts must share vertical center");
        assert_eq!(typography::numeric_editor_font(),
            typography::management_table_money_font());
        let source=include_str!("product_manager.rs");
        let table=source.split("let selected_variant=variants.iter().find(")
            .nth(1).unwrap().split("let action_rect=Rect::from_center_size(")
            .next().unwrap();
        assert!(table.contains("components::money_field_with_unit("));
        assert!(table.contains("true,\"원\","));
        assert!(!table.contains("components::money_field(\n"));
    }

    #[test]
    fn price_edit_mode_uses_exact_browse_column_positions_and_minimum_width(){
        // Price edit must never add 148pt per price column, as the former
        // inline Save/Cancel arrangement did (444pt across three columns).
        let base=product_table_base_widths();
        assert_eq!(base[2],theme::PRODUCT_TABLE_PRICE_WIDTH);
        assert_eq!(base[3],base[2]);
        assert_eq!(base[4],base[2]);
        assert_eq!(base[5],theme::PRODUCT_TABLE_ACTION_WIDTH);
        let min_width=theme::PRODUCT_TABLE_MIN_WIDTH.max(
            base.iter().sum::<f32>()+5.0*theme::SPACE_8
        );
        for viewport in [900.0_f32,1100.0,1300.0,1760.0]{
            let data_width=viewport.max(min_width);
            let browse=product_table_column_widths(data_width);
            let edit=product_table_column_widths(data_width);
            assert_eq!(browse,edit);
            let browse_rects=components::management_table_cells(
                egui::pos2(0.0,0.0),&browse,theme::MANAGEMENT_TABLE_HEADER_HEIGHT,theme::SPACE_8);
            let edit_rects=components::management_table_cells(
                egui::pos2(0.0,0.0),&edit,theme::PRODUCT_TABLE_ROW_HEIGHT,theme::SPACE_8);
            for i in 0..6{
                assert_eq!(browse_rects[i].left(),edit_rects[i].left());
                assert_eq!(browse_rects[i].right(),edit_rects[i].right());
            }
        }
        let source=include_str!("product_manager.rs");
        let table=source.split("let base=product_table_base_widths();")
            .nth(1).unwrap().split("let headings=[").next().unwrap();
        assert!(!table.contains("self.price_mode"));
        let production=source.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("self.excel.show_menu("));
        assert!(!production.contains("egui::Popup::"));
        let excel=include_str!("product_excel_ui.rs");
        assert!(excel.contains("crate::overlay::show_menu(trigger,"));
        assert!(!table.contains("PRODUCT_TABLE_EDIT_PRICE_WIDTH"));
    }
    #[test]
    fn fixed_product_table_columns_share_identical_header_and_row_geometry(){
        let widths=[340.0,182.0,116.0,116.0,116.0,100.0];
        let start=egui::pos2(20.0,30.0);
        let header=components::management_table_cells(start,&widths,theme::MANAGEMENT_TABLE_HEADER_HEIGHT,theme::SPACE_8);
        let row=components::management_table_cells(
            egui::pos2(start.x,start.y+theme::MANAGEMENT_TABLE_HEADER_HEIGHT),
            &widths,theme::PRODUCT_TABLE_ROW_HEIGHT,theme::SPACE_8);
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

#[cfg(test)]
mod reception_selection_tests{
    use super::selectable_order_product;
    use serde_json::json;
    #[test]
    fn picker_accepts_only_active_independent_catalog_sale_units(){
        let independent=json!({"id":"p","active":true,"variants":[{"id":"v"}]});
        assert!(selectable_order_product(&independent));
        let mut archived=independent.clone();
        archived["active"]=json!(false);
        assert!(!selectable_order_product(&archived));
        let mut undecided=independent.clone();
        undecided["requiresOption"]=json!(true);
        assert!(!selectable_order_product(&undecided));
        assert!(!selectable_order_product(&json!({"id":"p","variants":[]})));
        assert!(!selectable_order_product(&json!({"id":"p","variants":[{"id":"v1"},{"id":"v2"}]})));
    }
}

#[cfg(test)]
impl ProductManager {
    pub(crate) fn geometry_fixture(gateway:Arc<dyn BandoGateway>,ctx:egui::Context,tab:usize,loaded:bool)->Self{
        let mut page=Self::new(gateway,ctx);
        page.result=LatestJob::default(); page.next_query=None; page.tab=tab; if loaded{page.catalog=Some(serde_json::json!({"revision":1,"products":[],"categories":[],"brands":[],"aliases":[],"settings":{}}));}
        page
    }

    pub(crate) fn geometry_begin_read(&mut self)->Box<dyn FnOnce(Result<usize,String>)>{
        self.error=None;
        let finish=self.result.geometry_begin_read();
        Box::new(move |answer|finish(answer.map(|count|{
            let products=(0..count).map(|i|json!({"id":format!("geometry-p-{i}"),"name":format!("검증 상품 {i:03}"),"subtitle":"긴 이름과 옵션 검증","active":true,"variants":[{"id":format!("geometry-v-{i}"),"color":"Black","prices":{"wholesale":10000,"semi":12000,"retail":15000}}]})).collect::<Vec<_>>();
            let categories=(0..count).map(|i|json!({"id":format!("geometry-cat-{i}"),"name":format!("검증 분류 {i}"),"parentId":"catalog-root:BANDOSTAND"})).collect::<Vec<_>>();
            let brands=(0..count).map(|i|json!({"id":format!("geometry-brand-{i}"),"name":format!("검증 브랜드 {i}"),"korean":"합성","aliases":["검증 검색어"]})).collect::<Vec<_>>();
            let aliases=(0..count).map(|i|json!({"id":format!("geometry-alias-{i}"),"text":format!("검증 별칭 {i}"),"productId":format!("geometry-p-{i}")})).collect::<Vec<_>>();
            let groups=(0..count).map(|i|(format!("geometry-model-{i}"),json!({"name":format!("검증 모델군 {i}"),"productIds":[format!("geometry-p-{i}")],"representativeProductId":format!("geometry-p-{i}")}))).collect::<serde_json::Map<String,Value>>();
            ManagementProductRows{catalog:json!({"revision":1,"products":products.clone(),"categories":categories,"brands":brands,"aliases":aliases,"settings":{"colorGuidanceGroups":groups}}),products}
        })))
    }
    pub(crate) fn geometry_read_failed(&self)->bool{self.error.is_some()}
}
