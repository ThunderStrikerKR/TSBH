// Real Windows development-mode BANDO HUB / WebView2 business workflow test.
// Uses the running DEV_WINDOWS_TAURI.cmd native app (not a release binary).
// Every order, customer, product price, and file lives in CI's synthetic TEST DB.
import assert from "node:assert/strict";
import { chromium } from "playwright";
import { resolve, join } from "node:path";
import { promises as fs } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { probeTwoConsecutiveOrders } from "./migration31-real-two-orders.mjs";
import { probeFactoryReplyNotifications } from "./migration32-real-factory-notifications.mjs";
import { probeWindowsOcrCancel } from "./migration32-real-ocr-cancel.mjs";

assert.equal(process.platform, "win32", "Requires Windows desktop");
assert.equal(process.env.GITHUB_ACTIONS, "true", "Only synthetic isolated CI data allowed");
const root = resolve(process.env.BANDO_DEV_EVIDENCE || "dev-windows-smoke-evidence");
const pid = Number(process.env.BANDO_DEV_NATIVE_PID);
assert.ok(Number.isInteger(pid) && pid > 0);
await fs.mkdir(root, {recursive:true});
const results = { environment:"DEV_WINDOWS_TAURI.cmd", mode:"Tauri debug + Vite + TEST SQLite",
  sourceCommit:process.env.GITHUB_SHA, started:new Date().toISOString(),
  tests:[], failures:[], screenshots:[] };
