# BANDO HUB egui Gateway 계약 (2026-11-19)

> **현재 상황:** egui 프리뷰의 공통 호출 계층은 구현되어 있다. **서버 API 자체는 아직 구현·배포되지 않았다.** 기본값은 오프라인 LocalGateway이며 주문 저장·운영 DB 연결은 계속 비활성화되어 있다.

## 1. 의존성 방향

```text
egui UI (main.rs)
    ↓
reception_service.rs   — 결과 조합·표시용 읽기 전용 모델
    ↓
BandoGateway (gateway.rs)  — catalog / read-only Rust operations / revisioned working-source
    ├── LocalGateway  → 기존 src-tauri/src/business_core/recognition, rules
    │                  → 격리 egui-preview.sqlite3 (초기 카탈로그는 state-seed.json)
    └── HttpGateway   → HTTPS JSON API (명시적 선택시에만 사용)
```

- UI는 비즈니스 Rust 함수, SQLite, Tauri IPC, HTTP 라이브러리를 직접 호출하지 않는다.
- 기존 Rust 알고리즘과 JSON 요청/응답은 **단일 출처**로 유지한다.
- `ReadOperation` enum은 인식/조회 6개와 **DB를 수정하지 않는 순수 Rust 주문 변환 연산 5개**만 허용한다:
  `recognition/analyze_order`, `rules/resolve_unit_price`, `rules/detect_remote_delivery_area`, `catalog_clients/auto_match_reception_client`, `catalog_clients/reception_client_candidates`, `catalog_clients/client_candidates` 및 `rules/apply_reception_action`, `rules/order_edit_rules`, `rules/merge_reception_analysis`, `rules/reprice_order`, `rules/calculate_price`. 거래처 매칭·검색·주문 수정 판단은 기존 Rust 엔진을 공유한다. **egui 이 연산들은 운영 DB에 아무 것도 저장하지 않는다.**
- 업무 엔진의 읽기 전용 연산만 연결했다. 단, 프리뷰 편집기 원문은 별도의 versioned working-source 저장 계약으로 자동저장한다. **주문 확정·결제·송장·업무 DB 변경 명령은 없다.**
- 새 기능은 UI가 아니라 Gateway 인터페이스 및 전용 서비스에 구현한다. 외부 Windows 창·카카오톡 도킹은 네트워크 업무 API와 별개의 로컬 어댑터 책임이다.

## 2. 설정

기본 실행 (환경변수 미설정):

```powershell
cargo run --manifest-path egui-app/Cargo.toml
```

이 경우 `BANDO_HUB_GATEWAY=local`과 같다. 기존 로컬 Rust 분석 결과를 사용하고 어떤 네트워크 요청도 발생하지 않는다.

**서버 구현·배포 후에만 사용할 설정 예시:**

```powershell
$env:BANDO_HUB_GATEWAY = "http"
$env:BANDO_HUB_API_BASE_URL = "https://api.example.invalid"
$env:BANDO_HUB_API_TOKEN = "<서버 발급 토큰>"
cargo run --manifest-path egui-app/Cargo.toml
```

- `example.invalid`은 실행 가능한 서비스가 아닌 설명용 예시이다.
- HTTPS가 필수다. 단, 개발용 localhost 및 loopback에서는 HTTP를 허용한다.
- 토큰은 소스, 원문 데이터, URL 쿼리, DB 또는 Git에 저장하지 않는다. 실제 운영 시 OS 자격증명 저장소와 토큰 갱신 방식을 별도로 설계한다.
- 서버 모드를 선택했는데 URL/인증 설정이 없거나 잘못되면 시작 오류가 나며 **몰래 로컬 모드로 돌아가지 않는다.**
- 서버 모드를 선택하지 않으면 토큰·서버 URL을 읽어 원격으로 보내는 동작도 없다.

## 3. 제안된 HTTP 계약 (미구현 서버 사양)

공통: `Authorization: Bearer <token>`, `Content-Type: application/json`.

**(1) 읽기 전용 상품 카탈로그**

```http
GET /api/v1/catalog
```

응답: `products`, `aliases`, `brands` 배열을 가진 최소 카탈로그 JSON. 주문 내역, 수취인, 거래처 상세, 결제 및 워크플로 데이터는 포함하지 않는다.

