//! One native settings-account rule for both the React Tauri command and
//! the isolated egui settings editor. UI and persistence do not duplicate it.
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};
type Finding=(Vec<String>,Value,String);

pub fn validate(state: &Value) -> Result<(), String> {
    diagnostics(state).into_iter().next().map_or(Ok(()),|(_,_,message)|Err(message))
}

/// Complete findings. Unidentifiable and duplicate IDs cannot be exempted.
pub(crate) fn diagnostics(state:&Value)->Vec<Finding>{
    let mut out=Vec::new();
    let Some(accounts)=state["settings"]["accounts"].as_array()else{
        out.push((vec![],Value::Null,"입금 계좌 목록 형식 오류".into()));
        return out;
    };
    let mut counts=HashMap::new();
    for account in accounts{
        if let Some(id)=account["id"].as_str(){*counts.entry(id).or_insert(0usize)+=1;}
    }
    let mut ids=HashSet::new();
    for account in accounts{
        let Some(id)=account["id"].as_str()else{
            out.push((vec![],Value::Null,"계좌 ID 오류".into()));continue;
        };
        if id.is_empty()||!ids.insert(id){
            out.push((vec![],Value::Null,"입금 계좌 ID가 중복되거나 비어 있습니다.".into()));
        }
        for field in ["bank","number","holder"]{
            if account[field].as_str().unwrap_or("").trim().is_empty(){
                let key=if !id.is_empty()&&counts.get(id)==Some(&1){
                    vec!["account-field".into(),id.into(),field.into()]
                }else{vec![]};
                let evidence=match &account[field]{
                    Value::Null=>json!({"present":account.get(field).is_some()}),
                    Value::String(s)=>json!(s),
                    _=>{out.push((vec![],Value::Null,"은행명, 계좌번호, 예금주를 모두 입력한 뒤 저장하세요.".into()));continue;}
                };
                out.push((key,evidence,"은행명, 계좌번호, 예금주를 모두 입력한 뒤 저장하세요.".into()));
            }
        }
    }
    let mut check_id=|key:Vec<String>,value:&Value,message:&str|{
        if let Some(id)=value.as_str().filter(|x|!x.is_empty()){
            if !ids.contains(id){out.push((key,json!(id),message.into()));}
        }
    };
    check_id(vec!["default-account".into()],&state["settings"]["defaultAccountId"],"추천 계좌가 등록된 입금 계좌 목록에 없습니다.");
    for brand in ["BANDOSTAND","BANDOMALL"]{
        check_id(vec!["brand-account".into(),brand.into()],&state["settings"]["brandAccounts"][brand],"추천 계좌가 등록된 입금 계좌 목록에 없습니다.");
    }
    let Some(clients)=state["clients"].as_array()else{
        out.push((vec![],Value::Null,"거래처 데이터 형식 오류".into()));return out;
    };
    let mut client_counts=HashMap::new();
    for client in clients{
        if let Some(id)=client["id"].as_str(){*client_counts.entry(id).or_insert(0usize)+=1;}
    }
    for client in clients{
        if !client["deletedAt"].is_null(){continue;}
        let key=client["id"].as_str().filter(|id|!id.is_empty()&&client_counts.get(id)==Some(&1))
            .map(|id|vec!["client-account".into(),id.into()]).unwrap_or_default();
        check_id(key,&client["preferredAccountId"],"거래처에서 사용하는 입금 계좌는 삭제할 수 없습니다. 거래처의 주 사용 계좌부터 변경하세요.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn an_active_client_prevents_deletion_of_its_account() {
        let value=json!({"settings":{"accounts":[],"brandAccounts":{}},
            "clients":[{"preferredAccountId":"client-bank","deletedAt":null}]});
        assert!(validate(&value).unwrap_err().contains("거래처에서 사용"));
    }
    #[test]
    fn archived_clients_do_not_reserve_an_account() {
        let value=json!({"settings":{"accounts":[],"brandAccounts":{}},
            "clients":[{"preferredAccountId":"client-bank","deletedAt":"2026-01-01"}]});
        assert!(validate(&value).is_ok());
    }
}
