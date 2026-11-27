//! Standalone recognition engine. Integration with Tauri and price/freight lives elsewhere.
//! All spans exposed to TypeScript use UTF-16 offsets, not Rust byte positions.
mod model;
mod quantity;
mod corrections;
mod observations;
mod learning_context;
mod ts_semantics;
#[path = "../postcode.rs"]
mod postcode;

use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
struct Evidence {
    kind: &'static str,
    value: String,
    start: usize,
    end: usize,
    line: usize,
    paragraph: usize,
    reason: &'static str,
    recipient: Option<usize>,
    item_id: Option<String>,
}

#[derive(Clone, Debug)]
struct Line<'a> {
    text: &'a str,
    start: usize,
    number: usize,
    paragraph: usize,
}

#[derive(Clone, Debug)]
struct ProductHit {
    evidence: usize,
    source_start: usize,
    line: usize,
    raw: String,
    decision: Value,
    quantities: Vec<usize>,
    common: bool,
    each: bool,
}

/// The only public entrypoint. Returns an error instead of guessing on malformed input.
pub fn execute(operation: &str, payload: Value) -> Result<Value, String> {
    if !payload.is_object() { return Err("payload must be a JSON object".into()); }
    match operation {
        "analyze_order" => {
            let raw = required_string(&payload, "raw")?;
            let state = required_state(&payload)?;
            // Production reception never resolves a synthetic catalog or a
            // product containing synthetic variants. Do not rely on React
            // removing those candidates before crossing the native boundary.
            let mut reception = state.clone();
            reception["products"].as_array_mut()
                .ok_or("state products must be an array")?
                .retain(|product| {
                    product.get("catalog").and_then(Value::as_str)!=Some("DEMO") &&
                    !product.get("variants").and_then(Value::as_array)
                        .is_some_and(|variants|variants.iter().any(|variant|
                            variant.get("source").and_then(Value::as_str)==Some("DEMO")))
                });
            if payload["collectOwnerTrace"]==true || payload["collectCandidateTrace"]==true {
                analyze_with_owner_trace(raw,&reception,payload.get("client"),
                    payload["collectOwnerTrace"]==true,payload["collectCandidateTrace"]==true)
            }else{
                analyze(raw,&reception,payload.get("client"))
            }
        }
        "match_products" => {
            let state = required_state(&payload)?;
            let text = payload.get("text").or_else(|| payload.get("raw"))
                .and_then(Value::as_str).ok_or("match_products requires text or raw string")?;
            let client = client_id(payload.get("client"));
            let decision = model::decide(text, state, client);
            let mut result=model::decision_json(&decision);
            if let Some(span)=result.get_mut("evidence") {
                if let (Some(start),Some(end))=(span["start"].as_u64(),span["end"].as_u64()) {
                    let start=start as usize;
                    let end=end as usize;
                    if text.is_char_boundary(start) && text.is_char_boundary(end) {
                        *span=json!({"start":utf16(text,start),"end":utf16(text,end)});
                    }
                }
            }
            Ok(result)
        }
        "reanalyze_order" => reanalyze(&payload),
        "extract_correction_candidates" => corrections::extract(&payload),
        "compare_confirmation_observations" => observations::compare(&payload),
        "evaluate_shadow_predictions" => observations::evaluate_shadow(&payload),
        "calibrate_legacy_scores" => observations::calibrate_legacy_scores(&payload),
        "calibrate_shadow_candidates" => observations::calibrate_shadow_candidates(&payload),
        "learn_blend_weights" => observations::learn_blend_weights(&payload),
        "learn_field_blend_weights" => observations::learn_field_blend_weights(&payload),
        "run_field_blend_shadow" => observations::run_field_blend_shadow(&payload),
        "compare_field_models" => observations::compare_field_models(&payload),
        "run_blend_shadow" => observations::run_blend_shadow(&payload),
        "replay_owner_recommendation" => replay_owner_recommendation(&payload),
        "evaluate_combined_confirmation" => observations::evaluate_combined_confirmation(&payload),
        "compare_registered_models" => observations::compare_registered_models(&payload),
        "project_owner_contexts" => learning_context::lookup(&payload),
        "summarize_owner_statistics" => learning_context::statistics(&payload),
        "summarize_learning_confirmation" => learning_context::summarize_confirmation(&payload),
        _ => Err(format!("unknown recognition operation: {operation}")),
    }
}

// Replay a single checked recommendation against the same parser and
// catalog. Do not accept caller-supplied row replacements or modify orders.
fn replay_owner_recommendation(payload:&Value)->Result<Value,String>{
    let raw=required_string(payload,"raw")?;
    let state=required_state(payload)?;
    let mut reception=state.clone();
    reception["products"].as_array_mut().ok_or("state products must be an array")?
        .retain(|p|p["catalog"]!="DEMO"&&!p["variants"].as_array().is_some_and(|a|a.iter().any(|v|v["source"]=="DEMO")));
    let client=payload.get("client");
    let first=analyze_with_owner_trace(raw,&reception,client,true,true)?;
    // Only the parser's own fields participate in identity comparison;
    // gateway provenance is separate from the parser output.
    let expected=&payload["initialAnalysis"];
    if first.as_object().is_none_or(|o|o.iter().any(|(k,v)|expected.get(k)!=Some(v))){
        return Err("귀속 재계산의 최초 분석 불일치".into());
    }
    let d=&payload["decision"];
    let (source,target,old_row,new_row,field)=observations::checked_owner_replay(raw,&first,d)?;
    let overrides=std::collections::BTreeMap::from([(source,target)]);
    let replay=analyze_with_owner_overrides(raw,&reception,client,false,false,&overrides)?;
    observations::verify_owner_replay(raw,&first,&replay,source,target,old_row,new_row,&field)?;
    Ok(json!({"status":"validated_counterfactual","recognition":replay,
        "decisionChanged":false,"eligibleForAutomaticDecision":false}))
}
fn required_string<'a>(obj: &'a Value, key: &str) -> Result<&'a str, String> {
    obj.get(key).and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}
fn required_state(obj: &Value) -> Result<&Value, String> {
    let state = obj.get("state").ok_or("state is required")?;
    if !state.is_object() || !state.get("products").map_or(false, Value::is_array)
        || !state.get("aliases").map_or(false, Value::is_array) {
        return Err("state must contain products and aliases arrays".into());
    }
    Ok(state)
}
fn client_id(client: Option<&Value>) -> &str {
    client.and_then(|v| v.get("id")).and_then(Value::as_str).unwrap_or("")
}
fn utf16(raw: &str, byte_index: usize) -> usize {
    raw[..byte_index].encode_utf16().count()
}
/// Copy source order-evidence.ts reason contracts rather than exporting
/// internal Rust recognition diagnostics into the UI evidence list.
fn ts_evidence_reason(raw: &str,e:&Evidence) -> String {
    let start=raw[..e.start].rfind('\n').map_or(0,|i|i+1);
    let end=e.end+raw[e.end..].find('\n').unwrap_or(raw.len()-e.end);
    let line=raw[start..end].trim();
    let base=match (e.kind,e.reason) {
        ("product",_)=>"상품 코드 / 별칭 / 옵션 인식 결과; 귀속은 별도 판정",
        ("quantity","상품 행에 명시된 수량") if
            regex::Regex::new(r"(?i)\b(?:BC[\s-]*105)\s+1\s*$")
                .expect("bare unitted BC-105 source")
                .is_match(line)
            =>"같은 상품 행 또는 독립 수량 구간",
        ("quantity","독립 수량 원문; 상품 연결은 별도 판정")
          |("quantity","상품 행에 명시된 수량")
            =>"원문 전체 수량 후보 독립 추출",
        ("address","배송주소 원문 구간")
            =>"해당 원문 행에서 독립 추출",
        ("name","주소 앞 괄호의 수취인")
            =>"배송주소 앞 괄호 안 수취인 이름",
        ("name","직함 앞 독립 수취인 이름")
            =>"수취 기관 또는 직함을 포함한 이름",
        ("name","상품 수량 뒤의 독립 수취인명")
            =>"상품 수량 뒤의 독립 수취인명",
        ("name","수취인 복수 표기 이름")=>"수취인 복수 표기 이름",
        ("name","수취 기관 또는 직함을 포함한 이름")
            =>"수취 기관 또는 직함을 포함한 이름",
        ("name","독립 수취인 또는 기관명") if
            regex::Regex::new(
                r"^[가-힣A-Za-z0-9 ()]{2,30}(?:학교|교회|성당|병원|학원|월드|목사)$"
            ).expect("institution").is_match(line)
            =>"수취 기관 또는 직함을 포함한 이름",
        ("name","수취인명과 부연 문구") if
            regex::Regex::new(r"^[가-힣]{2,5}\s+[가-힣]{2,5}님$")
                .expect("multiple recipient name").is_match(line)
            =>"수취인 복수 표기 이름",
        ("name","이름과 연락처가 같은 행") if
            regex::Regex::new(r"^[가-힣]{2,5}(?:님)?\s+0[\d\s().-]{3,}$")
                .expect("inline contact").is_match(line)
            =>"이름과 연락처 모양이 같은 행에 있음",
        ("name","명시적 수취인 이름") if
            regex::Regex::new(
                r"(?:수취인명|수령자)\s*[:：]?\s*[가-힣]{2,10}\s*(?:연락처|전화|휴대)"
            ).expect("explicit name").is_match(line)
            =>"명시적 수취인 이름",
        ("name","명시적 수취인 이름")=>"명시적 수취인 이름",
        ("name","수취인 필드 경계에서 원문 전체 보존")=>"수취인 필드 경계에서 원문 전체 보존",
        ("name",_)=>"같은 행의 대안 후보; 직원 확인 필요",
        ("phone","연락처 원문 구간")
          |("mobile","연락처 원문 구간")=>{
            if line.starts_with("TEL") || line.starts_with("Tel") {
                "전화번호 구간 독립 추출"
            } else if regex::Regex::new(r"^[가-힣]{2,5}(?:님)?\s+0[\d\s().-]{3,}$")
                .expect("same line phone").is_match(line) {
                "같은 행의 연락처 원문 보존; 유효성은 별도 검증"
            } else if regex::Regex::new(
                r"(?i)^(?:(?:휴대폰|핸드폰|전화(?:번호)?|연락처|TEL)\s*[:：]?\s*)?\d[\d\s().-]*$"
            ).expect("standalone phone").is_match(line) {
                "연락처 모양의 원문 보존; 유효성은 별도 검증"
            } else {"전화번호 구간 독립 추출"}
        },
        _=>e.reason,
    };
    if e.recipient.is_some() && e.kind!="product" && e.kind!="quantity" {
        format!("{base} / 배송 건 원문 경계와 연결")
    } else {base.to_owned()}
}

fn e_json(raw: &str, e: &Evidence) -> Value {
    let start = utf16(raw, e.start);
    let end = utf16(raw, e.end);
    let mut result = json!({
        "id": format!("{}:{}:{}", e.kind, start, end),
        "text": &raw[e.start..e.end],
        "start": start, "end": end,
        "line": e.line, "paragraph": e.paragraph,
        "kind": e.kind, "value": e.value, "reason": ts_evidence_reason(raw,e),
    });
    if let Some(recipient) = e.recipient {
        result["recipientId"] = json!(format!("recognition:recipient:{recipient}"));
    }
    if let Some(id) = &e.item_id { result["itemId"] = json!(id); }
    result
}
fn add_evidence(events: &mut Vec<Evidence>, e: Evidence) -> usize {
    if let Some(pos) = events.iter().position(|p|
        p.kind == e.kind && p.start == e.start && p.end == e.end
    ) { return pos; }
    events.push(e);
    events.len() - 1
}
fn add_match(events: &mut Vec<Evidence>, line: &Line<'_>, start: usize,
    end: usize, kind: &'static str, value: String, reason: &'static str) -> usize {
    add_evidence(events, Evidence {
        kind, value, start: line.start + start, end: line.start + end,
        line: line.number, paragraph: line.paragraph, reason,
        recipient: None, item_id: None,
    })
}
fn physical_lines(raw: &str) -> Vec<Line<'_>> {
    let regex = Regex::new(r"[^\r\n]+").expect("physical lines");
    let mut paragraph = 0;
    let mut line_number = 0;
    let mut previous = 0;
    // Whitespace-only lines are blank separators, not discarded evidence.
    let mut skipped_blank = false;
    regex.find_iter(raw).filter_map(|m| {
        let gap = &raw[previous..m.start()];
        let newlines = gap.bytes().filter(|b| *b == b'\n').count();
        line_number += newlines;
        previous = m.end();
        if m.as_str().trim().is_empty() {
            skipped_blank = true;
            return None;
        }
        if newlines > 1 || (skipped_blank && newlines > 0) { paragraph += 1; }
        skipped_blank = false;
        Some(Line { text: m.as_str(), start: m.start(), number: line_number,
            paragraph })
    }).collect()
}

