/** Shared short-lived transport deduplication for deterministic compiled-Rust READS.
 * The exact serialized payload is the cache key, so a changed price, recipient,
 * settings revision, or workflow state cannot match an earlier result.
 * No command that mutates a draft, creates audit events, or writes SQLite is cached.
 */
type Domain = "recognition" | "rules" | "output" | "catalog_clients";
type ReadRecord = { promise: Promise<unknown>; settledAt: number | null };
const pureReads = new Set([
  "catalog_clients.client_candidates",
  "rules.preview_workflow",
  "rules.preview_order",
  "rules.preview_order_lines",
  "rules.order_issues_with_actions",
  "rules.delivery_requirements_with_messages",
  "rules.should_refresh_pricing",
  "rules.factory_reply_review_statuses",
]);
const entries = new Map<string, ReadRecord>();
const SETTLED_MS = 350;
const MAX_ENTRIES = 48;

export function coalescePureNativeRead<T>(
  domain: Domain,
  operation: string,
  payload: unknown,
  runNative: () => Promise<T>,
): Promise<T> {
  if (!pureReads.has(domain + "." + operation)) return runNative();
  let key: string;
  try {
    key = JSON.stringify([domain, operation, payload]);
  } catch {
    return runNative();
  }
  const existing = entries.get(key);
  if (existing) {
    if (existing.settledAt === null || Date.now() - existing.settledAt <= SETTLED_MS)
      return existing.promise as Promise<T>;
    entries.delete(key);
  }
  const promise = runNative();
  const record: ReadRecord = { promise, settledAt: null };
  entries.set(key, record);
  void promise.then(
    () => { record.settledAt = Date.now(); },
    () => { if (entries.get(key) === record) entries.delete(key); },
  );
  // Bounded memoization: cache memory cannot grow with the order history.
  while (entries.size > MAX_ENTRIES) {
    const oldest = entries.keys().next().value;
    if (oldest === undefined) break;
    entries.delete(oldest);
  }
  return promise;
}