const save = () => fs.writeFile(join(root,"dev-business-results.json"),JSON.stringify(results,null,2));
const record = (name, data={}) => {
  results.tests.push({name,at:new Date().toISOString(),...data});
  console.log("BANDO_DEV_BUSINESS_CHECK",name,JSON.stringify(data));
};
const snap = async(page,name) => {
  const filename="business-"+name+".png";
  await page.screenshot({path:join(root,filename),animations:"disabled",timeout:20000});
  results.screenshots.push(filename);
  await save();
};
let browser;
try {
  // Tauri dev Windows embeds WebView2 CDP only with CI's explicit
  // BANDO_DEV_GUI_AUDIT flag. This connects to the real native window.
  let target=null;
  for(let i=0;i<120;i++) {
    try {
      const r=await fetch("http://127.0.0.1:19651/json/list",{
        signal:AbortSignal.timeout(700)});
      if(r.ok) {
        const tabs=await r.json();
        target=tabs.find(t=>t.type==="page" && /localhost|tauri/i.test(t.url||""))
           ?? tabs.find(t=>t.type==="page");
        if(target)break;
      }
    }catch {}
    await sleep(500);
  }
  assert.ok(target,"DEV native WebView2 CDP port did not open");
  browser=await chromium.connectOverCDP("http://127.0.0.1:19651",{timeout:20000});
  const context=browser.contexts()[0];
  const page=context?.pages().find(p=>/localhost|tauri/i.test(p.url()))??context?.pages()[0];
  assert.ok(page,"Could not access native WebView2 page");
  page.setDefaultTimeout(30000);
  page.on("pageerror",e=>results.failures.push("js-pageerror: "+e.message));
  page.on("console",m=>{if(m.type()==="error") results.failures.push("console: "+m.text());});
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({timeout:90000});
  const native=await page.evaluate(async()=>({
    href:location.href,workspace:await window.bando.workspaceMode(),
    status:await window.bando.startupStatus(),
    before:await window.bando.load()
  }));
  assert.equal(native.workspace,true,"Running outside isolated TEST database");
  assert.equal(native.status.phase,"READY");
  assert.match(native.href,/127.0.0.1:5173|localhost:5173/,
    "Should be running DEV Vite, not installed Release EXE");
  assert.equal(native.before.orders.length,0,"Nonempty operational orders leaked into TEST");
  assert.equal(native.before.clients.length,0,"Operational clients leaked into TEST");
  record("DEV_NATIVE_STARTED",{
    href:native.href,workspace:true,startup:native.status.phase,
    productCount:native.before.products.length,initialOrders:0,initialClients:0
  });
  await snap(page,"01-real-dev-startup");

  // Test seed only; GUI actions below are performed through real WebView2.
  const seed=await page.evaluate(async()=>{
    const original=await window.bando.load();
    const account={id:"sim31-account",bank:"가상은행",number:"123-456-789",holder:"반도스탠드"};
    const state=structuredClone(original);
    state.settings.accounts=[...(state.settings.accounts??[]),account];
    state.settings.defaultAccountId=account.id;
    const dealers=[
      {id:"sim31-dealer",name:"가상대리점",tier:"wholesale"},
      {id:"sim32-retail",name:"가상소매점",tier:"retail"}
    ];
    let next=state;
    for(const dealer of dealers) {
      const client={...dealer,displayName:dealer.name,payment:"PREPAID",
        active:true,overrides:{},shippingFeeExcluded:false,identifiers:"",
        contact:"",address:"서울특별시 종로구 종로 1",phone:"02-123-4567",
        mobile:"",preferredAccountId:account.id};
      const result=await window.__TAURI_INTERNALS__.invoke("business_core_call",{
        domain:"catalog_clients",operation:"upsert_client",payload:{state:next,client}});
      next=result.state;
    }
    let priced=0;
    next.products=next.products.map(product=>{
      const prices=new Map([
        ["BD 709 / 블랙 / 타공 상판",{wholesale:42000,retail:42000}],
        ["BD GT-1 / 길이 14cm / 평평한 벽용",{wholesale:10000,retail:12000}]
      ]);
      const price=prices.get(product.name);
      if(!price)return product;
      priced++;
      return {...product,variants:product.variants.map(v=>({...v,...price}))};
    });
    if(priced!==2)throw Error("Required registered SKU test seed was not found");
    const committed=await window.bando.save(next);
    return {revision:committed.revision,clientCount:committed.clients.length,
      pricedCatalogItems:priced,orders:committed.orders.length};
  });
  assert.equal(seed.pricedCatalogItems,2);
  record("SYNTHETIC_TEST_DEALERS_AND_REGISTERED_PRICES",seed);
  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({timeout:60000});
  await page.getByRole("button",{name:"접수",exact:true}).click();

  const raw=["김가상","010-1234-5678","경기도 의정부시 시민로 100","709 2개",
    "박가상","010-2345-6789","서울특별시 마포구 월드컵로 20","GT-1 1개"].join("\n");
  await page.evaluate(text=>window.bando.copy(text),raw);
  await page.getByRole("button",{name:"대리발송",exact:true}).click();
  await page.waitForFunction(()=>document.querySelector(".review-pane")?.textContent?.includes("김가상"),
    null,{timeout:30000});
  record("RECOGNIZED_TWO_RECIPIENT_SOURCE",{rawLength:raw.length});
  await page.getByRole("button",{name:"거래처 찾기"}).click();
  await page.locator(".client-chooser-list .client-result")
    .filter({hasText:"가상소매점"}).click();
  await page.locator(".receipt-client b").filter({hasText:"가상소매점"})
    .waitFor({state:"visible",timeout:30000});
  await page.waitForTimeout(950);
  // Capture the actual displayed order line amount after selecting a dealer.
  // The reported bug was all price cells becoming "판매가를 등록하세요".
  const pricing=await page.evaluate(()=>({
    amountButtons:[...document.querySelectorAll("button.item-amount")].map(el=>el.textContent?.trim()),
    pricingErrors:[...document.querySelectorAll(".review-pane,[role=alert]")].map(el=>el.textContent||"")
      .filter(text=>text.includes("판매가를 등록하세요") || text.includes("가격 정보를 찾을 수")),
    review:document.querySelector(".review-pane")?.textContent?.slice(0,2500)
  }));
  await snap(page,"02-retail-dealer-selected-prices");
  record("AFTER_RETAIL_SELECTION_PRICE_DIAGNOSTIC",pricing);
  // The seed is 42,000 x 2 for the primary 709 row.
  assert.ok(pricing.amountButtons.some(text=>text?.includes("84,000")),
    "REGISTERED_PRICE_MISSING_AFTER_DEALER_SELECTED "+JSON.stringify(pricing));
  assert.equal(pricing.pricingErrors.length,0,
    "Registered retail price was rejected as missing");
  record("REGISTERED_RETAIL_PRICES_APPEAR_IN_GUI",{expectedFirstLine:84000});

  // Start a fresh receipt rather than contaminating the two-order regression.
  await page.getByRole("button",{name:/새 주문/}).first().click();
  await page.waitForFunction(()=>
    !(document.querySelector(".source-document")?.textContent||"").includes("김가상"),
    null,{timeout:20000});
  await snap(page,"03-ready-for-two-order-test");

  // Real GUI: clipboard, dealer choice, freight typing, two drafts, panel
  // geometry, confirmation actions, SQLite order identities.
  await probeTwoConsecutiveOrders(page,{record,snap},{holdBoth:true});
  await save();
  // Numeric waybill partial holds + notification pop-up + SQLite read-state
  // persistence on page reload. No user files are read.
  process.env.BANDO_PERSISTENT_NOTICE_AUDIT="true";
  const baseline=resolve(process.env.APPDATA||"",
    "com.bandostand.bandohub.tauri","bando-hub-v001.sqlite");
  await probeFactoryReplyNotifications(page,{baseline,record,snap});
  await save();
  // Real Windows snipping overlay, ESC via native Win32 keyboard, restore
  // previous window geometry and unchanged existing orders.
  await page.getByRole("button",{name:"접수",exact:true}).click();
  await probeWindowsOcrCancel(page,{appPid:pid,record,snap});
  results.status=results.failures.length?"FAIL":"PASS";
  if(results.failures.length)throw Error("Frontend console errors: "+JSON.stringify(results.failures));
  console.log("BANDO_DEV_BUSINESS_PASS",JSON.stringify({tests:results.tests.length,
    screenshots:results.screenshots.length}));
} catch(error) {
  results.status="FAIL";
  results.error=String(error?.stack||error);
  console.error("BANDO_DEV_BUSINESS_FAIL",results.error);
  throw error;
} finally {
  results.completed=new Date().toISOString();
  await save();
  await browser?.close().catch(()=>{});
}
