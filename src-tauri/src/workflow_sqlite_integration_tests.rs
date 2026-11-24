//! End-to-end native workflow lifecycle against the app's REAL SQLite
//! initializer and v2 relational core, not a mocked data store or in-memory
//! JSON-only test. Uses a unique temporary directory; never production data.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct IsolatedDatabase {
    root: PathBuf,
}
impl IsolatedDatabase {
    fn open() -> (Self, Connection) {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!(
            "bando-native-workflow-e2e-{}-{stamp}", std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let conn = initialize_database(&root.join("state.sqlite"))
            .expect("real production SQLite initializer must succeed");
        (Self { root }, conn)
    }
}
impl Drop for IsolatedDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn rust(operation: &str, payload: Value) -> Result<Value, String> {
    business_core::execute("rules", operation, payload)
}

fn stage_three(base: &Value, payment: &str) -> Value {
    assert!(base["orders"].as_array().unwrap().is_empty());
    let mut state = base.clone();
    let product = &base["products"][0];
    let product_id = product["id"].as_str().unwrap().to_owned();
    let variant_id = product["variants"][0]["id"].as_str().unwrap().to_owned();
    let client = json!({
        "id":"synthetic-workflow-client", "name":"합성 테스트 거래처",
        "tier":"wholesale", "payment":payment, "shippingFeeExcluded":false,
        "address":"서울특별시 강남구 테헤란로 123", "phone":"02-1234-5678",
        "mobile":"", "preferredAccountId":"synthetic-account", "overrides":{}
    });
    state["settings"]["accounts"] = json!([{
        "id":"synthetic-account", "bank":"가상은행",
        "number":"123-456", "holder":"합성 계좌"
    }]);
    state["settings"]["defaultAccountId"] = json!("synthetic-account");
    state["settings"]["operatorName"] = json!("검증 담당자");
    state["clients"].as_array_mut().unwrap().push(client.clone());

    let at = "2026-11-13T12:00:00+09:00";
    let raw = "합성 상품 1개 수취인 김테스트";
    let recipients = json!([{
        "id":"recipient:synthetic", "name":"김테스트",
        "address":"서울특별시 강남구 테헤란로 123, 4층",
        "phone":"010-1234-5678", "mobile":"",
        "vatDefault":true, "note":"",
        "items":[{
            "id":"item:synthetic","productId":product_id,
            "variantId":variant_id,"name":"합성 상품", "color":"색상 미표기",
            "quantity":1,"unitPrice":30000,"origin":"FACTORY"
        }]
    }]);
    let mut order = json!({
        "id":"order:synthetic","receptionVersion":1,
        "clientId":"synthetic-workflow-client","orderType":"AGENCY",
        "raw":raw,"createdAt":at,"updatedAt":at,"confirmedAt":at,
        "status":"STATEMENT","recipients":recipients,
        "applyVat":true,"freightCode":"010","shippingFee":3000,
        "pricesLocked":true,"vatAmount":null,
        "clientSnapshot":client,"senderSnapshot":client,
        "receipt":{
            "original":raw,"at":at,"actor":"검증 담당자",
            "clientId":"synthetic-workflow-client","legacy":false
        },"rawCorrections":[]
    });
    order["confirmationSnapshot"] = json!({
        "at":at,"raw":raw,"client":client,
        "recipients":recipients,"data":order
    });
    state["orders"].as_array_mut().unwrap().push(order);
    state["workflow"] = json!({
        "sources":[{
            "id":"source:synthetic","orderId":"order:synthetic",
            "dealerId":"synthetic-workflow-client","number":1
        }],
        "groups":[{
            "id":"group:synthetic","sourceId":"source:synthetic",
            "dealerId":"synthetic-workflow-client",
            "notices":[],"needsNotice":false
        }],
        "shipments":[{
            "id":"shipment:synthetic","sourceId":"source:synthetic",
            "orderId":"order:synthetic","recipientId":"recipient:synthetic",
            "groupId":"group:synthetic","step":3,
            "completedAt":{"1":at,"2":at,"3":at},
            "charges":{"FACTORY":3000},"chargeMode":"MANUAL",
            "freightVersion":"v1","infoSignature":"stable",
            "transferred":false
        }],
        "payments":[],"confirmations":[],"queue":[],"deliveries":[],
        "changeRequests":[],
        "events":[{
            "id":"event:confirmation","at":at,"actor":"검증 담당자",
            "action":"주문 확인 완료","shipmentIds":["shipment:synthetic"],
            "groupId":"group:synthetic"
        }]
    });
    state
}

#[test]
fn native_draft_preview_and_reconcile_are_read_only_against_relational_sqlite() {
    let (isolated, mut conn) = IsolatedDatabase::open();
    let before = get_snapshot(&conn).unwrap();
    assert!(relational_store::active(&conn).unwrap());
    let source = stage_three(&before, "POSTPAID");
    let saved = write_snapshot(&mut conn, source, 0, true)
        .expect("confirmed order must persist through the real SQLite transaction");
    let persisted_event_count = saved["workflow"]["events"].as_array().unwrap().len();

    let board = rust("preview_workflow", json!({"state":saved})).unwrap();
    assert_eq!(board["groups"][0]["groupId"], "group:synthetic");
    assert_eq!(board["groups"][0]["step"], 3);
    assert_eq!(board["groups"][0]["totals"]["total"], 36300);
    let detail = rust("preview_order", json!({
        "state": saved, "draft": saved["orders"][0],
        "orderId":"order:synthetic"
    })).unwrap();
    assert_eq!(detail["total"], 36300);
    assert_eq!(detail["shipmentCount"], 1);

    let mut edit = saved["orders"][0].clone();
    edit["recipients"][0]["note"] = json!("미저장 직원 수정");
    let event_ids = (0..256).map(|index|format!("transient-audit-{index}"))
        .collect::<Vec<_>>();
    let preview = rust("reconcile_workflow", json!({
        "state":saved,"draft":edit,
        "now":"2026-11-13T12:15:00+09:00","eventIds":event_ids
    })).expect("native reconciliation must return an unsaved preview");
    assert_eq!(preview["state"]["orders"][0]["recipients"][0]["note"],
        "미저장 직원 수정");
    assert_eq!(get_snapshot(&conn).unwrap(), saved);
    assert_eq!(saved["workflow"]["events"].as_array().unwrap().len(),
        persisted_event_count);
    assert_eq!(saved["revision"], 1);
    drop(conn);

    let reopened = initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(), saved);
    let count:i64 = reopened.query_row(
        "SELECT COUNT(*) FROM core_orders WHERE id='order:synthetic'",
        [], |row| row.get(0)
    ).unwrap();
    assert_eq!(count, 1);
    drop(reopened);
}

