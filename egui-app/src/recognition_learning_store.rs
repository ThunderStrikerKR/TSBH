//! Versioned derived recognition counts. No source document or recipient
//! values are copied into this store. Never use this diagnostic cache to
//! change a recognition decision until lifecycle reconciliation is wired.
use rusqlite::{params,Connection,Transaction};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

const FEATURE_VERSION:i64=1;
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
        PRIMARY KEY(scope,feature_version,family,context_key,metric,candidate));")
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

/// The caller holds an IMMEDIATE transaction in the audit database across
/// snapshot read, per-order projection, delta replacement and revalidation.
pub(super) fn synchronize(tx:&Transaction<'_>,scope:&str,ids:&[String],orders:&[Value],
    summarize:impl Fn(&Value)->Result<Value,String>)->Result<Value,String>{
    let requested=ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    if ids.is_empty()||ids.len()>250||requested.iter().any(|id|id.trim().is_empty()){
        return Err("학습 갱신은 1~250개 주문 ID를 명시해야 합니다.".into());
    }
    let mut by_id=BTreeMap::new();
    for order in orders{
        let id=order["id"].as_str().filter(|id|requested.contains(id)).ok_or("학습 주문 범위 오류")?;
        if by_id.insert(id,order).is_some(){return Err("학습 주문 중복".into());}
    }
    let mut changed=0;
    let mut eligible=0;
    for id in requested.iter().copied(){
        let new=match by_id.get(id){
            Some(order)=>facts(&summarize(order)?)?,
            None=>Vec::new(),
        };
        if !new.is_empty(){eligible+=1;}
        if replace(tx,scope,id,&new)?{changed+=1;}
    }
    let stored:i64=tx.query_row("SELECT count(*) FROM recognition_learning_orders WHERE scope=?1 AND feature_version=?2",
        params![scope,FEATURE_VERSION],|r|r.get(0)).map_err(|e|e.to_string())?;
    Ok(json!({"status":"persisted_diagnostic_only","featureVersion":FEATURE_VERSION,
        "requestedOrders":requested.len(),"changedOrders":changed,"eligibleSelectedOrders":eligible,
        "storedContributingOrders":stored,"missingSelectedOrders":requested.len()-by_id.len(),
        "automaticDecisionEnabled":false,"fullHistoryReconciled":false}))
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
        synchronize(&tx,"db",&ids,&orders[..1],|_|Ok(report(2))).unwrap();tx.commit().unwrap();
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
        synchronize(&tx,"first",&ids,&orders,|_|{let mut r=report(1);r["auditCoverage"]["verifiedManifestOrders"]=json!(0);Ok(r)}).unwrap();tx.commit().unwrap();
        assert_eq!(total(&c,"first","independentOrders"),0);
        assert_eq!(total(&c,"second","independentOrders"),1);
    }
    #[test]
    fn failed_batch_rolls_back_all_deltas(){
        let mut c=conn();let ids=vec!["a".into(),"b".into()];let orders=vec![json!({"id":"a"}),json!({"id":"b"})];
        {let tx=c.transaction().unwrap();
         assert!(synchronize(&tx,"db",&ids,&orders,|o|if o["id"]=="a"{Ok(report(1))}else{Err("test".into())}).is_err());}
        assert_eq!(total(&c,"db","independentOrders"),0);
        assert_eq!(c.query_row("SELECT count(*) FROM recognition_learning_facts",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}
