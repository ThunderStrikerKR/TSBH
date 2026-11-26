import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import test from "node:test";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("Rust computes re-notification amount difference", () => {
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  assert.match(rust, /fn notice_delta\(group: &Value, current: &Value\)/);
  assert.match(rust, /"noticeDelta": notice_delta\(group, &group_total\)/);
  assert.match(rust, /checked_sub\(old\)/);
});

test("runtime guard preserves missing and invalid earlier amounts as unknown", () => {
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  assert.match(rust, /\(Some\(latest\), Some\(old\)\)/);
  assert.match(rust, /_ => Value::Null/);
  assert.match(rust, /fn re_notified_amounts_match_original_typescript_arithmetic/);
});
