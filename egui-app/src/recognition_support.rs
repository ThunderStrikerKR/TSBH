//! P05 diagnostic evidence support. Order-normalized, not calibrated
//! probabilities. No effect on parser decisions, no inferred candidate.
use super::{cached_count,existing,Fact,FEATURE_VERSION,CONTRIBUTION_VERSION};
use rusqlite::{params,Transaction};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

const MAX_ORDERS:usize=50_000;
const MAX_FACTS:usize=2_000_000;
const SUPPORT_VERSION:i64=2;

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
        let mut weak_orders=BTreeMap::new();
        let mut positive_orders=BTreeMap::new();
        let mut negative_orders=BTreeMap::new();
        for (f,k,metric,candidate,amount) in facts{
            if f!=family||k!=key{continue;}
            let n=u64::try_from(*amount).ok().filter(|n|*n>0).ok_or("학습 증거량 오류")?;
            let is_map=matches!(metric.as_str(),"weakCandidates"|"explicitPositiveCandidates"|
                "explicitNegativeCandidates"|"weakCandidateOrders"|"explicitPositiveCandidateOrders"|
                "explicitNegativeCandidateOrders");
            if (is_map && candidate.trim().is_empty()) || (!is_map && !candidate.is_empty()){
                return Err("학습 후보 및 스칼라 규격 불일치".into());
            }
            match metric.as_str(){
                "weakCandidates"=>{e.weak.insert(candidate.clone(),n);},
                "explicitPositiveCandidates"=>{e.positive.insert(candidate.clone(),n);},
                "explicitNegativeCandidates"=>{e.negative.insert(candidate.clone(),n);},
                "weakCandidateOrders"=>{weak_orders.insert(candidate.clone(),n);},
                "explicitPositiveCandidateOrders"=>{positive_orders.insert(candidate.clone(),n);},
                "explicitNegativeCandidateOrders"=>{negative_orders.insert(candidate.clone(),n);},
                "observationCount"=>e.observations=n,
                "pendingCorrectionCount"=>e.pending=n,
                "unresolvedCount"=>e.unresolved=n,
                "independentOrders"=>e.independent=n,
                "weakIndependentOrders"=>e.weak_orders=n,
                "explicitCorrectionOrders"=>e.explicit_orders=n,
                "weakUnchangedCount"=>e.weak_count=n,
                "explicitCorrectionCount"=>e.correction_count=n,
                _=>return Err("지원하지 않는 학습 지표".into())
            }
        }
        // A sealed byte digest proves integrity, not semantic validity.
        // Each candidate seen in a single order must have exactly one order marker.
        let order_keys_match=|counts:&BTreeMap<String,u64>,orders:&BTreeMap<String,u64>|
            counts.len()==orders.len() && counts.keys().all(|k|orders.get(k)==Some(&1));
        if !order_keys_match(&e.weak,&weak_orders) || !order_keys_match(&e.positive,&positive_orders)
            || !order_keys_match(&e.negative,&negative_orders){
            return Err("주문별 후보 표본 카운터 불일치".into());
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
// One validation label per order, limited to a unique explicit correction
// with no unresolved or pending observations in this exact context.
// These are correction-conditional labels, not representative ground truth.
struct CorrectionSample{
    label:Option<String>,
    mass:BTreeMap<String,f64>,
    outside:f64,
}
fn correction_interval(success:u64,total:u64)->Value{
    if total==0{return Value::Null;}
    let n=total as f64;let p=success as f64/n;
    let z=1.959963984540054_f64;let z2=z*z;
    let center=(p+z2/(2.0*n))/(1.0+z2/n);
    let half=z*(p*(1.0-p)/n+z2/(4.0*n*n)).sqrt()/(1.0+z2/n);
    json!({"orders":success,"sampleOrders":total,"frequency":p,
        "wilson95Lower":(center-half).clamp(0.0,1.0),"wilson95Upper":(center+half).clamp(0.0,1.0)})
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
    correction_samples:Vec<CorrectionSample>,
    ambiguous_correction_orders:u64,
}
impl Aggregate{
    fn record_correction(&mut self,e:&Evidence,allowed:&BTreeSet<String>,w:f64,s:f64,weak:u64,strong:u64){
        if strong==0{return;}
        if e.positive.len()!=1 || e.pending!=0 || e.unresolved!=0{
            self.ambiguous_correction_orders+=1;
            return;
        }
        let label=e.positive.keys().next().filter(|k|allowed.contains(*k)).cloned();
        let mut sample=CorrectionSample{label,mass:BTreeMap::new(),outside:0.0};
        for (map,budget,count) in [(&e.weak,w,weak),(&e.positive,s,strong)]{
            if count==0{continue;}
            for (candidate,n) in map{
                let mass=budget*(*n as f64)/(count as f64);
                if allowed.contains(candidate){*sample.mass.entry(candidate.clone()).or_default()+=mass;}
                else{sample.outside+=mass;}
            }
        }
        self.correction_samples.push(sample);
    }
    fn correction_validation(&self,allowed:&BTreeSet<String>)->Value{
        let mut counts:BTreeMap<&str,u64>=BTreeMap::new();
        let mut outside_labels=0u64;
        let mut evaluated=0u64;let mut skipped=0u64;let mut tied=0u64;let mut correct=0u64;
        let mut brier=0.0;let mut true_share=0.0;
        for sample in &self.correction_samples{
            if let Some(label)=sample.label.as_deref(){*counts.entry(label).or_default()+=1;}
            else{outside_labels+=1;}
            // This sample has strong evidence, so its entire positive budget
            // is one. Subtract it before evaluating its explicit label.
            // The queried target was already excluded by the SQL read.
            let remaining=self.weight-1.0;
            if remaining<=1e-10{skipped+=1;continue;}
            let mut distribution=allowed.iter().map(|id|{
                let mass=self.candidates.get(id).map_or(0.0,|c|c.support)
                    -sample.mass.get(id).copied().unwrap_or(0.0);
                (Some(id.as_str()),mass.max(0.0))
            }).collect::<Vec<_>>();
            distribution.push((None,(self.outside_support-sample.outside).max(0.0)));
            let sum: f64=distribution.iter().map(|(_,m)|*m).sum();
            if sum<=1e-10{skipped+=1;continue;}
            let label=sample.label.as_deref();
            let mut top=-1.0_f64;let mut top_count=0;let mut top_correct=false;
            for (id,mass) in distribution{
                let p=mass/sum;let actual=id==label;
                brier+=(p-if actual{1.0}else{0.0}).powi(2);
                if actual{true_share+=p;}
                if p>top+1e-10{top=p;top_count=1;top_correct=actual;}
                else if (p-top).abs()<=1e-10{top_count+=1;}
            }
            evaluated+=1;
            if top_count!=1{tied+=1;}else if top_correct{correct+=1;}
        }
        let total=self.correction_samples.len() as u64;
        let ratio=|v:f64,n:u64|if n>0{Some(v/n as f64)}else{None};
        json!({"method":"leave_one_order_out_correction_only","labelOrders":total,
            "excludedAmbiguousCorrectionOrders":self.ambiguous_correction_orders,
            "candidateConditionalFrequencies":allowed.iter().map(|id|json!({"candidate":id,
                "estimate":correction_interval(*counts.get(id.as_str()).unwrap_or(&0),total)})).collect::<Vec<_>>(),
            "outsideCandidateEstimate":correction_interval(outside_labels,total),
            "evaluatedOrders":evaluated,"skippedNoTrainingEvidenceOrders":skipped,
            "tiedTopOrders":tied,"uniqueTopCorrectOrders":correct,
            "uniqueTopAccuracy":ratio(correct as f64,evaluated-tied),
            "multiclassBrierScore":ratio(brier,evaluated),
            "meanTrueLabelSupportShare":ratio(true_share,evaluated),
            "chronologicalHoldoutVerified":false,"representativeLabelsVerified":false,
            "intervalAssumesIndependentRepresentativeLabels":true,
            "calibratedProbability":null,"eligibleForAutomaticDecision":false})
    }
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
        self.record_correction(&e,allowed,w,s,weak,strong);
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
            "correctionValidation":self.correction_validation(allowed),
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
    fn valid_facts()->Vec<Fact>{
        let rows=[("observationCount","",4),("independentOrders","",1),
            ("weakIndependentOrders","",1),("weakUnchangedCount","",2),
            ("explicitCorrectionOrders","",1),("explicitCorrectionCount","",1),
            ("pendingCorrectionCount","",1),("weakCandidates","a",2),
            ("weakCandidateOrders","a",1),("explicitPositiveCandidates","b",1),
            ("explicitPositiveCandidateOrders","b",1),("explicitNegativeCandidates","a",1),
            ("explicitNegativeCandidateOrders","a",1)];
        rows.into_iter().map(|(metric,candidate,n)|
            ("fieldContextStatistics".into(),"ctx".into(),metric.into(),candidate.into(),n)).collect()
    }
    #[test]
    fn sealed_evidence_rejects_inconsistent_per_candidate_order_counts(){
        let good=valid_facts();
        let parse=|facts:&[Fact]|Evidence::from_facts(facts,"fieldContextStatistics","ctx");
        assert!(parse(&good).is_ok());
        for metric in ["weakCandidateOrders","explicitPositiveCandidateOrders","explicitNegativeCandidateOrders"]{
            let mut missing=good.clone();
            missing.retain(|f|f.2!=metric);
            assert!(parse(&missing).is_err(), "missing {metric}");
            let mut wrong=good.clone();
            wrong.iter_mut().find(|f|f.2==metric).unwrap().4=2;
            assert!(parse(&wrong).is_err(), "non-independent {metric}");
            let mut extra=good.clone();
            extra.push(("fieldContextStatistics".into(),"ctx".into(),metric.into(),"outside".into(),1));
            assert!(parse(&extra).is_err(), "unexpected candidate {metric}");
        }
        let mut scalar=good.clone();
        scalar.iter_mut().find(|f|f.2=="independentOrders").unwrap().3="a".into();
        assert!(parse(&scalar).is_err());
        let mut unknown=good.clone();
        unknown.push(("fieldContextStatistics".into(),"ctx".into(),"unknownMetric".into(),"".into(),1));
        assert!(parse(&unknown).is_err());
    }
    #[test]
    fn unresolved_and_pending_only_orders_never_supply_positive_mass(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(Evidence{pending:1,unresolved:2,..Default::default()},&allowed,3.0).unwrap();
        let wire=a.wire(&allowed,3.0);
        assert_eq!(wire["contextOrders"],1);
        assert_eq!(wire["supportedOrders"],0);
        assert_eq!(wire["noSupportOrders"],1);
        assert_eq!(wire["pendingCorrectionOrders"],1);
        assert_eq!(wire["unresolvedOrders"],1);
        assert!(wire["candidates"][0]["supportShare"].is_null());
        assert!(wire["weightEffectiveOrders"].is_null());
    }
    #[test]
    fn correction_validation_excludes_each_own_label_and_keeps_outside_mass(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(evidence(&[],&[("a",1)],&[("b",1)]),&allowed,3.0).unwrap();
        a.add(evidence(&[],&[("b",1)],&[("a",1)]),&allowed,3.0).unwrap();
        let v=a.wire(&allowed,3.0)["correctionValidation"].clone();
        assert_eq!(v["labelOrders"],2);assert_eq!(v["evaluatedOrders"],2);
        near(v["uniqueTopAccuracy"].as_f64().unwrap(),0.0);
        near(v["multiclassBrierScore"].as_f64().unwrap(),2.0);
        near(v["meanTrueLabelSupportShare"].as_f64().unwrap(),0.0);
        a.add(evidence(&[],&[("outside",1)],&[("b",1)]),&allowed,3.0).unwrap();
        let v=a.wire(&allowed,3.0)["correctionValidation"].clone();
        assert_eq!(v["outsideCandidateEstimate"]["orders"],1);
        assert_eq!(v["tiedTopOrders"],3);
        assert!(v["uniqueTopAccuracy"].is_null());
        assert!(v["calibratedProbability"].is_null());
    }
    #[test]
    fn correction_validation_excludes_unverified_and_ambiguous_labels(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(evidence(&[("a",1)],&[],&[]),&allowed,3.0).unwrap();
        a.add(evidence(&[],&[("a",1),("b",1)],&[("a",1),("b",1)]),&allowed,3.0).unwrap();
        let mut pending=evidence(&[],&[("a",1)],&[("b",1)]);pending.pending=1;
        a.add(pending,&allowed,3.0).unwrap();
        let v=a.wire(&allowed,3.0)["correctionValidation"].clone();
        assert_eq!(v["labelOrders"],0);
        assert_eq!(v["excludedAmbiguousCorrectionOrders"],2);
        assert!(v["multiclassBrierScore"].is_null());
        assert!(v["outsideCandidateEstimate"].is_null());
    }
    #[test]
    fn one_correction_cannot_validate_itself_or_claim_certainty(){
        let mut a=Aggregate::default();let allowed=allowed();
        a.add(evidence(&[],&[("a",1)],&[("b",1)]),&allowed,3.0).unwrap();
        let v=a.wire(&allowed,3.0)["correctionValidation"].clone();
        assert_eq!(v["evaluatedOrders"],0);
        assert_eq!(v["skippedNoTrainingEvidenceOrders"],1);
        let estimate=&v["candidateConditionalFrequencies"][0]["estimate"];
        near(estimate["frequency"].as_f64().unwrap(),1.0);
        assert!(estimate["wilson95Lower"].as_f64().unwrap()<0.25);
        assert_eq!(v["eligibleForAutomaticDecision"],false);
    }
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
