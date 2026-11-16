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


test("actual Rust freight edit -> bank-account copy preview (halt at first failure)",()=>{
  // Continue with an already staff-confirmed order. The wholesale amount is
  // synthetic: the official seed intentionally does not contain dealer prices.
  const s=structuredClone(seed);
  const product=s.products.find(p=>p.code==="BD 709"&&
    p.variants?.some(v=>v.color==="Black"));
  assert.ok(product,"original catalog should include black BD 709");
  const variant=product.variants.find(v=>v.color==="Black");
  variant.wholesale=50000; // Explicit synthetic fixture, never production price
  s.clients=[{...dealer,address:"서울 종로구 종로 1",phone:"02-1234-5678"}];
  s.settings={...s.settings,senderName:"반도스탠드",
    senderAddress:"서울 종로구 종로 10",senderPhone:"02-1234-5678"};
  const buyer=s.clients[0];
  const item={id:"sim-item-31",productId:product.id,variantId:variant.id,
    name:"BD 709",raw:"BD 709 (블랙) 2개",color:"Black",quantity:2,
    quantityConfirmed:true,unitPrice:50000,origin:"FACTORY",
    url:variant.url,issues:[]};
  const recipient={id:"sim-recipient-31",name:"김가상",
    address:"서울 종로구 청계천로 1",addressStatus:"COMPLETE",
    phone:"010-1234-5678",mobile:"",items:[item],note:"",
    vatDefault:false,shippingCharges:{FACTORY:3000}};
  const order={id:"sim-confirmed-31",clientId:buyer.id,
    orderType:"AGENCY",recipients:[recipient],raw:"BD 709 (블랙) 2개",
    createdAt:clock,updatedAt:clock,confirmedAt:clock,
    clientSnapshot:buyer,senderSnapshot:buyer,
    freightCode:"010",shippingFee:3000,applyVat:false,vatAmount:null,
    pricesLocked:true,status:"STATEMENT"};
  s.orders=[order];
  s.workflow={sources:[{id:"sim-source-31",orderId:order.id,dealerId:buyer.id,
      number:1,raw:order.raw,createdAt:clock}],
    groups:[{id:"sim-group-31",sourceId:"sim-source-31",dealerId:buyer.id,
      rootGroupId:"sim-group-31",createdAt:clock,notices:[],needsNotice:false}],
    shipments:[{id:"sim-shipment-31",sourceId:"sim-source-31",orderId:order.id,
      recipientId:recipient.id,groupId:"sim-group-31",step:1,
      completedAt:{"1":clock},charges:{FACTORY:3000},chargeMode:"MANUAL",
      infoSignature:"sim31-stable",transferred:false}],
    payments:[],queue:[],events:[],deliveries:[],changeRequests:[],confirmations:[]};
  s.exports=[];s.queue=[];
  const freight=native("rules","apply_workflow_action",{
    state:s,action:"set_shipment_freight",shipmentId:"sim-shipment-31",
    origin:"FACTORY",value:4000,now:clock,actor:"시험 직원",
    eventId:"sim-freight-event-31"
  });
  const f=freight.state;
  assert.equal(f.workflow.shipments[0].charges.FACTORY,4000);
  assert.equal(f.orders[0].recipients[0].shippingCharges.FACTORY,4000);
  assert.equal(f.workflow.shipments[0].step,1,
    "freight edit before stage 2 must not need re-approval");
  checkpoint("06_FREIGHT_EDIT",{before:3000,after:4000,step:1});
  const stage2=native("rules","apply_workflow_action",{
    state:f,action:"advance_group",groupId:"sim-group-31",
    now:clock,actor:"시험 직원",eventId:"sim-stage2-event-31"
  }).state;
  assert.equal(stage2.workflow.shipments[0].step,2,
    "confirmed freight must advance the order to account guidance stage");
  checkpoint("07_FREIGHT_APPROVAL",{step:2});
  const amounts=native("rules","calculate_shipment_totals",{
    state:stage2,shipmentId:"sim-shipment-31"});
  assert.deepEqual(
    {subtotal:amounts.subtotal,shipping:amounts.shipping,total:amounts.total},
    {subtotal:100000,shipping:4000,total:104000},
    "confirmed quantity 2 and freight 4,000 must produce exact total");
  checkpoint("08_TOTALS",{subtotal:amounts.subtotal,freight:amounts.shipping,total:amounts.total});
  const pay=native("output","render_automatic_message",{
    state:stage2,groupId:"sim-group-31",includeLinks:false,
    accountWire:stage2.settings.accounts.map(a=>({id:a.id,json:JSON.stringify(a)}))
  });
  console.log("SIM31_PAYMENT_TEXT_BEGIN\n"+(pay.text??"")+"\nSIM31_PAYMENT_TEXT_END");
  assert.equal(pay.phase,"payment",
    "once order/freight are confirmed the preview must become payment guidance");
  assert.deepEqual(pay.paymentIssues,[],"payment guidance must not have unresolved financial issues");
  assert.match(pay.text,/김가상/,"recipient distinguishes the shipment");
  assert.match(pay.text,/709/,"product distinguishes the item");
  assert.match(pay.text,/4,000/,"confirmed freight must appear");
  assert.match(pay.text,/104,000/,"exact payable total must appear");
  assert.match(pay.text,/시험은행/,"selected bank name must appear");
  assert.match(pay.text,/123-456-789/,"selected account number must appear");
  checkpoint("09_PAYMENT_COPY",{phase:pay.phase,issues:pay.paymentIssues.length,length:pay.text.length});
});
