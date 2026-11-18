import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("expanded and hovered workflow groups share one shipment table", () => {
  const board = source("src/WorkflowBoard.tsx");
  // One pending-reception list plus one shared, expanded/hovered workflow list.
  assert.equal((board.match(/className="workflow-shipment-list"/g) ?? []).length, 2);
  assert.match(board, /const renderShipmentList = \(inPreview: boolean\)/);
  assert.match(board, /\{open && renderShipmentList\(false\)\}/);
  assert.match(board, /\{renderShipmentList\(true\)\}/);
  assert.doesNotMatch(board, /workflow-preview-layer-shipment/);
  assert.match(board, /className="workflow-preview-layer hub-slot-3"/);
  assert.doesNotMatch(board, /className="app-v03 workflow-preview-layer"/);
});

test("preview uses natural row height, only scrolls when viewport requires it", () => {
  const board = source("src/WorkflowBoard.tsx");
  const css = source("src/styles/workflow.css");
  assert.match(board, /querySelector<HTMLElement>\(":scope > \.workflow-shipment-list"\)/);
  assert.match(board, /expandedRect\?\.width \?\? contentWidth/);
  assert.match(board, /\.querySelector<HTMLElement>\("\.workflow-shipment-list"\)\?\.scrollHeight/);
  assert.match(board, /maxHeight: availableHeight/);
  assert.doesNotMatch(board, /height:\s*previewLayer\.height/);
  assert.doesNotMatch(board, /overflowY:\s*"hidden"/);
  assert.doesNotMatch(board, /maxHeight:\s*expandedRect\s*\?/);
  assert.match(board, /useLayoutEffect\(\(\) =>/);
  assert.match(board, /resizeObserver\?\.observe\(list\)/);
  assert.match(board, /hovered && !open && previewLayer\?\.groupId === g\.id/);
  const layer = css.slice(css.indexOf(".workflow-preview-layer {"),
    css.indexOf(".workflow-preview-layer .workflow-shipment-list {"));
  assert.match(layer, /padding:\s*0;/);
  assert.match(layer, /border:\s*0;/);
  assert.match(layer, /box-shadow:\s*none;/);
  assert.match(layer, /overflow-y:\s*auto;/);
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
