# BANDO HUB egui Gateway 계약 (2026-11-19)

> **현재 상황:** egui 프리뷰의 공통 호출 계층은 구현되어 있다. **서버 API 자체는 아직 구현·배포되지 않았다.** 기본값은 오프라인 LocalGateway이며 주문 저장·운영 DB 연결은 계속 비활성화되어 있다.

## 1. 의존성 방향

```text
egui UI (main.rs)
    ↓
reception_service.rs   — 결과 조합·표시용 읽기 전용 모델
    ↓
BandoGateway (gateway.rs)  — load_catalog() / execute(ReadOperation, JSON)
    ├── LocalGateway  → 기존 src-tauri/src/business_core/recognition, rules
    │                  → 빌드 포함 state-seed.json (운영 DB 아님)
    └── HttpGateway   → HTTPS JSON API (명시적 선택시에만 사용)
```

- UI는 비즈니스 Rust 함수, SQLite, Tauri IPC, HTTP 라이브러리를 직접 호출하지 않는다.
- 기존 Rust 알고리즘과 JSON 요청/응답은 **단일 출처**로 유지한다.
- `ReadOperation` enum은 현재 프리뷰가 호출할 수 있는 세 연산만 허용한다:
  `recognition/analyze_order`, `rules/resolve_unit_price`, `rules/detect_remote_delivery_area`.
- read-only 연산만 연결했으며 결제·송장·주문 확정·DB 저장·삭제·업무 상태 변경 명령은 존재하지 않는다.
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

**(2) 읽기 전용 Rust 계산**

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
