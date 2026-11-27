//! Read-only WorkflowBoard projection from the SAME native Rust rules as React.
//! This is not a shadow workflow engine or an egui-owned order ledger.
//! Reads the v2 snapshot only when the user explicitly opted into that DB.
//! No SQLite writes, confirmation, stage transition, or invented work groups.
use crate::gateway::{BandoGateway, ReadOperation};
use serde_json::{json, Value};
use std::collections::{HashMap,HashSet};

#[derive(Clone, Debug)]
pub struct ShipmentRow {
    pub id: String,
    pub order_id: String,
    pub recipient_id: String,
    pub transferred: bool,
    pub recipient_name: String,
    pub completed_step: Option<u64>,
    pub paid: Option<i64>,
    pub products: Vec<String>,
    pub source_raw: String,
    pub recipient: Value,
    /// Exact Rust workflow preview decision (auto/review/staff/legacy), not a raw manual flag.
    pub remote: Option<Value>,
    /// Per-origin amounts after Rust locality multiplication, exactly as
    /// shown by the React workflow freight editor.
    pub display_charges: HashMap<String,Option<i64>>,
    pub freight_editing_blocked: bool,
    pub freight_exempt: bool,
    pub collect: bool,
    pub shipment_count: usize,
    pub order_total: Option<i64>,
}
#[derive(Clone, Debug)]
pub struct GroupRow {
    pub id: String,
    pub created_at: String,
    pub dealer_name: String,
    pub source_id: String,
    pub source_number: Option<u64>,
    pub dealer_id: String,
    pub transferred: bool,
    pub step: usize,
    pub stage_validity: [bool;4],
    pub needs_notice: bool,
    pub total: Option<i64>,
    pub vat_included: usize,
    pub vat_excluded: usize,
    pub shipments: Vec<ShipmentRow>,
    pub automatic_text: Option<String>,
    pub automatic_text_with_links: Option<String>,
    pub automatic_error: Option<String>,
    // Copy enablement follows the same Rust phase/payment issue response
    // as React; a rendered monetary string alone is not proof it is safe.
    pub automatic_phase: Option<String>,
    pub automatic_signature: Option<String>,
    pub payment_copied: bool,
    pub automatic_payment_issues: Vec<String>,
    pub issues: Vec<Value>,
}
impl GroupRow {
    pub fn message_text(&self,include_links:bool)->Option<&str>{
        if include_links{
            self.automatic_text_with_links.as_deref().or(self.automatic_text.as_deref())
        }else{self.automatic_text.as_deref()}
    }
    pub fn has_message_links(&self)->bool{
        self.automatic_text_with_links.as_deref().is_some_and(|linked|
            self.automatic_text.as_deref().is_some_and(|plain|linked!=plain))
    }
    pub fn management_target_label(&self)->eframe::egui::text::LayoutJob{
        let names=self.shipments.iter().map(|s|{
            if s.recipient_name.is_empty(){"수취인 확인"}else{s.recipient_name.as_str()}
        }).collect::<Vec<_>>().join(", ");
        crate::typography::inline_quantity_amount(
            &format!("{names} / 배송 "),self.shipments.len(),
            &format!("건 / {}/4 / ",self.step),self.total,"금액 미확정",false
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct BoardSnapshot {
    pub revision: Option<u64>,
    pub groups: Vec<GroupRow>,
}
/// Locate the evidence-owned shipment across ALL groups of the same order.
/// A recipient name is not an identity. If one recipient has multiple packages,
/// preserve the committed package when it is still a valid candidate.
pub fn find_recipient_shipment<'a>(
    snapshot:&'a BoardSnapshot,order_id:&str,recipient_id:&str,
    preferred_shipment_id:Option<&str>
)->Option<(&'a GroupRow,&'a ShipmentRow)>{
    let mut fallback=None;
    for group in &snapshot.groups{
        for shipment in &group.shipments{
            if shipment.order_id!=order_id || shipment.recipient_id!=recipient_id{
                continue;
            }
            if preferred_shipment_id==Some(shipment.id.as_str()){
                return Some((group,shipment));
            }
            if fallback.is_none(){
                fallback=Some((group,shipment));
            }
        }
    }
    fallback
}

fn rows(value:&Value)->&[Value]{
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn field<'a>(value:&'a Value,key:&str)->&'a Value{
    value.get(key).unwrap_or(&Value::Null)
}
fn text<'a>(value:&'a Value,key:&str)->&'a str{
    field(value,key).as_str().unwrap_or("")
}
fn active(value:&Value)->bool{
    field(value,"deletedAt").is_null()
}
fn id_row<'a>(array:&'a Value,id:&str)->Option<&'a Value>{
    rows(array).iter().find(|row|text(row,"id")==id)
}
fn company_name(value:&Value)->Option<String>{
    ["displayName","name"].iter()
        .map(|field_name|text(value,field_name).trim())
        .find(|name|!name.is_empty()).map(str::to_owned)
}
pub(crate) fn product_label(item:&Value)->String{
    let name=text(item,"name").trim();
    let raw=text(item,"raw").trim();
    let label=if !name.is_empty(){name}else if !raw.is_empty(){raw}else{"미확정 상품"};
    match field(item,"quantity").as_i64().filter(|n|*n>0){
        Some(quantity)=>format!("{label} × {quantity}"),
        None=>format!("{label} × ?"),
    }
}
/// Do not derive the displayed checkbox from the optional staff override.
/// The native workflow preview has already resolved the exact shipment state.
fn projected_remote(ship:&Value)->Option<Value>{
    ship.get("remoteDelivery").filter(|value|value.is_object()).cloned()
}

