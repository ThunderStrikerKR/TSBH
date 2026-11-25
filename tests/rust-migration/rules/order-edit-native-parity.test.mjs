/** Compare real legacy TypeScript oracles with compiled production Rust order rules.
 * The TypeScript edit adapter is frozen from the former useOrderDesk::edit.
 * It is only a test oracle; production React must never import this module.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {tsImport} from "tsx/esm/api";

const {orderClient}=await tsImport("../../../src/domain.ts",import.meta.url);
const {repriceOrder}=await tsImport("../oracles/reprice-order.ts",import.meta.url);
const {billingSnapshot}=await tsImport("../oracles/replies.ts",import.meta.url);
const {recommendShipping}=await tsImport("../oracles/legacy-shipping.ts",import.meta.url);
const root=fileURLToPath(new URL("../../../",import.meta.url));
const manifest=fileURLToPath(new URL("../integration/Cargo.toml",import.meta.url));
const now="2026-11-15T11:00:00.000Z";
const clone=x=>structuredClone(x);
const canonical=x=>JSON.parse(JSON.stringify(x));
const client=(id,waived=false,tier="wholesale")=>({
  id,name:"테스트 "+id,displayName:"테스트 "+id,tier,
  payment:"PREPAID",shippingFeeExcluded:waived,overrides:{},
  address:"경기도 가상로 1",phone:"010-0000-0000",mobile:"",active:true,
});
const c1=client("c1"),c2=client("c2",true,"retail");
const p={id:"p1",name:"BD 709",origin:"FACTORY",active:true,shippingGroupId:"music",
  variants:[{id:"v1",color:"Black",wholesale:30000,semi:35000,retail:40000,url:"/709"}]};
const order=()=>({
  id:"o1",clientId:"c1",clientSnapshot:clone(c1),orderType:"AGENCY",
  raw:"가상 발주서",channel:"FAX",
  recipients:[{id:"r1",name:"가상 고객",address:"경기도 가상로 1",
    phone:"",mobile:"010-2222-3333",note:"",
    items:[{id:"i1",productId:"p1",variantId:"v1",raw:"BD 709",
      name:"BD 709",color:"Black",unitPrice:30000,quantity:1,
      quantityConfirmed:true,issues:[],url:"",origin:"FACTORY",defaulted:false}]
  }],
  createdAt:now,updatedAt:now,
  pricesLocked:true,notified:true,paid:true,postpaidApproved:false,
  status:"STATEMENT",shippingMode:"AUTO",shippingFee:3000,
  shippingVersion:"old-version",applyVat:false,vatAmount:0,freightCode:"010",
  billingHistory:[],
});
const state=()=>({
  clients:[clone(c1),clone(c2)],products:[clone(p)],orders:[order()],
  settings:{senderName:"반도스탠드",senderAddress:"경기도 가상로 1",
    senderPhone:"02-000-0000",senderMobile:"",senderPostcode:"11111",
    accounts:[],defaultAccountId:""},
  shipping:{epoch:1,groups:[{id:"music",name:"보면대",current:{
    version:"v1",at:now,maxBundle:4,maxConfirmed:true,
    fees:{"1":3000,"2":4000},perItem:null,notes:""
  },history:[]}],productRules:{},combinations:[]},
  workflow:{sources:[],groups:[],shipments:[],payments:[],
    queue:[],events:[],deliveries:[],changeRequests:[]},
  queue:[],exports:[],
});
function legacyEdit(before,after,st,intent){
  const next=clone(after);
  delete next.pendingNativeEdit;
  // Original useOrderDesk::edit direct mutations, copied without alteration.
  if(before.pricesLocked &&
    !st.workflow?.shipments.some(ship=>ship.orderId===before.id&&!ship.legacyArchived))
    next.pendingBillingSnapshot={at:intent.at,reason:intent.reason};
  next.pricesLocked=false;
  const live=st.clients.find(c=>c.id===before.clientId&&!c.deletedAt)
    ??orderClient(before,st);
  next.clientSnapshot=live?clone(live):next.clientSnapshot;
  next.notified=false;
  next.paid=false;
  next.postpaidApproved=false;
  next.status="REVIEW";
  if(next.orderType)
    for(const row of next.recipients){
      row.dispatchOrigin??="FACTORY";
      row.items.forEach(item=>{item.origin=row.dispatchOrigin;});
    }
  next.updatedAt=intent.at;
  if(intent.recalculateShipping){
    next.shippingMode="AUTO";next.shippingFee=null;next.shippingVersion="";
  }
  return next;
}
function legacyPrepareSave(before,st){
  const chosen=orderClient(before,st);
  if(!chosen||!chosen.id) throw Error("거래처를 선택하세요.");
  const next=clone(before);
  if(next.shippingMode==="AUTO"&&next.shippingVersion===""&&next.shippingFee===null){
    const freight=recommendShipping(next,st);
    next.shippingFee=chosen.shippingFeeExcluded?0:freight.total;
    next.shippingVersion=freight.version;
  }
  return {order:next,client:chosen};
}
function legacyChangeDealer(before,st,target){
  const selected=st.clients.find(c=>c.id===target);
  const changed=repriceOrder({...clone(before),clientId:selected.id,
    clientSnapshot:clone(selected),pricesLocked:false,
    notified:false,paid:false,postpaidApproved:false,status:"REVIEW"},selected,st);
  if(before.pricesLocked){
    const snapshot=billingSnapshot(before,st,"거래처 변경");
    snapshot.at=now;
    changed.billingHistory=[...(changed.billingHistory??[]),snapshot];
  }
  changed.updatedAt=now;
  // New safety improvement: a dealer change invalidates old freight and
  // Rust must provide the new dealer's amount BEFORE the workflow save.
  const freight=recommendShipping(changed,st);
  changed.shippingMode="AUTO";
  changed.shippingFee=selected.shippingFeeExcluded?0:freight.total;
  changed.shippingVersion=freight.version;
  return changed;
}
// Deliberate approved Rust-only addition: store manual dealer provenance without a review alert.
 // Keep the frozen TypeScript business oracle unchanged and compare all
 // prices, shipment flags, and invoices plus the new audit field.
function expectedDealerChange(before,st,target){
  const changed=legacyChangeDealer(before,st,target);
  changed.clientMatch={source:"MANUAL",clientId:target,
    detail:"사용자가 거래처 ‘"+st.clients.find(c=>c.id===target).name+"’를 직접 선택했습니다."};
  return changed;
}
const cases=[];
function add(label,operation,st,before,expected,args={}){
  cases.push({label,request:{domain:"rules",operation:"order_edit_rules",
    payload:{action:operation,state:st,order:before,...args}},expected});
}
for(const recalculateShipping of [true,false]){
  for(const locked of [true,false]){
    const st=state(),before=order();
    before.pricesLocked=locked;
    const edited=clone(before);
    edited.recipients[0].note="사무실에서 요청";
    const intent={at:now,reason:"수취인 / 배송 정보 수정",recalculateShipping};
    edited.pendingNativeEdit=intent;
    add("edit / locked="+locked+" / freight="+recalculateShipping,
      "prepare_order_edit",st,edited,{order:legacyEdit(before,edited,st,intent)});
  }
}
{
  const st=state(),before=order(),edited=clone(before);
  st.workflow.shipments.push({id:"s1",orderId:"o1",recipientId:"r1",legacyArchived:false});
  edited.recipients[0].name="직원 변경";
  const intent={at:now,reason:"첫 수정",recalculateShipping:false};
  edited.pendingNativeEdit=intent;
  add("shipment-linked edit never freezes duplicate invoice","prepare_order_edit",
    st,edited,{order:legacyEdit(before,edited,st,intent)});
}
{
  const st=state(),before=order(),edited=clone(before);
  edited.pendingNativeEdit={at:"earliest",reason:"원본 수정",recalculateShipping:true};
  edited.recipients[0].address="직원 수정된 배송주소";
  add("earliest pending edit intent is kept","prepare_order_edit",
    st,edited,{order:legacyEdit(before,edited,st,edited.pendingNativeEdit)});
}
{
  const st=state(),before=order();
  add("frozen order chooses historical dealer","resolve_order_client",st,before,
    {client:orderClient(before,st)});
  const open=clone(before);open.pricesLocked=false;open.clientId="c2";
  add("open order chooses new dealer","resolve_order_client",st,open,
    {client:orderClient(open,st)});
  for(const openOrder of [before,open]){
    add("prepare save / "+openOrder.clientId,"prepare_order_save",st,openOrder,
      legacyPrepareSave(openOrder,st));
    const pending=clone(openOrder);
    pending.shippingFee=null;pending.shippingVersion="";pending.shippingMode="AUTO";
    add("prepare save missing freight / "+openOrder.clientId,
      "prepare_order_save",st,pending,legacyPrepareSave(pending,st));
    const freight=recommendShipping(pending,st);
    const chosen=st.clients.find(c=>c.id===pending.clientId);
    add("effective freight / "+pending.clientId,
      "calculate_order_shipping",st,pending,{
        total:chosen.shippingFeeExcluded?0:freight.total,
        version:freight.version,waived:chosen.shippingFeeExcluded
      });
  }
  add("change dealer freezes old invoice and reprices atomically",
    "change_order_client",st,before,{order:expectedDealerChange(before,st,"c2")},
    {dealerId:"c2",now});
  const confirmed=clone(before);confirmed.confirmedAt="2026-11-14T00:00:00.000Z";
  add("confirmed dealer change still reprices all configured products",
    "change_order_client",st,confirmed,{order:expectedDealerChange(confirmed,st,"c2")},
    {dealerId:"c2",now});
  const notLocked=clone(before);notLocked.pricesLocked=false;
  add("change dealer does not freeze a nonlocked invoice",
    "change_order_client",st,notLocked,{order:expectedDealerChange(notLocked,st,"c2")},
    {dealerId:"c2",now});
}
const exec=spawnSync("cargo",["run","--quiet","--manifest-path",manifest,
  "--bin","reception_runner"],{cwd:root,
  input:JSON.stringify(cases.map(x=>x.request)),encoding:"utf8",
  timeout:900_000,maxBuffer:60*1024*1024});
if(exec.error)throw exec.error;
assert.equal(exec.status,0,"Rust order-edit integration runner failed:\n"+exec.stderr);
const answers=JSON.parse(exec.stdout);
test("Rust and original TypeScript match all editable-order and dealer/freight scenarios",()=>{
  assert.equal(answers.length,cases.length);
  for(let i=0;i<cases.length;i++){
    const c=cases[i],got=answers[i];
    assert.equal(got.ok,true,c.label+": "+got.error);
    assert.deepEqual(canonical(got.value),canonical(c.expected),c.label);
  }
});
test("edited React draft is never persisted before native confirmation",()=>{
  const st=state(),edited=order();
  edited.pendingNativeEdit={at:now,reason:"수취인 수정",recalculateShipping:true};
  const req={domain:"rules",operation:"order_edit_rules",payload:{
    action:"prepare_order_save",state:st,order:edited
  }};
  const result=spawnSync("cargo",["run","--quiet","--manifest-path",manifest,
    "--bin","reception_runner"],{cwd:root,input:JSON.stringify([req]),
    encoding:"utf8",timeout:900_000});
  assert.equal(result.status,0,result.stderr);
  const [answer]=JSON.parse(result.stdout);
  assert.equal(answer.ok,false);
  assert.match(answer.error,/Rust 검증/);
});
