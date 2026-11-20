# BANDO HUB Rust/egui — 두 마스터 기반 공통 UI 계약 초안

> 상태: **CONFLICT-001 C안 승인·두 마스터 코드 반영 / CONFLICT-002 결정 대기 / 정적 코드 재조회 완료 / 컴파일·실행·Windows DPI 미검증**
>
> 승인된 기준 화면은 **운임 관리 → 배송 그룹 편성**과 **주문 내역** 두 화면이다.
> 어떤 화면도 다른 화면의 모든 열 너비·데이터 행 구조를 무조건 복제하지 않는다.
> 동일 역할의 값은 공유하고, 업무상 다른 역할은 명시적 예외로 남긴다.
> **충돌하는 규격은 사용자가 선택하기 전까지 바꾸지 않는다.**

## 1. 소스 소유권

| 역할 | 공통 소유자 | 마스터 호출부 |
| --- | --- | --- |
| 페이지 제목 상단·가로 기준 | `components::management_page_title_top_gap`, `management_top_content` | `freight_manager.rs`, `order_history_manager.rs` |
| 주 콘텐츠 카드 상단·하단 | `components::align_management_primary_card`, `management_primary_card_min_inner_height`, `fill_management_primary_card`; `theme::management_primary_card_top_offset` | 두 화면 모두 |
| 섹션 표면·패딩 | `theme::section_frame`, `SECTION_PADDING` | 두 화면 모두 |
| 도구줄 오른쪽 정렬 | `components::management_toolbar_trailing_rect` (카드 내부 오른쪽 끝) | 두 화면 모두. 스크롤바 폭을 차감하지 않는다. |
| 도구줄→구분선→고정 헤더 | `components::management_sticky_table_header` | 두 화면 모두 |
| 드롭다운 | `components::management_dropdown`, `theme::INPUT`, 비활성 상태 토큰 | 두 화면의 기준 선택/배송 그룹 선택 |
| 고정 기본행 | `theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT` | 배송 그룹 상품 행, 주문 내역 대표행 |
| 스크롤 스타일 | `theme::management_scroll_style` | 두 화면 모두 |

## 2. 확인된 동일 역할의 수치 (egui 논리 pt)

| ID | 역할 | 현재 값 | 근거 |
| --- | --- | --- | --- |
| PAGE-TITLE-TOP | 관리 페이지 제목 시작 | 최종 16pt (작업영역 6pt 포함) | `management_page_title_top_gap` |
| PAGE-TITLE-TEXT | 페이지 제목 글꼴 | 20pt | `typography::FONT_PAGE_HEADING` |
| CARD-ANCHOR | 첫 주 목록 카드 상단 | 뷰포트 폭과 KPI 줄바꿈을 고려해 `management_primary_card_top_offset()`에서 결정 | `align_management_primary_card` |
| CARD-INSET | 카드 안쪽 패딩 | 16pt | `section_frame` |
| TOOLBAR-HEIGHT | 검색·필터 기본 도구줄 | 36pt (주문내역의 좁은 화면 다중 행은 명시적 예외) | `theme::INPUT_HEIGHT` |
| TOOLBAR-RIGHT | 도구줄 우측 끝 | **주 카드 내부 오른쪽 경계** (스크롤바 폭 미차감) | `management_toolbar_trailing_rect` |
| SELECT-HEIGHT | 관리형 드롭다운 | 34pt | `theme::ACTION_STANDARD_HEIGHT` |
| HEADER-HEIGHT | 표 헤더 | 36pt (하단 2pt 윤곽선을 내부에 포함) | `theme::MANAGEMENT_TABLE_HEADER_HEIGHT` |
| ROW-BASE-HEIGHT | 기본 한 줄 데이터행 | 42pt | `theme::FREIGHT_ASSIGNMENT_ROW_HEIGHT` |
| TOOLBAR-TO-HEADER | 도구줄 후 구분선 및 헤더까지 전환 | 17pt (8+4+1+4) | `management_sticky_table_header` |
| SCROLL-RAIL | 실선 스크롤 레일 | 폭 10pt, 핸들 최소 길이 44pt, 대상 두 화면의 좌우 추가 여백 0 | `management_scroll_style` 및 화면별 조정 |
| SCROLL-TRACK | 트랙 상단 | 행 본문 시작점 +3pt | 두 화면의 `scroll_bar_rect` |
| SELECT-ACTIVE-BG | 활성 드롭다운 배경 | #292929 | `theme::INPUT` |
| SELECT-DISABLED-BG | 비활성 드롭다운 배경 | #1E1E1E | `theme::MANAGEMENT_DROPDOWN_DISABLED_BG` |
| ACTION-DISABLED | 비활성 공통 작업 버튼 | 배경 #1E1E1E, 텍스트 #7C7C7C, 테두리 #373737 | `theme.rs` |

수치는 **소스의 선언·계산에 근거한 논리 포인트**이며, 실측 물리 픽셀값이라고 주장하지 않는다.

## 3. 합리적인 화면별 예외 (충돌 아님)

