//! Read-only WorkflowBoard projection from the SAME native Rust rules as React.
//! This is not a shadow workflow engine or an egui-owned order ledger.
//! Reads the v2 snapshot only when the user explicitly opted into that DB.
//! No SQLite writes, confirmation, stage transition, or invented work groups.
use crate::gateway::{BandoGateway, ReadOperation};
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct ShipmentRow {
    pub id: String,
    pub order_id: String,
    pub recipient_id: String,
    pub recipient_name: String,
    pub products: Vec<String>,
    pub freight: Option<i64>,
    pub collect: bool,
}
#[derive(Clone, Debug)]
pub struct GroupRow {
    pub id: String,
    pub created_at: String,
    pub dealer_name: String,
    pub step: usize,
    pub needs_notice: bool,
    pub total: Option<i64>,
    pub vat_included: usize,
    pub vat_excluded: usize,
    pub shipments: Vec<ShipmentRow>,
}
#[derive(Clone, Debug, Default)]
pub struct BoardSnapshot {
    pub groups: Vec<GroupRow>,
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
            display_shipments.push(ShipmentRow{
                id:text(ship,"id").to_owned(),
                order_id:text(ship,"orderId").to_owned(),
                recipient_id:text(ship,"recipientId").to_owned(),
                recipient_name:text(recipient,"name").to_owned(),
                products,
                freight:field(field(native_ship,"totals"),"shipping").as_i64(),
                collect:text(native_ship,"freightCode")=="020",
            });
        }
        let vat_included=rows(field(native,"shipments")).iter()
            .filter(|ship|field(ship,"vatIncluded")==&Value::Bool(true)).count();
        let vat_excluded=rows(field(native,"shipments")).len()-vat_included;
        let step=field(native,"step").as_u64().unwrap_or(0).min(4) as usize;
        groups.push(GroupRow{
            id:group_id.to_owned(),
            created_at:text(group,"createdAt").to_owned(),
            dealer_name,
            step,
            needs_notice:field(native,"needsNotice")==&Value::Bool(true),
            total:field(field(native,"totals"),"total").as_i64(),
            vat_included,vat_excluded,
            shipments:display_shipments,
        });
    }
    groups.sort_by(|a,b|b.created_at.cmp(&a.created_at));
    Ok(BoardSnapshot{groups})
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
}
