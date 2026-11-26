//! Reconciliation port from src/workflow.ts. Mutations are pure JSON.
//! IDs and timestamps for newly emitted events are caller-supplied.
use serde_json::{json,Value};
use super::{field, find_by_id, flag, has_value, req, required_str, rows, set, str_field};
use super::{shipping,workflow_stages as stages};

fn workflow_empty() -> Value {
    json!({"sources":[],"groups":[],"shipments":[],"payments":[],
        "queue":[],"events":[],"deliveries":[],"changeRequests":[]})
}
struct Audit<'a> {
    payload:&'a Value,
    position:usize,
    scoped_seed:Option<&'a str>,
}
impl Audit<'_> {
    fn emit(&mut self,state:&mut Value,action:&str,shipment_ids:&[String],
        group_id:&str,before:Option<Value>,after:Option<Value>,reason:Option<&str>,
        actor:&str) -> Result<(),String> {
        // The legacy full-reconcile wire contract still requires explicit
        // eventIds. Scoped employee edits allocate only the IDs they emit,
        // using a caller-provided unique seed, as other native audits do.
        let generated;
        let id=if let Some(seed)=self.scoped_seed{
            generated=format!("{seed}:reconcile:{}",self.position);
            generated.as_str()
        }else{
            rows(field(self.payload,"eventIds")).get(self.position)
                .and_then(Value::as_str).filter(|v|!v.trim().is_empty())
                .ok_or_else(||format!("재조정 이벤트 ID 누락 (index {})",self.position))?
        };
        self.position+=1;
        if find_by_id(field(field(state,"workflow"),"events"),id).is_some() {
            return Err("업무 이벤트 ID가 중복됩니다.".into());
        }
        let mut event=json!({"id":id,"at":required_str(self.payload,"now")?,
            "actor":actor,"action":action,"shipmentIds":shipment_ids,"groupId":group_id});
        if let Some(v)=before {set(&mut event,"before",v)?;}
        if let Some(v)=after {set(&mut event,"after",v)?;}
        if let Some(v)=reason {set(&mut event,"reason",json!(v))?;}
        state["workflow"]["events"].as_array_mut()
            .ok_or("업무 이벤트 목록 누락")?.push(event);
        Ok(())
    }
}
fn party<'a>(state:&'a Value,order:&'a Value)->Option<&'a Value> {
    if (has_value(field(order,"confirmedAt")) || flag(order,"pricesLocked") ||
        str_field(order,"orderType")=="RETAIL") &&
        has_value(field(order,"clientSnapshot")) {
        return Some(field(order,"clientSnapshot"));
    }
    find_by_id(field(state,"clients"),str_field(order,"clientId"))
        .or_else(||order.get("clientSnapshot"))
}
pub(super) fn company_client(state:&Value)->Value{
    let settings=field(state,"settings");
    let name=if str_field(settings,"senderName").trim().is_empty() {"반도스탠드"}
        else{str_field(settings,"senderName").trim()};
    json!({"postcode":str_field(settings,"senderPostcode"),"id":"",
        "name":name,
        "address":str_field(settings,"senderAddress"),
        "phone":str_field(settings,"senderPhone"),
        "mobile":str_field(settings,"senderMobile"),
        "contact":"","tier":"retail","payment":"POSTPAID",
        "shippingFeeExcluded":false,"active":true,"identifiers":"","overrides":{}})
}
// src/explicit-sender.ts: recognize only a line that begins with the
// sender marker (after optional whitespace), followed by a colon.
fn explicit_sender_name(line:&str)->Option<String>{
    let line=line.trim_start();
    let rest=if let Some(after)=line.strip_prefix("보내는"){
        after.trim_start().strip_prefix("분")
            .or_else(||after.trim_start().strip_prefix("사람"))
    }else{
        line.strip_prefix("송하인명").or_else(||line.strip_prefix("송하인"))
    }?;
    let rest=rest.trim_start();
    let content=rest.strip_prefix(':').or_else(||rest.strip_prefix('：'))?;
    Some(content.trim().to_string())
}
// Match the first complete address label following the explicit sender line.
// Preserve the full remainder, not only a guessed first address line.
fn explicit_sender_address(text:&str)->String{
    for (idx,_) in text.char_indices(){
        let suffix=&text[idx..];
        for label in ["보내는곳","송하인주소","주소"]{
            let Some(after_label)=suffix.strip_prefix(label) else{continue};
            let after_label=after_label.trim_start();
            let Some(rest)=after_label.strip_prefix(':')
                .or_else(||after_label.strip_prefix('：')) else{continue};
            return rest.trim().lines().map(str::trim).collect::<Vec<_>>().join(" ");
        }
    }
    String::new()
}
pub(super) fn explicit_sender(raw:&str,base:&Value)->Option<Value>{
    let mut offset=0;
    for line in raw.split_inclusive('\n'){
        if let Some(name)=explicit_sender_name(line.trim_end_matches('\n').trim_end_matches('\r')){
            let relevant=&raw[offset..];
            let mut sender=base.clone();
            let map=sender.as_object_mut()?;
            map.insert("name".into(),json!(name));
            // Preserve historical fields on the base; never regenerate the retired invoice name.
            map.insert("address".into(),json!(explicit_sender_address(relevant)));
            map.insert("phone".into(),json!(super::workflow_issues::sender_phone(relevant)));
            map.insert("mobile".into(),json!(""));
            map.insert("postcode".into(),json!(""));
            return Some(sender);
        }
        offset+=line.len();
    }
    None
}
pub(super) fn sender_for(state:&Value,order:&Value)->Value {
    if has_value(field(order,"senderSnapshot")){
        return field(order,"senderSnapshot").clone();
    }
    if ["DEALER","RETAIL"].contains(&str_field(order,"orderType")){
        return company_client(state);
    }
    // senderFor uses the frozen order clientSnapshot BEFORE a changing
    // master-client record, independently of the order confirmation stage.
    let base=order.get("clientSnapshot").filter(|c|!c.is_null())
        .or_else(||find_by_id(field(state,"clients"),str_field(order,"clientId")))
        .cloned().unwrap_or(Value::Null);
    let raw=field(order,"analysisRaw").as_str().unwrap_or_else(||str_field(order,"raw"));
    explicit_sender(raw,&base).unwrap_or(base)
}
/// Complete missing label fields at export without rewriting frozen evidence.
/// AGENCY may only use its own dealer or explicit sender, never house settings.
pub(crate) fn sender_for_export(state:&Value,order:&Value,frozen:&Value)->Value{
    let present=|value:&Value,key:&str|!str_field(value,key).trim().is_empty();
    if present(frozen,"name")&&present(frozen,"address")&&
        (present(frozen,"phone")||present(frozen,"mobile")){
        return frozen.clone();
    }
    let kind=str_field(order,"orderType");
    let source=match kind{
        "DEALER"|"RETAIL"=>company_client(state),
        "AGENCY"=>{
            let id=str_field(order,"clientId");
            let Some(client)=find_by_id(field(state,"clients"),id)
                .filter(|client|!id.is_empty()&&!has_value(field(client,"deletedAt")))
                else{return frozen.clone()};
            let raw=field(order,"analysisRaw").as_str().unwrap_or_else(||str_field(order,"raw"));
            explicit_sender(raw,client).unwrap_or_else(||client.clone())
        },
        _=>return frozen.clone(),
    };
    if kind=="AGENCY" && ((present(frozen,"id")&&str_field(frozen,"id")!=str_field(&source,"id"))||
        (present(frozen,"name")&&str_field(frozen,"name").trim()!=str_field(&source,"name").trim())){
        return frozen.clone();
    }
    let mut result=if frozen.is_object(){frozen.clone()}else{json!({})};
    for key in ["name","address","postcode"]{
        if !present(&result,key)&&present(&source,key){result[key]=field(&source,key).clone();}
    }
    if !present(&result,"phone")&&!present(&result,"mobile"){
        for key in ["phone","mobile"]{
            if present(&source,key){result[key]=field(&source,key).clone();}
        }
    }
    result
}
fn signature(order:&Value,recipient:&Value,sender:&Value)->String {
    let sender_data=json!([field(sender,"name"),field(sender,"displayName"),
        field(sender,"address"),field(sender,"phone"),field(sender,"mobile"),
        field(sender,"payment")]);
    let mut recipient_data=vec![field(recipient,"name").clone(),field(recipient,"address").clone(),
        field(recipient,"addressStatus").clone(),field(recipient,"phone").clone(),
        field(recipient,"mobile").clone(),field(recipient,"note").clone()];
    // Keep existing signatures byte-identical for ordinary orders. When a
    // remote charge or employee override changes the bill, invalidate the old
    // workflow selection and payment notice just like any other freight edit.
    let remote=super::remote_delivery::resolve_recipient(recipient).ok();
    let selected=remote.as_ref().is_some_and(|v|flag(v,"checked"));
    let manual=remote.as_ref().is_some_and(|v|flag(v,"manual"));
    if selected || manual {recipient_data.push(json!(selected));}
    let recipient_data=json!(recipient_data);
    let items=rows(field(recipient,"items")).iter().map(|i|json!([
        field(i,"id"),field(i,"productId"),field(i,"variantId"),field(i,"color"),
        field(i,"quantity"),field(i,"quantityConfirmed"),field(i,"unitPrice"),
        field(i,"origin"),field(i,"issues")
    ])).collect::<Vec<_>>();
    // Explicit key order is significant: TS stores JSON.stringify output.
    format!("{{\"dealer\":{},\"sender\":{},\"recipient\":{},\"items\":{}}}",
        field(order,"clientId"),sender_data,recipient_data,json!(items))
}
/// Preserve the old signature only for the exact former generated-name
/// representation of otherwise identical data. Frozen snapshots stay intact.
fn compatible_info_signature(order:&Value,recipient:&Value,sender:&Value,old:&str)->String{
    let current=signature(order,recipient,sender);
    if old==current || has_value(field(order,"senderSnapshot")) || old.is_empty(){
        return current;
    }
    let raw=field(order,"analysisRaw").as_str().unwrap_or_else(||str_field(order,"raw"));
    if ["DEALER","RETAIL"].contains(&str_field(order,"orderType")) ||
        raw.lines().any(|line|explicit_sender_name(line).is_some()){
        let mut legacy=sender.clone();
        if let Some(map)=legacy.as_object_mut(){
            map.insert("displayName".into(),field(sender,"name").clone());
            if signature(order,recipient,&legacy)==old{return old.to_owned();}
        }
    }
    current
}
/// Separate order-review identity from financial values and non-identity
/// contact corrections. Unknown legacy signatures retain the old safe path.
fn review_basis(raw:&str,ignore_price:bool)->Option<Value>{
    let mut value:Value=serde_json::from_str(raw).ok()?;
    let recipient=value.get_mut("recipient")?.as_array_mut()?;
    if !(6..=7).contains(&recipient.len()){return None;}
    for index in [3,4,5]{recipient[index]=Value::Null;}
    // Explicit false and the ordinary absent remote marker are equivalent.
    if recipient.len()==7&&recipient[6]==Value::Bool(false){recipient.pop();}
    let items=value.get_mut("items")?.as_array_mut()?;
    for item in items{
        let values=item.as_array_mut()?;
        if values.len()!=9{return None;}
        if ignore_price{values[6]=Value::Null;}
    }
    Some(value)
}
fn same_review_basis(before:&str,after:&str,ignore_price:bool)->bool{
    review_basis(before,ignore_price).zip(review_basis(after,ignore_price))
        .is_some_and(|(old,new)|old==new)
}

