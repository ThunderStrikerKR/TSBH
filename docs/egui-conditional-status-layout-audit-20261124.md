# egui 조건부 상태 안내문구 레이아웃 전수조사 및 조치 보고서

- 조사일: 2026-11-24
- 저장소: `ThunderStrikerKR/TSBH`
- 기준 브랜치: `main`
- 코드 조사 기준 커밋: `24d1099f1b08f5138effda4807fb67880c408032`
- 조사 범위: `egui-app/src/` 아래 Rust 소스 56개 전체
- 상태: 코드 및 문서 수정은 main에 반영. 최종 Windows CI는 아래 검증 기록의 최신 상태를 따름.

## 1. 요약

수동 분류 상단의 `초안 적용 후 주문 확인 필요`는 상태별 안내문구를 헤더 행 안에서 교체 렌더링하던 구현이었다. 단일 문자열만 제거하면 같은 방식의 다른 안내문구가 남으므로, egui 소스 전체에서 조건부 라벨·경고·오류·힌트·빈 상태·비동기 상태가 부모 레이아웃을 소비하는지 함께 조사했다.

조사 결과, **13개 Rust 모듈의 19개 상태/안내 출력 위치**를 고정 슬롯 또는 고정 높이 행으로 통일했다. 수동 분류의 일반 안내문구는 제거했고, 실제 적용 차단 사유는 툴팁으로 옮겼다. 업무 검증과 저장·적용 조건은 삭제하지 않았다.

핵심 변경은 다음과 같다.

1. `components::fixed_row`와 `components::fixed_status_slot`을 공통 UI 레이어에 추가했다.
2. 상태문구가 없거나 나타나도 부모가 예약하는 Rect와 다음 컨트롤의 Rect가 같도록 했다.
3. 긴 메시지는 한 줄로 클리핑하고 전체 내용은 툴팁으로 제공한다.
4. 버튼이 포함된 안내 행도 고정 높이로 예약한다.
5. 실제 egui Rect 비교 테스트를 추가하고 Windows targeted workflow에서 실행하도록 연결했다.

## 2. 조사 방법과 범위

### 조사 기준

각 파일에서 다음 패턴을 함께 살폈다.

- 상태 변수: `error`, `notice`, `guidance`, `warning`, `pending`, `blocked`, `busy`, `loading`, `ready`, `empty`, `unassigned`, `validation` 등
- 렌더링 호출: `label`, `colored_label`, `status`, `caption`, `add_space`, `separator`, `horizontal`, `vertical`, `allocate_ui`, `allocate_ui_with_layout`, `add_enabled_ui` 등
- 레이아웃 경계: 모달의 본문/푸터, 고정 Rect, 스크롤 영역, 표의 고정 행, 형제 패널과 다음 컨트롤의 시작 좌표
- 상태 변경 시나리오: 오류 발생/해제, 중복 태그 추가, 비동기 응답 도착, 데이터 미확정/확정, 상품별/그룹별 모드 전환, 설정 경로 존재/미설정

단순히 조건문이 있다는 이유만으로 결함으로 분류하지 않았다. 실제로 부모 레이아웃의 행을 추가하거나 형제 컨트롤의 Y 좌표를 바꾸는지, 또는 이미 고정된 Rect·스크롤 뷰포트 안에서 데이터 콘텐츠만 바꾸는지를 구분했다.

### 전체 조사 파일 56개

아래 파일은 모두 `main`에서 열어 조건부 렌더링과 상태 관련 패턴을 확인했다.

**접수·공통 UI**
- `main.rs`
- `components.rs`
- `theme.rs`
- `typography.rs`
- `layout.rs`
- `floating_notice.rs`
- `review_panel.rs`
- `source_highlight.rs`
- `manual_source_view.rs`
- `manual_workspace.rs`
- `manual_product_search.rs`
- `reception_jobs.rs`
- `reception_service.rs`
- `clipboard_intake.rs`
- `source_autosave.rs`
- `latest_job.rs`
- `kakao_context.rs`
- `kakao_dock.rs`
- `ocr_learning.rs`