// Source geometry stays independent of source-value interpretation. Preserve
// source spans and distinguish textual distance from empty-line separators.
#[derive(Debug, Default, PartialEq, Eq)]
struct InterveningLines {
    content_lines: usize,
    blank_lines: usize,
    max_consecutive_blank_lines: usize,
}
fn intervening_lines(raw: &str, earlier: usize, later: usize) -> InterveningLines {
    let mut result = InterveningLines::default();
    if later <= earlier + 1 { return result; }
    let mut current_blank_run = 0usize;
    for line in raw.split('\n').skip(earlier + 1).take(later - earlier - 1) {
        if line.trim().is_empty() {
            result.blank_lines += 1;
            current_blank_run += 1;
            result.max_consecutive_blank_lines =
                result.max_consecutive_blank_lines.max(current_blank_run);
        } else {
            result.content_lines += 1;
            current_blank_run = 0;
        }
    }
    result
}
fn phone_regex() -> Regex {
    Regex::new(r"(?i)(0(?:50\d|1[016789]|2|[3-6]\d|70|80))[\s().-]*(\d{3,4})[\s().-]*(\d{4})")
        .expect("phone")
}
fn address_begin(line: &str) -> Option<usize> {
    // A bare prefix is not enough: "배송지메모: 상호" is NOT an address.
    // Require a boundary (colon/space) after the complete address label.
    let prefix = Regex::new(r"^\s*(?:주소|배송지(?:\s*주소)?|배송주소|받는곳|수취인\s*주소)(?:\s*[:：]\s*|\s+)")
        .expect("address prefix");
    if let Some(m)=prefix.find(line) {
        // parseLegacy.ts detects the province after an optional five-digit
        // postal marker. An address label must not turn (11806) into the
        // beginning of the actual shipping address/source span.
        // A labelled address can start with a modern five-digit postcode
        // in square brackets, parentheses or plain form. Some older source
        // documents carry six digits inside brackets; neither belongs in the
        // delivery-address value. Never consume the first five of six bare
        // digits or change parenthesized apartment/building details.
        let marker=Regex::new(r"^(?:\[\d{5,6}\]|\(\d{5,6}\)|\d{5}(?:\s+|$))\s*")
            .expect("postal marker");
        let skipped=marker.find(&line[m.end()..]).map_or(0,|v|v.end());
        let begin=m.end()+skipped;
        // An address HEADING must not be promoted to an address value.
        // "배송지 주소" matches "배송지 " + "주소" in the old prefix
        // regexp; accepting that remainder then polluted an otherwise
        // complete real address and made an extra address candidate.
        // Keep actual partial address lines (and original source offsets).
        let value=line[begin..].trim();
        if value.is_empty() || matches!(value,
            "주소"|"배송지"|"배송주소"|"배송지 주소"|"수취인주소"|"받는곳"
        ) { return None; }
        return Some(begin);
    }
    let address = Regex::new(concat!(
        r"(?:서울(?:특별시)?|부산(?:광역시)?|대구(?:광역시)?|인천(?:광역시)?|광주(?:광역시)?|",
        r"대전(?:광역시)?|울산(?:광역시)?|세종(?:특별자치시)?|경기(?:도)?|강원(?:특별자치도|도)?|",
        r"충청북도|충청남도|전라북도|전북특별자치도|전라남도|경상북도|경상남도|",
        r"충북|충남|전북|전남|경북|경남|제주(?:특별자치도|도)?|",
        r"[가-힣]{2,}(?:시|군|구|읍|면|동|리))\s+",
        r"|[가-힣A-Za-z0-9.]+(?:대로|로|길)\s*\d+(?:-\d+)?"
    )).expect("address");
    address.find(line).map(|m| m.start())
}
// A delivery label may contain any arrangement of department, contact,
// office/floor and bracketed remarks. Treat the entire pre-address field as
// one receiver label; never infer that only the parenthesized person is the
// shipping name. This function finds field BOUNDARIES, not a name format.
// An independent address must already have been recognized by the caller.
fn trim_span(text:&str,start:usize,end:usize)->Option<(usize,usize)>{
    let field=&text[start..end];
    let begin=start+field.len()-field.trim_start().len();
    let end=start+field.trim_end().len();
    (begin<end).then_some((begin,end))
}
fn recipient_label()->Regex {
    Regex::new(r"^\s*(?:수취인(?:명)?|수하인(?:명)?|수령자|받(?:는|으시는)\s*(?:분|사람)|성함|이름|(?i:recipient|consignee))(?:(?:\s*[:：]\s*|\s+)|$)")
        .expect("recipient field label")
}
// An explicit field is authoritative. Recognize the next actual field
// boundary, never impose a character, length, surname, or title constraint.
fn explicit_recipient_span(line:&str,label:&Regex,phone:&Regex)->Option<(usize,usize)>{
    let header=label.find(line)?;
    let start=header.end();
    let tail=&line[start..];
    // A compound heading without a separator (e.g. 수취인 주소) is
    // an address/contact field, not a recipient-name label. A colon
    // explicitly separating the name permits any following value.
    if !header.as_str().contains([':', '：']) &&
        Regex::new(r"^(?:주소|연락처|전화번호|휴대전화)(?:\s|[:：]|$)")
            .expect("compound field heading").is_match(tail){
        return None;
    }
    let mut end=line.len();
    let fields=Regex::new(r"(?i)(?:\s+|\s*[|]\s*)(?:연락처(?:\([^)]*\))?|전화(?:번호)?|휴대(?:전화|폰)?|번호|주소|배송지(?:\s*주소)?|배송주소|받는곳|TEL|phone|address|배송메모|메모|비고)\s*[:：]\s*")
        .expect("explicit next field");
    if let Some(m)=fields.find(tail){end=start+m.start();}
    let address_field=Regex::new(r"\s+(?:주소|배송지(?:\s*주소)?|배송주소|받는곳)\s+")
        .expect("address field boundary");
    for m in address_field.find_iter(tail){
        if address_begin(&tail[m.end()..]).is_some(){
            end=end.min(start+m.start());
            break;
        }
    }
    let contact=Regex::new(r"(?i)\s+(?:연락처(?:\([^)]*\))?|전화(?:번호)?|휴대(?:전화|폰)?|번호|TEL|phone)\s*[:：]?\s*$")
        .expect("contact field boundary");
    // A standalone phone-shaped value after an explicit label is still a
    // name; only a separate phone after a nonempty value establishes a field.
    if let Some(m)=phone.find(tail){
        let prefix=&tail[..m.start()];
        let cut=contact.find(prefix).map_or(m.start(),|c|c.start());
        if !tail[..cut].trim().is_empty(){end=end.min(start+cut);}
    }
    trim_span(line,start,end)
}
// Exclusions identify other document fields, not invalid name spellings.
fn other_recipient_field(text:&str)->bool{
    let text=text.trim();
    let heading=text.trim_end_matches([':', '：']).trim();
    // Separator-only tails such as ')' or '>' after a quantity are
    // structural punctuation, not unlabelled recipients.
    !heading.chars().any(char::is_alphanumeric)
    || matches!(heading,"주소"|"배송지"|"배송지 주소"|"배송주소"|"받는곳"|"수취인주소"|
        "수취인 주소"|"배송 주소"|"배송지주소"|"배송메모"|"배송 요청"|"배송 요청사항"|
        "메모"|"비고"|"연락처"|"전화번호"|"품목"|"수량"|"상품명"|"모델명"|"수령정보"|
        "ㅇ수령정보"|"ㅇ품목"|"발주서"|"발 주 서"|"공통"|"모두"|"전부"|"전원"|
        "민판"|"무타공"|"비타공"|"선불"|"착불"|"택배"|"반도"|"반도스탠드")
    || is_money_or_header(text)
    || recipient_label().is_match(text)
    || Regex::new(r"(?i)^(?:주소|배송지(?:\s*주소)?|배송주소|받는곳|수취인\s*주소|연락처|전화(?:번호)?|휴대(?:전화|폰)?|번호|TEL|배송\s*(?:메모|메시지|요청)|메모|비고|모델명|상품명|품목|수량|보내는\s*(?:분|사람|곳)|송하인(?:명)?|발주처|수주처)(?:\s*[:：]|\s+|$)")
        .expect("other field label").is_match(text)
    || Regex::new(r"^(?:[■@*]+|택배\s*(?:선불|착불)|배송\s*(?:전|후)|(?:오늘|내일)\s*(?:발송|배송)|(?:빠른|당일|선)\s*(?:출고|발송))|(?:부탁(?:드려요|드립니다|합니다)?[.!~]*$|주세요[.!~]*$|감사합니다[.!~]*$)")
        .expect("document instruction").is_match(text)
}
fn composite_name_before_address(prefix: &str) -> Option<(usize,usize)> {
    let postal=Regex::new(
        r"(?:\s+(?:\d{5}|\(\d{5,6}\)|\[\d{5,6}\])|(?:\(\d{5,6}\)|\[\d{5,6}\]))\s*$"
    ).expect("trailing postal marker");
    let end=postal.find(prefix).map_or(prefix.len(),|m|m.start());
    // Explicit recipient fields were already handled by the caller.
    // Do not reinterpret rejected compound labels as name values here.
    let (start,end)=trim_span(prefix,0,end)?;
    let candidate=&prefix[start..end];
    if other_recipient_field(candidate) ||
        !quantity::extract(candidate).is_empty() ||
        candidate.contains('|') {return None;}
    Some((start,end))
}
// Weak hints for incomplete unlabelled documents only; never used to
// validate an explicit or structurally bounded recipient value.
fn weak_recipient_hint(text: &str) -> bool {
    ts_semantics::person(text) ||
    Regex::new(r"(?:학교|교회|성당|병원|학원|월드|도서관|회관|목사|선생님|님)$")
        .expect("weak recipient hint").is_match(text)
}
fn delivery_boundary(line:&str,phone:&Regex)->bool{
    address_begin(line).is_some() || phone.is_match(line)
}
fn unlabelled_recipient_span(
    line:&str,address:Option<usize>,phone:&Regex,near_delivery:bool,state:&Value,client_id:&str,
    mut reject:impl FnMut(usize,usize,&str)
)->Option<(usize,usize)>{
    let contact=phone.find(line).map(|m|m.start());
    let end=address.into_iter().chain(contact).min().unwrap_or(line.len());
    let prefix=&line[..end];
    // The contact label and table's slash separator are field boundaries.
    let contact_label=Regex::new(r"(?i)(?:^|\s+)(?:연락처(?:\s*\([^)]*\))?|전화(?:번호)?|휴대(?:전화|폰)?|번호|TEL)\s*[:：]?\s*$")
        .expect("contact label before phone");
    let end=if contact==Some(end){
        contact_label.find(prefix).map_or(end,|m|m.start())
    }else{end};
    let prefix=&line[..end];
    let bounded=address.is_some()||contact.is_some()||near_delivery;
    // A real transcribed table contains product columns before the
    // recipient remarks column. Do not treat bars inside names as columns.
    if bounded && prefix.matches('|').count()>=4 {
        let start=prefix.rfind('|')?+1;
        let tail=prefix[start..].trim_end();
        let tail=tail.strip_suffix('/').unwrap_or(tail);
        let Some((a,b))=trim_span(line,start,start+tail.len())else{
            reject(start,start+tail.len(),"empty_table_recipient");return None;
        };
        if !other_recipient_field(&line[a..b]){return Some((a,b));}
        reject(a,b,"table_other_field");return None;
    }
    // A known product followed by an explicit quantity is a separate field.
    // Preserve the ENTIRE suffix, including company and English names.
    for q in quantity::extract(prefix).into_iter().rev(){
        let before=&prefix[..q.end];
        let decision=model::decision_json(&model::decide(before,state,client_id));
        if !product_signal(before,&decision,state){reject(0,q.end,"no_product_before_quantity");continue;}
        let tail=&prefix[q.end..];
        let tail=tail.trim_start_matches(|c:char|c.is_whitespace()||c=='|'||c=='ㅣ');
        let a=q.end+prefix[q.end..].len()-tail.len();
        let Some((a,b))=trim_span(line,a,end) else{reject(a,end,"empty_suffix");continue};
        let candidate=&line[a..b];
        if other_recipient_field(candidate){reject(a,b,"suffix_other_field");continue;}
        if bounded || weak_recipient_hint(candidate){
            return Some((a,b));
        }
        reject(a,b,"suffix_without_recipient_context");
    }
    if address.is_some() || contact.is_some(){
        let Some((a,b))=composite_name_before_address(prefix)else{
            reject(0,prefix.len(),"composite_name_excluded");return None;
        };
        let candidate=&line[a..b];
        // Address/phone prefixes with no product quantity must not promote
        // a known model to a consignee. The explicit label path has priority.
        let decision=model::decision_json(&model::decide(candidate,state,client_id));
        if product_signal(candidate,&decision,state){reject(a,b,"product_signal_before_delivery");return None;}
        return Some((a,b));
    }
    let Some((a,b))=trim_span(line,0,line.len())else{reject(0,line.len(),"empty_line");return None;};
    let candidate=&line[a..b];
    if other_recipient_field(candidate){reject(a,b,"other_document_field");return None;}
    if !quantity::extract(candidate).is_empty(){reject(a,b,"quantity_signal");return None;}
    if !near_delivery && !weak_recipient_hint(candidate){reject(a,b,"insufficient_recipient_context");return None;}
    let decision=model::decision_json(&model::decide(candidate,state,client_id));
    if product_signal(candidate,&decision,state){reject(a,b,"product_signal");return None;}
    Some((a,b))
}
fn is_money_or_header(text: &str) -> bool { ts_semantics::ignored_document_line(text) }
fn product_signal(text: &str, decision: &Value, _state: &Value) -> bool {
    // From parser.ts:479-570: only registered identity, aliases, quantity,
    // a genuine unknown model, or a product category can form a product row.
    let t=text.trim();
    if ts_semantics::ignored_document_line(t) { return false; }
    if t.contains("배송비") || t.contains("택배비") || t.contains("운임")
        || t.starts_with("번호") || t.starts_with("TEL")
        || t.starts_with("연락처") || t.starts_with("휴대") { return false; }
    let kind=decision.pointer("/match/kind").and_then(Value::as_str).unwrap_or("UNKNOWN");
    if kind!="UNKNOWN" { return true; }
    // In the TS recognizer an unknown three-digit model is an item candidate,
    // but an unlabelled money or phone line is not.
    let model=Regex::new(r"(?i)(?:BD\s*)?\d{3}[A-Za-z0-9-]*")
        .expect("unknown model");
    if model.is_match(t) { return true; }
    if model::category(t).is_some() { return true; }
    let product_category=Regex::new(
        r"(?i)보면대|악보대|스탠드|뮤직트레이|스테레오바|홀더|카트|의자|가방|발판|거치대|SSB"
    ).expect("product category");
    product_category.is_match(t)
        && (quantity::extract(t).len()>0 || t.contains("BANDO") || t.contains("반도"))
}
fn source_json(raw: &str, e: &Evidence, recipient_id: &str, target_id: &str) -> Value {
    json!({
        "start": utf16(raw,e.start), "end": utf16(raw,e.end),
        "field": e.kind, "targetId": target_id,
        "recipientId":recipient_id, "text": &raw[e.start..e.end],
    })
}
fn address_info(value: &str) -> (String, String, Value) {
    let road = Regex::new(r"[로길]\s*\d+(?:-\d+)?").expect("road").is_match(value);
    let lot = Regex::new(r"[동리읍면]\s+(?:산\s*)?\d+(?:-\d+)?").expect("lot").is_match(value);
    let kind = if road { "ROAD" } else if lot { "LOT" } else { "UNKNOWN" };
    let status = if (road || lot) && !value.contains('#') && !value.contains('@') {
        "COMPLETE"
    } else { "INCOMPLETE" };
    let province = Regex::new(r"^(?:서울특별시|서울|부산광역시|부산|대구광역시|대구|인천광역시|인천|광주광역시|광주|대전광역시|대전|울산광역시|울산|세종특별자치시|세종|경기도|경기|강원특별자치도|강원도|강원|충청북도|충청남도|전북특별자치도|전라북도|전라남도|경상북도|경상남도|제주특별자치도|제주도|제주|충북|충남|전북|전남|경북|경남)")
        .expect("province").find(value)
        .filter(|m|value[m.end()..].chars().next().map_or(true,char::is_whitespace))
        .map(|m|m.as_str().to_owned());
    let mut parts = json!({});
    let rest = province.as_ref().and_then(|p|value.strip_prefix(p))
        .map(str::trim).unwrap_or(value);
    if let Some(p) = province { parts["province"] = json!(p); }
    for (field, pattern) in [
        ("city", r"(?:^|\s)([가-힣]+(?:시|군))(?:\s|$)"),
        ("district", r"(?:^|\s)([가-힣]+구)(?:\s|$)"),
        ("town", r"(?:^|\s)([가-힣0-9]+(?:읍|면|동|리))(?:\s|$)"),
    ] {
        if let Some(c)=Regex::new(pattern).expect("administrative component").captures(rest) {
            if let Some(m)=c.get(1){parts[field]=json!(m.as_str());}
        }
    }
    let road=Regex::new(r"([가-힣A-Za-z0-9.]+(?:대로|로|길))\s+(\d+(?:-\d+)?)")
        .expect("road component").captures(rest);
    if let Some(m)=road {
        parts["road"] = json!(&m[1]);
        parts["building"] = json!(&m[2]);
        if let Some(full)=m.get(0){
            let detail=rest[full.end()..].trim();
            if !detail.is_empty(){parts["detail"]=json!(detail);}
        }
    } else if let Some(town)=parts.get("town").and_then(Value::as_str) {
        // src/parser.ts::addressParts uses the detected *entire* town
        // as the lot prefix. A suffix such as "410동 804호" is not an
        // administrative lot without the matching earlier 읍/면/동/리.
        let pattern=format!(r"{}\s+(?:산\s*)?(\d+(?:-\d+)?)",regex::escape(town));
        if let Some(m)=Regex::new(&pattern).expect("town-bound lot number").captures(rest){
            parts["building"]=json!(&m[1]);
            if let Some(full)=m.get(0){
                let detail=rest[full.end()..].trim();
                if !detail.is_empty(){parts["detail"]=json!(detail);}
            }
        }
    }
    (kind.into(), status.into(), parts)
}
fn blank_recipient(id: &str, name: &str, anchor: Option<&Evidence>, raw: &str) -> Value {
    let mut value = json!({
        "id":id, "name":name, "address":"", "phone":"", "mobile":"", "note":"",
        "items":[], "sources":[], "candidates":[],
    });
    if let Some(a) = anchor {
        value["evidence"] = json!({"anchorId": format!("{}:{}:{}",a.kind,utf16(raw,a.start),utf16(raw,a.end)),
            "start":utf16(raw,a.start),"end":utf16(raw,a.end)});
    }
    value
}
fn owner_index(anchors: &[usize], events: &[Evidence], offset: usize) -> usize {
    anchors.iter().enumerate().rev().find(|(_, i)| events[**i].start <= offset)
        .map(|(idx,_)|idx).unwrap_or(0)
}
// Existing ownership is the baseline policy, not the learned decision.
// Record only the two adjacent anchors; no full-document candidate search.
fn recipient_owner_index(
    anchors: &[usize], events: &[Evidence], evidence_index: usize, raw: &str,
    trace: Option<&mut Vec<Value>>,
) -> usize {
    let candidate=&events[evidence_index];
    let previous=owner_index(anchors,events,candidate.start);
    if anchors.len()<2 || !matches!(candidate.kind,"address"|"phone"|"mobile") {
        return previous;
    }
    let Some(&next_anchor_index)=anchors.get(previous+1) else {return previous};
    let preceding=&events[anchors[previous]];
    let following=&events[next_anchor_index];
    if candidate.start<=preceding.start || candidate.start>=following.start {
        return previous;
    }

    // On ordinary analysis preserve the original short-circuit costs.
    // In observation mode collect features even for locally rejected
    // alternatives, without changing the legacy decision.
    let same_previous=candidate.paragraph==preceding.paragraph;
    let same_following=candidate.paragraph==following.paragraph;
    let paragraph_blocked=same_previous || !same_following;
    if trace.is_none() && paragraph_blocked {return previous;}
    let previous_has_delivery=events.iter().any(|e|
        e.start>preceding.start && e.start<candidate.start
            && e.paragraph==preceding.paragraph
            && matches!(e.kind,"address"|"phone"|"mobile")
    );
    if trace.is_none() && !previous_has_delivery {return previous;}
    let previous_gap=intervening_lines(raw,preceding.line,candidate.line);
    let following_gap=intervening_lines(raw,candidate.line,following.line);
    if trace.is_none() && following_gap.blank_lines>0 {return previous;}
    let next_distance=following.line.saturating_sub(candidate.line);
    if trace.is_none() && next_distance>3 {return previous;}
    let has_nearby_address=events.iter().any(|e|
        e.kind=="address" && e.paragraph==candidate.paragraph
            && e.line.abs_diff(candidate.line)<=4
    );
    let contact_without_address=matches!(candidate.kind,"phone"|"mobile") &&
        !has_nearby_address;
    let blocked=if paragraph_blocked {Some("paragraph_boundary")}
        else if !previous_has_delivery {Some("prior_delivery_missing")}
        else if following_gap.blank_lines>0 {Some("blank_before_next")}
        else if next_distance>3 {Some("next_distance_exceeds_3")}
        else if contact_without_address {Some("contact_without_nearby_address")}
        else {None};

    let preceding_distance=candidate.line.saturating_sub(preceding.line);
    // These are the original score components, not probabilities. No
    // score is fabricated for a candidate the original gates rejected.
    let previous_paragraph=if same_previous {80usize} else {0};
    let previous_distance=20usize.saturating_sub(preceding_distance.saturating_mul(5));
    let next_distance_score=20usize.saturating_sub(next_distance.saturating_mul(5));
    let address_score=if has_nearby_address {20usize} else {0};
    let blank_score=previous_gap.blank_lines.min(2)*5;
    let content_penalty=following_gap.content_lines.min(2)*5;
    let previous_score=previous_paragraph+previous_distance;
    let following_score=(80usize+next_distance_score+address_score+blank_score)
        .saturating_sub(content_penalty);
    let selected=if blocked.is_none() && following_score>=previous_score+30 {
        previous+1
    } else {previous};
    if let Some(trace)=trace {
        let location=|e:&Evidence|json!({
            "start":utf16(raw,e.start),"end":utf16(raw,e.end),
            "kind":e.kind
        });
        let gap=|g:&InterveningLines|json!({
            "contentLines":g.content_lines,"blankLines":g.blank_lines,
            "maxConsecutiveBlankLines":g.max_consecutive_blank_lines
        });
        let previous_components=blocked.is_none().then(||json!({
            "sameParagraph":previous_paragraph,"distance":previous_distance
        }));
        let following_components=blocked.is_none().then(||json!({
            "sameParagraph":80,"distance":next_distance_score,
            "nearbyAddress":address_score,"previousBlankLines":blank_score,
            "contentPenalty":content_penalty
        }));
        trace.push(json!({
            "source":location(candidate),
            "selectedAnchor":location(&events[anchors[selected]]),
            "legacyFirstBlockingReason":blocked,
            "legacyRequiredMargin":30,
            "previousHasDelivery":previous_has_delivery,
            "nearbyAddress":has_nearby_address,
            "candidates":[
                {"relation":"previous","anchor":location(preceding),
                 "sameParagraph":same_previous,"lineDistance":preceding_distance,
                 "gap":gap(&previous_gap),
                 "legacyScore":blocked.is_none().then_some(previous_score),
                 "legacyComponents":previous_components},
                {"relation":"next","anchor":location(following),
                 "sameParagraph":same_following,"lineDistance":next_distance,
                 "gap":gap(&following_gap),
                 "legacyScore":blocked.is_none().then_some(following_score),
                 "legacyComponents":following_components}
            ]
        }));
    }
    selected
}
/// Original TypeScript resolvePrice for the recognition Item shape.
/// The separate pricing migration remains authoritative for business rules.
fn legacy_recognition_price(v: &Value,client: Option<&Value>) -> Value {
    let Some(client)=client.filter(|c|
        c.get("id").and_then(Value::as_str).map_or(false,|id|!id.is_empty()))
    else {return Value::Null;};
    let tier=client.get("tier").and_then(Value::as_str).unwrap_or("");
    let price=if tier=="special" {
        let overrides=client.get("overrides");
        let variant_id=v.get("id").and_then(Value::as_str).unwrap_or("");
        let override_price=overrides.and_then(|o|o.get(variant_id))
            .filter(|x|!x.is_null());
        let prior=v.get("legacyIds").and_then(Value::as_array)
            .into_iter().flatten().find_map(|id|
                overrides.and_then(|o|o.get(id.as_str().unwrap_or("")))
                    .filter(|x|!x.is_null()));
        override_price.or(prior).or_else(||v.get("wholesale"))
    } else {v.get(tier)};
    let chosen=price.cloned().unwrap_or(Value::Null);
    if chosen.as_f64()==Some(0.0) {Value::Null} else {chosen}
}
/// Port of src/domain.ts::itemFromProduct and parser.ts single-sale-unit rule.
/// No inferred sale option or color without registered evidence.
fn apply_registered_item(
    item: &mut Value,state: &Value,client: Option<&Value>,explicit: &str
) {
    let Some(pid)=item.get("productId").and_then(Value::as_str) else {return;};
    let Some(p)=model::products(state).into_iter()
        .find(|p|p.get("id").and_then(Value::as_str)==Some(pid)) else{return;};
    let variants=p.get("variants").and_then(Value::as_array);
    let selectable=variants.map_or(false,|xs|xs.len()==1)
        && p.get("requiresOption").and_then(Value::as_bool)!=Some(true);
    let default_color=p.get("defaultColor").and_then(Value::as_str).unwrap_or("");
    let selected_color=if explicit.is_empty(){default_color}else{explicit};
    let color=model::norm_color(selected_color);
    let chosen=if selectable {
        variants.and_then(|vs|vs.iter().find(|v|
            model::norm_color(v.get("color").and_then(Value::as_str).unwrap_or(""))==color))
    } else {None};
    let mut name=p.get("name").and_then(Value::as_str)
        .or_else(||p.get("code").and_then(Value::as_str)).unwrap_or("")
        .trim_start_matches("반도스탠드").trim().to_owned();
    if variants.map_or(false,|v|v.len()>1) {
        if let Some(option)=chosen.and_then(|v|v.get("option")).and_then(Value::as_str)
            .filter(|s|!s.trim().is_empty()){
            name.push_str(" / ");
            name.push_str(option.trim());
        }
    }
    if variants.map_or(false,|xs|xs.iter().any(|v|
        v.get("source").and_then(Value::as_str)==Some("DEMO")))
        && !name.starts_with("[가상] ") {
        name=format!("[가상] {name}");
    }
    item["name"]=json!(name);
    item["color"]=json!(color);
    item["variantId"]=chosen.and_then(|v|v.get("id"))
        .cloned().unwrap_or(Value::Null);
    item["defaulted"]=json!(explicit.is_empty());
    item["url"]=json!(chosen.and_then(|v|v.get("url"))
        .and_then(Value::as_str).unwrap_or(""));
    item["origin"]=p.get("origin").cloned()
        .unwrap_or(json!("FACTORY"));
    if let Some(catalog)=p.get("catalog"){item["catalog"]=catalog.clone();}
    item["unitPrice"]=chosen.map_or(Value::Null,|v|legacy_recognition_price(v,client));
    let mut issues:Vec<String>=item.get("issues").and_then(Value::as_array)
        .into_iter().flatten().filter_map(Value::as_str)
        .map(str::to_owned).collect();
    let invalid_color=p.get("colorStatus").and_then(Value::as_str)==Some("MISSING")
        || (p.get("saleUnit").and_then(Value::as_bool)!=Some(true)
            && ["색상 미표기",""].contains(&default_color));
    let missing_configuration=chosen.map_or(false,|v| {
        ["가방","바퀴"].iter().any(|token| {
            let pattern=regex::Regex::new(&format!(
                r"{token}\s*(?:미포함|포함|미장착|장착|없음|있음)"
            )).expect("configuration label");
            variants.into_iter().flatten().any(|other|
                pattern.is_match(other.get("option").and_then(Value::as_str).unwrap_or("")))
            && !pattern.is_match(v.get("option").and_then(Value::as_str).unwrap_or(""))
        })
    });
    let issue=if chosen.is_some() {
        if explicit.is_empty() && invalid_color {
            Some("기본 색상 확인 필요")
        } else if missing_configuration {
            Some("가방 또는 바퀴 포함 / 미포함 정보 확인 필요")
        } else {None}
    } else if p.get("requiresOption").and_then(Value::as_bool)==Some(true) {
        Some("상품 구성 미확정 / 판매 구성을 확인하세요")
    } else {
        Some("등록된 독립 상품을 다시 선택하세요")
    };
    if let Some(issue)=issue {
        if !issues.iter().any(|s|s==issue){issues.push(issue.to_owned());}
    }
    item["issues"]=json!(issues);
}