#[test]
fn native_unconfirmed_draft_first_stage_confirmation_commits_as_one_order() {
    let (isolated,mut conn)=IsolatedDatabase::open();
    let mut initial=get_snapshot(&conn).expect("initial relational SQLite snapshot");
    let product_id=initial["products"][0]["id"].as_str().unwrap().to_owned();
    let variant_id=initial["products"][0]["variants"][0]["id"].as_str().unwrap().to_owned();
    // The seed is intentionally catalog-only. Give this isolated fixture one
    // valid priced variant and one existing operational client.
    initial["products"][0]["variants"][0]["wholesale"]=json!(30000);
    let client=json!({
        "id":"client:unconfirmed-draft",
        "name":"합성 미확정 거래처",
        "tier":"wholesale",
        "payment":"POSTPAID",
        "shippingFeeExcluded":true,
        "defaultApplyVat":false,
        "address":"서울특별시 강남구 테헤란로 123",
        "phone":"02-1234-5678",
        "mobile":"",
        "preferredAccountId":"",
        "overrides":{},
        "active":true
    });
    initial["clients"].as_array_mut().unwrap().push(client.clone());
    let saved=write_snapshot(&mut conn,initial,0,true)
        .expect("synthetic client and price fixture must persist");

    let at="2026-11-25T10:00:00+09:00";
    let raw="합성 상품 1개 수취인 김테스트";
    let draft=json!({
        "id":"order:unconfirmed-draft",
        "receptionVersion":1,
        "clientId":client["id"],
        "orderType":"AGENCY",
        "raw":raw,
        "createdAt":at,
        "updatedAt":at,
        "confirmedAt":null,
        "status":"REVIEW",
        "recipients":[{
            "id":"recipient:unconfirmed-draft",
            "name":"김테스트",
            "address":"서울특별시 강남구 테헤란로 123, 4층",
            "addressStatus":"COMPLETE",
            "phone":"010-1234-5678",
            "mobile":"",
            "note":"",
            "freightCode":"010",
            "dispatchOrigin":"FACTORY",
            "vatDefault":false,
            "vatOverride":false,
            "items":[{
                "id":"item:unconfirmed-draft",
                "productId":product_id,
                "variantId":variant_id,
                "name":"합성 상품",
                "color":"색상 미표기",
                "quantity":1,
                "quantityConfirmed":true,
                "unitPrice":30000,
                "manualUnitPrice":30000,
                "origin":"FACTORY"
            }]
        }],
        "applyVat":false,
        "freightCode":"010",
        "shippingFee":0,
        "shippingVersion":null,
        "vatAmount":null,
        "pricesLocked":false,
        "clientSnapshot":null,
        "senderSnapshot":null,
        "receipt":null,
        "confirmationSnapshot":null,
        "rawCorrections":[]
    });
    let reconcile_ids=(0..256).map(|i|format!("draft-reconcile-{i}"))
        .collect::<Vec<_>>();
    let preview=rust("reconcile_workflow",json!({
        "state":saved,"draft":draft,"now":at,"eventIds":reconcile_ids
    })).expect("native reconciliation must project the selected dealer draft");
    let prepared=preview["state"].clone();
    assert_eq!(prepared["orders"].as_array().unwrap().len(),1);
    assert!(prepared["orders"][0]["confirmedAt"].is_null(),
        "dealer selection and pricing alone must not confirm the order");
    assert_eq!(prepared["workflow"]["groups"][0]["id"],"group:order:unconfirmed-draft");
    assert_eq!(prepared["workflow"]["shipments"][0]["step"],0);

    let audit_ids=(0..2048).map(|i|format!("draft-confirm-audit-{i}"))
        .collect::<Vec<_>>();
    let decision=rust("apply_workflow_action",json!({
        "state":prepared,
        "action":"advance_group",
        "groupId":"group:order:unconfirmed-draft",
        "eventId":"event:unconfirmed-draft-confirmation",
        "now":at,
        "actor":"검증 담당자",
        "auditEventIds":audit_ids,
        "newGroupId":"group:unused-new",
        "confirmationId":"confirmation:unconfirmed-draft",
        "noticeId":"notice:unconfirmed-draft"
    })).expect("only the explicit first-stage action confirms the draft");
    let confirmed=decision["state"].clone();
    assert_eq!(decision["step"],1);
    assert_eq!(confirmed["orders"][0]["confirmedAt"],at);
    assert_eq!(confirmed["orders"][0]["pricesLocked"],true);
    assert_eq!(confirmed["orders"][0]["clientSnapshot"]["id"],client["id"]);
    assert_eq!(confirmed["orders"][0]["receipt"]["original"],raw);
    assert_eq!(confirmed["workflow"]["shipments"][0]["step"],1);
    assert_eq!(confirmed["workflow"]["shipments"][0]["transferred"],false,
        "first-stage confirmation must not transfer the shipment to the Excel queue");
    assert_eq!(confirmed["workflow"]["groups"][0]["id"],
        "group:order:unconfirmed-draft",
        "first-stage confirmation must keep the order in its existing workflow group");
    assert!(confirmed["workflow"]["groups"][0]["deletedAt"].is_null(),
        "first-stage confirmation must not archive or delete the workflow group");
    rust("validate_workflow",json!({
        "state":confirmed,"before":saved
    })).expect("first-stage confirmation and audit must validate");

    let committed=write_snapshot(
        &mut conn,confirmed,saved["revision"].as_u64().unwrap(),true
    ).expect("confirmed order and first workflow stage must commit atomically");
    assert_eq!(committed["revision"],saved["revision"].as_u64().unwrap()+1);
    assert_eq!(committed["workflow"]["shipments"][0]["transferred"],false);
    assert_eq!(committed["workflow"]["groups"][0]["id"],
        "group:order:unconfirmed-draft");
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    let count:i64=conn.query_row(
        "SELECT COUNT(*) FROM core_orders WHERE id='order:unconfirmed-draft'",
        [],|row|row.get(0)
    ).unwrap();
    assert_eq!(count,1);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);
    assert_eq!(committed["workflow"]["shipments"][0]["step"],1);
    drop(reopened);
}

