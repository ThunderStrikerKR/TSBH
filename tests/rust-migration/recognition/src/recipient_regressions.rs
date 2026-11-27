//! Native recognition regressions retained from the historical TS comparison suite.
use crate::recognition;
use serde_json::{json,Value};

fn seed() -> Value {
    serde_json::from_str(include_str!(
        "../../../../src-tauri/resources/state-seed.json"
    )).expect("committed products")
}
fn corrected_state() -> (Value,Vec<Value>) {
    let mut state=seed();
    let m:Value=serde_json::from_str(include_str!(
        "../catalog-color-corrections-15.json"
    )).expect("reviewed color corrections");
    let corrections=m["corrections"].as_array().unwrap().clone();
    assert_eq!(corrections.len(),15);
    for c in &corrections {
        let id=c["productId"].as_str().unwrap();
        let p=state["products"].as_array_mut().unwrap().iter_mut()
            .find(|p|p["id"]==id).expect("correct SKU");
        assert_eq!(p["active"],true);
        assert_eq!(p["name"],c["expectedName"]);
        assert_eq!(p["defaultColor"],c["expectedDefaultColor"]);
        assert_eq!(p["variants"].as_array().unwrap().len(),1);
        assert_eq!(p["variants"][0]["id"],c["variantId"]);
        assert_eq!(p["variants"][0]["color"],c["expectedVariantColor"]);
        assert_eq!(p["variants"][0]["url"],c["registeredSourceUrl"]);
        p["defaultColor"]=c["color"].clone();
        p["variants"][0]["color"]=c["color"].clone();
    }
    (state,corrections)
}
fn original_samples()->Vec<Value>{
    serde_json::from_str(include_str!("../operational-order-samples.json"))
        .expect("the same 20 committed order inputs")
}

fn recipient_client() -> Value {
    json!({"id":"ci-dealer","name":"가상거래처","tier":"wholesale",
        "overrides":{},"active":true,"payment":"PREPAID",
        "contact":"","identifiers":"","shippingFeeExcluded":false})
}
fn recipient_cases() -> Vec<Value> {
    let fixture=original_samples();
    assert_eq!(fixture.len(),20);
    let mut cases:Vec<Value>=fixture.iter().map(|x| json!({
        "id":format!("original-{:02}",x["id"].as_u64().unwrap()),
        "raw":x["raw"]
    })).collect();
    let (state,corrections)=corrected_state();
    assert_eq!(state["products"].as_array().unwrap().len(),360);
    for (i,c) in corrections.iter().enumerate() {
        let name=c["expectedName"].as_str().expect("product name");
        cases.push(json!({"id":format!("color-{:02}",i+1),
            "raw":format!("{name} 2개\n김가상\n서울특별시 용산구 한강대로 100")}));
    }
    // Nine independent, business-oriented real order documents. Use
    // original TS and Rust, never a simulated recipient parser.
    for (id,raw) in [
        ("recipient-two","BD Floor-BTS / 블랙 / 높이 27cm 2개\n김가상\n서울특별시 용산구 한강대로 100\n010-1234-5678\nBD Desk-BTS / 블랙 / 높이 27cm 1개\n박가상\n경기도 의정부시 시민로 20\n010-2345-6789"),
        ("recipient-name-contact","수취인: 김가상\n연락처: 010-1234-5678\n배송지: 서울특별시 마포구 월드컵로 20\n709 민판 2개"),
        ("recipient-note","김가상\n서울특별시 서대문구 연희로 100\n010-1234-5678\n배송 요청사항: 현관 앞 부탁드립니다\nBD 709 1개"),
        ("recipient-inline","BD 709 민판 한개 이가상 010-1234-5678\n서울특별시 마포구 월드컵로 20"),
        ("recipient-address-detail","박가상\n010-2345-6789\n서울특별시 성북구 길음로 11\n  101동 1102호\n709 1개"),
        ("recipient-shipping","김가상\n경기도 의정부시 시민로 100\n010-1234-5678\nBD 950 3개\n메시지: 부재 시 문 앞"),
        ("recipient-unnamed","서울특별시 용산구 한강대로 100\nBD 709 2개"),
        ("recipient-duplicate-name","김가상\n서울특별시 용산구 한강대로 100\nBD 709 1개\n김가상\n경기도 의정부시 시민로 20\nBD 950 1개"),
        ("recipient-common","BD 709 2개 공통\n김가상\n서울특별시 용산구 한강대로 100\n박가상\n경기도 의정부시 시민로 20"),
    ] {
        cases.push(json!({"id":id,"raw":raw}));
    }
    assert_eq!(cases.len(),44);
    cases
}

