import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(new URL("../"+path,import.meta.url),"utf8");
const titles = read("src-tauri/src/business_core/catalog_clients/title_clients.rs");
const dock = read("src-tauri/src/kakao_dock.rs");

test("paste uses selected native KakaoTalk conversation and Rust-only dealer authority", () => {
  assert.match(dock,/active_chat_handle:inner\.active_chat\.map/);
  assert.match(titles,/"auto_match_reception_client"=>reception_client/);
});

test("receiver school and both destination phone numbers cannot trigger dealer auto-assignment", () => {
  assert.match(titles,/팔렬고등학교/);
  assert.match(titles,/010-9149-2496/);
  assert.match(titles,/010-3343-4754/);
  assert.match(titles,/연주용보면대 708-1 \/ 30개/);
  assert.match(titles,/지휘자보면대 BD CONDUCTOR \/ 1개/);
  assert.match(titles,/보면대카트 Cart-10 \/ 2개/);
  assert.match(titles,/if chats\.len\(\)==1/);
  assert.match(titles,/activeChatHandle/);
  assert.match(titles,/let title_count=chats\.iter\(\)\.filter/);
  assert.match(titles,/let Some\(\(label,value\)\)=line\.split_once/);
});
