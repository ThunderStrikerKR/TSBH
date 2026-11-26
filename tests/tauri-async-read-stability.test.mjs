import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");

test("Rust factory reply review reads factory reply records",()=>{
  const rust=read("src-tauri/src/business_core/rules/factory_reply.rs");
  const part=rust.slice(rust.indexOf("fn review_list("),rust.indexOf("pub(super) fn execute("));
  assert.match(part,/rows\(state,"factoryReplies"\)/);
});
