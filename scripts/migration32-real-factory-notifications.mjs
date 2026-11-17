// Real Windows CI-only Tauri/WebView2 notification and XLSX ingestion audit.
// Creates files only in the disposable developer-test workspace.
// Reused by the focused "factory" Windows GUI run on its own fresh TEST SQLite.
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { dirname, join } from "node:path";
import ExcelJS from "exceljs";

function row(tracking) {
  const values=Array(17).fill("");
  values[0]="가상 수취인";
  values[2]="서울특별시 테스트로 101";
  values[3]="010-1111-2222";
  values[6]=4000;
  values[16]=tracking;
  return values;
}
async function writeInbox(inbox,name,waybills) {
  const workbook=new ExcelJS.Workbook();
  const sheet=workbook.addWorksheet("로젠");
  sheet.addRow(Array.from({length:17},(_,i)=>"필드 "+(i+1)));
  waybills.forEach(number=>sheet.addRow(row(number)));
  await workbook.xlsx.writeFile(join(inbox,name));
}
export async function probeFactoryReplyNotifications(page,{baseline,record,snap}) {
  assert.equal(process.platform,"win32");
  assert.equal(process.env.GITHUB_ACTIONS,"true");
  const before=await page.evaluate(()=>window.bando.load());
  const inbox=join(dirname(baseline),"developer-test","Excel","공장 회신 수신");
  await fs.mkdir(inbox,{recursive:true});
  const open=()=>page.getByRole("button",{name:/^알림(?: \d+건)?$/});
  await open().waitFor({state:"visible",timeout:20000});
  assert.equal(await open().locator("svg").count(),1,"Header notification must show a bell icon");
  assert.equal(await page.locator(".app-notice").count(),0,"Floating toast is not allowed");

  // Structural failure: every Q waybill missing. Invalid file cannot alter DB.
  await writeInbox(inbox,"sim32-missing-q.xlsx",[null]);
  await page.waitForFunction(()=>[...document.querySelectorAll("button")]
    .some(b=>/^알림 \d+건$/.test(b.getAttribute("aria-label")||"")),
    null,{timeout:50000});
  await open().click();
  const modal=page.getByRole("dialog",{name:"알림"});
  await modal.waitFor();
  await modal.locator(".notification-center-item").first().click();
  await modal.getByText(/Q열 운송장번호 데이터가 없습니다/).waitFor();
  assert.equal(await page.locator(".app-notice").count(),0);
  const afterRejected=await page.evaluate(()=>window.bando.load());
  assert.equal(afterRejected.revision,before.revision,"Rejected file must not write SQLite");
  record("SIM32_INVALID_FACTORY_REPLY_NOTIFICATION",{
    popup:true,filename:"sim32-missing-q.xlsx",databaseUnchanged:true
  });
  await snap(page,"32-factory-reply-invalid-notification");
  await modal.getByRole("button",{name:/닫기/}).first().click();

  // Safe numeric Q must be readable; unsafe 16-digit Q held as a row.
  await writeInbox(inbox,"sim32-numeric-waybills.xlsx",[123456789012,1234567890123456]);
  await page.waitForFunction(async()=>
    (await window.bando.load()).factoryReplies?.some(r=>
      r.filename==="sim32-numeric-waybills.xlsx"
    ),null,{timeout:55000});
  await open().click();
  await modal.locator(".notification-center-item").first().click();
  await modal.getByText(/15자리 초과|정밀도/).waitFor();
  const after=await page.evaluate(()=>window.bando.load());
  const reply=after.factoryReplies.find(r=>r.filename==="sim32-numeric-waybills.xlsx");
  assert.equal(reply.rows.length,2);
  assert.equal(reply.rows[0].tracking,"123456789012");
  assert.ok(reply.rows[1].issue?.includes("15자리"));
  assert.deepEqual(after.workflow.shipments,before.workflow.shipments);
  assert.deepEqual(after.orders,before.orders);
  assert.equal(await page.locator(".app-notice").count(),0);
  record("SIM32_NUMERIC_WAYBILL_ROW_HOLD",{
    safeNumericRead:reply.rows[0].tracking,
    unsafeRowHeld:reply.rows[1].issue,
    databaseShipmentsUnchanged:true
  });
  await snap(page,"32-factory-reply-numeric-details");
  await modal.getByRole("button",{name:/닫기/}).first().click();

  if (process.env.BANDO_PERSISTENT_NOTICE_AUDIT === "true") {
    const names=["sim32-missing-q.xlsx","sim32-numeric-waybills.xlsx"];
    const initial = await page.evaluate(async () => window.bando.notificationList());
    const saved = initial.filter((notice) => names.includes(notice.filename));
    assert.equal(saved.length,2,"Both factory notices must be present in SQLite");
    // The modal click above marked both notifications read, durably.
    await page.waitForFunction(async(ids)=>{
      const rows=await window.bando.notificationList();
      return ids.every(id=>rows.find(n=>n.id===id)?.read===true);
    }, saved.map(n=>n.id), {timeout:15000});
    // Prevent the auto watcher from reimporting the intentionally rejected
    // spreadsheet when the UI loads again.
    for (const filename of names) {
      await fs.rm(join(inbox,filename),{force:true});
    }
    const snapshot=await page.evaluate(async()=>window.bando.load());
    await page.reload({waitUntil:"domcontentloaded"});
    await open().waitFor({state:"visible",timeout:45000});
    await page.waitForFunction(async(ids)=>{
      const rows=await window.bando.notificationList();
      return ids.every(id=>rows.find(n=>n.id===id)?.read===true);
    }, saved.map(n=>n.id), {timeout:15000});
    await open().click();
    const reloaded=page.getByRole("dialog",{name:"알림"});
    await reloaded.waitFor({state:"visible"});
    for (const filename of names) {
      await reloaded.locator(".notification-center-item").filter({
        hasText:filename,
      }).waitFor({state:"visible"});
    }
    assert.equal(await reloaded.locator(".notification-center-item.unread")
      .filter({hasText:"sim32-missing-q.xlsx"}).count(),0);
    const afterReload=await page.evaluate(async()=>window.bando.load());
    assert.equal(afterReload.revision,snapshot.revision,
      "Notification reload/read must not change business revision");
    record("SIM32_SQLITE_NOTIFICATIONS_SURVIVE_UI_RELOAD",{
      retained:saved.map(n=>({filename:n.filename,read:true})),
      businessRevisionUnchanged:true,
    });
    await snap(page,"32-sqlite-notification-reopen");
    await reloaded.getByRole("button",{name:"목록 비우기"}).click();
    await page.waitForFunction(async()=>
      (await window.bando.notificationList()).length===0,
    null,{timeout:15000});
    await reloaded.getByRole("button",{name:/닫기/}).first().click();
    await page.reload({waitUntil:"domcontentloaded"});
    await open().waitFor({state:"visible",timeout:45000});
    assert.equal((await page.evaluate(async()=>window.bando.notificationList())).length,0,
      "Notification clear must remain cleared across restart");
    assert.equal((await page.evaluate(async()=>window.bando.load())).revision,
      snapshot.revision,"Clearing inbox must not modify orders or business revision");
    record("SIM32_SQLITE_NOTIFICATIONS_CLEAR_SURVIVES_RELOAD",{
      remaining:0,businessRevisionUnchanged:true,
    });
  }
}
