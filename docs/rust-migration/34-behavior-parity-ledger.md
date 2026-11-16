# 34차 Rust 이전 원본-행위 검증 누적표

**상태: 진행 중 / 전체 동등성 검증 완료 아님**

## 고정 기준 및 검증 원칙

- 기존 TypeScript 고정 기준 SHA: `30a19cd2da7533dd2ed36b807593309bde70a7eb` (백업 브랜치 이름으로 재조회하지 않고 이 커밋을 사용)
- 첫 버그 수정: `4056ffc03ef0b984d0e34af69ca90fda315d759e`
- 현재 34차 원본 기준 복구: `5cc9d515546b430194c51473585e7b2bb7cbd1d3`
- 별도 브랜치 생성 없이 `main`에서 수정. **테스트·CI·실제 Windows 결과는 별개.**
- 원본 가격 스냅샷: `tests/rust-migration/recognition/original-price-20261116.ts`. `src/domain.ts::resolvePrice`의 고정 SHA 함수 본문.
- 원본 운임 스냅샷: `tests/rust-migration/recognition/original-shipping-20261116.ts`. type-only import 경로 외 원본 Git blob SHA `ee60df8101acf71e298fd289aabd593e85c84b21` 그대로.
- 테스트 기준 파일은 Rust의 출력을 모방해 바꾸지 않는다. `original-oracle-seal.test.mjs`에서 원본 SHA·가격 본문·null 초기화 금지를 검사한다.
- 다음 감사에서는 **미검증/Open부터** 검사. 이전 검사에서 검증되지 않은 것을 PASS로 요약하지 않는다.
- 의도된 동작 변경은 사용자 승인 근거·커밋·원본과의 차이를 별도 기재한다.

## 상태 체계

`FOUND_DIFF` = 고정 원본과 명확한 불일치,
`PATCH_COMMITTED` = 수정 저장소 반영(검증 통과와 무관),
`DIFF_PASS` = 실제 Rust 결과와 고정 TS oracle 동일 입력 대조 성공,
`ACTION_PASS` = 복수 사용자 행동·응답 역전 순서 성공,
`WIN_PASS` = 해당 커밋 실제 Windows Tauri/WebView2와 SQLite에서 재현 성공,
`OPEN` = 전수 또는 동작 미확인.

## 누적 검증·미검증 범위

| ID | 사용자 동작 | 원본 TypeScript 함수 | 실제 Rust 및 React 경로 | 발견/판정 | 상태 |
| --- | --- | --- | --- | --- | --- |
| R34-001 | 원문 붙여넣기→거래처 확정→2~3초 후 상품 단가 | `src/orders/useOrderDesk.tsx` 병합 Effect에서 `resolvePrice()` 즉시 적용 | `recognition/analyze_order`, `rules/reception_merge.rs`, `src/orders/useOrderDesk.tsx` | Rust 병합에서 기존 단가 `null`로 지우고 나중에 복구 시도 | PATCH_COMMITTED (`4056ffc0`), 실제 Windows·타이밍 미검증 |
| R34-002 | 같은 순서에서 운임 및 운임 버전 | 같은 TypeScript Effect 말미 `recommendShipping(next,state)`와 운임 면제 | `rules/reception_merge.rs`에서 `shipping::recommend` | 종전 Rust가 `shippingFee=null`/`shippingVersion=""`을 반환함 | PATCH_COMMITTED (`5cc9d515`), 실제 Windows 미검증 |
| R34-003 | 합포장 불가(`packable=false`) 물품 2개 운임 | 고정 `src/shipping.ts::shippingBundles` | `rules/shipping.rs::recommend` | 합포장 불가 직원 확인 조건이 Rust 및 이후 TS 사본에서 사라짐 | PATCH_COMMITTED (`5cc9d515`), 실제 차등·Windows 미검증 |
| R34-004 | Rust 병합 차등 테스트 자체 | 고정 TS 가격+운임 즉시 계산 | `tests/rust-migration/recognition/ts-reception-merge-oracle.ts` | 기존 이름만 원본인 oracle이 Rust의 null/빈 버전을 모방 | PATCH_COMMITTED, 고정 스냅샷·seal 테스트 추가 |
| R34-005 | 수동 분류 후 원문 수정 | 기존 TS는 원문 동일 시에만 재분석 차단 | `src/orders/rust-reception.ts`, `src/orders/useOrderDesk.tsx`, `rules/reception_merge.rs` | 기존 Rust/React가 수동 분류 존재 자체로 차단하는 원본 불일치 | PATCH_COMMITTED / CI·Windows 재검증 필요 |
| R34-006 | 수량·수취인·운임 수정 중 가격 유지 | 원본 React 가격 Effect 의존성은 상품 마스터·거래처 | 현재 React Effect는 `draft.recipients`도 의존 | 실행 트리거 차이. 중간 단가 덮어쓰기 여부 미재현 | OPEN |
| R34-007 | 가격 편집·주문 미리보기·저장 지속성 | 고정 TS 입력→주문 변경→저장 흐름 | `PriceCell`, `useNativeOrderPreview`, `reconcile_workflow`, SQLite | 실제 UI 지연 응답 시각·최종 저장 상태 확인 필요 | OPEN |
| R34-008 | 공장 전달→회신→이력·결산 | 고정 TS 업무 흐름 | Rust `factory_reply`, `workflow`, `output` 및 React | 개별 함수 차등 테스트만으로 전체 사용자 행동 체인 검증 불가 | OPEN |

