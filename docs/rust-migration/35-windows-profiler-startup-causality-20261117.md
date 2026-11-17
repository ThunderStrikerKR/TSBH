# 35차 React Profiler 병목 및 초기 로딩 상태-IPC 인과 분석

작성일: 2026-11-17 (KST). 검증 환경: 실제 GitHub Windows DEV runner + Tauri WebView2 + synthetic TEST SQLite (360 products, 2 clients).

## 변경된 프로그램과 계측

- 기준 CI-only profiler commit `204f000e`: React Profiler 실제 `actualDuration`, `baseDuration`, commit time, 각 Rust IPC request/resolved 및 React 상태 변화 시각 기록
- `2973698d`: 기존 startup / dock typed invoke contracts 그대로 유지하고, React 관측 로직에서 IPC 결과 추적
- `6ecd92ea`: `useNativeWorkflowReconcile`은 disabled/empty draft에서, `useNativeWorkflowBoard`와 `useNativeOrderPreview`는 disabled에서 전체 상태 fingerprint 직렬화를 생략. enabled 상태의 Rust 결과 및 stale input guards 변경 없음
- `f12f626c`: 접수/상세의 동일 `State.products` 배열에 `WeakMap` 키로 화면 표시용 `registeredProducts` + `sortProducts` 결과 공유. 상품 매칭/단가/운임 업무 계산과 무관. 새로운 카탈로그 배열은 새 결과로 정렬
- `29a60b54`: DEV StrictMode 이중 effect에서 첫 read-only startup_status의 동일 Promise 재사용. 취소된 첫 effect는 상태를 쓰지 않음; 실패 처리와 재시도 경로 유지
- `0334463d`: 동일 Windows runner에서 UI 정렬 원본/캐시 및 fingerprint 계산 비용을 직접 비교하는 CI-only WebView2 실제 카탈로그 마이크로벤치 추가

## Windows React Profiler 기준선 (5a8b233b)

Evidence: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44066282147 (Windows GUI 36 PASS).

| Profiler 경계 | 4개 시나리오 구간 actualDuration 합계 |
| --- | ---: |
| AppRoot (하위 포함) | 277.4ms |
| OrderDesk:details | 100.6ms |
| OrderDesk:reception | 84.0ms |
| WorkflowBoard | 13.6ms |
| MessageWorkspace | 7.3ms |

AppRoot 및 하위 경계 시간은 겹치므로 합산할 수 없다. actualDuration은 React의 커밋에 포함된 render-phase 측정치이며 앱 전체 사용자 체감 지연이나 순수 DOM commit 시간과 같지 않다.

## 코드 최적화 후 Windows 교차 실행 (독립 runner, 참고값)

| SHA | AppRoot 4구간 actualMs | Detail | Reception | 결과 |
| --- | ---: | ---: | ---: | --- |
| 5a8b233b baseline | 277.4 | 100.6 | 84.0 | Windows 36 PASS |
| 6ecd92ea skip-disabled-hash | 409.0 | 157.4 | 128.4 | Windows 36 PASS |
| f12f626c shared-product-view | 256.0 | 100.9 | 77.7 | Windows 36 PASS |
| 29a60b54 startup-status-share | 245.7 | 97.6 | 77.8 | Windows 36 PASS |
| 0334463d with in-run microbenchmark | 345.6 | 143.7 | 106.0 | Windows 37 PASS |

가상 Windows runner의 CPU 부하/성능과 CI 추가 마이크로벤치로 측정 구간 시작 시점이 달라진다. 이 교차 실행 값으로 앱 전체 속도 향상률이나 퇴보율을 산정하지 않는다.

Evidence:
- f12f626c: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44067346735
- 29a60b54: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44067536660
- 최종 0334463d: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44068158959
- 최종 frontend/Rust CI: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44068158939 (PASS)

## 동일 Windows runner 마이크로벤치 (0334463d)

로딩된 실제 synthetic 상품 카탈로그 360개, 80회 반복 1묶음 × 5묶음, 중앙값. 결과 순서·상품 ID 동일함을 매 반복 묶음마다 검증.

| 비교 | 80회 중앙값 |
| --- | ---: |
| 기존 sortProducts(registeredProducts(products)) 매번 계산 | 59.8ms |
| shared WeakMap 표시 정렬 재사용 | 0.00ms (소수 둘째 자리 표시에서 반올림) |
| 전체 State fingerprint JSON.stringify({input:state,drafts:[]}) | 33.1ms |

해석: 동일 카탈로그 참조를 다시 정렬하는 80회분 CPU 비용은 캐시로 제거. 실제 UI에서는 State.products 레퍼런스 변화 시 한 번은 정렬되므로, 59.8ms가 그대로 앱 지연에서 절감된다고 볼 수 없다. Disabled Rust hook은 실제 Rust 요청이 없으므로 33.1ms/80회의 불필요한 반복 fingerprint 연산 경로를 생략. 빈 초안 또는 비활성 키만 생략하며 업무 계산은 유지.

## 초기 IPC → React 상태 → 렌더 인과 타임라인

Before change run 44066282147 (`5a8b233b`):
- React.StrictMode mount effect `useAppModel.startup-effect` 2회 (relative time 954ms/955ms)
- Native startup_status resolved 2회 (958ms/959ms); 두 effect 중 취소되지 않은 effect만 React startup state 반영
- state_load request 960ms, completed 1029ms (69ms); React state loaded 360 products/2 clients and initial draft created 1029ms
- Rust preview_workflow starts 1054ms, preview_order_lines 1062ms, order_issues 1063ms, should_refresh_pricing 1064ms, delivery_requirements 1065ms, read-only Rust reconcile_workflow starts 1067ms and StrictMode replay starts again 1087ms
- portal targets become available around 1107ms; each independent hook resolves to separate React local state, even when equivalent native read IPC was coalesced

After change run 44067536660 (`29a60b54`):
- `startup-effect` remains 2 calls in StrictMode (731ms/733ms); only one effective startup status state update (737ms). Initial startup native call is shared via useRef Promise (source-level invariant and unit test)
- `state_load` request 737ms and response 793ms (56ms), initial draft set at 793ms
- Rust preview_workflow starts 812ms, preview_order_lines 819ms, order_issues 820ms, should_refresh_pricing and delivery_requirements 821ms
- `reconcile_workflow` starts 823ms and 846ms. Intentionally NOT globally cached: temporary audit eventIds and timestamps are operation inputs; no change to the previously approved business audit/preview semantics
- The UI has separate Rust projection hooks for WorkflowBoard, Reception OrderDesk and Detail OrderDesk. Their local `setResolved` state changes cause additional React commits even though pure read IPC transport coalesces exact requests.

## Safety, parity and sign-off

Business model and output remain Rust-owned. No TS fallback introduced and no changes to price, VAT, freight, workflow stage or shipment persistence. Disabled hook fingerprint bypass preserves `ready`, `state`, stale response guards and displayKey. Cache only stores product order by immutable array identity.

Windows GUI PASS: 36 tests on intermediate versions and 37 tests / 12 screenshots on final source 0334463d. The final source's Rust/frontend CI PASS. No claim of full project Rust migration PASS, Release EXE packaging PASS, or general production-wide latency improvement.

Recommended follow-up evidence for overall app-speed claims: same-runner A/B with identical startup load, React commit profiling and CPU tracing under production-sized customer/order data. DEV StrictMode replays are diagnostic behavior, not proof of a production race.
