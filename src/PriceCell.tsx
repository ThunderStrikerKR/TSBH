import { useEffect, useRef, useState } from "react";
import { readableError } from "./errors";
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
}: {
  value: Price;
  label: string;
  enabled: boolean;
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

  const findPriceCell = (offset: number) => {
    const current = input.current;
    const body = current?.closest("tbody");
    if (!current || !body) return null;
    const cells = Array.from(
      body.querySelectorAll<HTMLInputElement>("input[data-price-cell]"),
    ).filter((cell) => !cell.disabled && !cell.readOnly);
    const index = cells.indexOf(current);
    return index < 0 ? null : cells[index + offset] ?? null;
  };

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
            : value === null
              ? ""
              : value.toLocaleString("ko-KR")
        }
        placeholder="미등록"
        onFocus={(e) => {
          skipBlurSave.current = false;
          if (!enabled || busy) return;
          if (!editing) {
            expected.current = value;
            setDraft(value === null ? "" : value.toLocaleString("ko-KR"));
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
          if (e.key === "Escape") {
            e.preventDefault();
            cancel();
            return;
          }

          let offset: number | undefined;
          if (e.key === "Enter") offset = e.shiftKey ? -3 : 3;
          else if (e.key === "Tab") offset = e.shiftKey ? -1 : 1;
          else if (e.key === "ArrowDown") offset = 3;
          else if (e.key === "ArrowUp") offset = -3;
          if (offset === undefined) return;

          const target = findPriceCell(offset);
          if (e.key === "Tab" && !target) return;
          if (
            (e.key === "ArrowDown" || e.key === "ArrowUp") &&
            !target
          ) {
            return;
          }

          e.preventDefault();
          if (!parseDraft().ok) {
            showValidationError();
            return;
          }
          void save();
          if (target) target.focus();
          else input.current?.blur();
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
