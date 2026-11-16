import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const source = readFileSync(new URL("../BUILD_WINDOWS_TAURI.cmd", import.meta.url), "utf8");

test("Windows build checks both running EXE names before npm or cargo build", () => {
  const preflight = source.indexOf("call :ensure_bando_closed");
  const install = source.indexOf('scripts\\ensure-node-deps.ps1');
  const compile = source.indexOf("call npm run build:windows");
  assert.ok(preflight >= 0 && preflight < install && install < compile);
  assert.match(source, /tasklist \/FI "IMAGENAME eq bando_hub\.exe"/);
  assert.match(source, /tasklist \/FI "IMAGENAME eq bando_hub_tauri\.exe"/);
  assert.match(source, /:app_running\r?\n[\s\S]*BUILD NOT STARTED/);
  assert.doesNotMatch(source, /taskkill(?:\.exe)?\s/i);
});

test("Windows build stages new binary and rechecks before replacing published executable", () => {
  const stage = source.indexOf('"release\\bando_hub.exe.pending" >nul');
  const secondGuard = source.indexOf("call :ensure_bando_closed", source.indexOf("call npm run build:windows"));
  const rename = source.indexOf('move /Y "release\\bando_hub.exe.pending" "release\\bando_hub.exe"');
  const cleanup = source.indexOf('del /Q "release\\BANDO-HUB-v*.exe"');
  assert.ok(stage > 0 && secondGuard > stage && rename > secondGuard);
  assert.ok(cleanup > rename, "Old versions can only be deleted after a successful rename");
  assert.match(source, /if errorlevel 1 goto copy_failed/);
  assert.match(source, /:app_running_after_build[\s\S]*Existing release\\bando_hub\.exe was not changed/);
  assert.match(source, /:copy_failed[\s\S]*Existing release\\bando_hub\.exe was not changed/);
});

test("daily development is a real Tauri/Vite TEST workspace without release EXE or ZIP build", () => {
  const dev = readFileSync(new URL("../DEV_WINDOWS_TAURI.cmd", import.meta.url), "utf8");
  assert.match(dev, /tauri\.cmd" dev -- -- --bando-developer-test/);
  assert.match(dev, /ensure-node-deps\.ps1/);
  assert.doesNotMatch(dev, /npm run build:windows|Compress-Archive|package-ocr-runtime\.ps1/);
  assert.match(dev, /mklink \/J .*release\\ocr/);
});

test("only explicit release command builds full OCR/portable ZIP; EXE-only option skips packaging", () => {
  const exe = readFileSync(new URL("../BUILD_WINDOWS_EXE.cmd", import.meta.url), "utf8");
  const portable = readFileSync(new URL("../PACKAGE_WINDOWS_TAURI.cmd", import.meta.url), "utf8");
  assert.match(exe, /BUILD_WINDOWS_TAURI\.cmd" --exe-only/);
  assert.match(portable, /BUILD_WINDOWS_TAURI\.cmd"/);
  assert.ok(source.indexOf('if /I "%~1"=="--exe-only" goto exe_only') <
    source.indexOf("ensure-ocr-package.ps1"));
  assert.match(source, /:exe_only[\s\S]*EXE ONLY: release\\bando_hub\.exe/);
});

test("npm dependencies are installed once per lockfile or Node toolchain change", () => {
  const ensure = readFileSync(new URL("../scripts/ensure-node-deps.ps1", import.meta.url), "utf8");
  assert.match(ensure, /Get-FileHash.*\$lock/);
  assert.match(ensure, /Get-FileHash.*\$manifest/);
  assert.match(ensure, /\.bando-deps-fingerprint\.txt/);
  assert.match(ensure, /Skipping npm installation/);
  assert.match(ensure, /npm\.cmd ci --no-audit --no-fund/);
  assert.doesNotMatch(source, /call npm install --no-audit/);
});

test("expensive Windows release GUI/ZIP checks require explicit GitHub workflow dispatch", () => {
  const output = readFileSync(new URL("../.github/workflows/migration-output.yml", import.meta.url), "utf8");
  const verify = readFileSync(new URL("../.github/workflows/verify-tauri.yml", import.meta.url), "utf8");
  assert.match(output, /windows-p1p2-real-exe:[\s\S]*?if: github\.event_name == 'workflow_dispatch'/);
  assert.match(verify, /windows-release-build:[\s\S]*?if: github\.event_name == 'workflow_dispatch'/);
  assert.match(verify, /Windows SQLite persistence and Rust tests/);
});
