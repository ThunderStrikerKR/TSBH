// Isolated real Windows Tauri DEV/WebView2 domain test.
// The CI runner creates a fresh developer TEST SQLite for EACH invocation.
import assert from "node:assert/strict";
import { chromium } from "playwright";
import { resolve, join } from "node:path";
import { promises as fs } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { probeTwoConsecutiveOrders } from "./migration31-real-two-orders.mjs";
import { probeFactoryReplyNotifications } from "./migration32-real-factory-notifications.mjs";

const DOMAINS=["orders","pricing","shipping","clients","factory"];
const domain=process.argv[2];
if(!DOMAINS.includes(domain))throw Error("Unexpected isolated domain: "+domain);
assert.equal(process.platform,"win32");
assert.equal(process.env.GITHUB_ACTIONS,"true");
const pid=Number(process.env.BANDO_DEV_NATIVE_PID);
assert.ok(Number.isInteger(pid)&&pid>0,"A real Windows native process is required");
const root=resolve(process.env.BANDO_DEV_EVIDENCE||"dev-windows-smoke-evidence");
await fs.mkdir(root,{recursive:true});
const results={domain,sourceCommit:process.env.GITHUB_SHA,tests:[],screenshots:[],failures:[]};
const save=()=>fs.writeFile(join(root,"dev-domain-"+domain+".json"),JSON.stringify(results,null,2));
const record=(name,value={})=>{
  results.tests.push({name,at:new Date().toISOString(),...value});
  console.log("BANDO_DEV_DOMAIN_CHECK",domain,name,JSON.stringify(value));
};
const snap=async(page,name)=>{
  const filename="domain-"+domain+"-"+name+".png";
  await page.screenshot({path:join(root,filename),animations:"disabled",timeout:20000});
  results.screenshots.push(filename);
  await save();
};

async function setupFixture(page){
  const fixture=await page.evaluate(async()=>{
    const api=window.bando;
    if(!api||!await api.workspaceMode())throw Error("Fresh developer TEST workspace required");
    if((await api.startupStatus()).phase!=="READY")throw Error("Native startup is not ready");
    const initial=await api.load();
    if(initial.orders.length||initial.clients.length)throw Error("Previous scenario contaminated TEST DB");
    const account={id:"sim31-account",bank:"가상은행",number:"123-456-789",holder:"반도스탠드"};
    let next=structuredClone(initial);
    next.settings.accounts=[...(next.settings.accounts??[]),account];
    next.settings.defaultAccountId=account.id;
    for(const d of [
      {id:"sim31-dealer",name:"가상대리점",tier:"wholesale"},
      {id:"sim32-retail",name:"가상소매점",tier:"retail"}
    ]){
      const client={...d,displayName:d.name,payment:"PREPAID",
        active:true,overrides:{},shippingFeeExcluded:false,identifiers:"",
        contact:"",address:"서울특별시 종로구 종로 1",phone:"02-123-4567",
        mobile:"",preferredAccountId:account.id};
      next=(await window.__TAURI_INTERNALS__.invoke("business_core_call",{
        domain:"catalog_clients",operation:"upsert_client",payload:{state:next,client}
      })).state;
    }
    const prices=new Map([
      ["BD 709 / 블랙 / 타공 상판",{wholesale:42000,retail:42000}],
      ["BD GT-1 / 길이 14cm / 평평한 벽용",{wholesale:10000,retail:12000}]
    ]);
    let matched=0;
    next.products=next.products.map(p=>{
      const price=prices.get(p.name);
      if(!price)return p;
      matched++;
      return {...p,variants:p.variants.map(v=>({...v,...price}))};
    });
    if(matched!==2)throw Error("Seed catalog lacked two required SKUs");
    const saved=await api.save(next);
    return {revision:saved.revision,clients:saved.clients.length,
      products:saved.products.length,orders:saved.orders.length};
  });
  assert.equal(fixture.clients,2);
  assert.equal(fixture.orders,0);
  record("FRESH_NATIVE_FIXTURE",fixture);
  await page.reload({waitUntil:"domcontentloaded"});
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({timeout:60000});
}

async function paste(page,name,item="709 2개",dealer=null){
  await page.getByRole("button",{name:"접수",exact:true}).click();
  const raw=[...(dealer?["거래처: "+dealer]:[]),name,
    "010-1234-5678","경기도 의정부시 시민로 100",item].join("\n");
  await page.evaluate(text=>window.bando.copy(text),raw);
  await page.getByRole("button",{name:"대리발송",exact:true}).click();
  await page.waitForFunction(v=>
    (document.querySelector(".source-document")?.textContent||"").includes(v),
    name,{timeout:30000});
  return raw;
}
async function chooseDealer(page,name="가상소매점"){
  await page.getByRole("button",{name:"거래처 찾기",exact:true}).click();
  await page.locator(".client-chooser-list .client-result").filter({hasText:name}).click();
  await page.locator(".receipt-client b").filter({hasText:name})
    .waitFor({state:"visible",timeout:30000});
}

