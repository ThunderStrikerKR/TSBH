//! P05 diagnostic evidence support. Order-normalized, not calibrated
//! probabilities. No effect on parser decisions, no inferred candidate.
use super::{cached_count,existing,Fact,FEATURE_VERSION,CONTRIBUTION_VERSION};
use rusqlite::{params,Transaction};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

const MAX_ORDERS:usize=50_000;
const MAX_FACTS:usize=2_000_000;
const SUPPORT_VERSION:i64=1;

#[derive(Default)]
struct Evidence{
    weak:BTreeMap<String,u64>,
    positive:BTreeMap<String,u64>,
    negative:BTreeMap<String,u64>,
    observations:u64,
    pending:u64,
    unresolved:u64,
    independent:u64,
    weak_orders:u64,
    explicit_orders:u64,
    weak_count:u64,
    correction_count:u64,
}
fn sum(map:&BTreeMap<String,u64>)->Result<u64,String>{
    map.values().try_fold(0u64,|a,b|a.checked_add(*b).ok_or("학습 증거 합계 초과".to_owned()))
}
impl Evidence{
    fn from_facts(facts:&[Fact],family:&str,key:&str)->Result<Self,String>{
        let mut e=Self::default();
        for (f,k,metric,candidate,amount) in facts{
            if f!=family||k!=key{continue;}
            let n=u64::try_from(*amount).ok().filter(|n|*n>0).ok_or("학습 증거량 오류")?;
            match metric.as_str(){
                "weakCandidates"=>{e.weak.insert(candidate.clone(),n);},
                "explicitPositiveCandidates"=>{e.positive.insert(candidate.clone(),n);},
                "explicitNegativeCandidates"=>{e.negative.insert(candidate.clone(),n);},
                "observationCount"=>e.observations=n,
                "pendingCorrectionCount"=>e.pending=n,
                "unresolvedCount"=>e.unresolved=n,
                "independentOrders"=>e.independent=n,
                "weakIndependentOrders"=>e.weak_orders=n,
                "explicitCorrectionOrders"=>e.explicit_orders=n,
                "weakUnchangedCount"=>e.weak_count=n,
                "explicitCorrectionCount"=>e.correction_count=n,
                _=>{}
            }
        }
        let weak=sum(&e.weak)?;let positive=sum(&e.positive)?;let negative=sum(&e.negative)?;
        if e.independent!=1 || weak!=e.weak_count || positive!=e.correction_count || positive!=negative
            || e.weak_orders!=u64::from(weak>0) || e.explicit_orders!=u64::from(positive>0)
            || weak.checked_add(positive).and_then(|x|x.checked_add(e.pending)).and_then(|x|x.checked_add(e.unresolved))!=Some(e.observations)
            || [&e.weak,&e.positive,&e.negative].iter().any(|map|map.keys().any(|k|k.trim().is_empty())){
            return Err("주문별 학습 증거 규격 불일치".into());
        }
        Ok(e)
    }
}

