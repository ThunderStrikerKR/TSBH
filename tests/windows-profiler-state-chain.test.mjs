import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("native Windows audit measures real React commit actualDuration and IPC-resolved state chronology",()=>{
  const profiler=read("src/perf-audit-profiler.tsx");
  const hub=read("src/HubWorkspace.tsx");
  const driver=read("scripts/migration32-dev-business-ui.mjs");
  assert.match(profiler,/ProfilerOnRenderCallback/);
  assert.match(profiler,/actualDuration,baseDuration,startTime,commitTime/);
  assert.match(profiler,/VITE_BANDO_NATIVE_AUDIT/);
  for(const id of ["AppRoot","WorkflowBoard","MessageWorkspace","OrderDesk:reception","OrderDesk:details"]){
    assert.ok(read("src/main.tsx").includes('id="'+id+'"')||hub.includes('id="'+id+'"'),id);
  }
  assert.match(driver,/PERF_INITIAL_BOOT_REACT_NATIVE_CHAIN/);
  assert.match(driver,/profilerByComponent/);
  assert.match(driver,/transitionCounts/);
  const bridge=read("src/tauri-bridge.ts");
  assert.match(bridge,/auditStateTransition\("Rust IPC","resolved"/);
  assert.match(bridge,/auditStateTransition\("Rust IPC","state-load-resolved"/);
});
