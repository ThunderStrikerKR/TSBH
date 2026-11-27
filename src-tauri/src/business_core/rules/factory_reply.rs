//! Factory-reply XLSX business row validation, shipment matching and audit.
//! Source oracles: src/factory-reply-workbook.ts and src/factory-replies.ts.
//! ExcelJS remains an XLSX cell reader; this Rust module owns all decisions.
//! Pure computations do not write SQLite. The caller commits a fresh,
//! revision-checked state once through the existing Tauri persistence layer.
use serde_json::{json, Value};
use std::collections::{HashMap,HashSet};
use unicode_normalization::UnicodeNormalization;

fn field<'a>(v:&'a Value, k:&str)->&'a Value { v.get(k).unwrap_or(&Value::Null) }
fn s<'a>(v:&'a Value,k:&str)->&'a str { field(v,k).as_str().unwrap_or("") }
fn rows<'a>(v:&'a Value,k:&str)->&'a [Value] {
    field(v,k).as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn is_duplicate(state:&Value,id:&str)->bool {
    rows(state,"factoryReplies").iter().any(|record|s(record,"id")==id)
}
fn valid_tracking(text:&str)->bool {
    (8..=30).contains(&text.len()) && text.bytes().all(|b|b.is_ascii_digit())
}
fn normalized(text:&str)->String {
    text.nfkc().filter(|c|!c.is_whitespace() && !"(),.-[]".contains(*c))
        .flat_map(char::to_lowercase).collect()
}
fn parse_freight(input:&str)->Option<i64>{
    if input.is_empty(){return None}
    let groups=input.split(',').collect::<Vec<_>>();
    let valid=if groups.len()==1 {
        groups[0].bytes().all(|c|c.is_ascii_digit())
    }else{
        (1..=3).contains(&groups[0].len())
            && groups[0].bytes().all(|c|c.is_ascii_digit())
            && groups.iter().skip(1).all(|g|g.len()==3 && g.bytes().all(|c|c.is_ascii_digit()))
    };
    if !valid {return None}
    input.replace(',',"").parse::<i64>().ok()
        .filter(|n|(0..=9_007_199_254_740_991).contains(n))
}
struct QueueCandidate<'a>{
    day:Option<chrono::NaiveDate>,
    queue:&'a Value,
    tracking:&'a str,
    pending:bool,
}
type QueueMatchIndex<'a> = HashMap<String,Vec<QueueCandidate<'a>>>;

