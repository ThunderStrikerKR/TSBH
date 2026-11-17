# 35차 Windows IPC 반복 호출 최적화 — 실제 3단계 전후 검증

날짜: 2026-11-17. 실제 Windows Tauri DEV + WebView2, 격리된 TEST SQLite, 동일한 synthetic two-recipient 발주서와 거래처 선택 시나리오.

## 실측 요약

| 코드 버전 | 계측 IPC | 동일 입력 반복 IPC | React 주요 컴포넌트 렌더 함수 실행 | Windows 업무 검증 |
| --- | ---: | ---: | ---: | --- |
| 최초 기준선 f02ee3de | 86 | 44 | 212 | PASS, 33개 |
| 순수 조회·거래처 입력 안정화 2807b689 | 53 | 11 | 196 | PASS, 33개 |
| 접수·상세 패널 Rust 조정 공유 f1830ed3 | 45 | 3 | 192 | PASS, 33개 |
| 미확정 초안 중복 조정 제거 ef07dd0e | **38** | **0** | **172** | **PASS, 33개** |

최종 개선: IPC −48회 (−55.8%), 계측 범위 동일 입력 반복 −44회 (−100%), 계측 5종 React 렌더 함수 실행 −40회 (−18.9%).

## Windows 증거 (소스 SHA별 독립 증거)

- 기준선 f02ee3de: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44050666851
- 1차 2807b689: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44052739056
- 2차 실제 검증 f1830ed3: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44053969900
- 3차 실제 검증 ef07dd0e: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44054909373
- 최종 소스의 Rust output / frontend / parity CI: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44054909304 (PASS)

## 최종 측정 구간 (ef07dd0e)

| 업무 구간 | 측정 길이(ms) | 계측 IPC 호출 | 동일 입력 반복 | 핵심 컴포넌트 렌더 함수 실행 |
| --- | ---: | ---: | ---: | ---: |
| 접수 진입 직후 | 1,205 | 1 | 0 | 14 |
| 두 수취인 발주서 붙여넣기 | 4,796 | 18 | 0 | 58 |
| 거래처 선택 | 1,538 | 12 | 0 | 58 |
| 선택 후 표시 안정성 4.2초 | 4,223 | 7 | 0 | 42 |
| 합계 | 11,762 | 38 | **0** | **172** |

최종 IPC 38회 중 business_core_call 36회 및 클립보드 2회. 따라서 앱 전체 Tauri IPC 수치를 의미하지 않는다.

## 제거한 중복과 코드 소유권

1. SourceDocument.tsx: useMemo로 거래처 활성 목록 배열 참조를 보존. React 렌더만으로 client_candidates를 다시 호출하던 현상을 차단.
2. src/shared/native-read-coalescing.ts + src/tauri-bridge.ts: Rust가 계산하는 결정적 읽기 결과만 동일 도메인·명령·정확한 JSON 입력에 대해 진행 중/350ms 내 공유. 48건 한도. Rust 오류는 캐시 제거. React 계산 fallback 없음. 주문 저장·업무 상태 변경·reprice_order·order_edit_rules·reconcile_workflow는 일반 캐시 대상 제외.
3. HubWorkspace.tsx + useOrderDesk.tsx: 입력 State와 Order 객체가 정확히 같은 경우에만 상위 Rust reconcile 결과를 두 OrderDesk 인스턴스가 공유. 상세 패널의 passive price/shipping/prepare effects를 중복 실행하지 않고 접수 패널이 소유. 서로 다른 주문은 각각 독립 실행.
4. HubWorkspace.tsx: 거래처가 아직 없는 주문의 Rust 초안 조정도 두 패널 간 공통 결과로 공급하되, 기존 업무 보드에는 거래처 미확정 초안을 새로 노출하지 않음.
5. useNativeWorkflowReconcile.ts: 공유 중 로컬 훅을 비활성화할 때 아직 없는 resolved 값을 참조하지 않도록 enabled 조건을 포함함.

## 검증 범위 및 남은 사항

- 4개 구간의 반복 44→0은 동일한 테스트 시나리오에서의 계측 결과로, 다른 화면이나 더 큰 실제 DB에 존재할 모든 중복을 전수 부정하는 것은 아니다.
- 기존 Windows 비즈니스 GUI 시나리오 33개와 스크린샷 12개, 고객/주문 분리 및 저장·다시 읽기, 직원 확인·배송/가격 표시가 PASS.
- Stage2 / Stage3의 TypeScript 빌드, 프런트엔드 회귀 및 기존 Rust CI 결과가 PASS. 새 기능별 전체 28개 업무 TS↔Rust 동등성 COMPLETE 판정과는 별개.
- DEV React.StrictMode 함수 실행 수는 DOM 커밋 수가 아니며, 측정 구간 길이(특히 비동기 완료 대기)로 성능 지연 개선 비율을 계산해서는 안 된다.
- Release EXE + 오프라인 OCR 패키징 검증은 이번 DEV 측정에 포함되지 않았다.

## 검증 증거 분리

- PATCH: 2807b689, ccc19a92, f1830ed3, ef07dd0e
- UNIT: tests/tauri-native-pure-read-coalescing.test.mjs, tests/tauri-shared-live-order-flow.test.mjs, 3차 Rust output CI PASS
- DIFF: 업무 산출물 TS↔Rust 전체 차등 판정 아님. 기존 CI의 독립 parity 테스트는 PASS
- ACTION: 실제 Tauri WebView2 GUI 업무 자동화 33개 PASS
- CI: 44054909304 PASS
- WINDOWS: 44054909373 PASS, 수치·테스트·스크린샷 증거 연결

이 보고서는 구체적인 IPC 성능/중복 개선 범위에 한해 최종 검증 PASS이며, 전체 Rust 마이그레이션 최종 완료를 선언하지 않는다.
