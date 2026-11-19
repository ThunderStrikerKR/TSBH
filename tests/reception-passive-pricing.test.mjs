import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const { shouldRefreshUncommittedOrderPricing } =
  await tsImport("./rust-migration/oracles/passive-pricing.ts", import.meta.url);

const draft = (extra = {}) => ({
  id: "order-pricing-audit", confirmedAt: undefined, deletedAt: undefined,
  pricesLocked: false, raw: "CI 709 1개", shippingMode: "AUTO",
  shippingFee: 7500, shippingVersion: "historical-v1",
  recipients: [{ id: "recipient", name: "가상수취인", items: [
    { id: "item", productId: "product", variantId: "price", unitPrice: 13700, quantity: 1 },
  ] }],
  ...extra,
});

test("a saved legacy order must never passively adopt current master prices or freight", () => {
  const original = draft({ pricesLocked: false });
  const snapshot = structuredClone(original);
  // Historical saved order must never auto-reprice; compiled Rust owns the
  // decision, not the retired TypeScript repriceOrder helper.
  const native = readFileSync(new URL("../src-tauri/src/business_core/rules/pricing.rs", import.meta.url), "utf8");
  assert.match(native, /let refresh = flag\(payload, "explicitEdit"\) \|\| should_refresh\(payload\)\?/);
  assert.match(native, /if !refresh \{/);
  assert.match(native, /passive_catalog_reprice_keeps_frozen_shipments_at_original_price/);
  assert.equal(shouldRefreshUncommittedOrderPricing(original, [{ id: original.id }]), false);
  assert.deepEqual(original, snapshot, "Policy checks must never mutate snapshots");
  assert.equal(original.shippingFee, 7500);
  assert.equal(original.shippingVersion, "historical-v1");
});

test("a saved unconfirmed order and even an orphaned confirmed order are protected", () => {
  const order = draft();
  assert.equal(shouldRefreshUncommittedOrderPricing(order, [{ id: order.id }]), false);
  assert.equal(shouldRefreshUncommittedOrderPricing(draft({
    confirmedAt: "2026-11-01T00:00:00Z",
  }), []), false);
  assert.equal(shouldRefreshUncommittedOrderPricing(draft({
    pricesLocked: true,
  }), []), false);
  assert.equal(shouldRefreshUncommittedOrderPricing(draft({
    deletedAt: "2026-11-01T00:00:00Z",
  }), []), false);
});

test("new uncommitted reception follows master changes until finalized", () => {
  const unsaved = draft();
  assert.equal(shouldRefreshUncommittedOrderPricing(unsaved, []), true);
  assert.equal(shouldRefreshUncommittedOrderPricing(draft({
    id: "another-order",
  }), [{ id: "historical-order" }]), true);
});

test("native Rust pricing/shipping effects protect persisted records and explicit edits", () => {
  const source = readFileSync(new URL("../src/orders/useOrderDesk.tsx", import.meta.url), "utf8");
  const parts = source.split(/\n  useEffect\(\(\) => \{/);
  const priceEffect = parts.find((part) => part.includes('"rules", "reprice_order"'));
  const freightEffect = parts.find((part) => part.includes('action: "calculate_order_shipping"'));
  assert.ok(priceEffect, "New receptions must invoke the real Rust repricing engine");
  assert.ok(freightEffect, "Auto freight must invoke the real Rust shipping engine");
  assert.match(priceEffect, /!clientSelection\.ready \|\| !liveClient \|\|\s*!passivePricingAllowed/);
  assert.match(freightEffect, /!passivePricingAllowed && !shippingPending/,
    "Saved orders may refresh freight only after a deliberate edit marked pending");
  assert.match(source, /"rules", "should_refresh_pricing"/,
    "Actual React runtime must fetch passive price eligibility from Rust");
  assert.doesNotMatch(source, /shouldRefreshUncommittedOrderPricing\(/,
    "Original TS eligibility is a differential oracle only");
  for (const block of [priceEffect, freightEffect]) {
    assert.match(block, /previous !== draft/,
      "The exact draft must be rechecked before an asynchronous native result is applied");
    assert.match(block, /ruleRequests\.current/,
      "Out-of-order native responses must be rejected");
  }
  assert.match(source, /action: "prepare_order_edit"/,
    "Explicit staff edits must delegate shipping invalidation to Rust");
  const rust = readFileSync(new URL("../src-tauri/src/business_core/rules/order_edit.rs", import.meta.url), "utf8");
  assert.match(rust, /set\(order,"shippingFee",Value::Null\)/);
  assert.match(rust, /set\(order,"shippingVersion",json!\(""\)\)/);
  assert.doesNotMatch(source, /recommendShipping\(previous, state\)/,
    "React must not silently revert to TypeScript tariff computation");
  const orderDesk = readFileSync(new URL("../src/OrderDesk.tsx", import.meta.url), "utf8");
  assert.match(orderDesk, /onDealer=\{async \(c\) => \{[\s\S]*?action: "change_order_client"/,
    "Dealer change must call the compiled Rust price and invoice reducer");
  assert.match(orderDesk, /nativeReconcileWorkflow\(sourceState, next\)/,
    "Saved dealer edits must run Rust workflow reconciliation");
});

test("preview order is not treated as persisted when backing storage has no such order", () => {
  const local = draft({ id:"local-preview" });
  const previewState = [local, { id:"persisted-another-order" }];
  const actualPersisted = [{ id:"persisted-another-order" }];
  assert.equal(shouldRefreshUncommittedOrderPricing(local, previewState), false,
    "The incorrect preview-based lookup would mark the draft as saved");
  assert.equal(shouldRefreshUncommittedOrderPricing(local, actualPersisted), true);
  const workspace=readFileSync(new URL("../src/HubWorkspace.tsx", import.meta.url),"utf8");
  assert.equal((workspace.match(/persistedOrders=\{state\.orders\}/g)??[]).length,2,
    "Both rendered order desks must use canonical persisted records");
});
