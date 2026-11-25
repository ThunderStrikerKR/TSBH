import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const { hoveredWorkflowSelection, workflowGroupOpen } =
  await tsImport("../src/workspace-selection.ts", import.meta.url);

function seeded() {
  return {
    orders: ["o1","o2","o3"].map((id) => ({id, deletedAt:null})),
    workflow: {
      groups: [{id:"g1"},{id:"g2"}],
      shipments: [
        {id:"s1",orderId:"o1",groupId:"g1"},
        {id:"s2",orderId:"o2",groupId:"g2"},
        {id:"s3",orderId:"o3",groupId:"g2"},
      ],
    },
  };
}

test("hover on another group's one-touch button shows THAT order, not the first selected order", () => {
  const state=seeded();
  const chosen={orderId:"o1",groupId:"g1",shipmentId:"s1"};
  assert.deepEqual(hoveredWorkflowSelection(state,"g2",chosen),
    {orderId:"o2",groupId:"g2",shipmentId:"s2"});
  // The original selection is never mutated by hovering.
  assert.deepEqual(chosen,{orderId:"o1",groupId:"g1",shipmentId:"s1"});
  assert.equal(hoveredWorkflowSelection(state,undefined,chosen),undefined);
  assert.equal(hoveredWorkflowSelection(state,"missing",chosen),undefined);
});

test("hover retains the current recipient when hovering its own multi-recipient group", () => {
  const state=seeded();
  const selected={orderId:"o3",groupId:"g2",shipmentId:"s3"};
  assert.deepEqual(hoveredWorkflowSelection(state,"g2",selected),selected);
  state.workflow.groups[1].deletedAt="2026-11-18";
  assert.equal(hoveredWorkflowSelection(state,"g2",selected),undefined,
    "deleted groups cannot regain focus from transient hover");
});

test("one-touch hover leaves accordion heights unchanged while preview can show multiple shipments", () => {
  // Only an explicit shipment selection or title toggle expands a group.
  assert.equal(workflowGroupOpen("g1", "g1"), true);
  assert.equal(workflowGroupOpen("g2", "g1"), false);
  assert.equal(workflowGroupOpen("g1", "g1", "g1"), false);
  assert.equal(workflowGroupOpen("g2", "g1", "g1"), false);
  // Hover picks its own first shipment without mutating the committed selection.
  const state = seeded();
  const selected = {orderId:"o1", groupId:"g1", shipmentId:"s1"};
  assert.deepEqual(hoveredWorkflowSelection(state, "g2", selected),
    {orderId:"o2", groupId:"g2", shipmentId:"s2"});
  assert.equal(state.workflow.shipments.filter(s => s.groupId === "g2").length, 2);
  assert.deepEqual(selected, {orderId:"o1", groupId:"g1", shipmentId:"s1"});
});

