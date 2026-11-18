import { useEffect, useRef } from "react";
import { processWatchedFactoryReply } from "./factory-reply-auto";

const SCAN_INTERVAL_MS = 5_000;

/** Preserve the existing optional factory-file import, but do not publish
 * watcher scans, file arrivals or routine import outcomes as user alerts.
 * Factory reply records remain available in the Factory Queue history.
 * The future factory app can explicitly introduce delivery notifications. */
export function FactoryReplyWatcher({
  enabled, paused, onRefresh,
}: {
  enabled: boolean;
  paused: boolean;
  onRefresh: () => Promise<void>;
}) {
  const latest = useRef({ enabled, paused, onRefresh });
  latest.current = { enabled, paused, onRefresh };
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
          if (rejected.current.has(file.id)) continue;
          const result = await processWatchedFactoryReply(api, file);
          if (result.invalid) {
            // Do not loop over the same invalid file indefinitely. Its issue
            // can be handled from the explicit factory import workflow.
            rejected.current.add(file.id);
            lastError.current = "";
            console.warn("공장 회신 파일 자동 처리 제외:", file.filename, result.reason);
            continue;
          }
          if (result.duplicate) continue;
          await latest.current.onRefresh();
          lastError.current = "";
          // No file-arrival, held-row or success notification in this phase.
        }
      } catch (error) {
        const message = String(error);
        if (message !== lastError.current) {
          lastError.current = message;
          console.error("공장 회신 자동 감시 오류:", message);
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
