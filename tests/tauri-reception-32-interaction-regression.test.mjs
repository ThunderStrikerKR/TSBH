import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("native preview refresh keeps review layout, VAT control and order board stable", () => {
  const desk = read("src/OrderDesk.tsx");
  const checks = read("src/StaffChecks.tsx");
  const board = read("src/WorkflowBoard.tsx");
  const footer = read("src/WorkflowFooter.tsx");
  assert.doesNotMatch(desk, /주문 금액 및 업무 미리보기 계산 중/);
  assert.doesNotMatch(board, /주문 미리보기 계산 중/);
  assert.doesNotMatch(checks, /확인 항목을 계산하고 있습니다/);
  assert.match(desk, /\|\| freightEditor \? \(/);
  assert.match(desk, /unavailable=\{!flowSummaryReady\}/);
  assert.match(footer, /shipment\.transferred \|\| unavailable/);
  assert.match(checks, /const visible = blocks;/);
  assert.match(board, /aria-busy=\{!ready\}/);
});

test("freight typing stays local and flushes once after pause or before switching shipments", () => {
  const rows = read("src/WorkflowFooter.tsx");
  const input = rows.split("function FreightAmountInput(")[1]?.split("type FreightProps = {")[0];
  assert.ok(input);
  assert.match(input, /const \[text, setText\] = useState/);
  assert.match(input, /pending\.current = amount/);
  assert.match(input, /setTimeout\(flush, 350\)/);
  assert.match(input, /onBlur=\{flush\}/);
  assert.match(input, /if \(e\.key === "Enter"\)/);
  assert.match(input, /latestCommit\.current\(amount\)/);
  assert.doesNotMatch(input.split("onChange={(e) => {")[1]?.split("onBlur={flush}")[0] ?? "",
    /onFreight\(|latestCommit\.current\(/, "A keystroke must never trigger the heavy Rust reconciliation immediately");
  assert.match(rows, /onCommit=\{\(amount\) => onFreight\(origin, amount\)\}/);
});

test("two unconfirmed receipts are not discarded and are previewed independently", () => {
  const app = read("src/app/useAppModel.ts");
  const hub = read("src/HubWorkspace.tsx");
  const native = read("src/orders/useNativeWorkflowReconcile.ts");
  assert.match(app, /const newOrder = async \(\) => \{[\s\S]*?draft\.raw\.trim\(\)[\s\S]*?setDraft\(newReception\(\)\)/);
  assert.match(hub, /const draftsToRetain = \[draft, receptionOrder\]/);
  assert.match(hub, /Object\.fromEntries\(draftsToRetain\.map/);
  assert.match(hub, /pendingReceptions=\{pendingWithoutClient\}/);
  assert.match(hub, /onSelectPending=\{\(id\) => \{/);
  assert.match(app, /pendingDraftCount > 0 && !\(await confirmAction/);
  assert.match(hub, /Object\.values\(edits\)\.filter\(\(order\) => order\.id !== draft\.id && order\.clientId\)/);
  assert.match(hub, /useNativeWorkflowReconcile\(state, independentEdits\)/);
  assert.match(hub, /useNativeWorkflowReconcile\(base, draft\.clientId \? \[draft\] : \[\]\)/);
  assert.match(hub, /const newlyConfirmed = new Set\(next\.orders/);
  assert.match(native, /reconcile_workflow/);
  // Unsaved draft previews remain isolated from the durable SQLite commit.
  const reception = read("src-tauri/src/business_core/rules/reception_operations.rs");
  assert.match(reception, /"persisted_reception_state"/);
  assert.match(reception, /\.retain\(\|o\|!drafts\.contains\(str_field\(o,"id"\)\)\)/);
});
