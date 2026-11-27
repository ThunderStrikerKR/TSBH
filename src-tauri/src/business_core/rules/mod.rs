//! Pure JSON business-rule boundary. No SQLite, Tauri, clock or process globals.
//! Every mutation returns a new JSON value; persistence belongs to the integrator.
use serde_json::{json, Value};

/// Shared copy templates only. Each caller retains its own validation and
/// legacy-data projection; these functions cannot approve or mutate an order.
pub(crate) fn correction_message_header(shipment_id:&str)->Vec<String>{
    vec![
        "기존 출고 건 정정 요청입니다.".into(),
        format!("배송 ID: {shipment_id}"),
        "새 출고 주문이 아닙니다.".into(),
    ]
}
pub(crate) fn correction_change_line(label:&str,before:&str,after:&str)->String{
    format!("{label}: {before} → {after}")
}
pub(crate) fn factory_conflict_message(row:&str,name:&str,current_tracking:&str,
    current_fee:&str,reply_tracking:&str,reply_fee:&str)->String{
    format!("{row}행 / {name}\n기존: {current_tracking} / 운임 {current_fee}\n회신: {reply_tracking} / 운임 {reply_fee}\n이 한 행만 정정할까요? 기존 값과 원본 파일은 보존됩니다.")
}
#[cfg(test)]
#[test]
fn shared_correction_copy_retains_scope_and_before_after_values(){
    assert_eq!(correction_message_header("s1").join("\n"),
        "기존 출고 건 정정 요청입니다.\n배송 ID: s1\n새 출고 주문이 아닙니다.");
    assert_eq!(correction_change_line("공장 운임","미확정","0원"),"공장 운임: 미확정 → 0원");
    assert_eq!(factory_conflict_message("2","김수취","미등록","미확인","0012345678","3,000원"),
        "2행 / 김수취\n기존: 미등록 / 운임 미확인\n회신: 0012345678 / 운임 3,000원\n이 한 행만 정정할까요? 기존 값과 원본 파일은 보존됩니다.");
}

/// Display each correction amount independently, including missing values.
pub(crate) fn correction_fee_text(value:Option<&Value>)->String {
    let Some(v)=value.filter(|v|!v.is_null()) else {return "미확정".into();};
    let Some(n)=v.as_i64() else {return "확인 필요".into();};
    let digits=n.unsigned_abs().to_string();
    let groups=digits.as_bytes().rchunks(3).rev()
        .map(|part|std::str::from_utf8(part).expect("ASCII digits"))
        .collect::<Vec<_>>().join(",");
    format!("{}{}원",if n<0{"-"}else{""},groups)
}
#[cfg(test)]
#[test]
fn correction_fee_display_preserves_missing_zero_and_units(){
    assert_eq!(correction_fee_text(None),"미확정");
    assert_eq!(correction_fee_text(Some(&Value::Null)),"미확정");
    assert_eq!(correction_fee_text(Some(&json!(0))),"0원");
    assert_eq!(correction_fee_text(Some(&json!(3000))),"3,000원");
    assert_eq!(correction_fee_text(Some(&json!(1234567))),"1,234,567원");
    assert_eq!(correction_fee_text(Some(&json!("invalid"))),"확인 필요");
}

mod pricing;
/// Common business terminology, including the legacy read compatibility.
pub mod terminology {
    pub const MODEL_GROUP_MIN: &str = "모델군에는 상품을 2개 이상 등록하세요.";
    pub const MODEL_GROUP_DUPLICATE: &str = "모델군에 상품을 중복 등록할 수 없습니다.";
    pub const MODEL_GROUP_INDEPENDENT: &str = "등록된 독립 판매 상품만 모델군에 추가할 수 있습니다.";
    pub const MODEL_GROUP_REPRESENTATIVE: &str = "모델군에 속한 상품을 대표모델로 선택하세요.";
    pub const MODEL_GROUP_NAME_DUPLICATE: &str = "모델군 이름이 중복됩니다.";
    pub const ALIAS_SCOPE_CONFLICT: &str = "같은 적용 범위의 별칭이 충돌합니다.";
    pub const ALIAS_SCOPE_EXISTS: &str = "같은 적용 범위의 별칭이 이미 있습니다.";
    pub const ALIAS_MODEL_CONFLICT: &str = "다른 등록 상품명 또는 모델 표기와 충돌하는 별칭입니다.";
    pub const ALIAS_VARIANT_CONFLICT: &str = "상품명의 색상 또는 구성 표기와 충돌하는 별칭입니다.";
    pub const ALIAS_VARIANT_LINK: &str = "별칭 색상 / 옵션 연결을 확인하세요.";
    pub const ALIAS_EXISTING_LINK_CONFLICT: &str = "기존 별칭과 연결 상품이 충돌합니다. 기존 연결을 확인하세요.";
    pub const DB_SCHEMA_UNAVAILABLE: &str = "운영 DB 스키마를 확인할 수 없습니다.";
    pub const SHIPMENT_LINK_REQUIRED: &str = "배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.";
    pub const EXCEL_DIRECTORY_REQUIRED: &str = "설정에서 Excel 보관함 위치를 먼저 지정하고 저장하세요.";
    pub const FACTORY_REPLY_WRITE_DENIED: &str = "공장 회신 저장 권한 없음";
    pub const DISPATCH_FREIGHT_REQUIRED: &str = "출고 운임을 확인하세요.";
    pub fn settings_changed(field:&str)->String{
        format!("다른 화면에서 {field} 설정이 변경됐습니다. 다시 불러온 뒤 저장하세요.")
    }

