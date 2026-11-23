# 반도허브 80차 — 셀형 입력 전수조사 및 적용 보고서

**기준 저장소:** `ThunderStrikerKR/TSBH`  
**기준 브랜치:** `main`  
**조사 범위:** React/TypeScript 운영 화면과 Rust/egui 화면의 직접 편집형 셀  
**판정:** 공통 이동 계약·React 공통 키보드 핸들러·화면별 어댑터 적용 완료. Windows 실화면 검증 및 일부 편집 취소·undo 통합은 미완료.

## 1. 적용 대상으로 판정한 셀

| 화면 | 실제 셀 | 적용 위치 | 적용 내용 |
| --- | --- | --- | --- |
| 상품 관리 React | 도매가·반도매가·소매가 | `src/ProductManager.tsx`, `src/PriceCell.tsx` | 명시적 3열 좌표, 4방향 이동, Enter 같은 열 이동, Tab 순서, IME 보호 |
| 수동 분류 React | 수취인·상품·수량 | `src/orders/ManualClassification.tsx` | 공통 좌표 resolver, 기존 수량 확정 전 이동 제한 보존 |
| 주문 접수 React | 품목 수량 | `src/OrderDesk.tsx` | 수량 행 좌표, 방향키·Enter·Tab 이동 |
| 처리 중 주문 React | 실제 운임 | `src/WorkflowFooter.tsx` | 수취인 표 안에서 수량·운임 셀 간 명시적 이동, 기존 blur 저장 유지 |
| 상품 관리 egui | 도매가·반도매가·소매가 | `egui-app/src/product_manager.rs` | 공통 Rust resolver, 좌우 및 상하 좌표 이동, busy 셀 건너뛰기 |
| 수동 분류 egui | 선택된 수취인/상품 편집 | `egui-app/src/manual_workspace.rs` | 좌우 방향키 이동 및 IME 이벤트 보호 |
| 접수 egui | 수취인·품목 수량·단가·운임 | `egui-app/src/main.rs` | 명시적 행·열·순서 맵, 기존 inline 저장·전환 경로로 이동 |

## 2. 공통 구현

- React 공통 모듈: `src/ui/cell-navigation.ts` (`handleCellNavigationKey`가 IME·키 분류·경계·저장 콜백·포커스 이동을 공통 처리)
- Rust 공통 모듈: `egui-app/src/cell_navigation.rs`
- 계약 문서: `docs/cell-editing-contract.md`

두 UI 프레임워크가 단일 코드 파일을 공유할 수는 없으므로, 동일한 이동 규칙을 각각의 네이티브 구현으로 유지한다. 화면별 어댑터가 셀 좌표와 업무별 저장 동작을 소유하고 공통 resolver가 이동 대상만 계산한다.

## 3. 공통 대상에서 제외한 입력

다음은 표의 데이터 셀을 직접 편집하는 컨트롤이 아니므로 셀 이동을 강제하지 않는다.

- 검색 입력: `ManagementSearch`, 거래처·상품 검색 등
- 날짜 선택기 및 기간 필터
- 체크박스·라디오·선택형 드롭다운
- 상품 상세·거래처 상세·설정의 일반 폼
- OCR 원문, 자동 메시지, 관리 메모 등 장문 입력
- Excel 관리·주문 내역·일일 결산의 읽기 전용 표 셀
- 상품별 운임 설정 모달의 메모·묶음 수량 선택 등 일반 폼 필드

`ShippingFeeRanges`의 반복 운임 행은 표 셀과 다른 모달 폼 구조이므로 별도 어댑터 후보로 남긴다.

## 4. 보존한 업무 동작

- 상품 가격 셀의 기존 예상값 비교, 비동기 저장 큐, 저장 실패 초안 보존, 자동 blur 저장 및 Esc 취소를 유지했다.
- 수동 분류의 상품 선택 후 수량 미확정 상태에서는 이동을 막는 기존 확인 게이트를 유지했다.
- 실제 운임은 기존 pending 값과 blur/Enter 저장 경로를 유지했다.
- egui 접수는 `switch_inline_edit` / `submit_inline_edit`의 기존 저장·편집 전환 경로를 사용한다.
- 공통 resolver는 업무 계산·가격 결정·DB 저장을 수행하지 않는다.

## 5. 자동 검증 증거

| 검증 | 결과 | 증거 |
| --- | --- | --- |
| TypeScript `tsc --noEmit` | PASS | [Cell navigation contract run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45259377134) |
| React 공통 계약 테스트 | PASS, 8 tests | [Cell navigation contract run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257741678) |
| 가격 셀 기존 회귀 시나리오 | PASS | [Cell navigation contract run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257741678) |
| React 가격 저장·비동기 회귀 | PASS | [Cell navigation contract run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257741678) |
| egui 전체 `cargo check` | PASS | [egui contract run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257427206) |
| Rust 공통 이동 단위 테스트 | PASS, 4 tests | [egui contract run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257427206) |
| 수동 분류 모달·기존 입력 라우팅 회귀 | PASS | [Manual classification run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257427399) |
| 거래처 관리 UI 회귀 | PASS | [Client UI run](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45257427251) |

초기 React 검사에서는 기존 가격 셀 테스트에 삭제된 `findPriceCell` 구현을 기대하는 낡은 assertion이 남아 실패했다. 해당 assertion을 제거하고, 기존 전체 테스트 파일의 별도 미해결 `WorkingRawDialog.tsx` 경로 오류가 셀 작업을 가리지 않도록 가격 셀 테스트만 이름 필터로 격리했다. 이후 React contract workflow는 PASS했다.

## 6. 미완료·미검증

1. Windows 실제 화면에서 각 입력 유형의 키보드 이동, 포커스 복귀, IME 조합 및 저장 실패를 확인하지 않았다.
2. egui 입력 위젯의 키 이벤트 소비와 셀 경계 동작은 컴파일 검증만 통과했으며 실제 GUI 동작 검증은 남아 있다.
3. Ctrl/Cmd+Z는 공통화하지 않았다. 기존 화면의 실행 취소 동작만 유지한다.
4. 주문 접수 수량과 실제 운임의 Esc 취소·셀 단위 undo는 각 입력의 즉시 반영/blur 저장 구조를 고려한 별도 초안 정책이 필요하다.
5. React/egui 전체 입력 UI를 하나의 시각 컴포넌트로 교체한 것은 아니다. 이번 단계에서 공통화한 핵심은 셀 좌표·이동·IME 보호 계약과 화면별 어댑터다.

**완료 판정:** 공통 이동 엔진과 조사 대상 셀 어댑터의 코드·자동 테스트는 적용했다. 전체 엑셀 편집 UX 및 Windows 실화면까지 완료했다고 판정하지 않는다.
