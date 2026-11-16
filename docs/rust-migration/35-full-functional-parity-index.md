# 35차 고정 TypeScript 원본 대비 전체 기능군 감사

**주의: 이 문서의 2026-11-06 초기 TypeScript는 현재 사양이 아니다.** 2026-11-17 기준 이전 소스를 최종 정답으로 취급한 과거 감사 원칙은 폐기한다.
고정 *역사적 비교 자료* SHA: `30a19cd2da7533dd2ed36b807593309bde70a7eb` (Git 최초 커밋 2026-11-06 12:22:34 UTC; 당시 Tauri `package.json`은 `0.0.4`). 사용자가 언급한 Electron `0.8.10`과 해당 Tauri SHA가 동일 소스라는 근거는 아직 없다.
전수조사 목적은 기존 기능 퇴행을 막고, 최신 사용자 승인 개선을 보호하며, Rust 이식 중 **승인 없이 추가된** 동작만 근거를 확인하여 교정하는 것이다. **전체 동등성 PASS 아님.**

## 기준 충돌 정정: 역사적 TypeScript와 이후 승인된 개선

1. **최신 명시적 사용자 요구·승인된 변경이 가장 우선이다.** 11월 6일에 없는 기능이라고 해서 삭제하지 않는다.
2. **현행 운영 기능 중 승인 출처가 확인되지 않은 개선은 보류·검증한다.** 그것이 새 기능이라는 이유만으로 제거하거나, 최신 커밋에 있다는 이유만으로 무조건 승인했다고 간주하지 않는다.
3. **11월 6일 원본은 변경하지 않기로 한 기존 기능의 동작 비교에만 사용한다.** 이 경우에도 현재 의도된 변경이 없는지 먼저 확인한다.
4. 원본과 현행 동작이 다르면 `UNCLASSIFIED_DIFF`로 기록하고 **수정 전에** 승인된 개선인지 / 이식 중 회귀인지 / 확인되지 않은 추가 기능인지 분류한다.
5. 품목/운임/부가세뿐 아니라 UX, 화면 구성, 자동화, 저장 방식도 개선된 사용자 시나리오를 먼저 보존한다. 과거 TypeScript와 같게 만들기 위한 소스 덮어쓰기 금지.
6. 개선 여부는 이전 채팅의 확정 사항·명시된 UI 계약·동일 입력 전후 상태·Windows 시나리오·Git 변경 기록으로 뒷받침한다. **현재 TS 원본을 가져온 테스트는 승인 근거가 아니다.**

이 감사의 `CHANGED_SINCE_BASELINE`은 단지 2026-11-06과 파일 내용이 다르다는 뜻이며, **결함·복구 대상·삭제 허가가 아니다.**

2026-11-17 소스 트리 기준: 원본 TS 121개 / 현재 TS 189개 / 원본에서 변경 또는 추가된 TS 152개 / Rust 업무 모듈 60개 / 이식 테스트 및 도우미 39개.
파일수 비교는 기능 동등성 판정이 아니라 검토 대상 전체 크기다.

## 28개 기능군과 검증 현황