/// Product label colors use src/product-models.ts::colorLabel, which is
/// intentionally different from src/domain.ts::normalizeColor. This is a
/// display-only mapping; SKU selection continues to use canonical colors.
fn ts_candidate_color_label(value: &str) -> String {
    let lower=value.trim().to_lowercase();
    match lower.as_str() {
        "gold"|"골드"=>"골드".to_owned(),
        "rose gold"|"로즈골드"=>"로즈골드".to_owned(),
        "cosmo black"|"cosmoblack"|"코스모블랙"=>"코스모블랙".to_owned(),
        "black"|"검정"|"블랙"=>"블랙".to_owned(),
        "gray"|"grey"|"회색"|"그레이"=>"그레이".to_owned(),
        "brown"|"브라운"=>"브라운".to_owned(),
        "white"|"흰색"|"화이트"=>"화이트".to_owned(),
        "clear"|"클리어"=>"클리어".to_owned(),
        "frost"|"frosted"|"프로스트"=>"프로스트".to_owned(),
        "silver"=>"실버".to_owned(),
        "blue"=>"블루".to_owned(),
        "red"=>"레드".to_owned(),
        "색상 미표기"=>"".to_owned(),
        _=>value.to_owned(),
    }
}

/// Recover the parseLegacy-local product row without reassigning any
/// evidence. "name/phone" and table columns stay in original evidence,
/// but are not part of the Product Item.raw field.
fn ts_product_display<'a>(
    hit: &'a ProductHit, events: &[Evidence]
) -> (&'a str, usize) {
    let source=hit.raw.as_str();
    let trim_span=|begin:usize,end:usize|->(&'a str,usize) {
        let end=end.min(source.len());
        if begin>end || !source.is_char_boundary(begin) ||
            !source.is_char_boundary(end) {return (source,0);}
        let part=&source[begin..end];
        let trimmed=part.trim();
        let leading=part.find(trimmed).unwrap_or(0);
        (trimmed,begin+leading)
    };
    // A normalized spreadsheet line: item code is the second pipe cell.
    // The original parseLegacy creates that cell as a separate item line.
    if source.matches('|').count()>=4 {
        let bars:Vec<usize>=source.match_indices('|').map(|(i,_)|i).collect();
        if bars.len()>1 {
            // A numeric first column is only the line number. The TS
            // source retains the entire order row in that format.
            // A descriptive first column followed by a SKU in column 2
            // denotes the actual product field (sample 19: BB-3).
            let first=source[..bars[0]].trim();
            let numbered=!first.is_empty()
                && first.chars().all(|c|c.is_ascii_digit());
            if !numbered {
                let (cell,offset)=trim_span(bars[0]+1,bars[1]);
                if !cell.is_empty() && cell.chars().any(|c|c.is_alphabetic()) {
                    return (cell,offset);
                }
            }
        }
    }
    // The vertical column separator introduces recipient information.
    if let Some(separator)=source.find('ㅣ') {
        let (part,offset)=trim_span(0,separator);
        if !part.is_empty(){return (part,offset);}
    }
    // Mixed product+quantity+recipient+phone on one line, as in
    // original sample 18: only the product span through its quantity
    // belongs to the local Item, not the separate recipient/contact.
    let has_contact=regex::Regex::new(concat!(
        r"(?:050\d|01[016789]|02|0[3-6]\d|070)[\s().-]*\d{3,4}[\s().-]*\d{4}"
    )).expect("contact in mixed product").is_match(source);
    if has_contact && hit.quantities.len()==1 {
        let item_start=events[hit.evidence].start.saturating_sub(hit.source_start);
        let quantity_end=events[hit.quantities[0]].end.saturating_sub(hit.source_start);
        let (part,offset)=trim_span(item_start,quantity_end);
        if !part.is_empty(){return (part,offset);}
    }
    (source,0)
}

/// A standalone dispatch instruction with an explicit calendar date is
/// delivery evidence, not a product, quantity, or new recipient. Keep the
/// wording intact: do not infer a year or convert dispatch into arrival.
fn dated_dispatch_instruction(text:&str)->bool{
    static RE:std::sync::OnceLock<Regex>=std::sync::OnceLock::new();
    let re=RE.get_or_init(||Regex::new(concat!(
        r"^\s*(?:[※*■>-]+\s*)?(?:발송\s*(?:요청일|예정일|일자|일)|출고\s*(?:요청일|예정일|일자|일))?\s*[:：]?\s*",
        r"(?:[0-9]{4}\s*년\s*)?(?:0?[1-9]|1[0-2])\s*월\s*(?:0?[1-9]|[12][0-9]|3[01])\s*일",
        r"(?:\s*\([월화수목금토일](?:요일)?\))?\s*(?:에|까지|부터|이후|이전)?\s*",
        r"(?:발송|출고)(?:\s*(?:요청(?:드립니다|합니다)?|부탁(?:드립니다|합니다|드려요)?|해주세요|해\s*주세요|바랍니다|예정|필수|희망))?\s*[.!~]*\s*$"
    )).expect("dated dispatch instruction"));
    re.is_match(text)
}
fn analyze(raw: &str, state: &Value, client: Option<&Value>) -> Result<Value, String> {
    analyze_with_owner_trace(raw,state,client,false,false)
}

