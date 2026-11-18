import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("expanded and hovered workflow groups share one shipment table", () => {
  const board = source("src/WorkflowBoard.tsx");
  assert.equal((board.match(/className="workflow-shipment-list"/g) ?? []).length, 1);
  assert.match(board, /const renderShipmentList = \(inPreview: boolean\)/);
  assert.match(board, /\{open && renderShipmentList\(false\)\}/);
  assert.match(board, /\{renderShipmentList\(true\)\}/);
  assert.doesNotMatch(board, /workflow-preview-layer-shipment/);
  assert.match(board, /className="workflow-preview-layer hub-slot-3"/);
  assert.doesNotMatch(board, /className="app-v03 workflow-preview-layer"/);
});

test("source and detail hover use the actual delayed controller", () => {
  const workspace = source("src/HubWorkspace.tsx");
  const desk = source("src/OrderDesk.tsx");
  const props = source("src/orders/useOrderDesk.tsx");
  const linked = source("src/LinkedText.tsx");
  assert.match(workspace, /beginLinkHoverLive[\s\S]*?setTimeout\([\s\S]*?, 200\)/);
  assert.match(workspace, /clearLinkHoverLive[\s\S]*?setTimeout\([\s\S]*?, 250\)/);
  assert.match(desk, /beginHover: linkController\?\.beginHover/);
  assert.match(desk, /clearHover: linkController\?\.clearHover/);
  assert.match(props, /beginHover\?: \(key: string\) => void/);
  assert.match(linked, /\(c\.beginHover \?\? c\.setActive\)\(key\)/);
});
