import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const data = JSON.parse(source("src-tauri/resources/remote-delivery-zones.json"));
const provinces = new Set(
  "서울특별시 부산광역시 대구광역시 인천광역시 광주광역시 대전광역시 울산광역시 세종특별자치시 경기도 강원도 강원특별자치도 충청북도 충청남도 전라북도 전북특별자치도 전라남도 경상북도 경상남도 제주도 제주특별자치도 전남광주통합특별시 서울 부산 대구 인천 광주 대전 울산 경기 강원 충북 충남 전북 전남 경북 경남".split(" "),
);
const tokens = (s) => s.split(/\s+/).map(x => x.replace(/^[,:[\]():：（）]+|[,:[\]():：（）]+$/g, "")).filter(Boolean);
const detect = (address) => {
  const head = tokens(address);
  const offset = /^\d{5}$/.test(head[0]) ? 1 : 0;
  const starts = [offset, ...(provinces.has(head[offset]) ? [offset + 1] : [])];
  let found;
  for (const item of data.rules) {
    const parts = item.path.split(/\s+/);
    if ((found?.path.split(/\s+/).length ?? 0) >= parts.length) continue;
    if (starts.some(start => parts.every((p, i) => head[start + i] === p))) found = item;
  }
  return found?.decision ?? "unknown";
};

test("region dataset has provenance and never blindly converts road names to a whole city", () => {
  assert.ok(data.rules.length >= 90);
  assert.ok(data.sources.some(s => s.id === "domeggook"));
  assert.ok(data.sources.some(s => s.id === "epost"));
  assert.ok(data.sources.some(s => s.id === "kftc"));
  assert.ok(data.rules.every(x => ["auto", "review", "normal"].includes(x.decision)));
  assert.equal(new Set(data.rules.map(x => x.path)).size, data.rules.length);
  assert.ok(data.rules.every(x => !/(?:길|대로|번길|로)\d*$/.test(x.path.split(" ").at(-1))));
  assert.ok(data.rules.every(x => x.sources.length && x.reason));
});

test("Jeju is all-region, but Gunsan is NOT all-region", () => {
  assert.equal(detect("제주특별자치도 제주시 아라일동 55"), "auto");
  assert.equal(detect("[63644] 제주특별자치도 서귀포시 대정읍 에듀시티로 23"), "auto");
  assert.equal(detect("전북특별자치도 군산시 소룡동 33"), "unknown");
  assert.equal(detect("전북특별자치도 군산시 옥도면 어청도리 33"), "auto");
  assert.equal(detect("전라북도 군산시 옥도면 선유도리 33"), "review");
  assert.equal(detect("전라북도 군산시 옥도면 신시도리 33"), "review");
});

test("exception villages override generic remote districts", () => {
  assert.equal(detect("인천광역시 옹진군 백령면 33"), "auto");
  assert.equal(detect("인천광역시 옹진군 영흥면 33"), "normal");
  assert.equal(detect("여수시 화정면 개도리 33"), "auto");
  assert.equal(detect("전남 여수시 화정면 백야리 33"), "normal");
  assert.equal(detect("전남 신안군 압해읍 가란리 33"), "auto");
  assert.equal(detect("전남 신안군 압해읍 신장리 33"), "review");
  assert.equal(detect("전남 신안군 지도읍 선도리 33"), "auto");
  assert.equal(detect("전남 신안군 지도읍 읍내리 33"), "review");
  assert.equal(detect("충남 보령시 오천면 녹도리 33"), "auto");
  assert.equal(detect("충남 보령시 오천면 원산도리 33"), "review");
});

test("address labels from an unrelated district never trigger an island decision", () => {
  assert.equal(detect("서울특별시 서초구 제주특별자치도"), "unknown");
  assert.equal(detect("전북특별자치도 군산시 소룡동 옥도면길 33"), "unknown");
  assert.equal(detect("경기도 군포시 옥도면타워 7"), "unknown");
});

test("Rust operation remains pure, read-only and based on this versioned dataset", () => {
  const rust = source("src-tauri/src/business_core/rules/remote_delivery.rs");
  const router = source("src-tauri/src/business_core/rules/mod.rs");
  assert.match(rust, /include_str!\("\.\.\/\.\.\/\.\.\/resources\/remote-delivery-zones\.json"\)/);
  assert.match(rust, /"suggestedChecked": result == "auto"/);
  assert.match(rust, /"requiresReview": result == "review"/);
  assert.match(router, /"detect_remote_delivery_area" => remote_delivery::detect\(&payload\)/);
  assert.doesNotMatch(rust, /UPDATE|INSERT|DELETE|shippingCharges|shippingFee\s*=|chargeMode/);
});
