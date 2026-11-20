//! Isolated native freight transaction fixtures.
//! Freight command normalization and persistence are now shared with egui.
//! No production CLI writer is retained in this module.
use super::{business_core, get_snapshot, write_snapshot};
use rusqlite::Connection;
use serde_json::{json,Value};
use bando_data_access::shipping_action::action_payload;

#[cfg(test)]
mod isolated_shipping_sqlite_verification {
    use super::*;
    use crate::{initialize_database, relational_store};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Narrow integration verification of the actual v2 relational SQLite
    /// transaction. Uses a unique temporary file seeded by the normal native
    /// initializer; never points at an employee's installed app-data DB.
    #[test]
    fn freight_commit_preserves_staff_price_history_and_rejects_stale_revision() {
        let nanos=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!(
            "bando-freight-isolated-{}-{nanos}.sqlite",std::process::id()
        ));
        let mut conn=initialize_database(&path).expect("isolated v2 database initialization");
        assert!(relational_store::active(&conn).unwrap());
        let initial=get_snapshot(&conn).unwrap();
        let mut staff=initial.clone();
        let price=staff["products"].as_array_mut().unwrap().iter_mut()
            .find_map(|product|{
                let variant=product["variants"].as_array_mut()?.first_mut()?;
                let old=variant["retail"].as_i64()?;
                let edited=old+1234;
                variant["retail"]=json!(edited);
                Some(edited)
            }).expect("a normal product retail-price record exists");
        let first_revision=initial["revision"].as_u64().unwrap();
        let _=write_snapshot(&mut conn,staff,first_revision,true)
            .expect("staff retail price change committed");
        // The distributed seed intentionally starts with no shipping groups.
        // Create one through the real Rust action instead of inventing an
        // in-memory business record that bypasses the shipping command.
        let after_staff=get_snapshot(&conn).unwrap();
        let new_group=action_payload("create_group",&json!({
            "name":"격리 운임 검증 그룹"
        }),&after_staff).unwrap();
        let created=business_core::execute(
            "rules","apply_shipping_action",new_group
        ).expect("create group through existing Rust rule");
        let mut with_group=after_staff.clone();
        with_group["shipping"]=created["shipping"].clone();
        let _=write_snapshot(
            &mut conn,with_group,after_staff["revision"].as_u64().unwrap(),true
        ).expect("isolated group created");
        let before=get_snapshot(&conn).unwrap();
        assert!(before["products"].as_array().unwrap().iter().any(|p|
            p["variants"].as_array().is_some_and(|variants|
                variants.iter().any(|v|v["retail"]==json!(price)))));
        let unchanged_products=before["products"].clone();
        let unchanged_orders=before["orders"].clone();
        let unchanged_clients=before["clients"].clone();

        let group=before["shipping"]["groups"].as_array().unwrap()
            .iter().find(|g|g["deletedAt"].is_null())
            .expect("existing shipping group");
        let group_id=group["id"].as_str().unwrap().to_owned();
        let previous=group["current"].clone();
        let mut rate=previous.clone();
        rate["maxBundle"]=json!(2);
        rate["perItem"]=Value::Null;
        rate["fees"]=json!({"1":4321,"2":5678});
        rate["notes"]=json!("임시 DB 운임 검증");
        let payload=action_payload("save_group_rule",&json!({
            "groupId":group_id,"rule":rate
        }),&before).expect("bridge accepts valid freight-only update");
        let engine=business_core::execute("rules","apply_shipping_action",payload)
            .expect("reuse existing Rust shipping rules");
        let mut next=before.clone();
        next["shipping"]=engine["shipping"].clone();
        let expected=before["revision"].as_u64().unwrap();
        let committed=write_snapshot(&mut conn,next.clone(),expected,true)
            .expect("native relational SQLite snapshot commit");
        assert_eq!(committed["revision"],json!(expected+1));
        let actual=get_snapshot(&conn).unwrap();
        assert_eq!(actual["products"],unchanged_products);
        assert_eq!(actual["orders"],unchanged_orders);
        assert_eq!(actual["clients"],unchanged_clients);
        let saved=actual["shipping"]["groups"].as_array().unwrap()
            .iter().find(|g|g["id"]==group_id).unwrap();
        assert_eq!(saved["current"]["fees"]["1"],json!(4321));
        assert_eq!(saved["current"]["fees"]["2"],json!(5678));
        assert_eq!(saved["history"].as_array().unwrap().last(),Some(&previous));
        relational_store::verify(&conn).expect("all relational price rows intact");

