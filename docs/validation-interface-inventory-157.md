# 157차 공통 검증 설계 — 1단계 인터페이스 및 호출 경로

기준: f5287eafe7d81876f92ab1174fd40ed19e248466. 이번 단계는 기존 Rust 검증의 소유자 및 입력·출력 정리다. 실제 적용 범위/정책을 확장하지 않는다. 신규 중앙 실행기, 변경 범위 분석, 의존성 전파는 다음 단계이며 아직 저장에 연결하지 않는다.

## 인터페이스 인벤토리
모든 순수 오류 검증은 기존 Result<(), String>을 유지한다. 상태 참조는 &Value를 빌린다. 전이 검증의 논리 입력은 authoritative before와 candidate next로 통일하되 기존 함수의 매개변수 순서는 유지한다. 결과를 JSON으로 포장하거나 전체 상태를 복제할 필요가 없는 내부 호출은 직접 연결한다.

| 검증 ID | 원래 소유자 / 함수 | 실제 입력 / 결과 | 현행 호출 경로 | 이후 연결 주의 |
|---|---|---|---|---|
| SHAPE | validation.rs / valid_state | state -> Result | snapshot / finish_update / relational writer / scoped fast paths | 템플릿/잠정 안내문 검증을 내부 호출함; 중복 실행 금지 |
| CATEGORY_ASSIGN | category_assignment.rs / validate_product_assignment | state, product -> Result | catalog_clients::validate_product | 기존 동일 소속의 예외 유지 |
| CATEGORY_TRANSITION | category_assignment.rs / validate_category_transition | before,next -> Result | write_validated 첫 문장 / category reducer / Excel apply / legacy writer | CI 필수 호출 유지 |
| CATEGORY_TOPOLOGY | catalog_clients/categories.rs / validate | Option<before>,next -> Result | change/delete/legacy add 및 validate_categories dispatcher | 배정 전이 검증 포함, 독립 실행 시 중복 가능 |
| PRODUCT | catalog_clients.rs / validate_product | state,product -> Result | editor/upsert/registration/official import/Excel preview | 신규/기존 차이 및 category_assignment 포함; 필드 단위 분리 검토 필요 |
| PRICE | product_commands.rs / apply_product_price | &mut state,product ID,variant ID,tier,amount,expected -> Result | 가격 변경 명령 | 변경 함수; 순수 검사 아님. SQLite 금액 제약 존재 |
| PRICE_HISTORY | product_commands.rs / record_price_changes | before,&mut after,at,source -> Result | 상세·가격·엑셀 반영 | 변경 함수; 기존 이력의 append-only 전이 검증은 미구현 |
| BRAND | validation.rs / validate_brands | before,next -> Result | 공통 저장, scoped fast paths, legacy writer | catalog_clients/brands.rs의 별도 검증과 상충 여부 후속 조사 |
| BRAND_LINK | product_commands.rs / validate_product_brand_selection | before,product -> Result | 상품 편집 저장 | before의 브랜드 목록 기준; 브랜드 삭제에 따른 후보 참조 확인은 미구현 |
| ALIAS | catalog_clients/aliases.rs / alias_problem | record,aliases,products -> Vec<String> | save_alias / alias_problems[_one] | 다중 진단; 기존 오류와 신규 오류 비교 필요 |
| MODEL_GROUP | catalog_clients/model_groups.rs / validate | state -> Result | catalog_clients::validate_model_groups -> validate; dispatcher 및 두 설정 명령 공통 | 상품 삭제/상태 변경의 참조 검증 연결은 후속 |
| CLIENT | validation.rs / validate_clients | before,next -> Result | 공통/legacy writer | 기존 동일 거래처 건너뜀, 계좌 변경 영향은 별도 검사 |
| CLIENT_UNCHANGED | validation.rs / validate_unchanged_clients | state -> Result | 공통 저장 및 제한 저장 | 거래처 동일성 증명 전제 |
| CLIENT_COMMAND | catalog_clients/client_mutation.rs / validate_client | state,client -> Result | 거래처 직접 변경, 등록 | CLIENT와의 중복/차이 후속 조사 |
| ACCOUNT | business_core/rules/settings_accounts.rs / validate | state -> Result | shared validation::validate_accounts -> settings_commands | 원래 계좌 검사 그대로, 신규 참조 검사 미추가 |
| RATE_INPUT | shipping_action.rs / validate_rate_input | rule -> Result | action_payload | 입력값 검증, 배송그룹/이력 상태 검증과 다름 |
| SHIPPING_ACTION | business_core/rules/shipping.rs / shipping_action | payload -> Result<Value,String> | apply_shipping_action | 변경 함수; 전체 상태 전이 검사로 분리 검토 필요 |
| RECEPTION | src-tauri/src/reception.rs / validate_reception | before,next -> Result | storage::write_validated, finish_stable_freight, legacy writer | 원문/확정 기록 및 정정 이력 보호 |
| ORDER_IDS | reception.rs / validate_order_ids | state -> Result | 주문 동일성 증명 시, 공장 회신 경로 | RECEPTION의 축소 검증 |
| ORDER_TREE_IDS | relational_write.rs / validate_order_tree_ids | state -> Result | 단일 배송 재조회 | 관계형 ID 고유성 |
| WORKFLOW | rules::validate_workflow_transition | next,before -> Result | 일반/단일 배송/마이그레이션, gateway | 다른 전이 검증기와 인자 순서 반대; 이후 adapter에서 정규화 |
| WORKFLOW_SHARED_CORE | rules::validate_workflow_transition_shared_core | next,before -> Result | finish_factory_shared_core | 동일 core 입증 전제, before의 core 트리가 생략될 수 있음 |
| FACTORY_SCOPE | storage::validate_factory_scope | before,next -> Result | 공장 회신 및 정정 | 허용 필드 변경 제한, 업무 상태 검증과 별개 |
| MESSAGE | validation.rs / validate_message_templates | state -> Result | valid_state, settings patch | 원래 오류 문구 유지 |
| PROVISIONAL | validation.rs / validate_provisional_product_notices | state -> Result | valid_state, settings patch | 상품 참조 검증 포함 |

