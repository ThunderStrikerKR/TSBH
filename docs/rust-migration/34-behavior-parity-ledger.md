# 34차 Rust 이전 원본-행위 검증 누적표

> **과거 감사 이력:** 현행 고정 기준/검증 상태는 [AGENTS.md](../../AGENTS.md) 및 [누적 검증표](verification-ledger.json)를 따른다. 오래된 `packable=false` 복원 기록은 이후 승인된 `maxBundle` 규칙과 충돌하여 폐기되었으며 현재 재도입해서는 안 된다.

**상태: 진행 중 / 전체 동등성 검증 완료 아님**

## 고정 기준 및 검증 원칙

> **2026-11-17 정정:** 아래 '고정 기준'은 2026-11-06 초기 Tauri의 **역사적 참고 자료**이지, 현재 앱의 최종 사양이 아니다. 그 이후 사용자가 확정한 변경은 보존한다. 이 감사표의 '원본 복구' 기록은 실제 작업 이력이며, 후속 개선과 충돌하는지는 별도로 검증해야 한다. 자세한 기준은 [35차 기능감사 및 기준 충돌 정정](35-full-functional-parity-index.md).
>
> 단순히 과거 TypeScript와 다르다는 이유로 현재 Rust·React 기능을 삭제하거나 되돌리는 것을 금지한다.

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

## 34차 Windows 강화 검사에서 확인된 별도 실제 UI 결함 (2026-11-17)

