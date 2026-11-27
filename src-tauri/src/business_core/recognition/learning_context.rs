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
    let mut result=json!({"featureVersion":FEATURE_VERSION,"entries":entries});
    // Optional offline comparison. Ordinary context projection remains unchanged.
    if let Some(history)=payload.get("historicalOrders"){
        let orders=history.as_array().ok_or("비교 주문 목록 형식 오류")?;
        if orders.len()>250{return Err("비교 주문은 최대 250개만 허용합니다.".into());}
        let target=payload["targetOrderId"].as_str().filter(|id|!id.trim().is_empty())
            .ok_or("자기 학습 제외를 위한 비교 대상 주문 ID가 필요합니다.")?;
        let historical=orders.iter().filter(|order|order["id"].as_str()!=Some(target))
            .cloned().collect::<Vec<_>>();
        let summary=statistics(&json!({"orders":historical}))?;
        let counts=rows(&summary["statistics"]).filter_map(|record|
            Some((record["key"].as_str()?, &record["counts"])))
            .collect::<BTreeMap<_,_>>();
        let selected=rows(&first["recipientOwnerTrace"]["decisions"]).filter_map(|d|{
            let s=position(&units,&d["source"])?;
            if d["selectedAnchor"]["kind"]!="name"{return None;}
            let anchor=position(&units,&d["selectedAnchor"])?;
            let mut matching=rows(&d["candidates"]).filter(|c|
                c["anchor"]["kind"]=="name"&&position(&units,&c["anchor"])==Some(anchor));
            let relation=matching.next()?["relation"].as_str()?;
            if matching.next().is_some()||!matches!(relation,"previous"|"next"){return None;}
            Some((s,(relation,d["legacyFirstBlockingReason"].clone())))
        }).collect::<BTreeMap<_,_>>();
        let assessments=rows(&result["entries"]).filter_map(|entry|{
            let s=position(&units,&entry["source"])?;
            let (baseline,gate)=selected.get(&s)?;
            let levels=rows(&entry["contexts"]).map(|context|{
                let count=context["key"].as_str().and_then(|k|counts.get(k).copied());
                level_diagnostic(context,count)
            }).collect::<Vec<_>>();
            Some(conflict_diagnostic(s,baseline,gate,&levels))
        }).collect::<Vec<_>>();
        result["comparison"]=json!({
            "status":"diagnostic_only","excludedTargetOrder":true,
            "historicalUniqueOrders":summary["uniqueOrders"],
            "assessments":assessments
        });
    }
    Ok(result)
}

// Each context is an overlapping projection, never an independent vote.
// Distinguish mixed observations instead of fabricating a calibrated probability.
fn level_diagnostic(context:&Value,counts:Option<&Value>)->Value{
    let count=|name:&str|counts.and_then(|c|c["weakCandidateOrders"][name].as_u64())
        .unwrap_or(0);
    let previous=count("previous");
    let next=count("next");
    let direction=match (previous>0,next>0){
        (true,false)=>"previous",
        (false,true)=>"next",
        (true,true)=>"mixed",
        _=>"unavailable",
    };
    json!({
        "level":context["level"],"direction":direction,
        "previousOrders":previous,"nextOrders":next,
        "independentOrders":counts.and_then(|c|c["weakIndependentOrders"].as_u64()).unwrap_or(0),
        "pendingCorrections":counts.and_then(|c|c["pendingCorrectionCount"].as_u64()).unwrap_or(0)
    })
}

fn conflict_diagnostic(s:Span,baseline:&str,gate:&Value,levels:&[Value])->Value{
    let previous=levels.iter().any(|v|v["direction"]=="previous");
    let next=levels.iter().any(|v|v["direction"]=="next");
    let mixed=levels.iter().any(|v|v["direction"]=="mixed");
    let opposing=levels.iter().any(|v|
        matches!(v["direction"].as_str(),Some("previous"|"next"))&&v["direction"]!=baseline);
    let agreement=levels.iter().any(|v|v["direction"]==baseline);
    json!({
        "source":{"start":s.0,"end":s.1},
        "legacyCandidate":baseline,"legacyFirstBlockingReason":gate,
        "levels":levels,"contextConflict":previous&&next,
        "mixedContextEvidence":mixed,"baselineOpposition":opposing,
        "baselineAgreement":agreement,
        "eligibleForAutomaticDecision":false,
        "decisionChanged":false
    })
}

