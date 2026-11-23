import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";
const read = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const { handleCellNavigationKey, isCellNavigationKey, isImeComposing, resolveCellNavigationTarget } =
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
  assert.match(price, /handleCellNavigationKey\(e, \{/);
  assert.match(price, /enterMode: "same-column"/);
  assert.match(product, /data-cell-nav-grid="product-prices"/);
  assert.match(read("src/OrderDesk.tsx"), /handleCellNavigationKey\(e\)/);
  assert.match(read("src/WorkflowFooter.tsx"), /handleCellNavigationKey\(e, \{/);
  const manual = read("src/orders/ManualClassification.tsx");
  assert.match(manual, /handleCellNavigationKey\(event, \{/);
  assert.match(manual, /return finishQuantity\(\)/);
  assert.doesNotMatch(manual, /isCellNavigationKey\(/);
  assert.match(manual, /resolveCellNavigationTarget\(/);
  for (const path of [
    "egui-app/src/main.rs",
    "egui-app/src/manual_workspace.rs",
    "egui-app/src/product_manager.rs",
  ]) assert.match(read(path), /navigation_key_from_input\(/);
  assert.doesNotMatch(read("egui-app/src/product_manager.rs"), /fn price_navigation_key\(/);
});

test("shared React key handler saves before moving and focuses the explicit target", () => {
  const calls = [];
  const event = {
    key: "ArrowRight", shiftKey: false, currentTarget: {},
    preventDefault() { this.defaultPrevented = true; },
  };
  const handled = handleCellNavigationKey(event, {
    resolveTarget: () => ({}),
    onBeforeMove: () => { calls.push("save"); },
    focusTarget: () => { calls.push("focus"); return true; },
  });
  assert.equal(handled, true);
  assert.equal(event.defaultPrevented, true);
  assert.deepEqual(calls, ["save", "focus"]);
});

test("shared React key handler awaits asynchronous cell confirmation before focusing", async () => {
  const calls = [];
  let release;
  const event = {
    key: "ArrowDown", shiftKey: false, currentTarget: {},
    preventDefault() { this.defaultPrevented = true; },
  };
  const handled = handleCellNavigationKey(event, {
    resolveTarget: () => ({ id: "next" }),
    onBeforeMove: () => new Promise((resolve) => { release = resolve; }),
    focusTarget: (target) => { calls.push(target.id); return true; },
  });
  assert.equal(handled, true);
  assert.equal(event.defaultPrevented, true);
  assert.deepEqual(calls, []);
  release(true);
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, ["next"]);
});

test("shared React key handler lets Tab exit at the grid boundary and traps arrow boundaries", () => {
  let beforeMove = 0;
  const tab = {
    key: "Tab", shiftKey: false, currentTarget: {},
    preventDefault() { this.defaultPrevented = true; },
  };
  assert.equal(handleCellNavigationKey(tab, {
    resolveTarget: () => null,
    onBeforeMove: () => { beforeMove += 1; },
  }), false);
  assert.equal(tab.defaultPrevented, undefined);
  assert.equal(beforeMove, 1);
  const arrow = {
    key: "ArrowDown", shiftKey: false, currentTarget: {},
    preventDefault() { this.defaultPrevented = true; },
  };
  assert.equal(handleCellNavigationKey(arrow, { resolveTarget: () => null }), true);
  assert.equal(arrow.defaultPrevented, true);
});

test("async Tab boundary validation restores focus when the commit is rejected", async () => {
  const calls = [];
  let release;
  const currentTarget = {
    isConnected: true,
    focus() { calls.push("focus"); },
    scrollIntoView() { calls.push("scroll"); },
  };
  const tab = {
    key: "Tab", shiftKey: false, currentTarget,
    preventDefault() { this.defaultPrevented = true; },
  };
  const handled = handleCellNavigationKey(tab, {
    resolveTarget: () => null,
    onBeforeMove: () => new Promise((resolve) => { release = resolve; }),
  });
  assert.equal(handled, false);
  assert.equal(tab.defaultPrevented, undefined);
  release(false);
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, ["focus", "scroll"]);
});

test("shared React key handler protects IME composition and delegates Escape cancellation", () => {
  const composing = {
    key: "ArrowRight", shiftKey: false, isComposing: true, currentTarget: {},
    preventDefault() { this.defaultPrevented = true; },
  };
  assert.equal(handleCellNavigationKey(composing, { resolveTarget: () => ({}) }), false);
  assert.equal(composing.defaultPrevented, undefined);
  const escape = {
    key: "Escape", shiftKey: false, currentTarget: {},
    preventDefault() { this.defaultPrevented = true; },
  };
  let cancelled = false;
  assert.equal(handleCellNavigationKey(escape, { onEscape: () => { cancelled = true; } }), true);
  assert.equal(escape.defaultPrevented, true);
  assert.equal(cancelled, true);
});
