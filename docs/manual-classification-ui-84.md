# 반도허브 84차 — 수동 분류 화면 UX 수정·검증 기록

기준: 2026-11-24, `ThunderStrikerKR/TSBH/main`  
범위: Rust/egui 수동 분류 화면·공통 수동 셀 렌더러  
최신 패치: `34a7773566d5db588201c34980c1369e918e0ecb` (앞선 관련 패치: `9026c7c`, `1ac422c`, `0994cb8`)

## 스크린샷 대비 현행 코드 조사

- 제공된 스크린샷은 배송 건을 가로로 압축한 이전 구성이다. 작업 시작 당시 `main`의 `egui-app/src/manual_workspace.rs`에는 이미 수취인 5개 필드의 세로 편집, 상품·수량·단가 직접 편집, 배송 건수 직접 입력/증감, 원문 양방향 하이라이트, 상품 검색 및 운임 설정이 존재했다.
- 그러므로 해당 기능을 재구현·롤백하지 않고, 다건 배송 편집의 화면 밀도와 조작 일관성을 보완했다.

## 실제 코드 변경

| 영역 | 변경 | 구현 |
| --- | --- | --- |
| 다건 배송 | 전체 배송 카드의 연속 렌더링 대신 선택한 배송 건만 표시; 모델에는 모든 배송 건 유지 | `manual_workspace.rs`, `visible_manual_shipment` 및 필터 |
| 배송 탐색 | 별도 신규 버튼 대신 접수 마스터의 구간형 `reception_recipient_navigator` 재사용; 이름 전체 보기와 비활성 끝점도 재사용 | `manual_workspace.rs` + `components.rs` 기존 공유 함수 |
| 편집 흐름 | 배송 전환은 `queue_cell_selection`의 기존 미확정 입력 처리 및 수량 확정 제한을 유지 | `manual_workspace.rs` |
| 스크롤 | 배송 탐색기를 내부 행 스크롤 밖에 두고 스크롤 높이 예약 | `manual_workspace.rs` |
| 발주 원문 | 폭이 좁은 패널의 긴 제목 축소, 조작 설명은 호버 도움말로 이동 | `manual_workspace.rs` |
| 입력 글꼴 | 수동 편집 `TextEdit`의 14pt 크기를 읽기 전용 셀과 같은 공통 16pt 토큰으로 변경 | `components::manual_linked_table_editor` |
| 상품 정보 | 잘린 상품명 전체를 호버로 확인 가능 | `manual_workspace.rs` |
| 운임 설정 | 발송지와 운임 방식의 버튼 그룹을 각각 명확히 라벨링 | `manual_workspace.rs` |

## 기존 동작 보존 조건

- 배송 건수 증감·직접 입력(1~100), 유효 데이터 삭제 확인, 상품·수량 편집, 원문 블록 연결/해제, 적용 시 Rust reducer 사용 경로 보존.
- 수량이 미확정된 상품은 배송 전환을 막고, 미확정 입력이 있으면 기존 `queue_cell_selection` 확정 경로를 호출한다.
- 원본·수정 모델 및 가격 계산 권한은 변경하지 않는다. `ReadOperation::ManualClassification`을 새 TS 판단으로 대체하지 않는다.

## 검증 게이트

| 게이트 | 상태 | 증거/한계 |
| --- | --- | --- |
| PATCH | COMMITTED | `9026c7c`, `1ac422c`, `0994cb8`, `34a7773` 모두 `main` |
| STATIC | VERIFIED_SCOPED | GitHub에서 최종 파일 재조회: 선택 행 필터, 공유 탐색기 호출, 상품명 호버, 운임 라벨, 16pt 글꼴 토큰 및 Rust rule 경로 확인 |
| UNIT | NOT_EXECUTED | `manual_editor_keeps_one_delivery_visible_without_dropping_other_rows` 단위 테스트 작성. 현재 세션에서 Rust 컴파일러 및 의존성 빌드 환경이 없어 실행 증거 없음 |
| DIFF | NOT_VERIFIED | React 원본 대비 실제 동작 동등성·비동기 순서 역전 미검증 |
| ACTION | NOT_VERIFIED | 마우스·키보드·IME·다건 배송 시나리오 실제 앱 재현 필요 |
| CI | NOT_VERIFIED | 마지막 패치 커밋의 GitHub combined status 반환값이 빈 목록; 성공/실패 판정 불가 |
| WINDOWS / VISUAL | **FAIL** | 사용자가 2026-11-24 실제 Windows 실행 캡처 제공. 우측 발송·운임 버튼 하단 클리핑 확인. `docs/ui-visual-verification/audits/manual-classification-84.json`에 결함 OPEN 기록. |

## 후속 검증 시나리오

