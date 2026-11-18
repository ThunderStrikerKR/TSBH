import { useEffect, useState } from "react";
import type { NoticeSink } from "./shared/notices";
import { nativeRememberTitle, nativeKakaoManualCandidates, type NativeKakaoManualCandidates } from "./native-title-clients";
import type { DockWindow, Order, State, Client } from "./types";
import { Modal } from "./ui";
export function KakaoClients({
  state,
  order,
  windows,
  onWindows,
  onSelect,
  onSave,
}: {
  state: State;
  order: Order;
  windows: DockWindow[];
  onWindows: (windows: DockWindow[]) => void;
  onSelect: (id: string) => void | Promise<void>;
  onSave: (state: State) => Promise<void>;
  onNotice: NoticeSink;
}) {
  const [open, setOpen] = useState(false),
    [search, setSearch] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const chats = windows.filter((window) => window.kind === "CHAT" && window.title.trim());
  // Guard stale async answers when a chat closes or the dealer list changes.
  const fingerprint = JSON.stringify([state.clients, state.settings.kakaoTitleMappings, windows, search]);
  const [resolved, setResolved] = useState<{ key: string; result: NativeKakaoManualCandidates }>();
  const visible = resolved?.key === fingerprint ? resolved.result : undefined;
  useEffect(() => {
    if (!open) return;
    let active = true;
    void nativeKakaoManualCandidates(state, windows, search)
      .then((result) => {
        if (active) {
          setResolved({ key: fingerprint, result });
          setError("");
        }
      })
      .catch((cause) => {
        if (active) {
          setResolved(undefined);
          setError(String(cause));
        }
      });
    return () => { active = false; };
  }, [open, fingerprint]);
  const refresh = async () => {
    try {
      onWindows(await window.bando!.dockWindows());
      setError("");
    } catch (e) {
      setError(String(e));
    }
  };
  const choose = async (client: Client, chat?: DockWindow) => {
    if (busy) return;
    setBusy(true);
    try {
      if (chat)
        await onSave(
          await nativeRememberTitle(
            state,
            windows,
            chat.handle,
            client.id,
            state.settings.operatorName || "직원",
          ),
        );
      await onSelect(client.id);
      setOpen(false);
      setError("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  if (
    !order.raw.trim() ||
    order.clientId ||
    order.confirmedAt ||
    state.orders.some((o) => o.id === order.id) ||
    (!chats.length && !open)
  )
    return null;
  return (
    <>
      <button
        className="kakao-client-recommend"
        onClick={() => {
          setOpen(true);
          void refresh();
        }}
      >
        카카오톡 거래처 추천
      </button>
      {open && (
        <Modal
          title="카카오톡 대화창 제목 / 거래처 연결"
          onClose={() => setOpen(false)}
          busy={busy}
        >
          <div className="title-client-results">
            {chats.map((chat) => {
              const info = visible?.chats.find((entry) => entry.handle === chat.handle);
              const remembered = state.clients.find((client) => client.id === info?.rememberedClientId);
              const rows = (info?.candidates ?? []).flatMap(({ clientId }) => {
                const client = state.clients.find((candidate) => candidate.id === clientId);
                return client ? [client] : [];
              });
              const duplicate = info?.duplicate === true;
              return (
                <article className="title-client-row" key={chat.handle}>
                  <h3>현재 대화창: {chat.title}</h3>
                  {duplicate && <p>같은 제목의 창이 여러 개입니다. 직원이 확인하여 선택하세요.</p>}
                  {rows.length ? (
                    rows.map((client) => (
                      <button
                        key={client.id}
                        disabled={busy}
                        onClick={() => void choose(client, chat)}
                      >
                        {client.name}
                        {client.id === remembered?.id ? " / 기억된 거래처" : " / 선택"}
                      </button>
                    ))
                  ) : (
                    <p>{!visible && !error ? "거래처 후보 확인 중" : "일치 후보가 없습니다. 거래처를 직접 검색하세요."}</p>
                  )}
                </article>
              );
            })}
            {!chats.length && (
              <p>현재 대화창 제목을 확인할 수 없습니다. 대화창을 열고 새로고침하세요.</p>
            )}
          </div>
          <label className="field">
            <span>다른 거래처 직접 검색</span>
            <input
              aria-label="대화창 거래처 직접 검색"
              placeholder="거래처명 / 별칭 검색"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>
          {!!search && (
            <div className="title-client-search-results">
              {(visible?.search ?? []).flatMap(({ clientId }) => {
                const client = state.clients.find((candidate) => candidate.id === clientId);
                return client ? [client] : [];
              }).map((client) => (
                  <button
                    key={client.id}
                    onClick={() => void choose(client, chats.length === 1 ? chats[0] : undefined)}
                  >
                    {client.name} / 선택
                  </button>
                ))}
            </div>
          )}
          <div className="source-inline-status" role="status">
            {error}
          </div>
          <button onClick={() => void refresh()}>대화창 새로고침</button>
          <small>
            기억된 거래처는 자동 적용합니다. 대화창 제목만 사용하며 메시지 내용은 읽지 않습니다.
            직접 선택한 거래처는 자동 변경하지 않습니다.
          </small>
        </Modal>
      )}
    </>
  );
}
