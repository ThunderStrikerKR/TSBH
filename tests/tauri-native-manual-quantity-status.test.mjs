import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("manual grid and apply-button quantity approval are native Rust projections", () => {
  const rust = source("src-tauri/src/business_core/rules/manual_classification.rs");
  const dispatcher = source("src-tauri/src/business_core/rules/mod.rs");
  assert.match(rust, /if action == "manual_quantity_status"/);
  assert.match(rust, /"pending":pending_quantity\(model\)/);
  assert.match(rust, /"valid":valid_quantity_item\(item\)/);
  assert.match(dispatcher, /"apply_manual_classification"/);
});