## 실행·증거 게이트

1. **원본 고정**: 기준 SHA 및 바이트가 확인된 원본 oracle, 동일 입력 JSON, Rust 실제 바이너리 결과와 오류 메시지를 비교.
2. **단계별 상태**: 원문 입력 / 거래처 확정 / 지연된 분석 결과 / 주문 미리보기 / 수취인·상품·운임 수정 / 저장 전후 상태 비교.
3. **응답 순서 역전**: 먼저 보낸 분석·가격·운임 응답이 나중에 도착해도 최신 확정 값을 덮지 않는지 비교.
4. **물리 환경**: 해당 SHA의 Windows Tauri/WebView2 GUI + 테스트 SQLite 및 재실행 검증. 화면 캡처·콘솔·DB 상태와 함께 기록.
5. **결과 관리**: 코드 커밋, 단위 테스트, TS↔Rust 실제 차등 테스트, CI, Windows 실측은 각각 별도로 기록. 어느 하나로 전체 완료 판정 금지.

## 권장 검증 명령

```sh
node --test tests/rust-migration/recognition/original-oracle-seal.test.mjs
node --test tests/rust-migration/recognition/react-reception.test.mjs
cargo test --manifest-path tests/rust-migration/output/Cargo.toml
cargo test --manifest-path tests/rust-migration/integration/Cargo.toml
```

현재 문서 생성 시점은 **전체 CI 성공 또는 실제 Windows GUI PASS를 의미하지 않음**.

## 후속: 수동 분류 재분석 동작 및 실제 Rust 업무 시뮬레이션

- 원본 TS에서 원문이 바뀐 수동 분류 주문 재분석 허용을 Rust와 React guard에 동일 적용.
- 인식→Rust merge 차등 시나리오에 수동 분류가 있고 원문이 변경된 주문을 추가.
- `zz-workflow-31-simulation.test.mjs`의 `merge_reception_analysis` 입력에 새 필수 `state`/`client`를 추가함.
- 통과 판정은 이번 수정이 실제 완료된 CI에서 검증된 후에만 가능.

## Windows 사용자 행동 테스트 강화 (2026-11-17)

- `scripts/migration32-dev-business-ui.mjs`: 실제 Windows Tauri DEV + WebView2 + TEST SQLite에서
  발주 원문 붙여넣기 → 소매 거래처 직접 확정 → 정상 84,000원 표시까지 진행한 다음
  **4.2초간 200ms 간격으로 가격·오류 목록을 샘플링**해 지연된 Rust 인식/병합으로
  정상 가격이 사라지거나 직원 확인 오류가 다시 생기면 즉시 실패 처리함.
- 위 검증은 실제 `DEV_WINDOWS_TAURI.cmd` 스모크 워크플로를 통해 실행되며
  **CI 결과가 완료되기 전에는 ACTION_PASS / WIN_PASS로 표시하지 않음**.
- 사용자가 의도적으로 삭제한 `BUILD_WINDOWS_TAURI.cmd`는 복구하지 않음.
  남아 있는 `PACKAGE_WINDOWS_TAURI.cmd`가 이 삭제 파일을 호출하는 별도 빌드 경로 결함은
  운임/주문 Rust 이전과 분리해 **OPEN** 상태로 추적.
