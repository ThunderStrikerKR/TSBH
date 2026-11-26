import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");

test("native state_save returns the committed snapshot including revision", () => {
  const source=read("src-tauri/src/lib.rs");
  const block=source.slice(source.indexOf("fn state_save("),source.indexOf("fn canonical_color_key("));
  assert.match(block,/-> Result<Value, String>/);
  assert.match(block,/write_snapshot\(&mut \*lock_db\(&db\)\?, state, revision, true\)/);
  assert.doesNotMatch(block,/Ok\(\(\)\)/);
});