#[derive(Default)]
struct Counts{
    observations:u64,
    previous:u64,
    next:u64,
    pending:u64,
    unresolved:u64,
    orders:u64,
    weak_orders:u64,
    previous_orders:u64,
    next_orders:u64,
}
impl Counts{
    fn add_observation(&mut self,o:&Value){
        self.observations+=1;
        if o["outcome"]=="unchanged_observed" && o["eligibleForCandidateLearning"]==true{
            match o["finalCandidates"].as_array().filter(|v|v.len()==1)
                .and_then(|v|v[0].as_str()){
                Some("previous")=>self.previous+=1,
                Some("next")=>self.next+=1,
                _=>self.unresolved+=1,
            }
        }else if o["outcome"]=="changed_requires_review"{self.pending+=1;}
        else{self.unresolved+=1;}
    }
    fn add_order(&mut self,other:Self){
        self.observations+=other.observations;
        self.previous+=other.previous;
        self.next+=other.next;
        self.pending+=other.pending;
        self.unresolved+=other.unresolved;
        self.orders+=1;
        self.weak_orders+=u64::from(other.previous+other.next>0);
        self.previous_orders+=u64::from(other.previous>0);
        self.next_orders+=u64::from(other.next>0);
    }
    fn wire(&self)->Value{
        json!({
            "observationCount":self.observations,"independentOrders":self.orders,
            "weakUnchangedCount":self.previous+self.next,
            "weakIndependentOrders":self.weak_orders,
            "weakCandidates":{"previous":self.previous,"next":self.next},
            "weakCandidateOrders":{"previous":self.previous_orders,"next":self.next_orders},
            "pendingCorrectionCount":self.pending,"unresolvedCount":self.unresolved,
            "eligibleForAutomaticDecision":false
        })
    }
}

/// Per-order contributions, also usable by the future durable delta writer.
/// Group before counting independent orders; no sum of per-candidate order
/// counts can substitute for the distinct context denominator.
fn contributions(order:&Value,filter:&BTreeSet<&str>)->Result<Option<BTreeMap<String,Counts>>,String>{
    let confirmed=order["confirmedAt"].as_str().filter(|v|!v.trim().is_empty());
    let snapshot=&order["confirmationSnapshot"];
    if confirmed.is_none() || snapshot["at"].as_str()!=confirmed ||
        order["deletedAt"].as_str().is_some_and(|v|!v.trim().is_empty()){
        return Ok(None);
    }
    let compared=super::observations::compare(order)?;
    if compared["status"]!="compared"{return Ok(None);}
    let first_client=compared["initialRecognition"]["clientId"].as_str();
    let final_client=snapshot["data"]["clientId"].as_str();
    // A confirmed client context is available only if the exact manually
    // selected initial client survived confirmation. Do not substitute a
    // later client into the historical features.
    let client_valid=first_client.is_some() && first_client==final_client
        && compared["finalClientId"].as_str()==final_client
        && compared["finalClientSelection"]=="MANUAL"
        && compared["initialRecognition"]["clientBasis"]=="MANUAL";
    let mut groups=BTreeMap::<String,Counts>::new();
    for o in rows(&compared["observations"]).filter(|o|o["task"]=="recipient_ownership"){
        for context in rows(&o["contexts"]){
            if context["level"]=="client_structure"&&!client_valid{continue;}
            let Some(key)=context["key"].as_str()else{continue;};
            if !filter.is_empty()&&!filter.contains(key){continue;}
            groups.entry(key.to_owned()).or_default().add_observation(o);
        }
    }
    Ok(Some(groups))
}

