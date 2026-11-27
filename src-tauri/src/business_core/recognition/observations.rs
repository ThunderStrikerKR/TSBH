//! Read-only first/final comparison. Exact original UTF-16 spans only.
//! Missing or competing evidence is never invented as an engine error.
//! This is the first P02 slice, not a statistics writer or a classifier.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

pub(super) type Span=(usize,usize);
type Candidates=BTreeMap<(Span,&'static str),BTreeSet<String>>;

fn rows(v:&Value)->impl Iterator<Item=&Value>{
    v.as_array().into_iter().flatten()
}

pub(super) fn span(units:&[u16],source:&Value)->Option<Span>{
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
pub(super) fn position(units:&[u16],v:&Value)->Option<Span>{
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
    fn has_unique_anchor(&self,s:Span)->bool{
        self.names.get(&s).filter(|owners|owners.len()==1)
            .and_then(|owners|owners.iter().next())
            .is_some_and(|row|self.unique_anchor(*row)==Some(s))
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
    let before=OwnerIndex::new(units,&first["recipients"]);
    let after=OwnerIndex::new(units,final_rows);
    owner_observations_indexed(units,first,&before,&after)
}
fn owner_observations_indexed(units:&[u16],first:&Value,before:&OwnerIndex<'_>,after:&OwnerIndex<'_>)->Vec<Value>{
    let trace=&first["recipientOwnerTrace"];
    if trace["schemaVersion"]!=1{return Vec::new();}
    let contexts=super::learning_context::contexts(units,first);
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
                    candidates[0].0!=candidates[1].0 && candidates[0].1!=candidates[1].1 &&
                    candidates.iter().all(|(p,_)|before.has_unique_anchor(*p)){
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
            "eligibleForCandidateLearning":outcome=="unchanged_observed",
            "contexts":contexts.get(&s)
        }));
    }
    result
}

// A changed snapshot is not itself an error label. Require an explicit
// source binding audit, same frozen recognition, and independently verified
// old/new owner anchors. Only the final confirmed direction contributes.
// Verify the actual audited target, not a row number or a matching value
// elsewhere in the order. Competing links at the same source remain ambiguous.
fn audit_target<'a>(units:&[u16],rs:&'a Value,cell:&Value,s:Span)->Option<&'a Value>{
    let rid=cell["recipientId"].as_str().filter(|v|!v.is_empty())?;
    let mut matches=rows(rs).filter(|r|r["id"].as_str()==Some(rid));
    let row=matches.next()?;
    if matches.next().is_some(){return None;}
    let target=if let Some(iid)=cell["itemId"].as_str().filter(|v|!v.is_empty()){
        let mut items=rows(&row["items"]).filter(|i|i["id"].as_str()==Some(iid));
        let item=items.next()?;
        if items.next().is_some(){return None;}
        item
    }else{row};
    let field=cell["field"].as_str()?;
    let mut links=rows(rs).flat_map(|r|std::iter::once(r).chain(rows(&r["items"])))
        .flat_map(|t|rows(&t["sources"]).map(move |source|(t,source)))
        .filter(|(_,source)|span(units,source)==Some(s));
    let (owner,source)=links.next()?;
    if links.next().is_some()||!std::ptr::eq(owner,target)||source["field"].as_str()!=Some(field){return None;}
    Some(target)
}

fn singleton(v:&Value)->Option<&str>{
    let values=v.as_array().filter(|v|v.len()==1)?;
    values[0].as_str().filter(|s|!s.trim().is_empty())
}

// Option evidence is separate from operational recognition. Only explicit
// attributes from a single associated product line may support learning.
#[derive(Default,PartialEq,Eq)]
struct OptionFeatures{
    color:Option<String>,
    configuration:BTreeMap<&'static str,bool>,
}
fn option_features(text:&str)->Option<OptionFeatures>{
    use std::sync::OnceLock;
    static COLOR:OnceLock<regex::Regex>=OnceLock::new();
    static CONFIG:OnceLock<regex::Regex>=OnceLock::new();
    let colors=COLOR.get_or_init(||regex::Regex::new(concat!(
        r"(?i)rose\s*gold|로즈골드|cosmo\s*black|코스모블랙|검정색|",
        r"black|블랙|검정|white|화이트|흰색|brown|브라운|gray|grey|그레이|회색|",
        r"chrome|크롬|gold|골드|silver|실버|red|레드|blue|블루|clear|클리어|frost|프로스트"
    )).expect("option color proof"));
    let boundary=|start:usize,end:usize|{
        !text[..start].chars().next_back().is_some_and(char::is_alphabetic)
            &&!text[end..].chars().next().is_some_and(char::is_alphabetic)
    };
    let mut found=BTreeSet::new();
    for m in colors.find_iter(text){
        if !boundary(m.start(),m.end()){continue;}
        let compact=m.as_str().split_whitespace().collect::<Vec<_>>().join(" ");
        let lower=compact.to_lowercase();
        let color=match lower.as_str(){
            "rosegold"=>"Rose Gold".to_owned(),
            _=>super::model::norm_color(&compact),
        };
        found.insert(color);
    }
    if found.len()>1{return None;}
    let mut features=OptionFeatures{color:found.into_iter().next(),configuration:BTreeMap::new()};
    let configs=CONFIG.get_or_init(||regex::Regex::new(concat!(
        r"(가방|바퀴)\s*(미포함|포함|미장착|장착|없음|있음)|",
        r"(무\s*타공|비\s*타공|민\s*판|타\s*공)"
    )).expect("configuration proof"));
    for c in configs.captures_iter(text){
        let m=c.get(0)?;
        if !boundary(m.start(),m.end()){continue;}
        let (key,value)=if let (Some(k),Some(v))=(c.get(1),c.get(2)){
            (if k.as_str()=="가방"{"bag"}else{"wheel"},
                matches!(v.as_str(),"포함"|"장착"|"있음"))
        }else{
            let v=c.get(3)?.as_str().split_whitespace().collect::<String>();
            ("holes",v=="타공")
        };
        if features.configuration.insert(key,value).is_some_and(|old|old!=value){return None;}
    }
    Some(features)
}
fn option_target(item:&Value)->Option<OptionFeatures>{
    let name=item["name"].as_str().unwrap_or("");
    let mut features=option_features(name)?;
    if let Some(color)=item["color"].as_str().filter(|c|!c.trim().is_empty()&&*c!="색상 미표기"){
        let color=super::model::norm_color(color);
        if features.color.as_ref().is_some_and(|c|c!=&color){return None;}
        features.color=Some(color);
    }
    Some(features)
}
// Unknown slash-delimited sale labels are not covered by a color proof.
// They must be separately written in the source, with full-token boundaries.
fn extra_option_labels(item:&Value)->Vec<String>{
    item["name"].as_str().unwrap_or("").split('/').skip(1).map(str::trim)
        .filter(|part|!part.is_empty()&&*part!="색상 미표기")
        .filter(|part|{
            let compact=part.split_whitespace().collect::<String>();
            let known_color=option_features(part).and_then(|v|v.color)
                .is_some_and(|c|super::model::norm_color(part)==c);
            let known_config=["가방포함","가방미포함","가방장착","가방미장착","가방있음","가방없음",
                "바퀴포함","바퀴미포함","바퀴장착","바퀴미장착","바퀴있음","바퀴없음",
                "타공","무타공","비타공","민판","타공상판","무타공상판","비타공상판","민판상판"]
                .contains(&compact.as_str());
            !known_color&&!known_config
        }).map(str::to_owned).collect()
}
fn exact_option_label(text:&str,label:&str)->bool{
    if label.chars().count()<2||label.len()>128{return false;}
    let Some((a,b))=super::model::find_term(text,label,false)else{return false;};
    !text[..a].chars().next_back().is_some_and(char::is_alphanumeric)
        &&!text[b..].chars().next().is_some_and(char::is_alphanumeric)
}

fn verified_variant(units:&[u16],s:Span,rs:&Value,target:&Value,prior:Option<&Value>)->bool{
    let start=units[..s.0].iter().rposition(|u|*u==10).map_or(0,|p|p+1);
    let end=s.1+units[s.1..].iter().position(|u|*u==10).unwrap_or(units.len()-s.1);
    let Ok(line)=String::from_utf16(&units[start..end])else{return false;};
    let products=rows(rs).flat_map(|r|rows(&r["items"])).flat_map(|i|rows(&i["sources"]))
        .filter(|source|source["field"]=="product"&&span(units,source)
            .is_some_and(|p|p.0>=start&&p.1<=end)).count();
    if products!=1{return false;}
    // A recipient/note on the same line may own the apparent option word.
    if rows(rs).flat_map(|r|rows(&r["sources"])).any(|source|
        span(units,source).is_some_and(|p|p.0<end&&p.1>start)){return false;}
    let Some(mut written)=option_features(&line)else{return false;};
    let (Ok(prefix),Ok(token))=(String::from_utf16(&units[start..s.0]),
        String::from_utf16(&units[s.0..s.1]))else{return false;};
    if let Some((color,(a,b)))=super::model::audit_attached_color(&line,target,(prefix.len(),prefix.len()+token.len())){
        if written.color.as_ref().is_some_and(|c|c!=&color){return false;}
        // A second abbreviation outside the proven model attachment is
        // ambiguous. Never use the first match alone to decide.
        let rest=format!("{} {}",&line[..a],&line[b..]);
        if rest.split(|c:char|!c.is_ascii_alphabetic()).any(|v|
            matches!(v.to_ascii_uppercase().as_str(),"B"|"BK"|"BR"|"W"|"G"|"C")){return false;}
        written.color=Some(color);
    }
    let Some(selected)=option_target(target)else{return false;};
    let labels=extra_option_labels(target);
    if written!=selected || labels.iter().any(|label|!exact_option_label(&line,label))
        || written.color.is_none()&&written.configuration.is_empty()&&labels.is_empty(){return false;}
    if let Some(prior)=prior{
        let Some(previous)=option_target(prior)else{return false;};
        let previous_labels=extra_option_labels(prior);
        if previous==selected&&previous_labels==labels{return false;}
        if super::model::model_names(prior)!=super::model::model_names(target){return false;}
        // Competing old and new explicit labels on the same line are
        // ambiguous, even when the final selected label itself is present.
        if previous_labels.iter().filter(|label|!labels.contains(label))
            .any(|label|exact_option_label(&line,label)){return false;}
    }
    true
}

// A source position, rather than generated row IDs, joins the audit chain.
// Reconstruct the observed value at every recorded transition. Equal-time
// competing transitions, gaps, and ambiguous links are never guessed.
fn unique_source_target<'a>(units:&[u16],rs:&'a Value,s:Span)->Option<(&'a Value,&'a str)>{
    let mut links=rows(rs).flat_map(|r|std::iter::once(r).chain(rows(&r["items"])))
        .flat_map(|t|rows(&t["sources"]).map(move |source|(t,source)))
        .filter(|(_,source)|span(units,source)==Some(s));
    let (target,source)=links.next()?;
    if links.next().is_some(){return None;}
    Some((target,source["field"].as_str()?))
}
fn field_state(units:&[u16],rs:&Value,s:Span,task:&str)->Option<String>{
    let (target,field)=unique_source_target(units,rs,s)?;
    match task{
        "type"=>{
            // A stale recipient source link does not prove that the final
            // value still corresponds to the classified original text.
            let original=String::from_utf16(&units[s.0..s.1]).ok()?;
            if delivery(field)&&!same_delivery_value(field,field,target[field].as_str()?,&original){return None;}
            if matches!(field,"name"|"note")&&target[field].as_str()?.trim()!=original.trim(){return None;}
            Some(field.to_owned())
        },
        "product" if field=="product"=>target["productId"].as_str().filter(|v|!v.is_empty()).map(str::to_owned),
        "variant" if field=="product"=>target["variantId"].as_str().filter(|v|!v.is_empty()).map(str::to_owned),
        "quantity" if field=="quantity"&&target["quantityConfirmed"]==true=>
            target["quantity"].as_i64().filter(|q|*q>0).map(|q|q.to_string()),
        _=>None,
    }
}

struct AuditChain<'a>{
    last_change:&'a Value,
    changes:usize,
    same_client:bool,
}
// Include all matching audit snapshots, even when an intermediate selection
// does not match the original text. Only the final transition needs a
// source-grounded correction candidate; intermediate choices get no votes.
fn verified_chain<'a,T:Eq>(events:&[&'a Value],initial:T,final_value:T,
    state:impl Fn(&Value)->Option<T>)->Option<AuditChain<'a>>{
    let mut current=initial;
    let mut last_change=None;
    let mut last_at=None;
    let mut changes=0usize;
    let mut same_client=true;
    let mut client:Option<(&Value,&Value)>=None;
    for &event in events{
        let (before,after)=(state(&event["beforeRows"]),state(&event["afterRows"]));
        // Fail closed when the target is absent or ambiguous in any event.
        let (Some(before),Some(after))=(before,after)else{return None;};
        if before!=current{return None;}
        if before==after{continue;}
        let at=chrono::DateTime::parse_from_rfc3339(event["at"].as_str()?).ok()?;
        if last_at.as_ref().is_some_and(|last|last>=&at){return None;}
        last_at=Some(at);
        let next_client=(&event["context"]["clientId"],&event["context"]["clientSelection"]);
        if let Some(old)=client{same_client &= old==next_client;}else{client=Some(next_client);}
        current=after;
        last_change=Some(event);
        changes+=1;
    }
    if current!=final_value{return None;}
    Some(AuditChain{last_change:last_change?,changes,same_client})
}

fn reconcile_field_audits(units:&[u16],initial_rows:&Value,final_rows:&Value,events:&[&Value],candidates:&BTreeMap<Span,Vec<(&Value,Value)>>,
    observations:&mut [Value]){
    for o in observations.iter_mut().filter(|v|v["task"]!="recipient_ownership"){
        let Some(s)=position(units,&o["source"])else{continue;};
        let task=o["task"].as_str().unwrap_or("");
        let relevant=candidates.get(&s).into_iter().flatten().filter(|(_,c)|{
            let field=c["target"]["field"].as_str().unwrap_or("");
            match task{
                "type"=>c["method"]=="explicit_binding"&&c["kind"]=="type_change",
                "product"|"variant"=>field=="product",
                "quantity"=>field=="quantity",
                _=>false,
            }
        }).collect::<Vec<_>>();
        let state=|rs:&Value|field_state(units,rs,s,task);
        if o["outcome"]=="unchanged_observed"{
            let initial=singleton(&o["initialCandidates"]);
            let changed=events.iter().any(|e|
                state(&e["beforeRows"]).as_deref()!=initial ||
                state(&e["afterRows"]).as_deref()!=initial);
            if !changed && (relevant.is_empty() || task=="product"&&relevant.iter().all(|(_,c)|
                c["beforeValue"]["productId"]==c["afterValue"]["productId"]&&
                c["method"]!="explicit_binding")){continue;}
            o["outcome"]=json!("changed_requires_review");
            o["reason"]=json!("intervention_without_final_change");
            o["eligibleForCandidateLearning"]=json!(false);
            o["requiresReview"]=json!(true);
            continue;
        }
        if o["outcome"]!="changed_requires_review"{continue;}
        let (Some(initial),Some(final_value))=(singleton(&o["initialCandidates"]),singleton(&o["finalCandidates"]))
            else{continue;};
        if initial==final_value{continue;}
        let Some(chain)=verified_chain(events,initial.to_owned(),final_value.to_owned(),state)else{continue;};
        let mut last=relevant.into_iter().filter(|(event,_)|std::ptr::eq(*event,chain.last_change));
        let Some((event,candidate))=last.next()else{continue;};
        if last.next().is_some(){continue;}
        let cell=&candidate["target"];
        let Some(target)=audit_target(units,&event["afterRows"],cell,s)else{continue;};
        let verified=match task{
            "type"=>candidate["method"]=="explicit_binding"&&candidate["kind"]=="type_change"
                &&cell["field"].as_str()==Some(final_value),
            "product"=>target["productId"].as_str()==Some(final_value)
                && candidate["afterValue"]["productId"].as_str()==Some(final_value)
                &&event["sourceRaw"].as_str().is_some_and(|raw|
                    super::corrections::product_source_matches(raw,target,s.0,s.1)),
            "variant"=>target["variantId"].as_str()==Some(final_value)
                &&candidate["afterValue"]["variantId"].as_str()==Some(final_value)
                &&target["productId"].as_str().is_some_and(|id|events.iter().all(|e|
                    field_state(units,&e["beforeRows"],s,"product").as_deref()==Some(id)
                    &&field_state(units,&e["afterRows"],s,"product").as_deref()==Some(id)))
                &&unique_source_target(units,initial_rows,s).is_some_and(|(prior,_)|
                    verified_variant(units,s,&event["afterRows"],target,Some(prior))&&
                    unique_source_target(units,final_rows,s).is_some_and(|(final_item,_)|
                        option_target(final_item)==option_target(target)&&
                        extra_option_labels(final_item)==extra_option_labels(target)&&
                        verified_variant(units,s,final_rows,final_item,Some(prior)))),
            "quantity"=>target["quantityConfirmed"]==true
                &&target["quantity"].as_i64().is_some_and(|q|q>0&&q.to_string()==final_value)
                &&{
                    let qs=super::quantity::extract(&String::from_utf16_lossy(&units[s.0..s.1]));
                    qs.len()==1&&qs[0].value.to_string()==final_value
                },
            _=>false,
        };
        if !verified{continue;}
        o["outcome"]=json!("explicit_correction_observed");
        o["reason"]=json!("verified_source_grounded_field_correction");
        o["eligibleForCandidateLearning"]=json!(true);
        o["requiresReview"]=json!(false);
        o["negativeCandidates"]=o["initialCandidates"].clone();
        o["positiveCandidates"]=o["finalCandidates"].clone();
        o["verifiedTransitionCount"]=json!(chain.changes);
        if chain.same_client{
            o["auditClientId"]=event["context"]["clientId"].clone();
            o["auditClientSelection"]=event["context"]["clientSelection"].clone();
        }
    }
}

fn reconcile_owner_audits(payload:&Value,units:&[u16],first:&Value,observations:&mut [Value])->Result<(),String>{
    let Some(events)=payload.get("recognitionAuditEvents")else{return Ok(());};
    let events=events.as_array().ok_or("정정 감사 목록 형식 오류")?;
    if events.len()>1000{return Err("주문별 정정 감사는 최대 1000건입니다.".into());}
    let snapshot=&payload["confirmationSnapshot"];
    let Some(id)=payload["id"].as_str().filter(|s|!s.is_empty())else{return Ok(());};
    let Some(confirmed)=payload["confirmedAt"].as_str()
        .filter(|at|snapshot["at"].as_str()==Some(*at))
        .and_then(|at|chrono::DateTime::parse_from_rfc3339(at).ok())else{return Ok(());};
    let raw=snapshot["raw"].as_str().unwrap_or("");
    let mut seen=BTreeMap::<&str,&Value>::new();
    let mut candidates=BTreeMap::<Span,Vec<(&Value,Value)>>::new();
    let mut ordered=Vec::new();
    for event in events{
        if event["orderId"].as_str()!=Some(id)||event["sourceRaw"].as_str()!=Some(raw)
            || &event["initialAnalysis"]!=first || event["context"]["schemaVersion"]!=1{continue;}
        let Some(at)=event["at"].as_str().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok())else{continue;};
        if at>confirmed{continue;}
        let Some(audit_id)=event["id"].as_str().filter(|s|!s.is_empty())else{continue;};
        if let Some(old)=seen.insert(audit_id,event){
            if old!=event{return Err("동일 정정 감사 ID의 내용이 상충합니다.".into());}
            continue;
        }
        ordered.push((at,event));
        let extracted=super::corrections::extract(event)?;
        for candidate in rows(&extracted["candidates"]){
            if let Some(s)=span(units,&candidate["source"]){
                candidates.entry(s).or_default().push((event,candidate.clone()));
            }
        }
    }
    ordered.sort_by(|a,b|a.0.cmp(&b.0));
    let ordered=ordered.into_iter().map(|(_,event)|event).collect::<Vec<_>>();
    reconcile_field_audits(units,&first["recipients"],&snapshot["recipients"],&ordered,&candidates,observations);
    for observation in observations.iter_mut().filter(|v|v["task"]=="recipient_ownership"){
        let Some(s)=position(units,&observation["source"])else{continue;};
        let entries=candidates.get(&s).into_iter().flatten()
            .filter(|(_,c)|c["method"]=="explicit_binding").collect::<Vec<_>>();
        let kind=observation["sourceKind"].as_str().unwrap_or("");
        let Some(original)=rows(&first["candidates"]).find(|c|span(units,c)==Some(s)
            && c["kind"].as_str()==Some(kind)).and_then(|c|c["value"].as_str())else{continue;};
        let state=|rs:&Value|OwnerIndex::new(units,rs).owner(s,kind,original);
        if observation["outcome"]=="unchanged_observed"{
            let initial=position(units,&observation["initialAnchor"]);
            if entries.is_empty()&&initial.is_some()&&!ordered.iter().any(|e|
                state(&e["beforeRows"])!=initial||state(&e["afterRows"])!=initial){continue;}
            // An explicit intervention (including a reverted edit) is not
            // an untouched observation. Do not count it as weak success.
            observation["outcome"]=json!("changed_requires_review");
            observation["eligibleForCandidateLearning"]=json!(false);
            observation["requiresReview"]=json!(true);
            observation["reason"]=json!("explicit_intervention_without_final_change");
            continue;
        }
        if observation["reason"]!="different_verified_anchor"{continue;}
        let (Some(initial),Some(final_anchor))=(position(units,&observation["initialAnchor"]),
            position(units,&observation["finalAnchor"]))else{continue;};
        let Some(chain)=verified_chain(&ordered,initial,final_anchor,state)else{continue;};
        // Every owner transfer must have an explicit source-binding audit.
        let mut valid=true;
        for &event in &ordered{
            if state(&event["beforeRows"])==state(&event["afterRows"]){continue;}
            let matching=entries.iter().filter(|(e,c)|
                std::ptr::eq(*e,event)&&audit_target(units,&event["afterRows"],&c["target"],s).is_some()
                && c["target"]["itemId"].is_null()
                && c["target"]["field"].as_str().is_some_and(|field|kind==field||contact(kind)&&contact(field)))
                .count();
            if matching!=1{valid=false;break;}
        }
        if !valid{continue;}
        let mut last=entries.into_iter().filter(|(event,_)|std::ptr::eq(*event,chain.last_change));
        let Some((event,candidate))=last.next()else{continue;};
        if last.next().is_some(){continue;}
        let cell=&candidate["target"];
        let field=cell["field"].as_str().unwrap_or("");
        if cell["itemId"].is_string() || !(kind==field||contact(kind)&&contact(field)){continue;}
        let new=OwnerIndex::new(units,&event["afterRows"]);
        let Some(target_id)=cell["recipientId"].as_str().filter(|s|!s.is_empty())else{continue;};
        let mut targets=rows(&event["afterRows"]).enumerate()
            .filter(|(_,r)|r["id"].as_str()==Some(target_id));
        let Some((target_index,_))=targets.next()else{continue;};
        if targets.next().is_some()||new.unique_anchor(target_index)!=Some(final_anchor){continue;}
        observation["outcome"]=json!("explicit_correction_observed");
        observation["reason"]=json!("verified_explicit_owner_reassignment");
        observation["eligibleForCandidateLearning"]=json!(true);
        observation["requiresReview"]=json!(false);
        observation["negativeCandidates"]=observation["initialCandidates"].clone();
        observation["positiveCandidates"]=observation["finalCandidates"].clone();
        // Client-specific evidence must additionally match the audited client.
        observation["verifiedTransitionCount"]=json!(chain.changes);
        if chain.same_client{
            observation["auditClientId"]=event["context"]["clientId"].clone();
            observation["auditClientSelection"]=event["context"]["clientSelection"].clone();
        }
    }
    Ok(())
}

// The frozen manifest is the authority for which audit rows belong to
// this confirmation. Old orders lack it; their comparisons remain diagnostic
// and must not be treated as verified durable training evidence.
fn audit_coverage(payload:&Value)->&'static str{
    let baseline=&payload["confirmationSnapshot"]["recognitionBaseline"];
    let Some(events)=payload["recognitionAuditEvents"].as_array()else{return "missing_history";};
    let Some(manifest)=baseline.get("auditEventIds")else{return "legacy_unverified";};
    let Some(ids)=manifest.as_array()else{return "invalid_manifest";};
    let expected=ids.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    if expected.len()!=ids.len()||expected.iter().any(|id|id.trim().is_empty()){
        return "invalid_manifest";
    }
    let Some(confirmed)=payload["confirmedAt"].as_str()
        .and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok())else{return "invalid_manifest";};
    let mut actual=BTreeSet::new();
    for event in events{
        if event["orderId"]!=payload["id"] ||
            event["sourceRaw"]!=payload["confirmationSnapshot"]["raw"]{continue;}
        let Some(at)=event["at"].as_str().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok())
            else{return "incomplete_history";};
        if at>confirmed{continue;}
        // A previous analysis of identical raw text cannot be silently
        // treated as an untouched observation.
        if event["initialAnalysis"]!=baseline["initialAnalysis"]||event["context"]["schemaVersion"]!=1{
            return "incomplete_history";
        }
        let Some(id)=event["id"].as_str()else{return "incomplete_history";};
        actual.insert(id);
    }
    if actual==expected{"verified_manifest"}else{"incomplete_history"}
}

