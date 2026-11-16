import assert from "node:assert/strict";
import test from "node:test";
import {tsImport} from "tsx/esm/api";

const {nativeOrderDisplayKey} = await tsImport(
  "../src/orders/native-order-display-key.ts", import.meta.url
);
const state = {
  revision: 3, products: [{id:"p"}], clients: [{id:"c",tier:"wholesale"}],
  orders: [], workflow: {shipments: [], groups: []},
  shipping: {epoch: 1}, settings: {operatorName:"직원"},
};
const order = {
  id:"o", clientId:"c", orderType:"AGENCY", raw:"원문",
  shippingFee:3000, shippingMode:"AUTO", shippingVersion:"v1",
  freightCode:"010", applyVat:false, vatAmount:0, pricesLocked:false,
  paid:false, status:"REVIEW",
  recipients:[{
    id:"r",name:"김가상",address:"서울",phone:"010-1234-5678",
    vatDefault:false, vatOverride:undefined,freightCode:"010",
    shippingCharges:{FACTORY:3000},items:[{
      id:"i",productId:"p",variantId:"v",unitPrice:7000,quantity:3,
      origin:"FACTORY",
    }],
  }],
};
const copy=value=>structuredClone(value);
const baseline=nativeOrderDisplayKey(state,order);

test("changing ANY relevant top-level billing, dealer or mode input invalidates stale Rust quote",()=>{
  const mutations=[
    ["shippingFee",3500],["shippingVersion","v2"],["shippingMode","MANUAL"],
    ["freightCode","020"],["actualFreight",3900],["vatAmount",2100],
    ["applyVat",true],["clientId","c2"],["clientSnapshot",{id:"c",tier:"retail"}],
    ["senderSnapshot",{id:"sender"}],["pricesLocked",true],
    ["confirmedAt","2026-11-17T09:00:00Z"],["deletedAt","now"],
    ["accountMode","MANUAL"],["accountId","bank2"],
    ["groupAccounts",{g1:{mode:"MANUAL",id:"bank2"}}],
    ["orderType","RETAIL"],["paid",true],["status","EXPORTED"],
  ];
  for(const [field,value] of mutations){
    const draft=copy(order);
    draft[field]=value;
    assert.notEqual(nativeOrderDisplayKey(state,draft),baseline,field);
  }
});
test("recipient VAT default, actual freight, manual shipping and each line edit invalidate quote",()=>{
  const paths=[
    r=>r.vatDefault=true,r=>r.vatOverride=true,r=>r.freightCode="020",
    r=>r.actualFreight=4000,r=>r.shippingCharges.FACTORY=4000,
    r=>r.address="서울 다른 주소",
    r=>r.items[0].unitPrice=10000,r=>r.items[0].quantity=4,
    r=>r.items[0].variantId="variant-new",r=>r.items[0].origin="STORE",
    r=>r.items.push({id:"new",quantity:1,productId:"p",variantId:"v"}),
  ];
  for(const change of paths){
    const draft=copy(order);change(draft.recipients[0]);
    assert.notEqual(nativeOrderDisplayKey(state,draft),baseline,String(change));
  }
});
test("persisted order, workflow, customer price and tariff changes invalidate quote",()=>{
  const changes=[
    s=>s.orders.push(copy(order)),
    s=>s.workflow.shipments.push({id:"shipment"}),
    s=>s.products[0].retail=10000,
    s=>s.clients[0].tier="retail",
    s=>s.settings.operatorName="other",
    s=>s.shipping.epoch=2,
  ];
  for(const change of changes){
    const current=copy(state);change(current);
    assert.notEqual(nativeOrderDisplayKey(current,order),baseline,String(change));
  }
});
test("unrelated typing and revision updates do not flash an already-settled native price",()=>{
  const newOrder=copy(order);
  newOrder.raw="내용상 동일한 주문 원문 문장 교정";
  newOrder.analysisEvidence=[{kind:"text"}];
  newOrder.messageNotes={test:{payment:"메시지"}};
  const newState=copy(state);newState.revision=7;
  assert.equal(nativeOrderDisplayKey(newState,newOrder),baseline);
});
