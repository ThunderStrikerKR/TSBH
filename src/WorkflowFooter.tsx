import { useEffect, useRef, useState } from "react";
import type { Origin, Price, State } from "./types";
import {
  findCellNavigationTarget,
  focusCellElement,
  isCellNavigationKey,
  isImeComposing,
} from "./ui/cell-navigation";
import { shipmentContext } from "./workflow-view";
import type { WorkflowShipment } from "./workflow-types";
/** Keep freight typing local; native totals run on blur, Enter or unmount. */
function FreightAmountInput({ label, value, disabled, onCommit, navigationRow, navigationOrder }: {
  label: string; value: Price | undefined; disabled: boolean;
  navigationRow: number; navigationOrder: number;
  onCommit: (value: Price) => boolean | void | Promise<boolean | void>;
}) {
  const format = (amount: Price | undefined) =>
    amount === null || amount === undefined ? "" : amount.toLocaleString("ko-KR");
  const [text, setText] = useState(() => format(value));
  const pending = useRef<Price | undefined>(undefined);
  const currentValue = useRef(value);
  currentValue.current = value;
  const latestCommit = useRef(onCommit);
  latestCommit.current = onCommit;
  useEffect(() => {
    if (pending.current === undefined) setText(format(value));
  }, [value]);
  const flush = () => {
    const amount = pending.current;
    pending.current = undefined;
    if (amount === undefined || amount === currentValue.current) return;
    const rollback = () => {
      // A newer keystroke must not be discarded by a delayed native reply.
      if (pending.current === undefined) setText(format(currentValue.current));
    };
    const result = latestCommit.current(amount);
    if (result === false) rollback();
    else if (result instanceof Promise) {
      void result.then((accepted) => {
        if (accepted === false) rollback();
      }, rollback);
    }
  };
  useEffect(() => () => {
    if (pending.current !== undefined && pending.current !== currentValue.current)
      latestCommit.current(pending.current);
    pending.current = undefined;
  }, []);
  return (
    <input
      aria-label={label}
      data-cell-nav-id={`freight:${navigationRow}`}
      data-cell-nav-row={navigationRow}
      data-cell-nav-column={2}
      data-cell-nav-order={navigationOrder}
      inputMode="numeric"
      disabled={disabled}
      value={text}
      placeholder="—"
      onChange={(e) => {
        const input = e.currentTarget;
        const raw = input.value.replaceAll(",", "");
        if (!/^\d*$/.test(raw) || (raw !== "" && !Number.isSafeInteger(Number(raw))))
          return;
        const digitsBefore = input.value
          .slice(0, input.selectionStart ?? input.value.length)
          .replace(/\D/g, "").length;
        const amount = raw === "" ? null : Number(raw);
        pending.current = amount;
        setText(format(amount));
        requestAnimationFrame(() => {
          if (!input.isConnected || document.activeElement !== input) return;
          let at = 0, count = 0;
          while (at < input.value.length && count < digitsBefore) {
            if (/\d/.test(input.value[at])) count++;
            at++;
          }
          input.setSelectionRange(at, at);
        });
      }}
      onBlur={flush}
      onKeyDown={(e) => {
        if (isImeComposing(e) || !isCellNavigationKey(e.key)) return;
        const target = findCellNavigationTarget(e.currentTarget, e.key, e.shiftKey);
        if (e.key === "Tab" && !target) return;
        if (target) {
          e.preventDefault();
          focusCellElement(target);
          return;
        }
        if (e.key !== "Tab") e.preventDefault();
        if (e.key === "Enter") {
          flush();
          e.currentTarget.blur();
        }
      }}
    />
  );
}

