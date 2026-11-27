# 157차 공통 검증 설계 — 2단계 변경 범위 분석기

기준 5b9bb6b626e81f5eb0bc3776cef35ffc7884c847. 1단계 docs/validation-interface-inventory-157.md 및 실제 초기 데이터의 필드 구조를 참조. 공유 Rust crate의 change_set.rs에 읽기 전용 분석 API analyze(&Value,&Value)->ChangeSet 신설. 의존성 결정 및 검증 실행은 다음 단계. 현재 저장 명령/DB/업무 정책은 변경하지 않음.

## 계약
- 두 입력 모두 완전한 원본/후보 스냅샷. partial/fast path를 full snapshot으로 취급하지 않음.
- 객체는 key별 비교; unique nonempty string id를 가진 객체 배열은 ID로 매칭. 삽입/삭제와 남은 ID들의 상대 순서 변경을 구별. 가격 variants, orders/recipients/items, workflow 등 동일 규격.
- ID가 없는 배열(상품 카테고리 ID 목록, 입금 배분 일부, 이력 등), 중복/누락 ID, 비정형 배열은 배열 전체 변경(OpaqueArray). 추후 planner는 전체 하위 범위를 검증해야 함. 자동 정정/정규화 없음.
- 추가/삭제된 객체는 해당 경로와 전체 자손을 의미. 값의 복제나 로그 생성 없이 필드/레코드 경로만 기록. revision/updatedAt/알 수 없는 필드 포함. null과 누락 구분. 순수 순서 변경도 감지.
- 구조화된 Field/RecordId 구분, 경로 문자열의 특수문자 충돌 방지.
- 최대 변경 경로 20,000, 깊이 64. 초과 또는 비객체 root는 Full fallback 반환. 부분 결과를 정밀 결과로 오인하지 않음. 완전한 스냅샷 여부는 호출자 계약이며 이번 단계에서 검증하지 않음.
- 동일 subtree는 equality로 건너뜀. 전체 상태 clone 없음. 배열 인덱스와 결과 경로만 할당. 실제 성능 벤치마크 미실시.
- 다음 단계 의존성 등록부에서 Added/Removed/Modified/Reordered/OpaqueArray 및 Full을 보수적으로 해석. 범위 제한 저장의 별도 증명 규격 필요.

## 검증
8개 단위 테스트 작성 및 Windows compile gate에 필수 그룹 연결. 코드 작성/반영만 실시. Rust 실행/CI/성능 검증 미완료. 공개 read-only 모듈은 후속 planner 및 테스트에서 재사용할 의도이며 UI 업무 저장 권한을 부여하지 않는다.
