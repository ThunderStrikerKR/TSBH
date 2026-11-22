//! Editing helpers for customer search tags. Client storage keeps the
//! existing shortNames and identifiers strings for Rust matching/Excel parity.
//! The UI combines both only for display; adding an alias must not overwrite
//! a legacy identifier, and deleting one chip must remove its duplicate keys.
use serde_json::{json,Value};
use std::collections::HashSet;

fn field<'a>(client:&'a Value,key:&str)->&'a str{
    client.get(key).and_then(Value::as_str).unwrap_or("")
}
fn key(text:&str)->String{
    text.chars().filter(|c|!c.is_whitespace())
        .flat_map(char::to_lowercase).collect()
}
fn parts(text:&str)->impl Iterator<Item=&str>{
    text.split(|ch|matches!(ch,','|';'|'|'|'\n'))
        .map(str::trim).filter(|s|!s.is_empty())
}
/// One visible chip per distinct alias/identifier, in original order.
pub fn labels(client:&Value)->Vec<String>{
    let mut seen=HashSet::new();
    ["shortNames","identifiers"].into_iter()
        .flat_map(|source|parts(field(client,source)))
        .filter(|tag|seen.insert(key(tag)))
        .map(str::to_owned).collect()
}
/// Append new aliases only to shortNames. Existing identifiers remain byte
/// for byte unchanged. Name and any existing tag cannot be added twice.
pub fn add(client:&mut Value,text:&str)->(usize,usize){
    let mut seen=HashSet::new();
    seen.insert(key(field(client,"name")));
    for label in labels(client){seen.insert(key(&label));}
    let mut added=Vec::new();
    let mut duplicates=0;
    for label in parts(text){
        if seen.insert(key(label)){
            added.push(label.to_owned());
        }else{
            duplicates+=1;
        }
    }
    let count=added.len();
    if count>0{
        let previous=field(client,"shortNames").trim();
        let updated=if previous.is_empty(){added.join(", ")}
            else{format!("{previous}, {}",added.join(", "))};
        client["shortNames"]=json!(updated);
    }
    (count,duplicates)
}
/// Remove the selected key from both sources, preserving unrelated raw
/// fields. Only touched source strings are normalized to comma separators.
pub fn remove(client:&mut Value,selected:&str)->bool{
    let search=key(selected);
    if search.is_empty(){return false;}
    let mut changed=false;
    for source in ["shortNames","identifiers"]{
        let existing=field(client,source);
        let entries=parts(existing).collect::<Vec<_>>();
        let remaining=entries.iter().copied()
            .filter(|tag|key(tag)!=search).collect::<Vec<_>>();
        if remaining.len()!=entries.len(){
            let updated=remaining.join(", ");
            client[source]=json!(updated);
            changed=true;
        }
    }
    changed
}
/// Consume only completed tokens while typing or pasting a comma-separated
/// series; the incomplete final token stays in the input until Enter/추가.
pub fn take_delimited(input:&mut String)->Option<String>{
    let index=input.char_indices().rev()
        .find(|(_,ch)|matches!(ch,','|';'|'|'|'\n'))?.0;
    let prefix=input[..index].to_owned();
    let rest=input[index+1..].to_owned();
    *input=rest;
    Some(prefix)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn shows_distinct_tags_from_both_legacy_sources(){
        let client=json!({"shortNames":"테스트 납품처 01, 테스트, 테스트",
            "identifiers":"테스트납품처01; 내부코드|옛 상호"});
        assert_eq!(labels(&client),vec![
            "테스트 납품처 01","테스트","내부코드","옛 상호"
        ]);
    }
    #[test]
    fn add_does_not_destroy_identifiers_or_repeat_official_name(){
        let mut client=json!({"name":"테스트상사",
            "shortNames":"기존태그","identifiers":"내부코드"});
        let (added,duplicates)=add(&mut client,
            " 테스트상사, 내부 코드, 새 별칭, 새별칭, 두번째");
        assert_eq!((added,duplicates),(2,3));
        assert_eq!(client["shortNames"],"기존태그, 새 별칭, 두번째");
        assert_eq!(client["identifiers"],"내부코드");
    }
    #[test]
    fn removing_duplicate_alias_clears_only_its_entries(){
        let mut client=json!({"shortNames":"A, 기존 값; 남길태그",
            "identifiers":"기존값|중요한식별자"});
        assert!(remove(&mut client,"기존 값"));
        assert_eq!(client["shortNames"],"A, 남길태그");
        assert_eq!(client["identifiers"],"중요한식별자");
        assert!(!remove(&mut client,"없는값"));
    }
    #[test]
    fn comma_input_keeps_incomplete_tail(){
        let mut text="가, 나, 다".to_owned();
        assert_eq!(take_delimited(&mut text),Some("가, 나".into()));
        assert_eq!(text," 다");
        assert_eq!(take_delimited(&mut text),None);
        text.push(',');
        assert_eq!(take_delimited(&mut text),Some(" 다".into()));
        assert!(text.is_empty());
    }
}
