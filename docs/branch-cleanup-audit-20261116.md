# TSBH 원격 브랜치 정리 감사 — 2026-11-16

> **이 문서는 안전한 삭제 후보 목록이지 삭제 완료 기록이 아니다.**
> GitHub에 열린 PR 6건은 앞서 정리되었으며 #24는 이후 `main`에 병합 완료됐다.
> `main` 병합 직후 원격 브랜치 49개를 확인했다. 각 브랜치의 HEAD를
> `main`과 **실제 커밋 ancestry**로 비교했다. 코드 내용이 비슷하다는 이유로
> 분기된 커밋을 삭제 가능하다고 분류하지 않았다.

## 절대로 삭제하지 않을 2개 기준선

- `main`: 현재 단일 개발 통합 기준선.
- `integration/final-rust-migration-20261116`: #24의 최종 통합 기준선. 당분간 회귀 비교·문제 발생 시 원인 추적용으로 보존.

## 즉시 보존할 백업 7개

- `backup/main-before-rust-integration-20261116`: 병합 전 구 `main`, `30a19cd2da7533dd2ed36b807593309bde70a7eb`.
- `backup/feature-before-brand-name-fix-20261109`
- `backup/feature-before-catalog-names-20261109`
- `backup/feature-before-desk-tray-cleanup-20261109`
- `backup/feature-before-led-name-fix-20261109`
- `backup/feature-before-order-fix-20261109`
- `backup/feature-before-prefix-safetyfix-20261109`

## 커밋 기준으로 완전히 통합된 브랜치 19개 — 삭제 후보

아래 브랜치는 **해당 HEAD의 모든 커밋이 `main`의 조상**이었다
(`compare(base=old_branch, head=main)`: behind=0). 삭제는 이름이 아니라 이 증거에 따른다.

1. `feature/tauri-product-bulk-registration`
2. `fix/product-price-explicit-save-preserve-draft`
3. `fix/quantity-missing-provisional-button`
4. `fix/request-message-hover-links`
5. `fix/03-client-registration-native-audit`
6. `fix/03-kakao-win32-verified`
7. `fix/03-rust-retire-p0-billing-notice-client`
8. `audit/03-ts-runtime-after-p0-20261114`
9. `test/quantity-one-automatic-message-flow`
10. `integration/06-history-settlement`
11. `integration/07-catalog-clients`
12. `migration/04-workflow-preview`
13. `migration/05-factory-reply`
14. `migration/06-history-settlement`
15. `migration/07-catalog-clients`
16. `migration/27-unified-rust`
17. `migration/28-order-edit-rules-rust`
18. `migration/28-reception-manual-rust`
19. `migration/28-ts-runtime-closure`

브랜치를 삭제해도 해당 커밋의 변경 내역은 `main`에 남는다.
이 정리는 현재 연결된 GitHub 도구에 원격 브랜치 삭제 동작이 없어
**아직 서버에 적용하지 않았다**.

## `main`에 없는 고유 커밋이 남아 있는 21개 — 삭제 보류

`main`과 커밋 이력이 갈라져 있다. 고유 커밋은 중복 구현일 수도
있으나 실제 소스 차이와 미반영 사항을 확인하기 전에는 삭제하지 않는다.

| 브랜치 | `main`에 없는 고유 커밋 수 |
| --- | ---: |
| `feature/product-name-first-no-codes` | 17 |
| `fix/03-kakao-native-ci-20261113` | 6 |
| `audit/windows-e2e-final-20261114` | 7 |
| `audit/windows-gui-20261114` | 9 |
| `audit/08-windows-real-gui` | 6 |
| `test/desk-spec-product-names-20261109` | 13 |
| `test/guaranteed-brand-prefix-cleanup-20261109` | 3 |
| `test/normalize-mighty-bright-led-names-20261109` | 9 |
| `test/order-independent-products-20261109` | 8 |
| `test/order-ui-click-20261109` | 3 |
| `test/product-names-diamond-20261109` | 11 |
| `test/remove-legacy-brand-prefix-20261109` | 7 |
| `migration/01-recognition` | 102 |
| `migration/01-recognition-parity` | 64 |
| `migration/01-ts-parity` | 51 |
| `migration/02-rules` | 173 |
| `migration/03-integration` | 1 |
| `migration/08-runtime-closure-26` | 63 |
| `migration/26-manual-grid-native` | 19 |
| `migration/26-p1-p2-runtime-closure` | 36 |
| `migration/27-p1-p2-readside` | 28 |

## 작업자 안전 삭제 방법

1. GitHub 저장소 [Branches](https://github.com/ThunderStrikerKR/TSBH/branches)에서 위 **19개 삭제 후보만** 검색한다.
2. 이름·HEAD가 해당 브랜치와 일치하는지 확인하고 GitHub의 Delete branch(휴지통)를 선택한다.
3. `main`, `integration/final-rust-migration-20261116`, `backup/` 및 고유 커밋이 있는 21개를 삭제하지 않는다.
4. 삭제 후 GitHub Desktop에서 `Fetch origin`하고 `main`을 선택해 `Pull origin`한다.
5. 이후 새 기능은 `docs/development-branch-policy.md`에 따라 최신 `main`에서 새 브랜치로 작업한다.

**검증:** 삭제 후보 19개를 정리하면 원격 브랜치 49개에서 **30개**로 줄어든다.
추가 삭제는 고유 커밋 브랜치의 변경 비교 및 이관 여부 확인 뒤 별도로 수행한다.
