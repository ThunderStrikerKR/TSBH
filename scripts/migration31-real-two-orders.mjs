// An unselected order group owns no rendered shipment buttons until its
// clickable TITLE CELL is expanded. Tests must follow the real user flow
// rather than searching for hidden shipment children.
export async function selectGroupForRecipient(page, recipient) {
  const groups=page.locator(".workflow-group");
  const labelled="배송 건 "+recipient;
  for(let i=0;i<await groups.count();i++){
    const group=groups.nth(i);
    const button=group.getByRole("button",{name:labelled,exact:true});
    if(await button.count())return group;
    const title=group.locator(".flow-heading-toggle");
    if(await title.getAttribute("aria-expanded")!=="true"){
      await title.click();
      await page.waitForFunction(index=>
        document.querySelectorAll(".workflow-group .flow-heading-toggle")[index]
          ?.getAttribute("aria-expanded")==="true",
        i,{timeout:20000});
    }
    if(await button.count())return group;
  }
  throw Error("Unable to open an order group for recipient: "+recipient);
}

// The empty intake panel now contains instructional text and buttons.
 // Assert the actual empty-receipt state, not an empty DOM textContent.
async function waitForFreshReceipt(page,{record,snap}){
  try {
    await page.waitForFunction(()=>{
      const source=document.querySelector(".source-document.source-awaiting-input");
      const top=document.querySelector(".source-top");
      return !!source && !!top && !!source.querySelector(".source-intake-choice") &&
        !top.querySelector(".receipt-client,.source-client-actions");
    },null,{timeout:20000});
    await page.getByRole("button",{name:"대리발송",exact:true}).waitFor({state:"visible",timeout:20000});
  } catch (error) {
    const details=await page.evaluate(async()=>({
      sourceClass:document.querySelector(".source-document")?.className,
      sourceText:document.querySelector(".source-document")?.textContent?.slice(0,300),
      clientText:document.querySelector(".source-top")?.textContent?.slice(0,200),
      alerts:[...document.querySelectorAll('[role="alert"],[role="status"]')]
        .map(node=>node.textContent?.slice(0,200)),
      dialogs:[...document.querySelectorAll('[role="dialog"]')]
        .map(node=>node.textContent?.slice(0,200)),
      pending:(await window.bando.workingReceptionsLoad().catch(()=>[]))
        .map(order=>({id:order.id,clientId:order.clientId,rawLength:order.raw?.length}))
    }));
    record("NEW_RECEPTION_FOCUS_DIAGNOSTIC",details);
    await snap(page,"new-reception-focus-failed");
    throw error;
  }
}

// Real Windows Tauri EXE + WebView2 test: confirm TWO independent orders.
// Synthetic dealer/account are provisioned by the preceding smoke test.
// GitHub Actions disposable developer TEST SQLite only.
import assert from "node:assert/strict";