| ID | 기능군 | 고정 원본 위치 | 현재 Rust/React 실제 실행 또는 화면 경로 | 판정 |
| --- | --- | --- | --- | --- |
| F01 | 발주서 원문 분석·수량 인식 | parser.ts | recognition/mod.rs | PARTIAL |
| F02 | 수취인·주소·기관·연락처 귀속 | order-evidence.ts, parser.ts | recognition/model.rs | PARTIAL |
| F03 | 상품·별칭·옵션·대표 모델군 | matching.ts, model-families.ts | recognition, catalog_clients/aliases.rs, model_groups.rs | OPEN |
| F04 | 재분석 병합·사용자 수정 보존 | orders/useOrderDesk.tsx | rules/reception_merge.rs | PARTIAL |
| F05 | 거래처 자동연결·카카오톡 제목 | title-clients.ts, SourceDocument.tsx | catalog_clients/title_clients.rs | PARTIAL |
| F06 | 수동 분류·수량 잠정 승인 | orders/manual-classification.ts, validation.ts | rules/manual_*.rs | PARTIAL |
| F07 | 거래처 등급·예외 가격 | domain.ts::resolvePrice | rules/pricing.rs | FROZEN_PRICE_ORACLE_PARTIAL |
| F08 | 상품 변경·가격 입력·저장 | OrderDesk.tsx, domain.ts | rules/catalog_item.rs, order_edit.rs, PriceCell | PARTIAL |
| F09 | VAT·총액·배송 건수 | domain.ts, workflow.ts | rules/workflow_preview.rs | WINDOWS_22_FRAMES_PASS |
| F10 | 상품/그룹 운임 추천·합포장 | shipping.ts | rules/shipping.rs, shipping_read.rs | FROZEN_SHIPPING_ORACLE_PARTIAL |
| F11 | 운임 수동 입력·선불/착불 | shipping.ts, workflow.ts | WorkflowFooter, rules/shipping.rs | WINDOWS_CASES_PARTIAL |
| F12 | 발주 원문·수취인·품목 정정 | domain.ts, corrections.ts | rules/order_edit.rs, raw_corrections.rs | PARTIAL |
| F13 | 업무 단계·입금·공장 전달 | workflow.ts, replies.ts | rules/workflow.rs, order_save.rs | PARTIAL |
| F14 | 거래처 자동 안내·입금 복사 | automatic-message.ts, hub-messages.ts | output/automatic_full.rs, automatic_request.rs | ORACLE_REVIEW_OPEN |
| F15 | 공장/매장 Excel 출력 | factory-export.ts, store-export.ts | output/export.rs | ORACLE_REVIEW_OPEN |
| F16 | 공장 Excel 회신·송장 매칭 | factory-replies.ts, workbook code | rules/factory_reply.rs | ORACLE_REVIEW_OPEN |
| F17 | 정정·충돌 승인·차액 기록 | corrections.ts, factory-conflict-review.ts | rules/correction_rules.rs, factory_reply.rs | PARTIAL |
| F18 | 주문·배송·송장 이력 | app/order-history.ts, workflow.ts | rules/history.rs, OrderHistory.tsx | PARTIAL |
| F19 | 일일 결산·미수·실제 입금 | sales.ts, workflow.ts | output/settlement.rs, DailySettlement.tsx | PARTIAL |
| F20 | 거래처 등록·엑셀 일괄입력 | domain.ts, client-workbook.ts | catalog_clients/registration.rs, client_preview.rs | PARTIAL |
| F21 | 상품 등록·동기화·가격표 | product-models.ts, product-registry.ts | catalog_clients/editor.rs, product_sync_preview.rs | ORACLE_REVIEW_OPEN |
| F22 | 브랜드·카테고리·상품군 | category-tree.ts, model-families.ts | catalog_clients/categories.rs, brands.rs, model_groups.rs | ORACLE_REVIEW_OPEN |
| F23 | 과거 단가·기존 주문 스냅샷 | replies.ts, workflow.ts | rules/history.rs, reply_rules.rs, SQLite | PARTIAL |
| F24 | OCR 캡처·취소·교체 | SourceDocument.tsx | src-tauri/src/ocr.rs | WINDOWS_CANCEL_PASS_OTHER_OPEN |
| F25 | 공장 알림·회신 검토 | FactoryNotifications.tsx | rules/factory_reply.rs, SQLite notifications | PARTIAL |
| F26 | 계좌·환경설정·백업·복원 | SettingsPanel.tsx, app/useAppModel.ts | Tauri SQLite/settings/backup | OPEN |
| F27 | UI 탭·모달·엑셀 폴더·윈도우 배치 | React UI | Tauri Windows IPC | OPEN |
| F28 | DEV/Release·배포 빌드 | original release scripts | DEV_WINDOWS_TAURI.cmd, Actions | DEV_PASS_RELEASE_OPEN |

