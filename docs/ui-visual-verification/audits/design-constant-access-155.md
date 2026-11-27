# 155차 — 공개 디자인 상수 접근 범위 감사

- 조사 기준: `d9c3a63f5cdd619d18c5f5ec2a22a0c67282d731`
- 소스: 해당 리비전의 egui-app/src 아래 Rust 파일 103개(재귀 Git tree, truncated=false)
- 대상: 27개 공통 디자인 API 계약의 공개 상수 선언 424개(일부 업무·표시 정책 상수 포함)
- 방식: 12개 배치에서 소유 모듈 및 하위 테스트를 제외한 식별자 이름을 보수적으로 대조. 별도 테스트 파일 제외 및 주석·문자열 제거 후 코드 이름 대조, 원문 이름도 별도 대조. 개별 cfg(test) 함수와 인라인 테스트 모듈은 코드 분류에 남을 수 있으며 동명 식별자, 재노출, 매크로의 의미 해석은 하지 않음. 따라서 D는 실제 직접 호출 확정이 아니라 잠재 참조이며, C는 모듈 간 접근을 추가로 확인해야 함.

| 분류 | 개수 | 권고 |
|---|---:|---|
| A | 23 | 내부 전용 전환 우선 검토. 테스트 전용 또는 미사용이면 별도 조치 |
| B | 10 | 테스트 및 문자열 검증의 목적을 확인한 후 내부 또는 테스트 전용으로 전환 |
| C | 122 | 공통 모듈 간 의존성 유지 또는 역할 API/소유 모듈 이전. 개별 화면에는 직접 공개할 필요 없음 |
| D | 269 | 비계약 모듈의 용도 및 직접 접근 확인. 일괄 비공개 금지 |

A+B+C = 155개. 현재 참조 분류이며 155개 모두 즉시 private 전환 가능하다는 의미가 아니다. 전체 424개를 대상으로 한 이름 감사이며 Rust 의미 분석 또는 컴파일 검증은 아님.

D 중 Theme Color32 선언 63개는 역할 기반 팔레트로 전환 검토 대상. 작업 순서는 A -> B -> C -> D의 색상/상태 표현 -> D의 레이아웃 정책. SPACE 토큰, 화면의 실제 배치 요구, 업무 정책 상수는 직접 접근 여부만으로 삭제하지 않는다.

## 확인된 직접 참조 사례

- workflow_board_ui.rs:392–398, 446–449: 신호등 상태색과 테두리, 램프 크기 선택. 공통 진행 상태 컴포넌트에 소유권을 집중하는 후보.
- product_manager.rs:68–71: 판매 상태 문자열별 Theme 색상 선택. 판매 상태 시각 역할 API 후보.
- source_highlight.rs:373–376, 622–623, 806–808: 원문 및 연결 강조 팔레트 참조와 일부 재노출. 비계약 모듈이지만 전문 렌더러이므로 일반 화면과 동일 취급하지 않는다.

## 별도 검증 차단 요인

이전 커밋 cea807c의 Windows egui compile gate(45963406898)에서 API 선언 검사가 27개 모듈/1158개로 실행됐으나, DPI 접근 규칙에 의해 order_history_manager.rs:1157, product_manager.rs:2348, text_fit.rs:13,51이 검출되어 참조 보고서와 컴파일이 실행되지 않았다. 이번 감사는 해당 CI 산출물에 의존하지 않았다. 이번 차수는 조사만 수행하고 운영 소스나 API 허용 목록을 변경하지 않는다.

## 전체 상수 목록

### A — 소유 모듈 외부 이름 참조 없음

| 소유 모듈 | 상수 |
|---|---|
| `theme.rs` | `CARD_BORDER` |
| `theme.rs` | `ERROR_BG` |
| `theme.rs` | `HEADER_BG` |
| `theme.rs` | `HEADER_PADDING_X` |
| `theme.rs` | `INTERACTION_BLUE_TINT` |
| `theme.rs` | `KAKAO_CONTROL_HEIGHT` |
| `theme.rs` | `MANAGEMENT_REFERENCE_HEADING_LINE_HEIGHT` |
| `theme.rs` | `MANAGEMENT_TAB_HEIGHT` |
| `theme.rs` | `PLACEHOLDER_TEXT` |
| `theme.rs` | `RECEPTION_RECIPIENT_BODY_HEIGHT` |
| `theme.rs` | `RECEPTION_RECIPIENT_NAV_NAME_WIDTH` |
| `theme.rs` | `RECEPTION_SECTION_DIVIDER_HEIGHT` |
| `theme.rs` | `TABLE_ROW_ALT_BG` |
| `theme.rs` | `WORKFLOW_SELECTED_ROW_BG` |
| `typography.rs` | `FONT_BRAND` |
| `typography.rs` | `FONT_MODAL_TITLE` |
| `typography.rs` | `FONT_SECTION_HEADING` |
| `typography.rs` | `FONT_SUBSECTION_HEADING` |
| `typography.rs` | `MANAGEMENT_DATE_TEXT_OPTICAL_SHIFT_Y` |
| `typography.rs` | `SUBSECTION_LINE_HEIGHT` |
| `highlight/mod.rs` | `COMPACT_RADIUS` |
| `highlight/mod.rs` | `NAVIGATION_RADIUS` |
| `highlight/mod.rs` | `VALUE_PADDING` |