fn match_key(recipient:&Value)->String{
    normalized(s(recipient,"name"))
}
fn business_day(value:&str)->Option<chrono::NaiveDate>{
    chrono::DateTime::parse_from_rfc3339(value).ok()
        .map(|at|at.with_timezone(&chrono::FixedOffset::east_opt(9*3600).unwrap()).date_naive())
        .or_else(||chrono::NaiveDate::parse_from_str(value,"%Y-%m-%d").ok())
}
fn queue_match_index(state:&Value)->QueueMatchIndex<'_>{
    let export_days:HashMap<&str,Option<chrono::NaiveDate>>=rows(state,"exports").iter()
        .map(|e|(s(e,"id"),business_day(s(e,"at")))).collect();
    let ships:HashMap<&str,&Value>=rows(field(state,"workflow"),"shipments").iter()
        .map(|ship|(s(ship,"id"),ship)).collect();
    let mut index=HashMap::new();
    for queue in rows(field(state,"workflow"),"queue").iter().filter(|q|
        s(q,"origin")=="FACTORY" && s(q,"status")=="EXPORTED"
        && !field(q,"snapshot").is_null()){
        let key=match_key(field(field(queue,"snapshot"),"recipient"));
        if key.is_empty(){continue;}
        let day=business_day(s(queue,"exportedAt"))
            .or_else(||export_days.get(s(queue,"exportId")).copied().flatten());
        let ship=ships.get(s(queue,"shipmentId")).copied();
        if ship.is_some_and(|s|field(s,"legacyArchived")==true){continue;}
        let tracking=ship.map(|s|self::s(field(s,"tracking"),"FACTORY")).unwrap_or("");
        let pending=tracking.is_empty()||ship.is_none_or(|s|
            actual(s,field(field(queue,"snapshot"),"recipient")).is_null());
        index.entry(key).or_insert_with(Vec::new).push(QueueCandidate{day,queue,tracking,pending});
    }
    index
}
fn goods_key(value:&str)->Vec<String>{
    let mut parts=value.split('◆').map(normalized).filter(|x|!x.is_empty()).collect::<Vec<_>>();
    parts.sort();
    parts
}
fn queue_goods(queue:&Value)->Vec<String>{
    let recipient=field(field(queue,"snapshot"),"recipient");
    let goods=rows(recipient,"items").iter().filter(|i|s(i,"origin")=="FACTORY")
        .filter_map(|i|field(i,"quantity").as_i64().map(|qty|
            format!("◆ {} {}개",s(i,"name").trim(),qty))).collect::<Vec<_>>().join(" ");
    goods_key(&goods)
}
fn matching_queues<'a>(index:&QueueMatchIndex<'a>,row:&Value)->Vec<&'a Value>{
    let Some(entries)=index.get(&match_key(row)) else{return Vec::new();};
    let cutoff=business_day(s(row,"matchDate"));
    let eligible=entries.iter().filter(|e|e.day.is_none_or(|day|cutoff.is_none_or(|max|day<=max)))
        .collect::<Vec<_>>();
    // An already registered identical Q value is replay/correction evidence,
    // not a second new shipment. Only within the same recipient name.
    let tracking=s(row,"tracking");
    if !tracking.is_empty(){
        let identical=eligible.iter().filter(|e|e.tracking==tracking)
            .map(|e|e.queue).collect::<Vec<_>>();
        if !identical.is_empty(){return identical;}
    }
    // New replies search pending orders from today's export backwards.
    // Completed orders are a fallback only when no pending candidate exists.
    let pending=eligible.iter().any(|e|e.pending);
    let latest=eligible.iter().filter(|e|!pending||e.pending).filter_map(|e|e.day).max();
    let candidates=eligible.iter().filter(|e|(!pending||e.pending)&&e.day==latest)
        .map(|e|e.queue).collect::<Vec<_>>();
    let unique=candidates.iter().map(|q|s(q,"shipmentId")).collect::<HashSet<_>>();
    if unique.len()<=1{return candidates;}
    let wanted=goods_key(s(row,"goods"));
    if wanted.is_empty(){return candidates;}
    let narrowed=candidates.iter().copied().filter(|q|queue_goods(q)==wanted).collect::<Vec<_>>();
    // A missing or changed description cannot pick an unrelated older date.
    if narrowed.is_empty(){candidates}else{narrowed}
}
/// The legacy Tauri write guard must use exactly the same business matcher.
pub(crate) fn factory_reply_matches_shipment(state:&Value,row:&Value,id:&str)->bool{
    let index=queue_match_index(state);
    let ids=matching_queues(&index,row).into_iter()
        .map(|q|s(q,"shipmentId")).collect::<HashSet<_>>();
    ids.len()==1&&ids.contains(id)
}
// Independent linear baseline for the undated, unique-name performance fixture.
#[cfg(test)]
fn row_matches(row:&Value,queue:&Value)->bool{
    match_key(row)==match_key(field(field(queue,"snapshot"),"recipient"))
}
// Shipment positions never change during connect(). Keep the first position
// for an ID, just like the original find(), and all tracking owners.
struct ShipmentIndex<'a>{
    shipments:&'a [Value],
    first:HashMap<&'a str,usize>,
    owners:HashMap<String,HashSet<usize>>,
}
impl<'a> ShipmentIndex<'a>{
    fn new(state:&'a Value)->Self{
        let shipments=rows(field(state,"workflow"),"shipments");
        let mut first=HashMap::new();
        let mut owners:HashMap<String,HashSet<usize>>=HashMap::new();
        for (pos,ship) in shipments.iter().enumerate(){
            first.entry(s(ship,"id")).or_insert(pos);
            let tracking=s(field(ship,"tracking"),"FACTORY");
            if !tracking.is_empty(){owners.entry(tracking.to_owned()).or_default().insert(pos);}
        }
        Self{shipments,first,owners}
    }
    fn position(&self,id:&str)->Option<usize>{self.first.get(id).copied()}
    fn conflicts(&self,id:&str,tracking:&str)->bool{
        self.owners.get(tracking).is_some_and(|positions|
            positions.iter().any(|&pos|s(&self.shipments[pos],"id")!=id))
    }
    fn replace(&mut self,pos:usize,old:&str,new:&str){
        if old==new{return;}
        if let Some(positions)=self.owners.get_mut(old){positions.remove(&pos);}
        if !new.is_empty(){self.owners.entry(new.to_owned()).or_default().insert(pos);}
    }
}
fn actual(ship:&Value,recipient:&Value)->Value {
    match field(ship,"actualFreight").as_object().and_then(|m|m.get("FACTORY")){
        Some(value)=>value.clone(),
        None=>field(recipient,"actualFreight").clone(),
    }
}
fn actor(state:&Value)->String {
    let name=s(field(state,"settings"),"operatorName");
    if name.is_empty(){"공장 회신".into()}else{name.to_string()}
}
fn add_event(state:&mut Value,event:Value)->Result<(),String>{
    state["workflow"]["events"].as_array_mut()
        .ok_or("공장 회신 감사 이력 목록이 없습니다.")?.push(event);
    Ok(())
}
fn audit_id(payload:&Value,key:&str,index:usize)->String {
    rows(payload,key).get(index).and_then(Value::as_str)
        .unwrap_or("preview-only").to_string()
}
fn connect(input:&Value,source:&Value,approve:bool,context:&Value)->Result<Value,String>{
    let reply_id=s(source,"id");
    if is_duplicate(input,reply_id){
        return Ok(input.clone())
    }
    let mut next=input.clone();
    let mut reply=source.clone();
    let count=rows(&reply,"rows").len();
    let mut claimed=HashSet::<String>::new();
    // The reducer only mutates shipments, events and reply history.
    // Borrow the immutable input queue and preserve its original order.
    let queue_index=queue_match_index(input);
    let mut shipment_index=ShipmentIndex::new(input);
    for index in 0..count {
        let current=reply["rows"][index].clone();
        if !s(&current,"issue").is_empty(){continue}
        let valid=!match_key(&current).is_empty()
            && valid_tracking(s(&current,"tracking"))
            && field(&current,"actual").as_i64().is_some_and(|n|
                (0..=9_007_199_254_740_991).contains(&n));
        if !valid{
            reply["rows"][index]["issue"]=json!("필수 배송정보 / Q열 운송장 / 실제 운임 확인 필요");
            continue
        }
        let explicit=if source.get("resolves").is_some()&&count==1 {
            s(context,"selectedShipmentId")
        }else{""};
        let candidates=if approve&&!explicit.is_empty(){
            // A historical correction owns its previously recorded shipment.
            queue_index.get(&match_key(&current)).into_iter().flatten()
                .filter(|e|s(e.queue,"shipmentId")==explicit).map(|e|e.queue).collect()
        }else{matching_queues(&queue_index,&current)};
        let mut ids=Vec::<String>::new();
        for q in &candidates {
            let id=s(q,"shipmentId").to_string();
            if !ids.contains(&id){ids.push(id)}
        }
        // Explicit selection is accepted only from a revalidated single-row
        // correction, never an ordinary import or preview payload.
        if source.get("resolves").is_some() && !approve && count==1 {
            let chosen=s(context,"selectedShipmentId");
            if !chosen.is_empty(){ids.retain(|id|id==chosen);}
        }
        if ids.len()!=1{
            reply["rows"][index]["issue"]=json!(if ids.is_empty(){
                "일치하는 공장 발송 배송 건이 없습니다."
            }else{
                "배송 건이 여러 개입니다. 연결 대상 확인 필요"
            });
            continue
        }
        let shipment_id=ids[0].clone();
        let Some(shipment_pos)=shipment_index.position(&shipment_id) else{
            reply["rows"][index]["issue"]=json!("연결된 배송 건을 찾지 못했습니다.");
            continue;
        };
        let ship=rows(field(&next,"workflow"),"shipments")[shipment_pos].clone();
        reply["rows"][index]["shipmentId"]=json!(shipment_id);
        reply["rows"][index]["orderId"]=field(&ship,"orderId").clone();
        if !claimed.insert(shipment_id.clone()){
            reply["rows"][index]["issue"]=json!("한 파일에 동일 배송 건 중복 행");
            continue
        }
        if shipment_index.conflicts(&shipment_id,s(&current,"tracking")){
            reply["rows"][index]["issue"]=json!("운송장번호가 다른 배송 건에 이미 연결되어 있습니다.");
            if let Some(owner)=shipment_index.shipments.iter().find(|ship|
                s(ship,"id")!=shipment_id&&s(field(ship,"tracking"),"FACTORY")==s(&current,"tracking")){
                reply["rows"][index]["existingShipmentId"]=field(owner,"id").clone();
            }
            continue
        }
        let prev_tracking=s(field(&ship,"tracking"),"FACTORY");
        let wanted=field(&current,"actual");
        // A uniquely matched valid reply updates the two cost-side fields.
        // A differing prior value is audit evidence, not an approval gate.
        // recordActualFreight() only accepts an existing origin of the original
        // order. Keep its independent audit event before the reply event.
        let order=rows(&next,"orders").iter()
            .find(|o|s(o,"id")==s(&ship,"orderId"))
            .ok_or("실제 출고지와 배송 건을 확인하세요.")?;
        let original_recipient=rows(order,"recipients").iter()
            .find(|r|s(r,"id")==s(&ship,"recipientId"))
            .ok_or("실제 출고지와 배송 건을 확인하세요.")?;
        if !rows(original_recipient,"items").iter().any(|i|s(i,"origin")=="FACTORY"){
            return Err("실제 출고지와 배송 건을 확인하세요.".into())
        }
        // Read the current effective cost, never the frozen export note.
        let order_actual=actual(&ship,original_recipient);
        let prev_actual=order_actual.clone();
        // Notification evidence never participates in selecting the shipment
        // or overwriting the recipient. Group all differences into one row.
        let mut changes=Vec::new();
        let old_address=s(original_recipient,"address");
        let new_address=s(&current,"address");
        if !new_address.is_empty()&&normalized(old_address)!=normalized(new_address){
            changes.push(json!({"field":"주소 불일치","before":old_address,"after":new_address}));
        }
        let old_contacts=["phone","mobile"].into_iter().map(|k|s(original_recipient,k))
            .filter(|v|!v.is_empty()).collect::<Vec<_>>();
        let new_contacts=rows(&current,"contacts").iter().filter_map(Value::as_str)
            .filter(|v|!v.is_empty()).collect::<Vec<_>>();
        let contact_keys=|values:&[&str]|values.iter().map(|v|normalized(v))
            .collect::<HashSet<_>>();
        if !new_contacts.is_empty()&&contact_keys(&old_contacts)!=contact_keys(&new_contacts){
            changes.push(json!({"field":"연락처 불일치","before":old_contacts.join(" / "),"after":new_contacts.join(" / ")}));
        }
        if !prev_tracking.is_empty()&&prev_tracking!=s(&current,"tracking"){
            changes.push(json!({"field":"운송장번호 변경","before":prev_tracking,"after":s(&current,"tracking")}));
        }
        if !prev_actual.is_null()&&prev_actual!=*wanted{
            changes.push(json!({"field":"실제 운임 변경","before":grouped_amount(&prev_actual),"after":grouped_amount(wanted)}));
        }
        if !changes.is_empty(){reply["rows"][index]["notificationChanges"]=json!(changes);}
        if !prev_tracking.is_empty()&&prev_tracking==s(&current,"tracking")&&prev_actual==*wanted{
            reply["rows"][index]["applied"]=json!(true);
            continue
        }
        let changed_freight=order_actual!=*wanted;
        if changed_freight{
            let at=s(context,"freightAt");
            let freight_actor=actor(&next);
            add_event(&mut next,json!({
                "id":audit_id(context,"freightEventIds",index),
                "at":at,"actor":freight_actor,"action":"실제 출고 운임 입력",
                "shipmentIds":[shipment_id],"before":order_actual,
                "after":{"origin":"FACTORY","amount":wanted}
            }))?;
        }
        let shipment=next["workflow"]["shipments"].as_array_mut()
            .ok_or("공장 회신 배송 건이 없습니다.")?.get_mut(shipment_pos)
            .ok_or("회신할 배송 건이 없습니다.")?;
        if changed_freight {
            if !field(shipment,"actualFreight").is_object(){
                shipment["actualFreight"]=json!({});
            }
            shipment["actualFreight"]["FACTORY"]=wanted.clone();
        }
        if !field(shipment,"tracking").is_object(){
            shipment["tracking"]=json!({});
        }
        shipment["tracking"]["FACTORY"]=field(&current,"tracking").clone();
        shipment_index.replace(shipment_pos,prev_tracking,s(&current,"tracking"));
        let reply_action=if approve || (!prev_tracking.is_empty() && prev_tracking!=s(&current,"tracking"))
            || (!prev_actual.is_null()&&&prev_actual!=wanted) {
            "공장 송장 / 운임 정정"
        }else{"공장 회신 반영"};
        add_event(&mut next,json!({
            "id":audit_id(context,"replyEventIds",index),
            "at":field(&reply,"at"),
            "actor":if s(&reply,"actor").is_empty(){"공장 회신 Excel"}else{s(&reply,"actor")},
            "action":reply_action,"shipmentIds":[shipment_id],
            "before":{"tracking":field(field(&ship,"tracking"),"FACTORY"),
                      "actual":prev_actual},
            "after":{"tracking":field(&current,"tracking"),"actual":wanted,"fileId":reply_id}
        }))?;
        reply["rows"][index]["applied"]=json!(true);
    }
    let rows=rows(&reply,"rows");
    let applied=rows.iter().filter(|r|field(r,"applied")==true).count();
    let held=rows.len()-applied;
    reply["status"]=json!(if !s(&reply,"error").is_empty() || held>0{
        if applied>0{"PARTIAL"}else{"REVIEW"}
    }else{"APPLIED"});
    if !field(&next,"factoryReplies").is_array(){next["factoryReplies"]=json!([])}
    next["factoryReplies"].as_array_mut().ok_or("공장 회신 이력 오류")?.push(reply);
    // Factory invoices are cost-side evidence, never a customer payment edit.
    // Protect the confirmed customer freight and payment ledger for both
    // automatic imports and explicitly approved single-row corrections.
    ensure_customer_payment_unchanged(input,&next)?;
    Ok(next)
}
pub(crate) fn ensure_customer_payment_unchanged(before:&Value,next:&Value)->Result<(),String>{
    let old=field(before,"workflow");
    let new=field(next,"workflow");
    let old_ships=rows(old,"shipments");
    let new_ships=rows(new,"shipments");
    if field(old,"payments")!=field(new,"payments") ||
        field(old,"confirmations")!=field(new,"confirmations") ||
        old_ships.len()!=new_ships.len() ||
        old_ships.iter().zip(new_ships).any(|(a,b)|
            field(a,"id")!=field(b,"id") ||
            field(a,"charges")!=field(b,"charges") ||
            field(field(a,"actualFreight"),"STORE")!=field(field(b,"actualFreight"),"STORE") ||
            field(field(a,"tracking"),"STORE")!=field(field(b,"tracking"),"STORE")) {
        return Err("공장 회신으로 고객 청구 운임, 입금 기록 또는 매장 출고 기록을 변경할 수 없습니다.".into());
    }
    Ok(())
}
fn preview(state:&Value,reply:&Value)->Result<Value,String>{
    if is_duplicate(state,s(reply,"id")){
        return Err("이미 등록된 동일 회신 파일입니다. 회신 이력에서 확인하세요.".into())
    }
    let next=connect(state,reply,false,&Value::Null)?;
    let matched=rows(&next,"factoryReplies").last()
        .ok_or("공장 회신 미리보기를 생성할 수 없습니다.")?;
    let applied=rows(matched,"rows").iter().filter(|r|field(r,"applied")==true).count();
    let held=rows(matched,"rows").len()-applied;
    Ok(json!({
        "revision":field(state,"revision").as_i64().unwrap_or(0),
        "reply":reply,
        "rows":field(matched,"rows"),
        "applied":applied,"held":held,"status":field(matched,"status")
    }))
}
fn parse_reply(payload:&Value)->Result<Value,String>{
    let count=field(payload,"sourceRowCount").as_i64()
        .ok_or("공장 회신 Excel 행 수가 없습니다.")?;
    if count>1000{return Err("공장 회신은 한 번에 최대 1,000행을 처리합니다.".into())}
    let mut parsed=Vec::new();
    for source in rows(payload,"sourceRows"){
        let n=field(source,"row").as_i64().ok_or("공장 회신 Excel 행 번호 오류")?;
        if n<2 {return Err("공장 회신 Excel 행 번호 오류".into())}
        // Unrelated J and R+ columns are not shipping fields. Their presence
        // must not reject an otherwise readable factory reply workbook.
        let name=s(source,"name");
        let address=s(source,"address");
        let phone=s(source,"phone");
        let mobile=s(source,"mobile");
        let goods=s(source,"goods");
        let raw_freight=s(source,"rawFreight");
        let raw_tracking=s(source,"tracking");
        if [name,address,phone,mobile,goods,raw_freight,raw_tracking].iter().all(|t|t.is_empty())
            && s(source,"sourceIssue").is_empty(){continue}
        let tracking=raw_tracking.chars()
            .filter(|c|*c!='-'&&!c.is_whitespace()).collect::<String>();
        // Excel numeric cells are valid for safe, ordinary integer waybills.
        // More than 15 significant decimal digits may already have been
        // rounded by Excel. Such rows require review, not file-wide rejection.
        let number_format=s(source,"trackingNumberFormat");
        let zero_padded=number_format.len()>=2 &&
            number_format.bytes().all(|b|b==b'0') &&
            number_format.len()>raw_tracking.len();
        let numeric_risk=field(source,"trackingNumeric")==true &&
            (raw_tracking.len()>15 || !raw_tracking.bytes().all(|b|b.is_ascii_digit()) || zero_padded);
        let source_issue=s(source,"sourceIssue");
        let freight=parse_freight(raw_freight);
        let contacts=[phone,mobile].into_iter().filter(|v|!v.is_empty())
            .collect::<Vec<_>>();
        let issue=if !source_issue.is_empty(){
            Some(source_issue)
        }else if numeric_risk{
            Some("Q열 숫자형 운송장번호의 정확성을 보장할 수 없습니다. 15자리 초과·정밀도 또는 앞자리 0 손실 가능성이 있으므로 원본을 확인하세요.")
        }else if normalized(name).is_empty(){
            Some("수하인 이름 누락")
        }else if !valid_tracking(&tracking){
            Some("Q열 운송장번호가 없거나 올바르지 않습니다.")
        }else if freight.is_none(){
            Some("G열 실제 택배운임은 0 이상의 정수로 입력하세요.")
        }else{None};
        let mut row=json!({"row":n,"name":name,"address":address,
            "contacts":contacts,"goods":goods,"matchDate":field(payload,"businessDate"),
            "tracking":tracking,"actual":freight});
        if let Some(issue)=issue{row["issue"]=json!(issue)}
        parsed.push(row);
    }
    if parsed.is_empty(){return Err("회신 파일에 처리할 배송 행이 없습니다.".into())}
    if parsed.iter().all(|r|s(r,"tracking").is_empty()){
        return Err("로젠 회신 양식 불일치: Q열 운송장번호 데이터가 없습니다.".into())
    }
    let mut counts=HashMap::new();
    for row in &parsed {
        let tracking=s(row,"tracking");
        if !tracking.is_empty() && s(row,"issue").is_empty(){
            *counts.entry(tracking.to_string()).or_insert(0usize)+=1;
        }
    }
    for row in &mut parsed {
        let tracking=s(row,"tracking");
        if s(row,"issue").is_empty() && !tracking.is_empty()
            && counts.get(tracking).copied().unwrap_or(0)>1{
            row["issue"]=json!("같은 회신 파일에 운송장번호가 중복됩니다.");
        }
    }
    let reply=json!({
        "id":field(payload,"id"),
        "filename":field(payload,"filename"),
        "businessDate":field(payload,"businessDate"),
        "at":field(payload,"at"),
        "actor":field(payload,"actor"),
        "status":"REVIEW","rows":parsed
    });
    Ok(reply)
}
fn analyze(payload:&Value)->Result<Value,String>{
    preview(field(payload,"state"),&parse_reply(payload)?)
}
/// Automatic inbox only: the caller supplies the authoritative transaction
/// before-image, not an external preview. Parse and match exactly once.
pub(crate) fn import_automatic_factory_reply(state:&Value,payload:&Value)->Result<Value,String>{
    apply_prepared_factory_reply(state,&parse_reply(payload)?,payload)
}
/// Scope hydration by the same name/date/product matcher used by connect.
/// It must include every possible selected shipment, not address/contact matches.
pub(crate) fn prepare_automatic_factory_reply(state:&Value,payload:&Value)
    ->Result<(Value,Vec<String>),String>{
    let reply=parse_reply(payload)?;
    let workflow=field(state,"workflow");
    let mut shipments=HashMap::new();
    for ship in rows(workflow,"shipments"){
        shipments.entry(s(ship,"id")).or_insert(ship);
    }
    let index=queue_match_index(state);
    let mut orders=std::collections::BTreeSet::new();
    for row in rows(&reply,"rows").iter().filter(|row|s(row,"issue").is_empty()){
        for queue in matching_queues(&index,row){
            if let Some(ship)=shipments.get(s(queue,"shipmentId")){
                orders.insert(s(ship,"orderId").to_owned());
            }
        }
    }
    Ok((reply,orders.into_iter().collect()))
}
pub(crate) fn apply_prepared_factory_reply(state:&Value,reply:&Value,payload:&Value)
    ->Result<Value,String>{
    if is_duplicate(state,s(reply,"id")){
        return Err("이미 등록된 동일 회신 파일입니다. 회신 이력에서 확인하세요.".into())
    }
    let next=connect(state,reply,false,payload)?;
    let record=rows(&next,"factoryReplies").last().ok_or("회신 저장 결과 누락")?;
    let applied=rows(record,"rows").iter().filter(|row|field(row,"applied")==true).count();
    let held=rows(record,"rows").len()-applied;
    Ok(json!({"applied":applied,"held":held,"next":next}))
}
fn apply(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("공장 회신 저장 상태 누락")?;
    let reviewed=payload.get("preview").ok_or("공장 회신 미리보기 누락")?;
    if field(state,"revision").as_i64().unwrap_or(0)!=field(reviewed,"revision").as_i64().unwrap_or(0){
        return Err("미리보기 이후 DB가 수정되었습니다. 공장 회신 파일을 다시 검토하세요.".into())
    }
    let reply=field(reviewed,"reply");
    let current=preview(state,reply)?;
    for key in ["rows","applied","held","status"] {
        if field(&current,key)!=field(reviewed,key){
            return Err("공장 회신 연결 결과가 달라졌습니다. 최신 데이터를 다시 검토하세요.".into())
        }
    }
    let next=connect(state,reply,false,payload)?;
    Ok(json!({"next":next,"applied":field(&current,"applied"),"held":field(&current,"held")}))
}
// Preserve the source history. A later successful row with the same name
// and exact waybill resolves an earlier bad G-cell/held row; name alone never does.
fn resolved_row(state:&Value,record:&Value,row:&Value)->bool{
    let records=rows(state,"factoryReplies");
    let Some(position)=records.iter().position(|r|s(r,"id")==s(record,"id")) else{return false;};
    let tracking=s(row,"tracking");
    let name=match_key(row);
    records[position+1..].iter().any(|later|{
        let explicit=s(field(later,"resolves"),"id")==s(record,"id") &&
            field(field(later,"resolves"),"row")==field(row,"row");
        rows(later,"rows").iter().any(|r|field(r,"applied")==true && (explicit ||
            (valid_tracking(tracking)&&!name.is_empty()&&match_key(r)==name&&s(r,"tracking")==tracking
             && (s(row,"shipmentId").is_empty()||s(row,"shipmentId")==s(r,"shipmentId"))
             && (s(row,"goods").is_empty()||s(r,"goods").is_empty()||goods_key(s(row,"goods"))==goods_key(s(r,"goods"))))))
    })
}
fn grouped_amount(value:&Value)->String {
    let Some(n)=value.as_i64() else{return "미확인".into();};
    let digits=n.to_string();
    let mut result=String::new();
    for (index,ch) in digits.chars().enumerate(){
        if index>0 && (digits.len()-index)%3==0{result.push(',');}
        result.push(ch);
    }
    format!("{result}원")
}
fn ambiguous(row:&Value)->bool{
    s(row,"issue").starts_with("배송 건이 여러 개입니다.")
        && field(row,"applied")!=&Value::Bool(true)
}
fn held_row<'a>(state:&'a Value,payload:&Value)->Result<(&'a Value,&'a Value),String>{
    let record=rows(state,"factoryReplies").iter().find(|r|s(r,"id")==s(payload,"id"))
        .ok_or("원본 회신 누락")?;
    let row=rows(record,"rows").iter().find(|r|field(r,"row")==field(payload,"rowNumber"))
        .ok_or("원본 회신 행 누락")?;
    if resolved_row(state,record,row){return Err("이미 정정된 회신 행입니다.".into());}
    Ok((record,row))
}
fn reply_row_with_date(record:&Value,row:&Value)->Value{
    let mut row=row.clone();
    if business_day(s(&row,"matchDate")).is_none(){
        row["matchDate"]=field(record,"businessDate").clone();
    }
    row
}
fn ambiguity_candidates(state:&Value,row:&Value)->Vec<Value>{
    if !ambiguous(row){return Vec::new();}
    let index=queue_match_index(state);
    let ships=ShipmentIndex::new(state);
    let mut seen=HashSet::new();
    let mut result=Vec::new();
    for q in matching_queues(&index,row){
        let id=s(q,"shipmentId");
        if id.is_empty()||!seen.insert(id){continue;}
        let Some(pos)=ships.position(id) else{continue;};
        let ship=ships.shipments[pos].clone();
        let recipient=field(field(q,"snapshot"),"recipient");
        let tracking=s(field(&ship,"tracking"),"FACTORY");
        let old=actual(&ship,recipient);
        // Exported factory queue membership already establishes eligibility.
        // Selecting the target resolves ambiguity, not a second fee approval.
        let eligible=!ships.conflicts(id,s(row,"tracking"))
            && field(&ship,"legacyArchived")!=&Value::Bool(true);
        let export=rows(state,"exports").iter().find(|e|
            !s(q,"exportId").is_empty()&&s(e,"id")==s(q,"exportId"));
        let filename=export.map(|e|s(e,"filename")).unwrap_or("");
        let export_row=export.and_then(|e|rows(e,"outputRows").iter().position(|r|
            s(r,"queueId")==s(q,"id")&&s(r,"shipmentId")==id)).map(|i|i+2);
        let items=rows(recipient,"items").iter().map(|i|
            format!("{} {}개",s(i,"name"),field(i,"quantity").as_i64().unwrap_or(0)))
            .collect::<Vec<_>>().join(" / ");
        result.push(json!({"shipmentId":id,"orderId":field(&ship,"orderId"),
            "exportId":field(q,"exportId"),"filename":filename,"exportRow":export_row,
            "exportAt":export.map(|e|s(e,"at")).unwrap_or_else(||s(q,"at")),
            "items":items,"tracking":tracking,"actual":old,"eligible":eligible}));
    }
    result
}
fn selected_ambiguity(state:&Value,payload:&Value,row:&Value)->Result<Value,String>{
    if !ambiguous(row){return Err("중복 배송 건 검토 대상이 아닙니다.".into());}
    let selected=s(payload,"selectedShipmentId");
    if selected.is_empty(){return Err("연결할 배송 건을 선택하세요.".into());}
    ambiguity_candidates(state,row).into_iter().find(|c|
        s(c,"shipmentId")==selected&&field(c,"eligible")==&Value::Bool(true))
        .ok_or("선택한 배송 건을 연결할 수 없습니다. 최신 출력 이력과 송장을 확인하세요.".into())
}
fn conflict_confirmation(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("공장 회신 상태 누락")?;
    conflict_confirmation_for_state(state,payload)
}

