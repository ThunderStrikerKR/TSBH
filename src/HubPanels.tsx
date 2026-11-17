import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { useViewportScale } from "./shared/useViewportScale";

export const defaultPanels = [20, 80 / 3, 80 / 3, 80 / 3];
const SPLIT_WIDTH = 6;
// The order list must retain a single-line header with a ten-character dealer
// name, four stage indicators, the manage button and the delete button.
export const panelMinimumWidths = [200, 250, 250, 440];

export function validPanels(value: unknown): number[] {
  return Array.isArray(value) &&
    value.length === 4 &&
    value.every((n) => typeof n === "number" && Number.isFinite(n) && n > 0)
    ? value
    : defaultPanels;
}

/** Translate physical splitter movement into persistent four-panel ratios.
 * Only the two neighboring columns change; all other widths stay fixed.
 * A hidden Kakao column retains its proportional weight in external mode. */
export function resizePanelRatios(
  ratios: number[],
  shown: number[],
  measuredWidths: number[],
  splitIndex: number,
  deltaPx: number,
  minima: number[],
): number[] {
  const current = validPanels(ratios);
  if (splitIndex < 0 || splitIndex >= shown.length - 1 ||
      measuredWidths.length !== shown.length ||
      measuredWidths.some((v) => !Number.isFinite(v) || v <= 0) ||
      !Number.isFinite(deltaPx)) return current;
  const left = shown[splitIndex];
  const right = shown[splitIndex + 1];
  const pairWidth = measuredWidths[splitIndex] + measuredWidths[splitIndex + 1];
  const minLeft = minima[left];
  const minRight = minima[right];
  const leftWidth = Math.max(minLeft,
    Math.min(pairWidth - minRight, measuredWidths[splitIndex] + deltaPx));
  if (leftWidth < minLeft || pairWidth - leftWidth < minRight ||
      Math.abs(leftWidth - measuredWidths[splitIndex]) < 0.5) return current;
  const nextWidths = [...measuredWidths];
  nextWidths[splitIndex] = leftWidth;
  nextWidths[splitIndex + 1] = pairWidth - leftWidth;

  const sourceWeight = shown.reduce((sum, index) => sum + current[index], 0);
  const widthScale = measuredWidths.reduce((sum, value) => sum + value, 0) / sourceWeight;
  const next = current.map((value) => value * widthScale);
  for (const [index, panel] of shown.entries()) next[panel] = nextWidths[index];
  const sum = next.reduce((total, value) => total + value, 0);
  return next.map((value) => 100 * value / sum);
}

type SplitDrag = {
  index: number;
  pointerId: number;
  originX: number;
  widths: number[];
  base: number[];
  current: number[] | null;
};

export function HubPanels({
  children,
  externalKakao = false,
  ratios,
  onSave,
  onReset,
}: {
  children: ReactNode[];
  externalKakao?: boolean;
  ratios: number[];
  onSave: (r: number[]) => void;
  onReset: () => void;
}) {
  const viewportScale = useViewportScale();
  const minima = panelMinimumWidths.map((value) => value * viewportScale);
  const shown = externalKakao ? [1, 2, 3] : [0, 1, 2, 3];
  const [previewRatios, setPreviewRatios] = useState<number[] | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const drag = useRef<SplitDrag | null>(null);
  const currentRatios = validPanels(previewRatios ?? ratios);
  const total = shown.reduce((sum, index) => sum + currentRatios[index], 0);
  const minWidth = shown.reduce((sum, index) => sum + minima[index], 0) +
    (shown.length - 1) * SPLIT_WIDTH;

  useEffect(() => {
    if (drag.current || !previewRatios) return;
    if (validPanels(ratios).every((value, index) =>
      Math.abs(value - previewRatios[index]) < 0.001)) setPreviewRatios(null);
  }, [ratios, previewRatios]);

  const measuredWidths = () => Array.from(
    root.current?.querySelectorAll<HTMLElement>(".hub-panel-slot") ?? [],
    (slot) => slot.getBoundingClientRect().width,
  );

  const commitKeyboard = (index: number, delta: number) => {
    const next = resizePanelRatios(
      currentRatios, shown, measuredWidths(), index, delta, minima,
    );
    if (next.some((value, i) => Math.abs(value - currentRatios[i]) > 0.001)) {
      setPreviewRatios(next);
      onSave(next);
    }
  };

  return (
    <div className="hub-workspace-scroll">
      <div
        ref={root}
        className="hub-panels"
        style={{
          minWidth,
          gridTemplateColumns: shown
            .flatMap((i, index) => [
              `minmax(${minima[i]}px,${currentRatios[i] / total}fr)`,
              ...(index < shown.length - 1 ? [`${SPLIT_WIDTH}px`] : []),
            ])
            .join(" "),
        } as CSSProperties}
      >
        {shown.flatMap((panel, index) => [
          <div className={"hub-panel-slot hub-slot-" + panel} key={"panel" + panel}>
            {children[panel]}
          </div>,
          ...(index < shown.length - 1
            ? [
                <div
                  key={"split" + panel}
                  role="separator"
                  aria-label="업무 열 너비 조절"
                  aria-orientation="vertical"
                  aria-valuemin={Math.round(minima[panel])}
                  tabIndex={0}
                  className="hub-split"
                  title="드래그 또는 방향키로 열 너비 조절 / 두 번 클릭 초기화"
                  onPointerDown={(event) => {
                    if (event.pointerType === "mouse" && event.button !== 0) return;
                    const widths = measuredWidths();
                    if (widths.length !== shown.length) return;
                    drag.current = {
                      index, pointerId: event.pointerId, originX: event.clientX,
                      widths, base: [...currentRatios], current: null,
                    };
                    event.currentTarget.setPointerCapture(event.pointerId);
                    event.preventDefault();
                  }}
                  onPointerMove={(event) => {
                    const state = drag.current;
                    if (!state || state.index !== index ||
                        state.pointerId !== event.pointerId) return;
                    const next = resizePanelRatios(
                      state.base, shown, state.widths, index,
                      event.clientX - state.originX, minima,
                    );
                    state.current = next;
                    setPreviewRatios(next);
                  }}
                  onPointerUp={(event) => {
                    const state = drag.current;
                    if (!state || state.index !== index ||
                        state.pointerId !== event.pointerId) return;
                    drag.current = null;
                    if (event.currentTarget.hasPointerCapture(event.pointerId))
                      event.currentTarget.releasePointerCapture(event.pointerId);
                    if (state.current && state.current.some((value, i) =>
                      Math.abs(value - state.base[i]) > 0.001)) onSave(state.current);
                  }}
                  onPointerCancel={(event) => {
                    if (drag.current?.pointerId !== event.pointerId) return;
                    drag.current = null;
                    setPreviewRatios(null);
                  }}
                  onKeyDown={(event) => {
                    const step = event.shiftKey ? 32 : 12;
                    if (event.key === "ArrowLeft") {
                      event.preventDefault();
                      commitKeyboard(index, -step);
                    } else if (event.key === "ArrowRight") {
                      event.preventDefault();
                      commitKeyboard(index, step);
                    }
                  }}
                  onDoubleClick={() => {
                    drag.current = null;
                    setPreviewRatios(null);
                    onReset();
                  }}
                />,
              ]
            : []),
        ])}
      </div>
    </div>
  );
}
