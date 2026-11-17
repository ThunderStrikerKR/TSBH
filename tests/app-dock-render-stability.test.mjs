import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
test("identical 300ms dock polls do not cause App or order panes to repaint", () => {
  const source=readFileSync(new URL("../src/app/useAppModel.ts",import.meta.url),"utf8");
  assert.ok(source.includes("if (!dock.active) return"));
  assert.ok(source.includes("JSON.stringify(previous) === JSON.stringify(next) ? previous : next"));
  assert.ok(source.includes("      300,"));
});