export async function probeTwoConsecutiveOrders(page,{record,snap},{holdBoth=false}={}) {
  assert.equal(process.platform,"win32");
  assert.equal(process.env.GITHUB_ACTIONS,"true");
  const prep=await page.evaluate(async()=>{
    const api=window.bando;
    if(!api || !await api.workspaceMode())throw Error("TEST DB required");
    const state=await api.load();
    if(state.orders?.length)throw Error("Expected no saved orders at start");
    if(!state.clients?.some(c=>c.id==="sim31-dealer"))
      throw Error("Synthetic test dealer not provisioned");
    const prices=new Map([
      ["BD 709 / 블랙 / 타공 상판",42000],
      ["BD GT-1 / 길이 14cm / 평평한 벽용",10000],
    ]);
    let count=0;
    const next=structuredClone(state);
    next.products=next.products.map(product=>{
      const wholesale=prices.get(product.name);
      if(wholesale===undefined)return product;
      count++;
      return {...product,variants:product.variants.map(v=>({...v,wholesale}))};
    });
    if(count!==2)throw Error("Synthetic SKU prices unavailable");
    const saved=await api.save(next);
    return {revision:saved.revision,productsPatched:count,orders:saved.orders.length};
  });
  record("SIM31_MULTIPLE_SETUP synthetic prices in disposable DB",prep);
  await page.reload();
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({timeout:60000});
  await page.getByRole("button",{name:"접수",exact:true}).click();
  // A reload may restore the previously inspected unconfirmed receipt.
  // The independent two-order scenario must explicitly open a fresh one.
  await page.getByRole("button",{name:/새 주문/}).first().click();
  await waitForFreshReceipt(page,{record,snap});

  const cases=[
    {name:"김가상",model:"709 2개",address:"경기도 의정부시 시민로 100",
      phone:"010-1234-5678",freight:"4500"},
    {name:"박가상",model:"709 1개",address:"서울특별시 마포구 월드컵로 20",
      phone:"010-2345-6789",freight:"3500"}
  ];
  for(const [index,info] of cases.entries()){
    if(index) {
      await page.getByRole("button",{name:/새 주문/}).first().click();
      // New reception must start without an inherited dealer. If a previous
      // dealer remains selected, this is a real regression, not a reason to
      // click the Change button and hide it.
      // A missing/remounting SourceDocument must NEVER satisfy the fresh
      // draft check (the older "!text.includes(name)" check could pass when
      // the source element was temporarily absent).
      await waitForFreshReceipt(page,{record,snap});
      assert.equal(await page.locator(".receipt-client").count(),0,
        "NEW_ORDER_INHERITED_CLIENT before second clipboard paste");
      record("SIM33_NEW_RECEPTION_INDEPENDENT_BEFORE_PASTE",{
        previousRecipient:cases[0].name,nextRecipient:info.name,
        inheritedDealer:false
      });
    }
    const raw=[info.name,info.phone,info.address,info.model].join("\n");
    await page.evaluate(text=>window.bando.copy(text),raw);
    const clipboard=await page.evaluate(()=>window.bando.readClipboard());
    assert.equal(clipboard,raw,"Native Windows clipboard changed before reception paste");
    await page.getByRole("button",{name:"대리발송",exact:true}).click();
    try {
      await page.waitForFunction(name=>
        (document.querySelector(".source-document")?.textContent||"").includes(name),
        info.name,{timeout:30000});
    } catch (error) {
      const diagnostic=await page.evaluate(async()=>({
        sourceVisible:!!document.querySelector(".source-document"),
        sourceText:document.querySelector(".source-document")?.textContent?.slice(0,350),
        client:document.querySelector(".receipt-client")?.textContent?.trim(),
        pendingButtons:[...document.querySelectorAll(".source-heading button")]
          .map(b=>({label:b.textContent?.trim(),disabled:b.disabled})),
        stateOrderCount:(await window.bando.load()).orders.length,
        clipboard:(await window.bando.readClipboard()).slice(0,300),
        inlineStatus:document.querySelector(".source-inline-status")?.textContent?.trim(),
        modalTitles:[...document.querySelectorAll('[role="dialog"]')].map(e=>e.getAttribute("aria-label")||e.textContent?.slice(0,90)),
      }));
      record("SIM33_PASTE_AFTER_NEW_ORDER_DIAGNOSTIC",diagnostic);
      await snap(page,"33-paste-after-new-order-failure");
      throw Error("NEW_RECEPTION_PASTE_NOT_APPLIED "+JSON.stringify(diagnostic)+
        " cause="+String(error));
    }
    // These receipts have no explicit dealer marker, and no Kakao chat in CI.
    // Never silently "repair" a leaked dealer by clicking '변경'.
    const noDealer = page.getByRole("button",{name:"거래처 찾기",exact:true});
    await noDealer.waitFor({state:"visible",timeout:15000});
    assert.equal(await page.locator(".receipt-client").count(),0,
      "UNEXPECTED_AUTO_CLIENT_SELECTION for "+info.name);
    await noDealer.click();
    const dealer=page.locator(".client-chooser-list .client-result")
      .filter({hasText:"가상대리점"});
    await dealer.waitFor({state:"visible",timeout:25000});
    await dealer.click();
    await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
      .waitFor({state:"visible",timeout:30000});
    const amount=page.locator('.items-freight-panel .freight-money input[aria-label^="'+info.name+'"]:not([disabled])').first();
    await amount.waitFor({state:"visible",timeout:45000});
    await amount.click();
    await amount.press("ControlOrMeta+A");
    await amount.press("Backspace");
    await page.keyboard.type(info.freight,{delay:90});
    assert.equal((await amount.inputValue()).replaceAll(",",""),info.freight);
    if (holdBoth) {
      // The local freight editor flushes on blur or a short pause, and Rust
      // must finish its pending edit before this order is stashed.
      await amount.blur();
      await page.waitForTimeout(800);
      record("SIM32_PENDING_ORDER_TYPED real Windows GUI",{
        index:index+1,recipient:info.name,freight:info.freight
      });
      continue;
    }
    const group=await selectGroupForRecipient(page,info.name);
    const confirm=group.getByRole("button",{name:"주문 확인 완료",exact:true});
    await confirm.waitFor({state:"visible",timeout:45000});
    await confirm.click();
    try {
      await group.getByRole("button",{name:"운임 적용",exact:true})
        .waitFor({state:"visible",timeout:25000});
      await page.waitForFunction(async count=>
        (await window.bando.load()).orders?.length===count,
        index+1,{timeout:30000});
    }catch(error) {
      const diagnostic=await page.evaluate(async()=>({
        alerts:[...document.querySelectorAll('[role="alert"],[role="status"]')]
          .map(n=>n.textContent?.slice(0,350)),
        steps:[...document.querySelectorAll(".workflow-group .workflow-progress button.primary")]
          .map(b=>({label:b.getAttribute("aria-label"),disabled:b.disabled})),
        storedOrders:(await window.bando.load()).orders.map(o=>({
          id:o.id,recipients:o.recipients.map(r=>r.id)
        }))
      }));
      throw Error("SIM31_MULTIPLE_GUI_STOP "+JSON.stringify({index,diagnostic})+
        " cause="+String(error));
    }
    record("SIM31_MULTIPLE_ORDER_CONFIRMED real Windows GUI",
      {index:index+1,recipient:info.name,freight:info.freight});
  }
  if (holdBoth) {
    await page.waitForFunction(() => document.querySelectorAll(".workflow-group").length===2,
      null,{timeout:30000});
    const pending = await page.evaluate(async()=>({
      cards:[...document.querySelectorAll(".workflow-group")].map(el=>({
        label:el.querySelector(".flow-group-name")?.textContent?.trim(),
        height:el.getBoundingClientRect().height,
      })),
      saved:(await window.bando.load()).orders.length,
      review:document.querySelector(".review-layout")?.getBoundingClientRect().height,
      staff:document.querySelector(".staff-checks")?.getBoundingClientRect().height,
    }));
    assert.equal(pending.saved,0,"Two unconfirmed receipts must not be silently committed");
    assert.equal(pending.cards.length,2,"Both receipts must be displayed simultaneously");
    assert.ok(pending.cards.every(card=>card.label?.includes("가상대리점")),
      "Both receipts must retain their own client identity");
    assert.ok(pending.cards.every(card=>!card.label?.includes("미확정 · 임시")),
      "Redundant draft status must not consume another header line");
    const compact = await page.evaluate(() =>
      [...document.querySelectorAll(".workflow-group")].map(group => {
        const sum = group.querySelector(".flow-fixed-summary");
        const text = sum?.textContent?.replace(/\s+/g," ").trim() ?? "";
        const summaryStyle = sum && getComputedStyle(sum);
        const innerStyle = sum?.firstElementChild &&
          getComputedStyle(sum.firstElementChild);
        const style=getComputedStyle(group);
        return {
          text,
          summaryChildren:sum?.children.length ?? 0,
          rightAligned:!!sum && !!sum.querySelector(".flow-summary-payment") &&
            Math.abs(sum.querySelector(".flow-summary-payment").getBoundingClientRect().right -
              sum.getBoundingClientRect().right) <= 8,
          whitespace:innerStyle?.whiteSpace,
          cardBorder:parseFloat(style.borderTopWidth),
          cardMargin:parseFloat(style.marginBottom),
        };
      }));
    assert.equal(compact.length,2);
    assert.ok(compact.every(card =>
      /^배송 \d+건(?:\s*\(부가세 포함\)\s*)?[\d,]+원$/.test(card.text) &&
      !card.text.includes("입금액") && !card.text.includes("부가세 별도") &&
      card.summaryChildren===1 &&
      card.whitespace==="nowrap" &&
      card.rightAligned &&
      card.cardBorder>=1 && card.cardMargin>=6),
      "SIM32_COMPACT_CARD_FORMAT "+JSON.stringify(compact));
    record("SIM32_COMPACT_WORKFLOW_CARDS actual WebView2 1-line summary and distinct order borders",compact);
    record("SIM32_TWO_PENDING_ORDERS real WebView2 processing cards",pending);
    await snap(page,"08-sim32-two-unconfirmed-processing-cards");
    // Measure real WebView2 panel height during an actual freight edit.
    // The old status paragraph and conditional VAT footer caused resize pulses.
    await page.evaluate(() => {
      const selectors=[".staff-checks",".recipient-body",".review-pane"];
      window.__sim32Sizes=[];
      const sample=()=>window.__sim32Sizes.push(Object.fromEntries(selectors.map(selector=>[
        selector,document.querySelector(selector)?.getBoundingClientRect().height??null
      ])));
      const observer=new ResizeObserver(sample);
      for (const selector of selectors) {
        const el=document.querySelector(selector);
        if(el)observer.observe(el);
      }
      window.__sim32SizeObserver=observer;
      sample();
    });
    const edit=page.locator(".items-freight-panel .freight-money input:not([disabled])").first();
    await edit.click();
    await edit.press("ControlOrMeta+A");
    await page.keyboard.type("4600",{delay:65});
    assert.equal((await edit.inputValue()).replaceAll(",",""),"4600");
    await edit.blur();
    await page.waitForTimeout(1200);
    const sizes=await page.evaluate(()=>{
      window.__sim32SizeObserver?.disconnect();
      return window.__sim32Sizes??[];
    });
    const ranges=Object.fromEntries([".staff-checks",".recipient-body",".review-pane"].map(key=>{
      const h=sizes.map(row=>row[key]).filter(Number.isFinite);
      return [key, h.length ? Math.max(...h)-Math.min(...h) : -1];
    }));
    assert.ok(Object.values(ranges).every(v=>v>=0&&v<=3),
      "SIM32_RESIZE_PULSE "+JSON.stringify({ranges,samples:sizes.length}));
    record("SIM32_WEBVIEW2_PANEL_HEIGHT_STABLE during native freight update",{
      ranges,samples:sizes.length
    });

    for (const index of [1,0]) {
      const info=cases[index];
      const group=await selectGroupForRecipient(page,info.name);
      const confirm=group.getByRole("button",{name:"주문 확인 완료",exact:true});
      await confirm.waitFor({state:"visible",timeout:45000});
      await confirm.click();
      await group.getByRole("button",{name:"운임 적용",exact:true})
        .waitFor({state:"visible",timeout:30000});
      await page.waitForFunction(async count=>
        (await window.bando.load()).orders?.length===count,
        2-index,{timeout:30000});
      record("SIM32_PENDING_ORDER_CONFIRMED real Windows GUI",{
        index:index+1,recipient:info.name,
      });
    }
  }
  const verified=await page.evaluate(async()=>{
    const saved=await window.bando.load();
    return {orders:saved.orders.map(o=>({
      id:o.id,confirmedAt:o.confirmedAt,
      recipients:o.recipients.map(r=>({id:r.id,items:r.items.map(i=>i.id)}))
    }))};
  });
  assert.equal(verified.orders.length,2);
  const a=verified.orders[0],b=verified.orders[1];
  assert.ok(a.confirmedAt&&b.confirmedAt);
  assert.notEqual(a.recipients[0].id,b.recipients[0].id,
    "SIM31_MULTIPLE separate orders must never share recipient ID");
  assert.notEqual(a.recipients[0].items[0],b.recipients[0].items[0],
    "SIM31_MULTIPLE separate orders must never share item ID");
  record("SIM31_MULTIPLE_ORDER_SQLITE two GUI confirmations persisted",{verified});
  await snap(page,"09-sim31-two-separate-orders-confirmed");
}