// Diagnostic-only resource guard. Never truncate a source or an audit chain
// and then count it as a complete observation.
fn check_comparison_budget(payload:&Value,raw:&str,first:&Value,final_rows:&Value)->Result<(),String>{
    const LIMIT:&str="학습 진단 처리 한도 초과: 주문 또는 감사 범위를 분할하세요.";
    if raw.len()>128*1024{return Err(LIMIT.into());}
    let row_weight=|rs:&Value|->usize{
        rows(rs).map(|r|1usize.saturating_add(rows(&r["sources"]).count())
            .saturating_add(rows(&r["items"]).map(|i|
                1usize.saturating_add(rows(&i["sources"]).count())).fold(0usize,usize::saturating_add)))
            .fold(0usize,usize::saturating_add)
    };
    let events=rows(&payload["recognitionAuditEvents"]);
    let mut total=row_weight(&first["recipients"]).saturating_add(row_weight(final_rows));
    let candidates=rows(&first["candidates"]).count();
    let traces=rows(&first["recipientOwnerTrace"]["decisions"]).count();
    if candidates>4096||traces>4096||total>16384{return Err(LIMIT.into());}
    let mut count=0usize;
    for event in events{
        count+=1;
        total=total.saturating_add(row_weight(&event["beforeRows"])).saturating_add(row_weight(&event["afterRows"]));
        if count>1000||total>100_000{return Err(LIMIT.into());}
    }
    // Bound repeated source/row scans and document-local candidate searches.
    if total.saturating_mul(candidates.saturating_add(traces).max(1))>16_000_000
        || total.saturating_mul(raw.len())>64*1024*1024{return Err(LIMIT.into());}
    Ok(())
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
    check_comparison_budget(payload,raw,first,&snapshot["recipients"])?;
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
        let mut status=match (old,new){
            (Some(a),Some(b)) if a.len()==1 && b.len()==1 && a==b=>"unchanged_observed",
            (Some(a),Some(b)) if a.len()==1 && b.len()==1=>"changed_requires_review",
            _=>"unresolved_evidence",
        };
        // Sets collapse identical candidate IDs; they do not prove that the
        // source has exactly one owner. Require both snapshots to agree on
        // unique, valid source links even when duplicate labels are equal.
        let old_rows=&first["recipients"];
        let new_rows=&snapshot["recipients"];
        let old_state=field_state(&units,old_rows,position,task);
        let new_state=field_state(&units,new_rows,position,task);
        let matches=|values:Option<&BTreeSet<String>>,state:&Option<String>|
            values.is_some_and(|v|v.len()==1&&state.as_ref().is_some_and(|s|v.contains(s)));
        if !matches(old,&old_state)||!matches(new,&new_state){
            status="unresolved_evidence";
        }
        let original_count=rows(&first["candidates"]).filter(|c|span(&units,c)==Some(position)).count();
        if original_count>1 || task=="type"&&original_count!=1{status="unresolved_evidence";}
        if task=="type" && old_state.as_deref().is_some_and(contact)
            && new_state.as_deref().is_some_and(contact) && old_state!=new_state{
            // Phone/mobile slots can change without a classification error.
            status="unresolved_evidence";
        }
        if task=="quantity"&&status=="unchanged_observed"{
            let qs=super::quantity::extract(&String::from_utf16_lossy(&units[position.0..position.1]));
            if qs.len()!=1||new_state.as_deref()!=Some(qs[0].value.to_string().as_str()){
                status="unresolved_evidence";
            }
        }
        if task=="variant"&&field_state(&units,old_rows,position,"product")!=
            field_state(&units,new_rows,position,"product"){status="unresolved_evidence";}
        if task=="variant"&&status=="unchanged_observed"&&
            unique_source_target(&units,old_rows,position).zip(unique_source_target(&units,new_rows,position))
                .is_none_or(|((a,_),(b,_))|a["color"]!=b["color"]){
            status="unresolved_evidence";
        }

        // A default option is not a verified source observation. Require
        // the written color and a single associated product for weak credit.
        if task=="variant"&&status=="unchanged_observed"{
            let final_rows=&snapshot["recipients"];
            let mut items=rows(final_rows).flat_map(|r|rows(&r["items"]))
                .filter(|i|rows(&i["sources"]).any(|src|src["field"]=="product"&&span(&units,src)==Some(position)));
            if items.next().is_none_or(|item|!verified_variant(&units,position,final_rows,item,None))
                || items.next().is_some(){status="unresolved_evidence";}
        }
        observations.push(json!({
            "source":{"start":position.0,"end":position.1},
            "task":task,"initialCandidates":old,"finalCandidates":new,
            "outcome":status,"eligibleForCandidateLearning":status=="unchanged_observed",
            "requiresReview":status!="unchanged_observed"
        }));
    }
    observations.extend(owner_observations(&units,first,&snapshot["recipients"]));
    reconcile_owner_audits(payload,&units,first,&mut observations)?;
    let coverage=audit_coverage(payload);
    if matches!(coverage,"invalid_manifest"|"incomplete_history") ||
        baseline.get("auditEventIds").is_some()&&coverage!="verified_manifest"{
        for o in &mut observations{
            if o["eligibleForCandidateLearning"]==true{
                o["outcome"]=json!("changed_requires_review");
                o["eligibleForCandidateLearning"]=json!(false);
                o["requiresReview"]=json!(true);
                o["reason"]=json!("audit_history_incomplete");
                if let Some(object)=o.as_object_mut(){
                    object.remove("positiveCandidates");
                    object.remove("negativeCandidates");
                }
            }
        }
    }
    Ok(json!({"status":"compared","auditCoverage":coverage,"observations":observations,
        "initialRecognition":first.get("recognitionContext"),
        "finalClientId":baseline.get("finalClientId"),
        "finalClientSelection":baseline.get("finalClientSelection")}))
}

