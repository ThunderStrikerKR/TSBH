import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");
test("native backup and restore APIs keep pre-restore and validity checks", () => {
  const bridge = read("src/tauri-bridge.ts");
  const adapter = read("src-tauri/src/backup_manager.rs");
  const native = read("crates/bando-data-access/src/backup_service.rs");
  assert.match(adapter, /bando_data_access::backup_service::restore_backup/);
  for (const op of ["backup_create","backup_list","backup_restore","state_save","receipt_archive"]) {
    assert.ok(bridge.includes('"'+op+'"'),op);
  }
  assert.match(native, /fn restore_selected\(/);
  assert.match(native, /copy_entire_db\(&backup, conn\)\?/);
  assert.match(native, /"pre-restore"/);
  assert.match(native, /validate_database\(conn\)/);
  assert.match(native, /복원을 취소하고 직전 상태로 되돌렸습니다/);
});
