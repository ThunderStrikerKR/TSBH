import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("notification modal contains only dismissible urgent alerts, not factory history", () => {
  const modal = source("src/FactoryNotifications.tsx");
  assert.match(modal, /notices\.filter\(isActionRequiredNotification\)/);
  assert.match(modal, /현재 표시할 알림이 없습니다/);
  assert.match(modal, /onDelete\(entry\.id\)/);
  assert.match(modal, /onClear\(\)/);
  assert.doesNotMatch(modal, /state\.factoryReplies|factory_reply_review_statuses|공장 회신 이력|nativeFactoryConflictConfirmation/);
  const history = source("src/FactoryReplyHistory.tsx");
  assert.match(history, /공장 회신 이력/);
  assert.match(history, /approveFactoryConflict/);
});

test("routine, historical and factory-watcher messages never create active alerts", () => {
  const app = source("src/App.tsx");
  const watcher = source("src/FactoryReplyWatcher.tsx");
  const noticePolicy = source("src/app/notification-center.ts");
  const inbox = source("src/app/usePersistedNotifications.ts");
  assert.match(noticePolicy, /priority\?: "critical"/);
  assert.match(noticePolicy, /\{ priority: "critical" \}/);
  assert.match(noticePolicy, /return notice\.priority === "critical"/);
  assert.match(inbox, /setInbox\(notices\.filter\(isActionRequiredNotification\)\)/);
  assert.match(inbox, /setHiddenCount\(notices\.filter\(\(notice\) => !isActionRequiredNotification\(notice\)\)\.length\)/);
  assert.match(app, /if \(!state \|\| !draft \|\| !notice\?\.critical/);
  assert.match(app, /priority: "critical"/);
  assert.match(app, /notificationCount=\{inbox\.filter\(\(entry\) => !entry\.read\)\.length\}/);
  assert.doesNotMatch(app, /nativeReviewCount|factory_reply_review_statuses/);
  assert.doesNotMatch(watcher, /onNotification|addNotification|NewHubNotification/);
  assert.match(watcher, /await processWatchedFactoryReply\(api, file\)/);
  assert.match(watcher, /await latest\.current\.onRefresh\(\)/);
});

test("one-item dismissal is persisted separately from business data", () => {
  const hook = source("src/app/usePersistedNotifications.ts");
  const modal = source("src/FactoryNotifications.tsx");
  const bridge = source("src/tauri-bridge.ts");
  const api = source("src/types.ts");
  const db = source("src-tauri/src/notification_store.rs");
  const handler = source("src-tauri/src/lib.rs");
  assert.match(modal, /onDelete: \(id: string\) => void/);
  assert.match(hook, /window\.bando!\.notificationDelete\(id\)/);
  assert.match(bridge, /notificationDelete: \(id\) => invoke\("hub_notifications_delete", \{ id \}\)/);
  assert.match(api, /notificationDelete: \(id: string\)/);
  assert.match(db, /DELETE FROM hub_notifications WHERE id=\?1/);
  assert.match(db, /fn hub_notifications_delete/);
  assert.match(handler, /notification_store::hub_notifications_delete/);
  assert.doesNotMatch(modal, /state\.factoryReplies/);
  assert.match(db, /fn deleting_one_alert_does_not_remove_other_alerts_or_business_records/);
});
