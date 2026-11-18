import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const read = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("empty reception hides reserved dealer/action rows instead of shrinking the guide", () => {
  const source = read("src/SourceDocument.tsx");
  const css = read("src/styles/receipt-document.css");
  assert.match(source, /waitingForSource \? " source-intake-mode" : ""/);
  assert.match(source, /waitingForSource \? \(/);
  assert.match(source, /className="source-intake-choice"/);
  assert.match(css, /\.source-pane\.source-intake-mode > \.source-top,[\s\S]*?\.source-pane\.source-intake-mode > \.source-actions,[\s\S]*?display: none;/);
  assert.match(css, /\.source-pane\.source-intake-mode > \.source-inline-status:empty/);
  assert.match(source, /\{!formal && !!text && \(/);
});

test("empty intake instructions fit available space without an always-on scrollbar", () => {
  const css = read("src/styles/receipt-document.css");
  const source = read("src/SourceDocument.tsx");
  const choice = css.match(/\.source-intake-choice\s*\{([^}]+)\}/)?.[1] ?? "";
  assert.match(choice, /height: auto;/);
  assert.match(choice, /justify-content: safe center;/);
  assert.match(choice, /align-items: safe center;/);
  assert.match(choice, /overflow-y: auto;/,
    "Scroll remains available only if the pane is physically too short, never by hiding content");
  assert.doesNotMatch(choice, /height: 100%|overflow: hidden/);
  assert.match(css, /\.source-intake-inner\s*\{[^}]*width: min\(100%, 440px\);/s);
  for (const title of ["클립보드 접수", "다른 방법으로 접수", "폴더 자동 인식"]) {
    assert.ok(source.includes(title), title + " must remain accessible");
  }
  assert.match(source, /source-raw-content/);
  assert.match(css, /\.source-raw-content\s*\{[^}]*overflow: hidden;/s,
    "Real pasted order remains in the separate raw-view layout");
});