test("hover selects each actual source and detail and scrolls into stage content", () => {
  const hub=read("src/HubWorkspace.tsx");
  const board=read("src/WorkflowBoard.tsx");
  assert.match(hub,/hoveredWorkflowSelection\(preview, stageHover\?\.groupId, selected, stageHover\?\.shipmentId\)/);
  assert.match(hub,/const visibleSelection = hoverSelection \?\? linkedHoverSelection \?\? selected/);
  assert.match(hub,/const visibleReceptionId = hoverSelection\?\.orderId \?\? linkedHoverSelection\?\.orderId \?\? receptionId/);
  assert.match(hub,/visibleReceptionId === draft\.id/);
  assert.match(hub,/const detail = visibleSelection/);
  assert.match(hub,/workflowSelection=\{visibleSelection\}/);
  assert.match(hub,/hoveredShipmentId=\{hoverSelection\?\.shipmentId\}/);
  assert.match(board,/open = workflowGroupOpen\(g\.id, selection\?\.groupId, manuallyClosedGroup\)/);
  assert.match(board,/isPreviewed = inPreview && hovered && hoveredShipmentId === s\.id/);
  // The current in-place overlay announces its active recipient through the
  // shipment accessible label rather than an obsolete inline status badge.
  assert.match(board,/aria-label=\{"배송 건 " \+ recipient\.name \+ \(isPreviewed \? " \(미리보기 표시 중\)"/);
  assert.match(board,/aria-label=\{"배송 건 " \+ recipient\.name \+ \(isPreviewed \?/);
  assert.match(hub,/stageHover\.step === 1[\s\S]*?\.recipient-review/);
  assert.match(hub,/stageHover\.step === 2[\s\S]*?\.freight-input-area/);
  assert.match(hub,/stageHover\.step === 3[\s\S]*?\.message-copy-actions/);
  assert.match(hub,/\.scrollIntoView\(\{ block: "nearest", inline: "nearest" \}\)/);
  assert.match(board, /onMouseEnter=\{isPreview \? keepStagePreview : \(event\) => startStagePreview\(event\.currentTarget, g\.id, step\)\}/);
  assert.doesNotMatch(board, /createPortal\(/);
  assert.match(board, /previewLayer\?\.groupId === g\.id/);
  assert.match(board, /className="workflow-card-overlay"/);
  assert.match(board, /onMouseEnter=\{keepStagePreview\}/);
  assert.match(board, /className=\{[\s\S]*?"workflow-shipment"[\s\S]*?isPreviewed \? " previewed"/);
  assert.match(board, /onMouseLeave=\{scheduleStagePreviewExit\}/);
  assert.match(board, /if \(inPreview\) \{[\s\S]*?stopStagePreview\(\);/);
});

test("new order resets stale preview immediately and second click is idempotent", () => {
  const hub=read("src/HubWorkspace.tsx");
  const app=read("src/app/useAppModel.ts");
  assert.match(hub,/useLayoutEffect\(\(\) => \{[\s\S]*?setReceptionId\(draft\.id\);[\s\S]*?setStageHover\(undefined\);[\s\S]*?setActive\(""\);[\s\S]*?commitSelection\(undefined\);[\s\S]*?lastVisiblePreview\.current = undefined;/);
  assert.match(app,/const newOrder = async \(\) => \{[\s\S]*?draft\.receptionVersion === 1[\s\S]*?!draft\.raw\.trim\(\) && !draft\.clientId &&[\s\S]*?!draft\.recipients\.length && !draft\.photos\?\.length[\s\S]*?setWorkflowSelection\(undefined\);[\s\S]*?return;/);
});

test("new order always returns to a blank receipt when another temporary or saved receipt is visible", () => {
  const hub = read("src/HubWorkspace.tsx");
  const action = hub.split('const createReceptionLive = useEventCallback(() => {')[1]
    ?.split('  const openManual = ')[0];
  assert.ok(action, "new-order draft-retention handler must be mounted");
  const source = read("src/SourceDocument.tsx");
  assert.match(source, /className="source-new-order"/);
  assert.match(source, /onClick=\{onNew\}/);
  assert.match(hub, /onNew=\{createReceptionLive\}/);
  assert.doesNotMatch(hub.split('<h2>처리 중인 주문<\/h2>')[1]?.split('</header>')[0] ?? "",
    /새 주문/, "workflow queue must no longer own the button");
  // The current viewed receipt is saved only to React's temporary overlay.
  assert.match(action, /const draftsToRetain = \[draft, receptionOrder\]/);
  assert.match(action, /setEdits\(\(previous\) => \(\{/);
  // Existing empty draft is reused by useAppModel. Its ID does not change;
  // the local visible receipt, pending selection and hovered workflow must.
  assert.match(action, /const emptyNewDraft = draft\.receptionVersion === 1/);
  assert.match(action, /if \(emptyNewDraft\) \{\s*setReceptionId\(draft\.id\);\s*setStageHover\(undefined\);\s*setActive\(""\);\s*commitSelection\(undefined\);\s*lastVisiblePreview\.current = undefined;/);
  assert.ok(action.indexOf("setReceptionId(draft.id)") < action.indexOf("props.onNew()"));
  // Filled or saved receipts still obey their existing asynchronous leave guard.
  assert.match(action, /props\.onNew\(\)/);
});

test("redundant group heading and source button are gone, cards remain scrollable", () => {
  const board=read("src/WorkflowBoard.tsx");
  const hub=read("src/HubWorkspace.tsx");
  const css=read("src/styles/workflow.css");
  assert.doesNotMatch(board,/<div className="workflow-board-heading">/);
  assert.doesNotMatch(hub,/선택 원문 보기/);
  assert.match(board,/className="workflow-board"/);
  assert.match(css,/\.hub-slot-3 \.workflow-board\s*\{[\s\S]*?overflow: auto;/);
  assert.doesNotMatch(css,/\.workflow-board-heading/);
});

test("pending reception card keeps standard controls without obsolete draft badge", () => {
  const board = read("src/WorkflowBoard.tsx");
  const pending = board.split('if (entry.kind === "pending")')[1]?.split("const g = entry.group;")[0];
  assert.ok(pending, "pending reception branch must exist");
  assert.match(pending, /<SignalLights step=\{0\} \/>/);
  assert.match(pending, /묶음 관리/);
  assert.match(pending, /onDeletePending\?\.\(receipt\.id\)/);
  assert.match(pending, /className="workflow-action-line"/);
  assert.match(pending, /거래처 확인/);
  assert.match(pending, /className="workflow-undo"/);
  assert.doesNotMatch(pending, />미확정<\/span>/);
  assert.match(board, /pendingManage && pendingReceptions/);
  const hub = read("src/HubWorkspace.tsx");
  assert.match(hub, /onDeletePending=\{deletePendingLive\}/);
});

test("payment and VAT stay beside each other to the left of right-fixed stage buttons", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  const group = board.split("const g = entry.group;")[1];
  const action = group.split('<div className="workflow-action-line">')[1]
    ?.split("{(g.needsNotice")[0];
  assert.ok(action);
  assert.ok(action.indexOf('className="flow-fixed-summary"') <
    action.indexOf('className="workflow-progress"'),
    "financial summary must come before right-hand actions");
  assert.match(action, /className="flow-summary-financial"/);
  assert.match(action, /nativeGroup && vatIncludedCount > 0/);
  assert.doesNotMatch(action, /: "부가세 별도"\}/);
  assert.match(css, /\.flow-summary-financial\s*\{[\s\S]*?margin-left: auto;/);
  assert.match(css, /\.workflow-action-line > \.workflow-progress\s*\{[\s\S]*?justify-content: flex-end/);
});

test("one-touch preview enters and restores at 250ms without reflow or delayed click", () => {
  const board = read("src/WorkflowBoard.tsx");
  const css = read("src/styles/workflow.css");
  const sel = read("src/workspace-selection.ts");
  assert.match(board, /hoverDelay\.current = setTimeout\(\(\) => \{[\s\S]*?onStageHover\?\.\(\{ groupId, step: Math\.min\(step \+ 1, 4\) \}\);[\s\S]*?\}, 250\)/);
  assert.match(board, /hoverReturnDelay\.current = setTimeout\(\(\) => \{[\s\S]*?stopStagePreview\(\);[\s\S]*?\}, 250\)/);
  assert.match(board, /onMouseEnter=\{keepStagePreview\}/);
  assert.match(board, /onMouseLeave=\{scheduleStagePreviewExit\}/);
  assert.match(board, /if \(hoverReturnDelay\.current\) clearTimeout\(hoverReturnDelay\.current\)/);
  assert.match(board, /onClick=\{async \(\) => \{[\s\S]*?nativeApplyWorkflowAction/);
  // Current approved preview overlays the card in place: no portal and no
  // fixed viewport placement that can change button coordinates.
  assert.match(css, /\.workflow-card-anchor\s*\{\s*position: relative;/);
  assert.match(css, /\.workflow-card-overlay\s*\{\s*position: absolute;/);
  assert.doesNotMatch(css, /\.workflow-preview-layer\s*\{/);
  assert.match(css, /\.workflow-progress > \.primary:is\(:hover, \[data-preview-hover-active="true"\]\):not\(:disabled\)/);
  assert.match(css, /--ds-role-surface-emphasized-action-hover/);
  assert.match(sel, /return selectedGroupId === groupId && manuallyClosedGroupId !== groupId/);
  assert.doesNotMatch(sel, /hoveredGroupId/);
  assert.match(board, /aria-label=\{"배송 건 " \+ recipient\.name \+ \(isPreviewed \? " \(미리보기 표시 중\)"/);
  assert.match(css, /\.workflow-card-overlay \.workflow-shipment\.previewed::before/);

});


test("late SQLite working-copy recovery cannot steal focus from an explicit new receipt", () => {
  const hub = read("src/HubWorkspace.tsx");
  assert.match(hub, /const explicitNewReception = useRef\(false\)/);
  const restore = hub.split("workingReceptionsLoad().then((stored) => {")[1]
    ?.split("setWorkingStoreReady(true)")[0];
  assert.ok(restore);
  assert.match(restore, /setEdits\(\(previous\) => \(\{/,
    "all recovered receipts remain available on the board");
  assert.match(restore, /if \(pending && !explicitNewReception\.current\) setReceptionId\(pending\.id\)/);
  assert.match(hub, /const createReceptionLive = useEventCallback\(\(\) => \{\s*explicitNewReception\.current = true;/);
});