**상품·카탈로그·거래처**
- `product_manager.rs`
- `product_images.rs`
- `product_excel.rs`
- `product_excel_ui.rs`
- `alias_manager.rs`
- `brand_manager.rs`
- `category_dropdown.rs`
- `category_manager.rs`
- `model_group_manager.rs`
- `client_manager.rs`
- `client_chooser.rs`
- `client_directory.rs`
- `client_master.rs`
- `client_search_tags.rs`
- `automatic_message_manager.rs`

**운임·주문·정산·Excel**
- `freight_manager.rs`
- `order_history_detail.rs`
- `order_history_manager.rs`
- `order_history_memo.rs`
- `daily_settlement_manager.rs`
- `excel_manager.rs`
- `excel_service.rs`

**설정·도메인·인프라**
- `settings_view.rs`
- `settings_store.rs`
- `settings_help.rs`
- `management_queries.rs`
- `business_state.rs`
- `preview_store.rs`
- `pricing_basis.rs`
- `developer_ui_samples.rs`
- `workflow_board.rs`
- `workflow_board_ui.rs`
- `gateway.rs`
- `http_gateway.rs`
- `native_order_change.rs`
- `catalog_query_bridge.rs`

총 56개다.

## 3. 발견 사항 및 조치

| 화면 / 파일 | 기존 동작과 위험 | 조치 |
|---|---|---|
| 수동 분류 헤더 — `manual_workspace.rs` | 적용 가능 여부에 따라 안내문구가 헤더 안에서 교체되며, 일반 상태에도 `초안 적용 후 주문 확인 필요`가 표시됨 | 일반 안내문구를 제거했다. 헤더에는 미배정 원문 건수만 남겼다. 실제 차단 사유는 건수와 적용 버튼의 툴팁으로 제공한다. |
| 수동 분류 배송 정보 — `manual_workspace.rs` | 도서·산간 판정 결과/오류가 나타날 때 배송 옵션 위에 라벨 행이 추가됨 | 고정 상태 슬롯으로 변경했다. 판정 안내와 오류 내용은 유지한다. |
| 상품 상세 수정 — `item_editor.rs` | 선택 불가, 고객 확인, 저장 오류가 별도 조건부 라벨로 쌓여 적용/삭제 버튼을 밀 수 있음 | 세 메시지를 하나의 고정 슬롯으로 통합했다. 전체 문구는 툴팁으로 보존하고 오류/검증 상태에 맞는 색을 유지한다. |
| 거래처 검색 태그 — `client_manager.rs` | 중복 태그 안내가 발생할 때만 행을 삽입함 | 태그 편집기 아래 18pt 고정 슬롯을 상시 예약한다. 중복 안내는 해당 슬롯에만 표시한다. |
| 접수 부가세 팝업 — `main.rs` | 오류가 발생할 때만 적용 버튼 위에 오류 라벨이 추가됨 | 오류 유무와 무관하게 동일한 상태 슬롯을 예약한다. |
| 접수 자동 인식 폴더 — `main.rs` | 경로가 있을 때만 설명과 버튼 행이 생겼고, `folder_body_height`도 경로 유무에 따라 달랐다. 그 값이 `balanced_gap` 계산에 들어가 클립보드/직접 입력/폴더 영역의 간격과 Y 좌표를 바꿀 수 있었다. | 폴더 그룹의 본문 높이를 항상 `METADATA_LINE_HEIGHT + SPACE_8 + ACTION_STANDARD_HEIGHT`로 고정했다. 상태 설명 슬롯과 경로 복사/폴더 열기 행을 항상 예약하고, 실제 경로가 있을 때만 버튼을 그린다. |
| 일일 결산 날짜 — `daily_settlement_manager.rs` | 날짜 형식이 잘못된 경우 툴바 아래 오류 행이 추가됨 | 날짜 오류용 고정 슬롯을 예약한다. 날짜 검증은 그대로 유지한다. |
| 운임 수정 — `freight_manager.rs` | 상품/그룹 모드에 따라 안내문구 행이 나타나거나 사라짐 | 두 모드 모두 동일한 상태 슬롯을 예약하고 그룹 모드에서만 안내를 표시한다. |
| 운임 수정 — `freight_manager.rs` | 최대 묶음 수량 초과 시 해당 행에 상태 라벨이 추가되어 행 높이와 후속 행 정렬이 달라질 수 있음 | 24pt 고정 경고 슬롯에 `!`를 표시하고 전체 사유는 툴팁으로 제공한다. |
| 운임 기준 변경 — `freight_manager.rs` | 기준 변경 대상에 따라 서로 다른 길이의 설명이 본문에 직접 렌더링됨 | 모달 본문에 고정 높이 행을 예약하고, 긴 설명은 툴팁으로 보존한다. |
| Excel 작성 확인 — `excel_manager.rs` | 송하인 정보가 미완성일 때 경고와 설정 이동 버튼 행이 추가됨 | 액션 높이의 고정 행을 항상 예약한다. 미완성일 때만 경고와 설정 버튼을 렌더링한다. |
| 상품 Excel 검토 — `product_excel_ui.rs` | 누락 상품, 500행 초과, 검토 오류 문구가 각각 조건부로 추가됨 | 세 위치 모두 고정 상태 슬롯으로 변경했다. 원본 상품 보존 안내, 전체 행 검증, 오류 내용은 유지한다. |
| 주문 메모 — `order_history_manager.rs` | 저장 권한이 없거나 저장 오류가 발생할 때 안내 행이 추가됨 | 권한 안내와 저장 오류를 하나의 고정 슬롯에 결합한다. 둘 다 발생하면 전체 문구는 툴팁으로 보존한다. 저장 가능 여부와 저장 조건은 변경하지 않았다. |
| 카테고리 관리 — `category_manager.rs` | 기존 최상위 분류를 선택할 때만 긴 설명이 삽입됨 | 상세 편집 영역에 고정 슬롯을 예약하고 해당 분류일 때만 설명을 표시한다. |
| 모델군 관리 — `model_group_manager.rs` | 이전 대표색상은 있으나 대표모델 ID가 없는 경우 안내문구가 상품 목록 앞에 추가됨 | 고정 슬롯으로 변경했다. 대표모델 지정 안내와 데이터는 유지한다. |
| 별칭 관리 — `alias_manager.rs` | 진단 메시지 슬롯은 이미 고정 높이였으나 화면별 자체 구현으로 남아 있었음 | 공통 `fixed_status_slot`을 사용하도록 통일했다. 기존 3번째 캡션 행과 툴팁 동작은 유지한다. |
| 공통 요약 — `components.rs` | `summary_strip`의 선택적 footnote가 있을 때만 행을 추가함 | footnote 유무와 관계없이 고정 슬롯을 예약한다. 공통 헬퍼 자체의 재발 방지 조치다. |

