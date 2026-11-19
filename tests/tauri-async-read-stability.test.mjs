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
  const history=read("src/FactoryReplyHistory.tsx");
  assert.match(history,/factoryReplies: state.factoryReplies/);
  assert.match(history,/\[state.factoryReplies\]/);
  assert.match(history,/nativeReview\?\.source === reviewSource/);
  assert.match(history,/"factory_reply_review_statuses", \{ state: reviewSource/);
  // Global notifications now show critical infrastructure errors only,
  // never a second, stale copy of factory reply review projections.
  const modal=read("src/FactoryNotifications.tsx");
  assert.doesNotMatch(modal,/factory_reply_review_statuses|factoryReplies: state.factoryReplies/);
  assert.match(modal,/isActionRequiredNotification/);
});
