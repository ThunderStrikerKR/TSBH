//! React ProductManager 상품 목록: Rust-owned catalog selection and a
//! read-only price/detail projection. Product and price writes must not target
//! the isolated egui preview DB or bypass Tauri's operational save boundary.
use crate::{
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

pub struct ProductManager {
    gateway:Arc<dyn BandoGateway>,
    catalog:Option<Value>,
    products:Vec<Value>,
    query:String,
    category:String,
    brand:String,
    missing:bool,
    selected:Option<Value>,
    result:Option<Receiver<SearchReply>>,
    generation:u64,
    next_query:Option<Instant>,
    error:Option<String>,
}

impl ProductManager {
    pub fn new(gateway:Arc<dyn BandoGateway>,ctx:Context)->Self {
        let mut page=Self {
            gateway,catalog:None,products:Vec::new(),
            query:String::new(),category:String::new(),
            brand:String::new(),missing:false,selected:None,
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
    fn changed(&mut self,ctx:&Context) {
        self.next_query=Some(Instant::now()+Duration::from_millis(150));
        ctx.request_repaint_after(Duration::from_millis(160));
    }
    fn poll(&mut self,ctx:&Context) {
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
    pub fn has_modal(&self)->bool {self.selected.is_some()}
    pub fn close_detail(&mut self){self.selected=None;}

    pub fn show_page(&mut self,ui:&mut Ui){
        self.poll(ui.ctx());
        ui.label(typography::page_heading("상품 관리"));
        ui.add_space(theme::SPACE_12);
        components::management_tabs(ui,&[
            ("상품 목록",true,true),
            ("카테고리 관리",false,false),
            ("모델군 관리",false,false),
            ("별칭 관리",false,false),
            ("브랜드 관리",false,false),
        ]);
        ui.add_space(theme::SPACE_12);
        theme::section_frame().show(ui,|panel|{
            panel.horizontal(|header|{
                header.label(typography::section_heading("상품 가격표"));
                header.label(typography::caption(&format!(
                    "{}개 상품",self.products.len()
                )));
                header.with_layout(Layout::right_to_left(Align::Center),|actions|{
                    theme::action(actions,"상품 추가",theme::ButtonRole::Primary,false)
                        .on_disabled_hover_text("상품 저장 경로가 이전되지 않았습니다.");
                    theme::action(actions,"가격 수정 모드 켜기",
                        theme::ButtonRole::Standard,false)
                        .on_disabled_hover_text("가격 변경 저장 경로가 이전되지 않았습니다.");
                    theme::action(actions,"Excel 관리",theme::ButtonRole::Standard,false)
                        .on_disabled_hover_text("Excel 관리 기능이 이전되지 않았습니다.");
                });
            });
            components::soft_divider(panel);
            panel.horizontal_wrapped(|filter|{
                filter.label(typography::field_label("상품 검색"));
                if components::flexible_text_field(
                    filter,&mut self.query,theme::MANAGEMENT_SEARCH_MIN_WIDTH,
                    "모델명 또는 상품명 검색"
                ).changed(){
                    self.changed(filter.ctx());
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
                if self.category!=before_category {self.changed(filters.ctx());}
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
                if self.brand!=before_brand {self.changed(filters.ctx());}
                if filters.checkbox(&mut self.missing,"가격 미등록만").changed(){
                    self.changed(filters.ctx());
                }
            });
            components::soft_divider(panel);
            if let Some(error)=&self.error {
                panel.label(typography::status(error,theme::DANGER));
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
                                        grid.label(typography::quantity(
                                            &price(&variant[tier])
                                        ));
                                    }
                                    if theme::action(
                                        grid,"상세 관리",theme::ButtonRole::Standard,true
                                    ).clicked(){
                                        self.selected=Some(product.clone());
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
    }
    pub fn show_detail(&mut self,ctx:&Context,work_area:egui::Rect){
        let Some(product)=self.selected.clone()else{return};
        let mut close=false;
        components::modal_backdrop(ctx,"egui-product-detail-backdrop",false);
        components::modal_window(
            "상품 상세 관리","egui-product-detail",work_area
        ).resizable(false)
            .default_width(theme::MODAL_DEFAULT_MAX_WIDTH.min(work_area.width()))
            .show(ctx,|modal|{
                egui::ScrollArea::vertical()
                    .id_salt("egui-product-detail-content")
                    .max_height(theme::MANAGEMENT_MODAL_BODY_MAX_HEIGHT)
                    .show(modal,|body|{
                        body.label(typography::section_heading("상품 정보"));
                        for (label,key) in [
                            ("상품명","name"),("모델명","code"),
                            ("상품설명","subtitle"),("카테고리","category"),
                            ("판매 구분","catalog"),("기본 색상","defaultColor"),
                            ("발송지","origin"),
                        ] {
                            let value=field(&product,key);
                            body.horizontal_wrapped(|row|{
                                row.label(typography::field_label(label));
                                row.label(typography::body(
                                    if value.is_empty(){"—"}else{value}
                                ));
                            });
                        }
                        body.add_space(theme::SPACE_12);
                        body.label(typography::section_heading("등록된 판매 구성과 가격"));
                        for variant in rows(&product["variants"]) {
                            components::soft_divider(body);
                            let label=option_text(&product,variant);
                            body.label(typography::subsection_heading(
                                if label.is_empty(){"기본 구성"}else{&label}
                            ));
                            for (title,key) in [
                                ("도매가","wholesale"),("반도매가","semi"),
                                ("소매가","retail")
                            ]{
                                body.horizontal(|row|{
                                    row.label(typography::field_label(title));
                                    row.label(typography::quantity(&price(&variant[key])));
                                });
                            }
                            let url=field(variant,"url");
                            if !url.is_empty() {
                                body.horizontal_wrapped(|row|{
                                    row.label(typography::field_label("상품 연결"));
                                    row.label(typography::caption(url));
                                });
                            }
                        }
                    });
                modal.add_space(theme::SPACE_12);
                modal.horizontal(|actions|{
                    if theme::action(
                        actions,"닫기",theme::ButtonRole::Standard,true
                    ).clicked(){close=true;}
                });
            });
        if close {self.selected=None;}
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