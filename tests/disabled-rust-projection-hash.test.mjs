import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
const read=p=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("disabled workflow and financial Rust projections never hash whole state",()=>{
 const workflow=read("src/orders/useNativeWorkflowReconcile.ts");
 const board=read("src/orders/useNativeWorkflowBoard.ts");
 const totals=read("src/orders/useNativeOrderPreview.ts");
 assert.match(workflow,/!enabled \|\| drafts\.length === 0 \? "" : JSON\.stringify\(\{ input, drafts \}\)/);
 assert.match(workflow,/\[input, drafts, enabled\]/);
 assert.match(workflow,/if \(!enabled \|\| !latest\.current\.drafts\.length\) return/);
 assert.match(board,/enabled \? JSON\.stringify\(state\) : ""/);
 assert.match(board,/\[state, enabled\]/);
 assert.match(totals,/enabled \? JSON\.stringify\(\{ state, draft \}\) : ""/);
 assert.match(totals,/nativeOrderDisplayKey\(sourceState, draft\)/);
 assert.match(totals,/if \(!enabled\) return/);
});
