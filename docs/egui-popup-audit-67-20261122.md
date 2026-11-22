# 반도허브 67차 — OCR 마스터 기반 egui 팝업 전수점검

기준: 2026-11-22 `TSBH/main`의 Rust/egui 구현과 65·66차 OCR 검토창 마스터. 이 문서는 **기존 팝업 진입 코드 전수 목록과 67차 정리 범위**이며 Windows에서 모든 팝업을 실제로 열어 시각 검수한 보고서가 아니다.

## 1. 구조·업무 원칙

- **헤더**: 좌측 팝업 제목, 우상단 **취소/닫기**, 보조 설명이 필요한 OCR처럼 확정된 팝업만 설명 행 사용. 별도의 X 아이콘이나 하단 취소 버튼을 중복 추가하지 않는다.
- **본문**: 대상 확인 → 검색/편집 → 필요한 유효성·영향 정보 → 최종 행동 순서. 분리된 입력·이미지·표는 각자 국소 스크롤하며 헤더와 최종 행동 영역을 밀어내지 않는다.
- **푸터**: 최종 저장/등록/적용/확인/삭제는 우측. 큰 편집 팝업에서 다른 위험 작업(삭제, 초기화)이 함께 있다면 **왼쪽**, 최종 저장/적용은 **오른쪽**의 별도 슬롯을 사용한다.
- **위험 작업**: 확정 데이터 삭제·작업본 전체 초기화·미저장 변경 폐기·DB 복원은 `Danger` 역할. 사용자가 다시 확인할 기존 중첩 확인창은 유지하며 업무 로직을 변경하지 않는다.
- **단순 검색/조회**: 선택은 결과 행 내부에서 진행하고, 실제 저장/확정이 없는 팝업에는 푸터를 새로 만들지 않는다.
- **공통 소유권**: `components::modal_header` / `modal_master_header_in_rect`, `modal_action_row` / `modal_master_footer_in_rect`, `modal_action_row_split`, `theme.rs` 토큰을 사용한다. 고정된 58pt 푸터와 36pt 실행 버튼의 히트박스를 유지한다.

## 2. 팝업 진입 경로 전수 목록

실행 경로에서 `components::modal_window`·`components::decision_window`를 사용하는 팝업 **45개 / 17개 모듈**. 공통 함수 구현 내부의 자체 호출, 반복 UI 렌더링 및 중첩 확인창은 중복 집계하지 않는다.

| 모듈 | 수 | 팝업 ID |
|---|---:|---|
| `alias_manager.rs` | 2 | `alias-product-search`, `alias-unsaved-confirm` |
| `automatic_message_manager.rs` | 1 | `message-editor-leave` |
| `brand_manager.rs` | 1 | `brand-unsaved-confirm` |
| `category_manager.rs` | 3 | `native-category-editor`, `category-operation-modal`, `category-unsaved-confirm` |
| `client_chooser.rs` | 1 | `egui-reception-client-chooser` |
| `client_manager.rs` | 3 | `native-client-manager-editor`, `client-manager-delete-confirm`, `client-manager-discard-confirm` |
| `excel_manager.rs` | 1 | `excel-export-confirm` |
| `freight_manager.rs` | 9 | `freight-create`, `freight-rename`, `freight-delete`, `freight-product-reset`, `freight-rate-modal`, `freight-discard-confirm`, `freight-basis-confirm`, `freight-exception`, `freight-tab-leave` |
| `main.rs` | 6 | `egui-ocr-review`, `ocr-review-discard-confirm`, `egui-replace-source-confirm`, `egui-clear-source-confirm`, `freight-global-leave`, `settings-global-leave` |
| `manual_workspace.rs` | 4 | `bando-native-manual-classification`, `manual-shipment-removal`, `manual-reset-confirm`, `manual-discard-prompt` |
| `model_group_manager.rs` | 2 | `family-delete-confirm`, `family-unsaved-confirm` |
| `order_history_detail.rs` | 1 | `history-order-detail` |
| `order_history_manager.rs` | 1 | `history-shipment-memo` |
| `product_excel_ui.rs` | 2 | `egui-product-excel-review`, `egui-product-excel-confirm` |
| `product_manager.rs` | 3 | `egui-product-detail`, `egui-product-archive-confirm`, `egui-product-discard-confirm` |
| `settings_view.rs` | 4 | `settings-tab-unsaved`, `settings-reload-confirm`, `settings-audit-records`, `settings-restore-confirm` |
| `manual_product_search.rs` | 1 | `native-manual-product-search` |
| **합계** | **45** | |

## 3. 기존 공통화와 67차 실제 수정

| 범주 | 67차 조치 | 업무 영향 |
|---|---|---|
| 공통 푸터 | 고정형/흐름형 양쪽의 표면과 1px 상단 구분선을 단일 픽셀 정렬 함수로 통일 | 실행·닫기 동작 변경 없음 |
| 다중 행동 푸터 | 공통 `modal_action_row_split` 신설; 분류·거래처·상품 편집과 수동 분류에 적용 | 위험/보조 좌측, 저장/적용 우측 |
| 분류 편집 | 삭제·저장 후속 처리 요청을 푸터 렌더링에서 분리 | 동일 작업과 기존 Rust 검증 유지 |
| 거래처 편집 | 거래처 삭제·등록/저장 버튼 위치 통일 | 등록/삭제 실행 경로 유지 |
| 상품 편집 | 상품 삭제·등록/저장 위치 통일, 신규 상품 실행 라벨을 `상품 등록`으로 맞춤 | 상품 생성/수정 실행 경로 유지 |
| 수동 분류 | 전역 초기화를 상단 도구줄에서 푸터 좌측으로 이동, 미배정·정식접수 상태는 제목 아래 정보행에 유지 | 기존 초기화 확인·입력 반영 요구·초안 적용 유지 |
| 저장하지 않은 변경 | 기존 `저장 안 함` 버튼의 혼용된 `Standard`를 `Danger`로 통일 | 저장/폐기/이동 동작 유지 |
| 데이터 복구 | 최종 `이 백업으로 복구`를 `Danger` 역할로 변경 | 복구 전 안전 백업 및 복원 명령 그대로 |
| 예외 보존 | OCR 검토, 거래처 선택, 상품 선택, Excel 미리보기, 주문 상세, 감사 목록에 기능과 관계없는 푸터/확인을 추가하지 않음 | 검색·조회 방식 유지 |

## 4. 검수 범위 및 남은 확인

- **정적 소스 점검**: 팝업 45개 경로 식별, 공통 함수 연결, 확인/삭제/저장 버튼 역할, 본문 스크롤 및 기존 중첩 팝업 경로 점검.
- **새 기능 미추가**: Rust 업무 판단, 운임/부가세/가격, 확인·저장 상태, 원문·OCR 자료, 배송건 매칭, 데이터베이스 동작 미변경.
- **실기기 확인 필요**: Windows 100/125/150% 배율의 긴 버튼 라벨 잘림, 최단 너비에서 좌우 푸터 겹침, 긴 본문 스크롤과 하단 버튼 가시성, 다중 Dim·ESC·저장 실패 경로. 이 검증을 수행하기 전에는 화면까지 완료됐다고 판정하지 않는다.
- **기존 정책**: 미승인 버튼·설명문 추가 금지, 팝업 이동/크기변경 금지, 카카오톡 제외 작업영역 중앙 배치 유지.

67차에서 작업하지 않은 개별 팝업의 기능/디자인까지 실제 화면에서 검수했다고 주장하지 않는다.
