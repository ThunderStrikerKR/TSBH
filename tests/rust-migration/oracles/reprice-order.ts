/** Historical TS repricing oracle; production pricing is Rust-owned. */
import { resolvePrice } from "../../../src/domain.js";
import type { Order, Client, State } from "../../../src/types.js";
export function repriceOrder(order: Order, client: Client, state: State): Order {
  const o = structuredClone(order);
  for (const r of o.recipients)
    for (const i of r.items) {
      const p = state.products.find((p) => p.id === i.productId);
      if (p && i.variantId) {
        i.unitPrice = resolvePrice(p, i.variantId, client);
        if (i.modelProposal?.required)
          i.url = p.variants.find((v) => v.id === i.variantId)?.url ?? i.url;
      }
    }
  return o;
}
