/** Rust is the sole authority for the effective order party and the current
 * pricing client. A resolved result is only exposed for the exact selection
 * revision that produced it; stale IPC replies never reassign the dealer.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { businessCoreCall } from "../tauri-bridge";
import type { Client, Order, State } from "../types";

type ClientProjection = { client: Client | null; liveClient: Client | null };

export function useNativeOrderClients(order: Order, state: State) {
  // Text edits and shipment progress are not inputs to client selection. In
  // particular, a new raw keystroke must not restart dealer resolution.
  const key = useMemo(() => JSON.stringify({
    id: order.id,
    orderType: order.orderType,
    clientId: order.clientId,
    confirmedAt: order.confirmedAt,
    pricesLocked: order.pricesLocked,
    clientSnapshot: order.clientSnapshot,
    clients: state.clients,
    settings: order.orderType === "RETAIL" ? state.settings : undefined,
  }), [order.id, order.orderType, order.clientId, order.confirmedAt,
      order.pricesLocked, order.clientSnapshot, state.clients, state.settings]);
  const currentInput = useRef({ order, state });
  currentInput.current = { order, state };
  const [resolved, setResolved] = useState<{
    key: string; value: ClientProjection;
  }>();
  const [failure, setFailure] = useState<{ key: string; reason: string }>();
  useEffect(() => {
    let cancelled = false;
    const { order: sourceOrder, state: sourceState } = currentInput.current;
    void businessCoreCall<ClientProjection>("rules", "order_edit_rules", {
      action: "resolve_order_clients", order: sourceOrder, state: sourceState,
    }).then((value) => {
      if (cancelled) return;
      setResolved({ key, value });
      setFailure(undefined);
    }).catch((error) => {
      if (cancelled) return;
      setResolved(undefined);
      setFailure({ key, reason: String(error) });
    });
    return () => { cancelled = true; };
  }, [key]);
  const ready = resolved?.key === key;
  return {
    ready,
    client: ready ? resolved.value.client ?? undefined : undefined,
    liveClient: ready ? resolved.value.liveClient ?? undefined : undefined,
    error: failure?.key === key ? failure.reason : "",
  };
}
