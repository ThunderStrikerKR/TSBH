# 반도허브 egui 숫자 서체 정책 및 전체 소스 점검 — 설계수집 78차

> 기준: 2026-11-23 `TSBH/main`, `egui-app/src` Rust 파일 56개. 사용자 확정: **전화번호·날짜는 기본 UI 글꼴(Windows 맑은 고딕 Regular), 금액·수량만 Consolas Regular**. 실제 Windows 글꼴 파일 부재 시 등록된 비례/고정폭 대체 폰트를 사용한다.
>
> 실행상 유일한 글꼴 소유자는 `egui-app/src/typography.rs`. 이 문서는 검사 결과와 사용례를 기록하며 별도의 수치 원본이 아니다.

## 정식 정책

| 데이터 의미 | 서체 역할 | 기본 크기 | 정렬 | 보정 |
| --- | --- | --- | --- | --- |
| 전화번호 (Phone) | `FontId::proportional` (일반 UI Regular) | Form 14pt / Reception 16pt | 왼쪽 | 숫자 Consolas 오프셋 없음 |
| 날짜 (Date) | `FontId::proportional` (일반 UI Regular) | Form/Table 14pt | 왼쪽 기본, 개별 날짜 선택기는 기존 중앙 정렬 | 날짜 입력용 공통 `management_date_text_margin()` |
| 금액 (Money) | `FontId::monospace` (Windows Consolas) | Form/Table/CompactCard 14pt, Reception 16pt, ReceptionTotal 18pt, Summary 16pt | 오른쪽 | `DATA_NUMERIC_GLYPH_OFFSET_Y = 2.5` |
| 수량 (Quantity) | `FontId::monospace` (Windows Consolas) | Form/Table/CompactCard 14pt, Reception 16pt, CompactCaption 13pt | 오른쪽 | 동일 2.5pt |
| 발주원문·주소·이름·메시지 | `FontId::proportional` (일반 UI Regular) | Reception 16pt | 왼쪽 | 숫자 보정 없음 |
| 대형 KPI | 기존 명시적인 강조 글꼴 | 별도 마스터 토큰 | 기존 정책 | 별도 |
| OCR 확대율 % | 일반 UI 비례 글꼴 | 기존 라벨 크기 | 기존 정책 | 숫자 보정 없음 |

- 모든 금액·수량·날짜·전화의 서체 선택은 `data_value_font(kind, context)`가 담당한다. 위치·선택/호버·고정 높이·금액 입력 콤마·실제 데이터 파싱은 변경하지 않는다.
- 전화번호와 날짜의 문자열 내용에 숫자가 들어 있다는 이유만으로 Consolas를 적용하지 않는다. 의미별 분류가 우선이다.
- 기존 `typography::quantity()` 호환 경로는 13pt/20pt 표시를 보존하면서 `DataValueKind::Quantity + DataValueContext::CompactCaption`을 사용한다.

## UI 56개 파일 전수조사 결과: 중앙 정책 우회 지점

| 소스 | 우회 지점 | 처리 결과 |
| --- | --- | --- |
| `typography.rs` | `data_value_font`가 네 의미를 모두 고정폭으로 돌려줌 | Money·Quantity만 Monospace, Date·Phone은 Proportional |
| `typography.rs` | `quantity()`가 구형 `Role::Quantity` 스타일을 직접 사용 | `data_value_text(...Quantity,CompactCaption)`로 중앙화, 기존 13pt 유지 |
| `components.rs` | 수취인 연락처가 `DataValueKind::Phone`을 통해 Consolas로 표시됨 | 동일 경로 유지하되 중앙 정책 변경으로 16pt 기본 글꼴 적용. 읽기/편집은 같은 함수 공유 |
| `components.rs` | 합계 화면의 `FontId::monospace(18.0)` 직접 호출 | `Money + ReceptionTotal` 의미/표시 위치로 변경. 외관 크기 유지 |
| `main.rs` | OCR 검토 확대율 `RichText::monospace()` 직접 호출 | 확대율은 금액·수량이 아니므로 기본 비례 글꼴로 변경 |
| `order_history_manager.rs` | 날짜 입력창에 Consolas 폰트를 적용하면서 Consolas 오프셋용 별도 주석/마진 사용 | `Date + Form`을 통해 기본 글꼴 적용, 공통 날짜 마진으로 정리 |
| `components.rs` 내부 회귀 테스트 | 수취인 연락처가 Monospace 16pt인 것을 가정 | Proportional 16pt를 기대하도록 변경 |
| `typography.rs` 내부 회귀 테스트 | 날짜·전화까지 Monospace를 기대 | 전체 `DataValueKind × DataValueContext` 조합 검사로 교체 |

