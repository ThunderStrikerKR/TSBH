import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (name) => readFileSync(new URL('../' + name, import.meta.url), 'utf8');

test('Tauri v0.0.4 versions agree', () => {
  assert.equal(JSON.parse(read('package.json')).version,'0.0.4');
  assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).version,'0.0.4');
  assert.match(read('src-tauri/Cargo.toml'), /^version = "0\.0\.4"$/m);
  // The old local release CMD was intentionally removed. The explicit
  // release workflow now produces the portable bando_hub.exe instead.
  assert.match(read('.github/workflows/verify-tauri.yml'),
    /Copy-Item \(Join-Path \$root "bando_hub_tauri\.exe"\) \(Join-Path \$portable "bando_hub\.exe"\)/);
});
test('existing v0.0.1 user profile and product prices stay at same location', () => {
  const cfg = JSON.parse(read('src-tauri/tauri.conf.json'));
  assert.equal(cfg.identifier,'com.bandostand.bandohub.tauri');
  assert.match(read('src-tauri/src/startup_recovery.rs'), /bando-hub-v001\.sqlite/);
  assert.match(read('src-tauri/src/lib.rs'), /state_save/);
});
test('receipt archive is connected end-to-end', () => {
  const rust=read('src-tauri/src/lib.rs');
  const bridge=read('src/tauri-bridge.ts');
  assert.match(rust, /fn receipt_archive\(/);
  assert.match(rust, /generate_handler![\s\S]*receipt_archive/);
  assert.match(bridge, /receiptArchive: \(\) => invoke<[^\n]*\("receipt_archive"\)/);
  assert.match(rust, /reception::validate_reception/);
  assert.match(rust, /INSERT OR IGNORE INTO receipt_archive/);
});
