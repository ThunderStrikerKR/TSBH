export type CellNavigationKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Enter" | "Tab";
export type CellNavigationEnterMode = "linear" | "same-column";
export type CellNavigationCell = {
  id: string; row: number; column: number; order: number;
  disabled?: boolean; readOnly?: boolean;
};
type CompositionState = {
  isComposing?: boolean; keyCode?: number; nativeEvent?: { isComposing?: boolean };
};
export function isCellNavigationKey(key: string): key is CellNavigationKey {
  return key === "ArrowLeft" || key === "ArrowRight" || key === "ArrowUp" ||
    key === "ArrowDown" || key === "Enter" || key === "Tab";
}
export function isImeComposing(event: CompositionState): boolean {
  return event.nativeEvent?.isComposing === true || event.isComposing === true || event.keyCode === 229;
}
/** Resolve from explicit screen-owned coordinates; never infer a grid from DOM order. */
export function resolveCellNavigationTarget(
  cells: readonly CellNavigationCell[], currentId: string, key: CellNavigationKey,
  shift = false, enterMode: CellNavigationEnterMode = "linear",
): string | null {
  const editable = cells.filter((cell) => !cell.disabled && !cell.readOnly)
    .slice().sort((a, b) => a.order - b.order);
  const current = editable.find((cell) => cell.id === currentId);
  if (!current) return null;
  const linear = (direction: -1 | 1) => {
    const candidates = editable.filter((cell) => direction < 0
      ? cell.order < current.order : cell.order > current.order);
    candidates.sort((a, b) => direction < 0 ? b.order - a.order : a.order - b.order);
    return candidates[0]?.id ?? null;
  };
  const vertical = (direction: -1 | 1) => {
    const candidates = editable.filter((cell) => cell.column === current.column &&
      (direction < 0 ? cell.row < current.row : cell.row > current.row));
    candidates.sort((a, b) => direction < 0
      ? b.row - a.row || a.order - b.order : a.row - b.row || a.order - b.order);
    return candidates[0]?.id ?? null;
  };
  switch (key) {
    case "ArrowLeft": {
      const candidates = editable.filter((cell) => cell.row === current.row && cell.column < current.column);
      candidates.sort((a, b) => b.column - a.column || b.order - a.order);
      return candidates[0]?.id ?? null;
    }
    case "ArrowRight": {
      const candidates = editable.filter((cell) => cell.row === current.row && cell.column > current.column);
      candidates.sort((a, b) => a.column - b.column || a.order - b.order);
      return candidates[0]?.id ?? null;
    }
    case "ArrowUp": return vertical(-1);
    case "ArrowDown": return vertical(1);
    case "Enter": return enterMode === "same-column" ? vertical(shift ? -1 : 1) : linear(shift ? -1 : 1);
    case "Tab": return linear(shift ? -1 : 1);
  }
}
export function findCellNavigationTarget(
  current: HTMLElement, key: CellNavigationKey, shift = false,
  enterMode: CellNavigationEnterMode = "linear",
): HTMLElement | null {
  const grid = current.closest<HTMLElement>("[data-cell-nav-grid]");
  const currentId = current.dataset.cellNavId;
  if (!grid || !currentId) return null;
  const nodes = Array.from(grid.querySelectorAll<HTMLElement>("[data-cell-nav-id]"))
    .filter((node) => !node.closest("[hidden], [aria-hidden='true']"));
  const cells: CellNavigationCell[] = [];
  for (const node of nodes) {
    const row = Number(node.dataset.cellNavRow), column = Number(node.dataset.cellNavColumn);
    const order = Number(node.dataset.cellNavOrder);
    if (!node.dataset.cellNavId || !Number.isFinite(row) || !Number.isFinite(column) || !Number.isFinite(order)) continue;
    const control = node as HTMLInputElement | HTMLTextAreaElement;
    cells.push({
      id: node.dataset.cellNavId, row, column, order,
      disabled: control.disabled || node.getAttribute("aria-disabled") === "true",
      readOnly: control.readOnly,
    });
  }
  const targetId = resolveCellNavigationTarget(cells, currentId, key, shift, enterMode);
  return targetId ? nodes.find((node) => node.dataset.cellNavId === targetId) ?? null : null;
}
export function focusCellElement(target: HTMLElement | null | undefined): boolean {
  if (!target || !target.isConnected) return false;
  target.focus({ preventScroll: true });
  target.scrollIntoView({ block: "nearest", inline: "nearest" });
  return true;
}

export type CellNavigationKeyboardEvent = CompositionState & {
  key: string;
  shiftKey: boolean;
  currentTarget: HTMLElement;
  preventDefault: () => void;
  stopPropagation?: () => void;
};
export type CellNavigationKeyHandlerOptions = {
  enterMode?: CellNavigationEnterMode;
  resolveTarget?: (
    current: HTMLElement, key: CellNavigationKey, shift: boolean,
    enterMode: CellNavigationEnterMode,
  ) => HTMLElement | null;
  focusTarget?: (target: HTMLElement | null) => boolean;
  onEscape?: (event: CellNavigationKeyboardEvent) => void;
  onBeforeMove?: (
    key: CellNavigationKey, target: HTMLElement | null,
    event: CellNavigationKeyboardEvent,
  ) => boolean | void | Promise<boolean | void>;
  onBoundary?: (key: CellNavigationKey, event: CellNavigationKeyboardEvent) => void;
};
/** Shared React key-event policy; screens supply only save/cancel side effects. */
export function handleCellNavigationKey(
  event: CellNavigationKeyboardEvent,
  options: CellNavigationKeyHandlerOptions = {},
): boolean {
  if (isImeComposing(event)) return false;
  if (event.key === "Escape" && options.onEscape) {
    event.preventDefault();
    options.onEscape(event);
    return true;
  }
  if (!isCellNavigationKey(event.key)) return false;
  const key = event.key;
  const enterMode = options.enterMode ?? "linear";
  const resolveTarget = options.resolveTarget ?? findCellNavigationTarget;
  const target = resolveTarget(event.currentTarget, key, event.shiftKey, enterMode);
  const beforeMove = options.onBeforeMove?.(key, target, event);
  const isPending = (value: boolean | void | Promise<boolean | void>): value is Promise<boolean | void> =>
    !!value && typeof value === "object" && "then" in value &&
    typeof (value as Promise<boolean | void>).then === "function";
  const complete = (allowed: boolean | void) => {
    if (allowed === false) return;
    if (target) {
      (options.focusTarget ?? focusCellElement)(target);
    } else if (key === "Enter") {
      options.onBoundary?.(key, event);
    }
  };
  // Let Tab leave the grid naturally. Still run the screen's commit gate;
  // if an asynchronous gate rejects, restore focus to the cell being edited.
  if (key === "Tab" && !target) {
    if (beforeMove === false) {
      event.preventDefault();
      return true;
    }
    if (isPending(beforeMove)) {
      void Promise.resolve(beforeMove).then(
        (allowed) => { if (allowed === false) focusCellElement(event.currentTarget); },
        () => focusCellElement(event.currentTarget),
      );
    }
    return false;
  }
  if (target || key !== "Tab") event.preventDefault();
  if (beforeMove === false) return true;
  if (isPending(beforeMove)) {
    void Promise.resolve(beforeMove).then(complete, () => undefined);
    return true;
  }
  complete(beforeMove);
  return true;
}
