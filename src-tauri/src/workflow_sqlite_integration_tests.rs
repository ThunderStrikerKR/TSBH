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
    let product_name=initial["products"][0]["name"].as_str().unwrap().to_owned();
    let product_color=initial["products"][0]["variants"][0]["color"].as_str().unwrap().to_owned();
    assert_eq!(initial["products"][0]["variants"].as_array().unwrap().len(),1);
    // The seed is intentionally catalog-only. Give this isolated fixture one
    // valid priced variant and one existing operational client.
    initial["products"][0]["variants"][0]["wholesale"]=json!(30000);
    initial["products"][0]["variants"][0]["retail"]=json!(30000);
    // Reproduce a retail-price dealer requesting AGENCY shipment, with
    // no house sender information registered anywhere in settings.
    for key in ["senderAddress","senderPhone","senderMobile","senderPostcode"]{
        initial["settings"][key]=json!("");
    }
    let client=json!({
        "id":"client:unconfirmed-draft",
        "name":"합성 미확정 거래처",
        "tier":"retail",
        "payment":"POSTPAID",
        "shippingFeeExcluded":false,
        "defaultApplyVat":false,
        "address":"서울특별시 강남구 테헤란로 123",
        "phone":"02-1234-5678",
        "mobile":"",
        "preferredAccountId":"account:lifecycle",
        "overrides":{},
        "active":true
    });
    initial["clients"].as_array_mut().unwrap().push(client.clone());
    initial["settings"]["accounts"]=json!([{"id":"account:lifecycle","bank":"가상은행","holder":"합성 계좌","number":"123-456"}]);
    initial["settings"]["defaultAccountId"]=json!("account:lifecycle");
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
                "name":product_name,
                "color":product_color,
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
    assert_eq!(confirmed["orders"][0]["orderType"],"AGENCY");
    assert_eq!(confirmed["orders"][0]["senderSnapshot"]["id"],client["id"]);
    assert_eq!(confirmed["orders"][0]["senderSnapshot"]["address"],client["address"]);
    assert_eq!(confirmed["orders"][0]["senderSnapshot"]["phone"],client["phone"]);
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
    let mut reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);
    assert_eq!(committed["workflow"]["shipments"][0]["step"],1);

    // Continue the exact same order, recipient and shipment through billing,
    // real bank evidence, export finalization, reply and daily settlement.
    let shipment_id=committed["workflow"]["shipments"][0]["id"].clone();
    let group_id=committed["workflow"]["groups"][0]["id"].clone();
    let persist=|conn:&mut Connection, before:&Value, next:Value| {
        rust("validate_workflow",json!({"state":next,"before":before}))
            .expect("each lifecycle transition must preserve workflow invariants");
        let revision=before["revision"].as_u64().unwrap();
        let saved=write_snapshot(conn,next,revision,true).unwrap();
        assert_eq!(saved["revision"],revision+1);
        assert_eq!(get_snapshot(conn).unwrap(),saved);
        assert_eq!(saved["orders"].as_array().unwrap().len(),1);
        assert_eq!(saved["orders"][0]["id"],"order:unconfirmed-draft");
        assert_eq!(saved["orders"][0]["recipients"][0]["id"],"recipient:unconfirmed-draft");
        assert_eq!(saved["workflow"]["shipments"][0]["id"],shipment_id);
        saved
    };
    let output=|operation:&str,payload:Value| {
        business_core::execute("output",operation,payload)
            .unwrap_or_else(|error|panic!("{operation}: {error}"))
    };
    let action=|payload:Value| rust("apply_workflow_action",payload)
        .unwrap_or_else(|error|panic!("lifecycle workflow action: {error}"));

    let freight=action(json!({"state":committed,"action":"set_shipment_freight",
        "shipmentId":shipment_id,"origin":"FACTORY","value":3000,
        "eventId":"lifecycle:freight","now":at,"actor":"검증 담당자"}));
    let mut saved=persist(&mut reopened,&committed,freight["state"].clone());
    for step in 2..=3 {
        let result=action(json!({"state":saved,"action":"advance_group","groupId":group_id,
            "eventId":format!("lifecycle:step:{step}"),"now":at,"actor":"검증 담당자",
            "noticeId":"lifecycle:notice","paymentMessage":"가상 거래처 입금 안내"}));
        assert_eq!(result["step"],step);
        saved=persist(&mut reopened,&saved,result["state"].clone());
    }
    let totals=rust("calculate_shipment_totals",json!({"state":saved,"shipmentId":shipment_id})).unwrap();
    assert_eq!(totals["subtotal"],30000);
    assert_eq!(totals["shipping"],3000);
    assert_eq!(totals["total"],33000);
    let payment=action(json!({"state":saved,"action":"record_payment","groupId":group_id,
        "paidAmount":33000,"paidAt":at,"allocations":[{"shipmentId":shipment_id,"amount":33000}],
        "allocationIds":["lifecycle:allocation"],"paymentId":"lifecycle:payment",
        "eventId":"lifecycle:cash","now":at,"actor":"검증 담당자"}));
    saved=persist(&mut reopened,&saved,payment["state"].clone());
    let approval=action(json!({"state":saved,"action":"advance_group","groupId":group_id,
        "eventId":"lifecycle:approval","now":at,"actor":"검증 담당자"}));
    assert_eq!(approval["step"],4);
    saved=persist(&mut reopened,&saved,approval["state"].clone());
    let queued=action(json!({"state":saved,"action":"process_group","groupId":group_id,
        "eventId":"lifecycle:queue","now":at,"actor":"검증 담당자"}));
    assert_eq!(queued["entries"].as_array().unwrap().len(),1);
    let queue_id=queued["entries"][0]["id"].clone();
    saved=persist(&mut reopened,&saved,queued["state"].clone());
    let preview=output("prepare_factory_export",json!({"state":saved,"ids":[queue_id]}));
    assert_eq!(preview["rows"][0]["values"][11],client["name"]);
    assert_eq!(preview["rows"][0]["values"][13],client["address"]);
    assert_eq!(preview["rows"][0]["values"][14],client["phone"]);
    assert_eq!(get_snapshot(&reopened).unwrap(),saved);
    // This tests the native export ledger boundary, not XLSX file bytes.
    let exported=output("finalize_export",json!({"state":saved,"preview":preview,
        "origin":"FACTORY","savedPath":"C:/synthetic/lifecycle.xlsx","now":at,
        "exportId":"lifecycle:export","eventId":"lifecycle:export-event"}));
    saved=persist(&mut reopened,&saved,exported["next"].clone());
    assert_eq!(saved["workflow"]["queue"][0]["status"],"EXPORTED");
    let report=|state:&Value| {
        let history=rust("list_order_history",json!({"state":state,"query":"","date":"2026-11-25"})).unwrap();
        assert_eq!(history.as_array().unwrap().len(),1);
        assert_eq!(history[0]["shipmentIds"],json!([shipment_id]));
        (output("prepare_settlement",json!({"state":state,"date":"2026-11-25","historyRows":history})),history)
    };
    let (before_reply,history)=report(&saved);
    assert_eq!(history[0]["status"],"엑셀 추출 완료");
    assert_eq!(before_reply["freightUnknown"],1);
    assert!(before_reply["orderRows"][0]["freightCost"].is_null());
    let reply=rust("analyze_factory_reply",json!({"state":saved,"sourceRowCount":1,"sourceRows":[{
        "row":2,"name":"김테스트","address":"서울특별시 강남구 테헤란로 123, 4층",
        "phone":"010-1234-5678","mobile":"","rawFreight":"4,000","tracking":"001234567890",
        "trackingNumeric":false,"reservedJ":false,"extraAfterQ":false}],
        "id":"lifecycle:reply","filename":"reply.xlsx","businessDate":"2026-11-25",
        "at":at,"actor":"검증 담당자"})).unwrap();
    assert_eq!(reply["applied"],1);
    assert_eq!(get_snapshot(&reopened).unwrap(),saved);
    let replied=rust("apply_factory_reply",json!({"state":saved,"preview":reply,"freightAt":at,
        "freightEventIds":["lifecycle:actual"],"replyEventIds":["lifecycle:reply-event"]})).unwrap();
    assert_eq!(replied["applied"],1);
    assert_eq!(replied["held"],0);
    saved=persist(&mut reopened,&saved,replied["next"].clone());
    assert_eq!(saved["workflow"]["shipments"][0]["charges"]["FACTORY"],3000);
    assert_eq!(saved["workflow"]["shipments"][0]["actualFreight"]["FACTORY"],4000);
    assert_eq!(saved["workflow"]["shipments"][0]["tracking"]["FACTORY"],"001234567890");
    assert_eq!(saved["workflow"]["payments"][0]["amount"],33000);
    let (settled,history)=report(&saved);
    assert_eq!(history[0]["status"],"공장 회신 완료");
    assert_eq!(settled["billedKnown"],33000);
    assert_eq!(settled["shippingKnown"],3000);
    assert_eq!(settled["paidCash"],33000);
    assert_eq!(settled["outstandingKnown"],0);
    assert_eq!(settled["actualFreightKnown"],4000);
    assert_eq!(settled["freightUnknown"],0);
    assert_eq!(get_snapshot(&reopened).unwrap(),saved);
    drop(reopened);
    let final_reopen=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    let final_state=get_snapshot(&final_reopen).unwrap();
    assert_eq!(final_state,saved);
    assert_eq!(report(&final_state).0,settled);
    drop(final_reopen);
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


