import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const src=readFileSync(new URL("../src/OrderDesk.tsx",import.meta.url),"utf8");
test("during pending Rust contact/address assessment, React does not mark unknown fields invalid",()=>{
  assert.match(src,/aria-invalid=\{delivery\.ready \? !delivery\.byId\[r\.id\]\?\.name : undefined\}/);
  assert.match(src,/aria-invalid=\{delivery\.ready \? !delivery\.byId\[r\.id\]\?\.address : undefined\}/);
  assert.equal((src.match(/aria-invalid=\{delivery\.ready \? !delivery\.byId\[r\.id\]\?\.contact : undefined\}/g)||[]).length,2);
});