## 4. 공통 컴포넌트와 레이아웃 계약

### `components::fixed_row`

- 부모가 지정한 폭과 높이만큼 먼저 Rect를 예약한다.
- 실제 텍스트/버튼 렌더링은 그 Rect 안의 자식 UI에서만 수행한다.
- 자식 콘텐츠가 없어도 부모의 다음 행 위치는 변하지 않는다.
- 행 바깥으로 그려지는 텍스트는 클리핑한다.

### `components::fixed_status_slot`

- 상태 텍스트용 고정 폭·높이 슬롯이다.
- 기본 사용 높이는 `typography::METADATA_LINE_HEIGHT`(18pt)다.
- 한 줄 표시를 유지하고, 긴 원문은 툴팁으로 확인한다.
- 경고 마커처럼 좁은 슬롯도 같은 함수로 처리할 수 있다.

### 업무 로직 보존

이번 변경은 표시 위치만 다룬다. 다음 로직은 삭제하거나 완화하지 않았다.

- 수동 분류의 비동기 처리 중 적용 금지
- 미확정 수량이 남은 경우 적용 금지
- 미반영 입력이 남은 경우 적용 금지
- Rust 최종 reducer 검증
- 도서·산간 판정 결과 및 오류
- 운임 최대 묶음 수량 검증
- 송하인 정보 미완성 안내와 설정 이동
- 상품 Excel 누락 행 보존, 전체 행 검증 및 사용자 승인
- 운영 메모 저장 권한과 저장 오류
- 카테고리 ID·상품 연결 및 모델군 대표모델 지정 안내

## 5. 문제로 오인하지 않고 유지한 사례

다음 항목은 조건부 콘텐츠가 있지만, 고정된 부모 Rect 또는 스크롤 뷰포트 내부에서 실제 데이터/검색 결과를 보여주는 구조다. 이 항목들을 단순히 숨기거나 고정 슬롯으로 치환하면 업무 정보가 사라지거나 스크롤 동작을 해칠 수 있어 이번 레이아웃 이동 결함으로 분류하지 않았다.