    pub const UNASSIGNED_ACTOR: &str = "작업자 미지정";
    pub const FACTORY_DELIVERY_CONFIRMED: &str = "공장 전달 확인";

    pub fn canonical_event_action(action:&str)->&str{
        match action{
            "공장 전달 직원 확인"=>FACTORY_DELIVERY_CONFIRMED,
            _=>action,
        }
    }

    /// Translate persisted action names for display only; identity checks stay unchanged.
    pub fn display_event_action(action:&str)->&str{
        match action{
            "실제 출고 운임 입력"=>"출고 운임 입력",
            "실제 입금 기록"=>"입금 기록",
            "실제 배송 완료 / 운임 검증"=>"배송 완료 / 운임 검증",
            _=>canonical_event_action(action),
        }
    }

    /// Only translate known, generated factory-reply messages, not source text.
    pub fn display_factory_issue(issue:&str)->String{
        issue.replace("직원 확인 필요","확인 필요")
            .replace("실제 운임","출고 운임")
            .replace("실제 택배운임","출고 운임")
    }

    pub fn display_factory_field(field:&str)->&str{
        match field{
            "실제 운임 변경"=>"출고 운임 변경",
            _=>field,
        }
    }

    /// Render known legacy system placeholders without changing stored evidence.
    pub fn display_actor(actor:&str)->&str{
        match actor{
            ""|"직원"|"직원(이름 미설정)"=>UNASSIGNED_ACTOR,
            "과거 데이터 / 접수 직원 미확인"=>"과거 데이터 / 접수 작업자 미확인",
            _=>actor,
        }
    }

    pub fn display_reason(reason:&str)->&str{
        match reason{
            "직원이 운임 설정 변경"=>"운임 설정 변경",
            _=>reason,
        }
    }

    #[cfg(test)]
    mod tests{
        use super::*;
        #[test]
        fn legacy_terminology_is_display_only_and_event_aliases_match(){
            assert_eq!(canonical_event_action("공장 전달 직원 확인"),FACTORY_DELIVERY_CONFIRMED);
            assert_eq!(canonical_event_action(FACTORY_DELIVERY_CONFIRMED),FACTORY_DELIVERY_CONFIRMED);
            assert_eq!(canonical_event_action("공장 전달"),"공장 전달");
            assert_eq!(display_event_action("실제 출고 운임 입력"),"출고 운임 입력");
            assert_eq!(display_event_action("실제 입금 기록"),"입금 기록");
            assert_eq!(display_event_action("실제 배송 완료 / 운임 검증"),"배송 완료 / 운임 검증");
            assert_eq!(display_factory_issue("기존 송장 / 실제 운임과 충돌합니다. 직원 확인 필요"),"기존 송장 / 출고 운임과 충돌합니다. 확인 필요");
            assert_eq!(display_factory_field("실제 운임 변경"),"출고 운임 변경");
            assert_eq!(display_actor("직원"),UNASSIGNED_ACTOR);
            assert_eq!(display_actor("직원(이름 미설정)"),UNASSIGNED_ACTOR);
            assert_eq!(display_actor("김작업자"),"김작업자");
            assert_eq!(display_reason("직원이 운임 설정 변경"),"운임 설정 변경");
        }
    }
}

