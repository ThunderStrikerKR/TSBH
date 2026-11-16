import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("history and settlement only reload when Rust-consumed business sections change",()=>{
  for(const [path,key] of [["src/app/OrderHistory.tsx","historyState"],["src/app/DailySettlement.tsx","settlementState"]]){
    const src=read(path);
    assert.match(src,/useMemo\(\(\) => state, \[/);
    assert.ok(src.includes("["+key+", "),path);
    for(const name of ["state.orders","state.clients","state.products","state.workflow",
      "state.factoryReplies","state.shipping","state.settings"])assert.ok(src.includes(name),name);
  }
});
test("factory reply status cannot be invalidated by an unrelated order change",()=>{
  const rust=read("src-tauri/src/business_core/rules/factory_reply.rs");
  const part=rust.slice(rust.indexOf("fn review_list("),rust.indexOf("pub(super) fn execute("));
  assert.match(part,/rows\(state,"factoryReplies"\)/);
  for(const path of ["src/FactoryReplyHistory.tsx","src/FactoryNotifications.tsx"]){
    const src=read(path);
    assert.match(src,/factoryReplies: state.factoryReplies/);
    assert.match(src,/\[state.factoryReplies\]/);
    assert.match(src,/nativeReview\?\.source === reviewSource/);
    assert.match(src,/"factory_reply_review_statuses", \{ state: reviewSource/);
  }
});
