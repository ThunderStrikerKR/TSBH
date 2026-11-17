//! Conservative, all-category suggestions for the *manual* issue resolver.
//! Never sets productId/variantId: exact matching remains recognition::model.
//! A shortened model spelling can nominate several independent SKU choices.
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap,HashSet};
use super::{active,field,items,natural_cmp};

fn no_space(text:&str)->String {
    text.to_lowercase().chars().filter(|c|!c.is_whitespace()).collect()
}
fn compact_code(text:&str)->String {
    let cleaned=text.trim().to_uppercase().chars()
        .filter(|c|c.is_ascii_alphanumeric()).collect::<String>();
    // "BD" is a catalog prefix, not part of the model identity.
    cleaned.strip_prefix("BD").unwrap_or(&cleaned).to_owned()
}
fn product_codes(product:&Value)->Vec<String>{
    let mut raw=Vec::new();
    raw.push(field(product,"name").split('/').next().unwrap_or(""));
    if let Some(codes)=product.get("modelCodes").and_then(Value::as_array){
        raw.extend(codes.iter().filter_map(Value::as_str));
    }
    let code=field(product,"code");
    // P00000XX is a shopping-mall ID, not the manufacturer's model.
    if !code.starts_with('P') || code.len() < 8 {raw.push(code);}
    let mut seen=HashSet::new();
    raw.into_iter().filter(|s| {
        let compact=s.chars().filter(|c|c.is_ascii_alphanumeric()).collect::<String>();
        !Regex::new(r"(?i)^P[0-9A-Z]{6,}$").unwrap().is_match(&compact)
    }).map(compact_code)
        .filter(|s|s.len()>=2&&s.len()<=32&&seen.insert(s.clone())).collect()
}
fn input_codes(raw:&str)->Vec<String>{
    let trimmed=Regex::new(r"(?i)\s+\d+\s*(?:ea|pcs|개|대|세트)\s*$")
        .expect("quantity suffix").replace(raw,"");
    let re=Regex::new(r"(?i)[A-Z0-9]+(?:-[A-Z0-9]+)*")
        .expect("model tokens");
    let words=re.find_iter(&trimmed)
        .map(|m|m.as_str().to_uppercase())
        .filter(|s| !["EA","PCS","BANDO"].contains(&s.as_str()))
        .collect::<Vec<_>>();
    let mut seen=HashSet::new();
    let mut output=Vec::new();
    for i in 0..words.len(){
        for n in 1..=3.min(words.len()-i){
            let token=words[i..i+n].join("");
            let key=compact_code(&token);
            if key.len()>=2 && key.len()<=32 &&
                (key.chars().any(|c|c.is_ascii_digit())||key.len()>=3)&&
                seen.insert(key.clone()) {
                output.push(key);
            }
        }
    }
    output
}
fn distance(a:&str,b:&str)->usize {
    let ac=a.chars().collect::<Vec<_>>();
    let bc=b.chars().collect::<Vec<_>>();
    let mut row=(0..=bc.len()).collect::<Vec<_>>();
    for (i,left) in ac.iter().enumerate(){
        let mut next=vec![i+1];
        for (j,right) in bc.iter().enumerate(){
            next.push((next[j]+1).min(row[j+1]+1)
                .min(row[j]+usize::from(left!=right)));
        }
        row=next;
    }
    row[bc.len()]
}
fn code_points(input:&[String],product:&Value)->i32{
    let candidates=product_codes(product);
    let mut best=0;
    for token in input {
        for model in &candidates {
            if token==model { best=best.max(180);continue; }
            // Short model names (Z4, SS-B, GT-U, 705) are too similar to
            // unrelated inventory to use edit-distance matching.
            let len=token.chars().count().min(model.chars().count());
            if len<4 {continue;}
            let d=distance(token,model);
            if d==1 {best=best.max(78);}
            else if d==2 && len>=6 {best=best.max(52);}
        }
    }
    best
}
// Only vocabulary equivalences, NOT product identity guesses. An exact code
// carries stronger evidence than any of these coarse category cues.
const CUES:&[(&[&str],&[&str])]=&[
    (&["가방","케이스","파우치","bag","case"], &["가방","케이스","파우치","bag","case"]),
    (&["스피커","speaker","pa용"],&["스피커","speaker","pa용"]),
    (&["마이크","mic","microphone"],&["마이크","mic","microphone"]),
    (&["보면대","악보대","악보스탠드"],&["보면대","악보대","악보스탠드"]),
    (&["뮤직트레이","뮤직 트레이","악보트레이"],&["뮤직트레이","뮤직트레이","악보트레이"]),
    (&["건반","키보드","keyboard"],&["건반","키보드","keyboard"]),
    (&["기타","guitar"],&["기타","guitar"]),
    (&["첼로","cello"],&["첼로","cello"]),
    (&["색소폰","sax"],&["색소폰","sax"]),
    (&["노트북","notebook","laptop"],&["노트북","notebook","laptop"]),
    (&["태블릿","tablet","아이패드"],&["태블릿","tablet","아이패드"]),
    (&["카트","cart"],&["카트","cart"]),
    (&["바퀴","caster"],&["바퀴","caster"]),
    (&["홀더","holder"],&["홀더","holder"]),
    (&["조명","라이트","led"],&["조명","라이트","led"]),
    (&["의자","스툴","chair"],&["의자","스툴","chair"]),
    (&["발판","footrest"],&["발판","footrest"]),
    (&["마운트","브라켓","브래킷"],&["마운트","브라켓","브래킷"]),
    (&["클립","clip"],&["클립","clip"]),
];
fn cue_set(raw:&str)->HashSet<usize> {
    let plain=no_space(raw);
    CUES.iter().enumerate().filter_map(|(i,(source,_))|
        source.iter().any(|term|plain.contains(&no_space(term))).then_some(i))
        .collect()
}
fn semantic_points(cues:&HashSet<usize>,product:&Value)->(i32,usize){
    let product_text=no_space(&format!("{} {} {}",field(product,"category"),
        field(product,"name"),field(product,"subtitle")));
    let matched=cues.iter().filter(|i|CUES[**i].1.iter()
        .any(|word|product_text.contains(&no_space(word)))).count();
    (matched as i32*30,matched)
}
fn selected_history(state:&Value,raw:&str,client:&str)->HashMap<String,u32>{
    let requested=no_space(raw);
    let requested=Regex::new(r"(?i)\d+(?:ea|pcs|개|대|세트)$")
        .unwrap().replace(&requested,"").to_string();
    let mut result=HashMap::new();
    if client.is_empty()||requested.is_empty(){return result;}
    for order in items(state,"orders"){
        if !field(order,"deletedAt").is_empty()||
            field(order,"confirmedAt").is_empty()||
            field(order,"clientId")!=client {continue;}
        let snapshot=order.get("confirmationSnapshot").unwrap_or(&Value::Null);
        for recipient in items(snapshot,"recipients"){
            for item in items(recipient,"items"){
                let e=item.get("selectionEvidence").unwrap_or(&Value::Null);
                if e.is_null()||field(e,"productId")!=field(item,"productId")||
                    field(e,"variantId")!=field(item,"variantId"){continue;}
                let expression=no_space(field(e,"expression"));
                let expression=Regex::new(r"(?i)\d+(?:ea|pcs|개|대|세트)$")
                    .unwrap().replace(&expression,"").to_string();
                if expression==requested {
                    *result.entry(field(item,"productId").to_owned()).or_insert(0)+=1;
                }
            }
        }
    }
    result
}
fn color_hint(raw:&str)->Option<&'static str>{
    let s=no_space(raw);
    if s.contains("그레이")||s.contains("회색")||s.contains("gray"){Some("gray")}
    else if s.contains("블랙")||s.contains("검정")||s.contains("black"){Some("black")}
    else if s.contains("화이트")||s.contains("흰색")||s.contains("white"){Some("white")}
    else if s.contains("브라운")||s.contains("brown"){Some("brown")}
    else if s.contains("실버")||s.contains("silver"){Some("silver")}
    else {None}
}
fn product_color(product:&Value)->String{
    let s=no_space(&format!("{} {}",field(product,"name"),field(product,"subtitle")));
    if s.contains("그레이")||s.contains("gray"){return "gray".into();}
    if s.contains("블랙")||s.contains("black"){return "black".into();}
    if s.contains("화이트")||s.contains("white"){return "white".into();}
    if s.contains("브라운")||s.contains("brown"){return "brown".into();}
    if s.contains("실버")||s.contains("silver"){return "silver".into();}
    let colors=items(product,"variants").iter().map(|v|no_space(field(v,"color")))
        .collect::<Vec<_>>();
    if colors.len()==1 {
        let c=&colors[0];
        if c.contains("그레이")||c.contains("gray"){return "gray".into();}
        if c.contains("블랙")||c.contains("black"){return "black".into();}
        if c.contains("화이트")||c.contains("white"){return "white".into();}
        if c.contains("브라운")||c.contains("brown"){return "brown".into();}
        if c.contains("실버")||c.contains("silver"){return "silver".into();}
    }
    String::new()
}
pub(super) fn candidates(state:&Value,raw:&str,client:&str)->Vec<String>{
    let input=input_codes(raw);
    let cues=cue_set(raw);
    let history=selected_history(state,raw,client);
    let hint=color_hint(raw);
    let mut scored=Vec::new();
    for p in items(state,"products").iter().filter(|p|
        active(p) && field(p,"catalog")!="DEMO" &&
        !items(p,"variants").iter().any(|v|field(v,"source")=="DEMO")) {
        let code=code_points(&input,p);
        let (semantic,matched)=semantic_points(&cues,p);
        let previous=history.get(field(p,"id")).copied().unwrap_or(0);
        // Without a model or verified past selection, a single generic word
        // such as "가방" cannot nominate dozens of unrelated catalog items.
        if code==0 && previous==0 && matched<2 {continue;}
        let mut score=code+semantic+if previous>0 {115+12*previous.min(3) as i32} else {0};
        if let Some(desired)=hint{
            let actual=product_color(p);
            if actual==desired {score+=28;}
            else if !actual.is_empty() {score-=70;}
        }
        // A weak edit alone is insufficient unless the category also fits.
        if score<55 || (code>0 && code<100 && matched==0 && previous==0){continue;}
        scored.push((p,score,code,matched,previous));
    }
    scored.sort_by(|a,b|b.1.cmp(&a.1)
        .then_with(||b.2.cmp(&a.2))
        .then_with(||b.3.cmp(&a.3))
        .then_with(||b.4.cmp(&a.4))
        .then_with(||natural_cmp(field(a.0,"name"),field(b.0,"name"))));
    scored.into_iter().take(8).map(|(p,_,_,_,_)|field(p,"id").to_owned()).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn seed()->Value {
        serde_json::from_str(include_str!("../../../../resources/state-seed.json")).unwrap()
    }
    #[test]
    fn ssb_abbreviation_suggests_registered_speaker_case_first(){
        let state=seed();
        let ids=candidates(&state,"반도 SSB 가방    2EA","");
        assert_eq!(ids.first().map(String::as_str),Some("bando-site-315"),"{ids:?}");
        assert!(!ids.iter().any(|id|id=="bando-site-193"),"SS-2 is not SS-B");
    }
    #[test]
    fn all_families_and_short_model_ambiguities_are_supported(){
        let state=seed();
        assert_eq!(candidates(&state,"MICB 가방 1EA","").first().map(String::as_str),
            Some("bando-site-258"));
        assert_eq!(candidates(&state,"GT150H 기타 스탠드 1EA","").first().map(String::as_str),
            Some("bando-site-169"));
        let z4=candidates(&state,"BD Z4 1EA","");
        assert_eq!(z4.iter().filter(|id|id.starts_with("sku:bando-site-138:")).count(),4,
            "Independent Z-4 surface sizes must all remain selectable: {z4:?}");
        assert!(candidates(&state,"가방 1EA","").is_empty(),
            "Generic accessory term alone cannot guess a particular SKU");
        let speaker_bag=candidates(&state,"스피커 스탠드 가방 2EA","");
        assert_eq!(speaker_bag.first().map(String::as_str),Some("bando-site-315"),
            "Two independent product-use cues should find the speaker case: {speaker_bag:?}");
        assert!(!candidates(&state,"반도 705 보면대 1EA","").is_empty(),
            "A numeric three-digit model must remain searchable");
    }
    #[test]
    fn history_only_comes_from_confirmed_client_selected_evidence(){
        let mut state=json!({"products":[{"id":"p1","name":"Custom / 가나다 특수품","active":true,
            "category":"기타","variants":[{"id":"v1","color":"색상 미표기"}]}],"orders":[]});
        let snapshot=json!({"id":"o1","clientId":"c1","confirmedAt":"2026-01-01",
            "confirmationSnapshot":{"recipients":[{"items":[{
                "productId":"p1","variantId":"v1","selectionEvidence":{
                    "expression":"특수 주문 2EA","productId":"p1","variantId":"v1"}
            }]}]}});
        state["orders"]=json!([snapshot]);
        assert_eq!(candidates(&state,"특수 주문 2EA","c1"),vec!["p1"]);
        assert!(candidates(&state,"특수 주문 2EA","c2").is_empty());
        state["orders"][0]["confirmedAt"]=Value::Null;
        assert!(candidates(&state,"특수 주문 2EA","c1").is_empty());
    }
    #[test]
    fn no_cross_family_matches_for_unrelated_short_codes(){
        let state=seed();
        let ids=candidates(&state,"반도 SSB 가방 2EA","");
        assert!(!ids.contains(&"bando-site-258".to_owned()),
            "A microphone case must not be presented as the speaker case");
        assert!(candidates(&state,"택배 선불","").is_empty());
    }
}
