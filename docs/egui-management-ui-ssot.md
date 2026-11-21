# BANDO HUB — 관리 UI 정식 단일 규격 (SSOT)
 
> **확정 기준:** 2026-11-21, 두 공동 마스터 **운임 관리 → 배송 그룹 편성** / **주문 내역**.
> CONFLICT-001은 **C(도구줄 오른쪽 끝 = 카드 내부 오른쪽 끝)**, CONFLICT-002는 **현행 소스의 열 너비·최소 폭 유지**로 확정.
> **규격 원본은 Rust 소스**다. 이 문서는 해당 소스의 역할·소유권·허용 예외를 설명하는 **규범적 색인**이며 독립적인 두 번째 숫자 원본이 아니다. 문서와 소스가 달라지면 추측해 합치지 말고 사용자가 승인한 최신 결정과 실행 상태를 확인한다.

## 1. 적용 경계와 소유권

- 적용 기준은 `egui-app`의 Rust/egui **관리 화면**이다. 이전 React/CSS 화면의 숫자를 자동 이식하지 않는다.
- 같은 **역할**이면 수치와 컴포넌트를 공유한다. 역할이 다른 레이아웃은 별도의 명명된 규격을 둔다. 화면 파일에 같은 상수값을 새로 적거나, 기존 디자인 위에 1~3pt 임시 보정을 누적하지 않는다.
- 원본 코드: **`egui-app/src/theme.rs`**(색·간격·크기·마스터 표 사양), **`egui-app/src/typography.rs`**(서체·크기·행간·문자 광학 보정), **`egui-app/src/components.rs`**(공통 Rect 계산·컨트롤·고정 헤더·화살표).
- 두 공동 마스터의 실제 소비자: `egui-app/src/freight_manager.rs`와 `egui-app/src/order_history_manager.rs`. 업무 판단·DB·송장·저장은 이 디자인 계약으로 변경하지 않는다.
- 역사적 결정 기록: `egui-dual-master-ui-contract-draft.md`, 화면의 업무별 예외 참고: `egui-management-ui-reference.md` 및 `management-layout-rules.md`. **수치가 중복되면 이 문서에서 지정한 Rust 토큰을 단일 원본으로 취급**한다.

## 2. 역할별 공통 토큰

이 표의 숫자 단위는 **egui 논리 pt**다. 물리 px 실측값이 아니다.