#[test]
fn native_postpaid_stage_approval_audit_and_revision_survive_sqlite_reopen() {
    let (isolated, mut conn) = IsolatedDatabase::open();
    let source = stage_three(&get_snapshot(&conn).unwrap(), "POSTPAID");
    let saved = write_snapshot(&mut conn, source, 0, true).unwrap();
    let decision = rust("apply_workflow_action", json!({
        "state":saved,"action":"advance_group","groupId":"group:synthetic",
        "eventId":"event:postpaid-approval",
        "now":"2026-11-13T12:30:00+09:00","actor":"검증 담당자"
    })).expect("postpaid order should be able to approve stage 4");
    assert_eq!(decision["step"], 4);
    let approved = decision["state"].clone();
    assert_eq!(approved["workflow"]["shipments"][0]["step"], 4);
    rust("validate_workflow", json!({
        "state":approved,"before":saved
    })).expect("native audit and workflow must validate before SQLite commit");
    let committed = write_snapshot(&mut conn, approved, 1, true)
        .expect("approved transition must persist atomically");
    assert_eq!(committed["revision"], 2);
    assert!(committed["workflow"]["events"].as_array().unwrap().iter()
        .any(|event| event["id"]=="event:postpaid-approval"
            && event["action"]=="후불 출고 승인"));
    let stale = write_snapshot(&mut conn, saved.clone(), 1, true)
        .expect_err("stale workspace must not overwrite approved workflow");
    assert!(stale.contains("다른 작업"));
    assert_eq!(get_snapshot(&conn).unwrap(), committed);
    let count:i64 = conn.query_row(
        "SELECT COUNT(*) FROM core_recipients WHERE order_id='order:synthetic'",
        [], |row|row.get(0)
    ).unwrap();
    assert_eq!(count, 1);
    drop(conn);
    let reopened = initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(), committed);
    assert_eq!(committed["workflow"]["shipments"][0]["step"], 4);
    drop(reopened);
}

