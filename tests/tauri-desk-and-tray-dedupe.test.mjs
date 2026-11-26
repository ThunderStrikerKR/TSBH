import {test} from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";

const read = path => JSON.parse(readFileSync(new URL("../"+path,import.meta.url),"utf8"));
const seed = read("src-tauri/resources/state-seed.json");
const legacy = read("src-tauri/resources/legacy-state-seed-303.json");
const product = id => {
  const result=seed.products.find(p=>p.id===id);
  assert.ok(result, id);
  return result;
};
const bySite = site => {
  const result=seed.products.filter(p=>p.siteId===site);
  assert.equal(result.length,1,site);
  return result[0];
};

test("TS and IS Desk specification labels reflect each independently sold length",()=>{
  const expected = [
    ["247","BD Desk TS-1 / 실버 / Bar 길이 15cm"],
    ["248","BD Desk TS-2 / 실버 / Bar 길이 22cm"],
    ["249","BD Desk TS-3 / 실버 / Bar 길이 32cm"],
    ["252","BD Desk IS-1 / 블랙 / Bar 길이 10cm"],
    ["251","BD Desk IS-2 / 블랙 / Bar 길이 15cm"],
    ["250","BD Desk IS-3 / 블랙 / Bar 길이 26cm"],
  ];
  const historic=new Map(legacy.products.map(p=>[p.id,p]));
  for(const [site,name] of expected){
    const p=bySite(site);
    assert.equal(p.name,name,site);
    assert.equal(p.variants.length,1);
    assert.equal(p.variants[0].id,historic.get(p.id).variants[0].id);
    const currentVariant = { ...p.variants[0] };
     const historicVariant = { ...historic.get(p.id).variants[0] };
     delete currentVariant.color;
     delete historicVariant.color;
     assert.deepEqual(currentVariant, historicVariant);
    assert.ok(!p.subtitle?.includes("확인 필요"),site);
  }
});

test("dark-green and wine duplicate listings are retired, not deleted, without losing price or history",()=>{
  const pairs=[
    ["bando-site-77","bando-77-single","sku:bando-site-237:bando-237-P00000JC000H","다크 그린"],
    ["bando-site-78","bando-78-single","sku:bando-site-237:bando-237-P00000JC000I","와인"],
  ];
  assert.equal(seed.products.length,360);
  for(const [old,priceId,current,color] of pairs) {
    const previous=product(old),selected=product(current);
    assert.equal(previous.active,false);
    assert.equal(previous.supersededBy,current);
    assert.equal(previous.variants[0].id,priceId);
    assert.equal(previous.variants[0].retail,32000);
    assert.equal(selected.variants[0].retail,32000);
    assert.equal(selected.variants[0].option,color);
    assert.equal(selected.active,true);
    assert.ok(!selected.supersededBy);
    assert.ok(!selected.subtitle?.includes("옵션"));
  }
  
  assert.equal(new Set(seed.products.map(p=>p.variants[0].id)).size,360);
});
