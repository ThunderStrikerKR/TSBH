# 접수 화면 → 다른 내비게이션: 확인창 전체 경로 감사

최신 main 실제 소스 기준 (2026-11-19). '대화 확인'과 SQLite 저장은 별개다.

## 데이터 소유권
- 확인 전 발주서: `reception_working_orders` SQLite 테이블에 즉시 자동 저장; 재실행 시 `working_receptions_load`로 `HubWorkspace.edits` 복원. 화면 목록은 기존 Rust `reconcile_workflow` 프리뷰로 표시할 수 있지만 이것이 영속화 여부를 뜻하지 않음.
- 주문 확인 완료: `workflow.rs`의 1단계 진행에서 `confirmedAt` 및 불변 스냅샷 생성, 이때부터 `state.orders`와 주문내역 조회 대상으로 관리.
- 일반 탭 이동은 저장된 접수의 처리 상태를 바꾸거나 삭제하지 않음.

## 접수 화면 상단 내비게이션을 눌렀을 때

| 코드 경로 | 정확한 조건 | 기존/현재 사용자 노출 | 조치 |
|---|---|---|---|
| `AppHeader.onNavigate → useAppModel.navigate` | 현재 탭과 다른 메뉴 클릭 | 전환 요청 | 유지 |
| `navigate → HubWorkspace.leaveGuardRef → workingWriter.enqueue` | 미확정 접수가 있거나 SQLite 동기화가 진행 중 | 정상 시 팝업 없음; 저장 실패 시 오류 메시지 후 이동 중단 | 자동저장·완료 대기 반영 |
| `HubWorkspace.leaveGuardRef → askSaveChanges` | **SQLite 정식 주문에 속하는 기존 확정 주문**을 현재 화면에서 수정했으나 저장 전인 경우 | 「수정한 주문 정보가 저장되지 않았습니다. 저장하고 이동할까요?」; 저장/저장 안 함/취소 | 유효한 데이터 보호 경로로 유지; 이 경로를 미확정 접수에 적용하지 않음 |
| `useAppModel.navigate → hasPendingChanges` | 다른 활성 편집 컴포넌트가 미저장 상태이고 위 guard가 처리하지 못한 경우 | 별도 대화상자는 **아님**. 「저장하지 않은 편집창이 있습니다. 편집창에서 저장하거나 취소한 뒤 이동하세요.」 오류 안내 후 이동 차단 | 유지·별도 UX 개선 대상 |
| `useAppModel.confirmReceptionDraftLeave` | leave guard가 처리하지 않은, 기존 확정 주문의 `draft`가 dirty인 경우 | 「접수 화면에 저장하지 않은 변경사항이 있습니다. 어떻게 할까요?」; 저장/저장 안 함/취소 | 중복 가능성 있어 추후 통합 필요. 미확정 접수에는 사용하지 않음 |
| `HubWorkspace` 기존 `temporaryReceipts` leave modal | 과거: 단순 탭 이동 및 미확정 발주서 1건 이상 | 「미완성 발주서 처리 / 임시 보관하고 이동」 반복 팝업 | 제거 |
| `useAppModel.confirmReceptionDraftLeave` 기존 fallback | 과거: 미확정 초안에 dirty가 있지만 guard 미등록/미처리 | 「주문 확인 완료 전이므로 정식 저장되지 않습니다...」 | 제거; 현재는 SQLite 저장상태 불확실할 때 이동 차단 및 오류 안내 |
| 일반 탭 전환 시 `requestPageLeave` | **접수가 아닌 다른 탭**에서 또 다른 메뉴로 이동하는 경우만 실행 | 등록한 편집기의 미저장 변경이 있으면 「저장하지 않은 변경사항이 있습니다」 3선택 대화상자 | 접수→타 탭의 정상 경로에는 없음 |

## 다른 상황과 구분
- **처리 중인 주문 묶음/수취인 전환**: `HubWorkspace.setSelected`에서 `requestPageLeave`가 먼저 실행될 수 있음. 등록된 편집기가 dirty이면 대화상자가 뜰 수 있으나 상단 내비게이션 전환과 별개.
- **프로그램 닫기**: `useAppModel.windowAction("close")`가 `HubWorkspace` SQLite 동기화를 먼저 기다림. 미확정 접수가 있다는 이유만의 손실 확인은 제거. 정식 주문 수정 사항과 활성 편집기 변경사항 경고는 별도 확인 대상. `tauri-bridge.requestClose(true)`는 브라우저 `window.confirm("저장하지 않은 변경사항이 있습니다. 종료할까요?")`를 표시할 수 있음.
- **새 주문 버튼·수신 발주서 열기**: `confirmOrderLeave("switch")`가 실행되므로 저장된 정식 주문의 편집 상태에 따라 위와 같은 3선택 팝업이 가능함. 이는 '탭 이동'과 별개.
- **원터치 업무 버튼 / 공장 이관 / 그룹 삭제**: 별도 검증·확인창이며 상단 내비게이션이 자동 실행시키지 않음.

## 남은 검증
1. Windows EXE에서 접수 2건 → 접수 탭 이탈 → 종료 → 재실행 시 주문 묶음/원문/수취인/상품 정확 복원
2. 저장 전후 DB를 조회해 `core_orders`/주문내역 미노출을 확인하고, 1단계 확인 이후 정확히 한 번 노출되는지 확인
3. 빠른 붙여넣기와 탭 이탈·종료 시 마지막 원문 유실이 없는지 확인
4. 정식 주문 수정 도중 이탈하는 3선택 확인창, 활성 편집기의 별도 변경사항 대화상자 겹침 여부 확인

완료 판정은 실제 컴파일·테스트·Windows 재시작 복원 이후에만 한다.
