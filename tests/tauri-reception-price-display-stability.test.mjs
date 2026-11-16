import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";

// Exact original pre-migration rendering contract:
// src/OrderDesk.tsx at 30a19cd2da7533dd2ed36b807593309bde70a7eb.
const source=readFileSync(new URL("../src/OrderDesk.tsx", import.meta.url),"utf8");
const start=source.indexOf('className="item-row-edit item-amount"');
const end=source.indexOf("</button>",start);
const cell=source.slice(start,end);

test("a registered line displays its confirmed Rust unit price immediately, as original TS did",()=>{
  assert.ok(start>=0 && end>start,"registered line price button missing");
  assert.match(cell,/i\.unitPrice !== null\s*&&\s*i\.quantity !== null\s*&&\s*i\.productId\s*&&\s*i\.variantId/);
  assert.match(cell,/\? won\(i\.unitPrice \* i\.quantity\)\s*:\s*"확인 필요"/);
  assert.doesNotMatch(cell,/flowSummaryReady|orderPreview|ready|loading/);
});
test("eliminate the unnecessary delayed-price loading overlay and React quote cache",()=>{
  for(const obsolete of ["linePriceInputKey","lastNativeLineAmounts","visibleLineAmounts",
    '"계산 중…"']) assert.ok(!source.includes(obsolete),obsolete);
});