/// Persist price history through real SQLite and rehydrate the same catalog
/// projection used by egui after a restart. No operating DB is accessed.
#[test]
fn native_product_price_history_cell_detail_reopen_and_stale_guard(){
    fn find<'a>(state:&'a Value,id:&str)->&'a Value{
        state["products"].as_array().unwrap().iter()
            .find(|p|p["id"].as_str()==Some(id)).unwrap()
    }
    let (isolated,mut conn)=IsolatedDatabase::open();
    let before=get_snapshot(&conn).unwrap();
    let product=before["products"].as_array().unwrap().iter()
        .find(|p|p["active"]!=false&&p.get("deletedAt").is_none_or(Value::is_null)
            &&p.get("supersededBy").is_none_or(Value::is_null)&&p["catalog"]!="DEMO"
            &&p["variants"].as_array().is_some_and(|v|v.len()==1))
        .expect("editable independent product");
    let id=product["id"].as_str().unwrap();
    let vid=product["variants"][0]["id"].as_str().unwrap();
    let old=product["variants"][0]["wholesale"].clone();
    let first=old.as_i64().unwrap_or(0)+137;
    let previous=product["priceHistory"].as_array().cloned().unwrap_or_default();
    let request=json!({"expectedRevision":before["revision"],"action":"set_price",
        "data":{"productId":id,"variantId":vid,"tier":"wholesale",
            "price":first,"expected":old}});
    bando_data_access::patch_product_v2(&mut conn,&request).expect("cell price save");
    let cell=get_snapshot(&conn).unwrap();
    let history=find(&cell,id)["priceHistory"].as_array().unwrap();
    assert_eq!(history.len(),previous.len()+1);
    assert_eq!(&history[..previous.len()],previous.as_slice());
    let entry=history.last().unwrap();
    assert_eq!(entry["field"],format!("{vid}.wholesale"));
    assert_eq!(entry["before"],old);
    assert_eq!(entry["after"],first);
    assert_eq!(entry["source"],"MANUAL");
    assert!(chrono::DateTime::parse_from_rfc3339(entry["at"].as_str().unwrap()).is_ok());

    let mut draft=find(&cell,id).clone();
    draft["variants"][0]["wholesale"]=json!(first+137);
    let old_retail=draft["variants"][0]["retail"].clone();
    let retail=old_retail.as_i64().unwrap_or(0)+139;
    draft["variants"][0]["retail"]=json!(retail);
    let detail=json!({"expectedRevision":cell["revision"],"action":"upsert_product",
        "data":{"product":draft}});
    bando_data_access::patch_product_v2(&mut conn,&detail).expect("detail price save");
    let saved=get_snapshot(&conn).unwrap();
    let history=find(&saved,id)["priceHistory"].as_array().unwrap();
    assert_eq!(history.len(),previous.len()+3);
    assert_eq!(&history[..previous.len()+1],
        find(&cell,id)["priceHistory"].as_array().unwrap().as_slice());
    assert_eq!(history[previous.len()+1]["before"],first);
    assert_eq!(history[previous.len()+1]["after"],first+137);
    assert_eq!(history[previous.len()+2]["field"],format!("{vid}.retail"));
    assert_eq!(history[previous.len()+2]["before"],old_retail);
    assert_eq!(history[previous.len()+2]["after"],retail);
    assert!(bando_data_access::patch_product_v2(&mut conn,&request).is_err());
    assert_eq!(get_snapshot(&conn).unwrap(),saved);

    let same=json!({"expectedRevision":saved["revision"],"action":"set_price",
        "data":{"productId":id,"variantId":vid,"tier":"wholesale",
            "price":first+137,"expected":first+137}});
    bando_data_access::patch_product_v2(&mut conn,&same).expect("same price save");
    let unchanged=get_snapshot(&conn).unwrap();
    assert_eq!(find(&unchanged,id)["priceHistory"],find(&saved,id)["priceHistory"]);
    for key in ["orders","clients","workflow","aliases","shipping"]{
        assert_eq!(unchanged[key],before[key],"unrelated {key} must stay unchanged");
    }
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),unchanged);
    let catalog=bando_data_access::read_v2_catalog_snapshot(&reopened).unwrap();
    assert_eq!(find(&catalog,id)["priceHistory"],find(&saved,id)["priceHistory"]);
    assert_eq!(find(&catalog,id)["variants"],find(&saved,id)["variants"]);
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
    let previous=product["priceHistory"].as_array().cloned().unwrap_or_default();
    let history=edited["priceHistory"].as_array().expect("Excel price history");
    assert_eq!(history.len(),previous.len()+1,"no duplicate preview entry");
    assert_eq!(&history[..previous.len()],previous.as_slice());
    let entry=history.last().unwrap();
    assert_eq!(entry["field"],format!("{vid}.wholesale"));
    assert_eq!(entry["before"],variant["wholesale"]);
    assert_eq!(entry["after"],revised_price);
    assert_eq!(entry["source"],"MANUAL_EXCEL");
    assert!(chrono::DateTime::parse_from_rfc3339(entry["at"].as_str().unwrap()).is_ok());
    let stale=bando_data_access::patch_product_v2(&mut conn,&req)
        .expect_err("stale XLSX approval must not overwrite latest database");
    assert!(stale.contains("리비전")||stale.contains("새로고침"),
        "unexpected stale-write error: {stale}");
    assert_eq!(get_snapshot(&conn).unwrap(),after);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite"))
        .expect("open the same isolated v2 SQLite after commit");
    assert_eq!(get_snapshot(&reopened).unwrap(),after);
    let catalog=bando_data_access::read_v2_catalog_snapshot(&reopened).unwrap();
    let restored=catalog["products"].as_array().unwrap().iter()
        .find(|p|p["id"]==id).unwrap();
    assert_eq!(restored["priceHistory"],edited["priceHistory"]);
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

