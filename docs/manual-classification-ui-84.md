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
