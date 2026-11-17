import test from "node:test";
import assert from "node:assert/strict";
import { selectWindowsScope, stripWindowSections } from "../scripts/classify-dev-windows-smoke.mjs";
const original = [
  "mod example;",
  "// The window-state plugin can restore the real HWND",
  "mod native_maximized_restore { fn restore() {} }",
  "struct Database { value: u32 }",
  "fn run() {",
  "// Restore only the BANDO HUB main window.",
  "register_window_plugin();",
  ".setup(move |app| {",
  "register_business_ipc();",
  "}",
].join("\n");

test("native window-only changes route to focused Windows test", () => {
  assert.equal(selectWindowsScope(["src-tauri/src/lib.rs"], original,
    original.replace("register_window_plugin();", "register_window_plugin_v2();")), "window");
  assert.equal(selectWindowsScope(["src-tauri/src/lib.rs"], original,
    original.replace("fn restore() {}", "fn restore() { verify_hwnd(); }")), "window");
});
test("business code and mixed source changes require full suite", () => {
  assert.equal(selectWindowsScope(["src-tauri/src/lib.rs"], original,
    original.replace("register_business_ipc();", "register_changed_business_ipc();")), "full");
  assert.equal(selectWindowsScope(["src-tauri/src/lib.rs", "src/OrderDesk.tsx"], original, original), "full");
});
test("missing source anchors fail closed", () => {
  assert.equal(stripWindowSections("no window code"), null);
  assert.equal(selectWindowsScope(["src-tauri/src/lib.rs"], original,
    original.replace(".setup(move |app| {", ".setup(move |application| {")), "full");
});
test("test-only assertions avoid Windows build", () => {
  assert.equal(selectWindowsScope(["tests/tauri-window-placement.test.mjs"], null, null), "fast");
  assert.equal(selectWindowsScope(["tests/ci-windows-smoke-routing.test.mjs", "tests/dev-window-placement-real-restart.test.mjs"], null, null), "fast");
});
test("Win32 probe still gets real EXE restart", () => {
  assert.equal(selectWindowsScope(["scripts/verify-dev-window-placement-win.ps1"], null, null), "window");
});
test("unknown, structural and empty changes require full", () => {
  assert.equal(selectWindowsScope([], null, null), "full");
  assert.equal(selectWindowsScope(["src-tauri/src/business_core/rules/pricing.rs"], null, null), "full");
  assert.equal(selectWindowsScope([".github/workflows/dev-windows-smoke.yml"], null, null), "full");
});

test("single factory CI probe routes to isolated real Windows factory scenario", () => {
  assert.equal(selectWindowsScope(["scripts/migration32-real-factory-notifications.mjs"],null,null),"factory");
  assert.equal(selectWindowsScope(["scripts/migration32-real-factory-notifications.mjs","src/OrderDesk.tsx"],null,null),"full");
});

test("one domain-owned Windows probe change runs only that real GUI domain", () => {
  for (const domain of ["orders","pricing","shipping","clients","factory"]) {
    const file="scripts/ci-domains/"+domain+".mjs";
    assert.equal(selectWindowsScope([file],null,null),domain);
    assert.equal(selectWindowsScope([file,"tests/ci-dev-domain-gui-contract.test.mjs"],null,null),domain);
    assert.equal(selectWindowsScope([file,"src/OrderDesk.tsx"],null,null),"full");
  }
  assert.equal(selectWindowsScope(["scripts/ci-domains/orders.mjs","scripts/ci-domains/pricing.mjs"],null,null),"full");
  assert.equal(selectWindowsScope(["scripts/ci-dev-domain-gui.mjs"],null,null),"full");
});
