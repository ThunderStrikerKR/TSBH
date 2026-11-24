import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const manual = read("egui-app/src/manual_workspace.rs");
const service = read("egui-app/src/reception_service.rs");
const board = read("egui-app/src/workflow_board.rs");

test("native freight workspace keeps the current draft while quote projections refresh", () => {
  assert.match(manual, /remote_rows:Vec<RemoteRow>/);
  assert.match(manual, /view_quote:Option<Value>/);
  assert.match(manual, /self\.remote_rows=remote_rows/);
  assert.match(manual, /self\.view_quote=view_quote/);
  assert.match(manual, /self\.dirty=true/);
  assert.match(service, /remote_reference_matches/);
});

test("native freight editing retains local state and delegates remote calculations to Rust", () => {
  assert.match(manual, /shippingCharge/);
  assert.match(manual, /manual_navigation_key_before_editor/);
  assert.match(manual, /EscapeAction::RevertCell/);
  assert.match(service, /remote_reference_matches/);
  assert.match(board, /fn native_remote_preview_is_not_replaced_by_optional_manual_checkbox/);
});
