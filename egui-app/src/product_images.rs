//! Read-only product-photo projection shared by catalog and reception views.
//! React sources: src/tauri-bridge.ts::safeImage and ProductManager/OrderItemEditor.
//! Image media is presentation only; catalog mutation and validation stay in Rust.
//! Never fetch arbitrary product URLs or infer an image from the model name.

use std::{collections::HashMap,fs,path::{Path,PathBuf}};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use eframe::egui::{self,Color32,Rect,TextureHandle,TextureOptions,Ui,Vec2};
use serde_json::Value;
use crate::{theme,typography};

const BUNDLED: &str = include_str!("../../src/tauri-product-images.json");
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_777_216;

fn text<'a>(product:&'a Value,key:&str)->&'a str{
    product.get(key).and_then(Value::as_str).unwrap_or("")
}
fn embedded_data(value:&str)->Option<Vec<u8>>{
    let (mime,payload)=value.split_once(";base64,")?;
    if !matches!(mime,"data:image/jpeg"|"data:image/png"|"data:image/webp"){
        return None;
    }
    if payload.len()>MAX_IMAGE_BYTES*4/3+8 {return None;}
    let bytes=STANDARD.decode(payload).ok()?;
    (bytes.len()<=MAX_IMAGE_BYTES).then_some(bytes)
}
fn historical_filename(product:&Value)->Option<String>{
    let map:Value=serde_json::from_str(BUNDLED).ok()?;
    let source=product.get("siteId").filter(|v|!v.is_null())
        .or_else(||product.get("sourceProductId"))?;
    let id=source.as_str().map(str::to_owned)
        .or_else(||source.as_u64().map(|n|n.to_string()))?;
    let filename=map.get(id.as_str())?.as_str()?;
    // The file names come from our fixed, versioned React bundle map.
    // Restrict them even if future catalog values are untrusted.
    let path=Path::new(filename);
    if path.components().count()!=1 ||
       !filename.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'_'||b==b'-') {
        return None;
    }
    Some(filename.to_owned())
}
fn product_image_paths(filename:&str)->Vec<PathBuf>{
    let mut paths=Vec::new();
    if let Ok(cwd)=std::env::current_dir(){
        for ancestor in cwd.ancestors().take(7){
            paths.push(ancestor.join("public/product-images").join(filename));
            paths.push(ancestor.join("product-images").join(filename));
        }
    }
    if let Ok(exe)=std::env::current_exe(){
        for ancestor in exe.ancestors().skip(1).take(7){
            paths.push(ancestor.join("public/product-images").join(filename));
            paths.push(ancestor.join("product-images").join(filename));
        }
    }
    paths
}
fn bytes_for_product(product:&Value)->Option<Vec<u8>>{
    if text(product,"imageOverride")=="HIDDEN" {return None;}
    let manual=||embedded_data(text(product,"imageData"));
    if text(product,"imageOverride")=="MANUAL" {return manual();}
    if let Some(filename)=historical_filename(product) {
        for path in product_image_paths(&filename){
            if let Ok(metadata)=fs::metadata(&path) {
                if metadata.is_file() && metadata.len()<=MAX_IMAGE_BYTES as u64 {
                    if let Ok(bytes)=fs::read(&path){return Some(bytes);}
                }
            }
        }
    }
    manual()
}
fn cache_identity(product:&Value)->String{
    use std::hash::{Hash,Hasher};
    let mut hasher=std::collections::hash_map::DefaultHasher::new();
    for key in ["id","imageOverride","imageData","siteId","sourceProductId"] {
        // Uploaded data URIs can contain 1.5M characters. Hash the borrowed
        // source directly instead of allocating an equally large JSON string
        // on every egui repaint.
        if let Some(text)=product.get(key).and_then(Value::as_str) {
            text.hash(&mut hasher);
        }else{
            product.get(key).map(Value::to_string).hash(&mut hasher);
        }
    }
    format!("product-photo:{:016x}",hasher.finish())
}
fn decode(bytes:&[u8])->Option<egui::ColorImage>{
    let image=image::load_from_memory(bytes).ok()?;
    let (width,height)=(image.width(),image.height());
    if width==0 || height==0 || (width as u64)*(height as u64)>MAX_PIXELS {
        return None;
    }
    let rgba=image.into_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize,height as usize],rgba.as_raw()
    ))
}

