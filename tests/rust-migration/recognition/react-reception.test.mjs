/**
 * Actual native Rust analyze_order -> production React reception merge adapter.
 * This test does not call the legacy TypeScript parseEvidence() implementation.
 */
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { tsImport } from "tsx/esm/api";

const root=fileURLToPath(new URL("../../../",import.meta.url));
const cargo=fileURLToPath(new URL("../integration/Cargo.toml",import.meta.url));
const fixture=readFileSync(new URL("../../audit-20-order-samples.test.mjs",import.meta.url),"utf8");
const start=fixture.indexOf("const samples=")+"const samples=".length;
const end=fixture.indexOf("\n];",start)+2;
assert.ok(start>15 && end>start,"original fixture syntax changed");
const samples=JSON.parse(fixture.slice(start,end));
const state=JSON.parse(readFileSync(
  new URL("../../../src-tauri/resources/state-seed.json",import.meta.url),"utf8"));
const client={id:"ci-dealer",name:"가상거래처",tier:"wholesale",
  overrides:{},active:true,payment:"PREPAID",identifiers:"",shippingFeeExcluded:false};
state.clients=[...state.clients.filter(c=>c.id!==client.id),client];
const { newReception }=await tsImport("../../../src/orders/reception.ts",import.meta.url);
const { applyRustReceptionAnalysis,assertRustReceptionAnalysis,receptionAnalysisAllowed }=
  await tsImport("../../../src/orders/rust-reception.ts",import.meta.url);

const selected=Array.from({length:20},(_,i)=>i+1);
const inputs=selected.map(id=>({id,raw:samples[id-1].raw}));
inputs.push({id:"13-edited",raw:samples[12].raw+" "});
const rustRequests=inputs.map(x=>({domain:"recognition",operation:"analyze_order",
  payload:{raw:x.raw,client,state}}));
const run=spawnSync("cargo",["run","--quiet","--manifest-path",cargo,
  "--bin","reception_runner"],{
  cwd:root,input:JSON.stringify(rustRequests),encoding:"utf8",
  timeout:900_000,maxBuffer:60*1024*1024,
});
if(run.error)throw run.error;
assert.equal(run.status,0,"native integration runner must compile and run:\n"+run.stderr);
const answers=JSON.parse(run.stdout);
assert.equal(answers.length,inputs.length);
const natives=new Map();
inputs.forEach((input,index)=>{
  assert.equal(answers[index].ok,true,
    "Rust rejected original fixture "+input.id+": "+answers[index].error);
  assertRustReceptionAnalysis(answers[index].value,input.raw);
  natives.set(input.id,answers[index].value);
});

function orderFor(id) {
  const order=newReception(client.id);
  order.id="draft-"+id;
  order.raw="";
  order.analysisRaw="";
  order.analysisClientId="";
  return order;
}
function apply(previous,id,raw=samples[id-1].raw) {
  return applyRustReceptionAnalysis(previous,natives.get(id),{
    raw,state,pricingClient:client,at:"2026-11-13T00:00:00.000Z"
  });
}

test("all twenty original orders pass the Rust-to-React reception boundary",()=>{
  for(let id=1;id<=20;id++){
    const before=orderFor(id);
    const after=apply(before,id);
    assert.equal(after.recipients.length,samples[id-1].expectedRecipients,
      "sample "+id+" recipient count");
    assert.deepEqual(
      after.recipients.flatMap(r=>r.items.map(i=>i.quantity)).sort(),
      samples[id-1].expectedQuantities.slice().sort(),
      "sample "+id+" shipment quantities",
    );
    assert.equal(after.analysisEvidence.length,natives.get(id).candidates.length);
    for(const r of after.recipients)
      for(const item of r.items)
        for(const span of item.sources??[]){
          assert.equal(span.recipientId,r.id,"sample "+id+" stable recipient reference");
          assert.equal(span.targetId,item.id,"sample "+id+" stable item reference");
        }
  }
});

test("native parser's two-recipient shipment attaches to the real reception draft",()=>{
  const before=orderFor(13);
  const after=apply(before,13);
  assert.notEqual(after,before);
  assert.equal(after.recipients.length,2);
  assert.deepEqual(
    after.recipients.flatMap(r=>r.items.map(i=>i.quantity)).sort(),
    samples[12].expectedQuantities.slice().sort(),
  );
  assert.equal(after.analysisSnapshot.ruleVersion,"rust-recognition-v1");
  assert.equal(after.analysisSnapshot.raw,samples[12].raw);
  assert.equal(after.analysisEvidence.length,natives.get(13).candidates.length);
  assert.equal(after.analysisRaw,samples[12].raw);
  assert.equal(after.analysisClientId,client.id);
  assert.equal(before.recipients.length,0,"input draft must remain immutable");
  for(const r of after.recipients){
    for(const item of r.items) {
      for(const source of item.sources??[]) {
        assert.equal(source.recipientId,r.id);
        assert.equal(source.targetId,item.id);
      }
    }
  }
});

test("native parser rejects false date products in legacy TS-defective 19/20",()=>{
  for(const id of [19,20]){
    const after=apply(orderFor(id),id);
    assert.equal(after.recipients.length,1);
    assert.equal(after.recipients[0].items.length,1);
    assert.equal(after.recipients[0].items[0].quantity,1);
    assert.ok(after.recipients[0].items[0].productId);
    assert.ok(after.recipients[0].address);
    assert.ok(after.recipients[0].phone||after.recipients[0].mobile);
  }
});

