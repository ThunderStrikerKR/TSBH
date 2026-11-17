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


## 기능별 실제 Windows 업무 검증

수동 workflow_dispatch에서 orders, pricing, shipping, clients, factory 중 하나를 선택하면 각 실행이 독립된 GitHub Windows runner와 새로운 developer TEST SQLite로 시작한다. 이전 시나리오의 주문·거래처·운송장 상태를 재사용하지 않는다.

- orders: 실제 입력으로 주문 2건 확정·SQLite 주문/수취인 ID 분리
- pricing: 등록된 소매가 84,000원과 비동기 안정성 확인
- shipping: 4,500원 운임 입력, Rust 합계 88,500원, 실제 주문 확정
- clients: 발주 원문의 거래처 증거 기반 Rust 자동 연결·화면 표시
- factory: 로젠 XLSX 오류·운송장 15자리 초과 검증·SQLite 알림 영속성

기본 full은 기존 GUI 37개와 창 재실행 검증을 모두 보존한다. 공통 Rust/React 변경 및 범위가 불분명한 변경은 반드시 full을 실행한다. 파일 소유권이 단독으로 확정된 공장 회신 fixture 스크립트만 자동 factory로 분류한다.
