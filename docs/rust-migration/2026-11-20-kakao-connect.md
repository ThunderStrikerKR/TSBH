# 2026-11-20 카카오톡 연결 국소 수정

실행 대상은 egui-app/target/debug/bando-hub-egui.exe. 사용자 보고의 실제 재현·원인 확정은 미완료다.

## 수정

후보 탐색은 최소화된 창도 허용하지만 기존 sync는 최소화된 창을 모두 건너뛰어 빈 영역을 연결 성공으로 처리할 수 있었다. 원래 위치/최소화 상태를 저장한 Session 생성 후, 최초 sync 전에 창을 복원하고 성공 여부를 검사한다. 실패 또는 해제 시 기존 Session Drop으로 원래 상태를 복구한다.

연결 실패 원인이 stderr에만 기록되던 것을 연결 버튼 아래에도 표시한다. 연결 재시도와 해제 시 오류를 지운다. 운영 DB/공통 디자인 시스템은 변경하지 않는다.

## 검증 및 한계

- PATCH: 두 Rust 소스 수정. main 브랜치, 기존 Cargo.lock 변경은 이번 수정에 포함하지 않음.
- UNIT: cargo test --manifest-path egui-app/Cargo.toml --offline: 148개 중 146 통과, 2 실패. 실패: client_directory::tests::live_relational_database_is_read_only_and_exposes_only_names (phone 필드), layout::geometry_contract::reference_1920p_columns_do_not_trigger_min_width_clamping (열 비율). 수정 전 동일 테스트 비교는 하지 않았으므로 기존 실패로 확정하지 않음. 새 Win32 복원 경로의 실행 검증은 아님.
- DIFF: 미실행.
- ACTION: Computer Use로 실행 창과 카카오톡 로그인 상태를 접근성에서 확인. 반도허브 캡처는 FrameArrived timed out / window capture timed out으로 실패. UI 연결 클릭 및 성공 상태는 관찰하지 못함.
- CI: 미실행.
- WINDOWS: cargo rustc --offline으로 별도 bando-hub-egui-kakao-fixed.exe 빌드 성공. 실행 중 원본 EXE는 교체/재시작하지 않음. 실제 최소화→연결→모드 전환→해제 확인은 미완료.

실제 연결 해결 완료로 판정하지 않는다. 수정본은 egui-app/target/debug/bando-hub-egui-kakao-fixed.exe에 있다.