- **배송 그룹 편성:** 6열, 최소 표 너비 942pt. 현재 코드의 열 폭은 `[44,202,104,258,150,144]`pt, 열 사이 8pt×5. 남는 공간은 상품명 64%, 카테고리 36%로 추가 배분한다. 상품행은 42pt 고정이며 `virtual_fixed_rows`를 사용한다.
- **주문 내역:** 7열, 최소 표 너비 1242pt. 현재 코드의 열 폭은 `[112,156,300,126,124,112,264]`pt, 열 사이 8pt×6. 남는 폭은 상품 열로 배분한다. 대표행은 42pt이고 펼친 상세행은 내용에 따라 가변 높이를 사용한다.
- **가로 스크롤:** 배송 그룹 상품표는 `ScrollArea::both()`에 실제 필요한 스크롤바만 표시하며, 주문 내역은 너비가 최소보다 좁은 경우에만 양축 스크롤을 선택한다. 업무 데이터와 가변행 처리 차이로 구분한다.
- **툴바 줄바꿈:** 주문 내역은 조회 기간 도구를 좁은 화면에서 다음 줄로 내리지만, 배송 그룹 편성은 검색/일괄 작업을 현재 한 줄로 유지한다. 좁은 화면의 실제 검수는 별도다.

## 4. 충돌 결정 및 보류 항목

### CONFLICT-001: 도구줄 우측 정렬 — **C안 승인 및 코드 반영**

- **사용자 확정:** 두 마스터의 마지막 도구줄 컨트롤은 모두 **주 카드 내부 오른쪽 끝**에 맞춘다. 표 스크롤바의 별도 레일 폭을 차감하지 않는다.
- **배송 그룹 편성:** 기존 `toolbar_data_width`에서 스크롤바 할당 폭을 제외하던 계산을 제거했다. 전체 도구줄 폭을 기준으로 오른쪽 일괄 조작 슬롯을 배치한다.
- **주문 내역:** 기존 카드 오른쪽 정렬을 유지하면서 공통 슬롯 좌표 함수를 호출하도록 통일했다. 좁은 화면의 도구줄 줄바꿈은 유지한다.
- **공통 소유권:** `components::management_toolbar_trailing_rect(toolbar, width, y_offset, height)`. 오른쪽 끝이 `toolbar.right()`와 일치해야 하며 도구줄 높이 안에 있어야 한다. `debug_assert!`와 한 개의 집중된 기하 테스트를 추가했다(아직 실행되지 않음).
- **독립적인 표 규칙:** `management_sticky_table_header`는 기존처럼 헤더·데이터 너비에서 스크롤바 공간을 예약한다. 도구줄과 표의 마지막 데이터 열이 일치하지 않는 것은 C안에 따른 의도적 결과다.

### CONFLICT-002: 과거 UI 규칙 문서와 현행 마스터 소스의 열 치수 불일치

- 배송 그룹 편성: 기존 `docs/egui-management-ui-reference.md`는 상품명 218pt/설정 128pt로 적지만 **현행 theme.rs**는 상품명 202pt/설정 144pt다.
- 주문 내역: 기존 `docs/management-layout-rules.md`는 처리 열 132pt/최소 1110pt로 적지만 **현행 order_history_manager.rs**는 처리 열 264pt/최소 1242pt다.
- 승인된 **실제 실행 화면**과 과거 문서 중 어느 값이 사용자 승인 최종본인지 확인하기 전에는 옛 숫자로 코드를 되돌리거나 문서를 승인본으로 단정하지 않는다.

## 5. 수치 기반 검증 구현 계획 — 아직 미구현

- **1차 구현 완료:** 공통 trailing slot의 오른쪽 끝 및 높이 경계는 `debug_assert!`로 검증한다. `management_toolbar_geometry_tests`의 단일 집중 테스트가 카드 폭 400/704/1280pt와 줄바꿈 슬롯을 비교한다. 실제 컴파일 및 실행은 확인되지 않았다.
- **후속 과제:** 공통 레이아웃이 반환하는 `Rect`에 명시적인 역할 ID를 부여한다. 예: `page.title`, `card.outer`, `toolbar`, `table.header`, `table.data_right`, `scroll.track`, `button.primary`.
- 동일 역할의 `min.x/min.y/width/height`를 **같은 크기·같은 배율**에서 비교한다. 다른 길이의 텍스트와 서로 다른 열 콘텐츠는 동일 좌표 대상으로 취급하지 않는다.
- 고정 불변식은 `header.height=36pt`, `base_row.height=42pt`, `toolbar.height=36pt`, `dropdown.height=34pt`, `header.data_right=body.data_right`, `scroll.track.top=rows.top+3pt` 등이다.
- `points_per_pixel` 또는 `pixels_per_point`를 함께 기록하고 `실제 px = 논리 pt × pixels_per_point`로 구분한다. 정수 픽셀 반올림과 글리프 실제 잉크 위치를 구분한다.
- 테스트는 대량의 업무 회귀테스트가 아니라 **공통 UI 좌표 진단 1종 및 선택된 화면 샘플**부터 추가한다. 실제 Windows 캡처 및 100/125/150% DPI는 코드 정적 확인과 별도 보고한다.

## 6. 다음 구현 순서

1. **완료:** 충돌 001의 C안 결정과 공통 좌표 함수·두 마스터 호출부 반영
2. **결정 대기:** 충돌 002의 기존 문서 열 폭 vs 최신 소스 열 폭의 최종 승인 기준 확인
3. 충돌 002가 확정된 뒤, 구체 수치를 정식 SSOT 규격에 승격하고 오래된 문서와 맞추기
4. 나머지 동일 역할도 공통 토큰·컴포넌트로 연결하고 필요 예외만 선언
5. 실제 렌더링 시 역할별 `Rect`와 `pixels_per_point`를 수집해 다중 DPI 좌표 리포트를 구현
6. 다른 메뉴의 이탈 항목을 역할 단위로 순차 이전

> **현재 결과는 설계·감사 기준선 문서 작성이다.** 이번 문서 자체가 런타임 좌표 비교기나 컴파일·실화면 통과를 의미하지 않는다.
