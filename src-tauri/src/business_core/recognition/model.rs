use regex::{Regex, RegexBuilder};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug)]
pub(crate) struct Decision {
    pub(crate) product_id: Option<String>,
    pub(crate) variant_id: Option<String>,
    pub(crate) name: String,
    pub(crate) color: String,
    pub(crate) origin: String,
    pub(crate) kind: String,
    pub(crate) term: String,
    pub(crate) reason: Option<String>,
    pub(crate) candidates: Vec<Value>,
    pub(crate) alias_id: Option<String>,
    pub(crate) span: Option<(usize, usize)>,
}

fn str_field<'a>(v: &'a Value, name: &str) -> &'a str {
    v.get(name).and_then(Value::as_str).unwrap_or("")
}
fn registered(p: &Value) -> bool {
    p.get("active").and_then(Value::as_bool) != Some(false)
        && p.get("deletedAt").map_or(true, Value::is_null)
        && p.get("supersededBy").map_or(true, Value::is_null)
}
pub(crate) fn products(state: &Value) -> Vec<&Value> {
    state.get("products").and_then(Value::as_array)
        .map(|v| v.iter().filter(|p| registered(p)).collect()).unwrap_or_default()
}
fn variants(p: &Value) -> Vec<&Value> {
    p.get("variants").and_then(Value::as_array)
        .map(|xs| xs.iter().collect()).unwrap_or_default()
}
fn lead(p: &Value) -> &str { str_field(p, "name").split('/').next().unwrap_or("").trim() }
fn score_stand(p: &Value) -> bool {
    let c = str_field(p, "category").trim();
    c == "보면대" || c.starts_with("보면대 >")
}
fn score_model(p: &Value) -> Option<(String, bool, String)> {
    if !score_stand(p) { return None; }
    static SCORE_MODEL:OnceLock<Regex>=OnceLock::new();
    let re=SCORE_MODEL.get_or_init(||Regex::new(r"(?i)^(?:BD[\s-]*)?(\d{3})(-1)?\s*(BR|G|W)?$")
        .expect("registered music stand code"));
    let cap = re.captures(lead(p))?;
    Some((cap[1].to_owned(), cap.get(2).is_some(),
        cap.get(3).map(|m| m.as_str().to_uppercase()).unwrap_or_default()))
}
fn verified_bases(ps: &[&Value]) -> HashSet<String> {
    let mut with_holes = HashSet::new();
    let mut without = HashSet::new();
    for p in ps {
        if let Some((base, no_holes, _)) = score_model(p) {
            let name = str_field(p, "name").replace(' ', "");
            if no_holes && (name.contains("비타공상판") || name.contains("무타공상판")) {
                without.insert(base);
            } else if !no_holes && name.contains("/타공상판") {
                with_holes.insert(base);
            }
        }
    }
    with_holes.intersection(&without).cloned().collect()
}
fn top(p: &Value, verified: &HashSet<String>) -> Option<&'static str> {
    let (base, minus, _) = score_model(p)?;
    if minus { return Some("UNPERFORATED"); }
    let name = str_field(p, "name").replace(' ', "");
    if name.contains("/타공상판") || verified.contains(&base) {
        Some("PERFORATED")
    } else { None }
}
fn no_holes(text: &str) -> bool {
    static NO_HOLES:OnceLock<Regex>=OnceLock::new();
    NO_HOLES.get_or_init(||Regex::new(
        r"민\s*판|무\s*타공|비\s*타공|구멍\s*(?:없음|없는|안\s*뚫린)"
    ).expect("static regex")).is_match(text)
}
fn with_holes(text: &str) -> bool {
    static WITH_HOLES:OnceLock<Regex>=OnceLock::new();
    !no_holes(text) && WITH_HOLES.get_or_init(||Regex::new(
        r"(?:^|[\s/,(])타\s*공|구멍\s*(?:있는|있음|뚫린)"
    ).expect("static regex")).is_match(text)
}
fn is_boundary(text: &str, begin: usize, end: usize) -> bool {
    let disallow = |ch: char| ch.is_ascii_alphanumeric() || ch == '-';
    !text[..begin].chars().next_back().map_or(false, disallow)
        && !text[end..].chars().next().map_or(false, disallow)
}
// A color-code abbreviation must end at the same word/punctuation
// boundary accepted by the original TypeScript recognizer. A letter in
// "B타입"/"B 타입"/"B형"/"B Type" is an option designation, NOT Black.
// These checks apply only to model-attached color suffixes; they preserve
// standalone registered product codes and explicit color words.
fn color_suffix_boundary(text: &str, begin: usize, end: usize) -> bool {
    if !is_boundary(text, begin, end) { return false; }
    let rest=&text[end..];
    if !rest.chars().next().map_or(true,|c|
        c.is_whitespace() || ",.!*×".contains(c))
    { return false; }
    let after=rest.trim_start();
    if after.starts_with("타입") || after.starts_with('형')
        || after.get(..4).map_or(false,|part|part.eq_ignore_ascii_case("type"))
    {return false;}
    true
}
fn spell(term: &str, is_model: bool) -> String {
    let bare = if is_model {
        static BD_PREFIX:OnceLock<Regex>=OnceLock::new();
        let bd=BD_PREFIX.get_or_init(||Regex::new(r"(?i)^BD[\s-]+")
            .expect("registered code prefix"));
        bd.replace(term, "").into_owned()
    } else { term.to_owned() };
    let prefix = if is_model && bare.chars().next().map_or(false, |c| c.is_ascii_alphanumeric()) {
        r"(?:BD[\s-]*)?"
    } else { "" };
    let mut result = String::from(prefix);
    for ch in bare.chars() {
        if ch.is_whitespace() { result.push_str(r"\s*"); }
        else if ch == '/' { result.push_str(r"\s*/\s*"); }
        else { result.push_str(&regex::escape(&ch.to_string())); }
    }
    result
}
pub(crate) fn find_term(text: &str, term: &str, is_model: bool) -> Option<(usize, usize)> {
    if term.trim().is_empty() { return None; }
    // A full product catalog scan repeats the same registered model patterns
    // hundreds of times. Cache the compiled *identical* regex so this changes
    // CPU work, not matching semantics. Mutex ownership never escapes.
    static PATTERNS: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let pattern=spell(term.trim(),is_model);
    let cache=PATTERNS.get_or_init(||Mutex::new(HashMap::new()));
    let re={
        let mut lock=cache.lock().ok()?;
        if let Some(existing)=lock.get(&pattern) {
            existing.clone()
        } else {
            let compiled=RegexBuilder::new(&pattern)
                .case_insensitive(true).build().ok()?;
            lock.insert(pattern,compiled.clone());
            compiled
        }
    };
    let found=re.find_iter(text).find(|m| is_boundary(text,m.start(),m.end()))
        .map(|m|(m.start(),m.end()));
    found
}
pub(crate) fn model_names(p: &Value) -> Vec<String> {
    let name = lead(p);
    if name.is_empty() { return vec![]; }
    static BD_PREFIX:OnceLock<Regex>=OnceLock::new();
    let bd=BD_PREFIX.get_or_init(||Regex::new(r"(?i)^BD[\s-]+")
        .expect("registered code prefix"));
    let bare=bd.replace(name,"");
    if bare == name { vec![name.to_owned()] }
    else { vec![name.to_owned(), bare.into_owned()] }
}
pub(crate) fn category(text: &str) -> Option<&'static str> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let key = text.to_lowercase();
    if key.contains("키보드스탠드") || key.contains("건반스탠드") || key.contains("키보드") {
        Some("키보드")
    } else if key.contains("마이크스탠드") || key.contains("마이크") {
        Some("마이크")
    } else if key.contains("보면대") || key.contains("악보대") {
        Some("보면대")
    } else { None }
}
fn generic(text: &str) -> bool {
    let key: String = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    matches!(key.as_str(), "보면대" | "악보대" | "반도보면대" | "반도악보대"
        | "마이크스탠드" | "반도마이크스탠드"
        | "키보드스탠드" | "건반스탠드" | "반도키보드스탠드")
}
pub(crate) fn norm_color(source: &str) -> String {
    match source.trim().to_lowercase().as_str() {
        "black" | "블랙" | "검정" | "검정색" | "b" | "bk" => "Black",
        "white" | "화이트" | "흰색" | "w" => "White",
        "brown" | "브라운" | "br" => "Brown",
        "gray" | "grey" | "그레이" | "회색" | "g" => "Gray",
        "chrome" | "크롬" | "c" => "Chrome",
        "gold" | "골드" => "Gold",
        "rose gold" | "로즈골드" => "Rose Gold",
        "cosmo black" | "cosmoblack" | "코스모블랙" => "Cosmo Black",
        "silver" | "실버" => "Silver",
        "red" | "레드" => "Red",
        "blue" | "블루" => "Blue",
        "clear" | "클리어" => "Clear",
        "frost" | "프로스트" => "Frost",
        _ => source.trim()
    }.to_owned()
}
/// parser.ts::modelColorHint / registeredColorHint: a model-associated
/// color suffix or prefix is stronger evidence than an unqualified color word.
fn code_color_hint(text: &str, ps: &[&Value]) -> Option<(String,String,String)> {
    let number_suffix=Regex::new(concat!(
        r"(?i)(?:BD\s*)?(\d{3}(?:-\d+)?)\s*",
        r"(?:\(\s*(BK|BR|B|C|W|G|검정|블랙|크롬|화이트|그레이|브라운)\s*\)",
        r"|(BK|BR|B|C|W|G))"
    )).expect("model color suffix");
    for c in number_suffix.captures_iter(text) {
        let m=c.get(0)?;
        if !color_suffix_boundary(text,m.start(),m.end()) {continue;}
        let color=c.get(2).or_else(||c.get(3))?.as_str();
        return Some((c[1].to_owned(),norm_color(color),m.as_str().to_owned()));
    }
    let number_prefix=Regex::new(
        r"(?i)(BK|BR|B|C|W|G)\s+(?:BD\s*)?(\d{3}(?:-\d+)?)"
    ).expect("model color prefix");
    for c in number_prefix.captures_iter(text) {
        let m=c.get(0)?;
        if is_boundary(text,m.start(),m.end()) {
            return Some((c[2].to_owned(),norm_color(&c[1]),m.as_str().to_owned()));
        }
    }
    // The same code+color syntax also applies to alphabetic registered
    // models. The source implementation uses registered model names,
    // not unregistered fragments, as the only accepted bases.
    let color_code=Regex::new(r"(?i)\s*(?:\(\s*(BK|BR|B|C|W|G)\s*\)|(BK|BR|B|C|W|G))")
        .expect("registered color suffix");
    for p in ps {
        for model in model_names(p) {
            let Some((begin,end))=find_term(text,&model,true) else {continue;};
            if let Some(c)=color_code.captures(&text[end..]) {
                let m=c.get(0)?;
                if m.start()!=0 {continue;}
                let end=end+m.end();
                if !color_suffix_boundary(text,begin,end) {continue;}
                let color=c.get(1).or_else(||c.get(2))?.as_str();
                return Some((model.clone(),norm_color(color),
                    text[begin..end].to_owned()));
            }
        }
    }
    None
}
/// Read-only learning proof for the existing model-attached abbreviation
/// grammar. Verify the matching registered model and exact source overlap.
pub(super) fn audit_attached_color(text:&str,item:&Value,source:(usize,usize))->Option<(String,(usize,usize))>{
    let (model,color,matched)=code_color_hint(text,&[item])?;
    if !model_names(item).iter().any(|name|model_key(name)==model_key(&model)){
        return None;
    }
    let mut hits=text.match_indices(&matched);
    let (start,part)=hits.next()?;
    if hits.next().is_some(){return None;}
    let end=start+part.len();
    if start>=source.1||end<=source.0{return None;}
    Some((color,(start,end)))
}
fn written_color(text: &str) -> Option<String> {
    for word in ["rose gold","로즈골드","cosmo black","cosmoblack","코스모블랙",
        "gold","골드","black","블랙","검정","gray","grey","그레이",
        "회색","white","화이트","흰색","brown","브라운","clear",
        "클리어","frost","프로스트","silver","실버","blue",
        "블루","red","레드","pink","핑크"] {
        if find_term(text,word,false).is_some() {
            return Some(norm_color(word));
        }
    }
    None
}
fn model_key(source: &str) -> String {
    source.to_uppercase().chars().filter(|c|!c.is_whitespace()).collect::<String>()
        .trim_start_matches("BD").to_owned()
}
fn brand_words<'a>(state: &'a Value, p: &Value) -> Vec<&'a str> {
    let bid=str_field(p,"brandId");
    if bid.is_empty(){return Vec::new();}
    state.get("brands").and_then(Value::as_array).into_iter().flatten()
        .filter(|b|str_field(b,"id")==bid)
        .flat_map(|b| {
            let mut words=vec![str_field(b,"name"),str_field(b,"korean")];
            words.extend(b.get("aliases").and_then(Value::as_array)
                .into_iter().flatten().filter_map(Value::as_str));
            words
        }).filter(|w|!w.is_empty()).collect()
}
fn brand_matches(text: &str, words: &[&str]) -> bool {
    let lower=text.to_lowercase();
    words.iter().any(|w|!w.is_empty() && lower.contains(&w.to_lowercase()))
}
fn filter_business_candidates<'a>(
    text: &str,state: &'a Value, ps: &[&'a Value],mut entries:Vec<Entry<'a>>
) -> (Vec<Entry<'a>>,bool) {
    let branded:Vec<&Value>=ps.iter().copied().filter(|p|
        brand_matches(text,&brand_words(state,p))).collect();
    if !branded.is_empty() {
        entries.retain(|e|branded.iter().any(|p|str_field(p,"id")==str_field(e.p,"id")));
    } else if entries.iter().any(|e|str_field(e.p,"catalog")!="BANDOMALL")
        && Regex::new(r"(?i)^(?:BD\s*)?\d{3}[A-Za-z-]*(?:\s|$)")
            .expect("factory catalog code").is_match(text) {
        entries.retain(|e|str_field(e.p,"catalog")!="BANDOMALL");
    }
    if entries.iter().any(|e|!variants(e.p).iter()
        .any(|v|str_field(v,"source")=="DEMO")) {
        entries.retain(|e|!variants(e.p).iter()
            .any(|v|str_field(v,"source")=="DEMO"));
    }
    let known=state.get("brands").and_then(Value::as_array).map_or(false,|brands|
        brands.iter().any(|b|{
            let mut words=vec![str_field(b,"name"),str_field(b,"korean")];
            words.extend(b.get("aliases").and_then(Value::as_array)
                .into_iter().flatten().filter_map(Value::as_str));
            brand_matches(text,&words)
        }));
    (entries,known)
}