상품의 가격·원가 등 비공개 정보도 카탈로그에 포함될 수 있으므로 서버에서 사용자 역할별 상품 필드와 단가를 제한해야 한다. 전체 운영 DB 스냅샷을 그대로 반환하지 않는다.

**(2) 활성 원문 편집기 자동저장 (주문 확정과 무관)**

```http
GET /api/v1/working-source
PUT /api/v1/working-source
```

GET 응답: `{"raw":"현재 선택 원문","orderType":"AGENCY","activeId":"egui-1","drafts":[{"id":"egui-1","raw":"현재 선택 원문","orderType":"AGENCY"}],"revision":7}`. `orderType`은 `AGENCY`(대리발송) 또는 `DEALER`(대리점 납품)만 허용한다. `activeId`는 목록 안의 ID 또는 `null`이어야 한다. 빈 작업 목록은 `activeId: null`, `drafts: []`, `raw: ""`로 표현한다.

PUT 요청: `{"raw":"편집된 원문","orderType":"DEALER","activeId":"egui-1","drafts":[{"id":"egui-1","raw":"편집된 원문","orderType":"DEALER"}],"expectedRevision":7}`; 성공 응답은 `{"revision":8}`이다. 서버는 SQLite 트랜잭션/조건부 revision 비교로 동시 편집 충돌을 검증하고, 최신 상태가 다르면 HTTP `409 Conflict`를 반환해야 한다. 프리뷰 편집기 원문은 업무용 주문 확인 완료나 주문내역 이전을 의미하지 않는다. 민감한 주소·연락처를 취급하므로 저장 위치와 접근 제어에 주의한다. 해당 서버 라우트는 아직 **구현되지 않았다.**

**(3) 읽기 전용 Rust 계산**

```http
POST /api/v1/business-core
```

요청 예시:

```json
{
  "domain": "recognition",
  "operation": "analyze_order",
  "payload": {
    "raw": "708-1 1개",
    "state": {"products": [], "aliases": [], "brands": []}
  }
}
```

응답 예시:

```json
{"result": {"recipients": [], "candidates": [], "unassigned": []}}
```

서버는 `ReadOperation`에 해당하는 허용 목록만 처리해야 한다. 클라이언트가 보낸 `domain`과 `operation`을 아무 제한 없이 내부 관리자/변경 명령으로 전달해서는 안 된다. 실제 결과와 오류는 기존 Rust 코어의 JSON 계약을 따른다.

이 API는 현재 **설계된 클라이언트 계약일 뿐**, GitHub 저장소에 서버 라우터나 인증 서비스가 만들어졌다는 뜻이 아니다.

## 4. 향후 서버 데이터 연동 시 필수 조건

1. **인증과 접근 제어:** 거래처·매장·공장·관리자 역할을 서버에서 검증하고 최소 데이터만 보낸다.
2. **버전 및 충돌 제어:** 상태 스냅샷에 revision 또는 ETag를 부여하고 주문 변경은 서버에서 조건부 트랜잭션으로 처리한다.
3. **저장 작업 분리:** 순수 업무 판단 결과를 받았다는 이유로 저장이 완료된 것으로 처리하지 않는다. 승인·감사 이력·중복 요청 방지 키가 필요하다.
4. **인식 호출 최적화:** 현재 서비스는 카탈로그 조회 1회 + 분석 1회 + 수취인별 주소 조회 + 상품별 가격 조회를 수행한다. 서버로 이전하면 데이터 전송량과 지연이 늘 수 있으므로 묶음 조회/서버 측 조합을 추가할 계획이다. UI 변경 없이 서비스·Gateway만 조정한다.
5. **오프라인 정책:** 서버 연결 실패 시 로컬 결과로 자동 대체하거나 무단으로 큐잉하지 않는다. 향후 명시적 오프라인 동작을 정할 때만 추가한다.
6. **개인정보 보호:** 실제 발주원문/주소/전화번호가 서버로 전송되므로 서버·보안·보존 정책 승인 이전에 원격 모드를 운영에 사용하지 않는다.

## 5. 검증 및 롤백

- 기존 백업 브랜치: `backup/main-before-egui-20261119`.
- 이전 React + Tauri 파일과 운영 SQLite는 수정하지 않는다.
- 로컬 egui 프리뷰의 실제 컴파일과 Windows 화면 동작은 별도 확인이 필요하다. 문서 및 GitHub 소스 커밋만으로 빌드 성공이라고 판정하지 않는다.

