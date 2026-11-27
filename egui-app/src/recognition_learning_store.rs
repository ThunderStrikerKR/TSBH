//! Versioned derived recognition counts. No source document or recipient
//! values are copied into this store. Never use this diagnostic cache to
//! change a recognition decision until lifecycle reconciliation is wired.
use rusqlite::{params,Connection,OptionalExtension,Transaction};
use sha2::{Digest,Sha256};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

#[path="recognition_support.rs"]
mod recognition_support;

const FEATURE_VERSION:i64=1;
// Bump whenever the meaning of a contribution or its input changes,
// even if the public context feature keys stay the same.
const CONTRIBUTION_VERSION:i64=2;
const SCALARS:[&str;7]=["observationCount","independentOrders","weakIndependentOrders",
    "weakUnchangedCount","explicitCorrectionCount","explicitCorrectionOrders","pendingCorrectionCount"];
const MAPS:[&str;6]=["weakCandidates","explicitPositiveCandidates","explicitNegativeCandidates",
    "weakCandidateOrders","explicitPositiveCandidateOrders","explicitNegativeCandidateOrders"];
pub(super) type Fact=(String,String,String,String,i64);
// Only constructed by verification of the source fingerprint and fact proof.
// The foreground caller retains it for a single audit read transaction.
pub(super) struct VerifiedFacts(Vec<Fact>);
impl VerifiedFacts{
    pub(super) fn len(&self)->usize{self.0.len()}
    pub(super) fn memory_bytes(&self)->usize{
        self.0.iter().fold(self.0.capacity().saturating_mul(std::mem::size_of::<Fact>()),|n,(f,k,m,c,_)|
            n.saturating_add(f.capacity()).saturating_add(k.capacity()).saturating_add(m.capacity()).saturating_add(c.capacity()))
    }
    fn as_slice(&self)->&[Fact]{&self.0}
}

pub(super) fn prepare(conn:&Connection)->Result<(),String>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS recognition_learning_meta(
        key TEXT PRIMARY KEY,value INTEGER NOT NULL);
        INSERT OR IGNORE INTO recognition_learning_meta VALUES('schema',1);
        CREATE TABLE IF NOT EXISTS recognition_learning_orders(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,order_id TEXT NOT NULL,
        PRIMARY KEY(scope,feature_version,order_id));
        CREATE TABLE IF NOT EXISTS recognition_learning_facts(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,order_id TEXT NOT NULL,
        family TEXT NOT NULL,context_key TEXT NOT NULL,metric TEXT NOT NULL,candidate TEXT NOT NULL,
        amount INTEGER NOT NULL CHECK(typeof(amount)='integer' AND amount>0),
        PRIMARY KEY(scope,feature_version,order_id,family,context_key,metric,candidate),
        FOREIGN KEY(scope,feature_version,order_id) REFERENCES recognition_learning_orders
          (scope,feature_version,order_id) ON DELETE CASCADE);
        CREATE TABLE IF NOT EXISTS recognition_learning_totals(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,family TEXT NOT NULL,
        context_key TEXT NOT NULL,metric TEXT NOT NULL,candidate TEXT NOT NULL,
        amount INTEGER NOT NULL CHECK(typeof(amount)='integer' AND amount>=0),
        PRIMARY KEY(scope,feature_version,family,context_key,metric,candidate));
        CREATE TABLE IF NOT EXISTS recognition_learning_fingerprints(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,order_id TEXT NOT NULL,
        contribution_version INTEGER NOT NULL,
        fingerprint TEXT NOT NULL CHECK(length(fingerprint)=64),
        fact_count INTEGER NOT NULL CHECK(typeof(fact_count)='integer' AND fact_count>=0),
        PRIMARY KEY(scope,feature_version,order_id));
        CREATE TABLE IF NOT EXISTS recognition_learning_fact_proofs(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,order_id TEXT NOT NULL,
        digest TEXT NOT NULL CHECK(length(digest)=64),
        PRIMARY KEY(scope,feature_version,order_id),
        FOREIGN KEY(scope,feature_version,order_id) REFERENCES recognition_learning_fingerprints
          (scope,feature_version,order_id) ON DELETE CASCADE);
        CREATE TABLE IF NOT EXISTS recognition_learning_pending(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,order_id TEXT NOT NULL,
        PRIMARY KEY(scope,feature_version,order_id));
        CREATE TABLE IF NOT EXISTS recognition_combined_evaluation(
        scope TEXT NOT NULL,feature_version INTEGER NOT NULL,order_id TEXT NOT NULL,
        evaluation_version INTEGER NOT NULL CHECK(evaluation_version=1),
        source_digest TEXT NOT NULL,payload TEXT NOT NULL,payload_digest TEXT NOT NULL,
        PRIMARY KEY(scope,feature_version,order_id),
        FOREIGN KEY(scope,feature_version,order_id) REFERENCES recognition_learning_fingerprints
        (scope,feature_version,order_id) ON DELETE CASCADE);
        CREATE INDEX IF NOT EXISTS recognition_learning_facts_context
        ON recognition_learning_facts(scope,feature_version,family,context_key,order_id);")
        .map_err(|e|e.to_string())?;
    let version:i64=conn.query_row("SELECT value FROM recognition_learning_meta WHERE key='schema'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if version!=1{return Err("지원하지 않는 학습 저장소 스키마입니다.".into());}
    Ok(())
}

fn facts(report:&Value)->Result<Vec<Fact>,String>{
    if report["featureVersion"]!=FEATURE_VERSION || report["uniqueOrders"]!=1 ||
        report["status"]!="diagnostic_only" || report["eligibleForDurableLearning"]!=false{
        return Err("학습 기여량 규격이 다릅니다.".into());
    }
    if report["auditCoverage"]["verifiedManifestOrders"]!=1{return Ok(Vec::new());}
    let mut out=BTreeMap::new();
    for (family,key_field) in [("statistics","key"),("fieldStatistics","task"),("fieldContextStatistics","key")]{
        for row in report[family].as_array().ok_or("학습 통계 목록 오류")?{
            let key=row[key_field].as_str().filter(|v|!v.is_empty()&&v.len()<=8192).ok_or("학습 문맥 키 오류")?;
            let counts=row["counts"].as_object().ok_or("학습 카운터 오류")?;
            if counts.get("independentOrders").and_then(Value::as_u64)!=Some(1) ||
                counts.get("eligibleForAutomaticDecision")!=Some(&Value::Bool(false)){
                return Err("주문별 독립 표본 규격 오류".into());
            }
            let mut insert=|metric:&str,candidate:&str,value:&Value|->Result<(),String>{
                let n=value.as_u64().and_then(|v|i64::try_from(v).ok()).ok_or("학습 카운터 범위 오류")?;
                if candidate.len()>1024{return Err("학습 후보 키 한도 초과".into());}
                if n>0 && out.insert((family.to_owned(),key.to_owned(),metric.to_owned(),candidate.to_owned()),n).is_some(){
                    return Err("중복 학습 기여량".into());
                }
                if out.len()>100_000{return Err("주문별 학습 기여량 한도 초과".into());}
                Ok(())
            };
            for metric in SCALARS.into_iter().chain(["unresolvedCount"]){
                insert(metric,"",counts.get(metric).ok_or("학습 카운터 누락")?)?;
            }
            for metric in MAPS{
                for (candidate,value) in counts.get(metric).and_then(Value::as_object).ok_or("학습 후보 카운터 누락")?{
                    if candidate.is_empty(){return Err("학습 후보 키 누락".into());}
                    insert(metric,candidate,value)?;
                }
            }
        }
    }
    Ok(out.into_iter().map(|((family,key,metric,candidate),n)|(family,key,metric,candidate,n)).collect())
}

