// Real Windows Tauri EXE + WebView2 test: confirm TWO independent orders.
// Synthetic dealer/account are provisioned by the preceding smoke test.
// GitHub Actions disposable developer TEST SQLite only.
import assert from "node:assert/strict";

export async function probeTwoConsecutiveOrders(page,{record,snap}) {
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
    if(index)await page.getByRole("button",{name:/새 주문/}).first().click();
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