### B — 테스트 파일·문자열 등에서만 외부 이름 참조

| 소유 모듈 | 상수 |
|---|---|
| `theme.rs` | `CHECKBOX_CHECKED_BG` |
| `theme.rs` | `FREIGHT_COMBINATION_HISTORY_COL_WIDTHS` |
| `theme.rs` | `FREIGHT_GROUP_HISTORY_COL_WIDTHS` |
| `theme.rs` | `MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET` |
| `theme.rs` | `MANUAL_TABLE_ROW_HEIGHT` |
| `theme.rs` | `MODAL_SECTION_BG` |
| `theme.rs` | `RECEPTION_ITEMS_PRODUCT_MIN_WIDTH` |
| `typography.rs` | `FONT_OCR_REVIEW_TEXT` |
| `input/mod.rs` | `INPUT_PLACEHOLDER_COLOR` |
| `input/mod.rs` | `RECEPTION_FREIGHT_MONEY_WIDTH` |

### C — 공통 모듈 코드에서만 외부 이름 참조

| 소유 모듈 | 상수 |
|---|---|
| `theme.rs` | `ACTION_PRIMARY_HEIGHT` |
| `theme.rs` | `ACTION_QUIET_HEIGHT` |
| `theme.rs` | `BORDER` |
| `theme.rs` | `BUTTON_HOVER_BG` |
| `theme.rs` | `BUTTON_HOVER_BORDER_WIDTH` |
| `theme.rs` | `BUTTON_HOVER_LIGHT_BG` |
| `theme.rs` | `BUTTON_HOVER_LIGHT_BORDER` |
| `theme.rs` | `BUTTON_HOVER_TINT` |
| `theme.rs` | `CALENDAR_DAY_SIZE` |
| `theme.rs` | `CALENDAR_GRID_GAP` |
| `theme.rs` | `CALENDAR_GRID_WIDTH` |
| `theme.rs` | `CALENDAR_NAV_SIZE` |
| `theme.rs` | `CATALOG_TREE_CHEVRON_SIZE` |
| `theme.rs` | `CATALOG_TREE_FOLDER_COLOR` |
| `theme.rs` | `CATALOG_TREE_FOLDER_SIZE` |
| `theme.rs` | `CATALOG_TREE_FOLDER_STROKE` |
| `theme.rs` | `CATALOG_TREE_GAP` |
| `theme.rs` | `CATALOG_TREE_GUIDE_COLOR` |
| `theme.rs` | `CATALOG_TREE_GUIDE_WIDTH` |
| `theme.rs` | `CATALOG_TREE_INSET` |
| `theme.rs` | `CATALOG_TREE_TERMINAL_GAP` |
| `theme.rs` | `DISABLED_ACTION_BG` |
| `theme.rs` | `DISABLED_ACTION_BORDER` |
| `theme.rs` | `DISABLED_ACTION_TEXT` |
| `theme.rs` | `DOCK_SPLITTER_HOVER_LINE` |
| `theme.rs` | `DOCK_SPLITTER_LINE` |
| `theme.rs` | `ERROR_BORDER` |
| `theme.rs` | `ERROR_TEXT` |
| `theme.rs` | `FLOATING_NOTICE_ANCHOR_GAP` |
| `theme.rs` | `FLOATING_NOTICE_BG` |
| `theme.rs` | `FLOATING_NOTICE_CLOSE_HIT_SIZE` |
| `theme.rs` | `FLOATING_NOTICE_ICON_SIZE` |
| `theme.rs` | `FLOATING_NOTICE_MAX_WIDTH` |
| `theme.rs` | `FLOATING_NOTICE_PADDING_X` |
| `theme.rs` | `FLOATING_NOTICE_PADDING_Y` |
| `theme.rs` | `FLOATING_NOTICE_POINTER_HALF_WIDTH` |
| `theme.rs` | `FLOATING_NOTICE_POINTER_HEIGHT` |
| `theme.rs` | `FLOATING_NOTICE_SCREEN_MARGIN` |
| `theme.rs` | `FLOATING_NOTICE_SHADOW_ALPHA` |
| `theme.rs` | `FLOATING_NOTICE_SHADOW_BLUR` |
| `theme.rs` | `FLOATING_NOTICE_SHADOW_OFFSET` |
| `theme.rs` | `FLOATING_NOTICE_TEXT_SIZE` |
| `theme.rs` | `FREIGHT_SUMMARY_CARD_HEIGHT` |
| `theme.rs` | `FREIGHT_SUMMARY_CARD_WIDTH` |
| `theme.rs` | `FREIGHT_SUMMARY_PADDING_X` |
| `theme.rs` | `FREIGHT_SUMMARY_PADDING_Y` |
| `theme.rs` | `FREIGHT_SUMMARY_TEXT_GAP` |
| `theme.rs` | `KAKAO_ICON_INK` |
| `theme.rs` | `KAKAO_ICON_MODE_GAP` |
| `theme.rs` | `KAKAO_ICON_YELLOW` |
| `theme.rs` | `KAKAO_MENU_ICON_SIZE` |
| `theme.rs` | `MANAGEMENT_CLIENT_NAME_COLUMN_WIDTH` |
| `theme.rs` | `MANAGEMENT_CLIENT_PHONE_COLUMN_WIDTH` |
| `theme.rs` | `MANAGEMENT_CLIENT_TERMS_COLUMN_WIDTH` |
| `theme.rs` | `MANAGEMENT_DATE_CALENDAR_BUTTON_WIDTH` |
| `theme.rs` | `MANAGEMENT_DATE_CONTROL_GAP` |
| `theme.rs` | `MANAGEMENT_DATE_NAV_BUTTON_WIDTH` |
| `theme.rs` | `MANAGEMENT_DATE_RANGE_SEPARATOR_WIDTH` |
| `theme.rs` | `MANAGEMENT_DROPDOWN_HOVER_BG` |
| `theme.rs` | `MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT` |
| `theme.rs` | `MANAGEMENT_DROPDOWN_OPTION_HEIGHT` |
| `theme.rs` | `MANAGEMENT_DROPDOWN_SELECTED_BG` |
| `theme.rs` | `MANAGEMENT_SEARCHABLE_POPUP_MIN_WIDTH` |
| `theme.rs` | `MANAGEMENT_TABLE_ACTION_INSET` |
| `theme.rs` | `MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT` |
| `theme.rs` | `MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION` |
| `theme.rs` | `MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET` |
| `theme.rs` | `MANUAL_ASSIGNMENT_STROKE_WIDTH` |
| `theme.rs` | `MANUAL_ASSIGNMENT_TINT` |
| `theme.rs` | `MODAL_BACKDROP_ALPHA` |
| `theme.rs` | `MODAL_EDGE_ACTION_SHIFT` |
| `theme.rs` | `MODAL_MASTER_TOOLBAR_RULE_GAP` |
| `theme.rs` | `MODAL_NESTED_BACKDROP_ALPHA` |
| `theme.rs` | `MODAL_SECTION_RADIUS` |
| `theme.rs` | `ONE_TOUCH_BG` |
| `theme.rs` | `ONE_TOUCH_TEXT` |
| `theme.rs` | `ONE_TOUCH_TINT` |
| `theme.rs` | `ORANGE_EMPHASIS_TEXT` |
| `theme.rs` | `ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS` |
| `theme.rs` | `PANEL_RADIUS` |
| `theme.rs` | `RECEPTION_CLIENT_SUMMARY_BG` |
| `theme.rs` | `RECEPTION_CLIENT_SUMMARY_BORDER` |
| `theme.rs` | `RECEPTION_ITEMS_AMOUNT_MAX_WIDTH` |
| `theme.rs` | `RECEPTION_ITEMS_HEADER_HEIGHT` |
| `theme.rs` | `RECEPTION_ITEMS_QTY_WIDTH` |
| `theme.rs` | `RECEPTION_ITEMS_TO_FREIGHT_GAP` |
| `theme.rs` | `RECEPTION_RECIPIENT_NAV_ARROW_WIDTH` |
| `theme.rs` | `RECEPTION_RECIPIENT_NAV_COUNT_WIDTH` |
| `theme.rs` | `RECEPTION_RECIPIENT_NAV_DISABLED_BG` |
| `theme.rs` | `RECEPTION_RECIPIENT_NAV_WIDTH` |
| `theme.rs` | `REQUIRED_INPUT_BORDER` |
| `theme.rs` | `REQUIRED_INPUT_STROKE_WIDTH` |
| `theme.rs` | `REQUIRED_INPUT_TINT` |
| `theme.rs` | `SECONDARY` |
| `theme.rs` | `SECTION_HEADER_HEIGHT` |
| `theme.rs` | `SECTION_RADIUS` |
| `theme.rs` | `SELECTION` |
| `theme.rs` | `SELECTION_STROKE_WIDTH` |
| `theme.rs` | `SETTLEMENT_KPI_CARD_WIDTH` |
| `theme.rs` | `SUCCESS` |
| `theme.rs` | `TABLE_HEADER_OUTLINE` |
| `theme.rs` | `WORKFLOW_CARD_BORDER` |
| `theme.rs` | `WORKFLOW_CARD_RADIUS` |
| `theme.rs` | `WORKFLOW_DETAIL_ROW_BORDER` |
| `theme.rs` | `WORKFLOW_DETAIL_ROW_PADDING_X` |
| `theme.rs` | `WORKFLOW_SELECTED_ROW_BORDER` |
| `typography.rs` | `FONT_BODY` |
| `typography.rs` | `FONT_MAIN_NAV` |
| `typography.rs` | `FONT_MANAGEMENT_TAB` |
| `typography.rs` | `FONT_PRIMARY_ACTION` |
| `typography.rs` | `FONT_RECEPTION_RECIPIENT_TEXT` |
| `typography.rs` | `FONT_STANDARD_ACTION` |
| `typography.rs` | `MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y` |
| `ui_tokens.rs` | `FONT_FIELD_LABEL` |
| `ui_tokens.rs` | `FONT_FREIGHT_SUMMARY_LABEL` |
| `ui_tokens.rs` | `FONT_FREIGHT_SUMMARY_NUMBER` |
| `ui_tokens.rs` | `FONT_PAGE_HEADING` |
| `ui_tokens.rs` | `FREIGHT_SUMMARY_LABEL_LINE_HEIGHT` |
| `ui_tokens.rs` | `FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT` |
| `ui_tokens.rs` | `SPACE_16` |
| `ui_tokens.rs` | `SPACE_2` |
| `ui_tokens.rs` | `SPACE_6` |

