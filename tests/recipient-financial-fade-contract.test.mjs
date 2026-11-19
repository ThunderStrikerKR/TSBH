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
  assert.match(desk, /const recipientReady = !draft\.recipients\.length \|\|[\s\S]*?financialPreviewReady && \(delivery\.ready \|\| Boolean\(delivery\.error\)\)/);
  assert.match(desk, /const financialError = Boolean\(flowSummaryError \|\| orderLineError\)/);
});

test("one completed transition gates recipient, items, freight and financial footer", () => {
  const desk = source("src/OrderDesk.tsx");
  const css = source("src/styles/receipt-layout.css");
  assert.match(desk, /const recipientViewKey = \[draft\.id, shownRecipient \?\? "", props\.workflowSelection\?\.shipmentId \?\? ""\]/);
  assert.match(desk, /if \(!recipientReady\) return;[\s\S]*?requestAnimationFrame\(\(\) => setRevealedRecipient\(recipientViewKey\)\)/);
  assert.match(desk, /return \(\) => cancelAnimationFrame\(frame\)/);
  assert.match(desk, /revealedRecipient === recipientViewKey \? "ready" : "pending"/);
  assert.match(desk, /const recipientRevealPhase = props\.coordinatedFadePhase \?\? localRecipientPhase/);
  assert.match(desk, /const checksFadePhase = props\.coordinatedFadePhase \?\? localChecksPhase/);
  // Headers, borders, scroll containers and empty-state instructions must
  // never disappear. Only actual recipient/order data is transition content.
  assert.match(desk, /<div className="recipient-body">\s*<header className="recipient-section-heading">/);
  assert.match(desk, /<p className="empty detail-empty">발주 원문을 입력하거나 배송 건을 선택하세요\.<\/p>/);
  assert.match(desk, /className="recipient-review" key=\{r\.id\}\s*data-recipient-fade=\{recipientRevealPhase\}/);
  assert.match(desk, /<section className="items-freight-panel">\s*<header className="recipient-section-heading">/);
  assert.match(desk, /className="items-freight-scroll receipt-panel-content">/);
  assert.match(desk, /<article key=\{r\.id\}\s*data-recipient-fade=\{recipientRevealPhase\}/);
  assert.match(desk, /거래처 선택 후 운임과 청구금액을 확인합니다\./);
  assert.match(desk, /recipientFade=\{recipientRevealPhase\}/);
  assert.match(desk, /<div className="order-grand-total">\s*<span className="order-grand-total-label">총 주문<\/span>/);
  assert.match(desk, /<b data-recipient-fade=\{recipientRevealPhase\}>/);
  assert.match(desk, /inert=\{recipientRevealPhase === "pending"\}/);
  assert.doesNotMatch(desk, /className="recipient-body" data-recipient-fade/);
  assert.doesNotMatch(desk, /className="order-grand-total" data-recipient-fade/);
  assert.match(css, /\.compact-review \[data-recipient-fade="pending"\] \{[^}]*opacity:\s*0;[^}]*transition:\s*none;/s);
  assert.match(css, /\.compact-review \[data-recipient-fade="ready"\] \{[^}]*opacity:\s*1;[^}]*transition:\s*opacity 260ms ease-out;/s);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
});

test("workflow footer fades only its content while retaining panel structure", () => {
  const footer = source("src/WorkflowFooter.tsx");
  const desk = source("src/OrderDesk.tsx");
  assert.match(footer, /recipientFade\?: "pending" \| "ready"/);
  assert.match(footer, /className="workflow-footer order-settings" aria-label="발주 설정"/);
  assert.match(footer, /data-recipient-fade=\{recipientFade\}/);
  assert.match(desk, /className="items-freight-panel">/);
  assert.match(desk, /className="recipient-section-heading">\s*<h2>품목 및 운임<\/h2>/);
  assert.match(desk, /className="order-grand-total">/);
});

test("shipping count stays aligned and VAT status has no brackets", () => {
  const desk = source("src/OrderDesk.tsx");
  const css = source("src/styles/patch-v0808.css");
  assert.match(desk, /className="shipment-count-label">\| 택배<\/span>/);
  assert.match(desk, /className="shipment-count-number" data-recipient-fade=\{recipientRevealPhase\}/);
  assert.match(desk, /className="shipment-count-value"/);
  assert.match(desk, /className="shipment-count-unit">건<\/span>/);
  assert.match(desk, /className="shipment-count-divider">\|<\/span>/);
  assert.match(desk, /className="order-grand-total-label">총 주문<\/span>/);
  assert.match(desk, /className="order-vat-summary"/);
  assert.doesNotMatch(desk, /className="order-vat-paren"/);
  assert.match(desk, /className="order-vat-label" data-recipient-fade=\{recipientRevealPhase\}/);
  // The layout must not resize based on order amount or which VAT label appears.
  const footer = css.slice(
    css.indexOf(".hub-panels .compact-review > .order-grand-total {"),
    css.indexOf("/* Product links are an optional copy choice", css.indexOf(".hub-panels .compact-review > .order-grand-total {")),
  );
  assert.match(footer, /grid-template-columns:\s*minmax\(0,\s*1fr\)\s+148px;/);
  assert.match(footer, /\.shipment-count-number\s*\{[^}]*flex:\s*0 0 3ch;[^}]*width:\s*3ch;[^}]*text-align:\s*right;[^}]*font-variant-numeric:\s*tabular-nums;/s);
  assert.match(footer, /\.order-vat-summary\s*\{[^}]*width:\s*148px;[^}]*text-align:\s*right;/s);
  assert.doesNotMatch(footer, /\.order-vat-paren\s*\{/);
  assert.match(footer, /\.order-vat-label\s*\{[^}]*text-align:\s*right;/s);
  assert.doesNotMatch(footer, /grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto/);
  // Data is not rounded or capped to 999; three digits is a presentation slot.
  assert.match(desk, /orderPreview\?\.shipmentCount \?\? "—"/);
  assert.doesNotMatch(desk, /Math\.min\(999,\s*orderPreview\?\.shipmentCount/);
});

test("recipient reveal does not gate in-place edits on a fixed animation timer", () => {
  const desk = source("src/OrderDesk.tsx");
  const section = desk.slice(desk.indexOf("const recipientViewKey"), desk.indexOf("const addressRequests"));
  assert.doesNotMatch(section, /setTimeout/);
  assert.doesNotMatch(section, /recalculateShipping/);
  assert.match(section, /revealedRecipient === recipientViewKey/);
});