#[test]
fn full_staff_stage_sequence_reuses_same_native_ids_and_persists_each_step() {
    let (isolated, mut conn) = IsolatedDatabase::open();
    let mut staged = stage_three(&get_snapshot(&conn).unwrap(), "POSTPAID");
    staged["workflow"]["shipments"][0]["step"] = json!(1);
    staged["workflow"]["shipments"][0]["completedAt"] =
        json!({"1":"2026-11-13T12:00:00+09:00"});
    let mut saved = write_snapshot(&mut conn, staged, 0, true).unwrap();
    let id = saved["workflow"]["shipments"][0]["id"].clone();

    for step in 2..=4 {
        let at = format!("2026-11-13T12:{}:00+09:00", step * 10);
        let mut args = json!({
            "state":saved, "action":"advance_group",
            "groupId":"group:synthetic",
            "eventId":format!("event:advance-to-{step}"),
            "now":at, "actor":"검증 담당자"
        });
        if step == 3 {
            args["noticeId"] = json!("notice:synthetic");
            args["paymentMessage"] = json!("가상 거래처 안내: 결제정보 검증용");
        }
        let result = rust("apply_workflow_action", args)
            .unwrap_or_else(|reason|panic!("step {step} native action failed: {reason}"));
        assert_eq!(result["step"], step);
        assert_eq!(result["state"]["workflow"]["shipments"][0]["id"],id);
        assert_eq!(result["state"]["workflow"]["shipments"][0]["step"],step);
        rust("validate_workflow",json!({
            "state":result["state"],"before":saved
        })).unwrap_or_else(|reason|panic!("step {step} audit failed: {reason}"));
        let revision = saved["revision"].as_u64().unwrap();
        saved = write_snapshot(&mut conn,result["state"].clone(),revision,true)
            .unwrap_or_else(|reason|panic!("step {step} SQLite failed: {reason}"));
        assert_eq!(saved["revision"], revision+1);
        assert_eq!(get_snapshot(&conn).unwrap(),saved);
        let board = rust("preview_workflow",json!({"state":saved})).unwrap();
        assert_eq!(board["groups"][0]["step"],step);
    }
    assert_eq!(saved["revision"],4);
    assert_eq!(saved["workflow"]["groups"][0]["notices"].as_array().unwrap().len(),1);
    assert_eq!(saved["workflow"]["groups"][0]["notices"][0]["id"],"notice:synthetic");
    assert!(saved["workflow"]["events"].as_array().unwrap().iter()
        .any(|event|event["action"]=="후불 출고 승인"));
    drop(conn);
    let reopened = initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),saved);
    drop(reopened);
}

#[test]
fn payment_copy_evidence_survives_sqlite_reopen_and_rejects_changed_account(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let state=stage_three(&get_snapshot(&conn).unwrap(),"POSTPAID");
    let saved=write_snapshot(&mut conn,state,0,true).unwrap();
    let status=|state:&Value|{
        business_core::execute("output","payment_copy_status",json!({
            "state":state,"groupId":"group:synthetic","mode":"payment_copy_status"
        })).unwrap()["copied"].as_bool().unwrap()
    };
    assert!(!status(&saved));
    let message=business_core::execute("output","render_automatic_message",json!({
        "state":&saved,"groupId":"group:synthetic","includeLinks":false
    })).unwrap();
    let signature=message["signature"].as_str().filter(|s|!s.is_empty()).unwrap();
    let record=|state:&Value,copy_type:&str,token:&str,event_id:&str|{
        rust("record_message_copy",json!({
            "state":state,"groupId":"group:synthetic","copyType":copy_type,
            "token":token,"signature":signature,"eventId":event_id,
            "now":"2026-11-13T12:40:00+09:00"
        })).unwrap()["state"].clone()
    };
    let request=record(&saved,"REQUEST","request-token","event:request-copy");
    assert!(!status(&request),"request copy must not count as payment copy");
    let copied=record(&request,"PAYMENT","payment-token","event:payment-copy");
    assert!(status(&copied));
    assert_eq!(record(&copied,"PAYMENT","payment-token","event:duplicate-copy"),copied,
        "one clipboard transaction must create only one audit event");
    assert_eq!(copied["workflow"]["shipments"],saved["workflow"]["shipments"]);
    assert_eq!(copied["orders"],saved["orders"]);
    rust("validate_workflow",json!({"state":&copied,"before":&saved})).unwrap();
    let committed=write_snapshot(&mut conn,copied,1,true).unwrap();
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    let mut fresh=get_snapshot(&reopened).unwrap();
    assert_eq!(fresh,committed);
    assert!(status(&fresh));
    fresh["settings"]["accounts"][0]["number"]=json!("987-654");
    assert!(!status(&fresh),"different payment account must require fresh copy evidence");
    drop(reopened);
}

