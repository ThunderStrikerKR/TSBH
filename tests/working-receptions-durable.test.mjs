import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";
const source = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");
const { WorkingReceptionAutosave } = await tsImport(
  "../src/orders/working-reception-autosave.ts", import.meta.url,
);

test("concurrent typing saves the most recent full working copy, in order", async () => {
  let release;
  const snapshots = [];
  const saver = new WorkingReceptionAutosave(async (orders) => {
    snapshots.push(orders);
    if (snapshots.length === 1) await new Promise((resolve) => { release = resolve; });
  });
  const a = saver.enqueue([{ id:"a", raw:"붙" }]);
  const b = saver.enqueue([{ id:"a", raw:"붙여" }]);
  const c = saver.enqueue([{ id:"a", raw:"붙여넣기" }]);
  release();
  await Promise.all([a,b,c]);
  assert.equal(snapshots.length,2);
  assert.equal(snapshots[1][0].raw,"붙여넣기");
});

test("rejected SQLite write does not silently forget an unfinished receipt", async () => {
  let fail = true;
  const written = [];
  const saver = new WorkingReceptionAutosave(async (orders) => {
    if (fail) { fail=false;throw Error("SQLITE_BUSY"); }
    written.push(orders);
  });
  await assert.rejects(saver.enqueue([{id:"a",raw:"원문"}]),/SQLITE_BUSY/);
  await Promise.resolve();
  await saver.enqueue([{id:"a",raw:"원문 수정"}]);
  assert.equal(written.at(-1)[0].raw,"원문 수정");
});

test("native SQLite working receipts never alter confirmed order ledger or history eligibility", () => {
  const native = source("src-tauri/src/working_receptions.rs");
  const lib = source("src-tauri/src/lib.rs");
  const model = source("src/app/useAppModel.ts");
  const hub = source("src/HubWorkspace.tsx");
  const bridge = source("src/tauri-bridge.ts");
  assert.match(native,/CREATE TABLE IF NOT EXISTS reception_working_orders/);
  assert.match(native,/tx\.commit\(\)\.map_err\(err\)/);
  assert.doesNotMatch(native,/UPDATE app_state|INSERT INTO core_orders/);
  assert.match(lib,/working_receptions::initialize\(&conn\)/);
  assert.match(lib,/working_receptions::working_receptions_load/);
  assert.match(lib,/working_receptions::working_receptions_replace/);
  assert.match(bridge,/workingReceptionsLoad: \(\) => invoke<Order\[\]>\("working_receptions_load"\)/);
  assert.match(hub,/setWorkingStoreReady\(true\)/);
  assert.match(hub,/if \(!workingStoreReady\) return;/);
  assert.match(hub,/await workingWriter\.current!\.enqueue\(temporaryReceipts\)/);
  assert.match(model,/setRestoreEpoch\(\(current\) => current \+ 1\)/);
  assert.match(source("src/App.tsx"),/key=\{model\.restoreEpoch\}/);
  assert.doesNotMatch(model,/pendingDraftCount|미완성 발주서 처리/);
  assert.match(source("src-tauri/src/business_core/rules/workflow.rs"),/set\(order,"confirmedAt",json!\(now\)\)/);
});