/// Resolve the target using the stored conflict record and shipment only.
/// The matching reducer still sees the complete queue, tracking ownership
/// and history, and must return the very same shipment ID.
pub(crate) fn factory_correction_order_scope(state:&Value,payload:&Value)->Result<Vec<String>,String>{
    let record=rows(state,"factoryReplies").iter().find(|r|s(r,"id")==s(payload,"id"))
        .ok_or("원본 회신 누락")?;
    let source=rows(record,"rows").iter().find(|r|r["row"]==payload["rowNumber"])
        .ok_or("원본 회신 행 누락")?;
    let target=if ambiguous(source){
        let candidate=selected_ambiguity(state,payload,&reply_row_with_date(record,source))?;
        s(&candidate,"shipmentId").to_owned()
    }else{s(source,"shipmentId").to_owned()};
    let ship=rows(field(state,"workflow"),"shipments").iter()
        .find(|ship|s(ship,"id")==target)
        .ok_or("원본 배송 건을 찾지 못했습니다.")?;
    Ok(vec![s(ship,"orderId").to_owned()])
}

fn conflict_confirmation_for_state(state:&Value,payload:&Value)->Result<Value,String>{
    let id=s(payload,"id");
    let row_num=field(payload,"rowNumber").as_i64()
        .ok_or("승인 가능한 회신 충돌 행을 찾지 못했습니다.")?;
    let record=rows(state,"factoryReplies").iter().find(|r|s(r,"id")==id);
    let row=record.and_then(|r|rows(r,"rows").iter().find(|row|
        field(row,"row").as_i64()==Some(row_num)))
        .ok_or("승인 가능한 회신 충돌 행을 찾지 못했습니다.")?;
    if ambiguous(row){
        let (record,row)=held_row(state,payload)?;
        let candidate=selected_ambiguity(state,payload,&reply_row_with_date(record,row))?;
        let message=format!("{}행 / {}\n출력: {} / {}행\n출력일: {}\n품목: {}\n배송 건: {}\n회신: {} / 운임 {}\n선택한 출력 이력을 확인했으며 이 배송 건에만 연결할까요?",
            row_num,s(row,"name"),s(&candidate,"filename"),
            field(&candidate,"exportRow").as_u64().map(|n|n.to_string()).unwrap_or_else(||"미확인".into()),
            s(&candidate,"exportAt"),s(&candidate,"items"),s(&candidate,"shipmentId"),
            s(row,"tracking"),grouped_amount(field(row,"actual")));
        return Ok(json!({"text":message}));
    }
    if s(row,"shipmentId").is_empty()||!s(row,"issue").contains("충돌")||
        field(row,"applied").as_bool()==Some(true) {
        return Err("승인 가능한 회신 충돌 행을 찾지 못했습니다.".into())
    }
    let ship=rows(field(state,"workflow"),"shipments").iter()
        .find(|ship|s(ship,"id")==s(row,"shipmentId"))
        .ok_or("원본 배송 건을 찾지 못했습니다.")?;
    let recipient=rows(state,"orders").iter()
        .find(|order|s(order,"id")==s(ship,"orderId"))
        .and_then(|o|rows(o,"recipients").iter()
            .find(|r|s(r,"id")==s(ship,"recipientId")))
        .ok_or("원본 배송 건을 찾지 못했습니다.")?;
    let old=actual(ship,recipient);
    let current=s(field(ship,"tracking"),"FACTORY");
    let current=if current.is_empty(){"미등록"}else{current};
    let message=format!("{}행 / {}\n기존: {} / 운임 {}\n회신: {} / 운임 {}\n이 한 행만 정정할까요? 기존 값과 원본 파일은 보존됩니다.",
        row_num,s(row,"name"),current,grouped_amount(&old),
        s(row,"tracking"),grouped_amount(field(row,"actual")));
    Ok(json!({"text":message}))
}
// Historical review rows are resolved only by a later, successfully
// applied correction. Source oracle: factory-replies.ts::replyNeedsReview.
fn needs_review(state:&Value,record:&Value)->bool{
    if !s(record,"error").is_empty(){return true;}
    rows(record,"rows").iter().any(|row|
        field(row,"applied").as_bool()!=Some(true) &&
        !resolved_row(state,record,row))
}
fn review_list(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("공장 회신 상태 누락")?;
    let reviews=rows(state,"factoryReplies").iter().map(|record|json!({
        "id":field(record,"id"),"needsReview":needs_review(state,record),
        "resolvedRows":rows(record,"rows").iter().map(|row|json!({
            "row":field(row,"row"),"resolved":resolved_row(state,record,row)
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>();
    Ok(json!({"reviews":reviews}))
}
/// A correction is an append-only record for exactly one unresolved conflict.
/// Recheck the actual confirmation text and immutable shipment match.
fn approve_conflict(payload:&Value)->Result<Value,String>{
    let state=payload.get("state").ok_or("공장 회신 상태 누락")?;
    approve_factory_conflict_for_state(state,payload)
}

pub(crate) fn approve_factory_conflict_for_state(state:&Value,payload:&Value)->Result<Value,String>{
    if state["revision"]!=payload["revision"]{return Err("회신 검토 후 DB가 변경됐습니다.".into())}
    let confirmed=conflict_confirmation_for_state(state,payload)?;
    if confirmed["text"]!=payload["confirmedText"]{return Err("회신 충돌 내용이 변경됐습니다.".into())}
    let original=rows(state,"factoryReplies").iter().find(|r|s(r,"id")==s(payload,"id"))
        .ok_or("원본 회신 누락")?;
    let source=rows(original,"rows").iter().find(|r|r["row"]==payload["rowNumber"])
        .ok_or("원본 회신 행 누락")?;
    if resolved_row(state,original,source){return Err("이미 정정된 회신 행입니다.".into())}
    let correction=s(payload,"correctionId");
    if correction.is_empty()||is_duplicate(state,correction){return Err("회신 정정 ID 오류".into())}
    let selection=ambiguous(source);
    let target=if selection{
        s(&selected_ambiguity(state,payload,&reply_row_with_date(original,source))?,"shipmentId").to_owned()
    }else{s(source,"shipmentId").to_owned()};
    let mut clean=reply_row_with_date(original,source);
    for k in ["issue","applied","shipmentId","orderId"]{
        clean.as_object_mut().ok_or("회신 행 형식 오류")?.remove(k);
    }
    let reply=json!({"id":correction,"filename":original["filename"],
        "businessDate":payload["businessDate"],"at":payload["freightAt"],"actor":actor(state),
        "archivedPath":original["archivedPath"],"resolves":{"id":original["id"],"row":source["row"]},
        "status":"REVIEW","rows":[clean]});
    let mut context=payload.clone();
    context["selectedShipmentId"]=json!(target);
    let next=connect(state,&reply,!selection,&context)?;
    let appended=rows(&next,"factoryReplies").last().ok_or("정정 기록 누락")?;
    let applied=rows(appended,"rows").first().ok_or("정정 결과 누락")?;
    if applied["applied"]!=true||s(applied,"shipmentId")!=target{
        return Err("회신 정정 대상 또는 연결 결과가 변경됐습니다.".into())
    }
    Ok(next)
}

pub(super) fn execute(operation:&str,payload:&Value)->Result<Value,String>{
    match operation {
        "factory_reply_duplicate"=>{
            let state=payload.get("state").ok_or("공장 회신 상태 누락")?;
            let id=payload.get("id").and_then(Value::as_str)
                .ok_or("공장 회신 식별자 누락")?;
            Ok(json!({"duplicate":is_duplicate(state,id)}))
        },
        "factory_reply_review_statuses"=>review_list(payload),
        "factory_reply_candidates"=>{
            let state=payload.get("state").ok_or("공장 회신 상태 누락")?;
            let (record,row)=held_row(state,payload)?;
            Ok(json!({"candidates":ambiguity_candidates(state,&reply_row_with_date(record,row))}))
        },
        "factory_conflict_confirmation"=>conflict_confirmation(payload),
        "approve_factory_conflict"=>approve_conflict(payload),
        "analyze_factory_reply"=>analyze(payload),
        "preview_factory_reply"=>preview(
            payload.get("state").ok_or("공장 회신 상태 누락")?,
            payload.get("reply").ok_or("공장 회신 원본 누락")?
        ),
        "apply_factory_reply"=>apply(payload),
        _=>Err(format!("지원하지 않는 공장 회신 업무: {operation}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factory_payment_boundary_rejects_customer_or_store_mutations(){
        let before=json!({"workflow":{"payments":[{"id":"p","amount":13000}],
            "confirmations":[{"id":"c","amount":13000}],
            "shipments":[{"id":"s","charges":{"FACTORY":3000,"STORE":2500},
                "actualFreight":{"FACTORY":4000,"STORE":2600},"tracking":{"STORE":"1234567890"}}]}});
        let mut legitimate=before.clone();
        legitimate["workflow"]["shipments"][0]["actualFreight"]["FACTORY"]=json!(5500);
        legitimate["workflow"]["shipments"][0]["tracking"]["FACTORY"]=json!("001234567890");
        assert!(ensure_customer_payment_unchanged(&before,&legitimate).is_ok());
        for (pointer,value) in [
            ("/workflow/payments/0/amount",json!(15500)),
            ("/workflow/confirmations/0/amount",json!(15500)),
            ("/workflow/shipments/0/charges/FACTORY",json!(5500)),
            ("/workflow/shipments/0/actualFreight/STORE",json!(5500)),
            ("/workflow/shipments/0/tracking/STORE",json!("9999999999"))
        ]{
            let mut altered=legitimate.clone();
            *altered.pointer_mut(pointer).unwrap()=value;
            assert!(ensure_customer_payment_unchanged(&before,&altered).is_err(),"{pointer}");
        }
    }

    #[test] fn native_duplicate_preflight_and_preview_share_identity() {
        let state=json!({"factoryReplies":[{"id":"factory-reply-a"}]});
        assert_eq!(execute("factory_reply_duplicate",&json!({
            "state":state,"id":"factory-reply-a"
        })).unwrap(),json!({"duplicate":true}));
        assert_eq!(execute("factory_reply_duplicate",&json!({
            "state":state,"id":"factory-reply-b"
        })).unwrap(),json!({"duplicate":false}));
        assert!(execute("factory_reply_duplicate",&json!({"state":state})).is_err());
    }
    fn fixture()->Value{
        let recipient=json!({"id":"r1","name":"김수취","address":"서울 종로구 10",
            "phone":"010-1234-5678","mobile":"","actualFreight":null,
            "items":[{"id":"item","origin":"FACTORY"}]});
        json!({"revision":3,"factoryReplies":[],"settings":{"operatorName":"작업자"},
            "orders":[{"id":"o1","recipients":[recipient.clone()]}],
            "workflow":{"shipments":[{"id":"s1","orderId":"o1","recipientId":"r1",
                "tracking":{},"actualFreight":{}}],
                "queue":[{"shipmentId":"s1","origin":"FACTORY","status":"EXPORTED",
                    "snapshot":{"recipient":recipient}}],
                "events":[]}})
    }
    fn input(state:Value,tracking:&str,freight:&str)->Value{
        json!({"state":state,"sourceRowCount":1,"sourceRows":[{
            "row":2,"name":"김수취","address":"서울 종로구 10",
            "phone":"010-1234-5678","mobile":"",
            "rawFreight":freight,"tracking":tracking,
            "trackingNumeric":false,"reservedJ":false,"extraAfterQ":false
        }],"id":"factory-reply-test","filename":"reply.xlsx",
            "businessDate":"2026-11-13","at":"2026-11-13T10:00:00Z",
            "actor":"작업자"})
    }
    #[test] fn collision_preview_is_read_only_and_recovers_exact_old_values(){
        let mut state=fixture();
        state["workflow"]["shipments"][0]["tracking"]=json!({"FACTORY":"1234567890"});
        state["workflow"]["shipments"][0]["actualFreight"]=json!({"FACTORY":4900});
        state["factoryReplies"]=json!([{"id":"f1","rows":[{
            "row":2,"name":"김수취","shipmentId":"s1","tracking":"9999999999",
            "actual":8500,"issue":"기존 송장 / 실제 운임과 충돌합니다. 직원 확인 필요",
            "applied":false}]}]);
        let frozen=state.clone();
        let preview=conflict_confirmation(&json!({
            "state":state,"id":"f1","rowNumber":2})).unwrap();
        assert_eq!(preview["text"],"2행 / 김수취\n기존: 1234567890 / 운임 4,900원\n회신: 9999999999 / 운임 8,500원\n이 한 행만 정정할까요? 기존 값과 원본 파일은 보존됩니다.");
        assert_eq!(frozen["workflow"]["shipments"][0]["actualFreight"]["FACTORY"],4900);
        let denied=conflict_confirmation(&json!({
            "state":frozen,"id":"f1","rowNumber":99}));
        assert!(denied.is_err());
    }
    #[test]
    fn automatic_inbox_matches_reviewed_reducer_with_identical_audit_and_holds(){
        for (tracking,freight,old_tracking) in [
            ("001234567890","4000",""),
            ("001234567890","4000","9999999999"),
            ("001234567890","invalid",""),
            ("001234567890","0","")
        ]{
            let mut state=fixture();
            if !old_tracking.is_empty(){
                state["workflow"]["shipments"][0]["tracking"]["FACTORY"]=json!(old_tracking);
            }
            let mut payload=input(state.clone(),tracking,freight);
            payload["freightAt"]=json!("2026-11-13T10:00:00Z");
            payload["freightEventIds"]=json!(["f1"]);
            payload["replyEventIds"]=json!(["r1"]);
            let reviewed=analyze(&payload).unwrap();
            payload["preview"]=reviewed;
            let old=apply(&payload).unwrap();
            let new=import_automatic_factory_reply(&state,&payload).unwrap();
            assert_eq!(old,new);
            assert!(import_automatic_factory_reply(&new["next"],&payload).is_err());
        }
    }
    #[test]
    fn automatic_freight_correction_is_audited_without_changing_payment(){
        let mut state=fixture();
        state["workflow"]["shipments"][0]["tracking"]["FACTORY"]=json!("001234567890");
        state["workflow"]["shipments"][0]["actualFreight"]["FACTORY"]=json!(4000);
        state["workflow"]["shipments"][0]["charges"]=json!({"FACTORY":3000});
        state["workflow"]["payments"]=json!([{"id":"p1","amount":13000}]);
        state["workflow"]["confirmations"]=json!([{"id":"c1","amount":13000}]);
        let payload=input(state.clone(),"001234567890","5500");
        let held=import_automatic_factory_reply(&state,&payload).unwrap();
        assert_eq!(held["held"],0);
        assert_eq!(held["applied"],1);
        let next=held["next"].clone();
        let ship=&next["workflow"]["shipments"][0];
        assert_eq!(ship["tracking"]["FACTORY"],"001234567890");
        assert_eq!(ship["actualFreight"]["FACTORY"],5500);
        assert_eq!(ship["charges"]["FACTORY"],3000);
        for key in ["payments","confirmations","queue"]{
            assert_eq!(next["workflow"][key],state["workflow"][key],"{key}");
        }
        let events=rows(&next["workflow"],"events");
        assert_eq!(events.len(),2);
        assert_eq!(events[0]["before"],4000);
        assert_eq!(events[0]["after"]["amount"],5500);
        assert_eq!(events[1]["action"],"공장 송장 / 운임 정정");
        assert_eq!(next["factoryReplies"].as_array().unwrap().len(),1);
        assert_eq!(next["factoryReplies"][0]["status"],"APPLIED");
    }

    fn legacy_matching_queues<'a>(state:&'a Value,row:&Value)->Vec<&'a Value>{
        rows(field(state,"workflow"),"queue").iter()
            .filter(|q|s(q,"origin")=="FACTORY" && s(q,"status")=="EXPORTED"
                && !field(q,"snapshot").is_null() && row_matches(row,q))
            .collect()
    }

    #[test]
    fn indexed_queue_matching_ignores_address_and_contact_and_preserves_candidate_order(){
        let mut state=fixture();
        let original=state["workflow"]["queue"][0].clone();
        let mut same=original.clone();
        same["shipmentId"]=json!("s2");
        same["snapshot"]["recipient"]["name"]=json!("김 수취");
        same["snapshot"]["recipient"]["address"]=json!("서울, 종로구 10");
        same["snapshot"]["recipient"]["phone"]=json!("");
        same["snapshot"]["recipient"]["mobile"]=json!("01012345678");
        let mut wrong_contact=original.clone();
        wrong_contact["shipmentId"]=json!("s3");
        wrong_contact["snapshot"]["recipient"]["phone"]=json!("010-9999-9999");
        let wrong_contact_copy=wrong_contact.clone();
        let mut store=original.clone();
        store["origin"]=json!("STORE");
        let mut pending=original.clone();
        pending["status"]=json!("PENDING");
        let mut missing=original.clone();
        missing["snapshot"]=Value::Null;
        state["workflow"]["queue"]=json!([original.clone(),wrong_contact,same.clone(),
            store,original.clone(),pending,missing]);
        let index=queue_match_index(&state);
        for row in [
            json!({"name":"김 수취","address":"서울 종로구 10","contacts":["010-1234-5678"]}),
            json!({"name":"김수취","address":"서울 종로구 10","contacts":["010-9999-9999"]}),
            json!({"name":"김수취","address":"서울 종로구 10","contacts":["no match"]}),
            json!({"name":"김수취","address":"다른 주소","contacts":["010-1234-5678"]}),
            json!({"name":"김수취","address":"서울 종로구 10","contacts":[]})
        ]{
            assert_eq!(matching_queues(&index,&row),legacy_matching_queues(&state,&row));
        }
        let row=json!({"name":"김수취","address":"서울 종로구 10","contacts":["01012345678"]});
        assert_eq!(matching_queues(&index,&row),vec![&original,&wrong_contact_copy,&same,&original]);
    }

    #[test]
    fn indexed_factory_queue_large_synthetic_benchmark(){
        const QUEUE:usize=10_000;
        const REPLIES:usize=100;
        let queue=(0..QUEUE).map(|i|json!({
            "origin":"FACTORY","status":"EXPORTED","shipmentId":format!("s{i}"),
            "snapshot":{"recipient":{"name":format!("합성 수취인 {i}"),
                "address":format!("서울 종로구 {i}"),"phone":"010-1234-5678"}}
        })).collect::<Vec<_>>();
        let state=json!({"workflow":{"queue":queue}});
        let replies=(0..REPLIES).map(|i|json!({
            "name":format!("합성수취인{}",i*100),
            "address":format!("서울 종로구 {}",i*100),"contacts":["01012345678"]
        })).collect::<Vec<_>>();
        let start=std::time::Instant::now();
        let expected=replies.iter().map(|r|legacy_matching_queues(&state,r)).collect::<Vec<_>>();
        let legacy=start.elapsed();
        let start=std::time::Instant::now();
        let index=queue_match_index(&state);
        let actual=replies.iter().map(|r|matching_queues(&index,r)).collect::<Vec<_>>();
        let indexed=start.elapsed();
        assert_eq!(actual,expected);
        assert!(actual.iter().all(|c|c.len()==1));
        let candidate_visits=replies.iter().map(|r|index.get(&match_key(r))
            .map_or(0,Vec::len)).sum::<usize>();
        assert_eq!(candidate_visits,REPLIES);
        // Deterministic work bound, not a flaky wall-clock threshold.
        eprintln!("factory queue benchmark: {QUEUE} queue / {REPLIES} rows; legacy={legacy:?}, indexed including build={indexed:?}; legacy visits={}, indexed build + candidate visits={}",QUEUE*REPLIES,QUEUE+candidate_visits);
    }

    #[test]
    fn indexed_tracking_ownership_matches_linear_scan_after_each_change(){
        let initial=json!({"workflow":{"shipments":[
            {"id":"s1","tracking":{"FACTORY":"001234567890"}},
            {"id":"s2","tracking":{"FACTORY":"009876543210"}},
            {"id":"s2","tracking":{"FACTORY":"009876543210"}},
            {"id":"s3","tracking":{"STORE":"001234567890"}},
            {"id":"s4","tracking":{"FACTORY":"001234567890"}}
        ]}});
        let mut current=initial.clone();
        let mut index=ShipmentIndex::new(&initial);
        assert_eq!(index.position("s2"),Some(1));
        assert_eq!(index.position("missing"),None);
        for change in [None,Some((0,"1234567890")),Some((1,"1111111111")),
            Some((4,"2222222222")),Some((3,"1234567890"))]{
            if let Some((pos,new))=change{
                let ship=&mut current["workflow"]["shipments"][pos];
                index.replace(pos,s(field(ship,"tracking"),"FACTORY"),new);
                ship["tracking"]["FACTORY"]=json!(new);
            }
            for id in ["s1","s2","s3","s4","missing"]{
                for tracking in ["001234567890","009876543210","1234567890","1111111111","2222222222","9999999999"]{
                    let expected=rows(field(&current,"workflow"),"shipments").iter().any(|v|
                        s(v,"id")!=id && s(field(v,"tracking"),"FACTORY")==tracking);
                    assert_eq!(index.conflicts(id,tracking),expected,"{id} / {tracking}");
                }
            }
        }
    }

    #[test]
    fn indexed_tracking_large_synthetic_benchmark(){
        const SHIPMENTS:usize=10_000;
        const QUERIES:usize=100;
        let initial=json!({"workflow":{"shipments":(0..SHIPMENTS).map(|i|json!({
            "id":format!("s{i}"),"tracking":{"FACTORY":format!("{:012}",i+1)}
        })).collect::<Vec<_>>()}});
        let queries=(0..QUERIES).map(|i|(format!("s{}",i*100),
            format!("{:012}",i*100+1))).collect::<Vec<_>>();
        let start=std::time::Instant::now();
        let expected=queries.iter().map(|(id,tracking)|
            rows(field(&initial,"workflow"),"shipments").iter().any(|v|
                s(v,"id")!=id.as_str() && s(field(v,"tracking"),"FACTORY")==tracking.as_str()))
            .collect::<Vec<_>>();
        let legacy=start.elapsed();
        let start=std::time::Instant::now();
        let index=ShipmentIndex::new(&initial);
        let actual=queries.iter().map(|(id,tracking)|index.conflicts(id,tracking)).collect::<Vec<_>>();
        let indexed=start.elapsed();
        assert_eq!(actual,expected);
        assert!(actual.iter().all(|v|!*v));
        eprintln!("factory tracking benchmark: {SHIPMENTS} shipments / {QUERIES} queries; legacy={legacy:?}, indexed including build={indexed:?}");
    }

    #[test] fn valid_reply_matches_and_audits(){
        let s=fixture();
        let preview=analyze(&input(s.clone(),"001234567890","4,000")).unwrap();
        assert_eq!(preview["applied"],1);
        let result=apply(&json!({"state":s,"preview":preview,"freightAt":"2026-11-13T10:00:00Z",
            "freightEventIds":["f1"],"replyEventIds":["r1"]})).unwrap();
        assert_eq!(result["next"]["workflow"]["shipments"][0]["tracking"]["FACTORY"],
            "001234567890");
        assert_eq!(result["next"]["workflow"]["shipments"][0]["actualFreight"]["FACTORY"],4000);
        assert_eq!(result["next"]["workflow"]["events"].as_array().unwrap().len(),2);
    }
    #[test] fn mismatched_address_is_ignored_but_duplicate_number_never_applies(){
        let s=fixture();
        let mut req=input(s.clone(),"001234567890","4000");
        req["sourceRows"][0]["address"]=json!("서울 다른 구 22");
        assert_eq!(analyze(&req).unwrap()["applied"],1);
        let mut duplicate=input(s,"001234567890","4000");
        let another=duplicate["sourceRows"][0].clone();
        duplicate["sourceRows"].as_array_mut().unwrap().push(another);
        duplicate["sourceRowCount"]=json!(2);
        assert_eq!(analyze(&duplicate).unwrap()["held"],2);
    }
    #[test] fn numeric_tracking_leading_zero_format_is_reviewed_in_native_rust(){
        let mut req=input(fixture(),"123456789012","4000");
        req["sourceRows"][0]["trackingNumeric"]=json!(true);
        req["sourceRows"][0]["trackingNumberFormat"]=json!("0000000000000");
        let result=analyze(&req).unwrap();
        assert_eq!(result["held"],1);
        assert!(result["rows"][0]["issue"].as_str().unwrap().contains("앞자리 0"));
        req["sourceRows"][0]["trackingNumberFormat"]=json!("General");
        let accepted=analyze(&req).unwrap();
        assert_eq!(accepted["applied"],1);
    }
    #[test] fn corrected_waybill_and_safe_numeric_waybills_are_accepted(){
        let mut s=fixture();
        s["workflow"]["shipments"][0]["tracking"]["FACTORY"]=json!("009988776655");
        assert_eq!(analyze(&input(s,"001234567890","4000")).unwrap()["applied"],1);
        let mut req=input(fixture(),"123456789012","4000");
        req["sourceRows"][0]["trackingNumeric"]=json!(true);
        let accepted=analyze(&req).unwrap();
        assert_eq!(accepted["applied"],1);
        assert_eq!(accepted["held"],0);
        req["sourceRows"][0]["tracking"]=json!("1234567890123456");
        let unsafe_numeric=analyze(&req).unwrap();
        assert_eq!(unsafe_numeric["applied"],0);
        assert_eq!(unsafe_numeric["held"],1);
        assert!(unsafe_numeric["rows"][0]["issue"].as_str().unwrap().contains("15자리"));
    }
    #[test] fn unrelated_extra_columns_and_bad_rows_do_not_reject_good_rows(){
        let mut input=input(fixture(),"123456789012","4000");
        input["sourceRows"][0]["reservedJ"]=json!(true);
        input["sourceRows"][0]["extraAfterQ"]=json!(true);
        assert_eq!(analyze(&input).unwrap()["applied"],1);
        let mut second=input["sourceRows"][0].clone();
        second["row"]=json!(3);
        second["sourceIssue"]=json!("3행 Q열 수식 사용: 송장번호 확인 필요");
        input["sourceRows"].as_array_mut().unwrap().push(second);
        input["sourceRowCount"]=json!(2);
        let preview=analyze(&input).unwrap();
        assert_eq!(preview["applied"],1);
        assert_eq!(preview["held"],1);
        assert!(preview["rows"][1]["issue"].as_str().unwrap().contains("수식"));
    }
    #[test] fn stale_or_tampered_preview_cannot_commit(){
        let s=fixture();
        let preview=analyze(&input(s.clone(),"001234567890","4000")).unwrap();
        let mut altered=preview.clone();
        altered["rows"][0]["tracking"]=json!("009999999999");
        assert!(apply(&json!({"state":s.clone(),"preview":altered})).unwrap_err().contains("연결 결과"));
        let mut current=s;
        current["revision"]=json!(4);
        assert!(apply(&json!({"state":current,"preview":preview})).unwrap_err().contains("DB"));
    }

    #[test]
    fn operational_reply_matches_normalized_name_independent_of_address_and_contact(){
        let mut state=fixture();
        state["orders"][0]["recipients"][0]["address"]=json!("서울 다른 주소 20");
        let mut request=input(state.clone(),"001234567890","4000");
        request["sourceRows"][0]["name"]=json!("김 수취");
        request["sourceRows"][0]["address"]=json!("서울 종로구 １０");
        request["sourceRows"][0]["phone"]=json!("010 (1234)-5678");
        assert_eq!(analyze(&request).unwrap()["applied"],1);
        request["sourceRows"][0]["address"]=json!("서울 다른 주소 20");
        assert_eq!(analyze(&request).unwrap()["applied"],1);
        request["sourceRows"][0]["address"]=json!("서울 종로구 10");
        request["sourceRows"][0]["phone"]=json!("010-0000-0000");
        assert_eq!(analyze(&request).unwrap()["applied"],1);
        request["sourceRows"][0]["address"]=json!("");
        request["sourceRows"][0]["phone"]=json!("");
        assert_eq!(analyze(&request).unwrap()["applied"],1);
        request["sourceRows"][0]["name"]=json!("다른 수하인");
        assert_eq!(analyze(&request).unwrap()["held"],1);
    }
    #[test]
    fn operational_reply_only_matches_exported_factory_and_unique_shipment(){
        let state=fixture();
        for (key,value) in [("origin","STORE"),("status","QUEUED")]{
            let mut changed=state.clone();
            changed["workflow"]["queue"][0][key]=json!(value);
            assert_eq!(analyze(&input(changed,"001234567890","4000")).unwrap()["held"],1);
        }
        let mut repeated=state.clone();
        let second=repeated["workflow"]["queue"][0].clone();
        repeated["workflow"]["queue"].as_array_mut().unwrap().push(second);
        assert_eq!(analyze(&input(repeated.clone(),"001234567890","4000")).unwrap()["applied"],1);
        repeated["workflow"]["queue"][1]["shipmentId"]=json!("s2");
        assert_eq!(analyze(&input(repeated,"001234567890","4000")).unwrap()["held"],1);
    }
    #[test]
    fn operational_reply_freight_and_waybill_boundaries(){
        for (freight,accepted) in [("0",true),("1",true),("4,000",true),
            ("9,007,199,254,740,991",true),("9007199254740992",false),
            ("-1",false),("0.5",false),("4,00",false)]{
            let p=analyze(&input(fixture(),"001234567890",freight)).unwrap();
            assert_eq!(p["applied"],if accepted{1}else{0},"freight={freight}");
        }
        for (tracking,accepted) in [("00000000",true),("1234567",false),
            ("123-45678",true),("１２３４５６７８",false)]{
            let p=analyze(&input(fixture(),tracking,"4000")).unwrap();
            assert_eq!(p["applied"],if accepted{1}else{0},"tracking={tracking}");
        }
        assert_eq!(analyze(&input(fixture(),&"0".repeat(30),"4000")).unwrap()["applied"],1);
        assert_eq!(analyze(&input(fixture(),&"1".repeat(31),"4000")).unwrap()["held"],1);
    }
    #[test]
    fn operational_reply_repeat_and_duplicate_tracking_do_not_rewrite_history(){
        let mut state=fixture();
        state["workflow"]["shipments"][0]["tracking"]["FACTORY"]=json!("001234567890");
        state["workflow"]["shipments"][0]["actualFreight"]["FACTORY"]=json!(4000);
        let p=analyze(&input(state.clone(),"001234567890","4000")).unwrap();
        let r=apply(&json!({"state":state,"preview":p})).unwrap();
        assert_eq!(r["applied"],1);
        assert_eq!(r["next"]["workflow"]["events"],json!([]));
        assert_eq!(r["next"]["workflow"]["shipments"][0]["actualFreight"]["FACTORY"],4000);
        assert!(analyze(&input(r["next"].clone(),"001234567890","4000")).unwrap_err().contains("동일 회신"));
        let mut another=fixture();
        another["workflow"]["shipments"].as_array_mut().unwrap().push(json!({
            "id":"s2","tracking":{"FACTORY":"001234567890"},"actualFreight":{}
        }));
        let p=analyze(&input(another,"001234567890","4000")).unwrap();
        assert_eq!(p["held"],1);
        assert!(p["rows"][0]["issue"].as_str().unwrap().contains("다른 배송 건"));
    }
}

