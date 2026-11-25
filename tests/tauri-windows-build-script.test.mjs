import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
const read = p => readFileSync(new URL("../"+p, import.meta.url),"utf8");
const exists = p => existsSync(new URL("../"+p, import.meta.url));
const verify = read(".github/workflows/verify-tauri.yml");
const output = read(".github/workflows/migration-output.yml");
const cache = read("scripts/ensure-node-deps.ps1");

test("removed legacy release launchers stay absent",()=>{
  assert.equal(exists("BUILD_WINDOWS_TAURI.cmd"),false);
  assert.equal(exists("BUILD_WINDOWS_EXE.cmd"),false);
});
test("real Windows release is manual only and stages EXE with offline verified OCR ZIP",()=>{
  assert.match(verify,/windows-release-build:[\s\S]*?if: github\.event_name == 'workflow_dispatch'/);
  assert.match(verify,/run: npm run build:windows/);
  assert.match(verify,/Copy-Item \(Join-Path \$root "bando_hub_tauri\.exe"\) \(Join-Path \$portable "bando_hub\.exe"\)/);
  assert.match(verify,/Copy-Item "ocr-build-cache" \(Join-Path \$portable "ocr"\) -Recurse/);
  assert.match(verify,/Validate offline OCR from the extracted ZIP/);
  assert.match(verify,/Compress-Archive[\s\S]*?BANDO-HUB-portable\.zip/);
  assert.match(output,/windows-p1p2-real-exe:[\s\S]*?github\.event_name == 'workflow_dispatch'/);
});
test("Node installation skips only unchanged manifest, lockfile and version fingerprints",()=>{
  assert.match(cache,/Get-BandoSha256/);
  assert.match(cache,/\[System\.Security\.Cryptography\.SHA256\]/);
  assert.match(cache,/\.bando-deps-fingerprint\.txt/);
  assert.match(cache,/Skipping npm installation/);
  assert.match(cache,/npm\.cmd ci --no-audit --no-fund/);
});
test("manual release launcher directly builds and verifies portable ZIP without old CMD",()=>{
  const launcher=read("PACKAGE_WINDOWS_TAURI.cmd");
  const packer=read("scripts/package-windows-tauri.ps1");
  assert.doesNotMatch(launcher,/BUILD_WINDOWS_TAURI\.cmd|BUILD_WINDOWS_EXE\.cmd/);
  assert.match(launcher,/package-windows-tauri\.ps1/);
  assert.match(packer,/ensure-node-deps\.ps1/);
  assert.match(packer,/npm\.cmd run build:windows/);
  assert.match(packer,/ensure-ocr-package\.ps1/);
  assert.match(packer,/Compress-Archive/);
  assert.match(packer,/Expand-Archive/);
  assert.match(packer,/verify-packaged-ocr\.ps1/);
  assert.match(packer,/반도허브-배포검증/);
  assert.ok(packer.indexOf("verify-packaged-ocr.ps1") <
    packer.indexOf("Move-Item -LiteralPath $zipStage -Destination $zip"));
});

test("manual release partial promotion restores old ZIP, folder and OCR cache",()=>{
  const pack=read("scripts/package-windows-tauri.ps1");
  for(const promoted of ["zipPromoted","portablePromoted","ocrPromoted"]){
    assert.match(pack,new RegExp("\\$"+promoted+" = \\$true"));
  }
  assert.match(pack,/Remove-Item -LiteralPath \$zip -Force/);
  assert.match(pack,/Move-Item -LiteralPath \$zipBackup -Destination \$zip/);
  assert.match(pack,/Move-Item -LiteralPath \$portableBackup -Destination \$portable/);
  assert.match(pack,/Move-Item -LiteralPath \$ocrBackup -Destination \$ocrLive/);
  const workflow=read(".github/workflows/migration-output.yml");
  assert.match(workflow,/frontend:[\s\S]*?ParseFile\(/);
  assert.doesNotMatch(workflow,/^  distribution-script-syntax:$/m);
});
