import { useEffect, useRef, useState } from "react";
import { actualFreightOf } from "./actual-freight";
import { nativeRecordActualFreight } from "./orders/native-rules";
import { usePageLeaveGuard } from "./shared/page-leave";
import { Field, MoneyInput } from "./ui";
import type { Origin, Price, State } from "./types";
export function ActualFreightField({
  state,
  shipmentId,
  origin,
  onSave,
  draftValue,
  onDraftChange,
  onDraftSaved,
}: {
  state: State;
  shipmentId: string;
  origin: Origin;
  onSave: (s: State) => Promise<void>;
  draftValue?: Price;
  onDraftChange?: (value: Price) => void;
  onDraftSaved?: () => void;
}) {
  const ship = state.workflow!.shipments.find((s) => s.id === shipmentId)!,
    row = state.orders
      .find((o) => o.id === ship.orderId)!
      .recipients.find((r) => r.id === ship.recipientId)!,
    saved = actualFreightOf(ship, row, origin);
  const [localValue, setLocalValue] = useState(saved),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const previousSaved = useRef(saved);
  const controlled = !!onDraftChange;
  const value = controlled ? (draftValue === undefined ? saved : draftValue) : localValue;
  const change = (next: Price) => controlled ? onDraftChange?.(next) : setLocalValue(next);
  // Refresh untouched fields but never overwrite a user's pending amount.
  useEffect(() => {
    if (!controlled) setLocalValue((current) => current === previousSaved.current ? saved : current);
    previousSaved.current = saved;
  }, [saved, shipmentId, origin, controlled]);
  // The ref changes synchronously as a save starts, even before React paints
  // busy=true. Navigation awaits this exact native write without starting a
  // second overlapping actual-freight save.
  const saving = useRef<Promise<boolean> | null>(null);
  const save = (): Promise<boolean> => {
    if (saving.current) return saving.current;
    if (value === saved) return Promise.resolve(true);
    const task = (async (): Promise<boolean> => {
      setBusy(true);
      try {
        await onSave(await nativeRecordActualFreight(state, shipmentId, origin, value));
        onDraftSaved?.();
        setError("");
        return true;
      } catch (cause) {
        setError(String(cause));
        return false;
      } finally {
        setBusy(false);
      }
    })();
    saving.current = task;
    void task.then(() => {
      if (saving.current === task) saving.current = null;
    }, () => {
      if (saving.current === task) saving.current = null;
    });
    return task;
  };
  usePageLeaveGuard({
    label: "실제 출고 운임",
    hasChanges: () => !controlled && value !== saved,
    busy: () => busy || saving.current !== null,
    waitForIdle: async () => {
      const task = saving.current;
      if (task && !(await task)) throw Error("실제 출고 운임 저장이 실패했습니다.");
    },
    save,
    discard: () => setLocalValue(saved),
  });
  return (
    <div className="actual-freight-field">
      <Field label={(origin === "FACTORY" ? "공장" : "매장") + " 실제 출고 운임"}>
        <MoneyInput value={value} onChange={change} />
      </Field>
      <button disabled={busy || value === saved} onClick={() => void save()}>
        실제 운임 저장
      </button>
      <small role="status" className="form-error">{error}</small>
    </div>
  );
}
