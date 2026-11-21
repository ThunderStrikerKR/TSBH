# 2026-11-21 반도허브 관리형 표 텍스트 굵기·명도 전수감사

## 감사 사유와 결론

주문 내역의 **청구 합계**가 볼드로 남았던 직접 원인은 `order_history_manager.rs`가 `ManagementTableCellRole::Total`을 사용하고, `typography.rs`가 해당 역할만 `emphasized=true`로 렌더링했기 때문이다. 일일 결산의 청구 합계에도 같은 역할을 사용했다. 이전 전수조사에서 **반복 표의 최종 금액**을 **독립 KPI/요약값**으로 잘못 분류한 것이 근본 원인이다.

**현재 기준:** 반복되는 관리 표 행의 상품명·거래처명·옵션·수량·일반 금액·청구 합계(`RowTotal`)는 전부 **14pt Regular**. 금액/일반 이름은 `TEXT`, 상태·수량·카테고리·설명은 `SECONDARY`, 경로 조상은 `MUTED`. 표 헤더와 독립 KPI/요약 카드만 기존 독립 강조 규칙을 쓴다. 행 높이·열 너비·정렬·데이터·운임/부가세/매출 계산을 바꾸지 않았다.

## 조사 범위

`main`의 `egui-app/src/*.rs` **47개 Rust 소스 전체**를 파일 단위로 읽고, `*.strong()`, `FontId`, `FontFamily`, `RichText`, 강조 bool, 공통/직접 galley, 상품명·수량·금액 출력, 정렬·클리핑 경로를 검사했다. 그룹별 조회 항목은 다음과 같다.

- **반복 관리 표 및 목록 핵심:** `freight_manager.rs`, `order_history_manager.rs`, `product_manager.rs`, `excel_manager.rs`, `client_manager.rs`, `daily_settlement_manager.rs`, `components.rs`, `typography.rs`.
- **관련 트리·선택·상세·편집 UI:** `category_manager.rs`, `model_group_manager.rs`, `alias_manager.rs`, `brand_manager.rs`, `client_chooser.rs`, `client_directory.rs`, `client_master.rs`, `automatic_message_manager.rs`, `manual_product_search.rs`, `manual_source_view.rs`, `manual_workspace.rs`, `order_history_detail.rs`, `order_history_memo.rs`, `product_excel_ui.rs`, `settings_help.rs`, `settings_view.rs`, `developer_ui_samples.rs`, `workflow_board_ui.rs`, `main.rs`.
- **관련 데이터·기반 및 이외 소스:** `business_state.rs`, `catalog_query_bridge.rs`, `clipboard_intake.rs`, `excel_service.rs`, `gateway.rs`, `http_gateway.rs`, `kakao_context.rs`, `kakao_dock.rs`, `latest_job.rs`, `layout.rs`, `management_queries.rs`, `native_order_change.rs`, `preview_store.rs`, `product_excel.rs`, `reception_service.rs`, `settings_store.rs`, `source_autosave.rs`, `source_highlight.rs`, `theme.rs`, `workflow_board.rs`.

## 실제 렌더링 경로 확인

| 화면 | 확인한 데이터 렌더링 경로 | 현재 처리 |
|---|---|---|
| 배송 그룹 편성 | `freight_assignment_text` | 상품명 Regular, 옵션 Regular, 카테고리 경로 모두 Regular(색 차이 유지) |
| 배송 그룹 관리 / 조합·기준 이력 | `freight_rate_cell`, `history_cell` | 그룹명·상품 관련 설명·개별 운임·수량·상태 모두 Regular |
| 주문 내역 대표행/상세 | `paint_text`, `paint_detail_products` | 거래처명·상품 요약·상세 Regular; **청구 합계 `RowTotal` Regular** (대표행 및 주의 문구가 붙은 상단 반쪽 셀 모두) |
| Excel 대기/이력 | `excel_manager::cell` | 거래처·입금/청구액·건수 모두 Regular |
| 상품 목록/가격표 | `product_table_text`, `product_price_text` | 상품명·옵션 Regular; 가격은 기존 Regular numeric font 및 우측 정렬 유지 |
| 거래처 관리 목록 | `components::management_row_impl` | 대표 거래처 이름 Regular, 보조 정보 Regular; 등급 배지의 상태색은 유지 |
| 일일 결산 | `paint_cell`, `report_table` | 일반 금액 및 **청구 합계 `RowTotal` Regular** |
| 거래처 선택 목록 | `components::management_chooser_row` | 반복 거래처 이름에 남은 직접 `emphasized=true` 제거 |
| 설정 도움말/수동 입력 표 | `typography::table_cell` 및 Regular `FontId` | 기존 일반 굵기 유지 |

**의도적으로 유지한 강조:** 36pt 고정 표 헤더의 강조 글꼴, 운임/KPI 카드의 대형 숫자, 각 화면의 섹션·팝업 제목, 루트 카테고리 트리의 상위 그룹 제목(`CategoryPathFont::Subsection`), 명시적 오류·경고 상태 및 선택 내비게이션. 이는 일반 반복 표의 데이터 셀이 아니라 별도 시각적 위계 요소다. 카테고리 트리 루트 제목과 표 안의 카테고리 경로는 서로 다른 역할이다.

## 구조 변경 및 재발방지

1. `ManagementTableCellRole::Total`을 폐기하고 **`RowTotal`**로 명확히 명명. 이 역할도 다른 일반 행처럼 Regular.
2. `management_table_cell_galley`의 굵기 정책을 모든 데이터 역할에 대하여 Regular로 고정; 기존 14pt/색상 역할 보존.
3. 기존 공용 `freight_table_cell_galley(..., emphasized: bool, ...)`, `client_table_cell_galley(..., emphasized: bool)`를 제거. 드롭다운과 로딩 안내는 `management_control_text_galley`를 사용해 **Regular이면서 동적 disabled/error 색상**을 허용.
4. `typography::management_table_type_roles_tests`에 금액·수량·`RowTotal` 굵기 확인, 주문 내역 및 일일 결산 호출처 `RowTotal` 확인, 기존 bool 강조 함수 재유입 금지 테스트를 추가했다.
5. `.github/workflows/egui-targeted-check.yml`에서 `typography.rs` 변경 시 타깃 CI를 실행하며 공통 데이터 역할 단위 테스트도 실행한다.
6. 문서 `egui-management-ui-ssot.md`, `egui-management-ui-reference.md`, `management-layout-rules.md`의 잘못된 '**최종 청구 합계는 Bold**' 규칙을 정정했다.

## 검증 경계

- **PATCH:** GitHub `main` 수정 및 재조회로 검증.
- **STATIC:** `egui-app/src`의 47개 파일을 기준으로 로컬 직접-bold/공통 역할 우회 경로를 확인. `RowTotal`의 컴파일 시 타입 사용처는 두 화면에 남긴다.
- **UNIT/CI:** 새 테스트와 최신 CI의 실제 통과 여부는 해당 커밋 실행 결과로 별도 기록한다.
- **WINDOWS:** Windows 100/125/150% DPI에서 실제 글꼴 잉크·줄바꿈·클리핑 검수는 별도이며 소스 검사만으로 PASS가 아니다.