/// Explicit bounded batch, not an operational history scan. Caller supplies
/// immutable confirmed order records. Duplicate snapshots count once; a
/// conflicting duplicate fails instead of silently choosing a revision.
/// These are descriptive weak observations, not calibrated probabilities.
pub(super) fn statistics(payload:&Value)->Result<Value,String>{
    let orders=payload["orders"].as_array().ok_or("확정 주문 목록이 없습니다.")?;
    if orders.len()>250{return Err("귀속 통계 검증은 한 번에 최대 250개 주문만 허용합니다.".into());}
    let keys=payload.get("contextKeys");
    let mut filter=BTreeSet::new();
    if let Some(keys)=keys{
        let list=keys.as_array().ok_or("문맥 키 목록 형식 오류")?;
        if list.is_empty()||list.len()>750{return Err("문맥 키 조회는 1~750개만 허용합니다.".into());}
        for k in list{
            filter.insert(k.as_str().filter(|s|!s.is_empty()).ok_or("문맥 키 형식 오류")?);
        }
    }
    let mut seen=BTreeMap::<&str,&Value>::new();
    let mut groups=BTreeMap::<String,Counts>::new();
    let mut unavailable=0u64;
    let mut without_context=0u64;
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.trim().is_empty()).ok_or("확정 주문 ID 누락")?;
        if let Some(old)=seen.get(id){
            if *old!=order{return Err("동일 주문의 상충된 확정 자료입니다.".into());}
            continue;
        }
        seen.insert(id,order);
        match contributions(order,&filter)?{
            Some(local)=>{
                if local.is_empty(){without_context+=1;}
                for (key,counts) in local{groups.entry(key).or_default().add_order(counts);}
            }
            None=>unavailable+=1,
        }
    }
    let records=groups.into_iter().map(|(key,counts)|json!({
        "key":key,"counts":counts.wire()
    })).collect::<Vec<_>>();
    Ok(json!({
        "featureVersion":FEATURE_VERSION,"status":"diagnostic_only",
        "uniqueOrders":seen.len(),"unavailableOrders":unavailable,
        "ordersWithoutMatchingContext":without_context,"statistics":records
    }))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn independent_context_orders_are_not_the_sum_of_candidate_counts(){
        let weak=|candidate:&str|json!({
            "outcome":"unchanged_observed","eligibleForCandidateLearning":true,
            "finalCandidates":[candidate]
        });
        let mut local=Counts::default();
        local.add_observation(&weak("previous"));
        local.add_observation(&weak("previous"));
        local.add_observation(&weak("next"));
        let mut total=Counts::default();
        total.add_order(local);
        let c=total.wire();
        assert_eq!(c["observationCount"],3);
        assert_eq!(c["weakUnchangedCount"],3);
        assert_eq!(c["independentOrders"],1);
        assert_eq!(c["weakIndependentOrders"],1);
        assert_eq!(c["weakCandidateOrders"]["previous"],1);
        assert_eq!(c["weakCandidateOrders"]["next"],1);
    }
    #[test]
    fn owner_conflicts_preserve_baseline_and_do_not_sum_context_votes(){
        let count=|p,n|json!({
            "weakCandidateOrders":{"previous":p,"next":n},
            "weakIndependentOrders":3,"pendingCorrectionCount":1
        });
        let p=count(3,0);
        let n=count(0,3);
        let mixed=count(2,2);
        let levels=vec![
            level_diagnostic(&json!({"level":"base"}),Some(&p)),
            level_diagnostic(&json!({"level":"structure"}),Some(&n)),
            level_diagnostic(&json!({"level":"client_structure"}),Some(&mixed))
        ];
        let result=conflict_diagnostic((1,2),"previous",&json!("paragraph_boundary"),&levels);
        assert_eq!(result["contextConflict"],true);
        assert_eq!(result["mixedContextEvidence"],true);
        assert_eq!(result["baselineOpposition"],true);
        assert_eq!(result["baselineAgreement"],true);
        assert_eq!(result["legacyCandidate"],"previous");
        assert_eq!(result["legacyFirstBlockingReason"],"paragraph_boundary");
        assert_eq!(result["eligibleForAutomaticDecision"],false);
        assert_eq!(result["decisionChanged"],false);
        assert_eq!(result["levels"][2]["independentOrders"],3);
        assert_eq!(result["levels"][2]["pendingCorrections"],1);
    }
    #[test]
    fn owner_conflict_lookup_requires_target_and_excludes_its_history(){
        assert!(lookup(&json!({"raw":"x","historicalOrders":[]})).is_err());
        let result=lookup(&json!({
            "raw":"x","targetOrderId":"same",
            "historicalOrders":[{"id":"same"}]
        })).unwrap();
        assert_eq!(result["comparison"]["historicalUniqueOrders"],0);
        assert!(lookup(&json!({"raw":"x"})).unwrap().get("comparison").is_none());
    }
    #[test]
    fn explicitly_empty_lookup_does_not_expand_to_all_contexts(){
        assert!(statistics(&json!({"orders":[],"contextKeys":[]})).is_err());
    }
}
