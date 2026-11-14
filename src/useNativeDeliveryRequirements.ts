/** Native Rust recipient eligibility; never calculate live delivery rules in JS. */
import { useEffect, useMemo, useState } from "react";
import { businessCoreCall } from "./tauri-bridge";
import type { Recipient } from "./types";

export type NativeDeliveryRequirement = {
  recipientId: string;
  name: boolean;
  address: boolean;
  contact: boolean;
  product: boolean;
  quantity: boolean;
};

export async function evaluateNativeDeliveryRequirements(
  rows: Recipient[], allowLegacyAddressConfirmation: boolean,
): Promise<NativeDeliveryRequirement[]> {
  const result = await businessCoreCall<{results: NativeDeliveryRequirement[]}>(
    "rules", "delivery_requirements", { rows, allowLegacyAddressConfirmation },
  );
  return result.results;
}

export function useNativeDeliveryRequirements(
  rows: Recipient[], allowLegacyAddressConfirmation: boolean,
) {
  const fingerprint = useMemo(
    () => JSON.stringify([allowLegacyAddressConfirmation, rows]),
    [rows, allowLegacyAddressConfirmation],
  );
  const [resolved, setResolved] = useState<{
    fingerprint: string; entries: NativeDeliveryRequirement[];
  }>();
  const [failure, setFailure] = useState<{
    fingerprint: string; reason: string;
  }>();
  useEffect(() => {
    let active = true;
    void evaluateNativeDeliveryRequirements(rows, allowLegacyAddressConfirmation)
      .then((entries) => {
        if (!active) return;
        setResolved({ fingerprint, entries });
        setFailure(undefined);
      }).catch((error) => {
        if (!active) return;
        setResolved(undefined);
        setFailure({ fingerprint, reason: String(error) });
      });
    return () => { active = false; };
  }, [fingerprint]);
  const ready = resolved?.fingerprint === fingerprint;
  return {
    ready,
    byId: Object.fromEntries(
      (ready ? resolved.entries : []).map((entry) => [entry.recipientId, entry]),
    ) as Record<string, NativeDeliveryRequirement>,
    error: failure?.fingerprint === fingerprint ? failure.reason : "",
  };
}
