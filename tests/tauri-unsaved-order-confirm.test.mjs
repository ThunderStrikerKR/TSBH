import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("confirmed order edits are SQLite working copies, not confirmed ledger writes", () => {
  const native = read("src-tauri/src/working_receptions.rs");
  assert.match(native,/CREATE TABLE IF NOT EXISTS reception_confirmed_edits/);
  assert.match(native,/base_payload/);
  assert.match(native,/if baseline!=saved/);
});
