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
// Shared order evidence for the provisional owner comparator and learning
// diagnostics. A paragraph is an independent observation only when it has
// exactly one name and non-overlapping recognized spans. Neither final
// recipient assignments nor transaction/client history enters this index.
#[derive(Default)]
pub(super) struct OwnerOrderEvidence{
    paragraphs:BTreeMap<u64,(Vec<&'static str>,[i8;3])>,
    shapes:BTreeMap<Vec<&'static str>,usize>,
    totals:[[usize;2];3],
}
impl OwnerOrderEvidence{
    pub(super) fn new<'a>(source:impl IntoIterator<Item=(&'a str,u64,usize,usize)>)->Self{
        let mut grouped=BTreeMap::<u64,Vec<(usize,usize,&'static str)>>::new();
        for (kind,paragraph,start,end) in source{
            if let Some(r)=role(kind){grouped.entry(paragraph).or_default().push((start,end,r));}
        }
        let mut out=Self::default();
        for (paragraph,mut entries) in grouped{
            entries.sort_unstable();
            if entries.iter().any(|e|e.0>=e.1)||entries.windows(2).any(|w|w[0].1>w[1].0){continue;}
            let roles=entries.iter().map(|e|e.2).collect::<Vec<_>>();
            let mut names=roles.iter().enumerate().filter(|(_,r)|**r=="name");
            let Some((name,_))=names.next()else{continue;};
            if names.next().is_some(){continue;}
            let mut direction=[0i8;3];
            for (slot,field) in ["address","contact","product"].into_iter().enumerate(){
                let before=roles[..name].contains(&field);
                let after=roles[name+1..].contains(&field);
                direction[slot]=match (before,after){(true,false)=>-1,(false,true)=>1,_=>0};
                if direction[slot]!=0{out.totals[slot][if direction[slot]>0{1}else{0}]+=1;}
            }
            if direction==[0,0,0]{continue;}
            // Collapse adjacent equal roles (e.g. multiple contacts) without
            // treating quantities, raw strings or distances as votes.
            let mut shape=roles;
            shape.dedup();
            *out.shapes.entry(shape.clone()).or_default()+=1;
            out.paragraphs.insert(paragraph,(shape,direction));
        }
        out
    }
    pub(super) fn votes(&self,kind:&str,excluded:[u64;3])->[usize;2]{
        let slot=match role(kind){Some("address")=>0,Some("contact")=>1,Some("product")=>2,_=>return [0,0]};
        let mut votes=self.totals[slot];
        for (i,p) in excluded.iter().enumerate(){
            if excluded[..i].contains(p){continue;}
            if let Some((_,d))=self.paragraphs.get(p){
                if d[slot]!=0{votes[if d[slot]>0{1}else{0}]-=1;}
            }
        }
        votes
    }
    // Repeated exact paragraph shapes are reported separately, not added
    // as a second vote for the same observations.
    pub(super) fn repeated_shape_count(&self,paragraph:u64,excluded:[u64;3])->usize{
        let Some((shape,_))=self.paragraphs.get(&paragraph)else{return 0;};
        let mut n=self.shapes.get(shape).copied().unwrap_or(0);
        for (i,p) in excluded.iter().enumerate(){
            if excluded[..i].contains(p){continue;}
            if self.paragraphs.get(p).is_some_and(|(s,_)|s==shape){n=n.saturating_sub(1);}
        }
        n
    }
}
// 03-11: provisional document shape, separate from the existing local
// context contract. Never use final labels, values, distances or client IDs
// as pattern identity. Keep repetition count outside the structural key.
const DOCUMENT_PATTERN_VERSION:u32=1;
const DOCUMENT_PATTERN_MAX_EVENTS:usize=4096;
fn prefix_role_period(roles:&[&str])->usize{
    if roles.is_empty(){return 0;}
    // Prefix function: O(n) time and O(n) bounded temporary storage.
    let mut prefix=vec![0usize;roles.len()];
    for i in 1..roles.len(){
        let mut j=prefix[i-1];
        while j>0&&roles[i]!=roles[j]{j=prefix[j-1];}
        if roles[i]==roles[j]{j+=1;}
        prefix[i]=j;
    }
    roles.len()-prefix[roles.len()-1]
}
fn exact_role_period(len:usize,prefix:usize)->usize{
    if prefix==0||len%prefix==0{prefix}else{len}
}
fn observed_segment(events:&[Event],ranges:&[(usize,usize)],start:usize,end:usize)->Value{
    let first=ranges[start].0;
    let last=ranges[end-1].1-1;
    json!({"normalizedStart":start,"normalizedEnd":end,
        "source":{"start":events[first].position.0,"end":events[last].position.1},
        "startLine":events[first].line,"endLine":events[last].line})
}
// Exact local repetition between successive name anchors. Compare
// adjacent blocks only, avoiding a cross-product of all possible starts.
fn local_recipient_runs(roles:&[&str],events:&[Event],ranges:&[(usize,usize)])->Vec<Value>{
    let mut anchors=roles.iter().enumerate().filter_map(|(i,r)|(*r=="name").then_some(i)).collect::<Vec<_>>();
    anchors.push(roles.len());
    let mut result=Vec::new();
    let mut i=0;
    while i+2<anchors.len(){
        let start=anchors[i];let next=anchors[i+1];
        let pattern=&roles[start..next];
        if !(pattern.contains(&"address")||pattern.contains(&"contact")){i+=1;continue;}
        let mut j=i+1;
        while j+1<anchors.len()&&roles[anchors[j]..anchors[j+1]]==*pattern{j+=1;}
        if j>i+1{
            result.push(json!({"kind":"name_anchored_exact_run","cycle":pattern,"repeatCount":j-i,
                "segment":observed_segment(events,ranges,start,anchors[j])}));
            i=j;
        }else{i+=1;}
    }
    result
}
// Bounded partial order: each adjacent pair and each pair separated by
// one observed role. Keep the occurrence counts out of pattern identity.
fn partial_role_order(roles:&[&str])->Vec<Value>{
    let mut counts=BTreeMap::<(&str,&str,usize),usize>::new();
    for (i,&from) in roles.iter().enumerate(){
        for step in 1..=2{
            if let Some(&to)=roles.get(i+step){*counts.entry((from,to,step)).or_default()+=1;}
        }
    }
    counts.into_iter().map(|((from,to,step),count)|
        json!({"from":from,"to":to,"roleDistance":step,"count":count})).collect()
}
// Detect one missing or extra role, or one adjacent swap, without a
// quadratic edit-distance matrix. Identical blocks use the exact path.
fn near_block_relation(a:&[&str],b:&[&str])->Option<&'static str>{
    if a==b{return None;}
    let prefix=a.iter().zip(b).take_while(|(x,y)|x==y).count();
    if a.len()==b.len()+1&&prefix<a.len()&&a[prefix+1..]==b[prefix..]{
        return Some("later_block_missing_role");
    }
    if b.len()==a.len()+1&&prefix<b.len()&&b[prefix+1..]==a[prefix..]{
        return Some("later_block_extra_role");
    }
    if a.len()==b.len()&&prefix+1<a.len()&&a[prefix]==b[prefix+1]&&
        a[prefix+1]==b[prefix]&&a[prefix+2..]==b[prefix+2..]{
        return Some("adjacent_role_swap");
    }
    None
}
fn near_recipient_blocks(roles:&[&str],events:&[Event],ranges:&[(usize,usize)])->Vec<Value>{
    let mut anchors=roles.iter().enumerate().filter_map(|(i,r)|(*r=="name").then_some(i)).collect::<Vec<_>>();
    anchors.push(roles.len());
    let mut out=Vec::new();
    for w in anchors.windows(3){
        let (a,b,c)=(w[0],w[1],w[2]);
        let first=&roles[a..b];let next=&roles[b..c];
        // Require delivery evidence in both blocks. An isolated name
        // is not a verified partial recipient structure.
        let has_delivery=|r:&[&str]|r.contains(&"address")||r.contains(&"contact");
        if !has_delivery(first)||!has_delivery(next){continue;}
        if let Some(relation)=near_block_relation(first,next){
            out.push(json!({"relation":relation,"firstRoles":first,"nextRoles":next,
                "first":observed_segment(events,ranges,a,b),
                "next":observed_segment(events,ranges,b,c),"identityMerged":false}));
        }
    }
    out
}
// One local feature projection used by both owner and field contexts.
// Preserve v1 byte-for-byte order, magnitude and same-paragraph semantics.
fn local_context(events:&[Event],i:usize)->Vec<Value>{
    let e=&events[i];
    events[i.saturating_sub(2)..(i+3).min(events.len())].iter().map(|n|{
        let relation=super::SourceRelation::between(e.line,e.paragraph,n.line,n.paragraph);
        json!([n.role,n.position==e.position,distance_bucket(relation.distance()),relation.same_paragraph])
    }).collect()
}

