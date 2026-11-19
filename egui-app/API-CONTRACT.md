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
- `ReadOperation` enum은 현재 프리뷰가 호출할 수 있는 세 연산만 허용한다:
  `recognition/analyze_order`, `rules/resolve_unit_price`, `rules/detect_remote_delivery_area`.
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

GET 응답: `{"raw":"텍스트 원문","orderType":"AGENCY","revision":7}`. `orderType`은 `AGENCY`(대리발송) 또는 `DEALER`(대리점 납품)만 허용한다.

PUT 요청: `{"raw":"편집된 원문","orderType":"DEALER","expectedRevision":7}`; 성공 응답은 `{"revision":8}`이다. 서버는 SQLite 트랜잭션/조건부 revision 비교로 동시 편집 충돌을 검증하고, 최신 상태가 다르면 HTTP `409 Conflict`를 반환해야 한다. 프리뷰 편집기 원문은 업무용 주문 확인 완료나 주문내역 이전을 의미하지 않는다. 민감한 주소·연락처를 취급하므로 저장 위치와 접근 제어에 주의한다. 해당 서버 라우트는 아직 **구현되지 않았다.**

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
- SQLite 테이블: `preview_meta`(DB 식별자), `catalog_snapshot`(상품·별칭·브랜드), `working_source`(현재 편집기 원문·접수 유형·revision).
- 카탈로그는 **처음 생성 시** 공식 내장 시드에서 넣는다. 이 원본 시드는 실제 운영 DB에서 최신 거래처·상품을 자동 수집한 것이 아니다.
- 저장소가 다른 구조이거나 DB 식별자가 다르면 마이그레이션하거나 덮어쓰지 않고 시작 오류로 종료한다.
- 원문은 입력 중 별도 스레드에서 debounce 후 SQLite에 저장하고, 정상 종료 때 마지막 대기 저장을 마무리한다. 충돌 및 저장 오류는 UI 하단에 표시한다.
- **중요:** 이 저장 영역은 프로토타입의 편집기 지속성만 제공한다. 거래처·주문내역·결제·운임·발주서별 처리 상태가 완성됐다는 의미가 아니다.

## 7. 클립보드 접수 타입 및 OCR 경계 (2026-11-19)

- `clipboard_intake.rs`는 로컬 OS 클립보드 텍스트·이미지를 읽는 **로컬 UI 어댑터**다. 데이터 분류·상품 인식·가격 등 업무 판단은 여전히 `BandoGateway`를 경유해 기존 Rust 엔진에서 수행한다.
- 텍스트는 `대리발송`(AGENCY) / `대리점 납품`(DEALER) 선택과 함께 기존 `working_source`로 자동저장한다. `order_type`은 `raw`와 같은 revision-checked SQLite 트랜잭션으로 저장되며 이전 단일 원문 작업 데이터는 스키마 열 추가만으로 보존한다.
- 이미지가 들어오면 기존 `src-tauri/resources/ocr/recognize.py`와 PP-OCRv5 모델 실행 규약을 재사용한다. OCR 결과·이미지는 사용자가 `OCR 문자 인식 검토`에서 반영하기 전까지 저장하거나 Rust 업무 분석에 전달하지 않는다.
- OCR 이미지 작업은 Worker 스레드, 임시 폴더, 180초 제한을 사용한다. 원본 클립보드는 수신 시 읽기만 하고, 외부 실행 파일은 명시적 실행 경로 `BANDO_OCR_RUNTIME_DIR` 또는 egui 실행파일 옆 `ocr` 폴더에서만 선택한다.
- 클립보드에 유효한 텍스트/이미지가 없을 때 두 버튼은 비활성화된다. 400ms 간격의 상태 갱신과 Win32 시퀀스 번호로 반복 이미지 디코딩을 줄인다.
- 이미 존재하는 원문과 다르면 React 원본에서 쓰던 교체 확인을 적용하며, 사용자 승인 없는 덮어쓰기는 하지 않는다.
- HTTP 서버 구현체는 아직 없다. 향후 `working-source` 계약 구현 시 `orderType`을 함께 처리해야 하며, 필드 누락을 임의로 AGENCY로 기본 처리하지 않는다.
- **제한:** 현재 저장소는 격리 egui 작업 원문 1건이다. 여러 발주서, 거래처 자동 연결, 영구 주문 내역, 공장 전달 등 실운영 업무 이전 완료로 취급하지 않는다.