fn analyze_with_owner_trace(raw:&str,state:&Value,client:Option<&Value>,
    collect_owner_trace:bool,collect_candidate_trace:bool)->Result<Value,String>{
    analyze_with_owner_overrides(raw,state,client,collect_owner_trace,collect_candidate_trace,&std::collections::BTreeMap::new())
}
// Internal counterfactual replay only. The normal analyzer has no caller
// override parameter. Reuse the existing parser to rebuild all derived fields.
fn analyze_with_owner_overrides(raw:&str,state:&Value,client:Option<&Value>,
    collect_owner_trace:bool,collect_candidate_trace:bool,
    overrides:&std::collections::BTreeMap<(usize,usize),(usize,usize)>)->Result<Value,String>{
    let client_id = client_id(client);
    let lines = physical_lines(raw);
    let mut events: Vec<Evidence> = Vec::new();
    let mut product_hits: Vec<ProductHit> = Vec::new();
    let mut candidate_trace=Vec::new();
    let mut candidate_trace_truncated=false;
    let mut item_candidate_trace=Vec::new();
    let mut type_trace=Vec::new();
    let mut type_trace_truncated=false;
    let mut record_type=|line:usize,start:usize,end:usize,kind:&str,reason:&str|{
        if !collect_candidate_trace{return;}
        if type_trace.len()>=1024{type_trace_truncated=true;return;}
        type_trace.push(json!({"line":line,"source":{"start":raw.get(..start).map(|s|s.encode_utf16().count()),
            "end":raw.get(..end).map(|s|s.encode_utf16().count())},"kind":kind,"reasonCode":reason}));
    };
    let phone = phone_regex();
    let person_label = recipient_label();
    // Direct translation of order-evidence.ts:140-170: parenthesized
    // delivery requests and note continuation remain separate evidence.
    let note_label = Regex::new(
        r"^\s*\(?(?:배송\s*)?(?:메세지|메시지|요청(?:사항)?|메모)(?:\s*[:：]\s*|\s+)(.*)$"
    ).expect("note");
    let mut note_continuation: Option<usize>=None;
    let mut sender_section = false;
    for (line_index,line) in lines.iter().enumerate() {
        let text = line.text.trim();
        let explicit_name=explicit_recipient_span(line.text,&person_label,&phone);
        let near_delivery=lines.get(line_index+1).is_some_and(|next|
            next.number<=line.number+2 && delivery_boundary(next.text,&phone)) ||
            (line_index>0 && lines.get(line_index-1).is_some_and(|prev|
                line.number<=prev.number+2 && phone.is_match(prev.text)) &&
                lines.get(line_index+1).is_none_or(|next|next.paragraph!=line.paragraph));
        let after_label=line_index>0 && lines.get(line_index-1).is_some_and(|prev|
            person_label.find(prev.text).is_some_and(|m|prev.text[m.end()..].trim().is_empty()));
        if Regex::new(r"^(?:보내는\s*(?:분|사람|곳)|송하인(?:명)?)\s*[:：]")
            .expect("sender prefix").is_match(text) {
            sender_section = true;
        }
        if sender_section { record_type(line.number,line.start,line.start+line.text.len(),"line","sender_section");continue; }
        if postcode::labelled_line(text).is_some() {
            record_type(line.number,line.start,line.start+line.text.len(),"line","labelled_postcode");
            note_continuation=None;
            continue;
        }
        // Parse an explicit dated dispatch instruction before recipient or
        // product heuristics. An ordinary order date or a question does not
        // become a confirmed dispatch request.
        if dated_dispatch_instruction(text){
            let start=line.text.find(text).unwrap_or(0);
            add_match(&mut events,line,start,start+text.len(),"note",
                text.to_owned(),"발송 요청일 원문 구간");
            note_continuation=None;
            continue;
        }
        if is_money_or_header(text) { record_type(line.number,line.start,line.start+line.text.len(),"line","money_or_header");continue; }
        // The TS recognizer extends a memo through subsequent non-field
        // lines in the same paragraph, stopping at any contact/address/item.
        if let Some(i)=note_continuation {
            let field=person_label.is_match(line.text)
                || note_label.is_match(line.text)
                || address_begin(line.text).is_some()
                || phone.is_match(line.text)
                || (near_delivery && !other_recipient_field(text))
                || weak_recipient_hint(text)
                || (quantity::extract(line.text).len()>0
                    && model::category(line.text).is_some())
                || product_signal(line.text,
                    &model::decision_json(&model::decide(line.text,state,client_id)),state);
            if !field && line.paragraph==events[i].paragraph {
                events[i].end=line.start+line.text.len();
                if !events[i].value.is_empty(){events[i].value.push('\n');}
                events[i].value.push_str(text);
                record_type(line.number,line.start,line.start+line.text.len(),"line","note_continuation");continue;
            }
            note_continuation=None;
        }
        if let Some(c) = note_label.captures(line.text) {
            if let Some(v) = c.get(1) {
                let value=if text.starts_with('(') {
                    v.as_str().trim_end_matches(')').trim().to_owned()
                } else {v.as_str().trim().to_owned()};
                let i=add_match(&mut events, line, v.start(), v.end(), "note",
                    value,"배송 요청사항 표제어");
                note_continuation=Some(i);
            }
            record_type(line.number,line.start,line.start+line.text.len(),"line","note_label");continue;
        }

        // order-evidence.ts:170-188: an indented building/floor line
        // is continuation of the preceding address, never a new item.
        if line.text.starts_with("  ") && address_begin(line.text).is_none()
            && !phone.is_match(line.text)
            && !weak_recipient_hint(text)
            && Regex::new(r"\d+\s*(?:층|호)").expect("address detail").is_match(text) {
            if let Some(previous)=events.iter().rev()
                .find(|e|e.kind=="address" && e.paragraph==line.paragraph) {
                if previous.start<line.start
                    && !events.iter().any(|e|e.start>previous.end
                        && e.start<line.start
                        && (e.kind=="name"||e.kind=="phone"||e.kind=="mobile"||e.kind=="product")) {
                    let at=line.text.find(text).unwrap_or(0);
                    add_match(&mut events,line,at,at+text.len(),"address",
                        text.to_owned(),"들여쓰기된 상세주소 연속 행");
                    record_type(line.number,line.start,line.start+line.text.len(),"line","address_continuation");continue;
                }
            }
        }

        // An address-shaped substring inside an explicit name is not an address.
        let address = if let Some((_,end))=explicit_name{
            address_begin(&line.text[end..]).map(|a|end+a)
        }else{address_begin(line.text)};
        // parser.ts::parseLegacy masks complete phone numbers *before*
        // interpreting a mixed address/phone row. A phone is a distinct
        // contact field, not part of the recipient's delivery address.
        if let Some(start) = address {
            let original=&line.text[start..];
            let original=postcode::labelled_suffix(original)
                .map_or(original,|(end,_)|&original[..end]);
            let mut without_phones=original.to_owned();
            let spans:Vec<_>=phone.find_iter(original)
                .map(|m|(m.start(),m.end())).collect();
            for (from,to) in spans.into_iter().rev() {
                without_phones.replace_range(from..to,&" ".repeat(to-from));
            }
            let value=without_phones.trim();
            if !value.is_empty() {
                let end=start+without_phones.trim_end().len();
                add_match(&mut events,line,start,end,"address",
                    value.to_owned(),"배송주소 원문 구간");
            }
        }

        let name_span=explicit_name.or_else(||{
            if after_label && address.is_none() && !phone.is_match(line.text) &&
                !other_recipient_field(text){
                trim_span(line.text,0,line.text.len())
            }else{
                unlabelled_recipient_span(line.text,address,&phone,near_delivery,state,client_id,
                    |a,b,reason|record_type(line.number,line.start+a,line.start+b,"name",reason))
            }
        });
        if name_span.is_none(){record_type(line.number,line.start,line.start+line.text.len(),"name","no_accepted_name_span");}
        if let Some((begin,end))=name_span{
            add_match(&mut events,line,begin,end,"name",line.text[begin..end].to_owned(),
                if explicit_name.is_some()||after_label{"명시적 수취인 이름"}
                else{"수취인 필드 경계에서 원문 전체 보존"});
        }
        for c in phone.captures_iter(line.text) {
            let full = c.get(0).expect("phone match");
            if name_span.is_some_and(|(a,b)|a<full.end()&&full.start()<b){
                record_type(line.number,line.start+full.start(),line.start+full.end(),"contact","overlaps_name");continue;
            }
            // Reject a phone-like substring embedded in a longer digit run.
            if (full.start() > 0 && line.text[..full.start()].chars().next_back()
                .map_or(false, |v| v.is_ascii_digit()))
                || line.text[full.end()..].chars().next().map_or(false, |v|v.is_ascii_digit()) {
                record_type(line.number,line.start+full.start(),line.start+full.end(),"contact","embedded_digit_run");continue;
            }
            let value = format!("{}-{}-{}", &c[1], &c[2], &c[3]);
            let kind = if c[1].starts_with("01") { "mobile" } else { "phone" };
            add_match(&mut events, line, full.start(),full.end(),kind,value,
                "연락처 원문 구간");
        }
        let contact_spans: Vec<(usize,usize)> = phone.find_iter(line.text)
            .map(|m|(m.start(),m.end())).collect();
        if address.is_none() && ts_semantics::only_contact_or_identity(line.text,&contact_spans) {
            record_type(line.number,line.start,line.start+line.text.len(),"product","contact_or_identity_only");continue;
        }
        // A name-only field cannot create a phantom product or quantity,
        // even if the name itself looks like a registered model.
        if explicit_name.is_some() || name_span.is_some_and(|(a,_)|
            line.text[..a].trim().is_empty()) || (after_label && name_span.is_some()){
            record_type(line.number,line.start,line.start+line.text.len(),"product","name_field_priority");continue;
        }

        // parser.ts::logicalLines gives the address the whole line unless
        // a recognized/quantity-qualified SKU precedes the address.
        // Never infer a product from an address's five-digit postal code.
        if let Some(begin)=address {
            if !ts_semantics::model_before_address(&line.text[..begin],state) {
                record_type(line.number,line.start,line.start+line.text.len(),"product","address_priority_without_model");continue;
            }
        }
        let product_end=address.unwrap_or(line.text.len());
        let product_end=name_span.map_or(product_end,|(a,_)|a.min(product_end));
        // Table quantities depend on the original column boundaries.
        let product_part=if line.text[..product_end].matches('|').count()>=4 {
            &line.text[..address.unwrap_or(line.text.len())]
        }else{&line.text[..product_end]};
        let trimmed = product_part.trim();
        if trimmed.is_empty() || is_money_or_header(trimmed)
            || (trimmed.contains('|') && trimmed.split('|').any(|column|
                matches!(column.trim(),"배송비"|"택배비"|"운임"))) {
            record_type(line.number,line.start,line.start+line.text.len(),"product","empty_money_header_or_freight");continue;
        }
        // Exact TS parser.ts contact branch: a phone/identity row must not
        // create a product even when its digits match a generic model regex.
        let trim_phones: Vec<(usize,usize)> = phone.find_iter(trimmed)
            .map(|m|(m.start(),m.end())).collect();
        let decision = model::decision_json(&model::decide(trimmed,state,client_id));
        // parser.ts::parseLegacy: a full contact may share the same physical
        // line with a valid product and quantity; filter contact-only rows,
        // not valid joint records such as "709 민판 한개 이가상 010-...".
        if ts_semantics::only_contact_or_identity(trimmed,&trim_phones)
            && quantity::extract(trimmed).is_empty()
            && decision.pointer("/match/kind").and_then(Value::as_str)==Some("UNKNOWN") {
            record_type(line.number,line.start,line.start+line.text.len(),"product","unknown_contact_only");continue;
        }
        if !product_signal(trimmed, &decision, state) {
            record_type(line.number,line.start,line.start+line.text.len(),"product","no_product_signal");continue;
        }
        // Require model/category evidence, not a value made solely from a date or contact.
        if trimmed.contains("발주일") || trimmed.contains("발송일") {
            record_type(line.number,line.start,line.start+line.text.len(),"product","document_date");continue;
        }
        let skip_until = ts_semantics::table_product_prefix(trimmed);
        // Never synthesize a new string and then treat its indexes as offsets
        // into raw: UTF-8/UTF-16 source-evidence contract must be lossless.
        let capture=collect_candidate_trace && candidate_trace.len()<64;
        if collect_candidate_trace && !capture{candidate_trace_truncated=true;}
        let mut stages=Vec::new();
        let matched = model::decision_json(&model::decide_with_trace(skip_until,state,client_id,
            capture.then_some(&mut stages)));
        let mut extraction_trace=quantity::QuantityTrace::default();
        let mut quantity_values = if trimmed.contains('|') {
            let model_end=matched.get("evidence").and_then(Value::as_object)
                .and_then(|o|o.get("end")).and_then(Value::as_u64)
                .map(|v|v as usize);
            ts_semantics::table_quantity(trimmed,model_end).map_or_else(Vec::new,
                |(value,start,end)|vec![quantity::Quantity{value,start,end}])
        } else {
            let allow_trailing=model::products(state).iter().any(|p|model::model_names(p).iter()
                .any(|name|model::find_term(skip_until,name,true).is_some()));
            if capture{quantity::for_product_with_trace(skip_until,false,allow_trailing,Some(&mut extraction_trace))}
            else{quantity::for_product(skip_until,false,allow_trailing)}
        };
        let quantity_before=if capture{Some(quantity_values.clone())}else{None};
        // parser.ts:537-552: a compound sale-unit phrase containing
        // "2개" is part of the option name, not an order quantity.
        if quantity_values.len()==1 {
            if let Some(p)=matched.get("productId").and_then(Value::as_str)
                .and_then(|pid|model::products(state).into_iter().find(|p|
                    p.get("id").and_then(Value::as_str)==Some(pid))) {
                let sale_unit=p.get("sellingUnit").and_then(Value::as_str);
                let option=p.get("variants").and_then(Value::as_array)
                    .and_then(|vs|vs.first())
                    .and_then(|v|v.get("option")).and_then(Value::as_str);
                if sale_unit==Some("2개 구성")
                    && option.map_or(false,|o|!o.is_empty() && trimmed.contains(o)) {
                    quantity_values.clear();
                }
            }
        }
        let mut quantity_indexes = Vec::new();
        let offset = line.text.find(trimmed).unwrap_or(0);
        if capture{
            let encode=|qs:&[quantity::Quantity]|qs.iter().take(128).map(|q|json!({
                "value":q.value,"start":raw.get(..line.start+offset+q.start).map(|s|s.encode_utf16().count()),
                "end":raw.get(..line.start+offset+q.end).map(|s|s.encode_utf16().count())})).collect::<Vec<_>>();
            let before=quantity_before.as_deref().unwrap_or(&[]);
            candidate_trace.push(json!({"line":line.number,
                "source":{"start":utf16(raw,line.start+offset),
                    "end":utf16(raw,line.start+offset+trimmed.len())},
                "productStages":stages,"quantityExtraction":{"records":extraction_trace.records,
                    "truncated":extraction_trace.truncated,
                    "coordinateSystem":"matching_slice_utf16",
                    "tablePath":trimmed.contains('|')},
                "quantityBeforeSaleUnitFilter":encode(before),
                "quantityAfterSaleUnitFilter":encode(&quantity_values),
                "quantityTruncated":before.len()>128||quantity_values.len()>128,
                "saleUnitFilterRemoved":before.len()>quantity_values.len()}));
        }
        for q in quantity_values {
            quantity_indexes.push(add_match(&mut events, line, offset+q.start,offset+q.end,
                "quantity",q.value.to_string(),"상품 행에 명시된 수량"));
        }
        let term_span = matched.get("evidence").and_then(Value::as_object)
            .and_then(|o| Some((o.get("start")?.as_u64()? as usize,o.get("end")?.as_u64()? as usize)));
        let (p_start,p_end) = term_span.unwrap_or((0,skip_until.len()));
        // model spans are byte offsets inside this Rust module.
        let p_start = p_start.min(skip_until.len());
        let p_end = p_end.min(skip_until.len()).max(p_start);
        let p_start = if skip_until.is_char_boundary(p_start) { p_start } else { 0 };
        let p_end = if skip_until.is_char_boundary(p_end) { p_end } else { skip_until.len() };
        // The matching slice excludes recipient text, but preserve the
        // historical product raw line for reanalysis and manual identity.
        let original_product_line=line.text[..address.unwrap_or(line.text.len())].trim();
        let product_evidence = add_match(&mut events,line,offset+p_start,offset+p_end,
            "product",original_product_line.to_owned(),"상품명/모델명/별칭 인식 근거");
        product_hits.push(ProductHit {
            evidence:product_evidence,source_start:line.start+offset,
            line:line.number,raw:original_product_line.to_owned(),
            decision:matched,quantities:quantity_indexes,
            common:Regex::new(r"공통|모두|전부|전원|각\s*수취인|같은\s*상품")
                .expect("common item").is_match(trimmed),
            // order-evidence.ts:358-368: a common total is not a
            // per-recipient quantity without explicit 'each' evidence.
            each:Regex::new(r"(?i)(?:각각?|수취인마다|한\s*분당).*(?:\d+|한|두|세)\s*(?:개|대)|씩")
                .expect("each-recipient quantity").is_match(trimmed),
        });
        // TS OrderEvidence.value is the parsed product field, not the whole
        // mixed recipient/phone row or all columns from a shipment table.
        // Its evidence span still points only to the actual matched token.
        if let Some(hit)=product_hits.last() {
            // parser.ts::logicalLines splits model+quantity from an inline
            // delivery-address line. Without a detected address it retains
            // the original physical product line in candidate.value, even
            // when the line also contains a contact/person. Do not shorten
            // the top-level candidate solely because a phone was detected.
            let displayed=if address.is_some() {
                ts_product_display(hit,&events).0.to_owned()
            } else {hit.raw.clone()};
            events[product_evidence].value=displayed;
        }
    }
    // A generic quantity line is evidence but cannot itself prove which SKU was ordered.
    for line in &lines {
        if events.iter().any(|e| e.kind == "quantity" && e.line == line.number) { continue; }
        if is_money_or_header(line.text) { continue; }
        if product_hits.iter().any(|h| h.line == line.number) { continue; }
        let capture=collect_candidate_trace&&candidate_trace.len()<64;
        let mut extraction_trace=quantity::QuantityTrace::default();
        let extracted=quantity::extract_with_trace(line.text,capture.then_some(&mut extraction_trace));
        if collect_candidate_trace&&!capture{candidate_trace_truncated=true;}
        if capture&&!extraction_trace.records.is_empty(){
            candidate_trace.push(json!({"line":line.number,"source":{"start":utf16(raw,line.start),
                "end":utf16(raw,line.start+line.text.len())},"quantityExtraction":{
                "records":extraction_trace.records,"truncated":extraction_trace.truncated,
                "coordinateSystem":"line_utf16","tablePath":false}}));
        }
        for q in extracted {
            // An independently scanned quantity must not reuse characters
            // already owned by a recognized address/name/contact/memo span.
            // Preserve separately positioned quantities in mixed-field rows.
            let start=line.start+q.start;
            let end=line.start+q.end;
            let overlaps_field=events.iter().any(|e|
                matches!(e.kind,"address"|"name"|"phone"|"mobile"|"note")
                    && e.start < end && start < e.end);
            if overlaps_field {
                record_type(line.number,start,end,"quantity","overlaps_recipient_field");continue;
            }
            add_match(&mut events,line,q.start,q.end,"quantity",q.value.to_string(),
                "독립 수량 원문; 상품 연결은 별도 판정");
        }
    }

    let mut anchors: Vec<usize> = events.iter().enumerate()
        .filter_map(|(i,e)| (e.kind == "name").then_some(i)).collect();
    anchors.sort_by_key(|i| events[*i].start);
    anchors.dedup_by_key(|i| events[*i].line);
    if anchors.is_empty() {
        anchors = events.iter().enumerate()
            .filter_map(|(i,e)| (e.kind=="address").then_some(i)).collect();
    }
    // If a document has only one unnamed destination, keep exactly one row.
    if anchors.is_empty() && raw.trim().is_empty() {
        return Ok(json!({"recipients":[],"candidates":[],"unassigned":[]}));
    }
    let mut recipients: Vec<Value> = anchors.iter().enumerate().map(|(i,a)| {
        let e = &events[*a];
        blank_recipient(&format!("recognition:recipient:{i}"),
            if e.kind=="name" {&e.value} else {""},Some(e),raw)
    }).collect();
    if recipients.is_empty() {
        recipients.push(blank_recipient("recognition:recipient:0","",None,raw));
    }
    let mut owner_trace=collect_owner_trace.then(Vec::new);
    let mut postcodes=vec![HashSet::<String>::new();recipients.len()];
    let mut applied=0usize;
    for index in 0..events.len() {
        if events[index].kind == "product" || events[index].kind == "quantity" { continue; }
        let mut row_index = recipient_owner_index(&anchors,&events,index,raw,owner_trace.as_mut());
        if !overrides.is_empty(){
            let e=&events[index];
            let source=(utf16(raw,e.start),utf16(raw,e.end));
            if let Some(target)=overrides.get(&source){
                if !matches!(e.kind,"address"|"phone"|"mobile"){return Err("귀속 변경 대상 유형 오류".into());}
                let mut matches=anchors.iter().enumerate().filter(|(_,a)|{
                    let anchor=&events[**a];anchor.kind=="name"&&
                        (utf16(raw,anchor.start),utf16(raw,anchor.end))==*target
                });
                let (destination,_)=matches.next().ok_or("귀속 변경 수취인 원문 누락")?;
                if matches.next().is_some()||destination==row_index{return Err("귀속 변경 대상 불명확".into());}
                row_index=destination;applied+=1;
            }
        }
        let rid = recipients[row_index]["id"].as_str().unwrap_or("").to_owned();
        let e = &events[index];
        let kind = e.kind;
        if kind=="address" {
            if let Some((line_index,line))=lines.iter().enumerate()
                .find(|(_,line)|line.number==e.line){
                let prefix=&raw[line.start..e.start];
                if let Some(code)=postcode::before_address(prefix){
                    postcodes[row_index].insert(code.to_owned());
                }
                let tail=&line.text[e.start-line.start..];
                if let Some((_,code))=postcode::labelled_suffix(tail){
                    postcodes[row_index].insert(code.to_owned());
                }
                // A standalone labelled code belongs only to an unambiguous
                // immediately adjacent address in the same paragraph.
                for adjacent in [line_index.checked_sub(1),line_index.checked_add(1)]
                    .into_iter().flatten().filter_map(|i|lines.get(i)){
                    if adjacent.paragraph!=line.paragraph ||
                        adjacent.number.abs_diff(line.number)!=1{continue;}
                    if let Some(code)=postcode::labelled_line(adjacent.text){
                        let owners=events.iter().filter(|other|other.kind=="address"
                            && other.paragraph==adjacent.paragraph
                            && other.line.abs_diff(adjacent.number)==1).count();
                        if owners==1{postcodes[row_index].insert(code.to_owned());}
                    }
                }
            }
        }
        let v = e.value.clone();
        let previous = recipients[row_index][kind].as_str().unwrap_or("").to_owned();
        if previous.is_empty() {
            recipients[row_index][kind] = json!(v);
        } else if kind=="address" && !previous.contains(&v) {
            recipients[row_index]["address"] = json!(format!("{previous} {v}"));
        } else if kind=="note" && !previous.lines().any(|line|line==v.as_str()) && previous!=v {
            // Multiple independent delivery instructions share the existing
            // note field. Preserve order and avoid duplicate exact lines.
            recipients[row_index]["note"]=json!(format!("{previous}\n{v}"));
        } else if (kind=="phone" || kind=="mobile") && previous != v {
            let other = if kind=="phone" { "mobile" } else { "phone" };
            if recipients[row_index][other].as_str().unwrap_or("").is_empty() {
                recipients[row_index][other] = json!(v);
            }
        }
        // When two mobile contacts are present, the second is retained in
        // the alternate phone field. TS source(row,c,other) records the
        // effective destination field, while the original evidence remains
        // of kind "mobile".
        let effective_field = if (kind=="phone" || kind=="mobile")
            && !previous.is_empty() && previous != v {
            let other=if kind=="phone" {"mobile"} else {"phone"};
            if recipients[row_index][other].as_str()==Some(v.as_str()) {
                other
            } else {kind}
        } else {kind};
        let mut src=source_json(raw,e,&rid,&rid);
        src["field"]=json!(effective_field);
        recipients[row_index]["sources"].as_array_mut().expect("sources").push(src);
        recipients[row_index]["candidates"].as_array_mut().expect("candidates")
            .push(json!({"id":format!("{}:{}:{}",kind,utf16(raw,e.start),utf16(raw,e.end)),
                "field":effective_field,"value":e.value,"label":e.value,
                "start":utf16(raw,e.start),"end":utf16(raw,e.end)}));
        events[index].recipient = Some(row_index);
    }
    if applied!=overrides.len(){return Err("귀속 변경 원문 중복 또는 누락".into());}
    for (row,codes) in recipients.iter_mut().zip(postcodes) {
        if codes.len()==1 {
            row["postcode"]=json!(codes.into_iter().next().unwrap());
        }
        if let Some(address) = row["address"].as_str().filter(|s| !s.is_empty()) {
            let (kind,status,parts) = address_info(address);
            row["addressKind"] = json!(kind);
            row["addressStatus"] = json!(status);
            row["addressParts"] = parts;
        }
    }
    let has_leading = anchors.first().map_or(false, |a| product_hits.iter().any(|h|
        !h.common && events[h.evidence].start < events[*a].start));
    for hit in &product_hits {
        let anchor_index = owner_index(&anchors,&events,events[hit.evidence].start);
        let owner = if has_leading && !anchors.is_empty() {
            anchors.iter().position(|a| events[*a].start > events[hit.evidence].start)
                .unwrap_or(anchors.len()-1)
        } else { anchor_index };
        let attach_to: Vec<usize> = if hit.common {
            (0..recipients.len()).filter(|idx| recipients[*idx]["items"]
                .as_array().map_or(true,|xs|xs.is_empty())).collect()
        } else { vec![owner] };
        for idx in attach_to {
            let (display_raw,display_start)=ts_product_display(hit,&events);
            let id = format!("recognition:item:{}:{}", utf16(raw,events[hit.evidence].start),idx);
            let rid = recipients[idx]["id"].as_str().unwrap_or("").to_owned();
            // Direct port of order-evidence.ts:534-560:
            // - non-common goods keep their own line quantity
            // - common goods use a local detached quantity or an
            //   explicitly per-recipient ('각각'/'씩') source quantity
            // - a common document total must never be multiplied
            let selected_qty = if hit.common {
                events.iter().enumerate().find_map(|(qi,e)|
                    (e.kind=="quantity" && e.item_id.is_none()
                        && !hit.quantities.contains(&qi)
                        && !product_hits.iter().any(|p|p.line==e.line)
                        && owner_index(&anchors,&events,e.start)==idx).then_some(qi)
                ).or_else(||(hit.each && hit.quantities.len()==1)
                    .then(||hit.quantities[0]))
            } else if hit.quantities.len()==1 {
                Some(hit.quantities[0])
            } else {None};
            let unique = selected_qty.is_some();
            let quantity_value = selected_qty
                .and_then(|i|events[i].value.parse::<i64>().ok());
            let chosen = hit.decision.get("productId").and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            let decision_name=hit.decision.get("name").and_then(Value::as_str)
                .filter(|s|!s.is_empty()).unwrap_or(display_raw);
            // parser.ts::parseLegacy strips numeric quantity tokens from
            // unregistered product display names (not registered SKU names).
            let name=if chosen.is_none() && !regex::Regex::new(
                    r"(?i)(?:BD\s*)?\d{3}[A-Za-z0-9-]*"
                ).expect("legacy unknown model").is_match(display_raw) {
                regex::Regex::new(r"(?i)\s*(?:\d+|세|삼)\s*(?:개|대|ea).*")
                    .expect("legacy bare quantity")
                    .replace(display_raw,"").trim().to_owned()
            } else {decision_name.to_owned()};
            let reason = hit.decision.get("recognitionReason").cloned().unwrap_or(Value::Null);
            let mut issues: Vec<String> = Vec::new();
            if chosen.is_none() {
                issues.push("제품명이 확인되지 않습니다.".into());
            }
            // analyzeEvidence.ts::attach strips unresolved single-quantity
            // notices from the Item. quantityConfirmed remains the only
            // explicit approval state; ambiguous multiple quantities persist.
            if !unique && !hit.quantities.is_empty() {
                issues.push("한 줄에 여러 상품과 수량 확인 필요".to_owned());
            }
            let mut term = if hit.common {json!({
                "kind":"COMMON_INHERIT",
                "term":hit.raw,
                "productIds":chosen.into_iter().collect::<Vec<_>>()
            })} else {hit.decision["match"].clone()};
            if !hit.common {
                // parseLegacy.ts::match.term is the entire product line for
                // registered NAME matches (not the small regex evidence).
                if term["kind"]=="NAME"
                    && term.get("aliasId").map_or(true,Value::is_null) {
                    // Source parser.ts chooses unknownModel before the
                    // full line even when the registered SKU matched.
                    let numeric=regex::Regex::new(
                        r"(?i)(?:BD\s*)?\d{3}[A-Za-z0-9-]*"
                    ).expect("legacy unknown term");
                    term["term"]=json!(numeric.find(display_raw)
                        .map(|m|m.as_str()).unwrap_or(display_raw));
                }
                // TypeScript omits undefined aliasId, not JSON null.
                if term["aliasId"].is_null() {
                    term.as_object_mut().map(|obj|obj.remove("aliasId"));
                }
            }
            let mut products=hit.decision["productCandidates"].clone();
            if let Some(candidates)=products.as_array_mut() {
                // This is the local parseLegacy(line) candidate contract.
                // Full-document offsets belong only to source evidence.
                let begin=hit.raw[..display_start].encode_utf16().count();
                let end=begin+display_raw.encode_utf16().count();
                for candidate in candidates {
                    candidate["start"]=json!(begin);
                    candidate["end"]=json!(end);
                    if let Some(pid)=candidate.get("productId").and_then(Value::as_str) {
                        if let Some(p)=model::products(state).into_iter()
                            .find(|p|p.get("id").and_then(Value::as_str)==Some(pid)) {
                            let name=p.get("name").and_then(Value::as_str).unwrap_or("");
                            let suppress=p.get("colorStatus").and_then(Value::as_str)
                                ==Some("NOT_REQUIRED");
                            let color=if suppress {String::new()} else {
                                let first=p.get("variants").and_then(Value::as_array)
                                    .and_then(|v|v.first());
                                ts_candidate_color_label(first
                                    .and_then(|v|v.get("color"))
                                    .and_then(Value::as_str).unwrap_or(""))
                            };
                            candidate["label"]=json!(format!("{name} {color}"));
                        }
                    }
                }
            }
            let mut item = json!({
                "id":id,"productId":chosen,"variantId":hit.decision["variantId"],
                "raw":display_raw,"name":name,
                "color":hit.decision["color"],"quantity":quantity_value,
                "quantityConfirmed":unique,"unitPrice":null,
                "origin":hit.decision.get("origin").and_then(Value::as_str).unwrap_or("FACTORY"),
                "defaulted":false,"issues":issues,
                "url":"","match":term,
                "productCandidates":products,"sources":[],
            });
            // An absent TypeScript optional property is not JSON null.
            if !reason.is_null() {item["recognitionReason"]=reason;}
            apply_registered_item(&mut item,state,client,
                hit.decision.get("color").and_then(Value::as_str).unwrap_or(""));
            if collect_candidate_trace{
                if item_candidate_trace.len()<64{
                    let product=chosen.and_then(|pid|model::products(state).into_iter()
                        .find(|p|p["id"].as_str()==Some(pid)));
                    let variants=product.and_then(|p|p["variants"].as_array());
                    let variant_count=variants.map_or(0,Vec::len);
                    let selectable=variant_count==1&&product.is_some_and(|p|p["requiresOption"]!=true);
                    let color=item["color"].as_str().unwrap_or("");
                    let options=variants.into_iter().flatten().take(128).map(|v|{
                        let matches_color=model::norm_color(v["color"].as_str().unwrap_or(""))==color;
                        let selected=v["id"].is_string()&&v["id"]==item["variantId"];
                        json!({"variantId":v["id"],"selected":selected,
                            "reasonCode":if selected{"selected"}else if !selectable{"requires_option_or_multiple_variants"}
                                else if !matches_color{"color_mismatch"}else{"not_selected"}})
                    }).collect::<Vec<_>>();
                    let quantity_candidates=hit.quantities.iter().take(128).map(|qi|{
                        let q=&events[*qi];
                        json!({"start":utf16(raw,q.start),"end":utf16(raw,q.end),
                            "value":q.value,"selected":selected_qty==Some(*qi),
                            "reasonCode":if selected_qty==Some(*qi){"selected"}
                                else if hit.common&&!hit.each{"common_total_not_per_recipient"}
                                else if hit.quantities.len()>1{"multiple_quantities"}
                                else{"not_selected"}})
                    }).collect::<Vec<_>>();
                    item_candidate_trace.push(json!({"itemId":id,"recipientId":rid,
                        "productTraceLine":hit.line,"productTraceStart":utf16(raw,hit.source_start),
                        "productId":item["productId"],"selectedVariantId":item["variantId"],
                        "variantCount":variant_count,"variantTruncated":variant_count>128,
                        "variants":options,"quantityCandidates":quantity_candidates,
                        "quantityTruncated":hit.quantities.len()>128,
                        "selectedQuantity":item["quantity"],"quantityConfirmed":item["quantityConfirmed"]}));
                }else{candidate_trace_truncated=true;}
            }
            let mut product_source=source_json(raw,&events[hit.evidence],&rid,&id);
            // parser.ts::productNameEvidence returns the registered model
            // token rather than the surrounding "(블랙)" color hint. Keep
            // recognition's original evidence intact; correct only the
            // Item-level source span consumed by UI highlighting.
            if chosen.is_some() {
                let evidence_text=product_source["text"].as_str().unwrap_or("");
                if evidence_text.contains('(') && evidence_text.contains(')') {
                    let original_len=evidence_text.encode_utf16().count();
                    if let Some(p)=model::products(state).into_iter().find(|p|
                        p.get("id").and_then(Value::as_str)==chosen) {
                        let mut spans:Vec<_>=model::model_names(p).iter().filter_map(|name|
                            model::find_term(&hit.raw,name,true)).collect();
                        spans.retain(|(begin,end)| {
                            *end>*begin && hit.raw[*begin..*end].encode_utf16().count()
                                <original_len
                        });
                        spans.sort_by(|a,b|
                            a.0.cmp(&b.0).then_with(||b.1.cmp(&a.1)));
                        if let Some((begin,end))=spans.first().copied() {
                            let start=hit.source_start+begin;
                            let finish=hit.source_start+end;
                            product_source["start"]=json!(utf16(raw,start));
                            product_source["end"]=json!(utf16(raw,finish));
                            product_source["text"]=json!(&hit.raw[begin..end]);
                            // TS analyzes product NameEvidence once and
                            // exposes the same narrowed source span in both
                            // Item.sources and the top-level evidence list.
                            // Previously Rust narrowed only Item.sources.
                            events[hit.evidence].start=start;
                            events[hit.evidence].end=finish;
                        }
                    }
                }
            }
            let mut sources=vec![product_source];
            if let Some(qi)=selected_qty {
                sources.push(source_json(raw,&events[qi],&rid,&id));
                events[qi].item_id=Some(id.clone());
                events[qi].recipient=Some(idx);
            }
            item["sources"] = json!(sources);
            recipients[idx]["items"].as_array_mut().expect("items").push(item);
            if events[hit.evidence].recipient.is_none() {
                events[hit.evidence].recipient=Some(idx);
                events[hit.evidence].item_id=Some(id.clone());
            }
        }
    }
    // A detached quantity is attachable only when exactly one item and one quantity
    // are unresolved within the same recipient. Otherwise it stays visible as evidence.
    for idx in 0..recipients.len() {
        let standalone: Vec<usize> = events.iter().enumerate().filter_map(|(i,e)|
            (e.kind=="quantity" && e.item_id.is_none()
                && owner_index(&anchors,&events,e.start)==idx
                && !product_hits.iter().any(|h| h.line==e.line)).then_some(i)
        ).collect();
        let pending: Vec<usize> = recipients[idx]["items"].as_array().expect("items")
            .iter().enumerate().filter_map(|(i,it)|it["quantity"].is_null().then_some(i)).collect();
        if standalone.len()==1 && pending.len()==1 {
            let q = standalone[0];
            let item_idx = pending[0];
            let rid = recipients[idx]["id"].as_str().unwrap_or("").to_owned();
            let id = recipients[idx]["items"][item_idx]["id"].as_str().unwrap_or("").to_owned();
            if let Ok(value) = events[q].value.parse::<i64>() {
                recipients[idx]["items"][item_idx]["quantity"]=json!(value);
                recipients[idx]["items"][item_idx]["quantityConfirmed"]=json!(true);
                if let Some(issues) = recipients[idx]["items"][item_idx]["issues"].as_array_mut() {
                    issues.retain(|s|!s.as_str().unwrap_or("").contains("수량"));
                }
                recipients[idx]["items"][item_idx]["sources"].as_array_mut().expect("item sources")
                    .push(source_json(raw,&events[q],&rid,&id));
                events[q].recipient=Some(idx);
                events[q].item_id=Some(id);
            }
        }
    }
    // Attach evidence to the outer rows, but do not fabricate product/quantity ownership.
    // Source order-evidence.ts sorts all evidence by source position before
    // assigning and returning candidates. A quantity inserted ahead of its
    // corresponding product in Rust must not reverse the public array order.
    let mut ordered=events.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|e|(e.start,e.end));
    let mut unassigned=Vec::new();
    for e in &ordered {
        if e.recipient.is_none(){unassigned.push(e_json(raw,e)["id"].clone());}
    }
    for e in &events {
        if (e.kind=="product" || e.kind=="quantity") && e.recipient.is_none() {
            let idx=owner_index(&anchors,&events,e.start);
            // Indexing a missing JSON key creates null. Check the array
            // before indexing; otherwise the first unassigned evidence is
            // silently dropped and its shipping warning disappears.
            if !recipients[idx].get("unassignedEvidence")
                .and_then(Value::as_array).is_some() {
                recipients[idx]["unassignedEvidence"]=json!([]);
            }
            recipients[idx]["unassignedEvidence"].as_array_mut()
                .expect("unassigned evidence array").push(e_json(raw,e));
        }
    }
    let candidates: Vec<Value> = ordered.iter().map(|e| {
        let mut candidate=e_json(raw,e);
        if let Some(idx) = e.recipient {
            candidate["recipientId"]=recipients[idx]["id"].clone();
        }
        candidate
    }).collect();
    let mut result=json!({"recipients":recipients,"candidates":candidates,"unassigned":unassigned});
    if let Some(trace)=owner_trace {
        result["recipientOwnerTrace"]=json!({"schemaVersion":1,"scorePolicyVersion":"owner-margin-30-v1","decisions":trace});
    }
    if collect_candidate_trace{
        result["classificationCandidateTrace"]=json!({"schemaVersion":1,
            "coverage":"product_stages_variant_selection_and_quantity_post_extraction",
            "truncated":candidate_trace_truncated,"decisions":candidate_trace,
            "itemDecisions":item_candidate_trace,
            "typeGateRecords":type_trace,"typeGateRecordsTruncated":type_trace_truncated,
            "fullCandidateCoverage":false});
    }
    Ok(result)
}

