import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read=(p)=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
const ci=read(".github/workflows/verify-tauri.yml");
const dev=read("DEV_WINDOWS_TAURI.cmd");
const helper=read("scripts/ensure-ocr-package.ps1");
test("GitHub Windows jobs cache Rust compilation and reuse immutable OCR bundle",()=>{
  assert.ok((ci.match(/Swatinem\/rust-cache@v2/g)||[]).length>=2);
  assert.match(ci,/uses: actions\/cache@v4/);
  assert.match(ci,/path: ocr-build-cache/);
  assert.match(ci,/hashFiles\('scripts\/package-ocr-runtime\.ps1'/);
  assert.match(ci,/if: steps\.ocr-cache\.outputs\.cache-hit != 'true'/);
  assert.match(ci,/Copy-Item "ocr-build-cache" \(Join-Path \$portable "ocr"\) -Recurse/);
  assert.match(ci,/Validate offline OCR from the extracted ZIP/);
});
test("retained OCR bundle fingerprint is checked by helper and explicit CI release",()=>{
  assert.match(helper,/Get-FileHash/);
  assert.match(helper,/ocr-build-fingerprint\.txt/);
  assert.match(helper,/Cached verified runtime and models unchanged/);
  assert.match(helper,/package-ocr-runtime\.ps1/);
  assert.match(helper,/verify-packaged-ocr\.ps1/);
  assert.match(ci,/windows-release-build:[\s\S]*?github\.event_name == 'workflow_dispatch'/);
  assert.match(ci,/Copy-Item "ocr-build-cache" \(Join-Path \$portable "ocr"\) -Recurse/);
  assert.doesNotMatch(dev,/package-ocr-runtime\.ps1/);
});
