import { sameValue } from "../shared/draft";
import type { ReactNode } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { type WorkflowSelection } from "../WorkflowBoard";
import { unassignedClient } from "../domain";
import { useNativeOrderClients } from "./useNativeOrderClients";
import type { StaffBlock } from "../staff-block-types";
import { businessCoreCall } from "../tauri-bridge";
import { useNativeWorkflowReconcile } from "./useNativeWorkflowReconcile";
import { useNativeWorkflowBoard } from "./useNativeWorkflowBoard";
import { useNativeOrderPreview } from "./useNativeOrderPreview";
import { useNativeOrderLineAmounts } from "./useNativeOrderLineAmounts";
import { useNativeOrderIssues } from "./useNativeOrderIssues";
import { assertRustReceptionAnalysis, receptionAnalysisAllowed } from "./rust-reception";
import { sortedRegisteredProductView } from "./registered-product-view";

import type { NoticeSink } from "../shared/notices";
import type { Item, Order, Product, State, ValidationIssue } from "../types";
import { clone } from "../ui";
import { groupShipments } from "../workflow-view";
type SaveMode = "save" | "notify" | "pay" | "queue" | "wait";
export type OrderDeskProps = {
  onRawEditDirty?: (dirty: boolean) => void;
  portalTargets?: {
    source?: HTMLElement | null;
    messages?: HTMLElement | null;
    details?: HTMLElement | null;
  };
  viewMode?: "all" | "reception" | "details";
  /** Readiness of the Rust projection for the currently displayed raw source. */
  sourceProjectionReady?: boolean;
  /** The single workspace fade phase; only data changes opacity. */
  coordinatedFadePhase?: import("../shared/useReadyContentFade").ContentFadePhase;
  fadeKey?: string;
  onFadeReadiness?: (
    key: string, section: "message" | "checks" | "financial", ready: boolean,
  ) => void;
  // Same draft is already reconciled by HubWorkspace. Reuse that exact Rust
  // projection rather than dispatching an identical second/third IPC.
  sharedNativeFlow?: {
    input: State;
    draft: Order;
    value: ReturnType<typeof useNativeWorkflowReconcile>;
  };
  // The reception panel owns passive Rust price/shipping reconciliation when
  // both panes point to the exact same draft. Detail edits still flow through
  // onChange and are processed by the mounted reception panel.
  suppressPassiveOrderEffects?: boolean;
  linkController?: {
    active: string;
    recipientId?: string;
    setActive: (value: string) => void;
    beginHover?: (key: string) => void;
    clearHover?: () => void;
  };
  workflowSelection?: WorkflowSelection;
  onSelectWorkflow: (s: WorkflowSelection) => void;
  onWorkflowSave: (s: State) => Promise<void>;
  state: State;
  // HubWorkspace may supply a preview state containing unsaved orders.
  // Use the authoritative persisted order set when deciding whether this
  // record is a new reception that may track current master prices.
  persistedOrders?: readonly Pick<Order, "id">[];
  draft: Order;
  onChange: (next: Order | ((previous: Order) => Order)) => void;
  onSave: (mode: SaveMode) => Promise<void>;
  onSaveState: (state: State) => Promise<void>;
  onNotice: NoticeSink;
  onManageProducts: () => void;
  onManual?: () => void;
  onNew?: () => void;
  onSettings?: () => void;
};
export function useOrderDesk({
  state,
  persistedOrders,
  draft,
  onChange,
  onSave,
  onSaveState,
  onNotice,
  onManageProducts,
  onManual,
  onSettings,
  workflowSelection,
  onSelectWorkflow,
  onWorkflowSave,
  onRawEditDirty,
  portalTargets,
  viewMode = "all",
  linkController,
  sharedNativeFlow,
  suppressPassiveOrderEffects = false,
}: OrderDeskProps) {
  const [resolving, setResolving] = useState<{
    rid: string;
    iid: string;
  }>();
  const [productSearch, setProductSearch] = useState<{
    rid: string;
    iid?: string;
  }>();
  const [itemEditing, setItemEditing] = useState<{
      rid: string;
      iid: string;
    }>(),
    [pendingFocus, setPendingFocus] = useState<string>();
  const [rawEditing, setRawEditing] = useState(false);
  const [localHighlight, setLocalHighlight] = useState("");
  const highlight = linkController?.active ?? localHighlight,
    setHighlight = linkController?.setActive ?? setLocalHighlight;
  const place = (content: ReactNode, node?: HTMLElement | null) =>
    portalTargets ? (node ? createPortal(content, node) : null) : content;
  const [source, setSource] = useState(draft.analysisRaw ?? draft.raw);
  const clientSelection = useNativeOrderClients(draft, state);
  const { client, liveClient } = clientSelection;
  const pricingClient = liveClient ?? unassignedClient;
  useEffect(() => {
    if (clientSelection.error)
      onNotice("Rust 거래처 선택 확인 실패: " + clientSelection.error, "error");
  }, [clientSelection.error]);
  const products = useMemo(
    () => sortedRegisteredProductView(state.products),
    [state.products],
  );
  // Live order-detail reconciliation is a read-only Rust computation.
  // In-flight calculations never fall back to the old TS workflow engine.
  const overlay = useMemo(() => [draft], [draft]);
  const upstreamFlow = sharedNativeFlow?.input === state && sharedNativeFlow.draft === draft
    ? sharedNativeFlow.value : undefined;
  const ownFlow = useNativeWorkflowReconcile(state, overlay, !upstreamFlow);
  const nativeFlow = upstreamFlow ?? ownFlow;
  const flowState = nativeFlow.state;
  const flowReady = nativeFlow.ready;
  const nativeBoard = useNativeWorkflowBoard(flowState);
  const nativeOrder = useNativeOrderPreview(flowState, draft, flowReady, state);
  // Line prices come from native Rust immediately, not after the entire
  // shipment workflow and issue preview resolves.
  const nativeLines = useNativeOrderLineAmounts(draft);
  const nativeIssues = useNativeOrderIssues(draft);
  const flowSummaryReady = flowReady && nativeBoard.ready && nativeOrder.ready && nativeIssues.ready;
  const flowGroups = (flowState.workflow?.groups ?? []).filter((g) =>
    groupShipments(flowState, g.id).some((s) => s.orderId === draft.id),
  );
  const activeGroupId = flowGroups.some((g) => g.id === workflowSelection?.groupId)
    ? workflowSelection!.groupId
    : (
        flowGroups.find((g) => groupShipments(flowState, g.id).some((s) => !s.transferred)) ??
        flowGroups[0]
      )?.id;
  const activeShipments = activeGroupId ? groupShipments(flowState, activeGroupId) : [];
  const activeShipment =
    activeShipments.find((s) => s.id === workflowSelection?.shipmentId) ?? activeShipments[0];
  const activeShipmentPreview = flowSummaryReady && activeGroupId && activeShipment
    ? nativeBoard.byId[activeGroupId]?.shipments.find((entry) => entry.shipmentId === activeShipment.id)
    : undefined;

  const persisted = persistedOrders ?? state.orders;
  const saved = persisted.some((o) => o.id === draft.id);
  // Workflow stage issues remain relevant to unsaved receptions once Rust
  // has constructed their shipment groups. Never drop their staff actions
  // back to the narrower order-level issues during preview refreshes.
  // The Rust rules engine owns whether an uncommitted order may follow current prices.
  // The signature is only an IPC freshness key; it is not a TS eligibility rule.
  const pricingEligibilityKey = JSON.stringify({
    id: draft.id, confirmedAt: draft.confirmedAt, deletedAt: draft.deletedAt,
    pricesLocked: draft.pricesLocked,
    persistedIds: persisted.map((record) => record.id),
  });
  const [nativeEligibility, setNativeEligibility] = useState<{
    key: string; refresh: boolean;
  }>();
  useEffect(() => {
    let active = true;
    void businessCoreCall<{ refresh: boolean }>("rules", "should_refresh_pricing", {
      order: draft, persistedOrders: persisted,
    }).then(({ refresh }) => {
      if (active) setNativeEligibility({ key: pricingEligibilityKey, refresh });
    }).catch((error) => {
      if (active) {
        setNativeEligibility(undefined);
        onNotice("Rust 가격 갱신 조건 확인 실패: " +
          (error instanceof Error ? error.message : String(error)), "error");
      }
    });
    return () => { active = false; };
  }, [pricingEligibilityKey]);
  const passivePricingAllowed =
    nativeEligibility?.key === pricingEligibilityKey && nativeEligibility.refresh;
  const exported = draft.status === "EXPORTED" || activeShipment?.transferred === true ||
      (!flowReady && (state.workflow?.shipments.some((ship) =>
        ship.orderId === draft.id && ship.transferred && !ship.legacyArchived) ?? false));
  useEffect(() => {
    if (viewMode === "details" && highlight)
      document
        .querySelector('.review-pane [data-link-key=\"' + CSS.escape(highlight) + '\"]')
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [highlight, viewMode]);
  useEffect(() => {
    setRawEditing(false);
    setSource(draft.analysisRaw ?? draft.raw);
  }, [draft.id, draft.rawCorrections?.at(-1)?.id]);
  useEffect(() => {
    if (viewMode !== "details") onRawEditDirty?.(rawEditing && source !== draft.raw);
  }, [rawEditing, source, draft.raw, viewMode, onRawEditDirty]);
  // Rust responses for pricing and shipping may arrive after the employee
  // has changed the draft or the master catalog. Reject those stale results
  // rather than overwriting newer manually approved information.
  const ruleRequests = useRef({ price: 0, shipping: 0 });
  const rulesClientSignature = JSON.stringify(liveClient ?? null);
  const latestRulesInput = useRef({ draft, state, rulesClientSignature, saved });
  latestRulesInput.current = { draft, state, rulesClientSignature, saved };
  // The native workflow preview can return a new JSON object with identical
  // content while the employee is picking a dealer. Comparing React object
  // identities here silently rejects a legitimate completed selection.
  // Preserve the stale-result guard by comparing semantic input values.
  const isCurrentOrderInput = (sourceDraft: Order, sourceState: State): boolean =>
    (latestRulesInput.current.draft === sourceDraft ||
      sameValue(latestRulesInput.current.draft, sourceDraft)) &&
    (latestRulesInput.current.state === sourceState ||
      sameValue(latestRulesInput.current.state, sourceState));
  // Editable text stays responsive in React. A transient intent prevents any
  // save until Rust has decided invoice freeze, notification/payment flags,
  // dispatch origin and freight invalidation for the latest complete draft.
  useEffect(() => {
    if (suppressPassiveOrderEffects || !draft.pendingNativeEdit) return;
    const sourceDraft = draft;
    const sourceState = state;
    let current = true;
    void businessCoreCall<{ order: Order }>("rules", "order_edit_rules", {
      action: "prepare_order_edit", order: sourceDraft, state: sourceState,
    }).then(({ order: native }) => {
      if (!current || !isCurrentOrderInput(sourceDraft, sourceState)) return;
      onChange((previous) => sameValue(previous, sourceDraft) ? native : previous);
    }).catch((error) => {
      if (current && isCurrentOrderInput(sourceDraft, sourceState))
        onNotice("Rust 주문 수정 상태 확인 실패 — 저장하지 않았습니다: " +
          (error instanceof Error ? error.message : String(error)), "error");
    });
    return () => { current = false; };
  }, [draft, state, suppressPassiveOrderEffects]);

  // Rust pricing depends on product/price-record identity and dealer terms,
  // not on a recipient's manually typed delivery charge. A freight keystroke
  // still changes the recipient object, so avoid rerunning reprice_order for
  // a pricing input that has already settled.
  const pricingInputKey = JSON.stringify({
    orderId: draft.id,
    rows: draft.recipients.map((row) => [
      row.id, row.items.map((item) => [
        item.id, item.productId, item.variantId, item.modelProposal?.required,
      ]),
    ]),
    client: liveClient?.id, tier: liveClient?.tier,
    overrides: liveClient?.overrides,
  });
  const lastCompletedPricing = useRef<{
    key: string; products: typeof state.products;
  } | undefined>(undefined);
  useEffect(() => {
    const request = ++ruleRequests.current.price;
    if (suppressPassiveOrderEffects || !clientSelection.ready || !liveClient ||
        !passivePricingAllowed || draft.pendingNativeEdit) return;
    if (lastCompletedPricing.current?.key === pricingInputKey &&
        lastCompletedPricing.current.products === state.products) return;
    const before = JSON.stringify(draft.recipients);
    const orderId = draft.id;
    void businessCoreCall<{ order: Order; refreshed: boolean }>(
      "rules", "reprice_order", {
        order: draft,
        client: liveClient,
        state,
        persistedOrders: persisted,
      },
    ).then(({ order, refreshed }) => {
      const current = latestRulesInput.current;
      if (!refreshed || request !== ruleRequests.current.price ||
          current.state !== state || current.draft.id !== orderId ||
          current.rulesClientSignature !== rulesClientSignature) return;
      onChange((previous) => {
        if (request !== ruleRequests.current.price ||
            previous !== draft ||
            previous.id !== orderId ||
            JSON.stringify(previous.recipients) !== before) return previous;
        const next = { ...previous, recipients: order.recipients };
        lastCompletedPricing.current = {
          key: pricingInputKey, products: state.products,
        };
        return JSON.stringify(previous.recipients) === JSON.stringify(next.recipients)
          ? previous : next;
      });
    }).catch((error) => {
      if (request === ruleRequests.current.price)
        onNotice("Rust 단가 계산 실패: " +
          (error instanceof Error ? error.message : String(error)), "error");
    });
    return () => { if (request === ruleRequests.current.price) ruleRequests.current.price++; };
  }, [
    draft.id, draft.recipients, state.products, liveClient?.id,
    liveClient?.tier, JSON.stringify(liveClient?.overrides),
    passivePricingAllowed, draft.pendingNativeEdit, pricingInputKey,
    clientSelection.ready, suppressPassiveOrderEffects,
  ]);
  useEffect(() => {
    const request = ++ruleRequests.current.shipping;
    // Existing completed orders are not repriced passively. The empty version
    // marker is set only by an explicit employee edit requiring new freight.
    const shippingPending = draft.shippingVersion === "" && draft.shippingFee === null;
    if (suppressPassiveOrderEffects ||
        (!passivePricingAllowed && !shippingPending) ||
        draft.pendingNativeEdit ||
        draft.shippingMode === "MANUAL" || draft.pricesLocked ||
        draft.deletedAt || exported ||
        !draft.recipients.some((r) => r.items.length > 0)) return;
    const before = JSON.stringify(draft.recipients);
    const orderId = draft.id;
    void businessCoreCall<{ total: number | null; version: string }>(
      "rules", "order_edit_rules", {
        action: "calculate_order_shipping", order: draft, state,
      },
    ).then((result) => {
      const current = latestRulesInput.current;
      if (request !== ruleRequests.current.shipping ||
          current.state !== state || current.draft.id !== orderId ||
          current.rulesClientSignature !== rulesClientSignature) return;
      onChange((previous) => {
        if (request !== ruleRequests.current.shipping ||
            previous.id !== orderId || previous.pricesLocked ||
            previous.shippingMode === "MANUAL" ||
            JSON.stringify(previous.recipients) !== before ||
            previous !== draft ||
            (!passivePricingAllowed &&
              !(previous.shippingVersion === "" && previous.shippingFee === null)))
          return previous;
        const fee = result.total; // waiver decision is owned by Rust
        if (previous.shippingFee === fee &&
            previous.shippingVersion === result.version) return previous;
        return {
          ...previous, shippingFee: fee,
          shippingVersion: result.version, shippingMode: "AUTO",
        };
      });
    }).catch((error) => {
      if (request === ruleRequests.current.shipping)
        onNotice("Rust 운임 계산 실패: " +
          (error instanceof Error ? error.message : String(error)), "error");
    });
    return () => {
      if (request === ruleRequests.current.shipping) ruleRequests.current.shipping++;
    };
  }, [
    draft.id, draft.recipients, draft.shippingMode, draft.shippingFee,
    draft.shippingVersion, draft.pricesLocked, draft.deletedAt,
    state.shipping, state.products, liveClient?.shippingFeeExcluded,
    passivePricingAllowed, exported, suppressPassiveOrderEffects,
  ]);
  // Every native response is associated with a specific draft, raw text,
  // client and catalog revision. A completed previous request must never
  // replace newer typing, a newly selected order or employee corrections.
  const recognitionRequest = useRef(0);
  // RETAIL's companyParty() can allocate a new equal object each render.
  // A semantic signature prevents repeated native calls on those renders.
  const clientSignature = JSON.stringify(liveClient ?? null);
  const latestReceptionInput = useRef({
    orderId: draft.id,
    clientId: draft.clientId,
    source,
    saved,
    manual: !!draft.manualClassification && draft.analysisRaw === source,
    products: state.products,
    aliases: state.aliases,
    clientSignature,
  });
  latestReceptionInput.current = {
    orderId: draft.id,
    clientId: draft.clientId,
    source,
    saved,
    manual: !!draft.manualClassification && draft.analysisRaw === source,
    products: state.products,
    aliases: state.aliases,
    clientSignature,
  };
  useEffect(() => {
    const request = ++recognitionRequest.current;
    if (
      viewMode === "details" ||
      !clientSelection.ready ||
      saved ||
      (!!draft.manualClassification && draft.analysisRaw === source) ||
      !receptionAnalysisAllowed(draft, source, pricingClient.id)
    ) return;
    const orderId = draft.id;
    const clientId = draft.clientId;
    const pending: Promise<unknown> = source.trim()
      ? businessCoreCall<unknown>("recognition", "analyze_order", {
          raw: source,
          client: liveClient ?? null,
          state,
        })
      : Promise.resolve({ recipients: [], candidates: [], unassigned: [] });
    void pending.then(async (result) => {
      const latest = latestReceptionInput.current;
      if (
        request !== recognitionRequest.current ||
        latest.orderId !== orderId ||
        latest.clientId !== clientId ||
        latest.source !== source ||
        latest.saved || latest.manual ||
        latest.products !== state.products ||
        latest.aliases !== state.aliases ||
        latest.clientSignature !== clientSignature
      ) return;
      assertRustReceptionAnalysis(result, source);
      // Rust owns stable recipient/item identity, confirmed-field retention,
      // evidence rebinding and order state reset. No legacy TS merge is run.
      const { order: merged } = await businessCoreCall<{ order: Order }>(
        "rules", "merge_reception_analysis", {
          previous: draft,
          evidence: result,
          raw: source,
          clientId: pricingClient.id,
          state,
          client: pricingClient,
          at: new Date().toISOString(),
        },
      );
      const settled = latestReceptionInput.current;
      if (
        request !== recognitionRequest.current ||
        settled.orderId !== orderId ||
        settled.clientId !== clientId ||
        settled.source !== source ||
        settled.saved || settled.manual ||
        settled.products !== state.products ||
        settled.aliases !== state.aliases ||
        settled.clientSignature !== clientSignature
      ) return;
      onChange((previous) => {
        const active = latestReceptionInput.current;
        if (
          request !== recognitionRequest.current ||
          active.orderId !== orderId ||
          active.clientId !== clientId ||
          active.source !== source ||
          active.saved || active.manual ||
          active.products !== state.products ||
          active.aliases !== state.aliases ||
          active.clientSignature !== clientSignature ||
          previous !== draft ||
          previous.id !== orderId ||
          previous.clientId !== clientId ||
          !receptionAnalysisAllowed(previous, source, pricingClient.id)
        ) return previous;
        return merged;
      });
    }).catch((error: unknown) => {
      const latest = latestReceptionInput.current;
      if (
        request !== recognitionRequest.current ||
        latest.orderId !== orderId ||
        latest.clientId !== clientId ||
        latest.source !== source ||
        latest.saved || latest.manual
      ) return;
      onNotice("Rust 발주서 자동 분석 실패: " +
        (error instanceof Error ? error.message : String(error)), "error");
    });
    return () => {
      if (recognitionRequest.current === request) recognitionRequest.current++;
    };
  }, [
    source, draft.id, draft.clientId, draft.analysisRaw, draft.analysisClientId,
    draft.manualClassification, draft.confirmedAt, draft.pricesLocked,
    viewMode, saved, pricingClient.id, clientSignature, clientSelection.ready,
    state.products, state.aliases,
  ]);
  const run = async (fn: () => Promise<unknown> | unknown) => {
    try {
      await fn();
    } catch (e) {
      onNotice(e instanceof Error ? e.message : String(e), "error");
    }
  };
  const edit = (fn: (o: Order) => void, reason = "주문 정보 수정", recalculateShipping = true) => {
    if (draft.deletedAt) {
      onNotice("삭제된 발주는 이력으로만 조회할 수 있습니다.");
      return;
    }
    if (exported) {
      onNotice("공장 전달 완료 주문은 변경할 수 없습니다. 출고 취소 / 회수 확인이 필요합니다.");
      return;
    }
    onChange((previous) => {
      const next = clone(previous);
      fn(next); // UI's field change only; no financial/workflow mutation here.
      const firstIntent = previous.pendingNativeEdit;
      next.pendingNativeEdit = firstIntent ? {
        ...firstIntent,
        recalculateShipping: firstIntent.recalculateShipping || recalculateShipping,
      } : { at: new Date().toISOString(), reason, recalculateShipping };
      // The Rust reducer alone can clear pendingNativeEdit.
      return next;
    });
  };
  const recEdit = (rid: string, fn: (r: Order["recipients"][number]) => void, recalculate = true) =>
    edit(
      (o) => {
        const r = o.recipients.find((r) => r.id === rid);
        if (r) fn(r);
      },
      "수취인 / 배송 정보 수정",
      recalculate,
    );
  const itemEdit = (rid: string, iid: string, fn: (i: Item) => void) =>
    recEdit(rid, (r) => {
      const i = r.items.find((i) => i.id === iid);
      if (i) {
        const before = JSON.stringify([i.productId, i.variantId, i.quantity, i.quantityConfirmed]);
        fn(i);
        if (before !== JSON.stringify([i.productId, i.variantId, i.quantity, i.quantityConfirmed]))
          delete i.quantityApproval;
        i.manuallyConfirmed = true;
      }
    });

  // Keep the last Rust-produced stage issues for display while a new workflow
  // preview is in flight. They are only presentation data, NEVER authority
  // for an action: interaction is disabled until both native reads are fresh.
  const cachedWorkflowChecks = useRef<{ orderId: string; issues: ValidationIssue[] } | null>(null);
  const currentWorkflowIssues = flowReady && nativeBoard.ready && flowGroups.length > 0
    ? flowGroups.flatMap((g) => nativeBoard.byId[g.id]?.issues ?? [])
    : null;
  if (currentWorkflowIssues) {
    cachedWorkflowChecks.current = { orderId: draft.id, issues: currentWorkflowIssues };
  } else if (flowReady && flowGroups.length === 0 ||
             !draft.raw.trim() && !draft.recipients.length) {
    // A completed Rust reconciliation with no groups is a real removal.
    cachedWorkflowChecks.current = null;
  }
  const cachedIssueRows = cachedWorkflowChecks.current?.orderId === draft.id
    ? cachedWorkflowChecks.current.issues : null;
  const displayWorkflowIssues = currentWorkflowIssues ?? cachedIssueRows;
  const allBlocks = !draft.raw.trim() && !draft.recipients.length
    ? [] : displayWorkflowIssues ?? nativeIssues.issues;
  const staffChecksReady = displayWorkflowIssues !== null || nativeIssues.ready;
  const staffChecksActionsReady = nativeIssues.ready &&
    (currentWorkflowIssues !== null || cachedIssueRows === null);
  const staffChecksError = nativeIssues.error ||
    (flowReady && flowGroups.length > 0 ? nativeBoard.error : "") || nativeFlow.error;
  // Rust, not React, supplies which one-item provisional action is allowed.
  // The workflow board may supply the issue label; the live Rust order-issue
  // projection supplies the same issue's authoritative action eligibility.
  const approvalFlags = new Map(nativeIssues.issues.map(
    (issue) => [issue.issueId, issue.canApproveOne === true],
  ));
  const staffBlocks: StaffBlock[] = allBlocks
    .filter((b, i, a) => a.findIndex((x) => x.issueId === b.issueId) === i)
    .map((b) => {
      const r = draft.recipients.find((r) => r.id === b.recipientId),
        item = r?.items.find((i) => i.id === b.itemId);
      // This is a read-only description of the dealer already chosen by Rust.
      // An unresolved suggestion remains an ACTION; no candidate is applied here.
      const autoDealerApplied = b.kind === "CLIENT_MATCH_EVIDENCE" &&
        !!client && draft.clientId === draft.clientMatch?.clientId;
      return {
        ...b,
        title: autoDealerApplied ? "거래처 자동 적용" : b.title,
        canApproveOne: nativeIssues.ready && approvalFlags.get(b.issueId) === true,
        detail:
          (autoDealerApplied ? `${client?.name ?? "선택된"} 거래처를 자동 적용했습니다. ` : "") +
          b.detail +
          (b.kind === "APPLIED" ? " × " + item?.quantity : "") +
          (item?.recognitionReason ? " / " + item.recognitionReason : ""),
        choices:
          b.kind === "CLIENT" || b.kind.startsWith("CLIENT_MATCH_")
            ? draft.clientMatch?.candidates?.map((c) => ({
                label: c.name, value: c.clientId,
              }))
            : b.kind.startsWith("CANDIDATE_")
              ? r?.candidates
                  ?.filter((c) => c.field === b.kind.slice(10))
                  .map((c) => ({ label: c.label, value: c.value }))
              : undefined,
      };
    });
  const question = (b: StaffBlock) => {
    if (!b.issueId) return;
    const sourceDraft = draft;
    const issueId = b.issueId;
    void businessCoreCall<{ order: Order }>("rules", "apply_issue_action", {
      action: "pending_issue", order: sourceDraft, issueId,
      actor: state.settings.operatorName || "사용자", now: new Date().toISOString(),
    }).then(({ order: next }) => {
      if (latestRulesInput.current.draft !== sourceDraft) return;
      onChange((previous) => {
        if (previous !== sourceDraft) return previous;
        return {
          ...next,
          requestedIssueIds: [...new Set([...(previous.requestedIssueIds ?? []), issueId])],
        };
      });
    }).catch((error) => onNotice(
      "Rust 주문 확인 요청 처리 실패: " +
      (error instanceof Error ? error.message : String(error)), "error",
    ));
  };
  const shownRecipient =
    activeShipment?.recipientId ??
    draft.recipients.find(
      (r) => "shipment:" + draft.id + ":" + r.id === workflowSelection?.shipmentId,
    )?.id ??
    draft.recipients[0]?.id;
  const selectIssue = (block: StaffBlock) => {
    if (["CLIENT_MATCH_EVIDENCE", "CLIENT_MATCH_CONFLICT", "CLIENT_MATCH_ERROR"].includes(block.kind ?? "")) {
      // The first-step signal highlights this exact region on hover.
      // Clicking an evidence message moves staff to the same dealer control.
      const control = document.querySelector<HTMLButtonElement>(
        ".source-top .receipt-client button, .source-top .source-client-actions > button"
      );
      control?.scrollIntoView({ block: "center", behavior: "smooth" });
      control?.focus();
      return;
    }
    if (block.kind === "UNASSIGNED_EVIDENCE") {
      onManual?.();
      return;
    }
    if ((block.kind === "ITEM" || block.kind === "OPTION") && block.recipientId) {
      // A configuration is a standalone product, never a secondary dropdown selection.
      setProductSearch({ rid: block.recipientId, iid: block.itemId });
      return;
    }
    if (exported && block.itemId) {
      onNotice("전달한 품목은 엑셀 이력의 공장 정정 요청에서 변경하세요.");
      return;
    }
    if (
      draft.orderType === "DEALER" &&
      ["SENDER_NAME", "SENDER_ADDRESS", "SENDER_CONTACT"].includes(block.kind ?? "")
    ) {
      onSettings?.();
      return;
    }
    if (!block.recipientId) {
      if (block.kind === "CLIENT")
        document.querySelector<HTMLButtonElement>(".source-client-actions > button")?.click();
      document.querySelector<HTMLInputElement>('[aria-label="거래처 통합 검색"]')?.focus();
      return;
    }
    const shipment = flowState.workflow?.shipments.find(
      (s) => s.orderId === draft.id && s.recipientId === block.recipientId,
    );
    onSelectWorkflow(
      shipment
        ? {
            orderId: draft.id,
            groupId: shipment.groupId,
            shipmentId: shipment.id,
          }
        : {
            orderId: draft.id,
            groupId: "group:" + draft.id,
            shipmentId: "shipment:" + draft.id + ":" + block.recipientId,
          },
    );
    if (block.itemId) {
      if (
        block.severity !== "INFO" &&
        [
          "MODEL",
          "MODEL_CHOICE",
          "ALIAS_CONFLICT",
          "CATEGORY_CONFLICT",
          "QUANTITY_LINK",
          "OPTION",
        ].includes(block.kind ?? "")
      )
        setResolving({ rid: block.recipientId, iid: block.itemId });
      else if (block.kind !== "QUANTITY_MISSING")
        setItemEditing({ rid: block.recipientId, iid: block.itemId });
    }
    if (block.linkKey) {
      setHighlight(block.linkKey);
      setPendingFocus(block.linkKey);
    }
  };
  useEffect(() => {
    if (!pendingFocus) return;
    const frame = requestAnimationFrame(() => {
      const targets = Array.from(document.querySelectorAll<HTMLElement>("[data-link-key]")),
        target = targets.find(
          (e) =>
            e.dataset.linkKey === pendingFocus &&
            (e.closest(".review-pane") || e.closest('[aria-label="상품 상세 수정"]')),
        );
      if (!target) return;
      target.scrollIntoView({ block: "nearest" });
      (
        target.querySelector<HTMLElement>("input:not(:disabled)") ??
        target.querySelector<HTMLElement>("select,button")
      )?.focus({ preventScroll: true });
      setPendingFocus(undefined);
    });
    return () => cancelAnimationFrame(frame);
  }, [pendingFocus, shownRecipient, itemEditing]);
  const chooseProduct = (rid: string, p: Product, iid?: string) => {
    if (!clientSelection.ready) {
      onNotice("Rust 거래처 선택을 확인한 뒤 상품을 추가하세요.", "error");
      return;
    }
    const orderId = draft.id;
    const itemId = iid;
    const variantId = p.variants[0]?.id;
    void run(async () => {
      const { item: catalogItem } = await businessCoreCall<{ item: Item }>(
        "rules", "create_catalog_item", {
          product: p, variantId, client: pricingClient,
          quantity: null, newItemId: crypto.randomUUID(),
        },
      );
      // Rust already rejects ambiguous options and products without a
      // registered independent selling unit. Never preempt that decision
      // with a different TypeScript variant-count gate.
      if (!catalogItem.variantId) {
        onNotice(p.requiresOption
          ? "상품 구성이 미확정입니다. 판매 구성을 확인한 뒤 독립 상품으로 등록하세요."
          : "상품 가격 기록이 정확히 하나가 아닙니다. 상품 데이터를 확인하세요.", "error");
        return;
      }
      const current = latestRulesInput.current;
      if (current.draft.id !== orderId || current.state !== state ||
          current.rulesClientSignature !== rulesClientSignature ||
          current.draft.deletedAt || current.draft.status === "EXPORTED")
        return;
      if (itemId) {
        // The visible product name, old evidence and manually confirmed
        // quantity survive; only the price comes from native Rust.
        if (!current.draft.recipients.some((r) =>
          r.id === rid && r.items.some((item) => item.id === itemId))) return;
        itemEdit(rid, itemId, (item) => {
          const proposal = !item.productId || !!item.modelProposal?.required;
          const old = {
            id: item.id, raw: item.raw, sources: item.sources,
            quantityConfirmed: item.quantityConfirmed,
          };
          const currentQuantity = item.quantity;
          Object.assign(item, catalogItem, old, {
            quantity: currentQuantity,
            manuallyConfirmed: true, unitPrice: catalogItem.unitPrice,
          });
          item.match = { kind: "NAME", term: p.name, productIds: [p.id] };
          item.temporary = false;
          delete item.pendingAlias;
          if (proposal) {
            item.modelProposal = { original: old.raw, required: true };
            item.issues.push("추천 상품 고객 확인 필요");
          }
        });
      } else {
        recEdit(rid, (r) => r.items.push({
          ...catalogItem, quantityConfirmed: false,
        }));
      }
      setProductSearch(undefined);
    });
  };
  return {
    highlight,
    linkController,
    setHighlight,
    viewMode,
    place,
    state,
    draft,
    source,
    client,
    clientSelectionPending: !clientSelection.ready && !clientSelection.error,
    setSource,
    saved,
    onChange,
    exported,
    onWorkflowSave,
    onSave,
    onSaveState,
    onNotice,
    portalTargets,
    staffBlocks,
    selectIssue,
    question,
    recEdit,
    products,
    pricingClient,
    isCurrentOrderInput,
    shownRecipient,
    setItemEditing,
    itemEdit,
    activeShipment,
    activeShipmentPreview,
    flowState,
    flowReady,
    flowSummaryReady,
    // A recipient can only be revealed after the current Rust line amounts
    // and the workflow/order financial projection have both resolved.
    orderLineReady: nativeLines.ready,
    financialPreviewReady: flowSummaryReady && nativeLines.ready &&
      (!activeShipment || Boolean(activeShipmentPreview)),
    staffChecksReady,
    staffChecksActionsReady,
    staffChecksError,
    flowSummaryError: nativeFlow.error || nativeBoard.error || nativeOrder.error || nativeIssues.error,
    orderPreview: nativeOrder.displayResult,
    orderLineAmounts: nativeLines.lineAmounts,
    orderLineError: nativeLines.error,
    setProductSearch,
    edit,
    activeGroupId,
    run,
    resolving,
    setResolving,
    productSearch,
    chooseProduct,
    itemEditing,
  };
}