#[test]
fn scoped_freight_reconciliation_matches_full_for_target_and_preserves_other_order(){
    let (_isolated,conn)=IsolatedDatabase::open();
    let before=get_snapshot(&conn).unwrap();
    let mut input=stage_three(&before,"POSTPAID");
    // A distinct, stale but structurally linked order must not get repaired
    // as a side effect of editing the first order's freight.
    let mut other=input["orders"][0].clone();
    other["id"]=json!("order:other");
    other["recipients"][0]["id"]=json!("recipient:other");
    other["recipients"][0]["items"][0]["id"]=json!("item:other");
    input["orders"].as_array_mut().unwrap().push(other);
    input["workflow"]["sources"].as_array_mut().unwrap().push(json!({
        "id":"source:other","orderId":"order:other","number":2,
        "dealerId":"synthetic-workflow-client"
    }));
    input["workflow"]["groups"].as_array_mut().unwrap().push(json!({
        "id":"group:order:other","sourceId":"source:other",
        "dealerId":"synthetic-workflow-client","notices":[],"needsNotice":false
    }));
    let mut ship=input["workflow"]["shipments"][0].clone();
    ship["id"]=json!("shipment:other");
    ship["orderId"]=json!("order:other");
    ship["recipientId"]=json!("recipient:other");
    ship["sourceId"]=json!("source:other");
    ship["groupId"]=json!("group:order:other");
    input["workflow"]["shipments"].as_array_mut().unwrap().push(ship);

    let at="2026-11-25T08:45:00+09:00";
    let scoped=rust("reconcile_shipment_workflow",json!({
        "state":&input,"shipmentId":"shipment:synthetic",
        "groupId":"group:synthetic","eventSeed":"scoped-test","now":at
    })).expect("scoped reconciliation");
    let full=rust("reconcile_workflow",json!({
        "state":&input,"now":at,
        "eventIds":(0..256).map(|i|format!("scoped-test:reconcile:{i}")).collect::<Vec<_>>()
    })).expect("full reconciliation");
    let next=&scoped["state"];
    assert_eq!(next["orders"][0],full["state"]["orders"][0]);
    assert_eq!(next["workflow"]["shipments"][0],full["state"]["workflow"]["shipments"][0]);
    assert_eq!(next["orders"][1],input["orders"][1]);
    assert_eq!(next["workflow"]["shipments"][1],input["workflow"]["shipments"][1]);
    assert_eq!(next["workflow"]["groups"][1],input["workflow"]["groups"][1]);
    assert_ne!(full["state"]["workflow"]["shipments"][1],input["workflow"]["shipments"][1]);
    let events=next["workflow"]["events"].as_array().unwrap();
    assert!(events.iter().skip(1).all(|event|
        event["id"].as_str().unwrap().starts_with("scoped-test:reconcile:") &&
        event["shipmentIds"].as_array().unwrap().iter().all(|id|id=="shipment:synthetic")));
    assert!(rust("reconcile_shipment_workflow",json!({
        "state":&input,"shipmentId":"shipment:synthetic",
        "groupId":"group:order:other","eventSeed":"bad","now":at
    })).is_err());
    // Compare the scoped native output against the corresponding entries
    // of a full board projection. Both receive the same unfiltered state.
    let full_board=rust("preview_workflow",json!({"state":next})).unwrap();
    let scoped_board=rust("preview_workflow",json!({
        "state":next,"groupIds":["group:synthetic"]
    })).unwrap();
    let expected=full_board["groups"].as_array().unwrap().iter()
        .filter(|row|row["groupId"]=="group:synthetic").cloned().collect::<Vec<_>>();
    assert_eq!(scoped_board["groups"],json!(expected));
    assert_eq!(expected.len(),1);
    let full_shipments=rust("read_shipments",json!({"state":next})).unwrap();
    let scoped_shipments=rust("read_shipments",json!({
        "state":next,"groupIds":["group:synthetic"]
    })).unwrap();
    let expected=full_shipments["shipments"].as_array().unwrap().iter()
        .filter(|row|row["shipmentId"]=="shipment:synthetic").cloned().collect::<Vec<_>>();
    assert_eq!(scoped_shipments["shipments"],json!(expected));
    assert_eq!(expected.len(),1);
    assert_eq!(get_snapshot(&conn).unwrap(),before);

    // Group moves use this same order-scoped reconciliation. A split
    // sibling in another group must be reconciled, but an unrelated order
    // must remain unchanged even when its own stale state is repairable.
    let mut split=input.clone();
    let mut recipient=split["orders"][0]["recipients"][0].clone();
    recipient["id"]=json!("recipient:scope-sibling");
    recipient["items"][0]["id"]=json!("item:scope-sibling");
    split["orders"][0]["recipients"].as_array_mut().unwrap().push(recipient);
    let mut sibling_group=split["workflow"]["groups"][0].clone();
    sibling_group["id"]=json!("group:scope-sibling");
    split["workflow"]["groups"].as_array_mut().unwrap().push(sibling_group);
    let mut sibling=split["workflow"]["shipments"][0].clone();
    sibling["id"]=json!("shipment:scope-sibling");
    sibling["recipientId"]=json!("recipient:scope-sibling");
    sibling["groupId"]=json!("group:scope-sibling");
    split["workflow"]["shipments"].as_array_mut().unwrap().push(sibling);
    let scope=rust("reconcile_shipment_workflow",json!({
        "state":&split,"shipmentId":"shipment:synthetic",
        "groupId":"group:synthetic","eventSeed":"group-scope","now":at
    })).unwrap();
    let all=rust("reconcile_workflow",json!({
        "state":&split,"now":at,
        "eventIds":(0..256).map(|i|format!("group-full:{i}")).collect::<Vec<_>>()
    })).unwrap();
    let scoped=&scope["state"];
    assert_eq!(scoped["orders"][0],all["state"]["orders"][0]);
    for (field,index) in [("shipments",0usize),("shipments",2),("groups",0),("groups",2)]{
        assert_eq!(scoped["workflow"][field][index],all["state"]["workflow"][field][index]);
    }
    assert_eq!(scoped["orders"][1],split["orders"][1]);
    assert_eq!(scoped["workflow"]["shipments"][1],split["workflow"]["shipments"][1]);
    assert_eq!(scoped["workflow"]["groups"][1],split["workflow"]["groups"][1]);
    let moved=rust("apply_workflow_action",json!({
        "state":scoped,"action":"regroup_shipment",
        "shipmentId":"shipment:scope-sibling","groupId":"group:scope-sibling",
        "targetGroupId":"group:synthetic","ack":true,
        "eventId":"group-scope:regroup","now":at
    })).unwrap()["state"].clone();
    assert_eq!(moved["workflow"]["shipments"][2]["groupId"],"group:synthetic");
    assert_eq!(moved["orders"][1],split["orders"][1]);
    assert_eq!(moved["workflow"]["shipments"][1],split["workflow"]["shipments"][1]);
    assert_eq!(moved["workflow"]["groups"][1],split["workflow"]["groups"][1]);
    assert_eq!(get_snapshot(&conn).unwrap(),before);
}

