import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const load=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("live staff checks depend on Rust issue/workflow inputs, not freight/VAT/total result",()=>{
  const hook=load("src/orders/useOrderDesk.tsx");
  const ui=load("src/OrderDesk.tsx");
  const checks=load("src/StaffChecks.tsx");
  assert.match(hook,/const staffChecksReady = nativeIssues\.ready &&[\s\S]*?\(!needsWorkflowIssues \|\| nativeBoard\.ready\)/);
  assert.match(hook,/const needsWorkflowIssues = saved && flowReady && flowGroups\.length > 0/);
  assert.match(hook,/needsWorkflowIssues && nativeBoard\.ready[\s\S]*?flowGroups\.flatMap/);
  assert.match(hook,/const saved = persisted\.some/);
  assert.match(hook,/nativeIssues\.issues/);
  assert.doesNotMatch(hook.match(/const staffChecksReady =[\s\S]*?;/)?.[0]||"",/nativeOrder\.ready|nativeLines|flowSummaryReady/);
  assert.match(ui,/loading=\{!staffChecksReady\}/);
  assert.match(ui,/error=\{staffChecksError\}/);
  assert.match(checks,/disabled=\{unavailable\} onClick=\{\(\) => onSelect\(b\)\}/);
});