/// Compare historical billing evidence with actual per-shipment totals.
/// Keep the existing serialized signature format for old databases and
/// export audit compatibility; only use a proven matching notice to ignore
/// differences in tax/price representation.
fn same_billing_effect(state:&Value,index:usize,old:&str,new:&str)->Result<bool,String>{
    let (Ok(mut previous),Ok(mut current))=(
        serde_json::from_str::<Value>(old),serde_json::from_str::<Value>(new)
    )else{return Ok(false);};
    if field(&previous,"account")!=field(&current,"account"){return Ok(false);}
    let before=str_field(&previous,"info");
    let after=str_field(&current,"info");
    if same_review_basis(before,after,false){
        previous["info"]=Value::Null;
        current["info"]=Value::Null;
        if previous==current{return Ok(true);}
    }
    let ship=&state["workflow"]["shipments"][index];
    let group=find_by_id(field(&state["workflow"],"groups"),str_field(ship,"groupId"))
        .ok_or("현재 주문 묶음을 찾지 못했습니다.")?;
    let Some(notice)=rows(field(group,"notices")).last()else{return Ok(false);};
    let sid=str_field(ship,"id");
    if !rows(field(notice,"shipmentIds")).iter().any(|id|id.as_str()==Some(sid)){
        return Ok(false);
    }
    let account=field(notice,"account");
    let notice_key=json!([field(account,"id"),str_field(account,"bank").trim(),
        str_field(account,"number").trim(),str_field(account,"holder").trim()]);
    let encoded=serde_json::to_string(&notice_key).map_err(|e|e.to_string())?;
    if str_field(&current,"account")!=encoded{return Ok(false);}
    let actual=super::workflow::shipment_totals(state,sid)?;
    Ok(super::safe(field(field(notice,"amounts"),sid))
        .zip(super::safe(field(&actual,"total"))).is_some_and(|(a,b)|a==b))
}

