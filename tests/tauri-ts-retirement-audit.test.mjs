import test from "node:test";
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {existsSync,readFileSync} from "node:fs";
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
  assert.ok(output.inventory.some(row=>row.module==="src/product-price-xlsx.ts" && !row.exists && row.classification==="RETIRED_TO_TEST_ORACLE"),
    "Obsolete standalone price workbook must be retired to a comparison-only oracle");
  assert.ok(Number.isInteger(output.reachableLegacyModules));
  assert.ok(output.reachableLegacyModules>=0);
  assert.equal(output.retiredToTestOracle,19,
    "Retired TypeScript business engines must remain outside production src");
  assert.ok(output.inventory.some(row=>
    row.module==="src/shipping.ts" && !row.reachableFromApp),
    "Legacy TS shipping recommendation engine must stay out of the live frontend graph");
  assert.ok(output.inventory.some(row=>
    row.module==="src/automatic-message.ts"&&!row.exists&&!row.reachableFromApp),
    "Historical message generator must no longer exist in production src");
  const retired=[
    ["automatic-message.ts","automatic-message.ts"],
    ["message-model.ts","message-model.ts"],
    ["group-trash.ts","group-trash.ts"],
    ["title-clients.ts","title-clients.ts"],
    ["catalog-price-sync.ts","catalog-price-sync.ts"],
    ["official-product-import.ts","official-product-import.ts"],
    ["issue-actions.ts","issue-actions.ts"],
    ["corrections.ts","corrections.ts"],
    ["replies.ts","replies.ts"],
    ["working-raw.ts","working-raw.ts"],
    ["orders/retail.ts","retail.ts"],
    ["orders/passive-pricing.ts","passive-pricing.ts"],
    ["orders/manual-classification.ts","manual-classification.ts"],
    ["factory-replies.ts","factory-replies.ts"],
    ["factory-reply-workbook.ts","factory-reply-workbook.ts"],
    ["app/order-history.ts","order-history.ts"],
    ["app/settlement-report.ts","settlement-report.ts"],
    ["orders/reception.ts","reception.ts"],
    ["product-price-xlsx.ts","product-price-xlsx.ts"],
  ];
  for(const [name,oracle] of retired){
    assert.equal(existsSync(fileURLToPath(new URL("../src/"+name,import.meta.url))),false,
      name+" must not remain in production source");
    assert.equal(existsSync(fileURLToPath(new URL("../tests/rust-migration/oracles/"+oracle,import.meta.url))),true,
      name+" must be retained in the TypeScript test-oracle directory");
  }
  assert.ok(output.inventory.some(row=>
    row.module==="src/factory-export.ts"&&row.exists&&row.reachableFromApp),
    "ExcelJS serializer remains a legitimate TS module");
  const clientFile=readFileSync(new URL("../src/client-workbook.ts",import.meta.url),"utf8");
  assert.doesNotMatch(clientFile,/function (?:createCandidate|collision)|export function previewClientRows|export function applyClientWorkbook/);
  assert.match(clientFile,/export async function readClientWorkbookRows/);
  assert.match(readFileSync(new URL("../tests/rust-migration/oracles/client-workbook.ts",import.meta.url),"utf8"),/export function previewClientRows/);

  for(const name of ["factory-export.ts","store-export.ts"]){
    const row=output.inventory.find(row=>row.module==="src/"+name);
    assert.equal(row?.classification,"LIVE_XLSX_SERIALIZER_ONLY",
      name+" must remain a serializer, not a TypeScript business rule engine");
  }
  assert.equal(output.liveSerializersOnly,2);
});
test("native reception and issue validation never depend on legacy TS from app",()=>{
  const run=spawnSync(process.execPath,[script,"--guard-native-recognition"],{
    cwd:root,encoding:"utf8",timeout:60000,maxBuffer:8*1024*1024,
  });
  assert.equal(run.status,0,run.stderr);
  const line=run.stdout.split("\n").find(line=>line.startsWith("BANDO_NATIVE_SOURCE_BOUNDARY "));
  assert.ok(line,"Native recognition source boundary must be reported");
  const audit=JSON.parse(line.slice("BANDO_NATIVE_SOURCE_BOUNDARY ".length));
  assert.deepEqual(audit.guarded,["src/parser.ts","src/validation.ts"]);
  assert.deepEqual(audit.unexpectedRuntimeDependencies,[]);
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

test("product and unified Excel source only transport/serialize Rust-owned business data",()=>{
  const sync=readFileSync(new URL("../src/product-sync-workbook.ts",import.meta.url),"utf8");
  const unified=readFileSync(new URL("../src/product-unified-workbook.ts",import.meta.url),"utf8");
  for(const fn of ["previewProductSyncRows","applyProductSyncWorkbook","previewProductSyncWorkbook"])
    assert.doesNotMatch(sync,new RegExp("\\b"+fn+"\\s*\\("));
  for(const fn of ["previewUnifiedProductRows","previewUnifiedProductWorkbook","applyUnifiedProductWorkbook"])
    assert.doesNotMatch(unified,new RegExp("\\b"+fn+"\\s*\\("));
  assert.match(unified,/export async function exportUnifiedProductWorkbook/);
  assert.match(unified,/export async function readUnifiedProductWorkbookRows/);
  assert.match(readFileSync(new URL("../tests/rust-migration/oracles/product-unified-workbook.ts",import.meta.url),"utf8"),/export function previewUnifiedProductRows/);
});

test("live registration workbook remains ExcelJS-only; business preview and apply are Rust-owned",()=>{
  const live=readFileSync(new URL("../src/product-registration-xlsx.ts",import.meta.url),"utf8");
  const oracle=readFileSync(new URL("../tests/rust-migration/oracles/product-registration-xlsx.ts",import.meta.url),"utf8");
  assert.match(live,/export async function exportRegistrationTemplate/);
  assert.match(live,/export async function exportRegistrationResults/);
  assert.doesNotMatch(live,/export async function previewRegistrationWorkbook|export function prepareRegistrationCommit/);
  assert.match(oracle,/export async function previewRegistrationWorkbook/);
  assert.match(oracle,/export function prepareRegistrationCommit/);
});
