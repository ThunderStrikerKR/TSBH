//! Binary workbook tests, no installed app, actual business DB or legacy Node.
#[path="../src/native_workbooks.rs"]
mod native_workbooks;
#[path="../../src-tauri/src/business_core/catalog_clients/client_preview.rs"]
mod client_preview;
#[path="../../src-tauri/src/business_core/rules/factory_reply.rs"]
mod factory_reply;
use serde_json::{json,Value};
use rust_xlsxwriter::Workbook;
use native_workbooks::*;

fn client_state()->Value{
    json!({"revision":7,"clients":[{"id":"001","name":"합성 거래처","address":"서울",
        "postcode":"01234","phone":"02-1234-5678","mobile":"010-0000-0000",
        "tier":"special","payment":"PREPAID","active":true,"shippingFeeExcluded":false,
        "overrides":{"v1":1234}}],"settings":{"accounts":[]},"orders":[]})
}
#[test]
fn native_client_binary_roundtrip_preserves_identity_and_special_price(){
    let state=client_state();
    let bytes=export_clients(&state).unwrap();
    let mut payload=read_clients(&bytes).unwrap();
    assert_eq!(payload["sourceRevision"],7);
    assert_eq!(payload["input"][0]["cells"][0],"001");
    assert_eq!(payload["input"][0]["cells"][3],"01234");
    assert_eq!(payload["input"][0]["cells"][5],"010-0000-0000");
    payload["state"]=state.clone();
    payload["newIds"]=json!(["unused"]);
    let p=client_preview::preview(&payload).unwrap();
    assert_eq!(p["rows"][0]["status"],"UNCHANGED");
    assert_eq!(p["rows"][0]["client"]["overrides"],state["clients"][0]["overrides"]);
    payload["state"]["revision"]=json!(8);
    assert!(client_preview::preview(&payload).is_err());
}
fn factory_bytes(numeric:bool,duplicate:bool)->Vec<u8>{
    let mut book=Workbook::new();
    let sheet=book.add_worksheet();
    sheet.write_string(0,0,"수하인명").unwrap();
    sheet.write_string(0,16,"운송장번호").unwrap();
    for r in 1..=if duplicate{2}else{1}{
        for (c,v) in [(0,"김수취"),(2,"서울 종로구 10"),(3,"010-1234-5678"),(9,"참고"),(17,"참고")]{
            sheet.write_string(r,c,v).unwrap();
        }
        sheet.write_number(r,6,4000.).unwrap();
        if numeric{sheet.write_number(r,16,1234567890.).unwrap();}
        else{sheet.write_string(r,16,"001234567890").unwrap();}
    }
    book.save_to_buffer().unwrap()
}
fn factory_state()->Value{
    let recipient=json!({"id":"r1","name":"김수취","address":"서울 종로구 10",
        "phone":"010-1234-5678","mobile":"","actualFreight":null,
        "items":[{"id":"item","origin":"FACTORY"}]});
    json!({"revision":3,"factoryReplies":[],"settings":{"operatorName":"작업자"},
        "orders":[{"id":"o1","recipients":[recipient.clone()]}],
        "workflow":{"shipments":[{"id":"s1","orderId":"o1","recipientId":"r1",
            "tracking":{},"actualFreight":{}}],
            "queue":[{"shipmentId":"s1","origin":"FACTORY","status":"EXPORTED",
                "snapshot":{"recipient":recipient}}],"events":[]}})
}
fn analyze(bytes:&[u8])->Value{
    let mut p=read_factory_reply("합성.xlsx",bytes).unwrap();
    p["state"]=factory_state();p["businessDate"]=json!("2026-11-13");
    p["at"]=json!("2026-11-13T10:00:00Z");p["actor"]=json!("합성 작업자");
    factory_reply::execute("analyze_factory_reply",&p).unwrap()
}
#[test]
fn factory_binary_q_g_columns_hash_and_native_application(){
    let bytes=factory_bytes(false,false);
    let source=read_factory_reply("합성.xlsx",&bytes).unwrap();
    assert_eq!(source["sourceRows"][0]["tracking"],"001234567890");
    assert_eq!(source["sourceRows"][0]["rawFreight"],"4000");
    assert_eq!(source["id"],read_factory_reply("다른이름.xlsx",&bytes).unwrap()["id"]);
    let p=analyze(&bytes);
    assert_eq!(p["applied"],1);
    let applied=factory_reply::execute("apply_factory_reply",&json!({"state":factory_state(),
        "preview":p,"freightAt":"2026-11-13T10:00:00Z",
        "freightEventIds":["freight1"],"replyEventIds":["reply1"]})).unwrap();
    assert_eq!(applied["next"]["workflow"]["shipments"][0]["tracking"]["FACTORY"],"001234567890");
    assert_eq!(applied["next"]["workflow"]["shipments"][0]["actualFreight"]["FACTORY"],4000);
}
#[test]
fn factory_binary_duplicate_is_held_and_safe_numeric_tracking_is_accepted(){
    let duplicate=analyze(&factory_bytes(false,true));
    assert_eq!(duplicate["applied"],0);assert_eq!(duplicate["held"],2);
    let numeric=analyze(&factory_bytes(true,false));
    assert_eq!(numeric["applied"],1);assert_eq!(numeric["held"],0);
    assert!(read_factory_reply("../bad.xlsx",&factory_bytes(false,false)).is_err());
    assert!(read_factory_reply("bad.xlsx",b"invalid").is_err());
}

