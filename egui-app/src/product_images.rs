//! Read-only product-photo projection shared by catalog and reception views.
//! React sources: src/tauri-bridge.ts::safeImage and ProductManager/OrderItemEditor.
//! Image media is presentation only; catalog mutation and validation stay in Rust.
//! Never fetch arbitrary product URLs or infer an image from the model name.

use std::{collections::HashMap,fs,path::{Path,PathBuf}};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use eframe::egui::{self,Color32,Context,Rect,TextureHandle,TextureOptions,Ui,Vec2};
use serde_json::Value;
use crate::theme;

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
    let id=product.get("siteId").or_else(||product.get("sourceProductId"))?
        .as_str().or_else(||product.get("siteId")
            .or_else(||product.get("sourceProductId"))
            .and_then(Value::as_u64).map(|_|""))?;
    let filename=map.get(id)?.as_str()?;
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
    for value in ["id","imageOverride","imageData","siteId","sourceProductId"] {
        product.get(value).map(Value::to_string).hash(&mut hasher);
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

#[derive(Default)]
pub struct ProductImageCache{
    textures:HashMap<String,Option<TextureHandle>>,
}
impl ProductImageCache {
    pub fn clear(&mut self){self.textures.clear();}
    pub fn paint(&mut self,ui:&mut Ui,product:&Value,rect:Rect){
        ui.painter().rect_filled(rect,theme::CONTROL_RADIUS as f32,theme::INPUT);
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
                Color32::WHITE);
        }else{
            ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,
                "상품 사진 없음",egui::FontId::proportional(14.0),theme::SECONDARY);
        }
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