// The prior JS comparison also independently validated Rust evidence links.
#[test]
fn native_recipient_and_item_source_links_remain_valid(){
    let (mut state,_)=corrected_state();
    let client=recipient_client();
    state["clients"]=json!([client.clone()]);
    for case in recipient_cases(){
        let raw=case["raw"].as_str().expect("case raw");
        let rust=recognition::execute("analyze_order",json!({
            "raw":raw,"state":state,"client":client
        })).expect("real Rust recipient parser");
        for recipient in rust["recipients"].as_array().expect("recipients"){
            let rid=&recipient["id"];
            for source in recipient["sources"].as_array().into_iter().flatten(){
                assert_eq!(&source["targetId"],rid,"case {}",case["id"]);
                if let Some(owner)=source.get("recipientId"){
                    assert_eq!(owner,rid,"case {}",case["id"]);
                }
            }
            for item in recipient["items"].as_array().into_iter().flatten(){
                for source in item["sources"].as_array().into_iter().flatten(){
                    assert_eq!(source["targetId"],item["id"],"case {}",case["id"]);
                    if let Some(owner)=source.get("recipientId"){
                        assert_eq!(owner,rid,"case {}",case["id"]);
                    }
                }
            }
        }
    }
}

#[test]
fn protect_true_recipients_and_address_details_against_legacy_ts_false_positives() {
    let (state,_)=corrected_state();
    let client=recipient_client();
    let cases=recipient_cases();
    for id in ["original-19","original-20","recipient-name-contact",
        "recipient-inline","recipient-address-detail"] {
        let case=cases.iter().find(|c|c["id"]==id).expect("fixture");
        let raw=case["raw"].as_str().expect("literal original order");
        let rust=recognition::execute("analyze_order",json!({
            "raw":raw,"state":state,"client":client
        })).expect("production Rust");
        let recipients=rust["recipients"].as_array().expect("deliveries");
        assert_eq!(recipients.len(),1,
            "{id}: no phantom deliveries from order dates or model options");
        let actual=&recipients[0];
        match id {
            "original-19"=>{
                assert_eq!(actual["name"],"여가상");
                assert_ne!(actual["name"],"연주용의자");
                assert_eq!(actual["items"][0]["quantity"],1);
            }
            "original-20"=>{
                assert_eq!(actual["name"],"이가상 선생님");
                assert_eq!(actual["phone"],"053-123-4567");
                assert_eq!(actual["mobile"],"010-1234-5678");
                assert!(actual["address"].as_str().unwrap_or("").contains("경북 경산시"));
                assert_eq!(actual["items"][0]["quantity"],1);
            }
            "recipient-name-contact"=>{
                assert_eq!(actual["name"],"김가상");
                assert_ne!(actual["name"],"민판");
                assert_eq!(actual["mobile"],"010-1234-5678");
            }
            "recipient-inline"=>{
                assert_eq!(actual["name"],"이가상",
                    "explicit recipient after product quantity must not disappear");
                assert_eq!(actual["mobile"],"010-1234-5678");
            }
            "recipient-address-detail"=>{
                assert_eq!(actual["name"],"박가상");
                assert!(actual["address"].as_str().unwrap_or("")
                    .contains("101동 1102호"),
                    "indented apartment number belongs in shipping address");
                assert!(!rust["candidates"].as_array().unwrap().iter()
                    .any(|c|c["kind"]=="product" && c["text"]=="101"),
                    "apartment building must not become an unknown product SKU");
            }
            _=>unreachable!()
        }
        assert!(recipients.iter().all(|r|
            r["id"].as_str().map_or(false,|s|!s.is_empty())));
    }
}

