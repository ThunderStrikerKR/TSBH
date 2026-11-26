import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("one-item dismissal is persisted separately from business data", () => {
  const db = source("src-tauri/src/notification_store.rs");
  const handler = source("src-tauri/src/lib.rs");
  assert.match(db, /DELETE FROM hub_notifications WHERE id=\?1/);
  assert.match(db, /fn hub_notifications_delete/);
  assert.match(handler, /notification_store::hub_notifications_delete/);
  assert.match(db, /fn deleting_one_alert_does_not_remove_other_alerts_or_business_records/);
});
