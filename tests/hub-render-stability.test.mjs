import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");
test("stable event callback reads the latest handler, never a stale order closure", () => {
  const hook = read("src/shared/useEventCallback.ts");
  assert.match(hook, /latest\.current = callback/);
  assert.match(hook, /return useCallback\(\(\.\.\.args: Args\) => latest\.current\(\.\.\.args\), \[\]\)/);
});
test("memoized reception and detail panes receive stable live handlers and exact preview", () => {
  const hub=read("src/HubWorkspace.tsx");
  const desk=read("src/OrderDesk.tsx");
  assert.match(desk,/export const MemoOrderDesk = memo\(OrderDesk\)/);
  assert.match(hub,/sharedCurrentDraftFlow = useMemo\(/);
  assert.match(hub,/onChange=\{changeReceptionLive\}/);
  assert.match(hub,/onChange=\{changeDetailLive\}/);
  assert.match(hub,/onWorkflowSave=\{saveWorkflowLive\}/);
  assert.match(hub,/portalTargets=\{receptionTargets\}/);
  assert.match(hub,/portalTargets=\{detailTargets\}/);
  assert.match(hub,/onSelectWorkflow=\{setSelectedLive\}/);
  assert.match(hub,/onSave=\{saveDetailLive\}/);
});
test("workflow board and message pane only skip rendering for shallow-equal props", () => {
  const hub=read("src/HubWorkspace.tsx");
  assert.match(read("src/WorkflowBoard.tsx"),/export const MemoWorkflowBoard = memo\(WorkflowBoard\)/);
  assert.match(read("src/MessageWorkspace.tsx"),/export const MemoMessageWorkspace = memo\(MessageWorkspace\)/);
  assert.match(hub,/onSelectPending=\{selectPendingLive\}/);
  assert.match(hub,/onSelect=\{selectMessageLive\}/);
  assert.match(hub,/pendingWithoutClient = useMemo\(/);
});
