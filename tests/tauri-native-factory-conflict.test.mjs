import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../"+p, import.meta.url),"utf8");

test("native factory conflict IPC is wired through both review surfaces", () => {
  const rust=read("src-tauri/src/factory_conflict.rs");
  const lib=read("src-tauri/src/lib.rs");
  assert.match(lib,/factory_conflict::approve_factory_conflict/);
  assert.match(rust,/fn correct_state\(/);
  assert.match(rust,/verified_original\(/);
  assert.match(rust,/backup_before_correction\(/);
  assert.match(rust,/write_snapshot\(/);
  assert.match(rust,/expected_revision/);
  assert.match(rust,/Sha256::digest/);
});
