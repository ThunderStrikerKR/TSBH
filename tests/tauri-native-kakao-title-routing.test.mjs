import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");

test("Kakao title/client association is saved by native Rust with one SQLite commit", () => {
  const component = read("src/KakaoClients.tsx");
  const adapter = read("src/native-title-clients.ts");
  const native = read("src-tauri/src/business_core/catalog_clients.rs");
  assert.match(component, /await nativeRememberTitle\(/);
  assert.match(component, /await onSave\(/);
  assert.doesNotMatch(component, /\brememberTitle\(/);
  assert.match(adapter, /"catalog_clients", "remember_title_client"/);
  assert.match(native, /"remembered_title_client" \| "remember_title_client"/);
});
test("clipboard paste combines active KakaoTalk HWND with Rust-only automatic dealer matching", () => {
  const source = read("src/SourceDocument.tsx");
  const adapter = read("src/native-title-clients.ts");
  assert.match(source, /nativeAutoReceptionClient\(/);
  assert.match(source, /await nativeAutoReceptionClient\(/);
  assert.match(adapter, /"catalog_clients", "auto_match_reception_client"/);
  const catalogDispatcher = read("src-tauri/src/business_core/catalog_clients.rs");
  assert.match(catalogDispatcher, /"remember_title_client" \| "auto_match_reception_client"\s*=>\s*title_clients::execute/,
    "The live dispatcher must route automatic dealer matching");
  assert.doesNotMatch(source, /거래처 자동 연결에 실패했습니다\./,
    "An optional dealer match cannot display a red error toast");
  assert.match(source, /console\.warn\("거래처 자동 연결을 건너뛰었습니다\."/);
  assert.doesNotMatch(source, /\brememberedTitleClient\(/);
  assert.match(source, /activeChatHandle = dock\.activeChatHandle/);
  assert.match(source, /latestReception\.current\.order\.id !== order\.id/);
  assert.match(source, /latestReception\.current\.order\.raw !== order\.raw/);
  assert.match(source, /latestReception\.current\.text !== text/);
});

test("production routing and original TypeScript differential scenarios remain available", () => {
  const router = read("src-tauri/src/business_core/rules/mod.rs");
  const native = read("src-tauri/src/business_core/rules/kakao_title.rs");
  const parity = read("tests/rust-migration/rules/parity/compare.mts");
  assert.match(router, /"remember_kakao_title_client" \| "remembered_kakao_title_client"/);
  assert.match(native, /unicode_normalization::UnicodeNormalization/);
  for (const name of ["remember normalized chat", "duplicate normalized titles mark ambiguous",
    "two chat windows are not selected", "inactive dealer blocked"]) {
    assert.ok(parity.includes(name), name);
  }
});
