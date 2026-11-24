import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const board = read("src/WorkflowBoard.tsx");
const css = read("src/styles/workflow.css");
const source = read("src/SourceDocument.tsx");
const sourceCss = read("src/styles/receipt-document.css");
const main = read("src/main.tsx");
const history = read("src/shared/disable-input-history.ts");
const manager = read("src/ClientManager.tsx");

test("workflow summary uses one line for shipments, deposit and normalized tax status", () => {
  const groupCard = board.slice(board.indexOf('const g = entry.group;'));
  const block = groupCard.slice(groupCard.indexOf('className="flow-fixed-summary"'), groupCard.indexOf('{(g.needsNotice'));
  assert.equal((block.match(/className="flow-summary-compact"/g) || []).length, 1);
  assert.match(block, /배송 \{ss\.length\}건/);
  assert.match(block, /className="flow-summary-payment"/);
  assert.match(block, /vatIncludedCount > 0/);
  assert.match(block, /vatDescription/);
  assert.match(block, /mixedVat/);
  assert.match(block, /won\(nativeGroup\?\.totals\.total \?\? null\)/);
  assert.doesNotMatch(block, /입금액 |\(부가세 별도\)/);
  assert.doesNotMatch(block, /총 운임|입금 예정|미확정 · 임시/);
  const rust = read("src-tauri/src/business_core/rules/workflow_preview.rs");
  for (const label of ["부가세 포함", "부가세 별도", "부가세 포함/별도"]) assert.ok(rust.includes(label));
  assert.doesNotMatch(block, /부가세 적용|부가세 미추가/);
  assert.match(css, /\.flow-summary-compact \{[\s\S]*?white-space: nowrap/);
  assert.match(css, /\.hub-slot-3 \.workflow-group \{[\s\S]*?border: 1px solid/);
});
test("source text remains visually bounded before focus, without unnecessary notice", () => {
  assert.doesNotMatch(source, /확정 원문 \/ 주문 변경은/);
  assert.match(sourceCss, /\.source-pane \.source-document\s*\{[^}]*border: 1px solid #555;/);
});
test("input history suggestions are disabled for future modal input fields too", () => {
  assert.match(main, /disableInputHistorySuggestions\(document\.body\)/);
  assert.match(history, /MutationObserver/);
  assert.match(history, /setAttribute\("autocomplete", "off"\)/);
  assert.match(history, /observer\.observe\(root, \{ childList: true, subtree: true \}\)/);
  assert.match(manager, /<Field label="검색 태그" help=\{\{/);
  assert.doesNotMatch(manager, /<Field label="검색 식별자">/);
  assert.match(manager, /\[edit\.shortNames, edit\.identifiers\]\.filter\(Boolean\)\.join\(", "\)/);
  assert.match(manager, /shortNames: e\.target\.value, identifiers: ""/);
  // Invoice sender displayName was deliberately retired; the canonical client name and search tags replace it.
  assert.doesNotMatch(manager, /<Field label="송장 표시 상호"/);
  for (const label of ["거래처명", "거래처 주소", "전화번호", "휴대폰번호"]) {
    assert.ok(manager.includes('<Field label="' + label + '"'));
  }
  assert.doesNotMatch(manager, /<Field label="[^"]*(?: \*|\(선택\))">/);
});
