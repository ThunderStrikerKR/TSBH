# 36차 거래처 직접 선택의 불필요한 안내 제거

- 대상: 사용자가 접수 화면에서 거래처 `테스트 상사`를 직접 선택한 후, `확인할 내용`에 `거래처 연결 근거` 안내와 `거래처 확인` 버튼이 다시 표시되는 문제
- 실제 원인: `order_edit.rs`가 명시적 선택을 `clientMatch.source=MANUAL` 및 상세 문구로 보존했고, `issues.rs::order_issues`가 모든 상세 문구를 리뷰 항목 `CLIENT_MATCH_EVIDENCE`로 전환했다.
- 개선: `MANUAL` 선택 이력은 주문 provenance에 보존하되 사용자에게 추가 확인을 요청하지 않는다. 나중에 실제 미일치 또는 자동 매칭 충돌이 나타난 경우의 ACTION은 계속 유지한다.
- 문구: 선택 이력 `직원이 거래처 ...` → `사용자가 거래처 ...`. 주요 화면에서 사용자에게 노출되는 `직원` 표현은 `사용자` 또는 문맥상 자연스러운 `확인`으로 변경한다.
- 범위: 거래처 자동 증거 `RAW_EXPLICIT` INFO, `CONFLICT`/ `ERROR` ACTION, 미선택 `CLIENT` ACTION은 유지한다.
- PATCH: 이번 변경 커밋
- UNIT: Rust `manual_dealer_selection_review_tests` 2건 및 `tests/tauri-client-match-guidance.test.mjs`의 회귀 계약
- DIFF/ACTION/CI/WINDOWS: 별도 실행·확인 전까지 PASS로 선언하지 않음