fn reanalyze(payload: &Value) -> Result<Value, String> {
    let old = payload.get("order").ok_or("reanalyze_order requires order")?;
    if !old.is_object() { return Err("order must be an object".into()); }
    let state = required_state(payload)?;
    let mut next = old.clone();
    // parser.ts::reanalyzeWithAliases only skips automatic parsing when
    // manualClassification is truthy, not merely present (false is valid).
    let manually_classified=match old.get("manualClassification") {
        None|Some(Value::Null)|Some(Value::Bool(false))=>false,
        Some(Value::Number(n))=>n.as_f64().map_or(false,|n|n!=0.0),
        Some(Value::String(s))=>!s.is_empty(),
        _=>true,
    };
    if manually_classified {return Ok(next);}
    let raw = old.get("analysisRaw").and_then(Value::as_str)
        .or_else(||old.get("raw").and_then(Value::as_str))
        .ok_or("order needs raw or analysisRaw")?;
    let client = state.get("clients").and_then(Value::as_array)
        .and_then(|xs|xs.iter().find(|c|
            c.get("id").and_then(Value::as_str)==old.get("clientId").and_then(Value::as_str)))
        .or_else(||old.get("clientSnapshot"));
    let analysis = analyze(raw,state,client)?;
    let mut incoming = analysis["recipients"].as_array().cloned().unwrap_or_default();
    let prior = old.get("recipients").and_then(Value::as_array)
        .ok_or("order.recipients must be an array")?;
    let mut used=HashSet::<usize>::new();
    let mut recipient_rewrites=HashMap::<String,String>::new();
    let mut item_rewrites=HashMap::<String,String>::new();
    // order-evidence.ts::matchingRecipient receives the complete original
    // parsed rows, not partially updated rows from the reanalysis loop.
    let identity_rows=incoming.clone();
    for row in &mut incoming {
        let Some(index)=ts_semantics::matching_recipient(
            row,prior,&used,&identity_rows
        ) else {continue;};
        used.insert(index);
        let old_row=&prior[index];
        if let (Some(from),Some(to))=(row["id"].as_str(),old_row["id"].as_str()) {
            recipient_rewrites.insert(from.to_owned(),to.to_owned());
        }
        row["id"]=old_row["id"].clone();
        for key in ["shippingCharges","freightCode","vatDefault","vatOverride"] {
            if let Some(v)=old_row.get(key) {row[key]=v.clone();}
        }
        let confirmed=old_row.get("confirmedFields").and_then(Value::as_array)
            .cloned().unwrap_or_default();
        if let Some(v)=old_row.get("confirmedFields") {
            row["confirmedFields"]=v.clone();
        } else if let Some(object)=row.as_object_mut(){
            object.remove("confirmedFields");
        }
        for field in confirmed.iter().filter_map(Value::as_str) {
            if ["name","address","phone","mobile","note"].contains(&field) {
                let same_address=field=="address" && row["address"]==old_row["address"];
                if let Some(value)=old_row.get(field) { row[field]=value.clone(); }
                if field=="address" {
                    if let Some(v)=old_row.get("postcode"){row["postcode"]=v.clone();}
                    else if !same_address{row.as_object_mut().unwrap().remove("postcode");}
                    if let Some(v)=old_row.get("addressStatus"){row["addressStatus"]=v.clone();}
                    if let Some(v)=old_row.get("addressParts"){row["addressParts"]=v.clone();}
                }
            }
        }
        if let (Some(next_items),Some(old_items)) =
            (row["items"].as_array_mut(),old_row["items"].as_array()) {
            let mut consumed=HashSet::new();
            for item in next_items.iter_mut() {
                let matched:Vec<usize>=old_items.iter().enumerate().filter_map(|(i,p)|
                    (!consumed.contains(&i) && p["raw"]==item["raw"]).then_some(i)).collect();
                if matched.len()!=1 {continue;}
                let i=matched[0];
                consumed.insert(i);
                let previous=&old_items[i];
                let old_quantity_sources:Vec<_>=previous["sources"].as_array().into_iter()
                    .flatten().filter(|s|s["field"]=="quantity").map(|s|s["text"].clone()).collect();
                let new_quantity_sources:Vec<_>=item["sources"].as_array().into_iter()
                    .flatten().filter(|s|s["field"]=="quantity").map(|s|s["text"].clone()).collect();
                let changed=old_quantity_sources!=new_quantity_sources;
                let detected_quantity=item["quantity"].clone();
                let detected_confirmation=item["quantityConfirmed"].clone();
                let detected_issues=item["issues"].clone();
                let new_sources=item["sources"].clone();
                let new_id=previous["id"].clone();
                if let (Some(from),Some(to))=(item["id"].as_str(),new_id.as_str()) {
                    item_rewrites.insert(from.to_owned(),to.to_owned());
                }
                if previous["manuallyConfirmed"]==true {
                    *item=previous.clone();
                }
                item["id"]=new_id.clone();
                item["sources"]=new_sources;
                if changed {
                    item["quantity"]=detected_quantity;
                    item["quantityConfirmed"]=detected_confirmation;
                    if let Some(issues)=item["issues"].as_array_mut(){
                        issues.retain(|v|!v.as_str().unwrap_or("").contains("수량"));
                        if let Some(new_issues)=detected_issues.as_array() {
                            issues.extend(new_issues.iter().filter(|v|v.as_str()
                                .unwrap_or("").contains("수량")).cloned());
                        }
                    }
                    if let Some(object)=item.as_object_mut(){object.remove("quantityApproval");}
                }
                if let Some(sources)=item["sources"].as_array_mut() {
                    for source in sources {
                        source["targetId"]=new_id.clone();
                        source["recipientId"]=old_row["id"].clone();
                    }
                }
            }
            // A changed product raw may deliberately prevent preserving
            // the previous Item ID. The new Item must nonetheless refer
            // to the *stable recipient* rather than the temporary analysis
            // recipient that was just replaced. Upstream TS currently
            // omits this rebinding for unmatched items (source defect).
            for item in next_items.iter_mut() {
                let item_id=item["id"].clone();
                if let Some(sources)=item["sources"].as_array_mut() {
                    for source in sources {
                        source["recipientId"]=old_row["id"].clone();
                        source["targetId"]=item_id.clone();
                    }
                }
            }
        }
        if let Some(sources)=row["sources"].as_array_mut() {
            for source in sources {source["targetId"]=old_row["id"].clone();
                source["recipientId"]=old_row["id"].clone();}
        }
    }
    // Candidate ownership has to refer to the same stable IDs as the rows.
    let mut candidates=analysis["candidates"].as_array().cloned().unwrap_or_default();
    for c in &mut candidates {
        if let Some(from)=c["recipientId"].as_str() {
            if let Some(to)=recipient_rewrites.get(from){c["recipientId"]=json!(to);}
        }
        if let Some(from)=c["itemId"].as_str() {
            if let Some(to)=item_rewrites.get(from){c["itemId"]=json!(to);}
        }
    }
    next["recipients"]=json!(incoming);
    next["analysisEvidence"]=json!(candidates);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Value {
        json!({"products":[
            {"id":"p709","code":"709","name":"709 / 타공 상판","category":"보면대",
             "origin":"FACTORY","active":true,"variants":[{"id":"v709","color":"Black"}]},
            {"id":"p709-1","code":"709-1","name":"709-1 / 비타공 상판","category":"보면대",
             "origin":"FACTORY","active":true,"variants":[{"id":"v709-1","color":"Black"}]},
            {"id":"pgt","code":"GT-1","name":"GT-1 / 기타 스탠드","category":"기타",
             "origin":"FACTORY","active":true,"variants":[{"id":"vgt","color":"Black"}]}
        ],"aliases":[],"clients":[]})
    }
    #[test]
    fn contract_rejects_incomplete_state() {
        assert!(execute("analyze_order",json!({"raw":"709 1개","state":{}})).is_err());
        assert!(execute("invalid",json!({})).is_err());
    }
    #[test]
    fn production_reception_excludes_demo_catalogs_and_demo_variants_in_rust() {
        let mut data=state();
        data["products"].as_array_mut().unwrap().push(json!({
            "id":"demo-catalog","code":"D-700","name":"가상보면대 D-700",
            "catalog":"DEMO","category":"보면대","origin":"FACTORY",
            "active":true,"variants":[{"id":"dv","color":"Black","source":"MANUAL"}]
        }));
        data["products"].as_array_mut().unwrap().push(json!({
            "id":"demo-variant","code":"D-701","name":"가상보면대 D-701",
            "catalog":"BANDOSTAND","category":"보면대","origin":"FACTORY",
            "active":true,"variants":[{"id":"dv2","color":"Black","source":"DEMO"}]
        }));
        let raw=[
            "김가상","010-1234-5678","서울특별시 마포구 월드컵로 20",
            "709 1개","가상보면대 D-700 1개","가상보면대 D-701 1개"
        ].join("\n");
        let output=execute("analyze_order",json!({"raw":raw,"state":data})).unwrap();
        let mut recognized_real=false;
        for recipient in output["recipients"].as_array().unwrap() {
            for item in recipient["items"].as_array().unwrap() {
                assert_ne!(item["productId"],"demo-catalog");
                assert_ne!(item["productId"],"demo-variant");
                if item["productId"]=="p709" {recognized_real=true;}
            }
        }
        assert!(recognized_real,"A registered non-DEMO product must still match");
    }
    #[test]
    fn no_quantity_is_approved_without_evidence() {
        let out=execute("analyze_order",json!({"raw":"709 민판\n김가상\n010-1234-5678\n경기도 의정부시 시민로 100","state":state()})).unwrap();
        let item=&out["recipients"][0]["items"][0];
        assert_eq!(item["quantity"],Value::Null);
        assert_eq!(item["quantityConfirmed"],false);
        assert_ne!(item["productId"],"p709");
    }
    #[test]
    fn recipient_field_has_no_name_shape_or_length_restriction(){
        let names=["John Smith","BANDO Music & Sound","(주)대한악기","A/S 담당자","ABC-123",
            "홍길동님","(김가상)","수취인: 연구소","A","123","山田 太郎","🎵 Music / 2F",
            "서울 음악 연구소","BD 709 2개","국제공연예술교육지원센터(교육기획팀)(John Smith)(3층)"];
        for name in names{
            let raw=format!("수취인: {name}\n연락처: 010-1234-5678\n주소: 서울특별시 마포구 월드컵로 20\n709 1개");
            let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
            let rows=out["recipients"].as_array().unwrap();
            assert_eq!(rows.len(),1,"{name}: {out}");
            assert_eq!(rows[0]["name"],name,"{out}");
            assert_eq!(rows[0]["mobile"],"010-1234-5678");
            assert_eq!(rows[0]["items"].as_array().unwrap().len(),1);
            assert!(rows[0]["sources"].as_array().unwrap().iter().any(|v|v["field"]=="name"&&v["text"]==name));
        }
        let raw="709 1개\n수취인: John Smith 주소 서울특별시 마포구 월드컵로 20 연락처: 010-1234-5678";
        let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        assert_eq!(out["recipients"][0]["name"],"John Smith");
        assert_eq!(out["recipients"][0]["mobile"],"010-1234-5678");
        let long="A very long international organization and department name ".repeat(8);
        let raw=format!("수취인: {}\n010-1234-5678",long.trim_end());
        let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        assert_eq!(out["recipients"][0]["name"],long.trim_end());
    }
    #[test]
    fn structurally_bounded_unlabelled_recipient_preserves_whole_value(){
        for name in ["John Smith","BANDO Music & Sound","(주)대한악기","홍길동님","(김가상)","A/S 담당자"]{
            for raw in [format!("709 1개\n{name}\n010-1234-5678\n서울특별시 마포구 월드컵로 20"),
                format!("709 1개\n{name} 010-1234-5678 서울특별시 마포구 월드컵로 20")]{
                let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
                assert_eq!(out["recipients"].as_array().unwrap().len(),1,"{out}");
                assert_eq!(out["recipients"][0]["name"],name,"{out}");
                assert_eq!(out["recipients"][0]["mobile"],"010-1234-5678");
                assert_eq!(out["recipients"][0]["items"][0]["quantity"],1);
            }
        }
    }
    #[test]
    fn explicitly_labeled_recipient_with_title_preserves_name_and_contacts() {
        for (label, title) in [("받는 사람", "선생님"), ("수취인:", "목사"), ("", "선생님"), ("", "목사")] {
            let raw = format!(
                "배송지 주소 경기도 의정부시 시민로 100\n{label} 이가상 {title}\n연락처 031-123-4567 / 010-1234-5678\n709 1개"
            );
            let output = execute("analyze_order", json!({"raw":raw,"state":state()})).unwrap();
            let rows = output["recipients"].as_array().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["name"], format!("이가상 {title}"));
            assert_eq!(rows[0]["phone"], "031-123-4567");
            assert_eq!(rows[0]["mobile"], "010-1234-5678");
            assert_eq!(rows[0]["items"][0]["quantity"], 1);
            assert!(rows[0]["sources"].as_array().unwrap().iter().any(|source|
                source["field"] == "name" &&
                source["text"].as_str().is_some_and(|text| text == format!("이가상 {title}"))
            ));
        }
    }
    #[test]
    fn separate_recipients_and_utf16_spans() {
        let raw="김가상\n010-1234-5678\n경기도 의정부시 시민로 100\n709 2개\n박가상\n010-2345-6789\n서울특별시 마포구 월드컵로 20\nGT-1 1개";
        let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        assert_eq!(out["recipients"].as_array().unwrap().len(),2);
        assert_eq!(out["recipients"][0]["items"][0]["quantity"],2);
        assert_eq!(out["recipients"][1]["items"][0]["quantity"],1);
        let contact=out["candidates"].as_array().unwrap().iter()
            .find(|v|v["kind"]=="mobile").unwrap();
        let start=contact["start"].as_u64().unwrap() as usize;
        let end=contact["end"].as_u64().unwrap() as usize;
        let encoded:Vec<u16>=raw.encode_utf16().collect();
        assert_eq!(String::from_utf16(&encoded[start..end]).unwrap(),"010-1234-5678");
    }
    #[test]
    fn address_place_name_cannot_be_reused_as_nine_item_quantity() {
        // Fictional recipient names/phones; retain the reported street and
        // apartment-name shape that previously produced "구대" => 9 stands.
        let raw="안녕하세요 직발 요청드립니다\n\n김가상\n010-1234-5678\n전남광주통합특별시 순천시 가곡길 39 (가곡동) 순천염광교회\nBD 709 (블랙) 4개\n메모 문 앞에 놓아주세요\n\n박가상\n010-2345-6789\n전라남도 순천시 팔마1길 25 (연향동, 연향3지구대주피오레) 101동 1003호\n반도 뮤직트레이 그레이 1개\n메모 문 앞에 놓아주세요";
        let output=execute("analyze_order",json!({"raw":raw,"state":state()}))
            .expect("the real Rust recognition engine must analyze this reception");
        let recipients=output["recipients"].as_array().unwrap();
        assert_eq!(recipients.len(),2,"The two customers must remain two recipients: {output}");
        let quantities=output["candidates"].as_array().unwrap().iter()
            .filter(|candidate|candidate["kind"]=="quantity")
            .collect::<Vec<_>>();
        assert_eq!(quantities.len(),2,"Only the two explicit product quantities are evidence: {output}");
        assert!(quantities.iter().any(|q|q["value"]=="4"));
        assert!(quantities.iter().any(|q|q["value"]=="1"));
        assert!(!quantities.iter().any(|q|q["value"]=="9"));
        let addresses=output["candidates"].as_array().unwrap().iter()
            .filter(|candidate|candidate["kind"]=="address")
            .collect::<Vec<_>>();
        assert!(addresses.iter().any(|a|a["text"].as_str().unwrap_or("")
            .contains("연향3지구대주피오레")),"The entire apartment name remains address evidence");
    }
    #[test]
    fn overlapping_address_cannot_emit_independent_quantity_evidence() {
        // Boundary-aware numeric extraction also must not reuse an address
        // span when an address legitimately contains a stand-like number.
        let raw="김가상\n서울특별시 마포구 월드컵로 20 한개";
        let output=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        assert!(output["candidates"].as_array().unwrap().iter().all(|c|
            c["kind"]!="quantity"),"Quantity spans already owned by an address must be excluded: {output}");
    }
    #[test]
    fn nearby_contacts_follow_their_recipient_block_across_blank_lines(){
        // The second consignee's address and contact are separated from
        // the first by blank lines. Distance is evidence of grouping, not
        // evidence that these are two different ordering dealers.
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n\n\n박가상\n경기도 성남시 분당구 불정로 6\n010-2345-6789";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let recipients=result["recipients"].as_array().unwrap();
        assert_eq!(recipients.len(),2);
        assert_eq!(recipients[0]["name"],"김가상");
        assert_eq!(recipients[0]["mobile"],"010-1234-5678");
        assert_eq!(recipients[1]["name"],"박가상");
        assert_eq!(recipients[1]["mobile"],"010-2345-6789");
        assert!(recipients[0]["address"].as_str().unwrap().contains("월드컵로 20"));
        assert!(recipients[1]["address"].as_str().unwrap().contains("불정로 6"));
    }
    #[test]
    fn early_type_trace_preserves_legacy_result(){
        let raw="수취인: 010-1234-5678\n709 2개\n메모: 문 앞";
        let baseline=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let mut traced=execute("analyze_order",json!({"raw":raw,"state":state(),"collectCandidateTrace":true})).unwrap();
        let trace=traced.as_object_mut().unwrap().remove("classificationCandidateTrace").unwrap();
        assert_eq!(traced,baseline);
        let records=trace["typeGateRecords"].as_array().unwrap();
        assert!(records.iter().any(|r|r["reasonCode"]=="overlaps_name"));
        assert!(records.iter().any(|r|r["reasonCode"]=="note_label"));
        assert_eq!(trace["fullCandidateCoverage"],false);
    }
    #[test]
    fn candidate_trace_does_not_change_recognition_and_records_real_stage_survivors(){
        let raw="709 민판 2개";
        let mut catalog=state();
        catalog["products"][1]["defaultColor"]=json!("Black");
        let baseline=execute("analyze_order",json!({"raw":raw,"state":catalog})).unwrap();
        let mut traced=execute("analyze_order",json!({"raw":raw,"state":catalog,
            "collectCandidateTrace":true})).unwrap();
        let trace=traced.as_object_mut().unwrap().remove("classificationCandidateTrace").unwrap();
        assert_eq!(traced,baseline);
        let stages=trace["decisions"][0]["productStages"].as_array().unwrap();
        assert!(stages.iter().any(|s|s["stage"]=="literal_evidence"));
        assert!(stages.iter().any(|s|s["stage"]=="perforation_filter"));
        assert!(stages.iter().any(|s|s["stage"]=="final_decision"));
        assert_eq!(trace["decisions"][0]["quantityAfterSaleUnitFilter"][0]["value"],2);
        assert_eq!(trace["itemDecisions"][0]["selectedQuantity"],2);
        assert_eq!(trace["fullCandidateCoverage"],false);
    }

    #[test]
    fn owner_replay_recomputes_source_links_without_changing_original_analysis(){
        let raw="김가상\n010-1234-5678\n박가상";
        let catalog=state();
        let first=execute("analyze_order",json!({"raw":raw,"state":catalog,"collectOwnerTrace":true,"collectCandidateTrace":true})).unwrap();
        assert_eq!(first["recipients"][0]["mobile"],"010-1234-5678");
        let d=first["recipientOwnerTrace"]["decisions"].as_array().unwrap().iter()
            .find(|d|d["source"]["kind"]=="mobile").unwrap();
        let decision=json!({"start":d["source"]["start"],"end":d["source"]["end"],
            "status":"diagnostic_proposal_only","legacyCandidate":"previous","diagnosticCandidate":"next",
            "calibrationScope":"conditional_traced_owner_pair_v1","decisionChanged":false,"eligibleForAutomaticDecision":false});
        let mut payload=json!({"raw":raw,"state":catalog,"initialAnalysis":first,"decision":decision});
        let result=execute("replay_owner_recommendation",payload.clone()).unwrap();
        assert_eq!(result["recognition"]["recipients"][0]["mobile"],"");
        assert_eq!(result["recognition"]["recipients"][1]["mobile"],"010-1234-5678");
        assert_eq!(result["recognition"]["candidates"].as_array().unwrap().iter()
            .find(|d|d["kind"]=="mobile").unwrap()["recipientId"],result["recognition"]["recipients"][1]["id"]);
        assert_eq!(payload["initialAnalysis"]["recipients"][0]["mobile"],"010-1234-5678");
        assert_eq!(result["decisionChanged"],false);
        payload["initialAnalysis"]["recipients"][1]["mobile"]=json!("010-9999-9999");
        assert!(execute("replay_owner_recommendation",payload).is_err());
    }
    #[test]
    fn owner_trace_preserves_legacy_attribution_and_records_rejected_candidates(){
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n\n경기도 성남시 분당구 불정로 6\n010-2345-6789\n박가상";
        let baseline=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let mut traced=execute("analyze_order",json!({
            "raw":raw,"state":state(),"collectOwnerTrace":true
        })).unwrap();
        let trace=traced.as_object_mut().unwrap().remove("recipientOwnerTrace").unwrap();
        assert_eq!(traced,baseline,"observation mode may not alter legacy output");
        assert_eq!(trace["schemaVersion"],1);
        let decisions=trace["decisions"].as_array().unwrap();
        assert!(decisions.iter().any(|d|
            d["legacyFirstBlockingReason"]=="paragraph_boundary" &&
            d["candidates"][1]["legacyScore"].is_null()));
        let chosen=decisions.iter().find(|d|
            d["source"]["kind"]=="address" && d["legacyFirstBlockingReason"].is_null()
        ).expect("address before next recipient must be evaluated");
        assert_eq!(chosen["selectedAnchor"],chosen["candidates"][1]["anchor"]);
        assert!(chosen["candidates"][1]["legacyScore"].as_u64().unwrap()>=
            chosen["candidates"][0]["legacyScore"].as_u64().unwrap()+30);
        assert_eq!(chosen["candidates"][1]["sameParagraph"],true);
        assert!(chosen["source"].get("text").is_none());
    }

    #[test]
    fn owner_trace_records_contact_gate_without_changing_recipient(){
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n\n010-3456-7890\n박가상";
        let result=execute("analyze_order",json!({
            "raw":raw,"state":state(),"collectOwnerTrace":true
        })).unwrap();
        assert_eq!(result["recipients"][0]["phone"],"010-3456-7890");
        let decisions=result["recipientOwnerTrace"]["decisions"].as_array().unwrap();
        let rejected=decisions.iter().find(|d|
            d["legacyFirstBlockingReason"]=="contact_without_nearby_address"
        ).expect("uncorroborated contact must retain its rejection reason");
        assert_eq!(rejected["selectedAnchor"],rejected["candidates"][0]["anchor"]);
        assert_eq!(rejected["nearbyAddress"],false);
        assert_eq!(rejected["candidates"][1]["sameParagraph"],true);
    }

    #[test]
    fn address_and_phone_before_next_name_use_new_paragraph_and_proximity(){
        // A second delivery may list address/phone BEFORE its addressee.
        // Its own nearby address corroborates the contact; the blank line
        // rules out simply appending both fields to the previous recipient.
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n\n경기도 성남시 분당구 불정로 6\n010-2345-6789\n박가상";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["name"],"김가상");
        assert_eq!(rows[0]["mobile"],"010-1234-5678");
        assert!(rows[0]["address"].as_str().unwrap().contains("월드컵로 20"));
        assert_eq!(rows[1]["name"],"박가상");
        assert_eq!(rows[1]["mobile"],"010-2345-6789");
        assert!(rows[1]["address"].as_str().unwrap().contains("불정로 6"));
    }
    #[test]
    fn uncorroborated_contact_before_next_name_does_not_jump_recipient(){
        // A lone phone between blocks is not enough to infer its owner.
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n\n010-3456-7890\n박가상";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["mobile"],"010-1234-5678");
        assert_eq!(rows[0]["phone"],"010-3456-7890");
        assert!(rows[1]["mobile"].as_str().unwrap_or("").is_empty());
    }
    #[test]
    fn second_contact_can_still_belong_to_same_recipient(){
        // Physical separation alone cannot invent a second recipient.
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n010-3456-7890";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let recipients=result["recipients"].as_array().unwrap();
        assert_eq!(recipients.len(),1);
        assert_eq!(recipients[0]["mobile"],"010-1234-5678");
        assert_eq!(recipients[0]["phone"],"010-3456-7890");
    }
    #[test]
    fn content_blank_and_whitespace_only_lines_are_distinct_evidence() {
        let raw="김가상\n배송 전 연락\n  \n\t\n010-1234-5678";
        assert_eq!(intervening_lines(raw,0,4),InterveningLines {
            content_lines:1,blank_lines:2,max_consecutive_blank_lines:2,
        });
        let crlf="김가상\r\n  \r\n서울특별시 마포구 월드컵로 20";
        assert_eq!(intervening_lines(crlf,0,2),InterveningLines {
            content_lines:0,blank_lines:1,max_consecutive_blank_lines:1,
        });
        let content=physical_lines("김가상\n배송 전 연락\n010-1234-5678");
        assert_eq!(content.len(),3);
        assert_eq!(content[0].paragraph,content[2].paragraph,
            "ordinary content must not invent a block boundary");
    }
    #[test]
    fn whitespace_only_delimiter_keeps_address_and_phone_with_next_recipient() {
        let raw="김가상\n서울특별시 마포구 월드컵로 20\n010-1234-5678\n  \t\n경기도 성남시 분당구 불정로 6\n010-2345-6789\n박가상";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["name"],"김가상");
        assert_eq!(rows[0]["mobile"],"010-1234-5678");
        assert!(rows[0]["address"].as_str().unwrap().contains("월드컵로 20"));
        assert_eq!(rows[1]["name"],"박가상");
        assert_eq!(rows[1]["mobile"],"010-2345-6789");
        assert!(rows[1]["address"].as_str().unwrap().contains("불정로 6"));
        assert!(!rows[0]["address"].as_str().unwrap().contains("불정로 6"));
    }
    #[test]
    fn whitespace_only_lines_do_not_create_recipients_without_name_evidence() {
        let raw="김가상\n \t\n서울특별시 마포구 월드컵로 20\n\t\n010-1234-5678\n010-2345-6789";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0]["name"],"김가상");
        assert_eq!(rows[0]["mobile"],"010-1234-5678");
        assert_eq!(rows[0]["phone"],"010-2345-6789");
        assert!(rows[0]["address"].as_str().unwrap().contains("월드컵로 20"));
    }
    #[test]
    fn blank_after_first_name_must_not_steal_its_delivery_for_next_name() {
        // No independent delivery evidence for the first name exists before
        // this blank: proximity to the later name cannot prove ownership.
        for blank in ["\n\n","\n  \t\n"] {
            let raw=format!("김가상{blank}서울특별시 마포구 월드컵로 20\n010-1234-5678\n박가상");
            let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
            let rows=result["recipients"].as_array().unwrap();
            assert_eq!(rows.len(),2);
            assert_eq!(rows[0]["mobile"],"010-1234-5678");
            assert!(rows[0]["address"].as_str().unwrap().contains("월드컵로 20"));
            assert!(rows[1]["mobile"].as_str().unwrap_or("").is_empty());
            assert!(rows[1]["address"].as_str().unwrap_or("").is_empty());
        }
    }
    #[test]
    fn crlf_whitespace_block_boundary_keeps_phone_ownership() {
        let raw="김가상\r\n서울특별시 마포구 월드컵로 20\r\n010-1234-5678\r\n  \t\r\n경기도 성남시 분당구 불정로 6\r\n010-2345-6789\r\n박가상";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["mobile"],"010-1234-5678");
        assert_eq!(rows[1]["mobile"],"010-2345-6789");
    }
    #[test]
    fn shipping_memo_label_must_not_swallow_next_recipient_address(){
        assert_eq!(address_begin("배송지메모: 가상소매점"),None);
        assert_eq!(address_begin("배송지메시지: 경기도"),None);
        assert_eq!(address_begin("배송지: 서울특별시 마포구 월드컵로 32"),
            Some("배송지: ".len()));
        let raw="배송지메모: 가상소매점\n최가상\n010-7000-1002\n서울특별시 마포구 월드컵로 32\n709 1개";
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0]["name"],"최가상");
        assert_eq!(rows[0]["address"],"서울특별시 마포구 월드컵로 32");
        assert_eq!(rows[0]["mobile"],"010-7000-1002");
    }
    #[test]
    fn address_heading_is_not_a_shipping_address_candidate() {
        for title in ["배송지 주소","배송지:","주소","주소:","배송주소","받는곳",
            "수취인주소","배송지 주소:"] {
            assert_eq!(address_begin(title),None,
                "{title}: an empty field label is not an address");
        }
        assert_eq!(address_begin("배송지: 경상남도 함양군 함양읍 함양여중길 18-9"),
            Some("배송지: ".len()));
        assert_eq!(address_begin("배송지 주소: 경상남도 함양군 함양읍 함양여중길 18-9"),
            Some("배송지 주소: ".len()));
        assert_eq!(address_begin("주소: (50034) 경상남도 함양군 함양읍 함양여중길 18-9"),
            Some("주소: (50034) ".len()));
        assert_eq!(address_begin("주소: 50034 경상남도 함양군 함양읍 함양여중길 18-9"),
            Some("주소: 50034 ".len()));
    }
    #[test]
    fn bracketed_postcode_is_not_part_of_delivery_address_or_evidence() {
        // Synthetic recipient and apartment name; preserve the shape of the
        // reported 5-digit bracketed postcode + Jeju road-name address.
        let address="제주특별자치도 서귀포시 대정읍 테스트로 23 (대정읍, 가상아파트) 105동 403호";
        let raw=format!(
            "안녕하세요 직발송 부탁드립니다.\n\n반도 첼로스탠드 BC-105 1\n수령자 : 김가상\n휴대전화 : 010-0000-0000\n주소 : [63644] {address}"
        );
        let expected_start=raw.find(address).unwrap();
        let result=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),1,"A postcode must not create another recipient: {result}");
        assert_eq!(rows[0]["address"],address);
        assert_eq!(rows[0]["postcode"],"63644");
        assert_eq!(rows[0]["addressKind"],"ROAD");
        assert_eq!(rows[0]["addressParts"]["province"],"제주특별자치도");
        assert_eq!(rows[0]["addressParts"]["detail"],
            "(대정읍, 가상아파트) 105동 403호");
        let address_evidence=result["candidates"].as_array().unwrap().iter()
            .find(|item|item["kind"]=="address").unwrap();
        assert_eq!(address_evidence["value"],address);
        assert_eq!(address_evidence["text"],address);
        assert_eq!(address_evidence["start"].as_u64().unwrap() as usize,
            raw[..expected_start].encode_utf16().count());
        assert!(!address_evidence["text"].as_str().unwrap().contains("[63644]"));
    }

    #[test]
    fn bracketed_postcode_before_inline_address_does_not_become_recipient_name() {
        let alone="[63644] ";
        assert_eq!(composite_name_before_address(alone),None);
        let prefix="김가상 [63644] ";
        assert_eq!(composite_name_before_address(prefix),Some((0,"김가상".len())));
        let prefix_old="김가상 [663644] ";
        assert_eq!(composite_name_before_address(prefix_old),Some((0,"김가상".len())));
    }

    #[test]
    fn optional_postal_markers_are_skipped_only_at_an_address_boundary() {
        let address="제주특별자치도 서귀포시 대정읍 테스트로 23 (대정읍, 가상아파트) 105동 403호";
        for marker in ["[63644]","(63644)","63644","[663644]"] {
            let line=format!("주소 : {marker} {address}");
            assert_eq!(address_begin(&line),line.find(address),
                "Postal marker {marker:?} should not enter the address source span");
            let out=execute("analyze_order",json!({
                "raw":format!("수령자: 김가상\n{line}"),"state":state()
            })).unwrap();
            assert_eq!(out["recipients"][0]["address"],address,"{marker}");
        }
        // Square brackets do not make a standalone address, and apartment
        // parentheticals must not be mistaken for postal-code delimiters.
        assert_eq!(address_begin("주소: [63644]"),None);
        assert_eq!(address_begin("주소: [663644]"),None);
        let raw=format!("수령자: 김가상\n{address}");
        let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        assert_eq!(out["recipients"][0]["address"],address);
    }

    #[test]
    fn semantic_conductor_uses_registered_single_sku_only() {
        let state=serde_json::from_str::<Value>(include_str!(
            "../../../resources/state-seed.json"
        )).expect("catalog");
        let matched=execute("match_products",json!({
            "text":"BANDO STAND 반도 보면대 지휘용 악보 스탠드",
            "state":state
        })).unwrap();
        assert_eq!(matched["productId"],"bando-site-63");
        assert_eq!(matched["variantId"],"bando-63-single");
        assert_eq!(matched["match"]["kind"],"SEMANTIC_NAME");
        // Explicit codes win, including unknown codes; avoid inventing a
        // conductor SKU from an unrelated model's semantic description.
        for raw in ["Z-1 지휘용 악보 스탠드","709 지휘용 악보 스탠드",
            "지휘용 마이크 스탠드"] {
            let matched=execute("match_products",json!({
                "text":raw,"state":state
            })).unwrap();
            assert_ne!(matched["productId"],"bando-site-63",
                "explicit or non-score-stand evidence must not be hijacked: {raw}");
        }
        // A future catalog containing several independent conductor
        // models must leave the exact SKU ambiguous.
        let mut duplicate_state=state.clone();
        let products=duplicate_state["products"].as_array_mut().unwrap();
        let mut competitor=products.iter().find(|p|
            p["id"]=="bando-site-63").unwrap().clone();
        competitor["id"]=json!("independent-conductor");
        competitor["code"]=json!("BD DIRECTOR");
        competitor["name"]=json!("BD Director / 지휘자 보면대");
        competitor["modelCodes"]=json!(["BD Director"]);
        products.push(competitor);
        let ambiguous=execute("match_products",json!({
            "text":"지휘용 악보 스탠드","state":duplicate_state
        })).unwrap();
        assert!(ambiguous["productId"].is_null());
        assert_eq!(ambiguous["match"]["kind"],"CATEGORY");
    }
    #[test]
    fn province_name_requires_address_boundary_not_merchant_substring(){
        for merchant in ["서울악기사","대전악기사","경기악기사","전남악기사",
            "서울특별시악기사","전라남도악기사"] {
            assert_eq!(address_begin(merchant),None,
                "A business name cannot be cut into an address fragment: {merchant}");
        }
        for full_address in [
            "서울특별시 마포구 월드컵로 20",
            "대전광역시 서구 둔산로 100",
            "경기도 의정부시 시민로 100",
            "전라남도 순천시 중앙로 40",
        ] {
            assert_eq!(address_begin(full_address),Some(0),
                "Official administrative names and street structure indicate an address: {full_address}");
        }
    }
    #[test]
    fn changed_quantity_rebinds_all_item_sources_to_stable_recipient() {
        let old_raw="GT-1 2개\n김가상\n서울특별시 마포구 월드컵로 20";
        let new_raw="GT-1 3개\n김가상\n서울특별시 마포구 월드컵로 20";
        let analyzed=execute("analyze_order",json!({
            "raw":old_raw,"state":state()
        })).expect("old recognition");
        let mut order=json!({
            "raw":old_raw,"analysisRaw":new_raw,"clientId":"sample",
            "recipients":analyzed["recipients"],"status":"RECEIVED"
        });
        order["recipients"][0]["id"]=json!("stable-recipient-0");
        let result=execute("reanalyze_order",json!({
            "order":order,"state":state()
        })).expect("reanalysis after quantity edit");
        let row=&result["recipients"][0];
        assert_eq!(row["id"],"stable-recipient-0");
        let item=&row["items"][0];
        assert_eq!(item["quantity"],3);
        for span in item["sources"].as_array().unwrap() {
            assert_eq!(span["recipientId"],row["id"],
                "new item source must point to stable matched recipient");
            assert_eq!(span["targetId"],item["id"]);
        }
    }


    #[test]
    fn percussion_center_z1_actual_raw_diagnostic() {
        // Exact SKU and document structure with synthetic recipient and
        // phone. Never commit a real customer's contact to public test code.
        // Diagnostics intentionally do not change SKU catalog or matching.
        let raw="■퍼커션센터 발주서■\nㅇ품목\nZ-1(건반 스탠드) * 1EA\n\nㅇ수령정보\n서울 송파구 석촌호수로 162 가상교회\n김가상 목사\n010-1234-5678";
        let catalog=serde_json::from_str::<Value>(include_str!(
            "../../../resources/state-seed.json"
        )).expect("360 SKU seed");
        let matched=execute("match_products",json!({
            "text":"Z-1(건반 스탠드) * 1EA",
            "state":catalog
        })).expect("native product matcher");
        let analyzed=execute("analyze_order",json!({
            "raw":raw,"state":catalog
        })).expect("native complete-document analyzer");
        println!("PERCUSSION_CENTER_Z1_MATCH {}",matched);
        println!("PERCUSSION_CENTER_Z1_ANALYSIS {}",analyzed);
        assert!(analyzed["recipients"].is_array(),
            "the native engine must return a reviewable analysis");
    }
    #[test]
    fn composite_recipient_name_preserves_unbounded_formats_as_a_whole_field() {
        let variants=[
            "청소년활동팀(1층)(김가상)",
            "(청소년활동팀)김가상",
            "청소년활동팀 김가상",
            "청소년활동팀김가상",
            "청소년활동팀(1층)김가상",
            "김가상(청소년활동팀)",
            "김가상 청소년활동팀",
            "김가상청소년활동팀",
            "김가상(1층)청소년활동팀",
            "청소년활동팀-김가상",
            "청소년활동팀/김가상",
            "청소년활동팀(행정실)(2층)김가상",
        ];
        let state=state();
        for name in variants {
            let raw=format!(
                "709 1개\n{name}  (12416) 경기도 가평군 가평읍 문화로 120 010-1234-5678"
            );
            let out=execute("analyze_order",json!({"raw":raw,"state":state}))
                .expect("production parser");
            let rows=out["recipients"].as_array().expect("recipients");
            assert_eq!(rows.len(),1,"should be one consignee: {name}");
            assert_eq!(rows[0]["name"],name,"keep entire recipient label: {name}");
            assert_eq!(rows[0]["mobile"],"010-1234-5678","phone separated: {name}");
            assert_eq!(rows[0]["items"][0]["quantity"],1);
            assert_eq!(rows[0]["candidates"].as_array().unwrap().iter()
                .filter(|c|c["field"]=="name").count(),1,
                "no duplicate short name candidates: {name}");
        }
        assert_eq!(composite_name_before_address("(김가상) (12416) "),Some((0,"(김가상)".len())));
        assert_eq!(composite_name_before_address("709 1개 (김가상) (12416) "),None);
        assert_eq!(composite_name_before_address("매장 (김가상) (12416) "),
            Some((0,"매장 (김가상)".len())));
    }
    #[test]
    fn address_headings_cannot_become_whole_recipient_names() {
        let state=state();
        for header in ["배송지 주소","배송지","배송주소","받는곳",
            "받는곳 :","수취인주소","수취인 주소","주소:","배송지 주소:","주소"] {
            let raw=format!(
                "709 1개\\n{header} 경기도 가평군 가평읍 문화로 120 010-1234-5678"
            );
            let raw=raw.replace("\\n","\n");
            let result=execute("analyze_order",json!({"raw":raw,"state":state}))
                .expect("production Rust");
            let rows=result["recipients"].as_array().unwrap();
            assert_eq!(rows.len(),1,"one delivery row: {header}");
            assert_eq!(rows[0]["name"],"","address header cannot be a person: {header}");
            assert!(rows[0]["address"].as_str().unwrap_or("").contains("문화로 120"));
        }
        // A memo/contact label isn't an address line; verify the name
        // safeguard directly without requiring address extraction.
        assert_eq!(composite_name_before_address("배송메모 :"),None);
        assert_eq!(composite_name_before_address("연락처 :"),None);
    }
    #[test]
    fn lone_parenthesized_recipient_preserves_parentheses() {
        let raw="(김가상) (12416) 경기도 가평군 가평읍 문화로 120 010-1234-5678";
        let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        assert_eq!(out["recipients"].as_array().unwrap().len(),1);
        assert_eq!(out["recipients"][0]["name"],"(김가상)");
        assert_eq!(out["recipients"][0]["mobile"],"010-1234-5678");
        assert!(out["recipients"][0]["address"].as_str().unwrap().contains("문화로 120"));
    }
    #[test]
    fn lp_wa_type_choice_is_semantic_not_catalog_order() {
        let state=serde_json::from_str::<Value>(include_str!(
            "../../../resources/state-seed.json"
        )).expect("catalog");
        let variants=[
            ("BD-LP-WA A타입",Some("bando-site-202")),
            ("BD-LP-WA B타입",Some("bando-site-355")),
            ("BD LP-WA A 타입",Some("bando-site-202")),
            ("BD LP-WA B 타입",Some("bando-site-355")),
            ("모델명 : BD-LP-WA A타입 수량 : 1ea",Some("bando-site-202")),
            ("모델명 : BD-LP-WA B타입 수량 : 1ea",Some("bando-site-355")),
            ("BD-LP-WA",None),
            ("BD-LP-WA A타입 B타입",None),
            ("BD-LP-WA 타입A",Some("bando-site-202")),
            ("BD-LP-WA 타입B",Some("bando-site-355")),
            ("BD-LP-WA 타입: B",Some("bando-site-355")),
            ("BD-LP-WA타입B",None),
            ("BD-LP-WA A형",None),
            ("BD-LP-WB A타입",Some("bando-site-201")),
            ("BD-LP-WB B타입",Some("bando-site-356")),
        ];
        let mut reversed=state.clone();
        reversed["products"].as_array_mut().unwrap().reverse();
        for (order,catalog) in [("default",&state),("reversed",&reversed)] {
            for (raw,expected) in variants {
                let parsed=execute("match_products",json!({
                    "text":raw,"state":catalog
                })).expect("native Rust matcher");
                println!("LP_WA_AB_DIFFERENTIAL {order} {raw} {}",
                    parsed["productId"]);
                assert_eq!(parsed["productId"].as_str(),expected,
                    "model-specific option must not depend on SKU order: {order}, {raw}");
                if expected.is_some() {
                    assert_eq!(parsed["match"]["kind"],"NAME");
                    assert!(parsed["variantId"].as_str().is_some());
                }
            }
        }
        // Even if the A product is missing from the catalog, "A타입"
        // must not silently become the B product under the same model.
        let mut missing_a=state.clone();
        missing_a["products"].as_array_mut().unwrap().retain(|p|
            p["id"]!="bando-site-202"
        );
        let parsed=execute("match_products",json!({
            "text":"BD-LP-WA A타입","state":missing_a
        })).unwrap();
        assert!(parsed["productId"].is_null(),
            "missing requested A-type must not silently fall back to B");
    }
    #[test]
    fn soundpro_bd_lp_wa_a_type_order_diagnostic() {
        // Faithful order format; use a fictional two-syllable name and
        // fictional mobile to avoid disclosing a customer in public CI.
        let raw="안녕하세요 ! 사운드프로 발주드립니다.\n\n모델명 : BD-LP-WA A타입 수량 : 1ea\n\n성함 : 가상\n번호 : 010-1234-5678\n주소 : 서울특별시 도봉구 도봉로133길 50 (쌍문동, 가상교회) 가상교회 사무실 앞";
        let state=serde_json::from_str::<Value>(include_str!(
            "../../../resources/state-seed.json"
        )).expect("catalog");
        let standalone=execute("match_products",json!({
            "text":"모델명 : BD-LP-WA A타입 수량 : 1ea","state":state
        })).expect("product matcher");
        let analyzed=execute("analyze_order",json!({
            "raw":raw,"state":state
        })).expect("document analyzer");
        println!("SOUNDPRO_LP_WA_MATCH {}",standalone);
        println!("SOUNDPRO_LP_WA_ANALYSIS {}",analyzed);
        assert!(analyzed["recipients"].is_array());
    }
    #[test]
    fn one_line_team_name_zip_and_phone_diagnostic() {
        // Same format as user-submitted example, with synthetic name and
        // telephone. No identifying individual is written into public CI.
        let raw="반도 Bando 409 건반 스탠드 1개\n청소년활동팀(1층)(김가상)   (12416) 경기도 가평군 가평읍 문화로 120 (가평읍, 가상 청소년 문화의 집) 1층   010-1234-5678";
        let state=serde_json::from_str::<Value>(include_str!(
            "../../../resources/state-seed.json"
        )).expect("catalog");
        let result=execute("analyze_order",json!({"raw":raw,"state":state}))
            .expect("production Rust recognition");
        println!("ONE_LINE_TEAM_ZIP_DIAGNOSTIC {}",result);
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),1,"a composite shipping label is one recipient");
        assert_eq!(rows[0]["name"],"청소년활동팀(1층)(김가상)");
        assert_eq!(rows[0]["mobile"],"010-1234-5678");
        assert_eq!(rows[0]["address"],
            "경기도 가평군 가평읍 문화로 120 (가평읍, 가상 청소년 문화의 집) 1층");
        assert_eq!(rows[0]["items"][0]["productId"],"bando-site-130");
        assert_eq!(rows[0]["items"][0]["quantity"],1);
        let names:Vec<_>=rows[0]["candidates"].as_array().unwrap().iter()
            .filter(|v|v["field"]=="name").collect();
        assert_eq!(names.len(),1,"do not split the same name into two candidates");
        assert_eq!(names[0]["value"],"청소년활동팀(1층)(김가상)");
    }
    #[test]
    fn narrative_conductor_order_with_repeated_recipient_label_diagnostic() {
        // Structure-preserving synthetic stand-in for a real staff sample:
        // no actual individual's name or phone is committed to public CI.
        let raw="안녕하세요? 주문드립니다.\nBANDO STAND 반도 보면대 지휘용 악보 스탠드\n\n받으시는 분\n김가상\n\n받으시는 분\n010-1234-5678\n\n배송지 주소\n50034 경상남도 함양군 함양읍 함양여중길 18-9 (함양여자중학교)\n\n오늘 발송 가능한가요? 발송하시고 송장번호 부탁드립니다.";
        let state=serde_json::from_str::<Value>(include_str!(
            "../../../resources/state-seed.json"
        )).expect("production catalog seed");
        let match_result=execute("match_products",json!({
            "text":"BANDO STAND 반도 보면대 지휘용 악보 스탠드",
            "state":state
        })).expect("Rust standalone product matching");
        let result=execute("analyze_order",json!({
            "raw":raw,"state":state
        })).expect("Rust actual document analysis");
        println!("NARRATIVE_CONDUCTOR_MATCH {}",match_result);
        println!("NARRATIVE_CONDUCTOR_ANALYSIS {}",result);
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),1);
        let row=&rows[0];
        assert_eq!(row["name"],"김가상");
        assert_eq!(row["mobile"],"010-1234-5678");
        assert_eq!(row["address"],
            "경상남도 함양군 함양읍 함양여중길 18-9 (함양여자중학교)");
        assert_eq!(row["addressStatus"],"COMPLETE");
        assert_eq!(row["candidates"].as_array().unwrap().iter()
            .filter(|c|c["field"]=="address").count(),1);
        let item=&row["items"][0];
        assert_eq!(item["productId"],"bando-site-63");
        assert_eq!(item["variantId"],"bando-63-single");
        assert_eq!(item["match"]["kind"],"SEMANTIC_NAME");
        assert!(item["quantity"].is_null());
        assert_eq!(item["quantityConfirmed"],false);
        assert!(result["unassigned"].as_array().unwrap().is_empty());
    }
    #[test]
    fn contact_heading_with_parenthetical_qualifier_is_not_a_recipient(){
        for heading in ["연락처(안심번호)","연락처 (안심번호)","연락처(안심번호):","TEL","휴대전화"] {
            let raw=format!("709 1개\nJohn Smith\n{heading} 0502-1234-5678\n서울특별시 마포구 월드컵로 20");
            let out=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
            let rows=out["recipients"].as_array().unwrap();
            assert_eq!(rows.len(),1,"{heading}: {out}");
            assert_eq!(rows[0]["name"],"John Smith");
            assert_eq!(rows[0]["phone"],"0502-1234-5678");
        }
    }
    #[test]
    fn manually_confirmed_recipient_value_survives_reanalysis(){
        let raw="수취인: John Smith\n010-1234-5678\n서울특별시 마포구 월드컵로 20";
        let analysis=execute("analyze_order",json!({"raw":raw,"state":state()})).unwrap();
        let mut order=json!({"raw":raw,"clientId":"c","recipients":analysis["recipients"],"status":"RECEIVED"});
        order["recipients"][0]["name"]=json!("(주) International Music & Sound / A/S 담당자님");
        order["recipients"][0]["confirmedFields"]=json!(["name"]);
        let out=execute("reanalyze_order",json!({"order":order,"state":state()})).unwrap();
        assert_eq!(out["recipients"][0]["name"],order["recipients"][0]["name"]);
        assert_eq!(out["recipients"][0]["id"],order["recipients"][0]["id"]);
    }
    #[test]
    fn manual_confirmations_survive_identical_order() {
        let analysis=execute("analyze_order",json!({"raw":"709 2개\n김가상\n서울특별시 마포구 월드컵로 20","state":state()})).unwrap();
        let mut order=json!({"raw":"709 2개\n김가상\n서울특별시 마포구 월드컵로 20",
            "clientId":"c","recipients":analysis["recipients"],"status":"RECEIVED"});
        order["recipients"][0]["items"][0]["manuallyConfirmed"]=json!(true);
        order["recipients"][0]["items"][0]["productId"]=json!("staff-picked");
        let out=execute("reanalyze_order",json!({"order":order,"state":state()})).unwrap();
        assert_eq!(out["recipients"][0]["items"][0]["productId"],"staff-picked");
    }
}

