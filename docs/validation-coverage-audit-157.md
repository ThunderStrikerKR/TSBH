# 157차 검증기 21종 정적 감사
감사 기준 52e56c490a7401f4132d91de82c2fbc8eab636e7. 반영 시점 1d6246fc833d3f97c8d5c055c7320b65c3f55f0a의 주요 5개 검증 소스 blob 동일 확인. 운영 저장 연결 보류. 실제 컴파일/CI/SQLite 미검증. 코드 변경 없이 감사 기록만 작성.

| ID | 검증기 | 누락 및 과잉 검증 |
|---|---|---|
| V01 | Shape | 일부 필수 배열 및 settings만 검사. categories/brands/shipping/workflow 형식의 일관된 강제 부족. CAS 별도 유지. |
| V02 | Product | archived 후보 skip, 물리 삭제 및 ID 변경 보호 미흡. 전체 상품 필드 재검증으로 기존 복수 구성/잘못된 URL 등 무관한 수정도 차단 가능. |
| V03 | Price | archived 후보 skip; variant ID 교체/필수 필드 null 여부/officialRetail·baseRetail 이력 정책 확인. Product와 3가격 검증 중복. |
| V04 | PriceHistory | F01: 기존 variant를 못 찾으면 continue, 교체로 이력 회피 가능. 신규 상품에 과거 이력 존재하면 일괄 거부, 마이그레이션 별도. |
| V05 | Category | 문제 문자열 집합 비교라 동일 근거 문구의 새 연결 구분 부족. 일부 기존 배정 개선도 차단. |
| V06 | CategoryTopology | 비배열을 빈 목록으로 취급, 빈 ID 및 sibling 중복명 누락. 기존 계층 오류 전체 검사. |
| V07 | Brand | 비배열 허용, 동일 객체의 중복 추가가 unchanged skip. 기존 근거 URL 오류로 무관한 수정 거부. |
| V08 | BrandLink | 비문자 ID를 빈 값으로 취급. 중복 대상 및 삭제된 상품의 역사 연결 처리 정책 검토. |
| V09 | Alias | F02: 기존 충돌 문구와 같은 새 충돌 상대를 면제. 동일 범위/대상 중복에 save_alias와 차이. 수정 row의 기존 오류 전체 차단. |
| V10 | ModelGroup | non-object 그룹 허용, legacy 대표색 분기의 상품 적격성 검사 부족. 부분 개선 근거 변경 시 차단, 진단 비용 검토. |
| V11 | Client | 빈/중복 ID 미흡; 기존 동일 거래처는 계좌 참조 확인을 Account에 의존. 주소·전화 기존 오류가 무관한 변경을 차단. |
| V12 | Account | 비문자 참조 무시; 불명확한 ID 및 일부 개선 차단. |
| V13 | Shipping | 상품 제거 시 기존 productRules 참조, 운임 조합 활성/버전/epoch, 그룹명 중복 미흡. 기존 초기화·기준 변경 null placeholder 호환 검증 필요. |
| V14 | ShippingHistory | F04: current 변경 없이 이력 append 허용; 추가 이력 수/신규 기록/관측 필드 보호 부족. 기존 비정형 이력 복구 규격 필요. |
| V15 | Reception | F07: new_archive_entries의 전역 correction ID 및 validate_order_tree_ids 미연결. 기존 malformed order ID로 전체 검증 차단 가능. |
| V16 | Workflow | F08: workflow null이면 조기 Ok, unique_id는 빈 ID를 하나 허용. 기존 전체 상태 오류가 무관한 상품/설정 변경 차단. |
| V17 | FactoryReply | F05: 기존 ensure_customer_payment_unchanged, validate_factory_scope와 동등한 독립 보호 없음. status/applied, 중복 정정·송장 및 이벤트 검증 보완. 복수 회신의 과거 행이 최종 배송값과 다르면 정상 묶음도 차단 가능. |
| V18 | Message | 기존 20유형 규격 유지. 비관련 변경도 기존 오류로 차단하나 기존 valid_state의 엄격 정책이므로 임의 완화 금지. |
| V19 | ProvisionalNotice | 상품 존재만 검사, 삭제/superseded 연결 정책 확인. 비관련 변경도 기존 오류로 차단. |
| V20 | Registration | F06: deletedProductKeys의 재등록 방지 규칙이 신규 상품 전체에 적용되지 않음, 배치 행/createdIds 및 삭제 키 생성 대응 부족. 레거시 기록 복구 별도. |
| V21 | Mapping | F03: 기존 key 보존만 검사하여 다른 유효 대상 값으로 변경 가능. variantMappings 변경은 의미 미확정으로 거부. |

## 공통 보완
- 등록부는 필요 영역을 결정하지만 실제 각 검증기가 전체 입력을 다시 탐색. 가격/상품 중복 실행 및 광역 의존성 최적화는 정상 변경 차등 테스트 이후.
- F05는 명령 종류별 보호이므로 범용 before/next만으로 전용 명령의 범위를 추정하지 말 것. F07 기존 SQLite 제약 및 archive 검사 유지.
- Full fallback(미등록 경로/깊이/변경 수 초과)은 현재 전체 검증 실행해도 승인 불가. 데이터 규격 미등록과 분석 한도 초과 구분 필요. 부분 snapshot은 별도 proof 없이 호출 금지.
- validation_transitions::tests:: 6종 독립 필수 CI 그룹이 없음. 기존 gate/정적 검사만으로 업무 전이 정상 출력 및 회귀 통과를 주장하지 말 것.
- 순서: F01~F08 무결성 보완 → 정상 native reducer 결과 차등 테스트 → 기존 오류 및 세부 의존성 보완 → scoped/full 계약·CI·SQLite 검증 → 저장 연결.
- 근거: crates/bando-data-access/src/{validation_gate,validation_dependencies,validation_transitions,validation,validation_policy,category_assignment,storage,relational_write,product_commands}.rs; src-tauri/src/reception.rs; src-tauri/src/business_core/catalog_clients.rs 및 하위 aliases,model_groups,categories,registration; rules/{validation,shipping,workflow,factory_reply,settings_accounts}.rs; .github/workflows/egui-compile-gate.yml.