/// Physical delivery inputs are independent of recipient name, contact,
/// sender metadata and unit price. Unknown history must recheck freight.
fn freight_review_basis(raw:&str)->Option<Value>{
    let info:Value=serde_json::from_str(raw).ok()?;
    let recipient=field(&info,"recipient").as_array()?;
    if !(6..=7).contains(&recipient.len()){return None;}
    let items=field(&info,"items").as_array()?;
    let mut physical=Vec::new();
    for item in items{
        let values=item.as_array()?;
        if values.len()!=9{return None;}
        physical.push(json!([values[1],values[2],values[3],values[4],values[7]]));
    }
    Some(json!([field(&info,"dealer"),recipient[1],
        recipient.get(6).and_then(Value::as_bool).unwrap_or(false),physical]))
}
fn same_freight_review_basis(before:&str,after:&str)->bool{
    freight_review_basis(before).zip(freight_review_basis(after))
        .is_some_and(|(old,new)|old==new)
}

fn downgrade(state:&mut Value,audit:&mut Audit<'_>,index:usize,to:i64,reason:&str,
    preserve_financial:bool,invalidate_freight:bool)->Result<(),String> {
    let ship=state["workflow"]["shipments"][index].clone();
    let old=stages::read(&ship);
    let before=old.iter().rposition(|done|*done).map_or(0,|i|i as i64+1);
    let mut next=old;
    if to==0{next[0]=false;}
    if invalidate_freight{next[1]=false;}
    if !preserve_financial || to>=2{next[2]=false;next[3]=false;}
    if next==old{return Ok(());}
    let ship_id=str_field(&ship,"id").to_string();
    let group_id=str_field(&ship,"groupId").to_string();
    stages::write(&mut state["workflow"]["shipments"][index],next)?;
    let to=stages::prefix(next);
    if (old[3]&&!next[3])||(old[2]&&!next[2]) {
        let confirmed=rows(field(&state["workflow"],"confirmations")).iter()
            .filter(|c|rows(field(c,"shipmentIds")).iter()
                .any(|id|id.as_str()==Some(&ship_id)))
            .filter(|c|!rows(field(&state["workflow"],"events")).iter().any(|e|
                str_field(e,"action")=="입금 승인 재확인 필요" &&
                field(e,"before")==field(c,"id") &&
                rows(field(e,"shipmentIds")).iter()
                    .any(|id|id.as_str()==Some(&ship_id))))
            .map(|c|field(c,"id").clone()).collect::<Vec<_>>();
        for id in confirmed {
            audit.emit(state,"입금 승인 재확인 필요",&[ship_id.clone()],
                &group_id,Some(id),None,Some(reason),"시스템")?;
        }
    }
    if !field(&state["workflow"]["shipments"][index],"rechecks").is_array(){
        set(&mut state["workflow"]["shipments"][index],"rechecks",json!([]))?;
    }
    state["workflow"]["shipments"][index]["rechecks"].as_array_mut()
        .ok_or("재검증 이력 오류")?.push(json!({
            "at":required_str(audit.payload,"now")?,"reason":reason,
            "fromStage":before,"toStage":to
        }));
    if !next[3]{
        set(&mut state["workflow"]["shipments"][index],"legacyApproval",json!(false))?;
    }
    if old[2]&&!next[2] {
        let gi=rows(field(&state["workflow"],"groups")).iter()
            .position(|g|str_field(g,"id")==group_id)
            .ok_or("현재 주문 묶음을 찾지 못했습니다.")?;
        set(&mut state["workflow"]["groups"][gi],"needsNotice",json!(true))?;
    }
    audit.emit(state,"재검증 필요",&[ship_id],&group_id,
        Some(json!(before)),Some(json!(to)),Some(reason),"시스템")
}

/// A group re-notice invalidates financial approval even if its numeric
/// compatibility step is lower due to an independent review.
pub(super) fn invalidate_notice_approvals(state:&mut Value,payload:&Value,ships:&[usize])->Result<(),String>{
    let seed=format!("{}:notice",required_str(payload,"eventId")?);
    let mut audit=Audit{payload,position:0,scoped_seed:Some(&seed)};
    for &si in ships{downgrade(state,&mut audit,si,2,"청구 / 계좌 재안내",false,false)?;}
    Ok(())
}

/// Preserve downstream approvals only when the immutable group notice,
/// shipment allocation amount, payer and account still match. Unknown
/// legacy history or insufficient payment must take the conservative path.
pub(super) fn proven_financial_preservation(state:&Value,index:usize)->Result<bool,String>{
    let ship=&state["workflow"]["shipments"][index];
    let gid=str_field(ship,"groupId");
    let group=find_by_id(field(&state["workflow"],"groups"),gid)
        .ok_or("현재 주문 묶음을 찾지 못했습니다.")?;
    if rows(field(group,"notices")).is_empty() ||
        super::workflow_issues::notice_pending(state,gid)?{return Ok(false);}
    let old=serde_json::from_str::<Value>(str_field(ship,"billingSignature")).ok();
    let Some(old)=old else{return Ok(false);};
    let old_info=serde_json::from_str::<Value>(str_field(&old,"info")).ok();
    let Some(old_info)=old_info else{return Ok(false);};
    let order=find_by_id(field(state,"orders"),str_field(ship,"orderId"))
        .ok_or("배송 건의 발주서를 찾을 수 없습니다.")?;
    if str_field(&old_info,"dealer")!=str_field(order,"clientId"){return Ok(false);}
    let client=party(state,order).ok_or("거래처 정보를 확인하세요.")?;
    let new=billing(state,index)?;
    if !same_billing_effect(state,index,str_field(ship,"billingSignature"),&new)?{return Ok(false);}
    if stages::read(ship)[3] && str_field(client,"payment")=="PREPAID" &&
        super::workflow::payment_due(state,ship,order,client)?!=Some(0){return Ok(false);}
    // An unknown payment policy is not sufficient approval evidence.
    Ok(matches!(str_field(client,"payment"),"PREPAID"|"POSTPAID"))
}

