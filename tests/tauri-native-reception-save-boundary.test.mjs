import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("Native reducer routes persisted reception state", () => {
  const rules = read("src-tauri/src/business_core/rules/reception_operations.rs");
  assert.match(rules, /"persisted_reception_state"\s*=>/);
});
