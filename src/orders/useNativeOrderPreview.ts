/** Rust result for the order-detail financial footer.
 * A late IPC reply is discarded when the order, state, or draft changes.
 * No TypeScript business calculation runs as a fallback.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { businessCoreCall } from "../tauri-bridge";
import type { Order, Price, State } from "../types";

export type NativeOrderPreview = {
  orderId: string;
  shipmentCount: number;
  lineAmounts: Array<{ recipientId: string; itemId: string; amount: Price }>;
  total: Price;
  vatLabel: string;
};

export function useNativeOrderPreview(
  state: State,
  draft: Order,
  enabled = true,
  sourceState: State = state,
) {
  const fingerprint = useMemo(
    () => JSON.stringify({ state, draft }),
    [state, draft],
  );
  // Preserve a *Rust-produced* summary only while its financial inputs are
  // unchanged. Unlike a product-line price, VAT and freight totals cannot be
  // recomputed from the order object without duplicating Rust rules in React.
  const displayKey = useMemo(() => JSON.stringify({
    persisted: {
      orders: sourceState.orders,
      workflow: sourceState.workflow,
      products: sourceState.products,
      shipping: sourceState.shipping,
      clients: sourceState.clients,
      settings: sourceState.settings,
    },
    orderId: draft.id,
    clientId: draft.clientId,
    orderType: draft.orderType,
    applyVat: draft.applyVat,
    recipients: draft.recipients.map(r => ({
      id: r.id,
      vatOverride: r.vatOverride,
      freightCode: r.freightCode,
      dispatchOrigin: r.dispatchOrigin,
      shippingCharges: r.shippingCharges,
      items: r.items.map(i => ({
        id: i.id, productId: i.productId, variantId: i.variantId,
        unitPrice: i.unitPrice, quantity: i.quantity, origin: i.origin,
      })),
    })),
  }), [sourceState, draft]);
  const current = useRef({ state, draft });
  current.current = { state, draft };
  const [resolved, setResolved] = useState<{
    fingerprint: string;
    displayKey: string;
    result: NativeOrderPreview;
  }>();
  const [failure, setFailure] = useState<{
    fingerprint: string;
    reason: string;
  }>();
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const payload = current.current;
    void businessCoreCall<NativeOrderPreview>(
      "rules", "preview_order", {
        state: payload.state,
        draft: payload.draft,
        orderId: payload.draft.id,
      },
    ).then((result) => {
      if (!cancelled) {
        setResolved({ fingerprint, displayKey, result });
        setFailure(undefined);
      }
    }).catch((error) => {
      if (!cancelled) {
        setResolved(undefined);
        setFailure({ fingerprint, reason: String(error) });
      }
    });
    return () => { cancelled = true; };
  }, [enabled, fingerprint, displayKey]);

  const ready = enabled && resolved?.fingerprint === fingerprint;
  return {
    ready,
    result: ready ? resolved!.result : undefined,
    // Display-only: keep the last confirmed native value through unrelated
    // workflow/issue loads. Any financial input change invalidates it.
    displayResult: resolved?.displayKey === displayKey &&
      resolved.result.orderId === draft.id ? resolved.result : undefined,
    error: enabled && failure?.fingerprint === fingerprint
      ? failure.reason
      : "",
  };
}
