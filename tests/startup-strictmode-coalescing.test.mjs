import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("StrictMode effect replay coalesces only first read-only status without bypassing failure guard",()=>{
 const app=read("src/app/useAppModel.ts");
 assert.match(app,/const initialStartupStatus = useRef<Promise<StartupStatus> \| undefined>\(undefined\)/);
 assert.match(app,/await \(initialStartupStatus\.current \?\?= api\.startupStatus\(\)\)/);
 assert.match(app,/return \(\) => \{ cancelled = true; \}/);
 assert.match(app,/if \(cancelled\) return;/);
 assert.match(app,/if \(status\.phase !== "READY"\) return/);
 assert.match(app,/const next = native\?\.phase === "READY" \? native : await api\.retryStartup\(\)/);
});
