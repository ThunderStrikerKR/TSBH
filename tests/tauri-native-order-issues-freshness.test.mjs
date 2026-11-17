import assert from "node:assert/strict";
import test from "node:test";
import {tsImport} from "tsx/esm/api";
import {readFileSync} from "node:fs";
const {nativeOrderIssuesKey} = await tsImport("../src/orders/native-order-issues-key.ts",import.meta.url);
const clone=x=>structuredClone(x);
const item={
  id:"i",name:"709",productId:"p",variantId:"v",quantity:2,
  quantityConfirmed:true,unitPrice:42000,
  issues:[],color:"블랙",match:{kind:"EXACT"},sources:[],
};
const order={
  id:"o",clientId:"c",orderType:"DEALER",receptionVersion:1,
  raw:"최초 원문",analysisRaw:"원본",updatedAt:"2026-11-17T01:00:00Z",
  shippingFee:3000,shippingVersion:"v1",shippingMode:"AUTO",
  applyVat:false,vatAmount:0,pricesLocked:false,
  recipients:[{id:"r",name:"김가상",address:"경기도 의정부시 시민로 100",
    addressStatus:"COMPLETE",phone:"",mobile:"010-1234-5678",note:"",
    items:[item],sources:[],candidates:[],confirmedFields:[]}],
};
const before=nativeOrderIssuesKey(order);
test("unrelated native freight/total/metadata replies never re-request staff issues",()=>{
  const d=clone(order);
  d.raw="같은 주문 원문 메모";d.analysisRaw="재분석 메모";
  d.updatedAt="2026-11-17T02:00:00Z";
  d.shippingFee=4500;d.shippingVersion="v2";
  d.shippingMode="MANUAL";d.applyVat=true;d.vatAmount=8400;
  d.pricesLocked=true;d.accountId="account2";
  d.recipients[0].note="고객 요청 메모";
  d.recipients[0].items[0].url="https://example.com/709";
  assert.equal(nativeOrderIssuesKey(d),before);
});
test("all actual Rust issue and approval inputs still refresh staff issues",()=>{
  const mutators=[
    x=>{x.clientId="c2"},
    x=>{x.orderType="RETAIL"},
    x=>{x.clientMatch={source:"CONFLICT",clientId:"c3",detail:"다른 거래처"}},
    x=>{x.receptionVersion=undefined},
    x=>{x.issueResolutions={"issue:1":{resolution:"PENDING_CUSTOMER"}}},
    x=>{x.recipients[0].name="다른 수취인"},
    x=>{x.recipients[0].address="불명확한 주소"},
    x=>{x.recipients[0].mobile="010-123"},
    x=>{x.recipients[0].addressStatus="INCOMPLETE"},
    x=>{x.recipients[0].candidates.push({field:"mobile",value:"010"})},
    x=>{x.recipients[0].confirmedFields.push("address")},
    x=>{x.recipients[0].unassignedEvidence=[{kind:"quantity"}]},
    x=>{x.recipients[0].sources.push({field:"name",start:0,end:1})},
    x=>{x.recipients[0].items[0].quantity=3},
    x=>{x.recipients[0].items[0].quantityConfirmed=false},
    x=>{x.recipients[0].items[0].quantityApproval={quantity:1,actor:"직원"}},
    x=>{x.recipients[0].items[0].productId="other"},
    x=>{x.recipients[0].items[0].variantId=null},
    x=>{x.recipients[0].items[0].unitPrice=8000},
    x=>{x.recipients[0].items[0].match={kind:"QUANTITY"}},
    x=>{x.recipients[0].items[0].issues=["색상 확인"]},
    x=>{x.recipients[0].items[0].color="레드"},
    x=>{x.recipients[0].items[0].modelProposal={required:true}},
    x=>{x.recipients[0].items[0].sources.push({field:"product",start:1,end:3})},
  ];
  for(const mutate of mutators){
    const d=clone(order);mutate(d);
    assert.notEqual(nativeOrderIssuesKey(d),before,String(mutate));
  }
});
test("late order-issues results cannot override a newer Rust projection",()=>{
  const hook=readFileSync(
    new URL("../src/orders/useNativeOrderIssues.ts",import.meta.url),"utf8");
  assert.match(hook,/nativeOrderIssuesKey\(draft\)/);
  assert.match(hook,/let cancelled = false/);
  assert.match(hook,/if \(!cancelled\) \{/);
  assert.match(hook,/resolved\?\.fingerprint === fingerprint/);
});

test("complete multi-line Korean address does not become a choice of fragments",()=>{
  const rust=read("src-tauri/src/business_core/rules/issues.rs");
  assert.match(rust,/fn address_candidates_form_one_location/);
  assert.match(rust,/field_name=="address" &&\s*address_candidates_form_one_location\(row,&candidates\)/);
  assert.match(rust,/multiline_province_road_and_floor_are_one_complete_address/);
  assert.match(rust,/two_independent_complete_addresses_still_require_review/);
  assert.match(rust,/conflicting_administrative_fragments_are_not_auto_approved/);
});