pub(crate) use pricing::{ReceptionEditQuoteCache,vat_applies};
pub mod settings_accounts;
mod reconciliation;
pub(crate) use reconciliation::{sender_for_export,refresh_pending_freight_preview};
mod issues;
mod product_confirmation;
mod message_audit;
mod order_mutations;
mod order_edit;
mod group_operations;
mod correction_rules;
mod reception_operations;
mod reception_merge;
mod kakao_title;
mod manual_classification;
mod manual_draft;
mod manual_structure;
mod message_template_validation;
mod catalog_item;
mod retail_operations;
mod raw_corrections;
mod reply_rules;
mod shipping_read;
mod client_change;
mod factory_reply;
pub(crate) use factory_reply::ensure_customer_payment_unchanged;
pub(crate) use factory_reply::{factory_reply_matches_shipment,import_automatic_factory_reply,prepare_automatic_factory_reply,apply_prepared_factory_reply,factory_correction_order_scope,approve_factory_conflict_for_state};
mod legacy_migration;
mod shipping;
mod remote_delivery;
mod workflow;
pub(crate) use workflow::party as workflow_party;
mod workflow_stages;
mod workflow_issues;
mod workflow_preview;
mod validation;
mod order_save;
mod history;

pub(crate) use issues::quantity_user_approved;
const MAX_SAFE: i64 = 9_007_199_254_740_991;

/// Shared identity normalization for UI projections that build a manual
/// model before the final native merge. Keep one implementation beside merge.
pub(crate) fn scope_recognition_rows_for_order(
    order_id:&str,recipients:&mut [Value],candidates:&mut Value
)->Result<(),String>{
    reception_merge::scope_recognition_ids(order_id,recipients,candidates)
}

/// The same identity rule used for model rows, source spans and edit targets.
pub(crate) fn scope_reception_edit_ids(order_id:&str,recipient_id:&str,item_id:Option<&str>)
    ->(String,Option<String>){
    (reception_merge::scoped_recognition_id(order_id,recipient_id,"recognition:recipient:"),
     item_id.map(|id|reception_merge::scoped_recognition_id(order_id,id,"recognition:item:")))
}

pub(crate) fn scope_manual_model_ids(
    order_id:&str,model:&mut Value
)->Result<(),String>{
    reception_merge::scope_manual_model_ids(order_id,model)
}

/// Borrow the authoritative states directly instead of cloning both
/// complete ledgers into an intermediate JSON command.
pub(crate) fn validate_workflow_transition(state:&Value,before:&Value)->Result<(),String>{
    validation::validate(state,Some(before))
}

/// Factory reply storage proves core equality before dropping the duplicate
/// trees. The original validator uses the same core for old recipient lookup.
pub(crate) fn validate_workflow_transition_shared_core(state:&Value,before:&Value)->Result<(),String>{
    validation::validate_with_prior_core(state,Some(before),Some(state))
}

