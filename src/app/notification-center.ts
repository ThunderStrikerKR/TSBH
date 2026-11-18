import type { NoticeTone } from "../shared/notices";

/** The inbox is reserved for explicitly declared critical/action-blocking
 * application problems. Routine workflow outcomes and factory reply history
 * belong to their own screens, never to this inbox. */
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
  /** Legacy notices omit this field and must not reappear as active alerts. */
  priority?: "critical";
}

/** New alerts require explicit classification; tone="error" alone is not enough. */
export type NewHubNotification =
  Omit<HubNotification, "id" | "at" | "read" | "priority"> & { priority: "critical" };

export function isActionRequiredNotification(
  notice: Pick<HubNotification, "priority">,
): boolean {
  return notice.priority === "critical";
}

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
