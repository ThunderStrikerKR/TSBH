import type { Product } from "../types";
import { registeredProducts, sortProducts } from "../product-models";

/** Both live OrderDesk panes use the same immutable State.products array.
 * Cache only a display projection by exact array identity. New catalog
 * revisions use a new array and trigger a new projection. No Rust business
 * values, prices or product matching results are cached here.
 */
const cachedViews = new WeakMap<Product[], Product[]>();
export function sortedRegisteredProductView(products: Product[]): Product[] {
  const existing = cachedViews.get(products);
  if (existing) return existing;
  const sorted = sortProducts(registeredProducts(products));
  cachedViews.set(products, sorted);
  return sorted;
}
