import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL("../"+path, import.meta.url), "utf8");

test("notification schema uses dedicated SQLite table and never edits business snapshot", () => {
  const native = read("src-tauri/src/notification_store.rs");
  const lib = read("src-tauri/src/lib.rs");
  assert.match(native, /CREATE TABLE IF NOT EXISTS hub_notifications/);
  assert.match(native, /SELECT payload, read FROM hub_notifications/);
  assert.match(native, /fn insert\(conn: &mut Connection/);
  assert.match(native, /fn mark_read\(conn: &Connection/);
  assert.match(native, /fn clear\(conn: &Connection/);
  assert.doesNotMatch(native, /write_snapshot|UPDATE app_state|state_save/);
  assert.match(native, /tx\.commit\(\)/);
  assert.match(lib, /notification_store::initialize\(&conn\)/);
  for (const name of ["list","add","mark_read","clear"])
    assert.match(lib, new RegExp("notification_store::hub_notifications_"+name));
});