- `floating_notice.rs` 및 각 화면의 `floating_notice::sync/publish/section`: 화면 Rect를 덮는 오버레이 방식이며 부모 레이아웃 행을 추가하지 않는다.
- `components::management_body_state_overlay`: 이미 할당된 viewport 안에 그리는 방식이다. 기존 `management_async_layout_contract_tests`가 부모 cursor 불변을 검사한다.
- `components::management_pending_primary_card`: 비동기 로딩 중에도 승인된 기본 카드 크기를 먼저 예약한다.
- `client_chooser.rs`: 검색 중/빈 결과 메시지가 표의 고정 행 Rect 안에 렌더링된다.
- `manual_product_search.rs`: 오류/검색 중/빈 결과 메시지가 제한된 검색 결과 영역 안에 표시된다.
- `review_panel.rs`, `main.rs`의 수취인·품목 검토 이슈: 고정된 검토/주문 패널의 스크롤 콘텐츠다. 이슈 수에 따라 콘텐츠 길이는 바뀌지만 형제 패널 경계는 바뀌지 않는다.
- `order_history_detail.rs`: 입금 기록, 수취인, 변경 이력, 원문 등 실제 저장 데이터의 존재 여부를 보여주는 상세 콘텐츠다.
- `daily_settlement_manager.rs`의 보고서 로딩/빈 결과 및 검토 항목: 결산 카드 내부의 데이터 콘텐츠이며, 오류가 표시될 때 아래 액션을 밀어내는 형태가 아니다.
- `freight_manager.rs`의 삭제 영향 상품 목록: 삭제 전 영향 범위를 보여주는 본문 콘텐츠다. 모달 푸터는 별도 고정 영역이고 삭제 검증을 유지해야 한다.
- `client_manager.rs`의 거래처 검색 결과 없음: 고정 행 기반 목록의 빈 상태다. 상단 목록 상태 문구도 기존 헤더 행 안에서 한 줄 텍스트만 교체한다.
- `category_manager.rs`의 기존 분류 그룹과 `model_group_manager.rs`의 상품 검색 결과: 실제 목록 데이터이며 스크롤 영역 내부에 있다.
- `product_manager.rs`, `settings_view.rs`, `client_manager.rs`, `freight_manager.rs`, `order_history_manager.rs`에서 이미 FloatingNotice로 처리되는 오류: 오버레이 방식이므로 별도 변경하지 않았다.
- `main.rs`의 OCR 검토 힌트: 이미 고정된 footer Rect 안에 배치된다.

## 6. 테스트 및 CI

### 추가한 회귀 테스트

- `components::fixed_layout_slot_contract_tests::optional_status_keeps_its_rect_and_the_next_control_fixed`
  - 상태 텍스트 없음/있음 두 경우의 슬롯 Rect와 다음 컨트롤 Rect를 비교한다.
- `components::fixed_layout_slot_contract_tests::optional_action_keeps_its_row_and_the_next_control_fixed`
  - 버튼 행 콘텐츠 없음/있음 두 경우의 행 Rect와 다음 컨트롤 Rect를 비교한다.
- `manual_workspace_modal_routing_tests`
  - 일반 문구 `초안 적용 후 주문 확인 필요`가 수동 분류 렌더링 경로에 다시 들어오지 않는지 검사한다.
  - 실제 적용 차단 사유는 툴팁으로 남아 있는지 검사한다.

### CI 연결

`.github/workflows/egui-targeted-check.yml`에 다음 검증 단계를 추가했다.

```sh
cargo test --manifest-path egui-app/Cargo.toml fixed_layout_slot_contract_tests -- --nocapture
```

### 실행 결과 기록

