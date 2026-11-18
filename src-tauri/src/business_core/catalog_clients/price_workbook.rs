//! Native workbook price review. TypeScript may parse XLSX cells and render
//! option labels, but only Rust matches identities, validates amounts and
//! constructs the immutable proposed price history.
use serde_json::{json, Value};
use std::collections::HashSet;
use super::{field,items};

const SAFE_JS_MAX: u64 = 9_007_199_254_740_991;
const TIERS: [&str; 3] = ["wholesale","semi","retail"];
const TIER_LABELS: [&str; 3] = ["도매가","반도매가","소매가"];

fn formatted(value: &Value) -> String {
    let Some(n) = value.as_u64() else { return "미등록".into() };
    let s = n.to_string();
    let mut out = String::new();
    for (i,ch) in s.chars().enumerate() {
        if i > 0 && (s.len()-i)%3 == 0 {out.push(',');}
        out.push(ch);
    }
    out
}

fn parse_price(raw: &str) -> Result<Value, String> {
    if raw == "삭제" {return Ok(Value::Null);}
    if raw.is_empty() {return Err("가격을 입력하세요.".into());}
    let ungrouped=raw.replace(',',"");
    let correct_groups = !raw.contains(',') ||
        raw.split(',').enumerate().all(|(i,part)|
            !part.is_empty() && part.bytes().all(|c|c.is_ascii_digit()) &&
            (if i==0{part.len()<=3}else{part.len()==3}));
    if !correct_groups || !ungrouped.bytes().all(|c|c.is_ascii_digit()) {
        return Err("가격은 정수 또는 삭제로 입력하세요.".into());
    }
    let n: u64 = ungrouped.parse()
        .map_err(|_|"가격은 0 이상의 안전한 정수여야 합니다.")?;
    if n>SAFE_JS_MAX {return Err("가격은 0 이상의 안전한 정수여야 합니다.".into());}
    if n==0 {Ok(Value::Null)}else{Ok(json!(n))}
}

