// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";

export async function run(page,{record,snap,paste,chooseDealer}){
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

