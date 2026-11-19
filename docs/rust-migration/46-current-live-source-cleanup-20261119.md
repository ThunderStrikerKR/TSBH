# 46차 — 운영 소스 정리 및 검증표 현행화 (2026-11-19)

기준: `main`의 실제 React 진입점 `src/main.tsx`, Tauri IPC, Rust 소스. 정리 직전 근거 커밋 `ecf0f95d4b253cd52637cc7d03a2d8b1d8b4489b`. **회귀테스트·실제 Windows 빌드/실행은 사용자 요청에 따라 이번 변경에서 수행하지 않았다.** 이전 CI 결과를 최신 변경의 통과로 재사용하지 않는다.

## 확정 구현 및 소스 결정

| 구분 | 실제 적용 내용 | 커밋 |
|---|---|---|
| 도서·산간 운임 | `OrderDesk`에서 2배/역환산 판단 제거. `rules.normalize_remote_freight_input` / `remote_delivery::normalize_applied_input`가 기본 금액 환산, 음수/홀수 확인, 적용 여부를 결정한다. React는 입력과 최신 결과 반영만 담당 | `d9adc3b` |
| 구형 TypeScript | `src/parser.ts`, `validation.ts`, `workflow.ts`, `shipping.ts`, `hub-messages.ts`, `receipt.ts`를 운영 `src`에서 삭제. `tests/rust-migration/oracles/legacy-*.ts`로 보존. 테스트 참조를 이동하고 `StaffBlock` 타입을 `src/staff-block-types.ts`로 독립 | `ecf0f95` |
| 사용하지 않는 화면·개발기능 | `AppNotice`, 구형 report-workbook, DeveloperLab/DeveloperModalWorkbench/TestDatabasePanel/BackupRestoreDrill, WorkingRawDialog 제거. 필요 시험 자료는 테스트 오라클로 이동 | `ebc21eb` |
| 원문 정정 UI | `WorkingRawDialog`는 운영 진입점이 없고 개발자 검사 경로에서만 쓰여 복구하지 않음. `CorrectionPanel`·Rust `apply_raw_correction`은 유지하며 기능 자체 제거로 처리하지 않음 | `ebc21eb` |
| 실제 알림/결산 | `App.tsx → FactoryNotifications`, `DailySettlement`, `OrderHistory`는 활성 화면이라 유지 | 소스 확인 |
| 구형 패치 | 단계별 APPLY 실행파일·과거 사용설명 8개 삭제; 현행 `DEV_WINDOWS_TAURI.cmd`, `PACKAGE_WINDOWS_TAURI.cmd` 유지 | `5a0d6b5` |

## 진짜 남은 확인 (운영 추적 대상)

1. **적용 운임 입력 동작**: 자동 체크와 직원 해제, 주소 변경, 숫자 입력 후 blur/Enter, 오래된 IPC 응답 경쟁 시 저장 금액·화면 표시 일치 여부. 구현은 반영됐지만 Windows 실행 증거가 없다.
2. **오라클 격리 이후 빌드 무결성**: Typescript/Vite/Rust 패키징·실행 여부를 현행 소스 커밋 기준으로 검증해야 한다. 이전 CI/타입체크는 이 변경의 검증이 아니다. 회귀테스트는 수행하지 않았다.
3. **최종 설치·데이터 보존**: Windows DEV/배포 실행, 기존 SQLite 파일 유지, OCR 동봉 ZIP의 실제 동작을 이번 정리로 확인하지 않았다.

## 미완료로 *집계하지 않는* 항목

- 과거 28개 `features[].overall: OPEN`은 기능별 미확인 **검증 증빙**이며 28개의 현재 기능 결함이 아니다. 운영 진행상태는 `verification-ledger.json → activeUnresolved`만 본다. 기존 행은 감사 이력으로 보존한다.
- 구형 TypeScript 비교와 승인 이후 Rust 개선 차이는 그 자체로 오류가 아니다. 기존 실패 24건도 최근 수정으로 생긴 결함이라고 단정할 수 없다.
- `src/domain.ts`의 사용되지 않는 구형 산식 정의, `matching.ts` 등의 화면 보조 로직은 원칙적으로 호출 경로·실제 판단 결과가 확인되었을 때만 추가 작업으로 등록한다.
- 사용자가 요청하지 않은 독립 원문 정정 모달 복구, 미사용 개발 UI 재활성화, 구형 패치 실행파일 복구를 작업 목록에 넣지 않는다.

## 이전 문서 정정

- [03차 이관 체크리스트](./03-ts-retirement-checklist.md)의 `migration/03-integration` 미완료 표와 `src/parser.ts` 보존 지시는 2026-11-13의 역사적 기록이다. 2026-11-19 `main`에서는 소스가 이미 테스트 오라클로 이동했다.
- [44차 소스 판단 정리](./44-live-ts-business-decision-cleanup-20261119.md)의 `orderParty`, `actualFreightOf` 등 미완료 기술은 후속 [45차 Rust 조회 경계 변경](./45-live-ts-read-boundary-closure-20261119.md)으로 운영 경로에서 처리되었다. 소스에 남은 정의만 보고 다시 미완료로 올리지 않는다.
- 새 사실이 확인되기 전에는 중복 '전수조사'를 새로운 미완료 항목으로 생성하지 않는다.

**보호사항**: Rust 실패 시 TypeScript 업무 판단으로 폴백하지 않는다. 직원의 금액/체크 수정, 데이터 보존, SQLite revision 단일 커밋, 부가세 및 선불/착불 의미를 임의 변경하지 않는다.
