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

  // Regression for a real report: the board has completed shipments from
  // prior orders and two unsaved unassigned receipts. Clicking either receipt
  // must not silently select the first shipment and its existing dealer.
  for (const recipient of ["미선택가상갑", "미선택가상을"]) {
    await page.getByRole("button",{name:/새 주문/}).first().click();
    await paste(page,recipient,"709 1개");
    await page.getByRole("button",{name:"거래처 찾기",exact:true})
      .waitFor({state:"visible",timeout:30000});
  }
  await page.getByRole("button",{name:/새 주문/}).first().click();
  await paste(page,"지정가상병","709 1개","가상대리점");
  await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
    .waitFor({state:"visible",timeout:30000});
  assert.equal(await page.locator(".workflow-unassigned-draft").count(),2,
    "Two independent unassigned receipt cards must remain visible");
  for (const recipient of ["미선택가상갑", "미선택가상을", "미선택가상갑"]) {
    await page.locator(".workflow-unassigned-draft")
      .filter({hasText:recipient}).click();
    await page.waitForFunction(name=>
      (document.querySelector(".source-document")?.textContent||"").includes(name) &&
      !!document.querySelector(".source-client-actions") &&
      !document.querySelector(".receipt-client"),
      recipient,{timeout:30000});
    // Wait through native preview settling: previous code would switch back
    // to the first persisted shipment/dealer after the initial click.
    await page.waitForTimeout(1300);
    const observed=await page.evaluate(()=>({
      raw:document.querySelector(".source-document")?.textContent??"",
      dealer:document.querySelector(".receipt-client b")?.textContent?.trim()??null,
      pending:document.querySelectorAll(".workflow-unassigned-draft").length,
    }));
    assert.ok(observed.raw.includes(recipient),
      "P0_UNASSIGNED_RECEIPT_REPLACED_BY_SHIPMENT "+JSON.stringify(observed));
    assert.equal(observed.dealer,null,
      "P0_UNASSIGNED_RECEIPT_INHERITED_OTHER_DEALER "+recipient);
    assert.equal(observed.pending,2);
    record("ORDERS_UNASSIGNED_EXPLICIT_SELECTION_RETAINED",{
      recipient,otherDealerNotInherited:true,unassignedCount:observed.pending
    });
  }
  // Explicit dealer assignment to a retained receipt must keep that order
  // selected when its first Rust shipment is created.
  await chooseDealer(page,"가상소매점");
  await page.waitForFunction(name=>
    !![...document.querySelectorAll(".workflow-shipment")]
      .find(el=>el.getAttribute("aria-label")==="배송 건 "+name),
    "미선택가상갑",{timeout:30000});
  await page.waitForTimeout(900);
  const assigned=await page.evaluate(()=>({
    raw:document.querySelector(".source-document")?.textContent??"",
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim()??null,
    pending:document.querySelectorAll(".workflow-unassigned-draft").length,
  }));
  assert.ok(assigned.raw.includes("미선택가상갑"),
    "Assigning a dealer must not switch back to an unrelated order");
  assert.equal(assigned.dealer,"가상소매점");
  assert.equal(assigned.pending,1);
  record("ORDERS_RETAINED_UNASSIGNED_ASSIGNMENT",{
    recipient:"미선택가상갑",dealer:assigned.dealer,pending:assigned.pending
  });
  assert.equal((await page.evaluate(async()=>window.bando.load())).orders.length,2,
    "Switching unsaved receipts must not write new orders to SQLite");
  await snap(page,"orders-unassigned-selection");
}

