# 45차 — 운영 화면의 TypeScript 거래처·실운임 선택 경계 제거 (2026-11-19)

범위: `main`, `src/main.tsx`에서 도달하는 실제 React → Tauri IPC → Rust 업무 경로. 선행 44차에서 Rust로 이전한 다섯 경로는 재작성하지 않았다. 단순 함수명/원본 파일 존재만으로 활성 업무 판단을 판정하지 않는다.

## 실제 수정

| 운영 호출부 | 제거한 TypeScript 결정 | 현재 권한 | 변경 근거 |
|---|---|---|---|
| `useOrderDesk` → 접수/상품 생성/가격 갱신 | `orderClient(draft,state)`, `state.clients.find(...)`로 현재·확정·삭제 거래처 선택 | Rust `rules.order_edit_rules({action:"resolve_order_clients"})`가 `client`와 `liveClient` 분리 반환. React `useNativeOrderClients`는 키 일치/응답 최신성·입력 비활성만 처리 | `63019b6e`, `048e5103`, `e7c92d43`, `c8eb68f7` |
| `WorkflowBoard` / `WorkflowQueue` / `workflow-view::shipmentContext` | `orderParty`에서 거래처 현재 마스터/저장 스냅샷 선택 | Rust `rules.read_shipments`의 `client` 사용; 거래처 누락은 빈 거래처를 추정하지 않고 화면에서 명시 | `afa0df5b`, `829d95a0`, `6a632d61`, `e35417a4`, `15530ce4`, `9be9bb34` |
| `ActualFreightField` / `WorkflowQueue` | `actualFreightOf`로 배송 건별·수취인 과거 값 우선순위 판정 | Rust `rules.read_shipments`가 저장값 projection 반환; React는 초안/저장값 비교만, 저장은 기존 Rust 업무 명령 | `afa0df5b`, `507d3cd3`, `e2e83db0` |
| `CorrectionPanel` 상품 변경 | 거래처 선택값을 동기 TypeScript 문맥에서 전달, `variants.length === 1 && !requiresOption` 가격 적격 사전 판정 | Rust 배송 거래처 조회와 기존 Rust `create_catalog_item`에서 상품·옵션 판정 | `339fccd2` |
| 작업 표시 안정성 | 이전 IPC 응답이 현재 거래처·운임을 덮는 위험 | 서명 키, 요청 취소 및 현재 결과 게이트; `WorkflowBoard`의 Rust 거래처 조회는 독립 업무 미리보기의 로딩 중에도 유지 | `1eda762d`, `bd2da63b` |

Rust `workflow::read_shipments`는 배송 건 자체의 `actualFreight`가 `null`이어도 수취인 과거값으로 덮어쓰지 않는다. 해당 키가 **없는 경우에만** 과거 수취인 값을 보조 사용한다. 0원과 null을 다른 의미로 유지한다.

## 소스 잔존 분류

- `domain.ts::orderClient`, `order-types.ts::orderParty`, `actual-freight.ts::actualFreightOf`의 정의와 역사적 테스트용 파일이 남더라도, 위 운영 화면에서는 호출하지 않는다. 기존 비교 기준을 검증 없이 삭제하지 않는다.
- `src` 내 75개 `.tsx` 호출부를 검색했으며 `orderClient(`, `orderParty(`, `actualFreightOf(` 및 대표 원본 계산 함수의 직접 호출은 확인되지 않았다. 이 검색은 *함수 실행 부재의 증거 중 하나*이며 전체 JS 런타임 의미론의 자동 증명은 아니다.
- 제품/거래처 ExcelJS 파서·직렬화 및 템플릿 미리보기는 파일 데이터·화면을 다루는 경계로 별도 분류한다. 이전 TypeScript 원본은 비교 시험자료일 뿐 운영 구현의 기준 권한이 아니다.
- `scripts/audit-ts-business-retirement.mjs`의 **모듈 도달성 개수와 실제 업무 판정 함수 실행 횟수는 구별**한다.

## 새 테스트와 남은 검증

- Rust 단위 테스트: 확정 주문의 거래처 스냅샷/마스터 분리, 삭제 거래처 보조, 소매 회사 정보, 배송 건 실제운임 null·0·누락 키 선택.
- `tests/tauri-live-native-read-boundaries.test.mjs`: 운영 import/호출 경계 확인. 동작 재현 GUI 테스트를 대신할 수 없다.
- 새 IPC 패키징/연결은 `npm run build`와 compiled Rust 테스트로 별도 확인한다. 44차 이전부터 남아 있는 가격/운임 4건 및 주문 저장 3건의 **과거 TS 비교 불일치**는 사용자가 승인한 동작의 실제 오류로 확인되기 전까지 이번 이관 실패로 합산하지 않는다.
- 프런트엔드 기존 24건의 실패는 44차 이전 commit `60f6d55a`의 CI에도 있었다. 이번 거래처 준비 게이트 때문에 문자열 기대가 바뀐 정적 테스트 1건을 `d00b687a`에서 갱신했다. 다만 CI 완료 여부에 따라 별도 검증한다.
- **Windows DEV 화면, 실제 EXE 재실행, 거래처 변경 중 화면 흔들림·초안 보존·입금/출고 실행 시나리오는 아직 성공을 선언하지 않는다.** 이 검증 없이 Rust 마이그레이션 전체 완료로 표기하지 않는다.

이 문서의 ‘제거’는 해당 경로의 업무 권한을 Rust로 단일화했다는 뜻이다. React/TypeScript 표시·입력 계층 자체를 삭제했다는 뜻이 아니다.