        assert!(write_snapshot(&mut conn,next,expected,true)
            .unwrap_err().contains("다른 작업에서 데이터가 수정되었습니다."));
        assert_eq!(get_snapshot(&conn).unwrap(),actual);
        drop(conn);
        let reopened=Connection::open(&path).unwrap();
        assert_eq!(get_snapshot(&reopened).unwrap(),actual);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    /// Only run when Windows CI explicitly supplies an unused test DB path.
    /// Creates a real v2 Tauri DB through the native initializer so a separate
    /// egui test process can exercise the shipped one-shot CLI end to end.
    #[test]
    #[ignore = "explicit isolated E2E fixture; requires BANDO_EGUI_SHIPPING_FIXTURE_DB"]
    fn prepare_egui_shipping_gateway_fixture() {
        let raw=std::env::var_os("BANDO_EGUI_SHIPPING_FIXTURE_DB")
            .expect("explicit CI-only fixture path is required");
        let path=PathBuf::from(raw);
        assert!(path.is_absolute(),"isolated fixture must have an absolute path");
        assert_eq!(
            path.file_name().and_then(|name|name.to_str()),
            Some("bando-freight-gateway-e2e.sqlite"),
            "refuse any operational DB filename"
        );
        assert!(!path.exists(),"refuse to overwrite an existing DB");
        let mut conn=initialize_database(&path).expect("initialize isolated Tauri v2 DB");
        assert!(relational_store::active(&conn).unwrap());
        let mut state=get_snapshot(&conn).unwrap();
        let product=state["products"].as_array_mut().unwrap().first_mut().unwrap();
        let variant=product["variants"].as_array_mut().unwrap().first_mut().unwrap();
        let prior_price=variant["retail"].as_i64().unwrap();
        variant["retail"]=json!(prior_price+1234);
        // Include one locally deleted product in the fixture. The shipping
        // mutation must not restore it or silently rewrite its record.
        let removed=state["products"].as_array_mut().unwrap().get_mut(1).unwrap();
        removed["deletedAt"]=json!("2026-11-20T00:00:00+09:00");
        let original_revision=state["revision"].as_u64().unwrap();
        write_snapshot(&mut conn,state,original_revision,true)
            .expect("staff price/deletion edit saved to test DB");
        let before=get_snapshot(&conn).unwrap();
        let payload=action_payload("create_group",&json!({
            "name":"격리 전체 경로 검증"
        }),&before).unwrap();
        let response=business_core::execute("rules","apply_shipping_action",payload)
            .expect("native freight group creation");
        let mut next=before.clone();
        next["shipping"]=response["shipping"].clone();
        write_snapshot(&mut conn,next,before["revision"].as_u64().unwrap(),true)
            .expect("commit isolated shipping group");
        let ready=get_snapshot(&conn).unwrap();
        assert_eq!(ready["products"][0]["variants"][0]["retail"],json!(prior_price+1234));
        assert!(!ready["products"][1]["deletedAt"].is_null());
        assert!(ready["shipping"]["groups"].as_array().unwrap()
            .iter().any(|g|g["name"]=="격리 전체 경로 검증"));
        relational_store::verify(&conn).expect("test price-row integrity");
        // The second process opens this same file. Never clean or reset it
        // here, and never use any installed Bando DB.
    }
}
