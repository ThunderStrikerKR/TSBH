// Real Windows WebView2 business-domain regression; the entrypoint seeds an isolated TEST SQLite.
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { probeFactoryReplyNotifications } from "../migration32-real-factory-notifications.mjs";

export async function run(page,{record,snap,paste,chooseDealer}){
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
