import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("hover shows the SAME complete expanded card over its original card", () => {
  const board = source("src/WorkflowBoard.tsx");
  const css = source("src/styles/workflow.css");
  assert.equal((board.match(/className="workflow-shipment-list"/g) ?? []).length, 2);
  assert.match(board, /const renderShipmentList = \(inPreview: boolean\)/);
  assert.match(board, /const renderGroupCard = \(isPreview: boolean\) =>/);
  assert.match(board, /\{renderGroupCard\(false\)\}/);
  assert.match(board, /\{renderGroupCard\(true\)\}/);
  assert.match(board, /\{\(open \|\| isPreview\) && renderShipmentList\(isPreview\)\}/);
  assert.match(board, /className="workflow-preview-stage hub-slot-3"/);
  assert.match(board, /document\.querySelector\("\.app-v03"\) \?\? document\.body/);
  assert.doesNotMatch(board, /workflow-preview-layer/);
  assert.doesNotMatch(board, /createPortal\(\s*<div[^>]*>[\s\S]*?renderShipmentList\(true\)/);
  assert.match(css, /\.workflow-preview-stage \{/);
  assert.match(css, /\.workflow-preview-stage \.workflow-group \{/);
});

test("preview aligns to entire original card and reveals all its content", () => {
  const board = source("src/WorkflowBoard.tsx");
  const css = source("src/styles/workflow.css");
  assert.match(board, /\.closest<HTMLElement>\("\.workflow-group:not\(\.workflow-preview-clone\)"\)/);
  assert.match(board, /Math\.min\(rect\.width, window\.innerWidth - margin \* 2\)/);
  assert.match(board, /\.querySelector<HTMLElement>\("\.workflow-preview-clone"\)\?\.scrollHeight/);
  assert.match(board, /top = Math\.max\(margin, Math\.min\(rect\.top,/);
  assert.match(board, /maxHeight: previewLayer\.maxHeight/);
  assert.match(board, /onMouseEnter=\{keepStagePreview\}/);
  assert.match(board, /onMouseLeave=\{scheduleStagePreviewExit\}/);
  assert.doesNotMatch(board, /height:\s*previewLayer\.height/);
  assert.doesNotMatch(board, /overflowY:\s*"hidden"/);
  assert.match(board, /resizeObserver\?\.observe\(list\)/);
  const layer = css.slice(css.indexOf(".workflow-preview-stage {"),
    css.indexOf(".workflow-preview-stage .workflow-group {"));
  assert.match(layer, /position:\s*fixed;/);
  assert.match(layer, /padding:\s*0;/);
  assert.match(layer, /border:\s*0;/);
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