#[cfg(test)]
mod ambiguity_selection_tests{
    use super::*;
    #[test]
    fn explicit_ambiguity_resolution_preserves_other_order_and_rejects_wrong_target(){
        let recipient=json!({"id":"r","name":"가상","address":"서울 테스트로 1",
            "phone":"010-0000-0000","items":[{"origin":"FACTORY","name":"가상 상품","quantity":1}]});
        let mut state=json!({"revision":3,"orders":[
            {"id":"o1","recipients":[recipient.clone()]},{"id":"o2","recipients":[recipient.clone()]}],
            "workflow":{"shipments":[
                {"id":"s1","orderId":"o1","recipientId":"r","step":4,"transferred":true,
                    "tracking":{"FACTORY":"999999999999"},"actualFreight":{}},
                {"id":"s2","orderId":"o2","recipientId":"r","step":4,"transferred":true,
                    "tracking":{},"actualFreight":{}}],
                "queue":[{"id":"q1","shipmentId":"s1","origin":"FACTORY","status":"EXPORTED","snapshot":{"recipient":recipient.clone()}},
                    {"id":"q2","shipmentId":"s2","origin":"FACTORY","status":"EXPORTED","snapshot":{"recipient":recipient}}],
                "events":[],"payments":[],"confirmations":[]}});
        let request=json!({"sourceRowCount":1,"sourceRows":[{"row":2,"name":"가상",
            "address":"서울 테스트로 1","phone":"010-0000-0000","rawFreight":"12340",
            "tracking":"000099990001"}],"id":"reply","filename":"reply.xlsx",
            "businessDate":"2026-11-27","at":"2026-11-27T00:00:00Z"});
        state=import_automatic_factory_reply(&state,&request).unwrap()["next"].clone();
        assert_ne!(state["factoryReplies"][0]["rows"][0]["applied"],true);
        let mut review=json!({"id":"reply","rowNumber":2,"revision":3,
            "selectedShipmentId":"s2","correctionId":"correction","businessDate":"2026-11-27",
            "freightAt":"2026-11-27T00:00:00Z","freightEventIds":["f"],"replyEventIds":["r"]});
        let row=&state["factoryReplies"][0]["rows"][0];
        let candidates=ambiguity_candidates(&state,row);
        assert_eq!(candidates.len(),2);
        assert_eq!(candidates[0]["eligible"],true);
        assert_eq!(candidates[1]["eligible"],true);
        review["confirmedText"]=conflict_confirmation_for_state(&state,&review).unwrap()["text"].clone();
        let next=approve_factory_conflict_for_state(&state,&review).unwrap();
        assert_eq!(next["workflow"]["shipments"][0],state["workflow"]["shipments"][0]);
        assert_eq!(next["workflow"]["shipments"][1]["tracking"]["FACTORY"],"000099990001");
        assert_eq!(next["workflow"]["shipments"][1]["actualFreight"]["FACTORY"],12340);
        assert_eq!(next["factoryReplies"][0],state["factoryReplies"][0]);
        assert!(resolved_row(&next,&next["factoryReplies"][0],&next["factoryReplies"][0]["rows"][0]));
        for target in ["s1","unknown"]{
            review["selectedShipmentId"]=json!(target);
            assert!(approve_factory_conflict_for_state(&state,&review).is_err());
        }
        review["selectedShipmentId"]=json!("s2");
        assert!(approve_factory_conflict_for_state(&next,&review).is_err());
    }
}

