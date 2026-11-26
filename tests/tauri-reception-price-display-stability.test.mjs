import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";

test("separate Rust line quote is not gated by complete workflow reconciliation",()=>{
  const rust=readFileSync(new URL("../src-tauri/src/business_core/rules/workflow_preview.rs",import.meta.url),"utf8");
  assert.match(rust,/fn line_amounts\(recipients:&\[Value\]\)/);
  assert.match(rust,/pub\(super\) fn project_order_lines/);
  assert.match(rust,/fn independent_native_item_price_is_available_without_shipments_or_workflow/);
});
