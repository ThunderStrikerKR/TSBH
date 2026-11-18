/** View-only category choices for the official product import screen.
 * Business approval and product mutation are owned by native Rust. */
import { categoryPath } from "./category-tree";
import { catalogFromCategory, withCatalogRoots } from "./category-roots";
import type { Catalog, State } from "./types";

export function leafCategories(state: State, catalog: Catalog) {
  const cats = withCatalogRoots(state.categories);
  return cats.filter((item) => state.categories?.some((c) => c.id === item.id) &&
    item.active !== false && !cats.some((other) => other.parentId === item.id) &&
    catalogFromCategory(cats, item.id) === catalog).map((item) => ({
      id: item.id,
      label: categoryPath(cats, item.id).map((c) => c.name).join(" / "),
    }));
}
