import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const source = read("src-tauri/src/lib.rs");
const cargo = read("src-tauri/Cargo.toml");
const config = JSON.parse(read("src-tauri/tauri.conf.json"));

test("native window state plugin restores main window bounds and maximization", () => {
  assert.match(cargo, /^tauri-plugin-window-state\s*=\s*"2"/m);
  assert.match(source, /tauri_plugin_window_state::Builder::default\(\)/);
  assert.match(source, /\.with_filter\(\|label\| label == "main"\)/);
  assert.match(source, /bando-maximize-after-window-state/);
  assert.match(source, /on_window_ready\(move \|window\|/);
  assert.match(source, /app_config_dir\(\)/);
  assert.match(source, /saved\.get\("main"\)\?/);
  assert.match(source, /\.get\("maximized"\)\?\.as_bool\(\)/);
  assert.match(source, /run_on_main_thread\(move \|\|/);
  assert.match(source, /std::thread::spawn\(move \|\|/);
  assert.match(source, /\[\(1, 650u64\), \(2, 1750u64\)\]/);
  assert.match(source, /SetWindowPlacement\(hwnd, &placement\)/);
  assert.match(source, /std::thread::sleep\(std::time::Duration::from_millis\(delay_ms\)\)/);
  assert.doesNotMatch(source, /ready_window\.maximize\(\)/);
  // A frameless WebView can occupy a maximized rectangle without the
  // native SW_MAXIMIZE status. The real HWND must pass IsZoomed on restart.
  assert.match(source, /mod native_maximized_restore/);
  assert.match(source, /IsZoomed\(hwnd\)/);
  assert.match(source, /ShowWindow\(hwnd, 3\)/);
  assert.match(source, /ensure_native_maximized\(&candidate\)/);
  assert.doesNotMatch(source, /window\.restore_state\(StateFlags::MAXIMIZED\)/);
  for (const flag of ["SIZE", "POSITION", "MAXIMIZED", "VISIBLE"]) {
    assert.match(source, new RegExp("StateFlags::" + flag));
  }
  assert.match(source, /\.build\(\),\s*\)\s*\.setup\(/);
  assert.equal(config.app.windows.find(window => window.label === "main")?.visible, false,
    "main window must not flash in its default placement before restoration");
});

test("window placement is independent of SQLite order data and KakaoTalk docking", () => {
  assert.doesNotMatch(source, /"settings"\]\["windowGeometry"/);
  assert.match(source, /\.with_filter\(\|label\| label == "main"\)/);
  assert.match(source, /kakao_dock::restore_on_exit/);
});