#[cfg(test)]
mod date_name_matching_tests{
    use super::*;
    #[test]
    fn newest_korea_export_day_first_and_goods_only_for_same_day_duplicates(){
        fn queue(id:&str,at:&str,goods:&str)->Value{
            json!({"shipmentId":id,"origin":"FACTORY","status":"EXPORTED","exportedAt":at,
                "snapshot":{"recipient":{"name":"가상 수하인","items":[
                    {"origin":"FACTORY","name":goods,"quantity":1}]}}})
        }
        let state=json!({"workflow":{"queue":[
            queue("old","2026-11-25T04:00:00Z","BD 709"),
            queue("today","2026-11-26T15:01:00Z","BD 709"),
            queue("other","2026-11-27T03:00:00Z","BD 710"),
            queue("future","2026-11-28T03:00:00Z","BD 709")
        ]}});
        let index=queue_match_index(&state);
        let mut row=json!({"name":"가상수하인","matchDate":"2026-11-27","goods":"◆ BD 709 1개"});
        let ids=|row:&Value|matching_queues(&index,row).into_iter().map(|q|s(q,"shipmentId")).collect::<Vec<_>>();
        assert_eq!(ids(&row),vec!["today"]);
        row["goods"]=json!("◆ BD 710 1개");
        assert_eq!(ids(&row),vec!["other"]);
        row["goods"]=json!("다른 품목");
        assert_eq!(ids(&row),vec!["today","other"]);
        row["matchDate"]=json!("2026-11-26");
        assert_eq!(ids(&row),vec!["old"]);
        row["matchDate"]=json!("2026-11-24");
        assert!(ids(&row).is_empty());
    }
}

