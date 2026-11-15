import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const source=(p)=>readFileSync(new URL("../"+p,import.meta.url),"utf8");

test("live React reception merges Rust recognition results only through compiled Rust",()=>{
  const hook=source("src/orders/useOrderDesk.tsx");
  const adapter=source("src/orders/rust-reception.ts");
  const rust=source("src-tauri/src/business_core/rules/reception_merge.rs");
  const routing=source("src-tauri/src/business_core/rules/mod.rs");
  assert.match(hook,/"rules", "merge_reception_analysis"/);
  assert.match(hook,/assertRustReceptionAnalysis\(result, source\)/);
  assert.doesNotMatch(hook,/\bapplyRustReceptionAnalysis\(/);
  assert.doesNotMatch(adapter,/\bapplyRustReceptionAnalysis\(/);
  assert.match(routing,/"merge_reception_analysis" => reception_merge::execute/);
  for(const symbol of ["match_recipient","merge_item","analysisSnapshot","confirmedFields","analysisEvidence"]) {
    assert.ok(rust.includes(symbol),symbol);
  }
  assert.match(source("tests/rust-migration/recognition/react-reception.test.mjs"),
    /ts-reception-merge-oracle\.ts/);
});
test("manual source and quantity mutations are Rust-owned",()=>{
  const ui=source("src/orders/ManualClassification.tsx");
  const rust=source("src-tauri/src/business_core/rules/manual_structure.rs");
  const router=source("src-tauri/src/business_core/rules/mod.rs");
  const actions=["resize_manual_rows","add_manual_item","remove_manual_item",
    "confirm_manual_quantity","set_manual_quantity_draft",
    "step_manual_quantity","reset_manual_rows"];
  for(const action of actions){
    assert.ok(ui.includes('"'+action+'"'),"live UI must send "+action);
    assert.ok(rust.includes('"'+action+'"'),"Rust must own "+action);
    assert.ok(router.includes('"'+action+'"'),"native dispatch must route "+action);
  }
  assert.doesNotMatch(ui,/\bnewManualRecipient\(/);
  assert.doesNotMatch(ui,/\bemptyManualItem\(/);
  assert.doesNotMatch(ui,/\.quantity\s*=\s*quantity;/);
  assert.doesNotMatch(ui,/action: "parse_manual_quantity"/);
  assert.match(ui,/latestInputs\.current\.model !== previousModel/);
  assert.match(ui,/nativeDraftLock\.current = true/);
});
