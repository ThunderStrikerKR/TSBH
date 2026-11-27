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
type Fact=(String,String,String,String,i64);

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
    for (family,key,metric,candidate,n) in old{
        let changed=tx.execute("UPDATE recognition_learning_totals SET amount=amount-?7
            WHERE scope=?1 AND feature_version=?2 AND family=?3 AND context_key=?4 AND metric=?5 AND candidate=?6 AND amount>=?7",
            params![scope,FEATURE_VERSION,family,key,metric,candidate,n]).map_err(|e|e.to_string())?;
        if changed!=1{return Err("기존 학습 합계 불일치: 갱신을 중단했습니다.".into());}
    }
    tx.execute("DELETE FROM recognition_learning_orders WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    if !new.is_empty(){
        tx.execute("INSERT INTO recognition_learning_orders VALUES(?1,?2,?3)",params![scope,FEATURE_VERSION,id]).map_err(|e|e.to_string())?;
    }
    for (family,key,metric,candidate,n) in new{
        tx.execute("INSERT INTO recognition_learning_facts VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![scope,FEATURE_VERSION,id,family,key,metric,candidate,n]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO recognition_learning_totals VALUES(?1,?2,?3,?4,?5,?6,?7)
            ON CONFLICT(scope,feature_version,family,context_key,metric,candidate) DO UPDATE SET amount=amount+excluded.amount",
            params![scope,FEATURE_VERSION,family,key,metric,candidate,n]).map_err(|e|e.to_string())?;
    }
    tx.execute("DELETE FROM recognition_learning_totals WHERE scope=?1 AND feature_version=?2 AND amount=0",
        params![scope,FEATURE_VERSION]).map_err(|e|e.to_string())?;
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
    let count:Option<i64>=tx.query_row("SELECT fact_count FROM recognition_learning_fingerprints
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3 AND contribution_version=?4 AND fingerprint=?5",
        params![scope,FEATURE_VERSION,id,CONTRIBUTION_VERSION,digest],|r|r.get(0))
        .optional().map_err(|e|e.to_string())?;
    let Some(expected)=count else{return Ok(None);};
    // A missing ledger/facts row must not be hidden by a cached digest.
    let (actual,has_order):(i64,i64)=tx.query_row("SELECT
        (SELECT count(*) FROM recognition_learning_facts WHERE scope=?1 AND feature_version=?2 AND order_id=?3),
        (SELECT count(*) FROM recognition_learning_orders WHERE scope=?1 AND feature_version=?2 AND order_id=?3)",
        params![scope,FEATURE_VERSION,id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    if expected!=actual || has_order!=i64::from(expected>0){
        return Err("학습 지문과 기여 원장이 불일치합니다.".into());
    }
    let proof:Option<String>=tx.query_row("SELECT digest FROM recognition_learning_fact_proofs
        WHERE scope=?1 AND feature_version=?2 AND order_id=?3",
        params![scope,FEATURE_VERSION,id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    // Earlier builds have no proof. Re-project from the authoritative
    // source rather than blessing the existing derived facts.
    let Some(proof)=proof else{return Ok(None);};
    if fact_proof(&existing(tx,scope,id)?)?!=proof{
        return Err("학습 기여량 내용 지문 불일치".into());
    }
    Ok(Some(expected))
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
                reused+=1;
                if count>0{eligible+=1;}
                clear_pending(tx,scope,id)?;
                continue;
            }
            let order=by_id.get(id).ok_or("변경된 학습 주문 원본 누락")?;
            let new=facts(&summarize(order)?)?;
            recalculated+=1;
            if !new.is_empty(){eligible+=1;}
            if replace(tx,scope,id,&new)?{changed+=1;}
            save_fingerprint(tx,scope,id,digest,&new)?;
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

pub(super) fn assess_support(tx:&Transaction<'_>,scope:&str,input:&Value)->Result<Value,String>{
    recognition_support::assess(tx,scope,input)
}

// Bound foreground shadow capture before reading and verifying any full facts.
pub(super) fn owner_capture_ids(tx:&Transaction<'_>,scope:&str,target:&str,keys:&BTreeSet<String>)
    ->Result<Vec<String>,String>{
    if keys.is_empty()||keys.len()>8{return Err("실시간 학습 문맥 한도 오류".into());}
    let pending:i64=tx.query_row("SELECT count(*) FROM recognition_learning_pending WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending!=0{return Err("복구 대기 학습 주문".into());}
    let mut stmt=tx.prepare("SELECT DISTINCT order_id FROM recognition_learning_facts
        WHERE scope=?1 AND feature_version=?2 AND family='statistics' AND context_key=?3
        AND order_id<>?4 ORDER BY order_id LIMIT 251").map_err(|e|e.to_string())?;
    let mut ids=BTreeSet::new();
    for key in keys{
        let rows=stmt.query_map(params![scope,FEATURE_VERSION,key,target],|r|r.get::<_,String>(0))
            .map_err(|e|e.to_string())?;
        for id in rows{
            ids.insert(id.map_err(|e|e.to_string())?);
            if ids.len()>250{return Err("실시간 학습 주문 한도 초과".into());}
        }
    }
    Ok(ids.into_iter().collect())
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
    for table in ["recognition_learning_fact_proofs","recognition_learning_fingerprints",
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
        let ids=vec!["a".to_owned(),"target".to_owned(),"other".to_owned()];
        let orders=ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>();
        synchronize(&tx,"db",&ids,&orders,|o|{
            let mut r=report(1);
            let counts=r["fieldStatistics"][0]["counts"].clone();
            r["fieldStatistics"]=json!([]);
            r["statistics"]=json!([{"key":if o["id"]=="other"{"other"}else{"wanted"},"counts":counts}]);
            Ok(r)
        }).unwrap();
        let keys=BTreeSet::from(["wanted".to_owned()]);
        assert_eq!(owner_capture_ids(&tx,"db","target",&keys).unwrap(),vec!["a".to_owned()]);
        assert!(owner_capture_ids(&tx,"different","target",&keys).unwrap().is_empty());
        let excessive=(0..9).map(|n|n.to_string()).collect::<BTreeSet<_>>();
        assert!(owner_capture_ids(&tx,"db","target",&excessive).is_err());
        mark_pending(&tx,"db","a").unwrap();
        assert!(owner_capture_ids(&tx,"db","target",&keys).is_err());
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
