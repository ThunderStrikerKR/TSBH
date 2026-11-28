//! Pattern-isolated contribution projection. Reuses the previous
//! source-grounded audit comparison and per-order normalized counters.
//! All structure comes from the current common candidate feature module.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};
const FEATURE_VERSION:u32=2;
fn rows(v:&Value)->impl Iterator<Item=&Value>{v.as_array().into_iter().flatten()}
#[derive(Default)]
struct FieldCounts{
    observations:u64,
    orders:u64,
    weak_orders:u64,
    explicit_orders:u64,
    pending:u64,
    unresolved:u64,
    weak:BTreeMap<String,u64>,
    positive:BTreeMap<String,u64>,
    negative:BTreeMap<String,u64>,
    weak_candidate_orders:BTreeMap<String,u64>,
    positive_candidate_orders:BTreeMap<String,u64>,
    negative_candidate_orders:BTreeMap<String,u64>,
}
impl FieldCounts{
    fn add_observation(&mut self,o:&Value){
        self.observations+=1;
        let one=|key:&str|o[key].as_array().filter(|v|v.len()==1)
            .and_then(|v|v[0].as_str()).filter(|v|!v.trim().is_empty());
        match o["outcome"].as_str(){
            Some("unchanged_observed") if o["eligibleForCandidateLearning"]==true=>{
                if let Some(v)=one("finalCandidates"){*self.weak.entry(v.to_owned()).or_default()+=1;}
                else{self.unresolved+=1;}
            }
            Some("explicit_correction_observed") if o["eligibleForCandidateLearning"]==true=>{
                if let (Some(old),Some(new))=(one("negativeCandidates"),one("positiveCandidates")){
                    if old!=new{
                        *self.negative.entry(old.to_owned()).or_default()+=1;
                        *self.positive.entry(new.to_owned()).or_default()+=1;
                    }else{self.unresolved+=1;}
                }else{self.unresolved+=1;}
            }
            Some("changed_requires_review")=>self.pending+=1,
            _=>self.unresolved+=1,
        }
    }
    fn add_order(&mut self,other:Self){
        self.observations+=other.observations;
        self.orders+=1;
        self.weak_orders+=u64::from(!other.weak.is_empty());
        self.explicit_orders+=u64::from(!other.positive.is_empty());
        self.pending+=other.pending;
        self.unresolved+=other.unresolved;
        fn merge(total:&mut BTreeMap<String,u64>,orders:&mut BTreeMap<String,u64>,
            local:BTreeMap<String,u64>){
            for (candidate,n) in local{
                *orders.entry(candidate.clone()).or_default()+=1;
                *total.entry(candidate).or_default()+=n;
            }
        }
        merge(&mut self.weak,&mut self.weak_candidate_orders,other.weak);
        merge(&mut self.positive,&mut self.positive_candidate_orders,other.positive);
        merge(&mut self.negative,&mut self.negative_candidate_orders,other.negative);
    }
    fn wire(&self)->Value{
        json!({"observationCount":self.observations,"independentOrders":self.orders,
            "weakIndependentOrders":self.weak_orders,"explicitCorrectionOrders":self.explicit_orders,
            "weakUnchangedCount":self.weak.values().sum::<u64>(),
            "explicitCorrectionCount":self.positive.values().sum::<u64>(),
            "weakCandidates":self.weak,"explicitPositiveCandidates":self.positive,
            "explicitNegativeCandidates":self.negative,
            "weakCandidateOrders":self.weak_candidate_orders,
            "explicitPositiveCandidateOrders":self.positive_candidate_orders,
            "explicitNegativeCandidateOrders":self.negative_candidate_orders,
            "pendingCorrectionCount":self.pending,"unresolvedCount":self.unresolved,
            "eligibleForAutomaticDecision":false})
    }
}


