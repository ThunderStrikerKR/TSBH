# 157차 업무용 드래그 감사

기준: f14a02e282f3f408bfc9e5b99d7287c963acab7b (main). GitHub 코드 검색은 drag_started 결과가 없었으므로 main.rs 모듈 선언을 기준으로 직접 조회했다. 현재 확인 가능한 현행 egui 업무용 입력 경로에 한정하며, 전체 저장소의 재귀 파일 목록·빌드·Windows 실행 결과는 확보하지 못했다.

| 경로 | 분류 | 처리 |
|---|---|---|
| category_tree/mod.rs, category_manager.rs, category_dropdown.rs | 상품/폴더 업무 이동 | 공통 native payload 발행, 타입 검증·드롭 강조, 커서, 이름/수량 preview. 650ms 및 폴더 확인은 유지 |
| manual_source_view.rs, manual_workspace.rs | 발주 원문을 수취인/품목 셀에 배정 | 공통 sense, 좌표 hit, 드롭 강조, 커서, preview painter. 원문 모양·press origin·업무 reducer 유지 |
| main.rs | 헤더 창 이동 및 가로/세로 splitter | 데이터 이동이 아닌 창 레이아웃 제스처. 기존 별도 경로 유지 |
| input/mod.rs | 입력 내부 dragged 상태 | 텍스트 선택/입력 상태, 유지 |
| scrollbar/mod.rs | native scroll | 유지 |
| 기타 직접 조회한 관리·접수 UI | 일치하는 업무용 drag API 없음 | 변경 없음 |

검색어: drag_started, drag_stopped, dnd_, click_and_drag, Sense::drag, dragged(), drag_preview, drag_payload. 직접 조회한 파일은 main, manual_workspace, manual_source_view, category_tree, category_manager, category_dropdown, source_highlight, item_editor, workflow_board_ui, freight_manager, model_group_manager, product_manager, kakao_dock, product_images, product_excel_ui, order_history_manager, client_manager, scrollbar, input, overlay, highlight, reception_service, review_panel, clipboard_intake, ocr_learning, client_directory, client_master, client_search_tags, automatic_message_manager, alias_manager, brand_manager, factory_reply_inbox, notification_center, client_chooser, reception_jobs, workflow_board, one_touch, theme, typography, layout, ui_layout, navigation, composite_workflow, composite_client, composite_reception, text_fit, window_state, status_notice, ui_primitives, display_format, ui_tokens, table, button, popup, card, dropdown, checkbox, ui_layout/geometry, settings_view, settings_help, daily_settlement_manager, excel_manager, order_history_detail, order_history_memo, catalog_query_bridge, business_state.

## 검증
PATCH: 소스와 감사 JSON 동시 반영. STATIC: 해당 소스 경로 및 호출 검토. UNIT: 포인터 preview 경계 테스트 작성, 실행 미확인. ACTION/CI/WINDOWS: NOT_VERIFIED. 전체 재귀 소스 검사 및 시각 검증을 완료 판정하지 않는다.
