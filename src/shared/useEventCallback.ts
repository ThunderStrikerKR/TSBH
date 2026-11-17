import { useCallback, useRef } from "react";

/** Stable callback identity that always executes the latest render's logic.
 * Does not freeze order snapshots or ignore user edits.
 */
export function useEventCallback<Args extends unknown[], Result>(
  callback: (...args: Args) => Result,
): (...args: Args) => Result {
  const latest = useRef(callback);
  latest.current = callback;
  return useCallback((...args: Args) => latest.current(...args), []);
}
