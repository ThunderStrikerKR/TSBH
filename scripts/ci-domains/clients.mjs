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
  // Direct selection is already an explicit user decision. A manual
  // "CLIENT_MATCH_EVIDENCE" INFO card must not ask for another confirmation.
  await page.locator(".receipt-client").getByRole("button",{name:"변경",exact:true}).click();
  await page.locator(".client-chooser-list .client-result")
    .filter({hasText:"가상소매점"}).click();
  await page.locator(".receipt-client b").filter({hasText:"가상소매점"})
    .waitFor({state:"visible",timeout:30000});
  await page.waitForFunction(()=>
    document.querySelector(".staff-checks")?.getAttribute("aria-busy")==="false",
    null,{timeout:30000});
  // Observe past the asynchronous Rust review refresh; do not assert only
  // against a temporary loading/empty review panel.
  await page.waitForTimeout(1400);
  const afterManual=await page.evaluate(()=>({
    dealer:document.querySelector(".receipt-client b")?.textContent?.trim(),
    review:document.querySelector(".staff-checks")?.textContent??"",
  }));
  assert.equal(afterManual.dealer,"가상소매점");
  assert.ok(!afterManual.review.includes("거래처 연결 근거"),
    "An accepted manual dealer choice must not generate another review notice");
  assert.ok(!afterManual.review.includes("사용자가 거래처"),
    "Manual provenance is retained internally, not shown as a review card");
  record("CLIENTS_MANUAL_SELECTION_SILENT",{
    dealer:afterManual.dealer,
    hasRedundantEvidence:false,
  });

  // A genuine no-match reception has one required action, not both an
  // orange missing-dealer card and a blue "no automatic basis" notice.
  await page.getByRole("button",{name:/새 주문/}).first().click();
  await paste(page,"미선택가상","709 1개");
  await page.getByRole("button",{name:"거래처 찾기",exact:true})
    .waitFor({state:"visible",timeout:30000});
  await page.waitForFunction(()=>{
    const pane=document.querySelector(".staff-checks");
    if(!pane||pane.getAttribute("aria-busy")==="true")return false;
    return [...pane.querySelectorAll(".staff-block .issue-heading b")]
      .some(el=>el.textContent?.includes("거래처를 선택하면 단가가 적용됩니다"));
  },null,{timeout:30000});
  await page.waitForTimeout(1100);
  const missingDealer=await page.evaluate(()=>{
    const pane=document.querySelector(".staff-checks");
    const all=[...(pane?.querySelectorAll(".staff-block, .staff-fact")??[])];
    const relevant=all.filter(el=>/거래처를 선택하면 단가가 적용됩니다|거래처 자동 연결 근거가 없습니다|거래처를 찾지 못했습니다|거래처 자동 매칭 오류/.test(el.textContent??""));
    const primary=relevant.find(el=>el.classList.contains("staff-block"));
    return {
      count:relevant.length,
      titles:relevant.map(el=>el.querySelector("b")?.textContent?.trim()??""),
      duplicateSmall:!!primary?.querySelector(".issue-heading small")?.textContent?.includes("거래처를 선택하면 단가가 적용됩니다"),
      dealer:document.querySelector(".receipt-client b")?.textContent?.trim()??null,
    };
  });
  assert.equal(missingDealer.count,1,"No-match must not add a second blue dealer notice");
  assert.equal(missingDealer.dealer,null,"No dealer should be silently assigned");
  assert.equal(missingDealer.duplicateSmall,false,"Do not repeat the issue title as its detail");
  record("CLIENTS_UNMATCHED_DEALER_SINGLE_ACTION",missingDealer);
  await snap(page,"clients");
}

