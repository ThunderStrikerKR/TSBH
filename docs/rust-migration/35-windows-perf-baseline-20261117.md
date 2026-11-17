# 35차 Windows 업무 IPC·React 실행 횟수 실측 (2026-11-17)

- 측정 커밋: f02ee3de8a6c994cdee4df4d9d99dbb0d843aec8
- 증거: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44050666851
- 환경: 실제 Windows Tauri DEV + WebView2 + 격리 TEST SQLite
- 결과: Windows smoke PASS, GUI 업무 검사 33개 PASS, 스크린샷 12개
- 도구: scripts/migration32-dev-business-ui.mjs, src/perf-audit.ts, src/tauri-bridge.ts
- VITE_BANDO_NATIVE_AUDIT=true에서만 추가 계측. 업무 계산·운영 데이터 변경 없음.

## 단계별 측정

| 단계 | 시간(ms) | 계측 IPC | 업무 IPC | 동일 입력 반복 | 계측 컴포넌트 렌더 함수 실행 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 접수 화면 진입 직후 | 1219 | 8 | 8 | 4 | 50 |
| 복수 수취인 발주서 붙여넣기 | 5053 | 40 | 38 | 22 | 54 |
| 가상소매점 거래처 확정 | 2031 | 24 | 24 | 11 | 62 |
| 확정 후 표시 안정성 4.2초 | 4260 | 14 | 14 | 7 | 46 |
| **합계** | **12563** | **86** | **84** | **44** | **212** |

반복 44회는 같은 업무 입력을 다시 보낸 횟수이며, 삭제 가능하다고 확정한 횟수가 아니다.
초기 계측기의 readOps Set에서 순수 조회 client_candidates를 분류하지 않아
읽기 전용 반복 후보가 28회로 출력되었다. 이 조회의 반복 12회를 합하면
**읽기 전용 반복 40회**, 그 외 order_edit_rules·reprice_order 등은 4회다.

## 기능별 호출 및 같은 입력 반복

| IPC operation | 호출 | 같은 입력 반복 |
| --- | ---: | ---: |
| catalog_clients.client_candidates | 15 | 12 |
| rules.reconcile_workflow | 12 | 7 |
| rules.preview_order | 12 | 6 |
| rules.preview_workflow | 8 | 5 |
| rules.should_refresh_pricing | 6 | 3 |
| rules.order_issues_with_actions | 6 | 3 |
| rules.order_edit_rules | 7 | 3 |
| rules.preview_order_lines | 4 | 2 |
| rules.delivery_requirements_with_messages | 4 | 2 |
| rules.reprice_order | 2 | 1 |

입력 비교는 도메인/operation/action과 동일한 JSON 입력 문자열을 사용했다.
조회용 reconcile_workflow의 매번 생성되는 now 및 eventIds는 제외하고 state와 draft를 비교했다.

## React 주요 컴포넌트 실행

| 컴포넌트 | 실행 |
| --- | ---: |
| HubWorkspace | 22 |
| WorkflowBoard | 28 |
| MessageWorkspace | 34 |
| OrderDesk:reception | 70 |
| OrderDesk:details | 58 |
| **합계** | **212** |

함수 본문 직접 실행이며 DEV StrictMode 중복 실행도 포함한다.
React DOM commit/실제 변경 수, 전체 하위 컴포넌트 렌더 횟수를 나타내지 않는다.
따라서 212회를 전부 불필요한 렌더라고 판단해서는 안 된다.

## 중복 호출의 코드 원인

- 붙여넣기 구간에서 client_candidates 12회 중 11회 입력 동일:
  SourceDocument.tsx가 clients={state.clients.filter(...)}를 JSX에서 매번 새 배열로 생성.
  native-catalog-queries.ts의 useNativeClients는 그 배열의 참조가 변할 때
  조회 state를 새로 만들고 useNativeCatalogQuery의 Effect는 다시 Rust IPC를 요청한다.
  첫 번째 최소 변경 후보는 이 배열을 안정화하는 것이다.
- HubWorkspace와 접수용·상세용 OrderDesk가 별도 useNativeWorkflowReconcile 및
  Rust preview 훅을 실행하여 동일 입력 중복이 발생할 수 있다.
  실제 반복: reconcile_workflow 7회, preview_order 6회, preview_workflow 5회.
- 접수 사용자 시나리오에서도 OrderDesk:details 함수 진입이 58회 기록됐다.
  실제 화면 필요성과 연결 상태 확인 전 무조건 언마운트하거나 삭제하지 않는다.

## 판정 범위

- IPC 86회는 auditNativeCall이 수집한 업무 코어·일부 클립보드 등 명령만 포함한다.
  도킹·state_load 및 직접 invoke 등 앱 전체 Tauri IPC를 모두 계측한 값이 아니다.
- 첫 측정은 화면 진입 직후로 완전한 안정 idle이 아니며, 마지막 측정도
  거래처 확정 후 늦게 처리되는 작업이 포함된 관찰 구간이다.
- 이 보고는 최적화 전 기준선이다. CI 성공은 전체 기능 동등성 PASS가 아니다.
  수정 후 같은 Windows 시나리오에서 비교하고 단위·차등·업무·CI·Windows 결과를 따로 기록한다.