#[test]
fn native_prepaid_stage_rejection_leaves_sqlite_order_and_audit_untouched() {
    let (isolated, mut conn) = IsolatedDatabase::open();
    let source = stage_three(&get_snapshot(&conn).unwrap(), "PREPAID");
    let saved = write_snapshot(&mut conn, source, 0, true).unwrap();
    let count = saved["workflow"]["events"].as_array().unwrap().len();
    let issue = rust("apply_workflow_action", json!({
        "state":saved,"action":"advance_group","groupId":"group:synthetic",
        "eventId":"event:unpaid-approval",
        "now":"2026-11-13T12:30:00+09:00","actor":"검증 담당자"
    })).expect_err("a prepaid shipment cannot advance without a payment");
    assert!(issue.contains("입금") || issue.contains("부족"),
        "must explain the missing payment: {issue}");
    assert_eq!(get_snapshot(&conn).unwrap(), saved);
    assert_eq!(saved["workflow"]["events"].as_array().unwrap().len(),count);
    assert_eq!(saved["revision"],1);
    drop(conn);
    let reopened = initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),saved);
    drop(reopened);
}


/// The egui model-family editor saves through this exact Tauri settings_patch
/// function. Use the REAL v2 relational SQLite initializer and disk reopen,
/// not an in-memory mock or an operational database.
#[test]
fn native_model_group_sqlite_roundtrip_and_stale_editor_conflict() {
    let (isolated, mut conn) = IsolatedDatabase::open();
    let before = get_snapshot(&conn).expect("initial real SQLite snapshot");
    let product_ids: Vec<String> = before["products"].as_array().unwrap().iter()
        .filter(|p| {
            p["active"] != false &&
            p.get("deletedAt").is_none_or(Value::is_null) &&
            p.get("supersededBy").is_none_or(Value::is_null) &&
            p["catalog"] != "DEMO" &&
            p["variants"].as_array().is_some_and(|v| v.len() == 1
                && v.iter().all(|unit|unit["source"] != "DEMO"))
        })
        .filter_map(|p|p["id"].as_str().map(str::to_owned))
        .take(2).collect();
    assert_eq!(product_ids.len(), 2,
        "the real product seed must include two independent registered SKUs");

    let baseline = before["settings"].get("colorGuidanceGroups")
        .cloned().unwrap_or(Value::Null);
    let mut proposed = baseline.as_object().cloned().unwrap_or_default();
    let group_id = "synthetic-model-group-sqlite-roundtrip";
    assert!(!proposed.contains_key(group_id));
    proposed.insert(group_id.into(),json!({
        "name": "합성 모델군 / 타공·민판",
        "productIds": product_ids,
        "representativeProductId": product_ids[0]
    }));
    let proposed=Value::Object(proposed);

    // React's actual save order: validate, backup, per-key settings CAS.
    let mut candidate=before.clone();
    candidate["settings"]["colorGuidanceGroups"]=proposed.clone();
    assert_eq!(business_core::catalog_clients::execute(
        "validate_model_groups",json!({"state":candidate})).unwrap()["valid"],true);
    let backup=backup_manager::manual_backup(&conn,&isolated.root)
        .expect("create a full SQLite backup before changing a model group");
    let backup_conn=Connection::open_with_flags(
        &backup,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(get_snapshot(&backup_conn).unwrap(),before,
        "pre-save backup must contain the unchanged source snapshot");
    drop(backup_conn);

    let committed=apply_settings_patch(&mut conn,true,
        &json!({"colorGuidanceGroups":proposed}),
        &json!({"colorGuidanceGroups":baseline}))
        .expect("persist the valid family via Tauri's settings patch");
    assert_eq!(committed["settings"]["colorGuidanceGroups"],proposed);
    assert_eq!(committed["revision"].as_u64(),
        before["revision"].as_u64().map(|n|n+1));
    for field in ["products","orders","clients","aliases","queue","exports"] {
        assert_eq!(committed[field],before[field],
            "family edits must not change {field}");
    }
    drop(conn);

    // A fresh connection must hydrate precisely what the UI will reload.
    let mut reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);

    // Unrelated setting edits advance the global revision but must NOT
    // invalidate a model-family draft: CAS is per settings key.
    let earlier_operator=committed["settings"].get("operatorName")
        .cloned().unwrap_or(Value::Null);
    let unrelated=apply_settings_patch(&mut reopened,true,
        &json!({"operatorName":"동시 편집 B"}),
        &json!({"operatorName":earlier_operator})).unwrap();
    assert_eq!(unrelated["settings"]["colorGuidanceGroups"],proposed);

    // Editor A can save from its original group baseline despite that change.
    let mut from_a=proposed.clone();
    from_a[group_id]["name"]=json!("모델군 A의 저장값");
    let winner=apply_settings_patch(&mut reopened,true,
        &json!({"colorGuidanceGroups":from_a}),
        &json!({"colorGuidanceGroups":proposed}))
        .expect("independent settings key must not block this save");
    assert_eq!(winner["settings"]["operatorName"],"동시 편집 B");

    // Editor B submits a conflicting change using the STALE group baseline.
    let mut from_b=proposed.clone();
    from_b[group_id]["representativeProductId"]=json!(product_ids[1]);
    let error=apply_settings_patch(&mut reopened,true,
        &json!({"colorGuidanceGroups":from_b}),
        &json!({"colorGuidanceGroups":proposed}))
        .expect_err("stale group editor must be rejected");
    assert!(error.contains("colorGuidanceGroups 설정이 다른 화면에서 수정됐습니다"),
        "reject the stale field without overwriting editor A: {error}");
    assert_eq!(get_snapshot(&reopened).unwrap(),winner,
        "stale save cannot increment revision or mutate any rows");

    // Delete the saved group, then reopen SQLite one more time.
    let mut after_delete=from_a.clone();
    after_delete.as_object_mut().unwrap().remove(group_id);
    let removed=apply_settings_patch(&mut reopened,true,
        &json!({"colorGuidanceGroups":after_delete}),
        &json!({"colorGuidanceGroups":from_a})).unwrap();
    assert!(removed["settings"]["colorGuidanceGroups"].get(group_id).is_none());
    assert_eq!(removed["products"],before["products"]);
    assert_eq!(removed["orders"],before["orders"]);
    drop(reopened);
    let final_conn=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&final_conn).unwrap(),removed);
    drop(final_conn);
}


