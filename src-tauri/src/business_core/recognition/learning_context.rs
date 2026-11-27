//! Shared versioned owner context projections. Derive only from first-analysis
//! evidence and the existing adjacent-candidate trace; never final row labels.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};
use super::observations::{span,position,Span};

const FEATURE_VERSION:u32=1;
fn rows(v:&Value)->impl Iterator<Item=&Value>{v.as_array().into_iter().flatten()}
fn role(kind:&str)->Option<&'static str>{
    match kind {
        "name"=>Some("name"),"address"=>Some("address"),
        "phone"|"mobile"=>Some("contact"),"product"=>Some("product"),
        "quantity"=>Some("quantity"),"note"=>Some("note"),_=>None
    }
}
fn distance_bucket(n:u64)->u8{match n{0=>0,1=>1,2..=3=>2,4..=7=>3,_=>4}}
fn count_bucket(n:u64)->u8{match n{0=>0,1=>1,2=>2,_=>3}}

struct Event{position:Span,line:u64,paragraph:u64,role:&'static str}
fn event_index(units:&[u16],first:&Value)->Vec<Event>{
    let mut out=rows(&first["candidates"]).filter_map(|c|Some(Event{
        position:span(units,c)?,line:c["line"].as_u64()?,
        paragraph:c["paragraph"].as_u64()?,role:role(c["kind"].as_str()?)?,
    })).collect::<Vec<_>>();
    out.sort_by_key(|e|(e.position,e.role));
    out
}
fn document_lead(events:&[Event])->&'static str{
    let mut leads=BTreeMap::new();
    for e in events{
        let lead=match e.role{
            "name"=>"name","address"|"contact"=>"delivery","product"=>"product",
            _=>continue
        };
        leads.entry(e.paragraph).or_insert(lead);
    }
    let values=leads.values().copied().collect::<BTreeSet<_>>();
    if values.len()>1{"mixed"}else{values.into_iter().next().unwrap_or("unknown")}
}
fn candidate_features(c:&Value)->Option<Value>{
    let gap=&c["gap"];
    Some(json!([
        c["sameParagraph"].as_bool()?,
        distance_bucket(c["lineDistance"].as_u64()?),
        count_bucket(gap["blankLines"].as_u64()?),
        count_bucket(gap["contentLines"].as_u64()?),
        count_bucket(gap["maxConsecutiveBlankLines"].as_u64()?)
    ]))
}
fn key(level:&str,features:&Value,client:Option<&str>)->Value{
    // Array field order is the wire contract, independent of JSON map order.
    // Keys contain no name, address, telephone, raw substring or row ID.
    let encoded=json!([FEATURE_VERSION,"recipient_ownership",level,client,features]).to_string();
    json!({"featureVersion":FEATURE_VERSION,"level":level,"key":encoded})
}
pub(super) fn contexts(units:&[u16],first:&Value)->BTreeMap<Span,Vec<Value>>{
    let mut out=BTreeMap::new();
    let trace=&first["recipientOwnerTrace"];
    if trace["schemaVersion"]!=1{return out;}
    let events=event_index(units,first);
    let lead=document_lead(&events);
    let manual_client=first["recognitionContext"]["clientId"].as_str()
        .filter(|id|!id.trim().is_empty()&&first["recognitionContext"]["clientBasis"]=="MANUAL");
    let mut seen=BTreeSet::new();
    for d in rows(&trace["decisions"]){
        let Some(s)=position(units,&d["source"])else{continue};
        if !seen.insert(s){out.remove(&s);continue;}
        let Some(kind)=d["source"]["kind"].as_str().filter(|k|
            matches!(*k,"address"|"phone"|"mobile"))else{continue};
        let Some(i)=events.binary_search_by_key(&s,|e|e.position).ok()else{continue};
        if events[i].role!=role(kind).unwrap_or("") ||
            i>0&&events[i-1].position==s || i+1<events.len()&&events[i+1].position==s{
            continue;
        }
        let candidates=&d["candidates"];
        if candidates.as_array().is_none_or(|v|v.len()!=2){continue;}
        let Some(previous)=rows(candidates).find(|c|c["relation"]=="previous")else{continue};
        let Some(next)=rows(candidates).find(|c|c["relation"]=="next")else{continue};
        if previous["anchor"]["kind"]!="name"||next["anchor"]["kind"]!="name"{continue;}
        let (Some(a),Some(b))=(position(units,&previous["anchor"]),position(units,&next["anchor"]))else{continue};
        if !(a.1<=s.0&&s.1<=b.0){continue;}
        let (Some(p),Some(n))=(candidate_features(previous),candidate_features(next))else{continue};
        let (Some(delivery),Some(address))=(d["previousHasDelivery"].as_bool(),d["nearbyAddress"].as_bool())else{continue};
        let base=json!([kind,previous["sameParagraph"],next["sameParagraph"]]);
        let mut projections=vec![key("base",&base,None)];
        // A bounded local sequence and between-anchor sequence describe
        // the observed order of fields without guessing their final owners.
        let local=events[i.saturating_sub(2)..(i+3).min(events.len())].iter().map(|e|
            json!([e.role,e.position==s,distance_bucket(e.line.abs_diff(events[i].line)),
                e.paragraph==events[i].paragraph])
        ).collect::<Vec<_>>();
        let begin=events.partition_point(|e|e.position.0<a.0);
        let end=events.partition_point(|e|e.position.0<=b.0);
        let regional=if end-begin<=16 {
            Some(events[begin..end].iter().map(|e|json!([
                e.role,e.position==s,e.paragraph==events[i].paragraph
            ])).collect::<Vec<_>>())
        }else{None};
        let structural=json!([base,p,n,delivery,address,lead,local,regional]);
        projections.push(key("structure",&structural,None));
        if let Some(client)=manual_client{
            projections.push(key("client_structure",&structural,Some(client)));
        }
        out.insert(s,projections);
    }
    out
}

/// Explicit diagnostic/lookup projection, not a second parser. The same
/// context key function is used for first/final observations and new traces.
pub(super) fn lookup(payload:&Value)->Result<Value,String>{
    let raw=payload["raw"].as_str().ok_or("문맥 조회 원문이 없습니다.")?;
    let first=&payload["initialAnalysis"];
    let units=raw.encode_utf16().collect::<Vec<_>>();
    let entries=contexts(&units,first).into_iter().map(|(s,keys)|
        json!({"source":{"start":s.0,"end":s.1},"contexts":keys})
    ).collect::<Vec<_>>();
    Ok(json!({"featureVersion":FEATURE_VERSION,"entries":entries}))
}
