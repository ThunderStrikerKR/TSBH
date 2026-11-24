# 반도허브 egui 수동 분류 기능·입력 레이어 검수 — 79차

## 현상과 재현

사용자 화면: 수동분류(넓은 작업 모달) 안에 「수동 분류 변경 취소」 확인창이 열리고, 확인창의 상단 취소 및 하단 변경 취소를 포함한 모든 버튼 입력이 먹지 않아 화면에서 빠져나올 수 없음. 미확정 발주원문과 구성표는 뒤에 보임.

## 원인 추적

- `ManualWorkspace::show`가 자식 확인창이 열리면 부모 `ui.disable()`을 적용한 것은 정당한 입력 보호 동작.
- 그런데 자식 위에 자체 Foreground 전체화면 가림막(`modal_backdrop_for_window`)을 다시 띄운 뒤 수동으로 입력 레이어를 재정렬했다. egui 0.33 레이어의 이전 프레임 히트 테스트에서 가림막이 확인창 입력을 차단할 위험이 있다.
- 상위 애플리케이션은 이미 전체 화면 Middle `modal_backdrop`을 렌더하고 있으므로 수동분류의 두 번째 Foreground 가림막은 불필요하다.
- `modal_backdrop_for_window` 함수는 다른 기존 화면과 테스트를 위해 보존하되, 수동분류 동작 경로에서는 호출하지 않는다. 자식 Window 생성 후 `modal_foreground_for_window`로 실제 창만 top/modal 대상으로 지정한다.
- 모든 작업 모달을 강제 종료시키거나 입력 포커스를 무조건 해제하는 임시 처방은 사용하지 않는다.

## 발주원문 ↔ 구성표 연결 검수

- React: `src/orders/ManualClassification.tsx`, `src/orders/ManualPrompt.tsx`, `src/SourceDocument.tsx`.
- egui: `manual_workspace.rs`, `manual_source_view.rs`, `source_highlight.rs`, `components.rs`.
- 원문 텍스트 범위는 Rust 분류의 UTF-16 오프셋을 UTF-8/egui 커서로 정확히 변환하며, 텍스트 검색으로 중복 이름 위치를 추측하지 않는다.
- `collect_manual`은 수동 배정 후 변경된 수취인/상품 대상을 반영한다. `manual_hover_target`과 공통 `HoverState`가 양방향 호버와 원문 자동 스크롤을 연결한다. 진입 50ms·해제 150ms 유지.
- 기존에는 원문에 공통 회색/파란 블록이 있었지만 배송 구성표 대상은 파란 테두리만 추가했다. `manual_linked_table_cell`을 공통 구성요소로 도입하여 연결 대상 셀에 원문과 동일한 파란 배경, 글자색 및 2pt 테두리를 적용한다. 비연결 셀의 선택 표시(기존 주황색)는 보존한다.

## 흐름과 UX 개선

1. 수동분류 상단 버튼을 `닫기`로 명확히 표기한다.
2. 변경사항이 없으면 즉시 닫고, 변경된 모델/미반영 입력이 있을 때만 버리기 확인을 띄운다.
3. 확인창의 `계속 편집`은 자식 창만 닫으며 수정 초안을 유지한다. `변경사항 버리고 닫기`는 Rust 모델을 접수 초안에 적용하지 않은 상태로 편집기를 종료한다. Esc 역시 최상단 단계부터 해제한다.
4. 이미 `초안에 적용` 명령이 존재하므로 `수동 분류 완료 → 피드백 없이 진행` 중복 확인을 제거한다. 적용 가능 조건(비동기 처리 완료, 입력 반영 완료, 상품 수량 확정)은 유지한다.
5. 상단 헤더에는 `미배정 원문 N개`만 표시한다. `초안 적용 후 주문 확인 필요` 같은 일반 안내는 제거한다. 실제 적용 차단 사유(`변경사항 반영 중`, `수량 확인 필요 N건`, `선택한 입력을 먼저 반영하세요`)는 카운트/적용 버튼의 툴팁으로 제공하며, Rust 최종 검증과 적용 가능 조건은 그대로 유지한다. 조건부 상태문구가 헤더 높이나 다음 패널 위치를 바꾸어서는 안 된다.
6. `발주원문 · 항목 클릭으로 연결/해제`라는 동작 안내를 노출한다.

## 데이터 및 책임 경계

- 원본 발주서, 최초 Rust 인식 증거, 수동배정 순서, 되돌리기 이력, 원본 품목·수량 기록은 보존한다.
- `초안에 적용` 시 Rust `apply_manual` reducer를 그대로 호출하며, 수동분류는 주문 확정/발송이나 결제 완료를 수행하지 않는다.
- 자동 상품·수량 계산을 UI에서 새로 구현하지 않는다.

## 회귀 검증 대상

- Windows `cargo check` 및 `manual_nested_prompt_regression_tests`: 실제 포인터 이동/누름/뗌으로 두 확인 액션 클릭, 부모 입력 차단, 연결 셀의 파란 배경·테두리
- `modal_input_routing_tests`: 기존 모달 진입 테스트를 현재 가림막 없는 구조와 순차 입력 타임스탬프로 맞춤
- `manual_workspace_modal_routing_tests`: Esc 우선순위, 수량 승인·작업 이력·메뉴 ID, 적용 흐름
- `manual_source_view::tests` 및 `source_highlight::tests`: 원문 클릭/호버, 한글/이모지 문자 경계, 중복 텍스트 증거, 대상 연결
- `manual_pending_interaction_tests`, `manual_remote_row_tests`: 상품 수량 확정·도서산간 대상 일치
- 작업 대상은 `TSBH/main`이다. 모든 UI 픽셀 정렬과 실제 사용자 운영 DB에 대한 검증은 CI 단위 테스트와 별도로 확인해야 한다.

