// Real Windows Tauri WebView2 keyboard/mouse continuation after SIM31 freight.
// Use the *rendered* workflow controls, not direct business_core_call shortcuts.
import assert from "node:assert/strict";

const accountMarker="123-456-789";
const digits=v=>(v??"").replace(/\D/g,"");

export async function probeWindowsAccountCopy(page,{record,snap}) {
  const second=page.getByRole("button",{name:"배송 건 박가상"});
  await second.waitFor({state:"visible",timeout:30000});
  await second.click();
  await page.waitForFunction(()=>document.querySelector(".workflow-shipment.chosen")?.getAttribute("aria-label")==="배송 건 박가상",null,{timeout:30000});
  const money=page.locator('.items-freight-panel .freight-money input[aria-label^="박가상"]:not([disabled])').first();
  await money.waitFor({state:"visible",timeout:30000});
  await money.click();
  await money.press("ControlOrMeta+A");
  await money.press("Backspace");
  await page.keyboard.type("3500",{delay:90});
  await page.waitForTimeout(800);
  const secondValue=await money.inputValue();
  assert.equal(digits(secondValue),"3500",
    "SIM31_03 second recipient freight lost after typing: "+secondValue);
  record("SIM31_03 second recipient's freight edited in Windows EXE",
    {recipient:"박가상",value:secondValue});

  // Switching between two shipments must preserve the first manual edit.
  const first=page.getByRole("button",{name:"배송 건 김가상"});
  await first.click();
  await page.waitForFunction(()=>document.querySelector(".workflow-shipment.chosen")?.getAttribute("aria-label")==="배송 건 김가상",null,{timeout:30000});
  const firstField=page.locator('.items-freight-panel .freight-money input[aria-label^="김가상"]:not([disabled])').first();
  await firstField.waitFor({state:"visible",timeout:30000});
  await page.waitForTimeout(600);
  const firstValue=await firstField.inputValue();
  assert.equal(digits(firstValue),"4500",
    "SIM31_04 first manual freight was lost after changing shipment: "+firstValue);
  record("SIM31_04 first freight preserved across shipment selection",
    {recipient:"김가상",firstValue,secondValue});

  const stage=page.locator(".workflow-group .workflow-progress button.primary").first();
  async function advance(previous,expected,step) {
    await stage.waitFor({state:"visible",timeout:30000});
    await page.waitForFunction(
      label=>Array.from(document.querySelectorAll(".workflow-group .workflow-progress button.primary"))
        .some(b=>b.getAttribute("aria-label")===label&&!b.disabled),
      previous,{timeout:45000});
    const current=await stage.getAttribute("aria-label");
    assert.equal(current,previous,"SIM31_"+step+" unexpected stage "+current);
    await stage.click();
    try {
      await page.waitForFunction(
        label=>Array.from(document.querySelectorAll(".workflow-group .workflow-progress button.primary"))
          .some(b=>b.getAttribute("aria-label")===label),
        expected,{timeout:30000});
    } catch(cause) {
      const diagnostic=await page.evaluate(()=>({
        currentStep:[...document.querySelectorAll(".workflow-group .workflow-progress button.primary")]
          .map(x=>({label:x.getAttribute("aria-label"),disabled:x.disabled})),
        staff:document.querySelector(".staff-checks")?.textContent?.slice(0,2200),
        alerts:[...document.querySelectorAll('[role="alert"]')]
          .map(x=>x.textContent?.slice(0,450)),
        notices:[...document.querySelectorAll('[role="status"]')]
          .map(x=>x.textContent?.slice(0,280)),
        message:document.querySelector(".messages-pane")?.textContent?.slice(0,1200)
      }));
      console.error("SIM31_"+step+"_STOP",JSON.stringify(diagnostic));
      throw Error("SIM31_"+step+" workflow did not advance "+
        previous+" -> "+expected+": "+JSON.stringify(diagnostic)+
        "; "+String(cause));
    }
    record("SIM31_"+step+" actual GUI workflow advanced",
      {from:previous,to:expected});
  }
  await advance("주문 확인 완료","운임 적용","05");
  // Workflow buttons have a 2-second intentional anti-double-click cooldown.
  await page.waitForTimeout(2200);
  await advance("운임 적용","계좌 안내 완료","06");

  const copy=page.getByRole("button",{name:"그대로 복사"});
  await copy.waitFor({state:"visible",timeout:25000});
  await page.waitForFunction(
    marker=>{const x=document.querySelector('[aria-label="복사용 자동 메시지 본문"]');
      return x&&(x.value??x.textContent??"").includes(marker);},
    accountMarker,{timeout:30000});
  const output=await page.locator('[aria-label="복사용 자동 메시지 본문"]').evaluate(
    el=>el.value??el.textContent??"");
  assert.match(output,/가상은행/,"SIM31_07 bank name missing from copy preview");
  assert.ok(output.includes(accountMarker),
    "SIM31_07 bank number missing from copy preview");
  assert.ok(await copy.isEnabled(),"SIM31_07 bank copy button disabled");
  await copy.click();
  await page.waitForTimeout(450);
  const clipboard=await page.evaluate(()=>window.bando.readClipboard());
  assert.ok(clipboard.includes(accountMarker),
    "SIM31_07 copy to real Windows clipboard omitted bank account");
  assert.ok(clipboard.includes("가상은행"),
    "SIM31_07 copy to real Windows clipboard omitted bank name");
  record("SIM31_07 payment guidance copied through Windows clipboard",{
    previewLength:output.length,clipboardLength:clipboard.length,
    accountPresent:true,bankPresent:true
  });
  await snap(page,"08-sim31-windows-payment-copy");
  return {freightPreserved:true,workflowSteps:["주문 확인 완료","운임 적용"],
    bankCopy:true};
}
