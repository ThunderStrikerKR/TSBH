import { useState } from "react";
import type { HubNotification } from "./app/notification-center";
import { isActionRequiredNotification } from "./app/notification-center";
import { Modal } from "./ui";

/** Critical-action inbox only. Factory reply history and its conflict
 * corrections live in FactoryReplyHistory under the Factory Queue tab. */
export function FactoryNotifications({
  notices,
  hiddenCount,
  storageError,
  onRead,
  onDelete,
  onClear,
  onClose,
}: {
  notices: HubNotification[];
  hiddenCount: number;
  storageError?: string;
  onRead: (id: string) => void;
  onDelete: (id: string) => void;
  onClear: () => void;
  onClose: () => void;
}) {
  const [selectedId, setSelectedId] = useState<string>();
  const critical = notices.filter(isActionRequiredNotification);
  const selected = critical.find((notice) => notice.id === selectedId);
  const unread = critical.filter((entry) => !entry.read).length;
  return (
    <Modal title="알림" onClose={onClose}>
      {storageError && <p role="alert" className="form-error">{storageError}</p>}
      <section className="notification-center" aria-label="알림 목록">
        <header className="notification-center-heading">
          <h3>최근 알림 <small>{unread}건 미확인</small></h3>
          <button type="button" disabled={!critical.length && !hiddenCount}
            onClick={() => {
              setSelectedId(undefined);
              onClear();
            }}>
            {critical.length ? "목록 비우기" : "이전 알림 정리"}
          </button>
        </header>
        {!critical.length && (
          <p className="muted">현재 표시할 알림이 없습니다.</p>
        )}
        {!critical.length && hiddenCount > 0 && (
          <p className="muted">기존 일반 알림 {hiddenCount}건은 표시하지 않으며, 위 버튼으로 삭제할 수 있습니다.</p>
        )}
        {critical.length > 0 && (
          <div className="notification-center-layout">
            <div className="notification-center-list" role="list">
              {critical.map((entry) => (
                <div role="listitem" className="notification-center-entry" key={entry.id}>
                  <button type="button"
                    className={"notification-center-item" + (entry.read ? "" : " unread")}
                    aria-pressed={entry.id === selectedId}
                    onClick={() => {
                      setSelectedId(entry.id);
                      onRead(entry.id);
                    }}>
                    <strong>{entry.title}</strong>
                    <small>{entry.source} · {new Date(entry.at).toLocaleString("ko-KR")}</small>
                  </button>
                  <button type="button" className="notification-center-delete"
                    aria-label={entry.title + " 알림 삭제"}
                    onClick={() => {
                      if (selectedId === entry.id) setSelectedId(undefined);
                      onDelete(entry.id);
                    }}>삭제</button>
                </div>
              ))}
            </div>
            <div className="notification-center-detail" aria-live="polite">
              {selected ? (
                <article>
                  <h4>{selected.title}</h4>
                  <p className="muted">{new Date(selected.at).toLocaleString("ko-KR")}</p>
                  <p className="notification-detail-message">{selected.detail}</p>
                  {!!selected.issues?.length && (
                    <ul className="notification-detail-issues">
                      {selected.issues.map((issue) => (
                        <li key={issue.row}>
                          <strong>{issue.row}행 · {issue.name || "항목 미확인"}</strong>
                          <span>{issue.issue}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </article>
              ) : <p className="muted">알림을 선택하면 자세한 내용을 확인할 수 있습니다.</p>}
            </div>
          </div>
        )}
      </section>
    </Modal>
  );
}
