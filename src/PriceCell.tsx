import { useEffect, useRef, useState } from "react";
import { readableError } from "./errors";
import { handleCellNavigationKey } from "./ui/cell-navigation";
import type { Price } from "./types";

type ParsedPrice =
  | { ok: true; value: Price }
  | { ok: false };

export function PriceCell({
  value,
  label,
  enabled,
  onSave,
  onPendingChange,
  navigationRow,
  navigationColumn,
  navigationOrder,
}: {
  value: Price;
  label: string;
  enabled: boolean;
  navigationRow: number;
  navigationColumn: number;
  navigationOrder: number;
  onSave: (value: Price, expected: Price) => Promise<void>;
  onPendingChange?: (pending: boolean) => void;
}) {
  const [editing, setEditing] = useState(false),
    [draft, setDraft] = useState(value === null ? "" : String(value)),
    [busy, setBusy] = useState(false),
    [status, setStatus] = useState(""),
    [error, setError] = useState("");
  const saving = useRef(false),
    expected = useRef(value),
    optimisticCommit = useRef<{ next: Price } | null>(null),
    input = useRef<HTMLInputElement>(null),
    skipBlurSave = useRef(false),
    lastReportedPending = useRef(false);
  const pendingRef = useRef(onPendingChange);
  pendingRef.current = onPendingChange;

  const parseDraft = (raw = draft): ParsedPrice => {
    const text = raw.replaceAll(",", "");
    if (
      text !== "" &&
      (!/^(?:\d+|\d{1,3}(?:,\d{3})+)$/.test(raw) ||
        !Number.isSafeInteger(Number(text)))
    ) {
      return { ok: false };
    }
    return {
      ok: true,
      value: text === "" || Number(text) === 0 ? null : Number(text),
    };
  };

  const isDraftDirty = (raw = draft) => {
    const initial = expected.current === null ? "" : String(expected.current);
    if (raw.replaceAll(",", "") === initial) return false;
    const parsed = parseDraft(raw);
    return !parsed.ok || parsed.value !== expected.current;
  };

  const pending = editing && !busy && isDraftDirty();

  useEffect(() => {
    if (!editing) {
      const optimistic = optimisticCommit.current;
      // A list refresh may briefly return any pre-write snapshot. Keep
      // the latest committed cell value until the row matches that commit.
      if (optimistic && value !== optimistic.next) return;
      optimisticCommit.current = null;
      expected.current = value;
      setDraft(value === null ? "" : String(value));
    }
  }, [value, editing]);

  // Report only transitions, not every unedited cell on initial table mount.
  useEffect(() => {
    if (lastReportedPending.current === pending) return;
    lastReportedPending.current = pending;
    pendingRef.current?.(pending);
  }, [pending]);
  useEffect(() => () => {
    if (lastReportedPending.current) pendingRef.current?.(false);
  }, []);

  const showValidationError = () => {
    setError("원 단위 정수를 입력하세요. 0 또는 빈칸은 미등록입니다.");
    setStatus("입력 확인");
  };

  const cancel = () => {
    if (saving.current) return;
    skipBlurSave.current = true;
    if (lastReportedPending.current) {
      lastReportedPending.current = false;
      pendingRef.current?.(false);
    }
    setDraft(
      expected.current === null
        ? ""
        : expected.current.toLocaleString("ko-KR"),
    );
    setEditing(false);
    setError("");
    setStatus("");
    input.current?.blur();
  };

  const save = async () => {
    if (!editing || saving.current) return;
    const parsed = parseDraft();
    if (!parsed.ok) {
      showValidationError();
      return;
    }
    const next = parsed.value;
    if (!isDraftDirty() || next === expected.current) {
      if (lastReportedPending.current) {
        lastReportedPending.current = false;
        pendingRef.current?.(false);
      }
      setEditing(false);
      setError("");
      setStatus("");
      return;
    }

    saving.current = true;
    setBusy(true);
    if (lastReportedPending.current) {
      lastReportedPending.current = false;
      pendingRef.current?.(false);
    }
    setStatus("저장 중");
    setError("");
    try {
      await onSave(next, expected.current);
      optimisticCommit.current = { next };
      expected.current = next;
      setStatus("저장됨");
      setError("");
      setEditing(false);
    } catch (e) {
      // Keep the cell's value and comparison baseline for an automatic retry
      // on the next Enter, Tab, or focus change.
      if (!lastReportedPending.current) {
        lastReportedPending.current = true;
        pendingRef.current?.(true);
      }
      setError(readableError(e));
      setStatus("저장 실패");
    } finally {
      saving.current = false;
      setBusy(false);
    }
  };

  const displayedValue = optimisticCommit.current
    ? optimisticCommit.current.next
    : value;

  return (
    <div
      className={
        "price-cell" +
        (error ? " invalid" : "") +
        (pending ? " pending" : "") +
        (busy ? " busy" : "")
      }
      data-locked={!enabled}
      data-price-pending={pending}
      data-price-saving={busy}
    >
      <input
        ref={input}
        aria-label={label}
        data-price-cell
        data-cell-nav-id={`price:${navigationRow}:${navigationColumn}`}
        data-cell-nav-row={navigationRow}
        data-cell-nav-column={navigationColumn}
        data-cell-nav-order={navigationOrder}
        inputMode="numeric"
        readOnly={!enabled || busy}
        title={
          enabled
            ? "Enter 또는 다른 셀로 이동하면 자동 저장 / Esc로 현재 셀 변경 취소 (0은 미등록)"
            : "가격 수정 모드를 켜세요"
        }
        value={
          editing
            ? draft
            : displayedValue === null
              ? ""
              : displayedValue.toLocaleString("ko-KR")
        }
        placeholder="미등록"
        onFocus={(e) => {
          skipBlurSave.current = false;
          if (!enabled || busy) return;
          if (!editing) {
            const baseline = optimisticCommit.current ? expected.current : value;
            expected.current = baseline;
            setDraft(baseline === null ? "" : baseline.toLocaleString("ko-KR"));
            setEditing(true);
            setStatus("");
            setError("");
          }
          e.currentTarget.select();
        }}
        onChange={(e) => {
          if (enabled && !busy) {
            const next = e.target.value;
            setDraft(next);
            const nextPending = isDraftDirty(next);
            if (lastReportedPending.current !== nextPending) {
              lastReportedPending.current = nextPending;
              pendingRef.current?.(nextPending);
            }
            setError("");
            setStatus("");
          }
        }}
        onBlur={() => {
          if (skipBlurSave.current) {
            skipBlurSave.current = false;
            return;
          }
          if (saving.current || !editing) return;
          if (pending) {
            void save();
          } else {
            setEditing(false);
          }
        }}
        onKeyDown={(e) => {
          if (!enabled || busy) return;
          handleCellNavigationKey(e, {
            enterMode: "same-column",
            onEscape: () => cancel(),
            onBeforeMove: (key, target) => {
              if (!target && key !== "Enter") return;
              if (!parseDraft().ok) {
                showValidationError();
                return false;
              }
              void save();
            },
            onBoundary: (key) => {
              if (key === "Enter") input.current?.blur();
            },
          });
        }}
      />
      <small
        role={error ? "alert" : "status"}
        title={error || status}
        aria-live="polite"
      >
        {error || (busy ? "저장 중" : status)}
      </small>
    </div>
  );
}