| 역할/계약 ID | 확정 수치·정책 | 코드 소유자 |
| --- | --- | --- |
| 전역 메뉴 | 외곽 헤더 62pt, 상위 메뉴 44pt·글씨 18pt | `theme::HEADER_HEIGHT`, `NAV_HEIGHT`; `typography::FONT_MAIN_NAV` |
| 관리 페이지 제목 | 글씨 20pt, 작업영역 기준 상단·좌측 최종 16pt(기존 6pt 외곽 포함) | `typography::FONT_PAGE_HEADING`; `components::management_page_title_top_gap` / `management_top_content` |
| 하위 관리 탭 | 글씨 16pt, 버튼 높이 38pt | `typography::FONT_MANAGEMENT_TAB`; `theme::MANAGEMENT_TAB_HEIGHT` |
| 기본 상단 여백 리듬 | 제목→탭 8pt, 탭→KPI 12pt, KPI→첫 카드 16pt(각 요소가 실제 존재할 때) | `theme::management_primary_card_top_offset()` 및 상단 구성 함수 |
| 주요 카드 외곽 | 두 마스터의 첫 카드 상단을 동일한 기준식으로 계산. KPI 줄바꿈에 따라 기준 높이 가변; 한 줄 KPI에서 페이지 원점부터 216pt | `components::align_management_primary_card`; `theme::management_primary_card_top_offset` |
| 주요 카드 안쪽 | **일반 섹션 카드:** 상하좌우 16pt. **단일 목록 마스터 두 화면:** 상하좌우 **16pt**(데이터 행은 하단 16pt 위에서 종료, 스크롤 레일만 4pt 더 내려와 외곽선까지 12pt 유지), 테두리 1pt. 외곽 카드 크기·위치는 유지 | `theme::section_frame()`, `management_master_table_frame()`, `MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING`, `components::management_master_table_card_min_inner_height()` |
| 검색·필터 도구줄 | 기본 높이 36pt; 필드/드롭다운은 해당 역할의 높이로 가운데 정렬 | `theme::INPUT_HEIGHT`, `components::management_toolbar_trailing_rect` |
| **도구줄 오른쪽 끝** | **주 카드 내부 오른쪽 끝**과 일치, 스크롤바 폭을 차감하지 않음 (**CONFLICT-001=C**) | `components::management_toolbar_trailing_rect()` |
| 기본 컨트롤 | 텍스트 입력 36pt, 일반 선택·작업 버튼 34pt, 컨트롤 모서리 반경 6pt | `theme::INPUT_HEIGHT`, `ACTION_STANDARD_HEIGHT`, `CONTROL_RADIUS` |
| 컨트롤 색상 | 활성 입력/드롭다운 `#292929`, 활성 일반 작업 버튼 `#303030`; 비활성 공통 컨트롤 `#1E1E1E`, 비활성 글씨 `#7C7C7C`, 테두리 `#373737` | `theme::INPUT`, `RAISED`, `MANAGEMENT_DROPDOWN_DISABLED_BG`, `DISABLED_ACTION_*` |
| 공통 드롭다운 | 34pt 버튼, 왼쪽 캡션 패딩 12pt, 오른쪽 화살표 슬롯 30pt, 공통 V 도형(너비 10pt, 굵기 1.8pt) | `components::management_dropdown`, `paint_management_chevron`, `typography::MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y` |
| 주문 내역 날짜 도구줄 | 36pt 도구줄·날짜 그룹, 이전/다음 28pt 대칭 슬롯, 달력 36pt 슬롯, `~` 16pt 중앙 슬롯, 바깥 요소 간격 8pt. 34pt 드롭다운·이동 버튼·일반 작업 버튼은 36pt 행 중심에 배치 | `theme::MANAGEMENT_DATE_*` / `order_history_manager::range_date_picker_rtl`, `history_date_controls` |
| 테이블 헤더 | 높이 36pt, 하단 윤곽선 2pt를 **내부**에 포함, 헤더·본문 가로 경계 동일 | `theme::MANAGEMENT_TABLE_HEADER_HEIGHT`, `components::management_sticky_table_header` |
| 단일행 목록 | 높이 **42pt**. 같은 역할의 상품 목록·거래처 목록·배송그룹 편성·주문 대표행은 공통 토큰 참조 | `theme::MANAGEMENT_COMPACT_ROW_HEIGHT`, 관련 기존 상수는 **별칭** |
| **단일 목록 스크롤 높이 (CONFLICT-004=A)** | 카드 아래 경계 − 표 본문 시작 위치 − 하단 필수 영역 예약 높이. 화면 전체 60% 일괄 제한 금지. 복수 표·펼침 상세·편집 패널은 예외 | `components::management_single_list_scroll_height`; `ExcelTableHeightPolicy` |
| 두 줄·상세형 목록 | 길이와 정보 구조에 따른 **별도 역할**, 단일행 42pt로 억지 통일 금지 | 예: `theme::MANAGEMENT_TABLE_ROW_HEIGHT`(58pt); 아래 예외 표 |
| 도구줄→고정 헤더 (CONFLICT-005) | **총 21pt = 16pt 상단 여백 + 1pt 구분선 + 4pt 아래 여백**. 도구줄 프레임이 이미 8pt 하단 패딩을 제공하면 **기존 8pt + 추가 8pt + 1pt + 4pt**로 계산. 자동 세로 간격 중첩 금지. **마지막 도구줄과 표 헤더가 직접 연결되는 동일 역할에만 적용** | `theme::MANAGEMENT_TOOLBAR_DIVIDER_*_GAP`, `components::management_sticky_table_header`, `management_sticky_table_header_after_inset` |
| **표 스크롤 레일 — 단일 공통 정책** | **모든 관리 표 하단 연장 입력값 7pt** (광학 보정 3pt, 본문 종료점 아래 실제 4pt). 전용 solid 스크롤바 10pt·핸들 최소 44pt·좌우 추가 여백 0pt, 헤더 아래 시작 간격 0pt. `management_table_scroll_track_rect(ui,scroll_height)`가 **화면 역할과 무관하게** 단일 토큰 `MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION`을 사용한다. 페이지별 `Master / Section / List / Embedded` 스크롤 연장값 분기는 폐지. **푸터·상세·복수 표는 본문 높이와 예약 공간만 별도로 관리**하며 스크롤바 연장값을 달리하지 않는다. 표 행 높이와 가상 스크롤 범위는 불변 | `theme::MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION`, `theme::management_table_scroll_style()`, `components::management_table_scroll_track_rect()` |
| **표의 데이터 오른쪽 끝** | **헤더 외곽 오른쪽 끝 − 표 스크롤바 할당폭**, 도구줄의 우측 끝과 동일하게 강제하지 않음 | `components::management_sticky_table_header` |
| 폰트 역할 | 페이지 20pt, 섹션 16pt, 표 헤더/본문 14pt, 표준 작업 버튼 13pt | `typography.rs`의 역할별 토큰·spec |

## 3. 마스터 테이블 수치: 코드가 강제하는 원본

