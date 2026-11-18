import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("explicit managers retain three-choice dialogs, but reception navigation never opens one", () => {
  const dialogs = read("src/dialogs.ts");
  const hub = read("src/HubWorkspace.tsx");
  const model = read("src/app/useAppModel.ts");
  assert.match(dialogs,/export type SaveChangesChoice = "save" \| "discard" \| "cancel"/);
  assert.doesNotMatch(hub,/askSaveChanges|savedChoice|temporaryChoice/);
  assert.doesNotMatch(model,/askSaveChanges|수정한 주문 정보가 저장되지 않았습니다|미완성 발주서 처리/);
  assert.match(model, /requestPageLeave\([\s\S]*?\{ autosave: true \}/);
});

test("confirmed order edits are SQLite working copies, not confirmed ledger writes", () => {
  const hub = read("src/HubWorkspace.tsx");
  const native = read("src-tauri/src/working_receptions.rs");
  assert.match(hub, /const confirmedWorkingEdits = useMemo/);
  assert.match(hub, /JSON\.stringify\(draft\) !== JSON\.stringify\(currentSaved\)/);
  assert.match(hub, /const workingCopies = useMemo/);
  assert.match(hub, /await workingWriter\.current!\.enqueue\(workingCopies\)/);
  const guard = hub.split('leaveGuardRef.current = async (intent = "switch") => {')[1]
    ?.split("  useEffect(() => {\n    props.onRegisterOrderLeaveGuard")[0] ?? "";
  assert.doesNotMatch(guard,/onSave\(|nativeReconcileWorkflow\(|confirm\(|askSaveChanges\(/);
  assert.match(guard,/return \{ proceed: true, handledDraft: true \}/);
  assert.match(native,/CREATE TABLE IF NOT EXISTS reception_confirmed_edits/);
  assert.match(native,/base_payload/);
  assert.match(native,/if baseline!=saved/);
  assert.doesNotMatch(guard,/setDiscardRevision/);
});

test("tab exit, record switch and application close flush edits without browser confirmation", () => {
  const model = read("src/app/useAppModel.ts");
  const app = read("src/App.tsx");
  assert.match(app,/onRegisterOrderLeaveGuard=\{model\.registerOrderLeaveGuard\}/);
  assert.match(model,/confirmOrderLeave\("navigate"\)/);
  assert.match(model,/confirmOrderLeave\("close"\)/);
  assert.match(model,/const flushReceptionEditor = async \(\): Promise<boolean>/);
  assert.match(model,/return window\.bando!\.requestClose\(false\)/);
  assert.doesNotMatch(model,/requestClose\(dirty/);
});

test("workflow confirmation and explicit save still use Rust business rules", () => {
  const footer = read("src/WorkflowFooter.tsx");
  const order = read("src/OrderDesk.tsx");
  const model = read("src/app/useAppModel.ts");
  assert.doesNotMatch(footer,/주문 저장/);
  assert.match(order,/<WorkflowFooter/);
  assert.match(model,/"rules", "save_order"/);
  assert.match(model,/"rules", "finalize_billing_history"/);
  assert.match(model,/action: "persisted_reception_state"/);
});
