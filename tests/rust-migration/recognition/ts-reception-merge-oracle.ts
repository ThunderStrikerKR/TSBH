/** Original TypeScript reception merge kept solely for TS↔Rust differential tests.
 * This module is not imported from any production React execution path.
 */
import { matchingRecipient, rebindEvidence } from "../../../src/order-evidence.ts";
import { addressParts, mergeParsedItem } from "../../../src/parser.ts";
import { assertRustReceptionAnalysis, receptionAnalysisAllowed, type RustReceptionAnalysis } from "../../../src/orders/rust-reception.ts";
import type { Client, Order, State } from "../../../src/types.ts";

/**
 * Preserve the established reception behavior AFTER Rust has recognized the
 * original document. This function does not parse products, people or addresses.
 * It keeps stable employee-owned IDs, explicit approvals and frozen evidence.
 */
export function applyRustReceptionAnalysis(
  previous: Order,
  evidence: RustReceptionAnalysis,
  options: {
    raw: string;
    state: State;
    pricingClient: Client;
    at: string;
  },
): Order {
  const { raw, pricingClient, at } = options;
  assertRustReceptionAnalysis(evidence, raw);
  if (!receptionAnalysisAllowed(previous, raw, pricingClient.id)) return previous;

  // Native results are re-bound to existing employee-owned entities below.
  // Do not mutate the native response or the currently displayed draft.
  const parsed = structuredClone(evidence.recipients);
  const candidates = structuredClone(evidence.candidates);
  const originalParsed = structuredClone(parsed);
  const usedRecipients = new Set<string>();

  for (const recipient of parsed) {
    const prior = matchingRecipient(recipient, previous.recipients, usedRecipients, parsed);
    if (!prior) continue;
    usedRecipients.add(prior.id);
    const provisionalId = recipient.id;
    rebindEvidence(candidates, "recipientId", provisionalId, prior.id);
    recipient.id = prior.id;
    for (const span of recipient.sources ?? []) {
      span.targetId = prior.id;
      span.recipientId = prior.id;
    }

    for (const field of prior.confirmedFields ?? []) {
      if (["name", "address", "phone", "mobile", "note"].includes(field)) {
        (recipient as unknown as Record<string, unknown>)[field] =
          (prior as unknown as Record<string, unknown>)[field];
      }
    }
    recipient.confirmedFields = prior.confirmedFields;
    recipient.dispatchOrigin = prior.dispatchOrigin;
    recipient.actualFreight = prior.actualFreight;
    recipient.freightCode = prior.freightCode;
    recipient.vatDefault = prior.vatDefault;
    recipient.vatOverride = prior.vatOverride;
    recipient.shippingCharges = prior.shippingCharges;
    if (prior.confirmedFields?.includes("address")) {
      recipient.addressStatus = prior.addressStatus;
      recipient.addressParts = prior.addressParts ?? addressParts(prior.address);
    }

    const usedItems = new Set<string>();
    for (const item of recipient.items) {
      const priorItem = prior.items.find((candidate) =>
        candidate.raw === item.raw && !usedItems.has(candidate.id));
      if (priorItem) {
        usedItems.add(priorItem.id);
        rebindEvidence(candidates, "itemId", item.id, priorItem.id);
        item.id = priorItem.id;
        Object.assign(item, mergeParsedItem(item, priorItem));
        // The asynchronous native Rust reprice_order effect is authoritative
        // for previously recognized and employee-matched catalog items.
        if (item.variantId) item.unitPrice = null;
      }
      // Even newly recognized items must reference the retained recipient,
      // not Rust's provisional recipient ID.
      for (const span of item.sources ?? []) {
        span.targetId = item.id;
        span.recipientId = recipient.id;
      }
    }
  }

  if (previous.orderType) {
    for (const recipient of parsed) {
      recipient.dispatchOrigin ??= "FACTORY";
      for (const item of recipient.items) item.origin = recipient.dispatchOrigin;
    }
  }
  const next: Order = {
    ...previous,
    analysisEvidence: candidates,
    analysisSnapshot: previous.analysisSnapshot?.raw === raw
      ? previous.analysisSnapshot
      : {
        at,
        raw,
        ruleVersion: "rust-recognition-v1",
        recipients: originalParsed,
      },
    raw,
    analysisRaw: raw,
    analysisClientId: pricingClient.id,
    recipients: parsed,
    pricesLocked: false,
    notified: false,
    paid: false,
    postpaidApproved: false,
    status: "REVIEW",
  };
  // The live reception hook resolves freight with Rust calculate_shipping.
  // Keep an explicit pending marker so Save also awaits native freight.
  next.shippingFee = null;
  next.shippingMode = "AUTO";
  next.shippingVersion = "";
  return next;
}
