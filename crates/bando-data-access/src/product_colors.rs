//! One-time migration of the legacy "색상 미표기" catalog placeholder.
//! It is not a sellable option or a real color. Preserve all product,
//! variant, order, price and historical identifiers. Only current product
//! color fields are normalized; archived order and audit evidence is immutable.
use rusqlite::Connection;
use serde_json::{json,Value};
use std::path::Path;

const LEGACY:&str="색상 미표기";
const NO_COLOR:&str="";
fn placeholder(value:&Value)->bool {
    matches!(value.as_str().map(str::trim),Some("단일"|"색상 미표기"|"색상미표기"))
}

fn option_color(option:&str)->&'static str{
    let value=option.to_lowercase();
    // Only recognize color evidence explicitly present in the original
    // sale option. Size, perforation, packaging and model suffixes are not
    // colors and retain the internal no-color sentinel.
    if value.contains("다크 그린")||value.contains("dark green"){return "다크 그린";}
    if value.contains("와인")||value.contains("wine"){return "와인";}
    if value.contains("크롬")||value.contains("chrome"){return "Chrome";}
    if value.contains("블랙")||value.contains("검정")||value.contains("black"){return "Black";}
    if value.contains("실버")||value.contains("silver"){return "Silver";}
    if value.contains("그레이")||value.contains("gray")||value.contains("grey"){return "Gray";}
    if value.contains("화이트")||value.contains("white"){return "White";}
    if value.contains("브라운")||value.contains("brown"){return "Brown";}
    if value.contains("클리어")||value.contains("clear"){return "Clear";}
    if value.contains("프로스트")||value.contains("frost"){return "Frost";}
    NO_COLOR
}
fn normalize_product(product:&mut Value)->usize{
    let default_legacy=placeholder(&product["defaultColor"]);
    let mut first_fixed=None::<String>;
    let mut changed=0;
    if let Some(variants)=product.get_mut("variants").and_then(Value::as_array_mut){
        for variant in variants{
            if !placeholder(&variant["color"]){continue;}
            let fixed=if variant["color"].as_str()==Some(LEGACY) {
                option_color(variant["option"].as_str().unwrap_or(""))
            } else { NO_COLOR };
            variant["color"]=json!(fixed);
            if first_fixed.is_none(){first_fixed=Some(fixed.to_owned());}
            changed+=1;
        }
    }
    if default_legacy{
        // The former default always referenced the placeholder itself.
        // Prefer its just-normalized variant, never another variant's
        // arbitrary color. An absent color is the empty string.
        let replacement=first_fixed.or_else(||{
            product["variants"].as_array()
                .and_then(|vs|vs.first())
                .and_then(|v|v["color"].as_str())
                .map(str::to_owned)
        }).unwrap_or_else(||NO_COLOR.to_owned());
        product["defaultColor"]=json!(replacement);
        changed+=1;
    }
    changed
}
/// Read-only guard for a transaction whose core tables must remain unchanged.
pub(crate) fn needs_normalization(state:&Value)->Result<bool,String>{
    let products=state["products"].as_array().ok_or("상품 색상 정리 대상 목록이 없습니다.")?;
    Ok(products.iter().any(|p|placeholder(&p["defaultColor"]) ||
        p["variants"].as_array().is_some_and(|vs|
            vs.iter().any(|v|placeholder(&v["color"])))))
}

pub fn normalize_products(state:&mut Value)->Result<usize,String>{
    let products=state.get_mut("products").and_then(Value::as_array_mut)
        .ok_or("상품 색상 정리 대상 목록이 없습니다.")?;
    Ok(products.iter_mut().map(normalize_product).sum())
}
fn migrate_inner(conn:&mut Connection,backup_base:Option<&Path>)->Result<usize,String>{
    let version:i64=conn.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(|e|e.to_string())?;
    if !crate::supported_relational_version(version){return Err("지원하지 않는 운영 DB 버전입니다.".into());}
    let mut state=crate::read_v2_snapshot(conn)?;
    let revision=state["revision"].as_u64().ok_or("상품 색상 정리 DB revision 오류")?;
    let changed=normalize_products(&mut state)?;
    if changed==0{return Ok(0);}
    // Full verified before-image, transaction/CAS and relational index update
    // use the existing shared business writer. No orders are modified.
    if let Some(base)=backup_base{
        crate::backup_service::manual_backup(conn,base)?;
    }
    crate::write_v2_snapshot(conn,state,revision,false)?;
    Ok(changed)
}
pub fn migrate_v2(conn:&mut Connection,base:&Path)->Result<usize,String>{
    migrate_inner(conn,Some(base))
}
pub(crate) fn migrate_v2_with_verified_restore_backup(conn:&mut Connection)
    ->Result<usize,String>{
    migrate_inner(conn,None)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn legacy_placeholder_is_removed_without_changing_options_ids_or_prices(){
        let mut state=json!({"products":[
            {"id":"p1","defaultColor":LEGACY,"variants":[
                {"id":"v1","color":LEGACY,"option":"크롬 25cm","retail":40000}]},
            {"id":"p2","defaultColor":"단일","variants":[
                {"id":"v2","color":"단일","option":"상판 42x34cm","retail":12000}]},
            {"id":"p3","defaultColor":"Black","variants":[
                {"id":"v3","color":"Black","option":"블랙","retail":10000}]}
        ],"orders":[{"id":"existing","snapshot":{"color":LEGACY}}]});
        let old_orders=state["orders"].clone();
        assert_eq!(normalize_products(&mut state).unwrap(),4);
        assert_eq!(state["products"][0]["defaultColor"],"Chrome");
        assert_eq!(state["products"][0]["variants"][0]["color"],"Chrome");
        assert_eq!(state["products"][1]["defaultColor"],NO_COLOR);
        assert_eq!(state["products"][1]["variants"][0]["color"],NO_COLOR);
        assert_eq!(state["products"][0]["variants"][0]["id"],"v1");
        assert_eq!(state["products"][0]["variants"][0]["retail"],40000);
        assert_eq!(state["products"][1]["variants"][0]["option"],"상판 42x34cm");
        assert_eq!(state["products"][2]["defaultColor"],"Black");
        assert_eq!(state["orders"],old_orders);
        assert_eq!(normalize_products(&mut state).unwrap(),0);
    }
}