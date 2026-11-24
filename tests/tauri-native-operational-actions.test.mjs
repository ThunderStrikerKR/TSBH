import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const source = (path) =>
  readFileSync(new URL("../" + path, import.meta.url), "utf8");
const adapter = source("src/orders/native-rules.ts");
const queue = source("src/WorkflowQueue.tsx");
const correction = source("src/CorrectionPanel.tsx");
const retail = source("src/orders/RetailOrderDialog.tsx");
const rustRules = source("src-tauri/src/business_core/rules/mod.rs");
const nativeCorrection = source("src-tauri/src/business_core/rules/correction_rules.rs");
const nativeRetail = source("src-tauri/src/business_core/rules/retail_operations.rs");

// These checks assert UI-to-IPC wiring, not runtime GUI behavior. All native
// business decisions are independently compared with real TS originals by
// tests/rust-migration/rules/parity/compare.mts (678-case differential suite).
test("factory delivery confirmation and re-export call Rust before SQLite save", () => {
  assert.match(queue, /await onSave\(await nativeApplyCorrectionAction\(state, "mark_factory_delivered", \{ shipmentId: q!\.shipmentId \}\)\)/);
  assert.match(queue, /await onSave\(await nativeApplyCorrectionAction\(state, "reopen_unsent_export", \{ shipmentId: q!\.shipmentId \}\)\)/);
  assert.doesNotMatch(queue, /await onSave\((?:markFactoryDelivered|reopenUnsentExport)\(/);
  assert.match(queue, /if \(!\(await confirmAction\("이 발송 파일을 실제 공장에 전달했나요\?/);
  assert.match(queue, /if \(!\(await confirmAction\("아직 공장에 전달하지 않은 출력본을 폐기하고 다시 작성할까요\?/);
});

test("post-export correction register, copy and staff approval call Rust", () => {
  for (const action of ["request_correction", "copied_correction", "confirm_correction"]) {
    assert.ok(correction.includes('nativeApplyCorrectionAction(state, "' + action + '"'), action);
    assert.ok(nativeCorrection.includes('"' + action + '"=>'), action + " Rust match arm");
  }
  assert.doesNotMatch(correction, /(?:requestCorrection|copiedCorrection|confirmCorrection)\(state,/);
  assert.match(correction, /await window\.bando!\.copy\(native\.text\)/);
  assert.match(correction, /"output", "render_correction_summaries"/);
  assert.doesNotMatch(correction, /correctionMessage\(state,/);
  assert.match(correction, /await onSave\(await nativeApplyCorrectionAction\(state, "copied_correction"/);
});

test("native correction bridge injects audit IDs and validates workflow before save", () => {
  assert.match(adapter, /export async function nativeApplyCorrectionAction\(/);
  assert.match(adapter, /"rules", "apply_correction_action"/);
  assert.match(adapter, /ids: \[crypto\.randomUUID\(\), crypto\.randomUUID\(\), crypto\.randomUUID\(\)\]/);
  assert.match(adapter, /"rules", "validate_workflow", \{ state: updated, before: state \}/);
  assert.match(rustRules, /"apply_correction_action" => correction_rules::apply\(&payload\)/);
  for (const action of ["mark_factory_delivered", "reopen_unsent_export"]) {
    assert.ok(nativeCorrection.includes('"' + action + '"=>'), action + " Rust match arm");
  }
});

test("direct-entry retail confirm goes through the native reducer and preserves error guard", () => {
  assert.match(retail, /await onSave\(await nativeConfirmRetail\(state, order\)\)/);
  assert.doesNotMatch(retail, /await onSave\(confirmRetail\(/);
  assert.match(retail, /catch \(e\) \{\s*setError\(String\(e\)\)/);
  assert.match(adapter, /export async function nativeConfirmRetail\(/);
  assert.match(adapter, /"rules", "apply_retail_action"/);
  for (const key of ["receiptEventId", "processEventId", "reconcileEventIds", "processReconcileEventIds"]) {
    assert.ok(adapter.includes(key + ":"), key + " is supplied to native Rust");
  }
  assert.match(rustRules, /"apply_retail_action" => retail_operations::apply\(&payload\)/);
  assert.match(nativeRetail, /"confirm_retail"=>/);
});

test("customer-approved product resolution uses native Rust, not TS fallback", () => {
  const resolver = source("src/IssueResolver.tsx");
  const desk = source("src/OrderDesk.tsx");
  const nativeProduct = source("src-tauri/src/business_core/rules/product_confirmation.rs");
  assert.match(resolver, /const result = await nativeResolveProduct\(/);
  assert.doesNotMatch(resolver, /resolveProduct\(\s*state,/);
  assert.match(desk, /const result = await nativeConfirmProposedProduct\(state, draft, r\.id, i\.id\)/);
  assert.doesNotMatch(desk, /confirmProposedProduct\(state,/);
  assert.match(adapter, /"rules", "apply_product_action"/);
  assert.match(rustRules, /"apply_product_action" => product_confirmation::apply\(&payload\)/);
  assert.match(nativeProduct, /"resolve_product"=>resolve\(payload\)/);
  assert.match(nativeProduct, /"confirm_proposed_product"=>confirm\(payload\)/);
});

test("working raw correction remains explicitly blocked until its native egui editor is ported", () => {
  const nativeRaw = source("src-tauri/src/business_core/rules/raw_corrections.rs");
  const ledger = JSON.parse(source("docs/rust-migration/verification-ledger.json"));
  const feature = ledger.features.find((item) => item.id === "F12");
  assert.match(nativeRaw, /correct_working_raw/);
  assert.equal(feature?.overall, "OPEN");
  assert.equal(feature?.react?.entrypointVerified, false);
  assert.equal(feature?.gate?.windows?.status, "NOT_VERIFIED");
  assert.ok(feature?.discrepancies?.some((item) =>
    item.status === "OPEN" && /egui.*not yet been ported/.test(item.description)
  ));
});

test("unconfirmed reception removal uses compiled Rust before atomic SQLite save", () => {
  const app = source("src/app/useAppModel.ts");
  const nativeReception = source("src-tauri/src/business_core/rules/reception_operations.rs");
  assert.match(app, /"rules", "apply_reception_action"/);
  assert.match(app, /action: "persisted_reception_state", before: state!, state: billed/);
  assert.match(app, /if \(sameValue\(persistent, state\)\)/);
  assert.match(app, /const committed = await window\.bando!\.save\(persistent\)/);
  assert.doesNotMatch(app, /persistedReceptionState\(state!, next\)/);
  assert.match(nativeReception, /"persisted_reception_state"=>/);
});

test("factory correction clipboard and display use fresh Rust projection without TypeScript fallback", () => {
  const panel = source("src/CorrectionPanel.tsx");
  const native = source("src-tauri/src/business_core/output/corrections.rs");
  const dispatcher = source("src-tauri/src/business_core/output/mod.rs");
  assert.match(panel, /"output", "render_correction_summaries"/);
  assert.match(panel, /correctionsView\?\.source === state/);
  assert.match(panel, /await window\.bando!\.copy\(native\.text\)/);
  assert.doesNotMatch(panel, /(?:correctionMessage|correctionStatus)\(/);
  assert.match(native, /fn message\(/);
  assert.match(dispatcher, /"render_correction_summaries" => corrections::summaries/);
});


test("Kakao title-to-client decisions are Rust-owned in both UI entrypoints", () => {
  const kakao = source("src/KakaoClients.tsx");
  const reception = source("src/SourceDocument.tsx");
  const adapter = source("src/native-title-clients.ts");
  const native = source("src-tauri/src/business_core/catalog_clients/title_clients.rs");
  assert.match(kakao, /await nativeRememberTitle\(/);
  assert.doesNotMatch(kakao, /rememberTitle\(\s*state/);
  assert.match(reception, /nativeAutoReceptionClient\(/);
  assert.match(reception, /await nativeAutoReceptionClient\(/);
  assert.doesNotMatch(reception, /rememberedTitleClient\(state,/);
  assert.match(adapter, /"catalog_clients", "remember_title_client"/);
  assert.match(adapter, /"catalog_clients", "auto_match_reception_client"/);
  assert.match(native, /"remember_title_client"=>remember\(payload\)/);
  assert.match(reception, /action: "change_order_client"/);
});


test("order edit history is frozen natively at commit without delaying controlled inputs", () => {
  const app = source("src/app/useAppModel.ts");
  const desk = source("src/orders/useOrderDesk.tsx");
  const rust = source("src-tauri/src/business_core/rules/reply_rules.rs");
  const dispatcher = source("src-tauri/src/business_core/rules/mod.rs");
  assert.match(desk, /next\.pendingNativeEdit = firstIntent/);
  assert.match(desk, /at: new Date\(\)\.toISOString\(\), reason, recalculateShipping/);
  const nativeEdit = source("src-tauri/src/business_core/rules/order_edit.rs");
  assert.match(nativeEdit, /set\(order,"pendingBillingSnapshot"/);
  assert.match(nativeEdit, /"prepare_order_edit" => prepare_edit\(payload\)/);
  assert.doesNotMatch(desk, /billingSnapshot\(previous, state, reason\)/);
  assert.match(app, /"rules", "finalize_billing_history", \{ before: state!, state: next \}/);
  assert.match(app, /state: billed/);
  assert.match(rust, /pub\(super\) fn finalize_billing_history/);
  assert.match(rust, /object\.remove\("pendingBillingSnapshot"\)/);
  assert.match(rust, /let snapshot=billing\(previous,before,reason,at\)/);
  assert.match(dispatcher, /"finalize_billing_history" => reply_rules::finalize_billing_history/);
});


test("manual classification final approved order is produced by native Rust, never TS applyManual", () => {
  const manual = source("src/orders/ManualClassification.tsx");
  const rust = source("src-tauri/src/business_core/rules/manual_classification.rs");
  const dispatcher = source("src-tauri/src/business_core/rules/mod.rs");
  assert.match(manual, /"rules", "apply_manual_classification"/);
  assert.match(manual, /assignmentOrder: Object\.keys\(submittedModel\.assignments\)/);
  assert.match(manual, /onApply\(prepared\)/);
  assert.doesNotMatch(manual, /onApply\(applyManual\(/);
  assert.match(rust, /fn synchronize\(order:&mut Value\)/);
  assert.match(dispatcher, /"apply_manual_classification" => match str_field/);
  assert.match(dispatcher, /manual_draft::execute/);
  assert.match(dispatcher, /_ => manual_classification::apply/);
});


test("factory history owns native review and correction; critical alert modal never duplicates it", () => {
  const notice = source("src/FactoryNotifications.tsx");
  const history = source("src/FactoryReplyHistory.tsx");
  const adapter = source("src/native-factory-conflict-review.ts");
  const rust = source("src-tauri/src/business_core/rules/factory_reply.rs");
  assert.match(history, /"rules", "factory_reply_review_statuses"/);
  assert.match(history, /nativeReview\?\.source === reviewSource/);
  assert.match(history, /nativeFactoryConflictConfirmation\(/);
  assert.match(history, /approveFactoryConflict\(/);
  assert.doesNotMatch(history, /\breplyNeedsReview\(/);
  assert.match(notice, /const critical = notices\.filter\(isActionRequiredNotification\)/);
  assert.doesNotMatch(notice, /"factory_reply_review_statuses"|nativeFactoryConflictConfirmation|approveFactoryConflict/);
  assert.match(adapter, /"rules", "factory_conflict_confirmation"/);
  assert.match(rust, /fn resolved_row\(/);
  assert.match(rust, /fn conflict_confirmation\(/);
});

test("freight managers read tariff rule and current status from native Rust", () => {
  const group = source("src/ShippingManager.tsx");
  const product = source("src/ProductFreightPanel.tsx");
  const adapter = source("src/useNativeShippingRead.ts");
  const rust = source("src-tauri/src/business_core/rules/shipping_read.rs");
  const rules = source("src-tauri/src/business_core/rules/shipping.rs");
  assert.match(group, /useNativeShippingRead\(effectiveState\)/);
  assert.match(product, /action: "product_freight_view", state/);
  assert.match(product, /action: "reset_product_freight_rule", state/);
  for (const ui of [group, product]) {
    assert.doesNotMatch(ui, /\bcombinationCurrent\(/);
    assert.doesNotMatch(ui, /\beffectiveShippingRule\(/);
    assert.doesNotMatch(ui, /\beditableShippingRule\(/);
  }
  assert.match(adapter, /"rules", "shipping_read"/);
  assert.match(adapter, /snapshot\?\.source === source/);
  assert.match(adapter, /\[state\.products, state\.shipping\]/);
  assert.match(rust, /fn current_combo\(/);
  assert.match(rust, /fn edit_rule\(/);
  assert.match(rules, /"product_freight_view" =>/);
});

test("OCR, pasted and cleared reception drafts use Rust, never the old TS reducer", () => {
  const component = source("src/SourceDocument.tsx");
  const native = source("src-tauri/src/business_core/rules/reception_operations.rs");
  assert.match(component, /action: "replace_reception", order: source/);
  assert.match(component, /const next = await nativeReplaceReception\(order, raw\)/);
  assert.match(component, /await nativeReplaceReception\(order, ""\)/);
  assert.match(component, /latestReception\.current\.order\.id !== order\.id/);
  assert.match(component, /latestReception\.current\.order\.raw !== order\.raw/);
  assert.match(component, /latestReception\.current\.text !== text/);
  assert.doesNotMatch(component, /(?:^|[^A-Za-z])replaceReception\(/m);
  assert.match(native, /"replace_reception"=>/);
});


test("global badge counts unread critical alerts; pending factory reviews stay in history", () => {
  const app = source("src/App.tsx");
  const header = source("src/app/AppHeader.tsx");
  const history = source("src/FactoryReplyHistory.tsx");
  const native = source("src-tauri/src/business_core/rules/factory_reply.rs");
  assert.match(app, /notificationCount=\{inbox\.filter\(\(entry\) => !entry\.read\)\.length\}/);
  assert.match(app, /priority: "critical"/);
  assert.doesNotMatch(app, /"factory_reply_review_statuses"|replyNeedsReview\(/);
  assert.match(history, /"rules", "factory_reply_review_statuses"/);
  assert.match(header, /notificationCount/);
  assert.match(native, /fn review_list\(/);
});
