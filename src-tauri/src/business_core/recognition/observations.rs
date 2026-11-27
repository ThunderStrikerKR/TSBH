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
    let trace=&first["recipientOwnerTrace"];
    if trace["schemaVersion"]!=1{return Vec::new();}
    let contexts=super::learning_context::contexts(units,first);
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

#[cfg(test)]
mod tests{
    use super::*;
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
