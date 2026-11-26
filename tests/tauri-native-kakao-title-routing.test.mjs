import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");

test("Kakao title/client association is saved by native Rust with one SQLite commit", () => {
  const native = read("src-tauri/src/business_core/catalog_clients.rs");
  assert.match(native, /"remembered_title_client" \| "remember_title_client"/);
});

test("clipboard paste combines active KakaoTalk HWND with Rust-only automatic dealer matching", () => {
  const catalogDispatcher = read("src-tauri/src/business_core/catalog_clients.rs");
  assert.match(catalogDispatcher, /"remember_title_client" \| "auto_match_reception_client"\s*=>\s*title_clients::execute/,
    "The live dispatcher must route automatic dealer matching");
});

test("Native Kakao title routing and normalization", () => {
  const router = read("src-tauri/src/business_core/rules/mod.rs");
  const native = read("src-tauri/src/business_core/rules/kakao_title.rs");
  assert.match(router, /"remember_kakao_title_client" \| "remembered_kakao_title_client"/);
  assert.match(native, /unicode_normalization::UnicodeNormalization/);
});