test("employee-confirmed address, stable recipient and item IDs survive re-analysis",()=>{
  const first=apply(orderFor(13),13);
  const prior=structuredClone(first);
  const oldRecipient=prior.recipients[0];
  oldRecipient.confirmedFields=["address"];
  oldRecipient.address="경기도 의정부시 시민로 100 직원확정";
  const oldItem=oldRecipient.items[0];
  oldItem.name="직원이 직접 확정한 상품명";
  oldItem.manuallyConfirmed=true;
  oldItem.quantityApproval={
    at:"2026-11-13T00:00:00.000Z",actor:"직원",quantity:oldItem.quantity??1,
  };

  const next=applyRustReceptionAnalysis(prior,natives.get("13-edited"),{
    raw:samples[12].raw+" ",state,pricingClient:client,
    at:"2026-11-13T00:00:01.000Z",
  });
  assert.equal(next.recipients[0].id,oldRecipient.id);
  assert.equal(next.recipients[0].address,oldRecipient.address);
  assert.equal(next.recipients[0].items[0].id,oldItem.id);
  assert.equal(next.recipients[0].items[0].name,oldItem.name);
  assert.equal(next.recipients[0].items[0].manuallyConfirmed,true);
  assert.equal(next.recipients[0].items[0].quantityApproval?.actor,"직원");
  assert.equal(prior.analysisRaw,samples[12].raw,"previous state is not mutated");
  for(const item of next.recipients[0].items){
    for(const span of item.sources??[]){
      assert.equal(span.recipientId,oldRecipient.id);
      assert.equal(span.targetId,item.id);
    }
  }
});

test("stale or locked native analysis does not overwrite an already reviewed draft",()=>{
  const parsed=apply(orderFor(1),1);
  assert.equal(receptionAnalysisAllowed(parsed,samples[0].raw,client.id),false);
  assert.strictEqual(apply(parsed,1),parsed);
  for(const mutation of [
    o=>{o.confirmedAt="2026-11-13T00:00:00.000Z";},
    o=>{o.pricesLocked=true;},
    o=>{o.status="EXPORTED";},
    o=>{o.deletedAt="2026-11-13T00:00:00.000Z";},
    o=>{o.manualClassification={blocks:[],assignments:{}};},
  ]){
    const before=structuredClone(orderFor(1));
    mutation(before);
    assert.strictEqual(apply(before,1),before,"reviewed draft must not change");
  }
});

test("corrupt native evidence is rejected instead of silently falling back to TypeScript",()=>{
  const corrupted=structuredClone(natives.get(1));
  corrupted.candidates[0].text="not the source";
  assert.throws(()=>applyRustReceptionAnalysis(orderFor(1),corrupted,{
    raw:samples[0].raw,state,pricingClient:client,
    at:"2026-11-13T00:00:00.000Z",
  }),/원문 근거/);
  assert.throws(()=>assertRustReceptionAnalysis({recipients:[]},samples[0].raw),/형식/);
});


test("compiled Rust reception merge exactly matches original TS on all 20 recognitions and employee edits",()=>{
  const at="2026-11-13T00:00:01.000Z";
  const samplesToCompare=inputs.slice(0,20).map(entry=>({
    label:"initial-"+entry.id,
    previous:orderFor(entry.id),
    evidence:natives.get(entry.id),
    raw:entry.raw,
  }));
  const prior=apply(orderFor(13),13);
  prior.recipients[0].confirmedFields=["address","name","mobile"];
  prior.recipients[0].name="직원 확정 수취인";
  prior.recipients[0].address="경기도 의정부시 시민로 100 직원확정";
  prior.recipients[0].mobile="010-2222-3333";
  prior.recipients[0].items[0].name="직원이 직접 확정한 상품명";
  prior.recipients[0].items[0].manuallyConfirmed=true;
  prior.recipients[0].items[0].quantityApproval={
    at,actor:"직원",quantity:prior.recipients[0].items[0].quantity??1,
  };
  samplesToCompare.push({
    label:"employee-confirmed-reanalysis",
    previous:prior,
    evidence:natives.get("13-edited"),
    raw:samples[12].raw+" ",
  });
  const locked=structuredClone(prior);
  locked.status="EXPORTED";
  samplesToCompare.push({
    label:"exported-locked",previous:locked,
    evidence:natives.get("13-edited"),raw:samples[12].raw+" ",
  });
  const requests=samplesToCompare.map(row=>({
    domain:"rules",operation:"merge_reception_analysis",
    payload:{previous:row.previous,evidence:row.evidence,raw:row.raw,clientId:client.id,at},
  }));
  const result=spawnSync("cargo",["run","--quiet","--manifest-path",cargo,
    "--bin","reception_runner"],{
    cwd:root,input:JSON.stringify(requests),encoding:"utf8",
    timeout:900_000,maxBuffer:60*1024*1024,
  });
  if(result.error)throw result.error;
  assert.equal(result.status,0,"Rust reception merge runner:\n"+result.stderr);
  const answers=JSON.parse(result.stdout);
  assert.equal(answers.length,samplesToCompare.length);
  for(const [index,row] of samplesToCompare.entries()){
    const native=answers[index];
    assert.equal(native.ok,true,"Rust merge rejected "+row.label+": "+native.error);
    const original=applyRustReceptionAnalysis(row.previous,row.evidence,{
      raw:row.raw,state,pricingClient:client,at,
    });
    const normalized=value=>JSON.parse(JSON.stringify(value));
    assert.deepEqual(normalized(native.value.order),normalized(original),
      "Rust and original TS reception merge differ: "+row.label);
  }
});