fn factory_reply_columns(g:Option<f64>,q:Option<&str>)->Vec<u8>{
    let mut book=Workbook::new();
    let sheet=book.add_worksheet();
    for (c,v) in [(0,"수하인명"),(2,"수하인주소"),(3,"수하인전화번호"),
        (6,"택배운임"),(16,"운송장번호"),(17,"별도 운임")]{
        sheet.write_string(0,c,v).unwrap();
    }
    for (c,v) in [(0,"김수취"),(2,"서울 종로구 10"),(3,"010-1234-5678")]{
        sheet.write_string(1,c,v).unwrap();
    }
    if let Some(value)=g{sheet.write_number(1,6,value).unwrap();}
    if let Some(value)=q{sheet.write_string(1,16,value).unwrap();}
    sheet.write_number(1,17,9999.).unwrap();
    book.save_to_buffer().unwrap()
}

#[test]
fn factory_reply_requires_actual_g_and_tracking_q_without_substituting_r(){
    let missing_g=factory_reply_columns(None,Some("001234567890"));
    let parsed=read_factory_reply("합성.xlsx",&missing_g).unwrap();
    assert_eq!(parsed["sourceRows"][0]["rawFreight"],"");
    let held=analyze(&missing_g);
    assert_eq!(held["applied"],0);
    assert_eq!(held["held"],1);
    assert!(held["rows"][0]["issue"].as_str().unwrap().contains("G열"));
    for invalid in [-1.,0.5]{
        let held=analyze(&factory_reply_columns(Some(invalid),Some("001234567890")));
        assert_eq!(held["applied"],0);
        assert_eq!(held["held"],1);
    }
    for actual in [0.,4000.]{
        let valid=analyze(&factory_reply_columns(Some(actual),Some("001234567890")));
        assert_eq!(valid["applied"],1);
        assert_eq!(valid["held"],0);
        assert_eq!(valid["rows"][0]["actual"].as_i64(),Some(actual as i64));
    }
    let mut missing_q=read_factory_reply("합성.xlsx",&factory_reply_columns(Some(4000.),None)).unwrap();
    missing_q["state"]=factory_state();
    missing_q["businessDate"]=json!("2026-11-13");
    missing_q["at"]=json!("2026-11-13T10:00:00Z");
    missing_q["actor"]=json!("합성");
    let held=factory_reply::execute("analyze_factory_reply",&missing_q).unwrap();
    assert_eq!(held["held"],1);
    assert_eq!(held["rows"][0]["issueCode"],"INVALID_TRACKING");
}

#[test]
fn factory_reply_accepts_cached_formula_values_without_executing_formulas(){
    let mut book=Workbook::new();
    let sheet=book.add_worksheet();
    sheet.write_string(0,0,"수하인명").unwrap();
    sheet.write_string(1,0,"김수취").unwrap();
    sheet.write_formula(1,6,rust_xlsxwriter::Formula::new("=2000+2000").set_result("4000")).unwrap();
    sheet.write_formula(1,16,rust_xlsxwriter::Formula::new("=1234567890").set_result("1234567890")).unwrap();
    let bytes=book.save_to_buffer().unwrap();
    let source=read_factory_reply("formula.xlsx",&bytes).unwrap();
    assert_eq!(source["sourceRows"][0]["rawFreight"],"4000");
    assert_eq!(source["sourceRows"][0]["tracking"],"1234567890");
    let aggregate=&source["sourceRows"][0]["aggregateValues"];
    assert_eq!(aggregate.as_array().unwrap().len(),17);
    assert_eq!(aggregate[0],"김수취");
    assert_eq!(aggregate[6],"4000");
    assert_eq!(aggregate[16],"1234567890");
    assert!(source["sourceRows"][0].get("sourceIssue").is_none());
    let result=analyze(&bytes);
    assert_eq!(result["applied"],1);
    assert_eq!(result["held"],0);
}
#[test]
fn factory_reply_numeric_waybill_precision_and_leading_zero_checks_remain(){
    for (number,format) in [(1234567890123456.,"General"),(1234567890.,"000000000000")]{
        let mut book=Workbook::new();
        let sheet=book.add_worksheet();
        sheet.write_string(0,0,"수하인명").unwrap();
        sheet.write_string(1,0,"김수취").unwrap();
        sheet.write_number(1,6,4000.).unwrap();
        let style=rust_xlsxwriter::Format::new().set_num_format(format);
        sheet.write_number_with_format(1,16,number,&style).unwrap();
        let bytes=book.save_to_buffer().unwrap();
        let result=analyze(&bytes);
        assert_eq!(result["applied"],0);
        assert_eq!(result["held"],1);
    }
}

#[test]
fn factory_reply_allows_unrelated_merged_cells_and_rows_above_old_limit(){
    let mut book=Workbook::new();
    let sheet=book.add_worksheet();
    sheet.merge_range(0,9,0,12,"참고", &rust_xlsxwriter::Format::new()).unwrap();
    sheet.write_string(1001,0,"김수취").unwrap();
    sheet.write_number(1001,6,4000.).unwrap();
    sheet.write_string(1001,16,"001234567890").unwrap();
    let bytes=book.save_to_buffer().unwrap();
    let result=analyze(&bytes);
    assert_eq!(result["applied"],1);
    assert_eq!(result["held"],0);
}
