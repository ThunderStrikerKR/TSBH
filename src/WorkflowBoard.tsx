import { nativePaymentWasCopied } from "./native-automatic-message";
import { workflowGroupOpen } from "./workspace-selection";
import { useNativeWorkflowBoard } from "./orders/useNativeWorkflowBoard";
import { useNativeShipmentRead } from "./orders/useNativeShipmentRead";
import { memo, useEffect, useRef, useState } from "react";
import { auditReactRender } from "./perf-audit";
import { confirmAction } from "./dialogs";
import { won } from "./domain";
import { nativeApplyWorkflowAction, nativeChangeGroup } from "./orders/native-rules";
import { internalModel } from "./matching";
import type { NoticeSink } from "./shared/notices";
import type { Order, State } from "./types";
import { Field, Modal } from "./ui";

import {
  groupShipments,
  shipmentContext,
} from "./workflow-view";
export interface WorkflowSelection {
  orderId: string;
  groupId: string;
  shipmentId?: string;
}
export const stepNames = ["주문 확인", "운임 적용", "계좌 안내", "입금 확인"];
export function SignalLights({ step, notice = false }: { step: number; notice?: boolean }) {
  const current = step;
  return (
    <span
      className="workflow-signals"
      aria-label={`4단계 중 ${step}단계 완료${notice ? " / 재안내 필요" : ""}`}
    >
      {stepNames.map((name, index) => (
        <span
          key={name}
          title={name}
          aria-label={`${name}: ${notice && current === 2 && index === 2 ? "재안내 필요" : index < current ? "완료" : index === current ? "진행 중" : "대기"}`}
          className={
            "signal " + (index < current ? "done" : index === current ? "current" : "pending")
          }
        />
      ))}
    </span>
  );
}
export function WorkflowBoard({
  state,
  previewReady = true,
  previewError = "",
  persistedState,
  selection,
  activeKey,
  onSelect,
  onSave,
  onNotice,
  onStageHover,
  stageHover,
  hoveredShipmentId,
  pendingReceptions = [],
  selectedPendingOrderId,
  onSelectPending,
  onDeletePending,
}: {
  onStageHover?: (v: { groupId: string; step: number; shipmentId?: string } | undefined) => void;
  stageHover?: { groupId: string; step: number; shipmentId?: string };
  hoveredShipmentId?: string;
  state: State;
  previewReady?: boolean;
  previewError?: string;
  persistedState?: State;
  activeKey?: string;
  selection?: WorkflowSelection;
  onSelect: (s: WorkflowSelection) => void;
  onSave: (s: State) => Promise<void>;
  onNotice: NoticeSink;
  pendingReceptions?: Array<Pick<Order, "id" | "raw" | "recipients" | "createdAt">>;
  selectedPendingOrderId?: string;
  onSelectPending?: (orderId: string) => void;
  onDeletePending?: (orderId: string) => void;
}) {
  auditReactRender("WorkflowBoard");
  const [undoDelete, setUndoDelete] = useState<{ gid: string; at: string }>();
  const native = useNativeWorkflowBoard(state, previewReady);
  // Keep the selected dealer stable while independent workflow preview requests settle.
  const shipmentRead = useNativeShipmentRead(state);
  const ready = previewReady && native.ready && shipmentRead.ready;
  // A newly selected group opens automatically; all other groups are closed.
  useEffect(() => setManuallyClosedGroup(undefined), [selection?.groupId]);
  useEffect(() => {
    if (!undoDelete) return;
    const end = (e: Event) => {
      if (e.target instanceof Element && e.target.closest(".delete-undo")) return;
      setUndoDelete(undefined);
    };
    for (const type of ["pointerdown", "keydown", "paste"])
      document.addEventListener(type, end, true);
    return () => {
      for (const type of ["pointerdown", "keydown", "paste"])
        document.removeEventListener(type, end, true);
    };
  }, [undoDelete]);
  const previousSelection = useRef(selection?.shipmentId),
    [leaving, setLeaving] = useState<string>();
  useEffect(() => {
    if (previousSelection.current === selection?.shipmentId) return;
    setLeaving(previousSelection.current);
    previousSelection.current = selection?.shipmentId;
    const timer = setTimeout(() => setLeaving(undefined), 330);
    return () => clearTimeout(timer);
  }, [selection?.shipmentId]);
  const hoveredGroup = useRef<string | undefined>(undefined),
    hoveredRow = useRef<string | undefined>(undefined),
    hoverDelay = useRef<ReturnType<typeof setTimeout> | undefined>(undefined),
    hoverReturnDelay = useRef<ReturnType<typeof setTimeout> | undefined>(undefined),
    rowEnterDelay = useRef<ReturnType<typeof setTimeout> | undefined>(undefined),
    rowLeaveDelay = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // The preview is anchored in the SAME layout position as the original
  // card, so viewport coordinates and independent popup sizing are unnecessary.
  const [previewLayer, setPreviewLayer] = useState<{ groupId: string }>();
  const stopStagePreview = () => {
    if (hoverDelay.current) clearTimeout(hoverDelay.current);
    if (hoverReturnDelay.current) clearTimeout(hoverReturnDelay.current);
    if (rowEnterDelay.current) clearTimeout(rowEnterDelay.current);
    if (rowLeaveDelay.current) clearTimeout(rowLeaveDelay.current);
    hoverDelay.current = undefined;
    hoverReturnDelay.current = undefined;
    rowEnterDelay.current = undefined;
    rowLeaveDelay.current = undefined;
    hoveredGroup.current = undefined;
    hoveredRow.current = undefined;
    setPreviewLayer(undefined);
    onStageHover?.(undefined);
  };
  const startStagePreview = (button: HTMLButtonElement, groupId: string, step: number) => {
    if (hoverDelay.current) clearTimeout(hoverDelay.current);
    if (hoverReturnDelay.current) clearTimeout(hoverReturnDelay.current);
    hoverDelay.current = undefined;
    hoverReturnDelay.current = undefined;
    if (hoveredGroup.current === groupId) return;
    hoverDelay.current = setTimeout(() => {
      if (!button.isConnected) return;
      hoveredGroup.current = groupId;
      setPreviewLayer({ groupId });
      onStageHover?.({ groupId, step: Math.min(step + 1, 4) });
      hoverDelay.current = undefined;
    }, 250);
  };
  const scheduleStagePreviewExit = () => {
    if (hoverDelay.current) clearTimeout(hoverDelay.current);
    if (hoverReturnDelay.current) clearTimeout(hoverReturnDelay.current);
    hoverDelay.current = undefined;
    hoverReturnDelay.current = setTimeout(() => {
      hoverReturnDelay.current = undefined;
      stopStagePreview();
    }, 250);
  };
  const keepStagePreview = () => {
    if (hoverReturnDelay.current) clearTimeout(hoverReturnDelay.current);
    hoverReturnDelay.current = undefined;
  };
  // A preview row temporarily replaces the displayed shipment, without
  // changing the expanded/selected card or writing to Rust/SQLite.
  // Fast pointer passes do not cause the detail panes to flicker.
  const beginPreviewRowHover = (groupId: string, step: number, shipmentId: string) => {
    if (rowEnterDelay.current) clearTimeout(rowEnterDelay.current);
    if (rowLeaveDelay.current) clearTimeout(rowLeaveDelay.current);
    rowLeaveDelay.current = undefined;
    if (hoveredRow.current === shipmentId) return;
    rowEnterDelay.current = setTimeout(() => {
      rowEnterDelay.current = undefined;
      if (hoveredGroup.current !== groupId) return;
      hoveredRow.current = shipmentId;
      onStageHover?.({ groupId, step: Math.min(step + 1, 4), shipmentId });
    }, 100);
  };
  const endPreviewRowHover = (groupId: string, step: number) => {
    if (rowEnterDelay.current) clearTimeout(rowEnterDelay.current);
    if (rowLeaveDelay.current) clearTimeout(rowLeaveDelay.current);
    rowEnterDelay.current = undefined;
    rowLeaveDelay.current = setTimeout(() => {
      rowLeaveDelay.current = undefined;
      if (hoveredGroup.current !== groupId) return;
      hoveredRow.current = undefined;
      onStageHover?.({ groupId, step: Math.min(step + 1, 4) });
    }, 150);
  };

  useEffect(() => () => {
    if (hoverDelay.current) clearTimeout(hoverDelay.current);
    if (hoverReturnDelay.current) clearTimeout(hoverReturnDelay.current);
    if (rowEnterDelay.current) clearTimeout(rowEnterDelay.current);
    if (rowLeaveDelay.current) clearTimeout(rowLeaveDelay.current);
  }, []);
  useEffect(() => {
    const id = hoveredGroup.current;
    if (!id) return;
    if (!ready) return;
    const step = native.byId[id]?.step;
    if (step === undefined) return;
    onStageHover?.({ groupId: id, step: Math.min(step + 1, 4), shipmentId: hoveredRow.current });
  }, [ready, native.byId, onStageHover]);
  const [manuallyClosedGroup, setManuallyClosedGroup] = useState<string>(),
    [pendingManage, setPendingManage] = useState<string>(),
    [busy, setBusy] = useState(false),
    [manage, setManage] = useState<string>();
  const actionState = (groupId: string) =>
    persistedState &&
    persistedState.workflow?.groups.some((g) => g.id === groupId) &&
    groupShipments(state, groupId)[0]?.orderId !== selection?.orderId
      ? persistedState
      : state;
  const inFlight = useRef(false),
    [deadlines, setDeadlines] = useState<Record<string, number>>({}),
    [clock, setClock] = useState(Date.now);
  const stageDeadlines = { ...deadlines };
  for (const e of state.workflow?.events ?? [])
    if (
      e.groupId &&
      [
        "주문 확인 완료",
        "운임 적용",
        "계좌 안내 완료",
        "입금 확인 완료",
        "후불 출고 승인",
      ].includes(e.action)
    )
      stageDeadlines[e.groupId] = Math.max(stageDeadlines[e.groupId] ?? 0, Date.parse(e.at) + 2000);
  const latestDeadline = Math.max(0, ...Object.values(stageDeadlines));
  useEffect(() => {
    setClock(Date.now());
    if (latestDeadline <= Date.now()) return;
    const timer = setInterval(() => {
      setClock(Date.now());
      if (Date.now() >= latestDeadline) clearInterval(timer);
    }, 100);
    return () => clearInterval(timer);
  }, [latestDeadline]);
  const run = async (fn: () => Promise<State> | State, stageGroupId?: string) => {
    if (!ready || inFlight.current || (stageGroupId && (stageDeadlines[stageGroupId] ?? 0) > Date.now()))
      return;
    inFlight.current = true;
    setBusy(true);
    try {
      await onSave(await fn());
      if (stageGroupId) {
        const at = Date.now();
        setDeadlines((previous) => ({
          ...previous,
          [stageGroupId]: at + 2000,
        }));
        setClock(at);
      }
    } catch (e) {
      const issue = native.ready && stageGroupId
        ? native.byId[stageGroupId]?.issues.find((issue) =>
            issue.severity === "ACTION" &&
            issue.blockingStage <= Math.min(4, native.byId[stageGroupId]?.nextStep ?? 4),
          )
        : undefined;
      if (stageGroupId && issue) {
        const shipments = groupShipments(state, stageGroupId),
          target = shipments.find((ship) => ship.recipientId === issue.recipientId) ?? shipments[0];
        onSelect({
          orderId: target.orderId,
          groupId: stageGroupId,
          shipmentId: target.id,
        });
        document.querySelector(".staff-checks")?.scrollIntoView({ block: "nearest" });
      } else onNotice(e instanceof Error ? e.message : String(e), "error");
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };
  const groups = (state.workflow?.groups ?? [])
    .filter((g) => {
      const ss = groupShipments(state, g.id);
      return ss.length && ss.some((s) => !s.transferred);
    })
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  // A dealer-unassigned reception is a normal list entry, not a separate
  // alert-style card. It has no priced workflow group until a dealer is selected.
  const entries = [
    ...groups.map((group) => ({ kind: "group" as const, createdAt: group.createdAt, group })),
    ...pendingReceptions.map((receipt) => ({
      kind: "pending" as const, createdAt: receipt.createdAt, receipt,
    })),
  ].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  return (
    <div className="workflow-board" aria-label="진행 중인 배송 묶음" aria-busy={!ready}>
      {(previewError || native.error || shipmentRead.error) && (
        <p role="alert">업무 미리보기 계산 오류: {previewError || native.error || shipmentRead.error}</p>
      )}
      {!entries.length && (
        <p className="muted">거래처와 발주 원문을 입력하면 수취인별 배송 건이 생성됩니다.</p>
      )}
      {entries.map((entry) => {
        if (entry.kind === "pending") {
          const receipt = entry.receipt;
          const pendingSelected = selectedPendingOrderId === receipt.id;
          const pendingKey = "pending:" + receipt.id;
          const open = pendingSelected && manuallyClosedGroup !== pendingKey;
          return (
            <article className={"workflow-group" + (pendingSelected ? " selected" : "")}
              key={receipt.id} data-pending-order-id={receipt.id}>
              <header>
                <button type="button" className="flow-heading-toggle"
                  aria-label={"미확인 거래처" + (open ? " 묶음 접기" : " 묶음 펼치기")}
                  aria-expanded={open}
                  onClick={() => {
                    if (pendingSelected) {
                      setManuallyClosedGroup(open ? pendingKey : undefined);
                    } else {
                      setManuallyClosedGroup(undefined);
                      onSelectPending?.(receipt.id);
                    }
                  }}>
                  <span className="flow-toggle-icon" aria-hidden="true">{open ? "▾" : "▸"}</span>
                  <span className="flow-group-name"><b>미확인 거래처</b></span>
                  <SignalLights step={0} />
                </button>
                <button type="button" onClick={() => setPendingManage(receipt.id)}>
                  묶음 관리
                </button>
                <button type="button" className="trash-button"
                  aria-label="미확인 거래처 발주서 삭제"
                  onClick={async () => {
                    if (await confirmAction(
                      "미확인 거래처의 미완성 발주서를 삭제할까요? 임시 원문과 분석 내용이 사라집니다.",
                    )) onDeletePending?.(receipt.id);
                  }}>
                  <svg width="14" height="14" viewBox="0 0 16 16"
                    fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
                    <path d="M3 4h10M6 2h4M4 4l1 10h6l1-10M7 6v6M9 6v6" />
                  </svg>
                </button>
              </header>
              <div className="workflow-action-line">
                <div className="flow-fixed-summary" aria-label="발주서 묶음 금액 요약">
                  <div className="flow-summary-compact">
                    <span>배송 {receipt.recipients.length}건</span>
                    <span className="flow-summary-financial">
                      <span className="flow-summary-payment">금액 미확정</span>
                    </span>
                  </div>
                </div>
                <div className="workflow-progress">
                  <button type="button" className="primary" aria-label="거래처 확인"
                    onClick={() => onSelectPending?.(receipt.id)}>거래처 확인</button>
                  <button type="button" className="workflow-undo" disabled
                    title="거래처 확인 후 업무 단계를 시작할 수 있습니다."
                    aria-label="이전 단계 취소">↶</button>
                </div>
              </div>
              {open && (
                <div className="workflow-shipment-list">
                  <div className="workflow-list-head">
                    <span>수취인</span><span>상품</span><span>운임</span>
                  </div>
                  {receipt.recipients.length ? receipt.recipients.map((recipient) => (
                    <button type="button" className="workflow-shipment"
                      key={recipient.id} onClick={() => onSelectPending?.(receipt.id)}>
                      <span className="shipment-recipient" title={recipient.name}>
                        {recipient.name || "수취인 확인"}
                      </span>
                      <span className="shipment-products">
                        {recipient.items.map((item) => (
                          <span className="product-chip" key={item.id} title={item.name}>
                            {internalModel(item.name)} × {item.quantity ?? "?"}
                          </span>
                        ))}
                      </span>
                      <span className="shipment-freight">—</span>
                    </button>
                  )) : (
                    <div className="workflow-shipment">
                      <span className="shipment-recipient">수취인 확인</span>
                      <span className="shipment-products">
                        {receipt.raw.trim().split(/\r?\n/).find((line) => line.trim())?.slice(0, 60) || "사진 발주서"}
                      </span>
                      <span className="shipment-freight">—</span>
                    </div>
                  )}
                </div>
              )}
            </article>
          );
        }
        const g = entry.group;
        const nativeGroup = native.byId[g.id];
        const ss = groupShipments(state, g.id),
          step = nativeGroup?.step ?? 0,
          first = shipmentContext(state, ss[0], shipmentRead.shipments.get(ss[0].id)?.client),
          selected = selection?.groupId === g.id,
          hovered = stageHover?.groupId === g.id,
          open = workflowGroupOpen(g.id, selection?.groupId, manuallyClosedGroup);
        const dealerName = first.client?.name ||
          (shipmentRead.ready ? "거래처 확인 필요" : "거래처 확인 중");
        const label =
          g.needsNotice && step >= 2
            ? "계좌 재안내 완료"
            : step === 4
              ? "처리완료"
              : [
                  "주문 확인 완료",
                  "운임 적용",
                  "계좌 안내 완료",
                  first.client?.payment === "POSTPAID" ? "후불 출고 승인" : "입금 확인 완료",
                ][step];
        // The Rust preview already decides VAT per shipment. Present mixed
        // groups with actual included/excluded counts, not the ambiguous
        // native shorthand "부가세 포함/별도".
        const vatIncludedCount = nativeGroup?.shipments.filter((shipment) => shipment.vatIncluded).length ?? 0;
        const vatExcludedCount = nativeGroup ? nativeGroup.shipments.length - vatIncludedCount : 0;
        const mixedVat = vatIncludedCount > 0 && vatExcludedCount > 0;
        const vatDescription = mixedVat
          ? `부가세 포함 ${vatIncludedCount}건 · 부가세 별도 ${vatExcludedCount}건`
          : vatIncludedCount > 0 ? "부가세 포함" : "부가세 별도";
        const remaining = Math.max(
            0,
            Math.ceil(((stageDeadlines[g.id] ?? 0) - Math.max(clock, Date.now())) / 1000),
          ),
          buttonLabel = label + (remaining ? ` (${remaining})` : "");
        // Expanded contents and hover preview intentionally render the SAME
        // shipment table so their headings, fields and status cues cannot drift.
        const renderShipmentList = (inPreview: boolean) => (
              <div className="workflow-shipment-list">
                <div className="workflow-list-head">
                  <span>수취인</span>
                  <span>상품</span>
                  <span>운임</span>
                </div>
                {ss.map((s) => {
                  const { recipient } = shipmentContext(state, s);
                  const nativeShipment = nativeGroup?.shipments.find((row) => row.shipmentId === s.id);
                  // Orange is ONLY committed selection; blue is ONLY the
                  // temporary shipment visible through the hover preview.
                  const visuallyChosen = !inPreview && selection?.shipmentId === s.id;
                  const isPreviewed = inPreview && hovered && hoveredShipmentId === s.id;
                  return (
                    <button
                      key={s.id}
                      data-ui-role="nav-subsection"
                      className={
                        "workflow-shipment" +
                        (visuallyChosen ? " chosen" : "") +
                        (isPreviewed ? " previewed" : "") +
                        (!inPreview && s.id === leaving ? " query-leaving" : "") +
                        (activeKey &&
                        (activeKey.startsWith(recipient.id + ":") ||
                          recipient.items.some((i) => activeKey.startsWith(i.id + ":")))
                          ? " linked-active"
                          : "")
                      }
                      data-shipment-id={s.id}
                      aria-label={"배송 건 " + recipient.name + (isPreviewed ? " (미리보기 표시 중)" : "")}
                      onMouseEnter={inPreview ? () => beginPreviewRowHover(g.id, step, s.id) : undefined}
                      onMouseLeave={inPreview ? () => endPreviewRowHover(g.id, step) : undefined}
                      onClick={() => {
                        if (inPreview) {
                          // An already-selected but manually folded group must
                          // reopen too, not just update the selected shipment.
                          setManuallyClosedGroup(undefined);
                          stopStagePreview();
                        }
                        onSelect({
                          orderId: s.orderId,
                          groupId: g.id,
                          shipmentId: s.id,
                        });
                      }}
                    >
                      <span className="shipment-recipient" title={recipient.name}>
                        <span className="shipment-recipient-name">{recipient.name || "수취인 확인"}</span>
                        {/* Keep the recipient name full-width; the blue row itself
                            indicates temporary display without adding a label. */}
                      </span>
                      <span className="shipment-products">
                        {recipient.items.map((i) => (
                          <span
                            className={
                              "product-chip" +
                              (nativeGroup?.issues.some(
                                (issue) => issue.itemId === i.id && issue.severity === "ACTION",
                              )
                                ? " product-chip-error"
                                : "")
                            }
                            key={i.id}
                            title={i.name}
                          >
                            {internalModel(i.name)} × {i.quantity ?? "?"}
                          </span>
                        ))}
                      </span>
                      <span className="shipment-freight">
                        {nativeShipment?.freightCode === "020"
                          ? "착불"
                          : won(nativeShipment?.totals.shipping ?? null)}
                      </span>
                    </button>
                  );
                })}
              </div>
        );
        const renderGroupCard = (isPreview: boolean) => (
          <article
            className={
              "workflow-group" +
              (isPreview ? " workflow-preview-clone" : "") +
              (selected && !isPreview ? " selected" : "") +
              (isPreview ? " temporary-preview" : "") +
              (hovered && !isPreview ? " workflow-preview-group" : "") +
              (!isPreview && stageHover?.groupId === g.id && stageHover.step === 4
                ? " workflow-payment-hover"
                : "")
            }
            key={g.id}
            data-group-id={g.id}
            data-freight-preview={hovered && stageHover?.step === 2 ? "true" : undefined}
            data-payment-confirm-preview={hovered && stageHover?.step === 4 ? "true" : undefined}
          >
            <header>
              <button
                className="flow-heading-toggle"
                data-ui-role="nav-subsection"
                aria-label={dealerName + ((open || isPreview) ? " 묶음 접기" : " 묶음 펼치기")}
                aria-expanded={open || isPreview}
                onClick={() => {
                  if (isPreview) {
                    setManuallyClosedGroup(undefined);
                    stopStagePreview();
                    onSelect({ orderId: ss[0].orderId, groupId: g.id, shipmentId: ss[0].id });
                  } else if (selected) {
                    setManuallyClosedGroup(manuallyClosedGroup === g.id ? undefined : g.id);
                  } else {
                    setManuallyClosedGroup(undefined);
                    onSelect({
                      orderId: ss[0].orderId,
                      groupId: g.id,
                      shipmentId: ss[0].id,
                    });
                  }
                }}
              >
                <span className="flow-toggle-icon" aria-hidden="true">{(open || isPreview) ? "▾" : "▸"}</span>
                <span className="flow-group-name">
                  <b>{dealerName}</b>
                </span>
                {first.order.orderType !== "RETAIL" && nativeGroup && (
                  <SignalLights step={step} notice={g.needsNotice} />
                )}
              </button>
              <button
                disabled={!ready || (first.order.receptionVersion === 1 && !first.order.confirmedAt)}
                onClick={() => {
                  if (isPreview) stopStagePreview();
                  setManage(g.id);
                }}
              >
                묶음 관리
              </button>
              <button
                className="trash-button"
                disabled={!ready || (first.order.receptionVersion === 1 && !first.order.confirmedAt)}
                aria-label={"주문 묶음 삭제 " + dealerName}
                onClick={async () => {
                  if (isPreview) stopStagePreview();
                  if (
                    busy || !ready ||
                    !(await confirmAction(
                      dealerName +
                        " / 배송 " +
                        ss.length +
                        "건 묶음을 삭제할까요? 접수 원문 / 감사 기록은 보존됩니다.",
                    ))
                  )
                    return;
                  setBusy(true);
                  try {
                    const next = await nativeChangeGroup(state, g.id, "delete_group");
                    await onSave(next);
                    setUndoDelete({
                      gid: g.id,
                      at: next.workflow!.groups.find((x) => x.id === g.id)!.deletedAt!,
                    });
                  } catch (e) {
                    onNotice(e instanceof Error ? e.message : String(e), "error");
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.4"
                  aria-hidden="true"
                >
                  <path d="M3 4h10M6 2h4M4 4l1 10h6l1-10M7 6v6M9 6v6" />
                </svg>
              </button>
            </header>
            <div className="workflow-action-line">
              <div className="flow-fixed-summary" aria-label="발주서 묶음 금액 요약">
                <div className="flow-summary-compact">
                  <span>배송 {ss.length}건</span>
                  <span className="flow-summary-financial">
                    {nativeGroup && vatIncludedCount > 0 && (
                      <span className="flow-summary-vat" title={vatDescription}
                        aria-label={vatDescription}>
                        {mixedVat
                          ? `포함${vatIncludedCount}·별도${vatExcludedCount}`
                          : "부가세 포함"}
                      </span>
                    )}
                    <span className="flow-summary-payment"
                      title={!nativeGroup ? "부가세 계산 확인 중" : vatIncludedCount > 0 ? vatDescription : undefined}>
                      <span className="flow-summary-amount">{won(nativeGroup?.totals.total ?? null)}</span>
                    </span>
                  </span>
                </div>
              </div>
              {first.order.orderType !== "RETAIL" && (
                <div className="workflow-progress">
                <button
                  className="primary"
                  data-ui-role="action-primary"
                  data-preview-hover-active={hovered ? "true" : undefined}
                  disabled={!ready || busy || remaining > 0 || ss.some((s) => s.transferred)}
                  aria-label={buttonLabel}
                  onMouseEnter={isPreview ? keepStagePreview : (event) => startStagePreview(event.currentTarget, g.id, step)}
                  onMouseLeave={isPreview ? undefined : scheduleStagePreviewExit}
                  onClick={async () => {
                    if (isPreview) stopStagePreview();
                    if (
                      (step === 2 || (g.needsNotice && step >= 2)) &&
                      !(await nativePaymentWasCopied(actionState(g.id), g.id)) &&
                      !(await confirmAction(
                        "계좌 안내 메시지를 복사한 기록이 없습니다. 다른 방법으로 안내하셨나요?",
                        { cancel: "돌아가기", accept: "전달 완료로 처리" },
                      ))
                    )
                      return;
                    void run(
                      () => nativeApplyWorkflowAction(
                        actionState(g.id),
                        step === 4 && !g.needsNotice
                          ? "process_group"
                          : step === 3 && !g.needsNotice
                            ? "confirm_group_payment"
                            : "advance_group",
                        { groupId: g.id },
                      ),
                      g.id,
                    );
                  }}
                >
                  {buttonLabel}
                </button>
                <button
                  className="workflow-undo"
                  disabled={!ready || busy || step === 0 || ss.some((s) => s.transferred)}
                  title="이전 단계 취소"
                  aria-label="이전 단계 취소"
                  onClick={() => {
                    if (isPreview) stopStagePreview();
                    void run(() =>
                      nativeApplyWorkflowAction(actionState(g.id), "undo_group", { groupId: g.id }));
                  }}
                >
                  ↶
                </button>
                </div>
              )}
            </div>
            {(g.needsNotice || ss.some((s) => s.rechecks?.some((c) => !c.resolvedAt))) && (
              <div className="flow-recheck">
                <b>정정 후 재확인</b>
                {ss.flatMap(
                  (s) =>
                    s.rechecks
                      ?.filter((c) => !c.resolvedAt)
                      .slice(-1)
                      .map((c, n) => (
                        <small key={s.id + n}>
                          {shipmentContext(state, s).recipient.name} / {c.reason} / 이전{" "}
                          {c.fromStage}단계 이력 보존
                        </small>
                      )) ?? [],
                )}
                {g.needsNotice && (
                  <span>
                    금액 재안내 필요 / 이전 {won(g.notices.at(-1)?.total ?? null)} → 현재{" "}
                    {won(nativeGroup?.totals.total ?? null)} / 차액{" "}
                    {nativeGroup?.noticeDelta != null
                      ? won(nativeGroup.noticeDelta)
                      : "확인 필요"}
                  </span>
                )}
              </div>
            )}
            {(open || isPreview) && renderShipmentList(isPreview)}
          </article>
        );
        const showPreview = hovered && !open && previewLayer?.groupId === g.id;
        return (
          <div className={"workflow-card-anchor" + (showPreview ? " previewing" : "")} key={g.id}>
            {renderGroupCard(false)}
            {showPreview && (
              <div
                className="workflow-card-overlay"
                role="group"
                aria-label={dealerName + " 펼침 미리보기"}
                onMouseEnter={keepStagePreview}
                onMouseLeave={scheduleStagePreviewExit}
              >
                {renderGroupCard(true)}
              </div>
            )}
          </div>
        );
      })}
      {undoDelete && (
        <div className="delete-undo" role="status">
          주문 묶음을 삭제했습니다.{" "}
          <button
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                if (
                  state.workflow?.groups.find((g) => g.id === undoDelete.gid)?.deletedAt !==
                  undoDelete.at
                )
                  throw Error("직전 삭제 상태가 변경되었습니다.");
                await onSave(await nativeChangeGroup(state, undoDelete.gid, "restore_group"));
                setUndoDelete(undefined);
              } catch (e) {
                onNotice(e instanceof Error ? e.message : String(e), "error");
              } finally {
                setBusy(false);
              }
            }}
          >
            실행 취소
          </button>
        </div>
      )}
      {pendingManage && pendingReceptions.some((row) => row.id === pendingManage) && (
        <Modal title="배송 건 묶음 관리" onClose={() => setPendingManage(undefined)}>
          <p>거래처를 확인해야 실제 배송 묶음 및 업무 이력이 생성됩니다.</p>
          <p>거래처 지정 후 배송 건 분리와 다시 묶기 기능을 사용할 수 있습니다.</p>
          {pendingReceptions.find((row) => row.id === pendingManage)!.recipients.map((recipient) => (
            <div className="flow-manage-row" key={recipient.id}>
              <span>{recipient.name || "수취인 확인"} / 상품 {recipient.items.length}종</span>
            </div>
          ))}
          <button type="button" className="primary" onClick={() => {
            onSelectPending?.(pendingManage);
            setPendingManage(undefined);
          }}>거래처 확인하기</button>
        </Modal>
      )}
      {manage && (
        <GroupManagement
          state={actionState(manage)}
          groupId={manage}
          onClose={() => setManage(undefined)}
          onSave={async (s) => {
            await onSave(s);
            setManage(undefined);
          }}
          onNotice={onNotice}
        />
      )}
    </div>
  );
}

function GroupManagement({
  state,
  groupId,
  onClose,
  onSave,
  onNotice,
}: {
  state: State;
  groupId: string;
  onClose: () => void;
  onSave: (s: State) => Promise<void>;
  onNotice: NoticeSink;
}) {
  const native = useNativeWorkflowBoard(state);
  const ss = groupShipments(state, groupId),
    g = state.workflow!.groups.find((g) => g.id === groupId)!,
    [target, setTarget] = useState(""),
    [ack, setAck] = useState(false),
    [busy, setBusy] = useState(false);
  const targets = state.workflow!.groups.filter(
    (t) =>
      t.id !== g.id &&
      t.sourceId === g.sourceId &&
      t.dealerId === g.dealerId &&
      groupShipments(state, t.id).some((s) => !s.transferred),
  );
  const run = async (fn: () => Promise<State> | State) => {
    setBusy(true);
    try {
      await onSave(await fn());
    } catch (e) {
      onNotice(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="배송 건 묶음 관리" onClose={onClose}>
      <p>
        발주{" "}
        {String(state.workflow!.sources.find((s) => s.id === g.sourceId)!.number).padStart(3, "0")}{" "}
 / 현재 {ss.length}건 / {won(native.byId[groupId]?.totals.total ?? null)}
      </p>
      {ss.map((s) => (
        <div className="flow-manage-row" key={s.id}>
          <span>
            {shipmentContext(state, s).recipient.name} / {s.step}/4 완료 / 입금{" "}
            {won(native.byId[groupId]?.shipments.find((row) => row.shipmentId === s.id)?.paid ?? null)}
          </span>
          <button
            disabled={!native.ready || busy || ss.length < 2 || s.transferred}
            onClick={() => void run(() => nativeApplyWorkflowAction(state, "detach_shipment", { shipmentId: s.id }))}
          >
            이 배송 건 분리
          </button>
        </div>
      ))}
      {targets.length > 0 && (
        <>
          <Field label="다시 묶을 대상">
            <select value={target} onChange={(e) => setTarget(e.target.value)}>
              <option value="">같은 최초 발주서의 묶음 선택</option>
              {targets.map((t) => (
                <option key={t.id} value={t.id}>
                  {groupShipments(state, t.id)
                    .map((s) => shipmentContext(state, s).recipient.name)
                    .join(", ")}{" "}
 / {native.byId[t.id]?.step ?? "—"}/4 / {won(native.byId[t.id]?.totals.total ?? null)}
                </option>
              ))}
            </select>
          </Field>
          <label className="check">
            <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} /> 기존
            안내액 / 입금 배분 / 변경 합계를 확인했습니다. 합친 뒤 필요하면 다시 안내합니다.
          </label>
          {ss.map((s) => (
            <button
              key={s.id}
              disabled={!native.ready || busy || !target || s.transferred}
              onClick={() => void run(() => nativeApplyWorkflowAction(state, "regroup_shipment", { shipmentId: s.id, targetGroupId: target, ack }))}
            >
              {shipmentContext(state, s).recipient.name} 다시 묶기
            </button>
          ))}
        </>
      )}
      <p className="muted">
        배송 건 ID / 완료 이력 / 입금 배분은 그대로 보존됩니다. 단계나 계좌가 다른 묶음, 엑셀 이관 건은
        합치지 않습니다.
      </p>
    </Modal>
  );
}

/** Shallow props equality is safe because HubWorkspace passes stable event wrappers. */
export const MemoWorkflowBoard = memo(WorkflowBoard);
