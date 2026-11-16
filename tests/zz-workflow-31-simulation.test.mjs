/**
 * Live business-flow simulation 31: actual compiled Rust, sanitized purchase orders.
 * Stops at the first failing business boundary. No real customer or production DB.
 */
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { tsImport } from "tsx/esm/api";

const root=fileURLToPath(new URL("../",import.meta.url));
const manifest=fileURLToPath(new URL("../tests/rust-migration/integration/Cargo.toml",import.meta.url));
const fixture=readFileSync(new URL("./audit-20-order-samples.test.mjs",import.meta.url),"utf8");
const start=fixture.indexOf("const samples=")+"const samples=".length;
const end=fixture.indexOf("\n];",start)+2;
assert.ok(start>15 && end>start,"sanitized order fixtures unavailable");
const samples=JSON.parse(fixture.slice(start,end));
const {newReception}=await tsImport("../src/orders/reception.ts",import.meta.url);
const clock="2026-11-16T09:00:00+09:00";
const dealer={
  id:"sim-dealer-31",name:"가상대리점",displayName:"가상대리점",
  tier:"wholesale",payment:"PREPAID",overrides:{},active:true,
  identifiers:"",shippingFeeExcluded:false,preferredAccountId:"sim-account-31"
};
const seed=JSON.parse(readFileSync(new URL("../src-tauri/resources/state-seed.json",import.meta.url),"utf8"));
seed.clients=[...seed.clients.filter(c=>c.id!==dealer.id),dealer];
seed.settings={...seed.settings,operatorName:"시험 직원",
  accounts:[{id:"sim-account-31",bank:"시험은행",number:"123-456-789",
    holder:"반도스탠드"}],defaultAccountId:"sim-account-31"};

function native(domain,operation,payload){
  const result=spawnSync("cargo",["run","--quiet","--manifest-path",manifest,
    "--bin","reception_runner"],{
    cwd:root,input:JSON.stringify([{domain,operation,payload}]),
    encoding:"utf8",timeout:900000,maxBuffer:64*1024*1024
  });
  if(result.error)throw result.error;
  assert.equal(result.status,0,"Rust runner failed: "+result.stderr);
  const [answer]=JSON.parse(result.stdout);
  assert.equal(answer.ok,true,domain+"."+operation+": "+answer.error);
  return answer.value;
}
function checkpoint(name,details){
  console.log("SIM31_PASS "+name+" "+JSON.stringify(details??{}));
}
function reconcile(state,draft){
  return native("rules","reconcile_workflow",{
    state,draft,now:clock,
    eventIds:Array.from({length:512},(_,i)=>"sim-event-31-"+i)
  }).state;
}
function analyzeAndMerge(raw,id){
  const evidence=native("recognition","analyze_order",{raw,state:seed,client:dealer});
  const initial=newReception(dealer.id);
  initial.id="sim-order-31-"+id;
  const merged=native("rules","merge_reception_analysis",{
    previous:initial,evidence,raw,clientId:dealer.id,at:clock
  }).order;
  return {evidence,initial,merged};
}
test("actual Rust reception -> missing-field inquiry (halt at first failure)",()=>{
  const raw=samples[5].raw; // synthetic order #6: model/quantity unspecified
  const {evidence,merged}=analyzeAndMerge(raw,"missing-qty");
  assert.equal(evidence.recipients.length,1,"order #6 should have one recipient");
  assert.equal(merged.recipients.length,1,"merging must preserve one recipient");
  assert.equal(merged.recipients[0].items.length,1,"order #6 must include one item");
  assert.equal(merged.recipients[0].items[0].quantity,null,"never invent quantity");
  checkpoint("01_RECOGNITION",{recipients:1,items:1,quantity:null});
  assert.equal(merged.raw,raw,"original order text must be preserved");
  assert.ok(merged.analysisEvidence?.length,"source evidence must survive merging");
  checkpoint("02_RECEPTION_MERGE",{preservedRaw:true,evidenceCount:merged.analysisEvidence.length});
  const state=reconcile(seed,merged);
  const groupId="group:"+merged.id;
  assert.ok(state.workflow?.groups?.some(g=>g.id===groupId),
    "Rust must create a pending preview group");
  const issueResult=native("rules","workflow_issues",{state,groupId});
  const issues=issueResult.issues??[];
  const actionable=issues.filter(i=>i.severity==="ACTION" && i.blockingStage<=1);
  assert.ok(actionable.length,"unconfirmed quantity/product should create customer-check issues");
  checkpoint("03_ISSUE_DETECTION",{actions:actionable.map(i=>({kind:i.kind,title:i.title}))});
  const rendered=native("output","render_automatic_message",{
    state,groupId,includeLinks:false,
    accountWire:seed.settings.accounts.map(a=>({id:a.id,json:JSON.stringify(a)}))
  });
  console.log("SIM31_INQUIRY_TEXT_BEGIN\n"+(rendered.text??"")+"\nSIM31_INQUIRY_TEXT_END");
  assert.equal(rendered.phase,"request","missing quantities must NOT produce bank instructions");
  assert.ok((rendered.text??"").trim().length>0,"copyable customer inquiry must not be blank");
  assert.match(rendered.text,/수량|모델|상품/,
    "customer inquiry must identify a missing field");
  checkpoint("04_CUSTOMER_INQUIRY",{phase:rendered.phase,nonempty:true,length:rendered.text.length});
  // A second sanitized original fixture ensures recipient grouping is not over-split.
  const two=analyzeAndMerge(samples[12].raw,"two-recipients");
  assert.equal(two.merged.recipients.length,2,"order #13 should have two recipient shipments");
  assert.deepEqual(two.merged.recipients.flatMap(r=>r.items.map(i=>i.quantity)).sort(),
    [1,2],"the original two item quantities must stay attached");
  checkpoint("05_MULTI_RECIPIENT_BOUNDARY",{recipients:2,quantities:[1,2]});
});
