import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL("../"+path,import.meta.url),"utf8");
const source = read("src/SourceDocument.tsx");
const titles = read("src-tauri/src/business_core/catalog_clients/title_clients.rs");
const chooser = read("src/ManagementUI.tsx");
const hub = read("src/HubWorkspace.tsx");
const desk = read("src/OrderDesk.tsx");
const issues = read("src/StaffChecks.tsx");
const board = read("src/WorkflowBoard.tsx");
const hook = read("src/orders/useNativeWorkflowBoard.ts");
const dock = read("src-tauri/src/kakao_dock.rs");

test("paste uses selected native KakaoTalk conversation and Rust-only dealer authority", () => {
  assert.match(source,/window\.bando!\.dockWindows\(\), window\.bando!\.dockStatus\(\)/);
  assert.match(source,/activeChatHandle = dock\.activeChatHandle/);
  assert.match(source,/nativeAutoReceptionClient\(\s*state, chats, raw, activeChatHandle/);
  assert.match(source,/"rules", "order_edit_rules"/);
  assert.match(source,/action: "change_order_client"/);
  assert.doesNotMatch(source,/next\.clientId = matched\.id/);
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
  assert.doesNotMatch(chooser,/selectRef\.current\(suggested\[0\]\)/);
  assert.match(chooser,/거래처 후보 \{suggested\.length\}곳 확인/);
});

test("rust preview refresh preserves both review cards and processing-order cards", () => {
  assert.match(hub,/const lastVisiblePreview = useRef/);
  assert.match(hub,/state=\{boardPreview\}/);
  assert.match(hub,/key=\{\(detail\?\.id \?\? draft\.id\) \+ ":details:" \+ discardRevision\}/);
  assert.doesNotMatch(hub,/selected\?\.groupId \?\? "empty"\) \+ ":" \+ discardRevision/);
  assert.match(desk,/lastReadyChecks/);
  assert.match(desk,/blocks=\{shownChecks\}/);
  assert.match(issues,/const visible = blocks;/);
  assert.match(issues,/disabled=\{unavailable\}/);
  assert.match(board,/useNativeWorkflowBoard\(state, previewReady\)/);
  assert.match(board,/aria-busy=\{!ready\}/);
  assert.match(hook,/if \(!enabled\) return;/);
  assert.match(hook,/if \(!resolved\) return \{\}/);
});
