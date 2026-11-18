/**
 * Read-only text positions for the Rust-generated payment-copy preview.
 * Never chooses the bank account or calculates totals. Rust supplies the
 * selected account number; VAT and the exact printed total are located in
 * the rendered message so custom templates remain untouched.
 */
export type PaymentHighlightRange = {
  start: number;
  end: number;
  kind: "total" | "vat" | "account";
};

export function paymentMessageHighlights(
  text: string,
  selectedAccountNumber: string,
): PaymentHighlightRange[] {
  const result: PaymentHighlightRange[] = [];
  if (!text) return result;
  const vatMatches = [...text.matchAll(/\(부가세 (?:포함\/별도|포함|별도)\)/gu)];
  const vat = vatMatches.at(-1);
  if (vat && vat.index !== undefined) {
    const vatStart = vat.index;
    const vatEnd = vatStart + vat[0].length;
    result.push({ start: vatStart, end: vatEnd, kind: "vat" });
    // The tax label is appended to the grand-total line by the Rust core.
    // Only currency text on THAT line may be emphasized, never item prices
    // or individual shipment freight elsewhere in the message.
    const lineStart = text.lastIndexOf("\n", vatStart - 1) + 1;
    const beforeVat = text.slice(lineStart, vatStart);
    const amounts = [...beforeVat.matchAll(/\d[\d,]*원/gu)];
    const amount = amounts.at(-1);
    if (amount && amount.index !== undefined) {
      const start = lineStart + amount.index;
      result.push({ start, end: start + amount[0].length, kind: "total" });
    }
  }
  // The account number is an explicit decision of Rust's account_for().
  // Locate only that precise number in the payment section after its total.
  const number = selectedAccountNumber.trim();
  if (number) {
    const searchFrom = vat && vat.index !== undefined
      ? vat.index + vat[0].length : 0;
    const start = text.indexOf(number, searchFrom);
    if (start >= 0) result.push({
      start, end: start + number.length, kind: "account",
    });
  }
  return result.sort((a, b) => a.start - b.start);
}
