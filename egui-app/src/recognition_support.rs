//! P05 diagnostic evidence support. Order-normalized, not calibrated
//! probabilities. No effect on parser decisions, no inferred candidate.
use super::{verified_facts,VerifiedFacts,Fact,FEATURE_VERSION,CONTRIBUTION_VERSION};
use rusqlite::{params,Transaction};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

const MAX_ORDERS:usize=50_000;
const MAX_FACTS:usize=2_000_000;
const SUPPORT_VERSION:i64=4;

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

// P06 screening policy: deliberately provisional, not a probability or
// production admission rule. Independent prospective validation is still needed.
fn context_quality_policy()->Value{
    json!({"version":1,"status":"provisional_diagnostic",
        "minimumSupportedOrders":20,"minimumEffectiveOrders":15.0,
        "minimumExplicitLabelOrders":5,"minimumTopExplicitPositiveOrders":3,
        "minimumTopShare":0.65,"minimumMargin":0.20,"maximumOutsideShare":0.15,
        "maximumTopOppositionShare":0.10,"minimumConditionalValidationCoverage":0.80,
        "minimumConditionalValidationAgreement":0.70})
}
fn context_quality(report:&Value)->Value{
    let policy=context_quality_policy();
    let mut reasons=Vec::<&str>::new();
    let num=|key:&str|report[key].as_f64().filter(|v|v.is_finite()&&*v>=0.0);
    let supported=report["supportedOrders"].as_u64();
    let effective=num("weightEffectiveOrders");
    let explicit=report["correctionValidation"]["labelOrders"].as_u64();
    let validation=&report["correctionValidation"];
    let evaluated=validation["evaluatedOrders"].as_u64();
    let tied=validation["tiedTopOrders"].as_u64();
    let correct=validation["uniqueTopCorrectOrders"].as_u64();
    let validation_coverage=match (explicit,evaluated,tied,correct){
        (Some(n),Some(e),Some(t),Some(c)) if n>0&&e<=n&&t<=e&&c<=e-t=>Some((e-t) as f64/n as f64),
        _=>None
    };
    let validation_agreement=match (evaluated,tied,correct){
        (Some(e),Some(t),Some(c)) if e>t&&c<=e-t=>Some(c as f64/(e-t) as f64),
        _=>None
    };
    let mass=num("totalSupportMass").filter(|v|*v>0.0);
    let outside=num("outsideCandidateSupportMass");
    let outside_share=mass.zip(outside).map(|(m,o)|o/m);
    let mut ranked=Vec::new();
    let mut seen=BTreeSet::new();
    let mut valid=report["candidates"].as_array().is_some_and(|a|!a.is_empty()&&a.len()<=64);
    for c in report["candidates"].as_array().into_iter().flatten(){
        let id=c["candidate"].as_str().filter(|id|!id.is_empty());
        let support=c["supportMass"].as_f64().filter(|v|v.is_finite()&&*v>=0.0);
        if let (Some(id),Some(value))=(id,support){
            if seen.insert(id){ranked.push((id,value,c));}else{valid=false;}
        }else{valid=false;}
    }
    if let (Some(m),Some(o))=(mass,outside){
        valid&=(ranked.iter().map(|x|x.1).sum::<f64>()+o-m).abs()<=1e-7*m.max(1.0);
    }else{valid=false;}
    ranked.sort_by(|a,b|b.1.total_cmp(&a.1));
    let top=ranked.first().filter(|_|valid);
    let top_share=top.zip(mass).map(|(r,m)|r.1/m);
    let margin=top.zip(mass).zip(outside).map(|((r,m),o)|
        (r.1-ranked.get(1).map_or(0.0,|x|x.1).max(o))/m);
    let top_opposition=top.and_then(|r|r.2["explicitOppositionMass"].as_f64())
        .filter(|v|v.is_finite()&&*v>=0.0).zip(mass).map(|(o,m)|o/m);
    let top_explicit=top.and_then(|r|r.2["explicitPositiveOrders"].as_u64());
    if !valid{reasons.push("missing_or_inconsistent_support");}
    if report["pendingCorrectionOrders"].as_u64()!=Some(0)||
        report["unresolvedOrders"].as_u64()!=Some(0){reasons.push("pending_or_unresolved_evidence");}
    if supported.is_none_or(|n|n<20){reasons.push("insufficient_independent_orders");}
    if effective.is_none_or(|n|n<15.0){reasons.push("insufficient_effective_orders");}
    if explicit.is_none_or(|n|n<5){reasons.push("insufficient_explicit_labels");}
    if top_explicit.is_none_or(|n|n<3){reasons.push("insufficient_top_explicit_orders");}
    if top_share.is_none_or(|p|p<0.65){reasons.push("low_top_support");}
    if margin.is_none_or(|p|p<0.20-1e-10){reasons.push("small_or_negative_margin");}
    if outside_share.is_none_or(|p|p>0.15+1e-10){reasons.push("outside_candidate_risk");}
    if top_opposition.is_none_or(|p|p>0.10+1e-10){reasons.push("explicit_opposition");}
    if validation_coverage.is_none_or(|p|p<0.80){reasons.push("insufficient_conditional_validation_coverage");}
    if validation_agreement.is_none_or(|p|p<0.70){reasons.push("low_conditional_validation_agreement");}
    json!({"policy":policy,"status":if reasons.is_empty(){"diagnostic_screen_pass"}else{"insufficient_or_conflicting"},
        "reasons":reasons,"topCandidate":top.map(|r|r.0),"topSupportShare":top_share,
        "marginIncludingOutside":margin,"outsideSupportShare":outside_share,
        "topExplicitOppositionToSupportMass":top_opposition,"topExplicitPositiveOrders":top_explicit,
        "supportedOrders":supported,"effectiveOrders":effective,"explicitLabelOrders":explicit,
        "conditionalValidationCoverage":validation_coverage,"conditionalValidationAgreement":validation_agreement,
        "prospectivePerformanceVerified":false,"calibrationVerified":false,
        "representativeLabelsVerified":false,"confidenceProbability":null,
        "eligibleForAutomaticDecision":false})
}
// Choose exactly one context, never add correlated tiers together.
// Qualified disagreements block selection even when one tier is narrower.
// R01 diagnostic comparison: no fitted legacy score or blend weight is
// available for type yet. Never treat support share as a probability.
fn type_comparison(location:&Value,selection:&Value)->Value{
    let valid_kind=|s:&str|matches!(s,"name"|"address"|"phone"|"mobile"|"product"|"quantity"|"note");
    let allowed=location["candidates"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let baseline=location["legacyTypeCandidate"].as_str().filter(|s|valid_kind(s)&&allowed.contains(s));
    let top=selection["diagnosticCandidate"].as_str().filter(|s|valid_kind(s)&&allowed.contains(s));
    let complete=location["candidates"].as_array().is_some_and(|a|a.len()==allowed.len()&&a.len()<=64)
        &&allowed.iter().all(|s|valid_kind(s));
    let ready=baseline.is_some()&&complete&&allowed.len()>=2
        &&location["candidateCoverage"]=="traced_partial"
        &&selection["status"]=="diagnostic_context_selected"&&selection["contextConflict"]==false
        &&selection["selectedContextKey"].as_str().is_some()&&top.is_some();
    let status=if baseline.is_none(){"unverified_legacy_type"}
        else if !complete||allowed.len()<2||location["candidateCoverage"]!="traced_partial"{"insufficient_traced_alternatives"}
        else if !ready{"retain_legacy_no_qualified_context"}
        else if baseline==top{"diagnostic_agreement"}
        else if matches!(baseline,Some("phone"|"mobile"))&&matches!(top,Some("phone"|"mobile")){"contact_slot_not_type_change"}
        else{"diagnostic_disagreement"};
    json!({"schemaVersion":1,"status":status,"legacyCandidate":baseline,
        "learningCandidate":if ready{top}else{None},"candidateCoverage":location["candidateCoverage"],
        "selectedContextKey":selection["selectedContextKey"],"blendWeight":null,"calibratedProbability":null,
        "eligibleForAutomaticDecision":false,"decisionChanged":false})
}
pub(super) fn select_contexts(locations:&[Value],reports:&[Value])->Vec<Value>{
    locations.iter().map(|loc|{
        let mut levels=Vec::new();let mut passed=Vec::new();let mut seen=BTreeSet::new();
        let task=&loc["task"];
        let mut invalid=false;
        for key in loc["contextKeys"].as_array().into_iter().flatten(){
            let parsed=key.as_str().and_then(|k|serde_json::from_str::<Value>(k).ok());
            let level=parsed.as_ref().and_then(|v|v[2].as_str());
            let rank=match level{Some("client_structure")=>3,Some("structure")=>2,Some("base")=>1,_=>{invalid=true;continue;}};
            if parsed.as_ref().is_none_or(|v|v[1]!=*task)||!seen.insert(rank){invalid=true;continue;}
            let mut matched=reports.iter().filter(|r|r["contextKey"]==*key&&r["task"]==*task&&
                r["candidateIds"]==loc["candidates"]);
            let Some(report)=matched.next()else{invalid=true;continue;};
            if matched.next().is_some(){invalid=true;continue;}
            let q=&report["contextQuality"];
            if q["status"]=="diagnostic_screen_pass"{
                if let Some(id)=q["topCandidate"].as_str(){passed.push((rank,id,key.clone()));}else{invalid=true;}
            }
            levels.push(json!({"level":level,"contextKey":key,"quality":q}));
        }
        passed.sort_by_key(|r|std::cmp::Reverse(r.0));
        let conflict=passed.first().is_some_and(|first|passed.iter().any(|r|r.1!=first.1));
        let selected=if invalid||conflict{None}else{passed.first()};
        let mut result=json!({"start":loc["start"],"end":loc["end"],"task":task,
            "status":if invalid{"invalid_context_join"}else if conflict{"context_conflict"}else if selected.is_some(){"diagnostic_context_selected"}else{"retain_legacy_no_qualified_context"},
            "contextConflict":conflict,"selectedContextKey":selected.map(|r|&r.2),
            "diagnosticCandidate":selected.map(|r|r.1),"levels":levels,
            "overlappingContextsCombined":false,"eligibleForAutomaticDecision":false,"decisionChanged":false});
        if task=="type"{
            result["typeComparison"]=type_comparison(loc,&result);
        }
        result
    }).collect()
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

// Offline calibration of externally supplied, timestamped prediction records.
// Never claim that caller timestamps/labels are authenticated by the ledger.
// Calibrate TOP candidate correctness, not the full multiclass distribution.
const CALIBRATION_BINS:usize=10;
fn chronological_calibration(input:&Value,allowed:&BTreeSet<String>,key:&str,target:&str,ratio:f64)->Result<Value,String>{
    use chrono::DateTime;
    let date=|v:&Value|v.as_str().and_then(|s|DateTime::parse_from_rfc3339(s).ok())
        .ok_or("검증 시각은 RFC3339 형식이어야 합니다.".to_owned());
    let calibration_start=date(&input["calibrationStart"])?;
    let test_start=date(&input["testStart"])?;
    let test_end=date(&input["testEnd"])?;
    let evaluation_as_of=date(&input["evaluationAsOf"])?;
    if !(calibration_start<test_start && test_start<test_end && test_end<=evaluation_as_of){
        return Err("학습·보정·평가 시각 순서 오류".into());
    }
    let model=input["frozenModelId"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=256)
        .ok_or("고정 학습 모델 ID 필요")?;
    let training_through=date(&input["trainingDataThrough"])?;
    if training_through>=calibration_start{return Err("학습 종료 시각은 보정 시작 이전이어야 합니다.".into());}
    let samples=input["samples"].as_array().filter(|a|a.len()<=MAX_ORDERS).ok_or("시간순 검증 표본 한도 오류")?;
    let mut seen=BTreeSet::new();
    let mut fit=[(0u64,0u64);CALIBRATION_BINS];
    let mut test=Vec::new();
    let mut late_fit=0u64;let mut late_test=0u64;let mut excluded_target=0u64;
    for row in samples{
        let id=row["orderId"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=1024)
            .ok_or("검증 주문 ID 오류")?;
        if !seen.insert(id){return Err("검증 주문 중복".into());}
        if id==target{excluded_target+=1;continue;}
        if row["frozenModelId"].as_str()!=Some(model)||row["contextKey"].as_str()!=Some(key)
            ||row["explicitToWeakRatio"].as_f64()!=Some(ratio){
            return Err("검증 모델·문맥·가중치 규격 불일치".into());
        }
        if row["labelKind"]!="explicit_correction"{return Err("명시적 정정 검증 표본만 허용합니다.".into());}
        let predicted=date(&row["predictedAt"])?;
        let observed=date(&row["labelAvailableAt"])?;
        if predicted<training_through || predicted<calibration_start || predicted>=test_end || observed<=predicted{
            return Err("예측·정답 확정 시각 범위 오류".into());
        }
        let top=row["topCandidate"].as_str().filter(|s|allowed.contains(*s)).ok_or("현재 최상위 후보 오류")?;
        let actual=row["actualCandidate"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=1024)
            .ok_or("정정 정답 후보 오류")?;
        let score=row["topSupportShare"].as_f64().filter(|p|p.is_finite()&&(0.0..=1.0).contains(p))
            .ok_or("검증 지지도 범위 오류")?;
        let bin=((score*CALIBRATION_BINS as f64).floor() as usize).min(CALIBRATION_BINS-1);
        let correct=top==actual;
        if predicted<test_start{
            // Even a calibration-window prediction cannot fit the mapping
            // if its label was only available once evaluation started.
            if observed>=test_start{late_fit+=1;continue;}
            fit[bin].0+=1;fit[bin].1+=u64::from(correct);
        }else{
            if observed>evaluation_as_of{late_test+=1;continue;}
            test.push((bin,score,correct));
        }
    }
    let fitted:u64=fit.iter().map(|(n,_)|*n).sum();
    let test_count=test.len() as u64;
    let mut evaluated=0u64;let mut uncovered=0u64;
    let mut raw_brier=0.0;let mut calibrated_brier=0.0;
    for (bin,score,correct) in test{
        let (n,k)=fit[bin];
        if n==0{uncovered+=1;continue;}
        let p=k as f64/n as f64;let y=f64::from(u8::from(correct));
        raw_brier+=(score-y).powi(2);calibrated_brier+=(p-y).powi(2);
        evaluated+=1;
    }
    let mean=|v:f64|if evaluated==0{None}else{Some(v/evaluated as f64)};
    let bins=fit.iter().enumerate().map(|(i,(n,k))|json!({
        "lowerInclusive":i as f64/CALIBRATION_BINS as f64,
        "upper":(i+1) as f64/CALIBRATION_BINS as f64,
        "upperInclusive":i==CALIBRATION_BINS-1,
        "estimate":correction_interval(*k,*n)
    })).collect::<Vec<_>>();
    Ok(json!({"method":"fixed_bin_top_candidate_calibration","status":"external_replay_diagnostic_only",
        "frozenModelId":model,"fitOrders":fitted,"testOrders":test_count,
        "excludedTargetOrders":excluded_target,"lateCalibrationLabelOrders":late_fit,
        "lateEvaluationLabelOrders":late_test,"evaluatedOrders":evaluated,"uncoveredTestOrders":uncovered,
        "testCoverage":if test_count>0{Some(evaluated as f64/test_count as f64)}else{None},
        "rawBrierScoreOnCoveredTest":mean(raw_brier),"calibratedBrierScoreOnCoveredTest":mean(calibrated_brier),
        "bins":bins,"chronologicalInputChecksPassed":true,
        "sourceProvenanceVerified":false,"chronologicalHoldoutVerified":false,
        "representativeLabelsVerified":false,"calibratedProbability":null,
        "eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// Selected current authoritative snapshots, not archived historical model state.
// Never accept predictions or labels from the caller on this path.
pub(super) fn local_replay(tx:&Transaction<'_>,scope:&str,input:&Value,
    orders:&[Value],digests:&BTreeMap<String,String>)->Result<Value,String>{
    use chrono::DateTime;
    use sha2::{Digest,Sha256};
    let (family,key,target,allowed,ratio)=request(input)?;
    let config=&input["localChronologicalReplay"];
    let date=|v:&Value|v.as_str().and_then(|s|DateTime::parse_from_rfc3339(s).ok());
    let cutoff=date(&config["trainingDataThrough"]).ok_or("학습 종료 시각 오류")?;
    let start=date(&config["calibrationStart"]).ok_or("보정 시작 시각 오류")?;
    let test_start=date(&config["testStart"]).ok_or("검증 시작 시각 오류")?;
    let end=date(&config["testEnd"]).ok_or("검증 종료 시각 오류")?;
    let as_of=date(&config["evaluationAsOf"]).ok_or("평가 시각 오류")?;
    if !(cutoff<start && start<test_start && test_start<end && end<=as_of){
        return Err("학습·보정·검증 시각 순서 오류".into());
    }
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending!=0{return Err("복구 대기 학습 주문이 있습니다.".into());}
    let mut train=Aggregate::default();
    let mut evaluation=Vec::new();
    let mut missing_time=0u64;let mut no_context=0u64;let mut excluded=0u64;
    let mut ambiguous=0u64;let mut gap=0u64;
    let mut train_digests=BTreeMap::new();
    for order in orders{
        let id=order["id"].as_str().ok_or("확정 주문 ID 누락")?;
        if id==target || order["learningActive"]!=true ||
            order["deletedAt"].as_str().is_some_and(|s|!s.is_empty()){
            excluded+=1;continue;
        }
        let digest=digests.get(id).ok_or("학습 원본 지문 누락")?;
        let facts=verified_facts(tx,scope,id,digest)?.ok_or("학습 원장이 최신 원본과 불일치합니다.")?;
        let slice=facts.as_slice();
        let start_index=slice.partition_point(|(f,k,_,_,_)|(f.as_str(),k.as_str())<(family,key));
        let end_index=slice.partition_point(|(f,k,_,_,_)|(f.as_str(),k.as_str())<=(family,key));
        if start_index==end_index{no_context+=1;continue;}
        let evidence=Evidence::from_facts(&slice[start_index..end_index],family,key)?;
        let snapshot=&order["confirmationSnapshot"];
        let capture=&snapshot["recognitionBaseline"]["initialAnalysis"]["initialPredictionCapture"];
        let times=(date(&capture["analysisStartedAt"]),date(&capture["analysisCompletedAt"]),date(&order["confirmedAt"]));
        let (Some(began),Some(predicted),Some(confirmed))=times else{missing_time+=1;continue;};
        if capture["schemaVersion"]!=1 || capture["origin"]!="native_reception_worker" ||
            capture["orderId"].as_str()!=Some(id) || snapshot["at"]!=order["confirmedAt"] ||
            began>predicted || predicted>=confirmed{
            missing_time+=1;continue;
        }
        // All audit events must be after the initial recognition, and no
        // later than the frozen confirmation. Cache proof checks manifest
        // coverage and exact source bytes through the caller's digest.
        let Some(events)=order["recognitionAuditEvents"].as_array()else{missing_time+=1;continue;};
        if events.iter().any(|e|date(&e["at"]).is_none_or(|t|t<=predicted||t>confirmed)){
            missing_time+=1;continue;
        }
        if confirmed<=cutoff{
            train_digests.insert(id.to_owned(),digest.clone());
            train.add(evidence,&allowed,ratio)?;
        }else if predicted>=start && predicted<end{
            if evidence.positive.len()!=1 || evidence.pending!=0 || evidence.unresolved!=0{
                ambiguous+=1;continue;
            }
            evaluation.push((id.to_owned(),predicted,confirmed,evidence.positive.keys().next().unwrap().clone()));
        }else{gap+=1;}
    }
    // ID binds the exact currently verified training inputs, query and
    // algorithm version. It does not imply that this model existed then.
    let mut hash=Sha256::new();
    hash.update(serde_json::to_vec(&json!([SUPPORT_VERSION,FEATURE_VERSION,CONTRIBUTION_VERSION,
        key,allowed,ratio,config["trainingDataThrough"],train_digests])).map_err(|_|"학습 집합 지문 오류")?);
    let model=format!("replay-{:x}",hash.finalize());
    let mut ranking=allowed.iter().map(|id|(id.as_str(),train.candidates.get(id).map_or(0.0,|c|c.support)))
        .collect::<Vec<_>>();
    ranking.sort_by(|a,b|b.1.total_cmp(&a.1));
    let winner=ranking.first().filter(|(_,mass)|train.weight>0.0 && *mass>train.outside_support+1e-10 &&
        ranking.get(1).is_none_or(|other|*mass>other.1+1e-10));
    let mut samples=Vec::new();
    if let Some((top,mass))=winner{
        for (id,predicted,confirmed,actual) in &evaluation{
            samples.push(json!({"orderId":id,"frozenModelId":model,"contextKey":key,
                "explicitToWeakRatio":ratio,"labelKind":"explicit_correction",
                "predictedAt":predicted.to_rfc3339(),"labelAvailableAt":confirmed.to_rfc3339(),
                "topCandidate":top,"actualCandidate":actual,"topSupportShare":(mass/train.weight).clamp(0.0,1.0)}));
        }
    }
    let mut config=config.clone();
    config["frozenModelId"]=json!(model);
    config["samples"]=json!(samples);
    let mut report=chronological_calibration(&config,&allowed,key,target,ratio)?;
    report["status"]=json!("local_snapshot_replay_diagnostic_only");
    report["currentSourceDigestsVerified"]=json!(true);
    report["historicalModelVerified"]=json!(false);
    report["trainingOrders"]=json!(train_digests.len());
    report["trainingSupportedOrders"]=json!(train.supported_orders);
    report["excludedMissingOrInvalidTimeOrders"]=json!(missing_time);
    report["excludedNoContextOrders"]=json!(no_context);
    report["excludedInactiveOrTargetOrders"]=json!(excluded);
    report["excludedAmbiguousLabelOrders"]=json!(ambiguous);
    report["excludedOutsideWindowsOrders"]=json!(gap);
    report["noUniquePredictionOrders"]=json!(evaluation.len()-samples.len());
    report["selectedCurrentSnapshots"]=json!(orders.len());
    report["fullHistoryReconciled"]=json!(false);
    Ok(report)
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
        let facts=verified_facts(tx,scope,&id,&digest)?.filter(|f|f.len()>0)
            .ok_or("검증되지 않은 학습 기여량")?;
        let facts=facts.as_slice();
        checked_facts=checked_facts.saturating_add(facts.len());
        if checked_facts>MAX_FACTS{return Err("학습 지지도 조회 기여량 한도 초과".into());}
        result.add(Evidence::from_facts(facts,family,key)?,&allowed,ratio)?;
    }
    finish_assessment(result,input,&allowed,ratio,key,target)
}
// All members came from the same transaction's context query and source
// verification. No per-context SQL or hashing on this path.
pub(super) fn assess_verified(input:&Value,contexts:&BTreeMap<String,Vec<String>>,
    facts:&BTreeMap<String,VerifiedFacts>)->Result<Value,String>{
    let (family,key,target,allowed,ratio)=request(input)?;
    let ids=contexts.get(key).ok_or("검증된 문맥 목록 누락")?;
    let mut result=Aggregate::default();let mut checked_facts=0usize;
    let mut seen=BTreeSet::new();
    for id in ids{
        if id==target||!seen.insert(id){return Err("검증된 문맥 주문 범위 오류".into());}
        let f=facts.get(id).filter(|f|f.len()>0).ok_or("검증된 기여량 누락")?;
        checked_facts=checked_facts.saturating_add(f.len());
        if checked_facts>MAX_FACTS||seen.len()>MAX_ORDERS{return Err("학습 지지도 조회 한도 초과".into());}
        // Facts are sorted by (family,key,metric,candidate). Skip unrelated
        // contexts without repeatedly scanning all facts of the order.
        let slice=f.as_slice();
        let start=slice.partition_point(|(f,k,_,_,_)|(f.as_str(),k.as_str())<(family,key));
        let end=slice.partition_point(|(f,k,_,_,_)|(f.as_str(),k.as_str())<=(family,key));
        result.add(Evidence::from_facts(&slice[start..end],family,key)?,&allowed,ratio)?;
    }
    finish_assessment(result,input,&allowed,ratio,key,target)
}
fn finish_assessment(result:Aggregate,input:&Value,allowed:&BTreeSet<String>,ratio:f64,key:&str,target:&str)->Result<Value,String>{
    let mut report=result.wire(allowed,ratio);
    report["contextQuality"]=context_quality(&report);
    if let Some(validation)=input.get("chronologicalValidation"){
        report["chronologicalCalibration"]=chronological_calibration(validation,allowed,key,target,ratio)?;
    }
    Ok(report)
}

#[cfg(test)]
mod recognition_support_tests{
    use super::*;
    fn allowed()->BTreeSet<String>{BTreeSet::from(["a".into(),"b".into()])}
    #[test]
    fn context_quality_is_diagnostic_and_blocks_weak_or_conflicting_evidence(){
        let mut a=Aggregate::default();let ids=allowed();
        for _ in 0..20{a.add(evidence(&[],&[("a",1)],&[("b",1)]),&ids,3.0).unwrap();}
        let r=a.wire(&ids,3.0);
        let q=context_quality(&r);
        assert_eq!(q["status"],"diagnostic_screen_pass");
        assert_eq!(q["eligibleForAutomaticDecision"],false);
        assert!(q["confidenceProbability"].is_null());
        let mut weak=Aggregate::default();
        for _ in 0..20{weak.add(evidence(&[("a",1)],&[],&[]),&ids,3.0).unwrap();}
        let q=context_quality(&weak.wire(&ids,3.0));
        assert_eq!(q["status"],"insufficient_or_conflicting");
        assert!(q["reasons"].as_array().unwrap().iter().any(|r|r=="insufficient_explicit_labels"));
        let mut bad=r.clone();bad["pendingCorrectionOrders"]=json!(1);
        assert_eq!(context_quality(&bad)["status"],"insufficient_or_conflicting");
        bad=r.clone();bad["outsideCandidateSupportMass"]=json!(10.0);
        assert_eq!(context_quality(&bad)["status"],"insufficient_or_conflicting");
    }
    #[test]
    fn context_selection_falls_back_without_summing_and_blocks_qualified_conflicts(){
        let base=json!([1,"type","base",null,[]]).to_string();
        let structure=json!([1,"type","structure",null,[]]).to_string();
        let location=json!({"start":0,"end":1,"task":"type","candidates":["a","b"],
            "contextKeys":[base,structure]});
        let report=|k:&str,status:&str,id:&str|json!({"contextKey":k,"task":"type","candidateIds":["a","b"],
            "contextQuality":{"status":status,"topCandidate":id}});
        let mut reports=vec![report(&base,"diagnostic_screen_pass","a"),report(&structure,"insufficient_or_conflicting","b")];
        let chosen=select_contexts(&[location.clone()],&reports);
        assert_eq!(chosen[0]["selectedContextKey"],base);
        reports[1]=report(&structure,"diagnostic_screen_pass","a");
        let chosen=select_contexts(&[location.clone()],&reports);
        assert_eq!(chosen[0]["selectedContextKey"],structure);
        assert_eq!(chosen[0]["overlappingContextsCombined"],false);
        reports[1]=report(&structure,"diagnostic_screen_pass","b");
        let chosen=select_contexts(&[location],&reports);
        assert_eq!(chosen[0]["status"],"context_conflict");
        assert!(chosen[0]["diagnosticCandidate"].is_null());
        assert_eq!(chosen[0]["decisionChanged"],false);
    }
    #[test]
    fn type_comparison_requires_traced_candidates_and_qualified_context(){
        let key=json!([1,"type","base",null,[]]).to_string();
        let mut loc=json!({"start":0,"end":3,"task":"type","candidates":["name","product"],
            "legacyTypeCandidate":"product","candidateCoverage":"traced_partial","contextKeys":[key]});
        let mut report=json!({"task":"type","contextKey":key,"candidateIds":["name","product"],
            "contextQuality":{"status":"diagnostic_screen_pass","topCandidate":"name"}});
        let check=|loc:&Value,r:&Value|select_contexts(std::slice::from_ref(loc),std::slice::from_ref(r))[0]["typeComparison"].clone();
        let v=check(&loc,&report);
        assert_eq!(v["status"],"diagnostic_disagreement");
        assert_eq!(v["legacyCandidate"],"product");assert_eq!(v["learningCandidate"],"name");
        assert_eq!(v["decisionChanged"],false);assert!(v["blendWeight"].is_null());
        report["contextQuality"]["topCandidate"]=json!("product");
        assert_eq!(check(&loc,&report)["status"],"diagnostic_agreement");
        loc["candidateCoverage"]=json!("observed_only");
        assert_eq!(check(&loc,&report)["status"],"insufficient_traced_alternatives");
        loc["candidateCoverage"]=json!("traced_partial");
        loc["legacyTypeCandidate"]=Value::Null;
        assert_eq!(check(&loc,&report)["status"],"unverified_legacy_type");
        loc["legacyTypeCandidate"]=json!("product");
        report["contextQuality"]["topCandidate"]=json!("outside");
        assert_eq!(check(&loc,&report)["status"],"retain_legacy_no_qualified_context");
        loc["candidates"]=json!(["mobile","phone"]);loc["legacyTypeCandidate"]=json!("mobile");
        report["candidateIds"]=loc["candidates"].clone();report["contextQuality"]["topCandidate"]=json!("phone");
        assert_eq!(check(&loc,&report)["status"],"contact_slot_not_type_change");
    }
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
    fn chronological_fixture()->Value{
        let sample=|id:&str,predicted:&str,observed:&str,actual:&str,score:f64|json!({
            "orderId":id,"frozenModelId":"m1","contextKey":"ctx","explicitToWeakRatio":3.0,
            "labelKind":"explicit_correction","predictedAt":predicted,"labelAvailableAt":observed,
            "topCandidate":"a","actualCandidate":actual,"topSupportShare":score});
        json!({"frozenModelId":"m1","trainingDataThrough":"2026-01-01T00:00:00Z",
            "calibrationStart":"2026-02-01T00:00:00Z","testStart":"2026-03-01T00:00:00Z",
            "testEnd":"2026-04-01T00:00:00Z","evaluationAsOf":"2026-04-02T00:00:00Z",
            "samples":[
                sample("fit","2026-02-02T00:00:00Z","2026-02-03T00:00:00Z","a",0.85),
                sample("late","2026-02-04T00:00:00Z","2026-03-01T00:00:00Z","b",0.85),
                sample("test","2026-03-02T00:00:00Z","2026-03-03T00:00:00Z","b",0.85),
                sample("uncovered","2026-03-03T00:00:00Z","2026-03-04T00:00:00Z","a",0.55),
                sample("future","2026-03-03T00:00:00Z","2026-04-03T00:00:00Z","a",0.85),
                sample("target","2026-02-02T00:00:00Z","2026-02-03T00:00:00Z","b",0.85)
            ]})
    }
    #[test]
    fn chronological_calibration_never_fits_test_late_or_target_labels(){
        let v=chronological_calibration(&chronological_fixture(),&allowed(),"ctx","target",3.0).unwrap();
        assert_eq!(v["fitOrders"],1);assert_eq!(v["testOrders"],2);
        assert_eq!(v["lateCalibrationLabelOrders"],1);assert_eq!(v["lateEvaluationLabelOrders"],1);
        assert_eq!(v["excludedTargetOrders"],1);assert_eq!(v["evaluatedOrders"],1);
        assert_eq!(v["uncoveredTestOrders"],1);
        near(v["testCoverage"].as_f64().unwrap(),0.5);
        near(v["rawBrierScoreOnCoveredTest"].as_f64().unwrap(),0.85_f64.powi(2));
        near(v["calibratedBrierScoreOnCoveredTest"].as_f64().unwrap(),1.0);
        assert_eq!(v["sourceProvenanceVerified"],false);
        assert_eq!(v["eligibleForAutomaticDecision"],false);
    }
    #[test]
    fn chronological_calibration_rejects_duplicate_leaking_or_mismatched_samples(){
        let base=chronological_fixture();let parse=|v:&Value|chronological_calibration(v,&allowed(),"ctx","target",3.0);
        let mut duplicate=base.clone();duplicate["samples"][1]["orderId"]=json!("fit");
        assert!(parse(&duplicate).is_err());
        for (field,value) in [("labelAvailableAt",json!("2026-02-01T00:00:00Z")),
            ("predictedAt",json!("2026-01-01T00:00:00Z")),("contextKey",json!("other")),
            ("frozenModelId",json!("m2")),("explicitToWeakRatio",json!(2.0)),
            ("labelKind",json!("weak_unchanged")),("topSupportShare",json!(1.1))]{
            let mut bad=base.clone();bad["samples"][0][field]=value;
            assert!(parse(&bad).is_err(),"{field}");
        }
        let mut leak=base.clone();leak["trainingDataThrough"]=leak["calibrationStart"].clone();
        assert!(parse(&leak).is_err());
        let mut empty=base.clone();empty["samples"]=json!([]);
        let v=parse(&empty).unwrap();
        assert!(v["rawBrierScoreOnCoveredTest"].is_null());
        assert!(v["testCoverage"].is_null());
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
