//! Read-only WorkflowBoard projection from the SAME native Rust rules as React.
//! This is not a shadow workflow engine or an egui-owned order ledger.
//! Reads the v2 snapshot only when the user explicitly opted into that DB.
//! No SQLite writes, confirmation, stage transition, or invented work groups.
use crate::gateway::{BandoGateway, ReadOperation};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ShipmentRow {
    pub id: String,
    pub order_id: String,
    pub recipient_id: String,
    pub transferred: bool,
    pub recipient_name: String,
    pub products: Vec<String>,
    pub source_raw: String,
    pub recipient: Value,
    /// Exact Rust workflow preview decision (auto/review/staff/legacy), not a raw manual flag.
    pub remote: Option<Value>,
    /// Per-origin amounts after Rust locality multiplication, exactly as
    /// shown by the React workflow freight editor.
    pub display_charges: HashMap<String,Option<i64>>,
    pub freight_editing_blocked: bool,
    pub freight_code: String,
    pub freight: Option<i64>,
    pub collect: bool,
    pub vat_included: Option<bool>,
    pub shipment_count: usize,
    pub order_total: Option<i64>,
}
#[derive(Clone, Debug)]
pub struct GroupRow {
    pub id: String,
    pub created_at: String,
    pub dealer_name: String,
    pub source_id: String,
    pub dealer_id: String,
    pub transferred: bool,
    pub step: usize,
    pub needs_notice: bool,
    pub total: Option<i64>,
    pub vat_included: usize,
    pub vat_excluded: usize,
    pub shipments: Vec<ShipmentRow>,
    pub automatic_text: Option<String>,
    pub automatic_error: Option<String>,
    // Copy enablement follows the same Rust phase/payment issue response
    // as React; a rendered monetary string alone is not proof it is safe.
    pub automatic_phase: Option<String>,
    pub automatic_payment_issues: Vec<String>,
    pub issues: Vec<String>,
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
fn product_label(item:&Value)->String{
    let name=text(item,"name").trim();
    let raw=text(item,"raw").trim();
    let label=if name.is_empty(){raw}else{name};
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
fn workflow_issue_label(issue:&Value)->Option<String>{
    ["detail","title","message"].iter().find_map(|key|
        issue.get(*key).and_then(Value::as_str)
            .filter(|value|!value.trim().is_empty())
            .map(str::to_owned)
    ).or_else(||issue.as_str().map(str::to_owned))
}

/// React's workflow-view::groupShipments is a read-only join.
/// Business prices, freight code, VAT and stage are ALWAYS Rust projections.
pub fn load(gateway:&dyn BandoGateway)->Result<BoardSnapshot,String>{
    let state=gateway.load_business_state()?;
    let projected=gateway.execute(ReadOperation::PreviewWorkflow,json!({"state":&state}))?;
    let shipment_read=gateway.execute(ReadOperation::ReadShipments,json!({"state":&state}))?;
    let workflow=field(&state,"workflow");
    let orders=field(&state,"orders");
    let all_shipments=field(workflow,"shipments");
    let mut groups=Vec::new();
    let mut order_totals:HashMap<String,Option<i64>>=HashMap::new();
    for group in rows(field(workflow,"groups")).iter().filter(|g|active(g)){
        let group_id=text(group,"id");
        if group_id.is_empty(){continue;}
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
            let order_total=if let Some(cached)=order_totals.get(&order_id){
                *cached
            }else{
                let preview=gateway.execute(ReadOperation::PreviewOrder,json!({
                    "state":&state,"orderId":order_id,
                }))?;
                let total=field(&preview,"total").as_i64();
                order_totals.insert(order_id.clone(),total);
                total
            };
            display_shipments.push(ShipmentRow{
                id:text(ship,"id").to_owned(),
                order_id:text(ship,"orderId").to_owned(),
                recipient_id:text(ship,"recipientId").to_owned(),
                transferred:field(ship,"transferred")==&Value::Bool(true),
                recipient_name:text(recipient,"name").to_owned(),
                products,
                source_raw:text(order,"raw").to_owned(),
                recipient:recipient.clone(),
                remote:projected_remote(native_ship),
                display_charges:field(native_ship,"displayCharges").as_object()
                    .map(|values|values.iter().map(|(origin,value)|
                        (origin.clone(),value.as_i64())).collect())
                    .unwrap_or_default(),
                freight_editing_blocked:field(native_ship,"freightEditingBlocked")
                    .as_bool().unwrap_or(false),
                freight_code:text(native_ship,"freightCode").to_owned(),
                freight:field(field(native_ship,"totals"),"shipping").as_i64(),
                collect:text(native_ship,"freightCode")=="020",
                vat_included: field(native_ship, "vatIncluded").as_bool(),
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
        let automatic=gateway.execute(ReadOperation::RenderAutomaticMessage,json!({
            "state":&state,"groupId":group_id,"includeLinks":false,
            "accountWire":wire,
        }));
        let (automatic_text,automatic_error,automatic_phase,automatic_payment_issues)=
            match automatic{
                Ok(result)=>(
                    result.get("text").and_then(Value::as_str).map(str::to_owned),
                    None,
                    result.get("phase").and_then(Value::as_str).map(str::to_owned),
                    rows(field(&result,"paymentIssues")).iter()
                        .filter_map(Value::as_str).map(str::to_owned).collect()
                ),
                Err(error)=>(None,Some(error),None,Vec::new()),
            };
        let issues=rows(field(native,"issues")).iter()
            .filter_map(workflow_issue_label)
            .collect();
        groups.push(GroupRow{
            id:group_id.to_owned(),
            created_at:text(group,"createdAt").to_owned(),
            dealer_name,
            source_id:text(group,"sourceId").to_owned(),
            dealer_id:text(group,"dealerId").to_owned(),
            transferred:rows(all_shipments).iter().any(|ship|
                text(ship,"groupId")==group_id &&
                field(ship,"transferred")==&Value::Bool(true)),
            step,
            needs_notice:field(native,"needsNotice")==&Value::Bool(true),
            total:field(field(native,"totals"),"total").as_i64(),
            vat_included,vat_excluded,
            shipments:display_shipments,
            automatic_text,automatic_error,automatic_phase,
            automatic_payment_issues,issues,
        });
    }
    groups.sort_by(|a,b|b.created_at.cmp(&a.created_at));
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
            source_raw:"공통 발주원문".into(),recipient:json!({"id":recipient}),
            remote:None,display_charges:HashMap::new(),
            freight_editing_blocked:false,freight_code:"010".into(),
            freight:None,collect:false,vat_included:None,
            shipment_count:1,order_total:None,
        }
    }
    fn group(id:&str,shipments:Vec<ShipmentRow>)->GroupRow{
        GroupRow{
            id:id.into(),created_at:String::new(),dealer_name:String::new(),
            source_id:String::new(),dealer_id:String::new(),transferred:false,
            step:0,needs_notice:false,total:None,vat_included:0,vat_excluded:0,
            shipments,automatic_text:None,automatic_error:None,
            automatic_phase:None,automatic_payment_issues:vec![],issues:vec![],
        }
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
