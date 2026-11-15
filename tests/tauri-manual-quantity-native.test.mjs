import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");

test("manual grid parses edited quantity using native Rust, never TS legacy parser", () => {
  const source = read("src/orders/ManualClassification.tsx");
  const part = source.slice(source.indexOf('          if (cell.field === "quantity") {'),
    source.indexOf('          attempt(() => setModel(setManualValue(',source.indexOf('          if (cell.field === "quantity") {')));
  assert.match(part, /"rules", "apply_manual_classification"/);
  assert.match(part, /action: "set_manual_quantity_draft", model, cell, value/);
  assert.doesNotMatch(part, /setManualValue\(/);
  assert.match(part, /manualQuantityRequest\.current\[key\] !== request/);
  assert.match(source,/manualQuantityPending\.current\.has\(cellKey\(cell\)\)/);
  const incrementStart = source.indexOf("    const increment = (delta: number) => {");
  const stepper = source.slice(incrementStart, source.indexOf("    return (", incrementStart));
  assert.match(stepper, /action: "step_manual_quantity", model, cell, delta/);
  assert.doesNotMatch(stepper, /setManualValue\(/);
});
test("manual draft quantity edits and steppers use Rust result fields, not JS calculations", () => {
  const source = read("src/orders/ManualClassification.tsx");
  const core = read("src-tauri/src/business_core/rules/manual_structure.rs");
  const router = read("src-tauri/src/business_core/rules/mod.rs");
  assert.match(source,/action: "set_manual_quantity_draft", model, cell, value/);
  assert.match(source,/action: "step_manual_quantity", model, cell, delta/);
  assert.match(source,/setModel\(\(current\) => applyNativeQuantity\(current, updated, cell\)\)/);
  assert.match(core,/"set_manual_quantity_draft" => \{/);
  assert.match(core,/"step_manual_quantity" => \{/);
  assert.match(router,/"set_manual_quantity_draft"/);
  assert.match(router,/"step_manual_quantity"/);
});

test("manual quantity Rust implementation has executable original TS differential cases", () => {
  const core = read("src-tauri/src/business_core/rules/manual_classification.rs");
  const parity = read("tests/rust-migration/rules/parity/compare.mts");
  assert.match(core, /fn quantity_from_input\(input: &str\) -> Value/);
  assert.match(core, /action == "parse_manual_quantity"/);
  assert.match(parity, /add\("manual input quantity: "/);
  assert.match(parity, /setManualValue\(manualModel,cell,value,baseState as any,manualOrder as any\)/);
});

test("manual identity/contact/note text editing is native-owned with TS parity", () => {
  const source = read("src/orders/ManualClassification.tsx");
  const core = read("src-tauri/src/business_core/rules/manual_classification.rs");
  const parity = read("tests/rust-migration/rules/parity/compare.mts");
  assert.match(source, /action: "set_manual_recipient_field", model, cell, value/);
  assert.match(source, /manualTextRequest\.current\[key\] !== request/);
  assert.match(core, /action == "set_manual_recipient_field"/);
  assert.match(core, /fn manual_address_parts\(value: &str\) -> Value/);
  assert.match(source, /row\.addressParts = changed\.addressParts/);
  assert.match(parity, /"manual address parts: "/);
  assert.match(parity, /"manual recipient: "\+field/);
  assert.match(parity, /setManualValue\(manualModel,cell,value,baseState as any,manualOrder as any\)/);
});