fn document_pattern(events:&[Event],first:&Value)->Value{
    let unavailable=|reason:&str|json!({"patternVersion":DOCUMENT_PATTERN_VERSION,
        "status":reason,"candidateKey":null,"learningEligible":false});
    if events.is_empty(){return unavailable("no_observed_roles");}
    if events.len()>DOCUMENT_PATTERN_MAX_EVENTS{return unavailable("event_limit_exceeded");}
    // Missing, unknown, duplicate or overlapping candidate roles cannot
    // be silently resolved into a supposedly certain pattern.
    if first["candidates"].as_array().is_none_or(|v|v.len()!=events.len())||
        events.windows(2).any(|w|w[0].position.1>w[1].position.0||
            w[0].line>w[1].line||w[0].paragraph>w[1].paragraph){
        return unavailable("ambiguous_or_incomplete_candidates");
    }
    let observed=events.iter().map(|e|e.role).collect::<Vec<_>>();
    let mut normalized=Vec::new();
    let mut ranges:Vec<(usize,usize)>=Vec::new();
    let mut i=0;
    while i<events.len(){
        let e=&events[i];
        if e.role=="note"{i+=1;continue;}
        // A contiguous run of product/quantity pairs is a single item
        // section. Preserve whether quantity appears before or after a
        // product; do not guess a link from an orphan quantity.
        if e.role=="product"{
            normalized.push("product_section");
            let paragraph=e.paragraph;
            let start=i;
            i+=1;
            while i<events.len()&&events[i].paragraph==paragraph&&
                matches!(events[i].role,"product"|"quantity"){i+=1;}
            ranges.push((start,i));
            continue;
        }
        if e.role=="contact"&&i>0&&events[i-1].role=="contact"&&
            events[i-1].paragraph==e.paragraph{
            if let Some(last)=ranges.last_mut(){last.1=i+1;}
            i+=1;continue;
        }
        normalized.push(e.role);
        ranges.push((i,i+1));
        i+=1;
    }
    if normalized.is_empty(){return unavailable("no_structural_roles");}
    let prefix=prefix_role_period(&normalized);
    let p=exact_role_period(normalized.len(),prefix);
    let cycle=&normalized[..p];
    // A repeated field alone is not evidence of repeated recipient blocks.
    let recipient_cycle=cycle.contains(&"name")&&
        (cycle.contains(&"address")||cycle.contains(&"contact"));
    let cycle=if recipient_cycle{cycle}else{normalized.as_slice()};
    let repeats=normalized.len()/cycle.len();
    let candidate_key=json!([DOCUMENT_PATTERN_VERSION,"document_role_cycle",cycle]).to_string();
    let repeat_segments=if repeats>1{(0..repeats).map(|i|
        observed_segment(events,&ranges,i*cycle.len(),(i+1)*cycle.len())).collect::<Vec<_>>()
    }else{Vec::new()};
    // Incomplete prefix repetitions remain separate evidence and must
    // never inherit the exact-cycle identity automatically.
    let incomplete=if prefix<normalized.len()&&normalized.len()%prefix!=0&&
        normalized.len()/prefix>=1&&normalized[..prefix].contains(&"name")&&
        (normalized[..prefix].contains(&"contact")||normalized[..prefix].contains(&"address")){
        let count=normalized.len()/prefix;
        json!({"cycle":&normalized[..prefix],"completeCycles":count,
            "tailRoles":&normalized[count*prefix..],
            "tail":observed_segment(events,&ranges,count*prefix,normalized.len())})
    }else{Value::Null};
    let runs=local_recipient_runs(&normalized,events,&ranges);
    let near=near_recipient_blocks(&normalized,events,&ranges);
    let partial=partial_role_order(&normalized);
    // Preserve item role order, multiplicity and source boundaries outside
    // the provisional normalized identity. This is not an ownership link.
    let item_sections=normalized.iter().enumerate().filter(|(_,role)|**role=="product_section")
        .map(|(i,_)|{let (a,b)=ranges[i];
            json!({"segment":observed_segment(events,&ranges,i,i+1),
                "observedRoles":events[a..b].iter().map(|e|e.role).collect::<Vec<_>>()})
        }).collect::<Vec<_>>();
    let optional_notes=events.iter().filter(|e|e.role=="note").map(|e|
        json!({"start":e.position.0,"end":e.position.1,"line":e.line,"paragraph":e.paragraph})).collect::<Vec<_>>();
    json!({"patternVersion":DOCUMENT_PATTERN_VERSION,"status":"candidate_only",
        "candidateKey":candidate_key,"observedRoleSequence":observed,
        "normalizedRoleSequence":normalized,"cycle":cycle,"exactRepeatCount":repeats,
        "exactRepeatSegments":repeat_segments,"incompletePrefixRepeat":incomplete,
        "localRecipientRuns":runs,"nearRecipientBlocks":near,"partialRoleOrder":partial,
        "itemSections":item_sections,"optionalNotes":optional_notes,
        "recipientCycleObserved":recipient_cycle,"learningEligible":false})
}

// An observed role key is usable for controlled evaluation, not a
// certificate of complete extraction. Bind line/paragraph metadata back
// to the original document. Keep the eventual training key unavailable
// until candidate coverage and section normalization are proven.
fn pattern_identity(raw:&str,units:&[u16],events:&[Event],first:&Value,pattern:&Value)->Value{
    let mut blockers=Vec::new();
    if pattern["candidateKey"].as_str().is_none(){blockers.push("ambiguous_or_missing_structure");}
    let lines=super::physical_lines(raw);
    let mut starts=Vec::with_capacity(lines.len());
    let (mut last_byte,mut last_unit)=(0usize,0usize);
    for line in &lines{
        last_unit+=raw[last_byte..line.start].encode_utf16().count();
        last_byte=line.start;
        starts.push((last_unit,last_unit+line.text.encode_utf16().count(),line.number as u64,line.paragraph as u64));
    }
    let verified_geometry=events.iter().all(|e|{
        let index=starts.partition_point(|l|l.0<=e.position.0);
        index>0&&{
            let line=starts[index-1];
            e.position.0<line.1&&e.line==line.2&&e.paragraph==line.3
        }
    });
    if !verified_geometry{blockers.push("source_geometry_mismatch");}
    let inventory=&first["classificationCandidateTrace"]["rawCandidateInventory"];
    if inventory["version"]!=1{blockers.push("missing_raw_candidate_inventory");}
    else{
        if inventory["complete"]!=true||first["classificationCandidateTrace"]["fullCandidateCoverage"]!=true{
            blockers.push("incomplete_candidate_coverage");
        }
        if inventory["truncated"]!=false||inventory["invalidSourceCount"]!=0{
            blockers.push("invalid_or_truncated_inventory");
        }
        let mut retained=BTreeSet::new();let mut common=false;
        for record in rows(&inventory["records"]){
            let Some(r)=role(record["kind"].as_str().unwrap_or(""))else{continue;};
            if rows(&record["origins"]).any(|o|o["stage"]=="native_extraction"&&o["status"]=="retained"){
                if let Some(s)=span(units,&record["source"]){retained.insert((s,r));}
                else if !blockers.contains(&"invalid_native_span"){blockers.push("invalid_native_span");}
                common|=rows(&record["origins"]).any(|o|o["section"]=="common_product");
            }
        }
        let observed=events.iter().map(|e|(e.position,e.role)).collect::<BTreeSet<_>>();
        if retained!=observed{blockers.push("native_and_final_candidate_mismatch");}
        if common{blockers.push("common_section_identity_not_validated");}
    }
    // Never treat a caller-supplied completeness flag as a training
    // authorization. The versioned evaluation key contains only roles.
    json!({"identityVersion":DOCUMENT_PATTERN_VERSION,
        "evaluationKey":if verified_geometry{pattern["candidateKey"].clone()}else{Value::Null},
        "scope":"observed_structure_only","trainingKey":null,
        "learningEligible":false,"blockers":blockers})
}

