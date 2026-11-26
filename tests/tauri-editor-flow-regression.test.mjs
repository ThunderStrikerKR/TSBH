import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = path => readFileSync(new URL("../"+path, import.meta.url), "utf8");

test("Escape cancels only the current native price draft before blur can commit", () => {
  const manager = read("egui-app/src/product_manager.rs");
  const escape = manager.indexOf("if escape{");
  const keyboardCommit = manager.indexOf("else if keyboard_commit{", escape);
  const blurCommit = manager.indexOf("else if response.lost_focus(){", keyboardCommit);
  assert.ok(escape >= 0 && keyboardCommit > escape && blurCommit > keyboardCommit);
  const escapeBranch = manager.slice(escape, keyboardCommit);
  assert.match(escapeBranch, /entry\.reset_to_original\(\)/);
  assert.match(escapeBranch, /response\.surrender_focus\(\)/);
  assert.match(manager, /if entry\.dirty\(\)&&!cell_busy/);
});

test("native egui Escape cancels the topmost draft before closing the workspace", () => {
  const workspace = read("egui-app/src/manual_workspace.rs");
  const editor = read("egui-app/src/item_editor.rs");
  assert.match(workspace, /enum EscapeAction/);
  assert.match(workspace, /EscapeAction::CancelApply/);
  assert.match(workspace, /EscapeAction::CancelDiscard/);
  assert.match(workspace, /EscapeAction::RevertCell/);
  assert.match(workspace, /fn escape_action/);
  assert.match(editor, /pub fn escape\(&mut self,busy:bool\)/);
});