#[cfg(test)]
mod recipient_postcode_tests{
    use super::*;
    #[test]
    fn postal_codes_follow_their_address_not_the_order(){
        let state=json!({"products":[],"aliases":[],"clients":[]});
        let raw="수령자: 가상 수취인 A\n주소: [21534] 인천광역시 남동구 테스트로 1\n\n수령자: 가상 수취인 B\n주소: (12416) 경기도 가평군 테스트로 2\n\n수령자: 가상 수취인 C\n주소: 01234 서울특별시 중구 테스트로 3";
        let out=analyze(raw,&state,None).unwrap();
        let rows=out["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),3);
        for (row,code) in rows.iter().zip(["21534","12416","01234"]){
            assert_eq!(row["postcode"],code);
            assert!(!row["address"].as_str().unwrap().contains(code));
        }
    }
    #[test]
    fn separately_labelled_code_is_not_a_product_or_recipient(){
        let state=json!({"products":[],"aliases":[],"clients":[]});
        let out=analyze("수령자: 가상\n우편번호: 01234\n주소: 서울특별시 중구 테스트로 1",&state,None).unwrap();
        assert_eq!(out["recipients"].as_array().unwrap().len(),1);
        assert_eq!(out["recipients"][0]["postcode"],"01234");
        assert!(out["recipients"][0]["items"].as_array().unwrap().is_empty());
    }
}

#[cfg(test)]
mod dated_dispatch_tests{
    use super::*;
    #[test]
    fn explicit_dispatch_date_is_preserved_with_existing_note(){
        let state=json!({"products":[],"aliases":[],"clients":[]});
        for text in ["10월12일 발송","10월 12일 발송 부탁드립니다","2026년 10월 12일 출고 요청"]{
            let raw=format!("수령자: 가상 수취인\n주소: 서울특별시 중구 테스트로 1\n메모: 문 앞\n{text}");
            let result=analyze(&raw,&state,None).unwrap();
            let rows=result["recipients"].as_array().unwrap();
            assert_eq!(rows.len(),1);
            assert_eq!(rows[0]["note"],format!("문 앞\n{text}"));
            assert!(rows[0]["items"].as_array().unwrap().is_empty());
            assert!(rows[0]["sources"].as_array().unwrap().iter()
                .any(|s|s["field"]=="note" && s["text"]==text));
        }
    }
    #[test]
    fn dispatch_recognizer_does_not_turn_questions_or_order_dates_into_instructions(){
        for text in ["발주일: 10월12일","10월12일 발송 가능한가요?","10월12일 입금",
            "10월12일 배송 완료","10월12일 709 3개"]{
            assert!(!dated_dispatch_instruction(text),"{text}");
        }
    }
    #[test]
    fn each_recipient_keeps_its_local_dispatch_date(){
        let state=json!({"products":[],"aliases":[],"clients":[]});
        let raw="수령자: 가상 수취인 A\n주소: 서울특별시 중구 테스트로 1\n10월12일 발송\n\n수령자: 가상 수취인 B\n주소: 경기도 가평군 테스트로 2\n11월13일 출고";
        let result=analyze(raw,&state,None).unwrap();
        let rows=result["recipients"].as_array().unwrap();
        assert_eq!(rows.len(),2);
        assert_eq!(rows[0]["note"],"10월12일 발송");
        assert_eq!(rows[1]["note"],"11월13일 출고");
    }
}
