import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const source = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");
const dialogs = source("src/dialogs.ts");
const model = source("src/app/useAppModel.ts");
const hub = source("src/HubWorkspace.tsx");
const footer = source("src/WorkflowFooter.tsx");
const order = source("src/OrderDesk.tsx");
const app = source("src/App.tsx");

test("manual order modifications prompt for save, don't save or cancel", () => {
  assert.match(dialogs, /export type SaveChangesChoice = "save" \| "discard" \| "cancel"/);
  for (const text of ["저장", "저장 안 함", "취소"]) assert.ok(dialogs.includes(text));
  assert.match(dialogs, /dialog\.oncancel = \(event\) => \{[\s\S]*finish\("cancel"\)/);
  assert.match(dialogs, /if \(pending\) return Promise\.resolve\("cancel"\)/);
});
test("leaving selected order with edits saves or discards before changing selection", () => {
  assert.match(hub, /const persisted = state\.orders\.find/);
  assert.match(hub, /JSON\.stringify\(pending\) === JSON\.stringify\(persisted\)/);
  assert.match(hub, /savedChoice === "save"/);
  assert.match(hub, /temporaryChoice === "discard"/);
  assert.match(hub, /await onSave\(await nativeReconcileWorkflow\(state, pending\)\)/);
  // The original Rust reconciliation/save remains mandatory for confirmed edits.
  assert.match(hub, /if \(savedChoice === "save"\) \{\s*try \{\s*await onSave\(await nativeReconcileWorkflow\(state, pending!\)\);/);
  assert.match(hub, /catch \(error\) \{[\s\S]*?return \{ proceed: false, handledDraft: false \};/);
  // Explicit discard restores the prior persisted order, never the dirty copy.
  assert.match(hub, /props\.onChange\(structuredClone\(persisted!\)\)/);
  assert.match(hub, /delete next\[orderId\]/);
  assert.match(hub, /const leave = await leaveGuardRef\.current\(\)/);
  assert.match(hub, /if \(!leave\.proceed\) return/);
  assert.match(hub, /applySelection\(next\)/);
  assert.match(hub, /setDiscardRevision/);
});
test("top-level navigation and window close respect unsaved-order decision", () => {
  assert.match(app, /onRegisterOrderLeaveGuard=\{model\.registerOrderLeaveGuard\}/);
  assert.match(model, /const leave = await confirmOrderLeave\(\)/);
  assert.match(model, /if \(!leave\.proceed\) return/);
  assert.match(model, /return window\.bando!\.requestClose\(dirty && !leave\.handledDraft\)/);
});
test("unconditional order save is removed, workflow one-touch progression remains independent", () => {
  assert.doesNotMatch(footer, /주문 저장/);
  assert.doesNotMatch(order, /onSave=\{\(\) => void run\(\(\) => onWorkflowSave\(flowState\)\)\}/);
  assert.match(order, /<WorkflowFooter/);
  assert.match(app, /const workspaceWorkflowSave = useEventCallback\(model\.onWorkflowSave\)/);
  assert.match(app, /onWorkflowSave=\{workspaceWorkflowSave\}/);
});

test("cancelling either leave decision preserves saved edits and unfinished receipts",()=>{
  const guard=source("src/HubWorkspace.tsx").split('  leaveGuardRef.current = async (intent = "switch") => {')[1]
    ?.split("  useEffect(() => {\n    props.onRegisterOrderLeaveGuard")[0] ?? "";
  assert.match(guard,/savedChoice = choice/);
  assert.match(guard,/temporaryChoice = choice/);
  assert.ok(guard.indexOf("temporaryChoice = choice") < guard.indexOf('if (savedChoice === "save")'));
  assert.ok(guard.indexOf("temporaryChoice = choice") < guard.indexOf('if (savedChoice === "discard")'));
  assert.match(guard,/if \(temporaryChoice === "discard"\)/);
});
