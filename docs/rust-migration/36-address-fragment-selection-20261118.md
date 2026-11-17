# 주소 후보 조각 과다 확인 제거 — 2026-11-18

## 관찰
- 원문: `받는곳: 경상남도 창원시 / 진해구 안청남로 40 / 커피빈스 1층`
- 화면: `경상남도 창원시 진해구 안청남로 40 커피빈스 1층`로 이미 정상 결합
- 오류: `확인할 내용 → 주소 후보 선택 필요`에 같은 주소의 세 조각을 각각 `→ 적용`으로 제시
- 원인: `recognition/mod.rs`는 근거를 각각 `recipient.candidates`에 보존하며 `address`를 결합하지만, `rules/issues.rs::order_issues`는 `CANDIDATE_address` 판정을 `unique.len()>1`만으로 생성했다.

## 적용
- 검증된 단일 주소가 복수의 원문 근거를 순서대로 결합한 결과일 때는 `CANDIDATE_address`를 생성하지 않는다.
- 서로 다른 실제 배송주소 두 건, 충돌하는 행정구역, 완성 전 주소, 원본 근거와 다른 최종 주소는 자동으로 확정하지 않는다.
- 수취인 주소, 사용자 수정값, 원문 근거 및 인식 스팬은 변경하지 않는다. Rust 확인 항목 판정만 변경한다.
- React의 `StaffChecks`와 Rust `workflow_issues`는 변경된 `order_issues`를 그대로 사용한다.

## 검증 (각 게이트 독립)
- PATCH: `c6c79ac6c51aa08f6c3fe7a9efb186deeb1b0306` — `src-tauri/src/business_core/rules/issues.rs`
- UNIT: `address_fragment_review_tests` 3건, Rust dispatcher CI 성공
- FRONTEND: 회귀 연결 테스트 성공
- CI: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44139296803 — 성공, `aa576964c359ca00fdb2ce020e6eee560980df30`
- DIFF: 해당 승인된 변경 자체의 고정 TypeScript 차등 검증은 별도 미완료
- ACTION / WINDOWS: 해당 원문으로 실제 WebView2 화면을 재실행한 검증은 미완료
- 전체 F02 기능 종료 판정: OPEN