// Evaluate paired observations on each tier separately; do not pool tiers.
#[derive(Default)]
struct PairedEvaluation{
    eligible:u64,paired:u64,legacy_correct:u64,shadow_correct:u64,
    both_correct:u64,legacy_only:u64,shadow_only:u64,both_wrong:u64,
    abstained:u64,outside:u64,orders:u64,paired_orders:u64,
    order_legacy_sum:f64,order_shadow_sum:f64,
}
impl PairedEvaluation{
    fn observe(&mut self,legacy:&str,actual:&str,predicted:Option<&str>,outside:bool){
        self.eligible+=1;self.outside+=u64::from(outside);
        let Some(predicted)=predicted else{self.abstained+=1;return;};
        self.paired+=1;
        let old=legacy==actual;let new=predicted==actual;
        self.legacy_correct+=u64::from(old);self.shadow_correct+=u64::from(new);
        match (old,new){(true,true)=>self.both_correct+=1,(true,false)=>self.legacy_only+=1,
            (false,true)=>self.shadow_only+=1,(false,false)=>self.both_wrong+=1}
    }
    fn merge_order(&mut self,o:Self){
        if o.eligible==0{return;}
        self.orders+=1;self.eligible+=o.eligible;self.paired+=o.paired;
        self.legacy_correct+=o.legacy_correct;self.shadow_correct+=o.shadow_correct;
        self.both_correct+=o.both_correct;self.legacy_only+=o.legacy_only;
        self.shadow_only+=o.shadow_only;self.both_wrong+=o.both_wrong;
        self.abstained+=o.abstained;self.outside+=o.outside;
        if o.paired>0{
            self.paired_orders+=1;
            self.order_legacy_sum+=o.legacy_correct as f64/o.paired as f64;
            self.order_shadow_sum+=o.shadow_correct as f64/o.paired as f64;
        }
    }
    fn wire(&self)->Value{
        let fraction=|n:u64,d:u64|(d>0).then(||n as f64/d as f64);
        json!({"eligibleObservations":self.eligible,"pairedObservations":self.paired,
            "eligibleOrders":self.orders,"pairedOrders":self.paired_orders,
            "shadowAbstentions":self.abstained,"outsideCandidateObservations":self.outside,
            "coverage":fraction(self.paired,self.eligible),
            "legacyAgreement":fraction(self.legacy_correct,self.paired),
            "shadowAgreement":fraction(self.shadow_correct,self.paired),
            "bothAgree":self.both_correct,"legacyOnlyAgrees":self.legacy_only,
            "shadowOnlyAgrees":self.shadow_only,"bothDisagree":self.both_wrong,
            "orderMacroLegacyAgreement":(self.paired_orders>0).then(||self.order_legacy_sum/self.paired_orders as f64),
            "orderMacroShadowAgreement":(self.paired_orders>0).then(||self.order_shadow_sum/self.paired_orders as f64)})
    }
}
fn shadow_top<'a>(report:&'a Value,allowed:&BTreeSet<&str>)->Option<&'a str>{
    let mass=report["totalSupportMass"].as_f64().filter(|v|v.is_finite()&&*v>0.0)?;
    let outside=report["outsideCandidateSupportMass"].as_f64().filter(|v|v.is_finite()&&*v>=0.0)?;
    let mut scores=Vec::new();let mut seen=BTreeSet::new();
    for c in report["candidates"].as_array()?{
        let id=c["candidate"].as_str().filter(|id|allowed.contains(id))?;
        let score=c["supportMass"].as_f64().filter(|v|v.is_finite()&&*v>=0.0)?;
        if !seen.insert(id){return None;}
        scores.push((id,score));
    }
    if &seen!=allowed || (scores.iter().map(|x|x.1).sum::<f64>()+outside-mass).abs()>1e-7*mass.max(1.0){return None;}
    scores.sort_by(|a,b|b.1.total_cmp(&a.1));
    let (id,score)=*scores.first()?;
    (score>outside+1e-10&&scores.get(1).is_none_or(|x|score>x.1+1e-10)).then_some(id)
}
pub(super) fn evaluate_shadow(payload:&Value)->Result<Value,String>{
    let orders=payload["orders"].as_array().filter(|v|v.len()<=250).ok_or("평가 주문 한도 오류")?;
    let mut seen=BTreeSet::new();
    let mut totals=BTreeMap::<(String,String,String),PairedEvaluation>::new();
    let mut skipped=0u64;let mut labels_excluded=0u64;
    let mut missing_prediction=0u64;let mut invalid_join=0u64;
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("평가 주문 ID 오류")?;
        if !seen.insert(id){return Err("평가 주문 ID 중복".into());}
        let snap=&order["confirmationSnapshot"];
        let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let capture=&first["initialPredictionCapture"];
        let shadow=&capture["learningShadow"];
        let (Some(start),Some(end),Some(predicted),Some(confirmed))=(date(&capture["analysisStartedAt"]),
            date(&capture["analysisCompletedAt"]),date(&shadow["capturedAt"]),date(&order["confirmedAt"]))
            else{skipped+=1;continue;};
        if order["learningActive"]!=true || order["deletedAt"].as_str().is_some_and(|v|!v.is_empty()) ||
            capture["origin"]!="native_reception_worker" || capture["orderId"].as_str()!=Some(id) ||
            capture["schemaVersion"]!=1 || shadow["status"]!="captured_shadow_support" ||
            shadow["sourceDigestsMatched"]!=true || shadow["decisionChanged"]!=false ||
            shadow["modelId"].as_str().is_none_or(|s|!s.starts_with("shadow-")||s.len()!=71) ||
            snap["at"]!=order["confirmedAt"] || !(start<=end&&end<=predicted&&predicted<confirmed){
            skipped+=1;continue;
        }
        let Some(events)=order["recognitionAuditEvents"].as_array()else{skipped+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=predicted||t>confirmed)){
            skipped+=1;continue;
        }
        let compared=compare(order)?;
        if compared["auditCoverage"]!="verified_manifest"{skipped+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("평가 원문 누락")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let projection=super::learning_context::lookup(&json!({"raw":raw,"initialAnalysis":first}))?;
        let mut per_order=BTreeMap::<(String,String,String),PairedEvaluation>::new();
        let mut used=BTreeSet::new();
        for obs in rows(&compared["observations"]){
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",
                _=>{labels_excluded+=1;continue;}
            };
            let (Some(old),Some(actual),Some(s),Some(task))=(singleton(&obs["initialCandidates"]),
                singleton(&obs["finalCandidates"]),position(&units,&obs["source"]),obs["task"].as_str())
                else{labels_excluded+=1;continue;};
            if label=="explicit_correction" && (singleton(&obs["positiveCandidates"])!=Some(actual)||
                singleton(&obs["negativeCandidates"])!=Some(old)||old==actual){labels_excluded+=1;continue;}
            if !used.insert((s,task)){return Err("평가 관측 위치 중복".into());}
            let expected=if task=="recipient_ownership"{&projection["entries"]}else{&projection["fieldEntries"]};
            let mut matches=rows(expected).filter(|v|position(&units,&v["source"])==Some(s)&&
                (task=="recipient_ownership"||v["task"].as_str()==Some(task)));
            let Some(expected)=matches.next()else{invalid_join+=1;continue;};
            if matches.next().is_some(){invalid_join+=1;continue;}
            let mut loc=rows(&shadow["locations"]).filter(|v|v["task"].as_str()==Some(task)&&position(&units,v)==Some(s));
            let location=loc.next();
            if loc.next().is_some(){invalid_join+=1;continue;}
            let mut tiers=BTreeSet::new();
            for context in rows(&expected["contexts"]){
                let (Some(key),Some(level))=(context["key"].as_str(),context["level"].as_str())else{invalid_join+=1;continue;};
                if !tiers.insert(level){invalid_join+=1;continue;}
                let counter=per_order.entry((task.to_owned(),level.to_owned(),label.to_owned())).or_default();
                let Some(location)=location else{missing_prediction+=1;counter.observe(old,actual,None,false);continue;};
                let allowed=rows(&location["candidates"]).filter_map(Value::as_str).collect::<BTreeSet<_>>();
                if allowed.is_empty() || allowed.len()!=rows(&location["candidates"]).count()||!allowed.contains(old) ||
                    !rows(&location["contextKeys"]).any(|v|v.as_str()==Some(key)){
                    invalid_join+=1;counter.observe(old,actual,None,false);continue;
                }
                let mut reports=rows(&shadow["contexts"]).filter(|r|r["task"].as_str()==Some(task)&&
                    r["contextKey"].as_str()==Some(key)&&
                    rows(&r["candidateIds"]).filter_map(Value::as_str).collect::<BTreeSet<_>>()==allowed);
                let report=reports.next();
                let top=if reports.next().is_none(){report.and_then(|r|shadow_top(r,&allowed))}else{None};
                counter.observe(old,actual,top,!allowed.contains(actual));
            }
        }
        for (key,value) in per_order{totals.entry(key).or_default().merge_order(value);}
    }
    let groups=totals.into_iter().map(|((task,level,label),v)|json!({"task":task,"contextLevel":level,
        "labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>();
    Ok(json!({"status":"paired_shadow_diagnostic_only","uniqueOrders":seen.len(),
        "excludedOrders":skipped,"excludedUnverifiedLabels":labels_excluded,
        "missingPredictionTierObservations":missing_prediction,"invalidJoinObservations":invalid_join,
        "groups":groups,"representativeLabelsVerified":false,"historicalModelVerified":false,
        "sourceProvenanceVerified":false,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// P07-01: estimate next-anchor frequency conditional on a real score margin.
// Explicit corrections and unchanged observations are not independent truth.
const OWNER_SCORE_POLICY:&str="owner-margin-30-v1";
const MIN_CALIBRATION_ORDERS:usize=5;
fn owner_score_bin(delta:i64)->usize{
    match delta {i64::MIN..=-61=>0,-60..=-31=>1,-30..=-11=>2,-10..=-1=>3,
        0..=9=>4,10..=29=>5,30..=59=>6,_=>7}
}
fn owner_score_input(units:&[u16],trace:&Value,obs:&Value)->Option<(&'static str,usize)>{
    if !trace["legacyFirstBlockingReason"].is_null()||trace["legacyRequiredMargin"]!=30{return None;}
    let s=position(units,&obs["source"])?;
    if position(units,&trace["source"])!=Some(s){return None;}
    let kind=match trace["source"]["kind"].as_str()?{
        "address"=>"address","phone"|"mobile"=>"contact",_=>return None};
    let c=trace["candidates"].as_array().filter(|a|a.len()==2)?;
    let mut scores=BTreeMap::new();
    for row in c{
        let relation=row["relation"].as_str()?;
        if !matches!(relation,"previous"|"next")||row["anchor"]["kind"]!="name"{return None;}
        let anchor=position(units,&row["anchor"])?;
        let score=row["legacyScore"].as_i64().filter(|v|(0..=130).contains(v))?;
        if scores.insert(relation,(anchor,score)).is_some(){return None;}
    }
    let (previous,p)=*scores.get("previous")?;
    let (next,n)=*scores.get("next")?;
    if !(previous.0<s.0&&s.0<next.0){return None;}
    let delta=n-p-30;
    let (choice,anchor)=if delta>=0{("next",next)}else{("previous",previous)};
    if singleton(&obs["initialCandidates"])!=Some(choice)||
        position(units,&trace["selectedAnchor"])!=Some(anchor)||
        position(units,&obs["initialAnchor"])!=Some(anchor){return None;}
    Some((kind,owner_score_bin(delta)))
}
const OWNER_GATE_REASONS:[&str;5]=["paragraph_boundary","prior_delivery_missing",
    "blank_before_next","next_distance_exceeds_3","contact_without_nearby_address"];
// Only the first actually evaluated blocking reason is attributable.
// Reconstruct the ordered gates from the frozen trace, not from final rows.
fn owner_gate_input(units:&[u16],trace:&Value,obs:&Value)->Option<(&'static str,&'static str)>{
    let reason=*OWNER_GATE_REASONS.iter().find(|r|trace["legacyFirstBlockingReason"].as_str()==Some(**r))?;
    if trace["legacyRequiredMargin"]!=30||singleton(&obs["initialCandidates"])!=Some("previous"){return None;}
    let s=position(units,&obs["source"])?;
    if position(units,&trace["source"])!=Some(s){return None;}
    let kind=match trace["source"]["kind"].as_str()?{
        "address"=>"address","phone"|"mobile"=>"contact",_=>return None};
    let candidates=trace["candidates"].as_array().filter(|a|a.len()==2)?;
    let mut by_relation=BTreeMap::new();
    for c in candidates{
        let relation=c["relation"].as_str()?;
        if !matches!(relation,"previous"|"next")||c["anchor"]["kind"]!="name"||
            c.get("legacyScore")!=Some(&Value::Null){return None;}
        if by_relation.insert(relation,c).is_some(){return None;}
    }
    let previous=*by_relation.get("previous")?;
    let next=*by_relation.get("next")?;
    let a=position(units,&previous["anchor"])?;
    let b=position(units,&next["anchor"])?;
    if !(a.0<s.0&&s.0<b.0)||position(units,&trace["selectedAnchor"])!=Some(a)||
        position(units,&obs["initialAnchor"])!=Some(a){return None;}
    let same_previous=previous["sameParagraph"].as_bool()?;
    let same_next=next["sameParagraph"].as_bool()?;
    let prior=trace["previousHasDelivery"].as_bool()?;
    let blanks=next["gap"]["blankLines"].as_u64()?;
    let distance=next["lineDistance"].as_u64()?;
    let nearby=trace["nearbyAddress"].as_bool()?;
    let actual_reason=if same_previous||!same_next{"paragraph_boundary"}
        else if !prior{"prior_delivery_missing"}
        else if blanks>0{"blank_before_next"}
        else if distance>3{"next_distance_exceeds_3"}
        else if kind=="contact"&&!nearby{"contact_without_nearby_address"}
        else{return None;};
    (reason==actual_reason).then_some((kind,reason))
}
#[derive(Default)]
struct GateCounts{observations:usize,corrections:usize,unchanged:usize,orders:usize,order_error_sum:f64}
impl GateCounts{
    fn add(&mut self,corrections:usize,unchanged:usize){
        let n=corrections+unchanged;
        if n==0{return;}
        self.observations+=n;self.corrections+=corrections;self.unchanged+=unchanged;
        self.orders+=1;self.order_error_sum+=corrections as f64/n as f64;
    }
    fn wire(&self)->Value{
        json!({"eligibleObservations":self.observations,"explicitReassignmentObservations":self.corrections,
            "unchangedProxyObservations":self.unchanged,"independentOrders":self.orders,
            "observedCorrectionFraction":(self.observations>0).then(||self.corrections as f64/self.observations as f64),
            "orderMacroCorrectionFraction":(self.orders>0).then(||self.order_error_sum/self.orders as f64),
            "trueErrorRateVerified":false})
    }
}
#[derive(Default)]
struct OwnerFitBin{orders:usize,next_sum:f64}
impl OwnerFitBin{
    fn add_order(&mut self,next:usize,total:usize){
        if total>0{self.orders+=1;self.next_sum+=next as f64/total as f64;}
    }
    fn estimate(&self)->Option<f64>{
        (self.orders>=MIN_CALIBRATION_ORDERS).then(||(self.next_sum+1.0)/(self.orders as f64+2.0))
    }
}
#[derive(Default)]
struct OwnerTest{eligible:usize,covered:usize,brier:f64,legacy_brier:f64,
    orders:usize,covered_orders:usize,macro_brier:f64,macro_legacy_brier:f64}
impl OwnerTest{
    fn observe(&mut self,estimate:Option<f64>,old:bool,actual:bool){
        self.eligible+=1;
        if let Some(p)=estimate{
            let y=if actual{1.0}else{0.0};let baseline=if old{1.0}else{0.0};
            self.covered+=1;self.brier+=(p-y).powi(2);self.legacy_brier+=(baseline-y).powi(2);
        }
    }
    fn merge_order(&mut self,o:Self){
        if o.eligible==0{return;}
        self.orders+=1;self.eligible+=o.eligible;self.covered+=o.covered;
        self.brier+=o.brier;self.legacy_brier+=o.legacy_brier;
        if o.covered>0{
            self.covered_orders+=1;self.macro_brier+=o.brier/o.covered as f64;
            self.macro_legacy_brier+=o.legacy_brier/o.covered as f64;
        }
    }
    fn wire(&self)->Value{
        let divide=|x:f64,n:usize|(n>0).then(||x/n as f64);
        json!({"eligibleObservations":self.eligible,"coveredObservations":self.covered,
            "eligibleOrders":self.orders,"coveredOrders":self.covered_orders,
            "coverage":divide(self.covered as f64,self.eligible),
            "brierOnCovered":divide(self.brier,self.covered),
            "hardLegacyBrierOnCovered":divide(self.legacy_brier,self.covered),
            "orderMacroBrier":divide(self.macro_brier,self.covered_orders),
            "orderMacroHardLegacyBrier":divide(self.macro_legacy_brier,self.covered_orders)})
    }
}
pub(super) fn calibrate_legacy_scores(payload:&Value)->Result<Value,String>{
    calibrate_legacy_scores_prepared(payload,None)
}
fn calibrate_legacy_scores_prepared(payload:&Value,cache:Option<&ComparisonCache>)->Result<Value,String>{
    let config=&payload["config"];
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let fit_start=date(&config["fitStart"]).ok_or("보정 시작 시각 오류")?;
    let test_start=date(&config["testStart"]).ok_or("평가 시작 시각 오류")?;
    let test_end=date(&config["testEnd"]).ok_or("평가 종료 시각 오류")?;
    let as_of=date(&config["evaluationAsOf"]).ok_or("평가 기준 시각 오류")?;
    if !(fit_start<test_start&&test_start<test_end&&test_end<=as_of){return Err("보정·평가 기간 순서 오류".into());}
    let build=config["clientBuildVersion"].as_str().filter(|s|!s.is_empty()&&s.len()<=128)
        .ok_or("보정 대상 빌드 버전 필요")?;
    let orders=payload["orders"].as_array().filter(|v|!v.is_empty()&&v.len()<=250).ok_or("보정 주문 한도 오류")?;
    let mut seen=BTreeSet::new();
    let mut bins=BTreeMap::<(&str,usize),OwnerFitBin>::new();
    let mut tests=Vec::new();
    let mut gate_fit=BTreeMap::<(&str,&str),OwnerFitBin>::new();
    let mut gate_fit_counts=BTreeMap::<(&str,&str),GateCounts>::new();
    let mut gate_test_counts=BTreeMap::<(&str,&str),GateCounts>::new();
    let mut gate_tests=Vec::new();
    let mut invalid_gates=0usize;
    let (mut skipped,mut late,mut blocked,mut invalid,mut unverified)=(0usize,0usize,0usize,0usize,0usize);
    let (mut fit_orders,mut test_orders)=(0usize,0usize);
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("보정 주문 ID 오류")?;
        if !seen.insert(id){return Err("보정 주문 ID 중복".into());}
        let snap=&order["confirmationSnapshot"];
        let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let capture=&first["initialPredictionCapture"];
        let trace=&first["recipientOwnerTrace"];
        let (Some(start),Some(end),Some(confirmed))=(date(&capture["analysisStartedAt"]),
            date(&capture["analysisCompletedAt"]),date(&order["confirmedAt"]))else{skipped+=1;continue;};
        if order["learningActive"]!=true||order["deletedAt"].as_str().is_some_and(|s|!s.is_empty())||
            capture["origin"]!="native_reception_worker"||capture["schemaVersion"]!=1||
            capture["orderId"].as_str()!=Some(id)||capture["clientBuildVersion"].as_str()!=Some(build)||
            trace["schemaVersion"]!=1||trace["scorePolicyVersion"]!=OWNER_SCORE_POLICY||
            snap["at"]!=order["confirmedAt"]||!(start<=end&&end<confirmed)||
            start<fit_start||end>=test_end{skipped+=1;continue;}
        // Keep entire orders on one side of the split; no late label fitting.
        if start<test_start&&end>=test_start{skipped+=1;continue;}
        let fit=end<test_start;
        if (fit&&confirmed>=test_start)||(!fit&&confirmed>as_of){late+=1;continue;}
        let Some(events)=order["recognitionAuditEvents"].as_array()else{skipped+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=end||t>confirmed)){skipped+=1;continue;}
        let compared=shared_comparison(order,cache)?;
        if compared["auditCoverage"]!="verified_manifest"{skipped+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("보정 원문 누락")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let mut samples=Vec::new();let mut gate_samples=Vec::new();let mut used=BTreeSet::new();
        for obs in rows(&compared["observations"]).filter(|o|o["task"]=="recipient_ownership"){
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",
                _=>{unverified+=1;continue;}
            };
            let (Some(old),Some(actual),Some(s))=(singleton(&obs["initialCandidates"]),
                singleton(&obs["finalCandidates"]),position(&units,&obs["source"]))else{invalid+=1;continue;};
            if !matches!(old,"previous"|"next")||!matches!(actual,"previous"|"next")||
                (label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||
                    singleton(&obs["negativeCandidates"])!=Some(old)))||
                (label=="unchanged_proxy"&&old!=actual){invalid+=1;continue;}
            if !used.insert(s){return Err("보정 관측 위치 중복".into());}
            let mut traces=rows(&trace["decisions"]).filter(|d|position(&units,&d["source"])==Some(s));
            let Some(d)=traces.next()else{invalid+=1;continue;};
            if traces.next().is_some(){invalid+=1;continue;}
            if !d["legacyFirstBlockingReason"].is_null(){
                blocked+=1;
                if let Some((kind,reason))=owner_gate_input(&units,d,obs){
                    gate_samples.push((kind,reason,label,actual=="next"));
                }else{invalid_gates+=1;}
                continue;
            }
            let Some((kind,bin))=owner_score_input(&units,d,obs)else{invalid+=1;continue;};
            samples.push((kind,bin,label,old=="next",actual=="next"));
        }
        // Gate data has its own denominator and never enters numeric-score bins.
        if !gate_samples.is_empty(){
            let mut local=BTreeMap::<(&str,&str),(usize,usize)>::new();
            for &(kind,reason,_,actual) in &gate_samples{
                let c=local.entry((kind,reason)).or_default();
                if actual{c.0+=1;}else{c.1+=1;}
            }
            for (key,(corrections,unchanged)) in local{
                if fit{
                    gate_fit_counts.entry(key).or_default().add(corrections,unchanged);
                    gate_fit.entry(key).or_default().add_order(corrections,corrections+unchanged);
                }else{gate_test_counts.entry(key).or_default().add(corrections,unchanged);}
            }
            if !fit{gate_tests.push(gate_samples);}
        }
        if samples.is_empty(){continue;}
        if fit{
            fit_orders+=1;
            let mut local=BTreeMap::<(&str,usize),(usize,usize)>::new();
            for (kind,bin,_,_,actual) in samples{
                let n=local.entry((kind,bin)).or_default();n.0+=usize::from(actual);n.1+=1;
            }
            for (key,(next,total)) in local{bins.entry(key).or_default().add_order(next,total);}
        }else{test_orders+=1;tests.push(samples);}
    }
    let mut totals=BTreeMap::<(&str,&str),OwnerTest>::new();
    for samples in tests{
        let mut local=BTreeMap::<(&str,&str),OwnerTest>::new();
        for (kind,bin,label,old,actual) in samples{
            let p=bins.get(&(kind,bin)).and_then(OwnerFitBin::estimate);
            for group in [label,"mixed_proxy"]{local.entry((kind,group)).or_default().observe(p,old,actual);}
        }
        for (key,value) in local{totals.entry(key).or_default().merge_order(value);}
    }
    let intervals=["<=-61","-60..-31","-30..-11","-10..-1","0..9","10..29","30..59",">=60"];
    let mut table=Vec::new();
    for kind in ["address","contact"]{
        for (bin,interval) in intervals.iter().enumerate(){
            let empty=OwnerFitBin::default();let f=bins.get(&(kind,bin)).unwrap_or(&empty);
            table.push(json!({"sourceKind":kind,"bin":bin,"adjustedMargin":interval,
                "fitOrders":f.orders,"orderWeightedNextMass":f.next_sum,
                "estimatedNextProxyFrequency":f.estimate(),
                "estimatedPreviousProxyFrequency":f.estimate().map(|p|1.0-p)}));
        }
    }
    let groups=totals.into_iter().map(|((kind,label),v)|json!({"sourceKind":kind,"labelKind":label,
        "metrics":v.wire()})).collect::<Vec<_>>();
    let mut gate_totals=BTreeMap::<(&str,&str,&str),OwnerTest>::new();
    for samples in gate_tests{
        let mut local=BTreeMap::<(&str,&str,&str),OwnerTest>::new();
        for (kind,reason,label,actual) in samples{
            let p=gate_fit.get(&(kind,reason)).and_then(OwnerFitBin::estimate);
            for group in [label,"mixed_proxy"]{
                local.entry((kind,reason,group)).or_default().observe(p,false,actual);
            }
        }
        for (key,value) in local{gate_totals.entry(key).or_default().merge_order(value);}
    }
    let mut gates=Vec::new();
    for kind in ["address","contact"]{
        for reason in OWNER_GATE_REASONS{
            if kind=="address"&&reason=="contact_without_nearby_address"{continue;}
            let empty=GateCounts::default();
            let fit=gate_fit_counts.get(&(kind,reason)).unwrap_or(&empty);
            let test=gate_test_counts.get(&(kind,reason)).unwrap_or(&empty);
            let estimate=gate_fit.get(&(kind,reason)).and_then(OwnerFitBin::estimate);
            gates.push(json!({"sourceKind":kind,"firstBlockingReason":reason,
                "fit":fit.wire(),"test":test.wire(),
                "estimatedNextProxyFrequency":estimate,
                "estimatedPreviousProxyFrequency":estimate.map(|p|1.0-p)}));
        }
    }
    let gate_groups=gate_totals.into_iter().map(|((kind,reason,label),v)|json!({
        "sourceKind":kind,"firstBlockingReason":reason,"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>();
    Ok(json!({"status":"legacy_score_calibration_diagnostic_only","method":"fixed_bins_order_weighted_beta_1_1",
        "scorePolicyVersion":OWNER_SCORE_POLICY,"clientBuildVersion":build,"minimumFitOrdersPerBin":MIN_CALIBRATION_ORDERS,
        "fitStart":config["fitStart"],"testStart":config["testStart"],"testEnd":config["testEnd"],
        "evaluationAsOf":config["evaluationAsOf"],"fitOrders":fit_orders,"testOrders":test_orders,
        "excludedOrders":skipped,"lateLabelOrders":late,"blockedObservations":blocked,
        "invalidObservations":invalid,"unverifiedObservations":unverified,"bins":table,"testGroups":groups,
        "gateCalibration":{"method":"first_blocking_reason_order_weighted_beta_1_1",
            "minimumFitOrders":MIN_CALIBRATION_ORDERS,"invalidGateObservations":invalid_gates,
            "groups":gates,"testGroups":gate_groups,"onlyFirstBlockingReasonAttributed":true,
            "trueErrorRateVerified":false,"calibratedProbability":null,"decisionChanged":false,
            "eligibleForAutomaticDecision":false},
        "fitLabelKind":"mixed_operational_proxy","representativeLabelsVerified":false,
        "historicalModelVerified":false,"sourceProvenanceVerified":false,
        "calibratedProbability":null,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// P07-02: multiclass calibration on the original captured support vector.
// Last component is pooled outside-candidate mass, not a selectable SKU.
const SHADOW_TEMPERATURES:[f64;5]=[1.0,0.5,0.75,1.5,2.0];
const SHADOW_SMOOTHING:[f64;5]=[0.0,0.01,0.05,0.10,0.20];
type ShadowGroup=(String,String,String);
struct ShadowCalibrationSample{
    distribution:Vec<f64>,actual:usize,label:&'static str,
}
fn shadow_distribution(report:&Value,allowed:&BTreeSet<&str>,actual:&str)
    ->Option<ShadowCalibrationSample>{
    if allowed.is_empty()||allowed.len()>64{return None;}
    let total=report["totalSupportMass"].as_f64().filter(|v|v.is_finite()&&*v>0.0)?;
    let outside=report["outsideCandidateSupportMass"].as_f64().filter(|v|v.is_finite()&&*v>=0.0)?;
    let mut masses=BTreeMap::new();
    for c in report["candidates"].as_array()?{
        let id=c["candidate"].as_str().filter(|id|allowed.contains(id))?;
        let mass=c["supportMass"].as_f64().filter(|v|v.is_finite()&&*v>=0.0)?;
        if masses.insert(id,mass).is_some(){return None;}
    }
    if masses.len()!=allowed.len(){return None;}
    let sum=masses.values().sum::<f64>()+outside;
    if !sum.is_finite()||sum<=0.0||(sum-total).abs()>1e-7*total.max(1.0){return None;}
    let mut distribution=allowed.iter().map(|id|masses[id]/sum).collect::<Vec<_>>();
    distribution.push(outside/sum);
    let actual=allowed.iter().position(|id|*id==actual).unwrap_or(allowed.len());
    Some(ShadowCalibrationSample{distribution,actual,label:""})
}
fn shadow_transform(raw:&[f64],temperature:f64,smoothing:f64)->Vec<f64>{
    let power=raw.iter().map(|p|p.powf(1.0/temperature)).collect::<Vec<_>>();
    let sum=power.iter().sum::<f64>();
    let uniform=smoothing/raw.len() as f64;
    power.into_iter().map(|p|(1.0-smoothing)*p/sum+uniform).collect()
}
fn multiclass_brier(p:&[f64],actual:usize)->f64{
    p.iter().enumerate().map(|(i,x)|(x-if i==actual{1.0}else{0.0}).powi(2)).sum()
}
#[derive(Default)]
struct ShadowFit{orders:usize,losses:[f64;25]}
impl ShadowFit{
    fn add_order(&mut self,samples:&[ShadowCalibrationSample]){
        if samples.is_empty(){return;}
        self.orders+=1;
        let mut sums=[0.0;25];
        for v in samples{
            for (i,t) in SHADOW_TEMPERATURES.iter().enumerate(){
                let power=v.distribution.iter().map(|p|p.powf(1.0/t)).collect::<Vec<_>>();
                let total=power.iter().sum::<f64>();
                for (j,s) in SHADOW_SMOOTHING.iter().enumerate(){
                    let uniform=s/power.len() as f64;
                    sums[i*5+j]+=power.iter().enumerate().map(|(k,p)|
                        ((1.0-s)*p/total+uniform-if k==v.actual{1.0}else{0.0}).powi(2)).sum::<f64>();
                }
            }
        }
        for (loss,sum) in self.losses.iter_mut().zip(sums){*loss+=sum/samples.len() as f64;}
    }
    fn parameters(&self)->Option<(f64,f64)>{
        if self.orders<MIN_CALIBRATION_ORDERS{return None;}
        // Prefer identity on exact ties. No tuning on evaluation orders.
        let mut best=0;
        for i in 1..self.losses.len(){
            if self.losses[i]+1e-10<self.losses[best]{best=i;}
        }
        Some((SHADOW_TEMPERATURES[best/5],SHADOW_SMOOTHING[best%5]))
    }
}
#[derive(Default)]
struct ShadowCalTest{
    eligible:usize,covered:usize,outside:usize,orders:usize,covered_orders:usize,
    raw_loss:f64,adjusted_loss:f64,macro_raw:f64,macro_adjusted:f64,
}
impl ShadowCalTest{
    fn observe(&mut self,v:&ShadowCalibrationSample,params:Option<(f64,f64)>){
        self.eligible+=1;self.outside+=usize::from(v.actual+1==v.distribution.len());
        if let Some((t,s))=params{
            self.covered+=1;
            self.raw_loss+=multiclass_brier(&v.distribution,v.actual);
            self.adjusted_loss+=multiclass_brier(&shadow_transform(&v.distribution,t,s),v.actual);
        }
    }
    fn merge(&mut self,o:Self){
        if o.eligible==0{return;}
        self.orders+=1;self.eligible+=o.eligible;self.covered+=o.covered;self.outside+=o.outside;
        self.raw_loss+=o.raw_loss;self.adjusted_loss+=o.adjusted_loss;
        if o.covered>0{self.covered_orders+=1;
            self.macro_raw+=o.raw_loss/o.covered as f64;
            self.macro_adjusted+=o.adjusted_loss/o.covered as f64;}
    }
    fn wire(&self)->Value{
        let div=|x:f64,n:usize|(n>0).then(||x/n as f64);
        json!({"eligibleObservations":self.eligible,"coveredObservations":self.covered,
            "outsideCandidateObservations":self.outside,"independentOrders":self.orders,
            "coveredOrders":self.covered_orders,"coverage":div(self.covered as f64,self.eligible),
            "rawBrierOnCovered":div(self.raw_loss,self.covered),
            "adjustedBrierOnCovered":div(self.adjusted_loss,self.covered),
            "orderMacroRawBrier":div(self.macro_raw,self.covered_orders),
            "orderMacroAdjustedBrier":div(self.macro_adjusted,self.covered_orders)})
    }
}
pub(super) fn calibrate_shadow_candidates(payload:&Value)->Result<Value,String>{
    calibrate_shadow_candidates_prepared(payload,None)
}
fn calibrate_shadow_candidates_prepared(payload:&Value,cache:Option<&ComparisonCache>)->Result<Value,String>{
    let config=&payload["config"];
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let fit_start=date(&config["fitStart"]).ok_or("학습 보정 시작 시각 오류")?;
    let test_start=date(&config["testStart"]).ok_or("학습 평가 시작 시각 오류")?;
    let test_end=date(&config["testEnd"]).ok_or("학습 평가 종료 시각 오류")?;
    let as_of=date(&config["evaluationAsOf"]).ok_or("학습 평가 기준 시각 오류")?;
    if !(fit_start<test_start&&test_start<test_end&&test_end<=as_of){return Err("학습 보정 기간 순서 오류".into());}
    let build=config["clientBuildVersion"].as_str().filter(|s|!s.is_empty()&&s.len()<=128)
        .ok_or("보정 대상 빌드 버전 필요")?;
    let orders=payload["orders"].as_array().filter(|a|!a.is_empty()&&a.len()<=250).ok_or("학습 보정 주문 한도 오류")?;
    let mut seen=BTreeSet::new();
    let mut fits=BTreeMap::<ShadowGroup,ShadowFit>::new();
    let mut tests=Vec::new();
    let (mut skipped,mut late,mut invalid,mut missing,mut labels_excluded)=(0usize,0usize,0usize,0usize,0usize);
    let (mut fit_orders,mut test_orders)=(0usize,0usize);
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("학습 보정 주문 ID 오류")?;
        if !seen.insert(id){return Err("학습 보정 주문 ID 중복".into());}
        let snap=&order["confirmationSnapshot"];
        let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let capture=&first["initialPredictionCapture"];
        let shadow=&capture["learningShadow"];
        let (Some(start),Some(end),Some(predicted),Some(confirmed))=(date(&capture["analysisStartedAt"]),
            date(&capture["analysisCompletedAt"]),date(&shadow["capturedAt"]),date(&order["confirmedAt"]))
            else{skipped+=1;continue;};
        if order["learningActive"]!=true||order["deletedAt"].as_str().is_some_and(|s|!s.is_empty())||
            capture["origin"]!="native_reception_worker"||capture["schemaVersion"]!=1||
            capture["orderId"].as_str()!=Some(id)||capture["clientBuildVersion"].as_str()!=Some(build)||
            shadow["status"]!="captured_shadow_support"||shadow["captureVersion"]!=4||
            shadow["explicitToWeakRatio"].as_f64()!=Some(3.0)||shadow["sourceDigestsMatched"]!=true||
            shadow["decisionChanged"]!=false||shadow["modelId"].as_str().is_none_or(|s|!s.starts_with("shadow-")||s.len()!=71)||
            snap["at"]!=order["confirmedAt"]||!(start<=end&&end<=predicted&&predicted<confirmed)||
            start<fit_start||predicted>=test_end||start<test_start&&predicted>=test_start{skipped+=1;continue;}
        let fit=predicted<test_start;
        if (fit&&confirmed>=test_start)||(!fit&&confirmed>as_of){late+=1;continue;}
        let Some(events)=order["recognitionAuditEvents"].as_array()else{skipped+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=predicted||t>confirmed)){skipped+=1;continue;}
        let compared=shared_comparison(order,cache)?;
        if compared["auditCoverage"]!="verified_manifest"{skipped+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("학습 보정 원문 누락")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let projection=super::learning_context::lookup(&json!({"raw":raw,"initialAnalysis":first}))?;
        let mut local=BTreeMap::<ShadowGroup,Vec<ShadowCalibrationSample>>::new();
        let mut used=BTreeSet::new();
        for obs in rows(&compared["observations"]){
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",
                _=>{labels_excluded+=1;continue;}
            };
            let (Some(old),Some(actual),Some(s),Some(task))=(singleton(&obs["initialCandidates"]),
                singleton(&obs["finalCandidates"]),position(&units,&obs["source"]),obs["task"].as_str())
                else{labels_excluded+=1;continue;};
            if label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||
                singleton(&obs["negativeCandidates"])!=Some(old))||label=="unchanged_proxy"&&old!=actual{
                labels_excluded+=1;continue;
            }
            if !used.insert((s,task)){return Err("학습 보정 관측 위치 중복".into());}
            let expected=if task=="recipient_ownership"{&projection["entries"]}else{&projection["fieldEntries"]};
            let mut entries=rows(expected).filter(|v|position(&units,&v["source"])==Some(s)&&
                (task=="recipient_ownership"||v["task"].as_str()==Some(task)));
            let Some(entry)=entries.next()else{invalid+=1;continue;};
            if entries.next().is_some(){invalid+=1;continue;}
            let mut locs=rows(&shadow["locations"]).filter(|v|position(&units,v)==Some(s)&&v["task"].as_str()==Some(task));
            let Some(loc)=locs.next()else{missing+=1;continue;};
            if locs.next().is_some(){invalid+=1;continue;}
            let allowed=rows(&loc["candidates"]).filter_map(Value::as_str).collect::<BTreeSet<_>>();
            let expected_ids=if task=="recipient_ownership"{BTreeSet::from(["previous","next"])}
                else{rows(&entry["shadowCandidates"]).filter_map(Value::as_str).collect()};
            let coverage=loc["candidateCoverage"].as_str().unwrap_or("");
            if allowed!=expected_ids||!allowed.contains(old)||allowed.len()!=rows(&loc["candidates"]).count()||
                !matches!(coverage,"traced_pair"|"traced_partial"|"observed_only")||
                (task=="recipient_ownership"&&coverage!="traced_pair")||
                (task!="recipient_ownership"&&entry["candidateCoverage"].as_str()!=Some(coverage)){
                invalid+=1;continue;
            }
            let mut tiers=BTreeSet::new();
            for ctx in rows(&entry["contexts"]){
                let (Some(key),Some(level))=(ctx["key"].as_str(),ctx["level"].as_str())else{invalid+=1;continue;};
                if !tiers.insert(level)||!rows(&loc["contextKeys"]).any(|k|k.as_str()==Some(key)){invalid+=1;continue;}
                let mut reports=rows(&shadow["contexts"]).filter(|r|r["task"].as_str()==Some(task)&&
                    r["contextKey"].as_str()==Some(key)&&r["supportVersion"]==4&&
                    r["featureVersion"]==1&&r["contributionVersion"]==2&&
                    rows(&r["candidateIds"]).filter_map(Value::as_str).collect::<BTreeSet<_>>()==allowed);
                let Some(r)=reports.next()else{missing+=1;continue;};
                if reports.next().is_some(){invalid+=1;continue;}
                let Some(mut sample)=shadow_distribution(r,&allowed,actual)else{missing+=1;continue;};
                sample.label=label;
                local.entry((task.to_owned(),level.to_owned(),coverage.to_owned())).or_default().push(sample);
            }
        }
        if local.is_empty(){continue;}
        if fit{fit_orders+=1;for (key,samples) in local{fits.entry(key).or_default().add_order(&samples);}}
        else{test_orders+=1;tests.push(local);}
    }
    let mut totals=BTreeMap::<(ShadowGroup,String),ShadowCalTest>::new();
    for order in tests{
        for (key,samples) in order{
            let params=fits.get(&key).and_then(ShadowFit::parameters);
            let mut local=BTreeMap::<&str,ShadowCalTest>::new();
            for sample in samples{
                for label in [sample.label,"mixed_proxy"]{local.entry(label).or_default().observe(&sample,params);}
            }
            for (label,metrics) in local{totals.entry((key.clone(),label.to_owned())).or_default().merge(metrics);}
        }
    }
    let models=fits.into_iter().map(|((task,level,coverage),v)|{
        let params=v.parameters();
        json!({"task":task,"contextLevel":level,"candidateCoverage":coverage,"fitOrders":v.orders,
            "temperature":params.map(|p|p.0),"uniformSmoothing":params.map(|p|p.1)})
    }).collect::<Vec<_>>();
    let groups=totals.into_iter().map(|(((task,level,coverage),label),v)|json!({
        "task":task,"contextLevel":level,"candidateCoverage":coverage,"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>();
    Ok(json!({"status":"shadow_candidate_calibration_diagnostic_only",
        "method":"order_macro_multiclass_brier_grid_v1","captureVersion":4,"supportVersion":4,
        "clientBuildVersion":build,"explicitToWeakRatio":3.0,"minimumFitOrders":MIN_CALIBRATION_ORDERS,
        "fitStart":config["fitStart"],"testStart":config["testStart"],"testEnd":config["testEnd"],"evaluationAsOf":config["evaluationAsOf"],
        "fitOrders":fit_orders,"testOrders":test_orders,"excludedOrders":skipped,"lateLabelOrders":late,
        "invalidJoins":invalid,"missingDistributions":missing,"excludedUnverifiedLabels":labels_excluded,
        "models":models,"testGroups":groups,"outsideBucketSelectable":false,
        "fitLabelKind":"mixed_operational_proxy","representativeLabelsVerified":false,"historicalModelVerified":false,
        "sourceProvenanceVerified":false,"calibratedProbability":null,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// P07-03: two independently fitted proxy distributions, one frozen weight.
// Ownership is the only task with both sides calibrated. Outside is a
// nonselectable category, including in the mixed distribution.
const MIN_BLEND_ORDERS:usize=10;
type BlendGroup=(String,String,String);
struct BlendSample{
    legacy:Vec<f64>,shadow:Vec<f64>,actual:usize,label:&'static str,
    legacy_choice:usize,transfer_block:Option<&'static str>,
}
// A source transfer must be lossless. Never overwrite another recipient's
// delivery field, move a fragment of a combined value, or detach explicit
// same-line identity. This check uses only the first analysis, no final label.
#[cfg(test)]
fn owner_transfer_block(units:&[u16],first:&Value,d:&Value,s:Span,old:&str)->Option<&'static str>{
    owner_transfer_indexed(units,first,&OwnerIndex::new(units,&first["recipients"]),d,s,old)
}
fn owner_transfer_indexed(units:&[u16],first:&Value,index:&OwnerIndex<'_>,d:&Value,s:Span,old:&str)->Option<&'static str>{
    let fail=||->Option<Option<&'static str>>{
        let links=index.links.get(&s)?;
        if links.len()!=1{return Some(Some("ambiguous_source_link"));}
        let link=&links[0];
        if !delivery(link.field){return Some(Some("not_delivery_field"));}
        let source=String::from_utf16(&units[s.0..s.1]).ok()?;
        if source.trim()!=link.value.trim(){return Some(Some("partial_or_modified_source_value"));}
        if index.links.keys().any(|other|*other!=s&&other.0<s.1&&s.0<other.1){
            return Some(Some("overlapping_source_links"));
        }
        let old_anchor=index.unique_anchor(link.row)?;
        if position(units,&d["selectedAnchor"])!=Some(old_anchor){return None;}
        let target=if old=="next"{"previous"}else{"next"};
        let mut candidates=rows(&d["candidates"]).filter(|c|c["relation"]==target);
        let candidate=candidates.next()?;if candidates.next().is_some(){return None;}
        let anchor=position(units,&candidate["anchor"])?;
        if !index.has_unique_anchor(anchor)||anchor==old_anchor{return None;}
        let owners=index.names.get(&anchor)?;
        let target_row=*owners.iter().next()?;
        let row=first["recipients"].as_array()?.get(target_row)?;
        let between=if old_anchor.1<=s.0{&units[old_anchor.1..s.0]}
            else if s.1<=old_anchor.0{&units[s.1..old_anchor.0]}else{return None;};
        if !between.iter().any(|u|*u==10||*u==13){return Some(Some("same_line_explicit_identity_protected"));}
        let occupied=if link.field=="address"{
            row["address"].as_str().is_some_and(|v|!v.trim().is_empty())
        }else{["phone","mobile"].iter().any(|field|
            row[*field].as_str().is_some_and(|v|!v.trim().is_empty()))};
        if occupied{return Some(Some("target_delivery_field_occupied"));}
        // Even if the field value is blank, a conflicting source link is
        // not an available destination.
        if rows(&row["sources"]).any(|source|source["field"].as_str().is_some_and(|field|
            if link.field=="address"{field=="address"}else{contact(field)})){
            return Some(Some("target_delivery_source_exists"));
        }
        Some(None)
    };
    fail().unwrap_or(Some("unverified_source_or_anchor"))
}
const MIN_CHANGE_PROXY:f64=0.80;
const MIN_CHANGE_MARGIN:f64=0.25;
const MAX_CHANGE_OUTSIDE:f64=0.05;
// Pure diagnostic proposal. No operational authority is issued even if
// every screening check passes. Never consume evaluation labels here.
fn screen_blend_change(sample:&BlendSample,w:Option<f64>)->(&'static str,Option<usize>){
    let Some(w)=w.filter(|w|w.is_finite()&&(0.0..=1.0).contains(w))else{return ("missing_weight",None);};
    let valid=|v:&[f64]|v.len()==3&&v.iter().all(|p|p.is_finite()&&*p>=0.0&&*p<=1.0)&&
        (v.iter().sum::<f64>()-1.0).abs()<1e-8;
    if sample.legacy_choice>1||!valid(&sample.legacy)||!valid(&sample.shadow){return ("invalid_distribution",None);}
    let p=blend_distribution(&sample.legacy,&sample.shadow,w);
    let target=1-sample.legacy_choice;
    if p[sample.legacy_choice]>=p[target]{return ("retain_legacy",None);}
    if let Some(reason)=sample.transfer_block{return (reason,None);}
    if w<=0.0{return ("learning_weight_zero",None);}
    if p[2]>MAX_CHANGE_OUTSIDE+1e-10{return ("outside_mass_too_high",None);}
    if p[target]<MIN_CHANGE_PROXY-1e-10{return ("low_combined_support",None);}
    if p[target]-p[sample.legacy_choice].max(p[2])<MIN_CHANGE_MARGIN-1e-10{return ("small_combined_margin",None);}
    if sample.shadow[target]<=sample.shadow[sample.legacy_choice].max(sample.shadow[2])+1e-10{
        return ("shadow_does_not_support_change",None);
    }
    ("diagnostic_proposal_only",Some(target))
}
#[derive(Default)]
struct ChangeScreenMetrics{
    considered:usize,proposed:usize,improved:usize,worsened:usize,
    reasons:BTreeMap<&'static str,usize>,
}
impl ChangeScreenMetrics{
    fn observe(&mut self,s:&BlendSample,w:Option<f64>){
        self.considered+=1;
        let (reason,target)=screen_blend_change(s,w);
        *self.reasons.entry(reason).or_default()+=1;
        if let Some(target)=target{
            self.proposed+=1;
            if target==s.actual&&s.legacy_choice!=s.actual{self.improved+=1;}
            if target!=s.actual&&s.legacy_choice==s.actual{self.worsened+=1;}
        }
    }
    fn wire(&self)->Value{
        json!({"considered":self.considered,"diagnosticProposals":self.proposed,
            "improvedAgainstOperationalLabel":self.improved,"worsenedAgainstOperationalLabel":self.worsened,
            "reasons":self.reasons,"actualChanges":0,"eligibleForAutomaticDecision":false})
    }
}
fn blend_distribution(a:&[f64],b:&[f64],w:f64)->Vec<f64>{
    a.iter().zip(b).map(|(x,y)|(1.0-w)*x+w*y).collect()
}
#[derive(Default)]
struct BlendFit{orders:usize,loss:[f64;11]}
impl BlendFit{
    fn add_order(&mut self,samples:&[BlendSample]){
        if samples.is_empty(){return;}
        self.orders+=1;
        for (i,loss) in self.loss.iter_mut().enumerate(){
            let w=i as f64/10.0;
            *loss+=samples.iter().map(|s|s.legacy.iter().zip(&s.shadow).enumerate().map(|(j,(a,b))|
                ((1.0-w)*a+w*b-if j==s.actual{1.0}else{0.0}).powi(2)).sum::<f64>())
                .sum::<f64>()/samples.len() as f64;
        }
    }
    fn weight(&self)->Option<f64>{
        if self.orders<MIN_BLEND_ORDERS{return None;}
        let mut best=0;
        for i in 1..11{if self.loss[i]+1e-10<self.loss[best]{best=i;}}
        Some(best as f64/10.0)
    }
}
#[derive(Default)]
struct BlendTest{eligible:usize,covered:usize,orders:usize,covered_orders:usize,
    legacy:f64,shadow:f64,combined:f64,macro_legacy:f64,macro_shadow:f64,macro_combined:f64}
impl BlendTest{
    fn observe(&mut self,sample:Option<&BlendSample>,w:Option<f64>){
        self.eligible+=1;
        if let (Some(s),Some(w))=(sample,w){
            self.covered+=1;
            self.legacy+=multiclass_brier(&s.legacy,s.actual);
            self.shadow+=multiclass_brier(&s.shadow,s.actual);
            self.combined+=multiclass_brier(&blend_distribution(&s.legacy,&s.shadow,w),s.actual);
        }
    }
    fn merge(&mut self,o:Self){
        if o.eligible==0{return;}
        self.orders+=1;self.eligible+=o.eligible;self.covered+=o.covered;
        self.legacy+=o.legacy;self.shadow+=o.shadow;self.combined+=o.combined;
        if o.covered>0{
            self.covered_orders+=1;let n=o.covered as f64;
            self.macro_legacy+=o.legacy/n;self.macro_shadow+=o.shadow/n;self.macro_combined+=o.combined/n;
        }
    }
    fn wire(&self)->Value{
        let div=|x:f64,n:usize|(n>0).then(||x/n as f64);
        json!({"eligibleObservations":self.eligible,"coveredObservations":self.covered,
            "independentOrders":self.orders,"coveredOrders":self.covered_orders,
            "coverage":div(self.covered as f64,self.eligible),
            "legacyBrierOnCovered":div(self.legacy,self.covered),
            "shadowBrierOnCovered":div(self.shadow,self.covered),
            "combinedBrierOnCovered":div(self.combined,self.covered),
            "orderMacroLegacyBrier":div(self.macro_legacy,self.covered_orders),
            "orderMacroShadowBrier":div(self.macro_shadow,self.covered_orders),
            "orderMacroCombinedBrier":div(self.macro_combined,self.covered_orders)})
    }
}
type ComparisonCache=std::cell::RefCell<BTreeMap<String,std::rc::Rc<Value>>>;
fn shared_comparison(order:&Value,cache:Option<&ComparisonCache>)->Result<std::rc::Rc<Value>,String>{
    let Some(cache)=cache else{return Ok(std::rc::Rc::new(compare(order)?));};
    let id=order["id"].as_str().ok_or("학습 주문 ID 누락")?;
    if let Some(value)=cache.borrow().get(id){return Ok(value.clone());}
    let value=std::rc::Rc::new(compare(order)?);
    cache.borrow_mut().insert(id.to_owned(),value.clone());
    Ok(value)
}
pub(super) fn learn_blend_weights(payload:&Value)->Result<Value,String>{
    let config=&payload["config"];
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let fit_start=date(&config["fitStart"]).ok_or("결합 보정 시작 시각 오류")?;
    let weight_start=date(&config["weightStart"]).ok_or("결합 가중치 시작 시각 오류")?;
    let test_start=date(&config["testStart"]).ok_or("결합 평가 시작 시각 오류")?;
    let test_end=date(&config["testEnd"]).ok_or("결합 평가 종료 시각 오류")?;
    let as_of=date(&config["evaluationAsOf"]).ok_or("결합 평가 기준 시각 오류")?;
    if !(fit_start<weight_start&&weight_start<test_start&&test_start<test_end&&test_end<=as_of){
        return Err("결합 보정·가중치·평가 기간 순서 오류".into());
    }
    let build=config["clientBuildVersion"].as_str().filter(|s|!s.is_empty()&&s.len()<=128)
        .ok_or("결합 빌드 버전 필요")?;
    let orders=payload["orders"].as_array().filter(|a|!a.is_empty()&&a.len()<=250).ok_or("결합 주문 한도 오류")?;
    let mut seen=BTreeSet::new();
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("결합 주문 ID 오류")?;
        if !seen.insert(id){return Err("결합 주문 ID 중복".into());}
    }
    // No weight/test order is even passed to either calibration fit.
    let fit_orders=orders.iter().filter(|o|date(&o["confirmedAt"]).is_some_and(|t|t<weight_start))
        .cloned().collect::<Vec<_>>();
    if fit_orders.is_empty(){return Ok(json!({"status":"no_calibration_orders",
        "eligibleForAutomaticDecision":false,"decisionChanged":false}));}
    let fit_config=json!({"fitStart":config["fitStart"],"testStart":config["weightStart"],
        "testEnd":config["testStart"],"evaluationAsOf":config["testStart"],"clientBuildVersion":build});
    let fit_payload=json!({"orders":fit_orders,"config":fit_config});
    let cache=ComparisonCache::default();
    let legacy=calibrate_legacy_scores_prepared(&fit_payload,Some(&cache))?;
    let shadow_model=calibrate_shadow_candidates_prepared(&fit_payload,Some(&cache))?;
    drop(cache);
    let mut fits=BTreeMap::<BlendGroup,BlendFit>::new();
    let mut tests=Vec::new();
    let (mut excluded,mut late,mut unavailable,mut valid_weight_orders,mut valid_test_orders)=(0usize,0usize,0usize,0usize,0usize);
    let mut eligible_weight=0usize;let mut eligible_test=0usize;
    for order in orders{
        let snap=&order["confirmationSnapshot"];
        let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let capture=&first["initialPredictionCapture"];let shadow=&capture["learningShadow"];
        let (Some(start),Some(end),Some(predicted),Some(confirmed))=(date(&capture["analysisStartedAt"]),
            date(&capture["analysisCompletedAt"]),date(&shadow["capturedAt"]),date(&order["confirmedAt"]))
            else{excluded+=1;continue;};
        if start<weight_start{continue;}
        if order["learningActive"]!=true||order["deletedAt"].as_str().is_some_and(|v|!v.is_empty())||
            capture["origin"]!="native_reception_worker"||capture["schemaVersion"]!=1||
            capture["orderId"]!=order["id"]||capture["clientBuildVersion"].as_str()!=Some(build)||
            first["recipientOwnerTrace"]["schemaVersion"]!=1||
            first["recipientOwnerTrace"]["scorePolicyVersion"]!=OWNER_SCORE_POLICY||
            shadow["status"]!="captured_shadow_support"||shadow["captureVersion"]!=4||
            shadow["explicitToWeakRatio"].as_f64()!=Some(3.0)||shadow["sourceDigestsMatched"]!=true||
            shadow["decisionChanged"]!=false||shadow["modelId"].as_str().is_none_or(|s|!s.starts_with("shadow-")||s.len()!=71)||
            snap["at"]!=order["confirmedAt"]||!(start<=end&&end<=predicted&&predicted<confirmed)||
            predicted>=test_end||start<test_start&&predicted>=test_start{excluded+=1;continue;}
        let fit=predicted<test_start;
        if (fit&&confirmed>=test_start)||(!fit&&confirmed>as_of){late+=1;continue;}
        let Some(events)=order["recognitionAuditEvents"].as_array()else{excluded+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=predicted||t>confirmed)){excluded+=1;continue;}
        let compared=compare(order)?;
        if compared["auditCoverage"]!="verified_manifest"{excluded+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("결합 원문 누락")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let owner_index=OwnerIndex::new(&units,&first["recipients"]);
        let mut local=BTreeMap::<BlendGroup,Vec<BlendSample>>::new();
        let mut used=BTreeSet::new();let mut eligible=0usize;
        for obs in rows(&compared["observations"]).filter(|o|o["task"]=="recipient_ownership"){
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",_=>continue};
            let (Some(old),Some(actual),Some(s))=(singleton(&obs["initialCandidates"]),
                singleton(&obs["finalCandidates"]),position(&units,&obs["source"]))else{continue;};
            if !matches!(old,"next"|"previous")||!matches!(actual,"next"|"previous")||
                (label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||
                    singleton(&obs["negativeCandidates"])!=Some(old)))||(label=="unchanged_proxy"&&old!=actual){continue;}
            if !used.insert(s){return Err("결합 관측 중복".into());}
            eligible+=1;
            let candidate=(||->Option<(BlendGroup,BlendSample)>{
                let mut traces=rows(&first["recipientOwnerTrace"]["decisions"]).filter(|d|position(&units,&d["source"])==Some(s));
                let d=traces.next()?;if traces.next().is_some(){return None;}
                let (kind,route,p)=if d["legacyFirstBlockingReason"].is_null(){
                    let (kind,bin)=owner_score_input(&units,d,obs)?;
                    let mut found=rows(&legacy["bins"]).filter(|r|r["sourceKind"]==kind&&r["bin"].as_u64()==Some(bin as u64));
                    let r=found.next()?;if found.next().is_some(){return None;}
                    (kind,"score",r["estimatedNextProxyFrequency"].as_f64()?)
                }else{
                    let (kind,reason)=owner_gate_input(&units,d,obs)?;
                    let mut found=rows(&legacy["gateCalibration"]["groups"]).filter(|r|r["sourceKind"]==kind&&r["firstBlockingReason"]==reason);
                    let r=found.next()?;if found.next().is_some(){return None;}
                    (kind,"gate",r["estimatedNextProxyFrequency"].as_f64()?)
                };
                if !p.is_finite()||!(0.0..=1.0).contains(&p){return None;}
                let mut selections=rows(&shadow["contextSelection"]).filter(|r|r["task"]=="recipient_ownership"&&position(&units,r)==Some(s));
                let sel=selections.next()?;if selections.next().is_some()||
                    sel["status"]!="diagnostic_context_selected"||sel["contextConflict"]!=false{return None;}
                let key=sel["selectedContextKey"].as_str()?;
                let mut contexts=rows(&obs["contexts"]).filter(|c|c["key"].as_str()==Some(key));
                let context=contexts.next()?;if contexts.next().is_some(){return None;}
                let level=context["level"].as_str()?;
                let mut locs=rows(&shadow["locations"]).filter(|r|r["task"]=="recipient_ownership"&&position(&units,r)==Some(s));
                let loc=locs.next()?;if locs.next().is_some()||loc["candidateCoverage"]!="traced_pair"||
                    !rows(&loc["contextKeys"]).any(|k|k.as_str()==Some(key)){return None;}
                let allowed=BTreeSet::from(["next","previous"]);
                if rows(&loc["candidates"]).filter_map(Value::as_str).collect::<BTreeSet<_>>()!=allowed||
                    rows(&loc["candidates"]).count()!=2{return None;}
                let mut reports=rows(&shadow["contexts"]).filter(|r|r["task"]=="recipient_ownership"&&
                    r["contextKey"].as_str()==Some(key)&&r["supportVersion"]==4&&r["featureVersion"]==1&&
                    r["contributionVersion"]==2&&r["contextQuality"]["status"]=="diagnostic_screen_pass"&&
                    rows(&r["candidateIds"]).filter_map(Value::as_str).collect::<BTreeSet<_>>()==allowed);
                let r=reports.next()?;if reports.next().is_some(){return None;}
                let sample=shadow_distribution(r,&allowed,actual)?;
                let mut models=rows(&shadow_model["models"]).filter(|r|r["task"]=="recipient_ownership"&&
                    r["contextLevel"]==level&&r["candidateCoverage"]=="traced_pair");
                let model=models.next()?;if models.next().is_some(){return None;}
                let t=model["temperature"].as_f64()?;let smoothing=model["uniformSmoothing"].as_f64()?;
                if !SHADOW_TEMPERATURES.contains(&t)||!SHADOW_SMOOTHING.contains(&smoothing){return None;}
                // BTreeSet order: next, previous, outside.
                Some(((kind.to_owned(),route.to_owned(),level.to_owned()),BlendSample{
                    legacy:vec![p,1.0-p,0.0],shadow:shadow_transform(&sample.distribution,t,smoothing),
                    actual:sample.actual,label,legacy_choice:usize::from(old=="previous"),
                    transfer_block:owner_transfer_indexed(&units,first,&owner_index,d,s,old)}))
            })();
            if let Some((key,sample))=candidate{local.entry(key).or_default().push(sample);}
            else{unavailable+=1;}
        }
        if fit{
            eligible_weight+=eligible;
            if !local.is_empty(){valid_weight_orders+=1;}
            for (key,samples) in local{fits.entry(key).or_default().add_order(&samples);}
        }else{
            eligible_test+=eligible;
            if !local.is_empty(){valid_test_orders+=1;}
            tests.push(local);
        }
    }
    let mut totals=BTreeMap::<(BlendGroup,String),BlendTest>::new();
    let mut screens=BTreeMap::<(BlendGroup,String),ChangeScreenMetrics>::new();
    for order in tests{
        for (key,samples) in order{
            let weight=fits.get(&key).and_then(BlendFit::weight);
            let mut local=BTreeMap::<&str,BlendTest>::new();
            for sample in samples{
                for label in [sample.label,"mixed_proxy"]{
                    local.entry(label).or_default().observe(Some(&sample),weight);
                    screens.entry((key.clone(),label.to_owned())).or_default().observe(&sample,weight);
                }
            }
            for (label,metrics) in local{totals.entry((key.clone(),label.to_owned())).or_default().merge(metrics);}
        }
    }
    let covered=totals.iter().filter(|((_,label),_)|label=="mixed_proxy").map(|(_,m)|m.covered).sum::<usize>();
    let models=fits.into_iter().map(|((kind,route,level),v)|json!({"sourceKind":kind,"legacyRoute":route,
        "contextLevel":level,"weightFitOrders":v.orders,"learningWeight":v.weight(),
        "legacyWeight":v.weight().map(|w|1.0-w)})).collect::<Vec<_>>();
    let groups=totals.into_iter().map(|(((kind,route,level),label),v)|json!({
        "sourceKind":kind,"legacyRoute":route,"contextLevel":level,"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>();
    // Build independent sections before constructing the report to keep
    // serde_json's macro expansion within the default Rust recursion limit.
    let screen_groups=screens.into_iter().map(|(((kind,route,level),label),v)|json!({
        "sourceKind":kind,"legacyRoute":route,"contextLevel":level,"labelKind":label,"metrics":v.wire()
    })).collect::<Vec<_>>();
    let runtime=json!({"schemaVersion":1,"clientBuildVersion":build,
        "scorePolicyVersion":OWNER_SCORE_POLICY,"captureVersion":4,"supportVersion":4,
        "featureVersion":1,"contributionVersion":2,"explicitToWeakRatio":3.0,
        "safetyPolicyVersion":1,"availableAfter":config["evaluationAsOf"],
        "legacyBins":legacy["bins"],"legacyGates":legacy["gateCalibration"]["groups"],
        "shadowModels":shadow_model["models"],"blendModels":models,
        "automaticChangeAuthorized":false});
    let safety=json!({"policyVersion":1,"status":"diagnostic_only",
        "minimumCombinedProxySupport":MIN_CHANGE_PROXY,"minimumCombinedMargin":MIN_CHANGE_MARGIN,
        "maximumOutsideProxySupport":MAX_CHANGE_OUTSIDE,"requiresShadowAgreement":true,
        "scope":"initial_recipient_ownership_only","runtimeManualLockVerified":false,
        "runtimeDestinationValidationVerified":false,"independentApprovalVerified":false,
        "automaticChangeAuthorized":false,"groups":screen_groups});
    let mut report=json!({"status":"blend_weight_diagnostic_only","method":"order_macro_brier_grid_v1",
        "task":"recipient_ownership","clientBuildVersion":build,"fitStart":config["fitStart"],
        "weightStart":config["weightStart"],"testStart":config["testStart"],"testEnd":config["testEnd"],
        "evaluationAsOf":config["evaluationAsOf"],"minimumWeightFitOrders":MIN_BLEND_ORDERS,
        "calibrationInputOrders":fit_payload["orders"].as_array().map_or(0,Vec::len),
        "weightOrdersWithBothModels":valid_weight_orders,"testOrdersWithBothModels":valid_test_orders,
        "eligibleWeightObservations":eligible_weight,"eligibleTestObservations":eligible_test,
        "unavailableBothModelsObservations":unavailable,"excludedOrders":excluded,"lateLabelOrders":late,
        "overallTestCoverage":(eligible_test>0).then(||covered as f64/eligible_test as f64)});
    report["models"]=json!(models);
    report["testGroups"]=json!(groups);
    report["outsideBucketSelectable"]=json!(false);
    report["runtimeParameters"]=runtime;
    report["changeSafety"]=safety;
    report["representativeLabelsVerified"]=json!(false);
    report["historicalModelVerified"]=json!(false);
    report["sourceProvenanceVerified"]=json!(false);
    report["calibratedProbability"]=Value::Null;
    report["eligibleForAutomaticDecision"]=json!(false);
    report["decisionChanged"]=json!(false);
    Ok(report)
}

// R01-R04: independent multiclass field models. The old engine did not
// capture comparable numeric scores for these tasks. Fit a categorical
// reliability proxy (selected / other traced / outside) instead of inventing
// legacy scores. All fitting and evaluation remains diagnostic.
type FieldGroup=(String,String,String);
struct FieldBlendRecord{
    shadow:ShadowCalibrationSample,
    old:usize,
    candidate_ids:Vec<String>,
}
#[derive(Default)]
struct FieldCalibration{
    orders:usize,
    mass:[f64;3],
    shadow:ShadowFit,
}
impl FieldCalibration{
    fn add_order(&mut self,records:&[FieldBlendRecord]){
        if records.is_empty(){return;}
        self.orders+=1;
        let n=records.len() as f64;
        let mut samples=Vec::with_capacity(records.len());
        for r in records{
            let s=&r.shadow;
            let bucket=if s.actual==r.old{0}else if s.actual+1==s.distribution.len(){2}else{1};
            self.mass[bucket]+=1.0/n;
            samples.push(ShadowCalibrationSample{distribution:s.distribution.clone(),actual:s.actual,label:s.label});
        }
        self.shadow.add_order(&samples);
    }
    fn parameters(&self)->Option<([f64;3],(f64,f64))>{
        if self.orders<MIN_CALIBRATION_ORDERS{return None;}
        Some((self.mass.map(|x|x/self.orders as f64),self.shadow.parameters()?))
    }
    fn blend_sample(&self,r:&FieldBlendRecord)->Option<BlendSample>{
        let (p,(t,s))=self.parameters()?;
        field_sample_with_parameters(r,p,t,s)
    }
}
fn field_sample_with_parameters(r:&FieldBlendRecord,p:[f64;3],t:f64,s:f64)->Option<BlendSample>{
    let n=r.shadow.distribution.len().checked_sub(1)?;
    if n<2||r.old>=n||p.iter().any(|v|!v.is_finite()||!(0.0..=1.0).contains(v))||
        (p.iter().sum::<f64>()-1.0).abs()>1e-8||
        !SHADOW_TEMPERATURES.contains(&t)||!SHADOW_SMOOTHING.contains(&s){return None;}
    let mut legacy=vec![p[1]/(n-1) as f64;n+1];
    legacy[r.old]=p[0];legacy[n]=p[2];
    Some(BlendSample{legacy,shadow:shadow_transform(&r.shadow.distribution,t,s),
        actual:r.shadow.actual,label:r.shadow.label,legacy_choice:r.old,transfer_block:None})
}
fn field_blend_top(sample:&BlendSample,w:f64)->Option<usize>{
    if !(0.0..=1.0).contains(&w){return None;}
    let p=blend_distribution(&sample.legacy,&sample.shadow,w);
    let outside=*p.last()?;
    let mut ranked=p[..p.len()-1].iter().copied().enumerate().collect::<Vec<_>>();
    ranked.sort_by(|a,b|b.1.total_cmp(&a.1));
    let (i,top)=*ranked.first()?;
    let second=ranked.get(1)?.1;
    if top<=outside+1e-10||top<=second+1e-10{return None;}
    // A changed candidate is only a diagnostic proposal. Preserve the
    // owner's conservative screen and require independent shadow agreement.
    if i!=sample.legacy_choice&&(top<MIN_CHANGE_PROXY||outside>MAX_CHANGE_OUTSIDE||
        top-second.max(outside)<MIN_CHANGE_MARGIN||w<=0.0||
        sample.shadow.iter().enumerate().any(|(j,p)|j!=i&&*p+1e-10>=sample.shadow[i])){
        return None;
    }
    Some(i)
}
// Use the exact same recommendation/fallback decision in holdout
// evaluation and live inference. Never score an unsafe change as a win.
fn field_blend_recommendation(task:&str,record:&FieldBlendRecord,sample:&BlendSample,w:f64)->(usize,&'static str){
    let old=record.old;
    let Some(top)=field_blend_top(sample,w)else{return (old,"insufficient_or_conflicting_support");};
    if top==old{return (old,"legacy_agreement");}
    let Some(before)=record.candidate_ids.get(old)else{return (old,"invalid_candidate");};
    let Some(after)=record.candidate_ids.get(top)else{return (old,"invalid_candidate");};
    if task=="quantity"{return (old,"literal_quantity_protected");}
    if task=="type"&&contact(before)&&contact(after){return (old,"contact_slot_not_type_change");}
    (top,"diagnostic_proposal_only")
}
fn field_blend_record(units:&[u16],first:&Value,projection:&Value,shadow:&Value,
    obs:&Value,label:&'static str)->Option<(FieldGroup,FieldBlendRecord)>{
    let task=obs["task"].as_str().filter(|s|matches!(*s,"type"|"product"|"variant"|"quantity"))?;
    let s=position(units,&obs["source"])?;
    let old=singleton(&obs["initialCandidates"])?;
    let actual=singleton(&obs["finalCandidates"])?;
    if field_state(units,&first["recipients"],s,task).as_deref()!=Some(old){return None;}
    let mut entries=rows(&projection["fieldEntries"]).filter(|v|v["task"]==task&&position(units,&v["source"])==Some(s));
    let entry=entries.next()?;if entries.next().is_some(){return None;}
    if task=="type"&&entry["legacyTypeCandidate"]!=old{return None;}
    let mut locations=rows(&shadow["locations"]).filter(|v|v["task"]==task&&position(units,v)==Some(s));
    let loc=locations.next()?;if locations.next().is_some(){return None;}
    // No singleton candidates, untraced alternatives, or candidate sets
    // reconstructed from final labels may enter a fitted field model.
    if loc["candidateCoverage"]!="traced_partial"||entry["candidateCoverage"]!="traced_partial"{return None;}
    let ids=loc["candidates"].as_array()?;
    let allowed=ids.iter().map(Value::as_str).collect::<Option<BTreeSet<_>>>()?;
    let expected=rows(&entry["shadowCandidates"]).filter_map(Value::as_str).collect::<BTreeSet<_>>();
    if allowed.len()<2||allowed.len()!=ids.len()||allowed!=expected||!allowed.contains(old){return None;}
    let mut selections=rows(&shadow["contextSelection"]).filter(|v|v["task"]==task&&position(units,v)==Some(s));
    let selected=selections.next()?;if selections.next().is_some()||selected["status"]!="diagnostic_context_selected"||
        selected["contextConflict"]!=false{return None;}
    let key=selected["selectedContextKey"].as_str()?;
    let mut contexts=rows(&entry["contexts"]).filter(|v|v["key"]==key);
    let ctx=contexts.next()?;if contexts.next().is_some()||!rows(&loc["contextKeys"]).any(|k|k==key){return None;}
    let level=ctx["level"].as_str().filter(|s|matches!(*s,"base"|"structure"|"client_structure"))?;
    let mut reports=rows(&shadow["contexts"]).filter(|v|v["task"]==task&&v["contextKey"]==key&&
        v["supportVersion"]==4&&v["featureVersion"]==1&&v["contributionVersion"]==2&&
        v["contextQuality"]["status"]=="diagnostic_screen_pass"&&v["candidateIds"]==loc["candidates"]);
    let report=reports.next()?;if reports.next().is_some(){return None;}
    let top=shadow_top(report,&allowed)?;
    if Some(top)!=selected["diagnosticCandidate"].as_str(){return None;}
    let mut sample=shadow_distribution(report,&allowed,actual)?;
    sample.label=label;
    let old=allowed.iter().position(|id|*id==old)?;
    Some(((task.to_owned(),level.to_owned(),"traced_partial".into()),FieldBlendRecord{shadow:sample,old,candidate_ids:allowed.into_iter().map(str::to_owned).collect()}))
}
pub(super) fn learn_field_blend_weights(payload:&Value)->Result<Value,String>{
    let config=&payload["config"];
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let fit_start=date(&config["fitStart"]).ok_or("필드 보정 시작 시각 오류")?;
    let weight_start=date(&config["weightStart"]).ok_or("필드 가중치 시작 시각 오류")?;
    let test_start=date(&config["testStart"]).ok_or("필드 평가 시작 시각 오류")?;
    let test_end=date(&config["testEnd"]).ok_or("필드 평가 종료 시각 오류")?;
    let as_of=date(&config["evaluationAsOf"]).ok_or("필드 평가 기준 시각 오류")?;
    if !(fit_start<weight_start&&weight_start<test_start&&test_start<test_end&&test_end<=as_of){
        return Err("필드 보정·가중치·평가 기간 순서 오류".into());
    }
    let build=config["clientBuildVersion"].as_str().filter(|v|!v.is_empty()&&v.len()<=128).ok_or("필드 빌드 버전 필요")?;
    let orders=payload["orders"].as_array().filter(|v|!v.is_empty()&&v.len()<=250).ok_or("필드 평가 주문 한도 오류")?;
    let mut seen=BTreeSet::new();
    let mut calibration=BTreeMap::<FieldGroup,FieldCalibration>::new();
    let mut weight_orders=Vec::new();let mut test_orders=Vec::new();
    let mut eligible=BTreeMap::<(String,usize),u64>::new();
    let mut joined=BTreeMap::<(String,usize),u64>::new();
    let mut excluded=0u64;
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("필드 평가 주문 ID 오류")?;
        if !seen.insert(id){return Err("필드 평가 주문 ID 중복".into());}
        let snap=&order["confirmationSnapshot"];let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let cap=&first["initialPredictionCapture"];let shadow=&cap["learningShadow"];
        let (Some(start),Some(end),Some(predicted),Some(confirmed))=(date(&cap["analysisStartedAt"]),date(&cap["analysisCompletedAt"]),
            date(&shadow["capturedAt"]),date(&order["confirmedAt"]))else{excluded+=1;continue;};
        let (phase,boundary)=if predicted<weight_start{(0,weight_start)}else if predicted<test_start{(1,test_start)}else{(2,as_of)};
        let lower=match phase{0=>fit_start,1=>weight_start,_=>test_start};
        if order["learningActive"]!=true||order["deletedAt"].as_str().is_some_and(|v|!v.is_empty())||
            cap["origin"]!="native_reception_worker"||cap["schemaVersion"]!=1||cap["orderId"]!=id||
            cap["clientBuildVersion"]!=build||shadow["status"]!="captured_shadow_support"||shadow["captureVersion"]!=4||
            shadow["sourceDigestsMatched"]!=true||shadow["explicitToWeakRatio"].as_f64()!=Some(3.0)||shadow["decisionChanged"]!=false||
            shadow["modelId"].as_str().is_none_or(|s|s.len()!=71||!s.starts_with("shadow-")||!s[7..].bytes().all(|b|b.is_ascii_hexdigit()))||
            snap["at"]!=order["confirmedAt"]||!(lower<=start&&start<=end&&end<=predicted&&predicted<confirmed)||
            predicted>=test_end||confirmed>boundary||(phase!=2&&confirmed==boundary){excluded+=1;continue;}
        let Some(events)=order["recognitionAuditEvents"].as_array()else{excluded+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=predicted||t>confirmed)){excluded+=1;continue;}
        let compared=compare(order)?;
        if compared["auditCoverage"]!="verified_manifest"{excluded+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("필드 평가 원문 누락")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let projection=super::learning_context::lookup(&json!({"raw":raw,"initialAnalysis":first}))?;
        let mut local=BTreeMap::<FieldGroup,Vec<FieldBlendRecord>>::new();let mut used=BTreeSet::new();
        let mut fallback=BTreeMap::<(String,&str),ModelPairCounts>::new();
        for obs in rows(&compared["observations"]){
            let Some(task)=obs["task"].as_str().filter(|t|matches!(*t,"type"|"product"|"variant"|"quantity"))else{continue;};
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",_=>continue};
            let (Some(old),Some(actual),Some(s))=(singleton(&obs["initialCandidates"]),singleton(&obs["finalCandidates"]),position(&units,&obs["source"]))else{continue;};
            if (label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||singleton(&obs["negativeCandidates"])!=Some(old)))||
                (label=="unchanged_proxy"&&old!=actual){continue;}
            if !used.insert((s,task)){return Err("필드 평가 관측 중복".into());}
            *eligible.entry((task.to_owned(),phase)).or_default()+=1;
            if let Some((key,record))=field_blend_record(&units,first,&projection,shadow,obs,label){
                *joined.entry((task.to_owned(),phase)).or_default()+=1;
                local.entry(key).or_default().push(record);
            }else if phase==2{
                // An unavailable learning context is not a dropped test
                // observation. The live classifier retains its old answer.
                fallback.entry((task.to_owned(),label)).or_default().observe(old,actual,Some(old),None);
            }
        }
        match phase{0=>for (key,records) in local{calibration.entry(key).or_default().add_order(&records);},
            1=>weight_orders.push(local),_=>test_orders.push((local,fallback))}
    }
    let mut fits=BTreeMap::<FieldGroup,BlendFit>::new();
    for order in weight_orders{
        for (key,records) in order{
            let samples=records.iter().filter_map(|r|calibration.get(&key)?.blend_sample(r)).collect::<Vec<_>>();
            fits.entry(key).or_default().add_order(&samples);
        }
    }
    let mut totals=BTreeMap::<(FieldGroup,String),(BlendTest,PairedEvaluation)>::new();
    let mut task_totals=BTreeMap::<(String,String),ModelPairCounts>::new();
    for (order,mut fallback) in test_orders{
        for (key,records) in order{
            let w=fits.get(&key).and_then(BlendFit::weight);
            let mut local=BTreeMap::<&str,(BlendTest,PairedEvaluation)>::new();
            for record in records{
                let sample=calibration.get(&key).and_then(|c|c.blend_sample(&record));
                let old=record.old.to_string();let actual=record.shadow.actual.to_string();
                let predicted=sample.as_ref().zip(w).map(|(s,w)|field_blend_recommendation(&key.0,&record,s,w).0.to_string());
                fallback.entry((key.0.clone(),record.shadow.label)).or_default()
                    .observe(&old,&actual,Some(&old),predicted.as_deref());
                for label in [record.shadow.label,"mixed_proxy"]{
                    let (loss,agreement)=local.entry(label).or_default();
                    loss.observe(sample.as_ref(),w);
                    agreement.observe(&old,&actual,predicted.as_deref(),record.shadow.actual+1==record.shadow.distribution.len());
                }
            }
            for (label,(loss,agreement)) in local{
                let (a,b)=totals.entry((key.clone(),label.to_owned())).or_default();a.merge(loss);b.merge_order(agreement);
            }
        }
        // Count each order once per task/label, even across context levels.
        for ((task,label),value) in fallback{
            task_totals.entry((task,label.to_owned())).or_default().merge_order(value);
        }
    }
    let task_groups=task_totals.into_iter().map(|((task,label),v)|
        json!({"task":task,"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>();
    let models=calibration.iter().map(|((task,level,coverage),cal)|{
        let fit=fits.get(&(task.clone(),level.clone(),coverage.clone()));
        let params=cal.parameters();
        json!({"task":task,"contextLevel":level,"candidateCoverage":coverage,"calibrationOrders":cal.orders,
            "legacyReliabilityProxy":params.map(|p|p.0),"shadowTemperature":params.map(|p|p.1.0),
            "shadowUniformSmoothing":params.map(|p|p.1.1),"weightFitOrders":fit.map_or(0,|f|f.orders),
            "learningWeight":fit.and_then(BlendFit::weight),"automaticChangeAuthorized":false})
    }).collect::<Vec<_>>();
    let groups=totals.into_iter().map(|(((task,level,coverage),label),(loss,agreement))|
        json!({"task":task,"contextLevel":level,"candidateCoverage":coverage,"labelKind":label,
            "loss":loss.wire(),"agreement":agreement.wire()})).collect::<Vec<_>>();
    let coverage=["type","product","variant","quantity"].into_iter().map(|task|{
        let counts=|m:&BTreeMap<(String,usize),u64>|(0..3).map(|phase|m.get(&(task.to_owned(),phase)).copied().unwrap_or(0)).collect::<Vec<_>>();
        json!({"task":task,"eligibleByPhase":counts(&eligible),"joinedByPhase":counts(&joined)})
    }).collect::<Vec<_>>();
    Ok(json!({"status":"field_blend_diagnostic_only","schemaVersion":1,
        "method":"categorical_legacy_reliability_order_macro_brier_v1","phaseOrder":["calibration","weight","test"],
        "legacyProxyBuckets":["selected","other_traced","outside"],"models":models,"testGroups":groups,
        "coverageByTask":coverage,"testTaskGroups":task_groups,"excludedOrders":excluded,"clientBuildVersion":build,
        "representativeLabelsVerified":false,"sourceProvenanceVerified":false,
        "automaticChangeAuthorized":false,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// R01-R04 frozen inference. The dummy actual index is never consulted
// by candidate ranking, and no confirmed label is accepted by this path.
pub(super) fn run_field_blend_shadow(payload:&Value)->Result<Value,String>{
    let raw=payload["raw"].as_str().filter(|s|!s.is_empty()&&s.len()<=128*1024).ok_or("필드 예측 원문 오류")?;
    let first=&payload["initialAnalysis"];let shadow=&payload["learningShadow"];
    let models=payload["models"].as_array().filter(|v|v.len()<=4).ok_or("필드 모델 목록 오류")?;
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let now=date(&payload["predictionAt"]).ok_or("필드 예측 시각 오류")?;
    if shadow["status"]!="captured_shadow_support"||shadow["captureVersion"]!=4||
        shadow["sourceDigestsMatched"]!=true||shadow["decisionChanged"]!=false{
        return Err("필드 예측 원본 지지도 검증 필요".into());
    }
    let units=raw.encode_utf16().collect::<Vec<_>>();
    let projection=super::learning_context::lookup(&json!({"raw":raw,"initialAnalysis":first}))?;
    let mut tasks=BTreeSet::new();
    for m in models{
        let task=m["model"]["task"].as_str().filter(|v|matches!(*v,"type"|"product"|"variant"|"quantity")).ok_or("필드 모델 작업 오류")?;
        if !tasks.insert(task){return Err("필드 모델 작업 중복".into());}
        let model=&m["model"];
        if model["schemaVersion"]!=1||model["clientBuildVersion"]!=payload["clientBuildVersion"]||
            model["automaticChangeAuthorized"]!=false||model["selectionAuthorized"]!=false||
            date(&model["evaluationAsOf"]).is_none_or(|v|v>=now)||
            date(&m["createdAt"]).is_none_or(|v|v>=now)||
            m["modelId"].as_str().is_none_or(|v|v.len()!=76||!v.starts_with("field-blend-")||!v[12..].bytes().all(|b|b.is_ascii_hexdigit())){
            return Err("필드 모델 시각 또는 규격 오류".into());
        }
    }
    let mut decisions=Vec::new();
    for entry in rows(&projection["fieldEntries"]){
        let task=entry["task"].as_str().ok_or("필드 작업 오류")?;
        let Some(s)=position(&units,&entry["source"])else{continue;};
        let Some(m)=models.iter().find(|m|m["model"]["task"]==task)else{continue;};
        let old=field_state(&units,&first["recipients"],s,task);
        let result=(||->Option<Value>{
            let old=old.as_deref()?;
            let obs=json!({"task":task,"source":entry["source"],"initialCandidates":[old],"finalCandidates":[old]});
            let (group,record)=field_blend_record(&units,first,&projection,shadow,&obs,"runtime_unlabeled")?;
            let mut rows=rows(&m["model"]["models"]).filter(|v|v["task"]==task&&v["contextLevel"]==group.1&&v["candidateCoverage"]==group.2);
            let params=rows.next()?;if rows.next().is_some()||params["automaticChangeAuthorized"]!=false||
                params["calibrationOrders"].as_u64()?<MIN_CALIBRATION_ORDERS as u64||
                params["weightFitOrders"].as_u64()?<MIN_BLEND_ORDERS as u64{return None;}
            let p=params["legacyReliabilityProxy"].as_array()?;
            if p.len()!=3{return None;}
            let sample=field_sample_with_parameters(&record,[p[0].as_f64()?,p[1].as_f64()?,p[2].as_f64()?],
                params["shadowTemperature"].as_f64()?,params["shadowUniformSmoothing"].as_f64()?)?;
            let w=params["learningWeight"].as_f64().filter(|w|(0.0..=1.0).contains(w)&&(w*10.0-(w*10.0).round()).abs()<1e-8)?;
            let ids=rows_ids(entry)?;
            let (chosen,reason)=field_blend_recommendation(task,&record,&sample,w);
            let proposed=ids.get(chosen).copied().filter(|id|*id!=old);
            Some(json!({"task":task,"source":entry["source"],"modelId":m["modelId"],"contextLevel":group.1,
                "legacyCandidate":old,"candidateIds":ids,"learningWeight":w,
                "legacyProxy":sample.legacy,"learningProxy":sample.shadow,
                "combinedProxy":blend_distribution(&sample.legacy,&sample.shadow,w),
                "diagnosticCandidate":proposed,"status":if proposed.is_some(){"diagnostic_proposal_only"}else{"retain_legacy"},
                "decisionReason":reason,
                "jointValidationRequired":true,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
        })();
        decisions.push(result.unwrap_or_else(||json!({"task":task,"source":entry["source"],"modelId":m["modelId"],
            "legacyCandidate":old,"status":"unavailable_or_ineligible","eligibleForAutomaticDecision":false,"decisionChanged":false})));
    }
    Ok(json!({"schemaVersion":1,"status":"field_runtime_shadow_only","predictionAt":payload["predictionAt"],
        "decisions":decisions,"automaticChangeAuthorized":false,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}
// Compare immutable model revisions on the same later first-analysis
// snapshots. No re-fitting, no caller-supplied labels, no operational edits.
pub(super) fn compare_field_models(payload:&Value)->Result<Value,String>{
    let baseline=&payload["baseline"];let candidate=&payload["candidate"];
    let task=baseline["model"]["task"].as_str().filter(|t|matches!(*t,"type"|"product"|"variant"|"quantity")).ok_or("필드 비교 작업 오류")?;
    if candidate["model"]["task"]!=task||baseline["modelId"]==candidate["modelId"]{return Err("서로 다른 동일 작업 모델 필요".into());}
    let build=payload["clientBuildVersion"].as_str().ok_or("필드 비교 빌드 누락")?;
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let as_of=date(&payload["evaluationAsOf"]).ok_or("필드 비교 기준 시각 오류")?;
    let orders=payload["orders"].as_array().filter(|v|!v.is_empty()&&v.len()<=250).ok_or("필드 비교 주문 한도 오류")?;
    let mut seen=BTreeSet::new();let mut excluded=0usize;
    let mut totals=BTreeMap::<&str,ModelPairCounts>::new();
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("필드 비교 주문 ID 오류")?;
        if !seen.insert(id){return Err("필드 비교 주문 중복".into());}
        if [baseline,candidate].iter().any(|m|m["model"]["sourceDigests"].get(id).is_some()){
            excluded+=1;continue;
        }
        let snap=&order["confirmationSnapshot"];let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let cap=&first["initialPredictionCapture"];let shadow=&cap["learningShadow"];
        let (Some(start),Some(end),Some(predicted),Some(confirmed))=(date(&cap["analysisStartedAt"]),date(&cap["analysisCompletedAt"]),
            date(&shadow["capturedAt"]),date(&order["confirmedAt"]))else{excluded+=1;continue;};
        if order["learningActive"]!=true||order["deletedAt"].as_str().is_some_and(|v|!v.is_empty())||
            cap["origin"]!="native_reception_worker"||cap["schemaVersion"]!=1||cap["orderId"]!=id||
            cap["clientBuildVersion"]!=build||shadow["status"]!="captured_shadow_support"||shadow["captureVersion"]!=4||
            shadow["sourceDigestsMatched"]!=true||shadow["decisionChanged"]!=false||snap["at"]!=order["confirmedAt"]||
            !(start<=end&&end<=predicted&&predicted<confirmed&&confirmed<=as_of)||
            [baseline,candidate].iter().any(|m|date(&m["createdAt"]).is_none_or(|t|t>=start)||
                date(&m["model"]["evaluationAsOf"]).is_none_or(|t|t>=start)){
            excluded+=1;continue;
        }
        let Some(events)=order["recognitionAuditEvents"].as_array()else{excluded+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=predicted||t>confirmed)){excluded+=1;continue;}
        let compared=compare(order)?;
        if compared["auditCoverage"]!="verified_manifest"{excluded+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("필드 비교 원문 오류")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let run=|m:&Value|run_field_blend_shadow(&json!({"raw":raw,"initialAnalysis":first,"learningShadow":shadow,
            "models":[m],"predictionAt":shadow["capturedAt"],"clientBuildVersion":build}));
        let a=run(baseline)?;let b=run(candidate)?;
        let mut local=BTreeMap::<&str,ModelPairCounts>::new();let mut used=BTreeSet::new();
        for obs in rows(&compared["observations"]).filter(|o|o["task"]==task){
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",_=>continue};
            let (Some(old),Some(actual),Some(s))=(singleton(&obs["initialCandidates"]),singleton(&obs["finalCandidates"]),position(&units,&obs["source"]))else{continue;};
            if (label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||singleton(&obs["negativeCandidates"])!=Some(old)))||
                (label=="unchanged_proxy"&&old!=actual){continue;}
            if !used.insert(s){return Err("필드 비교 관측 중복".into());}
            fn prediction<'a>(v:&'a Value,units:&[u16],s:Span,old:&'a str)->Option<&'a str>{
                let mut matches=rows(&v["decisions"]).filter(|d|position(units,&d["source"])==Some(s)&&d["legacyCandidate"]==old);
                let d=matches.next()?;if matches.next().is_some(){return None;}
                match d["status"].as_str()?{
                    "diagnostic_proposal_only"=>d["diagnosticCandidate"].as_str(),
                    "retain_legacy"=>Some(old),_=>None
                }
            }
            for kind in [label,"mixed_proxy"]{
                local.entry(kind).or_default().observe(old,actual,prediction(&a,&units,s,old),prediction(&b,&units,s,old));
            }
        }
        for (label,value) in local{totals.entry(label).or_default().merge_order(value);}
    }
    let groups=totals.into_iter().map(|(label,v)|json!({"task":task,"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>();
    Ok(json!({"status":"field_paired_model_diagnostic_only","task":task,"baselineModelId":baseline["modelId"],
        "candidateModelId":candidate["modelId"],"sameFrozenInputs":true,"evaluationAsOf":payload["evaluationAsOf"],
        "uniqueOrders":seen.len(),"excludedOrders":excluded,"groups":groups,"winner":null,
        "representativeLabelsVerified":false,"automaticChangeAuthorized":false,"decisionChanged":false}))
}
fn rows_ids(entry:&Value)->Option<Vec<&str>>{
    let ids=entry["shadowCandidates"].as_array()?;
    let set=ids.iter().map(Value::as_str).collect::<Option<BTreeSet<_>>>()?;
    (set.len()==ids.len()).then(||set.into_iter().collect())
}

// P07-05: read-only runtime inference on initial recognition. The optional
// developer model is never a production authorization or an audit label.
pub(super) fn run_blend_shadow(payload:&Value)->Result<Value,String>{
    let raw=payload["raw"].as_str().filter(|s|!s.is_empty()&&s.len()<=128*1024).ok_or("그림자 원문 오류")?;
    let first=&payload["initialAnalysis"];let shadow=&payload["learningShadow"];
    let units=raw.encode_utf16().collect::<Vec<_>>();
    let owner_index=OwnerIndex::new(&units,&first["recipients"]);
    let observations=owner_observations_indexed(&units,first,&owner_index,&owner_index);
    run_blend_prepared(first,shadow,&payload["model"],&payload["predictionAt"],&payload["clientBuildVersion"],
        &units,&owner_index,&observations)
}
fn run_blend_prepared(first:&Value,shadow:&Value,model:&Value,prediction_at:&Value,build:&Value,
    units:&[u16],owner_index:&OwnerIndex<'_>,observations:&[Value])->Result<Value,String>{
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let now=date(prediction_at).ok_or("그림자 예측 시각 오류")?;
    let available=date(&model["availableAfter"]).ok_or("그림자 모델 시각 오류")?;
    if available>=now||model["schemaVersion"]!=1||&model["clientBuildVersion"]!=build||
        model["scorePolicyVersion"]!=OWNER_SCORE_POLICY||model["captureVersion"]!=4||model["supportVersion"]!=4||
        model["featureVersion"]!=1||model["contributionVersion"]!=2||model["safetyPolicyVersion"]!=1||
        model["explicitToWeakRatio"].as_f64()!=Some(3.0)||model["automaticChangeAuthorized"]!=false||
        shadow["captureVersion"]!=4||shadow["status"]!="captured_shadow_support"||
        shadow["sourceDigestsMatched"]!=true||shadow["decisionChanged"]!=false||
        first["recipientOwnerTrace"]["schemaVersion"]!=1||
        first["recipientOwnerTrace"]["scorePolicyVersion"]!=OWNER_SCORE_POLICY{
        return Err("그림자 모델 버전 또는 시각 불일치".into());
    }
    if observations.len()>64{return Err("결합 그림자 관측 한도 초과".into());}
    let mut decisions=Vec::new();let mut seen=BTreeSet::new();
    for obs in observations{
        let Some(s)=position(&units,&obs["source"])else{continue;};
        if !seen.insert(s){return Err("결합 그림자 관측 중복".into());}
        let outcome=(||->Option<Value>{
            let old=singleton(&obs["initialCandidates"])?;
            if !matches!(old,"next"|"previous"){return None;}
            let mut traces=rows(&first["recipientOwnerTrace"]["decisions"]).filter(|d|position(&units,&d["source"])==Some(s));
            let d=traces.next()?;if traces.next().is_some(){return None;}
            let (kind,route,p)=if d["legacyFirstBlockingReason"].is_null(){
                let (kind,bin)=owner_score_input(&units,d,&obs)?;
                let mut found=rows(&model["legacyBins"]).filter(|r|r["sourceKind"]==kind&&r["bin"].as_u64()==Some(bin as u64));
                let r=found.next()?;if found.next().is_some()||r["fitOrders"].as_u64()?<MIN_CALIBRATION_ORDERS as u64{return None;}
                (kind,"score",r["estimatedNextProxyFrequency"].as_f64()?)
            }else{
                let (kind,reason)=owner_gate_input(&units,d,&obs)?;
                let mut found=rows(&model["legacyGates"]).filter(|r|r["sourceKind"]==kind&&r["firstBlockingReason"]==reason);
                let r=found.next()?;if found.next().is_some()||r["fit"]["independentOrders"].as_u64()?<MIN_CALIBRATION_ORDERS as u64{return None;}
                (kind,"gate",r["estimatedNextProxyFrequency"].as_f64()?)
            };
            if !p.is_finite()||!(0.0..=1.0).contains(&p){return None;}
            let mut selections=rows(&shadow["contextSelection"]).filter(|r|r["task"]=="recipient_ownership"&&position(&units,r)==Some(s));
            let sel=selections.next()?;if selections.next().is_some()||sel["status"]!="diagnostic_context_selected"||sel["contextConflict"]!=false{return None;}
            let key=sel["selectedContextKey"].as_str()?;
            let mut contexts=rows(&obs["contexts"]).filter(|c|c["key"].as_str()==Some(key));
            let context=contexts.next()?;if contexts.next().is_some(){return None;}
            let level=context["level"].as_str()?;
            let mut locs=rows(&shadow["locations"]).filter(|r|r["task"]=="recipient_ownership"&&position(&units,r)==Some(s));
            let loc=locs.next()?;if locs.next().is_some()||loc["candidateCoverage"]!="traced_pair"||
                !rows(&loc["contextKeys"]).any(|k|k.as_str()==Some(key)){return None;}
            let allowed=BTreeSet::from(["next","previous"]);
            if rows(&loc["candidates"]).filter_map(Value::as_str).collect::<BTreeSet<_>>()!=allowed||
                rows(&loc["candidates"]).count()!=2{return None;}
            let mut reports=rows(&shadow["contexts"]).filter(|r|r["task"]=="recipient_ownership"&&
                r["contextKey"].as_str()==Some(key)&&r["supportVersion"]==4&&r["featureVersion"]==1&&
                r["contributionVersion"]==2&&r["contextQuality"]["status"]=="diagnostic_screen_pass"&&
                rows(&r["candidateIds"]).filter_map(Value::as_str).collect::<BTreeSet<_>>()==allowed);
            let r=reports.next()?;if reports.next().is_some(){return None;}
            // The dummy class is not used in inference or screening.
            let sample=shadow_distribution(r,&allowed,old)?;
            let mut models=rows(&model["shadowModels"]).filter(|r|r["task"]=="recipient_ownership"&&r["contextLevel"]==level&&r["candidateCoverage"]=="traced_pair");
            let sm=models.next()?;if models.next().is_some()||sm["fitOrders"].as_u64()?<MIN_CALIBRATION_ORDERS as u64{return None;}
            let t=sm["temperature"].as_f64()?;let smoothing=sm["uniformSmoothing"].as_f64()?;
            if !SHADOW_TEMPERATURES.contains(&t)||!SHADOW_SMOOTHING.contains(&smoothing){return None;}
            let mut weights=rows(&model["blendModels"]).filter(|r|r["sourceKind"]==kind&&r["legacyRoute"]==route&&r["contextLevel"]==level);
            let wm=weights.next()?;if weights.next().is_some()||wm["weightFitOrders"].as_u64()?<MIN_BLEND_ORDERS as u64{return None;}
            let w=wm["learningWeight"].as_f64()?;
            if !w.is_finite()||!(0.0..=1.0).contains(&w)||(w*10.0-(w*10.0).round()).abs()>1e-8{return None;}
            let blend=BlendSample{legacy:vec![p,1.0-p,0.0],shadow:shadow_transform(&sample.distribution,t,smoothing),
                actual:sample.actual,label:"runtime_unlabeled",legacy_choice:usize::from(old=="previous"),
                transfer_block:owner_transfer_indexed(&units,first,&owner_index,d,s,old)};
            let (reason,target)=screen_blend_change(&blend,Some(w));
            Some(json!({"start":s.0,"end":s.1,"legacyCandidate":old,"contextLevel":level,
                "sourceKind":kind,"legacyRoute":route,"learningWeight":w,
                "candidateOrder":["next","previous","outside"],"legacyProxy":blend.legacy,
                "learningProxy":blend.shadow,"combinedProxy":blend_distribution(&blend.legacy,&blend.shadow,w),
                "status":reason,"diagnosticCandidate":target.map(|i|if i==0{"next"}else{"previous"}),
                "decisionChanged":false,"eligibleForAutomaticDecision":false}))
        })();
        decisions.push(outcome.unwrap_or_else(||json!({"start":s.0,"end":s.1,
            "status":"unavailable_or_ineligible","decisionChanged":false,"eligibleForAutomaticDecision":false})));
    }
    Ok(json!({"status":"runtime_shadow_only","schemaVersion":1,"predictionAt":prediction_at,
        "decisions":decisions,"modelIndependentlyApproved":false,"representativeLabelsVerified":false,
        "automaticChangeAuthorized":false,"decisionChanged":false,"eligibleForAutomaticDecision":false}))
}

// P07-06: compare frozen runtime proposals, never rerun a model with final labels.
pub(super) fn evaluate_combined_confirmation(order:&Value)->Result<Value,String>{
    evaluate_combined_prepared(order,None)
}
pub(super) fn evaluate_combined_prepared(order:&Value,prepared:Option<&Value>)->Result<Value,String>{
    let snap=&order["confirmationSnapshot"];
    let first=&snap["recognitionBaseline"]["initialAnalysis"];
    let cap=&first["initialPredictionCapture"];
    let shadow=&cap["learningShadow"];
    let combined=&shadow["combinedShadow"];
    let empty=|status:&str|json!({"schemaVersion":1,"status":status,"groups":[],
        "eligibleForAutomaticDecision":false,"decisionChanged":false});
    if combined["status"]!="runtime_shadow_only"{return Ok(empty("no_combined_prediction"));}
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let (Some(a),Some(b),Some(c),Some(d),Some(e))=(date(&cap["analysisStartedAt"]),date(&cap["analysisCompletedAt"]),
        date(&shadow["capturedAt"]),date(&combined["predictionAt"]),date(&order["confirmedAt"]))
        else{return Ok(empty("unverified_prediction_time"));};
    if order["learningActive"]!=true||cap["origin"]!="native_reception_worker"||cap["schemaVersion"]!=1||
        cap["orderId"]!=order["id"]||shadow["captureVersion"]!=4||shadow["sourceDigestsMatched"]!=true||
        combined["schemaVersion"]!=1||combined["currentModelSourceDigestsMatched"]!=true||
        combined["automaticChangeAuthorized"]!=false||combined["decisionChanged"]!=false||
        snap["at"]!=order["confirmedAt"]||!(a<=b&&b<=c&&c<=d&&d<e){
        return Ok(empty("unverified_prediction_provenance"));
    }
    let Some(model)=combined["modelId"].as_str().filter(|s|s.len()==70&&s.starts_with("blend-")&&s[6..].bytes().all(|b|b.is_ascii_hexdigit()))
        else{return Ok(empty("unverified_model_identity"));};
    let Some(events)=order["recognitionAuditEvents"].as_array()else{return Ok(empty("unverified_audit"));};
    if events.iter().any(|v|date(&v["at"]).is_none_or(|t|t<=d||t>e)){return Ok(empty("unverified_audit_time"));}
    let owned;
    let compared=match prepared{Some(v)=>v,None=>{owned=compare(order)?;&owned}};
    if compared["auditCoverage"]!="verified_manifest"{return Ok(empty("unverified_audit"));}
    let units=snap["raw"].as_str().ok_or("평가 원문 누락")?.encode_utf16().collect::<Vec<_>>();
    let decisions=combined["decisions"].as_array().filter(|v|v.len()<=64).ok_or("결합 예측 목록 오류")?;
    let mut seen=BTreeSet::new();
    for v in decisions{if !seen.insert(position(&units,v).ok_or("결합 예측 위치 오류")?){return Err("결합 예측 중복".into());}}
    let mut groups=BTreeMap::<(&str,&str,&str,&str),[u64;7]>::new();
    let (mut eligible,mut missing,mut excluded)=(0u64,0u64,0u64);
    for obs in rows(&compared["observations"]).filter(|o|o["task"]=="recipient_ownership"){
        let label=match obs["outcome"].as_str(){
            Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
            Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",
            _=>{excluded+=1;continue;}
        };
        let (Some(old),Some(actual),Some(s))=(singleton(&obs["initialCandidates"]),singleton(&obs["finalCandidates"]),position(&units,&obs["source"]))
            else{excluded+=1;continue;};
        if !matches!(old,"previous"|"next")||!matches!(actual,"previous"|"next"|"outside_traced_candidates")||
            (label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||
                singleton(&obs["negativeCandidates"])!=Some(old)))||(label=="unchanged_proxy"&&old!=actual){
            excluded+=1;continue;
        }
        eligible+=1;
        let Some(v)=decisions.iter().find(|v|position(&units,v)==Some(s))else{missing+=1;continue;};
        let parsed=(||->Option<(&str,&str,&str,Option<&str>)>{
            if v["legacyCandidate"].as_str()!=Some(old)||v["decisionChanged"]!=false||
                v["eligibleForAutomaticDecision"]!=false||v["candidateOrder"]!=json!(["next","previous","outside"]){return None;}
            let kind=v["sourceKind"].as_str().filter(|s|matches!(*s,"address"|"contact"))?;
            let route=v["legacyRoute"].as_str().filter(|s|matches!(*s,"gate"|"score"))?;
            let level=v["contextLevel"].as_str().filter(|s|matches!(*s,"base"|"structure"|"client_structure"))?;
            let vector=|key:&str|->Option<Vec<f64>>{
                let a=v[key].as_array().filter(|a|a.len()==3)?;
                let p=a.iter().map(|v|v.as_f64().filter(|x|x.is_finite()&&(0.0..=1.0).contains(x))).collect::<Option<Vec<_>>>()?;
                ((p.iter().sum::<f64>()-1.0).abs()<1e-8).then_some(p)
            };
            let legacy=vector("legacyProxy")?;let learning=vector("learningProxy")?;let combined=vector("combinedProxy")?;
            let w=v["learningWeight"].as_f64().filter(|w|w.is_finite()&&(0.0..=1.0).contains(w))?;
            if blend_distribution(&legacy,&learning,w).iter().zip(&combined).any(|(a,b)|(a-b).abs()>1e-8){return None;}
            let proposal=v["diagnosticCandidate"].as_str();
            if let Some(p)=proposal{
                let target=if p=="next"{0}else if p=="previous"{1}else{return None;};
                if p==old||v["status"]!="diagnostic_proposal_only"||combined[target]<MIN_CHANGE_PROXY-1e-10||
                    combined[2]>MAX_CHANGE_OUTSIDE+1e-10||combined[target]-combined[1-target].max(combined[2])<MIN_CHANGE_MARGIN-1e-10||
                    learning[target]<=learning[1-target].max(learning[2])+1e-10||w<=0.0{return None;}
            }else if v["status"]=="diagnostic_proposal_only"{return None;}
            Some((kind,route,level,proposal))
        })();
        let Some((kind,route,level,proposal))=parsed else{missing+=1;continue;};
        let c=groups.entry((kind,route,level,label)).or_default();
        let predicted=proposal.unwrap_or(old);
        c[0]+=1;c[1]+=u64::from(old==actual);c[2]+=u64::from(predicted==actual);
        c[3]+=u64::from(proposal.is_some());c[4]+=u64::from(old!=actual&&predicted==actual);
        c[5]+=u64::from(old==actual&&predicted!=actual);c[6]+=u64::from(actual=="outside_traced_candidates");
    }
    let groups=groups.into_iter().map(|((kind,route,level,label),c)|json!({
        "sourceKind":kind,"legacyRoute":route,"contextLevel":level,"labelKind":label,"evaluated":c[0],
        "legacyAgrees":c[1],"screenedCombinedAgrees":c[2],"proposals":c[3],"improved":c[4],"worsened":c[5],"outsideLabels":c[6]})).collect::<Vec<_>>();
    Ok(json!({"schemaVersion":1,"status":"compared_diagnostic_only","modelId":model,"groups":groups,
        "eligibleObservations":eligible,"missingOrIneligiblePredictions":missing,"excludedLabels":excluded,
        "representativeLabelsVerified":false,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// Same frozen first-analysis input, same verified operational label, both
// model revisions. Retrospective replay is not a prospective A/B test.
#[derive(Default)]
struct ModelPairCounts{
    eligible:u64,baseline_covered:u64,candidate_covered:u64,paired:u64,
    both_agree:u64,baseline_only:u64,candidate_only:u64,both_disagree:u64,
    fallback_baseline_agrees:u64,fallback_candidate_agrees:u64,
    orders:u64,paired_orders:u64,macro_baseline:f64,macro_candidate:f64,
    order_deltas:Vec<f64>,paired_order_deltas:Vec<f64>,
}
// Each observation is a whole-order mean in [-1,1]. Hoeffding does
// not assume normally distributed errors or independent lines in an order.
fn order_delta_interval(values:&[f64],alpha:f64)->Value{
    if values.is_empty()||!(0.0..1.0).contains(&alpha){
        return json!({"orders":values.len(),"mean":null,"lower":null,"upper":null});
    }
    let n=values.len() as f64;
    let mean=values.iter().sum::<f64>()/n;
    let radius=(2.0*(2.0/alpha).ln()/n).sqrt();
    json!({"orders":values.len(),"mean":mean,"lower":(mean-radius).max(-1.0),
        "upper":(mean+radius).min(1.0),"confidenceLevel":1.0-alpha,
        "method":"bounded_order_mean_hoeffding_two_sided",
        "assumes_independent_orders":true})
}
// Exact binomial tail evaluated in log space, including n=250.
fn binomial_tail(n:usize,k:usize,p:f64,upper:bool)->f64{
    if p<=0.0{return if if upper{0>=k}else{0<=k}{1.0}else{0.0};}
    if p>=1.0{return if if upper{n>=k}else{n<=k}{1.0}else{0.0};}
    let mut log_choose=0.0;let mut sum=0.0;
    let log_p=p.ln();let log_q=(-p).ln_1p();
    for i in 0..=n{
        if (upper&&i>=k)||(!upper&&i<=k){
            sum+=(log_choose+i as f64*log_p+(n-i) as f64*log_q).exp();
        }
        if i<n{log_choose+=((n-i) as f64).ln()-((i+1) as f64).ln();}
    }
    sum.clamp(0.0,1.0)
}
// One-sided exact upper confidence bound for the probability that an
// entire order's mean agreement deteriorates, not a field-level error rate.
fn order_harm_upper(n:usize,harm:usize,alpha:f64)->Option<f64>{
    if n==0||harm>n||!(0.0..1.0).contains(&alpha){return None;}
    if harm==n{return Some(1.0);}
    let (mut lo,mut hi)=(0.0,1.0);
    for _ in 0..60{
        let mid=(lo+hi)/2.0;
        if binomial_tail(n,harm,mid,false)>alpha{lo=mid;}else{hi=mid;}
    }
    Some(hi)
}
fn order_inference(values:&[f64],alpha:f64)->Value{
    let wins=values.iter().filter(|v|**v>1e-10).count();
    let losses=values.iter().filter(|v|**v< -1e-10).count();
    let discordant=wins+losses;
    json!({"orderMeanInterval":order_delta_interval(values,alpha),
        "improvedOrders":wins,"worsenedOrders":losses,"tiedOrders":values.len()-discordant,
        "oneSidedImprovementSignP":(discordant>0).then(||binomial_tail(discordant,wins,0.5,true)),
        "oneSidedDeteriorationSignP":(discordant>0).then(||binomial_tail(discordant,losses,0.5,true)),
        "orderHarmUpper":order_harm_upper(values.len(),losses,alpha),
        "alpha":alpha,"representativeLabelsVerified":false})
}
impl ModelPairCounts{
    fn observe(&mut self,old:&str,actual:&str,a:Option<&str>,b:Option<&str>){
        self.eligible+=1;
        self.baseline_covered+=u64::from(a.is_some());self.candidate_covered+=u64::from(b.is_some());
        self.fallback_baseline_agrees+=u64::from(a.unwrap_or(old)==actual);
        self.fallback_candidate_agrees+=u64::from(b.unwrap_or(old)==actual);
        if let (Some(a),Some(b))=(a,b){
            self.paired+=1;
            match (a==actual,b==actual){
                (true,true)=>self.both_agree+=1,(true,false)=>self.baseline_only+=1,
                (false,true)=>self.candidate_only+=1,(false,false)=>self.both_disagree+=1,
            }
        }
    }
    fn merge_order(&mut self,o:Self){
        if o.eligible==0{return;}
        self.orders+=1;self.eligible+=o.eligible;self.baseline_covered+=o.baseline_covered;
        self.candidate_covered+=o.candidate_covered;self.paired+=o.paired;
        self.both_agree+=o.both_agree;self.baseline_only+=o.baseline_only;
        self.candidate_only+=o.candidate_only;self.both_disagree+=o.both_disagree;
        self.fallback_baseline_agrees+=o.fallback_baseline_agrees;
        self.fallback_candidate_agrees+=o.fallback_candidate_agrees;
        self.order_deltas.push((o.fallback_candidate_agrees as f64-o.fallback_baseline_agrees as f64)/o.eligible as f64);
        if o.paired>0{
            self.paired_order_deltas.push((o.candidate_only as f64-o.baseline_only as f64)/o.paired as f64);
            self.paired_orders+=1;
            self.macro_baseline+=(o.both_agree+o.baseline_only) as f64/o.paired as f64;
            self.macro_candidate+=(o.both_agree+o.candidate_only) as f64/o.paired as f64;}
    }
    fn wire(&self)->Value{
        let div=|v:f64,n:u64|(n>0).then(||v/n as f64);
        json!({"eligibleObservations":self.eligible,"baselineCovered":self.baseline_covered,
            "candidateCovered":self.candidate_covered,"pairedObservations":self.paired,
            "pairedCoverage":div(self.paired as f64,self.eligible),
            "bothAgree":self.both_agree,"baselineOnlyAgrees":self.baseline_only,
            "candidateOnlyAgrees":self.candidate_only,"bothDisagree":self.both_disagree,
            "baselineAgreement":div((self.both_agree+self.baseline_only) as f64,self.paired),
            "candidateAgreement":div((self.both_agree+self.candidate_only) as f64,self.paired),
            "pairedDelta":div(self.candidate_only as f64-self.baseline_only as f64,self.paired),
            "fallbackBaselineAgreement":div(self.fallback_baseline_agrees as f64,self.eligible),
            "fallbackCandidateAgreement":div(self.fallback_candidate_agrees as f64,self.eligible),
            "independentOrders":self.orders,"pairedOrders":self.paired_orders,
            "orderMacroBaselineAgreement":div(self.macro_baseline,self.paired_orders),
            "orderMacroCandidateAgreement":div(self.macro_candidate,self.paired_orders)})
    }
    fn wire_with_inference(&self,alpha:f64)->Value{
        let mut v=self.wire();
        v["fallbackOrderInference"]=order_inference(&self.order_deltas,alpha);
        v["pairedOrderInference"]=order_inference(&self.paired_order_deltas,alpha);
        v
    }
}
fn paired_decision<'a>(result:&'a Value,units:&[u16],s:Span,old:&'a str)->Option<&'a str>{
    let mut rows=rows(&result["decisions"]).filter(|d|position(units,d)==Some(s));
    let d=rows.next()?;
    if rows.next().is_some()||d["legacyCandidate"].as_str()!=Some(old)||
        d["decisionChanged"]!=false||d["eligibleForAutomaticDecision"]!=false||
        d["candidateOrder"]!=json!(["next","previous","outside"]){return None;}
    if d["status"]=="diagnostic_proposal_only"{
        d["diagnosticCandidate"].as_str().filter(|c|matches!(*c,"next"|"previous")&&*c!=old)
    }else if d["diagnosticCandidate"].is_null()&&d["combinedProxy"].as_array().is_some_and(|v|v.len()==3){
        Some(old)
    }else{None}
}
pub(super) fn compare_registered_models(payload:&Value)->Result<Value,String>{
    let a=&payload["baseline"];let b=&payload["candidate"];
    let date=|v:&Value|v.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok());
    let a_time=date(&a["createdAt"]).ok_or("기존 모델 생성 시각 오류")?;
    let b_time=date(&b["createdAt"]).ok_or("신규 모델 생성 시각 오류")?;
    let am=&a["model"];let bm=&b["model"];
    let mut cutoff=a_time.max(b_time).max(date(&am["availableAfter"]).ok_or("기존 모델 사용 시각 오류")?)
        .max(date(&bm["availableAfter"]).ok_or("신규 모델 사용 시각 오류")?);
    if let Some(v)=payload.get("minimumPredictionAfter"){
        cutoff=cutoff.max(date(v).ok_or("독립 평가 추가 시각 경계 오류")?);
    }
    let as_of=date(&payload["evaluationAsOf"]).ok_or("독립 평가 시각 오류")?;
    let legacy_baseline=payload["compareAgainstLegacy"]==true;
    // Internal bootstrap reuses the candidate's verified model envelope,
    // but the reference prediction is the unchanged original classifier.
    if cutoff>=as_of||(a["modelId"]==b["modelId"])!=legacy_baseline||
        (legacy_baseline&&a!=b)||am["clientBuildVersion"]!=bm["clientBuildVersion"]{
        return Err("독립 평가 모델 또는 시각 불일치".into());
    }
    let alpha=match payload.get("inferenceAlpha"){
        None=>0.05,
        Some(v)=>v.as_f64().filter(|a|a.is_finite()&&*a>0.0&&*a<=0.05).ok_or("독립 평가 유의수준 오류")?,
    };
    let orders=payload["orders"].as_array().filter(|v|!v.is_empty()&&v.len()<=250).ok_or("독립 평가 주문 한도 오류")?;
    let mut seen=BTreeSet::new();
    let mut totals=BTreeMap::<(String,String,String,String),ModelPairCounts>::new();
    let mut statuses=BTreeMap::<&str,u64>::new();
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.is_empty()).ok_or("독립 평가 주문 ID 오류")?;
        if !seen.insert(id){return Err("독립 평가 주문 중복".into());}
        if [am,bm].iter().any(|m|m["sourceDigests"].as_object().is_none_or(|v|v.contains_key(id))){
            *statuses.entry("model_source_overlap").or_default()+=1;continue;
        }
        let snap=&order["confirmationSnapshot"];let first=&snap["recognitionBaseline"]["initialAnalysis"];
        let cap=&first["initialPredictionCapture"];let shadow=&cap["learningShadow"];
        let (Some(start),Some(end),Some(predicted),Some(confirmed))=(date(&cap["analysisStartedAt"]),
            date(&cap["analysisCompletedAt"]),date(&shadow["capturedAt"]),date(&order["confirmedAt"]))
            else{*statuses.entry("missing_time").or_default()+=1;continue;};
        if order["learningActive"]!=true||order["deletedAt"].as_str().is_some_and(|s|!s.is_empty())||
            cap["origin"]!="native_reception_worker"||cap["schemaVersion"]!=1||cap["orderId"]!=order["id"]||
            cap["clientBuildVersion"]!=am["clientBuildVersion"]||snap["at"]!=order["confirmedAt"]||
            !(cutoff<start&&start<=end&&end<=predicted&&predicted<confirmed&&confirmed<=as_of){
            *statuses.entry("outside_independent_window_or_invalid_capture").or_default()+=1;continue;
        }
        let Some(events)=order["recognitionAuditEvents"].as_array()else{*statuses.entry("missing_audit").or_default()+=1;continue;};
        if events.iter().any(|v|date(&v["at"]).is_none_or(|t|t<=predicted||t>confirmed)){
            *statuses.entry("audit_time_invalid").or_default()+=1;continue;
        }
        let compared=compare(order)?;
        if compared["auditCoverage"]!="verified_manifest"{*statuses.entry("unverified_audit").or_default()+=1;continue;}
        let raw=snap["raw"].as_str().ok_or("독립 평가 원문 누락")?;
        let units=raw.encode_utf16().collect::<Vec<_>>();
        // Inference receives no final recipient rows, corrections or labels.
        let owner_index=OwnerIndex::new(&units,&first["recipients"]);
        let initial=owner_observations_indexed(&units,first,&owner_index,&owner_index);
        let replay=|model:&Value|run_blend_prepared(first,shadow,model,&shadow["capturedAt"],
            &cap["clientBuildVersion"],&units,&owner_index,&initial).unwrap_or_else(|_|json!({"decisions":[]}));
        let ar=if legacy_baseline{None}else{Some(replay(am))};let br=replay(bm);
        let mut local=BTreeMap::<(String,String,String,String),ModelPairCounts>::new();
        let mut used=BTreeSet::new();
        for obs in rows(&compared["observations"]).filter(|v|v["task"]=="recipient_ownership"){
            let label=match obs["outcome"].as_str(){
                Some("explicit_correction_observed") if obs["eligibleForCandidateLearning"]==true=>"explicit_correction",
                Some("unchanged_observed") if obs["eligibleForCandidateLearning"]==true=>"unchanged_proxy",
                _=>continue,
            };
            let (Some(old),Some(actual),Some(s))=(singleton(&obs["initialCandidates"]),singleton(&obs["finalCandidates"]),
                position(&units,&obs["source"]))else{continue;};
            if !matches!(old,"next"|"previous")||!matches!(actual,"next"|"previous"|"outside_traced_candidates")||
                (label=="explicit_correction"&&(old==actual||singleton(&obs["positiveCandidates"])!=Some(actual)||
                singleton(&obs["negativeCandidates"])!=Some(old)))||(label=="unchanged_proxy"&&old!=actual){continue;}
            if !used.insert(s){return Err("독립 평가 관측 위치 중복".into());}
            let x=if legacy_baseline{Some(old)}else{ar.as_ref().and_then(|v|paired_decision(v,&units,s,old))};
            let y=paired_decision(&br,&units,s,old);
            let kind=match obs["sourceKind"].as_str(){Some("address")=>"address",Some("phone"|"mobile")=>"contact",_=>"unknown"};
            let mut traces=rows(&first["recipientOwnerTrace"]["decisions"]).filter(|v|position(&units,&v["source"])==Some(s));
            let route=match (traces.next(),traces.next()){
                (Some(d),None)=>if d["legacyFirstBlockingReason"].is_null(){"score"}else{"gate"},_=>"unknown"};
            let mut selections=rows(&shadow["contextSelection"]).filter(|v|v["task"]=="recipient_ownership"&&position(&units,v)==Some(s));
            let level=match (selections.next(),selections.next()){
                (Some(v),None)=>v["selectedContextKey"].as_str().and_then(|s|serde_json::from_str::<Value>(s).ok())
                    .and_then(|v|v[2].as_str().map(str::to_owned)).unwrap_or_else(||"unselected".into()),
                _=>"unselected".into()};
            for key in [("all".to_owned(),"all".to_owned(),"all".to_owned(),label.to_owned()),
                (kind.to_owned(),route.to_owned(),level,label.to_owned())]{
                local.entry(key).or_default().observe(old,actual,x,y);
            }
        }
        *statuses.entry(if used.is_empty(){"no_verified_labels"}else{"evaluated"}).or_default()+=1;
        for (key,value) in local{totals.entry(key).or_default().merge_order(value);}
    }
    let groups=totals.into_iter().map(|((kind,route,level,label),v)|json!({
        "sourceKind":kind,"legacyRoute":route,"contextLevel":level,"labelKind":label,"metrics":v.wire_with_inference(alpha)})).collect::<Vec<_>>();
    Ok(json!({"status":"paired_model_replay_diagnostic_only","inferencePolicyVersion":1,"inferenceAlpha":alpha,
        "inferenceAssumesIndependentOrders":true,"globalMultiplicityControlled":false,"baselineModelId":if legacy_baseline{json!("legacy")}else{a["modelId"].clone()},"candidateModelId":b["modelId"],
        "cutoff":cutoff.to_rfc3339(),"evaluationAsOf":payload["evaluationAsOf"],"requestedOrders":orders.len(),
        "orderStatuses":statuses,"groups":groups,"sameFrozenInputs":true,"prospectiveTrialVerified":false,
        "representativeLabelsVerified":false,"winner":null,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn owner_bootstrap_compares_legacy_without_requiring_manual_model(){
        let model=json!({"modelId":"candidate","createdAt":"2026-01-02T00:00:00Z",
            "model":{"availableAfter":"2026-01-01T00:00:00Z","clientBuildVersion":"test",
                "sourceDigests":{"training":"digest"}}});
        let mut payload=json!({"baseline":model,"candidate":model,"evaluationAsOf":"2026-01-03T00:00:00Z",
            "orders":[{"id":"training"}]});
        assert!(compare_registered_models(&payload).is_err());
        payload["compareAgainstLegacy"]=json!(true);
        let report=compare_registered_models(&payload).unwrap();
        assert_eq!(report["baselineModelId"],"legacy");
        assert_eq!(report["candidateModelId"],"candidate");
        assert_eq!(report["groups"],json!([]));
        assert_eq!(report["decisionChanged"],false);
        payload["baseline"]["model"]["clientBuildVersion"]=json!("other");
        assert!(compare_registered_models(&payload).is_err());
    }
    #[test]
    fn paired_metrics_keep_abstentions_and_order_weights_separate(){
        let mut a=PairedEvaluation::default();
        a.observe("old","new",Some("new"),false);
        a.observe("old","old",None,false);
        let mut b=PairedEvaluation::default();
        for _ in 0..3{b.observe("old","old",Some("new"),false);}
        let mut total=PairedEvaluation::default();
        total.merge_order(a);total.merge_order(b);
        let v=total.wire();
        assert_eq!(v["pairedObservations"],4);
        assert_eq!(v["eligibleObservations"],5);
        assert_eq!(v["coverage"],0.8);
        assert_eq!(v["shadowAgreement"],0.25);
        assert_eq!(v["legacyAgreement"],0.75);
        assert_eq!(v["orderMacroShadowAgreement"],0.5);
        assert_eq!(v["orderMacroLegacyAgreement"],0.5);
        assert_eq!(v["shadowOnlyAgrees"],1);assert_eq!(v["legacyOnlyAgrees"],3);
    }
    #[test]
    fn shadow_top_rejects_tie_outside_mass_and_inconsistent_totals(){
        let allowed=BTreeSet::from(["a","b"]);
        let r=|a:f64,b:f64,o:f64|json!({"totalSupportMass":a+b+o,"outsideCandidateSupportMass":o,
            "candidates":[{"candidate":"a","supportMass":a},{"candidate":"b","supportMass":b}]});
        assert_eq!(shadow_top(&r(2.0,1.0,0.0),&allowed),Some("a"));
        assert_eq!(shadow_top(&r(1.0,1.0,0.0),&allowed),None);
        assert_eq!(shadow_top(&r(2.0,1.0,3.0),&allowed),None);
        let mut bad=r(2.0,1.0,0.0);bad["totalSupportMass"]=json!(9.0);
        assert_eq!(shadow_top(&bad,&allowed),None);
    }
    #[test]
    fn owner_calibration_bins_and_order_weighting_are_bounded(){
        assert_eq!(owner_score_bin(-1),3);assert_eq!(owner_score_bin(0),4);
        assert_eq!(owner_score_bin(-61),0);assert_eq!(owner_score_bin(60),7);
        let mut f=OwnerFitBin::default();f.add_order(100,100);
        assert!(f.estimate().is_none());
        for _ in 0..4{f.add_order(0,1);}
        assert!((f.estimate().unwrap()-2.0/7.0).abs()<1e-10);
        let mut t=OwnerTest::default();t.observe(None,true,false);t.observe(Some(0.75),false,true);
        let mut total=OwnerTest::default();total.merge_order(t);
        assert_eq!(total.wire()["coverage"],0.5);
        assert_eq!(total.wire()["brierOnCovered"],0.0625);
        assert_eq!(total.wire()["hardLegacyBrierOnCovered"],1.0);
    }
    #[test]
    fn owner_calibration_time_split_excludes_late_labels_and_blocked_scores(){
        let make=|id:&str,predicted:&str,confirmed:&str,blocked:bool|{
            let mut p=owner_fixture();
            p["id"]=json!(id);p["learningActive"]=json!(true);p["confirmedAt"]=json!(confirmed);
            p["confirmationSnapshot"]["at"]=json!(confirmed);
            p["recognitionAuditEvents"]=json!([]);
            let base=&mut p["confirmationSnapshot"]["recognitionBaseline"];
            base["auditEventIds"]=json!([]);
            let first=&mut base["initialAnalysis"];
            let phone=first["recipients"][0]["sources"].as_array_mut().unwrap().pop().unwrap();
            first["recipients"][0]["mobile"]=json!("");
            first["recipients"][1]["mobile"]=json!("010-1234-5678");
            first["recipients"][1]["sources"].as_array_mut().unwrap().push(phone);
            first["initialPredictionCapture"]=json!({"schemaVersion":1,"origin":"native_reception_worker",
                "orderId":id,"analysisStartedAt":predicted,"analysisCompletedAt":predicted,"clientBuildVersion":"test"});
            first["recipientOwnerTrace"]["scorePolicyVersion"]=json!(OWNER_SCORE_POLICY);
            let d=&mut first["recipientOwnerTrace"]["decisions"][0];
            d["legacyRequiredMargin"]=json!(30);
            d["legacyFirstBlockingReason"]=if blocked{json!("paragraph_boundary")}else{Value::Null};
            d["selectedAnchor"]=d["candidates"][1]["anchor"].clone();
            d["candidates"][0]["legacyScore"]=json!(10);
            d["candidates"][1]["legacyScore"]=json!(110);
            let final_rows=first["recipients"].clone();
            p["confirmationSnapshot"]["recipients"]=final_rows;
            p
        };
        let mut orders=(0..5).map(|i|make(&format!("fit-{i}"),"2026-01-02T00:00:00Z","2026-01-03T00:00:00Z",false)).collect::<Vec<_>>();
        orders.push(make("late","2026-01-02T00:00:00Z","2026-02-02T00:00:00Z",false));
        orders.push(make("test","2026-02-03T00:00:00Z","2026-02-04T00:00:00Z",false));
        orders.push(make("blocked","2026-02-03T00:00:00Z","2026-02-04T00:00:00Z",true));
        let config=json!({"fitStart":"2026-01-01T00:00:00Z","testStart":"2026-02-01T00:00:00Z",
            "testEnd":"2026-03-01T00:00:00Z","evaluationAsOf":"2026-03-02T00:00:00Z","clientBuildVersion":"test"});
        let report=calibrate_legacy_scores(&json!({"orders":orders,"config":config})).unwrap();
        assert_eq!(report["fitOrders"],5);assert_eq!(report["testOrders"],1);
        assert_eq!(report["lateLabelOrders"],1);assert_eq!(report["blockedObservations"],1);
        assert_eq!(report["bins"][15]["fitOrders"],5);
        assert!((report["bins"][15]["estimatedNextProxyFrequency"].as_f64().unwrap()-6.0/7.0).abs()<1e-10);
        assert!(report["testGroups"].as_array().unwrap().iter().all(|g|g["metrics"]["coveredObservations"]==1));
        assert_eq!(report["eligibleForAutomaticDecision"],false);
    }
    #[test]
    fn owner_gate_reconstructs_first_block_and_never_invents_numeric_score(){
        let units="😀김가상\n010-1234-5678\n박가상".encode_utf16().collect::<Vec<_>>();
        let mut p=owner_fixture();let obs=owner_result(&p);
        let d=&mut p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["recipientOwnerTrace"]["decisions"][0];
        d["legacyRequiredMargin"]=json!(30);
        d["legacyFirstBlockingReason"]=json!("paragraph_boundary");
        d["previousHasDelivery"]=json!(false);d["nearbyAddress"]=json!(false);
        d["candidates"][0]["sameParagraph"]=json!(true);d["candidates"][0]["legacyScore"]=Value::Null;
        d["candidates"][1]["sameParagraph"]=json!(false);d["candidates"][1]["legacyScore"]=Value::Null;
        d["candidates"][1]["gap"]=json!({"blankLines":1});d["candidates"][1]["lineDistance"]=json!(4);
        assert_eq!(owner_gate_input(&units,d,&obs),Some(("contact","paragraph_boundary")));
        d["legacyFirstBlockingReason"]=json!("prior_delivery_missing");
        assert!(owner_gate_input(&units,d,&obs).is_none());
        d["candidates"][0]["sameParagraph"]=json!(false);d["candidates"][1]["sameParagraph"]=json!(true);
        assert_eq!(owner_gate_input(&units,d,&obs),Some(("contact","prior_delivery_missing")));
        d["previousHasDelivery"]=json!(true);d["legacyFirstBlockingReason"]=json!("blank_before_next");
        assert_eq!(owner_gate_input(&units,d,&obs),Some(("contact","blank_before_next")));
        d["candidates"][1]["gap"]["blankLines"]=json!(0);d["legacyFirstBlockingReason"]=json!("next_distance_exceeds_3");
        assert_eq!(owner_gate_input(&units,d,&obs),Some(("contact","next_distance_exceeds_3")));
        d["candidates"][1]["lineDistance"]=json!(3);d["legacyFirstBlockingReason"]=json!("contact_without_nearby_address");
        assert_eq!(owner_gate_input(&units,d,&obs),Some(("contact","contact_without_nearby_address")));
        d["candidates"][0]["legacyScore"]=json!(0);
        assert!(owner_gate_input(&units,d,&obs).is_none());
    }
    #[test]
    fn gate_counts_keep_order_macro_and_observation_rate_distinct(){
        let mut counts=GateCounts::default();counts.add(1,0);counts.add(0,9);
        let v=counts.wire();assert_eq!(v["observedCorrectionFraction"],0.1);
        assert_eq!(v["orderMacroCorrectionFraction"],0.5);
        assert_eq!(v["trueErrorRateVerified"],false);
    }
    #[test]
    fn shadow_multiclass_transform_preserves_mass_and_outside_category(){
        let allowed=BTreeSet::from(["a","b"]);
        let r=json!({"totalSupportMass":10.0,"outsideCandidateSupportMass":2.0,
            "candidates":[{"candidate":"a","supportMass":7.0},{"candidate":"b","supportMass":1.0}]});
        let v=shadow_distribution(&r,&allowed,"not-listed").unwrap();
        assert_eq!(v.actual,2);
        assert_eq!(v.distribution,vec![0.7,0.1,0.2]);
        let p=shadow_transform(&v.distribution,1.5,0.05);
        assert!((p.iter().sum::<f64>()-1.0).abs()<1e-10);
        assert!(p[2]>0.0);
        let mut bad=r.clone();bad["totalSupportMass"]=json!(11.0);
        assert!(shadow_distribution(&bad,&allowed,"a").is_none());
    }
    #[test]
    fn shadow_calibration_fits_only_after_minimum_independent_orders(){
        let sample=||ShadowCalibrationSample{distribution:vec![0.9,0.1,0.0],actual:1,label:"explicit_correction"};
        let mut fit=ShadowFit::default();fit.add_order(&(0..100).map(|_|sample()).collect::<Vec<_>>());
        assert!(fit.parameters().is_none());
        for _ in 0..4{fit.add_order(&[sample()]);}
        let params=fit.parameters().unwrap();
        let mut metrics=ShadowCalTest::default();metrics.observe(&sample(),Some(params));metrics.observe(&sample(),None);
        let mut total=ShadowCalTest::default();total.merge(metrics);
        assert_eq!(total.wire()["coverage"],0.5);
        assert!(total.wire()["adjustedBrierOnCovered"].as_f64().unwrap()<total.wire()["rawBrierOnCovered"].as_f64().unwrap());
    }
    #[test]
    fn blend_fit_prefers_legacy_on_ties_and_requires_independent_orders(){
        let equal=||BlendSample{legacy:vec![0.7,0.3,0.0],shadow:vec![0.7,0.3,0.0],
            actual:0,label:"unchanged_proxy",legacy_choice:0,transfer_block:None};
        let mut fit=BlendFit::default();fit.add_order(&(0..100).map(|_|equal()).collect::<Vec<_>>());
        assert!(fit.weight().is_none());
        for _ in 1..10{fit.add_order(&[equal()]);}
        assert_eq!(fit.weight(),Some(0.0));
        let good=||BlendSample{legacy:vec![0.1,0.9,0.0],shadow:vec![0.9,0.05,0.05],
            actual:0,label:"explicit_correction",legacy_choice:1,transfer_block:None};
        let mut other=BlendFit::default();for _ in 0..10{other.add_order(&[good()]);}
        assert_eq!(other.weight(),Some(1.0));
        let s=good();let p=blend_distribution(&s.legacy,&s.shadow,0.5);
        assert!((p.iter().sum::<f64>()-1.0).abs()<1e-10);
        assert_eq!(p[2],0.025);
    }
    #[test]
    fn blend_test_compares_same_covered_observations(){
        let s=BlendSample{legacy:vec![0.0,1.0,0.0],shadow:vec![1.0,0.0,0.0],
            actual:0,label:"explicit_correction",legacy_choice:1,transfer_block:None};
        let mut local=BlendTest::default();local.observe(Some(&s),Some(0.5));local.observe(Some(&s),None);
        let mut total=BlendTest::default();total.merge(local);
        let v=total.wire();assert_eq!(v["coverage"],0.5);
        assert_eq!(v["legacyBrierOnCovered"],2.0);assert_eq!(v["shadowBrierOnCovered"],0.0);
        assert_eq!(v["combinedBrierOnCovered"],0.5);
    }
    #[test]
    fn blend_change_screen_never_grants_authority_and_checks_structural_blocks(){
        let mut s=BlendSample{legacy:vec![0.05,0.95,0.0],shadow:vec![0.95,0.04,0.01],
            actual:0,label:"explicit_correction",legacy_choice:1,transfer_block:None};
        assert_eq!(screen_blend_change(&s,Some(1.0)),("diagnostic_proposal_only",Some(0)));
        assert_eq!(screen_blend_change(&s,None),("missing_weight",None));
        assert_eq!(screen_blend_change(&s,Some(0.0)),("retain_legacy",None));
        s.transfer_block=Some("target_delivery_field_occupied");
        assert_eq!(screen_blend_change(&s,Some(1.0)),("target_delivery_field_occupied",None));
        s.transfer_block=None;s.shadow=vec![0.80,0.10,0.10];
        assert_eq!(screen_blend_change(&s,Some(1.0)),("outside_mass_too_high",None));
        s.shadow=vec![0.75,0.24,0.01];
        assert_eq!(screen_blend_change(&s,Some(1.0)),("low_combined_support",None));
        s.shadow=vec![0.95,0.04,0.01];
        let mut metrics=ChangeScreenMetrics::default();metrics.observe(&s,Some(1.0));
        assert_eq!(metrics.wire()["improvedAgainstOperationalLabel"],1);
        assert_eq!(metrics.wire()["actualChanges"],0);
        assert_eq!(metrics.wire()["eligibleForAutomaticDecision"],false);
    }
    #[test]
    fn owner_transfer_checks_original_links_and_occupied_target(){
        let mut p=owner_fixture();
        let raw=p["confirmationSnapshot"]["raw"].as_str().unwrap().encode_utf16().collect::<Vec<_>>();
        let first=&p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"];
        let d=&first["recipientOwnerTrace"]["decisions"][0];
        assert_eq!(owner_transfer_block(&raw,first,d,(6,19),"previous"),None);
        p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["recipients"][1]["mobile"]=json!("010-0000-0000");
        let first=&p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"];
        let d=&first["recipientOwnerTrace"]["decisions"][0];
        assert_eq!(owner_transfer_block(&raw,first,d,(6,19),"previous"),Some("target_delivery_field_occupied"));
        p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["recipients"][0]["mobile"]=json!("010-1234-5678 추가");
        let first=&p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"];
        let d=&first["recipientOwnerTrace"]["decisions"][0];
        assert_eq!(owner_transfer_block(&raw,first,d,(6,19),"previous"),Some("partial_or_modified_source_value"));
    }
    #[test]
    fn runtime_blend_rejects_future_or_incompatible_model(){
        let mut model=json!({"schemaVersion":1,"clientBuildVersion":"test","scorePolicyVersion":OWNER_SCORE_POLICY,
            "captureVersion":4,"supportVersion":4,"featureVersion":1,"contributionVersion":2,
            "safetyPolicyVersion":1,"explicitToWeakRatio":3.0,"automaticChangeAuthorized":false,
            "availableAfter":"2026-11-27T01:00:00Z"});
        let payload=|model:&Value|json!({"raw":"synthetic","clientBuildVersion":"test",
            "predictionAt":"2026-11-27T00:00:00Z","model":model,
            "initialAnalysis":{"recipients":[],"recipientOwnerTrace":{"schemaVersion":1,"scorePolicyVersion":OWNER_SCORE_POLICY}},
            "learningShadow":{"status":"captured_shadow_support","captureVersion":4,"sourceDigestsMatched":true,"decisionChanged":false}});
        assert!(run_blend_shadow(&payload(&model)).is_err());
        model["availableAfter"]=json!("2026-11-26T00:00:00Z");
        let result=run_blend_shadow(&payload(&model)).unwrap();
        assert_eq!(result["status"],"runtime_shadow_only");
        assert_eq!(result["decisionChanged"],false);
        model["automaticChangeAuthorized"]=json!(true);
        assert!(run_blend_shadow(&payload(&model)).is_err());
    }
    #[test]
    fn combined_confirmation_missing_or_unverified_prediction_never_becomes_a_label(){
        let mut p=owner_fixture();
        assert_eq!(evaluate_combined_confirmation(&p).unwrap()["status"],"no_combined_prediction");
        p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["initialPredictionCapture"]=json!({
            "learningShadow":{"combinedShadow":{"status":"runtime_shadow_only"}}});
        let r=evaluate_combined_confirmation(&p).unwrap();
        assert_eq!(r["status"],"unverified_prediction_time");
        assert_eq!(r["groups"],json!([]));assert_eq!(r["decisionChanged"],false);
    }
    #[test]
    fn paired_registered_model_metrics_preserve_abstention_and_order_weighting(){
        let mut first=ModelPairCounts::default();
        first.observe("previous","next",Some("previous"),Some("next"));
        first.observe("previous","previous",None,Some("next"));
        let mut second=ModelPairCounts::default();
        second.observe("previous","previous",Some("previous"),None);
        let mut total=ModelPairCounts::default();total.merge_order(first);total.merge_order(second);
        let v=total.wire();
        assert_eq!(v["eligibleObservations"],3);assert_eq!(v["pairedObservations"],1);
        assert_eq!(v["candidateOnlyAgrees"],1);assert_eq!(v["pairedDelta"],1.0);
        assert_eq!(v["pairedOrders"],1);assert_eq!(v["independentOrders"],2);
        assert!((v["fallbackBaselineAgreement"].as_f64().unwrap()-2.0/3.0).abs()<1e-10);
        assert!((v["fallbackCandidateAgreement"].as_f64().unwrap()-2.0/3.0).abs()<1e-10);
    }
    #[test]
    fn order_inference_exact_harm_and_sign_bounds(){
        let v=order_inference(&vec![1.0;20],0.05);
        assert!((v["oneSidedImprovementSignP"].as_f64().unwrap()-0.5_f64.powi(20)).abs()<1e-12);
        assert!(v["orderMeanInterval"]["lower"].as_f64().unwrap()>0.0);
        let z=order_harm_upper(100,0,0.0125).unwrap();
        assert!((z-(1.0-0.0125_f64.powf(0.01))).abs()<1e-10);
        assert_eq!(order_harm_upper(0,0,0.05),None);
        assert_eq!(order_harm_upper(10,10,0.05),Some(1.0));
        let tied=order_inference(&[0.0;10],0.05);
        assert!(tied["oneSidedImprovementSignP"].is_null());
        assert!(tied["orderMeanInterval"]["lower"].as_f64().unwrap()<0.0);
    }
    #[test]
    fn performance_combined_confirmation_shares_exact_comparison(){
        // The bare comparison fixture has no confirmed-order identity.
        // Exercise both the ordinary and the full verified evaluation paths.
        let mut p=patterned_order("performance-confirmed");
        p["learningActive"]=json!(true);
        p["confirmationSnapshot"]["recognitionBaseline"]["auditEventIds"]=json!([]);
        let assert_equivalent=|p:&Value|{
            let mut expected=super::super::learning_context::statistics(&json!({"orders":[p]})).unwrap();
            expected["combinedEvaluation"]=evaluate_combined_confirmation(p).unwrap();
            assert_eq!(expected["uniqueOrders"],1);
            assert_eq!(expected["auditCoverage"]["verifiedManifestOrders"],1);
            assert!(!expected["statistics"].as_array().unwrap().is_empty());
            assert_eq!(super::super::learning_context::summarize_confirmation(p).unwrap(),expected);
            expected
        };
        assert_eq!(assert_equivalent(&p)["combinedEvaluation"]["status"],"no_combined_prediction");
        p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["initialPredictionCapture"]=json!({
            "schemaVersion":1,"origin":"native_reception_worker","orderId":"performance-confirmed",
            "analysisStartedAt":"2026-11-26T23:00:00Z","analysisCompletedAt":"2026-11-26T23:00:01Z",
            "learningShadow":{"captureVersion":4,"sourceDigestsMatched":true,"capturedAt":"2026-11-26T23:00:02Z",
                "combinedShadow":{"status":"runtime_shadow_only","schemaVersion":1,
                    "modelId":format!("blend-{}","a".repeat(64)),"predictionAt":"2026-11-26T23:00:03Z",
                    "currentModelSourceDigestsMatched":true,"automaticChangeAuthorized":false,"decisionChanged":false,
                    "decisions":[{"start":6,"end":19,"legacyCandidate":"previous","sourceKind":"contact",
                        "legacyRoute":"score","contextLevel":"base","learningWeight":0.5,
                        "candidateOrder":["next","previous","outside"],"legacyProxy":[0.0,1.0,0.0],
                        "learningProxy":[0.0,1.0,0.0],"combinedProxy":[0.0,1.0,0.0],
                        "status":"legacy_retained","diagnosticCandidate":null,
                        "decisionChanged":false,"eligibleForAutomaticDecision":false}]}}});
        let evaluated=assert_equivalent(&p);
        assert_eq!(evaluated["combinedEvaluation"]["status"],"compared_diagnostic_only");
        assert_eq!(evaluated["combinedEvaluation"]["groups"][0]["evaluated"],1);
    }
    #[test]
    fn field_runtime_shadow_uses_frozen_model_and_preserves_initial_product(){
        let first=json!({"candidates":[{"start":0,"end":3,"text":"709","kind":"product","line":0,"paragraph":0}],
            "recipients":[{"items":[{"id":"i","productId":"old","sources":[{"start":0,"end":3,"text":"709","field":"product"}]}]}],
            "classificationCandidateTrace":{"schemaVersion":1,"decisions":[{"line":0,"source":{"start":0,"end":3},
                "productStages":[{"candidates":[{"productId":"old"},{"productId":"new"}]}]}],
                "itemDecisions":[{"itemId":"i","productId":"old","productTraceLine":0,"productTraceStart":0}]}});
        let projection=super::super::learning_context::lookup(&json!({"raw":"709","initialAnalysis":first})).unwrap();
        let entry=rows(&projection["fieldEntries"]).find(|v|v["task"]=="product").unwrap();
        assert_eq!(entry["shadowCandidates"],json!(["new","old"]));
        let key=entry["contexts"][0]["key"].clone();
        let shadow=json!({"status":"captured_shadow_support","captureVersion":4,"sourceDigestsMatched":true,"decisionChanged":false,
            "locations":[{"task":"product","start":0,"end":3,"candidateCoverage":"traced_partial",
                "contextKeys":[key],"candidates":["new","old"]}],
            "contextSelection":[{"task":"product","start":0,"end":3,"status":"diagnostic_context_selected",
                "contextConflict":false,"selectedContextKey":key,"diagnosticCandidate":"new"}],
            "contexts":[{"task":"product","contextKey":key,"supportVersion":4,"featureVersion":1,"contributionVersion":2,
                "candidateIds":["new","old"],"contextQuality":{"status":"diagnostic_screen_pass"},
                "totalSupportMass":100.0,"outsideCandidateSupportMass":1.0,
                "candidates":[{"candidate":"new","supportMass":98.0},{"candidate":"old","supportMass":1.0}]}]});
        let model=json!({"modelId":format!("field-blend-{}","a".repeat(64)),"createdAt":"2026-01-02T00:00:00Z",
            "model":{"task":"product","schemaVersion":1,"clientBuildVersion":"test","evaluationAsOf":"2026-01-01T00:00:00Z",
                "automaticChangeAuthorized":false,"selectionAuthorized":false,"models":[{"task":"product","contextLevel":"base",
                    "candidateCoverage":"traced_partial","calibrationOrders":10,"weightFitOrders":10,
                    "legacyReliabilityProxy":[0.9,0.1,0.0],"shadowTemperature":1.0,"shadowUniformSmoothing":0.0,
                    "learningWeight":1.0,"automaticChangeAuthorized":false}]}});
        let mut payload=json!({"raw":"709","initialAnalysis":first,"learningShadow":shadow,"models":[model],
            "predictionAt":"2026-01-03T00:00:00Z","clientBuildVersion":"test"});
        let result=run_field_blend_shadow(&payload).unwrap();
        assert_eq!(result["decisions"][0]["legacyCandidate"],"old");
        assert_eq!(result["decisions"][0]["diagnosticCandidate"],"new");
        assert_eq!(result["decisionChanged"],false);
        assert_eq!(payload["initialAnalysis"]["recipients"][0]["items"][0]["productId"],"old");
        payload["models"][0]["createdAt"]=json!("2026-01-04T00:00:00Z");
        assert!(run_field_blend_shadow(&payload).is_err());
    }
    #[test]
    fn field_paired_replay_excludes_training_orders_and_duplicate_ids(){
        let baseline=json!({"modelId":"a","model":{"task":"product","sourceDigests":{"training":"digest"}}});
        let candidate=json!({"modelId":"b","model":{"task":"product","sourceDigests":{}}});
        let mut payload=json!({"baseline":baseline,"candidate":candidate,"clientBuildVersion":"test",
            "evaluationAsOf":"2026-01-03T00:00:00Z","orders":[{"id":"training"}]});
        let result=compare_field_models(&payload).unwrap();
        assert_eq!(result["excludedOrders"],1);assert_eq!(result["groups"],json!([]));
        assert!(result["winner"].is_null());assert_eq!(result["decisionChanged"],false);
        payload["orders"]=json!([{"id":"training"},{"id":"training"}]);
        assert!(compare_field_models(&payload).is_err());
    }
    #[test]
    fn field_blend_calibration_weights_and_outside_are_independent(){
        for task in ["type","product","variant","quantity"]{
            let group=(task.to_owned(),"base".to_owned(),"traced_partial".to_owned());
            let make=|actual|FieldBlendRecord{old:0,candidate_ids:vec!["a".into(),"b".into()],shadow:ShadowCalibrationSample{
                distribution:vec![0.05,0.94,0.01],actual,label:"explicit_correction"}};
            let mut cal=FieldCalibration::default();
            assert!(cal.blend_sample(&make(1)).is_none());
            for _ in 0..MIN_CALIBRATION_ORDERS{cal.add_order(&[make(1)]);}
            let mut fits=BTreeMap::<FieldGroup,BlendFit>::new();
            let mut fit=BlendFit::default();
            for _ in 0..MIN_BLEND_ORDERS{fit.add_order(&[cal.blend_sample(&make(1)).unwrap()]);}
            assert!(fit.weight().is_some());
            fits.insert(group.clone(),fit);
            assert_eq!(fits.len(),1);
            let sample=cal.blend_sample(&make(1)).unwrap();
            assert!((sample.legacy.iter().sum::<f64>()-1.0).abs()<1e-10);
            assert_eq!(field_blend_top(&sample,1.0),Some(1));
            let outside=BlendSample{legacy:vec![0.0,0.0,1.0],shadow:vec![0.0,0.0,1.0],
                actual:2,label:"explicit_correction",legacy_choice:0,transfer_block:None};
            assert_eq!(field_blend_top(&outside,1.0),None);
        }
    }
    #[test]
    fn field_blend_evaluation_and_inference_share_protected_fallback(){
        let sample=BlendSample{legacy:vec![0.9,0.1,0.0],shadow:vec![0.01,0.98,0.01],
            actual:1,label:"explicit_correction",legacy_choice:0,transfer_block:None};
        let mut record=FieldBlendRecord{old:0,candidate_ids:vec!["old".into(),"new".into()],
            shadow:ShadowCalibrationSample{distribution:sample.shadow.clone(),actual:1,label:"explicit_correction"}};
        assert_eq!(field_blend_recommendation("product",&record,&sample,1.0),(1,"diagnostic_proposal_only"));
        assert_eq!(field_blend_recommendation("quantity",&record,&sample,1.0),(0,"literal_quantity_protected"));
        record.candidate_ids=vec!["mobile".into(),"phone".into()];
        assert_eq!(field_blend_recommendation("type",&record,&sample,1.0),(0,"contact_slot_not_type_change"));
        assert_eq!(field_blend_recommendation("type",&record,&sample,0.0),(0,"legacy_agreement"));
        let mut outside=sample;outside.shadow=vec![0.0,0.0,1.0];
        assert_eq!(field_blend_recommendation("product",&record,&outside,1.0),(0,"insufficient_or_conflicting_support"));
    }
    #[test]
    fn field_blend_task_evaluation_includes_unjoined_observations(){
        let mut order=seal_test_audits(patterned_order("missing-field-support"));
        order["learningActive"]=json!(true);
        order["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]["initialPredictionCapture"]=json!({
            "schemaVersion":1,"origin":"native_reception_worker","orderId":"missing-field-support",
            "clientBuildVersion":"test","analysisStartedAt":"2026-11-26T23:50:00Z","analysisCompletedAt":"2026-11-26T23:51:00Z",
            "learningShadow":{"status":"captured_shadow_support","captureVersion":4,"sourceDigestsMatched":true,
                "explicitToWeakRatio":3.0,"decisionChanged":false,"modelId":format!("shadow-{}","a".repeat(64)),
                "capturedAt":"2026-11-26T23:52:00Z","locations":[],"contexts":[],"contextSelection":[]}});
        let config=json!({"fitStart":"2026-08-01T00:00:00Z","weightStart":"2026-09-01T00:00:00Z",
            "testStart":"2026-11-01T00:00:00Z","testEnd":"2026-12-01T00:00:00Z",
            "evaluationAsOf":"2026-12-01T00:00:00Z","clientBuildVersion":"test"});
        let result=learn_field_blend_weights(&json!({"config":config,"orders":[order]})).unwrap();
        assert_eq!(result["excludedOrders"],0);
        let group=rows(&result["testTaskGroups"]).find(|v|v["task"]=="type"&&v["labelKind"]=="unchanged_proxy").unwrap();
        assert_eq!(group["metrics"]["eligibleObservations"],3);
        assert_eq!(group["metrics"]["independentOrders"],1);
        assert_eq!(group["metrics"]["candidateCovered"],0);
        assert_eq!(group["metrics"]["fallbackBaselineAgreement"],1.0);
        assert_eq!(group["metrics"]["fallbackCandidateAgreement"],1.0);
        assert_eq!(result["testGroups"],json!([]));
    }
    #[test]
    fn field_blend_rejects_overlapping_time_windows_and_duplicate_orders(){
        let mut config=json!({"fitStart":"2026-01-01T00:00:00Z","weightStart":"2026-02-01T00:00:00Z",
            "testStart":"2026-03-01T00:00:00Z","testEnd":"2026-04-01T00:00:00Z",
            "evaluationAsOf":"2026-05-01T00:00:00Z","clientBuildVersion":"test"});
        let order=json!({"id":"a"});
        let result=learn_field_blend_weights(&json!({"config":config,"orders":[order.clone()]})).unwrap();
        assert_eq!(result["models"],json!([]));
        assert_eq!(result["excludedOrders"],1);
        assert_eq!(result["automaticChangeAuthorized"],false);
        assert!(learn_field_blend_weights(&json!({"config":config,"orders":[order.clone(),order.clone()]})).is_err());
        config["weightStart"]=config["testStart"].clone();
        assert!(learn_field_blend_weights(&json!({"config":config,"orders":[order]})).is_err());
    }
    #[test]
    fn performance_shadow_grid_matches_reference_losses(){
        let samples=[ShadowCalibrationSample{distribution:vec![0.4,0.5,0.1],actual:0,label:"explicit_correction"},
            ShadowCalibrationSample{distribution:vec![0.7,0.1,0.2],actual:2,label:"unchanged_proxy"}];
        let mut fit=ShadowFit::default();fit.add_order(&samples);
        for (i,t) in SHADOW_TEMPERATURES.iter().enumerate(){
            for (j,s) in SHADOW_SMOOTHING.iter().enumerate(){
                let expected=samples.iter().map(|v|multiclass_brier(&shadow_transform(&v.distribution,*t,*s),v.actual)).sum::<f64>()/samples.len() as f64;
                assert!((expected-fit.losses[i*5+j]).abs()<1e-12);
            }
        }
    }
    fn owner_fixture()->Value{
        // Emoji makes the UTF-16 positions differ from byte offsets.
        let raw="😀김가상\n010-1234-5678\n박가상";
        let name_a=json!({"start":2,"end":5,"text":"김가상","field":"name"});
        let name_b=json!({"start":20,"end":23,"text":"박가상","field":"name"});
        let phone=json!({"start":6,"end":19,"text":"010-1234-5678","field":"mobile"});
        let a=json!({"id":"a","name":"김가상","mobile":"010-1234-5678",
            "sources":[name_a,phone.clone()]});
        let b=json!({"id":"b","name":"박가상","mobile":"","sources":[name_b]});
        json!({"confirmationSnapshot":{"raw":raw,"recipients":[a.clone(),b.clone()],
            "recognitionBaseline":{"schemaVersion":1,"initialAnalysis":{
                "recipients":[a,b],
                "candidates":[{"start":6,"end":19,"text":"010-1234-5678",
                    "kind":"mobile","value":"010-1234-5678"}],
                "recipientOwnerTrace":{"schemaVersion":1,"decisions":[{
                    "source":{"start":6,"end":19,"kind":"mobile"},
                    "selectedAnchor":{"start":2,"end":5,"kind":"name"},
                    "candidates":[
                        {"relation":"previous","anchor":{"start":2,"end":5,"kind":"name"}},
                        {"relation":"next","anchor":{"start":20,"end":23,"kind":"name"}}
                    ]
                }]}
            }}
        }})
    }
    fn owner_result(payload:&Value)->Value{
        let result=compare(payload).unwrap();
        let found=result["observations"].as_array().unwrap().iter()
            .filter(|v|v["task"]=="recipient_ownership").collect::<Vec<_>>();
        assert_eq!(found.len(),1);
        found[0].clone()
    }
    #[test]
    fn owner_trace_final_comparison_survives_changed_ids_and_row_order(){
        let mut p=owner_fixture();
        let rs=p["confirmationSnapshot"]["recipients"].as_array_mut().unwrap();
        rs[0]["id"]=json!("new-a");
        rs[1]["id"]=json!("new-b");
        rs.swap(0,1);
        let o=owner_result(&p);
        assert_eq!(o["outcome"],"unchanged_observed");
        assert_eq!(o["initialAnchor"],json!({"start":2,"end":5}));
        assert_eq!(o["finalAnchor"],o["initialAnchor"]);
        assert_eq!(o["eligibleForCandidateLearning"],true);
    }
    #[test]
    fn owner_trace_final_comparison_distinguishes_reassignment_and_alternate_phone(){
        let mut p=owner_fixture();
        let rs=p["confirmationSnapshot"]["recipients"].as_array_mut().unwrap();
        let mut source=rs[0]["sources"].as_array_mut().unwrap().pop().unwrap();
        source["field"]=json!("phone");
        rs[0]["mobile"]=json!("");
        rs[1]["phone"]=json!("01012345678");
        rs[1]["sources"].as_array_mut().unwrap().push(source);
        let o=owner_result(&p);
        assert_eq!(o["outcome"],"changed_requires_review");
        assert_eq!(o["initialCandidates"],json!(["previous"]));
        assert_eq!(o["finalCandidates"],json!(["next"]));
        assert_eq!(o["finalAnchor"],json!({"start":20,"end":23}));
        assert_eq!(o["eligibleForCandidateLearning"],false);
    }
    #[test]
    fn owner_trace_final_comparison_rejects_stale_missing_and_duplicate_links(){
        let mut stale=owner_fixture();
        stale["confirmationSnapshot"]["recipients"][0]["mobile"]=json!("010-9999-9999");
        assert_eq!(owner_result(&stale)["outcome"],"unresolved_evidence");
        let mut missing=owner_fixture();
        missing["confirmationSnapshot"]["recipients"][0]["sources"].as_array_mut().unwrap().pop();
        assert_eq!(owner_result(&missing)["outcome"],"unresolved_evidence");
        let mut duplicate=owner_fixture();
        let link=duplicate["confirmationSnapshot"]["recipients"][0]["sources"][1].clone();
        duplicate["confirmationSnapshot"]["recipients"][1]["sources"].as_array_mut().unwrap().push(link);
        duplicate["confirmationSnapshot"]["recipients"][1]["mobile"]=json!("010-1234-5678");
        assert_eq!(owner_result(&duplicate)["outcome"],"unresolved_evidence");
        let mut renamed=owner_fixture();
        renamed["confirmationSnapshot"]["recipients"][0]["name"]=json!("다른이름");
        assert_eq!(owner_result(&renamed)["outcome"],"unresolved_evidence");
    }
    #[test]
    fn owner_trace_final_comparison_rejects_unverified_initial_trace(){
        let mut p=owner_fixture();
        p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]
            ["recipientOwnerTrace"]["decisions"][0]["selectedAnchor"]=json!({
                "start":20,"end":23,"kind":"name"
            });
        assert_eq!(owner_result(&p)["outcome"],"unresolved_evidence");
    }
    #[test]
    fn owner_trace_final_comparison_rejects_duplicate_anchor_across_rows(){
        let mut p=owner_fixture();
        let anchor=p["confirmationSnapshot"]["recipients"][0]["sources"][0].clone();
        p["confirmationSnapshot"]["recipients"][1]["sources"].as_array_mut().unwrap().push(anchor);
        assert_eq!(owner_result(&p)["outcome"],"unresolved_evidence");
    }
    fn patterned_order(id:&str)->Value{
        let mut p=owner_fixture();
        p["id"]=json!(id);
        p["recognitionAuditEvents"]=json!([]);
        p["confirmedAt"]=json!("2026-11-27T00:00:00Z");
        let snapshot=&mut p["confirmationSnapshot"];
        snapshot["at"]=json!("2026-11-27T00:00:00Z");
        snapshot["data"]=json!({"clientId":"client-1"});
        let baseline=&mut snapshot["recognitionBaseline"];
        baseline["finalClientId"]=json!("client-1");
        baseline["finalClientSelection"]=json!("MANUAL");
        let first=&mut baseline["initialAnalysis"];
        first["recognitionContext"]=json!({"clientId":"client-1","clientBasis":"MANUAL"});
        first["candidates"][0]["line"]=json!(1);
        first["candidates"][0]["paragraph"]=json!(0);
        first["candidates"].as_array_mut().unwrap().extend([
            json!({"start":2,"end":5,"text":"김가상","kind":"name","line":0,"paragraph":0}),
            json!({"start":20,"end":23,"text":"박가상","kind":"name","line":2,"paragraph":0})
        ]);
        let decision=&mut first["recipientOwnerTrace"]["decisions"][0];
        decision["previousHasDelivery"]=json!(false);
        decision["nearbyAddress"]=json!(false);
        for candidate in decision["candidates"].as_array_mut().unwrap(){
            candidate["sameParagraph"]=json!(true);
            candidate["lineDistance"]=json!(1);
            candidate["gap"]=json!({"blankLines":0,"contentLines":0,"maxConsecutiveBlankLines":0});
        }
        p
    }
    fn audited_reassignment()->Value{
        let mut p=patterned_order("audit-order");
        let first=p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"].clone();
        let before=first["recipients"].clone();
        let mut after=before.clone();
        let link=after[0]["sources"].as_array_mut().unwrap().pop().unwrap();
        after[0]["mobile"]=json!("");
        after[1]["mobile"]=json!("010-1234-5678");
        after[1]["sources"].as_array_mut().unwrap().push(link);
        let block=json!({"id":"6:19","start":6,"end":19,"text":"010-1234-5678","field":"mobile"});
        p["confirmationSnapshot"]["recipients"]=after.clone();
        p["recognitionAuditEvents"]=json!([{
            "id":"audit-1","orderId":"audit-order","at":"2026-11-26T23:59:00Z",
            "sourceRaw":p["confirmationSnapshot"]["raw"],"initialAnalysis":first,
            "beforeRows":before,"afterRows":after,
            "context":{"schemaVersion":1,"clientId":"client-1","clientSelection":"MANUAL",
                "beforeClassification":{"blocks":[block.clone()],"assignments":{}},
                "afterClassification":{"blocks":[block],"assignments":{"6:19":{
                    "recipientId":"b","field":"mobile","value":"010-1234-5678"
                }}}
            }
        }]);
        p
    }
    #[test]
    fn owner_audit_reconciles_exact_explicit_correction_and_excludes_stale_events(){
        let p=audited_reassignment();
        let o=owner_result(&p);
        assert_eq!(o["outcome"],"explicit_correction_observed");
        assert_eq!(o["negativeCandidates"],json!(["previous"]));
        assert_eq!(o["positiveCandidates"],json!(["next"]));
        let mut duplicate=p.clone();
        let event=duplicate["recognitionAuditEvents"][0].clone();
        duplicate["recognitionAuditEvents"].as_array_mut().unwrap().push(event);
        assert_eq!(owner_result(&duplicate)["outcome"],"explicit_correction_observed");
        for (field,value) in [("sourceRaw",json!("different")),("orderId",json!("other")),
            ("at",json!("2026-11-28T00:00:00Z"))]{
            let mut stale=p.clone();
            stale["recognitionAuditEvents"][0][field]=value;
            assert_eq!(owner_result(&stale)["outcome"],"changed_requires_review");
        }
        let mut reverted=p.clone();
        reverted["confirmationSnapshot"]["recipients"]=p["recognitionAuditEvents"][0]["beforeRows"].clone();
        let o=owner_result(&reverted);
        assert_eq!(o["outcome"],"changed_requires_review");
        assert_eq!(o["eligibleForCandidateLearning"],false);
        let mut external=p.clone();
        external["confirmationSnapshot"]["recipients"][1]["mobile"]=json!("010-9999-9999");
        assert_eq!(owner_result(&external)["outcome"],"unresolved_evidence");
    }
    // Synthetic confirmed records seal their own exact audit IDs. Legacy
    // fixtures used for first/final comparisons intentionally remain unsealed.
    fn seal_test_audits(mut p:Value)->Value{
        let ids=rows(&p["recognitionAuditEvents"]).map(|e|e["id"].clone())
            .collect::<Vec<_>>();
        p["confirmationSnapshot"]["recognitionBaseline"]["auditEventIds"]=json!(ids);
        p
    }
    #[test]
    fn owner_audit_statistics_separate_positive_negative_and_weak_evidence(){
        let p=audited_reassignment();
        let summary=super::super::learning_context::statistics(&json!({
            "orders":[seal_test_audits(p.clone()),seal_test_audits(p),seal_test_audits(patterned_order("unchanged"))]
        })).unwrap();
        for group in summary["statistics"].as_array().unwrap(){
            let c=&group["counts"];
            assert_eq!(c["weakUnchangedCount"],1);
            assert_eq!(c["explicitCorrectionCount"],1);
            assert_eq!(c["explicitCorrectionOrders"],1);
            assert_eq!(c["explicitPositiveCandidates"]["next"],1);
            assert_eq!(c["explicitNegativeCandidates"]["previous"],1);
            assert_eq!(c["pendingCorrectionCount"],0);
        }
    }
    #[test]
    fn owner_context_lookup_and_observations_share_keys_without_personal_text(){
        let p=patterned_order("one");
        let observation=owner_result(&p);
        let contexts=observation["contexts"].as_array().unwrap();
        assert_eq!(contexts.len(),3);
        let lookup=super::super::learning_context::lookup(&json!({
            "raw":p["confirmationSnapshot"]["raw"],
            "initialAnalysis":p["confirmationSnapshot"]["recognitionBaseline"]["initialAnalysis"]
        })).unwrap();
        assert_eq!(lookup["entries"][0]["contexts"],observation["contexts"]);
        let serialized=observation["contexts"].to_string();
        for secret in ["김가상","박가상","010-1234-5678","😀"]{
            assert!(!serialized.contains(secret));
        }
    }
    #[test]
    fn owner_statistics_deduplicates_orders_and_separates_pending_corrections(){
        let a=patterned_order("one");
        let mut b=patterned_order("two");
        let rs=b["confirmationSnapshot"]["recipients"].as_array_mut().unwrap();
        let source=rs[0]["sources"].as_array_mut().unwrap().pop().unwrap();
        rs[0]["mobile"]=json!("");
        rs[1]["mobile"]=json!("010-1234-5678");
        rs[1]["sources"].as_array_mut().unwrap().push(source);
        let result=super::super::learning_context::statistics(&json!({
            "orders":[seal_test_audits(a.clone()),seal_test_audits(a.clone()),seal_test_audits(b)]
        })).unwrap();
        assert_eq!(result["uniqueOrders"],2);
        assert_eq!(result["statistics"].as_array().unwrap().len(),3);
        for group in result["statistics"].as_array().unwrap(){
            let c=&group["counts"];
            assert_eq!(c["observationCount"],2);
            assert_eq!(c["independentOrders"],2);
            assert_eq!(c["weakUnchangedCount"],1);
            assert_eq!(c["weakIndependentOrders"],1);
            assert_eq!(c["pendingCorrectionCount"],1);
            assert_eq!(c["weakCandidates"]["previous"],1);
            assert_eq!(c["weakCandidates"]["next"],0);
            assert_eq!(c["eligibleForAutomaticDecision"],false);
        }
        let mut conflict=a.clone();
        conflict["confirmedAt"]=json!("different");
        assert!(super::super::learning_context::statistics(&json!({
            "orders":[a,conflict]
        })).is_err());
    }
    #[test]
    fn owner_statistics_requires_matching_manual_client_and_can_lookup_one_context(){
        let a=patterned_order("one");
        let mut b=patterned_order("two");
        b["confirmationSnapshot"]["data"]["clientId"]=json!("different-client");
        let result=super::super::learning_context::statistics(&json!({"orders":[seal_test_audits(a.clone()),seal_test_audits(b)]})).unwrap();
        let groups=result["statistics"].as_array().unwrap();
        assert_eq!(groups.len(),3);
        let client=groups.iter().find(|g|g["key"].as_str().unwrap().contains("client_structure")).unwrap();
        assert_eq!(client["counts"]["independentOrders"],1);
        let filtered=super::super::learning_context::statistics(&json!({
            "orders":[seal_test_audits(a)],"contextKeys":[client["key"]]
        })).unwrap();
        assert_eq!(filtered["statistics"].as_array().unwrap().len(),1);
    }

    fn field_audit_fixture(raw:&str,token:&str,old_field:&str,new_field:&str,
        before_item:Value,after_item:Value)->Value{
        let start=raw.find(token).unwrap();
        let a=raw[..start].encode_utf16().count();
        let b=a+token.encode_utf16().count();
        let mut before_item=before_item;
        let mut after_item=after_item;
        for (item,field) in [(&mut before_item,old_field),(&mut after_item,new_field)]{
            item["id"]=json!("i");
            item["sources"]=json!([{"start":a,"end":b,"text":token,"field":field}]);
        }
        let before=json!([{"id":"r","items":[before_item]}]);
        let after=json!([{"id":"r","items":[after_item]}]);
        let first=json!({"recipients":before,"candidates":[{"start":a,"end":b,"text":token,"kind":old_field}]});
        let block=json!({"id":"block","start":a,"end":b,"text":token,"field":old_field});
        let label=if new_field=="product"{after[0]["items"][0]["name"].as_str().unwrap().to_owned()}
            else{after[0]["items"][0]["quantity"].to_string()};
        json!({"id":"field-order","confirmedAt":"2026-11-27T00:00:00Z",
            "confirmationSnapshot":{"raw":raw,"at":"2026-11-27T00:00:00Z","recipients":after,
                "recognitionBaseline":{"schemaVersion":1,"initialAnalysis":first}},
            "recognitionAuditEvents":[{"id":"audit","orderId":"field-order","at":"2026-11-26T23:59:00Z",
                "sourceRaw":raw,"initialAnalysis":first,"beforeRows":before,"afterRows":after,
                "context":{"schemaVersion":1,
                    "beforeClassification":{"blocks":[block.clone()],"assignments":{}},
                    "afterClassification":{"blocks":[block],"assignments":{"block":{
                        "recipientId":"r","itemId":"i","field":new_field,"value":label
                    }}}}
            }]})
    }
    fn field_observation(p:&Value,task:&str)->Value{
        let result=compare(p).unwrap();
        let found=rows(&result["observations"]).find(|o|o["task"].as_str()==Some(task)).unwrap().clone();
        found
    }
    #[test]
    fn multi_product_audit_counts_only_initial_and_final_and_rejects_gaps(){
        let mut p=field_audit_fixture("709","709","product","product",
            json!({"name":"BD 708","productId":"old"}),
            json!({"name":"BD 709","productId":"new"}));
        let mut final_event=p["recognitionAuditEvents"][0].clone();
        let mut first_event=final_event.clone();
        let mut middle=first_event["beforeRows"].clone();
        middle[0]["items"][0]["productId"]=json!("intermediate");
        middle[0]["items"][0]["name"]=json!("BD 707");
        first_event["id"]=json!("first");
        first_event["at"]=json!("2026-11-26T23:57:00Z");
        first_event["afterRows"]=middle.clone();
        first_event["context"]["afterClassification"]["assignments"]["block"]["value"]=json!("BD 707");
        final_event["beforeRows"]=middle;
        final_event["context"]["beforeClassification"]["assignments"]=
            first_event["context"]["afterClassification"]["assignments"].clone();
        // Direct product correction may retain the original assignment.
        p["recognitionAuditEvents"]=json!([final_event.clone(),first_event.clone()]);
        let o=field_observation(&p,"product");
        assert_eq!(o["outcome"],"explicit_correction_observed");
        assert_eq!(o["verifiedTransitionCount"],2);
        assert_eq!(o["negativeCandidates"],json!(["old"]));
        assert_eq!(o["positiveCandidates"],json!(["new"]));
        let mut gap=p.clone();
        gap["recognitionAuditEvents"]=json!([final_event.clone()]);
        assert_eq!(field_observation(&gap,"product")["outcome"],"changed_requires_review");
        let mut tie=p.clone();
        tie["recognitionAuditEvents"][1]["at"]=final_event["at"].clone();
        assert_eq!(field_observation(&tie,"product")["outcome"],"changed_requires_review");
        let summary=super::super::learning_context::statistics(&json!({"orders":[seal_test_audits(p)]})).unwrap();
        let product=rows(&summary["fieldStatistics"]).find(|v|v["task"]=="product").unwrap();
        assert_eq!(product["counts"]["explicitCorrectionCount"],1);
        assert!(product["counts"]["explicitPositiveCandidates"].get("intermediate").is_none());
    }
    #[test]
    fn multi_owner_audit_uses_final_anchor_and_reverted_chain_is_not_weak_success(){
        let mut p=audited_reassignment();
        let first=p["recognitionAuditEvents"][0].clone();
        let mut undo=first.clone();
        undo["id"]=json!("undo");
        undo["at"]=json!("2026-11-26T23:59:10Z");
        undo["beforeRows"]=first["afterRows"].clone();
        undo["afterRows"]=first["beforeRows"].clone();
        undo["context"]["beforeClassification"]=first["context"]["afterClassification"].clone();
        undo["context"]["afterClassification"]["assignments"]["6:19"]["recipientId"]=json!("a");
        let mut last=first.clone();
        last["id"]=json!("last");
        last["at"]=json!("2026-11-26T23:59:20Z");
        last["context"]["beforeClassification"]=undo["context"]["afterClassification"].clone();
        p["recognitionAuditEvents"]=json!([last,undo.clone(),first.clone(),first.clone()]);
        let o=owner_result(&p);
        assert_eq!(o["outcome"],"explicit_correction_observed");
        assert_eq!(o["verifiedTransitionCount"],3);
        assert_eq!(o["negativeCandidates"],json!(["previous"]));
        assert_eq!(o["positiveCandidates"],json!(["next"]));
        p["confirmationSnapshot"]["recipients"]=undo["afterRows"].clone();
        p["recognitionAuditEvents"]=json!([first,undo]);
        let o=owner_result(&p);
        assert_eq!(o["outcome"],"changed_requires_review");
        assert_eq!(o["eligibleForCandidateLearning"],false);
    }

    #[test]
    fn field_audit_product_and_type_corrections_have_opposite_polarities(){
        let p=field_audit_fixture("😀709","709","product","product",
            json!({"name":"BD 708","productId":"old","variantId":"old-v"}),
            json!({"name":"BD 709","productId":"new","variantId":"new-v"}));
        let o=field_observation(&p,"product");
        assert_eq!(o["outcome"],"explicit_correction_observed");
        assert_eq!(o["negativeCandidates"],json!(["old"]));
        assert_eq!(o["positiveCandidates"],json!(["new"]));
        assert_ne!(field_observation(&p,"variant")["outcome"],"explicit_correction_observed");
        let t=field_audit_fixture("709","709","quantity","product",
            json!({"quantity":709,"quantityConfirmed":true}),
            json!({"name":"BD 709","productId":"new"}));
        let o=field_observation(&t,"type");
        assert_eq!(o["outcome"],"explicit_correction_observed");
        assert_eq!(o["negativeCandidates"],json!(["quantity"]));
        assert_eq!(o["positiveCandidates"],json!(["product"]));
        let summary=super::super::learning_context::statistics(&json!({"orders":[seal_test_audits(p.clone()),seal_test_audits(p)]})).unwrap();
        let product=rows(&summary["fieldStatistics"]).find(|v|v["task"]=="product").unwrap();
        assert_eq!(product["counts"]["explicitPositiveCandidates"]["new"],1);
        assert_eq!(product["counts"]["explicitNegativeCandidates"]["old"],1);
    }
    #[test]
    fn field_audit_quantity_requires_written_quantity_and_variant_requires_written_color(){
        let q=field_audit_fixture("3개","3개","quantity","quantity",
            json!({"quantity":2,"quantityConfirmed":true}),
            json!({"quantity":3,"quantityConfirmed":true}));
        let o=field_observation(&q,"quantity");
        assert_eq!(o["outcome"],"explicit_correction_observed");
        assert_eq!(o["negativeCandidates"],json!(["2"]));
        assert_eq!(o["positiveCandidates"],json!(["3"]));
        let old=json!({"name":"BD 709","productId":"p","variantId":"black","color":"Black"});
        let new=json!({"name":"BD 709","productId":"p","variantId":"white","color":"White"});
        let v=field_audit_fixture("709 화이트","709","product","product",old.clone(),new.clone());
        assert_eq!(field_observation(&v,"variant")["positiveCandidates"],json!(["white"]));
        let no_color=field_audit_fixture("709","709","product","product",old,new);
        assert_eq!(field_observation(&no_color,"variant")["outcome"],"changed_requires_review");
        let mut stale=q.clone();
        stale["recognitionAuditEvents"][0]["sourceRaw"]=json!("외부 변경");
        assert_eq!(field_observation(&stale,"quantity")["outcome"],"changed_requires_review");
    }

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
    fn audit_manifest_missing_or_conflicting_history_cannot_produce_votes(){
        let mut p=audited_reassignment();
        p["confirmationSnapshot"]["recognitionBaseline"]["auditEventIds"]=json!(["audit-1"]);
        assert_eq!(compare(&p).unwrap()["auditCoverage"],"verified_manifest");
        assert_eq!(owner_result(&p)["outcome"],"explicit_correction_observed");
        let mut missing=p.clone();
        missing["recognitionAuditEvents"]=json!([]);
        assert_eq!(compare(&missing).unwrap()["auditCoverage"],"incomplete_history");
        assert_ne!(owner_result(&missing)["eligibleForCandidateLearning"],true);
        let mut reverted=p.clone();
        reverted["confirmationSnapshot"]["recipients"]=p["recognitionAuditEvents"][0]["beforeRows"].clone();
        reverted["recognitionAuditEvents"]=json!([]);
        assert_eq!(owner_result(&reverted)["outcome"],"changed_requires_review");
        assert_eq!(owner_result(&reverted)["eligibleForCandidateLearning"],false);
        let mut other=p.clone();
        other["recognitionAuditEvents"][0]["initialAnalysis"]["changed"]=json!(true);
        assert_eq!(compare(&other).unwrap()["auditCoverage"],"incomplete_history");
    }
    #[test]
    fn option_proof_handles_abbreviations_compounds_and_rejects_ambiguity(){
        let check=|raw:&str,token:&str,name:&str,color:&str|{
            let a=raw.find(token).unwrap();
            let start=raw[..a].encode_utf16().count();
            let end=start+token.encode_utf16().count();
            let item=json!({"name":name,"color":color,"sources":[
                {"start":start,"end":end,"text":token,"field":"product"}]});
            verified_variant(&raw.encode_utf16().collect::<Vec<_>>(),(start,end),
                &json!([{"items":[item]}]),&item,None)
        };
        assert!(check("709W","709W","BD 709","White"));
        assert!(check("BK 709","709","BD 709","Black"));
        assert!(!check("709 B 타입","709","BD 709","Black"));
        assert!(!check("709W BK","709W","BD 709","White"));
        assert!(check("709 로즈골드","709","BD 709","Rose Gold"));
        assert!(check("709 코스모블랙","709","BD 709","Cosmo Black"));
        assert!(!check("709 화이트 블랙","709","BD 709","White"));
        assert!(check("709 화이트 가방 미포함 바퀴 장착 무타공","709",
            "BD 709 / White / 가방 미포함 / 바퀴 장착 / 민판","White"));
        assert!(!check("709 화이트 가방 미포함 가방 포함","709",
            "BD 709 / White / 가방 포함","White"));
        assert!(!check("709 화이트","709","BD 709 / White / 가방 포함","White"));
        assert!(!check("709 화이트","709","BD 709 / White / 고급형","White"));
        assert!(check("709 화이트 고급형","709","BD 709 / White / 고급형","White"));
    }
    #[test]
    fn external_product_substitution_cannot_become_a_model_correction(){
        let p=field_audit_fixture("709","709","product","product",
            json!({"name":"BD 708","productId":"old"}),
            json!({"name":"BD 707","productId":"external"}));
        assert_eq!(field_observation(&p,"product")["outcome"],"changed_requires_review");
    }
    #[test]
    fn equal_candidate_duplicate_links_and_unwritten_quantity_do_not_vote(){
        let mut p=field_audit_fixture("709","709","product","product",
            json!({"name":"BD 709","productId":"same"}),
            json!({"name":"BD 709","productId":"same"}));
        p["recognitionAuditEvents"]=json!([]);
        let duplicate=p["confirmationSnapshot"]["recipients"][0]["items"][0].clone();
        p["confirmationSnapshot"]["recipients"][0]["items"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(field_observation(&p,"product")["outcome"],"unresolved_evidence");
        let mut q=field_audit_fixture("3개","3개","quantity","quantity",
            json!({"quantity":2,"quantityConfirmed":true}),
            json!({"quantity":2,"quantityConfirmed":true}));
        q["recognitionAuditEvents"]=json!([]);
        assert_eq!(field_observation(&q,"quantity")["outcome"],"unresolved_evidence");
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