/// Product Excel UI -> native workbook preview -> manual SQLite backup ->
/// direct Rust CAS writer. Uses ONLY a fresh isolated on-disk v2 database.
#[test]
fn native_product_excel_backup_commit_reopen_and_stale_revision_guard() {
    let (isolated,mut conn)=IsolatedDatabase::open();
    let before=get_snapshot(&conn).expect("real initial v2 SQLite state");
    let product=before["products"].as_array().unwrap().iter()
        .find(|p|p["active"]!=false&&
            p.get("deletedAt").is_none_or(Value::is_null)&&
            p.get("supersededBy").is_none_or(Value::is_null)&&
            p["catalog"]!="DEMO"&&
            p["variants"].as_array().is_some_and(|v|v.len()==1))
        .expect("real catalog must have an editable independent SKU");
    let variant=&product["variants"][0];
    let id=product["id"].as_str().unwrap();
    let vid=variant["id"].as_str().unwrap();
    let original_price=variant["wholesale"].as_i64().unwrap_or(0);
    let revised_price=original_price+137;
    let original_count=before["products"].as_array().unwrap().len();
    let category=product["categories"].as_array().map(|items|items.iter()
        .filter_map(Value::as_str).collect::<Vec<_>>().join(" / "))
        .unwrap_or_default();
    let value=|v:&Value,key:&str|v[key].as_str().unwrap_or("").to_owned();
    let group=if product["catalog"]=="BANDOMALL"{"반도몰"}else{"반도스탠드"};
    let origin=if product["origin"]=="STORE"{"매장"}else{"공장"};
    let price=|key:&str|variant[key].as_i64().map(|n|n.to_string())
        .unwrap_or_default();
    let source=json!({
        "revision":before["revision"],
        "products":[{"row":2,"cells":[
            id,"",value(product,"name"),value(product,"subtitle"),
            "",group,origin,category,value(product,"brandId"),
            value(product,"specification"),value(product,"sellingUnit"),
            value(product,"defaultColor"),
            if product["active"]==false{"중지"}else{"사용"},"정상"
        ]}],
        "options":[{"row":2,"cells":[
            id,"",vid,value(variant,"color"),value(variant,"option"),
            revised_price.to_string(),price("semi"),price("retail"),
            value(variant,"source"),value(variant,"siteOptionId")
        ]}]
    });
    let preview=bando_data_access::preview_unified_product_workbook(
        &before,&source
    ).expect("use authoritative native Rust preview for parsed Excel rows");
    assert!(preview["rows"].as_array().unwrap().iter().all(|r|
        r["status"]!="ERROR"&&r["status"]!="HOLD"),
        "valid price edit must not be rejected: {}",preview["rows"]);
    assert_eq!(preview["updatedProducts"],1);
    assert_eq!(get_snapshot(&conn).unwrap(),before,
        "preview cannot modify any SQLite rows");
    // Exact backup contract from React: backup before the product writer.
    let backup=backup_manager::manual_backup(&conn,&isolated.root)
        .expect("full manual SQLite backup must succeed");
    let backed=Connection::open_with_flags(
        &backup,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(get_snapshot(&backed).unwrap(),before);
    drop(backed);
    let req=json!({
        "expectedRevision":before["revision"],
        "action":"apply_product_workbook",
        "data":{"preview":preview}
    });
    let saved=bando_data_access::patch_product_v2(&mut conn,&req)
        .expect("write approved Excel through same native Rust CAS");
    assert_eq!(saved["revision"].as_u64(),
        before["revision"].as_u64().map(|n|n+1));
    let after=get_snapshot(&conn).unwrap();
    assert_eq!(after["products"].as_array().unwrap().len(),original_count,
        "Excel must preserve other products absent from the workbook");
    for key in ["orders","clients","workflow","aliases","shipping"]{
        assert_eq!(after[key],before[key],
            "product Excel cannot modify unrelated {key}");
    }
    let edited=after["products"].as_array().unwrap().iter()
        .find(|p|p["id"]==id).unwrap();
    assert_eq!(edited["variants"][0]["id"],vid);
    assert_eq!(edited["variants"][0]["wholesale"],revised_price);
    let stale=bando_data_access::patch_product_v2(&mut conn,&req)
        .expect_err("stale XLSX approval must not overwrite latest database");
    assert!(stale.contains("리비전")||stale.contains("새로고침"),
        "unexpected stale-write error: {stale}");
    assert_eq!(get_snapshot(&conn).unwrap(),after);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite"))
        .expect("open the same isolated v2 SQLite after commit");
    assert_eq!(get_snapshot(&reopened).unwrap(),after);
}

/// Alias UI -> shared Rust patch_product_v2 -> original catalog_clients::save_alias
/// -> revision-checked SQLite writer. No fake backend or operating DB mutation.
#[test]
fn native_alias_sqlite_save_correction_and_stale_editor_rejection() {
    let (isolated,mut conn)=IsolatedDatabase::open();
    let before=get_snapshot(&conn).expect("production SQLite initial snapshot");
    let p=before["products"].as_array().unwrap().iter()
        .find(|p|p["active"]!=false&&
            p["variants"].as_array().is_some_and(|v|v.len()==1))
        .expect("registered seeded product");
    let pid=p["id"].as_str().unwrap();
    let revision=before["revision"].as_u64().unwrap();
    let request=json!({
        "expectedRevision":revision,"action":"save_alias",
        "data":{"record":{
            "id":"","text":"별칭통합검증_가상표기_001",
            "productId":pid,"clientId":null
        }}
    });
    let response=bando_data_access::patch_product_v2(&mut conn,&request)
        .expect("the shared Rust product writer must save the alias");
    let first=get_snapshot(&conn).unwrap();
    assert_eq!(response["revision"],json!(revision+1));
    assert_eq!(first["revision"],response["revision"]);
    let record=first["aliases"].as_array().unwrap().iter()
        .find(|v|v["text"]=="별칭통합검증_가상표기_001").unwrap();
    let alias_id=record["id"].as_str().unwrap().to_owned();
    assert_eq!(record["productId"],pid);
    assert_eq!(record["variantId"],p["variants"][0]["id"]);
    assert!(record["clientId"].is_null());
    assert!(record["createdAt"].is_string());
    assert_eq!(first["products"],before["products"]);
    assert_eq!(first["orders"],before["orders"]);

    // Edit same alias ID without creating a duplicate row.
    let correction=json!({
        "expectedRevision":revision+1,"action":"save_alias",
        "data":{"record":{
            "id":alias_id,
            "text":"별칭통합검증_수정_002",
            "productId":pid,"clientId":null
        }}
    });
    bando_data_access::patch_product_v2(&mut conn,&correction).unwrap();
    let corrected=get_snapshot(&conn).unwrap();
    assert_eq!(corrected["aliases"].as_array().unwrap().len(),
        before["aliases"].as_array().unwrap().len()+1);
    assert_eq!(corrected["aliases"].as_array().unwrap()
        .iter().find(|v|v["id"]==alias_id).unwrap()["text"],
        "별칭통합검증_수정_002");

    // Another alias editor carrying the obsolete revision is refused.
    let error=bando_data_access::patch_product_v2(&mut conn,&request)
        .expect_err("stale alias editor must never overwrite a newer alias");
    assert!(error.contains("다른 작업에서 상품 데이터가 수정"));
    assert_eq!(get_snapshot(&conn).unwrap(),corrected,
        "failed stale write must leave revisions, aliases and orders untouched");
    drop(conn);

    let reopen=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopen).unwrap(),corrected);
    drop(reopen);
}