- 수정 과정 초기에 발견된 컴파일 오류: `egui::Response::on_hover_text(self)`가 Response를 소비한 뒤 `clicked()`를 호출한 소유권 오류였다.
- 수정: `clicked()` 결과를 먼저 읽고 툴팁을 연결하도록 순서를 변경했다. 수정 커밋은 `a2e12d9f25b7170dc6d7e036ee0743ee0afcaee1`이다.
- 수동 분류의 기존 source-contract 테스트가 이전 클릭 코드 문자열을 검사해 실패한 건은 `if apply_clicked{apply=true;}` 기준으로 갱신했다. 수정 커밋은 `0fc427aa2cbcc75083e8531457ed2db44e5fa0df`이며, 해당 커밋의 수동 분류 Windows 테스트 run `45254568332`는 `success`였다.
- 넓은 targeted run `45253872313`에서는 기존 소스 계약 테스트 세 건이 오래된 구현 문자열/개수 가정을 검사해 실패했다. 별칭 진단 슬롯의 직접 툴팁 호출, 카테고리 검색의 좁은/넓은 분기 2개, OCR 체크박스의 들여쓰기 문자열이 각각 원인이었다. 공통 슬롯 사용, 논리 검색 필드의 반응형 분기, 의미 기반 체크박스 검증으로 갱신했다. 수정 커밋은 `6de56471710ab78b8fbdec4580f73191a4237022`이다.
- `fixed_row`는 부모에 한 번만 정확한 크기를 할당하도록 `allocate_ui_with_layout` 기반으로 정리했다. 수정 커밋은 `ef3d8a8be2e45b42f20e1ddee31ee5810b5df11a`이다.
- 최종 구현 커밋 `ef3d8a8be2e45b42f20e1ddee31ee5810b5df11a`를 기준으로 Windows targeted workflow run `45255804832`는 문서 갱신 시점에 `pending` 상태였다. 수동 분류·클라이언트/운임·상품 모달 관련 후속 workflow도 같은 커밋에서 실행 중이다.
- 이전 SHA에서 실패/취소된 실행은 최종 성공으로 간주하지 않는다. 최종 Windows 컴파일과 새 Rect 회귀 테스트가 완료되기 전까지 전체 PASS로 판정하지 않는다.
- 실제 창의 픽셀/사용자 운영 DB 검증은 CI와 별도 경계로 남긴다.

## 7. 변경 파일

**Rust 소스 13개**
- `egui-app/src/components.rs`
- `egui-app/src/alias_manager.rs`
- `egui-app/src/manual_workspace.rs`
- `egui-app/src/item_editor.rs`
- `egui-app/src/client_manager.rs`
- `egui-app/src/main.rs`
- `egui-app/src/daily_settlement_manager.rs`
- `egui-app/src/freight_manager.rs`
- `egui-app/src/excel_manager.rs`
- `egui-app/src/product_excel_ui.rs`
- `egui-app/src/order_history_manager.rs`
- `egui-app/src/category_manager.rs`
- `egui-app/src/model_group_manager.rs`

**검증 설정 및 문서**
- `.github/workflows/egui-targeted-check.yml`
- `docs/egui-manual-classification-review-20261124.md`: 이전의 일반 안내문구 유지 지침을 고정 레이아웃/툴팁 정책으로 정정
- `docs/egui-conditional-status-layout-audit-20261124.md`: 본 전수조사 보고서

## 8. 재발 방지 기준

앞으로 상태·오류·경고·도움말을 추가할 때 다음 규칙을 적용한다.

1. 고정 화면/모달 안의 상태문구는 `fixed_status_slot` 또는 `fixed_row`를 우선 사용한다.
2. 단순 설명을 위해 형제 컨트롤 앞에 조건부 `ui.label` 행을 삽입하지 않는다.
3. 오류/경고는 표시를 제거하지 말고 고정 슬롯, 툴팁, FloatingNotice 중 정보 성격에 맞는 방식을 선택한다.
4. 비동기 상태 전환 전후에 부모 Rect, 다음 컨트롤 Rect, 푸터 Rect를 비교한다.
5. 실제 데이터 목록과 검토 이슈는 고정 스크롤 뷰포트 안에서 유지하되, 부모 패널의 크기를 바꾸지 않는다.
6. 새 공통 UI 슬롯을 만들면 최소한 상태 없음/있음, 버튼 없음/있음의 실제 egui Rect 회귀 테스트를 함께 추가한다.

**최종 판정:** 소스 전수조사와 코드 조치는 main에 반영했다. 최종 PASS 여부는 최신 구현 커밋 `ef3d8a8be2e45b42f20e1ddee31ee5810b5df11a`의 Windows CI run `45255804832` 완료 결과로 확정하며, 실제 운영 창의 픽셀 검증은 별도 경계로 남긴다.
