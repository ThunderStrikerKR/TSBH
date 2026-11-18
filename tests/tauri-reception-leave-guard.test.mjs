import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");

test("normal reception navigation waits for all SQLite work and never asks for save/discard", () => {
  const model = read("src/app/useAppModel.ts");
  const hub = read("src/HubWorkspace.tsx");
  assert.match(model,/const confirmReceptionDraftLeave = async \(\): Promise<boolean>/);
  assert.doesNotMatch(model,/임시 보관하고 이동|미확정 접수 .*임시 보관 중입니다/);
  assert.doesNotMatch(hub,/askSaveChanges|미완성 발주서 .*임시 보관 중입니다/);
  assert.match(model,/requestPageLeave\([\s\S]*?\{ autosave: true \}/);
  assert.match(model,/confirmOrderLeave\("navigate"\)/);
  assert.match(model,/confirmOrderLeave\("close"\)/);
  assert.match(hub,/workingReceptionsLoad\(\)/);
  assert.match(hub,/await workingWriter\.current!\.enqueue\(workingCopies\)/);
  assert.match(hub,/confirmedWorkingEdits/);
});
