//! Same authoritative v2 snapshot transaction for Tauri and egui.
//! Validates the current and requested state, synchronizes core relation rows,
//! legacy indexes and immutable receipt archive, and commits atomically.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json,Value};
use crate::{validate_brands,validate_clients};
fn err<E:std::fmt::Display>(e:E)->String{e.to_string()}
#[path = "../../../src-tauri/src/reception.rs"]
pub(super) mod reception;

fn read_current_envelope(tx:&Transaction<'_>,expected_revision:u64)->Result<Value,String>{
    let version:i64=tx.query_row(
        "SELECT version FROM core_schema WHERE key='business-core'",
        [],|row|row.get(0),
    ).map_err(err)?;
    if !crate::supported_relational_version(version) {return Err("지원하지 않는 운영 DB 버전입니다.".into());}
    // The envelope owns revision. Reject stale writers before loading any
    // relational rows, using the same transaction snapshot.
    let current=crate::read_envelope(tx)?;
    if current["revision"].as_u64()!=Some(expected_revision){
        return Err("다른 작업에서 데이터가 수정되었습니다. 새로고침 후 다시 저장하세요.".into());
    }
    Ok(current)
}

fn read_current(tx:&Transaction<'_>,expected_revision:u64)->Result<Value,String>{
    let mut current=read_current_envelope(tx,expected_revision)?;
    crate::hydrate_v2(tx,&mut current)?;
    Ok(current)
}

pub fn write_v2_snapshot(conn: &mut Connection, mut state: Value, expected_revision: u64, validate_orders: bool) -> Result<Value, String> {
    crate::product_colors::normalize_products(&mut state)?;
    crate::valid_state(&state)?;
    let tx=conn.transaction().map_err(err)?;
    let current=read_current(&tx,expected_revision)?;
    let (written,_)=write_validated(&tx,&current,state,expected_revision,validate_orders,false)?;
    tx.commit().map_err(err)?;
    Ok(written)
}