## 확정 발견 결함과 교정

- 상품 가격 일시 소실: 접수 병합에서 null 가격을 게시하고 비동기 갱신에 의존하던 원본 불일치 수정.
- 운임 null 초기화 및 합포장 불가 상품 예외 규칙 누락: 고정 shipping.ts 기준 복원.
- 주문 총액·배송 건수·부가세: 관련 없는 Rust 업무 미리보기 로딩과 금액 표시를 분리.
- 상품별/그룹별 운임 조회: 다른 앱 상태 변경 때 성공한 운임 표시를 초기화하지 않도록 변경.
- 직원 검토: Rust 이슈 조회 중 '확인 사항 없음'이라는 허위 완료 문구 표시 방지.
- 주문 이력/일일 결산 및 공장 회신: 관련 없는 상태 변경 시 기존 Rust 조회 결과 소실 방지.
- 테스트 자체 문제: 이전 후 현재 src/*.ts를 '원본'으로 가져오는 다수 차등 테스트 존재.

## 검증된 Windows 시나리오

- 9ac264fd: 실제 Tauri Windows DEV / WebView2 / TEST SQLite 29개 업무 점검 PASS.
- 9ac264fd: 4.2초 동안 22회 관찰, 84,000원 / 배송 2건 / 부가세 별도 누락 0프레임.
- 실행 URL: https://github.com/ThunderStrikerKR/TSBH/actions/runs/43956556686
- 관련 CLI/차등 PASS: https://github.com/ThunderStrikerKR/TSBH/actions/runs/43956556588

## 차등 테스트의 신뢰성 문제 및 자동화

- src/domain.ts::resolvePrice와 src/shipping.ts는 기존 pinned original-price-20261116.ts와 original-shipping-20261116.ts 및 blob seal 테스트가 있음.
- 그 밖의 비교 테스트 다수는 tests/rust-migration 아래에서 현재 src/*.ts를 tsImport하므로 원본 시절 입력·출력의 증거로 자동 승격 금지.
- CI의 original-ts-source-coverage job은 원본 git blob SHA와 현재 blob SHA를 비교하여 BYTE_IDENTICAL, CHANGED_SINCE_BASELINE, ABSENT_FROM_BASELINE으로 구별.
- 생성하는 migration-audit/original-ts-coverage.json 및 .md는 **코드 참조 차이 목록이지 실제 업무 기능 차등 테스트의 PASS가 아님**.
- 고정 원본에 없는 모델군/수동 가격/상품 엑셀 관련 모듈은 이 SHA만으로 '기존 TS 원본'이라고 간주하지 않음.

## 다음 검증 시 필수 미완료

- 각 기능군의 원본 함수/상태 변이/에러/출력과 실제 Rust operation을 같은 입력으로 실행해 바이트/정규화 JSON 비교.
- 수동 편집 중 거래처 변경, 상품 교체, 견적/부가세 수정, 거래처 등급 변경, 비동기 호출 역전 시나리오.
- 공장 회신 충돌, 송장 선행 0 보존, Excel 왕복, 이력/결산 보존, 백업/복구, DEV/Release 구분 실기기 확인.
- PACKAGE_WINDOWS_TAURI.cmd가 삭제한 BUILD_WINDOWS_TAURI.cmd를 참조하는 배포 경로 결함 OPEN.
- 코드 커밋·원본 차등 PASS·프런트 CI PASS·Windows PASS를 각각 별도로 기록; 하나로 전체 PASS 금지.

## 누적 감사 원칙

- 오래된 검사 완료 요약은 증거가 아니라 검색 단서로만 쓴다.
- 다음 세션은 source coverage JSON의 `CHANGED_SINCE_BASELINE`을 **수정 목록이 아니라 변경 분류 후보**로 사용한다. `OPEN`/승인 확인 대기 기능을 먼저 분류하고 고정 원본과의 차이만으로 수정하지 않는다.
- 원본과 다른 동작을 발견하면 **기존 기능의 수정 이력·사용자 승인 근거·현재 업무 결과를 우선 대조한다**. 고의로 개선된 행동은 유지하고, 근거 없는 이식 회귀만 교정한다.
- 회귀로 분류한 코드 수정 전후에는 새 기능의 사용자 행동 테스트를 함께 실행해 퇴행을 차단한다.

## 역사적 원본의 기계적 복원으로 인한 퇴행 위험 재감사 (2026-11-17)

- `4056ffc0` (`reception_merge.rs` 원본 TS 기준 즉시 재단가), `5cc9d515` (운임 즉시 재계산 및 `packable=false`), `98e97d5a` (`OrderDesk.tsx` 행별 금액 원본 표시식 복원)은 **실제 수정된 커밋**이다.
- 이 수정은 당시 발견한 상태 초기화/깜빡임 결함을 막기 위해 이루어졌지만, **11월 6일 이후 승인된 개선과 충돌하지 않는다는 포괄 검증은 수행되지 않았다**. 해당 영역을 `REGRESSION_RISK_REVIEW`로 표시한다.
- `9ac264fd`의 Windows DEV 29개 업무 테스트 및 22프레임 안정성 PASS는 **그 테스트 시나리오의 PASS**이지, 11월 6일 이후 모든 개선의 보존을 증명하지 않는다.
- 위 세 커밋을 무턱대고 되돌리는 행위 역시 최신 수정 사항을 훼손할 수 있으므로 **원본·현재 동작·최신 사용자 확정 요구를 대조하기 전 코드를 일괄 롤백하지 않는다**.
- 거래처·상품 모델군·카카오톡·운임 정책·접수 단계·공장 연동·알림 등 새롭게 추가 또는 개선된 영역은 최신 사용자 요구와 실제 실행 경로를 대조하기 전 `LEGACY_PARITY_PASS`를 부여하지 않는다.
- 과거 버전(0.8.10)의 일치성은 원본을 복제해야 한다는 뜻이 아니라, 변화가 의도되었는지를 확인하는 **역사적 차등 추적**이다.

## 36차 실측 확인: 초기 Tauri를 정답으로 삼아 생긴 회귀 교정

- **확인된 실제 역행:** `5cc9d515`에서 이전 `packable=false` 운임 차단을 되살림. 11월 10일 `f68c2747`/관련 검증에서는 `maxBundle`만으로 합포장 여부를 결정하도록 이미 개선되어 있었음. Rust 가드를 제거하고 해당 계약을 검증하는 테스트를 추가함.
- **확인된 실제 역행:** `98e97d5a`에서 React에 `unitPrice * quantity`를 다시 도입. 11월 13일 `0c92413b`에서 금액 결정 책임을 Rust `preview_order`로 이전한 뒤였음. 원래의 Rust `lineAmounts` 표시 경로를 복원하면서 전체 업무 로딩 의존은 제외함.
- 이전 `original-oracle-seal.test.mjs`의 현재 가격 함수와 최초 Tauri 코드 동일성 강제를 해제. 11월 6일 blob SHA를 보존해 역사적 자료임을 기록하는 테스트로 변경.
- 기존 수취인 자동병합·거래처 단가 즉시 재계산의 최신 개선 호환성은 별도 테스트 결과까지 `OPEN`으로 유지.

## 후속 — 사용자가 삭제한 Windows 빌드 CMD에 남아 있던 배포 연결 복구

- `PACKAGE_WINDOWS_TAURI.cmd`는 이미 삭제된 `BUILD_WINDOWS_TAURI.cmd`를 호출하여 아무 배포 파일도 만들지 못하는 상태였음. 사용자가 의도한 삭제 파일은 **복구하지 않음**.
- 기존 로컬 수동 실행 진입점 `PACKAGE_WINDOWS_TAURI.cmd`에서 직접 `scripts/package-windows-tauri.ps1`를 실행하도록 교정. DEV Windows 실행과 분리.
- 신규 스크립트는 노드 의존성 핑거프린트 확인 → 실제 Tauri release EXE 빌드 → 변경된 OCR 런타임만 준비 → ZIP 임시 생성 → 한국어 설치 경로로 압축 해제 및 PaddleOCR 실추론 검증 → 정상일 때만 기존 배포 ZIP 교체 순으로 실행.
- CI 구조 테스트는 `test.todo`에서 실제 계약 검사로 승격. Windows 실제 Release+OCR 전체 설치/배포 검증은 **별도 수동 workflow 검증 전 OPEN**.

## 후속 — 1개 잠정 수량 승인 버튼의 TypeScript 업무 판단 제거

- `OrderDesk.tsx`에서 수동으로 상품/옵션/수량 후보 문구를 해석해 `1개로 잠정 처리` 가능 여부를 결정한 코드를 제거.
- Rust `issues.rs::can_approve_missing_quantity`를 **실제 `apply_issue_action`과 UI 자격 평가의 단일 근거**로 통합.
- `order_issues_with_actions`만 버튼용 `canApproveOne` 상태를 추가하고, 기존 `order_issues` 페이로드는 원본과의 차등 검사 호환성을 위해 유지.
- React는 Rust가 반환한 true/false 및 이미 정한 화면 잠금 상태만 전달. 실제 승인 시 Rust가 같은 조건을 다시 검사.
- Rust 단위 테스트는 정상 1개 승인과 상품/옵션 누락, 이미 확정된 수량, 수량 연결 불명확 및 복수 후보의 거부를 확인. CI·Windows 결과는 따로 판정.

## 후속 — 주소·전화 오류 안내의 TypeScript 중복 규칙 제거

- `OrderDesk.tsx`에서 TypeScript `contactProblem`으로 연락처 자릿수/형식의 안내문을 재판단하던 경로 제거.
- Rust `issues.rs`의 기존 `contact_problem`과 `valid_contact`를 그대로 활용하여 `delivery_requirements_with_messages`에서 유효성 및 자세한 설명을 한 번에 반환.
- React 화면은 Rust 설명문을 표시만 함. 불필요하게 이전 상태에서 빈 오류 문구를 표시하지 않으며, 기존 `delivery_requirements` 응답은 차등 검사 호환성 유지.
- 전 기능 동등성 PASS 아님. CI / 실제 Windows 결과는 별도 기록.

## 후속 — 실 Windows에서 발견된 품목 금액 로딩 종속 재설계

- [실 Windows 실패 증거](https://github.com/ThunderStrikerKR/TSBH/actions/runs/43966959091): 거래처를 선택한 직후 React 계산을 제거한 화면에서 품목 가격이 계속 `—`로 남아 자동 시나리오 실패. 11월 13일 Rust 전면 이전 원칙은 유지함.
- 원인: 품목당 금액 `orderPreview.lineAmounts`를 얻으려면 배송 조정·전체 주문 미리보기 완료(`flowReady`)를 기다려야 했음. 단순 품목 값 조회가 전체 Rust workflow IPC에 종속돼 생긴 실제 UX 오류.
- 수정: Rust `workflow_preview.rs::line_amounts`를 공유 로직으로 추출하고 `preview_order_lines` 읽기 전용 명령 추가. 동일한 Rust 함수가 전체 주문 미리보기 및 독립 품목 금액을 계산함.
- React `useNativeOrderLineAmounts`는 가격에 영향을 주는 원천 인자만 fingerprint로 관리해, 배송·연락처·원문 변경과 관계없이 이미 계산된 Rust 품목 금액을 표시. 품목값이 변경되면 오래된 값은 즉시 무효화함.
- React 수량 곱셈을 부활시키지 않음. 전체 주문 총액·배송건수·부가세는 기존 `useNativeOrderPreview`의 Rust 결과 그대로 유지.
- 원본과 다른 변경은 사용자가 확정한 Rust 업무 분리/비동기 안정화를 위한 신설 실행 경로다. Windows 29시나리오 검증 완료 여부는 해당 커밋 결과까지 별도 확인.
