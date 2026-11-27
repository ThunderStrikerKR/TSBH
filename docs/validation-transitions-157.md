# 157차 공통 검증 설계 — 미연결 10종 연결
기준 a75b6a5e8dd82ba98da912c18b344da0da4e063b. 기존 검증기 11개에 validation_transitions.rs의 10개 후보 전이 검증을 연결. 기존 저장 경로는 그대로이며 진단 전용. 실행/컴파일/CI 미검증.
- Product: 기존 상품 검증 필드 부분을 공유, 후보 참조 및 원본 기준 신규 판단. 기존 validate_product의 첫 category guard 유지.
- Price: 기존 valid_money 공유, 변경된 가격 검증. PriceHistory: 기존 기록 prefix 및 실제 3등급 변경과 추가 기록 일치. 기존 상품 삭제 이력 보호. 신규 상품의 비어 있지 않은 역사 이력은 별도 정책 필요.
- BrandLink: 삭제된 브랜드/신규 dangling link 거부. Alias: 기존 alias_problem 공유 및 이전 문제 비교, 거래처 연결 확인.
- Shipping: 기존 validate_rate_input 공유, 묶음 수량 초과, 버전/시각, 그룹 및 상품 연결. ShippingHistory: 이전 기준 보존 및 기존 조합 불변 필드 확인. 조합 관측값/active 변경은 허용.
- FactoryReply: append-only, 원본 정정 링크, 반영 행의 송장·실제 운임·배송 연결 검사. 파일 파싱/자동 매칭/고객 입금 변경 금지 등 기존 전용 명령 검증 대체 아님.
- Registration: 신규 등록 기록 및 createdIds, 삭제 기록 보존. Mapping: 확인된 sale-unit productMappings의 oldId|variantId -> sourceProductId 및 실제 단일 variant 확인. variantMappings 의미 미확인이라 변경은 진단 거부, 자동 재작성 없음.
- 변경되지 않은 기존 오류 보존은 부분 적용. 상품 전체 검증, 신규 이력 정책, 조합 운임 관측값과 회신의 다중 변경, 등록/매핑 레거시 규격 등은 실제 fixture와 함께 후속 검토 필요. 등록된 21개 함수가 존재한다는 뜻이지 완전한 업무 검증을 인증하는 것은 아님.
- 6개 신규 단위 테스트와 기존 gate 테스트 변경. 실제 SQLite 저장 연결 전 의존성 누락·과잉 검증 및 정상 command output 회귀 테스트 필요.