pub fn execute(operation: &str, payload: Value) -> Result<Value, String> {
    match operation {
        "list_order_history" => history::list(
            req(&payload,"state")?,
            str_field(&payload,"query"),
            str_field(&payload,"date"),
        ),
        // The legacy single-date call stays identical. The new native
        // management UI opts into an inclusive start/end range and chooses
        // reception-day or allocated-bank-payment-day explicitly.
        "list_order_history_view" => {
            if payload.get("dateFrom").is_some() ||
                payload.get("dateTo").is_some() ||
                payload.get("dateBasis").is_some()
            {
                history::list_view_range(
                    req(&payload,"state")?,str_field(&payload,"query"),
                    str_field(&payload,"dateFrom"),str_field(&payload,"dateTo"),
                    str_field(&payload,"dateBasis"),field(&payload,"trackingOrder")
                )
            }else{
                history::list_view(
                    req(&payload,"state")?,str_field(&payload,"query"),
                    str_field(&payload,"date"),field(&payload,"trackingOrder")
                )
            }
        },
        "order_history_status" => Ok(json!({"status":history::status(
            req(&payload,"state")?,required_str(&payload,"orderId")?,
        )})),
        "save_order" => order_save::save(&payload),
        "order_edit_rules" => order_edit::execute(&payload),
        "calculate_price" => pricing::totals(req(&payload, "order")?, req(&payload, "client")?),
        "resolve_unit_price" => pricing::resolve_unit_price(
            req(&payload, "product")?,
            required_str(&payload, "variantId")?,
            req(&payload, "client")?,
        ).map(|price| json!({"unitPrice": price})),
        "resolve_unit_prices" => {
            let client=req(&payload,"client")?;
            let prices=rows(req(&payload,"quotes")?).iter().map(|quote|{
                pricing::resolve_unit_price(
                    req(quote,"product")?,
                    required_str(quote,"variantId")?,
                    client,
                )
            }).collect::<Result<Vec<_>,_>>()?;
            Ok(json!({"prices":prices}))
        },
        "reprice_order" => pricing::reprice_order(&payload),
        "calculate_reception_quote" => pricing::reception_quote(&payload),
        "should_refresh_pricing" => Ok(json!({"refresh": pricing::should_refresh(&payload)?})),
        "assess_delivery_address" => {
            let value=required_str(&payload,"value")?;
            Ok(json!({
                "valid":issues::valid_address(value),
                "addressParts":manual_classification::manual_address_parts(value),
                "addressStatus":if issues::valid_address(value){"COMPLETE"}else{"INCOMPLETE"},
            }))
        },
        "shipping_manager_view" => shipping::shipping_manager_view(&payload),
        "apply_shipping_action" => shipping::shipping_action(&payload),
        "shipping_read" => shipping_read::project(&payload),
        "detect_remote_delivery_area" => remote_delivery::detect(&payload),
        "resolve_remote_delivery_area" => remote_delivery::resolve_recipient(
            req(&payload,"recipient")?
        ),
        "normalize_remote_freight_input" => remote_delivery::normalize_applied_input(&payload),
        "reconcile_workflow" => reconciliation::reconcile(&payload),
        "reconcile_shipment_workflow" => reconciliation::reconcile_shipment(&payload),
        "calculate_shipping" => shipping::recommend(
            req(&payload, "order")?, req(&payload, "state")?,
        ),
        "calculate_shipment_totals" => workflow::shipment_totals(
            req(&payload, "state")?, required_str(&payload, "shipmentId")?,
        ),
        "evaluate_workflow" => workflow::evaluate(&payload),
        "preview_workflow" => workflow_preview::project(&payload),
        "preview_order" => workflow_preview::project_order(&payload),
        "preview_order_lines" => workflow_preview::project_order_lines(&payload),
        "workflow_issues" => Ok(json!({"issues":workflow_issues::workflow_issues(
            req(&payload,"state")?,required_str(&payload,"groupId")?)?})),
        "stage_issues" => Ok(json!({"issues":workflow_issues::stage_issues(
            req(&payload,"state")?,required_str(&payload,"groupId")?,
            req(&payload,"next")?.as_i64().ok_or("진행 단계 숫자가 필요합니다.")?)?})),
        "read_shipments" => workflow::read_shipments(&payload),
        "evaluate_dispatch_state" => workflow::dispatch_state(
            req(&payload, "state")?, required_str(&payload, "orderId")?,
        ),
        "order_issues" => Ok(json!({"issues": issues::order_issues(req(&payload,"order")?)})),
        "order_issues_with_actions" => {
            let order=req(&payload,"order")?;
            let mut result=issues::order_issues_with_actions(order);
            if let Some(state)=payload.get("state"){
                result.extend(workflow_issues::sale_issues(state,order,""));
            }
            Ok(json!({"issues":result}))
        },
        "delivery_requirements" => issues::delivery_requirements(&payload),
        "delivery_requirements_with_messages" =>
            issues::delivery_requirements_with_messages(&payload),
        "apply_issue_action" => issues::issue_action(&payload),
        "apply_product_action" => product_confirmation::apply(&payload),
        "record_message_copy" => message_audit::record(&payload),
        "factory_reply_candidates" | "factory_reply_duplicate" | "factory_conflict_confirmation" | "approve_factory_conflict" | "factory_reply_review_statuses" | "analyze_factory_reply" | "preview_factory_reply" | "apply_factory_reply" =>
            factory_reply::execute(operation, &payload),
        "apply_reply_action" => reply_rules::apply(&payload),
        "finalize_billing_history" => reply_rules::finalize_billing_history(&payload),
        "client_change_issue" => client_change::check(&payload),
        "apply_raw_correction" => raw_corrections::apply(&payload),
        "apply_retail_action" => retail_operations::apply(&payload),
        "apply_reception_action" => reception_operations::apply(&payload),
        "merge_reception_analysis" => reception_merge::execute(&payload),
        "apply_manual_classification" => match str_field(&payload, "action") {
            "initial_manual" | "set_manual_product" | "assign_manual_block" | "release_manual_block" =>
                manual_draft::execute(&payload),
            "resize_manual_rows" | "add_manual_item" | "remove_manual_item" |
            "confirm_manual_quantity" | "set_manual_quantity_draft" |
            "step_manual_quantity" | "reset_manual_rows" =>
                manual_structure::execute(&payload),
            _ => manual_classification::apply(&payload)
        },
        "validate_automatic_templates" => message_template_validation::validate(&payload),
        "validate_provisional_product_notices" => message_template_validation::validate_provisional(&payload),
        "create_catalog_item" => catalog_item::make(&payload),
        "remember_kakao_title_client" | "remembered_kakao_title_client" =>
            kakao_title::execute(operation, &payload),
        "apply_correction_action" => correction_rules::apply(&payload),
        "apply_group_action" => group_operations::mutate(&payload),
        "delete_order" => order_mutations::delete_order(&payload),
        "apply_order_raw_edit" => order_mutations::apply_raw_edit(&payload),
        "migrate_workflow" => legacy_migration::migrate(&payload),
        "validate_workflow" => {
            validation::validate(req(&payload, "state")?, payload.get("before"))?;
            Ok(json!({"valid": true}))
        },
        "apply_workflow_action" => workflow::apply(&payload),
        "validate_reception_boundary" => {
            validation::validate_reception(
                req(&payload, "before")?, req(&payload, "state")?,
            )?;
            Ok(json!({"valid": true}))
        },
        _ => Err(format!("지원하지 않는 업무 operation: {operation}")),
    }
}