### 중앙 정책을 이미 사용하고 있어 유지한 경로
- `product_manager.rs` 상품 도매가/반도매가/소매가 표시·편집: 금액 Monospace
- `components.rs` 접수 품목 수량·금액, 일반 `FieldKind::Money/Quantity/Telephone` 입력: 의미별 중앙 서체
- `workflow_board_ui.rs` 처리 중 주문 금액: Money + CompactCard
- `manual_workspace.rs` 기존 수량 카운터: 호환 `quantity()` 경유로 중앙 서체
- `order_history_manager.rs` 날짜 입력·커서 계산: 같은 `Date + Form` FontId 사용
- 거래처·주문·Excel·일일결산에 별도 Monospace 하드코딩 없음. 비례 글꼴로 출력하는 일반 텍스트는 유지

## 추가 검수: 화면에 드러나는 단독 금액 및 혼합 데이터

| 화면 | 추가로 확인한 우회 | 조치 |
| --- | --- | --- |
| 거래처 관리 / 특수 단가 | 상품명·금액을 하나의 본문 글꼴로 처리 | 하나의 `LayoutJob` 안에서 상품명은 Proportional, 금액은 `Money + Form` |
| 일일 결산 / 상세 지표 | `metric()`의 금액을 일반 `body` 서체로 렌더링 | `Money + Form`, 기존 22pt 줄높이 유지 |
| 주문 상세 / 입금액·실제 운임·청구 내역 | 금액이 일반 본문 글꼴 사용 | 단독 금액을 `data_value_text(Money,...)`로 통일 |
| 주문 상세 / 상품명 × 수량 · 단가 | 수량과 단가가 상품명과 같은 비례 글꼴 | 하나의 `LayoutJob` 안에서 수량=`Quantity`, 단가=`Money` 적용 |
| 주문 상세 / 청구 합계 | 합계 금액에 일반 섹션 강조 서체 직접 적용 | 금액은 `Money + Summary`, 기존 23pt 줄높이 유지 |
| 접수 품목 상세 팝업 / 가격 | 가격 텍스트에 일반 본문 글꼴 사용 | `Money + Form`으로 통일, 기존 22pt 줄높이 유지 |

- '배송 1건', '3개 거래처', '미확정 2건' 등 **서술형 복합 문구와 업무 상태 라벨**은 한글 기본 글꼴을 유지한다. 단독 금액·수량 데이터 열과 구분되는 정책상 예외이며, 단순 숫자 출현만으로 전체 문장을 Consolas로 바꾸지 않는다.
- 혼합 텍스트는 `typography::inline_body_values()` 또는 단일 금액 전용 `inline_body_money()`를 사용한다. 한 줄을 2~4개 라벨 위젯으로 쪼개지 않아 부모 레이아웃에 새 간격을 만들지 않는다.
- CI 컴파일 로그에서 `egui::Label::wrap(false)`는 현재 egui 0.33.3에 없는 API임을 확인해 제거했다.

## 재발 방지

1. `management_table_type_roles_tests::semantic_data_roles_keep_only_money_and_quantity_monospace`가 4개 의미 × 모든 표시 문맥에서 서체 종류/크기/정렬을 검증.
2. `components`의 수취인 읽기↔편집 동일 Rect/커서 테스트에서 Phone = Proportional 16pt 검증.
3. `.github/workflows/egui-targeted-check.yml`의 `git grep` 검사는 `typography.rs` 바깥 모든 `egui-app/src` 파일의 `FontId::monospace`, `FontFamily::Monospace`, `.monospace()` 직접 호출을 거부. 새 파일도 검사 범위에 포함된다.
4. CI와 Windows 실화면은 별개의 검수 단계다. 1920×1080 및 축소된 접수 화면에서 수취인 연락처, 날짜 필터, 숫자 상하 정렬을 비교해야 최종 시각 검수 완료로 판정한다.

### 변경하지 않은 사항

- 금액/수량의 Consolas `+2.5pt` 오프셋, Regular 강조 정책, 천 단위 콤마 입력 정책
- 표 헤더·열 너비·입력창 36pt 고정 크기·하이라이트와 커서 위치·카카오톡 도킹
- Rust 가격 엔진·송장·운임·SQLite·저장 및 이력 로직
