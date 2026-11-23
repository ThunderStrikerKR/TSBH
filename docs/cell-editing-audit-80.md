# 반도허브 80차 — 셀형 입력 전수조사 및 적용 보고서

**기준 저장소:** `ThunderStrikerKR/TSBH`  
**기준 브랜치:** `main`  
**수정 브랜치:** `fix/80-cell-navigation-shared-handler-20261124`  
**PR:** [#52 셀 편집 공통 처리 개선](https://github.com/ThunderStrikerKR/TSBH/pull/52)  
**조사 범위:** React/TypeScript 운영 화면과 Rust/egui 화면의 직접 편집형 셀  
**판정:** 조사 대상 셀에 공통 키 분류·IME 차단·이동·확정 게이트를 연결했다. React와 Rust의 타깃 검증은 통과했으며, Windows 실화면 검증과 main 병합은 아직 남아 있다.

## 1. 적용 대상으로 판정한 셀

| 화면 | 실제 셀 | 적용 위치 | 적용 내용 |
| --- | --- | --- | --- |
| 상품 관리 React | 도매가·반도매가·소매가 | `src/ProductManager.tsx`, `src/PriceCell.tsx` | 명시적 3열 좌표, 4방향 이동, Enter 동일 열 이동, Tab 순서, IME 보호 |
| 수동 분류 React | 수취인·상품·수량 | `src/orders/ManualClassification.tsx` | 공통 이벤트 핸들러 + 화면별 좌표 resolver, 수량 확정 비동기 완료 후 포커스 이동 |
| 주문 접수 React | 품목 수량 | `src/OrderDesk.tsx` | 수량 행 좌표, 공통 방향키·Enter·Tab 이동 |
| 처리 중 주문 React | 실제 운임 | `src/WorkflowFooter.tsx` | 주문 접수 표의 같은 그리드 안에서 수량·운임 셀 간 이동, 기존 blur 저장 유지 |
| 상품 관리 egui | 도매가·반도매가·소매가 | `egui-app/src/product_manager.rs` | 공통 Rust 키 디코더·좌표 resolver, busy 셀 건너뛰기, IME 차단 |
| 수동 분류 egui | 선택된 수취인/상품 편집 | `egui-app/src/manual_workspace.rs` | 공통 Rust 키 디코더·resolver, 좌우/상하 및 순차 이동, IME 차단 |
| 접수 egui | 수취인·품목 수량·단가·운임 | `egui-app/src/main.rs` | 명시적 행·열·순서 맵, 공통 Rust 키 디코더, 기존 inline 저장·전환 경로로 이동 |

## 2. 이번 추가 수정에서 확인한 누락

1. React 수동 분류는 공통 resolver만 사용하고 자체 keydown 분기를 유지하고 있었다. 이 때문에 마지막 셀의 Tab이 브라우저 기본 이동으로 빠져나가지 못했다. 공통 `handleCellNavigationKey`에 연결하고 Tab 경계 처리를 통일했다.
2. React 수동 분류의 수량 확정은 비동기 Rust 작업이었다. 이동 전에 확정 결과를 기다리고 성공한 경우에만 다음 셀로 포커스를 옮기도록 연결했다. 실패하면 현재 셀에 남고, 마지막 셀 Tab의 비동기 실패도 기존 입력 셀로 포커스를 복귀한다.
3. egui 상품 가격표의 기존 키 판정에는 IME 조합 보호가 없었다. 접수·수동 분류·상품 가격표가 각자 키를 분류하던 부분을 `cell_navigation::navigation_key_from_input`으로 통합하고 IME 이벤트를 공통 차단한다.
4. 화면별 Enter 규칙은 유지했다: 일반 접수는 순차 이동, 수동 분류는 기존 순방향 이동, 가격표는 동일 열의 위/아래 이동이다.

## 3. 공통 구현

- React 공통 모듈: `src/ui/cell-navigation.ts`
  - `handleCellNavigationKey`: IME·키 분류·Tab 경계·동기/비동기 확정 게이트·포커스 이동
  - 화면별 어댑터는 셀 좌표와 업무별 저장/취소 동작만 제공
- Rust 공통 모듈: `egui-app/src/cell_navigation.rs`
  - `navigation_key_from_input`: 방향키·Enter·Tab 분류 및 IME 이벤트 차단
  - `resolve_cell_navigation_target`: 명시적 행·열·순서 기반 이동 대상 결정
- 계약 문서: `docs/cell-editing-contract.md`

React와 egui는 서로 다른 UI 프레임워크이므로 시각 입력 위젯을 하나의 소스 컴포넌트로 공유하지 않는다. 대신 키 입력 정책과 이동 계약을 각각의 네이티브 구현에서 공통화한다. 가격·수량·운임의 업무 계산, 검증, DB 저장은 기존 Rust reducer를 유지한다.

## 4. 공통 대상에서 제외한 입력

다음은 표의 데이터 셀을 직접 편집하는 컨트롤이 아니므로 셀 이동을 강제하지 않는다.

- 검색 입력: `ManagementSearch`, 거래처·상품 검색 등
- 날짜 선택기 및 기간 필터
- 체크박스·라디오·선택형 드롭다운
- 상품 상세·거래처 상세·설정의 일반 폼
- OCR 원문, 자동 메시지, 관리 메모 등 장문 입력
- Excel 관리·주문 내역·일일 결산의 읽기 전용 표 셀
- 상품별 운임 설정 모달의 일반 폼 필드
- `ShippingFeeRanges`의 반복 운임 행: 표 데이터 셀이 아닌 모달 폼 목록이므로 별도 UX 검토 대상으로 유지

## 5. 자동 검증 증거

| 검증 | 결과 | 증거 |
| --- | --- | --- |
| TypeScript `tsc --noEmit` | PASS | [PR #52 셀 이동 계약 실행](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45265644261) |
| React 셀 이동 계약 | PASS, 10/10 | [PR #52 셀 이동 계약 실행](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45265644261) |
| 가격 셀 자동 저장 회귀 | PASS, 1/1 | [PR #52 셀 이동 계약 실행](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45265644261) |
| 가격 셀 인라인 저장 회귀 | PASS, 3/3 | [PR #52 셀 이동 계약 실행](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45265644261) |
| egui 전체 `cargo check` | PASS | [PR #52 셀 이동 계약 실행](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45265644261) |
| Rust 공통 키 디코더·이동 단위 테스트 | PASS, 6/6 | [PR #52 셀 이동 계약 실행](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45265644261) |

## 6. 미완료·미검증

1. Windows WebView2 실제 화면에서 각 입력 유형의 키보드 이동, 포커스 복귀, IME 조합 및 저장 실패를 확인하지 않았다.
2. egui 입력 위젯의 키 이벤트 소비와 셀 경계 동작은 `cargo check` 및 단위 테스트를 통과했으나 실제 GUI 동작 검증은 남아 있다.
3. Ctrl/Cmd+Z는 공통화하지 않았다. 기존 화면의 실행 취소 동작만 유지한다.
4. 주문 접수 수량과 실제 운임의 Esc 취소·셀 단위 undo는 즉시 반영/blur 저장 구조를 고려한 별도 정책이 필요하다.
5. PR에 함께 실행된 광범위한 마이그레이션 검증에는 이번 변경 파일과 직접 관련 없는 주문 저장 parity 및 UI 계약 테스트 실패가 남아 있다. 셀 이동 전용 React/Rust 검증은 모두 PASS다.
6. PR #52는 열려 있으며 아직 main에 병합되지 않았다.

**완료 판정:** 조사 대상 셀의 공통 키 처리·IME 보호·이동·확정 게이트 코드는 구현했고 셀 이동 전용 자동 검증을 통과했다. Windows 실화면과 main 병합까지 완료했다고 판정하지 않는다.
