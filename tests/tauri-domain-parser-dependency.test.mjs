import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("shared domain helpers must not import or re-export the deprecated TS parser", () => {
  const domain = read("src/domain.ts");
  assert.doesNotMatch(domain, /from\s+["']\.\/parser(?:\.js|\.ts)?["']/);
  assert.doesNotMatch(domain, /export\s+const\s+parseOrder\s*=/);
  assert.match(read("src/workflow.ts"), /parseSmart as parseOrder/);
  assert.match(read("tests/rust-migration/oracles/manual-classification.ts"), /parseSmart as parseOrder/);
  assert.match(read("tests/rust-migration/rules/parity/compare.mts"), /parseSmart as parseOrder/);
});
