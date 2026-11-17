import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import test from "node:test";
import {transform} from "esbuild";

const code=readFileSync(new URL("../src/shared/native-read-coalescing.ts",import.meta.url),"utf8");
const {code:js}=await transform(code,{loader:"ts",format:"esm"});
const {coalescePureNativeRead}=await import("data:text/javascript;base64,"+Buffer.from(js).toString("base64"));

test("simultaneous equivalent native Rust reads share one actual IPC",async()=>{
  let complete, calls=0;
  const run=()=>{calls++;return new Promise(resolve=>{complete=resolve;});};
  const input={state:{revision:1,clients:[{id:"a"}]},query:"same",raw:""};
  const first=coalescePureNativeRead("catalog_clients","client_candidates",input,run);
  const second=coalescePureNativeRead("catalog_clients","client_candidates",input,run);
  assert.strictEqual(first,second);
  assert.equal(calls,1);
  complete({ids:["a"]});
  assert.deepEqual(await second,{ids:["a"]});
  // The settled short-lived result also serves equal inputs.
  assert.deepEqual(await coalescePureNativeRead("catalog_clients","client_candidates",input,run),{ids:["a"]});
  assert.equal(calls,1);
  // Changed business state or search term must trigger another native read.
  await coalescePureNativeRead("catalog_clients","client_candidates",
    {state:{revision:2,clients:[{id:"a"}]},query:"same",raw:""},
    async()=>{calls++;return {ids:[]};});
  assert.equal(calls,2);
});
test("workflow reconciliation and order edit actions always execute natively",async()=>{
  let calls=0;
  const run=async()=>{calls++;return {state:{}};};
  for(const action of ["prepare_order_edit","calculate_order_shipping"]){
    const arg={action,state:{revision:1}};
    await coalescePureNativeRead("rules","order_edit_rules",arg,run);
    await coalescePureNativeRead("rules","order_edit_rules",arg,run);
  }
  await coalescePureNativeRead("rules","reconcile_workflow",{state:{},draft:{}},run);
  await coalescePureNativeRead("rules","reconcile_workflow",{state:{},draft:{}},run);
  assert.equal(calls,6);
});
test("rejecting native reads are not cached and can be retried",async()=>{
  let calls=0;
  const arg={order:{id:"reject-fixture"}};
  const run=()=>{calls++;return calls===1?Promise.reject(Error("temporary")):Promise.resolve({issues:[]});};
  await assert.rejects(coalescePureNativeRead("rules","order_issues_with_actions",arg,run),/temporary/);
  assert.deepEqual(await coalescePureNativeRead("rules","order_issues_with_actions",arg,run),{issues:[]});
  assert.equal(calls,2);
});
