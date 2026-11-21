//! Conservative, opt-in suggestions from employee-approved OCR corrections.
//! This is a local deterministic pattern learner, not a trained neural OCR model.
//! It never commits any source by itself: every suggestion is reviewed by staff.
//! The full original/corrected corpus remains independently auditable in SQLite.

use std::collections::{HashMap,HashSet};

pub const MIN_INDEPENDENT_RECEIPTS: usize = 3;
pub const MAX_RULES_PER_REVIEW: usize = 12;

#[derive(Clone,Debug)]
pub struct Suggestion {
    pub text:String,
    pub replacements:usize,
    pub patterns:usize,
}

fn is_safe_pattern(text:&str)->bool {
    let n=text.chars().count();
    // Never learn phone numbers, postal codes, item quantities, e-mail
    // addresses or anything containing numerals/unsafe punctuation.
    (3..=30).contains(&n)
        && text.chars().any(|c|c.is_alphabetic())
        && text.chars().all(|c|
            c.is_alphabetic() || c.is_whitespace())
}

/// Minimal changed phrase with up to three unchanged characters on either
/// side. This prevents learning a generic "space -> empty" rule that would
/// damage addresses and item names everywhere.
fn candidate(before:&str,after:&str)->Option<(String,String)>{
    if before==after {return None;}
    let a:Vec<char>=before.chars().collect();
    let b:Vec<char>=after.chars().collect();
    let mut prefix=0usize;
    while prefix<a.len().min(b.len()) && a[prefix]==b[prefix] {
        prefix+=1;
    }
    let mut suffix=0usize;
    while suffix<a.len().saturating_sub(prefix)
        && suffix<b.len().saturating_sub(prefix)
        && a[a.len()-suffix-1]==b[b.len()-suffix-1] {
        suffix+=1;
    }
    // A large unrelated edit is not a spelling correction.
    let old_core=a.len()-prefix-suffix;
    let new_core=b.len()-prefix-suffix;
    if old_core>8 || new_core>8 || old_core+new_core==0 {return None;}
    let left=prefix.min(3);
    let right=suffix.min(3);
    let old=a[prefix-left..a.len()-suffix+right].iter().collect::<String>();
    let new=b[prefix-left..b.len()-suffix+right].iter().collect::<String>();
    if !is_safe_pattern(&old)||!is_safe_pattern(&new)||old==new {
        return None;
    }
    Some((old,new))
}

/// Only same-position lines are eligible. Inserts/deletions and reordered
/// lines are intentionally excluded from automatic suggestions; the audit
/// retains all edits and can be reprocessed by a better future matcher.
fn line_patterns(original:&str,corrected:&str)->Vec<(String,String)>{
    let a:Vec<&str>=original.lines().collect();
    let b:Vec<&str>=corrected.lines().collect();
    if a.len()!=b.len()||a.len()>200 {return Vec::new();}
    a.into_iter().zip(b)
        .filter_map(|(old,new)|candidate(old,new))
        .collect()
}

fn replace_bounded(text:&str,from:&str,to:&str)->(String,usize){
    let mut result=String::with_capacity(text.len());
    let mut consumed=0usize;
    let mut count=0usize;
    for (idx,_) in text.match_indices(from){
        if idx<consumed {continue;}
        let before=text[..idx].chars().next_back();
        let after=text[idx+from.len()..].chars().next();
        // Do not replace inside longer words or model codes.
        if before.is_some_and(char::is_alphanumeric)
            || after.is_some_and(char::is_alphanumeric) {
            continue;
        }
        result.push_str(&text[consumed..idx]);
        result.push_str(to);
        consumed=idx+from.len();
        count+=1;
    }
    result.push_str(&text[consumed..]);
    (result,count)
}

