import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

// Source-boundary guards only. Real output and interaction equivalence requires
// compiled Rust, latest-HEAD CI and Windows GUI verification independently.
test("live reception never selects the display/pricing dealer with TS rules", () => {
  const rust = source("src-tauri/src/business_core/rules/order_edit.rs");
  assert.match(rust, /"resolve_order_clients" =>/);
  assert.match(rust, /"liveClient":live_client\(order,state\)/);
});

test("shipment queue and actual freight field use a shared native projection", () => {
  const router = source("src-tauri/src/business_core/rules/mod.rs");
  const rust = source("src-tauri/src/business_core/rules/workflow.rs");
  assert.match(router, /"read_shipments" => workflow::read_shipments/);
  assert.match(rust, /pub\(super\) fn read_shipments/);
  assert.match(rust, /super::order_edit::client\(order,state\)/);
  assert.match(rust, /field\(ship,"actualFreight"\)\.get\(origin\)\.cloned\(\)/);
});