## 6. egui 프리뷰 전용 로컬 저장

- Windows: `%LOCALAPPDATA%\\BANDO-HUB-egui-preview\\egui-preview.sqlite3`
- 홈 디렉터리 기반 환경: `~/.config/BANDO-HUB-egui-preview/egui-preview.sqlite3`
- SQLite 테이블: `preview_meta`(DB 식별자), `catalog_snapshot`(상품·별칭·브랜드), `working_source`(현재 편집기 원문·접수 유형·revision과 전체 미확정 접수 목록 `drafts_json`·선택 ID `active_id`).
- 카탈로그는 **처음 생성 시** 공식 내장 시드에서 넣는다. 내장 시드의 거래처는 0건이며 운영 DB를 무단 자동 탐색하지 않는다.
- 거래처 이름 인덱스는 `BANDO_EGUI_CLIENT_INDEX` JSON 파일 **또는** `BANDO_EGUI_BUSINESS_DB`에 명시한 기존 Tauri SQLite DB를 `SQLITE_OPEN_READ_ONLY`와 `PRAGMA query_only=ON`으로 조회해 메모리로 구성한다. 경로 자동 탐색·운영 스키마 수정·운영 주문 조회·단가 복제는 하지 않는다. 검색용 주소와 전화번호는 격리된 읽기 전용 메모리에서만 사용한다. 둘 다 설정되면 오류다. 둘 다 없으면 `clients=[]`이다.
- 저장소가 다른 구조이거나 DB 식별자가 다르면 마이그레이션하거나 덮어쓰지 않고 시작 오류로 종료한다.
- 원문·접수 유형·선택 ID·전체 미확정 접수 목록을 하나의 revision-checked SQLite 트랜잭션으로 저장한다. 입력은 별도 스레드에서 debounce 처리하고, 정상 종료 때 마지막 대기 저장을 마무리한다. 충돌 및 저장 오류는 UI 하단에 표시한다.
- **중요:** 이 저장 영역은 여러 미확정 발주원문의 선택 복원과 해당 원문에 대한 Rust 분석 근거·거래처 매칭 의견의 발주서별 보관만 제공한다. 거래처 확정·주문내역·결제·운임·발주서별 실제 처리 상태는 미이전이다.

## 7. 클립보드 접수 타입 및 OCR 경계 (2026-11-19)

- `clipboard_intake.rs`는 로컬 OS 클립보드 텍스트·이미지를 읽는 **로컬 UI 어댑터**다. 데이터 분류·상품 인식·가격 등 업무 판단은 여전히 `BandoGateway`를 경유해 기존 Rust 엔진에서 수행한다.
- 텍스트는 `대리발송`(AGENCY) / `대리점 납품`(DEALER) 선택과 함께 기존 `working_source`로 자동저장한다. `order_type`은 `raw`와 같은 revision-checked SQLite 트랜잭션으로 저장되며 이전 단일 원문 작업 데이터는 스키마 열 추가만으로 보존한다.
- 이미지가 들어오면 기존 `src-tauri/resources/ocr/recognize.py`와 PP-OCRv5 모델 실행 규약을 재사용한다. OCR 결과·이미지는 사용자가 `OCR 문자 인식 검토`에서 반영하기 전까지 저장하거나 Rust 업무 분석에 전달하지 않는다.
- OCR 이미지 작업은 Worker 스레드, 임시 폴더, 180초 제한을 사용한다. 원본 클립보드는 수신 시 읽기만 하고, 외부 실행 파일은 명시적 실행 경로 `BANDO_OCR_RUNTIME_DIR` 또는 egui 실행파일 옆 `ocr` 폴더에서만 선택한다.
- 클립보드에 유효한 텍스트/이미지가 없을 때 두 버튼은 비활성화된다. 400ms 간격의 상태 갱신과 Win32 시퀀스 번호로 반복 이미지 디코딩을 줄인다.
- 이미 존재하는 원문과 다르면 React 원본에서 쓰던 교체 확인을 적용하며, 사용자 승인 없는 덮어쓰기는 하지 않는다.
- HTTP 서버 구현체는 아직 없다. 향후 `working-source` 계약 구현 시 `orderType`을 함께 처리해야 하며, 필드 누락을 임의로 AGENCY로 기본 처리하지 않는다.
- **2026-11-20 반영:** 격리 egui에서 여러 미확정 발주원문을 각각 독립 ID로 유지한다. `+ 새 주문`으로 빈 접수를 열고, 오른쪽 `처리 중인 주문`에서 선택하면 해당 원문을 복원하여 Rust 분석을 다시 실행한다. 기존 단일 원문은 `legacy-working-source` ID로 메모리상 승계한 뒤 다음 사용자 변경 시 새 구조로 비파괴 저장한다.
- **2026-11-20 추가:** 기존 Rust `catalog_clients/title_clients.rs`의 발주원문 거래처 자동 매칭·후보 판단을 연결했다. egui Windows 카카오톡 창 어댑터는 실제 `KakaoTalk.exe`의 확인된 대화창 제목과 HWND만 제공한다. 사용자가 `카카오톡 연결`에서 대화를 선택하거나 최근 실제 활성 대화를 확인한 경우에만 창 정보를 Rust 매칭 엔진에 전달한다. 창 내용은 읽지 않는다. 거래처 명단이 없으면 매칭을 확정하지 않는다. 수취인 이름·학교·주소·상호만으로 발주처를 추측하지 않는다.
- **제한:** 거래처는 **격리 egui 미확정 발주서에만** 자동 후보 적용 또는 직원 수동 후보 선택으로 기록한다. Tauri 운영 주문의 거래처 확정·단가 계산·주문별 확인·공장 전달은 아직 이전되지 않았다. 오른쪽 목록은 egui 전용 미확정 접수 목록이다.