fn one<'a>(o:&'a Value,key:&str)->Option<&'a str>{
    let v=o[key].as_array().filter(|v|v.len()==1)?;
    v[0].as_str().filter(|v|!v.is_empty())
}
fn eligible(o:&Value)->bool{
    o["eligibleForCandidateLearning"]==true&&
        matches!(o["outcome"].as_str(),Some("unchanged_observed"|"explicit_correction_observed"))
}
fn evidence(old:Value,new:Value,explicit:bool)->Option<Value>{
    let a=old.to_string();let b=new.to_string();
    if a==b{
        if explicit{return None;}
        return Some(json!({"outcome":"unchanged_observed","eligibleForCandidateLearning":true,
            "finalCandidates":[b]}));
    }
    explicit.then(||json!({"outcome":"explicit_correction_observed","eligibleForCandidateLearning":true,
        "negativeCandidates":[a],"positiveCandidates":[b]}))
}
fn client_context(order:&Value,compared:&Value)->Option<String>{
    let snapshot=&order["confirmationSnapshot"];
    let first=&compared["initialRecognition"];
    let id=first["clientId"].as_str().filter(|v|!v.is_empty())?;
    if first["clientBasis"]!="MANUAL"||compared["finalClientSelection"]!="MANUAL"
        ||compared["finalClientId"].as_str()!=Some(id)||snapshot["data"]["clientId"].as_str()!=Some(id){
        return None;
    }
    if !rows(&order["recognitionAuditEvents"]).filter(|e|e["sourceRaw"]==snapshot["raw"]&&
        e["initialAnalysis"]==snapshot["recognitionBaseline"]["initialAnalysis"])
        .all(|e|e["context"]["clientId"].as_str()==Some(id)&&e["context"]["clientSelection"]=="MANUAL"){return None;}
    Some(id.to_owned())
}
fn add(groups:&mut BTreeMap<String,FieldCounts>,pattern:&Value,task:&str,feature:&Value,
    o:&Value,client:Option<&str>){
    // No general-context fallback and no global task denominator. Every
    // stored fact, including client evidence, belongs to exactly one pattern.
    for client in std::iter::once(None).chain(client.into_iter().map(Some)){
        let key=json!({"pattern":pattern,"task":task,"feature":feature,"client":client}).to_string();
        groups.entry(key).or_default().add_observation(o);
    }
}
fn pattern(structure:&Value)->Option<&Value>{
    if structure["schemaVersion"]!=FEATURE_VERSION{return None;}
    let p=&structure["pattern"];
    let roles=p["roles"].as_array()?;
    (p["version"]==1&&!roles.is_empty()&&roles.len()<=4096
        &&roles.iter().all(|r|r.as_u64().is_some_and(|v|v>0&&v<=31))).then_some(p)
}
fn span_start(o:&Value)->Option<u64>{o["source"]["start"].as_u64()}
fn structural_groups(groups:&mut BTreeMap<String,FieldCounts>,p:&Value,structure:&Value,
    compared:&Value,client:Option<&str>){
    let mut slots=BTreeMap::new();
    for slot in rows(&structure["slots"]){
        if let (Some(start),Some(line),Some(scalar),Some(paragraph))=(slot["start"].as_u64(),
            slot["line"].as_i64(),slot["scalar"].as_i64(),slot["paragraph"].as_i64()){
            slots.insert(start,(line,scalar,paragraph));
        }
    }
    let mut types=rows(&compared["observations"]).filter(|o|o["task"]=="type")
        .collect::<Vec<_>>();
    types.sort_by_key(|o|span_start(o).unwrap_or(u64::MAX));
    let explicit=|o:&Value|o["outcome"]=="explicit_correction_observed";
    if !types.is_empty()&&types.iter().all(|o|eligible(o)&&one(o,"initialCandidates").is_some()
        &&one(o,"finalCandidates").is_some()&&span_start(o).is_some_and(|s|slots.contains_key(&s))){
        let initial=types.iter().filter_map(|o|one(o,"initialCandidates")).collect::<Vec<_>>();
        let final_order=types.iter().filter_map(|o|one(o,"finalCandidates")).collect::<Vec<_>>();
        if let Some(o)=evidence(json!(initial),json!(final_order),types.iter().any(|o|explicit(o))){
            add(groups,p,"whole_order",&Value::Null,&o,client);
        }
        // Repetition counts are observations, never a global confidence
        // or a reason to down-weight a rarer cohort.
        if types.iter().all(|o|!explicit(o)){
            for r in rows(&structure["repeats"]){
                if r["period"].as_u64().is_some_and(|v|v>0)&&r["cycles"].as_u64().is_some_and(|v|v>=2){
                    let v=json!([r["period"],r["cycles"],r["remainder"]]);
                    if let Some(o)=evidence(v.clone(),v,false){
                        add(groups,p,"repeat",&Value::Null,&o,client);
                    }
                }
            }
        }
    }
    // The most recent verified occurrence of each role is sufficient to
    // record partial order and exact signed distance distributions without
    // materializing all pairs. An unresolved same-role observation breaks
    // that role's chain rather than becoming a weak vote.
    let mut last=BTreeMap::<String,&Value>::new();
    for o in types{
        let Some(role)=one(o,"finalCandidates")else{last.clear();continue;};
        let Some(start)=span_start(o)else{last.clear();continue;};
        let Some(&target)=slots.get(&start)else{last.clear();continue;};
        if !eligible(o){last.remove(role);continue;}
        for prior in last.values(){
            let Some(&origin)=span_start(prior).and_then(|s|slots.get(&s))else{continue;};
            let old=json!([one(prior,"initialCandidates"),one(o,"initialCandidates")]);
            let new=json!([one(prior,"finalCandidates"),role]);
            let corrected=explicit(prior)||explicit(o);
            if let Some(pair)=evidence(old.clone(),new.clone(),corrected){
                add(groups,p,"partial_order",&Value::Null,&pair,client);
            }
            for (metric,distance) in [("line_distance",target.0-origin.0),
                ("character_distance",target.1-origin.1),("paragraph_distance",target.2-origin.2)]{
                let old_value=json!([old,distance]);
                let new_value=json!([new,distance]);
                if let Some(fact)=evidence(old_value,new_value,corrected){
                    add(groups,p,metric,&Value::Null,&fact,client);
                }
            }
        }
        last.insert(role.to_owned(),o);
    }
}
fn contribution(order:&Value)->Result<Option<(BTreeMap<String,FieldCounts>,String)>,String>{
    let confirmed=order["confirmedAt"].as_str().filter(|v|!v.trim().is_empty());
    let snapshot=&order["confirmationSnapshot"];
    if order["learningActive"]!=true||confirmed.is_none()||snapshot["at"].as_str()!=confirmed
        ||order["deletedAt"].as_str().is_some_and(|v|!v.trim().is_empty()){return Ok(None);}
    let structure=&snapshot["recognitionBaseline"]["initialAnalysis"]["documentStructure"];
    let Some(p)=pattern(structure)else{return Ok(None);};
    let compared=super::observations::compare(order)?;
    let coverage=compared["auditCoverage"].as_str().unwrap_or("unavailable").to_owned();
    let mut groups=BTreeMap::new();
    if compared["status"]!="compared"||coverage!="verified_manifest"{return Ok(Some((groups,coverage)));}
    let client=client_context(order,&compared);
    for o in rows(&compared["observations"]){
        let Some(task)=o["task"].as_str().filter(|t|matches!(*t,"type"|"product"|"variant"|"quantity"|"recipient_ownership"|"product_ownership"|"quantity_link"))else{continue;};
        let mut scoped=client.as_deref();
        if o["outcome"]=="explicit_correction_observed"&&
            (o["auditClientId"].as_str()!=scoped||o["auditClientSelection"]!="MANUAL"){scoped=None;}
        let feature=if task=="recipient_ownership"||task=="product_ownership"||task=="quantity_link"{
            json!({"sourceKind":o["sourceKind"]})
        }else{Value::Null};
        add(&mut groups,p,task,&feature,o,scoped);
    }
    // Client structure requires the same verified manual client for every
    // explicit correction included in the sequence, not just final selection.
    let structural_client=client.as_deref().filter(|id|rows(&compared["observations"])
        .filter(|o|eligible(o)&&o["outcome"]=="explicit_correction_observed")
        .all(|o|o["auditClientId"].as_str()==Some(*id)&&o["auditClientSelection"]=="MANUAL"));
    structural_groups(&mut groups,p,structure,&compared,structural_client);
    Ok(Some((groups,coverage)))
}
pub(super) fn statistics(payload:&Value)->Result<Value,String>{
    let orders=payload["orders"].as_array().ok_or("확정 주문 목록이 없습니다.")?;
    // This is a bounded explicit batch, not a latest-250 training cohort.
    if orders.len()>250{return Err("한 번에 최대 250개 주문을 집계합니다.".into());}
    let mut seen=BTreeMap::<&str,&Value>::new();
    let mut groups=BTreeMap::<String,FieldCounts>::new();
    let mut verified=0;let mut unavailable=0;let mut incomplete=0;
    let mut without_context=0;
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("확정 주문 ID 누락")?;
        if let Some(old)=seen.get(id){
            if *old!=order{return Err("동일 주문의 상충된 확정 자료입니다.".into());}
            continue;
        }
        seen.insert(id,order);
        match contribution(order)?{
            Some((local,coverage)) if coverage=="verified_manifest"=>{
                verified+=1;
                if local.is_empty(){without_context+=1;}
                for (key,count) in local{groups.entry(key).or_default().add_order(count);}
            }
            Some(_)=>incomplete+=1,
            None=>unavailable+=1,
        }
    }
    Ok(json!({"featureVersion":FEATURE_VERSION,"status":"diagnostic_only","eligibleForDurableLearning":false,
        "uniqueOrders":seen.len(),"unavailableOrders":unavailable,"ordersWithoutMatchingContext":without_context,
        "auditCoverage":{"verifiedManifestOrders":verified,"legacyUnverifiedOrders":0,"incompleteHistoryOrders":incomplete},
        "statistics":[],"fieldStatistics":[],
        "fieldContextStatistics":groups.into_iter().map(|(key,count)|json!({"key":key,"counts":count.wire()})).collect::<Vec<_>>()
    }))
}
pub(super) fn summarize_confirmation(order:&Value)->Result<Value,String>{
    statistics(&json!({"orders":[order]}))
}
#[cfg(test)]
mod tests{
    use super::*;
    fn fixture(id:&str,roles:&[u16])->Value{
        let source=json!({"start":0,"end":1,"text":"가","field":"name","targetId":"r","recipientId":"r"});
        let row=json!({"id":"r","name":"가","sources":[source],"items":[]});
        let first=json!({"recipients":[row],"candidates":[{"start":0,"end":1,"text":"가","kind":"name","value":"가"}],
            "documentStructure":{"schemaVersion":2,"pattern":{"version":1,"roles":roles},
                "slots":[{"start":0,"line":0,"paragraph":0,"scalar":0}],"repeats":[]}});
        json!({"id":id,"learningActive":true,"confirmedAt":"2026-01-01T00:00:00Z","recognitionAuditEvents":[],
            "confirmationSnapshot":{"at":"2026-01-01T00:00:00Z","raw":"가","recipients":[row],
                "recognitionBaseline":{"schemaVersion":1,"auditEventIds":[],"initialAnalysis":first}}})
    }
    #[test]
    fn pattern_b_is_invariant_under_pattern_a_growth(){
        let b=fixture("b",&[1,2,4]);let a=fixture("a",&[1,4,2]);
        let base=statistics(&json!({"orders":[b]})).unwrap();
        let all=statistics(&json!({"orders":[b,a,fixture("a2",&[1,4,2]),a]})).unwrap();
        let select=|report:&Value|rows(&report["fieldContextStatistics"]).filter(|r|
            serde_json::from_str::<Value>(r["key"].as_str().unwrap()).unwrap()["pattern"]["roles"]==json!([1,2,4]))
            .cloned().collect::<Vec<_>>();
        assert!(!select(&base).is_empty());assert_eq!(select(&base),select(&all));
        assert_eq!(all["uniqueOrders"],3);
    }
    #[test]
    fn confirmed_and_corrected_learning_contributions_are_retractable(){
        let mut order=fixture("a",&[1]);
        let report=summarize_confirmation(&order).unwrap();
        assert!(rows(&report["fieldContextStatistics"]).any(|r|r["counts"]["weakUnchangedCount"]==1));
        order["learningActive"]=json!(false);
        assert!(rows(&summarize_confirmation(&order).unwrap()["fieldContextStatistics"]).next().is_none());
        order["learningActive"]=json!(true);
        order["confirmationSnapshot"]["recognitionBaseline"]["auditEventIds"]=json!(["missing"]);
        assert!(rows(&summarize_confirmation(&order).unwrap()["fieldContextStatistics"]).next().is_none());
        let mut c=FieldCounts::default();
        c.add_observation(&json!({"outcome":"explicit_correction_observed","eligibleForCandidateLearning":true,
            "negativeCandidates":["address"],"positiveCandidates":["name"]}));
        assert_eq!(c.wire()["explicitNegativeCandidates"]["address"],1);
        assert_eq!(c.wire()["explicitPositiveCandidates"]["name"],1);
        assert_eq!(c.wire()["weakUnchangedCount"],0);
    }
}