## 실제 단계 1 변경
- model_groups::validate의 가시성은 부모 모듈로만 확대한다. catalog_clients::validate_model_groups가 직접 참조 호출을 제공하고 기존 dispatcher도 이를 호출한다. product_commands의 모델군 저장 및 설정 변경 경로는 동일 함수를 호출한다. 기존 동작과 에러 문구는 유지.
- settings_accounts의 공유 source 경로를 validation.rs로 이동. 공유 crate 내부의 validate_accounts로 노출하되 외부 공개 API는 확대하지 않는다. settings_commands는 이 경로를 사용.
- 모델군 직접 호출과 dispatcher 결과가 같은지 비교하는 테스트 추가. 실행 여부는 별도.
- 카테고리 필수 CI 호출 위치, 기존 저장 명령, 범위 제한 fast path 및 legacy 호환 명령 유지.

## 남은 단계 1의 제약과 다음 단계
위 인벤토리는 확인한 대표 운영 Rust 경로 기준이며 전체 재귀 call graph 또는 SQL 감사가 아니다. 임의 전체 snapshot 저장, 저수준 마이그레이션, 데이터 복원, 별도 로컬 UI/알림 저장, 내부 테스트 전용 경로의 정책은 이 커밋에서 변경하지 않는다. PRICE/PRICE_HISTORY/SHIPPING_ACTION은 변경 함수이므로 아직 validation_plan에 순수 검증기로 등록할 수 없다. 검증기의 구체적 의존성 및 신규 오류 비교 정책 확정 후 단계별 연결이 필요하다. 소스 코드 작성과 실제 컴파일/CI/SQLite 검증은 별개다.
