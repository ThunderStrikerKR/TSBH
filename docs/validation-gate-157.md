# 157차 공통 검증 설계 — 4단계 검증 실행기

기준 a7044632adeeb1089031acf9a040626294963c13. validation_gate::inspect(before,next)는 전체 authoritative snapshot 두 개에서 change_set과 dependencies를 내부 생성하고 실행 결과를 반환한다. 호출자가 검증 목록을 줄여 전달할 수 없다. 읽기 전용 진단 API이며 SQLite 저장에 연결하지 않았다.

21개 요구사항 중 11개 기존 순수 검증기를 연결: Shape, Category, CategoryTopology, Brand, ModelGroup, Client, Account, Reception, Workflow, Message, ProvisionalNotice. 기존 valid_state의 shape와 두 메시지 검증을 분리하되 기존 호출의 의미/순서는 그대로 유지한다. 기존 category validate의 topology와 assignment를 분리하되 기존 호출은 둘 모두 수행한다. 전체 상태 복제나 명령 JSON 없음. reception 모듈의 crate 내부 접근만 확대.

10개는 미연결: Product(후보 참조/신규·기존/삭제 및 필드 선택 필요), Price, PriceHistory, BrandLink(후보 브랜드 연결), Alias(신규 오류 비교), Shipping, ShippingHistory, FactoryReply, Registration, Mapping. 기존 입력·변경 함수가 없는 것이 아니라 동일 요구 전체를 검사하는 순수 adapter가 아직 준비되지 않은 상태. 기존 검증의 정책 범위를 확장하지 않는다.

Report: required, checked(호출 시도), unavailable, failures, fallback. Shape는 항상 실행하고 기존 메시지 및 잠정 안내문 저장 검증 역시 항상 요구. Shape 실패시 후속 호출 중지. 중복 제거; 실패/미지원/미확인 fallback/미실행 중 하나라도 있으면 is_complete_and_valid=false. Full fallback은 전체 21개 요구로 확장하되 신규 경로의 의미를 알 수 없으므로 승인으로 처리하지 않는다. 기존 오류와 신규 오류 차이 정책은 기존 검증기 수준 유지, 별도 공통 정책 미완성. 전용 범위 제한 저장 및 마이그레이션 미포함. 8종 테스트 작성, CI 그룹 등록. 실제 실행 미확인. 5단계 저장 연결 전에 미연결 검증기와 dependency coverage를 검토해야 한다.
