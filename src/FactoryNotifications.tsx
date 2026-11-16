import { businessCoreCall } from "./tauri-bridge";
import { nativeFactoryConflictConfirmation } from "./native-factory-conflict-review";
import { confirmAction } from "./dialogs";
import { useEffect, useState } from "react";
import type { State } from "./types";
import type { HubNotification } from "./app/notification-center";
import { Modal } from "./ui";
import { won } from "./domain";
export function FactoryNotifications({
  state,
  notices,
  onRead,
  onClear,
  onClose,
  onOpen,
  onRefresh,
}: {
  state: State;
  notices: HubNotification[];
  onRead: (id: string) => void;
  onClear: () => void;
  onClose: () => void;
  onOpen: (id: string) => void;
  onRefresh: () => Promise<void>;
}) {
  const [error, setError] = useState("");
  const [selectedId, setSelectedId] = useState<string>();
  const selected = notices.find((notice) => notice.id === selectedId);
  const [nativeReview, setNativeReview] = useState<{
    state: State;
    statuses: Record<string, { needsReview: boolean; resolvedRows: { row: number; resolved: boolean }[] }>;
  }>();
  useEffect(() => {
    let active = true;
    void businessCoreCall<{ reviews: { id: string; needsReview: boolean; resolvedRows: { row: number; resolved: boolean }[] }[] }>(
      "rules", "factory_reply_review_statuses", { state },
    ).then(({ reviews }) => {
      if (active) setNativeReview({
        state, statuses: Object.fromEntries(reviews.map((row) => [row.id, row])),
      });
    }).catch((cause) => {
      if (active) setError("Rust 회신 상태 확인 실패: " +
        (cause instanceof Error ? cause.message : String(cause)));
    });
    return () => { active = false; };
  }, [state]);
  // Never display an obsolete revision's review result as 'completed'.
  const statuses = nativeReview?.state === state ? nativeReview.statuses : undefined;
  return (
    <Modal title="알림" onClose={onClose}>
      {error && <p role="alert" className="form-error">{error}</p>}
      <section className="notification-center" aria-label="알림 목록">
        <header className="notification-center-heading">
          <h3>최근 알림 <small>{notices.filter((entry) => !entry.read).length}건 미확인</small></h3>
          <button type="button" disabled={!notices.length} onClick={() => {
            setSelectedId(undefined);
            onClear();
          }}>목록 비우기</button>
        </header>
        {!notices.length && (
          <p className="muted">새 알림이 없습니다. 저장된 공장 회신은 아래 이력에서 확인할 수 있습니다.</p>
        )}
        {notices.length > 0 && (
          <div className="notification-center-layout">
            <div className="notification-center-list" role="list">
              {notices.map((entry) => (
                <div role="listitem" key={entry.id}>
                  <button type="button"
                    className={"notification-center-item" +
                      (entry.read ? "" : " unread")}
                    aria-pressed={entry.id === selectedId}
                    onClick={() => {
                      setSelectedId(entry.id);
                      onRead(entry.id);
                    }}>
                    <strong>{entry.title}</strong>
                    <small>{entry.source} · {new Date(entry.at).toLocaleString("ko-KR")}</small>
                  </button>
                </div>
              ))}
            </div>
            <div className="notification-center-detail" aria-live="polite">
              {selected ? (
                <article>
                  <h4>{selected.title}</h4>
                  <p className="muted">{selected.filename || selected.source} · {" "}
                    {new Date(selected.at).toLocaleString("ko-KR")}</p>
                  <p className="notification-detail-message">{selected.detail}</p>
                  {!!selected.issues?.length && (
                    <div>
                      <h4>확인이 필요한 배송 행 {selected.issues.length}건</h4>
                      <ul className="notification-detail-issues">
                        {selected.issues.map((issue) => (
                          <li key={issue.row}>
                            <strong>{issue.row}행 · {issue.name || "수하인 미확인"}</strong>
                            <span>{issue.issue}</span>
                            {issue.tracking && <small>운송장: {issue.tracking}</small>}
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </article>
              ) : <p className="muted">왼쪽 알림을 선택하면 자세한 내용을 볼 수 있습니다.</p>}
            </div>
          </div>
        )}
      </section>
      <h3 className="notification-center-history-title">공장 회신 이력</h3>
      <div className="factory-notifications">
        {!state.factoryReplies?.length && (
          <p>공장 회신 내역이 없습니다. 공장 목록의 엑셀 작성 이력에서 회신 파일을 업로드하세요.</p>
        )}
        {state.factoryReplies
          ?.slice()
          .reverse()
          .map((record) => (
            <article key={record.id}>
              <h3>
                {record.filename} / {" "}
                {statuses?.[record.id]?.needsReview === false
                  ? "반영 완료"
                  : statuses?.[record.id]?.needsReview === true
                    ? record.status === "PARTIAL" ? "일부 반영" : "직원 확인 필요"
                    : "확인 중"}
              </h3>
              <small>
                {record.businessDate} / {new Date(record.at).toLocaleString("ko-KR")}
              </small>
              {record.archivedPath && record.id.startsWith("factory-reply-") && (
                <button type="button"
                  onClick={() =>
                    void window.bando!.openFactoryFile!(record.id).catch((e) => setError(String(e)))
                  }
                >
                  회신 원본 열기
                </button>
              )}
              {record.error && <p className="form-error">{record.error}</p>}
              {record.rows.map((row) => (
                <div key={row.row}>
                  <p>
                    {row.row}행 / {row.name} / {" "}
                    {row.applied
                      ? `${row.tracking} / 실제 운임 ${won(row.actual)}`
                      : row.issue || "직원 확인 필요"}
                  </p>
                  {!row.applied &&
                    row.shipmentId &&
                    row.issue?.includes("충돌") &&
                    statuses?.[record.id]?.resolvedRows.find(
                      (r) => r.row === row.row,
                    )?.resolved === false && (
                      <button
                        onClick={async () => {
                          try {
                            if (!(await confirmAction(
                              await nativeFactoryConflictConfirmation(state, record.id, row.row),
                            ))) return;
                            if (!window.bando?.approveFactoryConflict) {
                              throw Error("공장 회신 충돌 승인 기능을 사용할 수 없습니다.");
                            }
                            try {
                              await window.bando.approveFactoryConflict(
                                record.id, row.row, state.revision ?? 0,
                              );
                            } finally {
                              await onRefresh();
                            }
                          } catch (e) {
                            setError(String(e));
                          }
                        }}
                      >
                        충돌 값 확인 후 정정
                      </button>
                    )}
                  {row.orderId && (
                    <button
                      onClick={() => {
                        onClose();
                        onOpen(row.orderId!);
                      }}
                    >
                      해당 주문 보기
                    </button>
                  )}
                </div>
              ))}
            </article>
          ))}
      </div>
    </Modal>
  );
}
