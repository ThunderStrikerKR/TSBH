import { lazy, memo, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentProps } from "react";
import { useEventCallback } from "./shared/useEventCallback";
import { useReadyContentFade, type ContentFadePhase } from "./shared/useReadyContentFade";
import { auditReactRender, auditStateTransition } from "./perf-audit";
import { AuditProfiler } from "./perf-audit-profiler";
import { createPortal, flushSync } from "react-dom";
import { HubPanels, defaultPanels } from "./HubPanels";
import { WorkingReceptionAutosave } from "./orders/working-reception-autosave";
import { sameValue } from "./shared/draft";
import { requestPageLeave } from "./shared/page-leave";
import { LinkContext } from "./LinkedText";
import { MemoMessageWorkspace as MessageWorkspace } from "./MessageWorkspace";
import { OrderDesk, MemoOrderDesk } from "./OrderDesk";
import { MemoWorkflowBoard as WorkflowBoard, type WorkflowSelection } from "./WorkflowBoard";
import { newReception } from "./orders/new-reception";
import { nativeReconcileWorkflow } from "./orders/native-rules";
import { useNativeWorkflowReconcile } from "./orders/useNativeWorkflowReconcile";
import { retainUnsavedOrderEdits } from "./order-edit-snapshots";
import type { Order } from "./types";
import { groupShipments } from "./workflow-view";
import { activeShipments, hoveredWorkflowSelection, validSelection } from "./workspace-selection";
type Props = ComponentProps<typeof OrderDesk> & {
  externalKakao: boolean;
  onNew: () => void;
  onRememberSelection: (s: WorkflowSelection) => void;
  onResetDock: () => void;
  onRegisterOrderLeaveGuard?: (guard?: (intent?: "switch" | "navigate" | "close") => Promise<{ proceed: boolean; handledDraft: boolean }>) => void;
};
const ManualClassification = lazy(() =>
  import("./orders/ManualClassification").then((module) => ({
    default: module.ManualClassification,
  })),
);
export function HubWorkspace(props: Props) {
  auditReactRender("HubWorkspace");
  const [manual, setManual] = useState(false);
  const [discardRevision, setDiscardRevision] = useState(0);
  const deletedSelection = useRef<WorkflowSelection | undefined>(undefined);
  const leaveGuardRef = useRef<(intent?: "switch" | "navigate" | "close") => Promise<{ proceed: boolean; handledDraft: boolean }>>(
    async () => ({ proceed: true, handledDraft: false }),
  );
  const { state, draft } = props;
  const [receptionId, setReceptionId] = useState(draft.id);
  // Startup recovery still restores every working copy to the board, but
  // must never take focus back after an explicit New Order click.
  const explicitNewReception = useRef(false);
  // A new draft must replace the old order before the next screen paints.
  // Never let a late Rust preview or remembered workflow selection replace
  // the newly blank reception with the first persisted shipment.
  useLayoutEffect(() => {
    setReceptionId(draft.id);
    setStageHover(undefined);
    setActive("");
    if (!state.orders.some((order) => order.id === draft.id)) {
      commitSelection(undefined);
      lastVisiblePreview.current = undefined;
    }
  }, [draft.id]);
  // Classified-field hover uses a short entrance delay and an exit grace
  // period. Focus and explicit commands must always react immediately.
  const linkHoverOnTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const linkHoverOffTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => {
    if (linkHoverOnTimer.current) clearTimeout(linkHoverOnTimer.current);
    if (linkHoverOffTimer.current) clearTimeout(linkHoverOffTimer.current);
    linkHoverOnTimer.current = undefined;
    linkHoverOffTimer.current = undefined;
  }, [draft.id]);
  const [workingStoreReady, setWorkingStoreReady] = useState(false);
  const currentDraftRef = useRef(draft);
  currentDraftRef.current = draft;
  const workingWriter = useRef<WorkingReceptionAutosave | null>(null);
  if (!workingWriter.current) {
    workingWriter.current = new WorkingReceptionAutosave(
      (orders) => window.bando!.workingReceptionsReplace(orders),
    );
  }
  const [source, setSource] = useState<HTMLElement | null>(null),
    [messages, setMessages] = useState<HTMLElement | null>(null),
    [details, setDetails] = useState<HTMLElement | null>(null),
    [selected, commitSelection] = useState<WorkflowSelection | undefined>(props.workflowSelection),
    [edits, setEdits] = useState<Record<string, Order>>({}),
    [active, setActive] = useState(""),
    [stageHover, setStageHover] = useState<{ groupId: string; step: number; shipmentId?: string }>();
  useEffect(() => { auditStateTransition("HubWorkspace","portal-ready",{source:!!source,messages:!!messages,details:!!details}); },[source,messages,details]);
  const applySelection = (next: WorkflowSelection | undefined) => {
    if (next) {
      setReceptionId(next.orderId);
      if (
        next.orderId !== draft.id &&
        receptionId === draft.id &&
        (draft.raw.trim() || draft.photos?.length) &&
        !state.orders.some((o) => o.id === draft.id)
      )
        setEdits((old) => ({ ...old, [draft.id]: structuredClone(draft) }));
    }
    if (
      next?.shipmentId === selected?.shipmentId &&
      next?.orderId === selected?.orderId &&
      next?.groupId === selected?.groupId
    )
      return;
    // After the user has chosen what happens to edits, change data ownership immediately.
    flushSync(() => commitSelection(next));
  };
  const setSelected = (
    value:
      | WorkflowSelection
      | undefined
      | ((previous: WorkflowSelection | undefined) => WorkflowSelection | undefined),
  ) => {
    const next = typeof value === "function" ? value(selected) : value;
    if (next?.shipmentId === selected?.shipmentId && next?.orderId === selected?.orderId && next?.groupId === selected?.groupId) return;
    void (async () => {
      // In-place shipment changes can unmount an actual-freight editor too.
      const fields = await requestPageLeave(
        (message) => props.onNotice(message, "error"), { autosave: true },
      );
      if (!fields.proceed) return;
      if (selected?.orderId && next?.orderId && selected.orderId !== next.orderId) {
        const leave = await leaveGuardRef.current();
        if (!leave.proceed) return;
      }
      applySelection(next);
    })().catch((error) => props.onNotice(String(error), "error"));
  };
  useEffect(() => {
    if (!selected?.shipmentId) return;
    const timer = setTimeout(
      () =>
        document
          .querySelector('[data-shipment-id="' + CSS.escape(selected.shipmentId!) + '"]')
          ?.scrollIntoView({ block: "nearest", inline: "nearest" }),
      20,
    );
    return () => clearTimeout(timer);
  }, [selected?.shipmentId]);
  // The old synchronous TypeScript reconciliation must not be the live
  // workflow board's authority. Rust evaluates each employee draft overlay,
  // returning only temporary preview state with no SQLite writes.
  const independentEdits = useMemo(
    () => Object.values(edits).filter((order) => order.id !== draft.id && order.clientId),
    [edits, draft.id],
  );
  // All unconfirmed receptions are persisted to SQLite immediately, and
  // continue to be visible in 처리 중인 주문 before the business confirmation.
  const temporaryReceipts = useMemo(() => {
    const byId = new Map<string, Order>();
    const unsaved = (order: Order) =>
      !order.confirmedAt &&
      !state.orders.some((saved) => saved.id === order.id) &&
      Boolean(order.raw.trim() || order.clientId || order.recipients.length || order.photos?.length);
    for (const order of Object.values(edits))
      if (unsaved(order)) byId.set(order.id, order);
    if (unsaved(draft)) byId.set(draft.id, draft);
    return [...byId.values()];
  }, [draft, edits, state.orders]);
  // For confirmed orders, save ONLY a working edit. Never overwrite the
  // frozen confirmed order or its billing snapshot on ordinary navigation.
  const confirmedWorkingEdits = useMemo(() => {
    const saved = new Map(state.orders.map((order) => [order.id, order]));
    const drafts = new Map<string, Order>();
    for (const edit of Object.values(edits)) {
      const original = saved.get(edit.id);
      if (original && !sameValue(edit, original))
        drafts.set(edit.id, edit);
    }
    const currentSaved = saved.get(draft.id);
    if (currentSaved && !sameValue(draft, currentSaved))
      drafts.set(draft.id, draft);
    return [...drafts.values()];
  }, [draft, edits, state.orders]);
  const workingCopies = useMemo(
    () => [...temporaryReceipts, ...confirmedWorkingEdits],
    [temporaryReceipts, confirmedWorkingEdits],
  );
  // Restore persisted receptions before starting autosave: writing an empty
  // list during startup would otherwise erase the previous session's orders.
  useEffect(() => {
    let cancelled = false;
    void window.bando!.workingReceptionsLoad().then((stored) => {
      if (cancelled) return;
      const recovered = stored.filter((order) => {
        const saved = state.orders.find((row) => row.id === order.id);
        return saved
          ? Boolean(order.confirmedAt) && !sameValue(order, saved)
          : !order.confirmedAt;
      });
      setEdits((previous) => ({
        ...Object.fromEntries(recovered.map((order) => [order.id, order])),
        ...previous,
      }));
      const current = currentDraftRef.current;
      const currentlySaved = state.orders.find((row) => row.id === current.id);
      const savedEdit = recovered.find((row) => row.id === current.id && row.confirmedAt);
      if (currentlySaved && savedEdit &&
          sameValue(current, currentlySaved)) {
        props.onChange(structuredClone(savedEdit));
      } else if (!current.raw.trim() && !current.clientId &&
          !current.recipients.length && !current.photos?.length) {
        const pending = recovered.find((row) => !row.confirmedAt);
        if (pending && !explicitNewReception.current) setReceptionId(pending.id);
      }
      setWorkingStoreReady(true);
    }).catch((error) =>
      props.onNotice("미확정 발주서 SQLite 복원 실패: " + String(error), "error"));
    return () => { cancelled = true; };
  }, []);
  // Every working copy is written to the independent SQLite table, without
  // advancing any workflow step or entering the confirmed order-history ledger.
  // Serialized/coalesced writes ensure the last edit wins under rapid typing.
  useEffect(() => {
    if (!workingStoreReady) return;
    void workingWriter.current!.enqueue(workingCopies).catch((error) =>
      props.onNotice("발주서 자동 저장 실패: " + String(error), "error"));
  }, [workingCopies, workingStoreReady, props.onNotice]);
  // An explicitly selected unassigned receipt must never fall back to a
  // shipment that belongs to an unrelated dealer.
  const pendingWithoutClient = useMemo(() =>
    temporaryReceipts.filter((order) => !order.clientId),
    [temporaryReceipts],
  );
  const nativeBase = useNativeWorkflowReconcile(state, independentEdits);
  const base = nativeBase.state;
  const nativePreview = useNativeWorkflowReconcile(base, draft.clientId ? [draft] : []);
  // The board intentionally excludes dealer-unassigned drafts, but both live
  // OrderDesk panes still need the same Rust draft projection. Compute it once
  // without publishing it to the workflow board or persisted SQLite state.
  const unassignedDraftPreview = useNativeWorkflowReconcile(
    base,
    !draft.clientId && receptionId === draft.id ? [draft] : [],
  );
  const preview = nativePreview.state;
  const previewReady = nativeBase.ready && nativePreview.ready;
  // Share one live Rust projection only for exact draft and state identity.
  // Independent persisted/detail orders still own their own reconciliation.
  const sharedCurrentDraftFlow = useMemo(() => {
    const projected = draft.clientId ? nativePreview :
      receptionId === draft.id ? unassignedDraftPreview : undefined;
    return projected ? { input: base, draft, value: {
      state: projected.state, ready: projected.ready, error: projected.error,
    } } : undefined;
  }, [
    base, draft, receptionId, nativePreview.state, nativePreview.ready,
    nativePreview.error, unassignedDraftPreview.state,
    unassignedDraftPreview.ready, unassignedDraftPreview.error,
  ]);
  // Keep the last settled Rust board on screen while the newest draft is
  // reconciled. The board is read-only until the newest result is ready.
  // This prevents the queue cards from disappearing/reappearing on paste.
  const lastVisiblePreview = useRef<{ orderId: string; state: typeof preview } | undefined>(undefined);
  if (previewReady) lastVisiblePreview.current = { orderId: draft.id, state: preview };
  const boardPreview = !previewReady && lastVisiblePreview.current
    ? lastVisiblePreview.current.state : preview;
  useEffect(() => {
    // Selection and active shipment identity must never be rewritten from a
    // pending, non-authoritative render of the previous draft.
    if (!previewReady) return;
    // Clicking a "거래처 미선택" card is an explicit choice of a reception
    // with NO shipment. The generic validSelection() fallback to shipments[0]
    // must never overwrite that receipt with an unrelated dealer/order.
    if (pendingWithoutClient.some((order) => order.id === receptionId)) {
      if (selected) commitSelection(undefined);
      return;
    }
    const fresh = receptionId === draft.id && !state.orders.some((o) => o.id === draft.id);
    // When a previously unassigned, locally retained receipt gets a dealer,
    // prefer that receipt's new Rust shipment rather than an old first card.
    const selectedReceptionShipment = receptionId !== draft.id && edits[receptionId]?.clientId
      ? activeShipments(preview).find((shipment) => shipment.orderId === receptionId)
      : undefined;
    const preferredSelection = selected ??
      (selectedReceptionShipment ? {
        orderId: selectedReceptionShipment.orderId,
        groupId: selectedReceptionShipment.groupId,
        shipmentId: selectedReceptionShipment.id,
      } : undefined);
    const next = fresh
      ? draft.recipients.length
        ? validSelection(
            preview,
            selected?.orderId === draft.id
              ? selected
              : { orderId: draft.id, groupId: "group:" + draft.id },
          )
        : undefined
      : validSelection(preview, preferredSelection);
    if (JSON.stringify(next) !== JSON.stringify(selected)) {
      commitSelection(next);
      if (next) setReceptionId(next.orderId);
    }
  }, [selected, preview, receptionId, draft, edits, pendingWithoutClient, previewReady]);
  useEffect(() => {
    if (selected) props.onRememberSelection(selected);
  }, [selected, props.onRememberSelection]);
  // A stage-button hover previews its OWN shipment, never the previously
  // selected order. This is read-only navigation: persistent selection and
  // unsaved edit ownership stay untouched until the card is clicked.
  const hoverSelection = previewReady
    ? hoveredWorkflowSelection(preview, stageHover?.groupId, selected, stageHover?.shipmentId)
    : undefined;
  // A linked token only changes the visible preview, not committed selection.
  const linkedHoverShipment = active && previewReady
    ? activeShipments(preview).find((ship) =>
        preview.orders.find((order) => order.id === ship.orderId)
          ?.recipients.some((recipient) =>
            recipient.id === ship.recipientId &&
            (active.startsWith(recipient.id + ":") ||
              recipient.items.some((item) => active.startsWith(item.id + ":"))),
          ),
      )
    : undefined;
  const linkedHoverSelection = linkedHoverShipment ? {
    orderId: linkedHoverShipment.orderId,
    groupId: linkedHoverShipment.groupId,
    shipmentId: linkedHoverShipment.id,
  } : undefined;
  const visibleSelection = hoverSelection ?? linkedHoverSelection ?? selected;
  const detail = visibleSelection
    ? visibleSelection.orderId === draft.id
      ? draft
      : (edits[visibleSelection.orderId] ??
        state.orders.find((o) => o.id === visibleSelection.orderId && !o.deletedAt))
    : undefined;
  const onSave = async (next: typeof state) => {
    const removed =
      next.workflow?.groups.filter(
        (g) => g.deletedAt && !state.workflow?.groups.find((old) => old.id === g.id)?.deletedAt,
      ) ?? [];
    const restored =
      next.workflow?.groups.filter(
        (g) => !g.deletedAt && state.workflow?.groups.find((old) => old.id === g.id)?.deletedAt,
      ) ?? [];
    const removedCurrent = removed.some((g) => g.id === selected?.groupId);
    if (removedCurrent) deletedSelection.current = selected;
    await props.onWorkflowSave(next);
    // A workflow save for another order must not discard this order's unsaved edits.
    setEdits((previous) => {
      // A draft just confirmed by the Rust workflow is now authoritative
      // in SQLite. Its earlier, unconfirmed local snapshot must not reappear
      // as a stale overlay after saving.
      const newlyConfirmed = new Set(next.orders
        .filter((row) => row.confirmedAt &&
          !state.orders.some((saved) => saved.id === row.id))
        .map((row) => row.id));
      const remaining = Object.fromEntries(Object.entries(previous)
        .filter(([id]) => !newlyConfirmed.has(id)));
      return retainUnsavedOrderEdits(remaining, next.orders);
    });
    if (removed.length || restored.length) {
      props.onNotice("");
      const target = validSelection(
        next,
        restored.some((g) => g.id === deletedSelection.current?.groupId)
          ? deletedSelection.current
          : removedCurrent
            ? undefined
            : selected,
      );
      commitSelection(target);
      setActive("");
      setStageHover(undefined);
      if (target) {
        setReceptionId(target.orderId);
        const order = next.orders.find((o) => o.id === target.orderId);
        if (order) props.onChange(structuredClone(order));
      } else {
        const empty = newReception();
        setReceptionId(empty.id);
        props.onChange(empty);
      }
    }
  };
  leaveGuardRef.current = async (intent = "switch") => {
    // Tab changes, order switches and app shutdown all flush SQLite edits.
    // Neither confirmed business invoices nor the original order ledger are
    // modified by this automatic working-copy save.
    if (workingCopies.length && !workingStoreReady) {
      props.onNotice("발주서 SQLite 자동 저장 준비가 끝나지 않았습니다.", "error");
      return { proceed: false, handledDraft: false };
    }
    try {
      if (workingStoreReady) await workingWriter.current!.enqueue(workingCopies);
    } catch (error) {
      props.onNotice("발주서 자동 저장 실패: " + String(error), "error");
      return { proceed: false, handledDraft: false };
    }
    // Preserve the current source in the local overlay on order switching;
    // the SQLite copy has already been written above.
    if (intent === "switch" && workingCopies.some((row) => row.id === draft.id)) {
      setEdits((previous) => ({ ...previous, [draft.id]: structuredClone(draft) }));
    }
    // There is no separate confirmation for unconfirmed or confirmed edits.
    return { proceed: true, handledDraft: true };
  };
  useEffect(() => {
    props.onRegisterOrderLeaveGuard?.((intent) => leaveGuardRef.current(intent));
    return () => props.onRegisterOrderLeaveGuard?.(undefined);
  }, [props.onRegisterOrderLeaveGuard]);
  const changeDetail: Props["onChange"] = (value) => {
    const current = detail ?? draft;
    if (
      state.orders.some((o) => o.id === current.id) &&
      !activeShipments(preview).some((s) => s.orderId === current.id)
    )
      return;
    const next = typeof value === "function" ? value(current) : value;
    if (JSON.stringify(next) === JSON.stringify(current)) return;
    if (current.id === draft.id) props.onChange(next);
    else setEdits((previous) => ({ ...previous, [next.id]: next }));
  };
  const [vertical, setVertical] = useState<number>(),
    verticalDrag = useRef<{ y: number; height: number; start: number } | undefined>(undefined);
  useEffect(() => setVertical(undefined), [state.settings.receptionVerticalRatio]);
  const verticalRatio = vertical ?? state.settings.receptionVerticalRatio ?? 0.5;
  const linkedRecipient = preview.orders
    .flatMap((o) => o.recipients.map((r) => ({ order: o, recipient: r })))
    .find(
      ({ recipient: r }) =>
        active.startsWith(r.id + ":") || r.items.some((i) => active.startsWith(i.id + ":")),
    );
  const setSelectedLive = useEventCallback(setSelected);
  const setActiveLive = useEventCallback((key: string) => {
    // Focus, click-driven evidence navigation and explicit clear are immediate.
    if (linkHoverOnTimer.current) clearTimeout(linkHoverOnTimer.current);
    if (linkHoverOffTimer.current) clearTimeout(linkHoverOffTimer.current);
    linkHoverOnTimer.current = undefined;
    linkHoverOffTimer.current = undefined;
    setActive(key);
  });
  const beginLinkHoverLive = useEventCallback((key: string) => {
    if (linkHoverOnTimer.current) clearTimeout(linkHoverOnTimer.current);
    if (linkHoverOffTimer.current) clearTimeout(linkHoverOffTimer.current);
    linkHoverOffTimer.current = undefined;
    linkHoverOnTimer.current = setTimeout(() => {
      linkHoverOnTimer.current = undefined;
      setActive(key);
    }, 200);
  });
  const clearLinkHoverLive = useEventCallback(() => {
    // Ignore fast pointer passes before the entry timer can activate.
    if (linkHoverOnTimer.current) clearTimeout(linkHoverOnTimer.current);
    if (linkHoverOffTimer.current) clearTimeout(linkHoverOffTimer.current);
    linkHoverOnTimer.current = undefined;
    linkHoverOffTimer.current = setTimeout(() => {
      linkHoverOffTimer.current = undefined;
      setActive("");
    }, 250);
  });
  const linkController = useMemo(() => ({
    active,
    recipientId: linkedRecipient?.recipient.id,
    setActive: setActiveLive,
    beginHover: beginLinkHoverLive,
    clearHover: clearLinkHoverLive,
  }), [active, linkedRecipient?.recipient.id, setActiveLive, beginLinkHoverLive, clearLinkHoverLive]);
  const visibleReceptionId = hoverSelection?.orderId ?? linkedHoverSelection?.orderId ?? receptionId;
  const receptionOrder =
    visibleReceptionId === draft.id
      ? draft
      : (edits[visibleReceptionId] ??
        state.orders.find((o) => o.id === visibleReceptionId && !o.deletedAt) ??
        draft);
  useEffect(() => setManual(false), [receptionOrder.id, receptionOrder.raw]);
  const changeReception: Props["onChange"] = (value) => {
    if (
      state.orders.some((o) => o.id === receptionOrder.id) &&
      !activeShipments(preview).some((s) => s.orderId === receptionOrder.id)
    )
      return;
    if (receptionOrder.id === draft.id) props.onChange(value);
    else {
      const next = typeof value === "function" ? value(receptionOrder) : value;
      setEdits((old) => ({ ...old, [next.id]: next }));
    }
  };
  const messageOrder = receptionOrder;
  const messageOverlay = useMemo(
    () => messageOrder.id !== draft.id && (messageOrder.clientId || messageOrder.orderType === "RETAIL")
      ? [messageOrder] : [],
    [messageOrder, draft.id],
  );
  const nativeMessage = useNativeWorkflowReconcile(base, messageOverlay);
  // Reuse the already reconciled draft preview instead of dispatching a
  // second identical Rust IPC for the currently edited reception.
  const messageState = messageOrder.id === draft.id ? preview : nativeMessage.state;
  const messageReady = messageOrder.id === draft.id
    ? previewReady : nativeBase.ready && nativeMessage.ready;
  const messageGroup =
      visibleSelection?.orderId === messageOrder.id &&
      visibleSelection?.groupId &&
      groupShipments(messageState, visibleSelection.groupId).length > 0
        ? visibleSelection.groupId
        : messageState.workflow?.groups.find((g) =>
            groupShipments(messageState, g.id).some((s) => s.orderId === messageOrder.id),
          )?.id;
  // All five changing content areas share ONE transition. The source
  // projection is known here; OrderDesk reports checks and financial data,
  // and MessageWorkspace reports the matching Rust-generated message.
  // No section may fade in before all have settled for this exact view.
  const fadeKey = [
    receptionOrder.id,
    detail?.id ?? "",
    visibleSelection?.shipmentId ?? "",
  ].join("|");
  type AsyncSection = "message" | "checks" | "financial";
  const [fadeReadiness, setFadeReadiness] = useState<{
    key: string;
    message?: boolean;
    checks?: boolean;
    financial?: boolean;
  }>({ key: "" });
  const currentFadeKey = useRef(fadeKey);
  currentFadeKey.current = fadeKey;
  const reportFadeReadiness = useCallback((key: string, section: AsyncSection, ready: boolean) => {
    // Effects from an old order must not reopen or reset the new order.
    if (currentFadeKey.current !== key) return;
    setFadeReadiness((previous) => {
      const current = previous.key === key ? previous : {
        key, message: undefined, checks: undefined, financial: undefined,
      };
      if (current[section] === ready) return current;
      return { ...current, [section]: ready };
    });
  }, []);
  const sourceSettled = receptionOrder.id === draft.id
    ? (nativeBase.ready && (draft.clientId ? nativePreview.ready : unassignedDraftPreview.ready)) ||
      Boolean(nativeBase.error || (draft.clientId ? nativePreview.error : unassignedDraftPreview.error))
    : nativeBase.ready || Boolean(nativeBase.error);
  const messageProjectionSettled = messageReady || Boolean(
    nativeBase.error || (messageOrder.id === draft.id ? nativePreview.error : nativeMessage.error),
  );
  // Retail orders do not use the dealer automatic message workflow.
  // Treat this as an intentional empty message, not an endless pending state.
  const noMessageExpected = messageReady && (
    messageOrder.orderType === "RETAIL" ||
    (!messageGroup &&
      !activeShipments(messageState).some((shipment) => shipment.orderId === messageOrder.id))
  );
  const allSectionsReady = sourceSettled &&
    fadeReadiness.key === fadeKey &&
    fadeReadiness.message === true &&
    fadeReadiness.checks === true &&
    fadeReadiness.financial === true;
  // Once the current view is shown, do not flash again on every edit/IPC refresh.
  // Switching to a new order or shipment always starts a fresh transition.
  const sharedFadePhase: ContentFadePhase =
    useReadyContentFade(fadeKey, allSectionsReady, true);
  // Highlight must reveal the actual content, not just light the section.
  // Run after the hovered order's source/detail portals have rendered.
  useEffect(() => {
    if (!hoverSelection || !stageHover) return;
    const timer = setTimeout(() => {
      const target = (
        stageHover.step === 1
          ? ".hub-slot-2 .recipient-review"
          : stageHover.step === 2
            ? ".hub-slot-2 .freight-input-area"
            : stageHover.step === 3
              ? ".hub-slot-1 .message-copy-actions"
              : ".hub-slot-2 .workflow-balance"
      );
      document.querySelector<HTMLElement>(target)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }, 40);
    return () => clearTimeout(timer);
  }, [hoverSelection?.shipmentId, stageHover?.step, messageReady]);
  // A parent repaint must not recreate event props for either live OrderDesk.
  // Each wrapper executes the newest order and Rust workflow handlers.
  const saveWorkflowLive = useEventCallback(onSave);
  const changeReceptionLive = useEventCallback(changeReception);
  const changeDetailLive = useEventCallback(changeDetail);
  const saveDetailLive = useEventCallback(async () =>
    onSave(await nativeReconcileWorkflow(base, detail ?? draft)),
  );
  const selectMessageLive = useEventCallback((id: string) => {
    const shipment = messageState.workflow?.shipments.find((entry) => entry.id === id);
    if (shipment) setSelectedLive({
      orderId: shipment.orderId, groupId: shipment.groupId, shipmentId: shipment.id,
    });
  });
  const selectPendingLive = useEventCallback((id: string) => {
    if (id === draft.id || edits[id]) {
      commitSelection(undefined);
      setReceptionId(id);
    }
  });
  const deletePendingLive = useEventCallback((id: string) => {
    // Remove the working copy; autosave deletes the durable reception row,
    // leaving confirmed orders and their immutable history untouched.
    setEdits((previous) => {
      if (!(id in previous)) return previous;
      const next = { ...previous };
      delete next[id];
      return next;
    });
    if (id === draft.id) {
      const empty = newReception();
      props.onChange(empty);
      setReceptionId(empty.id);
    } else if (receptionId === id) {
      setReceptionId(draft.id);
    }
    setStageHover(undefined);
  });
  const createReceptionLive = useEventCallback(() => {
    explicitNewReception.current = true;
    // Multiple unfinished documents stay visible while their SQLite working
    // records are durably saved outside the confirmed order history.
    const draftsToRetain = [draft, receptionOrder].filter((order) =>
      !state.orders.some((saved) => saved.id === order.id) &&
      (order.raw.trim() || order.clientId || order.recipients.length || order.photos?.length));
    if (draftsToRetain.length)
      setEdits((previous) => ({
        ...previous,
        ...Object.fromEntries(draftsToRetain.map((order) => [
          order.id, structuredClone(order),
        ])),
      }));
    // A blank draft's ID remains unchanged on repeated New Order clicks.
    // Return the source display to it even when another receipt was selected.
    const emptyNewDraft = draft.receptionVersion === 1 &&
      !state.orders.some((saved) => saved.id === draft.id) &&
      !draft.raw.trim() && !draft.clientId &&
      !draft.recipients.length && !draft.photos?.length;
    if (emptyNewDraft) {
      setReceptionId(draft.id);
      setStageHover(undefined);
      setActive("");
      commitSelection(undefined);
      lastVisiblePreview.current = undefined;
    }
    props.onNew();
  });
  const openManual = useCallback(() => setManual(true), []);
  const receptionTargets = useMemo(() => ({ source, messages }), [source, messages]);
  const detailTargets = useMemo(() => ({ details }), [details]);
  return (
    <div
      className="hub-live-workspace"
      data-hover-stage={stageHover?.step}
      data-hover-selected={stageHover?.groupId === visibleSelection?.groupId ? "true" : "false"}
      data-one-touch-hover={stageHover && hoverSelection ? "true" : "false"}
    >
      <HubPanels
        externalKakao={props.externalKakao}
        ratios={state.settings.panelRatios ?? defaultPanels}
        onSave={(ratios) =>
          void props
            .onSaveState({
              ...state,
              settings: { ...state.settings, panelRatios: ratios },
            })
            .catch((e) => props.onNotice(String(e)))
        }
        onReset={() => {
          setVertical(undefined);
          void props
            .onSaveState({
              ...state,
              settings: {
                ...state.settings,
                panelRatios: defaultPanels,
                receptionVerticalRatio: 0.5,
              },
            })
            .catch((e) => props.onNotice(String(e)));
          props.onResetDock();
        }}
      >
        {[
          <section className="desk-pane hub-kakao-placeholder">
            <h2>카카오톡</h2>
            <p>
              상단의 카카오톡 함께 보기를 누르면
              <br />
              기본 메신저와 대화창을 왼쪽에 배치합니다.
            </p>
          </section>,
          <div
            className="hub-reception-stack"
            style={{
              gridTemplateRows: `minmax(275px,${verticalRatio}fr) 6px minmax(160px,${1 - verticalRatio}fr)`,
            }}
          >
            <div ref={setSource} className="hub-portal-target" />
            <div
              className="hub-split horizontal"
              role="separator"
              aria-label="원문과 메시지 높이 조절"
              aria-orientation="horizontal"
              tabIndex={0}
              title="드래그 높이 조절 / 두 번 클릭 5:5"
              onDoubleClick={() => {
                setVertical(undefined);
                void props.onSaveState({
                  ...state,
                  settings: {
                    ...state.settings,
                    panelRatios: defaultPanels,
                    receptionVerticalRatio: 0.5,
                  },
                });
                props.onResetDock();
              }}
              onPointerDown={(e) => {
                verticalDrag.current = {
                  y: e.clientY,
                  height: e.currentTarget.parentElement!.getBoundingClientRect().height,
                  start: verticalRatio,
                };
                e.currentTarget.setPointerCapture(e.pointerId);
              }}
              onPointerMove={(e) => {
                const d = verticalDrag.current;
                if (d)
                  setVertical(
                    Math.max(0.25, Math.min(0.75, d.start + (e.clientY - d.y) / d.height)),
                  );
              }}
              onPointerUp={() => {
                if (!verticalDrag.current) return;
                const start = verticalDrag.current.start;
                verticalDrag.current = undefined;
                if (vertical === undefined || vertical === start) return;
                void props
                  .onSaveState({
                    ...state,
                    settings: {
                      ...state.settings,
                      receptionVerticalRatio: verticalRatio,
                    },
                  })
                  .catch((e) => props.onNotice(String(e)));
              }}
              onPointerCancel={() => {
                verticalDrag.current = undefined;
                setVertical(undefined);
              }}
            />
            <div ref={setMessages} className="hub-portal-target" />
          </div>,
          <div ref={setDetails} className="hub-portal-target" />,
          <section className="desk-pane hub-management">
            <header className="pane-heading">
              <h2>처리 중인 주문</h2>

            </header>
            <AuditProfiler id="WorkflowBoard"><WorkflowBoard
              state={boardPreview}
              previewReady={previewReady}
              previewError={nativeBase.error || nativePreview.error}
              persistedState={base}
              selection={selected}
              activeKey={active}
              onSelect={setSelectedLive}
              onSave={saveWorkflowLive}
              onNotice={props.onNotice}
              onStageHover={setStageHover}
              stageHover={stageHover}
              hoveredShipmentId={hoverSelection?.shipmentId}
              pendingReceptions={pendingWithoutClient}
              selectedPendingOrderId={!selected ? receptionId : undefined}
              onSelectPending={selectPendingLive}
              onDeletePending={deletePendingLive}
            /></AuditProfiler>
          </section>,
        ]}
      </HubPanels>
      {messages &&
        createPortal(
          <LinkContext.Provider value={linkController}>
            <AuditProfiler id="MessageWorkspace"><MessageWorkspace
              state={messageState}
              order={messageOrder}
              gid={messageReady ? messageGroup : undefined}
              projectionReady={messageProjectionSettled}
              projectionError={nativeBase.error ||
                (messageOrder.id === draft.id ? nativePreview.error : nativeMessage.error)}
              noMessageExpected={noMessageExpected}
              sharedFadePhase={sharedFadePhase}
              fadeKey={fadeKey}
              onFadeReadiness={reportFadeReadiness}
              onOrder={changeReceptionLive}
              onSave={saveWorkflowLive}
              onSelect={selectMessageLive}
              onNotice={props.onNotice}
            /></AuditProfiler>
          </LinkContext.Provider>,
          messages,
        )}
      <div className="hub-order-engine">
        <AuditProfiler id="OrderDesk:reception"><MemoOrderDesk
          {...props}
          key={receptionOrder.id + ":reception:" + discardRevision}
          state={base}
          persistedOrders={state.orders}
          draft={receptionOrder}
          sharedNativeFlow={receptionOrder === draft ? sharedCurrentDraftFlow : undefined}
          sourceProjectionReady={sourceSettled}
          coordinatedFadePhase={sharedFadePhase}
          onChange={changeReceptionLive}
          viewMode="reception"
          onNew={createReceptionLive}
          onManual={openManual}
          portalTargets={receptionTargets}
          linkController={linkController}
          onWorkflowSave={saveWorkflowLive}
        /></AuditProfiler>
        <AuditProfiler id="OrderDesk:details"><MemoOrderDesk
          {...props}
          key={(detail?.id ?? draft.id) + ":details:" + discardRevision}
          state={base}
          persistedOrders={state.orders}
          draft={detail ?? draft}
          sharedNativeFlow={(detail ?? draft) === draft ? sharedCurrentDraftFlow : undefined}
          suppressPassiveOrderEffects={receptionOrder === draft && (detail ?? draft) === draft}
          viewMode="details"
          coordinatedFadePhase={sharedFadePhase}
          fadeKey={fadeKey}
          onFadeReadiness={reportFadeReadiness}
          onManual={openManual}
          portalTargets={detailTargets}
          linkController={linkController}
          workflowSelection={visibleSelection}
          onSelectWorkflow={setSelectedLive}
          onChange={changeDetailLive}
          onWorkflowSave={saveWorkflowLive}
          onSave={saveDetailLive}
        /></AuditProfiler>
      </div>
      {manual && (
        <Suspense fallback={null}>
          <ManualClassification
            key={receptionOrder.id + ":" + receptionOrder.raw}
            state={state}
            order={receptionOrder}
            onNotice={props.onNotice}
            onClose={() => setManual(false)}
            onApply={(next) => {
              if (state.orders.some((o) => o.id === next.id) || receptionOrder.raw !== next.raw)
                throw Error("접수 상태가 변경되었습니다. 다시 확인하세요.");
              changeReception(next);
              setActive("");
              commitSelection(undefined);
            }}
          />
        </Suspense>
      )}
    </div>
  );
}

/** Skip App-only repaints while workspace props are unchanged. */
export const MemoHubWorkspace = memo(HubWorkspace);
