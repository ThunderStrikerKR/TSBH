import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read=(path)=>readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("React order/workflow views do not import historical TypeScript state reducers",()=>{
  for(const path of [
    "src/HubWorkspace.tsx","src/WorkflowBoard.tsx",
    "src/WorkflowFooter.tsx","src/WorkflowQueue.tsx",
    "src/CorrectionPanel.tsx","src/orders/useOrderDesk.tsx"
  ]){
    const source=read(path);
    assert.doesNotMatch(source,/from ["']\.\.?\/workflow["']/);
    assert.match(source,/from ["']\.\.?\/workflow-view["']/);
  }
  const app=read("src/app/useAppModel.ts");
  const workspace=read("src/HubWorkspace.tsx");
  assert.match(app,/from "\.\.\/orders\/new-reception"/);
  assert.match(workspace,/from "\.\/orders\/new-reception"/);
  assert.doesNotMatch(read("src/app/DateNavigator.tsx"),/from "\.\/settlement-report"/);
  const view=read("src/workflow-view.ts");
  assert.match(view,/export function shipmentContext/);
  assert.match(view,/export function groupShipments/);
  assert.doesNotMatch(view,/reconcileWorkflow|advanceGroup|recordPayment|processGroup/);
});