// Diagnostic feature projection only; do not change the v1 context key
// or duplicate all candidate pair combinations. Exact source geometry is
// available for adjacent events, including within one physical line.
fn source_pair_features(units:&[u16],geometry:&super::SourceGeometry,from:&Event,to:&Event)->Value{
    let (earlier,later,sign)=if from.position<=to.position{(from,to,1i64)}else{(to,from,-1i64)};
    let source_gap=if earlier.position.1<=later.position.0{
        let gap=&units[earlier.position.1..later.position.0];
        let scalars=gap.iter().filter(|u|!(0xDC00u16..=0xDFFFu16).contains(*u)).count();
        json!({"utf16":sign*(gap.len() as i64),"unicodeScalars":sign*(scalars as i64)})
    }else{Value::Null};
    let gap=geometry.between(earlier.line as usize,later.line as usize);
    let relation=super::SourceRelation::between(from.line,from.paragraph,to.line,to.paragraph);
    json!({"from":{"start":from.position.0,"end":from.position.1,"role":from.role},
        "to":{"start":to.position.0,"end":to.position.1,"role":to.role},
        "lineDelta":relation.line_delta,
        "sourceGap":source_gap,"sameParagraph":relation.same_paragraph,
        "blankLines":gap.blank_lines,"contentLines":gap.content_lines,
        "maxConsecutiveBlankLines":gap.max_consecutive_blank_lines})
}
fn document_geometry(raw:&str,units:&[u16],events:&[Event],pattern:&Value)->Value{
    if pattern["status"]!="candidate_only"{
        return json!({"status":"unavailable","adjacentPairs":[]});
    }
    let newlines=units.iter().enumerate().filter_map(|(i,u)|(*u==10).then_some(i)).collect::<Vec<_>>();
    if events.iter().any(|e|newlines.partition_point(|i|*i<e.position.0) as u64!=e.line){
        return json!({"status":"source_line_mismatch","adjacentPairs":[]});
    }
    let geometry=super::SourceGeometry::new(raw);
    let pairs=events.windows(2).map(|w|source_pair_features(units,&geometry,&w[0],&w[1])).collect::<Vec<_>>();
    json!({"status":"diagnostic_only","adjacentPairs":pairs})
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
    let events=event_index(units,first);
    owner_contexts_from_events(units,first,&events,document_lead(&events),&mut BTreeMap::new())
}
type LocalContextCache=BTreeMap<usize,Vec<Value>>;
fn owner_contexts_from_events(units:&[u16],first:&Value,events:&[Event],lead:&str,
    locals:&mut LocalContextCache)->BTreeMap<Span,Vec<Value>>{
    let mut out=BTreeMap::new();
    let trace=&first["recipientOwnerTrace"];
    if trace["schemaVersion"]!=1{return out;}
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
        let local=locals.entry(i).or_insert_with(||local_context(events,i));
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

// Field contexts use the same versioned hierarchy as ownership, but have
// distinct task keys. No final value or literal source text enters a key.
// A shape is deliberately coarse and is not a token/alias match.
fn source_shape(units:&[u16],s:Span)->Option<Value>{
    let text=String::from_utf16(&units[s.0..s.1]).ok()?;
    let mut digits=0u64;
    let mut letters=0u64;
    let mut hangul=0u64;
    let mut separators=0u64;
    let mut other=0u64;
    for c in text.chars(){
        if c.is_ascii_digit(){digits+=1;}
        else if c.is_ascii_alphabetic(){letters+=1;}
        else if ('가'..='힣').contains(&c)||('ㄱ'..='ㅎ').contains(&c)||('ㅏ'..='ㅣ').contains(&c){hangul+=1;}
        else if c.is_whitespace()||matches!(c,'-'|'/'|'('|')'|'_'|'.'|','){separators+=1;}
        else{other+=1;}
    }
    Some(json!([count_bucket(digits),count_bucket(letters),count_bucket(hangul),
        count_bucket(separators),count_bucket(other)]))
}
fn field_key(task:&str,level:&str,features:&Value,client:Option<&str>)->Value{
    let encoded=json!([FEATURE_VERSION,task,level,client,features]).to_string();
    json!({"featureVersion":FEATURE_VERSION,"level":level,"key":encoded})
}
type FieldContexts=BTreeMap<(Span,&'static str),Vec<Value>>;
fn field_contexts(units:&[u16],first:&Value)->FieldContexts{
    let events=event_index(units,first);
    field_contexts_from_events(units,first,&events,document_lead(&events),&mut BTreeMap::new())
}
fn field_contexts_from_events(units:&[u16],first:&Value,events:&[Event],lead:&str,
    locals:&mut LocalContextCache)->FieldContexts{
    let client=first["recognitionContext"]["clientId"].as_str()
        .filter(|id|!id.trim().is_empty()&&first["recognitionContext"]["clientBasis"]=="MANUAL");
    let mut items=BTreeMap::<Span,Vec<(&Value,&str)>>::new();
    for item in rows(&first["recipients"]).flat_map(|r|rows(&r["items"])){
        for source in rows(&item["sources"]){
            if let (Some(s),Some(field))=(span(units,source),source["field"].as_str()){
                items.entry(s).or_default().push((item,field));
            }
        }
    }
    let mut line_roles=BTreeMap::<u64,Vec<&str>>::new();
    for e in events{
        let line=line_roles.entry(e.line).or_default();
        if line.len()<17{line.push(e.role);}
    }
    let mut result=FieldContexts::new();
    for (i,e) in events.iter().enumerate(){
        let s=e.position;
        if i>0&&events[i-1].position==s || i+1<events.len()&&events[i+1].position==s{continue;}
        let Some(shape)=source_shape(units,s)else{continue;};
        let local=locals.entry(i).or_insert_with(||local_context(events,i));
        let line_order=line_roles.get(&e.line).filter(|v|v.len()<=16);
        let mut tasks=vec![("type",json!([e.role,shape]))];
        if let Some(links)=items.get(&s).filter(|v|v.len()==1){
            let (item,field)=links[0];
            let product=item["productId"].as_str().filter(|id|!id.trim().is_empty());
            match field{
                "product" if e.role=="product"=>{
                    if let Some(product)=product{
                        // Baseline product ID scopes similar wrong selections.
                        // A variant is always scoped to its original product.
                        tasks.push(("product",json!([shape,product])));
                        if let Some(variant)=item["variantId"].as_str().filter(|id|!id.trim().is_empty()){
                            tasks.push(("variant",json!([shape,product,variant])));
                        }
                    }
                }
                "quantity" if e.role=="quantity"=>{
                    // Quantity observations are diagnostics, not a rule to
                    // substitute a frequently ordered quantity for source text.
                    if item["quantityConfirmed"]==true&&item["quantity"].as_i64().is_some_and(|q|q>0){
                        tasks.push(("quantity",json!([shape,product])));
                    }
                }
                _=>{}
            }
        }
        for (task,base) in tasks{
            let structure=json!([base,lead,local,line_order]);
            let mut keys=vec![field_key(task,"base",&base,None),field_key(task,"structure",&structure,None)];
            if let Some(client)=client{keys.push(field_key(task,"client_structure",&structure,Some(client)));}
            result.insert((s,task),keys);
        }
    }
    result
}

// Only candidates already present in the original analysis. A selected
// value alone is not evidence that the full competing set was retained.
struct ShadowIndex<'a>{
    links:BTreeMap<Span,Vec<(&'a Value,&'a Value)>>,
    initial_types:BTreeMap<Span,Vec<&'a str>>,
    items:BTreeMap<&'a str,Vec<(usize,&'a Value)>>,
    products:BTreeMap<(u64,u64),Vec<(usize,&'a Value,Span)>>,
    gates:BTreeMap<Span,Vec<(usize,&'a Value)>>,
    traced:bool,
}
impl<'a> ShadowIndex<'a>{
    fn new(units:&[u16],first:&'a Value)->Self{
        let trace=&first["classificationCandidateTrace"];
        let mut out=Self{links:BTreeMap::new(),initial_types:BTreeMap::new(),items:BTreeMap::new(),products:BTreeMap::new(),
            gates:BTreeMap::new(),traced:trace["schemaVersion"]==1};
        for candidate in rows(&first["candidates"]){
            if let Some(s)=span(units,candidate){
                out.initial_types.entry(s).or_default().push(candidate["kind"].as_str().unwrap_or(""));
            }
        }
        for row in rows(&first["recipients"]){
            for source in rows(&row["sources"]){
                if let Some(s)=span(units,source){out.links.entry(s).or_default().push((row,source));}
            }
            for item in rows(&row["items"]){
                for source in rows(&item["sources"]){
                    if let Some(s)=span(units,source){out.links.entry(s).or_default().push((item,source));}
                }
            }
        }
        if out.traced{
            for (i,d) in rows(&trace["itemDecisions"]).enumerate(){
                if let Some(id)=d["itemId"].as_str().filter(|id|!id.is_empty()){
                    out.items.entry(id).or_default().push((i,d));
                }
            }
            for (i,d) in rows(&trace["decisions"]).enumerate(){
                if let (Some(line),Some(start),Some(s))=(d["line"].as_u64(),d["source"]["start"].as_u64(),position(units,&d["source"])){
                    if d["productStages"].is_array(){out.products.entry((line,start)).or_default().push((i,d,s));}
                }
            }
            for (i,g) in rows(&trace["typeGateRecords"]).enumerate(){
                if let Some(s)=position(units,&g["source"]){out.gates.entry(s).or_default().push((i,g));}
            }
        }
        out
    }
}
#[cfg(test)]
fn field_shadow_projection(units:&[u16],first:&Value,s:Span,task:&str)->Value{
    field_shadow_from_index(units,&ShadowIndex::new(units,first),s,task)
}
// R01: bind the baseline to the unique original candidate AND its exact
// source link. A phone/mobile slot mismatch is not a type correction.
fn type_baseline<'a>(index:&ShadowIndex<'a>,s:Span)->Option<&'a str>{
    let kinds=index.initial_types.get(&s).filter(|v|v.len()==1)?;
    let kind=kinds[0];
    role(kind)?;
    let links=index.links.get(&s).filter(|v|v.len()==1)?;
    (links[0].1["field"].as_str()==Some(kind)).then_some(kind)
}
fn field_shadow_from_index(units:&[u16],index:&ShadowIndex<'_>,s:Span,task:&str)->Value{
    let mut candidates=BTreeSet::new();
    let Some(links)=index.links.get(&s).filter(|l|l.len()==1)else{
        return json!({"candidates":[],"coverage":"ambiguous_source","traceReferences":[]});
    };
    let (item,source)=links[0];
    let field=source["field"].as_str().unwrap_or("");
    fn add(candidates:&mut BTreeSet<String>,id:Option<&str>){
        if let Some(id)=id.filter(|v|!v.trim().is_empty()&&v.len()<=1024){candidates.insert(id.to_owned());}
    }
    match task{
        "type"=>add(&mut candidates,Some(field)),
        "product" if field=="product"=>{
            add(&mut candidates,item["productId"].as_str());
            for candidate in rows(&item["productCandidates"]){add(&mut candidates,candidate["productId"].as_str());}
            for candidate in rows(&item["match"]["productIds"]){add(&mut candidates,candidate.as_str());}
        }
        "variant" if field=="product"=>add(&mut candidates,item["variantId"].as_str()),
        "quantity" if field=="quantity"&&item["quantityConfirmed"]==true=>{
            if let Some(q)=item["quantity"].as_i64().filter(|q|*q>0){candidates.insert(q.to_string());}
        }
        _=>{}
    }
    // A trace is an observation of the old engine, not a learned label.
    let mut references=Vec::new();
    let mut expanded=false;
    if index.traced {
        let item_id=item["id"].as_str().filter(|id|!id.is_empty());
        let mut item_matches=item_id.and_then(|id|index.items.get(id)).into_iter().flatten().copied()
            .filter(|(_,d)|d["productId"]==item["productId"]);
        if let Some((ii,d))=item_matches.next(){
            if item_matches.next().is_none(){
                match task{
                    "product" if field=="product"=>{
                        let start=d["productTraceStart"].as_u64();
                        let line=d["productTraceLine"].as_u64();
                        let mut matches=line.zip(start).and_then(|key|index.products.get(&key))
                            .into_iter().flatten().copied().filter(|(_,_,(a,b))|*a<=s.0&&s.1<=*b);
                        if let Some((di,decision,_))=matches.next(){
                            if matches.next().is_none(){
                                for (si,stage) in rows(&decision["productStages"]).enumerate(){
                                    for candidate in rows(&stage["candidates"]){
                                        add(&mut candidates,candidate["productId"].as_str());
                                    }
                                    references.push(json!({"decisionIndex":di,"stageIndex":si}));
                                }
                                expanded=true;
                            }
                        }
                    }
                    "variant" if field=="product"=>{
                        for (vi,v) in rows(&d["variants"]).enumerate(){
                            add(&mut candidates,v["variantId"].as_str());
                            references.push(json!({"itemDecisionIndex":ii,"variantIndex":vi}));
                        }
                        expanded=true;
                    }
                    "quantity" if field=="quantity"=>{
                        // Different numeric tokens are not alternative values
                        // for this exact source observation.
                        for (qi,q) in rows(&d["quantityCandidates"]).enumerate(){
                            if position(units,q)==Some(s){
                                if let Some(n)=q["value"].as_str().and_then(|v|v.parse::<i64>().ok())
                                    .filter(|n|*n>0){
                                    candidates.insert(n.to_string());
                                    references.push(json!({"itemDecisionIndex":ii,"quantityIndex":qi}));
                                    expanded=true;
                                }
                            }
                        }
                    }
                    _=>{}
                }
            }
        }
        if task=="type"{
            for (gi,g) in index.gates.get(&s).into_iter().flatten().copied(){
                // Only specific actual candidate rejection, never a
                // line-wide skipped branch or an invented alternate type.
                let alternate=match (g["kind"].as_str(),g["reasonCode"].as_str()){
                    (Some("contact"),Some("overlaps_name"|"embedded_digit_run"))=>None,
                    (Some("name"),Some("other_document_field"|"quantity_signal"|"product_signal"|
                        "product_signal_before_delivery"|"insufficient_recipient_context"|
                        "suffix_other_field"|"suffix_without_recipient_context"|"table_other_field"))=>Some("name"),
                    (Some("quantity"),Some("overlaps_recipient_field"))=>Some("quantity"),
                    _=>None
                };
                if let Some(id)=alternate{
                    add(&mut candidates,Some(id));expanded=true;
                }
                references.push(json!({"typeGateIndex":gi}));
            }
        }
    }
    if candidates.len()>64{return json!({"candidates":[],"coverage":"candidate_limit_exceeded","traceReferences":[]});}
    json!({"candidates":candidates.into_iter().collect::<Vec<_>>(),
        "coverage":if expanded{"traced_partial"}else{"observed_only"},
        "traceReferences":references,"traceIsTrainingEvidence":false})
}
#[cfg(test)]
fn field_shadow_candidates(units:&[u16],first:&Value,s:Span,task:&str)->Vec<String>{
    rows(&field_shadow_projection(units,first,s,task)["candidates"])
        .filter_map(Value::as_str).map(str::to_owned).collect()
}

