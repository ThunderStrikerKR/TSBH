//! Native structural reducers for the manually classified reception draft.
//! UUID allocation stays with the React caller; every business mutation is Rust-owned.
use serde_json::{json, Value};
use std::collections::HashSet;
use super::{field, req, required_str, rows, set, str_field, MAX_SAFE};

fn new_recipient(id: &str) -> Value {
    json!({"id":id,"name":"","address":"","phone":"","mobile":"","note":"","items":[]})
}
fn new_item(id: &str) -> Value {
    json!({"id":id,"productId":null,"variantId":null,"raw":"","name":"","color":"",
        "quantity":null,"unitPrice":null,"origin":"FACTORY","defaulted":false,
        "issues":[],"url":"","quantityConfirmed":false})
}
fn ensure_unique(model: &Value, id: &str) -> Result<(), String> {
    if id.is_empty() || rows(field(model,"rows")).iter().any(|r|
        str_field(r,"id")==id || rows(field(r,"items")).iter().any(|i|str_field(i,"id")==id)) {
        return Err("수동 분류 고유 식별자가 중복되거나 비어 있습니다.".into());
    }
    Ok(())
}
fn assignment_order(payload:&Value, model:&Value)->Vec<String>{
    let mut seen=HashSet::new();
    let mut keys=vec![];
    for key in rows(field(payload,"assignmentOrder")).iter().filter_map(Value::as_str){
        if model["assignments"].get(key).is_some() && seen.insert(key.to_string()){
            keys.push(key.to_string());
        }
    }
    if let Some(map)=field(model,"assignments").as_object(){
        for key in map.keys(){
            if seen.insert(key.clone()){keys.push(key.clone());}
        }
    }
    keys
}
pub(super) fn execute(payload:&Value)->Result<Value,String>{
    let action=required_str(payload,"action")?;
    let mut model=req(payload,"model")?.clone();
    match action {
        "resize_manual_rows" => {
            let count=field(payload,"count").as_u64()
                .filter(|n|*n>=1&&*n<=100).ok_or("배송 건수는 1~100건입니다.")? as usize;
            let current=rows(field(&model,"rows")).len();
            let count_new=count.saturating_sub(current);
            let ids=rows(field(payload,"newRecipientIds"));
            if ids.len()!=count_new{return Err("추가 배송 건의 ID가 누락되었습니다.".into());}
            let mut unique=HashSet::new();
            for id in ids {
                let id=id.as_str().filter(|s|!s.is_empty()).ok_or("배송 건 ID 오류")?;
                ensure_unique(&model,id)?;
                if !unique.insert(id){return Err("배송 건 ID가 중복되었습니다.".into());}
            }
            let data=model["rows"].as_array_mut().ok_or("배송 행을 확인하세요.")?;
            data.truncate(count);
            for id in ids {data.push(new_recipient(id.as_str().unwrap()));}
            let retained=rows(field(&model,"rows")).iter()
                .map(|r|str_field(r,"id").to_string()).collect::<HashSet<_>>();
            model["assignments"].as_object_mut().ok_or("배정 정보 오류")?
                .retain(|_,cell|retained.contains(str_field(cell,"recipientId")));
        },
        "add_manual_item" => {
            let rid=required_str(payload,"recipientId")?;
            let id=required_str(payload,"itemId")?;
            ensure_unique(&model,id)?;
            let recipient=model["rows"].as_array_mut().ok_or("배송 행을 확인하세요.")?
                .iter_mut().find(|r|str_field(r,"id")==rid).ok_or("배송 행을 선택하세요.")?;
            recipient["items"].as_array_mut().ok_or("상품 목록을 확인하세요.")?.push(new_item(id));
        },
        "remove_manual_item" => {
            let rid=required_str(payload,"recipientId")?;
            let iid=required_str(payload,"itemId")?;
            let recipient=model["rows"].as_array_mut().ok_or("배송 행을 확인하세요.")?
                .iter_mut().find(|r|str_field(r,"id")==rid).ok_or("배송 행을 선택하세요.")?;
            let items=recipient["items"].as_array_mut().ok_or("상품 목록을 확인하세요.")?;
            if !items.iter().any(|item|str_field(item,"id")==iid) {
                return Err("삭제할 상품 행을 확인하세요.".into());
            }
            items.retain(|item|str_field(item,"id")!=iid);
            model["assignments"].as_object_mut().ok_or("배정 정보 오류")?
                .retain(|_,cell|str_field(cell,"itemId")!=iid);
        },
        "confirm_manual_quantity" | "set_manual_quantity_draft" | "step_manual_quantity" => {
            let cell=req(payload,"cell")?;
            let rid=required_str(cell,"recipientId")?;
            let iid=required_str(cell,"itemId")?;
            let recipient=model["rows"].as_array_mut().ok_or("배송 행을 확인하세요.")?
                .iter_mut().find(|r|str_field(r,"id")==rid).ok_or("배송 행을 선택하세요.")?;
            let item=recipient["items"].as_array_mut().ok_or("상품 목록을 확인하세요.")?
                .iter_mut().find(|i|str_field(i,"id")==iid).ok_or("상품 행을 선택하세요.")?;
            match action {
                "confirm_manual_quantity" => {
                    field(item,"quantity").as_i64()
                        .filter(|n|*n>=1 && *n<=MAX_SAFE)
                        .ok_or("수량을 1 이상의 정수로 입력하세요.")?;
                    set(item,"quantityConfirmed",json!(true))?;
                },
                "set_manual_quantity_draft" => {
                    let parsed=super::manual_classification::apply(&json!({
                        "action":"parse_manual_quantity","value":required_str(payload,"value")?
                    }))?;
                    set(item,"quantity",field(&parsed,"quantity").clone())?;
                    set(item,"quantityConfirmed",json!(false))?;
                    item.as_object_mut().ok_or("상품 데이터 형식 오류")?.remove("quantityApproval");
                    if let Some(issues)=item.get_mut("issues").and_then(Value::as_array_mut){
                        issues.retain(|issue|!issue.as_str().unwrap_or("").contains("수량"));
                    }
                },
                "step_manual_quantity" => {
                    let delta=field(payload,"delta").as_i64()
                        .filter(|n|*n==-1||*n==1).ok_or("수량 변경 단위 오류")?;
                    let before=field(item,"quantity").as_i64().unwrap_or(0);
                    let updated=before.saturating_add(delta).max(1);
                    if updated>MAX_SAFE{return Err("안전한 정수 범위를 초과했습니다.".into());}
                    set(item,"quantity",json!(updated))?;
                    set(item,"quantityConfirmed",json!(true))?;
                    item.as_object_mut().ok_or("상품 데이터 형식 오류")?.remove("quantityApproval");
                    if let Some(issues)=item.get_mut("issues").and_then(Value::as_array_mut){
                        issues.retain(|issue|!issue.as_str().unwrap_or("").contains("수량"));
                    }
                },
                _=>unreachable!(),
            }
        },
        "reset_manual_rows" => {
            let id=required_str(payload,"recipientId")?;
            if id.is_empty(){return Err("배송 건 ID 오류".into());}
            set(&mut model,"rows",json!([new_recipient(id)]))?;
            set(&mut model,"assignments",json!({}))?;
        },
        _=>return Err(format!("지원하지 않는 수동 분류 구성 명령: {action}")),
    }
    if let Some(rows)=model["rows"].as_array_mut(){
        for row in rows {super::issues::prune_validation_approvals(row);}
    }
    let keys=assignment_order(payload,&model);
    Ok(json!({"model":model,"assignmentOrder":keys}))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn draft()->Value {json!({"rows":[{"id":"r1","name":"김가상","address":"서울",
        "items":[{"id":"i1","name":"709","quantity":2,"quantityConfirmed":false,
            "issues":["수량 확인"]}]}],"blocks":[{"id":"b","field":"product"}],
        "assignments":{"b":{"recipientId":"r1","itemId":"i1","field":"product"}}})}
    #[test]fn resize_preserves_remaining_rows_and_filters_assignments(){
        let a=execute(&json!({"action":"resize_manual_rows","model":draft(),
            "count":2,"newRecipientIds":["r2"],"assignmentOrder":["b"]})).unwrap();
        assert_eq!(a["model"]["rows"][0]["id"],"r1");
        assert_eq!(a["model"]["rows"][1]["id"],"r2");
        assert_eq!(a["assignmentOrder"],json!(["b"]));
        let b=execute(&json!({"action":"resize_manual_rows","model":a["model"],
            "count":1,"newRecipientIds":[],"assignmentOrder":["b"]})).unwrap();
        assert_eq!(b["model"]["rows"].as_array().unwrap().len(),1);
        assert_eq!(b["model"]["blocks"],draft()["blocks"]);
    }
    #[test]fn item_removal_clears_only_its_assignments(){
        let result=execute(&json!({"action":"remove_manual_item","model":draft(),
            "recipientId":"r1","itemId":"i1","assignmentOrder":["b"]})).unwrap();
        assert_eq!(result["model"]["rows"][0]["items"],json!([]));
        assert_eq!(result["model"]["assignments"],json!({}));
    }
    #[test]fn quantity_confirmation_does_not_change_other_fields(){
        let result=execute(&json!({"action":"confirm_manual_quantity","model":draft(),
            "cell":{"recipientId":"r1","itemId":"i1"}})).unwrap();
        assert_eq!(result["model"]["rows"][0]["items"][0]["quantity"],2);
        assert_eq!(result["model"]["rows"][0]["items"][0]["quantityConfirmed"],true);
        assert_eq!(result["model"]["rows"][0]["items"][0]["issues"],json!(["수량 확인"]));
    }
    #[test]fn rejects_duplicate_ids_and_invalid_quantities(){
        assert!(execute(&json!({"action":"add_manual_item","model":draft(),
            "recipientId":"r1","itemId":"i1"})).is_err());
        let mut invalid=draft();
        invalid["rows"][0]["items"][0]["quantity"]=json!(9_007_199_254_740_992_i64);
        assert!(execute(&json!({"action":"confirm_manual_quantity","model":invalid,
            "cell":{"recipientId":"r1","itemId":"i1"}})).is_err());
    }
}
