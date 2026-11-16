import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";

// Approved Nov 13 contract: Rust owns all product line amounts.
// Nov 17 hook retains last Rust preview across unrelated pending work.
const source=readFileSync(new URL("../src/OrderDesk.tsx", import.meta.url),"utf8");
const start=source.indexOf('className="item-row-edit item-amount"');
const end=source.indexOf("</button>",start);
const cell=source.slice(start,end);

test("product-line amounts come from the approved Rust preview, never duplicated in React",()=>{
  assert.ok(start>=0 && end>start,"registered line price button missing");
  assert.match(cell,/orderPreview\.lineAmounts\.find\(/);
  assert.match(cell,/entry\.recipientId === r\.id/);
  assert.match(cell,/entry\.itemId === i\.id/);
  assert.match(cell,/\? won\([\s\S]*?\?\.amount \?\? null\)\s*:\s*"—"/);
  assert.doesNotMatch(cell,/flowSummaryReady|i\.unitPrice \* i\.quantity|loading/);
});
test("eliminate the unnecessary delayed-price loading overlay and React quote cache",()=>{
  for(const obsolete of ["linePriceInputKey","lastNativeLineAmounts","visibleLineAmounts",
    '"계산 중…"']) assert.ok(!source.includes(obsolete),obsolete);
});

test("unrelated preview loading cannot fabricate missing VAT or hide available Rust order totals",()=>{
  const footer=source.slice(source.indexOf('className="order-grand-total"'),
    source.indexOf('portalTargets?.details',source.indexOf('className="order-grand-total"')));
  assert.match(footer,/orderPreview\?\.shipmentCount \?\? "—"/);
  assert.match(footer,/orderPreview\?\.total !== null/);
  assert.match(footer,/orderPreview\?\.vatLabel \?\? "—"/);
  assert.doesNotMatch(footer,/flowSummaryReady|확인 필요/);
  const board=readFileSync(new URL("../src/WorkflowBoard.tsx", import.meta.url),"utf8");
  assert.match(board,/nativeGroup\?\.vatLabel \?\? "—"/);
});

test("native order footer caches only a settled Rust result keyed to billing inputs",()=>{
  const hook=readFileSync(new URL("../src/orders/useNativeOrderPreview.ts",import.meta.url),"utf8");
  const desk=readFileSync(new URL("../src/orders/useOrderDesk.tsx",import.meta.url),"utf8");
  assert.match(hook,/nativeOrderDisplayKey\(sourceState, draft\)/);
  assert.match(hook,/displayResult: resolved\?\.displayKey === displayKey/);
  assert.match(hook,/resolved\.result\.orderId === draft\.id/);
  assert.match(desk,/useNativeOrderPreview\(flowState, draft, flowReady, state\)/);
  assert.match(desk,/orderPreview: nativeOrder\.displayResult/);
  assert.match(desk,/flowSummaryReady = flowReady && nativeBoard\.ready && nativeOrder\.ready/);
});
