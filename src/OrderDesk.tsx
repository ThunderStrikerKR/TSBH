import { memo, useEffect, useRef, useState } from "react";
import { auditReactRender } from "./perf-audit";
import { CorrectionPanel } from "./CorrectionPanel";
import { ActualFreightField } from "./ActualFreightField";
import { useNativeDeliveryRequirements } from "./useNativeDeliveryRequirements";
import { IssueResolver } from "./IssueResolver";
import { LinkContext, Linked } from "./LinkedText";
import { OrderItemEditor } from "./OrderItemEditor";
import { ProductSearchDialog } from "./ProductSearchDialog";
import { SourceDocument } from "./SourceDocument";
import { StaffChecks } from "./StaffChecks";
import { WorkflowFooter, WorkflowFreightRows } from "./WorkflowFooter";
import { won } from "./domain";

import { internalModel } from "./matching";
import { useOrderDesk, type OrderDeskProps } from "./orders/useOrderDesk";
import { nativeApproveMissingQuantity, nativeConfirmProposedProduct, nativeReconcileWorkflow } from "./orders/native-rules";
import { businessCoreCall } from "./tauri-bridge";
import type { Order } from "./types";
import { Field, clone } from "./ui";
export function OrderDesk(props: OrderDeskProps) {
  auditReactRender("OrderDesk:" + props.viewMode);
  const [changeRequest, setChangeRequest] = useState<string>();
  const [approvingQuantity, setApprovingQuantity] = useState(false);
  const quantityActionInFlight = useRef(false);
  const {
    highlight,
    linkController,
    setHighlight,
    viewMode,
    place,
    state,
    draft,
    source,
    client,
    setSource,
    saved,
    onChange,
    exported,
    onWorkflowSave,
    onSaveState,
    onNotice,
    portalTargets,
    staffBlocks,
    selectIssue,
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
    staffChecksReady,
    staffChecksActionsReady,
    staffChecksError,
    flowSummaryError,
    orderPreview,
    orderLineAmounts,
    orderLineError,
    setProductSearch,
    activeGroupId,
    run,
    resolving,
    setResolving,
    productSearch,
    chooseProduct,
    itemEditing,
  } = useOrderDesk(props);
  // Do not clear/recreate the "확인할 내용" list while multiple Rust
  // previews settle after a paste. Old blocks are read-only until current.
  const lastReadyChecks = useRef<{ orderId: string; blocks: typeof staffBlocks } | undefined>(undefined);
  if (staffChecksReady && !staffChecksError) {
    lastReadyChecks.current = { orderId: draft.id, blocks: staffBlocks };
  }
  const shownChecks = staffChecksReady ? staffBlocks
    : lastReadyChecks.current?.orderId === draft.id
      ? lastReadyChecks.current.blocks : staffBlocks;
  // Rust previews become briefly pending after each keystroke. Keep the
  // *input element* mounted; the last resolved projection is for display only.
  // The edited amount always comes from the current React draft below, and
  // final business validation remains in Rust.
  const lastReadyFreight = useRef<{
    orderId: string;
    recipientId: string;
    shipment: NonNullable<typeof activeShipment>;
    preview: NonNullable<typeof activeShipmentPreview>;
    state: typeof flowState;
  } | null>(null);
  if (flowSummaryReady && activeShipment && activeShipmentPreview) {
    lastReadyFreight.current = {
      orderId: draft.id,
      recipientId: activeShipment.recipientId,
      shipment: activeShipment,
      preview: activeShipmentPreview,
      state: flowState,
    };
  }
  const freightEditor = lastReadyFreight.current?.orderId === draft.id &&
    lastReadyFreight.current.recipientId === shownRecipient &&
    (!props.workflowSelection?.shipmentId ||
      props.workflowSelection.shipmentId === lastReadyFreight.current.shipment.id)
      ? lastReadyFreight.current : null;
  const delivery = useNativeDeliveryRequirements(draft.recipients, draft.receptionVersion !== 1);
  const addressRequests = useRef<Record<string, number>>({});
  const addressMounted = useRef(true);
  useEffect(() => {
    addressMounted.current = true;
    return () => { addressMounted.current = false; };
  }, []);
  const applyNativeAddress = (recipientId: string, value: string) => {
    const token = (addressRequests.current[recipientId] ?? 0) + 1;
    addressRequests.current[recipientId] = token;
    const sourceOrderId = draft.id;
    void businessCoreCall<{
      valid: boolean;
      addressParts: NonNullable<Order["recipients"][number]["addressParts"]>;
      addressStatus: "COMPLETE" | "INCOMPLETE";
    }>("rules", "assess_delivery_address", { value })
      .then(({ addressParts, addressStatus }) => {
        if (!addressMounted.current ||
            addressRequests.current[recipientId] !== token) return;
        onChange((previous) => {
          if (previous.id !== sourceOrderId) return previous;
          const original = previous.recipients.find((r) => r.id === recipientId);
          if (!original || original.address !== value) return previous;
          const next = structuredClone(previous);
          const row = next.recipients.find((r) => r.id === recipientId)!;
          row.addressParts = addressParts;
          row.addressStatus = addressStatus;
          return next;
        });
      }).catch((error) => {
        if (addressMounted.current && addressRequests.current[recipientId] === token)
          onNotice("Rust 배송 주소 확인 실패: " +
            (error instanceof Error ? error.message : String(error)), "error");
      });
  };
  if (draft.orderType === "RETAIL")
    return (
      <>
        {viewMode !== "reception" &&
          place(
            <section className="desk-pane review-pane retail-summary">
              <header className="pane-heading">
                <h2>소매 주문 / 반도스탠드</h2>
              </header>
              <div className="retail-order-body">
                {draft.recipients.map((r) => (
                  <article key={r.id}>
                    <h3>{r.name}</h3>
                    <p>{r.address}</p>
                    <p>{r.mobile || r.phone}</p>
                    <p>{r.note}</p>
                    <h3>품목 및 운임</h3>
                    {r.items.map((i) => (
                      <p key={i.id}>
                        {i.name} — {i.color} × {i.quantity}
                      </p>
                    ))}
                    <p>출고지: {r.dispatchOrigin === "STORE" ? "매장" : "공장"}</p>
                    <p>
                      실제 출고 운임:{" "}
                      {r.actualFreight === null || r.actualFreight === undefined
                        ? "회신 확인 필요"
                        : won(r.actualFreight)}
                    </p>
                  </article>
                ))}
                {state.workflow?.shipments
                  .filter((s) => s.orderId === draft.id)
                  .map(
                    (ship) =>
                      draft.recipients.find((r) => r.id === ship.recipientId)?.dispatchOrigin && (
                        <ActualFreightField
                          key={ship.id}
                          state={state}
                          shipmentId={ship.id}
                          origin={
                            draft.recipients.find((r) => r.id === ship.recipientId)!.dispatchOrigin!
                          }
                          onSave={onWorkflowSave}
                        />
                      ),
                  )}
                <p>송하인: {draft.senderSnapshot?.name}</p>
              </div>
            </section>,
            portalTargets?.details,
          )}
      </>
    );
  return (
    <LinkContext.Provider
      value={{
        active: highlight,
        recipientId: linkController?.recipientId,
        setActive: setHighlight,
      }}
    >
      {changeRequest && (
        <CorrectionPanel
          state={state}
          sid={changeRequest}
          onSave={onWorkflowSave}
          onClose={() => setChangeRequest(undefined)}
          onNotice={onNotice}
        />
      )}
      <div className="desk-grid">
        {viewMode !== "details" &&
          place(
            <SourceDocument
              state={state}
              onManual={props.onManual}
              onSettings={props.onSettings}
              order={draft}
              text={source}
              client={client}
              onText={(value) => {
                setSource(value);
              }}
              onOrder={onChange}
              onDealer={async (c) => {
                if (c.id === draft.clientId) return;
                const sourceDraft = draft;
                const sourceState = state;
                // Rust validates historical payment/transfer constraints,
                // selects the dealer, snapshots old invoices, reprices products
                // and invalidates the old dealer's freight atomically.
                const { order: next } = await businessCoreCall<{ order: Order }>(
                  "rules", "order_edit_rules", {
                    action: "change_order_client", state: sourceState,
                    order: sourceDraft, dealerId: c.id,
                    now: new Date().toISOString(),
                  },
                );
                if (!isCurrentOrderInput(sourceDraft, sourceState))
                  throw Error("주문 또는 거래처 정보가 변경되었습니다. 다시 선택하세요.");
                if (saved) {
                  // Reconciliation is committed once. Read back the SQLite
                  // authority: useAppModel can intentionally retain a current
                  // unsaved editor even when an unrelated workflow saves.
                  await onWorkflowSave(await nativeReconcileWorkflow(sourceState, next));
                  const persisted = await window.bando!.load();
                  const committed = persisted.orders.find((row) => row.id === next.id);
                  if (!committed)
                    throw Error("저장된 거래처 변경 주문을 다시 확인할 수 없습니다.");
                  onChange((current) => current === sourceDraft ? committed : current);
                } else {
                  onChange(next);
                }
              }}
              onSave={onWorkflowSave}
              onSaveState={onSaveState}
              onNotice={onNotice}
            />,
            portalTargets?.source,
          )}
        {viewMode !== "reception" &&
          place(
            <section className="desk-pane review-pane compact-review">
              <div className="review-layout">
                <StaffChecks
                  blocks={shownChecks}
                  loading={!staffChecksReady}
                  interactive={staffChecksActionsReady}
                  error={staffChecksError}
                  onSelect={selectIssue}
                  approvingQuantity={approvingQuantity}
                  canApproveOne={(b) => !exported && staffChecksActionsReady && b.canApproveOne === true}
                  onApproveOne={(b) => {
                    if (quantityActionInFlight.current || exported || !staffChecksActionsReady ||
                        b.canApproveOne !== true || !b.itemId || !b.recipientId) return;
                    quantityActionInFlight.current = true;
                    setApprovingQuantity(true);
                    void run(async () => {
                      try {
                        const approved = await nativeApproveMissingQuantity(
                          draft, b.recipientId, b.itemId!,
                          state.settings.operatorName?.trim() || "사용자",
                        );
                        if (!isCurrentOrderInput(draft, state))
                          throw Error("주문 정보가 변경되었습니다. 수량 확인을 다시 진행하세요.");
                        const source = approved.recipients.find((r) => r.id === b.recipientId)
                          ?.items.find((i) => i.id === b.itemId);
                        if (!source?.quantityApproval) throw Error("잠정 수량 승인 기록이 없습니다.");
                        // Preserve the normal order edit's price-history and
                        // freight invalidation, without running TS approval rules.
                        recEdit(b.recipientId, (recipient) => {
                          const target = recipient.items.find((i) => i.id === b.itemId);
                          if (!target) return;
                          target.quantity = source.quantity;
                          target.quantityConfirmed = source.quantityConfirmed;
                          target.quantityApproval = source.quantityApproval;
                          target.manuallyConfirmed = source.manuallyConfirmed;
                        });
                      } finally {
                        quantityActionInFlight.current = false;
                        setApprovingQuantity(false);
                      }
                    });
                  }}
                  onCancelApproval={(b) => {
                    if (b.itemId)
                      recEdit(
                        b.recipientId,
                        (r) => {
                          const i = r.items.find((i) => i.id === b.itemId);
                          if (i) delete i.quantityApproval;
                        },
                        false,
                      );
                  }}
                  onCandidate={(b, value) => {
                    if (b.kind === "OPTION") {
                      const row = draft.recipients.find((r) => r.id === b.recipientId);
                      const item = row?.items.find((i) => i.id === b.itemId);
                      const product = products.find((p) => p.id === item?.productId);
                      const variant = product?.variants.find((v) => v.id === value);
                      if (!item || !product || !variant) return;
                      void run(async () => {
                        const { unitPrice } = await businessCoreCall<{ unitPrice: number | null }>(
                          "rules", "resolve_unit_price", {
                            product, variantId: variant.id, client: pricingClient,
                          },
                        );
                        if (!isCurrentOrderInput(draft, state)) return;
                        recEdit(b.recipientId, (r) => {
                          const current = r.items.find((i) => i.id === b.itemId);
                          if (!current || current.productId !== product.id) return;
                          current.variantId = variant.id;
                          current.color = variant.color;
                          current.url = variant.url;
                          current.unitPrice = unitPrice;
                          current.issues = current.issues.filter((x) => !/색상|옵션/.test(x));
                          current.manuallyConfirmed = true;
                          delete current.quantityApproval;
                        }, false);
                      });
                      return;
                    }
                    recEdit(b.recipientId, (r) => {
                      const field = b.kind?.slice(10);
                      if (field && ["name", "address", "phone", "mobile"].includes(field)) {
                        (r as unknown as Record<string, unknown>)[field] = value;
                        r.confirmedFields = [...new Set([...(r.confirmedFields ?? []), field])];
                        if (field === "address") {
                          delete r.addressParts;
                          r.addressStatus = "INCOMPLETE";
                        }
                      }
                    }, false);
                    const field = b.kind?.slice(10);
                    if (field === "address") applyNativeAddress(b.recipientId, value);
                  }}
                />
                <div className="recipient-body">
                  <header className="recipient-section-heading">
                    <h2>수취인별 주문 정보</h2>
                    {exported &&
                      activeShipment &&
                      state.workflow?.queue.some(
                        (q) =>
                          q.shipmentId === activeShipment.id &&
                          ["EXPORTED", "STORE_SENT"].includes(q.status),
                      ) && (
                        <button onClick={() => setChangeRequest(activeShipment.id)}>
                          전달 후 변경 요청
                        </button>
                      )}
                  </header>
                  <div className="receipt-panel-content">
                    {!draft.recipients.length && (
                      <p className="empty detail-empty">발주 원문을 입력하거나 배송 건을 선택하세요.</p>
                    )}
                    {draft.recipients
                      .filter((r) => r.id === shownRecipient)
                      .map((r) => (
                        <article className="recipient-review" key={r.id}>
                          <div className="recipient-fields">
                            <Linked linkKey={r.id + ":name"}>
                              <Field label="이름">
                                <input
                                  aria-invalid={delivery.ready ? !delivery.byId[r.id]?.name : undefined}
                                  value={r.name}
                                  disabled={exported}
                                  onChange={(e) =>
                                    recEdit(
                                      r.id,
                                      (r) => {
                                        r.name = e.target.value;
                                        r.confirmedFields = [
                                          ...new Set([...(r.confirmedFields ?? []), "name"]),
                                        ];
                                      },
                                      false,
                                    )
                                  }
                                />
                              </Field>
                            </Linked>
                            <Linked linkKey={r.id + ":address"}>
                              <Field label="주소">
                                <input
                                  aria-invalid={delivery.ready ? !delivery.byId[r.id]?.address : undefined}
                                  value={r.address}
                                  disabled={exported}
                                  onChange={(e) => {
                                    recEdit(
                                      r.id,
                                      (r) => {
                                        r.address = e.target.value;
                                        delete r.addressParts;
                                        r.confirmedFields = [
                                          ...new Set([...(r.confirmedFields ?? []), "address"]),
                                        ];
                                        // Never presume a typed address is deliverable before Rust checks it.
                                        r.addressStatus = "INCOMPLETE";
                                      },
                                      false,
                                    );
                                    applyNativeAddress(r.id, e.target.value);
                                  }}
                                />
                              </Field>
                            </Linked>
                            <div className="recipient-phones">
                              <Linked linkKey={r.id + ":mobile"}>
                                <Field label="휴대폰">
                                  <input
                                    aria-invalid={delivery.ready ? !delivery.byId[r.id]?.contact : undefined}
                                    value={r.mobile}
                                    disabled={exported}
                                    onChange={(e) =>
                                      recEdit(
                                        r.id,
                                        (r) => {
                                          r.mobile = e.target.value;
                                          r.confirmedFields = [
                                            ...new Set([...(r.confirmedFields ?? []), "mobile"]),
                                          ];
                                        },
                                        false,
                                      )
                                    }
                                  />
                                  {!delivery.byId[r.id]?.contact &&
                                    delivery.byId[r.id]?.mobileProblem && (
                                      <small className="form-error">
                                        {delivery.byId[r.id].mobileProblem}
                                      </small>
                                    )}
                                </Field>
                              </Linked>
                              <Linked linkKey={r.id + ":phone"}>
                                <Field label="전화">
                                  <input
                                    aria-invalid={delivery.ready ? !delivery.byId[r.id]?.contact : undefined}
                                    value={r.phone}
                                    disabled={exported}
                                    onChange={(e) =>
                                      recEdit(
                                        r.id,
                                        (r) => {
                                          r.phone = e.target.value;
                                          r.confirmedFields = [
                                            ...new Set([...(r.confirmedFields ?? []), "phone"]),
                                          ];
                                        },
                                        false,
                                      )
                                    }
                                  />
                                  {!delivery.byId[r.id]?.contact &&
                                    delivery.byId[r.id]?.phoneProblem && (
                                      <small className="form-error">
                                        {delivery.byId[r.id].phoneProblem}
                                      </small>
                                    )}
                                </Field>
                              </Linked>
                            </div>
                            <Linked linkKey={r.id + ":note"}>
                              <Field label="요청 메시지">
                                <input
                                  value={r.note}
                                  disabled={exported}
                                  onChange={(e) =>
                                    recEdit(
                                      r.id,
                                      (r) => {
                                        r.note = e.target.value;
                                        r.confirmedFields = [
                                          ...new Set([...(r.confirmedFields ?? []), "note"]),
                                        ];
                                      },
                                      false,
                                    )
                                  }
                                />
                              </Field>
                            </Linked>
                          </div>
                        </article>
                      ))}
                    {!!draft.replies?.length && (
                      <details className="reply-history">
                        <summary>최초 주문 이후 회신 {draft.replies.length}건</summary>
                        {draft.replies.map((r) => (
                          <p key={r.id}>{r.raw}</p>
                        ))}
                      </details>
                    )}
                  </div>
                </div>
                <section className="items-freight-panel">
                  <header className="recipient-section-heading">
                    <h2>품목 및 운임</h2>
                  </header>
                  <div className="items-freight-scroll receipt-panel-content">
                    {draft.recipients
                      .filter((r) => r.id === shownRecipient)
                      .map((r) => (
                        <article key={r.id}>
                          <table className="compact-items">
                            <thead>
                              <tr>
                                <th>품목</th>
                                <th>수량</th>
                                <th>금액</th>
                              </tr>
                            </thead>
                            <tbody>
                              {r.items.map((i) => (
                                <tr key={i.id} data-item-id={i.id}>
                                  <td>
                                    <Linked linkKey={i.id + ":product"}>
                                      <button
                                        className="item-row-edit"
                                        aria-invalid={!i.productId || !i.variantId}
                                        onClick={() => setItemEditing({ rid: r.id, iid: i.id })}
                                        aria-label={"상품 수정 " + i.name}
                                      >
                                        {internalModel(i.name)}
                                      </button>
                                    </Linked>
                                  </td>
                                  <td>
                                    <Linked linkKey={i.id + ":quantity"}>
                                      <input
                                        type="number"
                                        aria-label={"수량 " + i.name}
                                        aria-invalid={
                                          i.quantityConfirmed === false ||
                                          !Number.isSafeInteger(i.quantity) ||
                                          (i.quantity ?? 0) < 1
                                        }
                                        min="1"
                                        step="1"
                                        disabled={exported}
                                        value={i.quantity ?? ""}
                                        onChange={(e) => {
                                          const q =
                                            e.target.value === "" ? null : Number(e.target.value);
                                          if (q !== null && (!Number.isSafeInteger(q) || q < 1)) return;
                                          itemEdit(r.id, i.id, (item) => {
                                            item.quantity = q;
                                            item.quantityConfirmed = true;
                                            item.issues = item.issues.filter(
                                              (x) => !x.includes("수량"),
                                            );
                                          });
                                        }}
                                      />
                                    </Linked>
                                  </td>
                                  <td>
                                    <Linked linkKey={i.id + ":price"}>
                                      <button
                                        className="item-row-edit item-amount"
                                        onClick={() => setItemEditing({ rid: r.id, iid: i.id })}
                                      >
                                        {orderLineAmounts
                                          ? won(orderLineAmounts.find(
                                              (entry) => entry.recipientId === r.id &&
                                                entry.itemId === i.id,
                                            )?.amount ?? null)
                                          : "—"}
                                      </button>
                                    </Linked>
                                  </td>
                                </tr>
                              ))}
                              {freightEditor?.recipientId === r.id && (
                                <WorkflowFreightRows
                                  state={freightEditor.state}
                                  shipment={freightEditor.shipment}
                                  freightCode={r.freightCode ?? freightEditor.preview.freightCode}
                                  displayCharges={{
                                    ...freightEditor.preview.displayCharges,
                                    ...r.shippingCharges,
                                  }}
                                  freightEditingBlocked={
                                    freightEditor.preview.freightEditingBlocked || r.freightCode === "020"
                                  }
                                  activeKey={highlight}
                                  onFreight={(origin, value) =>
                                    recEdit(
                                      r.id,
                                      (row) => {
                                        row.shippingCharges = {
                                          ...freightEditor.shipment.charges,
                                          ...row.shippingCharges,
                                          [origin]: value,
                                        };
                                      },
                                      false,
                                    )
                                  }
                                  onCode={(v) => recEdit(r.id, (row) => (row.freightCode = v), false)}
                                />
                              )}
                            </tbody>
                          </table>
                          <div className="recipient-row-actions">
                            <button
                              disabled={exported}
                              onClick={() => setProductSearch({ rid: r.id })}
                            >
                              ＋ 상품 추가
                            </button>
                            <div className="dispatch-origin" role="group" aria-label="발송지">
                              <span className="dispatch-origin-label">발송지:</span>
                              {(["FACTORY", "STORE"] as const).map((origin) => (
                                <button
                                  key={origin}
                                  disabled={exported}
                                  aria-pressed={
                                    (r.dispatchOrigin ??
                                      (r.items.every((i) => i.origin === "STORE")
                                        ? "STORE"
                                        : r.items.every((i) => i.origin === "FACTORY")
                                          ? "FACTORY"
                                          : undefined)) === origin
                                  }
                                  onClick={() =>
                                    recEdit(
                                      r.id,
                                      (row) => {
                                        const previous =
                                          row.dispatchOrigin ?? row.items[0]?.origin ?? "FACTORY";
                                        row.dispatchOrigin = origin;
                                        row.items.forEach((i) => (i.origin = origin));
                                        if (row.shippingCharges)
                                          row.shippingCharges = {
                                            [origin]: row.shippingCharges[previous] ?? null,
                                          };
                                      },
                                      false,
                                    )
                                  }
                                >
                                  {origin === "FACTORY" ? "공장" : "매장"}
                                </button>
                              ))}
                            </div>
                          </div>
                        </article>
                      ))}
                    {!(activeShipment && activeGroupId) && (
                      <div className="pending-shipping">
                        거래처 선택 후 운임과 청구금액을 확인합니다.
                      </div>
                    )}
                  </div>
                  {(flowSummaryReady && activeShipmentPreview && activeShipment && activeGroupId) || freightEditor ? (
                    <WorkflowFooter
                      state={flowState}
                      shipment={flowSummaryReady && activeShipment ? activeShipment : freightEditor!.shipment}
                      vatIncluded={flowSummaryReady && activeShipmentPreview
                        ? activeShipmentPreview.vatIncluded : freightEditor!.preview.vatIncluded}
                      groupId={flowSummaryReady && activeGroupId
                        ? activeGroupId : freightEditor!.shipment.groupId}
                      unavailable={!flowSummaryReady || !activeShipmentPreview || !activeShipment || !activeGroupId}
                      onVat={(v) => {
                        const recipientId = (flowSummaryReady && activeShipment
                          ? activeShipment : freightEditor!.shipment).recipientId;
                        recEdit(recipientId, (row) => (row.vatOverride = v), false);
                      }}
                    />
                  ) : null}
                </section>
              </div>
              {flowSummaryError && (
                <p role="alert">주문 미리보기 계산 오류: {flowSummaryError}</p>
              )}
              {orderLineError && (
                <p role="alert">Rust 품목 금액 조회 오류: {orderLineError}</p>
              )}
              <div className="order-grand-total">
                총 주문 금액{" "}
                <span className="shipment-count">
                  | 택배{" "}
                  {orderPreview?.shipmentCount ?? "—"}
                  건 |
                </span>
                <b>
                  {orderPreview?.total !== null && orderPreview?.total !== undefined
                    ? won(orderPreview.total) : "—"}
                </b>
                <small>
                  (
                  {orderPreview?.vatLabel ?? "—"}
                  )
                </small>
              </div>
            </section>,
            portalTargets?.details,
          )}
        {resolving &&
          (() => {
            const r = draft.recipients.find((r) => r.id === resolving.rid),
              i = r?.items.find((i) => i.id === resolving.iid);
            return r && i ? (
              <IssueResolver
                state={state}
                order={draft}
                recipient={r}
                item={i}
                onClose={() => setResolving(undefined)}
                onRemove={() => {
                  recEdit(r.id, (r) => (r.items = r.items.filter((x) => x.id !== i.id)));
                  setResolving(undefined);
                }}
                onNotice={onNotice}
                onApply={async (next, order) => {
                  if (JSON.stringify(next.aliases) !== JSON.stringify(state.aliases))
                    await onSaveState(next);
                  if (
                    order.recipients
                      .find((r) => r.id === resolving.rid)
                      ?.items.find((i) => i.id === resolving.iid)?.modelProposal?.required
                  ) {
                    order.requestedIssueIds = [
                      ...new Set([
                        ...(order.requestedIssueIds ?? []),
                        ["issue", order.id, resolving.rid, resolving.iid, "MODEL_CONFIRM"].join(
                          ":",
                        ),
                      ]),
                    ];
                  }
                  onChange(order);
                }}
              />
            ) : null;
          })()}
        {productSearch && (
          <ProductSearchDialog
            state={state}
            clientId={draft.clientId}
            onClose={() => setProductSearch(undefined)}
            onChoose={(p) => chooseProduct(productSearch.rid, p, productSearch.iid)}
          />
        )}
        {itemEditing &&
          !productSearch &&
          (() => {
            const r = draft.recipients.find((r) => r.id === itemEditing.rid),
              i = r?.items.find((i) => i.id === itemEditing.iid);
            return r && i ? (
              <OrderItemEditor
                state={state}
                item={i}
                client={pricingClient}
                locked={exported}
                onClose={() => setItemEditing(undefined)}
                onApply={(next) => itemEdit(r.id, i.id, (item) => Object.assign(item, next))}
                onRemove={() => {
                  recEdit(r.id, (r) => (r.items = r.items.filter((x) => x.id !== i.id)));
                  setItemEditing(undefined);
                }}
                onConfirm={() =>
                  void run(async () => {
                    const result = await nativeConfirmProposedProduct(state, draft, r.id, i.id);
                    if (JSON.stringify(result.state.aliases) !== JSON.stringify(state.aliases))
                      await onSaveState(result.state);
                    onChange(result.order);
                    setItemEditing(undefined);
                  })
                }
              />
            ) : null;
          })()}
      </div>
    </LinkContext.Provider>
  );
}

/** Shallow props equality is safe because HubWorkspace passes stable event wrappers. */
export const MemoOrderDesk = memo(OrderDesk);
