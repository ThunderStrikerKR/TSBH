import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root=fileURLToPath(new URL("../",import.meta.url));
function files(dir){
  return readdirSync(dir,{withFileTypes:true}).flatMap(entry=>{
    const path=join(dir,entry.name);
    return entry.isDirectory()?files(path):
      /\.(?:mjs|mts|ts|tsx)$/.test(entry.name)?[path]:[];
  });
}
test("all test-only legacy TS imports and source reads point to existing files",()=>{
  const missing=[];
  for(const file of files(join(root,"tests"))){
    const code=readFileSync(file,"utf8");
    // Importing original TS functions through tsx/esm must resolve to real files.
    for(const match of code.matchAll(/\btsImport\(\s*["'](\.{1,2}\/[^"']+\.ts)["']/g)){
      const target=resolve(dirname(file),match[1]);
      if(!existsSync(target))missing.push(file.slice(root.length)+": "+match[1]);
    }
    // Source-inspecting tests cannot retain a deleted production path either.
    for(const match of code.matchAll(/\b(?:read|source|load)\(\s*["'](src\/[^"']+\.(?:ts|tsx))["']/g)){
      if(!existsSync(join(root,match[1])))
        missing.push(file.slice(root.length)+": "+match[1]);
    }
  }
  assert.deepEqual(missing,[],"Retired TypeScript implementations must be referenced through tests/rust-migration/oracles");
});
