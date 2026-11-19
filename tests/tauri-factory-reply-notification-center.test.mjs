import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("factory watcher refreshes saved history and never generates routine global alerts", () => {
  const watcher=read("src/FactoryReplyWatcher.tsx");
  const auto=read("src/factory-reply-auto.ts");
  assert.match(watcher,/await processWatchedFactoryReply\(api, file\)/);
  assert.match(watcher,/if \(result\.invalid\)/);
  assert.match(watcher,/rejected\.current\.add\(file\.id\)/);
  assert.match(watcher,/if \(result\.duplicate\) continue/);
  assert.match(watcher,/await latest\.current\.onRefresh\(\)/);
  assert.doesNotMatch(watcher,/onNotification|notificationAdd\(/);
  assert.match(auto,/preview\.rows\.filter\(\(r\) => !r\.applied && !!r\.issue\)/);
});

test("global modal contains critical-only persisted alerts; ordinary factory replies remain history", () => {
  const app=read("src/App.tsx");
  const header=read("src/app/AppHeader.tsx");
  const modal=read("src/FactoryNotifications.tsx");
  const css=read("src/styles/application.css");
  assert.match(app,/usePersistedNotifications\(Boolean\(state && draft\)\)/);
  assert.match(app,/if \(!state \|\| !draft \|\| !notice\?\.critical/);
  assert.match(app,/priority: "critical"/);
  assert.match(app,/storageError=\{storageError\}/);
  assert.match(app,/onRead=\{\(id\) => \{ void markRead\(id\); \}\}/);
  assert.match(app,/onClear=\{\(\) => \{ void clear\(\); \}\}/);
  assert.doesNotMatch(app,/onNotification=\{addNotification\}|<AppNotice/);
  assert.match(header,/<span>알림<\/span>/);
  assert.doesNotMatch(header,/회신 알림/);
  assert.match(modal,/<Modal title="알림"/);
  assert.match(modal,/const critical = notices\.filter\(isActionRequiredNotification\)/);
  assert.match(modal,/critical\.map\(\(entry\) =>/);
  assert.match(modal,/onRead\(entry\.id\)/);
  assert.match(modal,/selected\.issues\?\.length/);
  assert.match(css,/\.notification-center-layout/);
});

test("native validation holds only untrustworthy rows and still enforces safe matching", () => {
  const rust=read("src-tauri/src/business_core/rules/factory_reply.rs");
  const native=read("src/native-factory-reply.ts");
  assert.doesNotMatch(rust,/if field\(source,"trackingNumeric"\)==true\{\s*return Err/);
  assert.doesNotMatch(rust,/return Err\(format!\("\{n\}행: 표준 양식의 J열/);
  assert.match(rust,/numeric_risk/);
  assert.match(rust,/source_issue/);
  assert.match(rust,/if !tracking\.is_empty\(\) && s\(row,"issue"\)\.is_empty\(\)/);
  assert.match(native,/problems\.push/);
  assert.match(rust,/trackingNumberFormat/);
  assert.match(rust,/zero_padded/);
  assert.doesNotMatch(native,/const numericRisk|trackingRisk\s*\?/);
  assert.match(native,/trackingNumberFormat: row\.getCell\(17\)\.numFmt/);
  assert.match(rust,/fn row_matches/);
  assert.match(rust,/ids\.len\(\)!=1/);
  assert.match(rust,/기존 송장 \/ 실제 운임과 충돌합니다/);
});
