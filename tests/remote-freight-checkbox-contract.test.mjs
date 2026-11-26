import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("staff checkbox is the only source of 1x/2x and staff choice survives Rust resolution", () => {
  const rules = source("src-tauri/src/business_core/rules/remote_delivery.rs");
  assert.match(rules, /let saved_for_address = decision\.get\("checked"\)/);
  assert.match(rules, /let manually_set = saved_for_address/);
  assert.match(rules, /let checked = if saved_for_address/);
  assert.match(rules, /value\.checked_mul\(if checked \{2\} else \{1\}\)/);
  assert.match(rules, /fn auto_check_can_be_manually_overridden_and_address_change_resets_it\(\)/);
  assert.match(rules, /fn previously_billed_legacy_area_can_preserve_its_original_freight\(\)/);
});

test("no charge is pre-doubled or saved as a permanently multiplied tariff", () => {
  const shipping = source("src-tauri/src/business_core/rules/shipping.rs");
  const reconcile = source("src-tauri/src/business_core/rules/reconciliation.rs");
  assert.match(shipping, /let applied = if selected \{/);
  assert.match(shipping, /if selected \{bundle\["baseRecommendation"\]=json!\(recommendation\);\}/);
  assert.match(reconcile, /\.map\(\|b\|b\.get\("baseRecommendation"\)\s*\.unwrap_or_else\(\|\|field\(b,"recommendation"\)\)\)/);
  assert.match(reconcile, /set\(&mut charges,origin,v\.clone\(\)\)/);
  assert.match(shipping, /fn island_check_doubles_applied_recommendation_but_never_the_base\(\)/);
});

test("all native invoice paths and actual-delivery verification use checked multiplier", () => {
  const workflow = source("src-tauri/src/business_core/rules/workflow.rs");
  const preview = source("src-tauri/src/business_core/rules/workflow_preview.rs");
  assert.match(workflow, /pub\(super\) fn fee_total[\s\S]*?remote_delivery::multiply_shipment\(base,row,ship,order\)/);
  assert.match(workflow, /"complete_delivery" => \{[\s\S]*?remote_delivery::multiply_shipment\(/);
  assert.match(workflow, /let active=eligible_base &&/);
  assert.match(workflow, /let remote_candidate=matches!\(str_field\(&remote_state,"status"\),"auto"\|"review"\)/);
  assert.match(workflow, /let eligible_base=!remote_selected && !remote_candidate/);
  assert.match(workflow, /if eligible_base \{\s*for previous in state\["shipping"\]\["combinations"\]/);
  assert.match(workflow, /fn invoice_uses_one_checked_multiplier_on_summed_undoubled_base_charges\(\)/);
  assert.match(preview, /let remote = super::remote_delivery::resolve_shipment\(recipient,ship,order\)\?/);
  assert.match(preview, /super::remote_delivery::multiply_shipment\(safe\(base\),recipient,ship,order\)/);
  assert.match(preview, /shipment_projection\["remoteDelivery"\]=remote;/);
});

test("native egui remote-delivery preview retains review and one-time doubling", () => {
  const workspace = source("egui-app/src/manual_workspace.rs");
  const service = source("egui-app/src/reception_service.rs");
  const rules = source("src-tauri/src/business_core/rules/remote_delivery.rs");
  const board = source("egui-app/src/workflow_board.rs");
  assert.match(workspace, /resolve_remote_rows/);
  assert.match(workspace, /remote_rows/);
  assert.match(service, /remote_reference_matches/);
  assert.match(rules, /checked_mul/);
  assert.match(rules, /fn auto_check_can_be_manually_overridden_and_address_change_resets_it/);
  assert.match(board, /fn native_remote_preview_is_not_replaced_by_optional_manual_checkbox/);
});

test("already billed and dispatched orders do not retroactively acquire a charge", () => {
  const reconcile = source("src-tauri/src/business_core/rules/reconciliation.rs");
  assert.match(reconcile, /let old_bill=existing_ship\.is_some_and/);
  assert.match(reconcile, /flag\(ship,"transferred"\)/);
  assert.match(reconcile, /field\(ship,"step"\)\.as_i64\(\)\.unwrap_or\(0\)>=1/);
  assert.match(reconcile, /"address":current_address,"checked":false,\s*"manual":false,"legacy":true/);
  assert.match(reconcile, /"address":current_address,"checked":true,"manual":false/);
  const detector = source("src-tauri/src/business_core/rules/remote_delivery.rs");
  assert.match(detector, /fn unreconciled_legacy_paid_shipments_keep_their_original_amounts\(\)/);
  assert.match(detector, /pub\(super\) fn multiply_shipment\(/);
});
