import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("Tauri account settings use isolated Rust patch command", () => {
  const rust = read("src-tauri/src/lib.rs");
  const settings = read("crates/bando-data-access/src/settings_commands.rs");
  const bridge = read("src/tauri-bridge.ts");
  const screen = read("src/SettingsPanel.tsx");
  assert.match(rust, /fn settings_patch\(/);
  assert.match(rust, /generate_handler![\s\S]*settings_patch/);
  assert.match(settings, /expected\.contains_key\(field\)/);
  assert.match(bridge, /patchSettings: .*invoke<State>\("settings_patch"/);
  assert.match(screen, /settingsChanges\(/);
  assert.match(screen, /onSaveSettings\(changes, expected\)/);
  assert.doesNotMatch(screen, /await onSave\(\{ \.\.\.state/);
});

test("Client per-account preference is stored and affects outbound billing", () => {
  const types = read("src/types.ts");
  const manager = read("src/ClientManager.tsx");
  const domain = read("src/domain.ts");
  const workflow = read("tests/rust-migration/oracles/legacy-workflow.ts");
  assert.match(types, /preferredAccountId\?: string/);
  assert.match(manager, /주 사용 입금계좌/);
  assert.match(manager, /preferredAccountId: value/);
  assert.match(domain, /if \(o\.accountMode === "MANUAL"\)/);
  assert.match(domain, /client\?\.preferredAccountId/);
  // The historical TypeScript billing oracle was relocated out of the shared domain.
  assert.match(read("tests/rust-migration/oracles/legacy-hub-messages.ts"), /recommendAccount\(o, s, products, c\)/);
  // Before confirmation, account recommendations use the live dealer;
  // confirmed orders must keep their captured preferred account setting.
  assert.match(workflow, /orderParty\(active, state\)/);
  assert.match(workflow, /orderParty\(order, state\)/);
});

test("Required sender information is validated at UI and Rust save boundary", () => {
  const manager = read("src/ClientManager.tsx");
  const native = read("crates/bando-data-access/src/validation.rs");
  const storage = read("crates/bando-data-access/src/storage.rs");
  const settings = read("crates/bando-data-access/src/settings_commands.rs");
  assert.match(manager, /"catalog_clients", "upsert_client"/);
  assert.doesNotMatch(manager, /if \(!edit\.address\?\.trim\(\)\)/);
  assert.match(native, /pub fn validate_clients\(/);
  assert.match(native, /client\["address"\]\.as_str\(\)\.unwrap_or\(""/);
  assert.match(native, /\["phone", "mobile"\]\.iter\(\)\.all/);
  assert.match(storage, /validate_clients\(&?current,\s*&state\)\?/);
  assert.match(settings, /settings_accounts::validate\(&state\)\?/);
  assert.match(settings, /settings_accounts::validate\(&state\)\?/);
});
