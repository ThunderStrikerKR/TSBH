//! Explicit modern Korean postal-code boundaries; no address lookup or guessing.
use regex::Regex;
use std::sync::OnceLock;

pub(super) fn before_address(prefix:&str)->Option<&str>{
    static RE:OnceLock<Regex>=OnceLock::new();
    let re=RE.get_or_init(||Regex::new(
        r"(?:\[([0-9]{5})\]|\(([0-9]{5})\)|(?:^|[\s:：])([0-9]{5}))\s*$"
    ).expect("postcode before address"));
    let c=re.captures(prefix)?;
    c.get(1).or_else(||c.get(2)).or_else(||c.get(3)).map(|m|m.as_str())
}

pub(super) fn labelled_line(line:&str)->Option<&str>{
    static RE:OnceLock<Regex>=OnceLock::new();
    let re=RE.get_or_init(||Regex::new(
        r"^\s*(?:수취인\s*|수하인\s*)?우편번호\s*[:：]?\s*([0-9]{5})\s*$"
    ).expect("postal label"));
    re.captures(line)?.get(1).map(|m|m.as_str())
}

pub(super) fn labelled_suffix(line:&str)->Option<(usize,&str)>{
    static RE:OnceLock<Regex>=OnceLock::new();
    let re=RE.get_or_init(||Regex::new(
        r"\s+우편번호\s*[:：]?\s*([0-9]{5})\s*$"
    ).expect("postal suffix"));
    let c=re.captures(line)?;
    Some((c.get(0)?.start(),c.get(1)?.as_str()))
}

pub(super) fn manual_address(value:&str)->(&str,Option<&str>){
    static RE:OnceLock<Regex>=OnceLock::new();
    let re=RE.get_or_init(||Regex::new(
        r"^\s*(?:\[([0-9]{5})\]|\(([0-9]{5})\)|([0-9]{5}))\s+"
    ).expect("postal prefix"));
    if let Some(c)=re.captures(value){
        let rest=&value[c.get(0).unwrap().end()..];
        if !rest.trim().is_empty(){
            return (rest,c.get(1).or_else(||c.get(2)).or_else(||c.get(3)).map(|m|m.as_str()));
        }
    }
    if let Some((end,code))=labelled_suffix(value){
        if !value[..end].trim().is_empty(){return (&value[..end],Some(code));}
    }
    (value,None)
}