/// A native read-modify-write uses one consistent before-image for the
/// reducer AND all write validations. No UI-supplied snapshot is trusted.
/// The deferred transaction rolls back on reducer/validation errors or if
/// a concurrent writer makes the read snapshot impossible to upgrade.
pub fn update_v2_snapshot<F>(
    conn:&mut Connection,expected_revision:u64,validate_orders:bool,mutate:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<Value,String>{
    update_v2_snapshot_if_changed(conn,expected_revision,validate_orders,
        |current|mutate(current).map(Some))
}

/// A no-op decision is made against the authoritative transaction before
/// image. It never increments revision or executes a write. Changed states
/// keep the existing full validation and scoped readback contracts.
pub fn update_v2_snapshot_if_changed<F>(
    conn:&mut Connection,expected_revision:u64,validate_orders:bool,mutate:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<Option<Value>,String>{
    let tx=conn.transaction().map_err(err)?;
    let current=read_current(&tx,expected_revision)?;
    let verified=match mutate(&current)?{
        Some(state)=>finish_update(&tx,current,state,expected_revision,validate_orders)?,
        None=>current,
    };
    tx.commit().map_err(err)?;
    Ok(verified)
}

/// A scoped compare-and-set reducer decides conflicts from the authoritative
/// entity before-image, rather than a stale global UI revision. Acquire the
/// write reservation before reading, so unrelated commits cannot race the
/// comparison or overwrite the validated state.
pub fn update_v2_entity_if_changed<F>(
    conn:&mut Connection,validate_orders:bool,mutate:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<Option<Value>,String>{
    update_v2_entity_with_color_policy(conn,validate_orders,true,mutate)
}

// A reviewed workbook already normalizes explicitly edited/new color cells.
// Do not apply the unrelated legacy color migration during its commit.
pub(crate) fn update_v2_reviewed_workbook<F>(conn:&mut Connection,mutate:F)
    ->Result<Value,String>
where F:FnOnce(&Value)->Result<Option<Value>,String>{
    update_v2_entity_with_color_policy(conn,true,false,mutate)
}
// Brand deletion changes only explicitly approved product brandId links.
// Avoid unrelated color migrations during this narrow catalog mutation.
pub(crate) fn update_v2_brand_delete<F>(conn:&mut Connection,mutate:F)
    ->Result<Value,String>
where F:FnOnce(&Value)->Result<Option<Value>,String>{
    update_v2_entity_with_color_policy(conn,true,false,mutate)
}
// Category edits must never trigger an unrelated legacy color migration.
pub(crate) fn update_v2_category_change<F>(conn:&mut Connection,mutate:F)
    ->Result<Value,String>
where F:FnOnce(&Value)->Result<Option<Value>,String>{
    update_v2_entity_with_color_policy(conn,true,false,mutate)
}
fn update_v2_entity_with_color_policy<F>(
    conn:&mut Connection,validate_orders:bool,normalize_colors:bool,mutate:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<Option<Value>,String>{
    let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let revision=crate::read_envelope(&tx)?["revision"].as_u64()
        .ok_or("운영 DB 리비전 오류")?;
    let current=read_current(&tx,revision)?;
    let verified=match mutate(&current)?{
        Some(mut state)=>{
            if normalize_colors{crate::product_colors::normalize_products(&mut state)?;}
            finish_validated_update(&tx,current,state,revision,validate_orders,false)?
        },
        None=>current,
    };
    tx.commit().map_err(err)?;
    Ok(verified)
}

/// Native automatic factory reply: only candidate order trees enter the
/// reducer. All shipments and payment evidence remain visible for collision
/// and immutability checks. Hydrate the complementary trees exactly once
/// for the original global validator and transactional writer.
pub fn update_v2_factory_reply_snapshot<F>(
    conn:&mut Connection,expected_revision:u64,source:&Value,preserve:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<String,String>{
    let tx=conn.transaction().map_err(err)?;
    let mut before=read_current_envelope(&tx,expected_revision)?;
    let mut payload=source.clone();
    payload["actor"]=json!(before["settings"]["operatorName"].as_str().unwrap_or(""));
    let (reply,ids)=crate::rules::prepare_automatic_factory_reply(&before,&payload)?;
    hydrate_factory_scope(&tx,&mut before,&ids)?;
    let mut result=crate::rules::apply_prepared_factory_reply(&before,&reply,&payload)?;
    let mut next=result.as_object_mut().and_then(|m|m.remove("next"))
        .ok_or("회신 저장 결과 누락")?;
    validate_factory_scope(&before,&next)?;
    let archived=preserve(&before)?;
    next["factoryReplies"].as_array_mut().and_then(|rows|rows.last_mut())
        .ok_or("회신 이력 누락")?["archivedPath"]=json!(archived);

    let verified=finish_factory_scope(&tx,before,next,&ids,expected_revision)?;
    crate::notification_store::record_factory_reply(&tx,&verified)?;
    tx.commit().map_err(err)?;
    result["next"]=verified;
    Ok(result)
}

/// Native approval shares the automatic reply's relational projection,
/// immutable-core guard, full validator and exact persisted readback.
/// The stored reply and shipment (not UI-provided IDs) define its scope.
pub fn update_v2_factory_correction_snapshot(
    conn:&mut Connection,expected_revision:u64,review:&Value
)->Result<Value,String>{
    let tx=conn.transaction().map_err(err)?;
    let mut before=read_current_envelope(&tx,expected_revision)?;
    let ids=crate::rules::factory_correction_order_scope(&before,review)?;
    hydrate_factory_scope(&tx,&mut before,&ids)?;
    let next=crate::rules::approve_factory_conflict_for_state(&before,review)?;
    validate_factory_scope(&before,&next)?;
    let verified=finish_factory_scope(&tx,before,next,&ids,expected_revision)?;
    crate::notification_store::record_factory_reply(&tx,&verified)?;
    tx.commit().map_err(err)?;
    Ok(verified)
}

fn hydrate_factory_scope(tx:&Transaction<'_>,before:&mut Value,ids:&[String])->Result<(),String>{
    // Partitioned item queries would otherwise hide orphan items in a
    // pre-existing damaged database. Preserve full-reader failure semantics.
    let broken:bool=tx.query_row("SELECT EXISTS(
        SELECT 1 FROM core_orders WHERE json_type(payload,'$.id') IS NOT 'text'
            OR json_extract(payload,'$.id') IS NOT id
        UNION ALL SELECT 1 FROM core_items i WHERE NOT EXISTS(
            SELECT 1 FROM core_recipients r WHERE r.id=i.recipient_id))",
        [],|r|r.get(0)).map_err(err)?;
    if broken{return Err("회신 대상 업무자료의 관계형 연결 오류".into());}
    // The relational tables, never a legacy embedded core list, own orders.
    let target=crate::read_factory_order_partition(tx,ids,false)?;
    if target.iter().any(|order|!order["id"].as_str()
        .is_some_and(|id|ids.iter().any(|expected|expected==id))){
        return Err("회신 대상 주문의 관계형 ID 불일치".into());
    }
    let root=before.as_object_mut().ok_or("업무 상태 형식 오류")?;
    root.remove("products");
    root.remove("clients");
    root.insert("orders".into(),Value::Array(target));
    Ok(())
}

pub(super) fn validate_factory_scope(before:&Value,next:&Value)->Result<(),String>{
    // This reducer may mutate workflow and append reply records, but it
    // must never alter the candidate order trees or other envelope fields.
    let old=before.as_object().ok_or("기존 회신 상태 오류")?;
    let new=next.as_object().ok_or("회신 상태 오류")?;
    let protected=|key:&str|!matches!(key,"workflow"|"factoryReplies");
    if !old.iter().filter(|(k,_)|protected(k)).eq(new.iter().filter(|(k,_)|protected(k))){
        return Err("공장 회신이 허용되지 않은 업무 자료를 변경했습니다.".into());
    }
    Ok(())
}

fn finish_factory_scope(tx:&Transaction<'_>,mut before:Value,mut next:Value,
    ids:&[String],expected_revision:u64)->Result<Value,String>{
    // Reassemble the full snapshot in authoritative SQL order; the scoped
    // rows are reused, so unrelated rows never enter the matching reducer.
    let mut by_id=std::collections::HashMap::new();
    let target=before["orders"].as_array_mut().ok_or("대상 주문 목록 누락")?;
    for order in std::mem::take(target).into_iter()
        .chain(crate::read_factory_order_partition(tx,ids,true)?){
        let id=order["id"].as_str().ok_or("주문 ID 누락")?.to_owned();
        if by_id.insert(id,order).is_some(){return Err("주문 ID 중복".into());}
    }
    let ordered_ids={
        let mut stmt=tx.prepare("SELECT id FROM core_orders ORDER BY position,id").map_err(err)?;
        let rows=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
        rows
    };
    let orders=ordered_ids.into_iter().map(|id|
        by_id.remove(&id).ok_or_else(||"주문 관계형 ID 불일치".to_owned()))
        .collect::<Result<Vec<_>,_>>()?;
    if !by_id.is_empty(){return Err("주문 관계형 ID 불일치".into());}
    let (products,clients)=crate::read_catalog_entities(tx)?;
    before["orders"]=Value::Array(orders);
    before["products"]=Value::Array(products);
    before["clients"]=Value::Array(clients);
    // Scope equality was established before the full trees were attached.
    // Legacy catalog normalization and triggers retain the conservative
    // writer. Otherwise one authoritative core allocation is sufficient.
    if crate::product_colors::needs_normalization(&before)? || has_write_triggers(tx)?{
        for key in ["orders","products","clients"]{next[key]=before[key].clone();}
        return finish_update_with_patch(tx,before,next,expected_revision,true,true);
    }
    move_factory_core(&mut before,&mut next)?;
    finish_factory_shared_core(tx,&before,next,expected_revision)
}

fn has_write_triggers(tx:&Transaction<'_>)->Result<bool,String>{
    tx.query_row("SELECT EXISTS(
        SELECT 1 FROM main.sqlite_master WHERE type='trigger'
        UNION ALL SELECT 1 FROM temp.sqlite_master WHERE type='trigger')",
        [],|row|row.get(0)).map_err(err)
}

fn move_factory_core(before:&mut Value,next:&mut Value)->Result<(),String>{
    let old=before.as_object_mut().ok_or("기존 업무 상태 오류")?;
    let new=next.as_object_mut().ok_or("업무 처리 결과 오류")?;
    for key in ["orders","products","clients"]{
        let core=old.remove(key).filter(Value::is_array).ok_or("기존 업무자료 누락")?;
        new.insert(key.to_owned(),core);
    }
    Ok(())
}

/// Called only after immutable-core scope validation, normalization guard
/// and trigger exclusion. Preserve the same global validators as the
/// generic writer; only the old core lookup borrows the new state's trees.
fn finish_factory_shared_core(tx:&Transaction<'_>,before:&Value,mut next:Value,
    expected_revision:u64)->Result<Value,String>{
    // The caller has proved all non-workflow fields equal to the
    // authoritative before-image, and this path writes only the envelope.
    // Keep structural and workflow integrity checks; unrelated unchanged
    // brand, account and message-template content is not a reply prerequisite.
    crate::validation::validate_shape(&next)?;
    if crate::workflow_state::active(tx)?{
        crate::rules::validate_workflow_transition_shared_core(&next,before)?;
    }
    reception::validate_order_ids(&next)?;
    next["revision"]=json!(expected_revision.checked_add(1)
        .ok_or("운영 DB revision 범위를 초과했습니다.")?);
    let changes=total_changes(tx)?;
    let direct=write_workflow_envelope(tx,before,&next)?;
    if direct==0 || total_changes(tx)?.checked_sub(changes)!=i64::try_from(direct).ok(){
        return Err("공장 회신 저장 중 예상하지 못한 DB 변경이 발생했습니다.".into());
    }
    let mut verified=crate::read_envelope(tx)?;
    verify_factory_envelope(&verified,&next)?;
    // No relational core writes, triggers or unaccounted DML occurred.
    // Move the original authoritative allocation into the returned state;
    // neither clone nor rehydrate any product, client or order tree.
    move_factory_core(&mut next,&mut verified)?;
    Ok(verified)
}

fn verify_factory_envelope(actual:&Value,expected:&Value)->Result<(),String>{
    let actual=actual.as_object().ok_or("저장 업무 상태 오류")?;
    let expected=expected.as_object().ok_or("업무 처리 결과 오류")?;
    let non_core=|key:&str|!matches!(key,"products"|"clients"|"orders");
    if !actual.iter().filter(|(k,_)|non_core(k)).eq(
        expected.iter().filter(|(k,_)|non_core(k))){
        return Err("공장 회신 저장 결과가 요청과 일치하지 않습니다.".into());
    }
    Ok(())
}

/// Compatibility entry point for the existing freight callers.
pub fn update_v2_freight_snapshot<F>(
    conn:&mut Connection,expected_revision:u64,shipment_id:&str,group_id:&str,mutate:F
)->Result<Value,String>
where F:FnOnce(&Value)->Result<Value,String>{
    update_v2_shipment_snapshot(conn,expected_revision,shipment_id,group_id,mutate)
        .map(|(state,_)|state)
}

/// Native shipment freight and group moves share the same single-order
/// transaction. Keep the full workflow envelope and catalog, including
/// audit IDs, payment allocations, tariffs and clients. The boolean is
/// true only when the stable scoped writer proves that unrelated core
/// rows were not modified; callers must use full projection on fallback.
pub fn update_v2_shipment_snapshot<F>(
    conn:&mut Connection,expected_revision:u64,shipment_id:&str,group_id:&str,mutate:F
)->Result<(Value,bool),String>
where F:FnOnce(&Value)->Result<Value,String>{
    let tx=conn.transaction().map_err(err)?;
    let mut before=read_current_envelope(&tx,expected_revision)?;
    let ship=before["workflow"]["shipments"].as_array()
        .and_then(|rows|rows.iter().find(|s|s["id"].as_str()==Some(shipment_id)))
        .ok_or("수정할 배송 건을 확인하세요.")?;
    if ship["groupId"].as_str()!=Some(group_id) ||
        ship["transferred"]==true || ship["legacyArchived"]==true{
        return Err("현재 묶음의 수정 가능한 배송 건을 확인하세요.".into());
    }
    let order_id=ship["orderId"].as_str().filter(|id|!id.trim().is_empty())
        .ok_or("배송 건의 발주서를 찾을 수 없습니다.")?.to_owned();
    let (products,clients)=crate::read_catalog_entities(&tx)?;
    let target=crate::read_order_entities(&tx,Some(&order_id))?;
    if target.len()!=1 || target[0]["id"].as_str()!=Some(order_id.as_str()){
        return Err("배송 건의 발주서를 찾을 수 없습니다.".into());
    }
    let root=before.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    root.insert("products".into(),Value::Array(products));
    root.insert("clients".into(),Value::Array(clients));
    root.insert("orders".into(),Value::Array(target));

    let mut changed=mutate(&before)?;
    // A freight reducer may change only the target order and workflow.
    // In particular, never accept a projected catalog mutation or insertion.
    let old_root=before.as_object().ok_or("기존 업무 상태 오류")?;
    let new_root=changed.as_object().ok_or("업무 처리 결과 오류")?;
    if !old_root.iter().filter(|(k,_)|k.as_str()!="orders" && k.as_str()!="workflow")
        .eq(new_root.iter().filter(|(k,_)|k.as_str()!="orders" && k.as_str()!="workflow")){
        return Err("운임 수정 범위를 벗어난 업무 데이터 변경입니다.".into());
    }
    if changed==before{
        return Err("현재 상태에서 수행할 변경이 없습니다.".into());
    }
    // The fast path is intentionally narrow. No triggers (including TEMP
    // triggers), no structural changes, no new correction or confirmation,
    // and no product migration. The fallback retains the old full writer.
    if stable_freight_candidate(&tx,&before,&changed)?{
        let verified=finish_stable_freight(&tx,before,changed,expected_revision,&order_id)?;
        tx.commit().map_err(err)?;
        return Ok((verified,true));
    }
    let target_after=changed["orders"].as_array_mut()
        .filter(|rows|rows.len()==1 && rows[0]["id"].as_str()==Some(order_id.as_str()))
        .and_then(Vec::pop).ok_or("운임 수정 대상 주문이 변경되었습니다.")?;
    let target_before=before["orders"].as_array_mut()
        .and_then(Vec::pop).ok_or("기존 대상 주문 누락")?;

    // Reconstruct the original full order sequence using the SQL position
    // and ID ordering, not a reducer-provided position or a new append.
    let position:i64=tx.query_row("SELECT COUNT(*) FROM core_orders
        WHERE position<(SELECT position FROM core_orders WHERE id=?1)
        OR (position=(SELECT position FROM core_orders WHERE id=?1) AND id<?1)",
        [&order_id],|row|row.get(0)).map_err(err)?;
    let position=usize::try_from(position).map_err(err)?;
    let mut other=crate::read_order_partition(&tx,Some(&order_id),true)?;
    if position>other.len() || other.iter().any(|o|o["id"].as_str()==Some(order_id.as_str())){
        return Err("운임 수정 주문의 전체 목록 연결 오류".into());
    }
    other.insert(position,target_before);
    before["orders"]=Value::Array(other);
    let mut all=before["orders"].clone();
    all[position]=target_after;
    changed["orders"]=all;
    // The exact old full-workflow validator still checks unrelated
    // shipments, payment links and immutable evidence before any SQL write.
    crate::rules::validate_workflow_transition(&changed,&before)?;
    if changed==before{
        return Err("현재 상태에서 수행할 변경이 없습니다.".into());
    }
    let verified=finish_update(&tx,before,changed,expected_revision,true)?;
    tx.commit().map_err(err)?;
    Ok((verified,false))
}

fn stable_freight_candidate(tx:&Transaction<'_>,before:&Value,changed:&Value)
    ->Result<bool,String>{
    let old=&before["orders"][0];
    let new=&changed["orders"][0];
    if !crate::relational_write::stable_orders(&before["orders"],&changed["orders"]) ||
        !old["confirmedAt"].as_str().is_some_and(|s|!s.is_empty()) ||
        old["confirmedAt"]!=new["confirmedAt"] ||
        old["rawCorrections"]!=new["rawCorrections"]{
        return Ok(false);
    }
    // Existing v2 normalization is still applied on the fallback path.
    // Do not silently omit a pending legacy catalog migration.
    let mut catalog=json!({"products":&changed["products"]});
    if crate::product_colors::normalize_products(&mut catalog)?!=0{return Ok(false);}
    if has_write_triggers(tx)?{return Ok(false);}
    // A corrupt payload ID must not direct an UPSERT at another order's
    // physical row. Verify the target tree's relational ownership first.
    let order_id=old["id"].as_str().ok_or("주문 ID 누락")?;
    let mismatched:bool=tx.query_row("SELECT EXISTS(
        SELECT 1 FROM core_orders WHERE id=?1 AND
            (json_type(payload,'$.id') IS NOT 'text' OR json_extract(payload,'$.id') IS NOT id)
        UNION ALL
        SELECT 1 FROM core_recipients WHERE order_id=?1 AND
            (json_type(payload,'$.id') IS NOT 'text' OR json_extract(payload,'$.id') IS NOT id)
        UNION ALL
        SELECT 1 FROM core_items WHERE recipient_id IN
            (SELECT id FROM core_recipients WHERE order_id=?1) AND
            (json_type(payload,'$.id') IS NOT 'text' OR json_extract(payload,'$.id') IS NOT id))",
        [order_id],|row|row.get(0)).map_err(err)?;
    if mismatched{return Err("운임 수정 대상 주문의 관계형 ID와 업무 데이터가 일치하지 않습니다.".into());}
    Ok(true)
}

fn finish_stable_freight(tx:&Transaction<'_>,mut before:Value,mut changed:Value,
    expected_revision:u64,order_id:&str)->Result<Value,String>{
    // Local invariants checked before DML. Global invariants checked on the
    // full, persisted projection before commit. A failure rolls back all DML.
    crate::valid_state(&changed)?;
    crate::validation::validate_unchanged_clients(&changed)?;
    validate_brands(&before,&changed)?;
    reception::validate_reception(&before,&changed)?;
    if !reception::new_archive_entries(&before,&changed)?.is_empty(){
        return Err("단일 운임 수정은 새 원문 감사 기록을 생성할 수 없습니다.".into());
    }
    let position:i64=tx.query_row("SELECT position FROM core_orders WHERE id=?1",
        [order_id],|row|row.get(0)).map_err(err)?;
    let rank:i64=tx.query_row("SELECT COUNT(*) FROM core_orders
        WHERE position<?1 OR (position=?1 AND id<?2)",
        params![position,order_id],|row|row.get(0)).map_err(err)?;
    let changes_before=total_changes(tx)?;
    let mut direct=crate::relational_write::store_single_order_delta(tx,&before,&changed,position)?;
    if before["orders"]!=changed["orders"]{
        direct+=sync_order_index(tx,&before,&changed)?;
    }
    changed["revision"]=json!(expected_revision.checked_add(1)
        .ok_or("운영 DB revision 범위를 초과했습니다.")?);
    direct+=write_workflow_envelope(tx,&before,&changed)?;
    if total_changes(tx)?.checked_sub(changes_before)!=i64::try_from(direct).ok(){
        return Err("운임 수정 중 예상하지 못한 DB 변경이 발생했습니다.".into());
    }
    // The single-order writer never prunes or writes other orders. With
    // stable ownership, no triggers and matching direct-write accounting,
    // all non-target order rows are unchanged. Hydrate once, after DML,
    // to return the full contract and run the exact global validation.
    let verified=crate::read_v2_snapshot(tx)?;
    let actual=verified["orders"].as_array().ok_or("저장 주문 목록 누락")?;
    let index=usize::try_from(rank).map_err(err)?;
    if actual.get(index)!=Some(&changed["orders"][0]) ||
        actual.get(index).and_then(|o|o["id"].as_str())!=Some(order_id){
        return Err("운임 수정 결과 또는 주문 순서 검증에 실패했습니다.".into());
    }
    let expected_root=changed.as_object().ok_or("업무 처리 결과 오류")?;
    let actual_root=verified.as_object().ok_or("저장 업무 상태 오류")?;
    if !expected_root.iter().filter(|(k,_)|k.as_str()!="orders")
        .eq(actual_root.iter().filter(|(k,_)|k.as_str()!="orders")){
        return Err("운임 수정 저장 결과가 요청과 일치하지 않습니다.".into());
    }
    let target_before=before["orders"].as_array_mut().and_then(Vec::pop)
        .ok_or("기존 대상 주문 누락")?;
    let mut previous_orders=actual.clone();
    previous_orders[index]=target_before;
    before["orders"]=Value::Array(previous_orders);
    crate::valid_state(&verified)?;
    if before["orders"]!=verified["orders"]{
        crate::relational_write::validate_order_tree_ids(&verified)?;
    }
    reception::validate_reception(&before,&verified)?;
    if !reception::new_archive_entries(&before,&verified)?.is_empty(){
        return Err("운임 수정의 전역 원문 감사 검증에 실패했습니다.".into());
    }
    crate::rules::validate_workflow_transition(&verified,&before)?;
    Ok(verified)
}

fn finish_update(tx:&Transaction<'_>,current:Value,state:Value,
    expected_revision:u64,validate_orders:bool)->Result<Value,String>{
    finish_update_with_patch(tx,current,state,expected_revision,validate_orders,false)
}

fn finish_update_with_patch(tx:&Transaction<'_>,current:Value,mut state:Value,
    expected_revision:u64,validate_orders:bool,prefer_workflow_patch:bool)->Result<Value,String>{
    crate::product_colors::normalize_products(&mut state)?;
    finish_validated_update(tx,current,state,expected_revision,validate_orders,prefer_workflow_patch)
}
fn finish_validated_update(tx:&Transaction<'_>,current:Value,state:Value,
    expected_revision:u64,validate_orders:bool,prefer_workflow_patch:bool)->Result<Value,String>{
    crate::valid_state(&state)?;
    let changes_before=total_changes(tx)?;
    let (written,scope)=write_validated(tx,&current,state,expected_revision,validate_orders,prefer_workflow_patch)?;
    verify_written(tx,current,written,scope,changes_before)
}

// Compare the actual SQLite change counter with all direct DML results.
// A trigger/cascade difference forces the original full projection.
fn total_changes(tx:&Transaction<'_>)->Result<i64,String>{
    tx.query_row("SELECT total_changes()",[],|row|row.get(0)).map_err(err)
}

#[derive(Clone,Copy,Default)]
struct WriteScope{
    products:bool,
    clients:bool,
    orders:bool,
    direct:usize,
    verify_envelope:bool,
}

fn verify_written(tx:&Transaction<'_>,current:Value,written:Value,
    scope:WriteScope,changes_before:i64)->Result<Value,String>{
    let revision=written["revision"].as_u64().ok_or("업무 저장 revision 누락")?;
    let actual=total_changes(tx)?.checked_sub(changes_before)
        .and_then(|n|usize::try_from(n).ok());
    let verified=if actual==Some(scope.direct) && scope.direct>0{
        verify_scoped(tx,current,&written,scope)?
    }else{
        drop(current);
        let actual=crate::read_v2_snapshot(tx)?;
        if scope.verify_envelope && actual!=written{
            return Err("공장 회신 저장 결과가 요청과 일치하지 않습니다.".into());
        }
        actual
    };
    if verified["revision"].as_u64()!=Some(revision){
        return Err("업무 저장 검증에 실패했습니다.".into());
    }
    Ok(verified)
}

fn verify_scoped(tx:&Transaction<'_>,mut current:Value,written:&Value,
    scope:WriteScope)->Result<Value,String>{
    let mut envelope=crate::read_envelope(tx)?;
    if scope.verify_envelope{verify_factory_envelope(&envelope,written)?;}
    let root=envelope.as_object_mut().ok_or("업무 상태는 객체여야 합니다.")?;
    let prior=current.as_object_mut().ok_or("기존 업무 상태 오류")?;
    for (key,changed) in [("products",scope.products),("clients",scope.clients),("orders",scope.orders)]{
        let old=prior.remove(key).ok_or("기존 업무자료 누락")?;
        let value=if !changed{old}else{
            match key{
                "products"=>Value::Array(crate::read_product_entities(tx)?),
                "clients"=>Value::Array(crate::read_entities(tx,crate::CORE_CLIENTS_SQL)?),
                "orders"=>Value::Array(verify_order_domain(tx,old,&written["orders"])?),
                _=>unreachable!(),
            }
        };
        root.insert(key.to_owned(),value);
    }
    Ok(envelope)
}

// Stable identities and positions permit indexed single-order readback.
// Structural/bulk edits use the full order-domain projector; a mismatch
// likewise falls back, rather than substituting requested data.
fn verify_order_domain(tx:&Transaction<'_>,old:Value,next:&Value)
    ->Result<Vec<Value>,String>{
    if crate::relational_write::stable_orders(&old,next){
        if let (Value::Array(mut rows),Some(expected))=(old,next.as_array()){
            let changed:Vec<usize>=rows.iter().zip(expected)
                .enumerate().filter_map(|(i,(a,b))|(a!=b).then_some(i)).collect();
            if changed.len()<=8{
                let mut matches=true;
                for i in changed{
                    let id=expected[i]["id"].as_str().ok_or("주문 ID 누락")?;
                    let mut actual=crate::read_order_entities(tx,Some(id))?;
                    if actual.len()!=1 || actual[0]!=expected[i]{
                        matches=false;
                        break;
                    }
                    rows[i]=actual.pop().unwrap();
                }
                if matches{return Ok(rows);}
            }
        }
    }
    crate::read_order_entities(tx,None)
}

fn write_validated(tx:&Transaction<'_>,current:&Value,mut state:Value,
    current_revision:u64,validate_orders:bool,prefer_workflow_patch:bool)->Result<(Value,WriteScope),String>{
    // A category topology change can invalidate unchanged product rows.
    crate::validate_category_transition(current,&state)?;
    let products_changed = current["products"] != state["products"];
    let clients_changed = current["clients"] != state["clients"];
    let orders_changed = current["orders"] != state["orders"];
    if clients_changed {
        validate_clients(current, &state)?;
    }else{
        crate::validation::validate_unchanged_clients(&state)?;
    }
    validate_brands(current, &state)?;
    if crate::workflow_state::active(tx)?{
        // The generic writer has a full authoritative before-image. The
        // stable freight writer instead performs this same full validation
        // after its scoped DML and before COMMIT.
        crate::rules::validate_workflow_transition(&state,current)?;
    }
    if validate_orders {
        if orders_changed {
            reception::validate_reception(current, &state)?;
        }else{
            reception::validate_order_ids(&state)?;
        }
    }
    // Only skip archive generation if equality and unique order IDs were
    // both established. The legacy validate_orders=false route retains
    // its original archive checks.
    let archive_entries=if validate_orders && !orders_changed{
        Vec::new()
    }else{
        reception::new_archive_entries(current,&state)?
    };
    let mut scope=WriteScope{products:products_changed,clients:clients_changed,
        orders:orders_changed,direct:0,verify_envelope:false};
    state["revision"] = json!(current_revision.checked_add(1)
        .ok_or("운영 DB revision 범위를 초과했습니다.")?);
    // The constrained SQLite tables own products, clients and orders.
    // Keep only non-core data in the compatibility envelope, atomically.
    // Settings-only changes never rewrite unchanged core rows.
    if products_changed || clients_changed || orders_changed {
        scope.direct+=crate::relational_write::store_v2_relational_delta(tx,current,&state)?;
    }
    scope.direct+=if crate::workflow_events::active(tx)?{
        crate::workflow_events::write(tx,current,&state)?
    }else if prefer_workflow_patch && !products_changed && !clients_changed && !orders_changed{
        // Automatic factory replies are append-only history plus workflow
        // deltas. Preserve the original full validation above and verify
        // the exact persisted envelope before committing the bounded patch.
        scope.verify_envelope=true;
        write_workflow_envelope(tx,current,&state)?
    }else{
        tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
            [serialize_envelope(&state)?]).map_err(err)?
    };
    // Legacy search indexes are projections of the same before-image.
    // Child-only order edits do not change indexed columns.
    if products_changed {
        scope.direct+=sync_product_index(tx,current,&state)?;
    }
    if orders_changed {
        scope.direct+=sync_order_index(tx,current,&state)?;
    }
    for (id, order_id, record) in archive_entries {
        // Never silently discard a colliding audit ID with a different owner or payload.
        let stored: Option<(String, String)> = tx.query_row(
            "SELECT order_id,record FROM receipt_archive WHERE id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(err)?;
        if let Some((stored_owner, stored_payload)) = stored {
            let stored_record: Value = serde_json::from_str(&stored_payload).map_err(err)?;
            if stored_owner != order_id || stored_record != record {
                return Err("기존 원문 감사 기록과 ID가 충돌합니다. 변경을 저장하지 않았습니다.".into());
            }
        } else {
            scope.direct+=tx.execute(
                "INSERT INTO receipt_archive(id,order_id,record) VALUES (?1,?2,?3)",
                params![id,order_id,serde_json::to_string(&record).map_err(err)?],
            ).map_err(err)?;
        }
    }
    Ok((state,scope))
}


// Only patch known workflow arrays whose existing positions are stable.
// Events are append-only: never replace or delete historical evidence via
// this optimization. An unknown field or large/structural edit falls back
// to the existing full serializer. Bounded SQL arguments stay below SQLite
// JSON function limits even on older bundled versions.
const MAX_ENVELOPE_PATCHES:usize=24;
fn workflow_envelope_patch(before:&Value,next:&Value)->Option<Vec<(String,String)>>{
    let old=before.as_object()?;
    let new=next.as_object()?;
    let ignored=|key:&str|matches!(key,"products"|"clients"|"orders"|"revision"|"workflow"|"factoryReplies");
    if !old.iter().filter(|(k,_)|!ignored(k)).eq(new.iter().filter(|(k,_)|!ignored(k))){
        return None;
    }
    let mut patch=vec![("$.revision".to_owned(),serde_json::to_string(next.get("revision")?).ok()?)];
    // Factory reply history is immutable. Only append new records to an
    // existing array; absent/legacy shapes or edits use the full writer.
    if before.get("factoryReplies")!=next.get("factoryReplies"){
        let prior=before.get("factoryReplies")?.as_array()?;
        let after=next.get("factoryReplies")?.as_array()?;
        if !after.starts_with(prior){return None;}
        for record in &after[prior.len()..]{
            if patch.len()==MAX_ENVELOPE_PATCHES{return None;}
            patch.push(("$.factoryReplies[#]".to_owned(),serde_json::to_string(record).ok()?));
        }
    }
    let old=before.get("workflow")?.as_object()?;
    let new=next.get("workflow")?.as_object()?;
    if old.keys().ne(new.keys()){return None;}
    for (key,after) in new{
        let prior=old.get(key)?;
        if prior==after{continue;}
        if !matches!(key.as_str(),"sources"|"groups"|"shipments"|"payments"|"queue"|
            "events"|"deliveries"|"changeRequests"|"confirmations"){return None;}
        let a=prior.as_array()?;
        let b=after.as_array()?;
        if a.len()>b.len(){return None;}
        for (i,(x,y)) in a.iter().zip(b).enumerate(){
            if x==y{continue;}
            if matches!(key.as_str(),"events"|"sources"|"payments"|"deliveries"|"changeRequests") ||
                x["id"].as_str().filter(|id|!id.is_empty()).is_none() || x["id"]!=y["id"]{
                return None;
            }
            if patch.len()==MAX_ENVELOPE_PATCHES{return None;}
            patch.push((format!("$.workflow.{key}[{i}]"),serde_json::to_string(y).ok()?));
        }
        for row in &b[a.len()..]{
            if patch.len()==MAX_ENVELOPE_PATCHES{return None;}
            patch.push((format!("$.workflow.{key}[#]"),serde_json::to_string(row).ok()?));
        }
    }
    Some(patch)
}

// Keep the schema-v2 compatibility envelope valid for both hosts. SQLite
// still rewrites the JSON document internally; Rust serializes only changed
// rows and appended events. The caller must compare the full persisted
// envelope against the intended state before COMMIT (stable freight does).
fn write_workflow_envelope(tx:&Transaction<'_>,before:&Value,next:&Value)->Result<usize,String>{
    if crate::workflow_events::active(tx)?{
        return crate::workflow_events::write(tx,before,next);
    }
    if let Some(patch)=workflow_envelope_patch(before,next){
        let mut sql="UPDATE app_state SET payload=json_set(payload".to_owned();
        let mut args=Vec::with_capacity(patch.len()*2);
        for (path,value) in patch{
            sql.push_str(",?,json(?)");
            args.push(path);
            args.push(value);
        }
        sql.push_str(") WHERE id=1 AND json_type(payload)='object'
            AND json_type(payload,'$.workflow')='object'
            AND json_type(payload,'$.products') IS NULL
            AND json_type(payload,'$.clients') IS NULL
            AND json_type(payload,'$.orders') IS NULL");
        // SQLite silently ignores [#] on a missing/non-array path. Require
        // the physical history array before applying append operations.
        if sql.contains("json_set") && before.get("factoryReplies")!=next.get("factoryReplies"){
            sql.push_str(" AND json_type(payload,'$.factoryReplies')='array'");
        }
        let count=tx.execute(&sql,rusqlite::params_from_iter(args.iter())).map_err(err)?;
        if count==1{return Ok(count);}
    }
    tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
        [serialize_envelope(next)?]).map_err(err)
}

// Serialize borrowed non-core fields only. Core rows have their own tables;
// do not clone their nested products, variants, orders or items just to
// discard them. Unknown envelope keys are preserved for compatibility.
fn serialize_envelope(state:&Value)->Result<String,String>{
    let root=state.as_object().ok_or("업무 상태는 객체여야 합니다.")?;
    let envelope:std::collections::BTreeMap<&str,&Value>=root.iter()
        .filter(|(key,_)| !matches!(key.as_str(),"products"|"clients"|"orders"))
        .map(|(key,value)|(key.as_str(),value)).collect();
    serde_json::to_string(&envelope).map_err(err)
}

fn index_rows<'a>(value:&'a Value,key:&str)->Result<std::collections::HashMap<&'a str,&'a Value>,String>{
    let mut map=std::collections::HashMap::new();
    for row in value[key].as_array().ok_or("검색 인덱스 목록 오류")?{
        let id=row["id"].as_str().ok_or("검색 인덱스 ID 누락")?;
        if map.insert(id,row).is_some(){return Err("검색 인덱스 ID 중복".into());}
    }
    Ok(map)
}
fn sync_product_index(tx:&Transaction<'_>,before:&Value,state:&Value)->Result<usize,String>{
    let mut direct=0;
    let old=index_rows(before,"products")?;
    let next=index_rows(state,"products")?;
    let mut insert=tx.prepare("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)
        ON CONFLICT(id) DO UPDATE SET code=excluded.code,name=excluded.name,payload=excluded.payload").map_err(err)?;
    for (id,product) in &next{
        if old.get(id)==Some(product){continue;}
        direct+=insert.execute(params![id,product["code"].as_str().ok_or("상품 코드 누락")?,
            product["name"].as_str().ok_or("상품명 누락")?,
            serde_json::to_string(product).map_err(err)?]).map_err(err)?;
    }
    for id in old.keys(){
        if !next.contains_key(id){direct+=tx.execute("DELETE FROM product_index WHERE id=?1",[id]).map_err(err)?;}
    }
    Ok(direct)
}
fn order_index_fields(order:&Value)->(&str,&str,Option<&str>,&str){
    (order["clientId"].as_str().unwrap_or(""),
        order["createdAt"].as_str().unwrap_or(""),order["confirmedAt"].as_str(),
        order["status"].as_str().unwrap_or("REVIEW"))
}
fn sync_order_index(tx:&Transaction<'_>,before:&Value,state:&Value)->Result<usize,String>{
    let mut direct=0;
    let old=index_rows(before,"orders")?;
    let next=index_rows(state,"orders")?;
    let mut insert=tx.prepare("INSERT INTO order_index(id,client_id,created_at,confirmed_at,status)
        VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET
        client_id=excluded.client_id,created_at=excluded.created_at,
        confirmed_at=excluded.confirmed_at,status=excluded.status").map_err(err)?;
    for (id,order) in &next{
        let fields=order_index_fields(order);
        if old.get(id).is_some_and(|row|order_index_fields(row)==fields){continue;}
        direct+=insert.execute(params![id,fields.0,fields.1,fields.2,fields.3]).map_err(err)?;
    }
    for id in old.keys(){
        if !next.contains_key(id){direct+=tx.execute("DELETE FROM order_index WHERE id=?1",[id]).map_err(err)?;}
    }
    Ok(direct)
}

#[cfg(test)]
mod snapshot_cost_tests {
    use super::*;

    #[test]
    fn workflow_json_patch_serializes_only_changed_rows_and_new_events(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let before=json!({"revision":7,"products":[],"clients":[],"orders":[],
            "settings":{"future":true},"unknown":{"nested":[1,null]},
            "workflow":{"shipments":[{"id":"s","charges":{"FACTORY":3000}},
                {"id":"untouched","charges":{"FACTORY":2500}}],
                "events":(0..1000).map(|i|json!({"id":format!("old-{i}"),"text":"기존 감사 기록"})).collect::<Vec<_>>(),
                "groups":[{"id":"g","needsNotice":false}]}});
        let mut next=before.clone();
        next["revision"]=json!(8);
        next["workflow"]["shipments"][0]["charges"]["FACTORY"]=json!(4000);
        next["workflow"]["groups"][0]["needsNotice"]=json!(true);
        next["workflow"]["events"].as_array_mut().unwrap().extend([
            json!({"id":"new-a","before":3000,"after":4000}),
            json!({"id":"new-b","after":{"notice":true}})]);
        let patch=workflow_envelope_patch(&before,&next).unwrap();
        assert_eq!(patch.len(),5);
        assert!(patch.iter().all(|(_,value)|!value.contains("기존 감사 기록")));
        let patch_bytes:usize=patch.iter().map(|(p,v)|p.len()+v.len()).sum();
        assert!(patch_bytes<serialize_envelope(&next).unwrap().len()/10);
        conn.execute("INSERT INTO app_state VALUES (1,?1)",[serialize_envelope(&before).unwrap()]).unwrap();
        let tx=conn.transaction().unwrap();
        assert_eq!(write_workflow_envelope(&tx,&before,&next).unwrap(),1);
        let actual=crate::read_envelope(&tx).unwrap();
        let expected:Value=serde_json::from_str(&serialize_envelope(&next).unwrap()).unwrap();
        assert_eq!(actual,expected);
        // Immutable history edits and deletions cannot enter the patch path.
        let mut invalid=next.clone();
        invalid["workflow"]["events"][0]["text"]=json!("changed");
        assert!(workflow_envelope_patch(&before,&invalid).is_none());
        invalid=next.clone();
        invalid["workflow"]["shipments"].as_array_mut().unwrap().reverse();
        assert!(workflow_envelope_patch(&before,&invalid).is_none());
        invalid=next.clone();
        invalid["settings"]["future"]=json!(false);
        assert!(workflow_envelope_patch(&before,&invalid).is_none());
        // Compatibility envelopes with stale embedded core fields fall back
        // to the full canonical serializer, rather than retaining them.
        tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",[before.to_string()]).unwrap();
        assert_eq!(write_workflow_envelope(&tx,&before,&next).unwrap(),1);
        assert_eq!(crate::read_envelope(&tx).unwrap(),expected);
        tx.rollback().unwrap();
    }

    #[test]
    fn factory_shared_core_preserves_original_allocations_without_relational_readback(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_schema(key TEXT,version INTEGER);
            INSERT INTO core_schema VALUES ('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let mut before=json!({"revision":7,"products":[{"id":"p","variants":[]}],
            "clients":[{"id":"c"}],"orders":(0..2000).map(|i|
                json!({"id":format!("o-{i}"),"raw":"합성 발주서","recipients":[]}))
                .collect::<Vec<_>>(),"aliases":[],"queue":[],"exports":[],
            "settings":{"accounts":[]},"factoryReplies":[],"workflow":{"events":[]}});
        conn.execute("INSERT INTO app_state VALUES (1,?1)",[serialize_envelope(&before).unwrap()]).unwrap();
        // There are deliberately no core tables. Rehydration must fail.
        let order_ptr=before["orders"].as_array().unwrap().as_ptr();
        let product_ptr=before["products"].as_array().unwrap().as_ptr();
        let client_ptr=before["clients"].as_array().unwrap().as_ptr();
        let mut next:Value=serde_json::from_str(&serialize_envelope(&before).unwrap()).unwrap();
        next["factoryReplies"].as_array_mut().unwrap().push(json!({"id":"new"}));
        let tx=conn.transaction().unwrap();
        assert!(!has_write_triggers(&tx).unwrap());
        assert!(!crate::product_colors::needs_normalization(&before).unwrap());
        move_factory_core(&mut before,&mut next).unwrap();
        assert!(before.get("orders").is_none() && before.get("products").is_none()
            && before.get("clients").is_none());
        let verified=finish_factory_shared_core(&tx,&before,next,7).unwrap();
        assert_eq!(verified["orders"].as_array().unwrap().as_ptr(),order_ptr);
        assert_eq!(verified["products"].as_array().unwrap().as_ptr(),product_ptr);
        assert_eq!(verified["clients"].as_array().unwrap().as_ptr(),client_ptr);
        assert_eq!(verified["revision"],8);
        assert_eq!(verified["factoryReplies"],json!([{"id":"new"}]));
        tx.rollback().unwrap();
        let mut legacy=verified;
        legacy["products"][0]["defaultColor"]=json!("색상 미표기");
        assert!(crate::product_colors::needs_normalization(&legacy).unwrap());
        conn.execute_batch("CREATE TEMP TRIGGER check_factory AFTER UPDATE ON app_state
            BEGIN SELECT 1; END;").unwrap();
        let tx=conn.transaction().unwrap();
        assert!(has_write_triggers(&tx).unwrap());
    }

    #[test]
    fn factory_reply_patch_preserves_history_and_serializes_only_new_record(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        let before=json!({"revision":7,"products":[],"clients":[],"orders":[],
            "settings":{"operatorName":"합성"},
            "factoryReplies":(0..2000).map(|i|json!({"id":format!("old-{i}"),
                "rows":[{"name":"기존 회신","tracking":"000000000001","actual":3000}]}))
                .collect::<Vec<_>>(),
            "workflow":{"shipments":[{"id":"s","charges":{"FACTORY":3000},
                "actualFreight":{},"tracking":{}}],
                "payments":[{"id":"p","amount":13000}],"events":[]}});
        let mut next=before.clone();
        next["revision"]=json!(8);
        next["workflow"]["shipments"][0]["actualFreight"]["FACTORY"]=json!(4000);
        next["workflow"]["shipments"][0]["tracking"]["FACTORY"]=json!("001234567890");
        next["workflow"]["events"].as_array_mut().unwrap().push(json!({"id":"e"}));
        next["factoryReplies"].as_array_mut().unwrap().push(json!({"id":"new","rows":[{"applied":true}]}));
        let patch=workflow_envelope_patch(&before,&next).unwrap();
        assert_eq!(patch.len(),4);
        assert!(patch.iter().any(|(path,_)|path=="$.factoryReplies[#]"));
        assert!(patch.iter().all(|(_,value)|!value.contains("기존 회신")));
        let bytes:usize=patch.iter().map(|(path,value)|path.len()+value.len()).sum();
        let full=serialize_envelope(&next).unwrap();
        assert!(bytes<full.len()/100);
        eprintln!("FACTORY_REPLY_PATCH_BYTES full={} patch={}",full.len(),bytes);
        conn.execute("INSERT INTO app_state VALUES (1,?1)",[serialize_envelope(&before).unwrap()]).unwrap();
        let tx=conn.transaction().unwrap();
        let changes=total_changes(&tx).unwrap();
        let direct=write_workflow_envelope(&tx,&before,&next).unwrap();
        assert_eq!(direct,1);
        assert_eq!(verify_written(&tx,before.clone(),next.clone(),
            WriteScope{direct,verify_envelope:true,..Default::default()},changes).unwrap(),next);
        let mut altered=next.clone();
        altered["factoryReplies"][0]["rows"][0]["actual"]=json!(1);
        assert!(workflow_envelope_patch(&before,&altered).is_none());
        altered=next.clone();
        altered["factoryReplies"].as_array_mut().unwrap().remove(0);
        assert!(workflow_envelope_patch(&before,&altered).is_none());
        altered=next.clone();
        altered["workflow"]["payments"][0]["amount"]=json!(1);
        assert!(workflow_envelope_patch(&before,&altered).is_none());
        altered=next.clone();
        for i in 0..MAX_ENVELOPE_PATCHES{
            altered["factoryReplies"].as_array_mut().unwrap().push(json!({"id":format!("more-{i}")}));
        }
        assert!(workflow_envelope_patch(&before,&altered).is_none());
        // The readback gate rejects silent or unintended envelope differences.
        let mut mismatch=next.clone();
        mismatch["factoryReplies"][0]["id"]=json!("wrong");
        assert!(verify_written(&tx,before,mismatch,
            WriteScope{direct,verify_envelope:true,..Default::default()},changes).is_err());
        tx.rollback().unwrap();
    }

    #[test]
    fn borrowed_envelope_preserves_all_non_core_fields(){
        let state=json!({
            "revision":42,"products":[{"variants":[{"id":"v"}]}],
            "clients":[{"id":"c"}],"orders":[{"recipients":[{"items":[{"id":"i"}]}]}],
            "workflow":{"events":[{"id":"e","detail":"배송"}]},
            "settings":{"vat":10},"future_field":{"nested":[null,true,3,"한글"]}
        });
        let mut expected=state.clone();
        for key in ["products","clients","orders"]{
            expected.as_object_mut().unwrap().remove(key);
        }
        let actual:Value=serde_json::from_str(&serialize_envelope(&state).unwrap()).unwrap();
        assert_eq!(actual,expected);
        assert_eq!(state["orders"][0]["recipients"][0]["items"][0]["id"],"i");
        assert!(serialize_envelope(&Value::Null).is_err());
    }

    #[test]
    fn envelope_only_verification_reuses_core_rows_and_falls_back_on_trigger(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE side_effect(id INTEGER PRIMARY KEY);
            INSERT INTO app_state VALUES (1,'{}');").unwrap();
        let current=json!({"revision":1,"products":[{"id":"p"}],
            "clients":[{"id":"c"}],"orders":[{"id":"o"}]});
        let mut written=current.clone();
        written["revision"]=json!(2);
        written["settings"]=json!({"uiScale":1.2});
        {
            let tx=conn.transaction().unwrap();
            let before=total_changes(&tx).unwrap();
            tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
                [serialize_envelope(&written).unwrap()]).unwrap();
            // There are deliberately no core tables. Successful verification
            // proves they were not queried again for the envelope-only write.
            assert_eq!(verify_written(&tx,current.clone(),written.clone(),WriteScope{direct:1,..Default::default()},before).unwrap(),written);
            tx.rollback().unwrap();
        }
        conn.execute_batch("CREATE TRIGGER unexpected AFTER UPDATE ON app_state
            BEGIN INSERT INTO side_effect VALUES (1); END;").unwrap();
        {
            let tx=conn.transaction().unwrap();
            let before=total_changes(&tx).unwrap();
            tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
                [serialize_envelope(&written).unwrap()]).unwrap();
            // A trigger write must force the original full read, which fails
            // here because core tables are intentionally absent.
            assert!(verify_written(&tx,current,written,WriteScope{direct:1,..Default::default()},before).is_err());
            tx.rollback().unwrap();
        }
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM side_effect",[],
            |row|row.get::<_,i64>(0)).unwrap(),0);
    }

    #[test]
    fn order_only_readback_reuses_catalog_and_unrelated_orders(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);
            CREATE TABLE core_orders(id TEXT PRIMARY KEY,position INTEGER,payload TEXT);
            CREATE TABLE core_recipients(id TEXT PRIMARY KEY,order_id TEXT,position INTEGER,payload TEXT);
            CREATE TABLE core_items(id TEXT PRIMARY KEY,recipient_id TEXT,position INTEGER,payload TEXT);
            INSERT INTO app_state VALUES (1,'{}');").unwrap();
        let current=json!({"revision":1,"products":[{"id":"p"}],"clients":[{"id":"c"}],
            "orders":[{"id":"a","recipients":[{"id":"r","items":[{"id":"i"}],"shippingCharges":{"FACTORY":3000}}]},
                {"id":"b","recipients":[]}]});
        let mut written=current.clone();
        written["revision"]=json!(2);
        written["orders"][0]["recipients"][0]["shippingCharges"]["FACTORY"]=json!(4000);
        conn.execute("INSERT INTO core_orders VALUES ('a',0,?1)",[json!({"id":"a"}).to_string()]).unwrap();
        conn.execute("INSERT INTO core_recipients VALUES ('r','a',0,?1)",
            [json!({"id":"r","shippingCharges":{"FACTORY":3000}}).to_string()]).unwrap();
        conn.execute("INSERT INTO core_items VALUES ('i','r',0,?1)",[json!({"id":"i"}).to_string()]).unwrap();
        // Isolate readback from initial hydration: any accidental full
        // catalog or unrelated-order rehydration must fail in this fixture.
        conn.execute("INSERT INTO core_orders VALUES ('b',1,'not JSON')",[]).unwrap();
        let tx=conn.transaction().unwrap();
        let before=total_changes(&tx).unwrap();
        let direct=tx.execute("UPDATE core_recipients SET payload=?1 WHERE id='r'",
            [json!({"id":"r","shippingCharges":{"FACTORY":4000}}).to_string()]).unwrap()
            +tx.execute("UPDATE app_state SET payload=?1 WHERE id=1",
                [serialize_envelope(&written).unwrap()]).unwrap();
        let scope=WriteScope{orders:true,direct,..Default::default()};
        assert_eq!(verify_written(&tx,current.clone(),written.clone(),scope,before).unwrap(),written);
        assert!(verify_written(&tx,current,written,WriteScope{direct:1,..scope},before).is_err());
        tx.rollback().unwrap();
    }

    #[test]
    fn stale_revision_is_rejected_before_relational_hydration(){
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE core_schema(key TEXT,version INTEGER);
            INSERT INTO core_schema VALUES ('business-core',2);
            CREATE TABLE app_state(id INTEGER PRIMARY KEY,payload TEXT);").unwrap();
        conn.execute("INSERT INTO app_state VALUES (1,?1)",
            [json!({"revision":7}).to_string()]).unwrap();
        // Deliberately omit every relational table: stale requests must
        // fail on revision, not try to hydrate, and never invoke the reducer.
        let error=update_v2_snapshot(&mut conn,6,true,|_|{
            panic!("stale revision must not execute the reducer")
        }).unwrap_err();
        assert!(error.contains("다른 작업에서 데이터가 수정되었습니다"));
        let error=update_v2_snapshot(&mut conn,7,true,|_|{
            panic!("invalid relational snapshot must not execute the reducer")
        }).unwrap_err();
        assert!(!error.contains("다른 작업에서 데이터가 수정되었습니다"));
        let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],
            |row|row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap()["revision"],7);
    }
}