/// Native workflow issues use detail/title; message is retained only as a
/// compatibility fallback for older projections. Never silently drop a
/// valid structured issue because the UI expected the wrong field name.
pub fn workflow_issue_label(issue:&Value)->Option<String>{
    ["detail","title","message"].iter().find_map(|key|
        issue.get(*key).and_then(Value::as_str)
            .filter(|value|!value.trim().is_empty())
            .map(str::to_owned)
    ).or_else(||issue.as_str().map(str::to_owned))
}

fn recipient_with_line_amounts(recipient:&Value,preview:&Value)->Value{
    let mut display=recipient.clone();
    let recipient_id=text(recipient,"id");
    if let Some(items)=display.get_mut("items").and_then(Value::as_array_mut){
        for item in items{
            let amount=rows(field(preview,"lineAmounts")).iter().find(|line|
                text(line,"recipientId")==recipient_id &&
                text(line,"itemId")==text(item,"id")
            ).map(|line|field(line,"amount").clone()).unwrap_or(Value::Null);
            if let Some(object)=item.as_object_mut(){
                object.insert("lineAmount".into(),amount);
            }
        }
    }
    display
}
// Presentation-only grouping of the two native calculation-failure labels.
fn calculation_failure_label(label:&str)->bool{
    let label=label.trim().trim_end_matches('.');
    label=="청구액 미확정"||label.ends_with(": 청구액 미확정")||
        label=="입금 청구액 확인 필요"||label.ends_with(" 입금 청구액 확인 필요")
}
fn calculation_failure_is_covered(native:&Value,shipments:&[ShipmentRow],issues:&[Value])->bool{
    // Only suppress the generic card if every missing native total is
    // explained by a concrete actionable issue on that exact delivery.
    // Missing projection data, VAT/overflow failures and unknown causes
    // must keep the generic calculation card.
    if shipments.is_empty(){return false;}
    let mut missing=false;
    for ship in shipments{
        let Some(projected)=rows(field(native,"shipments")).iter()
            .find(|p|text(p,"shipmentId")==ship.id)else{return false;};
        let totals=field(projected,"totals");
        if field(totals,"total").as_i64().is_some(){continue;}
        if totals.get("total")!=Some(&Value::Null){return false;}
        missing=true;
        let subtotal_missing=totals.get("subtotal")==Some(&Value::Null);
        let shipping_missing=totals.get("shipping")==Some(&Value::Null);
        if !subtotal_missing&&!shipping_missing{return false;}
        let has=|kinds:&[&str]|issues.iter().any(|i|
            text(i,"severity")=="ACTION"&&text(i,"orderId")==ship.order_id&&
            text(i,"recipientId")==ship.recipient_id&&kinds.contains(&text(i,"kind")));
        if subtotal_missing&&!has(&["PRICE","ITEM","MODEL","MODEL_CHOICE","QUANTITY_INVALID","QUANTITY_MISSING","QUANTITY_LINK","ALIAS_CONFLICT","CATEGORY_CONFLICT"]){return false;}
        if shipping_missing&&!has(&["FREIGHT","ORIGIN"]){return false;}
        if totals.get("subtotal").is_none()||totals.get("shipping").is_none(){return false;}
    }
    missing
}
/// Keep native issue identity, severity and recipient routing. Stage blockers
/// are a second projection of the same validation, not additional cards.
fn workflow_review_entries(native:&Value,shipments:&[ShipmentRow])->Vec<Value>{
    let mut issues:Vec<Value>=Vec::new();
    let mut represented=HashSet::new();
    let mut ids=HashSet::new();
    for issue in rows(field(native,"issues")){
        let Some(label)=workflow_issue_label(issue) else{continue};
        let id=text(issue,"issueId");
        if !id.is_empty()&&!ids.insert(id.to_owned()){continue;}
        represented.insert(review_message_key(&label));
        let title=text(issue,"title");
        if !title.is_empty()&&text(issue,"severity")=="ACTION"{
            let rid=text(issue,"recipientId");
            let recipient=if rid.is_empty(){shipments.first()}else{
                shipments.iter().find(|s|s.recipient_id==rid &&
                    (text(issue,"orderId").is_empty()||s.order_id==text(issue,"orderId")))
            };
            if let Some(s)=recipient{
                let name=if s.recipient_name.is_empty(){"수취인"}else{&s.recipient_name};
                // A bare name is not an identity. Keep the original structured
                // cards, and only match recipient-scoped string aliases if
                // the name maps to exactly one recipient in this group.
                if rid.is_empty()||shipments.iter().filter(|row|
                    row.recipient_name==s.recipient_name).all(|row|
                    row.order_id==s.order_id&&row.recipient_id==s.recipient_id){
                    represented.insert(review_message_key(&format!("{name}: {title}")));
                }
            }
        }
        issues.push(issue.clone());
    }
    let mut calculation=Vec::new();
    for label in rows(field(native,"nextStageBlockers")).iter().filter_map(workflow_issue_label){
        if represented.insert(review_message_key(&label)){
            if calculation_failure_label(&label){calculation.push(label);}
            else{issues.push(json!({"detail":label,"severity":"ACTION","kind":"STAGE_BLOCKER"}));}
        }
    }
    if !calculation.is_empty(){
        if calculation_failure_is_covered(native,shipments,&issues){
            // Preserve aliases for extra-message deduplication without
            // altering native blockers or creating a second visible card.
            if let Some(issue)=issues.iter_mut().find(|i|text(i,"severity")=="ACTION"&&
                ["PRICE","ITEM","MODEL","MODEL_CHOICE","QUANTITY_INVALID","QUANTITY_MISSING",
                 "QUANTITY_LINK","ALIAS_CONFLICT","CATEGORY_CONFLICT","FREIGHT","ORIGIN"]
                .contains(&text(i,"kind"))){
                issue["reviewCoveredMessages"]=json!(calculation);
            }
        }else{
            let projected=rows(field(native,"shipments"));
            let targets=shipments.iter().filter(|ship|projected.iter().any(|p|
                text(p,"shipmentId")==ship.id&&field(field(p,"totals"),"total").is_null()))
                .map(|ship|json!({"shipmentId":ship.id,"orderId":ship.order_id,
                    "recipientId":ship.recipient_id,"name":ship.recipient_name})).collect::<Vec<_>>();
            issues.push(json!({"detail":calculation[0],"title":"청구액 계산 불가",
                "severity":"ACTION","kind":"CHARGE_CALCULATION",
                "reviewCoveredMessages":calculation,"calculationTargets":targets}));
        }
    }
    issues
}

