import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import test from "node:test";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("re-notification amount difference is produced in native Rust, not React", () => {
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  const hook = read("src/orders/useNativeWorkflowBoard.ts");
  const ui = read("src/WorkflowBoard.tsx");
  assert.match(rust, /fn notice_delta\(group: &Value, current: &Value\)/);
  assert.match(rust, /"noticeDelta": notice_delta\(group, &group_total\)/);
  assert.match(rust, /checked_sub\(old\)/);
  assert.match(hook, /noticeDelta: Price/);
  assert.match(ui, /nativeGroup\?\.noticeDelta != null/);
  assert.doesNotMatch(ui, /nativeGroup\.totals\.total\s*-\s*g\.notices/);
});

test("runtime guard preserves missing and invalid earlier amounts as unknown", () => {
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  assert.match(rust, /\(Some\(latest\), Some\(old\)\)/);
  assert.match(rust, /_ => Value::Null/);
  assert.match(rust, /fn re_notified_amounts_match_original_typescript_arithmetic/);
});
