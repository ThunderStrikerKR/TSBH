import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const source=(name)=>readFileSync(new URL("../"+name,import.meta.url),"utf8");

test("Rust carrier collect freight code remains distinct",()=>{
  const rust=source("src-tauri/src/business_core/rules/workflow_preview.rs");
  assert.match(rust, /let freight_code = workflow::shipment_freight_code\(order, recipient\)/);
  assert.match(rust, /"freightCode": freight_code/);
  assert.match(rust, /freight_code == "020" \|\| excluded/);
});
