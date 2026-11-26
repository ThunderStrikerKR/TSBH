import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=path=>readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("Rust owns order payment/status/freight transitions",()=>{
  const reducer=read("src-tauri/src/business_core/rules/order_edit.rs");
  const routing=read("src-tauri/src/business_core/rules/mod.rs");
  assert.match(routing,/"order_edit_rules" => order_edit::execute/);
  assert.match(reducer,/"prepare_order_edit" => prepare_edit\(payload\)/);
  assert.match(reducer,/set\(order,"shippingFee",Value::Null\)/);
  assert.match(reducer,/set\(order,"pendingBillingSnapshot"/);
});

test("selected dealer, waiver and pending freight at save are native results",()=>{
  const rules=read("src-tauri/src/business_core/rules/order_edit.rs");
  assert.match(rules,/"prepare_order_save" => prepare_save\(payload\)/);
  assert.match(rules,/"calculate_order_shipping" => effective_shipping/);
  assert.match(rules,/flag\(&selected,"shippingFeeExcluded"\)/);
  assert.match(read("src-tauri/src/business_core/rules/order_save.rs"),
    /if has_value\(field\(old,"pendingNativeEdit"\)\)/);
  assert.match(read("src-tauri/src/business_core/rules/reply_rules.rs"),
    /if field\(order,"pendingNativeEdit"\)\.is_object\(\)/);
});

test("Rust owns dealer change and repricing",()=>{
  const rules=read("src-tauri/src/business_core/rules/order_edit.rs");
  assert.match(rules,/super::client_change::check/);
  assert.match(rules,/super::reply_rules::billing\(before,state,"거래처 변경",now\)/);
  assert.match(rules,/fn reprice_for_new_dealer\(/);
  assert.match(rules,/super::pricing::resolve_unit_price/);
  assert.match(rules,/let freight=effective_shipping\(&changed,state\)/);
});
