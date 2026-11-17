import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { DOMAIN_SCOPES, selectWindowsScope } from "../scripts/classify-dev-windows-smoke.mjs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");

test("each domain has an independent real Tauri Windows GUI entry",()=>{
  const source=read("scripts/ci-dev-domain-gui.mjs");
  for(const domain of DOMAIN_SCOPES){
    const module=read("scripts/ci-domains/"+domain+".mjs");
    assert.match(module,/export async function run\(page,\{record,snap,paste,chooseDealer\}\)/);
  }
  assert.match(source,/initial\.orders\.length\|\|initial\.clients\.length/);
  assert.match(source,/await api\.workspaceMode\(\)/);
  assert.match(source,/chromium\.connectOverCDP/);
  assert.ok(source.includes("127\\.0\\.0\\.1:5173"));
  assert.match(source,/tabs\.some\(t=>t\.type==="page"\)/);
  assert.match(read("scripts/ci-domains/orders.mjs"),/probeTwoConsecutiveOrders/);
  assert.match(read("scripts/ci-domains/factory.mjs"),/probeFactoryReplyNotifications/);
  assert.match(source,/BANDO_DEV_DOMAIN_PASS/);
  assert.match(source,/import\("\.\/ci-domains\/"/);
});
test("domain runs never masquerade as full GUI or window restart checks",()=>{
  const runner=read("scripts/migration32-dev-windows-smoke.ps1");
  for(const domain of DOMAIN_SCOPES)assert.ok(runner.includes('"'+domain+'"'));
  assert.match(runner,/scripts\/ci-dev-domain-gui\.mjs \$Scope/);
  assert.match(runner,/NOT_RUN_DOMAIN_SCOPE/);
  assert.match(runner,/if \(\$Scope -eq "full" -or \$Scope -eq "window"\)/);
  assert.equal(selectWindowsScope(["src-tauri/src/business_core/rules/pricing.rs"],null,null),"full");
});

test("all five independently owned domain modules parse as Node ESM without running Windows",()=>{
  for(const domain of DOMAIN_SCOPES){
    const path=new URL("../scripts/ci-domains/"+domain+".mjs",import.meta.url);
    const checked=spawnSync(process.execPath,["--check",fileURLToPath(path)],{encoding:"utf8"});
    assert.equal(checked.status,0,domain+" syntax failed: "+checked.stderr);
  }
});
