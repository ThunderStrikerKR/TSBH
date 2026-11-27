# 157차 상품·폴더 이동 저장 감사

기준: 55fb00a2de8c6bb67c89a040bb403e55f5ddf831. UI ProductManager → patch_product_v2 → update_v2_entity_with_color_policy (IMMEDIATE SQLite transaction) → validated write/readback → reload. 폴더 확인은 expectedScope를 사용한다.

- D01 기존 상품 이동 categories 단일 ID 교체: 논리적 최하위 소속 중 드래그한 소속만 교체, 중복 목적지 제거. 모호하거나 손상된 경로는 거부.
- D02 일반 상품 편집 reducer를 이동에 사용: 전용 좁은 변경으로 교체 (categories/category/updatedAt만).
- D03 무관한 색상 정규화: 분류 명령 전용 저장 경로에서 제외.
- D04 폴더 이동 시 무관한 category 재계산: 이동/이름 변경에 연결된 상품 및 명시 동의된 이전만 갱신.
- D05 catalog/modelKey 부수 변경: 상품 최상위 구분 이동은 별도 정책 승인 전 차단. 기존 폴더 이동 catalog 충돌 검사도 Rust에서 실제 parent 변경에 강제.
- D06 상품 이동 전역 revision: UI expectedScope 제공 및 기존 상품 원본 비교 유지. 관련 분류/소속이 바뀌면 거부. 다른 주문/가격 변경으로 인한 전체 revision만으로 거부하지 않음(선택 상품 전체 원본 비교는 유지).
- D07 폴더 루트/자손/비활성 이동: 실제 parent 변경 시 Rust에서도 검사.

남은 결정: 목적지에 직접 연결 상품이 있는 폴더를 하위 분류 추가로 비말단화할 때 기존 직접 소속을 유지할지 별도 동의할지. 기존 동작을 무단 변경하지 않음. 분류 상세 편집의 단일 선택 규격은 이번 감사의 드래그 이동 범위 밖이며 별도 검토 필요. 소스·테스트 작성과 CI/실제 SQLite/Windows 검증은 별개.
