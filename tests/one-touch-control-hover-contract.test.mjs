import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const source = (path) =>
  readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("one-touch preview controls the orange emphasis only while the preview shipment exists", () => {
  const hub = source("src/HubWorkspace.tsx");
  assert.match(hub, /data-one-touch-hover=\{stageHover && hoverSelection \? "true" : "false"\}/);
  assert.match(hub, /const visibleSelection = hoverSelection \?\? linkedHoverSelection \?\? selected/);
  assert.match(hub, /data-hover-stage=\{stageHover\?\.step\}/);
  assert.match(hub, /data-hover-selected=\{stageHover\?\.groupId === visibleSelection\?\.groupId/);
});

test("only the explicitly requested, editable, order-value controls get orange emphasis", () => {
  const css = source("src/styles/receipt-layout.css");
  const start = css.indexOf('.hub-live-workspace[data-one-touch-hover="true"] .hub-panels .compact-review :is(');
  const end = css.indexOf(") {", start);
  assert.ok(start >= 0 && end > start, "Missing precisely scoped one-touch whitelist");
  const selectors = css.slice(start, end);
  for (const target of [
    ".recipient-review .recipient-fields .linked-field:not(.linked-active) .field > input",
    ".items-freight-panel .compact-items tbody tr:not(.freight-item-row) .linked-field:not(.linked-active) > button",
    ".items-freight-panel .compact-items tbody tr:not(.freight-item-row) .linked-field:not(.linked-active) > input",
    ".items-freight-panel .freight-input-area .freight-money > input",
    ".items-freight-panel .freight-input-area .freight-payment-code > select",
    '.items-freight-panel .dispatch-origin > button[aria-pressed="true"]',
    ".items-freight-panel .shipment-vat label.check",
    ".order-grand-total > .order-grand-total-label",
    ".order-grand-total > b",
    ".order-grand-total .shipment-count-value",
  ]) {
    assert.ok(selectors.includes(target), "One-touch target missing: " + target);
  }
  for (const forbidden of [
    ".receipt-client", ".review-pane", ".source-top",
    ".payment-message", ".message-account", ".waiting-payment",
    ".workflow-balance", ".staff-checks", ".freight-input-area,",
    ".items-freight-panel,", ".order-grand-total,",
  ]) {
    assert.ok(!selectors.includes(forbidden), "Whole-area highlight returned: " + forbidden);
  }
  assert.match(css.slice(end), /background-color: #3b2b1b !important;[\s\S]*?box-shadow: inset 0 0 0 2px #f59e0b !important;/);
  assert.doesNotMatch(css, /\.hub-live-workspace\[data-hover-stage="1"\]\s+\.hub-slot-2\s+\.review-pane/);
  assert.doesNotMatch(css, /\.hub-live-workspace\[data-hover-stage="2"\]\s+\.freight-input-area/);
  assert.doesNotMatch(css, /\.workflow-payment-hover\s+\.waiting-payment/);
});

test("recipient, product, quantity, money, freight, selected origin and VAT use real controls", () => {
  const desk = source("src/OrderDesk.tsx");
  const freight = source("src/WorkflowFooter.tsx");
  assert.match(desk, /className="recipient-fields"/);
  assert.match(desk, /<Field label="이름">/);
  assert.match(desk, /<Field label="주소">/);
  assert.match(desk, /<Field label="휴대폰">/);
  assert.match(desk, /<Field label="전화">/);
  assert.match(desk, /<Field label="요청 메시지">/);
  assert.match(desk, /<table className="compact-items">/);
  assert.match(desk, /className="item-row-edit"/);
  assert.match(desk, /className="item-row-edit item-amount"/);
  assert.match(freight, /className="freight-item-row freight-input-area"/);
  assert.match(freight, /className="freight-money"/);
  assert.match(freight, /className="freight-payment-code"/);
  assert.match(freight, /<select[\s\S]*?value=\{freightCode\}/);
  assert.match(desk, /className="dispatch-origin" role="group"/);
  assert.match(desk, /aria-pressed=\{/);
  assert.match(freight, /className="shipment-vat"/);
  assert.match(freight, /type="checkbox"/);
});

test("total label, total amount and only the dynamic count-plus-unit are highlighted", () => {
  const desk = source("src/OrderDesk.tsx");
  const css = source("src/styles/receipt-layout.css");
  const oldFooter = source("src/styles/patch-v0808.css");
  assert.match(desk, /className="order-grand-total-label">총 주문 금액<\/span>/);
  assert.match(desk, /<b data-recipient-fade=\{recipientRevealPhase\}>/);
  assert.match(desk, /className="shipment-count-label">\| 택배<\/span>/);
  assert.match(desk, /className="shipment-count-value">[\s\S]*?className="shipment-count-number"[\s\S]*?className="shipment-count-unit">건<\/span>/);
  assert.match(desk, /className="shipment-count-divider">\|<\/span>/);
  assert.match(oldFooter, /\.shipment-count-number\s*\{[^}]*flex:\s*0 0 3ch;[^}]*width:\s*3ch;/s);
  assert.match(css, /\.shipment-count-value\s*\{[^}]*display: inline-flex;[^}]*white-space: nowrap;/s);
  assert.match(css, /\.order-grand-total > b\s*\{[^}]*justify-self: end;/s);
  assert.doesNotMatch(css, /\.order-grand-total > \.order-vat-summary[^\n]*\{/);
});
