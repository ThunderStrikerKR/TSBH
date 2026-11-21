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
