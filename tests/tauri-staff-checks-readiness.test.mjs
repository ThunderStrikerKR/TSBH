import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const load=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("order staff list uses the Rust stage issues for unsaved and saved grouped orders",()=>{
  const hook=load("src/orders/useOrderDesk.tsx");
  assert.match(hook,/const currentWorkflowIssues = flowReady && nativeBoard\.ready && flowGroups\.length > 0/);
  assert.match(hook,/cachedWorkflowChecks\.current = \{ orderId: draft\.id, issues: currentWorkflowIssues \}/);
  assert.match(hook,/const displayWorkflowIssues = currentWorkflowIssues \?\? cachedIssueRows/);
  assert.match(hook,/const allBlocks = !draft\.raw\.trim\(\) && !draft\.recipients\.length[\s\S]*?displayWorkflowIssues \?\? nativeIssues\.issues/);
  assert.doesNotMatch(hook,/saved && flowReady && flowGroups\.length/);
});
test("cached Rust staff issue rows display steadily but cannot trigger stale actions",()=>{
  const hook=load("src/orders/useOrderDesk.tsx");
  const ui=load("src/OrderDesk.tsx");
  const checks=load("src/StaffChecks.tsx");
  assert.match(hook,/const staffChecksReady = displayWorkflowIssues !== null \|\| nativeIssues\.ready/);
  assert.match(hook,/const staffChecksActionsReady = nativeIssues\.ready &&/);
  assert.match(hook,/currentWorkflowIssues !== null \|\| cachedIssueRows === null/);
  assert.match(ui,/loading=\{!staffChecksReady\}/);
  assert.match(ui,/interactive=\{staffChecksActionsReady\}/);
  assert.match(checks,/const unavailable = loading \|\| !!error \|\| !interactive/);
  assert.match(ui,/!staffChecksActionsReady \|\|/);
});