async function orders(page){
  await probeTwoConsecutiveOrders(page,{record,snap},{holdBoth:false});
  const stored=await page.evaluate(async()=>
    (await window.bando.load()).orders.map(o=>({id:o.id,confirmed:!!o.confirmedAt})));
  assert.equal(stored.length,2);
  assert.ok(stored.every(o=>o.confirmed));
  record("ORDERS_TWO_SQLITE_CONFIRMATIONS",{orders:stored});
}

async function pricing(page){
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

async function shipping(page){
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
  const group=page.locator(".workflow-group").filter({
    has:page.getByRole("button",{name:"배송 건 김가상"})
  }).first();
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

async function clients(page){
  const raw=[
    "거래처: 가상대리점","정가상","010-1234-5678",
    "경기도 의정부시 시민로 100","709 1개"
  ].join("\n");
  const evidence=await page.evaluate(async text=>{
    const state=await window.bando.load();
    return window.__TAURI_INTERNALS__.invoke("business_core_call",{
      domain:"catalog_clients",operation:"auto_match_reception_client",
      payload:{state,windows:[],raw:text,activeChatHandle:""}
    });
  },raw);
  assert.equal(evidence.source,"RAW_EXPLICIT");
  assert.equal(evidence.clientId,"sim31-dealer");
  await paste(page,"정가상","709 1개","가상대리점");
  await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
    .waitFor({state:"visible",timeout:30000});
  const actual=await page.evaluate(async()=>{
    const state=await window.bando.load();
    return {chosen:document.querySelector(".receipt-client b")?.textContent?.trim(),
      clients:state.clients.length,orders:state.orders.length};
  });
  assert.equal(actual.chosen,"가상대리점");
  assert.equal(actual.clients,2);
  assert.equal(actual.orders,0);
  record("CLIENTS_RUST_EXPLICIT_AUTO_MATCH",actual);
  await snap(page,"clients");
}

async function factory(page){
  process.env.BANDO_PERSISTENT_NOTICE_AUDIT="true";
  const baseline=resolve(process.env.APPDATA||"",
    "com.bandostand.bandohub.tauri","bando-hub-v001.sqlite");
  await probeFactoryReplyNotifications(page,{baseline,record,snap});
  const final=await page.evaluate(async()=>{
    const state=await window.bando.load();
    return {orders:state.orders.length,
      notices:(await window.bando.notificationList()).length};
  });
  assert.equal(final.orders,0);
  assert.equal(final.notices,0);
  record("FACTORY_INDEPENDENT_SQLITE_ROUNDTRIP",final);
}

let browser;
try{
  let available=false;
  for(let i=0;i<120;i++){
    try{
      const r=await fetch("http://127.0.0.1:19651/json/list",{
        signal:AbortSignal.timeout(700)});
      if(r.ok){
        const tabs=await r.json();
        available=tabs.some(t=>t.type==="page"&&/localhost|tauri/i.test(t.url||""));
        if(available)break;
      }
    }catch{}
    await sleep(500);
  }
  assert.ok(available,"Actual Windows WebView2 CDP endpoint did not open");
  browser=await chromium.connectOverCDP("http://127.0.0.1:19651",{timeout:20000});
  const context=browser.contexts()[0];
  const page=context?.pages().find(p=>/localhost|tauri/i.test(p.url()))??context?.pages()[0];
  assert.ok(page,"Actual native WebView2 page is missing");
  page.setDefaultTimeout(30000);
  page.on("pageerror",e=>results.failures.push(String(e)));
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({timeout:90000});
  await setupFixture(page);
  await ({orders,pricing,shipping,clients,factory})[domain](page);
  if(results.failures.length)throw Error("Unexpected JS errors: "+JSON.stringify(results.failures));
  results.status="PASS";
  console.log("BANDO_DEV_DOMAIN_PASS",JSON.stringify({
    domain,checks:results.tests.length,screenshots:results.screenshots.length,
    sourceCommit:process.env.GITHUB_SHA
  }));
}catch(error){
  results.status="FAIL";results.error=String(error?.stack||error);
  console.error("BANDO_DEV_DOMAIN_FAIL",domain,results.error);
  throw error;
}finally{
  await save();
  await browser?.close().catch(()=>{});
}
