#[path = "../../../../src-tauri/src/business_core/output/mod.rs"]
pub mod output;
#[path = "../../../../src-tauri/src/business_core/rules/mod.rs"]
pub mod rules;
#[path = "../../../../src-tauri/src/business_core/recognition/mod.rs"]
pub mod recognition;

#[cfg(test)]
mod tests {
    use super::output;
    use serde_json::{json, Value};

    fn item(origin: &str) -> Value {
        json!({
            "id":"item-1","origin":origin,"name":"BD 709 타공 상판 가방 미포함",
            "productId":"product-1","variantId":"price-1",
            "quantity":2,"color":"블랙"
        })
    }
    fn state(origin: &str) -> Value {
        json!({
            "revision":7,
            "orders":[{"id":"order-1"}],
            "products":[{"id":"product-1","name":"BD 709 타공 상판 가방 미포함",
                "variants":[{"id":"price-1","color":"블랙"}]}],
            "exports":[],
            "workflow":{
                "shipments":[{"id":"shipment-1","orderId":"order-1","step":4,
                    "transferred":true,"legacyArchived":false}],
                "queue":[{"id":"queue-1","shipmentId":"shipment-1",
                    "origin":origin,"status":"QUEUED",
                    "snapshot":{"freightCode":"010","recipient":{
                        "name":"김민수","address":"서울 종로구 123",
                        "phone":"02-123-4567","mobile":"","note":"문 앞",
                        "items":[item(origin)]
                    },"sender":{
                        "name":"대리점","displayName":"대리점 송장상호",
                        "address":"서울 중구 88","postcode":"04500",
                        "phone":"02-456-7890","mobile":""
                    }}}
                ],
                "payments":[],"confirmations":[],"events":[]
            }
        })
    }
    #[test]
    fn factory_preserves_immutable_identifiers_and_logen_columns() {
        let preview = output::execute("prepare_factory_export",
            json!({"state":state("FACTORY"),"ids":["queue-1"]})).unwrap();
        assert_eq!(preview["revision"],7);
        let values = preview["rows"][0]["values"].as_array().unwrap();
        assert_eq!(values.len(),16);
        assert_eq!(values[0],"김민수");
        assert_eq!(values[1],"");
        assert_eq!(values[5],""); // Package quantity is filled by the factory.
        assert_eq!(values[6],""); // Actual postage is filled by the factory.
        assert_eq!(values[7],"010");
        assert_eq!(values[8],"◆ BD 709 타공 상판 가방 미포함 2개");
        assert_eq!(values[11],"대리점 송장상호");
        assert_eq!(values[12],"04500");
    }
    #[test]
    fn store_product_separator_and_color_preserved() {
        let mut state = state("STORE");
        state["workflow"]["queue"][0]["snapshot"]["recipient"]["items"]
            .as_array_mut().unwrap().push(json!({
                "origin":"STORE","name":"SS-1","quantity":1,"color":"단일"
            }));
        let preview = output::execute("prepare_store_export",
            json!({"state":state,"ids":["queue-1"]})).unwrap();
        assert_eq!(preview["rows"][0]["values"][8],
            "BD 709 타공 상판 가방 미포함 블랙 2개 / SS-1 1개");
    }
    #[test]
    fn duplicate_selection_and_wrong_freight_are_blocked() {
        let s = state("FACTORY");
        assert!(output::execute("prepare_factory_export",
            json!({"state":s,"ids":["queue-1","queue-1"]})).unwrap_err().contains("중복"));
        let mut s = state("FACTORY");
        s["workflow"]["queue"][0]["snapshot"]["freightCode"] = json!("002");
        assert!(output::execute("prepare_factory_export",
            json!({"state":s,"ids":["queue-1"]})).unwrap_err().contains("운임구분"));
    }
    #[test]
    fn frozen_option_conflict_fails_instead_of_silently_correcting() {
        let mut s = state("FACTORY");
        s["workflow"]["queue"][0]["snapshot"]["recipient"]["items"][0]["name"] =
            json!("BD 709 비타공 상판 가방 포함");
        let error = output::execute("prepare_factory_export",
            json!({"state":s,"ids":["queue-1"]})).unwrap_err();
        assert!(error.contains("가방") || error.contains("상판"));
    }
    #[test]
    fn historical_product_mapping_requires_both_original_ids() {
        let mut s = state("FACTORY");
        s["products"][0]["id"]=json!("product-new");
        s["products"][0]["sourceProductId"]=json!("product-1");
        s["productMappings"] = json!({"product-1|price-1":"product-new"});
        assert!(output::execute("prepare_factory_export",
            json!({"state":s,"ids":["queue-1"]})).is_ok());
        let mut s = state("FACTORY");
        s["products"][0]["id"]=json!("product-new");
        s["products"][0]["sourceProductId"]=json!("product-1");
        s["productMappings"]=json!({"product-1|different":"product-new"});
        assert!(output::execute("prepare_factory_export",
            json!({"state":s,"ids":["queue-1"]})).is_err());
    }
    #[test]
    fn templates_keep_custom_values_and_forbid_removed_required_variables() {
        let result = output::execute("render_messages",json!({
            "mode":"template","templateId":"modelConfirm",
            "settings":{"automaticMessageTemplates":{"modelConfirm":"{상품명}으로 보내드릴까요?"}},
            "values":{"상품명":"BD 709"}
        })).unwrap();
        assert_eq!(result["text"],"BD 709으로 보내드릴까요?");
        let bad = output::execute("render_messages",json!({
            "mode":"validate_templates",
            "settings":{"automaticMessageTemplates":{"modelConfirm":"확인 부탁드립니다."}}
        }));
        assert!(bad.is_err());
    }
    #[test]
    fn payment_places_optional_guidance_after_amount_and_before_official_link() {
        let result = output::execute("render_messages",json!({
            "mode":"payment","settings":{},
            "clientSelected":true,"total":113300,"includeLinks":true,
            "account":{"bank":"국민은행","number":"000-123","holder":"반도"},
            "recipients":[{
                "name":"김민수","freightCode":"010","shipping":3000,
                "applyVat":true,"vat":10300,
                "items":[{
                    "name":"709","color":"Black","amount":100000,"quantity":2,
                    "productId":"p-1","variantId":"v-1",
                    "appliedRepresentativeModel":{"productId":"p-1","productName":"BD 709"},
                    "provisionalNotice":"상판 타입 확인 부탁드립니다.",
                    "url":"https://www.bandostand.com/product/709"
                }]
            }]
        })).unwrap();
        let m = result["payment"].as_str().unwrap();
        assert!(m.contains("BD 709 (블랙) * 2개 : 100,000원"));
        let amount = m.find("합계 : 113,300원").unwrap();
        let notice = m.find("상판 타입 확인 부탁드립니다.").unwrap();
        let link = m.find("https://www.bandostand.com/product/709").unwrap();
        assert!(amount < notice && notice < link);
    }
    #[test]
    fn settlement_does_not_merge_billed_amounts_with_cash_receipts() {
        let mut s = state("STORE");
        s["workflow"]["payments"]=json!([{
            "id":"payment-1","paidAt":"2026-11-13T00:00:00+09:00",
            "dealerId":"client-1","account":{"bank":"국민","holder":"반도","number":"123"},
            "amount":40000,"memo":"","allocations":[{"shipmentId":"shipment-1","amount":40000}]
        }]);
        s["clients"]=json!([{"id":"client-1","name":"대리점"}]);
        let history=json!([{
            "order":{"id":"order-1","createdAt":"2026-11-13T12:00:00+09:00",
                "confirmedAt":"2026-11-13T12:30:00+09:00","orderType":"DEALER",
                "paid":false,"recipients":[{"id":"recipient-1","items":[{"origin":"STORE"}]}]},
            "client":{"name":"대리점","payment":"PREPAID"},
            "billing":{"subtotal":100000,"shipping":3000,"vat":0,"total":103000},
            "shipmentIds":["shipment-1"],"status":"엑셀 추출 대기"
        }]);
        let r=output::execute("prepare_settlement",
            json!({"state":s,"date":"2026-11-13","historyRows":history})).unwrap();
        assert_eq!(r["billedKnown"],103000);
        assert_eq!(r["paidCash"],40000);
        assert_eq!(r["outstandingKnown"],63000);
        assert_eq!(r["paymentRows"][0]["dealer"],"대리점");
    }

