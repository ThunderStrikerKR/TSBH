//! Pure JSON business-rule boundary. No SQLite, Tauri, clock or process globals.
//! Every mutation returns a new JSON value; persistence belongs to the integrator.
use serde_json::{json, Value};

mod pricing;
pub(crate) use pricing::ReceptionEditQuoteCache;
pub mod settings_accounts;
mod reconciliation;
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
pub(crate) use factory_reply::{import_automatic_factory_reply,prepare_automatic_factory_reply,apply_prepared_factory_reply};
mod legacy_migration;
mod shipping;
mod remote_delivery;
mod workflow;
mod workflow_issues;
mod workflow_preview;
mod validation;
mod order_save;
mod history;

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
        "order_issues_with_actions" => Ok(json!({
            "issues": issues::order_issues_with_actions(req(&payload,"order")?)
        })),
        "delivery_requirements" => issues::delivery_requirements(&payload),
        "delivery_requirements_with_messages" =>
            issues::delivery_requirements_with_messages(&payload),
        "apply_issue_action" => issues::issue_action(&payload),
        "apply_product_action" => product_confirmation::apply(&payload),
        "record_message_copy" => message_audit::record(&payload),
        "factory_reply_duplicate" | "factory_conflict_confirmation" | "approve_factory_conflict" | "factory_reply_review_statuses" | "analyze_factory_reply" | "preview_factory_reply" | "apply_factory_reply" =>
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
