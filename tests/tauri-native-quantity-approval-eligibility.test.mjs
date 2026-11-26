import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const get=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("quantity provisional approval eligibility is native, not re-evaluated in React",()=>{
  const router=get("src-tauri/src/business_core/rules/mod.rs");
  const rust=get("src-tauri/src/business_core/rules/issues.rs");
  assert.match(router,/"order_issues_with_actions" =>/);
  assert.match(rust,/fn can_approve_missing_quantity\(item:&Value\)->bool/);
  assert.match(rust,/!can_approve_missing_quantity\(&order\["recipients"\]\[ri\]\["items"\]\[ii\]\)/);
  assert.match(rust,/fn staff_button_follows_exact_same_rust_guard_as_actual_action\(/);
});