#[test]
fn preserve_secondary_contact_and_show_unassigned_quantity_evidence() {
    let (state,_)=corrected_state();
    let client=recipient_client();
    let cases=recipient_cases();
    for id in ["original-01","original-03","recipient-common"] {
        let raw=cases.iter().find(|c|c["id"]==id).unwrap()["raw"]
            .as_str().unwrap().to_owned();
        let response=recognition::execute("analyze_order",json!({
            "raw":raw,"state":state,"client":client
        })).expect("real Rust");
        let row=&response["recipients"][0];
        match id {
            "original-01"=>{
                let sources=row["sources"].as_array().unwrap();
                assert!(sources.iter().any(|s|
                    s["field"]=="mobile" && s["text"]=="010-1234-5678"));
                assert!(sources.iter().any(|s|
                    s["field"]=="phone" && s["text"]=="010-1111-2222"));
                assert_eq!(row["mobile"],"010-1234-5678");
                assert_eq!(row["phone"],"010-1111-2222");
            }
            "original-03"|"recipient-common"=>{
                let evidence=row["unassignedEvidence"].as_array()
                    .expect("quantity still visible without a recipient SKU assignment");
                assert!(evidence.iter().any(|e|e["kind"]=="quantity"),
                    "{id}: unassigned quantity evidence must not silently disappear");
            }
            _=>unreachable!()
        }
    }
}