## 8. 거래처 이름 인덱스와 발주서별 분석 근거 (2026-11-20)

운영 DB를 자동 검색하거나 복사하지 않는다. 파일명과 경로는 사용자가 명시해야 한다. JSON 인덱스 파일뿐 아니라 기존 Tauri 관계형 SQLite의 거래처 테이블을 **읽기 전용**으로 조회할 수 있다.

```powershell
$env:BANDO_EGUI_CLIENT_INDEX = "C:\\private-data\\clients-index.json"
```

```json
{"clients":[{"id":"c-1","name":"예시악기사","shortNames":"예시악기","active":true}],"settings":{"kakaoTitleMappings":{}}}
```

- 원본에서 `id`, `name`, `displayName`, `shortNames`, `identifiers`, `active`, `deletedAt`과 **검색용 `address`, `phone`, `mobile`**만 투영한다. 주소·전화번호는 거래처 검색창이 열린 동안 읽기 전용 메모리에서 사용하며 egui 작업 DB에는 저장하지 않는다. 가격·계좌·주문내역은 제외한다. 최대 5,000건, 2MB다.
- 각 미확정 접수는 `analysisRaw`, `analysisEvidence`, `clientMatch`를 함께 저장할 수 있다. 분석 근거의 원문이 현재 원문과 다르면 저장을 거절하고, 입력이 변경될 때 기존 근거를 무효화한다.
- `RAW_EXPLICIT` 등은 영구 거래처 지정이 아니라 Rust의 읽기 전용 의견이다. 실제 지정은 이후 Rust 주문 변경·검증·저장 경로에서 처리한다.
- `kakao_context.rs`는 운영 Tauri의 창 검증 기준에 맞춰 실제 `KakaoTalk.exe`의 보이는 대화창 제목과 HWND를 조회한다. 창 배치·화면 임베딩·대화 메시지 조회는 하지 않는다. 선택된 HWND/제목이 현재 창 목록에서 일치하지 않으면 Rust 매칭에는 빈 창 목록을 전달한다.
- 직원이 Rust 후보 중 하나를 선택하면 접수별 `clientId`/`clientSelection=MANUAL`을 보존하고 Rust 분석을 다시 실행한다. 자동 고유 매칭은 `AUTO`로 저장한다. 선택한 거래처 ID는 egui 프리뷰에만 적용된다.
- 컴파일·Windows 실동작 확인 및 대규모 회귀 테스트는 수행하지 않았다.

### 관계형 운영 거래처 명단 읽기 전용 연결

Tauri 운영 DB 파일 경로를 **사용자가 명시한 경우에만** 조회한다. 아래 경로는 예시이며 실제 설치 위치를 확인해서 사용한다.