    // Explicit expectations of the live native renderers. No TS oracle.
    fn operational_message_fixture()->Value{
        json!({
            "groupId":"g","includeLinks":false,"includeCopyStatus":true,
            "state":{
                "settings":{"accounts":[{"id":"a","bank":"가상은행","number":"123","holder":"테스트"}],
                    "defaultAccountId":"a","senderAddress":"서울특별시 중구 세종대로 1","senderPhone":"02-1234-5678"},
                "clients":[{"id":"c","name":"거래처","address":"서울특별시 중구 세종대로 1",
                    "phone":"02-1234-5678","tier":"wholesale","payment":"POSTPAID"}],
                "products":[{"id":"p","name":"709","variants":[{"id":"v","wholesale":10000}]}],
                "orders":[{"id":"o","clientId":"c","orderType":"AGENCY","applyVat":true,
                    "freightCode":"010","recipients":[{"id":"r","name":"테스트",
                        "address":"서울특별시 중구 세종대로 1","phone":"010-1234-5678",
                        "items":[{"id":"i","productId":"p","variantId":"v","name":"709",
                            "quantity":1,"quantityConfirmed":true,"unitPrice":10000,"origin":"FACTORY"}]}]}],
                "workflow":{"groups":[{"id":"g","dealerId":"c","needsNotice":false}],
                    "shipments":[{"id":"s","orderId":"o","recipientId":"r","groupId":"g",
                        "step":2,"charges":{"FACTORY":3000},"transferred":false}],
                    "queue":[],"events":[]}
            }
        })
    }