/// React ProductManager image registration parity. The picker never writes
/// to SQLite directly: it returns only staged imageData for the existing
/// product-upsert CAS boundary, after bounded validation/optimization.
pub fn choose_and_optimize_photo()->Result<Option<String>,String>{
    #[cfg(not(windows))]
    {
        Err("상품 이미지 선택은 현재 Windows egui에서 사용할 수 있습니다.".into())
    }
    #[cfg(windows)]
    {
        let path=rfd::FileDialog::new()
            .add_filter("상품 이미지",&["jpg","jpeg","png","webp"])
            .pick_file();
        let Some(path)=path else{return Ok(None)};
        let metadata=fs::metadata(&path).map_err(|err|
            format!("선택한 이미지 정보를 읽을 수 없습니다: {err}")
        )?;
        if metadata.len()==0 || metadata.len()>5*1024*1024{
            return Err("이미지는 JPG, PNG, WEBP 형식이며 5MB 이하여야 합니다.".into());
        }
        let ext=path.extension().and_then(|x|x.to_str())
            .unwrap_or("").to_ascii_lowercase();
        if !matches!(ext.as_str(),"jpg"|"jpeg"|"png"|"webp"){
            return Err("이미지는 JPG, PNG, WEBP 형식이어야 합니다.".into());
        }
        let bytes=fs::read(&path).map_err(|err|
            format!("상품 이미지를 읽을 수 없습니다: {err}")
        )?;
        let image=image::load_from_memory(&bytes).map_err(|_|
            "이미지를 열 수 없습니다. 파일 형식을 확인하세요.".to_owned()
        )?;
        if image.width()==0 || image.height()==0 ||
            (image.width() as u64)*(image.height() as u64)>30_000_000{
            return Err("해상도가 너무 높은 이미지입니다. 크기를 줄인 뒤 다시 등록하세요.".into());
        }
        // Use the same descending dimension limits as React. WebP is
        // encoded locally, including alpha; no external image host is used.
        for edge in [1024,840,700,560]{
            let scale=(edge as f64 / image.width().max(image.height()) as f64)
                .min(1.0);
            let width=((image.width() as f64*scale).round() as u32).max(1);
            let height=((image.height() as f64*scale).round() as u32).max(1);
            let scaled=image.resize_exact(
                width,height,image::imageops::FilterType::Lanczos3
            );
            let mut output=std::io::Cursor::new(Vec::new());
            if scaled.write_to(&mut output,image::ImageFormat::WebP).is_ok(){
                let encoded=format!(
                    "data:image/webp;base64,{}",STANDARD.encode(output.into_inner())
                );
                if encoded.len()<=1_500_000{return Ok(Some(encoded));}
            }
        }
        Err("압축 후에도 이미지가 큽니다. 다른 사진을 선택하세요.".into())
    }
}

#[derive(Default)]
pub struct ProductImageCache{
    textures:HashMap<String,Option<TextureHandle>>,
}
impl ProductImageCache {
    pub fn clear(&mut self){self.textures.clear();}
    pub fn paint(&mut self,ui:&mut Ui,product:&Value,rect:Rect){
        let palette=theme::surface_palette(theme::SurfaceRole::Input,theme::VisualState::Normal);
        ui.painter().rect_filled(rect,theme::CONTROL_RADIUS as f32,palette.fill);
        let key=cache_identity(product);
        if !self.textures.contains_key(&key){
            let texture=bytes_for_product(product).and_then(|bytes|decode(&bytes))
                .map(|image|ui.ctx().load_texture(
                    key.clone(),image,TextureOptions::LINEAR
                ));
            self.textures.insert(key.clone(),texture);
        }
        if let Some(Some(texture))=self.textures.get(&key){
            let native=texture.size_vec2();
            let scale=(rect.width()/native.x).min(rect.height()/native.y);
            let size=native*scale;
            let destination=Rect::from_center_size(rect.center(),size);
            ui.painter().image(texture.id(),destination,
                Rect::from_min_max(egui::pos2(0.0,0.0),egui::pos2(1.0,1.0)),
                theme::image_tint());
        }else{
            ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,
                "상품 사진 없음",typography::product_image_placeholder_font(),theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
        }
        // Draw the inside border last, including the missing-photo surface.
        theme::paint_surface_outline(ui.painter(),rect,theme::CONTROL_RADIUS,palette.border);
    }
    pub fn photo(&mut self,ui:&mut Ui,product:&Value,size:f32){
        let (rect,_)=ui.allocate_exact_size(Vec2::splat(size),egui::Sense::hover());
        self.paint(ui,product,rect);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_site_images_only_resolve_from_known_bundle_ids(){
        assert_eq!(historical_filename(&json!({"siteId":"100"})).as_deref(),
            Some("100.jpg"));
        assert_eq!(historical_filename(&json!({"sourceProductId":"310"})).as_deref(),
            Some("310.png"));
        assert!(historical_filename(&json!({"siteId":"709"})).is_none());
        assert!(historical_filename(&json!({"siteId":"../../foo"})).is_none());
    }
    #[test]
    fn bd709_seed_product_resolves_to_an_actual_bundled_photo(){
        // Regression for the screenshot: BD 709 black perforated top,
        // catalog siteId=48, has an existing versioned 48.jpg photo.
        let original:Value=serde_json::from_str(include_str!(
            "../../src-tauri/resources/state-seed.json"
        )).expect("valid catalog seed");
        let product=original["products"].as_array().unwrap().iter()
            .find(|p|text(p,"name")=="BD 709 / 블랙 / 타공 상판")
            .expect("registered BD 709");
        assert_eq!(text(product,"siteId"),"48");
        assert_eq!(historical_filename(product).as_deref(),Some("48.jpg"));
        let path=Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../public/product-images/48.jpg");
        let bytes=fs::read(path).expect("bundle image remains present");
        assert!(decode(&bytes).is_some(),"actual BD 709 image must be decodable");
    }

    #[test]
    fn react_image_override_semantics_are_preserved(){
        let bytes="data:image/png;base64,aGVsbG8=";
        let manual=json!({"imageOverride":"MANUAL","imageData":bytes});
        assert_eq!(bytes_for_product(&manual),Some(b"hello".to_vec()));
        let hidden=json!({"imageOverride":"HIDDEN","imageData":bytes,
            "siteId":"100"});
        assert!(bytes_for_product(&hidden).is_none());
        let automatic=json!({"imageData":bytes,"siteId":"missing"});
        assert_eq!(bytes_for_product(&automatic),Some(b"hello".to_vec()));
    }
    #[test]
    fn distinct_manual_images_never_reuse_the_previous_product_texture(){
        let a=json!({"id":"p","imageOverride":"MANUAL","imageData":"a"});
        let b=json!({"id":"p","imageOverride":"MANUAL","imageData":"b"});
        assert_ne!(cache_identity(&a),cache_identity(&b));
    }
}
