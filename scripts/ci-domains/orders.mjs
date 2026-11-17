// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";
import { probeTwoConsecutiveOrders } from "../migration31-real-two-orders.mjs";

export async function run(page,{record,snap,paste,chooseDealer}){
  await probeTwoConsecutiveOrders(page,{record,snap},{holdBoth:false});
  const stored=await page.evaluate(async()=>
    (await window.bando.load()).orders.map(o=>({id:o.id,confirmed:!!o.confirmedAt})));
  assert.equal(stored.length,2);
  assert.ok(stored.every(o=>o.confirmed));
  record("ORDERS_TWO_SQLITE_CONFIRMATIONS",{orders:stored});
}

