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

  const cases=[
    {name:"김가상",model:"709 2개",address:"경기도 의정부시 시민로 100",
      phone:"010-1234-5678",freight:"4500"},
    {name:"박가상",model:"709 1개",address:"서울특별시 마포구 월드컵로 20",
      phone:"010-2345-6789",freight:"3500"}
  ];
  for(const [index,info] of cases.entries()){
    if(index) {
      await page.getByRole("button",{name:/새 주문/}).first().click();
      // Do not paste the second source into the still-mounted first receipt.
      await page.waitForFunction(name=>
        !(document.querySelector(".source-document")?.textContent||"").includes(name),
        cases[0].name,{timeout:20000});
    }
    const raw=[info.name,info.phone,info.address,info.model].join("\n");
    await page.evaluate(text=>window.bando.copy(text),raw);
    await page.getByRole("button",{name:"대리발송",exact:true}).click();
    await page.getByRole("button",{name:"거래처 찾기"}).click();
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
    const group=page.locator(".workflow-group").filter({
      has:page.getByRole("button",{name:"배송 건 "+info.name})
    }).first();
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
          whitespace:innerStyle?.whiteSpace,
          cardBorder:parseFloat(style.borderTopWidth),
          cardMargin:parseFloat(style.marginBottom),
        };
      }));
    assert.equal(compact.length,2);
    assert.ok(compact.every(card =>
      /^배송 \d+건, 입금액 .+ \(부가세 .+\)$/.test(card.text) &&
      !card.text.includes("총 운임") &&
      card.summaryChildren===1 &&
      card.whitespace==="nowrap" &&
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
      const group=page.locator(".workflow-group").filter({
        has:page.getByRole("button",{name:"배송 건 "+info.name})
      }).first();
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
