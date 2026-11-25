//! Test-only fresh v2 database from the bundled product seed. No live DB read.
use rusqlite::{Connection,OpenFlags,params};
use serde_json::Value;
use std::path::Path;

pub(super) fn initialize_new(path:&Path)->Result<(),String>{
    if path.exists(){return Err("기존 테스트 DB를 덮어쓰지 않습니다.".into());}
    let parent=path.parent().ok_or("테스트 DB 폴더 없음")?;
    let temp=parent.join(format!("test-initializing-{}.sqlite",uuid::Uuid::new_v4()));
    // Reserve exclusively, create and validate off-path, then publish without
    // overwriting a DB created concurrently by another launcher.
    std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)
        .map_err(|e|e.to_string())?;
    let outcome=(||{
        let mut db=Connection::open_with_flags(&temp,OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e|e.to_string())?;
        db.execute_batch(include_str!("test_workspace_schema.sql")).map_err(|e|e.to_string())?;
        let state:Value=serde_json::from_str(include_str!("../../src-tauri/resources/state-seed.json"))
            .map_err(|e|e.to_string())?;
        bando_data_access::valid_state(&state)?;
        for key in ["clients","orders","aliases","exports","factoryReplies"]{
            if state[key].as_array().map(Vec::len)!=Some(0){
                return Err(format!("테스트 초기 데이터에 비어 있지 않은 {key} 목록이 있습니다."));
            }
        }
        let tx=db.transaction().map_err(|e|e.to_string())?;
        bando_data_access::store_v2_relational(&tx,&state)?;
        for product in state["products"].as_array().ok_or("초기 상품 누락")?{
            tx.execute("INSERT INTO product_index(id,code,name,payload) VALUES (?1,?2,?3,?4)",
                params![product["id"].as_str().ok_or("상품 ID 누락")?,
                    product["code"].as_str().ok_or("상품 코드 누락")?,
                    product["name"].as_str().ok_or("상품명 누락")?,product.to_string()])
                .map_err(|e|e.to_string())?;
        }
        let mut envelope=state.clone();
        let object=envelope.as_object_mut().ok_or("초기 상태 형식 오류")?;
        for key in ["products","clients","orders"]{object.remove(key);}
        tx.execute("INSERT INTO app_state(id,payload) VALUES(1,?1)",[envelope.to_string()])
            .map_err(|e|e.to_string())?;
        tx.commit().map_err(|e|e.to_string())?;
        if bando_data_access::read_v2_snapshot(&db)?!=state{
            return Err("테스트 초기 DB 재조회 불일치".into());
        }
        drop(db);
        drop(bando_data_access::backup_service::read_backup(&temp)?);
        std::fs::hard_link(&temp,path).map_err(|e|e.to_string())?;
        Ok::<(),String>(())
    })();
    let _=std::fs::remove_file(&temp);
    outcome
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn fresh_test_database_preserves_seed_and_refuses_overwrite(){
        let root=std::env::temp_dir().join(format!("bando-test-bootstrap-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path=root.join("bando-hub-test.sqlite");
        initialize_new(&path).unwrap();
        let db=bando_data_access::backup_service::read_backup(&path).unwrap();
        let state=bando_data_access::read_v2_snapshot(&db).unwrap();
        assert!(!state["products"].as_array().unwrap().is_empty());
        assert!(state["orders"].as_array().unwrap().is_empty());
        assert!(state["clients"].as_array().unwrap().is_empty());
        drop(db);
        let bytes=std::fs::read(&path).unwrap();
        assert!(initialize_new(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
}
