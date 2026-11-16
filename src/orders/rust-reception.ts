import type { OrderEvidence } from "../order-evidence";
import type { Order, Recipient } from "../types";

/** Exact result envelope returned by recognition::execute("analyze_order"). */
export interface RustReceptionAnalysis {
  recipients: Recipient[];
  candidates: OrderEvidence[];
  unassigned: string[];
}

function sourceSpanIsValid(
  raw: string,
  source: { start: number; end: number; text: string },
): boolean {
  return Number.isInteger(source.start) && Number.isInteger(source.end) &&
    source.start >= 0 && source.end >= source.start && source.end <= raw.length &&
    typeof source.text === "string" && raw.slice(source.start, source.end) === source.text;
}

/** Never apply a partial or malformed native response to an editable order. */
export function assertRustReceptionAnalysis(
  value: unknown,
  raw: string,
): asserts value is RustReceptionAnalysis {
  if (!value || typeof value !== "object") throw Error("Rust 발주 분석 결과가 없습니다.");
  const result = value as Partial<RustReceptionAnalysis>;
  if (!Array.isArray(result.recipients) ||
      !Array.isArray(result.candidates) ||
      !Array.isArray(result.unassigned))
    throw Error("Rust 발주 분석 결과의 배송건 또는 근거 형식이 올바르지 않습니다.");
  for (const candidate of result.candidates) {
    if (!candidate || typeof candidate.id !== "string" ||
        typeof candidate.kind !== "string" ||
        !sourceSpanIsValid(raw, candidate))
      throw Error("Rust 발주 분석의 원문 근거 위치가 일치하지 않습니다.");
  }
  for (const recipient of result.recipients) {
    if (!recipient || typeof recipient.id !== "string" ||
        !Array.isArray(recipient.items) ||
        !Array.isArray(recipient.sources) ||
        typeof recipient.name !== "string" ||
        typeof recipient.address !== "string")
      throw Error("Rust 발주 분석의 수취인 자료가 올바르지 않습니다.");
    for (const span of recipient.sources) {
      if (!sourceSpanIsValid(raw, span))
        throw Error("Rust 수취인 근거가 발주 원문과 일치하지 않습니다.");
    }
    for (const item of recipient.items) {
      if (!item || typeof item.id !== "string" ||
          typeof item.raw !== "string" ||
          typeof item.name !== "string" ||
          !Array.isArray(item.issues) ||
          !Array.isArray(item.sources) ||
          (item.quantity !== null &&
            (!Number.isSafeInteger(item.quantity) || item.quantity <= 0)) ||
          typeof item.quantityConfirmed !== "boolean" ||
          (item.productId !== null && typeof item.productId !== "string") ||
          (item.variantId !== null && typeof item.variantId !== "string"))
        throw Error("Rust 발주 분석의 상품 또는 수량 자료가 올바르지 않습니다.");
      for (const span of item.sources) {
        if (!sourceSpanIsValid(raw, span))
          throw Error("Rust 상품 근거가 발주 원문과 일치하지 않습니다.");
      }
    }
  }
}

export function receptionAnalysisAllowed(
  previous: Order,
  raw: string,
  clientId: string,
): boolean {
  // Match the old TypeScript: a manual draft blocks reanalysis only for unchanged text.
  if (previous.deletedAt || previous.confirmedAt ||
      (previous.manualClassification && previous.analysisRaw === raw) ||
      previous.pricesLocked || previous.status === "EXPORTED")
    return false;
  return previous.analysisRaw !== raw || previous.analysisClientId !== clientId;
}

// The original TS reception merge now exists only in the test oracle.
// Live recognition applies the compiled Rust rules/merge_reception_analysis result.
