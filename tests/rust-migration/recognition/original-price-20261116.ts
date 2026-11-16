// Verbatim resolvePrice function from 30a19cd2da7533dd2ed36b807593309bde70a7eb; only type import added.
import type { Product, Client } from "../../../src/types.js";
export function resolvePrice(p: Product, variantId: string, client: Client) {
  if (!client.id) return null;
  const v = p.variants.find((v) => v.id === variantId);
  const value =
    client.tier === "special"
      ? (client.overrides?.[variantId] ??
        v?.legacyIds?.map((id) => client.overrides?.[id]).find((p) => p !== undefined) ??
        v?.wholesale ??
        null)
      : (v?.[client.tier] ?? null);
  return value === 0 ? null : value;
}
