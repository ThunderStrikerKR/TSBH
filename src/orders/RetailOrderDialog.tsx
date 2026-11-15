import { useEffect, useRef, useState } from "react";
import { sameValue } from "../shared/draft";
import { confirmDiscardDraft } from "../shared/editor-flow";
import { Modal, Field, MoneyInput } from "../ui";
import { businessCoreCall } from "../tauri-bridge";
import { contactProblem } from "../contact";
import { ProductSearchDialog } from "../ProductSearchDialog";
import { nativeConfirmRetail } from "./native-rules";
import type { Item, Order, State } from "../types";
import type { NativeDeliveryRequirement } from "../useNativeDeliveryRequirements";

type RetailPreflight = {
  delivery: NativeDeliveryRequirement[];
  senderIssues: string[];
  issues: string[];
};
export function RetailOrderDialog({
  state,
  onSave,
  onClose,
  initialOrder,
  onDraft,
  onSettings,
}: {
  state: State;
  onSave: (s: State) => Promise<void>;
  onClose: () => void;
  initialOrder?: Order;
  onDraft?: (o: Order) => void;
  onSettings?: () => void;
}) {
  const [order, setOrder] = useState<Order | undefined>(initialOrder),
    [search, setSearch] = useState(false),
    [attempted, setAttempted] = useState(false),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const initial = useRef<Order | undefined>(
    initialOrder ? structuredClone(initialOrder) : undefined,
  );
  const [preflight, setPreflight] = useState<{
    fingerprint: string;
    result: RetailPreflight;
  }>();
  const [preflightFailure, setPreflightFailure] = useState<{
    fingerprint: string;
    error: string;
  }>();
  const addingProduct = useRef(false);
  const alive = useRef(true);
  const draftIds = useRef({
    orderId: crypto.randomUUID(),
    recipientId: crypto.randomUUID(),
  });
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; };
  }, []);
  useEffect(() => {
    if (initialOrder) return;
    let active = true;
    void businessCoreCall<{ order: Order }>("rules", "apply_retail_action", {
      action: "new_retail_order",
      orderId: draftIds.current.orderId,
      recipientId: draftIds.current.recipientId,
      now: new Date().toISOString(),
    }).then(({ order: created }) => {
      if (!active || !alive.current) return;
      initial.current = structuredClone(created);
      setOrder(created);
    }).catch((cause) => {
      if (active && alive.current) setError(String(cause));
    });
    return () => { active = false; };
  }, [initialOrder]);
  useEffect(() => { if (order) onDraft?.(order); }, [order, onDraft]);

  // Current eligibility is projected in Rust from the same source rules that
  // ultimately approve or reject the retail SQLite transaction. Stale IPC
  // responses cannot unlock the UI; there is no TS business-rule fallback.
  const fingerprint = order
    ? JSON.stringify([order, state.settings, state.products])
    : "";
  useEffect(() => {
    if (!order) return;
    let active = true;
    void businessCoreCall<RetailPreflight>("rules", "apply_retail_action", {
      action: "retail_preflight", state, draft: order,
    }).then((result) => {
      if (!active) return;
      setPreflight({ fingerprint, result });
      setPreflightFailure(undefined);
    }).catch((cause) => {
      if (!active) return;
      setPreflight(undefined);
      setPreflightFailure({ fingerprint, error: String(cause) });
    });
    return () => { active = false; };
  }, [fingerprint]);
  const current = preflight?.fingerprint === fingerprint ? preflight.result : undefined;
  const nativeError = preflightFailure?.fingerprint === fingerprint
    ? preflightFailure.error : "";
  const closeDraft = () => {
    if (!order) { onClose(); return; }
    void confirmDiscardDraft(
      "직접 입력 주문", !sameValue(order, initial.current), onClose, busy,
    );
  };
  if (!order || !order.recipients[0]) {
    return (
      <Modal title="직접 입력" busy={busy} onClose={onClose}>
        <div className="retail-order-dialog">
          <p role="status">{error || "소매 주문 초안을 준비하고 있습니다."}</p>
          <button type="button" onClick={onClose}>닫기</button>
        </div>
      </Modal>
    );
  }
  const row = order.recipients[0],
    required = current?.delivery.find((entry) => entry.recipientId === row.id),
    senderIssues = current?.senderIssues ?? [],
    change = (values: Partial<typeof row>) =>
      setOrder({ ...order, recipients: [{ ...row, ...values }] });
  return (
    <>
      <Modal title="직접 입력" draft={order} busy={busy} onClose={closeDraft}>
        <div className="retail-order-dialog">
          <div className="retail-order-body">
            <p>주문처: 반도스탠드</p>
            <h3>배송 정보</h3>
            {(["name", "address", "mobile", "phone", "note"] as const).map((field, index) => {
              const invalid =
                attempted && !!required &&
                (field === "name"
                  ? !required.name
                  : field === "address"
                    ? !required.address
                    : ["mobile", "phone"].includes(field)
                      ? !required.contact
                      : false);
              return (
                <Field
                  key={field}
                  label={
                    [
                      "수취인명",
                      "배송 주소",
                      "휴대폰 번호",
                      "일반전화 번호",
                      "배송 요청사항(선택)",
                    ][index]
                  }
                >
                  <input
                    aria-label={
                      [
                        "수취인명",
                        "배송 주소",
                        "휴대폰 번호",
                        "일반전화 번호",
                        "배송 요청사항(선택)",
                      ][index]
                    }
                    value={row[field]}
                    aria-invalid={invalid}
                    onChange={(e) => change({ [field]: e.target.value })}
                  />
                  {invalid && (
                    <small className="form-error">
                      {field === "name"
                        ? "수취인 이름을 확인해주세요."
                        : field === "address"
                          ? "상세 배송 주소를 입력해주세요."
                          : row.mobile || row.phone
                            ? contactProblem(row.mobile || row.phone, !!row.mobile)
                            : "유효한 휴대폰 또는 일반전화가 필요합니다."}
                    </small>
                  )}
                </Field>
              );
            })}
            <h3>주문 상품</h3>
            <button onClick={() => setSearch(true)}>상품 추가</button>
            {attempted && !row.items.length && (
              <p className="form-error">등록된 상품과 옵션을 선택하세요.</p>
            )}
            {row.items.map((item, index) => (
              <article className="retail-item" key={item.id}>
                <b>
                  {item.name} — {item.color}
                </b>
                <label className="field">
                  옵션
                  <select
                    value={item.variantId ?? ""}
                    onChange={(e) => {
                      const product = state.products.find((p) => p.id === item.productId)!,
                        variant = product.variants.find((v) => v.id === e.target.value)!;
                      change({
                        items: row.items.map((i) =>
                          i.id === item.id
                            ? {
                                ...i,
                                variantId: variant.id,
                                color: variant.color,
                                url: variant.url,
                              }
                            : i,
                        ),
                      });
                    }}
                  >
                    {state.products
                      .find((p) => p.id === item.productId)
                      ?.variants.map((v) => (
                        <option key={v.id} value={v.id}>
                          {v.color} {v.option}
                        </option>
                      ))}
                  </select>
                </label>
                <div className="quantity-stepper">
                  <button
                    aria-label={"수량 감소 " + (index + 1)}
                    onClick={() =>
                      change({
                        items: row.items.map((i) =>
                          i.id === item.id
                            ? {
                                ...i,
                                quantity: Math.max(1, (i.quantity ?? 1) - 1),
                                quantityConfirmed: true,
                              }
                            : i,
                        ),
                      })
                    }
                  >
                    −
                  </button>
                  <input
                    aria-label={"상품 수량 " + (index + 1)}
                    type="number"
                    min="1"
                    step="1"
                    value={item.quantity ?? ""}
                    onChange={(e) => {
                      const quantity = e.target.value === "" ? null : Number(e.target.value);
                      if (quantity !== null && (!Number.isSafeInteger(quantity) || quantity < 1)) return;
                      change({
                        items: row.items.map((i) =>
                          i.id === item.id
                            ? {
                                ...i,
                                quantity,
                                quantityConfirmed: quantity !== null,
                              }
                            : i,
                        ),
                      });
                    }}
                  />
                  <button
                    aria-label={"수량 증가 " + (index + 1)}
                    onClick={() => {
                      const quantity = (item.quantity ?? 1) + 1;
                      if (!Number.isSafeInteger(quantity)) return;
                      change({
                        items: row.items.map((i) =>
                          i.id === item.id
                            ? { ...i, quantity, quantityConfirmed: true }
                            : i,
                        ),
                      });
                    }}
                  >
                    +
                  </button>
                </div>
                <button
                  onClick={() => change({ items: row.items.filter((i) => i.id !== item.id) })}
                >
                  상품 삭제
                </button>
              </article>
            ))}
            {attempted && row.items.length > 0 && required?.quantity === false && (
              <p className="form-error">
                수량을 직접 입력하거나 증감하여 1 이상의 정수로 확정하세요.
              </p>
            )}
            <h3>출고 정보</h3>
            <p>송하인: 반도스탠드</p>
            {senderIssues.length > 0 && (
              <div className="form-error">
                {senderIssues.map((text) => (
                  <p key={text}>{text}</p>
                ))}
                {onSettings && <button onClick={() => void confirmDiscardDraft(
                  "직접 입력 주문", !sameValue(order,initial.current),onSettings,busy,
                )}>반도스탠드 정보 설정</button>}
              </div>
            )}
            <Field label="출고지">
              <select
                aria-label="출고지"
                value={row.dispatchOrigin ?? "FACTORY"}
                onChange={(e) => change({ dispatchOrigin: e.target.value as "FACTORY" | "STORE" })}
              >
                <option value="FACTORY">공장</option>
                <option value="STORE">매장</option>
              </select>
            </Field>
            <Field label="실제 출고 운임">
              <MoneyInput
                value={row.actualFreight ?? null}
                onChange={(actualFreight) => change({ actualFreight })}
              />
            </Field>
          </div>
          <footer className="modal-footer">
            <span role="status" className="form-error">
              {error || (attempted ? nativeError : "")}
            </span>
            <button type="button" disabled={busy} onClick={closeDraft}>닫기</button>
            <button
              className="primary"
              disabled={busy}
              onClick={async () => {
                setAttempted(true);
                setBusy(true);
                try {
                  await onSave(await nativeConfirmRetail(state, order));
                  onClose();
                } catch (e) {
                  setError(String(e));
                } finally {
                  setBusy(false);
                }
              }}
            >
              주문 확인 완료
            </button>
          </footer>
        </div>
      </Modal>
      {search && (
        <ProductSearchDialog
          state={state}
          onClose={() => { if (!addingProduct.current) setSearch(false); }}
          onChoose={(product) => {
            if (addingProduct.current || busy) return;
            addingProduct.current = true;
            setBusy(true);
            setError("");
            const selectedVariant = product.variants[0];
            void businessCoreCall<{ item: Item }>("rules", "apply_retail_action", {
              action: "create_retail_item", state, product,
              color: selectedVariant?.color,
              variantId: product.variants.length === 1 ? selectedVariant?.id : undefined,
              newItemId: crypto.randomUUID(), quantity: 1,
            }).then(({ item: catalogItem }) => {
              if (!alive.current) return;
              if (!catalogItem.variantId) {
                throw Error(catalogItem.issues[0] || "등록된 독립 상품을 선택하세요.");
              }
              const item: Item = {
                ...catalogItem, quantityConfirmed: false, unitPrice: null,
              };
              // Preserve the newest recipient and dispatch-origin edits while
              // the native command was running; never append a stale shipment.
              setOrder((previous) => previous ? ({
                ...previous,
                recipients: previous.recipients.map((recipient, index) =>
                  index === 0 ? {
                    ...recipient, items: [...recipient.items, {
                      ...item, origin: recipient.dispatchOrigin ?? "FACTORY",
                    }],
                  } : recipient),
              }) : previous);
              setSearch(false);
            }).catch((cause) => {
              if (alive.current) setError(cause instanceof Error ? cause.message : String(cause));
            }).finally(() => {
              addingProduct.current = false;
              if (alive.current) setBusy(false);
            });
          }}
        />
      )}
    </>
  );
}
