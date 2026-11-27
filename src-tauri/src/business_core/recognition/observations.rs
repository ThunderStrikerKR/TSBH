//! Read-only first/final comparison. Exact original UTF-16 spans only.
//! Missing or competing evidence is never invented as an engine error.
//! This is the first P02 slice, not a statistics writer or a classifier.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

type Span=(usize,usize);
type Candidates=BTreeMap<(Span,&'static str),BTreeSet<String>>;

fn rows(v:&Value)->impl Iterator<Item=&Value>{
    v.as_array().into_iter().flatten()
}

fn span(units:&[u16],source:&Value)->Option<Span>{
    let start=usize::try_from(source["start"].as_u64()?).ok()?;
    let end=usize::try_from(source["end"].as_u64()?).ok()?;
    if start>=end || end>units.len(){return None;}
    let actual=String::from_utf16(&units[start..end]).ok()?;
    (source["text"].as_str()==Some(actual.as_str())).then_some((start,end))
}

fn add(out:&mut Candidates,position:Span,task:&'static str,value:&str){
    if !value.is_empty(){
        out.entry((position,task)).or_default().insert(value.to_owned());
    }
}

fn linked(units:&[u16],recipients:&Value,out:&mut Candidates){
    for row in rows(recipients){
        for source in rows(&row["sources"]){
            if let (Some(s),Some(field))=(span(units,source),source["field"].as_str()){
                add(out,s,"type",field);
            }
        }
        for item in rows(&row["items"]){
            for source in rows(&item["sources"]){
                let Some(s)=span(units,source) else{continue;};
                let Some(field)=source["field"].as_str() else{continue;};
                add(out,s,"type",field);
                if field=="product"{
                    if let Some(id)=item["productId"].as_str(){add(out,s,"product",id);}
                    if let Some(id)=item["variantId"].as_str(){add(out,s,"variant",id);}
                }
                if field=="quantity" && item["quantityConfirmed"]==true{
                    if let Some(q)=item["quantity"].as_i64().filter(|q|*q>0){
                        add(out,s,"quantity",&q.to_string());
                    }
                }
            }
        }
    }
}

pub(super) fn compare(payload:&Value)->Result<Value,String>{
    let snapshot=&payload["confirmationSnapshot"];
    let Some(raw)=snapshot["raw"].as_str().filter(|raw|!raw.trim().is_empty())
    else{return Ok(json!({"status":"unavailable","observations":[]}));};
    let baseline=&snapshot["recognitionBaseline"];
    let first=&baseline["initialAnalysis"];
    if baseline["schemaVersion"]!=1 || !first["recipients"].is_array()
        || !first["candidates"].is_array() || !snapshot["recipients"].is_array(){
        return Ok(json!({"status":"unavailable","observations":[]}));
    }
    let units=raw.encode_utf16().collect::<Vec<_>>();
    let mut before=Candidates::new();
    let mut after=Candidates::new();
    linked(&units,&first["recipients"],&mut before);
    // Candidate kinds are the actual first classification. Do not confuse
    // a second phone's effective destination field with the original kind.
    before.retain(|(_,task),_|*task!="type");
    for candidate in rows(&first["candidates"]){
        if let (Some(s),Some(kind))=(span(&units,candidate),candidate["kind"].as_str()){
            add(&mut before,s,"type",kind);
        }
    }
    linked(&units,&snapshot["recipients"],&mut after);
    let keys=before.keys().chain(after.keys()).copied().collect::<BTreeSet<_>>();
    let mut observations=Vec::new();
    for (position,task) in keys{
        let old=before.get(&(position,task));
        let new=after.get(&(position,task));
        let status=match (old,new){
            (Some(a),Some(b)) if a.len()==1 && b.len()==1 && a==b=>"unchanged_observed",
            (Some(a),Some(b)) if a.len()==1 && b.len()==1=>"changed_requires_review",
            _=>"unresolved_evidence",
        };
        observations.push(json!({
            "source":{"start":position.0,"end":position.1},
            "task":task,"initialCandidates":old,"finalCandidates":new,
            "outcome":status
        }));
    }
    Ok(json!({"status":"compared","observations":observations,
        "initialRecognition":first.get("recognitionContext"),
        "finalClientId":baseline.get("finalClientId"),
        "finalClientSelection":baseline.get("finalClientSelection")}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn unchanged_and_changed_are_distinct_without_inventing_error(){
        let source=json!({"start":0,"end":3,"text":"709","field":"product"});
        let payload=json!({"confirmationSnapshot":{
            "raw":"709",
            "recognitionBaseline":{"schemaVersion":1,"initialAnalysis":{
                "recipients":[{"items":[{"productId":"old","sources":[source.clone()]}]}],
                "candidates":[{"start":0,"end":3,"text":"709","kind":"product"}]
            }},
            "recipients":[{"items":[{"productId":"new","sources":[source]}]}]
        }});
        let result=compare(&payload).unwrap();
        let list=result["observations"].as_array().unwrap();
        assert_eq!(list.len(),2);
        assert!(list.iter().any(|v|v["task"]=="type"&&v["outcome"]=="unchanged_observed"));
        assert!(list.iter().any(|v|v["task"]=="product"&&v["outcome"]=="changed_requires_review"));
    }
    #[test]
    fn invalid_span_and_missing_baseline_do_not_create_success(){
        assert_eq!(compare(&json!({"confirmationSnapshot":{"raw":"709"}})).unwrap()["status"],"unavailable");
        let result=compare(&json!({"confirmationSnapshot":{
            "raw":"😀709","recognitionBaseline":{"schemaVersion":1,"initialAnalysis":{
                "recipients":[],"candidates":[{"start":1,"end":4,"text":"709","kind":"product"}]
            }},"recipients":[]
        }})).unwrap();
        assert_eq!(result["observations"],json!([]));
    }
    #[test]
    fn competing_final_sources_are_not_counted_as_success(){
        let s=json!({"start":0,"end":3,"text":"709","field":"product"});
        let result=compare(&json!({"confirmationSnapshot":{
            "raw":"709","recognitionBaseline":{"schemaVersion":1,"initialAnalysis":{
                "recipients":[{"items":[{"productId":"a","sources":[s.clone()]}]}],
                "candidates":[{"start":0,"end":3,"text":"709","kind":"product"}]
            }},"recipients":[{"items":[{"productId":"a","sources":[s.clone()]},
                {"productId":"b","sources":[s]}]}]
        }})).unwrap();
        assert!(result["observations"].as_array().unwrap().iter().any(|v|
            v["task"]=="product"&&v["outcome"]=="unresolved_evidence"));
    }
}
