#!/usr/bin/env node
import { readdirSync, readFileSync } from "node:fs";
import { resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import { tokenizeRust } from "./check-egui-state-layout.mjs";

// Reviewed compact record/menu/footer dividers; not structural section lines.
// Fingerprints are scoped to file + function + UI argument, not line number.
// A new use needs an explicit review; removing a use does not require approval.
const compact = new Map(Object.entries({
  "main.rs:show_workflow_dialog:list":1,
  "settings_view.rs:show_account_rows:ui":1,
  "settings_view.rs:show_data:panel":1,
  "alias_manager.rs:show_page:list":1,
  "alias_manager.rs:show_dialogs:results":1,
  "brand_manager.rs:show_page:list":1,
  "model_group_manager.rs:show_page:results":1,
  "model_group_manager.rs:show_page:registered":1,
  "model_group_manager.rs:show_page:right":2,
  "automatic_message_manager.rs:show:body":1,
  "order_history_detail.rs:payment_panel:ui":1,
  "freight_manager.rs:show_history:menu":1,
  "excel_manager.rs:show_reply_review:ui":1,
  "workflow_board_ui.rs:group_frame:ui":1,
  "workflow_board_ui.rs:pending_frame:ui":1,
  "manual_product_search.rs:show:ui":1,
  "category_dropdown.rs:menu_body:menu":2
}));
const scrollConstructors = ["vertical","horizontal","both","new","neither"];
const headerScreens = new Set(["product_manager.rs","excel_manager.rs",
  "daily_settlement_manager.rs","order_history_manager.rs","freight_manager.rs"]);
const controls = [
  "Button :: new (","Frame :: new (","TextEdit :: singleline (",
  "TextEdit :: multiline (","Checkbox :: new (","ComboBox ::",
  "Window :: new (",". button (",". checkbox (",
  ". text_edit_singleline (",". text_edit_multiline (",
  "ScrollStyle :: solid (","ScrollStyle :: floating ("
];
const retired = [
  "components :: table_header (","components :: table_row (",
  "components :: record_card (","components :: summary_strip (",
  "modal_master_spaced_divider ("
];

// Exclude test modules/functions, not production code after a test module.
// Inline cfg(test) probes remain visible; they cannot hide subsequent code.
function productionTokens(source){
  const tokens=tokenizeRust(source);
  const v=tokens.map(t=>t.kind==="literal"?"<literal>":t.v);
  const out=[];
  for(let i=0;i<tokens.length;i++){
    if(v.slice(i,i+7).join(" ")==="# [ cfg ( test ) ]" &&
       ["mod","fn"].includes(v[i+7])){
      let k=i+8;
      while(k<v.length && v[k]!=="{" && v[k]!==";")k++;
      if(v[k]==="{"){
        let depth=1;k++;
        while(k<v.length&&depth){if(v[k]==="{")depth++;if(v[k]==="}")depth--;k++;}
        if(depth)throw Error("Unclosed cfg(test) item");
        i=k-1;continue;
      }
    }
    out.push({...tokens[i],v:v[i]});
  }
  return out;
}

export function audit(source,file){
  const tokens=productionTokens(source);
  const v=tokens.map(t=>t.v);
  const hits=[],used=new Map();
  const is=(i,pattern)=>v.slice(i,i+pattern.split(" ").length).join(" ")===pattern;
  const flag=(i,rule)=>hits.push({line:tokens[i].line,rule});
  const base=file==="components.rs"||file==="theme.rs";
  let fn="";
  for(let i=0;i<v.length;i++){
    if(v[i]==="fn"&&/^[A-Za-z_]\w*$/.test(v[i+1]??""))fn=v[i+1];
    for(const pattern of retired)if(is(i,pattern))flag(i,"retired UI entry: "+pattern);
    if(!base){
      for(const pattern of controls)if(is(i,pattern))flag(i,"raw shared control: "+pattern);
      for(const pattern of ["theme :: soft_rule (","theme :: divider (",". separator (",
        ". on_hover_text (",". on_hover_text_at_pointer ("])
        if(is(i,pattern))flag(i,"raw divider/tooltip: "+pattern);
      if(is(i,"components :: soft_divider (")){
        const key=file+":"+fn+":"+v[i+4];
        const count=(used.get(key)||0)+1;used.set(key,count);
        if(count>(compact.get(key)||0))flag(i,"unreviewed compact divider: "+key);
      }
      if(headerScreens.has(file)&&is(i,"theme :: TABLE_HEADER_OUTLINE"))
        flag(i,"local managed header outline");
    }
    if(scrollConstructors.some(name=>is(i,"ScrollArea :: "+name+" (")) &&
       !(file==="components.rs"&&fn==="scroll_area"))
      flag(i,"raw scroll constructor outside shared scroll_area");
    if(file==="components.rs"){
      if(is(i,"TextEdit :: singleline (")&&fn!=="singleline_editor")
        flag(i,"single-line editor outside shared constructor");
      if((is(i,". on_hover_text (")||is(i,". on_hover_text_at_pointer ("))&&fn!=="overflow_tooltip")
        flag(i,"tooltip outside shared overflow policy");
    }
  }
  return hits;
}

function selfTest(){
  const cases=[
    ["product_manager.rs","fn render(){egui::ScrollArea :: both ( );}",1],
    ["main.rs","fn render(){egui::ScrollArea::both();}",1],
    ["alias_manager.rs","fn show_page(){egui::ScrollArea::vertical();}",1],
    ["new_screen.rs","fn render(){ScrollArea::horizontal();}",1],
    ["theme.rs","fn wrong(){ScrollArea::new([true,true]);}",1],
    ["new_screen.rs","fn render(){ScrollArea::neither();}",1],
    ["components.rs","fn wrong(){egui::ScrollArea::vertical();}",1],
    ["components.rs","fn scroll_area(){egui::ScrollArea::vertical();egui::ScrollArea::horizontal();egui::ScrollArea::both();}",0],
    ["new_screen.rs","fn render(){components::scroll_area(components::ScrollAxes::Vertical);}",0],
    ["alias_manager.rs","fn render(){components::management_register_list_scroll_area(ui,height);}",0],
    ["main.rs","#[cfg(test)] mod tests {fn t(){egui::ScrollArea::vertical();}} fn render(){egui::ScrollArea::horizontal();}",1],
    ["new_screen.rs","fn render(){egui::Button :: new ( \"x\" );}",1],
    ["new_screen.rs","fn render(){ui.on_hover_text(\"x\");}",1],
    ["new_screen.rs","fn render(){components::section_divider(ui);}",0],
    ["new_screen.rs","fn render(){components::soft_divider(ui);}",1],
    ["settings_view.rs","fn show_account_rows(){components::soft_divider(ui);}",0],
    ["settings_view.rs","fn show_account_rows(){components::soft_divider(ui);components::soft_divider(ui);}",1],
    ["settings_view.rs","fn other(){components::soft_divider(ui);}",1],
    ["components.rs","fn wrong(){egui::TextEdit::singleline(value);}",1],
    ["components.rs","fn singleline_editor(){egui::TextEdit::singleline(value);}",0],
    ["components.rs","fn overflow_tooltip(){response.on_hover_text(full);}",0],
    ["components.rs","fn wrong(){response.on_hover_text(full);}",1],
    ["new_screen.rs","// egui::Button::new(\nfn a(){let x=r#\"ui.on_hover_text(x)\"#;}",0],
    ["new_screen.rs","#[cfg(test)] mod tests {fn a(){egui::Button::new(\"x\");}} fn render(){ui.checkbox(x);}",1],
    ["new_screen.rs","fn a(){#[cfg(test)] record();ui.checkbox(x);}",1],
    ["excel_manager.rs","fn render(){painter.rect_filled(r,0.0,theme::TABLE_HEADER_OUTLINE);}",1],
    ["new_screen.rs","fn render(){components::table_header(ui,x);}",1],
  ];
  for(const [file,source,count] of cases)assert.equal(audit(source,file).length,count,source);
  console.log("EGUI_SHARED_UI_SELFTEST_PASS",cases.length);
}

function main(){
  selfTest();
  if(process.argv.includes("--self-test"))return;
  const root=resolve(fileURLToPath(new URL("..",import.meta.url)));
  const walk=p=>readdirSync(p,{withFileTypes:true}).flatMap(e=>
    e.isDirectory()?walk(resolve(p,e.name)):e.name.endsWith(".rs")?[resolve(p,e.name)]:[]);
  const files=walk(resolve(root,"egui-app/src"));
  let failures=0;
  for(const path of files){
    const file=relative(resolve(root,"egui-app/src"),path).replaceAll("\\","/");
    for(const hit of audit(readFileSync(path,"utf8"),file)){
      failures++;console.error(file+":"+hit.line+" "+hit.rule);
    }
  }
  console.log("EGUI_SHARED_UI_AUDIT",JSON.stringify({files:files.length,failures}));
  if(failures)process.exitCode=1;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))main();
