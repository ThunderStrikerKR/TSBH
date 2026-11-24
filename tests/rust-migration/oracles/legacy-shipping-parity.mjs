/**
 * Frozen JavaScript mirror of legacy-shipping.ts combinationVersions().
 * Kept as an ESM oracle because tsx/esm dynamic resolution from the isolated
 * tariff test runner resolves this nested TS path against its eval module.
 */
export function combinationVersions(key, state) {
  const versions = {};
  for (const part of key.split("|").slice(1)) {
    const pid = part.split("@")[0];
    const product = state.products.find((item) => item.id === pid);
    const group = state.shipping?.groups.find(
      (item) => item.id === product?.shippingGroupId && !item.deletedAt,
    );
    versions["assignment:" + pid] = group?.id ?? "individual";
    versions[group ? "group:" + group.id : "product:" + pid] =
      group?.current.version ??
      state.shipping?.productRules?.[pid]?.version ??
      "unregistered";
  }
  return versions;
}