// Independent paragraph support for adjacent role order. Each paragraph
// contributes at most once to a role pair. Exclude the target paragraph.
#[derive(Default)]
struct TypeOrderIndex{
    totals:BTreeMap<(&'static str,&'static str),usize>,
    paragraphs:BTreeSet<(u64,&'static str,&'static str)>,
}
impl TypeOrderIndex{
    fn new(events:&[Event])->Self{
        let mut out=Self::default();
        for w in events.windows(2){
            let (a,b)=(&w[0],&w[1]);
            if a.paragraph!=b.paragraph||a.position.1>b.position.0{continue;}
            if out.paragraphs.insert((a.paragraph,a.role,b.role)){
                *out.totals.entry((a.role,b.role)).or_default()+=1;
            }
        }
        out
    }
    fn support(&self,p:u64,a:&'static str,b:&'static str)->usize{
        self.totals.get(&(a,b)).copied().unwrap_or(0)
            .saturating_sub(if self.paragraphs.contains(&(p,a,b)){1}else{0})
    }
}
fn compare_types(events:&[Event],order:&TypeOrderIndex,index:&ShadowIndex<'_>,s:Span,shadow:&Value)->Value{
    let unavailable=|status:&str|json!({"status":status,"preferredCandidate":null,
        "decisionChanged":false,"eligibleForAutomaticDecision":false});
    let Some(baseline)=type_baseline(index,s)else{return unavailable("unbound_baseline");};
    let Ok(i)=events.binary_search_by_key(&s,|e|e.position)else{return unavailable("unbound_source");};
    let e=&events[i];
    if i>0&&events[i-1].position.1>s.0||i+1<events.len()&&events[i+1].position.0<s.1{
        return unavailable("overlapping_source");
    }
    let previous=i.checked_sub(1).and_then(|j|events.get(j)).filter(|p|p.paragraph==e.paragraph);
    let next=events.get(i+1).filter(|n|n.paragraph==e.paragraph);
    let mut candidates=Vec::new();
    let mut scores=Vec::new();
    for candidate in rows(&shadow["candidates"]).filter_map(Value::as_str){
        let Some(r)=role(candidate)else{continue;};
        // Existing explicit overlap rejection is not a soft score.
        let blocked=candidate=="quantity"&&index.gates.get(&s).into_iter().flatten().any(|(_,g)|
            g["kind"]=="quantity"&&g["reasonCode"]=="overlaps_recipient_field");
        let left=previous.map_or(0,|p|order.support(e.paragraph,p.role,r));
        let right=next.map_or(0,|n|order.support(e.paragraph,r,n.role));
        let features=[left.min(8) as u8,right.min(8) as u8,0];
        let score=left.min(8)+right.min(8);
        scores.push(super::FieldWeightCandidate{id:candidate,features,blocked});
        candidates.push(json!({"candidate":candidate,"id":candidate,"score":(!blocked).then_some(score),
            "features":features,"blocked":blocked,
            "precedingOrderSupport":left,"followingOrderSupport":right,
            "blockedBySourceOverlap":blocked}));
    }
    let preferred=if scores.iter().filter(|c|!c.blocked).count()>=2{
        super::field_weight_choice(&scores,[1,1,0],2)
    }else{None};
    json!({"status":"provisional_comparison","policyVersion":"type-order-candidate-v1",
        "baselineCandidate":baseline,"preferredCandidate":preferred,"candidates":candidates,
        "decisionChanged":false,"eligibleForAutomaticDecision":false})
}