/// Brand editor -> shared Rust patch_product_v2 -> original catalog_clients::upsert_brand
/// -> revision CAS -> real v2 SQLite persistence/reopen. No production DB.
#[test]
fn native_brand_sqlite_save_correction_collision_and_reopen() {
    let (isolated,mut conn)=IsolatedDatabase::open();
    let before=get_snapshot(&conn).expect("real SQLite initial snapshot");
    let revision=before["revision"].as_u64().unwrap();
    let request=json!({
        "expectedRevision":revision,
        "action":"upsert_brand",
        "data":{
            "editing":null,
            "name":"SYNTHETIC_BRAND_20261120_ORIGINAL",
            "korean":"검증용 상표",
            "aliases":"검증상표, UNIQUEALIAS77, 검증상표",
            "evidence":"https://example.com/verified-brand"
        }
    });
    let response=bando_data_access::patch_product_v2(&mut conn,&request)
        .expect("save using the actual Rust brand editor and SQLite writer");
    assert_eq!(response["revision"],json!(revision+1));
    let created=get_snapshot(&conn).unwrap();
    assert_eq!(created["brands"].as_array().unwrap().len(),
        before["brands"].as_array().unwrap().len()+1);
    let brand=created["brands"].as_array().unwrap().iter()
        .find(|b|b["name"]=="SYNTHETIC_BRAND_20261120_ORIGINAL").unwrap();
    let id=brand["id"].as_str().unwrap().to_owned();
    assert!(!id.is_empty());
    assert_eq!(brand["korean"],"검증용 상표");
    assert_eq!(brand["aliases"],json!(["검증상표","UNIQUEALIAS77"]));
    assert_eq!(brand["evidenceUrl"],"https://example.com/verified-brand");
    for key in ["products","aliases","orders","clients","settings"]{
        assert_eq!(created[key],before[key],"brand insert must not mutate {key}");
    }
    drop(conn);

    // Fresh process-like connection reads the exact new brand and references.
    let mut reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),created);

    let corrected_request=json!({
        "expectedRevision":revision+1,
        "action":"upsert_brand",
        "data":{
            "editing":id,
            "name":"SYNTHETIC_BRAND_20261120_CORRECTED",
            "korean":"수정된 한글명",
            "aliases":"변경 별칭, EXTRAALIAS",
            "evidence":"https://example.com/updated-evidence"
        }
    });
    bando_data_access::patch_product_v2(&mut reopened,&corrected_request)
        .expect("correct existing brand without changing its ID");
    let corrected=get_snapshot(&reopened).unwrap();
    assert_eq!(corrected["brands"].as_array().unwrap().len(),
        created["brands"].as_array().unwrap().len());
    let brand=corrected["brands"].as_array().unwrap().iter()
        .find(|b|b["id"]==id).expect("same id after correction");
    assert_eq!(brand["name"],"SYNTHETIC_BRAND_20261120_CORRECTED");
    assert_eq!(brand["aliases"],json!(["변경 별칭","EXTRAALIAS"]));
    for key in ["products","aliases","orders","clients","settings"]{
        assert_eq!(corrected[key],before[key],"brand edit must not mutate {key}");
    }

    // Invalid evidence and duplicate names are rejected before write_snapshot.
    let invalid=json!({
        "expectedRevision":revision+2,
        "action":"upsert_brand",
        "data":{"editing":id,"name":"SYNTHETIC_BRAND_20261120_CORRECTED",
            "korean":"수정된 한글명","aliases":"변경 별칭",
            "evidence":"http://example.com/not-https"}
    });
    let error=bando_data_access::patch_product_v2(&mut reopened,&invalid)
        .expect_err("brand requires verified HTTPS evidence");
    assert!(error.contains("HTTPS"));
    assert_eq!(get_snapshot(&reopened).unwrap(),corrected);

    let duplicate=json!({
        "expectedRevision":revision+2,
        "action":"upsert_brand",
        "data":{"editing":null,"name":"ANOTHER_NEW_BRAND_20261120",
            "korean":"새 브랜드","aliases":"EXTRAALIAS",
            "evidence":"https://example.com/other"}
    });
    let error=bando_data_access::patch_product_v2(&mut reopened,&duplicate)
        .expect_err("shared alias cannot silently merge two brands");
    assert!(error.contains("중복됩니다"));
    assert_eq!(get_snapshot(&reopened).unwrap(),corrected);

    let stale=bando_data_access::patch_product_v2(&mut reopened,&request)
        .expect_err("stale brand editor must not overwrite newer state");
    assert!(stale.contains("다른 작업에서 상품 데이터가 수정되었습니다"));
    assert_eq!(get_snapshot(&reopened).unwrap(),corrected);
    drop(reopened);
    let after_restart=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&after_restart).unwrap(),corrected);
}