/// Each tuple is (audit_id, order_id, initial OCR, employee correction).
/// Frequency is per distinct order, never per repeated line or duplicate
/// audit. The newest 1,000 records are read by the caller on a worker thread.
pub fn suggest(
    original:&str,samples:&[(String,String,String,String)]
)->Suggestion{
    let mut support:HashMap<(String,String),HashSet<String>>=HashMap::new();
    for (_,order_id,before,after) in samples {
        for pair in line_patterns(before,after) {
            support.entry(pair).or_default().insert(order_id.clone());
        }
    }
    let mut rules=support.into_iter()
        .filter_map(|((from,to),ids)|
            (ids.len()>=MIN_INDEPENDENT_RECEIPTS)
                .then_some((from,to,ids.len()))
        ).collect::<Vec<_>>();
    rules.sort_by(|a,b|b.2.cmp(&a.2)
        .then_with(||b.0.chars().count().cmp(&a.0.chars().count()))
        .then_with(||a.0.cmp(&b.0)));
    let mut current=original.to_owned();
    let mut replacements=0usize;
    let mut patterns=0usize;
    for (from,to,_) in rules.into_iter().take(MAX_RULES_PER_REVIEW){
        let (updated,count)=replace_bounded(&current,&from,&to);
        if count>0 {
            current=updated;
            replacements+=count;
            patterns+=1;
        }
    }
    Suggestion{text:current,replacements,patterns}
}

/// Conservative table-form layout learning, scoped to a verified dealer
/// or a unique matching dealer-specific form shape. Unlike character typo
/// suggestions this changes ONLY whitespace between identical OCR characters.
/// If any pattern is ambiguous, the original line breaks are preserved.
use crate::preview_store::OcrLayoutExample;

fn nonwhite_layout(text:&str)->(String,Vec<String>){
    let mut letters=String::new();
    let mut spaces=vec![String::new()];
    for ch in text.chars(){
        if ch.is_whitespace(){
            spaces.last_mut().expect("initial gap").push(ch);
        }else{
            letters.push(ch);
            spaces.push(String::new());
        }
    }
    (letters,spaces)
}

/// A low-information morphology signature. No customer names, phone digits
/// or street names become a template key. It only compares line structure:
/// short Hangul cells, number cells, mixed address cells, long text cells.
fn layout_shape(text:&str)->Option<Vec<u8>>{
    let lines=text.lines().collect::<Vec<_>>();
    if lines.len()<3 || lines.len()>200 {return None;}
    Some(lines.into_iter().map(|line|{
        let contents=line.trim();
        if contents.is_empty(){return 0}
        let hangul=contents.chars()
            .filter(|c|('가'..='힣').contains(c)).count();
        let digits=contents.chars().filter(char::is_ascii_digit).count();
        if hangul>0 && digits>0 {3}
        else if digits>0 {2}
        else if hangul>0 && contents.chars().count()<=3 {1}
        else if hangul>0 {4}
        else {5}
    }).collect())
}