#[test]
fn native_snapshot_update_reuses_authoritative_before_image_and_rolls_back(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let initial=get_snapshot(&conn).unwrap();
    let fixture=stage_three(&initial,"POSTPAID");
    let saved=write_snapshot(&mut conn,fixture,0,true).unwrap();
    let revision=saved["revision"].as_u64().unwrap();
    let mut calls=0;
    let committed=bando_data_access::update_v2_snapshot(&mut conn,revision,true,|current|{
        calls+=1;
        assert_eq!(current,&saved);
        let result=rust("apply_workflow_action",json!({
            "state":current,"action":"set_shipment_freight",
            "groupId":"group:synthetic","shipmentId":"shipment:synthetic",
            "origin":"FACTORY","appliedCharge":4000,
            "eventId":"event:transaction-freight","now":"2026-11-25T09:00:00+09:00",
            "actor":"검증 담당자"
        }))?;
        let changed=result["state"].clone();
        rust("validate_workflow",json!({"state":&changed,"before":current}))?;
        Ok(changed)
    }).expect("transactional freight update");
    assert_eq!(calls,1);
    assert_eq!(committed["revision"],revision+1);
    assert_eq!(committed["workflow"]["shipments"][0]["charges"]["FACTORY"],4000);
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    let stale=bando_data_access::update_v2_snapshot(&mut conn,revision,true,|_|{
        panic!("stale revision must be rejected before the reducer runs")
    });
    assert!(stale.is_err());
    let rejected=bando_data_access::update_v2_snapshot(&mut conn,revision+1,true,|_|{
        Err("synthetic reducer failure".into())
    });
    assert!(rejected.is_err());
    let invalid=bando_data_access::update_v2_snapshot(&mut conn,revision+1,true,|current|{
        let mut state=current.clone();
        state["orders"]=Value::Null;
        Ok(state)
    });
    assert!(invalid.is_err());
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);
}


#[test]
fn freight_delta_skips_unchanged_sql_rows_and_legacy_indexes(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let initial=get_snapshot(&conn).unwrap();
    let fixture=stage_three(&initial,"POSTPAID");
    let saved=write_snapshot(&mut conn,fixture,0,true).unwrap();
    let revision=saved["revision"].as_u64().unwrap();
    // BEFORE INSERT fires even for an UPSERT that subsequently takes the
    // no-op WHERE branch. Count attempted SQL row writes, not just changes().
    conn.execute_batch("CREATE TEMP TABLE delta_probe(table_name TEXT,operation TEXT);").unwrap();
    for table in ["core_products","core_product_prices","core_clients","core_orders",
        "core_recipients","core_items","product_index","order_index"]{
        for operation in ["INSERT","UPDATE","DELETE"]{
            conn.execute_batch(&format!(
                "CREATE TEMP TRIGGER probe_{table}_{operation} BEFORE {operation} ON main.{table}
                 BEGIN INSERT INTO delta_probe VALUES ('{table}','{operation}'); END;"
            )).unwrap();
        }
    }
    let committed=bando_data_access::update_v2_snapshot(&mut conn,revision,true,|current|{
        let mut next=current.clone();
        next["orders"][0]["recipients"][0]["shippingCharges"]=json!({"FACTORY":4000});
        next["workflow"]["shipments"][0]["charges"]["FACTORY"]=json!(4000);
        Ok(next)
    }).expect("recipient-only delta");
    assert_eq!(committed["revision"],revision+1);
    assert_eq!(committed["orders"][0]["recipients"][0]["shippingCharges"]["FACTORY"],4000);
    let count:i64=conn.query_row("SELECT COUNT(*) FROM delta_probe WHERE table_name!='core_recipients'",
        [],|row|row.get(0)).unwrap();
    assert_eq!(count,0,"freight-only row edit must not attempt unrelated SQL");
    let count:i64=conn.query_row("SELECT COUNT(*) FROM delta_probe WHERE table_name='core_recipients' AND operation='UPDATE'",
        [],|row|row.get(0)).unwrap();
    assert_eq!(count,1);
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);
}

