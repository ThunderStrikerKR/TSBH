import test from "node:test";
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {existsSync} from "node:fs";
import {fileURLToPath} from "node:url";

const root=fileURLToPath(new URL("../",import.meta.url));
const script=fileURLToPath(new URL("../scripts/audit-ts-business-retirement.mjs",import.meta.url));
test("TypeScript business retirement audit reads actual app source import graph",()=>{
  const run=spawnSync(process.execPath,[script,"--json"],{
    cwd:root,encoding:"utf8",timeout:60000,maxBuffer:8*1024*1024,
  });
  assert.equal(run.status,0,run.stderr);
  const output=JSON.parse(run.stdout);
  assert.equal(output.entry,"src/main.tsx");
  assert.ok(output.inspectedSourceFiles>100);
  assert.ok(output.trackedLegacyModules>=25);
  assert.ok(Number.isInteger(output.reachableLegacyModules));
  assert.ok(output.reachableLegacyModules>=0);
  assert.equal(output.retiredToTestOracle,6,
    "Retired TypeScript business engines must remain outside production src");
  assert.ok(output.inventory.some(row=>
    row.module==="src/automatic-message.ts"&&!row.exists&&!row.reachableFromApp),
    "Historical message generator must no longer exist in production src");
  for(const name of ["automatic-message.ts","message-model.ts","group-trash.ts",
    "title-clients.ts","catalog-price-sync.ts","official-product-import.ts"]){
    assert.equal(existsSync(fileURLToPath(new URL("../src/"+name,import.meta.url))),false,
      name+" must not remain in production source");
    assert.equal(existsSync(fileURLToPath(new URL("../tests/rust-migration/oracles/"+name,import.meta.url))),true,
      name+" must be retained only as a frozen TS test oracle");
  }
  assert.ok(output.inventory.some(row=>
    row.module==="src/factory-export.ts"&&row.exists&&row.reachableFromApp),
    "ExcelJS serializer remains a legitimate TS module");
});
test("strict TS retirement audit follows the actual live dependency count",()=>{
  const inspection=spawnSync(process.execPath,[script,"--json"],{
    cwd:root,encoding:"utf8",timeout:60000,
  });
  assert.equal(inspection.status,0,inspection.stderr);
  const remaining=JSON.parse(inspection.stdout).reachableLegacyModules;
  const run=spawnSync(process.execPath,[script,"--strict"],{
    cwd:root,encoding:"utf8",timeout:60000,
  });
  assert.equal(run.status,remaining>0?1:0);
  if(remaining>0){
    assert.match(run.stderr,/BLOCKED: TS business runtime dependencies remain/);
  } else {
    assert.doesNotMatch(run.stderr,/BLOCKED:/);
  }
});
