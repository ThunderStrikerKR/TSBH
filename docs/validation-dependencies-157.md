# 157차 공통 검증 설계 — 3단계 데이터 의존성 등록부

기준 79720df67a5f67f678c1c5e3a492325adfb287f5. 1단계 인터페이스 목록 및 2단계 change_set을 실제 참조. seed 303의 18개 최상위 키와 기존 Rust 상품/계좌/모델군/별칭/운임/업무 검증의 읽기 의존성을 토대로 검증 요구사항 등록. 이번 단계는 저장 연결/검증 실행이 아니다.

- validation_dependencies.rs: 21개 Validator 요구사항 식별자, for_change / resolve. 변경별 요구 집합의 합집합과 중복 제거. 비어 있지 않은 정밀 변경은 Shape 포함.
- 정확한 상품 및 variant 필드 경로를 분류. 상품 전체 추가/삭제/재배열/불투명 배열은 광역 의존성. 가격 변경은 Product, Price, PriceHistory + Shape. 상품 소속 변경은 Product, Category, Shipping + Shape. 폴더 구조는 CategoryTopology 추가.
- 계좌 변경은 Client/Account/Reception/Workflow, 모델군 설정은 ModelGroup, 상품 삭제·활성/식별 변경은 참조 검증 확대.
- orders/workflow/shipping/categories/aliases/brands/clients 등의 광역 등록은 해당 전체 영역을 검증한다는 계약이며, 모든 하위 키를 각각 분석했다고 주장하지 않는다. 현재로서는 보수적으로 일부 불필요한 검사 가능. 기존 데이터나 역사 주문 재계산/수정의 의미 아님.
- 미등록 경로 또는 Full diff는 Full 요구 반환. 등록된 광역 영역 안의 새 하위 키는 광역 검증으로 처리하며 별도 미등록 경로로 분류하지 않음. 등록 규칙은 실제 실행기 연결 전 재검토.
- PRICE, PRICE_HISTORY, SHIPPING_HISTORY, REGISTRATION, MAPPING, FACTORY_REPLY 등의 순수 전이 검증은 완성된 구현이라고 간주하지 않는다. 다음 단계 미구현 요구사항은 명시적으로 표시하고 안전한 gate 연결 전 저장 경로에 적용하지 말 것. Full은 모든 기존/필요 검증을 적용할 요구이며 검증의 성공을 의미하지 않음.
- 범위 제한 스냅샷/마이그레이션은 이번 분석 계약에 포함하지 않음.
- 9개 단위 테스트 및 필수 CI 그룹 작성. 실제 실행/컴파일 미검증.
