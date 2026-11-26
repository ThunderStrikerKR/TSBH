import {test} from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=path=>readFileSync(new URL("../"+path,import.meta.url),"utf8");
const native=read("src-tauri/src/kakao_dock.rs");

test("all explicit Kakao commands are registered in Tauri",()=>{
  const rust=read("src-tauri/src/lib.rs");
  for(const c of ["dock_status_native","dock_cycle_native","dock_start_native",
    "dock_stop_native","dock_select_native","dock_windows_native",
    "dock_region_native","dock_mode_native"]){
    assert.ok(rust.includes("kakao_dock::"+c),c+" not registered");
  }
});

test("native Kakao process authentication, safe original placement and no injection",()=>{
  for(const fragment of [
    "QueryFullProcessImageNameW","KakaoTalk.exe","GetWindowThreadProcessId",
    "GetWindowPlacement","SetWindowPlacement","restore_on_exit",
    "pub struct Tracked","pub fn restore_related","GetForegroundWindow",
  ]) assert.ok(native.includes(fragment),fragment+" absent");
  assert.doesNotMatch(native,/SetParent|ReadProcessMemory|WriteProcessMemory|SetWindowsHookEx/);
  assert.match(native,/verified\(t\.hwnd\)==Some\(t\.pid\)/);
  assert.match(native,/WS_CAPTION/);
  assert.match(native,/SWP_FRAMECHANGED/);
  assert.match(native,/fn hide_caption\(hwnd:Hwnd\)/);
  assert.match(native,/fn restore_style\(hwnd:Hwnd,original:isize\)/);
  assert.match(native,/original_style:isize/);
  assert.match(native,/restore_style\(t\.hwnd,t\.original_style\)/);
  assert.match(native,/restore_style\(s\.hwnd,s\.original_style\)/);
  assert.match(native,/if IsIconic\(hwnd\)==0/);
});

test("both modes use the original left KakaoTalk pane and never move BANDO HUB",()=>{
  assert.match(native,/pub fn stack\(s:&mut Session,app:&AppHandle,region:Region\)/);
  assert.match(native,/fn stack_targets\(area:Rect,main:Hwnd,chats:&\[Hwnd\]\)/);
  assert.match(native,/SendMessageTimeoutW/);
  assert.match(native,/WM_GETMINMAXINFO/);
  assert.match(native,/DwmGetWindowAttribute/);
  assert.match(native,/banner_top\(main,main_visible\)/);
  assert.match(native,/let old_visible=visible_rect\(main\)\?/);
  assert.match(native,/bottom:area.top\+old_visible.height\(\)/);
  assert.match(native,/min_visible_height\(main\)\?/);
  assert.match(native,/if chats\.is_empty\(\) \{/);
  assert.match(native,/previous\.height\(\)<area\.height\(\)/);
  const body=native.split("pub fn stack(s:&mut Session")[1]?.split("pub fn sync(")[0];
  assert.ok(body);
  assert.doesNotMatch(body,/set_rect\(s\.main|SetWindowPlacement\(s\.main|split_rect\(/);
});

test("all regular KakaoTalk MAIN and CHAT windows are tracked and arranged in relative order",()=>{
  assert.match(native,/WS_SYSMENU/);
  assert.match(native,/pub fn sync_related\(/);
  assert.match(native,/for candidate in list\.iter\(\)\.filter\(\|c\|c\.process_id==s\.pid\)/);
  assert.match(native,/if tracked\.len\(\)>=32/);
  assert.match(native,/newcomers\.iter\(\)\.rev\(\)/);
  assert.ok(native.includes("let initial_enrollment=tracked.is_empty()"));
  assert.ok(native.includes("if !initial_enrollment || active_chat.is_none()"));
  assert.match(native,/SetWindowPos\(base,chat,0,0,0,0,/);
  assert.match(native,/SetWindowPos\(s\.main,base,0,0,0,0,/);
  assert.doesNotMatch(native,/set_rect\(t\.hwnd,target,HWND_TOPMOST\)/);
});

test("failed layout switches are reversible and do not disconnect current KakaoTalk",()=>{
  const mode=native.split("pub fn dock_mode_native(")[1]?.split("#[tauri::command]")[0];
  assert.ok(mode);
  assert.match(mode,/let before=session/);
  assert.match(mode,/session=before/);
  assert.match(mode,/inner\.session=Some\(session\)/);
  assert.match(mode,/return Err\(error\)/);
  const related=native.split("pub fn sync_related(")[1]?.split("pub fn measured_ratio(")[0];
  assert.ok(related);
  assert.match(related,/let snapshots=all\.iter\(\)/);
  assert.match(related,/for \(hwnd,before\) in snapshots/);
  assert.match(native,/inner\.restore_related\(\)/);
  assert.match(native,/native::restore\(&s\)/);
  const restore=native.split("pub fn restore(s:&Session)")[1]?.split("#[cfg(test)]")[0];
  assert.ok(restore);
  assert.doesNotMatch(restore,/SetWindowPlacement\(s\.main/);
  assert.match(restore,/SetWindowPlacement\(s\.hwnd/);
});

test("connecting to an invalid HWND does not drop the existing session",()=>{
  const start=native.split("fn start(app:")[1]?.split("#[tauri::command]")[0];
  assert.ok(start);
  const check=start.indexOf("let requested=native::candidates()");
  const taking=start.indexOf("let previous=inner.session.take()");
  assert.ok(check>=0&&taking>check);
  assert.ok(start.includes('current.hwnd as usize)==root_handle'));
  assert.ok(start.includes('inner.active_chat=if requested.kind=="CHAT"'));
  assert.ok(start.includes("inner.sync_related(&current,app)?"));
  assert.match(start,/if rollback\.is_ok\(\)/);
});

test("the synthetic native Windows test covers both chat and main windows",()=>{
  assert.match(native,/spawn_fixture_window\("카카오톡"\)/);
  assert.match(native,/spawn_fixture_window\("합성 대화"\)/);
  assert.match(native,/stack_targets\(pane,fixture_hwnd,&\[chat_hwnd\]\)/);
  assert.match(native,/SetWindowPlacement must restore the original window bounds/);
});

test("extend KakaoTalk MAIN footer off physical screen, preserve CHAT composer and reversible placement", () => {
  const dock = read("src-tauri/src/kakao_dock.rs");
  assert.match(dock, /fn offscreen_main_target\(area:Rect,screen:Rect,dpi:u32\)->Rect/);
  assert.match(dock, /screen\.bottom/);
  assert.match(dock, /monitor_info\(s\.main\)\?\.monitor/);
  assert.match(dock, /else if \*kind=="MAIN"\s*\{\s*offscreen_main_target\(area,screen,main_dpi\)/);
  assert.match(dock, /else\{area\};\s*set_visible_rect\(\*hwnd,target\)/);
  assert.match(dock, /SetWindowPlacement\(s\.hwnd,&s\.kakao_original\)/);
  assert.match(dock, /fn kakao_main_footer_is_placed_below_physical_screen_without_moving_chat/);
});
