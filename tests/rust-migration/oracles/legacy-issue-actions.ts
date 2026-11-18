/** Historical issue-action oracles; production actions are Rust-owned. */
import { orderIssues } from "../../../src/validation.js";
import type { Order } from "../../../src/types.js";
export function approveMissingQuantity(o: Order, rid: string, iid: string, actor: string) {
  const next = structuredClone(o),
    i = next.recipients.find((r) => r.id === rid)?.items.find((i) => i.id === iid);
  if (
    !i ||
    !i.productId ||
    !i.variantId ||
    i.quantityConfirmed !== false ||
    i.match?.kind === "QUANTITY" ||
    i.issues.some((x) => /여러|불명확/.test(x))
  )
    throw Error("수량 미기재로 확인된 실제 상품만 1개 잠정 처리할 수 있습니다.");
  i.quantity = 1;
  i.quantityApproval = { at: new Date().toISOString(), actor, quantity: 1 };
  i.manuallyConfirmed = true;
  return next;
}
export function pendingIssue(o: Order, id: string, actor: string) {
  const next = structuredClone(o),
    issue = orderIssues(o).find((i) => i.issueId === id);
  if (!issue || issue.severity !== "ACTION") return next;
  const evidence = issue.evidence ?? "";
  next.issueResolutions = {
    ...next.issueResolutions,
    [id]: {
      resolution: "PENDING_CUSTOMER",
      resolutionSource: actor,
      updatedAt: new Date().toISOString(),
      evidence,
    },
  };
  return next;
}
