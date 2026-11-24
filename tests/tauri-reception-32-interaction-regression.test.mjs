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
  assert.match(desk, /unavailable=\{!flowSummaryReady \|\| !activeShipmentPreview/);
  assert.match(footer, /shipment\.transferred \|\| unavailable/);
  assert.match(checks, /const visible = blocks;/);
  assert.match(board, /aria-busy=\{!ready\}/);
});

test("native egui freight edits retain the local draft until a cell commit action", () => {
  const manual = read("egui-app/src/manual_workspace.rs");
  const board = read("egui-app/src/workflow_board.rs");
  assert.match(manual, /manual_navigation_key_before_editor/);
  assert.match(manual, /EscapeAction::RevertCell/);
  assert.match(manual, /remote_rows/);
  assert.match(manual, /view_quote/);
  assert.match(manual, /shippingCharge/);
  assert.match(board, /fn native_remote_preview_is_not_replaced_by_optional_manual_checkbox/);
});

test("two unconfirmed receipts are not discarded and are previewed independently", () => {
  const app = read("src/app/useAppModel.ts");
  const hub = read("src/HubWorkspace.tsx");
  const native = read("src/orders/useNativeWorkflowReconcile.ts");
  assert.match(app, /const newOrder = async \(\) => \{[\s\S]*?draft\.raw\.trim\(\)[\s\S]*?setDraft\(newReception\(\)\)/);
  assert.match(hub, /const draftsToRetain = \[draft, receptionOrder\]/);
  assert.match(hub, /Object\.fromEntries\(draftsToRetain\.map/);
  assert.match(hub, /pendingReceptions=\{pendingWithoutClient\}/);
  assert.match(hub, /onSelectPending=\{selectPendingLive\}/);
  assert.match(hub, /const selectPendingLive = useEventCallback\(\(id: string\) => \{/);
  assert.match(hub, /if \(id === draft\.id \|\| edits\[id\]\) \{/);
  assert.match(hub, /commitSelection\(undefined\);\s*setReceptionId\(id\)/);
  assert.match(hub, /workingReceptionsLoad\(\)/);
  assert.match(hub, /workingWriter\.current!\.enqueue\(workingCopies\)/);
  assert.doesNotMatch(app, /pendingDraftCount/);
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


test("each workflow receipt uses a separate compact card and a single-line payment summary", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  assert.match(board, /className="flow-summary-compact"/);
  assert.match(board, /배송 \{ss\.length\}건/);
  assert.match(board, /className="flow-summary-payment"/);
  assert.match(board, /nativeGroup\?\.shipments\.filter\(\(shipment\) => shipment\.vatIncluded\)\.length/);
  assert.match(board, /mixedVat[\s\S]*?포함\$\{vatIncludedCount\}·별도\$\{vatExcludedCount\}/);
  assert.match(board, /vatIncludedCount > 0 \? "부가세 포함" : "부가세 별도"/);
  assert.doesNotMatch(board, /입금액 \{won\(/);
  assert.doesNotMatch(board, /총 운임/);
  assert.doesNotMatch(board, /미확정 · 임시/);
  assert.match(css, /\.hub-slot-3 \.workflow-group \{[\s\S]*?border: 1px solid/);
  assert.match(css, /\.flow-summary-compact \{[\s\S]*?justify-content: space-between/);
  assert.match(css, /\.flow-summary-payment \{[\s\S]*?text-align: right/);
});

test("freight-only edits reuse completed native product pricing", () => {
  const hook = read("src/orders/useOrderDesk.tsx");
  const effect = hook.split("const pricingInputKey =")[1]?.split("  useEffect(() => {")[0];
  assert.ok(effect);
  assert.match(effect, /item\.productId, item\.variantId/);
  assert.doesNotMatch(effect, /shippingCharges/);
  assert.match(hook, /lastCompletedPricing\.current\?\.key === pricingInputKey/);
  assert.match(hook, /lastCompletedPricing\.current\.products === state\.products/);
  assert.match(hook, /"rules", "reprice_order"/);
});

test("explicit unassigned receipt selection is never replaced by the first shipment", () => {
  const hub = read("src/HubWorkspace.tsx");
  const selectionEffect = hub.split("  // Selection and active shipment identity")[1]?.split("  useEffect(() => {\n    if (selected) props.onRememberSelection")[0] ?? "";
  assert.match(selectionEffect, /pendingWithoutClient\.some\(\(order\) => order\.id === receptionId\)/,
    "An explicitly clicked dealer-unassigned receipt must be protected before shipment fallback");
  assert.match(selectionEffect, /if \(selected\) commitSelection\(undefined\);\s*return;/);
  assert.match(selectionEffect, /selectedReceptionShipment = receptionId !== draft\.id/);
  assert.match(selectionEffect, /activeShipments\(preview\)\.find\(\(shipment\) => shipment\.orderId === receptionId\)/);
  assert.match(selectionEffect, /validSelection\(preview, preferredSelection\)/);
  assert.match(hub, /const selectPendingLive = useEventCallback\(\(id: string\) => \{/);
  assert.match(hub, /commitSelection\(undefined\);\s*setReceptionId\(id\)/);
});

test("title selects one group and hover never changes the accordion layout", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  assert.match(board, /open = workflowGroupOpen\(g\.id, selection\?\.groupId, manuallyClosedGroup\)/);
  assert.match(board, /useEffect\(\(\) => setManuallyClosedGroup\(undefined\), \[selection\?\.groupId\]\)/);
  assert.match(board, /className="flow-heading-toggle"/);
  assert.match(board, /setManuallyClosedGroup\(manuallyClosedGroup === g\.id \? undefined : g\.id\)/);
  assert.match(board, /onSelect\(\{\s*orderId: ss\[0\]\.orderId,\s*groupId: g\.id/);
  assert.match(board, /selectedPendingOrderId === receipt\.id/);
  assert.match(css, /\.flow-heading-toggle \{[\s\S]*?flex: 1 1 auto/);
});
test("navigation retains every unconfirmed reception without repetitive discard dialogs", () => {
  const model = read("src/app/useAppModel.ts");
  const hub = read("src/HubWorkspace.tsx");
  const app = read("src/App.tsx");
  assert.match(hub, /const temporaryReceipts = useMemo/);
  assert.match(hub, /for \(const order of Object\.values\(edits\)\)/);
  assert.match(hub, /if \(unsaved\(draft\)\) byId\.set\(draft\.id, draft\)/);
  assert.match(app, /className="hub-desk-host" hidden=\{tab !== "접수"\}/);
  assert.match(hub, /await workingWriter\.current!\.enqueue\(workingCopies\)/);
  assert.doesNotMatch(hub, /미완성 발주서 \$\{temporaryReceipts\.length\}건이 임시 보관 중입니다/);
  assert.doesNotMatch(hub, /title: "미완성 발주서 처리"/);
  assert.doesNotMatch(hub, /모두 버리고 이동/);
  assert.match(model, /confirmOrderLeave\("navigate"\)/);
  assert.match(model, /if \(!leave\.handledDraft && !\(await confirmReceptionDraftLeave\(\)\)\) return;/);
  assert.doesNotMatch(model, /pendingDraftCount/);
  assert.match(model, /const leave = await confirmOrderLeave\("close"\)/);
  assert.match(model, /confirmOrderLeave\("close"\)/);
  assert.match(hub, /intent === "switch" && workingCopies\.some/);
  assert.match(hub, /setEdits\(\(previous\) => \(\{ \.\.\.previous, \[draft\.id\]: structuredClone\(draft\) \}\)\)/);
});

test("photograph-only unfinished orders are retained and counted as unsaved", () => {
  const app = read("src/app/useAppModel.ts");
  const hub = read("src/HubWorkspace.tsx");
  assert.match(app,/draft\.photos\?\.length/);
  assert.match(hub,/order\.photos\?\.length/);
  assert.match(hub,/lastVisiblePreview\.current = undefined/);
});

test("dealer-unassigned receptions share the normal workflow group list and order", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  assert.match(board, /const entries = \[/);
  assert.match(board, /\.\.\.groups\.map\(\(group\) => \(\{ kind: "group"/);
  assert.match(board, /\.\.\.pendingReceptions\.map\(\(receipt\) => \(\{/);
  assert.match(board, /\]\.sort\(\(a, b\) => b\.createdAt\.localeCompare\(a\.createdAt\)\)/);
  assert.match(board, /entries\.map\(\(entry\) => \{/);
  assert.match(board, /if \(entry\.kind === "pending"\)/);
  assert.match(board, /<article className=\{"workflow-group"/);
  assert.match(board, /<span className="flow-group-name"><b>미확인 거래처<\/b><\/span>/);
  assert.match(board, /pendingSelected && manuallyClosedGroup !== pendingKey/);
  assert.match(board, /onSelectPending\?\.\(receipt\.id\)/);
  assert.match(board, /receipt\.recipients\.map\(\(recipient\) =>/);
  assert.doesNotMatch(board, /workflow-unassigned-draft|거래처 미선택/);
  assert.doesNotMatch(css, /workflow-unassigned-draft/);
});

test("stage signal lights live beside the dealer name without widening the one-touch action row", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  const group = board.slice(board.indexOf("const g = entry.group;"));
  const header = group.slice(group.indexOf("<header>"), group.indexOf("</header>"));
  // Totals intentionally precede the right-aligned stage controls.
  const progressStart = group.indexOf('<div className="workflow-progress">');
  const progressEnd = group.indexOf('                </div>\n              )}', progressStart);
  assert.ok(progressStart > group.indexOf('<div className="flow-fixed-summary"'));
  assert.ok(progressEnd > progressStart);
  const progress = group.slice(progressStart, progressEnd);
  assert.match(header, /<span className="flow-group-name">[\s\S]*?<\/span>\s*\{first\.order\.orderType !== "RETAIL" && nativeGroup && \(\s*<SignalLights step=\{step\} notice=\{g\.needsNotice\} \/>/);
  assert.doesNotMatch(progress, /<SignalLights/);
  assert.match(progress, /className="primary"/);
  assert.match(progress, /className="workflow-undo"/);
  assert.match(css, /\.workflow-group \.flow-group-name\s*\{[\s\S]*?flex: 0 0 162px;[\s\S]*?width: 162px;/);
  assert.match(css, /\.workflow-group > header\s*\{[\s\S]*?flex-wrap: nowrap;/);
  assert.match(css, /\.workflow-group \.workflow-signals \.signal\s*\{[\s\S]*?width: 18px;[\s\S]*?height: 18px;/);
  assert.match(css, /\.flow-heading-toggle > \.workflow-signals\s*\{[\s\S]*?flex: 0 0 82px;[\s\S]*?margin-left: 4px;/);
  assert.match(css, /\.workflow-progress > \.primary\s*\{[\s\S]*?flex: 0 0 140px;[\s\S]*?width: 140px;[\s\S]*?max-width: 140px;/);
});

test("compact workflow cards retain two rows, consistent action geometry and unambiguous mixed VAT counts", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  const detail = read("src/OrderDesk.tsx");
  assert.match(board, /<div className="workflow-action-line">/);
  assert.match(css, /\.workflow-action-line > \.workflow-progress\s*\{[\s\S]*?flex: 0 0 177px;[\s\S]*?width: 177px;/);
  assert.match(css, /\.workflow-progress > \.primary\s*\{[\s\S]*?flex: 0 0 140px;[\s\S]*?width: 140px;/);
  assert.match(css, /\.workflow-group \.flow-group-name b\s*\{[\s\S]*?font-size: 16px;/);
  assert.match(css, /\.workflow-group > header > button\.trash-button\s*\{[\s\S]*?height: 34px;/);
  assert.match(board, /const mixedVat = vatIncludedCount > 0 && vatExcludedCount > 0/);
  assert.match(board, /부가세 포함 \$\{vatIncludedCount\}건 · 부가세 별도 \$\{vatExcludedCount\}건/);
  assert.doesNotMatch(board, /nativeGroup\?\.vatLabel === "부가세 포함"/);
  assert.match(detail, /부가세 혼합 · 배송별 확인/);
});
