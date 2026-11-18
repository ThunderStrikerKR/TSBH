import { useCallback, useEffect, useRef, useState } from "react";
import { addHubNotification, isActionRequiredNotification, type HubNotification, type NewHubNotification } from "./notification-center";

/**
 * Sequence all inbox operations against the dedicated SQLite table.
 *
 * Startup loading, automatic reply scans, user clicks, and a clear action can
 * overlap. Serializing native requests prevents an older load from
 * overwriting a newer read/add result in React. No business State revision is
 * touched by notification read/clear.
 */
export function usePersistedNotifications(ready: boolean) {
  const [inbox, setInbox] = useState<HubNotification[]>([]);
  const [hiddenCount, setHiddenCount] = useState(0);
  const [storageError, setStorageError] = useState("");
  const chain = useRef<Promise<void>>(Promise.resolve());
  const mounted = useRef(true);

  const enqueue = useCallback((op: () => Promise<HubNotification[]>) => {
    chain.current = chain.current
      .then(async () => {
        const notices = await op();
        if (!Array.isArray(notices)) throw Error("알림 저장소 응답이 올바르지 않습니다.");
        if (mounted.current) {
          // Older informational/success notifications remain deletable from
          // SQLite but never count as urgent, even after app restart.
          setInbox(notices.filter(isActionRequiredNotification));
          setHiddenCount(notices.filter((notice) => !isActionRequiredNotification(notice)).length);
          setStorageError("");
        }
      })
      .catch((cause: unknown) => {
        if (mounted.current) {
          setStorageError("알림 데이터베이스 처리 실패: " +
            (cause instanceof Error ? cause.message : String(cause)));
        }
      });
    return chain.current;
  }, []);

  useEffect(() => {
    if (!ready) return;
    mounted.current = true;
    void enqueue(() => window.bando!.notificationList());
    return () => { mounted.current = false; };
  }, [ready, enqueue]);

  const add = useCallback((incoming: NewHubNotification) => {
    // The database may have notifications from the previous app run.
    // Generate the identity once and send the same immutable event to Rust.
    const notice = addHubNotification([], incoming)[0];
    return enqueue(() => window.bando!.notificationAdd(notice));
  }, [enqueue]);

  const markRead = useCallback((id: string) =>
    enqueue(() => window.bando!.notificationMarkRead(id)), [enqueue]);

  const remove = useCallback((id: string) =>
    enqueue(() => window.bando!.notificationDelete(id)), [enqueue]);

  const clear = useCallback(() =>
    enqueue(() => window.bando!.notificationClear()), [enqueue]);

  return { inbox, hiddenCount, storageError, add, markRead, remove, clear };
}
