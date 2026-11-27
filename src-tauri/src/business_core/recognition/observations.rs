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

// Ownership identity is a verified name span, never a generated row ID,
// row position or a name string shared by two destinations.
fn position(units:&[u16],v:&Value)->Option<Span>{
    let a=usize::try_from(v["start"].as_u64()?).ok()?;
    let b=usize::try_from(v["end"].as_u64()?).ok()?;
    if a>=b || b>units.len(){return None;}
    String::from_utf16(&units[a..b]).ok()?;
    Some((a,b))
}
fn span_json(s:Span)->Value{json!({"start":s.0,"end":s.1})}
fn contact(field:&str)->bool{matches!(field,"phone"|"mobile")}
fn delivery(field:&str)->bool{field=="address"||contact(field)}
fn same_delivery_value(kind:&str,field:&str,actual:&str,original:&str)->bool{
    if actual.trim().is_empty() || original.trim().is_empty(){return false;}
    if kind=="address" && field=="address"{
        // Combined address fragments are valid only at token boundaries.
        // Do not promote a stale link after replacing the building number.
        return actual.match_indices(original).any(|(start,text)|{
            let end=start+text.len();
            actual[..start].chars().next_back().is_none_or(char::is_whitespace)
                && actual[end..].chars().next().is_none_or(char::is_whitespace)
        });
    }
    if contact(kind)&&contact(field){
        let digits=|s:&str|s.chars().filter(char::is_ascii_digit).collect::<String>();
        // The alternate phone field can be a valid destination for a second
        // mobile number. Ignore punctuation, not different digits.
        let expected=digits(original);
        return !expected.is_empty() && digits(actual)==expected;
    }
    false
}

struct Link<'a>{row:usize,field:&'a str,value:&'a str}
struct OwnerIndex<'a>{
    links:BTreeMap<Span,Vec<Link<'a>>>,
    names:BTreeMap<Span,BTreeSet<usize>>,
    row_names:BTreeMap<usize,BTreeSet<Span>>,
}
impl<'a> OwnerIndex<'a>{
    fn new(units:&[u16],recipients:&'a Value)->Self{
        let mut out=Self{links:BTreeMap::new(),names:BTreeMap::new(),
            row_names:BTreeMap::new()};
        for (idx,row) in rows(recipients).enumerate(){
            for source in rows(&row["sources"]){
                let Some(s)=span(units,source) else{continue;};
                let field=source["field"].as_str().unwrap_or("");
                let value=row[field].as_str().unwrap_or("");
                out.links.entry(s).or_default().push(Link{row:idx,field,value});
                if field=="name"{
                    out.names.entry(s).or_default().insert(idx);
                    if !value.trim().is_empty() &&
                        source["text"].as_str().is_some_and(|v|v.trim()==value.trim()){
                        out.row_names.entry(idx).or_default().insert(s);
                    }
                }
            }
            // Conflicting item links to the same source are ambiguity, not
            // independent corroboration of a delivery owner.
            for item in rows(&row["items"]){
                for source in rows(&item["sources"]){
                    if let Some(s)=span(units,source){
                        out.links.entry(s).or_default().push(Link{row:idx,field:"item",value:""});
                    }
                }
            }
        }
        out
    }
    fn unique_anchor(&self,row:usize)->Option<Span>{
        let anchors=self.row_names.get(&row)?;
        if anchors.len()!=1{return None;}
        let s=*anchors.iter().next()?;
        (self.names.get(&s).is_some_and(|owners|owners.len()==1&&owners.contains(&row)))
            .then_some(s)
    }
    fn owner(&self,s:Span,kind:&str,original:&str)->Option<Span>{
        let links=self.links.get(&s)?;
        let first=links.first()?;
        if !links.iter().all(|l|l.row==first.row && l.field==first.field &&
            same_delivery_value(kind,l.field,l.value,original)){return None;}
        self.unique_anchor(first.row)
    }
}
fn owner_observations(units:&[u16],first:&Value,final_rows:&Value)->Vec<Value>{
    let trace=&first["recipientOwnerTrace"];
    if trace["schemaVersion"]!=1{return Vec::new();}
    let before=OwnerIndex::new(units,&first["recipients"]);
    let after=OwnerIndex::new(units,final_rows);
    let mut originals=BTreeMap::<Span,Vec<&Value>>::new();
    for candidate in rows(&first["candidates"]){
        if let Some(s)=span(units,candidate){
            originals.entry(s).or_default().push(candidate);
        }
    }
    let mut decisions=BTreeMap::<Span,Vec<&Value>>::new();
    for d in rows(&trace["decisions"]){
        if let Some(s)=position(units,&d["source"]){
            decisions.entry(s).or_default().push(d);
        }
    }
    let mut result=Vec::new();
    for (s,entries) in decisions{
        let d=entries[0];
        let kind=d["source"]["kind"].as_str().unwrap_or("");
        if !delivery(kind){continue;}
        let mut initial=None;
        let mut final_anchor=None;
        let mut initial_relation=None;
        let mut final_relation=None;
        let mut reason="ambiguous_or_stale_source";
        if entries.len()==1{
            let selected=position(units,&d["selectedAnchor"])
                .filter(|_|d["selectedAnchor"]["kind"]=="name");
            let candidates=rows(&d["candidates"]).filter_map(|c|{
                if c["anchor"]["kind"]!="name"{return None;}
                let relation=c["relation"].as_str()?;
                if !matches!(relation,"previous"|"next"){return None;}
                Some((position(units,&c["anchor"])?,relation))
            }).collect::<Vec<_>>();
            if let Some(original)=originals.get(&s).filter(|v|v.len()==1)
                .and_then(|v|v.first()).filter(|v|v["kind"].as_str()==Some(kind))
                .and_then(|v|v["value"].as_str()){
                initial=before.owner(s,kind,original);
                final_anchor=after.owner(s,kind,original);
                let relation=|anchor:Span|candidates.iter().find(|(p,_)|*p==anchor)
                    .map(|(_,r)|*r);
                if initial.is_some() && initial==selected && candidates.len()==2 &&
                    candidates[0].0!=candidates[1].0 && candidates[0].1!=candidates[1].1{
                    initial_relation=initial.and_then(relation);
                    final_relation=final_anchor.map(|p|relation(p).unwrap_or("outside_traced_candidates"));
                    reason=if initial_relation.is_none(){"initial_anchor_not_traced"}
                        else if final_anchor.is_none(){"final_owner_unverified"}
                        else if final_relation==Some("outside_traced_candidates"){"final_anchor_not_traced"}
                        else if initial==final_anchor{"same_verified_anchor"}
                        else{"different_verified_anchor"};
                }else{reason="initial_owner_unverified";}
            }
        }
        let outcome=match reason{
            "same_verified_anchor"=>"unchanged_observed",
            "different_verified_anchor"|"final_anchor_not_traced"=>"changed_requires_review",
            _=>"unresolved_evidence",
        };
        result.push(json!({
            "source":span_json(s),"task":"recipient_ownership","sourceKind":kind,
            "initialCandidates":initial_relation.map(|v|vec![v]),
            "finalCandidates":final_relation.map(|v|vec![v]),
            "initialAnchor":initial.map(span_json),"finalAnchor":final_anchor.map(span_json),
            "outcome":outcome,"reason":reason,
            // A comparison alone cannot prove deliberate correction.
            // Audit reconciliation remains a separate P02 requirement.
            "requiresReview":outcome!="unchanged_observed",
            "eligibleForCandidateLearning":false
        }));
    }
    result
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
    observations.extend(owner_observations(&units,first,&snapshot["recipients"]));
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
