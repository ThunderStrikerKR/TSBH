/** Read-only order/shipment lookup used by React. No workflow state transition
 * or financial calculation is performed in this module.
 */
import type { Client, State } from "./types.js";
import type { WorkflowShipment } from "./workflow-types.js";

export function shipmentContext(state: State, s: WorkflowShipment, client: Client | null | undefined) {
  const order = state.orders.find((o) => o.id === s.orderId);
  const recipient = order?.recipients.find((r) => r.id === s.recipientId);
  if (!order || !recipient || !client)
    throw Error("배송 건의 발주서 / 수취인 / 거래처 연결을 확인하세요.");
  return { order, recipient, client };
}
export function groupShipments(state: State, groupId: string) {
  if (state.workflow?.groups.find((g) => g.id === groupId)?.deletedAt) return [];
  return (
    state.workflow?.shipments.filter(
      (s) =>
        s.groupId === groupId &&
        !s.legacyArchived &&
        !state.orders.find((o) => o.id === s.orderId)?.deletedAt,
    ) ?? []
  );
}
