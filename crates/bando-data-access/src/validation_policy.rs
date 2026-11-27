//! Conservative comparison of exhaustive, identified validation findings.
//! Evidence is internal; no data mutation or error-message-based exemption.
use serde_json::Value;
use std::collections::BTreeMap;
pub(super) type Finding=(Vec<String>,Value,String);
#[derive(Debug,PartialEq,Eq)]
pub(super) struct Comparison {
    pub blocking:Vec<String>,
    pub retained:usize,
    pub resolved:usize,
}
pub(super) fn compare(before:Vec<Finding>,next:Vec<Finding>)->Comparison{
    let mut old:BTreeMap<Vec<String>,Vec<Value>>=BTreeMap::new();
    for (key,evidence,_) in before{old.entry(key).or_default().push(evidence);}
    let mut counts=BTreeMap::new();
    for (key,_,_) in &next{*counts.entry(key.clone()).or_insert(0usize)+=1;}
    let mut result=Comparison{blocking:vec![],retained:0,resolved:0};
    for (key,evidence,message) in next{
        let stable=!key.is_empty()&&counts.get(&key)==Some(&1)&&
            old.get(&key).is_some_and(|v|v.len()==1&&v[0]==evidence);
        if stable{result.retained+=1;}else{result.blocking.push(message);}
        old.remove(&key);
    }
    result.resolved=old.into_iter().filter(|(key,v)|!key.is_empty()&&v.len()==1).count();
    result
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn f(id:&str,e:Value)->Finding{(vec!["missing".into(),id.into()],e,"same".into())}
    #[test] fn retained_and_resolved(){
        let result=compare(vec![f("a",json!(1)),f("b",json!(2))],vec![f("a",json!(1))]);
        assert_eq!((result.retained,result.resolved,result.blocking.len()),(1,1,0));
    }
    #[test] fn same_message_different_subject_or_evidence_blocks(){
        for next in [f("b",json!(1)),f("a",json!(2))]{
            assert_eq!(compare(vec![f("a",json!(1))],vec![next]).blocking.len(),1);
        }
    }
    #[test] fn existing_error_does_not_hide_second_error(){
        let result=compare(vec![f("a",json!(1))],vec![f("a",json!(1)),f("b",json!(1))]);
        assert_eq!((result.retained,result.blocking.len()),(1,1));
    }
    #[test] fn ambiguous_or_unidentified_findings_block(){
        let a=f("a",json!(1));
        assert_eq!(compare(vec![a.clone(),a.clone()],vec![a.clone()]).blocking.len(),1);
        assert_eq!(compare(vec![a.clone()],vec![a.clone(),a]).blocking.len(),2);
        let b=(vec![],json!(1),"unknown".into());
        assert_eq!(compare(vec![b.clone()],vec![b]).blocking.len(),1);
    }
}
