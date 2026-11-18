# 운영 경로 기준 TypeScript 업무 판단 재조사 — 2026-11-19

기준: `src/main.tsx`부터 실제 TypeScript 모듈 의존성 추적 및 CI의 Vite/Rollup 배포 청크 검사. 원본 TypeScript 파일이 존재하는 것만으로 운영 실행을 뜻하지 않는다. 사용자 승인 이후 개선된 Rust 로직을 과거 TS 기준으로 되돌리지 않는다.

## 실제 중복 판정 수정

| 이전 실행 경로 | 중복 판단 | Rust 단일 권한 및 실행 경로 | 커밋 |
|---|---|---|---|
| `WorkflowQueue.tsx`의 `companyMissing = chosen.some(...)` | 발송용 송하인 상호/주소/연락처 검증 | `output.review_export_sender` → `output/export.rs::sender_review`; 실제 파일 출력의 `sender_complete` 검사와 공유 | `2a9e8468` |
| `ProductSearchDialog.tsx`의 `validProductUrl(...)` | 공식 상품 상세 URL 허용 판정 | `catalog_clients.search_products_with_urls` → `queries.rs`에서 `product_url_ok`를 실행하여 제품 ID/상세 링크 가능 ID 동시 반환 | `aeaa8a81` |

두 화면 모두 Rust IPC 실패 시 TypeScript의 과거 판정기로 대신 처리하지 않는다. 송하인 설정 버튼은 비동기 응답의 최신 선택 상태를 검증하며, 상품 검색 결과 정렬/필터 로직은 원래 Rust 검색 함수를 그대로 재사용한다.

## 운영 엔트리에서 접근되는 기존 TS 모듈 — 기능 구분

2026-11-18T22:28Z CI `audit-ts-business-retirement.mjs`는 총 186개 소스 모듈을 조사했다. 추적 대상 36개 중 7개를 아직 LIVE 후보로 분류했으며, 이 수치는 **사업 규칙의 실제 호출 수**가 아니라 **파일의 실행 경로 도달 가능 수**다.

| 기존 TS 파일 | 실제 사용 | 분류 |
|---|---|---|
| `domain.ts` | `uid`, `won`, `money`, `unassignedClient` 및 일부 화면 편의 기능; 기존 가격·검증 함수는 테스트 기준으로 잔존 | UI 공통부와 역사적 비교 함수 혼재 — 분리 후보 |
| `matching.ts` | `internalModel`, `customerModel` 등 이름 표시 변환 | 프레젠테이션. 기존 별칭 관련 규칙은 별도 역사적 비교 대상 |
| `automatic-message-templates.ts` | 편집기의 20종 템플릿 메타정보와 예시 미리보기 | UI 편집/미리보기. 최종 저장 검증은 Rust |
| `factory-reply-auto.ts` | 감시 파일 처리의 비동기 순서·중복 조기 확인·오류 알림 | 프런트 오케스트레이션. 최종 적용/저장은 Rust, 중복 사전검사는 향후 분리 검토 |
| `client-workbook.ts` | ExcelJS 파일 입출력·셀/서식·원본 파일 형식 확인 | 파일 형식 경계. 거래처 의미 분석과 등록 판정은 Rust |
| `product-unified-workbook.ts` | ExcelJS 파일 입출력·셀 추출·헤더 확인 | 파일 형식 경계. 상품 의미 분석/단가 저장 판정은 Rust |
| `product-registration-xlsx.ts` | 등록 템플릿/결과 ExcelJS 직렬화 | 출력 직렬화, 영업 규칙의 최종 판정 아님 |
| `factory-export.ts`, `store-export.ts` | Rust가 만든 행을 ExcelJS로 XLSX 직렬화 | 별도 SERIALIZER_ONLY 2개 |

`parser.ts`, `validation.ts`, `workflow.ts`, `shipping.ts`, `hub-messages.ts`, `order-evidence.ts` 및 `product-search.ts` 등은 소스에서 보존되지만 앱 진입 경로에서 도달하지 않는다. `parser.ts`, `validation.ts`는 번들 청크 목록에서도 0건이며 소스-그래프 및 번들 차단 규칙이 동작한다.

## 실제 화면에 남아 있는 조회용 TS 판단과 향후 변경 조건

- `order-types.ts::orderParty`는 `workflow-view.ts::shipmentContext`의 표시용 거래처 스냅샷 선택에 쓰인다. Rust 워크플로의 거래처 권한 결정을 바꾸지는 않지만, 화면과 Rust 스냅샷 기준 차이 여부는 검증하지 않았다. 변경하려면 기존 `preview_workflow` 등의 Rust 조회 응답을 활용해 JSX 전반의 동기 조회를 대체해야 한다.
- `actual-freight.ts::actualFreightOf`는 `ActualFreightField`, `WorkflowQueue`에서 저장 운임의 표시와 미변경 여부 확인에 쓰인다. **실제 운임 저장 및 이력 생성은 Rust `record_actual_freight`가 담당**한다. 반응성/입력값 보존이 중요한 현재 UI를 무리하게 비동기 IPC로 대체하지 않았다.
- `domain.ts` 안의 과거 가격/합계 함수는 원본 비교 테스트의 호출자가 많다. 실행 번들에서 사용되는 함수 수준을 보존하며 과거 원본을 분리하려면 테스트 오라클의 import를 먼저 재배치해야 한다.

따라서 위 잔존 항목까지 **TypeScript 업무 판단 0건**이라고 선언하지 않는다. 사용자 입력과 기확정 주문의 기존 값 보존을 우선하며, 실제 실행 경로의 비중복 조회와 업무 결정 권한을 구분한다.

## 검증 근거

- 2026-11-18 `5c0806a4` CI: 186개 TS 원본 소스/7개 LIVE 후보, SERIALIZER_ONLY 2개.
- `2a9e8468`: 신규 Rust sender_review 단위 테스트 + Native output 디스패처 통과. 786개 프런트 테스트 중 24개 기존 실패.
- `00f2619c`: 송하인 검증 프런트 계약 테스트 2개 PASS.
- `aeaa8a81`: 기존 Rust 검색과 확장 검색 ID 목록의 순서·필터 동일성 및 URL 허용 목록 Rust 단위 테스트 PASS.
- `98789029`: 상품 검색 URL 라우팅 관련 프런트 계약 테스트 2개 PASS. 기존 `search_products` 문자열을 고정한 검사 조건은 `4aa62d55`에서 새 API 이름으로 변경.
- Windows 실제 GUI와 전체 CI 통과 여부는 별도 게이트. 역사적 원본과의 기계적 동등성을 근거로 사용자 승인 개선을 되돌리지 않는다.