/// Explicit diagnostic/lookup projection, not a second parser. The same
/// context key function is used for first/final observations and new traces.
pub(super) fn lookup(payload:&Value)->Result<Value,String>{
    let raw=payload["raw"].as_str().ok_or("문맥 조회 원문이 없습니다.")?;
    let first=&payload["initialAnalysis"];
    let units=raw.encode_utf16().collect::<Vec<_>>();
    let events=event_index(&units,first);
    let lead=document_lead(&events);
    let index=ShadowIndex::new(&units,first);
    let mut locals=LocalContextCache::new();
    let type_order=(payload["compareTypePolicy"]==true&&events.len()<=DOCUMENT_PATTERN_MAX_EVENTS)
        .then(||TypeOrderIndex::new(&events));
    let entries=owner_contexts_from_events(&units,first,&events,lead,&mut locals).into_iter().map(|(s,keys)|
        json!({"source":{"start":s.0,"end":s.1},"contexts":keys})
    ).collect::<Vec<_>>();
    let field_entries=field_contexts_from_events(&units,first,&events,lead,&mut locals).into_iter().map(|((s,task),keys)|{
        let shadow=field_shadow_from_index(&units,&index,s,task);
        let mut entry=json!({"source":{"start":s.0,"end":s.1},"task":task,"contexts":keys,
            "shadowCandidates":shadow["candidates"],"candidateCoverage":shadow["coverage"],
            "traceReferences":shadow["traceReferences"],"traceIsTrainingEvidence":false});
        if task=="type"{
            entry["legacyTypeCandidate"]=json!(type_baseline(&index,s));
            if let Some(order)=type_order.as_ref(){
                entry["candidatePolicyComparison"]=compare_types(&events,order,&index,s,&shadow);
            }
        }
        entry
    }).collect::<Vec<_>>();
    let pattern=document_pattern(&events,first);
    let identity=pattern_identity(raw,&units,&events,first,&pattern);
    let mut result=json!({"featureVersion":FEATURE_VERSION,"entries":entries,"fieldEntries":field_entries,
        "documentPattern":pattern,"patternIdentity":identity});
    if payload["includeDocumentFeatures"]==true{
        result["documentFeatures"]=document_geometry(raw,&units,&events,&result["documentPattern"]);
        let order=OwnerOrderEvidence::new(events.iter().map(|e|(if e.role=="contact"{"phone"}else{e.role},
            e.paragraph,e.position.0,e.position.1)));
        result["documentFeatures"]["ownerOrderEvidence"]=json!({
            "addressBeforeAfter":order.votes("address",[u64::MAX;3]),
            "contactBeforeAfter":order.votes("phone",[u64::MAX;3]),
            "productBeforeAfter":order.votes("product",[u64::MAX;3]),
            "status":"diagnostic_only"});
    }
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
        let field_counts=rows(&summary["fieldContextStatistics"]).filter_map(|record|
            Some((record["key"].as_str()?,&record["counts"]))).collect::<BTreeMap<_,_>>();
        let field_assessments=rows(&result["fieldEntries"]).map(|entry|{
            let levels=rows(&entry["contexts"]).map(|context|{
                let counts=context["key"].as_str().and_then(|k|field_counts.get(k).copied());
                json!({"level":context["level"],"counts":counts})
            }).collect::<Vec<_>>();
            json!({"source":entry["source"],"task":entry["task"],"levels":levels,
                "eligibleForAutomaticDecision":false,"decisionChanged":false})
        }).collect::<Vec<_>>();
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
            "assessments":assessments,"fieldAssessments":field_assessments
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
        "pendingCorrections":counts.and_then(|c|c["pendingCorrectionCount"].as_u64()).unwrap_or(0),
        "explicitCorrectionOrders":counts.and_then(|c|c["explicitCorrectionOrders"].as_u64()).unwrap_or(0),
        "explicitPositiveCandidateOrders":counts.map(|c|c["explicitPositiveCandidateOrders"].clone()),
        "explicitNegativeCandidateOrders":counts.map(|c|c["explicitNegativeCandidateOrders"].clone())
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
    explicit_previous:u64,
    explicit_next:u64,
    explicit_orders:u64,
    explicit_previous_orders:u64,
    explicit_next_orders:u64,
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
        }else if o["outcome"]=="explicit_correction_observed" && o["eligibleForCandidateLearning"]==true{
            match (o["negativeCandidates"].as_array().filter(|v|v.len()==1).and_then(|v|v[0].as_str()),
                o["positiveCandidates"].as_array().filter(|v|v.len()==1).and_then(|v|v[0].as_str())){
                (Some("next"),Some("previous"))=>self.explicit_previous+=1,
                (Some("previous"),Some("next"))=>self.explicit_next+=1,
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
        self.explicit_previous+=other.explicit_previous;
        self.explicit_next+=other.explicit_next;
        self.explicit_orders+=u64::from(other.explicit_previous+other.explicit_next>0);
        self.explicit_previous_orders+=u64::from(other.explicit_previous>0);
        self.explicit_next_orders+=u64::from(other.explicit_next>0);
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
            "explicitCorrectionCount":self.explicit_previous+self.explicit_next,
            "explicitCorrectionOrders":self.explicit_orders,
            "explicitPositiveCandidates":{"previous":self.explicit_previous,"next":self.explicit_next},
            "explicitNegativeCandidates":{"previous":self.explicit_next,"next":self.explicit_previous},
            "explicitPositiveCandidateOrders":{"previous":self.explicit_previous_orders,"next":self.explicit_next_orders},
            "explicitNegativeCandidateOrders":{"previous":self.explicit_next_orders,"next":self.explicit_previous_orders},
            "pendingCorrectionCount":self.pending,"unresolvedCount":self.unresolved,
            "eligibleForAutomaticDecision":false
        })
    }
}

// Field candidate IDs are categorical, unlike previous/next ownership.
// Keep task and evidence channels separate; never turn aggregate frequency
// into a source-specific probability.
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

/// Per-order contributions, also usable by the future durable delta writer.
/// Group before counting independent orders; no sum of per-candidate order
/// counts can substitute for the distinct context denominator.
fn contributions(order:&Value,filter:&BTreeSet<&str>,prepared:Option<&Value>)->Result<Option<(BTreeMap<String,Counts>,BTreeMap<String,FieldCounts>,BTreeMap<String,FieldCounts>,String)>,String>{
    let confirmed=order["confirmedAt"].as_str().filter(|v|!v.trim().is_empty());
    let snapshot=&order["confirmationSnapshot"];
    if confirmed.is_none() || snapshot["at"].as_str()!=confirmed ||
        order["deletedAt"].as_str().is_some_and(|v|!v.trim().is_empty()){
        return Ok(None);
    }
    let owned;
    let compared=match prepared{Some(v)=>v,None=>{owned=super::observations::compare(order)?;&owned}};
    if compared["status"]!="compared"{return Ok(None);}
    // Unverified orders remain visible through coverage counts and compare(),
    // but cannot inflate candidate counts OR independent-order denominators.
    let coverage=compared["auditCoverage"].as_str().unwrap_or("").to_owned();
    if coverage!="verified_manifest"{
        return Ok(Some((BTreeMap::new(),BTreeMap::new(),BTreeMap::new(),coverage)));
    }
    let first_client=compared["initialRecognition"]["clientId"].as_str();
    let final_client=snapshot["data"]["clientId"].as_str();
    // A confirmed client context is available only if the exact manually
    // selected initial client survived confirmation. Do not substitute a
    // later client into the historical features.
    let client_valid=first_client.is_some() && first_client==final_client
        && compared["finalClientId"].as_str()==final_client
        && compared["finalClientSelection"]=="MANUAL"
        && compared["initialRecognition"]["clientBasis"]=="MANUAL"
        && order["recognitionAuditEvents"].as_array().is_some_and(|events|
            events.iter().filter(|e|e["sourceRaw"]==snapshot["raw"]&&
                e["initialAnalysis"]==snapshot["recognitionBaseline"]["initialAnalysis"])
                .all(|e|e["context"]["clientId"].as_str()==final_client&&
                    e["context"]["clientSelection"]=="MANUAL"));
    let mut groups=BTreeMap::<String,Counts>::new();
    for o in rows(&compared["observations"]).filter(|o|o["task"]=="recipient_ownership"){
        for context in rows(&o["contexts"]){
            if context["level"]=="client_structure"{
                if !client_valid{continue;}
                if o["outcome"]=="explicit_correction_observed" &&
                    (o["auditClientId"].as_str()!=final_client || o["auditClientSelection"]!="MANUAL"){continue;}
            }
            let Some(key)=context["key"].as_str()else{continue;};
            if !filter.is_empty()&&!filter.contains(key){continue;}
            groups.entry(key.to_owned()).or_default().add_observation(o);
        }
    }
    let mut fields=BTreeMap::<String,FieldCounts>::new();
    let mut field_groups=BTreeMap::<String,FieldCounts>::new();
    let first=&snapshot["recognitionBaseline"]["initialAnalysis"];
    let units=snapshot["raw"].as_str().unwrap_or("").encode_utf16().collect::<Vec<_>>();
    let contexts=field_contexts(&units,first);
    for o in rows(&compared["observations"]){
        let Some(task)=o["task"].as_str().filter(|t|matches!(*t,"type"|"product"|"variant"|"quantity"))else{continue;};
        if filter.is_empty(){fields.entry(task.to_owned()).or_default().add_observation(o);}
        let Some(s)=position(&units,&o["source"])else{continue;};
        let Some(keys)=contexts.get(&(s,task))else{continue;};
        for context in keys{
            if context["level"]=="client_structure"{
                if !client_valid{continue;}
                if o["outcome"]=="explicit_correction_observed" &&
                    (o["auditClientId"].as_str()!=final_client||o["auditClientSelection"]!="MANUAL"){continue;}
            }
            let Some(key)=context["key"].as_str()else{continue;};
            if !filter.is_empty()&&!filter.contains(key){continue;}
            field_groups.entry(key.to_owned()).or_default().add_observation(o);
        }
    }
    Ok(Some((groups,fields,field_groups,coverage)))
}

