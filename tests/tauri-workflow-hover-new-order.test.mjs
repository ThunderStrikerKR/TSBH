import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const { hoveredWorkflowSelection } =
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

test("hover selects each actual source and detail and scrolls into stage content", () => {
  const hub=read("src/HubWorkspace.tsx");
  const board=read("src/WorkflowBoard.tsx");
  assert.match(hub,/hoveredWorkflowSelection\(preview, stageHover\?\.groupId, selected\)/);
  assert.match(hub,/const visibleSelection = hoverSelection \?\? selected/);
  assert.match(hub,/const visibleReceptionId = hoverSelection\?\.orderId \?\? receptionId/);
  assert.match(hub,/visibleReceptionId === draft\.id/);
  assert.match(hub,/const detail = visibleSelection/);
  assert.match(hub,/workflowSelection=\{visibleSelection\}/);
  assert.match(hub,/stageHover\.step === 1[\s\S]*?\.recipient-review/);
  assert.match(hub,/stageHover\.step === 2[\s\S]*?\.freight-input-area/);
  assert.match(hub,/stageHover\.step === 3[\s\S]*?\.payment-message/);
  assert.match(hub,/\.scrollIntoView\(\{ block: "nearest", inline: "nearest" \}\)/);
  assert.match(board,/onMouseEnter=\{\(\) => \{[\s\S]*?onStageHover\?\.\(\{ groupId: g\.id, step: Math\.min\(step \+ 1, 4\) \}\)/);
  assert.match(board,/onMouseLeave=\{\(\) => \{[\s\S]*?onStageHover\?\.\(undefined\)/);
});

test("new order resets stale preview immediately and second click is idempotent", () => {
  const hub=read("src/HubWorkspace.tsx");
  const app=read("src/app/useAppModel.ts");
  assert.match(hub,/useLayoutEffect\(\(\) => \{[\s\S]*?setReceptionId\(draft\.id\);[\s\S]*?setStageHover\(undefined\);[\s\S]*?setActive\(""\);[\s\S]*?commitSelection\(undefined\);[\s\S]*?lastVisiblePreview\.current = undefined;/);
  assert.match(app,/const newOrder = async \(\) => \{[\s\S]*?draft\.receptionVersion === 1[\s\S]*?!draft\.raw\.trim\(\) && !draft\.clientId &&[\s\S]*?!draft\.recipients\.length && !draft\.photos\?\.length[\s\S]*?setWorkflowSelection\(undefined\);[\s\S]*?return;/);
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