fn layout_suggestion(
    original:&str, examples:&[OcrLayoutExample], known_client:Option<&str>
)->(String,usize){
    let Some(signature)=layout_shape(original) else{
        return (original.to_owned(),0);
    };
    let (characters,original_gaps)=nonwhite_layout(original);
    let lines_to_fix=original_gaps.iter().enumerate()
        .filter(|(_,gap)|gap.contains('\n'))
        .map(|(index,_)|index).collect::<Vec<_>>();
    if lines_to_fix.is_empty(){return (original.to_owned(),0);}

    let mut groups:HashMap<&str,Vec<&OcrLayoutExample>>=HashMap::new();
    for example in examples {
        let Some(dealer)=example.client_id.as_deref() else{continue};
        if known_client.is_some_and(|current|current!=dealer) {
            continue;
        }
        if layout_shape(&example.original_text).as_ref()!=Some(&signature){
            continue;
        }
        let (initial_chars,old_gaps)=nonwhite_layout(&example.original_text);
        let (corrected_chars,_)=nonwhite_layout(&example.corrected_text);
        if initial_chars!=corrected_chars ||
            old_gaps.iter().filter(|gap|gap.contains('\n')).count()
                !=lines_to_fix.len(){
            continue;
        }
        groups.entry(dealer).or_default().push(example);
    }
    // A known dealer always stays within that dealer. Without an assigned
    // dealer, only ONE independently corroborated template may be used.
    let mut eligible=groups.into_iter().filter_map(|(_,rows)|{
        let mut receipts=HashSet::new();
        let unique=rows.into_iter().filter(|row|
            receipts.insert(row.order_id.as_str())
        ).collect::<Vec<_>>();
        (unique.len()>=MIN_INDEPENDENT_RECEIPTS).then_some(unique)
    }).collect::<Vec<_>>();
    if eligible.len()!=1 {
        return (original.to_owned(),0);
    }
    let group=eligible.pop().expect("one cohort");
    let mut new_gaps=original_gaps.clone();
    let mut changes=0usize;
    for (ordinal,index) in lines_to_fix.iter().copied().enumerate(){
        // An identical structural position must have an identical
        // correction on at least three independent receipts.
        let mut votes:HashMap<String,usize>=HashMap::new();
        let mut total=0usize;
        for item in &group {
            let (_,before)=nonwhite_layout(&item.original_text);
            let (_,after)=nonwhite_layout(&item.corrected_text);
            let old_idx=before.iter().enumerate()
                .filter(|(_,gap)|gap.contains('\n'))
                .nth(ordinal).map(|(index,_)|index);
            let Some(old_idx)=old_idx else{continue};
            if before[old_idx]!=original_gaps[index]{continue;}
            let destination=&after[old_idx];
            // Only a newline, a space, or a complete join is learnable.
            if !matches!(destination.as_str(),""|" "|"\n"|"\n\n"){continue;}
            *votes.entry(destination.clone()).or_default()+=1;
            total+=1;
        }
        if total<MIN_INDEPENDENT_RECEIPTS || votes.len()!=1 {continue;}
        let (target,support)=votes.into_iter().next().expect("one vote");
        if support==total && target!=original_gaps[index]{
            new_gaps[index]=target;
            changes+=1;
        }
    }
    if changes==0 {return (original.to_owned(),0);}
    let mut output=String::with_capacity(original.len());
    let chars=characters.chars().collect::<Vec<_>>();
    for (i,gap) in new_gaps.iter().enumerate(){
        output.push_str(gap);
        if let Some(ch)=chars.get(i){output.push(*ch);}
    }
    (output,changes)
}

