import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("hover renders the actual expanded card markup at the original card coordinates", () => {
  const board = source("src/WorkflowBoard.tsx");
  const css = source("src/styles/workflow.css");
  assert.equal((board.match(/className="workflow-shipment-list"/g) ?? []).length, 2);
  assert.match(board, /const renderShipmentList = \(inPreview: boolean\)/);
  assert.match(board, /const renderGroupCard = \(isPreview: boolean\) =>/);
  assert.match(board, /\{renderGroupCard\(false\)\}/);
  assert.match(board, /\{renderGroupCard\(true\)\}/);
  assert.match(board, /\{\(open \|\| isPreview\) && renderShipmentList\(isPreview\)\}/);
  assert.match(board, /className="workflow-card-overlay"/);
  assert.match(board, /className=\{"workflow-card-anchor"/);
  assert.doesNotMatch(board, /createPortal/);
  assert.doesNotMatch(board, /positionPreview/);
  assert.doesNotMatch(board, /workflow-preview-layer/);
  assert.match(css, /\.workflow-card-anchor\s*\{\s*position:\s*relative;/);
  assert.match(css, /\.workflow-card-overlay\s*\{\s*position:\s*absolute;\s*inset:\s*0 0 auto 0;/);
  assert.match(css, /\.workflow-card-overlay \.workflow-group\s*\{[^}]*width:\s*100%;[^}]*margin:\s*0;/s);
});

test("blue temporary display is separate from orange committed selection", () => {
  const board = source("src/WorkflowBoard.tsx");
  const css = source("src/styles/workflow.css");
  assert.match(board, /const visuallyChosen = !inPreview && selection\?\.shipmentId === s\.id/);
  assert.match(board, /const isPreviewed = inPreview && hovered && hoveredShipmentId === s\.id/);
  assert.match(board, /\(selected && !isPreview \? " selected" : ""\)/);
  assert.match(board, /\(isPreview \? " temporary-preview" : ""\)/);
  assert.match(css, /\.hub-slot-3 \.workflow-group\.selected\s*\{[^}]*border-left: 2px solid #f59e0b;/);
  assert.match(css, /\.hub-slot-3 \.workflow-card-overlay \.workflow-group\.temporary-preview\s*\{[^}]*border-left: 2px solid var\(--ds-role-border-selected\);/);
  assert.match(css, /\.hub-slot-3 \.workflow-card-overlay \.workflow-shipment\.previewed::before\s*\{[^}]*box-shadow: inset 3px 0 0 var\(--ds-role-border-selected\);/);
  assert.match(board, /const isPreviewed = inPreview/);
  assert.match(css, /\.workflow-card-anchor\.previewing\s*\{\s*z-index:\s*400;/);
  assert.doesNotMatch(board, /maxHeight: previewLayer/);
});

test("shipment-list renderer only references its own inPreview argument", () => {
  const board = source("src/WorkflowBoard.tsx");
  const start = board.indexOf("const renderShipmentList = (inPreview: boolean)");
  const end = board.indexOf("const renderGroupCard = (isPreview: boolean)", start);
  assert.ok(start >= 0 && end > start, "both render helpers must exist");
  const body = board.slice(start, end);
  assert.doesNotMatch(body, /\bisPreview\b/, "undefined isPreview crashes expanded order rendering");
  assert.match(body, /const visuallyChosen = !inPreview/);
  assert.match(body, /const isPreviewed = inPreview/);
  assert.match(body, /if \(inPreview\)/);
});

test("hovering recipient rows temporarily switches source and details", () => {
  const board = source("src/WorkflowBoard.tsx");
  const workspace = source("src/HubWorkspace.tsx");
  const selection = source("src/workspace-selection.ts");
  assert.match(board, /onMouseEnter=\{inPreview \? \(\) => beginPreviewRowHover\(g\.id, step, s\.id\) : undefined\}/);
  assert.match(board, /onMouseLeave=\{inPreview \? \(\) => endPreviewRowHover\(g\.id, step\) : undefined\}/);
  assert.match(board, /beginPreviewRowHover[\s\S]*?shipmentId[\s\S]*?\}, 100\)/);
  assert.match(board, /endPreviewRowHover[\s\S]*?\}, 150\)/);
  assert.match(board, /if \(rowEnterDelay\.current\) clearTimeout\(rowEnterDelay\.current\)/);
  assert.match(board, /if \(rowLeaveDelay\.current\) clearTimeout\(rowLeaveDelay\.current\)/);
  assert.match(workspace, /stageHover\?\.groupId, selected, stageHover\?\.shipmentId/);
  assert.match(selection, /shipments\.find\(\(s\) => s\.id === hoveredShipmentId\)/);
  assert.match(selection, /\?\? shipments\.find\(\(s\) => s\.id === selected\?\.shipmentId\)/);
  assert.doesNotMatch(board, /onMouseEnter=\{inPreview[^}]*onSelect/);
});

test("accordion preview preserves the exact card box and heading positions", () => {
  const css = source("src/styles/workflow.css");
  assert.match(css, /\.hub-slot-3 \.workflow-group\s*\{[^}]*box-sizing:\s*border-box;[^}]*border-left-width:\s*2px;/s);
  assert.match(css, /\.hub-slot-3 \.workflow-group\.selected\s*\{[^}]*border-left:\s*2px solid #f59e0b;/s);
  assert.match(css, /\.hub-slot-3 \.workflow-group \.flow-toggle-icon\s*\{[^}]*flex:\s*0 0 12px;[^}]*width:\s*12px;/s);
  assert.match(css, /\.workflow-card-anchor\s*\{[^}]*display:\s*flow-root;[^}]*margin-bottom:\s*8px;/s);
  assert.match(css, /\.hub-slot-3 \.workflow-card-anchor > \.workflow-group\s*\{\s*margin:\s*0;/);
  assert.match(css, /\.workflow-card-overlay \.workflow-group\s*\{\s*box-sizing:\s*border-box;\s*width:\s*100%;/);
});

test("clicking a shipment preview selects it and expands its order card", () => {
  const board = source("src/WorkflowBoard.tsx");
  assert.match(board, /if \(inPreview\) \{[\s\S]*?setManuallyClosedGroup\(undefined\);[\s\S]*?stopStagePreview\(\);[\s\S]*?\}[\s\S]*?onSelect\(\{/);
  assert.match(board, /useEffect\(\(\) => setManuallyClosedGroup\(undefined\), \[selection\?\.groupId\]\)/);
  const selection = source("src/workspace-selection.ts");
  assert.match(selection, /return selectedGroupId === groupId && manuallyClosedGroupId !== groupId/);
});

test("source and detail hover use the actual delayed controller", () => {
  const workspace = source("src/HubWorkspace.tsx");
  const desk = source("src/OrderDesk.tsx");
  const props = source("src/orders/useOrderDesk.tsx");
  const linked = source("src/LinkedText.tsx");
  assert.match(workspace, /beginLinkHoverLive[\s\S]*?setTimeout\([\s\S]*?, 100\)/);
  assert.match(workspace, /clearLinkHoverLive[\s\S]*?setTimeout\([\s\S]*?, 150\)/);
  assert.match(desk, /beginHover: linkController\?\.beginHover/);
  assert.match(desk, /clearHover: linkController\?\.clearHover/);
  assert.match(props, /beginHover\?: \(key: string\) => void/);
  assert.match(linked, /\(c\.beginHover \?\? c\.setActive\)\(key\)/);
});