## 남은 React 대비 차이

React는 각 구성표 입력 셀에서 직접 문자열을 수정할 수 있으나, egui의 현재 수동분류 구성표는 표에서 셀을 선택한 뒤 하단 단일 입력창으로 변경·반영하는 구조다. 이는 사용성과 데이터 처리 로직을 크게 바꾸는 작업이므로 이번 모달 복구와 혼합하지 않았다. 우선 잠김/데이터 손실 회귀를 방지하고, 직접 셀 편집은 별도 검수 항목으로 관리한다.

## 원문 호버 실제 코드 오류 추가 발견

- 실제 Windows 테스트 로그에서 수동 원문 텍스트의 egui Response는 `hovered=true`이나 `interact_pointer_pos()=None`을 반환했다. 순수 호버에는 상호작용(누름/드래그) 좌표가 없을 수 있으므로 원문 링크 검색이 작동하지 않았다.
- `manual_source_view::source_document`는 위젯의 `response.hovered() || response.clicked()` 보호 조건을 통과할 때 `interact_pointer_pos()`를 우선 사용하고, 비어 있으면 `input.pointer.hover_pos()`를 사용한다.
- 장애물 뒤/비활성 화면에서 포인터 위치만으로 링크가 활성화되지 않도록 위젯의 hovered/clicked 조건은 유지한다.
- UI 색상 변경만으로 해결되는 문제가 아니며, 이 좌표 수정이 원문↔구성표 호버 동작의 필수 수정이다. Windows 실제 포인터 이벤트 테스트를 통해 검증한다.

## 검증 결과 (Windows 실시간 CI)

- [수동분류 모달·하이라이트 회귀 검증](https://github.com/ThunderStrikerKR/TSBH/actions/runs/45218790023) — `success`, 검증 대상 구현 커밋 `224249d3`
- egui `cargo check`: 성공
- 실제 마우스 누름/뗌, 자식 확인창 두 버튼, 부모 입력 차단, 파란 연결 셀: 성공
- 수동분류 Esc·취소·적용 가드 및 UI 흐름: 성공
- 원문 링크의 마우스 호버·클릭(순수 호버 좌표 결손 수정): 성공
- source span 소유자·UTF-16 경계·동일 ID 연결·입출 지연: 성공
- 결과는 Windows CI의 egui 컴파일·headless 포인터 시뮬레이션 검증이다. 사용자 운영 화면과 DB를 직접 연 테스트는 아니므로 별도 스크린샷 확인을 최종 UI 판정으로 남긴다.


수동 분류 상단의 일반 안내문구 제거 및 egui 전체 조건부 상태 슬롯 전수조사 결과는 [`egui-conditional-status-layout-audit-20261124.md`](egui-conditional-status-layout-audit-20261124.md)를 따른다.


## 86차 추가 검수: 접수 마스터와 수동분류의 분기 재발 방지 (2026-11-24)

**상태: 코드 반영 / CI 및 Windows 실제 화면 검증 미완료.** 사용자가 제공한 비교 화면에서는 접수의 품목 금액과 X 삭제 버튼, 운임과 수동분류의 단가·삭제 버튼 누락·운임 미표시가 서로 달랐다. 이미 공통 카드/수취인 필드가 존재했지만 상위 호출부가 이전 품목 렌더러와 독립 수량/상품추가 경로를 사용했다. 기존 회귀 검사는 단순히 어떤 공통 렌더러가 사용되는지만 확인해 최신 마스터와의 차이를 검출하지 못했다.

- 접수/수동분류 편집용 품목 표는 `reception_editable_items_header`(금액 + 삭제 슬롯), `reception_item_row_with_remove`, `reception_item_numeric_editor`로 고정한다. 이전 삭제 슬롯 없는 편집용 행 함수 및 자유 헤더 API는 제거/비공개화했다. 수량/금액 편집 세부 함수도 공통 디스패처 내부로 제한한다.
- 상품 추가는 `ProductManager::show_selection_modal`과 `add_selected_manual_product` 공통 경로를 사용한다. 선택 전 빈 행 저장 금지, 기본 수량 1, 취소 무변경. 품목 삭제 확인은 공통 `reception_item_delete_confirmation`을 사용한다.
- 수동분류는 `preview_manual` → `CalculateReceptionQuote`의 읽기 전용 Rust 결과에서 접수와 동일한 품목 금액 및 적용 운임을 가져온다. UI 측 곱셈/운임 추측 금지. 가격 입력 및 기준 스냅샷이 동일하면 기존 작업 견적을 재사용하고, 되돌리기 시 일치하는 견적도 복원한다. 원문 배정, 수동 초안과 최종 적용 경계는 보존한다.
- 수동분류의 도서산간 안내는 기존 상단 상태의 툴팁으로 옮기고 품목/운임 내부의 추가 상태 줄을 제거해 접수와 같은 간격을 유지한다. 수동분류 고유 원문 배정과 배송 건수 컨트롤은 예외로 보존한다.
- `manual_cards_reuse_the_real_reception_renderers`는 두 실제 화면의 최신 편집용 API를 비교하고 구형 함수, 별도 수량 스텝, 견적 우회 경로를 막는다. 공통 컴포넌트/상품관리/삭제 팝업/운임 엔진 변경에도 기존 Windows 검사 워크플로가 실행된다.
- 완료 기준: Windows 1920×1080 및 좁은 작업영역에서 동일 주문으로 접수/수동분류 나란히 비교. 헤더·삭제 슬롯·수량/금액 편집·원 단위·운임·선착불·발송지·원문 호버·팝업 취소·초안 적용 검증. 실제 캡처와 정확한 커밋을 확보하기 전 VISUAL PASS 금지.
