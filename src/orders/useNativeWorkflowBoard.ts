/** Compiled Rust owns live workflow-board money, VAT and approval projections.
 * Only the React view model is cached here. Stale IPC responses are ignored,
 * and no old TypeScript calculator runs as a fallback.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { businessCoreCall } from "../tauri-bridge";
import { auditStateTransition } from "../perf-audit";
import type { Origin, Price, State, ValidationIssue } from "../types";

export type NativeShipmentPreview = {
  shipmentId: string;
  paid: number;
  displayCharges: Partial<Record<Origin, Price>>;
  freightEditingBlocked: boolean;
  totals: { subtotal: Price; shipping: Price; vat: Price; total: Price };
  freightCode: string;
  vatIncluded: boolean;
};
export type NativeGroupPreview = {
  groupId: string;
  step: number;
  nextStep: number;
  needsNotice: boolean;
  /** Signed difference to the most recently issued notice; null if unavailable. */
  noticeDelta: Price;
  transferred: boolean;
  shipmentCount: number;
  totals: { subtotal: Price; shipping: Price; vat: Price; total: Price };
  vatLabel: string;
  shipments: NativeShipmentPreview[];
  issues: ValidationIssue[];
  nextStageBlockers: string[];
};
type NativeProjection = { groups: NativeGroupPreview[] };

export function useNativeWorkflowBoard(state: State, enabled = true) {
  // A semantic key prevents unnecessary IPC when the caller recreates an
  // equivalent JSON object; its value changes for every meaningful edit.
  const fingerprint = useMemo(
    () => enabled ? JSON.stringify(state) : "",
    [state, enabled],
  );
  const latest = useRef(state);
  latest.current = state;
  const [resolved, setResolved] = useState<{
    fingerprint: string; result: NativeProjection;
  }>();
  const [failure, setFailure] = useState<{ fingerprint: string; reason: string }>();
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    auditStateTransition("useNativeWorkflowBoard","request");
    void businessCoreCall<NativeProjection>("rules", "preview_workflow", {
      state: latest.current,
    }).then((result) => {
      if (!cancelled) {
        auditStateTransition("useNativeWorkflowBoard","resolved");
        setResolved({ fingerprint, result });
        setFailure(undefined);
      }
    }).catch((error) => {
      if (!cancelled) {
        setResolved(undefined);
        setFailure({ fingerprint, reason: String(error) });
      }
    });
    return () => { cancelled = true; };
  }, [fingerprint, enabled]);
  const ready = enabled && resolved?.fingerprint === fingerprint;
  const byId = useMemo(() => {
    // A stale projection is for display only while Rust refreshes. Consumers
    // must check ready before actions; retaining it keeps card heights stable.
    if (!resolved) return {} as Record<string, NativeGroupPreview>;
    return Object.fromEntries(resolved.result.groups.map((group) => [group.groupId, group]));
  }, [resolved]);
  return {
    ready,
    byId,
    pending: enabled && !ready && failure?.fingerprint !== fingerprint,
    error: failure?.fingerprint === fingerprint ? failure.reason : "",
  };
}