1. 배송 건수를 1→3으로 늘린 다음 각 배송 건에 서로 다른 이름·주소·상품을 기입하고, 이전/다음 전환 및 재진입 후 데이터 유지 확인.
2. 이름 편집 중 배송 전환 시 값 확정/오류 처리, 수량 미확정 중 전환 차단, IME 조합 중 방향키 오동작 유무 확인.
3. 접수 원문과 배송 2의 주소·상품 항목을 연결하여 양방향 호버·스크롤·해제 동작 확인.
4. 1920×1080 및 축소 창에서 원문/편집 영역의 독립 스크롤, 상품명 툴팁, 입력 글꼴 16pt와 클리핑 확인.
5. 도서/산간, 착불, 운임 수정, 상품 검색/제거, 초기화, 초안 적용, 취소의 이전 동작 회귀 확인.

**84차 최종 재판정(2026-11-24):** 코드 변경 커밋은 존재하지만, **UI 실화면 품질은 FAIL / 승인 보류**. 사용자 실제 실행 캡처에서 하단 콘텐츠 클리핑·좌우 정보 밀도 불균형이 확인되었다. 84차 개선을 ‘완료’로 보고한 것은 잘못된 판정이었다. 이번 검증 체계 보강은 결함 해결과 별개이며 화면 재설계·동일 소스 Windows 검수 전까지 PASS로 바꾸지 않는다. [검증 계약](ui-visual-verification/README.md) 및 [84차 FAIL 감사기록](ui-visual-verification/audits/manual-classification-84.json) 참조.

## 84차 후속 — 실제 화면 하단 클리핑 수정 후보 (2026-11-24)