// Independent expectations transcribed from the existing anonymized 18
// text orders and the pinned catalog, NOT generated from parser output.
// Address anchors are partial truth; unspecified or unavailable SKU
// identities are not counted as correct automatically. Never emit PII.
#[test]
fn operational_field_truth_report(){
    struct Row<'a>{
        name:&'a str, address:&'a str, contacts:&'a [&'a str],
        items:&'a [(Option<&'a str>,Option<i64>)],
    }
    let truth:Vec<Vec<Row<'_>>>=vec![
        vec![Row{name:"가상고등학교",address:"동창로 207",contacts:&["010-1234-5678","010-1111-2222"],items:&[(Some("sku:bando-site-50:bando-50-P00000BX000C"),Some(30)),(Some("bando-site-63"),Some(1)),(Some("bando-site-83"),Some(2))]}],
        vec![Row{name:"김가상 예수아님",address:"한강대로104라길 13",contacts:&["0504-1234-5678"],items:&[(Some("bando-site-290"),Some(2))]}],
        vec![Row{name:"박가상",address:"구월말로 51",contacts:&["010-1234-5678"],items:&[(Some("bando-site-107"),Some(2))]}],
        vec![Row{name:"가상월드",address:"소래로 41번길 66-73",contacts:&["031-123-4567"],items:&[(Some("bando-site-87"),Some(1))]}],
        vec![Row{name:"정가상 목사",address:"석촌호수로 162",contacts:&["010-1234-5678"],items:&[(None,Some(1))]}],
        vec![Row{name:"남가상",address:"함양여중길 18-9",contacts:&["010-1234-5678"],items:&[(None,None)]}],
        vec![Row{name:"청소년활동팀(1층)(김가상)",address:"문화로 120",contacts:&["010-1234-5678"],items:&[(Some("bando-site-130"),Some(1))]}],
        vec![Row{name:"임가상",address:"구율2길 19",contacts:&["010-1234-5678"],items:&[(Some("bando-site-290"),Some(6))]}],
        vec![Row{name:"이가상",address:"안청남로 40",contacts:&["010-1234-5678"],items:&[(Some("bando-site-176"),Some(2))]}],
        vec![Row{name:"심가상",address:"410동 804호",contacts:&["0502-1234-5678"],items:&[(Some("sku:bando-site-48:bando-48-P00000BV000B"),Some(1))]}],
        vec![Row{name:"상가상",address:"도봉로133길 50",contacts:&["010-1234-5678"],items:&[(Some("bando-site-202"),Some(1))]}],
        vec![Row{name:"배가상",address:"석정로461번길 7",contacts:&["010-1234-5678"],items:&[(Some("sku:bando-site-48:bando-48-P00000BV000B"),Some(3))]}],
        vec![Row{name:"장가상",address:"월곡동 538-1",contacts:&["0502-1234-5678"],items:&[(None,Some(2))]},
             Row{name:"정가상",address:"하늘달빛로 113",contacts:&["0502-2345-6789"],items:&[(Some("sku:bando-site-23:bando-23-P000000W000F"),Some(1))]}],
        vec![Row{name:"김가상",address:"에듀시티로 23",contacts:&["010-1234-5678"],items:&[(Some("bando-site-150"),Some(1))]}],
        vec![Row{name:"김가상",address:"안양천서로 177",contacts:&["010-1234-5678"],items:&[(Some("bando-site-70"),Some(3))]}],
        vec![Row{name:"배가상",address:"백운길 199",contacts:&["0502-1234-5678"],items:&[(Some("bando-site-63"),Some(1))]}],
        vec![Row{name:"김가상",address:"가곡길 39",contacts:&["010-1234-5678"],items:&[(Some("sku:bando-site-48:bando-48-P00000BV000B"),Some(4))]},
             Row{name:"박가상",address:"팔마1길 25",contacts:&["010-2345-6789"],items:&[(Some("sku:bando-site-237:bando-237-P00000JC000J"),Some(1))]}],
        vec![Row{name:"이가상",address:"동탄역로 196",contacts:&["010-1234-5678"],items:&[(Some("sku:bando-site-48:bando-48-P00000BV000C"),Some(1))]}],
    ];
    let fixtures=original_samples();
    let state=seed();
    let client=recipient_client();
    let mut results=Vec::new();
    assert_eq!(truth.len(),18);
    for (i,expected) in truth.iter().enumerate(){
        let case=&fixtures[i];
        let raw=case["raw"].as_str().unwrap();
        assert_eq!(case["id"],json!(i+1));
        assert_eq!(expected.len(),case["expectedRecipients"].as_u64().unwrap() as usize);
        // Check the independent annotations actually refer to this input.
        for e in expected{
            assert!(raw.contains(e.name)&&raw.contains(e.address),"fixture {} label source",i+1);
            for contact in e.contacts{assert!(raw.contains(contact),"fixture {} contact source",i+1);}
            for (id,_) in e.items{
                if let Some(id)=id{assert!(state["products"].as_array().unwrap().iter().any(|p|p["id"]==*id),"fixture {} catalog identity",i+1);}
            }
        }
        let parsed=recognition::execute("analyze_order",json!({"raw":raw,"state":state,"client":client}))
            .unwrap_or_else(|_|panic!("field evaluation failed at fixture {}",i+1));
        let rows=parsed["recipients"].as_array().unwrap();
        let units=raw.encode_utf16().collect::<Vec<_>>();
        let span_ok=|s:&Value|{
            let Some(a)=s["start"].as_u64().map(|v|v as usize)else{return false;};
            let Some(b)=s["end"].as_u64().map(|v|v as usize)else{return false;};
            a<b&&units.get(a..b).and_then(|x|String::from_utf16(x).ok())
                .is_some_and(|x|s["text"].as_str()==Some(x.as_str()))
        };
        let mut checks=Vec::new();
        for (j,e) in expected.iter().enumerate(){
            // Recipient order is part of the expectation, not a sorted bag.
            let row=rows.get(j).unwrap_or(&Value::Null);
            let mut contacts=[row["phone"].as_str().unwrap_or(""),row["mobile"].as_str().unwrap_or("")]
                .into_iter().filter(|v|!v.is_empty()).collect::<Vec<_>>();
            contacts.sort_unstable();
            let mut want=e.contacts.to_vec();want.sort_unstable();
            let items=row["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            let item_checks=e.items.iter().enumerate().map(|(k,(pid,qty))|{
                let item=items.get(k).unwrap_or(&Value::Null);
                json!({"sku":pid.map(|id|!item.is_null()&&item["productId"]==id),
                    "quantity":!item.is_null()&&item["quantity"]==json!(qty),
                    "quantityConfirmed":!item.is_null()&&item["quantityConfirmed"]==qty.is_some()})
            }).collect::<Vec<_>>();
            checks.push(json!({"name":row["name"]==e.name,
                "addressAnchor":row["address"].as_str().is_some_and(|a|a.contains(e.address)),
                "contacts":contacts==want,"itemCount":items.len()==e.items.len(),
                "items":item_checks}));
        }
        let links=rows.iter().all(|r|{
            r["sources"].as_array().into_iter().flatten().all(|s|
                span_ok(s)&&s["recipientId"]==r["id"]&&s["targetId"]==r["id"])&&
            r["items"].as_array().into_iter().flatten().all(|it|
                it["sources"].as_array().into_iter().flatten().all(|s|
                    span_ok(s)&&s["recipientId"]==r["id"]&&s["targetId"]==it["id"]))
        })&&parsed["candidates"].as_array().into_iter().flatten().all(span_ok);
        results.push(json!({"fixtureId":i+1,"recipientCount":rows.len()==expected.len(),
            "sourceLinks":links,"recipients":checks}));
    }
    println!("RECOGNITION_FIELD_TRUTH {}",json!({"schemaVersion":1,"fixtureCount":18,
        "fullAddressTruth":false,"patternTruth":false,"fixtures":results}));
}