fn qualifiers(text: &str) -> HashSet<String> {
    // Most historical qualifiers intentionally tolerate whitespace. Do NOT
    // concatenate whitespace for A/B type: "...LP-WA 타입B" would fabricate
    // "A타입" across the model-code boundary and select the WRONG SKU.
    let input: String = text.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    static QUALIFIERS:OnceLock<Regex>=OnceLock::new();
    let re=QUALIFIERS.get_or_init(||Regex::new(
        r"(?:가방|바퀴)(?:미포함|포함|미장착|장착)|민판|무타공상판|무타공|비타공상판|타공상판|\d+(?:\.\d+)?cm"
    ).expect("static regex"));
    let mut found: HashSet<String> = re.find_iter(&input).map(|m| {
        let t = m.as_str();
        if t == "민판" || t.starts_with("무타공") { "비타공상판".to_owned() }
        else { t.to_owned() }
    }).collect();
    // Evaluate type tokens against ORIGINAL boundaries, then normalize both
    // "A 타입" and "타입 A" to the registered name's "a타입".
    // Run opposing orders separately: in "...LP-WA 타입B", the regex
    // "A 타입" can overlap the real "타입B" while starting *inside* the
    // registered model code. A single alternation would consume that span
    // before the boundary rejection and hide the valid second candidate.
    for expression in [r"(?i)([ab])\s*타입",r"(?i)타입\s*[:：]?\s*([ab])"] {
        let type_re=Regex::new(expression).expect("type expression");
        for c in type_re.captures_iter(text) {
            let Some(span)=c.get(0) else {continue};
            if !is_boundary(text,span.start(),span.end()) {continue;}
            if let Some(letter)=c.get(1) {
                found.insert(format!("{}타입",letter.as_str().to_ascii_lowercase()));
            }
        }
    }
    found
}
fn conflicting_qualifiers(proof:&HashSet<String>)->bool{
    [("가방포함","가방미포함"),("바퀴포함","바퀴미포함"),
        ("바퀴장착","바퀴미장착"),("타공상판","비타공상판"),("a타입","b타입")]
        .into_iter().any(|(a,b)|proof.contains(a)&&proof.contains(b))
}
fn qualifies(p: &Value, proof: &HashSet<String>, verified: &HashSet<String>) -> bool {
    if conflicting_qualifiers(proof){return false;}
    let mut named = qualifiers(str_field(p, "name"));
    match top(p, verified) {
        Some("PERFORATED") => { named.insert("타공상판".into()); },
        Some("UNPERFORATED") => { named.insert("비타공상판".into()); },
        _ => {}
    }
    proof.is_subset(&named)
}
fn option_score(text: &str, p: &Value) -> usize {
    str_field(p, "name").split('/').skip(1)
        .filter(|s| !s.trim().is_empty() && find_term(text, s.trim(), false).is_some())
        .count()
}
fn terms(p: &Value) -> Vec<(String, u8, bool)> {
    let mut xs = vec![(str_field(p, "name").to_owned(), 3, false)];
    xs.extend(model_names(p).into_iter().map(|s| (s, 2, true)));
    for segment in str_field(p, "name").split('/').skip(1) {
        let s = segment.trim();
        if s.chars().any(|c| ('가'..='힣').contains(&c)) &&
            s.chars().filter(|c| !c.is_whitespace()).count() >= 4 &&
            ["보면대","악보대","마이크 스탠드","키보드 스탠드","건반 스탠드",
                "의자","카트","블랙","검정","화이트","실버","그레이","브라운",
                "타공 상판","비타공 상판","가방 포함","가방 미포함"].contains(&s) == false &&
            ["스탠드","트레이","홀더","마운트","발판","거치대","어댑터",
                "테이블","마그네틱","클립"].iter().any(|w| s.contains(w)) {
                xs.push((s.to_owned(), 1, false));
            }
    }
    xs.sort_by(|a,b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())));
    xs
}
#[derive(Clone)]
struct Entry<'a> { p: &'a Value, priority: u8, span: (usize, usize) }
fn evidence<'a>(text: &str, p: &'a Value, verified: &HashSet<String>) -> Option<Entry<'a>> {
    for (term, priority, model) in terms(p) {
        if let Some(span) = find_term(text, &term, model) {
            return Some(Entry { p, priority, span });
        }
    }
    if let Some((base, _, _)) = score_model(p) {
        if (verified.contains(&base) && ((no_holes(text) && top(p, verified) == Some("UNPERFORATED"))
            || (!no_holes(text) && top(p, verified) == Some("PERFORATED"))))
            || (no_holes(text) && top(p, verified) == Some("UNPERFORATED")) {
            if let Some(span) = find_term(text, &base, true) {
                return Some(Entry { p, priority: 2, span });
            }
        }
    }
    None
}
// Lexicographic baseline evidence, not a calibrated probability. Compute
// option and length evidence once per eligible candidate, and expose the
// comparison components before lower-ranked candidates are discarded.
fn strongest<'a>(text: &str, entries: Vec<Entry<'a>>,trace:&mut Option<&mut Vec<Value>>) -> Vec<Entry<'a>> {
    if entries.is_empty() { return entries; }
    let priority=entries.iter().map(|e|e.priority).max().unwrap_or(0);
    let scored=entries.into_iter().map(|e|{
        let length=if e.priority==2{text[e.span.0..e.span.1].chars().filter(|c|!c.is_whitespace()).count()}else{0};
        let options=if e.priority>=2&&(e.priority==priority||trace.is_some()){option_score(text,e.p)}else{0};
        let rank=(e.priority,length,options);
        (e,rank)
    }).collect::<Vec<_>>();
    let best=scored.iter().map(|(_,r)|*r).max().unwrap_or_default();
    let ties=scored.iter().filter(|(_,r)|*r==best).count();
    if let Some(out)=trace.as_deref_mut(){
        out.push(json!({"stage":"name_candidate_comparison","policyVersion":"legacy-lexicographic-v1",
            "count":scored.len(),"truncated":scored.len()>128,
            "candidates":scored.iter().take(128).map(|(e,r)|json!({
                "productId":str_field(e.p,"id"),"priority":r.0,"matchedLength":r.1,
                "optionScore":r.2,"byteStart":e.span.0,"byteEnd":e.span.1,
                "survivesComparison":*r==best&&!(priority==1&&ties>1)
            })).collect::<Vec<_>>()}));
    }
    if priority==1&&ties>1{return vec![];}
    scored.into_iter().filter_map(|(e,r)|(r==best).then_some(e)).collect()
}
// Observe the real stage survivors without re-running matching. A bounded
// stage explicitly declares truncation; it cannot certify complete coverage.
fn trace_entries(trace:&mut Option<&mut Vec<Value>>,stage:&str,entries:&[Entry<'_>]){
    if let Some(out)=trace.as_deref_mut(){
        out.push(json!({"stage":stage,"count":entries.len(),"truncated":entries.len()>128,
            "candidates":entries.iter().take(128).map(|e|json!({
                "productId":str_field(e.p,"id"),"priority":e.priority,
                "byteStart":e.span.0,"byteEnd":e.span.1})).collect::<Vec<_>>()}));
    }
}
fn name_entries<'a>(text: &str, ps: &[&'a Value],
    verified: &HashSet<String>,trace:&mut Option<&mut Vec<Value>>) -> Vec<Entry<'a>> {
    let mut entries: Vec<_> = ps.iter().filter_map(|p| evidence(text, p, verified)).collect();
    trace_entries(trace,"literal_evidence",&entries);
    if no_holes(text) {
        entries.retain(|e| top(e.p, verified) != Some("PERFORATED"));
    } else if with_holes(text) {
        entries.retain(|e| top(e.p, verified) != Some("UNPERFORATED"));
    }
    trace_entries(trace,"perforation_filter",&entries);
    let numeric = Regex::new(r"(?i)(?:BD[\s-]*)?(\d{3})(?:\b|$)").expect("static regex");
    if let Some(m) = numeric.captures(text) {
        if let Some(whole) = m.get(0) {
            let base = m.get(1).map(|n| n.as_str()).unwrap_or("");
            if is_boundary(text, whole.start(), whole.end()) && verified.contains(base) {
                let desired = if no_holes(text) { "UNPERFORATED" } else { "PERFORATED" };
                let family: Vec<_> = entries.iter()
                    .filter(|e| score_model(e.p).map_or(false, |(b,_,_)| b == base)
                        && top(e.p, verified) == Some(desired))
                    .cloned().collect();
                if !family.is_empty() {
                    trace_entries(trace,"verified_model_family",&family);
                    let selected = if let Some(color) = written_color(text) {
                        family.into_iter().filter(|e| variants(e.p).iter().any(|v|
                            norm_color(str_field(v, "color")) == color) ||
                            norm_color(str_field(e.p, "name")) == color).collect()
                    } else {
                        let defaults: Vec<_> = family.iter().filter(|e|
                            score_model(e.p).map_or(false, |(_,_,suffix)| suffix.is_empty()))
                            .cloned().collect();
                        if defaults.is_empty() { family } else { defaults }
                    };
                    trace_entries(trace,"family_color_or_default_filter",&selected);
                    let selected=strongest(text,selected,trace);
                    trace_entries(trace,"strongest_name_evidence",&selected);
                    return selected;
                }
            }
        }
    }
    let selected=strongest(text,entries,trace);
    trace_entries(trace,"strongest_name_evidence",&selected);
    selected
}
fn alias_entries<'a>(text: &str, state: &'a Value, client_id: &str,
    ps: &[&'a Value]) -> Vec<(&'a Value, &'a Value)> {
    let mut aliases: Vec<&Value> = state.get("aliases").and_then(Value::as_array)
        .map(|xs| xs.iter().filter(|a| {
            let scope = a.get("clientId").and_then(Value::as_str);
            (scope.is_none() || scope == Some(client_id)) &&
                !generic(str_field(a,"text")) &&
                find_term(text, str_field(a,"text"), true).is_some()
        }).collect()).unwrap_or_default();
    aliases.sort_by(|a,b| {
        b.get("clientId").and_then(Value::as_str).is_some()
            .cmp(&a.get("clientId").and_then(Value::as_str).is_some())
            .then_with(|| str_field(b,"text").len().cmp(&str_field(a,"text").len()))
    });
    aliases.into_iter().filter_map(|a| ps.iter()
        .find(|p| str_field(p,"id") == str_field(a,"productId"))
        .map(|p| (a,*p))).collect()
}
// Vocabulary-level normalization, not a learned guess. An unambiguous
// conductor's score stand is described as "지휘용 악보 스탠드" in customer
// orders even when the catalog uses "지휘자 보면대". Do not override explicit
// model codes, registered names, or dealer aliases. More than one active
// conductor SKU must remain an unresolved choice.
fn conductor_semantic_entries<'a>(text: &str, ps: &[&'a Value]) -> Vec<Entry<'a>> {
    let phrase=Regex::new(
        r"지휘(?:자)?\s*용?\s*(?:악보\s*스탠드|보면대|악보대|뮤직\s*스탠드)"
    ).expect("conductor role with score stand");
    let Some(m)=phrase.find(text) else { return vec![]; };
    // A separate model token is stronger evidence, even when it is not
    // registered in this catalog. Never silently reinterpret its model.
    let competing_code=Regex::new(
        r"(?i)(?:^|[\s(/])(?:BD\s*[- ]*)?(?:[A-Z]{1,7}[- ]?\d+|\d{3}(?:-\d+)?)(?:\b|\s|$)"
    ).expect("independent model token");
    if competing_code.is_match(text) { return vec![]; }
    ps.iter().copied()
        .filter(|p| str_field(p,"category").contains("보면대")
            && str_field(p,"name").contains("지휘자")
            && str_field(p,"name").contains("보면대"))
        .map(|p|Entry{p,priority:1,span:(m.start(),m.end())}).collect()
}
pub(crate) fn decide(text: &str, state: &Value, client_id: &str) -> Decision {
    decide_with_trace(text,state,client_id,None)
}
pub(super) fn decide_with_trace(text:&str,state:&Value,client_id:&str,mut trace:Option<&mut Vec<Value>>)->Decision{
    let ps = products(state);
    let verified = verified_bases(&ps);
    let entries = name_entries(text, &ps, &verified,&mut trace);
    // parser.ts:487-510 filters brand/catalog and DEMO before options.
    let (mut entries,known_brand) =
        filter_business_candidates(text,state,&ps,entries);
    trace_entries(&mut trace,"business_catalog_filter",&entries);
    let aliases = alias_entries(text, state, client_id, &ps);
    if let Some(out)=trace.as_deref_mut(){
        out.push(json!({"stage":"matched_aliases","count":aliases.len(),"truncated":aliases.len()>128,
            "candidates":aliases.iter().take(128).map(|(a,p)|json!({
                "productId":str_field(p,"id"),"aliasId":str_field(a,"id"),
                "clientScoped":a["clientId"].is_string()})).collect::<Vec<_>>()}));
    }
    let mut hint=code_color_hint(text,&ps);
    if let Some((base,_,term))=hint.clone() {
        // parser.ts::protectedCode guards against reinterpreting the final
        // letter of a *registered full model* as a color suffix. Examples:
        // 700W (wide), 719B, 930BK, 801C. Once protected, that hint
        // must not be carried forward as an explicit requested color.
        let protected=entries.iter().any(|e|model_names(e.p).iter()
            .any(|name|model_key(name)==model_key(&term)));
        if protected {
            hint=None;
        } else {
            let mut matched:Vec<Entry<'_>>=ps.iter().copied()
                .filter(|p|model_names(p).iter()
                    .any(|name|model_key(name)==model_key(&base)))
                .map(|p|Entry{
                    p,priority:2,span:find_term(text,&term,false)
                        .unwrap_or((0,text.len()))
                }).collect();
            trace_entries(&mut trace,"color_code_matches",&matched);
            if matched.iter().any(|e|!variants(e.p).iter()
                .any(|v|str_field(v,"source")=="DEMO")) {
                matched.retain(|e|!variants(e.p).iter()
                    .any(|v|str_field(v,"source")=="DEMO"));
            }
            entries=matched;
        }
    }
    // Only use meaning-equivalent wording if literal names and aliases
    // found nothing. Ambiguous variants remain unresolved by the usual
    // candidate-selection safeguards.
    trace_entries(&mut trace,"color_code_filter",&entries);
    let mut semantic_name=false;
    if entries.is_empty() && aliases.is_empty() {
        let semantic=conductor_semantic_entries(text,&ps);
        if !semantic.is_empty() {
            trace_entries(&mut trace,"semantic_name_matches",&semantic);
            let (filtered,_)=filter_business_candidates(text,state,&ps,semantic);
            if !filtered.is_empty() {
                entries=filtered;
                semantic_name=true;
            }
        }
    }
    let mut branded_category=false;
    if entries.is_empty() && aliases.is_empty() && known_brand {
        entries=ps.iter().copied().filter(|p|
            brand_matches(text,&brand_words(state,p)))
            .map(|p|Entry{p,priority:0,span:(0,text.len())}).collect();
        branded_category=true;
    }
    trace_entries(&mut trace,"semantic_or_branded_fallback",&entries);
    let proof = qualifiers(text);
    let qualifier_conflict=conflicting_qualifiers(&proof);
    entries.retain(|e| qualifies(e.p, &proof, &verified));
    trace_entries(&mut trace,"qualifier_filter",&entries);
    let code_entries = entries.clone();
    let first_alias = aliases.first().map(|(a,_)| *a);
    if entries.is_empty() {
        if let Some((_,p)) = aliases.first() {
            if qualifies(p, &proof, &verified) {
                entries = vec![Entry {
                    p, priority: 1,
                    span: find_term(text, str_field(first_alias.unwrap(), "text"), true)
                        .unwrap_or((0, text.len())),
                }];
            }
        }
    }
    trace_entries(&mut trace,"alias_fallback",&entries);
    let explicit = hint.as_ref().map(|(_,color,_)|color.clone())
        .or_else(||written_color(text));
    let color_matched: Vec<_> = if let Some(ref color) = explicit {
        entries.iter().filter(|e| variants(e.p).iter()
            .any(|v| norm_color(str_field(v, "color")) == *color))
            .cloned().collect()
    } else { entries.clone() };
    trace_entries(&mut trace,"explicit_color_filter",&color_matched);
    let alias_selected = first_alias.and_then(|a| {
        entries.iter().find(|e| str_field(e.p, "id") == str_field(a, "productId"))
    }).cloned();
    let mut chosen = if color_matched.len() == 1 { color_matched.first().cloned() }
        else if explicit.is_none() { alias_selected } else { None };
    let exact: Vec<_> = if let Some(ref color) = explicit {
        code_entries.iter().filter(|e| variants(e.p).iter()
            .any(|v| norm_color(str_field(v, "color")) == *color))
            .cloned().collect()
    } else if let Some(ref pick) = chosen {
        code_entries.iter().filter(|e| str_field(e.p,"id") == str_field(pick.p,"id"))
            .cloned().collect()
    } else { code_entries.clone() };
    let alias_conflict = (!exact.is_empty() && aliases.iter()
            .any(|(_,p)| !exact.iter().any(|e| str_field(e.p,"id") == str_field(p,"id"))))
        || (exact.is_empty() && {
            let priority = first_alias.and_then(|a| a.get("clientId").and_then(Value::as_str)).is_some();
            let ids: HashSet<_> = aliases.iter().filter(|(a,_)|
                a.get("clientId").and_then(Value::as_str).is_some() == priority)
                .map(|(_,p)| str_field(p,"id")).collect();
            ids.len() > 1
        });
    let category_mismatch = chosen.as_ref().map_or(false, |pick| {
        let declared = category(text);
        let item_type = category(&format!("{} {} {}",str_field(pick.p,"category"),
            str_field(pick.p,"subtitle"),str_field(pick.p,"name")));
        declared.is_some() && item_type.is_some() && declared != item_type
    });
    let broad = category(text).is_some() && code_entries.is_empty() && aliases.is_empty();
    let mut available = if alias_conflict {
        let mut seen = HashSet::new();
        exact.iter().map(|e| e.p).chain(aliases.iter().map(|(_,p)| *p))
            .filter(|p| seen.insert(str_field(p,"id")))
            .map(|p| Entry {
                p, priority: 0, span: find_term(text, lead(p), true).unwrap_or((0,text.len()))
            }).collect::<Vec<_>>()
    } else if category_mismatch { exact.clone() } else { entries };
    if alias_conflict || category_mismatch || broad || branded_category || qualifier_conflict { chosen = None; }
    if broad { available.clear(); }
    if chosen.is_none() && available.len() == 1 && !alias_conflict
        && !category_mismatch && !broad && !branded_category && !qualifier_conflict && color_matched.len() == 1 {
        chosen = available.first().cloned();
    }
    trace_entries(&mut trace,"final_available",&available);
    let ambiguous = chosen.is_none() && available.len() > 1;
    let kind = if alias_conflict { "CONFLICT" }
        else if category_mismatch { "CATEGORY_CONFLICT" }
        else if broad || ambiguous || branded_category { "CATEGORY" }
        else if let Some(ref e) = chosen {
            if semantic_name {
                "SEMANTIC_NAME"
            } else if exact.iter().any(|x| str_field(x.p,"id") == str_field(e.p,"id")) {
                "NAME"
            } else if first_alias.and_then(|a| a.get("clientId").and_then(Value::as_str)).is_some() {
                "CLIENT_ALIAS"
            } else { "COMMON_ALIAS" }
        } else { "UNKNOWN" };
    let reason = if alias_conflict { Some("정확한 SKU 후보와 등록된 별칭이 서로 다른 상품을 가리킵니다.".to_owned()) }
        else if category_mismatch { Some("모델 코드와 상품 종류의 일치 여부를 직원이 확인해야 합니다.".to_owned()) }
        else if broad || branded_category { Some("상품 종류는 인식했지만 모델명은 확정되지 않았습니다. 실제 상품을 선택하세요.".to_owned()) }
        else if ambiguous { Some("여러 독립 상품 후보가 있습니다. 실제 상품과 색상을 확인하세요.".to_owned()) }
        else if chosen.is_none() { Some("등록된 모델명 또는 별칭과 일치하지 않습니다.".to_owned()) }
        else { None };
    let mut names = HashMap::<String, Value>::new();
    for e in &available {
        let p = e.p;
        names.entry(str_field(p,"id").to_owned()).or_insert_with(|| json!({
            "id": format!("candidate:{}",str_field(p,"id")),
            "field": "product", "value": str_field(p,"id"), "label": str_field(p,"name"),
            "start": text[..e.span.0].encode_utf16().count(),
            "end": text[..e.span.1].encode_utf16().count(),
            "productId": str_field(p,"id"),
            "variantId": variants(p).first().map(|v| str_field(v,"id"))
        }));
    }
    let candidates: Vec<_> = available.iter()
        .filter_map(|e| names.remove(str_field(e.p,"id"))).take(20).collect();
    let (product_id, variant_id, name, origin) = if let Some(ref entry) = chosen {
        let p = entry.p;
        // domain.ts::itemFromProduct: only a single sale variant without
        // a configuration requirement may be selected automatically.
        let variants=variants(p);
        let selectable=variants.len()==1
            && p.get("requiresOption").and_then(Value::as_bool)!=Some(true);
        let selected_variant=if selectable {
            variants.into_iter().find(|v|
                explicit.as_ref().map_or(true,|c|
                    norm_color(str_field(v,"color"))==*c))
        } else {None};
        (Some(str_field(p,"id").to_owned()),
            selected_variant.map(|v| str_field(v,"id").to_owned()),
            str_field(p,"name").to_owned(),
            str_field(p,"origin").to_owned())
    } else { (None,None,text.trim().to_owned(),"FACTORY".to_owned()) };
    if let Some(out)=trace.as_deref_mut(){
        out.push(json!({"stage":"final_decision","selectedProductId":product_id,
            "selectedVariantId":variant_id,"aliasConflict":alias_conflict,
            "categoryMismatch":category_mismatch,"broadCategory":broad,
            "brandedCategory":branded_category,"ambiguous":ambiguous,
            "explicitQualifierConflict":qualifier_conflict,"reasonCode":kind}));
    }
    let span = chosen.as_ref().map(|e|e.span)
        .or_else(|| available.first().map(|e|e.span));
    Decision {
        product_id, variant_id, name, color: explicit.unwrap_or_default(),
        origin, kind: kind.to_owned(),
        term: first_alias.map(|a| str_field(a,"text").to_owned())
            .unwrap_or_else(|| span.map(|(a,b)| text[a..b].to_owned())
                .unwrap_or_else(|| text.to_owned())),
        reason, candidates, alias_id: first_alias.map(|a|str_field(a,"id").to_owned()),
        span,
    }
}
pub(crate) fn decision_json(decision: &Decision) -> Value {
    json!({
        "productId": decision.product_id, "variantId":decision.variant_id,
        "name": decision.name, "color": decision.color,
        "match": { "kind":decision.kind, "term":decision.term,
            "productIds": decision.candidates.iter().filter_map(|c|
                c.get("productId").and_then(Value::as_str)).collect::<Vec<_>>(),
            "aliasId": decision.alias_id
        },
        "recognitionReason":decision.reason,
        "productCandidates":decision.candidates,
        "evidence":decision.span.map(|(start,end)| json!({"start":start,"end":end}))
    })
}

