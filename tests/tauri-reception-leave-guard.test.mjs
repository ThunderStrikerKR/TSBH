import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");

test("navigation flushes durable receptions instead of repeatedly prompting", () => {
  const model = read("src/app/useAppModel.ts");
  const hub = read("src/HubWorkspace.tsx");
  assert.match(model, /const confirmReceptionDraftLeave = async \(\): Promise<boolean>/);
  assert.doesNotMatch(model, /임시 보관하고 이동|미확정 접수 .*임시 보관 중입니다/);
  assert.doesNotMatch(hub, /미완성 발주서 .*임시 보관 중입니다/);
  assert.match(model, /confirmOrderLeave\("navigate"\)/);
  assert.match(model, /confirmOrderLeave\("close"\)/);
  assert.match(hub, /workingReceptionsLoad\(\)/);
  assert.match(hub, /await workingWriter\.current!\.enqueue\(temporaryReceipts\)/);
  assert.match(hub, /if \(savedOrderDirty\)/);
});
