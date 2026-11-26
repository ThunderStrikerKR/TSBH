import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

function source(path) {
  return readFileSync(new URL("../" + path, import.meta.url), "utf8");
}
test("real direct-entry retail draft and preflight decisions are native-only", () => {
  const rust = source("src-tauri/src/business_core/rules/retail_operations.rs");
  for (const action of ["new_retail_order", "retail_preflight", "create_retail_item"]) {
    assert.match(rust, new RegExp('"' + action + '"'), "Rust must implement "+action);
  }
  assert.match(rust, /"confirm_retail"=>/);
  assert.match(rust, /super::validation::retail_issues\(draft,state\)/);
  assert.match(rust, /super::validation::company_sender_missing\(state\)/);
  assert.match(rust, /super::catalog_item::make\(&input\)/);
});