### D — 비계약 모듈 코드에 이름 참조

| 소유 모듈 | 상수 |
|---|---|
| `theme.rs` | `ACTION_EMPHASIZED_HEIGHT` |
| `theme.rs` | `ACTION_STANDARD_HEIGHT` |
| `theme.rs` | `BRAND_ICON_OPTICAL_CENTER_Y` |
| `theme.rs` | `BRAND_ICON_SIZE` |
| `theme.rs` | `BRAND_WORDMARK_GAP` |
| `theme.rs` | `BRIGHT` |
| `theme.rs` | `BRIGHT_TEXT` |
| `theme.rs` | `CANVAS` |
| `theme.rs` | `CATALOG_MAP_TOOLBAR_ACTIONS_WIDTH` |
| `theme.rs` | `CATALOG_MAP_TOOLBAR_STACK_WIDTH` |
| `theme.rs` | `CATALOG_SEARCH_WIDTH` |
| `theme.rs` | `CATALOG_TREE_INDENT` |
| `theme.rs` | `CATALOG_TREE_ROW_HEIGHT` |
| `theme.rs` | `CATALOG_TREE_SELECTED_INK` |
| `theme.rs` | `CATEGORY_EXPLORER_ANCESTOR_INK` |
| `theme.rs` | `CATEGORY_EXPLORER_ANCESTOR_MAX_HEIGHT` |
| `theme.rs` | `CATEGORY_EXPLORER_APPLY_WIDTH` |
| `theme.rs` | `CATEGORY_EXPLORER_BACK_WIDTH` |
| `theme.rs` | `CATEGORY_EXPLORER_CRUMB_HEIGHT` |
| `theme.rs` | `CATEGORY_EXPLORER_CRUMB_TEXT_INSET` |
| `theme.rs` | `CATEGORY_EXPLORER_LIST_MAX_HEIGHT` |
| `theme.rs` | `CATEGORY_EXPLORER_OMITTED_WIDTH` |
| `theme.rs` | `CATEGORY_EXPLORER_POPUP_WIDTH` |
| `theme.rs` | `CATEGORY_EXPLORER_SEPARATOR_INK` |
| `theme.rs` | `CATEGORY_EXPLORER_SEPARATOR_WIDTH` |
| `theme.rs` | `CATEGORY_EXPLORER_VISIBLE_ROWS` |
| `theme.rs` | `CATEGORY_PARENT_ACTION_RESERVE_WIDTH` |
| `theme.rs` | `CATEGORY_PATH_PICKER_TEXT_PADDING` |
| `theme.rs` | `CATEGORY_PATH_PICKER_WIDTH` |
| `theme.rs` | `CLIENT_CHOOSER_DIALOG_HEIGHT` |
| `theme.rs` | `CLIENT_CHOOSER_DIALOG_WIDTH` |
| `theme.rs` | `CLIENT_CHOOSER_RESULTS_MAX_HEIGHT` |
| `theme.rs` | `CLIENT_TIER_RETAIL_BG` |
| `theme.rs` | `CLIENT_TIER_RETAIL_TEXT` |
| `theme.rs` | `CLIENT_TIER_SEMI_BG` |
| `theme.rs` | `CLIENT_TIER_SEMI_TEXT` |
| `theme.rs` | `CLIENT_TIER_SPECIAL_BG` |
| `theme.rs` | `CLIENT_TIER_SPECIAL_TEXT` |
| `theme.rs` | `CLIENT_TIER_WHOLESALE_BG` |
| `theme.rs` | `CLIENT_TIER_WHOLESALE_TEXT` |
| `theme.rs` | `CONTROL_RADIUS` |
| `theme.rs` | `DANGER` |
| `theme.rs` | `DIVIDER` |
| `theme.rs` | `FAMILY_LIST_WIDTH` |
| `theme.rs` | `FRAME_STROKE_WIDTH` |
| `theme.rs` | `FREIGHT_ASSIGNMENT_COL_GAP` |
| `theme.rs` | `FREIGHT_ASSIGNMENT_MASTER_TABLE` |
| `theme.rs` | `FREIGHT_ASSIGNMENT_ROW_HEIGHT` |
| `theme.rs` | `FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH` |
| `theme.rs` | `FREIGHT_BULK_GROUP_WIDTH` |
| `theme.rs` | `FREIGHT_RATE_COL_WIDTHS` |
| `theme.rs` | `FREIGHT_RATE_TABLE_MIN_WIDTH` |
| `theme.rs` | `HEADER_DRAG_STRIP` |
| `theme.rs` | `HEADER_HEIGHT` |
| `theme.rs` | `INPUT` |
| `theme.rs` | `INPUT_HEIGHT` |
| `theme.rs` | `KAKAO_TOOLBAR_HEIGHT` |
| `theme.rs` | `KAKAO_TOOLBAR_PAD_X` |
| `theme.rs` | `LINKED_PREVIEW_BG` |
| `theme.rs` | `LINKED_PREVIEW_BORDER` |
| `theme.rs` | `LINKED_PREVIEW_BORDER_WIDTH` |
| `theme.rs` | `LINKED_PREVIEW_TEXT` |
| `theme.rs` | `MANAGEMENT_CHOOSER_ROW_HEIGHT` |
| `theme.rs` | `MANAGEMENT_CLIENT_ROW_HEIGHT` |
| `theme.rs` | `MANAGEMENT_CLIENT_SEARCH_MAX_WIDTH` |
| `theme.rs` | `MANAGEMENT_CLIENT_TABLE_MIN_WIDTH` |
| `theme.rs` | `MANAGEMENT_CLIENT_TAG_COLUMN_WIDTH` |
| `theme.rs` | `MANAGEMENT_CLIENT_TAG_TABLE_MIN_WIDTH` |
| `theme.rs` | `MANAGEMENT_CLIENT_TOOLBAR_STACK_WIDTH` |
| `theme.rs` | `MANAGEMENT_COMPACT_ROW_HEIGHT` |
| `theme.rs` | `MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH` |
| `theme.rs` | `MANAGEMENT_FILTER_DROPDOWN_WIDTH` |
| `theme.rs` | `MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING` |
| `theme.rs` | `MANAGEMENT_MODAL_BODY_MAX_HEIGHT` |
| `theme.rs` | `MANAGEMENT_SEARCH_MIN_WIDTH` |
| `theme.rs` | `MANAGEMENT_TABLE_HEADER_HEIGHT` |
| `theme.rs` | `MANAGEMENT_TABLE_TEXT_LEADING_INSET` |
| `theme.rs` | `MANAGEMENT_TABLE_VIEWPORT_FRACTION` |
| `theme.rs` | `MANAGEMENT_TOOLBAR_ACTIONS_WIDTH` |
| `theme.rs` | `MANUAL_COLUMN_GAP` |
| `theme.rs` | `MANUAL_DIALOG_MAX_WIDTH` |
| `theme.rs` | `MANUAL_DIALOG_SIDE_GAP` |
| `theme.rs` | `MANUAL_ITEMS_CARD_MIN_HEIGHT` |
| `theme.rs` | `MANUAL_SHEET_MIN_WIDTH` |
| `theme.rs` | `MANUAL_SOURCE_BLOCK_BORDER_WIDTH` |
| `theme.rs` | `MANUAL_SOURCE_CLASSIFIED_BORDER` |
| `theme.rs` | `MANUAL_SOURCE_CLASSIFIED_FILL` |
| `theme.rs` | `MANUAL_SOURCE_CLASSIFIED_TEXT` |
| `theme.rs` | `MANUAL_SOURCE_MIN_WIDTH` |
| `theme.rs` | `MANUAL_SOURCE_UNASSIGNED_BORDER` |
| `theme.rs` | `MANUAL_SOURCE_UNASSIGNED_FILL` |
| `theme.rs` | `MANUAL_SOURCE_UNASSIGNED_TEXT` |
| `theme.rs` | `MODAL_DECISION_MAX_WIDTH` |
| `theme.rs` | `MODAL_DEFAULT_MAX_WIDTH` |
| `theme.rs` | `MODAL_FOOTER_HEIGHT` |
| `theme.rs` | `MODAL_FORM_LABEL_WIDTH` |
| `theme.rs` | `MODAL_FORM_WIDE_LABEL_WIDTH` |
| `theme.rs` | `MODAL_HEADER_HEIGHT` |
| `theme.rs` | `MODAL_MASTER_CONTENT_INSET` |
| `theme.rs` | `MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT` |
| `theme.rs` | `MODAL_MASTER_WORKSPACE_GAP` |
| `theme.rs` | `MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT` |
| `theme.rs` | `MODAL_PANEL_PADDING_X` |
| `theme.rs` | `MODAL_PANEL_PADDING_Y` |
| `theme.rs` | `MODAL_SECTION_HEADER_RULE_GAP` |
| `theme.rs` | `MODAL_SECTION_PADDING` |
| `theme.rs` | `MODAL_VIEWPORT_GUTTER` |
| `theme.rs` | `MUTED` |
| `theme.rs` | `NAV_HEIGHT` |
| `theme.rs` | `NOTIFICATION_READ_ACTION_WIDTH` |
| `theme.rs` | `ONE_TOUCH_BORDER` |
| `theme.rs` | `ONE_TOUCH_BORDER_WIDTH` |
| `theme.rs` | `ORANGE_EMPHASIS_BG` |
| `theme.rs` | `ORDER_HISTORY_MASTER_TABLE` |
| `theme.rs` | `ORDER_HISTORY_TABLE_MIN_WIDTH` |
| `theme.rs` | `PANEL` |
| `theme.rs` | `PANEL_GAP` |
| `theme.rs` | `PRIMARY_ACTION_FILL` |
| `theme.rs` | `PRODUCT_CATEGORY_FILTER_TRIGGER_WIDTH` |
| `theme.rs` | `PRODUCT_DETAIL_IMAGE_SIZE` |
| `theme.rs` | `PRODUCT_DETAIL_MODAL_WIDTH` |
| `theme.rs` | `PRODUCT_TABLE_ACTION_WIDTH` |
| `theme.rs` | `PRODUCT_TABLE_MIN_WIDTH` |
| `theme.rs` | `PRODUCT_TABLE_NAME_MIN_WIDTH` |
| `theme.rs` | `PRODUCT_TABLE_OPTION_WIDTH` |
| `theme.rs` | `PRODUCT_TABLE_PRICE_WIDTH` |
| `theme.rs` | `PRODUCT_TABLE_ROW_HEIGHT` |
| `theme.rs` | `RAISED` |
| `theme.rs` | `RECEPTION_ITEMS_MIN_TABLE_WIDTH` |
| `theme.rs` | `RECEPTION_ITEMS_ROW_HEIGHT` |
| `theme.rs` | `RECEPTION_ITEM_REMOVE_WIDTH` |
| `theme.rs` | `RECEPTION_KEYCAP_BOTTOM_HIGHLIGHT` |
| `theme.rs` | `RECEPTION_KEYCAP_DEPTH` |
| `theme.rs` | `RECEPTION_KEYCAP_EDGE` |
| `theme.rs` | `RECEPTION_KEYCAP_FACE` |
| `theme.rs` | `RECEPTION_KEYCAP_HEIGHT` |
| `theme.rs` | `RECEPTION_KEYCAP_HIGHLIGHT` |
| `theme.rs` | `RECEPTION_KEYCAP_RADIUS` |
| `theme.rs` | `RECEPTION_KEYCAP_SIDE` |
| `theme.rs` | `RECEPTION_MESSAGE_ACTION_GAP` |
| `theme.rs` | `RECEPTION_ORDER_TOTAL_SECTION_GAP` |
| `theme.rs` | `RECEPTION_ORDER_TOTAL_SECTION_HEIGHT` |
| `theme.rs` | `RECEPTION_PANE_PADDING_Y` |
| `theme.rs` | `RECEPTION_RECIPIENT_SECTION_HEIGHT` |
| `theme.rs` | `RECEPTION_SECTION_CHROME_HEIGHT` |
| `theme.rs` | `RECEPTION_SECTION_DIVIDER_GAP` |
| `theme.rs` | `RECEPTION_SHORTCUT_ROW_HEIGHT` |
| `theme.rs` | `RECEPTION_TOTAL_HEIGHT` |
| `theme.rs` | `RECEPTION_VAT_LIST_MIN_VISIBLE_ROWS` |
| `theme.rs` | `RECEPTION_VAT_LIST_ROW_HEIGHT` |
| `theme.rs` | `RECEPTION_VAT_LIST_SCREEN_RESERVE` |
| `theme.rs` | `RECEPTION_VAT_LIST_VISIBLE_ROWS` |
| `theme.rs` | `RECIPIENT_LABEL_GAP` |
| `theme.rs` | `RECIPIENT_LABEL_WIDTH` |
| `theme.rs` | `RECIPIENT_MOBILE_INPUT_WIDTH` |
| `theme.rs` | `RECIPIENT_MOBILE_WIDTH_SHARE` |
| `theme.rs` | `RECIPIENT_PHONE_COLUMN_GAP` |
| `theme.rs` | `RECIPIENT_PHONE_INPUT_WIDTH` |
| `theme.rs` | `RECIPIENT_PHONE_LABEL_WIDTH` |
| `theme.rs` | `RECIPIENT_PHONE_ROW_MIN_WIDTH` |
| `theme.rs` | `RECIPIENT_ROW_GAP` |
| `theme.rs` | `REQUIRED_REVIEW_BORDER` |
| `theme.rs` | `REQUIRED_REVIEW_TEXT` |
| `theme.rs` | `SALE_STATUS_PREPARING` |
| `theme.rs` | `SALE_STATUS_SELLING` |
| `theme.rs` | `SALE_STATUS_SOLD_OUT` |
| `theme.rs` | `SALE_STATUS_STOPPED` |
| `theme.rs` | `SECTION_DIVIDER_GAP` |
| `theme.rs` | `SECTION_DIVIDER_HEIGHT` |
| `theme.rs` | `SECTION_PADDING` |
| `theme.rs` | `SELECTED_ORDER` |
| `theme.rs` | `SETTINGS_ACCOUNT_DELETE_WIDTH` |
| `theme.rs` | `SETTINGS_ACCOUNT_FIELD_WIDTHS` |
| `theme.rs` | `SETTINGS_ACCOUNT_LIST_MAX_HEIGHT` |
| `theme.rs` | `SETTINGS_ACCOUNT_LIST_MIN_HEIGHT` |
| `theme.rs` | `SETTINGS_ACCOUNT_PANEL_FIXED_HEIGHT` |
| `theme.rs` | `SETTINGS_ACCOUNT_PICKER_MAX_WIDTH` |
| `theme.rs` | `SETTINGS_ACCOUNT_ROW_MIN_WIDTH` |
| `theme.rs` | `SETTINGS_ACCOUNT_STACKED_EXTRA_HEIGHT` |
| `theme.rs` | `SETTINGS_BASIC_LEFT_PANE_WIDTH` |
| `theme.rs` | `SETTINGS_BASIC_SAVE_RESERVE` |
| `theme.rs` | `SETTINGS_BASIC_SPLIT_MIN_WIDTH` |
| `theme.rs` | `SETTINGS_HELP_ICON_SIZE` |
| `theme.rs` | `SETTINGS_OPERATOR_FIELD_WIDTH` |
| `theme.rs` | `SETTINGS_SENDER_ADDRESS_WIDTH` |
| `theme.rs` | `SETTINGS_SENDER_NAME_WIDTH` |
| `theme.rs` | `SETTINGS_SENDER_PHONE_WIDTH` |
| `theme.rs` | `SETTINGS_SENDER_POSTCODE_WIDTH` |
| `theme.rs` | `SETTLEMENT_DATE_FIELD_WIDTH` |
| `theme.rs` | `SETTLEMENT_ORDER_COL_WIDTHS` |
| `theme.rs` | `SETTLEMENT_PAYMENT_COL_WIDTHS` |
| `theme.rs` | `SOFT_DIVIDER_WIDTH` |
| `theme.rs` | `SOURCE_BLOCK_ADJACENT_GAP` |
| `theme.rs` | `SOURCE_BLOCK_FILL` |
| `theme.rs` | `SOURCE_BLOCK_HORIZONTAL_PADDING` |
| `theme.rs` | `SOURCE_BLOCK_RADIUS` |
| `theme.rs` | `SOURCE_BLOCK_ROW_INSET` |
| `theme.rs` | `SOURCE_CONTEXT_FILL` |
| `theme.rs` | `SOURCE_DOCUMENT_TEXT_MARGIN` |
| `theme.rs` | `SOURCE_RECOGNIZED_BORDER` |
| `theme.rs` | `SOURCE_RECOGNIZED_BORDER_WIDTH` |
| `theme.rs` | `SPACING` |
| `theme.rs` | `TEXT` |
| `theme.rs` | `WORKFLOW_ACTION_ROW_HEIGHT` |
| `theme.rs` | `WORKFLOW_AUX_CONTROL_HEIGHT` |
| `theme.rs` | `WORKFLOW_BAND_DIVIDER_HEIGHT` |
| `theme.rs` | `WORKFLOW_BOTTOM_HEIGHT` |
| `theme.rs` | `WORKFLOW_CARD_BG` |
| `theme.rs` | `WORKFLOW_CARD_GAP` |
| `theme.rs` | `WORKFLOW_CARD_PADDING_X` |
| `theme.rs` | `WORKFLOW_CARD_PADDING_Y` |
| `theme.rs` | `WORKFLOW_CARD_SLOT_GAP` |
| `theme.rs` | `WORKFLOW_CONTROL_VERTICAL_INSET` |
| `theme.rs` | `WORKFLOW_CONTROL_WIDTH` |
| `theme.rs` | `WORKFLOW_DETAIL_ROW_BG` |
| `theme.rs` | `WORKFLOW_MANAGE_WIDTH` |
| `theme.rs` | `WORKFLOW_SELECTED_BG` |
| `theme.rs` | `WORKFLOW_SELECTED_ROW_TEXT` |
| `theme.rs` | `WORKFLOW_SELECTED_STRIPE` |
| `theme.rs` | `WORKFLOW_SHIPPING_WIDTH` |
| `theme.rs` | `WORKFLOW_SIGNAL_BLOCK_BG` |
| `theme.rs` | `WORKFLOW_SIGNAL_BLOCK_BORDER` |
| `theme.rs` | `WORKFLOW_SIGNAL_BLOCK_HEIGHT` |
| `theme.rs` | `WORKFLOW_SIGNAL_BLOCK_RADIUS` |
| `theme.rs` | `WORKFLOW_SIGNAL_CURRENT` |
| `theme.rs` | `WORKFLOW_SIGNAL_CURRENT_BORDER` |
| `theme.rs` | `WORKFLOW_SIGNAL_DONE` |
| `theme.rs` | `WORKFLOW_SIGNAL_DONE_BORDER` |
| `theme.rs` | `WORKFLOW_SIGNAL_EMPTY` |
| `theme.rs` | `WORKFLOW_SIGNAL_EMPTY_BORDER` |
| `theme.rs` | `WORKFLOW_SIGNAL_GAP` |
| `theme.rs` | `WORKFLOW_SIGNAL_HEIGHT` |
| `theme.rs` | `WORKFLOW_SIGNAL_PENDING` |
| `theme.rs` | `WORKFLOW_SIGNAL_PENDING_BORDER` |
| `theme.rs` | `WORKFLOW_SIGNAL_RADIUS` |
| `theme.rs` | `WORKFLOW_SIGNAL_SIZE` |
| `theme.rs` | `WORKFLOW_SIGNAL_WIDTH` |
| `theme.rs` | `WORKFLOW_SUMMARY_BOTTOM_INSET` |
| `theme.rs` | `WORKFLOW_TITLE_HEIGHT` |
| `theme.rs` | `WORKFLOW_TITLE_LEADING_INSET` |
| `theme.rs` | `WORKFLOW_TITLE_TRAILING_INSET` |
| `theme.rs` | `WORKFLOW_TOGGLE_ICON_WIDTH` |
| `theme.rs` | `WORKFLOW_TRASH_WIDTH` |
| `theme.rs` | `WORKFLOW_UNDO_WIDTH` |
| `theme.rs` | `WORKFLOW_VAT_TEXT` |
| `theme.rs` | `WORKFLOW_VAT_TOP_INSET` |
| `typography.rs` | `BRAND_OPTICAL_CENTER_Y` |
| `typography.rs` | `DATA_NUMERIC_GLYPH_OFFSET_Y` |
| `typography.rs` | `FONT_METADATA` |
| `typography.rs` | `FONT_OCR_PROGRESS_TITLE` |
| `typography.rs` | `FONT_RECEPTION_ITEM_TEXT` |
| `typography.rs` | `FONT_RECEPTION_SOURCE_TEXT` |
| `typography.rs` | `METADATA_LINE_HEIGHT` |
| `typography.rs` | `RECEPTION_SOURCE_LINE_HEIGHT` |
| `typography.rs` | `WORKFLOW_VAT_FONT_SIZE` |
| `ui_tokens.rs` | `NAVIGATION_SNAPSHOT_TTL` |
| `ui_tokens.rs` | `SPACE_12` |
| `ui_tokens.rs` | `SPACE_24` |
| `ui_tokens.rs` | `SPACE_4` |
| `ui_tokens.rs` | `SPACE_8` |
| `ui_tokens.rs` | `WORKSPACE_SPLITTER_WIDTH` |
| `input/mod.rs` | `RECEPTION_MONEY_PENDING` |
| `input/mod.rs` | `RECEPTION_MONEY_UNIT_GAP` |
| `highlight/mod.rs` | `CONTROL_RADIUS` |
| `highlight/mod.rs` | `SOURCE_RADIUS` |
| `navigation.rs` | `EXCEL_EXPORT_DATE_CONTROLS_WIDTH` |
| `one_touch.rs` | `INK` |
| `ui_layout.rs` | `RECEPTION_STACKED_CARD_GAP` |
| `cell_navigation.rs` | `RECIPIENT_FIELDS` |