```powershell
$env:BANDO_EGUI_BUSINESS_DB = "C:\\path\\to\\bando-hub-v001.sqlite"
```

읽는 테이블은 `core_schema`(버전 2 확인), `core_clients`(거래처), `app_state` 내 `settings.kakaoTitleMappings`뿐이다. 운영 DB를 마이그레이션하거나 수정하지 않는다. 거래처 원본 payload에서 이름·별칭·주소·전화번호·활성/삭제 상태를 메모리용 검색 인덱스로 투영하고, 해당 주소·전화번호·가격·주문 내역은 egui SQLite에 저장하지 않는다. 기존 JSON 인덱스 환경변수와 동시 사용은 금지한다.

### 발주서별 대화창·거래처 선택 계약

각 draft에는 다음 선택 필드가 추가될 수 있다: `chatHandle`, `chatTitle`(둘 다 존재하거나 둘 다 null), `clientId`, `clientSelection`(`AUTO` 또는 `MANUAL`), `analysisRaw`, `analysisEvidence`, `clientMatch`. 서버가 workspace 계약을 구현할 경우 누락·드롭 없이 왕복 보존해야 한다. 원문이 수정되거나 대화창 선택이 달라지면 기존 분석 근거를 무효화한다. 사용자가 수동 선택한 거래처는 다른 원문/대화 분석 때문에 자동으로 덮어쓰지 않는다.

자동 판정은 기존 Rust `catalog_clients/auto_match_reception_client`를 그대로 호출한다. `windows`와 `activeChatHandle`은 OS에서 검증된 대화창 정보만 전달한다. 복수 대화창에서 활성 창이 특정되지 않으면 임의로 고르지 않는다. Tauri 운영 주문 상태와는 여전히 완전히 분리되어 있다.

## 9. 전체 거래처 검색 및 미확정 수동 선택 (2026-11-20)

- 기존 React `ClientChooser`의 상호 / 별칭 / 주소 / 전화번호 검색 범위를 그대로 사용한다. Rust의 기존 `catalog_clients/queries.rs::client_search`를 공유 모듈 `client_search.rs`로 추출하여 Tauri와 egui가 같은 판단을 호출한다. 순서·공백 무시·삭제 거래처 제외 규칙을 유지한다.
- `client_chooser.rs`는 로컬 읽기 전용 이름 인덱스에서 최대 5,000개 거래처를 받아 GUI 검색으로 제공한다. `+ 새 주문`과 접수 원문 영역 크기, 4열 분할 및 82pt 거래처 영역 크기를 변경하지 않는다.
- 발주원문에 거래처 정보 영역이 보일 때 `거래처 찾기` / `변경`으로 전체 검색을 열 수 있다. 검색과 목록 조회는 작업 스레드에서 수행한다.
- 실제 발주서가 아닌 egui 격리 미확정 접수의 `clientId`와 `clientSelection=MANUAL`만 설정하며, 원문 분석을 무효화하고 기존 Rust 인식을 재실행한다. 같은 거래처를 다시 클릭한 경우 아무 것도 변경하지 않는다. 검색창이 열려 있는 동안 다른 발주서로 전환하면 해당 검색창을 닫는다.
- 원본 Tauri SQLite는 `SQLITE_OPEN_READ_ONLY` 및 `PRAGMA query_only=ON` 방식으로만 접속한다. 영구 주문의 거래처 변경은 **아직 실행하지 않는다**. 실제 Rust `order_edit_rules/change_order_client`와 revision-checked SQLite 저장 연결은 다음 단계에서 수행한다.
- 카카오톡 실제 창 연결이 아닌 대화 제목만 표시하는 문제는 독립 추적: https://github.com/ThunderStrikerKR/TSBH/issues/42. 이 단계에서 카카오톡 화면을 수정하지 않는다.
- Windows Cargo 빌드/GUI 동작 확인은 아직 수행하지 않았다. 대규모 회귀 테스트를 추가하거나 실행하지 않았다.

## 10. 거래처 변경 Rust 주문 규칙 연결 (2026-11-20)

