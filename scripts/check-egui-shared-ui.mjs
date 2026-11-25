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
// Table geometry and painting belong to the shared layer.
// Exceptions identify an exact call and occurrence limit, never a whole function.
const tableScreens = new Set([...headerScreens,"client_manager.rs",
  "client_chooser.rs","product_excel_ui.rs"]);
const retiredTable = new Set(["ManagementRowRule","management_row_striped",
  "management_row_impl","management_readonly_table_aligned",
  "paint_management_table_column_rule","paint_column_dividers",
  "manual_table_cell","manual_table_heading","manual_linked_table_editor"]);
const tablePaintMethods = new Set(["rect_filled","rect_stroke","rect",
  "line_segment","hline","vline"]);
const paintExceptions = [
  // Selected assignment row and selection stripe, not ordinary zebra styling.
  ["freight_manager.rs","show_groups",1,
    "rect_filled(row_rect,0.0,background)"],
  ["freight_manager.rs","show_groups",1,
    "rect_filled(egui::Rect::from_min_size(row_rect.min,egui::vec2(theme::WORKFLOW_SELECTED_STRIPE,row_rect.height())),0.0,theme::SELECTION)"],
  // History branch glyphs and date navigation, not table dividers.
  ["order_history_manager.rs","paint_history_branch",1,
    "line_segment([egui::pos2(x,center),egui::pos2(x,row_rect.bottom())],stroke)"],
  ["order_history_manager.rs","paint_history_branch",1,
    "line_segment([egui::pos2(x,row_rect.top()),egui::pos2(x,row_rect.bottom())],stroke)"],
  ["order_history_manager.rs","paint_history_branch",2,
    "line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke)"],
  ["order_history_manager.rs","paint_history_branch",1,
    "line_segment([egui::pos2(x,row_rect.top()),egui::pos2(x,center)],stroke)"],
  ["order_history_manager.rs","range_date_picker_rtl",1,
    "line_segment([egui::pos2(x,inner.top()+4.0),egui::pos2(x,inner.bottom()-4.0)],Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::DIVIDER))"],
  // Expanded shipment card and chooser selection outline.
  ["order_history_manager.rs","show_page",1,
    "rect_filled(row_rect,0.0,theme::CANVAS)"],
  ["order_history_manager.rs","show_page",1,
    "paint_bordered_surface(row_ui.painter(),block_rect,theme::ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS,theme::PANEL,theme::DIVIDER)"],
  ["client_chooser.rs","show",1,
    "rect_stroke(response.rect.shrink(1.0),0.0,egui::Stroke::new(theme::SOFT_DIVIDER_WIDTH,theme::SELECTION),egui::StrokeKind::Inside)"],
];
const normalized = source=>tokenizeRust(source).map(t=>t.kind==="literal"?"<literal>":t.v).join(" ");
const paintAllowance = new Map(paintExceptions.map(([file,fn,count,call])=>
  [file+":"+fn+":"+normalized(call),count]));
const masterContract = {
  management_table_cells:2,management_table_column_rules:2,
  paint_management_table_row_background:1,paint_management_table_row_rule:1,
  management_master_header_begin:1,management_master_header_finish:1,
};
const tableContracts = {
  "product_manager.rs:show_page":masterContract,
  "freight_manager.rs:show_groups":{...masterContract,
    paint_management_table_row_background:0,management_table_row_fill:1},
  "freight_manager.rs:show_rates":masterContract,
  "freight_manager.rs:freight_history_table":masterContract,
  "excel_manager.rs:table":masterContract,
  "daily_settlement_manager.rs:report_table":masterContract,
  "order_history_manager.rs:show_page":masterContract,
  "order_history_manager.rs:draw_line":{management_table_cells:1},
  "order_history_manager.rs:history_detail_cells":{management_table_cells:1},
  "client_manager.rs:show_page":{management_row:1,management_client_header:1},
  "client_chooser.rs:show":{management_row:1,management_client_header:1},
  "product_manager.rs:show_detail":{management_readonly_table:1},
  "product_excel_ui.rs:show_review":{management_readonly_table:1},
};
const tableMarkers = new Set(["management_table_cells","management_table_column_rules",
  "management_master_header_begin","management_row","management_readonly_table"]);

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