#[test]
fn scoped_freight_before_read_matches_full_reducer_and_preserves_ledger(){
    let (_isolated,mut conn)=IsolatedDatabase::open();
    let initial=get_snapshot(&conn).unwrap();
    let mut fixture=stage_three(&initial,"POSTPAID");
    // Clone all evidence IDs consistently, not only the mutable order ID.
    let mut other:Value=serde_json::from_str(&fixture.to_string()
        .replace("order:synthetic","order:other")
        .replace("recipient:synthetic","recipient:other")
        .replace("item:synthetic","item:other")
        .replace("source:synthetic","source:other")
        .replace("group:synthetic","group:other")
        .replace("shipment:synthetic","shipment:other")
        .replace("event:confirmation","event:other-confirmation")).unwrap();
    // Put the edited order LAST to exercise restoration of SQL ordering.
    fixture["orders"].as_array_mut().unwrap().insert(0,other["orders"].as_array_mut().unwrap().pop().unwrap());
    for key in ["sources","groups","shipments","events"]{
        let mut row=other["workflow"][key].as_array_mut().unwrap().pop().unwrap();
        if key=="sources"{row["number"]=json!(2);}
        fixture["workflow"][key].as_array_mut().unwrap().push(row);
    }
    let saved=write_snapshot(&mut conn,fixture,0,true).unwrap();
    let revision=saved["revision"].as_u64().unwrap();
    let reduce=|state:&Value|->Result<Value,String>{
        let reconciled=rust("reconcile_shipment_workflow",json!({
            "state":state,"shipmentId":"shipment:synthetic","groupId":"group:synthetic",
            "eventSeed":"before-read-reconcile","now":"2026-11-25T09:00:00+09:00"
        }))?;
        let result=rust("apply_workflow_action",json!({
            "state":reconciled["state"],"action":"set_shipment_freight",
            "shipmentId":"shipment:synthetic","groupId":"group:synthetic",
            "origin":"FACTORY","appliedCharge":4000,
            "eventId":"before-read-edit","now":"2026-11-25T09:00:00+09:00",
            "actor":"검증 담당자"
        }))?;
        Ok(result["state"].clone())
    };
    let mut expected=reduce(&saved).unwrap();
    rust("validate_workflow",json!({"state":&expected,"before":&saved})).unwrap();
    expected["revision"]=json!(revision+1);
    let committed=bando_data_access::update_v2_freight_snapshot(
        &mut conn,revision,"shipment:synthetic","group:synthetic",|projected|{
            assert_eq!(projected["orders"].as_array().unwrap().len(),1);
            assert_eq!(projected["orders"][0]["id"],"order:synthetic");
            assert_eq!(projected["workflow"],saved["workflow"]);
            reduce(projected)
        }).unwrap();
    assert_eq!(committed,expected);
    assert_eq!(committed["orders"][0],saved["orders"][0]);
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    // Reject stale revision and out-of-scope writes without touching SQLite.
    assert!(bando_data_access::update_v2_freight_snapshot(
        &mut conn,revision,"shipment:synthetic","group:synthetic",|_|panic!("stale reducer")).is_err());
    assert!(bando_data_access::update_v2_freight_snapshot(
        &mut conn,revision+1,"shipment:synthetic","group:synthetic",|projected|{
            let mut next=projected.clone();
            next["settings"]["operatorName"]=json!("invalid scoped mutation");
            Ok(next)
        }).is_err());
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
}

// Timings are diagnostic, never correctness thresholds on a shared CI VM.
#[test]
#[ignore = "explicit SQLite performance run, not a normal regression test"]
fn sqlite_freight_phase_benchmark(){
    use std::time::Instant;
    const SAMPLES:usize=7;
    fn median(values:&mut [f64])->f64{
        values.sort_by(f64::total_cmp);
        values[values.len()/2]
    }
    fn ms(start:Instant)->f64{start.elapsed().as_secs_f64()*1000.0}
    fn fixture(base:&Value,count:usize)->Value{
        let mut state=stage_three(base,"POSTPAID");
        let template=json!({"order":state["orders"][0],"workflow":state["workflow"]});
        for index in 1..count{
            let copy:Value=serde_json::from_str(&template.to_string()
                .replace("order:synthetic",&format!("order:bench-{index}"))
                .replace("recipient:synthetic",&format!("recipient:bench-{index}"))
                .replace("item:synthetic",&format!("item:bench-{index}"))
                .replace("source:synthetic",&format!("source:bench-{index}"))
                .replace("group:synthetic",&format!("group:bench-{index}"))
                .replace("shipment:synthetic",&format!("shipment:bench-{index}"))
                .replace("event:confirmation",&format!("event:bench-{index}"))).unwrap();
            state["orders"].as_array_mut().unwrap().push(copy["order"].clone());
            for key in ["sources","groups","shipments","events"]{
                let mut row=copy["workflow"][key][0].clone();
                if key=="sources"{row["number"]=json!(index+1);}
                state["workflow"][key].as_array_mut().unwrap().push(row);
            }
        }
        state
    }
    fn timed_update(conn:&mut Connection,revision:u64,scoped:bool,index:usize)
        ->(Value,[f64;4]){
        let start=Instant::now();
        let mut phases=[0.0;4];
        let reduce=|state:&Value|->Result<Value,String>{
            phases[0]=ms(start);
            let calc=Instant::now();
            let reconciled=rust("reconcile_shipment_workflow",json!({
                "state":state,"shipmentId":"shipment:synthetic","groupId":"group:synthetic",
                "eventSeed":format!("bench-reconcile-{index}"),"now":"2026-11-25T09:00:00+09:00"
            }))?;
            let result=rust("apply_workflow_action",json!({
                "state":reconciled["state"],"action":"set_shipment_freight",
                "shipmentId":"shipment:synthetic","groupId":"group:synthetic",
                "origin":"FACTORY","appliedCharge":if index%2==0{4000}else{5000},
                "eventId":format!("bench-edit-{index}"),"now":"2026-11-25T09:00:00+09:00",
                "actor":"검증 담당자"
            }))?;
            let next=result["state"].clone();
            phases[1]=ms(calc);
            Ok(next)
        };
        let saved=if scoped{
            bando_data_access::update_v2_freight_snapshot(
                conn,revision,"shipment:synthetic","group:synthetic",reduce).unwrap()
        }else{
            bando_data_access::update_v2_snapshot(conn,revision,true,reduce).unwrap()
        };
        phases[3]=ms(start);
        phases[2]=(phases[3]-phases[0]-phases[1]).max(0.0);
        (saved,phases)
    }
    for count in [1,30,100]{
        let (_full_dir,mut full)=IsolatedDatabase::open();
        let (_scoped_dir,mut scoped)=IsolatedDatabase::open();
        let source=fixture(&get_snapshot(&full).unwrap(),count);
        let initial=write_snapshot(&mut full,source.clone(),0,true).unwrap();
        assert_eq!(write_snapshot(&mut scoped,source,0,true).unwrap(),initial);
        let mut revision=initial["revision"].as_u64().unwrap();
        let mut full_times:[Vec<f64>;4]=std::array::from_fn(|_|Vec::new());
        let mut scoped_times:[Vec<f64>;4]=std::array::from_fn(|_|Vec::new());
        let mut reads=Vec::new();
        let mut validation=Vec::new();
        // Warm both connections once, then alternate measurement order.
        for index in 0..=SAMPLES{
            let ((expected,ft),(actual,st))=if index%2==0{
                let f=timed_update(&mut full,revision,false,index);
                let s=timed_update(&mut scoped,revision,true,index);
                (f,s)
            }else{
                let s=timed_update(&mut scoped,revision,true,index);
                let f=timed_update(&mut full,revision,false,index);
                (f,s)
            };
            assert_eq!(expected,actual,"full and scoped state must match");
            assert_eq!(actual["orders"].as_array().unwrap().len(),count);
            revision+=1;
            assert_eq!(actual["revision"],revision);
            let read_start=Instant::now();
            let readback=get_snapshot(&scoped).unwrap();
            let read_ms=ms(read_start);
            assert_eq!(readback,actual);
            let validation_start=Instant::now();
            rust("validate_workflow",json!({"state":actual,"before":actual})).unwrap();
            let validation_ms=ms(validation_start);
            if index>0{
                for phase in 0..4{
                    full_times[phase].push(ft[phase]);
                    scoped_times[phase].push(st[phase]);
                }
                reads.push(read_ms);
                validation.push(validation_ms);
            }
        }
        let f=full_times.each_mut().map(|v|median(v));
        let s=scoped_times.each_mut().map(|v|median(v));
        eprintln!("[sqlite-bench] orders={count} samples={SAMPLES} profile={} full_read_ms={:.3} full_reducer_ms={:.3} full_write_verify_commit_ms={:.3} full_total_ms={:.3} scoped_read_ms={:.3} scoped_reducer_ms={:.3} scoped_write_verify_commit_ms={:.3} scoped_total_ms={:.3} standalone_full_read_ms={:.3} standalone_validator_dispatch_ms={:.3}",
            if cfg!(debug_assertions){"debug"}else{"release"},
            f[0],f[1],f[2],f[3],s[0],s[1],s[2],s[3],median(&mut reads),median(&mut validation));
    }
}

