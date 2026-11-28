//! Shared, candidate-derived document features. No catalog search, final
//! type selection, learned scores or recipient assignment happens here.
use super::{DocumentCandidates,DocumentOccurrence,DocumentSpan,SourceGeometry,InterveningLines};

const NAME:u16=1;
const CONTACT:u16=2;
const ADDRESS:u16=4;
const PRODUCT:u16=8;
const QUANTITY:u16=16;
const NOTE:u16=32;
const ZONE:u16=64;
const OTHER:u16=128;
const CORE:u16=NAME|CONTACT|ADDRESS|PRODUCT|QUANTITY;
const LOCAL_PERIOD_LIMIT:usize=32;

fn role(kind:&str)->u16{
    match kind{
        "name"=>NAME,
        "phone"|"mobile"|"contact_lexical"=>CONTACT,
        "address"|"address_detail"=>ADDRESS,
        "registered"|"alias"|"color"|"semantic"=>PRODUCT,
        "quantity"=>QUANTITY,
        "note"|"dispatch"=>NOTE,
        "sender_zone"|"recipient_zone"|"common_zone"|"record_boundary"=>ZONE,
        _=>OTHER,
    }
}

/// All candidates at one start position remain alternatives; no score
/// or catalog identity is used to select a preferred information type.
#[derive(Debug,PartialEq,Eq)]
pub(super) struct FeatureSlot{
    pub(super) span:DocumentSpan,
    pub(super) line:usize,
    pub(super) paragraph:usize,
    pub(super) roles:u16,
    pub(super) candidates:std::ops::Range<usize>,
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) struct Position{
    pub(super) line:usize,
    pub(super) paragraph:usize,
    pub(super) scalar:usize,
    pub(super) utf16:usize,
}
#[derive(Debug,PartialEq,Eq)]
pub(super) struct Relation{
    pub(super) line_delta:i64,
    pub(super) scalar_delta:i64,
    pub(super) utf16_delta:i64,
    pub(super) same_paragraph:bool,
    pub(super) gap:InterveningLines,
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
struct Token{
    roles:u16,
    span:DocumentSpan,
    paragraph:usize,
}
/// Half-open indices in the structural sequence, including an optional
/// observed incomplete final cycle. The source span is never synthesized.
#[derive(Debug,Clone,PartialEq,Eq)]
pub(super) struct Repeat{
    pub(super) start:usize,
    pub(super) end:usize,
    pub(super) period:usize,
    pub(super) cycles:usize,
    pub(super) remainder:usize,
    pub(super) span:DocumentSpan,
}
#[derive(Debug,PartialEq,Eq)]
pub(super) struct LocalStructure{
    pub(super) paragraph:usize,
    pub(super) slots:std::ops::Range<usize>,
    pub(super) roles:Vec<u16>,
}
/// Versioned canonical structure, not a hash of raw text. Distance,
/// multiplicity, catalog IDs, aliases, scores and contact values never
/// participate in equality. Raw features remain separately available.
#[derive(Debug,Clone,PartialEq,Eq,Hash)]
pub(super) struct PatternIdentity{
    pub(super) version:u8,
    pub(super) roles:Vec<u16>,
}
pub(super) struct DocumentFeatures<'g,'a,'s>{
    geometry:&'g SourceGeometry<'a>,
    pub(super) occurrences:&'g [DocumentOccurrence<'s>],
    pub(super) slots:Vec<FeatureSlot>,
    structural:Vec<Token>,
    pub(super) repeats:Vec<Repeat>,
    pub(super) local:Vec<LocalStructure>,
    pub(super) pattern:PatternIdentity,
    // Compact directed adjacency. Arbitrary candidate pairs are queried
    // on demand instead of materializing the Cartesian product.
    pub(super) next_same_role:Vec<Vec<usize>>,
}
fn delta(from:usize,to:usize)->i64{to as i64-from as i64}
pub(super) fn position(geometry:&SourceGeometry<'_>,offset:usize)->Position{
    let scalar=geometry.utf16_boundaries.binary_search_by_key(&offset,|&(byte,_)|byte)
        .expect("feature offset must be a source character boundary");
    let line=geometry.lines.partition_point(|line|line.start<=offset).saturating_sub(1);
    Position{line,paragraph:geometry.lines[line].paragraph,
        scalar,utf16:geometry.utf16_boundaries[scalar].1}
}
pub(super) fn relation(geometry:&SourceGeometry<'_>,from:usize,to:usize)->Relation{
    let a=position(geometry,from);
    let b=position(geometry,to);
    Relation{line_delta:delta(a.line,b.line),scalar_delta:delta(a.scalar,b.scalar),
        utf16_delta:delta(a.utf16,b.utf16),same_paragraph:a.paragraph==b.paragraph,
        gap:geometry.gap(a.line.min(b.line),a.line.max(b.line))}
}
impl<'g,'a,'s> DocumentFeatures<'g,'a,'s>{
    pub(super) fn build(geometry:&'g SourceGeometry<'a>,document:&'g DocumentCandidates<'a,'s>)->Option<Self>{
        let occurrences=document.complete_occurrences()?;
        let mut slots=Vec::new();
        let mut begin=0;
        while begin<occurrences.len(){
            let start=occurrences[begin].span.start;
            let mut end=begin+1;
            while end<occurrences.len()&&occurrences[end].span.start==start{end+=1;}
            let group=&occurrences[begin..end];
            let pos=position(geometry,start);
            let roles=group.iter().fold(0,|mask,c|mask|role(c.kind));
            let span=DocumentSpan{start,end:group.iter().map(|c|c.span.end).max().unwrap_or(start)};
            slots.push(FeatureSlot{span,line:pos.line,paragraph:pos.paragraph,roles,candidates:begin..end});
            begin=end;
        }
        let structural=structural_tokens(&slots,occurrences);
        let repeats=find_repeats(&structural);
        let pattern=PatternIdentity{version:1,roles:canonical(&structural,&repeats)};
        let mut local=Vec::new();
        let mut start=0;
        while start<slots.len(){
            let paragraph=slots[start].paragraph;
            let end=start+slots[start..].partition_point(|s|s.paragraph==paragraph);
            let first=structural.partition_point(|t|t.paragraph<paragraph);
            let last=structural.partition_point(|t|t.paragraph<=paragraph);
            local.push(LocalStructure{paragraph,slots:start..end,
                roles:structural[first..last].iter().map(|t|t.roles).collect()});
            start=end;
        }
        let mut next_same_role=vec![Vec::new();slots.len()];
        let mut last:[Option<usize>;8]=[None;8];
        for (i,slot) in slots.iter().enumerate(){
            for bit in 0..8{
                if slot.roles&(1<<bit)!=0{
                    if let Some(previous)=last[bit]{next_same_role[previous].push(i);}
                    last[bit]=Some(i);
                }
            }
        }
        for edges in &mut next_same_role{edges.sort_unstable();edges.dedup();}
        Some(Self{geometry,occurrences,slots,structural,repeats,local,pattern,next_same_role})
    }
    /// The full and partial orders are views of the same slots; gaps and
    /// competing same-start roles stay visible, never resolved by this module.
    pub(super) fn sequence(&self)->impl Iterator<Item=&FeatureSlot>{self.slots.iter()}
    pub(super) fn partial_order(&self,from:usize,to:usize)->Option<Relation>{
        let a=self.slots.get(from)?;
        let b=self.slots.get(to)?;
        Some(self.between(a.span.start,b.span.start))
    }
    pub(super) fn between(&self,from:usize,to:usize)->Relation{relation(self.geometry,from,to)}
    pub(super) fn structural_order(&self)->impl Iterator<Item=u16>+'_{
        self.structural.iter().map(|t|t.roles)
    }
    /// Signed distance between the nearest edges of independent source
    /// ranges. Overlapping competing candidates have zero gap.
    pub(super) fn candidate_gap(&self,from:usize,to:usize)->Option<(i64,i64)>{
        let a=self.occurrences.get(from)?.span;
        let b=self.occurrences.get(to)?.span;
        let (start,end,sign)=if a.end<=b.start{(a.end,b.start,1)}
            else if b.end<=a.start{(b.end,a.start,-1)}
            else{return Some((0,0));};
        let x=position(self.geometry,start);
        let y=position(self.geometry,end);
        Some((sign*delta(x.scalar,y.scalar),sign*delta(x.utf16,y.utf16)))
    }
}
fn structural_tokens(slots:&[FeatureSlot],occurrences:&[DocumentOccurrence<'_>])->Vec<Token>{
    let mut tokens:Vec<Token>=Vec::new();
    let mut role_end=[0usize;5];
    let mut last_line=None;
    for slot in slots{
        if last_line!=Some(slot.line){role_end=[0;5];last_line=Some(slot.line);}
        let mut roles=0;
        let mut end=slot.span.start;
        for c in &occurrences[slot.candidates.clone()]{
            let r=role(c.kind)&CORE;
            if r==0||c.span.end<=c.span.start{continue;}
            let bit=r.trailing_zeros() as usize;
            // Nested suffixes of the same role are alternatives within
            // one source field, not an additional written field.
            if c.span.start>=role_end[bit]||c.span.end>role_end[bit]{
                roles|=r;
                end=end.max(c.span.end);
            }
        }
        for c in &occurrences[slot.candidates.clone()]{
            let r=role(c.kind)&CORE;
            if r!=0{let bit=r.trailing_zeros() as usize;role_end[bit]=role_end[bit].max(c.span.end);}
        }
        if roles==0{continue;}
        // A quantity following an item is optional structure, but its
        // actual position, value and candidate remain in the full sequence.
        if roles==QUANTITY{
            if let Some(previous)=tokens.last_mut(){
                if previous.roles==PRODUCT&&previous.paragraph==slot.paragraph{
                    previous.span.end=previous.span.end.max(end);continue;
                }
            }
        }
        tokens.push(Token{roles,span:DocumentSpan{start:slot.span.start,end},paragraph:slot.paragraph});
    }
    tokens
}
/// Exact shortest period for a complete sequence (including a truncated
/// final prefix), using the linear prefix-function. Require two cycles
/// before claiming a repeated structure.
fn period(tokens:&[Token])->Option<usize>{
    let n=tokens.len();
    if n<2{return None;}
    let mut prefix=vec![0usize;n];
    for i in 1..n{
        let mut j=prefix[i-1];
        while j>0&&tokens[i].roles!=tokens[j].roles{j=prefix[j-1];}
        if tokens[i].roles==tokens[j].roles{j+=1;}
        prefix[i]=j;
    }
    let p=n-prefix[n-1];
    (p<=n/2).then_some(p)
}
fn run(tokens:&[Token],start:usize,end:usize,p:usize)->Repeat{
    let len=end-start;
    Repeat{start,end,period:p,cycles:len/p,remainder:len%p,
        span:DocumentSpan{start:tokens[start].span.start,
            end:tokens[start..end].iter().map(|t|t.span.end).max().unwrap()}}
}
fn local_runs(tokens:&[Token],start:usize,end:usize,out:&mut Vec<Repeat>){
    if let Some(p)=period(&tokens[start..end]){
        out.push(run(tokens,start,end,p));return;
    }
    let mut i=start;
    while i+1<end{
        let limit=LOCAL_PERIOD_LIMIT.min((end-i)/2);
        let candidate=(1..=limit).find(|&p|
            (0..p).all(|j|tokens[i+j].roles==tokens[i+p+j].roles));
        if let Some(p)=candidate{
            let mut stop=i+2*p;
            while stop<end&&tokens[stop].roles==tokens[i+(stop-i)%p].roles{stop+=1;}
            out.push(run(tokens,i,stop,p));i=stop;
        }else{i+=1;}
    }
}
fn find_repeats(tokens:&[Token])->Vec<Repeat>{
    if let Some(p)=period(tokens){return vec![run(tokens,0,tokens.len(),p)];}
    // Detect exact paragraph patterns even when the period exceeds the
    // bounded local search. Fall back to a bounded scan of irregular data.
    let mut exact=Vec::new();
    let mut i=0;
    while i<tokens.len(){
        let end=i+tokens[i..].partition_point(|t|t.paragraph==tokens[i].paragraph);
        if let Some(p)=period(&tokens[i..end]){exact.push(run(tokens,i,end,p));}
        i=end;
    }
    let mut result=Vec::new();
    let mut start=0;
    for r in exact{
        local_runs(tokens,start,r.start,&mut result);
        start=r.end;result.push(r);
    }
    local_runs(tokens,start,tokens.len(),&mut result);
    result
}
fn canonical(tokens:&[Token],repeats:&[Repeat])->Vec<u16>{
    let mut roles=Vec::new();
    let mut i=0;
    for r in repeats{
        roles.extend(tokens[i..r.start].iter().map(|t|t.roles));
        roles.extend(tokens[r.start..r.start+r.period].iter().map(|t|t.roles));
        i=r.end;
    }
    roles.extend(tokens[i..].iter().map(|t|t.roles));
    // A block's consecutive product fields are independent quantities,
    // not distinct document-writing patterns. Do not collapse other roles.
    roles.dedup_by(|a,b|*a==PRODUCT&&*b==PRODUCT);
    // Unequal item counts can hide a repeated recipient cycle in the
    // original sequence. Normalize item multiplicity before checking
    // the canonical sequence once more. Do not rewrite source repeats.
    let normalized=roles.iter().enumerate().map(|(i,&roles)|Token{
        roles,span:DocumentSpan{start:i,end:i+1},paragraph:0}).collect::<Vec<_>>();
    if let Some(p)=period(&normalized){roles.truncate(p);}
    roles
}

#[cfg(test)]
mod tests{
    use super::*;
    fn token_sequence(roles:&[u16])->Vec<Token>{
        roles.iter().enumerate().map(|(i,&roles)|Token{roles,
            span:DocumentSpan{start:i,end:i+1},paragraph:0}).collect()
    }
    #[test]
    fn exact_and_incomplete_repetition_preserve_source_boundaries(){
        let tokens=token_sequence(&[PRODUCT,NAME,CONTACT,ADDRESS,PRODUCT,NAME,CONTACT,ADDRESS,PRODUCT,NAME]);
        let found=find_repeats(&tokens);
        assert_eq!(found,vec![Repeat{start:0,end:10,period:4,cycles:2,remainder:2,
            span:DocumentSpan{start:0,end:10}}]);
        assert_eq!(canonical(&tokens,&found),vec![PRODUCT,NAME,CONTACT,ADDRESS]);
        let irregular=token_sequence(&[OTHER,NAME,CONTACT,NAME,CONTACT,ADDRESS]);
        assert_eq!(find_repeats(&irregular)[0].span,DocumentSpan{start:1,end:5});
    }
    #[test]
    fn pattern_identity_is_independent_of_candidate_confidence(){
        let once=token_sequence(&[NAME,CONTACT,ADDRESS,PRODUCT]);
        let twice=token_sequence(&[NAME,CONTACT,ADDRESS,PRODUCT,NAME,CONTACT,ADDRESS,PRODUCT]);
        assert_eq!(canonical(&once,&find_repeats(&once)),canonical(&twice,&find_repeats(&twice)));
        let differently_ordered=token_sequence(&[CONTACT,NAME,ADDRESS,PRODUCT]);
        assert_ne!(canonical(&once,&find_repeats(&once)),canonical(&differently_ordered,&find_repeats(&differently_ordered)));
        let uneven=token_sequence(&[NAME,CONTACT,ADDRESS,PRODUCT,PRODUCT,NAME,CONTACT,ADDRESS,PRODUCT]);
        assert_eq!(canonical(&once,&find_repeats(&once)),canonical(&uneven,&find_repeats(&uneven)));
    }
    #[test]
    fn pattern_normalizes_item_multiplicity_without_discarding_source_candidates(){
        fn check(raw:&str,fields:&[(&str,&'static str,Option<&'static str>)])->(PatternIdentity,usize){
            let geometry=SourceGeometry::new(raw);
            let mut cursor=0;
            let occurrences=fields.iter().map(|&(text,kind,product)|{
                let start=cursor+raw[cursor..].find(text).unwrap();
                let end=start+text.len();cursor=end;
                DocumentOccurrence{span:DocumentSpan{start,end},kind,product,alias:None}
            }).collect();
            let document=DocumentCandidates{rows:vec![],projected_rows:0,occurrences,incomplete:false};
            let features=DocumentFeatures::build(&geometry,&document).unwrap();
            assert_eq!(features.occurrences.len(),fields.len());
            assert_eq!(features.candidate_gap(0,1),Some((1,1)));
            assert_eq!(features.candidate_gap(1,0),Some((-1,-1)));
            (features.pattern,features.slots.len())
        }
        let a=check("N T A P Q",&[("N","name",None),("T","phone",None),("A","address",None),
            ("P","registered",Some("sku-a")),("Q","quantity",None)]);
        let b=check("N T A P Q P Q M",&[("N","name",None),("T","mobile",None),("A","address",None),
            ("P","registered",Some("sku-b")),("Q","quantity",None),("P","alias",Some("sku-c")),
            ("Q","quantity",None),("M","note",None)]);
        assert_eq!(a.0,b.0);
        assert!(a.1<b.1);
    }
    #[test]
    fn signed_distances_and_blank_boundaries_use_original_geometry(){
        let raw="😀가\r\n\r\n나\n다";
        let geometry=SourceGeometry::new(raw);
        let a=raw.find('가').unwrap();let b=raw.find('다').unwrap();
        let forward=relation(&geometry,a,b);let reverse=relation(&geometry,b,a);
        assert_eq!(forward.line_delta,3);
        assert_eq!(forward.scalar_delta,7);
        assert_eq!(forward.utf16_delta,7);
        assert_eq!(forward.gap.blank_lines,1);
        assert_eq!(forward.gap.content_lines,1);
        assert!(!forward.same_paragraph);
        assert_eq!(reverse.line_delta,-3);
        assert_eq!(reverse.scalar_delta,-forward.scalar_delta);
        assert_eq!(reverse.gap,forward.gap);
        assert_eq!(relation(&geometry,0,a).scalar_delta,1);
        assert_eq!(relation(&geometry,0,a).utf16_delta,2);
    }
}
