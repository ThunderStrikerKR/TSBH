import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const state = JSON.parse(read("src-tauri/resources/state-seed.json"));
const p709 = state.products.find(p=>p.name==="BD 709 / 블랙 / 타공 상판");

test("Catalog seed preserves the BD 709 variant and retail price", () => {
  assert.equal(p709.variants.length,1);
  assert.equal(p709.variants[0].retail,59000);
});