/// The experimental checkbox combines two independent proposal families:
/// dealer/template-specific line layout + existing repeated text corrections.
/// Both are previews only; employee confirmation still owns the final order.
pub fn suggest_with_layout(
    original:&str,
    spelling_samples:&[(String,String,String,String)],
    table_samples:&[OcrLayoutExample],
    known_client:Option<&str>
)->Suggestion{
    let (unwrapped,joins)=layout_suggestion(original,table_samples,known_client);
    let ordinary=suggest(&unwrapped,spelling_samples);
    Suggestion{
        text:ordinary.text,
        replacements:ordinary.replacements+joins,
        patterns:ordinary.patterns+usize::from(joins>0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(id:&str,old:&str,new:&str)->(String,String,String,String){
        (id.into(),id.into(),old.into(),new.into())
    }

    #[test]
    fn spacing_needs_three_independent_receipts(){
        let samples=[
            sample("1","서울 특별시 강남구","서울특별시 강남구"),
            sample("2","서울 특별시 강북구","서울특별시 강북구"),
            sample("3","서울 특별시 마포구","서울특별시 마포구"),
        ];
        assert_eq!(suggest("서울 특별시 서초구",&samples).text,
            "서울특별시 서초구");
        assert_eq!(suggest("서울 특별시 서초구",&samples[..2]).text,
            "서울 특별시 서초구");
    }

    #[test]
    fn never_rewrite_phone_numbers_or_model_digits(){
        let samples=[
            sample("1","010-1234-5678","010-1234-5679"),
            sample("2","010-1234-5678","010-1234-5679"),
            sample("3","010-1234-5678","010-1234-5679"),
        ];
        assert_eq!(suggest("010-1234-5678",&samples).replacements,0);
    }

    fn table_example(id:&str,client:&str,name:[&str;3],
        phone:(&str,&str),street:(&str,&str),quantity:&str
    )->OcrLayoutExample{
        let original=format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n반도\n뮤직트레이 회색\n{}\n메모\n문 앞에",
            name[0],name[1],name[2],phone.0,phone.1,
            street.0,street.1,quantity
        );
        let corrected=format!(
            "{}{}{}\n{}{}\n{}{}\n반도 뮤직트레이 회색 {}\n메모 문 앞에",
            name[0],name[1],name[2],phone.0,phone.1,
            street.0,street.1,quantity
        );
        OcrLayoutExample{
            order_id:id.into(),client_id:Some(client.into()),
            original_text:original,corrected_text:corrected,
        }
    }

    #[test]
    fn learns_repeated_table_wrapping_without_changing_any_character(){
        let examples=[
            table_example("1","A",["홍","길","동"],("010-1111-","2222"),
                ("서울특별시 강남구 테","헤란로 3"),"1개"),
            table_example("2","A",["김","철","수"],("010-2222-","3333"),
                ("서울특별시 강남구 가","나다로 5"),"2개"),
            table_example("3","A",["박","영","희"],("010-3333-","4444"),
                ("서울특별시 강남구 나","다라로 7"),"3개"),
        ];
        let incoming=table_example("new","A",["최","민","준"],
            ("010-4444-","5555"),("서울특별시 강남구 라","마바로 9"),"4개");
        let (proposed,count)=layout_suggestion(
            &incoming.original_text,&examples,Some("A")
        );
        assert!(count>=6,"expected multiple table wraps to be joined");
        assert_eq!(proposed,incoming.corrected_text);
        // Two independent examples are not enough even with an exact shape.
        assert_eq!(layout_suggestion(
            &incoming.original_text,&examples[..2],Some("A")
        ).1,0);
    }

    #[test]
    fn never_mix_ambiguous_dealer_templates(){
        let a=table_example("1","A",["홍","길","동"],("010-1111-","2222"),
            ("서울특별시 강남구 테","헤란로 3"),"1개");
        let b=table_example("2","A",["김","철","수"],("010-2222-","3333"),
            ("서울특별시 강남구 가","나다로 5"),"2개");
        let c=table_example("3","A",["박","영","희"],("010-3333-","4444"),
            ("서울특별시 강남구 나","다라로 7"),"3개");
        let mut cases=vec![a,b,c];
        for id in 4..=6 {
            let mut other=table_example(&id.to_string(),"B",
                ["임","은","진"],("010-5555-","6666"),
                ("서울특별시 강남구 테","헤란로 3"),"1개");
            // A second dealer's identical visual layout may have
            // different column semantics; ambiguity means no proposal.
            other.corrected_text=other.original_text.clone();
            cases.push(other);
        }
        let incoming=table_example("new","A",["최","민","준"],
            ("010-4444-","5555"),("서울특별시 강남구 라","마바로 9"),"4개");
        assert_eq!(layout_suggestion(&incoming.original_text,&cases,None).1,0);
        assert_eq!(layout_suggestion(&incoming.original_text,&cases,Some("A")).0,
            incoming.corrected_text);
    }

    #[test]
    fn duplicate_receipts_are_not_training_support(){
        let samples=[
            sample("a","서울 특별시 강남구","서울특별시 강남구"),
            sample("a","서울 특별시 강남구","서울특별시 강남구"),
            sample("b","서울 특별시 강남구","서울특별시 강남구"),
        ];
        assert_eq!(suggest("서울 특별시 강남구",&samples).patterns,0);
    }
}
