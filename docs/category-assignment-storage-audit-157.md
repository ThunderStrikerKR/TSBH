# 157차 최하위 분류 저장 경로 감사

기준 2a6d291523be1d314a8327732b53e406f6dd4df0. 현행 egui 상품 저장은 gateway → patch_product_v2 → shared SQLite writer. 아래 조사 결과는 소스 경로 검토이며 실행 검증이 아니다.

| 경로 | 기존 상태 | 수정 |
|---|---|---|
| 상품 상세·직접 등록 / upsert_product_from_editor, upsert_product | validate_product가 category 문자열만 검사 | 공유 assignment 검증 |
| 등록 검토 확정 / prepare_registration_commit | 기존 validate_product 호출 | 공유 검증 상속 |
| 공식 상품 가져오기 / apply_official_product_import | 말단 및 직접 active 검사, 조상 상태 미검사 | 공유 검증 상속 |
| Excel category_edit | 계층/active 자식만 검사 | 공통 preview 및 최종 apply 검증으로 보완 |
| Excel apply_product_workbook | 검토 후보 배열 그대로 채택 | 전이 검증 |
| 상품 드래그 이동 | 전용 Rust 말단/경로 검증 | 최종 저장에서도 검증 |
| 폴더 이동·생성·삭제 | D08 이전 처리 및 기존 validate, 변경 없는 상품 검사 누락 | 공통 전이 검사 |
| legacy_add_category | 상품 영향 검증 없음 | validate 추가 |
| v2 snapshot/entity writer | 공통 유효성 검사에 분류 규칙 없음 | write_validated에서 동일 규칙 강제 |

상위 경로 ID는 실제 하위 배정이 있을 때 정상이다. 복수 독립 말단도 허용. 미지정은 현행 허용 유지. 비활성 또는 삭제된 상위 경로 및 없는 ID 차단. 기존 위반을 무관한 주문/가격 수정으로 강제 수정하지 않음. 신규/변경 배정 및 구조 변경으로 발생한 신규 위반만 차단.

범위 제한: DB 초기화/과거 v1 migration 및 raw relational store, SQL 직접 변경, 운영 DB 자체, 전체 재귀 호출 그래프까지는 확인되지 않음. UI 선택기의 비활성 분류 표시 및 Excel 복수 독립 소속 입력 규격은 별도 과제. 컴파일, 실제 SQLite, CI 미검증.
