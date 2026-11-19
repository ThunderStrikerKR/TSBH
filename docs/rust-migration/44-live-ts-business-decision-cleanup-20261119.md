> **2026-11-19 현행 안내:** 이 문서는 작성 당시의 역사적 이관 기준과 미완료 판단을 보존한 자료입니다. 현재 `main`의 소스 이동, 운영 진입점, 실제 작업 목록은 [46차 현행화 문서](./46-current-live-source-cleanup-20261119.md) 및 [verification-ledger.json](./verification-ledger.json)의 `activeUnresolved`를 우선합니다. 아래 `src/parser.ts`, `src/validation.ts`, `src/workflow.ts` 등 구형 위치는 현재 테스트 오라클 디렉터리로 이전되었습니다.\n\n# 44차 — 실제 운영 경로의 TypeScript 업무 판단 정리 (2026-11-19)

기준: `main`의 `src/main.tsx`에서 도달하는 React/Tauri 업무 경로. 파일 존재, 이름, 구형 테스트 import만으로 운영 중이라고 판정하지 않는다. 승인 이후 변경된 동작을 과거 TypeScript 오라클에 맞춰 되돌리지 않는다.

## 이번에 실제 제거한 중복 판단

| 운영 경로 | 이전 TypeScript 판단 | 변경 후 권한 | 근거 커밋 |
|---|---|---|---|
| `FactoryReplyWatcher → processWatchedFactoryReply` | `factoryReplies.some(record => record.id === source.id)`로 중복 파일을 미리 판정 | `rules.factory_reply_duplicate`가 Rust `factory_reply.rs::is_duplicate`를 호출한다. 같은 helper를 Rust 회신 검토에서도 사용한다. 파일 감시와 SQLite 최종 커밋은 각각 기존 중복 방지를 유지한다 | `e3563534`, 격리 테스트용 지연 import 보정 `8e23677a` |
| `readFactoryReplyCells → nativeAnalyzeFactoryReply` | 숫자형 Q열 운송장 `numericRisk`, 앞자리 0 손실, 15자리 초과 경고의 확정 | TS는 원본 숫자 여부·Excel 서식(`trackingNumberFormat`)만 수집한다. Rust `rules.factory_reply.analyze`가 `zero_padded`·`numeric_risk`를 판단한다 | `de765cc6`, 정적 계약 수정 `a3463711` |
| `useOrderDesk` 렌더 | 사용하지도 않는 `selectedItems → normalizeColor → colors` 공통 색상 가능 여부 계산 | 소비자가 없는 계산과 미사용 `selected` 상태를 모두 제거. 실제 상품 생성/가격은 기존 Rust `create_catalog_item`을 유지 | `cbb92767`, 정적 테스트 정정 `3ee56cff` |

| `useOrderDesk → recognition.analyze_order` | 접수 화면이 가상 `catalog === DEMO` 또는 `variant.source === DEMO` 상품을 인식 후보에서 먼저 제외 | `recognition/mod.rs`의 `analyze_order` 요청 진입점이 원본 상품 목록을 접수 대상 기준으로 필터링한다. `match_products` 및 과거 테스트 전용 인식은 임의 변경하지 않는다 | `a2e2cb97`, 테스트 보강 `2961c089` |
| `useOrderDesk → rules.reprice_order` | Rust 단가 갱신 응답을 받은 뒤 `transferred` 또는 `step >= 3` 수취인 행을 TS가 옛 값으로 복구 | Rust `pricing::reprice_order`가 수취인 단위 동결을 판단한다. 명시적 가격 편집(`explicitEdit`)과 수동 출고 전환은 별도로 유지한다 | `60f6d55a` |

위 다섯 경로에 Rust 실패 시 구형 TypeScript 업무 규칙으로 대체하는 코드는 추가하지 않았다. 타임스탬프, 원본 행, 화면 초안, 파일 바이트의 운반은 프런트에 유지한다.

## 아직 운영 소스에 남은 판단 / 분류

