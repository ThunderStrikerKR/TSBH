import { useEffect, useRef } from "react";
import { processWatchedFactoryReply } from "./factory-reply-auto";
import type { NewHubNotification } from "./app/notification-center";

const SCAN_INTERVAL_MS = 5_000;

/** Mounted outside individual pages so changing tabs does not stop reception.
 * A saved reception draft is never replaced: while the UI has unsaved changes
 * or another save is in progress, the watcher simply postpones DB commits.
 */
export function FactoryReplyWatcher({
  enabled, paused, onRefresh, onNotification,
}: {
  enabled: boolean;
  paused: boolean;
  onRefresh: () => Promise<void>;
  onNotification: (notice: NewHubNotification) => void;
}) {
  const latest = useRef({ enabled, paused, onRefresh, onNotification });
  latest.current = { enabled, paused, onRefresh, onNotification };
  const running = useRef(false);
  const lastError = useRef("");
  const rejected = useRef(new Set<string>());

  useEffect(() => {
    const tick = async () => {
      const api = window.bando;
      if (running.current || !api?.factoryReplyWatchScan
          || !latest.current.enabled || latest.current.paused) return;
      running.current = true;
      try {
        const files = await api.factoryReplyWatchScan();
        for (const file of files) {
          if (latest.current.paused || !latest.current.enabled) break;
          // Reject each unchanged malformed file only once per running session.
          if (rejected.current.has(file.id)) continue;
          const result = await processWatchedFactoryReply(api, file);
          if (result.invalid) {
            rejected.current.add(file.id);
            lastError.current = "";
            latest.current.onNotification({
              source: "공장 회신", tone: "error", filename: file.filename,
              title: file.filename + " · 회신 확인 필요",
              detail: "자동 반영하지 않았습니다. DB는 변경하지 않았습니다.\n" +
                (result.reason ?? "파일 내용을 확인하세요."),
            });
            continue;
          }
          if (result.duplicate) continue;
          await latest.current.onRefresh();
          lastError.current = "";
          latest.current.onNotification({
            source: "공장 회신",
            tone: result.held ? "info" : "success",
            filename: file.filename,
            title: file.filename + (result.held ? " · 직원 확인 필요" : " · 반영 완료"),
            detail: "자동 반영 " + result.applied + "건 / 보류 " + result.held + "건" +
              (result.archiveWarning ? "\n파일 보관 경고: " + result.archiveWarning : ""),
            issues: result.issues,
          });
        }
      } catch (error) {
        const message = String(error);
        if (message !== lastError.current) {
          lastError.current = message;
          latest.current.onNotification({
            source: "공장 회신", tone: "error",
            title: "공장 회신 자동 감시 확인 필요",
            detail: message,
          });
        }
      } finally {
        running.current = false;
      }
    };
    void tick();
    const timer = setInterval(() => void tick(), SCAN_INTERVAL_MS);
    return () => clearInterval(timer);
  }, []);
  return null;
}
