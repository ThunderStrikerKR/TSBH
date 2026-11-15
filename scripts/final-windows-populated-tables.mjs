// CI-only populated-data screen audit of the actual Windows Tauri EXE.
// Creates immutable synthetic confirmed orders through Rust state_save in the
// disposable developer-test SQLite, reopens the EXE and checks real tables.
// Never touches a user's BANDO DB or uploads private order data.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { promises as fs } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright";

assert.equal(process.platform,"win32");
assert.equal(process.env.GITHUB_ACTIONS,"true","Use an ephemeral Windows CI runner only");
assert.ok(process.env.APPDATA);
const exe=resolve(process.env.BANDO_NATIVE_EXE||"");
assert.match(exe,/\.exe$/i);
await fs.access(exe);
const out=resolve(process.env.BANDO_NATIVE_OUT||"populated-tables-evidence");
await fs.mkdir(out,{recursive:true});
const root=await fs.mkdtemp(join(tmpdir(),"bando-filled-tables-"));
const master=resolve(process.env.APPDATA,"com.bandostand.bandohub.tauri","bando-hub-v001.sqlite");
const testDB=resolve(process.env.APPDATA,"com.bandostand.bandohub.tauri","developer-test","bando-hub-test.sqlite");
await assert.rejects(fs.access(master),{code:"ENOENT"},"Never overwrite an existing operation DB");
const port=19651;
const report={
  target:"Windows Tauri WebView2 / Rust IPC / developer-test SQLite",
  commit:process.env.BANDO_NATIVE_BIN_COMMIT||"",
  executable:basename(exe),
  synthetic:true,productionData:false,
  before:[],after:[],checks:[],screens:[],
  errors:[],status:"STARTED"
};
const record=(name,metrics={})=>{
  report.checks.push({name,...metrics});
  console.log("FILLED_TABLE_PASS "+JSON.stringify({name,...metrics}));
};
const save=()=>fs.writeFile(join(out,"populated-tables-audit.json"),JSON.stringify(report,null,2),"utf8");
await save();
let proc=null,browser=null,page=null;
const kill=(pid)=>{if(pid)try{execFileSync("taskkill.exe",["/PID",String(pid),"/T","/F"],
  {stdio:"pipe",timeout:15000})}catch{/* process already stopped */}};
