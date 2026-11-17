# 36차 Windows CI 변경 범위별 검증

| Scope | 조건 | 실제 처리 |
|---|---|---|
| fast | 창 관련 정적 회귀 테스트만 수정 | Ubuntu Node 검사만 실행 (Windows 빌드 생략) |
| window | 창 복원 PowerShell 또는 lib.rs 창 전용 두 영역만 수정 | 실제 격리 Windows DEV EXE 재실행 3회, 위치·크기·최대화·설정 격리 검사 |
| full | 업무 로직, 공통 진입점, 런처, 워크플로, 혼합/불명확 변경 | 기존 Windows WebView2 업무 GUI 37개 + 창 복원 검증 |

- 분류: `scripts/classify-dev-windows-smoke.mjs`. 코드 변경은 이전/현재 `lib.rs`의 창 전용 영역을 제외한 부분이 동일해야 window로 인정한다. 앵커 변경·비정상 Git diff·알 수 없는 변경은 full로 처리한다.
- workflow_dispatch에서 수동 full 또는 window를 선택할 수 있다. window PASS는 전체 GUI PASS로 승격하지 않는다.
- npm ci 실행은 기존 DEV 런처만 담당한다. full GUI의 Playwright는 네이티브 실행 준비 후 한 번 설치하고, window에서는 설치하지 않는다.
- 기존 소스·테스트 수정에 대한 Rust/TS CI 게이트는 유지한다. migration-output은 문서 전용 변경에는 실행하지 않는다.
- 최신 push가 발생하면 동일 이벤트/브랜치의 이전 Windows 시도는 취소한다.
