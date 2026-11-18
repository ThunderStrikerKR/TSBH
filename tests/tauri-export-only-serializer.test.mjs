import {test} from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read = path => readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("factory and store live modules are serializers only, not TypeScript business rule engines",()=>{
  const factory=read("src/factory-export.ts"),store=read("src/store-export.ts");
  const workflow=read("src/WorkflowQueue.tsx"),native=read("src/native-export.ts");
  for(const [label,src] of [["factory",factory],["store",store]]){
    assert.doesNotMatch(src,/function (?:exportRow|storeRow)\s*\(/,label);
    assert.doesNotMatch(src,/function (?:previewFactoryExport|previewStoreExport|recordFactoryExport|recordStoreExport)(?:Prepared)?\s*\(/,label);
    assert.doesNotMatch(src,/\.status\s*=\s*"EXPORTED"/,label);
    assert.doesNotMatch(src,/shipment\.step|\.legacyArchived|\.transferred/,label);
  }
  assert.match(factory,/export async function createFactoryWorkbook/);
  assert.match(store,/export async function createStoreWorkbook/);
  assert.match(workflow,/"output", "prepare_factory_export"/);
  assert.match(workflow,/"output", "prepare_store_export"/);
  assert.match(native,/"output", "finalize_export"/);
  for (const file of ["factory-export","store-export"]){
    const oracle=read("tests/rust-migration/oracles/"+file+".ts");
    assert.match(oracle,/function (?:previewFactoryExport|previewStoreExport)\(/);
    assert.match(oracle,/function (?:recordFactoryExport|recordStoreExport)\(/);
  }
});