/// Presentation-only equality for the native detail and legacy blocker
/// wording. Keep the recipient prefix; different recipients are not duplicates.
fn review_message_key(message:&str)->String{
    message.trim().trim_end_matches('.').trim_end()
        .replace(" / ",": ")
}
/// The structured issue card owns its action and severity. Additional string
/// errors from mutations and automatic message generation must not repeat it.
/// Do not alter stored errors, refresh admission or native validation.
pub fn workflow_review_extra_messages(group:&GroupRow,workflow_error:Option<&str>)->Vec<String>{
    let mut represented=HashSet::new();
    for issue in &group.issues{
        for label in rows(field(issue,"reviewCoveredMessages")).iter().filter_map(Value::as_str){
            represented.insert(review_message_key(label));
        }
        if let Some(label)=workflow_issue_label(issue){
            represented.insert(review_message_key(&label));
        }
        let title=text(issue,"title");
        if title.is_empty(){continue;}
        if text(issue,"recipientId").is_empty(){
            represented.insert(review_message_key(title));
            // Native stage messages prefix group-wide actions with its first
            // shipment name. Match that exact projection, not arbitrary names.
            if let Some(ship)=group.shipments.first(){
                let name=if ship.recipient_name.is_empty(){"수취인"}else{&ship.recipient_name};
                represented.insert(review_message_key(&format!("{name}: {title}")));
            }
            continue;
        }
        if let Some(ship)=group.shipments.iter().find(|s|
            s.recipient_id==text(issue,"recipientId") &&
            (text(issue,"orderId").is_empty() || s.order_id==text(issue,"orderId"))
        ){
            let name=if ship.recipient_name.is_empty(){"수취인"}else{&ship.recipient_name};
            represented.insert(review_message_key(&format!("{name}: {title}")));
        }
    }
    let mut extra=Vec::new();
    for raw in [workflow_error,group.automatic_error.as_deref()].into_iter().flatten(){
        let remaining=raw.lines().filter(|line|!line.trim().is_empty())
            .filter(|line|represented.insert(review_message_key(line)))
            .collect::<Vec<_>>();
        if !remaining.is_empty(){extra.push(remaining.join("\n"));}
    }
    extra
}

/// The persistent accent and the sequential focus use the same native
/// freight issue and the exact per-origin projected amount.
pub fn missing_freight_origin(group:&GroupRow,ship:&ShipmentRow,origin:&str)->bool{
    !ship.transferred && !ship.collect && !ship.freight_editing_blocked &&
        ["FACTORY","STORE"].contains(&origin) &&
        ship.display_charges.get(origin).copied().flatten().is_none() &&
        rows(field(&ship.recipient,"items")).iter().any(|item|text(item,"origin")==origin) &&
        group.issues.iter().any(|issue|text(issue,"kind")=="FREIGHT" &&
            text(issue,"severity")=="ACTION" && text(issue,"recipientId")==ship.recipient_id &&
            text(issue,"orderId")==ship.order_id)
}
/// Resolve only an actual editable, missing per-origin charge. Never route
/// by the display name or an unrelated currently visible shipment.
pub fn missing_freight_target<'a>(group:&'a GroupRow,issue:&Value,preferred:&str)
    ->Option<(&'a ShipmentRow,String)>{
    if text(issue,"kind")!="FREIGHT"{return None;}
    let rid=text(issue,"recipientId");
    let oid=text(issue,"orderId");
    if rid.is_empty()||oid.is_empty(){return None;}
    let target=|ship:&'a ShipmentRow|{
        if ship.recipient_id!=rid||ship.order_id!=oid||ship.transferred||
            ship.collect||ship.freight_editing_blocked{return None;}
        rows(field(&ship.recipient,"items")).iter().filter_map(|item|{
            let origin=text(item,"origin");
            (["FACTORY","STORE"].contains(&origin)&&
                ship.display_charges.get(origin).copied().flatten().is_none())
                .then(||origin.to_owned())
        }).next().map(|origin|(ship,origin))
    };
    group.shipments.iter().find(|s|s.id==preferred).and_then(target)
        .or_else(||group.shipments.iter().find_map(target))
}

/// Use the displayed shipment order, then the same item-origin order as
/// the freight input rows. Only native FREIGHT issues with an editable
/// missing charge are eligible; do not invent a new validation rule.
pub fn first_missing_freight(group:&GroupRow)->Option<(&ShipmentRow,String)>{
    for ship in &group.shipments{
        for issue in &group.issues{
            if text(issue,"kind")!="FREIGHT" ||
                text(issue,"orderId")!=ship.order_id ||
                text(issue,"recipientId")!=ship.recipient_id{continue;}
            if let Some((target,origin))=missing_freight_target(group,issue,&ship.id){
                if target.id==ship.id{return Some((target,origin));}
            }
        }
    }
    None
}

/// React's workflow-view::groupShipments is a read-only join.
/// Business prices, freight code, VAT and stage are ALWAYS Rust projections.
pub fn load(gateway:&dyn BandoGateway)->Result<BoardSnapshot,String>{
    let state=gateway.load_business_state()?;
    from_state(gateway,state)
}