/// Explicit bounded batch, not an operational history scan. Caller supplies
/// immutable confirmed order records. Duplicate snapshots count once; a
/// conflicting duplicate fails instead of silently choosing a revision.
/// Weak observations and verified explicit corrections remain separate, not calibrated probabilities.
pub(super) fn statistics(payload:&Value)->Result<Value,String>{
    statistics_with_comparison(payload,None)
}
pub(super) fn summarize_confirmation(order:&Value)->Result<Value,String>{
    let compared=super::observations::compare(order)?;
    let mut report=statistics_with_comparison(&json!({"orders":[order]}),Some(&compared))?;
    report["combinedEvaluation"]=super::observations::evaluate_combined_prepared(order,Some(&compared))?;
    Ok(report)
}
fn statistics_with_comparison(payload:&Value,prepared:Option<&Value>)->Result<Value,String>{
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
    let mut fields=BTreeMap::<String,FieldCounts>::new();
    let mut field_groups=BTreeMap::<String,FieldCounts>::new();
    let mut unavailable=0u64;
    let mut without_context=0u64;
    let mut manifest_orders=0u64;
    let mut legacy_orders=0u64;
    let mut incomplete_orders=0u64;
    for order in orders{
        let id=order["id"].as_str().filter(|s|!s.trim().is_empty()).ok_or("확정 주문 ID 누락")?;
        if let Some(old)=seen.get(id){
            if *old!=order{return Err("동일 주문의 상충된 확정 자료입니다.".into());}
            continue;
        }
        seen.insert(id,order);
        match contributions(order,&filter,prepared)?{
            Some((local,field_counts,local_fields,coverage))=>{
                match coverage.as_str(){
                    "verified_manifest"=>manifest_orders+=1,
                    "legacy_unverified"=>legacy_orders+=1,
                    _=>incomplete_orders+=1,
                }
                if coverage=="verified_manifest"&&local.is_empty()&&local_fields.is_empty(){without_context+=1;}
                for (key,counts) in local{groups.entry(key).or_default().add_order(counts);}
                for (task,counts) in field_counts{fields.entry(task).or_default().add_order(counts);}
                for (key,counts) in local_fields{field_groups.entry(key).or_default().add_order(counts);}
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
        "auditCoverage":{"verifiedManifestOrders":manifest_orders,"legacyUnverifiedOrders":legacy_orders,
            "incompleteHistoryOrders":incomplete_orders},
        "eligibleForDurableLearning":false,
        "ordersWithoutMatchingContext":without_context,"statistics":records,
        "fieldStatistics":fields.into_iter().map(|(task,counts)|
            json!({"task":task,"counts":counts.wire()})).collect::<Vec<_>>(),
        "fieldContextStatistics":field_groups.into_iter().map(|(key,counts)|
            json!({"key":key,"counts":counts.wire()})).collect::<Vec<_>>()
    }))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn type_order_support_counts_distinct_other_paragraphs(){
        let events=vec![
            Event{position:(0,1),line:0,paragraph:0,role:"name"},
            Event{position:(1,2),line:0,paragraph:0,role:"contact"},
            Event{position:(2,3),line:0,paragraph:0,role:"name"},
            Event{position:(3,4),line:0,paragraph:0,role:"contact"},
            Event{position:(4,5),line:1,paragraph:1,role:"name"},
            Event{position:(5,6),line:1,paragraph:1,role:"contact"}];
        let index=TypeOrderIndex::new(&events);
        assert_eq!(index.support(0,"name","contact"),1);
        assert_eq!(index.support(1,"name","contact"),1);
        assert_eq!(index.support(2,"name","contact"),2);
        assert_eq!(index.support(2,"contact","product"),0);
    }
    #[test]
    fn owner_order_evidence_excludes_target_and_conflicting_paragraphs(){
        let mut source=Vec::new();
        for p in 0..5u64{
            let start=p as usize*100;
            source.extend([("name",p,start,start+1),("phone",p,start+2,start+3),
                ("address",p,start+4,start+5)]);
        }
        // Two names, overlap, and before/after ambiguity are not independent
        // directional observations.
        source.extend([("name",5,500,501),("name",5,502,503),("phone",5,504,505),
            ("name",6,600,603),("phone",6,602,604),
            ("phone",7,700,701),("name",7,702,703),("mobile",7,704,705)]);
        let index=OwnerOrderEvidence::new(source);
        assert_eq!(index.votes("phone",[0,1,2]),[0,2]);
        assert_eq!(index.votes("mobile",[0,0,1]),[0,3]);
        assert_eq!(index.votes("address",[0,1,2]),[0,2]);
        assert_eq!(index.repeated_shape_count(0,[0,1,2]),2);
        assert_eq!(index.repeated_shape_count(5,[0,1,2]),0);
        assert_eq!(index.votes("product",[0,1,2]),[0,0]);
        let products=OwnerOrderEvidence::new([
            ("product",0,0,1),("name",0,2,3),
            ("name",1,4,5),("product",1,6,7),
            ("name",2,8,9),("product",2,10,11)]);
        assert_eq!(products.votes("product",[0,0,0]),[0,2]);
        assert_eq!(products.votes("product",[1,2,2]),[1,0]);
    }
    #[test]
    fn pattern_identity_rejects_invented_geometry_and_unproven_coverage(){
        let raw="A\nB";
        let mut first=json!({"candidates":[
            {"start":0,"end":1,"text":"A","kind":"name","line":0,"paragraph":0},
            {"start":2,"end":3,"text":"B","kind":"address","line":1,"paragraph":0}]});
        let check=|first:&Value|lookup(&json!({"raw":raw,"initialAnalysis":first})).unwrap()["patternIdentity"].clone();
        let observed=check(&first);
        assert!(observed["evaluationKey"].is_string());
        assert_eq!(observed["trainingKey"],Value::Null);
        assert!(rows(&observed["blockers"]).any(|b|b=="missing_raw_candidate_inventory"));
        first["candidates"][1]["paragraph"]=json!(1);
        let invalid=check(&first);
        assert_eq!(invalid["evaluationKey"],Value::Null);
        assert!(rows(&invalid["blockers"]).any(|b|b=="source_geometry_mismatch"));
    }
    #[test]
    fn document_pattern_candidate_preserves_order_and_ignores_item_count(){
        // Synthetic single-character spans; no customer data.
        let project=|roles:&[&str],gap:usize|{
            let block=format!("x\n{}","#\n".repeat(gap-1));
            let raw=block.repeat(roles.len());
            let candidates=roles.iter().enumerate().map(|(i,kind)|{
                let start=i*block.len();
                json!({"start":start,"end":start+1,"text":"x","kind":kind,"line":i*gap,"paragraph":0})
            }).collect::<Vec<_>>();
            lookup(&json!({"raw":raw,"initialAnalysis":{"candidates":candidates}})).unwrap()["documentPattern"].clone()
        };
        let a=project(&["product","quantity","name","phone","address"],1);
        let b=project(&["product","quantity","product","quantity","name","mobile","phone","address"],3);
        assert_eq!(a["candidateKey"],b["candidateKey"]);
        assert_ne!(a["observedRoleSequence"],b["observedRoleSequence"]);
        assert_eq!(b["itemSections"][0]["observedRoles"],json!(["product","quantity","product","quantity"]));
        let noted=project(&["product","quantity","name","phone","address","note"],1);
        assert_eq!(a["candidateKey"],noted["candidateKey"]);
        assert_eq!(noted["optionalNotes"].as_array().unwrap().len(),1);
        let twice=project(&["product","quantity","name","phone","address",
            "product","quantity","name","phone","address"],1);
        assert_eq!(a["candidateKey"],twice["candidateKey"]);
        assert_eq!(twice["exactRepeatCount"],2);
        assert_eq!(twice["exactRepeatSegments"].as_array().unwrap().len(),2);
        let different=project(&["name","phone","address","product","quantity"],1);
        assert_ne!(a["candidateKey"],different["candidateKey"]);
        let partial=project(&["product","name","phone","address","product","name"],1);
        assert_eq!(partial["exactRepeatCount"],1);
        assert_ne!(a["candidateKey"],partial["candidateKey"]);
        assert_eq!(partial["incompletePrefixRepeat"]["completeCycles"],1);
        let local=project(&["product","name","phone","address","name","phone","address"],1);
        assert_eq!(local["localRecipientRuns"][0]["repeatCount"],2);
        let missing=project(&["name","phone","address","product","name","address","product"],1);
        assert_eq!(missing["nearRecipientBlocks"][0]["relation"],"later_block_missing_role");
        assert_eq!(missing["nearRecipientBlocks"][0]["identityMerged"],false);
        let swap=project(&["name","phone","address","name","address","phone"],1);
        assert_eq!(swap["nearRecipientBlocks"][0]["relation"],"adjacent_role_swap");
        assert_ne!(swap["candidateKey"],local["candidateKey"]);
        assert!(rows(&a["partialRoleOrder"]).any(|p|p["from"]=="name"&&p["to"]=="address"&&p["roleDistance"]==2));
        assert_eq!(a["learningEligible"],false);
        assert_eq!(project(&["product","unknown"],1)["candidateKey"],Value::Null);
        let overlap=json!({"raw":"xx","initialAnalysis":{"candidates":[
            {"start":0,"end":2,"text":"xx","kind":"name","line":0,"paragraph":0},
            {"start":1,"end":2,"text":"x","kind":"phone","line":0,"paragraph":0}]}});
        assert_eq!(lookup(&overlap).unwrap()["documentPattern"]["candidateKey"],Value::Null);
        let roles=["name","phone","name","phone"];
        assert_eq!(exact_role_period(roles.len(),prefix_role_period(&roles)),2);
        let roles=["name","phone","name"];
        assert_eq!(exact_role_period(roles.len(),prefix_role_period(&roles)),3);
    }
    #[test]
    fn shared_local_features_keep_legacy_context_keys_and_cache_once(){
        let raw="A\nT\nB";
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let gap=json!({"blankLines":0,"contentLines":0,"maxConsecutiveBlankLines":0});
        let first=json!({"candidates":[
            {"start":0,"end":1,"text":"A","kind":"name","line":0,"paragraph":0},
            {"start":2,"end":3,"text":"T","kind":"phone","line":1,"paragraph":0},
            {"start":4,"end":5,"text":"B","kind":"name","line":2,"paragraph":0}],
            "recipientOwnerTrace":{"schemaVersion":1,"decisions":[{
                "source":{"start":2,"end":3,"kind":"phone"},
                "previousHasDelivery":false,"nearbyAddress":true,
                "candidates":[
                    {"relation":"previous","anchor":{"start":0,"end":1,"kind":"name"},
                        "sameParagraph":true,"lineDistance":1,"gap":gap},
                    {"relation":"next","anchor":{"start":4,"end":5,"kind":"name"},
                        "sameParagraph":true,"lineDistance":1,"gap":gap}]}]}});
        let events=event_index(&units,&first);
        let lead=document_lead(&events);
        let mut locals=LocalContextCache::new();
        let owner=owner_contexts_from_events(&units,&first,&events,lead,&mut locals);
        assert_eq!(owner.len(),1);
        assert_eq!(locals.len(),1);
        let saved=locals[&1].clone();
        let fields=field_contexts_from_events(&units,&first,&events,lead,&mut locals);
        assert_eq!(fields.len(),3);
        assert_eq!(locals.len(),3);
        assert_eq!(locals[&1],saved);
        assert_eq!(owner,contexts(&units,&first));
        assert_eq!(fields,field_contexts(&units,&first));
        assert_eq!(json!(saved),json!([["name",false,1,true],["contact",true,0,true],["name",false,1,true]]));
    }
    #[test]
    fn document_features_preserve_exact_unicode_distance_and_boundaries(){
        let raw="A😀\n \nB";
        let first=json!({"candidates":[
            {"start":0,"end":1,"text":"A","kind":"name","line":0,"paragraph":0},
            {"start":6,"end":7,"text":"B","kind":"address","line":2,"paragraph":1}]});
        let result=lookup(&json!({"raw":raw,"initialAnalysis":first,"includeDocumentFeatures":true})).unwrap();
        let pair=&result["documentFeatures"]["adjacentPairs"][0];
        assert_eq!(pair["lineDelta"],2);
        assert_eq!(pair["sourceGap"],json!({"utf16":5,"unicodeScalars":4}));
        assert_eq!(pair["blankLines"],1);
        assert_eq!(pair["sameParagraph"],false);
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let events=event_index(&units,&first);
        let reverse=source_pair_features(&units,&super::super::SourceGeometry::new(raw),&events[1],&events[0]);
        assert_eq!(reverse["lineDelta"],-2);
        assert_eq!(reverse["sourceGap"]["utf16"],-5);
        let mut bad=first.clone();bad["candidates"][1]["line"]=json!(3);
        assert_eq!(lookup(&json!({"raw":raw,"initialAnalysis":bad,"includeDocumentFeatures":true})).unwrap()
            ["documentFeatures"]["status"],"source_line_mismatch");
    }
    #[test]
    fn shadow_trace_expansion_uses_only_bound_item_and_exact_source(){
        let units="709 3개".encode_utf16().collect::<Vec<_>>();
        let first=json!({"recipients":[{"items":[{
            "id":"i","productId":"p","variantId":"v","quantity":3,"quantityConfirmed":true,
            "sources":[{"start":0,"end":3,"text":"709","field":"product"},
                {"start":4,"end":6,"text":"3개","field":"quantity"}]}]}],
            "classificationCandidateTrace":{"schemaVersion":1,"fullCandidateCoverage":false,
                "decisions":[{"line":0,"source":{"start":0,"end":6},"productStages":[
                    {"stage":"literal_evidence","candidates":[{"productId":"p"},{"productId":"rejected"}]},
                    {"stage":"final_available","candidates":[{"productId":"p"}]}]},
                    {"line":1,"source":{"start":0,"end":6},"productStages":[
                        {"candidates":[{"productId":"other"}]}]}],
                "itemDecisions":[{"itemId":"i","productId":"p","productTraceLine":0,"productTraceStart":0,
                    "variants":[{"variantId":"v"},{"variantId":"v2","reasonCode":"color_mismatch"}],
                    "quantityCandidates":[{"start":4,"end":6,"value":"3"},
                        {"start":0,"end":3,"value":"709"}]}],
                "typeGateRecords":[{"kind":"name","reasonCode":"product_signal","source":{"start":0,"end":3}},
                    {"kind":"product","reasonCode":"name_field_priority","source":{"start":0,"end":6}}]}});
        let product=field_shadow_projection(&units,&first,(0,3),"product");
        assert_eq!(product["candidates"],json!(["p","rejected"]));
        assert_eq!(product["coverage"],"traced_partial");
        assert_eq!(product["traceIsTrainingEvidence"],false);
        assert_eq!(product["traceReferences"].as_array().unwrap().len(),2);
        assert_eq!(field_shadow_candidates(&units,&first,(0,3),"variant"),vec!["v","v2"]);
        assert_eq!(field_shadow_candidates(&units,&first,(4,6),"quantity"),vec!["3"]);
        assert_eq!(field_shadow_candidates(&units,&first,(0,3),"type"),vec!["name","product"]);
        let index=ShadowIndex::new(&units,&first);
        for (s,task) in [((0,3),"product"),((0,3),"variant"),((4,6),"quantity"),((0,3),"type")]{
            assert_eq!(field_shadow_from_index(&units,&index,s,task),
                field_shadow_projection(&units,&first,s,task));
        }
        assert_eq!(index.products.len(),2);
        assert_eq!(index.links.len(),2);
        let mut bad=first.clone();
        bad["classificationCandidateTrace"]["itemDecisions"][0]["productTraceStart"]=json!(1);
        assert_eq!(field_shadow_candidates(&units,&bad,(0,3),"product"),vec!["p"]);
        let duplicate=bad["classificationCandidateTrace"]["itemDecisions"][0].clone();
        bad["classificationCandidateTrace"]["itemDecisions"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(field_shadow_candidates(&units,&bad,(0,3),"variant"),vec!["v"]);
        // The shared index must retain duplicates as ambiguity, not choose
        // the first hit or overwrite it with the last one.
        let index=ShadowIndex::new(&units,&bad);
        assert_eq!(index.items["i"].len(),2);
        assert_eq!(field_shadow_from_index(&units,&index,(0,3),"variant")["candidates"],json!(["v"]));
    }
    #[test]
    fn field_shadow_candidates_are_original_span_bound_and_never_final_labels(){
        let units="709 3개".encode_utf16().collect::<Vec<_>>();
        let first=json!({"recipients":[{"items":[{
            "productId":"p","variantId":"v","quantity":3,"quantityConfirmed":true,
            "productCandidates":[{"productId":"p"},{"productId":"p2"},{"productId":null}],
            "match":{"productIds":["p3"]},
            "sources":[{"start":0,"end":3,"text":"709","field":"product"},
                {"start":4,"end":6,"text":"3개","field":"quantity"}]}]}],
            "finalCandidates":["never_use"]});
        assert_eq!(field_shadow_candidates(&units,&first,(0,3),"product"),vec!["p","p2","p3"]);
        assert_eq!(field_shadow_candidates(&units,&first,(0,3),"variant"),vec!["v"]);
        assert_eq!(field_shadow_candidates(&units,&first,(0,3),"type"),vec!["product"]);
        assert_eq!(field_shadow_candidates(&units,&first,(4,6),"quantity"),vec!["3"]);
        assert!(field_shadow_candidates(&units,&first,(0,3),"quantity").is_empty());
        let mut changed=first.clone();
        changed["recipients"][0]["items"][0]["quantityConfirmed"]=json!(false);
        assert!(field_shadow_candidates(&units,&changed,(4,6),"quantity").is_empty());
        changed["recipients"][0]["items"][0]["sources"][0]["text"]=json!("708");
        assert!(field_shadow_candidates(&units,&changed,(0,3),"product").is_empty());
        let mut ambiguous=first.clone();
        let duplicate=ambiguous["recipients"][0]["items"][0].clone();
        ambiguous["recipients"][0]["items"].as_array_mut().unwrap().push(duplicate);
        assert!(field_shadow_candidates(&units,&ambiguous,(0,3),"product").is_empty());
    }
    #[test]
    fn type_baseline_requires_unique_original_kind_and_exact_source_link(){
        let units="709".encode_utf16().collect::<Vec<_>>();
        let mut first=json!({"candidates":[{"start":0,"end":3,"text":"709","kind":"product","line":0,"paragraph":0}],
            "recipients":[{"items":[{"sources":[{"start":0,"end":3,"text":"709","field":"product"}]}]}]});
        assert_eq!(type_baseline(&ShadowIndex::new(&units,&first),(0,3)),Some("product"));
        let projected=lookup(&json!({"raw":"709","initialAnalysis":first})).unwrap();
        assert_eq!(projected["fieldEntries"][0]["legacyTypeCandidate"],"product");
        first["candidates"][0]["kind"]=json!("name");
        assert_eq!(type_baseline(&ShadowIndex::new(&units,&first),(0,3)),None);
        first["candidates"][0]["kind"]=json!("product");
        let duplicate=first["candidates"][0].clone();
        first["candidates"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(type_baseline(&ShadowIndex::new(&units,&first),(0,3)),None);
    }
    #[test]
    fn field_contexts_use_initial_shape_task_and_manual_client_only(){
        let first=json!({
            "recognitionContext":{"clientId":"client-a","clientBasis":"MANUAL"},
            "candidates":[
                {"start":0,"end":3,"text":"709","kind":"product","line":0,"paragraph":0},
                {"start":4,"end":6,"text":"3개","kind":"quantity","line":0,"paragraph":0}
            ],
            "recipients":[{"items":[{"productId":"p","variantId":"v","quantity":3,
                "quantityConfirmed":true,"sources":[
                    {"start":0,"end":3,"text":"709","field":"product"},
                    {"start":4,"end":6,"text":"3개","field":"quantity"}
                ]}]}]
        });
        let raw="709 3개";
        let lookup=lookup(&json!({"raw":raw,"initialAnalysis":first})).unwrap();
        let entries=lookup["fieldEntries"].as_array().unwrap();
        assert_eq!(entries.len(),5);
        let keys=entries.iter().flat_map(|e|rows(&e["contexts"]))
            .map(|c|c["key"].as_str().unwrap()).collect::<BTreeSet<_>>();
        assert_eq!(keys.len(),15);
        assert!(keys.iter().all(|k|!k.contains("709")&&!k.contains("3개")));
        let mut automatic=first.clone();
        automatic["recognitionContext"]["clientBasis"]=json!("AUTO");
        let auto=super::lookup(&json!({"raw":raw,"initialAnalysis":automatic})).unwrap();
        assert!(rows(&auto["fieldEntries"]).all(|e|e["contexts"].as_array().unwrap().len()==2));

        let order=json!({"id":"order","confirmedAt":"2026-11-27T00:00:00Z","recognitionAuditEvents":[],
            "confirmationSnapshot":{"at":"2026-11-27T00:00:00Z","raw":raw,
                "data":{"clientId":"client-a"},"recipients":first["recipients"],
                "recognitionBaseline":{"schemaVersion":1,"auditEventIds":[],"initialAnalysis":first,
                    "finalClientId":"client-a","finalClientSelection":"MANUAL"}}});
        let report=statistics(&json!({"orders":[order.clone(),order.clone()]})).unwrap();
        assert_eq!(report["uniqueOrders"],1);
        let records=report["fieldContextStatistics"].as_array().unwrap();
        assert_eq!(records.len(),15);
        assert!(records.iter().all(|r|r["counts"]["independentOrders"]==1));
        let selected=records.iter().find(|r|serde_json::from_str::<Value>(
            r["key"].as_str().unwrap()).is_ok_and(|key|key[1]=="quantity")).unwrap();
        let filtered=statistics(&json!({"orders":[order.clone()],"contextKeys":[selected["key"]]})).unwrap();
        assert_eq!(filtered["fieldContextStatistics"].as_array().unwrap().len(),1);
        assert_eq!(filtered["fieldStatistics"],json!([]));
        let mut changed=order.clone();
        changed["confirmationSnapshot"]["data"]["clientId"]=json!("other");
        let report=statistics(&json!({"orders":[changed]})).unwrap();
        assert_eq!(report["fieldContextStatistics"].as_array().unwrap().len(),10);
        let mut legacy=order.clone();
        legacy["confirmationSnapshot"]["recognitionBaseline"].as_object_mut().unwrap().remove("auditEventIds");
        let report=statistics(&json!({"orders":[legacy]})).unwrap();
        assert_eq!(report["auditCoverage"]["legacyUnverifiedOrders"],1);
        assert_eq!(report["fieldContextStatistics"],json!([]));
        assert_eq!(report["fieldStatistics"],json!([]));
    }
    #[test]
    fn field_evidence_keeps_candidate_polarity_and_distinct_order_counts(){
        let mut local=FieldCounts::default();
        let changed=json!({"outcome":"explicit_correction_observed","eligibleForCandidateLearning":true,
            "negativeCandidates":["old"],"positiveCandidates":["new"]});
        local.add_observation(&changed);
        local.add_observation(&changed);
        local.add_observation(&json!({"outcome":"unchanged_observed","eligibleForCandidateLearning":true,
            "finalCandidates":["new"]}));
        local.add_observation(&json!({"outcome":"changed_requires_review"}));
        let mut aggregate=FieldCounts::default();
        aggregate.add_order(local);
        let c=aggregate.wire();
        assert_eq!(c["explicitPositiveCandidates"]["new"],2);
        assert_eq!(c["explicitNegativeCandidates"]["old"],2);
        assert_eq!(c["explicitPositiveCandidateOrders"]["new"],1);
        assert_eq!(c["weakCandidates"]["new"],1);
        assert_eq!(c["pendingCorrectionCount"],1);
        assert_eq!(c["eligibleForAutomaticDecision"],false);
    }
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