**소스 패치:** [`c34b35d`](https://github.com/ThunderStrikerKR/TSBH/commit/c34b35d4582524a962d363cf84766e675cb7a109) → [`c44a017`](https://github.com/ThunderStrikerKR/TSBH/commit/c44a017db0694f51238d23c7a7f6397be0469e13)

### 원인
배송 정보 영역 안의 `manual-recipients-table` 스크롤은 수취인·상품·발송 옵션까지만 감쌌다. 그 아래의 수량 증감 버튼과 운임 직접 입력은 스크롤 밖에 있었으며, 스크롤 최대 높이도 배송 정보 패널의 남은 실제 공간이 아니라 `height - 고정된 상수들`을 사용했다. 따라서 짧은 작업영역에서 화면 하단의 필드와 버튼이 잘릴 수 있었다.

### 코드 조치
- 배송 건수·수취인 탐색을 렌더링한 후 `panel.available_height()`로 잔여 높이를 구해 스크롤 뷰포트 계산.
- 수량 조작과 `shippingCharge` 입력을 같은 스크롤 영역으로 이동. 내부 스크롤을 끝까지 내리면 마지막 조작에 접근하도록 구조 수정.
- `VisibleWhenNeeded`로 초과 콘텐츠의 스크롤 표시, 운임 수정 시 `scroll_to_rect`로 입력 필드 노출.
- 화면 구조가 되돌아가지 않도록 `manual_scroll_surface_owns_quantity_and_freight_controls` 회귀 테스트 추가.

### 검증 판정
- **PATCH:** main 커밋 반영 확인.
- **STATIC:** 최종 파일 재조회에서 패널 잔여 높이 계산, 스크롤 내부 운임 버튼·입력, 스크롤바, 편집 포커스 이동 및 Rust 처리 호출 유지 확인.
- **UNIT:** 테스트 추가만 확인; `cargo test` 실행 증거 없음.
- **CI:** 수정 SHA의 빌드/테스트 성공 여부 확인되지 않음.
- **VISUAL WINDOWS:** 동일 수정 SHA의 실제 Windows 화면 확인 전. **기존 84차 FAIL / OPEN 유지**.

실사용 캡처에서 `발송 · 운임` 마지막 버튼과 `적용 운임` 입력이 스크롤로 보이고 동작하는지, 하단 푸터와 겹치지 않는지 1920×1080 및 작은 작업영역에서 검증해야 완료로 판정한다. 이전 캡처의 개인정보는 공개 저장소에 올리지 않는다.

## 84차 후속 — 접수 UI 실제 공통화 패치 (2026-11-24)

**목표:** 사용자가 제공한 최신 접수 기본화면을 기준으로 수동 분류의 오른쪽 배송 정보 패널을 접수와 동일한 공통 컴포넌트로 렌더링한다. 이미지를 보고 별도 UI를 모방하는 방식은 폐기한다.

### UI 코드 변경

- `components::reception_section_with_header`를 접수 `main.rs`의 실사용 카드 구성에서 추출. 두 화면 모두 동일 함수가 카드 테두리/16pt 패딩/헤더/경계선을 출력한다.
- 수동 분류 수취인 데이터에 `reception_recipient_fields` 직접 적용. 이름·주소·휴대폰/전화 같은 줄·메시지 배치를 접수와 동일하게 구성하고, 선택 시 `reception_recipient_inline_cell_editor`로 해당 셀 위에서 편집한다.
- 상품을 수동 전용 4열 Grid에서 접수의 3열 표 `reception_item_row_editable`로 전환. `reception_items_header_with_amount_label`은 같은 표 렌더러를 사용하면서 접수는 금액, 수동 분류는 단가로 업무 의미만 구별한다.
- `reception_freight_row_editable` 및 `reception_freight_inline_cell_editor`로 운임 입력·선불/착불·도서산간을 접수와 공통화하고, `reception_item_origin_action_editable`을 재사용한다.
- `reception_item_add_action_editable` 버튼은 상품 카드 제목 오른쪽에 배치. 수동 분류 고유 상품 검색/삭제와 수량 증감은 별도 보조 조작으로 유지.
- 배송 건수 직접 입력/증감, 수취인 탐색, 원문 연결/해제, 원문 앰버 상태, 비동기 Rust 업무 경로와 초안 적용은 유지한다.
- 수동 분류 키보드 이동은 실제 공통 입력 배치에 맞춰 주소 아래 휴대폰·전화를 가로 2셀로 취급한다. 연락처 좌우 이동과 위아래 이동을 분리하고 IME 보호는 유지한다.
- 오른쪽 두 카드만 독립 스크롤하도록 구성해 모달 하단 적용 버튼을 스크롤 바깥에 유지한다.

### 주요 관련 커밋

- `82de9d3` 공통 카드 추출 → `821d6e9` 접수 연결
- `10c8063` 공통 입력 셀 오버레이 → `cc0b0b0` 수동 분류 카드/표 교체
- `3b3677f` 실제 배치에 맞는 방향키 이동
- `50ab8fe` 검사 갱신 → `175749c` UI borrow 정리
- `f34a0b0` 공통 헤더의 업무별 라벨 → `e6e33dc` 수동 단가 연결

### 검증 상태

| 단계 | 상태 | 의미 |
| --- | --- | --- |
| PATCH | COMMITTED | `main`에 공통 호출 경로 변경 반영 |
| STATIC | PASS_SCOPED | 공통 호출·기존 Grid 제거·Rust 네비게이션 검사 코드 존재 및 소스 괄호 균형 확인 |
| UNIT | NOT_EXECUTED | 회귀 테스트를 작성했지만 Rust 컴파일러가 현재 세션에 없어 실행 증거 없음 |
| CI | NOT_VERIFIED | GitHub 빌드 성공 여부 확인되지 않음 |
| WINDOWS | NOT_VERIFIED | 변경한 SHA의 Windows 화면 캡처·조작·DPI 검증 전 |

**중요:** 코드가 공통 함수를 호출한다는 사실만으로 두 화면이 시각적으로 완전히 동일하다고 판정하지 않는다. Windows에서 1920×1080 및 작은 작업공간 검수 후, 기존 OPEN 결함을 닫는다. 불합격 기록은 유지한다.

## 84차 후속 — 배송 건수 증감 버튼 날짜 선택기 스타일 통합 (2026-11-24)

- 사용자가 마지막에 지정한 대상은 수동 분류 상단의 배송 건수 `− / +` 버튼이다.
- 수취인별 주문 정보의 이전/다음 화살표는 이미 구현되어 있으므로 이번 변경 대상에서 제외했다.
- 기존 `theme::action(..., ButtonRole::Standard)`로 렌더링하던 증감 버튼을 `components::date_navigation_step_button`으로 교체했다.
- 날짜 선택기 `calendar_nav_button`과 배송 건수 스테퍼는 이제 동일한 `date_navigation_button_surface`를 호출한다. 따라서 28×28pt 클릭 영역, `theme::INPUT` 배경, 1pt `theme::BORDER`, `CONTROL_RADIUS`, 호버 강조가 같은 규격을 사용한다.
- 비활성 버튼은 `DISABLED_ACTION_BG / BORDER / TEXT`를 명시해 1건에서 감소, 100건에서 증가가 비활성화됐음을 구분한다.
- 숫자 직접 입력, 1~100건 제한, 기존 resize 요청 흐름은 유지한다.
- 회귀 테스트 `shipment_count_steppers_share_the_date_picker_navigation_surface`를 추가했다.

검증: 소스 연결 정적 확인 PASS. 해당 커밋의 GitHub status는 빈 배열이고 Rust 빌드 및 Windows 실화면 검증은 미실행이다. 따라서 84차 전체 UI 판정은 기존 FAIL을 유지한다.
