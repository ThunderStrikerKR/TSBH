//! Kakao window title → dealer identity rules.
//! Oracle: src/title-clients.ts; window docking remains a separate native service.
use serde_json::{json,Value};
use unicode_normalization::UnicodeNormalization;

fn s<'a>(v:&'a Value,key:&str)->&'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn list<'a>(v:&'a Value,key:&str)->&'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn rows(v:&Value)->&[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn truthy(v:Option<&Value>)->bool {
    match v {
        None|Some(Value::Null)|Some(Value::Bool(false))=>false,
        Some(Value::String(s)) if s.is_empty()=>false,
        Some(Value::Number(n)) if n.as_i64()==Some(0)=>false,
        _=>true,
    }
}
pub(crate) fn key(value:&str)->String {
    // JS: normalize("NFKC").trim().toLocaleLowerCase().replace(/\s+/g," ")
    value.nfkc().flat_map(char::to_lowercase).collect::<String>()
        .trim().split_whitespace().collect::<Vec<_>>().join(" ")
}
fn eligible_client(client:&Value,id:&str,allow_inactive:bool)->bool {
    s(client,"id")==id && !truthy(client.get("deletedAt"))
        && (allow_inactive || client.get("active")!=Some(&Value::Bool(false)))
}
fn remembered(state:&Value,windows:&Value)->Option<String> {
    let chats=rows(windows).iter()
        .filter(|w|s(w,"kind")=="CHAT").collect::<Vec<_>>();
    if chats.len()!=1{return None}
    let k=key(s(chats[0],"title"));
    let entry=state.get("settings")?.get("kakaoTitleMappings")?.get(&k)?;
    if truthy(entry.get("ambiguous")) {return None}
    let id=s(entry,"clientId");
    if list(state,"clients").iter().any(|c|eligible_client(c,id,false)){
        Some(id.to_owned())
    }else{None}
}
// The dealer is NOT the recipient. For example, a school name and two
// recipient phone numbers in an order are delivery details, not evidence that
// the school placed the order. Only explicitly marked sender/dealer fields
// qualify for automatic identification from raw text.
fn client_terms(client:&Value)->Vec<String>{
    ["name","shortNames","identifiers"].iter()
        .flat_map(|field| s(client,field).split(|ch| matches!(ch, ',' | ';' | '|' | '\n')))
        .map(str::trim).filter(|term|term.chars().count()>=2)
        .map(key).collect()
}
fn unique_client_id(state:&Value,needle:&str)->Option<String>{
    let needle=key(needle);
    if needle.is_empty(){return None;}
    let hits=list(state,"clients").iter().filter(|c|
        eligible_client(c,s(c,"id"),false) &&
        client_terms(c).iter().any(|term| term==&needle)
    ).collect::<Vec<_>>();
    if hits.len()==1{Some(s(hits[0],"id").to_owned())} else{None}
}
fn unique_title_client_id(state:&Value,title:&str)->Option<String>{
    let exact=unique_client_id(state,title);
    if exact.is_some(){return exact;}
    let title=key(title).replace(' ',"");
    let hits=list(state,"clients").iter().filter(|c|
        eligible_client(c,s(c,"id"),false) &&
        client_terms(c).iter().any(|term|{
            let term=term.replace(' ',"");
            term.chars().count()>=3 && title.contains(&term)
        })
    ).collect::<Vec<_>>();
    if hits.len()==1{Some(s(hits[0],"id").to_owned())} else{None}
}
fn confirmed_title_count(state:&Value,title:&str,client_id:&str)->usize{
    if let Some(counts)=state.get("confirmedTitleCounts"){
        return counts.get(key(title)).and_then(|v|v.get(client_id))
            .and_then(Value::as_u64).and_then(|n|usize::try_from(n).ok()).unwrap_or(0);
    }
    list(state,"orders").iter().filter(|order|{
        !s(order,"confirmedAt").is_empty() && !truthy(order.get("deletedAt")) &&
        s(order,"clientId")==client_id &&
        ["KAKAO_REMEMBERED","KAKAO_TITLE"].contains(
            &s(order.get("clientMatch").unwrap_or(&Value::Null),"source")) &&
        key(s(order.get("clientMatch").unwrap_or(&Value::Null),"chatTitle"))==key(title)
    }).count()
}
fn reception_client(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("현재 거래처 데이터가 없습니다.")?;
    let windows=payload.get("windows").unwrap_or(&Value::Null);
    let chats=rows(windows).iter().filter(|w|s(w,"kind")=="CHAT"
        && !s(w,"title").trim().is_empty()).collect::<Vec<_>>();
    let active=s(payload,"activeChatHandle");
    // When several conversations exist, never select one without the active
    // chat HWND. With one window the name remains strong but still gets an
    // explicit first-stage verification highlight in the UI.
    let chat=if chats.len()==1{Some(chats[0])}else{
        chats.iter().copied().find(|w|!active.is_empty()&&s(w,"handle")==active)
    };
    // Keep the original labeled-sender rule. An "상호" label can belong to
    // the recipient, so only strongly labeled dealer names create a conflict
    // with a trusted Kakao title.
    let mut raw_hits=Vec::<(String,String,String,bool)>::new();
    for line in s(payload,"raw").lines(){
        let Some((label,value))=line.split_once(':').or_else(||line.split_once('：')) else{continue;};
        let label=key(label);
        if !["거래처","발주처","주문처","주문업체","발신업체",
            "발신자","대리점","업체명","상호"].contains(&label.as_str()){continue;}
        if let Some(id)=unique_client_id(state,value){
            let strong=["거래처","발주처","주문처","주문업체","발신업체"]
                .contains(&label.as_str());
            raw_hits.push((id,label,value.trim().to_owned(),strong));
        }
    }
    let explicit_ids=raw_hits.iter().filter(|hit|hit.3)
        .map(|hit|hit.0.as_str()).collect::<std::collections::HashSet<_>>();
    if explicit_ids.len()>1{
        return Ok(json!({"clientId":null,"source":"CONFLICT",
            "detail":"발주서에 서로 다른 발주처가 있습니다. 거래처를 확인하세요."}));
    }
    let mut chat_match:Option<(String,&str,String)>=None;
    if let Some(chat)=chat{
        let title=key(s(chat,"title"));
        let title_count=chats.iter().filter(|w|key(s(w,"title"))==title).count();
        if title_count==1{
            let stored=state.get("settings")
                .and_then(|v|v.get("kakaoTitleMappings"))
                .and_then(|v|v.get(&title));
            if let Some(entry)=stored{
                let id=s(entry,"clientId");
                if !truthy(entry.get("ambiguous")) && list(state,"clients").iter()
                    .any(|c|eligible_client(c,id,false)){
                    chat_match=Some((id.to_owned(),"KAKAO_REMEMBERED",
                        s(chat,"title").to_owned()));
                }
            }
            // A recorded ambiguous title is not permission to guess again
            // by substring. Explicit ordering-business evidence may still win.
            if chat_match.is_none() && !stored.is_some_and(|entry|truthy(entry.get("ambiguous"))){
                if let Some(id)=unique_title_client_id(state,s(chat,"title")){
                    chat_match=Some((id,"KAKAO_TITLE",s(chat,"title").to_owned()));
                }
            }
        }
    }
    if let Some((id,source,title))=chat_match{
        if let Some((other,_,_,_))=raw_hits.iter().find(|hit|
            hit.3 && hit.0.as_str()!=id.as_str()){
            let dealer_name=|id:&str|list(state,"clients").iter()
                .find(|c|s(c,"id")==id).map(|c|s(c,"name").to_owned())
                .unwrap_or_else(||id.to_owned());
            return Ok(json!({"clientId":null,"source":"CONFLICT",
                "detail":format!("카카오톡 거래처 ‘{}’와 발주서 발주처 ‘{}’가 다릅니다. 확인하세요.",
                    dealer_name(&id),dealer_name(other))}));
        }
        let count=confirmed_title_count(state,&title,&id);
        return Ok(json!({"clientId":id,"source":source,
            "chatTitle":title,"confirmedMatchCount":count,
            "detail":format!("카카오톡 창 제목 '{}'를 거래처로 인식했습니다.",title)}));
    }
    // The clipboard may contain only message 3/3: shipment details.
    // Without a verified Kakao conversation, match only an explicitly
    // labeled ORDERING business, never a consignee's "상호"/"업체명".
    if let Some((id,label,value,_))=raw_hits.iter().find(|hit|hit.3){
        return Ok(json!({"clientId":id,"source":"RAW_EXPLICIT",
            "detail":format!("발주서 '{}: {}'를 거래처로 인식했습니다.",label,value)}));
    }
    Ok(json!({"clientId":null,"source":"NONE",
        "detail":"거래처를 찾지 못했습니다. 직접 선택하세요."}))
}


/* Advisory candidate search. Only the active Kakao title and explicitly
 * labeled ordering-business lines qualify. Consignee text is excluded.
 * Selecting a candidate remains a separate explicit Rust order edit. */
fn reception_candidates(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("거래처 데이터를 확인할 수 없습니다.")?;
    let windows=payload.get("windows").unwrap_or(&Value::Null);
    let chats=rows(windows).iter()
        .filter(|w|s(w,"kind")=="CHAT" && !s(w,"title").trim().is_empty())
        .collect::<Vec<_>>();
    let active=s(payload,"activeChatHandle");
    let chat=if chats.len()==1{Some(chats[0])}else{
        chats.iter().copied().find(|w|!active.is_empty() && s(w,"handle")==active)
    };
    let title=chat.map(|w|key(s(w,"title")).replace(' ',""));
    let mut senders=Vec::new();
    for line in s(payload,"raw").lines(){
        let Some((label,value))=line.split_once(':').or_else(||line.split_once('：')) else {continue};
        if ["거래처","발주처","주문처","주문업체","발신업체",
            "발신자","대리점","업체명","상호"].contains(&key(label).as_str()){
            let value=key(value);
            if !value.is_empty(){senders.push(value);}
        }
    }
    let mut candidates=Vec::<Value>::new();
    let mut seen=std::collections::HashSet::<String>::new();
    for c in list(state,"clients"){
        let id=s(c,"id");
        if id.is_empty() || !eligible_client(c,id,false){continue;}
        let terms=client_terms(c);
        let title_hit=title.as_ref().is_some_and(|name|terms.iter().any(|term|{
            let term=term.replace(' ',"");
            term.chars().count()>=3 && name.contains(&term)
        }));
        let explicit_hit=senders.iter().any(|sender|
            terms.iter().any(|term|term==sender));
        if !(title_hit||explicit_hit) || !seen.insert(id.to_owned()){continue;}
        candidates.push(json!({
            "clientId":id,
            "name":s(c,"name")
        }));
        if candidates.len()>=8{break;}
    }
    Ok(json!({"candidates":candidates}))
}

/* The manual Kakao chooser is advisory; it must never silently select a
 * client. Keep its historic two-character nickname matching behavior, but
 * calculate every candidate and remembered mapping in Rust (not React). */
fn manual_title_matches(title:&str,clients:&[Value])->Vec<Value>{
    let needle=key(title).replace(' ',"");
    if needle.is_empty(){return Vec::new();}
    clients.iter().filter(|client|{
        let id=s(client,"id");
        eligible_client(client,id,false) &&
        client_terms(client).iter().any(|term|{
            let term=term.replace(' ',"");
            term.chars().count()>=2 && needle.contains(&term)
        })
    }).map(|client|json!({"clientId":s(client,"id")})).collect()
}
fn manual_kakao_candidates(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("거래처 데이터를 확인할 수 없습니다.")?;
    let windows=payload.get("windows").unwrap_or(&Value::Null);
    let clients=list(state,"clients");
    let chats=rows(windows).iter().filter(|window|
        s(window,"kind")=="CHAT" && !s(window,"title").trim().is_empty()
    ).collect::<Vec<_>>();
    let result=chats.iter().map(|window|{
        let normalized=key(s(window,"title"));
        let duplicated=chats.iter().filter(|other|
            key(s(other,"title"))==normalized).count()>1;
        let stored=state.get("settings")
            .and_then(|settings|settings.get("kakaoTitleMappings"))
            .and_then(|mappings|mappings.get(&normalized));
        let remembered=stored.filter(|mapping|!truthy(mapping.get("ambiguous")))
            .and_then(|mapping|{
                let id=s(mapping,"clientId");
                clients.iter().find(|client|eligible_client(client,id,false))
                    .map(|_|id.to_owned())
            });
        let mut matches=manual_title_matches(s(window,"title"),clients);
        if let Some(id)=remembered.as_ref(){
            matches.retain(|candidate|s(candidate,"clientId")!=id);
            matches.insert(0,json!({"clientId":id}));
        }
        json!({"handle":s(window,"handle"),"duplicate":duplicated,
            "rememberedClientId":remembered,"candidates":matches})
    }).collect::<Vec<_>>();
    Ok(json!({"chats":result,
        "search":manual_title_matches(s(payload,"search"),clients)}))
}
fn remember(payload:&Value)->Result<Value,String> {
    let state=payload.get("state").ok_or("현재 거래처 데이터가 없습니다.")?;
    let windows=payload.get("windows").ok_or("카카오톡 창 목록이 없습니다.")?;
    let handle=s(payload,"handle");
    let client_id=s(payload,"clientId");
    let chat=rows(windows).iter().find(|w|
        s(w,"kind")=="CHAT"&&s(w,"handle")==handle);
    if chat.is_none() ||
        !list(state,"clients").iter().any(|c|eligible_client(c,client_id,true)) {
        return Err("현재 대화창과 거래처를 선택하세요.".into());
    }
    let chat=chat.unwrap();
    let k=key(s(chat,"title"));
    let ambiguous=rows(windows).iter()
        .filter(|w|s(w,"kind")=="CHAT"&&key(s(w,"title"))==k).count()>1;
    let actor=payload.get("actor").and_then(Value::as_str).unwrap_or("");
    let now=payload.get("now").and_then(Value::as_str)
        .filter(|s|!s.is_empty()).ok_or("매핑 저장 시각이 없습니다.")?;
    let mut next=state.clone();
    let settings=next.get_mut("settings").and_then(Value::as_object_mut)
        .ok_or("거래처 설정이 없습니다.")?;
    let entries=settings.entry("kakaoTitleMappings").or_insert_with(||json!({}));
    if !entries.is_object(){*entries=json!({});}
    entries[k]=json!({"clientId":client_id,"updatedAt":now,
        "actor":actor,"ambiguous":ambiguous});
    Ok(json!({"state":next}))
}
pub(super) fn execute(operation:&str,payload:&Value)->Result<Value,String>{
    match operation {
        "remembered_title_client"=>{
            let state=payload.get("state").ok_or("현재 데이터가 없습니다.")?;
            let windows=payload.get("windows").unwrap_or(&Value::Null);
            Ok(json!({"clientId":remembered(state,windows)}))
        }
        "remember_title_client"=>remember(payload),
        "auto_match_reception_client"=>reception_client(payload),
        "reception_client_candidates"=>reception_candidates(payload),
        "kakao_manual_candidates"=>manual_kakao_candidates(payload),
        _=>Err(format!("지원하지 않는 카카오 거래처 연결 업무: {operation}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_kakao_candidates_use_native_name_matching_and_preserve_two_character_aliases(){
        let state=json!({"clients":[
            {"id":"a","name":"경기악기사","active":true,"shortNames":"경기|경기 악기"},
            {"id":"b","name":"전남악기사","active":true},
            {"id":"c","name":"경기지점","active":false}
        ],"settings":{"kakaoTitleMappings":{"경기 방문":{"clientId":"a","ambiguous":false}}}});
        let windows=json!([
            {"kind":"CHAT","handle":"one","title":"경기 방문"},
            {"kind":"CHAT","handle":"two","title":"전남악기사"},
            {"kind":"CHAT","handle":"three","title":"경기 방문"}
        ]);
        let got=manual_kakao_candidates(&json!({
            "state":state,"windows":windows,"search":"경기",
        })).unwrap();
        assert_eq!(got["chats"][0]["duplicate"],true);
        assert_eq!(got["chats"][0]["rememberedClientId"],"a");
        assert_eq!(got["chats"][0]["candidates"][0]["clientId"],"a");
        assert_eq!(got["chats"][1]["candidates"][0]["clientId"],"b");
        assert_eq!(got["search"][0]["clientId"],"a");
        assert_eq!(got["search"].as_array().unwrap().len(),1);
        let invalid=manual_kakao_candidates(&json!({
            "state":state,"windows":[],"search":"경기지점",
        })).unwrap();
        assert_eq!(invalid["search"].as_array().unwrap().len(),1); // substring on active client
    }
    #[test]
    fn reception_auto_match_prefers_current_kakao_chat_and_never_guesses_recipient_school(){
        let state=json!({"clients":[
            {"id":"dealer-1","name":"홍천악기","active":true},
            {"id":"dealer-2","name":"서울악기","active":true},
            {"id":"school","name":"팔렬고등학교","active":true}
        ],"settings":{}});
        let raw=r#"안녕하세요 ^^

** 10월 12일(월) 발송부탁드리며 **
** 연락처 2개 다 넣어주세요^^ **

강원특별자치도 홍천군 내촌면 동창로 207
팔렬고등학교
010-9149-2496
010-3343-4754

연주용보면대 708-1 / 30개
지휘자보면대 BD CONDUCTOR / 1개
보면대카트 Cart-10 / 2개"#;
        let windows=json!([
            {"kind":"CHAT","title":"홍천악기","handle":"A"},
            {"kind":"CHAT","title":"서울악기","handle":"B"}
        ]);
        let base=json!({"state":state,"windows":windows,"raw":raw});
        assert_eq!(reception_client(&base).unwrap()["clientId"],Value::Null);
        let selected=json!({"state":state,"windows":windows,"raw":raw,
            "activeChatHandle":"B"});
        assert_eq!(reception_client(&selected).unwrap()["clientId"],"dealer-2");
        let nicknamed=json!({"state":state,"windows":[
            {"kind":"CHAT","title":"홍천악기 사장님","handle":"ONE"}
        ],"raw":raw});
        assert_eq!(reception_client(&nicknamed).unwrap()["clientId"],"dealer-1");
        let none=json!({"state":state,"windows":[],"raw":raw});
        assert_eq!(reception_client(&none).unwrap()["clientId"],Value::Null);
        let explicit=json!({"state":state,"windows":[],"raw":r#"거래처: 홍천악기
팔렬고등학교"#});
        assert_eq!(reception_client(&explicit).unwrap()["clientId"],"dealer-1");
    }
    #[test]
    fn reception_auto_match_respects_remembered_mapping_and_ambiguous_titles(){
        let state=json!({"clients":[{"id":"d","name":"동진악기","active":true}],
            "settings":{"kakaoTitleMappings":{
                "동진 악기사장님":{"clientId":"d","ambiguous":false}}}});
        let selected=json!({"state":state,"windows":[
            {"kind":"CHAT","title":"동진 악기사장님","handle":"1"}]});
        assert_eq!(reception_client(&selected).unwrap()["clientId"],"d");
        let duplicated=json!({"state":state,"windows":[
            {"kind":"CHAT","title":"동진 악기사장님","handle":"1"},
            {"kind":"CHAT","title":"동진 악기사장님","handle":"2"}],
            "activeChatHandle":"2"});
        assert_eq!(reception_client(&duplicated).unwrap()["clientId"],Value::Null);
    }
    #[test]
    fn matching_guidance_conflict_and_confirmed_title_history(){
        let state=json!({"clients":[
            {"id":"a","name":"홍천악기","active":true},
            {"id":"b","name":"서울악기","active":true}
        ],"orders":[
            {"id":"old","clientId":"a","confirmedAt":"2026-11-14T09:00:00Z",
                "clientMatch":{"source":"KAKAO_REMEMBERED","chatTitle":"홍천악기"}},
            {"id":"pending","clientId":"a",
                "clientMatch":{"source":"KAKAO_REMEMBERED","chatTitle":"홍천악기"}}
        ],"settings":{}});
        let windows=json!([{"kind":"CHAT","title":"홍천악기","handle":"A"}]);
        let context=|raw:&str|json!({"state":state,"windows":windows,"raw":raw});
        let same=reception_client(&context("발주처: 홍천악기\n팔렬고등학교 708 2개")).unwrap();
        assert_eq!(same["clientId"],"a");
        assert_eq!(same["source"],"KAKAO_TITLE");
        assert_eq!(same["confirmedMatchCount"],1);
        assert_eq!(same["detail"],"카카오톡 창 제목 '홍천악기'를 거래처로 인식했습니다.");
        let conflicting=reception_client(&context("발주처: 서울악기\n708 2개")).unwrap();
        assert_eq!(conflicting["source"],"CONFLICT");
        assert_eq!(conflicting["clientId"],Value::Null);
        assert!(conflicting["detail"].as_str().unwrap().contains("서울악기"));
        let explicit=reception_client(&json!({"state":state,"windows":[],
            "raw":"발주처: 서울악기\n708 2개"})).unwrap();
        assert_eq!(explicit["source"],"RAW_EXPLICIT");
        assert_eq!(explicit["clientId"],"b");
        let receiver=reception_client(&context("팔렬고등학교\n서울악기 배송지\n708 2개")).unwrap();
        assert_eq!(receiver["clientId"],"a");
    }
    #[test]
    fn multi_message_order_uses_chat_title_even_when_only_shipment_was_pasted(){
        let state=json!({"clients":[
            {"id":"dealer","name":"서울악기","active":true},
            {"id":"school","name":"팔렬고등학교","active":true},
            {"id":"recipient-store","name":"가상악기사","active":true}
        ],"settings":{}});
        // The employee pasted message 3 only. Messages 1 (greeting and
        // dealer identification) and 2 were deliberately NOT copied.
        let shipment="받으실 분: 김가상\n상호: 가상악기사\n팔렬고등학교\n강원특별자치도 홍천군 내촌면 동창로 207\n010-9149-2496\n연주용보면대 708-1 30개";
        let windows=json!([{"kind":"CHAT","title":"서울악기","handle":"CHAT_A"}]);
        let matched=reception_client(&json!({"state":state,
            "windows":windows,"activeChatHandle":"CHAT_A","raw":shipment})).unwrap();
        assert_eq!(matched["clientId"],"dealer");
        assert_eq!(matched["source"],"KAKAO_TITLE");
        assert_eq!(matched["detail"],"카카오톡 창 제목 '서울악기'를 거래처로 인식했습니다.");
        // No chat window: the consignee's school/store must NEVER become
        // the ordering dealer merely because it appears in shipment data.
        let no_chat=reception_client(&json!({"state":state,
            "windows":[],"raw":shipment})).unwrap();
        assert_eq!(no_chat["clientId"],Value::Null);
        assert_eq!(no_chat["source"],"NONE");
        // Likewise, a pasted greeting alone must not be required:
        // the same verified chat works with only product/shipping lines.
        let product_only=reception_client(&json!({"state":state,
            "windows":windows,"raw":"708-1 30개\nBD CONDUCTOR 1개"})).unwrap();
        assert_eq!(product_only["clientId"],"dealer");
        // Two simultaneous chats without a verified active HWND: no guessing.
        let multiple=json!([{"kind":"CHAT","title":"서울악기","handle":"A"},
            {"kind":"CHAT","title":"가상악기사","handle":"B"}]);
        let uncertain=reception_client(&json!({"state":state,
            "windows":multiple,"raw":shipment})).unwrap();
        assert_eq!(uncertain["clientId"],Value::Null);
        let active=reception_client(&json!({"state":state,
            "windows":multiple,"activeChatHandle":"A","raw":shipment})).unwrap();
        assert_eq!(active["clientId"],"dealer");
    }
    #[test]
    fn unique_active_client_only_and_duplicate_window_requires_employee() {
        let state=json!({"clients":[{"id":"c1","name":"악기사","active":true}],
            "settings":{"kakaoTitleMappings":{"악기사":{"clientId":"c1","ambiguous":false}}}});
        let single=json!([{"kind":"CHAT","title":"악기사","handle":"1"}]);
        assert_eq!(remembered(&state,&single),Some("c1".into()));
        let multiple=json!([{"kind":"CHAT","title":"악기사","handle":"1"},
            {"kind":"CHAT","title":"악기사","handle":"2"}]);
        assert_eq!(remembered(&state,&multiple),None);
        let result=remember(&json!({"state":state,"windows":multiple,
            "handle":"1","clientId":"c1","actor":"직원",
            "now":"2026-11-15T00:00:00Z"})).unwrap();
        assert_eq!(result["state"]["settings"]["kakaoTitleMappings"]["악기사"]["ambiguous"],true);
    }
}

#[cfg(test)]
mod title_ambiguity_regressions{
    use super::*;
    #[test]
    fn remembered_ambiguity_blocks_name_fallback_but_not_explicit_sender(){
        let state=json!({"clients":[{"id":"a","name":"가상악기"}],
            "settings":{"kakaoTitleMappings":{"가상악기":{"clientId":"a","ambiguous":true}}}});
        let mut input=json!({"state":state,"windows":[{"kind":"CHAT","title":"가상악기","handle":"1"}]});
        assert_eq!(reception_client(&input).unwrap()["clientId"],Value::Null);
        input["raw"]=json!("발주처: 가상악기");
        assert_eq!(reception_client(&input).unwrap()["source"],"RAW_EXPLICIT");
        input["state"]["settings"]["kakaoTitleMappings"]["가상악기"]["ambiguous"]=json!(false);
        input["state"]["confirmedTitleCounts"]=json!({"가상악기":{"a":3}});
        assert_eq!(reception_client(&input).unwrap()["confirmedMatchCount"],3);
    }
}