#[test]
fn stable_freight_precommit_global_validation_rolls_back_unrelated_invalid_link(){
    let (_isolated,mut conn)=IsolatedDatabase::open();
    let initial=get_snapshot(&conn).unwrap();
    let saved=write_snapshot(&mut conn,stage_three(&initial,"POSTPAID"),0,true).unwrap();
    let revision=saved["revision"].as_u64().unwrap();
    // Inject an unrelated broken shipment link in the envelope only. The
    // existing global validator must reject this even though the freight
    // reducer only sees the selected order. No real operational DB is used.
    let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|row|row.get(0)).unwrap();
    let mut envelope:Value=serde_json::from_str(&raw).unwrap();
    let mut broken=envelope["workflow"]["shipments"][0].clone();
    broken["id"]=json!("shipment:broken");
    broken["orderId"]=json!("order:missing");
    envelope["workflow"]["shipments"].as_array_mut().unwrap().push(broken);
    conn.execute("UPDATE app_state SET payload=?1 WHERE id=1",[envelope.to_string()]).unwrap();
    let before=get_snapshot(&conn).unwrap();
    let rejected=bando_data_access::update_v2_freight_snapshot(
        &mut conn,revision,"shipment:synthetic","group:synthetic",|current|{
            let mut next=current.clone();
            next["orders"][0]["recipients"][0]["shippingCharges"]=json!({"FACTORY":4000});
            next["workflow"]["shipments"][0]["charges"]["FACTORY"]=json!(4000);
            Ok(next)
        });
    assert!(rejected.unwrap_err().contains("연결"));
    assert_eq!(get_snapshot(&conn).unwrap(),before);
}

