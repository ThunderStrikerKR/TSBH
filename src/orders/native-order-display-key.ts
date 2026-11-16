/**
 * UI freshness signature only. No amounts, tariffs or validation are computed.
 *
 * A cached Rust preview must never remain visible after ANY financial or
 * grouping input changed. Text-only metadata can update without flickering
 * a previously verified quote, while recipient data is kept complete because
 * it can change which Rust workflow shipment/group is projected.
 */
import type { Order, State } from "../types";

export function nativeOrderDisplayKey(sourceState: State, draft: Order): string {
  return JSON.stringify({
    persisted: {
      orders: sourceState.orders,
      workflow: sourceState.workflow,
      products: sourceState.products,
      shipping: sourceState.shipping,
      clients: sourceState.clients,
      settings: sourceState.settings,
    },
    draft: {
      id: draft.id,
      clientId: draft.clientId,
      orderType: draft.orderType,
      clientSnapshot: draft.clientSnapshot,
      senderSnapshot: draft.senderSnapshot,
      shippingFee: draft.shippingFee,
      shippingMode: draft.shippingMode,
      shippingVersion: draft.shippingVersion,
      freightCode: draft.freightCode,
      actualFreight: draft.actualFreight,
      applyVat: draft.applyVat,
      vatAmount: draft.vatAmount,
      pricesLocked: draft.pricesLocked,
      confirmedAt: draft.confirmedAt,
      deletedAt: draft.deletedAt,
      accountMode: draft.accountMode,
      accountId: draft.accountId,
      groupAccounts: draft.groupAccounts,
      paid: draft.paid,
      status: draft.status,
      recipients: draft.recipients,
    },
  });
}
