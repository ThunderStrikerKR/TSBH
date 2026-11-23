import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const source = (file) => readFileSync(fileURLToPath(new URL("../src/" + file, import.meta.url)), "utf8");

test("price cells autosave on blur and expose no per-cell Save/Cancel controls", () => {
  const cell = source("PriceCell.tsx");
  const css = source("styles/table-density.css");
  const blur = cell.slice(cell.indexOf("onBlur={() => {"), cell.indexOf("onKeyDown={(e) => {"));
  assert.ok(blur.length > 0 && blur.length < 1200);
  assert.match(blur, /if \(pending\) \{\s*void save\(\);/);
  assert.match(blur, /if \(pending\) \{\s*void save\(\);/);
  assert.match(cell, /handleCellNavigationKey\(e, \{/);
  assert.match(cell, /enterMode: "same-column"/);
  assert.match(cell, /if \(e\.key === "Escape"\)[\s\S]*?cancel\(\)/);
  assert.doesNotMatch(cell, /price-cell-actions|price-cell-save|price-cell-cancel|<button/);
  assert.doesNotMatch(css, /price-cell-actions|price-cell-save|price-cell-cancel/);
  assert.match(css, /\.price-cell\.pending/);
});

test("automatic cell commits use expected-value native writes serialized through one queue", () => {
  const cell = source("PriceCell.tsx");
  const manager = source("ProductManager.tsx");
  assert.match(cell, /await onSave\(next, expected\.current\)/);
  assert.match(cell, /catch \(e\) \{[\s\S]*?setError\(readableError\(e\)\)/);
  assert.match(cell, /optimisticCommit\.current = \{ next \}/);
  assert.match(cell, /if \(optimistic && value !== optimistic\.next\) return/);
  assert.match(manager, /await window\.bando!\.setPrice\(p\.id, v\.id, tier, n, expected\)/);
  assert.match(manager, /await onRefresh\(committed\)/);
  assert.match(manager, /priceWriteChain\.current = write\.catch\(\(\) => \{\}\)/);
  assert.match(manager, /priceWrites\.current\+\+/);
  assert.match(manager, /finally \{\s*priceWrites\.current--/);
  assert.doesNotMatch(manager, /const \{ next \} = .*priceHistory/);
});

test("price draft state distinguishes unsaved input from an in-flight write and guards navigation", () => {
  const cell = source("PriceCell.tsx");
  const manager = source("ProductManager.tsx");
  const css = source("styles/table-density.css");
  assert.match(cell, /data-price-pending=\{pending\}/);
  assert.match(cell, /data-price-saving=\{busy\}/);
  assert.match(cell, /if \(lastReportedPending\.current === pending\) return/);
  assert.match(cell, /pendingRef\.current\?\.\(pending\)/);
  assert.match(cell, /if \(lastReportedPending\.current\) \{[\s\S]*?pendingRef\.current\?\.\(false\)/);
  assert.match(manager, /hasChanges: \(\) => editDirty \|\| hasPendingPrices/);
  assert.match(manager, /busy: \(\) => savingView \|\| priceWrites\.current > 0/);
  assert.match(manager, /const pendingPriceIdsRef = useRef<string\[\]>\(\[\]\)/);
  assert.match(manager, /const checkPriceDrafts = \(\) => \{/);
  assert.match(manager, /if \(pendingPriceIdsRef\.current\.length === 0\) return true/);
  assert.match(manager, /const togglePriceMode = \(\) => \{[\s\S]*?if \(pendingPriceIdsRef\.current\.length > 0\)/);
  assert.match(manager, /onChange=\{\(value\) => \{ if \(checkPriceDrafts\(\)\) setSearch\(value\); \}\}/);
  assert.match(manager, /disabled=\{priceWrites\.current > 0 \|\| pendingPriceIdsRef\.current\.length > 0\}/);
  assert.match(cell, /displayedValue === null/);
  assert.match(css, /\.price-cell\.busy/);
  assert.doesNotMatch(css, /price-cell-actions/);
});
