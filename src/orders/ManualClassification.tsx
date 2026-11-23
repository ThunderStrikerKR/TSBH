import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ProductSearchDialog } from "../ProductSearchDialog";
import { businessCoreCall } from "../tauri-bridge";
import { useNativeDeliveryRequirements, evaluateNativeDeliveryRequirements } from "../useNativeDeliveryRequirements";
import { useNativeOrderIssues } from "./useNativeOrderIssues";
import { usePendingChanges } from "../shared/pending-changes";
import { confirmDiscardDraft } from "../shared/editor-flow";
import {
  focusCellElement,
  isCellNavigationKey,
  isImeComposing,
  resolveCellNavigationTarget,
  type CellNavigationCell,
  type CellNavigationKey,
} from "../ui/cell-navigation";
import type { NoticeSink } from "../shared/notices";
import type { Order, Product, State } from "../types";
import { ManualPrompt } from "./ManualPrompt";
import {
  cellKey,
  manualValue,
  type Cell,
  type ManualModel,
} from "./manual-cell-view";

type Frame = {
  model: ManualModel;
  entry: Record<string, string>;
  quantityText: Record<string, string>;
  pending?: Cell;
  rollback?: Frame;
};
type ManualClassificationProps = {
  state: State;
  order: Order;
  onApply: (order: Order) => void;
  onClose: () => void;
  onNotice: NoticeSink;
};
type ManualRustDraft = { model: ManualModel; assignmentOrder: string[] };
function orderedNativeModel(result: ManualRustDraft): ManualModel {
  // JSON maps do not preserve the JS original assignment insertion sequence.
  const keys = [...new Set([...result.assignmentOrder, ...Object.keys(result.model.assignments)])];
  return {
    ...result.model,
    assignments: Object.fromEntries(
      keys.filter((key) => Object.hasOwn(result.model.assignments, key))
        .map((key) => [key, result.model.assignments[key]]),
    ),
  };
}
export function ManualClassification(props: ManualClassificationProps) {
  const [loaded, setLoaded] = useState<{ orderId: string; model: ManualModel }>();
  const [loadingError, setLoadingError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setLoadingError("");
    setLoaded(undefined);
    void businessCoreCall<ManualRustDraft>("rules", "apply_manual_classification", {
      action: "initial_manual",
      order: props.order,
      assignmentOrder: Object.keys(props.order.manualClassification?.assignments ?? {}),
    }).then((result) => {
      if (active) setLoaded({ orderId: props.order.id, model: orderedNativeModel(result) });
    }).catch((error) => {
      if (active) setLoadingError("Rust 수동 분류 초기화 실패: " +
        (error instanceof Error ? error.message : String(error)));
    });
    return () => { active = false; };
  }, [props.order.id, retry]);
  if (!loaded || loaded.orderId !== props.order.id) return (
    <section className="manual-workspace" role="dialog" aria-label="수동 분류">
      <header className="manual-workspace-heading">
        <h2>수동 분류</h2>
        <button onClick={props.onClose}>닫기 ×</button>
      </header>
      {loadingError
        ? <div role="alert">{loadingError}
            <button onClick={() => setRetry((value) => value + 1)}>다시 시도</button>
          </div>
        : <p role="status">Rust에서 원문 블록을 구성하고 있습니다.</p>}
    </section>
  );
  return <ManualClassificationEditor key={loaded.orderId}
    {...props} initialModel={loaded.model}/>;
}
function ManualClassificationEditor({
  state,
  order,
  onApply,
  onClose,
  initialModel,
}: ManualClassificationProps & { initialModel: ManualModel }) {
  const [model, setModel] = useState(() => initialModel);
  const nativeDraftLock = useRef(false);
  const [nativeDraftBusy, setNativeDraftBusy] = useState(false);
  const manualQuantityRequest = useRef<Record<string, number>>({});
  const manualTextRequest = useRef<Record<string, number>>({});
  const manualQuantityPending = useRef<Set<string>>(new Set());
  // Native text responses can arrive after a different cell edit. The visible
  // input overlay is NOT a committed draft row until Rust acknowledges it.
  const nativeTextPending = useRef<Set<string>>(new Set());
  const [nativeTextBusy, setNativeTextBusy] = useState(false);
  const delivery = useNativeDeliveryRequirements(model.rows, order.receptionVersion !== 1);
  const inspection = useMemo(() => ({ ...order, recipients: model.rows }), [order, model.rows]);
  const nativeOrderIssues = useNativeOrderIssues(inspection);
  // Rust already owns final apply_manual and the quantity predicate. React
  // consumes that result without an alternative TS eligibility policy.
  const quantityFingerprint = useMemo(() => JSON.stringify(
    model.rows.map((row) => [row.id, row.items.map((item) => [
      item.id, item.productId, item.name, item.quantity, item.quantityConfirmed,
    ])]),
  ), [model.rows]);
  type NativeQuantityStatus = {
    pending: boolean;
    items: { recipientId: string; itemId: string; valid: boolean }[];
  };
  const [quantityResult, setQuantityResult] = useState<{
    fingerprint: string;
    result: NativeQuantityStatus;
  }>();
  const [quantityFailure, setQuantityFailure] = useState<{
    fingerprint: string;
    reason: string;
  }>();
  useEffect(() => {
    let active = true;
    void businessCoreCall<NativeQuantityStatus>("rules", "apply_manual_classification", {
      action: "manual_quantity_status", model,
    }).then((result) => {
      if (!active) return;
      setQuantityResult({ fingerprint: quantityFingerprint, result });
      setQuantityFailure(undefined);
    }).catch((error) => {
      if (!active) return;
      setQuantityResult(undefined);
      setQuantityFailure({ fingerprint: quantityFingerprint, reason: String(error) });
    });
    return () => { active = false; };
  }, [quantityFingerprint]);
  const quantityReady = quantityResult?.fingerprint === quantityFingerprint;
  const quantityStatus = quantityReady && quantityResult ? quantityResult.result : undefined;
  const quantityError = quantityFailure?.fingerprint === quantityFingerprint
    ? quantityFailure.reason : "";
  const latestInputs = useRef({ order, model });
  latestInputs.current = { order, model };
  const initial = useRef(JSON.stringify(model));
  const [count, setCount] = useState(String(model.rows.length));
  const [active, setActive] = useState<Cell>(),
    [search, setSearch] = useState<Cell>();
  const [pending, setPendingState] = useState<Cell>();
  const [entry, setEntry] = useState<Record<string, string>>({});
  const [quantityText, setQuantityText] = useState<Record<string, string>>({});
  const [error, setError] = useState<{ key?: string; message: string }>();
  const [completion, setCompletion] = useState(false);
  const [confirm, setConfirm] = useState<{ title: string; message: string; execute: () => void }>();
  const root = useRef<HTMLElement>(null),
    pendingRef = useRef<Cell | undefined>(undefined);
  const history = useRef<Frame[]>([]),
    beforeProduct = useRef<Frame | undefined>(undefined);
  const editStart = useRef<{ cell: Cell; frame: Frame } | undefined>(undefined),
    applying = useRef(false);
  const dirty = JSON.stringify(model) !== initial.current || Object.keys(entry).length > 0 ||
    count !== String(model.rows.length);
  usePendingChanges(dirty);
  const closeDraft = () => void confirmDiscardDraft("수동 분류", dirty, onClose);
  const frame = (): Frame =>
    structuredClone({ model, entry, quantityText, pending, rollback: beforeProduct.current });
  const checkpoint = () => {
    history.current.push(frame());
    if (history.current.length > 100) history.current.shift();
  };
  const setPending = (cell?: Cell) => {
    pendingRef.current = cell;
    setPendingState(cell);
  };
  const restore = (value: Frame) => {
    setModel(value.model);
    setEntry(value.entry);
    setQuantityText(value.quantityText);
    setPending(value.pending);
    beforeProduct.current = value.rollback;
    setCount(String(value.model.rows.length));
    setError(undefined);
  };
  const focus = (cell: Cell) => {
    setActive(cell);
    requestAnimationFrame(() => {
      const input = root.current?.querySelector<HTMLInputElement>(
        `input[data-cell="${CSS.escape(cellKey(cell))}"]`,
      );
      focusCellElement(input);
    });
  };
  const fail = (message: string, cell?: Cell) =>
    setError({ key: cell ? cellKey(cell) : undefined, message });
  const markNativeTextPending = (key: string, busy: boolean) => {
    if (busy) nativeTextPending.current.add(key);
    else nativeTextPending.current.delete(key);
    setNativeTextBusy(nativeTextPending.current.size > 0);
  };
  const requireNativeCellsSettled = () => {
    if (!nativeDraftLock.current && !nativeTextPending.current.size &&
        !manualQuantityPending.current.size) return true;
    fail("입력한 배송 정보와 수량의 Rust 처리가 끝난 뒤 다시 시도하세요.");
    return false;
  };
  const nativeStructure = async (
    action: "resize_manual_rows" | "add_manual_item" | "remove_manual_item" |
      "confirm_manual_quantity" | "reset_manual_rows",
    changes: Record<string, unknown>,
    success?: (next: ManualModel) => void,
  ): Promise<boolean> => {
    if (!requireNativeCellsSettled()) return false;
    const previousModel = model;
    nativeDraftLock.current = true;
    setNativeDraftBusy(true);
    try {
      const response = await businessCoreCall<ManualRustDraft>(
        "rules", "apply_manual_classification", {
          action, model: previousModel,
          assignmentOrder: Object.keys(previousModel.assignments),
          ...changes,
        },
      );
      if (latestInputs.current.order !== order ||
          latestInputs.current.model !== previousModel)
        throw Error("수동 분류 내용이 변경되었습니다. 최신 내용을 확인하고 다시 시도하세요.");
      const next = orderedNativeModel(response);
      checkpoint();
      setModel(next);
      success?.(next);
      setError(undefined);
      return true;
    } catch (e) {
      fail(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      nativeDraftLock.current = false;
      setNativeDraftBusy(false);
    }
  };
  // React merges only the fields returned by the Rust quantity reducer.
  // This prevents another cell's recent input from being overwritten by an
  // older response that originated from the same model snapshot.
  const applyNativeQuantity = (current: ManualModel, updated: ManualModel, cell: Cell) => {
    const nativeItem = updated.rows.find((row) => row.id === cell.recipientId)
      ?.items.find((item) => item.id === cell.itemId);
    if (!nativeItem) return current;
    const next = structuredClone(current);
    const item = next.rows.find((row) => row.id === cell.recipientId)
      ?.items.find((item) => item.id === cell.itemId);
    if (!item) return current;
    item.quantity = nativeItem.quantity;
    item.quantityConfirmed = nativeItem.quantityConfirmed;
    item.issues = nativeItem.issues;
    if (nativeItem.quantityApproval) item.quantityApproval = nativeItem.quantityApproval;
    else delete item.quantityApproval;
    return next;
  };
  const attempt = (action: () => void, cell?: Cell) => {
    try {
      action();
      setError(undefined);
    } catch (e) {
      fail(String(e).replace(/^Error: /, ""), cell);
    }
  };
  const allowed = (cell: Cell) =>
    !nativeDraftBusy && (!pendingRef.current ||
      cellKey(pendingRef.current) === cellKey(cell));
  const nextMissing = (next: ManualModel, cell: Cell) => {
    // The next required field is selected from a fresh native projection.
    // A completed previous request must not move focus after later edits.
    const expected = JSON.stringify(next.rows);
    void evaluateNativeDeliveryRequirements(next.rows, order.receptionVersion !== 1)
      .then((results) => {
        if (JSON.stringify(latestInputs.current.model.rows) !== expected) return;
        const byId = new Map(results.map((item) => [item.recipientId, item]));
        const slots = next.rows.flatMap((row) => {
          const required = byId.get(row.id);
          if (!required) return [];
          return [
            !required.name && { recipientId: row.id, field: "name" },
            !required.address && { recipientId: row.id, field: "address" },
            !required.contact && { recipientId: row.id, field: "mobile" },
          ].filter(Boolean) as Cell[];
        });
        const rowIndex = next.rows.findIndex((row) => row.id === cell.recipientId);
        const fields = ["name", "address", "mobile", "phone"];
        const position = rowIndex * 4 + fields.indexOf(cell.field);
        const following = slots.find((slot) =>
          next.rows.findIndex((row) => row.id === slot.recipientId) * 4 +
          fields.indexOf(slot.field) > position);
        if (following) focus(following);
      }).catch((e) => fail("Rust 배송 필수정보 조회 실패: " +
        (e instanceof Error ? e.message : String(e)), cell));
  };
  const cancelProduct = () => {
    if (!requireNativeCellsSettled()) return;
    const cell = pendingRef.current;
    if (!cell) return;
    checkpoint();
    if (beforeProduct.current) restore(beforeProduct.current);
    else {
      setPending(undefined);
      setQuantityText({});
    }
    beforeProduct.current = undefined;
    setEntry((old) => {
      const next = { ...old };
      delete next[cellKey({ ...cell, field: "product" })];
      return next;
    });
    setQuantityText((old) => {
      const next = { ...old };
      delete next[cellKey(cell)];
      return next;
    });
    focus({ ...cell, field: "product" });
  };
  const selectProduct = (cell: Cell, value: string, product?: Product) => {
    if (!allowed(cell) || nativeDraftLock.current || !requireNativeCellsSettled()) return;
    nativeDraftLock.current = true;
    setNativeDraftBusy(true);
    const previousModel = model;
    void businessCoreCall<ManualRustDraft>("rules", "apply_manual_classification", {
      action: "set_manual_product", model, cell, value, state, order, product,
      candidateIds: Array.from({ length: 20 }, () => crypto.randomUUID()),
      assignmentOrder: Object.keys(model.assignments),
    }).then((result) => {
      if (latestInputs.current.model !== previousModel ||
          latestInputs.current.order !== order) return;
      const next = orderedNativeModel(result);
      checkpoint();
      beforeProduct.current = frame();
      setModel(next);
      setEntry((old) => {
        const copy = { ...old };
        delete copy[cellKey(cell)];
        return copy;
      });
      setQuantityText((old) => ({ ...old, [cellKey({ ...cell, field: "quantity" })]: "" }));
      setSearch(undefined);
      setPending({ ...cell, field: "quantity" });
      focus({ ...cell, field: "quantity" });
      setError(undefined);
    }).catch((e) => fail(e instanceof Error ? e.message : String(e), cell))
      .finally(() => {
        nativeDraftLock.current = false;
        setNativeDraftBusy(false);
      });
  };
  const finishQuantity = async (): Promise<boolean> => {
    const cell = pendingRef.current;
    if (!cell) return true;
    if (nativeTextPending.current.size || manualQuantityPending.current.has(cellKey(cell))) {
      fail("수취인 정보 또는 수량의 Rust 처리가 끝난 뒤 다시 시도하세요.", cell);
      return false;
    }
    const ok = await nativeStructure("confirm_manual_quantity", { cell }, () => {
      setPending(undefined);
      beforeProduct.current = undefined;
    });
    if (!ok) focus(cell);
    return ok;
  };
  const useBlock = (id: string) => {
    if (nativeDraftLock.current || !requireNativeCellsSettled()) return;
    const assigned = model.assignments[id];
    if (assigned && pendingRef.current &&
        assigned.itemId !== pendingRef.current.itemId) {
      fail("선택한 상품의 수량을 먼저 확정하세요.", pendingRef.current);
      return;
    }
    const cell = assigned ?? pendingRef.current ?? active;
    if (!cell) {
      fail("입력할 셀을 선택하세요.");
      return;
    }
    if (!assigned && pendingRef.current &&
        model.blocks.find((block) => block.id === id)?.field !== "quantity") {
      fail("선택한 상품의 수량을 먼저 확정하세요.", pendingRef.current);
      return;
    }
    const previousModel = model;
    nativeDraftLock.current = true;
    setNativeDraftBusy(true);
    void businessCoreCall<ManualRustDraft>("rules", "apply_manual_classification", {
      action: assigned ? "release_manual_block" : "assign_manual_block",
      model, id, cell, clear: !!assigned, state, order,
      candidateIds: Array.from({ length: 20 }, () => crypto.randomUUID()),
      assignmentOrder: Object.keys(model.assignments),
    }).then((result) => {
      if (latestInputs.current.model !== previousModel ||
          latestInputs.current.order !== order) return;
      const next = orderedNativeModel(result);
      checkpoint();
      setModel(next);
      setEntry((old) => {
        const copy = { ...old };
        delete copy[cellKey(cell)];
        return copy;
      });
      if (assigned) {
        if (assigned.itemId) setQuantityText((old) => {
          const copy = { ...old };
          delete copy[cellKey({ ...assigned, field: "quantity" })];
          return copy;
        });
        if (assigned.field === "product") {
          setPending(undefined);
          beforeProduct.current = undefined;
        }
        if (assigned.field === "quantity") setPending({ ...assigned, field: "quantity" });
      } else if (cell.field === "product") {
        beforeProduct.current = frame();
        setPending({ ...cell, field: "quantity" });
        setQuantityText((old) => ({ ...old, [cellKey({ ...cell, field: "quantity" })]: "" }));
        focus({ ...cell, field: "quantity" });
      } else if (cell.field === "quantity") {
        setQuantityText((old) => ({ ...old, [cellKey(cell)]: manualValue(next, cell) }));
        setPending(undefined);
        beforeProduct.current = undefined;
      } else nextMissing(next, cell);
      setError(undefined);
    }).catch((e) => fail(e instanceof Error ? e.message : String(e), cell))
      .finally(() => {
        nativeDraftLock.current = false;
        setNativeDraftBusy(false);
      });
  };
  const resize = (n: number) => {
    if (!requireNativeCellsSettled()) return;
    if (!Number.isSafeInteger(n) || n < 1 || n > 100) {
      fail("배송 건수는 1~100건입니다.");
      return;
    }
    if (pendingRef.current) {
      setCount(String(model.rows.length));
      fail("수량을 확정하거나 상품 선택을 취소하세요.", pendingRef.current);
      return;
    }
    if (n === model.rows.length) {
      setCount(String(n));
      return;
    }
    const execute = () => {
      const newRecipientIds = Array.from(
        { length: Math.max(0, n - model.rows.length) },
        () => crypto.randomUUID(),
      );
      void nativeStructure("resize_manual_rows", { count: n, newRecipientIds }, (next) => {
        const ids = new Set(next.rows.map((row) => row.id));
        setCount(String(next.rows.length));
        setEntry({});
        setQuantityText({});
        history.current = [];
        if (active && !ids.has(active.recipientId)) setActive(undefined);
      });
    };
    const removed = model.rows.slice(n);
    const populated = removed.some(
      (row) =>
        row.name || row.address || row.mobile || row.phone || row.note ||
        row.items.some((item) => item.name || item.productId || item.quantity !== null) ||
        Object.values(model.assignments).some((a) => a.recipientId === row.id),
    );
    setCount(String(model.rows.length));
    if (populated)
      setConfirm({
        title: "배송 건수 줄이기",
        message: "마지막 배송 건의 입력값과 원문 배정을 제거할까요?",
        execute,
      });
    else execute();
  };
  const addProduct = (recipientId: string) => {
    const itemId = crypto.randomUUID();
    void nativeStructure("add_manual_item", { recipientId, itemId }, () => {
      focus({ recipientId, itemId, field: "product" });
    });
  };
  const removeProduct = (cell: Cell) => {
    void nativeStructure("remove_manual_item", {
      recipientId: cell.recipientId, itemId: cell.itemId,
    });
  };
  const move = (cell: Cell, key: CellNavigationKey, shift = false) => {
    const inputs = Array.from(
      root.current?.querySelectorAll<HTMLInputElement>("input[data-cell]") ?? [],
    );
    let targetId: string | null = null;
    if (key === "ArrowUp" || key === "ArrowDown") {
      const rows = model.rows,
        rowIndex = rows.findIndex((row) => row.id === cell.recipientId),
        nextRow = rows[rowIndex + (key === "ArrowUp" ? -1 : 1)];
      if (nextRow) {
        const current = rows[rowIndex].items.findIndex((item) => item.id === cell.itemId);
        const nextCell: Cell = {
          ...cell,
          recipientId: nextRow.id,
          itemId: cell.itemId
            ? nextRow.items[Math.min(Math.max(current, 0), nextRow.items.length - 1)]?.id
            : undefined,
        };
        if (allowed(nextCell)) targetId = cellKey(nextCell);
      }
    } else {
      const sequence: Cell[] = model.rows.flatMap((row) => [
        ...(["name", "address", "mobile", "phone", "note"] as const)
          .map((field) => ({ recipientId: row.id, field })),
        ...row.items.flatMap((item) => [
          { recipientId: row.id, itemId: item.id, field: "product" as const },
          { recipientId: row.id, itemId: item.id, field: "quantity" as const },
        ]),
      ]);
      const navigationCells: CellNavigationCell[] = sequence.map((candidate, order) => ({
        id: cellKey(candidate), row: 0, column: order, order,
        disabled: !allowed(candidate),
      }));
      targetId = resolveCellNavigationTarget(navigationCells, cellKey(cell), key, shift);
    }
    const target = targetId ? inputs.find((input) => input.dataset.cell === targetId) : undefined;
    if (target && !target.disabled) focusCellElement(target);
  };
  const keyDown = async (event: KeyboardEvent<HTMLInputElement>, cell: Cell) => {
    if (isImeComposing(event)) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") return;
    const key = cellKey(cell);
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!requireNativeCellsSettled()) return;
      if (pendingRef.current) {
        cancelProduct();
        return;
      }
      if (cell.field === "product")
        setEntry((old) => {
          const copy = { ...old };
          delete copy[key];
          return copy;
        });
      else if (editStart.current && cellKey(editStart.current.cell) === key) {
        const previous = editStart.current.frame.model.rows.find(
          (row) => row.id === cell.recipientId,
        );
        if (previous) {
          checkpoint();
          if (cell.itemId) {
            setModel((old) => ({
              ...old,
              rows: old.rows.map((row) =>
                row.id === cell.recipientId
                  ? {
                      ...row,
                      items: row.items.map((item) =>
                        item.id === cell.itemId
                          ? structuredClone(previous.items.find((item) => item.id === cell.itemId)!)
                          : item,
                      ),
                    }
                  : row,
              ),
            }));
            setQuantityText((old) => {
              const copy = { ...old };
              delete copy[key];
              return copy;
            });
          } else
            setModel((old) => ({
              ...old,
              rows: old.rows.map((row) =>
                row.id === cell.recipientId
                  ? { ...row, [cell.field]: previous[cell.field as "name"] }
                  : row,
              ),
            }));
        }
      }
      setError(undefined);
      return;
    }
    const navigationKey = event.key;
    if (!isCellNavigationKey(navigationKey)) return;
    event.preventDefault();
    if (
      cell.field === "product" &&
      entry[key] !== undefined &&
      entry[key] !== manualValue(model, cell)
    ) {
      selectProduct(cell, entry[key]);
      return;
    }
    if (pendingRef.current) {
      if (cell.field === "quantity" && (event.key === "Enter" || event.key === "Tab")) {
        if (!(await finishQuantity())) return;
        requestAnimationFrame(() => move(cell, navigationKey, event.shiftKey));
        return;
      } else {
        fail("수량을 확정하거나 상품 선택을 취소하세요.", pendingRef.current);
        return;
      }
    }
    move(cell, navigationKey, event.shiftKey);
  };
  const textCell = (cell: Cell, label: string) => {
    const key = cellKey(cell),
      row = model.rows.find((row) => row.id === cell.recipientId)!,
      required = delivery.byId[row.id] ?? {
        name: false, address: false, contact: false, product: false, quantity: false,
      };
    const item = row.items.find((item) => item.id === cell.itemId);
    const invalid =
      cell.field === "name"
        ? !required.name
        : cell.field === "address"
          ? !required.address
          : ["phone", "mobile"].includes(cell.field)
            ? !required.contact
            : cell.field === "product"
              ? !item?.productId || !item?.variantId
              : cell.field === "quantity"
                ? !!item && quantityReady &&
                  quantityStatus?.items.find((entry) =>
                    entry.recipientId === cell.recipientId &&
                    entry.itemId === cell.itemId)?.valid === false
                : false;
    const value =
      cell.field === "product"
        ? (entry[key] ?? manualValue(model, cell))
        : cell.field === "quantity"
          ? (quantityText[key] ?? manualValue(model, cell))
          : (entry[key] ?? manualValue(model, cell));
    const input = (
      <input
        data-cell={key}
        aria-label={label}
        aria-selected={!!active && cellKey(active) === key}
        aria-invalid={invalid}
        title={invalid ? "필수정보를 확인하세요" : undefined}
        disabled={!allowed(cell)}
        value={value}
        inputMode={cell.field === "quantity" ? "numeric" : undefined}
        onFocus={() => {
          setActive(cell);
          editStart.current = { cell, frame: frame() };
        }}
        onChange={(event) => {
          if (!allowed(cell)) return;
          const value = event.target.value;
          checkpoint();
          setError(undefined);
          if (cell.field === "product") {
            setEntry((old) => {
              const next = { ...old };
              if (value === manualValue(model, cell)) delete next[key];
              else next[key] = value;
              return next;
            });
            return;
          }
          if (cell.field === "quantity") {
            const request = (manualQuantityRequest.current[key] ?? 0) + 1;
            manualQuantityRequest.current[key] = request;
            manualQuantityPending.current.add(key);
            setQuantityText((old) => ({ ...old, [key]: value }));
            if (!pendingRef.current) beforeProduct.current = frame();
            setPending(cell);
            // Incomplete typing is a draft; Rust owns quantity parsing and
            // invalidation of prior approval/quantity-related issues.
            void businessCoreCall<{ model: ManualModel }>(
              "rules", "apply_manual_classification", {
                action: "set_manual_quantity_draft", model, cell, value,
              },
            ).then(({ model: updated }) => {
              if (manualQuantityRequest.current[key] !== request ||
                  latestInputs.current.order !== order) return;
              manualQuantityPending.current.delete(key);
              setModel((current) => applyNativeQuantity(current, updated, cell));
            }).catch((e) => {
              if (manualQuantityRequest.current[key] === request) {
                manualQuantityPending.current.delete(key);
                fail(e instanceof Error ? e.message : String(e), cell);
              }
            });
            return;
          }
          if (["name", "address", "phone", "mobile", "note"].includes(cell.field)) {
            const request = (manualTextRequest.current[key] ?? 0) + 1;
            manualTextRequest.current[key] = request;
            markNativeTextPending(key, true);
            setEntry((old) => ({ ...old, [key]: value }));
            void businessCoreCall<{ model: ManualModel }>(
              "rules", "apply_manual_classification", {
                action: "set_manual_recipient_field", model, cell, value,
              },
            ).then(({ model: updated }) => {
              if (manualTextRequest.current[key] !== request) return;
              if (latestInputs.current.order !== order)
                throw Error("원본 주문이 변경되었습니다. 수동 분류를 다시 열어주세요.");
              const changed = updated.rows.find((row) => row.id === cell.recipientId);
              if (!changed) throw Error("배송 행을 선택하세요.");
              setModel((current) => {
                const next = structuredClone(current);
                const row = next.rows.find((row) => row.id === cell.recipientId);
                if (!row) return current;
                const field = cell.field as "name" | "address" | "phone" | "mobile" | "note";
                row[field] = changed[field];
                if (field === "address") {
                  row.addressParts = changed.addressParts;
                  delete row.addressStatus;
                }
                row.confirmedFields = [...new Set([...(row.confirmedFields ?? []), field])];
                return next;
              });
              setEntry((old) => {
                const next = { ...old };
                delete next[key];
                return next;
              });
            }).catch((e) => {
              if (manualTextRequest.current[key] === request)
                fail(e instanceof Error ? e.message : String(e), cell);
            }).finally(() => {
              // Superseded responses must not release the newer field request.
              if (manualTextRequest.current[key] === request)
                markNativeTextPending(key, false);
            });
            return;
          }
          fail("지원하지 않는 수동 필드입니다.", cell);
        }}
        onBlur={() => {
          if (
            cell.field === "product" &&
            entry[key] !== undefined &&
            entry[key] !== manualValue(model, cell)
          )
            selectProduct(cell, entry[key]);
        }}
        onKeyDown={(event) => keyDown(event, cell)}
      />
    );
    if (cell.field !== "quantity") return input;
    const increment = (delta: number) => {
      if (!allowed(cell) || manualQuantityPending.current.has(key) ||
          nativeTextPending.current.size) return;
      checkpoint();
      const request = (manualQuantityRequest.current[key] ?? 0) + 1;
      manualQuantityRequest.current[key] = request;
      manualQuantityPending.current.add(key);
      void businessCoreCall<{ model: ManualModel }>(
        "rules", "apply_manual_classification", {
          action: "step_manual_quantity", model, cell, delta,
        },
      ).then(({ model: updated }) => {
        if (manualQuantityRequest.current[key] !== request ||
            latestInputs.current.order !== order) return;
        manualQuantityPending.current.delete(key);
        const item = updated.rows.find((row) => row.id === cell.recipientId)
          ?.items.find((item) => item.id === cell.itemId);
        if (!item) {
          fail("수량을 1 이상의 정수로 입력하세요.", cell);
          return;
        }
        setModel((current) => applyNativeQuantity(current, updated, cell));
        setQuantityText((old) => ({ ...old, [key]: String(item.quantity) }));
        setPending(undefined);
        setError(undefined);
        requestAnimationFrame(() => focus(cell));
      }).catch((e) => {
        if (manualQuantityRequest.current[key] === request) {
          manualQuantityPending.current.delete(key);
          fail(e instanceof Error ? e.message : String(e), cell);
        }
      });
    };
    return (
      <div className="manual-quantity-cell">
        <div className="quantity-stepper">
          <button
            aria-label={label + " 감소"}
            disabled={!allowed(cell)}
            onClick={() => increment(-1)}
          >
            −
          </button>
          {input}
          <button
            aria-label={label + " 증가"}
            disabled={!allowed(cell)}
            onClick={() => increment(1)}
          >
            +
          </button>
        </div>
        {pending?.itemId === cell.itemId && (
          <button className="manual-cancel-product" onClick={cancelProduct}>
            상품 선택 취소
          </button>
        )}
      </div>
    );
  };
  const segments = [];
  let cursor = 0;
  for (const block of model.blocks) {
    segments.push((order.analysisRaw ?? order.raw).slice(cursor, block.start));
    segments.push(
      <button
        key={block.id}
        className={"manual-block" + (model.assignments[block.id] ? " assigned" : "")}
        aria-pressed={!!model.assignments[block.id]}
        aria-label={`${model.assignments[block.id] ? "배정됨" : "미배정"} ${block.text}`}
        onPointerDown={(event) => event.preventDefault()}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => useBlock(block.id)}
      >
        {block.text}
      </button>,
    );
    cursor = block.end;
  }
  segments.push((order.analysisRaw ?? order.raw).slice(cursor));
  const unassignedCount = model.blocks.filter((block) => !model.assignments[block.id]).length;
  // Any unacknowledged name/address/contact/note is as important as an
  // uncommitted product: never apply model.rows if the visible text differs.
  const uncommitted = Object.keys(entry).length > 0;
  const issues = nativeOrderIssues.issues.filter(
    (issue) => issue.severity === "ACTION",
  );
  const layer = completion || !!confirm || !!search || nativeDraftBusy;
  return (
    <section
      ref={root}
      className="manual-workspace"
      data-native-text-pending={nativeTextBusy}
      data-native-cell-uncommitted={uncommitted}
      role="dialog"
      aria-modal="false"
      aria-label="수동 분류"
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing || layer) return;
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") {
          event.preventDefault();
          if (!requireNativeCellsSettled()) return;
          const previous = history.current.pop();
          if (previous) {
            restore(previous);
            if (active) focus(active);
          }
        }
      }}
    >
      <div className="manual-workspace-body" inert={layer}>
        <header className="manual-workspace-heading">
          <h2>수동 분류</h2>
          <button
            disabled={nativeTextBusy || nativeDraftBusy || manualQuantityPending.current.size > 0}
            onClick={() => {
              if (!requireNativeCellsSettled()) return;
              setConfirm({
                title: "전체 초기화",
                message: "구성표와 배정을 초기화할까요? 최초 원문과 마스터는 보존됩니다.",
                execute: () => {
                  void nativeStructure("reset_manual_rows", {
                    recipientId: crypto.randomUUID(),
                  }, () => {
                    setCount("1");
                    setActive(undefined);
                    setPending(undefined);
                    setEntry({});
                    setQuantityText({});
                    history.current = [];
                    beforeProduct.current = undefined;
                  });
                },
              });
            }}
          >
            초기화
          </button>
          <button onClick={closeDraft}>닫기 ×</button>
        </header>
        <div className="manual-classification">
          <div className="manual-status" role="status" data-error-cell={error?.key}>
            {pending ? (
              <>
                <span>선택한 상품의 수량을 확정하세요.</span>
              </>
            ) : (
              <div className="manual-help-or-error">
                {error?.message || <strong>미배정됨: {unassignedCount}개</strong>}
              </div>
            )}
            {pending && error && <small>{error.message}</small>}
          </div>
          <div className="manual-columns">
            <section className="manual-source">
              <header className="manual-source-heading">
                <h3>붙여넣은 내용</h3>
              </header>
              <pre>{segments}</pre>
            </section>
            <section className="manual-sheet">
              <div className="manual-sheet-heading">
                <h3>배송 정보 구성표</h3>
                <div className="manual-count">
                  <span>배송 건수</span>
                  <button
                    aria-label="배송 건수 감소"
                    disabled={!!pending || nativeTextBusy || model.rows.length <= 1}
                    onClick={() => resize(model.rows.length - 1)}
                  >
                    −
                  </button>
                  <input
                    aria-label="배송 건수"
                    type="number"
                    min={1}
                    max={100}
                    value={count}
                    disabled={!!pending || nativeTextBusy}
                    onChange={(event) => {
                      if (!requireNativeCellsSettled()) return;
                      setCount(event.target.value);
                      if (event.target.value !== "") resize(Number(event.target.value));
                    }}
                    onBlur={() => setCount(String(model.rows.length))}
                  />
                  <button
                    aria-label="배송 건수 증가"
                    disabled={!!pending || nativeTextBusy || model.rows.length >= 100}
                    onClick={() => resize(model.rows.length + 1)}
                  >
                    +
                  </button>
                </div>
              </div>
              <div className="manual-grid-scroll">
                <table className="manual-delivery-table">
                  <colgroup>
                    <col className="manual-col-shipment" />
                    <col className="manual-col-name" />
                    <col className="manual-col-address" />
                    <col className="manual-col-mobile" />
                    <col className="manual-col-phone" />
                    <col className="manual-col-note" />
                  </colgroup>
                  {model.rows.map((row, index) => (
                    <tbody key={row.id} className="manual-shipment-group">
                      <tr className="manual-shipment-heading">
                        {["배송", "이름", "주소", "휴대폰", "전화", "배송 요청"].map((label) => (
                          <th key={label} scope="col">{label}</th>
                        ))}
                      </tr>
                      <tr className="manual-recipient-row">
                        <th scope="row">배송 {index + 1}</th>
                        {["name", "address", "mobile", "phone", "note"].map((field, n) => (
                          <td key={field}>
                            {textCell(
                              { recipientId: row.id, field },
                              `배송 ${index + 1} ${["이름", "주소", "휴대폰", "전화", "배송 요청"][n]}`,
                            )}
                          </td>
                        ))}
                      </tr>
                      <tr className="manual-product-section">
                        <td colSpan={6}>
                          <div className="manual-products-header">
                            <span>상품</span>
                            <span>수량</span>
                            <span>행 동작</span>
                          </div>
                          {row.items.map((item, itemIndex) => (
                            <div className="manual-product-row" key={item.id}>
                              <div className="manual-product-cell">
                                {textCell(
                                  { recipientId: row.id, itemId: item.id, field: "product" },
                                  `배송 ${index + 1} 상품 ${itemIndex + 1}`,
                                )}
                                <button
                                  disabled={!!pending || nativeTextBusy}
                                  onClick={() => { if (!requireNativeCellsSettled()) return; setSearch({
                                    recipientId: row.id,
                                    itemId: item.id,
                                    field: "product",
                                  }); }}
                                >
                                  검색
                                </button>
                              </div>
                              <div>
                                {textCell(
                                  { recipientId: row.id, itemId: item.id, field: "quantity" },
                                  `배송 ${index + 1} 수량 ${itemIndex + 1}`,
                                )}
                              </div>
                              <div className="manual-product-actions">
                                <button
                                  disabled={!!pending || nativeTextBusy}
                                  onClick={() => removeProduct({
                                    recipientId: row.id,
                                    itemId: item.id,
                                    field: "product",
                                  })}
                                >
                                  상품 제거
                                </button>
                              </div>
                            </div>
                          ))}
                          <div className="manual-add-product">
                            <button disabled={!!pending || nativeTextBusy} onClick={() => addProduct(row.id)}>
                              상품 추가
                            </button>
                          </div>
                        </td>
                      </tr>
                    </tbody>
                  ))}
                </table>
              </div>
            </section>
          </div>
          <footer className="manual-footer">
            <span>적용 후 확인 {issues.length}개 / 정식 접수는 주문 확인 완료
              {quantityError && <small role="alert"> / 수량 검증 오류: {quantityError}</small>}
            </span>
            <button onClick={closeDraft}>닫기</button>
            <button
              className="primary"
              disabled={
                !delivery.ready || !nativeOrderIssues.ready || nativeDraftBusy || nativeTextBusy ||
                !!pending || !model.rows.length || uncommitted ||
                !quantityReady || quantityStatus?.pending === true
              }
              onClick={() => {
                if (!quantityReady || quantityStatus?.pending === true) {
                  fail(quantityError || "Rust 수량 승인 조건을 확인한 뒤 적용하세요.");
                  return;
                }
                if (!requireNativeCellsSettled() || Object.keys(entry).length > 0) {
                  if (Object.keys(entry).length > 0)
                    fail("아직 적용되지 않은 수취인 또는 상품 입력이 있습니다. 다시 확인하세요.");
                  return;
                }
                setCompletion(true);
              }}
            >
              초안에 적용
            </button>
          </footer>
        </div>
      </div>
      {search && (
        <ProductSearchDialog
          surface={ManualPrompt}
          state={state}
          clientId={order.clientId}
          onClose={() => {
            setSearch(undefined);
            if (active) focus(active);
          }}
          onChoose={(product) => selectProduct(search, product.name, product)}
        />
      )}
      {confirm && (
        <ManualPrompt title={confirm.title} onClose={() => setConfirm(undefined)}>
          <p>{confirm.message}</p>
          <div className="manual-confirm-actions">
            <button
              className="primary"
              onClick={() => {
                confirm.execute();
                setConfirm(undefined);
              }}
            >
              확인
            </button>
            <button onClick={() => setConfirm(undefined)}>취소</button>
          </div>
        </ManualPrompt>
      )}
      {completion && (
        <ManualPrompt title="수동 분류 완료" onClose={() => setCompletion(false)}>
          <p>수동 분류 결과를 어떻게 처리할까요?</p>
          <div className="manual-completion-actions">
            <div className="manual-completion-ai">
              <button disabled>AI 자동분류 개선</button>
            </div>
            <button
              className="primary"
              data-autofocus
              disabled={applying.current || nativeDraftBusy}
              onClick={() => {
                if (applying.current) return;
                applying.current = true;
                const submittedOrder = order, submittedModel = model;
                void businessCoreCall<{ order: Order }>(
                  "rules", "apply_manual_classification", {
                    action: "apply_manual", order: submittedOrder, model: submittedModel,
                    assignmentOrder: Object.keys(submittedModel.assignments),
                  },
                ).then(({ order: prepared }) => {
                  if (latestInputs.current.order !== submittedOrder ||
                      latestInputs.current.model !== submittedModel) {
                    throw Error("수동 분류 내용이 변경되었습니다. 최신 내용을 확인하고 다시 적용하세요.");
                  }
                  onApply(prepared);
                  onClose();
                }).catch((e) => {
                  setCompletion(false);
                  fail(e instanceof Error ? e.message : String(e));
                }).finally(() => { applying.current = false; });
              }}
            >
              피드백 없이 진행
            </button>
            <button onClick={() => setCompletion(false)}>취소</button>
          </div>
        </ManualPrompt>
      )}
    </section>
  );
}
