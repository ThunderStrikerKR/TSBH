import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("ordinary order quantity never confirms a JavaScript-unsafe integer", () => {
  const source = read("src/OrderDesk.tsx");
  assert.match(source, /if \(q !== null && \(\!Number\.isSafeInteger\(q\) \|\| q < 1\)\) return/);
  assert.doesNotMatch(source, /if \(q !== null && \(\!Number\.isInteger\(q\)/);
  assert.match(source, /aria-invalid=\{[\s\S]*?!Number\.isSafeInteger\(i\.quantity\)/);
});

test("factory corrections cannot mark a null, fractional or unsafe quantity confirmed", () => {
  const source = read("src/CorrectionPanel.tsx");
  const start = source.indexOf('aria-label={"정정 수량 "');
  assert.ok(start >= 0);
  const control = source.slice(start, source.indexOf("</input>",start) >= 0 ? source.indexOf("</input>",start) : start+1300);
  assert.match(control, /\!Number\.isSafeInteger\(quantity\) \|\| quantity < 1/);
  assert.match(control, /quantityConfirmed: quantity !== null/);
  assert.match(control, /정정 수량은 1 이상의 안전한 정수로 입력하세요/);
});

test("retail direct-entry quantities permit clearing a field but reject unsafe values", () => {
  const source = read("src/orders/RetailOrderDialog.tsx");
  const a = source.indexOf('aria-label={"상품 수량 "');
  assert.ok(a >= 0);
  const section = source.slice(a, a+1800);
  assert.match(section, /value=\{item\.quantity \?\? ""\}/);
  assert.match(section, /const quantity = e\.target\.value === "" \? null : Number\(e\.target\.value\)/);
  assert.match(section, /\!Number\.isSafeInteger\(quantity\) \|\| quantity < 1/);
  assert.match(section, /quantityConfirmed: quantity !== null/);
  assert.match(source, /const quantity = \(item\.quantity \?\? 1\) \+ 1;\s*if \(\!Number\.isSafeInteger\(quantity\)\) return/);
});

test("quantity boundary values distinguish exact safe integers from rounded values", () => {
  const valid = (value) => Number.isSafeInteger(value) && value >= 1;
  assert.equal(valid(1), true);
  assert.equal(valid(Number.MAX_SAFE_INTEGER), true);
  assert.equal(valid(Number.MAX_SAFE_INTEGER + 1), false);
  assert.equal(valid(1.5), false);
  assert.equal(valid(0), false);
  assert.equal(valid(-1), false);
  assert.equal(valid(Infinity), false);
  assert.equal(valid(NaN), false);
});
