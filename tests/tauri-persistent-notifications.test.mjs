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

test("native notification operations are typed and serialized across startup reload and watcher activity", () => {
  const types=read("src/types.ts");
  const bridge=read("src/tauri-bridge.ts");
  const hook=read("src/app/usePersistedNotifications.ts");
  const app=read("src/App.tsx");
  const modal=read("src/FactoryNotifications.tsx");
  const header=read("src/app/AppHeader.tsx");
  for(const api of ["notificationList","notificationAdd","notificationMarkRead","notificationClear"]) {
    assert.match(types, new RegExp(api));
    assert.match(bridge, new RegExp(api+":"));
  }
  assert.match(hook,/chain\.current = chain\.current/);
  assert.match(hook,/enqueue\(\(\) => window\.bando!\.notificationList\(\)\)/);
  assert.match(hook,/window\.bando!\.notificationAdd\(notice\)/);
  assert.match(hook,/window\.bando!\.notificationMarkRead\(id\)/);
  assert.match(hook,/window\.bando!\.notificationClear\(\)/);
  assert.match(app,/usePersistedNotifications\(\)/);
  assert.match(app,/onNotification=\{addNotification\}/);
  assert.match(app,/storageError=\{storageError\}/);
  assert.match(modal,/storageError && <p role="alert"/);
  assert.match(header,/notificationError/);
  assert.doesNotMatch(app,/<AppNotice/);
});

test("real Windows verification includes saved read-state, clear, and business revision preservation",()=>{
  const windows=read("scripts/migration32-real-factory-notifications.mjs");
  const ci=read(".github/workflows/migration-output.yml");
  assert.match(windows,/SIM32_SQLITE_NOTIFICATIONS_SURVIVE_UI_RELOAD/);
  assert.match(windows,/SIM32_SQLITE_NOTIFICATIONS_CLEAR_SURVIVES_RELOAD/);
  assert.match(windows,/await page\.reload/);
  assert.match(windows,/window\.bando\.notificationList\(\)/);
  assert.match(windows,/Notification reload\/read must not change business revision/);
  assert.match(ci,/BANDO_PERSISTENT_NOTICE_AUDIT:/);
});