fn existing(tx:&Transaction<'_>,scope:&str,id:&str)->Result<Vec<Fact>,String>{
    let mut stmt=tx.prepare("SELECT family,context_key,metric,candidate,amount FROM recognition_learning_facts
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3 ORDER BY family,context_key,metric,candidate")
        .map_err(|e|e.to_string())?;
    let rows=stmt.query_map(params![scope,FEATURE_VERSION,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
        .map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}

fn replace(tx:&Transaction<'_>,scope:&str,id:&str,new:&[Fact])->Result<bool,String>{
    let old=existing(tx,scope,id)?;
    // A previously sealed contribution must be verified even on a source
    // cache miss or inactive order. Never subtract corrupted derived facts.
    let proof:Option<String>=tx.query_row("SELECT digest FROM recognition_learning_fact_proofs
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    if let Some(proof)=proof{
        if fact_proof(&old)?!=proof{return Err("기존 학습 기여 내용 손상".into());}
    }
    if old==new{return Ok(false);}
    // Keep unchanged facts and totals intact; only apply the sorted diff.
    let key=|f:&Fact|(f.0.clone(),f.1.clone(),f.2.clone(),f.3.clone());
    let previous=old.iter().map(|f|(key(f),f.4)).collect::<BTreeMap<_,_>>();
    let next=new.iter().map(|f|(key(f),f.4)).collect::<BTreeMap<_,_>>();
    if !new.is_empty()&&old.is_empty(){
        tx.execute("INSERT INTO recognition_learning_orders VALUES(?1,?2,?3)",params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    }
    for ((family,k,metric,candidate),n) in &previous{
        if next.get(&(family.clone(),k.clone(),metric.clone(),candidate.clone()))==Some(n){continue;}
        let changed=tx.execute("UPDATE recognition_learning_totals SET amount=amount-?7
            WHERE scope=?1 AND feature_version=?2 AND family=?3 AND context_key=?4 AND metric=?5 AND candidate=?6 AND amount>=?7",
            params![scope,FEATURE_VERSION,family,k,metric,candidate,n]).map_err(|e|e.to_string())?;
        if changed!=1{return Err("기존 학습 합계 불일치: 갱신을 중단했습니다.".into());}
        tx.execute("DELETE FROM recognition_learning_facts WHERE scope=?1 AND feature_version=?2 AND order_id=?3
            AND family=?4 AND context_key=?5 AND metric=?6 AND candidate=?7",
            params![scope,FEATURE_VERSION,id,family,k,metric,candidate]).map_err(|e|e.to_string())?;
        tx.execute("DELETE FROM recognition_learning_totals WHERE scope=?1 AND feature_version=?2
            AND family=?3 AND context_key=?4 AND metric=?5 AND candidate=?6 AND amount=0",
            params![scope,FEATURE_VERSION,family,k,metric,candidate]).map_err(|e|e.to_string())?;
    }
    for ((family,k,metric,candidate),n) in &next{
        if previous.get(&(family.clone(),k.clone(),metric.clone(),candidate.clone()))==Some(n){continue;}
        tx.execute("INSERT INTO recognition_learning_facts VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![scope,FEATURE_VERSION,id,family,k,metric,candidate,n]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO recognition_learning_totals VALUES(?1,?2,?3,?4,?5,?6,?7)
            ON CONFLICT(scope,feature_version,family,context_key,metric,candidate) DO UPDATE SET amount=amount+excluded.amount",
            params![scope,FEATURE_VERSION,family,k,metric,candidate,n]).map_err(|e|e.to_string())?;
    }
    if new.is_empty(){
        tx.execute("DELETE FROM recognition_learning_orders WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
            params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    }
    Ok(true)
}

// Hash the exact input to the contribution engine, including full audit
// event bodies and lifecycle state. Streaming avoids a second JSON buffer.
// Never persist the document or its personal values in this cache.
#[cfg(test)]
fn fingerprint(order:&Value)->Result<String,String>{
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter{
        fn write(&mut self,buf:&[u8])->std::io::Result<usize>{
            self.0.update(buf);Ok(buf.len())
        }
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    let mut writer=HashWriter(Sha256::new());
    writer.0.update(b"bando-recognition-contribution\0");
    writer.0.update(FEATURE_VERSION.to_le_bytes());
    writer.0.update(CONTRIBUTION_VERSION.to_le_bytes());
    serde_json::to_writer(&mut writer,order).map_err(|_|"학습 입력 지문 생성 실패")?;
    Ok(format!("{:x}",writer.0.finalize()))
}

fn fact_proof(facts:&[Fact])->Result<String,String>{
    // Sorted, typed tuples with an unambiguous JSON encoding.
    let encoded=serde_json::to_vec(facts).map_err(|_|"학습 기여 지문 생성 실패")?;
    let mut hash=Sha256::new();hash.update(b"bando-learning-facts-v1\0");hash.update(encoded);
    Ok(format!("{:x}",hash.finalize()))
}

pub(super) fn cached_count(tx:&Transaction<'_>,scope:&str,id:&str,digest:&str)->Result<Option<i64>,String>{
    Ok(verified_facts(tx,scope,id,digest)?.map(|f|f.len() as i64))
}
pub(super) fn verified_facts(tx:&Transaction<'_>,scope:&str,id:&str,digest:&str)->Result<Option<VerifiedFacts>,String>{
    let count:Option<i64>=tx.query_row("SELECT fact_count FROM recognition_learning_fingerprints
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3 AND contribution_version=?4 AND fingerprint=?5",
        params![scope,FEATURE_VERSION,id,CONTRIBUTION_VERSION,digest],|r|r.get(0))
        .optional().map_err(|e|e.to_string())?;
    let Some(expected)=count else{return Ok(None);};
    // A missing ledger/facts row must not be hidden by a cached digest.
    let has_order:i64=tx.query_row("SELECT count(*) FROM recognition_learning_orders
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id],|r|r.get(0)).map_err(|e|e.to_string())?;
    let facts=existing(tx,scope,id)?;
    if expected!=facts.len() as i64 || has_order!=i64::from(expected>0){
        return Err("학습 지문과 기여 원장이 불일치합니다.".into());
    }
    let proof:Option<String>=tx.query_row("SELECT digest FROM recognition_learning_fact_proofs
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    // Earlier builds have no proof. Re-project from the authoritative
    // source rather than blessing the existing derived facts.
    let Some(proof)=proof else{return Ok(None);};
    if fact_proof(&facts)?!=proof{
        return Err("학습 기여량 내용 지문 불일치".into());
    }
    Ok(Some(VerifiedFacts(facts)))
}

fn save_fingerprint(tx:&Transaction<'_>,scope:&str,id:&str,digest:&str,facts:&[Fact])->Result<(),String>{
    tx.execute("INSERT INTO recognition_learning_fingerprints VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(scope,feature_version,order_id) DO UPDATE SET
        contribution_version=excluded.contribution_version,fingerprint=excluded.fingerprint,fact_count=excluded.fact_count",
        params![scope,FEATURE_VERSION,id,CONTRIBUTION_VERSION,digest,facts.len() as i64]).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO recognition_learning_fact_proofs VALUES(?1,?2,?3,?4)
        ON CONFLICT(scope,feature_version,order_id) DO UPDATE SET digest=excluded.digest",
        params![scope,FEATURE_VERSION,id,fact_proof(facts)?]).map_err(|e|e.to_string())?;
    Ok(())
}

// Independent derived evaluation; no contribution to learning support.
pub(super) fn combined_evaluation_cached(tx:&Transaction<'_>,scope:&str,id:&str,digest:&str)->Result<bool,String>{
    let row:Option<(String,String,String,i64)>=tx.query_row("SELECT source_digest,payload,payload_digest,evaluation_version
        FROM recognition_combined_evaluation WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
        .optional().map_err(|e|e.to_string())?;
    let Some((source,payload,proof,version))=row else{return Ok(false);};
    if version!=1||source!=digest{return Ok(false);}
    if format!("{:x}",Sha256::digest(payload.as_bytes()))!=proof{return Err("결합 평가 지문 불일치".into());}
    Ok(true)
}
fn save_combined_evaluation(tx:&Transaction<'_>,scope:&str,id:&str,digest:&str,report:&Value)->Result<(),String>{
    let fallback=json!({"schemaVersion":1,"status":"no_combined_prediction","groups":[],
        "eligibleForAutomaticDecision":false,"decisionChanged":false});
    let v=report.get("combinedEvaluation").unwrap_or(&fallback);
    if v["schemaVersion"]!=1||v["eligibleForAutomaticDecision"]!=false||v["decisionChanged"]!=false||
        v["groups"].as_array().is_none_or(|a|a.len()>64){return Err("결합 평가 규격 오류".into());}
    let payload=serde_json::to_string(v).map_err(|_|"결합 평가 직렬화 오류")?;
    if payload.len()>64*1024{return Err("결합 평가 저장 한도 초과".into());}
    let proof=format!("{:x}",Sha256::digest(payload.as_bytes()));
    tx.execute("INSERT INTO recognition_combined_evaluation VALUES(?1,?2,?3,1,?4,?5,?6)
        ON CONFLICT(scope,feature_version,order_id) DO UPDATE SET source_digest=excluded.source_digest,
        payload=excluded.payload,payload_digest=excluded.payload_digest,evaluation_version=excluded.evaluation_version",
        params![scope,FEATURE_VERSION,id,digest,payload,proof]).map_err(|e|e.to_string())?;
    Ok(())
}
pub(super) fn combined_evaluation_report(tx:&Transaction<'_>,scope:&str,digests:&BTreeMap<String,String>)->Result<Value,String>{
    let mut reports=Vec::new();let mut missing=0;
    for (id,digest) in digests{
        if !combined_evaluation_cached(tx,scope,id,digest)?{missing+=1;continue;}
        let payload:String=tx.query_row("SELECT payload FROM recognition_combined_evaluation
            WHERE scope=?1 AND feature_version=?2 AND order_id=?3",params![scope,FEATURE_VERSION,id],|r|r.get(0))
            .map_err(|e|e.to_string())?;
        reports.push(serde_json::from_str::<Value>(&payload).map_err(|_|"결합 평가 JSON 오류")?);
    }
    Ok(json!({"status":"stored_combined_evaluation_diagnostic_only","orders":reports,
        "missingEvaluationOrders":missing,"eligibleForAutomaticDecision":false,"decisionChanged":false}))
}

// Aggregate only digest-verified current rows. A model's own cohort is
// not a randomized or paired comparison against any other model.
#[derive(Default)]
struct PerformanceCounts{
    orders:u64,observations:u64,legacy:u64,combined:u64,proposals:u64,improved:u64,worsened:u64,outside:u64,
    order_legacy:f64,order_combined:f64,
}
impl PerformanceCounts{
    fn add_order(&mut self,c:[u64;7])->Result<(),String>{
        let [n,old,new,proposed,better,worse,outside]=c;
        if n==0||old>n||new>n||proposed>n||better+worse>proposed||
            better>n-old||worse>old||outside>n||old+better!=new+worse{
            return Err("결합 평가 카운터 불일치".into());
        }
        self.orders+=1;self.observations+=n;self.legacy+=old;self.combined+=new;
        self.proposals+=proposed;self.improved+=better;self.worsened+=worse;self.outside+=outside;
        self.order_legacy+=old as f64/n as f64;self.order_combined+=new as f64/n as f64;
        Ok(())
    }
    fn wire(&self)->Value{
        let n=self.observations as f64;
        json!({"orders":self.orders,"observations":self.observations,"legacyAgrees":self.legacy,
            "screenedCombinedAgrees":self.combined,"proposals":self.proposals,"improved":self.improved,
            "worsened":self.worsened,"outsideLabels":self.outside,
            "legacyAgreement":(n>0.0).then(||self.legacy as f64/n),
            "screenedCombinedAgreement":(n>0.0).then(||self.combined as f64/n),
            "agreementDelta":(n>0.0).then(||(self.combined as f64-self.legacy as f64)/n),
            "orderMacroLegacyAgreement":(self.orders>0).then(||self.order_legacy/self.orders as f64),
            "orderMacroCombinedAgreement":(self.orders>0).then(||self.order_combined/self.orders as f64)})
    }
}
#[derive(Default)]
struct ModelPerformance{
    orders:u64,eligible:u64,missing:u64,excluded:u64,
    strata:BTreeMap<(String,String,String,String),PerformanceCounts>,
    labels:BTreeMap<String,PerformanceCounts>,
}
#[derive(Default)]
pub(super) struct CombinedPerformance{
    pub(super) scanned:u64,pub(super) inactive:u64,pub(super) unsynchronized:u64,
    pub(super) missing:u64,
    statuses:BTreeMap<String,u64>,models:BTreeMap<String,ModelPerformance>,
}
impl CombinedPerformance{
    pub(super) fn add(&mut self,report:&Value)->Result<(),String>{
        let status=report["status"].as_str().filter(|s|matches!(*s,"no_combined_prediction"|
            "unverified_prediction_time"|"unverified_prediction_provenance"|"unverified_model_identity"|
            "unverified_audit"|"unverified_audit_time"|"compared_diagnostic_only"))
            .ok_or("결합 평가 상태 오류")?;
        if report["schemaVersion"]!=1||report["decisionChanged"]!=false||
            report["eligibleForAutomaticDecision"]!=false{return Err("결합 평가 규격 오류".into());}
        *self.statuses.entry(status.to_owned()).or_default()+=1;
        let groups=report["groups"].as_array().filter(|a|a.len()<=64).ok_or("결합 평가 집단 오류")?;
        if status!="compared_diagnostic_only"{
            if !groups.is_empty(){return Err("미검증 결합 평가 집단".into());}
            return Ok(());
        }
        let id=report["modelId"].as_str().filter(|s|s.len()==70&&s.starts_with("blend-")&&
            s[6..].bytes().all(|b|b.is_ascii_hexdigit())).ok_or("평가 모델 ID 오류")?;
        let eligible=report["eligibleObservations"].as_u64().filter(|n|*n<=64).ok_or("평가 관측 수 오류")?;
        let missing=report["missingOrIneligiblePredictions"].as_u64().filter(|n|*n<=eligible).ok_or("평가 누락 수 오류")?;
        let excluded=report["excludedLabels"].as_u64().filter(|n|*n<=64).ok_or("평가 제외 수 오류")?;
        let m=self.models.entry(id.to_owned()).or_default();
        m.orders+=1;m.eligible+=eligible;m.missing+=missing;m.excluded+=excluded;
        let mut local=BTreeMap::<String,[u64;7]>::new();
        let mut seen=BTreeSet::new();let mut evaluated=0;
        for g in groups{
            let field=|key:&str,allowed:&[&str]|g[key].as_str().filter(|v|allowed.contains(v))
                .map(str::to_owned).ok_or("평가 집단 키 오류");
            let key=(field("sourceKind",&["address","contact"])?,field("legacyRoute",&["score","gate"])?,
                field("contextLevel",&["base","structure","client_structure"])?,
                field("labelKind",&["explicit_correction","unchanged_proxy"])?);
            if !seen.insert(key.clone()){return Err("주문 내 평가 집단 중복".into());}
            let mut c=[0;7];
            for (i,name) in ["evaluated","legacyAgrees","screenedCombinedAgrees","proposals","improved","worsened","outsideLabels"].iter().enumerate(){
                c[i]=g[*name].as_u64().filter(|n|*n<=64).ok_or("평가 카운터 오류")?;
            }
            if key.3=="explicit_correction"&&c[1]!=0||key.3=="unchanged_proxy"&&c[1]!=c[0]{
                return Err("정정 유형과 기존 판정 불일치".into());
            }
            evaluated+=c[0];
            let sum=local.entry(key.3.clone()).or_default();
            for i in 0..7{sum[i]+=c[i];}
            m.strata.entry(key).or_default().add_order(c)?;
        }
        if evaluated+missing!=eligible{return Err("결합 평가 분모 불일치".into());}
        for (label,c) in local{m.labels.entry(label).or_default().add_order(c)?;}
        Ok(())
    }
    pub(super) fn compare_models(&self,baseline:&str,candidate:&str)->Result<Value,String>{
        if baseline==candidate{return Err("서로 다른 모델을 선택하세요.".into());}
        let a=self.models.get(baseline).ok_or("기존 모델 평가 없음")?;
        let b=self.models.get(candidate).ok_or("신규 모델 평가 없음")?;
        let keys=a.strata.keys().chain(b.strata.keys()).collect::<BTreeSet<_>>();
        let strata=keys.into_iter().map(|key|{
            let x=a.strata.get(key);let y=b.strata.get(key);
            let delta=|v:&PerformanceCounts|(v.combined as f64-v.legacy as f64)/v.observations as f64;
            json!({"sourceKind":key.0,"legacyRoute":key.1,"contextLevel":key.2,"labelKind":key.3,
                "baseline":x.map(PerformanceCounts::wire),"candidate":y.map(PerformanceCounts::wire),
                "descriptiveImprovementDifference":x.zip(y).map(|(a,b)|delta(b)-delta(a)),
                "sameOrders":false})
        }).collect::<Vec<_>>();
        Ok(json!({"baselineModelId":baseline,"candidateModelId":candidate,"strata":strata,
            "comparisonKind":"unpaired_observed_cohorts","sameOrders":false,
            "winner":null,"eligibleForAutomaticDecision":false}))
    }
    pub(super) fn wire(&self)->Value{
        let models=self.models.iter().map(|(id,m)|json!({"modelId":id,"evaluatedOrders":m.orders,
            "eligibleObservations":m.eligible,"missingOrIneligiblePredictions":m.missing,"excludedLabels":m.excluded,
            "coverage":(m.eligible>0).then(||(m.eligible-m.missing) as f64/m.eligible as f64),
            "labelGroups":m.labels.iter().map(|(label,v)|json!({"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>(),
            "strata":m.strata.iter().map(|((kind,route,level,label),v)|json!({
                "sourceKind":kind,"legacyRoute":route,"contextLevel":level,"labelKind":label,"metrics":v.wire()})).collect::<Vec<_>>()
        })).collect::<Vec<_>>();
        json!({"status":"cumulative_model_performance_diagnostic_only",
            "scannedOrders":self.scanned,"inactiveOrMissingOrders":self.inactive,
            "unsynchronizedOrders":self.unsynchronized,"missingEvaluationOrders":self.missing,
            "evaluationStatuses":self.statuses,"models":models,
            "crossModelPairedComparison":false,"crossModelRankingAuthorized":false,
            "representativeLabelsVerified":false,"eligibleForAutomaticDecision":false,"decisionChanged":false})
    }
}
pub(super) fn accumulate_combined_performance(tx:&Transaction<'_>,scope:&str,digests:&BTreeMap<String,String>,
    totals:&mut CombinedPerformance)->Result<(),String>{
    for (id,digest) in digests{
        if cached_count(tx,scope,id,digest)?.is_none(){totals.unsynchronized+=1;continue;}
        if !combined_evaluation_cached(tx,scope,id,digest)?{totals.missing+=1;continue;}
        let payload:String=tx.query_row("SELECT payload FROM recognition_combined_evaluation
            WHERE scope=?1 AND feature_version=?2 AND order_id=?3",params![scope,FEATURE_VERSION,id],|r|r.get(0))
            .map_err(|e|e.to_string())?;
        totals.add(&serde_json::from_str::<Value>(&payload).map_err(|_|"결합 평가 JSON 오류")?)?;
    }
    Ok(())
}

/// The caller holds an IMMEDIATE transaction in the audit database across
/// snapshot read, per-order projection, delta replacement and revalidation.
pub(super) fn synchronize_with_digests(tx:&Transaction<'_>,scope:&str,ids:&[String],
    digests:&BTreeMap<String,String>,orders:&[Value],
    summarize:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
    let requested=ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    if ids.is_empty()||ids.len()>250||requested.iter().any(|id|id.trim().is_empty()){
        return Err("학습 갱신은 1~250개 주문 ID를 명시해야 합니다.".into());
    }
    if digests.keys().any(|id|!requested.contains(id.as_str())){
        return Err("학습 지문 주문 범위 오류".into());
    }
    let mut by_id=BTreeMap::new();
    for order in orders{
        let id=order["id"].as_str().filter(|id|requested.contains(id)).ok_or("학습 주문 범위 오류")?;
        if !digests.contains_key(id){return Err("학습 입력 지문 누락".into());}
        if by_id.insert(id,order).is_some(){return Err("학습 주문 중복".into());}
    }
    let mut changed=0;
    let mut eligible=0;
    let mut reused=0;
    let mut recalculated=0;
    for id in requested.iter().copied(){
        if let Some(digest)=digests.get(id){
            if let Some(count)=cached_count(tx,scope,id,digest)?{
                if combined_evaluation_cached(tx,scope,id,digest)?{
                reused+=1;
                if count>0{eligible+=1;}
                clear_pending(tx,scope,id)?;
                continue;
                }
            }
            let order=by_id.get(id).ok_or("변경된 학습 주문 원본 누락")?;
            let summary=summarize(order)?;
            let new=facts(&summary)?;
            recalculated+=1;
            if !new.is_empty(){eligible+=1;}
            if replace(tx,scope,id,&new)?{changed+=1;}
            save_fingerprint(tx,scope,id,digest,&new)?;
            save_combined_evaluation(tx,scope,id,digest,&summary)?;
        }else{
            // Missing/inactive orders revoke their prior contributions and
            // cache entry; reactivation must be checked from current inputs.
            if replace(tx,scope,id,&[])?{changed+=1;}
            tx.execute("DELETE FROM recognition_learning_fingerprints
                WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
                params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
        }
        clear_pending(tx,scope,id)?;
    }
    let stored:i64=tx.query_row("SELECT count(*) FROM recognition_learning_orders WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    Ok(json!({"status":"persisted_diagnostic_only","featureVersion":FEATURE_VERSION,
        "requestedOrders":requested.len(),"changedOrders":changed,"eligibleSelectedOrders":eligible,
        "reusedOrders":reused,"recalculatedOrders":recalculated,"contributionVersion":CONTRIBUTION_VERSION,
        "storedContributingOrders":stored,"missingSelectedOrders":requested.len()-digests.len(),
        "automaticDecisionEnabled":false,"fullHistoryReconciled":false}))
}

#[cfg(test)]
fn synchronize(tx:&Transaction<'_>,scope:&str,ids:&[String],orders:&[Value],
    summarize:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
    let digests=orders.iter().map(|order|Ok((
        order["id"].as_str().ok_or("학습 주문 ID 누락")?.to_owned(),fingerprint(order)?
    ))).collect::<Result<BTreeMap<_,_>,String>>()?;
    synchronize_with_digests(tx,scope,ids,&digests,orders,summarize)
}

pub(super) fn select_learning_contexts(locations:&[Value],reports:&[Value])->Vec<Value>{
    recognition_support::select_contexts(locations,reports)
}

pub(super) fn assess_support(tx:&Transaction<'_>,scope:&str,input:&Value)->Result<Value,String>{
    recognition_support::assess(tx,scope,input)
}

// Bound foreground shadow capture before reading and verifying any full facts.
pub(super) fn shadow_capture_contexts(tx:&Transaction<'_>,scope:&str,target:&str,keys:&BTreeSet<String>)
    ->Result<BTreeMap<String,Vec<String>>,String>{
    if keys.is_empty()||keys.len()>32{return Err("실시간 학습 문맥 한도 오류".into());}
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending!=0{return Err("복구 대기 학습 주문".into());}
    let mut stmt=tx.prepare("SELECT DISTINCT order_id FROM recognition_learning_facts
        WHERE scope=?1 AND feature_version=?2 AND family=?5 AND context_key=?3
        AND order_id<>?4 ORDER BY order_id LIMIT 251").map_err(|e|e.to_string())?;
    let mut ids=BTreeSet::new();
    let mut contexts=BTreeMap::new();
    for key in keys{
        let context:Value=serde_json::from_str(key).map_err(|_|"실시간 문맥 키 오류")?;
        let family=match context[1].as_str(){
            Some("recipient_ownership")=>"statistics",
            Some("type"|"product"|"variant"|"quantity")=>"fieldContextStatistics",
            _=>return Err("실시간 학습 작업 유형 오류".into())
        };
        let rows=stmt.query_map(params![scope,FEATURE_VERSION,key,target,family],|r|r.get::<_,String>(0))
            .map_err(|e|e.to_string())?;
        let mut members=Vec::new();
        for id in rows{
            let id=id.map_err(|e|e.to_string())?;
            ids.insert(id.clone());
            if ids.len()>250{return Err("실시간 학습 주문 한도 초과".into());}
            members.push(id);
        }
        contexts.insert(key.clone(),members);
    }
    Ok(contexts)
}
#[cfg(test)]
fn shadow_capture_ids(tx:&Transaction<'_>,scope:&str,target:&str,keys:&BTreeSet<String>)->Result<Vec<String>,String>{
    Ok(shadow_capture_contexts(tx,scope,target,keys)?.into_values().flatten().collect::<BTreeSet<_>>().into_iter().collect())
}
pub(super) fn assess_verified_support(input:&Value,contexts:&BTreeMap<String,Vec<String>>,
    facts:&BTreeMap<String,VerifiedFacts>)->Result<Value,String>{
    recognition_support::assess_verified(input,contexts,facts)
}

pub(super) fn assess_local_replay(tx:&Transaction<'_>,scope:&str,input:&Value,
    orders:&[Value],digests:&BTreeMap<String,String>)->Result<Value,String>{
    recognition_support::local_replay(tx,scope,input,orders,digests)
}

pub(super) fn order_page(conn:&Connection,scope:&str,after:&str,limit:usize)->Result<Vec<String>,String>{
    if !(1..=25).contains(&limit){return Err("학습 원장 페이지 범위 오류".into());}
    let mut stmt=conn.prepare("SELECT order_id FROM recognition_learning_orders
        WHERE scope=?1 AND feature_version=?2 AND order_id>?3
        UNION SELECT order_id FROM recognition_learning_fingerprints
        WHERE scope=?1 AND feature_version=?2 AND order_id>?3
        UNION SELECT order_id FROM recognition_learning_pending
        WHERE scope=?1 AND feature_version=?2 AND order_id>?3 ORDER BY order_id LIMIT ?4")
        .map_err(|e|e.to_string())?;
    let rows=stmt.query_map(params![scope,FEATURE_VERSION,after,limit as i64],|r|r.get::<_,String>(0))
        .map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}

fn clear_pending(tx:&Transaction<'_>,scope:&str,id:&str)->Result<(),String>{
    tx.execute("DELETE FROM recognition_learning_pending WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    Ok(())
}

pub(super) fn mark_pending(tx:&Transaction<'_>,scope:&str,id:&str)->Result<(),String>{
    tx.execute("INSERT OR IGNORE INTO recognition_learning_pending VALUES(?1,?2,?3)",
        params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    Ok(())
}

/// Only derived records are removed. If a damaged total cannot be safely
/// decremented, the caller rolls back this revocation but keeps the pending
/// marker committed separately; the scope is not eligible for activation.
pub(super) fn revoke_pending(tx:&Transaction<'_>,scope:&str,id:&str)->Result<(),String>{
    let old=existing(tx,scope,id)?;
    if !old.is_empty(){
        let proof:Option<String>=tx.query_row("SELECT digest FROM recognition_learning_fact_proofs
            WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
            params![scope,FEATURE_VERSION,id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        if proof.as_deref()!=Some(fact_proof(&old)?.as_str()){
            return Err("손상된 기여량은 안전하게 차감할 수 없습니다.".into());
        }
    }
    replace(tx,scope,id,&[])?;
    tx.execute("DELETE FROM recognition_learning_fingerprints WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    Ok(())
}

pub(super) fn integrity(tx:&Transaction<'_>,scope:&str)->Result<Value,String>{
    let mismatch:i64=tx.query_row("WITH expected AS (
        SELECT family,context_key,metric,candidate,SUM(amount) amount
        FROM recognition_learning_facts WHERE scope=?1 AND feature_version=?2
        GROUP BY family,context_key,metric,candidate),
        actual AS (SELECT family,context_key,metric,candidate,amount FROM recognition_learning_totals
        WHERE scope=?1 AND feature_version=?2),
        missing AS (SELECT * FROM expected EXCEPT SELECT * FROM actual),
        extra AS (SELECT * FROM actual EXCEPT SELECT * FROM expected)
        SELECT (SELECT count(*) FROM missing)+(SELECT count(*) FROM extra)",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending
        WHERE scope=?1 AND feature_version=?2",params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    let mut stmt=tx.prepare("SELECT order_id FROM recognition_learning_orders WHERE scope=?1 AND feature_version=?2
        UNION SELECT order_id FROM recognition_learning_fingerprints WHERE scope=?1 AND feature_version=?2
        UNION SELECT order_id FROM recognition_learning_facts WHERE scope=?1 AND feature_version=?2")
        .map_err(|e|e.to_string())?;
    let mut ids=stmt.query(params![scope,FEATURE_VERSION]).map_err(|e|e.to_string())?;
    let mut invalid=0u64;let mut inspected=0u64;
    while let Some(row)=ids.next().map_err(|e|e.to_string())?{
        let id:String=row.get(0).map_err(|e|e.to_string())?;
        let record:Option<(String,i64)>=tx.query_row("SELECT fingerprint,contribution_version
            FROM recognition_learning_fingerprints WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
            params![scope,FEATURE_VERSION,id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        inspected+=1;
        match record{
            Some((digest,version)) if version==CONTRIBUTION_VERSION=>{
                if !matches!(cached_count(tx,scope,&id,&digest),Ok(Some(_))){invalid+=1;}
            }
            _=>invalid+=1,
        }
    }
    Ok(json!({"status":"diagnostic_only","featureVersion":FEATURE_VERSION,
        "inspectedOrders":inspected,"invalidContributionOrders":invalid,
        "totalMismatchRows":mismatch,"pendingOrders":pending,
        "internallyConsistent":invalid==0&&mismatch==0&&pending==0,
        "fullHistoryReconciled":false,"automaticDecisionEnabled":false}))
}

/// Explicit last-resort recovery. Discard only reconstructible derived
/// counts for the selected scope/version; keep source/audit records intact.
/// Startup/periodic reconciliation will replay authoritative confirmations.
pub(super) fn reset_derived(tx:&Transaction<'_>,scope:&str)->Result<Value,String>{
    let mut removed=0usize;
    for table in ["recognition_combined_evaluation","recognition_learning_fact_proofs","recognition_learning_fingerprints",
        "recognition_learning_facts","recognition_learning_orders","recognition_learning_totals",
        "recognition_learning_pending"]{
        removed+=tx.execute(&format!("DELETE FROM {table} WHERE scope=?1 AND feature_version=?2"),
            params![scope,FEATURE_VERSION]).map_err(|e|e.to_string())?;
    }
    Ok(json!({"status":"derived_reset","removedDerivedRows":removed,"featureVersion":FEATURE_VERSION,
        "requiresSourceReplay":true,"fullHistoryReconciled":false,"automaticDecisionEnabled":false}))
}

/// Rebuild only this scope/version from its persisted per-order ledger.
/// This repairs materialized totals, but cannot discover unsynchronized
/// business orders. The entire replacement is transactional.
pub(super) fn rebuild_totals(tx:&Transaction<'_>,scope:&str)->Result<Value,String>{
    if integrity(tx,scope)?["invalidContributionOrders"]!=0{
        return Err("검증되지 않은 기여 원장으로 합계를 재구축할 수 없습니다.".into());
    }
    tx.execute("DELETE FROM recognition_learning_totals WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION]).map_err(|e|e.to_string())?;
    let rows=tx.execute("INSERT INTO recognition_learning_totals
        SELECT scope,feature_version,family,context_key,metric,candidate,SUM(amount)
        FROM recognition_learning_facts WHERE scope=?1 AND feature_version=?2
        GROUP BY scope,feature_version,family,context_key,metric,candidate",
        params![scope,FEATURE_VERSION]).map_err(|e|e.to_string())?;
    Ok(json!({"status":"rebuilt_diagnostic_only","featureVersion":FEATURE_VERSION,
        "totalRows":rows,"fullHistoryReconciled":false,"automaticDecisionEnabled":false}))
}

#[cfg(test)]
mod recognition_learning_store_tests{
    use super::*;
    #[test]
    fn cumulative_model_performance_counts_orders_once_per_label_and_never_ranks_unpaired_cohorts(){
        let mut totals=CombinedPerformance::default();
        let id1=format!("blend-{}","a".repeat(64));let id2=format!("blend-{}","b".repeat(64));
        let group=|kind:&str,n:u64,better:u64|json!({"sourceKind":kind,"legacyRoute":"gate",
            "contextLevel":"base","labelKind":"explicit_correction","evaluated":n,"legacyAgrees":0,
            "screenedCombinedAgrees":better,"proposals":better,"improved":better,"worsened":0,"outsideLabels":0});
        let report=|id:&str,groups:Vec<Value>,n:u64|json!({"schemaVersion":1,"status":"compared_diagnostic_only",
            "eligibleForAutomaticDecision":false,"decisionChanged":false,"modelId":id,"groups":groups,
            "eligibleObservations":n,"missingOrIneligiblePredictions":0,"excludedLabels":0});
        totals.add(&report(&id1,vec![group("address",1,1),group("contact",9,0)],10)).unwrap();
        totals.add(&report(&id1,vec![group("address",1,1)],1)).unwrap();
        totals.add(&report(&id2,vec![group("address",2,1)],2)).unwrap();
        let v=totals.wire();
        assert_eq!(v["models"][0]["labelGroups"][0]["metrics"]["orders"],2);
        assert_eq!(v["models"][0]["labelGroups"][0]["metrics"]["observations"],11);
        assert!((v["models"][0]["labelGroups"][0]["metrics"]["orderMacroCombinedAgreement"].as_f64().unwrap()-0.55).abs()<1e-10);
        let cmp=totals.compare_models(&id1,&id2).unwrap();
        assert!(cmp["winner"].is_null());assert_eq!(cmp["sameOrders"],false);
        assert_eq!(cmp["strata"][0]["descriptiveImprovementDifference"],-0.5);
        assert!(cmp["strata"][1]["candidate"].is_null());
        assert!(totals.compare_models(&id1,&id1).is_err());
        let mut bad=report(&id1,vec![group("address",1,1)],2);
        assert!(CombinedPerformance::default().add(&bad).is_err());
        bad["eligibleObservations"]=json!(1);bad["groups"][0]["improved"]=json!(0);
        assert!(CombinedPerformance::default().add(&bad).is_err());
    }
    #[test]
    fn combined_evaluation_is_replaced_verified_and_revoked_with_fingerprint(){
        let mut c=conn();
        c.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        let tx=c.transaction().unwrap();
        let id="combined-test";let digest="a".repeat(64);
        save_fingerprint(&tx,"scope",id,&digest,&[]).unwrap();
        assert!(!combined_evaluation_cached(&tx,"scope",id,&digest).unwrap());
        let mut report=json!({"combinedEvaluation":{"schemaVersion":1,"status":"compared_diagnostic_only",
            "groups":[],"eligibleForAutomaticDecision":false,"decisionChanged":false,"eligibleObservations":1}});
        save_combined_evaluation(&tx,"scope",id,&digest,&report).unwrap();
        assert!(combined_evaluation_cached(&tx,"scope",id,&digest).unwrap());
        let digests=BTreeMap::from([(id.to_owned(),digest.clone())]);
        assert_eq!(combined_evaluation_report(&tx,"scope",&digests).unwrap()["orders"][0]["eligibleObservations"],1);
        report["combinedEvaluation"]["eligibleObservations"]=json!(2);
        save_combined_evaluation(&tx,"scope",id,&digest,&report).unwrap();
        let r=combined_evaluation_report(&tx,"scope",&digests).unwrap();
        assert_eq!(r["orders"].as_array().unwrap().len(),1);
        assert_eq!(r["orders"][0]["eligibleObservations"],2);
        assert!(!combined_evaluation_cached(&tx,"scope",id,&"b".repeat(64)).unwrap());
        tx.execute("UPDATE recognition_combined_evaluation SET payload='{}' WHERE scope='scope'",[]).unwrap();
        assert!(combined_evaluation_cached(&tx,"scope",id,&digest).is_err());
        revoke_pending(&tx,"scope",id).unwrap();
        assert!(!combined_evaluation_cached(&tx,"scope",id,&digest).unwrap());
    }
    fn conn()->Connection{
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON").unwrap();prepare(&c).unwrap();c
    }
    fn report(n:u64)->Value{
        json!({"featureVersion":1,"uniqueOrders":1,"status":"diagnostic_only",
            "eligibleForDurableLearning":false,"auditCoverage":{"verifiedManifestOrders":1},
            "statistics":[],"fieldContextStatistics":[],"fieldStatistics":[{"task":"type","counts":{
            "observationCount":n,"independentOrders":1,"weakIndependentOrders":1,
            "weakUnchangedCount":n,"explicitCorrectionCount":0,"explicitCorrectionOrders":0,
            "pendingCorrectionCount":0,"unresolvedCount":0,"eligibleForAutomaticDecision":false,
            "weakCandidates":{"address":n},"weakCandidateOrders":{"address":1},
            "explicitPositiveCandidates":{},"explicitNegativeCandidates":{},
            "explicitPositiveCandidateOrders":{},"explicitNegativeCandidateOrders":{}}}]})
    }
    fn total(c:&Connection,scope:&str,metric:&str)->i64{
        c.query_row("SELECT coalesce(sum(amount),0) FROM recognition_learning_totals WHERE scope=?1 AND metric=?2",
            params![scope,metric],|r|r.get(0)).unwrap()
    }
    #[test]
    fn replaces_not_accumulates_and_removes_missing_orders(){
        let mut c=conn();let ids=vec!["a".into(),"b".into()];let orders=vec![json!({"id":"a"}),json!({"id":"b"})];
        for expected in [2,0]{
            let tx=c.transaction().unwrap();
            let result=synchronize(&tx,"db",&ids,&orders,|_|Ok(report(3))).unwrap();
            assert_eq!(result["changedOrders"],expected);tx.commit().unwrap();
        }
        assert_eq!(total(&c,"db","weakUnchangedCount"),6);
        assert_eq!(total(&c,"db","independentOrders"),2);
        let tx=c.transaction().unwrap();
        let corrected=vec![json!({"id":"a","confirmationSnapshot":{"revision":2}})];
        synchronize(&tx,"db",&ids,&corrected,|_|Ok(report(2))).unwrap();tx.commit().unwrap();
        assert_eq!(total(&c,"db","weakUnchangedCount"),2);
        assert_eq!(total(&c,"db","independentOrders"),1);
    }
    #[test]
    fn unverified_and_scope_isolation(){
        let mut c=conn();let ids=vec!["a".into()];let orders=vec![json!({"id":"a"})];
        for scope in ["first","second"]{
            let tx=c.transaction().unwrap();
            synchronize(&tx,scope,&ids,&orders,|_|Ok(report(1))).unwrap();tx.commit().unwrap();
        }
        let tx=c.transaction().unwrap();
        let changed=vec![json!({"id":"a","recognitionAuditEvents":[{"id":"changed"}]})];
        synchronize(&tx,"first",&ids,&changed,|_|{let mut r=report(1);r["auditCoverage"]["verifiedManifestOrders"]=json!(0);Ok(r)}).unwrap();tx.commit().unwrap();
        assert_eq!(total(&c,"first","independentOrders"),0);
        assert_eq!(total(&c,"second","independentOrders"),1);
    }
    #[test]
    fn totals_rebuild_uses_ledger_and_preserves_other_scope(){
        let mut c=conn();let ids=vec!["a".into()];let orders=vec![json!({"id":"a"})];
        for scope in ["first","second"]{
            let tx=c.transaction().unwrap();
            synchronize(&tx,scope,&ids,&orders,|_|Ok(report(3))).unwrap();tx.commit().unwrap();
        }
        c.execute("UPDATE recognition_learning_totals SET amount=99 WHERE scope='first'",[]).unwrap();
        let tx=c.transaction().unwrap();rebuild_totals(&tx,"first").unwrap();tx.commit().unwrap();
        assert_eq!(total(&c,"first","weakUnchangedCount"),3);
        assert_eq!(total(&c,"second","weakUnchangedCount"),3);
    }
    #[test]
    fn exact_inputs_skip_projection_even_when_no_contribution(){
        let mut c=conn();let ids=vec!["a".into()];
        let original=json!({"id":"a","learningActive":true,
            "confirmationSnapshot":{"recognitionBaseline":{"schemaVersion":1}},
            "recognitionAuditEvents":[{"id":"audit","afterRows":[1]}]});
        let tx=c.transaction().unwrap();
        let first=synchronize(&tx,"db",&ids,std::slice::from_ref(&original),|_|Ok(report(1))).unwrap();
        assert_eq!(first["recalculatedOrders"],1);tx.commit().unwrap();
        let tx=c.transaction().unwrap();
        let second=synchronize(&tx,"db",&ids,std::slice::from_ref(&original),|_|panic!("must not recalculate")).unwrap();
        assert_eq!(second["reusedOrders"],1);assert_eq!(second["changedOrders"],0);tx.commit().unwrap();
        let mut changed=original.clone();changed["recognitionAuditEvents"][0]["afterRows"]=json!([2]);
        let tx=c.transaction().unwrap();
        let third=synchronize(&tx,"db",&ids,std::slice::from_ref(&changed),|_|{
            let mut r=report(1);r["auditCoverage"]["verifiedManifestOrders"]=json!(0);Ok(r)
        }).unwrap();
        assert_eq!(third["recalculatedOrders"],1);assert_eq!(third["eligibleSelectedOrders"],0);tx.commit().unwrap();
        let tx=c.transaction().unwrap();
        let fourth=synchronize(&tx,"db",&ids,std::slice::from_ref(&changed),|_|panic!("zero evidence must be cached")).unwrap();
        assert_eq!(fourth["reusedOrders"],1);tx.commit().unwrap();
        assert_eq!(total(&c,"db","independentOrders"),0);
        assert_eq!(order_page(&c,"db","",16).unwrap(),ids);
        let tx=c.transaction().unwrap();synchronize(&tx,"db",&ids,&[],|_|panic!("missing order")).unwrap();tx.commit().unwrap();
        assert!(order_page(&c,"db","",16).unwrap().is_empty());
    }
    #[test]
    fn cached_raw_digest_requires_no_decoded_order_and_misses_fail_closed(){
        let mut c=conn();let ids=vec!["a".into()];
        let digests=BTreeMap::from([("a".to_owned(),"a".repeat(64))]);
        let tx=c.transaction().unwrap();
        synchronize_with_digests(&tx,"db",&ids,&digests,&[json!({"id":"a"})],|_|Ok(report(1))).unwrap();
        tx.commit().unwrap();
        let tx=c.transaction().unwrap();
        let result=synchronize_with_digests(&tx,"db",&ids,&digests,&[],|_|panic!("cached order was decoded")).unwrap();
        assert_eq!(result["reusedOrders"],1);
        assert_eq!(result["recalculatedOrders"],0);tx.commit().unwrap();
        let changed=BTreeMap::from([("a".to_owned(),"b".repeat(64))]);
        let tx=c.transaction().unwrap();
        assert!(synchronize_with_digests(&tx,"db",&ids,&changed,&[],|_|panic!("missing input")).is_err());
        drop(tx);assert_eq!(total(&c,"db","independentOrders"),1);
    }
    #[test]
    fn version_changes_and_reactivation_force_recalculation(){
        let mut c=conn();let ids=vec!["a".into()];let orders=vec![json!({"id":"a"})];
        let tx=c.transaction().unwrap();synchronize(&tx,"db",&ids,&orders,|_|Ok(report(1))).unwrap();tx.commit().unwrap();
        c.execute("UPDATE recognition_learning_fingerprints SET contribution_version=0",[]).unwrap();
        let tx=c.transaction().unwrap();
        assert_eq!(synchronize(&tx,"db",&ids,&orders,|_|Ok(report(1))).unwrap()["recalculatedOrders"],1);
        tx.commit().unwrap();
        let tx=c.transaction().unwrap();synchronize(&tx,"db",&ids,&[],|_|panic!("inactive")).unwrap();tx.commit().unwrap();
        let tx=c.transaction().unwrap();
        assert_eq!(synchronize(&tx,"db",&ids,&orders,|_|Ok(report(1))).unwrap()["recalculatedOrders"],1);
        tx.commit().unwrap();
    }
    #[test]
    fn same_count_corruption_and_wrong_totals_are_detected(){
        let mut c=conn();let ids=vec!["a".into()];let orders=vec![json!({"id":"a"})];
        let tx=c.transaction().unwrap();synchronize(&tx,"db",&ids,&orders,|_|Ok(report(3))).unwrap();tx.commit().unwrap();
        c.execute("UPDATE recognition_learning_totals SET amount=99 WHERE metric='weakUnchangedCount'",[]).unwrap();
        let tx=c.transaction().unwrap();
        assert_ne!(integrity(&tx,"db").unwrap()["totalMismatchRows"],0);
        rebuild_totals(&tx,"db").unwrap();
        assert_eq!(integrity(&tx,"db").unwrap()["internallyConsistent"],true);tx.commit().unwrap();
        c.execute("UPDATE recognition_learning_facts SET amount=99 WHERE metric='weakUnchangedCount'",[]).unwrap();
        let tx=c.transaction().unwrap();
        assert_eq!(integrity(&tx,"db").unwrap()["invalidContributionOrders"],1);
        assert!(rebuild_totals(&tx,"db").is_err());
        assert!(synchronize(&tx,"db",&ids,&orders,|_|panic!("corrupt cache must not be trusted")).is_err());
        assert!(revoke_pending(&tx,"db","a").is_err());
        assert!(synchronize(&tx,"db",&ids,&[],|_|panic!("inactive")).is_err());
        drop(tx);
        let tx=c.transaction().unwrap();reset_derived(&tx,"db").unwrap();tx.commit().unwrap();
        let tx=c.transaction().unwrap();
        synchronize(&tx,"db",&ids,&orders,|_|Ok(report(3))).unwrap();
        assert_eq!(integrity(&tx,"db").unwrap()["internallyConsistent"],true);tx.commit().unwrap();
    }
    #[test]
    fn pending_survives_revocation_and_reactivation_clears_it(){
        let mut c=conn();let ids=vec!["a".into()];let orders=vec![json!({"id":"a"})];
        let tx=c.transaction().unwrap();synchronize(&tx,"db",&ids,&orders,|_|Ok(report(1))).unwrap();tx.commit().unwrap();
        let tx=c.transaction().unwrap();mark_pending(&tx,"db","a").unwrap();tx.commit().unwrap();
        let tx=c.transaction().unwrap();revoke_pending(&tx,"db","a").unwrap();tx.commit().unwrap();
        assert_eq!(total(&c,"db","independentOrders"),0);
        assert_eq!(order_page(&c,"db","",16).unwrap(),ids);
        let tx=c.transaction().unwrap();assert_eq!(integrity(&tx,"db").unwrap()["pendingOrders"],1);
        synchronize(&tx,"db",&ids,&orders,|_|Ok(report(1))).unwrap();
        assert_eq!(integrity(&tx,"db").unwrap()["internallyConsistent"],true);tx.commit().unwrap();
    }
    // Real disposable SQLite files, never the configured business or audit DB.
    struct RecoveryFixture(std::path::PathBuf);
    impl RecoveryFixture{
        fn new()->Self{
            let path=std::env::temp_dir().join(format!("bando-learning-recovery-{}",uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();Self(path)
        }
        fn path(&self)->std::path::PathBuf{self.0.join("fixture.sqlite3")}
    }
    impl Drop for RecoveryFixture{
        fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}
    }
    fn disk(path:&std::path::Path)->Connection{
        let c=Connection::open(path).unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;PRAGMA busy_timeout=2000;").unwrap();
        prepare(&c).unwrap();c
    }
    fn seed_disk(c:&mut Connection){
        let tx=c.transaction().unwrap();
        synchronize(&tx,"fixture",&["a".into()],&[json!({"id":"a"})],|_|Ok(report(3))).unwrap();
        tx.commit().unwrap();
    }

    // Run in a separate test-harness process. Wait with a bounded deadline
    // so a missing readiness signal never hangs CI. The parent terminates
    // this process while the real write transaction is still open.
    #[test]
    fn recovery_child_uncommitted_writer(){
        let Some(root)=std::env::var_os("BANDO_LEARNING_CRASH_TEST_FIXTURE")else{return;};
        let root=std::path::PathBuf::from(root);
        assert!(root.file_name().and_then(|v|v.to_str()).is_some_and(|v|v.starts_with("bando-learning-recovery-")));
        assert_eq!(root.parent(),Some(std::env::temp_dir().as_path()));
        assert_eq!(std::fs::read_to_string(root.join("test-only")).unwrap(),"isolated-recovery-test");
        let mut c=disk(&root.join("fixture.sqlite3"));
        let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        synchronize(&tx,"fixture",&["a".into(),"b".into()],
            &[json!({"id":"a","revision":2}),json!({"id":"b"})],|_|Ok(report(9))).unwrap();
        mark_pending(&tx,"fixture","a").unwrap();
        assert_eq!(total(&tx,"fixture","weakUnchangedCount"),18);
        std::fs::write(root.join("ready"),b"transaction-open").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        // An unexpectedly surviving child fails; never commit the test write.
        panic!("parent did not terminate uncommitted writer");
    }
    #[test]
    fn killed_writer_recovers_committed_facts_totals_and_cache(){
        use std::time::{Duration,Instant};
        let fixture=RecoveryFixture::new();
        std::fs::write(fixture.0.join("test-only"),b"isolated-recovery-test").unwrap();
        {let mut c=disk(&fixture.path());seed_disk(&mut c);}
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["recovery_child_uncommitted_writer","--nocapture","--test-threads=1"])
            .env("BANDO_LEARNING_CRASH_TEST_FIXTURE",&fixture.0)
            .spawn().unwrap();
        let deadline=Instant::now()+Duration::from_secs(20);
        let ready=loop{
            if fixture.0.join("ready").is_file(){break true;}
            if child.try_wait().unwrap().is_some()||Instant::now()>=deadline{break false;}
            std::thread::sleep(Duration::from_millis(20));
        };
        let killed=child.kill();let status=child.wait().unwrap();
        assert!(ready,"child never reached uncommitted write");killed.unwrap();assert!(!status.success());
        let mut c=disk(&fixture.path());
        assert_eq!(total(&c,"fixture","weakUnchangedCount"),3);
        assert_eq!(total(&c,"fixture","independentOrders"),1);
        assert_eq!(order_page(&c,"fixture","",16).unwrap(),vec!["a".to_owned()]);
        let tx=c.transaction().unwrap();
        assert_eq!(integrity(&tx,"fixture").unwrap()["internallyConsistent"],true);
        let result=synchronize(&tx,"fixture",&["a".into()],&[json!({"id":"a"})],|_|panic!("committed cache should survive")).unwrap();
        assert_eq!(result["reusedOrders"],1);tx.commit().unwrap();
    }
    #[test]
    fn committed_pending_marker_survives_restart_and_replay(){
        let fixture=RecoveryFixture::new();
        {
            let mut c=disk(&fixture.path());seed_disk(&mut c);
            let tx=c.transaction().unwrap();mark_pending(&tx,"fixture","a").unwrap();tx.commit().unwrap();
            // Simulate restart between the two quarantine transactions.
        }
        {
            let mut c=disk(&fixture.path());
            let tx=c.transaction().unwrap();
            assert_eq!(integrity(&tx,"fixture").unwrap()["pendingOrders"],1);
            revoke_pending(&tx,"fixture","a").unwrap();tx.commit().unwrap();
        }
        let mut c=disk(&fixture.path());
        assert_eq!(total(&c,"fixture","independentOrders"),0);
        assert_eq!(order_page(&c,"fixture","",16).unwrap(),vec!["a".to_owned()]);
        let tx=c.transaction().unwrap();
        synchronize(&tx,"fixture",&["a".into()],&[json!({"id":"a","revision":2})],|_|Ok(report(4))).unwrap();
        assert_eq!(integrity(&tx,"fixture").unwrap()["internallyConsistent"],true);tx.commit().unwrap();
        assert_eq!(total(&c,"fixture","weakUnchangedCount"),4);
    }
    #[test]
    fn disk_replay_recovers_lost_notifications_and_replaced_source_set(){
        let fixture=RecoveryFixture::new();
        {
            let mut c=disk(&fixture.path());
            let tx=c.transaction().unwrap();
            synchronize(&tx,"fixture",&["a".into(),"b".into()],&[json!({"id":"a"}),json!({"id":"b"})],|_|Ok(report(3))).unwrap();
            synchronize(&tx,"other",&["a".into()],&[json!({"id":"a"})],|_|Ok(report(7))).unwrap();
            tx.commit().unwrap();
        }
        // Simulate the authoritative source set after a replacement:
        // 'a' changed, 'b' disappeared, 'c' was added. No wakeup survived.
        let mut c=disk(&fixture.path());
        let authoritative=vec![json!({"id":"a","revision":2}),json!({"id":"c"})];
        let source_ids=["a".to_owned(),"c".to_owned()];
        let ids=order_page(&c,"fixture","",16).unwrap().into_iter().chain(source_ids)
            .collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        let tx=c.transaction().unwrap();
        let r=synchronize(&tx,"fixture",&ids,&authoritative,|_|Ok(report(2))).unwrap();
        assert_eq!(r["changedOrders"],3);
        assert_eq!(integrity(&tx,"fixture").unwrap()["internallyConsistent"],true);tx.commit().unwrap();
        assert_eq!(total(&c,"fixture","weakUnchangedCount"),4);
        assert_eq!(total(&c,"other","weakUnchangedCount"),7);
        assert_eq!(order_page(&c,"fixture","",16).unwrap(),vec!["a".to_owned(),"c".to_owned()]);
    }
    #[test]
    fn support_reads_only_context_and_excludes_target_and_unverified_ledger(){
        let mut c=conn();
        let key=json!([1,"type","base",null,["address",[0,0,1,0,0]]]).to_string();
        let other=json!([1,"type","base",null,["contact",[3,0,0,1,0]]]).to_string();
        let ids=vec!["a".to_owned(),"b".to_owned(),"target".to_owned(),"other".to_owned()];
        let orders=ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>();
        let tx=c.transaction().unwrap();
        synchronize(&tx,"db",&ids,&orders,|o|{
            let mut r=report(if o["id"]=="target"{1000}else{1});
            let mut counts=r["fieldStatistics"][0]["counts"].clone();
            if o["id"]=="b"{
                counts["weakCandidates"]=json!({});
                counts["weakCandidateOrders"]=json!({});
                counts["weakIndependentOrders"]=json!(0);
                counts["weakUnchangedCount"]=json!(0);
                counts["explicitCorrectionCount"]=json!(1);
                counts["explicitCorrectionOrders"]=json!(1);
                counts["explicitPositiveCandidates"]=json!({"name":1});
                counts["explicitNegativeCandidates"]=json!({"address":1});
                counts["explicitPositiveCandidateOrders"]=json!({"name":1});
                counts["explicitNegativeCandidateOrders"]=json!({"address":1});
            }
            r["fieldStatistics"]=json!([]);
            r["fieldContextStatistics"]=json!([{"key":if o["id"]=="other"{&other}else{&key},"counts":counts}]);
            Ok(r)
        }).unwrap();
        tx.commit().unwrap();
        let input=json!({"task":"type","contextKey":key,"targetOrderId":"target",
            "candidates":["address","name"],"explicitToWeakRatio":3.0});
        let tx=c.transaction().unwrap();
        let result=assess_support(&tx,"db",&input).unwrap();
        let keys=BTreeSet::from([input["contextKey"].as_str().unwrap().to_owned()]);
        let members=shadow_capture_contexts(&tx,"db","target",&keys).unwrap();
        let mut verified=BTreeMap::new();
        for id in members.values().flatten(){
            let digest:String=tx.query_row("SELECT fingerprint FROM recognition_learning_fingerprints WHERE scope='db' AND order_id=?1",
                [id],|r|r.get(0)).unwrap();
            verified.insert(id.clone(),verified_facts(&tx,"db",id,&digest).unwrap().unwrap());
        }
        assert_eq!(verified.len(),2);
        assert_eq!(assess_verified_support(&input,&members,&verified).unwrap(),result);
        let mut narrowed=input.clone();narrowed["candidates"]=json!(["address"]);
        assert_eq!(assess_verified_support(&narrowed,&members,&verified).unwrap(),assess_support(&tx,"db",&narrowed).unwrap());
        let mut missing=verified;let first=missing.keys().next().unwrap().clone();missing.remove(&first);
        assert!(assess_verified_support(&input,&members,&missing).is_err());
        assert_eq!(result["contextOrders"],2);
        assert_eq!(result["supportedOrders"],2);
        assert_eq!(result["explicitOnlyOrders"],1);
        assert!((result["candidates"][1]["supportShare"].as_f64().unwrap()-0.75).abs()<1e-9);
        assert_eq!(result["candidates"][0]["explicitOppositionMass"],1.0);
        assert_eq!(result["eligibleForAutomaticDecision"],false);
        let mut invalid=input.clone();invalid["targetOrderId"]=Value::Null;
        assert!(assess_support(&tx,"db",&invalid).is_err());
        mark_pending(&tx,"db","a").unwrap();
        assert!(assess_support(&tx,"db",&input).is_err());
        clear_pending(&tx,"db","a").unwrap();
        tx.execute("UPDATE recognition_learning_facts SET amount=2 WHERE order_id='a' AND metric='weakCandidates'",[]).unwrap();
        assert!(assess_support(&tx,"db",&input).is_err());
    }

    #[test]
    fn owner_capture_only_reads_requested_contexts_excludes_target_and_blocks_pending(){
        let mut c=conn();
        let tx=c.transaction().unwrap();
        let ids=vec!["a".to_owned(),"target".to_owned(),"other".to_owned(),"field".to_owned()];
        let owner=json!([1,"recipient_ownership","base",null,["address",true,true]]).to_string();
        let other=json!([1,"recipient_ownership","base",null,["address",false,true]]).to_string();
        let field=json!([1,"type","base",null,["address",[0,0,1,0,0]]]).to_string();
        let orders=ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>();
        synchronize(&tx,"db",&ids,&orders,|o|{
            let mut r=report(1);
            let counts=r["fieldStatistics"][0]["counts"].clone();
            r["fieldStatistics"]=json!([]);
            if o["id"]=="field"{
                r["fieldContextStatistics"]=json!([{"key":field,"counts":counts}]);
            }else{
                r["statistics"]=json!([{"key":if o["id"]=="other"{&other}else{&owner},"counts":counts}]);
            }
            Ok(r)
        }).unwrap();
        let keys=BTreeSet::from([owner.clone(),field]);
        assert_eq!(shadow_capture_ids(&tx,"db","target",&keys).unwrap(),vec!["a".to_owned(),"field".to_owned()]);
        assert!(shadow_capture_ids(&tx,"different","target",&keys).unwrap().is_empty());
        let excessive=(0..33).map(|n|n.to_string()).collect::<BTreeSet<_>>();
        assert!(shadow_capture_ids(&tx,"db","target",&excessive).is_err());
        mark_pending(&tx,"db","a").unwrap();
        assert!(shadow_capture_ids(&tx,"db","target",&keys).is_err());
    }
    #[test]
    fn local_replay_uses_only_pre_cutoff_current_verified_training_orders(){
        let mut c=conn();
        let key=json!([1,"type","base",null,["address",[0,0,1,0,0]]]).to_string();
        let order=|id:&str,at:&str,confirmed:&str|json!({
            "id":id,"learningActive":true,"confirmedAt":confirmed,
            "confirmationSnapshot":{"at":confirmed,"recognitionBaseline":{"initialAnalysis":{
                "initialPredictionCapture":{"schemaVersion":1,"origin":"native_reception_worker",
                    "orderId":id,"analysisStartedAt":at,"analysisCompletedAt":at}}}},
            "recognitionAuditEvents":[]});
        let orders=vec![
            order("train","2026-01-01T00:00:00Z","2026-01-02T00:00:00Z"),
            order("fit","2026-02-02T00:00:00Z","2026-02-03T00:00:00Z"),
            order("test","2026-03-02T00:00:00Z","2026-03-03T00:00:00Z"),
            order("target","2026-01-01T00:00:00Z","2026-01-02T00:00:00Z")];
        let ids=orders.iter().map(|o|o["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
        let digests=orders.iter().map(|o|(o["id"].as_str().unwrap().to_owned(),fingerprint(o).unwrap())).collect::<BTreeMap<_,_>>();
        let tx=c.transaction().unwrap();
        synchronize_with_digests(&tx,"db",&ids,&digests,&orders,|o|{
            let mut r=report(1);
            let mut counts=r["fieldStatistics"][0]["counts"].clone();
            if o["id"]!="train"{
                counts["weakCandidates"]=json!({});counts["weakCandidateOrders"]=json!({});
                counts["weakIndependentOrders"]=json!(0);counts["weakUnchangedCount"]=json!(0);
                counts["explicitCorrectionCount"]=json!(1);counts["explicitCorrectionOrders"]=json!(1);
                counts["explicitPositiveCandidates"]=json!({"name":1});
                counts["explicitNegativeCandidates"]=json!({"address":1});
                counts["explicitPositiveCandidateOrders"]=json!({"name":1});
                counts["explicitNegativeCandidateOrders"]=json!({"address":1});
            }
            r["fieldStatistics"]=json!([]);
            r["fieldContextStatistics"]=json!([{"key":key,"counts":counts}]);
            Ok(r)
        }).unwrap();
        tx.commit().unwrap();
        let input=json!({"task":"type","contextKey":key,"targetOrderId":"target",
            "candidates":["address","name"],"explicitToWeakRatio":3.0,
            "localChronologicalReplay":{"trainingDataThrough":"2026-01-15T00:00:00Z",
                "calibrationStart":"2026-02-01T00:00:00Z","testStart":"2026-03-01T00:00:00Z",
                "testEnd":"2026-04-01T00:00:00Z","evaluationAsOf":"2026-04-02T00:00:00Z"}});
        let tx=c.transaction().unwrap();
        let v=assess_local_replay(&tx,"db",&input,&orders,&digests).unwrap();
        assert_eq!(v["trainingOrders"],1);assert_eq!(v["fitOrders"],1);assert_eq!(v["testOrders"],1);
        assert_eq!(v["excludedInactiveOrTargetOrders"],1);
        assert_eq!(v["rawBrierScoreOnCoveredTest"],1.0);assert_eq!(v["calibratedBrierScoreOnCoveredTest"],0.0);
        assert_eq!(v["currentSourceDigestsVerified"],true);
        assert_eq!(v["historicalModelVerified"],false);
        assert_eq!(v["eligibleForAutomaticDecision"],false);
        let mut stale=digests.clone();stale.insert("train".into(),"f".repeat(64));
        assert!(assess_local_replay(&tx,"db",&input,&orders,&stale).is_err());
        mark_pending(&tx,"db","train").unwrap();
        assert!(assess_local_replay(&tx,"db",&input,&orders,&digests).is_err());
    }
    #[test]
    fn failed_batch_rolls_back_all_deltas(){
        let mut c=conn();let ids=vec!["a".into(),"b".into()];let orders=vec![json!({"id":"a"}),json!({"id":"b"})];
        {let tx=c.transaction().unwrap();
         assert!(synchronize(&tx,"db",&ids,&orders,|o|if o["id"]=="a"{Ok(report(1))}else{Err("test".into())}).is_err());}
        assert_eq!(total(&c,"db","independentOrders"),0);
        assert_eq!(c.query_row("SELECT count(*) FROM recognition_learning_facts",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(c.query_row("SELECT count(*) FROM recognition_learning_fingerprints",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}
