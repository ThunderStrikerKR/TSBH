#!/usr/bin/env node
import { readdirSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

export function tokenizeRust(source) {
  // Preserve quoted text for diagnostics/fingerprints; ignore comments and
  // braces inside Rust string literals, including raw strings.
  const tokens=[]; let i=0,line=1;
  const advance=(n)=>{line+=(source.slice(i,i+n).match(/\n/g)||[]).length;i+=n;};
  while(i<source.length){
    const tail=source.slice(i);
    const ws=tail.match(/^\s+/); if(ws){advance(ws[0].length);continue;}
    if(tail.startsWith("//")){const end=tail.indexOf("\n");advance(end<0?tail.length:end);continue;}
    if(tail.startsWith("/*")){let j=2,depth=1;while(j<tail.length&&depth){if(tail.startsWith("/*",j)){depth++;j+=2;}else if(tail.startsWith("*/",j)){depth--;j+=2;}else j++;}advance(j);continue;}
    const raw=tail.match(/^(?:br|r)(#*)"/);
    if(raw){const end=tail.indexOf('"'+raw[1],raw[0].length);const n=end<0?tail.length:end+1+raw[1].length;tokens.push({v:tail.slice(0,n),line,kind:"literal"});advance(n);continue;}
    const lit=tail.match(/^(?:b)?"(?:\\[\s\S]|[^"\\])*"|^(?:b)?'(?:\\[\s\S]|[^'\\\n])'/);
    if(lit){tokens.push({v:lit[0],line,kind:"literal"});advance(lit[0].length);continue;}
    const m=tail.match(/^(?:r#)?[A-Za-z_][A-Za-z_0-9]*|^\d+(?:\.\d+)?|^(?:::|&&|\|\||=>|==|!=|<=|>=)|^[\s\S]/);
    tokens.push({v:m[0],line,kind:"code"});advance(m[0].length);
  }
  return tokens;
}

function scan(source) {
  const tokens=tokenizeRust(source);
  const v=tokens.map(t=>t.v);const pairs=new Map(),stack=[];
  for(let j=0;j<v.length;j++){if(tokens[j].kind==="literal")continue;if(["(","[","{"].includes(v[j]))stack.push(j);else if([")","]","}"].includes(v[j])){const a=stack.pop();if(a!==undefined&&{"(":")","[":"]","{":"}"}[v[a]]===v[j]){pairs.set(a,j);pairs.set(j,a);}}}
  const state=/^(?:.*(?:error|warning|notice|guidance|feedback|status|pending|busy|loading|ready|dirty|conflict|unassigned|blocked|validation|issues|receiver|empty|retry|edited|failure|success|can_apply|can_save|can_mutate).*)$/i;
  const guarded=(a,b)=>v.slice(a,b).some((t,j)=>tokens[a+j].kind==="code"&&state.test(t));
  // Match the same shared rendering call across a namespace-only extraction.
  // Keep the guard, arguments, literals and occurrence counts intact. Only
  // reviewed old/new shared function names may be canonicalized.
  const actionNames=new Set(["action","action_with_min_width","action_full_width","review_action"]);
  const dropdownNames=new Set(["management_dropdown_values","management_dropdown_option_values"]);
  const norm=(a,b)=>{
    const out=[];
    for(let i=a;i<b;i++){
      let start=i;
      if(v[i]==="crate"&&v[i+1]==="::")start=i+2;
      if(start+2<b&&tokens[start].kind==="code"&&v[start+1]==="::"){
        const module=v[start],name=v[start+2];
        if((module==="theme"||module==="button")&&actionNames.has(name)){
          out.push("shared_button","::",name);i=start+2;continue;
        }
        if((module==="components"||module==="button")&&name==="reception_item_header_action"){
          out.push("shared_button","::",name);i=start+2;continue;
        }
        if((module==="components"||module==="ui_layout")&&["fixed_row","fixed_status_slot","fixed_result_viewport","management_top_content"].includes(name)){
          out.push("shared_layout","::",name);i=start+2;continue;
        }
        if((module==="components"||module==="dropdown")&&dropdownNames.has(name)){
          out.push("shared_dropdown","::",name);i=start+2;continue;
        }
      }
      out.push(v[i]);
    }
    return out.join(" ");
  };
  const tests=[],safe=[],guards=[];
  const safeNames=new Set(["fixed_row","fixed_status_slot","fixed_result_viewport","context_menu","on_hover_ui","on_hover_ui_at_pointer"]);
  for(let j=0;j<v.length;j++){
    if(tokens[j].kind==="literal")continue;
    if(v[j]==="#"&&norm(j,j+7)==="# [ cfg ( test ) ]"){
      let k=j+7;while(k<v.length&&v[k]!=="{"&&v[k]!==";")k++;if(v[k]==="{"&&pairs.has(k))tests.push([j,pairs.get(k)]);
    }
    if(v[j]==="("&&safeNames.has(v[j-1])&&pairs.has(j))safe.push([j,pairs.get(j)]);
    if(v[j]!=="if"&&v[j]!=="match")continue;
    let k=j+1;while(k<v.length&&v[k]!=="{"&&v[k]!==";"){if(["(","["].includes(v[k])&&pairs.has(k))k=pairs.get(k)+1;else k++;}
    if(v[k]!=="{"||!pairs.has(k)||!guarded(j+1,k))continue;
    const head=norm(j,k);const end=pairs.get(k);
    // In an if-condition, an action after a short-circuit guard may itself
    // allocate UI before the branch opens (the round-88 failure).
    guards.push({start:j+1,body:k,end,head});
    let e=end+1;
    if(v[e]==="else"&&v[e+1]==="{"&&pairs.has(e+1))guards.push({start:e+1,body:e+1,end:pairs.get(e+1),head:"else "+head});
  }
  const hits=[];
  const direct=new Set(["label","colored_label","small","weak","add_space","separator","spinner","button","checkbox","add","add_sized","allocate_ui","allocate_ui_with_layout","allocate_exact_size","horizontal","horizontal_wrapped","vertical"]);
  const shared=new Set(["action","action_with_min_width","action_full_width","review_action","soft_divider","reception_item_header_action","fixed_row","fixed_status_slot","fixed_result_viewport"]);
  for(let j=0;j<v.length;j++){
    if(tokens[j].kind==="literal"||tests.some(([a,b])=>j>=a&&j<=b))continue;
    if(v[j+1]!=="("||!pairs.has(j+1))continue;
    const isDirect=v[j-1]==="."&&direct.has(v[j]);
    const isShared=v[j-1]==="::"&&["theme","components","button","ui_layout"].includes(v[j-2])&&shared.has(v[j]);
    if(!isDirect&&!isShared)continue;
    if(safe.some(([a,b])=>j>a&&j<b))continue;
    const guard=guards.filter(g=>j>=g.start&&j<g.end&&(j>g.body||v.slice(g.start,j).includes("&&")||v.slice(g.start,j).includes("||"))).at(-1);
    if(!guard)continue;
    const end=pairs.get(j+1);
    const call=norm(isDirect?j-2:j-2,end+1);
    // Every candidate is a review trigger, not proof of actual geometry
    // drift. Compare occurrence counts, not line numbers.
    hits.push({line:tokens[j].line,kind:isDirect?"direct-layout":"shared-action",signature:guard.head+" | "+call,call});
  }
  return hits;
}
function additions(before,after){
  const counts=new Map();for(const x of before)counts.set(x.signature,(counts.get(x.signature)||0)+1);
  return after.filter(x=>{const n=counts.get(x.signature)||0;if(n){counts.set(x.signature,n-1);return false;}return true;});
}

export { scan, additions };

function selfTest(){
  const cases=[
  [
    "if pending && crate::button::action(items_ui,\"상품 선택 취소\",ButtonRole::Quiet,true).clicked(){undo();}",
    1
  ],
  [
    "if let Some(error)=error { ui.label(error); ui.add_space(8.0); }",
    2
  ],
  [
    "components::fixed_row(ui,size,layout,|slot|{if busy{slot.label(\"busy\");}});",
    0
  ],
  [
    "if busy { components::fixed_row(ui,size,layout,|slot|{slot.label(\"busy\");}); }",
    1
  ],
  [
    "if let Some(error)=error { ui.label(r#\"{ }\"#); } // ui.label(\"comment\")",
    1
  ],
  [
    "#[cfg(test)] mod tests { fn t(){if busy{ui.label(\"test\");}} }",
    0
  ],
  [
    "if ready { ui.label(\"ready\"); } else { ui.label(\"wait\"); }",
    2
  ],
  [
    "if expanded { ui.label(\"details\"); }",
    0
  ],
  [
    "crate::button::action(ui,\"save\",ButtonRole::Standard,!busy);",
    0
  ]
];
  for(const [source,count] of cases)assert.equal(scan(source).length,count,source);
  const one=scan(cases[0][0]);
  assert.equal(additions(one,one).length,0);
  assert.equal(additions(one,[...one,...one]).length,1);
  assert.equal(additions([],one).length,1);
  assert.equal(additions(one,[]).length,0);
  const prior=scan('if pending && theme::action(ui,"save",role,enabled).clicked(){submit();}');
  const moved=scan('if pending && crate::button::action(ui,"save",role,enabled).clicked(){submit();}');
  assert.equal(additions(prior,moved).length,0);
  assert.equal(additions(prior,scan('if pending && crate::button::action(ui,"delete",role,enabled).clicked(){submit();}')).length,1);
  assert.equal(additions(prior,scan('if pending && retry && crate::button::action(ui,"save",role,enabled).clicked(){submit();}')).length,1);
  assert.equal(additions(prior,[...moved,...moved]).length,1);
  const oldDropdown=scan('if ready { ui.horizontal(|line|{components::management_dropdown_values(line,a,b,c,d);}); }');
  const newDropdown=scan('if ready { ui.horizontal(|line|{crate::dropdown::management_dropdown_values(line,a,b,c,d);}); }');
  assert.equal(additions(oldDropdown,newDropdown).length,0);
  assert.equal(additions(oldDropdown,scan('if ready { ui.horizontal(|line|{crate::dropdown::management_dropdown_values(line,a,b,c,e);}); }')).length,1);
  const oldHeader=scan('if rollback_pending { components::reception_item_header_action(ui,true,enabled); }');
  const newHeader=scan('if rollback_pending { crate::button::reception_item_header_action(ui,true,enabled); }');
  assert.equal(additions(oldHeader,newHeader).length,0);
  assert.equal(additions(oldHeader,scan('if rollback_pending { crate::button::reception_item_header_action(ui,false,enabled); }')).length,1);
  const oldSlot=scan('if busy { components::fixed_status_slot(ui,w,h,text,tip,color); }');
  const newSlot=scan('if busy { crate::ui_layout::fixed_status_slot(ui,w,h,text,tip,color); }');
  assert.equal(oldSlot.length,1);
  assert.equal(additions(oldSlot,newSlot).length,0);
  assert.equal(additions(oldSlot,scan('if busy { crate::ui_layout::fixed_status_slot(ui,w,h,other,tip,color); }')).length,1);
  console.log("EGUI_STATE_LAYOUT_SELFTEST_PASS",cases.length+15);
}

function main(){
  selfTest();
  if(process.argv.includes("--self-test"))return;
  const root=resolve(fileURLToPath(new URL("..",import.meta.url)));
  const dir=resolve(root,"egui-app/src");
  const walk=p=>readdirSync(p,{withFileTypes:true}).flatMap(e=>e.isDirectory()?walk(resolve(p,e.name)):e.name.endsWith(".rs")?[resolve(p,e.name)]:[]);
  const files=walk(dir);
  const baseIndex=process.argv.indexOf("--base");
  const base=baseIndex>=0?process.argv[baseIndex+1]:null;
  const git=(...args)=>execFileSync("git",args,{cwd:root,encoding:"utf8",maxBuffer:64*1024*1024,stdio:["ignore","pipe","pipe"]});
  if(base){assert.match(base,/^[0-9a-f]{40}$/i,"base must be a full commit SHA");git("cat-file","-e",base+"^{commit}");}
  const oldPaths=base?new Set(git("ls-tree","-r","--name-only",base,"--","egui-app/src").trim().split("\n")):new Set();
  let total=0;const introduced=[];
  for(const abs of files){
    const path=relative(root,abs).replaceAll("\\","/");
    const source=readFileSync(abs,"utf8");
    if(source.startsWith("#![cfg(test)]"))continue;
    const current=scan(source);total+=current.length;
    const before=base&&oldPaths.has(path)?scan(git("show",base+":"+path)):[];
    const fresh=base?additions(before,current):current;
    for(const hit of fresh)introduced.push({...hit,path});
  }
  for(const hit of introduced)console.log(hit.path+":"+hit.line+" "+hit.kind+" "+hit.signature.slice(0,260));
  console.log("EGUI_STATE_LAYOUT_AUDIT",JSON.stringify({files:files.length,candidates:total,newCandidates:introduced.length,base}));
  if(base&&introduced.length){console.error("New conditional layout allocations require a fixed unconditional slot, overlay, or explicit state-specific Rect review before merging.");process.exitCode=1;}
  if(process.argv.includes("--require-base")&&!base){console.error("Missing comparison base");process.exitCode=1;}
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))main();
