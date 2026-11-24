import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
const load=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
const ledger=JSON.parse(load("docs/rust-migration/verification-ledger.json"));
const policy=load("AGENTS.md");
test("all 4 user-approved architecture and parity directives are checked in",()=>{
  for(const exact of [
    "기능별로 확정된 TypeScript 원본과 최신 개선 사항",
    "검증 결과를 기능별로 누적 관리하고 중복 조사를 방지",
    "실제 사용자 행동과 비동기 처리 과정을 포함해 기능",
    "불일치가 해결되고 검증되기 전까지 완료 판정을 보류",
    "모든 업무 판단과 계산은 Rust가 담당",
  ])assert.ok(policy.includes(exact),exact);
});
test("28 function groups are tracked with baseline provenance and separate verification evidence",()=>{
  assert.equal(ledger.schemaVersion,1);
  assert.equal(ledger.features.length,28);
  assert.deepEqual(ledger.features.map(f=>f.id),Array.from({length:28},(_,i)=>"F"+String(i+1).padStart(2,"0")));
  for(const f of ledger.features){
    assert.ok(f.name&&f.baseline?.historicalSha&&f.baseline.historicalSource,f.id+" baseline");
    assert.ok(f.rust?.location&&Array.isArray(f.react?.candidatePaths),f.id+" execution");
    assert.equal(typeof f.react.entrypointVerified,"boolean",f.id+" React entrypoint evidence flag");
    if(f.react.entrypointVerified){
      assert.ok(f.react.candidatePaths.length>0,f.id+" missing React source path");
      if (f.gate.windows?.status === "PASS") {
        assert.ok(f.gate.windows?.evidence,f.id+" runtime path needs actual Windows evidence");
      }
    }
    assert.ok(f.scenario?.description&&Array.isArray(f.scenario.responseOrders),f.id+" user scenario");
    assert.ok(Array.isArray(f.discrepancies),f.id+" discrepancy");
    for(const gate of ledger.proofLevels) {
      const data=f.gate?.[gate];
      assert.ok(data?.status && "commit" in data && "evidence" in data, f.id+" "+gate);
      if(data.status==="PASS"||data.status==="PASS_SCOPED"){
        assert.ok(data.commit&&data.evidence,f.id+" "+gate+" lacks linked proof");
      }
    }
    if(f.overall==="COMPLETE"){
      assert.ok(f.baseline.selectedApprovedSha&&f.baseline.approvedEvidence);
      assert.ok(f.react.entrypointVerified);
      assert.ok(f.scenario.fixture&&f.scenario.inputOutputEvidence);
      assert.ok(f.scenario.responseOrders.includes("REVERSED"));
      assert.equal(f.discrepancies.filter(x=>x.status!=="RESOLVED").length,0);
      for(const gate of ledger.proofLevels)assert.equal(f.gate[gate].status,"PASS");
      const commit=f.gate.ci.commit;
      assert.equal(f.gate.windows.commit,commit,"different Windows SHA");
    }else {
      assert.ok(["OPEN","BLOCKED"].includes(f.overall));
    }
  }
});
test("historical TS SHA cannot silently be promoted to latest approved behavior",()=>{
  for(const f of ledger.features){
    if(f.baseline.selectedApprovedSha===null){
      assert.equal(f.baseline.status,"BASELINE_UNRESOLVED",f.id);
    }else{
      assert.notEqual(f.baseline.selectedApprovedSha,f.baseline.historicalSha);
      assert.ok(f.baseline.approvedEvidence);
    }
  }
});
