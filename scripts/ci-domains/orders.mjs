// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";

// This isolated suite tests draft switching directly. Durable confirmations
// and factory/price stages are verified by the separate full workflow suite.
export async function run(page,{record,snap,paste,chooseDealer}){
  await paste(page,"기존가상수취인","709 1개","가상대리점");
  await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
    .waitFor({state:"visible",timeout:30000});
  await page.getByRole("button",{name:"배송 건 기존가상수취인"})
    .waitFor({state:"visible",timeout:30000});
  const initial=await page.evaluate(async()=>({
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim(),
    queued:document.querySelectorAll(".workflow-group").length,
    stored:(await window.bando.load()).orders.length,
  }));
  assert.equal(initial.dealer,"가상대리점");
  assert.ok(initial.queued>=1);
  assert.equal(initial.stored,0);
  record("ORDERS_INITIAL_ASSIGNED_DRAFT_IN_BOARD",initial);

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
  // The remaining unassigned receipt must still be selectable even AFTER
  // the first pending receipt has gained its own valid shipment/group.
  await page.locator(".workflow-unassigned-draft")
    .filter({hasText:"미선택가상을"}).click();
  await page.waitForFunction(()=>
    (document.querySelector(".source-document")?.textContent??"").includes("미선택가상을") &&
    !document.querySelector(".receipt-client"),null,{timeout:30000});
  await page.waitForTimeout(1200);
  const remaining=await page.evaluate(()=>({
    raw:document.querySelector(".source-document")?.textContent??"",
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim()??null,
    pending:document.querySelectorAll(".workflow-unassigned-draft").length
  }));
  assert.ok(remaining.raw.includes("미선택가상을") && remaining.dealer===null,
    "P0_REMAINING_UNASSIGNED_RECEPTION_FORCED_TO_PREVIOUS_CLIENT "+JSON.stringify(remaining));
  assert.equal(remaining.pending,1);
  record("ORDERS_REMAINING_UNASSIGNED_AFTER_FIRST_ASSIGNED",{
    recipient:"미선택가상을",remainsWithoutDealer:true
  });
  assert.equal((await page.evaluate(async()=>window.bando.load())).orders.length,0,
    "Switching unsaved receipts must not write new orders to SQLite");
  await snap(page,"orders-unassigned-selection");
}

