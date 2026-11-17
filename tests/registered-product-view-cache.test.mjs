import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("reception and details share display product sorting only by immutable catalog identity",()=>{
  const view=read("src/orders/registered-product-view.ts");
  const desk=read("src/orders/useOrderDesk.tsx");
  assert.match(view,/new WeakMap<Product\[\], Product\[\]>/);
  assert.match(view,/cachedViews\.get\(products\)/);
  assert.match(view,/if \(existing\) return existing/);
  assert.match(view,/sortProducts\(registeredProducts\(products\)\)/);
  assert.match(view,/cachedViews\.set\(products, sorted\)/);
  assert.match(desk,/sortedRegisteredProductView\(state\.products\)/);
  assert.match(desk,/\[state\.products\]/);
});