    fn render_payment(payload:&Value)->String {
        output::execute("render_messages",payload.clone()).unwrap()["payment"]
            .as_str().expect("payment body").to_owned()
    }
    fn payment_fixture()->Value{
        json!({"mode":"payment","settings":{},"clientSelected":true,
            "total":113300,"includeLinks":true,
            "account":{"bank":"가상은행","number":"123-456","holder":"테스트"},
            "recipients":[{"name":"김가상","freightCode":"010","shipping":3000,
                "applyVat":true,"vat":10300,"items":[{"name":"BD 709","color":"Black",
                    "quantity":2,"amount":100000,"productId":"p","variantId":"v",
                    "url":"https://www.bandostand.com/product/709"}]}]})
    }
    #[test]
    fn operational_payment_preserves_calculated_amount_and_vat_mode(){
        let mut p=payment_fixture();
        let text=render_payment(&p);
        assert!(text.contains("100,000원"));
        assert!(text.contains("운임 : 3,000원"));
        assert!(text.contains("부가세 : 10,300원"));
        assert!(text.contains("합계 : 113,300원 입니다. (부가세 포함)"));
        p["recipients"][0]["applyVat"]=json!(false);
        p["total"]=json!(103000);
        let text=render_payment(&p);
        assert!(text.contains("합계 : 103,000원 입니다. (부가세 별도)"));
        assert!(!text.contains("부가세 :"));
        p["recipients"][0]["freightCode"]=json!("020");
        p["recipients"][0]["shipping"]=json!(0);
        p["total"]=json!(100000);
        let text=render_payment(&p);
        assert!(text.contains("운임 : 착불"));
        assert!(text.contains("합계 : 100,000원"));
    }
    #[test]
    fn operational_payment_mixed_vat_and_unresolved_values_are_not_fabricated(){
        let mut p=payment_fixture();
        let mut second=p["recipients"][0].clone();
        second["name"]=json!("박가상");
        second["applyVat"]=json!(false);
        p["recipients"].as_array_mut().unwrap().push(second);
        p["total"]=json!(216300);
        let result=output::execute("render_messages",p.clone()).unwrap();
        assert_eq!(result["mixedVat"],true);
        assert!(result["payment"].as_str().unwrap().contains("합계 : 216,300원"));
        p["total"]=Value::Null;
        p["account"]=Value::Null;
        p["recipients"][0]["items"][0]["amount"]=Value::Null;
        let text=render_payment(&p);
        assert!(text.contains("판매가 확인 필요"));
        assert!(text.contains("금액 확인 필요"));
        assert!(text.contains("[입금 계좌 등록 필요]"));
        assert!(!text.contains("합계 : 0원"));
    }
    #[test]
    fn operational_payment_official_links_and_color_visibility(){
        let mut p=payment_fixture();
        let mut duplicate=p["recipients"][0]["items"][0].clone();
        duplicate["colorNotRequired"]=json!(true);
        let mut unsafe_link=duplicate.clone();
        unsafe_link["url"]=json!("https://other.example/product/709");
        p["recipients"][0]["items"].as_array_mut().unwrap().extend([duplicate,unsafe_link]);
        let text=render_payment(&p);
        assert_eq!(text.matches("https://www.bandostand.com/product/709").count(),1);
        assert!(!text.contains("other.example"));
        assert_eq!(text.matches("(블랙)").count(),1);
        p["includeLinks"]=json!(false);
        assert!(!render_payment(&p).contains("https://"));
    }
    #[test]
    fn operational_request_scopes_blockers_and_custom_template(){
        let mut p=json!({"settings":{"automaticMessageTemplates":{
                "contactMissing":"{수취인}님 연락처 확인 요청",
                "requestClosing":"회신 부탁드립니다."}},
            "order":{"recipients":[{"id":"r1","name":"김가상","items":[]}]},
            "issues":[{"recipientId":"r1","kind":"CONTACT","severity":"ACTION","blockingStage":1},
                {"recipientId":"other","kind":"ADDRESS","severity":"ACTION","blockingStage":1},
                {"recipientId":"r1","kind":"NAME","severity":"ACTION","blockingStage":2}],
            "step":2});
        let result=output::execute("render_automatic_request",p.clone()).unwrap();
        assert_eq!(result["phase"],"request");
        assert_eq!(result["requiresCustomerResponse"],true);
        assert_eq!(result["requestText"],"김가상님 연락처 확인 요청\n\n회신 부탁드립니다.");
        p["issues"]=json!([]);
        let result=output::execute("render_automatic_request",p.clone()).unwrap();
        assert_eq!(result["phase"],"payment");
        assert_eq!(result["requiresCustomerResponse"],false);
        p["step"]=json!(1);
        assert_eq!(output::execute("render_automatic_request",p).unwrap()["phase"],"request");
    }
    #[test]
    fn operational_request_model_links_only_from_official_catalog(){
        let mut p=json!({"settings":{},"step":1,
            "order":{"recipients":[{"id":"r","name":"김가상","items":[
                {"id":"i","name":"BD 709","modelProposal":{"required":true},
                    "url":"https://www.bandostand.com/product/709"}]}]},
            "issues":[{"recipientId":"r","itemId":"i","kind":"MODEL_CONFIRM",
                "severity":"ACTION","blockingStage":1}]});
        let r=output::execute("render_automatic_request",p.clone()).unwrap();
        assert!(r["requestText"].as_str().unwrap().contains("https://www.bandostand.com/product/709"));
        p["order"]["recipients"][0]["items"][0]["url"]=json!("https://other.example/product/709");
        let r=output::execute("render_automatic_request",p).unwrap();
        assert!(!r["requestText"].as_str().unwrap().contains("other.example"));
    }
    #[test]
    fn operational_payment_account_precedence_and_missing_manual_account(){
        let mut p=operational_message_fixture();
        p["state"]["settings"]["accounts"].as_array_mut().unwrap().extend([
            json!({"id":"preferred","bank":"가상우선은행","number":"111","holder":"우선계좌"}),
            json!({"id":"manual","bank":"가상수동은행","number":"222","holder":"수동계좌"})
        ]);
        p["state"]["clients"][0]["preferredAccountId"]=json!("preferred");
        let notice=|payload:&Value| output::execute("render_payment_notice",payload.clone())
            .unwrap()["payment"].as_str().unwrap().to_owned();
        assert!(notice(&p).contains("가상우선은행 111 우선계좌"));
        p["state"]["workflow"]["groups"][0]["accountMode"]=json!("MANUAL");
        p["state"]["workflow"]["groups"][0]["accountId"]=json!("manual");
        let text=notice(&p);
        assert!(text.contains("가상수동은행 222 수동계좌"));
        assert!(!text.contains("우선계좌"));
        p["state"]["workflow"]["groups"][0]["accountId"]=json!("missing");
        assert!(notice(&p).contains("[입금 계좌 등록 필요]"));
    }
    #[test]
    fn operational_copy_evidence_tracks_exact_account_and_charge(){
        let mut p=operational_message_fixture();
        let first=output::execute("render_automatic_message",p.clone()).unwrap();
        assert_eq!(first["copied"],false);
        p["state"]["workflow"]["events"]=json!([{"groupId":"g","action":"메시지 복사",
            "after":{"type":"PAYMENT","signature":first["signature"]}}]);
        assert_eq!(output::execute("render_automatic_message",p.clone()).unwrap()["copied"],true);
        let mut changed=p.clone();
        changed["state"]["settings"]["accounts"][0]["number"]=json!("456");
        let next=output::execute("render_automatic_message",changed).unwrap();
        assert_ne!(next["signature"],first["signature"]);
        assert_eq!(next["copied"],false);
        p["state"]["workflow"]["shipments"][0]["charges"]["FACTORY"]=json!(4000);
        let next=output::execute("render_automatic_message",p).unwrap();
        assert_ne!(next["signature"],first["signature"]);
        assert_eq!(next["copied"],false);
    }
    #[test]
    fn operational_copy_greeting_is_not_duplicated_or_persisted_as_preview(){
        let body="합계 : 14,300원 입니다.";
        let r=output::execute("decorate_automatic_copy",json!({"text":body,"includeGreeting":true})).unwrap();
        assert_eq!(r["text"],format!("안녕하세요 반도스탠드입니다.\n\n{body}"));
        let twice=output::execute("decorate_automatic_copy",json!({"text":r["text"],"includeGreeting":true})).unwrap();
        assert_eq!(r,twice);
        assert_eq!(output::execute("decorate_automatic_copy",json!({"text":body,"includeGreeting":false})).unwrap()["text"],body);
        let p=operational_message_fixture();
        let notice=output::execute("render_payment_notice",p).unwrap();
        assert!(notice["payment"].as_str().unwrap().starts_with("안녕하세요."));
    }
    #[test]
    fn operational_correction_scopes_records_and_preserves_latest_status(){
        let before=json!({"name":"김가상","address":"서울","phone":"02-1234-5678",
            "mobile":"","note":"","freightCode":"010","items":[]});
        let mut after=before.clone();
        after["address"]=json!("경기"); after["freightCode"]=json!("020");
        after["shippingCharges"]=json!({"FACTORY":4000});
        let p=json!({"shipmentId":"s","state":{"workflow":{
            "shipments":[{"id":"s","charges":{"FACTORY":3000}}],
            "events":[{"action":"정정 요청 상태","before":"r","after":"REGISTERED"},
                {"action":"정정 요청 상태","before":"other","after":"CONFIRMED"},
                {"action":"정정 요청 상태","before":"r","after":"PENDING_FACTORY"}],
            "changeRequests":[{"id":"r","shipmentId":"s","beforeRecipient":before,"afterRecipient":after},
                {"id":"legacy","shipmentId":"s","afterRecipient":null},
                {"id":"other","shipmentId":"other","afterRecipient":null}]}}});
        let r=output::execute("render_correction_summaries",p).unwrap();
        let records=r["records"].as_array().unwrap();
        assert_eq!(records.len(),2);
        assert_eq!(records[0]["status"],"PENDING_FACTORY");
        assert_eq!(records[0]["text"],"기존 출고 건 정정 요청입니다.\n배송 ID: s\n새 출고 주문이 아닙니다.\n주소: 서울 → 경기\n공장 운임: 3000 → 4000원\n운임 방식: 010 → 020");
        assert_eq!(records[1]["id"],"legacy");
        assert_eq!(records[1]["text"],Value::Null);
    }
}
