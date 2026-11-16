import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
const read = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("automatic dealer selection stores the Rust match evidence and title count", () => {
  const rust = read("src-tauri/src/business_core/catalog_clients/title_clients.rs");
  const editor = read("src-tauri/src/business_core/rules/order_edit.rs");
  const receipt = read("src/SourceDocument.tsx");
  assert.match(rust, /fn confirmed_title_count/);
  assert.match(rust, /"confirmedMatchCount":count/);
  assert.match(rust, /"chatTitle":title/);
  assert.match(rust, /"source":"CONFLICT"/);
  assert.match(editor, /"chatTitle":match_info\.get\("chatTitle"\)/);
  assert.match(editor, /"clientMatch",guidance/);
  assert.match(receipt, /matchEvidence: result/);
  assert.match(receipt, /source: result\.source/);
  assert.doesNotMatch(receipt, /next\.clientId\s*=\s*matched\.id/);
});

test("staff review shows both evidence and conflict without a second notification popup", () => {
  const issues = read("src-tauri/src/business_core/rules/issues.rs");
  const checks = read("src/StaffChecks.tsx");
  const desk = read("src/orders/useOrderDesk.tsx");
  assert.match(issues, /CLIENT_MATCH_CONFLICT/);
  assert.match(issues, /CLIENT_MATCH_EVIDENCE/);
  assert.match(issues, /if collision\{"ACTION"\}else\{"INFO"\}/);
  assert.match(checks, /CLIENT_MATCH_EVIDENCE/);
  assert.match(checks, /거래처 확인/);
  assert.match(desk, /\.source-top \.receipt-client button/);
});

test("stage-one signal hover highlights dealer identity alongside order review", () => {
  const css = read("src/styles/receipt-layout.css");
  const board = read("src/WorkflowBoard.tsx");
  assert.match(css, /data-hover-stage="1"\] \.hub-slot-1 \.source-top \.receipt-client/);
  assert.match(css, /data-hover-stage="1"\] \.hub-slot-1 \.source-top \.source-client-actions/);
  assert.match(board, /onMouseEnter/);
  assert.match(board, /onStageHover/);
});