두 표는 같은 공통 헤더·기본 행·스크롤 문법을 사용하되 업무 열 구조는 **서로 다른 타입 사양**이다.

| 마스터 | 기본 열 너비(왼쪽→오른쪽, pt) | 열 간격 | 최소 폭 | 남는 데이터 폭 |
| --- | --- | --- | --- | --- |
| 배송 그룹 편성 | **44 / 202 / 104 / 258 / 150 / 144** | 8pt ×5 | **942pt** | 상품명 열 **64%**, 카테고리 열 **36%** |
| 주문 내역 | **112 / 156 / 300 / 126 / 124 / 112 / 264** | 8pt ×6 | **1242pt** | 배송 건/상품 열 **100%** |

- **유일한 선언**: `theme::FREIGHT_ASSIGNMENT_MASTER_TABLE` / `theme::ORDER_HISTORY_MASTER_TABLE`의 `base_columns`, `min_width`, `growth_shares`.
- **유일한 열 폭 산출**: `ManagementMasterTableSpec::widths_for(measured_data_width)`. 이 입력은 **스크롤바 할당폭을 제외한 실제 표 데이터 너비**이며 도구줄 너비를 넣지 않는다.
- 기존 상수 `FREIGHT_ASSIGNMENT_COL_WIDTHS`, `FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH`, `ORDER_HISTORY_COL_WIDTHS`, `ORDER_HISTORY_TABLE_MIN_WIDTH`는 중복 값이 아닌 **중앙 사양의 별칭**이다.
- **CONFLICT-002 확정**: 승인된 현행 코드 값을 보존한다. 옛 규격의 배송그룹 상품명 218/설정 128pt, 주문 처리 132pt/최소 1110pt를 다시 사용하지 않는다.
- 정식 코드의 `ManagementMasterRowPolicy`와 `ManagementMasterToolbarPolicy`를 아래 예외의 선언형 정책으로 사용한다.

## 4. 화면별 명시적 예외 등록부

| 예외 ID | 대상 | 승인된 차이 | 바꾸면 안 되는 공통 기준 |
| --- | --- | --- | --- |
| EX-FREIGHT-001 | 운임관리 → 배송그룹 편성 | `row_policy=Fixed`: 상품별 데이터는 42pt 고정·가상 스크롤. 선택·상품명·색상/옵션·카테고리·그룹·설정 6열 | 카드 기준선, 헤더 36pt, 툴바 우측 카드 끝, 공통 드롭다운 |
| EX-FREIGHT-002 | 운임관리 → 배송그룹 편성 | `toolbar_policy=SingleLine`: 검색/그룹 필터/일괄 적용 도구줄을 기본 1행으로 유지 | 마지막 컨트롤의 오른쪽 끝은 카드 안쪽 끝 |
| EX-HISTORY-001 | 주문 내역 | `row_policy=ExpandableDetails`: 주문 대표행 42pt, 펼친 배송 상세행은 내용에 맞게 가변 높이·prefix-offset 기반 가상화 | 기본행 높이·헤더 정렬·스크롤 기준 |
| EX-HISTORY-002 | 주문 내역 | `toolbar_policy=ResponsiveDateWrap`: 좁은 화면에서 날짜 필터만 다음 줄로 이동. 날짜 기준과 기간 필드의 기존 정책 유지 | 각 줄의 마지막 컨트롤은 카드 안쪽 끝 |
| EX-MASTER-003 | 두 화면 각각 | 열 개수·최소 폭·잉여 폭의 분배 규칙은 **표 사양마다 다름** | 스크롤바 차감 좌표, 헤더·본문 같은 열 경계 |
| EX-OTHER-004 | 그 외 관리 화면 | 다중 카드/좌우 편집 패널, 발주서 상세/팝업, 결산 표, 두 줄 선택 목록 등은 해당 역할이 다르므로 42pt 단일행/942pt 6열을 강제하지 않음 | **같은 역할**을 재사용할 때만 공통 토큰·컴포넌트 준수 |

예외는 **존재하지 않는 가짜 탭·KPI 카드·빈 공간을 추가**하는 근거가 아니다. 다른 화면으로 규격을 확장할 때는 예외 ID·대상·사유·재사용할 기본 토큰을 먼저 적고, 승인되지 않은 새로운 디자인 값은 만들지 않는다.

## 5. 검증과 변경 통제

