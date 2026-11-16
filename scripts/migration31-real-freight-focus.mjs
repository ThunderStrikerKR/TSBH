// Browser-driven actual Windows Tauri EXE / WebView2 / Rust IPC regression.
// Used ONLY with CI's disposable --bando-developer-test SQLite workspace.
// No mocked browser, business service, React state, or keyboard input.
import assert from "node:assert/strict";

export async function probeWindowsFreightFocus(page,{record,snap}) {
  assert.equal(process.platform,"win32","This is an actual Windows EXE GUI test");
  assert.equal(process.env.GITHUB_ACTIONS,"true","Only disposable CI Windows workspace");
  // Setup only: establish valid synthetic client/account in the real isolated
  // SQLite. All subsequent receipt, dealer selection and freight edits are
  // performed by clicking and typing in the ACTUAL application's WebView2.
  const fixture=await page.evaluate(async()=>{
    if(!window.bando || !await window.bando.workspaceMode())
      throw Error("Not the isolated developer TEST database");
    const before=await window.bando.load();
    if(before.orders?.length)throw Error("Fixture requires empty TEST orders");
    const account={id:"sim31-account",bank:"가상은행",number:"123-456-789",
      holder:"반도스탠드"};
    const client={id:"sim31-dealer",name:"가상대리점",displayName:"가상대리점",
      tier:"wholesale",payment:"PREPAID",active:true,overrides:{},
      shippingFeeExcluded:false,identifiers:"",contact:"",
      address:"서울특별시 종로구 종로 1",phone:"02-123-4567",mobile:"",
      preferredAccountId:account.id};
    const state=structuredClone(before);
    state.settings.accounts=[...(state.settings.accounts??[]),account];
    state.settings.defaultAccountId=account.id;
    const {state:validated}=await window.__TAURI_INTERNALS__.invoke(
      "business_core_call",{domain:"catalog_clients",operation:"upsert_client",
        payload:{state,client}});
    const committed=await window.bando.save(validated);
    if(!committed.clients?.some(c=>c.id===client.id))
      throw Error("Native validated synthetic dealer was not persisted");
    return {revision:committed.revision,
      client:client.id,account:account.id,orderCount:committed.orders.length};
  });
  record("SIM31_SETUP isolated SQLite synthetic dealer and account",fixture);

  // We are continuing the existing test's REAL GUI reception: clipboard
  // pasted order with 2 recipients/2 models was already analyzed by Rust.
  await page.getByRole("button",{name:"거래처 찾기"}).click();
  const choice=page.locator(".client-chooser-list .client-result")
    .filter({hasText:"가상대리점"});
  await choice.waitFor({state:"visible",timeout:30000});
  await choice.click();
  await page.locator(".receipt-client b").filter({hasText:"가상대리점"})
    .waitFor({state:"visible",timeout:30000});
  record("SIM31_01 receipt client selected by actual WebView2 click",{client:"가상대리점"});

  const freight=page.locator(".items-freight-panel .freight-money input:not([disabled])").first();
  try {
    await freight.waitFor({state:"visible",timeout:45000});
  } catch(error) {
    const diag=await page.evaluate(()=>({
      details:[...document.querySelectorAll(".review-pane")]
        .map(e=>e.textContent?.slice(0,900)),
      orderPanel:document.querySelector(".items-freight-panel")?.textContent?.slice(0,900),
      alerts:[...document.querySelectorAll('[role="alert"]')]
        .map(e=>e.textContent?.slice(0,350)),
      fields:[...document.querySelectorAll(".freight-money input")]
        .map(el=>({disabled:el.disabled,value:el.value,outer:el.outerHTML.slice(0,300)}))
    }));
    throw Error("SIM31_02 no editable freight field after dealer selection: "+
      JSON.stringify(diag)+"; original error: "+error);
  }

  const start=await freight.evaluate(el=>{
    window.__sim31FreightNode=el;
    window.__sim31FreightHistory=[];
    window.__sim31FreightObserver=new MutationObserver(()=>{
      const node=window.__sim31FreightNode;
      if(node && !node.isConnected)window.__sim31FreightHistory.push("DOM_DETACHED");
    });
    window.__sim31FreightObserver.observe(document.body,{childList:true,subtree:true});
    return {value:el.value,aria:el.getAttribute("aria-label"),
      connected:el.isConnected,disabled:el.disabled};
  });
  await freight.click();
  await freight.press("ControlOrMeta+A");
  await freight.press("Backspace");
  const samples=[];
  for(const digit of "4500"){
    await page.keyboard.type(digit,{delay:90});
    // Wait for the real asynchronous Rust preview (which used to unmount the
    // input every single keystroke), not only the synchronous DOM input event.
    await page.waitForTimeout(400);
    const observed=await page.evaluate(()=>{
      const node=window.__sim31FreightNode;
      return {connected:!!node?.isConnected,
        focused:document.activeElement===node,
        value:node?.value??null,
        cursor:node?.selectionStart??null,
        detached:(window.__sim31FreightHistory??[]).length,
        activeLabel:document.activeElement?.getAttribute("aria-label"),
        allInputs:document.querySelectorAll(".freight-money input").length};
    });
    samples.push({digit,...observed});
    assert.equal(observed.connected,true,
      "SIM31_FREIGHT_FAILURE input DOM unmounted after typing "+digit+
      ": "+JSON.stringify(samples));
    assert.equal(observed.focused,true,
      "SIM31_FREIGHT_FAILURE input focus left after typing "+digit+
      ": "+JSON.stringify(samples));
    assert.equal(observed.detached,0,
      "SIM31_FREIGHT_FAILURE input blinked/recreated: "+JSON.stringify(samples));
  }
  const final=await page.evaluate(()=>{
    const node=window.__sim31FreightNode;
    window.__sim31FreightObserver?.disconnect();
    return {value:node?.value??null,
      connected:!!node?.isConnected,focused:document.activeElement===node,
      detached:(window.__sim31FreightHistory??[]).length};
  });
  assert.equal(final.value.replaceAll(",",""),"4500",
    "SIM31_FREIGHT_FAILURE expected typed 4500, saw: "+JSON.stringify(samples));
  assert.equal(final.focused,true,
    "SIM31_FREIGHT_FAILURE input focus lost after render");
  record("SIM31_02 actual EXE applied freight typed without cursor loss",{
    start,typed:"4500",final,samples,
    result:"PASS_ACTUAL_WINDOWS_WEBVIEW2_KEYBOARD"
  });
  await snap(page,"07-sim31-native-freight-keyboard-focus");
  return {fixture,steps:["RECEIPT_DEALER_SELECTED","FREIGHT_KEYBOARD_FOCUS_PRESERVED"],
    tested:true,samples,final};
}
