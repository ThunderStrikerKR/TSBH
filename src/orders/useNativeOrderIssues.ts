/** Authoritative compiled Rust orderIssues projection for staff review.
 * The legacy TS hubMessages/validation functions must not decide live
 * approval state. Discard any response for an older issue-input signature.
 * Unrelated order finance/metadata changes must not clear displayed issues.
 */
import { useEffect, useMemo, useState } from "react";
import { businessCoreCall } from "../tauri-bridge";
import { auditStateTransition } from "../perf-audit";
import { nativeOrderIssuesKey } from "./native-order-issues-key";
import type { Order, ValidationIssue } from "../types";

export function useNativeOrderIssues(draft: Order) {
  const fingerprint = useMemo(() => nativeOrderIssuesKey(draft), [draft]);
  const [resolved, setResolved] = useState<{
    fingerprint: string;
    issues: ValidationIssue[];
  }>();
  const [failure, setFailure] = useState<{
    fingerprint: string;
    reason: string;
  }>();
  useEffect(() => {
    let cancelled = false;
    auditStateTransition("useNativeOrderIssues","request");
    void businessCoreCall<{ issues: ValidationIssue[] }>(
      "rules", "order_issues_with_actions", { order: draft },
    ).then(({ issues }) => {
      if (!cancelled) {
        auditStateTransition("useNativeOrderIssues","resolved");
        setResolved({ fingerprint, issues });
        setFailure(undefined);
      }
    }).catch((error) => {
      if (!cancelled) {
        setResolved(undefined);
        setFailure({ fingerprint, reason: String(error) });
      }
    });
    return () => { cancelled = true; };
  }, [fingerprint]);
  const ready = resolved?.fingerprint === fingerprint;
  return {
    ready,
    issues: ready ? resolved!.issues : [],
    error: failure?.fingerprint === fingerprint ? failure.reason : "",
  };
}