#[derive(Default)]
struct Candidate{
    support:f64,
    negative:f64,
    weak_orders:u64,
    positive_orders:u64,
    negative_orders:u64,
}
#[derive(Default)]
struct Aggregate{
    candidates:BTreeMap<String,Candidate>,
    context_orders:u64,
    supported_orders:u64,
    weak_only:u64,
    explicit_only:u64,
    mixed:u64,
    no_support:u64,
    pending_orders:u64,
    unresolved_orders:u64,
    weight:f64,
    weight_squared:f64,
    outside_support:f64,
    outside_negative:f64,
}
impl Aggregate{
    fn add(&mut self,e:Evidence,allowed:&BTreeSet<String>,ratio:f64)->Result<(),String>{
        self.context_orders+=1;
        self.pending_orders+=u64::from(e.pending>0);
        self.unresolved_orders+=u64::from(e.unresolved>0);
        let weak=sum(&e.weak)?;let strong=sum(&e.positive)?;
        // One independent order has at most one unit of positive mass.
        // Normalize within each channel before assigning channel budgets.
        let (w,s)=match (weak>0,strong>0){
            (true,true)=>{self.mixed+=1;(1.0/(1.0+ratio),ratio/(1.0+ratio))},
            (true,false)=>{self.weak_only+=1;(1.0/ratio,0.0)},
            (false,true)=>{self.explicit_only+=1;(0.0,1.0)},
            (false,false)=>{self.no_support+=1;return Ok(());}
        };
        self.supported_orders+=1;
        self.weight+=w+s;self.weight_squared+=(w+s)*(w+s);
        for (candidate,n) in e.weak{
            let mass=w*n as f64/weak as f64;
            if allowed.contains(&candidate){
                let row=self.candidates.entry(candidate).or_default();row.support+=mass;row.weak_orders+=1;
            }else{self.outside_support+=mass;}
        }
        for (candidate,n) in e.positive{
            let mass=s*n as f64/strong as f64;
            if allowed.contains(&candidate){
                let row=self.candidates.entry(candidate).or_default();row.support+=mass;row.positive_orders+=1;
            }else{self.outside_support+=mass;}
        }
        // Negative evidence is reported separately. A correction is NOT
        // counted once as positive and again as negative in the denominator.
        for (candidate,n) in e.negative{
            let mass=s*n as f64/strong as f64;
            if allowed.contains(&candidate){
                let row=self.candidates.entry(candidate).or_default();row.negative+=mass;row.negative_orders+=1;
            }else{self.outside_negative+=mass;}
        }
        Ok(())
    }
    fn wire(&self,allowed:&BTreeSet<String>,ratio:f64)->Value{
        let share=|v:f64|if self.weight>0.0{Some(v/self.weight)}else{None};
        let candidates=allowed.iter().map(|id|{
            let empty=Candidate::default();let c=self.candidates.get(id).unwrap_or(&empty);
            json!({"candidate":id,"supportMass":c.support,"supportShare":share(c.support),
                "explicitOppositionMass":c.negative,"weakOrders":c.weak_orders,
                "explicitPositiveOrders":c.positive_orders,"explicitNegativeOrders":c.negative_orders,
                "conflictingEvidence":c.support>0.0&&c.negative>0.0})
        }).collect::<Vec<_>>();
        let effective=if self.weight_squared>0.0{Some(self.weight*self.weight/self.weight_squared)}else{None};
        json!({"status":"diagnostic_only","supportVersion":SUPPORT_VERSION,"featureVersion":FEATURE_VERSION,
            "contributionVersion":CONTRIBUTION_VERSION,"explicitToWeakRatio":ratio,
            "contextOrders":self.context_orders,"supportedOrders":self.supported_orders,
            "weakOnlyOrders":self.weak_only,"explicitOnlyOrders":self.explicit_only,"mixedEvidenceOrders":self.mixed,
            "noSupportOrders":self.no_support,"pendingCorrectionOrders":self.pending_orders,"unresolvedOrders":self.unresolved_orders,
            "totalSupportMass":self.weight,"weightEffectiveOrders":effective,
            "outsideCandidateSupportMass":self.outside_support,"outsideCandidateSupportShare":share(self.outside_support),
            "outsideCandidateOppositionMass":self.outside_negative,"candidates":candidates,
            "calibratedProbability":null,"eligibleForAutomaticDecision":false,"decisionChanged":false,
            "fullHistoryReconciled":false,"targetOrderExcluded":true})
    }
}

fn request(input:&Value)->Result<(&str,&str,&str,BTreeSet<String>,f64),String>{
    let task=input["task"].as_str().filter(|t|matches!(*t,"recipient_ownership"|"type"|"product"|"variant"|"quantity")).ok_or("학습 작업 유형 오류")?;
    let key=input["contextKey"].as_str().filter(|v|!v.is_empty()&&v.len()<=8192).ok_or("학습 문맥 키 오류")?;
    let context:Value=serde_json::from_str(key).map_err(|_|"학습 문맥 키 형식 오류")?;
    let parts=context.as_array().filter(|a|a.len()==5).ok_or("학습 문맥 키 형식 오류")?;
    if parts[0]!=FEATURE_VERSION || parts[1].as_str()!=Some(task)
        || !matches!(parts[2].as_str(),Some("base"|"structure"|"client_structure"))
        || (parts[2]=="client_structure" && parts[3].as_str().is_none_or(|id|id.trim().is_empty()))
        || (parts[2]!="client_structure" && !parts[3].is_null()) || !parts[4].is_array(){
        return Err("학습 문맥 규격 오류".into());
    }
    let target=input["targetOrderId"].as_str().filter(|v|!v.trim().is_empty()&&v.len()<=1024).ok_or("자기 학습 제외 주문 ID 필요")?;
    let candidates=input["candidates"].as_array().filter(|v|!v.is_empty()&&v.len()<=64).ok_or("현재 후보 목록 오류")?;
    let mut allowed=BTreeSet::new();
    for c in candidates{
        let id=c.as_str().filter(|v|!v.trim().is_empty()&&v.len()<=1024).ok_or("후보 ID 오류")?;
        if !allowed.insert(id.to_owned()){return Err("중복 후보 ID".into());}
    }
    if task=="recipient_ownership" && allowed!=BTreeSet::from(["previous".to_owned(),"next".to_owned()]){
        return Err("귀속 후보는 previous/next 쌍이어야 합니다.".into());
    }
    let ratio=input["explicitToWeakRatio"].as_f64().filter(|v|v.is_finite()&&(1.0..=100.0).contains(v))
        .ok_or("진단 가중치 비율을 1~100 범위로 명시하세요.")?;
    Ok((if task=="recipient_ownership"{"statistics"}else{"fieldContextStatistics"},key,target,allowed,ratio))
}

