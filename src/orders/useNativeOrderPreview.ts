/** Rust result for the order-detail financial footer.
 * A late IPC reply is discarded when the order, state, or draft changes.
 * No TypeScript business calculation runs as a fallback.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { businessCoreCall } from "../tauri-bridge";
import { auditStateTransition } from "../perf-audit";
import { nativeOrderDisplayKey } from "./native-order-display-key";
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
  // Retain the last Rust result only for truly identical financial inputs.
  // This signature does not calculate money or replace the Rust authority.
  const displayKey = useMemo(
    () => nativeOrderDisplayKey(sourceState, draft),
    [sourceState, draft],
  );
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
    auditStateTransition("useNativeOrderPreview","request");
    void businessCoreCall<NativeOrderPreview>(
      "rules", "preview_order", {
        state: payload.state,
        draft: payload.draft,
        orderId: payload.draft.id,
      },
    ).then((result) => {
      if (!cancelled) {
        auditStateTransition("useNativeOrderPreview","resolved");
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
