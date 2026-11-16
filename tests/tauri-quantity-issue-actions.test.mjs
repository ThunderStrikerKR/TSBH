import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const read = (path) => readFileSync(fileURLToPath(new URL("../" + path, import.meta.url)), "utf8");

test("missing quantity issue offers an editor and a gated one-item provisional action", () => {
  const component = read("src/StaffChecks.tsx");
  assert.match(component, /b\.kind === "QUANTITY_MISSING" \? \(/);
  assert.match(component, /disabled=\{unavailable\} onClick=\{\(\) => onSelect\(b\)\}>수량 수정/);
  assert.match(component, /1개로 잠정 처리/);
  assert.match(component, /onClick=\{\(\) => onApproveOne\?\.\(b\)\}/);
  assert.match(component, /disabled=\{unavailable \|\| approvingQuantity \|\| !onApproveOne \|\| !canApproveOne\?\.\(b\)\}/);
  assert.match(component, /b\.resolution === "PROVISIONALLY_APPROVED"/);
  assert.match(component, /onClick=\{\(\) => onCancelApproval\?\.\(b\)\}>잠정 승인 취소/);
});

test("one-item approval executes the production Rust issue reducer with actor and handles stale responses", () => {
  const desk = read("src/OrderDesk.tsx");
  const native = read("src/orders/native-rules.ts");
  assert.match(desk, /onApproveOne=\{\(b\) => \{/);
  assert.match(desk, /b\.canApproveOne !== true/);
  assert.match(desk, /canApproveOne=\{\(b\) => !exported && staffChecksActionsReady && b\.canApproveOne === true\}/);
  assert.doesNotMatch(desk, /item\.match\?\.kind === "QUANTITY"/);
  const rust = read("src-tauri/src/business_core/rules/issues.rs");
  assert.match(rust, /fn can_approve_missing_quantity\(item:&Value\)->bool/);
  assert.match(rust, /!can_approve_missing_quantity\(&order\["recipients"\]\[ri\]\["items"\]\[ii\]\)/);
  assert.match(desk, /nativeApproveMissingQuantity\(/);
  assert.match(desk, /if \(!isCurrentOrderInput\(draft, state\)\)/);
  assert.match(desk, /recEdit\(b\.recipientId, \(recipient\) => \{/);
  assert.match(desk, /target\.quantityApproval = source\.quantityApproval/);
  assert.match(native, /"rules", "apply_issue_action"/);
  assert.match(native, /action: "approve_missing_quantity"/);
  assert.match(native, /now: new Date\(\)\.toISOString\(\)/);
});

test("Rust and TypeScript require an actual provisional decision before clearing the quantity blocker", () => {
  const ts = read("src/validation.ts");
  const rs = read("src-tauri/src/business_core/rules/issues.rs");
  const compare = read("tests/rust-migration/rules/parity/compare.mts");
  assert.match(ts, /kind === "QUANTITY_MISSING"/);
  assert.match(ts, /i\?\.quantityApproval\?\.quantity === 1/);
  assert.match(ts, /"PROVISIONALLY_APPROVED"/);
  assert.match(rs, /kind=="QUANTITY_MISSING"&&item\.is_some_and/);
  assert.match(rs, /"PROVISIONALLY_APPROVED"/);
  assert.match(compare, /order issues after one-item provisional approval/);
  assert.match(compare, /order issues after provisional approval cancellation/);
  assert.match(compare, /stale 1-item approval cannot clear changed quantity issue/);
});
