import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (name) => readFileSync(new URL("../" + name, import.meta.url), "utf8");

test("Tauri account settings use isolated Rust patch command", () => {
  const rust = read("src-tauri/src/lib.rs");
  const settings = read("crates/bando-data-access/src/settings_commands.rs");
  assert.match(rust, /fn settings_patch\(/);
  assert.match(rust, /generate_handler![\s\S]*settings_patch/);
  assert.match(settings, /expected\.contains_key\(field\)/);
});

test("Required sender information is validated at UI and Rust save boundary", () => {
  const native = read("crates/bando-data-access/src/validation.rs");
  const storage = read("crates/bando-data-access/src/storage.rs");
  const settings = read("crates/bando-data-access/src/settings_commands.rs");
  assert.match(native, /pub fn validate_clients\(/);
  assert.match(native, /client\["address"\]\.as_str\(\)\.unwrap_or\(""/);
  assert.match(native, /\["phone", "mobile"\]\.iter\(\)\.all/);
  assert.match(storage, /validate_clients\(&?current,\s*&state\)\?/);
  assert.match(settings, /settings_accounts::validate\(&state\)\?/);
});