#[cfg(test)]
mod pending_reply_policy_tests{
    use super::*;
    #[test]
    fn pending_orders_take_precedence_but_existing_waybill_replay_keeps_its_owner(){
        let q=|id:&str,date:&str|json!({"shipmentId":id,"origin":"FACTORY","status":"EXPORTED","exportedAt":date,
            "snapshot":{"recipient":{"name":"김수하","items":[{"origin":"FACTORY","name":"BD 709","quantity":1}]}}});
        let state=json!({"workflow":{"queue":[q("today","2026-11-27"),q("yesterday","2026-11-26"),q("future","2026-11-28")],
            "shipments":[{"id":"today","tracking":{"FACTORY":"11111111"},"actualFreight":{"FACTORY":3000}},
                {"id":"yesterday","tracking":{},"actualFreight":{}},{"id":"future","tracking":{},"actualFreight":{}}]}});
        let index=queue_match_index(&state);
        let mut row=json!({"name":"김수하","matchDate":"2026-11-27","tracking":"22222222"});
        assert_eq!(s(matching_queues(&index,&row)[0],"shipmentId"),"yesterday");
        row["tracking"]=json!("11111111");
        assert_eq!(s(matching_queues(&index,&row)[0],"shipmentId"),"today");
    }
    #[test]
    fn later_valid_same_waybill_resolves_invalid_g_without_erasing_history(){
        let old=json!({"id":"old","rows":[{"row":2,"name":"김수하","tracking":"001234567890","issue":"G열 오류"}]});
        let mut state=json!({"factoryReplies":[old.clone(),{"id":"new","rows":[{"row":8,"name":"김수하","tracking":"001234567890","shipmentId":"s1","applied":true}]}]});
        assert!(resolved_row(&state,&old,&old["rows"][0]));
        state["factoryReplies"][1]["rows"][0]["tracking"]=json!("009999999999");
        assert!(!resolved_row(&state,&old,&old["rows"][0]));
        assert_eq!(state["factoryReplies"][0],old);
    }
}
