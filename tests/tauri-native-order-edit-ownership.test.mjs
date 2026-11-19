import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=path=>readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("React edits only UI fields; order payment/status/freight transitions belong to Rust",()=>{
  const hook=read("src/orders/useOrderDesk.tsx");
  const reducer=read("src-tauri/src/business_core/rules/order_edit.rs");
  const routing=read("src-tauri/src/business_core/rules/mod.rs");
  const edit=hook.slice(hook.indexOf("  const edit = ("),hook.indexOf("  const recEdit = ("));
  assert.ok(edit.length>400);
  assert.match(edit,/fn\(next\)/);
  assert.match(edit,/next\.pendingNativeEdit = firstIntent/);
  for(const name of ["pricesLocked","notified","paid","postpaidApproved",
    "status","shippingFee","shippingVersion","clientSnapshot"]){
    assert.doesNotMatch(edit,new RegExp("next\\."+name+"\\s*="),name);
  }
  assert.match(hook,/action: "prepare_order_edit", order: sourceDraft, state: sourceState/);
  assert.match(hook,/sameValue\(previous, sourceDraft\) \? native : previous/);
  assert.match(hook,/import \{ sameValue \} from "\.\.\/shared\/draft"/);
  assert.match(hook,/draft\.pendingNativeEdit \|\|/);
  assert.match(routing,/"order_edit_rules" => order_edit::execute/);
  assert.match(reducer,/"prepare_order_edit" => prepare_edit\(payload\)/);
  assert.match(reducer,/set\(order,"shippingFee",Value::Null\)/);
  assert.match(reducer,/set\(order,"pendingBillingSnapshot"/);
});

test("selected dealer, waiver and pending freight at save are native results",()=>{
  const app=read("src/app/useAppModel.ts");
  const rules=read("src-tauri/src/business_core/rules/order_edit.rs");
  const save=app.slice(app.indexOf("  const saveOrder = async"),app.indexOf("  // A new reception cannot be saved"));
  assert.match(save,/action: "prepare_order_save", state: sourceState, order: sourceDraft/);
  assert.match(save,/if \(draft\.pendingNativeEdit\)/);
  assert.doesNotMatch(save,/orderClient\(/);
  assert.doesNotMatch(save,/shippingFeeExcluded\s*\?\s*0/);
  assert.doesNotMatch(save,/"calculate_shipping"/);
  assert.match(rules,/"prepare_order_save" => prepare_save\(payload\)/);
  assert.match(rules,/"calculate_order_shipping" => effective_shipping/);
  assert.match(rules,/flag\(&selected,"shippingFeeExcluded"\)/);
  assert.match(read("src-tauri/src/business_core/rules/order_save.rs"),
    /if has_value\(field\(old,"pendingNativeEdit"\)\)/);
  assert.match(read("src-tauri/src/business_core/rules/reply_rules.rs"),
    /if field\(order,"pendingNativeEdit"\)\.is_object\(\)/);
});

test("dealer change is one native mutation and cannot reapply stale React draft after save",()=>{
  const view=read("src/OrderDesk.tsx");
  const rules=read("src-tauri/src/business_core/rules/order_edit.rs");
  const start=view.indexOf("  const chooseDealer = async (c: Client) => {");
  assert.ok(start>=0,"The native dealer mutation must use a shared guarded handler");
  const change=view.slice(start,view.indexOf("  if (draft.orderType ===",start));
  assert.match(view,/onDealer=\{chooseDealer\}/);
  assert.match(change,/action: "change_order_client"/);
  assert.match(change,/if \(!isCurrentOrderInput\(sourceDraft, sourceState\)\)/);
  assert.match(change,/if \(saved\) \{[\s\S]*?await onWorkflowSave\(await nativeReconcileWorkflow/);
  assert.match(change,/\} else \{\s*onChange\(next\)/);
  assert.match(change,/const persisted = await window\.bando!\.load\(\)/);
  assert.match(change,/onChange\(\(current\) => current === sourceDraft \? committed : current\)/);
  assert.doesNotMatch(change,/nativeBillingSnapshot\(/);
  assert.doesNotMatch(change,/nativeRepriceEditedOrder\(/);
  assert.match(rules,/super::client_change::check/);
  assert.match(rules,/super::reply_rules::billing\(before,state,"거래처 변경",now\)/);
  assert.match(rules,/fn reprice_for_new_dealer\(/);
  assert.match(rules,/super::pricing::resolve_unit_price/);
  assert.match(rules,/let freight=effective_shipping\(&changed,state\)/);
  const adapters=read("src/orders/native-rules.ts");
  assert.doesNotMatch(adapters,/export async function nativeRepriceEditedOrder/);
});

test("all changed business behavior has an actual original TS-vs-Rust differential gate",()=>{
  const tests=read("tests/rust-migration/rules/order-edit-native-parity.test.mjs");
  const ci=read(".github/workflows/migration-output.yml");
  for(const feature of ["prepare_order_edit","resolve_order_client","prepare_order_save",
    "calculate_order_shipping","change_order_client"])assert.ok(tests.includes(feature),feature);
  assert.match(ci,/node --test tests\/rust-migration\/rules\/order-edit-native-parity\.test\.mjs/);
});
