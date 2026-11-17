// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";

export async function run(page,{record,snap,paste,chooseDealer}){
  await paste(page,"김가상");
  await chooseDealer(page);
  await page.waitForFunction(()=>[...document.querySelectorAll("button.item-amount")]
    .some(e=>e.textContent?.includes("84,000")),null,{timeout:30000});
  await sleep(4200);
  const actual=await page.evaluate(async()=>({
    amounts:[...document.querySelectorAll("button.item-amount")].map(e=>e.textContent?.trim()),
    orders:(await window.bando.load()).orders.length
  }));
  assert.ok(actual.amounts.some(x=>x?.includes("84,000")),"Retail price not retained");
  assert.equal(actual.orders,0,"Price preview unexpectedly saved an order");
  record("PRICING_RUST_LINE_STABLE",actual);
  await snap(page,"price");
}