pub(super) fn preview(payload: &Value) -> Result<Value,String> {
    let state=payload.get("state").ok_or("현재 상태가 없습니다.")?;
    let source=payload.get("source").ok_or("가격표 행 데이터가 없습니다.")?;
    let at=field(payload,"at");
    if at.is_empty(){return Err("가격표 검토 시각이 없습니다.".into());}
    let labels=items(payload,"labels");
    let mut products=state.get("products").filter(|v|v.is_array()).cloned()
        .ok_or("상품 목록이 없습니다.")?;
    let mut changes=Vec::<Value>::new();
    let mut errors=Vec::<String>::new();
    let mut seen=HashSet::<String>::new();
    for row in items(source,"rows") {
        let number=row.get("row").and_then(Value::as_u64).unwrap_or(0);
        let values=items(row,"values").iter().map(|v|v.as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        if values.iter().all(|v|v.is_empty()){continue;}
        let err=|msg: String|format!("{number}행: {msg}");
        if row.get("unsafe").and_then(Value::as_bool)==Some(true) {
            errors.push(err("병합된 셀과 수식은 허용되지 않습니다.".into()));
            continue;
        }
        if values.len()!=10 {
            errors.push(err("가격표 열 수가 올바르지 않습니다.".into()));continue;
        }
        let original_id=&values[6];
        let variant_id=&values[7];
        let mapping_key=format!("{original_id}|{variant_id}");
        let product_id=state.get("productMappings").and_then(|v|v.get(mapping_key.as_str()))
            .and_then(Value::as_str).unwrap_or(original_id);
        let key=format!("{product_id}|{variant_id}");
        if !seen.insert(key) {
            errors.push(err("상품과 가격기록 ID가 중복된 행입니다.".into()));continue;
        }
        let products_array=products.as_array_mut().ok_or("상품 목록이 없습니다.")?;
        let pi=products_array.iter().position(|p|
            field(p,"id")==product_id && p.get("active")!=Some(&Value::Bool(false)) &&
            field(p,"deletedAt").is_empty() && field(p,"supersededBy").is_empty());
        let Some(pi)=pi else{
            errors.push(err("상품명 또는 숨김 상품/가격 ID가 현재 데이터와 일치하지 않습니다. 최신 가격표를 다시 내려받으세요.".into()));
            continue;
        };
        let product=&mut products_array[pi];
        let vi=items(product,"variants").iter().position(|v|field(v,"id")==variant_id);
        let Some(vi)=vi else{
            errors.push(err("상품명 또는 숨김 상품/가격 ID가 현재 데이터와 일치하지 않습니다. 최신 가격표를 다시 내려받으세요.".into()));
            continue;
        };
        let variant=&product["variants"][vi];
        let expected_label=labels.iter().find(|l|
            field(l,"productId")==product_id && field(l,"variantId")==variant_id);
        let valid_label=expected_label.is_some_and(|label|
            values[2]==field(label,"display") || values[2]==field(label,"fallback"));
        if values[0]!=field(product,"name") ||
            values[1]!=field(product,"subtitle") ||
            values[8]!=field(variant,"option") ||
            values[9]!=field(variant,"color") || !valid_label {
            errors.push(err("상품명 또는 숨김 상품/가격 ID가 현재 데이터와 일치하지 않습니다. 최신 가격표를 다시 내려받으세요.".into()));
            continue;
        }
        let product_name=field(product,"name").to_string();
        let sale_label=expected_label.map(|label|field(label,"display").to_string()).unwrap_or_default();
        for (index,tier) in TIERS.iter().enumerate() {
            let raw=&values[index+3];
            if raw.is_empty(){continue;}
            let after=match parse_price(raw) {
                Ok(x)=>x,
                Err(message)=>{errors.push(err(format!("{}: {message}",TIER_LABELS[index])));continue;}
            };
            let before=product["variants"][vi][*tier].clone();
            if before==after{continue;}
            let variant=product["variants"][vi].clone();
            let warning=if *tier=="retail" && variant.get("officialRetail").is_some() &&
                variant.get("officialRetail")!=Some(&after) {
                let official=variant.get("officialRetail").unwrap();
                Some(format!("공식 수집 소매가 {}원과 다릅니다.",
                    if official.is_null(){"미확인".into()}else{formatted(official)}))
            }else{None};
            let before_label=formatted(&before);
            let after_label=if after.is_null(){"가격 삭제".into()}else{formatted(&after)};
            let mut change=json!({
                "id":format!("{}:{tier}",field(&variant,"id")),
                "label":format!("{product_name} {sale_label} / {}",TIER_LABELS[index]),
                "before":before_label,
                "after":after_label
            });
            if let Some(warning)=warning{change["warning"]=json!(warning);}
            changes.push(change);
            product["variants"][vi][*tier]=after.clone();
            if !product.get("priceHistory").is_some_and(Value::is_array) {
                product["priceHistory"]=json!([]);
            }
            product["priceHistory"].as_array_mut().unwrap().push(json!({
                "at":at,
                "field":format!("{}.{tier}",field(&variant,"id")),
                "before":before,
                "after":after,
                "source":"XLSX"
            }));
            product["updatedAt"]=json!(at);
        }
    }
    Ok(json!({"kind":"prices","products":products,"changes":changes,"errors":errors}))
}
#[cfg(test)]
mod tests {
    use super::preview;
    use serde_json::json;
    fn source(price:&str)->serde_json::Value {
        json!({
            "state":{"products":[{"id":"p1","name":"BD 709","subtitle":"","active":true,
                "updatedAt":"2026-01-01","variants":[{"id":"v1","color":"Black",
                "option":"","wholesale":10000,"semi":12000,"retail":15000}]}]},
            "source":{"rows":[{"row":2,"unsafe":false,
                "values":["BD 709","","Black",price,"","","p1","v1","","Black"]}]},
            "labels":[{"productId":"p1","variantId":"v1","display":"Black","fallback":"Black"}],
            "at":"2026-11-19T01:00:00.000Z"
        })
    }
    #[test]
    fn valid_price_change_and_history_are_native(){
        let r=preview(&source("11,000")).unwrap();
        assert_eq!(r["errors"],json!([]));
        assert_eq!(r["changes"][0]["id"],"v1:wholesale");
        assert_eq!(r["products"][0]["variants"][0]["wholesale"],11000);
        assert_eq!(r["products"][0]["priceHistory"][0]["source"],"XLSX");
    }
    #[test]
    fn invalid_price_is_not_applied(){
        let r=preview(&source("-100")).unwrap();
        assert_eq!(r["changes"],json!([]));
        assert_eq!(r["products"][0]["variants"][0]["wholesale"],10000);
        assert!(r["errors"][0].as_str().unwrap().contains("가격은 정수"));
    }
    #[test]
    fn forgery_of_display_or_id_cannot_change_price(){
        let mut s=source("9000");
        s["source"]["rows"][0]["values"][6]=json!("wrong-id");
        let r=preview(&s).unwrap();
        assert_eq!(r["changes"],json!([]));
        assert_eq!(r["products"][0]["variants"][0]["wholesale"],10000);
    }
}
