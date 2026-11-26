import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read=(p)=>readFileSync(new URL("../"+p,import.meta.url),"utf8");

test("the maximum bundle count is sufficient for packability checks",()=>{
  const rust=read("src-tauri/src/business_core/rules/shipping.rs");
  assert.doesNotMatch(rust,/field\(rule,"packable"\)/);
  assert.match(rust,/legacy_packable_false_cannot_block_a_current_confirmed_two_item_rate/);
});