- 기존 Tauri의 운영 거래처 변경 규칙 `rules/order_edit_rules {action: change_order_client}`를 egui Gateway로 연결했다. 원문 분석에서 실제 고객별 상품 별칭을 적용하려면 선택한 거래처의 **전체 원본 master**가 필요하므로 `BANDO_EGUI_BUSINESS_DB`로 명시한 v2 운영 DB를 `SQLITE_OPEN_READ_ONLY` 및 `PRAGMA query_only=ON`으로 조회한다.
- `business_state.rs`는 하나의 SQLite 읽기 트랜잭션에서 `app_state`, `core_products`, `core_product_prices`, `core_clients`, `core_orders`, `core_recipients`, `core_items`를 관계형 순서대로 읽고, 기존 Tauri `relational_store::hydrate`와 동일한 핵심 객체 구조로 복원한다. 설정·workflow/청구 이력은 규칙 실행 동안 메모리에서만 유지한다.
- 전체 JSON 거래처 인덱스만 설정했거나 DB 버전이 v2가 아닌 경우, 실운영 단가·운임 검증을 수행하지 않는다. 운영 DB에 이미 같은 주문 ID가 있으면 격리 접수용 임시 주문으로 변경하지 않는다.
- 직원이 다른 거래처를 선택하면 원문/주문 ID를 고정한 작업 스레드에서 Rust `new_reception → analyze_order(선택 거래처) → change_order_client → merge_reception_analysis → reprice_order → calculate_price` 순서로 수행한다. 기존 단가/운임 및 업무 상태 판단을 egui에서 복제하지 않는다.
- 성공하면 **egui 미확정 접수에만** `clientId`, `clientSelection=MANUAL`, `pricingPreview`를 저장한다. `pricingPreview`는 원문/거래처/DB revision/품목별 단가/운임/VAT/총액만 허용하며 고객 주소·전화·계좌·전체 order snapshot·청구 이력을 포함하지 않는다. 같은 원문/거래처를 검증하고 egui 전용 SQLite revision-checked 자동저장으로 보관한다. 실패하면 기존 거래처를 보존한다.
- 제품 마스터가 명시적 운영 DB에 연결되면 접수 인식도 해당 DB의 실제 상품/옵션/별칭을 이용한다. 오래된 내장 상품 시드의 ID로 인식하고 최신 운영 DB로 가격을 매기는 혼합을 금지한다.
- **중요한 잔여 범위:** 이 경로는 실제 Rust 주문 수정 규칙과 실제 마스터 값을 사용해 **미확정 주문의 참고 견적을 준비**하지만, 운영 `app_state/core_orders/reception_working_orders`를 **갱신하지 않는다**. 영구 운영 주문 변경을 완료하려면 기존 Tauri `write_snapshot/state_save`의 검증, workflow reconcile, revision 및 감사 이력 처리를 동일 저장 트랜잭션으로 호출하는 **명시적 전용 게이트웨이**가 추가로 필요하다. 임의 직접 SQL `UPDATE core_orders/client_id`는 절대 수행하지 않는다.
- 원문 하이라이트와 수취인 수동 분류는 다음 단계이며, 카카오톡 창 연결 문제는 독립 Issue #42에서 계속 추적한다.
- 실제 Windows Cargo 컴파일/GUI 확인은 아직 하지 않았고 대규모 회귀 테스트도 실행하지 않았다.

## 11. Tauri 미확정 접수 작업본의 명시적 저장 (2026-11-20)

