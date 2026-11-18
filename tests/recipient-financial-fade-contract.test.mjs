import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("recipient reveal waits for current Rust line, order, workflow and delivery readiness", () => {
  const hook = source("src/orders/useOrderDesk.tsx");
  const desk = source("src/OrderDesk.tsx");
  assert.match(hook, /financialPreviewReady: flowSummaryReady && nativeLines\.ready &&/);
  assert.match(hook, /\(!activeShipment \|\| Boolean\(activeShipmentPreview\)\)/);
  assert.match(hook, /const flowSummaryReady = flowReady && nativeBoard\.ready && nativeOrder\.ready && nativeIssues\.ready/);
  assert.match(desk, /const recipientReady = !draft\.recipients\.length \|\|[\s\S]*?financialPreviewReady && delivery\.ready/);
  assert.match(desk, /const financialError = Boolean\(flowSummaryError \|\| orderLineError \|\| delivery\.error\)/);
});

test("one completed transition gates recipient, items, freight and financial footer", () => {
  const desk = source("src/OrderDesk.tsx");
  const css = source("src/styles/receipt-layout.css");
  assert.match(desk, /const recipientViewKey = \[draft\.id, shownRecipient \?\? "", props\.workflowSelection\?\.shipmentId \?\? ""\]/);
  assert.match(desk, /if \(!recipientReady\) return;[\s\S]*?requestAnimationFrame\(\(\) => setRevealedRecipient\(recipientViewKey\)\)/);
  assert.match(desk, /return \(\) => cancelAnimationFrame\(frame\)/);
  assert.match(desk, /revealedRecipient === recipientViewKey \? "ready" : "pending"/);
  assert.match(desk, /className="recipient-body" data-recipient-fade=\{recipientRevealPhase\}/);
  assert.match(desk, /inert=\{recipientRevealPhase === "pending"\}/);
  assert.match(desk, /className="order-grand-total" data-recipient-fade=\{recipientRevealPhase\}/);
  assert.match(css, /\.compact-review \[data-recipient-fade="pending"\] \{[^}]*opacity:\s*0;[^}]*transition:\s*none;/s);
  assert.match(css, /\.compact-review \[data-recipient-fade="ready"\] \{[^}]*opacity:\s*1;[^}]*transition:\s*opacity 260ms ease-out;/s);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
});

test("recipient reveal does not gate in-place edits on a fixed animation timer", () => {
  const desk = source("src/OrderDesk.tsx");
  const section = desk.slice(desk.indexOf("const recipientViewKey"), desk.indexOf("const addressRequests"));
  assert.doesNotMatch(section, /setTimeout/);
  assert.doesNotMatch(section, /recalculateShipping/);
  assert.match(section, /revealedRecipient === recipientViewKey/);
});
