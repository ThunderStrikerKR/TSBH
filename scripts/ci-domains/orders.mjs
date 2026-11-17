import { selectGroupForRecipient } from "../migration31-real-two-orders.mjs";
// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";

// This isolated suite tests draft switching directly. Durable confirmations
// and factory/price stages are verified by the separate full workflow suite.
export async function run(page,{record,snap,paste,chooseDealer}){
  await paste(page,"김가상","709 1개","가상대리점");
  await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
    .waitFor({state:"visible",timeout:30000});
  await selectGroupForRecipient(page,"김가상");
  const initial=await page.evaluate(async()=>({
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim(),
    queued:document.querySelectorAll(".workflow-group").length,
    stored:(await window.bando.load()).orders.length,
  }));
  assert.equal(initial.dealer,"가상대리점");
  assert.ok(initial.queued>=1);
  assert.equal(initial.stored,0);
  record("ORDERS_INITIAL_ASSIGNED_DRAFT_IN_BOARD",initial);

  // Regression: a prior dealer-assigned draft still has a valid shipment
  // on the board. Two new unassigned receipts must not inherit that dealer
  // when the user explicitly clicks their own pending cards.
  for (const recipient of ["정가상", "박가상"]) {
    await page.getByRole("button",{name:/새 주문/}).first().click();
    await paste(page,recipient,"709 1개");
    await page.getByRole("button",{name:"거래처 찾기",exact:true})
      .waitFor({state:"visible",timeout:30000});
  }
  await page.getByRole("button",{name:/새 주문/}).first().click();
  await paste(page,"이가상","709 1개","가상대리점");
  await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
    .waitFor({state:"visible",timeout:30000});
  assert.equal(await page.locator(".workflow-unassigned-draft").count(),2,
    "Two independent unassigned receipt cards must remain visible");
  for (const recipient of ["정가상", "박가상", "정가상"]) {
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
  await page.waitForFunction(()=>
    document.querySelectorAll(".workflow-group").length>=3,
    null,{timeout:30000});
  await selectGroupForRecipient(page,"정가상");
  await page.waitForTimeout(900);
  const assigned=await page.evaluate(()=>({
    raw:document.querySelector(".source-document")?.textContent??"",
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim()??null,
    pending:document.querySelectorAll(".workflow-unassigned-draft").length,
  }));
  assert.ok(assigned.raw.includes("정가상"),
    "Assigning a dealer must not switch back to an unrelated order");
  assert.equal(assigned.dealer,"가상소매점");
  assert.equal(assigned.pending,1);
  record("ORDERS_RETAINED_UNASSIGNED_ASSIGNMENT",{
    recipient:"정가상",dealer:assigned.dealer,pending:assigned.pending
  });
  // The remaining unassigned receipt must still be selectable even AFTER
  // the first pending receipt has gained its own valid shipment/group.
  await page.locator(".workflow-unassigned-draft")
    .filter({hasText:"박가상"}).click();
  await page.waitForFunction(()=>
    (document.querySelector(".source-document")?.textContent??"").includes("박가상") &&
    !document.querySelector(".receipt-client"),null,{timeout:30000});
  await page.waitForTimeout(1200);
  const remaining=await page.evaluate(()=>({
    raw:document.querySelector(".source-document")?.textContent??"",
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim()??null,
    pending:document.querySelectorAll(".workflow-unassigned-draft").length
  }));
  assert.ok(remaining.raw.includes("박가상") && remaining.dealer===null,
    "P0_REMAINING_UNASSIGNED_RECEPTION_FORCED_TO_PREVIOUS_CLIENT "+JSON.stringify(remaining));
  assert.equal(remaining.pending,1);
  record("ORDERS_REMAINING_UNASSIGNED_AFTER_FIRST_ASSIGNED",{
    recipient:"박가상",remainsWithoutDealer:true
  });
  // The whole TITLE CELL (not the group body or management controls) is
  // the accessible accordion target. Only one selected group can expand.
  const headingButtons=page.locator(".workflow-group .flow-heading-toggle");
  assert.ok((await headingButtons.count()) >= 2);
  await headingButtons.first().click();
  await page.waitForFunction(()=>
    [...document.querySelectorAll(".workflow-group .flow-heading-toggle")]
      .filter(el=>el.getAttribute("aria-expanded")==="true").length===1 &&
    document.querySelectorAll(".workflow-group .workflow-shipment-list").length===1,
    null,{timeout:30000});
  await headingButtons.nth(1).click();
  await page.waitForFunction(()=>
    [...document.querySelectorAll(".workflow-group .flow-heading-toggle")]
      .filter(el=>el.getAttribute("aria-expanded")==="true").length===1 &&
    document.querySelectorAll(".workflow-group .workflow-shipment-list").length===1 &&
    [...document.querySelectorAll(".workflow-group .flow-heading-toggle")][1]
      ?.getAttribute("aria-expanded")==="true",
    null,{timeout:30000});
  await headingButtons.nth(1).click();
  await page.waitForFunction(()=>
    [...document.querySelectorAll(".workflow-group .flow-heading-toggle")]
      .every(el=>el.getAttribute("aria-expanded")==="false"),
    null,{timeout:30000});
  record("ORDERS_SINGLE_OPEN_TITLE_CELL_ACCORDION",{groupBodyNotClickable:true,maxOpen:1});
  // Return to the unfinished receipt before exercising the tab-leave policy.
  await page.locator(".workflow-unassigned-draft")
    .filter({hasText:"박가상"}).click();
  await page.waitForFunction(()=>
    (document.querySelector(".source-document")?.textContent??"").includes("박가상"),
    null,{timeout:30000});
  assert.equal((await page.evaluate(async()=>window.bando.load())).orders.length,0,
    "Switching unsaved receipts must not write new orders to SQLite");
  await snap(page,"orders-unassigned-selection");
  await page.getByRole("button",{name:"주문 내역",exact:true}).click();
  const leaveModal=page.getByRole("dialog",{name:"미완성 발주서 처리"});
  await leaveModal.waitFor({state:"visible",timeout:30000});
  const leaveText=await leaveModal.textContent();
  assert.match(leaveText??"",/미완성 발주서 [1-9]\d*건/);
  assert.match(leaveText??"",/주문 확인 완료 전에는 주문 내역에 저장되지 않으며/);
  // One choice covers both retained dealer-assigned drafts and the single
  // unassigned draft. A "save" label here would promise forbidden SQLite work.
  const count=Number(leaveText?.match(/미완성 발주서 (\d+)건/)?.[1]);
  assert.equal(count,4,"Every one of the four unfinished receptions must be counted");
  assert.equal(await leaveModal.getByRole("button",{name:"저장",exact:true}).count(),0);
  await leaveModal.getByRole("button",{name:"임시 보관하고 이동"}).click();
  await page.getByRole("button",{name:"주문 내역",exact:true})
    .waitFor({state:"attached"});
  await page.waitForFunction(()=>
    document.querySelector('nav button[aria-current="page"]')?.textContent?.trim()==="주문 내역",
    null,{timeout:30000});
  await page.getByRole("button",{name:"접수",exact:true}).click();
  await page.locator(".workflow-unassigned-draft")
    .filter({hasText:"박가상"}).waitFor({state:"visible",timeout:30000});
  assert.equal((await page.evaluate(async()=>window.bando.load())).orders.length,0);
  record("ORDERS_NAVIGATE_KEEP_ALL_TEMPORARY",{count,persisted:0});
  await page.getByRole("button",{name:"주문 내역",exact:true}).click();
  await leaveModal.waitFor({state:"visible",timeout:30000});
  await leaveModal.getByRole("button",{name:"모두 버리고 이동"}).click();
  await page.waitForFunction(()=>
    document.querySelector('nav button[aria-current="page"]')?.textContent?.trim()==="주문 내역",
    null,{timeout:30000});
  await page.getByRole("button",{name:"접수",exact:true}).click();
  await page.waitForFunction(()=>
    !document.querySelector(".workflow-unassigned-draft") &&
    !document.querySelector(".workflow-group"),null,{timeout:30000});
  assert.equal((await page.evaluate(async()=>window.bando.load())).orders.length,0);
  record("ORDERS_NAVIGATE_DISCARD_ALL_TEMPORARY",{remaining:0,persisted:0});
}

