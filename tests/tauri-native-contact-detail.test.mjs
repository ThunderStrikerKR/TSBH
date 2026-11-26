import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const get=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");

test("Rust contact validation and explanations",()=>{
  const rust=get("src-tauri/src/business_core/rules/issues.rs");
  const router=get("src-tauri/src/business_core/rules/mod.rs");
  assert.match(router,/"delivery_requirements_with_messages" =>/);
  assert.match(rust,/pub\(super\) fn delivery_requirements_with_messages/);
  assert.match(rust,/json!\(contact_problem\(value,is_mobile\)\)/);
  assert.match(rust,/fn invalid_phone_explanation_is_rust_generated_without_changing_base_projection/);
});
