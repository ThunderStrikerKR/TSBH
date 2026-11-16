import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source=readFileSync(new URL("../src/OrderDesk.tsx",import.meta.url),"utf8");
test("native order line price stays visible during independent workflow preview loading",()=>{
  assert.match(source,/lastNativeLineAmounts = useRef/);
  assert.match(source,/orderPreview\?\.orderId === draft\.id/);
  assert.match(source,/lastNativeLineAmounts\.current\?\.key === linePriceInputKey/);
  assert.match(source,/visibleLineAmounts\.find/);
  assert.doesNotMatch(source,/flowSummaryReady && orderPreview\s*\? won\(/);
  assert.match(source,/\: "계산 중…"/);
});
test("native line quote cache invalidates for every price-affecting order input",()=>{
  for(const input of ["orderId: draft.id","clientId: draft.clientId","tier: pricingClient.tier",
    "overrides: pricingClient.overrides","i.productId","i.variantId","i.unitPrice","i.quantity"]){
    assert.ok(source.includes(input),input);
  }
  const from=source.indexOf("const lastNativeLineAmounts");
  const to=source.indexOf("const lastReadyFreight",from);
  const cache=source.slice(from,to);
  assert.doesNotMatch(cache,/\*\s*i\.quantity|unitPrice\s*\*/);
  assert.match(cache,/orderPreview\.lineAmounts/);
});