/// Project the exact snapshot returned by an authoritative, verified write.
/// Periodic reads still go through load() to detect external changes.
pub fn from_state(gateway:&dyn BandoGateway,mut state:Value)->Result<BoardSnapshot,String>{
    gateway.prepare_workflow_preview(&mut state)?;
    project_state(gateway,&state,None)
}

/// Native freight edits and group moves reconcile exactly one source order.
/// Only reuse the immediately preceding board with a provable source/order
/// join. Include new and emptied groups; otherwise use full projection.
fn freight_refresh_scope(previous:&BoardSnapshot,state:&Value,group_id:&str)
    ->Option<HashSet<String>>{
    if previous.revision?.checked_add(1)!=state["revision"].as_u64(){return None;}
    let old=previous.groups.iter().find(|group|group.id==group_id)?;
    let order_id=old.shipments.first()?.order_id.as_str();
    if order_id.is_empty() || old.source_id.is_empty() ||
        old.shipments.iter().any(|ship|ship.order_id!=order_id){return None;}
    let workflow=field(state,"workflow");
    let source=id_row(field(workflow,"sources"),&old.source_id)?;
    if text(source,"orderId")!=order_id{return None;}
    let groups=rows(field(workflow,"groups"));
    let ships=rows(field(workflow,"shipments"));
    let mut scope=HashSet::new();
    for group in groups{
        let id=text(group,"id");
        let same_source=text(group,"sourceId")==old.source_id;
        let same_order=ships.iter().any(|ship|text(ship,"groupId")==id &&
            text(ship,"orderId")==order_id);
        if same_order && !same_source{return None;}
        if same_source{
            if id.is_empty() || ships.iter().any(|ship|text(ship,"groupId")==id &&
                text(ship,"orderId")!=order_id){return None;}
            scope.insert(id.to_owned());
        }
    }
    for group in &previous.groups{
        if group.source_id==old.source_id ||
            group.shipments.iter().any(|ship|ship.order_id==order_id){
            scope.insert(group.id.clone());
        }
    }
    if !scope.contains(group_id){return None;}
    Some(scope)
}

pub fn after_freight_edit(gateway:&dyn BandoGateway,mut state:Value,
    previous:Option<BoardSnapshot>,group_id:&str)->Result<BoardSnapshot,String>{
    if gateway.supports_scoped_freight_refresh(){
        if let Some(mut board)=previous{
            if let Some(scope)=freight_refresh_scope(&board,&state,group_id){
                gateway.prepare_workflow_preview(&mut state)?;
                let updated=match project_state(gateway,&state,Some(&scope)){
                    Ok(updated)=>updated,
                    Err(_)=>return from_state(gateway,state),
                };
                board.groups.retain(|group|!scope.contains(&group.id));
                board.groups.extend(updated.groups);
                // Match the full projection's stable tie order even when
                // a group has been removed or created by reconciliation.
                let positions:HashMap<&str,usize>=rows(field(field(&state,"workflow"),"groups"))
                    .iter().enumerate().map(|(i,g)|(text(g,"id"),i)).collect();
                board.groups.sort_by(|a,b|b.created_at.cmp(&a.created_at)
                    .then_with(||positions.get(a.id.as_str()).cmp(&positions.get(b.id.as_str()))));
                board.revision=updated.revision;
                return Ok(board);
            }
        }
    }
    from_state(gateway,state)
}

