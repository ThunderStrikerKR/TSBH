/**
 * Read-only text positions for the Rust-generated payment-copy preview.
 * Never chooses the bank account or calculates totals. Rust supplies the
 * selected account; VAT and the exact printed total are located in
 * the rendered message so custom templates remain untouched.
 */
export type PaymentHighlightRange = {
  start: number;
  end: number;
  kind: "total" | "vat" | "account";
};

/** Read the selected account already embedded by Rust in the canonical
 * payment copy signature. This parses output metadata; it never selects a
 * business account or changes the original TS-to-Rust response contract. */
export type SelectedPaymentAccount = Readonly<{
  bank: string;
  number: string;
  holder: string;
}>;
export function selectedAccountFromSignature(signature: string): SelectedPaymentAccount | null {
  try {
    const payload: unknown = JSON.parse(signature);
    if (!payload || typeof payload !== "object") return null;
    const account = (payload as { account?: unknown }).account;
    if (!account || typeof account !== "object") return null;
    const { bank, number, holder } = account as {
      bank?: unknown; number?: unknown; holder?: unknown;
    };
    if (typeof bank !== "string" || !bank.trim() ||
        typeof number !== "string" || !number.trim() ||
        typeof holder !== "string" || !holder.trim()) return null;
    return { bank: bank.trim(), number: number.trim(), holder: holder.trim() };
  } catch {
    return null;
  }
}

export function paymentMessageHighlights(
  text: string,
  selectedAccount: SelectedPaymentAccount | null,
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
  // The complete bank / number / holder is already selected by Rust.
  // Highlight all three as ONE span, including template separators, but
  // excluding sentence endings such as " 입니다.". Only seek the same
  // account line AFTER the total, not matching earlier product/contact text.
  if (selectedAccount) {
    let from = vat && vat.index !== undefined
      ? vat.index + vat[0].length : 0;
    while (from < text.length) {
      const newline = text.indexOf("\n", from);
      const to = newline < 0 ? text.length : newline;
      const line = text.slice(from, to);
      const parts = [
        { value: selectedAccount.bank, at: line.indexOf(selectedAccount.bank) },
        { value: selectedAccount.number, at: line.indexOf(selectedAccount.number) },
        { value: selectedAccount.holder, at: line.indexOf(selectedAccount.holder) },
      ];
      if (parts.every(({ at }) => at >= 0)) {
        const start = from + Math.min(...parts.map(({ at }) => at));
        const end = from + Math.max(...parts.map(({ value, at }) => at + value.length));
        result.push({ start, end, kind: "account" });
        break;
      }
      if (newline < 0) break;
      from = newline + 1;
    }
  }
  return result.sort((a, b) => a.start - b.start);
}
