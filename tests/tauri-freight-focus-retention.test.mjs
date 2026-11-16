import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

// Structural regression for a real WebView2 focus-loss defect.
// This check catches unmounting caused by Rust preview pending states;
// it is NOT a substitute for running the exact EXE with a physical focus test.
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const desk = read("src/OrderDesk.tsx");
const hook = read("src/orders/useOrderDesk.tsx");
const rows = read("src/WorkflowFooter.tsx");

test("freight editor stays mounted while Rust workflow/total previews refresh", () => {
  assert.match(hook,/const flowSummaryReady = flowReady && nativeBoard\.ready && nativeOrder\.ready && nativeIssues\.ready;/);
  assert.match(desk,/const lastReadyFreight = useRef</);
  assert.match(desk,/if \(flowSummaryReady && activeShipment && activeShipmentPreview\)/);
  assert.match(desk,/const freightEditor = lastReadyFreight\.current/);
  assert.match(desk,/\{freightEditor\?\.recipientId === r\.id && \(/);
  assert.doesNotMatch(desk,/\{flowSummaryReady && activeShipmentPreview &&\s*activeShipment\?\.recipientId === r\.id/);
});
test("freight edit input reads current editable draft, not a stale Rust projection", () => {
  assert.match(desk,/\.\.\.freightEditor\.preview\.displayCharges,\s*\.\.\.r\.shippingCharges/);
  assert.match(desk,/\.\.\.freightEditor\.shipment\.charges,\s*\.\.\.row\.shippingCharges,\s*\[origin\]: value/);
  assert.match(desk,/props\.workflowSelection\.shipmentId === lastReadyFreight\.current\.shipment\.id/);
  assert.match(rows,/key=\{origin\}/);
  assert.match(rows,/requestAnimationFrame\(\(\) => \{/);
  assert.match(rows,/input\.isConnected \|\| document\.activeElement !== input/);
});