pub(super) fn billing(state:&Value,index:usize)->Result<String,String> {
    let ship=&state["workflow"]["shipments"][index];
    let group_id=str_field(ship,"groupId");
    let group=find_by_id(field(&state["workflow"],"groups"),group_id)
        .ok_or("현재 주문 묶음을 찾지 못했습니다.")?;
    let order=find_by_id(field(state,"orders"),str_field(ship,"orderId"))
        .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    let recipient=find_by_id(field(order,"recipients"),str_field(ship,"recipientId"))
        .ok_or("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.")?;
    let client=party(state,order).ok_or("거래처 정보를 확인하세요.")?;
    let account=super::workflow::account_for(state,group,order,client);
    let account_key=if let Some(a)=account {
        serde_json::to_string(&json!([field(a,"id"),
            str_field(a,"bank").trim(),str_field(a,"number").trim(),
            str_field(a,"holder").trim()])).map_err(|e|e.to_string())?
    } else {String::new()};
    let vat=field(recipient,"vatOverride").as_bool()
        .or_else(||field(recipient,"vatDefault").as_bool())
        .unwrap_or(flag(order,"applyVat"));
    let collect=if has_value(field(recipient,"freightCode")) {
        str_field(recipient,"freightCode")=="020"
    } else {str_field(order,"freightCode")=="020"};
    let str_json=|s:&str|serde_json::to_string(s).unwrap_or_else(|_|"null".to_string());
    let suffix=if collect {",\"collect\":true"} else {""};
    let rate=field(recipient,"vatRateBps").as_i64().unwrap_or(1000);
    let rate_suffix=if vat && rate!=1000{format!(",\"vatRateBps\":{rate}")}else{String::new()};
    Ok(format!("{{\"info\":{},\"fees\":{},\"vat\":{},\"account\":{}{suffix}{rate_suffix}}}",
        field(ship,"infoSignature"),field(ship,"charges"),vat,str_json(&account_key)))
}
pub(super) fn reconcile(payload:&Value)->Result<Value,String> {
    reconcile_inner(payload,None,None)
}

/// Scope an existing employee shipment edit to its source order, including
/// all split groups and recipients. Group/source ownership remains checked
/// by the unchanged global workflow and SQLite validations at commit time.
/// Do not use this route for draft insertion or workflow progression.
pub(super) fn reconcile_shipment(payload:&Value)->Result<Value,String>{
    if payload.get("draft").is_some(){
        return Err("배송 건 재검증에서는 새 발주서를 확정할 수 없습니다.".into());
    }
    let state=req(payload,"state")?;
    let shipment_id=required_str(payload,"shipmentId")?;
    let group_id=required_str(payload,"groupId")?;
    let seed=required_str(payload,"eventSeed")?;
    if shipment_id.trim().is_empty() || group_id.trim().is_empty() ||
        seed.trim().is_empty(){
        return Err("배송 건 재검증 ID가 누락되었습니다.".into());
    }
    let ship=find_by_id(field(field(state,"workflow"),"shipments"),shipment_id)
        .ok_or("수정할 배송 건을 확인하세요.")?;
    if flag(ship,"transferred") || flag(ship,"legacyArchived") ||
        str_field(ship,"groupId")!=group_id{
        return Err("현재 묶음의 수정 가능한 배송 건을 확인하세요.".into());
    }
    let order_id=str_field(ship,"orderId");
    let order=find_by_id(field(state,"orders"),order_id)
        .ok_or("배송 건의 발주서를 찾을 수 없습니다.")?;
    if order_id.is_empty() || has_value(field(order,"deletedAt")) ||
        !rows(field(order,"recipients")).iter().any(|row|
            str_field(row,"id")==str_field(ship,"recipientId")){
        return Err("배송 건의 발주서 / 수취인 연결을 확인하세요.".into());
    }
    reconcile_inner(payload,Some(order_id),Some(seed))
}