#[test]
fn relational_workflow_event_migration_and_append_preserve_audit(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let initial=get_snapshot(&conn).unwrap();
    let saved=write_snapshot(&mut conn,stage_three(&initial,"POSTPAID"),0,true).unwrap();
    let backup=bando_data_access::workflow_events::migrate_with_backup(&mut conn,&isolated.root)
        .unwrap().expect("verified migration backup");
    let backup_conn=bando_data_access::backup_service::read_backup(std::path::Path::new(&backup)).unwrap();
    assert_eq!(get_snapshot(&backup_conn).unwrap(),saved);
    drop(backup_conn);
    let mut expected=saved.clone();
    let revision=saved["revision"].as_u64().unwrap()+1;
    expected["revision"]=json!(revision);
    assert_eq!(get_snapshot(&conn).unwrap(),expected);
    let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
    let physical:Value=serde_json::from_str(&raw).unwrap();
    assert!(physical.is_array());
    let envelope=&physical[0];
    let version:i64=conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)).unwrap();
    assert_eq!(version,3);
    assert!(envelope["workflow"].get("events").is_none());
    assert_eq!(envelope["_workflowEventsRelational"],1);
    // Reproduce the old v2 projector: it cannot hydrate a non-object root,
    // and its raw revision query cannot return an apparently valid revision.
    let mut old_projection=physical.clone();
    assert!(bando_data_access::hydrate_v2(&conn,&mut old_projection).is_err());
    let old_revision:Option<i64>=conn.query_row("SELECT json_extract(payload,'$.revision') FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
    assert!(old_revision.is_none());
    {
        let tx=conn.transaction().unwrap();
        tx.execute("UPDATE core_schema SET version=2 WHERE key='business-core'",[]).unwrap();
        assert!(get_snapshot(&tx).unwrap_err().contains("DB 버전 불일치"));
        tx.rollback().unwrap();
    }
    assert_eq!(envelope["_workflowEventsCount"],json!(saved["workflow"]["events"].as_array().unwrap().len()));
    assert!(bando_data_access::workflow_events::migrate_with_backup(&mut conn,&isolated.root).unwrap().is_none());
    let committed=bando_data_access::update_v2_freight_snapshot(
        &mut conn,revision,"shipment:synthetic","group:synthetic",|current|{
            let result=rust("apply_workflow_action",json!({
                "state":current,"action":"set_shipment_freight",
                "groupId":"group:synthetic","shipmentId":"shipment:synthetic",
                "origin":"FACTORY","appliedCharge":4000,"eventId":"split-audit",
                "now":"2026-11-25T09:00:00+09:00","actor":"검증 담당자"
            }))?;
            Ok(result["state"].clone())
        }).unwrap();
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    assert_eq!(committed["workflow"]["events"][0],saved["workflow"]["events"][0]);
    let count:i64=conn.query_row("SELECT COUNT(*) FROM core_workflow_events",[],|r|r.get(0)).unwrap();
    assert_eq!(count as usize,committed["workflow"]["events"].as_array().unwrap().len());
    // A missing final row leaves a contiguous sequence, so the physical
    // manifest count must independently reject tail loss and missing markers.
    {
        let tx=conn.transaction().unwrap();
        tx.execute("DELETE FROM core_workflow_events WHERE sequence=(SELECT MAX(sequence) FROM core_workflow_events)",[]).unwrap();
        assert!(get_snapshot(&tx).unwrap_err().contains("업무 이벤트 저장 개수 불일치"));
        tx.rollback().unwrap();
    }
    {
        let tx=conn.transaction().unwrap();
        tx.execute("UPDATE app_state SET payload=json_remove(payload,'$[0]._workflowEventsCount') WHERE id=1",[]).unwrap();
        assert!(get_snapshot(&tx).unwrap_err().contains("업무 이벤트 저장 개수"));
        tx.rollback().unwrap();
    }
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    let failed=bando_data_access::update_v2_snapshot(&mut conn,revision+1,true,|state|{
        let mut next=state.clone();
        next["workflow"]["events"][0]["action"]=json!("tampered");
        Ok(next)
    });
    assert!(failed.is_err());
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),committed);
    let mut reopened=reopened;
    let split_backup=bando_data_access::backup_service::manual_backup(&reopened,&isolated.root).unwrap();
    bando_data_access::backup_service::restore_backup(&mut reopened,&isolated.root,&split_backup).unwrap();
    let mut restored=committed.clone();
    restored["revision"]=json!(committed["revision"].as_u64().unwrap()+1);
    assert_eq!(get_snapshot(&reopened).unwrap(),restored);
    // Restore the original pre-migration backup, including its physical
    // schema, rather than merging its JSON into a split database.
    bando_data_access::backup_service::restore_backup(&mut reopened,&isolated.root,&backup).unwrap();
    let mut legacy=saved.clone();
    legacy["revision"]=json!(restored["revision"].as_u64().unwrap()+1);
    assert_eq!(get_snapshot(&reopened).unwrap(),legacy);
    let split_exists:bool=reopened.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='core_workflow_events')",[],|r|r.get(0)).unwrap();
    assert!(!split_exists);
    assert_eq!(reopened.query_row("SELECT version FROM core_schema WHERE key=\'business-core\'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
}

#[test]
fn relational_workflow_state_migration_and_delta_preserve_links(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let initial=get_snapshot(&conn).unwrap();
    let saved=write_snapshot(&mut conn,stage_three(&initial,"POSTPAID"),0,true).unwrap();
    assert!(bando_data_access::workflow_state::migrate_with_backup(&mut conn,&isolated.root).is_err());
    bando_data_access::workflow_events::migrate_with_backup(&mut conn,&isolated.root).unwrap();
    let event_split=get_snapshot(&conn).unwrap();
    let backup=bando_data_access::workflow_state::migrate_with_backup(&mut conn,&isolated.root).unwrap().unwrap();
    let backup_conn=bando_data_access::backup_service::read_backup(std::path::Path::new(&backup)).unwrap();
    assert_eq!(get_snapshot(&backup_conn).unwrap(),event_split);
    drop(backup_conn);
    let before=get_snapshot(&conn).unwrap();
    let revision=before["revision"].as_u64().unwrap();
    assert_eq!(before["workflow"],saved["workflow"]);
    let raw:String=conn.query_row("SELECT payload FROM app_state WHERE id=1",[],|r|r.get(0)).unwrap();
    let physical:Value=serde_json::from_str(&raw).unwrap();
    assert!(physical.is_array());
    let envelope=&physical[0];
    let version:i64=conn.query_row("SELECT version FROM core_schema WHERE key='business-core'",[],|r|r.get(0)).unwrap();
    assert_eq!(version,3);
    for key in ["groups","shipments","events"]{assert!(envelope["workflow"].get(key).is_none());}
    assert_eq!(envelope["_workflowStateRelational"]["shipments"],1);
    assert!(bando_data_access::workflow_state::migrate_with_backup(&mut conn,&isolated.root).unwrap().is_none());
    let committed=bando_data_access::update_v2_freight_snapshot(
        &mut conn,revision,"shipment:synthetic","group:synthetic",|current|{
            let result=rust("apply_workflow_action",json!({
                "state":current,"action":"set_shipment_freight",
                "groupId":"group:synthetic","shipmentId":"shipment:synthetic",
                "origin":"FACTORY","appliedCharge":4000,"eventId":"split-state-freight",
                "now":"2026-11-25T09:00:00+09:00","actor":"검증 담당자"
            }))?;
            Ok(result["state"].clone())
        }).unwrap();
    assert_eq!(get_snapshot(&conn).unwrap(),committed);
    assert_eq!(committed["workflow"]["shipments"][0]["charges"]["FACTORY"],4000);
    let payload:String=conn.query_row("SELECT payload FROM core_workflow_shipments WHERE id='shipment:synthetic'",[],|r|r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(),committed["workflow"]["shipments"][0]);
    // Reassign to a new group using the normal shared writer; the original
    // order/recipient/source identity and old group notices stay intact.
    let moved=bando_data_access::update_v2_snapshot(&mut conn,revision+1,true,|current|{
        let mut next=current.clone();
        let mut group=next["workflow"]["groups"][0].clone();
        group["id"]=json!("group:split-test");
        group["notices"]=json!([]);
        next["workflow"]["groups"].as_array_mut().unwrap().push(group);
        next["workflow"]["shipments"][0]["groupId"]=json!("group:split-test");
        Ok(next)
    }).unwrap();
    assert_eq!(get_snapshot(&conn).unwrap(),moved);
    assert_eq!(moved["workflow"]["shipments"][0]["orderId"],"order:synthetic");
    let failed=bando_data_access::update_v2_snapshot(&mut conn,revision+2,true,|current|{
        let mut next=current.clone();
        next["workflow"]["shipments"][0]["groupId"]=json!("group:missing");
        Ok(next)
    });
    assert!(failed.is_err());
    assert_eq!(get_snapshot(&conn).unwrap(),moved);
    for table in ["core_workflow_groups","core_workflow_shipments"]{
        let tx=conn.transaction().unwrap();
        tx.execute(&format!("DELETE FROM {table} WHERE position=(SELECT MAX(position) FROM {table})"),[]).unwrap();
        assert!(get_snapshot(&tx).unwrap_err().contains("배송 상태 저장 개수"));
        tx.rollback().unwrap();
    }
    assert_eq!(get_snapshot(&conn).unwrap(),moved);
    let split_backup=bando_data_access::backup_service::manual_backup(&conn,&isolated.root).unwrap();
    bando_data_access::backup_service::restore_backup(&mut conn,&isolated.root,&split_backup).unwrap();
    let mut restored=moved;
    restored["revision"]=json!(revision+3);
    assert_eq!(get_snapshot(&conn).unwrap(),restored);
}

#[test]
fn native_detach_regroup_preserves_shipment_financial_and_audit_records(){
    let (isolated,mut conn)=IsolatedDatabase::open();
    let mut fixture=stage_three(&get_snapshot(&conn).unwrap(),"POSTPAID");
    let at="2026-11-26T08:00:00+09:00";
    let mut recipient=fixture["orders"][0]["recipients"][0].clone();
    recipient["id"]=json!("recipient:second");
    recipient["name"]=json!("두번째 수취인");
    recipient["items"][0]["id"]=json!("item:second");
    fixture["orders"][0]["recipients"].as_array_mut().unwrap().push(recipient);
    let mut ship=fixture["workflow"]["shipments"][0].clone();
    ship["id"]=json!("shipment:second");
    ship["recipientId"]=json!("recipient:second");
    ship["charges"]["FACTORY"]=json!(4000);
    fixture["workflow"]["shipments"].as_array_mut().unwrap().push(ship);
    let account=fixture["settings"]["accounts"][0].clone();
    fixture["workflow"]["groups"][0]["notices"]=json!([{
        "id":"notice:split","at":at,"actor":"검증 담당자",
        "shipmentIds":["shipment:synthetic","shipment:second"],
        "amounts":{"shipment:synthetic":36300,"shipment:second":37400},
        "total":73700,"account":account,"message":"기존 합계 안내"
    }]);
    fixture["workflow"]["payments"]=json!([{
        "id":"payment:split","dealerId":"synthetic-workflow-client",
        "account":account,"amount":10000,"paidAt":at,"recordedAt":at,
        "actor":"검증 담당자","memo":"",
        "allocations":[{"id":"allocation:split","shipmentId":"shipment:second","amount":10000}]
    }]);
    let saved=write_snapshot(&mut conn,fixture,0,true).unwrap();
    let revision=saved["revision"].as_u64().unwrap();
    let change=|state:&Value,action:&str,ack:bool,event:&str|{
        let result=rust("apply_workflow_action",json!({
            "state":state,"action":action,"shipmentId":"shipment:second",
            "groupId":if action=="detach_shipment"{"group:synthetic"}else{"group:detached"},
            "newGroupId":"group:detached","targetGroupId":"group:synthetic",
            "ack":ack,"eventId":event,"now":at,"actor":"검증 담당자"
        }))?;
        let next=result["state"].clone();
        rust("validate_workflow",json!({"state":&next,"before":state}))?;
        Ok::<Value,String>(next)
    };
    let assert_preserved=|next:&Value,group:&str,event_count:usize|{
        assert_eq!(next["orders"],saved["orders"]);
        for key in ["sources","payments","confirmations","queue","deliveries","changeRequests"]{
            assert_eq!(next["workflow"][key],saved["workflow"][key],"{key}");
        }
        let mut expected=saved["workflow"]["shipments"].clone();
        expected[1]["groupId"]=json!(group);
        assert_eq!(next["workflow"]["shipments"],expected);
        assert_eq!(next["workflow"]["groups"][0]["notices"],saved["workflow"]["groups"][0]["notices"]);
        let old_events=saved["workflow"]["events"].as_array().unwrap();
        let events=next["workflow"]["events"].as_array().unwrap();
        assert_eq!(events.len(),old_events.len()+event_count);
        assert_eq!(&events[..old_events.len()],old_events.as_slice());
    };
    let split=bando_data_access::update_v2_snapshot(&mut conn,revision,true,
        |state|change(state,"detach_shipment",false,"event:detach")).unwrap();
    assert_preserved(&split,"group:detached",1);
    assert_eq!(split["workflow"]["groups"][0]["needsNotice"],true);
    assert_eq!(split["workflow"]["groups"][1]["needsNotice"],true);
    assert_eq!(split["workflow"]["groups"][1]["accountId"],"synthetic-account");
    assert_eq!(split["workflow"]["groups"][1]["notices"],json!([]));
    // Invalid regroup must not advance the revision or alter persisted data.
    assert!(bando_data_access::update_v2_snapshot(&mut conn,revision+1,true,
        |state|change(state,"regroup_shipment",false,"event:unapproved")).is_err());
    assert_eq!(get_snapshot(&conn).unwrap(),split);
    for (field,value) in [("step",json!(2)),("transferred",json!(true))]{
        let mut invalid=split.clone();
        invalid["workflow"]["shipments"][0][field]=value;
        assert!(change(&invalid,"regroup_shipment",true,"event:invalid").is_err());
    }
    let mut different=split.clone();
    different["workflow"]["groups"][0]["sourceId"]=json!("source:other");
    assert!(change(&different,"regroup_shipment",true,"event:source").is_err());
    let mut different=split.clone();
    different["workflow"]["groups"][0]["dealerId"]=json!("dealer:other");
    assert!(change(&different,"regroup_shipment",true,"event:dealer").is_err());
    let mut different=split.clone();
    different["settings"]["accounts"].as_array_mut().unwrap().push(json!({
        "id":"other-account","bank":"가상은행","number":"999","holder":"합성 계좌"
    }));
    different["workflow"]["groups"][0]["accountMode"]=json!("MANUAL");
    different["workflow"]["groups"][0]["accountId"]=json!("other-account");
    assert!(change(&different,"regroup_shipment",true,"event:account").is_err());
    let merged=bando_data_access::update_v2_snapshot(&mut conn,revision+1,true,
        |state|change(state,"regroup_shipment",true,"event:regroup")).unwrap();
    assert_preserved(&merged,"group:synthetic",2);
    assert_eq!(merged["workflow"]["groups"][0]["needsNotice"],true);
    assert_eq!(get_snapshot(&conn).unwrap(),merged);
    drop(conn);
    let reopened=initialize_database(&isolated.root.join("state.sqlite")).unwrap();
    assert_eq!(get_snapshot(&reopened).unwrap(),merged);
}
