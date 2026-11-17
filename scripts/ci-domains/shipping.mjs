import { selectGroupForRecipient } from "../migration31-real-two-orders.mjs";
// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";

export async function run(page,{record,snap,paste,chooseDealer}){
  await paste(page,"김가상");
  await chooseDealer(page);
  const money=page.locator('.items-freight-panel .freight-money input[aria-label^="김가상"]:not([disabled])').first();
  await money.waitFor({state:"visible",timeout:45000});
  await money.click();await money.press("ControlOrMeta+A");await money.press("Backspace");
  await page.keyboard.type("4500",{delay:70});await money.blur();
  await page.waitForFunction(()=>
    [...document.querySelectorAll(".flow-fixed-summary")]
      .some(e=>e.textContent?.includes("88,500")),null,{timeout:30000});
  assert.equal((await money.inputValue()).replaceAll(",",""),"4500");
  const group=await selectGroupForRecipient(page,"김가상");
  await group.getByRole("button",{name:"주문 확인 완료",exact:true}).click();
  await page.waitForFunction(async()=>(await window.bando.load()).orders.length===1,
    null,{timeout:30000});
  const stored=await page.evaluate(async()=>{
    const state=await window.bando.load();
    return {count:state.orders.length,confirmed:!!state.orders[0]?.confirmedAt,
      raw:state.orders[0]?.raw};
  });
  assert.equal(stored.count,1);
  assert.ok(stored.confirmed);
  record("SHIPPING_4500_RUST_TOTAL_AND_SQLITE_CONFIRM",stored);
  await snap(page,"shipping");
}

