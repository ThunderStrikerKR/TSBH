/**
 * The approved remote-area feature adds fresh Rust-only metadata to old
 * TypeScript parity outputs. Ignore ONLY the newly attached, unchecked,
 * non-manual legacy marker; compare every pre-existing business field.
 *
 * A checked/manual/non-legacy marker, invalid address or unknown metadata
 * remains a parity mismatch. The new remote-area behavior has independent
 * dedicated tests; this adapter must never rewrite the original TS oracle.
 */
const knownKeys = new Set([
  "address", "checked", "manual", "legacy", "status", "suggestedChecked",
  "requiresReview", "matchedPath", "reason", "sources", "dataVersion",
]);
function isAddedLegacyMarker(value, parent) {
  return value && typeof value === "object" && !Array.isArray(value)
    && value.checked === false && value.manual === false
    && value.legacy === true
    && (value.address === undefined ||
        (typeof value.address === "string" && value.address.trim().length > 0 &&
          (typeof parent.address !== "string" || value.address === parent.address)))
    && Object.keys(value).every((key) => knownKeys.has(key));
}

/** Retain all ordinary TS-versus-Rust differences, including newer rules. */
export function projectHistoricalRemoteDelivery(expected, actual) {
  if (Array.isArray(expected) && Array.isArray(actual)) {
    return actual.map((item, i) =>
      projectHistoricalRemoteDelivery(expected[i], item));
  }
  if (!expected || !actual || typeof expected !== "object" ||
      typeof actual !== "object" || Array.isArray(expected) ||
      Array.isArray(actual)) return actual;
  const projected = {...actual};
  if (!Object.hasOwn(expected, "remoteDelivery") &&
      Object.hasOwn(actual, "remoteDelivery") &&
      isAddedLegacyMarker(actual.remoteDelivery, actual)) {
    delete projected.remoteDelivery;
  }
  for (const key of Object.keys(projected)) {
    if (Object.hasOwn(expected, key)) {
      projected[key] = projectHistoricalRemoteDelivery(
        expected[key], projected[key]);
    }
  }
  return projected;
}
