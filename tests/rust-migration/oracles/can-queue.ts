/** Test-only historical oracle, copied from src/domain.ts at
 * 7d6c06b86fcd02d8f97f756a24f01a42e85dbf13 before 94991564 removed
 * the unreferenced runtime TS queue decision. The legacy displayName type
 * is confined to this fixture. Never import into the operational app.
 */
import { totals } from "./legacy-domain.js";
import { reviewIssues } from "./legacy-hub-messages.js";
import type { Order, Client, Settings } from "../../../src/types.js";
export function canQueue(o: Order, c: Client & {displayName?: string}, s: Settings) {
  const issues = reviewIssues(o);
  if (o.status === "EXPORTED")
    issues.push("이미 공장에 전달한 주문입니다. 수정 / 재확인 후 추가하세요.");
  if (totals(o, c).total === null) issues.push("청구 금액 / 운임 미확정");
  if (!o.notified) issues.push("안내 완료 처리 필요");
  if (c.payment === "PREPAID" && !o.paid) issues.push("실제 입금 확인 필요");
  if (c.payment === "POSTPAID" && !o.postpaidApproved) issues.push("후불 출고 승인 필요");
  if (!o.recipients.some((r) => r.items.some((i) => i.origin === "FACTORY")))
    issues.push("공장 출고 상품 없음");
  if (!(c.displayName || c.name) || !c.address || (!c.phone && !c.mobile))
    issues.push("거래처 송장 상호 / 주소 / 전화번호 등록 필요");
  return issues;
}