1. 공통 UI 값 변경 요청은 **역할 ID → 코드 토큰/함수 → 영향받는 화면 → 예외 충돌 여부** 순서로 평가한다.
2. 변경하려는 속성이 두 마스터에서 **같은 역할인데 서로 다른 값을 요구하면 사용자에게 먼저 충돌 결정**을 받는다. 예외를 승인 없이 지우거나 임의 보정값으로 덮지 않는다.
3. 소스에서 가벼운 검증을 수행한다. 현재 `theme.rs`에는 **두 마스터 최소 열 폭 = 열 너비 합 + 간격**, 잉여 폭 분배 합계 100%, 예외 정책 및 대표 너비 계산을 검사하는 집중 테스트를 정의했다. `components.rs`에는 도구줄 우측 끝·헤더 36pt·스크롤바 데이터 경계의 기하 불변식이 있다.
4. **컴파일·테스트 성공을 주장하려면 실제 실행 결과가 필요**하다. 테스트를 작성한 것과 실행한 것은 다르다. 이전에 실패한 대량 업무 회귀테스트를 UI 작업을 이유로 무작정 재도입하지 않는다.
5. 사용자 Windows 화면의 100%·125%·150% 배율에서 검수할 때 `pixels_per_point`도 기록한다. **물리 픽셀 = 논리 pt × pixels_per_point**; 글리프 잉크와 위젯 `Rect`의 시각적 중심은 별도로 확인한다.
6. **공동 마스터에서 다른 관리 화면으로 규격을 확장했다.** 실제 수정된 화면·기존값을 보존한 예외·사용자 결정이 필요한 충돌은 [타 화면 적용 기록](egui-management-ssot-rollout-20261121.md)에 기록한다. 전체 화면의 실시간 좌표를 수집·자동 대조하는 시스템과 Windows 실화면 판정은 여전히 별도 **Q2/Q3** 범위이며, 정적 연결 확인을 전체 UI 완전 통일로 보고하지 않는다.

## 5.1. 타 화면으로 SSOT 확장한 공통 소유권 (2026-11-21)

- `components::management_list_table_header`: 거래처·상품·Excel·일일 결산·운임 그룹/이력 등 일반 관리 목록의 **36pt 고정 헤더와 데이터 폭**을 계산한다. **CONFLICT-003 A안 확정:** 기존 추가 4pt를 제거하고, 실제 스크롤바 할당폭만 예약한다. 공동 마스터의 `management_sticky_table_header`와 내부의 `management_table_data_width` 계산식을 공유한다. CONFLICT-005 확정에 따라 **직접 연결되는 도구줄→표 헤더**는 `management_sticky_table_header` 또는 이미 존재하는 하단 8pt를 차감하는 `management_sticky_table_header_after_inset`으로 처리한다. 중간에 상태 메시지·독립 편집 영역이 있는 경로는 `management_list_table_header`를 유지한다.
- `theme::management_weighted_columns` 및 `management_single_stretch_columns`: 다른 목록이 승인된 개별 열 너비를 유지하면서 남는 공간을 일관된 함수로 배분한다. 두 마스터 자체는 전용 `ManagementMasterTableSpec`이 계속 원본이다.
- `theme::MANAGEMENT_FILTER_DROPDOWN_WIDTH`: 148pt 단일 라벨 필터. 상품관리의 카테고리 경로는 내용상 280pt 전용 역할 유지. 상품관리 카테고리/브랜드 필터는 공통 `management_dropdown`으로 변경되었다.
- **승인·반영 상태:** CONFLICT-003=A는 코드 반영 완료. **CONFLICT-004=A도 코드 반영 완료**(거래처 목록·배송그룹 관리·상품 목록·Excel 대기/이력 목록에 카드 남은 높이 사용, 필요한 푸터/빈 상태 공간 예약). Excel의 선택 건 관리 또는 이력 상세가 펼쳐졌을 때는 기존 상한으로 복귀한다. 일일 결산 및 복수 이력 표·복합 편집 화면은 기존 상한을 유지한다. **CONFLICT-005는 사용자 추천안 승인 및 코드 반영 완료**: 거래처 목록, 상품 목록의 직접 필터→헤더 경로, Excel 관리의 직접 검색→헤더 경로, 배송그룹 관리의 입력 도구줄→헤더 경로가 21pt 공통 함수로 연결됐다. 상태 안내·로딩이 끼어 있거나 여러 표/편집 패널이 있는 곳은 기존 구조 유지. 화면별 적용 및 가변 영역 보호 조건은 [타 화면 적용 기록](egui-management-ssot-rollout-20261121.md)을 따른다.

## 6. 후속 변경 원칙

- 화면 개별 패치 대신 승인된 공통 소유자를 수정한다. 예외가 실제로 존재하면 문서와 코드의 역할 정책을 함께 유지한다.
- **정식 기준은 코드의 현재 토큰**이며, 이 문서는 의도·예외·검증 방법의 규범이다. 숫자 복사본을 다른 문서에 다시 추가하지 않는다.
- 새로운 레이아웃을 마스터에 반영하기 전, 기존 구조·클릭 영역·스크롤·표 헤더 오프셋·업무 흐름을 그대로 보존할 수 있는지 검사한다.
