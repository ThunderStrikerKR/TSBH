// Actual Windows Tauri EXE / WebView2: launch the native snipping overlay,
// press real Win32 ESC, and verify window restoration and untouched SQLite.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

export async function probeWindowsOcrCancel(page, { appPid, record, snap }) {
  assert.equal(process.platform,"win32");
  assert.equal(process.env.GITHUB_ACTIONS,"true");
  const nativeWindow = (action="state") => {
    const output = execFileSync("pwsh.exe",[
      "-NoProfile","-ExecutionPolicy","Bypass",
      "-File",resolve("scripts/migration32-win-ocr-escape.ps1"),
      "-TargetPid",String(appPid),"-Action",action
    ], {encoding:"utf8",timeout:12000});
    return action==="state" ? JSON.parse(output.trim()) : output.trim();
  };
  const before = nativeWindow();
  assert.equal(before.minimized,false,"OCR audit must begin with a visible app");
  const persisted = await page.evaluate(async()=>window.bando.load());
  const currentText = await page.locator(".source-document").innerText();
  const button = page.getByRole("button",{name:"캡처 범위 지정",exact:true});
  await button.click();
  let during;
  for (let i=0;i<25;i++) {
    await sleep(180);
    try { during=nativeWindow(); if(during.minimized)break; } catch{}
  }
  assert.ok(during?.minimized,
    "The real BANDO HUB window must minimize before Windows region selection");
  assert.equal(await page.getByRole("status",{name:"OCR 진행 상태"}).count(),0,
    "OCR progress must not appear while the employee is selecting a region");
  const sent = nativeWindow("escape");
  assert.equal(sent,"ESC_SENT");
  let after;
  for (let i=0;i<35;i++) {
    await sleep(220);
    try { after=nativeWindow(); if(!after.minimized)break; } catch{}
  }
  assert.ok(after && !after.minimized,"ESC must restore the original app window");
  assert.equal(after.maximized,before.maximized,"Previous maximize state changed");
  assert.ok(after.rect.every((n,i)=>Math.abs(n-before.rect[i])<=8),
    "BANDO window bounds changed after cancel: "+JSON.stringify({before,after}));
  await page.waitForFunction(()=>{
    const button=[...document.querySelectorAll("button")]
      .find(b=>b.textContent?.trim()==="캡처 범위 지정");
    return button && !button.disabled;
  },null,{timeout:15000});
  assert.equal(await page.getByRole("dialog",{name:"OCR 실행 오류"}).count(),0);
  assert.equal(await page.getByRole("status",{name:"OCR 진행 상태"}).count(),0);
  assert.equal(await page.locator(".source-document").innerText(),currentText);
  const unchanged=await page.evaluate(async()=>window.bando.load());
  assert.equal(unchanged.revision,persisted.revision);
  assert.equal(JSON.stringify(unchanged.orders),JSON.stringify(persisted.orders));
  record("SIM32_OCR_ESC_NATIVE_WINDOWS_RESTORED",{
    before,after,changedOrderCount:0,
    captureCancellation:"ESC",nativeTextUnchanged:true
  });
  await snap(page,"32-native-ocr-esc-restored");
}