/**
 * P0/P1: Production Rust matching evidence + GUI client identity + SQLite
 * snapshots across A -> B -> A. Never mutate the business state via IPC:
 * only fixture provisioning elsewhere uses native state_save. All customer
 * choices and order confirmation in this probe occur through actual WebView2.
 */
export async function probeCrossDealerOrderIsolation(page,{record,snap}) {
  assert.equal(process.platform,"win32");
  assert.equal(process.env.GITHUB_ACTIONS,"true");
  assert.equal(await page.evaluate(()=>window.bando.workspaceMode()),true);
  const initial=await page.evaluate(async()=>window.bando.load());
  assert.equal(initial.orders.length,2,"Expected two original confirmed orders");
  const scenarios=[
    {recipient:"정가상",dealerId:"sim31-dealer",dealerName:"가상대리점",
      origin:"RAW_EXPLICIT",marker:"거래처: 가상대리점",freight:"4000"},
    {recipient:"이가상",dealerId:"sim32-retail",dealerName:"가상소매점",
      origin:"RAW_EXPLICIT",marker:"거래처: 가상소매점",freight:"4100"},
    {recipient:"최가상",dealerId:"sim31-dealer",dealerName:"가상대리점",
      origin:"NONE",marker:"배송지메모: 가상소매점",freight:"4200"}
  ];
  const observed=[];
  for(const [index,scenario] of scenarios.entries()){
    const before=await page.evaluate(async()=>{
      const s=await window.bando.load();
      return s.orders.map(o=>({id:o.id,clientId:o.clientId,confirmedAt:o.confirmedAt}));
    });
    await page.getByRole("button",{name:/새 주문/}).first().click();
    // Before pasting, no previous order/client may remain active.
    await waitForFreshReceipt(page,{record,snap});
    assert.equal(await page.locator(".receipt-client").count(),0,
      "P1_NEW_ORDER_STALE_DEALER "+scenario.recipient);
    const chooserBefore=page.getByRole("button",{name:"거래처 찾기",exact:true});
    // No raw input yet -> chooser is intentionally not visible.
    assert.equal(await chooserBefore.count(),0,
      "P1_NEW_ORDER_UNEXPECTED_SOURCE "+scenario.recipient);
    const raw=[scenario.marker,scenario.recipient,
      "010-7000-"+String(1000+index),
      "서울특별시 마포구 월드컵로 "+(30+index),
      "709 1개"].join("\n");
    // Read-only Rust oracle establishes WHAT matching evidence exists.
    // A recipient named after a dealer or a shipping memo does not qualify.
    const evidence=await page.evaluate(async text=>{
      const state=await window.bando.load();
      return window.__TAURI_INTERNALS__.invoke("business_core_call",{
        domain:"catalog_clients",operation:"auto_match_reception_client",
        payload:{state,windows:[],raw:text,activeChatHandle:""}
      });
    },raw);
    assert.equal(evidence.source,scenario.origin,
      "P0_NATIVE_MATCH_PROVENANCE "+scenario.recipient);
    assert.equal(evidence.clientId,scenario.origin==="NONE"?null:scenario.dealerId,
      "P0_NATIVE_MATCH_ID "+scenario.recipient);
    await page.evaluate(text=>window.bando.copy(text),raw);
    await page.getByRole("button",{name:"대리발송",exact:true}).click();
    await page.waitForFunction(name=>
      (document.querySelector(".source-document")?.textContent||"").includes(name),
      scenario.recipient,{timeout:30000});
    if(scenario.origin==="NONE"){
      const choose=page.getByRole("button",{name:"거래처 찾기",exact:true});
      await choose.waitFor({state:"visible",timeout:20000});
      assert.equal(await page.locator(".receipt-client").count(),0,
        "P1_UNEXPECTED_DEALER_AFTER_UNLABELED_PASTE "+scenario.recipient);
      await choose.click();
      await page.locator(".client-chooser-list .client-result")
        .filter({hasText:scenario.dealerName}).click();
    }else{
      // Evidence says Rust must auto-match. Manual selection would mask
      // a broken GUI-to-Rust connection, so it is forbidden in these cases.
      await page.locator(".receipt-client b")
        .filter({hasText:scenario.dealerName})
        .waitFor({state:"visible",timeout:25000});
    }
    const shown=await page.locator(".receipt-client b").innerText();
    assert.equal(shown.trim(),scenario.dealerName,
      "P0_GUI_CLIENT_NAME "+scenario.recipient);
    const freight=page.locator('.items-freight-panel .freight-money input[aria-label^="'+scenario.recipient+'"]:not([disabled])').first();
    await freight.waitFor({state:"visible",timeout:45000});
    await freight.click();
    await freight.press("ControlOrMeta+A");
    await freight.press("Backspace");
    await page.keyboard.type(scenario.freight,{delay:45});
    await freight.blur();
    await page.waitForFunction(count=>
      document.querySelectorAll(".workflow-group").length>=count,
      before.length+1,{timeout:30000});
    const group=await selectGroupForRecipient(page,scenario.recipient);
    await group.getByRole("button",{name:"주문 확인 완료",exact:true})
      .waitFor({state:"visible",timeout:45000});
    await group.getByRole("button",{name:"주문 확인 완료",exact:true}).click();
    try {
      await group.getByRole("button",{name:"운임 적용",exact:true})
        .waitFor({state:"visible",timeout:30000});
    } catch (error) {
      const diagnostic=await page.evaluate(async recipient=>{
        const state=await window.bando.load();
        const cards=[...document.querySelectorAll(".workflow-group")].map(card=>({
          text:card.textContent?.slice(0,750),
          steps:[...card.querySelectorAll(".workflow-progress button")].map(b=>({
            label:b.textContent?.trim(),disabled:b.disabled
          })),
        }));
        return {recipient,orders:state.orders.map(o=>({
          clientId:o.clientId,raw:o.raw?.slice(0,100),confirmed:!!o.confirmedAt
        })),cards,alerts:[...document.querySelectorAll('[role="alert"],[role="status"]')]
          .map(el=>el.textContent?.slice(0,350))};
      },scenario.recipient);
      record("SIM33_CROSS_DEALER_CONFIRMATION_DIAGNOSTIC",diagnostic);
      await snap(page,"33-cross-dealer-confirmation-failure");
      throw Error("CROSS_DEALER_CONFIRMATION_NOT_ADVANCED "+
        JSON.stringify(diagnostic)+" cause="+String(error));
    }
    await page.waitForFunction(async target=>
      (await window.bando.load()).orders.length===target,
      before.length+1,{timeout:30000});
    const after=await page.evaluate(async()=>window.bando.load());
    const created=after.orders.filter(o=>!before.some(b=>b.id===o.id));
    assert.equal(created.length,1,"P0_EXACTLY_ONE_ORDER_CREATED");
    const order=created[0];
    assert.ok(order.id && !before.some(b=>b.id===order.id));
    assert.equal(order.raw,raw,"P0_SAVED_SOURCE_CHANGED "+scenario.recipient);
    assert.equal(order.clientId,scenario.dealerId,
      "P0_SQLITE_CLIENT_ID "+scenario.recipient);
    assert.equal(order.clientSnapshot?.id,scenario.dealerId,
      "P0_SQLITE_CLIENT_SNAPSHOT "+scenario.recipient);
    assert.equal(order.confirmationSnapshot?.client?.id,scenario.dealerId,
      "P0_SQLITE_CONFIRMATION_CLIENT "+scenario.recipient);
    assert.ok(order.confirmedAt,"P0_ORDER_NOT_CONFIRMED "+scenario.recipient);
    for(const old of before){
      const existing=after.orders.find(o=>o.id===old.id);
      assert.equal(existing?.clientId,old.clientId,
        "P0_PREVIOUS_ORDER_CLIENT_MUTATED");
      assert.equal(existing?.confirmedAt,old.confirmedAt,
        "P0_PREVIOUS_ORDER_CONFIRMATION_MUTATED");
    }
    observed.push({recipient:scenario.recipient,orderId:order.id,
      clientId:order.clientId,source:evidence.source,
      recipientId:order.recipients[0]?.id,
      itemId:order.recipients[0]?.items[0]?.id});
    record("SIM33_CROSS_DEALER_ORDER_COMMITTED",observed.at(-1));
  }
  assert.equal(new Set(observed.map(o=>o.orderId)).size,scenarios.length);
  assert.equal(new Set(observed.map(o=>o.recipientId)).size,scenarios.length);
  assert.equal(new Set(observed.map(o=>o.itemId)).size,scenarios.length);
  await snap(page,"33-cross-dealer-a-b-a-confirmed");
  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({timeout:60000});
  const persisted=await page.evaluate(async()=>window.bando.load());
  assert.equal(persisted.orders.length,initial.orders.length+scenarios.length,
    "P0_RELOAD_ORDER_COUNT");
  for(const proof of observed){
    const order=persisted.orders.find(o=>o.id===proof.orderId);
    assert.equal(order?.clientId,proof.clientId,"P0_RELOAD_CLIENT_ID "+proof.recipient);
    assert.equal(order?.clientSnapshot?.id,proof.clientId,
      "P0_RELOAD_CLIENT_SNAPSHOT "+proof.recipient);
    assert.equal(order?.confirmationSnapshot?.client?.id,proof.clientId,
      "P0_RELOAD_CONFIRMATION "+proof.recipient);
  }
  record("SIM33_CROSS_DEALER_SQLITE_RELOAD_PASSED",
    {orderCount:persisted.orders.length,proof:observed});
}
