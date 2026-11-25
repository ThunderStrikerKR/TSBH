// Browser-only test bridge. The production dispatcher is pure: OS, SQLite,
// revision CAS and deliberate fault-injection remain explicit page mocks.
// Never fall back for unknown Tauri commands or swallow Rust domain errors.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import assert from "node:assert/strict";

const root=fileURLToPath(new URL("../",import.meta.url));
const manifest=join(root,"tests","rust-migration","integration","Cargo.toml");
const target=join(root,"tests","rust-migration","integration","target");
const executable=join(target,"debug",process.platform==="win32"?"reception_runner.exe":"reception_runner");
let built=false;
function ensureRunner(){
  if(built)return;
  // Cargo checks source freshness; never silently reuse a stale fixture.
  const result=spawnSync("cargo",["build","--quiet","--manifest-path",manifest,
    "--bin","reception_runner","--target-dir",target],
    {cwd:root,encoding:"utf8",timeout:300000,maxBuffer:16*1024*1024});
  if(result.error)throw result.error;
  assert.equal(result.status,0,"Rust fixture build failed: "+result.stderr);
  built=true;
}
export async function installNativeBusinessFixture(page){
  ensureRunner();
  const seen=new Set();
  await page.exposeBinding("__nativeBusinessFixture",(_source,args)=>{
    const {domain,operation,payload}=args??{};
    if(!["recognition","rules","output","catalog_clients"].includes(domain)||
      typeof operation!=="string"||!operation.trim())
      throw Error("Invalid Rust business fixture request");
    const key=domain+"/"+operation;
    if(!seen.has(key)){seen.add(key);console.log("NATIVE_FIXTURE "+key);}
    const result=spawnSync(executable,[],{
      cwd:root,input:JSON.stringify([{domain,operation,payload}]),
      encoding:"utf8",timeout:30000,maxBuffer:32*1024*1024
    });
    if(result.error)throw result.error;
    assert.equal(result.status,0,"Rust fixture process failed for "+key+": "+result.stderr);
    const responses=JSON.parse(result.stdout);
    assert.equal(responses.length,1,"Rust fixture response count");
    const answer=responses[0];
    if(answer.ok!==true)throw Error(answer.error??("Invalid Rust fixture response: "+key));
    return answer.value;
  });
}
