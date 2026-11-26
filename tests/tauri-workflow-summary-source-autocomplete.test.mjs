import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("Rust workflow summary VAT labels", () => {
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  for (const label of ["부가세 포함", "부가세 별도", "부가세 포함/별도"]) assert.ok(rust.includes(label));
});
