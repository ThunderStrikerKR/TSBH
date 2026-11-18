import assert from "node:assert/strict";
import test from "node:test";
import { projectHistoricalRemoteDelivery as project } from "./rust-migration/legacy-remote-delivery-parity.mjs";

const old = {value:{state:{orders:[{recipients:[{address:"서울시 가상로 1",paid:3000}]}]}}};
const marker = {address:"서울시 가상로 1",checked:false,manual:false,legacy:true};
const addMarker = (m) => structuredClone({
  value:{state:{orders:[{recipients:[{
    address:"서울시 가상로 1",paid:3000,remoteDelivery:m,
  }]}]}},
});

test("approved unchecked legacy marker does not fail the old TS oracle", () => {
  assert.deepEqual(project(old,addMarker(marker)),old);
});

test("checked, manual, newer and mismatched locality markers remain visible", () => {
  for(const change of [
    {checked:true}, {manual:true}, {legacy:false},
    {address:"제주특별자치도 제주시"}, {unknown:"unexpected"},
  ]) {
    const actual=addMarker({...marker,...change});
    assert.notDeepEqual(project(old,actual),old);
  }
});

test("financial changes and pre-existing locality fields are never suppressed", () => {
  const different=addMarker(marker);
  different.value.state.orders[0].recipients[0].paid=9999;
  assert.notDeepEqual(project(old,different),old);
  const withExisting=addMarker(marker);
  const withExpected=structuredClone(old);
  withExpected.value.state.orders[0].recipients[0].remoteDelivery={
    ...marker,checked:true,
  };
  assert.notDeepEqual(project(withExpected,withExisting),withExpected);
});

test("Rust-only read-only preview locality marker can be projected", () => {
  const expected={ok:true,value:{groups:[{shipments:[{paid:0}]}]}};
  const actual=structuredClone(expected);
  actual.value.groups[0].shipments[0].remoteDelivery={
    status:"unknown",checked:false,manual:false,legacy:true,
    suggestedChecked:false,requiresReview:false,matchedPath:null,
    reason:null,sources:[],dataVersion:"2026-11-19",
  };
  assert.deepEqual(project(expected,actual),expected);
});