export function audit(source,file,checkTableContracts=false){
  const tokens=productionTokens(source);
  const v=tokens.map(t=>t.v);
  const hits=[],used=new Map(),paintUsed=new Map(),sharedCalls=new Map();
  const is=(i,pattern)=>v.slice(i,i+pattern.split(" ").length).join(" ")===pattern;
  const flag=(i,rule)=>hits.push({line:tokens[i].line,rule});
  const base=file==="components.rs"||file==="theme.rs";
  const managed=!base&&(tableScreens.has(file)||v.some((x,i)=>
    x==="components"&&v[i+1]==="::"&&tableMarkers.has(v[i+2])));
  // Match the complete argument list so nested Rect/Stroke expressions
  // cannot turn a specific exception into a general painter exemption.
  const callEnd=open=>{
    let depth=0;
    for(let j=open;j<v.length;j++){
      if(v[j]==="(")depth++;
      if(v[j]===")"&&--depth===0)return j;
    }
    return v.length-1;
  };
  let fn="";
  for(let i=0;i<v.length;i++){
    if(v[i]==="fn"&&/^[A-Za-z_]\w*$/.test(v[i+1]??""))fn=v[i+1];
    for(const pattern of retired)if(is(i,pattern))flag(i,"retired UI entry: "+pattern);
    if(retiredTable.has(v[i]))flag(i,"retired table entry: "+v[i]);
    if(is(i,"components ::")&&v[i+3]==="("){
      const key=file+":"+fn+":"+v[i+2];
      sharedCalls.set(key,(sharedCalls.get(key)||0)+1);
    }
    if(managed){
      if(is(i,"theme :: TABLE_ROW_ALT_BG"))
        flag(i,"local table stripe color; use shared row fill");
      const localPaint=v[i]==="."&&tablePaintMethods.has(v[i+1])&&v[i+2]==="(";
      const genericPaint=is(i,"theme ::")&&
        ["paint_bordered_surface","paint_surface_outline","paint_soft_rule"].includes(v[i+2])&&v[i+3]==="(";
      if(localPaint||genericPaint){
        const start=localPaint?i+1:i+2;
        const fingerprint=v.slice(start,callEnd(start+1)+1).join(" ");
        const key=file+":"+fn+":"+fingerprint;
        const count=(paintUsed.get(key)||0)+1;paintUsed.set(key,count);
        if(count>(paintAllowance.get(key)||0))
          flag(i,"raw table painter: "+v[start]+"; use shared table rendering");
      }
      if(is(i,"from_fn (")){
        const body=v.slice(i+2,callEnd(i+1)).join(" ");
        if(/Rect :: (?:from_min_size|from_min_max|from_center_size|from_two_pos)/.test(body))
          flag(i,"local table cell geometry; use management_table_cells");
      }
    }
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
  if(checkTableContracts){
    for(const [scope,required] of Object.entries(tableContracts)){
      if(!scope.startsWith(file+":"))continue;
      for(const [entry,min] of Object.entries(required)){
        const count=sharedCalls.get(scope+":"+entry)||0;
        if(count<min)hits.push({line:1,rule:"missing shared table contract: "+scope+
          " requires "+entry+" ("+count+"/"+min+")"});
      }
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
    ["excel_manager.rs","fn render(){painter.rect_filled(r,0.0,theme::TABLE_HEADER_OUTLINE);}",2],
    ["new_screen.rs","fn render(){components::table_header(ui,x);}",1],
    ["product_manager.rs","fn render(){painter.line_segment([r.left_bottom(),r.right_bottom()],stroke);}",1],
    ["product_manager.rs","fn render(){theme::paint_bordered_surface(p,r,0,c,b);}",1],
    ["product_manager.rs","fn render(){theme::paint_soft_rule(p,r);}",1],
    ["order_history_manager.rs","fn show_page(){theme::paint_bordered_surface(row_ui.painter(),block_rect,theme::ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS,theme::PANEL,theme::DIVIDER);}",0],
    ["excel_manager.rs","fn render(){painter.rect_filled(r,0.0,theme::DIVIDER);}",1],
    ["product_manager.rs","fn render(){let c=theme::TABLE_ROW_ALT_BG;}",1],
    ["product_manager.rs","fn render(){let c=std::array::from_fn(|i|Rect::from_min_size(p,w));}",1],
    ["client_manager.rs","fn render(){let c=std::array::from_fn(|i|i.to_string());}",0],
    ["new_screen.rs","fn render(){components::management_table_cells(p,w,h,g);painter.rect_filled(r,0.0,ink);}",1],
    ["new_screen.rs","fn render(){components::management_table_cells(p,w,h,g);components::management_table_column_rules(ui,r,c,g);}",0],
    ["new_screen.rs","fn render(){components::management_row_striped(ui,id,c,t,i);}",1],
    ["order_history_manager.rs","fn paint_history_branch(){painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);}",0],
    ["order_history_manager.rs","fn paint_history_branch(){painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);painter.line_segment([egui::pos2(x,center),egui::pos2(tip,center)],stroke);}",1],
    ["order_history_manager.rs","fn paint_history_branch(){painter.line_segment([a,b],stroke);}",1],
    ["freight_manager.rs","fn show_groups(){painter.rect_filled(row_rect,0.0,background);}",0],
    ["freight_manager.rs","fn other(){painter.rect_filled(row_rect,0.0,background);}",1],
    ["product_manager.rs","#[cfg(test)] fn t(){painter.rect_filled(r,0.0,c);} fn render(){components::management_table_cells(p,w,h,g);}",0],
  ];
  for(const [file,source,count] of cases)assert.equal(audit(source,file).length,count,source);
  assert.equal(audit("fn show_page(){}","product_manager.rs",true).length,7);
  assert.equal(audit("fn show(){components::management_row(ui,id,c,t,i);components::management_client_header(ui,h,c,l,w,s);}","client_chooser.rs",true).length,0);
  console.log("EGUI_SHARED_UI_SELFTEST_PASS",cases.length+2);
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
    for(const hit of audit(readFileSync(path,"utf8"),file,true)){
      failures++;console.error(file+":"+hit.line+" "+hit.rule);
    }
  }
  console.log("EGUI_SHARED_UI_AUDIT",JSON.stringify({files:files.length,failures}));
  if(failures)process.exitCode=1;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))main();
