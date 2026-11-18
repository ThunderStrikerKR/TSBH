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

test("preview does not introduce its own selection colors, row labels or a second layout", () => {
  const board = source("src/WorkflowBoard.tsx");
  const css = source("src/styles/workflow.css");
  assert.match(board, /const visuallyChosen = isPreview[\s\S]*?ss\[0\]\.id === s\.id/);
  assert.match(board, /const isPreviewed = !isPreview && hovered/);
  assert.match(board, /\(selected \|\| isPreview \? " selected" : ""\)/);
  assert.match(board, /\(hovered && !isPreview \? " workflow-preview-group" : ""\)/);
  assert.match(css, /\.workflow-card-anchor\.previewing\s*\{\s*z-index:\s*400;/);
  assert.match(css, /\.workflow-card-overlay \.workflow-progress > \.primary:hover:not\(:disabled\)/);
  assert.doesNotMatch(board, /maxHeight: previewLayer/);
  assert.doesNotMatch(board, /transform:\s*previewLayer/);
});

test("shipment-list renderer only references its own inPreview argument", () => {
  const board = source("src/WorkflowBoard.tsx");
  const start = board.indexOf("const renderShipmentList = (inPreview: boolean)");
  const end = board.indexOf("const renderGroupCard = (isPreview: boolean)", start);
  assert.ok(start >= 0 && end > start, "both render helpers must exist");
  const body = board.slice(start, end);
  assert.doesNotMatch(body, /\bisPreview\b/, "undefined isPreview crashes expanded order rendering");
  assert.match(body, /const visuallyChosen = inPreview/);
  assert.match(body, /const isPreviewed = !inPreview/);
  assert.match(body, /if \(inPreview\)/);
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