1. **`order-types.ts::orderParty`, `workflow-view.ts::shipmentContext`**: 조회 화면의 거래처 스냅샷 선택. Rust `workflow::party`와 규칙이 겹치나, 프런트에서는 주문표시·검색·선택에 사용한다. 미완료. 단순 TS 파일 삭제로 처리하면 안 된다.
2. **`actual-freight.ts::actualFreightOf`**: 실제 출고 운임의 저장된 값(배송 건 우선, 수취인 이전값 보조)을 `ActualFreightField`와 `WorkflowQueue`가 동기적으로 표시/초안 비교한다. **저장 결정·이력은 Rust `record_actual_freight`가 담당**한다. 비동기 전환 시 화면 초안 손실 위험이 있어 미수정.
3. **`domain.ts::orderClient` 및 `useOrderDesk`의 `liveClient` 선택**: 화면·가격 인자를 위한 거래처 선택. `reprice_order`·`create_catalog_item` 계산 자체는 Rust지만 인자 선택은 TS 경로에 있다. 최우선으로 추가 추적할 경계다.
4. **`automatic-message-templates.ts`**: 편집기 샘플 미리보기. 저장 시 형식/필수 변수 검사는 Rust `validate_automatic_templates`. 프레젠테이션으로 분류.
5. **`client-workbook.ts`, `product-unified-workbook.ts`, `product-registration-xlsx.ts`, `factory-export.ts`, `store-export.ts`**: ExcelJS 파일 입출력/서식. 상품·거래처·출고 의미 판정은 Rust. 구조적 셀 읽기/파일 형식 점검은 프런트에 허용한다.
6. **`domain.ts`의 구형 `totals`, `resolvePrice`, `recommendAccount` 등**: 테스트에서 보존한 역사적 원본과 프로덕션 소스가 혼재한다. 함수 호출 그래프에서 운영 호출이 증명되지 않은 부분을 현재 버그로 집계하지 않는다.

**운영 TS 업무 판단 0건 판정은 아직 불가**. 앞의 3개 조회·선택 경계를 확정 제거/재분류한 후 최종 판정해야 한다. 43차 문서의 `LIVE_TS_BUSINESS_DEPENDENCY`는 모듈 도달 개수로, 실제 업무 알고리즘 실행 개수와 다르다.

## 검증 상태와 실패 해석

- 새 Rust 중복 사전판정과 Q열 숫자형 운송장 위험 분기 단위 테스트를 추가했다. 실수로 도입한 격리 테스트 import 오류와 정적 정규식 오류는 후속 커밋에서 수정했다.
- GitHub Actions의 **Native Rust output and integrated dispatcher** 검사는 `a3463711`, `cbb92767` 시점에 성공했다. 실제 Windows 실행/GUI 재실행을 뜻하지는 않는다.
- `cbb92767` 시점 전체 프런트엔드 테스트 **785건 중 24건 실패**. 여기에 이번에 추가한 정적 테스트 파일의 정규식 문법 오류가 1건 포함되어 있었으며 `3ee56cff`에서 수정했다. 최신 커밋의 전체 재실행 결과를 별도로 확인해야 한다.
- `a2e2cb97`과 `60f6d55a`에서 Rust 발주서 DEMO 제외·동결 수취인 가격 재산정의 단위 테스트를 추가했다. 새 커밋의 Rust CI는 재실행 결과를 확인하기 전까지 통과로 선언하지 않는다.
- 기존 비교 테스트는 `4/94` 가격/운임/갱신 오라클 불일치, `3/27` 주문 저장 오라클 불일치(`canQueue is not a function` 등)로 실패한다. 이는 이 이관 변경의 실제 업무 오류를 입증하지 못한다. 승인된 최신 Rust 동작과 실제 UI 동작을 기준으로 개별 증거를 먼저 확보한다.
- Windows GUI, 출고/입금/공장 회신 실사용 테스트 완료는 주장하지 않는다. 필요한 데이터 보존과 초안 입력 유지 검증을 별도로 완료해야 한다.

## 계속 적용할 차단 조건

- 실제 운영 경로의 업무 **판정**과 프런트 화면/입출력의 **표시**를 나누어 기록한다.
- Rust 호출 실패 시 TS 업무 판정으로 우회하지 않는다.
- 이미 Rust에 이전된 기능을 임의로 재구현하거나, 과거 테스트만 맞추기 위해 승인된 행동을 되돌리지 않는다.
- 검색/표시용 동기 조회를 비동기화할 때 화면 깜빡임·초안 손실·중복 IPC가 없는지 검증한 뒤 적용한다.
