# 36차 Windows 창 위치·크기·최대화 복원 — 원인과 검증

날짜: 2026-11-17. 기준 실제 코드 커밋: `2b8e517c65705b61398a235344a50f86c4435b09`.

## 현상과 실제 원인

- DEV Windows 실제 프로세스 재실행에서 저장 JSON의 `maximized=true`, Tauri 내부 Win32 `IsZoomed=true`임에도 외부 테스트의 `IsZoomed=false`가 반복되었다.
- 최종 계측에서 외부 `Process.MainWindowHandle`은 서로 다른 HWND를 반환할 수 있음이 확인됐다. 예: 2026-11-17T13:39Z의 외부 GetWindowPlacement는 HWND=328140, 실패 시각의 실제 Tauri 본창은 HWND=262670이었다.
- 외부 검사가 임의의 대표 창을 검사한 **거짓 실패**가 주된 원인이었다. 최대화 상태 자체를 무조건 재설정하는 방식은 올바른 해결책이 아니다.

## 실제 수정

- `scripts/verify-dev-window-placement-win.ps1`: `EnumWindows`로 각 최상위 HWND를 순회하고 `GetWindowThreadProcessId`로 실제 프로세스 ID가 일치하면서 제목이 `반도허브 BANDO HUB`로 시작하는 창만 찾도록 수정.
- 기존 `.MainWindowHandle` 의존을 완전히 제거. 크기·위치·최대화·종료 요청을 모두 실제 메인 HWND에만 보낸다.
- `src-tauri/src/lib.rs`: 실험적으로 추가한 6초간 24회 강제 최대화 감시를 제거하고 종료 시 저장된 상태에 따라 제한된 초기 복원만 수행. 사용자 수동 복원을 방해하지 않음. Win32 배치 정보까지 복원하는 fallback 유지.
- `tests/tauri-window-placement.test.mjs` 및 `tests/dev-window-placement-real-restart.test.mjs`를 새 창 검증 계약으로 변경.

## 실제 Windows Tauri DEV 자동화 결과

- Windows CI: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44095388492 — **PASS**
- 전체 Rust/프런트엔드 CI: https://github.com/ThunderStrikerKR/TSBH/actions/runs/44095388577 — **PASS**
- GUI 업무 검사: **37건 PASS**, 스크린샷 **12개**
- 정상 창 크기·위치 복원: `DEV_WINDOW_RECT_RESTORED` = x=0, y=20, width=1024, height=700
- 저장 후 실제 EXE 종료·재시작·확대 후 종료·다시 시작하는 총 **3회 실행** 순환
- 재시작 최대화 유지 관측: `DEV_WINDOW_ZOOM_TIMELINE` = 70 samples, first=false, last=true, falseCount=3 (초기 0.3초 동안 복원 과도 상태)
- 최종 Win32 독립 확인: `DEV_WINDOW_WIN32_PLACEMENT` = ok=true, showCmd=3 (SW_SHOWMAXIMIZED), styleMax=true, hwnd=197142
- `DEV_WINDOW_MAXIMIZED_RESTORED: true`, `DEV_WINDOW_RESTART_PASS: true`, `DEV_WINDOW_PRODUCTION_ISOLATION: true`
- 일반 운영 파일 `window-state.json`의 SHA-256 변화를 검사해 DEV 설정 파일 `window-state-developer.json`과 격리 확인.
- CI는 Tauri debug EXE와 Windows GUI를 사용했음. Release 설치 EXE의 동일 환경 별도 재실행 테스트 결과는 이 증거에 포함하지 않는다.

## 판정

**DEV 최대화/창 크기·위치 복원: PASS**. 이전 오류에는 잘못된 HWND를 선택한 검사 방식이 관여했다. Release 빌드 또는 다중 모니터 환경까지 검증 완료로 확대하지 않는다.