- 대상 커밋 `50791c31`, [실행 43946579059](https://github.com/ThunderStrikerKR/TSBH/actions/runs/43946579059): **WIN_FAIL**.
- 실제 TEST SQLite/WebView2에서 가상소매점 확정 후 `84,000원` 정상 표시.
  4.2초/200ms 샘플 중 **3개 프레임에서 `확인 필요`로 사라졌다 복구**.
  오류 메시지 `판매가를 등록하세요`는 관찰되지 않아 실제 단가 소실과 다른 UI 표현 결함임.
- 원인: `src/OrderDesk.tsx`의 금액 표시가 개별 Rust `preview_order` 가격 유효성과 관계없는
  전체 `flowSummaryReady` (workflow + board + issues 모두 완료) 상태에 결합된 구조.
- 교정: Rust가 산출한 최신 행별 금액만을 보존하고, 상품·옵션·수량·단가·거래처가
  동일한 동안 비관련 Rust 응답 대기 시 기존 금액 유지. 금액 입력이 바뀌면
  옛 값을 재사용하지 않고 `계산 중…` 표시; React 금액 산식 추가 금지.
- **이번 표시 교정은 PATCH_COMMITTED / 새 Windows 4.2초 검증 결과 확인 전 PASS 금지.**
- CI `react-native-reception` 직전에 원본 SHA 고정 seal 테스트를 항상 실행하도록 게이트 추가.

## 기존 프런트엔드 회귀 검사 최신화 (2026-11-17)

- 32차에서 의도적으로 삭제한 `BUILD_WINDOWS_TAURI.cmd` 및 `BUILD_WINDOWS_EXE.cmd`를
  옛 테스트가 여전히 로드해 발생하던 `ENOENT`는 실제로 남아 있는 수동 GitHub Actions
  Release+OCR ZIP 경로를 검증하도록 테스트 계약을 수정함. 삭제 파일은 복구하지 않음.
- 기존 React stale guard의 비교는 현재 `order.id`·`order.raw`·원문 text 비교,
  `sameValue(previous,sourceDraft)` 의미상 동일성 비교를 실제로 검사하도록 수정.
- 다만 `PACKAGE_WINDOWS_TAURI.cmd`의 누락된 CMD 참조는 **OPEN**,
  테스트에 TODO와 본 누적표로 명시. 기능 복구 없이 배포 경로 완료 판정 금지.
- 이번 테스트 변경은 동작 코드와 기존 TypeScript oracle을 변경하지 않음.

## R34-009: 가격 표시 임시 보완 기능 제거 및 고정 TypeScript 원본 화면 복원 (2026-11-17)

- 기준: `30a19cd2da7533dd2ed36b807593309bde70a7eb:src/OrderDesk.tsx` 원본.
  원본은 `unitPrice !== null && quantity !== null && productId && variantId`인 경우
  `won(unitPrice * quantity)`를 렌더링하고, 그 외에만 `확인 필요`를 보여줌.
- Rust 단가 업무 판단은 `pricing.rs`, `order_edit.rs` 그대로 유지. React UI의
  단순 수량 곱셈 표시도 위 고정 원본과 정확히 같도록 복원.
- 새로 덧대었던 `lastNativeLineAmounts` 캐시, `linePriceInputKey`,
  `visibleLineAmounts`, `계산 중…`, 행별 가격의 `flowSummaryReady`
  의존성을 모두 삭제. 정상 단가가 있으면 unrelated preview 응답에 가려지지 않음.
- `tests/tauri-reception-price-display-stability.test.mjs`의 기존 '캐시가 존재해야 PASS'
  계약을 없애고 고정 TS 원본 방식의 표시 분기를 검증하도록 교체.
- **상태: PATCH_COMMITTED.** Rust·프런트엔드 CI와 실 Windows 4.2초 관찰
  결과는 별도로 확인한 뒤 검증 완료로 갱신할 것.

## R35-001/002: 운임 읽기와 직원 검토 중간 상태 교정 (2026-11-17)

- `src/useNativeShippingRead.ts`→`rules/shipping_read.rs`는 `products`와 `shipping`
  데이터만 소비함. 기존 hook은 원본 전체 `state` 참조가 바뀌면 이미 검증된 운임 기록을
  지우고 `운임 기준 조회 중`으로 되돌림. 의존성을 정확한 입력 두 가지로 축소.
- `src/ProductFreightPanel.tsx`→`rules/shipping.rs::product_freight_view`도
  `products`/`shipping`만 소비하므로 동일하게 축소. 관계없는 주문·알림 변경으로
  등록 운임이 일시적으로 '확인 중'으로 되돌아가는 중간 상태 제거.
- `src/StaffChecks.tsx`: Rust 검사 결과를 기다리는 동안 빈 배열을 곧바로
  '필수 확인 사항 없음'으로 선언하던 분기를 수정. 완료/정상 상태일 때만 표시.
- 신규 `tests/tauri-shipping-loading-parity.test.mjs`는 위 로직 회귀를 고정.
- 실제 가격·운임 규칙은 Rust만 결정. 상태 갱신 트리거 외 계산 규칙은 변경하지 않음.
- 상태 **PATCH_COMMITTED**. CI / 실제 Windows 사용자 행동 결과는 별도 확인.

## R35-003: 실 Windows 검사로 발견한 배송 건수·부가세 로딩 회귀 교정

- [실패 확인 43954978288](https://github.com/ThunderStrikerKR/TSBH/actions/runs/43954978288):
  실제 Windows DEV WebView2에서 4.2초 동안 상품 `84,000원`은 유지했으나
  4개 샘플에서 `택배 2건 → —건 → 2건`, `부가세 별도 → — → 부가세 별도`가 발생.
- 원인: `useNativeOrderPreview.ts`는 Rust 정산 응답을 `fingerprint`가 일시적으로
  달라졌다는 이유로 화면에서 삭제. 아직 계산된 값을 드래프트 상태로 옮기지 않았음.
- 수정: `useNativeOrderPreview`의 **기존 마지막 Rust 결과**에서, 원본 주문의
  청구 입력(상품/수량/단가/운임/부가세·거래처)과 실제 저장 상태의 관련 원천이
  그대로인 동안만 읽기 전용 금액·배송건수·부가세 요약을 표시함.
  금액/운임/부가세/업무 상태가 바뀌면 키를 갱신하여 이전 금액 사용을 금지.
  Rust가 최신 상태를 확정해야 실제 업무 버튼은 활성화되며 React 계산기 추가 없음.
- 이전 패치의 서로 다른 UI `lastNativeLineAmounts` 캐시와는 달리,
  주문에 직접 없는 Rust의 총액/부가세 산출을 잃지 않기 위한 hook 내부의
  **기존 resolved 상태 재사용**이며 추가 중복 캐시는 만들지 않음.
- 상태: PATCH_COMMITTED / 새 Windows 테스트·CI 검증 전 완료 판정 금지.

## R35-004: 주문 이력·결산·공장 회신 상태의 불필요한 재조회 방지

- `OrderHistory`/`DailySettlement`는 Rust가 사용하는 주문/고객/상품/운임/설정/워크플로/회신
  원천이 변경될 때만 결과를 갱신. 관계없는 다른 앱 상태 변경으로 표시된 데이터를 지우지 않음.
- `factory_reply_review_statuses`는 `factoryReplies`만 읽으므로,
  `FactoryReplyHistory`/`FactoryNotifications`는 해당 원천만 변경될 때 다시 Rust 호출.
  기존 '반영 완료'가 다른 업무 갱신 때문에 '확인 중'으로 돌아가던 표시를 방지.
- 검증 단계 PATCH_COMMITTED / 최신 CI와 Windows 실제 동작 재확인 필요.
