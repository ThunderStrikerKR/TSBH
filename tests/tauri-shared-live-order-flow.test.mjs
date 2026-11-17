import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
const read = path => readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("both panes reuse only the exact matching parent Rust preview", () => {
  const hub = read("src/HubWorkspace.tsx");
  const order = read("src/orders/useOrderDesk.tsx");
  const reconcile = read("src/orders/useNativeWorkflowReconcile.ts");
  assert.match(hub, /sharedCurrentDraftFlow = draft\.clientId[\s\S]*?input: base, draft, value: nativePreview/);
  assert.match(hub, /sharedNativeFlow=\{receptionOrder === draft \? sharedCurrentDraftFlow : undefined\}/);
  assert.match(hub, /sharedNativeFlow=\{\(detail \?\? draft\) === draft \? sharedCurrentDraftFlow : undefined\}/);
  assert.match(order, /sharedNativeFlow\?\.input === state && sharedNativeFlow\.draft === draft/);
  assert.match(order, /useNativeWorkflowReconcile\(state, overlay, !upstreamFlow\)/);
  assert.match(reconcile, /if \(!enabled \|\| !latest\.current\.drafts\.length\) return/);
});
test("passive price, freight, and prepare edits have exactly one active pane owner", () => {
  const hub=read("src/HubWorkspace.tsx");
  const order=read("src/orders/useOrderDesk.tsx");
  assert.match(hub, /suppressPassiveOrderEffects=\{receptionOrder === draft && \(detail \?\? draft\) === draft\}/);
  assert.match(order, /if \(suppressPassiveOrderEffects \|\| !draft\.pendingNativeEdit\) return/);
  assert.match(order, /if \(suppressPassiveOrderEffects \|\| !liveClient/);
  assert.match(order, /if \(suppressPassiveOrderEffects \|\|[\s\S]*?shippingPending/);
  assert.match(order, /suppressPassiveOrderEffects,\s*\]\)/);
});
