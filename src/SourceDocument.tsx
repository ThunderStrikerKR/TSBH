import { flushSync } from "react-dom";
import { HelpTooltip } from "./ui/HelpTooltip";
import { RetailOrderDialog } from "./orders/RetailOrderDialog";
import { OcrReviewDialog } from "./OcrReviewDialog";
import { Modal } from "./ui";
import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ClientChooser } from "./ClientChooser";
import { KakaoClients } from "./KakaoClients";
import { LinkedText, sourceDocumentSpans } from "./LinkedText";
import { useReadyContentFade } from "./shared/useReadyContentFade";
import { ClientPicker, TierBadge } from "./ManagementUI";
import { confirmAction } from "./dialogs";
import { businessCoreCall } from "./tauri-bridge";
import type { NoticeSink } from "./shared/notices";
import { nativeAutoReceptionClient } from "./native-title-clients";
import type { Client, DockWindow, OcrCapture, Order, State } from "./types";

export function SourceDocument({
  state,
  order,
  text,
  client,
  contentReady = true,
  onText,
  onOrder,
  onDealer,
  onSaveState,
  onNotice,
  onManual,
  onNew,
  onSettings,
}: {
  state: State;
  order: Order;
  text: string;
  client?: Client;
  contentReady?: boolean;
  onText: (text: string) => void;
  onOrder: (order: Order) => void;
  onDealer: (client: Client) => Promise<void> | void;
  onSave: (state: State) => Promise<void>;
  onSaveState: (state: State) => Promise<void>;
  onNotice: NoticeSink;
  onManual?: () => void;
  onNew?: () => void;
  onSettings?: () => void;
}) {
  const [retail, setRetail] = useState(false);
  const [ocrResult, setOcrResult] = useState<OcrCapture | null>(null);
  const [ocrPhase, setOcrPhase] = useState<"idle" | "selecting" | "recognizing">("idle");
  const [retailDraft, setRetailDraft] = useState<Order>();
  const settingsLink = useRef(onSettings);
  settingsLink.current = onSettings;
  const [dealer, setDealer] = useState(false),
    [busy, setBusy] = useState(false);
  const ocrOrderType = useRef<"AGENCY" | "DEALER">("AGENCY");
  const pending = useRef(false),
    alive = useRef(true);
  const latestReception = useRef({ order, text });
  latestReception.current = { order, text };
  const nativeReplaceReception = async (source: Order, raw: string): Promise<Order> => {
    const { order: next } = await businessCoreCall<{ order: Order }>(
      "rules", "apply_reception_action", {
        action: "replace_reception", order: source,
        raw, now: new Date().toISOString(),
      },
    );
    return next;
  };
  const [windows, setWindows] = useState<DockWindow[]>([]);
  const watchedImageBusy = useRef(false);
  const [error, setError] = useState("");
  const [ocrFailure, setOcrFailure] = useState("");
  const [showOcrFailure, setShowOcrFailure] = useState(false);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (!text) {
      setWindows([]);
      setError("");
    }
  }, [text]);
  const formal = !!order.confirmedAt || state.orders.some((o) => o.id === order.id);
  const waitingForSource = !formal && !(order.analysisRaw ?? text).trim();
  // Keep the source frame and intake guidance fixed. A pasted/replaced
  // source becomes visible only after the matching Rust projection settles.
  // Editing unrelated fields must not restart the raw text fade.
  const sourceKey = [order.id, order.analysisRaw ?? order.raw, order.clientId].join(":");
  const sourceFadePhase = useReadyContentFade(sourceKey, contentReady || waitingForSource, true);
  // An inline filter allocated a new array on every repaint, restarting the
  // Rust client_candidates search even when the catalog had not changed.
  const activeClients = useMemo(
    () => state.clients.filter((candidate) => candidate.active !== false),
    [state.clients],
  );
  useEffect(() => {
    if (!text.trim() || client || formal) return;
    let current = true;
    const timer = setInterval(
      () =>
        void window
          .bando!.dockWindows()
          .then((rows) => {
            if (current)
              setWindows((previous) =>
                JSON.stringify(previous) === JSON.stringify(rows) ? previous : rows,
              );
          })
          .catch(() => {}),
      2000,
    );
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [!!text.trim(), client?.id, formal]);

  // New images are recognized only for empty drafts, and only after stable file observations.
  // The OCR review dialog still owns the final accept/reject decision.
  useEffect(() => {
    if (!waitingForSource || !state.settings.orderImageInboxDirectory) return;
    let disposed = false;
    const scan = async () => {
      if (disposed || watchedImageBusy.current || pending.current || ocrResult || !window.bando?.orderImageWatchScan) return;
      watchedImageBusy.current = true;
      try {
        const filenames = await window.bando.orderImageWatchScan();
        if (disposed || !filenames.length || !window.bando.orderImageOcr) return;
        ocrOrderType.current = order.orderType === "DEALER" ? "DEALER" : "AGENCY";
        setOcrPhase("recognizing");
        const capture = await window.bando.orderImageOcr(filenames[0]);
        if (disposed || latestReception.current.order.id !== order.id) return;
        if (!capture.text.trim()) throw Error("자동 수신 이미지에서 문자를 인식하지 못했습니다.");
        setOcrResult(capture);
        setError("");
      } catch (cause) {
        if (!disposed) setError("발주 이미지 자동 인식 오류: " + String(cause).replace(/^Error: /, ""));
      } finally {
        watchedImageBusy.current = false;
        if (!disposed) setOcrPhase("idle");
      }
    };
    void scan();
    const timer = setInterval(() => void scan(), 4000);
    return () => { disposed = true; clearInterval(timer); };
  }, [waitingForSource, state.settings.orderImageInboxDirectory, order.id, order.orderType, ocrResult]);
  const captureOcr = async () => {
    if (formal || pending.current) return;
    pending.current = true;
    setBusy(true);
    ocrOrderType.current = order.orderType === "DEALER" ? "DEALER" : "AGENCY";
    setOcrPhase("selecting");
    setError("");
    setOcrFailure("");
    let stopListening: (() => void) | undefined;
    try {
      if (!window.bando?.captureOcr) throw Error("이 버전에서는 OCR 화면 캡처를 사용할 수 없습니다.");
      // Subscribe before invoking Rust. The app is minimized for Windows
      // capture; only native recognition start makes the OCR progress visible.
      stopListening = await listen<string>("bando-ocr-phase", (event) => {
        if (alive.current && event.payload === "recognizing")
          setOcrPhase("recognizing");
      });
      const result = await window.bando.captureOcr();
      if (!alive.current) return;
      if (!result.text.trim()) {
        setError("선택한 화면에서 글자를 인식하지 못했습니다. 다시 범위를 지정해 주세요.");
        return;
      }
      setOcrResult(result);
    } catch (cause) {
      if (alive.current) {
        const detail = String(cause).replace(/^Error: /, "");
        // Esc in Windows Snipping Tool and the explicit OCR cancel button
        // are a normal, silent abort. Preserve the current order untouched.
        if (detail !== "OCR_CANCELLED") {
          setError("OCR 처리에 실패했습니다.");
          setOcrFailure(detail);
          setShowOcrFailure(true);
        }
      }
    } finally {
      stopListening?.();
      pending.current = false;
      if (alive.current) {
        setBusy(false);
        setOcrPhase("idle");
      }
    }
  };
  const applyOcr = async (edited: string, mode: "replace" | "append") => {
    if (formal || pending.current || !ocrResult) return;
    const cleaned = edited.trim();
    if (!cleaned) {
      setError("OCR 결과가 비어 있습니다. 내용을 확인해 주세요.");
      return;
    }
    const raw = mode === "append" && text.trim() ? text.trimEnd() + "\n" + cleaned : cleaned;
    if (raw.length > 20000) {
      setError("발주 원문은 최대 20,000자까지 입력할 수 있습니다.");
      return;
    }
    if (mode === "replace" && text.trim() && raw !== text &&
      !(await confirmAction("기존 미확정 원문과 분석 결과를 OCR 내용으로 교체할까요?"))) return;
    if (formal) return;
    pending.current = true;
    setBusy(true);
    try {
      const next = await nativeReplaceReception(order, raw);
      next.orderType = ocrOrderType.current;
      if (!alive.current || (latestReception.current.order.id !== order.id ||
              latestReception.current.order.raw !== order.raw) ||
          latestReception.current.text !== text) return;
      onText(raw);
      onOrder(next);
      setOcrResult(null);
      setError("");
      onNotice("검토한 OCR 문구를 발주 원문에 반영했습니다.", "success");
    } catch (error) {
      if (alive.current) setError(String(error).replace(/^Error: /, ""));
    } finally {
      pending.current = false;
      if (alive.current) setBusy(false);
    }
  };
  const paste = async (value?: string, orderType = order.orderType ?? "AGENCY") => {
    if (formal || pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      let raw = value ?? "";
      if (value === undefined) {
        try { raw = await window.bando!.readClipboard(); }
        catch { /* A clipboard image has no text payload. */ }
        if (!raw.trim()) {
          if (!window.bando?.clipboardOcr) throw Error("클립보드에서 텍스트나 이미지를 읽을 수 없습니다.");
          ocrOrderType.current = orderType === "DEALER" ? "DEALER" : "AGENCY";
          setOcrPhase("recognizing");
          const capture = await window.bando.clipboardOcr();
          if (!capture.text.trim()) throw Error("클립보드 이미지에서 문자를 인식하지 못했습니다.");
          if (alive.current && latestReception.current.order.id === order.id) {
            setOcrResult(capture);
            setError("");
          }
          return;
        }
      }
      if (!raw.trim()) throw Error("클립보드에 발주 텍스트가 없습니다.");
      if (
        text &&
        raw !== text &&
        !(await confirmAction("현재 초안과 분석 결과를 새 원문으로 교체할까요?"))
      )
        return;
      let next = raw === text ? structuredClone(order) : await nativeReplaceReception(order, raw);
      next.orderType = orderType;
      let chats: DockWindow[] = [];
      let activeChatHandle: string | undefined;
      try {
        // A visible chat can differ from the KakaoTalk main window. Use the
        // dock's active conversation HWND; never pick an arbitrary open chat.
        const [rows, dock] = await Promise.all([
          window.bando!.dockWindows(), window.bando!.dockStatus(),
        ]);
        chats = rows;
        activeChatHandle = dock.activeChatHandle;
      } catch {
        // KakaoTalk is optional; labeled sender evidence can still match.
      }
      if (!alive.current || (latestReception.current.order.id !== order.id ||
              latestReception.current.order.raw !== order.raw) ||
          latestReception.current.text !== text) return;
      setWindows(chats);
      if (!order.clientId) {
        try {
          const result = await nativeAutoReceptionClient(
            state, chats, raw, activeChatHandle,
          );
          if (!alive.current || (latestReception.current.order.id !== order.id ||
              latestReception.current.order.raw !== order.raw) ||
              latestReception.current.text !== text) return;
          const matched = state.clients.find(
            (c) => c.id === result.clientId && !c.deletedAt && c.active !== false,
          );
          if (matched) {
            // Rust still owns dealer change, sender/payment defaults and
            // freight invalidation. Resolve it BEFORE the one React commit:
            // paste -> dealer -> analysis must not show three partial drafts.
            const selected = await businessCoreCall<{ order: Order }>(
              "rules", "order_edit_rules", {
                action: "change_order_client", state,
                order: next, dealerId: matched.id,
                matchEvidence: result,
                now: new Date().toISOString(),
              },
            );
            next = selected.order;
          } else {
            // Rust distinguishes a missing match from conflicting strong evidence.
            // A conflict is an ACTION item in the first-stage review.
            const staleDealer = !!result.clientId;
            next.clientMatch = {
              source: staleDealer ? "ERROR" : result.source, clientId: null,
              detail: staleDealer
                ? "매칭된 거래처를 찾지 못했습니다. 직접 선택하세요."
                : result.detail,
            };
          }
        } catch (error) {
          // Auto-matching is optional. Keep the pasted order and let staff
          // select a dealer; a missing/failed suggestion is not a reception
          // error and must not display a blocking red toast.
          console.warn("거래처 자동 연결을 건너뛰었습니다.", error);
          next.clientMatch = {
            source: "ERROR", clientId: null,
            detail: "거래처 자동 매칭에 실패했습니다. 직접 선택하세요.",
          };
        }
      }
      if (!alive.current || (latestReception.current.order.id !== order.id ||
              latestReception.current.order.raw !== order.raw) ||
          latestReception.current.text !== text) return;
      setError("");
      onText(raw);
      onOrder(next);
    } catch (error) {
      setError(String(error).replace(/^Error: /, ""));
    } finally {
      pending.current = false;
      setBusy(false);
      setOcrPhase("idle");
    }
  };
  return (
    <section className={"desk-pane source-pane" + (text ? " received-source" : "")}>
      {retail && (
        <RetailOrderDialog
          state={state}
          initialOrder={retailDraft}
          onDraft={setRetailDraft}
          onSave={onSaveState}
          onClose={() => {
            setRetail(false);
            setRetailDraft(undefined);
          }}
          onSettings={
            onSettings
              ? () => {
                  flushSync(() => setRetail(false));
                  settingsLink.current?.();
                }
              : undefined
          }
        />
      )}
      <header className="pane-heading source-heading">
        <h2 className="source-help-heading">발주 원문 <HelpTooltip title="발주서 접수" purpose="거래처에서 받은 주문 내용을 입력하고, 주문할 상품과 받는 사람 정보를 확인합니다." when="카카오톡이나 문자, 메일, 팩스 등으로 주문이 들어왔을 때 사용합니다." steps={["대리발송이나 대리점 납품 등 주문에 맞는 입력 방식을 선택합니다.", "주문 내용을 붙여넣거나 캡처 범위 지정을 눌러 화면 글자를 인식합니다.", "거래처를 확인하고 수취인과 상품 내용을 검토합니다.", "확인이 필요한 항목을 수정한 뒤 주문을 진행합니다."]} result="주문 원문을 바탕으로 상품과 수취인 정보를 확인할 수 있습니다." caution="화면에서 글자를 인식하더라도 잘못 읽은 내용은 사용자가 직접 확인해야 합니다." /></h2>
        {onNew && (
          <button type="button" className="source-new-order" data-ui-role="action-emphasized"
            onClick={onNew}>＋ 새 주문</button>
        )}
      </header>
      <div className="source-top" aria-label="현재 거래처 영역">
        {!!text &&
          (client ? (
            <div className="receipt-client" data-content-fade={sourceFadePhase}
              aria-busy={sourceFadePhase === "pending"} inert={sourceFadePhase === "pending"}>
              <small>현재 거래처</small>
              <b title={client.name}>{client.name}</b>
              <TierBadge client={client} />
              <button disabled={busy} onClick={() => setDealer(true)}>
                변경
              </button>
            </div>
          ) : (
            <div className="source-client-actions" data-content-fade={sourceFadePhase}
              aria-busy={sourceFadePhase === "pending"} inert={sourceFadePhase === "pending"}>
              <button disabled={busy} onClick={() => setDealer(true)}>
                거래처 찾기
              </button>
              {!formal && !busy && (
                <ClientPicker
                  compact
                  clients={activeClients}
                  raw={text}
                  onSelect={(c) =>
                    void Promise.resolve()
                      .then(() => onDealer(c))
                      .catch((e) => setError(String(e)))
                  }
                />
              )}
              {!formal && (
                <KakaoClients
                  state={state}
                  order={order}
                  windows={windows}
                  onWindows={setWindows}
                  onSelect={async (id) => {
                    const c = state.clients.find((c) => c.id === id);
                    if (c) await onDealer(c);
                  }}
                  onSave={onSaveState}
                  onNotice={onNotice}
                />
              )}
            </div>
          ))}
      </div>
      <div
        className={"source-document" + (waitingForSource ? " source-awaiting-input" : "")}
        tabIndex={waitingForSource ? 0 : undefined}
        aria-label={waitingForSource ? "새 발주서 입력 대기" : undefined}
        onPaste={(event) => {
          event.preventDefault();
          event.stopPropagation();
          if (!formal) void paste(event.clipboardData.getData("text/plain") || undefined);
        }}
      >
        {waitingForSource ? (

          <div className="source-intake-choice" aria-label="발주서 입력 방법">
            <div className="source-intake-inner">
              <section className="source-intake-group" aria-label="클립보드 접수">
                <h3>클립보드 접수</h3>
                <p>복사한 텍스트와 이미지를 자동으로 구분해 인식합니다.</p>
                <div className="source-intake-buttons">
                  <button type="button" data-ui-role="action-emphasized"
                    disabled={busy} onClick={() => void paste(undefined, "AGENCY")}>대리발송</button>
                  <button type="button" data-ui-role="action-emphasized"
                    disabled={busy} onClick={() => void paste(undefined, "DEALER")}>대리점 납품</button>
                </div>
                <div className="source-intake-shortcuts">
                  <span>이미지 캡처는</span>
                  <span className="source-intake-key-group" aria-label="Ctrl + Shift + S">
                    <kbd className="source-intake-keycap">Ctrl</kbd>
                    <span>+</span>
                    <kbd className="source-intake-keycap">Shift</kbd>
                    <span>+</span>
                    <kbd className="source-intake-keycap">S</kbd>
                  </span>
                  <span>또는</span>
                  <kbd className="source-intake-keycap">Print Screen</kbd>
                  <span>으로 복사하세요.</span>
                </div>
              </section>
              <section className="source-intake-group source-intake-other" aria-label="다른 입력 방법">
                <h3>다른 방법으로 접수</h3>
                <div className="source-intake-buttons">
                  <button type="button" data-ui-role="action-emphasized"
                    disabled={busy} onClick={() => setRetail(true)}>직접 입력</button>
                  <button type="button" data-ui-role="action-emphasized"
                    disabled={busy} onClick={() => void captureOcr()}>캡처 범위 지정</button>
                </div>
              </section>
              <section className="source-intake-group source-intake-folder" aria-label="폴더 자동 인식">
                <h3>폴더 자동 인식</h3>
                {state.settings.orderImageInboxDirectory?.trim() ? (
                  <>
                    <button type="button" className="source-intake-folder-path" data-ui-role="action-standard"
                      title={state.settings.orderImageInboxDirectory}
                      aria-label={`지정된 발주 이미지 폴더 열기: ${state.settings.orderImageInboxDirectory}`}
                      onClick={() => {
                        void window.bando!.openOrderImageInboxDirectory().catch((cause) =>
                          setError("폴더 열기 실패: " + String(cause).replace(/^Error: /, "")));
                      }}>
                      {state.settings.orderImageInboxDirectory}
                    </button>
                    <p>이 폴더에 이미지를 저장하면 자동으로 이미지 인식을 실행합니다.</p>
                  </>
                ) : (
                  <p className="source-intake-hint">지정된 폴더가 없습니다. 설정 · 파일 관리에서 발주 이미지 수신 폴더를 지정하세요.</p>
                )}
              </section>
            </div>
          </div>
        ) : (
          <div className="source-raw-content" data-content-fade={sourceFadePhase}
            aria-busy={sourceFadePhase === "pending"} inert={sourceFadePhase === "pending"}>
            <LinkedText
              readOnly
              label="주문 원문"
              text={order.analysisRaw ?? order.raw}
              spans={sourceDocumentSpans(order.analysisRaw ?? order.raw, order)}
              onChange={() => {}}
            />
          </div>
        )}
        {ocrPhase === "recognizing" && (
          <div className="source-ocr-progress" role="status" aria-live="polite" aria-label="OCR 진행 상태">
            <strong>OCR 처리 중…</strong>
            <span>선택한 이미지에서 글자를 인식하고 있습니다.</span>
            <button type="button" onClick={() => void window.bando?.cancelOcr?.()}>
              취소
            </button>
          </div>
        )}
      </div>
      {order.analysisRaw && order.analysisRaw !== order.raw && (
        <small>작업용 정정 원문 / 최초 접수 원문 보존</small>
      )}
      <div className="source-inline-status" role="status" title={error}>
        {error}
        {!!ocrFailure && (
          <button type="button" className="ocr-error-detail-button"
            onClick={() => setShowOcrFailure(true)}>
            오류 상세
          </button>
        )}
      </div>
      {!formal && !!text && (
        <div className="source-actions">
          {!!text && (
            <button
              disabled={busy}
              onClick={async () => {
                if (pending.current || busy) return;
                if (!(await confirmAction("현재 미확정 원문과 분석 결과를 비울까요?"))) return;
                pending.current = true;
                setBusy(true);
                try {
                  const cleared = await nativeReplaceReception(order, "");
                  if (!alive.current || (latestReception.current.order.id !== order.id ||
              latestReception.current.order.raw !== order.raw) ||
                      latestReception.current.text !== text) return;
                  onText("");
                  onOrder(cleared);
                } catch (error) {
                  if (alive.current) setError(String(error).replace(/^Error: /, ""));
                } finally {
                  pending.current = false;
                  if (alive.current) setBusy(false);
                }
              }}
            >
              내용 삭제
            </button>
          )}
          {!!text && (
            <span className="source-actions-right">
              <button disabled={busy || order.analysisRaw !== text || !onManual} onClick={onManual}>
                수동 분류
              </button>
              <button disabled title="추후 지원 예정">
                AI 분석
              </button>
            </span>
          )}
        </div>
      )}
      {formal && <div className="source-footer-space" aria-hidden="true" />}
      {showOcrFailure && (
        <Modal title="OCR 실행 오류" onClose={() => setShowOcrFailure(false)}>
          <p>OCR 실행 중 오류가 발생했습니다. 기존 발주 원문과 주문 데이터는 변경되지 않았습니다.</p>
          <pre className="ocr-error-detail">{ocrFailure}</pre>
          <div className="actions">
            <button type="button" onClick={() => void window.bando?.copy(ocrFailure)}>
              오류 내용 복사
            </button>
            <button type="button" onClick={() => setShowOcrFailure(false)}>닫기</button>
          </div>
        </Modal>
      )}
      {ocrResult && !formal && (
        <OcrReviewDialog
          result={ocrResult}
          onCancel={() => setOcrResult(null)}
          onApply={(edited, mode) => void applyOcr(edited, mode)}
        />
      )}
      {dealer && (
        <ClientChooser
          clients={state.clients}
          current={client}
          onSelect={onDealer}
          onClose={() => setDealer(false)}
        />
      )}
    </section>
  );
}