fn reconcile_inner(payload:&Value,order_scope:Option<&str>,scoped_seed:Option<&str>)
    ->Result<Value,String>{
    let input=req(payload,"state")?;
    let mut state=input.clone();
    let mut audit=Audit{payload,position:0,scoped_seed};
    // Re-evaluate after all recipient/freight updates, not halfway through
    // a change that could transiently miscalculate the group bill.
    let mut pending_rechecks=std::collections::BTreeMap::<usize,(i64,bool,&'static str)>::new();
    if let Some(draft)=payload.get("draft").filter(|d|d.is_object() &&
        !has_value(field(d,"deletedAt")) &&
        (!str_field(d,"clientId").is_empty() || str_field(d,"orderType")=="RETAIL") &&
        (find_by_id(field(&state,"clients"),str_field(d,"clientId")).is_some() ||
            has_value(field(d,"clientSnapshot")))) {
        let id=str_field(draft,"id");
        if let Some(i)=rows(field(&state,"orders")).iter()
            .position(|o|str_field(o,"id")==id) {
            state["orders"][i]=draft.clone();
        } else {
            state["orders"].as_array_mut().ok_or("주문 목록 누락")?.push(draft.clone());
        }
    }
    if !field(&state,"workflow").is_object() {
        set(&mut state,"workflow",workflow_empty())?;
    }
    if !field(&state["workflow"],"confirmations").is_array() {
        set(&mut state["workflow"],"confirmations",json!([]))?;
    }
    let order_count=rows(field(&state,"orders")).len();
    for oi in 0..order_count {
        let mut order=state["orders"][oi].clone();
        let order_id=str_field(&order,"id").to_string();
        if order_scope.is_some_and(|id|id!=order_id.as_str()){continue;}
        let dealer_id=str_field(&order,"clientId").to_string();
        if has_value(field(&order,"deletedAt")) ||
            (dealer_id.is_empty() && str_field(&order,"orderType")!="RETAIL") ||
            (find_by_id(field(&state,"clients"),&dealer_id).is_none() &&
                !has_value(field(&order,"clientSnapshot"))) {
            continue;
        }
        super::issues::synchronize(&mut order)?;
        state["orders"][oi]=order.clone();
        let source_id=format!("source:{order_id}");
        let root_id=format!("group:{order_id}");
        if !rows(field(&state["workflow"],"sources")).iter().any(|s|
            str_field(s,"orderId")==order_id) {
            let number=rows(field(&state["workflow"],"sources")).iter()
                .filter_map(|s|field(s,"number").as_i64()).max().unwrap_or(0).max(0)+1;
            state["workflow"]["sources"].as_array_mut()
                .ok_or("원문 목록 누락")?.push(json!({
                    "number":number,"id":source_id,"orderId":order_id,
                    "dealerId":dealer_id,"raw":field(&order,"raw"),
                    "createdAt":field(&order,"createdAt")
                }));
        }
        let source=rows(field(&state["workflow"],"sources")).iter()
            .find(|s|str_field(s,"orderId")==order_id)
            .ok_or("원문 기록 누락")?;
        let actual_source_id=str_field(source,"id").to_string();
        if find_by_id(field(&state["workflow"],"groups"),&root_id).is_none() {
            let mut root=json!({"id":root_id,"sourceId":actual_source_id,
                "dealerId":dealer_id,"rootGroupId":root_id,
                "createdAt":field(&order,"createdAt"),"notices":[],
                "needsNotice":false});
            if let Some(edits)=order.get("messageEdits") {
                set(&mut root,"messageEdits",edits.clone())?;
            }
            state["workflow"]["groups"].as_array_mut()
                .ok_or("묶음 목록 누락")?.push(root);
        }
        // The TypeScript clientChangeIssue prevents changes once financial
        // or dispatch evidence exists; earlier steps may be reassigned.
        let group_ids=rows(field(&state["workflow"],"groups")).iter()
            .filter(|g|str_field(g,"sourceId")==actual_source_id)
            .map(|g|str_field(g,"id").to_string()).collect::<Vec<_>>();
        let affected=rows(field(&state["workflow"],"shipments")).iter()
            .filter(|s|str_field(s,"orderId")==order_id &&
                group_ids.iter().any(|gid|gid==str_field(s,"groupId")) &&
                find_by_id(field(&state["workflow"],"groups"),str_field(s,"groupId"))
                    .is_some_and(|g|str_field(g,"dealerId")!=dealer_id))
            .collect::<Vec<_>>();
        if affected.iter().any(|s|flag(s,"transferred") ||
            stages::read(s)[3] ||
            has_value(field(s,"delivery")) ||
            rows(field(&state["workflow"],"payments")).iter().any(|p|
                rows(field(p,"allocations")).iter().any(|a|
                    str_field(a,"shipmentId")==str_field(s,"id"))) ||
            rows(field(&state["workflow"],"queue")).iter().any(|q|
                str_field(q,"shipmentId")==str_field(s,"id") &&
                str_field(q,"status")!="CANCELLED")) {
            return Err("입금 / 출고 이력이 있는 주문은 거래처를 바로 변경할 수 없습니다. 기존 건을 확인한 뒤 새 주문으로 접수하세요.".into());
        }
        for gid in group_ids {
            let gi=rows(field(&state["workflow"],"groups")).iter()
                .position(|g|str_field(g,"id")==gid)
                .ok_or("현재 주문 묶음을 찾지 못했습니다.")?;
            let old_dealer=str_field(&state["workflow"]["groups"][gi],"dealerId").to_string();
            if old_dealer!=dealer_id {
                set(&mut state["workflow"]["groups"][gi],"dealerId",json!(dealer_id))?;
                let ship_ids=rows(field(&state["workflow"],"shipments")).iter()
                    .filter(|s|str_field(s,"groupId")==gid)
                    .map(|s|str_field(s,"id").to_string()).collect::<Vec<_>>();
                let actor=str_field(field(&state,"settings"),"operatorName").trim().to_string();
                audit.emit(&mut state,"거래처 변경",&ship_ids,&gid,
                    Some(json!(old_dealer)),Some(json!(dealer_id)),None,
                    if actor.is_empty() {"직원"} else {&actor})?;
            }
            if let Some(edits)=order.get("groupMessageEdits") {
                if let Some(value)=edits.get(&gid) {
                    set(&mut state["workflow"]["groups"][gi],"messageEdits",value.clone())?;
                } else {
                    state["workflow"]["groups"][gi].as_object_mut()
                        .ok_or("묶음 정보 오류")?.remove("messageEdits");
                }
            }
            if let Some(account)=field(&order,"groupAccounts").get(&gid) {
                set(&mut state["workflow"]["groups"][gi],"accountMode",
                    field(account,"mode").clone())?;
                set(&mut state["workflow"]["groups"][gi],"accountId",
                    field(account,"id").clone())?;
            }
        }
        let recipient_count=rows(field(&state["orders"][oi],"recipients")).len();
        for ri in 0..recipient_count {
            let mut recipient=state["orders"][oi]["recipients"][ri].clone();
            if rows(field(&recipient,"items")).is_empty() &&
                ["name","address","phone","mobile"].iter()
                    .all(|key|str_field(&recipient,key).is_empty()) {
                continue;
            }
            let ship_id=format!("shipment:{order_id}:{}",str_field(&recipient,"id"));
            let mut index=rows(field(&state["workflow"],"shipments")).iter().position(|s|
                str_field(s,"orderId")==order_id &&
                str_field(s,"recipientId")==str_field(&recipient,"id"));
            let order_before=state["orders"][oi].clone();
            let frozen=index.is_some() && (has_value(field(&order_before,"confirmedAt")) ||
                flag(&order_before,"pricesLocked"));
            // Migrate only when a decision is absent/stale. Old previously
            // billed or dispatched orders must NOT silently acquire a 2x
            // charge after a new remote-area dataset is deployed.
            let current_address=str_field(&recipient,"address").to_string();
            let saved_address=str_field(field(&recipient,"remoteDelivery"),"address");
            if saved_address!=current_address ||
                !field(&recipient,"remoteDelivery").is_object() {
                let existing_ship=index.and_then(|si|
                    rows(field(&state["workflow"],"shipments")).get(si));
                let old_bill=existing_ship.is_some_and(|ship|
                    flag(ship,"transferred") ||
                    field(ship,"step").as_i64().unwrap_or(0)>=1) ||
                    has_value(field(&order_before,"confirmedAt")) ||
                    flag(&order_before,"pricesLocked");
                let analysis=super::remote_delivery::detect(&json!({"address":current_address}))?;
                if old_bill {
                    set(&mut recipient,"remoteDelivery",json!({
                        "address":current_address,"checked":false,
                        "manual":false,"legacy":true
                    }))?;
                    state["orders"][oi]["recipients"][ri]=recipient.clone();
                } else if flag(&analysis,"suggestedChecked") {
                    set(&mut recipient,"remoteDelivery",json!({
                        "address":current_address,"checked":true,"manual":false
                    }))?;
                    state["orders"][oi]["recipients"][ri]=recipient.clone();
                } else if field(&recipient,"remoteDelivery").is_object() {
                    recipient.as_object_mut().ok_or("수취인 형식 오류")?
                        .remove("remoteDelivery");
                    state["orders"][oi]["recipients"][ri]=recipient.clone();
                }
            }
            let order=state["orders"][oi].clone();
            let client=party(&state,&order).ok_or("거래처 정보를 확인하세요.")?.clone();
            let sender=sender_for(&state,&order);
            let previous_info=index.map(|si|str_field(
                &state["workflow"]["shipments"][si],"infoSignature")).unwrap_or("");
            let info=compatible_info_signature(&order,&recipient,&sender,previous_info);
            let mut single=order.clone();
            set(&mut single,"recipients",json!([recipient]))?;
            let reco=shipping::recommend(&single,&state)?;
            if index.is_none() {
                if recipient.get("vatDefault").is_none() {
                    let value=if rows(field(input,"orders")).iter().any(|o|
                        str_field(o,"id")==order_id) {
                        flag(&order,"applyVat")
                    } else {flag(&client,"defaultApplyVat")};
                    set(&mut recipient,"vatDefault",json!(value))?;
                    state["orders"][oi]["recipients"][ri]=recipient.clone();
                }
                let ship=json!({"recipientSnapshot":recipient,
                    "id":ship_id,"sourceId":actual_source_id,"orderId":order_id,
                    "recipientId":field(&recipient,"id"),"groupId":root_id,
                    "step":0,"stageValidity":[false,false,false,false],"completedAt":{},"charges":{},"chargeMode":"AUTO",
                    "freightVersion":field(&reco,"version"),"infoSignature":info,
                    "transferred":false});
                state["workflow"]["shipments"].as_array_mut()
                    .ok_or("배송 건 목록 누락")?.push(ship);
                index=Some(rows(field(&state["workflow"],"shipments")).len()-1);
            }
            let si=index.ok_or("배송 건 누락")?;
            if flag(&state["workflow"]["shipments"][si],"transferred") ||
                flag(&state["workflow"]["shipments"][si],"legacyArchived") {continue;}
            let previous_info=str_field(&state["workflow"]["shipments"][si],"infoSignature");
            if previous_info!=info {
                let same_review=same_review_basis(previous_info,&info,true);
                let review_valid=if same_review{
                    let ship=&state["workflow"]["shipments"][si];
                    let gid=str_field(ship,"groupId");
                    let rid=str_field(ship,"recipientId");
                    !super::workflow_issues::workflow_issues(&state,gid)?.iter().any(|issue|
                        str_field(issue,"severity")=="ACTION" &&
                        field(issue,"blockingStage").as_i64().is_some_and(|stage|stage<=1) &&
                        (str_field(issue,"recipientId").is_empty()||str_field(issue,"recipientId")==rid))
                }else{false};
                if !review_valid{
                    let freight_changed=!same_freight_review_basis(previous_info,&info);
                    pending_rechecks.insert(si,(0,freight_changed,"수취인 / 상품 / 수량 / 가격 / 출고지 / 송하인 변경"));
                }
                set(&mut state["workflow"]["shipments"][si],"infoSignature",json!(info))?;
            }
            let old_version=field(&state["workflow"]["shipments"][si],"freightVersion");
            let freight_confirmed=stages::read(&state["workflow"]["shipments"][si])[1];
            if !frozen && old_version!=field(&reco,"version") &&
                !(freight_confirmed && str_field(&state["workflow"]["shipments"][si],"chargeMode")=="MANUAL") {
                pending_rechecks.entry(si).and_modify(|entry|entry.1=true).or_insert((1,true,"적용 운임 기준 변경"));
                set(&mut state["workflow"]["shipments"][si],"freightVersion",
                    field(&reco,"version").clone())?;
            }
            let old_snapshot=field(&state["workflow"]["shipments"][si],"recipientSnapshot");
            let old_code=old_snapshot.get("freightCode")
                .filter(|v|!v.is_null())
                .unwrap_or_else(||field(&order,"freightCode"));
            let new_code=field(&recipient,"freightCode");
            let new_code=if new_code.is_null(){field(&order,"freightCode")}else{new_code};
            if old_snapshot.is_object() && old_code!=new_code {
                pending_rechecks.entry(si).and_modify(|entry|entry.1=true).or_insert((1,true,"운임 구분 변경"));
            }
            set(&mut state["workflow"]["shipments"][si],"recipientSnapshot",recipient.clone())?;
            let previous_charges=field(&state["workflow"]["shipments"][si],"charges").clone();
            let mut charges=previous_charges.clone();
            if !charges.is_object() {charges=json!({});}
            let origins=rows(field(&recipient,"items")).iter()
                .map(|i|str_field(i,"origin").to_string()).collect::<Vec<_>>();
            for origin in ["FACTORY","STORE"] {
                if !origins.iter().any(|x|x==origin) {
                    charges.as_object_mut().ok_or("운임 형식 오류")?.remove(origin);
                }
            }
            let overrides=field(&recipient,"shippingCharges");
            let has_manual_amount=origins.iter().any(|origin|
                field(overrides,origin).as_i64().is_some());
            let cleared_override=overrides.is_object() && !has_manual_amount &&
                origins.iter().all(|origin|field(overrides,origin).is_null());
            // Draft editing stores null for a cleared override. The native
            // reception quote treats null as automatic, not a manual blank.
            // Keep the same contract when projecting the working shipment.
            // Existing frozen MANUAL shipments retain their historical blank.
            if cleared_override && !frozen {
                set(&mut state["workflow"]["shipments"][si],"chargeMode",json!("AUTO"))?;
            }
            let automatic=str_field(&state["workflow"]["shipments"][si],"chargeMode")=="AUTO";
            if has_value(overrides) && !(cleared_override && automatic) {
                set(&mut state["workflow"]["shipments"][si],"chargeMode",json!("MANUAL"))?;
                for origin in &origins {
                    let v=field(overrides,origin);
                    set(&mut charges,origin,v.clone())?;
                }
            } else if automatic && !frozen {
                for origin in &origins {
                    let v=rows(field(&reco,"bundles")).iter()
                        .find(|b|str_field(b,"origin")==origin)
                        .map(|b|b.get("baseRecommendation")
                            .unwrap_or_else(||field(b,"recommendation")))
                        .unwrap_or(&Value::Null);
                    set(&mut charges,origin,v.clone())?;
                }
            }
            if flag(&client,"shippingFeeExcluded") {
                for origin in &origins {set(&mut charges,origin,json!(0))?;}
            }
            set(&mut state["workflow"]["shipments"][si],"charges",charges.clone())?;
            if previous_charges!=charges {
                pending_rechecks.entry(si).and_modify(|entry|entry.1=true).or_insert((1,true,"추천 운임 변경"));
            }
        }
    }
    let count=rows(field(&state["workflow"],"shipments")).len();
    for si in 0..count {
        let ship=state["workflow"]["shipments"][si].clone();
        if order_scope.is_some_and(|id|id!=str_field(&ship,"orderId")){continue;}
        let order=find_by_id(field(&state,"orders"),str_field(&ship,"orderId"));
        let exists=order.is_some_and(|o|rows(field(o,"recipients")).iter().any(|r|
            str_field(r,"id")==str_field(&ship,"recipientId")));
        if !exists && !flag(&ship,"legacyArchived") {
            if flag(&ship,"transferred") ||
                rows(field(&state["workflow"],"payments")).iter().any(|p|
                    rows(field(p,"allocations")).iter().any(|a|
                        str_field(a,"shipmentId")==str_field(&ship,"id"))) {
                return Err("이관 / 입금 기록이 있는 배송 건은 삭제할 수 없습니다.".into());
            }
            set(&mut state["workflow"]["shipments"][si],"legacyArchived",json!(true))?;
            audit.emit(&mut state,"배송 건 제외",
                &[str_field(&ship,"id").to_string()],str_field(&ship,"groupId"),
                None,None,None,"직원")?;
        }
    }
    for (si,(to,freight_changed,reason)) in pending_rechecks {
        if flag(&state["workflow"]["shipments"][si],"legacyArchived"){continue;}
        let old=stages::read(&state["workflow"]["shipments"][si]);
        let preserve=old[2]&&proven_financial_preservation(&state,si)?;
        downgrade(&mut state,&mut audit,si,to,reason,preserve,freight_changed)?;
    }
    for si in 0..count {
        let ship=state["workflow"]["shipments"][si].clone();
        if order_scope.is_some_and(|id|id!=str_field(&ship,"orderId")){continue;}
        if !stages::read(&ship)[2] ||
            flag(&ship,"transferred") || flag(&ship,"legacyArchived") ||
            !has_value(field(&ship,"billingSignature")) {continue;}
        let current_billing=billing(&state,si)?;
        let previous_billing=str_field(&ship,"billingSignature");
        if previous_billing!=current_billing {
            if same_billing_effect(&state,si,previous_billing,&current_billing)?{
                // Refresh comparison metadata without revoking valid approval
                // or altering the frozen notice, payment and audit history.
                set(&mut state["workflow"]["shipments"][si],"billingSignature",json!(current_billing))?;
            }else{
                downgrade(&mut state,&mut audit,si,2,"계좌 / 청구 금액 변경",false,false)?;
            }
        }
    }
    Ok(json!({"state":state,"emittedEvents":audit.position}))
}


#[cfg(test)]
mod retired_sender_name_tests{
    use super::*;
    #[test]
    fn generated_senders_use_official_name_and_preserve_legacy_signature(){
        let state=json!({"settings":{"senderName":"반도스탠드","senderAddress":"가상 주소",
            "senderPhone":"02-1234-5678"}});
        let company=company_client(&state);
        assert_eq!(company["name"],"반도스탠드");
        assert!(company.get("displayName").is_none());
        assert!(super::super::validation::company_party(&state).get("displayName").is_none());
        let explicit=explicit_sender("송하인: 가상 송하인\n주소: 가상 주소\n010-1234-5678",
            &json!({"id":"c","name":"가상 거래처"})).unwrap();
        assert_eq!(explicit["name"],"가상 송하인");
        assert!(explicit.get("displayName").is_none());
        let order=json!({"orderType":"DEALER","clientId":"c"});
        let recipient=json!({"name":"가상 수취인","address":"가상 주소","items":[]});
        let mut legacy=company.clone();
        legacy["displayName"]=company["name"].clone();
        let old=signature(&order,&recipient,&legacy);
        assert_ne!(signature(&order,&recipient,&company),old);
        assert_eq!(compatible_info_signature(&order,&recipient,&company,&old),old);
        let mut edited=recipient.clone();
        edited["address"]=json!("다른 주소");
        assert_ne!(compatible_info_signature(&order,&edited,&company,&old),old);
        let mut frozen=order.clone();
        frozen["senderSnapshot"]=legacy.clone();
        assert_eq!(sender_for(&state,&frozen),legacy);
        assert_eq!(compatible_info_signature(&frozen,&recipient,&legacy,&old),old);
    }
}

#[cfg(test)]
mod stage_impact_tests{
    use super::*;
    #[test]
    fn review_identity_ignores_contact_note_and_price_but_not_goods(){
        let order=json!({"clientId":"c"});
        let sender=json!({"name":"발송처"});
        let mut row=json!({"name":"수취인","address":"서울 주소","phone":"010-1234-5678",
            "note":"","items":[{"id":"i","productId":"p","variantId":"v",
                "quantity":1,"unitPrice":30000,"origin":"FACTORY"}]});
        let original=signature(&order,&row,&sender);
        row["phone"]=json!("010-9876-5432");
        row["note"]=json!("문 앞");
        assert!(same_review_basis(&original,&signature(&order,&row,&sender),false));
        row["items"][0]["unitPrice"]=json!(31000);
        assert!(same_review_basis(&original,&signature(&order,&row,&sender),true));
        assert!(!same_review_basis(&original,&signature(&order,&row,&sender),false));
        row["items"][0]["quantity"]=json!(2);
        assert!(!same_review_basis(&original,&signature(&order,&row,&sender),true));
        assert!(!same_review_basis("unknown",&original,true));
    }
    #[test]
    fn contact_only_billing_difference_does_not_revoke_approval(){
        let order=json!({"clientId":"c"});
        let sender=json!({"name":"발송처"});
        let mut row=json!({"name":"수취인","address":"서울 주소","note":"","items":[]});
        let old=signature(&order,&row,&sender);
        row["note"]=json!("문 앞");
        let new=signature(&order,&row,&sender);
        let bill=|info:&str,account:&str|json!({"info":info,"fees":{"FACTORY":3000},
            "vat":true,"account":account}).to_string();
        assert!(same_billing_effect(&Value::Null,0,&bill(&old,"a"),&bill(&new,"a")).unwrap());
        assert!(!same_billing_effect(&Value::Null,0,&bill(&old,"a"),&bill(&new,"b")).unwrap());
    }
}

#[cfg(test)]
mod independent_stage_reconciliation_tests{
    use super::*;
    #[test]
    fn name_edit_is_not_a_physical_freight_change(){
        let order=json!({"clientId":"c"});
        let sender=json!({"name":"발송인"});
        let mut row=json!({"name":"수취인","address":"서울 주소","items":[
            {"id":"i","productId":"p","variantId":"v","quantity":1,"unitPrice":30000,"origin":"FACTORY"}
        ]});
        let before=signature(&order,&row,&sender);
        row["name"]=json!("수정된 수취인");
        assert!(same_freight_review_basis(&before,&signature(&order,&row,&sender)));
        row["items"][0]["quantity"]=json!(2);
        assert!(!same_freight_review_basis(&before,&signature(&order,&row,&sender)));
    }
    #[test]
    fn invalidating_review_only_keeps_proven_financial_approvals(){
        let mut state=json!({"workflow":{"shipments":[{"id":"s","groupId":"g","step":4}],
            "groups":[{"id":"g","needsNotice":false}],"events":[],
            "confirmations":[{"id":"confirmation","shipmentIds":["s"]}]}});
        let payload=json!({"now":"2026-11-26T10:00:00+09:00"});
        let mut audit=Audit{payload:&payload,position:0,scoped_seed:Some("independent")};
        downgrade(&mut state,&mut audit,0,0,"수취인 이름 변경",true,false).unwrap();
        assert_eq!(state["workflow"]["shipments"][0]["stageValidity"],json!([false,true,true,true]));
        assert_eq!(state["workflow"]["shipments"][0]["step"],0);
        assert_eq!(state["workflow"]["groups"][0]["needsNotice"],false);
        assert!(!rows(field(&state["workflow"],"events")).iter().any(|event|
            str_field(event,"action")=="입금 승인 재확인 필요"));
        stages::complete(&mut state["workflow"]["shipments"][0],1).unwrap();
        assert_eq!(state["workflow"]["shipments"][0]["step"],4);
        downgrade(&mut state,&mut audit,0,2,"금액 변경",false,false).unwrap();
        assert_eq!(state["workflow"]["shipments"][0]["stageValidity"],json!([true,true,false,false]));
        assert!(rows(field(&state["workflow"],"events")).iter().any(|event|
            str_field(event,"action")=="입금 승인 재확인 필요"));
    }
}

// Exercise the same reducer, quote and workflow projection used by intake.
#[cfg(test)]
mod reception_freight_reset_tests {
    use super::*;

    #[test]
    fn cleared_draft_freight_restores_registered_rate_and_survives_freeze(){
        let state=json!({
            "clients":[{"id":"c","name":"테스트 거래처","tier":"wholesale"}],
            "products":[{"id":"p","name":"테스트 상품","origin":"FACTORY",
                "variants":[{"id":"v","wholesale":45000}]}],
            "shipping":{"epoch":1,"productRules":{"p":{
                "version":"v1","fees":{"2":5200},"maxBundle":4,"maxConfirmed":true}}},
            "orders":[{"id":"o","clientId":"c","orderType":"AGENCY","applyVat":true,
                "recipients":[{"id":"r","name":"테스트 수취인",
                    "address":"서울특별시 중구 세종대로 1","dispatchOrigin":"FACTORY",
                    "freightCode":"010","shippingCharges":{"FACTORY":7000},
                    "items":[{"id":"i","name":"테스트 상품","productId":"p",
                        "variantId":"v","quantity":2,"origin":"FACTORY"}]}]}]
        });
        let project=|state:Value|reconcile(&json!({
            "state":state,"now":"2026-11-27T00:00:00+09:00",
            "eventIds":["synthetic-reset-1","synthetic-reset-2"]
        })).unwrap()["state"].clone();
        let mut state=project(state);
        assert_eq!(state["workflow"]["shipments"][0]["chargeMode"],"MANUAL");
        assert_eq!(state["workflow"]["shipments"][0]["charges"]["FACTORY"],7000);
        // A typed zero remains a manual amount, unlike a cleared field.
        state["orders"][0]["recipients"][0]["shippingCharges"]["FACTORY"]=json!(0);
        state=project(state);
        assert_eq!(state["workflow"]["shipments"][0]["chargeMode"],"MANUAL");
        assert_eq!(state["workflow"]["shipments"][0]["charges"]["FACTORY"],0);
        let model=super::super::execute("apply_manual_classification",json!({
            "action":"set_manual_delivery_option",
            "model":{"rows":state["orders"][0]["recipients"]},
            "cell":{"recipientId":"r","field":"shippingCharge"},
            "order":state["orders"][0],"value":null
        })).unwrap()["model"].clone();
        state["orders"][0]["recipients"]=model["rows"].clone();
        let quoted=super::super::execute("calculate_reception_quote",json!({
            "state":state,"client":state["clients"][0],"order":state["orders"][0]
        })).unwrap();
        assert_eq!(quoted["freightLines"][0]["appliedCharge"],5200);
        assert_eq!(quoted["totals"]["total"],104720);
        let automatic=project(state);
        assert_eq!(automatic["workflow"]["shipments"][0]["chargeMode"],"AUTO");
        assert_eq!(automatic["workflow"]["shipments"][0]["charges"]["FACTORY"],5200);
        // Missing tariff stays unresolved; never fabricate a zero.
        let mut missing=automatic.clone();
        missing["shipping"]["productRules"]["p"]["fees"]["2"]=Value::Null;
        let missing=project(missing);
        assert!(missing["workflow"]["shipments"][0]["charges"]["FACTORY"].is_null());
        // A later reconciliation must not erase the already frozen auto fee.
        let mut frozen=automatic;
        frozen["orders"][0]["confirmedAt"]=json!("2026-11-27T00:00:00+09:00");
        frozen["shipping"]["productRules"]["p"]["fees"]["2"]=json!(9000);
        let frozen=project(frozen);
        assert_eq!(frozen["workflow"]["shipments"][0]["chargeMode"],"AUTO");
        assert_eq!(frozen["workflow"]["shipments"][0]["charges"]["FACTORY"],5200);
    }
}
