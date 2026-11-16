import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("native shipping read refreshes only when referenced products or tariffs change",()=>{
  const source=read("src/useNativeShippingRead.ts");
  const rust=read("src-tauri/src/business_core/rules/shipping_read.rs");
  assert.match(source,/products: state\.products,[\s\S]*?shipping: state\.shipping/);
  assert.match(source,/\[state\.products, state\.shipping\]/);
  assert.match(source,/"shipping_read", \{ state: source \}/);
  assert.match(source,/snapshot\?\.source === source/);
  assert.doesNotMatch(source,/\}, \[state\]\)/);
  assert.match(rust,/list\(state,"products"\)/);
  assert.match(rust,/field\(state,"shipping"\)/);
});
test("product freight read ignores changes unrelated to product and freight tables",()=>{
  const source=read("src/ProductFreightPanel.tsx");
  const rust=read("src-tauri/src/business_core/rules/shipping.rs");
  assert.match(source,/products: state\.products,[\s\S]*?shipping: state\.shipping/);
  assert.match(source,/\[state\.products, state\.shipping\]/);
  assert.match(source,/action: "product_freight_view", state: freightSource/);
  assert.match(source,/nativeView\?\.source === freightSource/);
  assert.match(rust,/"product_freight_view" =>/);
});
test("no false all-clear while native issues have not finished",()=>{
  const source=read("src/StaffChecks.tsx");
  assert.match(source,/!error && !loading && !actions\.length/);
  assert.match(source,/aria-busy=\{loading\}/);
});
