import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
test("native order desks mount only after their portal DOM targets exist", () => {
  const src=readFileSync(new URL("../src/HubWorkspace.tsx",import.meta.url),"utf8");
  assert.match(src, /\{source && messages && \(\s*<MemoOrderDesk/);
  assert.match(src, /\{details && \(\s*<MemoOrderDesk/);
  assert.match(src, /portalTargets=\{receptionTargets\}/);
  assert.match(src, /portalTargets=\{detailTargets\}/);
  assert.match(src, /ref=\{setSource\}/);
  assert.match(src, /ref=\{setMessages\}/);
  assert.match(src, /ref=\{setDetails\}/);
});
