# 35차 고정 TypeScript 원본 대비 전체 기능군 감사

**전체 기능 동등성 PASS 아님.** 고정 원본 SHA: 30a19cd2da7533dd2ed36b807593309bde70a7eb.
전수조사 기록 목적: 같은 부분만 반복 검사하지 않고 미검증 범위를 보존하고, 원본이 바뀐 가짜 차등 검증을 구별한다.

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
- 다음 세션은 새로 생성된 source coverage JSON의 CHANGED_SINCE_BASELINE과 OPEN/ORACLE_REVIEW_OPEN부터 수정한다.
- 원본과 다른 동작을 발견하면 사용자 승인 없는 신규 규칙을 덧붙이지 않고 원본을 복원한다.
