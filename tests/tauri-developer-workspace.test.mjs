import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const read=(file)=>readFileSync(new URL("../"+file, import.meta.url),"utf8");
const rust=read("src-tauri/src/lib.rs");
const native=read("src-tauri/src/developer_workspace.rs");
const bridge=read("src/tauri-bridge.ts");
async function legacyFixtureNativeCall(domain, operation, payload) {
  // Synthetic contract oracle only. The application does not import TS parsers.
  if (domain === "recognition" && operation === "analyze_order") {
    const { parseEvidence } = await tsImport("./rust-migration/oracles/legacy-parser.ts", import.meta.url);
    const result = parseEvidence(payload.raw, payload.client, payload.state);
    return { recipients: result.recipients, candidates: result.candidates, unassigned: result.unassigned };
  }
  if (domain === "rules" && operation === "order_issues") {
    const { orderIssues } = await tsImport("./rust-migration/oracles/legacy-validation.ts", import.meta.url);
    return { issues: orderIssues(payload.order) };
  }
  throw Error("Unexpected diagnostic IPC operation: " + domain + "." + operation);
}


test("developer workspace can only launch a second process with a separate SQLite root",()=>{
  assert.match(rust,/mod developer_workspace/);
  assert.match(rust,/is_test: bool/);
  const startup=read("src-tauri/src/startup_recovery.rs");
  assert.match(startup,/developer_workspace::open_isolated/);
  assert.match(startup,/preflight_existing_sqlite\(&base\.join\("developer-test"\)/);
  assert.match(native,/Command::new\(std::env::current_exe\(\)/);
  assert.match(native,/--bando-developer-test/);
  assert.match(native,/developer-test/);
  assert.match(native,/bando-hub-test\.sqlite/);
  assert.match(native,/SQLITE_OPEN_READ_ONLY/);
  assert.match(native,/clean_activity/);
  assert.match(native,/"orders", "clients", "aliases"/);
  assert.match(native,/snapshot\["workflow"\]/);
  assert.match(native,/snapshot\["revision"\] = json!\(0\)/);
  assert.match(native,/if !db\.is_test \{/);
  assert.match(native,/VACUUM INTO/);
  assert.match(native,/DELETE FROM receipt_archive/);
  assert.match(native,/DELETE FROM developer_test_runs/);
  assert.match(native,/fn isolated_clone_keeps_master_and_does_not_touch_production_state/);
  assert.match(native,/fn test_reset_preserves_prices_and_archives_old_audit_and_case_runs/);
});

test("native commands reject test-only operations on normal database and isolate file exports",()=>{
  for(const command of [
    "workspace_mode","open_test_workspace","test_database_preview",
    "reset_test_database","test_record_run","test_run_history"
  ]){
    assert.match(bridge,new RegExp('"'+command+'"'));
    assert.match(rust,new RegExp("developer_workspace::"+command));
  }
  assert.match(rust,/workspace_excel_directory/);
  assert.match(rust,/if db\.is_test/);
  assert.match(rust,/테스트 작업공간 Excel 보관함은 변경할 수 없습니다/);
  assert.match(native,/시험 결과는 격리된 작업공간에서만 저장합니다/);
  assert.match(native,/정식 업무 데이터베이스는 개발자 모드에서 초기화할 수 없습니다/);
  assert.match(native,/200/);
});

test("synthetic developer diagnostics preserve report and state via an injected native contract",async()=>{
  const { inspectOrder, exampleRaw, developerCases } =
    await tsImport("./rust-migration/oracles/developer-scenarios.ts", import.meta.url);
  const state=JSON.parse(read("src-tauri/resources/state-seed.json"));
  const original=JSON.stringify(state);
  assert.ok(developerCases.length>=9);
  const raw=exampleRaw("missing-contact",state);
  const report=await inspectOrder(state,"missing-contact",raw,"","NONE",legacyFixtureNativeCall);
  assert.equal(report.scenario,"연락처 누락");
  assert.ok(report.detected.includes("CONTACT"),report.detected.join(","));
  assert.equal(report.passed,true);
  assert.equal(report.raw,raw);
  assert.equal(JSON.stringify(state),original);
  const repeat=await inspectOrder(state,"duplicate-order",exampleRaw("duplicate-order",state),"","NONE",legacyFixtureNativeCall);
  assert.equal(repeat.passed,true,"An in-memory duplicate fixture must be detected");
  assert.ok(repeat.notes.some(x=>x.includes("메모리에만 임시로 추가"))); 
  const inDb=await inspectOrder(state,"duplicate-db",exampleRaw("duplicate-db",state),"","NONE",legacyFixtureNativeCall);
  assert.equal(inDb.passed,false,"The separate DB-only scenario requires a persisted test order");
  await assert.rejects(inspectOrder(state,"custom","","","NONE",legacyFixtureNativeCall),/1자 이상/);
  assert.equal(JSON.stringify(state),original);
});


test("structured native diagnostic report fixtures remain isolated from persistence",async()=>{
  const { inspectOrder, exampleRaw, developerCases, expectedOptions } =
    await tsImport("./rust-migration/oracles/developer-scenarios.ts", import.meta.url);
  const state=JSON.parse(read("src-tauri/resources/state-seed.json"));
  const unchanged=JSON.stringify(state);
  const ids=[
    "no-client","no-recipient","quantity-invalid","price-missing",
    "option-unconfirmed","alias-conflict","category-conflict","model-choice",
    "quantity-link","candidate-name","candidate-address","candidate-phone",
    "candidate-mobile","unassigned-evidence","model-confirm",
  ];
  for(const id of ids){
    const report=await inspectOrder(state,id,exampleRaw(id,state),"","NONE",legacyFixtureNativeCall);
    assert.equal(report.passed,true,
      id+" expected "+report.expected.join(",")+" actual "+report.detected.join(","));
    assert.ok(report.notes.some(note=>note.includes("복사된 주문의 검증 필드")),id);
  }
  assert.ok(developerCases.length>=25);
  assert.ok(developerCases.some(entry=>entry.id==="duplicate-db"&&entry.batch===false));
  assert.ok(expectedOptions.some(entry=>entry.value==="CANDIDATE_mobile"));
  assert.equal(JSON.stringify(state),unchanged,"The real state must be unchanged");
});

test("archived developer diagnostic fixture calls Rust and never TS fallback",()=>{
  const scenarios=read("tests/rust-migration/oracles/developer-scenarios.ts");
  assert.doesNotMatch(scenarios,/from ["']\.\.\/parser(?:\.js)?["']/);
  assert.doesNotMatch(scenarios,/from ["']\.\.\/validation(?:\.js)?["']/);
  assert.match(scenarios,/"recognition", "analyze_order"/);
  assert.match(scenarios,/"rules", "order_issues"/);
  assert.match(scenarios,/assertRustReceptionAnalysis/);
});
