# 35차 React 불필요 렌더링 최적화 — Windows 실측 및 판정 (2026-11-17)

## 기준과 검증 범위
- Windows Tauri DEV / 실제 WebView2 / 격리된 synthetic TEST SQLite
- 최적화 전 기준: ef07dd0e (Windows run https://github.com/ThunderStrikerKR/TSBH/actions/runs/44054909373)
- 최종 검증 대상 소스: 39c2353c208a74a674dc0b828479e281d9146e63
- Windows 증거: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44061377451 (35 GUI tests PASS / 12 screenshots)
- Rust + frontend CI: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44061377446 (PASS)
- DEV React.StrictMode 주요 컴포넌트 5종의 함수 본문 진입 횟수: DOM commit 수나 앱 전체 렌더 횟수가 아님

## 동일 Windows 시나리오 계측
| 구간 | 기준 ef07dd0e | 검증 39c2353c | 차이 |
| --- | ---: | ---: | ---: |
| 접수 진입 직후 1.2초 (초기 비동기 작업 포함) | 14 | 42 | +28 |
| 두 수취인 발주서 붙여넣기 | 58 | 50 | -8 |
| 거래처 확정 | 58 | 54 | -4 |
| 확정 후 4.2초 안정화 | 42 | 42 | 0 |
| **사용자 업무 3구간** | **158** | **146** | **-12 (-7.6%)** |
| **원래 4구간 합계** | **172** | **188** | **+16 (개선 미입증)** |

두 버전에서 동일 입력 IPC 재호출은 모두 0회. 최적화 후 4구간의 감사 IPC는 42회.
초기 실행의 비동기 처리 시점 차이가 전체 4구간에 반영되므로 전체 렌더 비용 개선을 주장하지 않는다.
작업 구간 -7.6%는 함수 진입 수 감소이며 앱 반응시간이나 CPU 시간의 7.6% 개선을 의미하지 않는다.

## 업무와 독립된 App 상태 갱신 (직접 실측)
- 알림 모달 열기와 닫기: 133 ms
- HubWorkspace 렌더 함수 진입: 0회
- OrderDesk:reception: 0회
- OrderDesk:details: 0회
- WorkflowBoard: 0회
- MessageWorkspace: 0회
- 증거: Windows 로그 PERF_REAL_WINDOWS_APP_ONLY_NOTIFICATION_TOGGLE / APP_ONLY_NOTIFICATION_WORKSPACE_RENDER
- 검증 목표: 알림 상태만 바뀔 때 주문 상태가 바뀌지 않으며 주문 화면 렌더가 일어나지 않음을 입증

## 유지한 변경
- src/shared/useEventCallback.ts: stable callback identity and current order/state behavior, no obsolete state closure
- src/App.tsx: stable workspace event handlers and memoized HubWorkspace prevent notifications/dock parent changes from forcing order screen rerender
- src/HubWorkspace.tsx: stable Rust preview, portalTargets, linkController and callbacks to memoized receipt/detail/workflow/message panes
- src/OrderDesk.tsx, src/WorkflowBoard.tsx, src/MessageWorkspace.tsx: shallow-prop memo wrappers; changed draft and state continue rendering
- src/app/useAppModel.ts: active 300ms dock polling only updates React dock state when result content changed (dock-active improvement not measured in these Windows CI scenarios)
- scripts/migration32-dev-business-ui.mjs: dedicated App-only notification toggle real WebView2 audit, original receipt workflow retained
- tests/hub-render-stability.test.mjs, tests/app-workspace-render-stability.test.mjs, tests/app-dock-render-stability.test.mjs and updated existing assertions: preserve latest callback, draft, workflow save provenance

## 효과 없어 철회한 변경
- c1397a60: OrderDesk mount deferred until portal DOM targets exist.
- https://github.com/ThunderStrikerKR/TSBH/actions/runs/44062399463 : 35 Windows GUI tests PASS, but user action 146 unchanged and four-phase total 196.
- c71de84d: revert portal-only gate and its test.
- Git tree SHA verified equal for c71de84d and the earlier Windows-PASS 39c2353c: 7b32e814ddd2d3f16c3f01fc82979f7b8dc90449.
- Therefore final running source is byte-for-byte identical to the Windows validated source before this documentation-only change.

## Sign-off scope
- PATCH: code changes landed on main; ineffective gate reverted.
- UNIT/CI: Rust/frontend verified PASS on 39c2353c.
- ACTION/WINDOWS: 35 GUI workflow checks PASS on exact same source tree.
- REACT PERF: App-only notification toggling gives 0 workspace renders; receipt workflow actions 158 to 146.
- LIMITATION: full 4-phase total 172 to 188, NOT a demonstrated overall render reduction. No measured CPU or React Profiler commit-time improvement.
- OUT OF SCOPE: full TS to Rust migration final PASS, optimized active dock benchmark and Release Windows binary.
