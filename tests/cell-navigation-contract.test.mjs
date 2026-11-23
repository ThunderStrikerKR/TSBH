import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";
const read = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const { isCellNavigationKey, isImeComposing, resolveCellNavigationTarget } =
  await tsImport("../src/ui/cell-navigation.ts", import.meta.url);
const grid = [
  { id: "a", row: 0, column: 0, order: 0 },
  { id: "b", row: 0, column: 1, order: 1 },
  { id: "c", row: 1, column: 0, order: 2 },
  { id: "d", row: 1, column: 1, order: 3, disabled: true },
  { id: "e", row: 2, column: 1, order: 4 },
];
test("arrows use explicit row/column coordinates", () => {
  assert.equal(resolveCellNavigationTarget(grid, "a", "ArrowRight"), "b");
  assert.equal(resolveCellNavigationTarget(grid, "b", "ArrowLeft"), "a");
  assert.equal(resolveCellNavigationTarget(grid, "a", "ArrowDown"), "c");
  assert.equal(resolveCellNavigationTarget(grid, "b", "ArrowDown"), "e");
  assert.equal(resolveCellNavigationTarget(grid, "a", "ArrowLeft"), null);
  assert.equal(resolveCellNavigationTarget(grid, "e", "ArrowDown"), null);
});
test("Tab and Enter support forward/reverse order and price-column mode", () => {
  assert.equal(resolveCellNavigationTarget(grid, "b", "Tab"), "c");
  assert.equal(resolveCellNavigationTarget(grid, "c", "Tab", true), "b");
  assert.equal(resolveCellNavigationTarget(grid, "b", "Enter", false, "same-column"), "e");
  assert.equal(resolveCellNavigationTarget(grid, "e", "Enter", true, "same-column"), "b");
});
test("disabled and read-only cells are skipped", () => {
  const cells = [...grid, { id: "f", row: 3, column: 1, order: 5, readOnly: true }];
  assert.equal(resolveCellNavigationTarget(cells, "b", "ArrowDown"), "e");
  assert.equal(resolveCellNavigationTarget(cells, "d", "Tab"), null);
  assert.equal(resolveCellNavigationTarget(cells, "e", "Tab"), null);
});
test("IME composition blocks navigation keys", () => {
  assert.equal(isImeComposing({ nativeEvent: { isComposing: true } }), true);
  assert.equal(isImeComposing({ keyCode: 229 }), true);
  assert.equal(isImeComposing({ nativeEvent: { isComposing: false }, keyCode: 13 }), false);
  for (const key of ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Enter", "Tab"])
    assert.equal(isCellNavigationKey(key), true);
  assert.equal(isCellNavigationKey("Escape"), false);
});
test("React cell editors use the shared contract and retain manual quantity gating", () => {
  const price = read("src/PriceCell.tsx");
  const product = read("src/ProductManager.tsx");
  assert.match(price, /findCellNavigationTarget\(\s*e\.currentTarget, e\.key, e\.shiftKey, "same-column"/);
  assert.match(price, /isImeComposing\(e\)/);
  assert.match(product, /data-cell-nav-grid="product-prices"/);
  assert.match(read("src/OrderDesk.tsx"), /data-cell-nav-grid="order-intake"/);
  assert.match(read("src/WorkflowFooter.tsx"), /findCellNavigationTarget\(e\.currentTarget, e\.key, e\.shiftKey\)/);
  assert.match(read("src/orders/ManualClassification.tsx"), /resolveCellNavigationTarget\(/);
});
