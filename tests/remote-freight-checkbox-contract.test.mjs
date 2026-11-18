import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("staff checkbox is the only source of 1x/2x and staff choice survives Rust resolution", () => {
  const rules = source("src-tauri/src/business_core/rules/remote_delivery.rs");
  const types = source("src/types.ts");
  assert.match(types, /remoteDelivery\?: \{ address: string; checked: boolean; manual: true \}/);
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
  assert.match(workflow, /pub\(super\) fn fee_total[\s\S]*?remote_delivery::multiply\(base,row\)/);
  assert.match(workflow, /"complete_delivery" => \{[\s\S]*?remote_delivery::multiply\(/);
  assert.match(workflow, /let active=!remote_selected &&/);
  assert.match(workflow, /if !remote_selected \{\s*for previous in state\["shipping"\]\["combinations"\]/);
  assert.match(workflow, /fn invoice_uses_one_checked_multiplier_on_summed_undoubled_base_charges\(\)/);
  assert.match(preview, /let remote = super::remote_delivery::resolve_recipient\(recipient\)\?/);
  assert.match(preview, /super::remote_delivery::multiply\(safe\(base\),recipient\)/);
  assert.match(preview, /shipment_projection\["remoteDelivery"\]=remote;/);
});

test("UI automatically checks Rust-detected area but preserves manual checkbox corrections", () => {
  const desk = source("src/OrderDesk.tsx");
  const footer = source("src/WorkflowFooter.tsx");
  const preview = source("src/orders/useNativeWorkflowBoard.ts");
  assert.match(preview, /remoteDelivery\?: \{/);
  assert.match(desk, /remoteChecked=\{[\s\S]*?r\.remoteDelivery\?\.manual === true &&[\s\S]*?: freightEditor\.preview\.remoteDelivery\?\.checked === true/);
  assert.match(desk, /row\.remoteDelivery = \{\s*address: row\.address, checked, manual: true/);
  assert.match(desk, /if \(r\.address !== e\.target\.value\) delete r\.remoteDelivery/);
  assert.match(footer, /className="freight-remote-check"/);
  assert.match(footer, /aria-label="도서\/산간"/);
  assert.match(footer, /checked=\{remoteChecked\}/);
  assert.match(footer, /onChange=\{\(e\) => onRemoteChecked\(e\.currentTarget\.checked\)\}/);
  const checkboxIndex = footer.indexOf('className="freight-remote-check"');
  const buttonIndex = footer.indexOf('className="freight-inquiry"');
  assert.ok(checkboxIndex > 0 && checkboxIndex < buttonIndex,
    "checkbox must be immediately before the factory inquiry button");
  assert.match(footer, /remoteRowOrigin = origins\.includes\("FACTORY"\) \? "FACTORY" : origins\[0\]/);
  assert.match(footer, /origin === remoteRowOrigin && \(/);
});

test("blue message prompts review; no 2x is applied on top of the edited applied amount", () => {
  const desk = source("src/OrderDesk.tsx");
  const footer = source("src/WorkflowFooter.tsx");
  const css = source("src/styles/patch-v0808.css");
  assert.match(footer, /도서·산간 지역으로 판단했습니다\. 확인 후 체크 여부를 결정하세요\./);
  assert.match(footer, /도서·산간 지역일 수 있습니다\. 주소를 확인 후 체크 여부를 결정하세요\./);
  assert.match(css, /\.freight-remote-info\s*\{[^}]*background: rgba\([^}]*color: #70b4ff;/s);
  assert.match(desk, /displayCharges=\{freightEditor\.preview\.displayCharges\}/);
  assert.doesNotMatch(desk, /\.\.\.freightEditor\.preview\.displayCharges,\s*\.\.\.r\.shippingCharges/);
  assert.match(desk, /const base = checked && value !== null \? value \/ 2 : value/);
  assert.match(desk, /\[origin\]: base/);
  assert.match(desk, /value % 2 !== 0/);
  assert.match(footer, /latestCommit\.current\(amount\) === false/);
});

test("already billed and dispatched orders do not retroactively acquire a charge", () => {
  const reconcile = source("src-tauri/src/business_core/rules/reconciliation.rs");
  assert.match(reconcile, /let old_bill=existing_ship\.is_some_and/);
  assert.match(reconcile, /flag\(ship,"transferred"\)/);
  assert.match(reconcile, /field\(ship,"step"\)\.as_i64\(\)\.unwrap_or\(0\)>=2/);
  assert.match(reconcile, /"address":current_address,"checked":false,\s*"manual":false,"legacy":true/);
  assert.match(reconcile, /"address":current_address,"checked":true,"manual":false/);
});
