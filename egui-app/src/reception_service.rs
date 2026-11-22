//! Read-only reception orchestration.
//! It depends only on BandoGateway, never on transport, SQL or Tauri.
//! Recognition and rule decisions belong to the configured gateway.
//!
//! Any displayed retail figure is reference-only, never an invoice total.
//! This service performs no database writes.

use serde_json::{json, Value};
use std::ops::Deref;
use crate::{gateway::{BandoGateway, ReadOperation}, source_highlight};

#[derive(Debug)]
pub struct PriceReference {
    /// From the real Rust "resolve_unit_price" rule at the *retail* tier.
    /// This is a catalog reference only, never a confirmed recipient price.
    pub retail: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct RecipientReference {
    /// Rust resolved remote-area suggestion, checked state and matched reason.
    /// This is a read-only decision tied to the exact recipient address.
    pub remote: Option<Value>,
    pub remote_error: Option<String>,
    pub item_prices: Vec<PriceReference>,
}

#[derive(Debug)]
pub struct ReceptionAnalysis {
    pub recognition: Value,
    pub reference: Vec<RecipientReference>,
    /// Advisory until an actual order-client change is committed in Rust.
    pub client_match: Value,
    pub client_candidates: Vec<Value>,
    pub matched_client_name: Option<String>,
    pub chosen_client_name: Option<String>,
    pub directory_count: usize,
    /// Rust-owned UTF-16 source locations, projected once per analysis.
    pub linked_spans: Vec<source_highlight::LinkedSpan>,
}

/// Persist only display references and the native candidate projection.
/// The source-bound Rust recognition and client match remain the canonical
/// copies in WorkingReception, so reopening never calls AnalyzeOrder again.
impl ReceptionAnalysis {
    pub fn cached_view(&self)->Value {
        let reference=self.reference.iter().map(|row|json!({
            "remote":row.remote,"remoteError":row.remote_error,
            "itemPrices":row.item_prices.iter().map(|item|json!({
                "retail":item.retail,"error":item.error
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>();
        json!({
            "reference":reference,
            "clientCandidates":self.client_candidates,
            "matchedClientName":self.matched_client_name,
            "chosenClientName":self.chosen_client_name,
            "directoryCount":self.directory_count
        })
    }

    /// Apply a native manual reducer result without rerunning OCR or price
    /// calculations. Reference-only retail suggestions are cleared when the
    /// product rows change; remote decisions are retained only for the exact
    /// same recipient/address/manual override.
    pub fn apply_edited_recipients(&mut self,raw:&str,rows:&Value) {
        let old=recipients(&self.recognition).to_vec();
        let references=std::mem::take(&mut self.reference);
        self.reference=rows.as_array().into_iter().flatten().enumerate()
            .map(|(index,recipient)|{
                let previous=old.get(index);
                let mut reference=references.get(index)
                    .filter(|_|previous.is_some_and(|old|
                        old.get("id")==recipient.get("id")))
                    .map(|old|RecipientReference {
                        remote:old.remote.clone(),
                        remote_error:old.remote_error.clone(),
                        item_prices:old.item_prices.iter().map(|p|PriceReference{
                            retail:p.retail,error:p.error.clone()
                        }).collect(),
                    })
                    .unwrap_or(RecipientReference {
                        remote:None,remote_error:None,item_prices:Vec::new()
                    });
                if previous.and_then(|row|row.get("items"))!=recipient.get("items") {
                    reference.item_prices=recipient.get("items")
                        .and_then(Value::as_array).into_iter().flatten().map(|_|{
                            PriceReference{retail:None,error:None}
                        }).collect();
                }
                if reference.item_prices.len()!=recipient.get("items")
                    .and_then(Value::as_array).map(Vec::len).unwrap_or(0) {
                    reference.item_prices=recipient.get("items")
                        .and_then(Value::as_array).into_iter().flatten().map(|_|{
                            PriceReference{retail:None,error:None}
                        }).collect();
                }
                if reference.remote.as_ref().is_some_and(|decision|
                    !remote_reference_matches(recipient,decision)) {
                    reference.remote=None;
                    reference.remote_error=None;
                }
                reference
            }).collect();
        self.recognition["recipients"]=rows.clone();
        self.linked_spans=source_highlight::collect(raw,&self.recognition);
    }

    pub fn from_cached_view(
        raw:&str,recognition:Value,client_match:Value,view:&Value
    )->Option<Self>{
        let recipients=recipients(&recognition);
        let saved=view.get("reference")?.as_array()?;
        if saved.len()!=recipients.len(){return None;}
        let reference=saved.iter().zip(recipients).map(|(row,recipient)|{
            let prices=row.get("itemPrices")?.as_array()?;
            if prices.len()!=recipient.get("items").and_then(Value::as_array)
                .map(Vec::len).unwrap_or(0){return None;}
            let item_prices=prices.iter().map(|item|Some(PriceReference{
                retail:item.get("retail").and_then(Value::as_i64),
                error:item.get("error").and_then(Value::as_str)
                    .map(str::to_owned)
            })).collect::<Option<Vec<_>>>()?;
            let remote=row.get("remote").filter(|v|!v.is_null()).cloned()
                .filter(|decision|remote_reference_matches(recipient,decision));
            Some(RecipientReference{
                remote,remote_error:row.get("remoteError").and_then(Value::as_str)
                    .map(str::to_owned),
                item_prices
            })
        }).collect::<Option<Vec<_>>>()?;
        let client_candidates=view.get("clientCandidates")?.as_array()?.clone();
        let directory_count=usize::try_from(view.get("directoryCount")?.as_u64()?).ok()?;
        Some(Self {
            linked_spans:source_highlight::collect(raw,&recognition),
            recognition,reference,client_match,client_candidates,
            matched_client_name:view.get("matchedClientName").and_then(Value::as_str)
                .map(str::to_owned),
            chosen_client_name:view.get("chosenClientName").and_then(Value::as_str)
                .map(str::to_owned),
            directory_count,
        })
    }
}

impl Deref for ReceptionAnalysis {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.recognition
    }
}

/// Analyze a source, then derive separate *read-only* references through
/// the selected backend (local Rust or an explicitly configured HTTPS API). Build all results in
/// the existing worker thread; never run these computations every UI frame.
pub fn analyze_raw(
    gateway: &dyn BandoGateway, raw: &str,
    windows: Value, active_chat_handle: Option<String>,
    manual_client_id: Option<String>,
) -> Result<ReceptionAnalysis, String> {
    if raw.trim().is_empty() {
        return Err("발주원문을 먼저 입력하세요.".to_owned());
    }
    let state = gateway.load_catalog()?;
    let directory = state.get("clients").and_then(Value::as_array);
    let directory_count = directory.map(Vec::len).unwrap_or(0);

    // The same Rust matcher used by Tauri owns all client-selection logic.
    // Only OS-verified and explicitly selected KakaoTalk windows may enter
    // this boundary. Unselected/invalid handles are represented as [].
    let context = json!({
        "state": &state, "raw": raw,
        "windows": windows,
        "activeChatHandle": active_chat_handle,
    });
    let client_match = gateway.execute(
        ReadOperation::AutoMatchReceptionClient, context.clone()
    )?;
    let candidates = gateway.execute(
        ReadOperation::ReceptionClientCandidates, context
    )?;
    let client_candidates = candidates.get("candidates").and_then(Value::as_array)
        .cloned().unwrap_or_default();

    let client_for = |id: &str| {
        directory?.iter().find(|client|
            text(client, "id") == id &&
            client.get("deletedAt").is_none_or(Value::is_null) &&
            client.get("active") != Some(&Value::Bool(false))
        )
    };
    let automatic = client_match.get("clientId").and_then(Value::as_str)
        .and_then(client_for);
    let matched_client_name = automatic.map(|client| text(client, "name").to_owned());
    let selected = manual_client_id.as_deref().and_then(client_for).or(automatic);
    let chosen_client_name = selected.map(|client| text(client, "name").to_owned());

    // The client ID is valid for Rust product aliases, but this name-only
    // index has NO price tier. Passing only the ID deliberately leaves price
    // calculation unconfirmed; a future production rule owns billing.
    let recognition_client = selected.map(|client| json!({
        "id": text(client, "id"),
    }));
    let analysis = gateway.execute(ReadOperation::AnalyzeOrder, json!({
        "raw": raw, "state": &state, "client": recognition_client,
    }))?;
    let linked_spans=source_highlight::collect(raw,&analysis);
    let reference = recipients(&analysis).iter()
        .map(|recipient| recipient_reference(gateway, recipient, &state))
        .collect();
    Ok(ReceptionAnalysis {
        recognition: analysis,
        reference,
        client_match,
        client_candidates,
        matched_client_name,
        chosen_client_name,
        directory_count,
        linked_spans,
    })
}


/// Resolve the checked state on the complete recipient, not just the
/// address. The native engine owns auto detection, manual override and
/// address-change invalidation; the UI never guesses the checkbox value.
pub fn remote_decision(
    gateway:&dyn BandoGateway,recipient:&Value
)->Result<Value,String>{
    gateway.execute(ReadOperation::ResolveRemoteDeliveryArea,json!({
        "recipient":recipient
    }))
}

/// A completed Rust decision is usable only for the exact displayed row.
/// In particular, an older decision for the same address must not briefly
/// override a newer employee toggle while its Rust worker recalculates.
pub fn remote_reference_matches(recipient:&Value,decision:&Value)->bool{
    let address=text(recipient,"address");
    if decision["address"].as_str()!=Some(address){return false;}
    let manual=recipient.get("remoteDelivery")
        .filter(|value|value["address"].as_str()==Some(address)
            && value["manual"]==true)
        .and_then(|value|value["checked"].as_bool());
    match manual {
        Some(checked)=>decision["manual"]==true
            && decision["checked"].as_bool()==Some(checked),
        None=>decision["manual"]!=true,
    }
}

/// Read-only, synchronous helper used ONLY inside existing background workers
/// after a manual model has replaced the initial recognition projection.
pub fn refresh_remote_references(
    gateway:&dyn BandoGateway,analysis:&mut ReceptionAnalysis
){
    let recipients=recipients(&analysis.recognition).to_vec();
    for (index,recipient) in recipients.iter().enumerate(){
        let result=if text(recipient,"address").trim().is_empty(){
            Ok(None)
        }else{
            remote_decision(gateway,recipient).map(Some)
        };
        let entry=if let Some(reference)=analysis.reference.get_mut(index){
            reference
        }else{
            analysis.reference.push(RecipientReference{
                remote:None,remote_error:None,item_prices:Vec::new()
            });
            analysis.reference.last_mut().expect("pushed recipient reference")
        };
        match result{
            Ok(value)=>{entry.remote=value;entry.remote_error=None;},
            Err(error)=>{entry.remote=None;entry.remote_error=Some(error);}
        }
    }
    analysis.reference.truncate(recipients.len());
}

/// Display wording only; status, match path, checked state and reason are
/// always those returned by the Rust remote-area engine.
pub fn remote_delivery_notice(decision:&Value)->Option<String>{
    let status=decision.get("status").and_then(Value::as_str).unwrap_or("unknown");
    let checked=decision.get("checked").and_then(Value::as_bool).unwrap_or(false);
    let path=decision.get("matchedPath").and_then(Value::as_str)
        .unwrap_or("주소 기준 미확정");
    let reason=decision.get("reason").and_then(Value::as_str).unwrap_or("");
    let evidence=if reason.is_empty(){path.to_owned()}
        else{format!("{path} · {reason}")};
    match status{
        "auto"=>Some(if checked{
            format!("도서/산간 자동 판정: {evidence} · 운임 2배 적용")
        }else{
            format!("도서/산간 판정: {evidence} · 수동으로 적용 해제")
        }),
        "review"=>Some(if checked{
            format!("도서/산간 확인 필요: {evidence} · 수동 적용으로 운임 2배")
        }else{
            format!("도서/산간 확인 필요: {evidence} · 확인 후 체크")
        }),
        _ if checked=>Some("도서/산간 수동 적용 · 운임 2배 적용".to_owned()),
        _=>None,
    }
}

fn recipient_reference(gateway: &dyn BandoGateway, recipient: &Value, state: &Value) -> RecipientReference {
    let address = text(recipient, "address");
    let (remote, remote_error) = if address.trim().is_empty() {
        (None,None)
    }else{
        match remote_decision(gateway,recipient){
            Ok(result)=>(Some(result),None),
            Err(error)=>(None,Some(error))
        }
    };

    let item_prices = recipient.get("items").and_then(Value::as_array)
        .map(|items| items.iter().map(|item| retail_reference(gateway, item, state)).collect())
        .unwrap_or_default();
    RecipientReference { remote, remote_error, item_prices }
}

fn retail_reference(gateway: &dyn BandoGateway, item: &Value, state: &Value) -> PriceReference {
    let product_id = text(item, "productId");
    let variant_id = text(item, "variantId");
    if product_id.is_empty() || variant_id.is_empty() {
        return PriceReference { retail: None, error: None };
    }
    let Some(product) = state.get("products").and_then(Value::as_array)
        .and_then(|products| products.iter().find(|product|
            text(product, "id") == product_id
        ))
    else {
        return PriceReference { retail: None, error: None };
    };

    // The pure Rust rule requires a nonempty client ID to resolve a price.
    // This explicitly synthetic identity is NEVER saved. It requests a
    // read-only retail-tier reference, not an actual client's billing tier.
    match gateway.execute(ReadOperation::ResolveRetailUnitPrice, json!({
        "product": product,
        "variantId": variant_id,
        "client": { "id": "__egui_readonly_retail_reference__", "tier": "retail" },
    })) {
        Ok(value) => PriceReference {
            retail: value.get("unitPrice").and_then(Value::as_i64)
                .filter(|amount| *amount > 0),
            error: None,
        },
        Err(error) => PriceReference { retail: None, error: Some(error) },
    }
}

pub fn recipients(result: &Value) -> &[Value] {
    result.get("recipients").and_then(Value::as_array)
        .map(Vec::as_slice).unwrap_or(&[])
}

pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn item_title(item: &Value) -> String {
    let title = text(item, "name");
    if title.trim().is_empty() {
        let original = text(item, "raw");
        if original.trim().is_empty() {
            return "미확정 상품".to_owned();
        }
        return original.to_owned();
    }
    title.to_owned()
}

pub fn item_quantity(item: &Value) -> String {
    match item.get("quantity").and_then(Value::as_i64) {
        Some(quantity) if quantity > 0 => format!("{quantity}개"),
        _ => "수량 확인 필요".to_owned(),
    }
}

/// Display formatting only. The monetary amount is always calculated by
/// the Rust rules above; formatting it never changes the business decision.
pub fn display_won(amount: i64) -> String {
    let digits = amount.to_string();
    let mut output = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    format!("{output}원")
}

#[cfg(test)]
mod remote_notice_tests{
    use super::*;
    #[test]
    fn remote_result_cannot_outlive_a_manual_override_or_address_change(){
        let row=json!({"address":"제주특별자치도 제주시 1"});
        let auto=json!({"status":"auto","checked":true,"manual":false,
            "address":"제주특별자치도 제주시 1"});
        assert!(remote_reference_matches(&row,&auto));
        let revised=json!({"address":"제주특별자치도 제주시 1",
            "remoteDelivery":{"address":"제주특별자치도 제주시 1",
                "checked":false,"manual":true}});
        assert!(!remote_reference_matches(&revised,&auto));
        let manual=json!({"status":"auto","checked":false,"manual":true,
            "address":"제주특별자치도 제주시 1"});
        assert!(remote_reference_matches(&revised,&manual));
        let changed=json!({"address":"서울특별시 중구 1"});
        assert!(!remote_reference_matches(&changed,&auto));
        assert!(!remote_reference_matches(&row,&manual));
    }

    #[test]
    fn render_auto_checked_and_manual_override_without_ui_classification(){
        let detection=json!({"status":"auto","checked":true,
            "matchedPath":"제주특별자치도",
            "reason":"제주특별자치도 전역: 항공·도서 권역"});
        let checked=remote_delivery_notice(&detection).unwrap();
        assert!(checked.contains("자동 판정"));
        assert!(checked.contains("제주특별자치도"));
        assert!(checked.contains("운임 2배"));
        let mut override_detection=detection;
        override_detection["checked"]=json!(false);
        let unchecked=remote_delivery_notice(&override_detection).unwrap();
        assert!(unchecked.contains("적용 해제"));
        let review=json!({"status":"review","checked":false,
            "matchedPath":"군산시 옥도면 신시도리",
            "reason":"연륙교 연결 지역"});
        assert!(remote_delivery_notice(&review).unwrap().contains("확인 필요"));
        let normal=json!({"status":"normal","checked":false});
        assert!(remote_delivery_notice(&normal).is_none());
    }
}

#[cfg(test)]
mod persisted_analysis_restore_tests {
    use super::*;

    #[test]
    fn saved_native_evidence_rehydrates_without_analyzer_or_price_gateway(){
        let raw="김가상";
        let recognition=json!({
            "recipients":[{
                "id":"r1","name":"김가상","address":"서울시 중구",
                "items":[{"id":"i1","productId":"p1","variantId":"v1"}],
                "sources":[{"targetId":"r1","field":"name",
                    "start":0,"end":3,"text":"김가상"}]
            }],
            "candidates":[],"unassigned":[]
        });
        let analysis=ReceptionAnalysis{
            linked_spans:source_highlight::collect(raw,&recognition),
            recognition:recognition.clone(),
            reference:vec![RecipientReference{
                remote:Some(json!({"address":"서울시 중구","manual":false,
                    "checked":false,"status":"normal"})),
                remote_error:None,
                item_prices:vec![PriceReference{retail:Some(10000),error:None}]
            }],
            client_match:json!({"clientId":"c1"}),
            client_candidates:vec![json!({"clientId":"c1"})],
            matched_client_name:Some("거래처".into()),
            chosen_client_name:Some("거래처".into()),
            directory_count:23,
        };
        let view=analysis.cached_view();
        let restored=ReceptionAnalysis::from_cached_view(
            raw,recognition,analysis.client_match.clone(),&view
        ).unwrap();
        assert_eq!(restored.reference.len(),1);
        assert_eq!(restored.reference[0].item_prices[0].retail,Some(10000));
        assert!(restored.reference[0].remote.is_some());
        assert_eq!(restored.client_candidates,analysis.client_candidates);
        assert_eq!(restored.directory_count,23);
        assert_eq!(restored.chosen_client_name.as_deref(),Some("거래처"));
        assert_eq!(restored.linked_spans.len(),1);
        assert_eq!(restored.linked_spans[0].key.field,"name");
    }

    #[test]
    fn changed_recipient_structure_cannot_reuse_incompatible_reference(){
        let raw="김가상";
        let recognition=json!({"recipients":[{"id":"r1","items":[]}],
            "candidates":[],"unassigned":[]});
        let empty=ReceptionAnalysis{
            recognition:recognition.clone(),
            reference:vec![RecipientReference{
                remote:None,remote_error:None,item_prices:Vec::new()
            }],
            client_match:json!({}),client_candidates:Vec::new(),
            matched_client_name:None,chosen_client_name:None,
            directory_count:0,linked_spans:Vec::new(),
        };
        let view=empty.cached_view();
        let mut changed=recognition;
        changed["recipients"][0]["items"]=json!([{"id":"new"}]);
        assert!(ReceptionAnalysis::from_cached_view(
            raw,changed,json!({}),&view
        ).is_none());
    }

    #[test]
    fn edited_contact_keeps_reference_and_rebuilds_links_without_price_rules(){
        let raw="김가상";
        let recog=json!({"recipients":[{
            "id":"r1","name":"김가상","address":"서울",
            "items":[{"id":"i1","quantity":2}],
            "sources":[{"targetId":"r1","field":"name",
                "start":0,"end":3,"text":"김가상"}]
        }]});
        let mut analysis=ReceptionAnalysis{
            linked_spans:Vec::new(),recognition:recog.clone(),
            reference:vec![RecipientReference{
                remote:Some(json!({"address":"서울","manual":false,"checked":false})),
                remote_error:None,
                item_prices:vec![PriceReference{retail:Some(6000),error:None}]
            }],
            client_match:json!({}),client_candidates:Vec::new(),
            matched_client_name:None,chosen_client_name:None,directory_count:0,
        };
        let mut update=recog["recipients"].clone();
        update[0]["note"]=json!("수정");
        analysis.apply_edited_recipients(raw,&update);
        assert_eq!(analysis.reference[0].item_prices[0].retail,Some(6000));
        assert_eq!(analysis.linked_spans.len(),1);
        let mut address_change=update;
        address_change[0]["address"]=json!("제주");
        analysis.apply_edited_recipients(raw,&address_change);
        assert!(analysis.reference[0].remote.is_none());
    }
}
