import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

function source(path) {
  return readFileSync(new URL("../" + path, import.meta.url), "utf8");
}
test("real direct-entry retail draft and preflight decisions are native-only", () => {
  const dialog = source("src/orders/RetailOrderDialog.tsx");
  const sourceDocument = source("src/SourceDocument.tsx");
  const rust = source("src-tauri/src/business_core/rules/retail_operations.rs");
  assert.doesNotMatch(dialog, /import\s*{[^}]*\b(?:newRetailOrder|deliveryRequirements|companySenderMissing)\b/);
  assert.doesNotMatch(sourceDocument, /\bnewRetailOrder\s*\(/);
  for (const action of ["new_retail_order", "retail_preflight", "create_retail_item", "confirm_retail"]) {
    assert.match(dialog, new RegExp('"' + action + '"'), "real UI must call "+action);
    assert.match(rust, new RegExp('"' + action + '"'), "Rust must implement "+action);
  }
  assert.match(dialog, /preflight\?\.fingerprint === fingerprint/);
  assert.match(rust, /super::validation::retail_issues\(draft,state\)/);
  assert.match(rust, /super::validation::company_sender_missing\(state\)/);
  assert.match(rust, /super::catalog_item::make\(&input\)/);
});
test("legacy TypeScript rules remain available only as the parity oracle", () => {
  const oracle = source("tests/rust-migration/rules/parity/compare.mts");
  for (const name of [
    "retail native preflight: ",
    "retail native catalog quote:",
    "retail: initial two-UUID draft",
    "retail: valid factory paid externally",
  ]) assert.ok(oracle.includes(name),name);
});
