//! Versioned derived recognition counts. No source document or recipient
//! values are copied into this store. Never use this diagnostic cache to
//! change a recognition decision until lifecycle reconciliation is wired.
use rusqlite::{params,Connection,OptionalExtension,Transaction};
use sha2::{Digest,Sha256};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};


const FEATURE_VERSION:i64=2;
// Bump whenever the meaning of a contribution or its input changes,
// even if the public context feature keys stay the same.
const CONTRIBUTION_VERSION:i64=3;
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
        if next.get(&(family.clone(),k.clone(),metric.clone(),candidate.clone()))==Some(n){
            let valid:i64=tx.query_row("SELECT count(*) FROM recognition_learning_totals
                WHERE scope=?1 AND feature_version=?2 AND family=?3 AND context_key=?4 AND metric=?5 AND candidate=?6 AND amount>=?7",
                params![scope,FEATURE_VERSION,family,k,metric,candidate,n],|r|r.get(0)).map_err(|e|e.to_string())?;
            if valid!=1{return Err("기존 학습 합계 불일치: 갱신을 중단했습니다.".into());}
            continue;
        }
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
    // Preserve the exact v1 JSON byte stream without allocating its
    // complete serialized copy for each verified order.
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter{
        fn write(&mut self,buf:&[u8])->std::io::Result<usize>{
            self.0.update(buf);Ok(buf.len())
        }
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    let mut writer=HashWriter(Sha256::new());
    writer.0.update(b"bando-learning-facts-v1\0");
    serde_json::to_writer(&mut writer,facts).map_err(|_|"학습 기여 지문 생성 실패")?;
    Ok(format!("{:x}",writer.0.finalize()))
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


// The old diff writer is authoritative. No append-only duplicate votes:
// each confirmed order replaces its prior contribution in a transaction.
pub(super) struct Prepared{
    id:String,
    digest:String,
    facts:Vec<Fact>,
}
pub(super) fn prepare_order(order:&Value,report:&Value)->Result<Prepared,String>{
    let id=order["id"].as_str().filter(|v|!v.is_empty()).ok_or("학습 주문 ID 누락")?;
    let facts=facts(report)?;
    for (_,key,_,_,_) in &facts{
        let v:Value=serde_json::from_str(key).map_err(|_|"패턴 키 형식 오류")?;
        if v["pattern"]["version"]!=1||!v["pattern"]["roles"].is_array(){
            return Err("독립 패턴 키 누락".into());
        }
    }
    Ok(Prepared{id:id.to_owned(),digest:fingerprint(order)?,facts})
}
pub(super) fn apply(tx:&Transaction<'_>,scope:&str,prepared:&Prepared)->Result<(),String>{
    replace(tx,scope,&prepared.id,&prepared.facts)?;
    save_fingerprint(tx,scope,&prepared.id,&prepared.digest,&prepared.facts)?;
    clear_pending(tx,scope,&prepared.id)
}
pub(super) fn pattern_totals(conn:&Connection,scope:&str,pattern:&Value)->Result<Vec<Value>,String>{
    let mut stmt=conn.prepare("SELECT context_key,metric,candidate,amount
        FROM recognition_learning_totals WHERE scope=?1 AND feature_version=?2
        AND json_extract(context_key,'$.pattern')=json(?3)
        ORDER BY context_key,metric,candidate").map_err(|e|e.to_string())?;
    let rows=stmt.query_map(params![scope,FEATURE_VERSION,pattern.to_string()],|r|
        Ok(json!({"key":r.get::<_,String>(0)?,"metric":r.get::<_,String>(1)?,
            "candidate":r.get::<_,String>(2)?,"amount":r.get::<_,i64>(3)?})))
        .map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
#[cfg(test)]
mod pattern_learning_ledger_tests{
    use super::*;
    fn prepared(id:&str,role:u16,amount:i64)->Prepared{
        let key=json!({"pattern":{"version":1,"roles":[role]},"task":"type","feature":null,"client":null}).to_string();
        let facts=if amount>0{vec![("fieldContextStatistics".into(),key,"weakCandidates".into(),"name".into(),amount)]}else{vec![]};
        Prepared{id:id.into(),digest:format!("{:064x}",amount),facts}
    }
    #[test]
    fn pattern_ledger_replaces_and_retracts_only_the_affected_order(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        prepare(&conn).unwrap();
        let a=prepared("a",1,2);let b=prepared("b",2,1);
        let p=json!({"version":1,"roles":[2]});
        let tx=conn.transaction().unwrap();apply(&tx,"db",&a).unwrap();apply(&tx,"db",&b).unwrap();tx.commit().unwrap();
        let baseline=pattern_totals(&conn,"db",&p).unwrap();
        assert_eq!(baseline.len(),1);assert_eq!(baseline[0]["amount"],1);
        let tx=conn.transaction().unwrap();
        apply(&tx,"db",&a).unwrap();apply(&tx,"db",&prepared("a",1,3)).unwrap();
        apply(&tx,"db",&prepared("a2",1,5)).unwrap();tx.commit().unwrap();
        assert_eq!(pattern_totals(&conn,"db",&p).unwrap(),baseline);
        let tx=conn.transaction().unwrap();mark_pending(&tx,"db","a").unwrap();
        revoke_pending(&tx,"db","a").unwrap();tx.commit().unwrap();
        assert_eq!(pattern_totals(&conn,"db",&p).unwrap(),baseline);
        let other=pattern_totals(&conn,"db",&json!({"version":1,"roles":[1]})).unwrap();
        assert_eq!(other[0]["amount"],5);
        assert!(pattern_totals(&conn,"another-db",&p).unwrap().is_empty());
    }
}