fn req<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value.get(key).ok_or_else(|| format!("필수 입력 누락: {key}"))
}
fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str)
        .ok_or_else(|| format!("필수 문자열 누락: {key}"))
}
fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}
fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    field(value, key).as_str().unwrap_or("")
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn flag(value: &Value, key: &str) -> bool {
    field(value, key).as_bool() == Some(true)
}
fn safe(value: &Value) -> Option<i64> {
    value.as_i64().filter(|n| (0..=MAX_SAFE).contains(n))
}
// Manual amounts have three distinct states: unset (automatic), valid
// explicit amount (including zero), or invalid (never silently replaced).
pub(crate) fn manual_amount(value:&Value)->Result<Option<i64>,String>{
    if value.is_null(){return Ok(None);}
    safe(value).map(Some).ok_or_else(||"직접 입력한 금액은 0 이상의 안전한 정수여야 합니다.".into())
}
pub(super) fn manual_unit_price(item:&Value)->Result<Option<i64>,String>{
    manual_amount(field(item,"manualUnitPrice"))
}
pub(super) fn manual_freight(recipient:&Value,origin:&str)->Result<Option<i64>,String>{
    manual_amount(field(field(recipient,"shippingCharges"),origin))
}
#[cfg(test)]
mod manual_amount_priority_tests{
    use super::*;
    #[test]
    fn missing_zero_positive_and_invalid_amounts_remain_distinct(){
        for value in [Value::Null,json!(0),json!(7000),json!(MAX_SAFE)]{
            let expected=value.as_i64();
            assert_eq!(manual_amount(&value).unwrap(),expected);
            assert_eq!(manual_unit_price(&json!({"manualUnitPrice":value})).unwrap(),expected);
            assert_eq!(manual_freight(&json!({"shippingCharges":{"FACTORY":value}}),"FACTORY").unwrap(),expected);
        }
        assert_eq!(manual_unit_price(&json!({})).unwrap(),None);
        assert_eq!(manual_freight(&json!({}),"STORE").unwrap(),None);
        for invalid in [json!(-1),json!(1.5),json!("7000"),json!(MAX_SAFE+1),json!(true)]{
            assert!(manual_amount(&invalid).is_err());
        }
    }
}
fn checked_sum(values: impl IntoIterator<Item = i64>) -> Option<i64> {
    values.into_iter().try_fold(0_i64, |a, b| {
        a.checked_add(b).filter(|n| (0..=MAX_SAFE).contains(n))
    })
}
fn obj_mut(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    value.as_object_mut().ok_or_else(|| "JSON 객체 형식을 확인하세요.".to_string())
}
fn set(value: &mut Value, key: &str, other: Value) -> Result<(), String> {
    obj_mut(value)?.insert(key.to_string(), other);
    Ok(())
}
fn find_by_id<'a>(values: &'a Value, id: &str) -> Option<&'a Value> {
    rows(values).iter().find(|v| str_field(v, "id") == id)
}
fn has_value(value: &Value) -> bool {
    !value.is_null()
}