type FreightProps = {
  state: State;
  shipment: WorkflowShipment;
  freightCode: string;
  displayCharges: Partial<Record<Origin, Price>>;
  freightEditingBlocked: boolean;
  activeKey?: string;
  navigationRowOffset: number;
  onFreight: (origin: Origin, value: Price) => boolean | void | Promise<boolean | void>;
  onCode: (v: "010" | "020") => void;
  remoteChecked: boolean;
  onRemoteChecked: (checked: boolean) => void;
};
export function WorkflowFreightRows({
  state,
  shipment,
  freightCode,
  displayCharges,
  freightEditingBlocked,
  onFreight,
  onCode,
  remoteChecked,
  onRemoteChecked,
  activeKey,
  navigationRowOffset,
}: FreightProps) {
  const { recipient } = shipmentContext(state, shipment),
    origins = [...new Set(recipient.items.map((item) => item.origin))],
    remoteRowOrigin = origins.includes("FACTORY") ? "FACTORY" : origins[0],
    disabled = shipment.transferred;
  return (
    <>
      {origins.map((origin, index) => {
        const value = displayCharges[origin];
        return (
          <tr
            key={origin}
            data-freight-origin={origin}
            data-link-key={recipient.id + ":freight"}
            className={
              "freight-item-row freight-input-area" +
              (activeKey === recipient.id + ":freight" ? " freight-needs-focus" : "")
            }
          >
            <td colSpan={3}>
              <div className="freight-line">
                <b className="freight-name">
                  {origins.length === 1
                    ? "운임"
                    : origin === "FACTORY"
                      ? "공장 운임"
                      : "매장 운임"}
                </b>
                <div className="freight-amount-code">
                  <div className="freight-money">
                  <FreightAmountInput
                    label={`${recipient.name} ${origin === "FACTORY" ? "공장" : "매장"} 적용 운임`}
                    disabled={disabled || freightEditingBlocked}
                    value={value}
                    navigationRow={navigationRowOffset + index}
                    navigationOrder={navigationRowOffset + index}
                    onCommit={(amount) => onFreight(origin, amount)}
                  />
                    <span>원</span>
                  </div>
                  <span className="freight-payment-code">
                    <span aria-hidden="true">(</span>
                    <select
                      aria-label={index ? "운임구분 " + origin : "운임구분"}
                      value={freightCode}
                      disabled={disabled}
                      onChange={(e) => onCode(e.target.value as "010" | "020")}
                    >
                      <option value="010">선불</option>
                      <option value="020">착불</option>
                    </select>
                    <span aria-hidden="true">)</span>
                  </span>
                </div>
                {origin === remoteRowOrigin && (
                  <label className="freight-remote-check">
                    <input type="checkbox" aria-label="도서/산간"
                      disabled={disabled}
                      checked={remoteChecked}
                      onChange={(e) => onRemoteChecked(e.currentTarget.checked)}
                    />
                    <span>도서/산간</span>
                  </label>
                )}
                <button
                  className="freight-inquiry"
                  aria-hidden={origin !== "FACTORY"}
                  style={{ visibility: origin === "FACTORY" ? "visible" : "hidden" }}
                  disabled
                  title="공장 문의 연동 준비 중"
                >
                  공장에 운임 문의
                </button>
              </div>
            </td>
          </tr>
        );
      })}
    </>
  );
}
export function WorkflowFooter({
  shipment,
  vatIncluded,
  onVat,
  recipientFade,
  unavailable = false,
}: {
  state: State;
  shipment: WorkflowShipment;
  vatIncluded: boolean;
  groupId: string;
  onVat: (v: boolean | undefined) => void;
  recipientFade?: "pending" | "ready";
  unavailable?: boolean;
}) {
  // Keep the native preview's last settled checkbox mounted without
  // permitting a stale VAT decision while the next Rust result is pending.
  const disabled = shipment.transferred || unavailable;
  return (
    <section className="workflow-footer order-settings" aria-label="발주 설정"
      data-recipient-fade={recipientFade}
      aria-busy={recipientFade === "pending"}
      inert={recipientFade === "pending"}>
      <div className="shipment-vat">
        <label className="check">
          <input
            type="checkbox"
            aria-label="배송 건 부가세"
            disabled={disabled}
            checked={vatIncluded}
            onChange={(e) => onVat(e.target.checked)}
          />{" "}
          부가세 포함
        </label>
      </div>
    </section>
  );
}