const spawnApp=(args,kind)=>spawn(exe,args,{
  cwd:dirname(exe),windowsHide:false,stdio:["ignore","pipe","pipe"],
  env:{...process.env,WEBVIEW2_USER_DATA_FOLDER:join(root,kind+"-webview2")}
});
async function bootstrap(){
  proc=spawnApp([],"baseline");
  try{
    for(let i=0;i<130;i++){
      if(proc.exitCode!==null)throw Error("Baseline EXE exited before DB initialization");
      try{if((await fs.stat(master)).size>0)return;}catch{}
      await sleep(350);
    }
    throw Error("Baseline app database was not created");
  }finally{kill(proc.pid);proc=null;await sleep(900)}
}
async function connect(kind){
  proc=spawnApp(["--bando-developer-test"],kind);
  let stderr="";
  proc.stderr?.on("data",b=>{stderr+=b.toString()});
  let ready=false;
  for(let i=0;i<175;i++){
    if(proc.exitCode!==null)throw Error("EXE startup failed: "+stderr.slice(-1400));
    try{
      const res=await fetch("http://127.0.0.1:"+port+"/json/list",
        {signal:AbortSignal.timeout(900)});
      if(res.ok){
        const data=await res.json();
        if(data.some(x=>x.type==="page" && /tauri|localhost/i.test(x.url||""))){
          ready=true;break;
        }
      }
    }catch{}
    await sleep(350);
  }
  assert.ok(ready,"Actual WebView2 CDP is unavailable");
  browser=await chromium.connectOverCDP("http://127.0.0.1:"+port,{timeout:30000});
  // WebView2 initially exposes a target while the Playwright context can
  // still report about:blank or no page. Follow the proven existing native
  // auditor's first-page fallback and wait for the real Tauri navigation.
  for(let attempt=0;attempt<45;attempt++){
    const contexts=browser.contexts();
    const pages=contexts.flatMap(ctx=>ctx.pages());
    page=pages.find(p=>/tauri|localhost/i.test(p.url()))||pages[0]||null;
    if(page){
      try{
        await page.waitForURL(/tauri|localhost/i,{timeout:1800});
        break;
      }catch{ /* native WebView2 is still navigating */ }
    }
    await sleep(250);
  }
  assert.ok(page && /tauri|localhost/i.test(page.url()),
    "No ready Tauri WebView2 page after CDP connection");
  page.setDefaultTimeout(30000);
  page.on("pageerror",e=>report.errors.push(e.message));
  await page.locator("nav[aria-label='업무 메뉴']").waitFor({state:"visible",timeout:30000});
  const info=await page.evaluate(async()=>({
    href:location.href,workspace:await window.bando.workspaceMode(),
    state:await window.bando.load()
  }));
  assert.match(info.href,/^http:\/\/tauri\.localhost/);
  assert.equal(info.workspace,true,"Native developer TEST DB required");
  return info.state;
}
async function disconnect(){
  try{await browser?.close()}catch{}
  browser=null;page=null;
  kill(proc?.pid);
  proc=null;
  await sleep(1050);
}
async function snap(name){
  const file=String(report.screens.length+1).padStart(2,"0")+"-"+name+".png";
  await page.screenshot({path:join(out,file),animations:"disabled",timeout:30000});
  report.screens.push({name,file});
  await save();
}
async function menu(name){
  await page.locator("nav[aria-label='업무 메뉴']")
    .getByRole("button",{name,exact:true}).click();
  await page.waitForFunction(target=>
    document.querySelector("nav[aria-label='업무 메뉴'] button[aria-current='page']")
      ?.textContent?.trim()===target,name,{timeout:30000});
}
async function tab(scope,name){
  const area=page.locator(scope);
  await area.getByRole("button",{name,exact:true}).click();
  await sleep(250);
}
const nearly=(a,b,limit=2)=>Math.abs(a-b)<=limit;
async function examine(selector,label,minimumRows=8){
  const loc=page.locator(selector);
  await loc.waitFor({state:"visible",timeout:30000});
  const metrics=await loc.evaluate((el)=>{
    const table=el.querySelector("table"),headers=[...table.querySelectorAll("thead th")];
    const rows=[...table.querySelectorAll("tbody tr")];
    const dim=e=>{const r=e.getBoundingClientRect();return{
      x:+r.x.toFixed(2),y:+r.y.toFixed(2),w:+r.width.toFixed(2),h:+r.height.toFixed(2)}};
    const style=getComputedStyle(table);
    const firstRow=rows[0],nameCells=[...rows.slice(0,5)].flatMap(row=>
      [...row.cells].slice(0,3).map(cell=>({
        value:cell.textContent?.trim().slice(0,100),
        fontSize:parseFloat(getComputedStyle(cell).fontSize),
        width:cell.clientWidth,scrollWidth:cell.scrollWidth,
        whiteSpace:getComputedStyle(cell).whiteSpace
      })));
    const initialHeaders=headers.map(dim);
    const initial={left:el.scrollLeft,top:el.scrollTop};
    const maxX=el.scrollWidth-el.clientWidth,maxY=el.scrollHeight-el.clientHeight;
    el.scrollLeft=maxX;el.scrollTop=Math.min(270,maxY);
    const reached={left:el.scrollLeft,top:el.scrollTop};
    const afterHeaders=headers.map(dim);
    el.scrollLeft=0;el.scrollTop=0;
    return {
      rowCount:rows.length,columns:headers.length,
      viewport:dim(el),table:dim(table),tableLayout:style.tableLayout,
      scrollW:el.scrollWidth,clientW:el.clientWidth,scrollH:el.scrollHeight,
      clientH:el.clientHeight,maxX,maxY,reached,reset:{left:el.scrollLeft,top:el.scrollTop},
      headerWidths:initialHeaders.map(r=>r.w),
      headerWidthsAfter:afterHeaders.map(r=>r.w),
      firstRow: firstRow?.textContent?.trim().slice(0,260)||"",
      cells:nameCells,
      documentOverflow:document.documentElement.scrollWidth-innerWidth
    };
  });
  assert.ok(metrics.rowCount>=minimumRows,
    label+" expected populated rows, got "+metrics.rowCount);
  assert.ok(metrics.columns>=5,label+" lost expected columns");
  assert.ok(metrics.maxY>110,label+" lacks bounded vertical scrolling");
  assert.ok(metrics.reached.top>60,label+" cannot scroll downward");
  assert.ok(metrics.reset.top===0 && metrics.reset.left===0,label+" cannot restore scroll");
  assert.ok(metrics.headerWidths.every((w,i)=>nearly(w,metrics.headerWidthsAfter[i],0.75)),
    label+" header column widths drift during scrolling");
  assert.ok(metrics.documentOverflow<=3,label+" overflows whole application viewport");
  assert.ok(metrics.cells.every(x=>x.fontSize>=11),
    label+" has unreadably small table-cell font");
  record("populated "+label,metrics);
  await snap(label);
  return metrics;
}
async function seedOrders(){
  return await page.evaluate(async()=>{
    const state=await window.bando.load();
    if(state.orders.length || state.clients.length)
      throw Error("Populated test requires an otherwise empty isolated TEST workspace");
    if(!state.products.length || !state.workflow?.sources ||
       !state.workflow?.groups || !state.workflow?.shipments || !state.workflow?.events)
      throw Error("Native catalogue/workflow baseline is incomplete");
    const products=state.products.filter(x=>x.active!==false && !x.deletedAt &&
      x.variants?.length);
    if(products.length<3)throw Error("Real active catalogue does not contain three models");
    const now=new Date().toISOString();
    const client={
      id:"ci-filled-client",name:"긴거래처명검증악기사"+ "서초지점".repeat(5),
      displayName:"CI 표 데이터 거래처",
      contact:"표 전수검사",address:"경기도 의정부시 시민로 111 검증빌딩 2층",
      phone:"031-123-4567",mobile:"",
      tier:"wholesale",shippingFeeExcluded:false,payment:"PREPAID",
      active:true,identifiers:"CI 표 검증",overrides:{}
    };
    state.clients.push(client);
    const orderIds=[];
    for(let i=0;i<20;i++){
      const id="ci-filled-order-"+String(i+1).padStart(2,"0");
      const raw="CI_POPULATED_"+i+" 실제 저장된 발주서 " +
        "주문 요청 상세 내용 ".repeat(5);
      const recipients=[];
      const shipmentIds=[];
      for(let r=0;r<3;r++){
        const prod=products[(i*3+r)%products.length],variation=prod.variants[0];
        const product2=products[(i*3+r+11)%products.length];
        const vr2=product2.variants[0];
        const recId=id+"-recipient-"+r;
        const recipient={
          id:recId,name:"실주문 수취인 "+i+"-"+r+
            (r===1?" 이름이매우긴고객".repeat(6):""),
          address:"서울특별시 종로구 낙원동 123-45 " +
            "보충창고 입구 옆 상세 배송지 ".repeat(14),
          phone:"02-999-8888",mobile:"010-1234-5678",
          note:"배송시 현관 앞 연락 "+r,
          items:[
            {id:recId+"-item-a",productId:prod.id,variantId:variation.id,
              raw:prod.name+" 2개",name:prod.name,
              color:variation.color||"Black",quantity:2,
              unitPrice:11000+i*100,origin:"FACTORY",
              defaulted:false,issues:[],url:""},
            {id:recId+"-item-b",productId:product2.id,variantId:vr2.id,
              raw:product2.name+" 1개",name:product2.name,
              color:vr2.color||"Black",quantity:1,
              unitPrice:17500+i*50,origin:"FACTORY",
              defaulted:false,issues:[],url:""}
          ]
        };
        recipients.push(recipient);
        const shipId=id+"-ship-"+r;
        shipmentIds.push(shipId);
        state.workflow.shipments.push({
          id:shipId,sourceId:id+"-source",orderId:id,
          recipientId:recId,groupId:id+"-group",step:1,
          completedAt:{"1":now},charges:{FACTORY:3000+i*100},
          chargeMode:"AUTO",freightVersion:"CI-FREIGHT-1",
          infoSignature:"ci-filled-source",transferred:false,
          recipientSnapshot:structuredClone(recipient),
          sender:structuredClone(client)
        });
      }
      const base={
        id,clientId:client.id,clientSnapshot:structuredClone(client),
        orderType:"AGENCY",raw,channel:"카카오톡",recipients,
        shippingFee:3000+i*100,applyVat:false,vatAmount:0,status:"APPROVED",
        notified:false,paid:false,postpaidApproved:false,freightCode:"010",
        receptionVersion:1,confirmedAt:now,createdAt:now,updatedAt:now,
        receipt:{original:raw,at:now,actor:"CI Windows 실주문 검증",
          clientId:client.id,legacy:false}
      };
      const order={
        ...base,
        confirmationSnapshot:{
          at:now,raw,client:structuredClone(client),
          recipients:structuredClone(recipients),data:structuredClone(base)
        }
      };
      state.orders.push(order);
      state.workflow.sources.push({
        id:id+"-source",number:8100+i,orderId:id,
        dealerId:client.id,raw,createdAt:now
      });
      state.workflow.groups.push({
        id:id+"-group",rootGroupId:id+"-group",
        sourceId:id+"-source",dealerId:client.id,
        createdAt:now,notices:[],needsNotice:false
      });
      state.workflow.events.push({
        id:id+"-confirm",at:now,actor:"CI Windows 실주문 검증",
        action:"주문 확인 완료",shipmentIds
      });
      orderIds.push(id);
    }
    const committed=await window.bando.save(state);
    if(committed.orders.length!==20 || committed.revision<=state.revision)
      throw Error("Native Rust state_save did not commit 20 confirmed orders");
    return {
      revision:committed.revision,
      orders:committed.orders.length,clients:committed.clients.length,
      products:committed.products.length,
      shipments:committed.workflow.shipments.length,
      sampleOrderId:orderIds[0],
      originalProductCount:products.length
    };
  });
}
try{
  await bootstrap();
  const before=await connect("seed");
  assert.equal(before.orders.length,0);
  assert.ok(before.products.length>=300);
  report.before=[{revision:before.revision,products:before.products.length,orders:before.orders.length}];
  const seeded=await seedOrders();
  record("Native Rust state_save of confirmed synthetic orders",seeded);
  await disconnect();
  const state=await connect("reopen");
  assert.equal(state.orders.length,20,"20 persisted orders must survive EXE relaunch");
  assert.equal(state.workflow.shipments.length,60,
    "All 60 persisted delivery shipments must survive relaunch");
  assert.equal(state.products.length,seeded.products,"Original 360 catalogue rows changed");
  report.after=[{revision:state.revision,orders:state.orders.length,
    shipments:state.workflow.shipments.length,products:state.products.length}];
  record("EXE relaunch and native SQLite reload retains all 20 orders",report.after[0]);

  await menu("상품 관리");
  await page.locator(".product-prices tbody tr").first().waitFor({state:"visible",timeout:30000});
  const products=await examine(".product-list-workspace .price-table-scroll",
    "products-360-rows",300);
  assert.ok(products.rowCount>=300);
  // Long product names may overflow their own cell, but never force a
  // whole-window scrollbar or hide downstream price columns.
  // A stable vertical scrollbar consumes ~8px inside the scroll viewport.
  // The table must fill the client area, not the outer scrollport border box.
  assert.ok(products.table.w>=products.clientW-2,
    "Product table must fill its usable scroll client width");
  const fullTitles=await page.locator(".product-title-line[title]").evaluateAll(nodes=>
    nodes.filter(el=>el.getAttribute("title")?.length>65).length);
  assert.ok(fullTitles>0,
    "Clipped long product subtitles must be recoverable via full-text hover titles");
  record("Full model/description hover text survives compact price rows",{
    longRowsWithTitle:fullTitles
  });

  await menu("운임 관리");
  await tab("[aria-label='배송 관리']","상품별 기본 운임");
  const freight=await examine(".shipping-product-list .shipping-table-scroll",
    "freight-360-rows",300);
  await tab("[aria-label='배송 관리']","동급 배송 그룹 지정");
  const assignment=await examine(".shipping-assignment-list .shipping-table-scroll",
    "freight-group-assignment-360-rows",300);
  assert.ok(assignment.maxX>150,
    "Wide assignment table must retain horizontal scrolling for long names");
  assert.ok(assignment.reached.left>150,
    "Right-side assignment/status columns must be reachable");

  await menu("주문 내역");
  // This is an explicit all-dates scope: no silently filtered test data.
  const clear=page.getByRole("button",{name:"전체 날짜",exact:true});
  if(await clear.count())await clear.click();
  await page.waitForFunction(()=>{
    const panel=document.querySelector(".order-history-list");
    return panel?.querySelectorAll("tbody tr").length===20 &&
      !panel.classList.contains("order-history-empty");
  },null,{timeout:45000});
  const hist=await examine(".order-history-list .table-scroll",
    "history-20-orders-60-recipients",20);
  assert.equal(hist.rowCount,20);
  assert.ok(hist.maxX>100,
    "Populated history must keep its horizontal 1000px column contract");
  const row=page.locator(".order-history-list tbody tr").first();
  const button=row.locator("button[aria-expanded]").first();
  const base=await page.locator(".order-history-list thead th")
    .evaluateAll(nodes=>nodes.map(el=>el.getBoundingClientRect().width));
  await button.click();
  await row.getByText(/주문 당시:/).first().waitFor({state:"visible",timeout:15000});
  const expanded=await page.locator(".order-history-list thead th")
    .evaluateAll(nodes=>nodes.map(el=>el.getBoundingClientRect().width));
  assert.deepEqual(expanded.map(n=>Math.round(n*100)/100),
    base.map(n=>Math.round(n*100)/100),
    "History headers shifted after long recipient expansion");
  await snap("history-expanded-three-recipient-details");
  await button.click();
  await row.getByText(/주문 당시:/).first().waitFor({state:"hidden"});
  record("Long recipient expansion keeps all six header widths",{
    columns:base.length,headerWidths:base,recipientDetails:3
  });

  const final=await page.evaluate(async()=>window.bando.load());
  assert.equal(final.revision,state.revision,
    "Read-only populated table scrolling unexpectedly wrote SQLite");
  assert.equal(final.orders.length,20);
  assert.equal(final.products.length,seeded.products);
  assert.deepEqual(report.errors,[],"WebView2 JavaScript errors");
  report.status="PASSED_NATIVE_RUST_PERSISTED_POPULATED_TABLES";
  await save();
  console.log("FILLED_TABLE_AUDIT_PASS "+JSON.stringify({
    orders:20,recipients:60,products:final.products.length,
    screenshots:report.screens.length,checks:report.checks.length,
    revision:final.revision
  }));
}catch(error){
  report.status="FAILED";
  report.failure=String(error?.stack||error);
  process.exitCode=1;
  console.error("FILLED_TABLE_AUDIT_FAILED "+report.failure);
  if(page)try{await snap("FAILED-current-viewport")}catch{}
}finally{
  await save();
  await disconnect();
  // CI-only synthetic baseline is never a user's existing database.
  // Leave the runner-local evidence on disk for the job artifact upload.
}