fn project_state(gateway:&dyn BandoGateway,state:&Value,scope:Option<&HashSet<String>>)
    ->Result<BoardSnapshot,String>{
    let started=std::time::Instant::now();
    let mut request=json!({"state":state});
    if let Some(scope)=scope{
        request["groupIds"]=json!(scope.iter().collect::<Vec<_>>());
    }
    let projected=gateway.execute(ReadOperation::PreviewWorkflow,request.clone())?;
    let shipment_read=gateway.execute(ReadOperation::ReadShipments,request)?;
    let workflow=field(&state,"workflow");
    let orders=field(&state,"orders");
    let all_shipments=field(workflow,"shipments");
    let mut groups=Vec::new();
    let mut order_previews:HashMap<String,Value>=HashMap::new();
    for group in rows(field(workflow,"groups")).iter().filter(|g|active(g)){
        let group_id=text(group,"id");
        if group_id.is_empty() || scope.is_some_and(|ids|!ids.contains(group_id)){continue;}
        let ships=rows(all_shipments).iter().filter(|ship|
            text(ship,"groupId")==group_id &&
            field(ship,"legacyArchived")!=&Value::Bool(true)
        ).filter(|ship|id_row(orders,text(ship,"orderId"))
            .is_some_and(active)).collect::<Vec<_>>();
        // Match React: show only groups with a shipment yet to transfer.
        if ships.is_empty() || ships.iter().all(|s|
            field(s,"transferred")==&Value::Bool(true)){continue;}
        let native=rows(field(&projected,"groups")).iter()
            .find(|row|text(row,"groupId")==group_id)
            .ok_or_else(||format!("Rust 업무 미리보기에서 발주묶음 {group_id}을 찾을 수 없습니다."))?;
        let first=ships[0];
        let order=id_row(orders,text(first,"orderId"))
            .ok_or("배송 건의 발주서 연결이 끊겼습니다.")?;
        let client=rows(field(&shipment_read,"shipments")).iter()
            .find(|row|text(row,"shipmentId")==text(first,"id"))
            .map(|row|field(row,"client")).filter(|v|!v.is_null())
            .or_else(||id_row(field(&state,"clients"),text(order,"clientId")));
        let dealer_name=client.and_then(company_name)
            .unwrap_or_else(||"거래처 확인 필요".to_owned());
        let mut display_shipments=Vec::with_capacity(ships.len());
        for ship in ships{
            let order=id_row(orders,text(ship,"orderId"))
                .ok_or("배송 건의 발주서 연결이 끊겼습니다.")?;
            let recipient=id_row(field(order,"recipients"),text(ship,"recipientId"))
                .ok_or("배송 건의 수취인 연결이 끊겼습니다.")?;
            let native_ship=rows(field(native,"shipments")).iter()
                .find(|row|text(row,"shipmentId")==text(ship,"id"))
                .ok_or("Rust 업무 미리보기에 배송 금액이 없습니다.")?;
            let products=rows(field(recipient,"items")).iter()
                .map(product_label).collect();
            let order_id=text(order,"id").to_owned();
            if !order_previews.contains_key(&order_id){
                let preview=gateway.execute(ReadOperation::PreviewOrder,json!({
                    "state":&state,"orderId":order_id,
                }))?;
                order_previews.insert(order_id.clone(),preview);
            }
            let preview=&order_previews[&order_id];
            let order_total=field(preview,"total").as_i64();
            // Line amounts belong to the Rust preview, not persisted item fields.
            // Attach them only to the read-only presentation copy.
            let mut display_recipient=recipient_with_line_amounts(recipient,preview);
            // Preserve the actual effective VAT choice in the presentation
            // projection even for legacy rows inheriting order.applyVat.
            display_recipient["vatOverride"]=field(native_ship,"vatIncluded").clone();
            display_shipments.push(ShipmentRow{
                id:text(ship,"id").to_owned(),
                order_id:text(ship,"orderId").to_owned(),
                recipient_id:text(ship,"recipientId").to_owned(),
                transferred:field(ship,"transferred")==&Value::Bool(true),
                recipient_name:text(recipient,"name").to_owned(),
                completed_step:field(ship,"step").as_u64(),
                paid:field(native_ship,"paid").as_i64(),
                products,
                source_raw:text(order,"raw").to_owned(),
                recipient:display_recipient,
                remote:projected_remote(native_ship),
                display_charges:field(native_ship,"displayCharges").as_object()
                    .map(|values|values.iter().map(|(origin,value)|
                        (origin.clone(),value.as_i64())).collect())
                    .unwrap_or_default(),
                freight_exempt:field(native_ship,"billingReasons").as_object()
                    .is_some_and(|reasons|reasons.values().any(|v|v.as_str()==Some("EXEMPT"))),
                freight_editing_blocked:field(native_ship,"freightEditingBlocked")
                    .as_bool().unwrap_or(false),
                collect:text(native_ship,"freightCode")=="020",
                shipment_count: rows(all_shipments).iter().filter(|other|
                    text(other, "orderId") == order_id.as_str() &&
                    field(other, "legacyArchived") != &Value::Bool(true)
                ).count(),
                order_total,
            });
        }
        let vat_included=rows(field(native,"shipments")).iter()
            .filter(|ship|field(ship,"vatIncluded")==&Value::Bool(true)).count();
        let vat_excluded=rows(field(native,"shipments")).len()-vat_included;
        let step=field(native,"step").as_u64().unwrap_or(0).min(4) as usize;
        let wire=rows(field(field(&state,"settings"),"accounts")).iter()
            .map(|account|Ok(json!({
                "id":text(account,"id"),
                "json":serde_json::to_string(account).map_err(|e|e.to_string())?
            })))
            .collect::<Result<Vec<Value>,String>>()?;
        let automatic=gateway.render_automatic_preview(json!({
            "state":&state,"groupId":group_id,"includeLinks":false,
            "accountWire":&wire,"includeCopyStatus":true,
        }));
        // One native rendering supplies both the signature and its copy
        // evidence. Older adapters may omit copied; fall back safely.
        let payment_copied=match automatic.as_ref().ok()
            .and_then(|result|result["copied"].as_bool()){
            Some(copied)=>copied,
            None=>gateway.execute(ReadOperation::RenderAutomaticMessage,json!({
                "state":&state,"groupId":group_id,"accountWire":&wire,
                "mode":"payment_copy_status"
            })).ok().is_some_and(|result|result["copied"]==true),
        };
        let (automatic_text,automatic_text_with_links,automatic_error,automatic_phase,automatic_payment_issues,automatic_signature)=
            match automatic{
                Ok(result)=>(
                    result.get("text").and_then(Value::as_str).map(str::to_owned),
                    result.get("textWithLinks").and_then(Value::as_str).map(str::to_owned),
                    None,
                    result.get("phase").and_then(Value::as_str).map(str::to_owned),
                    rows(field(&result,"paymentIssues")).iter()
                        .filter_map(Value::as_str).map(str::to_owned).collect(),
                    result.get("signature").and_then(Value::as_str).map(str::to_owned)
                ),
                Err(error)=>(None,None,Some(error),None,Vec::new(),None),
            };
        let issues=workflow_review_entries(native,&display_shipments);
        groups.push(GroupRow{
            id:group_id.to_owned(),
            created_at:text(group,"createdAt").to_owned(),
            dealer_name,
            source_id:text(group,"sourceId").to_owned(),
            source_number:id_row(field(workflow,"sources"),text(group,"sourceId"))
                .and_then(|source|field(source,"number").as_u64()),
            dealer_id:text(group,"dealerId").to_owned(),
            transferred:rows(all_shipments).iter().any(|ship|
                text(ship,"groupId")==group_id &&
                field(ship,"transferred")==&Value::Bool(true)),
            step,
            stage_validity:std::array::from_fn(|i|
                native["stageValidity"].get(i).and_then(Value::as_bool).unwrap_or(i<step)),
            needs_notice:field(native,"needsNotice")==&Value::Bool(true),
            total:field(field(native,"totals"),"total").as_i64(),
            vat_included,vat_excluded,
            shipments:display_shipments,
            automatic_text,automatic_text_with_links,automatic_error,automatic_phase,
            automatic_payment_issues,automatic_signature,payment_copied,issues,
        });
    }
    groups.sort_by(|a,b|b.created_at.cmp(&a.created_at));
    if std::env::var_os("BANDO_EGUI_PROFILE_WORKFLOW").is_some(){
        eprintln!("[workflow-perf] phase=board_projection ms={:.3} groups={} scoped={}",
            started.elapsed().as_secs_f64()*1000.0,groups.len(),scope.is_some());
    }
    Ok(BoardSnapshot{
        revision:state["revision"].as_u64(),groups
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn formatted_workflow_rows_are_not_prices(){
        let row=json!({"name":"708","quantity":3});
        assert_eq!(product_label(&row),"708 × 3");
        let unknown=json!({"raw":"709-1"});
        assert_eq!(product_label(&unknown),"709-1 × ?");
    }
    #[test]
    fn native_remote_preview_is_not_replaced_by_optional_manual_checkbox(){
        let auto=json!({"remoteDelivery":{"status":"auto","checked":true,
            "matchedPath":"제주특별자치도","address":"제주특별자치도 제주시 1"}});
        let decision=projected_remote(&auto).expect("native automatic result");
        assert_eq!(decision["checked"],true);
        assert!(crate::reception_service::remote_delivery_notice(&decision)
            .unwrap().contains("운임 2배"));
        let manual=json!({"remoteDelivery":{"status":"auto","checked":false,
            "manual":true,"address":"제주특별자치도 제주시 1"}});
        assert_eq!(projected_remote(&manual).unwrap()["checked"],false);
        let review=json!({"remoteDelivery":{"status":"review","checked":false,
            "matchedPath":"군산시 옥도면"}});
        assert_eq!(projected_remote(&review).unwrap()["status"],"review");
        let legacy=json!({"remoteDelivery":{"status":"auto","checked":false,
            "legacy":true}});
        assert_eq!(projected_remote(&legacy).unwrap()["legacy"],true);
        assert!(projected_remote(&json!({"shipmentId":"ordinary"})).is_none());
    }
}

// The source and contact panes must resolve exactly the same shipment as
// the workflow board, even when a single order spans multiple groups.
#[cfg(test)]
mod linked_recipient_selection_tests{
    use super::*;
    fn shipment(id:&str,order:&str,recipient:&str)->ShipmentRow{
        ShipmentRow{
            id:id.into(),order_id:order.into(),recipient_id:recipient.into(),
            transferred:false,recipient_name:"동명이인".into(),products:vec![],
            completed_step:Some(0),paid:None,
            source_raw:"공통 발주원문".into(),recipient:json!({"id":recipient}),
            remote:None,display_charges:HashMap::new(),
            freight_editing_blocked:false,freight_exempt:false,collect:false,
            shipment_count:1,order_total:None,
        }
    }
    fn group(id:&str,shipments:Vec<ShipmentRow>)->GroupRow{
        GroupRow{
            id:id.into(),created_at:String::new(),dealer_name:String::new(),
            source_id:String::new(),source_number:None,dealer_id:String::new(),transferred:false,
            step:0,stage_validity:[false;4],needs_notice:false,total:None,vat_included:0,vat_excluded:0,
            shipments,automatic_text:None,automatic_text_with_links:None,automatic_error:None,
            automatic_phase:None,automatic_signature:None,payment_copied:false,
            automatic_payment_issues:vec![],issues:vec![],
        }
    }
    #[test]
    fn freight_refresh_includes_split_groups_and_rejects_stale_or_cross_order_scope(){
        let mut first=group("a",vec![shipment("s1","o1","r1")]);
        first.source_id="src1".into();
        let mut second=group("b",vec![shipment("s2","o1","r2")]);
        second.source_id="src1".into();
        let mut other=group("c",vec![shipment("s3","o2","r3")]);
        other.source_id="src2".into();
        let previous=BoardSnapshot{revision:Some(7),groups:vec![first,second,other]};
        let mut state=json!({"revision":8,"workflow":{
            "sources":[{"id":"src1","orderId":"o1"},{"id":"src2","orderId":"o2"}],
            "groups":[{"id":"a","sourceId":"src1"},{"id":"b","sourceId":"src1"},
                {"id":"c","sourceId":"src2"},{"id":"new","sourceId":"src1"}],
            "shipments":[{"id":"s1","groupId":"a","orderId":"o1"},
                {"id":"s2","groupId":"b","orderId":"o1"},
                {"id":"s3","groupId":"c","orderId":"o2"}]
        }});
        let scope=freight_refresh_scope(&previous,&state,"a").unwrap();
        assert_eq!(scope,["a","b","new"].into_iter().map(str::to_owned).collect::<HashSet<_>>());
        // A newly detached card joins the scope; the emptied original
        // card must still be removed when its last shipment is regrouped.
        state["workflow"]["shipments"][0]["groupId"]=json!("new");
        assert_eq!(freight_refresh_scope(&previous,&state,"a").unwrap(),scope);
        state["workflow"]["shipments"][0]["groupId"]=json!("b");
        assert_eq!(freight_refresh_scope(&previous,&state,"a").unwrap(),scope);
        state["revision"]=json!(9);
        assert!(freight_refresh_scope(&previous,&state,"a").is_none());
        state["revision"]=json!(8);
        state["workflow"]["shipments"][2]["groupId"]=json!("a");
        assert!(freight_refresh_scope(&previous,&state,"a").is_none());
    }
    #[test]
    fn workflow_review_keeps_next_stage_blockers_and_native_line_amounts(){
        let native=json!({"issues":[],"nextStageBlockers":["수취인: 청구액 미확정"]});
        let issues=workflow_review_entries(&native,&[]);
        assert_eq!(issues.len(),1);
        assert_eq!(issues[0]["detail"],"수취인: 청구액 미확정");
        assert_eq!(issues[0]["kind"],"CHARGE_CALCULATION");
        assert_eq!(issues[0]["severity"],"ACTION");
        let recipient=json!({"id":"r","items":[{"id":"i","quantity":3}]});
        let preview=json!({"lineAmounts":[
            {"recipientId":"other","itemId":"i","amount":999},
            {"recipientId":"r","itemId":"i","amount":9000}
        ]});
        let projected=recipient_with_line_amounts(&recipient,&preview);
        assert_eq!(projected["items"][0]["lineAmount"],9000);
        assert!(recipient["items"][0].get("lineAmount").is_none());
    }
    #[test]
    fn calculation_failures_merge_and_only_hide_when_native_totals_explain_every_target(){
        let ships=vec![shipment("s1","o1","r1"),shipment("s2","o1","r2")];
        let freight=json!({"issueId":"f","kind":"FREIGHT","severity":"ACTION",
            "orderId":"o1","recipientId":"r1","title":"배송 운임을 확인하세요"});
        let mut native=json!({"issues":[freight],"nextStageBlockers":[
            "동명이인: 청구액 미확정","가상 거래처 입금 청구액 확인 필요",
            "동명이인: 운임 적용 필요","가상 거래처 입금 청구 9,000원"],
            "shipments":[{"shipmentId":"s1","totals":{"subtotal":100,"shipping":null,"total":null}},
                {"shipmentId":"s2","totals":{"subtotal":100,"shipping":0,"total":100}}]});
        let covered=workflow_review_entries(&native,&ships);
        assert_eq!(covered.iter().filter(|i|i["kind"]=="CHARGE_CALCULATION").count(),0);
        assert_eq!(covered[0]["reviewCoveredMessages"].as_array().unwrap().len(),2);
        assert_eq!(covered.iter().filter(|i|i["kind"]=="STAGE_BLOCKER").count(),2);
        let mut group=group("g",ships.clone());
        group.issues=covered;
        assert!(workflow_review_extra_messages(&group,Some("가상 거래처 입금 청구액 확인 필요")).is_empty());
        native["shipments"][1]["totals"]["total"]=Value::Null;
        let uncovered=workflow_review_entries(&native,&ships);
        let calculation=uncovered.iter().filter(|i|i["kind"]=="CHARGE_CALCULATION").collect::<Vec<_>>();
        assert_eq!(calculation.len(),1);
        assert_eq!(calculation[0]["calculationTargets"].as_array().unwrap().len(),2);
        assert_eq!(calculation[0]["reviewCoveredMessages"].as_array().unwrap().len(),2);
        native["shipments"][0]["totals"]=json!({"subtotal":100,"shipping":0,"total":100});
        native["shipments"][1]["totals"]=json!({"subtotal":100,"shipping":0,"total":100});
        assert!(workflow_review_entries(&native,&ships).iter().any(|i|i["kind"]=="CHARGE_CALCULATION"));
    }
    #[test]
    fn freight_issue_deduplicates_stage_blocker_and_routes_exact_recipient(){
        let mut first=shipment("s1","o1","r1");
        first.recipient=json!({"items":[{"origin":"STORE"}]});
        first.display_charges.insert("STORE".into(),Some(2500));
        let mut second=shipment("s2","o1","r2");
        second.recipient=json!({"items":[{"origin":"FACTORY"}]});
        second.display_charges.insert("FACTORY".into(),None);
        let group=group("g",vec![first,second]);
        let issue=json!({"issueId":"freight-r2","kind":"FREIGHT","severity":"ACTION",
            "recipientId":"r2","orderId":"o1","title":"배송 운임을 확인하세요",
            "detail":"동명이인 / 배송 운임을 확인하세요"});
        let native=json!({"issues":[issue.clone()],"nextStageBlockers":[
            "동명이인: 배송 운임을 확인하세요","동명이인: 입금 부족"]});
        let entries=workflow_review_entries(&native,&group.shipments);
        assert_eq!(entries.len(),2);
        assert_eq!(entries[0],issue);
        let (ship,origin)=missing_freight_target(&group,&entries[0],"s1").unwrap();
        assert_eq!(ship.id,"s2");
        assert_eq!(origin,"FACTORY");
        let mut settled=group.clone();
        settled.shipments[1].display_charges.insert("FACTORY".into(),Some(3000));
        assert!(missing_freight_target(&settled,&entries[0],"s1").is_none());
        settled.shipments[1].display_charges.insert("FACTORY".into(),None);
        settled.shipments[1].collect=true;
        assert!(missing_freight_target(&settled,&entries[0],"s1").is_none());
    }
    #[test]
    fn every_missing_origin_is_accented_until_its_own_charge_is_resolved(){
        let mut first=shipment("s1","o1","r1");
        first.recipient=json!({"items":[{"origin":"STORE"},{"origin":"FACTORY"}]});
        let mut g=group("g",vec![first]);
        g.issues=vec![json!({"kind":"FREIGHT","severity":"ACTION","orderId":"o1","recipientId":"r1"})];
        assert!(missing_freight_origin(&g,&g.shipments[0],"STORE"));
        assert!(missing_freight_origin(&g,&g.shipments[0],"FACTORY"));
        g.shipments[0].display_charges.insert("STORE".into(),Some(0));
        assert!(!missing_freight_origin(&g,&g.shipments[0],"STORE"));
        assert!(missing_freight_origin(&g,&g.shipments[0],"FACTORY"));
        g.shipments[0].collect=true;
        assert!(!missing_freight_origin(&g,&g.shipments[0],"FACTORY"));
    }
    #[test]
    fn freight_guidance_follows_display_order_and_skips_completed_origins(){
        let mut first=shipment("s1","o1","r1");
        first.recipient=json!({"items":[{"origin":"STORE"},{"origin":"FACTORY"}]});
        let mut second=shipment("s2","o1","r2");
        second.recipient=json!({"items":[{"origin":"FACTORY"}]});
        let mut g=group("g",vec![first,second]);
        g.issues=vec![
            json!({"kind":"FREIGHT","orderId":"o1","recipientId":"r2"}),
            json!({"kind":"FREIGHT","orderId":"o1","recipientId":"r1"})
        ];
        let next=|group:&GroupRow|first_missing_freight(group)
            .map(|(ship,origin)|(ship.id.clone(),origin));
        assert_eq!(next(&g),Some(("s1".into(),"STORE".into())));
        g.shipments[0].display_charges.insert("STORE".into(),Some(0));
        assert_eq!(next(&g),Some(("s1".into(),"FACTORY".into())));
        g.shipments[0].display_charges.insert("FACTORY".into(),Some(3000));
        assert_eq!(next(&g),Some(("s2".into(),"FACTORY".into())));
        g.shipments[1].collect=true;
        assert_eq!(next(&g),None);
    }
    #[test]
    fn action_card_owns_duplicate_mutation_and_automatic_message_errors(){
        let mut group=group("g",vec![shipment("s1","o1","r1")]);
        group.issues=vec![json!({"issueId":"freight-r1","kind":"FREIGHT","severity":"ACTION",
            "recipientId":"r1","orderId":"o1","title":"배송 운임을 확인하세요",
            "detail":"동명이인 / 배송 운임을 확인하세요"})];
        group.automatic_error=Some("동명이인: 배송 운임을 확인하세요.".into());
        assert!(workflow_review_extra_messages(&group,
            Some("동명이인: 배송 운임을 확인하세요")).is_empty());
        assert_eq!(workflow_review_extra_messages(&group,
            Some("동명이인: 배송 운임을 확인하세요\n다른 수취인: 배송 운임을 확인하세요")),
            vec!["다른 수취인: 배송 운임을 확인하세요".to_owned()]);
        group.automatic_error=Some("주문 상태를 확인하세요.".into());
        assert_eq!(workflow_review_extra_messages(&group,Some("주문 상태를 확인하세요")),
            vec!["주문 상태를 확인하세요".to_owned()]);
        group.issues.clear();
        assert_eq!(workflow_review_extra_messages(&group,
            Some("동명이인: 배송 운임을 확인하세요")).len(),2);
    }
    #[test]
    fn stage_and_group_wide_errors_do_not_duplicate_action_cards(){
        let mut group=group("g",vec![shipment("s1","o1","r1")]);
        let issue=json!({"issueId":"account","kind":"ACCOUNT","severity":"ACTION",
            "title":"입금 계좌를 등록 / 선택하세요","detail":"입금 계좌를 등록 / 선택하세요",
            "recipientId":""});
        group.issues=workflow_review_entries(&json!({"issues":[issue],
            "nextStageBlockers":["동명이인: 입금 계좌를 등록 / 선택하세요."]}),&group.shipments);
        assert_eq!(group.issues.len(),1);
        group.automatic_error=Some("동명이인: 입금 계좌를 등록 / 선택하세요.".into());
        assert!(workflow_review_extra_messages(&group,None).is_empty());
        assert_eq!(workflow_review_extra_messages(&group,
            Some("다른 수취인: 입금 계좌를 등록 / 선택하세요.")),vec!["다른 수취인: 입금 계좌를 등록 / 선택하세요.".to_owned()]);
    }
    #[test]
    fn structured_workflow_issues_render_their_detail_and_title(){
        assert_eq!(workflow_issue_label(&json!({
            "kind":"FREIGHT","title":"배송 운임을 확인하세요",
            "detail":"김하영 / 배송 운임을 확인하세요"
        })).as_deref(),Some("김하영 / 배송 운임을 확인하세요"));
        assert_eq!(workflow_issue_label(&json!({
            "kind":"FREIGHT","title":"배송 운임을 확인하세요"
        })).as_deref(),Some("배송 운임을 확인하세요"));
        assert_eq!(workflow_issue_label(&json!({
            "message":"이전 형식 오류"
        })).as_deref(),Some("이전 형식 오류"));
    }

    #[test]
    fn finds_recipient_in_another_group_without_matching_duplicate_names(){
        let board=BoardSnapshot{
            revision:None,
            groups:vec![
                group("g1",vec![shipment("s1","order1","r1")]),
                group("g2",vec![
                    shipment("s2","order1","r2"),
                    shipment("s3","order2","r2"),
                ]),
            ],
        };
        let (found_group,found_ship)=find_recipient_shipment(
            &board,"order1","r2",Some("s1")
        ).expect("cross-group recipient");
        assert_eq!((found_group.id.as_str(),found_ship.id.as_str()),("g2","s2"));
        assert!(find_recipient_shipment(&board,"order1","r3",None).is_none());
        assert!(find_recipient_shipment(&board,"order2","r1",None).is_none());
    }
    #[test]
    fn preserves_clicked_package_for_recipient_with_multiple_shipments(){
        let board=BoardSnapshot{
            revision:None,groups:vec![
                group("g1",vec![shipment("s1","order1","r1")]),
                group("g2",vec![shipment("s2","order1","r1")]),
            ],
        };
        let (found_group,found_ship)=find_recipient_shipment(
            &board,"order1","r1",Some("s2")
        ).expect("committed shipment");
        assert_eq!((found_group.id.as_str(),found_ship.id.as_str()),("g2","s2"));
        assert_eq!(find_recipient_shipment(&board,"order1","r1",Some("gone"))
            .unwrap().1.id,"s1");
    }
}
