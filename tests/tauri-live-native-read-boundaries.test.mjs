import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

// Source-boundary guards only. Real output and interaction equivalence requires
// compiled Rust, latest-HEAD CI and Windows GUI verification independently.
test("live reception never selects the display/pricing dealer with TS rules", () => {
  const desk = source("src/orders/useOrderDesk.tsx");
  const hook = source("src/orders/useNativeOrderClients.ts");
  const rust = source("src-tauri/src/business_core/rules/order_edit.rs");
  assert.match(desk, /useNativeOrderClients\(draft, state\)/);
  assert.doesNotMatch(desk, /\borderClient\s*\(/);
  assert.doesNotMatch(desk, /state\.clients\.find\(/);
  assert.match(desk, /!clientSelection\.ready/);
  assert.match(desk, /pricingClient = liveClient \?\? unassignedClient/);
  assert.match(hook, /"order_edit_rules"/);
  assert.match(hook, /action: "resolve_order_clients"/);
  assert.match(hook, /if \(cancelled\) return/);
  assert.match(rust, /"resolve_order_clients" =>/);
  assert.match(rust, /"liveClient":live_client\(order,state\)/);
});

test("shipment queue and actual freight field use a shared native projection", () => {
  const view = source("src/workflow-view.ts");
  const queue = source("src/WorkflowQueue.tsx");
  const field = source("src/ActualFreightField.tsx");
  const hook = source("src/orders/useNativeShipmentRead.ts");
  const router = source("src-tauri/src/business_core/rules/mod.rs");
  const rust = source("src-tauri/src/business_core/rules/workflow.rs");
  assert.doesNotMatch(view, /\borderParty\s*\(/);
  assert.doesNotMatch(view, /from ["']\.\/order-types/);
  assert.doesNotMatch(queue, /\bactualFreightOf\s*\(/);
  assert.doesNotMatch(field, /\bactualFreightOf\s*\(/);
  assert.match(queue, /useNativeShipmentRead\(state\)/);
  assert.match(queue, /nativeRead=\{shipmentRead\}/);
  assert.match(field, /const read = nativeRead \?\? fallbackRead/);
  assert.match(field, /if \(!ready\) return Promise\.resolve\(false\)/);
  assert.match(hook, /"rules", "read_shipments"/);
  assert.match(router, /"read_shipments" => workflow::read_shipments/);
  assert.match(rust, /pub\(super\) fn read_shipments/);
  assert.match(rust, /super::order_edit::client\(order,state\)/);
  assert.match(rust, /field\(ship,"actualFreight"\)\.get\(origin\)\.cloned\(\)/);
});
