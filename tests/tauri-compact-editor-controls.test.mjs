import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("Rust line amounts use checked multiplication", () => {
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  assert.match(rust, /price\.checked_mul\(qty\)/);
  assert.match(rust, /fn line_amounts\(recipients:&\[Value\]\)/);
});