#[test]
fn native_workflow_freight_edit_normalizes_remote_and_reopens_sqlite(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let before=get_snapshot(&conn).unwrap();
    let source=stage_three(&before,"POSTPAID");
    let saved=write_snapshot(&mut conn,source,0,true)
        .expect("stage-three workflow fixture must persist");
    let decision=rust("apply_workflow_action",json!({
        "state":saved,"action":"set_shipment_freight",
        "groupId":"group:synthetic","shipmentId":"shipment:synthetic",
        "origin":"FACTORY","appliedCharge":6000,"remoteChecked":true,
        "eventId":"event:workflow-freight-edit",
        "now":"2026-11-13T12:45:00+09:00","actor":"검증 담당자"
    })).expect("native workflow freight edit must be accepted");
    let changed=decision["state"].clone();
    assert_eq!(changed["workflow"]["shipments"][0]["charges"]["FACTORY"],3000);
    assert_eq!(changed["orders"][0]["recipients"][0]["shippingCharges"]["FACTORY"],3000);
    assert_eq!(changed["orders"][0]["recipients"][0]["remoteDelivery"]["checked"],true);
    assert_eq!(changed["workflow"]["shipments"][0]["step"],1);
    assert_eq!(changed["workflow"]["groups"][0]["needsNotice"],true);
    rust("validate_workflow",json!({
        "state":changed,"before":saved
    })).expect("freight edit, stage rollback and audit must validate");
    let committed=write_snapshot(
        &mut conn,changed,saved["revision"].as_u64().unwrap(),true
    ).expect("freight edit must persist through the authoritative SQLite transaction");
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);
    assert_eq!(committed["workflow"]["shipments"][0]["charges"]["FACTORY"],3000);
    drop(reopened);
}
