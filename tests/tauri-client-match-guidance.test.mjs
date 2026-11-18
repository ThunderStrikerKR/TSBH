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
  assert.match(receipt, /source: staleDealer \? "ERROR" : result\.source/);
  assert.doesNotMatch(receipt, /next\.clientId\s*=\s*matched\.id/);
});

test("staff review shows both evidence and conflict without a second notification popup", () => {
  const issues = read("src-tauri/src/business_core/rules/issues.rs");
  const checks = read("src/StaffChecks.tsx");
  const desk = read("src/orders/useOrderDesk.tsx");
  assert.match(issues, /CLIENT_MATCH_CONFLICT/);
  assert.match(issues, /CLIENT_MATCH_EVIDENCE/);
  assert.match(issues, /if needs_action\{"ACTION"\}else\{"INFO"\}/);
  assert.match(issues, /CLIENT_MATCH_ERROR/);
  assert.match(checks, /CLIENT_MATCH_EVIDENCE/);
  assert.match(checks, /거래처 확인/);
  assert.match(desk, /const autoDealerApplied = b\.kind === "CLIENT_MATCH_EVIDENCE"/);
  assert.match(desk, /draft\.clientId === draft\.clientMatch\?\.clientId/);
  assert.match(desk, /title: autoDealerApplied \? "거래처 자동 적용" : b\.title/);
  assert.match(desk, /거래처를 자동 적용했습니다/);
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

test("user-selected dealer is retained only as provenance, not a review item", () => {
  const editor = read("src-tauri/src/business_core/rules/order_edit.rs");
  const issues = read("src-tauri/src/business_core/rules/issues.rs");
  assert.match(editor, /"source":"MANUAL"/);
  assert.match(editor, /사용자가 거래처/);
  assert.doesNotMatch(editor, /직원이 거래처/);
  assert.match(issues, /source!="MANUAL" && source!="NONE"/);
  assert.match(issues, /manually_selected_dealer_never_emits_a_redundant_review_notice/);
  assert.match(issues, /automatic_match_provenance_and_conflict_still_appear/);
});

test("unmatched dealer generates one required action, not a second blue notice", () => {
  const issues=read("src-tauri/src/business_core/rules/issues.rs");
  const checks=read("src/StaffChecks.tsx");
  assert.match(issues,/no_match_and_no_dealer_has_only_one_actionable_dealer_card/);
  assert.match(issues,/explicit_auto_match_error_is_one_action_not_two/);
  assert.match(issues,/!dealer_match_action/);
  assert.doesNotMatch(issues.split("#[cfg(test)]")[0],/거래처 자동 연결 근거가 없습니다/);
  assert.match(checks,/b\.detail && b\.detail !== b\.title/);
});

test("Rust candidate choices live in staff checks and use native dealer edits", () => {
  const rust=read("src-tauri/src/business_core/catalog_clients/title_clients.rs");
  const source=read("src/SourceDocument.tsx");
  const adapter=read("src/native-title-clients.ts");
  const desk=read("src/OrderDesk.tsx");
  const hook=read("src/orders/useOrderDesk.tsx");
  const checks=read("src/StaffChecks.tsx");
  const css=read("src/styles/patch-v0808.css");
  assert.match(rust,/fn reception_candidates\(payload:&Value\)/);
  assert.match(adapter,/"catalog_clients", "reception_client_candidates"/);
  assert.match(source,/nativeReceptionClientCandidates\(\s*state, chats, raw, activeChatHandle/);
  assert.match(source,/candidates: suggestions\.candidates/);
  assert.match(hook,/draft\.clientMatch\?\.candidates\?\.map/);
  assert.match(desk,/b\.kind === "CLIENT" \|\| b\.kind\?\.startsWith\("CLIENT_MATCH_"\)/);
  assert.match(desk,/void chooseDealer\(candidate\)/);
  assert.match(checks,/b\.choices\?\.map/);
  assert.match(checks,/staff-client-match-info/);
  assert.match(css,/\.staff-checks \.staff-client-match-info/);
  assert.doesNotMatch(source,/<KakaoClients|<ClientPicker/);
  assert.doesNotMatch(source,/setWindows/);
});
