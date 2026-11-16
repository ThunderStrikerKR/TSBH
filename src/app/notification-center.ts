import type { NoticeTone } from "../shared/notices";

/** Screen notifications are user-visible, not temporary toast overlays.
 * Factory replies saved to SQLite remain independently available in history. */
export interface HubNotification {
  id: string;
  at: string;
  tone: NoticeTone;
  source: "공장 회신" | "시스템";
  title: string;
  detail: string;
  filename?: string;
  issues?: Array<{ row: number; name: string; issue: string; tracking?: string }>;
  read: boolean;
}

export type NewHubNotification = Omit<HubNotification, "id" | "at" | "read">;

export function addHubNotification(
  previous: HubNotification[], incoming: NewHubNotification,
): HubNotification[] {
  // Keep the most recent 100 in-memory events. Repeated identical scans do
  // not displace prior notifications from unrelated files.
  if (previous[0] && previous[0].title === incoming.title &&
    previous[0].detail === incoming.detail &&
    previous[0].filename === incoming.filename) return previous;
  return [{
    ...incoming,
    id: crypto.randomUUID(),
    at: new Date().toISOString(),
    read: false,
  }, ...previous].slice(0, 100);
}