#[cfg(test)]
mod option_boundary_tests {
    use super::*;

    #[test]
    fn type_evidence_cannot_bridge_model_code_boundary() {
        for (input,type_key,other) in [
            ("BD-LP-WA A타입","a타입","b타입"),
            ("BD-LP-WA B타입","b타입","a타입"),
            ("BD-LP-WA A 타입","a타입","b타입"),
            ("BD-LP-WA B 타입","b타입","a타입"),
            ("BD-LP-WA 타입A","a타입","b타입"),
            ("BD-LP-WA 타입B","b타입","a타입"),
            ("BD-LP-WA 타입: B","b타입","a타입"),
        ] {
            let result=qualifiers(input);
            assert!(result.contains(type_key),"type missing: {input}");
            assert!(!result.contains(other),"false type created: {input}");
        }
        // With no separator, an A suffix in the model code must not be
        // converted into an option token at the start of the next word.
        let compact=qualifiers("BD-LP-WA타입B");
        assert!(!compact.contains("a타입"));
        assert!(!compact.contains("b타입"));
        assert_eq!(qualifiers("709 무타공").contains("비타공상판"),true);
    }

    #[test]
    fn product_candidate_comparison_preserves_priority_length_and_ties(){
        let a=json!({"id":"a","name":"AAA / 블랙"});
        let b=json!({"id":"b","name":"BBB / 화이트"});
        let entries=||vec![Entry{p:&a,priority:2,span:(0,3)},Entry{p:&b,priority:2,span:(0,3)}];
        let mut records=Vec::new();
        let result=strongest("AAA 화이트",entries(),&mut Some(&mut records));
        assert_eq!(result.len(),1);
        assert_eq!(str_field(result[0].p,"id"),"b");
        assert_eq!(records[0]["candidates"][0]["optionScore"],0);
        assert_eq!(records[0]["candidates"][1]["survivesComparison"],true);
        assert_eq!(strongest("AAA",entries(),&mut None).len(),2);
    }
    #[test]
    fn conflicting_explicit_qualifiers_cannot_qualify_any_product(){
        let proof=qualifiers("가방 포함 / 가방 미포함");
        assert!(conflicting_qualifiers(&proof));
        let p=json!({"name":"가방 포함 / 가방 미포함"});
        assert!(!qualifies(&p,&proof,&HashSet::new()));
        assert!(!conflicting_qualifiers(&qualifiers("가방 미포함 / 바퀴 장착")));
        assert!(conflicting_qualifiers(&qualifiers("A 타입 / B 타입")));
    }
    #[test]
    fn b_type_is_not_black_but_true_color_abbreviations_remain_valid() {
        let state:Value=serde_json::from_str(include_str!(
            "../../../resources/state-seed.json"
        )).expect("catalog");
        let ps=products(&state);
        for input in ["BD-LP-WA B타입","BD-LP-WA B 타입",
            "BD-LP-WA B형","BD-LP-WA B Type",
            "709B타입","709 B 타입"] {
            assert!(code_color_hint(input,&ps).is_none(),
                "type designation cannot become a color code: {input}");
        }
        for input in ["709 B","709BK","BD-LP-WA BK"] {
            assert_eq!(
                code_color_hint(input,&ps).map(|(_,color,_)|color),
                Some("Black".to_owned()),"real color code must work: {input}"
            );
        }
    }
}
