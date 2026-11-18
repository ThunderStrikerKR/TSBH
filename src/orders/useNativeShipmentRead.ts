/** Shipment display lookup resolved by the compiled Rust workflow rules.
 * The same snapshot supplies the dealer name and saved freight for every
 * visible row; React only performs ID joins and form draft coordination.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { businessCoreCall } from "../tauri-bridge";
import type { Client, Origin, Price, State } from "../types";

type ShipmentReadRow = {
  shipmentId: string;
  client: Client | null;
  actualFreight: Record<Origin, Price>;
};
type ShipmentReadResponse = { shipments: ShipmentReadRow[] };

export function useNativeShipmentRead(state: State, enabled = true) {
  // Search text, checkbox selection, and local freight drafts do not restart
  // native reads. This signature tracks every input Rust uses for the view.
  const key = useMemo(() => enabled ? JSON.stringify({
    orders: state.orders.map((order) => ({
      id: order.id,
      clientId: order.clientId,
      orderType: order.orderType,
      confirmedAt: order.confirmedAt,
      pricesLocked: order.pricesLocked,
      clientSnapshot: order.clientSnapshot,
      recipients: order.recipients.map((row) => ({
        id: row.id, actualFreight: row.actualFreight,
      })),
    })),
    clients: state.clients,
    settings: {
      senderPostcode: state.settings.senderPostcode,
      senderAddress: state.settings.senderAddress,
      senderPhone: state.settings.senderPhone,
      senderMobile: state.settings.senderMobile,
    },
    shipments: state.workflow?.shipments.map((ship) => ({
      id: ship.id, orderId: ship.orderId,
      recipientId: ship.recipientId, actualFreight: ship.actualFreight,
    })),
  }) : "", [enabled, state.orders, state.clients, state.settings,
    state.workflow?.shipments]);
  const input = useRef(state);
  input.current = state;
  const [resolved, setResolved] = useState<{
    key: string; result: ShipmentReadResponse;
  }>();
  const [failure, setFailure] = useState<{ key: string; reason: string }>();
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void businessCoreCall<ShipmentReadResponse>("rules", "read_shipments", {
      state: input.current,
    }).then((result) => {
      if (cancelled) return;
      setResolved({ key, result });
      setFailure(undefined);
    }).catch((error) => {
      if (cancelled) return;
      setResolved(undefined);
      setFailure({ key, reason: String(error) });
    });
    return () => { cancelled = true; };
  }, [enabled, key]);
  const ready = enabled && resolved?.key === key;
  const shipments = useMemo(() => new Map<string, ShipmentReadRow>(
    ready ? resolved!.result.shipments.map((item) => [item.shipmentId, item] as const) : [],
  ), [ready, resolved]);
  return {
    ready,
    shipments,
    error: enabled && failure?.key === key ? failure.reason : "",
  };
}

export type NativeShipmentRead = ReturnType<typeof useNativeShipmentRead>;