- 앞선 §10까지는 읽기 전용 견적 단계다. 이 절부터는 **사용자 클릭과 별도 환경 설정이 모두 있는 경우에 한해** 운영 DB의 `reception_working_orders`에 저장할 수 있다. 정식 주문 원장 `core_orders` 및 `app_state`는 그대로 둔다.
- 경로: egui `BandoGateway::commit_working_reception` → 사용자가 지정한 최신 Tauri EXE `--bando-egui-reception-commit` → `egui_reception_bridge::commit_file` → SQLite 즉시 트랜잭션. 인증 없는 HTTP 포트·브라우저 IPC는 추가하지 않았다.
- CLI 입력 표준입력 JSON: `orderId`, `sourceRaw`, `orderType`, `clientId`, `expectedAuthorityRevision`, `expectedWorkingVersion`(초기 null). 변경 시각은 Tauri 네이티브 프로세스가 직접 생성한다. 반환: `{ok:true,result:{orderId,clientId,workingVersion,authorityRevision,status,subtotal,shippingFee,vatAmount,total}}` 또는 `{ok:false,error}`. 송하인/수취인 개인정보, 계좌, 주문 원장 전체를 응답하지 않는다.
- 재계산: SQLite v2 관계형 전체 snapshot을 트랜잭션 안에서 다시 읽고, `business_core`가 `new_reception` 또는 `replace_reception`, `analyze_order`, `change_order_client`, `merge_reception_analysis`, `reprice_order`, `calculate_price`를 순서대로 실행한다. 운영 주문 ID 중복 및 이미 확인 완료된 주문은 거부한다.
- 기존 egui 작업본에 `_eguiBridgeVersion`, `_eguiBridgeAuthorityRevision`, `_eguiBridgePayloadHash`를 저장해 작업본 변경 감지와 CAS 갱신을 한다. 기존 React/Tauri `working_receptions::replace`는 누락되거나 버전이 다른 egui 작업본을 발견하면 저장을 거부한다.
- egui 격리 DB에는 `workingVersion`(CAS 토큰), `workingSynced`(현재 원문·유형·거래처의 마지막 저장 반영 여부)가 추가된다. 작업본 제출/수정은 자동 저장이 아니라 **운영 작업본 저장** 버튼의 명시적 클릭으로만 수행한다.
- 연동은 `BANDO_EGUI_BUSINESS_DB`, `BANDO_EGUI_TAURI_HELPER`, `BANDO_EGUI_ENABLE_WORKING_COMMIT=1` 세 설정이 모두 필요한 opt-in 방식이며, 설정이 없으면 기존 읽기 전용 미리보기가 유지된다.
- 이 단계는 `주문 확인 완료`로 정식 원장에 반영하는 `write_snapshot/state_save`가 아니다. 원문의 자동 분류 및 실제 단가 계산과 별개로 최종 검수, 수취인 수동 분류, 정식 확정과 배송 흐름은 아직 이어서 이전해야 한다.
- Windows Cargo 빌드, 격리 DB에서 동시 실행 검증은 미실시. Tauri 네이티브 단건 보관을 외부 운영 환경에 배포하기 전 수동 검증이 필요하다. 카카오톡 연결은 Issue #42에 남긴다.

## 12. Native Rust 수취인 수동 분류 작업본 (2026-11-20)

- UI는 `ReadOperation::ManualClassification`으로 기존 Rust `rules/apply_manual_classification`을 호출한다. `initial_manual`, `assign_manual_block`, `release_manual_block`, `resize_manual_rows`, `add_manual_item`, `set_manual_recipient_field`, `set_manual_product`, `set_manual_quantity_draft`, `confirm_manual_quantity`, `manual_quantity_status`, `apply_manual`만 1차 egui 작업 흐름에서 사용한다.
- 원문 블록의 `start/end`는 Rust의 UTF-16 근거 위치를 그대로 사용하고 `source_highlight::boundary`에서 유효한 UTF-8 문자 경계로 검사한다. 실제 원문·귀속 대상·분류 판단을 egui에 별도 구현하지 않는다.
- `WorkingReception.manual_model`은 `{sourceRaw,orderType,clientId,model,assignmentOrder}`만 허용하며 해당 발주서의 원문/유형/거래처와 반드시 일치해야 한다. 사용자가 **초안에 적용**하고 Rust가 `apply_manual`을 성공적으로 반환한 경우에만 격리 작업 DB에 기록한다.
- 활성 발주서 ID, 원문, 유형, 거래처가 분류 도중 바뀌면 결과를 거부한다. 신규 원문/거래처 변경 시 이전 수동 근거를 무효화한다. 재분석/재시작 시에는 저장된 모델을 Rust `apply_manual`로 재적용한다.
- 현재 `src-tauri/src/egui_reception_bridge.rs`의 운영 작업본 커밋은 자동 인식을 다시 수행하는 계약이라 수동 배정이 손실될 수 있다. **수동 분류가 있는 접수에 대해서는 운영 동기화를 의도적으로 차단**하며 직접 SQL로 우회하지 않는다. 수동 모델의 서버 검증, native scoped IDs, revision/작업본 버전·상품 재가격을 하나의 트랜잭션으로 연결하는 후속 단계가 필요하다.
- 배송 감소/삭제 확인, 고급 상품 검색, 키보드 표 조작, 운영 동기화 및 Windows 컴파일·시각 검증은 후속 단계로 보류한다. 카카오톡 Issue #42는 변경하지 않는다.
