import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DOMAIN_SCOPES, selectWindowsScope } from "../scripts/classify-dev-windows-smoke.mjs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");

test("each domain has an independent real Tauri Windows GUI entry",()=>{
  const source=read("scripts/ci-dev-domain-gui.mjs");
  for(const domain of DOMAIN_SCOPES){
    assert.match(source,new RegExp("async function "+domain+"\\(page\\)"));
  }
  assert.match(source,/initial\.orders\.length\|\|initial\.clients\.length/);
  assert.match(source,/await api\.workspaceMode\(\)/);
  assert.match(source,/chromium\.connectOverCDP/);
  assert.match(source,/probeTwoConsecutiveOrders/);
  assert.match(source,/probeFactoryReplyNotifications/);
  assert.match(source,/BANDO_DEV_DOMAIN_PASS/);
});
test("domain runs never masquerade as full GUI or window restart checks",()=>{
  const runner=read("scripts/migration32-dev-windows-smoke.ps1");
  for(const domain of DOMAIN_SCOPES)assert.ok(runner.includes('"'+domain+'"'));
  assert.match(runner,/scripts\/ci-dev-domain-gui\.mjs \$Scope/);
  assert.match(runner,/NOT_RUN_DOMAIN_SCOPE/);
  assert.match(runner,/if \(\$Scope -eq "full" -or \$Scope -eq "window"\)/);
  assert.equal(selectWindowsScope(["src-tauri/src/business_core/rules/pricing.rs"],null,null),"full");
});
