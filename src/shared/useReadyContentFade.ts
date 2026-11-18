import { useEffect, useState } from "react";

export type ContentFadePhase = "pending" | "ready";

/**
 * Fade only replaceable content; callers keep titles, borders, empty-state
 * guidance, scrolling containers and action toolbars permanently mounted.
 * The fade-in is scheduled after the CURRENT async projection is ready, not
 * after an arbitrary loading timeout.
 *
 * retainWhileRefreshing is for edit surfaces retaining a valid last-settled
 * projection during local changes. It never carries visibility across keys.
 */
export function useReadyContentFade(
  contentKey: string,
  ready: boolean,
  retainWhileRefreshing = false,
): ContentFadePhase {
  const [revealedKey, setRevealedKey] = useState<string>();
  useEffect(() => {
    if (!ready) return;
    const frame = requestAnimationFrame(() => setRevealedKey(contentKey));
    return () => cancelAnimationFrame(frame);
  }, [contentKey, ready]);
  return revealedKey === contentKey && (ready || retainWhileRefreshing)
    ? "ready" : "pending";
}
