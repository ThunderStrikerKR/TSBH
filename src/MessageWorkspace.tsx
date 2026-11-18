import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { auditReactRender } from "./perf-audit";

import { nativeAutomaticMessage, nativeDecorateCopyText, type NativeAutomaticMessage } from "./native-automatic-message";
import { nativeRecordMessageCopy } from "./native-message-audit";
import { LinkedText, messageSpans } from "./LinkedText";
import { paymentMessageHighlights, selectedAccountFromSignature } from "./message-payment-highlights";
import { useReadyContentFade } from "./shared/useReadyContentFade";
import type { NoticeSink } from "./shared/notices";
import type { Order, State } from "./types";
export function MessageWorkspace({
  state,
  order,
  gid,
  projectionReady = true,
  projectionError = "",
  noMessageExpected = false,
  sharedFadePhase,
  fadeKey,
  onFadeReadiness,
  onOrder,
  onSave,
}: {
  state: State;
  order: Order;
  gid?: string;
  projectionReady?: boolean;
  projectionError?: string;
  noMessageExpected?: boolean;
  sharedFadePhase?: import("./shared/useReadyContentFade").ContentFadePhase;
  fadeKey?: string;
  onFadeReadiness?: (key: string, section: "message" | "checks" | "financial", ready: boolean) => void;
  onOrder: (o: Order) => void;
  onSave: (s: State) => Promise<void>;
  onSelect: (id: string) => void;
  onNotice: NoticeSink;
}) {
  auditReactRender("MessageWorkspace");
  const [busy, setBusy] = useState(false),
    [status, setStatus] = useState(""),
    [includeLinks, setIncludeLinks] = useState(false),
    [resolved, setResolved] = useState<{
      fingerprint: string;
      groupId: string;
      orderId: string;
      links: boolean;
      result: NativeAutomaticMessage;
    }>(),
    [loadError, setLoadError] = useState<{
      fingerprint: string; groupId: string; links: boolean; reason: string;
    }>(),
    latest = useRef(order),
    latestState = useRef(state);
  latest.current = order;
  latestState.current = state;
  // Editing a request draft does not change its financial / workflow rules.
  // A late native response never overwrites a newer state or user input.
  const fingerprint = useMemo(() => JSON.stringify({
    ...state,
    orders: state.orders.map(({ requestMessages: _draft, ...businessOrder }) => businessOrder),
  }), [state]);
  useEffect(() => {
    if (!gid || order.orderType === "RETAIL") {
      setResolved(undefined);
      setLoadError(undefined);
      return;
    }
    let cancelled = false;
    setLoadError(undefined);
    const requestedOrderId = order.id;
    void nativeAutomaticMessage(latestState.current, gid, includeLinks)
      .then((result) => {
        if (!cancelled) setResolved({ fingerprint, orderId: requestedOrderId, groupId: gid, links: includeLinks, result });
      })
      .catch((error) => {
        if (!cancelled) {
          setResolved(undefined);
          setLoadError({
            fingerprint, groupId: gid, links: includeLinks, reason: String(error),
          });
        }
      });
    return () => { cancelled = true; };
  }, [fingerprint, gid, includeLinks, order.orderType, order.id]);
  // Reset user options only on an actual order switch. During Rust
  // reconciliation gid can temporarily disappear and reappear; coupling
  // that transient value to checkbox state triggers duplicate recalculation.
  useLayoutEffect(() => {
    setStatus("");
    setIncludeLinks(false);
  }, [order.id]);
  const automatic = resolved?.fingerprint === fingerprint &&
      resolved?.orderId === order.id &&
      resolved?.groupId === gid && resolved?.links === includeLinks
      ? resolved.result
      : undefined,
    request = automatic?.phase !== "payment",
    baseText = automatic
      ? request
        ? (order.requestMessages?.[gid!] ?? automatic.text)
        : automatic.text
      : "",
    // Confirmation links already required by the request remain as-is.
    // The checkbox adds only other valid product links, without duplicates.
    optionalRequestLinks = includeLinks && request && automatic
      ? automatic.validRequestLinks
          .filter((entry) => !baseText.includes(entry.slice(entry.lastIndexOf("\n") + 1)))
          .join("\n\n")
      : "",
    optionalSuffix = optionalRequestLinks ? "\n\n" + optionalRequestLinks : "",
    text = baseText + optionalSuffix;
  // Keep the message title, links toggle, copy controls and status area
  // mounted. Only the Rust-generated body fades after the exact current
  // fingerprint/group/link-option response is available. Request text edits
  // do not invalidate this key or restart the animation.
  const contentKey = [order.id, gid ?? "", order.orderType ?? "", includeLinks].join(":");
  // A missing group is NOT a completed message. It is an intermediate
  // reconciliation state, so wait for the exact Rust message response.
  const bodyReady = Boolean(gid && automatic);
  const currentLoadError = loadError?.fingerprint === fingerprint &&
    loadError.groupId === gid && loadError.links === includeLinks ? loadError.reason : "";
  const messageSettled = projectionReady &&
    (bodyReady || noMessageExpected || Boolean(currentLoadError || projectionError));
  useEffect(() => {
    if (!fadeKey || !onFadeReadiness) return;
    onFadeReadiness(fadeKey, "message", messageSettled);
  }, [fadeKey, onFadeReadiness, messageSettled]);
  const localMessageFadePhase = useReadyContentFade(contentKey, bodyReady);
  const messageFadePhase = sharedFadePhase ?? localMessageFadePhase;
  const disabled = busy || !automatic || !text.trim() || (!request && !!automatic.paymentIssues.length);
  const emphasisSpans = request ? [] : paymentMessageHighlights(
    text, automatic ? selectedAccountFromSignature(automatic.signature) : null,
  );
  const paymentCopyReady = Boolean(automatic && !request && !disabled);
  const copy = async (greeting: boolean) => {
    if (disabled || !gid) return;
    setBusy(true);
    try {
      await window.bando!.copy(await nativeDecorateCopyText(text, greeting));
      if (latest.current.id !== order.id || latest.current.raw !== order.raw) return;
      if (order.confirmedAt)
        await onSave(await nativeRecordMessageCopy(state, gid, request ? "REQUEST" : "PAYMENT", crypto.randomUUID(), automatic?.signature));
      else if (request)
        onOrder({
          ...latest.current,
          requestCopiedGroups: [...new Set([...(latest.current.requestCopiedGroups ?? []), gid])],
        });
      setStatus("복사했습니다.");
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <aside className="desk-pane messages-pane"
      data-payment-copy-ready={paymentCopyReady ? "true" : undefined}>
      <header className="pane-heading">
        <h2>복사용 자동 메시지</h2>
        <label className="message-link-toggle">
          <input
            type="checkbox"
            checked={includeLinks}
            onChange={(event) => {
              setIncludeLinks(event.target.checked);
              setStatus("");
            }}
          />
          링크 포함
        </label>
      </header>
      <div className="stage-message-preview">
        <div className="message-generated-content" data-content-fade={messageFadePhase}
          aria-busy={messageFadePhase === "pending"} inert={messageFadePhase === "pending"}>
        <LinkedText
          key={order.id + ":" + (gid ?? "") + ":" + (request ? "request" : "payment")}
          readOnly={!request}
          directEdit={request}
          label="복사용 자동 메시지 본문"
          text={text}
          spans={messageSpans(text, order, request ? "request" : "payment")}
          emphasisSpans={emphasisSpans}
          onChange={(value) => {
            if (gid && request) {
              // Optional links belong to the copy preview, not the editable request draft.
              const edited = optionalSuffix && value.endsWith(optionalSuffix)
                ? value.slice(0, -optionalSuffix.length)
                : value;
              onOrder({ ...order, requestMessages: { ...order.requestMessages, [gid]: edited } });
              setStatus("");
            }
          }}
        />
        </div>
      </div>
      <div className="message-inline-status" role="status">
        {projectionError || currentLoadError || status}
      </div>
      <footer className="message-copy-actions">
        <button disabled={disabled} onClick={() => void copy(false)}>
          그대로 복사
        </button>
        <button disabled={disabled} onClick={() => void copy(true)}>
          인사말 포함 복사
        </button>
      </footer>
    </aside>
  );
}

/** Shallow props equality is safe because HubWorkspace passes stable event wrappers. */
export const MemoMessageWorkspace = memo(MessageWorkspace);