/// Read only one context from the verified per-order ledger. Do not use
/// pre-summed candidate-order counts: candidates can overlap within orders.
pub(super) fn assess(tx:&Transaction<'_>,scope:&str,input:&Value)->Result<Value,String>{
    let (family,key,target,allowed,ratio)=request(input)?;
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending!=0{return Err("복구 대기 학습 주문이 있어 지지도 조회를 중단합니다.".into());}
    let mut stmt=tx.prepare("SELECT DISTINCT order_id FROM recognition_learning_facts
        WHERE scope=?1 AND feature_version=?2 AND family=?3 AND context_key=?4 AND order_id<>?5
        ORDER BY order_id LIMIT ?6").map_err(|e|e.to_string())?;
    let mut rows=stmt.query(params![scope,FEATURE_VERSION,family,key,target,(MAX_ORDERS+1) as i64]).map_err(|e|e.to_string())?;
    let mut result=Aggregate::default();let mut checked_facts=0usize;
    while let Some(row)=rows.next().map_err(|e|e.to_string())?{
        if result.context_orders>=MAX_ORDERS as u64{return Err("학습 지지도 조회 주문 한도 초과".into());}
        let id:String=row.get(0).map_err(|e|e.to_string())?;
        let digest:String=tx.query_row("SELECT fingerprint FROM recognition_learning_fingerprints
            WHERE scope=?1 AND feature_version=?2 AND order_id=?3 AND contribution_version=?4",
            params![scope,FEATURE_VERSION,id,CONTRIBUTION_VERSION],|r|r.get(0)).map_err(|_|"검증된 학습 지문 누락")?;
        if !matches!(cached_count(tx,scope,&id,&digest)?,Some(n) if n>0){
            return Err("검증되지 않은 학습 기여량".into());
        }
        let facts=existing(tx,scope,&id)?;
        checked_facts=checked_facts.saturating_add(facts.len());
        if checked_facts>MAX_FACTS{return Err("학습 지지도 조회 기여량 한도 초과".into());}
        result.add(Evidence::from_facts(&facts,family,key)?,&allowed,ratio)?;
    }
    Ok(result.wire(&allowed,ratio))
}

#[cfg(test)]
mod recognition_support_tests{
    use super::*;
    fn allowed()->BTreeSet<String>{BTreeSet::from(["a".into(),"b".into()])}
    fn evidence(weak:&[(&str,u64)],positive:&[(&str,u64)],negative:&[(&str,u64)])->Evidence{
        let map=|x:&[(&str,u64)]|x.iter().map(|(k,n)|(k.to_string(),*n)).collect();
        Evidence{weak:map(weak),positive:map(positive),negative:map(negative),..Default::default()}
    }
    fn near(x:f64,y:f64){assert!((x-y).abs()<1e-9,"{x} != {y}");}
    #[test]
    fn large_order_cannot_outvote_many_independent_orders(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(evidence(&[("a",1000)],&[],&[]),&allowed,3.0).unwrap();
        for _ in 0..10{a.add(evidence(&[("b",1)],&[],&[]),&allowed,3.0).unwrap();}
        near(a.candidates["a"].support/a.weight,1.0/11.0);
        assert_eq!(a.supported_orders,11);
    }
    #[test]
    fn correction_is_one_mass_and_negative_is_separate(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(evidence(&[("a",1000)],&[("b",1)],&[("a",1)]),&allowed,3.0).unwrap();
        near(a.weight,1.0);near(a.candidates["a"].support,0.25);
        near(a.candidates["b"].support,0.75);near(a.candidates["a"].negative,0.75);
        assert_eq!(a.supported_orders,1);
    }
    #[test]
    fn outside_candidates_keep_their_mass_and_empty_is_not_zero_probability(){
        let mut a=Aggregate::default();let allowed=allowed();
        assert!(a.wire(&allowed,3.0)["candidates"][0]["supportShare"].is_null());
        a.add(evidence(&[("a",1),("outside",1)],&[],&[]),&allowed,3.0).unwrap();
        near(a.outside_support/a.weight,0.5);near(a.candidates["a"].support/a.weight,0.5);
        assert!(a.wire(&allowed,3.0)["calibratedProbability"].is_null());
    }
    #[test]
    fn weak_and_explicit_orders_have_distinct_budgets(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(evidence(&[("a",1)],&[],&[]),&allowed,3.0).unwrap();
        a.add(evidence(&[],&[("b",1)],&[("a",1)]),&allowed,3.0).unwrap();
        near(a.weight,4.0/3.0);near(a.candidates["b"].support/a.weight,0.75);
    }
}
