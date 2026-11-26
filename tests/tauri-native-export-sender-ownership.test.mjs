import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("Excel sender-settings guidance uses the Rust export authority", () => {
  const native = source("src-tauri/src/business_core/output/mod.rs");
  const exportRules = source("src-tauri/src/business_core/output/export.rs");
  assert.match(native, /"review_export_sender"\s*=>\s*export::sender_review/);
  assert.match(exportRules, /fn sender_complete\(sender: &Value\) -> bool/);
  assert.match(exportRules, /if !sender_complete\(sender\)/);
  assert.match(exportRules, /pub\(super\) fn sender_review/);
});
